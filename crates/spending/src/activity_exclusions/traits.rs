use anyhow::Result;
use async_trait::async_trait;

use super::model::ActivityExclusion;

/// Repository for the `spending_activity_exclusions` table.
#[async_trait]
pub trait ActivityExclusionsRepositoryTrait: Send + Sync {
    /// Every excluded activity with its current transfer group. The set stays
    /// small (a handful of hand-picked rows), so totals load it whole.
    async fn list_all(&self) -> Result<Vec<ActivityExclusion>>;

    /// Exclude (`true`) or re-include (`false`) one activity. Idempotent.
    async fn set_excluded(&self, activity_id: &str, excluded: bool) -> Result<()>;
}
