//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/onboardingTryoutActions.test.ts` (suíte
//! `Onboarding tryout presentations`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (dentro do corpo
//!   assíncrono, que roda em `run_with_real_timers`); `store.add` vira `guard.add` / `guard.add_rc`. O `teardown(() =>
//!   sinon.restore())` não tem o que restaurar: os dublês são estruturas locais.
//! - **`upcastPartial<...>`.** São dublês locais com só o que o caso lê; o que o original deixa indefinido falha com
//!   `unreachable!`, como o `TypeError` do `upcastPartial`.
//! - **`sinon.stub().resolves(...)` / `.rejects(...)`.** É o [`StubCommandService`], que guarda as chamadas e devolve o
//!   resultado fixo; `executeCommand.args` é a lista de `(id, argumentos)`, comparada por [`assert_deep_strict_equal`].
//! - **`ContextKeyService` real.** Criado com o `TestConfigurationService`; `MenuRegistry.addCommand` e
//!   `CommandsRegistry.registerCommand` usam os registros globais do thread, descartados pelo guarda.
//! - **Modelos de texto.** O original usa `upcastPartial<ITextModel>`; aqui o `IModelService` dublê cria
//!   [`TextModel`] de verdade (com `create_text_model_with`), e a referência de modelo devolve um
//!   `ITextEditorModel` que só expõe o modelo.
//! - **`sinon.spy(sample.presentation, 'prepare')` + `firstCall.returnValue`.** É o registro das promessas que o
//!   `prepare` da definição devolve, instalado por cima da definição registrada.
//! - **`assert.rejects(promise, /no longer available/)`.** É a rejeição da promessa do `ITextModelContentProvider`
//!   conferida pelo trecho da mensagem.
//! - **`for (const part of [...]) test(...)`.** Vira um teste por parte, na ordem do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, Reference, plain_disposable};
use je_base::network::schemas;
use je_base::nls::LocalizedString;
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::uri::{URI, UriComponents};
use je_editor::language_service::{ILanguageSelection, ILanguageService, LanguageService};
use je_editor::model::{RelaxedTextModelCreationOptions, TextModelCreationOptions, TextSnapshot};
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::text_model_edit_source::TextModelEditSource;
use je_instantiation::ServicesAccessor;
use je_platform::action::CommandAction;
use je_platform::actions::menu_registry;
use je_platform::commands::commands::{CommandEvent, ICommandService, commands_registry};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::{ContextKeyExpr, IContextKeyService};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::telemetry::null_telemetry_service;
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::{
    EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, GroupIdentifier, IEditorPane, UntypedEditorInput,
};
use je_workbench::common::views::{
    IViewContainerModel, IViewDescriptor, IViewDescriptorService, ViewContainerLocation, ViewContainerLocationChange, ViewContainerRc,
    ViewContainersChange, ViewDescriptorRc, ViewVisibilityState, ViewsContainerChange, ViewsLocationChange,
};
use je_workbench::contrib::onboarding::onboarding_sample_presentation::EditorSampleTryoutPresentation;
use je_workbench::contrib::onboarding::onboarding_scenario::{OnboardingPresentationRef, OnboardingScenario, OnboardingTrigger};
use je_workbench::contrib::onboarding::onboarding_tryout::{
    OnboardingTryoutAvailability, OnboardingTryoutLaunchKind, OnboardingTryoutMetadata, OnboardingTryoutPreparation, OnboardingTryoutResult,
    OnboardingTryoutRunContext, OnboardingTryoutUnavailable,
};
use je_workbench::contrib::onboarding::onboarding_tryout_actions::{
    CommandTryoutPayload, CommandTryoutPresentation, EditorSampleTryoutPayload, ViewTryoutPayload, ViewTryoutPresentation, ViewTryoutTarget,
};
use je_workbench::contrib::onboarding::onboarding_tryout_service::{
    ChatEntitlement, ChatSentiment, IChatEntitlementService, IOnboardingTryoutService, OnboardingTryout, OnboardingTryoutService,
    register_onboarding_tryout, register_onboarding_tryout_presentation,
};
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::textmodel_resolver::resolver_service::{
    ITextEditorModel, ITextModelContentProvider, ITextModelService, TextModelReference,
};
use je_workbench::services::views::views_service::IViewsService;
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService};
use test_text_model::create_text_model_with;

// ---- comandos

/// `sinon.stub().resolves(result)` / `sinon.stub().rejects(error)` do `executeCommand`.
struct StubCommandService {
    calls: RefCell<Vec<(String, Vec<Dynamic>)>>,
    outcome: Result<Dynamic, Error>,
}

