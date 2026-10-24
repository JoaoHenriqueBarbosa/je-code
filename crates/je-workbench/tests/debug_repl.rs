//! Porte de `src/vs/workbench/contrib/debug/test/browser/repl.test.ts` (suíte `Debug - REPL`), caso a caso e na mesma ordem.
//!
//! Os 10 casos, todos portados: `repl output`, `repl output count`, `repl merging`, `repl expressions`, `repl ordering`,
//! `repl groups`, `repl identical line collapsing - character by character`, `... - single event multiple lines`,
//! `... - mixed scenarios` e `repl filter`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso; o `setup` (`createMockDebugModel`) roda
//!   no começo de cada um.
//! - `new TestConfigurationService({ debug: { console: { collapseIdenticalLines: true } } })` é o provedor de configuração
//!   do console (`DebugConsoleConfiguration`): colapsar linhas idênticas ligado, sem limite de linhas (o `maximumLines`
//!   `undefined` do original nunca é excedido).
//! - `<ReplOutputElement[]>repl.getReplElements()` é a variante `ReplElement::Output`; os acessos ao `value` e ao `severity`
//!   passam por [`output`].
//! - `new RawObjectReplElement('fakeid', 'fake', keyValueObject)` guarda o `Rc` do elemento para a conferência de
//!   `(element.expression as RawObjectReplElement).valueObj` (o `Rc<dyn IExpression>` não faz downcast): compara-se o
//!   ponteiro e o `value_obj`.
//! - `new StackFrame(thread, 1, <any>undefined, ...)` ganha uma `Source` sem dados (o `StackFrame` do porte exige a fonte;
//!   o caso só usa o `frameId`). `.then()` sem uso é o `drop` da promessa; os restos assíncronos esvaziam no fim do guarda.
//! - `session['raw'] = <any>rawSession` é `session.set_raw(...)`; o `repl ordering` roda com o relógio virtual (o
//!   `await timeout(0)` e o `setTimeout` do `MockDebugAdapter`).
//! - `ReplFilter.filter(e, TreeVisibility.Visible)` devolve `TreeFilterResult`; `filterResult === true || filterResult ===
//!   TreeVisibility.Visible` é o `match` das duas variantes.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::severity::Severity;
use je_platform::log::NullLogService;
use je_widgets::tree::tree_types::{ITreeFilter, TreeFilterResult, TreeVisibility};
use je_workbench::contrib::debug::debug::IDebugSessionReplMode;
use je_workbench::contrib::debug::debug_model::{DebugModel, StackFrame, Thread};
use je_workbench::contrib::debug::debug_session::{DebugSession, DebugSessionOptions};
use je_workbench::contrib::debug::debug_source::Source;
use je_workbench::contrib::debug::debug_types::{IDebugSessionFull, IExpression};
use je_workbench::contrib::debug::raw_debug_session::RawDebugSession;
use je_workbench::contrib::debug::repl_filter::ReplFilter;
use je_workbench::contrib::debug::repl_model::{
    DebugConsoleConfiguration, NewReplElementData, RawObjectReplElement, ReplElement, ReplGroup, ReplModel, ReplOutputElement,
};
use serde_json::json;
use support::mock_debug::{MockDebugAdapter, MockRawSession, create_mock_debug_model, create_test_session, mock_uri_identity_service};

/// `configurationService`.
fn configuration() -> je_workbench::contrib::debug::repl_model::DebugConsoleConfigurationProvider {
    Rc::new(|| DebugConsoleConfiguration { collapse_identical_lines: true, maximum_lines: usize::MAX })
}

fn new_repl() -> Rc<ReplModel> {
    ReplModel::new(configuration())
}

fn session_of(model: &Rc<DebugModel>, name: &str, options: Option<DebugSessionOptions>, guard: &LeakGuard) -> Rc<DebugSession> {
    guard.add_rc(create_test_session(model, name, options, &guard.store()))
}

