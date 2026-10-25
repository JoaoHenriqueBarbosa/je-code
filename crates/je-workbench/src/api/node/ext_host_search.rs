//! Porte de `src/vs/workbench/api/node/extHostSearch.ts`: a `NativeExtHostSearch`, o `ExtHostSearch` do host de extensões em
//! Node. Registra o provedor de texto do ripgrep para `vscode-userdata` (e, num host remoto ou quando a thread principal pede
//! em `$enableExtensionHostSearch`, os do esquema `file`: o de texto e o motor interno de busca de arquivo), lê o número de
//! threads da configuração `search.ripgrep.maxThreads` e usa o disco para listar irmãos nos `exclude` com cláusula `when`.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - **Herança.** `NativeExtHostSearch extends ExtHostSearch` é a composição com `Deref` para a base: os registros herdados
//!   (`registerTextSearchProvider...`) são os métodos de [`ExtHostSearch`]. Os métodos de [`ExtHostSearchShape`] que o original
//!   sobrescreve (`$enableExtensionHostSearch`, `$provideFileSearchResults`, `$clearCache`) e os que repassa a `super`
//!   (`$getAIName`, `$provideAITextSearchResults`, `$provideTextSearchResults`) são os do trait; `protected createTextSearchManager`
//!   é o [`TextSearchManagerFactory`] que o construtor entrega à base.
//! - **Ripgrep e motor interno.** `RipgrepSearchProvider` (`node/ripgrepSearchProvider.ts`) e `SearchService`
//!   (`node/rawSearchService.ts`) lançam o processo `rg` e entram pelo [`NativeSearchBackends`] do construtor, como fábricas
//!   que recebem o `OutputChannel` e o `getNumThreads` (o provedor de texto) ou o tipo de processo e o `getNumThreads` (o
//!   [`IInternalFileSearchService`]). O `NativeSearchBackends::default()` são os motores reais, os do original; só os testes
//!   entregam dublês. O `ISerializedSearchProgressItem` do `search.ts` é o [`SerializedSearchProgressItem`].
//! - **Dependências.** `IExtHostRpcService` é o [`IMainContext`]; `IExtHostInitDataService` é o [`ExtHostSearchInitData`] (só o
//!   `remote`); `IURITransformerService` é o trait mínimo de `ext_host_workspace`; `ILogService` é o [`LogServiceRef`]. O
//!   `IExtHostConfiguration` é o [`IExtHostSearchConfiguration`] (só o `getConfigProvider` e o que o host lê do provedor:
//!   `onDidChangeConfiguration` e `getConfiguration('search').get('ripgrep.maxThreads')`); o `ExtHostConfiguration` real o
//!   implementa.
//! - **`_pfs`.** `protected _pfs: typeof pfs = pfs; // allow extending for tests` é o `readdir` de
//!   [`NativeExtHostSearch::set_readdir`] (o disco, enquanto ninguém o trocar). O `NativeTextSearchManager` do host leva o tipo de
//!   processo `textSearchProvider`.
//! - **`await`.** `getNumThreads` e `doInternalFileSearchWithCustomCallback` são `async fn` ([`async_fn`]).
//! - **`dispose`.** `this.isDisposed` é lido pela continuação de `getConfigProvider()`, que só guarda o ouvinte se o objeto ainda
//!   vive e não foi descartado.

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_platform::log::{LogArg, LogServiceRef};
use je_platform::resources::uri_file;

use crate::api::common::ext_host_configuration::{ExtHostConfigProvider, ExtHostConfiguration};
use crate::api::common::ext_host_protocol::{ExtHostSearchShape, IMainContext, IRawAITextQuery, IRawFileQuery, IRawTextQuery};
use crate::api::common::ext_host_search::{ExtHostSearch, IExtHostSearch, TextSearchManagerFactory};
use crate::api::common::ext_host_workspace::IURITransformerService;
use crate::services::search::raw_search_service::SearchService;
use crate::services::search::ripgrep_search_provider::{GetNumThreadsFn, RipgrepSearchProvider};
use crate::services::search::ripgrep_search_utils::OutputChannel;
use crate::services::search::search::{FileSearchStatsType, IFileQuery, IRawSearchService, ISearchCompleteStats, ISerializedSearchProgressItem, ITextQuery, TextSearchStatsType};
use crate::services::search::search_ext_types::TextSearchProvider2;
use crate::services::search::text_search_manager::NativeTextSearchManager;

