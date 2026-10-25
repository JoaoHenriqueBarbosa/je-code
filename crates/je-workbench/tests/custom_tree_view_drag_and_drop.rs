//! Porte de `src/vs/workbench/test/browser/customTreeViewDragAndDrop.test.ts` (suíte `CustomTreeViewDragAndDrop`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso; `disposables.add(...)` é `add_rc`.
//! - `mock<ILabelService>()` é o [`MockLabelService`] (nenhum membro é usado). O `mock<IInstantiationService>()` não existe: o
//!   `fillEditorsDragData` do porte é o [`IDragDataTransfer::fill_editors_drag_data`].
//! - `Reflect.get(service, '_dragOperations')` é `TreeViewsDnDService::drag_operations`: `operations.size` é o tamanho da lista,
//!   `[...operations.keys()]` são as chaves na ordem de inserção e `Promise.all(operations.values())` espera cada promessa.
//! - `new DataTransfer()` e `new DragEvent(type, { dataTransfer })` são o [`TestDataTransfer`] (o `dropEffect` começa em `none`,
//!   como o do navegador) dentro de um [`DragEventData`]; o `successfulDragEnd()` é um `DataTransfer` com `dropEffect` `copy`.
//! - `DeferredPromise` é o par `Promise::new()` (a promessa e o resolvedor); `complete` é `resolve`.
//! - `await state.drag.drop(...)` é `drop(None, &evento)`: o `data`, o índice e o setor do original não são lidos.
//! - `item === state.transfer.get(...)` é a identidade do item: o item do teste é criado com o `id` `owned-transfer` (único, os
//!   itens vindos do `DataTransfer` não têm `id`), e a comparação é pelo `id`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::cancellation::CancellationToken;
use je_base::data_transfer::{VSDataTransfer, create_string_data_transfer_item};
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_editor::tree_views_dnd::{ITreeViewsDnDService, TreeViewsDnDService};
use je_platform::label::label::{
    FormatterChangeEvent, ILabelService, LabelFormatter, LabelSeparator, ResourceLabelFormatter, UriLabelOptions, WorkspaceLabelOptions, WorkspaceLabelTarget,
};
use je_platform::log::NullLogService;
use je_workbench::browser::parts::views::tree_view::{
    CustomTreeViewDragAndDrop, DragDataTransferItem, DragEventData, ElementsDragAndDropData, IDragDataTransfer, TreeViewDragAndDropServices,
};
use je_workbench::common::tree_view_contracts::ITreeViewDragAndDropController;
use je_workbench::common::views::{ITreeItem, ITreeItemLabel, TreeItemCollapsibleState};

const TRANSFER_MIME_TYPE: &str = "application/test-transfer";

type Transfer = Option<Rc<VSDataTransfer>>;

/// `mock<ILabelService>()`: nenhum membro é usado pelos casos.
struct MockLabelService;

impl ILabelService for MockLabelService {
    fn get_uri_label(&self, _resource: &URI, _options: &UriLabelOptions) -> String {
        panic!("getUriLabel is not a function");
    }

    fn get_uri_basename_label(&self, _resource: &URI) -> String {
        panic!("getUriBasenameLabel is not a function");
    }

    fn get_workspace_label(&self, _workspace: &WorkspaceLabelTarget, _options: Option<&WorkspaceLabelOptions>) -> String {
        panic!("getWorkspaceLabel is not a function");
    }

    fn get_host_label(&self, _scheme: &str, _authority: Option<&str>) -> String {
        panic!("getHostLabel is not a function");
    }

    fn get_host_tooltip(&self, _scheme: &str, _authority: Option<&str>) -> Option<String> {
        panic!("getHostTooltip is not a function");
    }

    fn get_separator(&self, _scheme: &str, _authority: Option<&str>) -> LabelSeparator {
        panic!("getSeparator is not a function");
    }

    fn get_uri_home(&self, _resource: &URI) -> Option<URI> {
        panic!("getUriHome is not a function");
    }

    fn register_formatter(&self, _formatter: LabelFormatter) -> DisposableRef {
        panic!("registerFormatter is not a function");
    }

