import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import type { OpenSecretContextType } from "@opensecret/react";
import type { BillingStatus } from "./billingApi";
import { initBillingService } from "./billingService";

const TOKEN_STORAGE_KEY = "maple_billing_token";

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>();

  get length(): number {
    return this.values.size;
  }

  clear(): void {
    this.values.clear();
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.values.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function billingStatus(productName: string): BillingStatus {
  return {
    is_subscribed: true,
    stripe_customer_id: null,
    product_id: productName.toLowerCase().replace(/ /g, "-"),
    product_name: productName,
    subscription_status: "active",
    current_period_end: null,
    can_chat: true,
    chats_remaining: null,
    payment_provider: null,
    total_tokens: 100,
    used_tokens: 25,
    usage_reset_date: null
  };
}

function openSecretContext(
  accountId: string | null,
  generateThirdPartyToken: () => Promise<{ token: string }>
): OpenSecretContextType {
  return {
    auth: {
      user: accountId ? { user: { id: accountId } } : null
    },
    generateThirdPartyToken
  } as unknown as OpenSecretContextType;
}

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

async function rejectionFrom<T>(promise: Promise<T>): Promise<unknown> {
  return promise.then(
    () => new Error("Expected the request to reject."),
    (error: unknown) => error
  );
}

const originalGlobals = {
  fetch: Object.getOwnPropertyDescriptor(globalThis, "fetch"),
  sessionStorage: Object.getOwnPropertyDescriptor(globalThis, "sessionStorage")
};

function setGlobal(name: "fetch" | "sessionStorage", value: unknown): void {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    value,
    writable: true
  });
}

function restoreGlobal(name: "fetch" | "sessionStorage", descriptor?: PropertyDescriptor): void {
  if (descriptor) {
    Object.defineProperty(globalThis, name, descriptor);
  } else {
    Reflect.deleteProperty(globalThis, name);
  }
}

