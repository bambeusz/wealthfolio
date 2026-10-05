import { describe, expect, it } from "vitest";

import {
  accountInAggregateScope,
  getCashCategoryFromMeta,
  getPortfolioFlagsFromMeta,
  setCashCategoryInMeta,
  setPortfolioFlagsInMeta,
} from "./account-meta";
import { AccountPurpose } from "./constants";

describe("getPortfolioFlagsFromMeta", () => {
  it("treats missing or unusable meta as included", () => {
    for (const meta of [undefined, null, "", "not json", "[]", "{}", '{"portfolio":{}}']) {
      expect(getPortfolioFlagsFromMeta(meta)).toEqual({
        includeInNetWorth: true,
        includeInPerformance: true,
      });
    }
  });

  it("reads each flag independently", () => {
    expect(getPortfolioFlagsFromMeta('{"portfolio":{"includeInNetWorth":false}}')).toEqual({
      includeInNetWorth: false,
      includeInPerformance: true,
    });
    expect(getPortfolioFlagsFromMeta('{"portfolio":{"includeInPerformance":false}}')).toEqual({
      includeInNetWorth: true,
      includeInPerformance: false,
    });
  });
});

describe("setPortfolioFlagsInMeta", () => {
  it("keeps allocation.cashCategoryId and other keys when a flag is switched off", () => {
    const meta = '{"allocation":{"cashCategoryId":"FIXED_INCOME"},"accountingSettings":{"x":1}}';
    const updated = setPortfolioFlagsInMeta(meta, { includeInNetWorth: false });

    expect(JSON.parse(updated)).toEqual({
      allocation: { cashCategoryId: "FIXED_INCOME" },
      accountingSettings: { x: 1 },
      portfolio: { includeInNetWorth: false },
    });
    expect(getCashCategoryFromMeta(updated)).toBe("FIXED_INCOME");
  });

  it("merges with the other flag and drops flags that are back to the default", () => {
    const off = setPortfolioFlagsInMeta(null, { includeInPerformance: false });
    expect(JSON.parse(off)).toEqual({ portfolio: { includeInPerformance: false } });

    const both = setPortfolioFlagsInMeta(off, { includeInNetWorth: false });
    expect(getPortfolioFlagsFromMeta(both)).toEqual({
      includeInNetWorth: false,
      includeInPerformance: false,
    });

    const restored = setPortfolioFlagsInMeta(both, {
      includeInNetWorth: true,
      includeInPerformance: true,
    });
    expect(JSON.parse(restored)).toEqual({});
  });

  it("is preserved by the cash category setter", () => {
    const meta = setPortfolioFlagsInMeta(null, { includeInNetWorth: false });
    const withCategory = setCashCategoryInMeta(meta, "FIXED_INCOME");
    expect(JSON.parse(withCategory)).toEqual({
      portfolio: { includeInNetWorth: false },
      allocation: { cashCategoryId: "FIXED_INCOME" },
    });
    expect(JSON.parse(setCashCategoryInMeta(withCategory, null))).toEqual({
      portfolio: { includeInNetWorth: false },
    });
  });
});

describe("accountInAggregateScope", () => {
  const netWorthOff = { meta: '{"portfolio":{"includeInNetWorth":false}}' };
  const performanceOff = { meta: '{"portfolio":{"includeInPerformance":false}}' };

  it("uses the net worth flag for net worth and holdings", () => {
    for (const purpose of [AccountPurpose.NET_WORTH, AccountPurpose.HOLDINGS]) {
      expect(accountInAggregateScope(netWorthOff, purpose)).toBe(false);
      expect(accountInAggregateScope(performanceOff, purpose)).toBe(true);
    }
  });

  it("uses the performance flag for performance and income", () => {
    for (const purpose of [AccountPurpose.PERFORMANCE, AccountPurpose.INCOME]) {
      expect(accountInAggregateScope(performanceOff, purpose)).toBe(false);
      expect(accountInAggregateScope(netWorthOff, purpose)).toBe(true);
    }
  });

  it("never excludes for spending", () => {
    expect(accountInAggregateScope(netWorthOff, AccountPurpose.SPENDING)).toBe(true);
    expect(accountInAggregateScope({}, AccountPurpose.NET_WORTH)).toBe(true);
  });
});
