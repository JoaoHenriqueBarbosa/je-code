//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/notebookSymbols.test.ts` (suíte `Notebook Symbols`), caso a caso e na mesma ordem. Os 4 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso.
//! - `createCellViewModel(version, textmodelId)` devolve um objeto que finge ser `ICellViewModel`; a camada lógica só tem o `CellViewModel`
//!   real, então cada caso cria um notebook de teste com as células de código `# code` (o `getText()` do original) e os símbolos vão
//!   por URI da célula (no original, por id do modelo de texto `textId`, `$1` e `$2`). `getOutlineEntries(createCellViewModel(...), 0)`,
//!   que cria outro objeto com o mesmo id, é a mesma célula (o cache é pelo `id` da célula).
//! - `OutlineModelStub` (`getTopLevelSymbols`) é um `OutlineModel` real com um `OutlineElement` por símbolo de topo (os filhos ficam no
//!   `children` do `DocumentSymbol`), `textModelService.createModelReference` devolve o `TextModel` que o serviço de modelos cria para a
//!   URI da célula, `getDebounceValue` devolve `0` e `executionService.getCellExecution()` devolve `undefined` (o serviço de execução de
//!   `setup_notebook_services`).
//! - `{ name, range: {}, children }` é um `DocumentSymbol` com intervalo vazio e `kind` ausente (`SymbolKind::File`, falso como o
//!   `undefined`).
//! - `await entryFactory.cacheSymbols(...)` é a chamada seguida de `run_microtasks`.
//! - `assert.equal(a, b, msg)` é `assert_eq!(a, b, msg)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::language_providers::{DocumentSymbol, SymbolKind};
use je_editor::model::TextSnapshot;
use je_editor::model_service::ModelService;
use je_editor::outline_model::{IOutlineModelService, OutlineElement, OutlineModel, TreeElement};
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::contrib::notebook::browser::view_model::notebook_outline_entry_factory::{INotebookOutlineEntryFactory, NotebookOutlineEntryFactory};
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference};
use support::test_notebook_editor::{MockNotebookCell, empty_metadata, with_test_notebook};

type SymbolsPerUri = Rc<RefCell<HashMap<String, Vec<DocumentSymbol>>>>;

fn uri_key(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `{ name, range: {}, children }`.
fn symbol(name: &str, children: Option<Vec<DocumentSymbol>>) -> DocumentSymbol {
    DocumentSymbol {
        name: name.to_string(),
        detail: String::new(),
        kind: SymbolKind::File,
        tags: Vec::new(),
        container_name: None,
        range: Range::new(1, 1, 1, 1),
        selection_range: Range::new(1, 1, 1, 1),
        children,
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

fn code_cell() -> MockNotebookCell {
    ("# code", "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

/// O `setup` comum: um notebook com uma célula de código `# code` por conjunto de símbolos, a fábrica de entradas e os modelos.
fn with_cells(symbols: Vec<Vec<DocumentSymbol>>, body: impl FnOnce(&NotebookOutlineEntryFactory, &[CellViewModel])) {
    let cells = symbols.iter().map(|_| code_cell()).collect();
    with_test_notebook(cells, |_editor, view_model, _ds, services| {
        let view_cells = view_model.view_cells();
        let symbols_per_uri: SymbolsPerUri = Rc::new(RefCell::new(HashMap::new()));
        for (cell, symbols) in view_cells.iter().zip(symbols) {
            symbols_per_uri.borrow_mut().insert(uri_key(&cell.uri()), symbols);
        }

        let outline_model_service: Rc<dyn IOutlineModelService> = Rc::new(StubOutlineModelService { symbols: symbols_per_uri });
        let text_model_service = Rc::new(StubTextModelService { model_service: services.model_service.clone(), models: RefCell::new(HashMap::new()) });
        let entry_factory = NotebookOutlineEntryFactory::new(services.notebook_execution_state_service(), outline_model_service, text_model_service.clone());

        body(&entry_factory, &view_cells);
        text_model_service.dispose_models();
    });
}

/// `await entryFactory.cacheSymbols(cell, CancellationToken.None)`.
fn cache_symbols(entry_factory: &NotebookOutlineEntryFactory, cell: &CellViewModel) {
    let cached = entry_factory.cache_symbols(cell, &CancellationToken::None);
    runtime::run_microtasks();
    assert!(cached.is_settled());
}

#[test]
fn cell_without_symbols_cache() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_cells(vec![vec![symbol("var", None)]], |entry_factory, cells| {
        let entries = entry_factory.get_outline_entries(&cells[0], 0);

        assert_eq!(entries.len(), 1, "no entries created");
        assert_eq!(entries[0].label, "# code", "entry should fall back to first line of cell");
    });
}

#[test]
fn cell_with_simple_symbols() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_cells(vec![vec![symbol("var1", None), symbol("var2", None)]], |entry_factory, cells| {
        let cell = &cells[0];

        cache_symbols(entry_factory, cell);
        let entries = entry_factory.get_outline_entries(cell, 0);

        assert_eq!(entries.len(), 3, "wrong number of outline entries");
        assert_eq!(entries[0].label, "# code");
        assert_eq!(entries[1].label, "var1");
        // 6 levels for markdown, all code symbols are greater than the max markdown level
        assert_eq!(entries[1].level, 8);
        assert_eq!(entries[1].index, 1);
        assert_eq!(entries[2].label, "var2");
        assert_eq!(entries[2].level, 8);
        assert_eq!(entries[2].index, 2);
    });
}

#[test]
fn cell_with_nested_symbols() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let symbols = vec![
        symbol("root1", Some(vec![symbol("nested1", None), symbol("nested2", None)])),
        symbol("root2", Some(vec![symbol("nested1", None)])),
    ];
    with_cells(vec![symbols], |entry_factory, cells| {
        let cell = &cells[0];

        cache_symbols(entry_factory, cell);
        let entries = entry_factory.get_outline_entries(cell, 0);

        assert_eq!(entries.len(), 6, "wrong number of outline entries");
        assert_eq!(entries[0].label, "# code");
        assert_eq!(entries[1].label, "root1");
        assert_eq!(entries[1].level, 8);
        assert_eq!(entries[2].label, "nested1");
        assert_eq!(entries[2].level, 9);
        assert_eq!(entries[3].label, "nested2");
        assert_eq!(entries[3].level, 9);
        assert_eq!(entries[4].label, "root2");
        assert_eq!(entries[4].level, 8);
        assert_eq!(entries[5].label, "nested1");
        assert_eq!(entries[5].level, 9);
    });
}

#[test]
fn multiple_cells_with_symbols() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_cells(vec![vec![symbol("var1", None)], vec![symbol("var2", None)]], |entry_factory, cells| {
        let (cell1, cell2) = (&cells[0], &cells[1]);
        cache_symbols(entry_factory, cell1);
        cache_symbols(entry_factory, cell2);

        let entries1 = entry_factory.get_outline_entries(cell1, 0);
        let entries2 = entry_factory.get_outline_entries(cell2, 0);

        assert_eq!(entries1.len(), 2, "wrong number of outline entries");
        assert_eq!(entries1[0].label, "# code");
        assert_eq!(entries1[1].label, "var1");
        assert_eq!(entries2.len(), 2, "wrong number of outline entries");
        assert_eq!(entries2[0].label, "# code");
        assert_eq!(entries2[1].label, "var2");
    });
}
