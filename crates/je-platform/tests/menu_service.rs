//! Porte de `src/vs/platform/actions/test/common/menuService.test.ts` (suíte `MenuService`), caso a caso e na mesma ordem.
//!
//! Divergência de `getMenuActions filters before sorting...`: o contador `hiddenTitleReads` (leituras do getter `title`
//! de um objeto literal) não existe com `Title` por valor; o filtro antes da ordenação é conferido pelas ações
//! resultantes, pela leitura única do registro, por zero chamadas de `keys` e por zero assinaturas de `onDidChangeMenu`.
//! Os itens que o teste altera depois de registrados vão ao registro como `SharedMenuItem`, a referência compartilhada
//! que faz o papel do objeto `IMenuItem` do original.
//!
//! Divergências: o `TestContextKeyService` é o `ContextKeyService` real (o do original avalia a regra contra as
//! chaves, que é o que o real faz); o `NullCommandService.executeCommand` espiado é um serviço de comandos que registra
//! as chamadas.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use je_platform::configuration::test_configuration_service::TestConfigurationService;
use common::keybinding::MockKeybindingService;
use common::*;
use je_base::actions::IAction;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uuid::generate_uuid;
use je_platform::action::{CommandAction, CommandActionTitle, CommandActionToggleInfo, Title, Toggled};
use je_platform::actions::{
    AnyMenuItem, IMenu, IMenuService, MenuAction, MenuActionGroup, MenuActionOptions, MenuCreateOptions, MenuId, MenuItem, MenuService, SharedMenuItem, SubmenuItem,
    menu_ids, menu_registry,
};
use je_platform::commands::{CommandEvent, ICommandService};
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::storage::storage::InMemoryStorageService;

/// `NullCommandService` com `executeCommand` espiado.
#[derive(Default)]
struct RecordingCommandService {
    calls: RefCell<Vec<Vec<String>>>,
}

impl ICommandService for RecordingCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        let mut call = vec![command_id.to_owned()];
        call.extend(args.iter().map(|a| match a {
            Dynamic::String(text) => text.to_string_lossy(),
            other => format!("{other:?}"),
        }));
        self.calls.borrow_mut().push(call);
        Promise::resolved(Dynamic::Undefined)
    }
}

struct Fixture {
    guard: LeakGuard,
    menu_service: Rc<MenuService>,
    commands: Rc<RecordingCommandService>,
    context_key_service: Rc<ContextKeyService>,
    test_menu_id: MenuId,
}

fn setup() -> Fixture {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let commands = Rc::new(RecordingCommandService::default());
    let storage = guard.add_rc(InMemoryStorageService::new());
    let menu_service = guard.add_rc(MenuService::new(commands.clone(), Rc::new(MockKeybindingService), Rc::new(storage)));
    let context_key_service = guard.add_rc(ContextKeyService::new(TestConfigurationService::new(None)));
    Fixture { guard, menu_service, commands, context_key_service, test_menu_id: MenuId::new(&format!("testo/{}", generate_uuid())) }
}

/// `disposables.add(menu)` para um `IMenu` atrás de um `Rc<dyn ...>`.
fn track(guard: &LeakGuard, menu: Rc<dyn IMenu>) -> Rc<dyn IMenu> {
    let to_dispose = menu.clone();
    guard.add(je_base::lifecycle::to_disposable(move || to_dispose.dispose()));
    menu
}

fn item(id: &str, title: &str) -> MenuItem {
    MenuItem::new(CommandAction::new(id, title))
}

fn append(f: &Fixture, id: &MenuId, item: MenuItem) {
    f.guard.add(menu_registry().append_menu_item(id, AnyMenuItem::Menu(item)));
}

fn action_id(action: &MenuAction) -> String {
    match action {
        MenuAction::Item(item) => item.id(),
        MenuAction::Submenu(submenu) => submenu.as_action().id(),
    }
}

fn ids(groups: &[MenuActionGroup]) -> Vec<(String, Vec<String>)> {
    groups.iter().map(|(group, actions)| (group.clone(), actions.iter().map(action_id).collect())).collect()
}

