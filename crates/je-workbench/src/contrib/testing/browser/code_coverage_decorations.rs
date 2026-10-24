//! Porte de `src/vs/workbench/contrib/testing/browser/codeCoverageDecorations.ts`.
//!
//! Recorte: só o `CoverageDetailsModel` (a geração dos intervalos sequenciais e sem sobreposição a partir dos detalhes
//! de cobertura, mais a descrição em markdown de cada um) e os auxiliares que ele usa (`namedDetailLabel`,
//! `tidyLocation`, `wrapInBackticks`, `wrapName`). Ficam de fora, por dependerem de editor, DOM e widgets:
//! `CodeCoverageDecorations` (a contribuição do editor), `CoverageToolbarWidget`, `ActionWithIcon`,
//! `CodiconActionViewItem`, as ações registradas e o `countBadge`.
//!
//! Diferenças de forma:
//! - o `ITextModel` do construtor só serve a `getValueInRange`, então vira o parâmetro `get_value_in_range`;
//! - `CoverageDetailsWithBranch` é [`CoverageDetailsWithBranch`], com a variante `Branch { branch, detail }`;
//! - os números das mensagens saem com `number_to_js_string`, como a interpolação do `localize` do original;
//! - `slice(0, 40)` e `length > 50` contam unidades UTF-16, como em JS.

use je_base::dynamic::number_to_js_string;
use je_base::html_content::MarkdownString;
use je_base::js_string::JsString;
use je_base::nls::localize;
use je_editor::position::Position;
use je_editor::range::Range;

use crate::contrib::testing::common::test_types::{
    CoverageCountValue, CoverageDetails, CoverageLocation, DetailType, IStatementCoverage,
};

/// `CoverageDetails | { type: DetailType.Branch; branch: number; detail: IStatementCoverage }`.
#[derive(Clone, Debug)]
pub enum CoverageDetailsWithBranch {
    Detail(CoverageDetails),
    Branch { branch: usize, detail: IStatementCoverage },
}

impl CoverageDetailsWithBranch {
    pub fn detail_type(&self) -> DetailType {
        match self {
            CoverageDetailsWithBranch::Detail(detail) => detail.detail_type(),
            CoverageDetailsWithBranch::Branch { .. } => DetailType::Branch,
        }
    }
}

/// O `metadata` de um [`DetailRange`].
#[derive(Clone, Debug)]
pub struct DetailRangeMetadata {
    pub detail: CoverageDetailsWithBranch,
    pub description: Option<MarkdownString>,
}

/// `DetailRange`.
#[derive(Clone, Debug)]
pub struct DetailRange {
    pub range: Range,
    pub primary: bool,
    pub metadata: DetailRangeMetadata,
}

pub struct CoverageDetailsModel {
    pub ranges: Vec<DetailRange>,
    pub details: Vec<CoverageDetails>,
}

fn detail_location(detail: &CoverageDetails) -> CoverageLocation {
    match detail {
        CoverageDetails::Declaration(declaration) => declaration.location,
        CoverageDetails::Statement(statement) => statement.location,
    }
}

impl CoverageDetailsModel {
    pub fn new(details: Vec<CoverageDetails>, get_value_in_range: &dyn Fn(&Range) -> String) -> CoverageDetailsModel {
        // Coverage from a provider can have a range that contains smaller ranges, such as a function declaration that
        // has nested statements. Aqui se fazem intervalos sequenciais e sem sobreposição para cada detalhe.
        let mut detail_ranges: Vec<DetailRange> = details
            .iter()
            .map(|detail| {
                let with_branch = CoverageDetailsWithBranch::Detail(detail.clone());
                DetailRange {
                    range: tidy_location(detail_location(detail)),
                    primary: true,
                    metadata: DetailRangeMetadata {
                        description: describe(&with_branch, get_value_in_range),
                        detail: with_branch,
                    },
                }
            })
            .collect();

        let initial = detail_ranges.len();
        for index in 0..initial {
            let range = detail_ranges[index].range;
            let statement = match &detail_ranges[index].metadata.detail {
                CoverageDetailsWithBranch::Detail(CoverageDetails::Statement(statement)) if statement.branches.is_some() => statement.clone(),
                _ => continue,
            };
            for (branch_index, branch_coverage) in statement.branches.iter().flatten().enumerate() {
                let branch = CoverageDetailsWithBranch::Branch { branch: branch_index, detail: statement.clone() };
                let location = branch_coverage.location.unwrap_or(CoverageLocation::Range(Range::from_positions(
                    range.get_end_position(),
                    range.get_end_position(),
                )));
                detail_ranges.push(DetailRange {
                    range: tidy_location(location),
                    primary: true,
                    metadata: DetailRangeMetadata { description: describe(&branch, get_value_in_range), detail: branch },
                });
            }
        }

        // type ordering is done so that function declarations come first on a tie so that single-statement functions
        // (`() => foo()` for example) get inline decorations.
        detail_ranges.sort_by(|a, b| {
            let by_start = Range::compare_ranges_using_starts(Some(&a.range), Some(&b.range));
            let order = if by_start != 0 {
                by_start
            } else {
                a.metadata.detail.detail_type() as i32 - b.metadata.detail.detail_type() as i32
            };
            order.cmp(&0)
        });

        let mut stack: Vec<DetailRange> = Vec::new();
        let mut result: Vec<DetailRange> = Vec::new();

        fn pop(stack: &mut Vec<DetailRange>, result: &mut Vec<DetailRange>) {
            let next = stack.pop().expect("a pilha não está vazia");
            if let Some(prev) = stack.last_mut() {
                prev.range = prev.range.set_start_position(next.range.end_line_number, next.range.end_column);
            }
            result.push(next);
        }

        for item in detail_ranges {
            // 1. Ensure that any ranges in the stack that ended before this are flushed
            let start = item.range.get_start_position();
            while stack.last().is_some_and(|top| !top.range.contains_position(&start)) {
                pop(&mut stack, &mut result);
            }

            // Empty ranges (usually representing missing branches) can be added without worry about overlay.
            if item.range.is_empty() {
                result.push(item);
                continue;
            }

            // 2. Take the last (overlapping) item in the stack, push range before the `item.range` into the result and
            // modify its stack to push the start until after the `item.range` ends.
            if let Some(prev) = stack.last_mut() {
                let primary = prev.primary;
                let si = prev.range.set_end_position(start.line_number, start.column);
                prev.range = prev.range.set_start_position(item.range.end_line_number, item.range.end_column);
                prev.primary = false;
                let metadata = prev.metadata.clone();
                // discard the previous range if it became empty, e.g. a nested statement
                if prev.range.is_empty() {
                    stack.pop();
                }
                result.push(DetailRange { range: si, primary, metadata });
            }

            stack.push(item);
        }
        while !stack.is_empty() {
            pop(&mut stack, &mut result);
        }

        CoverageDetailsModel { ranges: result, details }
    }

