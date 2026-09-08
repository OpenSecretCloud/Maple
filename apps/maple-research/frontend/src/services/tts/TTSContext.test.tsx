import { afterEach, describe, expect, mock, test } from "bun:test";
import { createRef, forwardRef, useImperativeHandle } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import type { BillingStatus } from "@/billing/billingApi";

const aiCustomFetch = mock(async () => {
  throw new Error("Free-plan TTS should not reach the backend");
});

mock.module("@opensecret/react", () => ({
  useOpenSecret: () => ({
    aiCustomFetch,
    apiUrl: "https://enclave.example"
  })
}));

const freeBillingStatus: BillingStatus = {
  is_subscribed: false,
  stripe_customer_id: null,
  product_id: "free",
  product_name: "Free",
  subscription_status: "active",
  current_period_end: null,
  can_chat: true,
  chats_remaining: 10,
  payment_provider: null,
  total_tokens: null,
  used_tokens: null,
  usage_reset_date: null
};

mock.module("@/state/useLocalState", () => ({
  useBillingState: () => ({ billingStatus: freeBillingStatus })
}));

const { TTSProvider, useTTS } = await import("./TTSContext");
type TTSContextSnapshot = ReturnType<typeof useTTS>;

const TTSProbe = forwardRef<TTSContextSnapshot>(function TTSProbe(_, ref) {
  const context = useTTS();
  useImperativeHandle(ref, () => context, [context]);
  return null;
});

describe("TTSProvider access handling", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) {
      act(() => renderer?.unmount());
      renderer = null;
    }
    aiCustomFetch.mockClear();
  });

  test("turns a known free-plan request into a consumable upgrade signal", async () => {
    const renderedContext = createRef<TTSContextSnapshot>();
    const currentContext = () => {
      if (!renderedContext.current) throw new Error("TTS context did not render");
      return renderedContext.current;
    };

    act(() => {
      renderer = create(
        <TTSProvider>
          <TTSProbe ref={renderedContext} />
        </TTSProvider>
      );
    });

    await act(async () => {
      await currentContext().speak("Read this response", "assistant-message");
    });

    expect(aiCustomFetch).not.toHaveBeenCalled();
    expect(currentContext().upgradeRequired).toBe(true);
    expect(currentContext().playbackError).toBeNull();
    expect(currentContext().isPreparing).toBe(false);
    expect(currentContext().isPlaying).toBe(false);

    act(() => currentContext().clearUpgradeRequired());
    expect(currentContext().upgradeRequired).toBe(false);
  });
});
