//! Porte de `src/vs/workbench/contrib/testing/test/browser/explorerProjections/treeProjection.test.ts` (suíte `Workbench -
//! Testing Explorer Hierarchal by Location Projection`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no começo de cada caso (antes do `setup`); o `ds`
//!   do original é o próprio guarda (`ds.add(harness)` e `ds.add(new Emitter())` são `add_rc`), e o `teardown` roda no
//!   `Drop` dele. Os casos rodam em `run_with_faked_timers`, como os demais do contrib testing.
//! - `class TestHierarchicalByLocationProjection extends TreeProjection {}` (vazia) é o próprio `TreeProjection`.
//! - `upcastPartial<ITestResultService>({ results: [], onResultsChanged: Event.None, onTestChanged, getStateById })` é o
//!   `PartialTestResultService`; `resultsService.getStateById = () => [undefined!, ...]` troca o fecho `get_state_by_id`,
//!   e o `undefined!` (o resultado ao vivo, que o teste nunca usa) é um `LiveTestResult` qualquer do guarda. Por isso o
//!   `ev.result !== results.results[0]` do original (falso, `undefined` dos dois lados) é verdadeiro aqui, e a
//!   projeção consulta o `getStateById` também no primeiro disparo; ele devolve o mesmo estado (`Queued`), então a
//!   árvore resultante é a mesma.
//! - `{ sortText: undefined, label: 'z' }` (a atribuição de `undefined` limpa a propriedade) é `sort_text: Some(None)`.
//! - `harness.tree.expand(element)` é `expand(&Some(elemento), false)`; `expandAll()` é `expand_all()`.
//! - `String(TestResultState.Queued)` é o número do estado em texto (`"1"`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::event::Emitter;
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_workbench::contrib::testing::browser::explorer_projections::index::{ITestTreeProjection, TestExplorerTreeElement};
use je_workbench::contrib::testing::browser::explorer_projections::testing_view_state::ISerializedTestTreeCollapseState;
use je_workbench::contrib::testing::browser::explorer_projections::tree_projection::TreeProjection;
use je_workbench::contrib::testing::common::test_id::TestId;
use je_workbench::contrib::testing::common::test_item_collection::{ITestChildrenLike, ITestItemChildren};
use je_workbench::contrib::testing::common::test_result::{ITestResult, LiveTestResult, TestResultItemChange, TestResultItemChangeKind, TestResultNode};
use je_workbench::contrib::testing::common::test_types::{
    ITestItem, ITestItemUpdate, InternalTestItem, PartialTestItem, ResolvedTestRunRequest, TestItemExpandState, TestMessageText, TestResultItem, TestResultState, TestsDiffOp,
    test_run_profile_bitset,
};
use support::test_object_tree::{PartialTestResultService, TestTreeTestHarness, leaf, leaf_with_data, node};
use support::test_stubs::TestTestItem;

struct Fixture {
    harness: Rc<TestTreeTestHarness<TreeProjection>>,
    results_service: Rc<PartialTestResultService>,
    on_test_changed: Rc<Emitter<TestResultItemChange>>,
    live_result: Rc<dyn ITestResult>,
    _guard: LeakGuard,
}

fn setup() -> Fixture {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let on_test_changed = guard.add_rc(Emitter::new());
    let results_service = PartialTestResultService::new(on_test_changed.event());
    let request = ResolvedTestRunRequest { group: test_run_profile_bitset::RUN, targets: Vec::new(), exclude: None, continuous: None, preserve_focus: None };
    let live_result = guard.add_rc(LiveTestResult::new("undefined", false, request, 0, null_telemetry_service()));

    let service = results_service.clone();
    let harness = guard.add_rc(Rc::new(TestTreeTestHarness::new(move |l| TreeProjection::new(ISerializedTestTreeCollapseState::default(), l, service.clone()), None)));
    Fixture { harness, results_service, on_test_changed, live_result, _guard: guard }
}

fn id(parts: &[&str]) -> TestId {
    TestId::new(parts.iter().map(|part| (*part).to_string()).collect())
}

fn added(controller_id: &str, expand: TestItemExpandState, parts: &[&str], label: &str) -> TestsDiffOp {
    TestsDiffOp::Add { item: InternalTestItem { controller_id: controller_id.to_string(), expand, item: TestTestItem::new(id(parts), label, None).to_test_item() } }
}

fn removed(parts: &[&str]) -> TestsDiffOp {
    TestsDiffOp::Remove { item_id: id(parts).to_string() }
}