impl StubCommandService {
    fn new(outcome: Result<Dynamic, Error>) -> Rc<Self> {
        Rc::new(Self { calls: RefCell::default(), outcome })
    }

    /// `executeCommand.args`, como `[[id, ...arguments], ...]`.
    fn calls_as_dynamic(&self) -> Dynamic {
        Dynamic::array(self.calls.borrow().iter().map(|(id, arguments)| {
            Dynamic::array(std::iter::once(Dynamic::string(id)).chain(arguments.iter().cloned()))
        }))
    }
}

impl ICommandService for StubCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.calls.borrow_mut().push((command_id.to_string(), args));
        match &self.outcome {
            Ok(value) => Promise::resolved(value.clone()),
            Err(error) => Promise::rejected(error.clone()),
        }
    }
}

/// `createContext(token)`.
fn create_context(guard: &LeakGuard, token: CancellationToken) -> OnboardingTryoutRunContext {
    OnboardingTryoutRunContext {
        id: "test.sample".to_string(),
        token,
        store: guard.add_rc(DisposableStore::new()),
        on_did_launch: None,
        on_did_finish_guidance: None,
    }
}

/// `createContextKeys()`.
fn create_context_keys(guard: &LeakGuard) -> Rc<ContextKeyService> {
    let configuration = TestConfigurationService::new(None);
    guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
    guard.add_rc(ContextKeyService::new(configuration))
}

/// `registerCommand(id)`.
fn register_command(guard: &LeakGuard, id: &str) {
    guard.add(commands_registry().register_command_handler(id, Some(Rc::new(|_: &ServicesAccessor, _: &[Dynamic]| Dynamic::Undefined))));
}

fn command_presentation(
    commands: &Rc<StubCommandService>,
    context_keys: &Rc<ContextKeyService>,
) -> Rc<CommandTryoutPresentation> {
    let command_service: Rc<dyn ICommandService> = commands.clone();
    let context_key_service: Rc<dyn IContextKeyService> = context_keys.clone();
    CommandTryoutPresentation::new(command_service, context_key_service)
}

fn command_payload(command_id: &str) -> CommandTryoutPayload {
    CommandTryoutPayload { command_id: command_id.to_string(), arguments: None, capture_target_scope: None }
}

/// O `run` de `{ kind: 'ready', run }`; qualquer outra preparação reprova o caso (`assert.strictEqual(prepared.kind, 'ready')`).
fn ready_run(prepared: OnboardingTryoutPreparation) -> Rc<dyn Fn() -> Promise<OnboardingTryoutResult>> {
    match prepared {
        OnboardingTryoutPreparation::Ready { run } => run,
        _ => panic!("a preparação deveria estar pronta"),
    }
}

fn unavailable(message: &str) -> OnboardingTryoutUnavailable {
    OnboardingTryoutUnavailable { message: message.to_string(), action: None }
}

fn executed(target_scope: Option<&str>) -> OnboardingTryoutResult {
    OnboardingTryoutResult::Launched { kind: OnboardingTryoutLaunchKind::Executed, target_scope: target_scope.map(str::to_string) }
}

fn opened() -> OnboardingTryoutResult {
    OnboardingTryoutResult::Launched { kind: OnboardingTryoutLaunchKind::Opened, target_scope: None }
}

#[test]
fn commands_preserve_exact_fixed_arguments_and_do_not_execute_during_preparation() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_command(&guard, "test.command");
        let commands = StubCommandService::new(Ok(Dynamic::Undefined));
        let presentation = command_presentation(&commands, &create_context_keys(&guard));
        let payload = CommandTryoutPayload {
            command_id: "test.command".to_string(),
            arguments: Some(vec![Dynamic::object([("setting", Dynamic::string("value"))]), Dynamic::string("second")]),
            capture_target_scope: None,
        };
        let prepared = presentation.prepare(&payload, &create_context(&guard, CancellationToken::None)).await.unwrap();
        let calls_before_run = commands.calls.borrow().len();
        let result = ready_run(prepared)().await.unwrap();

        assert_eq!((calls_before_run, result), (0, executed(None)));
        assert_deep_strict_equal(
            &commands.calls_as_dynamic(),
            &Dynamic::array([Dynamic::array([
                Dynamic::string("test.command"),
                Dynamic::object([("setting", Dynamic::string("value"))]),
                Dynamic::string("second"),
            ])]),
        );
    });
}

