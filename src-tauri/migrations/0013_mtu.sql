CREATE TABLE mtu_runs (
    id INTEGER PRIMARY KEY,
    target_input TEXT NOT NULL,
    resolved_ip TEXT NOT NULL,
    method TEXT NOT NULL,
    floor_mtu INTEGER NOT NULL,
    ceiling_mtu INTEGER NOT NULL,
    result_kind TEXT NOT NULL DEFAULT 'running' CHECK(result_kind IN ('running','exact','lower-bound','unreachable','failed','cancelled')),
    result_mtu INTEGER,
    detail TEXT,
    probes_sent INTEGER NOT NULL DEFAULT 0,
    started_at TEXT NOT NULL,
    ended_at TEXT
);
CREATE TABLE mtu_probes (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES mtu_runs(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    payload_size INTEGER NOT NULL,
    mtu_size INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK(outcome IN ('ok','too-big','timeout','error')),
    rtt_ms REAL,
    hint_mtu INTEGER,
    message TEXT,
    at TEXT NOT NULL
);
CREATE INDEX idx_mtu_probes_run ON mtu_probes(run_id, seq);
