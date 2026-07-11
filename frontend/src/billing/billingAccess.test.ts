import { describe, expect, test } from "bun:test";
import type { BillingStatus } from "./billingApi";
import { hasApiAccess } from "./billingAccess";

function billingStatus(productName: string): BillingStatus {
  return {
    is_subscribed: productName !== "Free",
    stripe_customer_id: null,
    product_id: "test-product",
    product_name: productName,
    subscription_status: "active",
    current_period_end: null,
    can_chat: true,
    chats_remaining: null,
    payment_provider: "stripe",
    total_tokens: null,
    used_tokens: null,
    usage_reset_date: null
  };
}

describe("hasApiAccess", () => {
  test.each(["Pro", "Max", "Team"])("allows the %s plan", (productName) => {
    expect(hasApiAccess(billingStatus(productName))).toBe(true);
  });

  test.each([null, undefined])("fails closed when billing is %s", (status) => {
    expect(hasApiAccess(status)).toBe(false);
  });

  test.each(["Free", "Starter"])("does not allow the %s plan", (productName) => {
    expect(hasApiAccess(billingStatus(productName))).toBe(false);
  });
});
