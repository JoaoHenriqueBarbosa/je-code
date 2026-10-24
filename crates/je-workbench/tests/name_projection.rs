//! Porte de `src/vs/workbench/contrib/testing/test/browser/explorerProjections/nameProjection.test.ts` (suíte `Workbench -
//! Testing Explorer Hierarchal by Name Projection`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no começo de cada caso (antes do `setup`); o
//!   `teardown` (`harness.dispose()`) roda no `Drop` do [`Fixture`], antes da conferência do guarda. Os casos rodam em
//!   `run_with_faked_timers`, como os demais do contrib testing.
//! - `upcastPartial<ITestResultService>({ onResultsChanged: Event.None, onTestChanged, getStateById: () => undefined })`
//!   é o `PartialTestResultService` de `support::test_object_tree` (o `getStateById` padrão devolve `None`).
//! - `new TestTestItem(id, label, undefined).toTestItem()` é `TestTestItem::new(id, label, None).to_test_item()`;
//!   `harness.c.root.children.get('id-a')!.children.add(...)` passa por `root().children()` e `ITestChildrenLike::get`.
//! - O `async` dos casos não espera nada (a árvore é síncrona), então o corpo é o mesmo.
//! - `harness.tree.getRendered()` e `harness.flush()` devolvem [`SerializedTree`] (`leaf`, `node`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::event::Emitter;
use je_base::lifecycle::IDisposable;
use je_workbench::contrib::testing::browser::explorer_projections::list_projection::ListProjection;
use je_workbench::contrib::testing::browser::explorer_projections::testing_view_state::ISerializedTestTreeCollapseState;
use je_workbench::contrib::testing::common::test_id::TestId;
use je_workbench::contrib::testing::common::test_item_collection::{ITestChildrenLike, ITestItemChildren};
use je_workbench::contrib::testing::common::test_result::TestResultItemChange;
use je_workbench::contrib::testing::common::test_types::{InternalTestItem, TestItemExpandState, TestsDiffOp};
use support::test_object_tree::{PartialTestResultService, TestTreeTestHarness, leaf, node};
use support::test_stubs::TestTestItem;

struct Fixture {
    harness: TestTreeTestHarness<ListProjection>,
    _on_test_changed: Rc<Emitter<TestResultItemChange>>,
    _guard: LeakGuard,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.harness.dispose();
    }
}

fn setup() -> Fixture {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let on_test_changed = Emitter::new();
    let results_service = PartialTestResultService::new(on_test_changed.event());

    let harness = TestTreeTestHarness::new(move |l| ListProjection::new(ISerializedTestTreeCollapseState::default(), l, results_service.clone()), None);
    Fixture { harness, _on_test_changed: on_test_changed, _guard: guard }
}

fn id(parts: &[&str]) -> TestId {
    TestId::new(parts.iter().map(|part| (*part).to_string()).collect())
}

fn added(controller_id: &str, expand: TestItemExpandState, parts: &[&str], label: &str) -> TestsDiffOp {
    TestsDiffOp::Add { item: InternalTestItem { controller_id: controller_id.to_string(), expand, item: TestTestItem::new(id(parts), label, None).to_test_item() } }
}

#[test]
fn renders_initial_tree() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        assert_eq!(f.harness.tree.get_rendered(None), vec![leaf("aa"), leaf("ab"), leaf("b")]);
    });
}

#[test]
fn updates_render_if_second_test_provider_appears() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        f.harness.push_diff(vec![
            added("ctrl2", TestItemExpandState::Expanded, &["ctrl2"], "root2"),
            added("ctrl2", TestItemExpandState::NotExpandable, &["ctrl2", "id-c"], "c"),
        ]);

        assert_eq!(f.harness.flush(), vec![node("root", vec![leaf("aa"), leaf("ab"), leaf("b")]), node("root2", vec![leaf("c")])]);
    });
}

#[test]
fn updates_nodes_if_they_add_children() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();

        f.harness.c.root().children().get("id-a").expect("id-a").children().add(TestTestItem::new(id(&["ctrlId", "id-a", "id-ac"]), "ac", None));

        assert_eq!(f.harness.flush(), vec![leaf("aa"), leaf("ab"), leaf("ac"), leaf("b")]);
    });
}

#[test]
fn updates_nodes_if_they_remove_children() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        f.harness.c.root().children().get("id-a").expect("id-a").children().delete("id-ab");

        assert_eq!(f.harness.flush(), vec![leaf("aa"), leaf("b")]);
    });
}

#[test]
fn swaps_when_node_is_no_longer_leaf() {
    run_with_faked_timers(async {
        let f = setup();
        f.harness.flush();
        f.harness.c.root().children().get("id-b").expect("id-b").children().add(TestTestItem::new(id(&["ctrlId", "id-b", "id-ba"]), "ba", None));

        assert_eq!(f.harness.flush(), vec![leaf("aa"), leaf("ab"), leaf("ba")]);
    });
}
