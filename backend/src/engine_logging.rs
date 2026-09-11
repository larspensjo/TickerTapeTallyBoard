use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
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
}

impl LogSettings {
    pub fn with_defaults(file_path: PathBuf) -> Self {
        Self {
            file_path,
            max_bytes: DEFAULT_MAX_BYTES,
            kept_rotations: DEFAULT_KEPT_ROTATIONS,
            terminal: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogInitOutcome {
    pub file_path: Option<PathBuf>,
    pub file_error: Option<String>,
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
    match RotatingFileWriter::open(
        settings.file_path.clone(),
        settings.max_bytes,
        settings.kept_rotations,
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
    bytes_written: u64,
    max_bytes: u64,
    kept_rotations: usize,
    pending: Vec<u8>,
}

impl RotatingFileWriter {
    pub fn open(path: PathBuf, max_bytes: u64, kept_rotations: usize) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let file = open_append(&path)?;
        let bytes_written = file.metadata()?.len();
        Ok(Self {
            path,
            file: Some(file),
            bytes_written,
            max_bytes,
            kept_rotations,
            pending: Vec::new(),
        })
    }

    fn write_record(&mut self, record: &[u8]) -> io::Result<()> {
        if self.bytes_written > 0
            && self.max_bytes > 0
            && self.bytes_written.saturating_add(record.len() as u64) > self.max_bytes
        {
            self.rotate()?;
        }
        self.file_mut()?.write_all(record)?;
        self.bytes_written = self.bytes_written.saturating_add(record.len() as u64);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
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
        self.bytes_written = 0;
        Ok(())
    }

    fn file_mut(&mut self) -> io::Result<&mut File> {
        self.file
            .as_mut()
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
        initialize, plan_rotation, rotated_path, LogSettings, RotatingFileWriter, RotationRename,
    };
    use std::{
        env, fs,
        io::Write,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
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
                .count()
                <= 3
        );
        assert_eq!(
            fs::read_to_string(&path).expect("base log should read"),
            "fourth line\n"
        );
        for entry in fs::read_dir(&directory).expect("directory should be readable") {
            let contents = fs::read_to_string(entry.expect("directory entry").path())
                .expect("rotated log should read");
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
        let parent_file = unique_dir("unwritable").with_extension("file");
        fs::create_dir_all(parent_file.parent().expect("parent directory"))
            .expect("test parent directory should be created");
        fs::write(&parent_file, "not a directory").expect("parent file should be created");
        let settings = LogSettings {
            file_path: parent_file.join("engine.log"),
            max_bytes: 1,
            kept_rotations: 0,
            terminal: false,
        };
        let outcome = initialize(&settings);
        assert!(outcome.file_path.is_none());
        assert!(outcome.file_error.is_some());
        fs::remove_file(parent_file).expect("test parent file should be removed");
    }

    fn unique_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        env::current_dir()
            .expect("current directory should resolve")
            .join("target")
            .join("test-engine-logging")
            .join(format!("{name}-{unique}"))
    }
}
