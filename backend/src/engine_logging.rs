use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use crate::{
    config::{desktop_log_file, AppConfig, LogFile},
    state::AppShell,
};

pub const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024;
pub const DEFAULT_KEPT_ROTATIONS: usize = 3;

#[macro_export]
macro_rules! engine_trace { ($($arg:tt)*) => {{ log::trace!($($arg)*); }}; }
#[macro_export]
macro_rules! engine_info { ($($arg:tt)*) => {{ log::info!($($arg)*); }}; }
#[macro_export]
macro_rules! engine_debug { ($($arg:tt)*) => {{ log::debug!($($arg)*); }}; }
#[macro_export]
macro_rules! engine_warn { ($($arg:tt)*) => {{ log::warn!($($arg)*); }}; }
#[macro_export]
macro_rules! engine_error { ($($arg:tt)*) => {{ log::error!($($arg)*); }}; }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSettings {
    pub file_path: PathBuf,
    pub max_bytes: u64,
    pub kept_rotations: usize,
    pub terminal: bool,
    /// Prefix completed file records with this process identity. Terminal output
    /// remains intentionally untagged.
    pub instance_tag: Option<String>,
}

impl LogSettings {
    pub fn with_defaults(file_path: PathBuf) -> Self {
        Self {
            file_path,
            max_bytes: DEFAULT_MAX_BYTES,
            kept_rotations: DEFAULT_KEPT_ROTATIONS,
            terminal: true,
            instance_tag: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogInitOutcome {
    pub file_path: Option<PathBuf>,
    pub file_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEvent {
    Startup,
    Shutdown,
}

impl LifecycleEvent {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Shutdown => "shutdown",
        }
    }
}

pub fn initialize_for_shell(shell: AppShell, config: &AppConfig) -> LogInitOutcome {
    let log_file = match shell {
        AppShell::Server => config.log_file.clone(),
        AppShell::Desktop => desktop_log_file(config.mode),
    };
    let outcome = match log_file {
        LogFile::Resolved(path) => {
            let mut settings = LogSettings::with_defaults(path);
            settings.max_bytes = config.log_max_bytes;
            settings.terminal = shell == AppShell::Server || cfg!(debug_assertions);
            settings.instance_tag = Some(format!("{}:{}", shell.as_str(), std::process::id()));
            initialize(&settings)
        }
        LogFile::Unresolved(reason) => {
            initialize_terminal();
            LogInitOutcome {
                file_path: None,
                file_error: Some(reason),
            }
        }
    };
    if let Some(error) = &outcome.file_error {
        crate::engine_error!(
            "{} file logging unavailable; using terminal logging only: {error}",
            shell.as_str()
        );
    }
    outcome
}

pub fn lifecycle_banner(
    event: LifecycleEvent,
    shell: AppShell,
    config: &AppConfig,
    log_outcome: &LogInitOutcome,
) -> String {
    let ledger = config.ledger.path.as_ref().map_or_else(
        || "in-memory (demo)".to_owned(),
        |path| path.display().to_string(),
    );
    let log_path = log_outcome.file_path.as_ref().map_or_else(
        || {
            format!(
                "unavailable ({})",
                log_outcome.file_error.as_deref().unwrap_or("unknown error")
            )
        },
        |path| path.display().to_string(),
    );
    let executable = std::env::current_exe().map_or_else(
        |error| format!("unavailable ({error})"),
        |path| path.display().to_string(),
    );
    format!(
        "{}: shell={} executable={} mode={} ledger={} backup_dir={} static_assets_dir={} log={}",
        event.as_str(),
        shell.as_str(),
        executable,
        config.mode.as_str(),
        ledger,
        config.backup_dir.display(),
        config.static_assets_dir().display(),
        log_path,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationRename {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationPlan {
    pub delete: Option<PathBuf>,
    pub renames: Vec<RotationRename>,
}

/// Plans a rotation without inspecting the filesystem. Rotation names are
/// `<base>.1`, `<base>.2`, and so on, with `.1` always the newest archive.
pub fn plan_rotation(base: &Path, kept: usize) -> RotationPlan {
    if kept == 0 {
        return RotationPlan {
            delete: Some(base.to_path_buf()),
            renames: Vec::new(),
        };
    }
    let mut renames = Vec::with_capacity(kept);
    for index in (1..kept).rev() {
        renames.push(RotationRename {
            from: rotated_path(base, index),
            to: rotated_path(base, index + 1),
        });
    }
    renames.push(RotationRename {
        from: base.to_path_buf(),
        to: rotated_path(base, 1),
    });
    RotationPlan {
        delete: Some(rotated_path(base, kept)),
        renames,
    }
}

fn rotated_path(base: &Path, index: usize) -> PathBuf {
    let mut path = base.as_os_str().to_os_string();
    path.push(format!(".{index}"));
    PathBuf::from(path)
}

pub fn initialize(settings: &LogSettings) -> LogInitOutcome {
    match RotatingFileWriter::open_tagged(
        settings.file_path.clone(),
        settings.max_bytes,
        settings.kept_rotations,
        settings.instance_tag.clone(),
    ) {
        Ok(writer) => {
            initialize_loggers(settings.terminal, Some(writer));
            LogInitOutcome {
                file_path: Some(settings.file_path.clone()),
                file_error: None,
            }
        }
        Err(error) => {
            initialize_loggers(settings.terminal, None);
            LogInitOutcome {
                file_path: None,
                file_error: Some(format!(
                    "could not open log file {}: {error}",
                    settings.file_path.display()
                )),
            }
        }
    }
}

pub fn initialize_terminal() {
    initialize_loggers(true, None);
}

fn initialize_loggers(terminal: bool, writer: Option<RotatingFileWriter>) {
    use simplelog::{
        ColorChoice, CombinedLogger, Config, SharedLogger, TermLogger, TerminalMode, WriteLogger,
    };

    let level = log::LevelFilter::Info;
    let mut loggers: Vec<Box<dyn SharedLogger>> = Vec::new();
    if terminal {
        loggers.push(TermLogger::new(
            level,
            Config::default(),
            TerminalMode::Stderr,
            ColorChoice::Auto,
        ));
    }
    if let Some(writer) = writer {
        loggers.push(WriteLogger::new(level, Config::default(), writer));
    }
    if !loggers.is_empty() {
        let _ = CombinedLogger::init(loggers);
    }
}

pub struct RotatingFileWriter {
    path: PathBuf,
    file: Option<File>,
    max_bytes: u64,
    kept_rotations: usize,
    instance_tag: Option<String>,
    pending: Vec<u8>,
}

impl RotatingFileWriter {
    pub fn open(path: PathBuf, max_bytes: u64, kept_rotations: usize) -> io::Result<Self> {
        Self::open_tagged(path, max_bytes, kept_rotations, None)
    }

    pub fn open_tagged(
        path: PathBuf,
        max_bytes: u64,
        kept_rotations: usize,
        instance_tag: Option<String>,
    ) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let file = open_append(&path)?;
        Ok(Self {
            path,
            file: Some(file),
            max_bytes,
            kept_rotations,
            instance_tag,
            pending: Vec::new(),
        })
    }

    fn write_record(&mut self, record: &[u8]) -> io::Result<()> {
        // This identity check costs an open/close plus platform file-identity
        // queries per record. Rotation-lock contention is bounded to one second;
        // current log volume is low enough that the simpler safety check wins.
        self.follow_foreign_rotation()?;
        let record = self.tagged_record(record);
        let live_bytes = self.file_ref()?.metadata()?.len();
        if live_bytes > 0
            && self.max_bytes > 0
            && live_bytes.saturating_add(record.len() as u64) > self.max_bytes
        {
            self.rotate()?;
        }
        // Another process can still rename the live file after the identity check
        // and before this append. That narrow race may place one record in the
        // newest archive; the next record reopens the live path.
        self.file_mut()?.write_all(&record)?;
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        let _lock = RotationLock::acquire(&self.path)?;
        if !self.holds_live_file()? {
            self.reopen_live_file()?;
            return Ok(());
        }
        self.file_mut()?.flush()?;
        let plan = plan_rotation(&self.path, self.kept_rotations);
        if plan.renames.is_empty() {
            drop(self.file.take());
            match open_truncate(&self.path) {
                Ok(file) => self.file = Some(file),
                Err(error) => {
                    self.file = open_append(&self.path).ok();
                    return Err(error);
                }
            }
        } else {
            if let Some(path) = plan.delete.filter(|path| path.exists()) {
                fs::remove_file(path)?;
            }
            for rename in plan.renames {
                if rename.from.exists() {
                    fs::rename(rename.from, rename.to)?;
                }
            }
            self.file = Some(open_append(&self.path)?);
        }
        Ok(())
    }

    fn tagged_record(&self, record: &[u8]) -> Vec<u8> {
        match &self.instance_tag {
            None => record.to_vec(),
            Some(tag) => {
                let mut tagged = Vec::with_capacity(tag.len() + record.len() + 3);
                tagged.extend_from_slice(b"[");
                tagged.extend_from_slice(tag.as_bytes());
                tagged.extend_from_slice(b"] ");
                tagged.extend_from_slice(record);
                tagged
            }
        }
    }

    fn follow_foreign_rotation(&mut self) -> io::Result<()> {
        if !self.holds_live_file()? {
            self.reopen_live_file()?;
        }
        Ok(())
    }

    fn holds_live_file(&self) -> io::Result<bool> {
        let live = match OpenOptions::new().read(true).open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        same_file(self.file_ref()?, &live)
    }

    fn reopen_live_file(&mut self) -> io::Result<()> {
        self.file = Some(open_append(&self.path)?);
        Ok(())
    }

    fn file_mut(&mut self) -> io::Result<&mut File> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "log file is not open"))
    }

    fn file_ref(&self) -> io::Result<&File> {
        self.file
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "log file is not open"))
    }

    fn write_complete_records(&mut self) -> io::Result<()> {
        // Rotation boundaries are newline-delimited log lines. An embedded newline
        // creates another boundary, so multiline messages are not kept atomic.
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let record: Vec<u8> = self.pending.drain(..=end).collect();
            self.write_record(&record)?;
        }
        Ok(())
    }
}

