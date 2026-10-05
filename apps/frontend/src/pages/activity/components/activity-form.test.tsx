import { ActivityType } from "@/lib/constants";
import type { ActivityDetails } from "@/lib/types";
import { render, screen } from "@/test/render";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ActivityForm } from "./activity-form";
import type { AccountSelectOption } from "./forms/fields";

const { useActivityFormMock } = vi.hoisted(() => ({
  useActivityFormMock: vi.fn(),
}));

vi.mock("@wealthfolio/ui/components/ui/sheet", () => ({
  Sheet: ({ children, open }: { children: ReactNode; open?: boolean }) =>
    open ? <>{children}</> : null,
  SheetContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  SheetDescription: ({ children }: { children: ReactNode }) => <p>{children}</p>,
  SheetFooter: ({ children }: { children: ReactNode }) => <footer>{children}</footer>,
  SheetHeader: ({ children }: { children: ReactNode }) => <header>{children}</header>,
  SheetTitle: ({ children }: { children: ReactNode }) => <h2>{children}</h2>,
}));

// The picker and the forms have their own tests; what matters here is whether
// the picker is offered at all, and which type the renderer is asked for.
vi.mock("./activity-type-picker", () => ({
  ActivityTypePicker: ({
    value,
    onSelect,
    includeReclassificationTypes,
    allowedTypes,
  }: {
    value?: string;
    onSelect: (type: string) => void;
    includeReclassificationTypes?: boolean;
    allowedTypes?: readonly string[];
  }) => (
    <div
      data-testid="type-picker"
      data-value={value ?? ""}
      data-allowed={allowedTypes?.join(",") ?? ""}
    >
      <button type="button" onClick={() => onSelect(ActivityType.DEPOSIT)}>
        pick deposit
      </button>
      {includeReclassificationTypes && (
        <button type="button" onClick={() => onSelect(ActivityType.ADJUSTMENT)}>
          pick adjustment
        </button>
      )}
    </div>
  ),
}));

vi.mock("./activity-form-renderer", () => ({
  ActivityFormRenderer: ({
    selectedType,
    accounts,
  }: {
    selectedType?: string;
    accounts: AccountSelectOption[];
  }) => (
    <>
      <div data-testid="form-renderer">{selectedType ?? "no-type"}</div>
      <div data-testid="rendered-account-ids">
        {accounts.map((account) => account.value).join(",")}
      </div>
    </>
  ),
}));

vi.mock("../hooks/use-activity-form", () => ({
  useActivityForm: (params: unknown) => useActivityFormMock(params),
}));

beforeEach(() => {
  useActivityFormMock.mockReturnValue({
    defaultValues: undefined,
    isLoading: false,
    isError: false,
    error: null,
    handleSubmit: vi.fn(),
  });
});

const accounts: AccountSelectOption[] = [
  {
    value: "current-account",
    label: "Current account",
    currency: "USD",
    restrictionLevel: "blocked",
  },
  {
    value: "eligible-account",
    label: "Eligible account",
    currency: "USD",
    restrictionLevel: "none",
  },
  {
    value: "other-blocked-account",
    label: "Other blocked account",
    currency: "USD",
    restrictionLevel: "blocked",
  },
];

describe("ActivityForm account filtering", () => {
  it("keeps the activity's current restricted account available while editing", () => {
    render(
      <ActivityForm
        open
        accounts={accounts}
        activity={{
          id: "credit-activity",
          accountId: "current-account",
          activityType: ActivityType.CREDIT,
        }}
      />,
    );

    expect(screen.getByTestId("rendered-account-ids")).toHaveTextContent(
      "current-account,eligible-account",
    );
  });

  it("continues to exclude restricted accounts when creating an activity", () => {
    render(
      <ActivityForm open accounts={accounts} activity={{ activityType: ActivityType.CREDIT }} />,
    );

    expect(screen.getByTestId("rendered-account-ids")).toHaveTextContent("eligible-account");
  });
});

const reclassificationAccounts: AccountSelectOption[] = [
  { value: "acc_1", label: "Brokerage", currency: "USD" } as AccountSelectOption,
];

function activity(id: string, activityType: ActivityType): Partial<ActivityDetails> {
  return { id, activityType, accountId: "acc_1", currency: "USD" };
}

function renderForm(activityToEdit: Partial<ActivityDetails>) {
  return render(
    <ActivityForm
      accounts={reclassificationAccounts}
      open
      onClose={vi.fn()}
      activity={activityToEdit}
    />,
  );
}

function renderedType() {
  return screen.getByTestId("form-renderer").textContent;
}

