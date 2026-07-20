use chrono::{Datelike, Days, Months, NaiveDate, TimeZone, Utc, Weekday};
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
            .or_else(|| parse_korean(value, today))
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

/// 한국어 상대 날짜 표현을 파싱한다. 어휘가 규칙적이라 직접 매핑한다.
/// 지원: 오늘·내일·모레·글피, N일/주/달 뒤(후), 다음주·다다음주,
/// (다음|다음주|다다음주|이번주) X요일, bare X요일.
fn parse_korean(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    let trimmed = input.trim();
    match trimmed {
        "오늘" => return Some(today),
        "내일" | "낼" => return checked_days_after(today, 1),
        "모레" => return checked_days_after(today, 2),
        "글피" => return checked_days_after(today, 3),
        "다음주" | "담주" => return next_weekday(today, Weekday::Mon),
        "다다음주" => {
            return next_weekday(today, Weekday::Mon).and_then(|mon| checked_days_after(mon, 7));
        }
        _ => {}
    }

    let compact = trimmed.replace(' ', "");
    parse_korean_relative(&compact, today)
        .or_else(|| parse_korean_weekday_expr(&compact, today))
}

/// "3일 뒤", "2주후", "1주일 뒤", "3개월 뒤", "1달 뒤" 형태. 공백은 이미 제거돼 있다.
fn parse_korean_relative(compact: &str, today: NaiveDate) -> Option<NaiveDate> {
    let core = compact
        .strip_suffix("후에")
        .or_else(|| compact.strip_suffix("뒤에"))
        .or_else(|| compact.strip_suffix("이후"))
        .or_else(|| compact.strip_suffix("후"))
        .or_else(|| compact.strip_suffix("뒤"))?;

    if let Some(number) = core.strip_suffix("개월").or_else(|| core.strip_suffix("달")) {
        let months = number.parse::<u32>().ok()?;
        return today.checked_add_months(Months::new(months));
    }

    let (number, multiplier) = if let Some(number) = core.strip_suffix("주일") {
        (number, 7_u64)
    } else if let Some(number) = core.strip_suffix("주") {
        (number, 7_u64)
    } else if let Some(number) = core.strip_suffix("일") {
        (number, 1_u64)
    } else {
        return None;
    };
    let amount = number.parse::<u64>().ok()?.checked_mul(multiplier)?;
    checked_days_after(today, amount)
}

/// "(다음|다음주|다다음주|이번주) X요일" 또는 bare "X요일". 공백은 이미 제거돼 있다.
fn parse_korean_weekday_expr(compact: &str, today: NaiveDate) -> Option<NaiveDate> {
    if let Some(rest) = compact.strip_prefix("이번주").or_else(|| compact.strip_prefix("이번")) {
        return this_week_weekday(today, parse_korean_weekday(rest)?);
    }

    let (weeks, rest) = if let Some(rest) = compact.strip_prefix("다다음주") {
        (2_u64, rest)
    } else if let Some(rest) = compact.strip_prefix("다음주") {
        (1, rest)
    } else if let Some(rest) = compact.strip_prefix("담주") {
        (1, rest)
    } else if let Some(rest) = compact.strip_prefix("다음") {
        (0, rest)
    } else {
        (0, compact)
    };

    let weekday = parse_korean_weekday(rest)?;
    if weeks == 0 {
        // "다음 X요일" 과 bare "X요일" 은 모두 다음 발생일로 본다.
        next_weekday(today, weekday)
    } else {
        let next_mon = next_weekday(today, Weekday::Mon)?;
        let base_mon = checked_days_after(next_mon, (weeks - 1) * 7)?;
        checked_days_after(base_mon, u64::from(weekday.num_days_from_monday()))
    }
}

/// "월"·"월요일"·"월욜" 같은 요일 토큰 하나를 Weekday 로.
fn parse_korean_weekday(token: &str) -> Option<Weekday> {
    let mut chars = token.chars();
    let weekday = match chars.next()? {
        '월' => Weekday::Mon,
        '화' => Weekday::Tue,
        '수' => Weekday::Wed,
        '목' => Weekday::Thu,
        '금' => Weekday::Fri,
        '토' => Weekday::Sat,
        '일' => Weekday::Sun,
        _ => return None,
    };
    let rest: String = chars.collect();
    if rest.is_empty() || rest == "요일" || rest == "욜" {
        Some(weekday)
    } else {
        None
    }
}

/// 이번 주(월요일 시작)의 지정 요일. 이미 지난 요일이면 과거 날짜가 나올 수 있다.
fn this_week_weekday(today: NaiveDate, weekday: Weekday) -> Option<NaiveDate> {
    let from_monday = u64::from(today.weekday().num_days_from_monday());
    let monday = today.checked_sub_days(Days::new(from_monday))?;
    checked_days_after(monday, u64::from(weekday.num_days_from_monday()))
}
