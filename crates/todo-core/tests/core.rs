use chrono::{DateTime, Local};
use sqlx::Row;
use tempfile::NamedTempFile;
use todo_core::{
    CreateTodoInput, DomainEvent, EmailLinkInput, EmailRef, Error, LinearLinkInput, LinearRef,
    Priority, Status, TodoCore, TodoFilter, TodoPatch,
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
async fn create_assigns_a_unique_four_char_code() {
    let (_database, core) = test_core().await;
    let a = core
        .create_todo(CreateTodoInput::new("code a"))
        .await
        .expect("create a");
    let b = core
        .create_todo(CreateTodoInput::new("code b"))
        .await
        .expect("create b");
    assert_eq!(
        a.code.len(),
        4,
        "code should be four chars, got {:?}",
        a.code
    );
    assert_ne!(a.code, b.code, "codes must differ");
}

#[tokio::test]
async fn id_for_code_resolves_case_insensitively_and_misses_gracefully() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("resolve me"))
        .await
        .expect("create todo");

    assert_eq!(
        core.id_for_code(&todo.code).await.expect("lookup"),
        Some(todo.id)
    );
    assert_eq!(
        core.id_for_code(&format!("#{}", todo.code.to_uppercase()))
            .await
            .expect("lookup upper"),
        Some(todo.id),
        "code lookup must ignore case and a leading #"
    );
    assert_eq!(
        core.id_for_code("2345").await.expect("miss"),
        None,
        "unknown code is a graceful miss"
    );
}

#[tokio::test]
async fn reconnect_backfills_codes_missing_from_older_rows() {
    let database = NamedTempFile::new().expect("temporary database");
    let url = format!("sqlite://{}", database.path().display());
    let core = TodoCore::connect(&url).await.expect("connect");
    let todo = core
        .create_todo(CreateTodoInput::new("legacy row"))
        .await
        .expect("create todo");
    // 코드 컬럼이 막 생긴 예전 행을 흉내낸다.
    sqlx::query("UPDATE todos SET code = NULL WHERE id = ?")
        .bind(todo.id.to_string())
        .execute(core.pool())
        .await
        .expect("null out code");
    drop(core);

    let core = TodoCore::connect(&url).await.expect("reconnect");
    let refreshed = core.get_todo(todo.id).await.expect("read todo");
    assert_eq!(
        refreshed.code.len(),
        4,
        "reconnect should backfill the code"
    );
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

    let results = core
        .list_todos(TodoFilter {
            query: Some("배포".to_owned()),
            ..TodoFilter::default()
        })
        .await
        .expect("search FTS5");
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
            core.list_todos(TodoFilter {
                query: Some(query.to_owned()),
                ..TodoFilter::default()
            })
            .await
            .is_ok(),
            "search input must be treated as text: {query:?}"
        );
    }

    let prefix_results = core
        .list_todos(TodoFilter {
            query: Some("배포 스크".to_owned()),
            ..TodoFilter::default()
        })
        .await
        .expect("search with last-token prefix");
    assert_eq!(prefix_results.len(), 1);

    // 따옴표로 감싸 검색하는 건 흔한 습관이다. 구문으로 해석하지 말고 글자로 찾아야 한다.
    let quoted_results = core
        .list_todos(TodoFilter {
            query: Some("\"배포\"".to_owned()),
            ..TodoFilter::default()
        })
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
async fn remote_completion_marks_done_without_enqueuing_outbox() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("Linear closes this"))
        .await
        .expect("create todo");
    core.link_linear(
        todo.id,
        LinearLinkInput {
            issue_id: "issue-remote-done".to_owned(),
            identifier: "PI-200".to_owned(),
            url: "https://linear.app/acme/issue/PI-200/test".to_owned(),
            team_id: "team-1".to_owned(),
        },
    )
    .await
    .expect("link todo");

    let completed = core
        .mark_done_from_remote(todo.id)
        .await
        .expect("apply remote completion");
    assert_eq!(completed.status, Status::Done);
    assert!(completed.completed_at.is_some());
    let outbox_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_outbox")
        .fetch_one(core.pool())
        .await
        .expect("count outbox rows");
    assert_eq!(outbox_count, 0);
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
async fn query_combines_with_status_and_priority_filters() {
    let (_database, core) = test_core().await;
    let matching_status = core
        .create_todo(CreateTodoInput::new("배포 진행 작업"))
        .await
        .expect("create matching status todo");
    core.set_status(matching_status.id, Status::InProgress)
        .await
        .expect("set matching status");
    let matching_priority = core
        .create_todo(CreateTodoInput {
            title: "배포 긴급 작업".to_owned(),
            priority: Priority::Urgent,
            ..CreateTodoInput::default()
        })
        .await
        .expect("create matching priority todo");
    core.create_todo(CreateTodoInput::new("배포 일반 작업"))
        .await
        .expect("create unmatched todo");
    let other_in_progress = core
        .create_todo(CreateTodoInput::new("문서 진행 작업"))
        .await
        .expect("create non-query status todo");
    core.set_status(other_in_progress.id, Status::InProgress)
        .await
        .expect("set non-query status");

    let by_status = core
        .list_todos(TodoFilter {
            query: Some("배포".to_owned()),
            status: Some(Status::InProgress),
            ..TodoFilter::default()
        })
        .await
        .expect("query with status");
    assert_eq!(by_status.len(), 1);
    assert_eq!(by_status[0].id, matching_status.id);

    let by_priority = core
        .list_todos(TodoFilter {
            query: Some("배포".to_owned()),
            priority: Some(Priority::Urgent),
            ..TodoFilter::default()
        })
        .await
        .expect("query with priority");
    assert_eq!(by_priority.len(), 1);
    assert_eq!(by_priority[0].id, matching_priority.id);
}

