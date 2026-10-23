//! Porte de `src/vs/editor/test/common/viewLayout/viewLineRenderer.test.ts` (suítes `renderViewLine`,
//! `renderViewLine2` e `renderViewLine - useTwoCellFullwidthCharacters`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `createRenderLineInput({...})` (o `Partial` espalhado sobre os padrões) vira
//!   [`create_render_line_input`], que parte de [`default_options`] e aplica uma closure de ajustes.
//! - `assertSnapshot` lê os arquivos de `ref/vscode/.../__snapshots__` em tempo de teste, com o mesmo nome que o
//!   VS Code dá (`<suíte e título sanitizados>.<índice>.<extensão>`). O mapeamento (`inflate()`) é formatado
//!   como o `formatValue` do original formata um array de arrays de números.
//! - `renderViewLine2 as renderViewLine` do original é `render_view_line2`.
//! - O `new RenderLineInput(...)` posicional do caso `issue-260239` vira o literal de `RenderLineInputOptions`
//!   com todos os campos, na ordem dos argumentos.
//! - Os `\uXXXX` e os caracteres combinantes do original que o editor poderia normalizar ao copiar viram
//!   escapes (`\u{...}`); as cadeias longas em escritas que não são latinas seguem literais.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::path::Path;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::char_code as cc;
use je_base::strings;
use je_editor::encoded_token_attributes::metadata_consts;
use je_editor::inline_decorations::InlineDecorationType;
use je_editor::line_decorations::LineDecoration;
use je_editor::line_tokens::IViewLineTokens;
use je_editor::offset_range::OffsetRange;
use je_editor::view_line_renderer::{
    CharacterMapping, DomPosition, RenderLineInput, RenderLineInputOptions, RenderLineOutput2, render_view_line2,
};
use support::test_line_token::{TestLineToken, TestLineTokens};

const SNAPSHOTS_DIR: &str = "../../ref/vscode/src/vs/editor/test/common/viewLayout/__snapshots__";

fn create_view_line_tokens(view_line_tokens: Vec<TestLineToken>) -> Rc<dyn IViewLineTokens> {
    Rc::new(TestLineTokens::new(view_line_tokens))
}

fn create_part(end_index: i32, foreground: u32) -> TestLineToken {
    TestLineToken::new(end_index, foreground << metadata_consts::FOREGROUND_OFFSET)
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

/// `str.length` do JavaScript (unidades UTF-16).
fn utf16_len(text: &str) -> i32 {
    text.encode_utf16().count() as i32
}

fn utf16_units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn is_basic_ascii(text: &str) -> bool {
    strings::is_basic_ascii(&utf16_units(text))
}

fn contains_rtl(text: &str) -> bool {
    strings::contains_rtl(&utf16_units(text))
}

fn html(output: &RenderLineOutput2) -> String {
    output.html.to_string_lossy().to_string()
}

/// A saída de `inflateRenderLineOutput`: as partes do HTML e o mapeamento inflado.
struct Inflated {
    html: Vec<String>,
    mapping: Vec<(u32, u32, u32)>,
}

fn inflate_render_line_output(render_line_output: &RenderLineOutput2) -> Inflated {
    // Remove o `<span>` que envolve tudo para simplificar a escrita dos testes.
    let mut html = html(render_line_output);
    if let Some(rest) = html.strip_prefix("<span>") {
        html = rest.to_owned();
    }
    if let Some(rest) = html.strip_suffix("</span>") {
        html = rest.to_owned();
    }
    let mut spans: Vec<String> = Vec::new();
    let mut last_index = 0;
    loop {
        // `indexOf('<span', lastIndex + 1)`: o `+ 1` do original, sem cair no meio de um caractere.
        let from = last_index + html[last_index..].chars().next().map_or(1, char::len_utf8);
        let Some(new_index) = html.get(from..).and_then(|rest| rest.find("<span")).map(|offset| from + offset) else {
            break;
        };
        spans.push(html[last_index..new_index].to_owned());
        last_index = new_index;
    }
    spans.push(html[last_index..].to_owned());

    Inflated { html: spans, mapping: render_line_output.character_mapping.inflate() }
}

fn default_options() -> RenderLineInputOptions {
    RenderLineInputOptions {
        use_monospace_optimizations: false,
        can_use_halfwidth_rightwards_arrow: true,
        line_content: js(""),
        continues_with_wrapped_line: false,
        is_basic_ascii: true,
        contains_rtl: false,
        faux_indent_length: 0,
        line_tokens: create_view_line_tokens(Vec::new()),
        line_decorations: Vec::new(),
        tab_size: 4,
        start_visible_column: 0,
        space_width: 10.0,
        middot_width: 10.0,
        wsmiddot_width: 10.0,
        stop_rendering_line_after: -1,
        render_whitespace: "none".to_owned(),
        render_control_characters: false,
        font_ligatures: false,
        selections_on_line: None,
        text_direction: None,
        vertical_scrollbar_size: 14.0,
        render_new_line_when_empty: false,
        use_two_cell_fullwidth_characters: false,
    }
}

fn create_render_line_input(configure: impl FnOnce(&mut RenderLineInputOptions)) -> RenderLineInput {
    let mut options = default_options();
    configure(&mut options);
    RenderLineInput::new(options)
}

/// `renderViewLine(createRenderLineInput({...}))`.
fn render_view_line(configure: impl FnOnce(&mut RenderLineInputOptions)) -> RenderLineOutput2 {
    render_view_line2(&create_render_line_input(configure))
}

// #region Snapshots

/// O nome do arquivo de snapshot: `sanitizeName(test.fullTitle()) + '.' + índice + '.' + extensão`.
fn snapshot_file_name(full_title: &str, index: usize, extension: &str) -> String {
    let sanitized: String = full_title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    format!("{sanitized}.{index}.{extension}")
}

/// `assertSnapshot(value, { extension })` para um valor de texto.
fn assert_snapshot(full_title: &str, index: usize, extension: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(SNAPSHOTS_DIR)
        .join(snapshot_file_name(full_title, index, extension));
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("snapshot ilegível ({}): {error}", path.display()));
    assert_eq!(
        expected.replace("\r\n", "\n"),
        actual.replace("\r\n", "\n"),
        "snapshot #{index} de {full_title} difere do esperado"
    );
}

/// O `formatValue` do original para um array de triplas de números: uma linha se couber em 80 colunas.
fn format_mapping(mapping: &[(u32, u32, u32)]) -> String {
    let children: Vec<String> = mapping.iter().map(|(a, b, c)| format!("[ {a}, {b}, {c} ]")).collect();
    if children.join(", ").len() > 80 {
        format!("[\n  {}\n]", children.join(",\n  "))
    } else {
        format!("[ {} ]", children.join(", "))
    }
}

/// O par de `assertSnapshot(inflated.html.join(''), HTML_EXTENSION)` e `assertSnapshot(inflated.mapping)`.
fn assert_html_and_mapping_snapshots(full_title: &str, inflated: &Inflated) {
    assert_snapshot(full_title, 0, "html", &inflated.html.join(""));
    assert_snapshot(full_title, 1, "snap", &format_mapping(&inflated.mapping));
}

// #endregion

/// `[horizontalOffset, [partIndex, charIndex]]`.
type CharacterMappingInfo = (u32, (u32, u32));

fn assert_character_mapping3(actual: &CharacterMapping, expected_info: &[CharacterMappingInfo]) {
    for (i, &(horizontal_offset, (part_index, char_index))) in expected_info.iter().enumerate() {
        let column = i as i32 + 1;

        let actual_dom_position = actual.get_dom_position(column);
        assert_eq!(actual_dom_position, DomPosition::new(part_index, char_index), "getDomPosition({column})");

        let mut part_length = char_index + 1;
        for &(_, (next_part_index, next_char_index)) in &expected_info[i + 1..] {
            if next_part_index == part_index {
                part_length = next_char_index + 1;
            } else {
                break;
            }
        }

        let actual_column = actual.get_column(&DomPosition::new(part_index, char_index), part_length);
        assert_eq!(actual_column, column, "actual.getColumn({part_index}, {char_index})");

        let actual_horizontal_offset = actual.get_horizontal_offset(column);
        assert_eq!(actual_horizontal_offset, horizontal_offset, "actual.getHorizontalOffset({column})");
    }

    assert_eq!(actual.length, expected_info.len(), "length mismatch");
}

