//! Porte de `src/vs/platform/storage/electron-main/storageMain.ts`: o dono das conexões de armazenamento
//! (`IStorageMain`), com as variantes de aplicação, aplicação compartilhada, perfil, workspace e em memória.
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; por isso o dono das
//! conexões é o mesmo, e o cliente ([`SqliteStorageService`](super::SqliteStorageService)) o chama direto, sem o IPC.
//!
//! Decisões de tradução:
//! - **Herança**: `BaseStorageMain` e as quatro subclasses (`ProfileStorageMain`, `ApplicationStorageMain`,
//!   `ApplicationSharedStorageMain`, `WorkspaceStorageMain`, mais a `InMemoryStorageMain`) viram uma struct só,
//!   [`BaseStorageMain`], que recebe os três membros que as subclasses definem (`path`, `doCreate` e `doInit`) como um
//!   `StorageMainKind`. `ProfileStorageMain` e as outras são estruturas sem campos cujo `new` devolve o
//!   [`BaseStorageMain`] montado com a variante certa (o `new` das classes do original).
//! - **`BaseProfileAwareStorageMain`** é a variante que o `ProfileStorageMain` e o `ApplicationStorageMain` dividem; o
//!   `ApplicationStorageMain` a liga ao `doInit` que grava as datas de sessão do telemetry.
//! - **`Storage` e `MigratingStorage`**: o `_storage` do original é um `Storage` (e o `MigratingStorage` o estende). Sem
//!   herança, o campo é um `Rc<dyn IStorage>`, e o `isInMemory()` do `Storage` entrou no trait `IStorage` do
//!   `je-storage` (o `MigratingStorage` o implementa sobrescrevendo `init` e `get`).
//! - **`instanceof ApplicationSharedStorageMain`** do `StorageDatabaseChannel` é [`IStorageMain::application_storage_items`]
//!   (`None` fora do armazém compartilhado).
//! - **`IStorageChangeEvent`** de `storageMain.ts` só tem a `key`, e é o [`StorageMainChangeEvent`]: o `BaseStorageMain`
//!   repassa o evento do `Storage` sem o `external`, como o original. O `external` pertence ao `IStorageChangeEvent` de
//!   `base/parts/storage` (o do `Storage` do cliente, que o `RemoteStorageService` lê). No processo único o cliente e o
//!   main dividem o mesmo `Storage`, então o evento dele, com o `external`, sai por
//!   [`IStorageMain::on_did_change_client_storage`], que só o cliente ([`SqliteStorageService`](super::SqliteStorageService))
//!   escuta; o `onDidChangeStorage` do main (o do canal de IPC e do serviço) é o do original.
//! - **`IUserDataProfile`**: só o `globalStorageHome` é lido, então o `ProfileStorageMain` o recebe do perfil.
//! - **`IEnvironmentService`**: o `WorkspaceStorageMain` só lê o `workspaceStorageHome`, lido na construção.
//! - **`ensureWorkspaceStorageFolderMeta`** roda sem `await` (como o original): a promessa é descartada, e o erro vai
//!   para o log. Como no original, o `workspace.json` é gravado quando a pasta do armazém é criada (a primeira abertura
//!   daquele `id`), com a origem do workspace completa; por isso toda abertura precisa receber o identificador inteiro
//!   (o `switch` do cliente o recebe em [`StorageWorkspace`](super::StorageWorkspace)), nunca só o `id`.
//! - **`close()`**: quem fecha os bancos é o dono (este `BaseStorageMain`), por `close()` direto ou pelo `onWillShutdown`
//!   do [`StorageMainService`](super::StorageMainService); o cliente só fecha o que é exclusivamente dele (o armazém do
//!   workspace), e o `onDidCloseStorage` tira o armazém do mapa do serviço, então o `onWillShutdown` não o fecha de novo.
//! - **`new Date().toUTCString()`** é o `Timestamp` do `jiff` no formato `%a, %d %b %Y %H:%M:%S GMT`, sobre o relógio do
//!   laço de eventos (`date_now`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::arrays::top;
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, json_stringify_indented};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::network::schemas;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_base::stopwatch::StopWatch;
use je_base::uri::{Field, URI, UriChange};
use je_storage::{
    IStorage, InMemoryStorageDatabase, ItemMap, LogFn, MigratingStorage, SqliteStorageDatabase, SqliteStorageDatabaseLoggingOptions, SqliteStorageDatabaseOptions, Storage, StorageChangeEvent,
    StorageHint, StorageOptions, StorageState, StorageValue,
};