struct RotationLock {
    _file: File,
}

impl RotationLock {
    fn acquire(log_path: &Path) -> io::Result<Self> {
        let mut lock_path = log_path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lock_path = PathBuf::from(lock_path);
        if let Some(parent) = lock_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        for attempt in 0..200 {
            match open_exclusive_lock(&lock_path) {
                Ok(file) => return Ok(Self { _file: file }),
                Err(error) if is_lock_contention(&error) && attempt < 199 => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("the bounded rotation lock retry loop always returns")
    }
}

fn is_lock_contention(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33))
}

#[cfg(windows)]
fn open_exclusive_lock(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .share_mode(0)
        .open(path)
}

#[cfg(not(windows))]
fn open_exclusive_lock(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).write(true).open(path)
}

#[cfg(windows)]
fn same_file(held: &File, live: &File) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    fn identity(file: &File) -> io::Result<(u32, u64)> {
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle comes from a live `File` and the output buffer is valid.
        let result =
            unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((
            information.dwVolumeSerialNumber,
            (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
        ))
    }

    Ok(identity(held)? == identity(live)?)
}

#[cfg(not(windows))]
fn same_file(held: &File, live: &File) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let held = held.metadata()?;
    let live = live.metadata()?;
    Ok(held.dev() == live.dev() && held.ino() == live.ino())
}

