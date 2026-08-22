#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/home" "$tmp/work"

cargo_log="$tmp/cargo.jsonl"
resolver_log="$tmp/resolver.jsonl"
action_log="$tmp/rust-actions.jsonl"
gate_log="$tmp/cargo-gate.log"

cat >"$tmp/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
python3 - "$TEST_CARGO_LOG" "$@" <<'PY'
import json
import os
import sys

with open(sys.argv[1], "a", encoding="utf-8") as output:
    output.write(json.dumps(sys.argv[2:]) + "\n")
if os.environ.get("TEST_CARGO_FAIL_ALL_FEATURES") == "1" and "--all-features" in sys.argv[2:]:
    print(
        "optional-feature fixture: telemetry-only source failed full-feature validation",
        file=sys.stderr,
    )
    raise SystemExit(42)
PY
printf 'running 1 test\n'
EOF

cat >"$tmp/bin/fake-scope-resolver" <<'EOF'
#!/usr/bin/env python3
import json
import os
import sys

case = os.environ["TEST_SCOPE_CASE"]
args = sys.argv[1:]
with open(os.environ["TEST_RESOLVER_LOG"], "a", encoding="utf-8") as output:
    output.write(json.dumps(args) + "\n")

try:
    explicit = json.loads(args[args.index("--explicit-json") + 1])
except (ValueError, IndexError, json.JSONDecodeError):
    explicit = {}

if case == "conflict":
    print("rust-validation-scope: explicit feature selection conflicts with required feature telemetry", file=sys.stderr)
    raise SystemExit(2)

results = {
    "inferred": {
        "explicit_inputs": {},
        "defaults": {},
        "affected_paths": ["crates/alpha/src/lib.rs"],
        "effective_scope": {
            "mode": "focused",
            "packages": ["alpha"],
            "targets": ["lib:alpha"],
            "features": [],
            "no_default_features": False,
            "all_features": False,
        },
        "resolution_source": "inferred",
        "fallback_reason": None,
    },
    "optional": {
        "explicit_inputs": {},
        "defaults": {},
        "affected_paths": ["crates/alpha/src/telemetry/mod.rs"],
        "effective_scope": {
            "mode": "focused",
            "packages": ["alpha"],
            "targets": ["lib:alpha"],
            "features": ["telemetry"],
            "no_default_features": False,
            "all_features": False,
        },
        "resolution_source": "inferred",
        "fallback_reason": None,
    },
    "explicit": {
        "explicit_inputs": {
            "packages": ["beta"],
            "targets": ["bin:beta-cli"],
            "features": ["user"],
            "no_default_features": True,
            "all_features": False,
        },
        "defaults": {},
        "affected_paths": ["crates/alpha/src/lib.rs"],
        "effective_scope": {
            "mode": "focused",
            "packages": ["beta"],
            "targets": ["bin:beta-cli"],
            "features": ["user"],
            "no_default_features": True,
            "all_features": False,
        },
        "resolution_source": "explicit",
        "fallback_reason": None,
    },
    "fallback": {
        "explicit_inputs": {},
        "defaults": {},
        "affected_paths": ["Cargo.lock"],
        "effective_scope": {
            "mode": "broad",
            "packages": [],
            "targets": [],
            "features": [],
            "no_default_features": False,
            "all_features": False,
        },
        "resolution_source": "conservative_fallback",
        "fallback_reason": "workspace path requires broad validation",
    },
    "broad-explicit": {
        "explicit_inputs": explicit,
        "defaults": {},
        "affected_paths": ["crates/alpha/src/lib.rs"],
        "effective_scope": {
            "mode": "broad",
            "packages": explicit.get("packages", []),
            "targets": explicit.get("targets", []),
            "features": explicit.get("features", []),
            "no_default_features": explicit.get("no_default_features", False),
            "all_features": explicit.get("all_features", False),
        },
        "resolution_source": "explicit",
        "fallback_reason": "explicit broad or release-sensitive Cargo request",
    },
}
json.dump(results[case], sys.stdout)
sys.stdout.write("\n")
EOF

cat >"$tmp/bin/flock" <<'EOF'
#!/usr/bin/env bash
printf 'unexpected flock invocation: %q\n' "$*" >>"$TEST_GATE_LOG"
exit 99
EOF

chmod +x "$tmp/bin/cargo" "$tmp/bin/fake-scope-resolver" "$tmp/bin/flock"

