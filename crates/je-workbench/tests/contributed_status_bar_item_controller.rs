//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/contributedStatusBarItemController.test.ts` (suíte `Notebook Statusbar`), caso a caso e na mesma ordem. O caso está portado.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do caso, e `await withTestNotebook(cells, async (editor, viewModel, _ds,
//!   accessor) => ...)` é o `with_test_notebook` com o corpo síncrono sob o relógio simulado.
//! - `accessor.get(INotebookCellStatusBarService)` é o `NotebookCellStatusBarService` real, e `editor` (`INotebookEditor`) é o
//!   [`VisibleCellsEditor`] sobre o editor de teste, com os eventos que o original nunca dispara como `Event::none()`.
//! - `asPromise(event, message)` (que rejeita em 1 s) é o [`ProvideWatcher`]: um ouvinte que guarda o valor do evento, e `await` é o
//!   avanço dos timers simulados um a um até o valor chegar, seguido da leitura dele (se não chegou, o caso falha com a mensagem).
//! - `provideCalls`, `onProvideCalled` e `_onDidChangeStatusBarItems` são os `Cell` e emissores do [`TestProvider`].
//! - `cell0.metadata = { ...cell0.metadata, ...{ newMetadata: true } }` é o `set_metadata` com o objeto acrescido da chave nova.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_workbench::contrib::notebook::browser::contrib::cell_status_bar::contributed_status_bar_item_controller::ContributedStatusBarItemController;
use je_workbench::contrib::notebook::browser::contrib::cell_status_bar::notebook_visible_cell_observer::IVisibleCellsEditor;
use je_workbench::contrib::notebook::browser::services::notebook_cell_status_bar_service_impl::NotebookCellStatusBarService;
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel};
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_cell_status_bar_service::INotebookCellStatusBarService;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, ICell, INotebookCellStatusBarItemList, INotebookCellStatusBarItemProvider};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{ActiveTestNotebookEditorDelegate, empty_metadata, with_test_notebook};

/// O `editor` do caso.
struct VisibleCellsEditor {
    editor: Rc<ActiveTestNotebookEditorDelegate>,
    view_model: Rc<NotebookViewModel>,
}

impl IVisibleCellsEditor for VisibleCellsEditor {
    fn on_did_change_visible_ranges(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_model(&self) -> Event<Option<Rc<NotebookTextModel>>> {
        Event::none()
    }

    fn on_did_change_view_cells(&self) -> Event<()> {
        Event::none()
    }

    fn has_model(&self) -> bool {
        true
    }

    fn visible_ranges(&self) -> Vec<ICellRange> {
        self.editor.visible_ranges()
    }

    fn cell_at(&self, index: i64) -> Option<CellViewModel> {
        self.view_model.cell_at(index)
    }

    fn get_cell_by_handle(&self, handle: i64) -> Option<CellViewModel> {
        self.view_model.get_cell_by_handle(handle)
    }

    fn get_view_model(&self) -> Option<Rc<NotebookViewModel>> {
        Some(self.view_model.clone())
    }
}

/// O `new class extends Disposable implements INotebookCellStatusBarItemProvider` do caso.
struct TestProvider {
    provide_calls: Cell<i32>,
    on_provide_called: Rc<Emitter<i32>>,
    on_did_change_status_bar_items: Rc<Emitter<()>>,
    view_type: String,
}

impl INotebookCellStatusBarItemProvider for TestProvider {
    fn view_type(&self) -> String {
        self.view_type.clone()
    }

    fn on_did_change_status_bar_items(&self) -> Option<Event<()>> {
        Some(self.on_did_change_status_bar_items.event())
    }

    fn provide_cell_status_bar_items(&self, _uri: &URI, index: usize, _token: &CancellationToken) -> Promise<Option<INotebookCellStatusBarItemList>> {
        if index == 0 {
            self.provide_calls.set(self.provide_calls.get() + 1);
            self.on_provide_called.fire(self.provide_calls.get());
        }

        Promise::resolved(Some(INotebookCellStatusBarItemList { items: Vec::new(), dispose: None }))
    }
}

/// `asPromise(provider.onProvideCalled, message)`.
struct ProvideWatcher {
    value: Rc<Cell<Option<i32>>>,
    listener: DisposableRef,
    message: &'static str,
}

impl ProvideWatcher {
    fn new(provider: &TestProvider, message: &'static str) -> Self {
        let value = Rc::new(Cell::new(None));
        let slot = value.clone();
        let listener = provider.on_provide_called.event().once().listen(move |calls| slot.set(Some(*calls)));
        Self { value, listener, message }
    }

    /// `await promise`: avança os timers simulados um a um só até o evento chegar, como o `await` do original, que retoma
    /// logo após o primeiro disparo e deixa os timers seguintes para o próximo `await`.
    fn wait(&self) -> i32 {
        while self.value.get().is_none() && runtime::run_next_timer(true) {}
        self.listener.dispose();
        self.value.get().unwrap_or_else(|| panic!("asPromise TIMEOUT reached: {}", self.message))
    }
}

#[test]
fn calls_item_provider() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![
            ("var b = 1;", "javascript", CellKind::Code, Vec::new(), Some(empty_metadata())),
            ("# header a", "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata())),
        ],
        |editor, view_model, _ds, _services| {
            let cell_statusbar_svc = NotebookCellStatusBarService::new();
            let service: Rc<dyn INotebookCellStatusBarService> = cell_statusbar_svc.clone();
            let visible_editor: Rc<dyn IVisibleCellsEditor> = Rc::new(VisibleCellsEditor { editor: editor.clone(), view_model: view_model.clone() });
            let controller = ContributedStatusBarItemController::new(visible_editor, service.clone());

            let on_provide_called = Emitter::<i32>::new();
            let on_did_change_status_bar_items = Emitter::<()>::new();
            let provider = Rc::new(TestProvider {
                provide_calls: Cell::new(0),
                on_provide_called: on_provide_called.clone(),
                on_did_change_status_bar_items: on_did_change_status_bar_items.clone(),
                view_type: editor.text_model.notebook_type(),
            });
            let provide_promise1 = ProvideWatcher::new(&provider, "registering provider");
            let registration = service.register_cell_status_bar_item_provider(provider.clone());
            assert_eq!(provide_promise1.wait(), 1, "should call provider on registration");

            let provide_promise2 = ProvideWatcher::new(&provider, "updating metadata");
            let cell0 = editor.text_model.cells()[0].clone();
            let new_metadata = ObjectRef::new();
            for (key, value) in cell0.metadata().own_entries() {
                new_metadata.set(&key, value);
            }
            new_metadata.set(&JsString::from("newMetadata"), Dynamic::Bool(true));
            cell0.set_metadata(Dynamic::Object(new_metadata));
            assert_eq!(provide_promise2.wait(), 2, "should call provider on updating metadata");

            let provide_promise3 = ProvideWatcher::new(&provider, "changing cell language");
            cell0.set_language("newlanguage");
            assert_eq!(provide_promise3.wait(), 3, "should call provider on changing language");

            let provide_promise4 = ProvideWatcher::new(&provider, "manually firing change event");
            on_did_change_status_bar_items.fire(());
            assert_eq!(provide_promise4.wait(), 4, "should call provider on manually firing change event");

            registration.dispose();
            controller.dispose();
            cell_statusbar_svc.dispose();
            on_provide_called.dispose();
            on_did_change_status_bar_items.dispose();
        },
    );
}
