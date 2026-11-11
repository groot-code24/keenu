// SPDX-License-Identifier: GPL-3.0-or-later

//! Requests that run by themselves at a set time, the morning briefing among them. They run only while Keenu is running,
//! and each goes through the same guard as any other: nobody is there to approve, so what would ask is not done.

use chrono::{DateTime, Datelike, Local, NaiveTime, TimeZone, Timelike, Weekday};
use serde::{Deserialize, Serialize};

/// A briefing's request, made from what Keenu knows.
pub const BRIEFING: &str = "Give me a short morning briefing: what is on my calendar and my reminders today, using what you were given, in a few sentences. Do not take any actions.";

/// When a task runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Repeat {
    /// Every day, at this local time.
    Daily { hour: u32, minute: u32 },
    /// Monday to Friday.
    Weekdays { hour: u32, minute: u32 },
    /// One day of the week; 0 is Monday.
    Weekly { day: u32, hour: u32, minute: u32 },
    /// Once, at this moment.
    Once { at_ms: i64 },
}

/// A request to run later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub request: String,
    pub repeat: Repeat,
    pub last_run_ms: i64,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// All of them, as saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub tasks: Vec<Task>,
    next_id: u64,
}

fn time_of(hour: u32, minute: u32) -> Option<NaiveTime> {
    NaiveTime::from_hms_opt(hour, minute, 0)
}

/// The last moment `repeat` fell due at or before `now`.
fn last_due<Tz: TimeZone>(repeat: &Repeat, now: &DateTime<Tz>) -> Option<DateTime<Tz>> {
    let tz = now.timezone();
    let on_day = |days_back: i64, hour: u32, minute: u32| {
        let day = (now.clone() - chrono::Duration::days(days_back)).date_naive();
        tz.from_local_datetime(&day.and_time(time_of(hour, minute)?))
            .earliest()
    };
    match *repeat {
        Repeat::Once { at_ms } => tz
            .timestamp_millis_opt(at_ms)
            .single()
            .filter(|at| at <= now),
        Repeat::Daily { hour, minute } => (0..2)
            .filter_map(|back| on_day(back, hour, minute))
            .find(|at| at <= now),
        Repeat::Weekdays { hour, minute } => (0..8)
            .filter_map(|back| on_day(back, hour, minute))
            .find(|at| at <= now && !matches!(at.weekday(), Weekday::Sat | Weekday::Sun)),
        Repeat::Weekly { day, hour, minute } => (0..8)
            .filter_map(|back| on_day(back, hour, minute))
            .find(|at| at <= now && at.weekday().num_days_from_monday() == day),
    }
}

impl Schedule {
    /// Adds a task; its id. It first runs the next time it falls due, not for a time already past today.
    pub fn add(&mut self, request: &str, repeat: Repeat, now_ms: i64) -> Result<u64, String> {
        let request = request.trim();
        if request.is_empty() {
            return Err("Say what it should do.".into());
        }
        match repeat {
            Repeat::Daily { hour, minute }
            | Repeat::Weekdays { hour, minute }
            | Repeat::Weekly { hour, minute, .. }
                if time_of(hour, minute).is_none() =>
            {
                return Err("That is not a time of day.".into());
            }
            Repeat::Weekly { day, .. } if day > 6 => {
                return Err("That is not a day of the week.".into());
            }
            _ => {}
        }
        self.next_id += 1;
        self.tasks.push(Task {
            id: self.next_id,
            request: request.to_owned(),
            repeat,
            last_run_ms: now_ms,
            enabled: true,
        });
        Ok(self.next_id)
    }

    /// True if it was there.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|task| task.id != id);
        self.tasks.len() != before
    }

    /// Whether the briefing is set, and its id.
    pub fn briefing(&self) -> Option<&Task> {
        self.tasks.iter().find(|task| task.request == BRIEFING)
    }

    /// The tasks that fell due since they last ran, marked as run. A computer that was off runs each once when it is
    /// back, and a one-off goes once it has run.
    pub fn take_due(&mut self, now: &DateTime<Local>) -> Vec<Task> {
        self.take_due_in(now)
    }

    fn take_due_in<Tz: TimeZone>(&mut self, now: &DateTime<Tz>) -> Vec<Task> {
        let now_ms = now.timestamp_millis();
        let mut due = Vec::new();
        for task in &mut self.tasks {
            if !task.enabled {
                continue;
            }
            let fell_due = last_due(&task.repeat, now)
                .is_some_and(|at| at.timestamp_millis() > task.last_run_ms);
            if fell_due {
                task.last_run_ms = now_ms;
                due.push(task.clone());
            }
        }
        self.tasks.retain(|task| {
            !(matches!(task.repeat, Repeat::Once { .. }) && due.iter().any(|ran| ran.id == task.id))
        });
        due
    }
}

