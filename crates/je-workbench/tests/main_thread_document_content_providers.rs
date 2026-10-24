//! Porte de `src/vs/workbench/api/test/browser/mainThreadDocumentContentProviders.test.ts` (suíte `MainThreadDocumentContentProviders`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (`store` é o guarda).
//! - `new TestRPCProtocol()` é o [`NoopContext`]: o proxy de `ExtHostDocumentContentProviders` nunca é chamado por estes casos (o
//!   `provideTextContent` só roda quando alguém resolve um modelo), então chamá-lo falha. `null!` do `ITextModelService` e do `ILanguageService`
//!   são o [`UnusedTextModelService`] e um `LanguageService` real (o trait `ILanguageService` não tem como ser `null`), nunca usados.
//! - `new class extends mock<IModelService>() { override getModel }` é o [`ModelServiceStub`]; os membros que o original deixa sem definição
//!   falham com `unreachable!`, como o `TypeError` do `mock`. O mesmo vale para o `IEditorWorkerService` ([`EditorWorkerServiceStub`]): só
//!   `computeMoreMinimalEdits` é definido.
//! - `new Promise((resolve, reject) => ...)` do primeiro caso é a promessa criada com `Promise::new`; o `try/catch` com `reject(err)` é o
//!   `resolver.reject`. O caso roda em `run_with_faked_timers` porque as promessas só andam com as microtarefas esvaziadas.
//! - `await new Promise(resolve => setTimeout(resolve, 10))` do segundo caso é `timeout(10)` no relógio virtual.
//! - `providers.$onVirtualDocumentChange(uri, '1\n2')` recebe a [`URI`] e o [`JsString`] diretamente.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::errors::Error;
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::languages::TextEdit;
use je_editor::editor_worker::{IEditorWorkerService, WordRanges};
use je_editor::language_service::{ILanguageSelection, ILanguageService, LanguageService};
use je_editor::model::{RelaxedTextModelCreationOptions, TextModelCreationOptions};
use je_editor::range::Range;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::text_model_edit_source::TextModelEditSource;
use je_workbench::api::browser::main_thread_document_content_providers::MainThreadDocumentContentProviders;
use je_workbench::api::common::ext_host_protocol::{ExtHostDocumentContentProvidersShape, IMainContext, MainThreadDocumentContentProvidersShape};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextModelService, TextModelReference};
use test_text_model::create_text_model_with;

/// `new TestRPCProtocol()`: o proxy que ninguém chama.
struct NoopContext;

struct UnusedProxy;

impl ExtHostDocumentContentProvidersShape for UnusedProxy {
    fn rpc_provide_text_document_content(&self, _handle: u64, _uri: &URI) -> Promise<Option<JsString>> {
        unreachable!("o proxy de ExtHostDocumentContentProviders não é chamado por estes casos")
    }
}

