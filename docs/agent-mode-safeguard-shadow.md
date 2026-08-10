# Agent Mode GPT-OSS Safeguard shadow

**Status:** Research-only, opt-in shadow experiment

Maple can synchronously send two Agent Mode safety checks to Tinfoil's hosted
`gpt-oss-safeguard-120b` model:

1. a bounded set of text tool results at the untrusted-output boundary, before Maple sends them to
   the primary model; and
2. bounded projections of model-proposed tool calls, before Goose routes them for approval or
   execution.

The experiment is deliberately observational. A verdict, timeout, malformed response, failed
attestation, or request failure never changes the tool result, permission decision, or proposed
call. The synchronous wait is intentional: the first question is whether the added latency feels
acceptable on every covered boundary.

## Enable it

Build without the credential, then use the dedicated runner. It prompts without echo only after
Nix, Tauri, Cargo, frontend hooks, and ONNX Runtime provisioning have finished:

Fully quit any Maple instance already running under this managed workspace's app identity first.
Maple is single-instance: launching the runner while that process is still alive would only focus
the existing process, which cannot inherit the new gate or credential.

```sh
unset TINFOIL_API_KEY
nix develop -c just install
nix develop -c frontend/src-tauri/scripts/run-safeguard-shadow.sh
```

Do not export the key around `just desktop-dev`: Nix, Bun, Vite, Cargo, and build hooks run before
Maple and would inherit it. Do not put it in `frontend/.env.local`; that file is development
configuration, not secret storage. The runner refuses an inherited key, builds a checkout-local
debug binary using the managed workspace's Tauri config when present, completes runtime
provisioning, and only then reads the key in the final launcher shell and immediately replaces that
shell with Maple. Maple's desktop entrypoint then captures and removes `TINFOIL_API_KEY` before
Tauri, Tokio, ACP, logging, or any Agent runtime, shell, or MCP subprocess can start—even when the
gate is absent or misspelled—so Agent tools cannot inherit it. Classifier traffic requires the
explicit gate and a nonblank key at startup. Changing either requires an app restart.

Optional process-start settings:

| Variable | Allowed values | Default |
| --- | --- | --- |
| `MAPLE_SAFEGUARD_TIMEOUT_MS` | `1000` through `60000` | `20000` |
| `MAPLE_SAFEGUARD_REASONING_EFFORT` | `low`, `medium`, `high` | `low` |
| `MAPLE_SAFEGUARD_TEMPERATURE` | finite number from `0` through `2` | omitted |

The model, 4,096-token completion bound, output schemas, and two policy prompts are compiled
constants. The SDK discovers a router endpoint and selects the latest signed router release at
runtime. Policy changes must also change their version constants so observations remain
interpretable.

## Privacy and observations

Maple uses the pinned first-party Tinfoil Rust SDK. Client construction verifies the confidential
router's attestation and expected configuration-repository identity before the first classification.
The verified router is responsible for chained verification of the selected model backend; Maple is
not directly attesting a model enclave. The experiment currently accepts the latest signed router
release selected by the SDK rather than a Maple-pinned release digest. Maple does not fall back to
unattested REST.

Tinfoil receives the bounded trusted kickoff prompt, working-directory path, source tool name and
projected tool-output text for the input lane. For the action lane it receives the bounded trusted
kickoff prompt, working directory, proposed tool name and arguments, plus the matching description,
input schema, and annotations as untrusted claims. These payloads are protected by the attested
encrypted route, but they do leave the local Maple process; this is not an all-local classifier.

Maple emits one metadata-only `provider_preparation` line for each guarded primary stream after its
bounded context and tool catalog are ready, including text-only streams. It reports separate kickoff,
context, and catalog preprocessing times, separate exhaustion flags for those stages, and whether the
owning run was cancelled. The kickoff time is the one-time enabled-run projection and repeats for
that run; context time measures reconstruction for this provider call; catalog time measures
construction after the primary response has started but before Maple yields its first item. These
fields overlap neither one another nor hosted evaluation time, but the kickoff field must not be
summed repeatedly across a run.

Maple also emits one metadata-only `lane_preparation` line per untrusted-input scan, including scans
that schedule no hosted evaluation. It reports local preprocessing time, scheduled-evaluation count,
preprocessing exhaustion, and cancellation. A normal observation follows for each hosted evaluation.
Candidate- or evaluation-count exhaustion emits a separate `coverage_budget_exhausted` summary with
the actual `limit_kind` and `limit`, while preprocessing exhaustion emits a
`preprocessing_budget_exhausted` summary. When preprocessing prevents every hosted evaluation, that
summary and the input-lane preparation line are the covered boundary's only lane logs; the per-stream
provider-preparation line is separate.

