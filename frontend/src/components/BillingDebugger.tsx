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
    used_tokens: currentStatus?.used_tokens ?? null,
    usage_reset_date: currentStatus?.usage_reset_date ?? null
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
          <label className="block text-sm">Messages Remaining</label>
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

        <div>
          <label className="block text-sm">Usage Reset Date (UTC)</label>
          <Input
            type="datetime-local"
            value={
              debugStatus.usage_reset_date
                ? new Date(debugStatus.usage_reset_date).toISOString().slice(0, 16)
                : ""
            }
            onChange={(e) =>
              setDebugStatus((prev) => ({
                ...prev,
                usage_reset_date: e.target.value
                  ? new Date(e.target.value + "Z").toISOString()
                  : null
              }))
            }
            placeholder="Enter UTC time"
            className="w-full bg-transparent border-yellow-500/30"
          />
          <span className="text-xs text-muted-foreground">
            Enter time in UTC (current UTC:{" "}
            {new Date().toISOString().slice(0, 19).replace("T", " ")})
          </span>
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
                used_tokens: 18500,
                usage_reset_date: null
              };
              setDebugStatus(newStatus);
              handleOverride(newStatus);
            }}
            className="border-yellow-500/30 hover:bg-yellow-500/20"
          >
            Test Pro + No Messages
          </Button>

          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              const inThreeHours = new Date();
              inThreeHours.setHours(inThreeHours.getHours() + 3);
              const newStatus: BillingStatus = {
                product_name: "Pro",
                chats_remaining: null,
                can_chat: true,
                is_subscribed: true,
                stripe_customer_id: "test_customer",
                product_id: "pro",
                subscription_status: "active",
                current_period_end: null,
                payment_provider: "stripe",
                total_tokens: 20000,
                used_tokens: 5000,
                usage_reset_date: inThreeHours.toISOString()
              };
              setDebugStatus(newStatus);
              handleOverride(newStatus);
            }}
            className="border-yellow-500/30 hover:bg-yellow-500/20"
          >
            Reset in 3 Hours
          </Button>

          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              const tomorrow = new Date();
              tomorrow.setDate(tomorrow.getDate() + 1);
              const newStatus: BillingStatus = {
                product_name: "Pro",
                chats_remaining: null,
                can_chat: true,
                is_subscribed: true,
                stripe_customer_id: "test_customer",
                product_id: "pro",
                subscription_status: "active",
                current_period_end: null,
                payment_provider: "stripe",
                total_tokens: 20000,
                used_tokens: 15000,
                usage_reset_date: tomorrow.toISOString()
              };
              setDebugStatus(newStatus);
              handleOverride(newStatus);
            }}
            className="border-yellow-500/30 hover:bg-yellow-500/20"
          >
            Reset Tomorrow
          </Button>

          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              const inFiveDays = new Date();
              inFiveDays.setDate(inFiveDays.getDate() + 5);
              const newStatus: BillingStatus = {
                product_name: "Max",
                chats_remaining: null,
                can_chat: true,
                is_subscribed: true,
                stripe_customer_id: "test_customer",
                product_id: "max",
                subscription_status: "active",
                current_period_end: null,
                payment_provider: "zaprite",
                total_tokens: 200000,
                used_tokens: 50000,
                usage_reset_date: inFiveDays.toISOString()
              };
              setDebugStatus(newStatus);
              handleOverride(newStatus);
            }}
            className="border-yellow-500/30 hover:bg-yellow-500/20"
          >
            Reset in 5 Days
          </Button>
        </div>
      </div>
    </Card>
  );
}
