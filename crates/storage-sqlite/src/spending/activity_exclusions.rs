//! Storage adapter for spending::activity_exclusions — Diesel impl over the
//! `spending_activity_exclusions` side table.
//!
//! Local to this device: rows are not written to the sync outbox.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use diesel::prelude::*;

use crate::db::{get_connection, DbPool, WriteHandle};
use crate::errors::StorageError;
use crate::schema::{activities, spending_activity_exclusions};
use wealthfolio_spending::activity_exclusions::{
    ActivityExclusion, ActivityExclusionsRepositoryTrait,
};

pub struct ActivityExclusionsRepository {
    pool: Arc<DbPool>,
    writer: WriteHandle,
}

impl ActivityExclusionsRepository {
    pub fn new(pool: Arc<DbPool>, writer: WriteHandle) -> Self {
        Self { pool, writer }
    }
}

#[async_trait]
impl ActivityExclusionsRepositoryTrait for ActivityExclusionsRepository {
    async fn list_all(&self) -> Result<Vec<ActivityExclusion>> {
        let mut conn = get_connection(&self.pool).map_err(|e| anyhow::anyhow!(e))?;
        let rows: Vec<(String, Option<String>)> = spending_activity_exclusions::table
            .inner_join(activities::table)
            .select((
                spending_activity_exclusions::activity_id,
                activities::source_group_id,
            ))
            .load(&mut conn)
            .map_err(StorageError::from)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(rows
            .into_iter()
            .map(|(activity_id, group_id)| ActivityExclusion {
                activity_id,
                group_id,
            })
            .collect())
    }

    async fn set_excluded(&self, activity_id: &str, excluded: bool) -> Result<()> {
        let activity_id = activity_id.to_string();
        self.writer
            .exec(move |conn| {
                if excluded {
                    let now = chrono::Utc::now().to_rfc3339();
                    diesel::insert_into(spending_activity_exclusions::table)
                        .values((
                            spending_activity_exclusions::activity_id.eq(&activity_id),
                            spending_activity_exclusions::created_at.eq(&now),
                            spending_activity_exclusions::updated_at.eq(&now),
                        ))
                        .on_conflict(spending_activity_exclusions::activity_id)
                        .do_nothing()
                        .execute(conn)
                        .map_err(StorageError::from)?;
                } else {
                    diesel::delete(
                        spending_activity_exclusions::table
                            .filter(spending_activity_exclusions::activity_id.eq(&activity_id)),
                    )
                    .execute(conn)
                    .map_err(StorageError::from)?;
                }
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_pool, init, run_migrations, write_actor::spawn_writer};
    use crate::schema::sync_outbox;
    use diesel::sqlite::SqliteConnection;
    use tempfile::tempdir;

    fn setup_db() -> (Arc<DbPool>, WriteHandle) {
        std::env::set_var("CONNECT_API_URL", "http://test.local");
        let app_data = tempdir()
            .expect("tempdir")
            .keep()
            .to_string_lossy()
            .to_string();
        let db_path = init(&app_data).expect("init db");
        run_migrations(&db_path).expect("migrate db");
        let pool = create_pool(&db_path).expect("create pool");
        let writer = spawn_writer(pool.as_ref().clone()).expect("spawn writer");
        (pool, writer)
    }

    fn insert_activity(conn: &mut SqliteConnection, id: &str, group: Option<&str>) {
        let account_id = format!("account-{id}");
        diesel::sql_query(format!(
            "INSERT INTO accounts \
             (id, name, account_type, `group`, currency, is_default, is_active, created_at, updated_at, \
              platform_id, account_number, meta, provider, provider_account_id, is_archived, tracking_mode) \
             VALUES ('{account_id}', 'Account {id}', 'CASH', NULL, 'USD', 0, 1, CURRENT_TIMESTAMP, \
                     CURRENT_TIMESTAMP, NULL, NULL, NULL, NULL, NULL, 0, 'TRANSACTIONS')"
        ))
        .execute(conn)
        .expect("insert account");
        let group = group.map_or("NULL".to_string(), |g| format!("'{g}'"));
        diesel::sql_query(format!(
            "INSERT INTO activities \
             (id, account_id, asset_id, activity_type, activity_type_override, source_type, subtype, \
              status, activity_date, settlement_date, quantity, unit_price, amount, fee, currency, \
              fx_rate, notes, metadata, source_system, source_record_id, source_group_id, \
              idempotency_key, import_run_id, is_user_modified, needs_review, created_at, updated_at) \
             VALUES ('{id}', '{account_id}', NULL, 'TRANSFER_OUT', NULL, NULL, NULL, 'POSTED', \
                     '2026-01-01T00:00:00Z', NULL, NULL, NULL, '10', NULL, 'USD', NULL, \
                     NULL, NULL, 'MANUAL', NULL, {group}, NULL, NULL, 0, 0, \
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ))
        .execute(conn)
        .expect("insert activity");
    }

    #[tokio::test]
    async fn round_trips_with_the_transfer_group_and_stays_local() {
        let (pool, writer) = setup_db();
        {
            let mut conn = get_connection(&pool).expect("conn");
            insert_activity(&mut conn, "linked-leg", Some("pair-1"));
            insert_activity(&mut conn, "single-leg", None);
        }
        let repo = ActivityExclusionsRepository::new(pool.clone(), writer);

        repo.set_excluded("linked-leg", true)
            .await
            .expect("exclude");
        repo.set_excluded("single-leg", true)
            .await
            .expect("exclude");
        // Idempotent.
        repo.set_excluded("single-leg", true)
            .await
            .expect("exclude again");

        let mut rows = repo.list_all().await.expect("list");
        rows.sort_by(|a, b| a.activity_id.cmp(&b.activity_id));
        assert_eq!(
            rows,
            vec![
                ActivityExclusion {
                    activity_id: "linked-leg".to_string(),
                    group_id: Some("pair-1".to_string()),
                },
                ActivityExclusion {
                    activity_id: "single-leg".to_string(),
                    group_id: None,
                },
            ]
        );

        repo.set_excluded("single-leg", false)
            .await
            .expect("include");
        repo.set_excluded("single-leg", false)
            .await
            .expect("include again");
        assert_eq!(repo.list_all().await.expect("list").len(), 1);

        let mut conn = get_connection(&pool).expect("conn");
        assert_eq!(
            sync_outbox::table
                .count()
                .get_result::<i64>(&mut conn)
                .expect("count outbox"),
            0
        );
    }

    #[tokio::test]
    async fn deleting_the_activity_removes_its_exclusion() {
        let (pool, writer) = setup_db();
        {
            let mut conn = get_connection(&pool).expect("conn");
            insert_activity(&mut conn, "gone", None);
        }
        let repo = ActivityExclusionsRepository::new(pool.clone(), writer);
        repo.set_excluded("gone", true).await.expect("exclude");

        {
            let mut conn = get_connection(&pool).expect("conn");
            diesel::sql_query("PRAGMA foreign_keys = ON")
                .execute(&mut conn)
                .expect("enable foreign keys");
            diesel::delete(activities::table.find("gone"))
                .execute(&mut conn)
                .expect("delete activity");
            assert_eq!(
                spending_activity_exclusions::table
                    .count()
                    .get_result::<i64>(&mut conn)
                    .expect("count exclusions"),
                0
            );
        }
        assert!(repo.list_all().await.expect("list").is_empty());
    }
}