The normal observation contains random opaque boundary and evaluation-group IDs, the lane, process
experiment ID, policy version, fixed result category, parsed verdict/category, cold-or-warm client
phase, `total_ms`, `boundary_elapsed_ms`, `queue_ms`, `request_ms`, `client_init_wait_ms`, the three
provider-preparation times, `lane_preprocessing_ms`, bounded input character count, chunk metadata,
truncation flag, and token counts—including cached prompt tokens—when returned. `total_ms` starts
when one hosted evaluation begins. `boundary_elapsed_ms` starts before input-batch construction or,
for actions, after scanning the first action-bearing stream item and before payload construction; it
can include multiple
evaluations, later primary-stream generation, and their waits. `lane_preprocessing_ms` is the input
batch's local work or the action stream's cumulative active local work at that payload. The timing
fields overlap and must not be summed. A preprocessing-exhaustion summary reports the exhausted
stage, its observed elapsed time, and the configured caps. If exhaustion happens before Maple
establishes that a classifier-eligible payload exists, its deferred/omitted/retryable fields are
`unknown` rather than making a coverage claim.

The IDs are generated locally and are not derived from request or session identifiers. They let
multi-chunk results and concurrent boundaries be grouped without logging payload provenance. An
observation must not contain the user prompt, working directory, tool name or arguments, tool output,
request ID, model reasoning, raw response, raw error, API key, or cache secret.

The first verified-client log additionally records the public router repository, release, digest,
selected endpoint, code and enclave fingerprints, and attestation duration. The model name reported
by each response must exactly match the requested model, but that check is only a routing sanity
check. `request_ms` covers the complete non-streaming safeguard request; it is not time to first
token.

The SDK cache namespace is derived from a random process seed and Maple's opaque account scope, so
accounts do not share prefix-cache timing, no user-cache secret is written to disk, and prefix-cache
reuse starts over after an app restart. If an unexpected stream path lacks account provenance,
Maple uses a fresh one-shot cache namespace rather than sharing an unscoped namespace. The verified
router client is cached for the process and does not periodically re-attest. First-use attestation
is driven by a service-owned, bounded task, so canceling the initiating Agent run does not pause its
timer or inflate the next run's cold-start measurement. An initialization error is cached; restart
Maple before repeating an experiment after persistent attestation, key-rotation, or transport
failures.

## Exact prototype coverage

The hook lives only in Maple's interactive provider `stream` path:

- It projects at most 64 previously unledgered newest `ToolResponse` occurrences per primary-model
  call. Ledger hits are skipped from occurrence metadata without re-reading or hashing the raw
  result. From the projected set it chooses newest-first, never schedules part of an output, and
  sends at most eight hosted evaluations total with four in flight across the Maple process. That is
  at most two configured evaluation-timeout waves; each evaluation's one deadline includes waiting
  for the global permit, first-use client verification, and its model request. Deferred candidates
  remain unmarked and can be checked on a later provider call, although continual newer results can
  starve older backlog.
- A bounded process-memory ledger fingerprints the opaque account scope, session, current bounded
  trusted request, working directory, and exact response occurrence metadata. Only outputs for
  which every chunk returned a valid classifier verdict enter the ledger. Exact Goose retries do
  not repeat those successful classifications; failures remain retryable, and newly appended or
  rebuilt response occurrences are checked again. Outputs whose projection contains no
  classifier-eligible text are terminally skipped in the same ledger so they cannot permanently
  hide older text backlog. Missing account/session provenance disables shared deduplication. The
  ledger is not persisted across app restarts.
- Tool content follows the pinned Goose OpenAI projection: direct text is retained verbatim; text
  resources receive Goose's Unicode normalization/tag filtering; images and binary resources use
  the same placeholders; audio, resource links, other non-text blocks, structured content, and
  protocol metadata are omitted. The bounded projection keeps the head and exact suffix, omits the
  middle above 190,464 characters, and produces no more than four overlapping chunks. Embedded
  Base64 resources above 1 MiB encoded size are not decoded and use a fixed omission marker.
- It correlates a tool result to the earlier model call ID to include the source tool name when that
  provenance is still available.
- It checks up to eight successfully parsed `ToolRequest`s across one primary Maple response stream,
  with at most four hosted evaluations in flight across the Maple process. Unless the owning Agent
  run is cancelled, each original stream item is yielded unchanged; cancellation before polling an
  item or while an action shadow check holds it returns Maple's cancellation error instead of the
  buffered item. Later streamed messages share the same eight-evaluation allowance and opaque
  boundary ID. Actions beyond that allowance are omitted from classification, are not retryable, and
  emit one `coverage_budget_exhausted` summary for the stream with `payloads_deferred=false`,
  `classifications_omitted=true`, and `retryable=false`; subject to cancellation, they otherwise
  continue downstream once. The envelope includes a bounded tool name, plus
  streaming head-and-tail JSON projections of arguments (32,000 bytes) and the matching description,
  input schema, and annotations (16,000 bytes) as explicitly untrusted claims. Maple builds that
  classifier-specific tool-definition catalog under a source-work cap instead of cloning the full
  MCP tool catalog; the top-level display title, output schema, icons, and protocol metadata are not
  copied into the safeguard path. An annotation title, when present, remains part of the explicitly
  untrusted annotations projection.