#[tokio::test]
async fn query_uses_relevance_order_while_plain_list_uses_default_order() {
    let (_database, core) = test_core().await;
    let relevant = core
        .create_todo(CreateTodoInput::new("deploy deploy deploy"))
        .await
        .expect("create relevant todo");
    let status_first = core
        .create_todo(CreateTodoInput::new("deploy"))
        .await
        .expect("create status-first todo");
    core.set_status(status_first.id, Status::InProgress)
        .await
        .expect("set in progress");

    let search = core
        .list_todos(TodoFilter {
            query: Some("deploy".to_owned()),
            ..TodoFilter::default()
        })
        .await
        .expect("relevance list");
    assert_eq!(search[0].id, relevant.id);

    let plain = core
        .list_todos(TodoFilter::default())
        .await
        .expect("default list");
    assert_eq!(plain[0].id, status_first.id);
}

#[tokio::test]
async fn limit_clamps_to_one_thousand() {
    let (_database, core) = test_core().await;
    for index in 0..1_001 {
        core.create_todo(CreateTodoInput::new(format!("todo {index:04}")))
            .await
            .expect("create todo for limit test");
    }

    let todos = core
        .list_todos(TodoFilter {
            limit: Some(5_000),
            ..TodoFilter::default()
        })
        .await
        .expect("clamped list");
    assert_eq!(todos.len(), 1_000);
}

#[tokio::test]
async fn offset_without_limit_and_stable_page_window_work() {
    let (_database, core) = test_core().await;
    for title in ["one", "two", "three", "four", "five"] {
        core.create_todo(CreateTodoInput::new(title))
            .await
            .expect("create page todo");
    }
    let all = core
        .list_todos(TodoFilter::default())
        .await
        .expect("full list");

    let offset_only = core
        .list_todos(TodoFilter {
            offset: Some(1),
            ..TodoFilter::default()
        })
        .await
        .expect("offset-only list");
    assert_eq!(offset_only, all[1..]);

    let filter = TodoFilter {
        limit: Some(2),
        offset: Some(1),
        ..TodoFilter::default()
    };
    let first_page = core.list_todos(filter.clone()).await.expect("first page");
    let second_page = core.list_todos(filter).await.expect("second page");
    assert_eq!(first_page, all[1..3]);
    assert_eq!(second_page, first_page);
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
        core.list_todos(TodoFilter {
            query: Some("숨김검색어".to_owned()),
            ..TodoFilter::default()
        })
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
        core.list_todos(TodoFilter {
            query: Some("숨김검색어".to_owned()),
            ..TodoFilter::default()
        })
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
async fn atomic_update_changes_title_status_and_enqueues_once_for_linked_todo() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("before"))
        .await
        .expect("create linked todo");
    core.link_linear(todo.id, linear_link("atomic-update-issue"))
        .await
        .expect("link todo");
    let mut events = core.subscribe();

    for attempt in 0..2 {
        let updated = core
            .update_todo(
                todo.id,
                TodoPatch {
                    title: Some("after".to_owned()),
                    status: Some(Status::Done),
                    ..TodoPatch::default()
                },
            )
            .await
            .expect("atomic update");
        assert_eq!(updated.title, "after");
        assert_eq!(updated.status, Status::Done);
        assert!(updated.completed_at.is_some());
        assert_eq!(
            events.recv().await.expect("todo updated event"),
            DomainEvent::TodoUpdated(todo.id)
        );
        if attempt == 0 {
            assert_eq!(
                events.recv().await.expect("sync state event"),
                DomainEvent::SyncStateChanged
            );
        } else {
            assert!(matches!(
                events.try_recv(),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
            ));
        }
    }

    let outbox_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_outbox WHERE todo_id = ?")
            .bind(todo.id.to_string())
            .fetch_one(core.pool())
            .await
            .expect("count atomic update outbox rows");
    assert_eq!(outbox_count, 1);
}

