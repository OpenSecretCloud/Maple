export type BillingStatus = {
  is_subscribed: boolean;
  stripe_customer_id: string | null;
  product_id: string;
  product_name: string;
  subscription_status: string;
  current_period_end: string | null;
  can_chat: boolean;
  chats_remaining: number | null;
  payment_provider: "stripe" | "zaprite" | null;
};

type BillingRecurringInfo = {
  interval: string;
  interval_count: number;
};

type BillingPriceInfo = {
  id: string;
  currency: string;
  unit_amount: number;
  recurring: BillingRecurringInfo;
};

export type BillingProduct = {
  id: string;
  name: string;
  description: string;
  active: boolean;
  default_price: BillingPriceInfo;
};

export async function fetchBillingStatus(thirdPartyToken: string): Promise<BillingStatus> {
  try {
    const response = await fetch(
      `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/subscription/status`,
      {
        headers: {
          Authorization: `Bearer ${thirdPartyToken}`,
          "Content-Type": "application/json"
        }
      }
    );

    if (!response.ok) {
      const errorText = await response.text();
      console.error("Billing status error response:", errorText);
      if (response.status === 401) {
        throw new Error("Unauthorized");
      }
      throw new Error(`Failed to fetch billing status: ${errorText}`);
    }

    return response.json() as Promise<BillingStatus>;
  } catch (error) {
    console.error("Error fetching billing status:", error);
    throw error;
  }
}

export async function fetchProducts(): Promise<BillingProduct[]> {
  try {
    const response = await fetch(
      `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/products`,
      {
        headers: {
          "Content-Type": "application/json"
        }
      }
    );
    return response.json() as Promise<BillingProduct[]>;
  } catch (error) {
    console.error("Error fetching billing products:", error);
    throw error;
  }
}

export async function fetchPortalUrl(thirdPartyToken: string): Promise<string> {
  const requestBody = {
    return_url: window.location.origin
  };
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/subscription/portal`,
    {
      method: "POST",
      body: JSON.stringify(requestBody),
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Portal URL error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to fetch portal URL: ${errorText}`);
  }

  const { portal_url } = await response.json();
  return portal_url;
}

export async function createCheckoutSession(
  thirdPartyToken: string,
  email: string,
  productId: string,
  successUrl: string,
  cancelUrl: string
): Promise<void> {
  const requestBody = {
    email,
    product_id: productId,
    success_url: successUrl,
    cancel_url: cancelUrl
  };
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/subscription/checkout`,
    {
      method: "POST",
      body: JSON.stringify(requestBody),
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Checkout error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to create checkout session: ${errorText}`);
  }

  const { checkout_url } = await response.json();
  console.log("Redirecting to checkout:", checkout_url);
  window.location.href = checkout_url;
}

export async function createZapriteCheckoutSession(
  thirdPartyToken: string,
  email: string,
  productId: string,
  successUrl: string
): Promise<void> {
  const requestBody = {
    email,
    product_id: productId,
    success_url: successUrl
  };
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/subscription/zaprite`,
    {
      method: "POST",
      body: JSON.stringify(requestBody),
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Zaprite checkout error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to create Zaprite checkout session: ${errorText}`);
  }

  const { checkout_url } = await response.json();
  console.log("Redirecting to Zaprite checkout:", checkout_url);
  window.location.href = checkout_url;
}
