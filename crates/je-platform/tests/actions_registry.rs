//! Casos que NÃO são do original: o `actions.test.ts` do VS Code é o do `menuService`, que não foi portado
//! (ver o topo de `src/actions/mod.rs`). Estes casos cobrem o que `actions.ts` faz sozinho: `MenuId`, `MenuRegistry`,
//! `MenuItemAction` e `registerAction2`. Os valores esperados seguem o código do original.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::contextkey::TestConfigurationService;
use common::*;
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::actions::IAction;
use je_base::promise::Promise;
use je_instantiation::ServicesAccessor;
use je_platform::action::{CommandAction, CommandActionToggleInfo, Title, Toggled};
use je_platform::actions::{
    Action2, Action2Keybinding, Action2MenuItem, Action2Options, AnyMenuItem, MenuActionOptions, MenuId, MenuItem, MenuItemAction, menu_ids, menu_registry,
    register_action2,
};
use je_platform::commands::{CommandEvent, ICommandService, commands_registry};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextkey::ContextKeyExpr;
use je_platform::keybinding::keybindings_registry::{Keybindings, keybindings_registry};

#[test]
fn menu_id_for_reuses_and_new_throws_on_duplicates() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(MenuId::for_id("CommandPalette"), menu_ids::command_palette());
    assert_eq!(MenuId::for_id("Custom.Menu"), MenuId::for_id("Custom.Menu"));
    assert!(errors::catch(|| MenuId::new("EditorContext")).is_err());
    assert_eq!(MenuId::new("Brand.New").id, "Brand.New");
    assert!(errors::catch(|| MenuId::new("Brand.New")).is_err());
}

#[test]
fn menu_registry_command_palette_lists_added_commands_implicitly() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = menu_registry();
    let command = CommandAction::new("test.cmd", "Test");
    let added = registry.add_command(command.clone());
    assert!(registry.get_command("test.cmd").is_some());

    let explicit = registry.append_menu_item(&menu_ids::command_palette(), AnyMenuItem::Menu(MenuItem::new(CommandAction::new("test.explicit", "Explicit"))));
    let ids: Vec<String> = registry
        .get_menu_items(&menu_ids::command_palette())
        .iter()
        .filter_map(|item| match item {
            AnyMenuItem::Menu(item) => Some(item.command.id.clone()),
            AnyMenuItem::Submenu(_) => None,
        })
        .collect();
    assert_eq!(ids, vec!["test.explicit".to_string(), "test.cmd".to_string()]);

    added.dispose();
    explicit.dispose();
    assert!(registry.get_command("test.cmd").is_none());
    assert!(registry.get_menu_items(&menu_ids::command_palette()).is_empty());
}

type Calls = Rc<RefCell<Vec<(String, Vec<Dynamic>)>>>;

struct RecordingCommandService {
    calls: Calls,
}

impl ICommandService for RecordingCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.calls.borrow_mut().push((command_id.to_string(), args));
        Promise::resolved(Dynamic::Undefined)
    }
}

#[test]
fn menu_item_action_label_enablement_toggle_and_run() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context = guard.add_rc(ContextKeyService::new(TestConfigurationService::new()));
    context.set_context("on", Dynamic::Bool(true));
    let calls = Rc::new(RefCell::new(Vec::new()));
    let command_service: Rc<dyn ICommandService> = Rc::new(RecordingCommandService { calls: calls.clone() });

    let mut item = CommandAction::new("test.toggle", "Toggle Thing");
    item.short_title = Some(Title::Plain("Toggle".into()));
    item.precondition = Some(ContextKeyExpr::has("off"));
    item.toggled = Some(Toggled::Info(CommandActionToggleInfo {
        condition: ContextKeyExpr::has("on"),
        icon: None,
        tooltip: Some("Tip when on".into()),
        title: Some("Thing is on".into()),
        mnemonic_title: None,
    }));

    let action = MenuItemAction::new(
        item,
        None,
        Some(MenuActionOptions { args: Some(vec![Dynamic::Number(1.0)]), should_forward_args: true, render_short_title: true, ..Default::default() }),
        None,
        None,
        &*context,
        command_service,
    );
    assert_eq!(action.label(), "Thing is on");
    assert_eq!(action.tooltip(), "Tip when on");
    assert!(!action.enabled());
    assert_eq!(action.checked(), Some(true));

    async_run(action.run(&[Dynamic::Number(2.0)]));
    assert_eq!(calls.borrow().clone(), vec![("test.toggle".to_string(), vec![Dynamic::Number(1.0), Dynamic::Number(2.0)])]);
}

fn async_run(promise: Promise<Dynamic>) {
    run_with_faked_timers(async move {
        promise.await.unwrap();
    });
}

struct TestAction {
    ran: Rc<RefCell<u32>>,
}

impl Action2 for TestAction {
    fn desc(&self) -> Action2Options {
        let mut command = CommandAction::new("test.action2", "Action Two");
        command.precondition = Some(ContextKeyExpr::has("enabled"));
        Action2Options {
            command,
            menu: vec![Action2MenuItem {
                id: menu_ids::editor_context(),
                null_precondition: true,
                alt: None,
                when: None,
                group: Some("navigation".into()),
                order: Some(1.0),
                is_hidden_by_default: None,
            }],
            keybinding: vec![Action2Keybinding {
                keybindings: Keybindings { primary: Some(je_base::key_codes::KeyMod::CTRL_CMD | 31), ..Default::default() },
                weight: 200,
                args: Dynamic::Undefined,
                when: Some(ContextKeyExpr::has("focus")),
            }],
            f1: true,
        }
    }

    fn run(&self, _accessor: &ServicesAccessor, _args: &[Dynamic]) {
        *self.ran.borrow_mut() += 1;
    }
}

#[test]
fn register_action2_registers_command_menu_f1_and_keybinding_and_disposes_all() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ran = Rc::new(RefCell::new(0));
    let registration = register_action2(Rc::new(TestAction { ran: ran.clone() }));

    assert!(commands_registry().get_command("test.action2").is_some());
    let editor_items = menu_registry().get_menu_items(&menu_ids::editor_context());
    assert_eq!(editor_items.len(), 1);
    match &editor_items[0] {
        // `precondition: null` no item de menu: o comando do item não herda a pré-condição.
        AnyMenuItem::Menu(item) => assert!(item.command.precondition.is_none()),
        AnyMenuItem::Submenu(_) => panic!("esperava um item de comando"),
    }
    let palette = menu_registry().get_menu_items(&menu_ids::command_palette());
    assert_eq!(palette.len(), 1);
    assert!(menu_registry().get_command("test.action2").is_some());
    let default_keybindings = keybindings_registry().get_default_keybindings();
    let item = default_keybindings.iter().find(|k| k.command.as_deref() == Some("test.action2")).expect("atalho registrado");
    // A pré-condição do comando entra no `when` do atalho.
    assert_eq!(item.when.as_ref().map(|w| w.serialize()), Some("enabled && focus".to_string()));

    // Registrar outra ação com o mesmo id lança.
    assert!(errors::catch(|| register_action2(Rc::new(TestAction { ran: ran.clone() }))).is_err());

    registration.dispose();
    assert!(commands_registry().get_command("test.action2").is_none());
    assert!(menu_registry().get_menu_items(&menu_ids::editor_context()).is_empty());
    assert!(menu_registry().get_command("test.action2").is_none());
    assert!(keybindings_registry().get_default_keybindings().iter().all(|k| k.command.as_deref() != Some("test.action2")));
}
