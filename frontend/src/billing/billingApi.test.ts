import { expect, spyOn, test } from "bun:test";
import { createZapriteUpgrade, fetchProducts } from "./billingApi";

test("fetchProducts rejects an HTTP error response before parsing it as products", async () => {
  const previousBillingApiUrl = process.env.VITE_MAPLE_BILLING_API_URL;
  process.env.VITE_MAPLE_BILLING_API_URL = "https://billing.example.test";
  const fetchSpy = spyOn(globalThis, "fetch").mockResolvedValue(
    new Response("service unavailable", { status: 503 })
  );
  const consoleErrorSpy = spyOn(console, "error").mockImplementation(() => {});

  try {
    await expect(fetchProducts()).rejects.toThrow(
      "Failed to fetch billing products: service unavailable"
    );
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  } finally {
    if (previousBillingApiUrl === undefined) {
      delete process.env.VITE_MAPLE_BILLING_API_URL;
    } else {
      process.env.VITE_MAPLE_BILLING_API_URL = previousBillingApiUrl;
    }
    fetchSpy.mockRestore();
    consoleErrorSpy.mockRestore();
  }
});

test("createZapriteUpgrade rejects a checkout URL from the wrong host", async () => {
  const previousBillingApiUrl = process.env.VITE_MAPLE_BILLING_API_URL;
  process.env.VITE_MAPLE_BILLING_API_URL = "https://billing.example.test";
  const fetchSpy = spyOn(globalThis, "fetch").mockResolvedValue(
    new Response(
      JSON.stringify({
        upgrade_id: "upgrade-1",
        status: "PENDING",
        checkout_url: "https://evil.example/pay",
        quote: {
          quote_id: "quote-1",
          source: { product_id: "pro", plan_name: "Pro", annual_amount_cents: 21600 },
          target: {
            product_id: "max",
            plan_name: "Max",
            monthly_amount_cents: 10000,
            annual_amount_cents: 108000,
            discount_basis_points: 1000
          },
          subscription_start: "2026-01-01T00:00:00Z",
          subscription_end: "2027-01-01T00:00:00Z",
          quote_effective_at: "2026-09-01T00:00:00Z",
          period_seconds: 31536000,
          remaining_seconds: 10000000,
          amount_due_cents: 80956,
          currency: "USD",
          expires_at: "2026-09-01T00:15:00Z"
        }
      }),
      { status: 200, headers: { "Content-Type": "application/json" } }
    )
  );

  try {
    await expect(
      createZapriteUpgrade("token", "quote-1", "11111111-1111-1111-1111-111111111111")
    ).rejects.toThrow("Checkout URL is not from the expected payment provider");
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  } finally {
    if (previousBillingApiUrl === undefined) {
      delete process.env.VITE_MAPLE_BILLING_API_URL;
    } else {
      process.env.VITE_MAPLE_BILLING_API_URL = previousBillingApiUrl;
    }
    fetchSpy.mockRestore();
  }
});
