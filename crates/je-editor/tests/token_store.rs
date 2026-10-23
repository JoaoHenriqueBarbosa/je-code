//! Porte de `src/vs/editor/test/common/model/tokenStore.test.ts` (suíte `TokenStore`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - O `textModel` do `setup` é um objeto com só `getValueLength: () => 11`; aqui é um modelo real com
//!   o texto `hello world` (11 unidades), criado por `create_text_model`.
//! - `store.root as ListNode` e `as LeafNode` viram `Node::as_list` e `Node::as_leaf`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::text_model::TextModel;
use je_editor::token_store::{ListNode, Node, TokenQuality, TokenStore, TokenUpdate};
use support::test_text_model::create_text_model;

/// O `setup` + `new TokenStore(textModel)` de cada caso.
fn with_store(body: impl FnOnce(&Rc<TextModel>, &TokenStore)) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let store = TokenStore::new(model.clone());
    body(&model, &store);
    model.dispose();
}

/// `{ startOffsetInclusive, length, token }`.
fn t(start_offset_inclusive: usize, length: usize, token: u32) -> TokenUpdate {
    TokenUpdate { start_offset_inclusive, length, token }
}

fn root_list(store: &TokenStore) -> Rc<ListNode> {
    store.root().as_list().clone()
}

fn child(list: &ListNode, index: usize) -> Node {
    list.children()[index].clone()
}

fn leaf_token(node: &Node) -> u32 {
    node.as_leaf().token
}

/// Os três tokens `1, 2, 3` de comprimento 3 que vários casos usam.
fn three_tokens() -> Vec<TokenUpdate> {
    vec![t(0, 3, 1), t(3, 3, 2), t(6, 3, 3)]
}

/// Os oito tokens de comprimento 1 (`token` de 1 a 8).
fn eight_unit_tokens() -> Vec<TokenUpdate> {
    (0..8).map(|i| t(i, 1, i as u32 + 1)).collect()
}

#[test]
fn constructs_with_empty_model() {
    with_store(|model, store| {
        assert_eq!(store.root().length(), model.get_value_length() as usize);
    });
}

#[test]
fn builds_store_with_single_token() {
    with_store(|_, store| {
        store.build_store(&[t(0, 5, 1)], TokenQuality::Accurate);
        assert_eq!(store.root().length(), 5);
    });
}

#[test]
fn builds_store_with_multiple_tokens() {
    with_store(|_, store| {
        store.build_store(&[t(0, 3, 1), t(3, 3, 2), t(6, 4, 3)], TokenQuality::Accurate);
        assert_eq!(store.root().length(), 10);
    });
}

#[test]
fn creates_balanced_tree_structure() {
    with_store(|_, store| {
        store.build_store(&[t(0, 2, 1), t(2, 2, 2), t(4, 2, 3), t(6, 2, 4)], TokenQuality::Accurate);

        let root = root_list(store);
        assert_eq!(root.children_len(), 2);
        assert_eq!(child(&root, 0).length(), 4);
        assert_eq!(child(&root, 1).length(), 4);
    });
}

#[test]
fn creates_deep_tree_structure() {
    with_store(|_, store| {
        store.build_store(&eight_unit_tokens(), TokenQuality::Accurate);

        let root = root_list(store);
        assert_eq!(root.children_len(), 2);
        let first = child(&root, 0);
        assert_eq!(first.as_list().children_len(), 2);
        let first_first = child(first.as_list(), 0);
        assert_eq!(first_first.as_list().children_len(), 2);
    });
}

#[test]
fn updates_single_token_in_middle() {
    with_store(|_, store| {
        store.build_store(&three_tokens(), TokenQuality::Accurate);

        store.update(3, &[t(3, 3, 4)], TokenQuality::Accurate);

        let tokens = root_list(store);
        assert_eq!(leaf_token(&child(&tokens, 0)), 1);
        assert_eq!(leaf_token(&child(&tokens, 1)), 4);
        assert_eq!(leaf_token(&child(&tokens, 2)), 3);
    });
}

#[test]
fn updates_multiple_consecutive_tokens() {
    with_store(|_, store| {
        store.build_store(&three_tokens(), TokenQuality::Accurate);

        store.update(6, &[t(3, 3, 4), t(6, 3, 5)], TokenQuality::Accurate);

        let tokens = root_list(store);
        assert_eq!(leaf_token(&child(&tokens, 0)), 1);
        assert_eq!(leaf_token(&child(&tokens, 1)), 4);
        assert_eq!(leaf_token(&child(&tokens, 2)), 5);
    });
}