    /// Gets the markdown description for the given detail.
    pub fn describe(&self, detail: &CoverageDetailsWithBranch, get_value_in_range: &dyn Fn(&Range) -> String) -> Option<MarkdownString> {
        describe(detail, get_value_in_range)
    }
}

fn markdown(text: String) -> MarkdownString {
    let mut markdown = MarkdownString::new(None, Default::default());
    markdown.append_markdown(JsString::from(text).units());
    markdown
}

fn describe(detail: &CoverageDetailsWithBranch, get_value_in_range: &dyn Fn(&Range) -> String) -> Option<MarkdownString> {
    let empty_statement = || "<empty statement>".to_string();
    match detail {
        CoverageDetailsWithBranch::Detail(CoverageDetails::Declaration(declaration)) => {
            Some(named_detail_label(&declaration.name, declaration.count))
        }
        CoverageDetailsWithBranch::Detail(CoverageDetails::Statement(statement)) => {
            let value = get_value_in_range(&tidy_location(statement.location));
            let trimmed = value.trim();
            let text = wrap_name(if trimmed.is_empty() { &empty_statement() } else { trimmed });
            match statement.branches.as_deref().filter(|branches| !branches.is_empty()) {
                Some(branches) => {
                    let covered = branches.iter().filter(|b| b.count.is_truthy()).count();
                    Some(markdown(localize(
                        "coverage.branches",
                        "{0} of {1} of branches in {2} were covered.",
                        &[&covered.to_string(), &branches.len().to_string(), &text],
                    )))
                }
                None => Some(named_detail_label(&text, statement.count)),
            }
        }
        CoverageDetailsWithBranch::Branch { branch, detail } => {
            let value = get_value_in_range(&tidy_location(detail.location));
            let trimmed = value.trim();
            let text = wrap_name(if trimmed.is_empty() { &empty_statement() } else { trimmed });
            let covered = &detail.branches.as_ref().expect("um ramo vem de um statement com branches")[*branch];
            let label = match covered.label.as_deref().filter(|label| !label.is_empty()) {
                Some(label) => wrap_in_backticks(label),
                None => format!("#{}", branch + 1),
            };
            Some(markdown(if !covered.count.is_truthy() {
                localize("coverage.branchNotCovered", "Branch {0} in {1} was not covered.", &[&label, &text])
            } else if let CoverageCountValue::Bool(_) = covered.count {
                localize("coverage.branchCoveredYes", "Branch {0} in {1} was executed.", &[&label, &text])
            } else {
                let CoverageCountValue::Number(count) = covered.count else { unreachable!() };
                localize("coverage.branchCovered", "Branch {0} in {1} was executed {2} time(s).", &[&label, &text, &number_to_js_string(count)])
            }))
        }
    }
}

/// `namedDetailLabel`: serve a `IStatementCoverage` e `IDeclarationCoverage`, que só entram aqui pelo nome e pelo `count`.
fn named_detail_label(name: &str, count: CoverageCountValue) -> MarkdownString {
    markdown(if !count.is_truthy() {
        // 0 or false
        localize("coverage.declExecutedNo", "`{0}` was not executed.", &[name])
    } else if let CoverageCountValue::Number(count) = count {
        localize("coverage.declExecutedCount", "`{0}` was executed {1} time(s).", &[name, &number_to_js_string(count)])
    } else {
        localize("coverage.declExecutedYes", "`{0}` was executed.", &[name])
    })
}

/// 'tidies' the range by normalizing it into a range and removing leading and trailing whitespace.
fn tidy_location(location: CoverageLocation) -> Range {
    match location {
        CoverageLocation::Position(position) => {
            Range::from_positions(position, Position::new(position.line_number, 0x7FFF_FFFF))
        }
        CoverageLocation::Range(range) => range,
    }
}

fn wrap_in_backticks(text: &str) -> String {
    let stripped: String = text.chars().filter(|c| !matches!(c, '\n' | '\r' | '`')).collect();
    format!("`{stripped}`")
}

fn wrap_name(function_name_or_code: &str) -> String {
    let units: Vec<u16> = function_name_or_code.encode_utf16().collect();
    if units.len() > 50 {
        let cut = String::from_utf16_lossy(&units[..40]);
        return wrap_in_backticks(&format!("{cut}..."));
    }
    wrap_in_backticks(function_name_or_code)
}
