const COMMANDS: &[&str] = &[
  "get_region", 
  "get_products", 
  "purchase", 
  "verify_purchase", 
  "get_transactions", 
  "restore_purchases", 
  "get_subscription_status"
];

fn main() {
  tauri_plugin::Builder::new(COMMANDS)
    .ios_path("ios")
    .build();
}