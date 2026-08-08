//! Offline CPU, memory, and latency profiler for common built-in tools.
//!
//! This exercises the production `Registry::execute` path, including tool
//! policy checks, lifecycle logging, hooks, telemetry, and context guards. It
//! deliberately avoids provider calls and network access.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use clap::Parser;
use jcode::provider::{EventStream, Provider};
use jcode::tool::{Registry, Tool, ToolContext, ToolExecutionMode, ToolOutput};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Parser, Debug)]
#[command(about = "Profile common Jcode tools through the production registry path")]
struct Args {
    /// Case to execute. Use --list to print available cases.
    #[arg(long)]
    case: Option<String>,

    /// Number of measured iterations after one warm-up iteration.
    #[arg(long, default_value_t = 20)]
    iterations: usize,

    /// Repository root used as the tool working directory.
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Directory containing tiny.txt and large.txt benchmark fixtures.
    #[arg(long)]
    fixture_dir: Option<PathBuf>,

    /// Print supported cases and exit.
    #[arg(long)]
    list: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct CpuUsage {
    user_us: u64,
    system_us: u64,
}

impl CpuUsage {
    fn total_us(self) -> u64 {
        self.user_us.saturating_add(self.system_us)
    }

    fn saturating_sub(self, earlier: Self) -> Self {
        Self {
            user_us: self.user_us.saturating_sub(earlier.user_us),
            system_us: self.system_us.saturating_sub(earlier.system_us),
        }
    }
}

#[derive(Serialize)]
struct ProfileResult {
    case: String,
    iterations: usize,
    wall_us: Distribution,
    cpu_self_us_total: u64,
    cpu_children_us_total: u64,
    cpu_total_us_per_iteration: f64,
    rss_kib_before: u64,
    rss_kib_after: u64,
    rss_kib_delta: i64,
    high_water_kib_before: u64,
    high_water_kib_after: u64,
    high_water_kib_delta: i64,
    output_bytes_last: usize,
    load_average_1m_before: Option<f64>,
    load_average_1m_after: Option<f64>,
    memory_available_kib_before: Option<u64>,
    memory_available_kib_after: Option<u64>,
}

#[derive(Serialize)]
struct Distribution {
    min: u64,
    p50: u64,
    p95: u64,
    max: u64,
    mean: f64,
}

struct NullProvider;

struct LegacyReadTool;

#[async_trait]
impl Provider for NullProvider {
    async fn complete(
        &self,
        _messages: &[jcode::message::Message],
        _tools: &[jcode::message::ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        anyhow::bail!("tool_profile never calls the provider")
    }

    fn name(&self) -> &str {
        "tool-profile"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for LegacyReadTool {
    fn name(&self) -> &str {
        "profile_legacy_read"
    }

    fn description(&self) -> &str {
        "Benchmark-only copy of the pre-streaming text read algorithm."
    }

    fn parameters_schema(&self) -> Value {
        json!({"type": "object"})
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> anyhow::Result<ToolOutput> {
        let path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("file_path is required"))?;
        let offset = input["start_line"].as_u64().unwrap_or(1).saturating_sub(1) as usize;
        let limit = input["limit"].as_u64().unwrap_or(20) as usize;
        let content = tokio::fs::read_to_string(path).await?;
        let mut output = String::with_capacity(limit.min(2000) * 80);
        let end_exclusive = offset.saturating_add(limit);
        use std::fmt::Write;
        for (index, line) in content.lines().enumerate() {
            if index < offset || index >= end_exclusive {
                continue;
            }
            writeln!(output, "{:>5}\t{}", index + 1, line)?;
        }
        Ok(ToolOutput::new(output))
    }
}

const CASES: &[&str] = &[
    "ls_root",
    "read_tiny",
    "read_large_head",
    "read_large_tail",
    "legacy_read_large_head",
    "legacy_read_large_tail",
    "agentgrep_grep",
    "agentgrep_find",
    "bash_true",
    "bash_output_64k",
    "bash_background_start",
    "bg_list",
    "batch_read_4",
    "sequential_read_4",
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.list {
        for case in CASES {
            println!("{case}");
        }
        return Ok(());
    }
    let case = args
        .case
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--case is required unless --list is used"))?;
    if !CASES.contains(&case) {
        anyhow::bail!("unknown case '{case}'; use --list")
    }
    if args.iterations == 0 {
        anyhow::bail!("--iterations must be greater than zero")
    }

    let repo = args.repo.canonicalize()?;
    let fixture_dir = args
        .fixture_dir
        .as_deref()
        .map(Path::canonicalize)
        .transpose()?;
    let registry = Registry::new(Arc::new(NullProvider)).await;
    registry
        .register("profile_legacy_read".to_string(), Arc::new(LegacyReadTool))
        .await;

    let case_rss_before = proc_status_kib("VmRSS").unwrap_or(0);
    let case_hwm_before = proc_status_kib("VmHWM").unwrap_or(0);
    let load_average_1m_before = load_average_1m();
    let memory_available_kib_before = proc_meminfo_kib("MemAvailable");

    // Warm every case once so lazy regexes, logger setup, and base-tool
    // initialization are not charged to steady-state measurements.
    drop(execute_case(&registry, case, &repo, fixture_dir.as_deref(), 0).await?);
    wait_for_background_case(case).await;

    let self_cpu_before = get_usage(libc::RUSAGE_SELF);
    let child_cpu_before = get_usage(libc::RUSAGE_CHILDREN);

    let mut wall_us = Vec::with_capacity(args.iterations);
    let mut output_bytes_last = 0;
    for iteration in 0..args.iterations {
        let started = Instant::now();
        let output = execute_case(
            &registry,
            case,
            &repo,
            fixture_dir.as_deref(),
            iteration + 1,
        )
        .await?;
        wall_us.push(started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64);
        output_bytes_last = output.output.len();
    }
    wait_for_background_case(case).await;

    let self_cpu = get_usage(libc::RUSAGE_SELF).saturating_sub(self_cpu_before);
    let child_cpu = get_usage(libc::RUSAGE_CHILDREN).saturating_sub(child_cpu_before);
    let rss_after = proc_status_kib("VmRSS").unwrap_or(0);
    let hwm_after = proc_status_kib("VmHWM").unwrap_or(0);
    let load_average_1m_after = load_average_1m();
    let memory_available_kib_after = proc_meminfo_kib("MemAvailable");
    let total_cpu = self_cpu.total_us().saturating_add(child_cpu.total_us());

    let result = ProfileResult {
        case: case.to_string(),
        iterations: args.iterations,
        wall_us: distribution(&mut wall_us),
        cpu_self_us_total: self_cpu.total_us(),
        cpu_children_us_total: child_cpu.total_us(),
        cpu_total_us_per_iteration: total_cpu as f64 / args.iterations as f64,
        rss_kib_before: case_rss_before,
        rss_kib_after: rss_after,
        rss_kib_delta: signed_delta(rss_after, case_rss_before),
        high_water_kib_before: case_hwm_before,
        high_water_kib_after: hwm_after,
        high_water_kib_delta: signed_delta(hwm_after, case_hwm_before),
        output_bytes_last,
        load_average_1m_before,
        load_average_1m_after,
        memory_available_kib_before,
        memory_available_kib_after,
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn execute_case(
    registry: &Registry,
    case: &str,
    repo: &Path,
    fixture_dir: Option<&Path>,
    iteration: usize,
) -> anyhow::Result<ToolOutput> {
    let ctx = ToolContext {
        session_id: format!("tool-profile-{}", std::process::id()),
        message_id: "tool-profile".to_string(),
        tool_call_id: format!("tool-profile-{case}-{iteration}"),
        working_dir: Some(repo.to_path_buf()),
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        execution_mode: ToolExecutionMode::Direct,
    };
    let fixture = |name: &str| -> anyhow::Result<String> {
        Ok(fixture_dir
            .ok_or_else(|| anyhow::anyhow!("--fixture-dir is required for {case}"))?
            .join(name)
            .to_string_lossy()
            .into_owned())
    };

    match case {
        "ls_root" => registry.execute("ls", json!({"path": "."}), ctx).await,
        "read_tiny" => {
            registry
                .execute("read", json!({"file_path": fixture("tiny.txt")?}), ctx)
                .await
        }
        "read_large_head" => {
            registry
                .execute(
                    "read",
                    json!({"file_path": fixture("large.txt")?, "start_line": 1, "limit": 20}),
                    ctx,
                )
                .await
        }
        "read_large_tail" => {
            registry
                .execute(
                    "read",
                    json!({"file_path": fixture("large.txt")?, "start_line": 900_000, "limit": 20}),
                    ctx,
                )
                .await
        }
        "legacy_read_large_head" => {
            registry
                .execute(
                    "profile_legacy_read",
                    json!({"file_path": fixture("large.txt")?, "start_line": 1, "limit": 20}),
                    ctx,
                )
                .await
        }
        "legacy_read_large_tail" => {
            registry
                .execute(
                    "profile_legacy_read",
                    json!({"file_path": fixture("large.txt")?, "start_line": 900_000, "limit": 20}),
                    ctx,
                )
                .await
        }
        "agentgrep_grep" => {
            registry
                .execute(
                    "agentgrep",
                    json!({
                        "mode": "grep",
                        "query": "ToolContext",
                        "path": "crates",
                        "glob": "**/*.rs",
                        "max_files": 20,
                        "max_regions": 20
                    }),
                    ctx,
                )
                .await
        }
        "agentgrep_find" => {
            registry
                .execute(
                    "agentgrep",
                    json!({
                        "mode": "find",
                        "query": "tool",
                        "path": "crates",
                        "glob": "**/*.rs",
                        "max_files": 20
                    }),
                    ctx,
                )
                .await
        }
        "bash_true" => {
            registry
                .execute("bash", json!({"command": "true", "notify": false}), ctx)
                .await
        }
        "bash_output_64k" => {
            registry
                .execute(
                    "bash",
                    json!({"command": "head -c 65536 /dev/zero | tr '\\0' x", "notify": false}),
                    ctx,
                )
                .await
        }
        "bash_background_start" => {
            registry
                .execute(
                    "bash",
                    json!({
                        "command": "true",
                        "run_in_background": true,
                        "notify": false,
                        "wake": false
                    }),
                    ctx,
                )
                .await
        }
        "bg_list" => {
            registry
                .execute(
                    "bg",
                    json!({"action": "list", "session_only": true, "status_filter": "all"}),
                    ctx,
                )
                .await
        }
        "batch_read_4" => {
            let path = fixture("tiny.txt")?;
            let calls: Vec<Value> = (0..4)
                .map(|index| {
                    json!({
                        "tool": "read",
                        "intent": format!("profile read {index}"),
                        "file_path": path
                    })
                })
                .collect();
            registry
                .execute("batch", json!({"tool_calls": calls}), ctx)
                .await
        }
        "sequential_read_4" => {
            let path = fixture("tiny.txt")?;
            let mut bytes = 0usize;
            for index in 0..4 {
                let output = registry
                    .execute(
                        "read",
                        json!({"file_path": path}),
                        ctx.for_subcall(format!("sequential-{iteration}-{index}")),
                    )
                    .await?;
                bytes += output.output.len();
            }
            Ok(ToolOutput::new("x".repeat(bytes)))
        }
        _ => unreachable!("case validated above"),
    }
}

async fn wait_for_background_case(case: &str) {
    if case == "bash_background_start" {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

fn distribution(values: &mut [u64]) -> Distribution {
    values.sort_unstable();
    let percentile = |fraction: f64| {
        let index = ((values.len() - 1) as f64 * fraction).round() as usize;
        values[index]
    };
    Distribution {
        min: values[0],
        p50: percentile(0.50),
        p95: percentile(0.95),
        max: values[values.len() - 1],
        mean: values.iter().map(|value| *value as f64).sum::<f64>() / values.len() as f64,
    }
}

fn get_usage(who: libc::c_int) -> CpuUsage {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage initializes the provided rusage on success.
    let result = unsafe { libc::getrusage(who, usage.as_mut_ptr()) };
    if result != 0 {
        return CpuUsage::default();
    }
    // SAFETY: the successful call above initialized the value.
    let usage = unsafe { usage.assume_init() };
    CpuUsage {
        user_us: timeval_us(usage.ru_utime),
        system_us: timeval_us(usage.ru_stime),
    }
}

fn timeval_us(value: libc::timeval) -> u64 {
    (value.tv_sec.max(0) as u64)
        .saturating_mul(1_000_000)
        .saturating_add(value.tv_usec.max(0) as u64)
}

fn proc_status_kib(field: &str) -> Option<u64> {
    let status = match std::fs::read_to_string("/proc/self/status") {
        Ok(status) => status,
        Err(_) => return None,
    };
    status.lines().find_map(|line| {
        let value = line.strip_prefix(field)?.strip_prefix(':')?.trim();
        value
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .into_iter()
            .next()
    })
}

fn proc_meminfo_kib(field: &str) -> Option<u64> {
    let status = match std::fs::read_to_string("/proc/meminfo") {
        Ok(status) => status,
        Err(_) => return None,
    };
    status.lines().find_map(|line| {
        let value = line.strip_prefix(field)?.strip_prefix(':')?.trim();
        value
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .into_iter()
            .next()
    })
}

fn load_average_1m() -> Option<f64> {
    let loadavg = match std::fs::read_to_string("/proc/loadavg") {
        Ok(loadavg) => loadavg,
        Err(_) => return None,
    };
    loadavg
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .into_iter()
        .next()
}

fn signed_delta(after: u64, before: u64) -> i64 {
    i64::try_from(after).unwrap_or(i64::MAX) - i64::try_from(before).unwrap_or(i64::MAX)
}
