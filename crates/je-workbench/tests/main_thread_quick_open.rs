//! Porte de `src/vs/workbench/api/test/browser/mainThreadQuickOpen.test.ts` (suíte `MainThreadQuickOpen pending item lifetime`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; os casos `async` rodam em
//!   `run_with_faked_timers`, e o `store.add(new MainThreadQuickOpen(...))` é `guard.add_rc`.
//! - `new class extends mock<IQuickInputService>() { override pick ... }` é o [`MockQuickInputService`]: só o `pick` (e a
//!   sobrecarga `canPickMany: true`, o `pick_many` do porte) responde, e todo outro membro falha com `X is not a function`,
//!   como o membro ausente do objeto vazio do `mock()`. O `DeferredPromise` do original é o par `Promise`/`Resolver`, e
//!   `close`, `accept` e `error` do `operations[n]` são os métodos de [`Operation`].
//! - O `throw pickError` síncrono de `pick` é `errors::throw` (o pânico do porte), que o `$show` captura.
//! - `SingleProxyRPCProtocol({ $onItemSelected() { } })` é um dublê de `ExtHostQuickOpen` que só aceita `$onItemSelected`.
//!   Os `mock<ILabelService>()`, `mock<ICustomEditorLabelService>()`, `mock<IModelService>()` e `mock<ILanguageService>()`
//!   nunca são chamados pelos casos: o rótulo é o `MockLabelService` dos serviços de teste, o do editor customizado e o de
//!   modelos são dublês que falham se chamados, e o de linguagem é o `LanguageService` real, sem registros.
//! - `Object.keys(Reflect.get(service, '_items'))` é [`MainThreadQuickOpen::pending_items`] (chaves em ordem crescente).
//! - `for (const canPickMany of [false, true])` e `for (const completion of ...)` são laços dentro de cada caso (o original
//!   gera um `test()` por combinação com o mesmo corpo); `assert.rejects(result, error)` é a comparação do erro rejeitado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::observable::base::IObservable;
use je_base::promise::{Promise, Resolver};
use je_base::uri::URI;
use je_editor::language_service::{ILanguageSelection, LanguageService};
use je_editor::model::TextModelCreationOptions;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::text_model_edit_source::TextModelEditSource;
use je_platform::quickinput::quick_input::{
    IInputBox, IInputOptions, IKeyMods, IPickOptions, IQuickInputService, IQuickNavigateConfiguration, IQuickPick, IQuickPickItem, IQuickTree,
    QuickInputAlignment, QuickInputAlignmentSetting, QuickInputAnchor, QuickInputAnchorPosition, QuickInputButtonRef, QuickInputHideReason, QuickInputRef,
    QuickPickCreateOptions, QuickPickInput, QuickPicks,
};
use je_workbench::api::browser::main_thread_quick_open::MainThreadQuickOpen;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostQuickOpenShape, MainThreadQuickOpenShape, QuickOpenShowResult, TransferQuickPickItem, TransferQuickPickItemOrSeparator,
};
use je_workbench::services::editor::custom_editor_label_service::ICustomEditorLabelService;
use je_workbench::test::browser::workbench_test_services::MockLabelService;
use support::test_rpc_protocol::single_proxy_rpc_protocol;

/// `{ $onItemSelected() { } }`.
struct OnItemSelectedOnly;

fn absent(member: &str) -> ! {
    panic!("{member} is not a function")
}

impl ExtHostQuickOpenShape for OnItemSelectedOnly {
    fn rpc_on_item_selected(&self, _handle: u32) {}

    fn rpc_validate_input(&self, _input: &str) -> Promise<je_platform::quickinput::quick_input::InputValidationResult> {
        absent("$validateInput")
    }

    fn rpc_on_did_change_active(&self, _session_id: u32, _handles: Vec<u32>) {
        absent("$onDidChangeActive")
    }

    fn rpc_on_did_change_selection(&self, _session_id: u32, _handles: Vec<u32>) {
        absent("$onDidChangeSelection")
    }

    fn rpc_on_did_accept(&self, _session_id: u32) {
        absent("$onDidAccept")
    }

