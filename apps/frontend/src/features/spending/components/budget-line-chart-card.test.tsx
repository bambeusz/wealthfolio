import { render as plainRender } from "@testing-library/react";
import { render, screen } from "@/test/render";
import { FormattingProvider } from "@wealthfolio/ui";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { BudgetCategoryRow, BudgetPace, PaceStatus } from "../types/budget";
import { BudgetLineChartCard } from "./budget-line-chart-card";

function pace(overrides: Partial<BudgetPace> = {}): BudgetPace {
  return {
    totalDays: 31,
    elapsedDays: 1,
    live: true,
    available: 1600,
    spent: 1000,
    expectedToDate: 1019.35,
    fixedExpectedToDate: 1000,
    flexibleExpectedToDate: 19.35,
    projected: 1000,
    projectionReliable: false,
    flexibleDailyRate: 0,
    status: "on_track",
    curveSource: "linear",
    expectedCurve: Array.from({ length: 31 }, (_, i) => 1000 + (600 * (i + 1)) / 31),
    spentCurve: [1000],
    ...overrides,
  };
}

function row(
  categoryId: string,
  overrides: Partial<BudgetCategoryRow> & { paceStatus: PaceStatus },
): BudgetCategoryRow {
  return {
    taxonomyId: "spending_categories",
    categoryId,
    groupId: "group-needs",
    parentId: null,
    name: categoryId,
    color: null,
    icon: null,
    target: 100,
    actual: 0,
    rolloverIn: 0,
    rolloverOut: 0,
    remaining: 100,
    overspent: false,
    hasDefaultTarget: true,
    hasMonthOverride: false,
    rolloverEnabled: false,
    pacing: "linear",
    dueDay: null,
    expectedToDate: 0,
    projected: 0,
    ...overrides,
  };
}

function renderCard(budgetPace: BudgetPace | null, allocations: BudgetCategoryRow[] = []) {
  render(
    <MemoryRouter>
      <BudgetLineChartCard
        monthKey="2026-10"
        today={{ year: 2026, month: 10, day: 1 }}
        isCurrentMonth
        onPreviousMonth={() => undefined}
        onNextMonth={() => undefined}
        canGoNextMonth={false}
        activityRange={{ from: "2026-10-01", to: "2026-10-01" }}
        pace={budgetPace}
        currency="USD"
        allocations={allocations}
        categoriesMeta={new Map()}
      />
    </MemoryRouter>,
  );
}

/** Food spent 1,234.50 of a 1,000 budget by day 16. */
function renderOverspentFood() {
  return render(
    <MemoryRouter>
      <BudgetLineChartCard
        monthKey="2026-10"
        today={{ year: 2026, month: 10, day: 16 }}
        isCurrentMonth
        onPreviousMonth={vi.fn()}
        onNextMonth={vi.fn()}
        canGoNextMonth={false}
        activityRange={{ from: "2026-10-01", to: "2026-10-31" }}
        pace={pace({
          elapsedDays: 16,
          available: 1000,
          spent: 1234.5,
          status: "over",
          expectedCurve: Array.from({ length: 31 }, (_, i) => (1000 * (i + 1)) / 31),
          spentCurve: Array.from({ length: 16 }, (_, i) => (i >= 4 ? 1234.5 : 0)),
        })}
        currency="USD"
        allocations={[
          row("food", { target: 1000, actual: 1234.5, remaining: -234.5, paceStatus: "over" }),
        ]}
        categoriesMeta={
          new Map([["food", { name: "Food", color: null, icon: null, parentId: null }]])
        }
      />
    </MemoryRouter>,
  );
}

afterEach(() => {
  localStorage.removeItem("privacy-settings");
});

