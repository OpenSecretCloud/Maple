import { OpenSecretContextType } from "@opensecret/react";
import {
  fetchBillingStatus,
  fetchPortalUrl,
  fetchProducts,
  fetchDiscount,
  createCheckoutSession,
  createZapriteCheckoutSession,
  BillingStatus,
  BillingProduct,
  DiscountResponse,
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

type StoredBillingCredential = {
  accountId: string;
  token: string;
};

type BillingOperationScope = {
  accountId: string;
  epoch: number;
  os: OpenSecretContextType;
};

class BillingSessionChangedError extends Error {
  constructor() {
    super("The billing session changed while the request was in progress.");
    this.name = "BillingSessionChangedError";
  }
}

function getAccountId(os: OpenSecretContextType): string | null {
  return os.auth.user?.user.id ?? null;
}

function isUnauthorizedError(error: unknown): boolean {
  return (
    error instanceof Error &&
    (error.message.includes("unauthorized") ||
      error.message.includes("Unauthorized") ||
      error.message.includes("Invalid JWT token") ||
      error.message.includes("401"))
  );
}

class BillingService {
  private os: OpenSecretContextType;
  private accountId: string | null;
  private credentialEpoch = 0;

  constructor(os: OpenSecretContextType) {
    this.os = os;
    this.accountId = getAccountId(os);
  }

  updateOpenSecret(os: OpenSecretContextType): void {
    const nextAccountId = getAccountId(os);
    if (nextAccountId !== this.accountId) {
      this.invalidateCredentials();
    }

    this.os = os;
    this.accountId = nextAccountId;
  }

  private removeStoredCredential(): void {
    sessionStorage.removeItem(TOKEN_STORAGE_KEY);
  }

  private invalidateCredentials(): void {
    this.credentialEpoch += 1;
    this.removeStoredCredential();
  }

  private captureScope(expectedAccountId?: string): BillingOperationScope {
    if (
      !this.accountId ||
      (expectedAccountId !== undefined && expectedAccountId !== this.accountId)
    ) {
      throw new BillingSessionChangedError();
    }

    return {
      accountId: this.accountId,
      epoch: this.credentialEpoch,
      os: this.os
    };
  }

  private assertCurrentScope(scope: BillingOperationScope): void {
    if (scope.accountId !== this.accountId || scope.epoch !== this.credentialEpoch) {
      throw new BillingSessionChangedError();
    }
  }

  private getStoredToken(scope: BillingOperationScope): string | null {
    this.assertCurrentScope(scope);
    const serialized = sessionStorage.getItem(TOKEN_STORAGE_KEY);
    if (!serialized) return null;

    try {
      const credential = JSON.parse(serialized) as Partial<StoredBillingCredential>;
      if (
        credential.accountId === scope.accountId &&
        typeof credential.token === "string" &&
        credential.token.length > 0
      ) {
        return credential.token;
      }
    } catch {
      // Legacy raw tokens and malformed credentials are intentionally discarded.
    }

    this.removeStoredCredential();
    return null;
  }

  private async generateAndStoreToken(scope: BillingOperationScope): Promise<string> {
    const token = await scope.os.generateThirdPartyToken(
      import.meta.env.VITE_MAPLE_BILLING_API_URL
    );
    this.assertCurrentScope(scope);
    sessionStorage.setItem(
      TOKEN_STORAGE_KEY,
      JSON.stringify({
        accountId: scope.accountId,
        token: token.token
      } satisfies StoredBillingCredential)
    );
    return token.token;
  }

  private async callWithToken<T>(
    scope: BillingOperationScope,
    token: string,
    apiCall: (token: string) => Promise<T>
  ): Promise<T> {
    this.assertCurrentScope(scope);

    try {
      const result = await apiCall(token);
      this.assertCurrentScope(scope);
      return result;
    } catch (error) {
      this.assertCurrentScope(scope);
      throw error;
    }
  }

  private async executeWithToken<T>(
    apiCall: (token: string) => Promise<T>,
    expectedAccountId?: string
  ): Promise<T> {
    let scope = this.captureScope(expectedAccountId);
    const storedToken = this.getStoredToken(scope);
    if (storedToken) {
      try {
        return await this.callWithToken(scope, storedToken, apiCall);
      } catch (error) {
        this.assertCurrentScope(scope);
        if (!isUnauthorizedError(error)) throw error;

        this.clearToken();
        scope = this.captureScope(expectedAccountId);
        const newToken = await this.generateAndStoreToken(scope);
        return this.callWithToken(scope, newToken, apiCall);
      }
    }

    const newToken = await this.generateAndStoreToken(scope);
    return this.callWithToken(scope, newToken, apiCall);
  }

  async getBillingStatus(expectedAccountId?: string): Promise<BillingStatus> {
    return this.executeWithToken((token) => fetchBillingStatus(token), expectedAccountId);
  }

  async getPortalUrl(): Promise<string> {
    return this.executeWithToken((token) => fetchPortalUrl(token));
  }

  async getProducts(version?: string): Promise<BillingProduct[]> {
    return fetchProducts(version);
  }

  async getDiscount(): Promise<DiscountResponse> {
    return fetchDiscount();
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
    this.invalidateCredentials();
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
  } else {
    billingServiceInstance.updateOpenSecret(os);
  }
  return billingServiceInstance;
}

export function getBillingService(): BillingService {
  if (!billingServiceInstance) {
    throw new Error("Billing service not initialized. Call initBillingService first.");
  }
  return billingServiceInstance;
}
