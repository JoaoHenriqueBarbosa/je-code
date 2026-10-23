//! Porte de `EditorLineDecorationsWidth` (`editorOptions.ts`, linhas 3352 a 3379).
//!
//! O valor validado é um número que, quando negativo, sinaliza um múltiplo da largura do caractere
//! (`"2ch"` vira `-2`). `compute` resolve o múltiplo com `env.fontInfo.typicalHalfwidthCharacterWidth`;
//! como [`EnvironmentalOptions`] ainda não carrega o `FontInfo` (`// TODO(config)` em `editor_options.rs`),
//! a conta está em [`EditorLineDecorationsWidth::compute_with_font_info`], e o `compute` do trait a chama
//! assim que o ambiente tiver a fonte.

use crate::config::editor_options::{clamped_int, EditorOption, EditorOptionImpl, Opt, OptionInput};
use crate::config::font_info::FontInfo;

/// Chave tipada de `lineDecorationsWidth`.
pub const LINE_DECORATIONS_WIDTH: Opt<f64> = Opt::new(EditorOption::LineDecorationsWidth);

/// `/^\d+(\.\d+)?ch$/` seguido de `parseFloat` no prefixo numérico. `\d` do JavaScript é só ASCII.
fn parse_ch_multiple(input: &str) -> Option<f64> {
    let number = input.strip_suffix("ch")?;
    let (integer, fraction) = match number.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (number, None),
    };
    let is_digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    if !is_digits(integer) || fraction.is_some_and(|fraction| !is_digits(fraction)) {
        return None;
    }
    number.parse().ok()
}

/// `EditorLineDecorationsWidth` (`number | string` na entrada, `number` calculado).
pub struct EditorLineDecorationsWidth;

impl EditorLineDecorationsWidth {
    pub fn new() -> Self {
        EditorLineDecorationsWidth
    }

    /// `compute` do original: `env.fontInfo` entra como parâmetro.
    pub fn compute_with_font_info(&self, font_info: &FontInfo, value: f64) -> f64 {
        if value < 0.0 {
            // Número negativo sinaliza um múltiplo.
            return clamped_int(&OptionInput::Number(-value * font_info.typical_halfwidth_character_width), self.default_value(), 0.0, 1000.0);
        }
        value
    }
}

impl Default for EditorLineDecorationsWidth {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorLineDecorationsWidth {
    type Value = f64;

    fn id(&self) -> EditorOption {
        EditorOption::LineDecorationsWidth
    }

    fn name(&self) -> &str {
        "lineDecorationsWidth"
    }

    fn default_value(&self) -> f64 {
        10.0
    }

    fn validate(&self, input: &OptionInput) -> f64 {
        if let OptionInput::String(text) = input {
            if let Some(multiple) = parse_ch_multiple(text) {
                // Número negativo sinaliza um múltiplo.
                return -multiple;
            }
        }
        clamped_int(input, self.default_value(), 0.0, 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::test_font_info;

    fn validate(input: OptionInput) -> f64 {
        EditorLineDecorationsWidth::new().validate(&input)
    }

    #[test]
    fn default_is_ten() {
        assert_eq!(EditorLineDecorationsWidth::new().default_value(), 10.0);
        assert_eq!(validate(OptionInput::Undefined), 10.0);
    }

    #[test]
    fn ch_strings_become_negative_multiples() {
        assert_eq!(validate(OptionInput::string("2ch")), -2.0);
        assert_eq!(validate(OptionInput::string("2.5ch")), -2.5);
        assert_eq!(validate(OptionInput::string("007ch")), -7.0);
        // Sem limite superior: o múltiplo não passa pelo clamp na validação.
        assert_eq!(validate(OptionInput::string("5000ch")), -5000.0);
    }

    #[test]
    fn malformed_ch_strings_go_through_clamped_int() {
        assert_eq!(validate(OptionInput::string("ch")), 10.0);
        assert_eq!(validate(OptionInput::string(".5ch")), 10.0);
        // `parseInt("-2ch")` dá -2, que o clamp sobe para o mínimo.
        assert_eq!(validate(OptionInput::string("-2ch")), 0.0);
        assert_eq!(validate(OptionInput::string("1.2.3ch")), 1.0);
        assert_eq!(validate(OptionInput::string("2ch ")), 2.0);
        assert_eq!(validate(OptionInput::string(" 2ch")), 2.0);
        // `parseInt("1.ch")` dá 1.
        assert_eq!(validate(OptionInput::string("1.ch")), 1.0);
        assert_eq!(validate(OptionInput::string("2CH")), 2.0);
    }

    #[test]
    fn plain_values_are_clamped_integers() {
        assert_eq!(validate(OptionInput::Number(12.7)), 12.0);
        assert_eq!(validate(OptionInput::Number(-5.0)), 0.0);
        assert_eq!(validate(OptionInput::Number(5000.0)), 1000.0);
        assert_eq!(validate(OptionInput::string("30")), 30.0);
        assert_eq!(validate(OptionInput::string("abc")), 10.0);
        assert_eq!(validate(OptionInput::Bool(true)), 10.0);
        assert_eq!(validate(OptionInput::Null), 10.0);
    }

    #[test]
    fn compute_resolves_multiples_with_the_font() {
        let option = EditorLineDecorationsWidth::new();
        let font = test_font_info(19.0, 8.0);
        assert_eq!(option.compute_with_font_info(&font, 15.0), 15.0);
        assert_eq!(option.compute_with_font_info(&font, -2.5), 20.0);
        assert_eq!(option.compute_with_font_info(&font, -0.1), 0.0);
        // O múltiplo calculado também é limitado a 1000.
        assert_eq!(option.compute_with_font_info(&font, -500.0), 1000.0);
        assert_eq!(option.compute_with_font_info(&font, 0.0), 0.0);
    }
}
