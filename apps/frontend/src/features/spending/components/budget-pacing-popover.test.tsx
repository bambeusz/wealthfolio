import { render, screen } from "@/test/render";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { BudgetPacingPopover } from "./budget-pacing-popover";

describe("BudgetPacingPopover", () => {
  it("saves once-a-month pacing with its due day", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(
      <BudgetPacingPopover categoryName="Housing" pacing="linear" dueDay={null} onSave={onSave} />,
    );

    await user.click(screen.getByRole("button", { name: "Pacing for Housing" }));
    await user.click(screen.getByRole("radio", { name: /Due on day/ }));
    const day = screen.getByRole("spinbutton", { name: "Due day" });
    await user.clear(day);
    await user.type(day, "31");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(onSave).toHaveBeenCalledWith("monthly_on_day", 31);
  });

  it("rejects a due day outside 1–31", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(
      <BudgetPacingPopover
        categoryName="Housing"
        pacing="monthly_on_day"
        dueDay={5}
        onSave={onSave}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Pacing for Housing" }));
    const day = screen.getByRole("spinbutton", { name: "Due day" });
    await user.clear(day);
    await user.type(day, "32");

    expect(screen.getByText("Enter a day from 1 to 31.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(onSave).not.toHaveBeenCalled();
  });

  it("switches back to an even spread without a due day", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(
      <BudgetPacingPopover
        categoryName="Housing"
        pacing="monthly_on_day"
        dueDay={5}
        onSave={onSave}
      />,
    );

    // The trigger shows the current due day.
    expect(screen.getByRole("button", { name: "Pacing for Housing" })).toHaveTextContent("5");
    await user.click(screen.getByRole("button", { name: "Pacing for Housing" }));
    await user.click(screen.getByRole("radio", { name: /Spread evenly/ }));
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(onSave).toHaveBeenCalledWith("linear", null);
  });
});