#[test]
fn command_preconditions_are_rechecked_immediately_before_execution() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let context_keys = create_context_keys(&guard);
        let enabled = context_keys.create_key("test.commandEnabled", Dynamic::Bool(true));
        let mut action = CommandAction::new("test.guardedCommand", "Test");
        action.precondition = Some(ContextKeyExpr::has("test.commandEnabled"));
        guard.add(menu_registry().add_command(action));
        register_command(&guard, "test.guardedCommand");
        let commands = StubCommandService::new(Ok(Dynamic::Undefined));
        let presentation = command_presentation(&commands, &context_keys);
        let prepared = presentation.prepare(&command_payload("test.guardedCommand"), &create_context(&guard, CancellationToken::None)).await.unwrap();
        let run = ready_run(prepared);
        enabled.set(Dynamic::Bool(false));
        let result = run().await.unwrap();

        assert_eq!((matches!(result, OnboardingTryoutResult::Unavailable(_)), commands.calls.borrow().len()), (true, 0));
    });
}

#[test]
fn command_failures_retain_their_normal_failure_result() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_command(&guard, "test.failure");
        let commands = StubCommandService::new(Err(Error::new("command failed")));
        let presentation = command_presentation(&commands, &create_context_keys(&guard));
        let prepared = presentation.prepare(&command_payload("test.failure"), &create_context(&guard, CancellationToken::None)).await.unwrap();
        let error = ready_run(prepared)().await.expect_err("o comando deveria rejeitar");

        assert!(error.to_string().contains("command failed"), "mensagem inesperada: {error}");
    });
}

#[test]
fn commands_can_bind_guided_steps_to_a_returned_target_scope() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_command(&guard, "test.scopedCommand");
        let commands = StubCommandService::new(Ok(Dynamic::object([("targetScope", Dynamic::string("prepared-instance"))])));
        let presentation = command_presentation(&commands, &create_context_keys(&guard));
        let payload = CommandTryoutPayload { capture_target_scope: Some(true), ..command_payload("test.scopedCommand") };
        let prepared = presentation.prepare(&payload, &create_context(&guard, CancellationToken::None)).await.unwrap();

        assert_eq!(ready_run(prepared)().await.unwrap(), executed(Some("prepared-instance")));
    });
}

#[test]
fn scoped_commands_fail_closed_when_no_target_scope_is_returned() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        register_command(&guard, "test.missingTarget");
        let commands = StubCommandService::new(Ok(Dynamic::Undefined));
        let presentation = command_presentation(&commands, &create_context_keys(&guard));
        let payload = CommandTryoutPayload { capture_target_scope: Some(true), ..command_payload("test.missingTarget") };
        let prepared = presentation.prepare(&payload, &create_context(&guard, CancellationToken::None)).await.unwrap();

        assert_eq!(
            ready_run(prepared)().await.unwrap(),
            OnboardingTryoutResult::Unavailable(unavailable("The example opened, but its target is no longer available."))
        );
    });
}

#[test]
fn unregistered_commands_are_unavailable_before_dispatch() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let commands = StubCommandService::new(Ok(Dynamic::Undefined));
        let presentation = command_presentation(&commands, &create_context_keys(&guard));

        assert_eq!(
            presentation.get_availability(&command_payload("test.notRegistered")),
            OnboardingTryoutAvailability::Unavailable(unavailable("This command is not available in the current context."))
        );
    });
}

// ---- view

/// `upcastPartial<IViewsService>({ openView })`: o `openView` resolve uma view e guarda `(id, focus)`.
struct StubViewsService {
    calls: RefCell<Vec<(String, bool)>>,
}

impl IViewsService for StubViewsService {
    fn open_view(&self, _id: &str, _focus: bool) -> Promise<()> {
        unreachable!("upcastPartial<IViewsService>: openView sem retorno não é definido")
    }

    fn try_open_view(&self, id: &str, focus: bool) -> Promise<bool> {
        self.calls.borrow_mut().push((id.to_string(), focus));
        Promise::resolved(true)
    }
}

/// `upcastPartial<IViewDescriptorService>({ onDidChangeViewContainers, onDidChangeContainer, getViewDescriptorById })`.
struct StubViewDescriptorService;

fn not_defined(member: &str) -> ! {
    unreachable!("upcastPartial<IViewDescriptorService>: {member} não é definido")
}

impl IViewDescriptorService for StubViewDescriptorService {
    fn view_containers(&self) -> Vec<ViewContainerRc> {
        not_defined("viewContainers")
    }

    fn on_did_change_view_containers(&self) -> Event<ViewContainersChange> {
        Event::none()
    }

    fn get_default_view_container(&self, _location: ViewContainerLocation) -> Option<ViewContainerRc> {
        not_defined("getDefaultViewContainer")
    }

