//! Budget pacing — the single rule for "how much should be spent by today",
//! "where will the month end" and "is that on track".
//!
//! Every surface that shows an on-track status reads it from here: the budget
//! snapshot (dashboard budget card, category rings, budget editor) and the
//! insight payload (Insights page, backend health status). Callers aggregate
//! spend with their own classifier and hand plain amounts in, so this module
//! stays pure and the status cannot drift between surfaces.
//!
//! Status rule, for a category row or a whole window:
//! - `Over` when spent > available;
//! - otherwise `Approaching` while the window is live and either spent is ahead
//!   of the expected-to-date line, or the projection is reliable (≥
//!   `PROJECTION_MIN_DAYS` elapsed) and ends above available;
//! - otherwise `OnTrack`.
//!
//! Expected-to-date follows each category's pacing: a `MonthlyOnDay` target
//! is a step (nothing before its due day, all of it from the due day on), a
//! `Linear` target follows the historical spending curve, or day/days when
//! there is too little history. A bill paid on its due day is therefore on
//! track, not "ahead of pace".

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use super::model::BudgetPacing;

/// Days of a live window before a run-rate projection may flag `Approaching`;
/// earlier, a single purchase would extrapolate to an absurd month total.
pub const PROJECTION_MIN_DAYS: u32 = 7;
/// Days the flexible run rate is averaged over.
pub const TRAILING_WINDOW_DAYS: u32 = 7;
/// Months before the budget month that feed the historical curve.
pub const HISTORY_MONTHS: u32 = 3;
/// Months with spending needed before the historical curve replaces day/days.
pub const MIN_HISTORY_MONTHS: usize = 2;

/// Amounts are money; anything under half a cent is noise from f64 sums.
const EPSILON: f64 = 0.005;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceStatus {
    OnTrack,
    Approaching,
    Over,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceCurveSource {
    /// Median shape of recent months' flexible spending.
    History,
    /// Even spread, day / days.
    Linear,
}

/// A category's resolved pacing. A `MonthlyOnDay` row without a due day
/// (rejected on write, but possible in a hand-edited database) paces linearly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacingRule {
    pub pacing: BudgetPacing,
    pub due_day: Option<u32>,
}

impl PacingRule {
    pub const LINEAR: Self = Self {
        pacing: BudgetPacing::Linear,
        due_day: None,
    };

    pub fn new(pacing: BudgetPacing, due_day: Option<i32>) -> Self {
        match (pacing, due_day) {
            (BudgetPacing::MonthlyOnDay, Some(day)) if (1..=31).contains(&day) => Self {
                pacing,
                due_day: Some(day as u32),
            },
            _ => Self::LINEAR,
        }
    }

    pub fn is_fixed(&self) -> bool {
        self.pacing == BudgetPacing::MonthlyOnDay && self.due_day.is_some()
    }

    /// Due date in the given month, with the due day clamped to the month's
    /// length (due 31 falls on the 30th in a 30-day month).
    pub fn due_date_in_month(&self, year: i32, month: u32) -> Option<NaiveDate> {
        if !self.is_fixed() {
            return None;
        }
        let day = self.due_day?.min(days_in_month(year, month)).max(1);
        NaiveDate::from_ymd_opt(year, month, day)
    }
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    match (
        NaiveDate::from_ymd_opt(year, month, 1),
        NaiveDate::from_ymd_opt(next_year, next_month, 1),
    ) {
        (Some(start), Some(next)) => (next - start).num_days() as u32,
        _ => 30,
    }
}

/// Where "today" sits in a window of whole local days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaceClock {
    pub total_days: u32,
    /// Days of the window up to and including today (0 for a future window,
    /// `total_days` for a closed one).
    pub elapsed_days: u32,
    /// Today falls inside the window.
    pub live: bool,
}

impl PaceClock {
    pub fn for_window(start: NaiveDate, end: NaiveDate, today: NaiveDate) -> Self {
        let total_days = ((end - start).num_days() + 1).max(0) as u32;
        if today < start {
            Self {
                total_days,
                elapsed_days: 0,
                live: false,
            }
        } else if today > end {
            Self {
                total_days,
                elapsed_days: total_days,
                live: false,
            }
        } else {
            Self {
                total_days,
                elapsed_days: ((today - start).num_days() + 1) as u32,
                live: true,
            }
        }
    }

