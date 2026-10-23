//! Porte de `src/vs/editor/test/common/model/intervalTree.test.ts` (suíte `IntervalTree 1`), caso a caso e na mesma ordem.
//!
//! A suíte `IntervalTree 1` tem as subsuítes `generated` (19 casos) e `searching` (5 casos, sobre a árvore
//! de Cormen); a segunda suíte do arquivo (`IntervalTree 2`, o caso `nodeAcceptEdit`) vem depois. O gerador
//! de testes aleatórios (`AutoTest`, com `GENERATE_TESTS = false` e `TEST_COUNT = 0` no original, nunca roda)
//! não é portado, então nenhum caso depende de `Math.random`. Os nós soltos (`new IntervalNode`) moram na
//! arena local de cada caso; na subsuíte `searching` a árvore de Cormen, criada uma vez no corpo da suíte do
//! original, é recriada em cada caso (as buscas só gravam os deslocamentos em cache, então dá no mesmo).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::interval_tree::{self,
    IntervalArena, IntervalNode, IntervalTree, NodeColor, NodeId, SENTINEL, get_node_color, interval_compare,
    set_node_stickiness,
};
use je_editor::model::TrackedRangeStickiness;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Interval {
    start: i64,
    end: i64,
}

/// `Oracle`: a lista ordenada de intervalos contra a qual a árvore é conferida. Os intervalos são
/// identificados pelo índice em `slots` (a identidade de objeto do original).
#[derive(Default)]
struct Oracle {
    slots: Vec<Interval>,
    intervals: Vec<usize>,
}

impl Oracle {
    fn sort(&mut self) {
        let slots = &self.slots;
        // `Array.prototype.sort` é estável, como `sort_by`.
        self.intervals.sort_by(|&a, &b| {
            let (a, b) = (slots[a], slots[b]);
            interval_compare(a.start, a.end, b.start, b.end).cmp(&0)
        });
    }

    fn insert(&mut self, slot: usize) {
        self.intervals.push(slot);
        self.sort();
    }

    fn delete(&mut self, slot: usize) {
        if let Some(i) = self.intervals.iter().position(|&s| s == slot) {
            self.intervals.remove(i);
        }
    }

    fn search(&self, interval: Interval) -> Vec<Interval> {
        self.intervals
            .iter()
            .map(|&s| self.slots[s])
            .filter(|int| int.start <= interval.end && int.end >= interval.start)
            .collect()
    }

    fn all(&self) -> Vec<Interval> {
        self.intervals.iter().map(|&s| self.slots[s]).collect()
    }
}

enum Op {
    Insert { begin: i64, end: i64 },
    Delete { id: usize },
    Search { begin: i64, end: i64 },
}

use Op::{Delete, Insert, Search};

/// `TestState`.
#[derive(Default)]
struct TestState {
    arena: IntervalArena,
    tree: IntervalTree,
    oracle: Oracle,
    tree_nodes: Vec<Option<NodeId>>,
    oracle_nodes: Vec<Option<usize>>,
}

impl TestState {
    fn cached(&self, nodes: &[NodeId]) -> Vec<Interval> {
        nodes
            .iter()
            .map(|&n| {
                let node = self.arena.get(n);
                Interval { start: node.cached_absolute_start, end: node.cached_absolute_end }
            })
            .collect()
    }

    fn accept_op(&mut self, op: &Op) {
        match *op {
            Insert { begin, end } => {
                let node = self.arena.alloc(String::new(), begin, end);
                self.tree.insert(&mut self.arena, node);
                self.tree_nodes.push(Some(node));
                self.oracle.slots.push(Interval { start: begin, end });
                let slot = self.oracle.slots.len() - 1;
                self.oracle.insert(slot);
                self.oracle_nodes.push(Some(slot));
            }
            Delete { id } => {
                let node = self.tree_nodes[id].expect("nó vivo");
                self.tree.delete(&mut self.arena, node);
                self.oracle.delete(self.oracle_nodes[id].expect("intervalo vivo"));
                self.tree_nodes[id] = None;
                self.oracle_nodes[id] = None;
            }
            Search { begin, end } => {
                let actual_nodes = self.tree.interval_search(&mut self.arena, begin, end, 0, false, false, 0, false);
                let actual = self.cached(&actual_nodes);
                let expected = self.oracle.search(Interval { start: begin, end });
                assert_eq!(actual, expected);
                return;
            }
        }

        assert_tree_invariants(&self.arena, &self.tree);

        let all = self.tree.get_all_in_order(&mut self.arena);
        assert_eq!(self.cached(&all), self.oracle.all());
    }
}

fn test_interval_tree(ops: &[Op]) {
    let mut state = TestState::default();
    for op in ops {
        state.accept_op(op);
    }
}

