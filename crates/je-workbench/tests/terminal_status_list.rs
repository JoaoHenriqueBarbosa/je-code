//! Porte de `src/vs/workbench/contrib/terminal/test/browser/terminalStatusList.test.ts` (suíte `Workbench - TerminalStatusList`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo e a
//!   `TerminalStatusList` (o `createInstance` do original injeta o `IConfigurationService`, que é o `TestConfigurationService`
//!   vazio do `workbenchInstantiationService`), registrada no store.
//! - **`list.add({ id, severity })`.** O objeto literal é um `Rc<ITerminalStatus>` novo ([`status`]); a identidade que o
//!   original compara por referência é a do `Rc`.
//! - **`list.remove('id')`.** É `remove_id`.
//! - **`deepStrictEqual(result, { id: 'test', severity: Severity.Info })`.** O `ITerminalStatus` do porte tem um `Rc<dyn Fn>`
//!   nas ações de hover e não é comparável por valor: [`assert_status`] compara cada campo do literal (os opcionais ausentes
//!   são `None`).
//! - **`new Promise(r => { store.add(list.onDidX(r)); ... })`.** Os eventos disparam de forma síncrona dentro do executor;
//!   o ouvinte guarda o valor e o teste o lê depois.
//! - **`Codicon.check`, `Codicon.play`, `Codicon.zap` e `spinningLoading`.** São `ThemeIcon::from_id` dos ids `check`, `play`,
//!   `zap` e `loading~spin`; `ThemeIcon.modify(Codicon.zap, 'spin')` é `ThemeIcon::modify` com `IconModifier::Spin`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::severity::Severity;
use je_base::themables::{IconModifier, ThemeIcon};
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::contrib::terminal::browser::terminal_status_list::{ITerminalStatusList, TerminalStatusList};
use je_workbench::contrib::terminal::common::terminal::ITerminalStatus;

fn status(id: &str, severity: Severity) -> Rc<ITerminalStatus> {
    Rc::new(ITerminalStatus { id: id.to_owned(), severity, icon: None, tooltip: None, detailed_tooltip: None, hover_actions: None })
}

fn status_with_icon(id: &str, severity: Severity, icon: ThemeIcon) -> Rc<ITerminalStatus> {
    Rc::new(ITerminalStatus { id: id.to_owned(), severity, icon: Some(icon), tooltip: None, detailed_tooltip: None, hover_actions: None })
}

/// `deepStrictEqual(status, { id, severity, icon })`.
#[track_caller]
fn assert_status(actual: &ITerminalStatus, id: &str, severity: Severity, icon: Option<&ThemeIcon>) {
    assert_eq!(actual.id, id);
    assert_eq!(actual.severity, severity);
    assert_eq!(actual.icon.as_ref(), icon);
    assert!(actual.tooltip.is_none());
    assert!(actual.detailed_tooltip.is_none());
    assert!(actual.hover_actions.is_none());
}

fn primary_id(list: &TerminalStatusList) -> Option<String> {
    list.primary().map(|primary| primary.id.clone())
}

fn statuses_equal(list: &TerminalStatusList, expected: &[(&str, Severity)]) {
    let actual: Vec<(String, Severity)> = list.statuses().iter().map(|e| (e.id.clone(), e.severity)).collect();
    let expected: Vec<(String, Severity)> = expected.iter().map(|(id, severity)| ((*id).to_owned(), *severity)).collect();
    assert_eq!(actual, expected);
}

struct Fixture {
    list: Rc<TerminalStatusList>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    store: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
        let list = store.add_rc(TerminalStatusList::create((), (configuration_service,)));
        Self { list, store }
    }
}

#[test]
fn primary() {
    let f = Fixture::new();
    let list = &f.list;
    assert_eq!(primary_id(list), None);
    list.add(status("info1", Severity::Info), None);
    assert_eq!(primary_id(list).as_deref(), Some("info1"));
    list.add(status("warning1", Severity::Warning), None);
    assert_eq!(primary_id(list).as_deref(), Some("warning1"));
    list.add(status("info2", Severity::Info), None);
    assert_eq!(primary_id(list).as_deref(), Some("warning1"));
    list.add(status("warning2", Severity::Warning), None);
    assert_eq!(primary_id(list).as_deref(), Some("warning2"));
    list.add(status("info3", Severity::Info), None);
    assert_eq!(primary_id(list).as_deref(), Some("warning2"));
    list.add(status("error1", Severity::Error), None);
    assert_eq!(primary_id(list).as_deref(), Some("error1"));
    list.add(status("warning3", Severity::Warning), None);
    assert_eq!(primary_id(list).as_deref(), Some("error1"));
    list.add(status("error2", Severity::Error), None);
    assert_eq!(primary_id(list).as_deref(), Some("error2"));
    list.remove_id("error1");
    assert_eq!(primary_id(list).as_deref(), Some("error2"));
    list.remove_id("error2");
    assert_eq!(primary_id(list).as_deref(), Some("warning3"));
}

#[test]
fn statuses() {
    let f = Fixture::new();
    let list = &f.list;
    assert_eq!(list.statuses().len(), 0);
    list.add(status("info", Severity::Info), None);
    list.add(status("warning", Severity::Warning), None);
    list.add(status("error", Severity::Error), None);
    assert_eq!(list.statuses().len(), 3);
    statuses_equal(list, &[("info", Severity::Info), ("warning", Severity::Warning), ("error", Severity::Error)]);
    list.remove_id("info");
    list.remove_id("warning");
    list.remove_id("error");
    assert_eq!(list.statuses().len(), 0);
}

