CREATE TABLE opnsense_profiles (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 443,
    allow_invalid_certs INTEGER NOT NULL DEFAULT 0,
    secret_key TEXT NOT NULL UNIQUE,
    ssh_port INTEGER NOT NULL DEFAULT 22,
    ssh_host_key TEXT,
    created_at TEXT NOT NULL
);
