import { describe, expect, test } from "bun:test";
import type OpenAI from "openai";
import {
  isCompactionMetadataUnsupported,
  readChatCompactions,
  readChatCompactionStatus
} from "./chatCompactionApi";

const RESPONSE = "10000000-0000-4000-8000-000000000001";
const MARKER = "30000000-0000-4000-8000-000000000001";

function fakeClient(result: unknown) {
  const calls: Array<{ path: string; options: unknown }> = [];
  return {
    calls,
    client: {
      get: async (path: string, options: unknown) => {
        calls.push({ path, options });
        return result;
      }
    } as unknown as OpenAI
  };
}

describe("encrypted Chat compaction metadata API", () => {
  test("uses the configured OpenAI transport and a bounded timeout, without adding auth", async () => {
    const { client, calls } = fakeClient({
      object: "list",
      data: [],
      has_more: false,
      first_id: null,
      last_id: null
    });
    const controller = new AbortController();
    await readChatCompactions(client, "conversation/path?", undefined, controller.signal);
    expect(calls).toEqual([
      {
        path: "/conversations/conversation%2Fpath%3F/compactions",
        options: {
          query: { limit: 100, order: "desc" },
          signal: controller.signal,
          timeout: 5000
        }
      }
    ]);
  });

  test("keeps metadata cursors separate and strips opaque checkpoint content", async () => {
    const { client, calls } = fakeClient({
      object: "list",
      data: [
        {
          id: MARKER,
          object: "conversation.compaction",
          status: "completed",
          response_id: RESPONSE,
          after_item_id: null,
          created_at: 1792700000,
          generation: 1,
          encrypted_content: "private-checkpoint",
          summary: "private-summary"
        }
      ],
      has_more: true,
      first_id: MARKER,
      last_id: MARKER
    });
    const page = await readChatCompactions(client, "conversation", MARKER);
    expect(calls[0].options).toMatchObject({
      query: { limit: 100, order: "desc", after: MARKER },
      timeout: 5000
    });
    expect(page.last_id).toBe(MARKER);
    expect(page.data[0]).toMatchObject({ id: MARKER, type: "compaction" });
    expect(JSON.stringify(page)).not.toContain("private");
  });

  test("requests progress for the exact submitted message with the same abort signal", async () => {
    const { client, calls } = fakeClient({
      status: "compacting",
      response_id: RESPONSE,
      encrypted_content: "private"
    });
    const controller = new AbortController();
    const status = await readChatCompactionStatus(
      client,
      "conversation/path",
      "message-id",
      controller.signal
    );
    expect(calls).toEqual([
      {
        path: "/conversations/conversation%2Fpath/compaction-status",
        options: {
          query: { message_id: "message-id" },
          signal: controller.signal,
          timeout: 5000
        }
      }
    ]);
    expect(status).toEqual({ status: "compacting", response_id: RESPONSE });
  });

  test("recognizes optional API absence, without swallowing auth or network failures", () => {
    expect(isCompactionMetadataUnsupported({ status: 404 })).toBe(true);
    for (const error of [
      null,
      undefined,
      new Error("network"),
      { status: "404" },
      { status: 401 },
      { status: 403 },
      { status: 500 }
    ]) {
      expect(isCompactionMetadataUnsupported(error)).toBe(false);
    }
  });

  test("does not retry or replace transport failures", async () => {
    const failure = new Error("private transport details");
    let calls = 0;
    const client = {
      get: async () => {
        calls += 1;
        throw failure;
      }
    } as unknown as OpenAI;
    await expect(readChatCompactions(client, "conversation")).rejects.toBe(failure);
    expect(calls).toBe(1);
  });
});
