//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/notebookOutline.test.ts` (suíte `Notebook Outline`), caso a caso e na
//! mesma ordem. Os 6 casos estão portados.
//!
//! Notas de tradução:
//! - `Symbols for goto quickpick are pre-cached` usa o `NotebookOutlineCreator` e o `NotebookCellOutline` portados (sem o registro no
//!   `IOutlineService` e sem o `IOutline`, ver `notebook_cell_outline.rs`): o `stub` da `INotebookOutlineEntryFactory` com o
//!   `cacheSymbols` trocado é o [`CacheSymbolsFlagFactory`] (que delega o resto à fábrica real), `notebookEditorPane` (`mock<INotebookEditorPane>`)
//!   é o [`NotebookEditorPaneMock`] e `LanguageFeaturesService`/`MarkerService` do `setup` são os reais. `await createOutline(...)` é a
//!   promessa resolvida com `run_microtasks` (o `with_test_notebook` roda sob o relógio simulado).
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso; o `setup` (o `DisposableStore` e o
//!   `setupInstantiationService`) é o `with_test_notebook`.
//! - `withNotebookOutline(cells, target, callback)` cria o outline do editor. Como o `NotebookCellOutline` não tem porte, o outline é as
//!   duas partes que o caso lê: `outline.entries` é o `entries` do [`NotebookCellOutlineDataSource`] (com a mesma fábrica de
//!   entradas, o serviço de marcadores e a configuração) e `outline.config.quickPickDataSource` é o [`NotebookQuickPickProvider`]
//!   sobre essa fonte de dados (que o `NotebookCellOutline` cria no `setDataSources`). `assert.ok(outline instanceof NotebookCellOutline)`
//!   é garantido pelo sistema de tipos e não aparece.
//! - `IThemeService.getFileIconTheme()` do `setup` (`hasFileIcons: true`) é o `Fn() -> IFileIconTheme` do seletor rápido.
//! - As mensagens de erro dos `assert.deepStrictEqual` (texto da célula e da primeira linha) não são portadas.
//! - Os casos que têm mais de um `withNotebookOutline` fazem as chamadas em sequência, cada uma com o seu notebook.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, Reference};
use je_base::promise::Promise;
use je_base::runtime;
use je_editor::language_features_service::LanguageFeaturesService;
use je_platform::markers::marker_service::MarkerService;
use je_platform::markers::markers::IMarkerService;
use je_workbench::common::editor::EditorPaneSelectionChangeEvent;
use je_workbench::contrib::notebook::browser::contrib::outline::notebook_cell_outline::{
    INotebookEditorPane, NotebookOutlineCreator, NotebookOutlineServices,
};
use je_workbench::contrib::notebook::browser::contrib::outline::notebook_outline::{IFileIconTheme, NotebookQuickPickProvider};
use je_workbench::contrib::notebook::browser::controller::IActiveNotebookEditor;
use je_workbench::contrib::notebook::browser::view_model::notebook_outline_data_source::{INotebookCellOutlineDataSource, NotebookCellOutlineDataSource};
use je_workbench::contrib::notebook::browser::view_model::notebook_outline_data_source_factory::{INotebookCellOutlineDataSourceFactory, NotebookCellOutlineDataSourceFactory};
use je_workbench::contrib::notebook::browser::view_model::notebook_outline_entry_factory::{INotebookOutlineEntryFactory, NotebookOutlineEntryFactory};
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use je_workbench::contrib::notebook::browser::view_model::outline_entry::OutlineEntry;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::services::outline::outline::{IQuickPickDataSource, OutlineTarget, QuickPickOutlineElement};
use support::notebook_outline_support::{UnusedOutlineModelService, UnusedTextModelService};
use support::test_notebook_editor::{MockNotebookCell, with_test_notebook};

/// `[source, 'md', CellKind.Markup]`.
fn markup(source: &'static str) -> MockNotebookCell {
    (source, "md", CellKind::Markup, Vec::new(), None)
}

/// O `NotebookCellOutline` do caso: as entradas e o seletor rápido (ver as notas do módulo).
struct Outline {
    entries: Vec<Rc<OutlineEntry>>,
    quick_pick_elements: Vec<QuickPickOutlineElement>,
}

/// `withNotebookOutline(cells, OutlineTarget.OutlinePane, callback)`.
fn with_notebook_outline<R>(cells: Vec<MockNotebookCell>, callback: impl FnOnce(&Outline) -> R) -> R {
    with_test_notebook(cells, |editor, _view_model, _ds, services| {
        let marker_service = MarkerService::new();
        let marker_service_ref: Rc<dyn IMarkerService> = marker_service.clone();
        let entry_factory: Rc<dyn INotebookOutlineEntryFactory> =
            Rc::new(NotebookOutlineEntryFactory::new(services.notebook_execution_state_service(), Rc::new(UnusedOutlineModelService), Rc::new(UnusedTextModelService)));
        let active_editor: Rc<dyn IActiveNotebookEditor> = editor.clone();
        let data_source = NotebookCellOutlineDataSource::new(active_editor, marker_service_ref, services.configuration_service.clone(), entry_factory);

        let data_source_object: Rc<dyn INotebookCellOutlineDataSource> = data_source.clone();
        let quick_pick = NotebookQuickPickProvider::new(
            Some(Reference::immortal(data_source_object)),
            &services.configuration_service,
            Rc::new(|| IFileIconTheme { has_file_icons: true, has_folder_icons: true, hides_explorer_arrows: false }),
        );
        let outline = Outline { entries: data_source.entries(), quick_pick_elements: quick_pick.get_quick_pick_elements() };

        let result = callback(&outline);
        quick_pick.dispose();
        data_source.dispose();
        marker_service.dispose();
        result
    })
}

#[test]
fn basic() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_notebook_outline(Vec::new(), |outline| {
        assert!(outline.quick_pick_elements.is_empty());
    });
}

