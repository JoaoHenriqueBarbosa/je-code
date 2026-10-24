//! Porte de `src/vs/workbench/api/test/browser/extHostDocumentSaveParticipant.test.ts` (suíte
//! `ExtHostDocumentSaveParticipant`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, depois do `setup` (no
//!   mocha o `setup` da suíte roda antes do gancho do guarda, então o `DisposableStore` do `ExtHostDocuments` não é
//!   rastreado). O corpo de cada caso (todos devolvem promessas do `$participateInSave`) roda em
//!   `run_with_faked_timers`, com o relógio virtual: os
//!   prazos de 5, 20 e 1500 ms e os `timeout(10)` e `timeout(100)` dos casos andam no tempo virtual.
//! - `new TestRPCProtocol()` é o [`TestRpcProtocol`] (o `SingleProxyRPCProtocol(null)` do original entrega `null` a
//!   qualquer proxy; aqui o contexto guarda o `MainThreadDocuments` que a criação do documento pede, um dublê que falha
//!   se for chamado). O `setup` monta o `ExtHostDocumentsAndEditors` com o documento `foo:bar` e o `ExtHostDocuments`.
//! - `mock<MainThreadBulkEditsShape>()` e o `mock<MainThreadTextEditorsShape>()` com `$tryApplyWorkspaceEdit` são o
//!   [`MockMainThreadBulkEdits`]: o fecho opcional faz o papel do método sobrescrito, e sem ele a chamada falha (o
//!   `TypeError` de método ausente). O `_edits.value` é o campo `value` do `SerializableObjectWithBuffers`.
//! - `nullExtensionDescription` é `null_extension_description()`; `participant.getOnWillSaveTextDocumentEvent(ext)(listener)`
//!   é `participant.get_on_will_save_text_document_event(&ext).listen(listener)`.
//! - `throw new Error(...)` num ouvinte é `errors::throw`. `event.waitUntil(p)` é `event.wait_until(p).unwrap()`, com
//!   `p` como `Promise<Dynamic>`: `timeout(n)` vira [`timeout_dynamic`] (o `timeout` do original resolve com `undefined`),
//!   `Promise.resolve([TextEdit...])` é `Promise::resolved` de um `Dynamic::Array` de `TextEdit::to_dynamic()` e
//!   `Promise.reject(new Error('dddd'))` é `Promise::rejected`.
//! - No caso `event delivery, immutable`, `event.document = null` não compila (o ouvinte recebe `&TextDocumentWillSaveEvent`
//!   e o campo não é mutável): o caso confere que o evento chegou. O `typeof event.waitUntil === 'function'` do caso
//!   `event delivery` é conferido em compilação (a atribuição de `wait_until` a um ponteiro de função).
//! - `undefined!` em `rangeOffset`, `rangeLength` e `eol` do `$acceptModelChanged` são `0`, `0` e `Eol::Lf` (o EOL do
//!   documento, que o espelho não troca); `detailedReason: undefined` é `None`.
//! - `assert.throws(() => event.waitUntil(timeout(10)))` é `assert!(event.wait_until(...).is_err())`.
//! - O `class extends NullLogService` do último caso é o [`FlagLogService`], que só marca `error_args`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::model::Eol;
use je_editor::range::Range as EditorRange;
use je_editor::text_model_events::ModelContentChange;
use je_platform::log::{LogArg, LogLevel, LogMessage, LogServiceRef, Logger, NullLogService};
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_document_save_participant::{ExtHostDocumentSaveParticipant, SaveParticipantThresholds, TextDocumentWillSaveEvent};
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    CreateDocumentOptions, DocumentsAndEditorsDelta, ExtHostDocumentSaveParticipantShape, ExtHostDocumentsAndEditorsShape, ExtHostDocumentsShape, IMainContext,
    IWorkspaceEditDto, MainContext, MainThreadBulkEditsShape, MainThreadDocumentsShape, ModelAddedData, OpenDocumentOptions, SerializableObjectWithBuffers,
    SerializedModelContentChangedEvent, WorkspaceEditEntryDto,
};
use je_workbench::api::common::ext_host_types::{EndOfLine, Position, TextDocumentSaveReason, TextEdit};
use je_workbench::api::common::ext_host_values::ApiValue;
use je_workbench::common::editor::SaveReason;
use je_workbench::services::extensions::extensions::null_extension_description;

