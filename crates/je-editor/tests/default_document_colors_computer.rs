//! Porte de `src/vs/editor/test/common/languages/defaultDocumentColorsComputer.test.ts` (suíte `Default Document Colors Computer`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::default_document_colors_computer::{
    IDocumentColorComputerTarget, RegExpMatchArray, compute_default_document_colors, match_all,
};
use je_base::js_regexp::JsRegExp;
use je_editor::position::Position;

struct TestDocumentModel {
    content: Vec<u16>,
}

impl TestDocumentModel {
    fn new(content: &str) -> Self {
        Self { content: content.encode_utf16().collect() }
    }
}

impl IDocumentColorComputerTarget for TestDocumentModel {
    fn get_value(&self) -> Vec<u16> {
        self.content.clone()
    }

    fn position_at(&self, offset: usize) -> Position {
        let head = &self.content[..offset.min(self.content.len())];
        let lines: Vec<&[u16]> = head.split(|&unit| unit == u16::from(b'\n')).collect();
        Position::new(lines.len() as i32, lines[lines.len() - 1].len() as i32 + 1)
    }

    fn find_matches(&self, regex: &JsRegExp) -> Vec<RegExpMatchArray> {
        match_all(&self.content, regex)
    }
}

/// Os `testCases.forEach` do original: cada conteúdo, embrulhado ou não, acha exatamente uma cor.
fn assert_detects_one(content: &str, message: &str) {
    let model = TestDocumentModel::new(content);
    let colors = compute_default_document_colors(&model);
    assert_eq!(colors.len(), 1, "{message}");
}

#[test]
fn hex_colors_in_strings_should_be_detected() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Caso da issue: a cor hexadecimal dentro de uma string não era detectada.
    let model = TestDocumentModel::new("const color = '#ff0000';");
    let colors = compute_default_document_colors(&model);

    assert_eq!(colors.len(), 1, "Should detect one hex color");
    assert_eq!(colors[0].color.red, 1.0, "Red component should be 1 (255/255)");
    assert_eq!(colors[0].color.green, 0.0, "Green component should be 0");
    assert_eq!(colors[0].color.blue, 0.0, "Blue component should be 0");
    assert_eq!(colors[0].color.alpha, 1.0, "Alpha should be 1");
}

#[test]
fn hex_colors_in_double_quotes_should_be_detected() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestDocumentModel::new("const color = \"#00ff00\";");
    let colors = compute_default_document_colors(&model);

    assert_eq!(colors.len(), 1, "Should detect one hex color");
    assert_eq!(colors[0].color.red, 0.0, "Red component should be 0");
    assert_eq!(colors[0].color.green, 1.0, "Green component should be 1 (255/255)");
    assert_eq!(colors[0].color.blue, 0.0, "Blue component should be 0");
}

#[test]
fn multiple_hex_colors_in_array_should_be_detected() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestDocumentModel::new("const colors = ['#ff0000', '#00ff00', '#0000ff'];");
    let colors = compute_default_document_colors(&model);

    assert_eq!(colors.len(), 3, "Should detect three hex colors");

    // Primeira cor: vermelho.
    assert_eq!(colors[0].color.red, 1.0, "First color red component should be 1");
    assert_eq!(colors[0].color.green, 0.0, "First color green component should be 0");
    assert_eq!(colors[0].color.blue, 0.0, "First color blue component should be 0");

    // Segunda cor: verde.
    assert_eq!(colors[1].color.red, 0.0, "Second color red component should be 0");
    assert_eq!(colors[1].color.green, 1.0, "Second color green component should be 1");
    assert_eq!(colors[1].color.blue, 0.0, "Second color blue component should be 0");

    // Terceira cor: azul.
    assert_eq!(colors[2].color.red, 0.0, "Third color red component should be 0");
    assert_eq!(colors[2].color.green, 0.0, "Third color green component should be 0");
    assert_eq!(colors[2].color.blue, 1.0, "Third color blue component should be 1");
}

#[test]
fn existing_functionality_should_still_work() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Casos que já funcionavam.
    let test_cases = [
        ("const color = ' #ff0000';", "hex with space before"),
        ("#ff0000", "hex at start of line"),
        ("  #ff0000", "hex with whitespace before"),
    ];

    for (content, name) in test_cases {
        assert_detects_one(content, &format!("Should still detect {name}"));
    }
}