use super::storage::IS_NEW_KEY;
use crate::environment::environment::EnvironmentService;
use crate::files::i_file_service::IFileService;
use crate::files::pfs;
use crate::log::{LogArg, LogLevel, LogMessage, LogServiceRef, Logger};
use crate::resources::{fs_path, uri_file};
use crate::telemetry::{CURRENT_SESSION_DATE_STORAGE_KEY, FIRST_SESSION_DATE_STORAGE_KEY, LAST_SESSION_DATE_STORAGE_KEY};
use crate::user_data_profile::{IUserDataProfilesService, UserDataProfile, UserDataProfilesMainService};
use crate::workspace::AnyWorkspaceIdentifier;

/// `LOG_SLOW_CLOSE_THRESHOLD`.
const LOG_SLOW_CLOSE_THRESHOLD: f64 = 2000.0;

/// O nome do arquivo do banco, o mesmo nas quatro variantes (`STORAGE_NAME` e `WORKSPACE_STORAGE_NAME`).
const STORAGE_NAME: &str = "state.vscdb";

const WORKSPACE_META_NAME: &str = "workspace.json";

/// `IStorageMainOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct StorageMainOptions {
    /// If enabled, storage will not persist to disk but into memory.
    pub use_in_memory_storage: bool,
}

/// `IStorageChangeEvent` de `storageMain.ts`: só a chave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageMainChangeEvent {
    pub key: String,
}

/// `IStorageMain`: acesso aos armazéns de aplicação, perfil e workspace, a partir do dono de todas as conexões.
pub trait IStorageMain: IDisposable {
    /// `onDidChangeStorage`: emitido sempre que um dado é atualizado ou removido.
    fn on_did_change_storage(&self) -> Event<StorageMainChangeEvent>;

    /// O `onDidChangeStorage` do `Storage` do cliente (`base/parts/storage`), com o `external`. No original ele nasce no
    /// `Storage` do renderer; aqui o cliente usa o `Storage` do próprio main, então o evento sai daqui.
    fn on_did_change_client_storage(&self) -> Event<StorageChangeEvent>;

    /// `onDidCloseStorage`: emitido quando o armazém é fechado.
    fn on_did_close_storage(&self) -> Event<()>;

    /// `items`: o cache de todos os itens do armazém.
    fn items(&self) -> ItemMap;

    /// `whenInit`: permite esperar o `init` terminar para usar o armazém com segurança.
    fn when_init(&self) -> Promise<()>;

    /// `storage`: o `IStorage`, que fica em memória enquanto o armazém não foi iniciado.
    fn storage(&self) -> Rc<dyn IStorage>;

    /// `path`: o caminho do arquivo do banco, se houver.
    fn path(&self) -> Option<String>;

    /// `init()`: chamada obrigatória para o serviço poder ser usado.
    fn init(&self) -> Promise<()>;

    /// `get(key, fallbackValue?)`.
    fn get(&self, key: &str, fallback_value: Option<&str>) -> Option<String>;

    /// `set(key, value)`: o valor é convertido para texto.
    fn set(&self, key: &str, value: StorageValue) -> Promise<()>;

    /// `delete(key)`.
    fn delete(&self, key: &str) -> Promise<()>;

    /// `isInMemory()`: se o armazém usa persistência em memória.
    fn is_in_memory(&self) -> bool;

    /// `optimize()`: tenta reduzir o tamanho do banco com comandos de otimização, se houver suporte.
    fn optimize(&self) -> Promise<()>;

    /// `close()`: fecha a conexão do armazém.
    fn close(&self) -> Promise<()>;

    /// `storage instanceof ApplicationSharedStorageMain ? storage.applicationStorageItems : undefined`: os itens do
    /// armazém da aplicação que o armazém compartilhado usa como reserva.
    fn application_storage_items(&self) -> Option<ItemMap> {
        None
    }
}

/// O que as subclasses de `BaseStorageMain` definem: `path`, `doCreate` e `doInit`.
trait StorageMainKind {
    fn path(&self) -> Option<String>;

    /// `doCreate()`.
    fn do_create(&self) -> Promise<Rc<dyn IStorage>>;

    /// `doInit(storage)`.
    fn do_init(&self, storage: Rc<dyn IStorage>) -> Promise<()> {
        storage.init()
    }