// #region suite('renderViewLine')

fn assert_character_replacement(line_content: &str, tab_size: i32, expected: &str, expected_char_offset_in_part: &[u32]) {
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.is_basic_ascii = is_basic_ascii(line_content);
        o.line_tokens = create_view_line_tokens(vec![TestLineToken::new(utf16_len(line_content), 0)]);
        o.tab_size = tab_size;
        o.space_width = 0.0;
        o.middot_width = 0.0;
        o.wsmiddot_width = 0.0;
    });

    assert_eq!(html(&actual), format!("<span><span class=\"mtk0\">{expected}</span></span>"));
    let info: Vec<CharacterMappingInfo> = expected_char_offset_in_part
        .iter()
        .map(|&absolute_offset| (absolute_offset, (0, absolute_offset)))
        .collect();
    assert_character_mapping3(&actual.character_mapping, &info);
}

#[test]
fn replaces_spaces() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_character_replacement(" ", 4, "\u{a0}", &[0, 1]);
    assert_character_replacement("  ", 4, "\u{a0}\u{a0}", &[0, 1, 2]);
    assert_character_replacement("a  b", 4, "a\u{a0}\u{a0}b", &[0, 1, 2, 3, 4]);
}

#[test]
fn escapes_html_markup() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_character_replacement("a<b", 4, "a&lt;b", &[0, 1, 2, 3]);
    assert_character_replacement("a>b", 4, "a&gt;b", &[0, 1, 2, 3]);
    assert_character_replacement("a&b", 4, "a&amp;b", &[0, 1, 2, 3]);
}

#[test]
fn replaces_some_bad_characters() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let utf8_bom = char::from_u32(u32::from(cc::UTF8_BOM)).expect("BOM é um caractere válido");
    assert_character_replacement("a\0b", 4, "a&#00;b", &[0, 1, 2, 3]);
    assert_character_replacement(&format!("a{utf8_bom}b"), 4, "a\u{fffd}b", &[0, 1, 2, 3]);
    assert_character_replacement("a\u{2028}b", 4, "a\u{fffd}b", &[0, 1, 2, 3]);
}

#[test]
fn handles_tabs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_character_replacement("\t", 4, "\u{a0}\u{a0}\u{a0}\u{a0}", &[0, 4]);
    assert_character_replacement("x\t", 4, "x\u{a0}\u{a0}\u{a0}", &[0, 1, 4]);
    assert_character_replacement("xx\t", 4, "xx\u{a0}\u{a0}", &[0, 1, 2, 4]);
    assert_character_replacement("xxx\t", 4, "xxx\u{a0}", &[0, 1, 2, 3, 4]);
    assert_character_replacement("xxxx\t", 4, "xxxx\u{a0}\u{a0}\u{a0}\u{a0}", &[0, 1, 2, 3, 4, 8]);
}

fn assert_parts(
    line_content: &str,
    tab_size: i32,
    parts: Vec<TestLineToken>,
    expected: &str,
    info: &[CharacterMappingInfo],
) {
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.line_tokens = create_view_line_tokens(parts);
        o.tab_size = tab_size;
        o.space_width = 0.0;
        o.middot_width = 0.0;
        o.wsmiddot_width = 0.0;
    });

    assert_eq!(html(&actual), format!("<span>{expected}</span>"));
    assert_character_mapping3(&actual.character_mapping, info);
}

#[test]
fn empty_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_parts("", 4, vec![], "<span></span>", &[]);
}

#[test]
fn uses_part_type() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_parts("x", 4, vec![create_part(1, 10)], "<span class=\"mtk10\">x</span>", &[(0, (0, 0)), (1, (0, 1))]);
    assert_parts("x", 4, vec![create_part(1, 20)], "<span class=\"mtk20\">x</span>", &[(0, (0, 0)), (1, (0, 1))]);
    assert_parts("x", 4, vec![create_part(1, 30)], "<span class=\"mtk30\">x</span>", &[(0, (0, 0)), (1, (0, 1))]);
}

#[test]
fn two_parts() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_parts(
        "xy",
        4,
        vec![create_part(1, 1), create_part(2, 2)],
        "<span class=\"mtk1\">x</span><span class=\"mtk2\">y</span>",
        &[(0, (0, 0)), (1, (1, 0)), (2, (1, 1))],
    );
    assert_parts(
        "xyz",
        4,
        vec![create_part(1, 1), create_part(3, 2)],
        "<span class=\"mtk1\">x</span><span class=\"mtk2\">yz</span>",
        &[(0, (0, 0)), (1, (1, 0)), (2, (1, 1)), (3, (1, 2))],
    );
    assert_parts(
        "xyz",
        4,
        vec![create_part(2, 1), create_part(3, 2)],
        "<span class=\"mtk1\">xy</span><span class=\"mtk2\">z</span>",
        &[(0, (0, 0)), (1, (0, 1)), (2, (1, 0)), (3, (1, 1))],
    );
}

// overflow
#[test]
fn overflow() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.line_content = js("Hello world!");
        o.line_tokens = create_view_line_tokens(vec![
            create_part(1, 0),
            create_part(2, 1),
            create_part(3, 2),
            create_part(4, 3),
            create_part(5, 4),
            create_part(6, 5),
            create_part(7, 6),
            create_part(8, 7),
            create_part(9, 8),
            create_part(10, 9),
            create_part(11, 10),
            create_part(12, 11),
        ]);
        o.stop_rendering_line_after = 6;
        o.render_whitespace = "boundary".to_owned();
    });

    assert_html_and_mapping_snapshots("renderViewLine overflow", &inflate_render_line_output(&actual));
}

// typical line
#[test]
fn typical() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "\t    export class Game { // http://test.com     ";
    let line_tokens = create_view_line_tokens(vec![
        create_part(5, 1),
        create_part(11, 2),
        create_part(12, 3),
        create_part(17, 4),
        create_part(18, 5),
        create_part(22, 6),
        create_part(23, 7),
        create_part(24, 8),
        create_part(25, 9),
        create_part(28, 10),
        create_part(43, 11),
        create_part(48, 12),
    ]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.line_tokens = line_tokens;
        o.render_whitespace = "boundary".to_owned();
    });

    assert_html_and_mapping_snapshots("renderViewLine typical", &inflate_render_line_output(&actual));
}

// issue #2255: Weird line rendering part 1
#[test]
fn issue_2255_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "\t\t\tcursorStyle:\t\t\t\t\t\t(prevOpts.cursorStyle !== newOpts.cursorStyle),";
    let line_tokens = create_view_line_tokens(vec![
        create_part(3, 1),  // 3 chars
        create_part(15, 2), // 12 chars
        create_part(21, 3), // 6 chars
        create_part(22, 4), // 1 char
        create_part(43, 5), // 21 chars
        create_part(45, 6), // 2 chars
        create_part(46, 7), // 1 char
        create_part(66, 8), // 20 chars
        create_part(67, 9), // 1 char
        create_part(68, 10), // 2 chars
    ]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.line_tokens = line_tokens;
    });

    assert_html_and_mapping_snapshots("renderViewLine issue-2255-1", &inflate_render_line_output(&actual));
}

// issue #2255: Weird line rendering part 2
#[test]
fn issue_2255_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = " \t\t\tcursorStyle:\t\t\t\t\t\t(prevOpts.cursorStyle !== newOpts.cursorStyle),";

    let line_tokens = create_view_line_tokens(vec![
        create_part(4, 1),  // 4 chars
        create_part(16, 2), // 12 chars
        create_part(22, 3), // 6 chars
        create_part(23, 4), // 1 char
        create_part(44, 5), // 21 chars
        create_part(46, 6), // 2 chars
        create_part(47, 7), // 1 char
        create_part(67, 8), // 20 chars
        create_part(68, 9), // 1 char
        create_part(69, 10), // 2 chars
    ]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.line_tokens = line_tokens;
    });

    assert_html_and_mapping_snapshots("renderViewLine issue-2255-2", &inflate_render_line_output(&actual));
}

