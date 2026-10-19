# Dedicated Maple inference

OpenSecret can serve the existing canonical `deepseek-v4-1-flash` model through
the internal `maple` provider. Clients continue to select public model IDs;
they cannot choose a provider, endpoint, release or measurement. Shared
`tinfoil` and `continuum` routes retain their existing behavior.

## Configuration and rollout

`MAPLE_INFERENCE_API_KEY` is optional and independent of `TINFOIL_API_KEY`.
Missing credentials disable only the dedicated provider. There is no fallback
to a shared-provider credential. Resolve local credentials through the
service-owned [SecretSpec procedure](local-macos-stack.md), and use the
[Nitro runtime contract](nitro-deploy.md#optional-dedicated-inference-api-key)
for an authorized enclave rollout. Do not put keys in generated env files.

The `maple_inference` boolean flag is resolved for the authenticated account
at Chat Completions/Responses entry. Missing configuration, absent flags,
failed requests and wrong-account responses deny dedicated eligibility.
An unconfigured dedicated provider needs no extra flags request. Eligibility
is captured for the logical request and inherited by helpers and tool turns;
caller JSON and sticky memory cannot grant it.

Flagged accounts prefer verified Maple capacity for DeepSeek, including accounts
with an older shared-provider preference. Existing same-model shared routes
remain fallback under normal route, capacity and transport-readiness gates.
Unflagged requests retain the existing routing policy. No additional node or
GPU-level sticky layer is configured: the existing provider identity separates
health, capacity, accepted-route stickiness and usage for this single endpoint.

The existing flags client caches evaluations for ten minutes. A flag update
does not instantly revoke cached membership or interrupt an accepted stream.
Model entitlements, credit checks and public model visibility remain separate
from provider eligibility. Keep the flag default off until credentials,
billing and deployment/network prerequisites are qualified.

## Attestation and staged releases

`src/maple_inference.rs` owns the fixed HTTPS origin and current/staged
`ApprovedRelease` entries. Every entry pins repository, release tag, artifact
digest and the complete expected measurement. DNS, API keys, remote document
hints and flags cannot extend that allowlist. Stage a reviewed additional entry
in OpenSecret before changing the GPU release; retain the old entry during the
transition and remove retired entries in a subsequent reviewed change.

The pinned Rust SDK fork provides explicit-host V3 verification with current
Format V2 collateral and direct attested TLS. Startup and recovery use the
same immutable endpoint/policy. There is no automatic shared-router discovery
or TLS/attestation downgrade in the direct client. V3 is a vendor beta path;
the client binds GPU evidence into the CPU quote and relies on the measured
runtime's GPU verification, rather than independently appraising NVIDIA
signatures.

Initialization and renewal are body-free and single-flight. An unverified or
expired client is excluded from routing while background renewal runs.
Renewal is lazy at expiry, so requests may use shared capacity during recovery.
The SDK enforces its authenticated freshness deadline on dispatch as well as
on route readiness. Verification does not renew an old witness's lifetime.
Already accepted Responses tool turns retain their route and can fail locally
if it becomes unavailable; they do not silently switch providers.

Raw request/response streaming retains the existing attested transport contract.
Only a typed connector failure proven to precede HTTP acceptance permits the
existing bounded same-endpoint retry. An ambiguous timeout/reset or partial
stream is not replayed merely because attestation expired concurrently.

## Usage and validation

The direct vLLM endpoint consumes the native `cache_salt` field. Tinfoil's
shared router translates `user_cache_secret` into that field, but the direct
shim/sidecar does not. OpenSecret therefore strips both caller-supplied fields
and sends a backend-derived, user-specific Maple namespace as `cache_salt`.
Shared Tinfoil keeps `user_cache_secret`, and Continuum keeps its own salted
namespace. V2 namespaces are stable for the authenticated user and private
client root; legacy Maple namespaces use a process-secret root and cold-start
on backend restart. The namespaces are separate across users and providers.
This follows the pinned engine's
[cache schema](https://github.com/vllm-project/vllm/blob/db9527a46873454610df6dbedf79a36d6bf1a7f6/vllm/entrypoints/openai/chat_completion/protocol.py#L456-L467).

Usage retains the canonical public model and actual `maple` provider label.
Customer prices, cached-input discounts, quotas and subscription/API credit
selection remain unchanged. The billing consumer must recognize this pair
before enabling the route: its zero incremental token cost excludes the fixed
GPU lease. OpenSecret's deprecated producer cost estimate is not authoritative
billing or total infrastructure expense.

Use focused routing, flags, cache and transport tests, then the full backend
Rust gates and current-host Nix checks. The synthetic entrypoint credential
test is included in `nix flake check`. Local mock tests do not qualify a real
credential, deployed GPU cache behavior, Nitro VSOCK forwarding or production
attestation from the enclave. Those require separately scoped integration and
deployment evidence. Do not run inherited live-provider tests as part of an
ordinary local validation run.
