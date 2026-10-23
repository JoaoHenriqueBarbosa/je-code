//! Porte de `src/vs/editor/contrib/inlineCompletions/test/browser/graph.test.ts` (suíte `DirectedGraph`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `getOutgoing` com `switch` vira `match` sobre o nó; os nós de texto são `&str`.
//! - `removeCycles()` devolve `{ foundCycles }` no original; aqui devolve o `Vec` de `foundCycles` direto.
//! - `[...x].sort()` vira `sort()` sobre o `Vec` devolvido.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::inline_completions::graph::DirectedGraph;

#[test]
fn from_creates_empty_graph() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let graph = DirectedGraph::<&str>::from(&[], |_| vec![]);
    assert_eq!(graph.get_outgoing(&"a"), Vec::<&str>::new());
}

#[test]
fn from_creates_graph_with_single_node() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let graph = DirectedGraph::from(&["a"], |_| vec![]);
    assert_eq!(graph.get_outgoing(&"a"), Vec::<&str>::new());
}

#[test]
fn from_creates_graph_with_nodes_and_edges() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let nodes = ["a", "b", "c"];
    let get_outgoing = |node: &&str| match *node {
        "a" => vec!["b", "c"],
        "b" => vec!["c"],
        "c" => vec![],
        _ => vec![],
    };

    let graph = DirectedGraph::from(&nodes, get_outgoing);

    let mut a = graph.get_outgoing(&"a");
    a.sort();
    assert_eq!(a, vec!["b", "c"]);
    assert_eq!(graph.get_outgoing(&"b"), vec!["c"]);
    assert_eq!(graph.get_outgoing(&"c"), Vec::<&str>::new());
}

#[test]
fn from_handles_duplicate_edges() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let nodes = ["a", "b"];
    let get_outgoing = |node: &&str| match *node {
        "a" => vec!["b", "b"], // Duplicate edge
        "b" => vec![],
        _ => vec![],
    };

    let graph = DirectedGraph::from(&nodes, get_outgoing);

    assert_eq!(graph.get_outgoing(&"a"), vec!["b"]);
    assert_eq!(graph.get_outgoing(&"b"), Vec::<&str>::new());
}

#[test]
fn remove_cycles_no_cycles() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let nodes = ["a", "b", "c"];
    let get_outgoing = |node: &&str| match *node {
        "a" => vec!["b"],
        "b" => vec!["c"],
        "c" => vec![],
        _ => vec![],
    };

    let mut graph = DirectedGraph::from(&nodes, get_outgoing);
    let found_cycles = graph.remove_cycles();

    assert_eq!(found_cycles, Vec::<&str>::new());
    assert_eq!(graph.get_outgoing(&"a"), vec!["b"]);
    assert_eq!(graph.get_outgoing(&"b"), vec!["c"]);
    assert_eq!(graph.get_outgoing(&"c"), Vec::<&str>::new());
}

#[test]
fn remove_cycles_simple_cycle() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let nodes = ["a", "b"];
    let get_outgoing = |node: &&str| match *node {
        "a" => vec!["b"],
        "b" => vec!["a"], // Creates cycle
        _ => vec![],
    };

    let mut graph = DirectedGraph::from(&nodes, get_outgoing);
    let found_cycles = graph.remove_cycles();

    assert_eq!(found_cycles.len(), 1);
    assert!(found_cycles.contains(&"a") || found_cycles.contains(&"b"));

    // After removing cycles, one of the edges should be removed
    let a_outgoing = graph.get_outgoing(&"a");
    let b_outgoing = graph.get_outgoing(&"b");
    assert!(
        (a_outgoing.is_empty() && b_outgoing.len() == 1) || (a_outgoing.len() == 1 && b_outgoing.is_empty())
    );
}

#[test]
fn remove_cycles_self_loop() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let nodes = ["a"];
    let get_outgoing = |node: &&str| match *node {
        "a" => vec!["a"], // Self loop
        _ => vec![],
    };

    let mut graph = DirectedGraph::from(&nodes, get_outgoing);
    let found_cycles = graph.remove_cycles();

    assert_eq!(found_cycles, vec!["a"]);
    assert_eq!(graph.get_outgoing(&"a"), Vec::<&str>::new());
}

#[test]
fn remove_cycles_complex_cycle() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let nodes = ["a", "b", "c", "d"];
    let get_outgoing = |node: &&str| match *node {
        "a" => vec!["b"],
        "b" => vec!["c"],
        "c" => vec!["d", "a"], // Creates cycle back to 'a'
        "d" => vec![],
        _ => vec![],
    };

    let mut graph = DirectedGraph::from(&nodes, get_outgoing);
    let found_cycles = graph.remove_cycles();

    assert!(!found_cycles.is_empty());

    // After removing cycles, there should be no path back to 'a' from 'c'
    let c_outgoing = graph.get_outgoing(&"c");
    assert!(!c_outgoing.contains(&"a"));
}

#[test]
fn remove_cycles_multiple_disconnected_cycles() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let nodes = ["a", "b", "c", "d"];
    let get_outgoing = |node: &&str| match *node {
        "a" => vec!["b"],
        "b" => vec!["a"], // Cycle 1: a <-> b
        "c" => vec!["d"],
        "d" => vec!["c"], // Cycle 2: c <-> d
        _ => vec![],
    };

    let mut graph = DirectedGraph::from(&nodes, get_outgoing);
    let found_cycles = graph.remove_cycles();

    assert!(found_cycles.len() >= 2);

    // After removing cycles, each pair should have only one direction
    let a_outgoing = graph.get_outgoing(&"a");
    let b_outgoing = graph.get_outgoing(&"b");
    let c_outgoing = graph.get_outgoing(&"c");
    let d_outgoing = graph.get_outgoing(&"d");

    assert!(
        (a_outgoing.is_empty() && b_outgoing.len() == 1) || (a_outgoing.len() == 1 && b_outgoing.is_empty())
    );
    assert!(
        (c_outgoing.is_empty() && d_outgoing.len() == 1) || (c_outgoing.len() == 1 && d_outgoing.is_empty())
    );
}

#[test]
fn get_outgoing_non_existent_node() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let graph = DirectedGraph::from(&["a"], |_| vec![]);
    assert_eq!(graph.get_outgoing(&"b"), Vec::<&str>::new());
}

#[test]
fn with_number_nodes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let nodes = [1, 2, 3];
    let get_outgoing = |node: &i32| match *node {
        1 => vec![2, 3],
        2 => vec![3],
        3 => vec![],
        _ => vec![],
    };

    let graph = DirectedGraph::from(&nodes, get_outgoing);

    let mut one = graph.get_outgoing(&1);
    one.sort();
    assert_eq!(one, vec![2, 3]);
    assert_eq!(graph.get_outgoing(&2), vec![3]);
    assert_eq!(graph.get_outgoing(&3), Vec::<i32>::new());
}
