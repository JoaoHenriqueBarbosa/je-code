//! Porte de `src/vs/workbench/api/common/extHostSearch.ts`: `workspace.registerFileSearchProvider` e
//! `registerTextSearchProvider` do lado das extensões. O host guarda os provedores por handle, avisa a thread principal do
//! registro e responde ao `$provideFileSearchResults`, `$provideTextSearchResults` e `$provideAITextSearchResults` dela.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `IExtHostRpcService` é o [`IMainContext`] (só serve para pedir o proxy `MainThreadSearch` no construtor); `ILogService`
//!   não é guardado aqui (só a subclasse nativa o lê) e `IURITransformerService` é o trait mínimo de `ext_host_workspace`.
//! - os `UriComponents` do protocolo são [`URI`] (já revivida), então `reviveQuery` não existe: as consultas cruas são as
//!   próprias consultas ([`IRawFileQuery`] e as outras).
//! - `$provideFileSearchResults` e `$provideTextSearchResults` lançam `Error` de forma síncrona para um handle desconhecido;
//!   aqui devolvem a promessa rejeitada com o mesmo erro (o RPC entrega as duas formas como rejeição).
//! - `protected createTextSearchManager` é o fecho [`TextSearchManagerFactory`] do construtor (a subclasse nativa passa o dela);
//!   `createAITextSearchManager` não é sobrescrito por ninguém.
//! - os cinco `register*` repartem o mesmo corpo em [`ExtHostSearch::register_provider`]: cada tipo de provedor tem o seu
//!   [`ProviderRegistry`] (esquemas usados e provedores por handle), e a mensagem de "já registrado" vem de quem chama (a do
//!   provedor de IA tem o espaço faltando antes de `is`, como no original).
//! - `register*` lançam no original; aqui devolvem [`Result`], e o `IDisposable` é um [`DisposableRef`].
//! - `IExtHostSearch extends ExtHostSearchShape` carrega os registros, que no porte são métodos de [`ExtHostSearch`] (a
//!   `NativeExtHostSearch` os herda por `Deref`); o trait [`IExtHostSearch`] guarda só o que a subclasse sobrescreve
//!   (`doInternalFileSearchWithCustomCallback`).
//! - `batch.map(p => p.resource)` e o repasse do `IFileMatch` à thread principal: o `IRawFileMatch2` do protocolo guarda só o
//!   `resource` e os `results` (um provedor de texto não produz `cellResults`).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::promise::Promise;
use je_base::uri::URI;

use super::ext_host_protocol::{ExtHostSearchShape, IMainContext, IRawAITextQuery, IRawFileMatch2, IRawFileQuery, IRawTextQuery, MainContext, MainThreadSearchShape};
use super::ext_host_workspace::IURITransformerService;
use crate::services::search::file_search_manager::FileSearchManager;
use crate::services::search::search::{IFileMatch, IFileQuery, ISearchCompleteStats, ITextQuery, TextSearchStatsType};
use crate::services::search::search_ext_conversion_types::{FileSearchProvider, OldFileSearchProviderConverter, OldTextSearchProviderConverter, TextSearchProvider};
use crate::services::search::search_ext_types::{AISearchKeyword, AITextSearchProvider, FileSearchProvider2, TextSearchProvider2};
use crate::services::search::text_search_manager::{IFileUtils, TextQueryProviderPair, TextSearchManager};

/// O `createTextSearchManager` sobrescrevível: monta o gerenciador de uma busca de texto.
pub type TextSearchManagerFactory = Rc<dyn Fn(ITextQuery, Rc<dyn TextSearchProvider2>) -> TextSearchManager>;

/// `IExtHostSearch`, no que a subclasse nativa sobrescreve (ver as notas do módulo).
pub trait IExtHostSearch: ExtHostSearchShape {
    /// `doInternalFileSearchWithCustomCallback(query, token, handleFileMatch)`.
    fn do_internal_file_search_with_custom_callback(&self, query: &IFileQuery, token: &CancellationToken, handle_file_match: Rc<dyn Fn(Vec<URI>)>) -> Promise<ISearchCompleteStats>;
}

/// Os provedores de um tipo (`_textSearchProvider` e `_textSearchUsedSchemes`, e os pares de IA e de arquivo).
pub struct ProviderRegistry<P: ?Sized + 'static> {
    used_schemes: RefCell<HashSet<String>>,
    providers: RefCell<HashMap<u64, Rc<P>>>,
}

impl<P: ?Sized + 'static> ProviderRegistry<P> {
    fn new() -> Rc<Self> {
        Rc::new(Self { used_schemes: RefCell::new(HashSet::new()), providers: RefCell::new(HashMap::new()) })
    }

    fn get(&self, handle: u64) -> Option<Rc<P>> {
        self.providers.borrow().get(&handle).cloned()
    }
}

/// `ExtHostSearch`.
pub struct ExtHostSearch {
    proxy: Rc<dyn MainThreadSearchShape>,
    handle_pool: Cell<u64>,

