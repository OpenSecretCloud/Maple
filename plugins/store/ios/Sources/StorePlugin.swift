//
//  StoreRegionPlugin.swift
//  store
//

import StoreKit
import Tauri

class StorePlugin: Plugin {
  @objc public func getRegion(_ invoke: Invoke) {
    // Using StoreKit 2 API for iOS 15+ (async)
    if #available(iOS 15.0, *) {
      // Create a Task to handle the async call
      Task {
        do {
          // Try to get the current storefront using StoreKit 2
          if let storefront = try? await StoreKit.Storefront.current {
            print("[StorePlugin] StoreKit 2 returned storefront: \(storefront.countryCode)")
            invoke.resolve(storefront.countryCode)
            return
          } else {
            print("[StorePlugin] StoreKit 2 storefront is nil")
          }
        } catch {
          print("[StorePlugin] Error getting StoreKit 2 storefront: \(error)")
        }
        
        // StoreKit 2 failed, fall back to StoreKit 1
        fallbackToStoreKit1(invoke)
      }
    } else {
      // For iOS 14 and below, use StoreKit 1 directly
      fallbackToStoreKit1(invoke)
    }
  }
  
  // Helper method for StoreKit 1 fallback
  private func fallbackToStoreKit1(_ invoke: Invoke) {
    if #available(iOS 13.0, *) {
      // Try StoreKit 1 API
      if let code = SKPaymentQueue.default().storefront?.countryCode {
        print("[StorePlugin] StoreKit 1 returned storefront: \(code)")
        invoke.resolve(code)
        return
      }
      print("[StorePlugin] StoreKit 1 storefront is nil")
    }
    
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
}

@_cdecl("init_plugin_store")
func initPlugin() -> Plugin {
  return StorePlugin()
}