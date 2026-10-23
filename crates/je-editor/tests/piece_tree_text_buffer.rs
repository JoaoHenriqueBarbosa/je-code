//! Porte de `src/vs/editor/test/common/model/pieceTreeTextBuffer/pieceTreeTextBuffer.test.ts` (suíte `PieceTreeTextBuffer`), caso a caso e na mesma ordem.
//!
//! Cada `suite` do original vira um `mod` (os nomes `random_bug_1` e afins se repetem entre as suítes).
//!
//! Notas de tradução:
//! - Os testes aleatórios do original usam `Math.random`; aqui usam um gerador com semente fixa
//!   (SplitMix64), o que torna a execução reprodutível. `crates/je-editor/tests/piece_tree_stress.rs`
//!   (fora do placar) roda o mesmo tipo de operação contra um modelo simples, com muitas sementes.
//! - Os casos da suíte `snapshot` usam `createTextModel` e `model.applyEdits`. O `TextModel` ainda não foi
//!   portado, então eles chamam `PieceTreeTextBuffer::apply_edits` direto com os mesmos intervalos (o
//!   `TextModel` só valida o intervalo e repassa); o que o caso exercita, a imutabilidade do snapshot,
//!   vive todo no buffer.
//! - `ensureNoDisposablesAreLeakedInTestSuite` vem do `common` do `je-base`, que este arquivo inclui por
//!   `#[path]` para não duplicar o auxiliar.
//! - `getLineCharCode` devolve `Option<u16>` (`None` é o `NaN` do JavaScript).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_base::strings;
use je_editor::position::Position;
use je_editor::range::Range;
use support::*;

mod inserts_and_deletes {
    use super::*;

    #[test]
    fn basic_insert_delete() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["This is a document with some text."], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(34, &u("This is some more text to insert at offset 34."), false);
        assert_text(
            &pt.get_lines_raw_content(),
            &u("This is a document with some text.This is some more text to insert at offset 34."),
        );
        pt.delete(42, 5);
        assert_text(
            &pt.get_lines_raw_content(),
            &u("This is a document with some text.This is more text to insert at offset 34."),
        );
        assert_tree_invariants(pt);
    }

    #[test]
    fn more_inserts() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("AAA"), false);
        assert_text(&pt.get_lines_raw_content(), &u("AAA"));
        pt.insert(0, &u("BBB"), false);
        assert_text(&pt.get_lines_raw_content(), &u("BBBAAA"));
        pt.insert(6, &u("CCC"), false);
        assert_text(&pt.get_lines_raw_content(), &u("BBBAAACCC"));
        pt.insert(5, &u("DDD"), false);
        assert_text(&pt.get_lines_raw_content(), &u("BBBAADDDACCC"));
        assert_tree_invariants(pt);
    }

    #[test]
    fn more_deletes() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["012345678"], true);
        let pt = buf.get_piece_tree_mut();
        pt.delete(8, 1);
        assert_text(&pt.get_lines_raw_content(), &u("01234567"));
        pt.delete(0, 1);
        assert_text(&pt.get_lines_raw_content(), &u("1234567"));
        pt.delete(5, 1);
        assert_text(&pt.get_lines_raw_content(), &u("123457"));
        pt.delete(5, 1);
        assert_text(&pt.get_lines_raw_content(), &u("12345"));
        pt.delete(0, 5);
        assert_text(&pt.get_lines_raw_content(), &u(""));
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_test_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("ceLPHmFzvCtFeHkCBej "), false);
        ins(&mut s, 0, "ceLPHmFzvCtFeHkCBej ");
        assert_text(&pt.get_lines_raw_content(), &s);
        pt.insert(8, &u("gDCEfNYiBUNkSwtvB K "), false);
        ins(&mut s, 8, "gDCEfNYiBUNkSwtvB K ");
        assert_text(&pt.get_lines_raw_content(), &s);
        pt.insert(38, &u("cyNcHxjNPPoehBJldLS "), false);
        ins(&mut s, 38, "cyNcHxjNPPoehBJldLS ");
        assert_text(&pt.get_lines_raw_content(), &s);
        pt.insert(59, &u("ejMx\nOTgWlbpeDExjOk "), false);
        ins(&mut s, 59, "ejMx\nOTgWlbpeDExjOk ");
        assert_text(&pt.get_lines_raw_content(), &s);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_test_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("VgPG "), false);
        ins(&mut s, 0, "VgPG ");
        pt.insert(2, &u("DdWF "), false);
        ins(&mut s, 2, "DdWF ");
        pt.insert(0, &u("hUJc "), false);
        ins(&mut s, 0, "hUJc ");
        pt.insert(8, &u("lQEq "), false);
        ins(&mut s, 8, "lQEq ");
        pt.insert(10, &u("Gbtp "), false);
        ins(&mut s, 10, "Gbtp ");
        assert_text(&pt.get_lines_raw_content(), &s);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_test_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("gYSz"), false);
        ins(&mut s, 0, "gYSz");
        pt.insert(1, &u("mDQe"), false);
        ins(&mut s, 1, "mDQe");
        pt.insert(1, &u("DTMQ"), false);
        ins(&mut s, 1, "DTMQ");
        pt.insert(2, &u("GGZB"), false);
        ins(&mut s, 2, "GGZB");
        pt.insert(12, &u("wXpq"), false);
        ins(&mut s, 12, "wXpq");
        assert_text(&pt.get_lines_raw_content(), &s);
    }

    #[test]
    fn random_delete_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("vfb"), false);
        ins(&mut s, 0, "vfb");
        assert_text(&pt.get_lines_raw_content(), &s);
        pt.insert(0, &u("zRq"), false);
        ins(&mut s, 0, "zRq");
        assert_text(&pt.get_lines_raw_content(), &s);
        pt.delete(5, 1);
        del(&mut s, 5, 1);
        assert_text(&pt.get_lines_raw_content(), &s);
        pt.insert(1, &u("UNw"), false);
        ins(&mut s, 1, "UNw");
        assert_text(&pt.get_lines_raw_content(), &s);
        pt.delete(4, 3);
        del(&mut s, 4, 3);
        assert_text(&pt.get_lines_raw_content(), &s);
        pt.delete(1, 4);
        del(&mut s, 1, 4);
        assert_text(&pt.get_lines_raw_content(), &s);
        pt.delete(0, 1);
        del(&mut s, 0, 1);
        assert_text(&pt.get_lines_raw_content(), &s);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_delete_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("IDT"), false);
        ins(&mut s, 0, "IDT");
        pt.insert(3, &u("wwA"), false);
        ins(&mut s, 3, "wwA");
        pt.insert(3, &u("Gnr"), false);
        ins(&mut s, 3, "Gnr");
        pt.delete(6, 3);
        del(&mut s, 6, 3);
        pt.insert(4, &u("eHp"), false);
        ins(&mut s, 4, "eHp");
        pt.insert(1, &u("UAi"), false);
        ins(&mut s, 1, "UAi");
        pt.insert(2, &u("FrR"), false);
        ins(&mut s, 2, "FrR");
        pt.delete(6, 7);
        del(&mut s, 6, 7);
        pt.delete(3, 5);
        del(&mut s, 3, 5);
        assert_text(&pt.get_lines_raw_content(), &s);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_delete_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("PqM"), false);
        ins(&mut s, 0, "PqM");
        pt.delete(1, 2);
        del(&mut s, 1, 2);
        pt.insert(1, &u("zLc"), false);
        ins(&mut s, 1, "zLc");
        pt.insert(0, &u("MEX"), false);
        ins(&mut s, 0, "MEX");
        pt.insert(0, &u("jZh"), false);
        ins(&mut s, 0, "jZh");
        pt.insert(8, &u("GwQ"), false);
        ins(&mut s, 8, "GwQ");
        pt.delete(5, 6);
        del(&mut s, 5, 6);
        pt.insert(4, &u("ktw"), false);
        ins(&mut s, 4, "ktw");
        pt.insert(5, &u("GVu"), false);
        ins(&mut s, 5, "GVu");
        pt.insert(9, &u("jdm"), false);
        ins(&mut s, 9, "jdm");
        pt.insert(15, &u("na\n"), false);
        ins(&mut s, 15, "na\n");
        pt.delete(5, 8);
        del(&mut s, 5, 8);
        pt.delete(3, 4);
        del(&mut s, 3, 4);
        assert_text(&pt.get_lines_raw_content(), &s);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_insert_delete_r_bug_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("a");
        let mut buf = create_text_buffer(&ds, &["a"], true);
        let pt = buf.get_piece_tree_mut();
        pt.delete(0, 1);
        del(&mut s, 0, 1);
        pt.insert(0, &u("\r\r\n\n"), false);
        ins(&mut s, 0, "\r\r\n\n");
        pt.delete(3, 1);
        del(&mut s, 3, 1);
        pt.insert(2, &u("\n\n\ra"), false);
        ins(&mut s, 2, "\n\n\ra");
        pt.delete(4, 3);
        del(&mut s, 4, 3);
        pt.insert(2, &u("\na\r\r"), false);
        ins(&mut s, 2, "\na\r\r");
        pt.insert(6, &u("\ra\n\n"), false);
        ins(&mut s, 6, "\ra\n\n");
        pt.insert(0, &u("aa\n\n"), false);
        ins(&mut s, 0, "aa\n\n");
        pt.insert(5, &u("\n\na\r"), false);
        ins(&mut s, 5, "\n\na\r");
        assert_text(&pt.get_lines_raw_content(), &s);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_insert_delete_r_bug_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("a");
        let mut buf = create_text_buffer(&ds, &["a"], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(1, &u("\naa\r"), false);
        ins(&mut s, 1, "\naa\r");
        pt.delete(0, 4);
        del(&mut s, 0, 4);
        pt.insert(1, &u("\r\r\na"), false);
        ins(&mut s, 1, "\r\r\na");
        pt.insert(2, &u("\n\r\ra"), false);
        ins(&mut s, 2, "\n\r\ra");
        pt.delete(4, 1);
        del(&mut s, 4, 1);
        pt.insert(8, &u("\r\n\r\r"), false);
        ins(&mut s, 8, "\r\n\r\r");
        pt.insert(7, &u("\n\n\na"), false);
        ins(&mut s, 7, "\n\n\na");
        pt.insert(13, &u("a\n\na"), false);
        ins(&mut s, 13, "a\n\na");
        pt.delete(17, 3);
        del(&mut s, 17, 3);
        pt.insert(2, &u("a\ra\n"), false);
        ins(&mut s, 2, "a\ra\n");
        assert_text(&pt.get_lines_raw_content(), &s);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_insert_delete_r_bug_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("a");
        let mut buf = create_text_buffer(&ds, &["a"], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\r\na\r"), false);
        ins(&mut s, 0, "\r\na\r");
        pt.delete(2, 3);
        del(&mut s, 2, 3);
        pt.insert(2, &u("a\r\n\r"), false);
        ins(&mut s, 2, "a\r\n\r");
        pt.delete(4, 2);
        del(&mut s, 4, 2);
        pt.insert(4, &u("a\n\r\n"), false);
        ins(&mut s, 4, "a\n\r\n");
        pt.insert(1, &u("aa\n\r"), false);
        ins(&mut s, 1, "aa\n\r");
        pt.insert(7, &u("\na\r\n"), false);
        ins(&mut s, 7, "\na\r\n");
        pt.insert(5, &u("\n\na\r"), false);
        ins(&mut s, 5, "\n\na\r");
        pt.insert(10, &u("\r\r\n\r"), false);
        ins(&mut s, 10, "\r\r\n\r");
        assert_text(&pt.get_lines_raw_content(), &s);
        pt.delete(21, 3);
        del(&mut s, 21, 3);
        assert_text(&pt.get_lines_raw_content(), &s);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_insert_delete_r_bug_4s() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("a");
        let mut buf = create_text_buffer(&ds, &["a"], true);
        let pt = buf.get_piece_tree_mut();
        pt.delete(0, 1);
        del(&mut s, 0, 1);
        pt.insert(0, &u("\naaa"), false);
        ins(&mut s, 0, "\naaa");
        pt.insert(2, &u("\n\naa"), false);
        ins(&mut s, 2, "\n\naa");
        pt.delete(1, 4);
        del(&mut s, 1, 4);
        pt.delete(3, 1);
        del(&mut s, 3, 1);
        pt.delete(1, 2);
        del(&mut s, 1, 2);
        pt.delete(0, 1);
        del(&mut s, 0, 1);
        pt.insert(0, &u("a\n\n\r"), false);
        ins(&mut s, 0, "a\n\n\r");
        pt.insert(2, &u("aa\r\n"), false);
        ins(&mut s, 2, "aa\r\n");
        pt.insert(3, &u("a\naa"), false);
        ins(&mut s, 3, "a\naa");
        assert_text(&pt.get_lines_raw_content(), &s);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_insert_delete_r_bug_5() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\n\n\n\r"), false);
        ins(&mut s, 0, "\n\n\n\r");
        pt.insert(1, &u("\n\n\n\r"), false);
        ins(&mut s, 1, "\n\n\n\r");
        pt.insert(2, &u("\n\r\r\r"), false);
        ins(&mut s, 2, "\n\r\r\r");
        pt.insert(8, &u("\n\r\n\r"), false);
        ins(&mut s, 8, "\n\r\n\r");
        pt.delete(5, 2);
        del(&mut s, 5, 2);
        pt.insert(4, &u("\n\r\r\r"), false);
        ins(&mut s, 4, "\n\r\r\r");
        pt.insert(8, &u("\n\n\n\r"), false);
        ins(&mut s, 8, "\n\n\n\r");
        pt.delete(0, 7);
        del(&mut s, 0, 7);
        pt.insert(1, &u("\r\n\r\r"), false);
        ins(&mut s, 1, "\r\n\r\r");
        pt.insert(15, &u("\n\r\r\r"), false);
        ins(&mut s, 15, "\n\r\r\r");
        assert_text(&pt.get_lines_raw_content(), &s);
        assert_tree_invariants(pt);
    }
}

