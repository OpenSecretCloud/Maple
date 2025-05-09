import { invoke } from "@tauri-apps/api/core";
import { allowExternalBilling } from "../utils/region-gate";

// Types for StoreKit
export type Product = {
  id: string;
  title: string;
  description: string;
  price: string;
  priceValue: number;
  currencyCode: string;
  type: "consumable" | "non_consumable" | "auto_renewable_subscription" | "non_renewable_subscription";
  subscriptionPeriod?: {
    unit: "day" | "week" | "month" | "year";
    value: number;
  };
  introductoryOffer?: SubscriptionOffer;
  promotionalOffers?: SubscriptionOffer[];
};

export type SubscriptionOffer = {
  id: string;
  displayPrice: string;
  period: {
    unit: "day" | "week" | "month" | "year";
    value: number;
  };
  paymentMode: "pay_as_you_go" | "pay_up_front" | "free_trial";
  type: "introductory" | "promotional" | "prepaid" | "consumable";
  discountType?: "percentage" | "nominal";
  discountPrice?: string;
};

export type Transaction = {
  id: number;
  originalId?: number;
  productId: string;
  purchaseDate: number;
  expirationDate?: number;
  webOrderLineItemId: string;
  quantity: number;
  type: "consumable" | "non_consumable" | "auto_renewable_subscription" | "non_renewable_subscription";
  ownershipType: "purchased" | "familyShared";
  signedDate: number;
};

export type PurchaseResult = {
  status: "success" | "pending";
  transactionId?: number;
  originalTransactionId?: number;
  productId?: string;
  purchaseDate?: number;
  expirationDate?: number;
  webOrderLineItemId?: string;
  quantity?: number;
  type?: string;
  ownershipType?: string;
  signedDate?: number;
  environment?: "sandbox" | "production";
  message?: string;
};

export type VerificationResult = {
  isValid: boolean;
  expirationDate?: number;
  purchaseDate?: number;
};

export type RestorePurchasesResult = {
  status: string;
  transactions: Transaction[];
};

export type SubscriptionStatus = {
  productId: string;
  status: "subscribed" | "expired" | "in_billing_retry_period" | "in_grace_period" | "revoked" | "not_subscribed";
  willAutoRenew: boolean;
  expirationDate?: number;
  gracePeriodExpirationDate?: number;
};

/**
 * Maps between Stripe/server product IDs and Apple product IDs
 * In a real app, this would be configured on the server and fetched during initialization
 */
const PRODUCT_ID_MAP: Record<string, string> = {
  // Example: Stripe product ID -> Apple product ID (monthly plans only for Apple Pay)
  "price_pro_monthly": "com.opensecret.maple.pro.monthly", 
  "price_starter_monthly": "com.opensecret.maple.starter.monthly"
};

class ApplePayService {
  private static instance: ApplePayService;
  private cachedProducts: Map<string, Product> = new Map();
  
  // A mapping between system product IDs (e.g., Stripe) and Apple Store product IDs
  private productIdMap: Record<string, string> = PRODUCT_ID_MAP;

  private constructor() {}

  public static getInstance(): ApplePayService {
    if (!ApplePayService.instance) {
      ApplePayService.instance = new ApplePayService();
    }
    return ApplePayService.instance;
  }

  /**
   * Set the product ID mapping
   */
  public setProductIdMap(map: Record<string, string>): void {
    this.productIdMap = { ...PRODUCT_ID_MAP, ...map };
  }

  /**
   * Convert a system product ID to an Apple product ID
   */
  public getAppleProductId(systemProductId: string): string {
    return this.productIdMap[systemProductId] || systemProductId;
  }

