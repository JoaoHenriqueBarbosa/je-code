//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/notebookOutlineViewProviders.test.ts` (suíte `Notebook Outline
//! View Providers`), caso a caso e na mesma ordem. Os 10 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso; os provedores que o original
//!   registra em `store` são descartados no fim do caso.
//! - As células `createMarkupCellViewModel(...)`/`createCodeCellViewModel(...)` do original são objetos literais que fingem ser
//!   `ICellViewModel`. A camada lógica só tem o `CellViewModel` real, então o caso cria um notebook de teste com as quatro
//!   células (`# h1` e `plaintext` em markdown, `# code cell 2` e `# code cell 3` em código) e os símbolos vão por URI da
//!   célula (no original, por id do modelo de texto `$0`..`$3`). O que cada caso lê (`getText`, a primeira linha, o tipo da
//!   célula, o id para o cache) é o mesmo. A célula do `fakeRoot` é a de código (ou a de markdown nos casos de breadcrumbs).
//! - O `OutlineModelStub` (`getTopLevelSymbols`) é um `OutlineModel` real com um `OutlineElement` por símbolo; o
//!   `textModelService.createModelReference` devolve o `TextModel` que o serviço de modelos cria para a URI da célula (descartado
//!   no fim do caso); `getDebounceValue` devolve `0`.
//! - `{ name: 'var2', range: {} }` (sem `kind`) é `SymbolKind::File` (valor `0`, falso em JavaScript como o `undefined`; ver
//!   `OutlineEntry::has_symbol_kind`); `range: {}` é um intervalo vazio.
//! - `index = -1` do `fakeRoot` cabe no `i64` do `OutlineEntry`.
//! - `new TestThemeService()` com `getFileIconTheme()` é o `Fn() -> IFileIconTheme` sem ícones de arquivo (o `IThemeService` do
//!   porte não tem tema de ícones).
//! - `await entryFactory.cacheSymbols(...)` é a chamada seguida de `run_microtasks` (as promessas do porte assentam nos
//!   microtasks).
//! - `createMockOutlineDataSource` é o `MockDataSource` dentro de `Reference::immortal`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, Reference};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::language_providers::{DocumentSymbol, SymbolKind};
use je_editor::model::TextSnapshot;
use je_editor::model_service::ModelService;
use je_editor::outline_model::{IOutlineModelService, OutlineElement, OutlineModel, TreeElement};
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::contrib::notebook::browser::contrib::outline::notebook_outline::{
    IFileIconTheme, NotebookBreadcrumbsProvider, NotebookOutlinePaneProvider, NotebookQuickPickProvider, OutlinePaneParent,
};
use je_workbench::contrib::notebook::browser::view_model::notebook_outline_data_source::INotebookCellOutlineDataSource;
use je_workbench::contrib::notebook::browser::view_model::notebook_outline_entry_factory::{INotebookOutlineEntryFactory, NotebookOutlineEntryFactory};
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use je_workbench::contrib::notebook::browser::view_model::outline_entry::OutlineEntry;
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::outline::outline::{IBreadcrumbsDataSource, IQuickPickDataSource};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference};
use support::test_notebook_editor::{MockNotebookCell, empty_metadata, with_test_notebook};

// #region Setup

type SymbolsPerUri = Rc<RefCell<HashMap<String, Vec<DocumentSymbol>>>>;

fn uri_key(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `{ name, range: {}, kind }`.
fn symbol(name: &str, kind: SymbolKind) -> DocumentSymbol {
    DocumentSymbol {
        name: name.to_string(),
        detail: String::new(),
        kind,
        tags: Vec::new(),
        container_name: None,
        range: Range::new(1, 1, 1, 1),
        selection_range: Range::new(1, 1, 1, 1),
        children: None,
    }
}

/// `OutlineModelStub` + `outlineModelService`.
struct StubOutlineModelService {
    symbols: SymbolsPerUri,
}

impl IOutlineModelService for StubOutlineModelService {
    fn get_or_create(&self, model: &Rc<TextModel>, _token: &CancellationToken) -> Promise<Rc<OutlineModel>> {
        let outline = OutlineModel::new(model.uri());
        let parent = TreeElement::Model(outline.clone());
        let symbols = self.symbols.borrow().get(&uri_key(&model.uri())).cloned().unwrap_or_default();
        for (i, symbol) in symbols.into_iter().enumerate() {
            let id = format!("symbol_{i}");
            let element = OutlineElement::new(&id, Some(&parent), symbol);
            outline.children().borrow_mut().insert(id, TreeElement::Element(element));
        }
        Promise::resolved(outline)
    }

    fn get_debounce_value(&self, _text_model: &TextModel) -> f64 {
        0.0
    }

    fn get_cached_models(&self) -> Vec<Rc<OutlineModel>> {
        Vec::new()
    }
}

/// O `IResolvedTextEditorModel` do `textModelService`.
struct ResolvedModel {
    text_editor_model: Rc<TextModel>,
}

impl IDisposable for ResolvedModel {
    fn dispose(&self) {}
}

impl IEditorModel for ResolvedModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        Event::none()
    }

    fn resolve(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn is_resolved(&self) -> bool {
        true
    }

    fn is_disposed(&self) -> bool {
        false
    }
}