    fn on_did_change_formatters(&self) -> Event<FormatterChangeEvent> {
        Event::none()
    }

    fn register_cached_formatter(&self, _formatter: ResourceLabelFormatter) -> DisposableRef {
        panic!("registerCachedFormatter is not a function");
    }
}

/// O `DataTransfer` do navegador: guarda os dados por tipo e o `dropEffect` (que começa em `none`).
struct TestDataTransfer {
    data: RefCell<Vec<(String, String)>>,
    drop_effect: RefCell<String>,
}

impl TestDataTransfer {
    fn new(drop_effect: &str) -> Rc<Self> {
        Rc::new(Self { data: RefCell::new(Vec::new()), drop_effect: RefCell::new(drop_effect.to_owned()) })
    }

    fn set_drop_effect(&self, drop_effect: &str) {
        *self.drop_effect.borrow_mut() = drop_effect.to_owned();
    }

    /// `new DragEvent(type, { dataTransfer })`.
    fn event(self: &Rc<Self>) -> DragEventData {
        let data_transfer: Rc<dyn IDragDataTransfer> = self.clone();
        DragEventData { data_transfer: Some(data_transfer) }
    }
}

impl IDragDataTransfer for TestDataTransfer {
    fn clear_data(&self, format: &str) {
        self.data.borrow_mut().retain(|(key, _)| key != format);
    }

    fn set_data(&self, format: &str, data: &str) {
        let mut entries = self.data.borrow_mut();
        entries.retain(|(key, _)| key != format);
        entries.push((format.to_owned(), data.to_owned()));
    }

    fn drop_effect(&self) -> String {
        self.drop_effect.borrow().clone()
    }

    fn items(&self) -> Vec<DragDataTransferItem> {
        self.data.borrow().iter().map(|(type_, _)| DragDataTransferItem { kind: "string".to_owned(), type_: type_.clone() }).collect()
    }

    fn to_external_vs_data_transfer(&self, _disable_file_entries: bool) -> VSDataTransfer {
        let mut transfer = VSDataTransfer::new();
        for (type_, data) in self.data.borrow().iter() {
            transfer.replace(type_, create_string_data_transfer_item(data.as_str(), None));
        }
        transfer
    }

    fn fill_editors_drag_data(&self, _resources: &[URI]) {}
}

/// O controlador de `drag.controller = { dragMimeTypes, dropMimeTypes, handleDrag, handleDrop, handleDragEnd }`.
struct TestController {
    provide_transfer: Box<dyn Fn(CancellationToken) -> Promise<Transfer>>,
    tokens: RefCell<Vec<CancellationToken>>,
    drops: RefCell<Vec<Rc<VSDataTransfer>>>,
    released: RefCell<Vec<String>>,
}

impl ITreeViewDragAndDropController for TestController {
    fn drop_mime_types(&self) -> Vec<String> {
        vec![TRANSFER_MIME_TYPE.to_owned()]
    }

    fn drag_mime_types(&self) -> Vec<String> {
        vec![TRANSFER_MIME_TYPE.to_owned()]
    }

    fn handle_drag(&self, _source_tree_item_handles: Vec<String>, _operation_uuid: &str, token: CancellationToken) -> Promise<Transfer> {
        self.tokens.borrow_mut().push(token.clone());
        (self.provide_transfer)(token)
    }

    fn handle_drag_end(&self, operation_uuid: &str) {
        self.released.borrow_mut().push(operation_uuid.to_owned());
    }

    fn handle_drop(
        &self,
        elements: Rc<VSDataTransfer>,
        _target: Option<ITreeItem>,
        _token: CancellationToken,
        _operation_uuid: Option<&str>,
        _source_tree_id: Option<&str>,
        _source_tree_item_handles: Option<Vec<String>>,
    ) -> Promise<()> {
        self.drops.borrow_mut().push(elements);
        Promise::resolved(())
    }
}

struct State {
    drag: Rc<CustomTreeViewDragAndDrop>,
    service: Rc<TreeViewsDnDService<Rc<VSDataTransfer>>>,
    transfer: Rc<VSDataTransfer>,
    controller: Rc<TestController>,
    data: ElementsDragAndDropData<ITreeItem>,
}

