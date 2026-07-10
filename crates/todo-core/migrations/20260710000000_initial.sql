CREATE TABLE todos (
    id            TEXT PRIMARY KEY,
    title         TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL DEFAULT 'todo'
                  CHECK (status IN ('todo', 'in_progress', 'done')),
    priority      INTEGER NOT NULL DEFAULT 0
                  CHECK (priority BETWEEN 0 AND 4),
    due_date      TEXT,
    completed_at  TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    deleted_at    TEXT,

    priority_rank INTEGER GENERATED ALWAYS AS
                  (CASE priority WHEN 0 THEN 5 ELSE priority END) VIRTUAL
);

CREATE INDEX idx_todos_active   ON todos(status, priority_rank, due_date)
                                WHERE deleted_at IS NULL;
CREATE INDEX idx_todos_due      ON todos(due_date) WHERE deleted_at IS NULL;
CREATE INDEX idx_todos_updated  ON todos(updated_at);

CREATE TABLE linear_links (
    todo_id            TEXT PRIMARY KEY REFERENCES todos(id) ON DELETE CASCADE,
    issue_id           TEXT NOT NULL UNIQUE,
    identifier         TEXT NOT NULL,
    url                TEXT NOT NULL,
    team_id            TEXT NOT NULL,
    last_pushed_status TEXT,
    linked_at          TEXT NOT NULL
);

CREATE TABLE sync_outbox (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    todo_id         TEXT NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,
    payload         TEXT NOT NULL DEFAULT '{}',
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL,
    last_error      TEXT,
    created_at      TEXT NOT NULL,
    completed_at    TEXT
);

CREATE UNIQUE INDEX idx_outbox_pending
    ON sync_outbox(todo_id, kind) WHERE completed_at IS NULL;

CREATE INDEX idx_outbox_ready
    ON sync_outbox(next_attempt_at) WHERE completed_at IS NULL;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE VIRTUAL TABLE todos_fts USING fts5(
    title, description,
    content = 'todos',
    content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER todos_fts_insert AFTER INSERT ON todos BEGIN
    INSERT INTO todos_fts(rowid, title, description)
    VALUES (new.rowid, new.title, new.description);
END;

CREATE TRIGGER todos_fts_delete AFTER DELETE ON todos BEGIN
    INSERT INTO todos_fts(todos_fts, rowid, title, description)
    VALUES ('delete', old.rowid, old.title, old.description);
END;

CREATE TRIGGER todos_fts_update AFTER UPDATE ON todos BEGIN
    INSERT INTO todos_fts(todos_fts, rowid, title, description)
    VALUES ('delete', old.rowid, old.title, old.description);
    INSERT INTO todos_fts(rowid, title, description)
    VALUES (new.rowid, new.title, new.description);
END;