    fn rpc_on_did_change_value(&self, _session_id: u32, _value: &str) {
        absent("$onDidChangeValue")
    }

    fn rpc_on_did_trigger_button(&self, _session_id: u32, _handle: Option<i64>, _checked: Option<bool>) {
        absent("$onDidTriggerButton")
    }

    fn rpc_on_did_trigger_item_button(&self, _session_id: u32, _item_handle: Option<u32>, _button_handle: Option<i64>, _checked: Option<bool>) {
        absent("$onDidTriggerItemButton")
    }

    fn rpc_on_did_hide(&self, _session_id: u32) {
        absent("$onDidHide")
    }
}

/// `mock<ICustomEditorLabelService>()`.
struct MockCustomEditorLabelService;

impl ICustomEditorLabelService for MockCustomEditorLabelService {
    fn on_did_change(&self) -> Event<()> {
        panic!("onDidChange is not defined")
    }

    fn get_name(&self, _resource: &URI) -> Option<String> {
        absent("getName")
    }
}

/// `mock<IModelService>()`.
struct MockModelService;

impl IModelService for MockModelService {
    fn get_model(&self, _resource: &URI) -> Option<Rc<TextModel>> {
        absent("getModel")
    }

    fn get_models(&self) -> Vec<Rc<TextModel>> {
        absent("getModels")
    }

    fn on_model_added(&self) -> Event<Rc<TextModel>> {
        panic!("onModelAdded is not defined")
    }

    fn get_creation_options(&self, _language: &str, _resource: Option<&URI>, _is_for_simple_widget: bool) -> TextModelCreationOptions {
        absent("getCreationOptions")
    }

    fn create_model(&self, _value: TextSource, _language_selection: Option<&dyn ILanguageSelection>, _resource: Option<&URI>, _is_for_simple_widget: bool) -> Rc<TextModel> {
        absent("createModel")
    }

    fn update_model(&self, _model: &TextModel, _value: TextSource, _reason: Option<TextModelEditSource>) {
        absent("updateModel")
    }

    fn destroy_model(&self, _resource: &URI) {
        absent("destroyModel")
    }
}

/// Como a operação do `operations` do original termina o `pick`: o `result` (`DeferredPromise`) do item único ou da lista.
enum Outcome {
    One(Resolver<Option<IQuickPickItem>>),
    Many(Resolver<Option<Vec<IQuickPickItem>>>),
}

/// `{ close(), accept(), error(error) }`.
#[derive(Clone)]
struct Operation {
    contents: Promise<Vec<QuickPickInput>>,
    outcome: Rc<Outcome>,
}

impl Operation {
    /// `close: () => result.complete(undefined)`.
    fn close(&self) {
        match &*self.outcome {
            Outcome::One(result) => result.resolve(None),
            Outcome::Many(result) => result.resolve(None),
        }
    }

    /// `error: error => result.error(error)`.
    fn error(&self, error: Error) {
        match &*self.outcome {
            Outcome::One(result) => result.reject(error),
            Outcome::Many(result) => result.reject(error),
        }
    }

    /// `accept`: os itens (sem separadores) como resultado, a lista inteira ou só o primeiro.
    async fn accept(&self) {
        let items: Vec<IQuickPickItem> = self.contents.clone().await.expect("os itens chegaram").into_iter().filter_map(QuickPickInput::into_item).collect();
        match &*self.outcome {
            Outcome::One(result) => result.resolve(items.first().cloned()),
            Outcome::Many(result) => result.resolve(Some(items)),
        }
    }
}

/// `new class extends mock<IQuickInputService>() { override pick ... }`.
struct MockQuickInputService {
    pick_error: Option<Error>,
    operations: Rc<RefCell<Vec<Operation>>>,
}

impl MockQuickInputService {
    /// O corpo comum das sobrecargas de `pick`.
    fn start(&self, picks: QuickPicks, outcome: Outcome, reject: impl Fn(&Error) + 'static) {
        let contents = picks.into_promise();
        // Like the real widget, item production errors reject the operation.
        let on_error = contents.then_result(move |result| {
            if let Err(error) = &result {
                reject(error);
            }
            Ok(())
        });
        let _ = on_error;
        self.operations.borrow_mut().push(Operation { contents, outcome: Rc::new(outcome) });
    }
}