    fn get_view_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_defined("getViewContainerById")
    }

    fn is_view_container_removed_permanently(&self, _id: &str) -> bool {
        not_defined("isViewContainerRemovedPermanently")
    }

    fn get_default_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        not_defined("getDefaultViewContainerLocation")
    }

    fn get_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        not_defined("getViewContainerLocation")
    }

    fn get_view_containers_by_location(&self, _location: ViewContainerLocation) -> Vec<ViewContainerRc> {
        not_defined("getViewContainersByLocation")
    }

    fn get_view_container_model(&self, _view_container: &ViewContainerRc) -> Rc<dyn IViewContainerModel> {
        not_defined("getViewContainerModel")
    }

    fn on_did_change_container_location(&self) -> Event<ViewContainerLocationChange> {
        not_defined("onDidChangeContainerLocation")
    }

    fn move_view_container_to_location(&self, _view_container: &ViewContainerRc, _location: ViewContainerLocation, _requested_index: Option<usize>, _reason: Option<&str>) {
        not_defined("moveViewContainerToLocation")
    }

    fn get_view_container_badge_enablement_state(&self, _id: &str) -> bool {
        not_defined("getViewContainerBadgeEnablementState")
    }

    fn set_view_container_badge_enablement_state(&self, _id: &str, _badges_enabled: bool) {
        not_defined("setViewContainerBadgeEnablementState")
    }

    fn get_view_descriptor_by_id(&self, id: &str) -> Option<ViewDescriptorRc> {
        let name = LocalizedString { original: "Test view".to_string(), value: "Test view".to_string() };
        Some(Rc::new(IViewDescriptor::new(id, name)))
    }

    fn get_view_container_by_view_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_defined("getViewContainerByViewId")
    }

    fn get_default_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_defined("getDefaultContainerById")
    }

    fn get_view_location_by_id(&self, _id: &str) -> Option<ViewContainerLocation> {
        not_defined("getViewLocationById")
    }

    fn can_move_views(&self) -> bool {
        not_defined("canMoveViews")
    }

    fn on_did_change_container(&self) -> Event<ViewsContainerChange> {
        Event::none()
    }

    fn move_views_to_container(&self, _views: &[ViewDescriptorRc], _view_container: &ViewContainerRc, _visibility_state: Option<ViewVisibilityState>, _reason: Option<&str>) {
        not_defined("moveViewsToContainer")
    }

    fn on_did_change_location(&self) -> Event<ViewsLocationChange> {
        not_defined("onDidChangeLocation")
    }

    fn move_view_to_location(&self, _view: &ViewDescriptorRc, _location: ViewContainerLocation, _reason: Option<&str>) {
        not_defined("moveViewToLocation")
    }

    fn reset(&self) {
        not_defined("reset")
    }
}

#[test]
fn opens_the_contributed_view_rather_than_relying_on_active_ui_state() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let views = Rc::new(StubViewsService { calls: RefCell::default() });
        let views_service: Rc<dyn IViewsService> = views.clone();
        let context_keys: Rc<dyn IContextKeyService> = create_context_keys(&guard);
        let presentation = ViewTryoutPresentation::new(views_service, Rc::new(StubViewDescriptorService), context_keys);
        let payload = ViewTryoutPayload { id: "test.view".to_string(), target: ViewTryoutTarget::View, focus: None };
        let prepared = presentation.prepare(&payload, &create_context(&guard, CancellationToken::None)).await.unwrap();
        let result = ready_run(prepared)().await.unwrap();

        assert_eq!((result, views.calls.borrow().clone()), (opened(), vec![("test.view".to_string(), true)]));
    });
}

// ---- amostra de editor

/// `new class { ... }` do `IEditorPane` que o `openEditor` devolve (`upcastPartial<IEditorPane>({})`).
struct StubEditorPane;

impl IEditorPane for StubEditorPane {
    fn input(&self) -> Option<EditorInputRef> {
        None
    }

    fn group_id(&self) -> GroupIdentifier {
        0
    }

    fn get_view_state(&self) -> Option<Dynamic> {
        None
    }

    fn on_did_change_control(&self) -> Event<()> {
        Event::none()
    }
}

/// `upcastPartial<IEditorService>({ openEditor })`: guarda as entradas abertas.
#[derive(Default)]
struct StubEditorService {
    opened: RefCell<Vec<UntypedEditorInput>>,
}

