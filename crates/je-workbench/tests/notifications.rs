//! Porte de `src/vs/workbench/test/common/notifications.test.ts` (suíte `Notifications`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `disposables`
//!   do original (um `DisposableStore` limpo no `teardown`) é o `add` do guarda.
//! - `createItem` registra um `toDisposable(() => item.close())`, como o original.
//! - Os valores que o TypeScript aceita em tempo de execução e o tipo do porte já recusa não são
//!   representáveis. No segundo caso, `legacyExtensionLinkParsing` com `true`, `{}` e um `Symbol` qualquer
//!   (que o original passa com `@ts-expect-error`) não existe: o único valor possível é a capacidade
//!   (`LEGACY_EXTENSION_LINK_PARSING`) ou nenhum, e o caso confere que sem a capacidade os três itens ficam
//!   literais. No caso `Items`, `message: null!` também não existe (a mensagem é obrigatória); fica a mensagem
//!   vazia.
//! - `NotificationText.format('Cannot open {0}. {1}', filename, link)` recebe os argumentos como
//!   [`NotificationPart`].
//! - `await timeout(0)` do caso assíncrono é `run_with_faked_timers` com o `timeout(0)` do relógio virtual.
//! - `assert.deepStrictEqual` de objetos e arrays é a comparação de tupla, `Vec` ou struct com `PartialEq`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::actions::{Action, ActionRef};
use je_base::error_message::create_error_with_actions;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::to_disposable;
use je_base::linked_text::{Link, LinkedTextNode};
use je_base::map::OrderedMap;
use je_platform::notification::notification_legacy::LEGACY_EXTENSION_LINK_PARSING;
use je_platform::notification::notification_message::{NotificationPart, NotificationText};
use je_platform::notification::{
    Notification, NotificationActions, NotificationPriority, NotificationProgress as _, NotificationProgressProperties, NotificationProperties,
    NotificationSource, NotificationSourceOrText, NotificationsFilter, Severity, 
};
use je_workbench::common::notifications::{
    INotificationsModel, NotificationChangeEvent, NotificationChangeType, NotificationViewItem, NotificationViewItemContentChangeKind, NotificationsFilterState,
    NotificationsModel, StatusMessageChangeEvent, StatusMessageChangeType,
};

fn no_filter() -> NotificationsFilterState {
    NotificationsFilterState { global: NotificationsFilter::Off, sources: OrderedMap::new() }
}

fn create_item(guard: &LeakGuard, notification: Notification) -> Rc<NotificationViewItem> {
    let item = NotificationViewItem::create(&notification, &no_filter()).expect("a notificação tem mensagem");
    let closing = item.clone();
    guard.add(to_disposable(move || closing.close()));
    item
}

fn text_node(text: &str) -> LinkedTextNode {
    LinkedTextNode::Text(JsString::from(text))
}

fn link_node(label: &str, href: &str, title: Option<&str>) -> LinkedTextNode {
    LinkedTextNode::Link(Link { label: JsString::from(label), href: JsString::from(href), title: title.map(JsString::from) })
}

fn nodes_of(item: &NotificationViewItem) -> Vec<LinkedTextNode> {
    item.message().linked_text.nodes.clone()
}

/// `item.message.linkedText.toString()`.
fn displayed(item: &NotificationViewItem) -> String {
    item.message().linked_text.to_js_string().to_string_lossy()
}

fn legacy_notification(message: &str) -> Notification {
    Notification {
        properties: NotificationProperties { legacy_extension_link_parsing: Some(LEGACY_EXTENSION_LINK_PARSING), ..NotificationProperties::default() },
        ..Notification::new(Severity::Info, message)
    }
}

fn action(guard: &LeakGuard, id: &str) -> ActionRef {
    let action: Rc<Action> = guard.add_rc(Action::new(id, "label", "", true, None));
    action
}

fn with_primary_actions(notification: Notification, primary: Vec<ActionRef>) -> Notification {
    Notification { actions: Some(NotificationActions { primary, secondary: Vec::new() }), ..notification }
}

/// Registra no guarda um listener que soma em `called` cada evento que passa no `filter`.
fn count<T: 'static>(guard: &LeakGuard, event: Event<T>, called: &Rc<Cell<u32>>, filter: impl Fn(&T) -> bool + 'static) {
    let called = called.clone();
    guard.add(event.listen(move |e| {
        if filter(e) {
            called.set(called.get() + 1);
        }
    }));
}