mod prefix_sum_for_line_feed {
    use super::*;

    #[test]
    fn basic() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["1\n2\n3\n4"], true);
        let pt = buf.get_piece_tree_mut();
        assert_eq!(pt.get_line_count(), 4);
        assert_eq!(pt.get_position_at(0), Position::new(1, 1));
        assert_eq!(pt.get_position_at(1), Position::new(1, 2));
        assert_eq!(pt.get_position_at(2), Position::new(2, 1));
        assert_eq!(pt.get_position_at(3), Position::new(2, 2));
        assert_eq!(pt.get_position_at(4), Position::new(3, 1));
        assert_eq!(pt.get_position_at(5), Position::new(3, 2));
        assert_eq!(pt.get_position_at(6), Position::new(4, 1));
        assert_eq!(pt.get_offset_at(1, 1), 0);
        assert_eq!(pt.get_offset_at(1, 2), 1);
        assert_eq!(pt.get_offset_at(2, 1), 2);
        assert_eq!(pt.get_offset_at(2, 2), 3);
        assert_eq!(pt.get_offset_at(3, 1), 4);
        assert_eq!(pt.get_offset_at(3, 2), 5);
        assert_eq!(pt.get_offset_at(4, 1), 6);
        assert_tree_invariants(pt);
    }

    #[test]
    fn append() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["a\nb\nc\nde"], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(8, &u("fh\ni\njk"), false);
        assert_eq!(pt.get_line_count(), 6);
        assert_eq!(pt.get_position_at(9), Position::new(4, 4));
        assert_eq!(pt.get_offset_at(1, 1), 0);
        assert_tree_invariants(pt);
    }

    #[test]
    fn insert() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["a\nb\nc\nde"], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(7, &u("fh\ni\njk"), false);
        assert_eq!(pt.get_line_count(), 6);
        assert_eq!(pt.get_position_at(6), Position::new(4, 1));
        assert_eq!(pt.get_position_at(7), Position::new(4, 2));
        assert_eq!(pt.get_position_at(8), Position::new(4, 3));
        assert_eq!(pt.get_position_at(9), Position::new(4, 4));
        assert_eq!(pt.get_position_at(12), Position::new(6, 1));
        assert_eq!(pt.get_position_at(13), Position::new(6, 2));
        assert_eq!(pt.get_position_at(14), Position::new(6, 3));
        assert_eq!(pt.get_offset_at(4, 1), 6);
        assert_eq!(pt.get_offset_at(4, 2), 7);
        assert_eq!(pt.get_offset_at(4, 3), 8);
        assert_eq!(pt.get_offset_at(4, 4), 9);
        assert_eq!(pt.get_offset_at(6, 1), 12);
        assert_eq!(pt.get_offset_at(6, 2), 13);
        assert_eq!(pt.get_offset_at(6, 3), 14);
        assert_tree_invariants(pt);
    }

    #[test]
    fn delete() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["a\nb\nc\ndefh\ni\njk"], true);
        let pt = buf.get_piece_tree_mut();
        pt.delete(7, 2);
        assert_text(&pt.get_lines_raw_content(), &u("a\nb\nc\ndh\ni\njk"));
        assert_eq!(pt.get_line_count(), 6);
        assert_eq!(pt.get_position_at(6), Position::new(4, 1));
        assert_eq!(pt.get_position_at(7), Position::new(4, 2));
        assert_eq!(pt.get_position_at(8), Position::new(4, 3));
        assert_eq!(pt.get_position_at(9), Position::new(5, 1));
        assert_eq!(pt.get_position_at(11), Position::new(6, 1));
        assert_eq!(pt.get_position_at(12), Position::new(6, 2));
        assert_eq!(pt.get_position_at(13), Position::new(6, 3));
        assert_eq!(pt.get_offset_at(4, 1), 6);
        assert_eq!(pt.get_offset_at(4, 2), 7);
        assert_eq!(pt.get_offset_at(4, 3), 8);
        assert_eq!(pt.get_offset_at(5, 1), 9);
        assert_eq!(pt.get_offset_at(6, 1), 11);
        assert_eq!(pt.get_offset_at(6, 2), 12);
        assert_eq!(pt.get_offset_at(6, 3), 13);
        assert_tree_invariants(pt);
    }

    #[test]
    fn add_delete_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["a\nb\nc\nde"], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(8, &u("fh\ni\njk"), false);
        pt.delete(7, 2);
        assert_text(&pt.get_lines_raw_content(), &u("a\nb\nc\ndh\ni\njk"));
        assert_eq!(pt.get_line_count(), 6);
        assert_eq!(pt.get_position_at(6), Position::new(4, 1));
        assert_eq!(pt.get_position_at(7), Position::new(4, 2));
        assert_eq!(pt.get_position_at(8), Position::new(4, 3));
        assert_eq!(pt.get_position_at(9), Position::new(5, 1));
        assert_eq!(pt.get_position_at(11), Position::new(6, 1));
        assert_eq!(pt.get_position_at(12), Position::new(6, 2));
        assert_eq!(pt.get_position_at(13), Position::new(6, 3));
        assert_eq!(pt.get_offset_at(4, 1), 6);
        assert_eq!(pt.get_offset_at(4, 2), 7);
        assert_eq!(pt.get_offset_at(4, 3), 8);
        assert_eq!(pt.get_offset_at(5, 1), 9);
        assert_eq!(pt.get_offset_at(6, 1), 11);
        assert_eq!(pt.get_offset_at(6, 2), 12);
        assert_eq!(pt.get_offset_at(6, 3), 13);
        assert_tree_invariants(pt);
    }

    #[test]
    fn insert_random_bug_1_prefix_sum_computer_remove_values_start_cnt_cnt_is_1_based() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u(" ZX \n Z\nZ\n YZ\nY\nZXX "), false);
        ins(&mut s, 0, " ZX \n Z\nZ\n YZ\nY\nZXX ");
        pt.insert(14, &u("X ZZ\nYZZYZXXY Y XY\n "), false);
        ins(&mut s, 14, "X ZZ\nYZZYZXXY Y XY\n ");
        assert_text(&pt.get_lines_raw_content(), &s);
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn insert_random_bug_2_prefix_sum_computer_initialize_does_not_do_deep_copy_of_uint32_array() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("ZYZ\nYY XY\nX \nZ Y \nZ "), false);
        ins(&mut s, 0, "ZYZ\nYY XY\nX \nZ Y \nZ ");
        pt.insert(3, &u("XXY \n\nY Y YYY  ZYXY "), false);
        ins(&mut s, 3, "XXY \n\nY Y YYY  ZYXY ");
        assert_text(&pt.get_lines_raw_content(), &s);
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn delete_random_bug_1_i_forgot_to_update_the_line_feed_cnt_when_deletion_is_on_one_single_piece() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("ba\na\nca\nba\ncbab\ncaa "), false);
        pt.insert(13, &u("cca\naabb\ncac\nccc\nab "), false);
        pt.delete(5, 8);
        pt.delete(30, 2);
        pt.insert(24, &u("cbbacccbac\nbaaab\n\nc "), false);
        pt.delete(29, 3);
        pt.delete(23, 9);
        pt.delete(21, 5);
        pt.delete(30, 3);
        pt.insert(3, &u("cb\nac\nc\n\nacc\nbb\nb\nc "), false);
        pt.delete(19, 5);
        pt.insert(18, &u("\nbb\n\nacbc\ncbb\nc\nbb\n "), false);
        pt.insert(65, &u("cbccbac\nbc\n\nccabba\n "), false);
        pt.insert(77, &u("a\ncacb\n\nac\n\n\n\n\nabab "), false);
        pt.delete(30, 9);
        pt.insert(45, &u("b\n\nc\nba\n\nbbbba\n\naa\n "), false);
        pt.insert(82, &u("ab\nbb\ncabacab\ncbc\na "), false);
        pt.delete(123, 9);
        pt.delete(71, 2);
        pt.insert(33, &u("acaa\nacb\n\naa\n\nc\n\n\n\n "), false);
        let s = pt.get_lines_raw_content().units().to_vec();
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn delete_random_bug_rb_tree_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("YXXZ\n\nYY\n"), false);
        ins(&mut s, 0, "YXXZ\n\nYY\n");
        pt.delete(0, 5);
        del(&mut s, 0, 5);
        pt.insert(0, &u("ZXYY\nX\nZ\n"), false);
        ins(&mut s, 0, "ZXYY\nX\nZ\n");
        pt.insert(10, &u("\nXY\nYXYXY"), false);
        ins(&mut s, 10, "\nXY\nYXYXY");
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn delete_random_bug_rb_tree_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("YXXZ\n\nYY\n"), false);
        ins(&mut s, 0, "YXXZ\n\nYY\n");
        pt.insert(0, &u("ZXYY\nX\nZ\n"), false);
        ins(&mut s, 0, "ZXYY\nX\nZ\n");
        pt.insert(10, &u("\nXY\nYXYXY"), false);
        ins(&mut s, 10, "\nXY\nYXYXY");
        pt.insert(8, &u("YZXY\nZ\nYX"), false);
        ins(&mut s, 8, "YZXY\nZ\nYX");
        pt.insert(12, &u("XX\nXXYXYZ"), false);
        ins(&mut s, 12, "XX\nXXYXYZ");
        pt.delete(0, 4);
        del(&mut s, 0, 4);
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn delete_random_bug_rb_tree_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("YXXZ\n\nYY\n"), false);
        ins(&mut s, 0, "YXXZ\n\nYY\n");
        pt.delete(7, 2);
        del(&mut s, 7, 2);
        pt.delete(6, 1);
        del(&mut s, 6, 1);
        pt.delete(0, 5);
        del(&mut s, 0, 5);
        pt.insert(0, &u("ZXYY\nX\nZ\n"), false);
        ins(&mut s, 0, "ZXYY\nX\nZ\n");
        pt.insert(10, &u("\nXY\nYXYXY"), false);
        ins(&mut s, 10, "\nXY\nYXYXY");
        pt.insert(8, &u("YZXY\nZ\nYX"), false);
        ins(&mut s, 8, "YZXY\nZ\nYX");
        pt.insert(12, &u("XX\nXXYXYZ"), false);
        ins(&mut s, 12, "XX\nXXYXYZ");
        pt.delete(0, 4);
        del(&mut s, 0, 4);
        pt.delete(30, 3);
        del(&mut s, 30, 3);
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }
}