#[test]
fn on_did_add_status() {
    let f = Fixture::new();
    let result: Rc<RefCell<Option<Rc<ITerminalStatus>>>> = Rc::default();
    let sink = result.clone();
    f.store.add(f.list.on_did_add_status().listen(move |status| *sink.borrow_mut() = Some(status.clone())));
    f.list.add(status("test", Severity::Info), None);
    let result = result.borrow().clone().expect("o evento disparou");
    assert_status(&result, "test", Severity::Info, None);
}

#[test]
fn on_did_remove_status() {
    let f = Fixture::new();
    let result: Rc<RefCell<Option<Rc<ITerminalStatus>>>> = Rc::default();
    let sink = result.clone();
    f.store.add(f.list.on_did_remove_status().listen(move |status| *sink.borrow_mut() = Some(status.clone())));
    f.list.add(status("test", Severity::Info), None);
    f.list.remove_id("test");
    let result = result.borrow().clone().expect("o evento disparou");
    assert_status(&result, "test", Severity::Info, None);
}

#[test]
fn on_did_change_primary_status() {
    let f = Fixture::new();
    let result: Rc<RefCell<Option<Option<Rc<ITerminalStatus>>>>> = Rc::default();
    let sink = result.clone();
    f.store.add(f.list.on_did_change_primary_status().listen(move |status| *sink.borrow_mut() = Some(status.clone())));
    f.list.add(status("test", Severity::Info), None);
    let result = result.borrow().clone().expect("o evento disparou").expect("o status principal existe");
    assert_status(&result, "test", Severity::Info, None);
}

#[test]
fn primary_is_not_updated_to_status_without_an_icon() {
    let f = Fixture::new();
    let check = ThemeIcon::from_id("check");
    f.list.add(status_with_icon("test", Severity::Info, check.clone()), None);
    f.list.add(status("warning", Severity::Warning), None);
    let primary = f.list.primary().expect("existe um status principal");
    assert_status(&primary, "test", Severity::Info, Some(&check));
}

#[test]
fn add() {
    let f = Fixture::new();
    let list = &f.list;
    statuses_equal(list, &[]);
    list.add(status("info", Severity::Info), None);
    statuses_equal(list, &[("info", Severity::Info)]);
    list.add(status("warning", Severity::Warning), None);
    statuses_equal(list, &[("info", Severity::Info), ("warning", Severity::Warning)]);
    list.add(status("error", Severity::Error), None);
    statuses_equal(list, &[("info", Severity::Info), ("warning", Severity::Warning), ("error", Severity::Error)]);
}

#[test]
fn add_should_remove_animation() {
    let f = Fixture::new();
    let list = &f.list;
    statuses_equal(list, &[]);
    list.add(status_with_icon("info", Severity::Info, ThemeIcon::from_id("loading~spin")), None);
    statuses_equal(list, &[("info", Severity::Info)]);
    assert_eq!(list.statuses()[0].icon.as_ref().unwrap().id, ThemeIcon::from_id("play").id, "loading~spin should be converted to play");
    list.add(status_with_icon("warning", Severity::Warning, ThemeIcon::modify(&ThemeIcon::from_id("zap"), Some(IconModifier::Spin))), None);
    statuses_equal(list, &[("info", Severity::Info), ("warning", Severity::Warning)]);
    assert_eq!(list.statuses()[1].icon.as_ref().unwrap().id, ThemeIcon::from_id("zap").id, "zap~spin should have animation removed only");
}

#[test]
fn add_should_fire_on_did_remove_status_if_same_status_id_with_a_different_object_reference_was_added() {
    let f = Fixture::new();
    let event_calls: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let sink = event_calls.clone();
    f.store.add(f.list.on_did_add_status().listen(move |_| sink.borrow_mut().push("add")));
    let sink = event_calls.clone();
    f.store.add(f.list.on_did_remove_status().listen(move |_| sink.borrow_mut().push("remove")));
    f.list.add(status("test", Severity::Info), None);
    f.list.add(status("test", Severity::Info), None);
    assert_eq!(*event_calls.borrow(), vec!["add", "remove", "add"]);
}

#[test]
fn remove() {
    let f = Fixture::new();
    let list = &f.list;
    list.add(status("info", Severity::Info), None);
    list.add(status("warning", Severity::Warning), None);
    list.add(status("error", Severity::Error), None);
    statuses_equal(list, &[("info", Severity::Info), ("warning", Severity::Warning), ("error", Severity::Error)]);
    list.remove_id("warning");
    statuses_equal(list, &[("info", Severity::Info), ("error", Severity::Error)]);
    list.remove_id("info");
    statuses_equal(list, &[("error", Severity::Error)]);
    list.remove_id("error");
    statuses_equal(list, &[]);
}

#[test]
fn toggle() {
    let f = Fixture::new();
    let list = &f.list;
    let info = status("info", Severity::Info);
    list.toggle(info.clone(), true);
    statuses_equal(list, &[("info", Severity::Info)]);
    list.toggle(info, false);
    statuses_equal(list, &[]);
}