/// `ISerializedSearchProgressItem = ISerializedFileMatch | ISerializedFileMatch[] | IProgressMessage`.
pub type SerializedSearchProgressItem = ISerializedSearchProgressItem;

/// `getNumThreads` entregue aos motores de busca.
pub type NumThreadsFn = GetNumThreadsFn;

/// `SearchService` (`node/rawSearchService.ts`), no que o host usa: a busca de arquivo com callback e o descarte do cache.
pub trait IInternalFileSearchService {
    /// `doFileSearch(query, numThreads, onResult, token)`.
    fn do_file_search(&self, query: &IFileQuery, num_threads: Option<i64>, on_result: Rc<dyn Fn(SerializedSearchProgressItem)>, token: &CancellationToken) -> Promise<ISearchCompleteStats>;

    /// `clearCache(cacheKey)`.
    fn clear_cache(&self, cache_key: &str);
}

impl IInternalFileSearchService for SearchService {
    fn do_file_search(&self, query: &IFileQuery, num_threads: Option<i64>, on_result: Rc<dyn Fn(SerializedSearchProgressItem)>, token: &CancellationToken) -> Promise<ISearchCompleteStats> {
        SearchService::do_file_search(self, query, num_threads, on_result, Some(token)).then(|success| ISearchCompleteStats { limit_hit: Some(success.limit_hit), messages: success.messages, stats: success.stats })
    }

    fn clear_cache(&self, cache_key: &str) {
        drop(IRawSearchService::clear_cache(self, cache_key));
    }
}

/// `new RipgrepSearchProvider(outputChannel, getNumThreads)`.
pub type RipgrepSearchProviderFactory = Rc<dyn Fn(OutputChannel, NumThreadsFn) -> Rc<dyn TextSearchProvider2>>;

/// `new SearchService(processType, getNumThreads)`.
pub type InternalFileSearchServiceFactory = Rc<dyn Fn(&str, NumThreadsFn) -> Rc<dyn IInternalFileSearchService>>;

/// Os motores que lançam o ripgrep (ver as notas do módulo). O `default` são os do original: o `RipgrepSearchProvider` e o
/// `SearchService`; os testes entregam dublês.
#[derive(Clone)]
pub struct NativeSearchBackends {
    pub create_ripgrep_search_provider: RipgrepSearchProviderFactory,
    pub create_internal_file_search_service: InternalFileSearchServiceFactory,
}

impl Default for NativeSearchBackends {
    fn default() -> Self {
        Self {
            create_ripgrep_search_provider: Rc::new(|output_channel: OutputChannel, get_num_threads: NumThreadsFn| {
                Rc::new(RipgrepSearchProvider::new(output_channel, get_num_threads)) as Rc<dyn TextSearchProvider2>
            }),
            create_internal_file_search_service: Rc::new(|process_type: &str, get_num_threads: NumThreadsFn| {
                let process_type = match process_type {
                    "fileSearchProvider" => FileSearchStatsType::FileSearchProvider,
                    _ => FileSearchStatsType::SearchProcess,
                };
                SearchService::new(process_type, Some(get_num_threads)) as Rc<dyn IInternalFileSearchService>
            }),
        }
    }
}

/// O `remote` de `IExtHostInitDataService`, no que o host lê.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtHostSearchInitData {
    pub is_remote: bool,
    pub authority: Option<String>,
}

/// O ouvinte de `onDidChangeConfiguration`: recebe o `event.affectsConfiguration`.
pub type ConfigChangeListener = Rc<dyn Fn(&dyn Fn(&str) -> bool)>;

/// O `ExtHostConfigProvider`, no que o host de busca lê.
pub trait IExtHostSearchConfigProvider {
    /// `onDidChangeConfiguration(listener)`.
    fn on_did_change_configuration(&self, listener: ConfigChangeListener) -> DisposableRef;