impl State {
    /// `state.operations`: as operações do serviço, na ordem de inserção.
    fn operations(&self) -> Vec<(String, Promise<Transfer>)> {
        self.service.drag_operations()
    }

    fn keys(&self) -> Vec<String> {
        self.operations().into_iter().map(|(key, _)| key).collect()
    }

    /// `Promise.all(state.operations.values())`.
    async fn settle_operations(&self) {
        for (_, operation) in self.operations() {
            operation.await.unwrap();
        }
    }

    /// `state.start()`.
    fn start(&self) -> Rc<TestDataTransfer> {
        let data_transfer = TestDataTransfer::new("none");
        self.drag.on_drag_start(&self.data, &data_transfer.event());
        data_transfer
    }

    fn canceled(&self, index: usize) -> bool {
        self.controller.tokens.borrow()[index].is_cancellation_requested()
    }

    /// `Array.from(state.drops[0]).some(([, item]) => item === state.transfer.get('application/test-transfer'))`.
    fn retains_transfer_item(&self) -> bool {
        let expected = self.transfer.get(TRANSFER_MIME_TYPE);
        let drops = self.controller.drops.borrow();
        drops[0].iter().any(|(_, item)| item.id.is_some() && expected.as_ref().is_some_and(|expected| item.id == expected.id))
    }
}

/// `successfulDragEnd()`: browsers reset `dropEffect` to `none` in synthetic events, so the event carries `copy`.
fn successful_drag_end() -> DragEventData {
    TestDataTransfer::new("copy").event()
}

fn create_drag(guard: &LeakGuard, provide_transfer: Option<Box<dyn Fn(CancellationToken) -> Promise<Transfer>>>) -> State {
    let service: Rc<TreeViewsDnDService<Rc<VSDataTransfer>>> = Rc::new(TreeViewsDnDService::new());
    let mut transfer = VSDataTransfer::new();
    transfer.replace(TRANSFER_MIME_TYPE, create_string_data_transfer_item("Owned transfer", Some("owned-transfer".to_owned())));
    let transfer = Rc::new(transfer);
    let provided = transfer.clone();
    let provide_transfer = provide_transfer.unwrap_or_else(|| Box::new(move |_| Promise::resolved(Some(provided.clone()))));
    let tree_views_dnd_service: Rc<dyn ITreeViewsDnDService<Rc<VSDataTransfer>>> = service.clone();
    let drag = guard.add_rc(CustomTreeViewDragAndDrop::new(
        "test-tree",
        TreeViewDragAndDropServices { label_service: Rc::new(MockLabelService), tree_views_dnd_service, log_service: NullLogService::shared() },
    ));
    let controller = Rc::new(TestController { provide_transfer, tokens: RefCell::new(Vec::new()), drops: RefCell::new(Vec::new()), released: RefCell::new(Vec::new()) });
    drag.set_controller(Some(controller.clone() as Rc<dyn ITreeViewDragAndDropController>));
    let item = ITreeItem {
        handle: "item".to_owned(),
        label: Some(ITreeItemLabel { label: StringOrMarkdown::String("Drag item".to_owned()), highlights: None }),
        collapsible_state: TreeItemCollapsibleState::None,
        ..ITreeItem::default()
    };
    State { drag, service, transfer, controller, data: ElementsDragAndDropData::new(vec![item]) }
}

#[test]
fn keeps_completed_data_available_while_its_drag_is_active() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let state = create_drag(&guard, None);
        state.start();
        let pending = state.operations();
        let value = pending[0].1.clone().await.unwrap().expect("a transferência");
        assert!(Rc::ptr_eq(&value, &state.transfer));
        assert_eq!((state.operations().len(), state.canceled(0)), (1, false));
    });
}

#[test]
fn releases_a_completed_transfer_when_the_drag_is_canceled() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let state = create_drag(&guard, None);
        let data_transfer = state.start();
        state.settle_operations().await;
        data_transfer.set_drop_effect("none");
        state.drag.on_drag_end(&data_transfer.event());
        assert_eq!((state.operations().len(), state.canceled(0)), (0, true));
    });
}

