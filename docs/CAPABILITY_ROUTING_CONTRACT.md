# Jcode Capability Routing Contract

**Status:** Accepted contract for downstream implementation
**Owner:** swarm routing policy and compatibility boundary
**Bead:** `jcode-7xi`
**Last updated:** 2026-08-04

## Decision summary

### Local explicit-spawn compatibility guard (jcode-h3eu)

The custom distribution preserves optional `swarm.model` for explicit spawn and
new-worker assignment paths. The tool, `CommSpawn`/`CommAssignNext` wire messages,
and both server dispatchers carry the request to the existing spawn resolver.
Explicit model wins over `agents.swarm_model`, which wins over coordinator
inheritance. Blank is omitted; explicit `inherit` or `coordinator` bypasses a
configured pin. Existing workers are not retargeted by assignment requests.

For an **explicit swarm request**, native `openai:gpt-6-astra` pins OpenAI OAuth,
including when the coordinator uses an API key. The canonical `AuthRoute` parser
also handles explicit credential prefixes such as `openai-oauth:` and
`openai-api:`. Configured model strings retain their existing prefix semantics.
The request's `effort` is forwarded independently. No user configuration changes
are required.

`list_models` shows at most 60 distinct routes within a 12 KiB output budget.
Current/configured models are prioritized, then available routes. Individual
metadata fields are Unicode-safely bounded, auth choices remain distinct, and
the footer reports omitted entries and points to the full model picker.

Before integrating an upstream sync, run:

```bash
bash scripts/check_swarm_routing_contract.sh --self-test
bash scripts/check_swarm_routing_contract.sh
```

The same gate runs in `scripts/check_guardrails.sh` and CI's Quality Guardrails
job. It checks that key compiled tests still exist before running the local
schema/projection, wire, precedence/auth, and catalog contract suite. Thus deleting
tests cannot silently pass with zero tests. Keep these downstream tests and gate
invocations when upstream changes its swarm policy. Runtime acceptance still
requires a freshly built private instance and actual worker route/effort metadata.

### Child-task policy

Swarm child-task routing has one resolver and one vocabulary. A task may optionally
provide a `role`, `model`, and `reasoning_effort`. The resolver computes one
immutable effective selection for the child session:

1. explicit per-task selection,
2. configured policy for the task role,
3. the existing coordinator/session selection,
4. the built-in provider/model default.

Automatic routing applies only to newly created swarm children. It never changes
the active top-level/coordinator session. The coordinator remains on the model
selected by the user unless the user explicitly changes it.

This record defines the contract. It does not implement role inference, provider
selection, or UI changes.

## 1. Canonical vocabulary and ownership

| Concept | Canonical name | Meaning | Owning surface |
| --- | --- | --- | --- |
| Task role | `role` | Optional stable policy key such as `general`, `reviewer`, or `researcher`. It is a routing input, not an LLM-inferred classification. | Swarm task protocol and planner output |
| Requested model | `model` | Optional model route specification. A route-qualified value such as `openai-api:gpt-5.5` is allowed when the caller needs a specific auth/provider route. | `SwarmTaskSpec`; resolved through `ModelRoute`/`RouteSelection` |
| Requested effort | `reasoning_effort` | Optional provider reasoning/thinking effort. It is passed to the child only when the selected provider/model accepts it. | `SwarmTaskSpec`; session/provider effort contract |
| Role policy | `agents.swarm_role_policies.<role>` | Optional configured defaults for a role. A policy may set `model` and/or `reasoning_effort`; absent members inherit lower-precedence values. | `AgentsConfig` in `jcode-config-types` |
| Effective route | `RouteSelection` | The concrete model, runtime key, API method, provider label, and route detail selected for execution. | `jcode-provider-core` |
| Persisted child identity | `model`, `provider_key`, `route_api_method`, `reasoning_effort` | Durable identity used to restore the child without falling back to process configuration. | `Session` in `jcode-base` |
| Observed runtime evidence | `SwarmMemberRuntime.model`, `.provider`, `.auth_method`, `.effort` | What the child actually started with and what the live swarm status reports. | `jcode-protocol` and server swarm state |

