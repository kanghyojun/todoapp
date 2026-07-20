use chrono::{Datelike, Days, NaiveDate, TimeZone, Utc, Weekday};
use chrono_english::{Dialect, parse_date_string};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid due date: {input}")]
pub struct ParseDueDateError {
    input: String,
}

impl ParseDueDateError {
    fn new(input: &str) -> Self {
        Self {
            input: input.to_owned(),
        }
    }
}

pub fn parse_due_date(
    input: &str,
    today: NaiveDate,
) -> Result<Option<NaiveDate>, ParseDueDateError> {
    let normalized = input.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(None);
    }

    let parsed = match normalized.as_str() {
        "today" => Some(today),
        "tomorrow" | "tmr" => checked_days_after(today, 1),
        "next week" => next_weekday(today, Weekday::Mon),
        "mon" => next_weekday(today, Weekday::Mon),
        "tue" => next_weekday(today, Weekday::Tue),
        "wed" => next_weekday(today, Weekday::Wed),
        "thu" => next_weekday(today, Weekday::Thu),
        "fri" => next_weekday(today, Weekday::Fri),
        "sat" => next_weekday(today, Weekday::Sat),
        "sun" => next_weekday(today, Weekday::Sun),
        value => parse_iso(value)
            .or_else(|| parse_relative(value, today))
            .or_else(|| parse_month_day(value, today))
            .or_else(|| parse_natural(value, today)),
    };

    parsed
        .map(Some)
        .ok_or_else(|| ParseDueDateError::new(input))
}

fn checked_days_after(date: NaiveDate, days: u64) -> Option<NaiveDate> {
    date.checked_add_days(Days::new(days))
}

fn next_weekday(today: NaiveDate, weekday: Weekday) -> Option<NaiveDate> {
    let current = i64::from(today.weekday().num_days_from_monday());
    let target = i64::from(weekday.num_days_from_monday());
    let days = (target - current).rem_euclid(7);
    let days = if days == 0 { 7 } else { days };
    checked_days_after(today, u64::try_from(days).ok()?)
}

fn parse_iso(input: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(input, "%Y-%m-%d").ok()
}

fn parse_relative(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    let (number, multiplier) = if let Some(number) = input.strip_suffix('d') {
        (number, 1_u64)
    } else if let Some(number) = input.strip_suffix('w') {
        (number, 7_u64)
    } else {
        return None;
    };
    let amount = number.parse::<u64>().ok()?.checked_mul(multiplier)?;
    checked_days_after(today, amount)
}

fn parse_month_day(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    let (month, day) = input.split_once('/')?;
    let month = month.parse::<u32>().ok()?;
    let day = day.parse::<u32>().ok()?;
    let this_year = NaiveDate::from_ymd_opt(today.year(), month, day)?;
    if this_year >= today {
        Some(this_year)
    } else {
        NaiveDate::from_ymd_opt(today.year().checked_add(1)?, month, day)
    }
}

/// 영어 자연어 표현("next monday", "in 2 weeks" 등)을 chrono-english 로 파싱한다.
/// 주입받은 today 를 기준 시각(UTC 자정)으로 삼아 테스트 재현성을 지킨다.
fn parse_natural(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    let now = Utc.from_utc_datetime(&today.and_hms_opt(0, 0, 0)?);
    parse_date_string(input, now, Dialect::Us)
        .ok()
        .map(|dt| dt.date_naive())
}
