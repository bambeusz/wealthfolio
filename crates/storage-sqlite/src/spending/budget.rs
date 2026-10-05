//! Storage adapter for spending::budget.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::{get_connection, write_actor::DbWriteTx, DbPool, WriteHandle};
use crate::errors::StorageError;
use crate::schema::{
    budget_group_assignments, budget_groups, budget_rollover_settings, budget_targets,
};
use crate::spending::deterministic_ids::{
    budget_group_assignment_id, budget_rollover_setting_id, budget_target_id,
};
use wealthfolio_core::sync::{SyncEntity, SyncOperation};
use wealthfolio_spending::budget::{
    BudgetGroup, BudgetGroupAssignment, BudgetPacing, BudgetRepositoryTrait, BudgetRolloverSetting,
    BudgetRolloverTargetType, BudgetTarget, BudgetTargetType, NewBudgetGroup,
    NewBudgetGroupAssignment, NewBudgetRolloverSetting, NewBudgetTarget, UpdateBudgetGroup,
};

#[derive(Queryable, Identifiable, Selectable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::budget_groups)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct BudgetGroupDB {
    pub id: String,
    pub name: String,
    pub key: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i32,
    pub is_system: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Insertable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::budget_groups)]
pub struct NewBudgetGroupDB {
    pub id: String,
    pub name: String,
    pub key: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i32,
    pub is_system: i32,
    pub created_at: String,
    pub updated_at: String,
}

impl crate::sync::SyncOutboxModel for BudgetGroupDB {
    const ENTITY: SyncEntity = SyncEntity::BudgetGroup;
    fn sync_entity_id(&self) -> &str {
        &self.id
    }

    fn should_sync_outbox(&self, _op: SyncOperation) -> bool {
        self.is_system == 0 || self.updated_at != self.created_at
    }
}

#[derive(Queryable, Identifiable, Selectable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::budget_group_assignments)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct BudgetGroupAssignmentDB {
    pub id: String,
    pub group_id: String,
    pub taxonomy_id: String,
    pub category_id: String,
    pub is_system: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Insertable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::budget_group_assignments)]
pub struct NewBudgetGroupAssignmentDB {
    pub id: String,
    pub group_id: String,
    pub taxonomy_id: String,
    pub category_id: String,
    pub is_system: i32,
    pub created_at: String,
    pub updated_at: String,
}

impl crate::sync::SyncOutboxModel for BudgetGroupAssignmentDB {
    const ENTITY: SyncEntity = SyncEntity::BudgetGroupAssignment;
    fn sync_entity_id(&self) -> &str {
        &self.id
    }

    fn should_sync_outbox(&self, _op: SyncOperation) -> bool {
        self.is_system == 0 || self.updated_at != self.created_at
    }
}

#[derive(Queryable, Identifiable, Selectable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::budget_targets)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct BudgetTargetDB {
    pub id: String,
    pub period_key: String,
    pub target_type: String,
    pub taxonomy_id: Option<String>,
    pub category_id: Option<String>,
    pub group_id: Option<String>,
    pub amount: String,
    pub created_at: String,
    pub updated_at: String,
    pub pacing: String,
    pub due_day: Option<i32>,
}

#[derive(Insertable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::budget_targets)]
pub struct NewBudgetTargetDB {
    pub id: String,
    pub period_key: String,
    pub target_type: String,
    pub taxonomy_id: Option<String>,
    pub category_id: Option<String>,
    pub group_id: Option<String>,
    pub amount: String,
    pub created_at: String,
    pub updated_at: String,
    pub pacing: String,
    pub due_day: Option<i32>,
}

impl crate::sync::SyncOutboxModel for BudgetTargetDB {
    const ENTITY: SyncEntity = SyncEntity::BudgetTarget;
    fn sync_entity_id(&self) -> &str {
        &self.id
    }
}

#[derive(Queryable, Identifiable, Selectable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::budget_rollover_settings)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct BudgetRolloverSettingDB {
    pub id: String,
    pub target_type: String,
    pub taxonomy_id: Option<String>,
    pub category_id: Option<String>,
    pub group_id: Option<String>,
    pub enabled: i32,
    pub start_month: String,
    pub starting_balance: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Insertable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::budget_rollover_settings)]
pub struct NewBudgetRolloverSettingDB {
    pub id: String,
    pub target_type: String,
    pub taxonomy_id: Option<String>,
    pub category_id: Option<String>,
    pub group_id: Option<String>,
    pub enabled: i32,
    pub start_month: String,
    pub starting_balance: String,
    pub created_at: String,
    pub updated_at: String,
}

impl crate::sync::SyncOutboxModel for BudgetRolloverSettingDB {
    const ENTITY: SyncEntity = SyncEntity::BudgetRolloverSetting;
    fn sync_entity_id(&self) -> &str {
        &self.id
    }
}

pub struct BudgetRepository {
    pool: Arc<DbPool>,
    writer: WriteHandle,
}