// suite('generated')

#[test]
fn gen01() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[Insert { begin: 28, end: 35 }, Insert { begin: 52, end: 54 }, Insert { begin: 63, end: 69 }]);
}

#[test]
fn gen02() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[Insert { begin: 80, end: 89 }, Insert { begin: 92, end: 100 }, Insert { begin: 99, end: 99 }]);
}

#[test]
fn gen03() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[Insert { begin: 89, end: 96 }, Insert { begin: 71, end: 74 }, Delete { id: 1 }]);
}

#[test]
fn gen04() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[Insert { begin: 44, end: 46 }, Insert { begin: 85, end: 88 }, Delete { id: 0 }]);
}

#[test]
fn gen05() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 82, end: 90 },
        Insert { begin: 69, end: 73 },
        Delete { id: 0 },
        Delete { id: 1 },
    ]);
}

#[test]
fn gen06() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 41, end: 63 },
        Insert { begin: 98, end: 98 },
        Insert { begin: 47, end: 51 },
        Delete { id: 2 },
    ]);
}

#[test]
fn gen07() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 24, end: 26 },
        Insert { begin: 11, end: 28 },
        Insert { begin: 27, end: 30 },
        Insert { begin: 80, end: 85 },
        Delete { id: 1 },
    ]);
}

#[test]
fn gen08() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[Insert { begin: 100, end: 100 }, Insert { begin: 100, end: 100 }]);
}

#[test]
fn gen09() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[Insert { begin: 58, end: 65 }, Insert { begin: 82, end: 96 }, Insert { begin: 58, end: 65 }]);
}

#[test]
fn gen10() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[Insert { begin: 32, end: 40 }, Insert { begin: 25, end: 29 }, Insert { begin: 24, end: 32 }]);
}

#[test]
fn gen11() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 25, end: 70 },
        Insert { begin: 99, end: 100 },
        Insert { begin: 46, end: 51 },
        Insert { begin: 57, end: 57 },
        Delete { id: 2 },
    ]);
}

#[test]
fn gen12() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 20, end: 26 },
        Insert { begin: 10, end: 18 },
        Insert { begin: 99, end: 99 },
        Insert { begin: 37, end: 59 },
        Delete { id: 2 },
    ]);
}

#[test]
fn gen13() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 3, end: 91 },
        Insert { begin: 57, end: 57 },
        Insert { begin: 35, end: 44 },
        Insert { begin: 72, end: 81 },
        Delete { id: 2 },
    ]);
}

#[test]
fn gen14() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 58, end: 61 },
        Insert { begin: 34, end: 35 },
        Insert { begin: 56, end: 62 },
        Insert { begin: 69, end: 78 },
        Delete { id: 0 },
    ]);
}

#[test]
fn gen15() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 63, end: 69 },
        Insert { begin: 17, end: 24 },
        Insert { begin: 3, end: 13 },
        Insert { begin: 84, end: 94 },
        Insert { begin: 18, end: 23 },
        Insert { begin: 96, end: 98 },
        Delete { id: 1 },
    ]);
}

#[test]
fn gen16() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 27, end: 27 },
        Insert { begin: 42, end: 87 },
        Insert { begin: 42, end: 49 },
        Insert { begin: 69, end: 71 },
        Insert { begin: 20, end: 27 },
        Insert { begin: 8, end: 9 },
        Insert { begin: 42, end: 49 },
        Delete { id: 1 },
    ]);
}

#[test]
fn gen17() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 21, end: 23 },
        Insert { begin: 83, end: 87 },
        Insert { begin: 56, end: 58 },
        Insert { begin: 1, end: 55 },
        Insert { begin: 56, end: 59 },
        Insert { begin: 58, end: 60 },
        Insert { begin: 56, end: 65 },
        Delete { id: 1 },
        Delete { id: 0 },
        Delete { id: 6 },
    ]);
}

#[test]
fn gen18() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_interval_tree(&[
        Insert { begin: 25, end: 25 },
        Insert { begin: 67, end: 79 },
        Delete { id: 0 },
        Search { begin: 65, end: 75 },
    ]);
}

#[test]
fn force_delta_overflow() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // O original manda procurar FORCE_OVERFLOWING_TEST no construtor de IntervalNode para forçar a
    // normalização do delta neste caso; com o construtor normal (o portado), o caso roda como os outros.
    test_interval_tree(&[
        Insert { begin: 686081138593427, end: 733009856502260 },
        Insert { begin: 591031326181669, end: 591031326181672 },
        Insert { begin: 940037682731896, end: 940037682731903 },
        Insert { begin: 598413641151120, end: 598413641151128 },
        Insert { begin: 800564156553344, end: 800564156553351 },
        Insert { begin: 894198957565481, end: 894198957565491 },
    ]);
}

