//! Porte de `src/vs/workbench/test/browser/notificationAccessibleView.test.ts` (suíte `NotificationAccessibleView`), caso a caso e
//! na mesma ordem (9 definições de teste, 11 casos: o laço `primary`/`secondary` roda 2 e o laço de erro e texto roda 2).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; `store.add` vira
//!   `guard.add` / `guard.add_rc`.
//! - **`workbenchInstantiationService(undefined, store)` e `instantiationService.stub(...)`.** É o `TestInstantiationService`
//!   com os serviços que o provedor pede ao `accessor` (`ITelemetryService`, `IListService`, `IAccessibleViewService`,
//!   `IOpenerService`, `ICommandService`, `IAccessibilitySignalService`), todos dublês deste arquivo ou de
//!   `support/notification_doubles.rs`.
//! - **`WorkbenchList` real.** O porte não tem a `WorkbenchList` sobre um contêiner de DOM; [`TestList`] é a lista de
//!   [`IWorkbenchList`] sobre os itens do modelo (`list.splice(0, 0, model.notifications)`), com foco em índice, `setFocus` que
//!   recusa índice fora da lista e `focusNext`/`focusPrevious` que não dão a volta, como a `WorkbenchList` sem `loop`. O
//!   `ICommandService.executeCommand` do original (`list.setFocus([0])`) é [`FocusFirstCommandService`].
//! - **`IAccessibleViewService`** conta só as chamadas de `next` (`upcastPartial({ next })`); os outros métodos reprovam.
//! - **`IOpenerService.open`** guarda o texto do URI e o `allowCommands` recebido (`{ uri, options }`).
//! - **`await action.run()`** é `run` seguido de [`run_microtasks`].
//! - **`action.run === originalRun`.** O contrato `IAction` do porte não expõe `run` como valor; o que o caso prova (a ação
//!   original não foi trocada nem mutada) é conferido pela identidade da ação entregue pelo provedor, que não é a original, e
//!   pela classe dela, que continua sem valor.
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de tuplas, com os mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "../../je-editor/tests/support/test_instantiation_service.rs"]
mod test_instantiation_service;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::actions::{ActionRef, IAction, ToActionProps, to_action};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime::run_microtasks;
use je_base::uri::URI;
use je_editor::code_action::platform::{I_ACCESSIBILITY_SIGNAL_SERVICE, IAccessibilitySignalService};
use je_platform::accessibility::accessible_view::{
    AccessibleContentProvider, AccessibleViewContentProvider, AccessibleViewProviderId, I_ACCESSIBLE_VIEW_SERVICE, IAccessibleViewService, ICodeBlockActionContext, IPosition, NavigationType,
};
use je_platform::commands::commands::{CommandEvent, ICommandService, I_COMMAND_SERVICE};
use je_platform::list::list_service::{I_LIST_SERVICE, IListService, IListWidget, IWorkbenchList};
use je_platform::notification::notification_message::{NotificationPart, NotificationText};
use je_platform::notification::notification_telemetry::{
    NotificationActionTelemetry, NotificationActionTelemetryId, NotificationTelemetry, NotificationTelemetryId, with_notification_action_telemetry,
};
use je_platform::notification::{
    Notification, NotificationActions, NotificationHandle, NotificationMessage, NotificationProgressProperties, NotificationProperties, Severity,
};
use je_platform::opener::opener::{
    AllowCommands, I_OPENER_SERVICE, IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolveExternalUriOptions,
    ResolvedExternalUri,
};
use je_platform::telemetry::{I_TELEMETRY_SERVICE, ITelemetryService};
use je_workbench::browser::parts::notifications::notification_accessible_view::NotificationAccessibleView;
use je_workbench::common::notifications::{INotificationsModel, NotificationViewItem, NotificationsModel};
use je_workbench::test::common::test_notification_telemetry::TestNotificationTelemetryService;
use notification_doubles::SilentAccessibilitySignals;
use test_instantiation_service::TestInstantiationService;

// region: dublês

