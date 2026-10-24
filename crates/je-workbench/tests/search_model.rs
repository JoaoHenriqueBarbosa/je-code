//! Porte de `src/vs/workbench/contrib/search/test/browser/searchModel.test.ts` (suíte `SearchModel`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`setup` e `teardown`.** O guarda de vazamentos (`ensureNoDisposablesAreLeakedInTestSuite`) é o [`LeakGuard`] criado no
//!   começo de cada caso. O `TestInstantiationService` é um [`InstantiationService`] sobre um [`ServiceCollection`] com os mesmos
//!   serviços (telemetria, rótulos, modelos, notebooks, identidade de URI, log e configuração com `search.searchOnType`
//!   ligado). O `instantiationService.stub(...)` de um serviço só vale para quem é criado depois dele (o modelo captura os
//!   serviços no construtor), então cada caso monta o serviço de busca e o de notebooks e só então cria o modelo com
//!   [`Fixture::create_model`]; os `stub` que o original faz depois da criação do modelo (casos `Search results are cleared
//!   during search` e `Previous search is cancelled when new search is called`) não alcançam o modelo e ficam de fora. O
//!   `INotebookService` do original não é usado pelo modelo do porte, e o `NotebookEditorWidgetService` real (que exige os
//!   grupos de editores, o contexto e o serviço de editores) é um dublê de `INotebookEditorService` sem editores abertos, que é
//!   o que o real devolve sem editores. `IReplaceService` não tem registro no original; aqui é um dublê que falha se usado
//!   (nenhum caso o usa), porque o contrato do porte o exige. O `ILabelService` é o `MockLabelService` do porte (o do original
//!   devolve `''` em `getUriBasenameLabel`, que nenhum caso lê).
//! - **Telemetria.** O modelo do porte usa `publicLog2` (o `publicLog` do original, com os mesmos eventos). O `sinon.stub`
//!   que devolve `nullEvent` e o `instantiationService.spy(ITelemetryService, 'publicLog')` são o [`RecordingTelemetryService`],
//!   que registra o nome de cada evento. O `nullEvent` (e o `stop` espionado, `target2`, que o original nunca confere) não
//!   existe: o `publicLog2` do porte não devolve evento.
//! - **`searchServiceWithResults(results)` sem `complete`.** O original resolve a promessa com `null` (o `complete!`), o que
//!   faz o `.then` do modelo lançar `TypeError` numa promessa que o caso `Search results are cleared during search` nunca
//!   espera. O porte resolve com um `ISearchComplete` vazio, que mantém o único efeito conferido (o estado síncrono dos
//!   resultados) sem criar uma rejeição sem tratador.
//! - **`sinon.stub(CellMatch.prototype, 'addContext')`.** É a célula falsa cujo `resolveTextModel()` nunca resolve (o mesmo
//!   efeito: o contexto não é preenchido). O `{ cellKind, textBuffer, id } as ICellViewModel` é o [`FakeCellViewModel`] sobre
//!   um buffer real com o texto da linha 1 (as demais linhas do `getLineContent` falso são vazias e nunca são lidas).
//! - **`sinon.spy(notebookSearchService, 'notebookSearch')`.** É o contador `call_count` do [`TestNotebookSearchService`].
//! - **`(match as MatchInNotebook)`** é `as_match_in_notebook()`, e `===` entre casamentos é a identidade do objeto.
//! - **Casos que retornam promessa** esvaziam as microtarefas (`runtime::run_microtasks`) e, nos que esperam
//!   `timeout(1)`, o laço de eventos (`runtime::run_until_idle`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::map::{ResourceMap, ResourceSet};
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_editor::model::{DefaultEndOfLine, FindMatch};
use je_editor::model_service::ModelService;
use je_editor::range::Range;
use je_editor::services_model::{I_MODEL_SERVICE, IModelService};
use je_editor::text_model::create_text_buffer;
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::{ILabelService, I_LABEL_SERVICE};
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_platform::resources::uri_file;
use je_platform::telemetry::telemetry::I_TELEMETRY_SERVICE;
use je_platform::telemetry::{CommonPropertyValue, ITelemetryData, ITelemetryService, TelemetryLevel};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::notebook::browser::notebook_browser::{
    CellEditState, CellWebviewFindMatch, ICellViewModel, SearchPreviewInfo, SearchPreviewRange,
};
use je_workbench::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use je_workbench::contrib::notebook::browser::services::notebook_editor_service::{I_NOTEBOOK_EDITOR_SERVICE, IBorrowValue, INotebookEditorService};
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, TextBufferRef};
use je_workbench::contrib::search::common::notebook_search::{I_NOTEBOOK_SEARCH_SERVICE, INotebookSearchService, NotebookSearchResult};
use je_workbench::contrib::search::notebook_search::notebook_search_model_base::IMatchInNotebook;
use je_workbench::contrib::search::notebook_search::search_notebook_helpers::{
    INotebookCellMatch, INotebookCellMatchWithModel, content_matches_to_text_search_matches, webview_matches_to_text_search_matches,
};
use je_workbench::contrib::search::replace::{I_REPLACE_SERVICE, IReplaceService};
use je_workbench::contrib::search::search_tree_model::search_model::SearchModelImpl;
use je_workbench::contrib::search::search_tree_model::search_tree_common::{
    FileMatchOrMatch, ISearchModel, ISearchTreeFileMatch, ISearchTreeMatch,
};
use je_workbench::services::search::search::{
    I_SEARCH_SERVICE, IAITextQuery, IFileMatch, IFileQuery, IFileSearchStats, IFolderQuery, ISearchComplete, ISearchEngineStats,
    ISearchProgressItem, ISearchResultProvider, ISearchService, ISearchStats, ITextQuery, ITextQueryProps, ITextSearchResult, FileSearchDetailStats,
    FileSearchStatsType, OneLineRange, PatternInfo, SearchProgressCallback, SearchProviderType, SearchRange, SharedResourceSet, TextSearchMatch,
    TextSearchSplitSyncAsync,
};
use je_workbench::services::textfile::collaborators::{IProgress, ProgressStep};
use je_workbench::test::browser::workbench_test_services::MockLabelService;
use je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService;