    /// `getConfiguration('search').get<number>('ripgrep.maxThreads')`.
    fn get_ripgrep_max_threads(&self) -> Option<i64>;
}

/// `IExtHostConfiguration`, no que o host de busca lê.
pub trait IExtHostSearchConfiguration {
    /// `getConfigProvider()`.
    fn get_config_provider(&self) -> Promise<Rc<dyn IExtHostSearchConfigProvider>>;
}

/// O provedor de configuração real, visto pelo host de busca.
struct ExtHostSearchConfigProvider(Rc<ExtHostConfigProvider>);

impl IExtHostSearchConfigProvider for ExtHostSearchConfigProvider {
    fn on_did_change_configuration(&self, listener: ConfigChangeListener) -> DisposableRef {
        self.0.on_did_change_configuration().listen(move |event| listener(&|section: &str| event.affects_configuration(section, None)))
    }

    fn get_ripgrep_max_threads(&self) -> Option<i64> {
        match self.0.get_configuration(Some("search"), None, None).get("ripgrep.maxThreads", None) {
            Dynamic::Number(num_threads) => Some(num_threads as i64),
            _ => None,
        }
    }
}

impl IExtHostSearchConfiguration for ExtHostConfiguration {
    fn get_config_provider(&self) -> Promise<Rc<dyn IExtHostSearchConfigProvider>> {
        ExtHostConfiguration::get_config_provider(self).then(|provider| Rc::new(ExtHostSearchConfigProvider(provider)) as Rc<dyn IExtHostSearchConfigProvider>)
    }
}

/// O `readdir` do disco que o host usa para listar irmãos (`_pfs.Promises.readdir`).
pub type ReaddirFn = Rc<dyn Fn(&str) -> Promise<Vec<String>>>;

/// `NativeExtHostSearch`.
pub struct NativeExtHostSearch {
    this: Weak<NativeExtHostSearch>,
    base: Rc<ExtHostSearch>,
    log_service: LogServiceRef,
    configuration_service: Rc<dyn IExtHostSearchConfiguration>,
    backends: NativeSearchBackends,

    /// `_pfs`, no que o host usa: `None` é o disco.
    readdir: Rc<RefCell<Option<ReaddirFn>>>,

    internal_file_search_handle: Cell<Option<u64>>,
    internal_file_search_provider: RefCell<Option<Rc<dyn IInternalFileSearchService>>>,

    registered_eh_search_provider: Cell<bool>,

    num_threads_promise: RefCell<Option<Promise<Option<i64>>>>,

    disposables: Rc<DisposableStore>,

    is_disposed: Cell<bool>,
}

impl Deref for NativeExtHostSearch {
    type Target = ExtHostSearch;

    fn deref(&self) -> &ExtHostSearch {
        &self.base
    }
}

impl NativeExtHostSearch {
    pub fn new(
        ext_host_rpc: &dyn IMainContext,
        init_data: &ExtHostSearchInitData,
        uri_transformer: Rc<dyn IURITransformerService>,
        configuration_service: Rc<dyn IExtHostSearchConfiguration>,
        log_service: LogServiceRef,
        backends: NativeSearchBackends,
    ) -> Rc<Self> {
        let readdir: Rc<RefCell<Option<ReaddirFn>>> = Rc::default();
        let create_text_search_manager: TextSearchManagerFactory = {
            let readdir = readdir.clone();
            Rc::new(move |query: ITextQuery, provider: Rc<dyn TextSearchProvider2>| {
                let readdir_fn: ReaddirFn = match readdir.borrow().clone() {
                    Some(readdir_fn) => readdir_fn,
                    None => Rc::new(NativeTextSearchManager::disk_readdir),
                };
                NativeTextSearchManager::with_readdir(query, provider, readdir_fn, TextSearchStatsType::TextSearchProvider)
            })
        };
        let base = ExtHostSearch::with_text_search_manager_factory(ext_host_rpc, uri_transformer, create_text_search_manager);

        let search = Rc::new_cyclic(|this| NativeExtHostSearch {
            this: this.clone(),
            base,
            log_service,
            configuration_service,
            backends,
            readdir,
            internal_file_search_handle: Cell::new(None),
            internal_file_search_provider: RefCell::new(None),
            registered_eh_search_provider: Cell::new(false),
            num_threads_promise: RefCell::new(None),
            disposables: DisposableStore::new(),
            is_disposed: Cell::new(false),
        });
        search.init(init_data);
        search
    }

