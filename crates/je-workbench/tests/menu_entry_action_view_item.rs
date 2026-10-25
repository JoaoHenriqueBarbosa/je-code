//! Porte de `src/vs/platform/actions/test/browser/menuEntryActionViewItem.test.ts` (suíte `Context menu alternative
//! actions`), caso a caso e na mesma ordem.
//!
//! Este teste vive no `je-workbench` (e não no `je-platform`) porque o `ContextMenuService` monta o `Menu` de
//! `je-widgets`, e a plataforma não depende de widgets (ver `je_workbench::platform`).
//!
//! Divergências de tradução (nenhuma enfraquece uma asserção):
//! - `for (const modifier of ['Alt', 'Shift'])` e os dois `for` aninhados do segundo caso viram laços dentro de um
//!   `#[test]` por `test(...)` do original (o laço gera os nomes dos casos no TypeScript; aqui o nome do teste é o do
//!   `test` e cada volta confere o mesmo que o caso conferiria).
//! - `mainWindow.dispatchEvent(new KeyboardEvent(...))` é um `Event` de teclado despachado na raiz da `Ui`, com o
//!   `KeyCode` do modificador e os modificadores (`altKey`, `shiftKey`) no `KeyData`; `append(mainWindow.document.body,
//!   $('div'))` é um nó filho da raiz, tirado ao fim do caso.
//! - `new class extends mock<IContextViewService>()` é o `RecordingContextViewService`: `showContextView` monta o
//!   delegado no `host` e guarda o `hide` que o `hideContextView` chama (com `didCancel` verdadeiro), como no original.
//! - `host.querySelector('.action-label')?.textContent` é o texto da primeira subárvore com a classe; o
//!   `dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ... bubbles: true }))` é um `KeyDown` com `Enter`
//!   despachado no `li` (o `je-ui` borbulha o evento).
//! - `await Promise.resolve()` é avançar o relógio virtual em 1 ms (`runtime::with_faked_timers`).
//! - `deepStrictEqual` de objetos é a comparação de tuplas, campo a campo.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;
mod platform_actions_doubles;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::keybinding::MockKeybindingService;
use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::actions::{ActionRef, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::event::Event as BaseEvent;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_platform::action::CommandAction;
use je_platform::actions::menu_entry_action_view_item::get_flat_context_menu_actions;
use je_platform::actions::{MenuAction, MenuActionOptions, MenuItemAction};
use je_platform::commands::{CommandEvent, ICommandService};
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, HideData, IContextMenuDelegate, IContextMenuService, IContextViewDelegate, IContextViewService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::telemetry::null_telemetry_service;
use je_ui::{Content, Event, EventKind, KeyData, Modifiers, NodeId, Ui};
use je_widgets::menu::{IMenuOptions, Menu, unthemed_menu_styles};
use je_widgets::modifier_key_emitter::ModifierKeyEmitter;
use je_workbench::platform::contextview::{ContextMenuService, IContextMenuHandlerOptions};
use je_workbench::services::contextview::native_context_view_service::node_element;
use je_workbench::test::common::workbench_test_services::TestNotificationService;
use platform_actions_doubles::UnusedMenuService;

/// `mainWindow.dispatchEvent(new KeyboardEvent(kind, { key, keyCode, altKey, shiftKey }))`.
fn dispatch_modifier(ui: &Ui, kind: EventKind, key_code: KeyCode, alt: bool, shift: bool) {
    ui.dispatch(&mut Event::new(kind, ui.root()).with_key(KeyData { key_code, modifiers: Modifiers { alt, shift, ..Modifiers::default() } }));
}

/// `append(mainWindow.document.body, $('div'))` e o `disposables.add(toDisposable(() => host.remove()))`.
fn append_host(store: &LeakGuard, ui: &Ui) -> NodeId {
    let host = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        let host = tree.create(root, je_ui::Role::Generic);
        tree.set_bounds(host, je_render::kurbo::Rect::new(0.0, 0.0, 400.0, 300.0));
        host
    };
    let ui = ui.clone();
    store.add(to_disposable(move || ui.remove(host)));
    host
}

