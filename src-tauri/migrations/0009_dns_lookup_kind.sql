-- Allow DNS lookup runs to be persisted alongside benchmark and diagnostics runs.
-- SQLite rewrites foreign keys that reference a renamed table, so both
-- dns_runs and its child dns_run_targets are rebuilt here.

ALTER TABLE dns_runs RENAME TO dns_runs_old;

CREATE TABLE dns_runs (
    id INTEGER PRIMARY KEY,
    target_input TEXT NOT NULL,
    kind TEXT CHECK(kind IN ('benchmark', 'diagnostics', 'lookup')),
    config_json TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT NOT NULL DEFAULT 'running'
);

INSERT INTO dns_runs SELECT * FROM dns_runs_old;

-- dns_run_targets now points at dns_runs_old; rebuild it against the new table.
CREATE TABLE dns_run_targets_new (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES dns_runs(id) ON DELETE CASCADE,
    target TEXT NOT NULL,
    protocol TEXT,
    metrics_json TEXT NOT NULL
);

INSERT INTO dns_run_targets_new SELECT * FROM dns_run_targets;

DROP TABLE dns_run_targets;
ALTER TABLE dns_run_targets_new RENAME TO dns_run_targets;
CREATE INDEX idx_dns_run_targets_run ON dns_run_targets(run_id);

DROP TABLE dns_runs_old;
