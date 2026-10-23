//! Teste diferencial do piece tree contra um modelo simples (`Vec<u16>`), com muitas sementes e um alfabeto
//! carregado de `\r` e `\n` (os casos de borda de CRLF dividido entre peças). Fica fora do placar de
//! conformidade: não é porte de teste do VS Code, é a rede de segurança do porte.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::strings;
use je_editor::model::{EndOfLinePreference, ValidAnnotatedEditOperation};
use je_editor::position::Position;
use je_editor::range::Range;
use support::*;

fn random_text(rng: &mut Rng, max_len: usize, alphabet: &[u16]) -> Vec<u16> {
    // Nunca vazio: o `insert` com texto vazio cria uma peça vazia, e nenhum chamador do original faz isso.
    let len = 1 + rng.random_int(max_len);
    (0..len).map(|_| alphabet[rng.random_int(alphabet.len())]).collect()
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn check_all(model: &[u16], pt: &je_editor::piece_tree_base::PieceTreeBase, check_line_length: bool) {
    assert_eq!(pt.get_length(), model.len());
    assert_text(&pt.get_lines_raw_content(), model);
    test_line_starts(model, pt);
    test_lines_content(model, pt);
    assert_tree_invariants(pt);

    // `getLinesContent` e `getLineLength` contra as linhas do modelo.
    let lines = strings::split_lines(model);
    let all = pt.get_lines_content();
    assert_eq!(all.len(), lines.len());
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(all[i].units(), *line, "getLinesContent, linha {}", i + 1);
        if check_line_length {
            // `getLineLength` supõe um único comprimento de fim de linha: só vale com o EOL normalizado.
            assert_eq!(pt.get_line_length(i as i32 + 1), line.len(), "getLineLength, linha {}", i + 1);
        }
    }
}

#[test]
fn random_ops_with_mixed_line_breaks_match_a_simple_model() {
    let alphabets: [Vec<u16>; 3] = [
        units("ab\r\n"),
        units("a\r\n\r\n\n\rb"),
        // pares substitutos e BOM no meio também têm de passar sem mexer nas linhas
        vec![u16::from(b'a'), u16::from(b'\n'), u16::from(b'\r'), 0xD83D, 0xDE0A, 0xFEFF, 0x2028],
    ];
    for seed in 0..60u64 {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut rng = Rng::new(seed * 7919 + 1);
        let alphabet = &alphabets[(seed % 3) as usize];
        let chunk_count = rng.random_int(4);
        let chunks: Vec<String> = (0..chunk_count)
            .map(|_| String::from_utf16_lossy(&random_text(&mut rng, 60, &units("ab\r\n"))))
            .collect();
        let chunk_refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
        let mut buf = create_text_buffer(&_ds, &chunk_refs, false);
        let pt = buf.get_piece_tree_mut();
        let mut model: Vec<u16> = chunks.iter().flat_map(|c| c.encode_utf16()).collect();
        // O construtor guarda um `\r` final do último pedaço junto: o conteúdo bruto não muda.
        check_all(&model, pt, false);

        for _ in 0..200 {
            if rng.next_f64() < 0.55 {
                let text = random_text(&mut rng, 40, alphabet);
                let pos = rng.random_int(model.len() + 1);
                pt.insert(pos, &text, false);
                model.splice(pos..pos, text);
            } else if !model.is_empty() {
                let pos = rng.random_int(model.len());
                let len = (model.len() - pos).min(rng.random_int(25));
                pt.delete(pos, len);
                model.drain(pos..pos + len);
            }
            check_all(&model, pt, false);
        }
    }
}

#[test]
fn random_ops_on_normalized_lf_buffer_match_a_simple_model() {
    let alphabet = units("ab\n");
    for seed in 0..60u64 {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut rng = Rng::new(seed * 104729 + 3);
        let initial = String::from_utf16_lossy(&random_text(&mut rng, 80, &alphabet));
        let mut buf = create_text_buffer(&ds, &[&initial], true);
        let pt = buf.get_piece_tree_mut();
        let mut model = units(&initial);
        check_all(&model, pt, true);

        for _ in 0..300 {
            if rng.next_f64() < 0.55 {
                let text = random_text(&mut rng, 40, &alphabet);
                let pos = rng.random_int(model.len() + 1);
                pt.insert(pos, &text, true);
                model.splice(pos..pos, text);
            } else if !model.is_empty() {
                let pos = rng.random_int(model.len());
                let len = (model.len() - pos).min(rng.random_int(25));
                pt.delete(pos, len);
                model.drain(pos..pos + len);
            }
            check_all(&model, pt, true);
        }
    }
}

#[test]
fn large_insert_is_split_into_chunks_and_stays_consistent() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut buf = create_text_buffer(&ds, &["x\r"], false);
    let pt = buf.get_piece_tree_mut();
    let mut model = units("x\r");
    let mut big: Vec<u16> = Vec::new();
    for i in 0..200_000usize {
        big.push(if i % 37 == 0 {
            13
        } else if i % 41 == 0 {
            10
        } else {
            u16::from(b'a') + (i % 26) as u16
        });
    }
    // O primeiro caractere é `\n` para formar CRLF com o `\r` do buffer original.
    big[0] = 10;
    pt.insert(2, &big, false);
    model.splice(2..2, big);
    check_all(&model, pt, false);
    pt.delete(1000, 70_000);
    model.drain(1000..71_000);
    check_all(&model, pt, false);
}