/// A `WorkbenchList<INotificationViewItem>` do caso (ver as notas do módulo).
struct TestList {
    items: Vec<Rc<NotificationViewItem>>,
    focus: Cell<isize>,
}

impl TestList {
    /// O ponteiro fino que identifica o elemento (`===`).
    fn identity(element: &Rc<dyn Any>) -> *const () {
        Rc::as_ptr(element).cast::<()>()
    }
}

impl IListWidget for TestList {
    fn is_active_element(&self) -> bool {
        true
    }

    fn get_focused_element(&self) -> Option<Rc<dyn Any>> {
        let focus = usize::try_from(self.focus.get()).ok()?;
        self.items.get(focus).map(|item| item.clone() as Rc<dyn Any>)
    }

    fn as_workbench_list(self: Rc<Self>) -> Option<Rc<dyn IWorkbenchList>> {
        Some(self)
    }
}

impl IWorkbenchList for TestList {
    fn index_of(&self, element: &Rc<dyn Any>) -> isize {
        let target = Self::identity(element);
        self.items.iter().position(|item| Rc::as_ptr(item).cast::<()>() == target).map_or(-1, |index| index as isize)
    }

    fn dom_focus(&self) {}

    fn set_focus(&self, indexes: &[isize]) -> Result<(), Error> {
        if indexes.iter().any(|index| *index < 0 || *index as usize >= self.items.len()) {
            return Err(Error::named("ListError", format!("Invalid index {indexes:?}")));
        }
        self.focus.set(indexes.first().copied().unwrap_or(-1));
        Ok(())
    }

    fn focus_next(&self) {
        self.focus.set((self.focus.get() + 1).min(self.items.len() as isize - 1));
    }

    fn focus_previous(&self) {
        self.focus.set((self.focus.get() - 1).max(0));
    }
}

/// `stub(IListService, { lastFocusedList: list })`.
struct FocusedListService {
    list: Rc<TestList>,
}

impl IListService for FocusedListService {
    fn last_focused_list(&self) -> Option<Rc<dyn IListWidget>> {
        let list: Rc<dyn IListWidget> = self.list.clone();
        Some(list)
    }
}

/// `stub(IAccessibleViewService, { next: () => nextCalls++ })`.
struct CountingAccessibleViewService {
    next_calls: Rc<Cell<usize>>,
}

impl IAccessibleViewService for CountingAccessibleViewService {
    fn show(&self, _provider: AccessibleViewContentProvider, _position: Option<IPosition>) {
        panic!("show is not a function")
    }
    fn show_last_provider(&self, _id: AccessibleViewProviderId) {
        panic!("showLastProvider is not a function")
    }
    fn show_accessible_view_help(&self) {
        panic!("showAccessibleViewHelp is not a function")
    }
    fn next(&self) {
        self.next_calls.set(self.next_calls.get() + 1);
    }
    fn previous(&self) {
        panic!("previous is not a function")
    }
    fn navigate_to_code_block(&self, _navigation: NavigationType) {
        panic!("navigateToCodeBlock is not a function")
    }
    fn go_to_symbol(&self) {
        panic!("goToSymbol is not a function")
    }
    fn disable_hint(&self) {
        panic!("disableHint is not a function")
    }
    fn get_position(&self, _id: AccessibleViewProviderId) -> Option<IPosition> {
        panic!("getPosition is not a function")
    }
    fn set_position(&self, _position: IPosition, _reveal: Option<bool>, _select: Option<bool>) {
        panic!("setPosition is not a function")
    }
    fn get_last_position(&self) -> Option<IPosition> {
        panic!("getLastPosition is not a function")
    }
    fn get_open_aria_hint(&self, _verbosity_setting_key: &str) -> Option<String> {
        panic!("getOpenAriaHint is not a function")
    }
    fn get_code_block_context(&self) -> Option<ICodeBlockActionContext> {
        panic!("getCodeBlockContext is not a function")
    }
    fn configure_keybindings(&self, _unassigned: bool) {
        panic!("configureKeybindings is not a function")
    }
    fn open_help_link(&self) {
        panic!("openHelpLink is not a function")
    }
}

