use super::*;

#[test]
fn desired_nofile_soft_limit_only_raises_when_possible() {
    assert_eq!(desired_nofile_soft_limit(1024, 524_288, 8192), Some(8192));
    assert_eq!(desired_nofile_soft_limit(8192, 524_288, 8192), None);
    assert_eq!(desired_nofile_soft_limit(1024, 4096, 8192), Some(4096));
}

#[cfg(target_os = "linux")]
#[test]
fn process_start_identity_requires_matching_token() {
    let pid = std::process::id();
    let token = super::process_start_token(pid).expect("current process start token");
    assert_eq!(
        super::verify_process_start_token(pid, Some(&token)),
        super::ProcessIdentityCheck::Matching
    );
    assert_eq!(
        super::verify_process_start_token(pid, Some("pid-reuse-fixture")),
        super::ProcessIdentityCheck::Mismatch
    );
    assert_eq!(
        super::verify_process_start_token(pid, None),
        super::ProcessIdentityCheck::Missing
    );
}

#[test]
fn verified_signal_fails_closed_without_identity() {
    assert_eq!(
        super::signal_verified_process_group(std::process::id(), None, 0),
        super::ProcessIdentityCheck::Missing
    );
}

#[cfg(target_os = "linux")]
#[test]
fn verified_signal_reports_signal_syscall_failure() {
    use std::process::Stdio;

    let mut command = std::process::Command::new("sleep");
    command
        .arg("60")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = super::spawn_detached(&mut command).expect("spawn isolated signal fixture");
    let pid = child.id();
    let token = super::process_start_token(pid).expect("fixture process start token");

    assert_eq!(
        super::signal_verified_process_group(pid, Some(&token), i32::MAX),
        super::ProcessIdentityCheck::SignalFailed
    );
    assert!(
        super::is_process_running(pid),
        "an invalid signal must not terminate the fixture"
    );

    super::signal_detached_process_group(pid, libc::SIGKILL).expect("clean up signal fixture");
    let _ = child.wait();
}

#[cfg(target_os = "linux")]
#[test]
fn leader_exit_keeps_verified_descendant_group_addressable() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg("sleep 60 & child=$!; echo $child; exit 0")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut leader = command.spawn().expect("spawn isolated group fixture");
    let leader_pid = leader.id();
    let leader_token = super::process_start_token(leader_pid).expect("leader token");
    let mut unrelated = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn unrelated process fixture");
    let unrelated_pid = unrelated.id();
    let member_pid = std::io::BufRead::lines(std::io::BufReader::new(
        leader.stdout.take().expect("leader stdout"),
    ))
    .next()
    .expect("member pid line")
    .expect("read member pid")
    .parse::<u32>()
    .expect("numeric member pid");
    let member_token = super::process_start_token(member_pid).expect("member token");
    let _ = leader.wait();

    let deadline = Instant::now() + Duration::from_secs(2);
    while super::is_process_running(leader_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        super::verify_process_group_identity(
            leader_pid,
            Some(&leader_token),
            Some((member_pid, &member_token)),
        ),
        super::ProcessIdentityCheck::Matching,
        "a verified descendant should retain safe group identity after leader exit"
    );
    assert_eq!(
        super::signal_verified_process_group_with_member(
            leader_pid,
            Some(&leader_token),
            Some((member_pid, &member_token)),
            libc::SIGKILL,
        ),
        super::ProcessIdentityCheck::Matching
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while super::is_process_group_live(leader_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!super::is_process_group_live(leader_pid));
    assert!(
        super::is_process_live(unrelated_pid),
        "unrelated process must survive"
    );
    let _ = unsafe { libc::kill(unrelated_pid as libc::pid_t, libc::SIGKILL) };
    let _ = unrelated.wait();
}

#[cfg(unix)]
#[test]
fn spawn_detached_creates_new_session() {
    use tempfile::NamedTempFile;

    let output = NamedTempFile::new().expect("temp file");
    let output_path = output.path().to_string_lossy().to_string();
    let parent_sid = unsafe { libc::getsid(0) };

    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c")
        .arg("ps -o sid= -p $$ > \"$JCODE_TEST_OUTPUT\"")
        .env("JCODE_TEST_OUTPUT", &output_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let mut child = super::spawn_detached(&mut cmd).expect("spawn detached child");
    let status = child.wait().expect("wait for child");
    assert!(status.success(), "child should exit successfully");

    let child_sid = std::fs::read_to_string(&output_path)
        .expect("read child sid")
        .trim()
        .parse::<u32>()
        .expect("parse child sid");

    assert_eq!(
        child_sid,
        child.id(),
        "detached child should lead its own session"
    );
    assert_ne!(
        child_sid as i32, parent_sid,
        "detached child should not share parent session"
    );
}

#[cfg(windows)]
#[test]
fn is_process_running_reports_exited_children_as_stopped() {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/C", "ping -n 3 127.0.0.1 >NUL"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("spawn child");
    let pid = child.id();
    assert!(
        super::is_process_running(pid),
        "child should initially be running"
    );

    let status = child.wait().expect("wait for child");
    assert!(status.success(), "child should exit successfully");
    std::thread::sleep(Duration::from_millis(100));

    assert!(
        !super::is_process_running(pid),
        "exited child should not be reported as running"
    );
}

#[cfg(windows)]
#[test]
fn signal_detached_process_group_terminates_descendant_tree() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let temp = tempfile::tempdir().expect("temp dir");
    let ready_path = temp.path().join("child-ready.txt");
    let survived_path = temp.path().join("child-survived.txt");
    let child_script_path = temp.path().join("child.cmd");
    let parent_script_path = temp.path().join("parent.cmd");
    let child_script = concat!(
        "@echo off\r\n",
        "echo ready>\"%~dp0child-ready.txt\"\r\n",
        "ping -n 6 127.0.0.1 >NUL\r\n",
        "echo survived>\"%~dp0child-survived.txt\"\r\n"
    );
    let parent_script = concat!(
        "@echo off\r\n",
        "start \"\" /B cmd.exe /D /C \"\"%~dp0child.cmd\"\"\r\n",
        "ping -n 30 127.0.0.1 >NUL\r\n"
    );
    std::fs::write(&child_script_path, child_script).expect("write child command script");
    std::fs::write(&parent_script_path, parent_script).expect("write parent command script");
    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/D", "/C"])
        .arg(&parent_script_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut parent = super::spawn_detached(&mut cmd).expect("spawn detached process tree");
    let parent_pid = parent.id();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready_path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(ready_path.exists(), "descendant should report ready");
    assert!(super::is_process_running(parent_pid));

    super::signal_detached_process_group(parent_pid, 0).expect("terminate process tree");
    let deadline = Instant::now() + Duration::from_secs(10);
    while super::is_process_running(parent_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = parent.wait();

    assert!(!super::is_process_running(parent_pid), "parent should stop");
    std::thread::sleep(Duration::from_secs(6));
    assert!(
        !survived_path.exists(),
        "descendant should not survive termination of the detached process tree"
    );
}

#[cfg(windows)]
#[test]
fn spawn_replacement_process_returns_without_waiting_for_child_exit() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/C", "ping -n 4 127.0.0.1 >NUL"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let start = Instant::now();
    let mut child = super::spawn_replacement_process(&mut cmd)
        .expect("spawn replacement process should succeed");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(1),
        "replacement spawn should not block, took {:?}",
        elapsed
    );
    assert!(
        child.try_wait().expect("poll child status").is_none(),
        "replacement child should still be running immediately after spawn"
    );

    child.kill().ok();
    let _ = child.wait();
}
