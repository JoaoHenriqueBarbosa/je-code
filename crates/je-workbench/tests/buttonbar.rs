//! Porte de `src/vs/platform/actions/test/browser/buttonbar.test.ts` (suíte `WorkbenchButtonBar`), caso a caso e na
//! mesma ordem.
//!
//! Este teste vive no `je-workbench` (e não no `je-platform`) porque o `WorkbenchButtonBar` estende a `ButtonBar` de
//! `je-widgets`, e a plataforma não depende de widgets (ver `je_workbench::platform`).
//!
//! Divergências de tradução (nenhuma enfraquece uma asserção):
//! - `$('div')` é um nó solto da `Ui`; `instantiationService.stub(...)` são os argumentos do construtor (o
//!   `IContextMenuService` que não mostra nada, o `IKeybindingService` cujo `appendKeybinding` devolve o rótulo, o
//!   `IHoverService` que não faz nada: o delegado base de hover do `je-widgets`, que devolve um hover vazio).
//! - `element.querySelectorAll(sel)` é a contagem de descendentes (sem o próprio nó) com a classe; `firstElementChild`
//!   é o primeiro filho; `classList` é a lista de classes do nó, na ordem; `textContent` é o texto de todos os
//!   `Content::Text` da subárvore.
//! - `.monaco-pixel-spinner-paused` (a classe que o observador de visibilidade alterna) não existe sem animação (ver
//!   `je_widgets::pixel_spinner`); o filtro dela em `leadingSlot` fica, e não tem o que filtrar.
//! - `button.click()` é um evento `Click` despachado no nó do botão. `undefined` do `leadingSlot` é `None`.
//! - `deepStrictEqual` de objetos é a comparação de tuplas (ou de `Option`) campo a campo, na mesma ordem.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;
mod platform_actions_doubles;

use std::cell::RefCell;
use std::rc::Rc;

use common::keybinding::MockKeybindingService;
use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::actions::{ActionRef, Separator, SubmenuAction, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::MaybePromise;
use je_base::themables::ThemeIcon;
use je_platform::action::{CommandAction, Icon};
use je_platform::actions::MenuItemAction;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_render::kurbo::Rect;
use je_ui::{Content, Event, EventKind, NodeId, Role, Ui};
use je_widgets::hover_api::get_base_layer_hover_delegate;
use je_workbench::platform::actions::{IButtonConfig, IButtonConfigProvider, IWorkbenchButtonBarOptions, WorkbenchButtonBar};
use platform_actions_doubles::{NoopContextMenuService, TelemetryEvent, TestTelemetryService, UnusedCommandService};

const SPINNER_CLASS: &str = "monaco-pixel-spinner";

struct Fixture {
    ui: Ui,
    bar: Rc<WorkbenchButtonBar>,
    container: NodeId,
}

/// `createButtonBar(buttonConfigProvider, options?)`.
fn create_button_bar(store: &LeakGuard, button_config_provider: IButtonConfigProvider, telemetry: Option<(Rc<TestTelemetryService>, &str)>) -> Fixture {
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let container = tree.create_detached(Role::Generic);
        tree.set_bounds(container, Rect::new(0.0, 0.0, 400.0, 30.0));
        container
    };
    let (telemetry_service, telemetry_source) = match telemetry {
        Some((service, source)) => (service, Some(source.to_owned())),
        None => (Rc::new(TestTelemetryService::default()), None),
    };
    let bar = store.add_rc(WorkbenchButtonBar::new(
        &ui,
        container,
        Some(IWorkbenchButtonBarOptions { button_config_provider: Some(button_config_provider), telemetry_source, ..IWorkbenchButtonBarOptions::default() }),
        Rc::new(NoopContextMenuService),
        Rc::new(MockKeybindingService),
        telemetry_service,
        get_base_layer_hover_delegate(),
    ));
    Fixture { ui, bar, container }
}

fn action(id: &str) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label: id.to_owned(),
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(|_: &[Dynamic]| MaybePromise::Value(Dynamic::Undefined)),
    })
}

/// An action carrying an icon the way any non-menu action does: as a CSS class.
fn icon_action(id: &str) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label: id.to_owned(),
        tooltip: None,
        enabled: None,
        checked: None,
        class: Some(ThemeIcon::from_id("git-commit").as_class_name()),
        alt: None,
        run: Rc::new(|_: &[Dynamic]| MaybePromise::Value(Dynamic::Undefined)),
    })
}

