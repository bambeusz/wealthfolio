export type BudgetTargetType = "category" | "group_buffer";
export type BudgetRolloverTargetType = "category" | "group";
/** `linear`: spread over the month; `monthly_on_day`: spent once, on `dueDay`. */
export type BudgetPacing = "linear" | "monthly_on_day";
/** The one on-track status, computed by the backend (`budget::pacing`). */
export type PaceStatus = "on_track" | "approaching" | "over";

export interface BudgetGroup {
  id: string;
  name: string;
  key: string;
  color: string | null;
  icon: string | null;
  sortOrder: number;
  isSystem: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface NewBudgetGroup {
  id?: string;
  name: string;
  key?: string;
  color?: string | null;
  icon?: string | null;
  sortOrder?: number;
  isSystem?: boolean;
}

export interface UpdateBudgetGroup {
  name?: string;
  color?: string | null;
  icon?: string | null;
  sortOrder?: number;
}

export interface BudgetGroupAssignment {
  id: string;
  groupId: string;
  taxonomyId: string;
  categoryId: string;
  createdAt: string;
  updatedAt: string;
}

export interface BudgetTarget {
  id: string;
  periodKey: string;
  targetType: BudgetTargetType;
  taxonomyId: string | null;
  categoryId: string | null;
  groupId: string | null;
  amount: string;
  pacing: BudgetPacing;
  dueDay: number | null;
  createdAt: string;
  updatedAt: string;
}

export interface NewBudgetTarget {
  id?: string;
  periodKey: string;
  targetType: BudgetTargetType;
  taxonomyId?: string | null;
  categoryId?: string | null;
  groupId?: string | null;
  amount: string;
  /** Omit to keep the row's pacing (a new month override inherits the default's). */
  pacing?: BudgetPacing;
  dueDay?: number | null;
}

/** One target in a bulk write for a single period (`set_budget_targets`). */
export interface BudgetTargetInput {
  targetType: BudgetTargetType;
  taxonomyId?: string | null;
  categoryId?: string | null;
  groupId?: string | null;
  amount: string;
  pacing?: BudgetPacing;
  dueDay?: number | null;
}

export interface BudgetRolloverSetting {
  id: string;
  targetType: BudgetRolloverTargetType;
  taxonomyId: string | null;
  categoryId: string | null;
  groupId: string | null;
  enabled: boolean;
  startMonth: string;
  startingBalance: string;
  createdAt: string;
  updatedAt: string;
}

export interface NewBudgetRolloverSetting {
  id?: string;
  targetType: BudgetRolloverTargetType;
  taxonomyId?: string | null;
  categoryId?: string | null;
  groupId?: string | null;
  enabled?: boolean;
  startMonth: string;
  startingBalance: string;
}

export interface BudgetCategoryRow {
  taxonomyId: string;
  categoryId: string;
  groupId: string | null;
  parentId: string | null;
  name: string;
  color: string | null;
  icon: string | null;
  target: number;
  actual: number;
  rolloverIn: number;
  rolloverOut: number;
  remaining: number;
  overspent: boolean;
  hasDefaultTarget: boolean;
  hasMonthOverride: boolean;
  rolloverEnabled: boolean;
  pacing: BudgetPacing;
  dueDay: number | null;
  /** Pace of a spending row in a month view; null for the default period and income rows. */
  expectedToDate: number | null;
  projected: number | null;
  paceStatus: PaceStatus | null;
}

/** Month-level pace; every surface reads its status from here (or the insight's). */
export interface BudgetPace {
  totalDays: number;
  elapsedDays: number;
  live: boolean;
  available: number;
  spent: number;
  expectedToDate: number;
  fixedExpectedToDate: number;
  flexibleExpectedToDate: number;
  projected: number;
  projectionReliable: boolean;
  flexibleDailyRate: number;
  status: PaceStatus;
  curveSource: "history" | "linear";
  /** Expected cumulative spend at the end of day d, at index d - 1. */
  expectedCurve: number[];
  /** Actual cumulative spend at the end of each elapsed day. */
  spentCurve: number[];
}

export interface BudgetGroupRow {
  group: BudgetGroup;
  categoryTargetTotal: number;
  buffer: number;
  plannedTotal: number;
  actual: number;
  rolloverIn: number;
  rolloverOut: number;
  remaining: number;
  overspent: boolean;
  rolloverEnabled: boolean;
  categories: BudgetCategoryRow[];
}

export interface BudgetTotals {
  spendingPlanned: number;
  spendingActual: number;
  spendingRemaining: number;
  incomePlanned: number;
  incomeActual: number;
  groupBuffer: number;
  rolloverIn: number;
  rolloverOut: number;
  overspentCount: number;
}

export interface BudgetSnapshot {
  state: {
    groups: BudgetGroup[];
    groupAssignments: BudgetGroupAssignment[];
    targets: BudgetTarget[];
    rolloverSettings: BudgetRolloverSetting[];
  };
  computed: {
    currency: string;
    periodKey: string;
    fxAsOf: string | null;
    groupRows: BudgetGroupRow[];
    ungroupedRows: BudgetCategoryRow[];
    incomeRows: BudgetCategoryRow[];
    totals: BudgetTotals;
    /** Null for the default period. */
    pace: BudgetPace | null;
  };
}
