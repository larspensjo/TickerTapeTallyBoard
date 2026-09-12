use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Datelike, Utc};

const PREFIX: &str = "portfolio-";
const TIMESTAMP_FORMAT: &str = "%Y%m%dT%H%M%S%3fZ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotKind {
    Launch,
    PreMigration,
    Failed,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFile {
    pub path: PathBuf,
    pub timestamp: DateTime<Utc>,
    pub kind: SnapshotKind,
}

impl SnapshotFile {
    pub fn parse(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        let (stem, kind) = if let Some(stem) = name.strip_suffix(".sqlite.partial") {
            (stem, SnapshotKind::Partial)
        } else if let Some(stem) = name.strip_suffix(".sqlite.failed") {
            (stem, SnapshotKind::Failed)
        } else {
            (name.strip_suffix(".sqlite")?, SnapshotKind::Launch)
        };
        let stem = stem.strip_prefix(PREFIX)?;
        let (stamp, pre_migration) = match stem.strip_suffix("-premigration") {
            Some(stamp) => (stamp, true),
            None => (stem, false),
        };
        let stamp = strip_disambiguator(stamp);
        let timestamp = chrono::NaiveDateTime::parse_from_str(stamp, TIMESTAMP_FORMAT)
            .ok()?
            .and_utc();
        Some(Self {
            path: path.to_path_buf(),
            timestamp,
            kind: if pre_migration && kind == SnapshotKind::Launch {
                SnapshotKind::PreMigration
            } else {
                kind
            },
        })
    }
}

fn strip_disambiguator(stamp: &str) -> &str {
    stamp
        .rsplit_once('-')
        .and_then(|(before, tail)| tail.parse::<u32>().ok().map(|_| before))
        .unwrap_or(stamp)
}

pub(crate) fn snapshot_file_name(
    timestamp: DateTime<Utc>,
    kind: SnapshotKind,
    disambiguator: u32,
) -> String {
    let mut name = format!("{PREFIX}{}", timestamp.format(TIMESTAMP_FORMAT));
    if disambiguator > 1 {
        name.push_str(&format!("-{disambiguator}"));
    }
    if kind == SnapshotKind::PreMigration {
        name.push_str("-premigration");
    }
    name.push_str(".sqlite");
    name
}

#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    pub recent_launches: usize,
    pub weekly_weeks: usize,
    pub monthly_months: usize,
    pub pre_migration_kept: usize,
    pub failed_kept: usize,
    pub abandoned_partial_age: Duration,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            recent_launches: 10,
            weekly_weeks: 8,
            monthly_months: 12,
            pre_migration_kept: 10,
            failed_kept: 3,
            abandoned_partial_age: Duration::from_secs(3600),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPlan {
    pub delete: Vec<PathBuf>,
}

pub fn plan_retention(
    files: &[SnapshotFile],
    now: DateTime<Utc>,
    policy: &RetentionPolicy,
) -> RetentionPlan {
    let mut delete = Vec::new();
    let mut launch: Vec<_> = files
        .iter()
        .filter(|file| file.kind == SnapshotKind::Launch)
        .collect();
    launch.sort_by_key(|file| std::cmp::Reverse(file.timestamp));
    let mut keep = std::collections::HashSet::new();
    for file in launch.iter().take(policy.recent_launches) {
        keep.insert(file.path.clone());
    }
    let weeks = last_weeks(now, policy.weekly_weeks);
    let months = last_months(now, policy.monthly_months);
    let mut week_seen = std::collections::HashSet::new();
    let mut month_seen = std::collections::HashSet::new();
    for file in &launch {
        let week = (
            file.timestamp.iso_week().year(),
            file.timestamp.iso_week().week(),
        );
        if weeks.contains(&week) && week_seen.insert(week) {
            keep.insert(file.path.clone());
        }
        let month = (file.timestamp.year(), file.timestamp.month());
        if months.contains(&month) && month_seen.insert(month) {
            keep.insert(file.path.clone());
        }
    }
    for file in launch {
        if !keep.contains(&file.path) {
            delete.push(file.path.clone());
        }
    }
    for (kind, amount) in [
        (SnapshotKind::PreMigration, policy.pre_migration_kept),
        (SnapshotKind::Failed, policy.failed_kept),
    ] {
        let mut tier: Vec<_> = files.iter().filter(|file| file.kind == kind).collect();
        tier.sort_by_key(|file| std::cmp::Reverse(file.timestamp));
        delete.extend(tier.into_iter().skip(amount).map(|file| file.path.clone()));
    }
    for file in files
        .iter()
        .filter(|file| file.kind == SnapshotKind::Partial)
    {
        if now
            .signed_duration_since(file.timestamp)
            .to_std()
            .is_ok_and(|age| age > policy.abandoned_partial_age)
        {
            delete.push(file.path.clone());
        }
    }
    RetentionPlan { delete }
}

fn last_weeks(now: DateTime<Utc>, count: usize) -> std::collections::HashSet<(i32, u32)> {
    (0..count)
        .map(|offset| {
            let date = now.date_naive() - chrono::Duration::weeks(offset as i64);
            let week = date.iso_week();
            (week.year(), week.week())
        })
        .collect()
}