fn updated(parts: &[&str], item: PartialTestItem) -> TestsDiffOp {
    TestsDiffOp::Update { item: ITestItemUpdate { ext_id: id(parts).to_string(), expand: None, item: Some(item) } }
}

fn expand_test(f: &Fixture, parts: &[&str]) {
    let element = f.harness.projection.get_element_by_test_id(&id(parts).to_string()).expect("elemento");
    f.harness.tree.expand(&Some(TestExplorerTreeElement::Item(element)), false);
}

fn result_in_state(state: TestResultState) -> Rc<TestResultNode> {
    TestResultNode::new(TestResultItem {
        item: ITestItem {
            ext_id: id(&["ctrlId", "id-a"]).to_string(),
            busy: false,
            description: None,
            error: None,
            label: "a".to_string(),
            range: None,
            sort_text: None,
            tags: Vec::new(),
            uri: None,
        },
        tasks: Vec::new(),
        own_computed_state: state,
        computed_state: state,
        expand: TestItemExpandState::NotExpandable,
        controller_id: "ctrl".to_string(),
        own_duration: None,
        retired: None,
    })
}

type GetStateById = Box<dyn Fn(&str) -> Option<(Rc<dyn ITestResult>, Rc<TestResultNode>)>>;

/// `() => [undefined!, resultInState(state)]`.
fn reports(live_result: &Rc<dyn ITestResult>, state: TestResultState) -> GetStateById {
    let live_result = live_result.clone();
    Box::new(move |_| Some((live_result.clone(), result_in_state(state))))
}

#[test]
fn renders_initial_tree() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        assert_eq!(f.harness.tree.get_rendered(None), vec![leaf("a"), leaf("b")]);
    });
}

#[test]
fn expands_children() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        expand_test(&f, &["ctrlId", "id-a"]);
        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("aa"), leaf("ab")]), leaf("b")]);
    });
}

#[test]
fn updates_render_if_second_test_provider_appears() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        f.harness.push_diff(vec![
            added("ctrl2", TestItemExpandState::Expanded, &["ctrlId2"], "c"),
            added("ctrl2", TestItemExpandState::NotExpandable, &["ctrlId2", "id-c"], "ca"),
        ]);

        assert_eq!(f.harness.flush(), vec![node("c", vec![leaf("ca")]), node("root", vec![leaf("a"), leaf("b")])]);
    });
}

#[test]
fn updates_nodes_if_they_add_children() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        expand_test(&f, &["ctrlId", "id-a"]);

        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("aa"), leaf("ab")]), leaf("b")]);

        f.harness.c.root().children().get("id-a").expect("id-a").children().add(TestTestItem::new(id(&["ctrlId", "id-a", "id-ac"]), "ac", None));

        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("aa"), leaf("ab"), leaf("ac")]), leaf("b")]);
    });
}

#[test]
fn updates_nodes_if_they_remove_children() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        expand_test(&f, &["ctrlId", "id-a"]);

        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("aa"), leaf("ab")]), leaf("b")]);

        f.harness.c.root().children().get("id-a").expect("id-a").children().delete("id-ab");

        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("aa")]), leaf("b")]);
    });
}

#[test]
fn applies_state_changes() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();

        let change = |state: TestResultState, previous_state: TestResultState| TestResultItemChange {
            item: result_in_state(state),
            result: f.live_result.clone(),
            change: TestResultItemChangeKind::OwnStateChange { previous_state, previous_own_duration: None },
        };

        // Applies the change:
        *f.results_service.get_state_by_id.borrow_mut() = reports(&f.live_result, TestResultState::Queued);
        f.on_test_changed.fire(change(TestResultState::Queued, TestResultState::Unset));
        f.harness.projection.apply_to(&f.harness.tree);

        assert_eq!(f.harness.tree.get_rendered(Some("state")), vec![leaf_with_data("a", "1"), leaf_with_data("b", "0")]);

        // Falls back if moved into unset state:
        *f.results_service.get_state_by_id.borrow_mut() = reports(&f.live_result, TestResultState::Failed);
        f.on_test_changed.fire(change(TestResultState::Unset, TestResultState::Queued));
        f.harness.projection.apply_to(&f.harness.tree);

        assert_eq!(f.harness.tree.get_rendered(Some("state")), vec![leaf_with_data("a", "4"), leaf_with_data("b", "0")]);
    });
}