// issue #91178: after decoration type shown before cursor
#[test]
fn issue_91178() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "//just a comment";
    let line_tokens = create_view_line_tokens(vec![create_part(16, 1)]);
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.can_use_halfwidth_rightwards_arrow = false;
        o.line_content = js(line_content);
        o.line_tokens = line_tokens;
        o.line_decorations = vec![
            LineDecoration::new(13, 13, "dec1", InlineDecorationType::After),
            LineDecoration::new(13, 13, "dec2", InlineDecorationType::Before),
        ];
    });

    assert_html_and_mapping_snapshots("renderViewLine issue-91178", &inflate_render_line_output(&actual));
}

// issue microsoft/monaco-editor#280: Improved source code rendering for RTL languages
#[test]
fn monaco_280() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "var קודמות = \"מיותר קודמות צ'ט של, אם לשון העברית שינויים ויש, אם\";";
    let line_tokens = create_view_line_tokens(vec![
        create_part(3, 6),
        create_part(13, 1),
        create_part(66, 20),
        create_part(67, 1),
    ]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.is_basic_ascii = false;
        o.contains_rtl = true;
        o.line_tokens = line_tokens;
    });

    assert_html_and_mapping_snapshots("renderViewLine monaco-280", &inflate_render_line_output(&actual));
}

// issue #137036: Issue in RTL languages in recent versions
#[test]
fn issue_137036() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "<option value=\"العربية\">العربية</option>";
    let line_tokens = create_view_line_tokens(vec![
        create_part(1, 2),
        create_part(7, 3),
        create_part(8, 4),
        create_part(13, 5),
        create_part(14, 4),
        create_part(23, 6),
        create_part(24, 2),
        create_part(31, 4),
        create_part(33, 2),
        create_part(39, 3),
        create_part(40, 2),
    ]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.is_basic_ascii = false;
        o.contains_rtl = true;
        o.line_tokens = line_tokens;
    });

    assert_html_and_mapping_snapshots("renderViewLine issue-137036", &inflate_render_line_output(&actual));
}

// issue #99589: Rendering whitespace influences bidi layout
#[test]
fn issue_99589() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "    [\"\u{1F5A8}\u{FE0F} چاپ فاکتور\",\"\u{1F3A8} تنظیمات\"]";
    let line_tokens = create_view_line_tokens(vec![
        create_part(5, 2),
        create_part(21, 3),
        create_part(22, 2),
        create_part(34, 3),
        create_part(35, 2),
    ]);
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js(line_content);
        o.is_basic_ascii = false;
        o.contains_rtl = true;
        o.line_tokens = line_tokens;
        o.render_whitespace = "all".to_owned();
    });

    assert_html_and_mapping_snapshots("renderViewLine issue-99589", &inflate_render_line_output(&actual));
}

// issue #260239: HTML containing bidirectional text is rendered incorrectly
#[test]
fn issue_260239() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Simulando HTML como: <p class="myclass" title="العربي">نشاط التدويل!</p>
    // A linha tem valores de atributo LTR (class="myclass") e RTL (title="العربي").
    let line_content = "<p class=\"myclass\" title=\"العربي\">نشاط التدويل!</p>";
    let line_tokens = create_view_line_tokens(vec![
        create_part(1, 1),  // <
        create_part(2, 2),  // p
        create_part(3, 3),  // (espaço)
        create_part(8, 4),  // class
        create_part(9, 5),  // =
        create_part(10, 6), // "
        create_part(17, 7), // myclass
        create_part(18, 6), // "
        create_part(19, 3), // (espaço)
        create_part(24, 4), // title
        create_part(25, 5), // =
        create_part(26, 6), // "
        create_part(32, 8), // العربي (texto RTL), 6 letras árabes da posição 26 a 31
        create_part(33, 6), // " (aspas de fechamento na posição 32)
        create_part(34, 1), // >
        create_part(47, 9), // نشاط التدويل! (texto RTL), 13 caracteres da posição 34 a 46
        create_part(48, 1), // <
        create_part(49, 2), // /
        create_part(50, 2), // p
        create_part(51, 1), // >
    ]);
    // O construtor posicional do original, na ordem dos argumentos.
    let actual = render_view_line2(&RenderLineInput::new(RenderLineInputOptions {
        use_monospace_optimizations: false,
        can_use_halfwidth_rightwards_arrow: true,
        line_content: js(line_content),
        continues_with_wrapped_line: false,
        is_basic_ascii: false,
        contains_rtl: true,
        faux_indent_length: 0,
        line_tokens,
        line_decorations: Vec::new(),
        tab_size: 4,
        start_visible_column: 0,
        space_width: 10.0,
        middot_width: 10.0,
        wsmiddot_width: 10.0,
        stop_rendering_line_after: -1,
        render_whitespace: "none".to_owned(),
        render_control_characters: false,
        font_ligatures: false,
        selections_on_line: None,
        text_direction: None,
        vertical_scrollbar_size: 14.0,
        render_new_line_when_empty: false,
        use_two_cell_fullwidth_characters: false,
    }));

    assert_html_and_mapping_snapshots("renderViewLine issue-260239", &inflate_render_line_output(&actual));
}

// issue #274604: Mixed LTR and RTL in a single token
#[test]
fn issue_274604() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "test.com##a:-abp-contains(إ)";
    let line_tokens = create_view_line_tokens(vec![create_part(utf16_len(line_content), 1)]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.is_basic_ascii = false;
        o.contains_rtl = true;
        o.line_tokens = line_tokens;
    });

    assert_html_and_mapping_snapshots("renderViewLine issue-274604", &inflate_render_line_output(&actual));
}

// issue #277693: Mixed LTR and RTL in a single token with template literal
#[test]
fn issue_277693() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "نام کاربر: ${user.firstName}";
    let line_tokens = create_view_line_tokens(vec![
        create_part(9, 1),  // نام کاربر (conteúdo RTL da string)
        create_part(11, 1), // : (espaço)
        create_part(13, 2), // ${ (pontuação da expressão do template)
        create_part(17, 3), // user (variável)
        create_part(18, 4), // . (pontuação)
        create_part(27, 3), // firstName (propriedade)
        create_part(28, 2), // } (pontuação da expressão do template)
    ]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.is_basic_ascii = false;
        o.contains_rtl = true;
        o.line_tokens = line_tokens;
    });

    assert_html_and_mapping_snapshots("renderViewLine issue-277693", &inflate_render_line_output(&actual));
}

/// `assertSplitsTokens` dos casos `issue-6885` e `issue-21476`, que só diferem em `fontLigatures`.
fn assert_splits_tokens(font_ligatures: bool, message: &str, line_content: &str, expected_output: &[&str]) {
    let line_tokens = create_view_line_tokens(vec![create_part(utf16_len(line_content), 1)]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.line_tokens = line_tokens;
        o.font_ligatures = font_ligatures;
    });
    assert_eq!(html(&actual), format!("<span>{}</span>", expected_output.concat()), "{message}");
}

// issue #6885: Splits large tokens
#[test]
fn issue_6885() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    //                                                                                                                  1         1         1
    //                        1         2         3         4         5         6         7         8         9         0         1         2
    //               1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234
    let line_text = "This is just a long line that contains very interesting text. This is just a long line that contains very interesting text.";

    // A token with 49 chars
    assert_splits_tokens(
        false,
        "49 chars",
        &line_text[..49],
        &["<span class=\"mtk1\">This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}contains\u{a0}very\u{a0}inter</span>"],
    );

    // A token with 50 chars
    assert_splits_tokens(
        false,
        "50 chars",
        &line_text[..50],
        &["<span class=\"mtk1\">This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}contains\u{a0}very\u{a0}intere</span>"],
    );

    // A token with 51 chars
    assert_splits_tokens(
        false,
        "51 chars",
        &line_text[..51],
        &[
            "<span class=\"mtk1\">This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}contains\u{a0}very\u{a0}intere</span>",
            "<span class=\"mtk1\">s</span>",
        ],
    );

    // A token with 99 chars
    assert_splits_tokens(
        false,
        "99 chars",
        &line_text[..99],
        &[
            "<span class=\"mtk1\">This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}contains\u{a0}very\u{a0}intere</span>",
            "<span class=\"mtk1\">sting\u{a0}text.\u{a0}This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}contain</span>",
        ],
    );

    // A token with 100 chars
    assert_splits_tokens(
        false,
        "100 chars",
        &line_text[..100],
        &[
            "<span class=\"mtk1\">This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}contains\u{a0}very\u{a0}intere</span>",
            "<span class=\"mtk1\">sting\u{a0}text.\u{a0}This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}contains</span>",
        ],
    );

    // A token with 101 chars
    assert_splits_tokens(
        false,
        "101 chars",
        &line_text[..101],
        &[
            "<span class=\"mtk1\">This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}contains\u{a0}very\u{a0}intere</span>",
            "<span class=\"mtk1\">sting\u{a0}text.\u{a0}This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}contains</span>",
            "<span class=\"mtk1\">\u{a0}</span>",
        ],
    );
}

