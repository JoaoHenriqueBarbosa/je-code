//! Porte de `src/vs/editor/browser/widget/multiDiffEditor/colors.ts`: os identificadores das três cores do editor de
//! diff de vários arquivos.
//!
//! No original cada `registerColor(id, defaults, description)` devolve o identificador que os temas e os widgets usam para
//! pedir a cor. Aqui o registro (identificador, padrões por tema e descrição) já está em
//! `je_theme::registered_colors` (marcado com a origem `editor/browser/widget/multiDiffEditor/colors.ts`), e este módulo
//! guarda só os identificadores, para quem pinta pedir a cor ao tema sem repetir o texto.

/// `multiDiffEditorHeaderBackground`: a cor de fundo do cabeçalho de cada arquivo.
pub const MULTI_DIFF_EDITOR_HEADER_BACKGROUND: &str = "multiDiffEditor.headerBackground";

/// `multiDiffEditorBackground`: a cor de fundo do editor de diff de vários arquivos.
pub const MULTI_DIFF_EDITOR_BACKGROUND: &str = "multiDiffEditor.background";

/// `multiDiffEditorBorder`: a cor da borda do editor de diff de vários arquivos.
pub const MULTI_DIFF_EDITOR_BORDER: &str = "multiDiffEditor.border";
