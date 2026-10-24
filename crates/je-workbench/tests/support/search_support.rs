//! A bancada de `searchResult.test.ts` e `searchViewlet.test.ts`: `searchTestCommon.ts` (`getRootName`,
//! `createFileUriFromPathFromRoot`, `stubModelService` e `stubNotebookEditorService`), o `setup` com os serviços das duas
//! suítes e os auxiliares `aSearchResult`, `aFileMatch`, `aFolderMatch` e `aRawMatch`.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O guarda de vazamentos (`ensureNoDisposablesAreLeakedInTestSuite`) é o [`LeakGuard`] criado no
//!   começo de cada caso. O `TestInstantiationService` do original é o [`InstantiationService`] sobre um [`ServiceCollection`] com os
//!   mesmos serviços: telemetria nula, `ModelService` real (com `search.searchOnType` ligado), serviço de editores de
//!   notebook, `UriIdentityService` real sobre um `FileService`, `IReplaceService` (dublê), `MockLabelService` e log nulo. O
//!   original deixa `ISearchService` e `INotebookSearchService` sem registro (`undefined`); aqui eles são dublês que
//!   falham se usados (nenhum caso os usa), porque o contrato do porte os exige. O `IThemeService` do `stubModelService` não
//!   existe no `ModelService` do porte, e o `NotebookEditorWidgetService` real (que exige os grupos de editores, o contexto e
//!   o serviço de editores) é um dublê de `INotebookEditorService` que não tem editores abertos, que é o que o real devolve
//!   sem editores.
//! - **`aFolderMatch`.** O `FolderMatchImpl` do porte lê o resultado de busca pela cadeia de pais (o `_searchResult` do original
//!   é o `searchModel.searchResult` de um modelo à parte, que o porte não recebe); o modelo à parte continua sendo criado e
//!   guardado no `LeakGuard`, como no original.
//! - **`aFileMatch` com `parentFolder`.** O `aFileMatch` da suíte `Search - Viewlet` recebe a pasta pai; o da suíte
//!   `SearchResult` recebe o resultado de busca e usa a pasta raiz dele. Os dois são [`Fixture::a_file_match_in_folder`].
//! - **`isWindows`.** `getRootName` e `createFileUriFromPathFromRoot` seguem a plataforma.

#[path = "../../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

pub use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::DisposableRef;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model_service::ModelService;
use je_editor::services_model::{I_MODEL_SERVICE, IModelService};
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileService;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::{ILabelService, I_LABEL_SERVICE};
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_platform::resources::uri_file;
use je_platform::telemetry::null_telemetry_service;
use je_platform::telemetry::telemetry::I_TELEMETRY_SERVICE;
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use je_workbench::contrib::notebook::browser::services::notebook_editor_service::{I_NOTEBOOK_EDITOR_SERVICE, IBorrowValue, INotebookEditorService};
use je_workbench::contrib::search::common::notebook_search::{I_NOTEBOOK_SEARCH_SERVICE, INotebookSearchService, NotebookSearchResult};
use je_workbench::contrib::search::notebook_search::notebook_search_model::{NotebookCompatibleFileMatch, NotebookCompatibleFileMatchArgs};
use je_workbench::contrib::search::notebook_search::notebook_search_model_base::INotebookFileInstanceMatch;
use je_workbench::contrib::search::replace::{I_REPLACE_SERVICE, IReplaceService};
use je_workbench::contrib::search::search_tree_model::folder_match::{FolderMatchArgs, FolderMatchImpl, FolderMatchKind};
use je_workbench::contrib::search::search_tree_model::search_model::SearchModelImpl;
use je_workbench::contrib::search::search_tree_model::search_tree_common::{
    FileMatchOrMatch, FolderMatchParent, ISearchResult, ISearchTreeFileMatch, ISearchTreeFolderMatch, ISearchTreeMatch, ITextSearchHeading,
};
use je_workbench::services::search::search::{
    I_SEARCH_SERVICE, IAITextQuery, IFileMatch, IFileQuery, IFolderQuery, ISearchComplete, ISearchResultProvider, ISearchService, ITextQuery,
    ITextQueryProps, ITextSearchResult, PatternInfo, SearchProgressCallback, SearchProviderType, SearchRange, SharedResourceSet, TextSearchMatch,
    TextSearchSplitSyncAsync,
};
use je_workbench::services::textfile::collaborators::{IProgress, ProgressStep};
use je_workbench::test::browser::workbench_test_services::MockLabelService;
use je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService;

// ---------------------------------------------------------------------------------------------------------
// searchTestCommon.ts
// ---------------------------------------------------------------------------------------------------------

pub fn is_windows() -> bool {
    Platform::detect().is_windows
}

/// `getRootName()`.
pub fn get_root_name() -> &'static str {
    if is_windows() { "c:" } else { "" }
}