#[test]
fn updates_tokens_at_start_of_document() {
    with_store(|_, store| {
        store.build_store(&three_tokens(), TokenQuality::Accurate);

        store.update(3, &[t(0, 3, 4)], TokenQuality::Accurate);

        let tokens = root_list(store);
        assert_eq!(leaf_token(&child(&tokens, 0)), 4);
        assert_eq!(leaf_token(&child(&tokens, 1)), 2);
        assert_eq!(leaf_token(&child(&tokens, 2)), 3);
    });
}

#[test]
fn updates_tokens_at_end_of_document() {
    with_store(|_, store| {
        store.build_store(&three_tokens(), TokenQuality::Accurate);

        store.update(3, &[t(6, 3, 4)], TokenQuality::Accurate);

        let tokens = root_list(store);
        assert_eq!(leaf_token(&child(&tokens, 0)), 1);
        assert_eq!(leaf_token(&child(&tokens, 1)), 2);
        assert_eq!(leaf_token(&child(&tokens, 2)), 4);
    });
}

#[test]
fn updates_length_of_tokens() {
    with_store(|_, store| {
        store.build_store(&three_tokens(), TokenQuality::Accurate);

        store.update(6, &[t(3, 5, 4)], TokenQuality::Accurate);

        let tokens = root_list(store);
        assert_eq!(leaf_token(&child(&tokens, 0)), 1);
        assert_eq!(child(&tokens, 0).length(), 3);
        assert_eq!(leaf_token(&child(&tokens, 1)), 4);
        assert_eq!(child(&tokens, 1).length(), 5);
    });
}

#[test]
fn update_deeply_nested_tree_with_new_token_length_in_the_middle() {
    with_store(|_, store| {
        store.build_store(&eight_unit_tokens(), TokenQuality::Accurate);

        // Update token in the middle (position 3-4) to span 3-6
        store.update(3, &[t(3, 3, 9)], TokenQuality::Accurate);

        let root = root_list(store);
        // Verify the structure remains balanced
        assert_eq!(root.children_len(), 3);
        assert_eq!(child(&root, 0).as_list().children_len(), 2);

        // Verify the lengths are updated correctly
        assert_eq!(child(&root, 0).length(), 2); // First 2 tokens
        assert_eq!(child(&root, 1).length(), 4); // Token 3 + our new longer token
        assert_eq!(child(&root, 2).length(), 2); // Last 2 tokens
    });
}

#[test]
fn update_deeply_nested_tree_with_a_range_of_tokens_that_causes_tokens_to_split() {
    with_store(|_, store| {
        store.build_store(
            &[t(0, 3, 1), t(3, 3, 2), t(6, 4, 3), t(10, 5, 4), t(15, 4, 5), t(19, 3, 6), t(22, 5, 7), t(27, 3, 8)],
            TokenQuality::Accurate,
        );

        // Update token in the middle which causes tokens to split
        store.update(8, &[t(12, 4, 9), t(16, 4, 10)], TokenQuality::Accurate);

        let root = root_list(store);
        // Verify the structure remains balanced
        assert_eq!(root.children_len(), 2);
        assert_eq!(child(&root, 0).as_list().children_len(), 2);

        // Verify the lengths are updated correctly
        assert_eq!(child(&root, 0).length(), 12);
        assert_eq!(child(&root, 1).length(), 18);
    });
}

#[test]
fn get_tokens_in_range_returns_tokens_in_middle_of_document() {
    with_store(|_, store| {
        store.build_store(&three_tokens(), TokenQuality::Accurate);

        let tokens = store.get_tokens_in_range(3, 6);
        assert_eq!(tokens, vec![t(3, 3, 2)]);
    });
}

#[test]
fn get_tokens_in_range_returns_tokens_at_start_of_document() {
    with_store(|_, store| {
        store.build_store(&three_tokens(), TokenQuality::Accurate);

        let tokens = store.get_tokens_in_range(0, 3);
        assert_eq!(tokens, vec![t(0, 3, 1)]);
    });
}

#[test]
fn get_tokens_in_range_returns_tokens_at_end_of_document() {
    with_store(|_, store| {
        store.build_store(&three_tokens(), TokenQuality::Accurate);

        let tokens = store.get_tokens_in_range(6, 9);
        assert_eq!(tokens, vec![t(6, 3, 3)]);
    });
}

#[test]
fn get_tokens_in_range_returns_multiple_tokens_across_nodes() {
    with_store(|_, store| {
        store.build_store(&[t(0, 1, 1), t(1, 1, 2), t(2, 1, 3), t(3, 1, 4), t(4, 1, 5), t(5, 1, 6)], TokenQuality::Accurate);

        let tokens = store.get_tokens_in_range(2, 5);
        assert_eq!(tokens, vec![t(2, 1, 3), t(3, 1, 4), t(4, 1, 5)]);
    });
}

