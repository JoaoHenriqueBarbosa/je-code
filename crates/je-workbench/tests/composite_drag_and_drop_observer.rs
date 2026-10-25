//! Porte de `src/vs/workbench/test/browser/compositeDragAndDropObserver.test.ts` (suíte `CompositeDragAndDropObserver`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo de cada caso; `store.add(...)` é
//!   `guard.add(...)`. O `CompositeDragAndDropObserver.INSTANCE` é `CompositeDragAndDropObserver::instance()`, um singleton marcado
//!   com `markAsSingleton`, então criá-lo dentro do caso não conta como vazamento.
//! - **DOM vira estado nativo.** O `document.createElement('div')` é `ViewElement::new()`. Os ouvintes de `addDisposableListener` e o
//!   `element.draggable` são o [`TestDragHost`] (o [`DragAndDropHost`] do caso), e `element.dispatchEvent(new DragEvent(type))` é
//!   `TestDragHost::dispatch_event(element, kind)`, que entrega o evento aos ouvintes ainda registrados do elemento. Os tipos
//!   `'dragstart'`, `'dragenter'`, `'dragover'`, `'dragleave'`, `'drop'` e `'dragend'` são os [`DragEventKind`] de mesmo nome.
//! - **Casos 1 e 2 (`WeakRef` e `gc`).** O original só roda com `--expose-gc` (senão `this.skip()`), porque a coleta de lixo é
//!   quem decide se o registro descartado ainda está retido. O Rust não tem coletor: um objeto fica vivo enquanto algum `Rc` o
//!   segura, então o `WeakRef` é o `Weak` do registro, e o `await timeout(0)` com `gc` é largar o último `Rc` do caso. A afirmação
//!   é a mesma (`'The observer still retains the disposed registration'`) e vale sempre, sem pular.
//! - **`registerTarget(element, callbacks)` e `registerDraggable(element, provider, callbacks)`** recebem o host do DOM como
//!   primeiro argumento (ver a nota de divergência do módulo `browser::dnd`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::{DisposableRef, plain_disposable};
use je_platform::contextview::view_element::ViewElement;
use je_workbench::browser::dnd::{
    CompositeCallback, CompositeDragAndDropObserver, CompositeDragAndDropObserverCallbacks, CompositeDragData, DragAndDropHost, DragEvent, DragEventHandler, DragEventKind,
    DraggedCompositeData, ViewType,
};

// region: dublês

struct RegisteredListener {
    id: u64,
    element: usize,
    kind: DragEventKind,
    handler: DragEventHandler,
}

fn key(element: &Rc<ViewElement>) -> usize {
    Rc::as_ptr(element) as usize
}

/// O DOM dos eventos de arrasto: os ouvintes de cada elemento e o `dispatchEvent`.
#[derive(Default)]
struct TestDragHost {
    listeners: Rc<RefCell<Vec<RegisteredListener>>>,
    next_id: Cell<u64>,
    draggable: RefCell<Vec<usize>>,
}

impl TestDragHost {
    fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// `element.dispatchEvent(new DragEvent(type))`.
    fn dispatch_event(&self, element: &Rc<ViewElement>, kind: DragEventKind) {
        let event = DragEvent::new(kind);
        let handlers: Vec<DragEventHandler> =
            self.listeners.borrow().iter().filter(|listener| listener.element == key(element) && listener.kind == kind).map(|listener| listener.handler.clone()).collect();
        for handler in handlers {
            handler(&event);
        }
    }

    fn is_draggable(&self, element: &Rc<ViewElement>) -> bool {
        self.draggable.borrow().contains(&key(element))
    }
}

impl DragAndDropHost for TestDragHost {
    fn add_listener(&self, element: &Rc<ViewElement>, kind: DragEventKind, handler: DragEventHandler) -> DisposableRef {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.listeners.borrow_mut().push(RegisteredListener { id, element: key(element), kind, handler });

        let listeners = self.listeners.clone();
        plain_disposable(move || listeners.borrow_mut().retain(|listener| listener.id != id))
    }

    fn set_draggable(&self, element: &Rc<ViewElement>, draggable: bool) {
        let mut elements = self.draggable.borrow_mut();
        elements.retain(|candidate| *candidate != key(element));
        if draggable {
            elements.push(key(element));
        }
    }
}

type Events = Rc<RefCell<Vec<String>>>;

/// `events.push(...)` num callback.
fn pushing(events: &Events, text: &str) -> CompositeCallback {
    let (events, text) = (events.clone(), text.to_owned());
    Rc::new(move |_: &DraggedCompositeData| events.borrow_mut().push(text.clone()))
}

/// `events.push(`${prefix}${e.dragAndDropData.getData().id}`)` num callback.
fn pushing_id(events: &Events, prefix: &str) -> CompositeCallback {
    let (events, prefix) = (events.clone(), prefix.to_owned());
    Rc::new(move |e: &DraggedCompositeData| events.borrow_mut().push(format!("{prefix}{}", e.drag_and_drop_data.get_data().id)))
}

/// `() => ({ type, id })`.
fn dragged_item(view_type: ViewType, id: &str) -> Rc<dyn Fn() -> CompositeDragData> {
    let id = id.to_owned();
    Rc::new(move || CompositeDragData { view_type, id: id.clone() })
}

fn host_of(host: &Rc<TestDragHost>) -> Rc<dyn DragAndDropHost> {
    host.clone()
}

// endregion

