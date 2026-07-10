use chrono::DateTime;
use sqlx::Row;
use tempfile::NamedTempFile;
use todo_core::{
    CreateTodoInput, DomainEvent, Error, LinearLinkInput, Priority, Status, TodoCore, TodoFilter,
    TodoPatch,
};

async fn test_core() -> (NamedTempFile, TodoCore) {
    let database = NamedTempFile::new().expect("temporary database");
    let url = format!("sqlite://{}", database.path().display());
    let core = TodoCore::connect(&url).await.expect("initialize todo core");
    (database, core)
}

#[tokio::test]
async fn in_memory_database_keeps_migrations_and_data_on_one_connection() {
    let core = TodoCore::connect("sqlite::memory:")
        .await
        .expect("initialize in-memory core");
    assert_eq!(core.pool().options().get_idle_timeout(), None);
    assert_eq!(core.pool().options().get_max_lifetime(), None);

    let first = core.create_todo(CreateTodoInput::new("in memory one"));
    let second = core.create_todo(CreateTodoInput::new("in memory two"));
    let (first, second) = tokio::join!(first, second);
    let todo = first.expect("create first in-memory todo");
    second.expect("create second in-memory todo");

    assert_eq!(core.get_todo(todo.id).await.expect("read todo"), todo);
}

#[tokio::test]
async fn every_pool_connection_has_required_sqlite_pragmas() {
    let (_database, core) = test_core().await;
    let mut connections = Vec::new();
    for _ in 0..5 {
        connections.push(core.pool().acquire().await.expect("acquire connection"));
    }

    for connection in &mut connections {
        let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&mut **connection)
            .await
            .expect("read journal mode");
        let foreign_keys = sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&mut **connection)
            .await
            .expect("read foreign keys");
        let busy_timeout = sqlx::query_scalar::<_, i64>("PRAGMA busy_timeout")
            .fetch_one(&mut **connection)
            .await
            .expect("read busy timeout");
        let synchronous = sqlx::query_scalar::<_, i64>("PRAGMA synchronous")
            .fetch_one(&mut **connection)
            .await
            .expect("read synchronous mode");

        assert_eq!(journal_mode, "wal");
        assert_eq!(foreign_keys, 1);
        assert_eq!(busy_timeout, 5_000);
        assert_eq!(synchronous, 1);
    }
}

#[tokio::test]
async fn fts5_search_matches_title_and_description() {
    let (_database, core) = test_core().await;
    let title_match = core
        .create_todo(CreateTodoInput::new("배포 스크립트 정리"))
        .await
        .expect("create title match");
    let description_match = core
        .create_todo(CreateTodoInput {
            title: "운영 준비".to_owned(),
            description: "릴리스 배포 절차 확인".to_owned(),
            ..CreateTodoInput::default()
        })
        .await
        .expect("create description match");

    let results = core.search_todos("배포").await.expect("search FTS5");
    let ids: Vec<_> = results.into_iter().map(|todo| todo.id).collect();

    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&title_match.id));
    assert!(ids.contains(&description_match.id));
}

#[tokio::test]
async fn search_treats_user_punctuation_as_text_and_prefixes_last_token() {
    let (_database, core) = test_core().await;
    core.create_todo(CreateTodoInput {
        title: "배포 스크립트 고치기".to_owned(),
        description: "쿼터 \" 처리".to_owned(),
        ..CreateTodoInput::default()
    })
    .await
    .expect("create searchable todo");

    for query in [
        "\"",
        "배포\"",
        "\"배포\"",
        "\"\"\"",
        "todo:",
        "col:val",
        "a AND",
        "a OR b",
        "NOT",
        "NEAR(a b)",
        "*",
        "^x",
        "(abc",
        "{x}",
        "-",
        "a-b",
    ] {
        assert!(
            core.search_todos(query).await.is_ok(),
            "search input must be treated as text: {query:?}"
        );
    }

    let prefix_results = core
        .search_todos("배포 스크")
        .await
        .expect("search with last-token prefix");
    assert_eq!(prefix_results.len(), 1);

    // 따옴표로 감싸 검색하는 건 흔한 습관이다. 구문으로 해석하지 말고 글자로 찾아야 한다.
    let quoted_results = core
        .search_todos("\"배포\"")
        .await
        .expect("quoted search must find the word, not raise a syntax error");
    assert_eq!(quoted_results.len(), 1);
}

