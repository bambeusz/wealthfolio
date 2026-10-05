//! Budget — backend-computed monthly targets, groups, and rollover state.

pub mod model;
pub mod pacing;
pub mod service;
pub mod traits;

pub use model::{
    BudgetCategoryRow, BudgetGroup, BudgetGroupAssignment, BudgetGroupRow, BudgetPacing,
    BudgetRolloverSetting, BudgetRolloverTargetType, BudgetSnapshot, BudgetSnapshotComputed,
    BudgetSnapshotState, BudgetTarget, BudgetTargetInput, BudgetTargetType, BudgetTotals,
    CopyMonthRequest, NewBudgetGroup, NewBudgetGroupAssignment, NewBudgetRolloverSetting,
    NewBudgetTarget, UpdateBudgetGroup,
};
pub use pacing::{BudgetPace, PaceCurveSource, PaceStatus};
pub use service::BudgetService;
pub use traits::BudgetRepositoryTrait;