/// `host.querySelector('.action-label')?.textContent`.
fn first_action_label(ui: &Ui, host: NodeId) -> Option<String> {
    let tree = ui.tree();
    let label = tree.subtree(host).into_iter().skip(1).find(|node| tree.has_class(*node, "action-label"))?;
    Some(tree.subtree(label).into_iter().filter_map(|node| if let Content::Text { text, .. } = &tree.style(node).content { Some(text.clone()) } else { None }).collect())
}

/// O `IContextViewService` do primeiro caso: `showContextView` monta o delegado no `host`, `hideContextView` o
/// desmonta e avisa o `onHide(true)`.
struct RecordingContextViewService {
    host: NodeId,
    store: LeakGuardStore,
    hide: RefCell<Option<Box<dyn FnOnce()>>>,
}

type LeakGuardStore = Rc<je_base::lifecycle::DisposableStore>;

impl IContextViewService for RecordingContextViewService {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, _container: Option<Rc<ViewElement>>, _shadow_root: bool) {
        let rendered: Option<DisposableRef> = delegate.render(&node_element(self.host));
        if let Some(rendered) = &rendered {
            self.store.add(rendered.clone());
        }
        *self.hide.borrow_mut() = Some(Box::new(move || {
            if let Some(rendered) = rendered {
                rendered.dispose();
            }
            delegate.on_hide(Some(Rc::new(true) as HideData));
        }));
    }

    fn hide_context_view(&self, _data: Option<HideData>) {
        let on_hide = self.hide.borrow_mut().take();
        if let Some(on_hide) = on_hide {
            on_hide();
        }
    }

    fn layout(&self) {
        unreachable!("TypeError: layout is not a function")
    }

    fn get_context_view_element(&self) -> Rc<ViewElement> {
        unreachable!("TypeError: getContextViewElement is not a function")
    }
}

/// `{ getAnchor: () => host, getActions: () => [action] }`.
struct HostMenuDelegate {
    host: NodeId,
    actions: Vec<ActionRef>,
}

impl IContextMenuDelegate for HostMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        AnchorTarget::Element(node_element(self.host))
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        self.actions.clone()
    }
}

/// O `ICommandService` do segundo caso: guarda os argumentos de cada `executeCommand`.
#[derive(Default)]
struct RecordingCommandService {
    commands: RefCell<Vec<Vec<Dynamic>>>,
}

impl ICommandService for RecordingCommandService {
    fn on_will_execute_command(&self) -> BaseEvent<CommandEvent> {
        BaseEvent::none()
    }
    fn on_did_execute_command(&self) -> BaseEvent<CommandEvent> {
        BaseEvent::none()
    }
    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        let mut call = vec![Dynamic::from(command_id)];
        call.extend(args);
        self.commands.borrow_mut().push(call);
        Promise::resolved(Dynamic::Undefined)
    }
}

fn run_noop(id: &str, label: &str) -> ToActionProps {
    ToActionProps { id: id.to_owned(), label: label.to_owned(), tooltip: None, enabled: None, checked: None, class: None, alt: None, run: Rc::new(|_: &[Dynamic]| MaybePromise::Value(Dynamic::Undefined)) }
}

