//! Porte de `src/vs/workbench/api/common/extHostOutput.ts`: o lado da extensão de `window.createOutputChannel`, com o canal de saída
//! ([`ExtHostOutputChannel`], o `ExtHostOutputChannel` e o `ExtHostLogOutputChannel` do original) e o serviço
//! ([`ExtHostOutputService`]) sobre o proxy `MainThreadOutputService` e o `ILoggerService`.
//!
//! Divergências:
//! - O `AbstractMessageLogger` do canal (`this.info(value, ...args)`) é o canal mesmo: `trace`, `debug`, `info`, `warn` e `error`
//!   recebem só a mensagem (os `...args` de formatação não existem: a extensão formata antes), e um canal descartado não escreve.
//! - Os objetos `vscode.OutputChannel` e `vscode.LogOutputChannel` do original são literais que fecham sobre `channelPromise` e
//!   `channelDisposables`; aqui são o [`OutputChannelHandle`], com os mesmos métodos, a mesma validação (`Channel has been closed`) e
//!   a mesma espera pela promessa do canal real.
//! - `IExtHostInitDataService` (`logsLocation` e `environment.extensionLogLevel`) e `IExtHostConsumerFileSystem.value.createDirectory`
//!   (que o host ainda não porta) são os argumentos do construtor: os dois URIs/níveis e o [`IDirectoryCreator`].
//! - `toLocalISOString(new Date())` é a hora em UTC no formato ISO (o porte não carrega o fuso do usuário no host), com a mesma
//!   limpeza de `-`, `:` e da parte decimal.
//! - A extensão chamadora é um [`ExtensionIdentifier`] (a `IExtensionDescription` só é lida por `identifier`).
//! - `columnOrPreserveFocus` de `show` é [`ShowArgument`].
//! - `LogOutputChannel.onDidChangeLogLevel` é um [`Event`]; `error(value: Error | string)` recebe o texto.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::date_to_iso_string;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{to_disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_base::buffer::VSBuffer;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::files::files::{to_file_system_provider_error_code, FileSystemProviderErrorCode};
use je_platform::log::{can_log, log, LogLevel, LogServiceRef, LoggerLogLevel, LoggerOptions, LoggerRef, LoggerService, ResourceOrId};
use je_platform::resources::ext_uri;

use super::ext_host_protocol::{ExtHostOutputServiceShape, IMainContext, MainContext, MainThreadOutputServiceShape};
use crate::services::output::output::OutputChannelUpdateMode;

/// `IExtHostConsumerFileSystem.value.createDirectory(uri)`.
pub trait IDirectoryCreator {
    fn create_directory(&self, uri: &URI) -> Promise<()>;
}

/// `columnOrPreserveFocus?: vscode.ViewColumn | boolean`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShowArgument {
    Column(i32),
    PreserveFocus(bool),
}

/// O que `createOutputChannel(name, options)` recebe em `options`: nada, um `languageId` ou `{ log: true }`.
#[derive(Clone, Debug, PartialEq)]
pub enum OutputChannelOptions {
    None,
    LanguageId(String),
    Log,
}

/// `name.replace(/[\\/:\*\?"<>\|]/g, '')`.
fn sanitize_file_name(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect()
}

/// `toLocalISOString(new Date()).replace(/-|:|\.\d+Z$/g, '')`.
fn output_session_stamp() -> String {
    let iso = date_to_iso_string(runtime::date_now() as f64).unwrap_or_default();
    let without_fraction = match (iso.rfind('.'), iso.ends_with('Z')) {
        (Some(dot), true) if iso[dot + 1..iso.len() - 1].chars().all(|c| c.is_ascii_digit()) => iso[..dot].to_owned(),
        _ => iso,
    };
    without_fraction.chars().filter(|c| *c != '-' && *c != ':').collect()
}

// ---- ExtHostOutputChannel ---------------------------------------------------------------------------------------------

/// `ExtHostOutputChannel` (e, com `is_log`, o `ExtHostLogOutputChannel`, cujo `appendLine` é só `append`).
pub struct ExtHostOutputChannel {
    id: String,
    name: String,
    logger: LoggerRef,
    proxy: Rc<dyn MainThreadOutputServiceShape>,
    extension: ExtensionIdentifier,
    is_log: bool,
    offset: Cell<u64>,
    visible: Cell<bool>,
    level: Rc<Cell<LogLevel>>,
    on_did_change_log_level: Rc<Emitter<LogLevel>>,
    subscriptions: Rc<DisposableStore>,
    disposed: Cell<bool>,
}

