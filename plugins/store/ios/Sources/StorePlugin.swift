//
//  StorePlugin.swift
//  store
//

import StoreKit
import Tauri

// Main StoreKit plugin class
@available(iOS 15.0, *)
class StorePlugin: Plugin {
  // MARK: - Properties
  
  // Dictionary to store pending transactions by identifier
  private var pendingTransactions: [UInt32: Transaction] = [:]
  
  // Store product identifiers
  private var availableProducts: [Product] = []
  
  // Transaction listener task
  private var transactionListenerTask: Task<Void, Error>? = nil
  
  // MARK: - Initialization
  
  override init() {
    super.init()
    // Start listening for transactions
    startTransactionListener()
  }
  
  deinit {
    // Cancel the transaction listener task when the plugin is destroyed
    transactionListenerTask?.cancel()
  }
  
  // MARK: - Transaction Listener
  
  private func startTransactionListener() {
    transactionListenerTask = Task {
      // Listen for transactions for the lifetime of the app
      for await verificationResult in Transaction.updates {
        // Handle transaction updates
        do {
          let transaction = try checkVerificationResult(verificationResult)
          
          // Process the transaction accordingly
          await handleTransactionUpdate(transaction)
        } catch {
          print("[StorePlugin] Error handling transaction update: \(error.localizedDescription)")
        }
      }
    }
  }
  
  private func checkVerificationResult<T>(_ result: VerificationResult<T>) throws -> T {
    switch result {
    case .unverified(let unverifiedItem, let error):
      print("[StorePlugin] Unverified transaction: \(error.localizedDescription)")
      // Still return the unverified item, but log the error
      return unverifiedItem
    case .verified(let verifiedItem):
      return verifiedItem
    }
  }
  
  private func handleTransactionUpdate(_ transaction: Transaction) async {
    // Notify your server about the transaction
    
    // For consumables, manage them appropriately
    
    // Finish the transaction to inform Apple the transaction is complete
    await transaction.finish()
    
    // Send a notification to the frontend if needed
    emit("transactionUpdated", transaction.jsonRepresentation)
  }
  
  // MARK: - Command: getRegion
  
  @objc public func getRegion(_ invoke: Invoke) {
    // Using StoreKit 2 API for iOS 15+
    Task {
      // Try to get the current storefront using StoreKit 2
      if let storefront = await StoreKit.Storefront.current {
        print("[StorePlugin] StoreKit 2 returned storefront: \(storefront.countryCode)")
        invoke.resolve(storefront.countryCode)
        return
      } else {
        print("[StorePlugin] StoreKit 2 storefront is nil")
        // StoreKit 2 failed, fall back to StoreKit 1
        fallbackToStoreKit1(invoke)
      }
    }
  }
  
  // Helper method for StoreKit 1 fallback
  private func fallbackToStoreKit1(_ invoke: Invoke) {
    // Try StoreKit 1 API
    if let code = SKPaymentQueue.default().storefront?.countryCode {
      print("[StorePlugin] StoreKit 1 returned storefront: \(code)")
      invoke.resolve(code)
      return
    }
    print("[StorePlugin] StoreKit 1 storefront is nil")
    
    // Final fallback - try to use device locale
    if let regionCode = Locale.current.regionCode {
      print("[StorePlugin] Using device locale region: \(regionCode)")
      invoke.resolve(regionCode)
      return
    }
    
    // Ultimate fallback
    print("[StorePlugin] All methods failed, returning UNKNOWN")
    invoke.resolve("UNKNOWN")
  }
  
  // MARK: - Command: getProducts
  
  @objc public func getProducts(_ invoke: Invoke) {
    let args = invoke.arguments
    guard let productIds = args["productIds"] as? [String] else {
      invoke.reject("Invalid arguments: expected an array of product IDs")
      return
    }
    
    Task {
      do {
        let storeProducts = try await Product.products(for: Set(productIds))
        availableProducts = storeProducts
        
        // Convert products to dictionaries for JSON serialization
        let productDicts = storeProducts.map { product -> [String: Any] in
          return product.jsonRepresentation
        }
        
        invoke.resolve(productDicts)
      } catch {
        print("[StorePlugin] Error fetching products: \(error.localizedDescription)")
        invoke.reject("Failed to fetch products: \(error.localizedDescription)")
      }
    }
  }
  
