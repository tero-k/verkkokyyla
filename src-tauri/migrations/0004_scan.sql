CREATE TABLE scans (
    id INTEGER PRIMARY KEY,
    interface_name TEXT NOT NULL,
    cidr TEXT NOT NULL,
    tcp_fallback INTEGER NOT NULL DEFAULT 1,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running','completed','cancelled','error')),
    host_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE scan_hosts (
    id INTEGER PRIMARY KEY,
    scan_id INTEGER NOT NULL REFERENCES scans(id) ON DELETE CASCADE,
    ip TEXT NOT NULL,
    mac TEXT,
    vendor TEXT,
    hostname TEXT,
    found_by TEXT NOT NULL CHECK(found_by IN ('ping','tcp','arp')),
    at TEXT NOT NULL
);

CREATE INDEX idx_scan_hosts_scan ON scan_hosts(scan_id, ip);