#[test]
fn does_not_restore_a_canceled_transfer_after_the_provider_completes() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (pending, resolver) = Promise::<Transfer>::new();
        let state = create_drag(&guard, Some(Box::new(move |_| pending.clone())));
        let data_transfer = state.start();
        let creation = state.operations();
        data_transfer.set_drop_effect("none");
        state.drag.on_drag_end(&data_transfer.event());
        resolver.resolve(Some(state.transfer.clone()));
        for (_, operation) in creation {
            operation.await.unwrap();
        }
        assert_eq!((state.operations().len(), state.canceled(0)), (0, true));
    });
}

#[test]
fn does_not_accumulate_completed_canceled_drag_records() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let state = create_drag(&guard, None);
        for _ in 0..37 {
            let data_transfer = state.start();
            state.settle_operations().await;
            data_transfer.set_drop_effect("none");
            state.drag.on_drag_end(&data_transfer.event());
        }
        assert_eq!(state.operations().len(), 0);
    });
}

#[test]
fn preserves_transfer_data_for_a_successful_drop() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let state = create_drag(&guard, None);
        let data_transfer = state.start();
        state.settle_operations().await;
        state.drag.drop(None, &data_transfer.event()).await.unwrap();
        state.drag.on_drag_end(&successful_drag_end());
        let retains_transfer_item = state.retains_transfer_item();
        assert_eq!((state.operations().len(), state.controller.drops.borrow().len(), retains_transfer_item), (0, 1, true));
    });
}

#[test]
fn releases_an_active_drag_when_the_view_is_disposed() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let state = create_drag(&guard, None);
        state.start();
        state.settle_operations().await;
        je_base::lifecycle::IDisposable::dispose(state.drag.as_ref());
        assert_eq!((state.operations().len(), state.canceled(0)), (0, true));
    });
}

#[test]
fn releases_only_its_own_canceled_transfer_and_notifies_its_original_controller_once() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let state = create_drag(&guard, None);
        let data_transfer = state.start();
        let uuid = state.keys()[0].clone();
        let unrelated: Promise<Transfer> = Promise::resolved(Some(Rc::new(VSDataTransfer::new())));
        state.service.add_drag_operation_transfer("unrelated", unrelated);
        state.settle_operations().await;
        state.drag.on_drag_end(&data_transfer.event());
        je_base::lifecycle::IDisposable::dispose(state.drag.as_ref());
        assert_eq!((state.keys(), state.controller.released.borrow().clone()), (vec!["unrelated".to_owned()], vec![uuid]));
    });
}

#[test]
fn releases_an_abandoned_operation_before_starting_another_drag() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let state = create_drag(&guard, None);
        state.start();
        let first_uuid = state.keys()[0].clone();
        state.settle_operations().await;
        state.start();
        let canceled: Vec<bool> = state.controller.tokens.borrow().iter().map(CancellationToken::is_cancellation_requested).collect();
        assert_eq!((state.operations().len(), canceled, state.controller.released.borrow().clone()), (1, vec![true, false], vec![first_uuid]));
    });
}

#[test]
fn releases_active_data_when_its_controller_is_removed() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let state = create_drag(&guard, None);
        state.start();
        let uuid = state.keys()[0].clone();
        state.settle_operations().await;
        state.drag.set_controller(None);
        assert_eq!((state.operations().len(), state.canceled(0), state.controller.released.borrow().clone()), (0, true, vec![uuid]));
    });
}

#[test]
fn keeps_pending_data_when_successful_drag_end_precedes_asynchronous_drop_completion() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (pending, resolver) = Promise::<Transfer>::new();
        let state = create_drag(&guard, Some(Box::new(move |_| pending.clone())));
        let data_transfer = state.start();
        let dropping = state.drag.drop(None, &data_transfer.event());
        state.drag.on_drag_end(&successful_drag_end());
        resolver.resolve(Some(state.transfer.clone()));
        dropping.await.unwrap();
        let retains_transfer_item = state.retains_transfer_item();
        assert_eq!(
            (state.operations().len(), state.controller.drops.borrow().len(), state.canceled(0), state.controller.released.borrow().clone(), retains_transfer_item),
            (0, 1, false, Vec::<String>::new(), true)
        );
    });
}