/// O `IOpenerService` do caso: `opened.push({ uri: uri.toString(), options })`.
#[derive(Default)]
struct OptionsRecordingOpener {
    opened: Rc<RefCell<Vec<(String, Option<AllowCommands>)>>>,
}

impl IOpenerService for OptionsRecordingOpener {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        unreachable!("o caso não registra opener")
    }
    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        unreachable!("o caso não registra validador")
    }
    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        unreachable!("o caso não registra resolvedor")
    }
    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        unreachable!("o caso não troca o opener externo")
    }
    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        unreachable!("o caso não registra opener externo")
    }
    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool> {
        let text = match resource {
            OpenResource::Uri(uri) => uri.to_string(false).to_string_lossy(),
            OpenResource::Text(text) => text.to_string_lossy(),
        };
        self.opened.borrow_mut().push((text, options.and_then(|options| options.allow_commands.clone())));
        Promise::resolved(true)
    }
    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        unreachable!("o caso não resolve URI externo")
    }
}

/// `stub(ICommandService, { executeCommand: async () => { list.setFocus([0]); return undefined; } })`.
struct FocusFirstCommandService {
    list: Rc<TestList>,
}

impl ICommandService for FocusFirstCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn execute_command(&self, _command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        let _ = self.list.set_focus(&[0]);
        Promise::resolved(Dynamic::Undefined)
    }
}

// endregion

// region: montagem

type Opened = Rc<RefCell<Vec<(String, Option<AllowCommands>)>>>;

/// O que `setupNotification` devolve.
struct Setup {
    instantiation: Rc<TestInstantiationService>,
    telemetry: Rc<TestNotificationTelemetryService>,
    handles: Vec<Rc<dyn NotificationHandle>>,
    opened: Opened,
    next_calls: Rc<Cell<usize>>,
    implementation: NotificationAccessibleView,
}

/// O que `createProvider` devolve: o provedor e os recursos do `onOpen` (`showing`).
struct CreatedProvider {
    provider: Rc<AccessibleContentProvider>,
    showing: DisposableRef,
}

/// `setupNotification(notifications)`.
fn setup_notification(guard: &LeakGuard, notifications: Vec<Notification>) -> Setup {
    let instantiation = guard.add_rc(TestInstantiationService::new());
    let telemetry = Rc::new(TestNotificationTelemetryService::new());
    let telemetry_service: Rc<dyn ITelemetryService> = telemetry.clone();
    instantiation.stub(I_TELEMETRY_SERVICE, telemetry_service);
    let model = guard.add_rc(NotificationsModel::new());
    let mut handles: Vec<Rc<dyn NotificationHandle>> = notifications
        .into_iter()
        .rev()
        .map(|notification| {
            let handle = model.add_notification(notification);
            let to_close = handle.clone();
            guard.add(to_disposable(move || to_close.close()));
            handle
        })
        .collect();
    handles.reverse();
    let list = Rc::new(TestList { items: model.notifications(), focus: Cell::new(0) });
    let list_service: Rc<dyn IListService> = Rc::new(FocusedListService { list: list.clone() });
    instantiation.stub(I_LIST_SERVICE, list_service);
    let next_calls = Rc::new(Cell::new(0));
    let accessible_view_service: Rc<dyn IAccessibleViewService> = Rc::new(CountingAccessibleViewService { next_calls: next_calls.clone() });
    instantiation.stub(I_ACCESSIBLE_VIEW_SERVICE, accessible_view_service);
    let opener = Rc::new(OptionsRecordingOpener::default());
    let opened = opener.opened.clone();
    let opener_service: Rc<dyn IOpenerService> = opener;
    instantiation.stub(I_OPENER_SERVICE, opener_service);
    let command_service: Rc<dyn ICommandService> = Rc::new(FocusFirstCommandService { list });
    instantiation.stub(I_COMMAND_SERVICE, command_service);
    let signals: Rc<dyn IAccessibilitySignalService> = Rc::new(SilentAccessibilitySignals);
    instantiation.stub(I_ACCESSIBILITY_SIGNAL_SERVICE, signals);
    Setup { instantiation, telemetry, handles, opened, next_calls, implementation: NotificationAccessibleView }
}

