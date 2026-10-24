//! Porte de `src/vs/workbench/contrib/debug/browser/replFilter.ts`: o filtro do console de depuração (a consulta separada
//! por vírgulas, com `!` para excluir e `\` para escapar o `!`), a parte lógica do filtro da árvore do REPL.
//!
//! Notas de tradução:
//! - `ITreeFilter<IReplElement, FuzzyScore>` é [`ITreeFilter<ReplElement>`] sem dados de filtro (o `FuzzyScore` que o
//!   original não devolve nunca); o resultado `boolean | TreeVisibility` é o [`TreeFilterResult`].
//! - `element instanceof ReplEvaluationInput | ReplEvaluationResult | Variable` é o `match` na variante de [`ReplElement`] e
//!   `IExpression::is_variable` (uma `Variable` filha de um elemento de saída é um `ReplElement::Expression`).
//! - `ReplFilter.matchQuery` (`matchesFuzzy`) casa em unidades UTF-16.

use std::cell::RefCell;

use je_base::filters::matches_fuzzy;
use je_base::glob::split_glob_aware;
use je_widgets::tree::tree_types::{ITreeFilter, TreeFilterResult, TreeVisibility};

use super::repl_model::ReplElement;

#[derive(Clone, Debug, PartialEq, Eq)]
enum QueryType {
    Include,
    Exclude,
}

#[derive(Clone, Debug)]
struct ParsedQuery {
    kind: QueryType,
    query: String,
}

/// `ReplFilter`.
#[derive(Default)]
pub struct ReplFilter {
    parsed_queries: RefCell<Vec<ParsedQuery>>,
}

impl ReplFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `ReplFilter.matchQuery(query, text)` (`matchesFuzzy`).
    fn match_query(query: &str, text: &str) -> bool {
        let query: Vec<u16> = query.encode_utf16().collect();
        let text: Vec<u16> = text.encode_utf16().collect();
        matches_fuzzy(&query, &text, false).is_some()
    }

    /// `set filterQuery(query)`.
    pub fn set_filter_query(&self, query: &str) {
        let mut parsed_queries = Vec::new();
        let query = query.trim();
        if !query.is_empty() {
            let filters: Vec<String> = split_glob_aware(query, ',').iter().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
            for f in filters {
                if let Some(rest) = f.strip_prefix('\\') {
                    parsed_queries.push(ParsedQuery { kind: QueryType::Include, query: rest.to_owned() });
                } else if let Some(rest) = f.strip_prefix('!') {
                    parsed_queries.push(ParsedQuery { kind: QueryType::Exclude, query: rest.to_owned() });
                } else {
                    parsed_queries.push(ParsedQuery { kind: QueryType::Include, query: f });
                }
            }
        }
        *self.parsed_queries.borrow_mut() = parsed_queries;
    }
}

impl ITreeFilter<ReplElement> for ReplFilter {
    fn filter(&self, element: &ReplElement, parent_visibility: TreeVisibility) -> TreeFilterResult<()> {
        let is_visible_kind = match element {
            ReplElement::Input(_) | ReplElement::Result(_) => true,
            ReplElement::Expression(expression) => expression.is_variable(),
            _ => false,
        };
        if is_visible_kind {
            // Only filter the output events, everything else is visible https://github.com/microsoft/vscode/issues/105863
            return TreeFilterResult::Visibility(TreeVisibility::Visible);
        }

        let mut include_query_present = false;
        let mut include_query_matched = false;

        let text = element.to_display_string(true);

        for ParsedQuery { kind, query } in self.parsed_queries.borrow().iter() {
            if *kind == QueryType::Exclude && Self::match_query(query, &text) {
                // If exclude query matches, ignore all other queries and hide
                return TreeFilterResult::Bool(false);
            } else if *kind == QueryType::Include {
                include_query_present = true;
                if Self::match_query(query, &text) {
                    include_query_matched = true;
                }
            }
        }

        if include_query_present { TreeFilterResult::Bool(include_query_matched) } else { TreeFilterResult::Visibility(parent_visibility) }
    }
}
