import { OpenSecretContextType } from "@opensecret/react";
import {
  fetchBillingStatus,
  fetchPortalUrl,
  fetchProducts,
  createCheckoutSession,
  createZapriteCheckoutSession,
  BillingStatus,
  BillingProduct,
  fetchTeamPlanAvailable,
  syncAppleTransaction
} from "./billingApi";
import { allowExternalBilling } from "../utils/region-gate";

const TOKEN_STORAGE_KEY = "maple_billing_token";

class BillingService {
  private os: OpenSecretContextType;

  constructor(os: OpenSecretContextType) {
    this.os = os;
  }

  async getStoredToken(): Promise<string | null> {
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

  async getProducts(): Promise<BillingProduct[]> {
    return fetchProducts();
  }

  async createCheckoutSession(
    email: string,
    productId: string,
    successUrl: string,
    cancelUrl: string
  ): Promise<void> {
    return this.executeWithToken((token) =>
      createCheckoutSession(token, email, productId, successUrl, cancelUrl)
    );
  }

  async createZapriteCheckoutSession(
    email: string,
    productId: string,
    successUrl: string
  ): Promise<void> {
    return this.executeWithToken((token) =>
      createZapriteCheckoutSession(token, email, productId, successUrl)
    );
  }

  async getTeamPlanAvailable(): Promise<boolean> {
    return this.executeWithToken((token) => fetchTeamPlanAvailable(token));
  }
  
  /**
   * Sync an Apple in-app purchase transaction with our backend
   */
  async syncAppleTransaction(
    transactionId: number,
    productId: string
  ): Promise<void> {
    return this.executeWithToken((token) =>
      syncAppleTransaction(token, transactionId, productId)
    );
  }
  
  /**
   * Check if Apple Pay should be shown as a payment option
   * On iOS, this is true if external billing is NOT allowed
   */
  async shouldShowApplePay(): Promise<boolean> {
    try {
      // Check if we're on iOS
      const { type } = await import("@tauri-apps/plugin-os");
      const platform = await type();
      
      if (platform !== "ios") {
        // Not on iOS, don't show Apple Pay
        return false;
      }
      
      // Check if external billing is allowed based on region
      const isExternalBillingAllowed = await allowExternalBilling();
      
      // We need to show Apple Pay if external billing is NOT allowed
      return !isExternalBillingAllowed;
    } catch (error) {
      console.error("Error checking if Apple Pay should be shown:", error);
      // Default to not showing Apple Pay on errors
      return false;
    }
  }
  
  /**
   * Check if Apple Pay should be the only payment option (non-US regions)
   * In non-US App Store regions, Apple requires using their IAP system
   */
  async isApplePayRequired(): Promise<boolean> {
    return this.shouldShowApplePay();
  }

  clearToken(): void {
    sessionStorage.removeItem(TOKEN_STORAGE_KEY);
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