  /**
   * Check if Apple Pay is available and permitted based on region
   */
  public async isApplePayAvailable(): Promise<boolean> {
    try {
      // First check if we're on iOS
      const { type } = await import("@tauri-apps/plugin-os");
      const platform = await type();
      
      if (platform !== "ios") {
        console.log("[ApplePay] Not on iOS platform");
        return false;
      }
      
      // For regions where external billing is not allowed, Apple Pay is required
      // For US regions, we can show both options
      return true;
    } catch (error) {
      console.error("[ApplePay] Error checking availability:", error);
      return false;
    }
  }

  /**
   * Check if Apple Pay should be the required payment method (non-US)
   */
  public async isApplePayRequired(): Promise<boolean> {
    try {
      // If not on iOS, it's never required
      const { type } = await import("@tauri-apps/plugin-os");
      const platform = await type();
      
      if (platform !== "ios") {
        return false;
      }
      
      // For non-US regions (where external billing is not allowed), Apple Pay is required
      const isUSRegion = await allowExternalBilling();
      return !isUSRegion;
    } catch (error) {
      console.error("[ApplePay] Error checking if required:", error);
      return false;
    }
  }

  /**
   * Get products from the App Store
   */
  public async getProducts(productIds: string[]): Promise<Product[]> {
    try {
      console.log("[ApplePay] Fetching products:", productIds);
      const products = await invoke<Product[]>("plugin:store|get_products", {
        productIds
      });
      
      // Cache products for later use
      products.forEach(product => {
        this.cachedProducts.set(product.id, product);
      });
      
      return products;
    } catch (error) {
      console.error("[ApplePay] Error fetching products:", error);
      throw error;
    }
  }

  /**
   * Purchase a product
   */
  public async purchase(productId: string): Promise<PurchaseResult> {
    try {
      console.log("[ApplePay] Purchasing product:", productId);
      return await invoke<PurchaseResult>("plugin:store|purchase", {
        productId
      });
    } catch (error) {
      console.error("[ApplePay] Purchase error:", error);
      throw error;
    }
  }

  /**
   * Verify a purchase
   */
  public async verifyPurchase(productId: string, transactionId: number): Promise<VerificationResult> {
    try {
      console.log("[ApplePay] Verifying purchase:", productId, transactionId);
      return await invoke<VerificationResult>("plugin:store|verify_purchase", {
        productId,
        transactionId
      });
    } catch (error) {
      console.error("[ApplePay] Verification error:", error);
      throw error;
    }
  }

  /**
   * Get all transactions, optionally filtered by product ID
   */
  public async getTransactions(productId?: string): Promise<Transaction[]> {
    try {
      console.log("[ApplePay] Getting transactions");
      return await invoke<Transaction[]>("plugin:store|get_transactions", {
        productId
      });
    } catch (error) {
      console.error("[ApplePay] Error getting transactions:", error);
      throw error;
    }
  }

  /**
   * Restore purchases
   */
  public async restorePurchases(): Promise<RestorePurchasesResult> {
    try {
      console.log("[ApplePay] Restoring purchases");
      return await invoke<RestorePurchasesResult>("plugin:store|restore_purchases");
    } catch (error) {
      console.error("[ApplePay] Restore error:", error);
      throw error;
    }
  }

  /**
   * Get subscription status
   */
  public async getSubscriptionStatus(productId: string): Promise<SubscriptionStatus> {
    try {
      console.log("[ApplePay] Getting subscription status:", productId);
      return await invoke<SubscriptionStatus>("plugin:store|get_subscription_status", {
        productId
      });
    } catch (error) {
      console.error("[ApplePay] Error getting subscription status:", error);
      throw error;
    }
  }

  /**
   * Format price to display currency with localization
   */
  public formatPrice(product: Product): string {
    return new Intl.NumberFormat(navigator.language, {
      style: "currency",
      currency: product.currencyCode
    }).format(product.priceValue);
  }

  /**
   * Get a product from cache by ID
   */
  public getCachedProduct(productId: string): Product | undefined {
    return this.cachedProducts.get(productId);
  }
}

// Create singleton instance
export const applePayService = ApplePayService.getInstance();