impl ITextEditorModel for ResolvedModel {
    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        Some(self.text_editor_model.clone())
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(false)
    }

    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        None
    }
}

/// `textModelService`.
struct StubTextModelService {
    model_service: Rc<ModelService>,
    models: RefCell<HashMap<String, Rc<TextModel>>>,
}

impl StubTextModelService {
    fn dispose_models(&self) {
        for (_, model) in self.models.borrow_mut().drain() {
            model.dispose();
        }
    }
}

impl ITextModelService for StubTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        let key = uri_key(resource);
        let existing = self.models.borrow().get(&key).cloned();
        let model = match existing {
            Some(model) => model,
            None => {
                let model = self.model_service.create_model("", None, Some(resource), false);
                self.models.borrow_mut().insert(key, model.clone());
                model
            }
        };
        let object: Rc<dyn ITextEditorModel> = Rc::new(ResolvedModel { text_editor_model: model });
        Promise::resolved(TextModelReference::immortal(object))
    }
}

// #endregion
// #region Helpers

fn md(source: &'static str) -> MockNotebookCell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

fn js(source: &'static str) -> MockNotebookCell {
    (source, "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

/// O `IReference<INotebookCellOutlineDataSource>` de `createMockOutlineDataSource(entries, activeElement)`.
struct MockDataSource {
    entries: Vec<Rc<OutlineEntry>>,
    active_element: Option<Rc<OutlineEntry>>,
}

impl INotebookCellOutlineDataSource for MockDataSource {
    fn active_element(&self) -> Option<Rc<OutlineEntry>> {
        self.active_element.clone()
    }

    fn entries(&self) -> Vec<Rc<OutlineEntry>> {
        self.entries.clone()
    }
}

fn create_mock_outline_data_source(entries: Vec<Rc<OutlineEntry>>, active_element: Option<Rc<OutlineEntry>>) -> Option<Reference<Rc<dyn INotebookCellOutlineDataSource>>> {
    let object: Rc<dyn INotebookCellOutlineDataSource> = Rc::new(MockDataSource { entries, active_element });
    Some(Reference::immortal(object))
}

fn flatten(element: &Rc<OutlineEntry>, data_source: &NotebookOutlinePaneProvider) -> Vec<Rc<OutlineEntry>> {
    let mut elements: Vec<Rc<OutlineEntry>> = Vec::new();

    let children = data_source.get_children(&OutlinePaneParent::Entry(element.clone()));
    for child in children {
        elements.push(child.clone());
        elements.extend(flatten(&child, data_source));
    }

    elements
}

fn build_outline_tree(entries: &[Rc<OutlineEntry>]) -> Option<Vec<Rc<OutlineEntry>>> {
    if entries.is_empty() {
        return None;
    }
    let mut result: Vec<Rc<OutlineEntry>> = vec![entries[0].clone()];
    let mut parent_stack: Vec<Rc<OutlineEntry>> = vec![entries[0].clone()];

    for entry in entries.iter().skip(1) {
        loop {
            match parent_stack.last().cloned() {
                None => {
                    // root node
                    result.push(entry.clone());
                    parent_stack.push(entry.clone());
                    break;
                }
                Some(parent_candidate) => {
                    if parent_candidate.level < entry.level {
                        parent_candidate.add_child(entry.clone());
                        parent_stack.push(entry.clone());
                        break;
                    }
                    parent_stack.pop();
                }
            }
        }
    }
    Some(result)
}

/// Set the configuration settings relevant to various outline views (OutlinePane, QuickPick, Breadcrumbs).
struct OutlineViewConfiguration {
    /// `notebook.outline.showMarkdownHeadersOnly`
    outline_show_markdown_headers_only: bool,
    /// `notebook.outline.showCodeCells`
    outline_show_code_cells: bool,
    /// `notebook.outline.showCodeCellSymbols`
    outline_show_code_cell_symbols: bool,
    /// `notebook.gotoSymbols.showAllSymbols`
    quick_pick_show_all_symbols: bool,
    /// `notebook.breadcrumbs.showCodeCells`
    breadcrumbs_show_code_cells: bool,
}

fn set_outline_view_configuration(configuration_service: &TestConfigurationService, config: &OutlineViewConfiguration) {
    let settings = [
        ("notebook.outline.showMarkdownHeadersOnly", config.outline_show_markdown_headers_only),
        ("notebook.outline.showCodeCells", config.outline_show_code_cells),
        ("notebook.outline.showCodeCellSymbols", config.outline_show_code_cell_symbols),
        ("notebook.gotoSymbols.showAllSymbols", config.quick_pick_show_all_symbols),
        ("notebook.breadcrumbs.showCodeCells", config.breadcrumbs_show_code_cells),
    ];
    for (key, value) in settings {
        let _ = configuration_service.set_user_configuration(key, Dynamic::Bool(value), None);
    }
}

/// O que cada caso monta: as quatro células com os símbolos, o cache de símbolos e a configuração.
struct Fixture {
    configuration_service: Rc<TestConfigurationService>,
    entry_factory: NotebookOutlineEntryFactory,
    cells: Vec<CellViewModel>,
}

impl Fixture {
    fn configuration(&self) -> Rc<dyn IConfigurationService> {
        self.configuration_service.clone()
    }

    /// `new OutlineEntry(-1, -1, rootCell, 'fakeRoot', false, false, undefined, undefined)` com as entradas de cada célula.
    fn raw_outline(&self, root_cell: &CellViewModel) -> Rc<OutlineEntry> {
        let outline_model = OutlineEntry::new(-1, -1, root_cell.clone(), "fakeRoot", false, false, None, None);
        for cell in &self.cells {
            self.entry_factory.get_outline_entries(cell, 0).into_iter().for_each(|entry| outline_model.add_child(entry));
        }
        outline_model
    }
}

/// O começo comum dos casos: configuração, modelos, símbolos (`setSymbolsForTextModel`) e `cacheSymbols` de cada célula.
/// `kinds` são os tipos dos símbolos das células de código `$2` e `$3` (`None` é o `kind` ausente).
fn with_fixture(config: OutlineViewConfiguration, symbols: [Vec<(&str, Option<SymbolKind>)>; 4], body: impl FnOnce(&Fixture)) {
    with_test_notebook(vec![md("# h1"), md("plaintext"), js("# code cell 2"), js("# code cell 3")], |_editor, view_model, _ds, services| {
        let configuration_service = TestConfigurationService::new(None);
        set_outline_view_configuration(&configuration_service, &config);

        let cells = view_model.view_cells();
        let symbols_per_uri: SymbolsPerUri = Rc::new(RefCell::new(HashMap::new()));
        for (cell, symbols) in cells.iter().zip(symbols) {
            let document_symbols = symbols.into_iter().map(|(name, kind)| symbol(name, kind.unwrap_or(SymbolKind::File))).collect();
            symbols_per_uri.borrow_mut().insert(uri_key(&cell.uri()), document_symbols);
        }

        let outline_model_service: Rc<dyn IOutlineModelService> = Rc::new(StubOutlineModelService { symbols: symbols_per_uri });
        let text_model_service = Rc::new(StubTextModelService { model_service: services.model_service.clone(), models: RefCell::new(HashMap::new()) });
        let entry_factory = NotebookOutlineEntryFactory::new(services.notebook_execution_state_service(), outline_model_service, text_model_service.clone());

        // Cache symbols
        for cell in &cells {
            let cached = entry_factory.cache_symbols(cell, &CancellationToken::None);
            runtime::run_microtasks();
            assert!(cached.is_settled());
        }

        body(&Fixture { configuration_service, entry_factory, cells: cells.clone() });
        text_model_service.dispose_models();
    });
}

fn entry_of(element: &Rc<dyn Any>) -> Rc<OutlineEntry> {
    element.clone().downcast::<OutlineEntry>().ok().expect("o elemento é um OutlineEntry")
}

fn no_file_icons() -> Rc<dyn Fn() -> IFileIconTheme> {
    Rc::new(|| IFileIconTheme { has_file_icons: false, has_folder_icons: false, hides_explorer_arrows: false })
}

// #endregion
// #region OutlinePane

#[test]
fn outline_pane_0_default_settings_headers_only_on_code_cells_off_symbols_on() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_fixture(
        OutlineViewConfiguration {
            outline_show_markdown_headers_only: true,
            outline_show_code_cells: false,
            outline_show_code_cell_symbols: true,
            quick_pick_show_all_symbols: false,
            breadcrumbs_show_code_cells: false,
        },
        [vec![], vec![], vec![("var2", None)], vec![("var3", None)]],
        |fixture| {
            // Generate raw outline
            let outline_model = fixture.raw_outline(&fixture.cells[2]);

            // Generate filtered outline (view model)
            let outline_pane_provider = NotebookOutlinePaneProvider::new(None, &fixture.configuration());
            let results = flatten(&outline_model, &outline_pane_provider);

            // Validate
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].label, "h1");
            assert_eq!(results[0].level, 1);
            outline_pane_provider.dispose();
        },
    );
}