impl IEditorService for StubEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        unreachable!("upcastPartial<IEditorService>: visibleEditorPanes não é definido")
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        unreachable!("upcastPartial<IEditorService>: activeEditor não é definido")
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        unreachable!("upcastPartial<IEditorService>: activeTextEditorControl não é definido")
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        unreachable!("upcastPartial<IEditorService>: activeTextEditorLanguageId não é definido")
    }

    fn open_editor(&self, editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        self.opened.borrow_mut().push(editor);
        let pane: Rc<dyn IEditorPane> = Rc::new(StubEditorPane);
        Promise::resolved(Some(pane))
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        unreachable!("upcastPartial<IEditorService>: findEditors não é definido")
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        unreachable!("upcastPartial<IEditorService>: save não é definido")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        unreachable!("upcastPartial<IEditorService>: saveAll não é definido")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        unreachable!("upcastPartial<IEditorService>: getEditors não é definido")
    }

    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        unreachable!("upcastPartial<IEditorService>: openEditors não é definido")
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        unreachable!("upcastPartial<IEditorService>: isVisible não é definido")
    }
}

/// `upcastPartial<IResolvedTextEditorModel>({ textEditorModel: model })`.
struct ResolvedTextEditorModel {
    model: Rc<TextModel>,
}

impl IDisposable for ResolvedTextEditorModel {
    fn dispose(&self) {}
}

impl IEditorModel for ResolvedTextEditorModel {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        unreachable!("upcastPartial<IResolvedTextEditorModel>: onWillDispose não é definido")
    }

    fn resolve(&self) -> Promise<()> {
        unreachable!("upcastPartial<IResolvedTextEditorModel>: resolve não é definido")
    }

    fn is_resolved(&self) -> bool {
        unreachable!("upcastPartial<IResolvedTextEditorModel>: isResolved não é definido")
    }

    fn is_disposed(&self) -> bool {
        unreachable!("upcastPartial<IResolvedTextEditorModel>: isDisposed não é definido")
    }
}

impl ITextEditorModel for ResolvedTextEditorModel {
    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        Some(self.model.clone())
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(false)
    }

    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        unreachable!("upcastPartial<IResolvedTextEditorModel>: createSnapshot não é definido")
    }
}

type ReferenceHook = Rc<dyn Fn(&URI) -> Promise<()>>;

/// `upcastPartial<ITextModelService>({ registerTextModelContentProvider, createModelReference })`.
struct StubTextModelService {
    provider: RefCell<Option<Rc<dyn ITextModelContentProvider>>>,
    references: Rc<RefCell<Vec<String>>>,
    released: Rc<Cell<i32>>,
    on_reference: Option<ReferenceHook>,
}

impl ITextModelService for StubTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        let provider = self.provider.borrow().clone();
        let (resource, references, released, on_reference) = (resource.clone(), self.references.clone(), self.released.clone(), self.on_reference.clone());
        async_fn(async move {
            let Some(provider) = provider else { return Err(Error::new("Expected a registered sample provider.")) };
            let content = match provider.provide_text_content(&resource) {
                MaybePromise::Value(content) => content,
                MaybePromise::Promise(promise) => promise.await?,
            };
            let model = content.ok_or_else(|| Error::new("Expected a sample model."))?;
            references.borrow_mut().push(resource.path().to_string_lossy());
            if let Some(on_reference) = &on_reference {
                on_reference(&resource).await?;
            }
            let object: Rc<dyn ITextEditorModel> = Rc::new(ResolvedTextEditorModel { model });
            Ok(Reference::new(object, plain_disposable(move || released.set(released.get() + 1))))
        })
    }

    fn register_text_model_content_provider(&self, scheme: &str, provider: Rc<dyn ITextModelContentProvider>) -> DisposableRef {
        assert_eq!(scheme, schemas::VSCODE_ONBOARDING_SAMPLE);
        *self.provider.borrow_mut() = Some(provider);
        plain_disposable(|| {})
    }
}

/// `upcastPartial<IModelService>({ getModel, createModel })`: os modelos criados ficam na ordem de criação.
struct StubModelService {
    models: Rc<RefCell<Vec<(String, Rc<TextModel>)>>>,
}

impl IModelService for StubModelService {
    fn get_model(&self, resource: &URI) -> Option<Rc<TextModel>> {
        let key = resource.to_string(false).to_string_lossy();
        self.models.borrow().iter().find(|(candidate, _)| *candidate == key).map(|(_, model)| model.clone())
    }

    fn get_models(&self) -> Vec<Rc<TextModel>> {
        unreachable!("upcastPartial<IModelService>: getModels não é definido")
    }

    fn on_model_added(&self) -> Event<Rc<TextModel>> {
        unreachable!("upcastPartial<IModelService>: onModelAdded não é definido")
    }

