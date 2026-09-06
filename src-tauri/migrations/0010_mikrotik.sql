CREATE TABLE mikrotik_profiles (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    use_tls INTEGER NOT NULL DEFAULT 1,
    allow_invalid_certs INTEGER NOT NULL DEFAULT 0,
    username TEXT NOT NULL,
    secret_key TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

CREATE TABLE mikrotik_sessions (
    id INTEGER PRIMARY KEY,
    profile_id INTEGER NOT NULL REFERENCES mikrotik_profiles(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT NOT NULL,
    board_name TEXT,
    routeros_version TEXT,
    architecture_name TEXT,
    update_status_json TEXT,
    firmware_status_json TEXT
);

CREATE TABLE mikrotik_snapshots (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES mikrotik_sessions(id) ON DELETE CASCADE,
    at TEXT NOT NULL,
    cpu_load REAL,
    mem_used_bytes INTEGER,
    mem_total_bytes INTEGER,
    uptime TEXT,
    warning TEXT,
    sensors_json TEXT,
    interfaces_json TEXT,
    vlans_json TEXT,
    bridge_vlans_json TEXT
);

CREATE INDEX idx_mikrotik_snapshots_session_at ON mikrotik_snapshots(session_id, at);