impl Write for RotatingFileWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buffer);
        self.write_complete_records()?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            let record = std::mem::take(&mut self.pending);
            self.write_record(&record)?;
        }
        self.file_mut()?.flush()
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn open_truncate(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::{
        initialize, lifecycle_banner, plan_rotation, rotated_path, LifecycleEvent, LogInitOutcome,
        LogSettings, RotatingFileWriter, RotationRename, DEFAULT_MAX_BYTES,
    };
    use std::{
        fs,
        io::Write,
        net::{IpAddr, Ipv4Addr},
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        config::{AppConfig, BackupDirectory, LogFile, Mode},
        ledger,
        state::AppShell,
    };

    #[test]
    fn rotation_plan_renames_descending_and_deletes_the_oldest_archive() {
        let base = PathBuf::from("target/test-engine-logging/engine.log");
        let plan = plan_rotation(&base, 3);
        assert_eq!(
            plan.delete,
            Some(PathBuf::from("target/test-engine-logging/engine.log.3"))
        );
        assert_eq!(
            plan.renames,
            vec![
                RotationRename {
                    from: PathBuf::from("target/test-engine-logging/engine.log.2"),
                    to: PathBuf::from("target/test-engine-logging/engine.log.3")
                },
                RotationRename {
                    from: PathBuf::from("target/test-engine-logging/engine.log.1"),
                    to: PathBuf::from("target/test-engine-logging/engine.log.2")
                },
                RotationRename {
                    from: base.clone(),
                    to: PathBuf::from("target/test-engine-logging/engine.log.1")
                },
            ]
        );
    }

    #[test]
    fn rotation_plan_for_a_fresh_path_has_no_renames_when_no_archives_are_kept() {
        let base = PathBuf::from("target/test-engine-logging/fresh.log");
        let plan = plan_rotation(&base, 0);
        assert_eq!(plan.delete, Some(base));
        assert!(plan.renames.is_empty());
    }

    #[test]
    fn rotating_writer_bounds_archives_and_keeps_complete_newest_lines_in_base() {
        let directory = unique_dir("rotation");
        let path = directory.join("engine.log");
        let mut writer = RotatingFileWriter::open(path.clone(), 12, 2).expect("writer should open");
        for record in [
            b"first line\n".as_slice(),
            b"second line\n",
            b"third line\n",
            b"fourth line\n",
        ] {
            writer.write_all(record).expect("record should write");
        }
        writer.flush().expect("writer should flush");
        assert!(
            fs::read_dir(&directory)
                .expect("directory should be readable")
                .filter(|entry| entry
                    .as_ref()
                    .is_ok_and(|entry| entry.path() != path.with_extension("log.lock")))
                .count()
                <= 3
        );
        assert_eq!(
            fs::read_to_string(&path).expect("base log should read"),
            "fourth line\n"
        );
        for entry in fs::read_dir(&directory)
            .expect("directory should be readable")
            .filter_map(Result::ok)
            .filter(|entry| entry.path() != path.with_extension("log.lock"))
        {
            let contents = fs::read_to_string(entry.path()).expect("rotated log should read");
            assert!(
                contents.ends_with('\n'),
                "lines must not be split: {contents:?}"
            );
        }
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn rotating_writer_truncates_the_live_file_when_no_archives_are_kept() {
        let directory = unique_dir("no-archives");
        let path = directory.join("engine.log");
        let mut writer = RotatingFileWriter::open(path.clone(), 6, 0).expect("writer should open");

        writer
            .write_all(b"first\n")
            .expect("first line should write");
        writer
            .write_all(b"second\n")
            .expect("second line should rotate");
        writer.flush().expect("writer should flush");

        assert_eq!(
            fs::read_to_string(&path).expect("base log should read"),
            "second\n"
        );
        assert!(!rotated_path(&path, 1).exists());
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn rotating_writer_keeps_a_fragmented_line_together_across_rotation() {
        let directory = unique_dir("fragmented-line");
        let path = directory.join("engine.log");
        let mut writer = RotatingFileWriter::open(path.clone(), 24, 1).expect("writer should open");
        writer.write_all(b"seed\n").expect("seed should write");

        writer
            .write_all(b"12:00 [INFO] par")
            .expect("first fragment should buffer");
        assert_eq!(
            fs::read_to_string(&path).expect("base log should read"),
            "seed\n"
        );

        writer
            .write_all(b"t two\n")
            .expect("second fragment should complete the line");
        writer.flush().expect("writer should flush");

        assert_eq!(
            fs::read_to_string(&path).expect("base log should read"),
            "12:00 [INFO] part two\n"
        );
        assert_eq!(
            fs::read_to_string(rotated_path(&path, 1)).expect("archive should read"),
            "seed\n"
        );
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn initialize_reports_an_unwritable_file_path_without_panicking() {
        let directory = unique_dir("unwritable");
        fs::create_dir_all(&directory).expect("test directory should be created");
        let settings = LogSettings {
            file_path: directory.clone(),
            max_bytes: 1,
            kept_rotations: 0,
            terminal: false,
            instance_tag: None,
        };
        let outcome = initialize(&settings);
        assert!(outcome.file_path.is_none());
        assert!(outcome.file_error.is_some());
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn tagged_writer_tags_each_completed_line_once_after_split_writes() {
        let directory = unique_dir("tagged-split");
        let path = directory.join("engine.log");
        let mut writer =
            RotatingFileWriter::open_tagged(path.clone(), 1024, 1, Some("desktop:42".to_owned()))
                .expect("writer should open");

        writer
            .write_all(b"first li")
            .expect("fragment should buffer");
        assert_eq!(fs::read_to_string(&path).expect("log should read"), "");
        writer
            .write_all(b"ne\nsecond\nthi")
            .expect("completed lines should write");
        writer
            .write_all(b"rd\n")
            .expect("split line should complete");
        writer.flush().expect("writer should flush");

        assert_eq!(
            fs::read_to_string(&path).expect("log should read"),
            "[desktop:42] first line\n[desktop:42] second\n[desktop:42] third\n"
        );
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn independent_writers_rotate_a_shared_file_without_writing_into_an_archive() {
        let directory = unique_dir("two-writers");
        let path = directory.join("engine.log");
        let max_bytes = 64;
        let kept_rotations = 2;
        let mut first = RotatingFileWriter::open_tagged(
            path.clone(),
            max_bytes,
            kept_rotations,
            Some("first".to_owned()),
        )
        .expect("first writer opens");
        let mut second = RotatingFileWriter::open_tagged(
            path.clone(),
            max_bytes,
            kept_rotations,
            Some("second".to_owned()),
        )
        .expect("second writer opens");
        let mut largest_record = 0_u64;

        for index in 0..80 {
            let (writer, tag) = if index % 2 == 0 {
                (&mut first, "first")
            } else {
                (&mut second, "second")
            };
            let marker = format!("record-{index:03}");
            let record = format!("{marker}\n");
            largest_record = largest_record.max((tag.len() + record.len() + 3) as u64);
            writer
                .write_all(record.as_bytes())
                .expect("both writers keep writing after rotations");
            assert!(
                fs::read_to_string(&path)
                    .expect("live log should read")
                    .contains(&marker),
                "the newest write must land in the live file"
            );
        }
        first.flush().expect("first flush");
        second.flush().expect("second flush");

        let expected = vec![path.clone(), rotated_path(&path, 1), rotated_path(&path, 2)];
        let mut actual: Vec<_> = fs::read_dir(&directory)
            .expect("directory should read")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|candidate| candidate != &path.with_extension("log.lock"))
            .collect();
        actual.sort();
        let mut expected_sorted = expected.clone();
        expected_sorted.sort();
        assert_eq!(actual, expected_sorted, "the archive set is exact");

        let mut total_bytes = 0_u64;
        for file in expected {
            let text = fs::read_to_string(&file).expect("log should read");
            total_bytes += fs::metadata(&file).expect("metadata should read").len();
            for line in text.lines() {
                let tag_count = line.matches("[first]").count() + line.matches("[second]").count();
                assert_eq!(
                    tag_count, 1,
                    "each record has exactly one writer tag: {line:?}"
                );
            }
        }
        assert!(
            total_bytes <= (kept_rotations as u64 + 1) * max_bytes + largest_record,
            "retained bytes stay within the configured bound plus one record"
        );
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn writer_follows_a_live_file_renamed_out_from_under_it() {
        let directory = unique_dir("foreign-rotation");
        let path = directory.join("engine.log");
        let renamed = directory.join("renamed.log");
        let mut writer = RotatingFileWriter::open(path.clone(), 100, 1).expect("writer opens");

        writer.write_all(b"seed\n").expect("seed write");
        fs::rename(&path, &renamed).expect("live file should be renamed");
        writer
            .write_all(b"live\n")
            .expect("writer should recreate and follow the live path");
        writer.flush().expect("writer flush");

        assert_eq!(fs::read_to_string(&path).expect("live log"), "live\n");
        assert_eq!(fs::read_to_string(&renamed).expect("renamed log"), "seed\n");
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn writer_recreates_a_missing_live_path() {
        let directory = unique_dir("missing-live");
        let path = directory.join("engine.log");
        let mut writer = RotatingFileWriter::open(path.clone(), 100, 1).expect("writer opens");

        writer.write_all(b"discarded\n").expect("seed write");
        fs::remove_file(&path).expect("live path should be removed");
        writer
            .write_all(b"replacement\n")
            .expect("writer should recreate a missing live path");
        writer.flush().expect("writer flush");

        assert_eq!(
            fs::read_to_string(&path).expect("replacement log"),
            "replacement\n"
        );
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn lifecycle_banners_name_each_shell_event_and_demo_ledger() {
        let config = AppConfig {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8480,
            ledger: ledger::memory(),
            static_assets_dir: PathBuf::from("C:/source/frontend/dist"),
            mode: Mode::Demo,
            create_ledger_if_missing: false,
            backup_enabled: false,
            backup_dir: BackupDirectory::Unresolved("demo".to_owned()),
            log_file: LogFile::Unresolved("test".to_owned()),
            log_max_bytes: DEFAULT_MAX_BYTES,
            market_data_refresh_enabled: false,
            launch_refresh_enabled: false,
        };
        let outcome = LogInitOutcome {
            file_path: Some(PathBuf::from("C:/logs/engine.log")),
            file_error: None,
        };

        for shell in [AppShell::Server, AppShell::Desktop] {
            for (event, label) in [
                (LifecycleEvent::Startup, "startup"),
                (LifecycleEvent::Shutdown, "shutdown"),
            ] {
                let banner = lifecycle_banner(event, shell, &config, &outcome);
                assert!(banner.starts_with(label));
                assert!(banner.contains(&format!("shell={}", shell.as_str())));
                assert!(banner.contains("ledger=in-memory (demo)"));
                assert!(!banner.contains(['\r', '\n']));
            }
        }
    }

    fn unique_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        crate::test_support::workspace_target_path("test-engine-logging")
            .join(format!("{name}-{unique}"))
    }
}