#[test]
fn outline_pane_1_all_markdown() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_fixture(
        OutlineViewConfiguration {
            outline_show_markdown_headers_only: false,
            outline_show_code_cells: false,
            outline_show_code_cell_symbols: false,
            quick_pick_show_all_symbols: false,
            breadcrumbs_show_code_cells: false,
        },
        [vec![], vec![], vec![("var2", None)], vec![("var3", None)]],
        |fixture| {
            let outline_model = fixture.raw_outline(&fixture.cells[2]);

            let outline_pane_provider = NotebookOutlinePaneProvider::new(None, &fixture.configuration());
            let results = flatten(&outline_model, &outline_pane_provider);

            assert_eq!(results.len(), 2);

            assert_eq!(results[0].label, "h1");
            assert_eq!(results[0].level, 1);

            assert_eq!(results[1].label, "plaintext");
            assert_eq!(results[1].level, 7);
            outline_pane_provider.dispose();
        },
    );
}

#[test]
fn outline_pane_2_only_headers() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_fixture(
        OutlineViewConfiguration {
            outline_show_markdown_headers_only: true,
            outline_show_code_cells: false,
            outline_show_code_cell_symbols: false,
            quick_pick_show_all_symbols: false,
            breadcrumbs_show_code_cells: false,
        },
        [vec![], vec![], vec![("var2", None)], vec![("var3", None)]],
        |fixture| {
            let outline_model = fixture.raw_outline(&fixture.cells[2]);

            let outline_pane_provider = NotebookOutlinePaneProvider::new(None, &fixture.configuration());
            let results = flatten(&outline_model, &outline_pane_provider);

            assert_eq!(results.len(), 1);

            assert_eq!(results[0].label, "h1");
            assert_eq!(results[0].level, 1);
            outline_pane_provider.dispose();
        },
    );
}

