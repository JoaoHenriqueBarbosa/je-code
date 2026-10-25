//! Porte de `src/vs/workbench/test/browser/notificationsList.test.ts` (suítes `NotificationsList row height`,
//! `NotificationRenderer`, `NotificationsList AccessibilityProvider` e `NotificationsCenter`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (declarado antes de
//!   tudo, para ser o último a cair); `store.add` vira `guard.add` / `guard.add_rc`. O `teardown` da primeira suíte
//!   (`setNotificationRowHeight(DEFAULT_NOTIFICATION_ROW_HEIGHT)`) é o `Drop` de [`RowHeightReset`].
//! - **`suiteSetup` de aquecimento.** No original ele também cria as ações `static` do `NotificationTemplateRenderer` antes
//!   do rastreador de vazamentos ligar; [`warm_up_static_actions`] faz o mesmo no começo de cada caso que monta um renderer
//!   (o rastreador e as `static` são por fio, e cada caso roda no seu).
//! - **`workbenchInstantiationService(undefined, store)` e `instantiationService.stub(...)`.** São os serviços montados à mão
//!   em [`renderer_services_in`] e [`center_services`], com os dublês de `support/notification_doubles.rs` (`NoKeybindingService`,
//!   `RecordingOpener`, `NoCommandService`, `NoClipboard`, `EmptyThemeService`, `NoMenuService`, `SilentAccessibilitySignals`,
//!   `TestLayoutService`). O `NotificationService` é o real, sobre o `InMemoryStorageService`.
//! - **DOM vira árvore do `je-ui`.** `document.createElement('div')` é um nó solto (ou filho da raiz, quando o original o
//!   anexa ao `body`); `querySelector('.monaco-list-row.focused')` é a busca por classes na subárvore; `style.height` e
//!   `style.top` são os limites do nó (`row.bounds()` e o `top` do `.monaco-list-rows`); `row.click()`, `dispatchEvent(new
//!   KeyboardEvent('keydown', ...))` e `dispatchEvent(new MouseEvent(...))` são `Ui::dispatch` de um evento do mesmo tipo.
//! - **`aria-disabled` do botão.** O `je-ui` só tem `role` e `label`; o `Button` marca `aria-disabled` e a classe `disabled`
//!   no mesmo passo (`set_enabled`), e o porte lê a classe: `'true'` é `disabled` presente.
//! - **`a.getAttribute('href')`.** O nó do link não guarda o `href`; o que o `href` provaria (o link existir e abrir o destino
//!   certo) é conferido pelo número de links (`Role::Link`) e pelo que o `IOpenerService` recebeu, que é o que o caso confere
//!   em `opened`. O `textContent` é o texto de todos os nós de texto da subárvore, em ordem.
//! - **`(templateData.toDispose as unknown as { _toDispose: Set<object> })._toDispose.size`.** É [`DisposableStore::len`].
//! - **`closeActionVisible` e `clearAllDisabled`.** Existência do item com a classe `codicon-notifications-clear` dentro do
//!   `.notification-list-item-toolbar-container` e a classe `disabled` no item `codicon-notifications-clear-all`.
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de tuplas, com os mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;
mod support;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::actions::{Action, ActionRef, ActionRunner, IAction};
use je_base::dynamic::Dynamic;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::promise::MaybePromise;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::notification::notification_legacy::LEGACY_EXTENSION_LINK_PARSING;
use je_platform::notification::notification_message::{NotificationPart, NotificationText};
use je_platform::notification::{Notification, NotificationActions, NotificationHandle, NotificationMessage, Severity};
use je_render::kurbo::Rect;
use je_ui::{Content, EventKind, KeyData, Modifiers, NodeId, PointerButton, PointerData, Role, Ui};
use je_widgets::list::list_types::{IListRenderer, TemplateData};
use je_workbench::browser::parts::notifications::notifications_center::{NotificationsCenter, NotificationsCenterServices};
use je_workbench::browser::parts::notifications::notifications_list::NotificationAccessibilityProvider;
use je_workbench::browser::parts::notifications::notifications_viewer::{
    DEFAULT_NOTIFICATION_ROW_HEIGHT, NotificationRenderer, NotificationTemplateData, on_did_change_notification_row_height, set_notification_row_height,
};
use je_workbench::common::notifications::{INotificationsModel, NotificationViewItem, NotificationsFilterState, NotificationsModel};
use notification_doubles::{
    EmptyThemeService, NoCommandService, NoKeybindingService, NoMenuService, RecordingOpener, SilentAccessibilitySignals, TestLayoutService, renderer_services_in, test_editor_groups_service,
    warm_up_static_actions,
};

