#!/usr/bin/env bun
/** Encrypted, loopback-only transcription validation using the in-tree SDK. */
import { createHash } from "node:crypto";
import { writeSync } from "node:fs";
import {
  readFile,
  realpath,
  mkdir,
  open,
  type FileHandle,
} from "node:fs/promises";
import { basename, dirname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

type Account = "pro" | "free";
type AnchorName = "begin" | "middle" | "end";
type CorpusCase = {
  id: string;
  audio: string;
  content_type?: string;
  language?: string;
  reference?: string;
  anchors?: Partial<Record<AnchorName, string>>;
  expected?: "transcript" | "no_speech" | "rejection";
  expected_status?: number | number[];
  expected_error_code?: string;
  max_wer?: number;
  min_anchor_score?: number;
  duration_seconds?: number;
  tags?: string[];
};
type Corpus = { version: number; cases: CorpusCase[] };
type Users = {
  version: number;
  workspace: string;
  client_id: string;
  billing_url?: string;
  accounts: Record<
    Account,
    { email: string; password: string; user_id: string }
  >;
};
type Options = {
  apiUrl: string;
  users: string;
  corpus: string;
  output: string;
  cases: string[];
  all: boolean;
  repeat: number;
  account: Account;
  expectStatus?: number;
  authProbes: boolean;
  billingCheck: boolean;
  responseFormat: "json" | "verbose_json";
  dryRun: boolean;
  timeoutSeconds: number;
};
type AnchorMatch = {
  score: number;
  token_offset: number | null;
  token_length: number;
};
type Metrics = {
  reference_words: number;
  hypothesis_words: number;
  normalized_wer: number | null;
  anchors: Partial<Record<AnchorName, AnchorMatch>>;
  anchor_score: number | null;
  anchors_present: boolean | null;
  anchors_in_order: boolean | null;
};
type BillingSnapshot = {
  is_subscribed: boolean | null;
  product_name: string | null;
  subscription_status: string | null;
  api_credit_balance: number | null;
  total_tokens: number | null;
  used_tokens: number | null;
};
type Result = {
  case_id: string;
  repeat: number;
  account: Account;
  auth: "jwt" | "api_key";
  expected: CorpusCase["expected"];
  status: "pass" | "fail" | "dry_run";
  http_status: number | null;
  error_code: string | null;
  error_message?: string;
  latency_seconds: number;
  outer_requests: number;
  audio_bytes: number;
  audio_sha256: string;
  audio_duration_seconds: number | null;
  text?: string;
  reference?: string;
  usage?: unknown;
  response?: unknown;
  metrics?: Metrics;
  failures: string[];
};

const help = `Encrypted transcription validation (Bun; in-tree SDK dependencies required)

  bun services/opensecret/scripts/validate-transcription.ts \\
    --api-url http://127.0.0.1:PORT --users /private/test-users.json \\
    --corpus /corpus/harness-manifest.json --output /private/evidence.json \\
    --cases english-10min,spanish-clean [--repeat 2] [--auth-probes] [--billing-check]

Required: --api-url, --users, --corpus, --output, and --cases ID[,ID] or --all.
Options: --account pro|free (pro), --expect-status N (override case expectation),
         --timeout-seconds N (360), --repeat N (1), --dry-run, --auth-probes,
         --billing-check (before/after credit-pool snapshots from local billing),
         --response-format json|verbose_json (json; verbose requires language).
Dry run reads fixtures and references without authenticating or sending audio.
Auth probes use the first selected audio with a free JWT and a temporary valid
Pro API key (created and deleted using the SDK). Both must return HTTP 403.
Evidence includes full transcripts; it must be outside the repository and is
created exclusively with mode 0600. Stdout contains safe summary metadata only.
`;

function parseArgs(args: string[]): Options {
  const values = new Map<string, string>();
  const flags = new Set<string>();
  const booleans = new Set([
    "--all",
    "--auth-probes",
    "--billing-check",
    "--dry-run",
  ]);
  const known = new Set([
    "--api-url",
    "--users",
    "--corpus",
    "--output",
    "--cases",
    "--repeat",
    "--account",
    "--expect-status",
    "--timeout-seconds",
    "--response-format",
  ]);
  for (let i = 0; i < args.length; i++) {
    const name = args[i];
    if (name === "--help" || name === "-h") {
      writeSync(process.stdout.fd, help);
      process.exit(0);
    }
    if (booleans.has(name)) flags.add(name);
    else {
      if (!known.has(name) || !args[i + 1] || args[i + 1].startsWith("--")) {
        throw new Error("Unknown option or missing argument; use --help.");
      }
      values.set(name, args[++i]);
    }
  }
  for (const name of ["--api-url", "--users", "--corpus", "--output"]) {
    if (!values.get(name)) throw new Error(`Missing ${name}; use --help.`);
  }
  if (flags.has("--all") === values.has("--cases")) {
    throw new Error("Select exactly one of --all or --cases.");
  }
  const positiveInteger = (
    name: string,
    fallback: number,
    maximum: number,
  ): number => {
    const n = Number(values.get(name) ?? fallback);
    if (!Number.isInteger(n) || n < 1 || n > maximum) {
      throw new Error(`Invalid ${name}.`);
    }
    return n;
  };
  const account = values.get("--account") ?? "pro";
  if (account !== "pro" && account !== "free")
    throw new Error("Invalid --account.");
  const url = new URL(values.get("--api-url")!);
  if (
    url.protocol !== "http:" ||
    !["localhost", "127.0.0.1", "[::1]"].includes(url.hostname) ||
    url.username ||
    url.password ||
    url.search ||
    url.hash ||
    url.pathname !== "/"
  ) {
    throw new Error("This harness requires an exact HTTP loopback API origin.");
  }
  const expectStatus = values.has("--expect-status")
    ? positiveInteger("--expect-status", 403, 599)
    : undefined;
  if (expectStatus !== undefined && expectStatus < 400) {
    throw new Error("--expect-status must be an HTTP error status.");
  }
  const responseFormat = values.get("--response-format") ?? "json";
  if (responseFormat !== "json" && responseFormat !== "verbose_json") {
    throw new Error("Invalid --response-format.");
  }
  return {
    apiUrl: url.origin,
    users: resolve(values.get("--users")!),
    corpus: resolve(values.get("--corpus")!),
    output: resolve(values.get("--output")!),
    cases: (values.get("--cases") ?? "").split(",").filter(Boolean),
    all: flags.has("--all"),
    repeat: positiveInteger("--repeat", 1, 100),
    account,
    expectStatus,
    authProbes: flags.has("--auth-probes"),
    billingCheck: flags.has("--billing-check"),
    responseFormat,
    dryRun: flags.has("--dry-run"),
    timeoutSeconds: positiveInteger("--timeout-seconds", 360, 3600),
  };
}

function tokens(text: string, language?: string): string[] {
  const normalized = text
    .normalize("NFKC")
    .toLocaleLowerCase()
    .replace(/[’']/gu, "")
    .replace(/[^\p{L}\p{N}\s]/gu, " ");
  // Segmenting words also permits meaningful multilingual/CJK measurements.
  return Array.from(
    new Intl.Segmenter(language, { granularity: "word" }).segment(normalized),
  )
    .filter((part) => part.isWordLike)
    .map((part) => part.segment);
}

function editDistance(reference: string[], hypothesis: string[]): number {
  let previous = Uint32Array.from(
    { length: hypothesis.length + 1 },
    (_, i) => i,
  );
  let current = new Uint32Array(hypothesis.length + 1);
  for (let r = 1; r <= reference.length; r++) {
    current[0] = r;
    for (let h = 1; h <= hypothesis.length; h++) {
      current[h] = Math.min(
        previous[h] + 1,
        current[h - 1] + 1,
        previous[h - 1] + Number(reference[r - 1] !== hypothesis[h - 1]),
      );
    }
    [previous, current] = [current, previous];
  }
  return previous[hypothesis.length];
}

function matchAnchor(
  needle: string[],
  haystack: string[],
  start: number,
): AnchorMatch {
  if (needle.length === 0)
    return { score: 0, token_offset: null, token_length: 0 };
  let best = { score: 0, token_offset: null as number | null, token_length: 0 };
  const slack = Math.max(2, Math.ceil(needle.length * 0.3));
  for (let offset = start; offset < haystack.length; offset++) {
    for (
      let length = Math.max(1, needle.length - slack);
      length <= needle.length + slack && offset + length <= haystack.length;
      length++
    ) {
      const score = Math.max(
        0,
        1 -
          editDistance(needle, haystack.slice(offset, offset + length)) /
            needle.length,
      );
      if (score > best.score)
        best = { score, token_offset: offset, token_length: length };
      if (score === 1) return best;
    }
  }
  return best;
}

function evaluate(
  text: string,
  reference: string | undefined,
  c: CorpusCase,
): Metrics {
  const hypothesis = tokens(text, c.language);
  const expected = reference === undefined ? [] : tokens(reference, c.language);
  const anchors: Metrics["anchors"] = {};
  let nextOffset = 0;
  for (const name of ["begin", "middle", "end"] as const) {
    if (!c.anchors?.[name]) continue;
    const match = matchAnchor(
      tokens(c.anchors[name]!, c.language),
      hypothesis,
      nextOffset,
    );
    anchors[name] = match;
    if (
      match.token_offset !== null &&
      match.score >= (c.min_anchor_score ?? 0.8)
    ) {
      // Short fixtures may overlap phrase windows; their starts must still
      // advance so one occurrence cannot satisfy multiple positions.
      nextOffset = match.token_offset + 1;
    }
  }
  const matches = Object.values(anchors);
  return {
    reference_words: expected.length,
    hypothesis_words: hypothesis.length,
    normalized_wer:
      reference === undefined || expected.length === 0
        ? null
        : editDistance(expected, hypothesis) / expected.length,
    anchors,
    anchor_score:
      matches.length === 0
        ? null
        : matches.reduce((sum, match) => sum + match.score, 0) / matches.length,
    anchors_present:
      matches.length === 0
        ? null
        : matches.every(
            (match) =>
              match.token_offset !== null &&
              match.score >= (c.min_anchor_score ?? 0.8),
          ),
    anchors_in_order:
      matches.length === 0 ||
      matches.some((match) => match.token_offset === null)
        ? null
        : matches.every(
            (match, i) =>
              i === 0 || match.token_offset! > matches[i - 1].token_offset!,
          ),
  };
}

function wavDuration(bytes: Buffer): number | null {
  if (
    bytes.length < 12 ||
    bytes.toString("ascii", 0, 4) !== "RIFF" ||
    bytes.toString("ascii", 8, 12) !== "WAVE"
  )
    return null;
  let rate: number | undefined;
  let data = 0;
  for (let offset = 12; offset + 8 <= bytes.length;) {
    const length = bytes.readUInt32LE(offset + 4);
    if (offset + 8 + length > bytes.length) return null;
    const id = bytes.toString("ascii", offset, offset + 4);
    if (id === "fmt " && length >= 16) rate = bytes.readUInt32LE(offset + 16);
    if (id === "data") data += length;
    offset += 8 + length + (length % 2);
  }
  return rate ? data / rate : null;
}

function isInside(path: string, parent: string): boolean {
  const delta = relative(parent, path);
  return delta === "" || (!delta.startsWith("..") && !isAbsolute(delta));
}

async function createEvidence(path: string): Promise<FileHandle> {
  const root = await realpath(
    resolve(dirname(fileURLToPath(import.meta.url)), "../../.."),
  );
  if (isInside(path, root))
    throw new Error("Evidence output must be outside the repository.");
  await mkdir(dirname(path), { recursive: true, mode: 0o700 });
  if (isInside(await realpath(dirname(path)), root)) {
    throw new Error("Evidence output resolves inside the repository.");
  }
  return open(path, "wx", 0o600);
}

function validateCorpus(value: unknown): Corpus {
  const corpus = value as Corpus;
  if (
    corpus?.version !== 1 ||
    !Array.isArray(corpus.cases) ||
    corpus.cases.length === 0
  ) {
    throw new Error("Unsupported or empty corpus manifest.");
  }
  const ids = new Set<string>();
  for (const c of corpus.cases) {
    if (
      !c ||
      typeof c.id !== "string" ||
      !/^[a-zA-Z0-9_.-]{1,100}$/u.test(c.id) ||
      ids.has(c.id) ||
      typeof c.audio !== "string"
    )
      throw new Error("Invalid corpus case.");
    ids.add(c.id);
    if (
      c.expected !== undefined &&
      !["transcript", "no_speech", "rejection"].includes(c.expected)
    ) {
      throw new Error("Invalid corpus case expectation.");
    }
    if (
      (c.max_wer !== undefined &&
        (!Number.isFinite(c.max_wer) || c.max_wer < 0)) ||
      (c.min_anchor_score !== undefined &&
        (!Number.isFinite(c.min_anchor_score) ||
          c.min_anchor_score < 0 ||
          c.min_anchor_score > 1))
    ) {
      throw new Error("Invalid corpus quality threshold.");
    }
  }
  return corpus;
}

function billingSnapshot(value: unknown, account: Account): BillingSnapshot {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Billing status response has an invalid shape.");
  }
  const record = value as Record<string, unknown>;
  const text = (name: string): string | null => {
    const item = record[name];
    if (item === null || item === undefined) return null;
    if (typeof item !== "string" || item.length > 128) {
      throw new Error("Billing status contains invalid plan metadata.");
    }
    return item;
  };
  const credit = (name: string, allowNull: boolean): number | null => {
    const item = record[name];
    if (item === null && allowNull) return null;
    if (typeof item !== "number" || !Number.isSafeInteger(item)) {
      throw new Error(
        "Billing status does not expose the required credit pools.",
      );
    }
    return item;
  };
  if (typeof record.is_subscribed !== "boolean") {
    throw new Error("Billing status does not expose subscription state.");
  }
  return {
    is_subscribed: record.is_subscribed,
    product_name: text("product_name"),
    subscription_status: text("subscription_status"),
    api_credit_balance: credit("api_credit_balance", false),
    total_tokens: credit("total_tokens", account === "free"),
    used_tokens: credit("used_tokens", account === "free"),
  };
}

async function readBillingJson(response: Response): Promise<unknown> {
  if (!response.ok)
    throw new Error(`Billing status returned HTTP ${response.status}.`);
  const reader = response.body?.getReader();
  if (!reader) throw new Error("Billing status returned no response body.");
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      length += value.byteLength;
      if (length > 64 * 1024) {
        await reader.cancel();
        throw new Error(
          "Billing status response exceeds the validation limit.",
        );
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  try {
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    throw new Error("Billing status response is not valid JSON.");
  }
}

async function main(): Promise<void> {
  const options = parseArgs(process.argv.slice(2));
  const corpus = validateCorpus(
    JSON.parse(await readFile(options.corpus, "utf8")),
  );
  const selected = options.all
    ? corpus.cases
    : options.cases.map((id) => {
        const c = corpus.cases.find((candidate) => candidate.id === id);
        if (!c)
          throw new Error(
            "A selected case is absent from the corpus manifest.",
          );
        return c;
      });
  if (selected.length === 0) throw new Error("No cases selected.");
  if (
    options.responseFormat === "verbose_json" &&
    selected.some((c) => !c.language)
  ) {
    throw new Error(
      "Verbose transcription requires a language for every selected case.",
    );
  }
  const users = JSON.parse(await readFile(options.users, "utf8")) as Users;
  if (
    users.version !== 1 ||
    !users.client_id ||
    !users.accounts?.pro ||
    !users.accounts?.free
  ) {
    throw new Error("Unsupported test-user fixture manifest.");
  }
  let billingOrigin: string | null = null;
  if (options.billingCheck) {
    if (!users.billing_url)
      throw new Error("Fixture manifest has no billing URL.");
    const url = new URL(users.billing_url);
    if (
      url.protocol !== "http:" ||
      !["localhost", "127.0.0.1", "[::1]"].includes(url.hostname) ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      url.pathname !== "/"
    ) {
      throw new Error("Billing checks require an exact HTTP loopback origin.");
    }
    billingOrigin = url.origin;
  }
  const privateValues = [
    users.client_id,
    ...Object.values(users.accounts).flatMap((a) => [
      a.email,
      a.password,
      a.user_id,
    ]),
  ];
  const sanitize = (value: unknown): string => {
    let message =
      value instanceof Error ? value.message : "Unknown harness error";
    for (const secret of privateValues)
      if (secret) message = message.replaceAll(secret, "[redacted]");
    return message.slice(0, 4096);
  };
  const evidence = {
    version: 1,
    started_at: new Date().toISOString(),
    completed_at: null as string | null,
    api_origin: options.apiUrl,
    sdk_source: "sdk/src/lib/api.ts",
    sdk_version: JSON.parse(
      await readFile(
        resolve(
          dirname(fileURLToPath(import.meta.url)),
          "../../../sdk/package.json",
        ),
        "utf8",
      ),
    ).version,
    environment: "linked_local",
    attestation:
      "SDK exact-loopback development policy; encrypted Transport V2",
    selected_cases: selected.map((c) => c.id),
    repeat: options.repeat,
    timeout_seconds: options.timeoutSeconds,
    dry_run: options.dryRun,
    response_format: options.responseFormat,
    metrics_policy:
      "NFKC, case/punctuation normalized, Intl word segmentation; ordered approximate anchors",
    results: [] as Result[],
    fatal_error: null as string | null,
    api_key_probe_name: null as string | null,
    api_key_cleanup: "not_created",
    billing: {
      enabled: options.billingCheck,
      origin: billingOrigin,
      account: options.account,
      before: null as BillingSnapshot | null,
      after: null as BillingSnapshot | null,
      credit_pools_unchanged: null as boolean | null,
      failures: [] as string[],
    },
  };
  const output = await createEvidence(options.output);
  const checkpoint = async () => {
    const bytes = new TextEncoder().encode(
      JSON.stringify(evidence, null, 2) + "\n",
    );
    let written = 0;
    while (written < bytes.length) {
      const next = await output.write(
        bytes,
        written,
        bytes.length - written,
        written,
      );
      if (next.bytesWritten === 0)
        throw new Error("Could not checkpoint evidence.");
      written += next.bytesWritten;
    }
    await output.truncate(bytes.length);
    await output.sync();
  };
  await checkpoint();

  // Browser SDK console diagnostics must not leak attestation/error content.
  const originalConsole = globalThis.console;
  globalThis.console = new Proxy(originalConsole, {
    get(target, property) {
      if (
        [
          "log",
          "info",
          "warn",
          "error",
          "debug",
          "group",
          "groupCollapsed",
          "groupEnd",
        ].includes(String(property))
      ) {
        return () => {};
      }
      return Reflect.get(target, property);
    },
  });
  const originalFetch = globalThis.fetch;
  let activeController: AbortController | undefined;
  let activeOuterRequests = 0;
  globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
    if (activeController) activeOuterRequests++;
    const signals = [
      activeController?.signal,
      init?.signal,
      input instanceof Request ? input.signal : undefined,
    ].filter(Boolean) as AbortSignal[];
    return originalFetch(input, {
      ...init,
      ...(signals.length ? { signal: AbortSignal.any(signals) } : {}),
    });
  }) as typeof fetch;
  const bounded = async <T>(
    run: () => Promise<T>,
    seconds: number,
  ): Promise<T> => {
    if (activeController)
      throw new Error("Concurrent SDK calls are unsupported by this harness.");
    activeController = new AbortController();
    activeOuterRequests = 0;
    const timer = setTimeout(
      () =>
        activeController?.abort(new Error("Harness request deadline exceeded")),
      seconds * 1000,
    );
    try {
      return await run();
    } finally {
      clearTimeout(timer);
      activeController = undefined;
    }
  };
  let sdk: typeof import("../../../sdk/src/lib/api");
  let encryptedApi: typeof import("../../../sdk/src/lib/encryptedApi");
  let logicalResponse: { status: number; code: string | null } | undefined;
  const readLogicalResponse = ():
    { status: number; code: string | null } | undefined => logicalResponse;
  try {
    if (!options.dryRun) {
      Bun.plugin({
        name: "transcription-validation-der",
        setup(build) {
          build.onLoad({ filter: /\.der$/u }, async (args) => ({
            loader: "js",
            contents: `export default new Uint8Array(${JSON.stringify(Array.from(await readFile(args.path)))});`,
          }));
        },
      });
      sdk = await import("../../../sdk/src/lib/api");
      if (options.responseFormat === "verbose_json") {
        encryptedApi = await import("../../../sdk/src/lib/encryptedApi");
      }
      sdk.setApiUrl(options.apiUrl, { environment: "development" });
      // Observe logical status only; no decrypted request or response body is logged.
      const { transportV2Runtime } =
        await import("../../../sdk/src/lib/transportV2/runtime");
      const request = transportV2Runtime.request.bind(transportV2Runtime);
      transportV2Runtime.request = async (args) => {
        const exchange = await request(args);
        if (args.request.target === "/v1/audio/transcriptions") {
          logicalResponse = {
            status: exchange.response.status,
            code: exchange.response.headers.get("x-opensecret-error-code"),
          };
        }
        return exchange;
      };
    }
    const login = async (account: Account) => {
      const fixture = users.accounts[account];
      const response = await bounded(
        () => sdk.fetchLogin(fixture.email, fixture.password, users.client_id),
        60,
      );
      if (response.id !== fixture.user_id)
        throw new Error("Authenticated fixture identity mismatch.");
      privateValues.push(response.access_token, response.refresh_token);
    };
    const captureBilling = async (): Promise<BillingSnapshot> => {
      if (!billingOrigin) throw new Error("Billing origin is not configured.");
      return bounded(async () => {
        const generated = await sdk.generateThirdPartyToken(billingOrigin!);
        if (!generated.token) throw new Error("SDK returned no billing token.");
        privateValues.push(generated.token);
        const response = await fetch(
          `${billingOrigin}/v1/maple/subscription/status`,
          {
            headers: { Authorization: `Bearer ${generated.token}` },
            redirect: "error",
          },
        );
        return billingSnapshot(
          await readBillingJson(response),
          options.account,
        );
      }, 30);
    };
    const runCase = async (
      c: CorpusCase,
      repeat: number,
      account: Account,
      apiKey?: string,
      forcedStatus?: number,
      forcedCode?: string,
    ) => {
      const bytes = await readFile(resolve(dirname(options.corpus), c.audio));
      const reference = c.reference
        ? await readFile(resolve(dirname(options.corpus), c.reference), "utf8")
        : undefined;
      const result: Result = {
        case_id: c.id,
        repeat,
        account,
        auth: apiKey ? "api_key" : "jwt",
        expected:
          forcedStatus !== undefined
            ? "rejection"
            : (c.expected ?? "transcript"),
        status: options.dryRun ? "dry_run" : "fail",
        http_status: null,
        error_code: null,
        latency_seconds: 0,
        outer_requests: 0,
        audio_bytes: bytes.length,
        audio_sha256: createHash("sha256")
          .update(
            new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength),
          )
          .digest("hex"),
        audio_duration_seconds:
          wavDuration(bytes) ?? c.duration_seconds ?? null,
        failures: [],
      };
      if (!options.dryRun) {
        const file = new File([bytes], basename(c.audio), {
          type: c.content_type ?? "audio/wav",
        });
        const started = performance.now();
        logicalResponse = undefined;
        try {
          const response = await bounded(
            () =>
              options.responseFormat === "verbose_json"
                ? encryptedApi.openAiAuthenticatedApiCall<
                    import("../../../sdk/src/lib/api").WhisperTranscriptionRequest & {
                      response_format: "verbose_json";
                    },
                    import("../../../sdk/src/lib/api").WhisperTranscriptionResponse
                  >(
                    `${options.apiUrl}/v1/audio/transcriptions`,
                    "POST",
                    {
                      file: bytes.toString("base64"),
                      filename: file.name,
                      content_type: file.type,
                      model: "whisper-large-v3",
                      language: c.language!,
                      response_format: "verbose_json",
                    },
                    "Failed to transcribe audio",
                    apiKey,
                  )
                : sdk.transcribeAudio(
                    file,
                    "whisper-large-v3",
                    c.language,
                    undefined,
                    undefined,
                    apiKey,
                  ),
            options.timeoutSeconds,
          );
          result.latency_seconds = (performance.now() - started) / 1000;
          result.response = response;
          result.http_status = readLogicalResponse()?.status ?? 200;
          result.usage = (response as { usage?: unknown }).usage;
          if (typeof response?.text !== "string")
            result.failures.push("Response has no string text field");
          else {
            result.text = response.text;
            result.reference = reference;
            result.metrics = evaluate(response.text, reference, c);
            if (result.expected === "transcript" && !response.text.trim())
              result.failures.push("Empty transcript");
            if (result.expected === "no_speech" && response.text.trim())
              result.failures.push("Silence produced text");
            if (
              result.metrics.normalized_wer !== null &&
              result.expected === "transcript" &&
              result.metrics.normalized_wer > (c.max_wer ?? 0.25)
            )
              result.failures.push("WER exceeds threshold");
            if (
              result.metrics.anchors_present === false &&
              result.expected === "transcript"
            ) {
              result.failures.push(
                "Beginning, middle, or end anchor is below the completeness threshold",
              );
            }
            if (
              result.metrics.anchors_in_order === false &&
              result.metrics.anchors_present !== false &&
              result.expected === "transcript"
            ) {
              result.failures.push("Anchor occurrences are out of order");
            }
          }
          if (result.expected === "rejection")
            result.failures.push("Expected rejection but request succeeded");
        } catch (error) {
          if (result.latency_seconds === 0)
            result.latency_seconds = (performance.now() - started) / 1000;
          const record = error as { status?: number; headers?: Headers };
          result.http_status =
            readLogicalResponse()?.status ?? record.status ?? null;
          result.error_code =
            readLogicalResponse()?.code ??
            record.headers?.get("x-opensecret-error-code") ??
            null;
          result.error_message = sanitize(error);
          const expected =
            forcedStatus ?? options.expectStatus ?? c.expected_status;
          const statuses =
            expected === undefined
              ? []
              : Array.isArray(expected)
                ? expected
                : [expected];
          const noSpeech =
            result.expected === "no_speech" &&
            result.error_code === "transcription_no_speech";
          if (
            !noSpeech &&
            !(
              result.expected === "rejection" &&
              (statuses.length
                ? statuses.includes(result.http_status!)
                : result.http_status! >= 400)
            )
          ) {
            result.failures.push("Unexpected request error");
          }
          const expectedCode = forcedCode ?? c.expected_error_code;
          if (
            expectedCode !== undefined &&
            result.error_code !== expectedCode
          ) {
            result.failures.push("Unexpected application error code");
          }
        }
        result.outer_requests = activeOuterRequests;
        result.status = result.failures.length ? "fail" : "pass";
      }
      bytes.fill(0);
      evidence.results.push(result);
      await checkpoint();
      process.stdout.write(
        JSON.stringify({
          case_id: result.case_id,
          repeat: result.repeat,
          account: result.account,
          auth: result.auth,
          status: result.status,
          http_status: result.http_status,
          error_code: result.error_code,
          seconds: Number(result.latency_seconds.toFixed(3)),
          audio_seconds: result.audio_duration_seconds,
          words: result.metrics?.hypothesis_words ?? 0,
          wer: result.metrics?.normalized_wer ?? null,
          anchor_score: result.metrics?.anchor_score ?? null,
          anchors_present: result.metrics?.anchors_present ?? null,
          anchors_in_order: result.metrics?.anchors_in_order ?? null,
        }) + "\n",
      );
    };
    if (!options.dryRun) {
      await login(options.account);
      if (options.billingCheck) {
        evidence.billing.before = await captureBilling();
        await checkpoint();
      }
    }
    for (let repeat = 1; repeat <= options.repeat; repeat++) {
      for (const c of selected) {
        const expectation =
          options.expectStatus === undefined
            ? c
            : { ...c, expected: "rejection" as const };
        await runCase(
          expectation,
          repeat,
          options.account,
          undefined,
          options.expectStatus,
        );
      }
    }
    if (options.authProbes && !options.dryRun) {
      await login("free");
      await runCase(
        { ...selected[0], id: `${selected[0].id}.free-denial` },
        1,
        "free",
        undefined,
        403,
        "transcription_paid_required",
      );
      await login("pro");
      const name = `audio-test-${crypto.randomUUID()}`;
      const created = await bounded(() => sdk.createApiKey(name), 60);
      privateValues.push(created.key);
      evidence.api_key_probe_name = name;
      evidence.api_key_cleanup = "pending";
      await checkpoint();
      try {
        await runCase(
          { ...selected[0], id: `${selected[0].id}.api-key-denial` },
          1,
          "pro",
          created.key,
          403,
          "transcription_app_only",
        );
      } finally {
        await bounded(
          () => sdk.deleteApiKey(name, users.accounts.pro.user_id),
          60,
        );
        evidence.api_key_cleanup = "deleted";
      }
    }
    if (options.billingCheck && !options.dryRun) {
      if (options.authProbes) await login(options.account);
      evidence.billing.after = await captureBilling();
      const before = evidence.billing.before!;
      const after = evidence.billing.after;
      evidence.billing.credit_pools_unchanged =
        before.api_credit_balance === after.api_credit_balance &&
        before.total_tokens === after.total_tokens &&
        before.used_tokens === after.used_tokens;
      if (!evidence.billing.credit_pools_unchanged) {
        evidence.billing.failures.push(
          "Customer credit pools changed during transcription validation",
        );
        process.stderr.write(
          "Billing credit-pool assertion failed; inspect the private evidence file.\n",
        );
      }
    }
  } catch (error) {
    evidence.fatal_error = sanitize(error);
    process.stderr.write(
      "Harness stopped; inspect the private evidence file for the error.\n",
    );
    process.exitCode = 1;
  } finally {
    evidence.completed_at = new Date().toISOString();
    await checkpoint();
    await output.close();
    globalThis.console = originalConsole;
    globalThis.fetch = originalFetch;
  }
  if (evidence.results.some((result) => result.status === "fail"))
    process.exitCode = 1;
  if (evidence.billing.failures.length) process.exitCode = 1;
}

main().catch(() => {
  process.stderr.write(
    "Harness setup failed; check arguments, fixture access, dependencies, and output path.\n",
  );
  process.exitCode = 1;
});
