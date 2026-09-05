#!/usr/bin/env bash
# Downstream contract owned by jcode-h3eu. Keep this gate during upstream syncs.
# Thin Cargo orchestration, matching check_guardrails.sh without a new toolchain.
set -euo pipefail
cd "$(dirname "$0")/.."
required=(
  exposed_and_openai_projected_schema_preserve_optional_model
  spawn_and_assignment_wire_preserve_explicit_native_route
  new_worker_requests_forward_explicit_model_and_effort
  oversized_route_catalog_is_bounded
  explicit_native_oauth_request_overrides_api_coordinator
  resolve_swarm_spawn_model_requested_model_overrides_configured_pin
  resolve_swarm_spawn_model_requested_inherit_overrides_configured_pin
  resolve_swarm_spawn_model_inherits_coordinator_auth_route_for_oauth_vs_api
  resolve_swarm_spawn_model_blank_requested_model_falls_back_to_config
)
check_inventory() {
  local inventory="$1" name
  for name in "${required[@]}"; do
    if ! grep -Fq "::$name: test" <<<"$inventory"; then
      echo "Local swarm routing contract missing: $name. Preserve explicit routing during upstream syncs." >&2
      return 1
    fi
  done
}
if [[ "${1:-}" == --self-test ]]; then
  inventory=$(printf 'local_swarm_routing_contract::%s: test\n' "${required[@]}")
  check_inventory "$inventory"
  for name in "${required[@]}"; do
    if check_inventory "$(grep -Fv "::$name: test" <<<"$inventory")" 2>/dev/null; then
      echo "Contract inventory failed to detect deleted test: $name" >&2
      exit 1
    fi
  done
  echo "Local swarm routing inventory rejects every required test deletion."
  exit 0
fi
if (( $# )); then echo "Usage: $0 [--self-test]" >&2; exit 2; fi
cargo_args=(test --profile selfdev -p jcode-app-core --lib local_swarm_routing_contract)
inventory=$(bash scripts/dev_cargo.sh "${cargo_args[@]}" -- --list)
check_inventory "$inventory"
bash scripts/dev_cargo.sh "${cargo_args[@]}" -- --nocapture