fn append(repl: &ReplModel, session: &Rc<DebugSession>, output: &str, sev: Severity) {
    let session: Rc<dyn IDebugSessionFull> = session.clone();
    repl.append_to_repl(&session, NewReplElementData { output: output.to_owned(), expression: None, sev, source: None });
}

fn output(element: &ReplElement) -> Rc<ReplOutputElement> {
    match element {
        ReplElement::Output(element) => element.clone(),
        _ => panic!("o elemento não é um ReplOutputElement"),
    }
}

fn outputs(repl: &ReplModel) -> Vec<Rc<ReplOutputElement>> {
    repl.get_repl_elements().iter().map(output).collect()
}

fn child_of(parent: &Rc<DebugSession>) -> Option<DebugSessionOptions> {
    Some(DebugSessionOptions { parent_session: Some(parent.clone() as Rc<dyn IDebugSessionFull>), ..DebugSessionOptions::default() })
}

#[test]
fn repl_output() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    let repl = new_repl();
    append(&repl, &session, "first line\n", Severity::Error);
    append(&repl, &session, "second line ", Severity::Error);
    append(&repl, &session, "third line ", Severity::Error);
    append(&repl, &session, "fourth line", Severity::Error);

    let mut elements = outputs(&repl);
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0].value, "first line\n");
    assert_eq!(elements[0].severity, Severity::Error);
    assert_eq!(elements[1].value, "second line third line fourth line");
    assert_eq!(elements[1].severity, Severity::Error);

    append(&repl, &session, "1", Severity::Warning);
    elements = outputs(&repl);
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[2].value, "1");
    assert_eq!(elements[2].severity, Severity::Warning);

    let key_value_object = json!({ "key1": 2, "key2": "value" });
    let raw = RawObjectReplElement::new("fakeid".to_owned(), "fake".to_owned(), key_value_object.clone(), None, None);
    let session_full: Rc<dyn IDebugSessionFull> = session.clone();
    repl.append_to_repl(
        &session_full,
        NewReplElementData { output: String::new(), expression: Some(raw.clone() as Rc<dyn IExpression>), sev: Severity::Info, source: None },
    );
    let element = match &repl.get_repl_elements()[3] {
        ReplElement::Variable(element) => element.clone(),
        _ => panic!("o elemento não é um ReplVariableElement"),
    };
    assert_eq!(element.expression.value(), "Object");
    assert!(std::ptr::addr_eq(Rc::as_ptr(&element.expression), Rc::as_ptr(&raw)));
    assert_eq!(raw.value_obj, key_value_object);

    repl.remove_repl_expressions();
    assert_eq!(repl.get_repl_elements().len(), 0);

    append(&repl, &session, "1\n", Severity::Info);
    append(&repl, &session, "2", Severity::Info);
    append(&repl, &session, "3\n4", Severity::Info);
    append(&repl, &session, "5\n", Severity::Info);
    append(&repl, &session, "6", Severity::Info);
    elements = outputs(&repl);
    assert_eq!(elements.iter().map(|e| e.to_display_string(false)).collect::<Vec<_>>(), vec!["1\n", "23\n", "45\n", "6"]);

    repl.remove_repl_expressions();
}

#[test]
fn repl_output_count() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    let repl = new_repl();
    for line in ["first line\n", "first line\n", "first line\n", "second line\n", "second line\n", "third line\n"] {
        append(&repl, &session, line, Severity::Info);
    }
    let elements = outputs(&repl);
    let summary: Vec<(String, i64)> = elements.iter().map(|e| (e.value.clone(), e.count())).collect();
    assert_eq!(summary, vec![("first line\n".to_owned(), 3), ("second line\n".to_owned(), 2), ("third line\n".to_owned(), 1)]);
}

