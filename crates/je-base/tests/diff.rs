//! Porte de `src/vs/base/test/common/diff/diff.test.ts` (suítes `Diff` e `Diff - Ported from VS`), caso a
//! caso e na mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque o `LcsDiff` não cria `IDisposable`. O
//! `this.timeout(10000)` do original não tem equivalente: o `cargo test` não impõe prazo ao teste.

use std::cell::Cell;
use std::rc::Rc;

use je_base::diff::{DiffChange, LcsDiff, StringDiffSequence};
use je_base::js_string::JsString;

fn sequence(text: &str) -> Rc<StringDiffSequence> {
    Rc::new(StringDiffSequence::new(JsString::from(text)))
}

fn mask_based_substring(str: &str, mask: &[bool]) -> String {
    str.chars().zip(mask).filter(|(_, keep)| **keep).map(|(c, _)| c).collect()
}

fn assert_answer(original_str: &str, modified_str: &str, changes: &[DiffChange], answer_str: &str, only_length: bool) {
    let mut original_mask = vec![true; original_str.len()];
    let mut modified_mask = vec![true; modified_str.len()];

    for change in changes {
        for j in 0..change.original_length {
            original_mask[(change.original_start + j) as usize] = false;
        }

        for j in 0..change.modified_length {
            modified_mask[(change.modified_start + j) as usize] = false;
        }
    }

    let original_answer = mask_based_substring(original_str, &original_mask);
    let modified_answer = mask_based_substring(modified_str, &modified_mask);

    if only_length {
        assert_eq!(original_answer.len(), answer_str.len());
        assert_eq!(modified_answer.len(), answer_str.len());
    } else {
        assert_eq!(original_answer, answer_str);
        assert_eq!(modified_answer, answer_str);
    }
}

fn lcs_inner_test(original_str: &str, modified_str: &str, answer_str: &str, only_length: bool) {
    let mut diff = LcsDiff::new(sequence(original_str), sequence(modified_str), None);
    let changes = diff.compute_diff(false).changes;
    assert_answer(original_str, modified_str, &changes, answer_str, only_length);
}

fn string_power(str: &str, power: u32) -> String {
    let mut r = str.to_string();
    for _ in 0..power {
        r = format!("{r}{r}");
    }
    r
}

fn lcs_test(original_str: &str, modified_str: &str, answer_str: &str) {
    lcs_inner_test(original_str, modified_str, answer_str, false);
    for i in 2..=5 {
        lcs_inner_test(&string_power(original_str, i), &string_power(modified_str, i), &string_power(answer_str, i), true);
    }
}

#[test]
fn lcs_diff_different_strings_tests() {
    lcs_test("heLLo world", "hello orlando", "heo orld");
    lcs_test("abcde", "acd", "acd"); // simple
    lcs_test("abcdbce", "bcede", "bcde"); // skip
    lcs_test("abcdefgabcdefg", "bcehafg", "bceafg"); // long
    lcs_test("abcde", "fgh", ""); // no match
    lcs_test("abcfabc", "fabc", "fabc");
    lcs_test("0azby0", "9axbzby9", "azby");
    lcs_test("0abc00000", "9a1b2c399999", "abc");

    lcs_test("fooBar", "myfooBar", "fooBar"); // all insertions
    lcs_test("fooBar", "fooMyBar", "fooBar"); // all insertions
    lcs_test("fooBar", "fooBar", "fooBar"); // identical sequences
}

#[test]
fn using_continue_processing_predicate_to_quit_early() {
    let left = "abcdef";
    let right = "abxxcyyydzzzzezzzzzzzzzzzzzzzzzzzzf";

    // We use a long non-matching portion at the end of the right-side string, so the backwards tracking logic
    // doesn't get there first.
    let predicate_call_count = Rc::new(Cell::new(0));

    let count = predicate_call_count.clone();
    let mut diff = LcsDiff::new(
        sequence(left),
        sequence(right),
        Some(Box::new(move |left_index, _longest_match_so_far| {
            assert_eq!(count.get(), 0);

            count.set(count.get() + 1);

            assert_eq!(left_index, 1);

            // cancel processing
            false
        })),
    );
    let mut changes = diff.compute_diff(true).changes;

    assert_eq!(predicate_call_count.get(), 1);

    // Doesn't include 'c', 'd', or 'e', since we quit on the first request
    assert_answer(left, right, &changes, "abf", false);

    // Cancel after the first match ('c')
    let mut diff = LcsDiff::new(
        sequence(left),
        sequence(right),
        Some(Box::new(|_left_index, longest_match_so_far| {
            assert!(longest_match_so_far <= 1.0); // We never see a match of length > 1

            // Continue processing as long as there hasn't been a match made.
            longest_match_so_far < 1.0
        })),
    );
    changes = diff.compute_diff(true).changes;

    assert_answer(left, right, &changes, "abcf", false);

    // Cancel after the second match ('d')
    let mut diff = LcsDiff::new(
        sequence(left),
        sequence(right),
        Some(Box::new(|_left_index, longest_match_so_far| {
            assert!(longest_match_so_far <= 2.0); // We never see a match of length > 2

            // Continue processing as long as there hasn't been a match made.
            longest_match_so_far < 2.0
        })),
    );
    changes = diff.compute_diff(true).changes;

    assert_answer(left, right, &changes, "abcdf", false);

    // Cancel *one iteration* after the second match ('d')
    let hit_second_match = Rc::new(Cell::new(false));
    let hit = hit_second_match.clone();
    let mut diff = LcsDiff::new(
        sequence(left),
        sequence(right),
        Some(Box::new(move |_left_index, longest_match_so_far| {
            assert!(longest_match_so_far <= 2.0); // We never see a match of length > 2

            let hit_yet = hit.get();
            hit.set(longest_match_so_far > 1.0);
            // Continue processing as long as there hasn't been a match made.
            !hit_yet
        })),
    );
    changes = diff.compute_diff(true).changes;

    assert_answer(left, right, &changes, "abcdf", false);

    // Cancel after the third and final match ('e')
    let mut diff = LcsDiff::new(
        sequence(left),
        sequence(right),
        Some(Box::new(|_left_index, longest_match_so_far| {
            assert!(longest_match_so_far <= 3.0); // We never see a match of length > 3

            // Continue processing as long as there hasn't been a match made.
            longest_match_so_far < 3.0
        })),
    );
    changes = diff.compute_diff(true).changes;

    assert_answer(left, right, &changes, "abcdef", false);
}