impl BudgetRepository {
    pub fn new(pool: Arc<DbPool>, writer: WriteHandle) -> Self {
        Self { pool, writer }
    }

    async fn upsert_group_assignments_with_system_flag(
        &self,
        assignments: Vec<NewBudgetGroupAssignment>,
        is_system: i32,
    ) -> Result<Vec<BudgetGroupAssignment>> {
        let now = chrono::Utc::now().to_rfc3339();
        self.writer
            .exec_tx(move |tx| {
                let mut out = Vec::with_capacity(assignments.len());
                for assignment in assignments {
                    let NewBudgetGroupAssignment {
                        id,
                        group_id,
                        taxonomy_id,
                        category_id,
                    } = assignment;
                    let id = id
                        .unwrap_or_else(|| budget_group_assignment_id(&taxonomy_id, &category_id));
                    let existing = budget_group_assignments::table
                        .filter(budget_group_assignments::taxonomy_id.eq(&taxonomy_id))
                        .filter(budget_group_assignments::category_id.eq(&category_id))
                        .first::<BudgetGroupAssignmentDB>(tx.conn())
                        .optional()
                        .map_err(StorageError::from)?;
                    if let Some(existing) = existing {
                        if existing.group_id == group_id && existing.is_system == is_system {
                            out.push(existing);
                            continue;
                        }
                        diesel::update(budget_group_assignments::table.find(&existing.id))
                            .set((
                                budget_group_assignments::group_id.eq(&group_id),
                                budget_group_assignments::is_system.eq(is_system),
                                budget_group_assignments::updated_at.eq(&now),
                            ))
                            .execute(tx.conn())
                            .map_err(StorageError::from)?;
                        let updated = budget_group_assignments::table
                            .find(&existing.id)
                            .first::<BudgetGroupAssignmentDB>(tx.conn())
                            .map_err(StorageError::from)?;
                        tx.update(&updated)?;
                        out.push(updated);
                        continue;
                    }

                    let row = NewBudgetGroupAssignmentDB {
                        id,
                        group_id,
                        taxonomy_id,
                        category_id,
                        is_system,
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    };
                    let inserted = diesel::insert_into(budget_group_assignments::table)
                        .values(&row)
                        .returning(BudgetGroupAssignmentDB::as_returning())
                        .get_result(tx.conn())
                        .map_err(StorageError::from)?;
                    tx.update(&inserted)?;
                    out.push(inserted);
                }
                Ok(out)
            })
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
            .map_err(|e| anyhow::anyhow!(e))
    }
}

fn parse_dt(s: &str) -> NaiveDateTime {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.naive_utc())
        .unwrap_or_else(|_| chrono::Utc::now().naive_utc())
}

fn target_type_from_str(value: &str) -> BudgetTargetType {
    match value {
        "group_buffer" => BudgetTargetType::GroupBuffer,
        _ => BudgetTargetType::Category,
    }
}

fn rollover_target_type_from_str(value: &str) -> BudgetRolloverTargetType {
    match value {
        "group" => BudgetRolloverTargetType::Group,
        _ => BudgetRolloverTargetType::Category,
    }
}

/// The insert row for a target, and whether an update must keep the stored
/// pacing (the caller sent none).
fn new_target_row(target: NewBudgetTarget, now: &str) -> (NewBudgetTargetDB, bool) {
    let NewBudgetTarget {
        id,
        period_key,
        target_type,
        taxonomy_id,
        category_id,
        group_id,
        amount,
        pacing,
        due_day,
    } = target;
    let target_type = target_type.as_str().to_string();
    let id = id.unwrap_or_else(|| {
        budget_target_id(
            &period_key,
            &target_type,
            taxonomy_id.as_deref(),
            category_id.as_deref(),
            group_id.as_deref(),
        )
    });
    let row = NewBudgetTargetDB {
        id,
        period_key,
        target_type,
        taxonomy_id,
        category_id,
        group_id,
        amount,
        created_at: now.to_string(),
        updated_at: now.to_string(),
        pacing: pacing.unwrap_or_default().as_str().to_string(),
        due_day: if pacing.is_some() { due_day } else { None },
    };
    (row, pacing.is_none())
}

