import { useLocalState } from "@/state/useLocalState";
import { formatResetDate } from "@/utils/dateFormat";

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

function toPlanNameLabel(rawPlanName: string | undefined): string {
  const cleaned = (rawPlanName ?? "Pro").trim();
  const hasPlanSuffix = /\bplan\b/i.test(cleaned);
  return hasPlanSuffix ? cleaned : `${cleaned} Plan`;
}

type CreditUsageViewProps = {
  planLabel: string;
  percentRemaining: number;
  roundedRemaining: number;
  total: number;
  tokensRemaining: number;
  apiBalance?: number;
  hasApiCredits: boolean;
  resetFullLabel: string;
  formatCredits: (n: number) => string;
};

function CreditUsageView(p: CreditUsageViewProps) {
  return (
    <div
      className="w-full rounded-xl border border-[hsl(var(--sidebar-chrome))] bg-[hsl(var(--sidebar-chrome))]/30 p-2"
      title={p.resetFullLabel || undefined}
    >
      <div className="flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[10px] leading-tight">
        <span className="inline-flex shrink-0 items-center rounded-full border border-border/50 bg-muted px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-foreground">
          {p.planLabel}
        </span>
        <span className="text-muted-foreground/50">·</span>
        <span className="shrink-0 font-semibold tabular-nums text-foreground">
          {p.roundedRemaining}% left
        </span>
        {p.resetFullLabel && (
          <>
            <span className="text-muted-foreground/50">·</span>
            <span className="min-w-0 flex-1 truncate text-muted-foreground">
              {p.resetFullLabel}
            </span>
          </>
        )}
      </div>
      {/* Bar + token row: hover or focus the bar area to reveal exact token amounts */}
      <div
        className="group/creditbar mt-1.5 min-h-0 cursor-default rounded-sm py-1.5 outline-none transition-shadow focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
        tabIndex={0}
        role="group"
        aria-label={`${p.roundedRemaining} percent of plan tokens remaining. Hover the usage bar or focus this control to read exact token counts.`}
      >
        <div className="h-[4px] w-full overflow-hidden rounded-full bg-[hsl(var(--sidebar-chrome))]">
          <div
            className="h-full rounded-full transition-[width] duration-500 ease-out"
            style={{
              width: `${p.percentRemaining}%`,
              background:
                "linear-gradient(90deg, hsl(var(--maple-primary)), hsl(var(--maple-primary-strong)))"
            }}
          />
        </div>
        <div className="grid grid-rows-[0fr] transition-[grid-template-rows] duration-200 ease-out group-hover/creditbar:grid-rows-[1fr] group-focus-within/creditbar:grid-rows-[1fr] [@media(hover:none)]:grid-rows-[1fr]">
          <div className="min-h-0 overflow-hidden">
            <div className="pt-1.5 text-[9.5px] leading-none text-muted-foreground">
              <span className="min-w-0 truncate tabular-nums">
                {p.formatCredits(p.tokensRemaining)} / {p.formatCredits(p.total)} tokens
                {p.hasApiCredits && (
                  <span className="ml-1 text-[hsl(var(--maple-success))]">
                    +{p.formatCredits(p.apiBalance ?? 0)}
                  </span>
                )}
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

export function CreditUsage({ mockScenario }: { mockScenario?: MockScenario }) {
  const { billingStatus } = useLocalState();

  const totalLive = billingStatus?.total_tokens;
  const usedLive = billingStatus?.used_tokens;
  const hasRealUsage = totalLive != null && totalLive > 0 && usedLive != null && usedLive > 0;

  const mockFlag = readMockScenario();
  const forcedMock = import.meta.env.DEV ? mockScenario : undefined;
  const scenario = forcedMock ?? (mockFlag !== null && mockFlag !== "off" ? mockFlag : null);
  const useMock = !hasRealUsage && import.meta.env.DEV && scenario !== null;

  if (!hasRealUsage && !useMock) {
    return null;
  }

  const mock = useMock ? mockPreset(scenario as MockScenario) : null;
  const total = hasRealUsage ? totalLive! : mock!.total_tokens;
  const used = hasRealUsage ? usedLive! : mock!.used_tokens;
  const productName = hasRealUsage ? billingStatus?.product_name : "Pro";
  const usageResetDate = hasRealUsage ? billingStatus!.usage_reset_date : mockUsageResetIso();
  const apiBalance = hasRealUsage ? billingStatus!.api_credit_balance : mock?.api_credit_balance;

  const percentUsed = Math.min(100, Math.max(0, (used / total) * 100));
  const percentRemaining = Math.max(0, 100 - percentUsed);
  const roundedRemaining = Math.round(percentRemaining);
  const tokensRemaining = Math.max(0, total - used);

  const isMaxPlan = productName?.toLowerCase().includes("max") ?? false;
  if (isMaxPlan && percentUsed < 90) {
    return null;
  }

  const hasApiCredits = apiBalance !== undefined && apiBalance > 0;

  const formatCredits = (credits: number) => new Intl.NumberFormat("en-US").format(credits);

  const planLabel = toPlanNameLabel(productName);
  const resetFullLabel = formatResetDate(usageResetDate);

  const props: CreditUsageViewProps = {
    planLabel,
    percentRemaining,
    roundedRemaining,
    total,
    tokensRemaining,
    apiBalance,
    hasApiCredits,
    resetFullLabel,
    formatCredits
  };

  return <CreditUsageView {...props} />;
}
