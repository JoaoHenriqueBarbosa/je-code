//! Porte de `src/vs/platform/log/node/loggerService.ts` e `spdlogLog.ts`: o serviço de loggers que escreve
//! arquivos de log rotativos em `logs/`.
//!
//! O `SpdLogLogger` usa o `@vscode/spdlog` (C++), proibido aqui. O [`RotatingFileSink`] reproduz o
//! que o spdlog faz pelo VS Code: arquivo `name.log` com o padrão `%Y-%m-%d %H:%M:%S.%e [%l] %v`, rotação por
//! tamanho (os backups se chamam `name.1.log`, `name.2.log`..., como no `rotating_file_sink` do spdlog 1.x)
//! e escrita imediata (o `setFlushOn(Trace)` do original). A criação assíncrona do spdlog, que obrigava o
//! `SpdLogLogger` a guardar mensagens até o logger existir, não existe: o arquivo é aberto na primeira
//! escrita. Divergência não medida: o nome exato dos backups segue o spdlog 1.x e não foi conferido com o
//! `@vscode/spdlog` empacotado.

use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_base::uri::URI;

use super::{AbstractLoggerService, LogLevel, LoggerOptions, LoggerRef, MessageLogger, MessageSink, current_timestamp};
use crate::files::files::byte_size;
use crate::resources::fs_path;

struct SinkState {
    file: Option<File>,
    size: u64,
}

/// O destino do `SpdLogLogger`: um arquivo rotativo.
pub struct RotatingFileSink {
    path: PathBuf,
    max_size: u64,
    max_files: u32,
    donot_use_formatters: bool,
    state: RefCell<SinkState>,
}

fn level_name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Trace => "trace",
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warning => "warning",
        LogLevel::Error => "error",
        LogLevel::Off => "off",
    }
}

impl RotatingFileSink {
    pub fn new(path: &Path, rotating: bool, donot_use_formatters: bool) -> Self {
        let file_count: u32 = if rotating { 6 } else { 1 };
        Self {
            path: path.to_path_buf(),
            max_size: u64::from(30 / file_count) * byte_size::MB,
            max_files: file_count,
            donot_use_formatters,
            state: RefCell::new(SinkState { file: None, size: 0 }),
        }
    }

    /// `calc_filename(filename, index)` do spdlog: `name.log`, `name.1.log`...
    fn indexed_path(&self, index: u32) -> PathBuf {
        if index == 0 {
            return self.path.clone();
        }
        let stem = self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = self.path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        self.path.with_file_name(format!("{stem}.{index}{ext}"))
    }

    fn open(&self, state: &mut SinkState) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        state.size = file.metadata()?.len();
        state.file = Some(file);
        Ok(())
    }

    /// `rotate_()` do spdlog: `name.log` vira `name.1.log`, `name.1.log` vira `name.2.log`...
    fn rotate(&self, state: &mut SinkState) -> std::io::Result<()> {
        state.file = None;
        for index in (1..=self.max_files).rev() {
            let source = self.indexed_path(index - 1);
            if !source.exists() {
                continue;
            }
            let target = self.indexed_path(index);
            if target.exists() {
                fs::remove_file(&target)?;
            }
            fs::rename(&source, &target)?;
        }
        self.open(state)
    }

    fn write(&self, data: &str) -> std::io::Result<()> {
        let mut state = self.state.borrow_mut();
        if state.file.is_none() {
            self.open(&mut state)?;
        }
        let len = data.len() as u64;
        if state.size > 0 && state.size + len > self.max_size {
            self.rotate(&mut state)?;
        }
        if let Some(file) = state.file.as_mut() {
            file.write_all(data.as_bytes())?;
            file.flush()?;
        }
        state.size += len;
        Ok(())
    }
}

impl MessageSink for RotatingFileSink {
    fn log(&self, level: LogLevel, message: &str) {
        let line = if self.donot_use_formatters {
            message.to_owned()
        } else {
            format!("{} [{}] {message}\n", current_timestamp(), level_name(level))
        };
        if let Err(error) = self.write(&line) {
            eprintln!("{error}");
        }
    }

    fn dispose(&self) {
        self.state.borrow_mut().file = None;
    }
}

/// `SpdLogLogger`.
pub type SpdLogLogger = MessageLogger<RotatingFileSink>;

impl SpdLogLogger {
    pub fn new_rotating(filepath: &Path, rotating: bool, donot_use_formatters: bool, level: LogLevel) -> Rc<Self> {
        let logger = MessageLogger::new(RotatingFileSink::new(filepath, rotating, donot_use_formatters));
        super::Logger::set_level(logger.as_ref(), level);
        logger
    }
}

/// `LoggerService` do node: um logger rotativo por arquivo, em `logs_home`.
pub fn logger_service(log_level: LogLevel, logs_home: URI) -> Rc<AbstractLoggerService> {
    AbstractLoggerService::new(
        log_level,
        logs_home,
        Vec::new(),
        Rc::new(|resource: &URI, log_level: LogLevel, options: &LoggerOptions| -> LoggerRef {
            SpdLogLogger::new_rotating(Path::new(&fs_path(resource)), !options.donot_rotate, options.donot_use_formatters, log_level)
        }),
    )
}