/// `await timeout(0)`.
fn wait_for_timeout() {
    run_with_faked_timers(async {
        timeout(0).await.expect("o timeout assenta");
    });
}

#[test]
fn renders_strings_and_errors_literally_by_default() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let message = "README.md [Open](command:workbench.action.files.newUntitledFile \"Open README\") [Help](https://example.com) <b>text</b>";
    let items = [
        create_item(&guard, Notification::new(Severity::Info, message)),
        create_item(&guard, Notification::new(Severity::Error, Error::new(message))),
        create_item(&guard, Notification::new(Severity::Warning, message)),
    ];

    for item in &items {
        assert_eq!(item.message().raw, message);
        assert_eq!(nodes_of(item), vec![text_node(message)]);
    }
}

#[test]
fn does_not_accept_boolean_or_lookalike_legacy_parsing_capabilities_at_runtime() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let message = "[Open](command:unexpected)";
    let nodes: Vec<Vec<LinkedTextNode>> =
        (0..3).map(|_| nodes_of(&create_item(&guard, Notification::new(Severity::Info, message)))).collect();
    assert_eq!(nodes, vec![vec![text_node(message)], vec![text_node(message)], vec![text_node(message)]]);
}

#[test]
fn preserves_explicit_links_without_interpreting_literal_fragments_or_link_labels() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let filename = "[Open](command:unexpected)";
    let (label, href, title) = (filename, "command:showLogs?%5B%22file%22%5D", "Show Logs");
    let message = NotificationText::format(
        "Cannot open {0}. {1}",
        &[NotificationPart::from(filename), NotificationPart::from(NotificationText::link(label, href, Some(title)))],
    );
    let item = create_item(&guard, Notification::new(Severity::Error, message));

    assert_eq!(item.message().raw, format!("Cannot open {filename}. {filename}"));
    assert_eq!(nodes_of(&item), vec![text_node(&format!("Cannot open {filename}. ")), link_node(label, href, Some(title))]);
}

#[test]
fn updating_a_structured_message_with_plain_text_removes_links_even_when_the_text_is_unchanged() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let item = create_item(&guard, Notification::new(Severity::Info, NotificationText::link("Details", "command:showDetails", None)));
    let changes: Rc<RefCell<Vec<NotificationViewItemContentChangeKind>>> = Rc::default();
    let recorded = changes.clone();
    guard.add(item.on_did_change_content().listen(move |event| recorded.borrow_mut().push(event.kind)));

    item.update_message("Details".into());
    item.update_message("Details".into());

    assert_eq!(nodes_of(&item), vec![text_node("Details")]);
    assert_eq!(*changes.borrow(), vec![NotificationViewItemContentChangeKind::Message]);
}

#[test]
fn updates_link_targets_without_requiring_a_label_change() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let item = create_item(&guard, Notification::new(Severity::Info, NotificationText::link("Details", "command:first", None)));
    let changes: Rc<RefCell<Vec<NotificationViewItemContentChangeKind>>> = Rc::default();
    let recorded = changes.clone();
    guard.add(item.on_did_change_content().listen(move |event| recorded.borrow_mut().push(event.kind)));

    item.update_message(NotificationText::link("Details", "command:second", None).into());
    item.update_message(NotificationText::link("Details", "command:second", None).into());

    assert_eq!(nodes_of(&item), vec![link_node("Details", "command:second", None)]);
    assert_eq!(*changes.borrow(), vec![NotificationViewItemContentChangeKind::Message]);
}

#[test]
fn does_not_deduplicate_literal_text_and_different_links_with_the_same_label() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let literal = create_item(&guard, Notification::new(Severity::Info, "Details"));
    let first = create_item(&guard, Notification::new(Severity::Info, NotificationText::link("Details", "command:first", None)));
    let duplicate = create_item(&guard, Notification::new(Severity::Info, NotificationText::link("Details", "command:first", None)));
    let second = create_item(&guard, Notification::new(Severity::Info, NotificationText::link("Details", "command:second", None)));

    assert_eq!([literal.equals(&first), first.equals(&second), first.equals(&duplicate)], [false, false, true]);
}

