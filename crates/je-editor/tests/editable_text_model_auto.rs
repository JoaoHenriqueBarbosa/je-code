//! Porte de `src/vs/editor/test/common/model/editableTextModelAuto.test.ts` (suíte `EditorModel Auto Tests`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - Os quatro casos são fixos (foram gerados uma vez pelo gerador do próprio arquivo). O gerador
//!   (`getRandomInt`, `getRandomString`, `generateFile`, `generateEdits`, `TestModel` e o laço sob
//!   `GENERATE_TESTS`) está portado logo abaixo dos casos, com o `Math.random` trocado pelo gerador de
//!   semente fixa [`Rng`] de `tests/support/mod.rs`. Como no original, ele só roda com
//!   `GENERATE_TESTS = true` (o laço do original fica no carregamento do módulo; aqui é
//!   [`generate_tests`], que nenhum caso chama). Por isso esses itens levam `#[allow(dead_code)]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::errors;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::position::Position;
use je_editor::range::Range;
use support::Rng;
use support::editable_text_model_test_utils::*;

const GENERATE_TESTS: bool = false;

/// `editOp(startLineNumber, startColumn, endLineNumber, endColumn, text)`.
fn edit_op(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32, text: &[&str]) -> SingleEditOperation {
    SingleEditOperation::new(
        Range::new(start_line_number, start_column, end_line_number, end_column),
        Some(JsString::from(text.join("\n").as_str())),
        false,
    )
}

/// `auto1`
#[test]
fn auto1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_apply_edits_with_synced_models(
        &["ioe", "", "yjct", "", ""],
        &[
            edit_op(1, 2, 1, 2, &["b", "r", "fq"]),
            edit_op(1, 4, 2, 1, &["", ""]),
        ],
        &["ib", "r", "fqoe", "", "yjct", "", ""],
        false,
    );
}

/// `auto2`
#[test]
fn auto2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_apply_edits_with_synced_models(
        &["f", "littnhskrq", "utxvsizqnk", "lslqz", "jxn", "gmm"],
        &[
            edit_op(1, 2, 1, 2, &["", "o"]),
            edit_op(2, 4, 2, 4, &["zaq", "avb"]),
            edit_op(2, 5, 6, 2, &["jlr", "zl", "j"]),
        ],
        &["f", "o", "litzaq", "avbtjlr", "zl", "jmm"],
        false,
    );
}

/// `auto3`
#[test]
fn auto3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_apply_edits_with_synced_models(
        &["ofw", "qsxmziuvzw", "rp", "qsnymek", "elth", "wmgzbwudxz", "iwsdkndh", "bujlbwb", "asuouxfv", "xuccnb"],
        &[
            edit_op(4, 3, 4, 3, &[""]),
        ],
        &["ofw", "qsxmziuvzw", "rp", "qsnymek", "elth", "wmgzbwudxz", "iwsdkndh", "bujlbwb", "asuouxfv", "xuccnb"],
        false,
    );
}

/// `auto4`
#[test]
fn auto4() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_apply_edits_with_synced_models(
        &["fefymj", "qum", "vmiwxxaiqq", "dz", "lnqdgorosf"],
        &[
            edit_op(1, 3, 1, 5, &["hp"]),
            edit_op(1, 7, 2, 1, &["kcg", "", "mpx"]),
            edit_op(2, 2, 2, 2, &["", "aw", ""]),
            edit_op(2, 2, 2, 2, &["vqr", "mo"]),
            edit_op(4, 2, 5, 3, &["xyc"]),
        ],
        &["fehpmjkcg", "", "mpxq", "aw", "vqr", "moum", "vmiwxxaiqq", "dxycqdgorosf"],
        false,
    );
}

/// `getRandomInt(min, max)`.
#[allow(dead_code)]
fn get_random_int(rng: &mut Rng, min: usize, max: usize) -> usize {
    (rng.next_f64() * (max - min + 1) as f64).floor() as usize + min
}

/// `getRandomString(minLength, maxLength)`.
#[allow(dead_code)]
fn get_random_string(rng: &mut Rng, min_length: usize, max_length: usize) -> String {
    let length = get_random_int(rng, min_length, max_length);
    (0..length)
        .map(|_| char::from(get_random_int(rng, usize::from(b'a'), usize::from(b'z')) as u8))
        .collect()
}

/// `generateFile(small)`.
#[allow(dead_code)]
fn generate_file(rng: &mut Rng, small: bool) -> String {
    let line_count = get_random_int(rng, 1, if small { 3 } else { 10 });
    let lines: Vec<String> = (0..line_count)
        .map(|_| get_random_string(rng, 0, if small { 3 } else { 10 }))
        .collect();
    lines.join("\n")
}