#[test]
fn realistic_scenario_one() {
    // inspired by this snippet, with the update adding a space in the constructor's curly braces:
    // /*
    // */
    // class XY {
    // 	constructor() {}
    // }
    with_store(|_, store| {
        store.build_store(
            &[
                t(0, 3, 164164),
                t(3, 1, 32836),
                t(4, 3, 164164),
                t(7, 2, 32836),
                t(9, 5, 196676),
                t(14, 1, 32836),
                t(15, 2, 557124),
                t(17, 4, 32836),
                t(21, 1, 32836),
                t(22, 11, 196676),
                t(33, 7, 32836),
                t(40, 3, 32836),
            ],
            TokenQuality::Accurate,
        );

        store.update(
            33,
            &[
                t(9, 5, 196676),
                t(14, 1, 32836),
                t(15, 2, 557124),
                t(17, 4, 32836),
                t(21, 1, 32836),
                t(22, 11, 196676),
                t(33, 8, 32836),
                t(41, 3, 32836),
            ],
            TokenQuality::Accurate,
        );
    });
}

#[test]
fn realistic_scenario_two() {
    // inspired by this snippet, with the update deleteing the space in the body of class x
    // class x {
    //
    // }
    // class y {

    // }
    with_store(|_, store| {
        store.build_store(
            &[
                t(0, 5, 196676),
                t(5, 1, 32836),
                t(6, 1, 557124),
                t(7, 4, 32836),
                t(11, 3, 32836),
                t(14, 3, 32836),
                t(17, 5, 196676),
                t(22, 1, 32836),
                t(23, 1, 557124),
                t(24, 4, 32836),
                t(28, 2, 32836),
                t(30, 1, 32836),
            ],
            TokenQuality::Accurate,
        );
        let tokens0 = store.get_tokens_in_range(0, 16);
        assert_eq!(
            tokens0,
            vec![t(0, 5, 196676), t(5, 1, 32836), t(6, 1, 557124), t(7, 4, 32836), t(11, 3, 32836), t(14, 2, 32836)]
        );

        store.update(
            14,
            &[t(0, 5, 196676), t(5, 1, 32836), t(6, 1, 557124), t(7, 4, 32836), t(11, 2, 32836), t(13, 3, 32836)],
            TokenQuality::Accurate,
        );

        let tokens = store.get_tokens_in_range(0, 16);
        assert_eq!(
            tokens,
            vec![t(0, 5, 196676), t(5, 1, 32836), t(6, 1, 557124), t(7, 4, 32836), t(11, 2, 32836), t(13, 3, 32836)]
        );
    });
}

#[test]
fn realistic_scenario_three() {
    // inspired by this snippet, with the update adding a space after the { in the constructor
    // /*--
    //  --*/
    //  class TreeViewPane {
    // 	constructor(
    // 		options: IViewletViewOptions,
    // 	) {
    // 	}
    // }
    with_store(|_, store| {
        store.build_store(
            &[
                t(0, 5, 164164),
                t(5, 1, 32836),
                t(6, 5, 164164),
                t(11, 2, 32836),
                t(13, 5, 196676),
                t(18, 1, 32836),
                t(19, 12, 557124),
                t(31, 4, 32836),
                t(35, 1, 32836),
                t(36, 11, 196676),
                t(47, 3, 32836),
                t(50, 2, 32836),
                t(52, 7, 327748),
                t(59, 1, 98372),
                t(60, 1, 32836),
                t(61, 19, 557124),
                t(80, 1, 32836),
                t(81, 2, 32836),
                t(83, 6, 32836),
                t(89, 4, 32836),
                t(93, 3, 32836),
            ],
            TokenQuality::Accurate,
        );
        let tokens0 = store.get_tokens_in_range(36, 59);
        assert_eq!(tokens0, vec![t(36, 11, 196676), t(47, 3, 32836), t(50, 2, 32836), t(52, 7, 327748)]);

        store.update(
            82,
            &[
                t(13, 5, 196676),
                t(18, 1, 32836),
                t(19, 12, 557124),
                t(31, 4, 32836),
                t(35, 1, 32836),
                t(36, 11, 196676),
                t(47, 3, 32836),
                t(50, 2, 32836),
                t(52, 7, 327748),
                t(59, 1, 98372),
                t(60, 1, 32836),
                t(61, 19, 557124),
                t(80, 1, 32836),
                t(81, 2, 32836),
                t(83, 7, 32836),
                t(90, 4, 32836),
                t(94, 3, 32836),
            ],
            TokenQuality::Accurate,
        );

        let tokens = store.get_tokens_in_range(36, 59);
        assert_eq!(tokens, vec![t(36, 11, 196676), t(47, 3, 32836), t(50, 2, 32836), t(52, 7, 327748)]);
    });
}