impl IQuickInputService for MockQuickInputService {
    fn pick(&self, picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<IQuickPickItem>> {
        if let Some(error) = &self.pick_error {
            errors::throw(error.clone());
        }
        let (promise, resolver) = Promise::new();
        let on_error = resolver.clone();
        self.start(picks, Outcome::One(resolver), move |error| on_error.reject(error.clone()));
        promise
    }

    fn pick_many(&self, picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<Vec<IQuickPickItem>>> {
        if let Some(error) = &self.pick_error {
            errors::throw(error.clone());
        }
        let (promise, resolver) = Promise::new();
        let on_error = resolver.clone();
        self.start(picks, Outcome::Many(resolver), move |error| on_error.reject(error.clone()));
        promise
    }

    fn back_button(&self) -> QuickInputButtonRef {
        panic!("backButton is not defined")
    }

    fn on_show(&self) -> Event<()> {
        panic!("onShow is not defined")
    }

    fn on_hide(&self) -> Event<()> {
        panic!("onHide is not defined")
    }

    fn alignment(&self) -> IObservable<QuickInputAlignment> {
        panic!("alignment is not defined")
    }

    fn with_quick_input_anchor(
        &self,
        _anchor: Option<QuickInputAnchor>,
        _anchor_position: Option<QuickInputAnchorPosition>,
        _operation: Box<dyn FnOnce() -> Promise<Dynamic>>,
    ) -> Promise<Dynamic> {
        absent("withQuickInputAnchor")
    }

    fn input(&self, _options: Option<&IInputOptions>, _token: Option<&CancellationToken>) -> Promise<Option<String>> {
        absent("input")
    }

    fn create_quick_pick(&self, _options: Option<QuickPickCreateOptions>) -> Rc<dyn IQuickPick> {
        absent("createQuickPick")
    }

    fn create_input_box(&self) -> Rc<dyn IInputBox> {
        absent("createInputBox")
    }

    fn create_quick_tree(&self) -> Rc<dyn IQuickTree> {
        absent("createQuickTree")
    }

    fn focus(&self) {
        absent("focus")
    }

    fn toggle(&self) {
        absent("toggle")
    }

    fn navigate(&self, _next: bool, _quick_navigate: Option<&IQuickNavigateConfiguration>) {
        absent("navigate")
    }

    fn back(&self) -> Promise<()> {
        absent("back")
    }

    fn accept(&self, _key_mods: Option<IKeyMods>) -> Promise<()> {
        absent("accept")
    }

    fn cancel(&self, _reason: Option<QuickInputHideReason>) -> Promise<()> {
        absent("cancel")
    }

    fn toggle_hover(&self) {
        absent("toggleHover")
    }

    fn current_quick_input(&self) -> Option<QuickInputRef> {
        panic!("currentQuickInput is not defined")
    }

    fn set_alignment(&self, _alignment: QuickInputAlignmentSetting) {
        absent("setAlignment")
    }
}

/// `createService(pickError?)`.
struct Fixture {
    service: Rc<MainThreadQuickOpen>,
    operations: Rc<RefCell<Vec<Operation>>>,
}

impl Fixture {
    /// `operations[index]`.
    fn operation(&self, index: usize) -> Operation {
        self.operations.borrow()[index].clone()
    }

    /// `pending()`.
    fn pending(&self) -> Vec<u32> {
        self.service.pending_items()
    }

    /// `service.$show(instance, { canPickMany }, CancellationToken.None)`.
    fn show(&self, instance: u32, can_pick_many: Option<bool>) -> Promise<Option<QuickOpenShowResult>> {
        self.service.rpc_show(instance, IPickOptions { can_pick_many, ..IPickOptions::default() }, CancellationToken::None)
    }
}

fn create_service(guard: &LeakGuard, pick_error: Option<Error>) -> Fixture {
    let operations: Rc<RefCell<Vec<Operation>>> = Rc::default();
    let quick_input = Rc::new(MockQuickInputService { pick_error, operations: operations.clone() });
    let proxy: Rc<dyn ExtHostQuickOpenShape> = Rc::new(OnItemSelectedOnly);
    let language_service = guard.add_rc(LanguageService::new(false));
    let service = guard.add_rc(MainThreadQuickOpen::new(
        &*single_proxy_rpc_protocol(proxy),
        quick_input,
        MockLabelService::new(None),
        Rc::new(MockCustomEditorLabelService),
        Rc::new(MockModelService),
        language_service,
    ));
    Fixture { service, operations }
}

/// `{ label, handle }`.
fn item(label: &str, handle: u32) -> TransferQuickPickItemOrSeparator {
    TransferQuickPickItemOrSeparator::Item(TransferQuickPickItem { label: Some(label.to_owned()), handle, ..TransferQuickPickItem::default() })
}

#[test]
fn releases_pending_items_after_completion() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        for can_pick_many in [false, true] {
            for completion in ["close", "cancel", "error"] {
                let fixture = create_service(&guard, None);
                let result = fixture.show(1, Some(can_pick_many));
                assert_eq!(fixture.pending(), vec![1], "completion={completion}, multiple={can_pick_many}");
                if completion == "close" {
                    fixture.operation(0).close();
                    assert_eq!(result.await.expect("fechou sem erro"), None);
                } else {
                    let error = if completion == "cancel" { Error::cancellation() } else { Error::new("Owned widget error") };
                    fixture.operation(0).error(error.clone());
                    assert_eq!(result.await.expect_err("rejeitou"), error);
                }
                assert_eq!(fixture.pending(), Vec::<u32>::new(), "completion={completion}, multiple={can_pick_many}");
            }
        }
    });
}

