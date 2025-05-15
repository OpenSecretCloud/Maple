use tauri::{AppHandle, command, Runtime};

use crate::Result;
use crate::StoreExt;

#[command]
pub(crate) async fn get_region<R: Runtime>(
    app: AppHandle<R>,
) -> Result<String> {
    app.store().get_region()
}

#[command]
pub(crate) async fn get_products<R: Runtime>(
    app: AppHandle<R>,
    product_ids: Vec<String>,
) -> Result<Vec<crate::mobile::Product>> {
    app.store().get_products(product_ids)
}

#[command]
pub(crate) async fn purchase<R: Runtime>(
    app: AppHandle<R>,
    product_id: String,
) -> Result<crate::mobile::PurchaseResult> {
    app.store().purchase(product_id)
}

#[command]
pub(crate) async fn verify_purchase<R: Runtime>(
    app: AppHandle<R>,
    product_id: String,
    transaction_id: u64,
) -> Result<crate::mobile::VerificationResult> {
    app.store().verify_purchase(product_id, transaction_id)
}

#[command]
pub(crate) async fn get_transactions<R: Runtime>(
    app: AppHandle<R>,
    product_id: Option<String>,
) -> Result<Vec<crate::mobile::Transaction>> {
    app.store().get_transactions(product_id)
}

#[command]
pub(crate) async fn restore_purchases<R: Runtime>(
    app: AppHandle<R>,
) -> Result<crate::mobile::RestorePurchasesResult> {
    app.store().restore_purchases()
}

#[command]
pub(crate) async fn get_subscription_status<R: Runtime>(
    app: AppHandle<R>,
    product_id: String,
) -> Result<crate::mobile::SubscriptionStatus> {
    app.store().get_subscription_status(product_id)
}