//! Porte de `src/vs/editor/contrib/codeAction/test/browser/codeActionModel.test.ts` (suíte `CodeActionModel`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `setup`/`teardown` são [`Fixture`]: o `MarkerService`, o modelo `foo-lang` com a URI `untitled:path`, o editor de
//!   teste com a posição 1,1 e o registro novo; o `teardown` descarta o editor, o modelo e o serviço de marcadores.
//!   `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` (o `store` do original).
//! - `MockContextKeyService` é o `ContextKeyService` de `je-platform` com o `TestConfigurationService`, como em
//!   `snippet_controller2.rs`.
//! - `runWithFakedTimers({ useFakeTimers: true }, () => ...)` e os casos `async` são o corpo `async` de
//!   [`run_with_faked_timers`], que o executa no event loop do porte (os casos sem `runWithFakedTimers` rodam do mesmo
//!   jeito: não usam timers além do `setTimeout(0)`).
//! - `promiseWithResolvers` é `Promise::new()`; `e.actions.then(fixes => ..., done)` é `then_result` com o `done` como
//!   resolução ou rejeição. `assertType(e.type === Triggered)` é o `let ... else` sobre o estado.
//! - O argumento `5 /*delay*/` que o terceiro caso passa a `onDidChangeState` é o `thisArgs` do original e não existe
//!   aqui.
//! - `TestConfigurationService({ 'editor.codeActionWidget.includeNearbyQuickFixes': true })` é o objeto `Dynamic` com a
//!   chave pontilhada, que o serviço lê pelo `hasOwnProperty`, como o original.
//! - Os provedores de teste são estruturas que implementam [`CodeActionProvider`]; o literal `{ provideCodeActions() }`
//!   do `testProvider` é [`TestProvider`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout, LeakGuard};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::code_action::code_action_model::{CodeActionModel, CodeActionsState, CodeActionsTriggered};
use je_editor::code_action::languages::{
    CodeAction, CodeActionContext, CodeActionList, CodeActionListObject, CodeActionProvider, CodeActionRef,
    CodeActionTriggerType, Command,
};
use je_editor::code_action::platform::ActionSet;
use je_editor::code_action::types::{CodeActionTrigger, CodeActionTriggerSource};
use je_editor::code_editor_widget::{ICodeEditor, SelectionOrRange};
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::ProviderResult;
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::position::Position;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::markers::marker_service::MarkerService;
use je_platform::markers::{IMarkerService, MarkerCode, MarkerData, MarkerSeverity};
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, create_test_code_editor};
use support::test_text_model::create_text_model_with;

const LANGUAGE_ID: &str = "foo-lang";

type Registry = Rc<LanguageFeatureRegistry<dyn CodeActionProvider>>;

fn action_ref(action: CodeAction) -> CodeActionRef {
    Rc::new(RefCell::new(action))
}

fn test_action() -> CodeActionRef {
    let mut action = CodeAction::new("test");
    action.command = Some(Command { id: "test-command".to_owned(), title: "test".to_owned(), tooltip: None, arguments: Some(Vec::new()) });
    action_ref(action)
}

/// `testProvider`.
struct TestProvider;

impl CodeActionProvider for TestProvider {
    fn provide_code_actions(
        &self,
        _model: &Rc<TextModel>,
        _range: &SelectionOrRange,
        _context: &CodeActionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<dyn CodeActionList>> {
        let list: Rc<dyn CodeActionList> = CodeActionListObject::new(vec![test_action()], || {});
        Promise::resolved(Some(list))
    }
}

/// O que `setup()` deixa em variáveis da suíte.
struct Fixture {
    ds: LeakGuard,
    uri: URI,
    marker_service: Rc<MarkerService>,
    model: Rc<TextModel>,
    editor: Rc<TestCodeEditor>,
    registry: Registry,
    context_keys: Rc<ContextKeyService>,
}

impl Fixture {
    fn new() -> Self {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let uri = URI::parse("untitled:path", false);
        let marker_service = MarkerService::new();
        let model = create_text_model_with(
            "foobar  foo bar\nfarboo far boo",
            Some(LANGUAGE_ID),
            RelaxedTextModelCreationOptions::default(),
            Some(uri.clone()),
        );
        let editor = create_test_code_editor(Some(model.clone()), TestCodeEditorInstantiationOptions::default());
        editor.set_position(Position::new(1, 1), None);
        let registry: Registry = Rc::new(LanguageFeatureRegistry::new(None));
        let context_keys = ContextKeyService::new(TestConfigurationService::new(None));
        Self { ds, uri, marker_service, model, editor, registry, context_keys }
    }