#[test]
fn repl_merging() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let options = |parent: Option<&Rc<DebugSession>>, repl: Option<IDebugSessionReplMode>| {
        Some(DebugSessionOptions {
            parent_session: parent.map(|parent| parent.clone() as Rc<dyn IDebugSessionFull>),
            repl,
            ..DebugSessionOptions::default()
        })
    };
    // 'mergeWithParent' should be ignored when there is no parent.
    let parent = session_of(&model, "parent", options(None, Some(IDebugSessionReplMode::MergeWithParent)), &guard);
    let child1 = session_of(&model, "child1", options(Some(&parent), Some(IDebugSessionReplMode::Separate)), &guard);
    let child2 = session_of(&model, "child2", options(Some(&parent), Some(IDebugSessionReplMode::MergeWithParent)), &guard);
    let grand_child = session_of(&model, "grandChild", options(Some(&child2), Some(IDebugSessionReplMode::MergeWithParent)), &guard);
    let child3 = session_of(&model, "child3", child_of(&parent), &guard);

    let parent_changes = Rc::new(Cell::new(0));
    let counter = parent_changes.clone();
    guard.add(parent.on_did_change_repl_elements().listen(move |_| counter.set(counter.get() + 1)));

    let append_to = |session: &Rc<DebugSession>, text: &str| {
        session.append_to_repl(NewReplElementData { output: text.to_owned(), expression: None, sev: Severity::Info, source: None }, false);
    };
    let counts = |sessions: [&Rc<DebugSession>; 5]| -> Vec<usize> { sessions.iter().map(|s| s.get_repl_elements().len()).collect() };

    append_to(&parent, "1\n");
    assert_eq!(parent_changes.get(), 1);
    assert_eq!(counts([&parent, &child1, &child2, &grand_child, &child3]), vec![1, 0, 1, 1, 0]);

    append_to(&grand_child, "2\n");
    assert_eq!(parent_changes.get(), 2);
    assert_eq!(counts([&parent, &child1, &child2, &grand_child, &child3]), vec![2, 0, 2, 2, 0]);

    append_to(&child3, "3\n");
    assert_eq!(parent_changes.get(), 2);
    assert_eq!(counts([&parent, &child1, &child2, &grand_child, &child3]), vec![2, 0, 2, 2, 1]);

    append_to(&child1, "4\n");
    assert_eq!(parent_changes.get(), 2);
    assert_eq!(counts([&parent, &child1, &child2, &grand_child, &child3]), vec![2, 1, 2, 2, 1]);
}

#[test]
fn repl_expressions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let raw_session = MockRawSession::new();
    let session = session_of(&model, "mockSession", None, &guard);
    assert_eq!(session.get_repl_elements().len(), 0);
    model.add_session(session.clone());

    session.set_raw(Some(raw_session));
    let thread = Thread::new(session.clone(), "mockthread", 1);
    let source = Rc::new(Source::new(None, "aDebugSessionId", &*mock_uri_identity_service(), &NullLogService));
    let stack_frame = StackFrame::new(thread, 1, source, "app.js", Some("normal"), Some(je_editor::range::Range::new(1, 1, 1, 10)), 1, true, None);
    let repl_model = new_repl();
    let session_full: Rc<dyn IDebugSessionFull> = session.clone();
    drop(repl_model.add_repl_expression(session_full.clone(), Some(stack_frame.clone()), "myVariable"));
    drop(repl_model.add_repl_expression(session_full.clone(), Some(stack_frame.clone()), "myVariable"));
    drop(repl_model.add_repl_expression(session_full, Some(stack_frame), "myVariable"));

    assert_eq!(repl_model.get_repl_elements().len(), 3);
    for re in repl_model.get_repl_elements() {
        match re {
            ReplElement::Input(input) => assert_eq!(input.value, "myVariable"),
            _ => panic!("o elemento não é um ReplEvaluationInput"),
        }
    }

    repl_model.remove_repl_expressions();
    assert_eq!(repl_model.get_repl_elements().len(), 0);
}