// issue #21476: Does not split large tokens when ligatures are on
#[test]
fn issue_21476() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    //                                                                                                                  1         1         1
    //                        1         2         3         4         5         6         7         8         9         0         1         2
    //               1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234
    let line_text = "This is just a long line that contains very interesting text. This is just a long line that contains very interesting text.";

    // A token with 101 chars
    assert_splits_tokens(
        true,
        "101 chars",
        &line_text[..101],
        &[
            "<span class=\"mtk1\">This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}contains\u{a0}very\u{a0}</span>",
            "<span class=\"mtk1\">interesting\u{a0}text.\u{a0}This\u{a0}is\u{a0}just\u{a0}a\u{a0}long\u{a0}line\u{a0}that\u{a0}</span>",
            "<span class=\"mtk1\">contains\u{a0}</span>",
        ],
    );
}

// issue #20624: Unaligned surrogate pairs are corrupted at multiples of 50 columns
#[test]
fn issue_20624() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "a𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷𠮷";
    let line_tokens = create_view_line_tokens(vec![create_part(utf16_len(line_content), 1)]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.is_basic_ascii = false;
        o.line_tokens = line_tokens;
    });

    assert_snapshot("renderViewLine issue-20624", 0, "html", &inflate_render_line_output(&actual).html.join(""));
}

// issue #6885: Does not split large tokens in RTL text
#[test]
fn issue_6885_rtl() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "את גרמנית בהתייחסות שמו, שנתי המשפט אל חפש, אם כתב אחרים ולחבר. של התוכן אודות בויקיפדיה כלל, של עזרה כימיה היא. על עמוד יוצרים מיתולוגיה סדר, אם שכל שתפו לעברית שינויים, אם שאלות אנגלית עזה. שמות בקלות מה סדר.";
    let line_tokens = create_view_line_tokens(vec![create_part(utf16_len(line_content), 1)]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.is_basic_ascii = false;
        o.contains_rtl = true;
        o.line_tokens = line_tokens;
    });

    assert_snapshot("renderViewLine issue-6885-rtl", 0, "html", &html(&actual));
}

// issue #95685: Uses unicode replacement character for Paragraph Separator
#[test]
fn issue_95685() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "var ftext = [\u{2029}\"Und\", \"dann\", \"eines\"];";
    let line_tokens = create_view_line_tokens(vec![create_part(utf16_len(line_content), 1)]);
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.is_basic_ascii = false;
        o.line_tokens = line_tokens;
    });

    assert_html_and_mapping_snapshots("renderViewLine issue-95685", &inflate_render_line_output(&actual));
}

// issue #19673: Monokai Theme bad-highlighting in line wrap
#[test]
fn issue_19673() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "    MongoCallback<string>): void {";
    let line_tokens = create_view_line_tokens(vec![
        create_part(17, 1),
        create_part(18, 2),
        create_part(24, 3),
        create_part(26, 4),
        create_part(27, 5),
        create_part(28, 6),
        create_part(32, 7),
        create_part(34, 8),
    ]);
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js(line_content);
        o.faux_indent_length = 4;
        o.line_tokens = line_tokens;
    });

    assert_html_and_mapping_snapshots("renderViewLine issue-19673", &inflate_render_line_output(&actual));
}

// #endregion

// #region suite('renderViewLine2')

fn test_create_line_parts(
    font_is_monospace: bool,
    line_content: &str,
    tokens: Vec<TestLineToken>,
    faux_indent_length: i32,
    render_whitespace: &str,
    selections: Option<Vec<OffsetRange>>,
) -> Inflated {
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = font_is_monospace;
        o.line_content = js(line_content);
        o.faux_indent_length = faux_indent_length;
        o.line_tokens = create_view_line_tokens(tokens);
        o.render_whitespace = render_whitespace.to_owned();
        o.selections_on_line = selections;
    });
    inflate_render_line_output(&actual)
}

// issue #18616: Inline decorations ending at the text length are no longer rendered
#[test]
fn issue_18616() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "https://microsoft.com";
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.line_tokens = create_view_line_tokens(vec![create_part(21, 3)]);
        o.line_decorations = vec![LineDecoration::new(1, 22, "link", InlineDecorationType::Regular)];
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-18616", &inflate_render_line_output(&actual));
}

// issue #19207: Link in Monokai is not rendered correctly
#[test]
fn issue_19207() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "'let url = `http://***/_api/web/lists/GetByTitle(\\'Teambuildingaanvragen\\')/items`;'";
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js(line_content);
        o.line_tokens = create_view_line_tokens(vec![
            create_part(49, 6),
            create_part(51, 4),
            create_part(72, 6),
            create_part(74, 4),
            create_part(84, 6),
        ]);
        o.line_decorations = vec![LineDecoration::new(13, 51, "detected-link", InlineDecorationType::Regular)];
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-19207", &inflate_render_line_output(&actual));
}

// createLineParts simple
#[test]
fn simple() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(false, "Hello world!", vec![create_part(12, 1)], 0, "none", None);
    assert_html_and_mapping_snapshots("renderViewLine2 simple", &actual);
}

// createLineParts simple two tokens
#[test]
fn two_tokens() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual =
        test_create_line_parts(false, "Hello world!", vec![create_part(6, 1), create_part(12, 2)], 0, "none", None);
    assert_html_and_mapping_snapshots("renderViewLine2 two-tokens", &actual);
}

// createLineParts render whitespace - 4 leading spaces
#[test]
fn ws_4_leading() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        "    Hello world!    ",
        vec![create_part(4, 1), create_part(6, 2), create_part(20, 3)],
        0,
        "boundary",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-4-leading", &actual);
}

// createLineParts render whitespace - 8 leading spaces
#[test]
fn ws_8_leading() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        "        Hello world!        ",
        vec![create_part(8, 1), create_part(10, 2), create_part(28, 3)],
        0,
        "boundary",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-8-leading", &actual);
}

// createLineParts render whitespace - 2 leading tabs
#[test]
fn ws_2_tabs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        "\t\tHello world!\t",
        vec![create_part(2, 1), create_part(4, 2), create_part(15, 3)],
        0,
        "boundary",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-2-tabs", &actual);
}

// createLineParts render whitespace - mixed leading spaces and tabs
#[test]
fn ws_mixed() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        "  \t\t  Hello world! \t  \t   \t    ",
        vec![create_part(6, 1), create_part(8, 2), create_part(31, 3)],
        0,
        "boundary",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-mixed", &actual);
}

// createLineParts render whitespace skips faux indent
#[test]
fn ws_faux_indent() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        "\t\t  Hello world! \t  \t   \t    ",
        vec![create_part(4, 1), create_part(6, 2), create_part(29, 3)],
        2,
        "boundary",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-faux-indent", &actual);
}

// createLineParts does not emit width for monospace fonts
#[test]
fn ws_monospace() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        true,
        "\t\t  Hello world! \t  \t   \t    ",
        vec![create_part(4, 1), create_part(6, 2), create_part(29, 3)],
        2,
        "boundary",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-monospace", &actual);
}

// createLineParts render whitespace in middle but not for one space
#[test]
fn ws_middle() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        "it  it it  it",
        vec![create_part(6, 1), create_part(7, 2), create_part(13, 3)],
        0,
        "boundary",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-middle", &actual);
}