    /// `markerService.changeOne('fake', uri, [...])` com o marcador do original.
    fn change_marker(&self) {
        let mut marker = MarkerData::new(MarkerSeverity(1), "error", 1, 1, 1, 6);
        marker.code = Some(MarkerCode::Text(String::new()));
        marker.source = Some(String::new());
        self.marker_service.change_one("fake", &self.uri, vec![marker], None);
    }

    fn marker_service(&self) -> Rc<dyn IMarkerService> {
        self.marker_service.clone()
    }

    /// `new CodeActionModel(editor, registry, markerService, contextKeys, undefined, configurationService?)`.
    fn create_model(&self, configuration_service: Option<Rc<dyn IConfigurationService>>) -> Rc<CodeActionModel> {
        let model = CodeActionModel::new(
            self.editor.widget(),
            &self.registry,
            self.marker_service(),
            &*self.context_keys,
            None,
            configuration_service,
        );
        self.ds.add(Rc::new(DisposeOnDrop(model.clone())) as Rc<dyn IDisposable>);
        model
    }

    /// `teardown()`.
    fn teardown(&self) {
        self.editor.dispose();
        self.model.dispose();
        self.marker_service.dispose();
        self.context_keys.dispose();
    }
}

/// O `store.add(model)`: descarta o modelo no fim do caso (o `dispose` do modelo é idempotente).
struct DisposeOnDrop(Rc<CodeActionModel>);

impl IDisposable for DisposeOnDrop {
    fn dispose(&self) {
        self.0.dispose();
    }
}

fn auto_trigger_only(state: &CodeActionsState) -> Rc<CodeActionsTriggered> {
    // assertType(e.type === CodeActionsState.Type.Triggered)
    let CodeActionsState::Triggered(state) = state else {
        panic!("esperado o estado Triggered, veio {:?}", state.r#type());
    };
    Rc::clone(state)
}

/// O corpo `async` do caso; uma rejeição reprova o caso.
fn run(body: impl std::future::Future<Output = Result<(), Error>> + 'static) {
    run_with_faked_timers(async move { body.await.expect("o caso terminou com erro") });
}

// suite('CodeActionModel')

#[test]
fn oracle_marker_added() {
    run(async {
        let fixture = Rc::new(Fixture::new());
        let (done_promise, done) = Promise::<()>::new();

        fixture.ds.add(fixture.registry.register(LANGUAGE_ID.into(), Rc::new(TestProvider)));

        let model = fixture.create_model(None);
        let listened_model = model.clone();
        fixture.ds.add(model.on_did_change_state().listen(move |e: &CodeActionsState| {
            let e = auto_trigger_only(e);
            assert_eq!(e.trigger.r#type, CodeActionTriggerType::Auto);

            let (model, done) = (listened_model.clone(), done.clone());
            e.actions.then_result(move |fixes| {
                match fixes {
                    Ok(fixes) => {
                        model.dispose();
                        assert_eq!(fixes.valid_actions().len(), 1);
                        done.resolve(());
                    }
                    Err(error) => done.reject(error),
                }
                Ok(())
            });
        }));

        // start here
        fixture.change_marker();
        done_promise.await?;
        fixture.teardown();
        Ok(())
    });
}

#[test]
fn oracle_position_changed() {
    run(async {
        let fixture = Rc::new(Fixture::new());
        fixture.ds.add(fixture.registry.register(LANGUAGE_ID.into(), Rc::new(TestProvider)));

        fixture.change_marker();

        fixture.editor.set_position(Position::new(2, 1), None);

        let (done_promise, done) = Promise::<()>::new();
        let model = fixture.create_model(None);
        let listened_model = model.clone();
        fixture.ds.add(model.on_did_change_state().listen(move |e: &CodeActionsState| {
            let e = auto_trigger_only(e);
            assert_eq!(e.trigger.r#type, CodeActionTriggerType::Auto);
            let (model, done) = (listened_model.clone(), done.clone());
            e.actions.then_result(move |fixes| {
                match fixes {
                    Ok(fixes) => {
                        model.dispose();
                        assert_eq!(fixes.valid_actions().len(), 1);
                        done.resolve(());
                    }
                    Err(error) => done.reject(error),
                }
                Ok(())
            });
        }));
        // start here
        fixture.editor.set_position(Position::new(1, 1), None);
        done_promise.await?;
        fixture.teardown();
        Ok(())
    });
}

#[test]
fn oracle_should_only_auto_trigger_once_for_cursor_and_marker_update_right_after_each_other() {
    run(async {
        let fixture = Rc::new(Fixture::new());
        let (done_promise, done) = Promise::<()>::new();
        fixture.ds.add(fixture.registry.register(LANGUAGE_ID.into(), Rc::new(TestProvider)));

        let trigger_count = Rc::new(Cell::new(0));
        let model = fixture.create_model(None);
        let listened_model = model.clone();
        let counted = trigger_count.clone();
        fixture.ds.add(model.on_did_change_state().listen(move |e: &CodeActionsState| {
            let e = auto_trigger_only(e);
            assert_eq!(e.trigger.r#type, CodeActionTriggerType::Auto);
            counted.set(counted.get() + 1);

            // give time for second trigger before completing test
            let (model, done, counted) = (listened_model.clone(), done.clone(), counted.clone());
            runtime::set_timeout(
                move || {
                    model.dispose();
                    assert_eq!(counted.get(), 1);
                    done.resolve(());
                },
                0,
            );
        }));

        fixture.change_marker();

        fixture.editor.set_selection(Selection::new(1, 1, 4, 1).into(), None);

        done_promise.await?;
        fixture.teardown();
        Ok(())
    });
}

/// O provedor de `disposes manually triggered code actions when the editor model is cleared`.
struct InvokeOnlyProvider {
    dispose_count: Rc<Cell<u32>>,
}

impl CodeActionProvider for InvokeOnlyProvider {
    fn provide_code_actions(
        &self,
        _model: &Rc<TextModel>,
        _range: &SelectionOrRange,
        context: &CodeActionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<dyn CodeActionList>> {
        if context.trigger != CodeActionTriggerType::Invoke {
            return Promise::resolved(None);
        }
        let dispose_count = self.dispose_count.clone();
        let list: Rc<dyn CodeActionList> =
            CodeActionListObject::new(vec![test_action()], move || dispose_count.set(dispose_count.get() + 1));
        Promise::resolved(Some(list))
    }
}

#[test]
fn disposes_manually_triggered_code_actions_when_the_editor_model_is_cleared() {
    run(async {
        let fixture = Rc::new(Fixture::new());
        let dispose_count = Rc::new(Cell::new(0));
        fixture.ds.add(fixture.registry.register(LANGUAGE_ID.into(), Rc::new(InvokeOnlyProvider { dispose_count: dispose_count.clone() })));

        let code_action_model = fixture.create_model(None);
        let (promise, resolve) = Promise::<Rc<je_editor::code_action::code_action_model::CodeActionsTriggered>>::new();
        fixture.ds.add(code_action_model.on_did_change_state().listen(move |state: &CodeActionsState| {
            let CodeActionsState::Triggered(triggered) = state else {
                return;
            };
            if triggered.trigger.r#type != CodeActionTriggerType::Invoke {
                return;
            }
            resolve.resolve(Rc::clone(triggered));
        }));

        code_action_model.trigger(CodeActionTrigger::new(CodeActionTriggerType::Invoke, CodeActionTriggerSource::Default));
        let state = promise.await?;
        state.actions.clone().await?;
        fixture.editor.set_model(None);
        assert_eq!(dispose_count.get(), 1);
        fixture.teardown();
        Ok(())
    });
}

/// O provedor de `assertManualActionsSurviveCanceledAutomaticRequest`: o primeiro pedido espera o resultado
/// automático, o segundo (manual) devolve uma correção rápida.
struct ScriptedProvider {
    automatic_result: Promise<Option<Rc<dyn CodeActionList>>>,
    provide_count: Cell<u32>,
    manual_dispose_count: Rc<Cell<u32>>,
}

impl CodeActionProvider for ScriptedProvider {
    fn provide_code_actions(
        &self,
        _model: &Rc<TextModel>,
        _range: &SelectionOrRange,
        context: &CodeActionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<dyn CodeActionList>> {
        let call = self.provide_count.get();
        self.provide_count.set(call + 1);
        if call == 0 {
            return self.automatic_result.clone();
        }
        assert_eq!(context.trigger, CodeActionTriggerType::Invoke);
        let mut action = CodeAction::with_kind("test", Some("quickfix"));
        action.command = Some(Command {
            id: "test-command".to_owned(),
            title: "test".to_owned(),
            tooltip: None,
            arguments: Some(vec![Dynamic::object([("value", Dynamic::Bool(true))])]),
        });
        let manual_dispose_count = self.manual_dispose_count.clone();
        let list: Rc<dyn CodeActionList> =
            CodeActionListObject::new(vec![action_ref(action)], move || manual_dispose_count.set(manual_dispose_count.get() + 1));
        Promise::resolved(Some(list))
    }
}

async fn assert_manual_actions_survive_canceled_automatic_request(claim_before_automatic_result_completes: bool) -> Result<(), Error> {
    let fixture = Rc::new(Fixture::new());
    let (automatic_result, resolve_automatic_result) = Promise::<Option<Rc<dyn CodeActionList>>>::new();
    let manual_dispose_count = Rc::new(Cell::new(0));
    fixture.ds.add(fixture.registry.register(
        LANGUAGE_ID.into(),
        Rc::new(ScriptedProvider {
            automatic_result,
            provide_count: Cell::new(0),
            manual_dispose_count: manual_dispose_count.clone(),
        }),
    ));

    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(Some(Dynamic::object([(
        "editor.codeActionWidget.includeNearbyQuickFixes",
        Dynamic::Bool(true),
    )])));
    let code_action_model = fixture.create_model(Some(configuration_service));
    let (promise, resolve) = Promise::<Rc<je_editor::code_action::code_action_model::CodeActionsTriggered>>::new();
    fixture.ds.add(code_action_model.on_did_change_state().listen(move |state: &CodeActionsState| {
        if let CodeActionsState::Triggered(triggered) = state
            && triggered.trigger.r#type == CodeActionTriggerType::Invoke
        {
            resolve.resolve(Rc::clone(triggered));
        }
    }));

    code_action_model.trigger(CodeActionTrigger::new(CodeActionTriggerType::Invoke, CodeActionTriggerSource::QuickFix));
    let manual_action_set = promise.await?.actions.clone().await?;
    let mut manual_actions =
        if claim_before_automatic_result_completes { code_action_model.take_code_actions(&manual_action_set) } else { None };

    let empty_list: Rc<dyn CodeActionList> = CodeActionListObject::new(Vec::new(), || {});
    resolve_automatic_result.resolve(Some(empty_list));
    timeout(0).await?;
    if manual_actions.is_none() {
        manual_actions = code_action_model.take_code_actions(&manual_action_set);
    }
    let was_claimed = manual_actions.is_some();
    let dispose_count_before_release = manual_dispose_count.get();
    if let Some(manual_actions) = &manual_actions {
        manual_actions.dispose();
    }

    assert_eq!(
        (was_claimed, dispose_count_before_release, manual_dispose_count.get()),
        (true, 0, 1)
    );
    fixture.teardown();
    Ok(())
}

#[test]
fn does_not_dispose_claimed_manual_code_actions_when_a_canceled_automatic_request_completes() {
    run(assert_manual_actions_survive_canceled_automatic_request(true));
}

#[test]
fn does_not_replace_manual_code_actions_when_a_canceled_automatic_request_completes_before_claim() {
    run(assert_manual_actions_survive_canceled_automatic_request(false));
}