describe("BillingService credential ownership", () => {
  let requests: string[];
  let storage: MemoryStorage;

  beforeEach(() => {
    requests = [];
    storage = new MemoryStorage();
    setGlobal("sessionStorage", storage);
    setGlobal(
      "fetch",
      mock(async (_input: RequestInfo | URL, init?: RequestInit) => {
        const authorization = new Headers(init?.headers).get("Authorization") ?? "";
        requests.push(authorization);
        const productName =
          authorization === "Bearer account-a-token" ? "Account A Pro" : "Account B Pro";
        return new Response(JSON.stringify(billingStatus(productName)), {
          headers: { "Content-Type": "application/json" },
          status: 200
        });
      })
    );

    const service = initBillingService(openSecretContext(null, async () => ({ token: "unused" })));
    service.clearToken();
  });

  afterEach(() => {
    restoreGlobal("fetch", originalGlobals.fetch);
    restoreGlobal("sessionStorage", originalGlobals.sessionStorage);
  });

  test("does not let a late Account A token authenticate Account B after logout", async () => {
    const accountAToken = deferred<{ token: string }>();
    const generateAccountAToken = mock(() => accountAToken.promise);
    const generateAccountBToken = mock(async () => ({ token: "account-b-token" }));
    const service = initBillingService(openSecretContext("account-a", generateAccountAToken));

    const accountARequest = service.getBillingStatus("account-a");
    await flushPromises();
    expect(generateAccountAToken).toHaveBeenCalledTimes(1);

    service.clearToken();
    initBillingService(openSecretContext("account-b", generateAccountBToken));
    const accountARejection = rejectionFrom(accountARequest);

    accountAToken.resolve({ token: "account-a-token" });
    const accountAError = await accountARejection;

    expect(accountAError).toBeInstanceOf(Error);
    expect((accountAError as Error).message).toContain("billing session changed");
    expect(storage.getItem(TOKEN_STORAGE_KEY)).toBeNull();
    expect(requests).toEqual([]);

    const accountBStatus = await service.getBillingStatus("account-b");

    expect(accountBStatus.product_name).toBe("Account B Pro");
    expect(generateAccountBToken).toHaveBeenCalledTimes(1);
    expect(requests).toEqual(["Bearer account-b-token"]);
    expect(JSON.parse(storage.getItem(TOKEN_STORAGE_KEY) ?? "null")).toEqual({
      accountId: "account-b",
      token: "account-b-token"
    });
  });

  test("clears a stored credential when the authenticated account changes", async () => {
    const service = initBillingService(
      openSecretContext("account-a", async () => ({ token: "account-a-token" }))
    );
    await service.getBillingStatus("account-a");

    const generateAccountBToken = mock(async () => ({ token: "account-b-token" }));
    initBillingService(openSecretContext("account-b", generateAccountBToken));
    const accountBStatus = await service.getBillingStatus("account-b");

    expect(accountBStatus.product_name).toBe("Account B Pro");
    expect(generateAccountBToken).toHaveBeenCalledTimes(1);
    expect(requests).toEqual(["Bearer account-a-token", "Bearer account-b-token"]);
  });

  test("rejects a stale query owner before generating a token for the current account", async () => {
    const generateAccountBToken = mock(async () => ({ token: "account-b-token" }));
    const service = initBillingService(openSecretContext("account-b", generateAccountBToken));

    const error = await rejectionFrom(service.getBillingStatus("account-a"));

    expect(error).toBeInstanceOf(Error);
    expect((error as Error).message).toContain("billing session changed");
    expect(generateAccountBToken).not.toHaveBeenCalled();
    expect(requests).toEqual([]);
  });

  test("keeps a same-account token generation valid across provider rerenders", async () => {
    const accountAToken = deferred<{ token: string }>();
    const firstContextGenerator = mock(() => accountAToken.promise);
    const replacementContextGenerator = mock(async () => ({ token: "unexpected-token" }));
    const service = initBillingService(openSecretContext("account-a", firstContextGenerator));
    const request = service.getBillingStatus("account-a");
    await flushPromises();

    initBillingService(openSecretContext("account-a", replacementContextGenerator));
    accountAToken.resolve({ token: "account-a-token" });

    await expect(request).resolves.toMatchObject({ product_name: "Account A Pro" });
    expect(firstContextGenerator).toHaveBeenCalledTimes(1);
    expect(replacementContextGenerator).not.toHaveBeenCalled();
    expect(requests).toEqual(["Bearer account-a-token"]);
  });

  test("discards legacy raw tokens instead of assigning them to the current account", async () => {
    const generateAccountAToken = mock(async () => ({ token: "account-a-token" }));
    const service = initBillingService(openSecretContext("account-a", generateAccountAToken));
    storage.setItem(TOKEN_STORAGE_KEY, "legacy-unowned-token");

    await service.getBillingStatus("account-a");

    expect(generateAccountAToken).toHaveBeenCalledTimes(1);
    expect(requests).toEqual(["Bearer account-a-token"]);
  });

  test("rotates an unauthorized same-account credential once", async () => {
    const authorizations: string[] = [];
    let requestCount = 0;
    setGlobal(
      "fetch",
      mock(async (_input: RequestInfo | URL, init?: RequestInit) => {
        authorizations.push(new Headers(init?.headers).get("Authorization") ?? "");
        requestCount += 1;
        if (requestCount === 1) return new Response("expired", { status: 401 });

        return new Response(JSON.stringify(billingStatus("Account A Pro")), {
          headers: { "Content-Type": "application/json" },
          status: 200
        });
      })
    );
    const generateAccountAToken = mock(async () => ({ token: "account-a-token" }));
    const service = initBillingService(openSecretContext("account-a", generateAccountAToken));
    storage.setItem(
      TOKEN_STORAGE_KEY,
      JSON.stringify({ accountId: "account-a", token: "expired-account-a-token" })
    );
    const originalConsoleError = console.error;
    console.error = mock(() => {});

    try {
      await expect(service.getBillingStatus("account-a")).resolves.toMatchObject({
        product_name: "Account A Pro"
      });
    } finally {
      console.error = originalConsoleError;
    }

    expect(generateAccountAToken).toHaveBeenCalledTimes(1);
    expect(authorizations).toEqual(["Bearer expired-account-a-token", "Bearer account-a-token"]);
    expect(JSON.parse(storage.getItem(TOKEN_STORAGE_KEY) ?? "null")).toEqual({
      accountId: "account-a",
      token: "account-a-token"
    });
  });

  test("rejects an API result that completes after an account transition", async () => {
    const response = deferred<Response>();
    const fetchRequest = mock(() => response.promise);
    setGlobal("fetch", fetchRequest);
    const service = initBillingService(
      openSecretContext("account-a", async () => ({ token: "account-a-token" }))
    );
    const request = service.getBillingStatus("account-a");
    await flushPromises();
    expect(fetchRequest).toHaveBeenCalledTimes(1);

    initBillingService(openSecretContext("account-b", async () => ({ token: "account-b-token" })));
    const rejection = rejectionFrom(request);
    response.resolve(
      new Response(JSON.stringify(billingStatus("Account A Pro")), {
        headers: { "Content-Type": "application/json" },
        status: 200
      })
    );

    const error = await rejection;
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).message).toContain("billing session changed");
  });
});