impl Setup {
    /// `handle`: o primeiro.
    fn handle(&self) -> &Rc<dyn NotificationHandle> {
        &self.handles[0]
    }

    fn next_calls(&self) -> usize {
        self.next_calls.get()
    }

    /// `createProvider()`.
    fn create_provider(&self, guard: &LeakGuard) -> CreatedProvider {
        let provider = self.instantiation.invoke_function(|accessor| self.implementation.get_content_provider(accessor)).expect("o provedor existe");
        guard.add_rc(provider.clone());
        let on_open = provider.on_open.as_ref().expect("onOpen existe");
        let showing = on_open().expect("onOpen devolve os recursos");
        CreatedProvider { provider, showing: guard.add(showing) }
    }
}

/// O texto do `event.<key>` de um evento de telemetria.
fn field(event: &Dynamic, key: &str) -> String {
    match event.get_str(key) {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava texto em {key}, veio {other:?}"),
    }
}

fn info(message: impl Into<NotificationMessage>) -> Notification {
    Notification::new(Severity::Info, message)
}

/// `provider.actions!.find(predicate)!`.
fn find_action(provider: &AccessibleContentProvider, predicate: impl Fn(&ActionRef) -> bool) -> ActionRef {
    provider.actions().expect("o provedor tem ações").into_iter().find(|action| predicate(action)).expect("a ação existe")
}

/// `provider.actions!.filter(id.startsWith('notification.link.'))`.
fn link_actions(provider: &AccessibleContentProvider) -> Vec<ActionRef> {
    provider.actions().expect("o provedor tem ações").into_iter().filter(|action| action.id().starts_with("notification.link.")).collect()
}

/// `await action.run()`.
fn run(action: &ActionRef) {
    action.run(&[]);
    run_microtasks();
}

/// `{ uri, options: { allowCommands: true } }`.
fn opened_with_commands(uri: &str) -> (String, Option<AllowCommands>) {
    (uri.to_owned(), Some(AllowCommands::Bool(true)))
}

// endregion

fn records_exposure_and_action_once(role_is_primary: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let invoked = Rc::new(Cell::new(0));
    let action = {
        let invoked = invoked.clone();
        with_notification_action_telemetry(
            to_action(ToActionProps {
                id: "private ID".to_owned(),
                label: "Private label".to_owned(),
                tooltip: None,
                enabled: None,
                checked: None,
                class: None,
                alt: None,
                run: Rc::new(move |_| {
                    invoked.set(invoked.get() + 1);
                    MaybePromise::Value(Dynamic::Undefined)
                }),
            }),
            Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::Continue)),
        )
    };
    let actions = if role_is_primary {
        NotificationActions { primary: vec![action.clone()], secondary: Vec::new() }
    } else {
        NotificationActions { primary: Vec::new(), secondary: vec![action.clone()] }
    };
    let notification = setup_notification(
        &guard,
        vec![Notification {
            properties: NotificationProperties { telemetry: Some(NotificationTelemetry::Id(NotificationTelemetryId::AuthenticationContinue)), ..NotificationProperties::default() },
            actions: Some(actions),
            ..info("private message")
        }],
    );
    notification.create_provider(&guard).showing.dispose();
    let CreatedProvider { provider, .. } = notification.create_provider(&guard);
    run(&find_action(&provider, |candidate| candidate.id() == action.id()));
    let role = if role_is_primary { "primary" } else { "secondary" };
    let shown: Vec<String> = notification.telemetry.shown().iter().map(|event| field(event, "surface")).collect();
    let interactions: Vec<(String, String, String, String)> = notification
        .telemetry
        .interactions()
        .iter()
        .map(|event| (field(event, "interaction"), field(event, "actionRole"), field(event, "actionId"), field(event, "surface")))
        .collect();
    let private_payload = notification
        .telemetry
        .events()
        .iter()
        .any(|(name, data)| name.contains("private") || json_stringify(data).is_some_and(|text| text.contains("private")));
    assert_eq!(
        (invoked.get(), shown, interactions, private_payload),
        (
            1,
            vec!["accessibleView".to_owned()],
            vec![(format!("{role}Action"), role.to_owned(), "continue".to_owned(), "accessibleView".to_owned())],
            false
        )
    );
}

