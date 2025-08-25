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
  total_tokens: number | null;
  used_tokens: number | null;
  usage_reset_date: string | null;
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
  is_available?: boolean;
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

export async function fetchProducts(version?: string): Promise<BillingProduct[]> {
  try {
    const url = new URL(`${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/products`);
    if (version) {
      url.searchParams.append("version", version);
    }
    const response = await fetch(url.toString(), {
      headers: {
        "Content-Type": "application/json"
      }
    });
    return response.json() as Promise<BillingProduct[]>;
  } catch (error) {
    console.error("Error fetching billing products:", error);
    throw error;
  }
}

export async function fetchPortalUrl(thirdPartyToken: string): Promise<string> {
  let returnUrl = window.location.origin;

  try {
    // Check if we're in a Tauri environment
    const isTauri = await import("@tauri-apps/api/core")
      .then((m) => m.isTauri())
      .catch(() => false);

    if (isTauri) {
      // For iOS, use the actual website origin instead of tauri://localhost
      const { type } = await import("@tauri-apps/plugin-os");
      const platform = await type();

      if (platform === "ios") {
        console.log("[Billing] iOS detected, using trymaple.ai as return URL");
        returnUrl = "https://trymaple.ai";
      }
    }
  } catch (error) {
    console.error("[Billing] Error determining platform:", error);
  }

  const requestBody = {
    return_url: returnUrl
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

  try {
    // Check if we're in a Tauri environment
    const isTauri = await import("@tauri-apps/api/core")
      .then((m) => m.isTauri())
      .catch(() => false);

    if (isTauri) {
      // Log the URL we're about to open
      console.log("[Billing] Opening URL in external browser:", checkout_url);

      // For iOS, we need to force Safari to open with no fallback
      const { type } = await import("@tauri-apps/plugin-os");
      const platform = await type();
      const { invoke } = await import("@tauri-apps/api/core");

      if (platform === "ios") {
        console.log("[Billing] iOS detected, using opener plugin to launch Safari");

        // Use the opener plugin directly - with NO fallback for iOS
        await invoke("plugin:opener|open_url", { url: checkout_url })
          .then(() => {
            console.log("[Billing] Successfully opened URL in external browser");
          })
          .catch((error: Error) => {
            console.error("[Billing] Failed to open external browser:", error);
            throw new Error(
              "Failed to open payment page in external browser. This is required for iOS payments."
            );
          });

        // Add a small delay to ensure the browser has time to open
        await new Promise((resolve) => setTimeout(resolve, 300));
        return;
      } else {
        // For other platforms, maintain original behavior - use window.location directly
        window.location.href = checkout_url;
        return;
      }
    }
  } catch (error) {
    console.error("[Billing] Error opening URL with Tauri:", error);
  }

  // Fall back to regular navigation if not on Tauri or if Tauri opener fails
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

  try {
    // Check if we're in a Tauri environment
    const isTauri = await import("@tauri-apps/api/core")
      .then((m) => m.isTauri())
      .catch(() => false);

    if (isTauri) {
      // Log the URL we're about to open
      console.log("[Billing] Opening URL in external browser:", checkout_url);

      // For iOS, we need to force Safari to open with no fallback
      const { type } = await import("@tauri-apps/plugin-os");
      const platform = await type();
      const { invoke } = await import("@tauri-apps/api/core");

      if (platform === "ios") {
        console.log("[Billing] iOS detected, using opener plugin to launch Safari");

        // Use the opener plugin directly - with NO fallback for iOS
        await invoke("plugin:opener|open_url", { url: checkout_url })
          .then(() => {
            console.log("[Billing] Successfully opened URL in external browser");
          })
          .catch((error: Error) => {
            console.error("[Billing] Failed to open external browser:", error);
            throw new Error(
              "Failed to open payment page in external browser. This is required for iOS payments."
            );
          });

        // Add a small delay to ensure the browser has time to open
        await new Promise((resolve) => setTimeout(resolve, 300));
        return;
      } else {
        // For other platforms, maintain original behavior - use window.location directly
        window.location.href = checkout_url;
        return;
      }
    }
  } catch (error) {
    console.error("[Billing] Error opening URL with Tauri:", error);
  }

  // Fall back to regular navigation if not on Tauri or if Tauri opener fails
  window.location.href = checkout_url;
}

// Team Management API Functions
import type {
  TeamStatus,
  CreateTeamRequest,
  CreateTeamResponse,
  InviteMembersRequest,
  InviteMembersResponse,
  TeamMembersResponse,
  CheckInviteResponse,
  AcceptInviteRequest,
  UpdateTeamNameResponse
} from "@/types/team";

export async function fetchTeamStatus(thirdPartyToken: string): Promise<TeamStatus> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/team/status`,
    {
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Team status error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to fetch team status: ${errorText}`);
  }

  return response.json();
}

