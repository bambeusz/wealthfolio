import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { DashboardCard } from "@/components/dashboard-card";
import { cn } from "@/lib/utils";
import {
  Icons,
  PrivacyAmount,
  useAmountFormatting,
  useBalancePrivacy,
  useDateFormatting,
} from "@wealthfolio/ui";

import type { BudgetCategoryRow, BudgetPace, PaceStatus } from "../types/budget";
import { CategoryIcon, type CategoryMetaMap } from "./category-chips";

type Status = "ok" | "warn" | "over";
interface PacePoint {
  day: number;
  value: number;
}
interface BudgetToday {
  year: number;
  month: number;
  day: number;
}

// The status comes from the backend pace (`budget::pacing`), shared with the
// rings, the Insights page and the health status — never re-derived here.
const STATUS_FROM_PACE: Record<PaceStatus, Status> = {
  on_track: "ok",
  approaching: "warn",
  over: "over",
};

function parseMonthKey(value: string | null | undefined): { year: number; month: number } | null {
  if (!value || !/^\d{4}-(0[1-9]|1[0-2])$/.test(value)) return null;
  const [year, month] = value.split("-").map(Number);
  return { year, month };
}

const STATUS_ACCENTS: Record<
  Status,
  {
    lineColor: string;
    pillBg: string;
    accent: string;
    Icon: typeof Icons.AlertCircle;
    labelKey: string;
  }
> = {
  over: {
    lineColor: "#B85544",
    pillBg: "var(--destructive)",
    accent: "var(--destructive)",
    Icon: Icons.AlertTriangle,
    labelKey: "spending:budgetChart.overBudget",
  },
  warn: {
    lineColor: "#C28B47",
    pillBg: "#C28B47",
    accent: "#C28B47",
    Icon: Icons.AlertCircle,
    labelKey: "spending:budgetChart.trendingHigh",
  },
  ok: {
    lineColor: "hsl(73 84% 27%)",
    pillBg: "hsl(73 84% 27%)",
    accent: "var(--success)",
    Icon: Icons.CheckCircle ?? Icons.AlertCircle,
    labelKey: "spending:budgetChart.onTrack",
  },
};