// suite('searching')

fn create_cormen_tree() -> (IntervalArena, IntervalTree) {
    let mut arena = IntervalArena::new();
    let mut r = IntervalTree::new();
    let data: [(i64, i64); 10] =
        [(16, 21), (8, 9), (25, 30), (5, 8), (15, 23), (17, 19), (26, 26), (0, 3), (6, 10), (19, 20)];
    for (start, end) in data {
        let node = arena.alloc(String::new(), start, end);
        r.insert(&mut arena, node);
    }
    (arena, r)
}

fn assert_interval_search(start: i64, end: i64, expected: &[(i64, i64)]) {
    let (mut arena, t) = create_cormen_tree();
    let actual_nodes = t.interval_search(&mut arena, start, end, 0, false, false, 0, false);
    let actual: Vec<(i64, i64)> = actual_nodes
        .iter()
        .map(|&n| (arena.get(n).cached_absolute_start, arena.get(n).cached_absolute_end))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn cormen_1_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_interval_search(1, 2, &[(0, 3)]);
}

#[test]
fn cormen_4_8() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_interval_search(4, 8, &[(5, 8), (6, 10), (8, 9)]);
}

#[test]
fn cormen_10_15() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_interval_search(10, 15, &[(6, 10), (15, 23)]);
}

#[test]
fn cormen_21_25() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_interval_search(21, 25, &[(15, 23), (16, 21), (25, 30)]);
}

#[test]
fn cormen_24_24() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_interval_search(24, 24, &[]);
}

// suite('IntervalTree 2')

use TrackedRangeStickiness::{
    AlwaysGrowsWhenTypingAtEdges as Always, GrowsOnlyWhenTypingAfter as After, GrowsOnlyWhenTypingBefore as Before,
    NeverGrowsWhenTypingAtEdges as Never,
};

#[allow(clippy::too_many_arguments)]
fn assert_node_accept_edit(
    msg: &str,
    node_start: i64,
    node_end: i64,
    node_stickiness: TrackedRangeStickiness,
    start: i64,
    end: i64,
    text_length: i64,
    force_move_markers: bool,
    expected_node_start: i64,
    expected_node_end: i64,
) {
    let mut arena = IntervalArena::new();
    let node = arena.alloc(String::new(), node_start, node_end);
    set_node_stickiness(arena.get_mut(node), node_stickiness);
    interval_tree::node_accept_edit(&mut arena, node, start, end, text_length, force_move_markers);
    let n = arena.get(node);
    assert_eq!([n.start, n.end], [expected_node_start, expected_node_end], "{msg}");
}