// region: montagem

fn center_services(guard: &LeakGuard, ui: &Ui) -> NotificationsCenterServices {
    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
    NotificationsCenterServices {
        renderer: renderer_services_in(&guard.store(), ui, Rc::new(RecordingOpener::default())),
        theme_service: Rc::new(EmptyThemeService),
        layout_service: Rc::new(TestLayoutService),
        context_key_service,
        editor_groups_service: test_editor_groups_service(),
        accessibility_signal_service: Rc::new(SilentAccessibilitySignals),
        menu_service: Rc::new(NoMenuService),
        command_service: Rc::new(NoCommandService),
    }
}

/// `NotificationViewItem.create(notification, { global: NotificationsFilter.OFF, sources: new Map() })!`.
fn create_item(notification: Notification) -> Rc<NotificationViewItem> {
    NotificationViewItem::create(&notification, &NotificationsFilterState::default()).expect("a notificação não é filtrada")
}

/// `store.add(toDisposable(() => notification.close()))`.
fn close_on_dispose(guard: &LeakGuard, notification: &Rc<NotificationViewItem>) {
    let notification = notification.clone();
    guard.add(to_disposable(move || notification.close()));
}

/// Um nó filho da raiz com os limites dados (`document.body.appendChild(container)`).
fn attached_container(ui: &Ui, class: Option<&str>, width: f64, height: f64) -> NodeId {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    tree.set_bounds(root, Rect::new(0.0, 0.0, 800.0, 600.0));
    let container = tree.create(root, Role::Generic);
    tree.set_bounds(container, Rect::new(0.0, 0.0, width, height));
    if let Some(class) = class {
        tree.add_class(container, class);
    }
    container
}

/// Um nó solto (`document.createElement('div')` sem anexar ao `body`).
fn detached_container(ui: &Ui, width: f64, height: f64) -> NodeId {
    let mut tree = ui.tree_mut();
    let container = tree.create_detached(Role::Generic);
    tree.set_bounds(container, Rect::new(0.0, 0.0, width, height));
    container
}

/// `store.add(toDisposable(() => container.remove()))`.
fn remove_on_dispose(guard: &LeakGuard, ui: &Ui, container: NodeId) {
    let ui = ui.clone();
    guard.add(to_disposable(move || ui.remove(container)));
}

/// Os nós da subárvore de `root` com todas as classes dadas.
fn find_all(ui: &Ui, root: NodeId, classes: &[&str]) -> Vec<NodeId> {
    let tree = ui.tree();
    tree.subtree(root).into_iter().filter(|node| classes.iter().all(|class| tree.has_class(*node, class))).collect()
}

fn template_of(template: &TemplateData) -> Rc<NotificationTemplateData> {
    let any: Rc<dyn Any> = template.clone();
    any.downcast::<NotificationTemplateData>().unwrap_or_else(|_| panic!("template de notificação inesperado"))
}

fn pointer() -> PointerData {
    PointerData { x: 0.0, y: 0.0, button: PointerButton::Primary, modifiers: Modifiers::default(), click_count: 1 }
}

/// `node.dispatchEvent(new MouseEvent(type, ...))` e `node.click()`.
fn dispatch_pointer(ui: &Ui, node: NodeId, kind: EventKind) {
    ui.dispatch(&mut je_ui::Event::new(kind, node).with_pointer(pointer()));
}

/// `node.dispatchEvent(new KeyboardEvent('keydown', { key, keyCode, bubbles: true }))`.
fn dispatch_key_down(ui: &Ui, node: NodeId, key_code: KeyCode) {
    ui.dispatch(&mut je_ui::Event::new(EventKind::KeyDown, node).with_key(KeyData { key_code, modifiers: Modifiers::default() }));
}