/// `ITestModelEdit`.
#[allow(dead_code)]
struct TestModelEdit {
    offset: usize,
    length: usize,
    text: String,
}

/// `generateEdits(content)`. O conteúdo gerado é só ASCII, então o comprimento em bytes é o
/// comprimento em unidades UTF-16 do original.
#[allow(dead_code)]
fn generate_edits(rng: &mut Rng, content: &str) -> Vec<TestModelEdit> {
    let mut result: Vec<TestModelEdit> = Vec::new();
    let mut cnt = get_random_int(rng, 1, 5);

    let mut max_offset = content.len();

    while cnt > 0 && max_offset > 0 {
        let offset = get_random_int(rng, 0, max_offset);
        let length = get_random_int(rng, 0, max_offset - offset);
        let text = generate_file(rng, true);

        result.push(TestModelEdit { offset, length, text });

        max_offset = offset;
        cnt -= 1;
    }

    result.reverse();

    result
}

/// `TestModel`.
#[allow(dead_code)]
struct TestModel {
    initial_content: String,
    resulting_content: String,
    edits: Vec<SingleEditOperation>,
}

#[allow(dead_code)]
impl TestModel {
    /// `_generateOffsetToPosition(content)`.
    fn generate_offset_to_position(content: &str) -> Vec<Position> {
        let mut result: Vec<Position> = Vec::new();
        let mut line_number = 1;
        let mut column = 1;

        let bytes = content.as_bytes();
        for offset in 0..=bytes.len() {
            let ch = bytes.get(offset).copied();

            result.push(Position::new(line_number, column));

            if ch == Some(b'\n') {
                line_number += 1;
                column = 1;
            } else {
                column += 1;
            }
        }

        result
    }

    fn new(rng: &mut Rng) -> Self {
        let initial_content = generate_file(rng, false);

        let edits = generate_edits(rng, &initial_content);

        let offset_to_position = TestModel::generate_offset_to_position(&initial_content);
        let mut model_edits = Vec::new();
        for edit in &edits {
            let start_position = offset_to_position[edit.offset];
            let end_position = offset_to_position[edit.offset + edit.length];
            model_edits.push(SingleEditOperation::new(
                Range::new(start_position.line_number, start_position.column, end_position.line_number, end_position.column),
                Some(JsString::from(edit.text.as_str())),
                false,
            ));
        }

        let mut resulting_content = initial_content.clone();
        for edit in edits.iter().rev() {
            resulting_content = format!(
                "{}{}{}",
                &resulting_content[..edit.offset],
                edit.text,
                &resulting_content[edit.offset + edit.length..]
            );
        }

        TestModel { initial_content, resulting_content, edits: model_edits }
    }

    fn print(&self) -> String {
        let mut r: Vec<String> = vec!["testApplyEditsWithSyncedModels(".to_owned(), "\t[".to_owned()];
        r.extend(self.initial_content.split('\n').map(|i| format!("\t\t'{i}',")));
        r.push("\t],".to_owned());
        r.push("\t[".to_owned());
        r.extend(self.edits.iter().map(|i| {
            let text = i.text.as_ref().map(JsString::to_string_lossy).unwrap_or_default();
            let text = format!("['{}']", text.split('\n').collect::<Vec<_>>().join("', '"));
            format!(
                "\t\teditOp({}, {}, {}, {}, {text}),",
                i.range.start_line_number, i.range.start_column, i.range.end_line_number, i.range.end_column
            )
        }));
        r.push("\t],".to_owned());
        r.push("\t[".to_owned());
        r.extend(self.resulting_content.split('\n').map(|i| format!("\t\t'{i}',")));
        r.push("\t]".to_owned());
        r.push(");".to_owned());

        r.join("\n")
    }
}

/// O laço `if (GENERATE_TESTS) { ... }` do original: gera modelos até um falhar e imprime o caso.
#[allow(dead_code)]
fn generate_tests(seed: u64) {
    if !GENERATE_TESTS {
        return;
    }
    let mut rng = Rng::new(seed);
    let mut number = 1;
    loop {
        println!("------BEGIN NEW TEST: {number}");

        let test_model = TestModel::new(&mut rng);

        println!("------END NEW TEST: {number}");
        number += 1;

        let initial_lines: Vec<&str> = test_model.initial_content.split('\n').collect();
        let resulting_lines: Vec<&str> = test_model.resulting_content.split('\n').collect();
        if let Err(err) = errors::catch(|| {
            test_apply_edits_with_synced_models(&initial_lines, &test_model.edits, &resulting_lines, false)
        }) {
            println!("{err:?}");
            println!("{}", test_model.print());
            break;
        }
    }
}