fn line_one_range() -> SearchRange {
    OneLineRange::new(1, 0, 1)
}

// ---------------------------------------------------------------------------------------------------------
// searchTestCommon.ts
// ---------------------------------------------------------------------------------------------------------

fn is_windows() -> bool {
    je_base::platform::Platform::detect().is_windows
}

/// `getRootName()`.
fn get_root_name() -> &'static str {
    if is_windows() { "c:" } else { "" }
}

/// `createFileUriFromPathFromRoot(path?)`.
fn create_file_uri_from_path_from_root(path: Option<&str>) -> URI {
    let root_name = get_root_name();
    match path.filter(|path| !path.is_empty()) {
        Some(path) => uri_file(&format!("{root_name}{path}")),
        None => {
            if is_windows() {
                uri_file(&format!("{root_name}/"))
            } else {
                uri_file(root_name)
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Dublês dos serviços
// ---------------------------------------------------------------------------------------------------------

fn unused(service: &str) -> ! {
    panic!("{service} não é usado por este teste (o modelo não chama esse método)")
}

/// Telemetria que registra o nome de cada evento (`publicLog` e `publicLog2`).
struct RecordingTelemetryService {
    events: RefCell<Vec<String>>,
}

impl RecordingTelemetryService {
    fn new() -> Rc<Self> {
        Rc::new(Self { events: RefCell::new(Vec::new()) })
    }

    fn events(&self) -> Vec<String> {
        self.events.borrow().clone()
    }

    /// `target.calledWith(name)`.
    fn called_with(&self, name: &str) -> bool {
        self.events.borrow().iter().any(|event| event == name)
    }
}

impl ITelemetryService for RecordingTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        TelemetryLevel::Usage
    }
    fn session_id(&self) -> String {
        String::new()
    }
    fn machine_id(&self) -> String {
        String::new()
    }
    fn sqm_id(&self) -> String {
        String::new()
    }
    fn dev_device_id(&self) -> String {
        String::new()
    }
    fn first_session_date(&self) -> String {
        String::new()
    }
    fn send_error_telemetry(&self) -> bool {
        false
    }
    fn public_log(&self, event_name: &str, _data: Option<ITelemetryData>) {
        self.events.borrow_mut().push(event_name.to_string());
    }
    fn public_log2(&self, event_name: &str, _data: Option<Dynamic>) {
        self.events.borrow_mut().push(event_name.to_string());
    }
    fn public_log_error(&self, _error_event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log_error2(&self, _event_name: &str, _data: Option<Dynamic>) {}
    fn set_experiment_property(&self, _name: &str, _value: &str) {}
    fn set_common_property(&self, _name: &str, _value: Option<CommonPropertyValue>) {}
}

/// `IReplaceService` que o original deixa sem registro.
struct UnusedReplaceService;

impl IReplaceService for UnusedReplaceService {
    fn replace_match(&self, _match: &Rc<dyn ISearchTreeMatch>) -> Promise<()> {
        unused("IReplaceService")
    }

    fn replace_files(&self, _files: &[Rc<dyn ISearchTreeFileMatch>], _progress: Option<&dyn IProgress<ProgressStep>>) -> Promise<()> {
        unused("IReplaceService")
    }

    fn open_replace_preview(&self, _element: &FileMatchOrMatch, _preserve_focus: Option<bool>, _side_by_side: Option<bool>, _pinned: Option<bool>) -> Promise<()> {
        unused("IReplaceService")
    }

    fn update_replace_preview(&self, _file: &Rc<dyn ISearchTreeFileMatch>, _override: Option<bool>) -> Promise<()> {
        unused("IReplaceService")
    }
}

/// O `INotebookEditorService` sem editores abertos.
struct NoNotebookEditorService {
    on_did_add_notebook_editor: Rc<Emitter<Rc<dyn NotebookEditorWidget>>>,
}

impl INotebookEditorService for NoNotebookEditorService {
    fn on_did_add_notebook_editor(&self) -> Event<Rc<dyn NotebookEditorWidget>> {
        self.on_did_add_notebook_editor.event()
    }

    fn retrieve_existing_widget_from_uri(&self, _resource: &URI) -> Option<IBorrowValue<dyn NotebookEditorWidget>> {
        None
    }
}

/// `{ cellKind, textBuffer, id } as ICellViewModel`: o texto do modelo nunca resolve (o `addContext` do original é
/// substituído por um stub que não faz nada).
struct FakeCellViewModel {
    id: String,
    cell_kind: CellKind,
    text_buffer: TextBufferRef,
}

impl FakeCellViewModel {
    fn new(id: &str, cell_kind: CellKind, line_one: &str) -> Rc<dyn ICellViewModel> {
        let text_buffer: TextBufferRef = Rc::new(RefCell::new(create_text_buffer(line_one.into(), DefaultEndOfLine::Lf)));
        Rc::new(Self { id: id.to_string(), cell_kind, text_buffer })
    }
}

impl ICellViewModel for FakeCellViewModel {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn cell_kind(&self) -> CellKind {
        self.cell_kind
    }

    fn text_buffer(&self) -> TextBufferRef {
        self.text_buffer.clone()
    }

    fn resolve_text_model(&self) -> Promise<Rc<je_editor::text_model::TextModel>> {
        Promise::new().0
    }

    fn get_edit_state(&self) -> CellEditState {
        unused("ICellViewModel.getEditState")
    }

    fn update_edit_state(&self, _new_state: CellEditState, _source: &str) {
        unused("ICellViewModel.updateEditState")
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `ISearchComplete` com os campos que os casos usam.
fn search_complete(limit_hit: Option<bool>, results: Vec<IFileMatch>, stats: Option<ISearchStats>) -> ISearchComplete {
    ISearchComplete { limit_hit, messages: Vec::new(), stats, results, exit: None, ai_keywords: None }
}

/// `{ results: [], messages: [] }`.
fn empty_complete() -> ISearchComplete {
    search_complete(None, Vec::new(), None)
}

/// Os quatro `ISearchService` do original (`searchServiceWithResults`, `searchServiceWithError`, `canceleableSearchService` e
/// `searchServiceWithDeferredPromise`). O modelo só chama `textSearchSplitSyncAsync`.
enum SearchServiceBehavior {
    WithResults { results: Vec<IFileMatch>, complete: ISearchComplete },
    WithError(Error),
    Cancelable { token_source: Rc<CancellationTokenSource>, store: Rc<DisposableStore> },
    WithDeferredPromise(Promise<ISearchComplete>),
}

struct TestSearchService {
    behavior: SearchServiceBehavior,
}

impl TestSearchService {
    fn with_results(results: Vec<IFileMatch>, complete: Option<ISearchComplete>) -> Rc<dyn ISearchService> {
        Rc::new(Self { behavior: SearchServiceBehavior::WithResults { results, complete: complete.unwrap_or_else(empty_complete) } })
    }

    fn with_error(error: Error) -> Rc<dyn ISearchService> {
        Rc::new(Self { behavior: SearchServiceBehavior::WithError(error) })
    }

    fn cancelable(token_source: &Rc<CancellationTokenSource>, guard: &LeakGuard) -> Rc<dyn ISearchService> {
        Rc::new(Self { behavior: SearchServiceBehavior::Cancelable { token_source: token_source.clone(), store: guard.store() } })
    }

    fn with_deferred_promise(promise: Promise<ISearchComplete>) -> Rc<dyn ISearchService> {
        Rc::new(Self { behavior: SearchServiceBehavior::WithDeferredPromise(promise) })
    }
}

/// `queueMicrotask(() => { results.forEach(onProgress!); resolve(complete!) })`.
fn deliver_results(results: Vec<IFileMatch>, complete: ISearchComplete, on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
    Promise::with_executor(move |resolver| {
        runtime::queue_microtask(move || {
            if let Some(on_progress) = &on_progress {
                for result in &results {
                    on_progress(&ISearchProgressItem::FileMatch(result.clone()));
                }
            }
            resolver.resolve(complete);
        });
    })
}

impl ISearchService for TestSearchService {
    fn text_search(&self, _query: &ITextQuery, _token: Option<CancellationToken>, _on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
        unused("ISearchService.textSearch")
    }

    fn ai_text_search(&self, _query: &IAITextQuery, _token: Option<CancellationToken>, _on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
        unused("ISearchService.aiTextSearch")
    }

    fn get_ai_name(&self) -> Promise<Option<String>> {
        unused("ISearchService.getAIName")
    }

    fn text_search_split_sync_async(
        &self,
        _query: &ITextQuery,
        token: Option<CancellationToken>,
        on_progress: Option<SearchProgressCallback>,
        _notebook_files_to_ignore: Option<SharedResourceSet>,
        _async_notebook_files_to_ignore: Option<Promise<SharedResourceSet>>,
    ) -> TextSearchSplitSyncAsync {
        let async_results = match &self.behavior {
            SearchServiceBehavior::WithResults { results, complete } => deliver_results(results.clone(), complete.clone(), on_progress),
            SearchServiceBehavior::WithError(error) => Promise::rejected(error.clone()),
            SearchServiceBehavior::Cancelable { token_source, store } => {
                if let Some(token) = token {
                    let token_source = token_source.clone();
                    store.add(token.on_cancellation_requested().listen(move |_| token_source.cancel()));
                }
                deliver_results(Vec::new(), empty_complete(), None)
            }
            SearchServiceBehavior::WithDeferredPromise(promise) => promise.clone(),
        };
        TextSearchSplitSyncAsync { sync_results: empty_complete(), async_results }
    }

    fn file_search(&self, _query: &IFileQuery, _token: Option<CancellationToken>) -> Promise<ISearchComplete> {
        unused("ISearchService.fileSearch")
    }

    fn scheme_has_file_search_provider(&self, _scheme: &str) -> bool {
        unused("ISearchService.schemeHasFileSearchProvider")
    }

    fn clear_cache(&self, _cache_key: &str) -> Promise<()> {
        unused("ISearchService.clearCache")
    }

    fn register_search_result_provider(&self, _scheme: &str, _provider_type: SearchProviderType, _provider: Rc<dyn ISearchResultProvider>) -> DisposableRef {
        unused("ISearchService.registerSearchResultProvider")
    }
}

/// `notebookSearchServiceWithInfo(results, tokenSource)`.
struct TestNotebookSearchService {
    results: Vec<IFileMatch>,
    token_source: Option<Rc<CancellationTokenSource>>,
    store: Rc<DisposableStore>,
    call_count: Cell<usize>,
}

impl TestNotebookSearchService {
    fn new(results: Vec<IFileMatch>, token_source: Option<&Rc<CancellationTokenSource>>, guard: &LeakGuard) -> Rc<Self> {
        Rc::new(Self { results, token_source: token_source.cloned(), store: guard.store(), call_count: Cell::new(0) })
    }
}

impl INotebookSearchService for TestNotebookSearchService {
    fn notebook_search(
        &self,
        _query: &ITextQuery,
        token: Option<CancellationToken>,
        _search_instance_id: &str,
        on_progress: Option<SearchProgressCallback>,
    ) -> NotebookSearchResult {
        self.call_count.set(self.call_count.get() + 1);
        if let Some(token) = token {
            let token_source = self.token_source.clone();
            self.store.add(token.on_cancellation_requested().listen(move |_| {
                if let Some(token_source) = &token_source {
                    token_source.cancel();
                }
            }));
        }
        let mut local_results: ResourceMap<IFileMatch> = ResourceMap::with_key_fn(Rc::new(|uri: &URI| uri.path().clone()));
        for result in &self.results {
            local_results.set(result.resource.clone(), result.clone());
        }

        let local: Vec<IFileMatch> = local_results.values().cloned().collect();
        if let Some(on_progress) = &on_progress {
            for result in &local {
                on_progress(&ISearchProgressItem::FileMatch(result.clone()));
            }
        }
        NotebookSearchResult {
            open_files_to_scan: Rc::new(RefCell::new(ResourceSet::from_entries(local_results.keys().cloned(), None))),
            complete_data: Promise::resolved(search_complete(Some(false), local, None)),
            all_scanned_files: Promise::resolved(Rc::new(RefCell::new(ResourceSet::new(None)))),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// setup
// ---------------------------------------------------------------------------------------------------------

struct Fixture {
    model_service: Rc<dyn IModelService>,
    configuration_service: Rc<dyn IConfigurationService>,
    notebook_editor_service: Rc<dyn INotebookEditorService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    label_service: Rc<dyn ILabelService>,
}

fn setup(guard: &LeakGuard) -> Fixture {
    // stubModelService
    let test_configuration_service = TestConfigurationService::new(None);
    guard.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    let _ = test_configuration_service.set_user_configuration("search", Dynamic::object([("searchOnType", Dynamic::Bool(true))]), None);
    let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;
    let model_service = guard.add_rc(ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service.clone()),
        UndoRedoService::for_tests(),
    ));
    let model_service: Rc<dyn IModelService> = model_service;

    // stubNotebookEditorService
    let on_did_add_notebook_editor = Emitter::<Rc<dyn NotebookEditorWidget>>::new();
    guard.add_rc(on_did_add_notebook_editor.clone());
    let notebook_editor_service: Rc<dyn INotebookEditorService> = Rc::new(NoNotebookEditorService { on_did_add_notebook_editor });

    let file_service = guard.add_rc(FileService::new(NullLogService::shared()));
    let file_service: Rc<dyn IFileService> = file_service;
    let uri_identity_service = guard.add_rc(UriIdentityService::create((), (file_service,)));
    let uri_identity_service: Rc<dyn IUriIdentityService> = uri_identity_service;

    let label_service: Rc<dyn ILabelService> = MockLabelService::new(None);
    Fixture { model_service, configuration_service, notebook_editor_service, uri_identity_service, label_service }
}

impl Fixture {
    /// `instantiationService.createInstance(SearchModelImpl)` com os serviços de busca e a telemetria do caso.
    fn create_model(
        &self,
        guard: &LeakGuard,
        search_service: Rc<dyn ISearchService>,
        notebook_search_service: Rc<dyn INotebookSearchService>,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<SearchModelImpl> {
        let replace_service: Rc<dyn IReplaceService> = Rc::new(UnusedReplaceService);
        let services = ServiceCollection::new()
            .with(I_TELEMETRY_SERVICE, telemetry_service)
            .with(I_MODEL_SERVICE, self.model_service.clone())
            .with(I_NOTEBOOK_EDITOR_SERVICE, self.notebook_editor_service.clone())
            .with(I_URI_IDENTITY_SERVICE, self.uri_identity_service.clone())
            .with(I_REPLACE_SERVICE, replace_service)
            .with(I_LABEL_SERVICE, self.label_service.clone())
            .with(I_LOG_SERVICE, NullLogService::shared())
            .with(I_CONFIGURATION_SERVICE, self.configuration_service.clone())
            .with(I_SEARCH_SERVICE, search_service)
            .with(I_NOTEBOOK_SEARCH_SERVICE, notebook_search_service);
        let instantiation_service = InstantiationService::new(services);
        guard.add_rc(instantiation_service.create_instance::<SearchModelImpl>(()))
    }
}

// ---------------------------------------------------------------------------------------------------------
// Auxiliares do original
// ---------------------------------------------------------------------------------------------------------

fn test_search_stats() -> ISearchStats {
    ISearchStats::File(IFileSearchStats {
        from_cache: false,
        detail_stats: FileSearchDetailStats::Engine(ISearchEngineStats {
            file_walk_time: 0.0,
            directories_walked: 2.0,
            files_walked: 3.0,
            cmd_time: 0.0,
            cmd_result_count: Some(0.0),
        }),
        result_count: 1.0,
        stats_type: FileSearchStatsType::SearchProcess,
        sorting_time: None,
    })
}

/// `folderQueries`.
fn folder_queries() -> Vec<IFolderQuery> {
    vec![IFolderQuery::new(create_file_uri_from_path_from_root(None))]
}

/// `{ contentPattern: { pattern, isRegExp? }, type: QueryType.Text, folderQueries }`.
fn text_query(pattern: &str, is_reg_exp: Option<bool>) -> ITextQuery {
    ITextQueryProps::new(folder_queries(), PatternInfo { is_reg_exp, ..PatternInfo::new(pattern) })
}

fn text_match(text: &str, range: SearchRange) -> TextSearchMatch {
    TextSearchMatch::new(text, &[range], None, None)
}

/// `aRawMatch(resource, ...results)`.
fn a_raw_match(resource: &str, results: Vec<TextSearchMatch>) -> IFileMatch {
    IFileMatch::new(create_file_uri_from_path_from_root(Some(resource)), Some(results.into_iter().map(ITextSearchResult::Match).collect()))
}

/// `aRawMatchWithCells(resource, ...cells)`.
fn a_raw_match_with_cells(resource: &str, cells: Vec<INotebookCellMatchWithModel>) -> IFileMatch {
    IFileMatch {
        resource: create_file_uri_from_path_from_root(Some(resource)),
        results: None,
        cell_results: Some(cells.into_iter().map(INotebookCellMatch::WithModel).collect()),
    }
}

fn matches_of(model: &Rc<SearchModelImpl>) -> Vec<Rc<dyn ISearchTreeFileMatch>> {
    model.search_result().matches(None)
}

/// `a === b` entre dois casamentos vistos por contratos diferentes: a identidade do objeto.
fn same_object<A: ?Sized, B: ?Sized>(a: &Rc<A>, b: &Rc<B>) -> bool {
    std::ptr::eq(Rc::as_ptr(a).cast::<()>(), Rc::as_ptr(b).cast::<()>())
}

fn as_notebook_match(match_: &Rc<dyn ISearchTreeMatch>) -> Rc<dyn IMatchInNotebook> {
    match_.as_match_in_notebook().expect("o casamento é um MatchInNotebook")
}

fn assert_ok(promise: &Promise<ISearchComplete>) {
    assert!(matches!(promise.result(), Some(Ok(_))), "a busca terminou sem erro");
}

// ---------------------------------------------------------------------------------------------------------
// Casos
// ---------------------------------------------------------------------------------------------------------

#[test]
fn search_model_search_adds_to_results() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let results = vec![
        a_raw_match("/1", vec![text_match("preview 1", OneLineRange::new(1, 1, 4)), text_match("preview 1", OneLineRange::new(1, 4, 11))]),
        a_raw_match("/2", vec![text_match("preview 2", line_one_range())]),
    ];
    let test_object = t.create_model(
        &guard,
        TestSearchService::with_results(results.clone(), Some(search_complete(Some(false), results, None))),
        TestNotebookSearchService::new(Vec::new(), None, &guard),
        RecordingTelemetryService::new(),
    );
    let search = test_object.search(text_query("somestring", None), None, None);
    runtime::run_microtasks();
    assert_ok(&search.async_results);

    let actual = matches_of(&test_object);

    assert_eq!(2, actual.len());
    assert_eq!(uri_file(&format!("{}/1", get_root_name())).to_string(false), actual[0].resource().to_string(false));

    let mut actua_matches = actual[0].matches();
    assert_eq!(2, actua_matches.len());
    assert_eq!(actua_matches[0].text(), "preview 1");
    assert_eq!(Range::new(2, 2, 2, 5), actua_matches[0].range());
    assert_eq!(actua_matches[1].text(), "preview 1");
    assert_eq!(Range::new(2, 5, 2, 12), actua_matches[1].range());

    actua_matches = actual[1].matches();
    assert_eq!(1, actua_matches.len());
    assert_eq!(actua_matches[0].text(), "preview 2");
    assert_eq!(Range::new(2, 1, 2, 2), actua_matches[0].range());
}

#[test]
fn search_model_search_can_return_notebook_results() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let results = vec![
        a_raw_match("/2", vec![text_match("test", OneLineRange::new(1, 1, 5)), text_match("this is a test", OneLineRange::new(1, 11, 15))]),
        a_raw_match("/3", vec![text_match("test", line_one_range())]),
    ];

    let md_input_cell = FakeCellViewModel::new("mdInputCell", CellKind::Markup, "# Test");
    let find_match_mds = vec![FindMatch { range: Range::new(1, 3, 1, 7), matches: Some(vec![Some("Test".into())]) }];

    let code_cell = FakeCellViewModel::new("codeCell", CellKind::Code, "print(\"test! testing!!\")");
    let find_match_code_cells = vec![
        FindMatch { range: Range::new(1, 8, 1, 12), matches: Some(vec![Some("test".into())]) },
        FindMatch { range: Range::new(1, 14, 1, 18), matches: Some(vec![Some("test".into())]) },
    ];
    let webview_matches = vec![
        CellWebviewFindMatch {
            index: 0,
            search_preview_info: Some(SearchPreviewInfo { line: "test! testing!!".into(), range: SearchPreviewRange { start: 1, end: 5 } }),
        },
        CellWebviewFindMatch {
            index: 1,
            search_preview_info: Some(SearchPreviewInfo { line: "test! testing!!".into(), range: SearchPreviewRange { start: 7, end: 11 } }),
        },
    ];
    let cell_match_md = INotebookCellMatchWithModel {
        index: 0,
        content_results: content_matches_to_text_search_matches(&find_match_mds, &md_input_cell),
        webview_results: Vec::new(),
        cell: md_input_cell,
    };
    let cell_match_code = INotebookCellMatchWithModel {
        index: 1,
        content_results: content_matches_to_text_search_matches(&find_match_code_cells, &code_cell),
        webview_results: webview_matches_to_text_search_matches(&webview_matches),
        cell: code_cell,
    };

    let notebook_search_service =
        TestNotebookSearchService::new(vec![a_raw_match_with_cells("/1", vec![cell_match_md, cell_match_code])], None, &guard);
    let model = t.create_model(
        &guard,
        TestSearchService::with_results(results.clone(), Some(search_complete(Some(false), results, None))),
        notebook_search_service.clone(),
        RecordingTelemetryService::new(),
    );
    let search = model.search(text_query("test", None), None, None);
    runtime::run_microtasks();
    assert_ok(&search.async_results);
    let actual = matches_of(&model);

    // `assert(notebookSearch.calledOnce)`
    assert_eq!(1, notebook_search_service.call_count.get());

    assert_eq!(3, actual.len());
    assert_eq!(uri_file(&format!("{}/1", get_root_name())).to_string(false), actual[0].resource().to_string(false));
    let notebook_file_matches = actual[0].matches();

    assert_eq!(Range::new(1, 3, 1, 7), notebook_file_matches[0].range());
    assert_eq!(Range::new(1, 8, 1, 12), notebook_file_matches[1].range());
    assert_eq!(Range::new(1, 14, 1, 18), notebook_file_matches[2].range());
    assert_eq!(Range::new(1, 2, 1, 6), notebook_file_matches[3].range());
    assert_eq!(Range::new(1, 8, 1, 12), notebook_file_matches[4].range());

    // `notebookFileMatches.forEach(match => match instanceof MatchInNotebook)` (a expressão não é conferida no original)
    let in_notebook: Vec<Rc<dyn IMatchInNotebook>> = notebook_file_matches.iter().map(as_notebook_match).collect();
    let cell_id = |index: usize| in_notebook[index].cell().map(|cell| cell.id());
    assert!(cell_id(0).as_deref() == Some("mdInputCell"));
    assert!(cell_id(1).as_deref() == Some("codeCell"));
    assert!(cell_id(2).as_deref() == Some("codeCell"));
    assert!(cell_id(3).as_deref() == Some("codeCell"));
    assert!(cell_id(4).as_deref() == Some("codeCell"));

    let md_cell_match_processed = in_notebook[0].cell_parent();
    let code_cell_match_processed = in_notebook[1].cell_parent();

    assert!(md_cell_match_processed.content_matches().len() == 1);
    assert!(code_cell_match_processed.content_matches().len() == 2);
    assert!(code_cell_match_processed.webview_matches().len() == 2);

    assert!(same_object(&md_cell_match_processed.content_matches()[0], &notebook_file_matches[0]));
    assert!(same_object(&code_cell_match_processed.content_matches()[0], &notebook_file_matches[1]));
    assert!(same_object(&code_cell_match_processed.content_matches()[1], &notebook_file_matches[2]));
    assert!(same_object(&code_cell_match_processed.webview_matches()[0], &notebook_file_matches[3]));
    assert!(same_object(&code_cell_match_processed.webview_matches()[1], &notebook_file_matches[4]));

    assert_eq!(uri_file(&format!("{}/2", get_root_name())).to_string(false), actual[1].resource().to_string(false));
    assert_eq!(uri_file(&format!("{}/3", get_root_name())).to_string(false), actual[2].resource().to_string(false));
}

#[test]
fn search_model_search_reports_telemetry_on_search_completed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let target = RecordingTelemetryService::new();
    let results = vec![
        a_raw_match("/1", vec![text_match("preview 1", OneLineRange::new(1, 1, 4)), text_match("preview 1", OneLineRange::new(1, 4, 11))]),
        a_raw_match("/2", vec![text_match("preview 2", line_one_range())]),
    ];
    let test_object = t.create_model(
        &guard,
        TestSearchService::with_results(results.clone(), Some(search_complete(Some(false), results, None))),
        TestNotebookSearchService::new(Vec::new(), None, &guard),
        target.clone(),
    );
    let search = test_object.search(text_query("somestring", None), None, None);
    runtime::run_microtasks();
    assert_ok(&search.async_results);

    // `assert.ok(target.calledThrice)`
    assert_eq!(3, target.events().len());
    assert!(target.called_with("searchResultsFirstRender"));
    assert!(target.called_with("searchResultsFinished"));
}

#[test]
fn search_model_search_reports_timed_telemetry_on_search_when_progress_is_not_called() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let target1 = RecordingTelemetryService::new();

    let test_object = t.create_model(
        &guard,
        TestSearchService::with_results(Vec::new(), Some(search_complete(Some(false), Vec::new(), None))),
        TestNotebookSearchService::new(Vec::new(), None, &guard),
        target1.clone(),
    );
    let result = test_object.search(text_query("somestring", None), None, None).async_results;

    // `return result.then(() => timeout(1).then(() => { ... }))`
    runtime::run_until_idle();
    assert_ok(&result);
    assert!(target1.called_with("searchResultsFirstRender"));
    assert!(target1.called_with("searchResultsFinished"));
}

#[test]
fn search_model_search_reports_timed_telemetry_on_search_when_progress_is_called() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let target1 = RecordingTelemetryService::new();

    let test_object = t.create_model(
        &guard,
        TestSearchService::with_results(
            vec![a_raw_match("/1", vec![text_match("some preview", line_one_range())])],
            Some(search_complete(None, Vec::new(), Some(test_search_stats()))),
        ),
        TestNotebookSearchService::new(Vec::new(), None, &guard),
        target1.clone(),
    );
    let result = test_object.search(text_query("somestring", None), None, None).async_results;

    // `return result.then(() => timeout(1).then(() => { ... }))`
    runtime::run_until_idle();
    assert_ok(&result);
    // timeout because promise handlers may run in a different order. We only care that these are fired at some point.
    assert!(target1.called_with("searchResultsFirstRender"));
    assert!(target1.called_with("searchResultsFinished"));
}

#[test]
fn search_model_search_reports_timed_telemetry_on_search_when_error_is_called() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let target1 = RecordingTelemetryService::new();

    let test_object = t.create_model(
        &guard,
        TestSearchService::with_error(Error::new("This error should be thrown by this test.")),
        TestNotebookSearchService::new(Vec::new(), None, &guard),
        target1.clone(),
    );
    let result = test_object.search(text_query("somestring", None), None, None).async_results;

    // `return result.then(() => { }, () => timeout(1).then(() => { ... }))`
    runtime::run_until_idle();
    assert!(matches!(result.result(), Some(Err(_))), "a busca rejeita com o erro do serviço");
    assert!(target1.called_with("searchResultsFirstRender"));
    assert!(target1.called_with("searchResultsFinished"));
}

#[test]
fn search_model_search_reports_timed_telemetry_on_search_when_error_is_cancelled_error() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let target1 = RecordingTelemetryService::new();

    let deferred_promise = DeferredPromise::<ISearchComplete>::new();

    let test_object = t.create_model(
        &guard,
        TestSearchService::with_deferred_promise(deferred_promise.promise.clone()),
        TestNotebookSearchService::new(Vec::new(), None, &guard),
        target1.clone(),
    );
    let result = test_object.search(text_query("somestring", None), None, None).async_results;

    let _ = deferred_promise.cancel();

    // `return result.then(() => { }, async () => timeout(1).then(() => { ... }))`
    runtime::run_until_idle();
    assert!(matches!(result.result(), Some(Err(_))), "a busca rejeita com o erro de cancelamento");
    assert!(target1.called_with("searchResultsFirstRender"));
    assert!(target1.called_with("searchResultsFinished"));
}

