//! Porte de `src/vs/workbench/contrib/notebook/browser/diff/editorHeightCalculator.ts`: o cálculo da altura de um editor de
//! diff de célula (`IDiffEditorHeightCalculatorService` e `DiffEditorHeightCalculatorService`).
//!
//! Divergências:
//! - `DiffEditorHeightCalculatorService` não declara `implements` no original, mas o resto do código o usa como
//!   `IDiffEditorHeightCalculatorService` (tipagem estrutural); aqui ele implementa o trait.
//! - `diffAndComputeHeight` (`async`, com `try`/`finally` que descarta as duas referências) é uma `async_fn` do je-base; o
//!   `finally` roda depois de qualquer resultado, inclusive o erro.
//! - `IReference<IResolvedTextEditorModel>` é o [`TextModelReference`]; `object.textEditorModel` é
//!   `ITextEditorModel::text_editor_model` (o modelo resolvido nunca é nulo aqui).
//! - `getValue<number>(...) ?? 3` é a leitura do [`Dynamic`] como número, com 3 quando ausente.
//! - As contagens de linha são `i64`, e a altura é `f64` (o `number` do original).

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::diff::document_diff_provider::DocumentDiffProviderOptions;
use je_editor::diff_editor_view_model::UnchangedRegion;
use je_editor::editor_worker::{DiffAlgorithmName, IEditorWorkerService};
use je_platform::configuration::service::IConfigurationService;

use super::diff_cell_editor_options::get_editor_padding;
use super::diff_element_view_model::HEIGHT_OF_HIDDEN_LINES_REGION_IN_DIFF_EDITOR;
use super::notebook_diff_editor_browser::config_flag;
use crate::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService};

/// `IDiffEditorHeightCalculatorService`.
pub trait IDiffEditorHeightCalculatorService {
    fn diff_and_compute_height(&self, original: &URI, modified: &URI) -> Promise<f64>;
    fn compute_height_from_lines(&self, line_count: i64) -> f64;
}

/// `DiffEditorHeightCalculatorService`.
pub struct DiffEditorHeightCalculatorService {
    line_height: f64,
    text_model_resolver_service: Rc<dyn ITextModelService>,
    editor_worker_service: Rc<dyn IEditorWorkerService>,
    configuration_service: Rc<dyn IConfigurationService>,
}

impl DiffEditorHeightCalculatorService {
    pub fn new(
        line_height: f64,
        text_model_resolver_service: Rc<dyn ITextModelService>,
        editor_worker_service: Rc<dyn IEditorWorkerService>,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        Rc::new(Self { line_height, text_model_resolver_service, editor_worker_service, configuration_service })
    }
}

/// `getValue<number>(key)`: `None` quando ausente.
fn config_number(value: &Dynamic) -> Option<i32> {
    match value {
        Dynamic::Number(number) => Some(*number as i32),
        _ => None,
    }
}

impl IDiffEditorHeightCalculatorService for DiffEditorHeightCalculatorService {
    fn diff_and_compute_height(&self, original: &URI, modified: &URI) -> Promise<f64> {
        let original = original.clone();
        let modified = modified.clone();
        let line_height = self.line_height;
        let text_model_resolver_service = self.text_model_resolver_service.clone();
        let editor_worker_service = self.editor_worker_service.clone();
        let configuration_service = self.configuration_service.clone();
        async_fn(async move {
            let references = Promise::all(vec![
                text_model_resolver_service.create_model_reference(&original),
                text_model_resolver_service.create_model_reference(&modified),
            ])
            .await?;
            let original_model = references[0].clone();
            let modified_model = references[1].clone();

            let result = async {
                let options = DocumentDiffProviderOptions {
                    ignore_trim_whitespace: true,
                    max_computation_time_ms: 0.0,
                    compute_moves: false,
                    extend_to_subwords: None,
                };
                let diff_changes = editor_worker_service
                    .compute_diff(&original, &modified, &options, DiffAlgorithmName::Advanced)
                    .await?
                    .map(|diff| diff.changes)
                    .unwrap_or_default();

                let unchanged_region_feature_enabled = config_flag(&configuration_service.get_value(Some("diffEditor.hideUnchangedRegions.enabled"), None));
                let minimum_line_count = config_number(&configuration_service.get_value(Some("diffEditor.hideUnchangedRegions.minimumLineCount"), None));
                let context_line_count = config_number(&configuration_service.get_value(Some("diffEditor.hideUnchangedRegions.contextLineCount"), None));
                let original_line_count =
                    original_model.object.text_editor_model().expect("textEditorModel").get_line_count();
                let modified_line_count =
                    modified_model.object.text_editor_model().expect("textEditorModel").get_line_count();
                let unchanged = if unchanged_region_feature_enabled {
                    UnchangedRegion::from_diffs(
                        &diff_changes,
                        original_line_count,
                        modified_line_count,
                        minimum_line_count.unwrap_or(3),
                        context_line_count.unwrap_or(3),
                    )
                } else {
                    Vec::new()
                };

                let number_of_new_lines = diff_changes.iter().fold(0_i64, |prev, curr| {
                    if curr.original.is_empty() && !curr.modified.is_empty() {
                        return prev + i64::from(curr.modified.length());
                    }
                    if !curr.original.is_empty() && !curr.modified.is_empty() && curr.modified.length() > curr.original.length() {
                        return prev + i64::from(curr.modified.length()) - i64::from(curr.original.length());
                    }
                    prev
                });
                let orginal_number_of_lines = i64::from(original_model.object.text_editor_model().expect("textEditorModel").get_line_count());
                let number_of_hidden_lines: i64 = unchanged.iter().map(|region| i64::from(region.line_count)).sum();
                let number_of_hidden_sections = unchanged.len() as f64;
                let unchange_regions_height = number_of_hidden_sections * HEIGHT_OF_HIDDEN_LINES_REGION_IN_DIFF_EDITOR;
                let visible_line_count = orginal_number_of_lines + number_of_new_lines - number_of_hidden_lines;

                // Nota do original: quando houver barra de rolagem horizontal, é preciso somar 12 à altura. Hoje não há como
                // saber se a barra está visível no editor.
                let padding = get_editor_padding(visible_line_count);
                Ok::<f64, Error>(visible_line_count as f64 * line_height + padding.top as f64 + padding.bottom as f64 + unchange_regions_height)
            }
            .await;

            original_model.dispose();
            modified_model.dispose();
            result
        })
    }

    fn compute_height_from_lines(&self, line_count: i64) -> f64 {
        let padding = get_editor_padding(line_count);
        line_count as f64 * self.line_height + padding.top as f64 + padding.bottom as f64
    }
}