#[test]
fn set_eol_normalizes_the_whole_buffer() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut buf = create_text_buffer(&ds, &["a\r\nb\nc\rd"], true);
    buf.set_eol(je_editor::model::Eol::CrLf);
    assert_text(&buf.get_piece_tree().get_lines_raw_content(), &units("a\r\nb\r\nc\r\nd"));
    assert_eq!(buf.get_line_count(), 4);
    buf.set_eol(je_editor::model::Eol::Lf);
    assert_text(&buf.get_piece_tree().get_lines_raw_content(), &units("a\nb\nc\nd"));
    assert_tree_invariants(buf.get_piece_tree());
}

/// Posição (linha, coluna) de um deslocamento num texto só com `\n`.
fn position_of(model: &[u16], offset: usize) -> Position {
    let line = 1 + model[..offset].iter().filter(|&&c| c == 10).count();
    let line_start = model[..offset].iter().rposition(|&c| c == 10).map_or(0, |i| i + 1);
    Position::new(line as i32, (offset - line_start) as i32 + 1)
}

#[test]
fn apply_edits_matches_a_simple_model_and_its_reverse_edits_restore_the_text() {
    let alphabet = units("ab\n\r");
    for seed in 0..150u64 {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let mut rng = Rng::new(seed * 15485863 + 7);
        let initial = String::from_utf16_lossy(&random_text(&mut rng, 120, &units("ab\n")));
        let mut buf = create_text_buffer(&ds, &[&initial], true);
        let original = units(&initial);

        // Edições disjuntas em ordem crescente de deslocamento.
        let edit_count = 1 + rng.random_int(6);
        let mut cuts: Vec<usize> = (0..edit_count * 2).map(|_| rng.random_int(original.len() + 1)).collect();
        cuts.sort_unstable();
        let mut edits: Vec<(usize, usize, Vec<u16>)> = Vec::new();
        for pair in cuts.chunks(2) {
            edits.push((pair[0], pair[1], random_text(&mut rng, 10, &alphabet)));
        }

        // Mistura a ordem das operações: o buffer ordena por conta própria (pelo fim, depois pelo
        // início, depois pela posição na lista), e a ordem na lista decide os empates.
        let mut shuffled = edits.clone();
        for i in (1..shuffled.len()).rev() {
            let j = rng.random_int(i + 1);
            shuffled.swap(i, j);
        }
        let mut ordered = shuffled.clone();
        ordered.sort_by_key(|(start, end, _)| (*end, *start));

        let mut expected: Vec<u16> = Vec::new();
        let mut last = 0;
        for (start, end, text) in &ordered {
            expected.extend_from_slice(&original[last..*start]);
            // O buffer normaliza `\r` e `\r\n` do texto novo para `\n`.
            let normalized = String::from_utf16_lossy(text).replace("\r\n", "\n").replace('\r', "\n");
            expected.extend(normalized.encode_utf16());
            last = *end;
        }
        expected.extend_from_slice(&original[last..]);

        let ops: Vec<ValidAnnotatedEditOperation> = shuffled
            .iter()
            .map(|(start, end, text)| ValidAnnotatedEditOperation {
                identifier: None,
                range: Range::from_positions(position_of(&original, *start), position_of(&original, *end)),
                text: Some(JsString::from_units(text.clone())),
                force_move_markers: false,
                is_auto_whitespace_edit: false,
                is_tracked: false,
            })
            .collect();

        let result = buf.apply_edits(&ops, false, true);
        assert_text(&buf.get_piece_tree().get_lines_raw_content(), &expected);
        assert_tree_invariants(buf.get_piece_tree());

        // Desfazer: as edições reversas devolvem o texto original.
        let reverse = result.reverse_edits.expect("pediu as edições reversas");
        let undo: Vec<ValidAnnotatedEditOperation> = reverse
            .iter()
            .map(|op| ValidAnnotatedEditOperation {
                identifier: None,
                range: op.range,
                text: Some(op.text.clone()),
                force_move_markers: false,
                is_auto_whitespace_edit: false,
                is_tracked: false,
            })
            .collect();
        buf.apply_edits(&undo, false, false);
        assert_text(&buf.get_piece_tree().get_lines_raw_content(), &original);
        assert_tree_invariants(buf.get_piece_tree());
    }
}

#[test]
fn value_in_range_and_lengths_follow_the_requested_eol() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let buf = create_text_buffer(&ds, &["ab\ncd\nef"], true);
    let range = Range::new(1, 2, 3, 2);
    assert_text(
        &buf.get_value_in_range(&range, EndOfLinePreference::TextDefined),
        &units("b\ncd\ne"),
    );
    assert_text(&buf.get_value_in_range(&range, EndOfLinePreference::CrLf), &units("b\r\ncd\r\ne"));
    assert_eq!(buf.get_value_length_in_range(&range, EndOfLinePreference::TextDefined), 6);
    assert_eq!(buf.get_value_length_in_range(&range, EndOfLinePreference::CrLf), 8);
    assert_eq!(buf.get_character_count_in_range(&range, EndOfLinePreference::CrLf), 8);
    assert_eq!(buf.get_line_first_non_whitespace_column(1), 1);
    assert_eq!(buf.get_line_max_column(2), 3);
}
