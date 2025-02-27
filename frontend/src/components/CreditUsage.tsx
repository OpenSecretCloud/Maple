import { useLocalState } from "@/state/useLocalState";

export function CreditUsage() {
  const { billingStatus } = useLocalState();

  // Only show credit usage for paid plans
  if (!billingStatus?.total_tokens || !billingStatus?.used_tokens) {
    return null;
  }

  // Calculate percentage, ensuring it's between 0 and 100
  const percentUsed = Math.min(
    100,
    Math.max(0, (billingStatus.used_tokens / billingStatus.total_tokens) * 100)
  );
  const roundedPercent = Math.round(percentUsed);

  return (
    <div className="px-2 py-2 text-xs text-muted-foreground">
      <div className="mb-1 flex justify-between">
        <span>Credit Usage</span>
        <span>{roundedPercent}%</span>
      </div>
      <div className="h-2.5 w-full overflow-hidden rounded-full bg-secondary">
        <div
          className={`h-full transition-all ${
            percentUsed >= 90 ? "bg-destructive" : percentUsed >= 75 ? "bg-amber-500" : "bg-primary"
          }`}
          style={{ width: `${percentUsed}%` }}
        />
      </div>
      <div className="mt-1 text-xs text-right">Resets Monthly</div>
    </div>
  );
}
