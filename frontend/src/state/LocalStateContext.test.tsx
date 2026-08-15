import { afterEach, describe, expect, test } from "bun:test";
import { memo } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";

import type { BillingStatus } from "@/billing/billingApi";
import type {
  BillingState,
  ModelState,
  SelectedProjectState,
  SidebarSearchState
} from "./LocalStateContextDef";
import { DEFAULT_MODEL_ID, LocalStateProvider, PAID_DEFAULT_MODEL_ID } from "./LocalStateContext";
import {
  useBillingState,
  useModelState,
  useSelectedProjectState,
  useSidebarSearchState
} from "./useLocalState";

class CountingMemoryStorage {
  private readonly values = new Map<string, string>();
  readonly reads = new Map<string, number>();

  getItem(key: string): string | null {
    this.reads.set(key, (this.reads.get(key) ?? 0) + 1);
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }
}

type DomainSnapshots = {
  model?: ModelState;
  billing?: BillingState;
  sidebarSearch?: SidebarSearchState;
  selectedProject?: SelectedProjectState;
};

type RenderCounts = {
  model: number;
  billing: number;
  sidebarSearch: number;
  selectedProject: number;
};

type ProbeProps = {
  snapshots: DomainSnapshots;
  counts: RenderCounts;
};

const ModelProbe = memo(function ModelProbe({ snapshots, counts }: ProbeProps) {
  counts.model += 1;
  snapshots.model = useModelState();
  return null;
});

const BillingProbe = memo(function BillingProbe({ snapshots, counts }: ProbeProps) {
  counts.billing += 1;
  snapshots.billing = useBillingState();
  return null;
});

const SidebarSearchProbe = memo(function SidebarSearchProbe({ snapshots, counts }: ProbeProps) {
  counts.sidebarSearch += 1;
  snapshots.sidebarSearch = useSidebarSearchState();
  return null;
});

const SelectedProjectProbe = memo(function SelectedProjectProbe({ snapshots, counts }: ProbeProps) {
  counts.selectedProject += 1;
  snapshots.selectedProject = useSelectedProjectState();
  return null;
});

function DomainProbes(props: ProbeProps) {
  return (
    <>
      <ModelProbe {...props} />
      <BillingProbe {...props} />
      <SidebarSearchProbe {...props} />
      <SelectedProjectProbe {...props} />
    </>
  );
}

function createCounts(): RenderCounts {
  return { model: 0, billing: 0, sidebarSearch: 0, selectedProject: 0 };
}

function expectRenderDelta(
  counts: RenderCounts,
  before: RenderCounts,
  expected: Partial<RenderCounts>
) {
  expect({
    model: counts.model - before.model,
    billing: counts.billing - before.billing,
    sidebarSearch: counts.sidebarSearch - before.sidebarSearch,
    selectedProject: counts.selectedProject - before.selectedProject
  }).toEqual({
    model: expected.model ?? 0,
    billing: expected.billing ?? 0,
    sidebarSearch: expected.sidebarSearch ?? 0,
    selectedProject: expected.selectedProject ?? 0
  });
}

function freeBillingStatus(): BillingStatus {
  return {
    is_subscribed: false,
    stripe_customer_id: null,
    product_id: "free",
    product_name: "Free",
    subscription_status: "inactive",
    current_period_end: null,
    can_chat: true,
    chats_remaining: null,
    payment_provider: null,
    total_tokens: null,
    used_tokens: null,
    usage_reset_date: null
  };
}

function proBillingStatus(): BillingStatus {
  return {
    ...freeBillingStatus(),
    is_subscribed: true,
    product_id: "pro",
    product_name: "Pro",
    subscription_status: "active",
    payment_provider: "stripe"
  };
}