/// O `teardown(() => setNotificationRowHeight(DEFAULT_NOTIFICATION_ROW_HEIGHT))`.
struct RowHeightReset;

impl Drop for RowHeightReset {
    fn drop(&mut self) {
        set_notification_row_height(DEFAULT_NOTIFICATION_ROW_HEIGHT);
    }
}

// endregion

// region: NotificationsList row height

#[test]
fn deduplicates_row_height_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let _reset = RowHeightReset;
    let changes: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));
    let recorded = changes.clone();
    guard.add(on_did_change_notification_row_height().listen(move |height| recorded.borrow_mut().push(*height)));

    set_notification_row_height(34.0);
    set_notification_row_height(34.0);
    set_notification_row_height(DEFAULT_NOTIFICATION_ROW_HEIGHT);

    assert_eq!(*changes.borrow(), vec![34.0, DEFAULT_NOTIFICATION_ROW_HEIGHT]);
}

#[test]
fn notification_center_updates_row_heights_and_preserves_the_focused_row_viewport_position() {
    warm_up_static_actions();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let _reset = RowHeightReset;
    set_notification_row_height(34.0);
    let ui = Ui::new();
    let container = attached_container(&ui, Some("monaco-workbench"), 800.0, 600.0);
    remove_on_dispose(&guard, &ui, container);

    let model = guard.add_rc(NotificationsModel::new());
    {
        let model = model.clone();
        guard.add(to_disposable(move || {
            for notification in model.notifications() {
                notification.close();
            }
        }));
    }
    let center = guard.add_rc(NotificationsCenter::new(container, model.clone(), center_services(&guard, &ui)));

    for index in 0..10 {
        model.add_notification(Notification::new(Severity::Info, format!("Message {index}")));
    }
    center.show();

    // `{ height: row?.style.height, viewportTop: parseInt(row.style.top) + parseInt(rows.style.top || '0') }`
    let get_row_state = || {
        let row = find_all(&ui, container, &["monaco-list-row", "focused"]).first().copied();
        let rows = find_all(&ui, container, &["monaco-list-rows"]).first().copied();
        let tree = ui.tree();
        let height = row.map(|row| tree.bounds(row).height());
        let viewport_top = match (row, rows) {
            (Some(row), Some(rows)) => Some(tree.bounds(row).y0 + tree.bounds(rows).y0),
            _ => None,
        };
        (height, viewport_top)
    };

    // `.monaco-list-row[data-index="7"]`: a linha de índice 7 é a que começa em 7 * 34
    let row_to_focus = {
        let tree = ui.tree();
        find_all(&ui, container, &["monaco-list-row"]).into_iter().find(|row| tree.bounds(*row).y0 == 7.0 * 34.0).expect("a linha de índice 7 está renderizada")
    };
    dispatch_pointer(&ui, row_to_focus, EventKind::PointerDown);
    dispatch_pointer(&ui, row_to_focus, EventKind::Click);
    let before = get_row_state();
    set_notification_row_height(DEFAULT_NOTIFICATION_ROW_HEIGHT);
    let after = get_row_state();
    center.dispose();
    set_notification_row_height(36.0);

    let preserved = match (before.1, after.1) {
        (Some(before_top), Some(after_top)) => (after_top - before_top).abs() <= DEFAULT_NOTIFICATION_ROW_HEIGHT - 34.0,
        _ => false,
    };
    assert_eq!(
        (before.0, after.0, preserved, find_all(&ui, container, &["monaco-list-row"]).len()),
        (Some(34.0), Some(DEFAULT_NOTIFICATION_ROW_HEIGHT), true, 0)
    );
}

// endregion

// region: NotificationRenderer

