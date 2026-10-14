# Encrypted transcription validation

The runner at `scripts/validate-transcription.ts` sends whole files through the
in-tree TypeScript SDK, using its encrypted password login, attestation policy,
and `transcribeAudio` API. It is restricted to an explicitly selected HTTP
loopback origin. Local mock attestation is development evidence; it does not
prove deployed Nitro attestation, public-edge deadlines, or application recovery.

Use an isolated linked Local stack and its generated paid/free account manifest.
Initialize the stack, provider credentials, and account fixtures using the stack
owner's instructions. Run from the monorepo root, with the SDK's pinned Bun and
installed frozen SDK dependencies. No app dependency wiring changes are needed.

```sh
bun services/opensecret/scripts/validate-transcription.ts \
  --api-url http://127.0.0.1:PORT \
  --users /private/workspace/env/test-users.json \
  --corpus /private/corpus/harness-manifest.json \
  --output /private/evidence/transcription-run.json \
  --cases english-10min,spanish-clean --repeat 2 --auth-probes --billing-check
```

Replace placeholders with the actual linked stack values. Output must be a new
file outside the repository. It contains full transcripts and references and is
created with mode `0600`; avoid copying it into logs or commits. Stdout contains
case IDs, status, timing, word counts, WER, and anchor completeness only. The
runner suppresses SDK console diagnostics and never persists tokens, passwords,
API keys, or account identifiers. Temporary API keys are deleted after probes.

`--dry-run` reads selected fixtures without login or inference. Select `--all`
or `--cases ID[,ID]` explicitly. `--repeat N` repeats complete files; it never
retries a failed logical case. A request deadline aborts the SDK's network fetches
and remains active through response consumption. `--auth-probes` additionally
checks free-JWT and valid-API-key denial using the first selected audio file.
Use a small file first to keep those negative probes inexpensive and fast.

`--billing-check` reads the fixture's exact loopback `billing_url` and uses the
SDK to mint an audience-scoped third-party token for two billing status reads.
Before/after evidence retains only subscription state, plan name/status, API
credit balance, and monthly total/used credits. It fails if either credit pool
changes or required balances are unavailable. The bearer token and other billing
response fields are discarded. A dry run performs no billing requests. These
snapshots cover the selected account and observed interval; they do not establish
future asynchronous event behavior or production accounting.

`--response-format verbose_json` is an opt-in compatibility/diagnostic request
through the in-tree SDK's encrypted API helper. Every selected case must supply
its language. It adds the legacy `response_format` field while preserving the
usual base64 file, filename, MIME type, model, and authentication. Full returned
responses, including any segment timestamps, remain in the private evidence
file. The default `json` mode continues using `sdk.transcribeAudio` unchanged.

The corpus manifest is versioned separately from private account fixtures:

```json
{
  "version": 1,
  "cases": [
    {
      "id": "english-10min",
      "audio": "english-10min.wav",
      "content_type": "audio/wav",
      "language": "en",
      "reference": "english-10min.txt",
      "duration_seconds": 600,
      "anchors": {
        "begin": "A distinctive phrase heard at the beginning",
        "middle": "A different phrase heard near the middle",
        "end": "The closing words actually heard at the end"
      },
      "expected": "transcript",
      "max_wer": 0.25,
      "min_anchor_score": 0.8
    }
  ]
}
```

Audio/reference paths resolve relative to the corpus manifest. An optional
`tags` array documents fixture provenance or categories. Record public dataset
source, license, original IDs, transformations, and reference boundaries beside
the corpus; a stitched public speech fixture is not a spontaneous continuous
human rant. The runner records SHA-256, file bytes, and parsed WAV duration for
each upload. For other formats, provide `duration_seconds` measured independently.

WER uses Unicode NFKC, case/punctuation normalization, and locale-aware word
segmentation. It does not reconcile numbers written as digits versus words.
Approximate begin/middle/end anchor matches must occur in order; this catches
missing tails that a nonempty output check misses. Default acceptance thresholds
are WER at most `0.25` and each supplied anchor score at least `0.8`. Set fixture
thresholds deliberately before running; do not relax them merely to hide failure.
These metrics check completeness and transcription accuracy, not speaker labels,
semantic correctness, or every word's timestamp.

`anchors_present` requires every supplied phrase to meet its completeness
threshold. `anchors_in_order` compares the matched phrases' start offsets and is
unknown (`null`) when an offset is unavailable. A phrase below its threshold
fails completeness without claiming that phrases were reordered. Evidence from
earlier runner versions used `anchors_in_order` for the combined presence and
ordering check; inspect those older scores/offsets rather than interpreting
`false` as proof of reordering. Existing evidence files are never rewritten.

Negative cases use `expected: "rejection"` and `expected_status: 400` (or an
array of allowed statuses), with optional `expected_error_code` for the stable
application error. Auth probes require both HTTP `403` and the specific
`transcription_paid_required` / `transcription_app_only` codes. Use
`expected: "no_speech"` for silence: empty text
or the explicit `transcription_no_speech` error is accepted, while hallucinated
text fails. `--account free --expect-status 403` overrides selected cases for
paid-only gating. Add malformed WAV, silence, and over-duration fixtures to the
manifest rather than adding validation bypasses to the SDK or server.

Interpret source/unit checks, encrypted Local provider inference, billing
inspection, public-edge behavior, and deployed behavior as separate evidence.
Inspect the private JSON for individual failures and provider usage returned by
the server. A successful SDK call alone does not prove the selected model,
provider, accurate billing, or production reliability; correlate with bounded,
content-free server/provider and billing evidence.
