import { render, screen } from "@/test/render";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { BudgetRing } from "./budget-line-chart-card";

const ring = {
  categoryId: "groceries",
  name: "Groceries",
  color: null,
  icon: null,
  target: 1234567.5,
  spent: 1000000,
  pct: 0.81,
};

function renderRing() {
  return render(
    <MemoryRouter>
      <BudgetRing
        ring={ring}
        currency="USD"
        activityRange={{ from: "2026-01-01", to: "2026-01-31" }}
      />
    </MemoryRouter>,
  );
}

// The ring reads privacy from the shared UI hook, which is backed by localStorage.
beforeEach(() => localStorage.removeItem("privacy-settings"));
afterEach(() => localStorage.removeItem("privacy-settings"));

describe("BudgetRing", () => {
  it("carries the full formatted amounts in the tooltips and truncates the label", () => {
    renderRing();

    const link = screen.getByRole("link");
    expect(link).toHaveAttribute("title", expect.stringContaining("Groceries"));
    expect(link.getAttribute("title")).toContain("1,000,000.00");
    expect(link.getAttribute("title")).toContain("1,234,567.50");
    expect(link.className).toContain("w-20");
    expect(link.className).toContain("min-w-0");

    const amount = screen.getByTitle("$234,567.50");
    expect(amount.className).toContain("truncate");
    expect(amount.className).toContain("whitespace-nowrap");
    expect(amount.className).toContain("max-w-full");
  });

  it("shows only the name in the tooltip when balances are hidden", () => {
    localStorage.setItem("privacy-settings", "true");
    renderRing();

    expect(screen.getByRole("link")).toHaveAttribute("title", "Groceries");
    expect(screen.queryByTitle(/\d/)).not.toBeInTheDocument();
  });
});
