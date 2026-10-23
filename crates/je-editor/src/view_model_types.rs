//! Os tipos de `src/vs/editor/common/viewModel.ts` que o `linesLayout.ts` precisa: `IEditorWhitespace`,
//! `IWhitespaceChangeAccessor`, `ILineHeightChangeAccessor`, `IPartialViewLinesViewportData` e
//! `IViewWhitespaceViewportData`.
//!
//! Também traz `ViewModelDecoration` e `isModelDecorationVisible` (com `isModelDecorationInComment` e
//! `isModelDecorationInString`) de `src/vs/editor/common/viewModel/viewModelDecoration.ts`, que o
//! `inlineDecorations.ts` consome.
//!
//! TODO(integração): mover quando o viewModel for portado.
//!
//! Representação: os `number` que o original reduz com `| 0` (números de linha, ordinal, altura e largura
//! mínima de whitespace) são `i32` já no tipo, então o `| 0` do acessor está feito pela assinatura. Posições
//! verticais, somas de altura e a altura de linha (que pode ser fracionária) são `f64`.

use std::rc::Rc;

use crate::encoded_token_attributes::standard_token_type;
use crate::line_tokens::IViewLineTokens;
use crate::model::ModelDecoration;
use crate::range::Range;
use crate::text_model::{ModelDecorationOptions, TextModel};

/// `IEditorWhitespace`. No original, `getWhitespaces()` devolve os próprios objetos internos do layout; aqui
/// é uma cópia do estado no momento da chamada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IEditorWhitespace {
    pub id: String,
    pub after_line_number: i32,
    pub height: i32,
}

/// `IWhitespaceChangeAccessor`: insere, muda e remove whitespaces em lote.
pub trait IWhitespaceChangeAccessor {
    fn insert_whitespace(&mut self, after_line_number: i32, ordinal: i32, height_in_px: i32, min_width: i32) -> String;
    fn change_one_whitespace(&mut self, id: &str, new_after_line_number: i32, new_height: i32);
    fn remove_whitespace(&mut self, id: &str);
}

/// `ILineHeightChangeAccessor`: insere, muda e remove alturas de linha customizadas.
pub trait ILineHeightChangeAccessor {
    fn insert_or_change_custom_line_height(
        &mut self,
        decoration_id: &str,
        start_line_number: i32,
        end_line_number: i32,
        line_height: f64,
    );
    fn remove_custom_line_height(&mut self, decoration_id: &str);
}

/// `IPartialViewLinesViewportData`.
#[derive(Clone, Debug, PartialEq)]
pub struct IPartialViewLinesViewportData {
    /// Valor a subtrair de `scrollTop` (para os números de offset vertical ficarem abaixo de 1MM).
    pub big_numbers_delta: f64,
    /// A primeira linha (parcialmente) visível.
    pub start_line_number: i32,
    /// A última linha (parcialmente) visível.
    pub end_line_number: i32,
    /// `relative_vertical_offset[i]` é o `top` da linha `i + start_line_number`.
    pub relative_vertical_offset: Vec<f64>,
    /// A linha centralizada no viewport.
    pub centered_line_number: i32,
    /// A primeira linha completamente visível.
    pub completely_visible_start_line_number: i32,
    /// A última linha completamente visível.
    pub completely_visible_end_line_number: i32,
    /// A altura de uma linha.
    pub line_height: f64,
}

/// `IViewWhitespaceViewportData`.
#[derive(Clone, Debug, PartialEq)]
pub struct IViewWhitespaceViewportData {
    pub id: String,
    pub after_line_number: i32,
    pub vertical_offset: f64,
    pub height: i32,
}

/// `ViewModelDecoration` (`viewModelDecoration.ts`): o intervalo da decoração na visão, com as opções do
/// modelo.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewModelDecoration {
    pub range: Range,
    pub options: Rc<ModelDecorationOptions>,
}

impl ViewModelDecoration {
    pub fn new(range: Range, options: Rc<ModelDecorationOptions>) -> Self {
        Self { range, options }
    }
}

/// `isModelDecorationVisible(model, decoration)`.
pub fn is_model_decoration_visible(model: &TextModel, decoration: &ModelDecoration) -> bool {
    if decoration.options.hide_in_comment_tokens.unwrap_or(false) && is_model_decoration_in_comment(model, decoration) {
        return false;
    }

    if decoration.options.hide_in_string_tokens.unwrap_or(false) && is_model_decoration_in_string(model, decoration) {
        return false;
    }

    true
}

/// `isModelDecorationInComment(model, decoration)`.
pub fn is_model_decoration_in_comment(model: &TextModel, decoration: &ModelDecoration) -> bool {
    test_tokens_in_range(model, &decoration.range, |token_type| token_type == standard_token_type::COMMENT)
}

/// `isModelDecorationInString(model, decoration)`.
pub fn is_model_decoration_in_string(model: &TextModel, decoration: &ModelDecoration) -> bool {
    test_tokens_in_range(model, &decoration.range, |token_type| token_type == standard_token_type::STRING)
}

/// `testTokensInRange(model, range, callback)`: chama o `callback` para cada token que cruza o intervalo.
/// Se o `callback` devolve `false`, a iteração para e o resultado é `false`; senão é `true`.
fn test_tokens_in_range(model: &TextModel, range: &Range, mut callback: impl FnMut(u32) -> bool) -> bool {
    for line_number in range.start_line_number..=range.end_line_number {
        let line_tokens = model.tokenization().get_line_tokens(line_number);
        let is_first_line = line_number == range.start_line_number;
        let is_end_line = line_number == range.end_line_number;

        let mut token_idx = if is_first_line { line_tokens.find_token_index_at_offset(range.start_column - 1) } else { 0 };
        while token_idx < line_tokens.get_count() {
            if is_end_line {
                let start_offset = line_tokens.get_start_offset(token_idx);
                if start_offset > range.end_column - 1 {
                    break;
                }
            }

            if !callback(line_tokens.get_standard_token_type(token_idx)) {
                return false;
            }
            token_idx += 1;
        }
    }
    true
}