impl ExtHostOutputChannel {
    fn new(id: String, name: String, logger: LoggerRef, proxy: Rc<dyn MainThreadOutputServiceShape>, extension: ExtensionIdentifier, is_log: bool) -> Rc<Self> {
        let level = Rc::new(Cell::new(logger.get_level()));
        let on_did_change_log_level = Emitter::new();
        let subscriptions = DisposableStore::new();
        {
            let (level, emitter) = (level.clone(), on_did_change_log_level.clone());
            subscriptions.add(logger.on_did_change_log_level().listen(move |new_level| {
                if level.get() != *new_level {
                    level.set(*new_level);
                    emitter.fire(*new_level);
                }
            }));
        }
        Rc::new(Self { id, name, logger, proxy, extension, is_log, offset: Cell::new(0), visible: Cell::new(false), level, on_did_change_log_level, subscriptions, disposed: Cell::new(false) })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn extension(&self) -> &ExtensionIdentifier {
        &self.extension
    }

    pub fn visible(&self) -> bool {
        self.visible.get()
    }

    pub fn set_visible(&self, visible: bool) {
        self.visible.set(visible);
    }

    /// `logLevel`.
    pub fn log_level(&self) -> LogLevel {
        self.level.get()
    }

    /// `onDidChangeLogLevel`.
    pub fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.on_did_change_log_level.event()
    }

    pub fn append_line(&self, value: &str) {
        if self.is_log {
            self.append(value);
        } else {
            self.append(&format!("{value}\n"));
        }
    }

    pub fn append(&self, value: &str) {
        self.info(value);
    }

    pub fn clear(&self) {
        let till = self.offset.get();
        self.logger.flush();
        let _ = self.proxy.rpc_update(&self.id, OutputChannelUpdateMode::Clear, Some(till));
    }

    pub fn replace(&self, value: &str) {
        let till = self.offset.get();
        self.info(value);
        let _ = self.proxy.rpc_update(&self.id, OutputChannelUpdateMode::Replace, Some(till));
        if self.visible.get() {
            self.logger.flush();
        }
    }

    pub fn show(&self, column_or_preserve_focus: Option<ShowArgument>, preserve_focus: Option<bool>) {
        self.logger.flush();
        let preserve = match column_or_preserve_focus {
            Some(ShowArgument::PreserveFocus(flag)) => flag,
            _ => preserve_focus.unwrap_or(false),
        };
        let _ = self.proxy.rpc_reveal(&self.id, preserve);
    }

    pub fn hide(&self) {
        let _ = self.proxy.rpc_close(&self.id);
    }

    /// `canLog` do `AbstractLogger`: um canal descartado não escreve.
    fn can_log(&self, level: LogLevel) -> bool {
        !self.disposed.get() && can_log(self.level.get(), level)
    }

    pub fn trace(&self, message: &str) {
        self.write(LogLevel::Trace, message);
    }

    pub fn debug(&self, message: &str) {
        self.write(LogLevel::Debug, message);
    }

    pub fn info(&self, message: &str) {
        self.write(LogLevel::Info, message);
    }

    pub fn warn(&self, message: &str) {
        self.write(LogLevel::Warning, message);
    }

    pub fn error(&self, message: &str) {
        self.write(LogLevel::Error, message);
    }

    fn write(&self, level: LogLevel, message: &str) {
        if self.can_log(level) {
            self.log(level, message);
        }
    }

    /// `protected log(level, message)`.
    fn log(&self, level: LogLevel, message: &str) {
        self.offset.set(self.offset.get() + VSBuffer::from_string(message).byte_length() as u64);
        log(&*self.logger, level, message);
        if self.visible.get() {
            self.logger.flush();
            let _ = self.proxy.rpc_update(&self.id, OutputChannelUpdateMode::Append, None);
        }
    }
}

impl IDisposable for ExtHostOutputChannel {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        let _ = self.proxy.rpc_dispose(&self.id);
        self.subscriptions.dispose();
        self.on_did_change_log_level.dispose();
    }
}

// ---- OutputChannelHandle ----------------------------------------------------------------------------------------------

/// O `vscode.OutputChannel` (e, com `log_state`, o `vscode.LogOutputChannel`) que a extensão segura: cada chamada valida o canal e
/// espera a promessa do canal real.
pub struct OutputChannelHandle {
    name: String,
    channel_promise: Promise<Rc<ExtHostOutputChannel>>,
    channel_disposables: Rc<DisposableStore>,
    /// O `logLevel` e o emissor do `LogOutputChannel`.
    log_state: Option<(Rc<Cell<LogLevel>>, Rc<Emitter<LogLevel>>)>,
}