#[test]
fn outline_pane_3_only_headers_code_cells() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_fixture(
        OutlineViewConfiguration {
            outline_show_markdown_headers_only: true,
            outline_show_code_cells: true,
            outline_show_code_cell_symbols: false,
            quick_pick_show_all_symbols: false,
            breadcrumbs_show_code_cells: false,
        },
        [vec![], vec![], vec![("var2", None)], vec![("var3", None)]],
        |fixture| {
            let outline_model = fixture.raw_outline(&fixture.cells[2]);

            let outline_pane_provider = NotebookOutlinePaneProvider::new(None, &fixture.configuration());
            let results = flatten(&outline_model, &outline_pane_provider);

            assert_eq!(results.len(), 3);

            assert_eq!(results[0].label, "h1");
            assert_eq!(results[0].level, 1);

            assert_eq!(results[1].label, "# code cell 2");
            assert_eq!(results[1].level, 7);

            assert_eq!(results[2].label, "# code cell 3");
            assert_eq!(results[2].level, 7);
            outline_pane_provider.dispose();
        },
    );
}

#[test]
fn outline_pane_4_only_headers_code_cells_symbols() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_fixture(
        OutlineViewConfiguration {
            outline_show_markdown_headers_only: true,
            outline_show_code_cells: true,
            outline_show_code_cell_symbols: true,
            quick_pick_show_all_symbols: false,
            breadcrumbs_show_code_cells: false,
        },
        [vec![], vec![], vec![("var2", None)], vec![("var3", None)]],
        |fixture| {
            let outline_model = fixture.raw_outline(&fixture.cells[2]);

            let outline_pane_provider = NotebookOutlinePaneProvider::new(None, &fixture.configuration());
            let results = flatten(&outline_model, &outline_pane_provider);

            // validate
            assert_eq!(results.len(), 5);

            assert_eq!(results[0].label, "h1");
            assert_eq!(results[0].level, 1);

            assert_eq!(results[1].label, "# code cell 2");
            assert_eq!(results[1].level, 7);

            assert_eq!(results[2].label, "var2");
            assert_eq!(results[2].level, 8);

            assert_eq!(results[3].label, "# code cell 3");
            assert_eq!(results[3].level, 7);

            assert_eq!(results[4].label, "var3");
            assert_eq!(results[4].level, 8);
            outline_pane_provider.dispose();
        },
    );
}

