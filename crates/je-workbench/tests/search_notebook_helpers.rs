//! Porte de `src/vs/workbench/contrib/search/test/browser/searchNotebookHelpers.test.ts` (suíte `searchNotebookHelpers`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`setup` e `teardown`.** O guarda de vazamentos (`ensureNoDisposablesAreLeakedInTestSuite`) é o [`LeakGuard`] criado no
//!   começo de cada caso. O `TestInstantiationService` é um [`InstantiationService`] sobre um [`ServiceCollection`] com os mesmos
//!   serviços que o `searchResult.test.ts` usa (`ModelService` real com `search.searchOnType` ligado, `UriIdentityService` real,
//!   `MockLabelService`, telemetria nula e log nulo). O `NotebookEditorWidgetService` real (que exige os grupos de editores, o
//!   contexto e o serviço de editores) é um dublê de `INotebookEditorService` sem editores abertos, o que o real devolve sem
//!   editores. `ISearchService`, `INotebookSearchService` e `IReplaceService` não têm registro no original; aqui são dublês que
//!   falham se usados (nenhum caso os usa), porque o contrato do porte os exige.
//! - **Estado entre os casos.** O segundo caso do original lê `markdownContentResults`, `codeContentResults` e
//!   `codeWebviewResults`, variáveis da suíte que o primeiro caso preenche. Como cada `#[test]` roda isolado, o segundo caso as
//!   calcula com as mesmas funções (`contentMatchesToTextSearchMatches` e `webviewMatchesToTextSearchMatches`) sobre os mesmos
//!   casamentos antes de converter.
//! - **`{ id, cellKind, textBuffer } as ICellViewModel`.** É o [`FakeCellViewModel`] sobre um buffer real com o texto das
//!   linhas que o `getLineContent` falso devolve (as demais linhas são vazias e nunca são lidas).
//! - **`CellFindMatchModel`.** É o `CellFindMatchWithIndex` do porte (célula, índice, casamentos de conteúdo e de webview),
//!   de onde o caso lê `contentMatches` e `webviewMatches`.
//! - **`FolderMatchImpl` (a classe base).** Não é instanciada no porte; o `createInstance(FolderMatchImpl, URI.file('somepath'),
//!   '', 0, query, plainTextSearchResult, searchResult, null)` é o [`FolderMatchArgs::workspace_root`] (recurso, sem raiz mais
//!   próxima, pai o resultado de texto simples).
//! - **`assertRangesEqual`.** Confere o tamanho e os quatro campos de cada intervalo; um `Range` do editor é comparado inteiro.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::DisposableRef;
use je_base::platform::Platform;
use je_base::promise::Promise;
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
use je_platform::telemetry::null_telemetry_service;
use je_platform::telemetry::telemetry::I_TELEMETRY_SERVICE;
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::notebook::browser::notebook_browser::{
    CellEditState, CellFindMatchWithIndex, CellWebviewFindMatch, ICellViewModel, SearchPreviewInfo, SearchPreviewRange,
};
use je_workbench::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use je_workbench::contrib::notebook::browser::services::notebook_editor_service::{I_NOTEBOOK_EDITOR_SERVICE, IBorrowValue, INotebookEditorService};
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, TextBufferRef};
use je_workbench::contrib::search::common::notebook_search::{I_NOTEBOOK_SEARCH_SERVICE, INotebookSearchService, NotebookSearchResult};
use je_workbench::contrib::search::notebook_search::notebook_search_model::{
    CellMatch, NotebookCompatibleFileMatch, NotebookCompatibleFileMatchArgs, text_search_matches_to_notebook_matches,
};
use je_workbench::contrib::search::notebook_search::notebook_search_model_base::{ICellMatch, IMatchInNotebook, INotebookFileInstanceMatch};
use je_workbench::contrib::search::notebook_search::search_notebook_helpers::{
    content_matches_to_text_search_matches, webview_matches_to_text_search_matches,
};
use je_workbench::contrib::search::replace::{I_REPLACE_SERVICE, IReplaceService};
use je_workbench::contrib::search::search_tree_model::folder_match::{FolderMatchArgs, FolderMatchImpl};
use je_workbench::contrib::search::search_tree_model::search_model::SearchModelImpl;
use je_workbench::contrib::search::search_tree_model::search_tree_common::{
    FileMatchOrMatch, ISearchModel, ISearchTreeFileMatch, ISearchTreeMatch,
};
use je_workbench::services::search::search::{
    I_SEARCH_SERVICE, IAITextQuery, IFileMatch, IFileQuery, IFolderQuery, ISearchComplete, ISearchRange, ISearchResultProvider, ISearchService,
    ITextQuery, ITextQueryProps, PatternInfo, SearchProgressCallback, SearchProviderType, SharedResourceSet, TextSearchMatch, TextSearchSplitSyncAsync,
};
use je_workbench::services::textfile::collaborators::{IProgress, ProgressStep};
use je_workbench::test::browser::workbench_test_services::MockLabelService;
use je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService;

