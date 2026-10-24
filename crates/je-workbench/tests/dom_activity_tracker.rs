//! Porte de `src/vs/workbench/services/userActivity/test/browser/domActivityTracker.test.ts` (suíte `DomActivityTracker`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `sinon.useFakeTimers()` e `clock.tick(ms)` viram `runtime::with_faked_timers` e `runtime::advance_fake_time`; o `setup`
//!   e o `teardown` viram o corpo e o fim de `run`. `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard`, que
//!   confere os vazamentos ao sair do corpo.
//! - `new TestInstantiationService()` é o `InstantiationService` sobre uma coleção vazia (o teste não registra serviço).
//! - **O DOM.** O `document` do original é o [`TestDocument`], que guarda os ouvintes por tipo de evento
//!   (`InputWindow::add_listener`) e entrega `dispatch(kind)` a quem estiver ouvindo; o `document.dispatchEvent(new
//!   MouseEvent('keydown'))` é `document.dispatch(InputEventKind::KeyDown)`. O rastreador só olha o tipo do evento (o
//!   `MouseEvent` com o nome `keydown` é só um jeito de criar um evento com esse tipo), então o tipo carrega tudo.
//!   Como o `mainWindow` do original é um só, o registro é o `SingleWindowRegistry` sobre esse documento.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::{to_disposable, DisposableRef};
use je_base::runtime;
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_workbench::services::user_activity::dom_activity_tracker::{DomActivityTracker, InputEventKind, InputWindow, SingleWindowRegistry};
use je_workbench::services::user_activity::user_activity_service::{IUserActivityService, UserActivityService};

/// `maxTimeToBecomeIdle`: `(MIN_INTERVALS_WITHOUT_ACTIVITY + 1) * CHECK_INTERVAL`.
const MAX_TIME_TO_BECOME_IDLE: u64 = 3 * 30_000;

fn tick(ms: u64) {
    runtime::advance_fake_time(ms);
}

/// O `document` do teste: os ouvintes registrados e o `dispatchEvent`.
#[derive(Default)]
struct TestDocument {
    listeners: RefCell<Vec<(usize, InputEventKind, Rc<dyn Fn()>)>>,
    next_id: Cell<usize>,
}

impl TestDocument {
    /// `document.dispatchEvent(new MouseEvent(kind))`.
    fn dispatch(&self, kind: InputEventKind) {
        let targets: Vec<Rc<dyn Fn()>> = self.listeners.borrow().iter().filter(|(_, k, _)| *k == kind).map(|(_, _, l)| l.clone()).collect();
        for listener in targets {
            listener();
        }
    }

    fn listener_count(&self) -> usize {
        self.listeners.borrow().len()
    }
}

struct DocumentHandle(Rc<TestDocument>);

impl InputWindow for DocumentHandle {
    fn add_listener(&self, kind: InputEventKind, listener: Rc<dyn Fn()>) -> DisposableRef {
        let document = self.0.clone();
        let id = document.next_id.get();
        document.next_id.set(id + 1);
        document.listeners.borrow_mut().push((id, kind, listener));
        to_disposable(move || document.listeners.borrow_mut().retain(|(i, _, _)| *i != id))
    }
}

/// `setup`, o corpo do teste e `teardown`.
fn run(body: impl FnOnce(&dyn IUserActivityService, &TestDocument)) {
    runtime::with_faked_timers(|| {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let document = Rc::new(TestDocument::default());
        let instantiation = store.add_rc(InstantiationService::new(ServiceCollection::new()));
        let uas = store.add_rc(UserActivityService::create((), (instantiation,)));
        let registry = store.add_rc(SingleWindowRegistry::new(Rc::new(DocumentHandle(document.clone()))));
        let uas_service: Rc<dyn IUserActivityService> = uas.clone();
        store.add_rc(DomActivityTracker::new(uas_service, registry));
        body(uas.as_ref(), &document);
        // o rastreador ouve os três tipos de evento no documento enquanto está vivo
        assert_eq!(document.listener_count(), 3);
    });
}

#[test]
fn marks_inactive_on_no_input() {
    run(|uas, _document| {
        assert!(uas.is_active());
        tick(MAX_TIME_TO_BECOME_IDLE);
        assert!(!uas.is_active());
    });
}

#[test]
fn preserves_activity_state_when_active() {
    run(|uas, document| {
        assert!(uas.is_active());

        let div = 10;
        for _ in 0..div {
            document.dispatch(InputEventKind::KeyDown);
            tick(MAX_TIME_TO_BECOME_IDLE / div);
        }

        assert!(uas.is_active());
    });
}

#[test]
fn restores_active_state() {
    run(|uas, document| {
        assert!(uas.is_active());
        tick(MAX_TIME_TO_BECOME_IDLE);
        assert!(!uas.is_active());

        document.dispatch(InputEventKind::KeyDown);
        assert!(uas.is_active());

        tick(MAX_TIME_TO_BECOME_IDLE);
        assert!(!uas.is_active());
    });
}
