import { useLocalState } from "@/state/useLocalState";
import { formatResetDate } from "@/utils/dateFormat";

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

  // Set bar color based on usage
  const getBarColor = () => {
    if (percentUsed >= 90) return "rgb(239, 68, 68)"; // Tailwind red-500
    if (percentUsed >= 75) return "rgb(245, 158, 11)"; // Tailwind amber-500
    return "rgb(16, 185, 129)"; // Tailwind emerald-500
  };

  return (
    <div className="px-2 py-2 text-xs text-muted-foreground">
      <div className="mb-1 flex justify-between">
        <span>Credit Usage</span>
        <span>{roundedPercent}%</span>
      </div>
      <div className="h-2.5 w-full overflow-hidden rounded-full bg-muted">
        <div
          className="h-full transition-all"
          style={{
            width: `${percentUsed}%`,
            backgroundColor: getBarColor()
          }}
        />
      </div>
      <div className="mt-1 text-xs text-right">
        {formatResetDate(billingStatus.usage_reset_date)}
      </div>
    </div>
  );
}
