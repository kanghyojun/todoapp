use chrono::{Datelike, NaiveDate, Weekday};
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

#[test]
fn parses_korean_named_and_relative_days() {
    let friday = date(2026, 7, 10);

    assert_eq!(parse_due_date("오늘", friday).expect("오늘"), Some(friday));
    assert_eq!(
        parse_due_date("내일", friday).expect("내일"),
        Some(date(2026, 7, 11))
    );
    assert_eq!(
        parse_due_date("모레", friday).expect("모레"),
        Some(date(2026, 7, 12))
    );
    assert_eq!(
        parse_due_date("글피", friday).expect("글피"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("3일 뒤", friday).expect("N일 뒤"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("3일 후", friday).expect("N일 후"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("2주 뒤", friday).expect("N주 뒤"),
        Some(date(2026, 7, 24))
    );
    assert_eq!(
        parse_due_date("2주후", friday).expect("N주후 공백 없이"),
        Some(date(2026, 7, 24))
    );
    assert_eq!(
        parse_due_date("1주일 뒤", friday).expect("N주일 뒤"),
        Some(date(2026, 7, 17))
    );
    assert_eq!(
        parse_due_date("1달 뒤", friday).expect("N달 뒤"),
        Some(date(2026, 8, 10))
    );
    assert_eq!(
        parse_due_date("3개월 뒤", friday).expect("N개월 뒤"),
        Some(date(2026, 10, 10))
    );

    assert!(parse_due_date("아무개", friday).is_err());
    assert!(parse_due_date("3일", friday).is_err());
}

#[test]
fn parses_korean_weekday_expressions() {
    let friday = date(2026, 7, 10);

    // bare 요일: 다음 발생일. 금요일이면 같은 요일 → +7.
    assert_eq!(
        parse_due_date("월요일", friday).expect("월요일"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("월", friday).expect("월 약어"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("금요일", friday).expect("금요일"),
        Some(date(2026, 7, 17))
    );

    // 다음/다음주/다다음주.
    assert_eq!(
        parse_due_date("다음주", friday).expect("다음주"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("다다음주", friday).expect("다다음주"),
        Some(date(2026, 7, 20))
    );
    assert_eq!(
        parse_due_date("다음 월요일", friday).expect("다음 월요일"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("다음주 월요일", friday).expect("다음주 월요일"),
        Some(date(2026, 7, 13))
    );

    // 토요일은 "이번 주말"과 "다음주"가 갈린다.
    assert_eq!(
        parse_due_date("토요일", friday).expect("토요일"),
        Some(date(2026, 7, 11))
    );
    assert_eq!(
        parse_due_date("다음주 토요일", friday).expect("다음주 토요일"),
        Some(date(2026, 7, 18))
    );
    assert_eq!(
        parse_due_date("이번주 토요일", friday).expect("이번주 토요일"),
        Some(date(2026, 7, 11))
    );
}

#[test]
fn parses_english_natural_language() {
    let friday = date(2026, 7, 10);

    // 원 요청: "next monday" 가 에러 대신 미래의 월요일로 파싱돼야 한다.
    let next_monday = parse_due_date("next monday", friday)
        .expect("next monday")
        .expect("some date");
    assert_eq!(next_monday.weekday(), Weekday::Mon);
    assert!(next_monday > friday);

    assert_eq!(
        parse_due_date("3 days", friday).expect("3 days"),
        Some(date(2026, 7, 13))
    );
    assert_eq!(
        parse_due_date("2 weeks", friday).expect("2 weeks"),
        Some(date(2026, 7, 24))
    );
    assert_eq!(
        parse_due_date("2 days ago", friday).expect("2 days ago"),
        Some(date(2026, 7, 8))
    );
}
