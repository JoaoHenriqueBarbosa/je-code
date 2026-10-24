//! Porte de `src/vs/workbench/test/browser/parts/editor/textResourceEditorInput.test.ts` (suíte `TextResourceEditorInput`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`), e o `setup` ([`setup`]) roda logo depois; o `disposables` do original é o store do guarda.
//! - **`workbenchInstantiationService(undefined, disposables)` e `TestServiceAccessor`.** Os serviços que a entrada lê vêm
//!   do [`WorkbenchServices`]; o `ModelService` e o `LanguageService` são os reais (o `accessor.modelService` e o
//!   `accessor.languageService`), e `createInstance(TextResourceEditorInput, resource, name, description, languageId,
//!   contents)` é o `Injectable::create` com os argumentos estáticos e as dependências na ordem do construtor.
//! - **`ITextModelService`.** O `TextModelResolverService` real ainda não foi portado: o [`TestTextModelService`] faz o
//!   que ele faz para um documento em memória (`createInstance(TextResourceEditorModel, resource)`, com a referência
//!   descartando o modelo ao ser liberada). O `ITextFileService`, o `ILanguageDetectionService` e o
//!   `IAccessibilityService` são dublês que o caso não exercita (`TestTextFileService`, `TestLanguageDetectionService` e
//!   `TestAccessibilityService`).
//! - **Casos `async`** rodam em `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`(model as TextResourceEditorModel)`** e `model.textEditorModel` são o `downcast_ref` por `IEditorModel::as_any`.
//!   `snapshotToString` e `setValue` são os do `textfiles` e do `TextModel`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "configuration_event_support/mod.rs"]
mod configuration_event_support;
mod editor_input_services;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use editor_input_services::WorkbenchServices;
use je_base::JsString;
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::model_service::ModelService;
use je_editor::modes_registry::{LanguageExtensionPoint, PLAINTEXT_LANGUAGE_ID};
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::Injectable;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::text_resource_editor_input::TextResourceEditorInput;
use je_workbench::common::editor::text_resource_editor_model::TextResourceEditorModel;
use je_workbench::common::editor::RevertOptions;
use je_workbench::services::editor::custom_editor_label_service::CustomEditorLabelService;
use je_workbench::services::language_detection::language_detection_worker_service::ILanguageDetectionService;
use je_workbench::services::textfile::textfiles::{ILanguageSupport, ITextFileService, TextFileSaveAsOptions, TextFileSaveOptions, snapshot_to_string};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference};
use je_workbench::test::browser::workbench_test_services::TestEditorService;
use je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService;

/// `TestTextFileService`: o `ITextFileService` que os casos não exercitam (nenhum salva ou reverte).
struct TestTextFileService;

impl ITextFileService for TestTextFileService {
    fn save(&self, resource: &URI, _options: Option<&TextFileSaveOptions>) -> Promise<Option<URI>> {
        Promise::resolved(Some(resource.clone()))
    }

    fn save_as(&self, resource: &URI, target_resource: Option<&URI>, _options: Option<&TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        Promise::resolved(Some(target_resource.unwrap_or(resource).clone()))
    }

    fn revert(&self, _resource: &URI, _options: Option<RevertOptions>) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `TestLanguageDetectionService`: a detecção desligada.
struct TestLanguageDetectionService;

impl ILanguageDetectionService for TestLanguageDetectionService {
    fn is_enabled_for_language(&self, _language_id: &str) -> bool {
        false
    }

    fn detect_language(&self, _resource: &URI, _supported_langs: Option<&[String]>) -> Promise<Option<String>> {
        Promise::resolved(None)
    }
}

/// `TestAccessibilityService`.
struct TestAccessibilityService;

impl IAccessibilityService for TestAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_reduced_motion(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        Event::none()
    }

    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }

    fn is_screen_reader_optimized(&self) -> bool {
        false
    }

    fn is_motion_reduced(&self) -> bool {
        false
    }

    fn is_transparency_reduced(&self) -> bool {
        false
    }

    fn get_accessibility_support(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }

    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {}

    fn alert(&self, _message: &str) {}

    fn status(&self, _message: &str) {}
}

