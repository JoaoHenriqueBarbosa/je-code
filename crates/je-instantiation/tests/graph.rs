//! Porte de `src/vs/platform/instantiation/test/common/graph.test.ts` (suíte `Graph`), caso a caso e na mesma ordem.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_instantiation::Graph;

fn graph() -> Graph<String> {
    Graph::new(|s: &String| s.clone())
}

fn s(v: &str) -> String {
    v.to_string()
}

mod graph_suite {
    use super::*;

    #[test]
    fn is_possible_to_lookup_nodes_that_dont_exist() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let graph = graph();
        assert!(graph.lookup(&s("ddd")).is_none());
    }

    #[test]
    fn inserts_nodes_when_not_there_yet() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut graph = graph();
        assert!(graph.lookup(&s("ddd")).is_none());
        assert_eq!(graph.lookup_or_insert_node(s("ddd")).data, "ddd");
        assert_eq!(graph.lookup(&s("ddd")).unwrap().data, "ddd");
    }

    #[test]
    fn can_remove_nodes_and_get_length() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut graph = graph();
        assert!(graph.is_empty());
        assert!(graph.lookup(&s("ddd")).is_none());
        assert_eq!(graph.lookup_or_insert_node(s("ddd")).data, "ddd");
        assert!(!graph.is_empty());
        graph.remove_node(&s("ddd"));
        assert!(graph.lookup(&s("ddd")).is_none());
        assert!(graph.is_empty());
    }

    #[test]
    fn root() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut graph = graph();
        graph.insert_edge(s("1"), s("2"));
        let roots = graph.roots();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].data, "2");

        graph.insert_edge(s("2"), s("1"));
        let roots = graph.roots();
        assert_eq!(roots.len(), 0);
    }

    #[test]
    fn root_complex() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut graph = graph();
        graph.insert_edge(s("1"), s("2"));
        graph.insert_edge(s("1"), s("3"));
        graph.insert_edge(s("3"), s("4"));

        let roots = graph.roots();
        assert_eq!(roots.len(), 2);
        assert!(
            ["2", "4"]
                .iter()
                .all(|n| roots.iter().any(|node| node.data == *n))
        );
    }
}