#[tokio::test]
async fn update_moving_out_of_done_clears_completed_at() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput {
            title: "done todo".to_owned(),
            status: Status::Done,
            ..CreateTodoInput::default()
        })
        .await
        .expect("create done todo");
    assert!(todo.completed_at.is_some());

    let reopened = core
        .update_todo(
            todo.id,
            TodoPatch {
                status: Some(Status::Todo),
                ..TodoPatch::default()
            },
        )
        .await
        .expect("move out of done");
    assert_eq!(reopened.status, Status::Todo);
    assert_eq!(reopened.completed_at, None);
}

#[tokio::test]
async fn uncompleting_a_linked_todo_does_not_enqueue_another_outbox_row() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("reopen linked"))
        .await
        .expect("create todo");
    core.link_linear(todo.id, linear_link("reopen-linked-issue"))
        .await
        .expect("link todo");
    core.set_status(todo.id, Status::Done)
        .await
        .expect("mark done");
    sqlx::query("UPDATE sync_outbox SET completed_at = created_at WHERE todo_id = ?")
        .bind(todo.id.to_string())
        .execute(core.pool())
        .await
        .expect("complete original outbox row");

    core.set_status(todo.id, Status::InProgress)
        .await
        .expect("reopen todo");
    let pending = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sync_outbox WHERE todo_id = ? AND completed_at IS NULL",
    )
    .bind(todo.id.to_string())
    .fetch_one(core.pool())
    .await
    .expect("count pending rows");
    assert_eq!(pending, 0);
}

/// 완료했다가 워커가 push 하기 전에 되돌리면, 아직 밀어내지 않은
/// linear_complete 아웃박스 행을 취소해야 한다. 안 그러면 워커가 뒤늦게
/// 그 행을 밀어내 Linear 이슈를 닫아버린다.
#[tokio::test]
async fn reopening_before_flush_cancels_pending_outbox_row() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("reopen before flush"))
        .await
        .expect("create todo");
    core.link_linear(todo.id, linear_link("reopen-before-flush-issue"))
        .await
        .expect("link todo");
    core.set_status(todo.id, Status::Done)
        .await
        .expect("mark done");

    // 워커가 push 하기 전이라 아웃박스 행은 아직 pending 이다.
    core.set_status(todo.id, Status::InProgress)
        .await
        .expect("reopen todo");

    let pending = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sync_outbox WHERE todo_id = ? AND completed_at IS NULL",
    )
    .bind(todo.id.to_string())
    .fetch_one(core.pool())
    .await
    .expect("count pending rows");
    assert_eq!(pending, 0);
}

/// 보류(defer)도 done 을 벗어나는 전이다. 워커 flush 전에 보류로 보내면
/// 마찬가지로 pending linear_complete 를 취소해야 한다.
#[tokio::test]
async fn deferring_before_flush_cancels_pending_outbox_row() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("defer before flush"))
        .await
        .expect("create todo");
    core.link_linear(todo.id, linear_link("defer-before-flush-issue"))
        .await
        .expect("link todo");
    core.set_status(todo.id, Status::Done)
        .await
        .expect("mark done");

    core.defer_todo(todo.id, "")
        .await
        .expect("defer todo");

    let pending = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sync_outbox WHERE todo_id = ? AND completed_at IS NULL",
    )
    .bind(todo.id.to_string())
    .fetch_one(core.pool())
    .await
    .expect("count pending rows");
    assert_eq!(pending, 0);
}