#[test]
fn preserves_accepted_items() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        for can_pick_many in [false, true] {
            let fixture = create_service(&guard, None);
            let result = fixture.show(1, Some(can_pick_many));
            fixture.service.rpc_set_items(1, vec![item("first", 3), item("second", 8)]).await.expect("setItems");
            assert_eq!(fixture.pending(), Vec::<u32>::new());
            fixture.operation(0).accept().await;
            let expected = if can_pick_many { QuickOpenShowResult::Many(vec![3, 8]) } else { QuickOpenShowResult::One(3) };
            assert_eq!(result.await.expect("aceitou"), Some(expected), "multiple={can_pick_many}");
        }
    });
}

#[test]
fn item_production_errors_preserve_their_original_error_and_release_the_request() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let fixture = create_service(&guard, None);
        let error = Error::new("Owned item production error");
        let result = fixture.show(1, None);
        fixture.service.rpc_set_error(1, error.clone()).await.expect("setError");
        assert_eq!(result.await.expect_err("rejeitou"), error);
        assert_eq!(fixture.pending(), Vec::<u32>::new());
    });
}

#[test]
fn synchronous_widget_creation_errors_release_the_request() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let error = Error::new("Owned widget creation error");
        let fixture = create_service(&guard, Some(error.clone()));
        assert_eq!(fixture.show(1, None).await.expect_err("rejeitou"), error);
        assert_eq!(fixture.pending(), Vec::<u32>::new());
    });
}

#[test]
fn settlement_only_releases_its_own_request_and_late_items_are_harmless() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let fixture = create_service(&guard, None);
        let first = fixture.show(1, None);
        let second = fixture.show(2, None);
        fixture.operation(0).close();
        first.await.expect("primeiro fechou");
        assert_eq!(fixture.pending(), vec![2]);
        fixture.service.rpc_set_items(1, vec![item("late", 9)]).await.expect("setItems tardio");
        fixture.service.rpc_set_error(1, Error::new("Ignored late error")).await.expect("setError tardio");
        assert_eq!(fixture.pending(), vec![2]);
        fixture.service.rpc_set_items(2, vec![item("current", 4)]).await.expect("setItems");
        fixture.operation(1).accept().await;
        assert_eq!(second.await.expect("segundo aceitou"), Some(QuickOpenShowResult::One(4)));
        assert_eq!(fixture.pending(), Vec::<u32>::new());
    });
}