mod offset_2_position {
    use super::*;

    #[test]
    fn random_tests_bug_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("huuyYzUfKOENwGgZLqn "), false);
        ins(&mut s, 0, "huuyYzUfKOENwGgZLqn ");
        pt.delete(18, 2);
        del(&mut s, 18, 2);
        pt.delete(3, 1);
        del(&mut s, 3, 1);
        pt.delete(12, 4);
        del(&mut s, 12, 4);
        pt.insert(3, &u("hMbnVEdTSdhLlPevXKF "), false);
        ins(&mut s, 3, "hMbnVEdTSdhLlPevXKF ");
        pt.delete(22, 8);
        del(&mut s, 22, 8);
        pt.insert(4, &u("S umSnYrqOmOAV\nEbZJ "), false);
        ins(&mut s, 4, "S umSnYrqOmOAV\nEbZJ ");
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }
}

mod get_text_in_range {
    use super::*;

    #[test]
    fn get_content_in_range() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["a\nb\nc\nde"], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(8, &u("fh\ni\njk"), false);
        pt.delete(7, 2);
        assert_text(&pt.get_value_in_range(&Range::new(2, 1, 2, 3), None), &u("b\n"));
        assert_text(&pt.get_value_in_range(&Range::new(3, 1, 3, 3), None), &u("c\n"));
        assert_text(&pt.get_value_in_range(&Range::new(4, 1, 4, 4), None), &u("dh\n"));
        assert_text(&pt.get_value_in_range(&Range::new(5, 1, 5, 3), None), &u("i\n"));
        assert_text(&pt.get_value_in_range(&Range::new(6, 1, 6, 3), None), &u("jk"));
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_test_value_in_range() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("ZXXY"), false);
        ins(&mut s, 0, "ZXXY");
        pt.insert(1, &u("XZZY"), false);
        ins(&mut s, 1, "XZZY");
        pt.insert(5, &u("\nX\n\n"), false);
        ins(&mut s, 5, "\nX\n\n");
        pt.insert(3, &u("\nXX\n"), false);
        ins(&mut s, 3, "\nXX\n");
        pt.insert(12, &u("YYYX"), false);
        ins(&mut s, 12, "YYYX");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_test_value_in_range_exception() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("XZ\nZ"), false);
        ins(&mut s, 0, "XZ\nZ");
        pt.delete(0, 3);
        del(&mut s, 0, 3);
        pt.delete(0, 1);
        del(&mut s, 0, 1);
        pt.insert(0, &u("ZYX\n"), false);
        ins(&mut s, 0, "ZYX\n");
        pt.delete(0, 4);
        del(&mut s, 0, 4);
        pt.get_value_in_range(&Range::new(1, 1, 1, 1), None);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_tests_bug_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("huuyYzUfKOENwGgZLqn "), false);
        ins(&mut s, 0, "huuyYzUfKOENwGgZLqn ");
        pt.delete(18, 2);
        del(&mut s, 18, 2);
        pt.delete(3, 1);
        del(&mut s, 3, 1);
        pt.delete(12, 4);
        del(&mut s, 12, 4);
        pt.insert(3, &u("hMbnVEdTSdhLlPevXKF "), false);
        ins(&mut s, 3, "hMbnVEdTSdhLlPevXKF ");
        pt.delete(22, 8);
        del(&mut s, 22, 8);
        pt.insert(4, &u("S umSnYrqOmOAV\nEbZJ "), false);
        ins(&mut s, 4, "S umSnYrqOmOAV\nEbZJ ");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_tests_bug_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("xfouRDZwdAHjVXJAMV\n "), false);
        ins(&mut s, 0, "xfouRDZwdAHjVXJAMV\n ");
        pt.insert(16, &u("dBGndxpFZBEAIKykYYx "), false);
        ins(&mut s, 16, "dBGndxpFZBEAIKykYYx ");
        pt.delete(7, 6);
        del(&mut s, 7, 6);
        pt.delete(9, 7);
        del(&mut s, 9, 7);
        pt.delete(17, 6);
        del(&mut s, 17, 6);
        pt.delete(0, 4);
        del(&mut s, 0, 4);
        pt.insert(9, &u("qvEFXCNvVkWgvykahYt "), false);
        ins(&mut s, 9, "qvEFXCNvVkWgvykahYt ");
        pt.delete(4, 6);
        del(&mut s, 4, 6);
        pt.insert(11, &u("OcSChUYT\nzPEBOpsGmR "), false);
        ins(&mut s, 11, "OcSChUYT\nzPEBOpsGmR ");
        pt.insert(15, &u("KJCozaXTvkE\nxnqAeTz "), false);
        ins(&mut s, 15, "KJCozaXTvkE\nxnqAeTz ");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn get_line_content() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["1"], true);
        let pt = buf.get_piece_tree_mut();
        assert_text(&pt.get_line_raw_content(1, 0), &u("1"));
        pt.insert(1, &u("2"), false);
        assert_text(&pt.get_line_raw_content(1, 0), &u("12"));
        assert_tree_invariants(pt);
    }

    #[test]
    fn get_line_content_basic() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["1\n2\n3\n4"], true);
        let pt = buf.get_piece_tree_mut();
        assert_text(&pt.get_line_raw_content(1, 0), &u("1\n"));
        assert_text(&pt.get_line_raw_content(2, 0), &u("2\n"));
        assert_text(&pt.get_line_raw_content(3, 0), &u("3\n"));
        assert_text(&pt.get_line_raw_content(4, 0), &u("4"));
        assert_tree_invariants(pt);
    }

    #[test]
    fn get_line_content_after_inserts_deletes() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["a\nb\nc\nde"], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(8, &u("fh\ni\njk"), false);
        pt.delete(7, 2);
        assert_text(&pt.get_line_raw_content(2, 0), &u("b\n"));
        assert_text(&pt.get_line_raw_content(3, 0), &u("c\n"));
        assert_text(&pt.get_line_raw_content(4, 0), &u("dh\n"));
        assert_text(&pt.get_line_raw_content(5, 0), &u("i\n"));
        assert_text(&pt.get_line_raw_content(6, 0), &u("jk"));
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("J eNnDzQpnlWyjmUu\ny "), false);
        ins(&mut s, 0, "J eNnDzQpnlWyjmUu\ny ");
        pt.insert(0, &u("QPEeRAQmRwlJqtZSWhQ "), false);
        ins(&mut s, 0, "QPEeRAQmRwlJqtZSWhQ ");
        pt.delete(5, 1);
        del(&mut s, 5, 1);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("DZoQ tglPCRHMltejRI "), false);
        ins(&mut s, 0, "DZoQ tglPCRHMltejRI ");
        pt.insert(10, &u("JRXiyYqJ qqdcmbfkKX "), false);
        ins(&mut s, 10, "JRXiyYqJ qqdcmbfkKX ");
        pt.delete(16, 3);
        del(&mut s, 16, 3);
        pt.delete(25, 1);
        del(&mut s, 25, 1);
        pt.insert(18, &u("vH\nNlvfqQJPm\nSFkhMc "), false);
        ins(&mut s, 18, "vH\nNlvfqQJPm\nSFkhMc ");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }
}

