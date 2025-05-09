use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_store);

// Initialize the plugin
pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<Store<R>> {
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_store)?;

    #[cfg(not(target_os = "ios"))]
    let handle = {
        // Dummy handle for non-iOS platforms
        let handle: PluginHandle<R> = unsafe { std::mem::zeroed() };
        handle
    };

    Ok(Store(handle))
}

/// Access to the store APIs.
pub struct Store<R: Runtime>(PluginHandle<R>);

// Transaction response type
#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub id: u64,
    #[serde(rename = "originalId")]
    pub original_id: Option<u64>,
    #[serde(rename = "productId")]
    pub product_id: String,
    #[serde(rename = "purchaseDate")]
    pub purchase_date: i64,
    #[serde(rename = "expirationDate")]
    pub expiration_date: Option<i64>,
    #[serde(rename = "webOrderLineItemId")]
    pub web_order_line_item_id: String,
    pub quantity: i32,
    pub r#type: String,
    #[serde(rename = "ownershipType")]
    pub ownership_type: String,
    #[serde(rename = "signedDate")]
    pub signed_date: i64,
}

// Product response type
#[derive(Debug, Serialize, Deserialize)]
pub struct Product {
    pub id: String,
    pub title: String,
    pub description: String,
    pub price: String,
    #[serde(rename = "priceValue")]
    pub price_value: f64,
    #[serde(rename = "currencyCode")]
    pub currency_code: String,
    pub r#type: String,
    #[serde(rename = "subscriptionPeriod", default, skip_serializing_if = "Option::is_none")]
    pub subscription_period: Option<SubscriptionPeriod>,
    #[serde(rename = "introductoryOffer", default, skip_serializing_if = "Option::is_none")]
    pub introductory_offer: Option<SubscriptionOffer>,
    #[serde(rename = "promotionalOffers", default, skip_serializing_if = "Option::is_none")]
    pub promotional_offers: Option<Vec<SubscriptionOffer>>,
}

// Subscription period type
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscriptionPeriod {
    pub unit: String,
    pub value: i32,
}

// Subscription offer type
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscriptionOffer {
    pub id: String,
    #[serde(rename = "displayPrice")]
    pub display_price: String,
    pub period: SubscriptionPeriod,
    #[serde(rename = "paymentMode")]
    pub payment_mode: String,
    pub r#type: String,
    #[serde(rename = "discountType", default, skip_serializing_if = "Option::is_none")]
    pub discount_type: Option<String>,
    #[serde(rename = "discountPrice", default, skip_serializing_if = "Option::is_none")]
    pub discount_price: Option<String>,
}

// Purchase result type
#[derive(Debug, Serialize, Deserialize)]
pub struct PurchaseResult {
    pub status: String,
    #[serde(rename = "transactionId", default, skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<u64>,
    #[serde(rename = "originalTransactionId", default, skip_serializing_if = "Option::is_none")]
    pub original_transaction_id: Option<u64>,
    #[serde(rename = "productId", default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<String>,
    #[serde(rename = "purchaseDate", default, skip_serializing_if = "Option::is_none")]
    pub purchase_date: Option<i64>,
    #[serde(rename = "expirationDate", default, skip_serializing_if = "Option::is_none")]
    pub expiration_date: Option<i64>,
    #[serde(rename = "webOrderLineItemId", default, skip_serializing_if = "Option::is_none")]
    pub web_order_line_item_id: Option<String>,
    #[serde(rename = "quantity", default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<i32>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(rename = "ownershipType", default, skip_serializing_if = "Option::is_none")]
    pub ownership_type: Option<String>,
    #[serde(rename = "signedDate", default, skip_serializing_if = "Option::is_none")]
    pub signed_date: Option<i64>,
    #[serde(rename = "environment", default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// Verification result type
#[derive(Debug, Serialize, Deserialize)]
pub struct VerificationResult {
    #[serde(rename = "isValid")]
    pub is_valid: bool,
    #[serde(rename = "expirationDate", default, skip_serializing_if = "Option::is_none")]
    pub expiration_date: Option<i64>,
    #[serde(rename = "purchaseDate", default, skip_serializing_if = "Option::is_none")]
    pub purchase_date: Option<i64>,
}

// Restore purchases result type
#[derive(Debug, Serialize, Deserialize)]
pub struct RestorePurchasesResult {
    pub status: String,
    pub transactions: Vec<Transaction>,
}

// Subscription status type
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscriptionStatus {
    #[serde(rename = "productId")]
    pub product_id: String,
    pub status: String,
    #[serde(rename = "willAutoRenew")]
    pub will_auto_renew: bool,
    #[serde(rename = "expirationDate", default, skip_serializing_if = "Option::is_none")]
    pub expiration_date: Option<i64>,
    #[serde(rename = "gracePeriodExpirationDate", default, skip_serializing_if = "Option::is_none")]
    pub grace_period_expiration_date: Option<i64>,
}

impl<R: Runtime> Store<R> {
    // Get region code from the App Store
    pub fn get_region(&self) -> crate::Result<String> {
        self.0
            .run_mobile_plugin("getRegion", ())
            .map_err(Into::into)
    }

    // Get products from the App Store
    pub fn get_products(&self, product_ids: Vec<String>) -> crate::Result<Vec<Product>> {
        self.0
            .run_mobile_plugin("getProducts", serde_json::json!({ "productIds": product_ids }))
            .map_err(Into::into)
    }

    // Make a purchase
    pub fn purchase(&self, product_id: String) -> crate::Result<PurchaseResult> {
        self.0
            .run_mobile_plugin("purchase", serde_json::json!({ "productId": product_id }))
            .map_err(Into::into)
    }

    // Verify a purchase
    pub fn verify_purchase(&self, product_id: String, transaction_id: u64) -> crate::Result<VerificationResult> {
        self.0
            .run_mobile_plugin(
                "verifyPurchase",
                serde_json::json!({
                    "productId": product_id,
                    "transactionId": transaction_id
                }),
            )
            .map_err(Into::into)
    }

    // Get all transactions, optionally filtered by product ID
    pub fn get_transactions(&self, product_id: Option<String>) -> crate::Result<Vec<Transaction>> {
        let args = match product_id {
            Some(id) => serde_json::json!({ "productId": id }),
            None => serde_json::json!({}),
        };

        self.0
            .run_mobile_plugin("getTransactions", args)
            .map_err(Into::into)
    }

    // Restore purchases
    pub fn restore_purchases(&self) -> crate::Result<RestorePurchasesResult> {
        self.0
            .run_mobile_plugin("restorePurchases", ())
            .map_err(Into::into)
    }

    // Get subscription status
    pub fn get_subscription_status(&self, product_id: String) -> crate::Result<SubscriptionStatus> {
        self.0
            .run_mobile_plugin(
                "getSubscriptionStatus",
                serde_json::json!({ "productId": product_id }),
            )
            .map_err(Into::into)
    }
}