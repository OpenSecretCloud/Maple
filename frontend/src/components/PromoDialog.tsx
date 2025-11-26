import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tag, Sparkles, Check, Cpu, Image, FileText, Mic, Globe, Zap } from "lucide-react";
import { useNavigate } from "@tanstack/react-router";
import type { DiscountResponse } from "@/billing/billingApi";

interface PromoDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  discount: DiscountResponse & { active: true };
}

export function PromoDialog({ open, onOpenChange, discount }: PromoDialogProps) {
  const navigate = useNavigate();

  const handleUpgrade = () => {
    onOpenChange(false);
    navigate({ to: "/pricing" });
  };

  const benefits = [
    {
      icon: <Cpu className="h-4 w-4" />,
      text: "7 powerful AI models including DeepSeek R1"
    },
    {
      icon: <Image className="h-4 w-4" />,
      text: "Image upload and vision analysis"
    },
    {
      icon: <FileText className="h-4 w-4" />,
      text: "Document processing (PDF, TXT, MD)"
    },
    {
      icon: <Mic className="h-4 w-4" />,
      text: "Voice recording with Whisper transcription"
    },
    {
      icon: <Globe className="h-4 w-4" />,
      text: "Live web search powered by Brave"
    },
    {
      icon: <Zap className="h-4 w-4" />,
      text: "No daily limits - generous monthly usage"
    }
  ];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[calc(100vw-2rem)] sm:max-w-lg max-h-[calc(100vh-2rem)] overflow-y-auto">
        <DialogHeader>
          <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 mb-2">
            <div className="flex items-center gap-3">
              <div className="p-2 sm:p-2.5 rounded-xl bg-gradient-to-br from-pink-500/20 to-orange-500/20 shrink-0">
                <Tag className="h-6 w-6 sm:h-7 sm:w-7 text-pink-500" />
              </div>
              <div>
                <DialogTitle className="text-lg sm:text-xl">{discount.name}</DialogTitle>
                <DialogDescription className="text-sm mt-0.5">Limited time offer</DialogDescription>
              </div>
            </div>
            <Badge className="bg-gradient-to-r from-pink-500 to-orange-500 text-white text-base sm:text-lg px-3 py-1 sm:py-1.5 shadow-lg w-fit">
              {discount.percent_off}% OFF
            </Badge>
          </div>
        </DialogHeader>

        <div className="space-y-5 py-4">
          {/* Promo description */}
          <div className="p-4 rounded-xl bg-gradient-to-r from-pink-500/10 to-orange-500/10 border border-pink-500/20">
            <p className="text-base font-medium text-foreground">{discount.description}</p>
            {discount.duration_months && (
              <p className="text-sm text-muted-foreground mt-1">
                Discount applies for the first {discount.duration_months} month
                {discount.duration_months > 1 ? "s" : ""} of your subscription.
              </p>
            )}
          </div>

          {/* Benefits list */}
          <div className="space-y-3">
            <p className="text-sm font-medium text-muted-foreground">Upgrade to Pro and unlock:</p>
            <ul className="grid gap-2.5">
              {benefits.map((benefit, i) => (
                <li key={i} className="flex items-center gap-3 text-sm">
                  <div className="p-1.5 rounded-lg bg-green-500/10 text-green-500 shrink-0">
                    {benefit.icon}
                  </div>
                  <span>{benefit.text}</span>
                </li>
              ))}
            </ul>
          </div>

          {/* Privacy note */}
          <div className="pt-3 border-t">
            <div className="flex items-start gap-2 text-sm text-muted-foreground">
              <Check className="h-4 w-4 text-green-500 mt-0.5 shrink-0" />
              <span>All features include end-to-end encryption. Your data stays private.</span>
            </div>
          </div>
        </div>

        <DialogFooter className="flex-col-reverse sm:flex-row gap-2 sm:gap-2">
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            className="w-full sm:w-auto"
          >
            Maybe Later
          </Button>
          <Button
            onClick={handleUpgrade}
            className="w-full sm:w-auto gap-2 bg-gradient-to-r from-pink-500 to-orange-500 hover:from-pink-600 hover:to-orange-600 text-white border-0"
          >
            <Sparkles className="h-4 w-4" />
            Upgrade Now
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Get the localStorage key for tracking if a promo has been seen
 */
export function getPromoSeenKey(promoName: string): string {
  // Normalize the promo name to create a consistent key
  const normalizedName = promoName.toLowerCase().replace(/[^a-z0-9]/g, "_");
  return `promo_seen_${normalizedName}`;
}

/**
 * Check if a promo has been seen
 */
export function hasSeenPromo(promoName: string): boolean {
  try {
    return localStorage.getItem(getPromoSeenKey(promoName)) === "true";
  } catch {
    return false;
  }
}

/**
 * Mark a promo as seen
 */
export function markPromoAsSeen(promoName: string): void {
  try {
    localStorage.setItem(getPromoSeenKey(promoName), "true");
  } catch {
    // Silently fail if localStorage is unavailable
  }
}
