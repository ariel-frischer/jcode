"""Regression checks for main-owned fork release publishing."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW_PATH = ROOT / ".github/workflows/release.yml"


def load_workflow() -> dict:
    return yaml.load(WORKFLOW_PATH.read_text(), Loader=yaml.BaseLoader)


def step_script(workflow: dict, job_name: str, step_name: str) -> str:
    for step in workflow["jobs"][job_name]["steps"]:
        if step.get("name") == step_name:
            return textwrap.dedent(step["run"])
    raise AssertionError(f"step not found: {job_name}/{step_name}")


def run_bash(script: str, cwd: Path, **variables: str) -> subprocess.CompletedProcess[str]:
    environment = os.environ.copy()
    environment.update(variables)
    return subprocess.run(
        ["bash", "-euo", "pipefail", "-c", script],
        cwd=cwd,
        env=environment,
        text=True,
        capture_output=True,
    )


def git(repo: Path, *args: str, env: dict[str, str] | None = None) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=repo,
        env=env,
        check=True,
        text=True,
        capture_output=True,
    )
    return result.stdout.strip()


def init_repo(path: Path) -> str:
    path.mkdir()
    git(path, "init", "--initial-branch=main")
    git(path, "config", "user.email", "test@example.invalid")
    git(path, "config", "user.name", "Release Test")
    (path / "source.txt").write_text("source\n")
    git(path, "add", "source.txt")
    commit_env = os.environ | {
        "GIT_AUTHOR_DATE": "2026-09-09T12:34:56Z",
        "GIT_COMMITTER_DATE": "2026-09-09T12:34:56Z",
    }
    subprocess.run(
        ["git", "commit", "-m", "source"],
        cwd=path,
        env=commit_env,
        check=True,
        text=True,
        capture_output=True,
    )
    return git(path, "rev-parse", "HEAD")


class MainReleaseTests(unittest.TestCase):
    def test_only_main_pushes_trigger_release(self):
        workflow = load_workflow()
        self.assertEqual(workflow["on"], {"push": {"branches": ["main"]}})

        self.assertNotIn("tags:", WORKFLOW_PATH.read_text())

    def test_release_jobs_are_read_only_except_publisher(self):
        workflow = load_workflow()
        jobs = workflow["jobs"]
        self.assertEqual(
            set(jobs), {"prepare", "build-linux", "build-macos", "build-windows", "publish"}
        )
        for name in ("prepare", "build-linux", "build-macos", "build-windows"):
            self.assertEqual(jobs[name]["permissions"], {"contents": "read"})
        self.assertEqual(
            jobs["publish"]["permissions"], {"actions": "read", "contents": "write"}
        )

        text = WORKFLOW_PATH.read_text()
        self.assertNotRegex(text, r"(?i)(ssh-agent|ssh-key|secrets\.|azure|homebrew|aur|discord)")
        self.assertNotIn("continue-on-error", text)
        self.assertNotIn("always()", text)
        self.assertIn('--repo "$GITHUB_REPOSITORY"', text)
        self.assertEqual(text.count("ref: ${{ github.sha }}"), 5)

    def test_build_matrix_keeps_cli_packaging_contracts(self):
        workflow = load_workflow()
        text = WORKFLOW_PATH.read_text()
        publish_needs = workflow["jobs"]["publish"]["needs"]
        self.assertEqual(
            set(publish_needs), {"prepare", "build-linux", "build-macos", "build-windows"}
        )
        self.assertIn("scripts/build_linux_compat.sh dist", text)
        self.assertGreaterEqual(text.count("cargo build --locked --release"), 2)
        self.assertIn("'--locked'", text)
        for artifact in (
            "jcode-linux-x86_64",
            "jcode-linux-aarch64",
            "jcode-macos-aarch64",
            "jcode-macos-x86_64",
            "jcode-windows-x86_64",
            "jcode-windows-aarch64",
        ):
            self.assertIn(artifact, text)

    def test_main_dev_and_tag_event_matrix(self):
        workflow = load_workflow()
        trigger = workflow["on"]

        def triggers(event: str) -> bool:
            return event == "push-main" and trigger == {"push": {"branches": ["main"]}}

        self.assertTrue(triggers("push-main"))
        self.assertFalse(triggers("push-dev"))
        self.assertFalse(triggers("push-tag"))

    def test_metadata_script_derives_deterministic_numeric_tag(self):
        workflow = load_workflow()
        script = step_script(workflow, "prepare", "Compute deterministic release metadata")
        with tempfile.TemporaryDirectory() as temporary:
            repo = Path(temporary) / "repo"
            source_sha = init_repo(repo)
            output = Path(temporary) / "output"
            summary = Path(temporary) / "summary"
            result = run_bash(
                script,
                repo,
                SOURCE_SHA=source_sha,
                RUN_NUMBER="17",
                GITHUB_RUN_NUMBER="17",
                GITHUB_REPOSITORY="ariel-frischer/jcode",
                GITHUB_OUTPUT=str(output),
                GITHUB_STEP_SUMMARY=str(summary),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            values = dict(line.split("=", 1) for line in output.read_text().splitlines())
            self.assertEqual(values["release_tag"], "v2026.9.17")
            self.assertEqual(values["source_sha"], source_sha)
            self.assertIn(source_sha, summary.read_text())

    def test_existing_tag_for_different_source_is_rejected(self):
        workflow = load_workflow()
        script = step_script(
            workflow, "publish", "Verify release tag source and create draft release"
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            repo = root / "repo"
            source_sha = init_repo(repo)
            remote = root / "remote.git"
            subprocess.run(["git", "init", "--bare", str(remote)], check=True, capture_output=True)
            git(repo, "remote", "add", "origin", str(remote))
            git(repo, "push", "origin", "main")
            (repo / "source.txt").write_text("different\n")
            git(repo, "add", "source.txt")
            git(repo, "commit", "-m", "different")
            wrong_sha = git(repo, "rev-parse", "HEAD")
            release_tag = "v2026.9.17"
            git(repo, "tag", release_tag, wrong_sha)
            git(repo, "push", "origin", f"refs/tags/{release_tag}")

            result = run_bash(
                script,
                repo,
                SOURCE_SHA=source_sha,
                RELEASE_TAG=release_tag,
                GITHUB_REPOSITORY="ariel-frischer/jcode",
                GH_TOKEN="test-token",
                GITHUB_OUTPUT=str(root / "output"),
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("does not point to source commit", result.stderr)

    def test_rerun_reuses_tag_and_does_not_mutate_published_release(self):
        workflow = load_workflow()
        script = step_script(
            workflow, "publish", "Verify release tag source and create draft release"
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            repo = root / "repo"
            source_sha = init_repo(repo)
            remote = root / "remote.git"
            subprocess.run(["git", "init", "--bare", str(remote)], check=True, capture_output=True)
            git(repo, "remote", "add", "origin", str(remote))
            git(repo, "push", "origin", "main")

            fake_bin = root / "bin"
            fake_bin.mkdir()
            log = root / "gh.log"
            state = root / "release-state"
            fake_gh = fake_bin / "gh"
            fake_gh.write_text(
                textwrap.dedent(
                    """\
                    #!/usr/bin/env bash
                    set -euo pipefail
                    printf '%s\n' "$*" >> "$GH_LOG"
                    if [[ "$1" == release && "$2" == view ]]; then
                      if [[ -f "$GH_STATE" ]]; then printf '%s\n' "$(cat "$GH_STATE")"; else exit 1; fi
                    elif [[ "$1" == release && "$2" == create ]]; then
                      git tag "$3" "$GITHUB_SHA"
                      git push origin "refs/tags/$3" >/dev/null
                      printf '%s\n' draft > "$GH_STATE"
                    else
                      echo "unexpected gh invocation: $*" >&2
                      exit 2
                    fi
                    """
                )
            )
            fake_gh.chmod(0o755)
            environment = {
                "SOURCE_SHA": source_sha,
                "RELEASE_TAG": "v2026.9.17",
                "GITHUB_REPOSITORY": "ariel-frischer/jcode",
                "GH_TOKEN": "test-token",
                "GH_LOG": str(log),
                "GH_STATE": str(state),
                "GITHUB_OUTPUT": str(root / "output"),
                "PATH": f"{fake_bin}:{os.environ['PATH']}",
            }
            environment["GITHUB_SHA"] = source_sha
            first = run_bash(script, repo, **environment)
            second = run_bash(script, repo, **environment)
            self.assertEqual(first.returncode, 0, first.stderr)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertEqual(log.read_text().count("release create"), 1)
            tag_ref = git(repo, "ls-remote", "origin", "refs/tags/v2026.9.17")
            self.assertEqual(tag_ref.split()[0], source_sha)

            state.write_text("false\n")
            before = log.read_text()
            published_retry = run_bash(script, repo, **environment)
            self.assertEqual(published_retry.returncode, 0, published_retry.stderr)
            self.assertEqual(log.read_text(), before + "release view v2026.9.17 --repo ariel-frischer/jcode --json isDraft --jq .isDraft\n")

    def test_failed_upload_cannot_publish_release(self):
        workflow = load_workflow()
        script = step_script(workflow, "publish", "Upload assets and publish release")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            (artifacts / "jcode-linux-x86_64.tar.gz").write_text("asset")
            (root / "SHA256SUMS").write_text("checksum\n")
            (root / "BUILD-PROVENANCE.txt").write_text("provenance\n")
            fake_bin = root / "bin"
            fake_bin.mkdir()
            log = root / "gh.log"
            fake_gh = fake_bin / "gh"
            fake_gh.write_text(
                "#!/usr/bin/env bash\nset -euo pipefail\nprintf '%s\\n' \"$*\" >> \"$GH_LOG\"\nif [[ \"$2\" == upload ]]; then exit 23; fi\n"
            )
            fake_gh.chmod(0o755)
            result = run_bash(
                script,
                root,
                RELEASE_TAG="v2026.9.17",
                GITHUB_REPOSITORY="ariel-frischer/jcode",
                GH_TOKEN="test-token",
                GH_LOG=str(log),
                PATH=f"{fake_bin}:{os.environ['PATH']}",
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("--draft=false", log.read_text())


if __name__ == "__main__":
    unittest.main()