#[tokio::test]
async fn status_transitions_set_and_clear_completed_at() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("상태 전이"))
        .await
        .expect("create todo");

    let done = core
        .set_status(todo.id, Status::Done)
        .await
        .expect("mark done");
    let completed_at = done.completed_at.as_deref().expect("completed timestamp");
    let parsed = DateTime::parse_from_rfc3339(completed_at).expect("RFC3339 timestamp");
    assert_eq!(parsed.offset().local_minus_utc(), 0);

    let reopened = core
        .set_status(todo.id, Status::InProgress)
        .await
        .expect("reopen todo");
    assert_eq!(reopened.completed_at, None);
}

#[tokio::test]
async fn create_todo_accepts_initial_status_and_completes_done_todos() {
    let (_database, core) = test_core().await;
    let in_progress = core
        .create_todo(CreateTodoInput {
            title: "Linear import".to_owned(),
            status: Status::InProgress,
            ..CreateTodoInput::default()
        })
        .await
        .expect("create in-progress todo");
    assert_eq!(in_progress.status, Status::InProgress);
    assert_eq!(in_progress.completed_at, None);

    let done = core
        .create_todo(CreateTodoInput {
            title: "Already done".to_owned(),
            status: Status::Done,
            ..CreateTodoInput::default()
        })
        .await
        .expect("create done todo");
    assert_eq!(done.status, Status::Done);
    let completed_at = done.completed_at.expect("done completion timestamp");
    assert_eq!(
        DateTime::parse_from_rfc3339(&completed_at)
            .expect("RFC3339 completion timestamp")
            .offset()
            .local_minus_utc(),
        0
    );
}

#[tokio::test]
async fn create_and_update_trim_titles_and_reject_blank_titles() {
    let (_database, core) = test_core().await;

    let create_error = core
        .create_todo(CreateTodoInput::new("   "))
        .await
        .expect_err("blank create title must fail");
    assert!(matches!(create_error, Error::InvalidInput(_)));

    let todo = core
        .create_todo(CreateTodoInput::new("  trimmed title  "))
        .await
        .expect("create trimmed todo");
    assert_eq!(todo.title, "trimmed title");

    let update_error = core
        .update_todo(
            todo.id,
            TodoPatch {
                title: Some("  ".to_owned()),
                ..TodoPatch::default()
            },
        )
        .await
        .expect_err("blank update title must fail");
    assert!(matches!(update_error, Error::InvalidInput(_)));
    assert_eq!(
        core.get_todo(todo.id)
            .await
            .expect("read unchanged todo")
            .title,
        "trimmed title"
    );

    let updated = core
        .update_todo(
            todo.id,
            TodoPatch {
                title: Some("  renamed title  ".to_owned()),
                ..TodoPatch::default()
            },
        )
        .await
        .expect("update trimmed title");
    assert_eq!(updated.title, "renamed title");
}

#[tokio::test]
async fn default_order_uses_status_due_date_priority_rank_and_created_at() {
    let (_database, core) = test_core().await;
    let none = core
        .create_todo(CreateTodoInput::new("none"))
        .await
        .expect("create none");
    let low = core
        .create_todo(CreateTodoInput {
            title: "low".to_owned(),
            priority: Priority::Low,
            ..CreateTodoInput::default()
        })
        .await
        .expect("create low");
    let urgent = core
        .create_todo(CreateTodoInput {
            title: "urgent".to_owned(),
            priority: Priority::Urgent,
            ..CreateTodoInput::default()
        })
        .await
        .expect("create urgent");
    let in_progress = core
        .create_todo(CreateTodoInput::new("in progress"))
        .await
        .expect("create in progress");
    core.set_status(in_progress.id, Status::InProgress)
        .await
        .expect("set in progress");
    let done = core
        .create_todo(CreateTodoInput::new("done"))
        .await
        .expect("create done");
    core.set_status(done.id, Status::Done)
        .await
        .expect("set done");

    let todos = core
        .list_todos(TodoFilter::default())
        .await
        .expect("list todos");
    let ids: Vec<_> = todos.into_iter().map(|todo| todo.id).collect();

    assert_eq!(
        ids,
        vec![in_progress.id, urgent.id, low.id, none.id, done.id]
    );
}