#[tokio::test]
async fn deleted_todo_is_not_found_before_due_date_validation() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("deleted date"))
        .await
        .expect("create todo");
    core.delete_todo(todo.id).await.expect("delete todo");

    let error = core
        .update_todo(
            todo.id,
            TodoPatch {
                due_date: Some(Some("쓰레기".to_owned())),
                ..TodoPatch::default()
            },
        )
        .await
        .expect_err("deleted todo must win over invalid due date");
    assert!(matches!(error, Error::NotFound(id) if id == todo.id));
}

#[tokio::test]
async fn update_parses_due_date_text_clears_empty_and_rejects_garbage() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("due date update"))
        .await
        .expect("create todo");

    let dated = core
        .update_todo(
            todo.id,
            TodoPatch {
                due_date: Some(Some("2026-04-20".to_owned())),
                ..TodoPatch::default()
            },
        )
        .await
        .expect("set due date");
    assert_eq!(dated.due_date.as_deref(), Some("2026-04-20"));

    let cleared = core
        .update_todo(
            todo.id,
            TodoPatch {
                due_date: Some(Some(String::new())),
                ..TodoPatch::default()
            },
        )
        .await
        .expect("clear due date");
    assert_eq!(cleared.due_date, None);

    let error = core
        .update_todo(
            todo.id,
            TodoPatch {
                due_date: Some(Some("garbage".to_owned())),
                ..TodoPatch::default()
            },
        )
        .await
        .expect_err("invalid due date must fail");
    assert!(matches!(error, Error::InvalidInput(_)));
}

#[tokio::test]
async fn update_emits_exactly_one_todo_updated_event() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("single event"))
        .await
        .expect("create todo");
    let mut events = core.subscribe();

    core.update_todo(
        todo.id,
        TodoPatch {
            title: Some("changed".to_owned()),
            status: Some(Status::Done),
            ..TodoPatch::default()
        },
    )
    .await
    .expect("update todo once");

    assert_eq!(
        events.recv().await.expect("todo updated event"),
        DomainEvent::TodoUpdated(todo.id)
    );
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
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
async fn reads_carry_the_linear_link_when_present() {
    let (_database, core) = test_core().await;
    let linked = core
        .create_todo(CreateTodoInput::new("linked"))
        .await
        .expect("create linked");
    let bare = core
        .create_todo(CreateTodoInput::new("bare"))
        .await
        .expect("create bare");
    core.link_linear(linked.id, linear_link("read-issue"))
        .await
        .expect("link issue");

    // 단건 읽기는 링크를 실어 온다.
    let got = core.get_todo(linked.id).await.expect("get linked");
    assert_eq!(
        got.linear,
        Some(LinearRef {
            identifier: "PI-1234".to_owned(),
            url: "https://linear.app/example/issue/PI-1234".to_owned(),
        }),
    );

    // 링크 없는 todo 는 None 이다.
    assert_eq!(core.get_todo(bare.id).await.expect("get bare").linear, None,);

    // 목록 읽기도 같은 정보를 실어 온다.
    let listed = core
        .list_todos(TodoFilter::default())
        .await
        .expect("list todos");
    let linked_in_list = listed
        .iter()
        .find(|todo| todo.id == linked.id)
        .expect("linked todo in list");
    assert_eq!(
        linked_in_list.linear.as_ref().map(|link| &link.identifier),
        Some(&"PI-1234".to_owned())
    );
    let bare_in_list = listed
        .iter()
        .find(|todo| todo.id == bare.id)
        .expect("bare todo in list");
    assert_eq!(bare_in_list.linear, None);
}

