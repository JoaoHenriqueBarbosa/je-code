//! Porte de `src/vs/workbench/test/browser/notificationTelemetry.test.ts` (suíte `Notification telemetry UI`), caso a caso e na
//! mesma ordem (13 casos).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; `resources.add` vira
//!   `store.add` / `store.add_rc`. O `suiteSetup` de aquecimento é [`warm_up`], sobre um `DisposableStore` próprio, antes do
//!   guarda de cada caso (o rastreador e as `static` são por fio).
//! - **`workbenchInstantiationService(undefined, resources)` e `instantiation.stub(...)`.** É o `TestInstantiationService`
//!   com os serviços que os comandos pedem ao `accessor` (`INotificationService`, `ITelemetryService`, `IListService`,
//!   `IAccessibilitySignalService`, `ICommandService`); os mesmos dublês entram à mão nos serviços do centro e dos toasts.
//!   O `ICommandService` executa o comando registrado (`execute`), como o `stub(ICommandService, { executeCommand })`.
//! - **`IContextMenuService.showContextMenu`.** Há dois contratos no porte (o do `DropdownMenuActionViewItem` e o do
//!   `ButtonWithDropdown`); [`RecordingMenus`] guarda os delegados de cada um em `menus`.
//! - **`IListService.lastFocusedList`.** O centro do porte não registra a lista num `IListService` (não há `WorkbenchList`):
//!   [`FocusedNotificationListService`] devolve, com o centro visível, uma lista cujo elemento em foco é a primeira
//!   notificação do modelo, que é o que a lista do centro tem em foco nos casos que dependem dele. O caso das linhas
//!   visíveis (`list.scrollTop`, `scrollHeight`, `renderHeight`) rola a lista com o evento de roda do `je-ui`.
//! - **DOM vira árvore do `je-ui`.** `document.body.appendChild(container)` é um filho da raiz; `querySelector` é a busca por
//!   classe na subárvore; `button.click()`, `dispatchEvent(new KeyboardEvent('keydown', ...))` e `dispatchEvent(new
//!   MouseEvent(...))` são `Ui::dispatch` de um evento do mesmo tipo (`auxclick` do botão do meio é o `PointerUp` auxiliar).
//! - **`await Promise.resolve()`** é [`run_microtasks`]; **`ui.render()`** são dois quadros (`Ui::run_animation_frame`), cada
//!   um seguido das microtarefas.
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de tuplas, com os mesmos campos, na mesma ordem; os eventos de
//!   telemetria (`Dynamic`) são lidos campo a campo por [`field`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "../../je-editor/tests/support/test_instantiation_service.rs"]
mod test_instantiation_service;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::actions::{ActionRef, IAction, IActionRunner, ToActionProps, to_action};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime::run_microtasks;
use je_editor::code_action::platform::{I_ACCESSIBILITY_SIGNAL_SERVICE, IAccessibilitySignalService};
use je_editor::core_2d::dimension::Dimension;
use je_platform::commands::commands::{CommandEvent, ICommandService, I_COMMAND_SERVICE, commands_registry};
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::list::list_service::{I_LIST_SERVICE, IListService, IListWidget};
use je_platform::notification::notification_message::NotificationText;
use je_platform::notification::notification_telemetry::{
    NotificationActionTelemetry, NotificationActionTelemetryId, NotificationTelemetry, NotificationTelemetryId, with_notification_action_telemetry,
};
use je_platform::notification::{
    BasePromptChoice, INotificationService, I_NOTIFICATION_SERVICE, NeverShowAgainOptions, Notification, NotificationActions, NotificationFilterSetting, NotificationHandle, NotificationMessage,
    NotificationPriority, NotificationProgressProperties, NotificationSourceOrText, NotificationsFilter, PromptChoice, PromptChoiceKind, PromptChoiceWithMenu, PromptOptions, Severity,
};
use je_platform::storage::storage::{AbstractStorageService, InMemoryStorageService, StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::{I_TELEMETRY_SERVICE, ITelemetryService};
use je_render::kurbo::Rect;
use je_storage::StorageValue;
use je_ui::{DeltaMode, EventKind, KeyData, Modifiers, NodeId, PointerButton, PointerData, Role, Ui, WheelData};
use je_widgets::button::{ContextMenuDelegate, IContextMenuProvider as IButtonContextMenuProvider};
use je_widgets::context_view_api::{IContextMenuDelegate, IContextMenuProvider};
use je_workbench::browser::parts::notifications::notifications_actions::ClearNotificationAction;
use je_workbench::browser::parts::notifications::notifications_center::{NotificationsCenter, NotificationsCenterServices};
use je_workbench::browser::parts::notifications::notifications_commands::{
    ACCEPT_PRIMARY_ACTION_NOTIFICATION, CLEAR_ALL_NOTIFICATIONS, CLEAR_NOTIFICATION, COLLAPSE_NOTIFICATION, EXPAND_NOTIFICATION, INotificationsCenterController, INotificationsToastController,
    NotificationActionRunner, notification_view_item_context, notification_view_item_from_context, register_notification_commands,
};
use je_workbench::browser::parts::notifications::notifications_toasts::{NotificationsToasts, NotificationsToastsServices};
use je_workbench::browser::parts::notifications::notifications_viewer::notification_row_height;
use je_workbench::common::notification_telemetry::{NotificationTelemetrySurface, log_notification_shown};
use je_workbench::common::notifications::{ChoiceAction, INotificationsModel, NotificationViewItem};
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::services::host::host::IHostService;
use je_workbench::services::notification::common::notification_service::NotificationService;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestHostService};
use je_workbench::test::common::test_notification_telemetry::TestNotificationTelemetryService;
use notification_doubles::{
    EmptyThemeService, NoMenuService, RecordingOpener, SilentAccessibilitySignals, TestLayoutService, renderer_services_with, test_editor_groups_service, test_lifecycle_service, warm_up_static_actions,
};
use test_instantiation_service::TestInstantiationService;

