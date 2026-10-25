//! Porte de `src/vs/workbench/contrib/mergeEditor/browser/model/diffComputer.ts`.
//!
//! Notas de tradução:
//! - `ITextModel` é o `Rc<TextModel>` do je-editor; `Promise<X>` é a `Promise<X>` do je-base, e o `throw new Error(...)`
//!   de dentro do `async` é a rejeição dela.
//! - `observableConfigValue<'smart' | 'experimental' | 'legacy' | 'advanced'>(...)` lê o texto da configuração (o
//!   `?? defaultValue` vale também para um valor que não é texto); o `.map` normaliza `smart` para `legacy` e
//!   `experimental` para `advanced`, e o nome vira o [`DiffAlgorithmName`] na chamada do serviço.
//! - O `@IEditorWorkerService` e o `@IConfigurationService` chegam por `new`.
//! - `assertFn` roda só onde o `assert_fn` do je-base roda (como no original).
//! - `c.innerChanges?.map(...)` é o `inner_changes` opcional do `DetailedLineRangeMapping` do je-editor.

use std::rc::Rc;

use je_base::assert::{assert_fn, check_adjacent_items};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::observable::{IObservable, IReader, Observable, ObservableExt};
use je_base::promise::Promise;
use je_editor::diff::range_mapping::RangeMapping as DiffRangeMapping;
use je_editor::editor_worker::{DiffAlgorithmName, IEditorWorkerService};
use je_editor::line_range::LineRange;
use je_editor::text_model::TextModel;
use je_platform::configuration::service::IConfigurationService;
use je_platform::observable::platform_observable_utils::observable_config_value;

use super::line_range::MergeEditorLineRange;
use super::mapping::{DetailedLineRangeMapping, RangeMapping};

pub trait IMergeDiffComputer {
    fn compute_diff(&self, text_model1: &Rc<TextModel>, text_model2: &Rc<TextModel>, reader: &dyn IReader) -> Promise<MergeDiffComputerResult>;
}

#[derive(Clone, Debug)]
pub struct MergeDiffComputerResult {
    pub diffs: Option<Vec<DetailedLineRangeMapping>>,
}

pub struct MergeDiffComputer {
    merge_algorithm: IObservable<String>,
    editor_worker_service: Rc<dyn IEditorWorkerService>,
}

impl MergeDiffComputer {
    pub fn new(editor_worker_service: Rc<dyn IEditorWorkerService>, configuration_service: Rc<dyn IConfigurationService>) -> Self {
        let merge_algorithm = observable_config_value::<String>("mergeEditor.diffAlgorithm", "advanced".to_string(), configuration_service, |value| match value {
            Dynamic::String(s) => Some(s.to_string_lossy()),
            _ => None,
        })
        .map(|v, _| match v.as_str() {
            "smart" => "legacy".to_string(),
            "experimental" => "advanced".to_string(),
            _ => v,
        });
        Self { merge_algorithm, editor_worker_service }
    }
}

impl IMergeDiffComputer for MergeDiffComputer {
    fn compute_diff(&self, text_model1: &Rc<TextModel>, text_model2: &Rc<TextModel>, reader: &dyn IReader) -> Promise<MergeDiffComputerResult> {
        let diff_algorithm = match self.merge_algorithm.read(reader).as_str() {
            "legacy" => DiffAlgorithmName::Legacy,
            "advanced-external" => DiffAlgorithmName::AdvancedExternal,
            "advanced-wasm" => DiffAlgorithmName::AdvancedWasm,
            _ => DiffAlgorithmName::Advanced,
        };
        let input_version = text_model1.get_version_id();
        let output_version = text_model2.get_version_id();

        let result = self.editor_worker_service.compute_diff(
            &text_model1.uri(),
            &text_model2.uri(),
            &je_editor::diff::lines_diff_computer::LinesDiffComputerOptions {
                ignore_trim_whitespace: false,
                max_computation_time_ms: 0.0,
                compute_moves: false,
                extend_to_subwords: None,
            },
            diff_algorithm,
        );

        let text_model1 = text_model1.clone();
        let text_model2 = text_model2.clone();
        result.then_result(move |result| {
            let Some(result) = result? else {
                return Err(Error::new("Diff computation failed"));
            };

            if text_model1.is_disposed() || text_model2.is_disposed() {
                return Ok(MergeDiffComputerResult { diffs: None });
            }

            let changes: Vec<DetailedLineRangeMapping> = result
                .changes
                .iter()
                .map(|c| {
                    DetailedLineRangeMapping::new(
                        to_line_range(&c.original),
                        text_model1.clone(),
                        to_line_range(&c.modified),
                        text_model2.clone(),
                        c.inner_changes.as_ref().map(|inner| inner.iter().map(to_range_mapping).collect()),
                    )
                })
                .collect();

            let new_input_version = text_model1.get_version_id();
            let new_output_version = text_model2.get_version_id();

            if input_version != new_input_version || output_version != new_output_version {
                return Ok(MergeDiffComputerResult { diffs: None });
            }

            assert_fn(|| {
                // O bloco comentado do original (validação dos mapeamentos internos) não vale, ver
                // https://github.com/microsoft/vscode-copilot/issues/10610.
                changes.is_empty()
                    || (changes[0].input_range().start_line_number == changes[0].output_range().start_line_number
                        && check_adjacent_items(&changes, |m1, m2| {
                            m2.input_range().start_line_number - m1.input_range().end_line_number_exclusive
                                == m2.output_range().start_line_number - m1.output_range().end_line_number_exclusive
                                // There has to be an unchanged line in between (otherwise both diffs should have been joined)
                                && m1.input_range().end_line_number_exclusive < m2.input_range().start_line_number
                                && m1.output_range().end_line_number_exclusive < m2.output_range().start_line_number
                        }))
            });

            Ok(MergeDiffComputerResult { diffs: Some(changes) })
        })
    }
}

pub fn to_line_range(range: &LineRange) -> MergeEditorLineRange {
    MergeEditorLineRange::from_length(range.start_line_number, range.length())
}

pub fn to_range_mapping(mapping: &DiffRangeMapping) -> RangeMapping {
    RangeMapping::new(mapping.original_range, mapping.modified_range)
}