#[test]
fn records_accessible_view_exposure_and_primary_action_once_without_collecting_content() {
    records_exposure_and_action_once(true);
}

#[test]
fn records_accessible_view_exposure_and_secondary_action_once_without_collecting_content() {
    records_exposure_and_action_once(false);
}

#[test]
fn accessible_view_clear_is_a_dismissal_not_progress_cancellation_or_an_automatic_close_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let notification = setup_notification(
        &guard,
        vec![Notification { progress: Some(NotificationProgressProperties { infinite: true, total: None, worked: None }), ..info("Progress") }],
    );
    let CreatedProvider { provider, .. } = notification.create_provider(&guard);
    run(&find_action(&provider, |action| action.id() == "clearNotification"));
    notification.handle().close();
    let interactions: Vec<(String, String)> = notification.telemetry.interactions().iter().map(|event| (field(event, "interaction"), field(event, "surface"))).collect();
    assert_eq!(interactions, vec![("dismiss".to_owned(), "accessibleView".to_owned())]);
}

#[test]
fn releases_close_listeners_when_show_resources_are_disposed_while_the_notification_survives() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let notification = setup_notification(&guard, vec![info("Synthetic notification")]);
    for _ in 0..3 {
        notification.create_provider(&guard).showing.dispose();
    }
    notification.handle().close();
    assert_eq!(notification.next_calls(), 0);
}

#[test]
fn only_the_surviving_provider_responds_when_the_notification_closes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let notification = setup_notification(&guard, vec![info("Synthetic notification")]);
    notification.create_provider(&guard).showing.dispose();
    let provider = notification.create_provider(&guard);
    notification.handle().close();
    provider.showing.dispose();
    assert_eq!(notification.next_calls(), 1);
}

#[test]
fn exposes_explicit_command_and_documentation_links_without_parsing_literal_fragments_or_labels() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let literal = "README.md [Open](command:unexpected)";
    let link_label = "[Documentation](command:unexpected)";
    let notification = setup_notification(
        &guard,
        vec![info(NotificationText::concat([
            NotificationPart::from(literal),
            NotificationPart::from("x".repeat(1001)),
            NotificationPart::from(NotificationText::link(link_label, "https://example.com/docs", None)),
            NotificationPart::from(" "),
            NotificationPart::from(NotificationText::link("Show Logs", "command:showLogs?%5B%22file%22%5D", Some("Open Log"))),
        ]))],
    );
    let CreatedProvider { provider, .. } = notification.create_provider(&guard);
    let links = link_actions(&provider);
    for link in &links {
        run(link);
    }

    let literal_present = provider.provide_content().contains(literal);
    let link_texts: Vec<(String, String)> = links.iter().map(|action| (action.label(), action.tooltip())).collect();
    let opened = notification.opened.borrow().clone();
    assert_eq!(
        (literal_present, link_texts, opened, notification.next_calls()),
        (
            true,
            vec![(link_label.to_owned(), "https://example.com/docs".to_owned()), ("Show Logs".to_owned(), "Open Log".to_owned())],
            vec![opened_with_commands("https://example.com/docs"), opened_with_commands("command:showLogs?%5B%22file%22%5D")],
            0
        )
    );
}

fn does_not_add_actions_for_links_embedded_in_literal(error: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let message = "README.md [Open](command:unexpected) [Help](https://example.com)";
    let notification = setup_notification(
        &guard,
        vec![Notification::new(Severity::Error, if error { NotificationMessage::Error(Error::new(message)) } else { NotificationMessage::Text(message.to_owned()) })],
    );
    let CreatedProvider { provider, .. } = notification.create_provider(&guard);
    assert_eq!((provider.provide_content().contains(message), link_actions(&provider).len()), (true, 0));
}