// region: dublês

/// Os `menus` do original: o delegado de cada `showContextMenu`, um vetor por contrato de menu.
#[derive(Default)]
struct RecordingMenus {
    dropdown: RefCell<Vec<Rc<dyn IContextMenuDelegate>>>,
    button: RefCell<Vec<ContextMenuDelegate>>,
}

impl IContextMenuProvider for RecordingMenus {
    fn show_context_menu(&self, delegate: Rc<dyn IContextMenuDelegate>) {
        self.dropdown.borrow_mut().push(delegate);
    }
}

impl IButtonContextMenuProvider for RecordingMenus {
    fn show_context_menu(&self, delegate: ContextMenuDelegate) {
        self.button.borrow_mut().push(delegate);
    }
}

/// O `stub(ICommandService, { executeCommand })`: executa o comando registrado com o `accessor` da instanciação.
struct ExecutingCommandService {
    instantiation: Weak<TestInstantiationService>,
}

impl ICommandService for ExecutingCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        let instantiation = self.instantiation.upgrade().expect("a instanciação está viva");
        execute_with(&instantiation, command_id, &args);
        Promise::resolved(Dynamic::Undefined)
    }
}

/// `execute`: `instantiation.invokeFunction(accessor => command.handler(accessor, ...args))`.
fn execute_with(instantiation: &TestInstantiationService, id: &str, args: &[Dynamic]) -> Dynamic {
    let command = commands_registry().get_command(id).unwrap_or_else(|| panic!("{id}"));
    let handler = command.handler;
    instantiation.invoke_function(|accessor| handler(accessor, args))
}

/// A lista com o foco do centro: o elemento em foco é a primeira notificação do modelo.
struct FocusedNotificationList {
    model: Rc<dyn INotificationsModel>,
}

impl IListWidget for FocusedNotificationList {
    fn is_active_element(&self) -> bool {
        true
    }
    fn get_focused_element(&self) -> Option<Rc<dyn Any>> {
        self.model.notifications().first().map(|item| item.clone() as Rc<dyn Any>)
    }
}

/// `listService` (`get lastFocusedList()`): a lista do centro, enquanto ele está visível.
struct FocusedNotificationListService {
    center: Weak<NotificationsCenter>,
    model: Rc<dyn INotificationsModel>,
}

impl IListService for FocusedNotificationListService {
    fn last_focused_list(&self) -> Option<Rc<dyn IListWidget>> {
        let center = self.center.upgrade()?;
        center.is_visible().then(|| Rc::new(FocusedNotificationList { model: self.model.clone() }) as Rc<dyn IListWidget>)
    }
}

// endregion

// region: montagem

/// O que `createUI` devolve, mais a [`Ui`] que desenha o contêiner.
struct TestUi {
    ui: Ui,
    container: NodeId,
    instantiation: Rc<TestInstantiationService>,
    notifications: Rc<NotificationService>,
    storage: Rc<AbstractStorageService>,
    telemetry: Rc<TestNotificationTelemetryService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    notification_service: Rc<dyn INotificationService>,
    command_service: Rc<dyn ICommandService>,
    center: Rc<NotificationsCenter>,
    toasts: Rc<NotificationsToasts>,
    menus: Rc<RecordingMenus>,
    opener: Rc<RecordingOpener>,
}

const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;

fn dimensions() -> Dimension {
    Dimension { width: WIDTH, height: HEIGHT }
}

