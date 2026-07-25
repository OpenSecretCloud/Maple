import { describe, expect, test } from "bun:test";
import {
  isPaidTTSAccessError,
  synthesizeTTSChunk,
  TTSSynthesisHttpError,
  TTSSynthesisProviderError,
  VOXTRAL_TTS_MODEL,
  type AiCustomFetch
} from "./ttsSynthesis";

describe("synthesizeTTSChunk", () => {
  test("requests explicit Voxtral preferences and returns WAV bytes", async () => {
    const calls: Array<{ input: string | URL | Request; init?: RequestInit }> = [];
    const expected = new Uint8Array([82, 73, 70, 70]).buffer;
    const aiCustomFetch: AiCustomFetch = async (input, init) => {
      calls.push({ input, init });
      return new Response(expected, { status: 200, headers: { "content-type": "audio/wav" } });
    };
    const controller = new AbortController();

    const result = await synthesizeTTSChunk(
      aiCustomFetch,
      "https://enclave.example/",
      "Hello.",
      { voice: "fr_female", speed: 1.2 },
      controller.signal
    );

    expect(new Uint8Array(result)).toEqual(new Uint8Array(expected));
    expect(calls).toHaveLength(1);
    expect(calls[0]?.input).toBe("https://enclave.example/v1/audio/speech");
    expect(calls[0]?.init).toEqual({
      method: "POST",
      headers: {
        Accept: "audio/wav",
        "Content-Type": "application/json"
      },
      body: JSON.stringify({
        input: "Hello.",
        model: VOXTRAL_TTS_MODEL,
        voice: "fr_female",
        speed: 1.2
      }),
      signal: controller.signal
    });
  });

  test("rejects non-success and empty responses", async () => {
    const controller = new AbortController();
    const forbidden: AiCustomFetch = async () => new Response(null, { status: 403 });
    const empty: AiCustomFetch = async () => new Response(new Uint8Array(), { status: 200 });

    await expect(
      synthesizeTTSChunk(
        forbidden,
        "https://enclave.example",
        "Hello.",
        { voice: "neutral_female", speed: 1.2 },
        controller.signal
      )
    ).rejects.toEqual(new TTSSynthesisHttpError(403));
    await expect(
      synthesizeTTSChunk(
        empty,
        "https://enclave.example",
        "Hello.",
        { voice: "neutral_female", speed: 1.2 },
        controller.signal
      )
    ).rejects.toThrow("empty audio file");
  });

  test("surfaces successful JSON provider errors before audio decoding", async () => {
    const controller = new AbortController();
    const providerError: AiCustomFetch = async () =>
      new Response(JSON.stringify({ error: { message: "Voice preset is unavailable" } }), {
        status: 200,
        headers: { "content-type": "application/json; charset=utf-8" }
      });

    await expect(
      synthesizeTTSChunk(
        providerError,
        "https://enclave.example",
        "Hello.",
        { voice: "neutral_female", speed: 1.2 },
        controller.signal
      )
    ).rejects.toEqual(
      new TTSSynthesisProviderError(
        "Text-to-speech provider returned an error: Voice preset is unavailable"
      )
    );
  });

  test("recognizes structured +json errors and rejects non-audio success bodies", async () => {
    const controller = new AbortController();
    const problemJson: AiCustomFetch = async () =>
      new Response(JSON.stringify({ detail: "Speech generation failed" }), {
        status: 200,
        headers: { "content-type": "application/problem+json" }
      });
    const textResponse: AiCustomFetch = async () =>
      new Response("not audio", {
        status: 200,
        headers: { "content-type": "text/plain" }
      });

    await expect(
      synthesizeTTSChunk(
        problemJson,
        "https://enclave.example",
        "Hello.",
        { voice: "neutral_female", speed: 1.2 },
        controller.signal
      )
    ).rejects.toThrow("Text-to-speech provider returned an error: Speech generation failed");
    await expect(
      synthesizeTTSChunk(
        textResponse,
        "https://enclave.example",
        "Hello.",
        { voice: "neutral_female", speed: 1.2 },
        controller.signal
      )
    ).rejects.toThrow("unexpected content type: text/plain");
  });

  test("honors cancellation before and after request body reads", async () => {
    const before = new AbortController();
    before.abort();
    let called = false;
    const neverCalled: AiCustomFetch = async () => {
      called = true;
      return new Response(new Uint8Array([1]));
    };

    await expect(
      synthesizeTTSChunk(
        neverCalled,
        "https://enclave.example",
        "Hello.",
        { voice: "neutral_female", speed: 1.2 },
        before.signal
      )
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(called).toBe(false);

    const during = new AbortController();
    const canceledDuringRequest: AiCustomFetch = async () => {
      during.abort();
      return new Response(new Uint8Array([1]));
    };
    await expect(
      synthesizeTTSChunk(
        canceledDuringRequest,
        "https://enclave.example",
        "Hello.",
        { voice: "neutral_female", speed: 1.2 },
        during.signal
      )
    ).rejects.toMatchObject({ name: "AbortError" });

    const duringProviderBody = new AbortController();
    const canceledDuringProviderBody: AiCustomFetch = async () => {
      const response = new Response(JSON.stringify({ error: "failed" }), {
        status: 200,
        headers: { "content-type": "application/json" }
      });
      const readBody = response.text.bind(response);
      response.text = async () => {
        const body = await readBody();
        duringProviderBody.abort();
        return body;
      };
      return response;
    };
    await expect(
      synthesizeTTSChunk(
        canceledDuringProviderBody,
        "https://enclave.example",
        "Hello.",
        { voice: "neutral_female", speed: 1.2 },
        duringProviderBody.signal
      )
    ).rejects.toMatchObject({ name: "AbortError" });
  });
});

describe("isPaidTTSAccessError", () => {
  test("recognizes paid-plan HTTP failures", () => {
    expect(isPaidTTSAccessError(new TTSSynthesisHttpError(403))).toBe(true);
    expect(isPaidTTSAccessError({ response: { status: 402 } })).toBe(true);
    expect(isPaidTTSAccessError(new Error("Forbidden"))).toBe(true);
    expect(isPaidTTSAccessError(new Error("Network failed"))).toBe(false);
  });
});