#[test]
fn preserves_legacy_link_parsing_and_updates_with_the_extension_capability() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let item = create_item(&guard, legacy_notification("  [Show Logs](command:python.viewOutput)\r\n[Help](https://example.com)  "));
    let initial = nodes_of(&item);
    item.update_message("[Check details](command:java.show.server.task.status \"Build Status\")".into());

    assert_eq!(initial, vec![link_node("Show Logs", "command:python.viewOutput", None), text_node(" "), link_node("Help", "https://example.com", None)]);
    assert_eq!(nodes_of(&item), vec![link_node("Check details", "command:java.show.server.task.status", Some("Build Status"))]);
}

#[test]
fn normalizes_and_truncates_messages_while_preserving_the_original_text() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let message = format!("  line one\r\nline two\rline three\n{}", "x".repeat(1000));
    let item = create_item(&guard, Notification::new(Severity::Info, message.as_str()));
    let legacy = create_item(&guard, legacy_notification(&format!("{}[Open](command:unexpected)", "x".repeat(995))));

    let first_thousand: String = message.chars().take(1000).collect();
    let expected_displayed = format!("{}...", first_thousand.replace("\r\n", " ").replace(['\n', '\r'], " ").trim_start());
    assert_eq!(item.message().raw, message);
    assert_eq!(displayed(&item), expected_displayed);
    assert_eq!(nodes_of(&legacy), vec![text_node(&format!("{}[Open...", "x".repeat(995)))]);
}

#[test]
fn truncates_structured_labels_without_changing_link_targets() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let href = format!("command:showLogs?{}", "x".repeat(1100));
    let item = create_item(
        &guard,
        Notification::new(
            Severity::Info,
            NotificationText::concat([NotificationPart::from("x".repeat(999)), NotificationPart::from(NotificationText::link("Details", &href, None))]),
        ),
    );

    assert_eq!(nodes_of(&item), vec![text_node(&"x".repeat(999)), link_node("D", &href, None), text_node("...")]);
}

#[test]
fn shows_structured_status_messages_as_plain_text() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = guard.add_rc(NotificationsModel::new());
    let message = NotificationText::concat([NotificationPart::from("Open "), NotificationPart::from(NotificationText::link("logs", "command:showLogs", None))]);
    let handle = model.show_status_message(message.into(), None);
    guard.add(to_disposable(move || handle.close()));

    assert_eq!(model.status_message().map(|status| status.message.clone()).as_deref(), Some("Open logs"));
}