#[test]
fn primary_buttons_expose_and_track_disabled_actions_without_invoking_or_closing_the_notification() {
    warm_up_static_actions();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = detached_container(&ui, 450.0, 42.0);
    let services = renderer_services_in(&guard.store(), &ui, Rc::new(RecordingOpener::default()));
    let renderer = NotificationRenderer::new({ let runner = guard.add_rc(ActionRunner::new()); runner }, None, services);
    let template = renderer.render_template(container);
    {
        let (renderer, template) = (renderer.clone(), template.clone());
        guard.add(to_disposable(move || renderer.dispose_template(&template)));
    }
    let invoked = Rc::new(Cell::new(0));
    let closed = Rc::new(Cell::new(false));
    let action = {
        let invoked = invoked.clone();
        guard.add_rc(Action::new(
            "cancel",
            "Cancel",
            "",
            false,
            Some(Rc::new(move |_| {
                invoked.set(invoked.get() + 1);
                MaybePromise::from(Dynamic::Undefined)
            })),
        ))
    };
    let notification = create_item(Notification {
        actions: Some(NotificationActions { primary: vec![action.clone() as ActionRef], secondary: Vec::new() }),
        ..Notification::new(Severity::Info, "Exporting")
    });
    close_on_dispose(&guard, &notification);
    {
        let closed = closed.clone();
        guard.add(notification.on_did_close().listen(move |_| closed.set(true)));
    }
    notification.expand();
    renderer.render_element(&notification, 0, &template, None);
    let button = find_all(&ui, template_of(&template).buttons_container, &["monaco-button"]).first().copied().expect("o botão primário existe");
    let is_disabled = || ui.tree().has_class(button, "disabled");
    let initial_disabled = is_disabled();
    action.set_enabled(true);
    let enabled = is_disabled();
    action.set_enabled(false);
    dispatch_pointer(&ui, button, EventKind::Click);
    dispatch_key_down(&ui, button, KeyCode::Enter);
    assert_eq!((initial_disabled, enabled, is_disabled(), invoked.get(), closed.get()), (true, false, true, 0, false));
}