#[tokio::test]
async fn soft_delete_hides_from_list_and_search_and_restore_returns_it() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("숨김검색어 작업"))
        .await
        .expect("create todo");

    core.delete_todo(todo.id).await.expect("soft delete");
    assert!(
        core.list_todos(TodoFilter::default())
            .await
            .expect("list")
            .is_empty()
    );
    assert!(
        core.search_todos("숨김검색어")
            .await
            .expect("search")
            .is_empty()
    );

    let restored = core.restore_todo(todo.id).await.expect("restore");
    assert_eq!(restored.deleted_at, None);
    assert_eq!(
        core.list_todos(TodoFilter::default())
            .await
            .expect("list restored")
            .len(),
        1
    );
    assert_eq!(
        core.search_todos("숨김검색어")
            .await
            .expect("search restored")
            .len(),
        1
    );
}

#[tokio::test]
async fn deleted_todo_cannot_be_read_updated_or_linked() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("original title"))
        .await
        .expect("create todo");
    core.delete_todo(todo.id).await.expect("delete todo");

    let get_error = core
        .get_todo(todo.id)
        .await
        .expect_err("deleted todo must not be readable");
    assert!(matches!(get_error, Error::NotFound(id) if id == todo.id));

    let update_error = core
        .update_todo(
            todo.id,
            TodoPatch {
                title: Some("changed title".to_owned()),
                ..TodoPatch::default()
            },
        )
        .await
        .expect_err("deleted todo must not be updated");
    assert!(matches!(update_error, Error::NotFound(id) if id == todo.id));

    let blank_update_error = core
        .update_todo(
            todo.id,
            TodoPatch {
                title: Some("   ".to_owned()),
                ..TodoPatch::default()
            },
        )
        .await
        .expect_err("deleted todo must be not found before patch validation");
    assert!(matches!(blank_update_error, Error::NotFound(id) if id == todo.id));

    let link_error = core
        .link_linear(todo.id, linear_link("deleted-unlinked-issue"))
        .await
        .expect_err("deleted todo must not be linked");
    assert!(matches!(link_error, Error::NotFound(id) if id == todo.id));

    let stored_title = sqlx::query_scalar::<_, String>("SELECT title FROM todos WHERE id = ?")
        .bind(todo.id.to_string())
        .fetch_one(core.pool())
        .await
        .expect("read deleted row directly");
    assert_eq!(stored_title, "original title");
    let link_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM linear_links")
        .fetch_one(core.pool())
        .await
        .expect("count links");
    assert_eq!(link_count, 0);
}

#[tokio::test]
async fn linked_done_enqueues_exactly_one_outbox_row() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("linked"))
        .await
        .expect("create todo");
    core.link_linear(todo.id, linear_link("issue-linked"))
        .await
        .expect("link issue");

    core.set_status(todo.id, Status::Done)
        .await
        .expect("first done");
    core.set_status(todo.id, Status::Done)
        .await
        .expect("second done");

    let row = sqlx::query("SELECT COUNT(*) AS count FROM sync_outbox WHERE todo_id = ?")
        .bind(todo.id.to_string())
        .fetch_one(core.pool())
        .await
        .expect("count outbox rows");
    assert_eq!(row.get::<i64, _>("count"), 1);
    let kind = sqlx::query_scalar::<_, String>("SELECT kind FROM sync_outbox WHERE todo_id = ?")
        .bind(todo.id.to_string())
        .fetch_one(core.pool())
        .await
        .expect("read outbox kind");
    assert_eq!(kind, "linear_complete");
}