// createLineParts render whitespace for all in middle
#[test]
fn ws_all_middle() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        " Hello world!\t",
        vec![create_part(4, 0), create_part(6, 1), create_part(14, 2)],
        0,
        "all",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-all-middle", &actual);
}

// createLineParts render whitespace for selection with no selections
#[test]
fn ws_sel_none() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        " Hello world!\t",
        vec![create_part(4, 0), create_part(6, 1), create_part(14, 2)],
        0,
        "selection",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-sel-none", &actual);
}

// createLineParts render whitespace for selection with whole line selection
#[test]
fn ws_sel_whole() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        " Hello world!\t",
        vec![create_part(4, 0), create_part(6, 1), create_part(14, 2)],
        0,
        "selection",
        Some(vec![OffsetRange::new(0, 14)]),
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-sel-whole", &actual);
}

// createLineParts render whitespace for selection with selection spanning part of whitespace
#[test]
fn ws_sel_partial() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        " Hello world!\t",
        vec![create_part(4, 0), create_part(6, 1), create_part(14, 2)],
        0,
        "selection",
        Some(vec![OffsetRange::new(0, 5)]),
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-sel-partial", &actual);
}

// createLineParts render whitespace for selection with multiple selections
#[test]
fn ws_sel_multiple() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        " Hello world!\t",
        vec![create_part(4, 0), create_part(6, 1), create_part(14, 2)],
        0,
        "selection",
        Some(vec![OffsetRange::new(0, 5), OffsetRange::new(9, 14)]),
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-sel-multiple", &actual);
}

// createLineParts render whitespace for selection with multiple, initially unsorted selections
#[test]
fn ws_sel_unsorted() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        " Hello world!\t",
        vec![create_part(4, 0), create_part(6, 1), create_part(14, 2)],
        0,
        "selection",
        Some(vec![OffsetRange::new(9, 14), OffsetRange::new(0, 5)]),
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-sel-unsorted", &actual);
}

// createLineParts render whitespace for selection with selections next to each other
#[test]
fn ws_sel_adjacent() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        " * S",
        vec![create_part(4, 0)],
        0,
        "selection",
        Some(vec![OffsetRange::new(0, 1), OffsetRange::new(1, 2), OffsetRange::new(2, 3)]),
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-sel-adjacent", &actual);
}

// createLineParts render whitespace for trailing with leading, inner, and without trailing whitespace
#[test]
fn ws_trail_no_trail() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        " Hello world!",
        vec![create_part(4, 0), create_part(6, 1), create_part(14, 2)],
        0,
        "trailing",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-trail-no-trail", &actual);
}

// createLineParts render whitespace for trailing with leading, inner, and trailing whitespace
#[test]
fn ws_trail_with_trail() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        " Hello world! \t",
        vec![create_part(4, 0), create_part(6, 1), create_part(15, 2)],
        0,
        "trailing",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-trail-with-trail", &actual);
}

// createLineParts render whitespace for trailing with 8 leading and 8 trailing whitespaces
#[test]
fn ws_trail_8_8() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = test_create_line_parts(
        false,
        "        Hello world!        ",
        vec![create_part(8, 1), create_part(10, 2), create_part(28, 3)],
        0,
        "trailing",
        None,
    );
    assert_html_and_mapping_snapshots("renderViewLine2 ws-trail-8-8", &actual);
}

// createLineParts render whitespace for trailing with line containing only whitespaces
#[test]
fn ws_trail_only() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual =
        test_create_line_parts(false, " \t ", vec![create_part(2, 0), create_part(3, 1)], 0, "trailing", None);
    assert_html_and_mapping_snapshots("renderViewLine2 ws-trail-only", &actual);
}

// createLineParts can handle unsorted inline decorations
#[test]
fn unsorted_deco() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.line_content = js("Hello world");
        o.line_tokens = create_view_line_tokens(vec![create_part(11, 0)]);
        o.line_decorations = vec![
            LineDecoration::new(5, 7, "a", InlineDecorationType::Regular),
            LineDecoration::new(1, 3, "b", InlineDecorationType::Regular),
            LineDecoration::new(2, 8, "c", InlineDecorationType::Regular),
        ];
    });

    // 01234567890
    // Hello world
    // ----aa-----
    // bb---------
    // -cccccc----

    assert_html_and_mapping_snapshots("renderViewLine2 unsorted-deco", &inflate_render_line_output(&actual));
}

// issue #11485: Visible whitespace conflicts with before decorator attachment
#[test]
fn issue_11485() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "\tbla";

    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.line_tokens = create_view_line_tokens(vec![create_part(4, 3)]);
        o.line_decorations = vec![LineDecoration::new(1, 2, "before", InlineDecorationType::Before)];
        o.render_whitespace = "all".to_owned();
        o.font_ligatures = true;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-11485", &inflate_render_line_output(&actual));
}

// issue #32436: Non-monospace font + visible whitespace + After decorator causes line to "jump"
#[test]
fn issue_32436() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "\tbla";

    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.line_tokens = create_view_line_tokens(vec![create_part(4, 3)]);
        o.line_decorations = vec![LineDecoration::new(2, 3, "before", InlineDecorationType::Before)];
        o.render_whitespace = "all".to_owned();
        o.font_ligatures = true;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-32436", &inflate_render_line_output(&actual));
}

// issue #30133: Empty lines don't render inline decorations
#[test]
fn issue_30133() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let line_content = "";

    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.line_tokens = create_view_line_tokens(vec![create_part(0, 3)]);
        o.line_decorations = vec![LineDecoration::new(1, 2, "before", InlineDecorationType::Before)];
        o.render_whitespace = "all".to_owned();
        o.font_ligatures = true;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-30133", &inflate_render_line_output(&actual));
}

// issue #37208: Collapsing bullet point containing emoji in Markdown document results in [??] character
#[test]
fn issue_37208() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js("  1. \u{1F64F}");
        o.is_basic_ascii = false;
        o.line_tokens = create_view_line_tokens(vec![create_part(7, 3)]);
        o.line_decorations = vec![LineDecoration::new(7, 8, "inline-folded", InlineDecorationType::After)];
        o.tab_size = 2;
        o.stop_rendering_line_after = 10000;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-37208", &inflate_render_line_output(&actual));
}

// issue #37401 #40127: Allow both before and after decorations on empty line
#[test]
fn issue_37401() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js("");
        o.line_tokens = create_view_line_tokens(vec![create_part(0, 3)]);
        o.line_decorations = vec![
            LineDecoration::new(1, 1, "before", InlineDecorationType::Before),
            LineDecoration::new(1, 1, "after", InlineDecorationType::After),
        ];
        o.tab_size = 2;
        o.stop_rendering_line_after = 10000;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-37401", &inflate_render_line_output(&actual));
}

// issue #118759: enable multiple text editor decorations in empty lines
#[test]
fn issue_118759() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js("");
        o.line_tokens = create_view_line_tokens(vec![create_part(0, 3)]);
        o.line_decorations = vec![
            LineDecoration::new(1, 1, "after1", InlineDecorationType::After),
            LineDecoration::new(1, 1, "after2", InlineDecorationType::After),
            LineDecoration::new(1, 1, "before1", InlineDecorationType::Before),
            LineDecoration::new(1, 1, "before2", InlineDecorationType::Before),
        ];
        o.tab_size = 2;
        o.stop_rendering_line_after = 10000;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-118759", &inflate_render_line_output(&actual));
}

// issue #38935: GitLens end-of-line blame no longer rendering
#[test]
fn issue_38935() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js("\t}");
        o.line_tokens = create_view_line_tokens(vec![create_part(2, 3)]);
        o.line_decorations = vec![
            LineDecoration::new(
                3,
                3,
                "ced-TextEditorDecorationType2-5e9b9b3f-3 ced-TextEditorDecorationType2-3",
                InlineDecorationType::Before,
            ),
            LineDecoration::new(
                3,
                3,
                "ced-TextEditorDecorationType2-5e9b9b3f-4 ced-TextEditorDecorationType2-4",
                InlineDecorationType::After,
            ),
        ];
        o.stop_rendering_line_after = 10000;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-38935", &inflate_render_line_output(&actual));
}

