import { useId } from "react";

import { useLocalState } from "@/state/useLocalState";
import { formatResetDate } from "@/utils/dateFormat";
import { cn } from "@/utils/utils";

type CreditUsageLayout = "bar" | "ring";

/** Dev-only opt-in: localStorage `maple_mock_credit_scenario` = demo | full | high | warn | ok | off */
type MockScenario = "demo" | "full" | "high" | "warn" | "ok";

function readMockScenario(): MockScenario | "off" | null {
  if (!import.meta.env.DEV || typeof window === "undefined") return null;
  let raw: string | null = null;
  try {
    raw = localStorage.getItem("maple_mock_credit_scenario");
  } catch {
    return null;
  }
  if (raw === "off") return "off";
  if (raw === "demo" || raw === "full" || raw === "high" || raw === "warn" || raw === "ok")
    return raw;
  return null;
}

function mockPreset(scenario: MockScenario): {
  total_tokens: number;
  used_tokens: number;
  api_credit_balance?: number;
} {
  const t = 10_000;
  switch (scenario) {
    case "demo":
      return { total_tokens: t, used_tokens: 6900 };
    case "full":
      return { total_tokens: t, used_tokens: t, api_credit_balance: 2_500 };
    case "high":
      return { total_tokens: t, used_tokens: 9_650 };
    case "warn":
      return { total_tokens: t, used_tokens: 7_800 };
    case "ok":
      return { total_tokens: t, used_tokens: 3_200, api_credit_balance: 12_000 };
    default:
      return { total_tokens: t, used_tokens: 6900 };
  }
}

/** ~14 days out so formatResetDate shows a friendly string */
function mockUsageResetIso(): string {
  const d = new Date();
  d.setDate(d.getDate() + 14);
  return d.toISOString();
}

function usageTone(percentUsed: number): "danger" | "warn" | "ok" {
  if (percentUsed >= 90) return "danger";
  if (percentUsed >= 75) return "warn";
  return "ok";
}

function toneColor(tone: ReturnType<typeof usageTone>): string {
  switch (tone) {
    case "danger":
      return "hsl(var(--maple-error))";
    case "warn":
      return "hsl(var(--maple-warning))";
    default:
      return "hsl(var(--maple-success))";
  }
}

function toneTextClass(tone: ReturnType<typeof usageTone>): string {
  switch (tone) {
    case "danger":
      return "text-maple-error";
    case "warn":
      return "text-maple-warning";
    default:
      return "text-maple-success";
  }
}

type RingMeterProps = {
  percent: number;
  size?: number;
  stroke?: number;
};

function RingMeter({ percent, size = 32, stroke = 3.5 }: RingMeterProps) {
  const gradId = `credit-ring-grad-${useId().replace(/:/g, "")}`;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const clamped = Math.min(100, Math.max(0, percent));
  const offset = c - (clamped / 100) * c;

  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      className="absolute left-0 top-0 -rotate-90"
      aria-hidden
    >
      <defs>
        <linearGradient
          id={gradId}
          gradientUnits="userSpaceOnUse"
          x1={size / 2}
          y1={0}
          x2={size / 2}
          y2={size}
        >
          <stop offset="0%" stopColor="hsl(var(--maple-primary))" />
          <stop offset="100%" stopColor="hsl(var(--maple-primary-strong))" />
        </linearGradient>
      </defs>
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        strokeWidth={stroke}
        className="fill-none stroke-[hsl(var(--sidebar-chrome))]"
      />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke={`url(#${gradId})`}
        strokeWidth={stroke}
        strokeLinecap="round"
        strokeDasharray={c}
        strokeDashoffset={offset}
        className="transition-[stroke-dashoffset] duration-500 ease-out"
      />
    </svg>
  );
}

