//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalLinkHelpers.test.ts`
//! (suíte `Workbench - Terminal Link Helpers`, `convertLinkRangeToBuffer`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `deepStrictEqual` vira `assert_eq!` sobre o [`IBufferRange`].
//! - `TestBufferLine` é o dublê do original: `getCell` monta uma linha fictícia de células sobre o texto
//!   (em unidades UTF-16, como o `charAt`) e só implementa `getChars` e `getWidth`; o resto da célula e o
//!   `translateToString` do original não existem ou lançam, e aqui são `unreachable!`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::js_string::JsString;
use je_editor::range::Range;
use je_workbench::contrib::terminal::xterm_buffer::{IBufferCell, IBufferCellPosition, IBufferLine, IBufferRange};
use je_workbench::contrib::terminal_contrib::links::terminal_link_helpers::convert_link_range_to_buffer;

/// `TEST_WIDE_CHAR`.
const TEST_WIDE_CHAR: u16 = '文' as u16;
/// `TEST_NULL_CHAR`.
const TEST_NULL_CHAR: u16 = 'C' as u16;

/// O objeto `{ getChars, getWidth } as any` do original.
struct TestBufferCell {
    cells: Vec<Vec<u16>>,
    x: usize,
}

impl TestBufferCell {
    /// `cells[x]`: indefinido no original (e `.charCodeAt` lança) se `x` passa do fim.
    fn cell(&self) -> &[u16] {
        self.cells.get(self.x).expect("cells[x] indefinido (TypeError no original)")
    }
}

impl IBufferCell for TestBufferCell {
    fn get_chars(&self) -> JsString {
        match self.cells.get(self.x) {
            Some(chars) => JsString::from_units(chars.clone()),
            None => JsString::new(),
        }
    }

    fn get_width(&self) -> i32 {
        let cell = self.cell();
        if cell == [TEST_WIDE_CHAR] {
            2
        } else if cell == [TEST_NULL_CHAR] {
            0
        } else if cell.first().is_some_and(|unit| *unit > 255) {
            // Medida ingênua: tudo fora do ascii nos testes é largo.
            2
        } else {
            1
        }
    }

    fn is_bold(&self) -> u32 {
        unreachable!("o objeto do teste só tem getChars e getWidth")
    }

    fn is_inverse(&self) -> u32 {
        unreachable!("o objeto do teste só tem getChars e getWidth")
    }

    fn is_strikethrough(&self) -> u32 {
        unreachable!("o objeto do teste só tem getChars e getWidth")
    }

    fn is_underline(&self) -> u32 {
        unreachable!("o objeto do teste só tem getChars e getWidth")
    }

    fn is_dim(&self) -> u32 {
        unreachable!("o objeto do teste só tem getChars e getWidth")
    }

    fn is_italic(&self) -> u32 {
        unreachable!("o objeto do teste só tem getChars e getWidth")
    }
}

struct TestBufferLine {
    text: Vec<u16>,
    length: i32,
    is_wrapped: bool,
}

impl IBufferLine for TestBufferLine {
    fn is_wrapped(&self) -> bool {
        self.is_wrapped
    }

    fn length(&self) -> i32 {
        self.length
    }

    fn get_cell(&self, x: i32) -> Option<Rc<dyn IBufferCell>> {
        // Cria uma linha fictícia de células e usa ela para resolver a largura.
        let mut cells: Vec<Vec<u16>> = Vec::new();
        let mut wide_null_cell_offset = 0; // Não há caractere nulo de largura 0 depois de um largo.
        let emoji_offset = 0; // Pula caracteres, já que emoji são vários.
        let mut i = 0;
        while i <= x - wide_null_cell_offset + emoji_offset {
            let unit = usize::try_from(i).ok().and_then(|index| self.text.get(index)).copied();
            let mut chars: Vec<u16> = unit.into_iter().collect();
            if unit == Some(0xD83D) {
                // Faz "🙂".
                chars.push(0xDE42);
            }
            let first_unit = chars.first().copied();
            cells.push(chars);
            if unit == Some(TEST_WIDE_CHAR) || first_unit.is_some_and(|first| first > 255) {
                // Pula o próximo caractere, já que a largura dele é 0.
                cells.push(vec![TEST_NULL_CHAR]);
                wide_null_cell_offset += 1;
            }
            i += 1;
        }
        let x = usize::try_from(x).ok()?;
        Some(Rc::new(TestBufferCell { cells, x }))
    }

    fn translate_to_string(&self, _trim_right: Option<bool>, _start_column: Option<i32>, _end_column: Option<i32>) -> JsString {
        panic!("Method not implemented.");
    }
}

/// `createBufferLineArray`: só a última linha não é continuação da seguinte.
fn create_buffer_line_array(lines: &[(&str, i32)]) -> Vec<Rc<dyn IBufferLine>> {
    lines
        .iter()
        .enumerate()
        .map(|(i, (text, width))| {
            Rc::new(TestBufferLine { text: text.encode_utf16().collect(), length: *width, is_wrapped: i + 1 != lines.len() }) as Rc<dyn IBufferLine>
        })
        .collect()
}