#[test]
fn repl_ordering() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    run_with_faked_timers(async move {
        let session = session_of(&model, "mockSession", None, &guard);
        model.add_session(session.clone());

        let adapter = MockDebugAdapter::new();
        let raw = guard.add_rc(RawDebugSession::new(adapter, None, "", "", None));
        session.initialize_for_test(raw);

        let value_of = |index: usize| -> String {
            match &session.get_repl_elements()[index] {
                ReplElement::Input(element) => element.value.clone(),
                ReplElement::Output(element) => element.value.clone(),
                ReplElement::Result(element) => element.value(),
                _ => panic!("elemento inesperado"),
            }
        };

        session.add_repl_expression(None, "before.1").await.unwrap();
        assert_eq!(session.get_repl_elements().len(), 3);
        assert!(matches!(session.get_repl_elements()[0], ReplElement::Input(_)));
        assert_eq!(value_of(0), "before.1");
        assert!(matches!(session.get_repl_elements()[1], ReplElement::Output(_)));
        assert_eq!(value_of(1), "before.1");
        assert!(matches!(session.get_repl_elements()[2], ReplElement::Result(_)));
        assert_eq!(value_of(2), "=before.1");

        session.add_repl_expression(None, "after.2").await.unwrap();
        timeout(0).await.unwrap();
        assert_eq!(session.get_repl_elements().len(), 6);
        assert!(matches!(session.get_repl_elements()[3], ReplElement::Input(_)));
        assert_eq!(value_of(3), "after.2");
        assert!(matches!(session.get_repl_elements()[4], ReplElement::Result(_)));
        assert_eq!(value_of(4), "=after.2");
        assert!(matches!(session.get_repl_elements()[5], ReplElement::Output(_)));
        assert_eq!(value_of(5), "after.2");
    });
}

fn group_of(element: &ReplElement) -> Rc<ReplGroup> {
    match element {
        ReplElement::Group(group) => group.clone(),
        _ => panic!("o elemento não é um ReplGroup"),
    }
}

#[test]
fn repl_groups() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    let session_full: Rc<dyn IDebugSessionFull> = session.clone();
    let repl = new_repl();

    append(&repl, &session, "first global line", Severity::Info);
    repl.start_group(&session_full, "group_1", true, None);
    append(&repl, &session, "first line in group", Severity::Info);
    append(&repl, &session, "second line in group", Severity::Info);
    let elements = repl.get_repl_elements();
    assert_eq!(elements.len(), 2);
    let group = group_of(&elements[1]);
    assert_eq!(group.name, "group_1");
    assert!(group.auto_expand);
    assert!(group.has_children());
    assert!(!group.has_ended());

    repl.start_group(&session_full, "group_2", false, None);
    append(&repl, &session, "first line in subgroup", Severity::Info);
    append(&repl, &session, "second line in subgroup", Severity::Info);
    let children = group.get_children();
    assert_eq!(children.len(), 3);
    assert_eq!(output(&children[0]).value, "first line in group");
    assert_eq!(output(&children[1]).value, "second line in group");
    assert_eq!(group_of(&children[2]).name, "group_2");
    assert!(!group_of(&children[2]).has_ended());
    assert_eq!(group_of(&children[2]).get_children().len(), 2);
    repl.end_group();
    assert!(group_of(&children[2]).has_ended());
    append(&repl, &session, "third line in group", Severity::Info);
    assert_eq!(group.get_children().len(), 4);
    assert!(!group.has_ended());
    repl.end_group();
    assert!(group.has_ended());
    append(&repl, &session, "second global line", Severity::Info);
    assert_eq!(repl.get_repl_elements().len(), 3);
    assert_eq!(output(&repl.get_repl_elements()[2]).value, "second global line");
}