mod crlf {
    use super::*;

    #[test]
    fn delete_cr_in_crlf_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("a\r\nb"), false);
        pt.delete(0, 2);
        assert_eq!(pt.get_line_count(), 2);
        assert_tree_invariants(pt);
    }

    #[test]
    fn delete_cr_in_crlf_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("a\r\nb"), false);
        pt.delete(2, 2);
        assert_eq!(pt.get_line_count(), 2);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\n\n\r\r"), false);
        ins(&mut s, 0, "\n\n\r\r");
        pt.insert(1, &u("\r\n\r\n"), false);
        ins(&mut s, 1, "\r\n\r\n");
        pt.delete(5, 3);
        del(&mut s, 5, 3);
        pt.delete(2, 3);
        del(&mut s, 2, 3);
        let lines = strings::split_lines(&s);
        assert_eq!(pt.get_line_count(), lines.len());
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\n\r\n\r"), false);
        ins(&mut s, 0, "\n\r\n\r");
        pt.insert(2, &u("\n\r\r\r"), false);
        ins(&mut s, 2, "\n\r\r\r");
        pt.delete(4, 1);
        del(&mut s, 4, 1);
        let lines = strings::split_lines(&s);
        assert_eq!(pt.get_line_count(), lines.len());
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\n\n\n\r"), false);
        ins(&mut s, 0, "\n\n\n\r");
        pt.delete(2, 2);
        del(&mut s, 2, 2);
        pt.delete(0, 2);
        del(&mut s, 0, 2);
        pt.insert(0, &u("\r\r\r\r"), false);
        ins(&mut s, 0, "\r\r\r\r");
        pt.insert(2, &u("\r\n\r\r"), false);
        ins(&mut s, 2, "\r\n\r\r");
        pt.insert(3, &u("\r\r\r\n"), false);
        ins(&mut s, 3, "\r\r\r\n");
        let lines = strings::split_lines(&s);
        assert_eq!(pt.get_line_count(), lines.len());
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_4() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\n\n\n\n"), false);
        ins(&mut s, 0, "\n\n\n\n");
        pt.delete(3, 1);
        del(&mut s, 3, 1);
        pt.insert(1, &u("\r\r\r\r"), false);
        ins(&mut s, 1, "\r\r\r\r");
        pt.insert(6, &u("\r\n\n\r"), false);
        ins(&mut s, 6, "\r\n\n\r");
        pt.delete(5, 3);
        del(&mut s, 5, 3);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_5() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\n\n\n\n"), false);
        ins(&mut s, 0, "\n\n\n\n");
        pt.delete(3, 1);
        del(&mut s, 3, 1);
        pt.insert(0, &u("\n\r\r\n"), false);
        ins(&mut s, 0, "\n\r\r\n");
        pt.insert(4, &u("\n\r\r\n"), false);
        ins(&mut s, 4, "\n\r\r\n");
        pt.delete(4, 3);
        del(&mut s, 4, 3);
        pt.insert(5, &u("\r\r\n\r"), false);
        ins(&mut s, 5, "\r\r\n\r");
        pt.insert(12, &u("\n\n\n\r"), false);
        ins(&mut s, 12, "\n\n\n\r");
        pt.insert(5, &u("\r\r\r\n"), false);
        ins(&mut s, 5, "\r\r\r\n");
        pt.insert(20, &u("\n\n\r\n"), false);
        ins(&mut s, 20, "\n\n\r\n");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_6() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\n\r\r\n"), false);
        ins(&mut s, 0, "\n\r\r\n");
        pt.insert(4, &u("\r\n\n\r"), false);
        ins(&mut s, 4, "\r\n\n\r");
        pt.insert(3, &u("\r\n\n\n"), false);
        ins(&mut s, 3, "\r\n\n\n");
        pt.delete(4, 8);
        del(&mut s, 4, 8);
        pt.insert(4, &u("\r\n\n\r"), false);
        ins(&mut s, 4, "\r\n\n\r");
        pt.insert(0, &u("\r\n\n\r"), false);
        ins(&mut s, 0, "\r\n\n\r");
        pt.delete(4, 0);
        del(&mut s, 4, 0);
        pt.delete(8, 4);
        del(&mut s, 8, 4);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_8() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\r\n\n\r"), false);
        ins(&mut s, 0, "\r\n\n\r");
        pt.delete(1, 0);
        del(&mut s, 1, 0);
        pt.insert(3, &u("\n\n\n\r"), false);
        ins(&mut s, 3, "\n\n\n\r");
        pt.insert(7, &u("\n\n\r\n"), false);
        ins(&mut s, 7, "\n\n\r\n");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_7() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\r\r\n\n"), false);
        ins(&mut s, 0, "\r\r\n\n");
        pt.insert(4, &u("\r\n\n\r"), false);
        ins(&mut s, 4, "\r\n\n\r");
        pt.insert(7, &u("\n\r\r\r"), false);
        ins(&mut s, 7, "\n\r\r\r");
        pt.insert(11, &u("\n\n\r\n"), false);
        ins(&mut s, 11, "\n\n\r\n");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_10() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("qneW"), false);
        ins(&mut s, 0, "qneW");
        pt.insert(0, &u("YhIl"), false);
        ins(&mut s, 0, "YhIl");
        pt.insert(0, &u("qdsm"), false);
        ins(&mut s, 0, "qdsm");
        pt.delete(7, 0);
        del(&mut s, 7, 0);
        pt.insert(12, &u("iiPv"), false);
        ins(&mut s, 12, "iiPv");
        pt.insert(9, &u("V\rSA"), false);
        ins(&mut s, 9, "V\rSA");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_9() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("\n\n\n\n"), false);
        ins(&mut s, 0, "\n\n\n\n");
        pt.insert(3, &u("\n\r\n\r"), false);
        ins(&mut s, 3, "\n\r\n\r");
        pt.insert(2, &u("\n\r\n\n"), false);
        ins(&mut s, 2, "\n\r\n\n");
        pt.insert(0, &u("\n\n\r\r"), false);
        ins(&mut s, 0, "\n\n\r\r");
        pt.insert(3, &u("\r\r\r\r"), false);
        ins(&mut s, 3, "\r\r\r\r");
        pt.insert(3, &u("\n\n\r\r"), false);
        ins(&mut s, 3, "\n\n\r\r");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }
}