/// `TestRPCProtocol`: os proxies são os objetos registrados por `set`.
#[derive(Default)]
struct TestRpcProtocol {
    objects: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
}

impl IMainContext for TestRpcProtocol {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        self.objects.borrow().get(sid).cloned().unwrap_or_else(|| panic!("o proxy '{sid}' não foi registrado"))
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        self.objects.borrow_mut().insert(sid, value);
    }
}

/// O `MainThreadDocuments` que nenhum método do caso chama.
struct UnusedMainThreadDocuments;

impl IDisposable for UnusedMainThreadDocuments {
    fn dispose(&self) {}
}

impl IExtHostDocumentSaveDelegate for UnusedMainThreadDocuments {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        panic!("o proxy não deveria ser chamado");
    }
}

impl MainThreadDocumentsShape for UnusedMainThreadDocuments {
    fn rpc_try_create_document(&self, _options: Option<CreateDocumentOptions>) -> Promise<URI> {
        panic!("o proxy não deveria ser chamado");
    }

    fn rpc_try_open_document(&self, _uri: &URI, _options: Option<OpenDocumentOptions>) -> Promise<URI> {
        panic!("o proxy não deveria ser chamado");
    }
}

type TryApplyWorkspaceEditFn = Box<dyn Fn(SerializableObjectWithBuffers<IWorkspaceEditDto>) -> Promise<bool>>;

/// O dublê de `MainThreadBulkEditsShape`: só o `$tryApplyWorkspaceEdit`, e só quando o caso o define.
#[derive(Default)]
struct MockMainThreadBulkEdits {
    try_apply_workspace_edit: Option<TryApplyWorkspaceEditFn>,
}

impl MockMainThreadBulkEdits {
    fn with(try_apply_workspace_edit: impl Fn(SerializableObjectWithBuffers<IWorkspaceEditDto>) -> Promise<bool> + 'static) -> Self {
        Self { try_apply_workspace_edit: Some(Box::new(try_apply_workspace_edit)) }
    }
}

impl IDisposable for MockMainThreadBulkEdits {
    fn dispose(&self) {}
}

impl MainThreadBulkEditsShape for MockMainThreadBulkEdits {
    fn rpc_try_apply_workspace_edit(
        &self,
        workspace_edit_dto: SerializableObjectWithBuffers<IWorkspaceEditDto>,
        _undo_redo_group_id: Option<f64>,
        _is_refactoring: Option<bool>,
    ) -> Promise<bool> {
        match &self.try_apply_workspace_edit {
            Some(try_apply_workspace_edit) => try_apply_workspace_edit(workspace_edit_dto),
            None => panic!("$tryApplyWorkspaceEdit é undefined"),
        }
    }
}

/// O `LogService` que só marca ter registrado um erro (o `override error` do último caso).
struct FlagLogService {
    did_log_something: Rc<Cell<bool>>,
}

impl IDisposable for FlagLogService {
    fn dispose(&self) {}
}

impl Logger for FlagLogService {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        Event::none()
    }

    fn get_level(&self) -> LogLevel {
        LogLevel::Info
    }

    fn set_level(&self, _level: LogLevel) {}
    fn trace_args(&self, _message: &str, _args: &[LogArg]) {}
    fn debug_args(&self, _message: &str, _args: &[LogArg]) {}
    fn info_args(&self, _message: &str, _args: &[LogArg]) {}
    fn warn_args(&self, _message: &LogMessage, _args: &[LogArg]) {}

    fn error_args(&self, _message: &LogMessage, _args: &[LogArg]) {
        self.did_log_something.set(true);
    }

    fn flush(&self) {}
}

/// `URI.parse('foo:bar')`.
fn resource() -> URI {
    URI::parse("foo:bar", false)
}

/// O `setup`: o documento `foo:bar` (versão 1, linhas `['foo']`) e o `ExtHostDocuments` sobre ele.
fn setup() -> Rc<ExtHostDocuments> {
    let rpc: Rc<dyn IMainContext> = Rc::new(TestRpcProtocol::default());
    let proxy: Rc<dyn MainThreadDocumentsShape> = Rc::new(UnusedMainThreadDocuments);
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, proxy);
    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(rpc.clone(), NullLogService::shared()));
    documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
        added_documents: Some(vec![ModelAddedData {
            is_dirty: false,
            language_id: "foo".to_owned(),
            uri: resource(),
            version_id: 1,
            lines: vec![JsString::from("foo")],
            eol: Eol::Lf,
            encoding: "utf8".to_owned(),
        }]),
        ..DocumentsAndEditorsDelta::default()
    });
    ExtHostDocuments::new(&*rpc, documents_and_editors)
}

