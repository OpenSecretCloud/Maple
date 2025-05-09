import React, { useState, useEffect } from "react";
import { cn } from "@/utils/utils";
import { applePayService } from "../billing/applePayService";
import { getBillingService } from "../billing/billingService";
import { useQueryClient } from "@tanstack/react-query";

type ApplePayButtonProps = {
  productId: string;
  className?: string;
  text?: string;
  disabled?: boolean;
  onSuccess?: (transactionId: number) => void;
  onError?: (error: Error) => void;
  onCancel?: () => void;
};

export function ApplePayButton({
  productId,
  className,
  text = "Pay with Apple",
  disabled = false,
  onSuccess,
  onError,
  onCancel
}: ApplePayButtonProps) {
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [buttonLabel, setButtonLabel] = useState<string>(text);
  const queryClient = useQueryClient();
  
  // Load product info when available
  useEffect(() => {
    async function loadProductInfo() {
      try {
        const products = await applePayService.getProducts([productId]);
        if (products && products.length > 0) {
          const product = products[0];
          setButtonLabel(`${text} ${product.price}`);
        }
      } catch (error) {
        console.error("Error loading product info:", error);
      }
    }
    
    loadProductInfo();
  }, [productId, text]);
  
  // Handle purchase
  const handlePurchase = async () => {
    if (disabled || isLoading) return;
    
    setIsLoading(true);
    try {
      // Make the purchase
      const result = await applePayService.purchase(productId);
      
      if (result.status === "success" && result.transactionId) {
        console.log("Purchase successful:", result);
        
        // Sync with backend
        try {
          // Update user's subscription with your backend
          const billingService = getBillingService();
          await billingService.syncAppleTransaction(result.transactionId, productId);
          
          // Refresh billing status data
          await queryClient.invalidateQueries({ queryKey: ["billingStatus"] });
          
          // Call success callback
          onSuccess?.(result.transactionId);
        } catch (syncError) {
          console.error("Error syncing purchase with backend:", syncError);
          // Even if sync fails, the purchase was successful
          onSuccess?.(result.transactionId);
        }
      } else if (result.status === "pending") {
        console.log("Purchase pending:", result);
        // Handle pending purchase (like "Ask to Buy" for children)
        setButtonLabel("Purchase pending approval");
      } else {
        // This shouldn't happen, but handle it anyway
        throw new Error("Purchase failed with unknown status");
      }
    } catch (error) {
      console.error("Purchase error:", error);
      
      // If user canceled, call the onCancel callback
      if (error instanceof Error && error.message.includes("cancelled")) {
        onCancel?.();
      } else {
        // Otherwise call the onError callback
        onError?.(error instanceof Error ? error : new Error(String(error)));
      }
    } finally {
      setIsLoading(false);
    }
  };
  
  return (
    <button
      className={cn(
        "bg-black text-white hover:bg-gray-800 px-6 py-3 rounded-lg flex items-center justify-center gap-2 transition-all",
        isLoading && "opacity-70 cursor-not-allowed",
        disabled && "opacity-50 cursor-not-allowed",
        className
      )}
      onClick={handlePurchase}
      disabled={disabled || isLoading}
    >
      {/* Apple logo */}
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M9 7c-3 0-4 3-4 5.5 0 3 2 7.5 4 7.5 1.088-.046 1.679-.5 3-.5 1.312 0 1.5.5 3 .5s4-3 4-5c-.028-.01-2.472-.403-2.5-3 0-2.355 2.064-3.684 2.5-4-1.246-1.698-3.117-1.725-3.5-2-2 0-3 1-3.5 1s-1.5-1-3-1z" />
        <path d="M9 7c-.195-2.275 1.786-3 2.5-3 .5 0 1.956.15 2.5 1" />
      </svg>
      {buttonLabel}
      {isLoading && (
        <div className="animate-spin h-4 w-4 border-2 border-white border-t-transparent rounded-full ml-2" />
      )}
    </button>
  );
}