    /// `ApplicationSharedStorageMain.applicationStorageItems`.
    fn application_storage_items(&self) -> Option<ItemMap> {
        None
    }
}

/// `new Date().toUTCString()`.
fn now_utc_string() -> String {
    let milliseconds = i64::try_from(runtime::date_now()).expect("a data cabe em milissegundos de 64 bits");
    jiff::Timestamp::from_millisecond(milliseconds).expect("a data está no intervalo do Timestamp").strftime("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `uri.with({ scheme: Schemas.file }).fsPath`.
pub(super) fn file_fs_path(uri: &URI) -> String {
    fs_path(&uri.with(&UriChange { scheme: Field::Set(schemas::FILE.into()), ..Default::default() }))
}

/// `join(...)` do módulo `path` da plataforma.
pub(super) fn join(segments: &[&str]) -> String {
    PathModule::native(&ProcessEnvironment).join(segments)
}

/// `createLoggingOptions()`.
fn create_logging_options(log_service: &LogServiceRef) -> SqliteStorageDatabaseLoggingOptions {
    let log_trace: Option<LogFn> = (log_service.get_level() == LogLevel::Trace).then(|| {
        let log_service = log_service.clone();
        Rc::new(move |message: &str| log_service.trace(message)) as LogFn
    });
    let log_service = log_service.clone();
    SqliteStorageDatabaseLoggingOptions { log_error: Some(Rc::new(move |message: &str| log_service.error(message))), log_trace }
}

/// `new SQLiteStorageDatabase(path, { logging: this.createLoggingOptions() })`.
fn open_database(path: &str, log_service: &LogServiceRef) -> Rc<SqliteStorageDatabase> {
    SqliteStorageDatabase::new(path, SqliteStorageDatabaseOptions { logging: Some(create_logging_options(log_service)), ..Default::default() })
}

/// `BaseStorageMain`.
pub struct BaseStorageMain {
    me: Weak<BaseStorageMain>,
    base: Rc<Disposable>,
    kind: Rc<dyn StorageMainKind>,
    log_service: LogServiceRef,
    file_service: Rc<dyn IFileService>,
    on_did_change_storage: Rc<Emitter<StorageMainChangeEvent>>,
    on_did_change_client_storage: Rc<Emitter<StorageChangeEvent>>,
    on_did_close_storage: Rc<Emitter<()>>,
    /// `_storage`: em memória até o `init` criar o de verdade.
    storage: RefCell<Rc<dyn IStorage>>,
    initialize_promise: RefCell<Option<Promise<()>>>,
    when_init: DeferredPromise<()>,
    state: Cell<StorageState>,
}

impl BaseStorageMain {
    fn new(kind: Rc<dyn StorageMainKind>, log_service: LogServiceRef, file_service: Rc<dyn IFileService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_storage = Emitter::new();
        base.register(on_did_change_storage.clone());
        let on_did_change_client_storage = Emitter::new();
        base.register(on_did_change_client_storage.clone());
        let on_did_close_storage = Emitter::new();
        base.register(on_did_close_storage.clone());
        // storage is in-memory until initialized
        let storage: Rc<dyn IStorage> = Storage::new(InMemoryStorageDatabase::new(), StorageOptions { hint: Some(StorageHint::StorageInMemory) });
        base.register(storage.clone());

        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            kind,
            log_service,
            file_service,
            on_did_change_storage,
            on_did_change_client_storage,
            on_did_close_storage,
            storage: RefCell::new(storage),
            initialize_promise: RefCell::new(None),
            when_init: DeferredPromise::new(),
            state: Cell::new(StorageState::None),
        })
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o armazém está vivo enquanto um método dele roda")
    }

    /// O corpo do `try` do `init`: cria o armazém pela variante, troca o em memória pelo de verdade e o inicia.
    async fn create_and_init(&self) -> Result<(), Error> {
        // Create storage via subclasses
        let storage = self.kind.do_create().await?;
        let registered: DisposableRef = storage.clone();
        self.base.register(registered);

        // Replace our in-memory storage with the real once as soon as possible without awaiting the init call.
        let in_memory = std::mem::replace(&mut *self.storage.borrow_mut(), storage.clone());
        in_memory.dispose();

        // Re-emit storage changes via event
        let (on_did_change_storage, on_did_change_client_storage) = (self.on_did_change_storage.clone(), self.on_did_change_client_storage.clone());
        self.base.register(storage.on_did_change_storage().listen(move |e| {
            on_did_change_storage.fire(StorageMainChangeEvent { key: e.key.clone() });
            on_did_change_client_storage.fire(e.clone());
        }));

        // Await storage init
        self.kind.do_init(storage.clone()).await?;

        // Ensure we track whether storage is new or not
        match storage.get_boolean(IS_NEW_KEY, None) {
            None => {
                let _ = storage.set(IS_NEW_KEY, StorageValue::Bool(true), false);
            }
            Some(true) => {
                let _ = storage.set(IS_NEW_KEY, StorageValue::Bool(false), false);
            }
            Some(false) => {}
        }
        Ok(())
    }

    fn do_close(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            // Ensure we are not accidentally leaving a pending initialized storage behind in case `close()` was called
            // before `init()` finishes.
            let initialize_promise = this.initialize_promise.borrow().clone();
            if let Some(initialize_promise) = initialize_promise {
                initialize_promise.await?;
            }

            // Update state
            this.state.set(StorageState::Closed);

            // Propagate to storage lib
            let storage = this.storage.borrow().clone();
            storage.close().await
        })
    }

    /// `logSlowClose(watch)`: se o `close()` demorou, há chance de o banco ser grande (no disco ou em geral), e então
    /// registra mais informação para diagnosticar.
    fn log_slow_close(&self, elapsed: f64) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            let Some(path) = this.kind.path() else {
                return Ok(());
            };

            let stats: Result<(String, u64), Error> = async {
                let entries: Vec<(String, usize)> = IStorageMain::items(this.as_ref()).snapshot().iter().map(|(key, value)| (key.clone(), value.encode_utf16().count())).collect();
                let largest_entries = top(&entries, |entry_a, entry_b| entry_b.1 as f64 - entry_a.1 as f64, 5).iter().map(|(key, length)| format!("{key}:{length}")).collect::<Vec<_>>().join(", ");
                let db_size = this.file_service.stat(&uri_file(&path)).await?.size();
                Ok((largest_entries, db_size))
            }
            .await;

            match stats {
                Ok((largest_entries, db_size)) => this.log_service.warn(&format!("[storage main] detected slow close() operation: Time: {elapsed}ms, DB size: {db_size}b, Large Keys: {largest_entries}")),
                Err(error) => this.log_service.error_args(&LogMessage::from("[storage main] figuring out stats for slow DB on close() resulted in an error"), &[LogArg::Error(error)]),
            }
            Ok(())
        })
    }
}

