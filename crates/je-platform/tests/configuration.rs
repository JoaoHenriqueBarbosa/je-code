//! Porte de `src/vs/platform/configuration/test/common/configuration.test.ts` (suíte `Configuration`), caso a caso e na mesma ordem.
//!
//! O arquivo original tem duas suítes: `Configuration` (a árvore de valores) e `Configuration Changes: Merge`
//! (`mergeChanges`), aqui os módulos `configuration` e `configuration_changes_merge`.

mod common;

use je_base::dynamic::Dynamic;
use je_platform::configuration::models::merge_changes;
use je_platform::configuration::{merge, remove_from_value_tree};

use common::*;

mod configuration {
    use super::*;

    #[test]
    fn simple_merge() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let base = j("{ 'a': 1, 'b': 2 }");
        merge(&base, &j("{ 'a': 3, 'c': 4 }"), true);
        assert_deep_strict_equal(&base, &j("{ 'a': 3, 'b': 2, 'c': 4 }"));
        let base = j("{ 'a': 1, 'b': 2 }");
        merge(&base, &j("{ 'a': 3, 'c': 4 }"), false);
        assert_deep_strict_equal(&base, &j("{ 'a': 1, 'b': 2, 'c': 4 }"));
    }

    #[test]
    fn object_merge() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let base = j("{ 'a': { 'b': 1, 'c': true, 'd': 2 } }");
        let add = obj(vec![("a", obj(vec![("b", Dynamic::Undefined), ("c", Dynamic::Bool(false)), ("e", s("a"))]))]);
        merge(&base, &add, true);
        let expected = obj(vec![("a", obj(vec![("b", Dynamic::Undefined), ("c", Dynamic::Bool(false)), ("d", n(2.0)), ("e", s("a"))]))]);
        assert_deep_strict_equal(&base, &expected);
    }

    #[test]
    fn array_merge() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let base = j("{ 'a': ['b', 'c'] }");
        merge(&base, &j("{ 'a': ['b', 'd'] }"), true);
        assert_deep_strict_equal(&base, &j("{ 'a': ['b', 'd'] }"));
    }

    #[test]
    fn remove_from_value_tree_remove_a_non_existing_key() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = j("{ 'a': { 'b': 2 } }");

        remove_from_value_tree(&target, "c");

        assert_deep_strict_equal(&target, &j("{ 'a': { 'b': 2 } }"));
    }

    #[test]
    fn remove_from_value_tree_remove_a_multi_segmented_key_from_an_object_that_has_only_sub_sections_of_the_key() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = j("{ 'a': { 'b': 2 } }");

        remove_from_value_tree(&target, "a.b.c");

        assert_deep_strict_equal(&target, &j("{ 'a': { 'b': 2 } }"));
    }

    #[test]
    fn remove_from_value_tree_remove_a_single_segmented_key() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = j("{ 'a': 1 }");

        remove_from_value_tree(&target, "a");

        assert_deep_strict_equal(&target, &j("{}"));
    }

    #[test]
    fn remove_from_value_tree_remove_a_single_segmented_key_when_its_value_is_undefined() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = obj(vec![("a", Dynamic::Undefined)]);

        remove_from_value_tree(&target, "a");

        assert_deep_strict_equal(&target, &j("{}"));
    }

    #[test]
    fn remove_from_value_tree_remove_a_multi_segmented_key_when_its_value_is_undefined() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = j("{ 'a': { 'b': 1 } }");

        remove_from_value_tree(&target, "a.b");

        assert_deep_strict_equal(&target, &j("{}"));
    }

    #[test]
    fn remove_from_value_tree_remove_a_multi_segmented_key_when_its_value_is_array() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = j("{ 'a': { 'b': [1] } }");

        remove_from_value_tree(&target, "a.b");

        assert_deep_strict_equal(&target, &j("{}"));
    }

    #[test]
    fn remove_from_value_tree_remove_a_multi_segmented_key_first_segment_value_is_array() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = j("{ 'a': [1] }");

        remove_from_value_tree(&target, "a.0");

        assert_deep_strict_equal(&target, &j("{ 'a': [1] }"));
    }

    #[test]
    fn remove_from_value_tree_remove_when_key_is_the_first_segmenet() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = j("{ 'a': { 'b': 1 } }");

        remove_from_value_tree(&target, "a");

        assert_deep_strict_equal(&target, &j("{}"));
    }

    #[test]
    fn remove_from_value_tree_remove_a_multi_segmented_key_when_the_first_node_has_more_values() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = j("{ 'a': { 'b': { 'c': 1 }, 'd': 1 } }");

        remove_from_value_tree(&target, "a.b.c");

        assert_deep_strict_equal(&target, &j("{ 'a': { 'd': 1 } }"));
    }

    #[test]
    fn remove_from_value_tree_remove_a_multi_segmented_key_when_in_between_node_has_more_values() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = j("{ 'a': { 'b': { 'c': { 'd': 1 }, 'd': 1 } } }");

        remove_from_value_tree(&target, "a.b.c.d");

        assert_deep_strict_equal(&target, &j("{ 'a': { 'b': { 'd': 1 } } }"));
    }

    #[test]
    fn remove_from_value_tree_remove_a_multi_segmented_key_when_the_last_but_one_node_has_more_values() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let target = j("{ 'a': { 'b': { 'c': 1, 'd': 1 } } }");

        remove_from_value_tree(&target, "a.b.c");

        assert_deep_strict_equal(&target, &j("{ 'a': { 'b': { 'd': 1 } } }"));
    }
}

