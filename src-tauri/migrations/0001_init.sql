-- Schema v1: ping sessions and their probe rows.
CREATE TABLE sessions (
    id INTEGER PRIMARY KEY,
    target_input TEXT NOT NULL,
    resolved_ip TEXT NOT NULL,
    family TEXT NOT NULL,
    engine TEXT NOT NULL,
    interval_ms INTEGER NOT NULL,
    timeout_ms INTEGER NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT
);

CREATE TABLE probes (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    rtt_ms REAL,
    loss INTEGER NOT NULL,
    at TEXT NOT NULL
);

CREATE INDEX idx_probes_session_seq ON probes(session_id, seq);
