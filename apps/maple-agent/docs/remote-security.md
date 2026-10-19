# Remote pairing security and qualification

## Authority

Each account/deployment/channel/storage scope owns a random persistent Iroh key
and local grants in an owner-only, atomically replaced file, separate from the
auth token cache. Changing builds does not change identity. Losing that file
requires re-pairing; discovery cannot reconstruct grants. Signing out invalidates
the account session and closes its remote connections. Same-account sign-in can
reuse persisted grants. Local revocation wins over delayed pairing acknowledgements.

The execution daemon opens a monotonic five-minute window W with eight uniform
Base32 characters (40 bits). It registers its endpoint, W and remaining duration
through authenticated Transport V2 with an endpoint-key signature before displaying
the code. OpenSecret returns a short-lived opaque admission handle bound to the
account, controller key, execution key and W. The execution daemon checks the
actual authenticated Iroh peer, asks OpenSecret to validate admission, then
rechecks and consumes its one local attempt before processing a PAKE message.

PAKE binds both roles/endpoint keys, account, project, deployment, channel, W and
protocol variant. The execution daemon confirms first. Only after verifying that
proof does the controller confirm. The execution grant is durable before its
acknowledgement. The controller persists that it verified the execution proof
before sending confirmation. A lost acknowledgement can query the existing grant;
it cannot retry the code, create a missing grant or undo revocation. A controller
also persists attempted windows before its code-derived message leaves the process.

A compromised enrollment service can deny service, redirect discovery, burn an
open window or make one online code guess. It cannot reset the daemon's local
attempt counter, learn the code from stored records or create a local grant.
A malicious database can disrupt availability and replay old valid ciphertext;
AEAD is not an anti-rollback system. Local windows, authenticated endpoint keys,
account generations and local grants remain authority under such replay.

## Selected PAKE and evidence

The implementation pins `pake-cpace` 0.1.0 from 1Password/cpace at
`7e2e42c7d359969f4356292c2bd2cf3ff6eeeecc` (initial public source release,
2026-06-04). It implements CPace Ristretto255/SHA-512 from draft-06 with HMAC
confirmation, **not** draft-21 or an RFC-conformance claim. The domain string
names this exact variant; changing it requires a new pairing protocol/version.
No local changes to the cryptographic construction are made.

Evidence assessed:

- The repository's eleven upstream tests, including its draft-06 vectors, pass
  at that exact pin. Maple's tests exercise correct confirmation order, every
  identity/context binding, wrong codes, reflection, malformed/noncanonical and
  identity group elements, wrong-length tags, expiry and one-attempt consumption.
- The included Cure53 report describes a December 2022 review by two testers
  over six days of approximately 750 lines including vectors. Its single
  informational Rust lifetime finding was reported fixed. The report does not
  identify this 2026 commit, and the public history begins at one release commit:
  there is no verifiable audited-commit-to-current diff. Underlying curve
  primitives were outside that audit. This is supporting construction/source
  evidence, not an audit of Maple or proof that this exact pin was audited.
- Source inspection confirms canonical Ristretto decoding, identity rejection,
  constant-time confirmation verification and CSPRNG input. Maple accepts only
  fixed eight-byte ASCII codes, avoiding unbounded password input to the draft-06
  generator; context fields come from bounded authenticated identities.
- Code buffers and returned session keys are zeroized where owned by Maple. The
  dependency's internal ephemeral scalar/intermediate key is not explicitly
  erased on drop. No claim is made of complete memory erasure or protection from
  a compromised local process. Codes, permits, PAKE messages and endpoint secrets
  are not diagnostic output or service database values.

This evidence qualifies the dependency for the scoped implementation/internal
trial. It does not substitute for a review of the complete concrete implementation
before broad release. A cryptographic dependency update must reassess the variant,
source/audit relationship, malformed inputs, vectors and confirmation ordering.

Sources: [1Password source](https://github.com/1Password/cpace),
[review report](https://github.com/1Password/cpace/blob/7e2e42c7d359969f4356292c2bd2cf3ff6eeeecc/assets/2022-Cure53-Audit.pdf),
[current CPace draft](https://www.ietf.org/archive/id/draft-irtf-cfrg-cpace-21.html).

## Exposure and validation limits

A paired controller has the ordinary host agent's tool authority, including shell
and computer use. No additional remote tool-approval system is introduced. Remote
API account/auth/billing/API-key/host-stop commands are excluded. Protocol versions,
identity scope, grant checks and account fences are checked before dispatch;
account-local receipt lookup cannot expose another account's outcome.

Tests on one machine can prove separate profiles, identity/authentication,
framing, public-relay traffic, cancellation, snapshot recovery and receipt behavior.
They cannot prove two independent NATs, sleep/wake routing, Windows/Linux native
lifecycle or signed macOS TCC identity. Record those as separate trial evidence.
