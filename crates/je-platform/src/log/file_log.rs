//! Porte de `src/vs/platform/log/common/bufferLog.ts` e `fileLog.ts`: o logger que guarda as mensagens até
//! ter um destino ([`BufferLogger`]), o logger que escreve num arquivo pelo `IFileService` ([`FileLogger`]) e o
//! serviço de loggers de arquivo ([`file_logger_service`]).
//!
//! `FileLogger.flush` é `async` no original e o `ILogger.flush` devolve `void`: o trait [`Logger`] dispara
//! a escrita e solta a promessa, e [`FileLoggerSink::flush_async`] devolve a promessa para quem quiser
//! esperar.

use crate::files::i_file_service::IFileService;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::{ThrottledDelayer, async_fn};
use je_base::buffer::VSBuffer;
use je_base::lifecycle::{DisposableRef, IDisposable, MutableDisposable};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;

use super::{AbstractLoggerService, LogLevel, Logger, LoggerOptions, LoggerRef, MessageLogger, MessageSink, current_timestamp, log};
use crate::files::file_service::{FileService, when_provider_registered};
use crate::files::files::{FileOperationError, FileOperationResult, byte_size};
use crate::resources::host_environment;

/// `ILog` de `bufferLog.ts`.
struct BufferedLog {
    level: LogLevel,
    message: String,
}

/// O destino de `BufferLogger`: guarda as mensagens até o `logger` ser definido.
pub struct BufferSink {
    buffer: RefCell<Vec<BufferedLog>>,
    logger: RefCell<Option<LoggerRef>>,
    log_level_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    level: Rc<Cell<LogLevel>>,
}

impl MessageSink for BufferSink {
    fn log(&self, level: LogLevel, message: &str) {
        if let Some(logger) = self.logger.borrow().as_ref() {
            log(logger.as_ref(), level, message);
        } else if self.level.get() <= level {
            self.buffer.borrow_mut().push(BufferedLog { level, message: message.to_owned() });
        }
    }

    fn flush(&self) {
        if let Some(logger) = self.logger.borrow().as_ref() {
            logger.flush();
        }
    }

    fn dispose(&self) {
        if let Some(logger) = self.logger.borrow().as_ref() {
            logger.dispose();
        }
        self.log_level_disposable.dispose();
    }
}

/// `BufferLogger`.
pub type BufferLogger = MessageLogger<BufferSink>;

impl BufferLogger {
    pub fn new_buffer(log_level: LogLevel) -> Rc<Self> {
        let logger = MessageLogger::build(|level| BufferSink {
            buffer: RefCell::new(Vec::new()),
            logger: RefCell::new(None),
            log_level_disposable: MutableDisposable::new(),
            level,
        });
        logger.set_level(log_level);
        logger
    }

    /// `set logger(logger)`: passa a escrever nele e despeja o que estava guardado.
    pub fn set_logger(self: &Rc<Self>, logger: LoggerRef) {
        *self.sink.logger.borrow_mut() = Some(logger.clone());
        self.set_level(logger.get_level());
        let weak = Rc::downgrade(self);
        let listener: DisposableRef = logger.on_did_change_log_level().listen(move |level| {
            if let Some(this) = weak.upgrade() {
                this.set_level(*level);
            }
        });
        self.sink.log_level_disposable.set(Some(listener));

        let buffered = std::mem::take(&mut *self.sink.buffer.borrow_mut());
        for BufferedLog { level, message } in buffered {
            log(logger.as_ref(), level, &message);
        }
    }
}

const MAX_FILE_SIZE: u64 = 5 * byte_size::MB;

struct FileLoggerInner {
    resource: URI,
    donot_use_formatters: bool,
    file_service: Rc<dyn IFileService>,
    initialize_promise: Promise<()>,
    flush_delayer: ThrottledDelayer<()>,
    backup_index: Cell<u32>,
    buffer: RefCell<String>,
}

/// O destino de `FileLogger`.
pub struct FileLoggerSink(Rc<FileLoggerInner>);

impl FileLoggerInner {
    fn initialize(file_service: &Rc<dyn IFileService>, resource: &URI) -> Promise<()> {
        let (file_service, resource) = (file_service.clone(), resource.clone());
        async_fn(async move {
            match file_service.create_file(&resource, None, None).await {
                Ok(_) => Ok(()),
                Err(error) => match FileOperationError::from_error(&error) {
                    Some(operation) if operation.file_operation_result == FileOperationResult::FileModifiedSince => Ok(()),
                    _ => Err(error),
                },
            }
        })
    }