    /// O corpo do construtor que precisa do `this`.
    fn init(&self, init_data: &ExtHostSearchInitData) {
        self.register_ripgrep_text_search_provider("RipgrepSearchUD", schemas::VSCODE_USER_DATA);
        if init_data.is_remote && init_data.authority.as_deref().is_some_and(|authority| !authority.is_empty()) {
            self.register_eh_search_providers();
        }

        let this = self.this.clone();
        drop(self.configuration_service.get_config_provider().then(move |provider| {
            let Some(this) = this.upgrade() else {
                return;
            };
            if this.is_disposed.get() {
                return;
            }
            let weak = this.this.clone();
            let listener: ConfigChangeListener = Rc::new(move |affects_configuration: &dyn Fn(&str) -> bool| {
                if let Some(this) = weak.upgrade() {
                    this.handle_configuration_changed(affects_configuration);
                }
            });
            this.disposables.add(provider.on_did_change_configuration(listener));
        }));
    }

    /// `this._disposables.add(this.registerTextSearchProvider(scheme, new RipgrepSearchProvider(outputChannel, this.getNumThreadsCached)))`.
    fn register_ripgrep_text_search_provider(&self, output_channel_prefix: &str, scheme: &str) {
        let output_channel = OutputChannel::new(output_channel_prefix, self.log_service.clone());
        let provider = (self.backends.create_ripgrep_search_provider)(output_channel, self.num_threads_fn());
        let registration = self.base.register_text_search_provider(scheme, provider).unwrap_or_else(|error| errors::throw(error));
        self.disposables.add(registration);
    }

    /// `this.getNumThreadsCached`, preso ao `this`.
    fn num_threads_fn(&self) -> NumThreadsFn {
        let this = self.this.clone();
        Rc::new(move || match this.upgrade() {
            Some(this) => this.get_num_threads_cached(),
            None => Promise::resolved(None),
        })
    }

    fn handle_configuration_changed(&self, affects_configuration: &dyn Fn(&str) -> bool) {
        if !affects_configuration("search") {
            return;
        }
        *self.num_threads_promise.borrow_mut() = None;
    }

    pub fn get_num_threads(&self) -> Promise<Option<i64>> {
        let configuration_service = self.configuration_service.clone();
        async_fn(async move {
            let config_provider = configuration_service.get_config_provider().await?;
            Ok(config_provider.get_ripgrep_max_threads())
        })
    }

    pub fn get_num_threads_cached(&self) -> Promise<Option<i64>> {
        let cached = self.num_threads_promise.borrow().clone();
        match cached {
            Some(promise) => promise,
            None => {
                let promise = self.get_num_threads();
                *self.num_threads_promise.borrow_mut() = Some(promise.clone());
                promise
            }
        }
    }

    /// `protected _pfs` (allow extending for tests): troca o `readdir` que lista os irmãos de um resultado.
    pub fn set_readdir(&self, readdir: ReaddirFn) {
        *self.readdir.borrow_mut() = Some(readdir);
    }

    fn register_eh_search_providers(&self) {
        if self.registered_eh_search_provider.get() {
            return;
        }

        self.registered_eh_search_provider.set(true);
        self.register_ripgrep_text_search_provider("RipgrepSearchEH", schemas::FILE);
        let provider = (self.backends.create_internal_file_search_service)("fileSearchProvider", self.num_threads_fn());
        let registration = self.register_internal_file_search_provider(schemas::FILE, provider);
        self.disposables.add(registration);
    }