#[test]
fn repl_identical_line_collapsing_character_by_character() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    let repl = new_repl();

    // Test case 1: Character-by-character output should NOT be collapsed
    // These should print "111\n", not "(3)1"
    append(&repl, &session, "1", Severity::Info);
    append(&repl, &session, "1", Severity::Info);
    append(&repl, &session, "1", Severity::Info);
    append(&repl, &session, "\n", Severity::Info);

    let mut elements = outputs(&repl);
    // Should be one element with "111\n" value, not collapsed
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].value, "111\n");
    assert_eq!(elements[0].count(), 1);

    repl.remove_repl_expressions();

    // Test case 2: Character-by-character with mixed output
    append(&repl, &session, "5", Severity::Info);
    append(&repl, &session, "5", Severity::Info);
    append(&repl, &session, "\n", Severity::Info);

    elements = outputs(&repl);
    // Should be one element with "55\n" value, not "(2)5"
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].value, "55\n");
    assert_eq!(elements[0].count(), 1);
}

#[test]
fn repl_identical_line_collapsing_single_event_multiple_lines() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    let repl = new_repl();

    // Test case: Single event with multiple identical lines should be collapsed
    // This should be collapsed into "(2)hello"
    append(&repl, &session, "hello\nhello\n", Severity::Info);

    let elements = outputs(&repl);
    // Should be one collapsed element with count 2
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].value, "hello\n");
    assert_eq!(elements[0].count(), 2);
}

#[test]
fn repl_identical_line_collapsing_mixed_scenarios() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    let repl = new_repl();

    // Test case: Mix of single events and multi-line events
    append(&repl, &session, "test\n", Severity::Info);
    append(&repl, &session, "test\ntest\n", Severity::Info);

    let elements = outputs(&repl);
    // Should be one collapsed element with count 3
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].value, "test\n");
    assert_eq!(elements[0].count(), 3);
}

#[test]
fn repl_filter() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    let repl = new_repl();
    let repl_filter = ReplFilter::new();

    let get_filtered_elements = || -> Vec<Rc<ReplOutputElement>> {
        repl.get_repl_elements()
            .iter()
            .filter(|e| {
                matches!(
                    repl_filter.filter(e, TreeVisibility::Visible),
                    TreeFilterResult::Bool(true) | TreeFilterResult::Visibility(TreeVisibility::Visible)
                )
            })
            .map(output)
            .collect()
    };

    append(&repl, &session, "first line\n", Severity::Info);
    append(&repl, &session, "second line\n", Severity::Info);
    append(&repl, &session, "third line\n", Severity::Info);
    append(&repl, &session, "fourth line\n", Severity::Info);

    repl_filter.set_filter_query("first");
    let r1 = get_filtered_elements();
    assert_eq!(r1.len(), 1);
    assert_eq!(r1[0].value, "first line\n");

    repl_filter.set_filter_query("!first");
    let r2 = get_filtered_elements();
    assert_eq!(r1.len(), 1);
    assert_eq!(r2[0].value, "second line\n");
    assert_eq!(r2[1].value, "third line\n");
    assert_eq!(r2[2].value, "fourth line\n");

    repl_filter.set_filter_query("first, line");
    let r3 = get_filtered_elements();
    assert_eq!(r3.len(), 4);
    assert_eq!(r3[0].value, "first line\n");
    assert_eq!(r3[1].value, "second line\n");
    assert_eq!(r3[2].value, "third line\n");
    assert_eq!(r3[3].value, "fourth line\n");

    repl_filter.set_filter_query("line, !second");
    let r4 = get_filtered_elements();
    assert_eq!(r4.len(), 3);
    assert_eq!(r4[0].value, "first line\n");
    assert_eq!(r4[1].value, "third line\n");
    assert_eq!(r4[2].value, "fourth line\n");

    repl_filter.set_filter_query("!second, line");
    let r4_same = get_filtered_elements();
    assert_eq!(r4.len(), r4_same.len());

    repl_filter.set_filter_query("!line");
    let r5 = get_filtered_elements();
    assert_eq!(r5.len(), 0);

    repl_filter.set_filter_query("smth");
    let r6 = get_filtered_elements();
    assert_eq!(r6.len(), 0);
}
