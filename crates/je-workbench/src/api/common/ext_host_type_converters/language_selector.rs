//! `extHostTypeConverters.ts`: `LanguageSelector`, a conversão do `vscode.DocumentSelector` para o seletor de linguagem
//! do editor (`languageSelector.ts`).
//!
//! Divergências:
//! - `vscode.DocumentSelector` (`DocumentFilter | string | ReadonlyArray<DocumentFilter | string>`) é o
//!   [`DocumentSelectorInput`]. `GlobPattern.from` já existe em `ext_host_workspace` ([`glob_pattern_from`], com os tipos
//!   [`GlobPattern`] e [`GlobPatternDto`]) e é reutilizado, de modo que o `pattern` de um filtro é um `GlobPattern` tipado: o
//!   ramo "objeto com a forma de `RelativePattern`" do original não existe.
//! - Um item falso de uma lista (texto vazio) é descartado, pois o `LanguageSelector` do editor não tem o `undefined`. O
//!   `exclusive` ausente é `false` no `LanguageFilter`.

use je_base::glob::RelativePattern as EditorRelativePattern;
use je_editor::language_selector::{FilterPattern, LanguageFilter, LanguageSelector as EditorLanguageSelector};

use crate::api::common::ext_host_workspace::{GlobPattern, GlobPatternDto, glob_pattern_from};

/// `vscode.DocumentFilter`.
#[derive(Clone, Debug, Default)]
pub struct DocumentFilterInput {
    pub language: Option<String>,
    pub scheme: Option<String>,
    pub pattern: Option<GlobPattern>,
    pub exclusive: Option<bool>,
    pub notebook_type: Option<String>,
}

/// `vscode.DocumentSelector`.
#[derive(Clone, Debug)]
pub enum DocumentSelectorInput {
    Language(String),
    Filter(DocumentFilterInput),
    Array(Vec<DocumentSelectorInput>),
}

/// `typeConverters.LanguageSelector`.
pub struct LanguageSelector;

impl LanguageSelector {
    /// `LanguageSelector.from`: um seletor ausente (ou um texto vazio) é `None`.
    pub fn from(selector: Option<&DocumentSelectorInput>) -> Option<EditorLanguageSelector> {
        match selector? {
            DocumentSelectorInput::Language(language) if language.is_empty() => None,
            DocumentSelectorInput::Language(language) => Some(EditorLanguageSelector::Id(language.clone())),
            DocumentSelectorInput::Array(items) => Some(EditorLanguageSelector::Array(items.iter().filter_map(|item| LanguageSelector::from(Some(item))).collect())),
            DocumentSelectorInput::Filter(filter) => Some(EditorLanguageSelector::Filter(LanguageFilter {
                language: filter.language.clone(),
                scheme: filter.scheme.clone(),
                pattern: filter.pattern.as_ref().map(|pattern| match glob_pattern_from(pattern) {
                    GlobPatternDto::Pattern(text) => FilterPattern::Glob(text),
                    GlobPatternDto::Relative(dto) => FilterPattern::Relative(EditorRelativePattern { base: dto.base, pattern: dto.pattern }),
                }),
                exclusive: filter.exclusive.unwrap_or(false),
                notebook_type: filter.notebook_type.clone(),
                ..LanguageFilter::default()
            })),
        }
    }
}
