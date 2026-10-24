//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookRendererMessagingService.test.ts` (suíte `NotebookRendererMessaging`), caso a caso e na mesma ordem. Os 2 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso, e o `setup` (o `NullExtensionService`, o serviço e o
//!   ouvinte de `onShouldPostMessage` que guarda em `sent`) é [`Suite::new`].
//! - `NullExtensionService` com `stub(extService, 'activateByEvent').returns(Promise.resolve())` é o [`RecordingExtensionService`]: o
//!   `activateByEvent` devolve a promessa resolvida e guarda os eventos (`activate.args`, sem o `activationKind`, que o serviço não passa).
//! - `activate.args` é `Vec<String>` dos eventos, e `sent` é a lista de `(editorId, rendererId, message)` com a `message` numérica.
//! - `await timeout(0)` é o `.await` do `timeout` de `je-base/tests/common`, num caso que roda com `run_with_real_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_extensions::scanner::ExtensionDescription;
use je_workbench::contrib::notebook::browser::services::notebook_renderer_messaging_service_impl::NotebookRendererMessagingService;
use je_workbench::contrib::notebook::common::notebook_renderer_messaging_service::{INotebookRendererMessagingService, IScopedRendererMessaging};
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};

/// `NullExtensionService` com o `activateByEvent` trocado por um `stub` que resolve e guarda os argumentos.
#[derive(Default)]
struct RecordingExtensionService {
    activated: RefCell<Vec<String>>,
}

impl IExtensionService for RecordingExtensionService {
    fn activate_by_event(&self, activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        self.activated.borrow_mut().push(activation_event.to_string());
        Promise::resolved(())
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        false
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        Promise::resolved(None)
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        Event::none()
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        Vec::new()
    }
}

/// `(editorId, rendererId, message)`.
type Sent = (String, String, f64);

/// O `setup` da suíte.
struct Suite {
    ext_service: Rc<RecordingExtensionService>,
    m: Rc<NotebookRendererMessagingService>,
    sent: Rc<RefCell<Vec<Sent>>>,
    _ds: LeakGuard,
}

impl Suite {
    fn new() -> Self {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let ext_service = Rc::new(RecordingExtensionService::default());
        let m = ds.add_rc(NotebookRendererMessagingService::new(ext_service.clone()));
        let sent: Rc<RefCell<Vec<Sent>>> = Rc::default();
        let collected = sent.clone();
        ds.add(m.on_should_post_message().listen(move |e| {
            let Dynamic::Number(message) = e.message else { panic!("a mensagem do caso é um número") };
            collected.borrow_mut().push((e.editor_id.clone(), e.renderer_id.clone(), message));
        }));
        Self { ext_service, m, sent, _ds: ds }
    }
}

#[test]
fn activates_on_prepare() {
    let suite = Suite::new();
    suite.m.prepare("foo");
    suite.m.prepare("foo");
    suite.m.prepare("foo");

    assert_eq!(*suite.ext_service.activated.borrow(), vec!["onRenderer:foo".to_string()]);
}

#[test]
fn buffers_and_then_plays_events() {
    run_with_real_timers(async move {
        let suite = Suite::new();

        let scoped = suite.m.get_scoped("some-editor");
        scoped.post_message("foo", Dynamic::Number(1.0));
        scoped.post_message("foo", Dynamic::Number(2.0));
        assert_eq!(*suite.sent.borrow(), Vec::<Sent>::new());

        timeout(0).await.unwrap();

        let expected: Vec<Sent> = vec![("some-editor".into(), "foo".into(), 1.0), ("some-editor".into(), "foo".into(), 2.0)];

        assert_eq!(*suite.sent.borrow(), expected);

        scoped.post_message("foo", Dynamic::Number(3.0));

        let mut with_third = expected.clone();
        with_third.push(("some-editor".into(), "foo".into(), 3.0));
        assert_eq!(*suite.sent.borrow(), with_third);
    });
}