#[test]
fn node_accept_edit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let f = false;
    let t = true;
    // A. decoração colapsada
    {
        // nada muda
        assert_node_accept_edit("A.000", 0, 0, Always, 0, 0, 0, f, 0, 0);
        assert_node_accept_edit("A.001", 0, 0, Never, 0, 0, 0, f, 0, 0);
        assert_node_accept_edit("A.002", 0, 0, Before, 0, 0, 0, f, 0, 0);
        assert_node_accept_edit("A.003", 0, 0, After, 0, 0, 0, f, 0, 0);
        assert_node_accept_edit("A.004", 0, 0, Always, 0, 0, 0, t, 0, 0);
        assert_node_accept_edit("A.005", 0, 0, Never, 0, 0, 0, t, 0, 0);
        assert_node_accept_edit("A.006", 0, 0, Before, 0, 0, 0, t, 0, 0);
        assert_node_accept_edit("A.007", 0, 0, After, 0, 0, 0, t, 0, 0);
        // inserção
        assert_node_accept_edit("A.008", 0, 0, Always, 0, 0, 1, f, 0, 1);
        assert_node_accept_edit("A.009", 0, 0, Never, 0, 0, 1, f, 1, 1);
        assert_node_accept_edit("A.010", 0, 0, Before, 0, 0, 1, f, 0, 0);
        assert_node_accept_edit("A.011", 0, 0, After, 0, 0, 1, f, 1, 1);
        assert_node_accept_edit("A.012", 0, 0, Always, 0, 0, 1, t, 1, 1);
        assert_node_accept_edit("A.013", 0, 0, Never, 0, 0, 1, t, 1, 1);
        assert_node_accept_edit("A.014", 0, 0, Before, 0, 0, 1, t, 1, 1);
        assert_node_accept_edit("A.015", 0, 0, After, 0, 0, 1, t, 1, 1);
    }

    // B. decoração não colapsada
    {
        // nada muda
        assert_node_accept_edit("B.000", 0, 5, Always, 0, 0, 0, f, 0, 5);
        assert_node_accept_edit("B.001", 0, 5, Never, 0, 0, 0, f, 0, 5);
        assert_node_accept_edit("B.002", 0, 5, Before, 0, 0, 0, f, 0, 5);
        assert_node_accept_edit("B.003", 0, 5, After, 0, 0, 0, f, 0, 5);
        assert_node_accept_edit("B.004", 0, 5, Always, 0, 0, 0, t, 0, 5);
        assert_node_accept_edit("B.005", 0, 5, Never, 0, 0, 0, t, 0, 5);
        assert_node_accept_edit("B.006", 0, 5, Before, 0, 0, 0, t, 0, 5);
        assert_node_accept_edit("B.007", 0, 5, After, 0, 0, 0, t, 0, 5);
        // inserção no início
        assert_node_accept_edit("B.008", 0, 5, Always, 0, 0, 1, f, 0, 6);
        assert_node_accept_edit("B.009", 0, 5, Never, 0, 0, 1, f, 1, 6);
        assert_node_accept_edit("B.010", 0, 5, Before, 0, 0, 1, f, 0, 6);
        assert_node_accept_edit("B.011", 0, 5, After, 0, 0, 1, f, 1, 6);
        assert_node_accept_edit("B.012", 0, 5, Always, 0, 0, 1, t, 1, 6);
        assert_node_accept_edit("B.013", 0, 5, Never, 0, 0, 1, t, 1, 6);
        assert_node_accept_edit("B.014", 0, 5, Before, 0, 0, 1, t, 1, 6);
        assert_node_accept_edit("B.015", 0, 5, After, 0, 0, 1, t, 1, 6);
        // inserção no meio
        assert_node_accept_edit("B.016", 0, 5, Always, 2, 2, 1, f, 0, 6);
        assert_node_accept_edit("B.017", 0, 5, Never, 2, 2, 1, f, 0, 6);
        assert_node_accept_edit("B.018", 0, 5, Before, 2, 2, 1, f, 0, 6);
        assert_node_accept_edit("B.019", 0, 5, After, 2, 2, 1, f, 0, 6);
        assert_node_accept_edit("B.020", 0, 5, Always, 2, 2, 1, t, 0, 6);
        assert_node_accept_edit("B.021", 0, 5, Never, 2, 2, 1, t, 0, 6);
        assert_node_accept_edit("B.022", 0, 5, Before, 2, 2, 1, t, 0, 6);
        assert_node_accept_edit("B.023", 0, 5, After, 2, 2, 1, t, 0, 6);
        // inserção no fim
        assert_node_accept_edit("B.024", 0, 5, Always, 5, 5, 1, f, 0, 6);
        assert_node_accept_edit("B.025", 0, 5, Never, 5, 5, 1, f, 0, 5);
        assert_node_accept_edit("B.026", 0, 5, Before, 5, 5, 1, f, 0, 5);
        assert_node_accept_edit("B.027", 0, 5, After, 5, 5, 1, f, 0, 6);
        assert_node_accept_edit("B.028", 0, 5, Always, 5, 5, 1, t, 0, 6);
        assert_node_accept_edit("B.029", 0, 5, Never, 5, 5, 1, t, 0, 6);
        assert_node_accept_edit("B.030", 0, 5, Before, 5, 5, 1, t, 0, 6);
        assert_node_accept_edit("B.031", 0, 5, After, 5, 5, 1, t, 0, 6);

        // troca por texto maior até o início
        assert_node_accept_edit("B.032", 5, 10, Always, 4, 5, 2, f, 5, 11);
        assert_node_accept_edit("B.033", 5, 10, Never, 4, 5, 2, f, 6, 11);
        assert_node_accept_edit("B.034", 5, 10, Before, 4, 5, 2, f, 5, 11);
        assert_node_accept_edit("B.035", 5, 10, After, 4, 5, 2, f, 6, 11);
        assert_node_accept_edit("B.036", 5, 10, Always, 4, 5, 2, t, 6, 11);
        assert_node_accept_edit("B.037", 5, 10, Never, 4, 5, 2, t, 6, 11);
        assert_node_accept_edit("B.038", 5, 10, Before, 4, 5, 2, t, 6, 11);
        assert_node_accept_edit("B.039", 5, 10, After, 4, 5, 2, t, 6, 11);
        // troca por texto menor até o início
        assert_node_accept_edit("B.040", 5, 10, Always, 3, 5, 1, f, 4, 9);
        assert_node_accept_edit("B.041", 5, 10, Never, 3, 5, 1, f, 4, 9);
        assert_node_accept_edit("B.042", 5, 10, Before, 3, 5, 1, f, 4, 9);
        assert_node_accept_edit("B.043", 5, 10, After, 3, 5, 1, f, 4, 9);
        assert_node_accept_edit("B.044", 5, 10, Always, 3, 5, 1, t, 4, 9);
        assert_node_accept_edit("B.045", 5, 10, Never, 3, 5, 1, t, 4, 9);
        assert_node_accept_edit("B.046", 5, 10, Before, 3, 5, 1, t, 4, 9);
        assert_node_accept_edit("B.047", 5, 10, After, 3, 5, 1, t, 4, 9);

        // troca por texto maior selecionando o início
        assert_node_accept_edit("B.048", 5, 10, Always, 4, 6, 3, f, 5, 11);
        assert_node_accept_edit("B.049", 5, 10, Never, 4, 6, 3, f, 5, 11);
        assert_node_accept_edit("B.050", 5, 10, Before, 4, 6, 3, f, 5, 11);
        assert_node_accept_edit("B.051", 5, 10, After, 4, 6, 3, f, 5, 11);
        assert_node_accept_edit("B.052", 5, 10, Always, 4, 6, 3, t, 7, 11);
        assert_node_accept_edit("B.053", 5, 10, Never, 4, 6, 3, t, 7, 11);
        assert_node_accept_edit("B.054", 5, 10, Before, 4, 6, 3, t, 7, 11);
        assert_node_accept_edit("B.055", 5, 10, After, 4, 6, 3, t, 7, 11);
        // troca por texto menor selecionando o início
        assert_node_accept_edit("B.056", 5, 10, Always, 4, 6, 1, f, 5, 9);
        assert_node_accept_edit("B.057", 5, 10, Never, 4, 6, 1, f, 5, 9);
        assert_node_accept_edit("B.058", 5, 10, Before, 4, 6, 1, f, 5, 9);
        assert_node_accept_edit("B.059", 5, 10, After, 4, 6, 1, f, 5, 9);
        assert_node_accept_edit("B.060", 5, 10, Always, 4, 6, 1, t, 5, 9);
        assert_node_accept_edit("B.061", 5, 10, Never, 4, 6, 1, t, 5, 9);
        assert_node_accept_edit("B.062", 5, 10, Before, 4, 6, 1, t, 5, 9);
        assert_node_accept_edit("B.063", 5, 10, After, 4, 6, 1, t, 5, 9);

        // troca por texto maior a partir do início
        assert_node_accept_edit("B.064", 5, 10, Always, 5, 6, 2, f, 5, 11);
        assert_node_accept_edit("B.065", 5, 10, Never, 5, 6, 2, f, 5, 11);
        assert_node_accept_edit("B.066", 5, 10, Before, 5, 6, 2, f, 5, 11);
        assert_node_accept_edit("B.067", 5, 10, After, 5, 6, 2, f, 5, 11);
        assert_node_accept_edit("B.068", 5, 10, Always, 5, 6, 2, t, 7, 11);
        assert_node_accept_edit("B.069", 5, 10, Never, 5, 6, 2, t, 7, 11);
        assert_node_accept_edit("B.070", 5, 10, Before, 5, 6, 2, t, 7, 11);
        assert_node_accept_edit("B.071", 5, 10, After, 5, 6, 2, t, 7, 11);
        // troca por texto menor a partir do início
        assert_node_accept_edit("B.072", 5, 10, Always, 5, 7, 1, f, 5, 9);
        assert_node_accept_edit("B.073", 5, 10, Never, 5, 7, 1, f, 5, 9);
        assert_node_accept_edit("B.074", 5, 10, Before, 5, 7, 1, f, 5, 9);
        assert_node_accept_edit("B.075", 5, 10, After, 5, 7, 1, f, 5, 9);
        assert_node_accept_edit("B.076", 5, 10, Always, 5, 7, 1, t, 6, 9);
        assert_node_accept_edit("B.077", 5, 10, Never, 5, 7, 1, t, 6, 9);
        assert_node_accept_edit("B.078", 5, 10, Before, 5, 7, 1, t, 6, 9);
        assert_node_accept_edit("B.079", 5, 10, After, 5, 7, 1, t, 6, 9);

        // troca por texto maior até o fim
        assert_node_accept_edit("B.080", 5, 10, Always, 9, 10, 2, f, 5, 11);
        assert_node_accept_edit("B.081", 5, 10, Never, 9, 10, 2, f, 5, 10);
        assert_node_accept_edit("B.082", 5, 10, Before, 9, 10, 2, f, 5, 10);
        assert_node_accept_edit("B.083", 5, 10, After, 9, 10, 2, f, 5, 11);
        assert_node_accept_edit("B.084", 5, 10, Always, 9, 10, 2, t, 5, 11);
        assert_node_accept_edit("B.085", 5, 10, Never, 9, 10, 2, t, 5, 11);
        assert_node_accept_edit("B.086", 5, 10, Before, 9, 10, 2, t, 5, 11);
        assert_node_accept_edit("B.087", 5, 10, After, 9, 10, 2, t, 5, 11);
        // troca por texto menor até o fim
        assert_node_accept_edit("B.088", 5, 10, Always, 8, 10, 1, f, 5, 9);
        assert_node_accept_edit("B.089", 5, 10, Never, 8, 10, 1, f, 5, 9);
        assert_node_accept_edit("B.090", 5, 10, Before, 8, 10, 1, f, 5, 9);
        assert_node_accept_edit("B.091", 5, 10, After, 8, 10, 1, f, 5, 9);
        assert_node_accept_edit("B.092", 5, 10, Always, 8, 10, 1, t, 5, 9);
        assert_node_accept_edit("B.093", 5, 10, Never, 8, 10, 1, t, 5, 9);
        assert_node_accept_edit("B.094", 5, 10, Before, 8, 10, 1, t, 5, 9);
        assert_node_accept_edit("B.095", 5, 10, After, 8, 10, 1, t, 5, 9);

        // troca por texto maior selecionando o fim
        assert_node_accept_edit("B.096", 5, 10, Always, 9, 11, 3, f, 5, 10);
        assert_node_accept_edit("B.097", 5, 10, Never, 9, 11, 3, f, 5, 10);
        assert_node_accept_edit("B.098", 5, 10, Before, 9, 11, 3, f, 5, 10);
        assert_node_accept_edit("B.099", 5, 10, After, 9, 11, 3, f, 5, 10);
        assert_node_accept_edit("B.100", 5, 10, Always, 9, 11, 3, t, 5, 12);
        assert_node_accept_edit("B.101", 5, 10, Never, 9, 11, 3, t, 5, 12);
        assert_node_accept_edit("B.102", 5, 10, Before, 9, 11, 3, t, 5, 12);
        assert_node_accept_edit("B.103", 5, 10, After, 9, 11, 3, t, 5, 12);
        // troca por texto menor selecionando o fim
        assert_node_accept_edit("B.104", 5, 10, Always, 9, 11, 1, f, 5, 10);
        assert_node_accept_edit("B.105", 5, 10, Never, 9, 11, 1, f, 5, 10);
        assert_node_accept_edit("B.106", 5, 10, Before, 9, 11, 1, f, 5, 10);
        assert_node_accept_edit("B.107", 5, 10, After, 9, 11, 1, f, 5, 10);
        assert_node_accept_edit("B.108", 5, 10, Always, 9, 11, 1, t, 5, 10);
        assert_node_accept_edit("B.109", 5, 10, Never, 9, 11, 1, t, 5, 10);
        assert_node_accept_edit("B.110", 5, 10, Before, 9, 11, 1, t, 5, 10);
        assert_node_accept_edit("B.111", 5, 10, After, 9, 11, 1, t, 5, 10);

        // troca por texto maior a partir do fim
        assert_node_accept_edit("B.112", 5, 10, Always, 10, 11, 3, f, 5, 10);
        assert_node_accept_edit("B.113", 5, 10, Never, 10, 11, 3, f, 5, 10);
        assert_node_accept_edit("B.114", 5, 10, Before, 10, 11, 3, f, 5, 10);
        assert_node_accept_edit("B.115", 5, 10, After, 10, 11, 3, f, 5, 10);
        assert_node_accept_edit("B.116", 5, 10, Always, 10, 11, 3, t, 5, 13);
        assert_node_accept_edit("B.117", 5, 10, Never, 10, 11, 3, t, 5, 13);
        assert_node_accept_edit("B.118", 5, 10, Before, 10, 11, 3, t, 5, 13);
        assert_node_accept_edit("B.119", 5, 10, After, 10, 11, 3, t, 5, 13);
        // troca por texto menor a partir do fim
        assert_node_accept_edit("B.120", 5, 10, Always, 10, 12, 1, f, 5, 10);
        assert_node_accept_edit("B.121", 5, 10, Never, 10, 12, 1, f, 5, 10);
        assert_node_accept_edit("B.122", 5, 10, Before, 10, 12, 1, f, 5, 10);
        assert_node_accept_edit("B.123", 5, 10, After, 10, 12, 1, f, 5, 10);
        assert_node_accept_edit("B.124", 5, 10, Always, 10, 12, 1, t, 5, 11);
        assert_node_accept_edit("B.125", 5, 10, Never, 10, 12, 1, t, 5, 11);
        assert_node_accept_edit("B.126", 5, 10, Before, 10, 12, 1, t, 5, 11);
        assert_node_accept_edit("B.127", 5, 10, After, 10, 12, 1, t, 5, 11);

        // apaga até o início
        assert_node_accept_edit("B.128", 5, 10, Always, 4, 5, 0, f, 4, 9);
        assert_node_accept_edit("B.129", 5, 10, Never, 4, 5, 0, f, 4, 9);
        assert_node_accept_edit("B.130", 5, 10, Before, 4, 5, 0, f, 4, 9);
        assert_node_accept_edit("B.131", 5, 10, After, 4, 5, 0, f, 4, 9);
        assert_node_accept_edit("B.132", 5, 10, Always, 4, 5, 0, t, 4, 9);
        assert_node_accept_edit("B.133", 5, 10, Never, 4, 5, 0, t, 4, 9);
        assert_node_accept_edit("B.134", 5, 10, Before, 4, 5, 0, t, 4, 9);
        assert_node_accept_edit("B.135", 5, 10, After, 4, 5, 0, t, 4, 9);

        // apaga selecionando o início
        assert_node_accept_edit("B.136", 5, 10, Always, 4, 6, 0, f, 4, 8);
        assert_node_accept_edit("B.137", 5, 10, Never, 4, 6, 0, f, 4, 8);
        assert_node_accept_edit("B.138", 5, 10, Before, 4, 6, 0, f, 4, 8);
        assert_node_accept_edit("B.139", 5, 10, After, 4, 6, 0, f, 4, 8);
        assert_node_accept_edit("B.140", 5, 10, Always, 4, 6, 0, t, 4, 8);
        assert_node_accept_edit("B.141", 5, 10, Never, 4, 6, 0, t, 4, 8);
        assert_node_accept_edit("B.142", 5, 10, Before, 4, 6, 0, t, 4, 8);
        assert_node_accept_edit("B.143", 5, 10, After, 4, 6, 0, t, 4, 8);

        // apaga a partir do início
        assert_node_accept_edit("B.144", 5, 10, Always, 5, 6, 0, f, 5, 9);
        assert_node_accept_edit("B.145", 5, 10, Never, 5, 6, 0, f, 5, 9);
        assert_node_accept_edit("B.146", 5, 10, Before, 5, 6, 0, f, 5, 9);
        assert_node_accept_edit("B.147", 5, 10, After, 5, 6, 0, f, 5, 9);
        assert_node_accept_edit("B.148", 5, 10, Always, 5, 6, 0, t, 5, 9);
        assert_node_accept_edit("B.149", 5, 10, Never, 5, 6, 0, t, 5, 9);
        assert_node_accept_edit("B.150", 5, 10, Before, 5, 6, 0, t, 5, 9);
        assert_node_accept_edit("B.151", 5, 10, After, 5, 6, 0, t, 5, 9);

        // apaga até o fim
        assert_node_accept_edit("B.152", 5, 10, Always, 9, 10, 0, f, 5, 9);
        assert_node_accept_edit("B.153", 5, 10, Never, 9, 10, 0, f, 5, 9);
        assert_node_accept_edit("B.154", 5, 10, Before, 9, 10, 0, f, 5, 9);
        assert_node_accept_edit("B.155", 5, 10, After, 9, 10, 0, f, 5, 9);
        assert_node_accept_edit("B.156", 5, 10, Always, 9, 10, 0, t, 5, 9);
        assert_node_accept_edit("B.157", 5, 10, Never, 9, 10, 0, t, 5, 9);
        assert_node_accept_edit("B.158", 5, 10, Before, 9, 10, 0, t, 5, 9);
        assert_node_accept_edit("B.159", 5, 10, After, 9, 10, 0, t, 5, 9);

        // apaga selecionando o fim
        assert_node_accept_edit("B.160", 5, 10, Always, 9, 11, 0, f, 5, 9);
        assert_node_accept_edit("B.161", 5, 10, Never, 9, 11, 0, f, 5, 9);
        assert_node_accept_edit("B.162", 5, 10, Before, 9, 11, 0, f, 5, 9);
        assert_node_accept_edit("B.163", 5, 10, After, 9, 11, 0, f, 5, 9);
        assert_node_accept_edit("B.164", 5, 10, Always, 9, 11, 0, t, 5, 9);
        assert_node_accept_edit("B.165", 5, 10, Never, 9, 11, 0, t, 5, 9);
        assert_node_accept_edit("B.166", 5, 10, Before, 9, 11, 0, t, 5, 9);
        assert_node_accept_edit("B.167", 5, 10, After, 9, 11, 0, t, 5, 9);

        // apaga a partir do fim
        assert_node_accept_edit("B.168", 5, 10, Always, 10, 11, 0, f, 5, 10);
        assert_node_accept_edit("B.169", 5, 10, Never, 10, 11, 0, f, 5, 10);
        assert_node_accept_edit("B.170", 5, 10, Before, 10, 11, 0, f, 5, 10);
        assert_node_accept_edit("B.171", 5, 10, After, 10, 11, 0, f, 5, 10);
        assert_node_accept_edit("B.172", 5, 10, Always, 10, 11, 0, t, 5, 10);
        assert_node_accept_edit("B.173", 5, 10, Never, 10, 11, 0, t, 5, 10);
        assert_node_accept_edit("B.174", 5, 10, Before, 10, 11, 0, t, 5, 10);
        assert_node_accept_edit("B.175", 5, 10, After, 10, 11, 0, t, 5, 10);

        // troca o trecho inteiro por texto maior
        assert_node_accept_edit("B.176", 5, 10, Always, 5, 10, 3, f, 5, 8);
        assert_node_accept_edit("B.177", 5, 10, Never, 5, 10, 3, f, 5, 8);
        assert_node_accept_edit("B.178", 5, 10, Before, 5, 10, 3, f, 5, 8);
        assert_node_accept_edit("B.179", 5, 10, After, 5, 10, 3, f, 5, 8);
        assert_node_accept_edit("B.180", 5, 10, Always, 5, 10, 3, t, 8, 8);
        assert_node_accept_edit("B.181", 5, 10, Never, 5, 10, 3, t, 8, 8);
        assert_node_accept_edit("B.182", 5, 10, Before, 5, 10, 3, t, 8, 8);
        assert_node_accept_edit("B.183", 5, 10, After, 5, 10, 3, t, 8, 8);
        // troca o trecho inteiro por texto menor
        assert_node_accept_edit("B.184", 5, 10, Always, 5, 10, 7, f, 5, 12);
        assert_node_accept_edit("B.185", 5, 10, Never, 5, 10, 7, f, 5, 10);
        assert_node_accept_edit("B.186", 5, 10, Before, 5, 10, 7, f, 5, 10);
        assert_node_accept_edit("B.187", 5, 10, After, 5, 10, 7, f, 5, 12);
        assert_node_accept_edit("B.188", 5, 10, Always, 5, 10, 7, t, 12, 12);
        assert_node_accept_edit("B.189", 5, 10, Never, 5, 10, 7, t, 12, 12);
        assert_node_accept_edit("B.190", 5, 10, Before, 5, 10, 7, t, 12, 12);
        assert_node_accept_edit("B.191", 5, 10, After, 5, 10, 7, t, 12, 12);
    }
}

