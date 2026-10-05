//! Per-activity "Exclude from Spending" switch.
//!
//! Sidecar table (1:1 by `activity_id` PK), kept outside the core `activities`
//! row like `activity_events`. An excluded activity is ignored by every
//! spending total — reports, insights, budgets, events and pace — but stays in
//! the ledger with its real cash-flow bucket, and nothing outside Spending
//! (balances, net worth, performance, transfer links) looks at this table.
//!
//! A linked transfer is excluded as a pair: the index resolves the switch
//! through the leg's `source_group_id`, so excluding either leg is enough and
//! linking/unlinking needs no bookkeeping here.

pub mod model;
pub mod traits;

pub use model::ActivityExclusion;
pub use traits::ActivityExclusionsRepositoryTrait;
