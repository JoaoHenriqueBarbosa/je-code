//! Porte de `src/vs/editor/standalone/test/browser/standaloneServices.test.ts`.
//!
//! Notas de tradução:
//! - **`InMemoryTextModelService`** mora no `je-workbench` (o `ITextEditorModel` que ele devolve depende do `IEditorModel` do
//!   workbench), por isso o `je-editor` tem o `je-workbench` como dependência de desenvolvimento (circular, aceita pelo Cargo).
//! - **`createModelServices(disposables)` + `services.createInstance(InMemoryTextModelService)`** são o [`ModelService`] com a
//!   configuração de teste, descartado com o guarda, e o [`InMemoryTextModelService`] criado sobre ele ([`create_model_services`]).
//!   `services.get(IModelService)` é o mesmo [`ModelService`].
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o `LeakGuard` de cada caso, e o `disposables` da suíte é o próprio guarda.
//!   Os casos assíncronos rodam em tempo real (`run_with_real_timers`) e `await` é `.await.unwrap()`.
//! - **`IReference` e `disposables.add(reference)`.** A referência é o `Reference` de `je_base::lifecycle`, que se clona
//!   dividindo o descarte de chamada única; `disposables.add(ref)` é registrar uma cópia no guarda ([`track`]).
//! - **`for (const releaseCreatorFirst of [false, true])`** vira um `#[test]` por valor, os dois chamando o mesmo corpo.
//! - **`assert.strictEqual(a, b)` entre modelos** é a identidade do objeto ([`same_model`]); `assert.rejects(p, /Model not found/)`
//!   é a conferência de que a promessa rejeita com a mensagem.
//! - **`StandaloneKeybindingService`.** O `ICodeEditorService` e o `StandaloneCodeEditorService` que o original passa ao
//!   construtor só servem para ligar `keydown`/`keyup` aos contêineres de DOM dos editores; a UI nativa não tem DOM, então o
//!   serviço não os recebe (ver `standalone_services.rs`), e o `StandaloneThemeService`, que só alimentava o editor, também
//!   não entra. `testDispatch` (`super._dispatch(e, null!)`) é `dispatch_internal(service, e, None)`. O `_documentHasFocus`
//!   que o serviço real lê da janela é a função `|| true`.
//! - **`new InstantiationService(serviceCollection, true)`** é `InstantiationService::with_options` com `strict`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::dynamic::Dynamic;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model_service::ModelService;
use je_editor::services_model::IModelService;
use je_editor::standalone::standalone_services::{
    StandaloneCommandService, StandaloneConfigurationService, StandaloneKeybindingService, StandaloneNotificationService,
};
use je_editor::text_model::TextModel;
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::{Injectable, InstantiationOptions, InstantiationService, ServiceCollection};
use je_platform::commands::CommandHandler;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::keybinding::KeyboardEvent;
use je_platform::keybinding::abstract_keybinding_service::dispatch_internal;
use je_platform::log::NullLogService;
use je_platform::telemetry::null_telemetry_service;
use je_workbench::services::textmodel_resolver::in_memory_text_model_service::InMemoryTextModelService;
use je_workbench::services::textmodel_resolver::resolver_service::{ITextModelService, TextModelReference};
use je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService;

//#region InMemoryTextModelService

/// `createModelServices(store)` e `services.createInstance(InMemoryTextModelService)`.
struct ModelServices {
    model_service: Rc<ModelService>,
    resolver: Rc<InMemoryTextModelService>,
}

fn create_model_services(disposables: &LeakGuard) -> ModelServices {
    let test_configuration_service = TestConfigurationService::new(None);
    disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;
    let model_service = disposables.add_rc(ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service),
        UndoRedoService::for_tests(),
    ));
    let model_service_contract: Rc<dyn IModelService> = model_service.clone();
    let resolver = InMemoryTextModelService::create((), (model_service_contract,));
    ModelServices { model_service, resolver }
}

/// `disposables.add(reference)`: uma cópia da referência no guarda (o descarte é de chamada única e dividido).
fn track(disposables: &LeakGuard, reference: &TextModelReference) {
    disposables.add(Rc::new(reference.clone()));
}

/// `reference.object.textEditorModel`.
fn text_model_of(reference: &TextModelReference) -> Rc<TextModel> {
    reference.object.text_editor_model().expect("o modelo de texto existe")
}

