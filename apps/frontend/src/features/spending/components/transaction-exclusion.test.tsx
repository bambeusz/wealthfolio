import { fireEvent, render, screen } from "@/test/render";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Table, TableBody, TooltipProvider } from "@wealthfolio/ui";
import type { ReactNode } from "react";
import { beforeAll, describe, expect, it, vi } from "vitest";

import type { CashActivity } from "../types/cash-activity";

// The inline popovers fetch on mount through a Tauri bridge that does not
// exist under jsdom, so their data hooks are stubbed.
vi.mock("@/hooks/use-taxonomies", () => ({
  useTaxonomy: () => ({ data: null, isLoading: false }),
  useTaxonomies: () => ({ data: [], isLoading: false }),
}));
vi.mock("../hooks/use-spending-events", () => ({
  useSpendingEvents: () => ({ data: [], isLoading: false }),
  useEventTypes: () => ({ data: [], isLoading: false }),
  useEventSpendingSummaries: () => ({ data: [], isLoading: false }),
}));
vi.mock("./event-dialog-provider", () => ({
  useEventDialog: () => ({ openEventDialog: vi.fn(), openEventTypeDialog: vi.fn() }),
}));
beforeAll(() => {
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: () => Promise.resolve(null),
    transformCallback: () => 0,
  };
});
import { toRowVM } from "../lib/transactions-helpers";
import { TransactionCard } from "./transaction-card";
import { TransactionRow } from "./transaction-row";

function activity(excludedFromSpending: boolean): CashActivity {
  return {
    id: "activity-1",
    activityType: "TRANSFER_OUT",
    activityDate: "2026-06-06T19:45:00.000Z",
    accountId: "account-1",
    amount: "500",
    currency: "USD",
    cashFlowBucket: "saving",
    assignments: [],
    splits: [],
    isUserModified: false,
    needsReview: false,
    netAmount: -500,
    notes: "Monthly sweep",
    status: "POSTED",
    sourceGroupId: "pair-1",
    transferLinkStatus: "linked",
    excludedFromSpending,
    createdAt: "2026-06-06T19:45:00.000Z",
    updatedAt: "2026-06-06T19:45:00.000Z",
  } as CashActivity;
}

function props(onToggleSpendingExclusion = vi.fn()) {
  return {
    account: undefined,
    event: null,
    eventTypeColor: null,
    appTimezone: "UTC",
    isSelected: false,
    showAccount: false,
    onToggleSelect: vi.fn(),
    onAssignCategory: vi.fn(),
    onClearCategory: vi.fn(),
    onSetEvent: vi.fn(),
    onMarkReimbursement: vi.fn(),
    onEditSplits: vi.fn(),
    onEdit: vi.fn(),
    onDuplicate: vi.fn(),
    onDelete: vi.fn(),
    onToggleSpendingExclusion,
  };
}

function withProviders({ children }: { children: ReactNode }) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{children}</TooltipProvider>
    </QueryClientProvider>
  );
}

function renderRow(excluded: boolean, onToggle = vi.fn()) {
  render(
    <Table>
      <TableBody>
        <TransactionRow row={toRowVM(activity(excluded), new Map())} {...props(onToggle)} />
      </TableBody>
    </Table>,
    { wrapper: withProviders },
  );
  return onToggle;
}

async function openRowMenu() {
  fireEvent.keyDown(screen.getByRole("button", { name: "Row actions" }), { key: "Enter" });
  return screen.findByRole("menu");
}

describe("Exclude from Spending", () => {
  it("marks an excluded row and keeps it listed", () => {
    renderRow(true);

    expect(screen.getByText("Monthly sweep")).toBeInTheDocument();
    expect(screen.getByText("Excluded")).toBeInTheDocument();
  });

  it("does not mark a row that counts", () => {
    renderRow(false);

    expect(screen.queryByText("Excluded")).not.toBeInTheDocument();
  });

  it("offers to exclude a counted row from the row menu", async () => {
    const onToggle = renderRow(false);

    await openRowMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Exclude from Spending" }));

    expect(onToggle).toHaveBeenCalledWith(
      expect.objectContaining({ activity: expect.objectContaining({ id: "activity-1" }) }),
    );
  });

  it("offers to include an excluded row again", async () => {
    renderRow(true);

    await openRowMenu();

    expect(screen.getByRole("menuitem", { name: "Include in Spending" })).toBeInTheDocument();
    expect(
      screen.queryByRole("menuitem", { name: "Exclude from Spending" }),
    ).not.toBeInTheDocument();
  });

  it("marks an excluded card too", () => {
    render(
      <TransactionCard
        row={toRowVM(activity(true), new Map())}
        selectionMode={false}
        {...props()}
      />,
      { wrapper: withProviders },
    );

    expect(screen.getByText("Excluded")).toBeInTheDocument();
  });
});
