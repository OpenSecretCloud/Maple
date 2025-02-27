import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card } from "@/components/ui/card";
import { useLocalState } from "@/state/useLocalState";
import { BillingStatus } from "@/billing/billingApi";

interface BillingDebuggerProps {
  currentStatus: BillingStatus | null;
  onOverride: (status: BillingStatus) => void;
}

export function BillingDebugger({ currentStatus, onOverride }: BillingDebuggerProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [debugStatus, setDebugStatus] = useState<BillingStatus>({
    product_name: currentStatus?.product_name || "Free",
    chats_remaining: currentStatus?.chats_remaining ?? 10,
    can_chat: currentStatus?.can_chat ?? true,
    is_subscribed: currentStatus?.is_subscribed ?? false,
    stripe_customer_id: currentStatus?.stripe_customer_id ?? null,
    product_id: currentStatus?.product_id ?? "free",
    subscription_status: currentStatus?.subscription_status ?? "active",
    current_period_end: currentStatus?.current_period_end ?? null,
    payment_provider: currentStatus?.payment_provider ?? null,
    total_tokens: currentStatus?.total_tokens ?? null,
    used_tokens: currentStatus?.used_tokens ?? null
  });
  const { setBillingStatus } = useLocalState();

  const handleOverride = (newStatus: BillingStatus) => {
    onOverride(newStatus);
    setBillingStatus(newStatus);
  };

  if (!isOpen) {
    return (
      <Button
        variant="outline"
        size="sm"
        onClick={() => setIsOpen(true)}
        className="fixed bottom-4 right-4 z-50 bg-yellow-500/50 border-yellow-500/30 text-yellow-500"
      >
        Debug Billing
      </Button>
    );
  }

  return (
    <Card className="fixed bottom-4 right-4 z-50 p-4 bg-yellow-500/50 border-yellow-500/30 text-black space-y-4">
      <div className="flex justify-between items-center">
        <h3 className="font-bold">Billing Debugger</h3>
        <Button variant="ghost" size="sm" onClick={() => setIsOpen(false)}>
          Close
        </Button>
      </div>

      <div className="space-y-2">
        <div>
          <label className="block text-sm">Product Name</label>
          <Input
            value={debugStatus.product_name}
            onChange={(e) => setDebugStatus((prev) => ({ ...prev, product_name: e.target.value }))}
            className="w-full bg-transparent border-yellow-500/30"
          />
        </div>

        <div>
          <label className="block text-sm">Chats Remaining</label>
          <Input
            type="number"
            value={debugStatus.chats_remaining ?? ""}
            onChange={(e) =>
              setDebugStatus((prev) => ({
                ...prev,
                chats_remaining: e.target.value ? parseInt(e.target.value) : null
              }))
            }
            className="w-full bg-transparent border-yellow-500/30"
          />
        </div>

        <div>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={debugStatus.can_chat}
              onChange={(e) =>
                setDebugStatus((prev) => ({
                  ...prev,
                  can_chat: e.target.checked
                }))
              }
              className="rounded border-yellow-500/30"
            />
            Can Chat
          </label>
        </div>

        <div>
          <label className="block text-sm">Total Credits</label>
          <Input
            type="number"
            value={debugStatus.total_tokens ?? ""}
            onChange={(e) =>
              setDebugStatus((prev) => ({
                ...prev,
                total_tokens: e.target.value ? parseInt(e.target.value) : null
              }))
            }
            className="w-full bg-transparent border-yellow-500/30"
          />
        </div>

        <div>
          <label className="block text-sm">Used Credits</label>
          <Input
            type="number"
            value={debugStatus.used_tokens ?? ""}
            onChange={(e) =>
              setDebugStatus((prev) => ({
                ...prev,
                used_tokens: e.target.value ? parseInt(e.target.value) : null
              }))
            }
            className="w-full bg-transparent border-yellow-500/30"
          />
        </div>

        <div className="space-x-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => handleOverride(debugStatus)}
            className="border-yellow-500/30 hover:bg-yellow-500/20"
          >
            Apply Override
          </Button>

          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              const newStatus: BillingStatus = {
                product_name: "Pro",
                chats_remaining: null,
                can_chat: false,
                is_subscribed: true,
                stripe_customer_id: "test_customer",
                product_id: "pro",
                subscription_status: "active",
                current_period_end: null,
                payment_provider: "stripe",
                total_tokens: 20000,
                used_tokens: 18500
              };
              setDebugStatus(newStatus);
              handleOverride(newStatus);
            }}
            className="border-yellow-500/30 hover:bg-yellow-500/20"
          >
            Test Pro + No Chats
          </Button>
        </div>
      </div>
    </Card>
  );
}
