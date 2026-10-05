import { render, screen } from "@testing-library/react";
import { FormattingProvider } from "@wealthfolio/ui";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { WhereIAmStage } from "./where-i-am-stage";
import type { MonthlyReport } from "../../../types/report";
import type { BudgetSnapshot } from "../../../types/budget";
import type { PaceState } from "../../../types/insight";
import { comparisonRange } from "../../../lib/reports-period";
import { spendingRangeToReportsRange } from "../../../lib/date-range-params";

vi.mock("@/hooks/use-balance-privacy", () => ({
  useBalancePrivacy: () => ({ isBalanceHidden: false }),
}));

vi.mock("../../../hooks/use-spending-settings", () => ({
  useSpendingSettings: () => ({ excludedCategoryIds: [] }),
}));

function report(outflow: number): MonthlyReport {
  const summary = { income: 0, outflow, saved: 0, net: -outflow, count: 1 };
  return {
    baseCurrency: "USD",
    current: summary,
    prior: summary,
    spendingBreakdown: [],
    incomeBreakdown: [],
    savingsBreakdown: [],
    byDay: [],
    byDayByCategory: [],
  };
}

const timezone = "America/Toronto";
const range = spendingRangeToReportsRange(
  { from: new Date(2025, 2, 8), to: new Date(2025, 2, 10) },
  timezone,
);

function setup(custom: boolean) {
  render(
    <MemoryRouter>
      <FormattingProvider locale="en-US" timezone={timezone}>
        <WhereIAmStage
          range={range}
          priorRange={custom ? comparisonRange(range, "prior", timezone)! : undefined}
          currentReport={report(300)}
          priorReport={report(150)}
          months={[]}
          taxonomyCategories={[]}
          incomeCategories={[]}
          savingsCategories={[]}
          budget={undefined}
          currency="USD"
          isLoading={false}
        />
      </FormattingProvider>
    </MemoryRouter>,
  );
}

function budgetWithPlanned(planned: number): BudgetSnapshot {
  return {
    state: { groups: [], groupAssignments: [], targets: [], rolloverSettings: [] },
    computed: {
      currency: "USD",
      periodKey: "default",
      fxAsOf: null,
      groupRows: [],
      ungroupedRows: [],
      incomeRows: [],
      totals: {
        spendingPlanned: planned,
        spendingActual: 0,
        spendingRemaining: planned,
        incomePlanned: 0,
        incomeActual: 0,
        groupBuffer: 0,
        rolloverIn: 0,
        rolloverOut: 0,
        overspentCount: 0,
      },
      pace: null,
    },
  };
}

function pace(status: PaceState["status"], expectedSpendToDate: number): PaceState {
  return {
    dailyAvg: 0,
    daysElapsed: 3,
    daysRemaining: 0,
    projectedSpend: 0,
    expectedSpendToDate,
    fixedExpectedToDate: 0,
    flexibleExpectedToDate: expectedSpendToDate,
    projectionReliable: false,
    status,
  };
}

function setupWithPace(spent: number, planned: number, reconciledPace: PaceState) {
  render(
    <MemoryRouter>
      <FormattingProvider locale="en-US" timezone={timezone}>
        <WhereIAmStage
          range={range}
          currentReport={report(spent)}
          priorReport={report(0)}
          months={[]}
          taxonomyCategories={[]}
          incomeCategories={[]}
          savingsCategories={[]}
          budget={budgetWithPlanned(planned)}
          currency="USD"
          isLoading={false}
          reconciledPace={reconciledPace}
        />
      </FormattingProvider>
    </MemoryRouter>,
  );
}

describe("Where I am pace status", () => {
  it("shows the backend status even when most of the budget is spent", () => {
    // A fixed share-of-budget threshold would call 95% spent "trending high";
    // a rent paid on its due day is on track by the backend's pacing-aware rule.
    setupWithPace(950, 1000, pace("on_track", 960));
    expect(screen.getByText("ON TRACK")).toBeInTheDocument();
    expect(screen.queryByText("TRENDING HIGH")).not.toBeInTheDocument();
  });

  it("shows approaching from the backend at a low share of the budget", () => {
    setupWithPace(300, 1000, pace("approaching", 200));
    expect(screen.getByText("TRENDING HIGH")).toBeInTheDocument();
  });

  it("shows over from the backend", () => {
    setupWithPace(1100, 1000, pace("over", 1000));
    expect(screen.getByText("OVER BUDGET")).toBeInTheDocument();
  });
});

describe("Where I am comparison labels", () => {
  it("labels a custom span as a period and shows the actual prior dates across DST", () => {
    setup(true);
    expect(screen.getByText("SPENT THIS PERIOD")).toBeInTheDocument();
    expect(screen.getByText(/vs Mar 5, 2025 – Mar 7, 2025/)).toBeInTheDocument();
    expect(screen.queryByText("SPENT THIS MONTH")).not.toBeInTheDocument();
    expect(screen.queryByText(/vs Feb/)).not.toBeInTheDocument();
  });
  it("retains month labels for preset month selections", () => {
    setup(false);
    expect(screen.getByText("SPENT THIS MONTH")).toBeInTheDocument();
    expect(screen.getByText(/vs Feb/)).toBeInTheDocument();
  });
});