#[test]
fn releases_disposed_target_registrations() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestDragHost::new();
    let observer = CompositeDragAndDropObserver::instance();

    let registration = observer.register_target(&host_of(&host), &ViewElement::new(), CompositeDragAndDropObserverCallbacks::default());
    registration.dispose();
    let weak_registration = Rc::downgrade(&registration);
    drop(registration);

    assert!(weak_registration.upgrade().is_none(), "The observer still retains the disposed registration");
}

#[test]
fn releases_disposed_draggable_registrations() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestDragHost::new();
    let observer = CompositeDragAndDropObserver::instance();

    let registration =
        observer.register_draggable(&host_of(&host), &ViewElement::new(), dragged_item(ViewType::View, "test-view"), CompositeDragAndDropObserverCallbacks::default());
    registration.dispose();
    let weak_registration = Rc::downgrade(&registration);
    drop(registration);

    assert!(weak_registration.upgrade().is_none(), "The observer still retains the disposed registration");
}

#[test]
fn delivers_the_complete_drag_and_drop_lifecycle_to_a_live_target() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestDragHost::new();
    let observer = CompositeDragAndDropObserver::instance();
    let source = ViewElement::new();
    let target = ViewElement::new();
    let events: Events = Rc::new(RefCell::new(Vec::new()));

    guard.add(observer.register_draggable(&host_of(&host), &source, dragged_item(ViewType::View, "test-view"), CompositeDragAndDropObserverCallbacks::default()));
    guard.add(observer.register_target(
        &host_of(&host),
        &target,
        CompositeDragAndDropObserverCallbacks {
            on_drag_start: Some(pushing_id(&events, "start:")),
            on_drag_enter: Some(pushing(&events, "enter")),
            on_drag_over: Some(pushing(&events, "over")),
            on_drag_leave: Some(pushing(&events, "leave")),
            on_drop: Some(pushing_id(&events, "drop:")),
            on_drag_end: Some(pushing(&events, "end")),
        },
    ));
    assert!(host.is_draggable(&source));
    host.dispatch_event(&source, DragEventKind::DragStart);
    for kind in [DragEventKind::DragEnter, DragEventKind::DragOver, DragEventKind::DragLeave, DragEventKind::Drop] {
        host.dispatch_event(&target, kind);
    }
    host.dispatch_event(&source, DragEventKind::DragEnd);

    assert_eq!(*events.borrow(), vec!["start:test-view", "enter", "over", "leave", "drop:test-view", "end"]);
}

#[test]
fn disposing_a_target_repeatedly_removes_its_callbacks_but_preserves_a_live_peer() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestDragHost::new();
    let observer = CompositeDragAndDropObserver::instance();
    let source = ViewElement::new();
    let removed_target = ViewElement::new();
    let live_target = ViewElement::new();
    let events: Events = Rc::new(RefCell::new(Vec::new()));

    guard.add(observer.register_draggable(&host_of(&host), &source, dragged_item(ViewType::Composite, "test-composite"), CompositeDragAndDropObserverCallbacks::default()));
    let removed = guard.add(observer.register_target(
        &host_of(&host),
        &removed_target,
        CompositeDragAndDropObserverCallbacks {
            on_drag_start: Some(pushing(&events, "removed-start")),
            on_drop: Some(pushing(&events, "removed-drop")),
            on_drag_end: Some(pushing(&events, "removed-end")),
            ..CompositeDragAndDropObserverCallbacks::default()
        },
    ));
    guard.add(observer.register_target(
        &host_of(&host),
        &live_target,
        CompositeDragAndDropObserverCallbacks { on_drop: Some(pushing_id(&events, "")), ..CompositeDragAndDropObserverCallbacks::default() },
    ));
    removed.dispose();
    removed.dispose();
    host.dispatch_event(&source, DragEventKind::DragStart);
    host.dispatch_event(&removed_target, DragEventKind::Drop);
    host.dispatch_event(&live_target, DragEventKind::Drop);
    host.dispatch_event(&source, DragEventKind::DragEnd);

    assert_eq!(*events.borrow(), vec!["test-composite"]);
}

#[test]
fn a_disposed_draggable_no_longer_starts_drags_or_observes_another_draggable() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestDragHost::new();
    let observer = CompositeDragAndDropObserver::instance();
    let source = ViewElement::new();
    let peer = ViewElement::new();
    let events: Events = Rc::new(RefCell::new(Vec::new()));

    let removed = guard.add(observer.register_draggable(
        &host_of(&host),
        &source,
        dragged_item(ViewType::View, "removed"),
        CompositeDragAndDropObserverCallbacks {
            on_drag_start: Some(pushing(&events, "removed-start")),
            on_drag_end: Some(pushing(&events, "removed-end")),
            ..CompositeDragAndDropObserverCallbacks::default()
        },
    ));
    guard.add(observer.register_draggable(
        &host_of(&host),
        &peer,
        dragged_item(ViewType::View, "peer"),
        CompositeDragAndDropObserverCallbacks { on_drag_start: Some(pushing_id(&events, "")), ..CompositeDragAndDropObserverCallbacks::default() },
    ));
    removed.dispose();
    removed.dispose();
    host.dispatch_event(&source, DragEventKind::DragStart);
    host.dispatch_event(&source, DragEventKind::DragEnd);
    host.dispatch_event(&peer, DragEventKind::DragStart);
    host.dispatch_event(&peer, DragEventKind::DragEnd);

    assert_eq!(*events.borrow(), vec!["peer"]);
}