### 1.1 Compatibility alias

`subagent_type` is the legacy planner/task field. It remains accepted for old
planner output and old requests, but it is not a second routing vocabulary:

- `role` is canonical for new wire messages and planner output.
- A request containing only `subagent_type` is normalized to `role`.
- If both fields are present, trim both values. Equal values are accepted.
  Distinct non-empty values are a visible protocol error, not an arbitrary choice.
- An empty `role` or `subagent_type` is treated as absent. Existing requests that
  omit both fields remain valid.

The implementation may keep the Rust field named `subagent_type` temporarily if
serde accepts `role` as the canonical name, but all new documentation, generated
planner instructions, and downstream code must use `role`.

### 1.2 Route identity is derived, not duplicated

Tasks do not grow independent `provider`, `provider_key`, or `route_api_method`
request fields. A task's `model` is resolved through the existing
`ModelRoute`/`RouteSelection` catalog and route parser. The resolver then carries
the resulting provider/auth identity into the child `Session`.

This prevents a task from requesting contradictory values such as one provider
label with another provider's auth route. `provider_key` and `route_api_method`
remain persisted/runtime identity, not user-authored duplicate inputs.

## 2. Canonical resolution algorithm

The resolver accepts:

- the normalized task fields (`role`, `model`, `reasoning_effort`),
- the configured role policy, if any,
- the coordinator's persisted/effective session selection,
- the built-in provider/model default.

It returns either one effective selection or an actionable error. Resolution is
performed once before child execution. The child must not independently re-run a
different precedence chain.

### 2.1 Precedence

| Priority | Source | Applies when | Result |
| --- | --- | --- | --- |
| 1 | Explicit task fields | The task supplies a non-empty `model` and/or `reasoning_effort`. | Resolve and validate the requested value. No lower source may replace an invalid, unavailable, or mismatched explicit model. |
| 2 | Configured policy | A matching role policy, or the legacy role-independent configured policy, supplies a value not explicitly supplied by the task. A matching role policy wins per field; the legacy policy fills fields that the role policy leaves unset. | Resolve the configured value and combine it with coordinator identity only for fields both configured sources leave unset. |
| 3 | Coordinator/session inheritance | The task and applicable role policy leave a field unset. | Copy the coordinator's effective model, `provider_key`, `route_api_method`, and effort where applicable. |
| 4 | Built-in default | No coordinator/session value is available, such as a new standalone child with no persisted identity. | Use the existing provider/model default resolver. This is the existing legacy behavior, not a new swarm override. |

Resolution is field-aware. For example, a role policy may set only effort while
model and route continue to inherit from the coordinator. A selected model always
resolves its matching route as one unit; provider/auth identity must not be mixed
from a different model source.

### 2.2 Coordinator invariant

The resolver may select a different model for a child, but it MUST NOT mutate the
coordinator's provider, model, route, or effort. The coordinator remains on the
user-selected top-level model for planning, orchestration, integration, and the
final response. Automatic changes to the top-level model require an explicit user
action such as `/model`.

## 3. Value semantics and failure policy

Normalization happens at the task/config trust boundary before precedence is
applied:

| Input state | Task field behavior | Config policy behavior |
| --- | --- | --- |
| Absent/unset | Continue to the next precedence source. | No policy value; continue to inheritance/default. |
| Empty or whitespace-only | Equivalent to absent for optional fields. It is not an explicit model request. | Equivalent to absent; do not create an empty policy. |
| Valid non-empty value | Resolve and validate at the provider/catalog boundary. | Resolve and validate when the policy is loaded or first used. |
| Invalid non-empty value | Visible task error before child execution. Do not fall back or substitute. | Visible configuration error. Do not silently skip the policy or use another model. |
| Unavailable route/model | Visible error naming the requested non-secret route/model and availability problem. Do not run a different model. | The affected task fails visibly. Do not silently fall back to the coordinator. |
| Provider resolves a different model | Visible mismatch error before the task is considered started. | Same rule. |
| Unknown role with no matching role policy | Role remains observable metadata. Resolution continues to the legacy role-independent configured policy, if present, and then to coordinator/default inheritance. | A role-policy key is exact-match only; no fuzzy or alias match. |