    fn create_model(&self, value: TextSource, language_selection: Option<&dyn ILanguageSelection>, resource: Option<&URI>, _is_for_simple_widget: bool) -> Rc<TextModel> {
        let Some(resource) = resource else { panic!("Expected a text sample and its resource.") };
        let language_id = language_selection.map_or_else(|| "plaintext".to_string(), |selection| selection.language_id());
        let model = create_text_model_with(value, Some(language_id.as_str()), RelaxedTextModelCreationOptions::default(), Some(resource.clone()));
        self.models.borrow_mut().push((resource.to_string(false).to_string_lossy(), model.clone()));
        model
    }

    fn update_model(&self, _model: &TextModel, _value: TextSource, _reason: Option<TextModelEditSource>) {
        unreachable!("upcastPartial<IModelService>: updateModel não é definido")
    }

    fn destroy_model(&self, _resource: &URI) {
        unreachable!("upcastPartial<IModelService>: destroyModel não é definido")
    }

    fn get_creation_options(&self, _language: &str, _resource: Option<&URI>, _is_for_simple_widget: bool) -> TextModelCreationOptions {
        unreachable!("upcastPartial<IModelService>: getCreationOptions não é definido")
    }
}

/// `upcastPartial<IOnboardingTryoutService>({ getTryout })`.
struct StubTryoutService {
    scenario: Rc<OnboardingScenario>,
}

impl IOnboardingTryoutService for StubTryoutService {
    fn on_did_change(&self) -> Event<()> {
        unreachable!("upcastPartial<IOnboardingTryoutService>: onDidChange não é definido")
    }

    fn get_tryout(&self, id: &str) -> Option<Rc<OnboardingScenario>> {
        (id == "test.sample").then(|| self.scenario.clone())
    }

    fn get_tryouts(&self) -> Vec<Rc<OnboardingScenario>> {
        unreachable!("upcastPartial<IOnboardingTryoutService>: getTryouts não é definido")
    }

    fn get_availability(&self, _id: &str) -> OnboardingTryoutAvailability {
        unreachable!("upcastPartial<IOnboardingTryoutService>: getAvailability não é definido")
    }

    fn run(
        &self,
        _id: &str,
        _token: CancellationToken,
        _options: Option<je_workbench::contrib::onboarding::onboarding_tryout_service::OnboardingTryoutRunOptions>,
    ) -> Promise<OnboardingTryoutResult> {
        unreachable!("upcastPartial<IOnboardingTryoutService>: run não é definido")
    }
}

/// O que `createSamplePresentation` devolve: `{ presentation, models, references, openEditor, released }`.
struct Sample {
    presentation: Rc<EditorSampleTryoutPresentation>,
    models: Rc<RefCell<Vec<(String, Rc<TextModel>)>>>,
    references: Rc<RefCell<Vec<String>>>,
    editor: Rc<StubEditorService>,
    released: Rc<Cell<i32>>,
}

impl Sample {
    fn contents(&self) -> Vec<String> {
        self.models.borrow().iter().map(|(_, model)| model.get_value().to_string_lossy()).collect()
    }

    /// Descarta os modelos que o dublê criou (o original usa modelos parciais, sem descarte).
    fn dispose_models(&self) {
        for (_, model) in self.models.borrow().iter() {
            model.dispose();
        }
    }
}

/// `createSamplePresentation(payload, onReference?)`.
fn create_sample_presentation(guard: &LeakGuard, payload: &EditorSampleTryoutPayload, on_reference: Option<ReferenceHook>) -> Sample {
    let models: Rc<RefCell<Vec<(String, Rc<TextModel>)>>> = Rc::default();
    let references: Rc<RefCell<Vec<String>>> = Rc::default();
    let released = Rc::new(Cell::new(0));
    let editor = Rc::new(StubEditorService::default());
    let text_model_service =
        Rc::new(StubTextModelService { provider: RefCell::default(), references: references.clone(), released: released.clone(), on_reference });
    let language_service = guard.add_rc(LanguageService::new(false));
    let scenario = Rc::new(OnboardingScenario {
        id: "test.sample".to_string(),
        seen_key: None,
        when: None,
        trigger: OnboardingTrigger::Command { command_id: "test.command".to_string() },
        priority: None,
        presentation: OnboardingPresentationRef { kind: "editorSample".to_string(), payload: payload.to_dynamic() },
        tryout: Some(Rc::new(OnboardingTryoutMetadata {
            title: "Sample".to_string(),
            description: "Sample description".to_string(),
            ..OnboardingTryoutMetadata::default()
        })),
        experiment: None,
        developer_mode_variations: None,
        repeatable: None,
    });
    let editor_service: Rc<dyn IEditorService> = editor.clone();
    let text_model_dyn: Rc<dyn ITextModelService> = text_model_service.clone();
    let language_dyn: Rc<dyn ILanguageService> = language_service;
    let presentation = guard.add_rc(EditorSampleTryoutPresentation::new(
        editor_service,
        text_model_dyn,
        Rc::new(StubModelService { models: models.clone() }),
        language_dyn,
        Rc::new(StubTryoutService { scenario }),
    ));
    let provider: Rc<dyn ITextModelContentProvider> = presentation.clone();
    guard.add(text_model_service.register_text_model_content_provider(schemas::VSCODE_ONBOARDING_SAMPLE, provider));
    Sample { presentation, models, references, editor, released }
}