    pub fn for_month(year: i32, month: u32, today: NaiveDate) -> Option<Self> {
        let start = NaiveDate::from_ymd_opt(year, month, 1)?;
        let end = NaiveDate::from_ymd_opt(year, month, days_in_month(year, month))?;
        Some(Self::for_window(start, end, today))
    }

    pub fn days_remaining(&self) -> u32 {
        if self.live {
            self.total_days.saturating_sub(self.elapsed_days)
        } else {
            0
        }
    }

    pub fn projection_reliable(&self) -> bool {
        self.live && self.elapsed_days >= PROJECTION_MIN_DAYS
    }
}

/// Cumulative share of a window's flexible budget expected by the end of each
/// day: `fraction(0) == 0`, `fraction(total_days) == 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct PaceCurve {
    fractions: Vec<f64>,
    source: PaceCurveSource,
}

impl PaceCurve {
    pub fn linear(total_days: u32) -> Self {
        let total = total_days.max(1) as f64;
        Self {
            fractions: (0..=total_days).map(|day| day as f64 / total).collect(),
            source: PaceCurveSource::Linear,
        }
    }

    pub fn total_days(&self) -> u32 {
        (self.fractions.len().max(1) - 1) as u32
    }

    pub fn fraction(&self, day: u32) -> f64 {
        let last = self.fractions.len().saturating_sub(1);
        self.fractions
            .get((day as usize).min(last))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn source(&self) -> PaceCurveSource {
        self.source
    }
}

/// One past month of flexible spending, `daily[d - 1]` = spend on day `d`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HistoryMonth {
    pub daily: Vec<f64>,
}

/// Median cumulative-spending shape of recent months, rescaled onto a month of
/// `total_days`. Built from flexible (linear) spending only — fixed bills have
/// their own step and would otherwise pull the curve to their due days.
/// Returns `None` with fewer than `MIN_HISTORY_MONTHS` months that had spend.
pub fn historical_curve(months: &[HistoryMonth], total_days: u32) -> Option<PaceCurve> {
    if total_days == 0 {
        return None;
    }
    let eligible: Vec<(usize, Vec<f64>, f64)> = months
        .iter()
        .filter_map(|month| {
            let total: f64 = month.daily.iter().sum();
            if total <= EPSILON || month.daily.is_empty() {
                return None;
            }
            let days = month.daily.len();
            let mut cumulative = vec![0.0_f64; days + 1];
            let mut running = 0.0_f64;
            for day in 1..=days {
                running += month.daily[day - 1];
                cumulative[day] = cumulative[day - 1].max(running.clamp(0.0, total));
            }
            Some((days, cumulative, total))
        })
        .collect();
    if eligible.len() < MIN_HISTORY_MONTHS {
        return None;
    }

    let total = total_days as usize;
    let mut fractions = vec![0.0; total + 1];
    for (day, fraction) in fractions.iter_mut().enumerate().skip(1) {
        let values: Vec<f64> = eligible
            .iter()
            .map(|(days, cumulative, month_total)| {
                // Same relative position in a month of a different length.
                let history_day = ((day * days).div_ceil(total)).clamp(1, *days);
                (cumulative[history_day] / month_total).clamp(0.0, 1.0)
            })
            .collect();
        *fraction = median(values);
    }
    Some(PaceCurve {
        fractions,
        source: PaceCurveSource::History,
    })
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    }
}

/// Average spend per day over the last `TRAILING_WINDOW_DAYS` elapsed days.
/// `daily[d - 1]` = spend on day `d` of the window. Refund-heavy windows clamp
/// to zero: refunds don't predict negative spending.
pub fn trailing_daily_rate(daily: &[f64], elapsed_days: u32) -> f64 {
    let elapsed = (elapsed_days as usize).min(daily.len());
    let days = (TRAILING_WINDOW_DAYS as usize).min(elapsed);
    if days == 0 {
        return 0.0;
    }
    let sum: f64 = daily[elapsed - days..elapsed].iter().sum();
    (sum / days as f64).max(0.0)
}