fn upsert_target_tx(
    tx: &mut DbWriteTx<'_>,
    row: &NewBudgetTargetDB,
    keep_pacing: bool,
) -> wealthfolio_core::Result<BudgetTargetDB> {
    let existing_id = if row.target_type == "category" {
        budget_targets::table
            .filter(budget_targets::target_type.eq("category"))
            .filter(budget_targets::period_key.eq(&row.period_key))
            .filter(budget_targets::taxonomy_id.eq(&row.taxonomy_id))
            .filter(budget_targets::category_id.eq(&row.category_id))
            .select(budget_targets::id)
            .first::<String>(tx.conn())
            .optional()
            .map_err(StorageError::from)?
    } else {
        budget_targets::table
            .filter(budget_targets::target_type.eq("group_buffer"))
            .filter(budget_targets::period_key.eq(&row.period_key))
            .filter(budget_targets::group_id.eq(&row.group_id))
            .select(budget_targets::id)
            .first::<String>(tx.conn())
            .optional()
            .map_err(StorageError::from)?
    };
    let result = if let Some(existing_id) = existing_id {
        if keep_pacing {
            diesel::update(budget_targets::table.find(&existing_id))
                .set((
                    budget_targets::amount.eq(&row.amount),
                    budget_targets::updated_at.eq(&row.updated_at),
                ))
                .execute(tx.conn())
                .map_err(StorageError::from)?;
        } else {
            diesel::update(budget_targets::table.find(&existing_id))
                .set((
                    budget_targets::amount.eq(&row.amount),
                    budget_targets::pacing.eq(&row.pacing),
                    budget_targets::due_day.eq(row.due_day),
                    budget_targets::updated_at.eq(&row.updated_at),
                ))
                .execute(tx.conn())
                .map_err(StorageError::from)?;
        }
        budget_targets::table
            .find(&existing_id)
            .first::<BudgetTargetDB>(tx.conn())
            .map_err(StorageError::from)?
    } else {
        diesel::insert_into(budget_targets::table)
            .values(row)
            .returning(BudgetTargetDB::as_returning())
            .get_result(tx.conn())
            .map_err(StorageError::from)?
    };
    tx.update(&result)?;
    Ok(result)
}

fn sum_amount_strings(left: &str, right: &str) -> String {
    let left = left.parse::<Decimal>().unwrap_or(Decimal::ZERO);
    let right = right.parse::<Decimal>().unwrap_or(Decimal::ZERO);
    (left + right).normalize().to_string()
}

impl From<BudgetGroupDB> for BudgetGroup {
    fn from(db: BudgetGroupDB) -> Self {
        Self {
            id: db.id,
            name: db.name,
            key: db.key,
            color: db.color,
            icon: db.icon,
            sort_order: db.sort_order,
            is_system: db.is_system != 0,
            created_at: parse_dt(&db.created_at),
            updated_at: parse_dt(&db.updated_at),
        }
    }
}

impl From<BudgetGroupAssignmentDB> for BudgetGroupAssignment {
    fn from(db: BudgetGroupAssignmentDB) -> Self {
        Self {
            id: db.id,
            group_id: db.group_id,
            taxonomy_id: db.taxonomy_id,
            category_id: db.category_id,
            created_at: parse_dt(&db.created_at),
            updated_at: parse_dt(&db.updated_at),
        }
    }
}

impl From<BudgetTargetDB> for BudgetTarget {
    fn from(db: BudgetTargetDB) -> Self {
        Self {
            id: db.id,
            period_key: db.period_key,
            target_type: target_type_from_str(&db.target_type),
            taxonomy_id: db.taxonomy_id,
            category_id: db.category_id,
            group_id: db.group_id,
            amount: db.amount,
            pacing: BudgetPacing::from_db(&db.pacing),
            due_day: db.due_day,
            created_at: parse_dt(&db.created_at),
            updated_at: parse_dt(&db.updated_at),
        }
    }
}

impl From<BudgetRolloverSettingDB> for BudgetRolloverSetting {
    fn from(db: BudgetRolloverSettingDB) -> Self {
        Self {
            id: db.id,
            target_type: rollover_target_type_from_str(&db.target_type),
            taxonomy_id: db.taxonomy_id,
            category_id: db.category_id,
            group_id: db.group_id,
            enabled: db.enabled != 0,
            start_month: db.start_month,
            starting_balance: db.starting_balance,
            created_at: parse_dt(&db.created_at),
            updated_at: parse_dt(&db.updated_at),
        }
    }
}

