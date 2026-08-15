import { afterEach, describe, expect, mock, test } from "bun:test";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useCallback, useState, type ReactNode } from "react";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import type { BillingStatus } from "@/billing/billingApi";
import {
  NESTED_BILLING_QUERY_MOUNT_POLICY,
  useBillingStatusQuery,
  type BillingStatusQueryDependencies
} from "@/billing/useBillingStatusQuery";
import { CreditUsage } from "@/components/CreditUsage";
import { BillingStateContext } from "@/state/LocalStateContextDef";
import { useSettingsBillingRefresh } from "./useSettingsBillingRefresh";

function billingStatus(productName: string): BillingStatus {
  return {
    is_subscribed: productName !== "Free",
    stripe_customer_id: null,
    product_id: productName.toLowerCase(),
    product_name: productName,
    subscription_status: "active",
    current_period_end: null,
    can_chat: true,
    chats_remaining: null,
    payment_provider: null,
    total_tokens: 100,
    used_tokens: 25,
    usage_reset_date: null,
    api_credit_balance: 50
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function createQueryClient(staleTime = Number.POSITIVE_INFINITY) {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        staleTime
      }
    }
  });
}

const noopClearBillingStatus = () => {};

function NestedBillingProbe({
  accountId = "user-1",
  dependencies,
  label,
  setBillingStatus
}: {
  accountId?: string | null;
  dependencies: BillingStatusQueryDependencies;
  label: string;
  setBillingStatus: (status: BillingStatus) => void;
}) {
  const { data } = useBillingStatusQuery({
    accountId,
    billingStatusAccountId: null,
    clearBillingStatus: noopClearBillingStatus,
    dependencies,
    refetchOnMount: NESTED_BILLING_QUERY_MOUNT_POLICY.refetchOnMount,
    setBillingStatus
  });

  return <span data-label={label}>{data?.product_name ?? "Loading"}</span>;
}

function BillingProbe({
  accountId = "user-1",
  dependencies,
  label,
  rerenderToken = 0,
  setBillingStatus
}: {
  accountId?: string | null;
  dependencies: BillingStatusQueryDependencies;
  label: string;
  rerenderToken?: number;
  setBillingStatus: (status: BillingStatus) => void;
}) {
  const { data } = useSettingsBillingRefresh({
    accountId,
    billingStatusAccountId: null,
    clearBillingStatus: noopClearBillingStatus,
    dependencies,
    setBillingStatus
  });

  return (
    <span data-label={label} data-rerender-token={rerenderToken}>
      {data?.product_name ?? "Loading"}
    </span>
  );
}

function UsageProbe({
  accountId = "user-1",
  dependencies,
  initialStatus,
  initialStatusAccountId = "user-1",
  onPublish
}: {
  accountId?: string | null;
  dependencies: BillingStatusQueryDependencies;
  initialStatus: BillingStatus;
  initialStatusAccountId?: string | null;
  onPublish: (status: BillingStatus, accountId?: string | null) => void;
}) {
  const [localBilling, setLocalBilling] = useState({
    status: initialStatus as BillingStatus | null,
    accountId: initialStatusAccountId
  });
  const publishStatus = useCallback(
    (status: BillingStatus, ownerAccountId: string | null = null) => {
      onPublish(status, ownerAccountId);
      setLocalBilling({ status, accountId: ownerAccountId });
    },
    [onPublish]
  );
  const clearBillingStatus = useCallback(() => {
    setLocalBilling({ status: null, accountId: null });
  }, []);
  useSettingsBillingRefresh({
    accountId,
    billingStatusAccountId: localBilling.accountId,
    clearBillingStatus,
    dependencies,
    setBillingStatus: publishStatus
  });

  return (
    <BillingStateContext.Provider
      value={{
        billingStatus: localBilling.status,
        billingStatusAccountId: localBilling.accountId,
        clearBillingStatus,
        setBillingStatus: publishStatus
      }}
    >
      <div data-label="usage-card">
        <CreditUsage pagePresentation />
      </div>
    </BillingStateContext.Provider>
  );
}