    fn get_backup_resource(&self) -> URI {
        let index = if self.backup_index.get() > 5 { 1 } else { self.backup_index.get() };
        self.backup_index.set(index + 1);
        let ext_uri = ExtUri::unbiased(host_environment());
        ext_uri.join_path(&ext_uri.dirname(&self.resource), &[&format!("{}_{index}", ext_uri.basename(&self.resource, None))])
    }

    fn load_content(self: &Rc<Self>) -> Promise<String> {
        let this = self.clone();
        async_fn(async move {
            match this.file_service.read_file(&this.resource, None, None).await {
                Ok(content) => Ok(content.value.to_string()),
                Err(_) => Ok(String::new()),
            }
        })
    }

    fn flush(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            if this.buffer.borrow().is_empty() {
                return Ok(());
            }
            this.initialize_promise.clone().await?;
            let mut content = this.load_content().await?;
            if content.encode_utf16().count() as u64 > MAX_FILE_SIZE {
                this.file_service.write_file(&this.get_backup_resource(), VSBuffer::from_string(&content).into(), None).await?;
                content = String::new();
            }
            if !this.buffer.borrow().is_empty() {
                content.push_str(&this.buffer.take());
                this.file_service.write_file(&this.resource, VSBuffer::from_string(&content).into(), None).await?;
            }
            Ok(())
        })
    }
}

fn stringify_log_level(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "debug",
        LogLevel::Error => "error",
        LogLevel::Info => "info",
        LogLevel::Trace => "trace",
        LogLevel::Warning => "warning",
        LogLevel::Off => "",
    }
}

impl FileLoggerSink {
    /// `flush()`, `async` no original.
    pub fn flush_async(&self) -> Promise<()> {
        self.0.flush()
    }
}

impl MessageSink for FileLoggerSink {
    fn log(&self, level: LogLevel, message: &str) {
        if self.0.donot_use_formatters {
            self.0.buffer.borrow_mut().push_str(message);
        } else {
            self.0.buffer.borrow_mut().push_str(&format!("{} [{}] {message}\n", current_timestamp(), stringify_log_level(level)));
        }
        let inner = self.0.clone();
        let _ = self.0.flush_delayer.trigger(move |_| inner.flush(), None);
    }

    fn flush(&self) {
        let _ = self.0.flush();
    }
}

/// `FileLogger`.
pub type FileLogger = MessageLogger<FileLoggerSink>;

impl FileLogger {
    pub fn new_file(resource: &URI, level: LogLevel, donot_use_formatters: bool, file_service: &Rc<dyn IFileService>) -> Rc<Self> {
        let logger = MessageLogger::new(FileLoggerSink(Rc::new(FileLoggerInner {
            resource: resource.clone(),
            donot_use_formatters,
            file_service: file_service.clone(),
            // buffer saves over a short time
            flush_delayer: ThrottledDelayer::new(100),
            initialize_promise: FileLoggerInner::initialize(file_service, resource),
            backup_index: Cell::new(1),
            buffer: RefCell::new(String::new()),
        })));
        logger.set_level(level);
        logger
    }
}

/// `FileLoggerService`: cria loggers que escrevem pelo `IFileService`, guardando as mensagens até o provider
/// do esquema do recurso existir.
pub fn file_logger_service(log_level: LogLevel, logs_home: URI, file_service: Rc<dyn IFileService>) -> Rc<AbstractLoggerService> {
    AbstractLoggerService::new(
        log_level,
        logs_home,
        Vec::new(),
        Rc::new(move |resource: &URI, log_level: LogLevel, options: &LoggerOptions| -> LoggerRef {
            let logger = BufferLogger::new_buffer(log_level);
            let (target, donot_use_formatters, file_service) = (resource.clone(), options.donot_use_formatters, file_service.clone());
            let buffer = logger.clone();
            when_provider_registered(resource, file_service.as_ref()).then(move |()| {
                let file_logger = FileLogger::new_file(&target, buffer.get_level(), donot_use_formatters, &file_service);
                buffer.set_logger(file_logger);
            });
            logger
        }),
    )
}