An explicit effort value that the selected provider/model does not support is a
visible error for that task. It must not be silently downgraded. An unset effort
may use the provider's existing model-specific default after model selection.

A configured role policy is an operator-authored routing decision, so an invalid
policy is a configuration error. A temporarily unavailable configured route fails
the affected task and reports the reason. This preserves intent and prevents a
role policy from silently turning into coordinator execution.

Errors must expose actionable non-secret values such as role, requested model,
resolved model, provider label, and route kind. They must never expose credentials,
tokens, or secret configuration contents.

## 4. Protocol and planner contract

The additive task shape is:

```json
{
  "description": "Review the provider boundary",
  "prompt": "Inspect the route selection implementation and report risks.",
  "role": "reviewer",
  "model": "openai-oauth:gpt-5.5",
  "reasoning_effort": "high"
}
```

Protocol obligations:

- `description` and `prompt` retain their existing required behavior.
- `role`, `model`, and `reasoning_effort` are optional and default to unset.
- Serde defaults keep legacy task arrays valid when all new fields are absent.
- New planner instructions emit `role`, not `subagent_type`.
- Legacy planner output using `subagent_type` remains accepted through the
  compatibility alias described above.
- Planner output is data, not authority. The same resolver validates direct task
  requests and planner-generated tasks.
- A task's explicit model must reach the child-session creation path as the
  resolved model plus its matching route identity. It must not be reduced to a
  display label or reinterpreted by a second child-side resolver.
- The task response/status path must identify whether the child started with the
  requested or inherited selection and must surface failures before execution.

No new required wire fields, enum variants, or positional representations may be
introduced for this capability. Existing clients that do not know these fields
continue to send and receive the legacy shape.

## 5. Configuration contract

The future role-policy configuration is owned by `AgentsConfig` and uses one
canonical map:

```toml
[agents.swarm_role_policies.reviewer]
model = "openai-oauth:gpt-5.5"
reasoning_effort = "high"

[agents.swarm_role_policies.researcher]
model = "openrouter:anthropic/claude-sonnet-4-6"
```

Contract rules:

- The map is optional and absent by default, preserving current behavior.
- Keys are normalized role names and matched exactly after trimming. No fuzzy
  matching or prompt classification is part of this contract.
- Each policy field is optional. A policy may select only a model, only effort, or
  both.
- Task fields override the corresponding policy fields independently.
- The existing `agents.swarm_model` setting remains supported as the legacy
  role-independent configured model policy until a separately approved migration.
  During migration, it is part of the configured-policy tier: an explicit task
  model wins, a matching role policy wins for its configured fields, and
  `agents.swarm_model` fills an unset model field before coordinator inheritance.
  It is not renamed or reinterpreted by this bead.
- Environment/config precedence for the policy map follows the existing config
  loader contract. This bead does not introduce a second environment namespace.
- Diagnostics may show configured role names and non-secret model/effort values,
  but never credential values.

The implementation bead must decide and document the migration ordering between
`agents.swarm_role_policies` and the existing `agents.swarm_model` without
changing the canonical task precedence above. Until that migration is landed,
legacy `agents.swarm_model` is a configured model policy and never overrides an
explicit task model or a matching role-policy model.

## 6. Persistence and runtime evidence

### 6.1 Child session persistence

After resolution and before execution, the child `Session` persists:

- `model`: the effective model identifier,
- `provider_key`: the effective provider/profile key,
- `route_api_method`: the effective API/auth route,
- `reasoning_effort`: the effective effort when set.