/// A menu action carrying its icon the way the production title bar supplies one: declared on the command rather than
/// as a CSS class.
fn menu_icon_action(store: &LeakGuard, id: &str) -> ActionRef {
    let item = CommandAction { icon: Some(Icon::Theme(ThemeIcon::from_id("git-commit"))), ..CommandAction::new(id, id) };
    let context_key_service = store.add_rc(MockContextKeyService::new());
    MenuItemAction::new(item, None, None, None, None, &*context_key_service, Rc::new(UnusedCommandService))
}

/// `querySelectorAll('.class').length`: os descendentes (sem o próprio nó) com a classe.
fn count_with_class(ui: &Ui, root: NodeId, class: &str) -> usize {
    let tree = ui.tree();
    tree.subtree(root).into_iter().skip(1).filter(|node| tree.has_class(*node, class)).count()
}

/// `textContent`: o texto de todos os `Content::Text` da subárvore, na ordem.
fn text_content(ui: &Ui, root: NodeId) -> String {
    let tree = ui.tree();
    tree.subtree(root).into_iter().filter_map(|node| if let Content::Text { text, .. } = &tree.style(node).content { Some(text.clone()) } else { None }).collect()
}

/// Classes of the button's leading slot, minus the animation-state class an observer toggles on the spinner.
fn leading_slot(ui: &Ui, button: NodeId) -> Option<String> {
    let tree = ui.tree();
    let leading = tree.children(button).first().copied()?;
    Some(tree.classes(leading).iter().filter(|name| name.as_str() != "monaco-pixel-spinner-paused").cloned().collect::<Vec<_>>().join(" "))
}

fn element_of(fixture: &Fixture, index: usize) -> NodeId {
    fixture.bar.buttons()[index].element()
}

fn has_class(ui: &Ui, node: NodeId, class: &str) -> bool {
    ui.tree().has_class(node, class)
}

/// `(_action, index) => ({ showLabel, showIcon, showSpinner: index === 0 })`.
fn spinner_on_first(show_label: bool, show_icon: Option<bool>) -> IButtonConfigProvider {
    Rc::new(move |_: &ActionRef, index: usize| Some(IButtonConfig { show_label: Some(show_label), show_icon, show_spinner: Some(index == 0), ..IButtonConfig::default() }))
}

#[test]
fn renders_the_spinner_only_for_buttons_that_ask_for_it() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_button_bar(&store, spinner_on_first(true, None), None);

    fixture.bar.update(&[action("busy"), action("idle")], &[]);
    let button = element_of(&fixture, 0);
    let ui = &fixture.ui;

    let first_child = ui.tree().children(button).first().copied();
    assert_eq!(
        (
            count_with_class(ui, fixture.container, SPINNER_CLASS),
            count_with_class(ui, element_of(&fixture, 1), SPINNER_CLASS),
            // The spinner leads the label rather than replacing it.
            first_child.map(|node| has_class(ui, node, SPINNER_CLASS)),
            text_content(ui, button),
        ),
        (1, 0, Some(true), "busy".to_owned()),
    );
}

#[test]
fn renders_the_spinner_on_the_primary_half_of_a_dropdown_button() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_button_bar(&store, Rc::new(|_: &ActionRef, _: usize| Some(IButtonConfig { show_label: Some(true), show_spinner: Some(true), ..IButtonConfig::default() })), None);

    let separator: ActionRef = Separator::new();
    let submenu: ActionRef = SubmenuAction::new("id", "Merge", vec![action("Merge"), separator, action("Other")], None);
    fixture.bar.update(&[submenu], &[]);
    let button = element_of(&fixture, 0);
    let ui = &fixture.ui;

    let dropdown_button = {
        let tree = ui.tree();
        tree.subtree(button).into_iter().skip(1).find(|node| tree.has_class(*node, "monaco-dropdown-button"))
    };
    assert_eq!(
        (
            count_with_class(ui, button, SPINNER_CLASS),
            // Not on the dropdown chevron, which is a sibling button.
            dropdown_button.map(|node| count_with_class(ui, node, SPINNER_CLASS)),
        ),
        (1, Some(0)),
    );
}

#[test]
fn the_spinner_takes_the_place_of_the_icon_in_the_shared_leading_slot() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_button_bar(&store, spinner_on_first(true, Some(true)), None);

    fixture.bar.update(&[icon_action("busy"), icon_action("idle")], &[]);
    let (busy, idle) = (element_of(&fixture, 0), element_of(&fixture, 1));
    let ui = &fixture.ui;

    assert_eq!(
        (
            leading_slot(ui, busy),
            count_with_class(ui, busy, "codicon"),
            text_content(ui, busy),
            leading_slot(ui, idle),
            count_with_class(ui, idle, SPINNER_CLASS),
            text_content(ui, idle),
        ),
        (
            Some("monaco-pixel-spinner monaco-button-leading-icon".to_owned()),
            0,
            "busy".to_owned(),
            Some("codicon codicon-git-commit monaco-button-leading-icon".to_owned()),
            0,
            "idle".to_owned(),
        ),
    );
}