function SettingsVisit({ children, client }: { children: ReactNode; client: QueryClient }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

async function flushQueryNotifications() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function renderedPlan(renderer: ReactTestRenderer, label: string) {
  return renderer.root.findByProps({ "data-label": label }).children.join("");
}

function renderedText(node: ReactTestInstance): string {
  return node.children
    .map((child) => (typeof child === "string" ? child : renderedText(child)))
    .join("");
}

describe("useSettingsBillingRefresh", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
  });

  test("requests billing once on an initial direct Settings entry", async () => {
    const fresh = billingStatus("Pro");
    const getBillingStatus = mock(async () => fresh);
    const setBillingStatus = mock(() => {});
    const client = createQueryClient();

    await act(async () => {
      renderer = create(
        <SettingsVisit client={client}>
          <BillingProbe
            dependencies={{ getBillingStatus }}
            label="settings"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });
    await flushQueryNotifications();

    expect(getBillingStatus).toHaveBeenCalledTimes(1);
    expect(setBillingStatus).toHaveBeenCalledTimes(1);
    expect(renderedPlan(renderer!, "settings")).toBe("Pro");
  });

  test("does not refresh for a detail-like rerender within the same Settings visit", async () => {
    const getBillingStatus = mock(async () => billingStatus("Pro"));
    const setBillingStatus = mock(() => {});
    const dependencies = { getBillingStatus };
    const client = createQueryClient();

    await act(async () => {
      renderer = create(
        <SettingsVisit client={client}>
          <BillingProbe
            dependencies={dependencies}
            label="settings"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });

    await act(async () => {
      renderer?.update(
        <SettingsVisit client={client}>
          <BillingProbe
            dependencies={dependencies}
            label="settings"
            rerenderToken={1}
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });

    expect(getBillingStatus).toHaveBeenCalledTimes(1);
    expect(setBillingStatus).toHaveBeenCalledTimes(1);
  });

  test("does not refresh again when the nested API detail observer mounts", async () => {
    const getBillingStatus = mock(async () => billingStatus("Pro"));
    const setBillingStatus = mock(() => {});
    const dependencies = { getBillingStatus };
    const client = createQueryClient(0);

    await act(async () => {
      renderer = create(
        <SettingsVisit client={client}>
          <BillingProbe
            dependencies={dependencies}
            label="settings-shell"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });
    await flushQueryNotifications();

    await act(async () => {
      renderer?.update(
        <SettingsVisit client={client}>
          <BillingProbe
            dependencies={dependencies}
            label="settings-shell"
            setBillingStatus={setBillingStatus}
          />
          <NestedBillingProbe
            dependencies={dependencies}
            label="api-detail"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });

    expect(getBillingStatus).toHaveBeenCalledTimes(1);
    expect(setBillingStatus).toHaveBeenCalledTimes(1);
    expect(renderedPlan(renderer!, "api-detail")).toBe("Pro");
  });

  test("refreshes once more after leaving and re-entering Settings", async () => {
    let visit = 0;
    const getBillingStatus = mock(async () => {
      visit += 1;
      return billingStatus(visit === 1 ? "Free" : "Pro");
    });
    const setBillingStatus = mock(() => {});
    const dependencies = { getBillingStatus };
    const client = createQueryClient();

    await act(async () => {
      renderer = create(
        <SettingsVisit client={client}>
          <BillingProbe
            dependencies={dependencies}
            label="first-visit"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });
    act(() => renderer?.unmount());
    renderer = null;

    await act(async () => {
      renderer = create(
        <SettingsVisit client={client}>
          <BillingProbe
            dependencies={dependencies}
            label="second-visit"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });
    await flushQueryNotifications();
    await flushQueryNotifications();

    expect(getBillingStatus).toHaveBeenCalledTimes(2);
    expect(setBillingStatus).toHaveBeenCalledTimes(2);
  });

  test("deduplicates a direct API detail entry across shell and nested observers", async () => {
    const request = deferred<BillingStatus>();
    const getBillingStatus = mock(() => request.promise);
    const setBillingStatus = mock(() => {});
    const dependencies = { getBillingStatus };
    const client = createQueryClient();

    act(() => {
      renderer = create(
        <SettingsVisit client={client}>
          <BillingProbe
            dependencies={dependencies}
            label="shell"
            setBillingStatus={setBillingStatus}
          />
          <NestedBillingProbe
            dependencies={dependencies}
            label="api-detail"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });

    expect(getBillingStatus).toHaveBeenCalledTimes(1);

    await act(async () => {
      request.resolve(billingStatus("Pro"));
      await request.promise;
    });
    await flushQueryNotifications();

    expect(setBillingStatus).toHaveBeenCalledTimes(1);
    expect(renderedPlan(renderer!, "shell")).toBe("Pro");
    expect(renderedPlan(renderer!, "api-detail")).toBe("Pro");
  });

  test("deduplicates an in-flight StrictMode-style remount", async () => {
    const request = deferred<BillingStatus>();
    const getBillingStatus = mock(() => request.promise);
    const setBillingStatus = mock(() => {});
    const dependencies = { getBillingStatus };
    const client = createQueryClient();

    act(() => {
      renderer = create(
        <SettingsVisit client={client}>
          <BillingProbe
            dependencies={dependencies}
            label="first-mount"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });
    act(() => renderer?.unmount());
    renderer = null;
    act(() => {
      renderer = create(
        <SettingsVisit client={client}>
          <BillingProbe
            dependencies={dependencies}
            label="strict-remount"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });

    expect(getBillingStatus).toHaveBeenCalledTimes(1);

    await act(async () => {
      request.resolve(billingStatus("Pro"));
      await request.promise;
    });
    await flushQueryNotifications();

    expect(setBillingStatus).toHaveBeenCalledTimes(1);
    expect(renderedPlan(renderer!, "strict-remount")).toBe("Pro");
  });

  test("keeps the prior Usage card visible until a deferred refresh succeeds", async () => {
    const previous = billingStatus("Free");
    const fresh = billingStatus("Pro");
    const request = deferred<BillingStatus>();
    const getBillingStatus = mock(() => request.promise);
    const setBillingStatus = mock(() => {});
    const client = createQueryClient();

    act(() => {
      renderer = create(
        <SettingsVisit client={client}>
          <UsageProbe
            dependencies={{ getBillingStatus }}
            initialStatus={previous}
            onPublish={setBillingStatus}
          />
        </SettingsVisit>
      );
    });

    expect(getBillingStatus).toHaveBeenCalledTimes(1);
    expect(setBillingStatus).not.toHaveBeenCalled();
    expect(renderedText(renderer!.root.findByProps({ "data-label": "usage-card" }))).toContain(
      "Free Plan"
    );

    await act(async () => {
      request.resolve(fresh);
      await request.promise;
    });
    await flushQueryNotifications();

    expect(setBillingStatus).toHaveBeenCalledTimes(1);
    expect(setBillingStatus).toHaveBeenCalledWith(fresh, "user-1");
    expect(renderedText(renderer!.root.findByProps({ "data-label": "usage-card" }))).toContain(
      "Pro Plan"
    );
  });

  test("does not publish a late result after logout", async () => {
    const request = deferred<BillingStatus>();
    const getBillingStatus = mock(() => request.promise);
    const setBillingStatus = mock(() => {});
    const dependencies = { getBillingStatus };
    const client = createQueryClient();

    act(() => {
      renderer = create(
        <SettingsVisit client={client}>
          <BillingProbe
            accountId="user-1"
            dependencies={dependencies}
            label="settings"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });

    act(() => {
      renderer?.update(
        <SettingsVisit client={client}>
          <BillingProbe
            accountId={null}
            dependencies={dependencies}
            label="settings"
            setBillingStatus={setBillingStatus}
          />
        </SettingsVisit>
      );
    });

    await act(async () => {
      request.resolve(billingStatus("Free"));
      await request.promise;
    });
    await flushQueryNotifications();

    expect(getBillingStatus).toHaveBeenCalledTimes(1);
    expect(setBillingStatus).not.toHaveBeenCalled();
  });

  test("withholds prior-account Usage while refreshing the current account", async () => {
    const priorRequest = deferred<BillingStatus>();
    const currentRequest = deferred<BillingStatus>();
    const prior = billingStatus("Free");
    const current = billingStatus("Pro");
    let requestCount = 0;
    const getBillingStatus = mock(() => {
      requestCount += 1;
      return requestCount === 1 ? priorRequest.promise : currentRequest.promise;
    });
    const setBillingStatus = mock(() => {});
    const dependencies = { getBillingStatus };
    const client = createQueryClient();

    act(() => {
      renderer = create(
        <SettingsVisit client={client}>
          <UsageProbe
            accountId="user-1"
            dependencies={dependencies}
            initialStatus={prior}
            initialStatusAccountId="user-1"
            onPublish={setBillingStatus}
          />
        </SettingsVisit>
      );
    });

    act(() => {
      renderer?.update(
        <SettingsVisit client={client}>
          <UsageProbe
            accountId="user-2"
            dependencies={dependencies}
            initialStatus={prior}
            initialStatusAccountId="user-1"
            onPublish={setBillingStatus}
          />
        </SettingsVisit>
      );
    });

    expect(getBillingStatus).toHaveBeenCalledTimes(2);
    expect(setBillingStatus).not.toHaveBeenCalled();
    const usageAfterSwitch = renderedText(
      renderer!.root.findByProps({ "data-label": "usage-card" })
    );
    expect(usageAfterSwitch).toContain("Loading...");
    expect(usageAfterSwitch).not.toContain("Free Plan");

    await act(async () => {
      priorRequest.resolve(prior);
      await priorRequest.promise;
    });
    await flushQueryNotifications();

    expect(setBillingStatus).not.toHaveBeenCalled();
    expect(renderedText(renderer!.root.findByProps({ "data-label": "usage-card" }))).not.toContain(
      "Free Plan"
    );

    await act(async () => {
      currentRequest.resolve(current);
      await currentRequest.promise;
    });
    await flushQueryNotifications();

    expect(getBillingStatus).toHaveBeenCalledTimes(2);
    expect(setBillingStatus).toHaveBeenCalledTimes(1);
    expect(setBillingStatus).toHaveBeenCalledWith(current, "user-2");
    expect(setBillingStatus).not.toHaveBeenCalledWith(prior, "user-1");
    expect(renderedText(renderer!.root.findByProps({ "data-label": "usage-card" }))).toContain(
      "Pro Plan"
    );
  });
});