mod configuration_changes_merge {
    use super::*;

    #[test]
    fn merge_only_keys() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = merge_changes(&[change(&["a", "b"], &[]), change(&["c", "d"], &[])]);
        assert_eq!(actual, change(&["a", "b", "c", "d"], &[]));
    }

    #[test]
    fn merge_only_keys_with_duplicates() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = merge_changes(&[change(&["a", "b"], &[]), change(&["c", "d"], &[]), change(&["a", "d", "e"], &[])]);
        assert_eq!(actual, change(&["a", "b", "c", "d", "e"], &[]));
    }

    #[test]
    fn merge_only_overrides() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = merge_changes(&[change(&[], &[("a", &["1", "2"])]), change(&[], &[("b", &["3", "4"])])]);
        assert_eq!(actual, change(&[], &[("a", &["1", "2"]), ("b", &["3", "4"])]));
    }

    #[test]
    fn merge_only_overrides_with_duplicates() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = merge_changes(&[
            change(&[], &[("a", &["1", "2"]), ("b", &["5", "4"])]),
            change(&[], &[("b", &["3", "4"])]),
            change(&[], &[("c", &["1", "4"]), ("a", &["2", "3"])]),
        ]);
        assert_eq!(actual, change(&[], &[("a", &["1", "2", "3"]), ("b", &["5", "4", "3"]), ("c", &["1", "4"])]));
    }

    #[test]
    fn merge() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = merge_changes(&[
            change(&["b", "b"], &[("a", &["1", "2"]), ("b", &["5", "4"])]),
            change(&["b"], &[("b", &["3", "4"])]),
            change(&["c", "a"], &[("c", &["1", "4"]), ("a", &["2", "3"])]),
        ]);
        assert_eq!(actual, change(&["b", "c", "a"], &[("a", &["1", "2", "3"]), ("b", &["5", "4", "3"]), ("c", &["1", "4"])]));
    }

    #[test]
    fn merge_single_change() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = merge_changes(&[change(&["b", "b"], &[("a", &["1", "2"]), ("b", &["5", "4"])])]);
        assert_eq!(actual, change(&["b", "b"], &[("a", &["1", "2"]), ("b", &["5", "4"])]));
    }

    #[test]
    fn merge_no_changes() {
        let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let actual = merge_changes(&[]);
        assert_eq!(actual, change(&[], &[]));
    }
}