fn flat_ids(groups: &[MenuActionGroup]) -> Vec<String> {
    groups.iter().flat_map(|(_, actions)| actions.iter().map(action_id)).collect()
}

fn sorted_contexts(menu_service: &MenuService, id: &MenuId) -> Vec<String> {
    let mut contexts: Vec<String> = menu_service.get_menu_contexts(id).into_iter().collect();
    contexts.sort();
    contexts
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

#[test]
fn create_menu_collects_menu_items_only_once() {
    let f = setup();
    let before = menu_registry().menu_item_reads(&f.test_menu_id);
    track(&f.guard, f.menu_service.create_menu(&f.test_menu_id, f.context_key_service.clone(), None));

    assert_eq!(menu_registry().menu_item_reads(&f.test_menu_id) - before, 1);
}

#[test]
fn get_menu_actions_filters_before_sorting_and_does_not_collect_event_dependencies() {
    let f = setup();
    let when = ContextKeyExpr::equals("view", "anotherView");
    for i in 0..875 {
        append(&f, &f.test_menu_id, MenuItem { when: Some(when.clone()), ..item(&format!("hidden{i}"), &format!("Hidden {i}")) });
    }
    append(&f, &f.test_menu_id, item("z", "Z"));
    append(&f, &f.test_menu_id, item("a", "A"));
    let (reads_before, keys_before, listens_before) =
        (menu_registry().menu_item_reads(&f.test_menu_id), ContextKeyExpression::keys_call_count(), menu_registry().change_subscriptions());

    let groups = f.menu_service.get_menu_actions(&f.test_menu_id, &*f.context_key_service, None);

    assert_eq!(ids(&groups), vec![(String::new(), strings(&["a", "z"]))]);
    assert_eq!(menu_registry().menu_item_reads(&f.test_menu_id) - reads_before, 1);
    assert_eq!(ContextKeyExpression::keys_call_count() - keys_before, 0);
    assert_eq!(menu_registry().change_subscriptions() - listens_before, 0);
}

#[test]
fn get_menu_actions_evaluates_each_row_context_independently() {
    let f = setup();
    append(&f, &f.test_menu_id, MenuItem { when: Some(ContextKeyExpr::equals("viewItem", "file")), ..item("file", "File") });
    append(&f, &f.test_menu_id, MenuItem { when: Some(ContextKeyExpr::equals("viewItem", "folder")), ..item("folder", "Folder") });
    let mut action_ids: Vec<String> = Vec::new();

    for i in 0..100 {
        let row_context = f.guard.add_rc(ContextKeyService::new(TestConfigurationService::new(None)));
        row_context.create_key("viewItem", Dynamic::string(if i % 2 == 0 { "file" } else { "folder" }));
        let groups = f.menu_service.get_menu_actions(&f.test_menu_id, &*row_context, None);
        action_ids.extend(flat_ids(&groups));
    }

    let contexts = sorted_contexts(&f.menu_service, &f.test_menu_id);

    assert_eq!(action_ids, (0..100).map(|i| if i % 2 == 0 { "file" } else { "folder" }.to_owned()).collect::<Vec<_>>());
    assert_eq!(contexts, strings(&["viewItem"]));
}

#[test]
fn one_shot_and_persistent_menus_use_the_same_group_and_item_ordering() {
    let f = setup();
    let localized = CommandAction::new("localized", Title::Localized(CommandActionTitle { value: "ZZZ".into(), original: "AAA".into(), mnemonic_title: None }));
    let items = vec![
        item("z", "Z"),
        MenuItem { group: Some(String::new()), when: Some(ContextKeyExpr::has("hidden")), ..item("hidden", "Hidden") },
        item("a", "A"),
        MenuItem { group: Some(String::new()), order: Some(-1.0), ..item("first", "First") },
        MenuItem { group: Some("navigation".into()), ..item("navigation", "Navigation") },
        MenuItem { group: Some("primary".into()), ..MenuItem::new(localized) },
        MenuItem { group: Some("primary".into()), ..item("middle", "BBB") },
        MenuItem { group: Some("primary".into()), ..item("tie", "BBB") },
    ];
    f.guard.add(menu_registry().append_menu_items(items.into_iter().map(|i| (f.test_menu_id.clone(), AnyMenuItem::Menu(i))).collect()));
    let menu = track(&f.guard, f.menu_service.create_menu(&f.test_menu_id, f.context_key_service.clone(), None));
    let snapshots = [ids(&menu.get_actions(None)), ids(&f.menu_service.get_menu_actions(&f.test_menu_id, &*f.context_key_service, None))];
    let expected = vec![
        ("navigation".to_owned(), strings(&["navigation"])),
        ("primary".to_owned(), strings(&["localized", "middle", "tie"])),
        (String::new(), strings(&["first", "a", "z"])),
    ];

    assert_eq!(snapshots, [expected.clone(), expected]);
}

#[test]
fn get_menu_actions_observes_menu_registrations_synchronously() {
    let f = setup();
    append(&f, &f.test_menu_id, item("b", "B"));
    let get_ids = || flat_ids(&f.menu_service.get_menu_actions(&f.test_menu_id, &*f.context_key_service, None));
    let mut snapshots = vec![get_ids()];
    let mut contexts = vec![sorted_contexts(&f.menu_service, &f.test_menu_id)];
    let registration = menu_registry().append_menu_item(&f.test_menu_id, AnyMenuItem::Menu(MenuItem { when: Some(ContextKeyExpr::not("missing")), ..item("a", "A") }));
    snapshots.push(get_ids());
    contexts.push(sorted_contexts(&f.menu_service, &f.test_menu_id));
    registration.dispose();
    snapshots.push(get_ids());
    contexts.push(sorted_contexts(&f.menu_service, &f.test_menu_id));

    assert_eq!(snapshots, vec![strings(&["b"]), strings(&["a", "b"]), strings(&["b"])]);
    assert_eq!(contexts, vec![strings(&[]), strings(&["missing"]), strings(&[])]);
}

fn submenu_item(title: &str, submenu: &MenuId) -> SubmenuItem {
    SubmenuItem { title: Title::from(title), submenu: submenu.clone(), icon: None, when: None, group: None, order: None, is_selection: None, is_split_button: None }
}

#[test]
fn get_menu_actions_omits_empty_submenus_and_observes_nested_registrations() {
    let f = setup();
    let submenu_id = MenuId::new(&format!("testo/{}", generate_uuid()));
    let nested_submenu_id = MenuId::new(&format!("testo/{}", generate_uuid()));
    f.guard.add(menu_registry().append_menu_item(&f.test_menu_id, AnyMenuItem::Submenu(submenu_item("Submenu", &submenu_id))));
    f.guard.add(menu_registry().append_menu_item(&submenu_id, AnyMenuItem::Submenu(submenu_item("Nested submenu", &nested_submenu_id))));
    let before = f.menu_service.get_menu_actions(&f.test_menu_id, &*f.context_key_service, None);
    let registration = menu_registry().append_menu_item(&nested_submenu_id, AnyMenuItem::Menu(item("nested", "Nested")));
    let groups = f.menu_service.get_menu_actions(&f.test_menu_id, &*f.context_key_service, None);
    let MenuAction::Submenu(submenu) = &groups[0].1[0] else { panic!("devia ser um submenu") };
    let nested = submenu.actions()[0].as_any().downcast_ref::<je_base::actions::SubmenuAction>().expect("o item aninhado é um submenu");
    let nested_actions: Vec<String> = nested.actions().iter().map(|a| a.id()).collect();
    registration.dispose();
    let after = f.menu_service.get_menu_actions(&f.test_menu_id, &*f.context_key_service, None);

    assert!(before.is_empty());
    assert_eq!(nested_actions, strings(&["nested"]));
    assert!(after.is_empty());
}

#[test]
fn get_menu_actions_observes_implicit_command_palette_items_synchronously() {
    let f = setup();
    let command_id = format!("testo/{}", generate_uuid());
    let contains_command = || {
        f.menu_service
            .get_menu_actions(&menu_ids::command_palette(), &*f.context_key_service, None)
            .iter()
            .any(|(_, actions)| actions.iter().any(|a| action_id(a) == command_id))
    };
    let mut snapshots = vec![contains_command()];
    let registration = menu_registry().add_command(CommandAction::new(command_id.as_str(), "Implicit"));
    snapshots.push(contains_command());
    registration.dispose();
    snapshots.push(contains_command());

    assert_eq!(snapshots, [false, true, false]);
}

/// Registra um item que o teste altera depois, como o objeto `IMenuItem` do original.
fn append_shared(f: &Fixture, id: &MenuId, item: MenuItem) -> SharedMenuItem {
    let shared: SharedMenuItem = Rc::new(RefCell::new(AnyMenuItem::Menu(item)));
    f.guard.add(menu_registry().append_shared_menu_item(id, shared.clone()));
    shared
}

fn edit(shared: &SharedMenuItem, change: impl FnOnce(&mut MenuItem)) {
    let AnyMenuItem::Menu(item) = &mut *shared.borrow_mut() else { panic!("devia ser um item de comando") };
    change(item);
}

fn action_label(action: &MenuAction) -> String {
    match action {
        MenuAction::Item(item) => item.label(),
        MenuAction::Submenu(submenu) => submenu.as_action().label(),
    }
}

#[test]
fn get_menu_actions_observes_changes_to_registered_titles_orders_and_groups() {
    let f = setup();
    let dynamic = append_shared(&f, &f.test_menu_id, group_item("dynamic", "A", "primary", None));
    append(&f, &f.test_menu_id, group_item("fixed", "B", "primary", None));
    let snapshot = || -> Vec<(String, Vec<String>)> {
        f.menu_service
            .get_menu_actions(&f.test_menu_id, &*f.context_key_service, None)
            .iter()
            .map(|(group, actions)| (group.clone(), actions.iter().map(action_label).collect()))
            .collect()
    };
    let mut snapshots = vec![snapshot()];
    edit(&dynamic, |item| item.command.title = Title::from("Z"));
    snapshots.push(snapshot());
    edit(&dynamic, |item| item.order = Some(-1.0));
    snapshots.push(snapshot());
    edit(&dynamic, |item| item.group = Some("navigation".into()));
    snapshots.push(snapshot());

    let group = |name: &str, labels: &[&str]| (name.to_owned(), strings(labels));
    assert_eq!(
        snapshots,
        vec![
            vec![group("primary", &["A", "B"])],
            vec![group("primary", &["B", "Z"])],
            vec![group("primary", &["Z", "B"])],
            vec![group("navigation", &["Z"]), group("primary", &["B"])],
        ]
    );
}

#[test]
fn get_menu_contexts_observes_changes_to_registered_expressions() {
    let f = setup();
    let mut command = CommandAction::new("command", "Command");
    command.precondition = Some(ContextKeyExpr::has("enabled"));
    command.toggled = Some(Toggled::Info(CommandActionToggleInfo { condition: ContextKeyExpr::has("toggled"), icon: None, tooltip: None, title: None, mnemonic_title: None }));
    let shared = append_shared(&f, &f.test_menu_id, MenuItem { when: Some(ContextKeyExpr::has("visible")), ..MenuItem::new(command) });
    let mut snapshots = vec![sorted_contexts(&f.menu_service, &f.test_menu_id)];
    edit(&shared, |item| {
        item.when = Some(ContextKeyExpr::has("newVisible"));
        item.command.precondition = Some(ContextKeyExpr::has("newEnabled"));
        item.command.toggled =
            Some(Toggled::Info(CommandActionToggleInfo { condition: ContextKeyExpr::has("newToggled"), icon: None, tooltip: None, title: None, mnemonic_title: None }));
    });
    snapshots.push(sorted_contexts(&f.menu_service, &f.test_menu_id));

    assert_eq!(snapshots, vec![strings(&["enabled", "toggled", "visible"]), strings(&["newEnabled", "newToggled", "newVisible"])]);
}

#[test]
fn get_menu_actions_observes_changes_to_implicit_command_palette_exclusions() {
    let f = setup();
    let prefix = format!("testo/{}", generate_uuid());
    let first = CommandAction::new(format!("{prefix}/first"), "First");
    let second = CommandAction::new(format!("{prefix}/second"), "Second");
    f.guard.add(menu_registry().add_command(first.clone()));
    f.guard.add(menu_registry().add_command(second.clone()));
    let explicit = append_shared(
        &f,
        &menu_ids::command_palette(),
        MenuItem { alt: Some(first), ..item(&format!("{prefix}/explicit"), "Explicit") },
    );
    let snapshot = || -> Vec<String> {
        f.menu_service
            .get_menu_actions(&menu_ids::command_palette(), &*f.context_key_service, None)
            .iter()
            .flat_map(|(_, actions)| actions.iter().filter(|a| action_id(a).starts_with(&prefix)).map(action_label))
            .collect()
    };
    let mut snapshots = vec![snapshot()];
    edit(&explicit, |item| item.alt = Some(second));
    snapshots.push(snapshot());

    assert_eq!(snapshots, vec![strings(&["Explicit", "Second"]), strings(&["Explicit", "First"])]);
}

#[test]
fn create_menu_still_emits_changes_for_nested_submenu_registrations() {
    let f = setup();
    run_with_faked_timers(async move {
        let submenu_id = MenuId::new(&format!("testo/{}", generate_uuid()));
        f.guard.add(menu_registry().append_menu_item(&f.test_menu_id, AnyMenuItem::Submenu(submenu_item("Submenu", &submenu_id))));
        let menu = track(&f.guard, f.menu_service.create_menu(
            &f.test_menu_id,
            f.context_key_service.clone(),
            Some(MenuCreateOptions { emit_events_for_submenu_changes: true, event_debounce_delay: Some(0) }),
        ));
        let before = menu.get_actions(None);
        let changed = menu.on_did_change().to_promise(None);
        f.guard.add(menu_registry().append_menu_item(&submenu_id, AnyMenuItem::Menu(item("nested", "Nested"))));
        let event = changed.await.unwrap();
        let groups = menu.get_actions(None);
        let MenuAction::Submenu(submenu) = &groups[0].1[0] else { panic!("devia ser um submenu") };
        let actions: Vec<String> = submenu.actions().iter().map(|a| a.id()).collect();

        assert!(before.is_empty());
        assert_eq!(actions, strings(&["nested"]));
        assert_eq!([event.is_structural_change, event.is_enablement_change, event.is_toggle_change], [true, true, true]);
    });
}

#[test]
fn get_menu_actions_reevaluates_enablement_toggled_state_and_alternate_actions() {
    let f = setup();
    let context = f.guard.add_rc(ContextKeyService::new(TestConfigurationService::new(None)));
    let enabled = context.create_key("enabled", Dynamic::Bool(true));
    let toggled = context.create_key("toggled", Dynamic::Bool(false));
    let mut command = CommandAction::new("command", "Command");
    command.precondition = Some(ContextKeyExpr::has("enabled"));
    command.toggled = Some(Toggled::Info(CommandActionToggleInfo {
        condition: ContextKeyExpr::has("toggled"),
        icon: None,
        tooltip: None,
        title: Some("Toggled".into()),
        mnemonic_title: None,
    }));
    let mut alt = CommandAction::new("alt", "Alternate");
    alt.precondition = Some(ContextKeyExpr::has("enabled"));
    f.guard.add(menu_registry().append_menu_item(&f.test_menu_id, AnyMenuItem::Menu(MenuItem { alt: Some(alt), ..MenuItem::new(command) })));
    let menu = track(&f.guard, f.menu_service.create_menu(&f.test_menu_id, context.clone(), None));
    let mut snapshots = Vec::new();

    let getters: [Box<dyn Fn() -> Vec<MenuActionGroup>>; 2] = [
        Box::new({
            let menu = menu.clone();
            move || menu.get_actions(None)
        }),
        Box::new({
            let (service, id, context) = (f.menu_service.clone(), f.test_menu_id.clone(), context.clone());
            move || service.get_menu_actions(&id, &*context, None)
        }),
    ];
    for get_actions in &getters {
        enabled.set(Dynamic::Bool(true));
        toggled.set(Dynamic::Bool(false));
        for _ in 0..2 {
            let groups = get_actions();
            let MenuAction::Item(action) = &groups[0].1[0] else { panic!("não devia ser um submenu") };
            snapshots.push((action.enabled(), action.checked(), action.label(), action.alt().map(|a| a.enabled())));
            enabled.set(Dynamic::Bool(false));
            toggled.set(Dynamic::Bool(true));
        }
    }

    let expected = [(true, Some(false), "Command".to_owned(), Some(true)), (false, Some(true), "Toggled".to_owned(), Some(false))];
    assert_eq!(snapshots, [expected.clone(), expected].concat());
}

#[test]
fn get_menu_actions_preserves_per_call_options_and_current_hidden_state() {
    run_with_faked_timers(async {
        let f = setup();
        let mut command = CommandAction::new("command", "Long title");
        command.short_title = Some(Title::from("Short"));
        f.guard.add(menu_registry().append_menu_item(&f.test_menu_id, AnyMenuItem::Menu(MenuItem { is_hidden_by_default: Some(true), ..MenuItem::new(command) })));
        let first_options = MenuActionOptions { render_short_title: true, args: Some(vec![Dynamic::string("first")]), should_forward_args: true, ..MenuActionOptions::default() };
        let first = f.menu_service.get_menu_actions(&f.test_menu_id, &*f.context_key_service, Some(&first_options)).remove(0).1.remove(0);
        let MenuAction::Item(first) = first else { panic!("não devia ser um submenu") };
        let first_hide = first.hide_actions.clone().expect("tem ação de esconder");
        let mut hidden = vec![(first_hide.is_hidden)()];
        first_hide.toggle.run(&[]).await.unwrap();
        let second_options = MenuActionOptions { arg: Some(Dynamic::string("second")), ..MenuActionOptions::default() };
        let second = f.menu_service.get_menu_actions(&f.test_menu_id, &*f.context_key_service, Some(&second_options)).remove(0).1.remove(0);
        let MenuAction::Item(second) = second else { panic!("não devia ser um submenu") };
        hidden.push((second.hide_actions.clone().expect("tem ação de esconder").is_hidden)());
        f.menu_service.reset_hidden_states(Some(&[f.test_menu_id.clone()]));
        let third = f.menu_service.get_menu_actions(&f.test_menu_id, &*f.context_key_service, None).remove(0).1.remove(0);
        let MenuAction::Item(third) = third else { panic!("não devia ser um submenu") };
        hidden.push((third.hide_actions.clone().expect("tem ação de esconder").is_hidden)());
        first.run(&[Dynamic::string("forwarded")]).await.unwrap();
        second.run(&[Dynamic::string("ignored")]).await.unwrap();

        assert_eq!([first.label(), second.label()], ["Short".to_owned(), "Long title".to_owned()]);
        assert_eq!(hidden, [true, false, true]);
        assert_eq!(*f.commands.calls.borrow(), vec![strings(&["command", "first", "forwarded"]), strings(&["command", "second"])]);
    });
}

fn group_item(id: &str, title: &str, group: &str, order: Option<f64>) -> MenuItem {
    MenuItem { group: Some(group.to_owned()), order, ..item(id, title) }
}

#[test]
fn group_sorting() {
    let f = setup();
    append(&f, &f.test_menu_id, group_item("one", "FOO", "0_hello", None));
    append(&f, &f.test_menu_id, group_item("two", "FOO", "hello", None));
    append(&f, &f.test_menu_id, group_item("three", "FOO", "Hello", None));
    append(&f, &f.test_menu_id, group_item("four", "FOO", "", None));
    append(&f, &f.test_menu_id, group_item("five", "FOO", "navigation", None));

    let menu = track(&f.guard, f.menu_service.create_menu(&f.test_menu_id, f.context_key_service.clone(), None));
    let groups = menu.get_actions(None);

    assert_eq!(groups.len(), 5);
    let names: Vec<&str> = groups.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["navigation", "0_hello", "hello", "Hello", ""]);
}