#[async_trait]
impl BudgetRepositoryTrait for BudgetRepository {
    async fn list_groups(&self) -> Result<Vec<BudgetGroup>> {
        let mut conn = get_connection(&self.pool).map_err(|e| anyhow::anyhow!(e))?;
        let rows = budget_groups::table
            .order((budget_groups::sort_order.asc(), budget_groups::name.asc()))
            .load::<BudgetGroupDB>(&mut conn)
            .map_err(StorageError::from)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn create_group(&self, new_group: NewBudgetGroup) -> Result<BudgetGroup> {
        let now = chrono::Utc::now().to_rfc3339();
        let row = NewBudgetGroupDB {
            id: new_group.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            key: new_group.key.unwrap_or_else(|| Uuid::new_v4().to_string()),
            name: new_group.name,
            color: new_group.color,
            icon: new_group.icon,
            sort_order: new_group.sort_order.unwrap_or(0),
            is_system: if new_group.is_system { 1 } else { 0 },
            created_at: now.clone(),
            updated_at: now,
        };
        self.writer
            .exec_tx(move |tx| {
                let inserted = diesel::insert_into(budget_groups::table)
                    .values(&row)
                    .returning(BudgetGroupDB::as_returning())
                    .get_result(tx.conn())
                    .map_err(StorageError::from)?;
                tx.insert(&inserted)?;
                Ok(inserted)
            })
            .await
            .map(Into::into)
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn update_group(&self, id: &str, patch: UpdateBudgetGroup) -> Result<BudgetGroup> {
        let id = id.to_string();
        self.writer
            .exec_tx(move |tx| {
                let mut existing = budget_groups::table
                    .find(&id)
                    .first::<BudgetGroupDB>(tx.conn())
                    .map_err(StorageError::from)?;
                if let Some(name) = patch.name {
                    existing.name = name;
                }
                if let Some(color) = patch.color {
                    existing.color = color;
                }
                if let Some(icon) = patch.icon {
                    existing.icon = icon;
                }
                if let Some(sort_order) = patch.sort_order {
                    existing.sort_order = sort_order;
                }
                existing.updated_at = chrono::Utc::now().to_rfc3339();
                diesel::update(budget_groups::table.find(&id))
                    .set((
                        budget_groups::name.eq(&existing.name),
                        budget_groups::color.eq(&existing.color),
                        budget_groups::icon.eq(&existing.icon),
                        budget_groups::sort_order.eq(existing.sort_order),
                        budget_groups::updated_at.eq(&existing.updated_at),
                    ))
                    .execute(tx.conn())
                    .map_err(StorageError::from)?;
                tx.update(&existing)?;
                Ok(existing)
            })
            .await
            .map(Into::into)
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn delete_group(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        self.writer
            .exec_tx(move |tx| {
                let affected = diesel::delete(budget_groups::table.find(&id))
                    .execute(tx.conn())
                    .map_err(StorageError::from)?;
                if affected > 0 {
                    tx.delete::<BudgetGroupDB>(id.clone());
                }
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn delete_group_and_reassign(
        &self,
        id: &str,
        reassign_to_group_id: &str,
        reassignments: Vec<NewBudgetGroupAssignment>,
    ) -> Result<()> {
        let id = id.to_string();
        let reassign_to_group_id = reassign_to_group_id.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.writer
            .exec_tx(move |tx| {
                for assignment in reassignments {
                    let NewBudgetGroupAssignment {
                        id,
                        group_id,
                        taxonomy_id,
                        category_id,
                    } = assignment;
                    let id = id
                        .unwrap_or_else(|| budget_group_assignment_id(&taxonomy_id, &category_id));
                    let row = NewBudgetGroupAssignmentDB {
                        id,
                        group_id,
                        taxonomy_id,
                        category_id,
                        is_system: 0,
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    };
                    let inserted = diesel::insert_into(budget_group_assignments::table)
                        .values(&row)
                        .on_conflict((
                            budget_group_assignments::taxonomy_id,
                            budget_group_assignments::category_id,
                        ))
                        .do_update()
                        .set((
                            budget_group_assignments::group_id.eq(&row.group_id),
                            budget_group_assignments::updated_at.eq(&row.updated_at),
                        ))
                        .returning(BudgetGroupAssignmentDB::as_returning())
                        .get_result(tx.conn())
                        .map_err(StorageError::from)?;
                    tx.update(&inserted)?;
                }

                let source_buffers = budget_targets::table
                    .filter(budget_targets::target_type.eq("group_buffer"))
                    .filter(budget_targets::group_id.eq(&id))
                    .load::<BudgetTargetDB>(tx.conn())
                    .map_err(StorageError::from)?;

                for source in source_buffers {
                    let destination = budget_targets::table
                        .filter(budget_targets::target_type.eq("group_buffer"))
                        .filter(budget_targets::period_key.eq(&source.period_key))
                        .filter(budget_targets::group_id.eq(&reassign_to_group_id))
                        .first::<BudgetTargetDB>(tx.conn())
                        .optional()
                        .map_err(StorageError::from)?;

                    if let Some(mut destination) = destination {
                        destination.amount =
                            sum_amount_strings(&destination.amount, &source.amount);
                        destination.updated_at = now.clone();
                        diesel::update(budget_targets::table.find(&destination.id))
                            .set((
                                budget_targets::amount.eq(&destination.amount),
                                budget_targets::updated_at.eq(&destination.updated_at),
                            ))
                            .execute(tx.conn())
                            .map_err(StorageError::from)?;
                        tx.update(&destination)?;

                        diesel::delete(budget_targets::table.find(&source.id))
                            .execute(tx.conn())
                            .map_err(StorageError::from)?;
                        tx.delete::<BudgetTargetDB>(source.id);
                    } else {
                        let mut moved = source;
                        moved.group_id = Some(reassign_to_group_id.clone());
                        moved.updated_at = now.clone();
                        diesel::update(budget_targets::table.find(&moved.id))
                            .set((
                                budget_targets::group_id.eq(&moved.group_id),
                                budget_targets::updated_at.eq(&moved.updated_at),
                            ))
                            .execute(tx.conn())
                            .map_err(StorageError::from)?;
                        tx.update(&moved)?;
                    }
                }

                let affected = diesel::delete(budget_groups::table.find(&id))
                    .execute(tx.conn())
                    .map_err(StorageError::from)?;
                if affected > 0 {
                    tx.delete::<BudgetGroupDB>(id.clone());
                }
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn upsert_system_groups(&self, groups: Vec<NewBudgetGroup>) -> Result<Vec<BudgetGroup>> {
        let now = chrono::Utc::now().to_rfc3339();
        self.writer
            .exec_tx(move |tx| {
                let mut out = Vec::with_capacity(groups.len());
                for group in groups {
                    let row = NewBudgetGroupDB {
                        id: group.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                        key: group.key.unwrap_or_else(|| Uuid::new_v4().to_string()),
                        name: group.name,
                        color: group.color,
                        icon: group.icon,
                        sort_order: group.sort_order.unwrap_or(0),
                        is_system: 1,
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    };
                    let existing = budget_groups::table
                        .filter(budget_groups::key.eq(&row.key))
                        .first::<BudgetGroupDB>(tx.conn())
                        .optional()
                        .map_err(StorageError::from)?;
                    if let Some(existing) = existing {
                        if existing.name == row.name
                            && existing.color == row.color
                            && existing.icon == row.icon
                            && existing.sort_order == row.sort_order
                            && existing.is_system == 1
                        {
                            out.push(existing);
                            continue;
                        }
                    }
                    let inserted = diesel::insert_into(budget_groups::table)
                        .values(&row)
                        .on_conflict(budget_groups::key)
                        .do_update()
                        .set((
                            budget_groups::name.eq(&row.name),
                            budget_groups::color.eq(&row.color),
                            budget_groups::icon.eq(&row.icon),
                            budget_groups::sort_order.eq(row.sort_order),
                            budget_groups::is_system.eq(1),
                            budget_groups::updated_at.eq(&row.updated_at),
                        ))
                        .returning(BudgetGroupDB::as_returning())
                        .get_result(tx.conn())
                        .map_err(StorageError::from)?;
                    tx.update(&inserted)?;
                    out.push(inserted);
                }
                Ok(out)
            })
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn list_group_assignments(&self) -> Result<Vec<BudgetGroupAssignment>> {
        let mut conn = get_connection(&self.pool).map_err(|e| anyhow::anyhow!(e))?;
        let rows = budget_group_assignments::table
            .load::<BudgetGroupAssignmentDB>(&mut conn)
            .map_err(StorageError::from)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn upsert_group_assignment(
        &self,
        assignment: NewBudgetGroupAssignment,
    ) -> Result<BudgetGroupAssignment> {
        let rows = self.upsert_group_assignments(vec![assignment]).await?;
        rows.into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Failed to save budget group assignment"))
    }

    async fn upsert_group_assignments(
        &self,
        assignments: Vec<NewBudgetGroupAssignment>,
    ) -> Result<Vec<BudgetGroupAssignment>> {
        self.upsert_group_assignments_with_system_flag(assignments, 0)
            .await
    }

    async fn upsert_system_group_assignments(
        &self,
        assignments: Vec<NewBudgetGroupAssignment>,
    ) -> Result<Vec<BudgetGroupAssignment>> {
        self.upsert_group_assignments_with_system_flag(assignments, 1)
            .await
    }

    async fn list_targets(&self) -> Result<Vec<BudgetTarget>> {
        let mut conn = get_connection(&self.pool).map_err(|e| anyhow::anyhow!(e))?;
        let rows = budget_targets::table
            .load::<BudgetTargetDB>(&mut conn)
            .map_err(StorageError::from)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn upsert_target(&self, target: NewBudgetTarget) -> Result<BudgetTarget> {
        let (row, keep_pacing) = new_target_row(target, &chrono::Utc::now().to_rfc3339());
        self.writer
            .exec_tx(move |tx| upsert_target_tx(tx, &row, keep_pacing))
            .await
            .map(Into::into)
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn upsert_targets(&self, targets: Vec<NewBudgetTarget>) -> Result<Vec<BudgetTarget>> {
        let now = chrono::Utc::now().to_rfc3339();
        let rows: Vec<(NewBudgetTargetDB, bool)> = targets
            .into_iter()
            .map(|target| new_target_row(target, &now))
            .collect();
        self.writer
            .exec_tx(move |tx| {
                rows.iter()
                    .map(|(row, keep_pacing)| upsert_target_tx(tx, row, *keep_pacing))
                    .collect::<wealthfolio_core::Result<Vec<_>>>()
            })
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn delete_target(&self, id: &str) -> Result<()> {
        self.delete_targets(vec![id.to_string()]).await
    }

    async fn delete_targets(&self, ids: Vec<String>) -> Result<()> {
        self.writer
            .exec_tx(move |tx| {
                for id in ids {
                    let affected = diesel::delete(budget_targets::table.find(&id))
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                    if affected > 0 {
                        tx.delete::<BudgetTargetDB>(id);
                    }
                }
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn list_rollover_settings(&self) -> Result<Vec<BudgetRolloverSetting>> {
        let mut conn = get_connection(&self.pool).map_err(|e| anyhow::anyhow!(e))?;
        let rows = budget_rollover_settings::table
            .load::<BudgetRolloverSettingDB>(&mut conn)
            .map_err(StorageError::from)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn upsert_rollover_setting(
        &self,
        setting: NewBudgetRolloverSetting,
    ) -> Result<BudgetRolloverSetting> {
        let now = chrono::Utc::now().to_rfc3339();
        let NewBudgetRolloverSetting {
            id,
            target_type,
            taxonomy_id,
            category_id,
            group_id,
            enabled,
            start_month,
            starting_balance,
        } = setting;
        let target_type = target_type.as_str().to_string();
        let id = id.unwrap_or_else(|| {
            budget_rollover_setting_id(
                &target_type,
                taxonomy_id.as_deref(),
                category_id.as_deref(),
                group_id.as_deref(),
            )
        });
        let row = NewBudgetRolloverSettingDB {
            id,
            target_type,
            taxonomy_id,
            category_id,
            group_id,
            enabled: if enabled { 1 } else { 0 },
            start_month,
            starting_balance,
            created_at: now.clone(),
            updated_at: now,
        };
        self.writer
            .exec_tx(move |tx| {
                let existing_id = if row.target_type == "category" {
                    budget_rollover_settings::table
                        .filter(budget_rollover_settings::target_type.eq("category"))
                        .filter(budget_rollover_settings::taxonomy_id.eq(&row.taxonomy_id))
                        .filter(budget_rollover_settings::category_id.eq(&row.category_id))
                        .select(budget_rollover_settings::id)
                        .first::<String>(tx.conn())
                        .optional()
                        .map_err(StorageError::from)?
                } else {
                    budget_rollover_settings::table
                        .filter(budget_rollover_settings::target_type.eq("group"))
                        .filter(budget_rollover_settings::group_id.eq(&row.group_id))
                        .select(budget_rollover_settings::id)
                        .first::<String>(tx.conn())
                        .optional()
                        .map_err(StorageError::from)?
                };
                let result = if let Some(existing_id) = existing_id {
                    diesel::update(budget_rollover_settings::table.find(&existing_id))
                        .set((
                            budget_rollover_settings::enabled.eq(row.enabled),
                            budget_rollover_settings::start_month.eq(&row.start_month),
                            budget_rollover_settings::starting_balance.eq(&row.starting_balance),
                            budget_rollover_settings::updated_at.eq(&row.updated_at),
                        ))
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                    budget_rollover_settings::table
                        .find(&existing_id)
                        .first::<BudgetRolloverSettingDB>(tx.conn())
                        .map_err(StorageError::from)?
                } else {
                    diesel::insert_into(budget_rollover_settings::table)
                        .values(&row)
                        .returning(BudgetRolloverSettingDB::as_returning())
                        .get_result(tx.conn())
                        .map_err(StorageError::from)?
                };
                tx.update(&result)?;
                Ok(result)
            })
            .await
            .map(Into::into)
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn delete_rollover_setting(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        self.writer
            .exec_tx(move |tx| {
                let affected = diesel::delete(budget_rollover_settings::table.find(&id))
                    .execute(tx.conn())
                    .map_err(StorageError::from)?;
                if affected > 0 {
                    tx.delete::<BudgetRolloverSettingDB>(id.clone());
                }
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn disable_category_rollovers(
        &self,
        taxonomy_id: &str,
        category_ids: &[String],
    ) -> Result<Vec<BudgetRolloverSetting>> {
        if category_ids.is_empty() {
            return Ok(Vec::new());
        }
        let taxonomy_id = taxonomy_id.to_string();
        let category_ids = category_ids.to_vec();
        self.writer
            .exec_tx(move |tx| {
                let now = chrono::Utc::now().to_rfc3339();
                diesel::update(
                    budget_rollover_settings::table
                        .filter(budget_rollover_settings::target_type.eq("category"))
                        .filter(budget_rollover_settings::taxonomy_id.eq(&taxonomy_id))
                        .filter(budget_rollover_settings::category_id.eq_any(&category_ids)),
                )
                .set((
                    budget_rollover_settings::enabled.eq(0),
                    budget_rollover_settings::updated_at.eq(&now),
                ))
                .execute(tx.conn())
                .map_err(StorageError::from)?;

                let rows = budget_rollover_settings::table
                    .filter(budget_rollover_settings::target_type.eq("category"))
                    .filter(budget_rollover_settings::taxonomy_id.eq(&taxonomy_id))
                    .filter(budget_rollover_settings::category_id.eq_any(&category_ids))
                    .load::<BudgetRolloverSettingDB>(tx.conn())
                    .map_err(StorageError::from)?;
                for row in &rows {
                    tx.update(row)?;
                }
                Ok(rows)
            })
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn copy_period_targets(
        &self,
        source_period_key: &str,
        target_period_key: &str,
        overwrite: bool,
    ) -> Result<Vec<BudgetTarget>> {
        let source = source_period_key.to_string();
        let target = target_period_key.to_string();
        self.writer
            .exec_tx(move |tx| {
                let source_rows = budget_targets::table
                    .filter(budget_targets::period_key.eq(&source))
                    .load::<BudgetTargetDB>(tx.conn())
                    .map_err(StorageError::from)?;

                if overwrite {
                    let to_delete = budget_targets::table
                        .filter(budget_targets::period_key.eq(&target))
                        .load::<BudgetTargetDB>(tx.conn())
                        .map_err(StorageError::from)?;
                    diesel::delete(
                        budget_targets::table.filter(budget_targets::period_key.eq(&target)),
                    )
                    .execute(tx.conn())
                    .map_err(StorageError::from)?;
                    for row in &to_delete {
                        tx.delete::<BudgetTargetDB>(row.id.clone());
                    }
                }

                let now = chrono::Utc::now().to_rfc3339();
                for source_row in source_rows {
                    let id = budget_target_id(
                        &target,
                        &source_row.target_type,
                        source_row.taxonomy_id.as_deref(),
                        source_row.category_id.as_deref(),
                        source_row.group_id.as_deref(),
                    );
                    let new_row = NewBudgetTargetDB {
                        id,
                        period_key: target.clone(),
                        target_type: source_row.target_type.clone(),
                        taxonomy_id: source_row.taxonomy_id.clone(),
                        category_id: source_row.category_id.clone(),
                        group_id: source_row.group_id.clone(),
                        amount: source_row.amount.clone(),
                        created_at: now.clone(),
                        updated_at: now.clone(),
                        pacing: source_row.pacing.clone(),
                        due_day: source_row.due_day,
                    };
                    let inserted = diesel::insert_into(budget_targets::table)
                        .values(&new_row)
                        .on_conflict_do_nothing()
                        .returning(BudgetTargetDB::as_returning())
                        .get_result(tx.conn())
                        .optional()
                        .map_err(StorageError::from)?;
                    if let Some(row) = inserted {
                        tx.update(&row)?;
                    }
                }

                let all_target_rows = budget_targets::table
                    .filter(budget_targets::period_key.eq(&target))
                    .load::<BudgetTargetDB>(tx.conn())
                    .map_err(StorageError::from)?;
                Ok(all_target_rows)
            })
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
            .map_err(|e| anyhow::anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_pool, init, run_migrations, write_actor::spawn_writer};
    use tempfile::tempdir;

    const PACING_UP: &str =
        include_str!("../../migrations/2026-10-05-000002_budget_target_pacing/up.sql");
    const PACING_DOWN: &str =
        include_str!("../../migrations/2026-10-05-000002_budget_target_pacing/down.sql");

    fn setup() -> (BudgetRepository, String) {
        let app_data = tempdir()
            .expect("tempdir")
            .keep()
            .to_string_lossy()
            .to_string();
        let db_path = init(&app_data).expect("init db");
        run_migrations(&db_path).expect("migrate db");
        let pool = create_pool(&db_path).expect("create pool");
        let writer = spawn_writer(pool.as_ref().clone()).expect("writer");
        (BudgetRepository::new(pool, writer), db_path)
    }

    fn housing(
        period_key: &str,
        amount: &str,
        pacing: Option<BudgetPacing>,
        due_day: Option<i32>,
    ) -> NewBudgetTarget {
        NewBudgetTarget {
            id: None,
            period_key: period_key.to_string(),
            target_type: BudgetTargetType::Category,
            taxonomy_id: Some("spending_categories".to_string()),
            category_id: Some("cat_housing".to_string()),
            group_id: None,
            amount: amount.to_string(),
            pacing,
            due_day,
        }
    }

    fn insert_raw(db_path: &str, pacing: &str, due_day: &str) -> rusqlite::Result<usize> {
        let conn = rusqlite::Connection::open(db_path).expect("open db");
        conn.execute(
            &format!(
                "INSERT INTO budget_targets (id, period_key, target_type, taxonomy_id, \
                 category_id, amount, pacing, due_day) VALUES ('raw-{pacing}-{due_day}', \
                 'default', 'category', 'spending_categories', 'cat_food', '10', \
                 '{pacing}', {due_day})"
            ),
            [],
        )
    }

    #[tokio::test]
    async fn pacing_columns_are_checked() {
        let (_repo, db_path) = setup();
        assert!(insert_raw(&db_path, "weekly", "NULL").is_err());
        assert!(insert_raw(&db_path, "monthly_on_day", "0").is_err());
        assert!(insert_raw(&db_path, "monthly_on_day", "32").is_err());
        assert!(insert_raw(&db_path, "monthly_on_day", "31").is_ok());

        let conn = rusqlite::Connection::open(&db_path).expect("open db");
        conn.execute(
            "INSERT INTO budget_targets (id, period_key, target_type, taxonomy_id, \
             category_id, amount) VALUES ('defaulted', '2026-10', 'category', \
             'spending_categories', 'cat_food', '10')",
            [],
        )
        .expect("insert without pacing");
        let (pacing, due_day): (String, Option<i32>) = conn
            .query_row(
                "SELECT pacing, due_day FROM budget_targets WHERE id = 'defaulted'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read defaults");
        assert_eq!(pacing, "linear");
        assert_eq!(due_day, None);
    }

    #[tokio::test]
    async fn pacing_migration_reverts_and_reapplies() {
        let (_repo, db_path) = setup();
        let conn = rusqlite::Connection::open(&db_path).expect("open db");
        conn.execute(
            "INSERT INTO budget_targets (id, period_key, target_type, taxonomy_id, \
             category_id, amount, pacing, due_day) VALUES ('kept', 'default', 'category', \
             'spending_categories', 'cat_housing', '1200', 'monthly_on_day', 1)",
            [],
        )
        .expect("insert row");
        conn.execute_batch(PACING_DOWN).expect("down migration");
        let columns: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('budget_targets')")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<String>>>()
            })
            .expect("columns");
        assert!(!columns.iter().any(|c| c == "pacing" || c == "due_day"));
        let amount: String = conn
            .query_row(
                "SELECT amount FROM budget_targets WHERE id = 'kept'",
                [],
                |row| row.get(0),
            )
            .expect("row survives");
        assert_eq!(amount, "1200");
        conn.execute_batch(PACING_UP).expect("up migration again");
        let pacing: String = conn
            .query_row(
                "SELECT pacing FROM budget_targets WHERE id = 'kept'",
                [],
                |row| row.get(0),
            )
            .expect("re-added column");
        assert_eq!(pacing, "linear");
    }

    #[tokio::test]
    async fn upsert_round_trips_and_keeps_pacing_when_none_is_sent() {
        let (repo, _) = setup();
        let saved = repo
            .upsert_target(housing(
                "default",
                "1200",
                Some(BudgetPacing::MonthlyOnDay),
                Some(3),
            ))
            .await
            .expect("save");
        assert_eq!(saved.pacing, BudgetPacing::MonthlyOnDay);
        assert_eq!(saved.due_day, Some(3));

        let updated = repo
            .upsert_target(housing("default", "1300", None, None))
            .await
            .expect("update amount");
        assert_eq!(updated.id, saved.id);
        assert_eq!(updated.amount, "1300");
        assert_eq!(updated.pacing, BudgetPacing::MonthlyOnDay);
        assert_eq!(updated.due_day, Some(3));

        let linear = repo
            .upsert_target(housing("default", "1300", Some(BudgetPacing::Linear), None))
            .await
            .expect("switch to linear");
        assert_eq!(linear.pacing, BudgetPacing::Linear);
        assert_eq!(linear.due_day, None);
    }

    #[tokio::test]
    async fn copy_period_targets_carries_pacing() {
        let (repo, _) = setup();
        repo.upsert_target(housing(
            "2026-09",
            "1200",
            Some(BudgetPacing::MonthlyOnDay),
            Some(31),
        ))
        .await
        .expect("save");
        let copied = repo
            .copy_period_targets("2026-09", "2026-10", false)
            .await
            .expect("copy");
        assert_eq!(copied.len(), 1);
        assert_eq!(copied[0].pacing, BudgetPacing::MonthlyOnDay);
        assert_eq!(copied[0].due_day, Some(31));
    }

    #[tokio::test]
    async fn bulk_upsert_and_delete_run_in_one_transaction() {
        let (repo, _) = setup();
        let saved = repo
            .upsert_targets(vec![
                housing("2026-10", "1200", Some(BudgetPacing::MonthlyOnDay), Some(1)),
                NewBudgetTarget {
                    category_id: Some("cat_food".to_string()),
                    ..housing("2026-10", "300", None, None)
                },
            ])
            .await
            .expect("bulk save");
        assert_eq!(saved.len(), 2);

        // A failing row rolls back the whole batch.
        let failed = repo
            .upsert_targets(vec![
                housing("2026-11", "1200", None, None),
                // Violates the table CHECK: a category target needs a taxonomy.
                NewBudgetTarget {
                    taxonomy_id: None,
                    category_id: Some("cat_food".to_string()),
                    ..housing("2026-11", "1", None, None)
                },
            ])
            .await;
        assert!(failed.is_err());
        assert!(repo
            .list_targets()
            .await
            .expect("list")
            .iter()
            .all(|t| t.period_key != "2026-11"));

        repo.delete_targets(saved.iter().map(|t| t.id.clone()).collect())
            .await
            .expect("bulk delete");
        assert!(repo.list_targets().await.expect("list").is_empty());
    }
}