mod centralized_line_starts_with_crlf {
    use super::*;

    #[test]
    fn delete_cr_in_crlf_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["a\r\nb"], false);
        let pt = buf.get_piece_tree_mut();
        pt.delete(2, 2);
        assert_eq!(pt.get_line_count(), 2);
        assert_tree_invariants(pt);
    }

    #[test]
    fn delete_cr_in_crlf_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["a\r\nb"], true);
        let pt = buf.get_piece_tree_mut();
        pt.delete(0, 2);
        assert_eq!(pt.get_line_count(), 2);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("\n\n\r\r");
        let mut buf = create_text_buffer(&ds, &["\n\n\r\r"], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(1, &u("\r\n\r\n"), false);
        ins(&mut s, 1, "\r\n\r\n");
        pt.delete(5, 3);
        del(&mut s, 5, 3);
        pt.delete(2, 3);
        del(&mut s, 2, 3);
        let lines = strings::split_lines(&s);
        assert_eq!(pt.get_line_count(), lines.len());
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("\n\r\n\r");
        let mut buf = create_text_buffer(&ds, &["\n\r\n\r"], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(2, &u("\n\r\r\r"), false);
        ins(&mut s, 2, "\n\r\r\r");
        pt.delete(4, 1);
        del(&mut s, 4, 1);
        let lines = strings::split_lines(&s);
        assert_eq!(pt.get_line_count(), lines.len());
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("\n\n\n\r");
        let mut buf = create_text_buffer(&ds, &["\n\n\n\r"], false);
        let pt = buf.get_piece_tree_mut();
        pt.delete(2, 2);
        del(&mut s, 2, 2);
        pt.delete(0, 2);
        del(&mut s, 0, 2);
        pt.insert(0, &u("\r\r\r\r"), false);
        ins(&mut s, 0, "\r\r\r\r");
        pt.insert(2, &u("\r\n\r\r"), false);
        ins(&mut s, 2, "\r\n\r\r");
        pt.insert(3, &u("\r\r\r\n"), false);
        ins(&mut s, 3, "\r\r\r\n");
        let lines = strings::split_lines(&s);
        assert_eq!(pt.get_line_count(), lines.len());
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_4() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("\n\n\n\n");
        let mut buf = create_text_buffer(&ds, &["\n\n\n\n"], false);
        let pt = buf.get_piece_tree_mut();
        pt.delete(3, 1);
        del(&mut s, 3, 1);
        pt.insert(1, &u("\r\r\r\r"), false);
        ins(&mut s, 1, "\r\r\r\r");
        pt.insert(6, &u("\r\n\n\r"), false);
        ins(&mut s, 6, "\r\n\n\r");
        pt.delete(5, 3);
        del(&mut s, 5, 3);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_5() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("\n\n\n\n");
        let mut buf = create_text_buffer(&ds, &["\n\n\n\n"], false);
        let pt = buf.get_piece_tree_mut();
        pt.delete(3, 1);
        del(&mut s, 3, 1);
        pt.insert(0, &u("\n\r\r\n"), false);
        ins(&mut s, 0, "\n\r\r\n");
        pt.insert(4, &u("\n\r\r\n"), false);
        ins(&mut s, 4, "\n\r\r\n");
        pt.delete(4, 3);
        del(&mut s, 4, 3);
        pt.insert(5, &u("\r\r\n\r"), false);
        ins(&mut s, 5, "\r\r\n\r");
        pt.insert(12, &u("\n\n\n\r"), false);
        ins(&mut s, 12, "\n\n\n\r");
        pt.insert(5, &u("\r\r\r\n"), false);
        ins(&mut s, 5, "\r\r\r\n");
        pt.insert(20, &u("\n\n\r\n"), false);
        ins(&mut s, 20, "\n\n\r\n");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_6() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("\n\r\r\n");
        let mut buf = create_text_buffer(&ds, &["\n\r\r\n"], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(4, &u("\r\n\n\r"), false);
        ins(&mut s, 4, "\r\n\n\r");
        pt.insert(3, &u("\r\n\n\n"), false);
        ins(&mut s, 3, "\r\n\n\n");
        pt.delete(4, 8);
        del(&mut s, 4, 8);
        pt.insert(4, &u("\r\n\n\r"), false);
        ins(&mut s, 4, "\r\n\n\r");
        pt.insert(0, &u("\r\n\n\r"), false);
        ins(&mut s, 0, "\r\n\n\r");
        pt.delete(4, 0);
        del(&mut s, 4, 0);
        pt.delete(8, 4);
        del(&mut s, 8, 4);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_7() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("\r\n\n\r");
        let mut buf = create_text_buffer(&ds, &["\r\n\n\r"], false);
        let pt = buf.get_piece_tree_mut();
        pt.delete(1, 0);
        del(&mut s, 1, 0);
        pt.insert(3, &u("\n\n\n\r"), false);
        ins(&mut s, 3, "\n\n\n\r");
        pt.insert(7, &u("\n\n\r\n"), false);
        ins(&mut s, 7, "\n\n\r\n");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_8() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("\r\r\n\n");
        let mut buf = create_text_buffer(&ds, &["\r\r\n\n"], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(4, &u("\r\n\n\r"), false);
        ins(&mut s, 4, "\r\n\n\r");
        pt.insert(7, &u("\n\r\r\r"), false);
        ins(&mut s, 7, "\n\r\r\r");
        pt.insert(11, &u("\n\n\r\n"), false);
        ins(&mut s, 11, "\n\n\r\n");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_9() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("qneW");
        let mut buf = create_text_buffer(&ds, &["qneW"], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(0, &u("YhIl"), false);
        ins(&mut s, 0, "YhIl");
        pt.insert(0, &u("qdsm"), false);
        ins(&mut s, 0, "qdsm");
        pt.delete(7, 0);
        del(&mut s, 7, 0);
        pt.insert(12, &u("iiPv"), false);
        ins(&mut s, 12, "iiPv");
        pt.insert(9, &u("V\rSA"), false);
        ins(&mut s, 9, "V\rSA");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_bug_10() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut s = u("\n\n\n\n");
        let mut buf = create_text_buffer(&ds, &["\n\n\n\n"], false);
        let pt = buf.get_piece_tree_mut();
        pt.insert(3, &u("\n\r\n\r"), false);
        ins(&mut s, 3, "\n\r\n\r");
        pt.insert(2, &u("\n\r\n\n"), false);
        ins(&mut s, 2, "\n\r\n\n");
        pt.insert(0, &u("\n\n\r\r"), false);
        ins(&mut s, 0, "\n\n\r\r");
        pt.insert(3, &u("\r\r\r\r"), false);
        ins(&mut s, 3, "\r\r\r\r");
        pt.insert(3, &u("\n\n\r\r"), false);
        ins(&mut s, 3, "\n\n\r\r");
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_chunk_bug_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["\n\r\r\n\n\n\r\n\r"], false);
        let pt = buf.get_piece_tree_mut();
        let mut s = u("\n\r\r\n\n\n\r\n\r");
        pt.delete(0, 2);
        del(&mut s, 0, 2);
        pt.insert(1, &u("\r\r\n\n"), false);
        ins(&mut s, 1, "\r\r\n\n");
        pt.insert(7, &u("\r\r\r\r"), false);
        ins(&mut s, 7, "\r\r\r\r");
        assert_text(&pt.get_lines_raw_content(), &s);
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_chunk_bug_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["\n\r\n\n\n\r\n\r\n\r\r\n\n\n\r\r\n\r\n"], false);
        let pt = buf.get_piece_tree_mut();
        let mut s = u("\n\r\n\n\n\r\n\r\n\r\r\n\n\n\r\r\n\r\n");
        pt.insert(16, &u("\r\n\r\r"), false);
        ins(&mut s, 16, "\r\n\r\r");
        pt.insert(13, &u("\n\n\r\r"), false);
        ins(&mut s, 13, "\n\n\r\r");
        pt.insert(19, &u("\n\n\r\n"), false);
        ins(&mut s, 19, "\n\n\r\n");
        pt.delete(5, 0);
        del(&mut s, 5, 0);
        pt.delete(11, 2);
        del(&mut s, 11, 2);
        assert_text(&pt.get_lines_raw_content(), &s);
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_chunk_bug_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["\r\n\n\n\n\n\n\r\n"], false);
        let pt = buf.get_piece_tree_mut();
        let mut s = u("\r\n\n\n\n\n\n\r\n");
        pt.insert(4, &u("\n\n\r\n\r\r\n\n\r"), false);
        ins(&mut s, 4, "\n\n\r\n\r\r\n\n\r");
        pt.delete(4, 4);
        del(&mut s, 4, 4);
        pt.insert(11, &u("\r\n\r\n\n\r\r\n\n"), false);
        ins(&mut s, 11, "\r\n\r\n\n\r\r\n\n");
        pt.delete(1, 2);
        del(&mut s, 1, 2);
        assert_text(&pt.get_lines_raw_content(), &s);
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_chunk_bug_4() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["\n\r\n\r"], false);
        let pt = buf.get_piece_tree_mut();
        let mut s = u("\n\r\n\r");
        pt.insert(4, &u("\n\n\r\n"), false);
        ins(&mut s, 4, "\n\n\r\n");
        pt.insert(3, &u("\r\n\n\n"), false);
        ins(&mut s, 3, "\r\n\n\n");
        assert_text(&pt.get_lines_raw_content(), &s);
        test_line_starts(&s, pt);
        assert_tree_invariants(pt);
    }
}