#[test]
fn realistic_scenario_four() {
    // inspired by this snippet, with the update adding a new line after the return true;
    // function x() {
    // 	return true;
    // }

    // class Y {
    // 	private z = false;
    // }
    with_store(|_, store| {
        store.build_store(
            &[
                t(0, 8, 196676),
                t(8, 1, 32836),
                t(9, 1, 524356),
                t(10, 6, 32836),
                t(16, 1, 32836),
                t(17, 6, 589892),
                t(23, 1, 32836),
                t(24, 4, 196676),
                t(28, 1, 32836),
                t(29, 2, 32836),
                t(31, 3, 32836), // This is the closing curly brace + newline chars
                t(34, 2, 32836),
                t(36, 5, 196676),
                t(41, 1, 32836),
                t(42, 1, 557124),
                t(43, 4, 32836),
                t(47, 1, 32836),
                t(48, 7, 196676),
                t(55, 1, 32836),
                t(56, 1, 327748),
                t(57, 1, 32836),
                t(58, 1, 98372),
                t(59, 1, 32836),
                t(60, 5, 196676),
                t(65, 1, 32836),
                t(66, 2, 32836),
                t(68, 1, 32836),
            ],
            TokenQuality::Accurate,
        );
        let tokens0 = store.get_tokens_in_range(36, 59);
        assert_eq!(
            tokens0,
            vec![
                t(36, 5, 196676),
                t(41, 1, 32836),
                t(42, 1, 557124),
                t(43, 4, 32836),
                t(47, 1, 32836),
                t(48, 7, 196676),
                t(55, 1, 32836),
                t(56, 1, 327748),
                t(57, 1, 32836),
                t(58, 1, 98372),
            ]
        );

        // insert a tab + new line after `return true;` (like hitting enter after the ;)
        store.update(
            32,
            &[
                t(0, 8, 196676),
                t(8, 1, 32836),
                t(9, 1, 524356),
                t(10, 6, 32836),
                t(16, 1, 32836),
                t(17, 6, 589892),
                t(23, 1, 32836),
                t(24, 4, 196676),
                t(28, 1, 32836),
                t(29, 2, 32836),
                t(31, 3, 32836), // This is the new line, which consists of 3 characters: \t\r\n
                t(34, 2, 32836),
            ],
            TokenQuality::Accurate,
        );

        let tokens1 = store.get_tokens_in_range(36, 59);
        assert_eq!(
            tokens1,
            vec![
                t(36, 2, 32836),
                t(38, 2, 32836),
                t(40, 5, 196676),
                t(45, 1, 32836),
                t(46, 1, 557124),
                t(47, 4, 32836),
                t(51, 1, 32836),
                t(52, 7, 196676),
            ]
        );

        // Delete the tab character
        store.update(
            37,
            &[
                t(0, 8, 196676),
                t(8, 1, 32836),
                t(9, 1, 524356),
                t(10, 6, 32836),
                t(16, 1, 32836),
                t(17, 6, 589892),
                t(23, 1, 32836),
                t(24, 4, 196676),
                t(28, 1, 32836),
                t(29, 2, 32836),
                t(31, 2, 32836), // This is the changed line: \t\r\n to \r\n
                t(33, 3, 32836),
            ],
            TokenQuality::Accurate,
        );

        let tokens2 = store.get_tokens_in_range(36, 59);
        assert_eq!(
            tokens2,
            vec![
                t(36, 1, 32836),
                t(37, 2, 32836),
                t(39, 5, 196676),
                t(44, 1, 32836),
                t(45, 1, 557124),
                t(46, 4, 32836),
                t(50, 1, 32836),
                t(51, 7, 196676),
                t(58, 1, 32836),
            ]
        );
    });
}