fn last_months(now: DateTime<Utc>, count: usize) -> std::collections::HashSet<(i32, u32)> {
    (0..count)
        .map(|offset| {
            let total = now.year() * 12 + now.month0() as i32 - offset as i32;
            (total.div_euclid(12), total.rem_euclid(12) as u32 + 1)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> SnapshotFile {
        SnapshotFile::parse(Path::new(name)).expect("valid snapshot")
    }

    fn launch(timestamp: DateTime<Utc>) -> SnapshotFile {
        file(&snapshot_file_name(timestamp, SnapshotKind::Launch, 1))
    }

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("valid test timestamp")
            .with_timezone(&Utc)
    }

    #[test]
    fn generated_snapshot_names_round_trip_through_the_parser() {
        let timestamp = at("2026-09-06T05:28:58.588Z");

        for (kind, disambiguator) in [
            (SnapshotKind::Launch, 1),
            (SnapshotKind::Launch, 2),
            (SnapshotKind::PreMigration, 1),
            (SnapshotKind::PreMigration, 2),
        ] {
            let name = snapshot_file_name(timestamp, kind, disambiguator);
            let parsed = SnapshotFile::parse(Path::new(&name)).expect("writer output should parse");

            assert_eq!(parsed.timestamp, timestamp);
            assert_eq!(parsed.kind, kind);
        }
    }

    #[test]
    fn ten_most_recent_launches_survive_regardless_of_age() {
        let now = at("2026-08-29T14:30:12Z");
        let files: Vec<_> = (0..12)
            .map(|days| launch(now - chrono::Duration::days(1_000 + days)))
            .collect();
        let policy = RetentionPolicy {
            recent_launches: 10,
            weekly_weeks: 0,
            monthly_months: 0,
            ..RetentionPolicy::default()
        };

        let plan = plan_retention(&files, now, &policy);

        assert_eq!(plan.delete.len(), 2);
        for file in files.iter().take(10) {
            assert!(!plan.delete.contains(&file.path));
        }
        for file in files.iter().skip(10) {
            assert!(plan.delete.contains(&file.path));
        }
    }

    #[test]
    fn weekly_tier_keeps_newest_snapshot_in_each_of_last_eight_iso_weeks() {
        let now = at("2026-08-26T14:30:12Z");
        let mut files = Vec::new();
        for week in 0..9 {
            files.push(launch(now - chrono::Duration::weeks(week)));
            files.push(launch(
                now - chrono::Duration::weeks(week) - chrono::Duration::days(1),
            ));
        }
        let policy = RetentionPolicy {
            recent_launches: 0,
            weekly_weeks: 8,
            monthly_months: 0,
            ..RetentionPolicy::default()
        };

        let plan = plan_retention(&files, now, &policy);

        for week in 0..8 {
            assert!(!plan.delete.contains(&files[week * 2].path));
            assert!(plan.delete.contains(&files[week * 2 + 1].path));
        }
        assert!(plan.delete.contains(&files[16].path));
        assert!(plan.delete.contains(&files[17].path));
    }

    #[test]
    fn monthly_tier_keeps_newest_snapshot_in_each_of_last_twelve_months() {
        use chrono::TimeZone;

        let now = at("2026-08-15T14:30:12Z");
        let mut files = Vec::new();
        for offset in 0..13 {
            let total = now.year() * 12 + now.month0() as i32 - offset;
            let year = total.div_euclid(12);
            let month = total.rem_euclid(12) as u32 + 1;
            files.push(launch(
                Utc.with_ymd_and_hms(year, month, 15, 14, 30, 12)
                    .single()
                    .expect("valid month timestamp"),
            ));
            files.push(launch(
                Utc.with_ymd_and_hms(year, month, 10, 14, 30, 12)
                    .single()
                    .expect("valid month timestamp"),
            ));
        }
        let policy = RetentionPolicy {
            recent_launches: 0,
            weekly_weeks: 0,
            monthly_months: 12,
            ..RetentionPolicy::default()
        };

        let plan = plan_retention(&files, now, &policy);

        for month in 0..12 {
            assert!(!plan.delete.contains(&files[month * 2].path));
            assert!(plan.delete.contains(&files[month * 2 + 1].path));
        }
        assert!(plan.delete.contains(&files[24].path));
        assert!(plan.delete.contains(&files[25].path));
    }

    #[test]
    fn planner_keeps_tiers_and_reclaims_old_partials() {
        let now = DateTime::parse_from_rfc3339("2026-08-29T14:30:12Z")
            .unwrap()
            .with_timezone(&Utc);
        let policy = RetentionPolicy {
            recent_launches: 1,
            weekly_weeks: 0,
            monthly_months: 0,
            pre_migration_kept: 1,
            failed_kept: 1,
            abandoned_partial_age: Duration::from_secs(3600),
        };
        let old = file("portfolio-20260829T120000000Z.sqlite.partial");
        let young = file("portfolio-20260829T141500000Z.sqlite.partial");
        let files = vec![
            file("portfolio-20260829T140000000Z.sqlite"),
            file("portfolio-20260828T140000000Z.sqlite"),
            file("portfolio-20260829T140000000Z-premigration.sqlite"),
            file("portfolio-20260828T140000000Z-premigration.sqlite"),
            old.clone(),
            young.clone(),
        ];
        let plan = plan_retention(&files, now, &policy);
        assert!(plan.delete.contains(&old.path));
        assert!(!plan.delete.contains(&young.path));
        assert_eq!(plan.delete.len(), 3);
    }

    #[test]
    fn empty_and_unparsable_are_safe() {
        let now = crate::clock::now_utc();
        let plan = plan_retention(&[], now, &RetentionPolicy::default());
        assert!(plan.delete.is_empty());
        assert!(SnapshotFile::parse(Path::new("notes.sqlite")).is_none());
    }
}