mod random_is_unsupervised {
    use super::*;

    #[test]
    fn splitting_large_change_buffer() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        let mut s = u("");
        pt.insert(0, &u("WUZ\nXVZY\n"), false);
        ins(&mut s, 0, "WUZ\nXVZY\n");
        pt.insert(8, &u("\r\r\nZXUWVW"), false);
        ins(&mut s, 8, "\r\r\nZXUWVW");
        pt.delete(10, 7);
        del(&mut s, 10, 7);
        pt.delete(10, 1);
        del(&mut s, 10, 1);
        pt.insert(4, &u("VX\r\r\nWZVZ"), false);
        ins(&mut s, 4, "VX\r\r\nWZVZ");
        pt.delete(11, 3);
        del(&mut s, 11, 3);
        pt.delete(12, 4);
        del(&mut s, 12, 4);
        pt.delete(8, 0);
        del(&mut s, 8, 0);
        pt.delete(10, 2);
        del(&mut s, 10, 2);
        pt.insert(0, &u("VZXXZYZX\r"), false);
        ins(&mut s, 0, "VZXXZYZX\r");
        assert_text(&pt.get_lines_raw_content(), &s);
        test_line_starts(&s, pt);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_insert_delete() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut rng = Rng::new(0x9E37_79B9_0001);
        let mut s = u("");
        let mut buf = create_text_buffer(&ds, &[""], false);
        let pt = buf.get_piece_tree_mut();
        for _ in 0..1000 {
            if rng.next_f64() < 0.6 {
                // insert
                let text = random_str(&mut rng, 100);
                let pos = rng.random_int(s.len() + 1);
                pt.insert(pos, &u(&text), false);
                ins(&mut s, pos, &text);
            } else {
                // delete
                let pos = rng.random_int(s.len());
                let length = (s.len() - pos).min((rng.next_f64() * 10.0) as usize);
                pt.delete(pos, length);
                del(&mut s, pos, length);
            }
        }

