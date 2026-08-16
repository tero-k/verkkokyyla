CREATE TABLE traces (
    id INTEGER PRIMARY KEY,
    target_input TEXT NOT NULL,
    resolved_ip TEXT NOT NULL,
    family TEXT NOT NULL,
    engine TEXT NOT NULL,
    max_hops INTEGER NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running','completed','cancelled','error')),
    reached_target INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE trace_hops (
    id INTEGER PRIMARY KEY,
    trace_id INTEGER NOT NULL REFERENCES traces(id) ON DELETE CASCADE,
    hop INTEGER NOT NULL,
    address TEXT,
    hostname TEXT,
    rtt1_ms REAL,
    rtt2_ms REAL,
    rtt3_ms REAL,
    annotation TEXT,
    at TEXT NOT NULL
);

CREATE INDEX idx_trace_hops_trace ON trace_hops(trace_id, hop);