  // MARK: - Command: purchase
  
  @objc public func purchase(_ invoke: Invoke) {
    let args = invoke.arguments
    guard let productId = args["productId"] as? String else {
      invoke.reject("Invalid arguments: expected a product ID")
      return
    }
    
    // Find the product in the available products
    guard let product = availableProducts.first(where: { $0.id == productId }) else {
      invoke.reject("Product not found. Make sure to call getProducts first.")
      return
    }
    
    Task {
      do {
        // Create a purchase option
        let options: Set<Product.PurchaseOption> = []
        
        // Purchase the product
        let result = try await product.purchase(options: options)
        
        // Handle the purchase result
        switch result {
        case .success(let verificationResult):
          do {
            let transaction = try checkVerificationResult(verificationResult)
            
            // Save the transaction with the invoke ID for later resolution
            pendingTransactions[invoke.callbackId] = transaction
            
            // Resolve the purchase with the transaction details
            invoke.resolve([
              "status": "success",
              "transactionId": transaction.id,
              "originalTransactionId": transaction.originalID,
              "productId": transaction.productID,
              "purchaseDate": Int(transaction.purchaseDate.timeIntervalSince1970 * 1000),
              "expirationDate": transaction.expirationDate.map { Int($0.timeIntervalSince1970 * 1000) },
              "webOrderLineItemId": transaction.webOrderLineItemID ?? "",
              "quantity": transaction.purchasedQuantity,
              "type": transactionTypeToString(transaction),
              "ownershipType": transaction.ownershipType == .purchased ? "purchased" : "familyShared",
              "signedDate": Int(transaction.signedDate.timeIntervalSince1970 * 1000),
              "environment": environmentToString()
            ])
            
            // Process and finish the transaction
            await handleTransactionUpdate(transaction)
          } catch {
            invoke.reject("Transaction verification failed: \(error.localizedDescription)")
          }
        case .userCancelled:
          invoke.reject("Purchase was cancelled by the user")
        case .pending:
          // For ask to buy or other pending states
          invoke.resolve(["status": "pending", "message": "Purchase is pending approval"])
        @unknown default:
          invoke.reject("Unknown purchase result")
        }
      } catch {
        print("[StorePlugin] Purchase error: \(error.localizedDescription)")
        invoke.reject("Purchase failed: \(error.localizedDescription)")
      }
    }
  }
  
  // MARK: - Command: verifyPurchase
  
  @objc public func verifyPurchase(_ invoke: Invoke) {
    let args = invoke.arguments
    guard let productId = args["productId"] as? String,
          let transactionId = args["transactionId"] as? UInt64 else {
      invoke.reject("Invalid arguments: expected productId and transactionId")
      return
    }
    
    Task {
      do {
        // Get all transaction history for the specified product
        var verifiedTransaction: Transaction?
        
        for await result in Transaction.currentEntitlements {
          do {
            let transaction = try checkVerificationResult(result)
            
            if transaction.productID == productId && transaction.id == transactionId {
              verifiedTransaction = transaction
              break
            }
          } catch {
            continue // Skip unverified transactions
          }
        }
        
        if let transaction = verifiedTransaction {
          // Transaction is valid
          invoke.resolve([
            "isValid": true,
            "expirationDate": transaction.expirationDate.map { Int($0.timeIntervalSince1970 * 1000) },
            "purchaseDate": Int(transaction.purchaseDate.timeIntervalSince1970 * 1000)
          ])
        } else {
          // Transaction not found or not valid
          invoke.resolve(["isValid": false])
        }
      } catch {
        print("[StorePlugin] Verification error: \(error.localizedDescription)")
        invoke.reject("Verification failed: \(error.localizedDescription)")
      }
    }
  }
  
  // MARK: - Command: getTransactions
  
  @objc public func getTransactions(_ invoke: Invoke) {
    let args = invoke.arguments
    let productId = args["productId"] as? String // Optional filter by product ID
    
    Task {
      do {
        var transactions: [[String: Any]] = []
        
        // Get all current entitlements (active subscriptions and non-consumables)
        for await verificationResult in Transaction.currentEntitlements {
          do {
            let transaction = try checkVerificationResult(verificationResult)
            
            // Filter by product ID if specified
            if let productId = productId, transaction.productID != productId {
              continue
            }
            
            transactions.append(transaction.jsonRepresentation)
          } catch {
            continue // Skip unverified transactions
          }
        }
        
        invoke.resolve(transactions)
      } catch {
        print("[StorePlugin] Error fetching transactions: \(error.localizedDescription)")
        invoke.reject("Failed to fetch transactions: \(error.localizedDescription)")
      }
    }
  }
  
