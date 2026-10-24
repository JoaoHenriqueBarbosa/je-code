//! Porte de `src/vs/workbench/test/browser/parts/editor/resourceEditorInput.test.ts` (suíte `ResourceEditorInput`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`, `disposables` e `teardown`.** O guarda de vazamentos é criado no começo
//!   de cada caso (`LeakGuard`), e o `disposables` do original é o store dele.
//! - **`workbenchInstantiationService(undefined, disposables)`** é o [`WorkbenchServices`] (os serviços que a entrada lê, com
//!   os mesmos dublês do original); `instantiationService.stub(IConfigurationService, testConfigurationService)` é passar o
//!   `TestConfigurationService` ao [`WorkbenchServices::new`] e ao `CustomEditorLabelService`, e `createInstance` é o
//!   `Injectable::create`, com os argumentos estáticos e as dependências na ordem do construtor.
//! - **`class TestResourceEditorInput extends AbstractResourceEditorInput`** guarda a [`AbstractResourceEditorInput`] e herda
//!   os métodos de `EditorInput` pela macro `inherit_abstract_resource_editor_input`.
//! - **`onDidChangeConfigurationEmitter.fire({ affectsConfiguration, source: USER })`** é `fire_configuration_change`, que
//!   dispara o evento real com a chave alterada.
//! - **`assert.strictEqual(input.isReadonly(), false)`** é um `matches!` sobre `Readonly::Bool(false)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "configuration_event_support/mod.rs"]
mod configuration_event_support;
mod editor_input_services;

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use configuration_event_support::fire_configuration_change;
use editor_input_services::WorkbenchServices;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::uri::{URI, UriComponents};
use je_instantiation::Injectable;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::ILabelService;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::resource_editor_input::{AbstractResourceEditorInput, ResourceEditorInputServices};
use je_workbench::common::editor::{EditorInputCapabilities, Verbosity};
use je_workbench::services::editor::custom_editor_label_service::{CustomEditorLabelService, ICustomEditorLabelService};
use je_workbench::services::files_configuration::files_configuration_service::{IFilesConfigurationService, Readonly};

/// `class TestResourceEditorInput extends AbstractResourceEditorInput`.
struct TestResourceEditorInput {
    input: AbstractResourceEditorInput,
}

impl Injectable for TestResourceEditorInput {
    type Args = (URI,);
    type Deps = (
        Rc<dyn ILabelService>,
        Rc<dyn IFileService>,
        Rc<dyn IFilesConfigurationService>,
        Rc<dyn ITextResourceConfigurationService>,
        Rc<dyn ICustomEditorLabelService>,
    );

    fn create(
        (resource,): Self::Args,
        (label_service, file_service, files_configuration_service, text_resource_configuration_service, custom_editor_label_service): Self::Deps,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            input: AbstractResourceEditorInput::new(
                this.clone(),
                resource.clone(),
                Some(resource),
                ResourceEditorInputServices {
                    label_service,
                    file_service,
                    files_configuration_service,
                    text_resource_configuration_service,
                    custom_editor_label_service,
                },
            ),
        })
    }
}

impl IDisposable for TestResourceEditorInput {
    fn dispose(&self) {
        self.input.dispose();
    }
}

impl EditorInput for TestResourceEditorInput {
    je_workbench::inherit_abstract_resource_editor_input!(
        input; base, resource, preferred_resource, capabilities, get_name, get_description, get_title, is_readonly
    );

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        "test.typeId".to_string()
    }
}

/// `createServices()`: os serviços, o `TestConfigurationService` e o `CustomEditorLabelService`.
fn create_services(disposables: &LeakGuard) -> (WorkbenchServices, Rc<TestConfigurationService>, Rc<CustomEditorLabelService>) {
    let test_configuration_service = TestConfigurationService::new(None);
    disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    let services = WorkbenchServices::new(disposables, &test_configuration_service);

    let custom_editor_label_service =
        disposables.add_rc(CustomEditorLabelService::create((), (test_configuration_service.clone(), services.context_service.clone())));

    (services, test_configuration_service, custom_editor_label_service)
}