#[test]
fn in_group_sorting_by_title() {
    let f = setup();
    append(&f, &f.test_menu_id, group_item("a", "aaa", "Hello", None));
    append(&f, &f.test_menu_id, group_item("b", "fff", "Hello", None));
    append(&f, &f.test_menu_id, group_item("c", "zzz", "Hello", None));

    let menu = track(&f.guard, f.menu_service.create_menu(&f.test_menu_id, f.context_key_service.clone(), None));
    let groups = menu.get_actions(None);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].1.len(), 3);
    assert_eq!(groups[0].1.iter().map(action_id).collect::<Vec<_>>(), ["a", "b", "c"]);
}

#[test]
fn in_group_sorting_by_title_and_order() {
    let f = setup();
    append(&f, &f.test_menu_id, group_item("a", "aaa", "Hello", Some(10.0)));
    append(&f, &f.test_menu_id, group_item("b", "fff", "Hello", None));
    append(&f, &f.test_menu_id, group_item("c", "zzz", "Hello", Some(-1.0)));
    append(&f, &f.test_menu_id, group_item("d", "yyy", "Hello", Some(-1.0)));

    let menu = track(&f.guard, f.menu_service.create_menu(&f.test_menu_id, f.context_key_service.clone(), None));
    let groups = menu.get_actions(None);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].1.len(), 4);
    assert_eq!(groups[0].1.iter().map(action_id).collect::<Vec<_>>(), ["d", "c", "b", "a"]);
}