impl OutputChannelHandle {
    fn validate(&self) -> Result<(), Error> {
        if self.channel_disposables.is_disposed() {
            return Err(Error::new("Channel has been closed"));
        }
        Ok(())
    }

    /// Roda `action` quando o canal real existir (`channelPromise.then(channel => ...)`).
    fn with_channel(&self, action: impl FnOnce(&ExtHostOutputChannel) + 'static) -> Result<(), Error> {
        self.validate()?;
        let _ = self.channel_promise.then(move |channel| action(&channel));
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn append(&self, value: &str) -> Result<(), Error> {
        let value = value.to_owned();
        self.with_channel(move |channel| channel.append(&value))
    }

    pub fn append_line(&self, value: &str) -> Result<(), Error> {
        let value = value.to_owned();
        self.with_channel(move |channel| channel.append_line(&value))
    }

    pub fn clear(&self) -> Result<(), Error> {
        self.with_channel(|channel| channel.clear())
    }

    pub fn replace(&self, value: &str) -> Result<(), Error> {
        let value = value.to_owned();
        self.with_channel(move |channel| channel.replace(&value))
    }

    pub fn show(&self, column_or_preserve_focus: Option<ShowArgument>, preserve_focus: Option<bool>) -> Result<(), Error> {
        self.with_channel(move |channel| channel.show(column_or_preserve_focus, preserve_focus))
    }

    pub fn hide(&self) -> Result<(), Error> {
        self.with_channel(|channel| channel.hide())
    }

    pub fn dispose(&self) {
        self.channel_disposables.dispose();
    }

    /// `logLevel` (só no `LogOutputChannel`).
    pub fn log_level(&self) -> Option<LogLevel> {
        self.log_state.as_ref().map(|(level, _)| level.get())
    }

    /// `onDidChangeLogLevel` (só no `LogOutputChannel`).
    pub fn on_did_change_log_level(&self) -> Option<Event<LogLevel>> {
        self.log_state.as_ref().map(|(_, emitter)| emitter.event())
    }

    pub fn trace(&self, value: &str) -> Result<(), Error> {
        let value = value.to_owned();
        self.with_channel(move |channel| channel.trace(&value))
    }

    pub fn debug(&self, value: &str) -> Result<(), Error> {
        let value = value.to_owned();
        self.with_channel(move |channel| channel.debug(&value))
    }

    pub fn info(&self, value: &str) -> Result<(), Error> {
        let value = value.to_owned();
        self.with_channel(move |channel| channel.info(&value))
    }

    pub fn warn(&self, value: &str) -> Result<(), Error> {
        let value = value.to_owned();
        self.with_channel(move |channel| channel.warn(&value))
    }

    pub fn error(&self, value: &str) -> Result<(), Error> {
        let value = value.to_owned();
        self.with_channel(move |channel| channel.error(&value))
    }
}

// ---- ExtHostOutputService ---------------------------------------------------------------------------------------------

/// `ExtHostOutputService`.
pub struct ExtHostOutputService {
    this: Weak<ExtHostOutputService>,
    proxy: Rc<dyn MainThreadOutputServiceShape>,
    logs_location: URI,
    extension_log_levels: Vec<(ExtensionIdentifier, LogLevel)>,
    directory_creator: Rc<dyn IDirectoryCreator>,
    logger_service: Rc<dyn LoggerService>,
    log_service: LogServiceRef,
    outputs_location: URI,
    output_directory_promise: RefCell<Option<Promise<URI>>>,
    extension_log_directory_creation: RefCell<HashMap<String, Promise<()>>>,
    log_output_channels: RefCell<HashMap<String, Rc<OutputChannelHandle>>>,
    name_pool: Cell<u32>,
    channels: RefCell<HashMap<String, Rc<ExtHostOutputChannel>>>,
    visible_channel_id: RefCell<Option<String>>,
}

impl ExtHostOutputService {
    pub fn new(
        ext_host_rpc: &dyn IMainContext,
        logs_location: URI,
        extension_log_levels: Vec<(ExtensionIdentifier, LogLevel)>,
        directory_creator: Rc<dyn IDirectoryCreator>,
        logger_service: Rc<dyn LoggerService>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        let outputs_location = ext_uri().join_path(&logs_location, &[&format!("output_logging_{}", output_session_stamp())]);
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            proxy: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_OUTPUT_SERVICE),
            logs_location,
            extension_log_levels,
            directory_creator,
            logger_service,
            log_service,
            outputs_location,
            output_directory_promise: RefCell::new(None),
            extension_log_directory_creation: RefCell::new(HashMap::new()),
            log_output_channels: RefCell::new(HashMap::new()),
            name_pool: Cell::new(1),
            channels: RefCell::new(HashMap::new()),
            visible_channel_id: RefCell::new(None),
        })
    }

    fn rc(&self) -> Rc<ExtHostOutputService> {
        self.this.upgrade().expect("o ExtHostOutputService está vivo enquanto um método dele roda")
    }

    /// O canal real com `id` (para os testes e para quem precisa do `visible`).
    pub fn channel(&self, id: &str) -> Option<Rc<ExtHostOutputChannel>> {
        self.channels.borrow().get(id).cloned()
    }

    /// `createOutputChannel(name, options, extension)`.
    pub fn create_output_channel(&self, name: &str, options: OutputChannelOptions, extension: &ExtensionIdentifier) -> Result<Rc<OutputChannelHandle>, Error> {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(Error::new("illegal argument `name`. must not be falsy"));
        }
        let language_id = match &options {
            OutputChannelOptions::LanguageId(language_id) => {
                if language_id.trim().is_empty() {
                    return Err(Error::new("illegal argument `languageId`. must not be empty"));
                }
                Some(language_id.clone())
            }
            _ => None,
        };
        let is_log = options == OutputChannelOptions::Log;

        let channel_disposables = DisposableStore::new();
        let mut log_level = self.extension_log_levels.iter().find(|(identifier, _)| identifier.value.eq_ignore_ascii_case(&extension.value)).map(|(_, level)| *level);
        let mut log_file: Option<URI> = None;
        let channel_promise: Promise<Rc<ExtHostOutputChannel>>;
        if is_log {
            let extension_log_directory = ext_uri().join_path(&self.logs_location, &[extension.value.as_str()]);
            let file = ext_uri().join_path(&extension_log_directory, &[format!("{}.log", sanitize_file_name(&name)).as_str()]);
            let key = file.to_string(false).to_string_lossy();
            if let Some(existing) = self.log_output_channels.borrow().get(&key) {
                return Ok(existing.clone());
            }
            // Only override the extension-specific default log level if the user has explicitly configured a level for this logger.
            if let Some(registered) = self.logger_service.get_registered_logger(&file)
                && let Some(registered_level) = registered.log_level
            {
                log_level = Some(registered_level);
            }
            channel_promise = self.rc().do_create_log_output_channel(name.clone(), file.clone(), log_level, extension.clone(), channel_disposables.clone());
            log_file = Some(file);
        } else {
            channel_promise = self.rc().do_create_output_channel(name.clone(), language_id, extension.clone(), channel_disposables.clone());
        }

        {
            let (this, disposables, log_file) = (self.this.clone(), channel_disposables.clone(), log_file.clone());
            let _ = channel_promise.then(move |channel| {
                let Some(this) = this.upgrade() else { return };
                this.channels.borrow_mut().insert(channel.id().to_owned(), channel.clone());
                channel.set_visible(this.visible_channel_id.borrow().as_deref() == Some(channel.id()));
                let (cleanup_this, id) = (Rc::downgrade(&this), channel.id().to_owned());
                disposables.add(to_disposable(move || {
                    if let Some(this) = cleanup_this.upgrade() {
                        this.channels.borrow_mut().remove(&id);
                        if let Some(log_file) = &log_file {
                            this.log_output_channels.borrow_mut().remove(&log_file.to_string(false).to_string_lossy());
                        }
                    }
                }));
            });
        }

        let initial_level = log_level.unwrap_or_else(|| self.log_service.get_level());
        let handle = self.create_handle(&name, channel_promise, channel_disposables, is_log.then_some(initial_level));
        if let Some(log_file) = log_file {
            self.log_output_channels.borrow_mut().insert(log_file.to_string(false).to_string_lossy(), handle.clone());
        }
        Ok(handle)
    }

    fn output_directory(&self) -> Promise<URI> {
        if let Some(promise) = self.output_directory_promise.borrow().clone() {
            return promise;
        }
        let location = self.outputs_location.clone();
        let promise = self.directory_creator.create_directory(&location).then(move |()| location.clone());
        *self.output_directory_promise.borrow_mut() = Some(promise.clone());
        promise
    }

    fn do_create_output_channel(self: Rc<Self>, name: String, language_id: Option<String>, extension: ExtensionIdentifier, channel_disposables: Rc<DisposableStore>) -> Promise<Rc<ExtHostOutputChannel>> {
        async_fn(async move {
            let output_dir = self.output_directory().await?;
            let number = self.name_pool.get();
            self.name_pool.set(number + 1);
            let file = ext_uri().join_path(&output_dir, &[format!("{number}-{}.log", sanitize_file_name(&name)).as_str()]);
            let logger = self.logger_service.create_logger(
                ResourceOrId::Resource(file.clone()),
                Some(LoggerOptions {
                    log_level: Some(LoggerLogLevel::Always),
                    donot_rotate: true,
                    donot_use_formatters: true,
                    hidden: Some(true),
                    ..LoggerOptions::default()
                }),
            );
            let logger_disposable: DisposableRef = logger.clone();
            channel_disposables.add(logger_disposable);
            let id = self.proxy.rpc_register(&name, file.clone(), language_id, &extension.value).await?;
            let logger_service = self.logger_service.clone();
            channel_disposables.add(to_disposable(move || logger_service.deregister_logger(&ResourceOrId::Resource(file.clone()))));
            Ok(ExtHostOutputChannel::new(id, name, logger, self.proxy.clone(), extension, false))
        })
    }

    fn do_create_log_output_channel(
        self: Rc<Self>,
        name: String,
        file: URI,
        log_level: Option<LogLevel>,
        extension: ExtensionIdentifier,
        channel_disposables: Rc<DisposableStore>,
    ) -> Promise<Rc<ExtHostOutputChannel>> {
        async_fn(async move {
            self.create_extension_log_directory(&file).await?;
            let id = format!("{}.{}", extension.value, ext_uri().basename(&file, None));
            let logger = self.logger_service.create_logger(
                ResourceOrId::Resource(file.clone()),
                Some(LoggerOptions {
                    id: Some(id.clone()),
                    name: Some(name.clone()),
                    log_level: log_level.map(LoggerLogLevel::Level),
                    extension_id: Some(extension.value.clone()),
                    ..LoggerOptions::default()
                }),
            );
            let logger_disposable: DisposableRef = logger.clone();
            channel_disposables.add(logger_disposable);
            let logger_service = self.logger_service.clone();
            channel_disposables.add(to_disposable(move || logger_service.deregister_logger(&ResourceOrId::Resource(file.clone()))));
            Ok(ExtHostOutputChannel::new(id, name, logger, self.proxy.clone(), extension, true))
        })
    }

    fn create_extension_log_directory(&self, file: &URI) -> Promise<()> {
        let extension_log_directory = ext_uri().dirname(file);
        let key = extension_log_directory.to_string(false).to_string_lossy();
        if let Some(promise) = self.extension_log_directory_creation.borrow().get(&key) {
            return promise.clone();
        }
        let promise = self.directory_creator.create_directory(&extension_log_directory).then_result(|outcome| match outcome {
            Err(error) if to_file_system_provider_error_code(Some(&error)) == FileSystemProviderErrorCode::FileExists => Ok(()),
            other => other,
        });
        self.extension_log_directory_creation.borrow_mut().insert(key, promise.clone());
        promise
    }

    /// `createExtHostOutputChannel` / `createExtHostLogOutputChannel`: o objeto que a extensão segura.
    fn create_handle(&self, name: &str, channel_promise: Promise<Rc<ExtHostOutputChannel>>, channel_disposables: Rc<DisposableStore>, log_level: Option<LogLevel>) -> Rc<OutputChannelHandle> {
        {
            let disposables = channel_disposables.clone();
            let _ = channel_promise.then(move |channel| {
                let to_dispose: DisposableRef = channel;
                disposables.add(to_dispose);
            });
        }
        let log_state = log_level.map(|initial| {
            let level = Rc::new(Cell::new(initial));
            let emitter = Emitter::new();
            channel_disposables.add(emitter.clone());
            {
                let (level, emitter, disposables) = (level.clone(), emitter.clone(), channel_disposables.clone());
                let _ = channel_promise.then(move |channel| {
                    if channel.log_level() != level.get() {
                        level.set(channel.log_level());
                        emitter.fire(channel.log_level());
                    }
                    let (level, emitter) = (level.clone(), emitter.clone());
                    disposables.add(channel.on_did_change_log_level().listen(move |new_level| {
                        level.set(*new_level);
                        emitter.fire(*new_level);
                    }));
                });
            }
            (level, emitter)
        });
        Rc::new(OutputChannelHandle { name: name.to_owned(), channel_promise, channel_disposables, log_state })
    }
}

impl ExtHostOutputServiceShape for ExtHostOutputService {
    fn rpc_set_visible_channel(&self, visible_channel_id: Option<String>) {
        *self.visible_channel_id.borrow_mut() = visible_channel_id.clone();
        for (id, channel) in self.channels.borrow().iter() {
            channel.set_visible(Some(id.as_str()) == visible_channel_id.as_deref());
        }
    }
}
