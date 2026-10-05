import { useState } from "react";
import { useTranslation } from "react-i18next";

import {
  Button,
  Icons,
  Input,
  Label,
  Popover,
  PopoverContent,
  PopoverTrigger,
  RadioGroup,
  RadioGroupItem,
} from "@wealthfolio/ui";

import { cn } from "@/lib/utils";

import type { BudgetPacing } from "../types/budget";

interface BudgetPacingPopoverProps {
  categoryName: string;
  pacing: BudgetPacing;
  dueDay: number | null;
  onSave: (pacing: BudgetPacing, dueDay: number | null) => void;
}

function parseDueDay(value: string): number | null {
  const day = Number(value);
  return Number.isInteger(day) && day >= 1 && day <= 31 ? day : null;
}

/**
 * How a category's monthly amount is expected to be spent: evenly across the
 * month, or once on a due day (rent, subscriptions). The backend paces the
 * on-track status from this, so a bill paid on its due day stays on track.
 */
export function BudgetPacingPopover({
  categoryName,
  pacing,
  dueDay,
  onSave,
}: BudgetPacingPopoverProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [draftPacing, setDraftPacing] = useState<BudgetPacing>(pacing);
  const [draftDay, setDraftDay] = useState(String(dueDay ?? 1));
  const isFixed = pacing === "monthly_on_day" && dueDay !== null;
  const parsedDay = parseDueDay(draftDay);
  const canSave = draftPacing === "linear" || parsedDay !== null;

  const handleOpenChange = (next: boolean) => {
    if (next) {
      // Start from the saved value each time the popover opens.
      setDraftPacing(pacing);
      setDraftDay(String(dueDay ?? 1));
    }
    setOpen(next);
  };

  const save = () => {
    if (!canSave) return;
    onSave(draftPacing, draftPacing === "monthly_on_day" ? parsedDay : null);
    setOpen(false);
  };

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className={cn(
            "h-6 shrink-0 gap-0.5 px-1 text-[10px] tabular-nums",
            isFixed ? "text-foreground" : "text-muted-foreground/60 hover:text-foreground",
          )}
          aria-label={t("spending:budgetEditor.pacingLabel", { name: categoryName })}
        >
          <Icons.Calendar className="h-3 w-3" />
          {isFixed ? <span>{dueDay}</span> : null}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-64 p-3" align="end">
        <div className="text-foreground mb-2 text-xs font-semibold">
          {t("spending:budgetEditor.pacingTitle")}
        </div>
        <RadioGroup
          value={draftPacing}
          onValueChange={(value) => setDraftPacing(value as BudgetPacing)}
          className="gap-3"
        >
          <div className="flex items-start gap-2">
            <RadioGroupItem value="linear" id="budget-pacing-linear" className="mt-0.5" />
            <Label htmlFor="budget-pacing-linear" className="text-xs font-normal leading-snug">
              <span className="text-foreground block font-medium">
                {t("spending:budgetEditor.pacingLinear")}
              </span>
              <span className="text-muted-foreground">
                {t("spending:budgetEditor.pacingLinearHint")}
              </span>
            </Label>
          </div>
          <div className="flex items-start gap-2">
            <RadioGroupItem value="monthly_on_day" id="budget-pacing-monthly" className="mt-0.5" />
            <div className="min-w-0 flex-1">
              <Label htmlFor="budget-pacing-monthly" className="text-xs font-normal leading-snug">
                <span className="text-foreground block font-medium">
                  {t("spending:budgetEditor.pacingMonthlyOnDay")}
                </span>
                <span className="text-muted-foreground">
                  {t("spending:budgetEditor.pacingMonthlyOnDayHint")}
                </span>
              </Label>
              <Input
                type="number"
                inputMode="numeric"
                min={1}
                max={31}
                value={draftDay}
                disabled={draftPacing !== "monthly_on_day"}
                onChange={(event) => setDraftDay(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") save();
                }}
                aria-label={t("spending:budgetEditor.pacingDueDay")}
                aria-invalid={draftPacing === "monthly_on_day" && parsedDay === null}
                className="mt-1.5 h-7 w-20 text-xs"
              />
              {draftPacing === "monthly_on_day" && parsedDay === null ? (
                <p className="text-destructive mt-1 text-[11px]">
                  {t("spending:budgetEditor.pacingInvalidDay")}
                </p>
              ) : null}
            </div>
          </div>
        </RadioGroup>
        <div className="mt-3 flex justify-end">
          <Button
            type="button"
            size="sm"
            className="h-7 text-xs"
            disabled={!canSave}
            onClick={save}
          >
            {t("common:save")}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
