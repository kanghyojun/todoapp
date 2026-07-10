-- todos 의 status CHECK 에 'deferred' 를 넣고 deferred_until 을 더한다.
-- SQLite 는 CHECK 를 바꾸려면 테이블을 다시 만들어야 한다. 그런데 todos 를
-- 지우면 ON DELETE CASCADE 가 linear_links·sync_outbox 를 함께 쓸어버린다.
-- 그래서 두 자식을 임시 테이블에 백업했다가 되돌린다. 외래키가 켜진
-- 트랜잭션 안에서도 안전하다. 실패하면 통째로 롤백된다.

CREATE TEMP TABLE _linear_links_backup AS SELECT * FROM linear_links;
CREATE TEMP TABLE _sync_outbox_backup AS SELECT * FROM sync_outbox;

DROP TRIGGER todos_fts_insert;
DROP TRIGGER todos_fts_delete;
DROP TRIGGER todos_fts_update;

CREATE TABLE todos_new (
    id            TEXT PRIMARY KEY,
    title         TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL DEFAULT 'todo'
                  CHECK (status IN ('todo', 'in_progress', 'done', 'deferred')),
    priority      INTEGER NOT NULL DEFAULT 0
                  CHECK (priority BETWEEN 0 AND 4),
    due_date      TEXT,
    completed_at  TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    deleted_at    TEXT,
    -- 보류 복귀일. 있으면 그날 todo 로 돌아온다. NULL 이면 계속 보류.
    -- status != 'deferred' 이면 항상 NULL 이다.
    deferred_until TEXT,

    priority_rank INTEGER GENERATED ALWAYS AS
                  (CASE priority WHEN 0 THEN 5 ELSE priority END) VIRTUAL
);

INSERT INTO todos_new
    (id, title, description, status, priority, due_date, completed_at,
     created_at, updated_at, deleted_at, deferred_until)
SELECT id, title, description, status, priority, due_date, completed_at,
       created_at, updated_at, deleted_at, NULL
FROM todos;

DROP TABLE todos;                       -- cascade 가 두 자식을 비운다 (백업됨)
ALTER TABLE todos_new RENAME TO todos;

DELETE FROM linear_links;
DELETE FROM sync_outbox;
INSERT INTO linear_links SELECT * FROM _linear_links_backup;
INSERT INTO sync_outbox  SELECT * FROM _sync_outbox_backup;

DROP TABLE _linear_links_backup;
DROP TABLE _sync_outbox_backup;

CREATE INDEX idx_todos_active   ON todos(status, priority_rank, due_date)
                                WHERE deleted_at IS NULL;
CREATE INDEX idx_todos_due      ON todos(due_date) WHERE deleted_at IS NULL;
CREATE INDEX idx_todos_updated  ON todos(updated_at);
CREATE INDEX idx_todos_deferred ON todos(deferred_until)
                                WHERE status = 'deferred' AND deleted_at IS NULL;

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

-- todos 를 다시 만들면서 rowid 가 바뀌었으니 FTS 색인을 재구축한다.
INSERT INTO todos_fts(todos_fts) VALUES('rebuild');
