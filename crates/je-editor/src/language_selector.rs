//! Porte de `src/vs/editor/common/languageSelector.ts`: o seletor de linguagem (`LanguageSelector`) e a
//! função [`score`], que mede o quanto um seletor casa com um documento (0 não casa, 5 casa por curinga,
//! 10 casa exato).
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `string | LanguageFilter | ReadonlyArray<string | LanguageFilter>` é o enum [`LanguageSelector`]; o
//!   `undefined` e o `null` do seletor são `None` em [`score`]. O original aceita aninhar arrays em tempo de
//!   execução (a recursão testa `Array.isArray`), por isso `Array` guarda `Vec<LanguageSelector>`.
//! - `pattern?: string | IRelativePattern` é [`FilterPattern`]; os campos opcionais de texto são
//!   `Option<String>` e, como no original, o texto vazio conta como ausente (`if (scheme)`).
//! - O `Set<string>` de [`select_language_ids`] é um `Vec<String>` sem repetição que preserva a ordem de
//!   inserção, a mesma que o `Set` do JavaScript enumera.
//! - `fsPath` e `normalize` usam a plataforma do processo, como o original (`isWindows`, `process.platform`).

use je_base::JsString;
use je_base::glob::{self, GlobOptions, PatternArg, RelativePattern};
use je_base::path::{Environment, PathModule, ProcessEnvironment};
use je_base::uri::URI;

/// `pattern?: string | IRelativePattern` de um [`LanguageFilter`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterPattern {
    Glob(String),
    Relative(RelativePattern),
}

impl From<&str> for FilterPattern {
    fn from(value: &str) -> Self {
        FilterPattern::Glob(value.to_string())
    }
}

impl From<RelativePattern> for FilterPattern {
    fn from(value: RelativePattern) -> Self {
        FilterPattern::Relative(value)
    }
}

/// `LanguageFilter`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguageFilter {
    pub language: Option<String>,
    pub scheme: Option<String>,
    pub pattern: Option<FilterPattern>,
    pub notebook_type: Option<String>,
    /// Este provedor é implementado na thread de UI.
    pub has_access_to_all_models: bool,
    pub exclusive: bool,
    /// Este provedor vem de uma extensão embutida.
    pub is_builtin: bool,
}

/// `LanguageSelector`: `string | LanguageFilter | ReadonlyArray<string | LanguageFilter>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanguageSelector {
    Id(String),
    Filter(LanguageFilter),
    Array(Vec<LanguageSelector>),
}

impl LanguageSelector {
    pub fn array(items: impl IntoIterator<Item = LanguageSelector>) -> Self {
        LanguageSelector::Array(items.into_iter().collect())
    }
}

impl From<&str> for LanguageSelector {
    fn from(value: &str) -> Self {
        LanguageSelector::Id(value.to_string())
    }
}

impl From<LanguageFilter> for LanguageSelector {
    fn from(value: LanguageFilter) -> Self {
        LanguageSelector::Filter(value)
    }
}

/// O texto de um campo opcional, ausente quando `None` ou vazio (a verdade de `if (campo)`).
fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|text| !text.is_empty())
}

/// `candidateUri.scheme === scheme`, comparando em unidades UTF-16.
fn scheme_equals(uri: &URI, scheme: &str) -> bool {
    *uri.scheme() == JsString::from(scheme)
}

pub fn score(
    selector: Option<&LanguageSelector>,
    candidate_uri: &URI,
    candidate_language: &str,
    candidate_is_synchronized: bool,
    candidate_notebook_uri: Option<&URI>,
    candidate_notebook_type: Option<&str>,
) -> i32 {
    match selector {
        Some(LanguageSelector::Array(filters)) => {
            // array -> take max individual value
            let mut ret = 0;
            for filter in filters {
                let value = score(
                    Some(filter),
                    candidate_uri,
                    candidate_language,
                    candidate_is_synchronized,
                    candidate_notebook_uri,
                    candidate_notebook_type,
                );
                if value == 10 {
                    return value; // already at the highest
                }
                if value > ret {
                    ret = value;
                }
            }
            ret
        }
        Some(LanguageSelector::Id(selector)) => {
            if !candidate_is_synchronized {
                return 0;
            }

            // short-hand notion, desugars to
            // 'fooLang' -> { language: 'fooLang'}
            // '*' -> { language: '*' }
            if selector == "*" {
                5
            } else if selector == candidate_language {
                10
            } else {
                0
            }
        }
        Some(LanguageSelector::Filter(filter)) => {
            score_filter(filter, candidate_uri, candidate_language, candidate_is_synchronized, candidate_notebook_uri, candidate_notebook_type)
        }
        None => 0,
    }
}