reset_logs() {
  : >"$cargo_log"
  : >"$resolver_log"
  : >"$action_log"
}

run_dev_cargo() {
  local scope_case="$1"
  local affected_paths="$2"
  shift 2
  (
    export PATH="$tmp/bin:/usr/bin:/bin:$PATH"
    export HOME="$tmp/home"
    export TMPDIR="$tmp/work"
    export TEST_CARGO_LOG="$cargo_log"
    export TEST_GATE_LOG="$gate_log"
    export TEST_RESOLVER_LOG="$resolver_log"
    export TEST_SCOPE_CASE="$scope_case"
    export TEST_CARGO_FAIL_ALL_FEATURES="${TEST_CARGO_FAIL_ALL_FEATURES:-0}"
    export JCODE_BUILD_GIT_HASH=test
    export JCODE_BUILD_JOBS=1
    export JCODE_BUILD_TMPDIR="$tmp/work/cargo-tmp"
    export JCODE_CARGO_GATE=off
    export JCODE_DEV_CARGO_AFFECTED_PATHS="$affected_paths"
    export JCODE_DEV_CARGO_SCOPE_RESOLVER="$tmp/bin/fake-scope-resolver"
    export JCODE_PARALLEL_FRONTEND=0
    export JCODE_REMOTE_CARGO=0
    export JCODE_RUST_ACTION_LOG_PATH="$action_log"
    export SCCACHE_DISABLE=1
    hash -r
    "$repo_root/scripts/dev_cargo.sh" "$@"
  )
}

