use serde::{Deserialize, Serialize};

/// An activity the user took out of Spending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityExclusion {
    pub activity_id: String,
    /// The activity's current `source_group_id` (the transfer link), read at
    /// query time rather than stored, so a later link or unlink is picked up
    /// without touching this table.
    pub group_id: Option<String>,
}
