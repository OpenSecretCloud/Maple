import { describe, expect, test } from "bun:test";
import {
  CHAT_ACCOUNT_CREDENTIAL_MISMATCH_CODE,
  ChatAccountCredentialMismatchError,
  assertChatAccountCredential,
  chatAccessTokenSubject,
  createAccountBoundChatFetch,
  isChatAccountCredentialMismatchError
} from "./chatAccountCredential";

function tokenForSubject(subject: string): string {
  const encode = (value: object) =>
    btoa(JSON.stringify(value)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return `${encode({ alg: "ES256K", typ: "JWT" })}.${encode({ sub: subject })}.signature`;
}

describe("account-bound Chat credentials", () => {
  test("extracts a JWT subject and rejects malformed credentials", () => {
    expect(chatAccessTokenSubject(tokenForSubject("user-a"))).toBe("user-a");
    expect(chatAccessTokenSubject("not-a-jwt")).toBeNull();
    expect(chatAccessTokenSubject(null)).toBeNull();
  });

  test("allows refreshed tokens for the same account", async () => {
    let token = tokenForSubject("user-a");
    const calls: string[] = [];
    const fetch = createAccountBoundChatFetch({
      expectedUserId: "user-a",
      getAccessToken: () => token,
      fetch: async (input) => {
        calls.push(String(input));
        return Response.json({ ok: true });
      }
    });

    await fetch("https://example.test/first");
    token = tokenForSubject("user-a");
    await fetch("https://example.test/after-refresh");

    expect(calls).toEqual(["https://example.test/first", "https://example.test/after-refresh"]);
  });

  test("blocks a replaced account before invoking the transport", async () => {
    let called = false;
    const fetch = createAccountBoundChatFetch({
      expectedUserId: "user-a",
      getAccessToken: () => tokenForSubject("user-b"),
      fetch: async () => {
        called = true;
        return Response.json({ ok: true });
      }
    });

    try {
      await fetch("https://example.test/blocked");
      throw new Error("expected account-bound fetch to reject");
    } catch (error) {
      expect(isChatAccountCredentialMismatchError(error)).toBe(true);
      expect(error).toMatchObject({
        code: CHAT_ACCOUNT_CREDENTIAL_MISMATCH_CODE,
        requestDispatchCode: "opensecret_request_not_dispatched",
        definitelyNotDispatched: true
      });
    }
    expect(called).toBe(false);
  });

  test("guards non-Chat account operations with the same account identity", () => {
    expect(() =>
      assertChatAccountCredential("user-a", () => tokenForSubject("user-a"))
    ).not.toThrow();
    expect(() => assertChatAccountCredential("user-a", () => tokenForSubject("user-b"))).toThrow(
      ChatAccountCredentialMismatchError
    );
    expect(() => assertChatAccountCredential(undefined, () => tokenForSubject("user-a"))).toThrow(
      ChatAccountCredentialMismatchError
    );
  });

  test("recognizes an OpenAI-style wrapped mismatch", () => {
    const mismatch = { code: CHAT_ACCOUNT_CREDENTIAL_MISMATCH_CODE };
    expect(isChatAccountCredentialMismatchError({ cause: mismatch })).toBe(true);
    expect(isChatAccountCredentialMismatchError(new Error("network"))).toBe(false);
  });
});