/// `7:30`, `07:30` or `7:30 pm` as an hour and minute.
pub fn parse_time(text: &str) -> Option<(u32, u32)> {
    let text = text.trim().to_lowercase();
    let (clock, afternoon) = match text.strip_suffix("pm") {
        Some(rest) => (rest.trim(), Some(true)),
        None => match text.strip_suffix("am") {
            Some(rest) => (rest.trim(), Some(false)),
            None => (text.as_str(), None),
        },
    };
    let (hour, minute) = clock.split_once(':').unwrap_or((clock, "0"));
    let (mut hour, minute) = (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?);
    if let Some(afternoon) = afternoon {
        if !(1..=12).contains(&hour) {
            return None;
        }
        hour = hour % 12 + if afternoon { 12 } else { 0 };
    }
    time_of(hour, minute).map(|time| (time.hour(), time.minute()))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        // 2026-11-02 is a Monday.
        Utc.with_ymd_and_hms(2026, 11, day, hour, minute, 0)
            .unwrap()
    }

    fn ms(time: DateTime<Utc>) -> i64 {
        time.timestamp_millis()
    }

    #[test]
    fn a_daily_task_runs_once_a_day_and_not_for_a_time_already_past() {
        let mut schedule = Schedule::default();
        schedule
            .add(
                "brief me",
                Repeat::Daily { hour: 8, minute: 0 },
                ms(at(2, 9, 0)),
            )
            .unwrap();
        assert!(schedule.take_due_in(&at(2, 9, 1)).is_empty());
        assert!(schedule.take_due_in(&at(3, 7, 59)).is_empty());
        assert_eq!(schedule.take_due_in(&at(3, 8, 0)).len(), 1);
        assert!(schedule.take_due_in(&at(3, 8, 1)).is_empty());
        assert_eq!(schedule.take_due_in(&at(4, 8, 30)).len(), 1);
    }

    #[test]
    fn days_missed_run_once_on_return() {
        let mut schedule = Schedule::default();
        schedule
            .add(
                "brief me",
                Repeat::Daily { hour: 8, minute: 0 },
                ms(at(2, 7, 0)),
            )
            .unwrap();
        assert_eq!(schedule.take_due_in(&at(9, 12, 0)).len(), 1);
        assert!(schedule.take_due_in(&at(9, 12, 1)).is_empty());
    }

    #[test]
    fn weekdays_skip_the_weekend() {
        let mut schedule = Schedule::default();
        schedule
            .add(
                "x",
                Repeat::Weekdays { hour: 8, minute: 0 },
                ms(at(6, 9, 0)),
            )
            .unwrap();
        assert!(schedule.take_due_in(&at(7, 9, 0)).is_empty(), "Saturday");
        assert!(schedule.take_due_in(&at(8, 9, 0)).is_empty(), "Sunday");
        assert_eq!(schedule.take_due_in(&at(9, 9, 0)).len(), 1, "Monday");
    }

    #[test]
    fn weekly_keeps_to_its_day() {
        let mut schedule = Schedule::default();
        schedule
            .add(
                "x",
                Repeat::Weekly {
                    day: 2,
                    hour: 8,
                    minute: 0,
                },
                ms(at(2, 9, 0)),
            )
            .unwrap();
        assert!(schedule.take_due_in(&at(3, 9, 0)).is_empty(), "Tuesday");
        assert_eq!(schedule.take_due_in(&at(4, 9, 0)).len(), 1, "Wednesday");
    }

    #[test]
    fn a_one_off_goes_once_it_has_run() {
        let mut schedule = Schedule::default();
        let id = schedule
            .add(
                "x",
                Repeat::Once {
                    at_ms: ms(at(3, 8, 0)),
                },
                ms(at(2, 9, 0)),
            )
            .unwrap();
        assert!(schedule.take_due_in(&at(2, 10, 0)).is_empty());
        assert_eq!(schedule.take_due_in(&at(3, 8, 1))[0].id, id);
        assert!(schedule.tasks.is_empty());
    }

    #[test]
    fn a_task_turned_off_waits_and_a_removed_one_is_gone() {
        let mut schedule = Schedule::default();
        let id = schedule
            .add("x", Repeat::Daily { hour: 8, minute: 0 }, ms(at(2, 7, 0)))
            .unwrap();
        schedule.tasks[0].enabled = false;
        assert!(schedule.take_due_in(&at(2, 9, 0)).is_empty());
        assert!(schedule.remove(id));
        assert!(!schedule.remove(id));
    }

    #[test]
    fn nonsense_is_refused() {
        let mut schedule = Schedule::default();
        assert!(
            schedule
                .add(" ", Repeat::Daily { hour: 8, minute: 0 }, 0)
                .is_err()
        );
        assert!(
            schedule
                .add(
                    "x",
                    Repeat::Daily {
                        hour: 25,
                        minute: 0
                    },
                    0
                )
                .is_err()
        );
        assert!(
            schedule
                .add(
                    "x",
                    Repeat::Weekly {
                        day: 9,
                        hour: 8,
                        minute: 0
                    },
                    0
                )
                .is_err()
        );
    }

    #[test]
    fn reads_times_as_people_write_them() {
        assert_eq!(parse_time("7:30"), Some((7, 30)));
        assert_eq!(parse_time("07:30 pm"), Some((19, 30)));
        assert_eq!(parse_time("12:05am"), Some((0, 5)));
        assert_eq!(parse_time("12 pm"), Some((12, 0)));
        assert_eq!(parse_time("9"), Some((9, 0)));
        assert_eq!(parse_time("25:00"), None);
        assert_eq!(parse_time("13 pm"), None);
        assert_eq!(parse_time("soon"), None);
    }

    #[test]
    fn survives_saving() {
        let mut schedule = Schedule::default();
        schedule
            .add(BRIEFING, Repeat::Daily { hour: 8, minute: 0 }, 5)
            .unwrap();
        let text = serde_json::to_string(&schedule).unwrap();
        let back: Schedule = serde_json::from_str(&text).unwrap();
        assert_eq!(back, schedule);
        assert!(back.briefing().is_some());
    }
}
