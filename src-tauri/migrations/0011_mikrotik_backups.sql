CREATE TABLE mikrotik_backups (
    id INTEGER PRIMARY KEY,
    profile_id INTEGER REFERENCES mikrotik_profiles(id) ON DELETE SET NULL,
    profile_name TEXT NOT NULL,
    name TEXT NOT NULL,
    backup_path TEXT NOT NULL,
    export_path TEXT,
    created_at TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    has_rsc_export INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_mikrotik_backups_created_at ON mikrotik_backups(created_at DESC);
CREATE INDEX idx_mikrotik_backups_profile ON mikrotik_backups(profile_id);
