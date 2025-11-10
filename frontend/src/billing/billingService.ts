import { OpenSecretContextType } from "@opensecret/react";
import {
  fetchBillingStatus,
  fetchPortalUrl,
  fetchProducts,
  createCheckoutSession,
  createZapriteCheckoutSession,
  BillingStatus,
  BillingProduct,
  fetchTeamStatus,
  createTeam,
  inviteTeamMembers,
  fetchTeamMembers,
  checkTeamInvite,
  acceptTeamInvite,
  removeTeamMember,
  leaveTeam,
  revokeTeamInvite,
  updateTeamName,
  fetchApiCreditBalance,
  fetchApiCreditSettings,
  purchaseApiCredits,
  purchaseApiCreditsZaprite,
  updateApiCreditSettings,
  ApiCreditBalance,
  ApiCreditSettings,
  PurchaseCreditsRequest,
  PurchaseCreditsZapriteRequest,
  CheckoutResponse,
  UpdateCreditSettingsRequest,
  checkPassCode,
  redeemPassCode,
  PassCheckResponse,
  PassRedeemRequest,
  PassRedeemResponse
} from "./billingApi";
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

const TOKEN_STORAGE_KEY = "maple_billing_token";

class BillingService {
  private os: OpenSecretContextType;

  constructor(os: OpenSecretContextType) {
    this.os = os;
  }

  private async getStoredToken(): Promise<string | null> {
    return sessionStorage.getItem(TOKEN_STORAGE_KEY);
  }

  private async generateAndStoreToken(): Promise<string> {
    const token = await this.os.generateThirdPartyToken(import.meta.env.VITE_MAPLE_BILLING_API_URL);
    sessionStorage.setItem(TOKEN_STORAGE_KEY, token.token);
    return token.token;
  }

  private async executeWithToken<T>(apiCall: (token: string) => Promise<T>): Promise<T> {
    // Try with stored token first
    const storedToken = await this.getStoredToken();
    if (storedToken) {
      try {
        return await apiCall(storedToken);
      } catch (error) {
        // If unauthorized or invalid token, try with new token
        if (
          error instanceof Error &&
          (error.message.includes("unauthorized") ||
            error.message.includes("Unauthorized") ||
            error.message.includes("Invalid JWT token") ||
            error.message.includes("401"))
        ) {
          // Clear the invalid token
          this.clearToken();
          // Generate new token
          const newToken = await this.generateAndStoreToken();
          return await apiCall(newToken);
        }
        throw error;
      }
    }

    // No stored token, generate new one
    const newToken = await this.generateAndStoreToken();
    return await apiCall(newToken);
  }

  async getBillingStatus(): Promise<BillingStatus> {
    return this.executeWithToken((token) => fetchBillingStatus(token));
  }

  async getPortalUrl(): Promise<string> {
    return this.executeWithToken((token) => fetchPortalUrl(token));
  }

  async getProducts(version?: string): Promise<BillingProduct[]> {
    return fetchProducts(version);
  }

  async createCheckoutSession(
    email: string,
    productId: string,
    successUrl: string,
    cancelUrl: string,
    quantity?: number
  ): Promise<void> {
    return this.executeWithToken((token) =>
      createCheckoutSession(token, email, productId, successUrl, cancelUrl, quantity)
    );
  }

  async createZapriteCheckoutSession(
    email: string,
    productId: string,
    successUrl: string,
    quantity?: number
  ): Promise<void> {
    return this.executeWithToken((token) =>
      createZapriteCheckoutSession(token, email, productId, successUrl, quantity)
    );
  }

  clearToken(): void {
    sessionStorage.removeItem(TOKEN_STORAGE_KEY);
  }

  // Team Management Methods
  async getTeamStatus(): Promise<TeamStatus> {
    return this.executeWithToken((token) => fetchTeamStatus(token));
  }

  async createTeam(data: CreateTeamRequest): Promise<CreateTeamResponse> {
    return this.executeWithToken((token) => createTeam(token, data));
  }

  async inviteTeamMembers(data: InviteMembersRequest): Promise<InviteMembersResponse> {
    return this.executeWithToken((token) => inviteTeamMembers(token, data));
  }

  async getTeamMembers(): Promise<TeamMembersResponse> {
    return this.executeWithToken((token) => fetchTeamMembers(token));
  }

  async checkTeamInvite(inviteId: string): Promise<CheckInviteResponse> {
    return this.executeWithToken((token) => checkTeamInvite(token, inviteId));
  }

  async acceptTeamInvite(inviteId: string, data: AcceptInviteRequest): Promise<TeamStatus> {
    return this.executeWithToken((token) => acceptTeamInvite(token, inviteId, data));
  }

  async removeTeamMember(userId: string): Promise<void> {
    return this.executeWithToken((token) => removeTeamMember(token, userId));
  }

  async leaveTeam(): Promise<void> {
    return this.executeWithToken((token) => leaveTeam(token));
  }

  async revokeTeamInvite(inviteId: string): Promise<void> {
    return this.executeWithToken((token) => revokeTeamInvite(token, inviteId));
  }

  async updateTeamName(name: string): Promise<UpdateTeamNameResponse> {
    return this.executeWithToken((token) => updateTeamName(token, name));
  }

  // API Credits methods
  async getApiCreditBalance(): Promise<ApiCreditBalance> {
    return this.executeWithToken((token) => fetchApiCreditBalance(token));
  }

  async getApiCreditSettings(): Promise<ApiCreditSettings> {
    return this.executeWithToken((token) => fetchApiCreditSettings(token));
  }

  async purchaseApiCredits(data: PurchaseCreditsRequest): Promise<CheckoutResponse> {
    return this.executeWithToken((token) => purchaseApiCredits(token, data));
  }

  async purchaseApiCreditsZaprite(data: PurchaseCreditsZapriteRequest): Promise<CheckoutResponse> {
    return this.executeWithToken((token) => purchaseApiCreditsZaprite(token, data));
  }

  async updateApiCreditSettings(data: UpdateCreditSettingsRequest): Promise<ApiCreditSettings> {
    return this.executeWithToken((token) => updateApiCreditSettings(token, data));
  }

  // Subscription Pass methods
  async checkPassCode(passCode: string): Promise<PassCheckResponse> {
    return checkPassCode(passCode);
  }

  async redeemPassCode(data: PassRedeemRequest): Promise<PassRedeemResponse> {
    return this.executeWithToken((token) => redeemPassCode(token, data));
  }
}

// Singleton instance
let billingServiceInstance: BillingService | null = null;

export function initBillingService(os: OpenSecretContextType): BillingService {
  if (!billingServiceInstance) {
    billingServiceInstance = new BillingService(os);
  }
  return billingServiceInstance;
}

export function getBillingService(): BillingService {
  if (!billingServiceInstance) {
    throw new Error("Billing service not initialized. Call initBillingService first.");
  }
  return billingServiceInstance;
}