#[test]
fn items() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let no_filter = no_filter();
    let create = |notification: Notification| NotificationViewItem::create(&notification, &no_filter).expect("a notificação tem mensagem");

    // Invalid
    assert!(NotificationViewItem::create(&Notification::new(Severity::Error, ""), &no_filter).is_none());

    // Duplicates
    let item1 = create(Notification::new(Severity::Error, "Error Message"));
    let item2 = create(Notification::new(Severity::Error, "Error Message"));
    let item3 = create(Notification::new(Severity::Info, "Info Message"));
    let item4 = create(Notification { source: Some(NotificationSourceOrText::Text("Source".into())), ..Notification::new(Severity::Error, "Error Message") });
    let item5 = create(with_primary_actions(Notification::new(Severity::Error, "Error Message"), vec![action(&guard, "id")]));
    let item6 = create(Notification {
        progress: Some(NotificationProgressProperties { infinite: true, ..NotificationProgressProperties::default() }),
        ..with_primary_actions(Notification::new(Severity::Error, "Error Message"), vec![action(&guard, "id")])
    });

    assert!(item1.equals(&item1));
    assert!(item2.equals(&item2));
    assert!(item3.equals(&item3));
    assert!(item4.equals(&item4));
    assert!(item5.equals(&item5));

    assert!(item1.equals(&item2));
    assert!(!item1.equals(&item3));
    assert!(!item1.equals(&item4));
    assert!(!item1.equals(&item5));

    let item_id1 = create(Notification { id: Some("same".into()), ..Notification::new(Severity::Info, "Info Message") });
    let item_id2 = create(Notification { id: Some("same".into()), ..Notification::new(Severity::Error, "Error Message") });

    assert!(item_id1.equals(&item_id2));
    assert!(!item_id1.equals(&item3));

    // Progress
    assert!(!item1.has_progress());
    assert!(!item1.has_active_progress());
    assert!(item6.has_progress());
    assert!(item6.has_active_progress());

    // Message Box
    assert!(!item5.can_collapse());
    assert!(item5.expanded());

    // Events
    let called = Rc::new(Cell::new(0));
    count(&guard, item1.on_did_change_expansion(), &called, |_| true);

    item1.expand();
    item1.expand();
    item1.collapse(false);
    item1.collapse(false);

    assert_eq!(called.get(), 2);

    called.set(0);
    count(&guard, item1.on_did_change_content(), &called, |e| e.kind == NotificationViewItemContentChangeKind::Progress);

    {
        item1.progress().infinite();
        item1.progress().done();
    }

    assert_eq!(called.get(), 2);

    called.set(0);
    count(&guard, item1.on_did_change_content(), &called, |e| e.kind == NotificationViewItemContentChangeKind::Message);

    item1.update_message("message update".into());

    called.set(0);
    count(&guard, item1.on_did_change_content(), &called, |e| e.kind == NotificationViewItemContentChangeKind::Severity);

    item1.update_severity(Severity::Error);

    called.set(0);
    count(&guard, item1.on_did_change_content(), &called, |e| e.kind == NotificationViewItemContentChangeKind::Actions);

    item1.update_actions(Some(NotificationActions { primary: vec![action(&guard, "id2")], secondary: Vec::new() }));

    assert_eq!(called.get(), 1);

    called.set(0);
    count(&guard, item1.on_did_change_visibility(), &called, |_| true);

    item1.update_visibility(true);
    item1.update_visibility(false);
    item1.update_visibility(false);

    assert_eq!(called.get(), 2);

    called.set(0);
    count(&guard, item1.on_did_close(), &called, |_| true);

    item1.close();
    assert_eq!(called.get(), 1);

    // Error with Action
    let item7 = create(Notification::new(Severity::Error, create_error_with_actions("Hello Error".into(), vec![action(&guard, "id")])));
    assert_eq!(item7.actions().primary.len(), 1);

    // Filter
    let item8 = NotificationViewItem::create(&Notification::new(Severity::Error, "Error Message"), &NotificationsFilterState { global: NotificationsFilter::Off, sources: OrderedMap::new() })
        .expect("a notificação tem mensagem");
    assert_eq!(item8.priority(), NotificationPriority::Default);

    let item9 = NotificationViewItem::create(&Notification::new(Severity::Error, "Error Message"), &NotificationsFilterState { global: NotificationsFilter::Error, sources: OrderedMap::new() })
        .expect("a notificação tem mensagem");
    assert_eq!(item9.priority(), NotificationPriority::Default);

    let item10 = NotificationViewItem::create(&Notification::new(Severity::Warning, "Error Message"), &NotificationsFilterState { global: NotificationsFilter::Error, sources: OrderedMap::new() })
        .expect("a notificação tem mensagem");
    assert_eq!(item10.priority(), NotificationPriority::Silent);

    let mut sources = OrderedMap::new();
    sources.insert("test.source".to_string(), NotificationsFilter::Error);
    let source_filter = NotificationsFilterState { global: NotificationsFilter::Off, sources };
    let create_filtered = |source: NotificationSourceOrText| {
        NotificationViewItem::create(&Notification { source: Some(source), ..Notification::new(Severity::Warning, "Error Message") }, &source_filter)
            .expect("a notificação tem mensagem")
    };
    let item11 = create_filtered(NotificationSourceOrText::Text("test.source".into()));
    assert_eq!(item11.priority(), NotificationPriority::Default);
    let item12 = create_filtered(NotificationSourceOrText::Source(NotificationSource { id: "test.source".into(), label: "foo".into() }));
    assert_eq!(item12.priority(), NotificationPriority::Silent);
    let item13 = create_filtered(NotificationSourceOrText::Source(NotificationSource { id: "test.source2".into(), label: "foo".into() }));
    assert_eq!(item13.priority(), NotificationPriority::Default);

    for item in [item1, item2, item3, item4, item5, item6, item_id1, item_id2, item7, item8, item9, item10, item11, item12, item13] {
        item.close();
    }
}

