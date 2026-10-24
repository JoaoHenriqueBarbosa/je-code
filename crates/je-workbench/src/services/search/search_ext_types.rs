//! Porte de `src/vs/workbench/services/search/common/searchExtTypes.ts`: os tipos de busca da API de extensões.
//!
//! Portado o que o código do workbench constrói e lê: `Position`, `Range` e os dois enums
//! (`ExcludeSettingOptions`, `TextSearchCompleteMessageType`). Os métodos de `Position` e `Range` que o original
//! declara só como assinatura do tipo da API (devolvem constantes, a implementação real vive no host de
//! extensões) não existem aqui. As interfaces e classes que dependem de `URI`, `CancellationToken` e
//! `IProgress` (provedores de busca de texto e de arquivo, `TextSearchMatch2`, `TextSearchContext2`...) ficam
//! para quando a API de extensões for portada.
//!
//! Linhas e colunas são `i64`, como em [`SearchRange`](super::search::SearchRange).

/// `Position`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: i64,
    pub character: i64,
}

impl Position {
    pub fn new(line: i64, character: i64) -> Self {
        Self { line, character }
    }
}

/// `Range`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start_line: i64, start_col: i64, end_line: i64, end_col: i64) -> Self {
        Self { start: Position::new(start_line, start_col), end: Position::new(end_line, end_col) }
    }
}

/// `ExcludeSettingOptions`: opções para seguir os settings `search.exclude` e `files.exclude`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExcludeSettingOptions {
    /// Não usa nenhum setting de exclusão.
    None = 1,
    /// Usa o setting `files.exclude`.
    FilesExclude = 2,
    /// Usa os settings `files.exclude` e `search.exclude`.
    SearchAndFilesExclude = 3,
}

/// `TextSearchCompleteMessageType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSearchCompleteMessageType {
    Information = 1,
    Warning = 2,
}
