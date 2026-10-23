//! Porte de `src/vs/editor/common/textModelGuides.ts`: os tipos dos guias de indentação e de pares de
//! colchetes.

/// `IActiveIndentGuideInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveIndentGuideInfo {
    pub start_line_number: i32,
    pub end_line_number: i32,
    pub indent: i32,
}

/// `HorizontalGuidesState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalGuidesState {
    Disabled,
    EnabledForActive,
    Enabled,
}

/// `BracketGuideOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BracketGuideOptions {
    pub include_inactive: bool,
    pub horizontal_guides: HorizontalGuidesState,
    pub highlight_active: bool,
}

/// `IndentGuide`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndentGuide {
    /// `-1` se não é um guia vertical
    pub visible_column: i32,
    /// `-1` se não é um guia por coluna do modelo
    pub column: i32,
    pub class_name: String,
    /// Se definido, o guia é horizontal (sem parte vertical). Começa em `visible_column` e continua até
    /// `end_column`.
    pub horizontal_line: Option<IndentGuideHorizontalLine>,
    /// Se diferente de `-1`, mostra o guia só para linhas quebradas que não contêm esta coluna do modelo, mas
    /// vêm depois dela.
    pub for_wrapped_lines_after_column: i32,
    pub for_wrapped_lines_before_or_at_column: i32,
}

impl IndentGuide {
    /// O construtor do original lança se `visibleColumn` e `column` forem ambos definidos ou ambos `-1`.
    pub fn new(
        visible_column: i32,
        column: i32,
        class_name: String,
        horizontal_line: Option<IndentGuideHorizontalLine>,
        for_wrapped_lines_after_column: i32,
        for_wrapped_lines_before_or_at_column: i32,
    ) -> Self {
        if (visible_column != -1) == (column != -1) {
            je_base::errors::throw(je_base::errors::Error::new(
                "IndentGuide: visibleColumn e column não podem ser ambos definidos ou ambos -1",
            ));
        }
        Self {
            visible_column,
            column,
            class_name,
            horizontal_line,
            for_wrapped_lines_after_column,
            for_wrapped_lines_before_or_at_column,
        }
    }
}

/// `IndentGuideHorizontalLine`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndentGuideHorizontalLine {
    pub top: bool,
    pub end_column: i32,
}