#[test]
fn insert_new_line_and_remove_tabs_split_tokens() {
    // class A {
    // 	a() {
    // 	}
    // }
    //
    // interface I {
    //
    // }
    with_store(|_, store| {
        store.build_store(
            &[
                t(0, 5, 196676),
                t(5, 1, 32836),
                t(6, 1, 557124),
                t(7, 3, 32836),
                t(10, 1, 32836),
                t(11, 1, 524356),
                t(12, 5, 32836),
                t(17, 3, 32836), // This is the closing curly brace line of a()
                t(20, 2, 32836),
                t(22, 1, 32836),
                t(23, 9, 196676),
                t(32, 1, 32836),
                t(33, 1, 557124),
                t(34, 3, 32836),
                t(37, 1, 32836),
                t(38, 1, 32836),
            ],
            TokenQuality::Accurate,
        );

        let tokens0 = store.get_tokens_in_range(23, 39);
        assert_eq!(
            tokens0,
            vec![t(23, 9, 196676), t(32, 1, 32836), t(33, 1, 557124), t(34, 3, 32836), t(37, 1, 32836), t(38, 1, 32836)]
        );

        // Insert a new line after a() { }, which will add 2 tabs
        store.update(
            21,
            &[
                t(0, 5, 196676),
                t(5, 1, 32836),
                t(6, 1, 557124),
                t(7, 3, 32836),
                t(10, 1, 32836),
                t(11, 1, 524356),
                t(12, 5, 32836),
                t(17, 3, 32836),
                t(20, 3, 32836),
                t(23, 1, 32836),
            ],
            TokenQuality::Accurate,
        );

        let tokens1 = store.get_tokens_in_range(26, 42);
        assert_eq!(
            tokens1,
            vec![t(26, 9, 196676), t(35, 1, 32836), t(36, 1, 557124), t(37, 3, 32836), t(40, 1, 32836), t(41, 1, 32836)]
        );

        // Insert another new line at the cursor, which will also cause the 2 tabs to be deleted
        store.update(
            24,
            &[
                t(0, 5, 196676),
                t(5, 1, 32836),
                t(6, 1, 557124),
                t(7, 3, 32836),
                t(10, 1, 32836),
                t(11, 1, 524356),
                t(12, 5, 32836),
                t(17, 3, 32836),
                t(20, 1, 32836),
                t(21, 2, 32836),
                t(23, 1, 32836),
            ],
            TokenQuality::Accurate,
        );

        let tokens2 = store.get_tokens_in_range(26, 42);
        assert_eq!(
            tokens2,
            vec![t(26, 9, 196676), t(35, 1, 32836), t(36, 1, 557124), t(37, 3, 32836), t(40, 1, 32836), t(41, 1, 32836)]
        );
    });
}

#[test]
fn delete_removes_tokens_in_the_middle() {
    with_store(|_, store| {
        store.build_store(&three_tokens(), TokenQuality::Accurate);
        store.delete(3, 3); // delete 3 chars starting at offset 3
        let tokens = store.get_tokens_in_range(0, 9);
        assert_eq!(tokens, vec![t(0, 3, 1), t(3, 3, 3)]);
    });
}

#[test]
fn delete_merges_partially_affected_token() {
    with_store(|_, store| {
        store.build_store(&[t(0, 5, 1), t(5, 5, 2)], TokenQuality::Accurate);
        store.delete(3, 4); // removes 4 chars within token 1 and partially token 2
        let tokens = store.get_tokens_in_range(0, 10);
        assert_eq!(
            tokens,
            vec![
                t(0, 4, 1),
                // token 2 is now shifted left by 4
                t(4, 3, 2),
            ]
        );
    });
}

#[test]
fn replace_a_token_with_a_slightly_larger_token() {
    with_store(|_, store| {
        store.build_store(
            &[t(0, 5, 1), t(5, 1, 2), t(6, 1, 2), t(7, 17, 2), t(24, 1, 2), t(25, 5, 2), t(30, 1, 2), t(31, 1, 2), t(32, 5, 2)],
            TokenQuality::Accurate,
        );
        store.update(17, &[t(7, 19, 0)], TokenQuality::Accurate); // removes 4 chars within token 1 and partially token 2
        let tokens = store.get_tokens_in_range(0, 39);
        assert_eq!(
            tokens,
            vec![
                t(0, 5, 1),
                t(5, 1, 2),
                t(6, 1, 2),
                t(7, 19, 0),
                t(26, 1, 2),
                t(27, 5, 2),
                t(32, 1, 2),
                t(33, 1, 2),
                t(34, 5, 2),
            ]
        );
    });
}

#[test]
fn replace_a_character_from_a_large_token() {
    with_store(|_, store| {
        store.build_store(&[t(0, 2, 1), t(2, 5, 2), t(7, 1, 3)], TokenQuality::Accurate);
        store.delete(1, 3);
        let tokens = store.get_tokens_in_range(0, 7);
        assert_eq!(tokens, vec![t(0, 2, 1), t(2, 1, 2), t(3, 3, 2), t(6, 1, 3)]);
    });
}