/// Expected spend by the end of `day` of a month, for one category's amount.
/// The curve spans that month, so its length is the month's length.
pub fn expected_to_date(rule: PacingRule, amount: f64, day: u32, curve: &PaceCurve) -> f64 {
    match rule.due_day.filter(|_| rule.is_fixed()) {
        Some(due_day) => {
            let due = due_day.min(curve.total_days()).max(1);
            if day >= due {
                amount
            } else {
                0.0
            }
        }
        None => amount * curve.fraction(day),
    }
}

/// Where a category ends the window: a once-a-month charge happens once, so
/// it ends at the larger of what was spent and its target; a linear category
/// keeps its own trailing run rate. Closed windows end at what was spent.
pub fn projected(
    rule: PacingRule,
    spent: f64,
    target: f64,
    trailing_rate: f64,
    clock: PaceClock,
) -> f64 {
    if !clock.live {
        return spent;
    }
    if rule.is_fixed() {
        spent.max(target)
    } else {
        spent + trailing_rate * clock.days_remaining() as f64
    }
}

/// The one on-track rule (see the module docs).
pub fn pace_status(
    spent: f64,
    available: f64,
    expected_to_date: f64,
    projected: f64,
    clock: PaceClock,
) -> PaceStatus {
    if spent > available + EPSILON {
        return PaceStatus::Over;
    }
    let ahead_of_pace = spent > expected_to_date + EPSILON;
    let projected_over = clock.projection_reliable() && projected > available + EPSILON;
    if clock.live && (ahead_of_pace || projected_over) {
        return PaceStatus::Approaching;
    }
    PaceStatus::OnTrack
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinePace {
    pub expected_to_date: f64,
    pub projected: f64,
    pub status: PaceStatus,
}

/// Pace of one budget category in a month. `available` is the target plus
/// rollover carried in; the pacing shape is applied to what is available.
pub fn category_pace(
    rule: PacingRule,
    target: f64,
    rollover_in: f64,
    spent: f64,
    daily: &[f64],
    clock: PaceClock,
    curve: &PaceCurve,
) -> LinePace {
    let available = target + rollover_in;
    let expected = expected_to_date(rule, available.max(0.0), clock.elapsed_days, curve);
    let rate = trailing_daily_rate(daily, clock.elapsed_days);
    let projected = projected(rule, spent, target, rate, clock);
    LinePace {
        expected_to_date: expected,
        projected,
        status: pace_status(spent, available, expected, projected, clock),
    }
}

/// A fixed (`MonthlyOnDay`) category inside a window: its charges as steps on
/// the window's day index (1-based), and what it has spent so far.
#[derive(Debug, Clone, PartialEq)]
pub struct FixedLine {
    pub spent: f64,
    pub steps: Vec<(u32, f64)>,
}

impl FixedLine {
    /// A fixed category in a window that is exactly one calendar month.
    pub fn for_month(rule: PacingRule, target: f64, spent: f64, total_days: u32) -> Self {
        let steps = rule
            .due_day
            .filter(|_| rule.is_fixed())
            .map(|due| vec![(due.min(total_days).max(1), target)])
            .unwrap_or_default();
        Self { spent, steps }
    }

    pub fn target(&self) -> f64 {
        self.steps.iter().map(|(_, amount)| amount).sum()
    }

    fn expected_at(&self, day: u32) -> f64 {
        self.steps
            .iter()
            .filter(|(due, _)| *due <= day)
            .map(|(_, amount)| amount)
            .sum()
    }
}

/// Inputs for the pace of a whole window (a budget month, or an insight
/// window). `flexible_daily` / `spent_daily` index day `d` at `d - 1` and need
/// only cover the elapsed days.
#[derive(Debug, Clone, Copy)]
pub struct WindowPaceInput<'a> {
    pub clock: PaceClock,
    /// Planned spending for the window (fixed + flexible + buffers).
    pub available: f64,
    /// Everything spent in the window, uncategorized included.
    pub spent: f64,
    pub fixed: &'a [FixedLine],
    /// Spend outside fixed categories (linear categories + uncategorized).
    pub flexible_daily: &'a [f64],
    pub spent_daily: &'a [f64],
    /// Historical shape for the flexible part; `None` spreads it evenly.
    pub curve: Option<&'a PaceCurve>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetPace {
    pub total_days: u32,
    pub elapsed_days: u32,
    pub live: bool,
    pub available: f64,
    pub spent: f64,
    pub expected_to_date: f64,
    pub fixed_expected_to_date: f64,
    pub flexible_expected_to_date: f64,
    pub projected: f64,
    pub projection_reliable: bool,
    /// Trailing run rate of flexible spend; fixed bills don't inflate it.
    pub flexible_daily_rate: f64,
    pub status: PaceStatus,
    pub curve_source: PaceCurveSource,
    /// Expected cumulative spend at the end of each day, `[d - 1]` for day `d`.
    pub expected_curve: Vec<f64>,
    /// Actual cumulative spend at the end of each elapsed day.
    pub spent_curve: Vec<f64>,
}

