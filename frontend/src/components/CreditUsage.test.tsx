import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { CreditUsageView } from "./CreditUsage";

const longPlanName = "Maple Professional Workspace With An Exceptionally Long Name";

describe("CreditUsage", () => {
  test("constrains and truncates a long plan label inside the shared card", () => {
    const markup = renderToStaticMarkup(
      <CreditUsageView
        pagePresentation
        planLabel={longPlanName}
        percentUsed={25}
        roundedUsed={25}
        used={25}
        apiBalance={50}
        hasApiCredits
        resetFullLabel="Resets monthly"
        formatCredits={(credits) => credits.toString()}
      />
    );

    expect(markup).toContain(longPlanName);
    expect(markup).toContain("max-w-full");
    expect(markup).toContain("truncate");
  });
});