// issue #136622: Inline decorations are not rendering on non-ASCII lines when renderControlCharacters is on
#[test]
fn issue_136622() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js("some text \u{a3}");
        o.is_basic_ascii = false;
        o.line_tokens = create_view_line_tokens(vec![create_part(11, 3)]);
        o.line_decorations = vec![
            LineDecoration::new(5, 5, "inlineDec1", InlineDecorationType::After),
            LineDecoration::new(6, 6, "inlineDec2", InlineDecorationType::Before),
        ];
        o.stop_rendering_line_after = 10000;
        o.render_control_characters = true;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-136622", &inflate_render_line_output(&actual));
}

// issue #22832: Consider fullwidth characters when rendering tabs
#[test]
fn issue_22832_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js("asd = \"擦\"\t\t#asd");
        o.is_basic_ascii = false;
        o.line_tokens = create_view_line_tokens(vec![create_part(15, 3)]);
        o.stop_rendering_line_after = 10000;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-22832-1", &inflate_render_line_output(&actual));
}

// issue #22832: Consider fullwidth characters when rendering tabs (render whitespace)
#[test]
fn issue_22832_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js("asd = \"擦\"\t\t#asd");
        o.is_basic_ascii = false;
        o.line_tokens = create_view_line_tokens(vec![create_part(15, 3)]);
        o.stop_rendering_line_after = 10000;
        o.render_whitespace = "all".to_owned();
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-22832-2", &inflate_render_line_output(&actual));
}

// issue #22352: COMBINING ACUTE ACCENT (U+0301)
#[test]
fn issue_22352_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        // O "á" decomposto: a + U+0301.
        o.line_content = js("12345689012345678901234568901234567890123456890aba\u{301}ba");
        o.is_basic_ascii = false;
        o.line_tokens = create_view_line_tokens(vec![create_part(53, 3)]);
        o.stop_rendering_line_after = 10000;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-22352-1", &inflate_render_line_output(&actual));
}

// issue #22352: Partially Broken Complex Script Rendering of Tamil
#[test]
fn issue_22352_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js(" JoyShareல் பின்தொடர்ந்து, விடீயோ, ஜோக்குகள், அனிமேசன், நகைச்சுவை படங்கள் மற்றும் செய்திகளை பெறுவீர்");
        o.is_basic_ascii = false;
        o.line_tokens = create_view_line_tokens(vec![create_part(100, 3)]);
        o.stop_rendering_line_after = 10000;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-22352-2", &inflate_render_line_output(&actual));
}

// issue #42700: Hindi characters are not being rendered properly
#[test]
fn issue_42700() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js(" वो ऐसा क्या है जो हमारे अंदर भी है और बाहर भी है। जिसकी वजह से हम सब हैं। जिसने इस सृष्टि की रचना की है।");
        o.is_basic_ascii = false;
        o.line_tokens = create_view_line_tokens(vec![create_part(105, 3)]);
        o.stop_rendering_line_after = 10000;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-42700", &inflate_render_line_output(&actual));
}

// issue #38123: editor.renderWhitespace: "boundary" renders whitespace at line wrap point when line is wrapped
#[test]
fn issue_38123() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.line_content = js("This is a long line which never uses more than two spaces. ");
        o.continues_with_wrapped_line = true;
        o.line_tokens = create_view_line_tokens(vec![create_part(59, 3)]);
        o.stop_rendering_line_after = 10000;
        o.render_whitespace = "boundary".to_owned();
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-38123", &inflate_render_line_output(&actual));
}

// issue #33525: Long line with ligatures takes a long time to paint decorations
#[test]
fn issue_33525_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.can_use_halfwidth_rightwards_arrow = false;
        o.line_content = js("append data to append data to append data to append data to append data to append data to append data to append data to append data to append data to append data to append data to append data to");
        o.line_tokens = create_view_line_tokens(vec![create_part(194, 3)]);
        o.stop_rendering_line_after = 10000;
        o.font_ligatures = true;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-33525-1", &inflate_render_line_output(&actual));
}

// issue #33525: Long line with ligatures takes a long time to paint decorations - not possible
#[test]
fn issue_33525_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.can_use_halfwidth_rightwards_arrow = false;
        o.line_content = js("appenddatatoappenddatatoappenddatatoappenddatatoappenddatatoappenddatatoappenddatatoappenddatatoappenddatatoappenddatatoappenddatatoappenddatatoappenddatato");
        o.line_tokens = create_view_line_tokens(vec![create_part(194, 3)]);
        o.stop_rendering_line_after = 10000;
        o.font_ligatures = true;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-33525-2", &inflate_render_line_output(&actual));
}

// issue #91936: Semantic token color highlighting fails on line with selected text
#[test]
fn issue_91936() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.line_content = js("                    else if ($s = 08) then '\\b'");
        o.line_tokens = create_view_line_tokens(vec![
            create_part(20, 1),
            create_part(24, 15),
            create_part(25, 1),
            create_part(27, 15),
            create_part(28, 1),
            create_part(29, 1),
            create_part(29, 1),
            create_part(31, 16),
            create_part(32, 1),
            create_part(33, 1),
            create_part(34, 1),
            create_part(36, 6),
            create_part(36, 1),
            create_part(37, 1),
            create_part(38, 1),
            create_part(42, 15),
            create_part(43, 1),
            create_part(47, 11),
        ]);
        o.stop_rendering_line_after = 10000;
        o.render_whitespace = "selection".to_owned();
        o.selections_on_line = Some(vec![OffsetRange::new(0, 47)]);
        o.middot_width = 11.0;
        o.wsmiddot_width = 11.0;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-91936", &inflate_render_line_output(&actual));
}

// issue #119416: Delete Control Character (U+007F / &#127;) displayed as space
#[test]
fn issue_119416() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.can_use_halfwidth_rightwards_arrow = false;
        o.line_content = js("[\u{7f}] [\u{0}]");
        o.line_tokens = create_view_line_tokens(vec![create_part(7, 3)]);
        o.stop_rendering_line_after = 10000;
        o.render_control_characters = true;
        o.font_ligatures = true;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-119416", &inflate_render_line_output(&actual));
}

// issue #116939: Important control characters aren't rendered
#[test]
fn issue_116939() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.can_use_halfwidth_rightwards_arrow = false;
        o.line_content = js("transferBalance(5678,\u{202E}6776,4321\u{202C},\"USD\");");
        o.is_basic_ascii = false;
        o.line_tokens = create_view_line_tokens(vec![create_part(42, 3)]);
        o.stop_rendering_line_after = 10000;
        o.render_control_characters = true;
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-116939", &inflate_render_line_output(&actual));
}

// issue #124038: Multiple end-of-line text decorations get merged
#[test]
fn issue_124038() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.use_monospace_optimizations = true;
        o.can_use_halfwidth_rightwards_arrow = false;
        o.line_content = js("    if");
        o.line_tokens = create_view_line_tokens(vec![create_part(4, 1), create_part(6, 2)]);
        o.line_decorations = vec![
            LineDecoration::new(
                7,
                7,
                "ced-1-TextEditorDecorationType2-17c14d98-3 ced-1-TextEditorDecorationType2-3",
                InlineDecorationType::Before,
            ),
            LineDecoration::new(
                7,
                7,
                "ced-1-TextEditorDecorationType2-17c14d98-4 ced-1-TextEditorDecorationType2-4",
                InlineDecorationType::After,
            ),
            LineDecoration::new(7, 7, "ced-ghost-text-1-4", InlineDecorationType::After),
        ];
        o.stop_rendering_line_after = 10000;
        o.render_whitespace = "all".to_owned();
    });

    assert_html_and_mapping_snapshots("renderViewLine2 issue-124038", &inflate_render_line_output(&actual));
}