#[test]
fn applies_test_changes_resort() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        expand_test(&f, &["ctrlId", "id-a"]);
        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("aa"), leaf("ab")]), leaf("b")]);

        // sortText causes order to change
        f.harness.push_diff(vec![
            updated(&["ctrlId", "id-a", "id-aa"], PartialTestItem { sort_text: Some(Some("z".to_string())), ..Default::default() }),
            updated(&["ctrlId", "id-a", "id-ab"], PartialTestItem { sort_text: Some(Some("a".to_string())), ..Default::default() }),
        ]);
        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("ab"), leaf("aa")]), leaf("b")]);

        // label causes order to change
        f.harness.push_diff(vec![
            updated(&["ctrlId", "id-a", "id-aa"], PartialTestItem { sort_text: Some(None), label: Some("z".to_string()), ..Default::default() }),
            updated(&["ctrlId", "id-a", "id-ab"], PartialTestItem { sort_text: Some(None), label: Some("a".to_string()), ..Default::default() }),
        ]);
        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("a"), leaf("z")]), leaf("b")]);

        f.harness.push_diff(vec![
            updated(&["ctrlId", "id-a", "id-aa"], PartialTestItem { label: Some("a2".to_string()), ..Default::default() }),
            updated(&["ctrlId", "id-a", "id-ab"], PartialTestItem { label: Some("z2".to_string()), ..Default::default() }),
        ]);
        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("a2"), leaf("z2")]), leaf("b")]);
    });
}

#[test]
fn applies_test_changes_error() {
    run_with_faked_timers(async {
        let f = setup();
        let error = |text: &str| PartialTestItem { error: Some(Some(TestMessageText::Text(text.to_string()))), ..Default::default() };

        f.harness.flush();
        assert_eq!(f.harness.flush(), vec![leaf("a"), leaf("b")]);

        // sortText causes order to change
        f.harness.push_diff(vec![updated(&["ctrlId", "id-a"], error("bad"))]);
        assert_eq!(f.harness.flush(), vec![leaf("a"), leaf("b")]);

        expand_test(&f, &["ctrlId", "id-a"]);
        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("bad"), leaf("aa"), leaf("ab")]), leaf("b")]);

        f.harness.push_diff(vec![updated(&["ctrlId", "id-a"], error("badder"))]);
        assert_eq!(f.harness.flush(), vec![node("a", vec![leaf("badder"), leaf("aa"), leaf("ab")]), leaf("b")]);
    });
}

#[test]
fn fixes_204805() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        f.harness.push_diff(vec![
            removed(&["ctrlId"]),
            added("ctrlId", TestItemExpandState::NotExpandable, &["ctrlId"], "ctrl"),
            added("ctrlId", TestItemExpandState::NotExpandable, &["ctrlId", "a"], "a"),
        ]);

        assert_eq!(f.harness.flush(), vec![leaf("a")]);

        f.harness.push_diff(vec![added("ctrlId", TestItemExpandState::NotExpandable, &["ctrlId", "a", "b"], "b")]);
        f.harness.flush();
        f.harness.tree.expand_all();
        assert_eq!(f.harness.tree.get_rendered(None), vec![node("a", vec![leaf("b")])]);

        f.harness.push_diff(vec![added("ctrlId", TestItemExpandState::NotExpandable, &["ctrlId", "a", "b", "c"], "c")]);
        f.harness.flush();
        f.harness.tree.expand_all();
        assert_eq!(f.harness.tree.get_rendered(None), vec![node("a", vec![node("b", vec![leaf("c")])])]);
    });
}

#[test]
fn fixes_213316_single_root() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        assert_eq!(f.harness.tree.get_rendered(None), vec![leaf("a"), leaf("b")]);
        f.harness.push_diff(vec![removed(&["ctrlId", "id-a"])]);
        f.harness.flush();
        assert_eq!(f.harness.tree.get_rendered(None), vec![leaf("b")]);
    });
}

#[test]
fn fixes_213316_multi_root() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.push_diff(vec![
            added("ctrl2", TestItemExpandState::Expanded, &["ctrlId2"], "c"),
            added("ctrl2", TestItemExpandState::NotExpandable, &["ctrlId2", "id-c"], "ca"),
        ]);
        f.harness.flush();
        assert_eq!(f.harness.flush(), vec![node("c", vec![leaf("ca")]), node("root", vec![leaf("a"), leaf("b")])]);

        f.harness.push_diff(vec![removed(&["ctrlId", "id-a"])]);
        f.harness.flush();
        assert_eq!(f.harness.tree.get_rendered(None), vec![node("c", vec![leaf("ca")]), node("root", vec![leaf("b")])]);

        f.harness.push_diff(vec![removed(&["ctrlId", "id-b"])]);
        f.harness.flush();
        assert_eq!(f.harness.tree.get_rendered(None), vec![leaf("ca")]);
    });
}