impl IDisposable for BaseStorageMain {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IStorageMain for BaseStorageMain {
    fn on_did_change_storage(&self) -> Event<StorageMainChangeEvent> {
        self.on_did_change_storage.event()
    }

    fn on_did_change_client_storage(&self) -> Event<StorageChangeEvent> {
        self.on_did_change_client_storage.event()
    }

    fn on_did_close_storage(&self) -> Event<()> {
        self.on_did_close_storage.event()
    }

    fn items(&self) -> ItemMap {
        self.storage().items()
    }

    fn when_init(&self) -> Promise<()> {
        self.when_init.promise.clone()
    }

    fn storage(&self) -> Rc<dyn IStorage> {
        self.storage.borrow().clone()
    }

    fn path(&self) -> Option<String> {
        self.kind.path()
    }

    fn init(&self) -> Promise<()> {
        if self.initialize_promise.borrow().is_none() {
            let this = self.this();
            let promise = async_fn(async move {
                if this.state.get() != StorageState::None {
                    return Ok(()); // either closed or already initialized
                }

                if let Err(error) = this.create_and_init().await {
                    this.log_service.error(&format!("[storage main] initialize(): Unable to init storage due to {error}"));
                }

                // Update state
                this.state.set(StorageState::Initialized);

                // Mark init promise as completed
                let _ = this.when_init.complete(());
                Ok(())
            });
            *self.initialize_promise.borrow_mut() = Some(promise);
        }

        self.initialize_promise.borrow().clone().expect("definida acima")
    }

    fn get(&self, key: &str, fallback_value: Option<&str>) -> Option<String> {
        self.storage().get(key, fallback_value)
    }

    fn set(&self, key: &str, value: StorageValue) -> Promise<()> {
        self.storage().set(key, value, false)
    }