//#region Asserções

fn assert_tree_invariants(arena: &IntervalArena, t: &IntervalTree) {
    let sentinel = arena.get(SENTINEL);
    assert_eq!(get_node_color(sentinel), NodeColor::Black);
    assert_eq!(sentinel.parent, SENTINEL);
    assert_eq!(sentinel.left, SENTINEL);
    assert_eq!(sentinel.right, SENTINEL);
    assert_eq!(sentinel.start, 0);
    assert_eq!(sentinel.end, 0);
    assert_eq!(sentinel.delta, 0);
    assert_eq!(arena.get(t.root).parent, SENTINEL);
    assert_valid_tree(arena, t);
}

fn depth(arena: &IntervalArena, n: NodeId) -> i32 {
    if n == SENTINEL {
        // as folhas são pretas
        return 1;
    }
    let node = arena.get(n);
    assert_eq!(depth(arena, node.left), depth(arena, node.right));
    i32::from(get_node_color(node) == NodeColor::Black) + depth(arena, node.left)
}

fn assert_valid_node(arena: &IntervalArena, n: NodeId, delta: i64) {
    if n == SENTINEL {
        return;
    }

    let node: &IntervalNode = arena.get(n);
    let (l, r) = (arena.get(node.left), arena.get(node.right));

    if get_node_color(node) == NodeColor::Red {
        assert_eq!(get_node_color(l), NodeColor::Black);
        assert_eq!(get_node_color(r), NodeColor::Black);
    }

    let mut expected_max_end = node.end;
    if node.left != SENTINEL {
        assert!(interval_compare(l.start + delta, l.end + delta, node.start + delta, node.end + delta) <= 0);
        expected_max_end = expected_max_end.max(l.max_end);
    }
    if node.right != SENTINEL {
        assert!(
            interval_compare(
                node.start + delta,
                node.end + delta,
                r.start + delta + node.delta,
                r.end + delta + node.delta
            ) <= 0
        );
        expected_max_end = expected_max_end.max(r.max_end + node.delta);
    }
    assert_eq!(node.max_end, expected_max_end);

    assert_valid_node(arena, node.left, delta);
    assert_valid_node(arena, node.right, delta + node.delta);
}

fn assert_valid_tree(arena: &IntervalArena, t: &IntervalTree) {
    if t.root == SENTINEL {
        return;
    }
    let root = arena.get(t.root);
    assert_eq!(get_node_color(root), NodeColor::Black);
    assert_eq!(depth(arena, root.left), depth(arena, root.right));
    assert_valid_node(arena, t.root, 0);
}

//#endregion
