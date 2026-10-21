use anyhow::{Result, ensure};
use chrono::{DateTime, Duration, FixedOffset};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Lecture {
    pub id: String,
    pub title: String,
    pub category: String,
    pub campus: String,
    pub location: String,
    pub start: DateTime<FixedOffset>,
    pub end: DateTime<FixedOffset>,
    pub online: bool,
    pub reservable: bool,
}
impl Lecture {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.id.trim().is_empty(), "lecture ID is empty");
        ensure!(
            self.end > self.start,
            "invalid time interval for {}",
            self.id
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub categories: Vec<String>,
    pub titles: Vec<String>,
    pub campuses: Vec<String>,
    pub enable_online: bool,
    pub enable_offline: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            categories: vec![],
            titles: vec![],
            campuses: vec![],
            enable_online: true,
            enable_offline: true,
        }
    }
}
pub fn conflicts(a: &Lecture, b: &Lecture, buffer_minutes: i64) -> Result<bool> {
    a.validate()?;
    b.validate()?;
    ensure!(buffer_minutes >= 0, "buffer must be nonnegative");
    let buffer =
        Duration::try_minutes(buffer_minutes).ok_or_else(|| anyhow::anyhow!("buffer overflow"))?;
    Ok(a.start.signed_duration_since(b.end) < buffer
        && b.start.signed_duration_since(a.end) < buffer)
}
pub fn selected_conflicts(
    lectures: &[Lecture],
    buffer_minutes: i64,
) -> Result<Vec<(String, String)>> {
    ensure!(buffer_minutes >= 0, "buffer must be nonnegative");
    let mut pairs = Vec::new();
    for (i, a) in lectures.iter().enumerate() {
        a.validate()?;
        for b in &lectures[i + 1..] {
            if a.id != b.id && conflicts(a, b, buffer_minutes)? {
                pairs.push((a.id.clone(), b.id.clone()));
            }
        }
    }
    Ok(pairs)
}
pub fn filter_lectures(
    lectures: &[Lecture],
    preferences: &Preferences,
    bookings: &[Lecture],
    buffer_minutes: i64,
) -> Result<Vec<Lecture>> {
    ensure!(buffer_minutes >= 0, "buffer must be nonnegative");
    for b in bookings {
        b.validate()?;
    }
    let mut result = Vec::new();
    for l in lectures {
        l.validate()?;
        let named = !preferences.titles.is_empty();
        if !l.reservable
            || (l.online && !preferences.enable_online)
            || (!l.online && !preferences.enable_offline)
        {
            continue;
        }
        if named {
            if !preferences.titles.contains(&l.title) {
                continue;
            }
        } else if (!preferences.categories.is_empty()
            && !preferences.categories.contains(&l.category))
            || (!preferences.campuses.is_empty() && !preferences.campuses.contains(&l.campus))
        {
            continue;
        }
        let mut blocked = false;
        for b in bookings {
            if b.id == l.id || conflicts(l, b, buffer_minutes)? {
                blocked = true;
                break;
            }
        }
        if !blocked {
            result.push(l.clone());
        }
    }
    Ok(result)
}