fn create_test_get_column_of_line_part_offset(
    line_content: &str,
    tab_size: i32,
    parts: Vec<TestLineToken>,
    _expected_part_lengths: &[u32],
) -> impl Fn(u32, u32, u32, i32) {
    let render_line_output = render_view_line(|o| {
        o.line_content = js(line_content);
        o.tab_size = tab_size;
        o.line_tokens = create_view_line_tokens(parts);
    });

    move |part_index: u32, part_length: u32, offset: u32, expected: i32| {
        let actual_column = render_line_output
            .character_mapping
            .get_column(&DomPosition::new(part_index, offset), part_length);
        assert_eq!(actual_column, expected, "getColumn for {part_index}, {offset}");
    }
}

#[test]
fn get_column_of_line_part_offset_1_simple_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_get_column_of_line_part_offset =
        create_test_get_column_of_line_part_offset("hello world", 4, vec![create_part(11, 1)], &[11]);
    test_get_column_of_line_part_offset(0, 11, 0, 1);
    test_get_column_of_line_part_offset(0, 11, 1, 2);
    test_get_column_of_line_part_offset(0, 11, 2, 3);
    test_get_column_of_line_part_offset(0, 11, 3, 4);
    test_get_column_of_line_part_offset(0, 11, 4, 5);
    test_get_column_of_line_part_offset(0, 11, 5, 6);
    test_get_column_of_line_part_offset(0, 11, 6, 7);
    test_get_column_of_line_part_offset(0, 11, 7, 8);
    test_get_column_of_line_part_offset(0, 11, 8, 9);
    test_get_column_of_line_part_offset(0, 11, 9, 10);
    test_get_column_of_line_part_offset(0, 11, 10, 11);
    test_get_column_of_line_part_offset(0, 11, 11, 12);
}

#[test]
fn get_column_of_line_part_offset_2_regular_js() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_get_column_of_line_part_offset = create_test_get_column_of_line_part_offset(
        "var x = 3;",
        4,
        vec![
            create_part(3, 1),
            create_part(4, 2),
            create_part(5, 3),
            create_part(8, 4),
            create_part(9, 5),
            create_part(10, 6),
        ],
        &[3, 1, 1, 3, 1, 1],
    );
    test_get_column_of_line_part_offset(0, 3, 0, 1);
    test_get_column_of_line_part_offset(0, 3, 1, 2);
    test_get_column_of_line_part_offset(0, 3, 2, 3);
    test_get_column_of_line_part_offset(0, 3, 3, 4);
    test_get_column_of_line_part_offset(1, 1, 0, 4);
    test_get_column_of_line_part_offset(1, 1, 1, 5);
    test_get_column_of_line_part_offset(2, 1, 0, 5);
    test_get_column_of_line_part_offset(2, 1, 1, 6);
    test_get_column_of_line_part_offset(3, 3, 0, 6);
    test_get_column_of_line_part_offset(3, 3, 1, 7);
    test_get_column_of_line_part_offset(3, 3, 2, 8);
    test_get_column_of_line_part_offset(3, 3, 3, 9);
    test_get_column_of_line_part_offset(4, 1, 0, 9);
    test_get_column_of_line_part_offset(4, 1, 1, 10);
    test_get_column_of_line_part_offset(5, 1, 0, 10);
    test_get_column_of_line_part_offset(5, 1, 1, 11);
}

#[test]
fn get_column_of_line_part_offset_3_tab_with_tab_size_6() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_get_column_of_line_part_offset =
        create_test_get_column_of_line_part_offset("\t", 6, vec![create_part(1, 1)], &[6]);
    test_get_column_of_line_part_offset(0, 6, 0, 1);
    test_get_column_of_line_part_offset(0, 6, 1, 1);
    test_get_column_of_line_part_offset(0, 6, 2, 1);
    test_get_column_of_line_part_offset(0, 6, 3, 1);
    test_get_column_of_line_part_offset(0, 6, 4, 2);
    test_get_column_of_line_part_offset(0, 6, 5, 2);
    test_get_column_of_line_part_offset(0, 6, 6, 2);
}

#[test]
fn get_column_of_line_part_offset_4_once_indented_line_tab_size_4() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_get_column_of_line_part_offset = create_test_get_column_of_line_part_offset(
        "\tfunction",
        4,
        vec![create_part(1, 1), create_part(9, 2)],
        &[4, 8],
    );
    test_get_column_of_line_part_offset(0, 4, 0, 1);
    test_get_column_of_line_part_offset(0, 4, 1, 1);
    test_get_column_of_line_part_offset(0, 4, 2, 1);
    test_get_column_of_line_part_offset(0, 4, 3, 2);
    test_get_column_of_line_part_offset(0, 4, 4, 2);
    test_get_column_of_line_part_offset(1, 8, 0, 2);
    test_get_column_of_line_part_offset(1, 8, 1, 3);
    test_get_column_of_line_part_offset(1, 8, 2, 4);
    test_get_column_of_line_part_offset(1, 8, 3, 5);
    test_get_column_of_line_part_offset(1, 8, 4, 6);
    test_get_column_of_line_part_offset(1, 8, 5, 7);
    test_get_column_of_line_part_offset(1, 8, 6, 8);
    test_get_column_of_line_part_offset(1, 8, 7, 9);
    test_get_column_of_line_part_offset(1, 8, 8, 10);
}

#[test]
fn get_column_of_line_part_offset_5_twice_indented_line_tab_size_4() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_get_column_of_line_part_offset = create_test_get_column_of_line_part_offset(
        "\t\tfunction",
        4,
        vec![create_part(2, 1), create_part(10, 2)],
        &[8, 8],
    );
    test_get_column_of_line_part_offset(0, 8, 0, 1);
    test_get_column_of_line_part_offset(0, 8, 1, 1);
    test_get_column_of_line_part_offset(0, 8, 2, 1);
    test_get_column_of_line_part_offset(0, 8, 3, 2);
    test_get_column_of_line_part_offset(0, 8, 4, 2);
    test_get_column_of_line_part_offset(0, 8, 5, 2);
    test_get_column_of_line_part_offset(0, 8, 6, 2);
    test_get_column_of_line_part_offset(0, 8, 7, 3);
    test_get_column_of_line_part_offset(0, 8, 8, 3);
    test_get_column_of_line_part_offset(1, 8, 0, 3);
    test_get_column_of_line_part_offset(1, 8, 1, 4);
    test_get_column_of_line_part_offset(1, 8, 2, 5);
    test_get_column_of_line_part_offset(1, 8, 3, 6);
    test_get_column_of_line_part_offset(1, 8, 4, 7);
    test_get_column_of_line_part_offset(1, 8, 5, 8);
    test_get_column_of_line_part_offset(1, 8, 6, 9);
    test_get_column_of_line_part_offset(1, 8, 7, 10);
    test_get_column_of_line_part_offset(1, 8, 8, 11);
}

// #endregion

// #region suite('renderViewLine - useTwoCellFullwidthCharacters')

/// O atributo style emitido para um caractere centralizado em duas células. As declarações que ele divide com
/// todos os outros desse tipo ficam na classe `mtkfullwidth`.
fn cell(width: u32) -> String {
    format!(" style=\"width:{width}px\"")
}

fn render_with(line_content: &str, configure: impl FnOnce(&mut RenderLineInputOptions)) -> Vec<String> {
    let actual = render_view_line(|o| {
        o.line_content = js(line_content);
        o.is_basic_ascii = is_basic_ascii(line_content);
        o.contains_rtl = contains_rtl(line_content);
        o.line_tokens = create_view_line_tokens(vec![create_part(utf16_len(line_content), 1)]);
        o.use_two_cell_fullwidth_characters = true;
        configure(o);
    });
    inflate_render_line_output(&actual).html
}

fn render(line_content: &str) -> Vec<String> {
    render_with(line_content, |_| {})
}

#[test]
fn a_lone_full_width_character_is_centered_in_two_cells() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(render("漢"), vec![format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(20))]);
}

#[test]
fn every_full_width_character_of_a_run_gets_its_own_cell() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render("漢字"),
        vec![
            format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(20)),
            format!("<span{} class=\"mtk1 mtkfullwidth\">字</span>", cell(20)),
        ]
    );
}

#[test]
fn narrow_characters_around_a_full_width_character_stay_in_shared_parts() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render("ab漢cd"),
        vec![
            "<span class=\"mtk1\">ab</span>".to_owned(),
            format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(20)),
            "<span class=\"mtk1\">cd</span>".to_owned(),
        ]
    );
}

