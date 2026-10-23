//! Porte de `src/vs/editor/contrib/folding/browser/indentRangeProvider.ts`: o provedor de regiões de
//! dobra por indentação (`IndentRangeProvider`), o coletor `RangesCollector` e `computeRanges`.
//!
//! Decisões de tradução:
//! - O parâmetro `foldingRangesLimit` de `computeRanges`, com valor padrão no original, é um `Option`:
//!   `None` usa [`DefaultFoldingLimitReporter`] (`foldingRangesLimitDefault`).
//! - O `RegExp` que une os marcadores (`(start)|(?:end)`, com as flags de `start`) é recriado com
//!   [`JsRegExp`]; um padrão inválido entra em pânico, como o `new RegExp` lançaria.
//! - `!!m[1]` (o grupo de início casou com texto não vazio) é testado pelo tamanho do grupo, porque a
//!   string vazia é falsa em JavaScript.
//! - `_indentOccurrences` é um `Vec` preenchido com zero; as posições vazias do array esparso do
//!   original (`if (n)`) são os zeros.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use zyarr::Match;

use super::folding_ranges::{FoldingRegions, MAX_LINE_NUMBER};
use super::{FoldingLimitReporter, RangeProvider};
use crate::js_reg_exp::JsRegExp;
use crate::language_configuration::{FoldingMarkers, reg_exp_test};
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::model_utils::compute_indent_level;
use crate::text_model::TextModel;

const MAX_FOLDING_REGIONS_FOR_INDENT_DEFAULT: usize = 5000;

const ID_INDENT_PROVIDER: &str = "indent";

/// `IndentRangeProvider`.
pub struct IndentRangeProvider {
    editor_model: Rc<TextModel>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    folding_ranges_limit: Rc<dyn FoldingLimitReporter>,
}

impl IndentRangeProvider {
    pub fn new(
        editor_model: Rc<TextModel>,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
        folding_ranges_limit: Rc<dyn FoldingLimitReporter>,
    ) -> Self {
        Self {
            editor_model,
            language_configuration_service,
            folding_ranges_limit,
        }
    }
}

impl RangeProvider for IndentRangeProvider {
    fn id(&self) -> &str {
        ID_INDENT_PROVIDER
    }

    fn dispose(&self) {}

    fn compute(&self, _cancellation_token: &CancellationToken) -> Promise<Option<Rc<FoldingRegions>>> {
        let configuration = self
            .language_configuration_service
            .get_language_configuration(&self.editor_model.get_language_id());
        let folding_rules = &configuration.folding_rules;
        let off_side = folding_rules.off_side.unwrap_or(false);
        let markers = folding_rules.markers.as_ref();
        Promise::resolved(Some(Rc::new(compute_ranges(
            &self.editor_model,
            off_side,
            markers,
            Some(&*self.folding_ranges_limit),
        ))))
    }
}

/// `RangesCollector` (público só para testes no original).
pub struct RangesCollector<'a> {
    start_indexes: Vec<u32>,
    end_indexes: Vec<u32>,
    indent_occurrences: Vec<usize>,
    length: usize,
    folding_ranges_limit: &'a dyn FoldingLimitReporter,
}

impl<'a> RangesCollector<'a> {
    pub fn new(folding_ranges_limit: &'a dyn FoldingLimitReporter) -> Self {
        Self {
            start_indexes: Vec::new(),
            end_indexes: Vec::new(),
            indent_occurrences: Vec::new(),
            length: 0,
            folding_ranges_limit,
        }
    }

    pub fn insert_first(&mut self, start_line_number: u32, end_line_number: u32, indent: i32) {
        if start_line_number > MAX_LINE_NUMBER || end_line_number > MAX_LINE_NUMBER {
            return;
        }
        self.start_indexes.push(start_line_number);
        self.end_indexes.push(end_line_number);
        self.length += 1;
        if indent < 1000 {
            let indent = indent as usize;
            if self.indent_occurrences.len() <= indent {
                self.indent_occurrences.resize(indent + 1, 0);
            }
            self.indent_occurrences[indent] += 1;
        }
    }

    pub fn to_indent_ranges(&self, model: &TextModel) -> FoldingRegions {
        let limit = self.folding_ranges_limit.limit();
        if self.length <= limit {
            self.folding_ranges_limit.update(self.length, None);

            // reverse and create arrays of the exact length
            let start_indexes: Vec<u32> = self.start_indexes.iter().rev().copied().collect();
            let end_indexes: Vec<u32> = self.end_indexes.iter().rev().copied().collect();
            FoldingRegions::new(start_indexes, end_indexes, None)
        } else {
            self.folding_ranges_limit.update(self.length, Some(limit));

            let mut entries = 0usize;
            let mut max_indent = self.indent_occurrences.len();
            for (i, &n) in self.indent_occurrences.iter().enumerate() {
                if n != 0 {
                    if n + entries > limit {
                        max_indent = i;
                        break;
                    }
                    entries += n;
                }
            }
            let tab_size = model.get_options().tab_size;
            // reverse and create arrays of the exact length
            let mut start_indexes = vec![0u32; limit];
            let mut end_indexes = vec![0u32; limit];
            let mut k = 0usize;
            for i in (0..self.length).rev() {
                let start_index = self.start_indexes[i];
                let line_content = model.get_line_content(start_index as i32);
                let indent = i64::from(compute_indent_level(line_content.units(), tab_size));
                let fits = indent < max_indent as i64
                    || (indent == max_indent as i64 && {
                        let within_limit = entries < limit;
                        entries += 1;
                        within_limit
                    });
                if fits {
                    // Escrever fora do `Uint32Array` é ignorado no original.
                    if k < limit {
                        start_indexes[k] = start_index;
                        end_indexes[k] = self.end_indexes[i];
                    }
                    k += 1;
                }
            }
            FoldingRegions::new(start_indexes, end_indexes, None)
        }
    }
}