    fn delete(&self, key: &str) -> Promise<()> {
        self.storage().delete(key, false)
    }

    fn is_in_memory(&self) -> bool {
        self.storage().is_in_memory()
    }

    fn optimize(&self) -> Promise<()> {
        self.storage().optimize()
    }

    fn close(&self) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            // Measure how long it takes to close storage
            let mut watch = StopWatch::new(Some(false));
            this.do_close().await?;
            watch.stop();

            // If close() is taking a long time, there is a chance that the underlying DB is large either on disk or in
            // general. In that case log some additional info to further diagnose
            if watch.elapsed() > LOG_SLOW_CLOSE_THRESHOLD {
                this.log_slow_close(watch.elapsed()).await?;
            }

            // Signal as event
            this.on_did_close_storage.fire(());
            Ok(())
        })
    }

    fn application_storage_items(&self) -> Option<ItemMap> {
        self.kind.application_storage_items()
    }
}

/// `BaseProfileAwareStorageMain`, com o `doInit` do `ApplicationStorageMain` (`with_telemetry_state`).
struct ProfileAwareKind {
    global_storage_home: URI,
    options: StorageMainOptions,
    log_service: LogServiceRef,
    with_telemetry_state: bool,
}

impl StorageMainKind for ProfileAwareKind {
    fn path(&self) -> Option<String> {
        (!self.options.use_in_memory_storage).then(|| join(&[&file_fs_path(&self.global_storage_home), STORAGE_NAME]))
    }

    fn do_create(&self) -> Promise<Rc<dyn IStorage>> {
        let path = self.path();
        let database = open_database(path.as_deref().unwrap_or(SqliteStorageDatabase::IN_MEMORY_PATH), &self.log_service);
        let storage: Rc<dyn IStorage> = Storage::new(database, StorageOptions { hint: path.is_none().then_some(StorageHint::StorageInMemory) });
        async_fn(async move { Ok(storage) })
    }

    fn do_init(&self, storage: Rc<dyn IStorage>) -> Promise<()> {
        let with_telemetry_state = self.with_telemetry_state;
        async_fn(async move {
            storage.init().await?;

            // Apply telemetry values as part of the application storage initialization
            if with_telemetry_state {
                update_telemetry_state(storage.as_ref());
            }
            Ok(())
        })
    }
}

/// `ApplicationStorageMain.updateTelemetryState(storage)`.
fn update_telemetry_state(storage: &dyn IStorage) {
    // First session date (once)
    if storage.get(FIRST_SESSION_DATE_STORAGE_KEY, None).is_none() {
        let _ = storage.set(FIRST_SESSION_DATE_STORAGE_KEY, StorageValue::String(now_utc_string()), false);
    }

    // Last / current session (always)
    // previous session date was the "current" one at that time
    // current session date is "now"
    let last_session_date = storage.get(CURRENT_SESSION_DATE_STORAGE_KEY, None);
    let current_session_date = now_utc_string();
    let _ = storage.set(LAST_SESSION_DATE_STORAGE_KEY, last_session_date.map_or(StorageValue::Null, StorageValue::String), false);
    let _ = storage.set(CURRENT_SESSION_DATE_STORAGE_KEY, StorageValue::String(current_session_date), false);
}

/// `ProfileStorageMain`.
pub struct ProfileStorageMain;

impl ProfileStorageMain {
    pub fn new(profile: &UserDataProfile, options: StorageMainOptions, log_service: LogServiceRef, file_service: Rc<dyn IFileService>) -> Rc<BaseStorageMain> {
        let kind = ProfileAwareKind { global_storage_home: profile.global_storage_home.clone(), options, log_service: log_service.clone(), with_telemetry_state: false };
        BaseStorageMain::new(Rc::new(kind), log_service, file_service)
    }
}

/// `ApplicationStorageMain`: o armazém do perfil padrão.
pub struct ApplicationStorageMain;

impl ApplicationStorageMain {
    pub fn new(options: StorageMainOptions, user_data_profile_service: &UserDataProfilesMainService, log_service: LogServiceRef, file_service: Rc<dyn IFileService>) -> Rc<BaseStorageMain> {
        let kind = ProfileAwareKind { global_storage_home: user_data_profile_service.default_profile().global_storage_home, options, log_service: log_service.clone(), with_telemetry_state: true };
        BaseStorageMain::new(Rc::new(kind), log_service, file_service)
    }
}