export function BudgetLineChartCard({
  monthKey,
  today,
  isCurrentMonth,
  onPreviousMonth,
  onNextMonth,
  canGoNextMonth,
  activityRange,
  pace,
  currency,
  allocations,
  categoriesMeta,
}: {
  monthKey: string;
  today: BudgetToday;
  isCurrentMonth: boolean;
  onPreviousMonth: () => void;
  onNextMonth: () => void;
  canGoNextMonth: boolean;
  activityRange: { from: string; to: string };
  /** Month pace from the budget snapshot (`computed.pace`). */
  pace: BudgetPace | null;
  currency: string;
  allocations: BudgetCategoryRow[];
  categoriesMeta: CategoryMetaMap;
}) {
  const amountFormatting = useAmountFormatting();
  const dateFormatting = useDateFormatting();

  const { t } = useTranslation();
  const target = pace?.available ?? 0;
  const spent = pace?.spent ?? 0;
  const live = pace?.live ?? isCurrentMonth;
  // All hooks must run unconditionally — the `target <= 0` early return below
  // sits between hooks otherwise, which trips "Rendered more hooks than during
  // the previous render" when a target is added or cleared.
  const monthMeta = useMemo(() => {
    const parts = parseMonthKey(monthKey) ?? today;
    const year = parts.year;
    const month = parts.month;
    const daysInMonth = pace?.totalDays ?? new Date(year, month, 0).getDate();
    const dayOfMonth =
      pace?.elapsedDays ?? (isCurrentMonth ? Math.min(today.day, daysInMonth) : daysInMonth);
    return {
      dayOfMonth,
      daysInMonth,
      monthLabel: dateFormatting
        .formatCalendarDate(
          { year, month, day: 1 },
          { calendar: "gregory", month: "long", year: "numeric" },
        )
        .toUpperCase(),
      shortLabel: dateFormatting
        .formatCalendarDate(
          { year, month, day: 1 },
          { calendar: "gregory", month: "short", year: "numeric" },
        )
        .toUpperCase(),
    };
  }, [monthKey, isCurrentMonth, dateFormatting, today, pace?.totalDays, pace?.elapsedDays]);
  const { dayOfMonth, daysInMonth, monthLabel } = monthMeta;

  const cumulative = useMemo(
    () => (pace?.spentCurve ?? []).map((value, index) => ({ day: index + 1, value })),
    [pace?.spentCurve],
  );

  const rings = useMemo(() => {
    return allocations
      .map((al) => {
        const t = al.target || 0;
        if (t <= 0) return null;
        const meta = categoriesMeta.get(al.categoryId);
        const s = Math.max(0, al.actual);
        // Before a once-a-month category's due day, the ring says when it's due.
        const dueDay =
          al.pacing === "monthly_on_day" && al.dueDay ? Math.min(al.dueDay, daysInMonth) : null;
        return {
          id: al.categoryId,
          categoryId: al.categoryId,
          name: meta?.name ?? al.categoryId,
          color: meta?.color ?? null,
          icon: meta?.icon ?? null,
          target: t,
          spent: s,
          remaining: al.remaining,
          pct: s / t,
          status: al.paceStatus ?? "on_track",
          dueDay: live && dueDay !== null && dayOfMonth < dueDay ? dueDay : null,
        };
      })
      .filter((r): r is NonNullable<typeof r> => r !== null)
      .sort((x, y) => y.pct - x.pct);
  }, [allocations, categoriesMeta, daysInMonth, dayOfMonth, live]);

  // Chart geometry derived from target — captured here so actualPath useMemo
  // can depend on stable primitives instead of recomputing each render.
  const chartW = 320;
  const chartH = 110;
  const padL = 0;
  const padR = 0;
  const padT = 24;
  const padB = 14;
  const innerW = chartW - padL - padR;
  const innerH = chartH - padT - padB;
  const expectedCurve = pace?.expectedCurve;
  const yMax = Math.max(target, spent, ...(expectedCurve ?? [0])) * 1.05;

  const actualPath = useMemo(() => {
    if (!cumulative.length || yMax <= 0) return "";
    const xForDay = (day: number) => padL + ((day - 1) / Math.max(1, daysInMonth - 1)) * innerW;
    const yForVal = (v: number) => padT + (1 - v / yMax) * innerH;
    return toSvgPath(cumulative, xForDay, yForVal);
  }, [cumulative, daysInMonth, innerW, innerH, padL, padT, yMax]);

  // The expected line steps up on each once-a-month due day and follows the
  // historical (or even) curve for the rest.
  const targetPacePath = useMemo(() => {
    if (!expectedCurve?.length || target <= 0 || yMax <= 0) return "";
    const xForDay = (day: number) => padL + ((day - 1) / Math.max(1, daysInMonth - 1)) * innerW;
    const yForVal = (v: number) => padT + (1 - v / yMax) * innerH;
    return toSvgPath(
      expectedCurve.map((value, index) => ({ day: index + 1, value })),
      xForDay,
      yForVal,
    );
  }, [expectedCurve, target, daysInMonth, innerW, innerH, padL, padT, yMax]);

  const forecast = pace?.projected ?? 0;
  const headerAction = (
    <BudgetCardHeaderActions
      monthLabel={monthMeta.shortLabel}
      monthKey={monthKey}
      onPreviousMonth={onPreviousMonth}
      onNextMonth={onNextMonth}
      canGoNextMonth={canGoNextMonth}
    />
  );

  if (target <= 0) {
    return (
      <DashboardCard
        title={t("spending:budgetChart.monthlyBudget")}
        subtitle={monthMeta.shortLabel}
        action={headerAction}
        className="text-center"
      >
        <p className="text-muted-foreground text-sm">{t("spending:budgetChart.noTarget")}</p>
        <Link
          to={`/spending/budget?month=${monthKey}`}
          className="text-foreground mt-2 inline-flex text-xs underline-offset-4 hover:underline"
        >
          {t("spending:budgetChart.setBudget")}
        </Link>
      </DashboardCard>
    );
  }

  const remaining = Math.max(0, target - spent);
  const overBy = spent - target;
  const status: Status = pace ? STATUS_FROM_PACE[pace.status] : "ok";
  const isOver = status === "over";
  const forecastReliable = live && (pace?.projectionReliable ?? false);
  const forecastDelta = forecast - target;
  const willOverspend = forecastReliable && forecastDelta > 0;

  const gapVsPace = spent - (pace?.expectedToDate ?? 0);
  const aheadOfPace = gapVsPace <= 0;

  const a = STATUS_ACCENTS[status];
  const { Icon } = a;
  const statusLabel = !live && !isOver ? t("spending:budgetChart.underBudget") : t(a.labelKey);

  const xForDay = (day: number) => padL + ((day - 1) / Math.max(1, daysInMonth - 1)) * innerW;
  const yForVal = (v: number) => padT + (1 - v / yMax) * innerH;

  const paceX1 = xForDay(1);
  const paceY1 = yForVal(0);
  const paceX2 = xForDay(daysInMonth);
  const paceY2 = yForVal(target);

  const endX = cumulative.length ? xForDay(cumulative[cumulative.length - 1].day) : padL;
  const endY = cumulative.length ? yForVal(cumulative[cumulative.length - 1].value) : padT + innerH;

  const gapAbs = Math.abs(gapVsPace);
  const gapLabel = live
    ? isOver
      ? t("spending:budgetChart.overBudgetAmount", {
          amount: amountFormatting.formatCompactAmount(overBy, currency),
        })
      : aheadOfPace
        ? t("spending:budgetChart.underBudgetAmount", {
            amount: amountFormatting.formatCompactAmount(gapAbs, currency),
          })
        : t("spending:budgetChart.overPaceAmount", {
            amount: amountFormatting.formatCompactAmount(gapAbs, currency),
          })
    : isOver
      ? t("spending:budgetChart.overBudgetAmount", {
          amount: amountFormatting.formatCompactAmount(overBy, currency),
        })
      : t("spending:budgetChart.leftAmount", {
          amount: amountFormatting.formatCompactAmount(remaining, currency),
        });

  const pillLeftPctRaw = (endX / chartW) * 100;
  const pillLeftPct = Math.min(78, Math.max(8, pillLeftPctRaw - 4));
  // Near the right edge, anchor the badge to the endpoint and grow leftward so
  // it never overflows the card.
  const pillFlip = pillLeftPctRaw > 55;
  const pillTopPx = Math.max(0, endY - 28);

  return (
    <DashboardCard
      title={t("spending:budgetChart.monthlyBudget")}
      subtitle={monthLabel}
      action={headerAction}
    >
      <div className="flex items-center gap-2">
        <Icon className="h-4 w-4 shrink-0" style={{ color: a.accent }} />
        <span className="text-foreground text-sm font-semibold">{statusLabel}</span>
        <span className="text-muted-foreground/70 ml-auto text-xs tabular-nums">
          {live
            ? t("spending:budgetChart.dayOf", { day: dayOfMonth, total: daysInMonth })
            : t("spending:budgetChart.closed")}
        </span>
      </div>

      <div className="mt-3">
        {live && willOverspend && forecastDelta > target * 0.05 ? (
          <>
            <div className="text-foreground text-2xl font-bold tabular-nums tracking-tight">
              <PrivacyAmount value={forecast} currency={currency} />{" "}
              <span className="text-muted-foreground/70 text-base font-medium">
                {t("spending:budgetChart.forecastLower")}
              </span>
            </div>
            <div className="text-destructive mt-0.5 inline-flex items-center gap-1 text-xs font-semibold tabular-nums">
              <Icons.ArrowUp className="h-3 w-3" />
              <PrivacyAmount value={forecastDelta} currency={currency} />{" "}
              {t("spending:budgetChart.overBudgetLower")}
            </div>
            <div className="text-muted-foreground/80 mt-0.5 text-xs tabular-nums">
              <PrivacyAmount value={remaining} currency={currency} />{" "}
              {t("spending:budgetChart.leftTodayOf")}{" "}
              <PrivacyAmount value={target} currency={currency} />{" "}
              {t("spending:budgetChart.budgetedThisMonth")}
            </div>
          </>
        ) : !live ? (
          <>
            <div className="text-foreground text-2xl font-bold tabular-nums tracking-tight">
              <PrivacyAmount value={spent} currency={currency} />{" "}
              <span className="text-muted-foreground/70 text-base font-medium">
                {t("spending:budgetChart.spentLower")}
              </span>
            </div>
            <div
              className={cn(
                "mt-0.5 inline-flex items-center gap-1 text-xs font-semibold tabular-nums",
                isOver ? "text-destructive" : "text-success",
              )}
            >
              <PrivacyAmount value={isOver ? overBy : remaining} currency={currency} />{" "}
              {isOver
                ? t("spending:budgetChart.overBudgetLower")
                : t("spending:budgetChart.leftLower")}
            </div>
            <div className="text-muted-foreground/80 mt-0.5 text-xs tabular-nums">
              {t("spending:budgetChart.ofLower")}{" "}
              <PrivacyAmount value={target} currency={currency} />{" "}
              {t("spending:budgetChart.budgetedLower")}
            </div>
          </>
        ) : (
          <>
            <div className="text-foreground text-2xl font-bold tabular-nums tracking-tight">
              <PrivacyAmount value={isOver ? overBy : remaining} currency={currency} />{" "}
              <span className="text-muted-foreground/70 text-base font-medium">
                {isOver ? t("spending:budgetChart.overLower") : t("spending:budgetChart.leftLower")}
              </span>
            </div>
            <div className="text-muted-foreground/80 text-xs tabular-nums">
              {t("spending:budgetChart.ofLower")}{" "}
              <PrivacyAmount value={target} currency={currency} />{" "}
              {t("spending:budgetChart.budgetedThisMonth")}
            </div>
          </>
        )}
      </div>

      <div className="relative mt-4 w-full">
        <svg
          viewBox={`0 0 ${chartW} ${chartH}`}
          preserveAspectRatio="none"
          className="block h-[110px] w-full"
        >
          {targetPacePath ? (
            <path
              d={targetPacePath}
              fill="none"
              stroke="var(--muted-foreground)"
              strokeOpacity={0.35}
              strokeDasharray="3 4"
              strokeWidth={1.25}
              strokeLinecap="round"
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
            />
          ) : (
            <line
              x1={paceX1}
              y1={paceY1}
              x2={paceX2}
              y2={paceY2}
              stroke="var(--muted-foreground)"
              strokeOpacity={0.35}
              strokeDasharray="3 4"
              strokeWidth={1.25}
              vectorEffect="non-scaling-stroke"
            />
          )}
          {actualPath && (
            <path
              d={actualPath}
              fill="none"
              stroke={a.lineColor}
              strokeWidth={2.5}
              strokeLinecap="round"
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
            />
          )}
        </svg>
        {cumulative.length > 0 && (
          // Rendered as HTML rather than an SVG <circle> so it stays round: the
          // SVG uses preserveAspectRatio="none", which would stretch a circle
          // into an ellipse.
          <div
            className="absolute h-[9px] w-[9px] rounded-full bg-white"
            style={{
              left: `${pillLeftPctRaw}%`,
              top: `${endY}px`,
              transform: "translate(-50%, -50%)",
              border: `2.5px solid ${a.lineColor}`,
            }}
          />
        )}
        {cumulative.length > 0 && (
          <div
            className="absolute whitespace-nowrap rounded-full px-2 py-0.5 text-[11px] font-semibold tabular-nums shadow-sm"
            style={{
              left: `${pillFlip ? pillLeftPctRaw : pillLeftPct}%`,
              top: `${pillTopPx}px`,
              transform: pillFlip ? "translateX(calc(-100% - 6px))" : undefined,
              backgroundColor: a.pillBg,
              color: "white",
            }}
          >
            {gapLabel}
          </div>
        )}
      </div>
      <div className="text-muted-foreground/70 mt-1 flex justify-between text-[10px] tabular-nums">
        <span>{t("spending:budgetChart.dayN", { day: 1 })}</span>
        <span>{t("spending:budgetChart.dayN", { day: daysInMonth })}</span>
      </div>

      <div className="border-border mt-4 grid grid-cols-2 gap-3 border-t pt-3 text-xs">
        <div>
          <div className="text-muted-foreground/70 text-[11px] uppercase tracking-wide">
            {live ? t("spending:budgetChart.spentSoFar") : t("spending:budgetChart.spentUpper")}
          </div>
          <div className="text-foreground text-sm font-semibold tabular-nums">
            <PrivacyAmount value={spent} currency={currency} />
          </div>
        </div>
        <div className="text-right">
          <div className="text-muted-foreground/70 text-[11px] uppercase tracking-wide">
            {live ? t("spending:budgetChart.forecastUpper") : t("spending:budgetChart.result")}
          </div>
          {live ? (
            <div
              className={cn(
                "text-sm font-semibold tabular-nums",
                forecastReliable
                  ? willOverspend
                    ? "text-destructive"
                    : "text-foreground"
                  : "text-muted-foreground/60",
              )}
            >
              {forecastReliable ? <PrivacyAmount value={forecast} currency={currency} /> : "—"}
            </div>
          ) : (
            <div
              className={cn(
                "text-sm font-semibold tabular-nums",
                isOver ? "text-destructive" : "text-foreground",
              )}
            >
              <PrivacyAmount value={isOver ? overBy : remaining} currency={currency} />
            </div>
          )}
          <div className="text-muted-foreground/60 text-[10px]">
            {live
              ? forecastReliable
                ? t("spending:budgetChart.atCurrentPace")
                : t("spending:budgetChart.moreDataNeeded")
              : isOver
                ? t("spending:budgetChart.overBudgetLower")
                : t("spending:budgetChart.leftLower")}
          </div>
        </div>
      </div>

      <div className="border-border mt-5 border-t pt-4">
        <div className="mb-3 flex items-center justify-between">
          <span className="text-muted-foreground/80 text-[11px] font-semibold uppercase tracking-wide">
            {t("spending:budgetChart.byCategory")}
          </span>
          <BudgetManageLink monthKey={monthKey} />
        </div>
        {rings.length === 0 ? (
          <div className="text-muted-foreground py-2 text-center text-xs">
            {t("spending:budgetBars.noBudgets")}{" "}
            <Link
              to="/settings/spending/setup"
              className="hover:text-foreground underline-offset-4 hover:underline"
            >
              {t("spending:budgetBars.setOne")}
            </Link>
          </div>
        ) : (
          <div
            data-no-swipe-drag
            className="-mx-1 flex min-w-0 touch-pan-x gap-3 !overflow-x-auto overscroll-x-contain px-1 pb-1 [-webkit-overflow-scrolling:touch] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
            style={{
              maskImage: "linear-gradient(to right, black calc(100% - 32px), transparent 100%)",
              WebkitMaskImage:
                "linear-gradient(to right, black calc(100% - 32px), transparent 100%)",
            }}
          >
            {rings.map((r) => (
              <BudgetRing key={r.id} ring={r} currency={currency} activityRange={activityRange} />
            ))}
          </div>
        )}
      </div>
    </DashboardCard>
  );
}