#[tokio::test]
async fn create_todo_from_email_persists_and_reads_the_email_snapshot() {
    let (_database, core) = test_core().await;
    let created = core
        .create_todo_from_email(
            "메일 답장하기".to_owned(),
            EmailLinkInput {
                account_id: "account-1".to_owned(),
                gmail_id: "message-1".to_owned(),
                thread_id: "thread-1".to_owned(),
                subject: "회의 일정".to_owned(),
                from_name: "Kim".to_owned(),
                from_email: "kim@example.com".to_owned(),
            },
        )
        .await
        .expect("create todo from email");

    let expected = EmailRef {
        account_id: "account-1".to_owned(),
        gmail_id: "message-1".to_owned(),
        thread_id: "thread-1".to_owned(),
        subject: "회의 일정".to_owned(),
        from_name: "Kim".to_owned(),
        from_email: "kim@example.com".to_owned(),
    };
    assert_eq!(created.title, "메일 답장하기");
    assert_eq!(created.email, Some(expected.clone()));

    let fetched = core.get_todo(created.id).await.expect("get linked todo");
    assert_eq!(fetched.email, Some(expected.clone()));

    let listed = core
        .list_todos(TodoFilter::default())
        .await
        .expect("list todos");
    assert_eq!(
        listed
            .iter()
            .find(|todo| todo.id == created.id)
            .and_then(|todo| todo.email.clone()),
        Some(expected),
    );

    let duplicate_source = core
        .create_todo_from_email(
            "같은 메일의 두 번째 할 일".to_owned(),
            EmailLinkInput {
                account_id: "account-1".to_owned(),
                gmail_id: "message-1".to_owned(),
                thread_id: "thread-1".to_owned(),
                subject: "회의 일정".to_owned(),
                from_name: "Kim".to_owned(),
                from_email: "kim@example.com".to_owned(),
            },
        )
        .await
        .expect("allow another todo from the same email");
    assert_ne!(created.id, duplicate_source.id);
    assert_eq!(
        duplicate_source.email.as_ref().map(|email| &email.gmail_id),
        Some(&"message-1".to_owned())
    );

    let bare = core
        .create_todo(CreateTodoInput::new("일반 할 일"))
        .await
        .expect("create bare todo");
    assert_eq!(bare.email, None);
    assert_eq!(
        core.get_todo(bare.id).await.expect("get bare todo").email,
        None,
    );
}

fn list_ids(todos: &[todo_core::Todo]) -> Vec<todo_core::TodoId> {
    todos.iter().map(|todo| todo.id).collect()
}

#[tokio::test]
async fn defer_hides_from_normal_list_but_shows_in_deferred_filter() {
    let (_database, core) = test_core().await;
    let a = core
        .create_todo(CreateTodoInput::new("a"))
        .await
        .expect("a");
    let b = core
        .create_todo(CreateTodoInput::new("b"))
        .await
        .expect("b");

    let future = Local::now().date_naive() + chrono::Duration::days(7);
    let future_str = future.format("%Y-%m-%d").to_string();
    let deferred = core.defer_todo(a.id, &future_str).await.expect("defer a");
    assert_eq!(deferred.status, Status::Deferred);
    assert_eq!(deferred.deferred_until, Some(future_str));

    // 전체 목록은 보류를 뺀다.
    let all = core
        .list_todos(TodoFilter::default())
        .await
        .expect("list all");
    assert_eq!(list_ids(&all), vec![b.id]);

    // 보류 필터는 보류만 준다.
    let lane = core
        .list_todos(TodoFilter {
            status: Some(Status::Deferred),
            ..TodoFilter::default()
        })
        .await
        .expect("list deferred");
    assert_eq!(list_ids(&lane), vec![a.id]);
}

#[tokio::test]
async fn a_past_return_date_wakes_the_todo_on_read() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("wake me"))
        .await
        .expect("create");
    let past = Local::now().date_naive() - chrono::Duration::days(1);
    core.defer_todo(todo.id, &past.format("%Y-%m-%d").to_string())
        .await
        .expect("defer into the past");

    // 읽기가 깨운다. 지난 복귀일이면 todo 로 돌아오고 복귀일은 지워진다.
    let all = core.list_todos(TodoFilter::default()).await.expect("list");
    assert_eq!(list_ids(&all), vec![todo.id]);
    let woken = core.get_todo(todo.id).await.expect("get woken");
    assert_eq!(woken.status, Status::Todo);
    assert_eq!(woken.deferred_until, None);
}

#[tokio::test]
async fn an_indefinite_defer_never_wakes() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("sleep forever"))
        .await
        .expect("create");
    core.defer_todo(todo.id, "").await.expect("defer forever");

    let all = core.list_todos(TodoFilter::default()).await.expect("list");
    assert!(all.is_empty(), "무기한 보류는 전체 목록에 안 나온다");
    let still = core.get_todo(todo.id).await.expect("get");
    assert_eq!(still.status, Status::Deferred);
    assert_eq!(still.deferred_until, None);
}

#[tokio::test]
async fn leaving_deferred_clears_the_return_date_and_pushes_linked_done() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("deferred then done"))
        .await
        .expect("create");
    core.link_linear(todo.id, linear_link("defer-done-issue"))
        .await
        .expect("link");
    let future = (Local::now().date_naive() + chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    core.defer_todo(todo.id, &future).await.expect("defer");

    // 보류 항목을 done 으로 걸면: done 되고 복귀일이 지워지고 Linear 로 밀린다.
    let done = core.set_status(todo.id, Status::Done).await.expect("done");
    assert_eq!(done.status, Status::Done);
    assert_eq!(done.deferred_until, None);
    let outbox = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sync_outbox WHERE kind = 'linear_complete'",
    )
    .fetch_one(core.pool())
    .await
    .expect("count outbox");
    assert_eq!(outbox, 1);
}