fn diff_payload(language_id: Option<&str>) -> EditorSampleTryoutPayload {
    EditorSampleTryoutPayload::Diff {
        title: "Sample comparison".to_string(),
        original: if language_id.is_some() { "const value = 1;" } else { "before" }.to_string(),
        modified: if language_id.is_some() { "const value = 2;" } else { "after" }.to_string(),
        language_id: language_id.map(str::to_string),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

#[test]
fn prepares_real_comparison_content_without_opening_or_writing_user_files() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let payload = diff_payload(Some("typescript"));
        let sample = create_sample_presentation(&guard, &payload, None);
        let context = create_context(&guard, CancellationToken::None);
        let prepared = sample.presentation.prepare(&payload, &context).await.unwrap();
        let opened_before_run = sample.editor.opened.borrow().len();
        let result = ready_run(prepared)().await.unwrap();
        let (original_scheme, modified_scheme) = {
            let opened = sample.editor.opened.borrow();
            let input = opened.first().expect("o editor deveria ter sido aberto");
            let scheme_of = |side: &Option<Box<UntypedEditorInput>>| {
                side.as_ref().and_then(|side| side.resource.as_ref()).map(|resource| resource.scheme().to_string_lossy())
            };
            (scheme_of(&input.original), scheme_of(&input.modified))
        };
        context.store.dispose();
        sample.dispose_models();

        assert_eq!(
            (result, opened_before_run, sample.references.borrow().clone(), sample.contents(), original_scheme, modified_scheme, sample.released.get()),
            (
                opened(),
                0,
                strings(&["/test.sample/original", "/test.sample/modified"]),
                strings(&["const value = 1;", "const value = 2;"]),
                Some(schemas::VSCODE_ONBOARDING_SAMPLE.to_string()),
                Some(schemas::VSCODE_ONBOARDING_SAMPLE.to_string()),
                2
            )
        );
    });
}

#[test]
fn cancelled_sample_preparation_never_opens_an_editor() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let cancellation = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
        let payload = diff_payload(None);
        let to_cancel = cancellation.clone();
        let on_reference: ReferenceHook = Rc::new(move |_| {
            to_cancel.cancel();
            Promise::resolved(())
        });
        let sample = create_sample_presentation(&guard, &payload, Some(on_reference));
        let context = create_context(&guard, cancellation.token());
        let prepared = sample.presentation.prepare(&payload, &context).await.unwrap();
        context.store.dispose();
        sample.dispose_models();

        assert_eq!(
            (
                matches!(prepared, OnboardingTryoutPreparation::Cancelled),
                sample.editor.opened.borrow().len(),
                sample.references.borrow().clone(),
                sample.released.get()
            ),
            (true, 0, strings(&["/test.sample/original"]), 1)
        );
    });
}

/// `chat.sentiment` e eventos do `IChatEntitlementService` que o `OnboardingTryoutService` lê.
struct StubChatEntitlementService;

impl IChatEntitlementService for StubChatEntitlementService {
    fn sentiment(&self) -> ChatSentiment {
        ChatSentiment::default()
    }

    fn entitlement(&self) -> ChatEntitlement {
        ChatEntitlement::Pro
    }

    fn anonymous(&self) -> bool {
        false
    }

    fn has_byok_models(&self) -> bool {
        false
    }