/// `createUI(resources)`.
fn create_ui(resources: &DisposableStore) -> TestUi {
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, WIDTH, HEIGHT));
        let container = tree.create(root, Role::Generic);
        tree.set_bounds(container, Rect::new(0.0, 0.0, WIDTH, HEIGHT));
        tree.add_class(container, "monaco-workbench");
        container
    };
    {
        let ui = ui.clone();
        resources.add(to_disposable(move || ui.remove(container)));
    }

    let instantiation = resources.add_rc(TestInstantiationService::new());
    let storage = resources.add_rc(InMemoryStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage.clone());
    let notifications = resources.add_rc(NotificationService::new(storage_service));
    {
        let notifications = notifications.clone();
        resources.add(to_disposable(move || {
            for item in notifications.model.notifications() {
                item.close();
            }
        }));
    }
    let telemetry = Rc::new(TestNotificationTelemetryService::new());
    let menus = Rc::new(RecordingMenus::default());
    let opener = Rc::new(RecordingOpener::default());
    let notification_service: Rc<dyn INotificationService> = notifications.clone();
    let telemetry_service: Rc<dyn ITelemetryService> = telemetry.clone();
    let command_service: Rc<dyn ICommandService> = Rc::new(ExecutingCommandService { instantiation: Rc::downgrade(&instantiation) });
    let accessibility_signal_service: Rc<dyn IAccessibilitySignalService> = Rc::new(SilentAccessibilitySignals);

    let model: Rc<dyn INotificationsModel> = notifications.model.clone();
    let renderer = renderer_services_with(&ui, opener.clone(), notification_service.clone(), telemetry_service.clone(), menus.clone(), menus.clone(), command_service.clone());
    let editor_groups_service = test_editor_groups_service();
    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();

    let center = resources.add_rc(NotificationsCenter::new(
        container,
        model.clone(),
        NotificationsCenterServices {
            renderer: renderer.clone(),
            theme_service: Rc::new(EmptyThemeService),
            layout_service: Rc::new(TestLayoutService),
            context_key_service: context_key_service.clone(),
            editor_groups_service: editor_groups_service.clone(),
            accessibility_signal_service: accessibility_signal_service.clone(),
            menu_service: Rc::new(NoMenuService),
            command_service: command_service.clone(),
        },
    ));
    let lifecycle_service = test_lifecycle_service(resources);
    let host_service: Rc<dyn IHostService> = Rc::new(TestHostService);
    let environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
    let toasts = resources.add_rc(NotificationsToasts::new(
        container,
        model.clone(),
        NotificationsToastsServices {
            renderer,
            layout_service: Rc::new(TestLayoutService),
            theme_service: Rc::new(EmptyThemeService),
            editor_groups_service,
            context_key_service,
            lifecycle_service,
            host_service,
            environment_service,
        },
    ));
    {
        let (weak_center, toasts) = (Rc::downgrade(&center), toasts.clone());
        resources.add(center.on_did_change_visibility().listen(move |_| {
            if let Some(center) = weak_center.upgrade() {
                toasts.update(center.is_visible());
            }
        }));
    }
    {
        let center: Rc<dyn INotificationsCenterController> = center.clone();
        let toasts: Rc<dyn INotificationsToastController> = toasts.clone();
        resources.add(register_notification_commands(center, toasts, notifications.model.clone()));
    }

    let list_service: Rc<dyn IListService> = Rc::new(FocusedNotificationListService { center: Rc::downgrade(&center), model });
    instantiation.stub(I_NOTIFICATION_SERVICE, notification_service.clone());
    instantiation.stub(I_TELEMETRY_SERVICE, telemetry_service.clone());
    instantiation.stub(I_LIST_SERVICE, list_service);
    instantiation.stub(I_ACCESSIBILITY_SIGNAL_SERVICE, accessibility_signal_service);
    instantiation.stub(I_COMMAND_SERVICE, command_service.clone());

    toasts.layout(Some(dimensions()));
    center.layout(Some(dimensions()));
    run_microtasks();

    TestUi { ui, container, instantiation, notifications, storage, telemetry, telemetry_service, notification_service, command_service, center, toasts, menus, opener }
}

/// O `suiteSetup`: um erro até a renderização, sobre um `DisposableStore` descartável.
fn warm_up() {
    warm_up_static_actions();
    let warmup = DisposableStore::new();
    let test_ui = create_ui(&warmup);
    test_ui.notifications.notify(Notification::new(Severity::Error, "Warmup"));
    test_ui.render();
    warmup.dispose();
}

fn start() -> LeakGuard {
    warm_up();
    ensure_no_disposables_are_leaked_in_test_suite()
}

impl TestUi {
    /// `render()`: dois quadros de animação.
    fn render(&self) {
        for _ in 0..2 {
            self.ui.run_animation_frame();
            run_microtasks();
        }
    }

    /// `execute(id, ...args)`.
    fn execute(&self, id: &str, args: &[Dynamic]) {
        execute_with(&self.instantiation, id, args);
        run_microtasks();
    }

    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle> {
        self.notifications.notify(notification)
    }