  // MARK: - Command: restorePurchases
  
  @objc public func restorePurchases(_ invoke: Invoke) {
    Task {
      do {
        // Request a refresh of App Store purchase history
        try await AppStore.sync()
        
        var restoredTransactions: [[String: Any]] = []
        
        // Get all current entitlements after sync
        for await verificationResult in Transaction.currentEntitlements {
          do {
            let transaction = try checkVerificationResult(verificationResult)
            restoredTransactions.append(transaction.jsonRepresentation)
          } catch {
            continue // Skip unverified transactions
          }
        }
        
        invoke.resolve(["status": "success", "transactions": restoredTransactions])
      } catch {
        print("[StorePlugin] Restore error: \(error.localizedDescription)")
        invoke.reject("Restore failed: \(error.localizedDescription)")
      }
    }
  }
  
  // MARK: - Command: getSubscriptionStatus
  
  @objc public func getSubscriptionStatus(_ invoke: Invoke) {
    let args = invoke.arguments
    guard let productId = args["productId"] as? String else {
      invoke.reject("Invalid arguments: expected a product ID")
      return
    }
    
    Task {
      do {
        var subscriptionGroupStatus: [String: Any] = [
          "productId": productId,
          "status": "not_subscribed",
          "willAutoRenew": false,
          "expirationDate": nil,
          "gracePeriodExpirationDate": nil
        ]
        
        // Look for subscription transactions for this product
        for await verificationResult in Transaction.currentEntitlements {
          do {
            let transaction = try checkVerificationResult(verificationResult)
            
            // Only check for subscriptions that match this product ID
            if transaction.productID == productId && transaction.productType == .autoRenewable {
              
              // Check the subscription info
              let status = try? await transaction.subscriptionStatus
              
              if let status = status {
                // Update subscription status
                subscriptionGroupStatus["status"] = subscriptionStateToString(status.state)
                subscriptionGroupStatus["willAutoRenew"] = status.willAutoRenew
                subscriptionGroupStatus["expirationDate"] = transaction.expirationDate.map { Int($0.timeIntervalSince1970 * 1000) }
                
                // Check for grace period
                if status.state == .inGracePeriod, let renewalInfo = status.renewalInfo {
                  subscriptionGroupStatus["gracePeriodExpirationDate"] = Int(renewalInfo.expirationDate.timeIntervalSince1970 * 1000)
                }
                
                // Return the first valid subscription we find
                break
              }
            }
          } catch {
            continue // Skip unverified transactions
          }
        }
        
        invoke.resolve(subscriptionGroupStatus)
      } catch {
        print("[StorePlugin] Error getting subscription status: \(error.localizedDescription)")
        invoke.reject("Failed to get subscription status: \(error.localizedDescription)")
      }
    }
  }
  
  // MARK: - Helper Functions
  
  private func transactionTypeToString(_ transaction: Transaction) -> String {
    switch transaction.productType {
    case .consumable:
      return "consumable"
    case .nonConsumable:
      return "non_consumable"
    case .nonRenewable:
      return "non_renewable_subscription"
    case .autoRenewable:
      return "auto_renewable_subscription"
    @unknown default:
      return "unknown"
    }
  }
  
  private func subscriptionStateToString(_ state: Product.SubscriptionInfo.RenewalState) -> String {
    switch state {
    case .subscribed:
      return "subscribed"
    case .expired:
      return "expired"
    case .inBillingRetryPeriod:
      return "in_billing_retry_period"
    case .inGracePeriod:
      return "in_grace_period"
    case .revoked:
      return "revoked"
    @unknown default:
      return "unknown"
    }
  }
  
  private func environmentToString() -> String {
    #if DEBUG
    return "sandbox"
    #else
    return AppStore.currentStorefront != nil ? "production" : "sandbox"
    #endif
  }
}

// MARK: - Extensions

