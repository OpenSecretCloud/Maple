---
name: review-opensecret-security
description: Review security-sensitive OpenSecret changes and claims. Use when a diff reaches attestation, encrypted sessions, authentication or OAuth, key ownership, encrypted persistence, provider or web trust, external billing or flag APIs, secrets, logs, Nitro/KMS/PCR evidence, or another boundary where source, artifact, and deployed-environment claims must be separated.
---

# Review OpenSecret security

Review the current source and diff as evidence. This skill defines a method; it
does not catalogue current findings. Default to read-only review and implement
changes only when the user asks.

## Establish the comparison

1. Read `AGENTS.md`, relevant source, tests, migrations, and public docs.
2. Confirm the revision, intended base, worktree state, and submodule revisions.
   Do not rewrite user work to manufacture a clean comparison.
3. Inspect committed, staged, unstaged, untracked, generated, lockfile,
   workflow, Nix, environment, migration, and submodule changes. For an
   `origin/master` comparison, include:

   ```sh
   git status --short --branch
   git diff origin/master...HEAD --
   git diff --cached --
   git diff --
   git ls-files --others --exclude-standard
   git submodule status
   git diff --check origin/master...HEAD --
   git diff --cached --check
   git diff --check
   ```

4. Trace each changed value from input through authority, decryption,
   persistence, external calls, response encryption, errors, and logs.

Classify each observation as introduced, worsened or newly relied upon,
pre-existing baseline, unrelated, or indeterminate. Only introduced or
materially worsened/newly relied-upon behavior determines a scoped diff verdict.
Mention baseline behavior only when needed to explain the change; keep it
task-local and non-blocking unless the change depends on it, expands it, or the
user requested a repository-wide audit. Mark a verdict provisional when the
comparison base is uncertain.

## Label the evidence

- **source-confirmed**: the reviewed revision directly establishes the claim.
- **test-confirmed**: a named test exercised it in this run.
- **build-confirmed**: a named reproducible build or artifact check passed.
- **live-confirmed**: the named deployed or external environment was exercised.
- **inferred**: the claim follows from stated evidence and assumptions.
- **unverified**: the required source, system, or environment was not inspected.

Local source and tests do not establish live database transport, provider
behavior, billing decisions, logging policy, IAM/KMS policy, PCR trust, artifact
identity, or which revision serves an environment.

## Map the boundary

Identify every link the change crosses:

- client to the OpenSecret enclave and encrypted session;
- router to user, API-key, OAuth, project, and record authority;
- enclave plaintext to host-visible persistence, metadata, errors, or logs;
- OpenSecret to external model, web, OAuth, email, billing, or flag APIs;
- enclave to its parent over VSOCK for credentials, secrets, and logs;
- source and Nix build to EIF/PCR evidence, KMS policy, and deployment.

Client attestation of OpenSecret and OpenSecret’s attested connection to an
upstream enclave prove different links. Local mock attestation proves protocol
shape, not Nitro or production trust.

## Review end to end

For the changed boundary, answer:

1. What authenticated identity or capability authorizes the operation, and
   where is project/record ownership enforced?
2. Who owns each key, plaintext value, persisted row, provider credential, and
   policy decision? Is that authority ever taken from untrusted input?
3. Where are size, count, time, concurrency, expiry, replay, cancellation, and
   cleanup bounds enforced on success and failure paths?
4. What crosses into host-visible storage, metadata, logs, errors, parent
   services, or external APIs? Is it necessary, bounded, and sanitized?
5. Can client, model, provider, database, or parent-instance data gain URL,
   identity, routing, key, or execution authority?
6. Are ambiguous retries, partial streams, disconnects, and restarts safe for
   persistence, usage, and external side effects?
7. Does the change alter a shared SDK/client protocol, persisted format,
   provider contract, or deployment trust claim? What compatibility and
   rollback path is required?

Apply these OpenSecret-specific invariants:

- An encryption session is transport state, not identity or authorization.
- JWT, API-key, OAuth, user-key, and enclave/system-key domains are not
  interchangeable.
- Ownership checks precede decryption or mutation; query scoping is part of
  authorization.
- Provider and model output is untrusted. Provider choice, credentials, cache
  namespaces, URL provenance, and SSRF policy remain backend-owned.
- Sensitive or user-controlled plaintext must not enter logs or public errors.
  Safe metadata is bounded and allowlisted.
- A changed ciphertext format needs explicit versioning, compatibility,
  rollback, and access to the owning key; ordinary startup lacks user keys.
- Shared protocol changes require coordinated review of the SDK source under a
  selected Maple checkout's `sdk/` directory and the dependency actually
  resolved by each affected Maple application path. Treat
  `frontend/package.json` as authoritative for the TypeScript client; the Rust
  client remains on its published crate until the coordinated proxy/Rust
  switch.

Use `$change-opensecret-api` or `$change-opensecret-provider` for the detailed
contract procedure rather than duplicating it here.

## Match claims to proof and authority

Keep the evidence ladder separate:

1. Rust tests establish local implementation behavior.
2. Nix checks establish reviewed source/build invariants.
3. An EIF build plus comparison with a reviewed PCR reference establishes an
   artifact measurement.
4. Inspection of live KMS/IAM, client trust policy, deployed EIF, parent
   services, and runtime smoke establishes deployment behavior.

Load `$validate-opensecret` and run the tiers reached by the diff. Report local,
database, provider, client, build/artifact, and live evidence separately.

Local artifact builds and read-only PCR comparison are validation when in
scope. A green pull-request EIF build is not PCR evidence; GitHub Actions
skips PCR comparison on pull requests and still verifies on master and
`workflow_dispatch`. Require explicit authorization for PCR
reference/history mutation, signing, KMS/IAM changes, shared or remote
migrations, artifact transfer, enclave or remote-service lifecycle, secret
writes, staging, deployment, or release actions. Inspect recipes before
deciding whether they are read-only.

## Report the review

Lead with the verdict and prioritized diff findings. For each finding, state:

- evidence class and exact file or symbol;
- affected boundary and required preconditions;
- concrete impact without incident language;
- invariant or design change required;
- regression proof and client/deployment coordination still needed.

Then list commands run, omitted or unavailable checks, and residual uncertainty.
If there are no findings, name the boundaries reviewed without implying that
uninspected systems are secure. Keep revision-specific observations in the
review output; promote only durable methods back into repository guidance.