    /// `notifications.model.notifications[0]`.
    fn first_item(&self) -> Rc<NotificationViewItem> {
        self.notifications.model.notifications()[0].clone()
    }

    fn item_count(&self) -> usize {
        self.notifications.model.notifications().len()
    }

    /// `container.querySelectorAll(...)` com as classes dadas, na ordem do documento.
    fn find_all(&self, root: NodeId, classes: &[&str]) -> Vec<NodeId> {
        let tree = self.ui.tree();
        tree.subtree(root).into_iter().filter(|node| classes.iter().all(|class| tree.has_class(*node, class))).collect()
    }

    /// `container.querySelector(...)`.
    fn query(&self, classes: &[&str]) -> NodeId {
        self.find_all(self.container, classes).first().copied().unwrap_or_else(|| panic!("nenhum nó com as classes {classes:?}"))
    }

    fn pointer(&self, node: NodeId, kind: EventKind, button: PointerButton) {
        let data = PointerData { x: 0.0, y: 0.0, button, modifiers: Modifiers::default(), click_count: 1 };
        self.ui.dispatch(&mut je_ui::Event::new(kind, node).with_pointer(data));
    }

    fn click(&self, node: NodeId) {
        self.pointer(node, EventKind::Click, PointerButton::Primary);
    }

    fn key_down(&self, node: NodeId, key_code: KeyCode) {
        self.ui.dispatch(&mut je_ui::Event::new(EventKind::KeyDown, node).with_key(KeyData { key_code, modifiers: Modifiers::default() }));
    }

    /// `ui.telemetry.shown.map(event => event.surface)` e companhia.
    fn shown_field(&self, key: &str) -> Vec<String> {
        self.telemetry.shown().iter().map(|event| field(event, key)).collect()
    }

    /// `ui.telemetry.interactions.map(event => [event.a, event.b, ...])`.
    fn interaction_rows(&self, keys: &[&str]) -> Vec<Vec<String>> {
        self.telemetry.interactions().iter().map(|event| keys.iter().map(|key| field(event, key)).collect()).collect()
    }

    /// `JSON.stringify([...ui.telemetry.shown, ...ui.telemetry.interactions])`.
    fn payload_json(&self, include_shown: bool) -> String {
        let shown = if include_shown { self.telemetry.shown() } else { Vec::new() };
        shown.iter().chain(self.telemetry.interactions().iter()).map(|event| json_stringify(event).expect("o evento serializa")).collect()
    }

    fn telemetry_events_are_empty(&self) -> bool {
        self.telemetry.events().is_empty()
    }
}

fn text_of(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava texto, veio {other:?}"),
    }
}

fn number_of(value: &Dynamic) -> f64 {
    match value {
        Dynamic::Number(number) => *number,
        other => panic!("esperava número, veio {other:?}"),
    }
}

/// `event.<key>` de um evento de telemetria, em texto.
fn field(event: &Dynamic, key: &str) -> String {
    match event.get_str(key) {
        Dynamic::String(text) => text.to_string_lossy(),
        other => format!("{}", number_of(&other)),
    }
}

/// O vetor de linhas de texto que o `deepStrictEqual` espera.
fn rows(expected: &[&[&str]]) -> Vec<Vec<String>> {
    expected.iter().map(|row| row.iter().map(|text| (*text).to_owned()).collect()).collect()
}

fn strings(expected: &[&str]) -> Vec<String> {
    expected.iter().map(|text| (*text).to_owned()).collect()
}

fn with_telemetry(mut notification: Notification, id: NotificationTelemetryId) -> Notification {
    notification.properties.telemetry = Some(NotificationTelemetry::Id(id));
    notification
}

/// `{ ..., priority: NotificationPriority.SILENT }`.
fn silent(severity: Severity, message: impl Into<NotificationMessage>) -> Notification {
    let mut notification = Notification::new(severity, message);
    notification.properties.priority = Some(NotificationPriority::Silent);
    notification
}

fn action_with(id: &str, label: &str, run: impl Fn() + 'static) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label: label.to_owned(),
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(move |_| {
            run();
            MaybePromise::Value(Dynamic::Undefined)
        }),
    })
}

fn choice(label: &str, telemetry_id: Option<NotificationActionTelemetryId>, keep_open: bool, run: Rc<dyn Fn()>) -> BasePromptChoice {
    BasePromptChoice { telemetry_id, label: label.to_owned(), keep_open, run }
}

fn plain_choice(base: BasePromptChoice) -> PromptChoiceKind {
    PromptChoiceKind::Plain(PromptChoice { base, is_secondary: false })
}

fn prompt_options(notification: Notification) -> Option<PromptOptions> {
    Some(PromptOptions { properties: notification.properties, on_cancel: None })
}