        assert_text(&pt.get_lines_raw_content(), &s);

        test_line_starts(&s, pt);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_chunks() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut rng = Rng::new(0x9E37_79B9_0002);
        let chunks: Vec<String> = (0..5).map(|_| random_str(&mut rng, 1000)).collect();

        let chunk_refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
        let mut buf = create_text_buffer(&ds, &chunk_refs, false);
        let pt = buf.get_piece_tree_mut();
        let mut s = u(&chunks.join(""));

        for _ in 0..1000 {
            if rng.next_f64() < 0.6 {
                // insert
                let text = random_str(&mut rng, 100);
                let pos = rng.random_int(s.len() + 1);
                pt.insert(pos, &u(&text), false);
                ins(&mut s, pos, &text);
            } else {
                // delete
                let pos = rng.random_int(s.len());
                let length = (s.len() - pos).min((rng.next_f64() * 10.0) as usize);
                pt.delete(pos, length);
                del(&mut s, pos, length);
            }
        }

        assert_text(&pt.get_lines_raw_content(), &s);
        test_line_starts(&s, pt);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn random_chunks_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut rng = Rng::new(0x9E37_79B9_0003);
        let chunks: Vec<String> = vec![random_str(&mut rng, 1000)];

        let chunk_refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
        let mut buf = create_text_buffer(&ds, &chunk_refs, false);
        let pt = buf.get_piece_tree_mut();
        let mut s = u(&chunks.join(""));

        for _ in 0..50 {
            if rng.next_f64() < 0.6 {
                // insert
                let text = random_str(&mut rng, 30);
                let pos = rng.random_int(s.len() + 1);
                pt.insert(pos, &u(&text), false);
                ins(&mut s, pos, &text);
            } else {
                // delete
                let pos = rng.random_int(s.len());
                let length = (s.len() - pos).min((rng.next_f64() * 10.0) as usize);
                pt.delete(pos, length);
                del(&mut s, pos, length);
            }
            test_lines_content(&s, pt);
        }

        assert_text(&pt.get_lines_raw_content(), &s);
        test_line_starts(&s, pt);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }
}

mod buffer_api {
    use super::*;

    #[test]
    fn equal() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let a = create_text_buffer(&ds, &["abc"], true);
        let b = create_text_buffer(&ds, &["ab", "c"], true);
        let c = create_text_buffer(&ds, &["abd"], true);
        let d = create_text_buffer(&ds, &["abcd"], true);

        assert!(a.get_piece_tree().equal(b.get_piece_tree()));
        assert!(!a.get_piece_tree().equal(c.get_piece_tree()));
        assert!(!a.get_piece_tree().equal(d.get_piece_tree()));
    }

    #[test]
    fn equal_with_more_chunks() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let a = create_text_buffer(&ds, &["ab", "cd", "e"], true);
        let b = create_text_buffer(&ds, &["ab", "c", "de"], true);
        assert!(a.get_piece_tree().equal(b.get_piece_tree()));
    }

    #[test]
    fn equal_2_empty_buffer() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let a = create_text_buffer(&ds, &[""], true);
        let b = create_text_buffer(&ds, &[""], true);

        assert!(a.get_piece_tree().equal(b.get_piece_tree()));
    }

    #[test]
    fn equal_3_empty_buffer() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let a = create_text_buffer(&ds, &["a"], true);
        let b = create_text_buffer(&ds, &[""], true);

        assert!(!a.get_piece_tree().equal(b.get_piece_tree()));
    }

    #[test]
    fn get_line_char_code_issue_45735() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["LINE1\nline2"], true);
        let pt = buf.get_piece_tree_mut();
        assert_eq!(pt.get_line_char_code(1, 0), Some(u16::from(b'L')), "L");
        assert_eq!(pt.get_line_char_code(1, 1), Some(u16::from(b'I')), "I");
        assert_eq!(pt.get_line_char_code(1, 2), Some(u16::from(b'N')), "N");
        assert_eq!(pt.get_line_char_code(1, 3), Some(u16::from(b'E')), "E");
        assert_eq!(pt.get_line_char_code(1, 4), Some(u16::from(b'1')), "1");
        assert_eq!(pt.get_line_char_code(1, 5), Some(u16::from(b'\n')), "\\n");
        assert_eq!(pt.get_line_char_code(2, 0), Some(u16::from(b'l')), "l");
        assert_eq!(pt.get_line_char_code(2, 1), Some(u16::from(b'i')), "i");
        assert_eq!(pt.get_line_char_code(2, 2), Some(u16::from(b'n')), "n");
        assert_eq!(pt.get_line_char_code(2, 3), Some(u16::from(b'e')), "e");
        assert_eq!(pt.get_line_char_code(2, 4), Some(u16::from(b'2')), "2");
    }

    #[test]
    fn get_line_char_code_issue_47733() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["", "LINE1\n", "line2"], true);
        let pt = buf.get_piece_tree_mut();
        assert_eq!(pt.get_line_char_code(1, 0), Some(u16::from(b'L')), "L");
        assert_eq!(pt.get_line_char_code(1, 1), Some(u16::from(b'I')), "I");
        assert_eq!(pt.get_line_char_code(1, 2), Some(u16::from(b'N')), "N");
        assert_eq!(pt.get_line_char_code(1, 3), Some(u16::from(b'E')), "E");
        assert_eq!(pt.get_line_char_code(1, 4), Some(u16::from(b'1')), "1");
        assert_eq!(pt.get_line_char_code(1, 5), Some(u16::from(b'\n')), "\\n");
        assert_eq!(pt.get_line_char_code(2, 0), Some(u16::from(b'l')), "l");
        assert_eq!(pt.get_line_char_code(2, 1), Some(u16::from(b'i')), "i");
        assert_eq!(pt.get_line_char_code(2, 2), Some(u16::from(b'n')), "n");
        assert_eq!(pt.get_line_char_code(2, 3), Some(u16::from(b'e')), "e");
        assert_eq!(pt.get_line_char_code(2, 4), Some(u16::from(b'2')), "2");
    }

    #[test]
    fn get_nearest_chunk() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["012345678"], true);
        let pt = buf.get_piece_tree_mut();
        pt.insert(3, &u("ABC"), false);
        assert_text(&pt.get_line_content(1), &u("012ABC345678"));
        assert_text(&pt.get_nearest_chunk(3), &u("ABC"));
        assert_text(&pt.get_nearest_chunk(6), &u("345678"));
        pt.delete(9, 1);
        assert_text(&pt.get_line_content(1), &u("012ABC34578"));
        assert_text(&pt.get_nearest_chunk(6), &u("345"));
        assert_text(&pt.get_nearest_chunk(9), &u("78"));
    }
}

