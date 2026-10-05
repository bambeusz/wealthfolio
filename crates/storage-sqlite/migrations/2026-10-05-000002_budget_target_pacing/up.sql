-- How a budget target is expected to be spent across its month: 'linear'
-- (spread evenly, the previous behaviour) or 'monthly_on_day' (once, on
-- due_day, clamped to the month's length). Rules that span both columns
-- (due_day set only for monthly_on_day, group buffers linear) are enforced by
-- the budget service.
ALTER TABLE budget_targets
    ADD COLUMN pacing TEXT NOT NULL DEFAULT 'linear'
    CHECK (pacing IN ('linear', 'monthly_on_day'));
ALTER TABLE budget_targets
    ADD COLUMN due_day INTEGER
    CHECK (due_day IS NULL OR (due_day BETWEEN 1 AND 31));
