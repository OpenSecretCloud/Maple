import { describe, expect, test } from "bun:test";
import type { BillingStatus } from "./billingApi";
import { hasApiAccess, isKnownFreePlan, shouldWarnBeforeAccountDeletion } from "./billingAccess";

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

  test.each(["Free", "Unknown"])("does not allow the %s plan", (productName) => {
    expect(hasApiAccess(billingStatus(productName))).toBe(false);
  });
});

describe("isKnownFreePlan", () => {
  test("identifies a loaded free plan without treating unknown billing as free", () => {
    expect(isKnownFreePlan(billingStatus("Free"))).toBe(true);
    expect(isKnownFreePlan(billingStatus(""))).toBe(false);
    expect(isKnownFreePlan(billingStatus("Unknown"))).toBe(false);
    expect(isKnownFreePlan(billingStatus("Pro"))).toBe(false);
    expect(isKnownFreePlan(null)).toBe(false);
  });
});

describe("shouldWarnBeforeAccountDeletion", () => {
  test.each(["Pro", "Max", "Team"])("warns for the %s plan", (productName) => {
    expect(shouldWarnBeforeAccountDeletion(billingStatus(productName))).toBe(true);
  });

  test("does not warn for a free plan", () => {
    expect(shouldWarnBeforeAccountDeletion(billingStatus("Free"))).toBe(false);
  });

  test.each([null, undefined])("does not warn when billing is %s", (status) => {
    expect(shouldWarnBeforeAccountDeletion(status)).toBe(false);
  });

  test("warns when subscribed even if the product name is unrecognized", () => {
    expect(shouldWarnBeforeAccountDeletion(billingStatus("Unknown"))).toBe(true);
  });

  test("warns for a paid product name even if is_subscribed is false", () => {
    expect(
      shouldWarnBeforeAccountDeletion({
        ...billingStatus("Pro"),
        is_subscribed: false
      })
    ).toBe(true);
  });
});