#[tokio::test]
async fn deferring_a_linked_todo_never_enqueues_outbox() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("linked deferred"))
        .await
        .expect("create");
    core.link_linear(todo.id, linear_link("defer-only-issue"))
        .await
        .expect("link");
    core.defer_todo(todo.id, "").await.expect("defer");

    let outbox = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_outbox")
        .fetch_one(core.pool())
        .await
        .expect("count outbox");
    assert_eq!(outbox, 0, "보류는 done 이 아니니 아무것도 안 민다");
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

// completed_at 을 임의 시각으로 되돌린다. 코어는 항상 "지금" 으로 적기 때문에
// 오래 전에 끝낸 일은 이렇게 만들어야 한다.
async fn backdate_completion(core: &TodoCore, id: todo_core::TodoId, days_ago: i64) {
    let when = (chrono::Utc::now() - chrono::Duration::days(days_ago))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    sqlx::query("UPDATE todos SET completed_at = ? WHERE id = ?")
        .bind(when)
        .bind(id.to_string())
        .execute(core.pool())
        .await
        .expect("backdate completion");
}

#[tokio::test]
async fn completed_since_hides_only_old_done_todos() {
    let (_database, core) = test_core().await;

    let stale = core
        .create_todo(CreateTodoInput::new("오래 전에 끝낸 일"))
        .await
        .expect("create stale");
    let fresh = core
        .create_todo(CreateTodoInput::new("어제 끝낸 일"))
        .await
        .expect("create fresh");
    let open = core
        .create_todo(CreateTodoInput::new("아직 하는 일"))
        .await
        .expect("create open");

    for id in [stale.id, fresh.id] {
        core.set_status(id, Status::Done).await.expect("mark done");
    }
    backdate_completion(&core, stale.id, 30).await;
    backdate_completion(&core, fresh.id, 1).await;

    let cutoff = (chrono::Utc::now() - chrono::Duration::days(7)).date_naive();
    let visible = core
        .list_todos(TodoFilter {
            completed_since: Some(cutoff),
            ..TodoFilter::default()
        })
        .await
        .expect("list with cutoff");
    let ids: Vec<_> = visible.iter().map(|todo| todo.id).collect();

    assert!(!ids.contains(&stale.id), "30일 전 완료는 빠져야 한다");
    assert!(ids.contains(&fresh.id), "어제 완료는 남아야 한다");
    assert!(
        ids.contains(&open.id),
        "완료가 아닌 항목은 건드리면 안 된다"
    );
}

#[tokio::test]
async fn completed_since_is_off_when_unset() {
    let (_database, core) = test_core().await;
    let stale = core
        .create_todo(CreateTodoInput::new("오래 전에 끝낸 일"))
        .await
        .expect("create stale");
    core.set_status(stale.id, Status::Done)
        .await
        .expect("mark done");
    backdate_completion(&core, stale.id, 90).await;

    let visible = core
        .list_todos(TodoFilter::default())
        .await
        .expect("list without cutoff");
    assert!(visible.iter().any(|todo| todo.id == stale.id));
}

// 완료 시각을 모르는 예전 데이터를 조용히 숨기면 사라진 것처럼 보인다.
#[tokio::test]
async fn completed_since_keeps_done_todos_without_a_timestamp() {
    let (_database, core) = test_core().await;
    let todo = core
        .create_todo(CreateTodoInput::new("완료 시각이 없는 일"))
        .await
        .expect("create todo");
    core.set_status(todo.id, Status::Done)
        .await
        .expect("mark done");
    sqlx::query("UPDATE todos SET completed_at = NULL WHERE id = ?")
        .bind(todo.id.to_string())
        .execute(core.pool())
        .await
        .expect("clear completion time");

    let cutoff = (chrono::Utc::now() - chrono::Duration::days(7)).date_naive();
    let visible = core
        .list_todos(TodoFilter {
            completed_since: Some(cutoff),
            ..TodoFilter::default()
        })
        .await
        .expect("list with cutoff");
    assert!(visible.iter().any(|item| item.id == todo.id));
}