/// Window totals: expected = Σ fixed steps due so far + flexible planned ×
/// curve(day); projected = Σ fixed max(spent, target) + flexible spent +
/// flexible run rate × days remaining.
pub fn window_pace(input: WindowPaceInput<'_>) -> BudgetPace {
    let clock = input.clock;
    let total_days = clock.total_days;
    let fixed_total: f64 = input.fixed.iter().map(FixedLine::target).sum();
    let flexible_planned = (input.available - fixed_total).max(0.0);
    let linear;
    let curve = match input.curve {
        Some(curve) if curve.total_days() == total_days => curve,
        _ => {
            linear = PaceCurve::linear(total_days);
            &linear
        }
    };
    let fixed_expected_at =
        |day: u32| -> f64 { input.fixed.iter().map(|line| line.expected_at(day)).sum() };

    let expected_curve = (1..=total_days)
        .map(|day| fixed_expected_at(day) + flexible_planned * curve.fraction(day))
        .collect();
    let fixed_expected = fixed_expected_at(clock.elapsed_days);
    let flexible_expected = flexible_planned * curve.fraction(clock.elapsed_days);
    let expected = fixed_expected + flexible_expected;

    let fixed_spent: f64 = input.fixed.iter().map(|line| line.spent).sum();
    let flexible_spent = input.spent - fixed_spent;
    let rate = trailing_daily_rate(input.flexible_daily, clock.elapsed_days);
    let projected = if clock.live {
        input
            .fixed
            .iter()
            .map(|line| line.spent.max(line.target()))
            .sum::<f64>()
            + flexible_spent
            + rate * clock.days_remaining() as f64
    } else {
        input.spent
    };

    let mut running = 0.0;
    let spent_curve = (0..clock.elapsed_days as usize)
        .map(|index| {
            running += input.spent_daily.get(index).copied().unwrap_or(0.0);
            running
        })
        .collect();

    BudgetPace {
        total_days,
        elapsed_days: clock.elapsed_days,
        live: clock.live,
        available: input.available,
        spent: input.spent,
        expected_to_date: expected,
        fixed_expected_to_date: fixed_expected,
        flexible_expected_to_date: flexible_expected,
        projected,
        projection_reliable: clock.projection_reliable(),
        flexible_daily_rate: rate,
        status: pace_status(input.spent, input.available, expected, projected, clock),
        curve_source: curve.source(),
        expected_curve,
        spent_curve,
    }
}

/// `(year, month)` of a `YYYY-MM` key shifted by `delta` months.
pub fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let index = year * 12 + month as i32 - 1 + delta;
    (index.div_euclid(12), (index.rem_euclid(12) + 1) as u32)
}

/// The `HISTORY_MONTHS` calendar months before `(year, month)`, oldest first.
pub fn history_months(year: i32, month: u32) -> Vec<(i32, u32)> {
    (1..=HISTORY_MONTHS as i32)
        .rev()
        .map(|back| shift_month(year, month, -back))
        .collect()
}

