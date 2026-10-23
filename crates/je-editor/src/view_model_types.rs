//! Os tipos de `src/vs/editor/common/viewModel.ts` que o `linesLayout.ts` precisa: `IEditorWhitespace`,
//! `IWhitespaceChangeAccessor`, `ILineHeightChangeAccessor`, `IPartialViewLinesViewportData` e
//! `IViewWhitespaceViewportData`.
//!
//! TODO(integração): mover quando o viewModel for portado.
//!
//! Representação: os `number` que o original reduz com `| 0` (números de linha, ordinal, altura e largura
//! mínima de whitespace) são `i32` já no tipo, então o `| 0` do acessor está feito pela assinatura. Posições
//! verticais, somas de altura e a altura de linha (que pode ser fracionária) são `f64`.

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