impl IMainContext for NoopContext {
    fn get_proxy_erased(&self, _sid: &'static str) -> Rc<dyn Any> {
        let proxy: Rc<dyn ExtHostDocumentContentProvidersShape> = Rc::new(UnusedProxy);
        Rc::new(proxy)
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

/// O `null!` que a suíte passa como `ITextModelService`: qualquer uso falha.
struct UnusedTextModelService;

impl ITextModelService for UnusedTextModelService {
    fn create_model_reference(&self, _resource: &URI) -> Promise<TextModelReference> {
        unreachable!("Cannot read properties of null (reading 'createModelReference')")
    }
}

/// `new class extends mock<IModelService>() { override getModel(_uri) { assert...; return model; } }`.
struct ModelServiceStub {
    uri: URI,
    model: Rc<TextModel>,
}

impl IModelService for ModelServiceStub {
    fn get_model(&self, resource: &URI) -> Option<Rc<TextModel>> {
        assert_eq!(self.uri.to_string(false), resource.to_string(false));
        Some(self.model.clone())
    }

    fn get_models(&self) -> Vec<Rc<TextModel>> {
        unreachable!("mock<IModelService>: get_models não é definido")
    }

    fn on_model_added(&self) -> Event<Rc<TextModel>> {
        unreachable!("mock<IModelService>: on_model_added não é definido")
    }

    fn create_model(&self, _value: TextSource, _language_selection: Option<&dyn ILanguageSelection>, _resource: Option<&URI>, _is_for_simple_widget: bool) -> Rc<TextModel> {
        unreachable!("mock<IModelService>: create_model não é definido")
    }

    fn update_model(&self, _model: &TextModel, _value: TextSource, _reason: Option<TextModelEditSource>) {
        unreachable!("mock<IModelService>: update_model não é definido")
    }

    fn destroy_model(&self, _resource: &URI) {
        unreachable!("mock<IModelService>: destroy_model não é definido")
    }

    fn get_creation_options(&self, _language: &str, _resource: Option<&URI>, _is_for_simple_widget: bool) -> TextModelCreationOptions {
        unreachable!("mock<IModelService>: get_creation_options não é definido")
    }
}

/// `new class extends mock<IEditorWorkerService>() { override computeMoreMinimalEdits }`: o fecho recebe o recurso e as edições.
struct EditorWorkerServiceStub {
    compute_more_minimal_edits: Box<dyn Fn(&URI, Option<Vec<TextEdit>>) -> Promise<Option<Vec<TextEdit>>>>,
}

impl IEditorWorkerService for EditorWorkerServiceStub {
    fn can_compute_word_ranges(&self, _resource: &URI) -> bool {
        unreachable!("mock<IEditorWorkerService>: can_compute_word_ranges não é definido")
    }

    fn compute_word_ranges(&self, _resource: &URI, _range: Range) -> Promise<Option<WordRanges>> {
        unreachable!("mock<IEditorWorkerService>: compute_word_ranges não é definido")
    }

    fn compute_more_minimal_edits(&self, resource: &URI, edits: Option<Vec<TextEdit>>) -> Promise<Option<Vec<TextEdit>>> {
        (self.compute_more_minimal_edits)(resource, edits)
    }
}

fn new_providers(uri: &URI, model: &Rc<TextModel>, editor_worker_service: EditorWorkerServiceStub) -> Rc<MainThreadDocumentContentProviders> {
    let language_service: Rc<dyn ILanguageService> = LanguageService::new(false);
    MainThreadDocumentContentProviders::new(
        &NoopContext,
        Rc::new(UnusedTextModelService),
        language_service,
        Rc::new(ModelServiceStub { uri: uri.clone(), model: model.clone() }),
        Rc::new(editor_worker_service),
    )
}

#[test]
fn events_are_processed_properly() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let uri = URI::parse("test:uri", false);
        let model = create_text_model_with("1", None, RelaxedTextModelCreationOptions::default(), Some(uri.clone()));

        let model_for_edits = model.clone();
        let providers = new_providers(
            &uri,
            &model,
            EditorWorkerServiceStub {
                compute_more_minimal_edits: Box::new(move |_uri, data| {
                    assert_eq!(model_for_edits.get_value(), "1");
                    Promise::resolved(data)
                }),
            },
        );

        store.add_rc(model.clone());
        store.add_rc(providers.clone());

        let (done, resolver) = Promise::<()>::new();
        let expected_events = Rc::new(Cell::new(1_i32));
        let listened_model = model.clone();
        store.add(model.on_did_change_content().listen(move |_e| {
            expected_events.set(expected_events.get() - 1);
            if expected_events.get() < 0 {
                resolver.reject(Error::new("expectedEvents < 0"));
            }
            if listened_model.get_value() == "1\n2\n3" {
                listened_model.dispose();
                resolver.resolve(());
            }
        }));
        let _first = providers.rpc_on_virtual_document_change(&uri, &JsString::from("1\n2"));
        let _second = providers.rpc_on_virtual_document_change(&uri, &JsString::from("1\n2\n3"));
        done.await.expect("os eventos foram processados");
    });
}

#[test]
fn model_disposed_during_async_operation() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let uri = URI::parse("test:disposed", false);
        let model = create_text_model_with("initial", None, RelaxedTextModelCreationOptions::default(), Some(uri.clone()));

        let dispose_model_during_edit = Rc::new(Cell::new(false));

        let model_for_edits = model.clone();
        let flag = dispose_model_during_edit.clone();
        let providers = new_providers(
            &uri,
            &model,
            EditorWorkerServiceStub {
                compute_more_minimal_edits: Box::new(move |_uri, data| {
                    // Simulate async operation
                    let model = model_for_edits.clone();
                    let flag = flag.clone();
                    timeout(10).then(move |_| {
                        // Dispose model during the async operation if flag is set
                        if flag.get() {
                            model.dispose();
                        }

                        data
                    })
                }),
            },
        );

        store.add_rc(model.clone());
        store.add_rc(providers.clone());

        // First call should work normally
        providers.rpc_on_virtual_document_change(&uri, &JsString::from("updated")).await.expect("a primeira chamada termina");
        assert_eq!(model.get_value(), "updated");

        // Second call should not throw even though model gets disposed during async operation
        dispose_model_during_edit.set(true);
        providers.rpc_on_virtual_document_change(&uri, &JsString::from("should not apply")).await.expect("a segunda chamada termina");

        // Model should be disposed and value unchanged
        assert!(model.is_disposed());
    });
}
