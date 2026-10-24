//! Porte de `src/vs/workbench/services/workingCopy/test/browser/workingCopyEditorService.test.ts` (suíte `WorkingCopyEditorService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `setup` registra o painel de `registerTestResourceEditor` ([`setup`]); o `teardown`
//!   (`disposables.clear()`) é o `drop` do `LeakGuard` ao fim do corpo, que também faz o
//!   `ensureNoDisposablesAreLeakedInTestSuite()`. O `disposables` interno do caso `findEditor` (um segundo `DisposableStore`
//!   descartado no fim do corpo) é o próprio guarda, descartado por `drop(disposables)`.
//! - **`workbenchInstantiationService`, `createEditorPart` e `EditorService`.** São o [`EditorServiceAccessor`] (as divergências
//!   dele estão em `support/editor_service_support.rs`): a parte e o `EditorService` real já vêm criados nele, em vez de o caso
//!   criar um segundo `EditorService` por `createInstance(EditorService, undefined)`; o `instantiationService.stub(
//!   IEditorGroupsService, part)` é o `PartGroupsService` do acessor.
//! - **`IWorkingCopyEditorHandler`.** O objeto literal é o [`TestHandler`], com uma função por membro. Uma working copy passada
//!   a `handles` e `isOpen` chega como [`WorkingCopyIdentifier`] (no original ela própria é o identificador): o `workingCopy ===
//!   testWorkingCopy` é a igualdade com o identificador da working copy de teste. O `throw new Error()` de `createEditor` é
//!   `panic!`.
//! - **`handlerEvent`.** O `assert.strictEqual(handlerEvent, editorHandler)` é a identidade do ponteiro do handler.
//! - **`new TestEditorService()`.** Não é descartável aqui (não guarda estado), então não passa por `disposables.add`.
//! - **`disposables.add(instantiationService.createInstance(UntitledTextEditorInput, model))`.** É `create_instance` com o modelo
//!   que `accessor.untitledTextEditorService.create({ initialValue: 'foo' })` devolve, registrado para descarte.
//! - **Relógio.** O original não usa timers falsos: o caso assíncrono roda em `run_with_real_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::promise::MaybePromise;
use je_base::uri::URI;
use je_workbench::common::editor::{EditorInputRef, EditorInputWithOptions};
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use je_workbench::services::untitled::untitled_text_editor_service::{IUntitledTextEditorService, UntitledTextEditorOptions};
use je_workbench::services::working_copy::working_copy::WorkingCopyIdentifier;
use je_workbench::services::working_copy::working_copy_editor_service::{IWorkingCopyEditorHandler, IWorkingCopyEditorService, WorkingCopyEditorService};
use je_workbench::test::browser::workbench_test_services::TestEditorService;
use je_workbench::test::common::workbench_test_services::TestWorkingCopy;
use support::editor_service_support::{EditorServiceAccessor, register_test_resource_editor};

//#region Utilitários dos casos

/// O `setup` da suíte.
fn setup(disposables: &LeakGuard) {
    disposables.add(register_test_resource_editor());
}

type HandlesFn = Box<dyn Fn(&WorkingCopyIdentifier) -> bool>;
type IsOpenFn = Box<dyn Fn(&WorkingCopyIdentifier, &EditorInputRef) -> bool>;

/// O objeto literal `IWorkingCopyEditorHandler` dos casos: `createEditor` sempre lança (`throw new Error()`).
struct TestHandler {
    handles: HandlesFn,
    is_open: IsOpenFn,
}

impl IWorkingCopyEditorHandler for TestHandler {
    fn handles(&self, working_copy: &WorkingCopyIdentifier) -> MaybePromise<bool> {
        MaybePromise::Value((self.handles)(working_copy))
    }

    fn is_open(&self, working_copy: &WorkingCopyIdentifier, editor: &EditorInputRef) -> bool {
        (self.is_open)(working_copy, editor)
    }

    fn create_editor(&self, _working_copy: &WorkingCopyIdentifier) -> MaybePromise<EditorInputRef> {
        panic!("Error")
    }
}

/// `URI.parse(value)`.
fn parse_uri(value: &str) -> URI {
    URI::parse(value, false)
}

//#endregion

#[test]
fn registry_basics() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    setup(&disposables);

    let service = disposables.add_rc(WorkingCopyEditorService::new(Rc::new(TestEditorService)));

    let handler_event: Rc<RefCell<Option<Rc<dyn IWorkingCopyEditorHandler>>>> = Rc::default();
    {
        let handler_event = handler_event.clone();
        disposables.add(service.on_did_register_handler().listen(move |handler| {
            *handler_event.borrow_mut() = Some(handler.clone());
        }));
    }

    let editor_handler = Rc::new(TestHandler { handles: Box::new(|_working_copy| false), is_open: Box::new(|_working_copy, _editor| false) });
    let editor_handler_ref: Rc<dyn IWorkingCopyEditorHandler> = editor_handler;

    disposables.add(service.register_handler(editor_handler_ref.clone()));

    let received = handler_event.borrow().clone().expect("o evento disparou");
    assert!(std::ptr::addr_eq(Rc::as_ptr(&received), Rc::as_ptr(&editor_handler_ref)));

    drop(received);
    drop(handler_event);
    drop(disposables);
}

#[test]
fn find_editor() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&disposables);

        let accessor = EditorServiceAccessor::with_default_backup_service(&disposables.store());
        disposables.add_rc(accessor.files());
        let editor_service: Rc<dyn IEditorService> = accessor.editor_service.clone();

        let service = disposables.add_rc(WorkingCopyEditorService::new(editor_service));

        let resource = parse_uri("custom://some/folder/custom.txt");
        let test_working_copy = disposables.add_rc(TestWorkingCopy::new(resource, Some(false), Some("testWorkingCopyTypeId1")));
        let test_working_copy_identifier = WorkingCopyIdentifier::of(test_working_copy.as_ref());

        assert!(service.find_editor(test_working_copy.as_ref()).is_none());

        let editor_handler = Rc::new(TestHandler {
            handles: Box::new({
                let identifier = test_working_copy_identifier.clone();
                move |working_copy| *working_copy == identifier
            }),
            is_open: Box::new({
                let identifier = test_working_copy_identifier.clone();
                move |working_copy, _editor| *working_copy == identifier
            }),
        });
        let editor_handler_ref: Rc<dyn IWorkingCopyEditorHandler> = editor_handler;

        disposables.add(service.register_handler(editor_handler_ref));

        let new_untitled_editor = |disposables: &LeakGuard| -> EditorInputRef {
            let model = IUntitledTextEditorService::create(
                accessor.untitled_text_editor_service.as_ref(),
                Some(UntitledTextEditorOptions { initial_value: Some("foo".to_string()), ..UntitledTextEditorOptions::default() }),
            );
            let input = disposables.add_rc(accessor.instantiation_service.create_instance::<UntitledTextEditorInput>((model,)));
            let editor: EditorInputRef = input;

            editor
        };
        let editor1 = new_untitled_editor(&disposables);
        let editor2 = new_untitled_editor(&disposables);

        IEditorService::open_editors(
            accessor.editor_service.as_ref(),
            vec![EditorInputWithOptions { editor: editor1, options: None }, EditorInputWithOptions { editor: editor2, options: None }],
        )
        .await
        .unwrap();

        assert!(service.find_editor(test_working_copy.as_ref()).is_some());

        drop(disposables);
    });
}
