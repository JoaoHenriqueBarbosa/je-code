//! Porte de `src/vs/workbench/test/browser/parts/editor/textEditorPane.test.ts` (suíte `TextEditorPane`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado antes do corpo (o
//!   `disposables` do original é o store dele) e descartado depois dele (`drop(guard)`), que é o `disposables.clear()` do
//!   `teardown` seguido da conferência de vazamentos. O `setup` ([`setup`]) registra o painel `TestTextFileEditor`.
//! - **`createServices`.** O [`EditorServiceAccessor`] já monta o `workbenchInstantiationService`, o `createEditorPart`, o
//!   `IEditorGroupsService` sobre a parte e o `EditorService` real (ver o cabeçalho de `support/editor_service_support.rs`);
//!   [`create_services`] só espera a parte ficar pronta.
//! - **`TestTextFileEditor`.** `setSelection(selection, reason)` é `EditorPane::set_selection` do painel registrado por
//!   `register_test_file_editor`, `pane.onDidChangeSelection` é `IEditorPane::on_did_change_selection` e `pane.getSelection()` é
//!   `IEditorPane::get_selection` (ver o cabeçalho de `editor_panes.rs`).
//! - **`DeferredPromise`.** É a promessa de `Promise::new` com um resolvedor; o `complete` só vale uma vez (o guarda
//!   `completed`), porque o painel pode avisar `EDIT` mais de uma vez.
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))` com o título completo do caso.
//! - **`pane.group.closeAllEditors()`.** O grupo é a busca pelo `group_id` do painel no serviço de grupos.
//! - **`sel4`.** O literal `{ compare, restore }` é o [`ThrowingSelection`] (o `as_any` padrão de `IEditorPaneSelection` não
//!   reconhece nenhuma classe de seleção, que é o `instanceof TextEditorPaneSelection` falso do original).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::lifecycle::DisposableStore;
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::selection::Selection;
use je_editor::text_model::TextSource;
use je_platform::resources::{host_environment, uri_file};
use je_workbench::browser::parts::editor::editor_panes::EditorPane;
use je_workbench::browser::parts::editor::text_editor::TextEditorPaneSelection;
use je_workbench::common::editor::{
    EditorPaneSelectionChangeEvent, EditorPaneSelectionChangeReason, EditorPaneSelectionCompareResult, IEditorPane, IEditorPaneSelection, TextEditorOptions, UntypedEditorInput,
    is_editor_pane_with_selection,
};
use je_workbench::services::editor::editor_groups_service::IEditorPart;
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::services::textfile::textfiles::ITextFileEditorModelManager;
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use support::editor_service_support::{EditorServiceAccessor, register_test_file_editor};

/// O título da suíte, que o `this.test.fullTitle()` do `toResource` usa.
const SUITE_TITLE: &str = "TextEditorPane";

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();

    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// O `setup`: `disposables.add(registerTestFileEditor())`.
fn setup(disposables: &Rc<DisposableStore>) {
    disposables.add(register_test_file_editor());
}

/// `createServices()`.
async fn create_services(disposables: &Rc<DisposableStore>) -> EditorServiceAccessor {
    let accessor = EditorServiceAccessor::with_default_backup_service(disposables);
    accessor.part.when_ready().await.unwrap();

    accessor
}

/// `accessor.editorService.openEditor(editor)`.
async fn open_editor(accessor: &EditorServiceAccessor, editor: UntypedEditorInput) -> Option<Rc<dyn IEditorPane>> {
    IEditorService::open_editor(accessor.editor_service.as_ref(), editor).await.unwrap()
}

/// `await pane.group.closeAllEditors()`.
async fn close_all_editors_of(accessor: &EditorServiceAccessor, pane: &Rc<dyn IEditorPane>) {
    let group = accessor.editor_groups_service.get_group(pane.group_id()).expect("o grupo do painel existe");
    group.close_all_editors(None).await.unwrap();
}

/// O literal `{ compare: () => { throw new Error(); }, restore: (options) => options }`.
struct ThrowingSelection;

impl IEditorPaneSelection for ThrowingSelection {
    fn compare(&self, _other_selection: &dyn IEditorPaneSelection) -> EditorPaneSelectionCompareResult {
        panic!("Error");
    }

    fn restore(&self, options: &TextEditorOptions) -> TextEditorOptions {
        options.clone()
    }
}

#[test]
fn editor_pane_selection() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_real_timers(async move {
        let disposables = guard.store();
        setup(&disposables);

        let accessor = create_services(&disposables).await;

        let resource = to_resource("editor pane selection", "/path/index.txt");
        let pane = open_editor(&accessor, UntypedEditorInput { resource: Some(resource.clone()), ..UntypedEditorInput::default() }).await;

        let pane = pane.expect("o editor abriu");
        assert!(is_editor_pane_with_selection(Some(&pane)));

        let (on_did_fire_selection_event_of_edit_type, resolver) = Promise::<EditorPaneSelectionChangeEvent>::new();
        let completed = Rc::new(Cell::new(false));
        disposables.add(pane.on_did_change_selection().expect("o painel tem seleção").listen(move |e| {
            if e.reason == EditorPaneSelectionChangeReason::Edit && !completed.replace(true) {
                resolver.resolve(e.clone());
            }
        }));

        // Changing model reports selection change
        // of EDIT kind

        let model = accessor.files().resolve(&resource, None).await.unwrap();
        disposables.add(model.clone());
        model.text_editor_model().expect("o modelo de texto existe").set_value(TextSource::from("Hello World"));

        let event = on_did_fire_selection_event_of_edit_type.await.unwrap();
        assert_eq!(event.reason, EditorPaneSelectionChangeReason::Edit);

        // getSelection() works and can be restored
        //
        // Note: this is a bit bogus because in tests our code editors have
        //       no view and no cursor can be set as such. So the selection
        //       will always report for the first line and column.

        pane.as_any().downcast_ref::<EditorPane>().expect("o painel é o TestTextFileEditor").set_selection(Some(Selection::new(1, 1, 1, 1)), EditorPaneSelectionChangeReason::User);
        let selection = pane.get_selection().expect("o painel tem seleção");
        close_all_editors_of(&accessor, &pane).await;
        let options = selection.restore(&TextEditorOptions::default());
        let pane = open_editor(&accessor, UntypedEditorInput { resource: Some(resource.clone()), options: Some(options), ..UntypedEditorInput::default() }).await;

        let pane = pane.expect("o editor abriu");
        assert!(is_editor_pane_with_selection(Some(&pane)));

        let new_selection = pane.get_selection().expect("o painel tem seleção");
        assert_eq!(new_selection.compare(&*selection), EditorPaneSelectionCompareResult::Identical);

        IWorkingCopy::revert(&*model, None).await.unwrap();
        close_all_editors_of(&accessor, &pane).await;

        drop(guard);
    });
}

#[test]
fn text_editor_pane_selection() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let sel1 = TextEditorPaneSelection::new(Selection::new(1, 1, 2, 2));
    let sel2 = TextEditorPaneSelection::new(Selection::new(5, 5, 6, 6));
    let sel3 = TextEditorPaneSelection::new(Selection::new(50, 50, 60, 60));
    let sel4 = ThrowingSelection;

    assert_eq!(sel1.compare(&sel1), EditorPaneSelectionCompareResult::Identical);
    assert_eq!(sel1.compare(&sel2), EditorPaneSelectionCompareResult::Similar);
    assert_eq!(sel1.compare(&sel3), EditorPaneSelectionCompareResult::Different);
    assert_eq!(sel1.compare(&sel4), EditorPaneSelectionCompareResult::Different);
}