#[test]
fn special_characters_in_heading() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_notebook_outline(vec![markup("# Hellö & Hällo")], |outline| {
        assert_eq!(outline.quick_pick_elements.len(), 1);
        assert_eq!(outline.quick_pick_elements[0].label, "Hellö & Hällo");
    });

    with_notebook_outline(vec![markup("# bo<i>ld</i>")], |outline| {
        assert_eq!(outline.quick_pick_elements.len(), 1);
        assert_eq!(outline.quick_pick_elements[0].label, "bold");
    });
}

#[test]
fn notebook_falsely_detects_empty_cells() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_notebook_outline(vec![markup("  的时代   ")], |outline| {
        assert_eq!(outline.quick_pick_elements.len(), 1);
        assert_eq!(outline.entries[0].label, "的时代");
        assert_eq!(outline.quick_pick_elements[0].label, "的时代");
    });

    with_notebook_outline(vec![markup("   ")], |outline| {
        assert_eq!(outline.quick_pick_elements.len(), 1);
        assert_eq!(outline.entries[0].label, "empty cell");
        assert_eq!(outline.quick_pick_elements[0].label, "empty cell");
    });

    with_notebook_outline(vec![markup("+++++[]{}--)(0  ")], |outline| {
        assert_eq!(outline.quick_pick_elements.len(), 1);
        assert_eq!(outline.entries[0].label, "+++++[]{}--)(0");
        assert_eq!(outline.quick_pick_elements[0].label, "+++++[]{}--)(0");
    });

    with_notebook_outline(vec![markup("+++++[]{}--)(0 Hello **&^ ")], |outline| {
        assert_eq!(outline.quick_pick_elements.len(), 1);
        assert_eq!(outline.entries[0].label, "+++++[]{}--)(0 Hello **&^");
        assert_eq!(outline.quick_pick_elements[0].label, "+++++[]{}--)(0 Hello **&^");
    });

    with_notebook_outline(vec![markup("!@#$\n Überschrïft")], |outline| {
        assert_eq!(outline.quick_pick_elements.len(), 1);
        assert_eq!(outline.entries[0].label, "!@#$");
        assert_eq!(outline.quick_pick_elements[0].label, "!@#$");
    });
}

#[test]
fn heading_text_defines_entry_label() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_notebook_outline(vec![markup("foo\n # h1")], |outline| {
        assert_eq!(outline.quick_pick_elements.len(), 1);
        assert_eq!(outline.quick_pick_elements[0].label, "h1");
    });
}

