//! Porte de `src/vs/platform/files/common/diskFileSystemProvider.ts`: a parte do provider de disco que não
//! depende de Node, a observação de arquivos. Escolhe o watcher universal (recursivo) ou o não recursivo,
//! agrupa os pedidos de `watch` num `ThrottledDelayer` e repassa mudanças, mensagens de log e erros.
//!
//! O original é uma classe abstrata com dois métodos abstratos (`createUniversalWatcher` e
//! `createNonRecursiveWatcher`) que `DiskFileSystemProvider` implementa. Rust não tem herança: o
//! [`DiskFileSystemProvider`](super::disk::DiskFileSystemProvider) contém um [`AbstractDiskFileSystemProvider`]
//! e as duas fábricas de cliente entram no construtor ([`CreateWatcherClient`]).
//!
//! Divergências: `reviveFileChanges` (reconstrói os `URI` que vieram serializados pelo IPC) não existe,
//! porque o cliente do watcher entrega `FileChange` com `URI` de verdade; e o `watch(requests)` do cliente
//! recebe uma cópia da lista de pedidos no momento do refresh, não a lista viva do original (a lista viva
//! só importa para o reinício do watcher, que o cliente refaz com a cópia que guardou, e um novo refresh
//! sempre manda a lista atualizada).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::ThrottledDelayer;
use je_base::errors::on_unexpected_error;
use je_base::event::{Emitter, Event};
use je_base::extpath::remove_trailing_path_separator;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::path::{Environment, PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::uri::URI;

use super::files::{FileChange, WatchOptions};
use super::watcher::{LogMessage, LogMessageType, RecursiveWatcherOptions, UsePolling, WatchRequest, WatcherClient};
use crate::log::{LogLevel, LogServiceRef};

/// `IDiskFileSystemProviderOptions.watcher`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiskWatcherOptions {
    /// Extra options for the recursive file watching.
    pub recursive: Option<RecursiveWatcherOptions>,
    /// Forces all file watch requests to run through a single universal file watcher, both recursive and
    /// non-recursively.
    ///
    /// Enabling this option might cause some overhead, specifically the universal file watcher will run in
    /// a separate process given its complexity. Only enable it when you understand the consequences.
    pub force_universal: bool,
}

/// `IDiskFileSystemProviderOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiskFileSystemProviderOptions {
    pub watcher: Option<DiskWatcherOptions>,
}

/// `createUniversalWatcher(onChange, onLogMessage, verboseLogging)` e `createNonRecursiveWatcher(...)`: o
/// que o provider concreto injeta para criar o cliente do watcher.
pub type CreateWatcherClient = Rc<dyn Fn(Rc<dyn Fn(Vec<FileChange>)>, Rc<dyn Fn(LogMessage)>, bool) -> Rc<WatcherClient>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WatcherKind {
    Universal,
    NonRecursive,
}

/// O que o original repete para os dois tipos de watcher: a lista de pedidos, o delayer do refresh e o
/// cliente, criado no primeiro refresh.
struct WatcherSlot {
    create: CreateWatcherClient,
    client: RefCell<Option<Rc<WatcherClient>>>,
    /// `insert(array, request)`: o id identifica o pedido na hora de remover.
    requests: RefCell<Vec<(u64, WatchRequest)>>,
    delayer: Rc<ThrottledDelayer<()>>,
}

impl WatcherSlot {
    fn new(create: CreateWatcherClient) -> Self {
        // `new ThrottledDelayer(getRefreshWatchersDelay(requests.length))`: a lista está vazia aqui.
        Self { create, client: RefCell::new(None), requests: RefCell::new(Vec::new()), delayer: Rc::new(ThrottledDelayer::new(refresh_watchers_delay(0))) }
    }
}

/// `getRefreshWatchersDelay(count)`.
fn refresh_watchers_delay(count: usize) -> u64 {
    if count > 200 {
        // If there are many requests to refresh, start to throttle the refresh to reduce pressure. We see
        // potentially thousands of requests coming in on startup repeatedly so we take it easy.
        return 500;
    }

    // By default, use a short delay to keep watchers updating fast but still with a delay so that we can
    // efficiently deduplicate requests or reuse existing watchers.
    0
}