mod search_offset_cache {
    use super::*;

    #[test]
    fn render_white_space_exception() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["class Name{\n\t\n\t\t\tget() {\n\n\t\t\t}\n\t\t}"], true);
        let pt = buf.get_piece_tree_mut();
        let mut s = u("class Name{\n\t\n\t\t\tget() {\n\n\t\t\t}\n\t\t}");
        pt.insert(12, &u("s"), false);
        ins(&mut s, 12, "s");
        pt.insert(13, &u("e"), false);
        ins(&mut s, 13, "e");
        pt.insert(14, &u("t"), false);
        ins(&mut s, 14, "t");
        pt.insert(15, &u("()"), false);
        ins(&mut s, 15, "()");
        pt.delete(16, 1);
        del(&mut s, 16, 1);
        pt.insert(17, &u("()"), false);
        ins(&mut s, 17, "()");
        pt.delete(18, 1);
        del(&mut s, 18, 1);
        pt.insert(18, &u("}"), false);
        ins(&mut s, 18, "}");
        pt.insert(12, &u("\n"), false);
        ins(&mut s, 12, "\n");
        pt.delete(12, 1);
        del(&mut s, 12, 1);
        pt.delete(18, 1);
        del(&mut s, 18, 1);
        pt.insert(18, &u("}"), false);
        ins(&mut s, 18, "}");
        pt.delete(17, 2);
        del(&mut s, 17, 2);
        pt.delete(16, 1);
        del(&mut s, 16, 1);
        pt.insert(16, &u(")"), false);
        ins(&mut s, 16, ")");
        pt.delete(15, 2);
        del(&mut s, 15, 2);
        let content = pt.get_lines_raw_content();
        assert_text(&content, &s);
    }

    #[test]
    fn line_breaks_replacement_is_not_necessary_when_eol_is_normalized() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["abc"], true);
        let pt = buf.get_piece_tree_mut();
        let mut s = u("abc");
        pt.insert(3, &u("def\nabc"), false);
        s.extend(u("def\nabc"));
        test_line_starts(&s, pt);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn line_breaks_replacement_is_not_necessary_when_eol_is_normalized_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["abc\n"], true);
        let pt = buf.get_piece_tree_mut();
        let mut s = u("abc\n");
        pt.insert(4, &u("def\nabc"), false);
        s.extend(u("def\nabc"));
        test_line_starts(&s, pt);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn line_breaks_replacement_is_not_necessary_when_eol_is_normalized_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["abc\n"], true);
        let pt = buf.get_piece_tree_mut();
        let mut s = u("abc\n");
        pt.insert(2, &u("def\nabc"), false);
        ins(&mut s, 2, "def\nabc");
        test_line_starts(&s, pt);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }

    #[test]
    fn line_breaks_replacement_is_not_necessary_when_eol_is_normalized_4() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &["abc\n"], true);
        let pt = buf.get_piece_tree_mut();
        let mut s = u("abc\n");
        pt.insert(3, &u("def\nabc"), false);
        ins(&mut s, 3, "def\nabc");
        test_line_starts(&s, pt);
        test_lines_content(&s, pt);
        assert_tree_invariants(pt);
    }
}

mod snapshot {
    use super::*;

    #[test]
    fn bug_45564_piece_tree_pieces_should_be_immutable() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut model = create_model(&ds, "\n");
        apply_edit(&mut model, Range::new(2, 1, 2, 1), "!");
        let mut snapshot = model.create_snapshot(true);
        let mut snapshot1 = model.create_snapshot(true);
        assert_text(&lines_joined(&model), get_value_in_snapshot(&mut snapshot).units());

        apply_edit(&mut model, Range::new(2, 1, 2, 2), "");
        apply_edit(&mut model, Range::new(2, 1, 2, 1), "!");

        assert_text(&lines_joined(&model), get_value_in_snapshot(&mut snapshot1).units());

        model.dispose();
    }

    #[test]
    fn immutable_snapshot_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut model = create_model(&ds, "abc\ndef");
        let mut snapshot = model.create_snapshot(true);
        apply_edit(&mut model, Range::new(2, 1, 2, 4), "");

        apply_edit(&mut model, Range::new(1, 1, 2, 1), "abc\ndef");

        assert_text(&lines_joined(&model), get_value_in_snapshot(&mut snapshot).units());

        model.dispose();
    }

    #[test]
    fn immutable_snapshot_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut model = create_model(&ds, "abc\ndef");
        let mut snapshot = model.create_snapshot(true);
        apply_edit(&mut model, Range::new(2, 1, 2, 1), "!");

        apply_edit(&mut model, Range::new(2, 1, 2, 2), "");

        assert_text(&lines_joined(&model), get_value_in_snapshot(&mut snapshot).units());

        model.dispose();
    }

    #[test]
    fn immutable_snapshot_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut model = create_model(&ds, "abc\ndef");
        apply_edit(&mut model, Range::new(2, 4, 2, 4), "!");
        let mut snapshot = model.create_snapshot(true);
        apply_edit(&mut model, Range::new(2, 5, 2, 5), "!");

        assert_ne!(lines_joined(&model), get_value_in_snapshot(&mut snapshot));

        model.dispose();
    }
}

mod chunk_based_search {
    use super::*;

    #[test]
    fn issue_45892_for_some_cases_the_buffer_is_empty_but_we_still_try_to_search() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut buf = create_text_buffer(&ds, &[""], true);
        buf.get_piece_tree_mut().delete(0, 1);
        let search_data = search_data("abc", "", Some(",./"), Some("abc"));
        let ret = buf.find_matches_line_by_line(&Range::new(1, 1, 1, 1), &search_data, true, 1000);
        assert_eq!(ret.len(), 0);
    }

    #[test]
    fn issue_45770_find_in_node_should_not_cross_node_boundary() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = [
            "balabalababalabalababalabalaba",
            "balabalababalabalababalabalaba",
            "",
            "* [ ] task1",
            "* [x] task2 balabalaba",
            "* [ ] task 3",
        ]
        .join("\n");
        let mut buf = create_text_buffer(&ds, &[&text], true);
        let piece_table = buf.get_piece_tree_mut();

        piece_table.delete(0, 62);
        piece_table.delete(16, 1);

        piece_table.insert(16, &u(" "), false);
        let search_data = search_data("\\[", "gi", Some(",./"), Some("["));
        let ret = piece_table.find_matches_line_by_line(&Range::new(1, 1, 4, 13), &search_data, true, 1000);
        assert_eq!(ret.len(), 3);

        assert_eq!(ret[0].range, Range::new(2, 3, 2, 4));
        assert_eq!(ret[1].range, Range::new(3, 3, 3, 4));
        assert_eq!(ret[2].range, Range::new(4, 3, 4, 4));
    }

    #[test]
    fn search_searching_from_the_middle() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["def", "dbcabc"].join("\n");
        let mut buf = create_text_buffer(&ds, &[&text], true);
        let piece_table = buf.get_piece_tree_mut();

        piece_table.delete(4, 1);
        let search_data = search_data("a", "gi", None, Some("a"));
        let mut ret = piece_table.find_matches_line_by_line(&Range::new(2, 3, 2, 6), &search_data, true, 1000);
        assert_eq!(ret.len(), 1);
        assert_eq!(ret[0].range, Range::new(2, 3, 2, 4));

        piece_table.delete(4, 1);
        ret = piece_table.find_matches_line_by_line(&Range::new(2, 2, 2, 5), &search_data, true, 1000);
        assert_eq!(ret.len(), 1);
        assert_eq!(ret[0].range, Range::new(2, 2, 2, 3));
    }
}