// endregion

#[test]
fn a_notification_removed_before_the_rendering_frame_has_no_exposure() {
    let guard = start();
    let ui = create_ui(&guard.store());
    let handle = ui.notify(with_telemetry(Notification::new(Severity::Info, "Short-lived"), NotificationTelemetryId::ExtensionInstall));
    let before_close = ui.telemetry_events_are_empty();
    handle.close();
    ui.render();
    assert_eq!((before_close, ui.telemetry_events_are_empty()), (true, true));
}

#[test]
fn a_notification_added_to_an_initially_empty_center_receives_one_exposure() {
    let guard = start();
    let ui = create_ui(&guard.store());
    ui.center.show();
    ui.render();
    let empty_exposures = ui.telemetry.shown().len();
    ui.notify(with_telemetry(Notification::new(Severity::Info, "Arrived while open"), NotificationTelemetryId::ExtensionsDisabled));
    ui.render();
    ui.center.layout(Some(dimensions()));
    ui.render();
    ui.execute(CLEAR_NOTIFICATION, &[notification_view_item_context(&ui.first_item())]);
    let interactions: Vec<(String, String, bool)> = ui
        .telemetry
        .interactions()
        .iter()
        .map(|event| (field(event, "interaction"), field(event, "surface"), number_of(&event.get_str("timeSinceShownMs")) >= 0.0))
        .collect();
    assert_eq!(
        (empty_exposures, ui.shown_field("surface"), interactions),
        (0, strings(&["center"]), vec![("dismiss".to_owned(), "center".to_owned(), true)])
    );
}

#[test]
fn dnd_silent_and_never_show_again_notifications_have_no_toast_exposure_center_exposure_is_separate() {
    let guard = start();
    let ui = create_ui(&guard.store());
    ui.storage.store("neverAgain", StorageValue::Bool(true), StorageScope::Application, StorageTarget::User);
    let mut never_shown = Notification::new(Severity::Info, "Never shown");
    never_shown.properties.never_show_again = Some(NeverShowAgainOptions { id: "neverAgain".to_owned(), is_secondary: false, scope: None });
    ui.notify(never_shown);
    ui.notification_service.set_filter(NotificationFilterSetting::Global(NotificationsFilter::Error));
    ui.notify(with_telemetry(Notification::new(Severity::Info, "DND"), NotificationTelemetryId::ExtensionsDisabled));
    ui.notify(silent(Severity::Error, "Silent"));
    ui.render();
    let before_center = ui.telemetry.events().len();
    ui.center.show();
    ui.render();
    assert_eq!((before_center, ui.shown_field("surface"), ui.telemetry.interactions().len()), (0, strings(&["center", "center"]), 0));
}

#[test]
fn toast_measurement_repeated_layout_updates_dedup_and_center_re_show_do_not_duplicate_exposures() {
    let guard = start();
    let ui = create_ui(&guard.store());
    ui.toasts.layout(Some(Dimension { width: WIDTH, height: 1.0 }));
    for index in 0..3 {
        let mut notification = with_telemetry(Notification::new(Severity::Error, format!("Notice {index}")), NotificationTelemetryId::ExtensionInstall);
        notification.id = Some(format!("dedup{index}"));
        notification.properties.sticky = true;
        ui.notify(notification);
    }
    ui.render();
    let measured = ui.telemetry.shown().len();
    ui.toasts.layout(Some(dimensions()));
    ui.render();
    let first_instances = ui.shown_field("instanceId").len();
    ui.first_item().update_message(NotificationMessage::from("Updated content"));
    ui.toasts.layout(Some(dimensions()));
    ui.render();
    let mut duplicate = with_telemetry(Notification::new(Severity::Error, "Notice 2"), NotificationTelemetryId::ExtensionInstall);
    duplicate.id = Some("dedup2".to_owned());
    duplicate.properties.sticky = true;
    ui.notify(duplicate);
    ui.render();
    ui.center.show();
    ui.render();
    ui.center.hide();
    ui.center.show();
    ui.render();
    let instances: HashSet<String> = ui.shown_field("instanceId").into_iter().collect();
    assert_eq!(
        (measured, ui.shown_field("surface"), instances.len(), first_instances, ui.telemetry.interactions().len()),
        (0, strings(&["toast", "toast", "toast", "center", "center", "center"]), 3, 3, 0)
    );
}