/// `a === b` para os modelos (o endereço do objeto, sem o `vtable`).
fn same_model<A: ?Sized, B: ?Sized>(a: &Rc<A>, b: &Rc<B>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// O corpo de `owns synthetic documents until the last reference releases (creator first: ${releaseCreatorFirst})`.
fn owns_synthetic_documents_until_the_last_reference_releases(release_creator_first: bool) {
    run_with_real_timers(async move {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let services = create_model_services(&disposables);

        let owner = services.resolver.create_synthetic_document("shared".into(), None).await.unwrap();
        track(&disposables, &owner);
        let model = text_model_of(&owner);
        let reference = services.resolver.create_model_reference(&model.uri()).await.unwrap();
        track(&disposables, &reference);
        if release_creator_first {
            owner.dispose();
        } else {
            reference.dispose();
        }
        assert_eq!(model.get_value().to_string_lossy(), "shared");
        owner.dispose();
        reference.dispose();
        assert!(model.is_disposed());
    });
}

#[test]
fn owns_synthetic_documents_until_the_last_reference_releases_creator_first_false() {
    owns_synthetic_documents_until_the_last_reference_releases(false);
}

#[test]
fn owns_synthetic_documents_until_the_last_reference_releases_creator_first_true() {
    owns_synthetic_documents_until_the_last_reference_releases(true);
}

#[test]
fn reentrant_opens_from_on_model_added_share_the_creator_reference() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let services = create_model_services(&disposables);

        let pending: Rc<RefCell<Option<Promise<TextModelReference>>>> = Rc::default();
        let (listener_pending, resolver) = (pending.clone(), services.resolver.clone());
        disposables.add(services.model_service.on_model_added().listen(move |model| {
            *listener_pending.borrow_mut() = Some(resolver.create_model_reference(&model.uri()));
        }));
        let owner = services.resolver.create_synthetic_document("".into(), None).await.unwrap();
        track(&disposables, &owner);
        let pending = pending.borrow().clone().expect("a referência foi pedida no onModelAdded");
        let reference = pending.await.unwrap();
        track(&disposables, &reference);
        assert!(same_model(&reference.object, &owner.object));
        reference.dispose();
        assert!(!text_model_of(&owner).is_disposed());
    });
}

#[test]
fn does_not_take_ownership_of_externally_created_models() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let services = create_model_services(&disposables);

        let model = disposables.add_rc(services.model_service.create_model("external", None, None, false));
        let reference = services.resolver.create_model_reference(&model.uri()).await.unwrap();
        track(&disposables, &reference);
        reference.dispose();
        assert_eq!((model.is_disposed(), model.get_value().to_string_lossy()), (false, "external".to_string()));
    });
}

#[test]
fn releases_failed_acquisitions_so_a_later_open_can_succeed() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let services = create_model_services(&disposables);

        let resource = URI::parse("inmemory:/later", false);
        let error = services.resolver.create_model_reference(&resource).await.err().expect("a resolução rejeita");
        assert!(error.to_string().contains("Model not found"), "{error}");
        let model = disposables.add_rc(services.model_service.create_model("later", None, Some(&resource), false));
        let reference = services.resolver.create_model_reference(&resource).await.unwrap();
        track(&disposables, &reference);
        assert!(same_model(&text_model_of(&reference), &model));
    });
}

//#endregion

//#region StandaloneKeybindingService

#[test]
fn issue_microsoft_monaco_editor_167() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let disposables = DisposableStore::new();
    let instantiation_service = InstantiationService::with_options(ServiceCollection::new(), InstantiationOptions { strict: true, ..InstantiationOptions::default() });
    let configuration_service = StandaloneConfigurationService::new(NullLogService::shared());
    let context_key_service = ContextKeyService::new(configuration_service);
    disposables.add(context_key_service.clone());
    let command_service = StandaloneCommandService::new(instantiation_service);
    let notification_service = StandaloneNotificationService::new();
    let keybinding_service = StandaloneKeybindingService::new(
        context_key_service,
        command_service,
        null_telemetry_service(),
        notification_service,
        NullLogService::shared(),
        Rc::new(|| true),
    );
    disposables.add(keybinding_service.clone());

    let command_invoked = Rc::new(Cell::new(false));
    let invoked = command_invoked.clone();
    let handler: CommandHandler = Rc::new(move |_, _| {
        invoked.set(true);
        Dynamic::Undefined
    });
    disposables.add(keybinding_service.add_dynamic_keybinding("testCommand", KeyCode::F9.0 as u32, handler, None));

    dispatch_internal(
        &*keybinding_service,
        &KeyboardEvent { ctrl_key: false, shift_key: false, alt_key: false, meta_key: false, alt_graph_key: false, key_code: KeyCode::F9, code: None },
        None,
    );

    assert!(command_invoked.get(), "command invoked");

    disposables.dispose();
}

//#endregion