describe("BudgetLineChartCard", () => {
  it("reads On track from the backend when rent was paid on its due day", () => {
    // Rent (1000, due on the 1st) paid on day 1 is most of the month's
    // budget; a straight day/days pace line would call it "trending high".
    renderCard(pace(), [
      row("cat_housing", {
        target: 1000,
        actual: 1000,
        remaining: 0,
        pacing: "monthly_on_day",
        dueDay: 1,
        paceStatus: "on_track",
      }),
    ]);

    expect(screen.getByText("On track")).toBeInTheDocument();
    expect(screen.queryByText("Trending high")).not.toBeInTheDocument();
    expect(screen.getByText("Day 1 / 31")).toBeInTheDocument();
  });

  it("shows the backend's approaching and over statuses", () => {
    renderCard(pace({ status: "approaching", spent: 1100, spentCurve: [1100] }));
    expect(screen.getByText("Trending high")).toBeInTheDocument();
  });

  it("shows over budget when the backend says so", () => {
    renderCard(pace({ status: "over", spent: 1700, spentCurve: [1700] }));
    expect(screen.getByText("Over budget")).toBeInTheDocument();
  });

  it("labels a once-a-month ring with its due day until it is due", () => {
    renderCard(pace(), [
      row("cat_subscriptions", {
        target: 30,
        pacing: "monthly_on_day",
        dueDay: 31,
        remaining: 30,
        paceStatus: "on_track",
      }),
      row("cat_food", {
        target: 200,
        actual: 250,
        remaining: -50,
        paceStatus: "over",
      }),
    ]);

    expect(screen.getByText("due day 31")).toBeInTheDocument();
    expect(screen.getByText("over")).toBeInTheDocument();
  });

  it("asks for a budget when the month has none", () => {
    renderCard(null);
    expect(screen.getByText("No monthly target set for this budget month.")).toBeInTheDocument();
  });

  it.each([
    ["JPY", 1234, 1000, "food: 1,234 / 1,000"],
    ["KWD", 1.2345, 1, "food: 1.235 / 1.000"],
    ["EUR", 1234.5, 1000, "food: 1.234,50 / 1.000,00"],
  ])("formats a %s ring's tooltip in the currency's format", (currency, spent, target, title) => {
    plainRender(
      <MemoryRouter>
        <FormattingProvider locale="US" currencyFormattingRegions={{ EUR: "DE" }}>
          <BudgetLineChartCard
            monthKey="2026-09"
            today={{ year: 2026, month: 9, day: 30 }}
            isCurrentMonth={false}
            onPreviousMonth={vi.fn()}
            onNextMonth={vi.fn()}
            canGoNextMonth={false}
            activityRange={{ from: "2026-09-01", to: "2026-09-30" }}
            pace={pace({
              totalDays: 30,
              elapsedDays: 30,
              live: false,
              available: target,
              spent,
              status: "over",
              expectedCurve: Array.from({ length: 30 }, () => target),
              spentCurve: Array.from({ length: 30 }, () => spent),
            })}
            currency={currency}
            allocations={[
              row("food", { target, actual: spent, remaining: target - spent, paceStatus: "over" }),
            ]}
            categoriesMeta={new Map()}
          />
        </FormattingProvider>
      </MemoryRouter>,
    );

    expect(screen.getByTitle(title)).toBeInTheDocument();
  });
});

describe("BudgetLineChartCard with Hide Balance", () => {
  it("shows the pace pill amount and the ring's amounts when amounts are visible", () => {
    renderOverspentFood();

    expect(screen.getByText("$235 over budget")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /\$235/ })).toHaveAttribute(
      "title",
      "Food: 1,234.50 / 1,000.00",
    );
  });

  it("masks the pace pill amount and the ring's tooltip", () => {
    // The card and `PrivacyAmount` read the stored setting the privacy hook is built on.
    localStorage.setItem("privacy-settings", "true");
    const { container } = renderOverspentFood();

    expect(screen.getByText("•••• over budget")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /••••/ })).toHaveAttribute(
      "title",
      "Food: •••• / ••••",
    );
    expect(container.textContent).not.toMatch(/\$/);
  });
});