#[test]
fn eight_digit_hex_colors_should_also_work() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestDocumentModel::new("const color = '#ff0000ff';");
    let colors = compute_default_document_colors(&model);

    assert_eq!(colors.len(), 1, "Should detect one 8-digit hex color");
    assert_eq!(colors[0].color.red, 1.0, "Red component should be 1");
    assert_eq!(colors[0].color.green, 0.0, "Green component should be 0");
    assert_eq!(colors[0].color.blue, 0.0, "Blue component should be 0");
    assert_eq!(colors[0].color.alpha, 1.0, "Alpha should be 1 (ff/255)");
}

#[test]
fn hsl_100_percent_saturation_works_with_decimals() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_detects_one("const color = hsl(253, 100.00%, 47.10%);", "Should detect one hsl color");
}

#[test]
fn hsl_100_percent_saturation_works_without_decimals() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_detects_one("const color = hsl(253, 100%, 47.10%);", "Should detect one hsl color");
}

#[test]
fn hsl_not_100_percent_saturation_should_also_work() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_detects_one("const color = hsl(0, 83.60%, 47.80%);", "Should detect one hsl color");
}

#[test]
fn hsl_with_decimal_hue_values_should_work() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Caso do comentário da issue #180436.
    let test_cases = [
        ("hsl(253.5, 100%, 50%)", "decimal hue"),
        ("hsl(360.0, 50%, 50%)", "360.0 hue"),
        ("hsl(100.5, 50.5%, 50.5%)", "all decimals"),
        ("hsl(0.5, 50%, 50%)", "small decimal hue"),
        ("hsl(359.9, 100%, 50%)", "near-max decimal hue"),
    ];

    for (content, name) in test_cases {
        assert_detects_one(
            &format!("const color = {content};"),
            &format!("Should detect hsl color with {name}: {content}"),
        );
    }
}

#[test]
fn hsla_with_decimal_values_should_work() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_cases = [
        ("hsla(253.5, 100%, 50%, 0.5)", "decimal hue with alpha"),
        ("hsla(360.0, 50.5%, 50.5%, 1)", "all decimals with alpha 1"),
        ("hsla(0.5, 50%, 50%, 0.25)", "small decimal hue with alpha"),
    ];

    for (content, name) in test_cases {
        assert_detects_one(
            &format!("const color = {content};"),
            &format!("Should detect hsla color with {name}: {content}"),
        );
    }
}

#[test]
fn hsl_with_space_separator_css_level_4_syntax_should_work() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // O CSS Level 4 permite valores separados por espaço no lugar da vírgula.
    let test_cases = [
        ("hsl(253 100% 50%)", "space-separated"),
        ("hsl(253.5 100% 50%)", "space-separated with decimal hue"),
        ("hsla(253 100% 50% / 0.5)", "hsla with slash separator for alpha"),
        ("hsla(253.5 100% 50% / 0.5)", "hsla with decimal hue and slash separator"),
        ("hsla(253 100% 50% / 1)", "hsla with slash and alpha 1"),
    ];

    for (content, name) in test_cases {
        assert_detects_one(
            &format!("const color = {content};"),
            &format!("Should detect hsl color with {name}: {content}"),
        );
    }
}

#[test]
fn rgb_and_rgba_with_css_level_4_space_separated_syntax_should_work() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // O CSS Level 4 permite valores separados por espaço para RGB e RGBA.
    let test_cases = [
        ("rgb(255 0 0)", "rgb space-separated"),
        ("rgb(128 128 128)", "rgb space-separated gray"),
        ("rgba(255 0 0 / 0.5)", "rgba with slash separator for alpha"),
        ("rgba(128 128 128 / 0.8)", "rgba gray with slash separator"),
        ("rgba(255 0 0 / 1)", "rgba with slash and alpha 1"),
        // A sintaxe tradicional com vírgula continua valendo.
        ("rgb(255, 0, 0)", "rgb comma-separated (traditional)"),
        ("rgba(255, 0, 0, 0.5)", "rgba comma-separated (traditional)"),
    ];

    for (content, name) in test_cases {
        assert_detects_one(
            &format!("const color = {content};"),
            &format!("Should detect rgb/rgba color with {name}: {content}"),
        );
    }
}
