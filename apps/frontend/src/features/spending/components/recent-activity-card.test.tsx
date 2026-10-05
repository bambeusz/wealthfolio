import { render, screen } from "@/test/render";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { RecentActivityCard } from "./recent-activity-card";
import type { Activity } from "@/lib/types";

vi.mock("@tanstack/react-query", () => ({
  useQueries: vi.fn(() => []),
}));

function renderRecentActivityCard(activities: Activity[] = []) {
  return render(
    <MemoryRouter>
      <RecentActivityCard activities={activities} categoriesMeta={new Map()} />
    </MemoryRouter>,
  );
}

describe("RecentActivityCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows the standard empty state without a setup link", () => {
    renderRecentActivityCard();

    expect(screen.getByText("No recent activity.")).toBeInTheDocument();
    expect(screen.queryByText("No spending accounts selected.")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("link", { name: "Open spending settings →" }),
    ).not.toBeInTheDocument();
  });

  it("labels each activity with its own currency, not the base currency", () => {
    const activity: Activity = {
      id: "1",
      accountId: "acc-1",
      activityDate: new Date().toISOString().slice(0, 10),
      activityType: "WITHDRAWAL",
      amount: "100",
      currency: "EUR",
      notes: "Coffee",
    } as unknown as Activity;

    renderRecentActivityCard([activity]);

    const row = screen.getByText("Coffee").closest("a");
    expect(row).toBeInTheDocument();
    expect(row?.textContent).toContain("€");
    expect(row?.textContent).not.toContain("$");
  });

  it("marks a row excluded from Spending instead of asking to tag it", () => {
    const day = new Date().toISOString().slice(0, 10);
    const row = (id: string, notes: string, excludedFromSpending: boolean) =>
      ({
        id,
        accountId: "acc-1",
        activityDate: day,
        activityType: "WITHDRAWAL",
        amount: "40",
        currency: "USD",
        notes,
        excludedFromSpending,
      }) as unknown as Activity;

    renderRecentActivityCard([row("1", "Card sweep", true), row("2", "Bakery", false)]);

    const excluded = screen.getByText("Card sweep").closest("a");
    expect(excluded?.textContent).toContain("Excluded");
    expect(excluded?.textContent).not.toContain("Uncategorized");
    expect(excluded?.querySelector(".tabular-nums")).toHaveClass("text-muted-foreground");

    const counted = screen.getByText("Bakery").closest("a");
    expect(counted?.textContent).not.toContain("Excluded");
    expect(counted?.textContent).toContain("Uncategorized");
  });
});