// #endregion
// #region QuickPick

#[test]
fn quick_pick_0_symbols_on_2_cells_with_symbols() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_fixture(
        OutlineViewConfiguration {
            outline_show_markdown_headers_only: false,
            outline_show_code_cells: false,
            outline_show_code_cell_symbols: false,
            quick_pick_show_all_symbols: true,
            breadcrumbs_show_code_cells: false,
        },
        [vec![], vec![], vec![("var2", Some(SymbolKind::Variable))], vec![("var3", Some(SymbolKind::Variable))]],
        |fixture| {
            let outline_model = fixture.raw_outline(&fixture.cells[2]);

            let quick_pick_provider = NotebookQuickPickProvider::new(create_mock_outline_data_source(outline_model.children(), None), &fixture.configuration(), no_file_icons());
            let results = quick_pick_provider.get_quick_pick_elements();

            // Validate
            assert_eq!(results.len(), 4);

            assert_eq!(results[0].label, "$(markdown) h1");
            assert_eq!(entry_of(&results[0].element).level, 1);

            assert_eq!(results[1].label, "$(markdown) plaintext");
            assert_eq!(entry_of(&results[1].element).level, 7);

            assert_eq!(results[2].label, "$(symbol-variable) var2");
            assert_eq!(entry_of(&results[2].element).level, 8);

            assert_eq!(results[3].label, "$(symbol-variable) var3");
            assert_eq!(entry_of(&results[3].element).level, 8);
            quick_pick_provider.dispose();
        },
    );
}

#[test]
fn quick_pick_1_symbols_on_1_cell_with_symbol_1_cell_without_symbol() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_fixture(
        OutlineViewConfiguration {
            outline_show_markdown_headers_only: false,
            outline_show_code_cells: false,
            outline_show_code_cell_symbols: false,
            quick_pick_show_all_symbols: true,
            breadcrumbs_show_code_cells: false,
        },
        [vec![], vec![], vec![], vec![("var3", Some(SymbolKind::Variable))]],
        |fixture| {
            let outline_model = fixture.raw_outline(&fixture.cells[2]);

            let quick_pick_provider = NotebookQuickPickProvider::new(create_mock_outline_data_source(outline_model.children(), None), &fixture.configuration(), no_file_icons());
            let results = quick_pick_provider.get_quick_pick_elements();

            // Validate
            assert_eq!(results.len(), 4);

            assert_eq!(results[0].label, "$(markdown) h1");
            assert_eq!(entry_of(&results[0].element).level, 1);

            assert_eq!(results[1].label, "$(markdown) plaintext");
            assert_eq!(entry_of(&results[1].element).level, 7);

            assert_eq!(results[2].label, "$(code) # code cell 2");
            assert_eq!(entry_of(&results[2].element).level, 7);

            assert_eq!(results[3].label, "$(symbol-variable) var3");
            assert_eq!(entry_of(&results[3].element).level, 8);
            quick_pick_provider.dispose();
        },
    );
}

