//! Porte de `src/vs/workbench/test/browser/parts/editor/editorDiffModel.test.ts` (suíte `TextDiffEditorModel`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de cada caso
//!   (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois: o
//!   `workbenchInstantiationService`, o `TestServiceAccessor` e o `TextModelResolverService` que ele registra como
//!   `ITextModelService` (a mesma montagem de `text_model_resolver_service.rs`). O `teardown` (`disposables.clear()`) roda quando o
//!   guarda sai de escopo, antes de ele conferir os vazamentos. O caso roda em tempo real (`run_with_real_timers`) e `await` é
//!   `.await.unwrap()`.
//! - **`ITextModelContentProvider` literal.** O objeto `{ provideTextContent }` é o [`ContentProvider`], que guarda o fecho.
//! - **`URI.from({ scheme: 'test', authority: null!, path: 'thePath' })`.** É [`uri_of`] (a autoridade ausente é o `None`).
//! - **`instantiationService.createInstance(TextResourceEditorInput, ...)` e `createInstance(DiffEditorInput, ...)`.** A primeira é o
//!   `create_instance` do serviço de instanciação do acessor, com os argumentos estáticos `(resource, name, description,
//!   preferredLanguageId, preferredContents)`. A `DiffEditorInput` do porte recebe o `IEditorService` direto no construtor (o
//!   `TestEditorService`, o único serviço que ela injeta), com o `forceOpenAsBinary` `None` do `undefined`.
//! - **`model instanceof TextDiffEditorModel`.** É o `downcast_ref` por `IEditorModel::as_any`; `assert(diffEditorModel.original)`
//!   e `assert(diffEditorModel.modified)` são sempre verdadeiros porque o [`IDiffEditorModel`] do porte guarda os dois modelos de
//!   texto sem `Option` (o tipo já garante a presença), e a leitura delas confere isso em tempo de compilação.
//! - **`assert(diffEditorModel !== model.textDiffEditorModel)`.** É a comparação de identidade do `Rc` (`Rc::ptr_eq`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::{URI, UriComponents};
use je_editor::language_service::ILanguageService;
use je_editor::text_model::TextModel;
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_workbench::common::editor::diff_editor_input::DiffEditorInput;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::text_diff_editor_model::TextDiffEditorModel;
use je_workbench::common::editor::text_resource_editor_input::TextResourceEditorInput;
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::services::environment::environment_service::{IWorkbenchEnvironmentService, I_WORKBENCH_ENVIRONMENT_SERVICE};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextModelContentProvider, ITextModelService, I_TEXT_MODEL_SERVICE};
use je_workbench::services::textmodel_resolver::text_model_resolver_service::TextModelResolverService;
use je_workbench::test::browser::workbench_test_services::{TestEditorService, TestEnvironmentService};
use support::text_file_support::TextFileTestAccessor;

/// O que o `setup` do original monta: o `accessor` e o `TextModelResolverService` dele (`accessor.textModelResolverService`).
struct Fixture {
    accessor: TextFileTestAccessor,
    resolver: Rc<TextModelResolverService>,
}

/// `setup`: `workbenchInstantiationService(undefined, disposables)` e `instantiationService.createInstance(TestServiceAccessor)`.
fn setup(disposables: &LeakGuard) -> Fixture {
    let accessor = TextFileTestAccessor::new(&disposables.store());
    disposables.add_rc(accessor.files());

    // `instantiationService.stub(ITextModelService, disposables.add(instantiationService.createInstance(TextModelResolverService)))`
    let uri_identity_service: Rc<dyn IUriIdentityService> = accessor.uri_identity_service.clone();
    accessor.collection.set(I_URI_IDENTITY_SERVICE, uri_identity_service);
    // O `workbenchInstantiationService` registra o `IWorkbenchEnvironmentService` (o `UntitledTextEditorInput` o injeta).
    accessor.collection.set(I_WORKBENCH_ENVIRONMENT_SERVICE, TestEnvironmentService::new() as Rc<dyn IWorkbenchEnvironmentService>);
    let resolver = disposables.add_rc(accessor.instantiation_service.create_instance::<TextModelResolverService>(()));
    let text_model_service: Rc<dyn ITextModelService> = resolver.clone();
    accessor.collection.set(I_TEXT_MODEL_SERVICE, text_model_service);

    Fixture { accessor, resolver }
}

/// `URI.from({ scheme, authority: null!, path })`.
fn uri_of(scheme: &str, path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(scheme)), path: Some(JsString::from(path)), ..UriComponents::default() }, false)
}

/// O `ITextModelContentProvider` literal `{ provideTextContent: ... }`.
struct ContentProvider<F: Fn(&URI) -> MaybePromise<Option<Rc<TextModel>>>>(F);

impl<F: Fn(&URI) -> MaybePromise<Option<Rc<TextModel>>>> ITextModelContentProvider for ContentProvider<F> {
    fn provide_text_content(&self, resource: &URI) -> MaybePromise<Option<Rc<TextModel>>> {
        (self.0)(resource)
    }
}

#[test]
fn basics() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let (model_service, language_service, store) = (fixture.accessor.model_service.clone(), fixture.accessor.language_service.clone(), disposables.store());
        disposables.add(fixture.resolver.register_text_model_content_provider(
            "test",
            Rc::new(ContentProvider(move |resource: &URI| {
                if resource.scheme() == "test" {
                    let model_content = "Hello Test";
                    let language_selection = language_service.create_by_id(Some("json"));

                    return MaybePromise::Promise(Promise::resolved(Some(store.add_rc(model_service.create_model(model_content, Some(language_selection.as_ref()), Some(resource), false)))));
                }

                MaybePromise::Promise(Promise::resolved(None))
            })),
        ));

        let editor_service: Rc<dyn IEditorService> = Rc::new(TestEditorService);
        let input = disposables.add_rc(fixture.accessor.instantiation_service.create_instance::<TextResourceEditorInput>((
            uri_of("test", "thePath"),
            Some("name".to_string()),
            Some("description".to_string()),
            None,
            None,
        )));
        let other_input = disposables.add_rc(fixture.accessor.instantiation_service.create_instance::<TextResourceEditorInput>((
            uri_of("test", "thePath"),
            Some("name2".to_string()),
            Some("description".to_string()),
            None,
            None,
        )));
        let diff_input = disposables.add_rc(DiffEditorInput::new(Some("name"), Some("description"), input, other_input, None, editor_service));

        let model = diff_input.resolve().await.unwrap().expect("o modelo do diff");
        disposables.add(model.clone());

        let text_diff_model = model.as_any().downcast_ref::<TextDiffEditorModel>().expect("o modelo é um TextDiffEditorModel");

        let diff_editor_model = text_diff_model.text_diff_editor_model().expect("o modelo de diff de texto existe");
        let (_original, _modified) = (diff_editor_model.original(), diff_editor_model.modified());

        let model = diff_input.resolve().await.unwrap().expect("o modelo do diff");
        disposables.add(model.clone());
        assert!(model.is_resolved());

        let text_diff_model = model.as_any().downcast_ref::<TextDiffEditorModel>().expect("o modelo é um TextDiffEditorModel");
        assert!(!Rc::ptr_eq(&diff_editor_model, &text_diff_model.text_diff_editor_model().expect("o modelo de diff de texto existe")));
        diff_input.dispose();
        assert!(text_diff_model.text_diff_editor_model().is_none());
    });
}