    fn on_did_change_sentiment(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_entitlement(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_anonymous(&self) -> Event<()> {
        Event::none()
    }
}

fn cancelling_a_run_releases_a_late_sample_reference(part: &'static str) {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let cancellation = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
        let reference_started = Rc::new(DeferredPromise::<()>::new());
        let finish_reference = Rc::new(DeferredPromise::<()>::new());
        let payload = diff_payload(None);
        let on_reference: ReferenceHook = {
            let (reference_started, finish_reference) = (reference_started.clone(), finish_reference.clone());
            Rc::new(move |resource: &URI| {
                if resource.path().to_string_lossy().ends_with(&format!("/{part}")) {
                    reference_started.complete(());
                    return finish_reference.promise.clone();
                }
                Promise::resolved(())
            })
        };
        let sample = create_sample_presentation(&guard, &payload, Some(on_reference));
        let prepared_log: Rc<RefCell<Vec<Promise<OnboardingTryoutPreparation>>>> = Rc::default();
        let mut definition = sample.presentation.definition();
        let inner_prepare = definition.prepare.clone();
        {
            let prepared_log = prepared_log.clone();
            definition.prepare = Rc::new(move |payload: &Dynamic, context: &OnboardingTryoutRunContext| {
                let prepared = inner_prepare(payload, context);
                prepared_log.borrow_mut().push(prepared.clone());
                prepared
            });
        }
        let configuration = TestConfigurationService::new(None);
        guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
        let context_keys = guard.add_rc(ContextKeyService::new(configuration));
        let context_keys: Rc<dyn IContextKeyService> = context_keys;
        let chat: Rc<dyn IChatEntitlementService> = Rc::new(StubChatEntitlementService);
        let environment: Rc<dyn IWorkbenchEnvironmentService> =
            TestEnvironmentService::with_overrides(EnvironmentOverrides { is_sessions_window: true, ..EnvironmentOverrides::default() });
        let service = guard.add_rc(OnboardingTryoutService::new(context_keys, chat, environment, null_telemetry_service()));
        guard.add(register_onboarding_tryout_presentation(definition));
        guard.add(register_onboarding_tryout(OnboardingTryout {
            id: "test.sample".to_string(),
            when: None,
            presentation: OnboardingPresentationRef { kind: "editorSample".to_string(), payload: payload.to_dynamic() },
            metadata: OnboardingTryoutMetadata {
                title: "Sample".to_string(),
                description: "Sample description".to_string(),
                ..OnboardingTryoutMetadata::default()
            },
        }));

        let pending = service.run("test.sample", cancellation.token(), None);
        reference_started.promise.clone().await.unwrap();
        cancellation.cancel();
        let result = pending.await.unwrap();
        finish_reference.complete(());
        let prepared = prepared_log.borrow()[0].clone().await.unwrap();
        sample.dispose_models();

        assert_eq!(
            (
                result,
                matches!(prepared, OnboardingTryoutPreparation::Cancelled),
                sample.editor.opened.borrow().len(),
                sample.released.get()
            ),
            (OnboardingTryoutResult::Cancelled, true, 0, if part == "original" { 1 } else { 2 })
        );
    });
}

#[test]
fn cancelling_a_run_releases_a_late_original_sample_reference() {
    cancelling_a_run_releases_a_late_sample_reference("original");
}

#[test]
fn cancelling_a_run_releases_a_late_modified_sample_reference() {
    cancelling_a_run_releases_a_late_sample_reference("modified");
}

/// `assert.rejects(provider.provideTextContent(resource), /no longer available/)`.
async fn assert_provide_rejects(presentation: &EditorSampleTryoutPresentation, resource: &URI) {
    match presentation.provide_text_content(resource) {
        MaybePromise::Promise(promise) => {
            let Err(error) = promise.await else { panic!("a promessa deveria rejeitar") };
            assert!(error.to_string().contains("no longer available"), "mensagem inesperada: {error}");
        }
        MaybePromise::Value(_) => panic!("o conteúdo não deveria ser resolvido"),
    }
}

fn sample_uri(path: &str, query: Option<&str>) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(schemas::VSCODE_ONBOARDING_SAMPLE)),
            path: Some(JsString::from(path)),
            query: query.map(JsString::from),
            ..UriComponents::default()
        },
        false,
    )
}

#[test]
fn sample_content_can_only_be_resolved_from_registered_example_data() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let payload = EditorSampleTryoutPayload::Text { title: "Sample".to_string(), text: "Known example".to_string(), language_id: None };
        let sample = create_sample_presentation(&guard, &payload, None);
        let injected = sample_uri("/test.sample/text", Some("text=untrusted"));
        let injected_model = create_text_model_with("untrusted", None, RelaxedTextModelCreationOptions::default(), Some(injected.clone()));
        sample.models.borrow_mut().push((injected.to_string(false).to_string_lossy(), injected_model));

        assert_provide_rejects(&sample.presentation, &sample_uri("/unknown/text", None)).await;
        assert_provide_rejects(&sample.presentation, &injected).await;
        assert_provide_rejects(&sample.presentation, &sample_uri("/test.sample/modified", None)).await;
        sample.dispose_models();
    });
}