describe("LocalStateProvider", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) {
      act(() => renderer?.unmount());
      renderer = null;
    }
  });

  test("isolates updates to the domain that changed", () => {
    const storage = new CountingMemoryStorage();
    const snapshots: DomainSnapshots = {};
    const counts = createCounts();

    act(() => {
      renderer = create(
        <LocalStateProvider storage={storage}>
          <DomainProbes snapshots={snapshots} counts={counts} />
        </LocalStateProvider>
      );
    });

    let before = { ...counts };
    act(() => snapshots.model?.setModel(PAID_DEFAULT_MODEL_ID));
    expectRenderDelta(counts, before, { model: 1 });

    before = { ...counts };
    act(() => snapshots.billing?.setBillingStatus(freeBillingStatus()));
    expectRenderDelta(counts, before, { billing: 1 });

    before = { ...counts };
    act(() => {
      snapshots.sidebarSearch?.setSearchQuery("maple");
      snapshots.sidebarSearch?.setIsSearchVisible(true);
    });
    expectRenderDelta(counts, before, { sidebarSearch: 1 });

    before = { ...counts };
    act(() => snapshots.selectedProject?.setSelectedProjectId("project-1"));
    expectRenderDelta(counts, before, { selectedProject: 1 });

    before = { ...counts };
    act(() => snapshots.selectedProject?.setSelectedProjectId("project-1"));
    expectRenderDelta(counts, before, {});
  });

  test("keeps the intentional paid-plan model default scoped to billing and model consumers", () => {
    const storage = new CountingMemoryStorage();
    const snapshots: DomainSnapshots = {};
    const counts = createCounts();

    act(() => {
      renderer = create(
        <LocalStateProvider storage={storage}>
          <DomainProbes snapshots={snapshots} counts={counts} />
        </LocalStateProvider>
      );
    });

    const before = { ...counts };
    act(() => snapshots.billing?.setBillingStatus(proBillingStatus()));

    expectRenderDelta(counts, before, { model: 1, billing: 1 });
    expect(snapshots.model?.model).toBe(PAID_DEFAULT_MODEL_ID);
  });

  test("tracks billing ownership and clears only the account-scoped snapshot", () => {
    const snapshots: DomainSnapshots = {};
    const counts = createCounts();

    act(() => {
      renderer = create(
        <LocalStateProvider storage={null}>
          <DomainProbes snapshots={snapshots} counts={counts} />
        </LocalStateProvider>
      );
    });

    act(() => snapshots.billing?.setBillingStatus(freeBillingStatus(), "account-a"));
    expect(snapshots.billing?.billingStatus?.product_name).toBe("Free");
    expect(snapshots.billing?.billingStatusAccountId).toBe("account-a");

    const before = { ...counts };
    act(() => snapshots.billing?.clearBillingStatus());

    expect(snapshots.billing?.billingStatus).toBeNull();
    expect(snapshots.billing?.billingStatusAccountId).toBeNull();
    expectRenderDelta(counts, before, { billing: 1 });
  });

  test("preserves an in-memory model choice when storage is unavailable", () => {
    const snapshots: DomainSnapshots = {};
    const counts = createCounts();

    act(() => {
      renderer = create(
        <LocalStateProvider storage={null}>
          <DomainProbes snapshots={snapshots} counts={counts} />
        </LocalStateProvider>
      );
    });

    act(() => {
      snapshots.model?.setModel("user-selected-model", {
        id: "user-selected-model",
        object: "model",
        created: 1,
        owned_by: "opensecret"
      });
      snapshots.billing?.setBillingStatus(proBillingStatus());
    });

    expect(snapshots.model?.model).toBe("user-selected-model");
  });

  test("persists model choices without making a no-op selection sticky", () => {
    const storage = new CountingMemoryStorage();
    const snapshots: DomainSnapshots = {};
    const counts = createCounts();

    act(() => {
      renderer = create(
        <LocalStateProvider storage={storage}>
          <DomainProbes snapshots={snapshots} counts={counts} />
        </LocalStateProvider>
      );
    });

    act(() => snapshots.model?.setModel(DEFAULT_MODEL_ID));
    expect(storage.getItem("selectedModel")).toBeNull();

    act(() => {
      snapshots.model?.setModel("user-selected-model", {
        id: "user-selected-model",
        object: "model",
        created: 1,
        owned_by: "opensecret"
      });
      snapshots.model?.setModel(DEFAULT_MODEL_ID);
    });

    expect(snapshots.model?.model).toBe(DEFAULT_MODEL_ID);
    expect(storage.getItem("selectedModel")).toBe(DEFAULT_MODEL_ID);
    expect(storage.getItem("selectedModelMetadata")).toBeNull();
  });

  test("reads cached model state only during lazy provider initialization", () => {
    const storage = new CountingMemoryStorage();
    const snapshots: DomainSnapshots = {};
    const counts = createCounts();
    storage.setItem("selectedModel", "cached-model");
    storage.setItem(
      "selectedModelMetadata",
      JSON.stringify({
        id: "cached-model",
        object: "model",
        created: 1,
        owned_by: "opensecret"
      })
    );

    act(() => {
      renderer = create(
        <LocalStateProvider storage={storage}>
          <DomainProbes snapshots={snapshots} counts={counts} />
        </LocalStateProvider>
      );
    });

    const initialReads = new Map(storage.reads);
    expect([...initialReads.values()].reduce((sum, reads) => sum + reads, 0)).toBeGreaterThan(0);
    expect(snapshots.model?.model).toBe("cached-model");

    act(() => {
      snapshots.sidebarSearch?.setSearchQuery("persist across routes");
      snapshots.selectedProject?.setSelectedProjectId("project-2");
    });

    act(() => {
      renderer?.update(
        <LocalStateProvider storage={storage}>
          <DomainProbes snapshots={snapshots} counts={counts} />
          <span>another route</span>
        </LocalStateProvider>
      );
    });

    expect(storage.reads).toEqual(initialReads);
    expect(snapshots.sidebarSearch?.searchQuery).toBe("persist across routes");
    expect(snapshots.selectedProject?.selectedProjectId).toBe("project-2");
  });
});