/// `AbstractDiskFileSystemProvider`.
pub struct AbstractDiskFileSystemProvider {
    this: Weak<AbstractDiskFileSystemProvider>,
    log_service: LogServiceRef,
    options: Option<DiskFileSystemProviderOptions>,
    disposables: Rc<DisposableStore>,
    on_did_change_file: Rc<Emitter<Vec<FileChange>>>,
    on_did_watch_error: Rc<Emitter<String>>,
    universal: WatcherSlot,
    non_recursive: WatcherSlot,
    next_request_id: Cell<u64>,
}

impl AbstractDiskFileSystemProvider {
    pub fn new(
        log_service: LogServiceRef,
        options: Option<DiskFileSystemProviderOptions>,
        create_universal_watcher: CreateWatcherClient,
        create_non_recursive_watcher: CreateWatcherClient,
    ) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let universal = WatcherSlot::new(create_universal_watcher);
        let non_recursive = WatcherSlot::new(create_non_recursive_watcher);
        disposables.add_rc(universal.delayer.clone());
        disposables.add_rc(non_recursive.delayer.clone());
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            log_service,
            options,
            on_did_change_file: disposables.add_rc(Emitter::new()),
            on_did_watch_error: disposables.add_rc(Emitter::new()),
            disposables,
            universal,
            non_recursive,
            next_request_id: Cell::new(0),
        })
    }

    pub fn log_service(&self) -> &LogServiceRef {
        &self.log_service
    }

    /// `onDidChangeFile`.
    pub fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        self.on_did_change_file.event()
    }

    /// `onDidWatchError`.
    pub fn on_did_watch_error(&self) -> Event<String> {
        self.on_did_watch_error.event()
    }

    fn slot(&self, kind: WatcherKind) -> &WatcherSlot {
        match kind {
            WatcherKind::Universal => &self.universal,
            WatcherKind::NonRecursive => &self.non_recursive,
        }
    }

    /// `watch(resource, opts)`.
    pub fn watch(&self, resource: &URI, opts: &WatchOptions) -> DisposableRef {
        let force_universal = self.options.as_ref().and_then(|options| options.watcher.as_ref()).is_some_and(|watcher| watcher.force_universal);
        let kind = if opts.recursive || force_universal { WatcherKind::Universal } else { WatcherKind::NonRecursive };
        self.watch_with(kind, resource, opts)
    }

    /// `watchUniversal` e `watchNonRecursive`: os dois só diferem na lista e no watcher.
    fn watch_with(&self, kind: WatcherKind, resource: &URI, opts: &WatchOptions) -> DisposableRef {
        // Add to list of paths to watch
        let id = self.next_request_id.get();
        self.next_request_id.set(id + 1);
        self.slot(kind).requests.borrow_mut().push((id, self.to_watch_request(resource, opts)));

        // Trigger update
        self.refresh_watchers(kind);

        let this = self.this.clone();
        to_disposable(move || {
            let Some(this) = this.upgrade() else { return };

            // Remove from list of paths to watch
            this.slot(kind).requests.borrow_mut().retain(|(request_id, _)| *request_id != id);

            // Trigger update
            this.refresh_watchers(kind);
        })
    }

    /// `toWatchRequest(resource, opts)`: o pedido do watcher, com o polling configurado quando é recursivo.
    fn to_watch_request(&self, resource: &URI, opts: &WatchOptions) -> WatchRequest {
        let mut request = WatchRequest {
            path: self.to_watch_path(resource),
            excludes: opts.excludes.clone(),
            includes: opts.includes.clone(),
            recursive: opts.recursive,
            filter: opts.filter,
            correlation_id: opts.correlation_id,
            polling_interval: None,
        };

        if request.is_recursive() {
            // Adjust for polling
            let recursive_options = self.options.as_ref().and_then(|options| options.watcher.as_ref()).and_then(|watcher| watcher.recursive.as_ref());
            if let Some(recursive_options) = recursive_options {
                let polling = match &recursive_options.use_polling {
                    UsePolling::Bool(true) => true,
                    UsePolling::Bool(false) => false,
                    UsePolling::Paths(paths) => paths.contains(&request.path),
                };
                if polling {
                    request.polling_interval = Some(recursive_options.polling_interval.unwrap_or(5000));
                }
            }
        }

        request
    }

    fn refresh_watchers(&self, kind: WatcherKind) {
        let this = self.this.clone();
        let count = self.slot(kind).requests.borrow().len();
        self.slot(kind)
            .delayer
            .trigger(
                move |_token| match this.upgrade() {
                    Some(this) => this.do_refresh_watchers(kind),
                    None => Promise::resolved(()),
                },
                Some(refresh_watchers_delay(count)),
            )
            .catch(|error| on_unexpected_error(&error));
    }

    fn do_refresh_watchers(&self, kind: WatcherKind) -> Promise<()> {
        let slot = self.slot(kind);

        // Create watcher if this is the first time
        if slot.client.borrow().is_none() {
            let client = self.create_client(kind);
            *slot.client.borrow_mut() = Some(client);
        }

        // Ask to watch the provided paths
        let client = slot.client.borrow().clone().expect("o cliente acabou de ser criado");
        let requests = slot.requests.borrow().iter().map(|(_, request)| request.clone()).collect();
        client.watch(requests)
    }

    fn create_client(&self, kind: WatcherKind) -> Rc<WatcherClient> {
        let (on_change_target, on_log_target) = (self.this.clone(), self.this.clone());
        let on_change: Rc<dyn Fn(Vec<FileChange>)> = Rc::new(move |changes| {
            if let Some(this) = on_change_target.upgrade() {
                this.on_did_change_file.fire(changes);
            }
        });
        let on_log_message: Rc<dyn Fn(LogMessage)> = Rc::new(move |message| {
            if let Some(this) = on_log_target.upgrade() {
                this.on_watcher_log_message(&message);
            }
        });
        let client = (self.slot(kind).create)(on_change, on_log_message, self.log_service.get_level() == LogLevel::Trace);
        self.disposables.add_rc(client.clone());

        // Apply log levels dynamically
        let this = self.this.clone();
        self.disposables.add(self.log_service.on_did_change_log_level().listen(move |_| {
            let Some(this) = this.upgrade() else { return };
            let client = this.slot(kind).client.borrow().clone();
            if let Some(client) = client {
                let _ = client.set_verbose_logging(this.log_service.get_level() == LogLevel::Trace);
            }
        }));
        client
    }

    fn on_watcher_log_message(&self, message: &LogMessage) {
        if message.message_type == LogMessageType::Error {
            self.on_did_watch_error.fire(message.message.clone());
        }

        self.log_watcher_message(message);
    }

    /// `logWatcherMessage(msg)`: `this.logService[msg.type](msg.message)`.
    pub fn log_watcher_message(&self, message: &LogMessage) {
        let text = message.message.as_str();
        match message.message_type {
            LogMessageType::Trace => self.log_service.trace(text),
            LogMessageType::Debug => self.log_service.debug(text),
            LogMessageType::Info => self.log_service.info(text),
            LogMessageType::Warn => self.log_service.warn(text),
            LogMessageType::Error => self.log_service.error(text),
        }
    }

    /// `toFilePath(resource)`.
    pub fn to_file_path(&self, resource: &URI) -> String {
        PathModule::native(&ProcessEnvironment).normalize(&resource.fs_path(ProcessEnvironment.platform()).to_string_lossy())
    }

    fn to_watch_path(&self, resource: &URI) -> String {
        let file_path = self.to_file_path(resource);

        // Ensure to have any trailing path separators removed, otherwise we may believe the path is not
        // "real" and will convert every event back to this form, which is not warranted.
        // See also https://github.com/microsoft/vscode/issues/210517
        remove_trailing_path_separator(&file_path, &ProcessEnvironment)
    }
}

impl IDisposable for AbstractDiskFileSystemProvider {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}