#[test]
fn search_model_search_results_are_cleared_during_search() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let results = vec![
        a_raw_match("/1", vec![text_match("preview 1", OneLineRange::new(1, 1, 4)), text_match("preview 1", OneLineRange::new(1, 4, 11))]),
        a_raw_match("/2", vec![text_match("preview 2", line_one_range())]),
    ];
    let test_object = t.create_model(
        &guard,
        TestSearchService::with_results(results, Some(search_complete(Some(false), Vec::new(), None))),
        TestNotebookSearchService::new(Vec::new(), None, &guard),
        RecordingTelemetryService::new(),
    );
    let search = test_object.search(text_query("somestring", None), None, None);
    runtime::run_microtasks();
    assert_ok(&search.async_results);
    assert!(!test_object.search_result().is_empty());

    // O `instantiationService.stub(ISearchService, searchServiceWithResults([]))` do original não alcança o modelo já
    // criado: a segunda busca usa o mesmo serviço. O resultado esvazia porque a nova consulta troca as pastas raiz.
    let _ = test_object.search(text_query("somestring", None), None, None);
    assert!(test_object.search_result().is_empty());
}

#[test]
fn search_model_previous_search_is_cancelled_when_new_search_is_called() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let token_source = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
    let test_object = t.create_model(
        &guard,
        TestSearchService::cancelable(&token_source, &guard),
        TestNotebookSearchService::new(Vec::new(), Some(&token_source), &guard),
        RecordingTelemetryService::new(),
    );
    let _ = test_object.search(text_query("somestring", None), None, None);
    // Os `stub` de `ISearchService` e `INotebookSearchService` que o original faz aqui não alcançam o modelo já criado.
    let _ = test_object.search(text_query("somestring", None), None, None);

    assert!(token_source.token().is_cancellation_requested());
}

