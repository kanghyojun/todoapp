use chrono::NaiveDate;
use todo_core::parse_due_date;

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid test date")
}

#[test]
fn parses_supported_due_date_expressions_and_boundaries() {
    let friday = date(2026, 7, 10);

    assert_eq!(parse_due_date("", friday).expect("empty input"), None);
    assert_eq!(
        parse_due_date("today", friday).expect("today"),
        Some(friday)
    );
    assert_eq!(
        parse_due_date("tomorrow", friday).expect("tomorrow"),
        Some(date(2026, 7, 11))
    );
    assert_eq!(
        parse_due_date("tmr", friday).expect("tmr"),
        Some(date(2026, 7, 11))
    );
    assert_eq!(
        parse_due_date("fri", friday).expect("same weekday"),
        Some(date(2026, 7, 17))
    );
    assert_eq!(
        parse_due_date("mon", friday).expect("next weekday"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("next week", friday).expect("next week"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("3d", friday).expect("days"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("2w", friday).expect("weeks"),
        Some(date(2026, 7, 24))
    );
    assert_eq!(
        parse_due_date("4/20", friday).expect("past month/day"),
        Some(date(2027, 4, 20))
    );
    assert_eq!(
        parse_due_date("2026-04-20", friday).expect("ISO date"),
        Some(date(2026, 4, 20))
    );
    assert!(parse_due_date("sometime-ish", friday).is_err());
}