function UsageRing({
  percentUsed,
  roundedPercent,
  size = 32,
  stroke = 3.5
}: {
  percentUsed: number;
  roundedPercent: number;
  size?: number;
  stroke?: number;
}) {
  return (
    <div
      className="relative shrink-0"
      style={{ width: size, height: size }}
      aria-label={`${roundedPercent} percent of plan credits used`}
    >
      <RingMeter percent={percentUsed} size={size} stroke={stroke} />
    </div>
  );
}

export function CreditUsage({ layout = "bar" }: { layout?: CreditUsageLayout }) {
  const { billingStatus } = useLocalState();

  const totalLive = billingStatus?.total_tokens;
  const usedLive = billingStatus?.used_tokens;
  const hasRealUsage = totalLive != null && totalLive > 0 && usedLive != null && usedLive > 0;

  const mockFlag = readMockScenario();
  const useMock = !hasRealUsage && import.meta.env.DEV && mockFlag !== null && mockFlag !== "off";

  if (!hasRealUsage && !useMock) {
    return null;
  }

  const mock = useMock ? mockPreset(mockFlag as MockScenario) : null;
  const total = hasRealUsage ? totalLive! : mock!.total_tokens;
  const used = hasRealUsage ? usedLive! : mock!.used_tokens;
  const productName = hasRealUsage ? billingStatus?.product_name : "Pro";
  const usageResetDate = hasRealUsage ? billingStatus!.usage_reset_date : mockUsageResetIso();
  const apiBalance = hasRealUsage ? billingStatus!.api_credit_balance : mock?.api_credit_balance;

  const percentUsed = Math.min(100, Math.max(0, (used / total) * 100));
  const roundedPercent = Math.round(percentUsed);
  const tone = usageTone(percentUsed);
  const barColor = toneColor(tone);

  const isMaxPlan = productName?.toLowerCase().includes("max") ?? false;
  if (isMaxPlan && percentUsed < 90) {
    return null;
  }

  const hasApiCredits = apiBalance !== undefined && apiBalance > 0;

  const formatCredits = (credits: number) => {
    return new Intl.NumberFormat("en-US").format(credits);
  };

  const statusLabel =
    percentUsed >= 100 ? "Limit reached" : percentUsed >= 90 ? "Almost full" : "Plan credits";

  if (layout === "ring") {
    return (
      <div className="w-full rounded-xl border border-[hsl(var(--sidebar-chrome))] bg-transparent p-3">
        <div className="flex items-center gap-3">
          <div className="min-w-0 flex-1">
            <div className="flex items-baseline gap-2">
              <span className="truncate text-[11px] font-medium text-foreground">
                {statusLabel}
              </span>
            </div>
            <p className="mt-0.5 truncate text-[10px] text-muted-foreground">
              {hasApiCredits && (
                <>
                  +{formatCredits(apiBalance ?? 0)} extra
                  <span className="text-muted-foreground/50"> · </span>
                </>
              )}
              {formatResetDate(usageResetDate)}
            </p>
          </div>
          <UsageRing percentUsed={percentUsed} roundedPercent={roundedPercent} />
        </div>
      </div>
    );
  }

  return (
    <div className="px-2 py-2 text-xs text-muted-foreground">
      <div className="mb-1 flex justify-between">
        <span>{percentUsed >= 100 ? "Plan credits (full)" : "Plan Credits"}</span>
        <span className={cn("tabular-nums", toneTextClass(tone))}>{roundedPercent}%</span>
      </div>
      <div className="h-2.5 w-full overflow-hidden rounded-full bg-muted">
        <div
          className="h-full transition-all duration-500 ease-out"
          style={{
            width: `${percentUsed}%`,
            backgroundColor: barColor
          }}
        />
      </div>
      <div className="mt-1 flex justify-between text-xs">
        {hasApiCredits && <span>+ {formatCredits(apiBalance ?? 0)} extra credits</span>}
        <span className={hasApiCredits ? "" : "ml-auto"}>{formatResetDate(usageResetDate)}</span>
      </div>
    </div>
  );
}