assert_last_cargo_argv() {
  local expected_json="$1"
  python3 - "$cargo_log" "$expected_json" <<'PY'
import json
import sys

lines = [line for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
assert lines, "expected the fake cargo command to be invoked"
actual = json.loads(lines[-1])
expected = json.loads(sys.argv[2])
assert actual == expected, f"cargo argv mismatch\nexpected: {expected!r}\nactual:   {actual!r}"
PY
}

assert_no_cargo_invocation() {
  if [[ -s "$cargo_log" ]]; then
    printf 'expected no cargo invocation, got:\n%s\n' "$(cat "$cargo_log")" >&2
    exit 1
  fi
}

assert_resolver_called_with() {
  local expected_path="$1"
  python3 - "$resolver_log" "$expected_path" <<'PY'
import json
import sys

lines = [line for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
assert lines, "expected focused-scope resolver invocation"
argv = json.loads(lines[-1])
assert sys.argv[2] in argv, f"missing affected path {sys.argv[2]!r} in resolver argv: {argv!r}"
PY
}

assert_resolver_json_option() {
  local option="$1"
  local expected_json="$2"
  python3 - "$resolver_log" "$option" "$expected_json" <<'PY'
import json
import sys

lines = [line for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
assert lines, "expected focused-scope resolver invocation"
argv = json.loads(lines[-1])
try:
    value = argv[argv.index(sys.argv[2]) + 1]
except (ValueError, IndexError) as error:
    raise AssertionError(f"resolver argv missing {sys.argv[2]} value: {argv!r}") from error
assert json.loads(value) == json.loads(sys.argv[3]), (
    f"{sys.argv[2]} mismatch: expected {sys.argv[3]}, got {value}"
)
PY
}

assert_scope_receipt() {
  local expected_source="$1"
  local expected_mode="$2"
  local expected_packages="$3"
  local expected_targets="$4"
  local expected_features="$5"
  python3 - "$action_log" "$expected_source" "$expected_mode" \
    "$expected_packages" "$expected_targets" "$expected_features" <<'PY'
import json
import sys

lines = [line for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
assert len(lines) == 1, f"expected one rust-actions receipt, got {len(lines)}"
record = json.loads(lines[0])
scope = record.get("validation_scope")
assert isinstance(scope, dict), f"receipt missing validation_scope metadata: {record!r}"
configured = scope.get("configured_scope")
effective = scope.get("effective_scope")
assert isinstance(configured, dict), f"receipt missing configured_scope: {scope!r}"
assert isinstance(effective, dict), f"receipt missing effective_scope: {scope!r}"
for field in ("mode", "packages", "targets", "features"):
    assert field in configured, f"configured_scope missing {field}: {configured!r}"
    assert field in effective, f"effective_scope missing {field}: {effective!r}"
assert scope.get("resolution_source") == sys.argv[2], scope
assert effective["mode"] == sys.argv[3], effective
assert effective["packages"] == json.loads(sys.argv[4]), effective
assert effective["targets"] == json.loads(sys.argv[5]), effective
assert effective["features"] == json.loads(sys.argv[6]), effective
PY
}

assert_full_feature_receipt() {
  local receipt_index="$1"
  local expected_argv="$2"
  local expected_action="$3"
  local expected_profile="$4"
  local expected_exit="$5"
  python3 - "$action_log" "$receipt_index" "$expected_argv" "$expected_action" \
    "$expected_profile" "$expected_exit" <<'PY'
import json
import sys

records = [json.loads(line) for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
index = int(sys.argv[2])
assert len(records) > index, f"missing receipt {index}; got {len(records)} receipt(s)"
record = records[index]
expected_exit = int(sys.argv[6])
assert record.get("argv") == json.loads(sys.argv[3]), record
assert record.get("action") == sys.argv[4], record
assert record.get("profile") == sys.argv[5], record
assert record.get("exit_code") == expected_exit, record
assert record.get("success") is (expected_exit == 0), record
scope = record.get("validation_scope")
assert isinstance(scope, dict), f"receipt missing validation_scope: {record!r}"
assert scope.get("resolution_source") == "explicit", scope
configured = scope.get("configured_scope")
effective = scope.get("effective_scope")
assert isinstance(configured, dict), scope
assert isinstance(effective, dict), scope
assert configured.get("all_features") is True, configured
assert effective.get("all_features") is True, effective
assert effective.get("mode") == "broad", effective
assert effective.get("packages") == [], effective
assert effective.get("targets") == [], effective
PY
}

assert_focused_success_receipt() {
  local receipt_index="$1"
  local expected_argv="$2"
  python3 - "$action_log" "$receipt_index" "$expected_argv" <<'PY'
import json
import sys

records = [json.loads(line) for line in open(sys.argv[1], encoding="utf-8") if line.strip()]
record = records[int(sys.argv[2])]
assert record.get("argv") == json.loads(sys.argv[3]), record
assert record.get("action") == "check", record
assert record.get("profile") == "dev", record
assert record.get("exit_code") == 0, record
assert record.get("success") is True, record
scope = record.get("validation_scope")
assert isinstance(scope, dict), record
assert scope.get("resolution_source") == "explicit", scope
effective = scope.get("effective_scope")
assert isinstance(effective, dict), scope
assert effective.get("mode") == "focused", effective
assert effective.get("packages") == ["beta"], effective
assert effective.get("targets") == ["bin:beta-cli"], effective
assert effective.get("features") == ["user"], effective
assert effective.get("all_features") is False, effective
PY
}

# Eligible focused requests insert inferred Cargo selection before harness argv
# while preserving every caller-provided argument and its ordering.
reset_logs
run_dev_cargo inferred 'crates/alpha/src/lib.rs' test --quiet alpha_test -- --exact --nocapture
assert_last_cargo_argv '["test","--quiet","alpha_test","-p","alpha","--lib","--","--exact","--nocapture"]'
assert_resolver_called_with 'crates/alpha/src/lib.rs'
assert_scope_receipt inferred focused '["alpha"]' '["lib:alpha"]' '[]'

# Required optional features are additive and appear in the effective receipt.
reset_logs
run_dev_cargo optional 'crates/alpha/src/telemetry/mod.rs' check --quiet
assert_last_cargo_argv '["check","--quiet","-p","alpha","--lib","--features","telemetry"]'
assert_scope_receipt inferred focused '["alpha"]' '["lib:alpha"]' '["telemetry"]'

# Explicit Cargo package, target, feature, and no-default-feature arguments retain
# byte-for-byte argv precedence over inferred scope.
reset_logs
run_dev_cargo explicit 'crates/alpha/src/lib.rs' check -p beta --bin beta-cli \
  --no-default-features --features user --quiet
assert_last_cargo_argv '["check","-p","beta","--bin","beta-cli","--no-default-features","--features","user","--quiet"]'
assert_resolver_json_option --explicit-json '{"packages":["beta"],"targets":["bin:beta-cli"],"features":["user"],"no_default_features":true,"all_features":false}'
assert_scope_receipt explicit focused '["beta"]' '["bin:beta-cli"]' '["user"]'

# Conservative fallback records why the established broad command was retained.
reset_logs
run_dev_cargo fallback 'Cargo.lock' check --quiet
assert_last_cargo_argv '["check","--quiet"]'
assert_scope_receipt conservative_fallback broad '[]' '[]' '[]'

# Repository feature profiles remain broad under affected-path narrowing because
# their root-package feature names may not belong to the inferred leaf package.
for profile_case in \
  'minimal|["check","--quiet","--no-default-features"]|[]' \
  'none|["check","--quiet","--no-default-features"]|[]' \
  'pdf|["check","--quiet","--no-default-features","--features","pdf"]|["pdf"]' \
  'embeddings|["check","--quiet","--no-default-features","--features","embeddings"]|["embeddings"]' \
  'full|["check","--quiet","--features","embeddings,pdf,bedrock"]|["embeddings","pdf","bedrock"]'; do
  IFS='|' read -r profile expected_argv expected_features <<<"$profile_case"
  reset_logs
  JCODE_DEV_FEATURE_PROFILE="$profile" run_dev_cargo inferred \
    'crates/alpha/src/lib.rs' check --quiet
  assert_last_cargo_argv "$expected_argv"
  assert_scope_receipt conservative_fallback broad '[]' '[]' "$expected_features"
done

# Resolver conflicts fail before Cargo launch and remain actionable.
reset_logs
if run_dev_cargo conflict 'crates/alpha/src/telemetry/mod.rs' check \
  --no-default-features --features serde 2>"$tmp/conflict.stderr"; then
  echo 'expected focused-scope conflict to fail' >&2
  exit 1
fi
assert_no_cargo_invocation
grep -Fq 'conflicts with required feature telemetry' "$tmp/conflict.stderr"

# Explicit workspace/all-feature guardrails are never narrowed or reordered.
reset_logs
run_dev_cargo broad-explicit 'crates/alpha/src/lib.rs' check --workspace --all-targets --all-features --quiet
assert_last_cargo_argv '["check","--workspace","--all-targets","--all-features","--quiet"]'
assert_full_feature_receipt 0 '["check","--workspace","--all-targets","--all-features","--quiet"]' check dev 0

# Explicit full-feature test and build commands retain their broad targets,
# feature selection, exit contract, and rust-actions receipt fields.
reset_logs
run_dev_cargo broad-explicit 'crates/alpha/src/lib.rs' test --workspace --all-targets --all-features --quiet
assert_last_cargo_argv '["test","--workspace","--all-targets","--all-features","--quiet"]'
assert_full_feature_receipt 0 '["test","--workspace","--all-targets","--all-features","--quiet"]' test dev 0

reset_logs
run_dev_cargo broad-explicit 'crates/alpha/src/lib.rs' build --workspace --all-targets --all-features --quiet
assert_last_cargo_argv '["build","--workspace","--all-targets","--all-features","--quiet"]'
assert_full_feature_receipt 0 '["build","--workspace","--all-targets","--all-features","--quiet"]' build dev 0

# Release-sensitive commands remain broad even when affected paths would
# otherwise be eligible for inferred package and target narrowing.
reset_logs
run_dev_cargo broad-explicit 'crates/alpha/src/lib.rs' build --workspace --all-targets --all-features --release --quiet
assert_last_cargo_argv '["build","--workspace","--all-targets","--all-features","--release","--quiet"]'
assert_full_feature_receipt 0 '["build","--workspace","--all-targets","--all-features","--release","--quiet"]' build release 0

# A source defect hidden behind an optional feature can pass a focused command,
# but the explicit full-feature release gate still fails and records that exit.
reset_logs
run_dev_cargo explicit 'crates/alpha/src/lib.rs' check -p beta --bin beta-cli \
  --no-default-features --features user --quiet
assert_last_cargo_argv '["check","-p","beta","--bin","beta-cli","--no-default-features","--features","user","--quiet"]'
if TEST_CARGO_FAIL_ALL_FEATURES=1 run_dev_cargo broad-explicit \
  'crates/alpha/src/telemetry/mod.rs' build --workspace --all-targets \
  --all-features --release --quiet 2>"$tmp/full-feature.stderr"; then
  echo 'expected missing optional-feature fixture to fail the full-feature release gate' >&2
  exit 1
fi
grep -Fq 'telemetry-only source failed full-feature validation' "$tmp/full-feature.stderr"
assert_focused_success_receipt 0 '["check","-p","beta","--bin","beta-cli","--no-default-features","--features","user","--quiet"]'
assert_full_feature_receipt 1 '["build","--workspace","--all-targets","--all-features","--release","--quiet"]' build release 42

echo 'dev_cargo focused-scope integration tests passed'
