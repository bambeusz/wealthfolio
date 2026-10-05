import { AccountPurpose } from "@/lib/constants";

/**
 * Helpers for the account `meta` JSON string. Every writer parses the existing object and
 * only touches its own key, so unrelated keys (allocation, accountingSettings, ...) survive.
 *
 * `meta.portfolio.{includeInNetWorth, includeInPerformance}` mirror the Rust accessors on
 * `Account`: a missing or non-boolean value means `true`.
 */

export interface AccountPortfolioFlags {
  includeInNetWorth: boolean;
  includeInPerformance: boolean;
}

function parseMeta(meta?: string | null): Record<string, unknown> {
  if (!meta) return {};
  try {
    const parsed: unknown = JSON.parse(meta);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
}

export function getCashCategoryFromMeta(meta?: string | null): string | null {
  const allocation = parseMeta(meta).allocation as Record<string, unknown> | undefined;
  return (allocation?.cashCategoryId as string) ?? null;
}

export function setCashCategoryInMeta(
  meta: string | null | undefined,
  categoryId: string | null,
): string {
  const parsed = parseMeta(meta);
  if (categoryId) {
    parsed.allocation = { cashCategoryId: categoryId };
  } else {
    delete parsed.allocation;
  }
  return JSON.stringify(parsed);
}

export function getPortfolioFlagsFromMeta(meta?: string | null): AccountPortfolioFlags {
  const portfolio = parseMeta(meta).portfolio as Record<string, unknown> | undefined;
  return {
    includeInNetWorth: portfolio?.includeInNetWorth !== false,
    includeInPerformance: portfolio?.includeInPerformance !== false,
  };
}

/**
 * Merges the flags into `meta.portfolio`. Flags that are back at their default (true) are
 * dropped so an untouched account keeps a meta identical to what it had before.
 */
export function setPortfolioFlagsInMeta(
  meta: string | null | undefined,
  flags: Partial<AccountPortfolioFlags>,
): string {
  const parsed = parseMeta(meta);
  const current = { ...getPortfolioFlagsFromMeta(meta), ...flags };
  const existing = (parsed.portfolio as Record<string, unknown> | undefined) ?? {};
  const portfolio: Record<string, unknown> = { ...existing };
  for (const key of ["includeInNetWorth", "includeInPerformance"] as const) {
    if (current[key]) {
      delete portfolio[key];
    } else {
      portfolio[key] = false;
    }
  }
  if (Object.keys(portfolio).length > 0) {
    parsed.portfolio = portfolio;
  } else {
    delete parsed.portfolio;
  }
  return JSON.stringify(parsed);
}

/**
 * Whether an account counts in a multi-account aggregate for `purpose` (same rule as the
 * backend `account_in_aggregate_scope`). Single-account views must not apply it.
 */
export function accountInAggregateScope(
  account: { meta?: string | null },
  purpose: AccountPurpose,
): boolean {
  const flags = getPortfolioFlagsFromMeta(account.meta);
  switch (purpose) {
    case AccountPurpose.NET_WORTH:
    case AccountPurpose.HOLDINGS:
      return flags.includeInNetWorth;
    case AccountPurpose.PERFORMANCE:
    case AccountPurpose.INCOME:
      return flags.includeInPerformance;
    default:
      return true;
  }
}