function toSvgPath(
  points: PacePoint[],
  xForDay: (day: number) => number,
  yForVal: (value: number) => number,
): string {
  if (!points.length) return "";
  return (
    "M " +
    points.map((p) => `${xForDay(p.day).toFixed(2)} ${yForVal(p.value).toFixed(2)}`).join(" L ")
  );
}

function BudgetCardHeaderActions({
  monthLabel,
  monthKey,
  onPreviousMonth,
  onNextMonth,
  canGoNextMonth,
}: {
  monthLabel: string;
  monthKey: string;
  onPreviousMonth: () => void;
  onNextMonth: () => void;
  canGoNextMonth: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-1.5">
      <div className="bg-muted/60 inline-flex items-center rounded-full p-0.5">
        <button
          type="button"
          onClick={onPreviousMonth}
          className="hover:bg-background flex h-6 w-6 items-center justify-center rounded-full transition-colors"
          aria-label={t("spending:budgetChart.previousBudgetMonth")}
        >
          <Icons.ChevronLeft className="h-3.5 w-3.5" />
        </button>
        <span className="text-foreground min-w-[74px] px-1 text-center text-[11px] font-medium tabular-nums">
          {monthLabel}
        </span>
        <button
          type="button"
          onClick={onNextMonth}
          disabled={!canGoNextMonth}
          className="hover:bg-background disabled:text-muted-foreground/40 flex h-6 w-6 items-center justify-center rounded-full transition-colors disabled:cursor-not-allowed"
          aria-label={t("spending:budgetChart.nextBudgetMonth")}
        >
          <Icons.ChevronRight className="h-3.5 w-3.5" />
        </button>
      </div>
      <BudgetManageLink monthKey={monthKey} />
    </div>
  );
}