/// `instantiationService.createInstance(TestResourceEditorInput, resource)`.
fn create_input(services: &WorkbenchServices, custom_editor_label_service: &Rc<CustomEditorLabelService>, resource: URI) -> Rc<TestResourceEditorInput> {
    TestResourceEditorInput::create(
        (resource,),
        (
            services.label_service.clone(),
            services.file_service.clone(),
            services.files_configuration_service.clone(),
            services.text_resource_configuration_service.clone(),
            custom_editor_label_service.clone(),
        ),
    )
}

/// `URI.from({ scheme: 'testResource', path })`.
fn test_resource(path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from("testResource")), path: Some(JsString::from(path)), ..UriComponents::default() }, false)
}

/// `await testConfigurationService.setUserConfiguration(key, value)` e o disparo da mudança da chave.
fn update_configuration(test_configuration_service: &TestConfigurationService, key: &str, value: Dynamic) {
    let _ = test_configuration_service.set_user_configuration(key, value, None);
    fire_configuration_change(test_configuration_service, &[key]);
}

/// `{ [pattern]: label }` na ordem dada.
fn patterns(entries: &[(&str, &str)]) -> Dynamic {
    Dynamic::object(entries.iter().map(|(pattern, label)| (*pattern, Dynamic::string(label))))
}

#[test]
fn basics() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (services, _, custom_editor_label_service) = create_services(&disposables);

    let resource = test_resource("thePath/of/the/resource.txt");

    let input = disposables.add_rc(create_input(&services, &custom_editor_label_service, resource));

    assert!(!input.get_name().is_empty());

    assert!(!input.get_description(Some(Verbosity::Short)).unwrap().is_empty());
    assert!(!input.get_description(Some(Verbosity::Medium)).unwrap().is_empty());
    assert!(!input.get_description(Some(Verbosity::Long)).unwrap().is_empty());

    assert!(!input.get_title(Some(Verbosity::Short)).is_empty());
    assert!(!input.get_title(Some(Verbosity::Medium)).is_empty());
    assert!(!input.get_title(Some(Verbosity::Long)).is_empty());

    assert!(!input.has_capability(EditorInputCapabilities::READONLY));
    assert!(matches!(input.is_readonly(), Readonly::Bool(false)));
    assert!(input.has_capability(EditorInputCapabilities::UNTITLED));
}

#[test]
fn custom_editor_name() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (services, test_configuration_service, custom_editor_label_service) = create_services(&disposables);

    let resource1 = test_resource("thePath/of/the/resource.txt");
    let resource2 = test_resource("theOtherPath/of/the/resource.md");

    let input1 = disposables.add_rc(create_input(&services, &custom_editor_label_service, resource1));
    let input2 = disposables.add_rc(create_input(&services, &custom_editor_label_service, resource2));

    update_configuration(
        &test_configuration_service,
        CustomEditorLabelService::SETTING_ID_PATTERNS,
        patterns(&[("**/theOtherPath/**", "Label 1"), ("**/*.txt", "Label 2"), ("**/resource.txt", "Label 3")]),
    );

    let label1_name = Rc::new(RefCell::new(String::new()));
    let label2_name = Rc::new(RefCell::new(String::new()));
    let (listener_label1_name, listener_label2_name, listener_input1, listener_input2) = (label1_name.clone(), label2_name.clone(), input1.clone(), input2.clone());
    disposables.add(custom_editor_label_service.on_did_change().listen(move |_| {
        *listener_label1_name.borrow_mut() = listener_input1.get_name();
        *listener_label2_name.borrow_mut() = listener_input2.get_name();
    }));

    update_configuration(&test_configuration_service, CustomEditorLabelService::SETTING_ID_ENABLED, Dynamic::Bool(true));

    assert!(*label1_name.borrow() == "Label 3");
    assert!(*label2_name.borrow() == "Label 1");

    update_configuration(&test_configuration_service, CustomEditorLabelService::SETTING_ID_ENABLED, Dynamic::Bool(false));

    assert!(*label1_name.borrow() == "resource.txt");
    assert!(*label2_name.borrow() == "resource.md");

    update_configuration(&test_configuration_service, CustomEditorLabelService::SETTING_ID_ENABLED, Dynamic::Bool(true));

    update_configuration(
        &test_configuration_service,
        CustomEditorLabelService::SETTING_ID_PATTERNS,
        patterns(&[("thePath/**/resource.txt", "Label 4"), ("thePath/of/*/resource.txt", "Label 5")]),
    );

    assert!(*label1_name.borrow() == "Label 5");
    assert!(*label2_name.borrow() == "resource.md");
}