#[test]
fn does_not_add_actions_for_links_embedded_in_literal_strings() {
    does_not_add_actions_for_links_embedded_in_literal(false);
}

#[test]
fn does_not_add_actions_for_links_embedded_in_literal_errors() {
    does_not_add_actions_for_links_embedded_in_literal(true);
}

#[test]
fn updates_link_actions_and_removes_them_when_a_message_becomes_literal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let notification = setup_notification(&guard, vec![info(NotificationText::link("Details", "command:first", None))]);
    let CreatedProvider { provider, showing } = notification.create_provider(&guard);
    let changes = Rc::new(Cell::new(0));
    {
        let changes = changes.clone();
        let on_did_change_content = provider.on_did_change_content.as_ref().expect("onDidChangeContent existe");
        guard.add(on_did_change_content.listen(move |_| changes.set(changes.get() + 1)));
    }

    notification.handle().update_message(NotificationMessage::Rich(NotificationText::link("Details", "command:second", None)));
    run(&link_actions(&provider).into_iter().next().expect("o link existe"));
    notification.handle().update_message(NotificationMessage::Text("Details".to_owned()));
    let plain_links = link_actions(&provider).len();
    showing.dispose();
    notification.handle().update_message(NotificationMessage::Text("Update after closing the view".to_owned()));

    let opened = notification.opened.borrow().clone();
    assert_eq!((opened, plain_links, changes.get()), (vec![opened_with_commands("command:second")], 0, 2));
}

#[test]
fn keeps_navigation_actions_and_close_listeners_with_the_currently_displayed_notification() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let notification = setup_notification(
        &guard,
        vec![info(NotificationText::link("First", "command:first", None)), info(NotificationText::link("Second", "command:second", None))],
    );
    let CreatedProvider { provider, .. } = notification.create_provider(&guard);
    let second_content = (provider.provide_next_content.as_ref().expect("provideNextContent existe"))();
    let second_link = link_actions(&provider).into_iter().next().expect("o link existe");
    run(&second_link);
    let first_content = (provider.provide_previous_content.as_ref().expect("providePreviousContent existe"))();
    let first_link = link_actions(&provider).into_iter().next().expect("o link existe");
    run(&first_link);
    notification.handles[1].close();
    let next_calls_after_other_closes = notification.next_calls();
    notification.handles[0].close();

    let opened = notification.opened.borrow().clone();
    assert_eq!(
        (
            second_content.is_some_and(|content| content.contains("Second")),
            first_content.is_some_and(|content| content.contains("First")),
            vec![second_link.label(), first_link.label()],
            opened,
            next_calls_after_other_closes,
            notification.next_calls()
        ),
        (
            true,
            true,
            vec!["Second".to_owned(), "First".to_owned()],
            vec![opened_with_commands("command:second"), opened_with_commands("command:first")],
            0,
            1
        )
    );
}

#[test]
fn does_not_mutate_existing_actions_when_reopening_or_updating_the_accessible_view() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let calls = Rc::new(Cell::new(0));
    let action = {
        let calls = calls.clone();
        to_action(ToActionProps {
            id: "original".to_owned(),
            label: "Original Action".to_owned(),
            tooltip: None,
            enabled: None,
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                calls.set(calls.get() + 1);
                MaybePromise::Value(Dynamic::Undefined)
            }),
        })
    };
    let notification = setup_notification(
        &guard,
        vec![Notification { actions: Some(NotificationActions { primary: vec![action.clone()], secondary: Vec::new() }), ..info("Message") }],
    );
    notification.create_provider(&guard).showing.dispose();
    let CreatedProvider { provider, .. } = notification.create_provider(&guard);
    notification.handle().update_message(NotificationMessage::Rich(NotificationText::link("Help", "https://example.com", None)));
    let delivered = find_action(&provider, |candidate| candidate.id() == action.id());
    run(&delivered);

    let original_preserved = !std::ptr::addr_eq(Rc::as_ptr(&delivered), Rc::as_ptr(&action));
    assert_eq!((calls.get(), original_preserved, action.class(), notification.next_calls()), (1, true, None, 1));
}