describe("ActivityForm reclassification", () => {
  it("offers the type picker for a stored type that has no editor", () => {
    renderForm(activity("act_1", ActivityType.UNKNOWN));

    // The row is reported under the type it is stored as, alongside the picker.
    expect(screen.getByText(ActivityType.UNKNOWN)).toBeInTheDocument();
    expect(screen.getByTestId("type-picker")).toHaveAttribute("data-value", "");
    expect(renderedType()).toBe("no-type");
  });

  it("renders the picked type's form for the row being reclassified", async () => {
    const user = userEvent.setup();
    renderForm(activity("act_1", ActivityType.UNKNOWN));

    await user.click(screen.getByRole("button", { name: "pick deposit" }));

    expect(renderedType()).toBe(ActivityType.DEPOSIT);
  });

  // ADJUSTMENT is the one editable type the picker never offers for a new
  // activity, so reclassification is the only way to reach it.
  it("can reclassify UNKNOWN as ADJUSTMENT", async () => {
    const user = userEvent.setup();
    renderForm(activity("act_1", ActivityType.UNKNOWN));

    await user.click(screen.getByRole("button", { name: "pick adjustment" }));

    expect(renderedType()).toBe(ActivityType.ADJUSTMENT);
  });

  it.each([ActivityType.CREDIT, ActivityType.ADJUSTMENT])(
    "keeps a %s row on its own type instead of asking for a new one",
    (activityType) => {
      renderForm(activity("act_1", activityType));

      expect(screen.queryByTestId("type-picker")).not.toBeInTheDocument();
      expect(renderedType()).toBe(activityType);
    },
  );

  it("offers Change type for an editable stored type and hides the picker until it is used", async () => {
    const user = userEvent.setup();
    renderForm(activity("act_1", ActivityType.WITHDRAWAL));

    expect(screen.queryByTestId("type-picker")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Change type" }));
    expect(screen.getByTestId("type-picker")).toBeInTheDocument();
    // The stored form stays until a replacement is picked.
    expect(renderedType()).toBe(ActivityType.WITHDRAWAL);

    await user.click(screen.getByRole("button", { name: "pick deposit" }));
    expect(renderedType()).toBe(ActivityType.DEPOSIT);

    await user.click(screen.getByRole("button", { name: "Keep type" }));
    expect(screen.queryByTestId("type-picker")).not.toBeInTheDocument();
    expect(renderedType()).toBe(ActivityType.WITHDRAWAL);
  });

  it("does not offer Change type when creating or for a type without an editor", () => {
    const { unmount } = render(<ActivityForm accounts={reclassificationAccounts} open />);
    expect(screen.queryByRole("button", { name: "Change type" })).not.toBeInTheDocument();
    unmount();

    renderForm(activity("act_1", ActivityType.UNKNOWN));
    expect(screen.queryByRole("button", { name: "Change type" })).not.toBeInTheDocument();
  });

  // The Spending ledger opens its transfer editor with hidePicker, because a
  // new transfer is already typed; editing a leg must still be able to retype it.
  describe("with hidePicker (Spending ledger entry point)", () => {
    it("still offers Change type for a linked transfer leg, limited to the allowed types", async () => {
      const user = userEvent.setup();
      render(
        <ActivityForm
          accounts={reclassificationAccounts}
          open
          hidePicker
          allowedTypes={["DEPOSIT", "WITHDRAWAL", "TRANSFER"]}
          activity={{ ...activity("act_1", ActivityType.TRANSFER_OUT), sourceGroupId: "group-1" }}
        />,
      );

      expect(screen.queryByTestId("type-picker")).not.toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Change type" }));
      expect(screen.getByTestId("type-picker")).toHaveAttribute(
        "data-allowed",
        "DEPOSIT,WITHDRAWAL,TRANSFER",
      );

      await user.click(screen.getByRole("button", { name: "pick deposit" }));
      expect(renderedType()).toBe(ActivityType.DEPOSIT);
      expect(screen.getByText(/unlinks the pair first/)).toBeInTheDocument();
    });

    it("still hides the picker when creating", () => {
      render(
        <ActivityForm
          accounts={reclassificationAccounts}
          open
          hidePicker
          activity={{ activityType: ActivityType.TRANSFER_OUT, accountId: "acc_1" }}
        />,
      );

      expect(screen.queryByTestId("type-picker")).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Change type" })).not.toBeInTheDocument();
      expect(renderedType()).toBe("TRANSFER");
    });
  });

  it("warns that leaving a linked transfer unlinks it", async () => {
    const user = userEvent.setup();
    renderForm({
      ...activity("act_1", ActivityType.TRANSFER_OUT),
      sourceGroupId: "group-1",
    });

    await user.click(screen.getByRole("button", { name: "Change type" }));
    expect(screen.queryByText(/unlinks the pair first/)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "pick deposit" }));
    expect(screen.getByText(/unlinks the pair first/)).toBeInTheDocument();
  });

  it("resets the Change type picker when a different activity is opened", async () => {
    const user = userEvent.setup();
    const { rerender } = renderForm(activity("act_1", ActivityType.WITHDRAWAL));

    await user.click(screen.getByRole("button", { name: "Change type" }));
    expect(screen.getByTestId("type-picker")).toBeInTheDocument();

    rerender(
      <ActivityForm
        accounts={reclassificationAccounts}
        open
        onClose={vi.fn()}
        activity={activity("act_2", ActivityType.WITHDRAWAL)}
      />,
    );

    expect(screen.queryByTestId("type-picker")).not.toBeInTheDocument();
  });

  it("drops the picked type when a different activity is opened", async () => {
    const user = userEvent.setup();
    const { rerender } = renderForm(activity("act_1", ActivityType.UNKNOWN));

    await user.click(screen.getByRole("button", { name: "pick deposit" }));
    expect(renderedType()).toBe(ActivityType.DEPOSIT);

    rerender(
      <ActivityForm
        accounts={reclassificationAccounts}
        open
        onClose={vi.fn()}
        activity={activity("act_2", ActivityType.UNKNOWN)}
      />,
    );

    expect(screen.getByTestId("type-picker")).toHaveAttribute("data-value", "");
    expect(renderedType()).toBe("no-type");
  });
});