export async function createTeam(
  thirdPartyToken: string,
  data: CreateTeamRequest
): Promise<CreateTeamResponse> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/team/create`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      },
      body: JSON.stringify(data)
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Create team error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to create team: ${errorText}`);
  }

  return response.json();
}

export async function inviteTeamMembers(
  thirdPartyToken: string,
  data: InviteMembersRequest
): Promise<InviteMembersResponse> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/team/invites`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      },
      body: JSON.stringify(data)
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Invite members error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    if (response.status === 400) {
      // Parse error message for user-friendly display
      try {
        const errorData = JSON.parse(errorText);
        throw new Error(errorData.message || errorText);
      } catch (parseError) {
        console.error("Failed to parse error response:", parseError);
        throw new Error(errorText);
      }
    }
    throw new Error(`Failed to invite members: ${errorText}`);
  }

  return response.json();
}

export async function fetchTeamMembers(thirdPartyToken: string): Promise<TeamMembersResponse> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/team/members`,
    {
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Fetch team members error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to fetch team members: ${errorText}`);
  }

  return response.json();
}

export async function checkTeamInvite(
  thirdPartyToken: string,
  inviteId: string
): Promise<CheckInviteResponse> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/team/invites/${inviteId}/check`,
    {
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Check invite error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to check invite: ${errorText}`);
  }

  return response.json();
}

export async function acceptTeamInvite(
  thirdPartyToken: string,
  inviteId: string,
  data: AcceptInviteRequest
): Promise<TeamStatus> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/team/invites/${inviteId}/accept`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      },
      body: JSON.stringify(data)
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Accept invite error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }

    // Try to parse JSON error response for any status code
    try {
      const errorData = JSON.parse(errorText);
      throw new Error(errorData.error || errorData.message || errorText);
    } catch (parseError) {
      console.error("Failed to parse error response:", parseError);
      // If not JSON, use the text as-is
      throw new Error(errorText || "Failed to accept invitation");
    }
  }

  return response.json();
}

export async function removeTeamMember(thirdPartyToken: string, userId: string): Promise<void> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/team/members/${userId}`,
    {
      method: "DELETE",
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Remove member error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    if (response.status === 403) {
      throw new Error("Only team admins can remove members");
    }
    throw new Error(`Failed to remove member: ${errorText}`);
  }
}

export async function leaveTeam(thirdPartyToken: string): Promise<void> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/team/leave`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Leave team error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to leave team: ${errorText}`);
  }
}

export async function revokeTeamInvite(thirdPartyToken: string, inviteId: string): Promise<void> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/team/invites/${inviteId}`,
    {
      method: "DELETE",
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Revoke invite error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    if (response.status === 403) {
      throw new Error("Only team admins can revoke invites");
    }
    throw new Error(`Failed to revoke invite: ${errorText}`);
  }
}

