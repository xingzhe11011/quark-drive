CREATE TABLE IF NOT EXISTS accounts (
    id            TEXT PRIMARY KEY,
    uid           TEXT NOT NULL DEFAULT '',
    nickname      TEXT NOT NULL DEFAULT '',
    avatar_url    TEXT NOT NULL DEFAULT '',
    vip_type      TEXT NOT NULL DEFAULT '',
    total_space   INTEGER NOT NULL DEFAULT 0,
    used_space    INTEGER NOT NULL DEFAULT 0,
    cookie_ref    TEXT NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    last_check_at INTEGER
);

CREATE TABLE IF NOT EXISTS file_cache (
    account_id TEXT NOT NULL,
    fid        TEXT NOT NULL,
    pdir_fid   TEXT NOT NULL,
    name       TEXT NOT NULL,
    is_dir     INTEGER NOT NULL,
    size       INTEGER NOT NULL DEFAULT 0,
    category   INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    cached_at  INTEGER NOT NULL,
    PRIMARY KEY (account_id, fid)
);

CREATE INDEX IF NOT EXISTS idx_file_cache_dir
    ON file_cache(account_id, pdir_fid);

CREATE TABLE IF NOT EXISTS transfers (
    id               TEXT PRIMARY KEY,
    account_id       TEXT NOT NULL,
    kind             TEXT NOT NULL,
    status           TEXT NOT NULL,
    phase            TEXT NOT NULL DEFAULT 'queued',
    fid              TEXT,
    local_path       TEXT NOT NULL,
    remote_name      TEXT NOT NULL,
    remote_parent_fid TEXT NOT NULL DEFAULT '0',
    total_bytes      INTEGER NOT NULL DEFAULT 0,
    done_bytes       INTEGER NOT NULL DEFAULT 0,
    error_code       TEXT,
    error_msg        TEXT,
    session_json     TEXT,
    priority         INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_transfers_status
    ON transfers(status, priority DESC, created_at);

CREATE TABLE IF NOT EXISTS transfer_parts (
    transfer_id TEXT NOT NULL,
    part_number INTEGER NOT NULL,
    offset      INTEGER NOT NULL,
    size        INTEGER NOT NULL,
    etag        TEXT,
    status      TEXT NOT NULL DEFAULT 'pending',
    PRIMARY KEY (transfer_id, part_number)
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