#[test]
fn progress_activity_tracks_starts_updates_completion_and_restart() {
    #[derive(Debug, PartialEq)]
    struct State {
        has_progress: bool,
        has_active_progress: bool,
        sticky: bool,
        active_progress_changed: Option<bool>,
    }

    fn capture_state(item: &NotificationViewItem, states: &RefCell<Vec<State>>, active_progress_changed: Option<bool>) {
        states.borrow_mut().push(State {
            has_progress: item.has_progress(),
            has_active_progress: item.has_active_progress(),
            sticky: item.sticky(),
            active_progress_changed,
        });
    }

    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let item = NotificationViewItem::create(&Notification::new(Severity::Info, "Progress"), &no_filter()).expect("a notificação tem mensagem");
    let states: Rc<RefCell<Vec<State>>> = Rc::default();
    let (observed, recorded) = (Rc::downgrade(&item), states.clone());
    guard.add(item.on_did_change_content().listen(move |e| {
        if e.kind == NotificationViewItemContentChangeKind::Progress
            && let Some(item) = observed.upgrade()
        {
            capture_state(&item, &recorded, e.active_progress_changed);
        }
    }));

    capture_state(&item, &states, None);
    let progress = item.progress();
    capture_state(&item, &states, None);
    progress.infinite();
    progress.total(100.0);
    progress.worked(10.0);
    progress.done();
    progress.infinite();

    let state = |has_active_progress: bool, active_progress_changed: Option<bool>| State {
        has_progress: true,
        has_active_progress,
        sticky: has_active_progress,
        active_progress_changed,
    };
    assert_eq!(
        *states.borrow(),
        vec![
            State { has_progress: false, has_active_progress: false, sticky: false, active_progress_changed: None },
            state(false, None),
            state(true, Some(true)),
            state(true, Some(false)),
            state(true, Some(false)),
            state(false, Some(true)),
            state(true, Some(true)),
        ]
    );

    item.close();
}

#[test]
fn completed_progress_notifications_remain_unique() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = guard.add_rc(NotificationsModel::new());
    let first = model.add_notification(Notification::new(Severity::Info, "Same message"));
    let first_closed = Rc::new(Cell::new(false));
    let closed = first_closed.clone();
    guard.add(first.on_did_close().listen(move |_| closed.set(true)));

    first.progress().infinite();
    first.progress().done();
    let second = model.add_notification(Notification::new(Severity::Info, "Same message"));

    assert_eq!((model.notifications().len(), first_closed.get()), (2, false));

    first.close();
    second.close();
}

#[test]
fn items_does_not_fire_changed_when_message_did_not_change_content_severity() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let item1 = NotificationViewItem::create(&Notification::new(Severity::Error, "Error Message"), &no_filter()).expect("a notificação tem mensagem");

    let fired = Rc::new(Cell::new(0));
    count(&guard, item1.on_did_change_content(), &fired, |_| true);

    item1.update_message("Error Message".into());
    wait_for_timeout();
    assert_eq!(fired.get(), 0, "Expected onDidChangeContent to not be fired");

    item1.update_severity(Severity::Error);
    wait_for_timeout();
    assert_eq!(fired.get(), 0, "Expected onDidChangeContent to not be fired");

    item1.close();
}

