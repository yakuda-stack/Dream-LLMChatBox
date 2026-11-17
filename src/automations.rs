use crate::i18n::{tr, trf};
use chrono::{DateTime, Datelike, Duration, Local, NaiveTime, TimeZone, Weekday};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const KINDS: [&str; 4] = ["Every day", "Weekdays (Mon–Fri)", "Once a week", "Every few hours"];
pub const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Automation {
    pub id: String,
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub kind: u32,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub weekday: u32,
    #[serde(default)]
    pub hours: u32,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub last_run: i64,
    #[serde(default)]
    pub last_chat: String,
}

fn file() -> PathBuf {
    crate::storage::data_dir().join("automations.json")
}

pub fn load() -> Vec<Automation> {
    fs::read_to_string(file())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(list: &[Automation]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    fs::write(file(), text).map_err(|e| e.to_string())
}

pub fn parse_time(text: &str) -> Option<NaiveTime> {
    let text = text.trim().replace('.', ":");
    NaiveTime::parse_from_str(&text, "%H:%M").ok()
}

fn weekday_index(day: Weekday) -> u32 {
    day.num_days_from_monday()
}

pub fn next_after(automation: &Automation, after: DateTime<Local>) -> Option<DateTime<Local>> {
    if automation.kind == 3 {
        if automation.hours == 0 {
            return None;
        }
        return Some(after + Duration::hours(automation.hours as i64));
    }
    let time = parse_time(&automation.time)?;
    let start = after.date_naive();
    (0..9).find_map(|offset| {
        let date = start + Duration::days(offset);
        let fits = match automation.kind {
            1 => weekday_index(date.weekday()) < 5,
            2 => weekday_index(date.weekday()) == automation.weekday,
            _ => true,
        };
        if !fits {
            return None;
        }
        let when = Local.from_local_datetime(&date.and_time(time)).earliest()?;
        (when > after).then_some(when)
    })
}

fn timestamp(seconds: i64) -> DateTime<Local> {
    Local.timestamp_opt(seconds, 0).single().unwrap_or_else(Local::now)
}

pub fn next_run(automation: &Automation) -> Option<DateTime<Local>> {
    let from = automation.last_run.max(automation.created);
    next_after(automation, timestamp(from))
}

pub fn is_due(automation: &Automation, now: DateTime<Local>) -> bool {
    automation.enabled
        && !automation.prompt.trim().is_empty()
        && next_run(automation).is_some_and(|when| when <= now)
}

pub fn schedule_text(automation: &Automation) -> String {
    let time = automation.time.trim();
    match automation.kind {
        1 => trf("Mon–Fri at {}", &[&time]),
        2 => {
            let day = tr(WEEKDAYS.get(automation.weekday as usize).copied().unwrap_or("Monday"));
            trf("Every {} at {}", &[&day, &time])
        }
        3 => trf("Every {} h", &[&automation.hours]),
        _ => trf("Every day at {}", &[&time]),
    }
}

pub fn check(automation: &Automation) -> Result<(), String> {
    if automation.name.trim().is_empty() {
        return Err(tr("Please give the automation a name."));
    }
    if automation.prompt.trim().is_empty() {
        return Err(tr("Please write what the model should do."));
    }
    if automation.kind == 3 {
        if automation.hours == 0 {
            return Err(tr("Please choose at least 1 hour."));
        }
    } else if parse_time(&automation.time).is_none() {
        return Err(tr("Please write the time like 08:00."));
    }
    Ok(())
}

pub fn run_note(name: &str) -> String {
    format!(
        "This is the automation \"{name}\", started on a schedule at {}. Nobody is watching right now: do not ask questions, make sensible assumptions and give a complete answer that can be read later.",
        Local::now().format("%Y-%m-%d %H:%M (%A)")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        let date = NaiveDate::from_ymd_opt(2026, 11, day).unwrap();
        Local.from_local_datetime(&date.and_hms_opt(hour, minute, 0).unwrap()).earliest().unwrap()
    }

    fn automation(kind: u32) -> Automation {
        Automation {
            name: "News".into(),
            prompt: "Sag hallo".into(),
            kind,
            time: "08:00".into(),
            weekday: 2,
            hours: 6,
            enabled: true,
            ..Default::default()
        }
    }

    #[test]
    fn schedules() {
        assert_eq!(next_after(&automation(0), at(17, 7, 0)), Some(at(17, 8, 0)));
        assert_eq!(next_after(&automation(0), at(17, 8, 0)), Some(at(18, 8, 0)));
        assert_eq!(next_after(&automation(1), at(20, 9, 0)), Some(at(23, 8, 0)));
        assert_eq!(next_after(&automation(2), at(17, 9, 0)), Some(at(18, 8, 0)));
        assert_eq!(next_after(&automation(2), at(18, 9, 0)), Some(at(25, 8, 0)));
        assert_eq!(next_after(&automation(3), at(17, 9, 0)), Some(at(17, 15, 0)));
    }

    #[test]
    fn due_and_check() {
        let mut job = automation(0);
        job.created = at(16, 9, 0).timestamp();
        assert!(is_due(&job, at(17, 8, 1)));
        job.last_run = at(17, 8, 1).timestamp();
        assert!(!is_due(&job, at(17, 20, 0)));
        job.enabled = false;
        assert!(!is_due(&job, at(19, 9, 0)));
        assert!(check(&job).is_ok());
        job.time = "25:00".into();
        assert!(check(&job).is_err());
        assert_eq!(parse_time("7.30"), NaiveTime::from_hms_opt(7, 30, 0));
        assert_eq!(schedule_text(&automation(2)), "Every Wednesday at 08:00");
    }
}