@available(iOS 15.0, *)
extension Product {
  var jsonRepresentation: [String: Any] {
    var dict: [String: Any] = [
      "id": id,
      "title": displayName,
      "description": description,
      "price": displayPrice,
      "priceValue": price,
      "currencyCode": priceFormatStyle.currencyCode ?? "USD",
      "type": productTypeToString()
    ]
    
    // Add subscription-specific details if available
    if let subscription = subscription {
      dict["subscriptionPeriod"] = subscription.subscriptionPeriod.jsonRepresentation
      
      if let introductoryOffer = subscription.introductoryOffer {
        dict["introductoryOffer"] = introductoryOffer.jsonRepresentation
      }
      
      if let promotionalOffers = subscription.promotionalOffers, !promotionalOffers.isEmpty {
        dict["promotionalOffers"] = promotionalOffers.map { $0.jsonRepresentation }
      }
    }
    
    return dict
  }
  
  private func productTypeToString() -> String {
    switch type {
    case .consumable:
      return "consumable"
    case .nonConsumable:
      return "non_consumable"
    case .nonRenewable:
      return "non_renewable_subscription"
    case .autoRenewable:
      return "auto_renewable_subscription"
    @unknown default:
      return "unknown"
    }
  }
}

@available(iOS 15.0, *)
extension Product.SubscriptionPeriod {
  var jsonRepresentation: [String: Any] {
    return [
      "unit": unitToString(),
      "value": value
    ]
  }
  
  private func unitToString() -> String {
    switch unit {
    case .day:
      return "day"
    case .week:
      return "week"
    case .month:
      return "month"
    case .year:
      return "year"
    @unknown default:
      return "unknown"
    }
  }
}

@available(iOS 15.0, *)
extension Product.SubscriptionOffer {
  var jsonRepresentation: [String: Any] {
    var dict: [String: Any] = [
      "id": id,
      "displayPrice": displayPrice,
      "period": subscriptionPeriod.jsonRepresentation,
      "paymentMode": paymentModeToString(),
      "type": offerTypeToString()
    ]
    
    if let discount = discount {
      dict["discountType"] = discountTypeToString(discount)
      dict["discountPrice"] = discount.displayPrice
    }
    
    return dict
  }
  
  private func paymentModeToString() -> String {
    switch paymentMode {
    case .payAsYouGo:
      return "pay_as_you_go"
    case .payUpFront:
      return "pay_up_front"
    case .freeTrial:
      return "free_trial"
    @unknown default:
      return "unknown"
    }
  }
  
  private func offerTypeToString() -> String {
    switch type {
    case .introductory:
      return "introductory"
    case .promotional:
      return "promotional"
    case .prepaid:
      return "prepaid"
    case .consumable:
      return "consumable"
    @unknown default:
      return "unknown"
    }
  }
  
  private func discountTypeToString(_ discount: Product.SubscriptionOffer.Discount) -> String {
    if let numericValue = discount.numericDiscount, numericValue > 0 {
      // numericDiscount is a decimal percentage value (e.g., 0.5 for 50%)
      return "percentage"
    } else {
      return "nominal" // Fixed amount discount
    }
  }
}

@available(iOS 15.0, *)
extension Transaction {
  var jsonRepresentation: [String: Any] {
    return [
      "id": id,
      "originalId": originalID,
      "productId": productID,
      "purchaseDate": Int(purchaseDate.timeIntervalSince1970 * 1000),
      "expirationDate": expirationDate.map { Int($0.timeIntervalSince1970 * 1000) },
      "webOrderLineItemId": webOrderLineItemID ?? "",
      "quantity": purchasedQuantity,
      "type": productType.productTypeToString(),
      "ownershipType": ownershipType == .purchased ? "purchased" : "familyShared",
      "signedDate": Int(signedDate.timeIntervalSince1970 * 1000)
    ]
  }
}

@available(iOS 15.0, *)
extension Product.ProductType {
  func productTypeToString() -> String {
    switch self {
    case .consumable:
      return "consumable"
    case .nonConsumable:
      return "non_consumable"
    case .nonRenewable:
      return "non_renewable_subscription"
    case .autoRenewable:
      return "auto_renewable_subscription"
    @unknown default:
      return "unknown"
    }
  }
}

// MARK: - Init Plugin

@available(iOS 15.0, *)
@_cdecl("init_plugin_store")
func initPlugin() -> Plugin {
  return StorePlugin()
}