/// Builds `HistoryMonth`s from dated flexible amounts; dates outside the
/// requested months are ignored.
pub fn history_from_daily(
    months: &[(i32, u32)],
    amounts: impl IntoIterator<Item = (NaiveDate, f64)>,
) -> Vec<HistoryMonth> {
    let mut out: Vec<HistoryMonth> = months
        .iter()
        .map(|(year, month)| HistoryMonth {
            daily: vec![0.0; days_in_month(*year, *month) as usize],
        })
        .collect();
    for (date, amount) in amounts {
        if let Some(index) = months
            .iter()
            .position(|(year, month)| date.year() == *year && date.month() == *month)
        {
            let day = date.day() as usize;
            if let Some(slot) = out[index].daily.get_mut(day - 1) {
                *slot += amount;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn fixed(day: i32) -> PacingRule {
        PacingRule::new(BudgetPacing::MonthlyOnDay, Some(day))
    }

    fn live(total_days: u32, elapsed_days: u32) -> PaceClock {
        PaceClock {
            total_days,
            elapsed_days,
            live: true,
        }
    }

    fn closed(total_days: u32) -> PaceClock {
        PaceClock {
            total_days,
            elapsed_days: total_days,
            live: false,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn rule_without_due_day_or_out_of_range_paces_linearly() {
        assert_eq!(
            PacingRule::new(BudgetPacing::MonthlyOnDay, None),
            PacingRule::LINEAR
        );
        assert_eq!(
            PacingRule::new(BudgetPacing::MonthlyOnDay, Some(0)),
            PacingRule::LINEAR
        );
        assert_eq!(
            PacingRule::new(BudgetPacing::MonthlyOnDay, Some(32)),
            PacingRule::LINEAR
        );
        assert_eq!(
            PacingRule::new(BudgetPacing::Linear, Some(5)),
            PacingRule::LINEAR
        );
        assert!(fixed(5).is_fixed());
    }

    #[test]
    fn clock_covers_live_closed_and_future_windows() {
        assert_eq!(
            PaceClock::for_month(2026, 10, d(2026, 10, 5)).unwrap(),
            live(31, 5)
        );
        assert_eq!(
            PaceClock::for_month(2026, 9, d(2026, 10, 5)).unwrap(),
            closed(30)
        );
        let future = PaceClock::for_month(2026, 11, d(2026, 10, 5)).unwrap();
        assert_eq!(future.elapsed_days, 0);
        assert!(!future.live);
        assert_eq!(live(31, 5).days_remaining(), 26);
        assert_eq!(closed(30).days_remaining(), 0);
        assert!(!live(31, 6).projection_reliable());
        assert!(live(31, 7).projection_reliable());
        assert!(!closed(31).projection_reliable());
    }

    #[test]
    fn expected_to_date_table() {
        let curve31 = PaceCurve::linear(31);
        let curve30 = PaceCurve::linear(30);
        let curve28 = PaceCurve::linear(28);
        // (rule, amount, day, curve, expected)
        let cases: Vec<(PacingRule, f64, u32, &PaceCurve, f64)> = vec![
            // Fixed: nothing before the due day, everything from it on.
            (fixed(10), 1000.0, 9, &curve31, 0.0),
            (fixed(10), 1000.0, 10, &curve31, 1000.0),
            (fixed(10), 1000.0, 25, &curve31, 1000.0),
            (fixed(1), 1000.0, 1, &curve31, 1000.0),
            // Due 31 in a 30-day month falls on the 30th.
            (fixed(31), 900.0, 29, &curve30, 0.0),
            (fixed(31), 900.0, 30, &curve30, 900.0),
            // Due 30 in February falls on the 28th.
            (fixed(30), 50.0, 27, &curve28, 0.0),
            (fixed(30), 50.0, 28, &curve28, 50.0),
            // Linear: day / days without history.
            (PacingRule::LINEAR, 310.0, 10, &curve31, 100.0),
            (PacingRule::LINEAR, 310.0, 0, &curve31, 0.0),
            (PacingRule::LINEAR, 310.0, 31, &curve31, 310.0),
        ];
        for (rule, amount, day, curve, expected) in cases {
            assert!(
                close(expected_to_date(rule, amount, day, curve), expected),
                "{rule:?} {amount} day {day}"
            );
        }
    }

    #[test]
    fn due_date_clamps_to_month_length() {
        assert_eq!(fixed(31).due_date_in_month(2026, 9), Some(d(2026, 9, 30)));
        assert_eq!(fixed(31).due_date_in_month(2026, 2), Some(d(2026, 2, 28)));
        assert_eq!(fixed(31).due_date_in_month(2028, 2), Some(d(2028, 2, 29)));
        assert_eq!(PacingRule::LINEAR.due_date_in_month(2026, 9), None);
    }

    #[test]
    fn status_branches() {
        // (spent, available, expected, projected, clock, status)
        let cases = [
            // Over beats everything, live or closed.
            (101.0, 100.0, 200.0, 0.0, live(30, 3), PaceStatus::Over),
            (101.0, 100.0, 100.0, 101.0, closed(30), PaceStatus::Over),
            // Ahead of the expected line.
            (
                60.0,
                100.0,
                50.0,
                60.0,
                live(30, 15),
                PaceStatus::Approaching,
            ),
            // Behind the line but a reliable projection ends over.
            (
                40.0,
                100.0,
                50.0,
                120.0,
                live(30, 15),
                PaceStatus::Approaching,
            ),
            // Same projection before PROJECTION_MIN_DAYS is ignored.
            (10.0, 100.0, 20.0, 300.0, live(30, 6), PaceStatus::OnTrack),
            // Healthy run.
            (40.0, 100.0, 50.0, 90.0, live(30, 15), PaceStatus::OnTrack),
            // Exactly on the line and exactly at the budget is fine.
            (50.0, 100.0, 50.0, 100.0, live(30, 15), PaceStatus::OnTrack),
            // A closed window is never "approaching".
            (100.0, 100.0, 100.0, 100.0, closed(30), PaceStatus::OnTrack),
            // No budget and some spend is over; no budget and no spend is fine.
            (5.0, 0.0, 0.0, 5.0, live(30, 15), PaceStatus::Over),
            (0.0, 0.0, 0.0, 0.0, live(30, 15), PaceStatus::OnTrack),
        ];
        for (spent, available, expected, projected, clock, status) in cases {
            assert_eq!(
                pace_status(spent, available, expected, projected, clock),
                status,
                "spent {spent} available {available} expected {expected} projected {projected} {clock:?}"
            );
        }
    }

    #[test]
    fn rent_paid_on_its_due_day_is_on_track() {
        let curve = PaceCurve::linear(31);
        for due in [1, 2, 15] {
            let mut daily = vec![0.0; due as usize];
            daily[due as usize - 1] = 1200.0;
            let pace = category_pace(
                fixed(due),
                1200.0,
                0.0,
                1200.0,
                &daily,
                live(31, due as u32),
                &curve,
            );
            assert_eq!(pace.status, PaceStatus::OnTrack, "due {due}");
            assert!(close(pace.expected_to_date, 1200.0));
            assert!(close(pace.projected, 1200.0));
        }
        // Later in the month the paid bill stays on track.
        let pace = category_pace(
            fixed(1),
            1200.0,
            0.0,
            1200.0,
            &[1200.0; 20],
            live(31, 20),
            &curve,
        );
        assert_eq!(pace.status, PaceStatus::OnTrack);
    }

    #[test]
    fn bill_paid_before_its_due_day_reads_ahead_of_pace() {
        let pace = category_pace(
            fixed(5),
            1200.0,
            0.0,
            1200.0,
            &[0.0, 0.0, 1200.0],
            live(31, 3),
            &PaceCurve::linear(31),
        );
        assert_eq!(pace.status, PaceStatus::Approaching);
    }

    #[test]
    fn linear_category_uses_its_own_run_rate() {
        // 15 of 30 days, 10/day for the last week → 150 + 10 × 15 = 300 > 200.
        let daily = vec![10.0; 15];
        let pace = category_pace(
            PacingRule::LINEAR,
            200.0,
            0.0,
            150.0,
            &daily,
            live(30, 15),
            &PaceCurve::linear(30),
        );
        assert!(close(pace.expected_to_date, 100.0));
        assert!(close(pace.projected, 300.0));
        assert_eq!(pace.status, PaceStatus::Approaching);
    }

    #[test]
    fn rollover_extends_what_is_available() {
        let curve = PaceCurve::linear(30);
        // 100 target + 50 carried in: 120 spent by day 30 of 30 is fine.
        let pace = category_pace(
            PacingRule::LINEAR,
            100.0,
            50.0,
            120.0,
            &[4.0; 30],
            closed(30),
            &curve,
        );
        assert_eq!(pace.status, PaceStatus::OnTrack);
        assert!(close(pace.expected_to_date, 150.0));
        // A negative carry shrinks it: the same rent now overspends.
        let pace = category_pace(
            fixed(1),
            1000.0,
            -100.0,
            1000.0,
            &[1000.0],
            live(30, 1),
            &curve,
        );
        assert_eq!(pace.status, PaceStatus::Over);
        // Expected follows the carried-in amount on a live day.
        let pace = category_pace(
            PacingRule::LINEAR,
            100.0,
            50.0,
            60.0,
            &[4.0; 15],
            live(30, 15),
            &curve,
        );
        assert!(close(pace.expected_to_date, 75.0));
        assert_eq!(pace.status, PaceStatus::OnTrack);
    }

    #[test]
    fn closed_month_projects_what_was_spent() {
        let pace = category_pace(
            fixed(10),
            1000.0,
            0.0,
            900.0,
            &[0.0; 31],
            closed(31),
            &PaceCurve::linear(31),
        );
        assert!(close(pace.projected, 900.0));
        assert!(close(pace.expected_to_date, 1000.0));
        assert_eq!(pace.status, PaceStatus::OnTrack);
    }

    #[test]
    fn trailing_rate_uses_last_seven_days_and_clamps_refunds() {
        let mut daily = vec![100.0; 10];
        daily[0] = 1000.0; // outside the trailing window
        assert!(close(trailing_daily_rate(&daily, 10), 100.0));
        assert!(close(trailing_daily_rate(&[30.0, 0.0, 0.0], 3), 10.0));
        assert!(close(trailing_daily_rate(&[-50.0, 10.0], 2), 0.0));
        assert!(close(trailing_daily_rate(&[], 0), 0.0));
        // Fewer recorded days than elapsed: the missing days count as no spend.
        assert!(close(trailing_daily_rate(&[70.0], 5), 70.0));
    }

    #[test]
    fn historical_curve_needs_two_months_with_spend() {
        let one = vec![HistoryMonth {
            daily: vec![10.0; 30],
        }];
        assert!(historical_curve(&one, 31).is_none());
        let with_empty = vec![
            HistoryMonth {
                daily: vec![10.0; 30],
            },
            HistoryMonth {
                daily: vec![0.0; 31],
            },
        ];
        assert!(historical_curve(&with_empty, 31).is_none());
    }

    #[test]
    fn historical_curve_is_the_median_shape() {
        // Three 30-day months: front-loaded, even, front-loaded.
        let mut front = vec![0.0; 30];
        front[0] = 60.0;
        front[29] = 40.0;
        let even = vec![10.0; 30];
        let months = vec![
            HistoryMonth {
                daily: front.clone(),
            },
            HistoryMonth { daily: even },
            HistoryMonth { daily: front },
        ];
        let curve = historical_curve(&months, 30).unwrap();
        assert_eq!(curve.source(), PaceCurveSource::History);
        assert!(close(curve.fraction(0), 0.0));
        assert!(close(curve.fraction(1), 0.6));
        assert!(close(curve.fraction(15), 0.6));
        assert!(close(curve.fraction(30), 1.0));
        // Rescaled onto a 31-day month it still ends at 1.
        let curve = historical_curve(&months, 31).unwrap();
        assert!(close(curve.fraction(31), 1.0));
    }

    #[test]
    fn window_pace_mixes_fixed_steps_with_flexible_curve() {
        // Rent 1000 due on the 1st (paid), groceries 600 linear, buffer 20.
        // Day 10 of 30: groceries 150 spent, 15/day for the last week.
        let mut flexible_daily = vec![0.0; 10];
        flexible_daily[0] = 45.0;
        for day in flexible_daily.iter_mut().skip(3) {
            *day = 15.0;
        }
        let mut spent_daily = flexible_daily.clone();
        spent_daily[0] += 1000.0;
        let fixed_lines = vec![FixedLine::for_month(fixed(1), 1000.0, 1000.0, 30)];
        let pace = window_pace(WindowPaceInput {
            clock: live(30, 10),
            available: 1620.0,
            spent: 1150.0,
            fixed: &fixed_lines,
            flexible_daily: &flexible_daily,
            spent_daily: &spent_daily,
            curve: None,
        });
        assert!(close(pace.fixed_expected_to_date, 1000.0));
        assert!(close(pace.flexible_expected_to_date, 620.0 * 10.0 / 30.0));
        assert!(close(pace.expected_to_date, 1000.0 + 620.0 / 3.0));
        // Rent does not inflate the run rate: 15/day × 20 days left.
        assert!(close(pace.flexible_daily_rate, 15.0));
        assert!(close(pace.projected, 1000.0 + 150.0 + 15.0 * 20.0));
        assert_eq!(pace.status, PaceStatus::OnTrack);
        assert_eq!(pace.curve_source, PaceCurveSource::Linear);
        assert_eq!(pace.expected_curve.len(), 30);
        assert!(close(pace.expected_curve[0], 1000.0 + 620.0 / 30.0));
        assert!(close(pace.expected_curve[29], 1620.0));
        assert_eq!(pace.spent_curve.len(), 10);
        assert!(close(pace.spent_curve[9], 1150.0));
    }

    #[test]
    fn window_pace_rent_on_day_one_is_on_track() {
        let fixed_lines = vec![FixedLine::for_month(fixed(1), 1000.0, 1000.0, 31)];
        let pace = window_pace(WindowPaceInput {
            clock: live(31, 1),
            available: 1600.0,
            spent: 1000.0,
            fixed: &fixed_lines,
            flexible_daily: &[0.0],
            spent_daily: &[1000.0],
            curve: None,
        });
        assert_eq!(pace.status, PaceStatus::OnTrack);
    }

    #[test]
    fn window_pace_uses_history_curve_and_falls_back_on_length_mismatch() {
        let months = vec![
            HistoryMonth {
                daily: vec![10.0; 30],
            },
            HistoryMonth {
                daily: vec![10.0; 30],
            },
        ];
        let curve = historical_curve(&months, 30).unwrap();
        let input = WindowPaceInput {
            clock: live(30, 15),
            available: 300.0,
            spent: 100.0,
            fixed: &[],
            flexible_daily: &[0.0; 15],
            spent_daily: &[0.0; 15],
            curve: Some(&curve),
        };
        assert_eq!(window_pace(input).curve_source, PaceCurveSource::History);
        let mismatched = WindowPaceInput {
            clock: live(31, 15),
            ..input
        };
        assert_eq!(
            window_pace(mismatched).curve_source,
            PaceCurveSource::Linear
        );
    }

    #[test]
    fn window_pace_closed_and_over() {
        let pace = window_pace(WindowPaceInput {
            clock: closed(30),
            available: 500.0,
            spent: 520.0,
            fixed: &[],
            flexible_daily: &[],
            spent_daily: &[],
            curve: None,
        });
        assert_eq!(pace.status, PaceStatus::Over);
        assert!(close(pace.projected, 520.0));
        assert!(close(pace.expected_to_date, 500.0));
        assert!(!pace.projection_reliable);
    }

    #[test]
    fn history_months_and_buckets() {
        assert_eq!(
            history_months(2026, 2),
            vec![(2025, 11), (2025, 12), (2026, 1)]
        );
        let months = history_months(2026, 2);
        let history = history_from_daily(
            &months,
            [
                (d(2025, 12, 31), 5.0),
                (d(2025, 12, 31), 2.0),
                (d(2026, 2, 1), 99.0),
            ],
        );
        assert_eq!(history[1].daily.len(), 31);
        assert!(close(history[1].daily[30], 7.0));
        assert!(history[2].daily.iter().all(|v| *v == 0.0));
    }
}
