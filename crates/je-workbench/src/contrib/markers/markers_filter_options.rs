//! Porte de `src/vs/workbench/contrib/markers/browser/markersFilterOptions.ts`: as opções do filtro do painel de
//! problemas (`@source:`, negação com `!`, globs separados por vírgula e o `files.exclude` por pasta raiz).
//!
//! Notas de tradução:
//! - **`SOURCE_FILTER_REGEX`.** O original usa `/(!)?@source:("[^"]*"|[^\s,]+)(\s*)/i`; o porte faz a mesma busca à
//!   esquerda sem biblioteca de regex ([`find_source_filter`]), com `\s` do JavaScript ([`is_js_whitespace`]) e a flag
//!   `i` só sobre o ASCII de `source`, como o motor faz sem a flag `u`.
//! - **`filesExclude`.** O parâmetro `{ root, expression }[] | IExpression` é o enum [`FilesExclude`]. O construtor
//!   original altera a expressão recebida (acrescenta `<padrão>/**`); aqui ele toma a posse da expressão.
//! - **`trim`, `ltrim` e `rtrim`.** O `trim` é o do JavaScript; o `ltrim(filter, '!')` e o `rtrim(pattern, '/')` com um
//!   único caractere são `trim_start_matches` e `trim_end_matches`.
//! - **`IFilter`.** `matchesFuzzy` tem o terceiro parâmetro opcional ausente (`false`) na chamada original.

use std::rc::Rc;

use je_base::filters::{Match, matches_fuzzy, matches_fuzzy2};
use je_base::glob::{ExprValue, Expression, GlobOptions, ParsedExpression, parse_expression, split_glob_aware};
use je_base::js_string::is_js_whitespace;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::URI;
use je_base::JsString;
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

/// Uma expressão do `files.exclude` de uma pasta raiz do workspace (`{ root: URI; expression: IExpression }`).
#[derive(Clone, Debug)]
pub struct RootExpression {
    pub root: URI,
    pub expression: Expression,
}

/// O `{ root: URI; expression: ParsedExpression }` guardado na árvore por raiz.
struct ParsedRootExpression {
    root: URI,
    expression: ParsedExpression,
}

/// `String#trim` do JavaScript (o conjunto de espaços dele difere do `str::trim` do Rust).
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.len_utf16() == 1 && is_js_whitespace(c as u16))
}

fn to_lower_case(text: &str) -> String {
    JsString::from(text).to_lower_case().to_string_lossy()
}

pub struct ResourceGlobMatcher {
    global_expression: ParsedExpression,
    expressions_by_root: TernarySearchTree<URI, ParsedRootExpression>,
}

impl ResourceGlobMatcher {
    pub fn new(global_expression: &Expression, root_expressions: &[RootExpression], uri_identity_service: &dyn IUriIdentityService) -> Self {
        let options = GlobOptions::default();
        let global_expression = parse_expression(global_expression, &options);
        let uri_ext = uri_identity_service.ext_uri();
        let mut expressions_by_root = TernarySearchTree::for_uris(Rc::new(move |uri: &URI| uri_ext.ignore_path_casing(uri)), Rc::new(|_: &URI| false));
        for expression in root_expressions {
            expressions_by_root
                .set(expression.root.clone(), ParsedRootExpression { root: expression.root.clone(), expression: parse_expression(&expression.expression, &options) });
        }
        Self { global_expression, expressions_by_root }
    }

    pub fn matches(&self, resource: &URI) -> bool {
        if let Some(root_expression) = self.expressions_by_root.find_substr(resource) {
            let path = ext_uri().relative_path(&root_expression.root, resource);
            if let Some(path) = path.filter(|path| !path.is_empty()) {
                if !root_expression.expression.matches(&path, None, None).is_none() {
                    return true;
                }
            }
        }
        !self.global_expression.matches(&resource.path().to_string_lossy(), None, None).is_none()
    }
}

