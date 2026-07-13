CREATE TABLE email_links (
    todo_id     TEXT PRIMARY KEY REFERENCES todos(id) ON DELETE CASCADE,
    account_id  TEXT NOT NULL,
    gmail_id    TEXT NOT NULL,
    thread_id   TEXT NOT NULL DEFAULT '',
    subject     TEXT NOT NULL DEFAULT '',
    from_name   TEXT NOT NULL DEFAULT '',
    from_email  TEXT NOT NULL DEFAULT '',
    linked_at   TEXT NOT NULL
);

CREATE INDEX idx_email_links_msg ON email_links(account_id, gmail_id);