#[test]
fn get_replace_string_returns_proper_replace_string_for_reg_expressions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let results = vec![a_raw_match(
        "/1",
        vec![text_match("preview 1", OneLineRange::new(1, 1, 4)), text_match("preview 1", OneLineRange::new(1, 4, 11))],
    )];
    let test_object = t.create_model(
        &guard,
        TestSearchService::with_results(results.clone(), Some(search_complete(Some(false), results, None))),
        TestNotebookSearchService::new(Vec::new(), None, &guard),
        RecordingTelemetryService::new(),
    );
    let search = test_object.search(text_query("re", None), None, None);
    runtime::run_microtasks();
    assert_ok(&search.async_results);
    test_object.set_replace_string("hello");
    let mut match_ = matches_of(&test_object)[0].matches()[0].clone();
    assert_eq!(match_.replace_string(), "hello");

    let search = test_object.search(text_query("re", Some(true)), None, None);
    runtime::run_microtasks();
    assert_ok(&search.async_results);
    match_ = matches_of(&test_object)[0].matches()[0].clone();
    assert_eq!(match_.replace_string(), "hello");

    let search = test_object.search(text_query("re(?:vi)", Some(true)), None, None);
    runtime::run_microtasks();
    assert_ok(&search.async_results);
    match_ = matches_of(&test_object)[0].matches()[0].clone();
    assert_eq!(match_.replace_string(), "hello");

    let search = test_object.search(text_query("r(e)(?:vi)", Some(true)), None, None);
    runtime::run_microtasks();
    assert_ok(&search.async_results);
    match_ = matches_of(&test_object)[0].matches()[0].clone();
    assert_eq!(match_.replace_string(), "hello");

    let search = test_object.search(text_query("r(e)(?:vi)", Some(true)), None, None);
    runtime::run_microtasks();
    assert_ok(&search.async_results);
    test_object.set_replace_string("hello$1");
    match_ = matches_of(&test_object)[0].matches()[0].clone();
    assert_eq!(match_.replace_string(), "helloe");
}
