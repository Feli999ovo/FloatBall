//! 定时任务（B5）：存在本地 config 里；后台每 30 秒看一次有没有到点的。
//! 到点时只发一个事件给小球窗口，由界面像普通提问一样在后台跑（审批卡、预算、历史都复用）。
//! 错过的（关机、睡眠）开机后补跑一次，只补最近一个周期内的；更早的不补。

use chrono::{DateTime, Datelike, Duration, Local, NaiveTime, TimeZone, Timelike, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Routine {
    pub id: String,
    pub name: String,
    /// 每次运行时交给 Agent 的指令
    pub prompt: String,
    /// "day"（每天）/ "week"（每周）/ "hour"（每小时整点）
    pub every: String,
    /// 每周几：0 = 周日 … 6 = 周六（只有 every = "week" 用）
    pub weekday: u8,
    /// "HH:MM"（每小时时只用分钟）
    pub time: String,
    pub on: bool,
    /// 上次到点的时间（秒）。补跑、去重都靠它
    pub last: i64,
    /// 最近几次运行记录，新的在前（最多 5 条）
    pub runs: Vec<Run>,
}

impl Default for Routine {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), prompt: String::new(), every: "day".into(), weekday: 1, time: "09:00".into(), on: true, last: 0, runs: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Run {
    pub at: i64,
    pub ok: bool,
    pub note: String,
}

pub const MAX_ROUTINES: usize = 32;

fn hm(t: &str) -> NaiveTime {
    NaiveTime::parse_from_str(t.trim(), "%H:%M").unwrap_or(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
}

fn at<Tz: TimeZone>(tz: &Tz, d: chrono::NaiveDate, t: NaiveTime) -> Option<DateTime<Tz>> {
    // 夏令时跳过的那个钟点取它之后最早的时间；重复的取第一个
    tz.from_local_datetime(&d.and_time(t)).earliest().or_else(|| tz.from_local_datetime(&(d.and_time(t) + Duration::hours(1))).earliest())
}

/// 最近一次「应该运行」的时间（≤ now）
pub fn prev_due<Tz: TimeZone>(r: &Routine, now: &DateTime<Tz>) -> Option<DateTime<Tz>> {
    let tz = now.timezone();
    let t = hm(&r.time);
    match r.every.as_str() {
        "hour" => {
            let base = now.date_naive().and_hms_opt(now.hour(), t.minute(), 0)?;
            let c = tz.from_local_datetime(&base).earliest()?;
            Some(if c <= *now { c } else { c - Duration::hours(1) })
        }
        "week" => {
            let want = Weekday::try_from(((r.weekday % 7) + 6) % 7).ok()?; // 0=周日 → chrono 的 Sun
            (0..8).map(|k| now.date_naive() - Duration::days(k)).filter(|d| d.weekday() == want).filter_map(|d| at(&tz, d, t)).find(|c| c <= now)
        }
        _ => (0..2).map(|k| now.date_naive() - Duration::days(k)).filter_map(|d| at(&tz, d, t)).find(|c| c <= now),
    }
}

/// 下一次运行的时间（> now）
pub fn next_due<Tz: TimeZone>(r: &Routine, now: &DateTime<Tz>) -> Option<DateTime<Tz>> {
    let period = period(r);
    // 从「最近一次」往后加一个周期；夏令时那天差一小时不要紧，再往后找一次
    let p = prev_due(r, now)?;
    let mut n = p + period;
    if n <= *now { n = n + period; }
    prev_due(r, &n).filter(|x| x > now).or(Some(n))
}

fn period(r: &Routine) -> Duration {
    match r.every.as_str() { "hour" => Duration::hours(1), "week" => Duration::weeks(1), _ => Duration::days(1) }
}

/// 现在要不要跑：最近一次该跑的时间在上次之后，并且没有过一个周期那么久（太久以前错过的不补）
pub fn due<Tz: TimeZone>(r: &Routine, now: &DateTime<Tz>) -> Option<i64> {
    if !r.on { return None; }
    let p = prev_due(r, now)?;
    let ts = p.timestamp();
    (ts > r.last && now.timestamp() - ts < period(r).num_seconds()).then_some(ts)
}

/// 给 Config 用的整理：名字、时间格式、个数
pub fn clean(mut r: Routine) -> Routine {
    r.name = r.name.trim().chars().take(24).collect();
    r.prompt = r.prompt.trim().to_string();
    if r.name.is_empty() { r.name = r.prompt.chars().take(12).collect(); }
    if !matches!(r.every.as_str(), "day" | "week" | "hour") { r.every = "day".into(); }
    r.weekday %= 7;
    r.time = hm(&r.time).format("%H:%M").to_string();
    r.runs.truncate(5);
    r
}

/// 以本地时区算（界面显示用）
pub fn next_local(r: &Routine) -> Option<i64> {
    next_due(r, &Local::now()).map(|d| d.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};

    fn tz() -> FixedOffset { FixedOffset::east_opt(8 * 3600).unwrap() }
    fn t(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<FixedOffset> { tz().with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap() }
    fn day(time: &str) -> Routine { Routine { every: "day".into(), time: time.into(), ..Routine::default() } }

    #[test]
    fn daily_runs_once_after_the_time() {
        let mut r = day("09:00");
        // 2026-11-20 是周五
        assert_eq!(due(&r, &t(2026, 11, 20, 8, 59)).is_some(), true, "前一天 09:00 还在一个周期内，没跑过就补");
        r.last = t(2026, 11, 19, 9, 0).timestamp();
        assert_eq!(due(&r, &t(2026, 11, 20, 8, 59)), None, "还没到今天的点");
        assert_eq!(due(&r, &t(2026, 11, 20, 9, 0)), Some(t(2026, 11, 20, 9, 0).timestamp()));
        r.last = t(2026, 11, 20, 9, 0).timestamp();
        assert_eq!(due(&r, &t(2026, 11, 20, 9, 30)), None, "跑过了不重复");
    }

    #[test]
    fn missed_runs_only_catch_up_within_one_period() {
        let mut r = day("09:00");
        r.last = t(2026, 11, 17, 9, 0).timestamp();
        // 关机三天，20 日 10:00 开机：只补 20 日 09:00 那一次
        assert_eq!(due(&r, &t(2026, 11, 20, 10, 0)), Some(t(2026, 11, 20, 9, 0).timestamp()));
        // 20 日 08:00 开机：19 日 09:00 那次在 23 小时前，还在一个周期内，补一次
        assert_eq!(due(&r, &t(2026, 11, 20, 8, 0)), Some(t(2026, 11, 19, 9, 0).timestamp()));
        r.on = false;
        assert_eq!(due(&r, &t(2026, 11, 20, 10, 0)), None, "关掉的不跑");
    }

    #[test]
    fn weekly_and_hourly() {
        // 每周五 17:00；2026-11-20 是周五
        let mut r = Routine { every: "week".into(), weekday: 5, time: "17:00".into(), ..Routine::default() };
        r.last = t(2026, 11, 13, 17, 0).timestamp();
        assert_eq!(due(&r, &t(2026, 11, 20, 16, 0)), None);
        assert_eq!(due(&r, &t(2026, 11, 20, 17, 1)), Some(t(2026, 11, 20, 17, 0).timestamp()));
        assert_eq!(next_due(&r, &t(2026, 11, 20, 17, 1)).unwrap(), t(2026, 11, 27, 17, 0));
        // 周日 = 0
        let sun = Routine { every: "week".into(), weekday: 0, time: "20:00".into(), ..Routine::default() };
        assert_eq!(next_due(&sun, &t(2026, 11, 20, 12, 0)).unwrap(), t(2026, 11, 22, 20, 0));
        // 每小时 :15
        let mut h = Routine { every: "hour".into(), time: "00:15".into(), ..Routine::default() };
        h.last = t(2026, 11, 20, 9, 15).timestamp();
        assert_eq!(due(&h, &t(2026, 11, 20, 10, 10)), None);
        assert_eq!(due(&h, &t(2026, 11, 20, 10, 16)), Some(t(2026, 11, 20, 10, 15).timestamp()));
        assert_eq!(next_due(&h, &t(2026, 11, 20, 10, 16)).unwrap(), t(2026, 11, 20, 11, 15));
    }

    #[test]
    fn clean_fixes_bad_input() {
        let r = clean(Routine { name: "  ".into(), prompt: " 汇总未读邮件并分组 ".into(), every: "month".into(), weekday: 9, time: "25:99".into(), runs: vec![Run::default(); 9], ..Routine::default() });
        assert_eq!((r.name.as_str(), r.every.as_str(), r.weekday, r.time.as_str(), r.runs.len()), ("汇总未读邮件并分组", "day", 2, "09:00", 5));
    }
}