These fields are the restore source. A reload or headless recreation must use the
persisted identity rather than re-reading the coordinator's current config and
silently changing the child route. Legacy sessions with absent fields remain
valid and continue using existing restore defaults.

### 6.2 Observed runtime evidence

The live swarm member status uses the existing `SwarmMemberRuntime` fields:

- `model`: the actual provider model after startup,
- `provider`: the actual provider/runtime display label,
- `auth_method`: the human-facing credential route such as `OAuth` or `API key`,
- `effort`: the actual effective effort,
- `elapsed_secs`: existing activity timing.

The status must report observed child state, not merely requested task fields. If
requested and observed values differ, startup fails for explicit model/effort
requests rather than publishing a misleading successful worker. Exact route
identity remains available through the persisted session's `provider_key` and
`route_api_method` and through structured lifecycle diagnostics where needed.

### 6.3 Trust-boundary order

The child startup sequence is:

1. normalize task fields and resolve precedence;
2. resolve the model to one `RouteSelection`;
3. validate route availability and credentials without exposing secrets;
4. create the child provider/session with the selected route;
5. verify actual model and effort;
6. persist session identity and publish runtime evidence;
7. only then run the task prompt.

A failure before step 7 is a failed task, not a fallback execution.

## 7. Focused test obligations for implementation beads

The downstream implementation must add focused tests for each contract boundary:

1. **Serialization compatibility:** legacy task JSON without new fields parses
   unchanged; canonical `role` fields round-trip; `subagent_type` compatibility
   works; conflicting role aliases fail visibly.
2. **Normalization:** absent, empty, and whitespace-only optional fields have the
   documented semantics.
3. **Precedence:** explicit task model/effort beats role policy, role policy beats
   coordinator inheritance, and inheritance beats the built-in default.
4. **Field-wise resolution:** a policy that sets only effort does not replace the
   inherited model or route.
5. **Route propagation:** a route-qualified model persists the matching
   `provider_key` and `route_api_method` and starts with the matching provider.
6. **Coordinator isolation:** child resolution never mutates the coordinator's
   model, route, or effort.
7. **Failure refusal:** invalid, unavailable, unsupported, and mismatched explicit
   selections fail before the prompt runs and do not run another model.
8. **Legacy config:** existing `agents.swarm_model` and legacy task requests keep
   their current inheritance behavior.
9. **Runtime evidence:** child startup status reports actual model/provider/auth/
   effort values, including the inherited case.
10. **Persistence/reload:** a child with a selected route restores the same
    identity even when process defaults or coordinator state differ later.

Documentation-only validation for this bead is link/path checking and a manual
trace against the current owners above. Runtime tests belong to the dependent
implementation beads and must reference this record rather than redefine policy.

## 8. Scope boundaries

In scope for this contract:

- one field vocabulary and one precedence chain;
- additive legacy compatibility rules;
- provider/auth route identity ownership;
- invalid/unavailable/mismatch failure semantics;
- persistence and observed runtime evidence obligations;
- downstream focused-test requirements.

Out of scope:

- implementing a router or changing provider implementations;
- prompt-LLM task classification or automatic role inference;
- changing the top-level/coordinator model automatically;
- changing external Codex plugins or compatibility adapters;
- UI redesign or new swarm controls;
- renaming/removing `agents.swarm_model`;
- changing the existing `ModelRoute`, `RouteSelection`, `Session`, or
  `SwarmMemberRuntime` wire shapes unless a later implementation requires an
  additive, separately reviewed change.

## 9. Downstream implementation rule

`jcode-l38` and later role-routing work must reference this document as the
single source of truth. They must not independently rename task fields, invent a
second precedence chain, derive provider/auth identity from display labels, or
silently substitute a model after an explicit request. If implementation reveals
an ambiguity in this record, stop and amend this contract in a focused change
before changing runtime behavior.
