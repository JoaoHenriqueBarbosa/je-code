//! Porte de `src/vs/editor/test/common/model/bracketPairColorizer/concat23Trees.test.ts` (suíte `Bracket Pair Colorizer - mergeItems`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::ast::{AstNodeKind, AstNodeRef, ListAstNode, TextAstNode};
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::concat23_trees::concat23_trees;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::length::to_length;

fn equals(node1: &AstNodeRef, node2: &AstNodeRef) -> bool {
    if node1.length() != node2.length() {
        return false;
    }

    let (children1, children2) = (node1.children(), node2.children());
    if children1.len() != children2.len() {
        return false;
    }

    for i in 0..children1.len() {
        if !equals(&children1[i], &children2[i]) {
            return false;
        }
    }

    if !node1.missing_opening_bracket_ids().equals(&node2.missing_opening_bracket_ids()) {
        return false;
    }

    if node1.kind() == AstNodeKind::Pair && node2.kind() == AstNodeKind::Pair {
        true
    } else {
        node1.kind() == node2.kind()
    }
}

fn test_merge(lists: Vec<AstNodeRef>) {
    let mut cloned: Vec<AstNodeRef> = lists.iter().map(|l| l.deep_clone()).collect();
    let node = concat23_trees(&mut cloned).unwrap_or_else(|| ListAstNode::create(vec![], false)).flatten_lists();
    // Esta fusão trivial não mantém a invariante da árvore (2,3).
    let reference_node = ListAstNode::create(lists, false).flatten_lists();

    assert!(equals(&node, &reference_node), "merge23Trees failed");
}

#[test]
fn clone() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let tree = ListAstNode::create(vec![TextAstNode::create(to_length(1, 1)), TextAstNode::create(to_length(1, 1))], false);

    assert!(equals(&tree, &tree.deep_clone()));
}

#[test]
fn empty_list() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    test_merge(vec![]);
}

#[test]
fn same_height_lists() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let text_node = TextAstNode::create(to_length(1, 1));
    let tree = ListAstNode::create(vec![text_node.deep_clone(), text_node.deep_clone()], false);
    test_merge(vec![tree.deep_clone(), tree.deep_clone(), tree.deep_clone(), tree.deep_clone(), tree.deep_clone()]);
}

#[test]
fn different_height_lists_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let text_node = TextAstNode::create(to_length(1, 1));
    let tree1 = ListAstNode::create(vec![text_node.deep_clone(), text_node.deep_clone()], false);
    let tree2 = ListAstNode::create(vec![tree1.deep_clone(), tree1.deep_clone()], false);

    test_merge(vec![tree1, tree2]);
}

#[test]
fn different_height_lists_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let text_node = TextAstNode::create(to_length(1, 1));
    let tree1 = ListAstNode::create(vec![text_node.deep_clone(), text_node.deep_clone()], false);
    let tree2 = ListAstNode::create(vec![tree1.deep_clone(), tree1.deep_clone()], false);

    test_merge(vec![tree2, tree1]);
}

#[test]
fn different_height_lists_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let text_node = TextAstNode::create(to_length(1, 1));
    let tree1 = ListAstNode::create(vec![text_node.deep_clone(), text_node.deep_clone()], false);
    let tree2 = ListAstNode::create(vec![tree1.deep_clone(), tree1.deep_clone()], false);

    test_merge(vec![tree2.clone(), tree1.clone(), tree1, tree2.clone(), tree2]);
}