#[test]
fn quick_pick_3_symbols_off() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_fixture(
        OutlineViewConfiguration {
            outline_show_markdown_headers_only: false,
            outline_show_code_cells: false,
            outline_show_code_cell_symbols: false,
            quick_pick_show_all_symbols: false,
            breadcrumbs_show_code_cells: false,
        },
        [vec![], vec![], vec![("var2", Some(SymbolKind::Variable))], vec![("var3", Some(SymbolKind::Variable))]],
        |fixture| {
            let outline_model = fixture.raw_outline(&fixture.cells[2]);

            let quick_pick_provider = NotebookQuickPickProvider::new(create_mock_outline_data_source(outline_model.children(), None), &fixture.configuration(), no_file_icons());
            let results = quick_pick_provider.get_quick_pick_elements();

            // Validate
            assert_eq!(results.len(), 4);

            assert_eq!(results[0].label, "$(markdown) h1");
            assert_eq!(entry_of(&results[0].element).level, 1);

            assert_eq!(results[1].label, "$(markdown) plaintext");
            assert_eq!(entry_of(&results[1].element).level, 7);

            assert_eq!(results[2].label, "$(code) # code cell 2");
            assert_eq!(entry_of(&results[2].element).level, 7);

            assert_eq!(results[3].label, "$(code) # code cell 3");
            assert_eq!(entry_of(&results[3].element).level, 7);
            quick_pick_provider.dispose();
        },
    );
}

// #endregion
// #region Breadcrumbs

#[test]
fn breadcrumbs_0_code_cells_on() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_fixture(
        OutlineViewConfiguration {
            outline_show_markdown_headers_only: false,
            outline_show_code_cells: false,
            outline_show_code_cell_symbols: false,
            quick_pick_show_all_symbols: false,
            breadcrumbs_show_code_cells: true,
        },
        [vec![], vec![], vec![("var2", Some(SymbolKind::Variable))], vec![("var3", Some(SymbolKind::Variable))]],
        |fixture| {
            let outline_model = fixture.raw_outline(&fixture.cells[0]);
            let outline_tree = build_outline_tree(&outline_model.children()).expect("a árvore tem entradas");

            let active_element = outline_tree[0].children()[1].clone();
            let breadcrumbs_provider = NotebookBreadcrumbsProvider::new(create_mock_outline_data_source(Vec::new(), Some(active_element)), &fixture.configuration());
            let results = breadcrumbs_provider.get_breadcrumb_elements();

            // Validate
            assert_eq!(results.len(), 3);

            assert_eq!(entry_of(&results[0].element).label, "fakeRoot");
            assert_eq!(entry_of(&results[0].element).level, -1);

            assert_eq!(entry_of(&results[1].element).label, "h1");
            assert_eq!(entry_of(&results[1].element).level, 1);

            assert_eq!(entry_of(&results[2].element).label, "# code cell 2");
            assert_eq!(entry_of(&results[2].element).level, 7);
            breadcrumbs_provider.dispose();
        },
    );
}

#[test]
fn breadcrumbs_1_code_cells_off() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_fixture(
        OutlineViewConfiguration {
            outline_show_markdown_headers_only: false,
            outline_show_code_cells: false,
            outline_show_code_cell_symbols: false,
            quick_pick_show_all_symbols: false,
            breadcrumbs_show_code_cells: false,
        },
        [vec![], vec![], vec![("var2", Some(SymbolKind::Variable))], vec![("var3", Some(SymbolKind::Variable))]],
        |fixture| {
            let outline_model = fixture.raw_outline(&fixture.cells[0]);
            let outline_tree = build_outline_tree(&outline_model.children()).expect("a árvore tem entradas");

            let active_element = outline_tree[0].children()[1].clone();
            let breadcrumbs_provider = NotebookBreadcrumbsProvider::new(create_mock_outline_data_source(Vec::new(), Some(active_element)), &fixture.configuration());
            let results = breadcrumbs_provider.get_breadcrumb_elements();

            // Validate
            assert_eq!(results.len(), 2);

            assert_eq!(entry_of(&results[0].element).label, "fakeRoot");
            assert_eq!(entry_of(&results[0].element).level, -1);

            assert_eq!(entry_of(&results[1].element).label, "h1");
            assert_eq!(entry_of(&results[1].element).level, 1);
            breadcrumbs_provider.dispose();
        },
    );
}

// #endregion
