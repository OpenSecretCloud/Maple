import { expect, test } from "bun:test";
import {
  isAllowedZapriteCheckoutUrl,
  zapritePaidPlanButtonText,
  zapriteUpgradeTarget
} from "./zapriteUpgrade";
import type { BillingStatus } from "./billingApi";

const zapritePro: BillingStatus = {
  is_subscribed: true,
  stripe_customer_id: null,
  product_id: "local-pro",
  product_name: "Pro",
  subscription_status: "active",
  current_period_end: null,
  can_chat: true,
  chats_remaining: null,
  payment_provider: "zaprite",
  total_tokens: null,
  used_tokens: null,
  usage_reset_date: null
};

test("eligible Zaprite Pro renders upgrade CTA rather than support email", () => {
  expect(zapritePaidPlanButtonText(zapritePro, { id: "local-max", name: "Max" }, false)).toBe(
    "Upgrade to Max"
  );
  expect(zapriteUpgradeTarget(zapritePro, { id: "local-max", name: "Max" })).toBe(true);
  expect(zapritePaidPlanButtonText(zapritePro, { id: "local-pro", name: "Pro" }, true)).toBe(
    "Start Chatting"
  );
  expect(zapritePaidPlanButtonText(zapritePro, { id: "team", name: "Team" }, false)).toBe(
    "Contact Us"
  );
});

test("checkout URL allowlist accepts Zaprite and local mock hosts", () => {
  expect(isAllowedZapriteCheckoutUrl("https://checkout.zaprite.com/abc")).toBe(true);
  expect(isAllowedZapriteCheckoutUrl("http://checkout.zaprite.com/abc")).toBe(false);
  expect(isAllowedZapriteCheckoutUrl("https://evil.example/abc")).toBe(false);
  expect(isAllowedZapriteCheckoutUrl("http://127.0.0.1:36683/v1/dev/zaprite-mock/checkout/x")).toBe(
    true
  );
});