- The action lane receives the Maple kickoff message through a native task-local as its trusted user
  request. When the experiment is enabled, Maple projects that request to a bounded head-and-tail
  snapshot once before entering the provider; disabled runs do not materialize safeguard context.
  If kickoff projection exhausts its preprocessing budget, Maple preserves that state even when no
  bounded text or message ID survives and carries it through every provider call in the Agent run.
  Each affected input boundary is skipped with a preprocessing-exhaustion summary; a response stream
  that later contains a valid proposed call similarly skips the action lane and emits its summary
  once. Agent-visible MCP prompt messages are not elevated to trusted context merely because they
  carry a user role. A separate per-run marker records when the model has proposed a valid tool call,
  so Goose compaction or cancellation recovery dropping the kickoff message ID does not promote old
  tool history into the current run or erase the normal post-tool signal.
- Auxiliary `complete` calls are intentionally excluded. Those calls include compaction and other
  internal classifiers; scanning them would create false action checks and possible recursion.
- Preprocessing checks cancellation at stage checkpoints while traversing history, tool definitions,
  tool content, and proposed calls. The bounded kickoff projection, per-provider-call turn-context
  reconstruction, tool-definition catalog, and untrusted-output batch are separate stages, each with
  its own one-second wall-clock window and 8 MiB/65,536-item source-work allowances. Once the first
  valid proposed call activates the action stage, streamed-content scanning and action-payload
  serialization share stream-wide source-byte and item counters, one cumulative one-second
  active-work allowance, and a sticky exhaustion state. Hosted-classifier waits do not consume that
  active-work allowance. When Maple has established an affected tool-output candidate, input-lane
  exhaustion leaves it unledgered, deferred, and eligible on a later provider call; otherwise the
  summary records an unknown payload disposition. Action-lane exhaustion omits affected and
  remaining action classifications for that stream, is not retryable, and emits one
  `preprocessing_budget_exhausted` summary with `payloads_deferred=false`,
  `classifications_omitted=true`, and `retryable=false`. Subject to cancellation, proposed actions
  still continue downstream once. One hard per-evaluation deadline covers process-global queueing,
  first-use client verification, and the model request.
- If action pre-scan work exhausts before Maple has recognized a valid call, it emits one
  unknown-disposition preprocessing summary rather than silently claiming an omission. A valid call
  recognized later in the stream remains unclassified under that exhausted budget, emits the
  omitted/nonretryable summary once, and still sets the per-run post-tool signal before continuing
  downstream, subject to cancellation. Recognition after exhaustion uses a separate stream-wide,
  tag-only scan budget capped at one second and 65,536 content items; it inspects only content kinds
  and tool-call parse status, never the call name, arguments, or schema.

This is useful for latency and policy-quality research, but it is not a universal enforcement
boundary. It does not currently cover:

- Goose's `!command` shell shortcut or calls synthesized after the provider;
- tool-shim-generated calls, direct ACP dispatch, or nested platform-tool dispatch;
- reliable provenance and owning-run cancellation for detached Goose `delegate`/subagent provider
  streams. Those streams retain the provider-level hooks but do not inherit Maple's task-local
  account scope, trusted kickoff, cancellation token, or post-tool marker; they therefore use the
  unscoped one-shot cache namespace, cannot share result deduplication, and retain the parent provider
  working directory rather than a delegate-specific one;
- MCP initialization instructions, slash-command prompt content, or other untrusted content whose
  provenance is elevated or lost before the provider call (matching tool descriptions/schemas are
  visible only to the proposed-action lane, not independently injection-scanned);
- instructions encoded only in image, audio, binary, or other non-text tool content;
- deferred tool-result candidates (including preprocessing-budget exhaustion), omitted middles of
  very large projected results, and embedded Base64 resources above the decoding bound;
- original tool results that Goose replaced with a large-response file notice before inference; or
- deterministic authorization facts such as resolved paths, actual capabilities, sandbox state,
  credentials, and remote side effects that are not present in the proposed call envelope.

A production guard needs lower Goose-level input and action seams, deterministic capability policy,
and an explicit fail-open/fail-closed decision. Shadow verdicts must not be described as approvals
or proof that content is safe.

## Reading the experiment

Compare cold-client and warm-client observations separately. For each lane, collect at least hosted
evaluation latency, boundary latency, local preprocessing times, timeout/failure rate, parsed
decision distribution, input size, and token usage. Include the per-stream provider-preparation and
input-lane preparation lines when judging first-item feel, including text-only or no-payload turns.
Review false positives on code, READMEs, logs, quoted security material, and legitimate read-only
tools; review misses on indirect instructions, exfiltration attempts, obfuscation, pipelines,
redirects, network access, and destructive commands.

Do not select thresholds or enforcement behavior from the model's verdict labels alone. This first
experiment returns closed categories but no independently calibrated risk probability.

## Build footprint

The feature gate is runtime-only. Every desktop build currently compiles and links the pinned
Tinfoil SDK and its attestation/cryptography dependencies even when the experiment is disabled;
iOS and Android do not include it. Before production distribution, Maple must explicitly decide
whether to keep that supply-chain and bundle footprint, run the cross-platform desktop build matrix,
and include the Tinfoil (Apache-2.0) and EHBP (MIT) license texts in its attribution process.
