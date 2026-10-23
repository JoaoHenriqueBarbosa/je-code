//! Porte parcial de `src/vs/editor/common/viewModel/inlineDecorations.ts`: `InlineDecorationType` e
//! `InlineDecoration`, o que o `lineDecorations.ts` consome.
// TODO(integração): unificar com view_model_types quando o viewModel for portado.
// Pendente: `IViewDecorationsCollection`, `IInlineDecorationsComputer`, `InlineModelDecorationsComputer`
// (dependem de `ICoordinatesConverter`, `ViewModelDecoration` e `isModelDecorationVisible`, do viewModel
// ainda não portado) e `InjectedTextInlineDecorationsComputer` (depende de `InjectedTextOptions` e do
// contexto de quebra de linha do viewModel).

use je_base::JsString;

use crate::range::Range;

/// `InlineDecorationType` (enum constante no original; os valores numéricos indexam a ordem de
/// `LineDecoration::compare`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InlineDecorationType {
    Regular = 0,
    Before = 1,
    After = 2,
    RegularAffectingLetterSpacing = 3,
    /// Decoração que não cobre nenhum caractere e mesmo assim é renderizada como elemento próprio.
    /// Serve para texto injetado que só reserva espaço horizontal (por exemplo
    /// `{ content: '', widthInEm: 1 }`): não há caractere a decorar, então a largura vem da classe da
    /// decoração. Diferente de `Before`/`After`, não é um pseudo-elemento preso a uma decoração vizinha,
    /// e sim o próprio conteúdo injetado.
    WidthOnly = 4,
}

/// Uma decoração em linha sobre um intervalo da visão.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineDecoration {
    pub range: Range,
    pub inline_class_name: JsString,
    pub r#type: InlineDecorationType,
}

impl InlineDecoration {
    pub fn new(range: Range, inline_class_name: impl Into<JsString>, r#type: InlineDecorationType) -> Self {
        Self {
            range,
            inline_class_name: inline_class_name.into(),
            r#type,
        }
    }
}
