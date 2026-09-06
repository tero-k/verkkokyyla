-- Allow benchmark results to be persisted alongside single-file and full-page results.

ALTER TABLE download_speed_sessions RENAME TO download_speed_sessions_old;

CREATE TABLE download_speed_sessions (
    id INTEGER PRIMARY KEY,
    url TEXT NOT NULL,
    mode TEXT NOT NULL CHECK(mode IN ('single', 'page', 'benchmark')),
    http_settings_json TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running', 'completed', 'error')),
    average_mbps REAL NOT NULL DEFAULT 0,
    total_time_ms INTEGER NOT NULL DEFAULT 0,
    result_json TEXT NOT NULL
);

INSERT INTO download_speed_sessions SELECT * FROM download_speed_sessions_old;

DROP INDEX IF EXISTS idx_download_speed_sessions_started_at;
CREATE INDEX idx_download_speed_sessions_started_at ON download_speed_sessions(started_at);

DROP TABLE download_speed_sessions_old;