/// O parâmetro `filesExclude`: `{ root, expression }[] | IExpression`.
#[derive(Clone, Debug)]
pub enum FilesExclude {
    ByRoot(Vec<RootExpression>),
    Global(Expression),
}

/// `textFilter`: `{ readonly text: string; readonly negate: boolean }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFilter {
    pub text: String,
    pub negate: bool,
}

/// A primeira ocorrência de `SOURCE_FILTER_REGEX` no texto.
struct SourceMatch {
    /// O `sourceMatch.index`, em caracteres.
    start: usize,
    /// O fim de `sourceMatch[0]` (inclui os espaços finais), em caracteres.
    end: usize,
    /// O `!` inicial (`sourceMatch[1]`).
    negate: bool,
    /// O `sourceMatch[2]`, com as aspas se houver.
    source: String,
}

const SOURCE_PREFIX: &str = "@source:";

fn is_space(c: char) -> bool {
    c.len_utf16() == 1 && is_js_whitespace(c as u16)
}

/// `SOURCE_FILTER_REGEX.exec(filter)`: `/(!)?@source:("[^"]*"|[^\s,]+)(\s*)/i`, a ocorrência mais à esquerda.
fn find_source_filter(filter: &[char]) -> Option<SourceMatch> {
    let prefix: Vec<char> = SOURCE_PREFIX.chars().collect();
    (0..filter.len()).find_map(|start| {
        let negate = filter[start] == '!';
        let prefix_start = start + usize::from(negate);
        let prefix_end = prefix_start + prefix.len();
        let prefix_matches = filter.get(prefix_start..prefix_end).is_some_and(|candidate| candidate.iter().zip(&prefix).all(|(a, b)| a.eq_ignore_ascii_case(b)));
        if !prefix_matches {
            return None;
        }

        // `"[^"]*"` primeiro; sem a aspa de fechamento, cai para `[^\s,]+`.
        let value_end = (if filter.get(prefix_end) == Some(&'"') {
            filter[prefix_end + 1..].iter().position(|&c| c == '"').map(|closing| prefix_end + 1 + closing + 1)
        } else {
            None
        })
        .or_else(|| {
            let length = filter[prefix_end..].iter().take_while(|&&c| !is_space(c) && c != ',').count();
            (length > 0).then_some(prefix_end + length)
        })?;

        let end = value_end + filter[value_end..].iter().take_while(|&&c| is_space(c)).count();
        Some(SourceMatch { start, end, negate, source: filter[prefix_end..value_end].iter().collect() })
    })
}

pub struct FilterOptions {
    pub filter: String,
    pub show_warnings: bool,
    pub show_errors: bool,
    pub show_infos: bool,
    pub text_filter: TextFilter,
    pub excludes_matcher: ResourceGlobMatcher,
    pub includes_matcher: ResourceGlobMatcher,

    pub include_source_filters: Vec<String>,
    pub exclude_source_filters: Vec<String>,
}

impl FilterOptions {
    /// `_filter`.
    pub const FILTER: fn(&[u16], &[u16]) -> Option<Vec<Match>> = matches_fuzzy2;
    /// `_messageFilter`.
    pub const MESSAGE_FILTER: fn(&[u16], &[u16]) -> Option<Vec<Match>> = |word, word_to_match_against| matches_fuzzy(word, word_to_match_against, false);

    /// `EMPTY(uriIdentityService)`.
    pub fn empty(uri_identity_service: &dyn IUriIdentityService) -> Self {
        Self::new("", FilesExclude::ByRoot(Vec::new()), false, false, false, uri_identity_service)
    }