/// `createFileUriFromPathFromRoot(path?)`.
pub fn create_file_uri_from_path_from_root(path: Option<&str>) -> URI {
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

pub fn unused(service: &str) -> ! {
    panic!("{service} não é usado por este teste (o original deixa o serviço sem registro)")
}

/// `ISearchService` que o original deixa sem registro.
struct UnusedSearchService;

impl ISearchService for UnusedSearchService {
    fn text_search(&self, _query: &ITextQuery, _token: Option<CancellationToken>, _on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
        unused("ISearchService")
    }

    fn ai_text_search(&self, _query: &IAITextQuery, _token: Option<CancellationToken>, _on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete> {
        unused("ISearchService")
    }

    fn get_ai_name(&self) -> Promise<Option<String>> {
        unused("ISearchService")
    }

    fn text_search_split_sync_async(
        &self,
        _query: &ITextQuery,
        _token: Option<CancellationToken>,
        _on_progress: Option<SearchProgressCallback>,
        _notebook_files_to_ignore: Option<SharedResourceSet>,
        _async_notebook_files_to_ignore: Option<Promise<SharedResourceSet>>,
    ) -> TextSearchSplitSyncAsync {
        unused("ISearchService")
    }

    fn file_search(&self, _query: &IFileQuery, _token: Option<CancellationToken>) -> Promise<ISearchComplete> {
        unused("ISearchService")
    }

    fn scheme_has_file_search_provider(&self, _scheme: &str) -> bool {
        unused("ISearchService")
    }

    fn clear_cache(&self, _cache_key: &str) -> Promise<()> {
        unused("ISearchService")
    }

    fn register_search_result_provider(&self, _scheme: &str, _provider_type: SearchProviderType, _provider: Rc<dyn ISearchResultProvider>) -> DisposableRef {
        unused("ISearchService")
    }
}

/// `INotebookSearchService` que o original deixa sem registro.
struct UnusedNotebookSearchService;

impl INotebookSearchService for UnusedNotebookSearchService {
    fn notebook_search(
        &self,
        _query: &ITextQuery,
        _token: Option<CancellationToken>,
        _search_instance_id: &str,
        _on_progress: Option<SearchProgressCallback>,
    ) -> NotebookSearchResult {
        unused("INotebookSearchService")
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

/// O argumento de `IReplaceService.replace`: um casamento ou uma lista de arquivos.
pub enum ReplaceCall {
    Match(Rc<dyn ISearchTreeMatch>),
    Files(Vec<Rc<dyn ISearchTreeFileMatch>>),
}

type ReplaceHandler = Rc<dyn Fn(ReplaceCall) -> Promise<()>>;

/// `IReplaceService` do teste: `replace` devolve o que o handler devolve (padrão `Promise.resolve(null)`).
pub struct TestReplaceService {
    handler: RefCell<ReplaceHandler>,
}

impl TestReplaceService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { handler: RefCell::new(Rc::new(|_| Promise::resolved(()))) })
    }

    /// `instantiationService.stub(IReplaceService, 'replace', handler)`.
    pub fn stub_replace(&self, handler: impl Fn(ReplaceCall) -> Promise<()> + 'static) {
        *self.handler.borrow_mut() = Rc::new(handler);
    }

    fn call(&self, call: ReplaceCall) -> Promise<()> {
        let handler = self.handler.borrow().clone();
        handler(call)
    }
}

impl IReplaceService for TestReplaceService {
    fn replace_match(&self, match_: &Rc<dyn ISearchTreeMatch>) -> Promise<()> {
        self.call(ReplaceCall::Match(match_.clone()))
    }

    fn replace_files(&self, files: &[Rc<dyn ISearchTreeFileMatch>], _progress: Option<&dyn IProgress<ProgressStep>>) -> Promise<()> {
        self.call(ReplaceCall::Files(files.to_vec()))
    }

    fn open_replace_preview(&self, _element: &FileMatchOrMatch, _preserve_focus: Option<bool>, _side_by_side: Option<bool>, _pinned: Option<bool>) -> Promise<()> {
        unused("IReplaceService.openReplacePreview")
    }

    fn update_replace_preview(&self, _file: &Rc<dyn ISearchTreeFileMatch>, _override: Option<bool>) -> Promise<()> {
        unused("IReplaceService.updateReplacePreview")
    }
}

// ---------------------------------------------------------------------------------------------------------
// setup
// ---------------------------------------------------------------------------------------------------------

pub struct Fixture {
    pub instantiation_service: Rc<InstantiationService>,
    pub replace_service: Rc<TestReplaceService>,
}

pub fn setup(guard: &LeakGuard) -> Fixture {
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

    let replace_service = TestReplaceService::new();
    let replace_service_dyn: Rc<dyn IReplaceService> = replace_service.clone();
    let label_service: Rc<dyn ILabelService> = MockLabelService::new(None);
    let search_service: Rc<dyn ISearchService> = Rc::new(UnusedSearchService);
    let notebook_search_service: Rc<dyn INotebookSearchService> = Rc::new(UnusedNotebookSearchService);

    let services = ServiceCollection::new()
        .with(I_TELEMETRY_SERVICE, null_telemetry_service())
        .with(I_MODEL_SERVICE, model_service)
        .with(I_NOTEBOOK_EDITOR_SERVICE, notebook_editor_service)
        .with(I_URI_IDENTITY_SERVICE, uri_identity_service)
        .with(I_REPLACE_SERVICE, replace_service_dyn)
        .with(I_LABEL_SERVICE, label_service)
        .with(I_LOG_SERVICE, NullLogService::shared())
        .with(I_CONFIGURATION_SERVICE, configuration_service)
        .with(I_SEARCH_SERVICE, search_service)
        .with(I_NOTEBOOK_SEARCH_SERVICE, notebook_search_service);
    Fixture { instantiation_service: InstantiationService::new(services), replace_service }
}

// ---------------------------------------------------------------------------------------------------------
// Auxiliares do original
// ---------------------------------------------------------------------------------------------------------

pub fn text_match(text: &str, range: SearchRange) -> TextSearchMatch {
    TextSearchMatch::new(text, &[range], None, None)
}

/// `aRawMatch(resource, ...results)`.
pub fn a_raw_match(resource: &str, results: Vec<TextSearchMatch>) -> IFileMatch {
    IFileMatch::new(create_file_uri_from_path_from_root(Some(resource)), Some(results.into_iter().map(ITextSearchResult::Match).collect()))
}

/// A consulta de `aSearchResult`, `aFolderMatch` e do `Data Source`: a pasta raiz do sistema e o padrão `pattern`.
pub fn root_query_with_pattern(pattern: &str) -> ITextQuery {
    ITextQueryProps::new(vec![IFolderQuery::new(create_file_uri_from_path_from_root(None))], PatternInfo::new(pattern))
}

/// A consulta de `aSearchResult`: a pasta raiz do sistema e o padrão vazio.
pub fn root_query() -> ITextQuery {
    root_query_with_pattern("")
}

impl Fixture {
    /// `aSearchResult()`.
    pub fn a_search_result(&self, guard: &LeakGuard) -> Rc<dyn ISearchResult> {
        let search_model = self.instantiation_service.create_instance::<SearchModelImpl>(());
        guard.add_rc(search_model.clone());
        search_model.search_result().set_query(Some(root_query()));
        search_model.search_result()
    }

    /// `aFileMatch(path, parentFolder, ...lineMatches)`, com o `parentFolder` dado.
    pub fn a_file_match_in_folder(
        &self,
        guard: &LeakGuard,
        path: &str,
        parent_folder: Rc<dyn ISearchTreeFolderMatch>,
        line_matches: Vec<TextSearchMatch>,
    ) -> Rc<dyn INotebookFileInstanceMatch> {
        let raw_match = IFileMatch::new(uri_file(&format!("/{path}")), Some(line_matches.into_iter().map(ITextSearchResult::Match).collect()));
        let file_match = self.instantiation_service.create_instance::<NotebookCompatibleFileMatch>(NotebookCompatibleFileMatchArgs {
            query: PatternInfo::new(""),
            preview_options: None,
            max_results: None,
            parent: parent_folder,
            raw_match,
            closest_root: None,
            search_instance_id: String::new(),
        });
        file_match.create_matches();

        guard.add_rc(file_match.clone());
        file_match
    }

    /// `aFileMatch(path, searchResult, ...lineMatches)` da suíte `SearchResult`: o pai é a pasta raiz do resultado.
    pub fn a_file_match(
        &self,
        guard: &LeakGuard,
        path: &str,
        search_result: Option<Rc<dyn ISearchResult>>,
        line_matches: Vec<TextSearchMatch>,
    ) -> Rc<dyn INotebookFileInstanceMatch> {
        let search_result = search_result.unwrap_or_else(|| self.a_search_result(guard));
        let root = search_result.folder_matches(false)[0].clone();
        self.a_file_match_in_folder(guard, path, root, line_matches)
    }

    /// `aFolderMatch(path, index, parent?)`: o pai padrão é a primeira pasta de um novo resultado de busca.
    pub fn a_folder_match(&self, guard: &LeakGuard, path: &str, index: i32, parent: Option<Rc<dyn ITextSearchHeading>>) -> Rc<dyn ISearchTreeFolderMatch> {
        let search_model = self.instantiation_service.create_instance::<SearchModelImpl>(());
        guard.add_rc(search_model);
        let parent = match parent {
            Some(heading) => FolderMatchParent::Heading(heading),
            None => FolderMatchParent::FolderMatch(self.a_search_result(guard).folder_matches(false)[0].clone()),
        };
        let folder_match = self.instantiation_service.create_instance::<FolderMatchImpl>(FolderMatchArgs {
            kind: FolderMatchKind::WithResource,
            resource: Some(create_file_uri_from_path_from_root(Some(path))),
            id: path.to_string(),
            index,
            query: root_query(),
            parent,
            closest_root: None,
        });
        guard.add_rc(folder_match.clone());
        folder_match
    }
}