#[test]
fn html_context_menus_retain_the_modifier_at_opening_and_update_without_hover() {
    for (modifier, key_code) in [("Alt", KeyCode::Alt), ("Shift", KeyCode::Shift)] {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let ui = Ui::new();
        let (alt, shift) = (modifier == "Alt", modifier == "Shift");
        let modifiers = ModifierKeyEmitter::get_instance(&ui);
        modifiers.reset_key_status();
        disposables.add(to_disposable({
            let modifiers = modifiers.clone();
            move || modifiers.reset_key_status()
        }));
        let host = append_host(&disposables, &ui);
        let context_view_service = Rc::new(RecordingContextViewService { host, store: disposables.store(), hide: RefCell::new(None) });
        let service = disposables.add_rc(ContextMenuService::new(
            &ui,
            null_telemetry_service(),
            Rc::new(TestNotificationService),
            context_view_service.clone(),
            Rc::new(MockKeybindingService),
            Rc::new(UnusedMenuService),
            disposables.add_rc(MockContextKeyService::new()),
            Rc::new(unthemed_menu_styles()),
        ));
        service.configure(IContextMenuHandlerOptions { block_mouse: false });
        let alternative_action = to_action(run_noop("alternative", "Alternative"));
        let action = to_action(ToActionProps { alt: Some(alternative_action), ..run_noop("primary", "Primary") });
        let press = || dispatch_modifier(&ui, EventKind::KeyDown, key_code, alt, shift);
        press();
        service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(HostMenuDelegate { host, actions: vec![action] })));
        let mut labels = vec![first_action_label(&ui, host)];
        dispatch_modifier(&ui, EventKind::KeyUp, key_code, false, false);
        labels.push(first_action_label(&ui, host));
        press();
        labels.push(first_action_label(&ui, host));
        context_view_service.hide_context_view(None);
        let platform = Platform::detect();
        let alternative = if modifier == "Alt" || platform.is_windows || platform.is_linux { "Alternative" } else { "Primary" };
        let status = modifiers.key_status();
        assert_eq!(
            (labels, status.alt_key, status.shift_key),
            (vec![Some(alternative.to_owned()), Some("Primary".to_owned()), Some(alternative.to_owned())], false, false),
            "modificador {modifier}"
        );
    }
}

#[test]
fn alt_at_open_and_alt_at_activation() {
    for alt_at_open in [false, true] {
        for alt_at_activation in [false, true] {
            let disposables = ensure_no_disposables_are_leaked_in_test_suite();
            let ui = Ui::new();
            let modifiers = ModifierKeyEmitter::get_instance(&ui);
            let commands = Rc::new(RecordingCommandService::default());
            let context_key_service = disposables.add_rc(MockContextKeyService::new());
            let action = MenuItemAction::new(
                CommandAction::new("primary", "Primary"),
                Some(CommandAction::new("alternative", "Alternative")),
                Some(MenuActionOptions { args: Some(vec![Dynamic::from("source")]), ..MenuActionOptions::default() }),
                None,
                None,
                &*context_key_service,
                commands.clone(),
            );
            dispatch_modifier(&ui, EventKind::KeyDown, KeyCode::Alt, alt_at_open, false);
            let actions = get_flat_context_menu_actions(&[("navigation".to_owned(), vec![MenuAction::Item(action)])], None);
            let host = append_host(&disposables, &ui);
            let menu = disposables.add_rc(Menu::new(&ui, host, &actions, IMenuOptions::default(), Rc::new(unthemed_menu_styles())));
            let executed = Rc::new(Cell::new(false));
            let (label_at_open, label_at_activation) = {
                let (ui, host, executed) = (ui.clone(), host, executed.clone());
                let mut labels = (None, None);
                runtime::with_faked_timers(|| {
                    menu.focus(true);
                    labels.0 = first_action_label(&ui, host);
                    dispatch_modifier(&ui, if alt_at_activation { EventKind::KeyDown } else { EventKind::KeyUp }, KeyCode::Alt, alt_at_activation, false);
                    labels.1 = first_action_label(&ui, host);
                    let item = {
                        let tree = ui.tree();
                        tree.subtree(host).into_iter().skip(1).find(|node| tree.has_class(*node, "action-item")).expect("li do item")
                    };
                    ui.dispatch(&mut Event::new(EventKind::KeyDown, item).with_key(KeyData { key_code: KeyCode::Enter, modifiers: Modifiers { alt: alt_at_activation, ..Modifiers::default() } }));
                    // `await Promise.resolve()`
                    runtime::advance_fake_time(1);
                    executed.set(true);
                });
                labels
            };
            assert!(executed.get());
            let expected_command = if alt_at_activation { "alternative" } else { "primary" };
            assert_eq!(
                (label_at_open, label_at_activation, commands.commands.borrow().clone()),
                (
                    Some(if alt_at_open { "Alternative" } else { "Primary" }.to_owned()),
                    Some(if alt_at_activation { "Alternative" } else { "Primary" }.to_owned()),
                    vec![vec![Dynamic::from(expected_command), Dynamic::from("source")]],
                ),
                "Alt at open: {alt_at_open}, Alt at activation: {alt_at_activation}"
            );
            modifiers.reset_key_status();
        }
    }
}