/// O corpo dos três casos `renders ${name} text and activates only explicit links with mouse and keyboard`.
fn renders_text_and_activates_only_explicit_links(message: NotificationMessage, legacy: bool, expected_text: &str, expected_links: &[&str]) {
    warm_up_static_actions();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    // larga o bastante para a mensagem não ser cortada com reticências (o `textContent` do DOM não é cortado)
    let container = detached_container(&ui, 2000.0, 42.0);
    let opener = Rc::new(RecordingOpener::default());
    let services = renderer_services_in(&guard.store(), &ui, opener.clone());
    let renderer = NotificationRenderer::new({ let runner = guard.add_rc(ActionRunner::new()); runner }, None, services);
    let template = renderer.render_template(container);
    {
        let (renderer, template) = (renderer.clone(), template.clone());
        guard.add(to_disposable(move || renderer.dispose_template(&template)));
    }
    let mut notification = Notification::new(Severity::Info, message);
    if legacy {
        notification.properties.legacy_extension_link_parsing = Some(LEGACY_EXTENSION_LINK_PARSING);
    }
    let notification = create_item(notification);
    close_on_dispose(&guard, &notification);
    renderer.render_element(&notification, 0, &template, None);

    let message_node = template_of(&template).message;
    let (anchors, text) = {
        let tree = ui.tree();
        let nodes: Vec<NodeId> = tree.subtree(message_node).into_iter().skip(1).collect();
        let anchors: Vec<NodeId> = nodes.iter().copied().filter(|node| tree.role(*node) == Role::Link).collect();
        let text: String = nodes
            .iter()
            .filter_map(|node| match &tree.style(*node).content {
                Content::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        (anchors, text)
    };
    for anchor in &anchors {
        dispatch_pointer(&ui, *anchor, EventKind::Click);
        dispatch_key_down(&ui, *anchor, KeyCode::Enter);
        dispatch_key_down(&ui, *anchor, KeyCode::Space);
    }

    let expected_opened: Vec<String> = expected_links.iter().flat_map(|href| [href.to_string(), href.to_string(), href.to_string()]).collect();
    assert_eq!((text, anchors.len(), opener.opened.borrow().clone()), (expected_text.to_owned(), expected_links.len(), expected_opened));
}

const README: &str = "README.md [Open](command:unexpected \"Open README\")";

#[test]
fn renders_literal_text_and_activates_only_explicit_links_with_mouse_and_keyboard() {
    renders_text_and_activates_only_explicit_links(NotificationMessage::Text(README.to_owned()), false, README, &[]);
}

#[test]
fn renders_structured_text_and_activates_only_explicit_links_with_mouse_and_keyboard() {
    let message = NotificationText::concat([NotificationPart::from(README), NotificationPart::from(" "), NotificationPart::from(NotificationText::link("Show Logs", "command:showLogs", None))]);
    renders_text_and_activates_only_explicit_links(NotificationMessage::Rich(message), false, &format!("{README} Show Logs"), &["command:showLogs"]);
}

#[test]
fn renders_legacy_extension_text_and_activates_only_explicit_links_with_mouse_and_keyboard() {
    renders_text_and_activates_only_explicit_links(NotificationMessage::Text(README.to_owned()), true, "README.md Open", &["command:unexpected"]);
}

#[test]
fn releases_replaced_notification_action_view_items() {
    warm_up_static_actions();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = attached_container(&ui, None, 450.0, 42.0);
    remove_on_dispose(&guard, &ui, container);

    let services = renderer_services_in(&guard.store(), &ui, Rc::new(RecordingOpener::default()));
    let renderer = NotificationRenderer::new({ let runner = guard.add_rc(ActionRunner::new()); runner }, None, services);
    let template = renderer.render_template(container);
    {
        let (renderer, template) = (renderer.clone(), template.clone());
        guard.add(to_disposable(move || renderer.dispose_template(&template)));
    }
    let template_disposables = template_of(&template).to_dispose.clone();

    let first_action = guard.add_rc(Action::new("first.action", "First Action", "", true, None));
    let first_notification = create_item(Notification {
        actions: Some(NotificationActions { primary: Vec::new(), secondary: vec![first_action as ActionRef] }),
        ..Notification::new(Severity::Info, "First notification")
    });
    close_on_dispose(&guard, &first_notification);

    let replacement_action = guard.add_rc(Action::new("replacement.action", "Replacement Action", "", true, None));
    let replacement_notification = create_item(Notification {
        actions: Some(NotificationActions { primary: Vec::new(), secondary: vec![replacement_action as ActionRef] }),
        ..Notification::new(Severity::Info, "Replacement notification")
    });
    close_on_dispose(&guard, &replacement_notification);

    renderer.render_element(&first_notification, 0, &template, None);
    let disposable_count = template_disposables.len();
    renderer.render_element(&replacement_notification, 0, &template, None);

    assert_eq!(template_disposables.len(), disposable_count);
}

// endregion

// region: NotificationsList AccessibilityProvider

/// O `setup`: o provedor sobre o `TestConfigurationService` e o `MockKeybindingService`.
fn accessibility_provider() -> NotificationAccessibilityProvider {
    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    NotificationAccessibilityProvider::new(None, Rc::new(NoKeybindingService), configuration_service)
}

#[test]
fn get_aria_label_includes_severity_prefix_for_error_notifications() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let provider = accessibility_provider();
    let notification = create_item(Notification::new(Severity::Error, "Something went wrong"));
    close_on_dispose(&guard, &notification);
    let aria_label = provider.get_aria_label(&notification);

    assert!(aria_label.starts_with("Error: "), "Expected aria label to start with \"Error: \", but got: {aria_label}");
    assert!(aria_label.contains("Something went wrong"), "Expected aria label to include original message");
    assert!(aria_label.contains("notification"), "Expected aria label to include \"notification\"");
}

#[test]
fn get_aria_label_includes_severity_prefix_for_warning_notifications() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let provider = accessibility_provider();
    let notification = create_item(Notification::new(Severity::Warning, "This is a warning"));
    close_on_dispose(&guard, &notification);
    let aria_label = provider.get_aria_label(&notification);

    assert!(aria_label.starts_with("Warning: "), "Expected aria label to start with \"Warning: \", but got: {aria_label}");
    assert!(aria_label.contains("This is a warning"), "Expected aria label to include original message");
    assert!(aria_label.contains("notification"), "Expected aria label to include \"notification\"");
}

