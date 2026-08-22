#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/home" "$tmp/work"

cargo_log="$tmp/cargo.jsonl"
action_log="$tmp/rust-actions.jsonl"

cat > "$tmp/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
/usr/bin/python3 - "$TEST_CARGO_LOG" "$@" <<'PY'
import json
import sys

with open(sys.argv[1], "a", encoding="utf-8") as output:
    output.write(json.dumps(sys.argv[2:]) + "\n")
PY
EOF

cat > "$tmp/bin/uname" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  -s) printf '%s\n' "${TEST_UNAME_S:-Darwin}" ;;
  -m) printf '%s\n' "${TEST_UNAME_M:-arm64}" ;;
  *) printf '%s\n' "${TEST_UNAME_S:-Darwin}" ;;
esac
EOF

cat > "$tmp/bin/nproc" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "${TEST_CPU_COUNT:-8}"
EOF

cat > "$tmp/bin/vm_stat" <<'EOF'
#!/usr/bin/env bash
if [[ "${TEST_VM_STAT_MODE:-valid}" == "failed" ]]; then
  exit 1
fi
if [[ "${TEST_VM_STAT_MODE:-valid}" == "malformed" ]]; then
  cat <<'STATS'
Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                               unknown.
STATS
  exit 0
fi
cat <<STATS
Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                               32768.
Pages active:                            99999.
Pages inactive:                         393216.
Pages speculative:                       32768.
Pages throttled:                             0.
Pages wired down:                        99999.
Pages purgeable:                         65536.
STATS
EOF

chmod +x "$tmp/bin/cargo" "$tmp/bin/uname" "$tmp/bin/nproc" "$tmp/bin/vm_stat"

run_setup() {
  (
    unset CARGO_BUILD_JOBS JCODE_BUILD_JOBS
    export PATH="$tmp/bin:/usr/bin:/bin:$PATH"
    export HOME="$tmp/home"
    export TMPDIR="$tmp/work"
    export JCODE_BUILD_GIT_HASH=test
    export JCODE_PARALLEL_FRONTEND=0
    export SCCACHE_DISABLE=1
    for assignment in "$@"; do
      export "$assignment"
    done
    "$repo_root/scripts/dev_cargo.sh" --print-setup
  )
}

run_action() {
  : >"$cargo_log"
  : >"$action_log"
  (
    unset CARGO_BUILD_JOBS JCODE_BUILD_JOBS
    export PATH="$tmp/bin:/usr/bin:/bin:$PATH"
    export HOME="$tmp/home"
    export TMPDIR="$tmp/work"
    export TEST_CARGO_LOG="$cargo_log"
    export JCODE_BUILD_GIT_HASH=test
    export JCODE_CARGO_GATE=off
    export JCODE_PARALLEL_FRONTEND=0
    export JCODE_REMOTE_CARGO=0
    export JCODE_RUST_ACTION_LOG_PATH="$action_log"
    export SCCACHE_DISABLE=1
    for assignment in "$@"; do
      export "$assignment"
    done
    hash -r
    "$repo_root/scripts/dev_cargo.sh" check --quiet
  )
}

assert_line() {
  local output="$1" expected="$2"
  if ! grep -Fqx "$expected" <<<"$output"; then
    printf 'expected setup output to contain %q\noutput:\n%s\n' "$expected" "$output" >&2
    exit 1
  fi
}

assert_job_receipt() {
  local expected_configured_json="$1"
  local expected_policy="$2"
  local expected_jobs="$3"
  /usr/bin/python3 - "$action_log" "$cargo_log" "$expected_configured_json" \
    "$expected_policy" "$expected_jobs" <<'PY'
import json
import sys

receipts = [line for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
invocations = [line for line in open(sys.argv[2], encoding="utf-8") if line.strip()]
assert len(receipts) == 1, f"expected one rust-actions receipt, got {len(receipts)}"
assert len(invocations) == 1, f"expected one fake Cargo invocation, got {len(invocations)}"

record = json.loads(receipts[0])
cargo_jobs = record.get("cargo_jobs")
assert isinstance(cargo_jobs, dict), f"receipt missing cargo_jobs metadata: {record!r}"
configured = cargo_jobs.get("configured")
effective = cargo_jobs.get("effective")
assert configured == json.loads(sys.argv[3]), (
    f"configured Cargo job policy mismatch: {configured!r}"
)
assert isinstance(effective, dict), f"receipt missing effective Cargo job policy: {cargo_jobs!r}"
assert effective.get("policy") == sys.argv[4], effective
assert effective.get("jobs") == int(sys.argv[5]), effective
PY
}

# 32,768 free + 393,216 inactive + 32,768 speculative 16 KiB pages =
# 7,168 MiB available. At 1,792 MiB/job, memory limits an 8-CPU Mac to 4 jobs.
output=$(run_setup)
assert_line "$output" 'os=Darwin'
assert_line "$output" 'build_jobs_status=adaptive:4 (cpus=8, mem_avail=7168MiB, budget=1792MiB/job)'
assert_line "$output" 'cargo_build_jobs=4'

# Both documented overrides bypass memory probing, with JCODE_BUILD_JOBS taking
# precedence when both are present.
output=$(run_setup TEST_VM_STAT_MODE=failed CARGO_BUILD_JOBS=7)
assert_line "$output" 'build_jobs_status=override:7'
assert_line "$output" 'cargo_build_jobs=7'
output=$(run_setup TEST_VM_STAT_MODE=failed CARGO_BUILD_JOBS=7 JCODE_BUILD_JOBS=3)
assert_line "$output" 'build_jobs_status=override:3'
assert_line "$output" 'cargo_build_jobs=3'

# If vm_stat cannot be read, leave CARGO_BUILD_JOBS unset so Cargo can apply its
# own config/default rather than guessing from invalid memory data.
output=$(run_setup TEST_VM_STAT_MODE=failed)
assert_line "$output" 'build_jobs_status=cargo-default'
assert_line "$output" 'cargo_build_jobs=<unset>'
output=$(run_setup TEST_VM_STAT_MODE=malformed)
assert_line "$output" 'build_jobs_status=cargo-default'
assert_line "$output" 'cargo_build_jobs=<unset>'
output=$(run_setup TEST_UNAME_S=FreeBSD)
assert_line "$output" 'build_jobs_status=cargo-default'
assert_line "$output" 'cargo_build_jobs=<unset>'

# Rust action receipts expose both the configured override inputs and the
# effective policy selected after precedence and adaptive sizing. The fake Cargo
# command keeps these contract tests deterministic and prevents real or
# concurrent compilation work.
run_action
assert_job_receipt \
  '{"jcode_build_jobs":null,"cargo_build_jobs":null}' adaptive 4

run_action CARGO_BUILD_JOBS=6
assert_job_receipt \
  '{"jcode_build_jobs":null,"cargo_build_jobs":6}' cargo-build-jobs 6

run_action JCODE_BUILD_JOBS=8
assert_job_receipt \
  '{"jcode_build_jobs":8,"cargo_build_jobs":null}' jcode-build-jobs 8

# JCODE_BUILD_JOBS remains authoritative when both documented overrides are
# configured, including the explicit 6/8 experiment values.
run_action CARGO_BUILD_JOBS=8 JCODE_BUILD_JOBS=6
assert_job_receipt \
  '{"jcode_build_jobs":6,"cargo_build_jobs":8}' jcode-build-jobs 6

echo 'dev_cargo job sizing tests passed'