#[test]
fn model() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = guard.add_rc(NotificationsModel::new());

    let last_notification_event: Rc<RefCell<Option<NotificationChangeEvent>>> = Rc::default();
    let last = last_notification_event.clone();
    guard.add(model.on_did_change_notification().listen(move |e| *last.borrow_mut() = Some(e.clone())));
    let last_event = || last_notification_event.borrow().clone().expect("houve um evento de notificação");

    let last_status_message_event: Rc<RefCell<Option<StatusMessageChangeEvent>>> = Rc::default();
    let last = last_status_message_event.clone();
    guard.add(model.on_did_change_status_message().listen(move |e| *last.borrow_mut() = Some(e.clone())));
    let last_status_event = || last_status_message_event.borrow().clone().expect("houve um evento de mensagem de status");

    let item1 = with_primary_actions(Notification::new(Severity::Error, "Error Message"), vec![action(&guard, "id")]);
    let item2 = Notification { source: Some(NotificationSourceOrText::Text("Some Source".into())), ..Notification::new(Severity::Warning, "Warning Message") };
    let item2_duplicate = item2.clone();
    let item3 = Notification::new(Severity::Info, "Info Message");

    let item1_handle = model.add_notification(item1.clone());
    assert_eq!(last_event().item.severity(), item1.severity);
    assert_eq!(displayed(&last_event().item), "Error Message");
    assert_eq!(last_event().index, 0);
    assert_eq!(last_event().kind, NotificationChangeType::Add);

    item1_handle.update_message("Different Error Message".into());
    assert_eq!(last_event().kind, NotificationChangeType::Change);
    assert_eq!(last_event().detail, Some(NotificationViewItemContentChangeKind::Message));

    item1_handle.update_severity(Severity::Warning);
    assert_eq!(last_event().kind, NotificationChangeType::Change);
    assert_eq!(last_event().detail, Some(NotificationViewItemContentChangeKind::Severity));

    item1_handle.update_actions(Some(NotificationActions { primary: Vec::new(), secondary: Vec::new() }));
    assert_eq!(last_event().kind, NotificationChangeType::Change);
    assert_eq!(last_event().detail, Some(NotificationViewItemContentChangeKind::Actions));

    item1_handle.progress().infinite();
    assert_eq!(last_event().kind, NotificationChangeType::Change);
    assert_eq!(last_event().detail, Some(NotificationViewItemContentChangeKind::Progress));

    let item2_handle = model.add_notification(item2.clone());
    assert_eq!(last_event().item.severity(), item2.severity);
    assert_eq!(displayed(&last_event().item), "Warning Message");
    assert_eq!(last_event().index, 0);
    assert_eq!(last_event().kind, NotificationChangeType::Add);

    let item3_handle = model.add_notification(item3.clone());
    assert_eq!(last_event().item.severity(), item3.severity);
    assert_eq!(displayed(&last_event().item), "Info Message");
    assert_eq!(last_event().index, 0);
    assert_eq!(last_event().kind, NotificationChangeType::Add);

    assert_eq!(model.notifications().len(), 3);

    let called = Rc::new(Cell::new(0));
    count(&guard, item1_handle.on_did_close(), &called, |_| true);

    item1_handle.close();
    assert_eq!(called.get(), 1);
    assert_eq!(model.notifications().len(), 2);
    assert_eq!(last_event().item.severity(), Severity::Warning);
    assert_eq!(displayed(&last_event().item), "Different Error Message");
    assert_eq!(last_event().index, 2);
    assert_eq!(last_event().kind, NotificationChangeType::Remove);

    let item2_duplicate_handle = model.add_notification(item2_duplicate.clone());
    assert_eq!(model.notifications().len(), 2);
    assert_eq!(last_event().item.severity(), item2_duplicate.severity);
    assert_eq!(displayed(&last_event().item), "Warning Message");
    assert_eq!(last_event().index, 0);
    assert_eq!(last_event().kind, NotificationChangeType::Add);

    item2_handle.close();
    assert_eq!(model.notifications().len(), 1);
    assert_eq!(last_event().item.severity(), item2_duplicate.severity);
    assert_eq!(displayed(&last_event().item), "Warning Message");
    assert_eq!(last_event().index, 0);
    assert_eq!(last_event().kind, NotificationChangeType::Remove);

    model.notifications()[0].expand();
    assert_eq!(last_event().item.severity(), item3.severity);
    assert_eq!(displayed(&last_event().item), "Info Message");
    assert_eq!(last_event().index, 0);
    assert_eq!(last_event().kind, NotificationChangeType::ExpandCollapse);

    let disposable = model.show_status_message("Hello World".into(), None);
    assert_eq!(model.status_message().expect("há mensagem de status").message, "Hello World");
    assert_eq!(last_status_event().item.message, model.status_message().expect("há mensagem de status").message);
    assert_eq!(last_status_event().kind, StatusMessageChangeType::Add);
    disposable.close();
    assert!(model.status_message().is_none());
    assert_eq!(last_status_event().kind, StatusMessageChangeType::Remove);

    let disposable2 = model.show_status_message("Hello World 2".into(), None);
    let disposable3 = model.show_status_message("Hello World 3".into(), None);

    assert_eq!(model.status_message().expect("há mensagem de status").message, "Hello World 3");

    disposable2.close();
    assert_eq!(model.status_message().expect("há mensagem de status").message, "Hello World 3");

    disposable3.close();
    assert!(model.status_message().is_none());

    item2_duplicate_handle.close();
    item3_handle.close();
}