/// `ApplicationSharedStorageMain`.
struct ApplicationSharedKind {
    options: StorageMainOptions,
    storage_folder_path: String,
    application_storage: Rc<dyn IStorageMain>,
    log_service: LogServiceRef,
}

impl ApplicationSharedKind {
    /// `prepareStorageFolder()`.
    fn prepare_storage_folder(&self) -> Promise<(String, bool)> {
        let (use_in_memory_storage, storage_folder_path) = (self.options.use_in_memory_storage, self.storage_folder_path.clone());
        async_fn(async move {
            if use_in_memory_storage {
                return Ok((SqliteStorageDatabase::IN_MEMORY_PATH.to_string(), true));
            }

            let storage_database_path = join(&[&storage_folder_path, STORAGE_NAME]);

            if pfs::exists(&storage_folder_path) {
                return Ok((storage_database_path, false));
            }

            pfs::mkdir_recursive(&storage_folder_path, None)?;

            Ok((storage_database_path, true))
        })
    }
}

impl StorageMainKind for ApplicationSharedKind {
    fn path(&self) -> Option<String> {
        (!self.options.use_in_memory_storage).then(|| join(&[&self.storage_folder_path, STORAGE_NAME]))
    }

    fn do_create(&self) -> Promise<Rc<dyn IStorage>> {
        let prepared = self.prepare_storage_folder();
        let (log_service, application_storage) = (self.log_service.clone(), self.application_storage.clone());
        async_fn(async move {
            let (storage_file_path, was_created) = prepared.await?;

            log_service.info(&format!("[shared storage] Creating shared storage database at '{storage_file_path}' (wasCreated: {was_created})"));

            let database = open_database(&storage_file_path, &log_service);

            log_service.info(&format!("[shared storage] Initializing fallback application storage (path: {})", application_storage.path().as_deref().unwrap_or("in-memory")));
            application_storage.init().await?;
            log_service.info(&format!("[shared storage] Fallback application storage initialized with {} items", application_storage.items().len()));

            let migrating_storage = MigratingStorage::new(database, StorageOptions { hint: was_created.then_some(StorageHint::StorageDoesNotExist) });
            migrating_storage.set_fallback_storage(application_storage.storage(), false);
            let storage: Rc<dyn IStorage> = migrating_storage;
            Ok(storage)
        })
    }

    fn application_storage_items(&self) -> Option<ItemMap> {
        Some(self.application_storage.items())
    }
}

/// `ApplicationSharedStorageMain`: o armazém compartilhado entre todos os perfis e janelas, que usa o da aplicação como
/// reserva para migrar as chaves de escopo `APPLICATION` para `APPLICATION_SHARED`.
pub struct ApplicationSharedStorageMain;

impl ApplicationSharedStorageMain {
    pub fn new(options: StorageMainOptions, storage_folder_path: String, application_storage: Rc<dyn IStorageMain>, log_service: LogServiceRef, file_service: Rc<dyn IFileService>) -> Rc<BaseStorageMain> {
        let kind = ApplicationSharedKind { options, storage_folder_path, application_storage, log_service: log_service.clone() };
        BaseStorageMain::new(Rc::new(kind), log_service, file_service)
    }
}

/// `WorkspaceStorageMain`.
struct WorkspaceKind {
    workspace: AnyWorkspaceIdentifier,
    options: StorageMainOptions,
    log_service: LogServiceRef,
    workspace_storage_home: URI,
}

impl WorkspaceKind {
    /// `join(workspaceStorageHome, workspace.id)`: a pasta do armazém do workspace.
    fn folder_path(&self) -> String {
        join(&[&file_fs_path(&self.workspace_storage_home), self.workspace.id()])
    }

    /// `prepareWorkspaceStorageFolder()`.
    fn prepare_workspace_storage_folder(&self) -> Promise<(String, bool)> {
        let use_in_memory_storage = self.options.use_in_memory_storage;
        let workspace_storage_folder_path = self.folder_path();
        let (log_service, workspace) = (self.log_service.clone(), self.workspace.clone());
        async_fn(async move {
            // Return early if using inMemory storage
            if use_in_memory_storage {
                return Ok((SqliteStorageDatabase::IN_MEMORY_PATH.to_string(), true));
            }

            // Otherwise, ensure the storage folder exists on disk
            let workspace_storage_database_path = join(&[&workspace_storage_folder_path, STORAGE_NAME]);

            if pfs::exists(&workspace_storage_folder_path) {
                return Ok((workspace_storage_database_path, false));
            }

            // Ensure storage folder exists
            pfs::mkdir_recursive(&workspace_storage_folder_path, None)?;

            // Write metadata into folder (but do not await)
            drop(ensure_workspace_storage_folder_meta(log_service, workspace, workspace_storage_folder_path));

            Ok((workspace_storage_database_path, true))
        })
    }
}