    text_search: Rc<ProviderRegistry<dyn TextSearchProvider2>>,
    ai_text_search: Rc<ProviderRegistry<dyn AITextSearchProvider>>,
    file_search: Rc<ProviderRegistry<dyn FileSearchProvider2>>,

    file_search_manager: FileSearchManager,
    uri_transformer: Rc<dyn IURITransformerService>,
    create_text_search_manager: TextSearchManagerFactory,
}

/// `new TextSearchManager({ query, provider }, { readdir: resource => Promise.resolve([]), toCanonicalName: encoding => encoding }, processType)`.
fn text_search_manager_without_disk(pair: TextQueryProviderPair, process_type: TextSearchStatsType) -> TextSearchManager {
    TextSearchManager::new(
        pair,
        IFileUtils { readdir: Rc::new(|_: &URI| Promise::resolved(Vec::new())), to_canonical_name: Rc::new(|encoding: &str| encoding.to_owned()) },
        process_type,
    )
}

impl ExtHostSearch {
    pub fn new(ext_host_rpc: &dyn IMainContext, uri_transformer: Rc<dyn IURITransformerService>) -> Rc<Self> {
        let create_text_search_manager: TextSearchManagerFactory = Rc::new(|query: ITextQuery, provider: Rc<dyn TextSearchProvider2>| {
            text_search_manager_without_disk(TextQueryProviderPair::Text { query, provider }, TextSearchStatsType::TextSearchProvider)
        });
        Self::with_text_search_manager_factory(ext_host_rpc, uri_transformer, create_text_search_manager)
    }

    /// O construtor de uma subclasse que sobrescreve `createTextSearchManager`.
    pub fn with_text_search_manager_factory(
        ext_host_rpc: &dyn IMainContext,
        uri_transformer: Rc<dyn IURITransformerService>,
        create_text_search_manager: TextSearchManagerFactory,
    ) -> Rc<Self> {
        Rc::new(ExtHostSearch {
            proxy: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_SEARCH),
            handle_pool: Cell::new(0),
            text_search: ProviderRegistry::new(),
            ai_text_search: ProviderRegistry::new(),
            file_search: ProviderRegistry::new(),
            file_search_manager: FileSearchManager::new(),
            uri_transformer,
            create_text_search_manager,
        })
    }

    /// `this._proxy`.
    pub fn proxy(&self) -> &Rc<dyn MainThreadSearchShape> {
        &self.proxy
    }

    /// `this._handlePool++`.
    pub fn allocate_handle(&self) -> u64 {
        let handle = self.handle_pool.get();
        self.handle_pool.set(handle + 1);
        handle
    }

    /// `_transformScheme`.
    pub fn transform_scheme(&self, scheme: &str) -> String {
        self.uri_transformer.transform_outgoing_scheme(scheme)
    }

    /// O corpo comum dos `register*`: recusa um esquema já usado, guarda o provedor sob um handle novo, avisa a thread principal
    /// (`register_remote`) e devolve o `IDisposable` que desfaz tudo.
    fn register_provider<P: ?Sized + 'static>(
        &self,
        registry: &Rc<ProviderRegistry<P>>,
        scheme: &str,
        provider: Rc<P>,
        already_registered: String,
        register_remote: impl FnOnce(&dyn MainThreadSearchShape, u64, &str),
    ) -> Result<DisposableRef, Error> {
        if registry.used_schemes.borrow().contains(scheme) {
            return Err(Error::new(already_registered));
        }

        registry.used_schemes.borrow_mut().insert(scheme.to_owned());
        let handle = self.allocate_handle();
        registry.providers.borrow_mut().insert(handle, provider);
        register_remote(self.proxy.as_ref(), handle, &self.transform_scheme(scheme));

        let (registry, proxy, scheme) = (registry.clone(), self.proxy.clone(), scheme.to_owned());
        Ok(to_disposable(move || {
            registry.used_schemes.borrow_mut().remove(&scheme);
            registry.providers.borrow_mut().remove(&handle);
            proxy.rpc_unregister_provider(handle);
        }))
    }

    pub fn register_text_search_provider_old(&self, scheme: &str, provider: Rc<dyn TextSearchProvider>) -> Result<DisposableRef, Error> {
        self.register_text_search_provider(scheme, Rc::new(OldTextSearchProviderConverter::new(provider)))
    }

    pub fn register_text_search_provider(&self, scheme: &str, provider: Rc<dyn TextSearchProvider2>) -> Result<DisposableRef, Error> {
        self.register_provider(
            &self.text_search,
            scheme,
            provider,
            format!("a text search provider for the scheme '{scheme}' is already registered"),
            |proxy, handle, scheme| proxy.rpc_register_text_search_provider(handle, scheme),
        )
    }

    pub fn register_ai_text_search_provider(&self, scheme: &str, provider: Rc<dyn AITextSearchProvider>) -> Result<DisposableRef, Error> {
        self.register_provider(
            &self.ai_text_search,
            scheme,
            provider,
            format!("an AI text search provider for the scheme '{scheme}'is already registered"),
            |proxy, handle, scheme| proxy.rpc_register_ai_text_search_provider(handle, scheme),
        )
    }

    pub fn register_file_search_provider_old(&self, scheme: &str, provider: Rc<dyn FileSearchProvider>) -> Result<DisposableRef, Error> {
        self.register_file_search_provider(scheme, Rc::new(OldFileSearchProviderConverter::new(provider)))
    }

    pub fn register_file_search_provider(&self, scheme: &str, provider: Rc<dyn FileSearchProvider2>) -> Result<DisposableRef, Error> {
        self.register_provider(
            &self.file_search,
            scheme,
            provider,
            format!("a file search provider for the scheme '{scheme}' is already registered"),
            |proxy, handle, scheme| proxy.rpc_register_file_search_provider(handle, scheme),
        )
    }

    /// `createAITextSearchManager`.
    fn create_ai_text_search_manager(query: crate::services::search::search::IAITextQuery, provider: Rc<dyn AITextSearchProvider>) -> TextSearchManager {
        text_search_manager_without_disk(TextQueryProviderPair::Ai { query, provider }, TextSearchStatsType::AiTextSearchProvider)
    }

    /// O callback de resultados de texto: entrega o lote à thread principal (`$handleTextMatch`).
    fn text_match_reporter(&self, handle: u64, session: u64) -> Rc<dyn Fn(Vec<IFileMatch>)> {
        let proxy = self.proxy.clone();
        Rc::new(move |progress: Vec<IFileMatch>| {
            let data = progress.into_iter().map(|file_match| IRawFileMatch2 { resource: file_match.resource, results: file_match.results }).collect();
            proxy.rpc_handle_text_match(handle, session, data);
        })
    }
}