/// `new ExtHostDocumentSaveParticipant(nullLogService, documents, mainThreadBulkEdits)`.
fn new_participant(documents: &Rc<ExtHostDocuments>, bulk_edits: MockMainThreadBulkEdits) -> ExtHostDocumentSaveParticipant {
    ExtHostDocumentSaveParticipant::new(NullLogService::shared(), documents.clone(), Rc::new(bulk_edits))
}

/// O mesmo, com os limites `{ timeout, errors }` do quarto argumento.
fn new_participant_with_thresholds(documents: &Rc<ExtHostDocuments>, timeout: u64, errors: u32) -> ExtHostDocumentSaveParticipant {
    ExtHostDocumentSaveParticipant::with_thresholds(
        NullLogService::shared(),
        documents.clone(),
        Rc::new(MockMainThreadBulkEdits::default()),
        SaveParticipantThresholds { timeout, errors },
    )
}

/// `timeout(n)` do original como o `Promise<any>` que `waitUntil` recebe (resolve com `undefined`).
fn timeout_dynamic(ms: u64) -> Promise<Dynamic> {
    timeout(ms).then(|()| Dynamic::Undefined)
}

/// `Promise.resolve([TextEdit...])`.
fn resolved_edits(edits: &[TextEdit]) -> Promise<Dynamic> {
    Promise::resolved(Dynamic::array(edits.iter().map(ApiValue::to_dynamic)))
}

/// `new Position(0, 0)`.
fn origin() -> Position {
    Position::new(0.0, 0.0).expect("(0, 0) é válida")
}

#[test]
fn no_listeners_no_problem() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant(&documents, MockMainThreadBulkEdits::default());
        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
    });
}

#[test]
fn event_delivery() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant(&documents, MockMainThreadBulkEdits::default());

        let event: Rc<RefCell<Option<TextDocumentWillSaveEvent>>> = Rc::default();
        let sub = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let event = event.clone();
            move |e| {
                *event.borrow_mut() = Some(e.clone());
            }
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub.dispose();

        let event = event.borrow().clone();
        let event = event.expect("o evento foi entregue");
        assert_eq!(event.reason, TextDocumentSaveReason::Manual);
        // `typeof event.waitUntil === 'function'`
        let _: fn(&TextDocumentWillSaveEvent, Promise<Dynamic>) -> Result<(), Error> = TextDocumentWillSaveEvent::wait_until;
    });
}

#[test]
fn event_delivery_immutable() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant(&documents, MockMainThreadBulkEdits::default());

        let event: Rc<RefCell<Option<TextDocumentWillSaveEvent>>> = Rc::default();
        let sub = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let event = event.clone();
            move |e| {
                *event.borrow_mut() = Some(e.clone());
            }
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub.dispose();

        // `event.document = null` não compila: o evento só é emprestado, e o campo não é mutável
        assert!(event.borrow().is_some());
    });
}

#[test]
fn event_delivery_bad_listener() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant(&documents, MockMainThreadBulkEdits::default());

        let sub = participant.get_on_will_save_text_document_event(&null_extension_description()).listen(|_| {
            errors::throw(Error::new("💀"));
        });

        let values = participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub.dispose();

        let first = values[0];
        assert!(!first);
    });
}

#[test]
fn event_delivery_bad_listener_doesnt_prevent_more_events() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant(&documents, MockMainThreadBulkEdits::default());

        let sub1 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen(|_| {
            errors::throw(Error::new("💀"));
        });
        let event: Rc<RefCell<Option<TextDocumentWillSaveEvent>>> = Rc::default();
        let sub2 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let event = event.clone();
            move |e| {
                *event.borrow_mut() = Some(e.clone());
            }
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub1.dispose();
        sub2.dispose();

        assert!(event.borrow().is_some());
    });
}

#[test]
fn event_delivery_in_subscriber_order() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant(&documents, MockMainThreadBulkEdits::default());

        let counter = Rc::new(Cell::new(0));
        let sub1 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let counter = counter.clone();
            move |_event| {
                assert_eq!(counter.get(), 0);
                counter.set(counter.get() + 1);
            }
        });

        let sub2 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let counter = counter.clone();
            move |_event| {
                assert_eq!(counter.get(), 1);
                counter.set(counter.get() + 1);
            }
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub1.dispose();
        sub2.dispose();
    });
}