#[test]
fn notebook_outline_ignores_markdown_headings_115200() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_notebook_outline(vec![markup("## h2 \n# h1")], |outline| {
        assert_eq!(outline.quick_pick_elements.len(), 2);
        assert_eq!(outline.quick_pick_elements[0].label, "h2");
        assert_eq!(outline.quick_pick_elements[1].label, "h1");
    });

    with_notebook_outline(vec![markup("## h2"), markup("# h1")], |outline| {
        assert_eq!(outline.quick_pick_elements.len(), 2);
        assert_eq!(outline.quick_pick_elements[0].label, "h2");
        assert_eq!(outline.quick_pick_elements[1].label, "h1");
    });
}

/// `testOutlineEntryFactory.cacheSymbols = async () => { symbolsCached = true; }`: a fábrica real com o `cacheSymbols` trocado.
struct CacheSymbolsFlagFactory {
    inner: NotebookOutlineEntryFactory,
    symbols_cached: Rc<Cell<bool>>,
}

impl INotebookOutlineEntryFactory for CacheSymbolsFlagFactory {
    fn get_outline_entries(&self, cell: &CellViewModel, index: usize) -> Vec<Rc<OutlineEntry>> {
        self.inner.get_outline_entries(cell, index)
    }

    fn cache_symbols(&self, _cell: &CellViewModel, _cancel_token: &CancellationToken) -> Promise<()> {
        self.symbols_cached.set(true);
        Promise::resolved(())
    }
}

/// `new class extends mock<INotebookEditorPane>() { getControl() { return editor; } onDidChangeModel = Event.None; onDidChangeSelection = Event.None; }`.
struct NotebookEditorPaneMock {
    editor: Rc<dyn IActiveNotebookEditor>,
}

impl INotebookEditorPane for NotebookEditorPaneMock {
    fn get_control(&self) -> Option<Rc<dyn IActiveNotebookEditor>> {
        Some(self.editor.clone())
    }

    fn on_did_change_model(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_selection(&self) -> Event<EditorPaneSelectionChangeEvent> {
        Event::none()
    }

    // `textModel` não é sobrescrito no `mock`: fica `undefined`.
    fn text_model(&self) -> Option<Rc<NotebookTextModel>> {
        None
    }
}

#[test]
fn symbols_for_goto_quickpick_are_pre_cached() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let symbols_cached = Rc::new(Cell::new(false));
    with_test_notebook(vec![("a = 1\nb = 2", "python", CellKind::Code, Vec::new(), None)], |editor, _view_model, _ds, services| {
        let marker_service = MarkerService::new();
        let marker_service_ref: Rc<dyn IMarkerService> = marker_service.clone();
        let entry_factory: Rc<dyn INotebookOutlineEntryFactory> = Rc::new(CacheSymbolsFlagFactory {
            inner: NotebookOutlineEntryFactory::new(services.notebook_execution_state_service(), Rc::new(UnusedOutlineModelService), Rc::new(UnusedTextModelService)),
            symbols_cached: symbols_cached.clone(),
        });
        let data_source_factory: Rc<dyn INotebookCellOutlineDataSourceFactory> =
            Rc::new(NotebookCellOutlineDataSourceFactory::new(marker_service_ref, services.configuration_service.clone(), entry_factory));
        let creator = NotebookOutlineCreator::new(NotebookOutlineServices {
            configuration_service: services.configuration_service.clone(),
            language_features_service: Rc::new(LanguageFeaturesService::new()),
            notebook_execution_state_service: services.notebook_execution_state_service(),
            data_source_factory,
            on_did_file_icon_theme_change: Event::none(),
            file_icon_theme: Rc::new(|| IFileIconTheme { has_file_icons: true, has_folder_icons: true, hides_explorer_arrows: false }),
        });
        let active_editor: Rc<dyn IActiveNotebookEditor> = editor.clone();
        let notebook_editor_pane: Rc<dyn INotebookEditorPane> = Rc::new(NotebookEditorPaneMock { editor: active_editor });

        let created = creator.create_outline(notebook_editor_pane, OutlineTarget::QUICK_PICK, &CancellationToken::None);
        runtime::run_microtasks();
        let outline = created.result().expect("o outline foi criado").expect("a criação não falhou");

        assert!(symbols_cached.get());

        outline.dispose();
        marker_service.dispose();
    });
}