impl IExtHostSearch for ExtHostSearch {
    /// Não faz nada além de devolver as mensagens vazias: só a subclasse nativa tem um motor interno de busca de arquivo.
    fn do_internal_file_search_with_custom_callback(&self, _query: &IFileQuery, _token: &CancellationToken, _handle_file_match: Rc<dyn Fn(Vec<URI>)>) -> Promise<ISearchCompleteStats> {
        Promise::resolved(ISearchCompleteStats { limit_hit: None, messages: Vec::new(), stats: None })
    }
}

impl ExtHostSearchShape for ExtHostSearch {
    fn rpc_enable_extension_host_search(&self) {}

    fn rpc_get_ai_name(&self, handle: u64) -> Promise<Option<String>> {
        let Some(provider) = self.ai_text_search.get(handle) else {
            return Promise::resolved(None);
        };

        // if the provider is defined, but has no name, use default name
        Promise::resolved(Some(provider.name().unwrap_or_else(|| "AI".to_owned())))
    }

    fn rpc_provide_file_search_results(&self, handle: u64, session: u64, query: IRawFileQuery, token: CancellationToken) -> Promise<ISearchCompleteStats> {
        let Some(provider) = self.file_search.get(handle) else {
            return Promise::rejected(Error::new(format!("unknown provider: {handle}")));
        };

        let proxy = self.proxy.clone();
        self.file_search_manager.file_search(
            &query,
            provider,
            Rc::new(move |batch: Vec<IFileMatch>| proxy.rpc_handle_file_match(handle, session, batch.into_iter().map(|file_match| file_match.resource).collect())),
            &token,
        )
    }

    fn rpc_provide_ai_text_search_results(&self, handle: u64, session: u64, query: IRawAITextQuery, token: CancellationToken) -> Promise<ISearchCompleteStats> {
        let Some(provider) = self.ai_text_search.get(handle) else {
            return Promise::rejected(Error::new(format!("Unknown AI Text Search Provider {handle}")));
        };

        let engine = Self::create_ai_text_search_manager(query, provider);
        let proxy = self.proxy.clone();
        engine.search(
            self.text_match_reporter(handle, session),
            &token,
            Some(Rc::new(move |result: &AISearchKeyword| proxy.rpc_handle_keyword_result(handle, session, result.clone()))),
        )
    }

    fn rpc_provide_text_search_results(&self, handle: u64, session: u64, query: IRawTextQuery, token: CancellationToken) -> Promise<ISearchCompleteStats> {
        let Some(provider) = self.text_search.get(handle) else {
            return Promise::rejected(Error::new(format!("Unknown Text Search Provider {handle}")));
        };

        let engine = (self.create_text_search_manager)(query, provider);
        engine.search(self.text_match_reporter(handle, session), &token, None)
    }

    fn rpc_clear_cache(&self, cache_key: &str) -> Promise<()> {
        self.file_search_manager.clear_cache(cache_key);

        Promise::resolved(())
    }
}
