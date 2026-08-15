import { describe, expect, mock, test } from "bun:test";
import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";

const billingStatus = {
  product_name: "Pro",
  total_tokens: 100,
  used_tokens: 25,
  api_credit_balance: 50,
  usage_reset_date: null
};

mock.module("@tanstack/react-query", () => ({
  useQuery: () => ({ data: undefined })
}));

mock.module("@tanstack/react-router", () => ({
  Link: ({
    to,
    state,
    children,
    ...props
  }: Omit<ComponentPropsWithoutRef<"a">, "href"> & {
    to: string;
    state?: unknown;
    children: ReactNode;
  }) => {
    void state;
    return (
      <a href={to} {...props}>
        {children}
      </a>
    );
  }
}));

mock.module("@opensecret/react", () => ({
  useOpenSecret: () => ({ auth: { user: { user: { id: "user-1" } } } })
}));

mock.module("@/billing/billingService", () => ({
  getBillingService: () => ({
    getBillingStatus: async () => billingStatus,
    getTeamStatus: async () => null
  })
}));

mock.module("@/components/settings/useCompactSettingsLayout", () => ({
  useCompactSettingsLayout: () => true
}));

mock.module("@/state/useLocalState", () => ({
  useBillingState: () => ({ billingStatus, setBillingStatus: () => {} })
}));

const { AccountMenu } = await import("./AccountMenu");

describe("AccountMenu", () => {
  test("keeps Settings and Usage together by default", () => {
    const markup = renderToStaticMarkup(<AccountMenu pagePresentation />);

    expect(markup).toContain('href="/settings"');
    expect(markup).toContain('href="/pricing"');
    expect(markup).toContain('aria-label="Open settings"');
    expect(markup).toContain("h-11 w-11");
  });

  test("renders the shared 44px Settings control without Usage for a page header", () => {
    const markup = renderToStaticMarkup(<AccountMenu pagePresentation showCreditUsage={false} />);

    expect(markup).toContain('href="/settings"');
    expect(markup).toContain('aria-label="Open settings"');
    expect(markup).toContain("h-11 w-11");
    expect(markup).toContain("w-auto");
    expect(markup).not.toContain('href="/pricing"');
    expect(markup.match(/<a /g)).toHaveLength(1);
  });
});
