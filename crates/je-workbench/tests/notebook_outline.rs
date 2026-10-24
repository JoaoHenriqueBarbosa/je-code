//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/notebookOutline.test.ts` (suíte `Notebook Outline`), caso a caso e na
//! mesma ordem. Estão portados 5 dos 6 casos.
//!
//! Casos de fora:
//! - `Symbols for goto quickpick are pre-cached`: o caso confere que `NotebookOutlineCreator.createOutline` (alvo `QuickPick`) chama o
//!   `cacheSymbols` da fábrica antes de devolver o outline (a fábrica é trocada por um `cacheSymbols` que marca um flag). O
//!   `NotebookOutlineCreator` e o `NotebookCellOutline` (que dependem do `IOutlineService`, do `IEditorPane`/`NotebookEditor`, do
//!   registro de provedores de símbolos e dos `Delayer`s) não têm porte; o `computeFullSymbols` da fonte de dados, que ele chama,
//!   não é o que o caso mede.
//!
//! Notas de tradução:
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

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::{IDisposable, Reference};
use je_platform::markers::marker_service::MarkerService;
use je_platform::markers::markers::IMarkerService;
use je_workbench::contrib::notebook::browser::contrib::outline::notebook_outline::{IFileIconTheme, NotebookQuickPickProvider};
use je_workbench::contrib::notebook::browser::controller::IActiveNotebookEditor;
use je_workbench::contrib::notebook::browser::view_model::notebook_outline_data_source::{INotebookCellOutlineDataSource, NotebookCellOutlineDataSource};
use je_workbench::contrib::notebook::browser::view_model::notebook_outline_entry_factory::{INotebookOutlineEntryFactory, NotebookOutlineEntryFactory};
use je_workbench::contrib::notebook::browser::view_model::outline_entry::OutlineEntry;
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::services::outline::outline::{IQuickPickDataSource, QuickPickOutlineElement};
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