/// `PreviousRegion`.
struct PreviousRegion {
    /// indent or -2 if a marker
    indent: i32,
    /// end line number for the region above
    end_above: i32,
    /// start line of the region. Only used for marker regions.
    line: i32,
}

/// `foldingRangesLimitDefault`.
pub struct DefaultFoldingLimitReporter;

impl FoldingLimitReporter for DefaultFoldingLimitReporter {
    fn limit(&self) -> usize {
        MAX_FOLDING_REGIONS_FOR_INDENT_DEFAULT
    }

    fn update(&self, _computed: usize, _limited: Option<usize>) {}
}

/// `!!m[1]`: o grupo 1 casou com texto não vazio.
fn start_group_matched(m: &Match) -> bool {
    m.captures
        .get(1)
        .and_then(|capture| capture.as_ref())
        .is_some_and(|range| !range.is_empty())
}

/// `computeRanges(model, offSide, markers, foldingRangesLimit)`.
pub fn compute_ranges(
    model: &TextModel,
    off_side: bool,
    markers: Option<&FoldingMarkers>,
    folding_ranges_limit: Option<&dyn FoldingLimitReporter>,
) -> FoldingRegions {
    let tab_size = model.get_options().tab_size;
    let default_limit = DefaultFoldingLimitReporter;
    let mut result = RangesCollector::new(folding_ranges_limit.unwrap_or(&default_limit));

    let mut pattern: Option<JsRegExp> = None;
    let mut start_pattern: Option<Rc<JsRegExp>> = None;
    let mut end_pattern: Option<Rc<JsRegExp>> = None;
    if let Some(markers) = markers {
        if markers.start.flags() == markers.end.flags() {
            let mut source: Vec<u16> = "(".encode_utf16().collect();
            source.extend_from_slice(markers.start.source());
            source.extend(")|(?:".encode_utf16());
            source.extend_from_slice(markers.end.source());
            source.extend(")".encode_utf16());
            pattern = Some(
                JsRegExp::new(&source, &markers.start.flags().to_flags_string())
                    .expect("os marcadores de dobra formam uma expressão regular válida"),
            );
        } else {
            start_pattern = Some(markers.start.clone());
            end_pattern = Some(markers.end.clone());
        }
    }

    let mut previous_regions: Vec<PreviousRegion> = Vec::new();
    let line_count = model.get_line_count();
    let sentinel_line = line_count + 1;
    // sentinel, to make sure there's at least one entry
    previous_regions.push(PreviousRegion {
        indent: -1,
        end_above: sentinel_line,
        line: sentinel_line,
    });

    for line in (1..=line_count).rev() {
        let line_content = model.get_line_content(line);
        let units = line_content.units();
        let indent = compute_indent_level(units, tab_size);
        let mut previous = previous_regions.len() - 1;
        if indent == -1 {
            if off_side {
                // for offSide languages, empty lines are associated to the previous block
                // note: the next block is already written to the results, so this only
                // impacts the end position of the block before
                previous_regions[previous].end_above = line;
            }
            continue; // only whitespace
        }
        let mut is_start_match = false;
        let mut is_end_match = false;
        if let Some(pattern) = &pattern {
            pattern.set_last_index(0);
            if let Some(m) = pattern.exec(units) {
                is_start_match = start_group_matched(&m);
                is_end_match = !is_start_match;
            }
        } else {
            if let Some(start_pattern) = &start_pattern {
                start_pattern.set_last_index(0);
                is_start_match = reg_exp_test(start_pattern, units);
            }
            if !is_start_match && let Some(end_pattern) = &end_pattern {
                end_pattern.set_last_index(0);
                is_end_match = reg_exp_test(end_pattern, units);
            }
        }
        if is_start_match || is_end_match {
            // folding pattern match
            if is_start_match {
                // start pattern match
                // discard all regions until the folding pattern
                let mut i = previous_regions.len() - 1;
                while i > 0 && previous_regions[i].indent != -2 {
                    i -= 1;
                }
                if i > 0 {
                    previous_regions.truncate(i + 1);
                    previous = i;

                    // new folding range from pattern, includes the end line
                    result.insert_first(line as u32, previous_regions[previous].line as u32, indent);
                    let region = &mut previous_regions[previous];
                    region.line = line;
                    region.indent = indent;
                    region.end_above = line;
                    continue;
                }
                // no end marker found, treat line as a regular line
            } else {
                // end pattern match
                previous_regions.push(PreviousRegion {
                    indent: -2,
                    end_above: line,
                    line,
                });
                continue;
            }
        }
        if previous_regions[previous].indent > indent {
            // discard all regions with larger indent
            loop {
                previous_regions.pop();
                previous = previous_regions.len() - 1;
                if previous_regions[previous].indent <= indent {
                    break;
                }
            }

            // new folding range
            let end_line_number = previous_regions[previous].end_above - 1;
            if end_line_number - line >= 1 {
                // needs at east size 1
                result.insert_first(line as u32, end_line_number as u32, indent);
            }
        }
        if previous_regions[previous].indent == indent {
            previous_regions[previous].end_above = line;
        } else {
            // previous.indent < indent
            // new region with a bigger indent
            previous_regions.push(PreviousRegion {
                indent,
                end_above: line,
                line,
            });
        }
    }
    result.to_indent_ranges(model)
}
