import { expect, spyOn, test } from "bun:test";
import { fetchProducts } from "./billingApi";

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