/// `{ startColumn, startLineNumber: 1, endColumn, endLineNumber: 1 }`.
fn link(start_column: i32, end_column: i32) -> Range {
    Range { start_line_number: 1, start_column, end_line_number: 1, end_column }
}

/// `{ start: { x, y }, end: { x, y } }`.
fn buffer_range(start: (i32, i32), end: (i32, i32)) -> IBufferRange {
    IBufferRange { start: IBufferCellPosition { x: start.0, y: start.1 }, end: IBufferCellPosition { x: end.0, y: end.1 } }
}

// suite('convertLinkRangeToBuffer')

#[test]
fn should_convert_ranges_for_ascii_characters() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("AA http://t", 11), (".com/f/", 8)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(4, 19), 0);
    assert_eq!(range, buffer_range((4, 1), (7, 2)));
}

#[test]
fn should_convert_ranges_for_wide_characters_before_the_link() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("A文 http://", 11), ("t.com/f/", 9)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(4, 19), 0);
    assert_eq!(range, buffer_range((4 + 1, 1), (7 + 1, 2)));
}

#[test]
fn should_give_correct_range_for_links_containing_multi_character_emoji() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("A🙂 http://", 11)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(1, 2 + 1), 0);
    assert_eq!(range, buffer_range((1, 1), (2, 1)));
}

#[test]
fn should_convert_ranges_for_combining_characters_before_the_link() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("A🙂 http://", 11), ("t.com/f/", 9)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(4 + 1, 19 + 1), 0);
    assert_eq!(range, buffer_range((6, 1), (9, 2)));
}

#[test]
fn should_convert_ranges_for_wide_characters_inside_the_link() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("AA http://t", 11), (".com/文/", 8)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(4, 19), 0);
    assert_eq!(range, buffer_range((4, 1), (7 + 1, 2)));
}

#[test]
fn should_convert_ranges_for_wide_characters_before_and_inside_the_link() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("A文 http://", 11), ("t.com/文/", 9)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(4, 19), 0);
    assert_eq!(range, buffer_range((4 + 1, 1), (7 + 2, 2)));
}

#[test]
fn should_convert_ranges_for_emoji_before_and_wide_inside_the_link() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("A🙂 http://", 11), ("t.com/文/", 9)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(4 + 1, 19 + 1), 0);
    assert_eq!(range, buffer_range((6, 1), (10 + 1, 2)));
}

#[test]
fn should_convert_ranges_for_ascii_characters_link_starts_on_wrapped() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("AAAAAAAAAAA", 11), ("AA http://t", 11), (".com/f/", 8)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(15, 30), 0);
    assert_eq!(range, buffer_range((4, 2), (7, 3)));
}

#[test]
fn should_convert_ranges_for_wide_characters_before_the_link_link_starts_on_wrapped() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("AAAAAAAAAAA", 11), ("A文 http://", 11), ("t.com/f/", 9)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(15, 30), 0);
    assert_eq!(range, buffer_range((4 + 1, 2), (7 + 1, 3)));
}

#[test]
fn regression_test_147619_preview_image_failed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("获取模板 25235168 的预览图失败", 30)]);
    assert_eq!(convert_link_range_to_buffer(&lines, 30, &link(1, 5), 0), buffer_range((1, 1), (8, 1)));
    assert_eq!(convert_link_range_to_buffer(&lines, 30, &link(6, 14), 0), buffer_range((10, 1), (17, 1)));
    assert_eq!(convert_link_range_to_buffer(&lines, 30, &link(15, 21), 0), buffer_range((19, 1), (30, 1)));
}

#[test]
fn should_convert_ranges_for_wide_characters_inside_the_link_link_starts_on_wrapped() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("AAAAAAAAAAA", 11), ("AA http://t", 11), (".com/文/", 8)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(15, 30), 0);
    assert_eq!(range, buffer_range((4, 2), (7 + 1, 3)));
}

#[test]
fn should_convert_ranges_for_wide_characters_before_and_inside_the_link_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("AAAAAAAAAAA", 11), ("A文 http://", 11), ("t.com/文/", 9)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(15, 30), 0);
    assert_eq!(range, buffer_range((4 + 1, 2), (7 + 2, 3)));
}

#[test]
fn should_convert_ranges_for_several_wide_characters_before_the_link() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("A文文AAAAAA", 11), ("AA文文 http", 11), ("://t.com/f/", 11)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(15, 30), 0);
    // Este teste garante que o deslocamento do início é aplicado ao fim antes de ele ser contado.
    assert_eq!(range, buffer_range((3 + 4, 2), (6 + 4, 3)));
}

#[test]
fn should_convert_ranges_for_several_wide_characters_before_and_inside_the_link() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = create_buffer_line_array(&[("A文文AAAAAA", 11), ("AA文文 http", 11), ("://t.com/文", 11), ("文/", 3)]);
    let range = convert_link_range_to_buffer(&lines, 11, &link(14, 31), 0);
    // Este teste garante que o deslocamento do início é aplicado ao fim antes de ele ser contado.
    assert_eq!(range, buffer_range((5, 2), (1, 4)));
}