#[test]
fn center_exposures_count_intersecting_rows_not_all_model_entries_or_overscan_and_follow_scrolling() {
    let guard = start();
    let ui = create_ui(&guard.store());
    for index in 0..25 {
        ui.notify(silent(Severity::Info, format!("Notice {index}")));
    }
    ui.center.show();
    ui.render();
    let list = ui.query(&["monaco-list"]);
    let bounds = ui.ui.tree().absolute_bounds(list);
    let visible_rows = ui
        .find_all(ui.container, &["monaco-list-row"])
        .into_iter()
        .filter(|row| {
            let row_bounds = ui.ui.tree().absolute_bounds(*row);
            row_bounds.y1 > bounds.y0.max(0.0) && row_bounds.y0 < bounds.y1.min(HEIGHT)
        })
        .count();
    let initial_count = ui.telemetry.shown().len();
    // `list.scrollTop = top`: a lista rola pela roda, em passos de `renderHeight - 10`, até o `scrollHeight`
    let scroll_height = 25.0 * notification_row_height();
    let render_height = bounds.height();
    let mut top = 0.0;
    while top < scroll_height {
        if top > 0.0 {
            // `StandardWheelEvent` converte pixels em `-delta / 40` e o scrollable multiplica por 50: 1 px de
            // roda rola 1,25 px; o delta é escalado de volta para a lista andar exatamente `renderHeight - 10`.
            let wheel_delta = (render_height - 10.0) * 40.0 / 50.0;
            ui.ui.wheel_input(WheelData::new((bounds.x0 + bounds.x1) / 2.0, (bounds.y0 + bounds.y1) / 2.0, 0.0, wheel_delta, DeltaMode::Pixel));
        }
        ui.render();
        top += render_height - 10.0;
    }
    let unique: HashSet<String> = ui.shown_field("instanceId").into_iter().collect();
    assert_eq!(
        (initial_count == visible_rows, initial_count > 0 && initial_count < 25, ui.telemetry.shown().len(), unique.len()),
        (true, true, 25, 25)
    );
}

#[test]
fn mouse_and_keyboard_primary_buttons_each_produce_one_invocation_alongside_unchanged_legacy_telemetry() {
    let guard = start();
    let ui = create_ui(&guard.store());
    let invoked = Rc::new(Cell::new(0));
    let counter = invoked.clone();
    let mut options = with_telemetry(Notification::new(Severity::Info, "Prompt"), NotificationTelemetryId::AuthenticationContinue);
    options.properties.priority = Some(NotificationPriority::Silent);
    ui.notification_service.prompt(
        Severity::Info,
        NotificationMessage::from("Prompt"),
        vec![plain_choice(choice("Continue", Some(NotificationActionTelemetryId::Continue), true, Rc::new(move || counter.set(counter.get() + 1))))],
        prompt_options(options),
    );
    ui.center.show();
    ui.render();
    let buttons_container = ui.query(&["notification-list-item-buttons-container"]);
    let button = ui.find_all(buttons_container, &["monaco-button"]).first().copied().expect("o botão primário existe");
    log_notification_shown(&*ui.telemetry_service, &ui.first_item(), NotificationTelemetrySurface::AccessibleView);
    ui.click(button);
    ui.key_down(button, KeyCode::Enter);
    run_microtasks();
    let legacy: Vec<(String, String)> = ui
        .telemetry
        .events()
        .iter()
        .filter(|(name, _)| name == "workbenchActionExecuted")
        .map(|(_, data)| (text_of(&data.get_str("id")), text_of(&data.get_str("from"))))
        .collect();
    assert_eq!(
        (invoked.get(), ui.interaction_rows(&["interaction", "actionId", "surface"]), legacy),
        (
            2,
            rows(&[&["primaryAction", "continue", "center"], &["primaryAction", "continue", "center"]]),
            vec![("workbench.dialog.choice.0".to_owned(), "message".to_owned()), ("workbench.dialog.choice.0".to_owned(), "message".to_owned())]
        )
    );
}

#[test]
fn secondary_menu_actions_retain_the_notification_context_without_logging_the_label_or_arbitrary_action_id() {
    let guard = start();
    let ui = create_ui(&guard.store());
    let invoked = Rc::new(Cell::new(0));
    let counter = invoked.clone();
    let secondary = with_notification_action_telemetry(
        action_with("private command", "Private label", move || counter.set(counter.get() + 1)),
        Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::Reload)),
    );
    let mut notification = silent(Severity::Info, "Notice");
    notification.actions = Some(NotificationActions { primary: Vec::new(), secondary: vec![secondary.clone()] });
    ui.notify(notification);
    let item = ui.first_item();
    ui.center.show();
    ui.render();
    let gear = ui.query(&["codicon-notifications-configure"]);
    ui.pointer(gear, EventKind::PointerDown, PointerButton::Primary);
    let menu = ui.menus.dropdown.borrow().last().cloned().expect("o menu da engrenagem abriu");
    let includes_secondary = menu.get_actions().iter().any(|action| Rc::ptr_eq(action, &secondary));
    let context = menu.get_actions_context(None);
    let retains_item = context.as_ref().and_then(notification_view_item_from_context).is_some_and(|context_item| Rc::ptr_eq(&context_item, &item));
    let runner = menu.action_runner().expect("o menu tem o runner da notificação");
    let _ = runner.run(&secondary, context);
    run_microtasks();
    assert!(includes_secondary);
    assert!(retains_item);
    assert_eq!(
        (invoked.get(), ui.interaction_rows(&["interaction", "actionRole", "actionId"]), ui.payload_json(false).contains("private")),
        (1, rows(&[&["secondaryAction", "secondary", "reload"]]), false)
    );
}

