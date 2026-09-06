CREATE TABLE dns_runs (
    id INTEGER PRIMARY KEY,
    target_input TEXT NOT NULL,
    kind TEXT CHECK(kind IN ('benchmark','diagnostics')),
    config_json TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT NOT NULL DEFAULT 'running'
);

CREATE TABLE dns_run_targets (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES dns_runs(id) ON DELETE CASCADE,
    target TEXT NOT NULL,
    protocol TEXT,
    metrics_json TEXT NOT NULL
);

CREATE INDEX idx_dns_run_targets_run ON dns_run_targets(run_id);
