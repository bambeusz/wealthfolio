import { render, screen } from "@/test/render";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CategoryRankedBar, GroupedCategoryBlock } from "./spending-tab-content";

const privacy = vi.hoisted(() => ({ hidden: false }));

vi.mock("@/hooks/use-balance-privacy", () => ({
  useBalancePrivacy: () => ({ isBalanceHidden: privacy.hidden }),
}));

// PrivacyAmount reads the shared UI hook, which is backed by localStorage.
function setHidden(hidden: boolean) {
  privacy.hidden = hidden;
  if (hidden) localStorage.setItem("privacy-settings", "true");
  else localStorage.removeItem("privacy-settings");
}

beforeEach(() => setHidden(false));
afterEach(() => setHidden(false));

const rows = [
  { id: "groceries", name: "Groceries", amount: 1234567.5, color: null, icon: null },
  { id: "travel", name: "Travel", amount: 765432.5, color: null, icon: null },
];

const hrefFor = (id: string) => `/activities?category=${id}`;

describe("CategoryRankedBar (flat layout)", () => {
  function renderBar() {
    return render(
      <MemoryRouter>
        <CategoryRankedBar
          rows={rows}
          total={2000000}
          currency="USD"
          themeColor="#336699"
          hasNoIncludedAccounts={false}
          activityHrefFor={hrefFor}
        />
      </MemoryRouter>,
    );
  }

  it("titles the name and the full amount, and keeps the amount column from shrinking", () => {
    renderBar();

    expect(screen.getByTitle("Groceries")).toBeInTheDocument();
    const amount = screen.getByTitle("$1,234,567.50");
    expect(amount.className).toContain("shrink-0");
    expect(amount.className).toContain("whitespace-nowrap");
    expect(amount.className).toContain("min-w-24");
  });

  it("drops the amount titles when balances are hidden but keeps the name", () => {
    setHidden(true);
    renderBar();

    expect(screen.getByTitle("Groceries")).toBeInTheDocument();
    expect(screen.queryByTitle("$1,234,567.50")).not.toBeInTheDocument();
    expect(screen.queryByTitle(/\$/)).not.toBeInTheDocument();
  });
});

describe("GroupedCategoryBlock", () => {
  const bucket = {
    id: "needs",
    name: "Needs",
    color: null,
    categories: rows,
    total: 2000000,
  };

  function renderBlock() {
    return render(
      <MemoryRouter>
        <GroupedCategoryBlock
          bucket={bucket}
          total={2000000}
          currency="USD"
          themeColor="#336699"
          activityHrefFor={hrefFor}
        />
      </MemoryRouter>,
    );
  }

  it("titles group and category names and amounts", async () => {
    renderBlock();
    await userEvent.setup().click(screen.getByRole("button"));

    expect(screen.getByTitle("Needs")).toBeInTheDocument();
    expect(screen.getByTitle("Travel")).toBeInTheDocument();
    expect(screen.getByTitle("$2,000,000.00").className).toContain("shrink-0");
    expect(screen.getByTitle("$765,432.50").className).toContain("whitespace-nowrap");
  });

  it("omits amount titles when balances are hidden", async () => {
    setHidden(true);
    renderBlock();
    await userEvent.setup().click(screen.getByRole("button"));

    expect(screen.getByTitle("Needs")).toBeInTheDocument();
    expect(screen.queryByTitle(/\$/)).not.toBeInTheDocument();
  });
});