/// O `TextModelResolverService` para um documento em memória: um `TextResourceEditorModel` sobre o modelo que já existe,
/// com a referência descartando o modelo ao ser liberada.
struct TestTextModelService {
    language_service: Rc<dyn ILanguageService>,
    model_service: Rc<dyn IModelService>,
}

impl ITextModelService for TestTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        let model = TextResourceEditorModel::create(
            (resource.clone(),),
            (self.language_service.clone(), self.model_service.clone(), Rc::new(TestLanguageDetectionService), Rc::new(TestAccessibilityService)),
        );

        let model_to_release = model.clone();
        Promise::resolved(TextModelReference::new(model, to_disposable(move || model_to_release.dispose())))
    }
}

/// O que o `setup` do original monta: o `instantiationService` e o `accessor`.
struct Fixture {
    services: WorkbenchServices,
    custom_editor_label_service: Rc<CustomEditorLabelService>,
    language_service: Rc<LanguageService>,
    model_service: Rc<ModelService>,
    text_model_service: Rc<TestTextModelService>,
}

fn setup(disposables: &LeakGuard) -> Fixture {
    let test_configuration_service = TestConfigurationService::new(None);
    disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    let services = WorkbenchServices::new(disposables, &test_configuration_service);

    let custom_editor_label_service =
        disposables.add_rc(CustomEditorLabelService::create((), (test_configuration_service.clone(), services.context_service.clone())));

    let language_service = disposables.add_rc(LanguageService::new(false));
    let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;
    let model_service = disposables.add_rc(ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service),
        UndoRedoService::for_tests(),
    ));
    let text_model_service = Rc::new(TestTextModelService { language_service: language_service.clone(), model_service: model_service.clone() });

    Fixture { services, custom_editor_label_service, language_service, model_service, text_model_service }
}

impl Fixture {
    /// `instantiationService.createInstance(TextResourceEditorInput, resource, name, description, preferredLanguageId,
    /// preferredContents)`.
    fn create_input(
        &self,
        resource: &URI,
        name: &str,
        description: &str,
        preferred_language_id: Option<&str>,
        preferred_contents: Option<&str>,
    ) -> Rc<TextResourceEditorInput> {
        TextResourceEditorInput::create(
            (
                resource.clone(),
                Some(name.to_string()),
                Some(description.to_string()),
                preferred_language_id.map(str::to_string),
                preferred_contents.map(str::to_string),
            ),
            (
                self.text_model_service.clone(),
                Rc::new(TestTextFileService),
                Rc::new(TestEditorService),
                self.services.file_service.clone(),
                self.services.label_service.clone(),
                self.services.files_configuration_service.clone(),
                self.services.text_resource_configuration_service.clone(),
                self.custom_editor_label_service.clone(),
            ),
        )
    }

    /// `accessor.modelService.createModel('function test() {}', accessor.languageService.createById(PLAINTEXT_LANGUAGE_ID),
    /// resource)`.
    fn create_plaintext_model(&self, resource: &URI) {
        let language_selection = self.language_service.create_by_id(Some(PLAINTEXT_LANGUAGE_ID));
        self.model_service.create_model("function test() {}", Some(language_selection.as_ref()), Some(resource), false);
    }
}

/// `URI.from({ scheme: 'inmemory', authority: null!, path: 'thePath' })`.
fn in_memory_resource() -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from("inmemory")), path: Some(JsString::from("thePath")), ..UriComponents::default() }, false)
}

/// `model as TextResourceEditorModel`.
fn as_text_resource_model(model: &Rc<dyn IEditorModel>) -> &TextResourceEditorModel {
    model.as_any().downcast_ref::<TextResourceEditorModel>().expect("o modelo resolvido é um TextResourceEditorModel")
}