/// O ramo `else if (selector)` de `score`: o seletor é um [`LanguageFilter`].
fn score_filter<'a>(
    filter: &LanguageFilter,
    mut candidate_uri: &'a URI,
    candidate_language: &str,
    candidate_is_synchronized: bool,
    candidate_notebook_uri: Option<&'a URI>,
    candidate_notebook_type: Option<&str>,
) -> i32 {
    // filter -> select accordingly, use defaults for scheme
    if !candidate_is_synchronized && !filter.has_access_to_all_models {
        return 0;
    }

    let notebook_type = non_empty(&filter.notebook_type);

    // selector targets a notebook -> use the notebook uri instead
    // of the "normal" document uri.
    if let (Some(_), Some(notebook_uri)) = (notebook_type, candidate_notebook_uri) {
        candidate_uri = notebook_uri;
    }

    let mut ret = 0;

    if let Some(scheme) = non_empty(&filter.scheme) {
        if scheme_equals(candidate_uri, scheme) {
            ret = 10;
        } else if scheme == "*" {
            ret = 5;
        } else {
            return 0;
        }
    }

    if let Some(language) = non_empty(&filter.language) {
        if language == candidate_language {
            ret = 10;
        } else if language == "*" {
            ret = ret.max(5);
        } else {
            return 0;
        }
    }

    if let Some(notebook_type) = notebook_type {
        if Some(notebook_type) == candidate_notebook_type {
            ret = 10;
        } else if notebook_type == "*" && candidate_notebook_type.is_some() {
            ret = ret.max(5);
        } else {
            return 0;
        }
    }

    if let Some(pattern) = &filter.pattern
        && !matches!(pattern, FilterPattern::Glob(text) if text.is_empty())
    {
        let fs_path = candidate_uri.fs_path(ProcessEnvironment.platform()).to_string_lossy();
        let matched = match pattern {
            FilterPattern::Glob(text) => text == &fs_path || glob::match_pattern(PatternArg::Str(text), &fs_path, &GlobOptions::default()),
            FilterPattern::Relative(relative) => {
                // Since this pattern has a `base` property, we need
                // to normalize this path first before passing it on
                // because we will compare it against `Uri.fsPath`
                // which uses platform specific separators.
                // Refs: https://github.com/microsoft/vscode/issues/99938
                let normalized = RelativePattern {
                    base: PathModule::native(&ProcessEnvironment).normalize(&relative.base),
                    pattern: relative.pattern.clone(),
                };
                glob::match_pattern(&normalized, &fs_path, &GlobOptions::default())
            }
        };
        if matched {
            ret = 10;
        } else {
            return 0;
        }
    }

    ret
}

pub fn targets_notebooks(selector: &LanguageSelector) -> bool {
    match selector {
        LanguageSelector::Id(_) => false,
        LanguageSelector::Array(items) => items.iter().any(targets_notebooks),
        LanguageSelector::Filter(filter) => non_empty(&filter.notebook_type).is_some(),
    }
}

pub fn select_language_ids(selector: &LanguageSelector, into: &mut Vec<String>) {
    match selector {
        LanguageSelector::Id(id) => insert_id(into, id),
        LanguageSelector::Array(items) => {
            for item in items {
                select_language_ids(item, into);
            }
        }
        LanguageSelector::Filter(filter) => {
            if let Some(language) = non_empty(&filter.language) {
                insert_id(into, language);
            }
        }
    }
}

/// `Set#add`: acrescenta ao fim, sem repetir.
fn insert_id(into: &mut Vec<String>, id: &str) {
    if !into.iter().any(|existing| existing == id) {
        into.push(id.to_string());
    }
}