#[test]
fn in_group_sorting_special_navigation() {
    let f = setup();
    append(&f, &f.test_menu_id, group_item("a", "aaa", "navigation", Some(1.3)));
    append(&f, &f.test_menu_id, group_item("b", "fff", "navigation", Some(1.2)));
    append(&f, &f.test_menu_id, group_item("c", "zzz", "navigation", Some(1.1)));

    let menu = track(&f.guard, f.menu_service.create_menu(&f.test_menu_id, f.context_key_service.clone(), None));
    let groups = menu.get_actions(None);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].1.len(), 3);
    assert_eq!(groups[0].1.iter().map(action_id).collect::<Vec<_>>(), ["c", "b", "a"]);
}

#[test]
fn special_menu_id_palette() {
    let f = setup();
    f.guard.add(menu_registry().append_menu_item(&menu_ids::command_palette(), AnyMenuItem::Menu(item("a", "Explicit"))));
    f.guard.add(menu_registry().add_command(CommandAction::new("b", "Implicit")));

    let (mut found_a, mut found_b) = (false, false);
    for entry in menu_registry().get_menu_items(&menu_ids::command_palette()) {
        if let AnyMenuItem::Menu(entry) = entry {
            if entry.command.id == "a" {
                assert_eq!(entry.command.title, Title::from("Explicit"));
                found_a = true;
            }
            if entry.command.id == "b" {
                assert_eq!(entry.command.title, Title::from("Implicit"));
                found_b = true;
            }
        }
    }
    assert!(found_a);
    assert!(found_b);
}

#[test]
fn extension_contributed_submenus_missing_with_errors_in_output_155030() {
    let _f = setup();
    let id = generate_uuid();
    let menu = MenuId::new(&id);

    assert!(je_base::errors::catch(|| MenuId::new(&id)).is_err());
    assert_eq!(menu, MenuId::for_id(&id));
}