#[test]
fn get_aria_label_includes_severity_prefix_for_info_notifications() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let provider = accessibility_provider();
    let notification = create_item(Notification::new(Severity::Info, "Information message"));
    close_on_dispose(&guard, &notification);
    let aria_label = provider.get_aria_label(&notification);

    assert!(aria_label.starts_with("Info: "), "Expected aria label to start with \"Info: \", but got: {aria_label}");
    assert!(aria_label.contains("Information message"), "Expected aria label to include original message");
    assert!(aria_label.contains("notification"), "Expected aria label to include \"notification\"");
}

#[test]
fn get_aria_label_includes_source_when_present() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let provider = accessibility_provider();
    let notification = create_item(Notification {
        source: Some(je_platform::notification::NotificationSourceOrText::Text("TestExtension".to_owned())),
        ..Notification::new(Severity::Error, "Error with source")
    });
    close_on_dispose(&guard, &notification);
    let aria_label = provider.get_aria_label(&notification);

    assert!(aria_label.starts_with("Error: "), "Expected aria label to start with severity prefix");
    assert!(aria_label.contains("Error with source"), "Expected aria label to include original message");
    assert!(aria_label.contains("source: TestExtension"), "Expected aria label to include source information");
    assert!(aria_label.contains("notification"), "Expected aria label to include \"notification\"");
}

#[test]
fn severity_prefix_consistency() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let provider = accessibility_provider();
    // Test that the severity prefixes are consistent with the ARIA alerts
    let error_notification = create_item(Notification::new(Severity::Error, "Error message"));
    let warning_notification = create_item(Notification::new(Severity::Warning, "Warning message"));
    let info_notification = create_item(Notification::new(Severity::Info, "Info message"));
    for notification in [&error_notification, &warning_notification, &info_notification] {
        close_on_dispose(&guard, notification);
    }

    let error_label = provider.get_aria_label(&error_notification);
    let warning_label = provider.get_aria_label(&warning_notification);
    let info_label = provider.get_aria_label(&info_notification);

    // Check that each severity type gets the correct prefix
    assert!(error_label.contains("Error: Error message"), "Error notifications should have Error prefix");
    assert!(warning_label.contains("Warning: Warning message"), "Warning notifications should have Warning prefix");
    assert!(info_label.contains("Info: Info message"), "Info notifications should have Info prefix");
}

// endregion

// region: NotificationsCenter

#[test]
fn updates_dismissal_affordances_when_progress_starts_completes_and_restarts() {
    warm_up_static_actions();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = attached_container(&ui, Some("monaco-workbench"), 800.0, 600.0);
    remove_on_dispose(&guard, &ui, container);

    let model = guard.add_rc(NotificationsModel::new());
    {
        let model = model.clone();
        guard.add(to_disposable(move || {
            for notification in model.notifications() {
                notification.close();
            }
        }));
    }
    let center = guard.add_rc(NotificationsCenter::new(container, model.clone(), center_services(&guard, &ui)));
    let handle = model.add_notification(Notification::new(Severity::Info, "Working..."));

    center.show();

    let clear_all_action = {
        let toolbars = find_all(&ui, container, &["notifications-center-header-toolbar"]);
        let toolbar = *toolbars.first().expect("a barra do cabeçalho existe");
        find_all(&ui, toolbar, &["codicon-notifications-clear-all"]).first().copied().expect("a ação de limpar tudo existe")
    };
    let states: RefCell<Vec<(bool, bool)>> = RefCell::new(Vec::new());
    let capture_state = || {
        let close_action_visible = find_all(&ui, container, &["notification-list-item-toolbar-container"])
            .into_iter()
            .any(|toolbar| !find_all(&ui, toolbar, &["codicon-notifications-clear"]).is_empty());
        let clear_all_disabled = ui.tree().has_class(clear_all_action, "disabled");
        states.borrow_mut().push((close_action_visible, clear_all_disabled));
    };

    capture_state();
    let progress = handle.progress();
    capture_state();
    progress.infinite();
    capture_state();
    progress.total(100.0);
    capture_state();
    progress.done();
    capture_state();
    progress.infinite();
    capture_state();
    progress.done();
    capture_state();

    assert_eq!(*states.borrow(), vec![(true, false), (true, false), (false, true), (false, true), (true, false), (false, true), (true, false)]);

    center.clear_all();
    assert_eq!(model.notifications().len(), 0);
}

// endregion
