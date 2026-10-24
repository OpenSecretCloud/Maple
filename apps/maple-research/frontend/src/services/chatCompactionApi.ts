import type OpenAI from "openai";
import { parseCompactionPage, parseCompactionStatus } from "./chatCompaction";

/** Additive OpenSecret metadata, over the same account-bound encrypted fetch as chat. */
export async function readChatCompactions(
  client: OpenAI,
  conversationId: string,
  after?: string,
  signal?: AbortSignal
) {
  const result = await client.get<unknown>(
    `/conversations/${encodeURIComponent(conversationId)}/compactions`,
    { query: { limit: 100, order: "desc", ...(after ? { after } : {}) }, signal, timeout: 5000 }
  );
  return parseCompactionPage(result);
}

export async function readChatCompactionStatus(
  client: OpenAI,
  conversationId: string,
  messageId: string,
  signal: AbortSignal
) {
  const result = await client.get<unknown>(
    `/conversations/${encodeURIComponent(conversationId)}/compaction-status`,
    { query: { message_id: messageId }, signal, timeout: 5000 }
  );
  return parseCompactionStatus(result);
}

/** Old servers do not expose this optional presentation API. */
export function isCompactionMetadataUnsupported(error: unknown): boolean {
  return Boolean(error && typeof error === "object" && "status" in error && error.status === 404);
}