export async function updateTeamName(
  thirdPartyToken: string,
  name: string
): Promise<UpdateTeamNameResponse> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/team/update`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      },
      body: JSON.stringify({ name: name.trim() })
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Update team name error response:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    if (response.status === 400) {
      // Parse error message for user-friendly display
      try {
        const errorData = JSON.parse(errorText);
        throw new Error(errorData.message || errorText);
      } catch (parseError) {
        console.error("Failed to parse error response:", parseError);
        if (errorText.includes("team admin")) {
          throw new Error("You are not a team admin");
        }
        if (errorText.includes("between 1 and 100 characters")) {
          throw new Error("Team name must be between 1 and 100 characters");
        }
        throw new Error(errorText);
      }
    }
    throw new Error(`Failed to update team name: ${errorText}`);
  }

  return response.json();
}

// API Credits Types
export type ApiCreditBalance = {
  balance: number;
  auto_topup_enabled: boolean;
  auto_topup_threshold: number;
  auto_topup_amount: number;
};

export type ApiCreditSettings = {
  user_id: string;
  auto_topup_enabled: boolean;
  auto_topup_threshold: number;
  auto_topup_amount: number;
  stripe_customer_id: string | null;
  stripe_payment_method_id: string | null;
  created_at: string;
  updated_at: string;
};

export type PurchaseCreditsRequest = {
  credits: number;
  success_url: string;
  cancel_url: string;
};

export type PurchaseCreditsZapriteRequest = {
  credits: number;
  email: string;
  success_url: string;
};

export type CheckoutResponse = {
  checkout_url: string;
};

export type UpdateCreditSettingsRequest = {
  auto_topup_enabled?: boolean;
  auto_topup_threshold?: number;
  auto_topup_amount?: number;
};

// API Credits Endpoints
export async function fetchApiCreditBalance(thirdPartyToken: string): Promise<ApiCreditBalance> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/api-credits/balance`,
    {
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Fetch credit balance error:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to fetch credit balance: ${errorText}`);
  }

  return response.json();
}

export async function fetchApiCreditSettings(thirdPartyToken: string): Promise<ApiCreditSettings> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/api-credits/settings`,
    {
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      }
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Fetch credit settings error:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to fetch credit settings: ${errorText}`);
  }

  return response.json();
}

export async function purchaseApiCredits(
  thirdPartyToken: string,
  data: PurchaseCreditsRequest
): Promise<CheckoutResponse> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/api-credits/purchase`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      },
      body: JSON.stringify(data)
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Purchase credits error:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    if (response.status === 400) {
      throw new Error("Minimum purchase is 10,000 credits ($10)");
    }
    throw new Error(`Failed to create checkout session: ${errorText}`);
  }

  return response.json();
}

export async function purchaseApiCreditsZaprite(
  thirdPartyToken: string,
  data: PurchaseCreditsZapriteRequest
): Promise<CheckoutResponse> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/api-credits/purchase-zaprite`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      },
      body: JSON.stringify(data)
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Purchase credits with Zaprite error:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    if (response.status === 400) {
      throw new Error("Minimum purchase is 10,000 credits ($10)");
    }
    throw new Error(`Failed to create Zaprite checkout: ${errorText}`);
  }

  return response.json();
}

export async function updateApiCreditSettings(
  thirdPartyToken: string,
  data: UpdateCreditSettingsRequest
): Promise<ApiCreditSettings> {
  const response = await fetch(
    `${import.meta.env.VITE_MAPLE_BILLING_API_URL}/v1/maple/api-credits/settings`,
    {
      method: "PUT",
      headers: {
        Authorization: `Bearer ${thirdPartyToken}`,
        "Content-Type": "application/json"
      },
      body: JSON.stringify(data)
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    console.error("Update credit settings error:", errorText);
    if (response.status === 401) {
      throw new Error("Unauthorized");
    }
    throw new Error(`Failed to update credit settings: ${errorText}`);
  }

  return response.json();
}