#[test]
fn the_spinner_stands_in_for_the_icon_of_an_icon_only_button() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_button_bar(&store, spinner_on_first(false, Some(true)), None);

    fixture.bar.update(&[icon_action("busy"), icon_action("idle")], &[]);
    let (busy, idle) = (element_of(&fixture, 0), element_of(&fixture, 1));
    let ui = &fixture.ui;

    assert_eq!(
        (
            // An icon-only button wears its icon as a class on the button itself, so it has to come off while the
            // spinner stands in for it.
            has_class(ui, busy, "codicon-git-commit"),
            leading_slot(ui, busy),
            has_class(ui, idle, "codicon-git-commit"),
            leading_slot(ui, idle),
        ),
        (false, Some("monaco-pixel-spinner monaco-button-leading-icon monaco-button-leading-icon-only".to_owned()), true, None),
    );
}

#[test]
fn a_menu_action_gets_its_declared_icon_in_the_leading_slot_replaced_by_the_spinner() {
    // The production title bar supplies `MenuItemAction`s, whose icon is declared on the command rather than carried as a
    // CSS class.
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_button_bar(&store, spinner_on_first(true, Some(true)), None);

    fixture.bar.update(&[menu_icon_action(&store, "busy"), menu_icon_action(&store, "idle")], &[]);
    let (busy, idle) = (element_of(&fixture, 0), element_of(&fixture, 1));
    let ui = &fixture.ui;

    assert_eq!(
        (
            leading_slot(ui, busy),
            count_with_class(ui, busy, "codicon"),
            text_content(ui, busy),
            leading_slot(ui, idle),
            count_with_class(ui, idle, SPINNER_CLASS),
            // The icon renders in its own slot rather than inline in the label.
            text_content(ui, idle),
        ),
        (
            Some("monaco-pixel-spinner monaco-button-leading-icon".to_owned()),
            0,
            "busy".to_owned(),
            Some("codicon codicon-git-commit monaco-button-leading-icon".to_owned()),
            0,
            "idle".to_owned(),
        ),
    );
}

#[test]
fn an_icon_only_menu_action_wears_its_declared_icon_and_only_the_spinner_while_busy() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_button_bar(&store, spinner_on_first(false, Some(true)), None);

    fixture.bar.update(&[menu_icon_action(&store, "busy"), menu_icon_action(&store, "idle")], &[]);
    let (busy, idle) = (element_of(&fixture, 0), element_of(&fixture, 1));
    let ui = &fixture.ui;

    assert_eq!(
        (has_class(ui, busy, "codicon-git-commit"), leading_slot(ui, busy), has_class(ui, idle, "codicon-git-commit"), leading_slot(ui, idle)),
        (false, Some("monaco-pixel-spinner monaco-button-leading-icon monaco-button-leading-icon-only".to_owned()), true, None),
    );
}

#[test]
fn logs_telemetry_before_an_action_disposes_the_button_bar() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let telemetry_service = Rc::new(TestTelemetryService::default());
    let fixture = create_button_bar(
        &store,
        Rc::new(|_: &ActionRef, _: usize| Some(IButtonConfig { show_label: Some(true), ..IButtonConfig::default() })),
        Some((telemetry_service.clone(), "testButtonBar")),
    );
    let events_during_run: Rc<RefCell<Vec<TelemetryEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let self_disposing_action = to_action(ToActionProps {
        id: "selfDisposing".to_owned(),
        label: "selfDisposing".to_owned(),
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: Rc::new({
            let (telemetry_service, events_during_run, bar) = (telemetry_service.clone(), events_during_run.clone(), Rc::downgrade(&fixture.bar));
            move |_: &[Dynamic]| {
                *events_during_run.borrow_mut() = telemetry_service.events.borrow().clone();
                if let Some(bar) = bar.upgrade() {
                    bar.dispose();
                }
                MaybePromise::Value(Dynamic::Undefined)
            }
        }),
    });

    fixture.bar.update(&[self_disposing_action], &[]);
    fixture.ui.dispatch(&mut Event::new(EventKind::Click, element_of(&fixture, 0)));

    let expected_events = vec![TelemetryEvent::workbench_action_executed("selfDisposing", "testButtonBar")];
    assert_eq!(*events_during_run.borrow(), expected_events);
    assert_eq!(*telemetry_service.events.borrow(), expected_events);
}