#[test]
fn event_delivery_ignore_bad_listeners() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant_with_thresholds(&documents, 5, 1);

        let call_count = Rc::new(Cell::new(0));
        let sub = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let call_count = call_count.clone();
            move |_event| {
                call_count.set(call_count.get() + 1);
                errors::throw(Error::new("boom"));
            }
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();

        sub.dispose();
        assert_eq!(call_count.get(), 2);
    });
}

#[test]
fn event_delivery_overall_timeout() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant_with_thresholds(&documents, 20, 5);

        let calls: Rc<RefCell<Vec<i32>>> = Rc::default();
        let sub1 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let calls = calls.clone();
            move |_event| {
                calls.borrow_mut().push(1);
            }
        });

        let sub2 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let calls = calls.clone();
            move |event| {
                calls.borrow_mut().push(2);
                event.wait_until(timeout_dynamic(100)).unwrap();
            }
        });

        let sub3 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let calls = calls.clone();
            move |_event| {
                calls.borrow_mut().push(3);
            }
        });

        let values = participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub1.dispose();
        sub2.dispose();
        sub3.dispose();
        assert_eq!(*calls.borrow(), vec![1, 2]);
        assert_eq!(values.len(), 2);
    });
}

#[test]
fn event_delivery_wait_until() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant(&documents, MockMainThreadBulkEdits::default());

        let sub = participant.get_on_will_save_text_document_event(&null_extension_description()).listen(|event| {
            event.wait_until(timeout_dynamic(10)).unwrap();
            event.wait_until(timeout_dynamic(10)).unwrap();
            event.wait_until(timeout_dynamic(10)).unwrap();
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub.dispose();
    });
}

#[test]
fn event_delivery_wait_until_must_be_called_sync() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant(&documents, MockMainThreadBulkEdits::default());

        let sub = participant.get_on_will_save_text_document_event(&null_extension_description()).listen(|event| {
            let late_event = event.clone();
            let promise = Promise::<Dynamic>::with_executor(move |resolver| {
                runtime::set_timeout(
                    move || {
                        let outcome = errors::catch(|| {
                            assert!(late_event.wait_until(timeout_dynamic(10)).is_err());
                        });
                        match outcome {
                            Ok(()) => resolver.resolve(Dynamic::Undefined),
                            Err(error) => resolver.reject(error),
                        }
                    },
                    10,
                );
            });
            event.wait_until(promise).unwrap();
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub.dispose();
    });
}

#[test]
fn event_delivery_wait_until_will_timeout() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant_with_thresholds(&documents, 5, 3);

        let sub = participant.get_on_will_save_text_document_event(&null_extension_description()).listen(|event| {
            event.wait_until(timeout_dynamic(100)).unwrap();
        });

        let values = participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub.dispose();

        let first = values[0];
        assert!(!first);
    });
}

#[test]
fn event_delivery_wait_until_failure_handling() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant(&documents, MockMainThreadBulkEdits::default());

        let sub1 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen(|e| {
            e.wait_until(Promise::rejected(Error::new("dddd"))).unwrap();
        });

        let event: Rc<RefCell<Option<TextDocumentWillSaveEvent>>> = Rc::default();
        let sub2 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let event = event.clone();
            move |e| {
                *event.borrow_mut() = Some(e.clone());
            }
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        assert!(event.borrow().is_some());
        sub1.dispose();
        sub2.dispose();
    });
}

#[test]
fn event_delivery_push_edits_sync() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let dto: Rc<RefCell<Option<IWorkspaceEditDto>>> = Rc::default();
        let participant = new_participant(
            &documents,
            MockMainThreadBulkEdits::with({
                let dto = dto.clone();
                move |edits| {
                    *dto.borrow_mut() = Some(edits.value);
                    Promise::resolved(true)
                }
            }),
        );

        let sub = participant.get_on_will_save_text_document_event(&null_extension_description()).listen(|e| {
            e.wait_until(resolved_edits(&[TextEdit::insert(origin(), "bar")])).unwrap();
            e.wait_until(resolved_edits(&[TextEdit::set_end_of_line(EndOfLine::CRLF)])).unwrap();
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub.dispose();

        let dto = dto.borrow();
        let dto = dto.as_ref().expect("as edições foram aplicadas");
        assert_eq!(dto.edits.len(), 2);
        assert!(matches!(&dto.edits[0], WorkspaceEditEntryDto::Text(_)));
        assert!(matches!(&dto.edits[1], WorkspaceEditEntryDto::Text(_)));
    });
}