#[tokio::test]
async fn deleted_linked_todo_cannot_be_completed_or_enqueued() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("deleted linked"))
        .await
        .expect("create todo");
    core.link_linear(todo.id, linear_link("deleted-linked-issue"))
        .await
        .expect("link issue");
    core.delete_todo(todo.id).await.expect("delete todo");

    let error = core
        .set_status(todo.id, Status::Done)
        .await
        .expect_err("deleted todo must not be completed");
    assert!(matches!(error, Error::NotFound(id) if id == todo.id));

    let count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_outbox WHERE completed_at IS NULL")
            .fetch_one(core.pool())
            .await
            .expect("count pending outbox rows");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn unlinked_done_does_not_enqueue_outbox_row() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("unlinked"))
        .await
        .expect("create todo");

    core.set_status(todo.id, Status::Done)
        .await
        .expect("mark done");

    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_outbox")
        .fetch_one(core.pool())
        .await
        .expect("count outbox rows");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn duplicate_linear_issue_returns_issue_already_linked() {
    let (_database, core) = test_core().await;
    let first = core
        .create_todo(CreateTodoInput::new("first"))
        .await
        .expect("create first");
    let second = core
        .create_todo(CreateTodoInput::new("second"))
        .await
        .expect("create second");
    core.link_linear(first.id, linear_link("same-issue"))
        .await
        .expect("link first");

    let error = core
        .link_linear(second.id, linear_link("same-issue"))
        .await
        .expect_err("duplicate issue must fail");
    assert!(matches!(error, Error::IssueAlreadyLinked));
}

#[tokio::test]
async fn link_linear_preserves_local_title_and_description() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput {
            title: "내 제목".to_owned(),
            description: "내 설명".to_owned(),
            ..CreateTodoInput::default()
        })
        .await
        .expect("create todo");

    core.link_linear(todo.id, linear_link("preserve-issue"))
        .await
        .expect("link issue");
    let after = core.get_todo(todo.id).await.expect("get linked todo");

    assert_eq!(after.title, "내 제목");
    assert_eq!(after.description, "내 설명");
}

#[tokio::test]
async fn domain_events_fire_after_create_update_delete_and_restore() {
    let (_database, core) = test_core().await;
    let mut events = core.subscribe();

    let todo = core
        .create_todo(CreateTodoInput::new("events"))
        .await
        .expect("create todo");
    assert_eq!(
        events.recv().await.expect("create event"),
        DomainEvent::TodoCreated(todo.id)
    );
    assert!(core.get_todo(todo.id).await.is_ok());

    core.update_todo(
        todo.id,
        TodoPatch {
            title: Some("updated".to_owned()),
            ..TodoPatch::default()
        },
    )
    .await
    .expect("update todo");
    assert_eq!(
        events.recv().await.expect("update event"),
        DomainEvent::TodoUpdated(todo.id)
    );
    assert_eq!(
        core.get_todo(todo.id).await.expect("read update").title,
        "updated"
    );

    core.delete_todo(todo.id).await.expect("delete todo");
    assert_eq!(
        events.recv().await.expect("delete event"),
        DomainEvent::TodoDeleted(todo.id)
    );
    assert!(matches!(
        core.get_todo(todo.id).await,
        Err(Error::NotFound(id)) if id == todo.id
    ));

    core.restore_todo(todo.id).await.expect("restore todo");
    assert_eq!(
        events.recv().await.expect("restore event"),
        DomainEvent::TodoRestored(todo.id)
    );
    assert_eq!(
        core.get_todo(todo.id)
            .await
            .expect("read restore")
            .deleted_at,
        None
    );
}

fn linear_link(issue_id: &str) -> LinearLinkInput {
    LinearLinkInput {
        issue_id: issue_id.to_owned(),
        identifier: "PI-1234".to_owned(),
        url: "https://linear.app/example/issue/PI-1234".to_owned(),
        team_id: "team-1".to_owned(),
    }
}