/// `ensureWorkspaceStorageFolderMeta(workspaceStorageFolderPath)`: grava o `workspace.json` com a origem do workspace
/// (a pasta ou o arquivo de workspace); a janela vazia não tem metadados.
fn ensure_workspace_storage_folder_meta(log_service: LogServiceRef, workspace: AnyWorkspaceIdentifier, workspace_storage_folder_path: String) -> Promise<()> {
    async_fn(async move {
        let meta = match &workspace {
            AnyWorkspaceIdentifier::SingleFolder(folder) => Some(("folder", uri_text(&folder.uri))),
            AnyWorkspaceIdentifier::Workspace(workspace) => Some(("workspace", uri_text(&workspace.config_path))),
            AnyWorkspaceIdentifier::Empty(_) => None,
        };

        if let Some((key, value)) = meta {
            let written: Result<(), Error> = async {
                let workspace_storage_meta_path = join(&[&workspace_storage_folder_path, WORKSPACE_META_NAME]);
                if !pfs::exists(&workspace_storage_meta_path) {
                    let meta = Dynamic::object([(key, Dynamic::string(&value))]);
                    pfs::write_file(&workspace_storage_meta_path, json_stringify_indented(&meta, 2).unwrap_or_default(), None).await?;
                }
                Ok(())
            }
            .await;
            if let Err(error) = written {
                log_service.error(&format!("[storage main] ensureWorkspaceStorageFolderMeta(): Unable to create workspace storage metadata due to {error}"));
            }
        }
        Ok(())
    })
}

impl StorageMainKind for WorkspaceKind {
    fn path(&self) -> Option<String> {
        (!self.options.use_in_memory_storage).then(|| join(&[&self.folder_path(), STORAGE_NAME]))
    }

    fn do_create(&self) -> Promise<Rc<dyn IStorage>> {
        let prepared = self.prepare_workspace_storage_folder();
        let (log_service, use_in_memory_storage) = (self.log_service.clone(), self.options.use_in_memory_storage);
        async_fn(async move {
            let (storage_file_path, was_created) = prepared.await?;

            let hint = if use_in_memory_storage {
                Some(StorageHint::StorageInMemory)
            } else if was_created {
                Some(StorageHint::StorageDoesNotExist)
            } else {
                None
            };
            let storage: Rc<dyn IStorage> = Storage::new(open_database(&storage_file_path, &log_service), StorageOptions { hint });
            Ok(storage)
        })
    }
}

/// `WorkspaceStorageMain`.
pub struct WorkspaceStorageMain;

impl WorkspaceStorageMain {
    pub fn new(
        workspace: AnyWorkspaceIdentifier,
        options: StorageMainOptions,
        log_service: LogServiceRef,
        environment_service: &dyn EnvironmentService,
        file_service: Rc<dyn IFileService>,
    ) -> Rc<BaseStorageMain> {
        let kind = WorkspaceKind { workspace, options, log_service: log_service.clone(), workspace_storage_home: environment_service.workspace_storage_home() };
        BaseStorageMain::new(Rc::new(kind), log_service, file_service)
    }
}

/// `InMemoryStorageMain`.
struct InMemoryKind;

impl StorageMainKind for InMemoryKind {
    fn path(&self) -> Option<String> {
        None // in-memory has no path
    }

    fn do_create(&self) -> Promise<Rc<dyn IStorage>> {
        let storage: Rc<dyn IStorage> = Storage::new(InMemoryStorageDatabase::new(), StorageOptions { hint: Some(StorageHint::StorageInMemory) });
        async_fn(async move { Ok(storage) })
    }
}

/// `InMemoryStorageMain`.
pub struct InMemoryStorageMain;

impl InMemoryStorageMain {
    pub fn new(log_service: LogServiceRef, file_service: Rc<dyn IFileService>) -> Rc<BaseStorageMain> {
        BaseStorageMain::new(Rc::new(InMemoryKind), log_service, file_service)
    }
}