#[test]
fn primary_dropdown_choices_and_the_wrapped_primary_action_each_produce_only_one_contextual_invocation() {
    let guard = start();
    let ui = create_ui(&guard.store());
    let invoked: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (primary_log, menu_log) = (invoked.clone(), invoked.clone());
    let choices = vec![PromptChoiceKind::WithMenu(PromptChoiceWithMenu {
        base: choice("Continue", Some(NotificationActionTelemetryId::Continue), false, Rc::new(move || primary_log.borrow_mut().push("primary"))),
        menu: vec![choice("Decline", Some(NotificationActionTelemetryId::Decline), true, Rc::new(move || menu_log.borrow_mut().push("menu")))],
    })];
    ui.notification_service.prompt(Severity::Info, NotificationMessage::from("Prompt"), choices, prompt_options(silent(Severity::Info, "Prompt")));
    let action = ui.first_item().actions().primary[0].clone();
    let choice_action = action.as_any().downcast_ref::<ChoiceAction>().expect("a ação primária é um ChoiceAction");
    for menu_choice in choice_action.menu().expect("o menu") {
        guard.add_rc(menu_choice.clone());
    }
    ui.center.show();
    ui.render();
    ui.click(ui.query(&["monaco-dropdown-button"]));
    let menu = ui.menus.button.borrow().last().cloned().expect("o menu do botão abriu");
    let actions = (menu.get_actions)();
    let runner = menu.action_runner.clone().expect("o menu tem o runner da notificação");
    let _ = runner.run(&actions[1], None);
    run_microtasks();
    let _ = runner.run(&actions[0], None);
    run_microtasks();
    assert_eq!(
        (invoked.borrow().clone(), ui.interaction_rows(&["interaction", "actionId", "surface"])),
        (vec!["menu", "primary"], rows(&[&["primaryAction", "decline", "center"], &["primaryAction", "continue", "center"]]))
    );
}

#[test]
fn keyboard_links_and_context_menu_copy_record_bounded_interactions_not_content_or_urls() {
    let guard = start();
    let ui = create_ui(&guard.store());
    ui.notify(silent(Severity::Info, NotificationMessage::from(NotificationText::link("private link", "https://private.example/path", None))));
    ui.center.show();
    ui.render();
    log_notification_shown(&*ui.telemetry_service, &ui.first_item(), NotificationTelemetrySurface::AccessibleView);
    let message = ui.query(&["notification-list-item-message"]);
    let anchor = {
        let tree = ui.ui.tree();
        tree.subtree(message).into_iter().skip(1).find(|node| tree.role(*node) == Role::Link).expect("o link da mensagem existe")
    };
    ui.key_down(anchor, KeyCode::Enter);
    ui.pointer(ui.query(&["notification-list-item"]), EventKind::ContextMenu, PointerButton::Secondary);
    let menu = ui.menus.dropdown.borrow().last().cloned().expect("o menu de contexto abriu");
    let runner = menu.action_runner().expect("o menu tem o runner da notificação");
    let _ = runner.run(&menu.get_actions()[0], menu.get_actions_context(None));
    run_microtasks();
    assert_eq!(
        (ui.opener.opened.borrow().len(), ui.interaction_rows(&["interaction", "actionId", "surface"]), ui.payload_json(true).contains("private")),
        (1, rows(&[&["link", "unknown", "center"], &["copy", "copy", "center"]]), false)
    );
}