// ---------------------------------------------------------------------------------------------------------
// searchTestCommon.ts
// ---------------------------------------------------------------------------------------------------------

fn is_windows() -> bool {
    Platform::detect().is_windows
}

/// `createFileUriFromPathFromRoot()`: a raiz do sistema de arquivos.
fn create_file_uri_from_root() -> URI {
    if is_windows() { uri_file("c:/") } else { uri_file("") }
}

// ---------------------------------------------------------------------------------------------------------
// Dublês dos serviços
// ---------------------------------------------------------------------------------------------------------

fn unused(service: &str) -> ! {
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

/// `{ id, cellKind, textBuffer } as ICellViewModel`.
struct FakeCellViewModel {
    id: String,
    cell_kind: CellKind,
    text_buffer: TextBufferRef,
}

impl FakeCellViewModel {
    fn new(id: &str, cell_kind: CellKind, text: &str) -> Rc<dyn ICellViewModel> {
        let text_buffer: TextBufferRef = Rc::new(RefCell::new(create_text_buffer(text.into(), DefaultEndOfLine::Lf)));
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
        unused("ICellViewModel.resolveTextModel")
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

// ---------------------------------------------------------------------------------------------------------
// setup
// ---------------------------------------------------------------------------------------------------------

struct Fixture {
    instantiation_service: Rc<InstantiationService>,
    counter: Cell<u32>,
    md_input_cell: Rc<dyn ICellViewModel>,
    code_cell: Rc<dyn ICellViewModel>,
    md_cell_find_match: CellFindMatchWithIndex,
    code_cell_find_match: CellFindMatchWithIndex,
}

fn find_match(range: Range, text: &str) -> FindMatch {
    FindMatch { range, matches: Some(vec![Some(text.into())]) }
}

fn webview_match(index: usize, line: &str, start: i64, end: i64) -> CellWebviewFindMatch {
    CellWebviewFindMatch { index, search_preview_info: Some(SearchPreviewInfo { line: JsString::from(line), range: SearchPreviewRange { start, end } }) }
}

fn setup(guard: &LeakGuard) -> Fixture {
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

    let on_did_add_notebook_editor = Emitter::<Rc<dyn NotebookEditorWidget>>::new();
    guard.add_rc(on_did_add_notebook_editor.clone());
    let notebook_editor_service: Rc<dyn INotebookEditorService> = Rc::new(NoNotebookEditorService { on_did_add_notebook_editor });

    let file_service = guard.add_rc(FileService::new(NullLogService::shared()));
    let file_service: Rc<dyn IFileService> = file_service;
    let uri_identity_service = guard.add_rc(UriIdentityService::create((), (file_service,)));
    let uri_identity_service: Rc<dyn IUriIdentityService> = uri_identity_service;

    let replace_service: Rc<dyn IReplaceService> = Rc::new(UnusedReplaceService);
    let label_service: Rc<dyn ILabelService> = MockLabelService::new(None);
    let search_service: Rc<dyn ISearchService> = Rc::new(UnusedSearchService);
    let notebook_search_service: Rc<dyn INotebookSearchService> = Rc::new(UnusedNotebookSearchService);
    let services = ServiceCollection::new()
        .with(I_TELEMETRY_SERVICE, null_telemetry_service())
        .with(I_MODEL_SERVICE, model_service)
        .with(I_NOTEBOOK_EDITOR_SERVICE, notebook_editor_service)
        .with(I_URI_IDENTITY_SERVICE, uri_identity_service)
        .with(I_REPLACE_SERVICE, replace_service)
        .with(I_LABEL_SERVICE, label_service)
        .with(I_LOG_SERVICE, NullLogService::shared())
        .with(I_CONFIGURATION_SERVICE, configuration_service)
        .with(I_SEARCH_SERVICE, search_service)
        .with(I_NOTEBOOK_SEARCH_SERVICE, notebook_search_service);
    let instantiation_service = InstantiationService::new(services);

    let md_input_cell = FakeCellViewModel::new("mdCell", CellKind::Markup, "# Hello World Test");
    let find_match_mds = vec![find_match(Range::new(1, 15, 1, 19), "Test")];
    let code_cell = FakeCellViewModel::new("codeCell", CellKind::Code, "print(\"test! testing!!\")\nprint(\"this is a Test\")");
    let find_match_code_cells = vec![
        find_match(Range::new(1, 8, 1, 12), "test"),
        find_match(Range::new(1, 14, 1, 18), "test"),
        find_match(Range::new(2, 18, 2, 22), "Test"),
    ];

    let webview_matches = vec![webview_match(0, "test! testing!!", 1, 5), webview_match(1, "test! testing!!", 7, 11), webview_match(3, "this is a Test", 11, 15)];

    let md_cell_find_match = CellFindMatchWithIndex { index: 0, cell: md_input_cell.clone(), content_matches: find_match_mds, webview_matches: Vec::new() };
    let code_cell_find_match = CellFindMatchWithIndex { index: 5, cell: code_cell.clone(), content_matches: find_match_code_cells, webview_matches };

    Fixture { instantiation_service, counter: Cell::new(0), md_input_cell, code_cell, md_cell_find_match, code_cell_find_match }
}

impl Fixture {
    /// `aFileMatch()`.
    fn a_file_match(&self, guard: &LeakGuard) -> Rc<dyn INotebookFileInstanceMatch> {
        self.counter.set(self.counter.get() + 1);
        let raw_match = IFileMatch::new(uri_file(&format!("somepath{}", self.counter.get())), Some(Vec::new()));

        let search_model = self.instantiation_service.create_instance::<SearchModelImpl>(());
        guard.add_rc(search_model.clone());
        let query = ITextQueryProps::new(vec![IFolderQuery::new(create_file_uri_from_root())], PatternInfo::new(""));
        let folder_match = self.instantiation_service.create_instance::<FolderMatchImpl>(FolderMatchArgs::workspace_root(
            uri_file("somepath"),
            String::new(),
            0,
            query,
            search_model.search_result().plain_text_search_result(),
        ));
        let file_match = self.instantiation_service.create_instance::<NotebookCompatibleFileMatch>(NotebookCompatibleFileMatchArgs {
            query: PatternInfo::new(""),
            preview_options: None,
            max_results: None,
            parent: folder_match.clone(),
            raw_match,
            closest_root: None,
            search_instance_id: String::new(),
        });
        file_match.create_matches();
        guard.add_rc(folder_match);
        guard.add_rc(file_match.clone());

        file_match
    }
}

/// `assertRangesEqual(actual, expected)`: o tamanho e os quatro campos de cada intervalo.
fn assert_ranges_equal(actual: &[ISearchRange], expected: &[ISearchRange]) {
    assert_eq!(actual.len(), expected.len());
    for (range, expected_range) in actual.iter().zip(expected) {
        assert_eq!(
            (range.start_line_number, range.start_column, range.end_line_number, range.end_column),
            (expected_range.start_line_number, expected_range.start_column, expected_range.end_line_number, expected_range.end_column)
        );
    }
}

fn previews(matches: &TextSearchMatch) -> Vec<ISearchRange> {
    matches.range_locations.iter().map(|location| location.preview).collect()
}

fn sources(matches: &TextSearchMatch) -> Vec<ISearchRange> {
    matches.range_locations.iter().map(|location| location.source).collect()
}

fn range(start_line_number: i64, start_column: i64, end_line_number: i64, end_column: i64) -> ISearchRange {
    ISearchRange::new(start_line_number, start_column, end_line_number, end_column)
}

/// `contentMatchesToTextSearchMatches` e `webviewMatchesToTextSearchMatches` sobre os casamentos do `setup`.
fn convert(t: &Fixture) -> (Vec<TextSearchMatch>, Vec<TextSearchMatch>, Vec<TextSearchMatch>) {
    let markdown_content_results = content_matches_to_text_search_matches(&t.md_cell_find_match.content_matches, &t.md_input_cell);
    let code_content_results = content_matches_to_text_search_matches(&t.code_cell_find_match.content_matches, &t.code_cell);
    let code_webview_results = webview_matches_to_text_search_matches(&t.code_cell_find_match.webview_matches);
    (markdown_content_results, code_content_results, code_webview_results)
}

// ---------------------------------------------------------------------------------------------------------
// notebookEditorMatchesToTextSearchResults
// ---------------------------------------------------------------------------------------------------------

#[test]
fn convert_cell_find_match_model_to_i_text_search_match_and_check_results() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let (markdown_content_results, code_content_results, code_webview_results) = convert(&t);

    assert_eq!(markdown_content_results.len(), 1);
    assert_eq!(markdown_content_results[0].preview_text, "# Hello World Test\n");
    assert_ranges_equal(&previews(&markdown_content_results[0]), &[range(0, 14, 0, 18)]);
    assert_ranges_equal(&sources(&markdown_content_results[0]), &[range(0, 14, 0, 18)]);

    assert_eq!(code_content_results.len(), 2);
    assert_eq!(code_content_results[0].preview_text, "print(\"test! testing!!\")\n");
    assert_eq!(code_content_results[1].preview_text, "print(\"this is a Test\")\n");
    assert_ranges_equal(&previews(&code_content_results[0]), &[range(0, 7, 0, 11), range(0, 13, 0, 17)]);
    assert_ranges_equal(&sources(&code_content_results[0]), &[range(0, 7, 0, 11), range(0, 13, 0, 17)]);

    assert_eq!(code_webview_results.len(), 3);
    assert_eq!(code_webview_results[0].preview_text, "test! testing!!");
    assert_eq!(code_webview_results[1].preview_text, "test! testing!!");
    assert_eq!(code_webview_results[2].preview_text, "this is a Test");

    assert_ranges_equal(&previews(&code_webview_results[0]), &[range(0, 1, 0, 5)]);
    assert_ranges_equal(&previews(&code_webview_results[1]), &[range(0, 7, 0, 11)]);
    assert_ranges_equal(&previews(&code_webview_results[2]), &[range(0, 11, 0, 15)]);
    assert_ranges_equal(&sources(&code_webview_results[0]), &[range(0, 1, 0, 5)]);
    assert_ranges_equal(&sources(&code_webview_results[1]), &[range(0, 7, 0, 11)]);
    assert_ranges_equal(&sources(&code_webview_results[2]), &[range(0, 11, 0, 15)]);
}

#[test]
fn convert_i_text_search_match_to_match_in_notebook() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = setup(&guard);
    let (markdown_content_results, code_content_results, code_webview_results) = convert(&t);

    let md_cell_match = CellMatch::new(&t.a_file_match(&guard), Some(t.md_input_cell.clone()), 0);
    let markdown_cell_content_match_objs = text_search_matches_to_notebook_matches(&markdown_content_results, &md_cell_match);

    let code_cell_match = CellMatch::new(&t.a_file_match(&guard), Some(t.code_cell.clone()), 0);
    let code_cell_content_match_objs = text_search_matches_to_notebook_matches(&code_content_results, &code_cell_match);
    let code_webview_content_match_objs = text_search_matches_to_notebook_matches(&code_webview_results, &code_cell_match);

    let cell_id = |match_: &Rc<je_workbench::contrib::search::notebook_search::notebook_search_model::MatchInNotebook>| match_.cell().map(|cell| cell.id());

    assert_eq!(cell_id(&markdown_cell_content_match_objs[0]), Some(md_cell_match.id()));
    assert_eq!(markdown_cell_content_match_objs[0].range(), Range::new(1, 15, 1, 19));

    assert_eq!(cell_id(&code_cell_content_match_objs[0]), Some(code_cell_match.id()));
    assert_eq!(cell_id(&code_cell_content_match_objs[1]), Some(code_cell_match.id()));
    assert_eq!(code_cell_content_match_objs[0].range(), Range::new(1, 8, 1, 12));
    assert_eq!(code_cell_content_match_objs[1].range(), Range::new(1, 14, 1, 18));
    assert_eq!(code_cell_content_match_objs[2].range(), Range::new(2, 18, 2, 22));

    assert_eq!(cell_id(&code_webview_content_match_objs[0]), Some(code_cell_match.id()));
    assert_eq!(cell_id(&code_webview_content_match_objs[1]), Some(code_cell_match.id()));
    assert_eq!(cell_id(&code_webview_content_match_objs[2]), Some(code_cell_match.id()));
    assert_eq!(code_webview_content_match_objs[0].range(), Range::new(1, 2, 1, 6));
    assert_eq!(code_webview_content_match_objs[1].range(), Range::new(1, 8, 1, 12));
    assert_eq!(code_webview_content_match_objs[2].range(), Range::new(1, 12, 1, 16));
}