    pub fn new(
        filter: &str,
        files_exclude: FilesExclude,
        show_warnings: bool,
        show_errors: bool,
        show_infos: bool,
        uri_identity_service: &dyn IUriIdentityService,
    ) -> Self {
        let original_filter = filter;
        let mut filter = js_trim(filter).to_string();

        let (mut files_exclude_by_root, mut excludes_expression) = match files_exclude {
            FilesExclude::ByRoot(by_root) => (by_root, Expression::new()),
            FilesExclude::Global(expression) => (Vec::new(), expression),
        };

        for root_expression in &mut files_exclude_by_root {
            let expression = &mut root_expression.expression;
            let entries: Vec<(String, ExprValue)> = expression.entries().to_vec();
            for (pattern, value) in entries {
                if !pattern.ends_with("/**") {
                    // Append `/**` to pattern to match a parent folder #103631
                    expression.insert(format!("{}/**", pattern.trim_end_matches('/')), value);
                }
            }
        }

        let mut include_source_filters: Vec<String> = Vec::new();
        let mut exclude_source_filters: Vec<String> = Vec::new();
        loop {
            let characters: Vec<char> = filter.chars().collect();
            let Some(source_match) = find_source_filter(&characters) else { break };
            let raw: Vec<char> = source_match.source.chars().collect();
            // Remove quotes if present
            let source: String = if raw.first() == Some(&'"') && raw.last() == Some(&'"') {
                // `slice(1, -1)`: um único `"` vira a cadeia vazia
                raw.get(1..raw.len() - 1).unwrap_or_default().iter().collect()
            } else {
                raw.iter().collect()
            };
            if source_match.negate {
                exclude_source_filters.push(to_lower_case(&source));
            } else {
                include_source_filters.push(to_lower_case(&source));
            }
            // Remove the entire match (including trailing whitespace)
            let remaining: String = characters[..source_match.start].iter().chain(&characters[source_match.end..]).collect();
            filter = js_trim(&remaining).to_string();
        }

        let negate = filter.starts_with('!');
        let text_filter = TextFilter { text: js_trim(if negate { filter.trim_start_matches('!') } else { filter.as_str() }).to_string(), negate };
        let mut include_expression = Expression::new();

        if !filter.is_empty() {
            let filters: Vec<String> = split_glob_aware(&filter, ',').iter().map(|s| js_trim(s).to_string()).filter(|s| !s.is_empty()).collect();
            for f in filters {
                if f.starts_with('!') {
                    let filter_text = f.trim_start_matches('!');
                    if !filter_text.is_empty() {
                        Self::set_pattern(&mut excludes_expression, filter_text);
                    }
                } else {
                    Self::set_pattern(&mut include_expression, &f);
                }
            }
        }

        let excludes_matcher = ResourceGlobMatcher::new(&excludes_expression, &files_exclude_by_root, uri_identity_service);
        let includes_matcher = ResourceGlobMatcher::new(&include_expression, &[], uri_identity_service);

        Self {
            filter: original_filter.to_string(),
            show_warnings,
            show_errors,
            show_infos,
            text_filter,
            excludes_matcher,
            includes_matcher,
            include_source_filters,
            exclude_source_filters,
        }
    }

    pub fn matches_source_filters(&self, marker_source: Option<&str>) -> bool {
        if self.include_source_filters.is_empty() && self.exclude_source_filters.is_empty() {
            return true;
        }

        // `markerSource?.toLowerCase()`; a cadeia vazia é falsa no `source &&` do original.
        let source = marker_source.map(to_lower_case).filter(|source| !source.is_empty());

        // Check negative filters first - if any match, exclude
        if source.as_ref().is_some_and(|source| self.exclude_source_filters.contains(source)) {
            return false;
        }

        // If there are positive filters, check if any match
        if !self.include_source_filters.is_empty() {
            return source.is_some_and(|source| self.include_source_filters.contains(&source));
        }

        true
    }

    fn set_pattern(expression: &mut Expression, pattern: &str) {
        let pattern = if pattern.starts_with('.') { format!("*{pattern}") } else { pattern.to_string() }; // convert ".js" to "*.js"
        expression.insert(format!("**/{pattern}/**"), true);
        expression.insert(format!("**/{pattern}"), true);
    }
}
