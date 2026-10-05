-- Per-activity "Exclude from Spending" switch.
--
-- A row here makes Spending treat the activity as ignored in every bucket
-- (spending, income, saving, refunds) while the ledger, balances, net worth,
-- performance and transfer links keep it. A side table rather than an
-- `activities` column, like `spending_activity_events`: the core activities
-- row stays free of spending-domain concepts. Local to this device (no sync).
CREATE TABLE IF NOT EXISTS spending_activity_exclusions (
  activity_id TEXT NOT NULL PRIMARY KEY,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  FOREIGN KEY (activity_id) REFERENCES activities(id) ON DELETE CASCADE
);