#[test]
fn keyboard_double_click_and_toolbar_command_paths_distinguish_user_expansion_and_dismissal() {
    let guard = start();
    let ui = create_ui(&guard.store());
    let mut notification = silent(Severity::Info, "Notice");
    notification.source = Some(NotificationSourceOrText::Text("Source".to_owned()));
    ui.notify(notification);
    let item = ui.first_item();
    ui.center.show();
    ui.render();
    ui.execute(EXPAND_NOTIFICATION, &[notification_view_item_context(&item)]);
    ui.execute(EXPAND_NOTIFICATION, &[notification_view_item_context(&item)]);
    ui.execute(COLLAPSE_NOTIFICATION, &[notification_view_item_context(&item)]);
    ui.pointer(ui.query(&["notification-list-item"]), EventKind::DoubleClick, PointerButton::Primary);
    log_notification_shown(&*ui.telemetry_service, &item, NotificationTelemetrySurface::AccessibleView);
    ui.execute("notification.toggle", &[]);
    let runner = guard.add_rc(NotificationActionRunner::new(Some(NotificationTelemetrySurface::Center), ui.telemetry_service.clone(), ui.notification_service.clone()));
    let clear = guard.add_rc(ClearNotificationAction::new(ClearNotificationAction::ID, &ClearNotificationAction::label(), ui.command_service.clone()));
    let clear_action: ActionRef = clear;
    let _ = runner.run(&clear_action, Some(notification_view_item_context(&item)));
    run_microtasks();
    assert_eq!(
        (ui.interaction_rows(&["interaction", "surface"]), ui.item_count()),
        (rows(&[&["expand", "center"], &["collapse", "center"], &["expand", "center"], &["collapse", "center"], &["dismiss", "center"]]), 0)
    );
}

#[test]
fn keyboard_acceptance_and_middle_click_clear_are_attributed_but_automatic_removal_and_clear_of_active_progress_are_not() {
    let guard = start();
    let ui = create_ui(&guard.store());
    let invoked = Rc::new(Cell::new(0));
    let counter = invoked.clone();
    ui.notification_service.prompt(
        Severity::Info,
        NotificationMessage::from("Prompt"),
        vec![plain_choice(choice("Continue", None, false, Rc::new(move || counter.set(counter.get() + 1))))],
        prompt_options(silent(Severity::Info, "Prompt")),
    );
    ui.center.show();
    ui.render();
    ui.execute(ACCEPT_PRIMARY_ACTION_NOTIFICATION, &[]);
    let automatic = ui.notify(silent(Severity::Info, "Automatic"));
    automatic.close();
    ui.notify(silent(Severity::Info, "Middle click"));
    ui.center.show();
    ui.render();
    ui.pointer(ui.query(&["notification-list-item"]), EventKind::PointerUp, PointerButton::Auxiliary);
    let mut running = silent(Severity::Info, "Running");
    running.progress = Some(NotificationProgressProperties { infinite: true, ..NotificationProgressProperties::default() });
    ui.notify(running);
    ui.execute(CLEAR_NOTIFICATION, &[notification_view_item_context(&ui.first_item())]);
    assert_eq!(
        (invoked.get(), ui.interaction_rows(&["interaction"]), ui.item_count()),
        (1, rows(&[&["primaryAction"], &["dismiss"]]), 1)
    );
}

#[test]
fn clear_all_records_one_clear_per_removable_item_including_unseen_items_and_never_cancels_progress() {
    let guard = start();
    let ui = create_ui(&guard.store());
    ui.notify(silent(Severity::Info, "Visible"));
    ui.center.show();
    ui.render();
    ui.center.hide();
    ui.notify(silent(Severity::Info, "Unseen"));
    let progress = |message: &str| {
        let mut notification = silent(Severity::Info, message);
        notification.progress = Some(NotificationProgressProperties { infinite: true, ..NotificationProgressProperties::default() });
        notification
    };
    let completed = ui.notify(progress("Completed"));
    completed.progress().done();
    ui.notify(progress("Active"));
    ui.execute(CLEAR_ALL_NOTIFICATIONS, &[]);
    let unseen_count = ui.telemetry.interactions().iter().filter(|event| number_of(&event.get_str("timeSinceShownMs")) == -1.0).count();
    assert_eq!(
        (ui.interaction_rows(&["interaction", "surface"]), unseen_count, ui.item_count()),
        (rows(&[&["clearAll", "unknown"], &["clearAll", "unknown"], &["clearAll", "center"]]), 2, 1)
    );
}

#[test]
fn actions_report_invocation_even_when_they_throw_and_do_not_collect_the_error() {
    let guard = start();
    let ui = create_ui(&guard.store());
    let action = to_action(ToActionProps {
        id: "private command".to_owned(),
        label: "Private button".to_owned(),
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(|_| -> MaybePromise<Dynamic> { errors::throw(Error::new("private failure")) }),
    });
    let mut notification = silent(Severity::Info, "Notice");
    notification.actions = Some(NotificationActions { primary: vec![action.clone()], secondary: Vec::new() });
    ui.notify(notification);
    let item = ui.first_item();
    let runner = guard.add_rc(NotificationActionRunner::new(None, ui.telemetry_service.clone(), ui.notification_service.clone()));
    let _ = runner.run(&action, Some(notification_view_item_context(&item)));
    run_microtasks();
    let interactions = ui.telemetry.interactions();
    assert_eq!(
        (interactions.len(), field(&interactions[0], "actionId"), ui.first_item().severity() == Severity::Error, ui.payload_json(false).contains("private")),
        (1, "unknown".to_owned(), true, false)
    );
}
