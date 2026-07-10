CREATE TABLE gmail_accounts (
    id             TEXT PRIMARY KEY,        -- 내부 UUID
    email          TEXT NOT NULL UNIQUE,
    color          TEXT NOT NULL,           -- 계정 구분용 accent
    history_id     TEXT,                    -- 마지막으로 반영한 Gmail historyId
    sync_state     TEXT NOT NULL DEFAULT 'idle'
                   CHECK (sync_state IN ('idle', 'syncing', 'needs_auth', 'error')),
    last_error     TEXT,
    last_synced_at TEXT,
    added_at       TEXT NOT NULL
);

CREATE TABLE gmail_messages (
    account_id      TEXT NOT NULL REFERENCES gmail_accounts(id) ON DELETE CASCADE,
    gmail_id        TEXT NOT NULL,          -- Gmail 메시지 id
    thread_id       TEXT NOT NULL,          -- 스레드 그룹핑용(미래 대비)
    from_name       TEXT NOT NULL DEFAULT '',
    from_email      TEXT NOT NULL DEFAULT '',
    subject         TEXT NOT NULL DEFAULT '',
    snippet         TEXT NOT NULL DEFAULT '',
    internal_date   INTEGER NOT NULL,       -- Gmail internalDate(ms), 정렬 키
    in_inbox        INTEGER NOT NULL DEFAULT 0,  -- INBOX 라벨 유무
    is_unread       INTEGER NOT NULL DEFAULT 0,  -- UNREAD 라벨 유무
    body_text       TEXT,                   -- 지연 로드, NULL = 미페치
    body_html       TEXT,
    body_fetched_at TEXT,
    updated_at      TEXT NOT NULL,
    PRIMARY KEY (account_id, gmail_id)
);

CREATE INDEX idx_gmail_msgs_list  ON gmail_messages(internal_date DESC);
CREATE INDEX idx_gmail_msgs_inbox ON gmail_messages(in_inbox, internal_date DESC);

CREATE TABLE gmail_outbox (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id      TEXT NOT NULL REFERENCES gmail_accounts(id) ON DELETE CASCADE,
    gmail_id        TEXT NOT NULL,
    kind            TEXT NOT NULL,          -- archive | mark_read | mark_unread
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL,
    last_error      TEXT,
    created_at      TEXT NOT NULL,
    completed_at    TEXT
);

CREATE UNIQUE INDEX idx_gmail_outbox_pending
    ON gmail_outbox(account_id, gmail_id, kind) WHERE completed_at IS NULL;
CREATE INDEX idx_gmail_outbox_ready
    ON gmail_outbox(next_attempt_at) WHERE completed_at IS NULL;
