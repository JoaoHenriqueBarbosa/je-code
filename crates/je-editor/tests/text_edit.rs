//! Porte de `src/vs/editor/test/common/core/textEdit.test.ts`, com o `Random` de `tests/support/random.rs`
//! (`random.ts`). Os `test.skip` do original são `#[ignore]`. Os casos `test N` e `case N` do original, um por
//! semente, rodam num laço por suíte, com a semente na mensagem de falha.

use je_base::JsString;
use je_editor::offset_range::OffsetRange;
use je_editor::text::abstract_text::StringText;

#[path = "support/random.rs"]
mod random;

use random::{BASIC_ALPHABET, Random};

mod inverse {
    use super::*;

    fn run_test(seed: u32) {
        let mut rand = Random::create(seed);
        let source = StringText::new(rand.next_multi_line_string(10, OffsetRange::new(0, 10), BASIC_ALPHABET));

        let count = rand.next_int_range(1, 5);
        let edit = rand.next_text_edit(&source, count);
        let inv_edit = edit.inverse(&source);

        let s1 = edit.apply(&source);
        let s2 = inv_edit.apply_to_string(s1);

        assert_eq!(s2, source.value, "seed {seed}");
    }

    #[test]
    #[ignore = "test.skip no original"]
    fn brute_force() {
        for i in 0..100_000 {
            run_test(i);
        }
    }

    #[test]
    fn test_seeds_0_to_19() {
        for seed in 0..20 {
            run_test(seed);
        }
    }
}

mod compose {
    use super::*;

    fn run_test(seed: u32) {
        let mut rand = Random::create(seed);

        let s0 = StringText::new(rand.next_multi_line_string(10, OffsetRange::new(0, 10), BASIC_ALPHABET));

        let count1 = rand.next_int_range(1, 4);
        let edits1 = rand.next_text_edit(&s0, count1);
        let s1: JsString = edits1.apply_to_string(s0.value.clone());

        let s1_text = StringText::new(s1.clone());
        let count2 = rand.next_int_range(1, 4);
        let edits2 = rand.next_text_edit(&s1_text, count2);
        let s2 = edits2.apply_to_string(s1);

        let combined_edits = edits1.compose(&edits2);
        let s2_c = combined_edits.apply_to_string(s0.value.clone());
        assert_eq!(s2_c, s2, "seed {seed}");
    }

    #[test]
    #[ignore = "test.skip no original"]
    fn fuzz() {
        for i in 0..1_000_000 {
            run_test(i);
        }
    }

    #[test]
    fn case_seeds_0_to_99() {
        for seed in 0..100 {
            run_test(seed);
        }
    }
}