#[test]
fn event_delivery_concurrent_change() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let edits: Rc<RefCell<Option<IWorkspaceEditDto>>> = Rc::default();
        let participant = new_participant(
            &documents,
            MockMainThreadBulkEdits::with({
                let edits = edits.clone();
                move |applied| {
                    *edits.borrow_mut() = Some(applied.value);
                    Promise::resolved(true)
                }
            }),
        );

        let sub = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let documents = documents.clone();
            move |e| {
                // concurrent change from somewhere
                documents.rpc_accept_model_changed(
                    &resource(),
                    SerializedModelContentChangedEvent {
                        changes: vec![ModelContentChange { range: EditorRange::new(1, 1, 1, 1), range_offset: 0, range_length: 0, text: JsString::from("bar") }],
                        eol: Eol::Lf,
                        version_id: 2,
                        is_redoing: false,
                        is_undoing: false,
                        detailed_reason: None,
                        is_flush: false,
                        is_eol_change: false,
                    },
                    true,
                );

                e.wait_until(resolved_edits(&[TextEdit::insert(origin(), "bar")])).unwrap();
            }
        });

        let values = participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub.dispose();

        assert!(edits.borrow().is_none());
        assert!(!values[0]);
    });
}

#[test]
fn event_delivery_two_listeners_two_document_states() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let participant = new_participant(
            &documents,
            MockMainThreadBulkEdits::with({
                let documents = documents.clone();
                move |dto| {
                    for edit in &dto.value.edits {
                        let WorkspaceEditEntryDto::Text(edit) = edit else {
                            panic!("a edição deveria ser de texto");
                        };
                        let uri = edit.resource.clone();
                        let text = edit.text_edit.text.as_str();
                        let range = edit.text_edit.range;
                        let version_id = documents.get_document_data(&uri).expect("o documento existe").document().version() as i64 + 1;
                        documents.rpc_accept_model_changed(
                            &uri,
                            SerializedModelContentChangedEvent {
                                changes: vec![ModelContentChange { range, text: JsString::from(text), range_offset: 0, range_length: 0 }],
                                eol: Eol::Lf,
                                version_id,
                                is_redoing: false,
                                is_undoing: false,
                                detailed_reason: None,
                                is_flush: false,
                                is_eol_change: false,
                            },
                            true,
                        );
                    }

                    Promise::resolved(true)
                }
            }),
        );

        let document = documents.get_document(&resource()).unwrap();

        let sub1 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let document = document.clone();
            move |e| {
                // the document state we started with
                assert_eq!(document.version(), 1.0);
                assert_eq!(document.get_text(None), "foo");

                e.wait_until(resolved_edits(&[TextEdit::insert(origin(), "bar")])).unwrap();
            }
        });

        let sub2 = participant.get_on_will_save_text_document_event(&null_extension_description()).listen({
            let document = document.clone();
            move |e| {
                // the document state AFTER the first listener kicked in
                assert_eq!(document.version(), 2.0);
                assert_eq!(document.get_text(None), "barfoo");

                e.wait_until(resolved_edits(&[TextEdit::insert(origin(), "bar")])).unwrap();
            }
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub1.dispose();
        sub2.dispose();

        // the document state AFTER eventing is done
        assert_eq!(document.version(), 3.0);
        assert_eq!(document.get_text(None), "barbarfoo");
    });
}

#[test]
fn log_failing_listener() {
    let documents = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let did_log_something = Rc::new(Cell::new(false));
        let log_service: LogServiceRef = Rc::new(FlagLogService { did_log_something: did_log_something.clone() });
        let participant = ExtHostDocumentSaveParticipant::new(log_service, documents.clone(), Rc::new(MockMainThreadBulkEdits::default()));

        let sub = participant.get_on_will_save_text_document_event(&null_extension_description()).listen(|_e| {
            errors::throw(Error::new("boom"));
        });

        participant.rpc_participate_in_save(&resource(), SaveReason::Explicit).await.unwrap();
        sub.dispose();
        assert!(did_log_something.get());
    });
}
