//! Porte de `src/vs/base/test/common/executionGraph.test.ts` (suíte `executionGraph`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - O `fire(time, label, parent: ExecutionRoot | ExecutionEvent)` do `HistoryBuilder` decide por
//!   `this.roots.includes(parent)`; aqui o tipo do pai ([`Parent`]) já diz se é raiz ou evento.
//! - O `ensureNoDisposablesAreLeakedInTestSuite` entra em cada teste pela guarda de `common`.

mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::execution_graph::{ExecutionEvent, ExecutionHistory, ExecutionRoot, render_lane_graph, render_swimlanes};

/// O pai de um evento disparado: uma raiz (filho direto) ou outro evento.
enum Parent {
    Root(Rc<ExecutionRoot>),
    Event(Rc<ExecutionEvent>),
}

impl From<&Rc<ExecutionRoot>> for Parent {
    fn from(root: &Rc<ExecutionRoot>) -> Self {
        Parent::Root(root.clone())
    }
}

impl From<&Rc<ExecutionEvent>> for Parent {
    fn from(event: &Rc<ExecutionEvent>) -> Self {
        Parent::Event(event.clone())
    }
}

/// Construtor fluente para montar `ExecutionHistory` à mão nos testes. Mantém as chamadas curtas:
/// `h.root("A")`, `h.fire(0.0, "setTimeout", &root_a)`, `h.fire(16.0, "rAF", &parent_event)`.
#[derive(Default)]
struct HistoryBuilder {
    roots: Vec<Rc<ExecutionRoot>>,
    events: Vec<Rc<ExecutionEvent>>,
}

impl HistoryBuilder {
    fn root(&mut self, label: &str) -> Rc<ExecutionRoot> {
        let root = Rc::new(ExecutionRoot { label: label.to_string() });
        self.roots.push(root.clone());
        root
    }

    fn fire(&mut self, time: f64, label: &str, parent: impl Into<Parent>) -> Rc<ExecutionEvent> {
        let (root, parent_event) = match parent.into() {
            Parent::Root(root) => (root, None),
            Parent::Event(event) => (event.root.clone(), Some(event)),
        };
        let event = Rc::new(ExecutionEvent { time, label: label.to_string(), root, parent: parent_event, detail: None });
        self.events.push(event.clone());
        event
    }

    fn build(&self) -> ExecutionHistory {
        ExecutionHistory { roots: self.roots.clone(), events: self.events.clone() }
    }
}

#[test]
fn render_swimlanes_empty_history() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(render_swimlanes(&ExecutionHistory::default()), "(empty history)");
}

#[test]
fn render_swimlanes_single_root_linear_chain() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut h = HistoryBuilder::default();
    let root_a = h.root("A");
    let t0 = h.fire(0.0, "setTimeout", &root_a);
    let t1 = h.fire(16.0, "rAF", &t0);
    h.fire(32.0, "rAF", &t1);

    // Slot-based indentation: all events are last-children, so all
    // share slot 0. No visual indentation for the tree structure.
    assert_eq!(
        render_swimlanes(&h.build()),
        [
            "            A",
            " +0ms ├─ setTimeout",
            "+16ms    └─ rAF",
            "+32ms       └─ rAF",
        ]
        .join("\n")
    );
}

#[test]
fn render_swimlanes_forest_with_forks_across_two_roots() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut h = HistoryBuilder::default();
    let root_a = h.root("A");
    let root_b = h.root("B");

    // A: setTimeout(+0) spawns rAF(+16) and setTimeout(+50)
    //    rAF(+16) -> rAF(+32)
    let a_t0 = h.fire(0.0, "setTimeout", &root_a);
    let b_t10 = h.fire(10.0, "setTimeout", &root_b);
    let a_raf16 = h.fire(16.0, "requestAnimationFrame", &a_t0);
    let b_raf26 = h.fire(26.0, "requestAnimationFrame", &b_t10);
    h.fire(32.0, "requestAnimationFrame", &a_raf16);
    h.fire(46.0, "setTimeout", &b_raf26);
    h.fire(50.0, "setTimeout", &a_t0);

    let out = render_swimlanes(&h.build());
    // Slot-based indentation: last-children share their parent's slot.
    // B lane: bT10 at slot 0, bRaf26 (last-child) at slot 0, bT46 (last-child) at slot 0.
    assert_eq!(
        out,
        [
            "                     A                             B",
            " +0ms ├─ setTimeout",
            "+10ms │  |                            ├─ setTimeout",
            "+16ms │  ├─ requestAnimationFrame     │",
            "+26ms │  │                               └─ requestAnimationFrame",
            "+32ms │     └─ requestAnimationFrame     │",
            "+46ms │                                     └─ setTimeout",
            "+50ms    └─ setTimeout",
        ]
        .join("\n")
    );
}

#[test]
fn render_swimlanes_degenerate_linked_list_tree_5_nodes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut h = HistoryBuilder::default();
    let root_a = h.root("A");
    let e1 = h.fire(0.0, "n1", &root_a);
    let e2 = h.fire(10.0, "n2", &e1);
    let e3 = h.fire(20.0, "n3", &e2);
    h.fire(30.0, "n4", &e3);

    assert_eq!(
        render_swimlanes(&h.build()),
        [
            "             A",
            " +0ms ├─ n1",
            "+10ms    └─ n2",
            "+20ms       └─ n3",
            "+30ms          └─ n4",
        ]
        .join("\n")
    );
}

#[test]
fn render_lane_graph_degenerate_linked_list_tree_5_nodes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut h = HistoryBuilder::default();
    let root_a = h.root("A");
    let e1 = h.fire(0.0, "n1", &root_a);
    let e2 = h.fire(10.0, "n2", &e1);
    let e3 = h.fire(20.0, "n3", &e2);
    h.fire(30.0, "n4", &e3);

    assert_eq!(
        render_lane_graph(&h.build()),
        [
            "+A",
            "└─╷─    [   +0ms] n1",
            "  └─╷─  [  +10ms] n2",
            "    └─╷─[  +20ms] n3",
            "      └─[  +30ms] n4",
        ]
        .join("\n")
    );
}

#[test]
fn render_lane_graph_forest_with_forks_across_two_roots() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut h = HistoryBuilder::default();
    let root_a = h.root("A");
    let root_b = h.root("B");

    let a_t0 = h.fire(0.0, "setTimeout", &root_a);
    let b_t10 = h.fire(10.0, "setTimeout", &root_b);
    let a_raf16 = h.fire(16.0, "requestAnimationFrame", &a_t0);
    let b_raf26 = h.fire(26.0, "requestAnimationFrame", &b_t10);
    h.fire(32.0, "requestAnimationFrame", &a_raf16);
    h.fire(46.0, "setTimeout", &b_raf26);
    h.fire(50.0, "setTimeout", &a_t0);

    assert_eq!(
        render_lane_graph(&h.build()),
        [
            "+A",
            "└─╷─        [   +0ms] setTimeout",
            "  │ +B",
            "  │ └─╷─    [  +10ms] setTimeout",
            "  ├───┼─╷─  [  +16ms] requestAnimationFrame",
            "  │   └─┼─╷─[  +26ms] requestAnimationFrame",
            "  │     └─┼─[  +32ms] requestAnimationFrame",
            "  │       └─[  +46ms] setTimeout",
            "  └─────────[  +50ms] setTimeout",
        ]
        .join("\n")
    );
}