    fn register_internal_file_search_provider(&self, scheme: &str, provider: Rc<dyn IInternalFileSearchService>) -> DisposableRef {
        let handle = self.base.allocate_handle();
        *self.internal_file_search_provider.borrow_mut() = Some(provider);
        self.internal_file_search_handle.set(Some(handle));
        self.base.proxy().rpc_register_file_search_provider(handle, &self.base.transform_scheme(scheme));
        let this = self.this.clone();
        to_disposable(move || {
            if let Some(this) = this.upgrade() {
                *this.internal_file_search_provider.borrow_mut() = None;
                this.base.proxy().rpc_unregister_provider(handle);
            }
        })
    }

    fn do_internal_file_search(&self, handle: u64, session: u64, raw_query: IFileQuery, token: CancellationToken) -> Promise<ISearchCompleteStats> {
        let proxy = self.base.proxy().clone();
        self.do_internal_file_search_with_custom_callback(&raw_query, &token, Rc::new(move |data: Vec<URI>| proxy.rpc_handle_file_match(handle, session, data)))
    }
}

impl IDisposable for NativeExtHostSearch {
    fn dispose(&self) {
        self.is_disposed.set(true);
        self.disposables.dispose();
    }
}

impl IExtHostSearch for NativeExtHostSearch {
    fn do_internal_file_search_with_custom_callback(&self, raw_query: &IFileQuery, token: &CancellationToken, handle_file_match: Rc<dyn Fn(Vec<URI>)>) -> Promise<ISearchCompleteStats> {
        let log_service = self.log_service.clone();
        let on_result: Rc<dyn Fn(SerializedSearchProgressItem)> = Rc::new(move |ev: SerializedSearchProgressItem| {
            let matches = match ev {
                SerializedSearchProgressItem::FileMatch(file_match) => vec![file_match],
                SerializedSearchProgressItem::FileMatches(matches) => matches,
                SerializedSearchProgressItem::Message(message) => {
                    if !message.message.is_empty() {
                        log_service.debug_args("ExtHostSearch", &[LogArg::from(message.message.as_str())]);
                    }
                    return;
                }
            };

            handle_file_match(matches.iter().map(|file_match| uri_file(&file_match.path)).collect());
        });

        let Some(internal_file_search_provider) = self.internal_file_search_provider.borrow().clone() else {
            return Promise::rejected(Error::new("No internal file search handler"));
        };
        let (query, token) = (raw_query.clone(), token.clone());
        let num_threads = self.get_num_threads_cached();
        async_fn(async move {
            let num_threads = num_threads.await?;
            internal_file_search_provider.do_file_search(&query, num_threads, on_result, &token).await
        })
    }
}

impl ExtHostSearchShape for NativeExtHostSearch {
    fn rpc_enable_extension_host_search(&self) {
        self.register_eh_search_providers();
    }

    fn rpc_get_ai_name(&self, handle: u64) -> Promise<Option<String>> {
        self.base.rpc_get_ai_name(handle)
    }

    fn rpc_provide_file_search_results(&self, handle: u64, session: u64, raw_query: IRawFileQuery, token: CancellationToken) -> Promise<ISearchCompleteStats> {
        if self.internal_file_search_handle.get() == Some(handle) {
            let start = runtime::date_now();
            let log_service = self.log_service.clone();
            return self.do_internal_file_search(handle, session, raw_query, token).then(move |result| {
                let elapsed = runtime::date_now().saturating_sub(start);
                log_service.debug(&format!("Ext host file search time: {elapsed}ms"));
                result
            });
        }

        self.base.rpc_provide_file_search_results(handle, session, raw_query, token)
    }

    fn rpc_provide_ai_text_search_results(&self, handle: u64, session: u64, query: IRawAITextQuery, token: CancellationToken) -> Promise<ISearchCompleteStats> {
        self.base.rpc_provide_ai_text_search_results(handle, session, query, token)
    }

    fn rpc_provide_text_search_results(&self, handle: u64, session: u64, query: IRawTextQuery, token: CancellationToken) -> Promise<ISearchCompleteStats> {
        self.base.rpc_provide_text_search_results(handle, session, query, token)
    }

    fn rpc_clear_cache(&self, cache_key: &str) -> Promise<()> {
        if let Some(provider) = self.internal_file_search_provider.borrow().as_ref() {
            provider.clear_cache(cache_key);
        }

        self.base.rpc_clear_cache(cache_key)
    }
}
