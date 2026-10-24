//! Porte de `src/vs/workbench/api/test/browser/extHostDecorations.test.ts` (suíte `ExtHostDecorations`), caso a caso e na mesma ordem.
//!
//! PORTADOS: o único caso do original (`SCM Decorations missing #100524`). Nenhum fica de fora.
//!
//! Notas de tradução:
//! - `new class extends mock<IExtHostRpcService>() { override getProxy() { return mainThreadShape; } }` é o [`TestMainContext`]
//!   com o dublê registrado em [`MainContext::MAIN_THREAD_DECORATIONS`]. O dublê de `MainThreadDecorationsShape` só implementa
//!   `$registerDecorationProvider` (que guarda o `handle` em `providers`); o resto falha com `panic!`, como o `mock<T>()`.
//! - `providers` (um `Set<number>` na ordem de inserção) é um `Vec<u32>`: os `handle` são únicos.
//! - `Promise.race([first, timeout(30).then(() => false)])` e o `typeof` do resultado (`'boolean'` para o que não terminou, `'object'`
//!   para a resposta) são o enum [`Raced`] com a resposta ou o fim do prazo.
//! - `new Promise(() => { })` (a que nunca termina) é `Promise::with_executor(|_| {})`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` sem `store` é o guarda criado no começo do caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_decorations::{ExtHostDecorations, FileDecorationProvider};
use je_workbench::api::common::ext_host_protocol::{DecorationReply, DecorationRequest, ExtHostDecorationsShape, MainContext, MainThreadDecorationsShape};
use je_workbench::api::common::ext_host_types::{FileDecoration, FileDecorationBadge};
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_workspace_support::TestMainContext;

/// `mainThreadShape = new class extends mock<MainThreadDecorationsShape>() { override $registerDecorationProvider(handle) { providers.add(handle); } }`.
struct RecordingMainThreadDecorations {
    providers: Rc<RefCell<Vec<u32>>>,
}

impl IDisposable for RecordingMainThreadDecorations {
    fn dispose(&self) {}
}

impl MainThreadDecorationsShape for RecordingMainThreadDecorations {
    fn rpc_register_decoration_provider(&self, handle: u32, _label: &str) {
        self.providers.borrow_mut().push(handle);
    }

    fn rpc_unregister_decoration_provider(&self, _handle: u32) {
        panic!("$unregisterDecorationProvider is not a function")
    }

    fn rpc_on_did_change(&self, _handle: u32, _resources: Option<Vec<URI>>) {
        panic!("$onDidChange is not a function")
    }
}

/// O provedor do original: um objeto com `provideFileDecoration()`.
struct FnFileDecorationProvider(Box<dyn Fn() -> MaybePromise<Option<FileDecoration>>>);

impl FileDecorationProvider for FnFileDecorationProvider {
    fn provide_file_decoration(&self, _uri: &URI, _token: CancellationToken) -> MaybePromise<Option<FileDecoration>> {
        (self.0)()
    }
}

/// O resultado de `Promise.race([request, timeout(30).then(() => false)])`.
#[derive(Clone)]
enum Raced {
    Reply(DecorationReply),
    TimedOut,
}

#[test]
fn scm_decorations_missing_100524() {
    run_with_faked_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        // setup
        let providers: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
        let main_context = TestMainContext::new();
        main_context.set(
            &MainContext::MAIN_THREAD_DECORATIONS,
            Rc::new(RecordingMainThreadDecorations { providers: Rc::clone(&providers) }) as Rc<dyn MainThreadDecorationsShape>,
        );
        let ext_host_decorations = ExtHostDecorations::new(&main_context, NullLogService::shared());

        let called_a = Rc::new(Cell::new(false));
        let called_b = Rc::new(Cell::new(false));

        // never returns
        let flag = Rc::clone(&called_a);
        let _ = ext_host_decorations.register_file_decoration_provider(
            Rc::new(FnFileDecorationProvider(Box::new(move || {
                flag.set(true);
                MaybePromise::Promise(Promise::with_executor(|_| {}))
            }))),
            &null_extension_description(),
        );

        // always returns
        let flag = Rc::clone(&called_b);
        let _ = ext_host_decorations.register_file_decoration_provider(
            Rc::new(FnFileDecorationProvider(Box::new(move || {
                flag.set(true);
                MaybePromise::Promise(Promise::resolved(Some(FileDecoration::new(Some(FileDecorationBadge::Text("H".to_owned())), Some("Hello"), None))))
            }))),
            &null_extension_description(),
        );

        let requests: Vec<Promise<DecorationReply>> = providers
            .borrow()
            .iter()
            .enumerate()
            .map(|(idx, handle)| {
                ext_host_decorations.rpc_provide_decorations(
                    *handle,
                    vec![DecorationRequest { id: idx as u32, uri: URI::parse("test:///file", false) }],
                    CancellationToken::None,
                )
            })
            .collect();

        assert!(called_a.get());
        assert!(called_b.get());

        assert_eq!(requests.len(), 2);
        let (first, second) = (requests[0].clone(), requests[1].clone());

        let first_result = Promise::race(vec![first.then(Raced::Reply), timeout(30).then(|_| Raced::TimedOut)]).await.expect("race");
        assert!(matches!(first_result, Raced::TimedOut)); // never finishes...

        let second_result = Promise::race(vec![second.then(Raced::Reply), timeout(30).then(|_| Raced::TimedOut)]).await.expect("race");
        assert!(matches!(second_result, Raced::Reply(_)));

        timeout(30).await.expect("timeout");
    });
}