/// `model.textEditorModel` de um modelo resolvido.
fn text_editor_model_of(model: &Rc<dyn IEditorModel>) -> Rc<TextModel> {
    as_text_resource_model(model).text_editor_model().expect("o modelo de texto existe")
}

#[test]
fn basics() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = in_memory_resource();
        fixture.create_plaintext_model(&resource);

        let input = disposables.add_rc(fixture.create_input(&resource, "The Name", "The Description", None, None));

        let model = input.resolve().await.unwrap().expect("o modelo");
        disposables.add(model.clone());

        let mut snapshot = as_text_resource_model(&model).create_snapshot().expect("o snapshot");
        assert_eq!(snapshot_to_string(snapshot.as_mut()).to_string_lossy(), "function test() {}");
    });
}

#[test]
fn preferred_language_via_ctor() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let registration = fixture.language_service.register_language(LanguageExtensionPoint::new("resource-input-test"));

        let resource = in_memory_resource();
        fixture.create_plaintext_model(&resource);

        let input = disposables.add_rc(fixture.create_input(&resource, "The Name", "The Description", Some("resource-input-test"), None));

        let model = input.resolve().await.unwrap().expect("o modelo");
        disposables.add(model.clone());
        assert_eq!(text_editor_model_of(&model).get_language_id(), "resource-input-test");

        input.set_language_id("text", None);
        assert_eq!(text_editor_model_of(&model).get_language_id(), PLAINTEXT_LANGUAGE_ID);

        disposables.add(input.resolve().await.unwrap().expect("o modelo"));
        assert_eq!(text_editor_model_of(&model).get_language_id(), PLAINTEXT_LANGUAGE_ID);
        registration.dispose();
    });
}

#[test]
fn preferred_language_via_set_preferred_language_id() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let registration = fixture.language_service.register_language(LanguageExtensionPoint::new("resource-input-test"));

        let resource = in_memory_resource();
        fixture.create_plaintext_model(&resource);

        let input = disposables.add_rc(fixture.create_input(&resource, "The Name", "The Description", None, None));
        input.set_preferred_language_id("resource-input-test");

        let model = input.resolve().await.unwrap().expect("o modelo");
        disposables.add(model.clone());
        assert_eq!(text_editor_model_of(&model).get_language_id(), "resource-input-test");
        registration.dispose();
    });
}

#[test]
fn preferred_contents_via_ctor() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = in_memory_resource();
        fixture.create_plaintext_model(&resource);

        let input = disposables.add_rc(fixture.create_input(&resource, "The Name", "The Description", None, Some("My Resource Input Contents")));

        let model = input.resolve().await.unwrap().expect("o modelo");
        disposables.add(model.clone());
        assert_eq!(text_editor_model_of(&model).get_value().to_string_lossy(), "My Resource Input Contents");

        text_editor_model_of(&model).set_value(TextSource::from("Some other contents"));
        assert_eq!(text_editor_model_of(&model).get_value().to_string_lossy(), "Some other contents");

        disposables.add(input.resolve().await.unwrap().expect("o modelo"));
        assert_eq!(text_editor_model_of(&model).get_value().to_string_lossy(), "Some other contents"); // preferred contents only used once
    });
}

#[test]
fn preferred_contents_via_set_preferred_contents() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let resource = in_memory_resource();
        fixture.create_plaintext_model(&resource);

        let input = disposables.add_rc(fixture.create_input(&resource, "The Name", "The Description", None, None));
        input.set_preferred_contents("My Resource Input Contents");

        let model = input.resolve().await.unwrap().expect("o modelo");
        disposables.add(model.clone());
        assert_eq!(text_editor_model_of(&model).get_value().to_string_lossy(), "My Resource Input Contents");

        text_editor_model_of(&model).set_value(TextSource::from("Some other contents"));
        assert_eq!(text_editor_model_of(&model).get_value().to_string_lossy(), "Some other contents");

        disposables.add(input.resolve().await.unwrap().expect("o modelo"));
        assert_eq!(text_editor_model_of(&model).get_value().to_string_lossy(), "Some other contents"); // preferred contents only used once
    });
}