const BudgetManageLink = ({ monthKey }: { monthKey: string }) => {
  const { t } = useTranslation();
  return (
    <Link
      to={`/spending/budget?month=${monthKey}`}
      className="text-muted-foreground hover:text-foreground text-xs underline-offset-4 hover:underline"
    >
      {t("spending:budgetChart.manage")}
    </Link>
  );
};

function BudgetRing({
  ring,
  currency,
  activityRange,
}: {
  ring: {
    categoryId: string;
    name: string;
    color: string | null;
    icon: string | null;
    target: number;
    spent: number;
    /** Target + rollover carried in − spent, as the budget snapshot reports it. */
    remaining: number;
    pct: number;
    status: PaceStatus;
    /** Set while a once-a-month category's due day is still ahead. */
    dueDay: number | null;
  };
  currency: string;
  activityRange: { from: string; to: string };
}) {
  const formatting = useAmountFormatting();
  const { t } = useTranslation();
  const { isBalanceHidden } = useBalancePrivacy();
  const isOver = ring.status === "over";
  const ringColor = isOver
    ? "var(--destructive)"
    : ring.status === "approaching"
      ? "#C28B47"
      : "var(--success)";
  const displayAmount = Math.abs(ring.remaining);

  const size = 56;
  const stroke = 4;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const fillPct = Math.min(1, ring.pct);
  const dash = `${c * fillPct} ${c}`;

  return (
    <Link
      to={`/activities?tab=spending&category=${encodeURIComponent(ring.categoryId)}&from=${
        activityRange.from
      }&to=${activityRange.to}`}
      className="hover:bg-muted/40 flex w-16 shrink-0 flex-col items-center gap-1 rounded-md px-1 py-1 transition-colors"
      title={`${ring.name}: ${ring.spent.toFixed(2)} / ${ring.target.toFixed(2)}`}
    >
      <div className="relative" style={{ width: size, height: size }}>
        <svg width={size} height={size} className="-rotate-90">
          <circle
            cx={size / 2}
            cy={size / 2}
            r={r}
            fill="none"
            stroke={ringColor}
            strokeOpacity={0.22}
            strokeWidth={stroke}
          />
          <circle
            cx={size / 2}
            cy={size / 2}
            r={r}
            fill="none"
            stroke={ringColor}
            strokeWidth={stroke}
            strokeLinecap="round"
            strokeDasharray={dash}
          />
        </svg>
        <div
          className="absolute inset-0 flex items-center justify-center"
          style={{ color: ring.color ?? ringColor }}
        >
          <CategoryIcon icon={ring.icon} fallback={ring.name} className="h-5 w-5" />
        </div>
      </div>
      <div className="text-foreground text-xs font-semibold tabular-nums">
        {isBalanceHidden ? "••••" : formatting.formatCompactAmount(displayAmount, currency)}
      </div>
      <div
        className={cn(
          "text-[10px] uppercase tracking-wide",
          isOver ? "text-destructive" : "text-muted-foreground/70",
        )}
      >
        {isOver
          ? t("spending:budgetChart.overLower")
          : ring.dueDay !== null
            ? t("spending:budgetChart.dueOn", { day: ring.dueDay })
            : t("spending:budgetChart.leftLower")}
      </div>
    </Link>
  );
}