#[test]
fn a_full_width_character_at_either_end_of_the_line_is_centered() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render("漢a漢"),
        vec![
            format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(20)),
            "<span class=\"mtk1\">a</span>".to_owned(),
            format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(20)),
        ]
    );
}

#[test]
fn full_width_forms_of_ascii_characters_are_centered() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render("Ａ１！"),
        vec![
            format!("<span{} class=\"mtk1 mtkfullwidth\">Ａ</span>", cell(20)),
            format!("<span{} class=\"mtk1 mtkfullwidth\">１</span>", cell(20)),
            format!("<span{} class=\"mtk1 mtkfullwidth\">！</span>", cell(20)),
        ]
    );
}

#[test]
fn cjk_punctuation_is_centered() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render("あ、い。"),
        vec![
            format!("<span{} class=\"mtk1 mtkfullwidth\">あ</span>", cell(20)),
            format!("<span{} class=\"mtk1 mtkfullwidth\">、</span>", cell(20)),
            format!("<span{} class=\"mtk1 mtkfullwidth\">い</span>", cell(20)),
            format!("<span{} class=\"mtk1 mtkfullwidth\">。</span>", cell(20)),
        ]
    );
}

#[test]
fn hangul_jamo_outside_the_shared_full_width_classification_is_left_alone() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let hangul_jamo = "\u{1100}";
    assert_eq!(render(hangul_jamo), vec![format!("<span class=\"mtk1\">{hangul_jamo}</span>")]);
}

#[test]
fn half_width_katakana_is_left_alone() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(render("ｱｲｳ"), vec!["<span class=\"mtk1\">ｱｲｳ</span>".to_owned()]);
}

#[test]
fn non_ascii_narrow_characters_are_left_alone() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(render("Ünïcödé"), vec!["<span class=\"mtk1\">Ünïcödé</span>".to_owned()]);
}

#[test]
fn characters_included_by_the_shared_full_width_classification_are_centered() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let vai_syllable = "\u{A500}";
    assert_eq!(
        render(vai_syllable),
        vec![format!("<span{} class=\"mtk1 mtkfullwidth\">{vai_syllable}</span>", cell(20))]
    );
}

#[test]
fn the_cell_width_follows_the_space_width() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_with("漢", |o| o.space_width = 7.5),
        vec![format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(15))]
    );
}

#[test]
fn nothing_changes_when_the_setting_is_off() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_with("ab漢cd", |o| o.use_two_cell_fullwidth_characters = false),
        vec!["<span class=\"mtk1\">ab漢cd</span>".to_owned()]
    );
}

#[test]
fn basic_ascii_lines_are_untouched() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(render("abcd"), vec!["<span class=\"mtk1\">abcd</span>".to_owned()]);
}

#[test]
fn rtl_parts_preserve_bidi_isolation_without_centering() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render("漢عربى"),
        vec![
            "<span style=\"unicode-bidi:isolate\" class=\"mtk1\">漢</span>".to_owned(),
            "<span style=\"unicode-bidi:isolate\" class=\"mtk1\">عربى</span>".to_owned(),
        ]
    );
}

#[test]
fn a_full_width_character_carrying_a_combining_mark_is_centered_by_code_point() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // あ + U+0301 (acento agudo combinante) + い
    assert_eq!(
        render("あ\u{301}い"),
        vec![
            format!("<span{} class=\"mtk1 mtkfullwidth\">あ</span>", cell(20)),
            "<span class=\"mtk1\">\u{301}</span>".to_owned(),
            format!("<span{} class=\"mtk1 mtkfullwidth\">い</span>", cell(20)),
        ]
    );
}

#[test]
fn a_full_width_character_followed_by_a_variation_selector_is_centered_by_code_point() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // 神 + U+FE00 (seletor de variação 1) + 社
    assert_eq!(
        render("神\u{FE00}社"),
        vec![
            format!("<span{} class=\"mtk1 mtkfullwidth\">神</span>", cell(20)),
            "<span class=\"mtk1\">\u{FE00}</span>".to_owned(),
            format!("<span{} class=\"mtk1 mtkfullwidth\">社</span>", cell(20)),
        ]
    );
}

#[test]
fn characters_outside_the_shared_full_width_classification_are_not_centered() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let ideograph = "\u{2000B}";
    assert_eq!(
        render(&format!("{ideograph}漢")),
        vec![
            format!("<span class=\"mtk1\">{ideograph}</span>"),
            format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(20)),
        ]
    );
}

#[test]
fn supplementary_characters_with_variation_selectors_are_left_intact() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let ideograph_with_variation_selector = "\u{2000B}\u{FE00}";
    assert_eq!(
        render(ideograph_with_variation_selector),
        vec![format!("<span class=\"mtk1\">{ideograph_with_variation_selector}</span>")]
    );
}

#[test]
fn emoji_modifier_and_zwj_sequences_are_left_intact() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let emoji_with_modifier = "\u{1F469}\u{1F3FD}";
    let emoji_with_zwj = "\u{1F469}\u{200D}\u{1F4BB}";
    assert_eq!(
        (render(emoji_with_modifier), render(emoji_with_zwj)),
        (
            vec![format!("<span class=\"mtk1\">{emoji_with_modifier}</span>")],
            vec![format!("<span class=\"mtk1\">{emoji_with_zwj}</span>")],
        )
    );
}

#[test]
fn token_boundaries_are_preserved() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_with("ab漢cd", |o| o.line_tokens = create_view_line_tokens(vec![create_part(3, 1), create_part(5, 2)])),
        vec![
            "<span class=\"mtk1\">ab</span>".to_owned(),
            format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(20)),
            "<span class=\"mtk2\">cd</span>".to_owned(),
        ]
    );
}

#[test]
fn a_token_boundary_in_the_middle_of_a_run_of_full_width_characters_is_preserved() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_with("漢字", |o| o.line_tokens = create_view_line_tokens(vec![create_part(1, 1), create_part(2, 2)])),
        vec![
            format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(20)),
            format!("<span{} class=\"mtk2 mtkfullwidth\">字</span>", cell(20)),
        ]
    );
}

#[test]
fn inline_decorations_are_preserved() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_with("a漢b", |o| {
            o.line_decorations = vec![LineDecoration::new(1, 4, "link", InlineDecorationType::Regular)];
        }),
        vec![
            "<span class=\"mtk1 link\">a</span>".to_owned(),
            format!("<span{} class=\"mtk1 link mtkfullwidth\">漢</span>", cell(20)),
            "<span class=\"mtk1 link\">b</span>".to_owned(),
        ]
    );
}

#[test]
fn rendered_whitespace_next_to_a_full_width_character_is_preserved() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_with("  漢", |o| {
            o.use_monospace_optimizations = true;
            o.render_whitespace = "all".to_owned();
        }),
        vec![
            "<span class=\"mtkw\">\u{b7}\u{200c}\u{b7}\u{200c}</span>".to_owned(),
            format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(20)),
        ]
    );
}

#[test]
fn stop_rendering_line_after_truncates_before_centering() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_with("漢字漢字", |o| o.stop_rendering_line_after = 2),
        vec![
            format!("<span{} class=\"mtk1 mtkfullwidth\">漢</span>", cell(20)),
            format!("<span{} class=\"mtk1 mtkfullwidth\">字</span>", cell(20)),
            "<span class=\"mtkoverflow\">Show more (2 chars)</span>".to_owned(),
        ]
    );
}

// O mapeamento de caracteres já trata um caractere de largura total como duas colunas; centralizar o glifo é o
// que faz a linha renderizada concordar com ele.
#[test]
fn the_character_mapping_is_unaffected_by_the_extra_parts() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = render_view_line(|o| {
        o.line_content = js("a漢b");
        o.is_basic_ascii = false;
        o.line_tokens = create_view_line_tokens(vec![create_part(3, 1)]);
        o.use_two_cell_fullwidth_characters = true;
    });

    assert_character_mapping3(&actual.character_mapping, &[(0, (0, 0)), (1, (1, 0)), (3, (2, 0)), (4, (2, 1))]);
}

// #endregion
