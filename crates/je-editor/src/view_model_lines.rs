//! Porte de `src/vs/editor/common/viewModel/viewModelLines.ts`: `IViewModelLines`,
//! `ViewModelLinesFromProjectedModel` (quebra de linha, dobra por `setHiddenAreas`, texto injetado, guias de
//! indentação e o `CoordinatesConverter`) e `ViewModelLinesFromModelAsIs` (o modelo sem projeção).
//!
//! Decisões de tradução:
//! - `IViewModelLines` é um trait com `&self`, como `IViewModel` em `view_model_types`: o original muta o
//!   estado por trás de uma interface compartilhada, então a implementação usa mutabilidade interior
//!   (`RefCell`). Isso também deixa o conversor de coordenadas compartilhar as linhas (`Rc`), como o mesmo
//!   objeto do original. Por isso `ViewModelLinesFromProjectedModel::new` devolve `Rc<Self>`. Nenhum empréstimo
//!   do estado fica aberto durante uma chamada ao modelo que possa disparar evento.
//! - `IDisposable` é o supertrait [`IDisposable`] de `je_base::lifecycle`.
//! - `ITextModel` é o `Rc<TextModel>`; o `ISimpleModel` que a projeção lê é o próprio `TextModel`
//!   ([`SimpleModel`]).
//! - Números de linha, colunas e contagens de linhas da visão são `i32`; comprimentos de linha e índices de
//!   linha de saída (que a projeção recebe como `usize`) são `usize`. `x | 0` sobre `number` é a identidade
//!   em `i32`. `tabSize` e `wrappingColumn` são `f64`, porque a fábrica de quebras os recebe assim.
//! - `wrappingStrategy: 'simple' | 'advanced'` vira [`WrappingStrategy`] e `wordBreak` é o `WordBreak` de
//!   `model_line_projection_data`.
//! - Parâmetros opcionais com padrão (`affinity = PositionAffinity.None`, `allowZeroLineNumber = false`,
//!   `belowHiddenRanges = false`) são parâmetros obrigatórios nos métodos das linhas; o
//!   [`ICoordinatesConverter`] (que os tem como `Option`) aplica os padrões.
//! - `createLineBreaksComputer(context?)`: sem contexto, o computador usa um contexto guardado nas próprias
//!   linhas (`getLineContent` e `getLineInjectedText` do modelo, com o id do editor), porque o computador
//!   devolvido empresta o contexto durante todo o seu uso.
//! - `onModelLinesDeleted` e afins recebem `versionId: number | null` como `Option<i64>`; o `!versionId` do
//!   original (`null` ou 0) é tratado como "sem versão".
//! - O caminho lento de `getDecorationsInRange` repassa só os cinco primeiros argumentos, como o original (o
//!   `onlyMarginDecorations` fica no padrão, falso).
//! - Os `Array` de resultado do `ViewModelLinesFromModelAsIs.getViewLinesBracketGuides` (`new Array(n).fill([])`)
//!   são `n` vetores vazios.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::lifecycle::IDisposable;

use crate::config::editor_options::wrapping_indent::WrappingIndent;
use crate::config::font_info::FontInfo;
use crate::coordinates_converter::{ICoordinatesConverter, IdentityCoordinatesConverter};
use crate::cursor_common::CursorSimpleModel;
use crate::line_tokens::IViewLineTokens;
use crate::model::{ModelDecoration, ModelDeltaDecoration, PositionAffinity};
use crate::model_line_projection::{IModelLineProjection, create_model_line_projection};
use crate::model_line_projection_data::{
    InjectedText, LineBreaksComputer, LineBreaksComputerContext, LineBreaksComputerFactory, ModelLineProjectionData,
    WordBreak,
};
use crate::position::Position;
use crate::prefix_sum_computer::ConstantTimePrefixSumComputer;
use crate::range::Range;
use crate::text_model::{ModelDecorationOptions, TextModel};
use crate::text_model_events::LineInjectedText;
use crate::text_model_guides::{
    ActiveIndentGuideInfo, BracketGuideOptions, IndentGuide, IndentGuideHorizontalLine,
};
use crate::view_events::{ViewLinesChangedEvent, ViewLinesDeletedEvent, ViewLinesInsertedEvent};
use crate::view_model_types::ViewLineData;

/// `wrappingStrategy: 'simple' | 'advanced'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrappingStrategy {
    Simple,
    Advanced,
}

/// O resultado de `onModelLineChanged`: `[lineMappingChanged, linesChanged, linesInserted, linesDeleted]`.
pub type ModelLineChangedResult =
    (bool, Option<ViewLinesChangedEvent>, Option<ViewLinesInsertedEvent>, Option<ViewLinesDeletedEvent>);

/// `IViewModelLines`.
pub trait IViewModelLines: IDisposable {
    fn create_coordinates_converter(&self) -> Box<dyn ICoordinatesConverter>;

    fn set_wrapping_settings(
        &self,
        font_info: &FontInfo,
        wrapping_strategy: WrappingStrategy,
        wrapping_column: f64,
        wrapping_indent: WrappingIndent,
        word_break: WordBreak,
        use_two_cell_fullwidth_characters: bool,
    ) -> bool;
    fn set_tab_size(&self, new_tab_size: f64) -> bool;
    fn get_hidden_areas(&self) -> Vec<Range>;
    fn set_hidden_areas(&self, ranges: &[Range]) -> bool;

    fn create_line_breaks_computer<'a>(
        &'a self,
        context: Option<&'a dyn LineBreaksComputerContext>,
    ) -> Box<dyn LineBreaksComputer + 'a>;
    fn on_model_flushed(&self);
    fn on_model_lines_deleted(
        &self,
        version_id: Option<i64>,
        from_line_number: i32,
        to_line_number: i32,
    ) -> Option<ViewLinesDeletedEvent>;
    fn on_model_lines_inserted(
        &self,
        version_id: Option<i64>,
        from_line_number: i32,
        to_line_number: i32,
        line_breaks: Vec<Option<ModelLineProjectionData>>,
    ) -> Option<ViewLinesInsertedEvent>;
    fn on_model_line_changed(
        &self,
        version_id: Option<i64>,
        line_number: i32,
        line_break_data: Option<ModelLineProjectionData>,
    ) -> ModelLineChangedResult;
    fn accept_version_id(&self, version_id: i64);

    fn get_view_line_count(&self) -> i32;
    fn get_active_indent_guide(&self, view_line_number: i32, min_line_number: i32, max_line_number: i32) -> ActiveIndentGuideInfo;
    fn get_view_lines_indent_guides(&self, view_start_line_number: i32, view_end_line_number: i32) -> Vec<i32>;
    fn get_view_lines_bracket_guides(
        &self,
        start_line_number: i32,
        end_line_number: i32,
        active_position: Option<Position>,
        options: BracketGuideOptions,
    ) -> Vec<Vec<IndentGuide>>;
    fn get_view_line_content(&self, view_line_number: i32) -> JsString;
    fn get_view_line_length(&self, view_line_number: i32) -> usize;
    fn get_view_line_min_column(&self, view_line_number: i32) -> i32;
    fn get_view_line_max_column(&self, view_line_number: i32) -> i32;
    fn get_view_line_continues_with_wrapped_line(&self, view_line_number: i32) -> bool;
    fn get_view_line_data(&self, view_line_number: i32) -> ViewLineData;
    fn get_view_lines_data(&self, view_start_line_number: i32, view_end_line_number: i32, needed: &[bool]) -> Vec<Option<ViewLineData>>;

    fn get_decorations_in_range(
        &self,
        range: Range,
        owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        only_minimap_decorations: bool,
        only_margin_decorations: bool,
    ) -> Vec<ModelDecoration>;

    fn get_injected_text_at(&self, view_position: Position) -> Option<InjectedText>;

    fn normalize_position(&self, position: Position, affinity: PositionAffinity) -> Position;
    /// A coluna em que a indentação termina numa dada linha.
    fn get_line_indent_column(&self, line_number: i32) -> i32;
}

/// `ComparerRangesUsingStarts` para ordenar: `Range.compareRangesUsingStarts` como [`Ordering`].
pub(crate) fn compare_ranges_using_starts(a: &Range, b: &Range) -> Ordering {
    Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0)
}

/// O contexto padrão do computador de quebras: o texto da linha e o texto injetado do modelo.
struct ModelLineBreaksContext {
    editor_id: i32,
    model: Rc<TextModel>,
}

impl LineBreaksComputerContext for ModelLineBreaksContext {
    fn get_line_content(&self, line_number: i32) -> JsString {
        self.model.get_line_content(line_number)
    }

    fn get_line_injected_text(&self, line_number: i32) -> Option<Vec<LineInjectedText>> {
        Some(self.model.get_line_injected_text(line_number, self.editor_id))
    }
}

/// Os dados que mudam das linhas projetadas.
struct ProjectedState {
    valid_model_version_id: i64,

    font_info: FontInfo,
    tab_size: f64,
    wrapping_column: f64,
    wrapping_indent: WrappingIndent,
    word_break: WordBreak,
    wrapping_strategy: WrappingStrategy,
    use_two_cell_fullwidth_characters: bool,

    model_line_projections: Vec<Box<dyn IModelLineProjection>>,

    /// Reflete a soma das contagens de linhas de todas as linhas do modelo projetadas.
    projected_model_line_line_counts: ConstantTimePrefixSumComputer,

    hidden_areas_decoration_ids: Vec<String>,
}

/// Troca a visibilidade da projeção `index` (o `setVisible` invalida a instância atual e devolve a vigente).
fn set_projection_visible(projections: &mut [Box<dyn IModelLineProjection>], index: usize, is_visible: bool) {
    let current = std::mem::replace(&mut projections[index], create_model_line_projection(None, false));
    projections[index] = current.set_visible(is_visible);
}

/// `ViewModelLinesFromProjectedModel`.
pub struct ViewModelLinesFromProjectedModel {
    me: Weak<ViewModelLinesFromProjectedModel>,
    editor_id: i32,
    model: Rc<TextModel>,

    dom_line_breaks_computer_factory: Rc<dyn LineBreaksComputerFactory>,
    monospace_line_breaks_computer_factory: Rc<dyn LineBreaksComputerFactory>,
    wrap_on_escaped_line_feeds: bool,
    default_line_breaks_context: ModelLineBreaksContext,

    state: RefCell<ProjectedState>,
}

impl ViewModelLinesFromProjectedModel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        editor_id: i32,
        model: Rc<TextModel>,
        dom_line_breaks_computer_factory: Rc<dyn LineBreaksComputerFactory>,
        monospace_line_breaks_computer_factory: Rc<dyn LineBreaksComputerFactory>,
        font_info: FontInfo,
        tab_size: f64,
        wrapping_strategy: WrappingStrategy,
        wrapping_column: f64,
        wrapping_indent: WrappingIndent,
        word_break: WordBreak,
        wrap_on_escaped_line_feeds: bool,
        use_two_cell_fullwidth_characters: bool,
    ) -> Rc<Self> {
        let lines = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            editor_id,
            model: model.clone(),
            dom_line_breaks_computer_factory,
            monospace_line_breaks_computer_factory,
            wrap_on_escaped_line_feeds,
            default_line_breaks_context: ModelLineBreaksContext { editor_id, model },
            state: RefCell::new(ProjectedState {
                valid_model_version_id: -1,
                font_info,
                tab_size,
                wrapping_column,
                wrapping_indent,
                word_break,
                wrapping_strategy,
                use_two_cell_fullwidth_characters,
                model_line_projections: Vec::new(),
                projected_model_line_line_counts: ConstantTimePrefixSumComputer::new(Vec::new()),
                hidden_areas_decoration_ids: Vec::new(),
            }),
        });
        lines.construct_lines(true, None);
        lines
    }

    /// O id do editor dono das linhas (o dono dos textos injetados que elas leem).
    pub fn editor_id(&self) -> i32 {
        self.editor_id
    }

    /// `this.hiddenAreasDecorationIds = this.model.deltaDecorations(this.hiddenAreasDecorationIds, [])`.
    fn remove_hidden_areas_decorations(&self) {
        let old_ids = std::mem::take(&mut self.state.borrow_mut().hidden_areas_decoration_ids);
        let new_ids = self.model.delta_decorations(&old_ids, &[], 0);
        self.state.borrow_mut().hidden_areas_decoration_ids = new_ids;
    }

    /// Os intervalos das decorações das áreas escondidas, na ordem das decorações.
    fn hidden_areas_from_decorations(&self) -> Vec<Range> {
        let ids = self.state.borrow().hidden_areas_decoration_ids.clone();
        ids.iter()
            .map(|id| self.model.get_decoration_range(id).expect("a decoração da área escondida existe"))
            .collect()
    }

    fn construct_lines(&self, reset_hidden_areas: bool, mut previous_line_breaks: Option<Vec<Option<ModelLineProjectionData>>>) {
        if reset_hidden_areas {
            self.remove_hidden_areas_decorations();
        }

        let line_count = self.model.get_lines_content().len();
        let lines_breaks = {
            let mut line_breaks_computer = self.create_line_breaks_computer(None);
            for i in 0..line_count {
                let previous = previous_line_breaks.as_mut().and_then(|previous| previous[i].take());
                line_breaks_computer.add_request(i as i32 + 1, previous);
            }
            line_breaks_computer.finalize()
        };

        let mut hidden_areas = self.hidden_areas_from_decorations();
        hidden_areas.sort_by(compare_ranges_using_starts);

        let mut projections: Vec<Box<dyn IModelLineProjection>> = Vec::with_capacity(line_count);
        let mut values: Vec<u32> = Vec::with_capacity(line_count);

        let (mut hidden_area_start, mut hidden_area_end) = (1, 0);
        let mut hidden_area_idx: isize = -1;
        let next_line_number_to_update = |hidden_area_idx: isize, hidden_area_end: i32| {
            if ((hidden_area_idx + 1) as usize) < hidden_areas.len() { hidden_area_end + 1 } else { line_count as i32 + 2 }
        };
        let mut next_line_number_to_update_hidden_area = next_line_number_to_update(hidden_area_idx, hidden_area_end);

        for (i, line_break_data) in lines_breaks.into_iter().enumerate() {
            let line_number = i as i32 + 1;

            if line_number == next_line_number_to_update_hidden_area {
                hidden_area_idx += 1;
                hidden_area_start = hidden_areas[hidden_area_idx as usize].start_line_number;
                hidden_area_end = hidden_areas[hidden_area_idx as usize].end_line_number;
                next_line_number_to_update_hidden_area = next_line_number_to_update(hidden_area_idx, hidden_area_end);
            }

            let is_in_hidden_area = line_number >= hidden_area_start && line_number <= hidden_area_end;
            let line = create_model_line_projection(line_break_data, !is_in_hidden_area);
            values.push(line.get_view_line_count() as u32);
            projections.push(line);
        }

        {
            let mut state = self.state.borrow_mut();
            state.valid_model_version_id = self.model.get_version_id();
            state.model_line_projections = projections;
            state.projected_model_line_line_counts = ConstantTimePrefixSumComputer::new(values);
        }

        self.ensure_at_least_one_visible_line();
    }

    pub fn model_position_is_visible(&self, model_line_number: i32, _model_column: i32) -> bool {
        let state = self.state.borrow();
        if model_line_number < 1 || model_line_number as usize > state.model_line_projections.len() {
            // invalid arguments
            return false;
        }
        state.model_line_projections[(model_line_number - 1) as usize].is_visible()
    }

    pub fn get_model_line_view_line_count(&self, model_line_number: i32) -> i32 {
        let state = self.state.borrow();
        if model_line_number < 1 || model_line_number as usize > state.model_line_projections.len() {
            // invalid arguments
            return 1;
        }
        state.model_line_projections[(model_line_number - 1) as usize].get_view_line_count() as i32
    }

    fn ensure_at_least_one_visible_line(&self) {
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        if state.projected_model_line_line_counts.get_total_sum() == 0 && !state.model_line_projections.is_empty() {
            set_projection_visible(&mut state.model_line_projections, 0, true);
            let view_line_count = state.model_line_projections[0].get_view_line_count() as u32;
            state.projected_model_line_line_counts.set_value(0, view_line_count);
        }
    }

    fn to_valid_view_line_number(&self, view_line_number: i32) -> i32 {
        if view_line_number < 1 {
            return 1;
        }
        let view_line_count = self.get_view_line_count();
        if view_line_number > view_line_count {
            return view_line_count;
        }
        view_line_number
    }

    // #region ViewLineInfo

    /// A projeção da linha do modelo `model_line_number`, emprestada durante `f`.
    fn with_projection<R>(&self, model_line_number: i32, f: impl FnOnce(&dyn IModelLineProjection) -> R) -> R {
        let state = self.state.borrow();
        f(&*state.model_line_projections[(model_line_number - 1) as usize])
    }

    fn get_view_line_info(&self, view_line_number: i32) -> ViewLineInfo {
        let view_line_number = self.to_valid_view_line_number(view_line_number);
        let r = self.state.borrow().projected_model_line_line_counts.get_index_of(i64::from(view_line_number) - 1);
        ViewLineInfo::new(r.index as i32 + 1, r.remainder as usize)
    }

    /// A projeção da linha do modelo da linha da visão, junto das informações da linha.
    fn with_view_line<R>(&self, view_line_number: i32, f: impl FnOnce(&dyn IModelLineProjection, &ViewLineInfo) -> R) -> R {
        let info = self.get_view_line_info(view_line_number);
        self.with_projection(info.model_line_number, |line| f(line, &info))
    }

    fn get_min_column_of_view_line(&self, view_line_info: &ViewLineInfo) -> i32 {
        self.with_projection(view_line_info.model_line_number, |line| {
            line.get_view_line_min_column(&*self.model, view_line_info.model_line_number, view_line_info.model_line_wrapped_line_idx)
        })
    }

    fn get_max_column_of_view_line(&self, view_line_info: &ViewLineInfo) -> i32 {
        self.with_projection(view_line_info.model_line_number, |line| {
            line.get_view_line_max_column(&*self.model, view_line_info.model_line_number, view_line_info.model_line_wrapped_line_idx)
        })
    }

    /// A posição do modelo da coluna `view_column` da linha da visão, escolhida por `view_column_of`.
    fn model_position_of_view_line(
        &self,
        view_line_info: &ViewLineInfo,
        view_column_of: impl FnOnce(&dyn IModelLineProjection) -> i32,
    ) -> Position {
        self.with_projection(view_line_info.model_line_number, |line| {
            let column = line.get_model_column_of_view_position(view_line_info.model_line_wrapped_line_idx, view_column_of(line));
            Position::new(view_line_info.model_line_number, column)
        })
    }

    fn get_model_start_position_of_view_line(&self, view_line_info: &ViewLineInfo) -> Position {
        self.model_position_of_view_line(view_line_info, |line| {
            line.get_view_line_min_column(&*self.model, view_line_info.model_line_number, view_line_info.model_line_wrapped_line_idx)
        })
    }

    fn get_model_end_position_of_view_line(&self, view_line_info: &ViewLineInfo) -> Position {
        self.model_position_of_view_line(view_line_info, |line| {
            line.get_view_line_max_column(&*self.model, view_line_info.model_line_number, view_line_info.model_line_wrapped_line_idx)
        })
    }

    fn get_view_line_infos_grouped_by_model_ranges(
        &self,
        view_start_line_number: i32,
        view_end_line_number: i32,
    ) -> Vec<ViewLineInfoGroupedByModelRange> {
        let start_view_line = self.get_view_line_info(view_start_line_number);
        let end_view_line = self.get_view_line_info(view_end_line_number);

        let mut result: Vec<ViewLineInfoGroupedByModelRange> = Vec::new();
        let mut last_visible_model_pos: Option<Position> = Some(self.get_model_start_position_of_view_line(&start_view_line));
        let mut view_lines: Vec<ViewLineInfo> = Vec::new();

        for cur_model_line in start_view_line.model_line_number..=end_view_line.model_line_number {
            let (is_visible, view_line_count) =
                self.with_projection(cur_model_line, |line| (line.is_visible(), line.get_view_line_count()));

            if is_visible {
                let start_offset =
                    if cur_model_line == start_view_line.model_line_number { start_view_line.model_line_wrapped_line_idx } else { 0 };
                let end_offset = if cur_model_line == end_view_line.model_line_number {
                    end_view_line.model_line_wrapped_line_idx + 1
                } else {
                    view_line_count
                };

                for i in start_offset..end_offset {
                    view_lines.push(ViewLineInfo::new(cur_model_line, i));
                }
            }

            match (is_visible, last_visible_model_pos) {
                (false, Some(last_visible_model_pos_1)) => {
                    let last_visible_model_pos_2 =
                        Position::new(cur_model_line - 1, self.model.get_line_max_column(cur_model_line - 1) + 1);

                    let model_range = Range::from_positions(last_visible_model_pos_1, last_visible_model_pos_2);
                    result.push(ViewLineInfoGroupedByModelRange::new(model_range, std::mem::take(&mut view_lines)));

                    last_visible_model_pos = None;
                }
                (true, None) => last_visible_model_pos = Some(Position::new(cur_model_line, 1)),
                _ => {}
            }
        }

        if let Some(last_visible_model_pos) = last_visible_model_pos {
            let model_range =
                Range::from_positions(last_visible_model_pos, self.get_model_end_position_of_view_line(&end_view_line));
            result.push(ViewLineInfoGroupedByModelRange::new(model_range, view_lines));
        }

        result
    }

    // #endregion

    /// O mapeamento de um guia de colchetes do modelo para a linha da visão (`undefined` do original vira
    /// `None`).
    fn map_bracket_guide(&self, g: &IndentGuide, view_line_info: &ViewLineInfo) -> Option<IndentGuide> {
        let wrapped_line_idx = view_line_info.model_line_wrapped_line_idx as i32;
        let view_position_of = |model_column: i32| {
            self.with_projection(view_line_info.model_line_number, |line| {
                line.get_view_position_of_model_position(0, model_column, None)
            })
        };

        if g.for_wrapped_lines_after_column != -1 {
            let p = view_position_of(g.for_wrapped_lines_after_column);
            if p.line_number >= wrapped_line_idx {
                return None;
            }
        }

        if g.for_wrapped_lines_before_or_at_column != -1 {
            let p = view_position_of(g.for_wrapped_lines_before_or_at_column);
            if p.line_number < wrapped_line_idx {
                return None;
            }
        }

        let Some(horizontal_line) = g.horizontal_line else {
            return Some(g.clone());
        };

        let mut column = -1;
        if g.column != -1 {
            let p = view_position_of(g.column);
            match p.line_number.cmp(&wrapped_line_idx) {
                Ordering::Equal => column = p.column,
                Ordering::Less => column = self.get_min_column_of_view_line(view_line_info),
                Ordering::Greater => return None,
            }
        }

        let view_position = self.convert_model_position_to_view_position(
            view_line_info.model_line_number,
            horizontal_line.end_column,
            PositionAffinity::None,
            false,
            false,
        );
        let p = view_position_of(horizontal_line.end_column);
        match p.line_number.cmp(&wrapped_line_idx) {
            Ordering::Equal => Some(IndentGuide::new(
                g.visible_column,
                column,
                g.class_name.clone(),
                Some(IndentGuideHorizontalLine { top: horizontal_line.top, end_column: view_position.column }),
                -1,
                -1,
            )),
            Ordering::Less => None,
            Ordering::Greater => {
                if g.visible_column != -1 {
                    // Don't repeat horizontal lines that use visibleColumn for unrelated lines.
                    return None;
                }
                Some(IndentGuide::new(
                    g.visible_column,
                    column,
                    g.class_name.clone(),
                    Some(IndentGuideHorizontalLine {
                        top: horizontal_line.top,
                        end_column: self.get_max_column_of_view_line(view_line_info),
                    }),
                    -1,
                    -1,
                ))
            }
        }
    }

    pub fn validate_view_position(&self, view_line_number: i32, view_column: i32, expected_model_position: Position) -> Position {
        let view_line_number = self.to_valid_view_line_number(view_line_number);

        let (model_line_number, view_column, computed_model_column) = self.with_view_line(view_line_number, |line, info| {
            let min_column = line.get_view_line_min_column(&*self.model, info.model_line_number, info.model_line_wrapped_line_idx);
            let max_column = line.get_view_line_max_column(&*self.model, info.model_line_number, info.model_line_wrapped_line_idx);
            let view_column = view_column.max(min_column).min(max_column);
            (
                info.model_line_number,
                view_column,
                line.get_model_column_of_view_position(info.model_line_wrapped_line_idx, view_column),
            )
        });
        let computed_model_position = self.model.validate_position(Position::new(model_line_number, computed_model_column));

        if computed_model_position == expected_model_position {
            return Position::new(view_line_number, view_column);
        }

        self.convert_model_position_to_view_position(
            expected_model_position.line_number,
            expected_model_position.column,
            PositionAffinity::None,
            false,
            false,
        )
    }

    pub fn validate_view_range(&self, view_range: Range, expected_model_range: Range) -> Range {
        let valid_view_start = self.validate_view_position(
            view_range.start_line_number,
            view_range.start_column,
            expected_model_range.get_start_position(),
        );
        let valid_view_end =
            self.validate_view_position(view_range.end_line_number, view_range.end_column, expected_model_range.get_end_position());
        Range::new(valid_view_start.line_number, valid_view_start.column, valid_view_end.line_number, valid_view_end.column)
    }

    pub fn convert_view_position_to_model_position(&self, view_line_number: i32, view_column: i32) -> Position {
        let info = self.get_view_line_info(view_line_number);

        let input_column = self.with_projection(info.model_line_number, |line| {
            line.get_model_column_of_view_position(info.model_line_wrapped_line_idx, view_column)
        });
        self.model.validate_position(Position::new(info.model_line_number, input_column))
    }

    pub fn convert_view_range_to_model_range(&self, view_range: Range) -> Range {
        let start = self.convert_view_position_to_model_position(view_range.start_line_number, view_range.start_column);
        let end = self.convert_view_position_to_model_position(view_range.end_line_number, view_range.end_column);
        Range::new(start.line_number, start.column, end.line_number, end.column)
    }

    pub fn convert_model_position_to_view_position(
        &self,
        model_line_number: i32,
        model_column: i32,
        affinity: PositionAffinity,
        allow_zero_line_number: bool,
        below_hidden_ranges: bool,
    ) -> Position {
        let valid_position = self.model.validate_position(Position::new(model_line_number, model_column));
        let input_line_number = valid_position.line_number;
        let input_column = valid_position.column;

        let state = self.state.borrow();
        let projections = &state.model_line_projections;

        let mut line_index = (input_line_number - 1) as usize;
        let mut line_index_changed = false;
        if below_hidden_ranges {
            while line_index < projections.len() && !projections[line_index].is_visible() {
                line_index += 1;
                line_index_changed = true;
            }
        } else {
            while line_index > 0 && !projections[line_index].is_visible() {
                line_index -= 1;
                line_index_changed = true;
            }
        }
        if line_index == 0 && !projections[line_index].is_visible() {
            // Could not reach a real line
            return Position::new(if allow_zero_line_number { 0 } else { 1 }, 1);
        }
        let delta_line_number = 1 + state.projected_model_line_line_counts.get_prefix_sum(line_index) as i32;

        if line_index_changed {
            let column = if below_hidden_ranges { 1 } else { self.model.get_line_max_column(line_index as i32 + 1) };
            projections[line_index].get_view_position_of_model_position(delta_line_number, column, Some(affinity))
        } else {
            projections[(input_line_number - 1) as usize].get_view_position_of_model_position(
                delta_line_number,
                input_column,
                Some(affinity),
            )
        }
    }

    /// `affinity`: a afinidade no caso de um intervalo vazio. Não tem efeito em intervalos não vazios.
    pub fn convert_model_range_to_view_range(&self, model_range: Range, affinity: PositionAffinity) -> Range {
        if model_range.is_empty() {
            let start = self.convert_model_position_to_view_position(
                model_range.start_line_number,
                model_range.start_column,
                affinity,
                false,
                false,
            );
            Range::from_positions(start, start)
        } else {
            let start = self.convert_model_position_to_view_position(
                model_range.start_line_number,
                model_range.start_column,
                PositionAffinity::Right,
                false,
                false,
            );
            let end = self.convert_model_position_to_view_position(
                model_range.end_line_number,
                model_range.end_column,
                PositionAffinity::Left,
                false,
                false,
            );
            Range::new(start.line_number, start.column, end.line_number, end.column)
        }
    }

    pub fn get_view_line_number_of_model_position(&self, model_line_number: i32, model_column: i32) -> i32 {
        let state = self.state.borrow();
        let projections = &state.model_line_projections;
        let mut line_index = (model_line_number - 1) as usize;
        if projections[line_index].is_visible() {
            // this model line is visible
            let delta_line_number = 1 + state.projected_model_line_line_counts.get_prefix_sum(line_index) as i32;
            return projections[line_index].get_view_line_number_of_model_position(delta_line_number, model_column);
        }

        // this model line is not visible
        while line_index > 0 && !projections[line_index].is_visible() {
            line_index -= 1;
        }
        if line_index == 0 && !projections[line_index].is_visible() {
            // Could not reach a real line
            return 1;
        }
        let delta_line_number = 1 + state.projected_model_line_line_counts.get_prefix_sum(line_index) as i32;
        projections[line_index]
            .get_view_line_number_of_model_position(delta_line_number, self.model.get_line_max_column(line_index as i32 + 1))
    }
}

impl IDisposable for ViewModelLinesFromProjectedModel {
    fn dispose(&self) {
        self.remove_hidden_areas_decorations();
    }
}

impl IViewModelLines for ViewModelLinesFromProjectedModel {
    fn create_coordinates_converter(&self) -> Box<dyn ICoordinatesConverter> {
        Box::new(CoordinatesConverter {
            lines: self.me.upgrade().expect("as linhas vivem dentro do Rc criado por `new`"),
        })
    }

    fn get_hidden_areas(&self) -> Vec<Range> {
        self.hidden_areas_from_decorations()
    }

    fn set_hidden_areas(&self, ranges: &[Range]) -> bool {
        let validated_ranges: Vec<Range> = ranges.iter().map(|r| self.model.validate_range(r)).collect();
        let new_ranges = normalize_line_ranges(validated_ranges);

        // TODO@Martin: Please stop calling this method on each model change!

        // This checks if there really was a change
        let mut old_ranges = self.hidden_areas_from_decorations();
        old_ranges.sort_by(compare_ranges_using_starts);
        if new_ranges == old_ranges {
            return false;
        }

        let empty_options = ModelDecorationOptions::empty();
        let new_decorations: Vec<ModelDeltaDecoration> = new_ranges
            .iter()
            .map(|r| ModelDeltaDecoration { range: *r, options: empty_options.clone() })
            .collect();

        let old_ids = std::mem::take(&mut self.state.borrow_mut().hidden_areas_decoration_ids);
        let new_ids = self.model.delta_decorations(&old_ids, &new_decorations, 0);

        let mut has_visible_line = false;
        {
            let mut state = self.state.borrow_mut();
            let state = &mut *state;
            state.hidden_areas_decoration_ids = new_ids;

            let hidden_areas = &new_ranges;
            let projection_count = state.model_line_projections.len();
            let next_line_number_to_update = |hidden_area_idx: isize, hidden_area_end: i32| {
                if ((hidden_area_idx + 1) as usize) < hidden_areas.len() { hidden_area_end + 1 } else { projection_count as i32 + 2 }
            };
            let (mut hidden_area_start, mut hidden_area_end) = (1, 0);
            let mut hidden_area_idx: isize = -1;
            let mut next_line_number_to_update_hidden_area = next_line_number_to_update(hidden_area_idx, hidden_area_end);

            for i in 0..projection_count {
                let line_number = i as i32 + 1;

                if line_number == next_line_number_to_update_hidden_area {
                    hidden_area_idx += 1;
                    hidden_area_start = hidden_areas[hidden_area_idx as usize].start_line_number;
                    hidden_area_end = hidden_areas[hidden_area_idx as usize].end_line_number;
                    next_line_number_to_update_hidden_area = next_line_number_to_update(hidden_area_idx, hidden_area_end);
                }

                let should_be_visible = !(line_number >= hidden_area_start && line_number <= hidden_area_end);
                if should_be_visible {
                    has_visible_line = true;
                }
                if state.model_line_projections[i].is_visible() != should_be_visible {
                    set_projection_visible(&mut state.model_line_projections, i, should_be_visible);
                    let new_output_line_count = state.model_line_projections[i].get_view_line_count() as u32;
                    state.projected_model_line_line_counts.set_value(i, new_output_line_count);
                }
            }
        }

        if !has_visible_line {
            // Cannot have everything be hidden => reveal everything!
            self.set_hidden_areas(&[]);
        }

        true
    }

    fn set_tab_size(&self, new_tab_size: f64) -> bool {
        {
            let mut state = self.state.borrow_mut();
            if state.tab_size == new_tab_size {
                return false;
            }
            state.tab_size = new_tab_size;
        }

        self.construct_lines(false, None);

        true
    }

    fn set_wrapping_settings(
        &self,
        font_info: &FontInfo,
        wrapping_strategy: WrappingStrategy,
        wrapping_column: f64,
        wrapping_indent: WrappingIndent,
        word_break: WordBreak,
        use_two_cell_fullwidth_characters: bool,
    ) -> bool {
        let previous_line_breaks = {
            let mut state = self.state.borrow_mut();
            let equal_font_info = state.font_info.equals(font_info);
            let equal_wrapping_strategy = state.wrapping_strategy == wrapping_strategy;
            let equal_wrapping_column = state.wrapping_column == wrapping_column;
            let equal_wrapping_indent = state.wrapping_indent == wrapping_indent;
            let equal_word_break = state.word_break == word_break;
            let equal_use_two_cell_fullwidth_characters = state.use_two_cell_fullwidth_characters == use_two_cell_fullwidth_characters;
            if equal_font_info
                && equal_wrapping_strategy
                && equal_wrapping_column
                && equal_wrapping_indent
                && equal_word_break
                && equal_use_two_cell_fullwidth_characters
            {
                return false;
            }

            let only_wrapping_column_changed = equal_font_info
                && equal_wrapping_strategy
                && !equal_wrapping_column
                && equal_wrapping_indent
                && equal_word_break
                && equal_use_two_cell_fullwidth_characters;

            state.font_info = font_info.clone();
            state.wrapping_strategy = wrapping_strategy;
            state.wrapping_column = wrapping_column;
            state.wrapping_indent = wrapping_indent;
            state.word_break = word_break;
            state.use_two_cell_fullwidth_characters = use_two_cell_fullwidth_characters;

            only_wrapping_column_changed.then(|| {
                state.model_line_projections.iter().map(|projection| projection.get_projection_data().cloned()).collect::<Vec<_>>()
            })
        };

        self.construct_lines(false, previous_line_breaks);

        true
    }

    fn create_line_breaks_computer<'a>(
        &'a self,
        context: Option<&'a dyn LineBreaksComputerContext>,
    ) -> Box<dyn LineBreaksComputer + 'a> {
        let state = self.state.borrow();
        let line_breaks_computer_factory = if state.wrapping_strategy == WrappingStrategy::Advanced {
            &self.dom_line_breaks_computer_factory
        } else {
            &self.monospace_line_breaks_computer_factory
        };
        let context: &'a dyn LineBreaksComputerContext = context.unwrap_or(&self.default_line_breaks_context);
        line_breaks_computer_factory.create_line_breaks_computer(
            context,
            &state.font_info,
            state.tab_size,
            state.wrapping_column,
            state.wrapping_indent,
            state.word_break,
            self.wrap_on_escaped_line_feeds,
            state.use_two_cell_fullwidth_characters,
        )
    }

    fn on_model_flushed(&self) {
        self.construct_lines(true, None);
    }

    fn on_model_lines_deleted(
        &self,
        version_id: Option<i64>,
        from_line_number: i32,
        to_line_number: i32,
    ) -> Option<ViewLinesDeletedEvent> {
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        if version_id.is_none_or(|version_id| version_id == 0 || version_id <= state.valid_model_version_id) {
            // Here we check for versionId in case the lines were reconstructed in the meantime.
            // We don't want to apply stale change events on top of a newer read model state.
            return None;
        }

        let counts = &mut state.projected_model_line_line_counts;
        let output_from_line_number =
            if from_line_number == 1 { 1 } else { counts.get_prefix_sum((from_line_number - 1) as usize) as i32 + 1 };
        let output_to_line_number = counts.get_prefix_sum(to_line_number as usize) as i32;

        let delete_count = (to_line_number - from_line_number + 1) as usize;
        let start = ((from_line_number - 1) as usize).min(state.model_line_projections.len());
        let end = (start + delete_count).min(state.model_line_projections.len());
        state.model_line_projections.drain(start..end);
        counts.remove_values((from_line_number - 1) as usize, delete_count);

        Some(ViewLinesDeletedEvent::new(output_from_line_number, output_to_line_number))
    }

    fn on_model_lines_inserted(
        &self,
        version_id: Option<i64>,
        from_line_number: i32,
        _to_line_number: i32,
        line_breaks: Vec<Option<ModelLineProjectionData>>,
    ) -> Option<ViewLinesInsertedEvent> {
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        if version_id.is_none_or(|version_id| version_id == 0 || version_id <= state.valid_model_version_id) {
            // Here we check for versionId in case the lines were reconstructed in the meantime.
            // We don't want to apply stale change events on top of a newer read model state.
            return None;
        }

        // cannot use this.getHiddenAreas() because those decorations have already seen the effect of this model change
        let is_in_hidden_area =
            from_line_number > 2 && !state.model_line_projections[(from_line_number - 2) as usize].is_visible();

        let counts = &mut state.projected_model_line_line_counts;
        let output_from_line_number =
            if from_line_number == 1 { 1 } else { counts.get_prefix_sum((from_line_number - 1) as usize) as i32 + 1 };

        let mut total_output_line_count = 0;
        let mut insert_lines: Vec<Box<dyn IModelLineProjection>> = Vec::with_capacity(line_breaks.len());
        let mut insert_prefix_sum_values: Vec<u32> = Vec::with_capacity(line_breaks.len());

        for line_break in line_breaks {
            let line = create_model_line_projection(line_break, !is_in_hidden_area);
            let output_line_count = line.get_view_line_count();
            insert_lines.push(line);

            total_output_line_count += output_line_count as i32;
            insert_prefix_sum_values.push(output_line_count as u32);
        }

        let insert_at = ((from_line_number - 1) as usize).min(state.model_line_projections.len());
        state.model_line_projections.splice(insert_at..insert_at, insert_lines);

        counts.insert_values(insert_at, &insert_prefix_sum_values);

        Some(ViewLinesInsertedEvent::new(output_from_line_number, output_from_line_number + total_output_line_count - 1))
    }

    fn on_model_line_changed(
        &self,
        version_id: Option<i64>,
        line_number: i32,
        line_break_data: Option<ModelLineProjectionData>,
    ) -> ModelLineChangedResult {
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        if version_id.is_some_and(|version_id| version_id <= state.valid_model_version_id) {
            // Here we check for versionId in case the lines were reconstructed in the meantime.
            // We don't want to apply stale change events on top of a newer read model state.
            return (false, None, None, None);
        }

        let line_index = (line_number - 1) as usize;

        let old_output_line_count = state.model_line_projections[line_index].get_view_line_count() as i32;
        let is_visible = state.model_line_projections[line_index].is_visible();
        let line = create_model_line_projection(line_break_data, is_visible);
        let new_output_line_count = line.get_view_line_count() as i32;
        state.model_line_projections[line_index] = line;

        let mut line_mapping_changed = false;
        let change_from = state.projected_model_line_line_counts.get_prefix_sum(line_index) as i32 + 1;
        let mut change_to = change_from + new_output_line_count - 1;
        let (mut insert_from, mut insert_to) = (0, -1);
        let (mut delete_from, mut delete_to) = (0, -1);

        match old_output_line_count.cmp(&new_output_line_count) {
            Ordering::Greater => {
                delete_from = change_to + 1;
                delete_to = delete_from + (old_output_line_count - new_output_line_count) - 1;
                line_mapping_changed = true;
            }
            Ordering::Less => {
                change_to = change_from + old_output_line_count - 1;
                insert_from = change_to + 1;
                insert_to = insert_from + (new_output_line_count - old_output_line_count) - 1;
                line_mapping_changed = true;
            }
            Ordering::Equal => {}
        }

        state.projected_model_line_line_counts.set_value(line_index, new_output_line_count as u32);

        let view_lines_changed_event =
            (change_from <= change_to).then(|| ViewLinesChangedEvent::new(change_from, change_to - change_from + 1));
        let view_lines_inserted_event = (insert_from <= insert_to).then(|| ViewLinesInsertedEvent::new(insert_from, insert_to));
        let view_lines_deleted_event = (delete_from <= delete_to).then(|| ViewLinesDeletedEvent::new(delete_from, delete_to));

        (line_mapping_changed, view_lines_changed_event, view_lines_inserted_event, view_lines_deleted_event)
    }

    fn accept_version_id(&self, version_id: i64) {
        self.state.borrow_mut().valid_model_version_id = version_id;
        self.ensure_at_least_one_visible_line();
    }

    fn get_view_line_count(&self) -> i32 {
        self.state.borrow().projected_model_line_line_counts.get_total_sum() as i32
    }

    fn get_active_indent_guide(&self, view_line_number: i32, min_line_number: i32, max_line_number: i32) -> ActiveIndentGuideInfo {
        let view_line_number = self.to_valid_view_line_number(view_line_number);
        let min_line_number = self.to_valid_view_line_number(min_line_number);
        let max_line_number = self.to_valid_view_line_number(max_line_number);

        let model_position = self.convert_view_position_to_model_position(view_line_number, self.get_view_line_min_column(view_line_number));
        let model_min_position = self.convert_view_position_to_model_position(min_line_number, self.get_view_line_min_column(min_line_number));
        let model_max_position = self.convert_view_position_to_model_position(max_line_number, self.get_view_line_min_column(max_line_number));
        let result = self.model.guides().get_active_indent_guide(
            model_position.line_number,
            model_min_position.line_number,
            model_max_position.line_number,
        );

        let view_start_position =
            self.convert_model_position_to_view_position(result.start_line_number, 1, PositionAffinity::None, false, false);
        let view_end_position = self.convert_model_position_to_view_position(
            result.end_line_number,
            self.model.get_line_max_column(result.end_line_number),
            PositionAffinity::None,
            false,
            false,
        );
        ActiveIndentGuideInfo {
            start_line_number: view_start_position.line_number,
            end_line_number: view_end_position.line_number,
            indent: result.indent,
        }
    }

    fn get_view_lines_bracket_guides(
        &self,
        view_start_line_number: i32,
        view_end_line_number: i32,
        active_view_position: Option<Position>,
        options: BracketGuideOptions,
    ) -> Vec<Vec<IndentGuide>> {
        let model_active_position = active_view_position
            .map(|position| self.convert_view_position_to_model_position(position.line_number, position.column));
        let mut result_per_view_line: Vec<Vec<IndentGuide>> = Vec::new();

        for group in self.get_view_line_infos_grouped_by_model_ranges(view_start_line_number, view_end_line_number) {
            let model_range_start_line_number = group.model_range.start_line_number;

            let bracket_guides_per_model_line = self.model.guides().get_lines_bracket_guides(
                model_range_start_line_number,
                group.model_range.end_line_number,
                model_active_position,
                options,
            );

            for view_line_info in &group.view_lines {
                let bracket_guides =
                    &bracket_guides_per_model_line[(view_line_info.model_line_number - model_range_start_line_number) as usize];

                // visibleColumns stay as they are (this is a bug and needs to be fixed, but it is not a regression)
                // model-columns must be converted to view-model columns.
                result_per_view_line
                    .push(bracket_guides.iter().filter_map(|g| self.map_bracket_guide(g, view_line_info)).collect());
            }
        }

        result_per_view_line
    }

    fn get_view_lines_indent_guides(&self, view_start_line_number: i32, view_end_line_number: i32) -> Vec<i32> {
        // TODO: Use the same code as in `getViewLinesBracketGuides`.
        // Future TODO: Merge with `getViewLinesBracketGuides`.
        // However, this requires more refactoring of indent guides.
        let view_start_line_number = self.to_valid_view_line_number(view_start_line_number);
        let view_end_line_number = self.to_valid_view_line_number(view_end_line_number);

        let model_start = self
            .convert_view_position_to_model_position(view_start_line_number, self.get_view_line_min_column(view_start_line_number));
        let model_end =
            self.convert_view_position_to_model_position(view_end_line_number, self.get_view_line_max_column(view_end_line_number));

        let mut result: Vec<i32> = Vec::new();
        let mut result_repeat_count: Vec<i32> = Vec::new();
        let mut result_repeat_option: Vec<IndentGuideRepeatOption> = Vec::new();
        let model_start_line_index = model_start.line_number - 1;
        let model_end_line_index = model_end.line_number - 1;

        // O número da linha onde o pedido pendente começa.
        let mut req_start: Option<i32> = None;
        for model_line_index in model_start_line_index..=model_end_line_index {
            let model_line_number = model_line_index + 1;
            let visible_line = self.with_projection(model_line_number, |line| {
                if !line.is_visible() {
                    return None;
                }
                let view_line_start_index = line.get_view_line_number_of_model_position(
                    0,
                    if model_line_index == model_start_line_index { model_start.column } else { 1 },
                );
                let view_line_end_index =
                    line.get_view_line_number_of_model_position(0, self.model.get_line_max_column(model_line_number));
                let count = view_line_end_index - view_line_start_index + 1;
                let mut option = IndentGuideRepeatOption::BlockNone;
                if count > 1
                    && line.get_view_line_min_column(&*self.model, model_line_number, view_line_end_index as usize) == 1
                {
                    // wrapped lines should block indent guides
                    option = if view_line_start_index == 0 {
                        IndentGuideRepeatOption::BlockSubsequent
                    } else {
                        IndentGuideRepeatOption::BlockAll
                    };
                }
                Some((count, option))
            });

            match visible_line {
                Some((count, option)) => {
                    result_repeat_count.push(count);
                    result_repeat_option.push(option);
                    // merge into previous request
                    req_start.get_or_insert(model_line_number);
                }
                None => {
                    // hit invisible line => flush request
                    if let Some(req_start) = req_start.take() {
                        result.extend(self.model.guides().get_lines_indent_guides(req_start, model_line_index));
                    }
                }
            }
        }

        if let Some(req_start) = req_start {
            result.extend(self.model.guides().get_lines_indent_guides(req_start, model_end.line_number));
        }

        let view_line_count = view_end_line_number - view_start_line_number + 1;
        let mut view_indents = vec![0; view_line_count.max(0) as usize];
        let mut curr_index = 0;
        for (i, indent) in result.iter().enumerate() {
            let mut value = *indent;
            let count = (view_line_count - curr_index).min(result_repeat_count[i]);
            let block_at_index = match result_repeat_option[i] {
                IndentGuideRepeatOption::BlockAll => 0,
                IndentGuideRepeatOption::BlockSubsequent => 1,
                IndentGuideRepeatOption::BlockNone => count,
            };
            for j in 0..count {
                if j == block_at_index {
                    value = 0;
                }
                view_indents[curr_index as usize] = value;
                curr_index += 1;
            }
        }
        view_indents
    }

    fn get_view_line_content(&self, view_line_number: i32) -> JsString {
        self.with_view_line(view_line_number, |line, info| {
            line.get_view_line_content(&*self.model, info.model_line_number, info.model_line_wrapped_line_idx)
        })
    }

    fn get_view_line_length(&self, view_line_number: i32) -> usize {
        self.with_view_line(view_line_number, |line, info| {
            line.get_view_line_length(&*self.model, info.model_line_number, info.model_line_wrapped_line_idx)
        })
    }

    fn get_view_line_min_column(&self, view_line_number: i32) -> i32 {
        self.get_min_column_of_view_line(&self.get_view_line_info(view_line_number))
    }

    fn get_view_line_max_column(&self, view_line_number: i32) -> i32 {
        self.get_max_column_of_view_line(&self.get_view_line_info(view_line_number))
    }

    fn get_view_line_continues_with_wrapped_line(&self, view_line_number: i32) -> bool {
        self.with_view_line(view_line_number, |line, info| {
            line.get_view_line_continues_with_wrapped_line(info.model_line_wrapped_line_idx)
        })
    }

    fn get_view_line_data(&self, view_line_number: i32) -> ViewLineData {
        let info = self.get_view_line_info(view_line_number);
        let base_view_line_number =
            self.state.borrow().projected_model_line_line_counts.get_prefix_sum((info.model_line_number - 1) as usize) as i32 + 1;
        self.with_projection(info.model_line_number, |line| {
            line.get_view_line_data(&*self.model, info.model_line_number, info.model_line_wrapped_line_idx, base_view_line_number)
        })
    }

    fn get_view_lines_data(&self, view_start_line_number: i32, view_end_line_number: i32, needed: &[bool]) -> Vec<Option<ViewLineData>> {
        let view_start_line_number = self.to_valid_view_line_number(view_start_line_number);
        let view_end_line_number = self.to_valid_view_line_number(view_end_line_number);

        let start = self.state.borrow().projected_model_line_line_counts.get_index_of(i64::from(view_start_line_number) - 1);
        let mut view_line_number = view_start_line_number;
        let start_model_line_index = start.index as i32;
        let start_remainder = start.remainder as usize;

        let mut result: Vec<Option<ViewLineData>> = Vec::new();
        for model_line_index in start_model_line_index..self.model.get_line_count() {
            let last_line = self.with_projection(model_line_index + 1, |line| {
                if !line.is_visible() {
                    return None;
                }
                let from_view_line_index = if model_line_index == start_model_line_index { start_remainder } else { 0 };
                let mut remaining_view_line_count = line.get_view_line_count() as i32 - from_view_line_index as i32;

                let mut last_line = false;
                if view_line_number + remaining_view_line_count > view_end_line_number {
                    last_line = true;
                    remaining_view_line_count = view_end_line_number - view_line_number + 1;
                }
                let base_view_line_number =
                    self.state.borrow().projected_model_line_line_counts.get_prefix_sum(model_line_index as usize) as i32 + 1;
                line.get_view_lines_data(
                    &*self.model,
                    model_line_index + 1,
                    from_view_line_index,
                    remaining_view_line_count as usize,
                    base_view_line_number,
                    (view_line_number - view_start_line_number) as usize,
                    needed,
                    &mut result,
                );

                view_line_number += remaining_view_line_count;
                Some(last_line)
            });

            if last_line == Some(true) {
                break;
            }
        }

        result
    }

    fn get_decorations_in_range(
        &self,
        range: Range,
        owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        only_minimap_decorations: bool,
        only_margin_decorations: bool,
    ) -> Vec<ModelDecoration> {
        let model_start = self.convert_view_position_to_model_position(range.start_line_number, range.start_column);
        let model_end = self.convert_view_position_to_model_position(range.end_line_number, range.end_column);

        if model_end.line_number - model_start.line_number <= range.end_line_number - range.start_line_number {
            // most likely there are no hidden lines => fast path
            // fetch decorations from column 1 to cover the case of wrapped lines that have whole line decorations at column 1
            return self.model.get_decorations_in_range(
                &Range::new(model_start.line_number, 1, model_end.line_number, model_end.column),
                owner_id,
                filter_out_validation,
                filter_font_decorations,
                only_minimap_decorations,
                only_margin_decorations,
            );
        }

        let mut result: Vec<ModelDecoration> = Vec::new();
        let model_start_line_index = model_start.line_number - 1;
        let model_end_line_index = model_end.line_number - 1;

        // O começo (linha e coluna) do pedido pendente.
        let mut req_start: Option<Position> = None;
        for model_line_index in model_start_line_index..=model_end_line_index {
            if self.with_projection(model_line_index + 1, |line| line.is_visible()) {
                // merge into previous request
                if req_start.is_none() {
                    req_start = Some(Position::new(
                        model_line_index + 1,
                        if model_line_index == model_start_line_index { model_start.column } else { 1 },
                    ));
                }
            } else if let Some(req_start) = req_start.take() {
                // hit invisible line => flush request
                let max_line_column = self.model.get_line_max_column(model_line_index);
                result.extend(self.model.get_decorations_in_range(
                    &Range::new(req_start.line_number, req_start.column, model_line_index, max_line_column),
                    owner_id,
                    filter_out_validation,
                    filter_font_decorations,
                    only_minimap_decorations,
                    false,
                ));
            }
        }

        if let Some(req_start) = req_start {
            result.extend(self.model.get_decorations_in_range(
                &Range::new(req_start.line_number, req_start.column, model_end.line_number, model_end.column),
                owner_id,
                filter_out_validation,
                filter_font_decorations,
                only_minimap_decorations,
                false,
            ));
        }

        result.sort_by(|a, b| compare_ranges_using_starts(&a.range, &b.range).then_with(|| a.id.cmp(&b.id)));

        // Eliminate duplicate decorations that might have intersected our visible ranges multiple times
        let mut final_result: Vec<ModelDecoration> = Vec::with_capacity(result.len());
        let mut prev_dec_id: Option<String> = None;
        for dec in result {
            if prev_dec_id.as_ref() == Some(&dec.id) {
                // skip
                continue;
            }
            prev_dec_id = Some(dec.id.clone());
            final_result.push(dec);
        }

        final_result
    }

    fn get_injected_text_at(&self, position: Position) -> Option<InjectedText> {
        self.with_view_line(position.line_number, |line, info| {
            line.get_injected_text_at(info.model_line_wrapped_line_idx, position.column)
        })
    }

    fn normalize_position(&self, position: Position, affinity: PositionAffinity) -> Position {
        self.with_view_line(position.line_number, |line, info| {
            line.normalize_position(info.model_line_wrapped_line_idx, position, affinity)
        })
    }

    fn get_line_indent_column(&self, line_number: i32) -> i32 {
        let info = self.get_view_line_info(line_number);
        if info.model_line_wrapped_line_idx == 0 {
            return self.model.get_line_indent_column(info.model_line_number);
        }

        // wrapped lines have no indentation.
        // We deliberately don't handle the case that indentation is wrapped
        // to avoid two view lines reporting indentation for the very same model line.
        0
    }
}

/// Overlapping unsorted ranges:
/// ```text
/// [   )      [ )       [  )
///    [    )      [       )
/// ->
/// Non overlapping sorted ranges:
/// [       )  [ ) [        )
/// ```
///
/// Note: This function only considers line information! Columns are ignored.
fn normalize_line_ranges(ranges: Vec<Range>) -> Vec<Range> {
    if ranges.is_empty() {
        return Vec::new();
    }

    let mut sorted_ranges = ranges;
    sorted_ranges.sort_by(compare_ranges_using_starts);

    let mut result: Vec<Range> = Vec::new();
    let mut current_range_start = sorted_ranges[0].start_line_number;
    let mut current_range_end = sorted_ranges[0].end_line_number;

    for range in &sorted_ranges[1..] {
        if range.start_line_number > current_range_end + 1 {
            result.push(Range::new(current_range_start, 1, current_range_end, 1));
            current_range_start = range.start_line_number;
            current_range_end = range.end_line_number;
        } else if range.end_line_number > current_range_end {
            current_range_end = range.end_line_number;
        }
    }
    result.push(Range::new(current_range_start, 1, current_range_end, 1));
    result
}

/// Represents a view line. Can be used to efficiently query more information about it.
struct ViewLineInfo {
    model_line_number: i32,
    model_line_wrapped_line_idx: usize,
}

impl ViewLineInfo {
    fn new(model_line_number: i32, model_line_wrapped_line_idx: usize) -> Self {
        Self { model_line_number, model_line_wrapped_line_idx }
    }

    #[allow(dead_code)]
    fn is_wrapped_line_continuation(&self) -> bool {
        self.model_line_wrapped_line_idx > 0
    }
}

/// A list of view lines that have a contiguous span in the model.
struct ViewLineInfoGroupedByModelRange {
    model_range: Range,
    view_lines: Vec<ViewLineInfo>,
}

impl ViewLineInfoGroupedByModelRange {
    fn new(model_range: Range, view_lines: Vec<ViewLineInfo>) -> Self {
        Self { model_range, view_lines }
    }
}

struct CoordinatesConverter {
    lines: Rc<ViewModelLinesFromProjectedModel>,
}

impl ICoordinatesConverter for CoordinatesConverter {
    // View -> Model conversion and related methods

    fn convert_view_position_to_model_position(&self, view_position: Position) -> Position {
        self.lines.convert_view_position_to_model_position(view_position.line_number, view_position.column)
    }

    fn convert_view_range_to_model_range(&self, view_range: Range) -> Range {
        self.lines.convert_view_range_to_model_range(view_range)
    }

    fn validate_view_position(&self, view_position: Position, expected_model_position: Position) -> Position {
        self.lines.validate_view_position(view_position.line_number, view_position.column, expected_model_position)
    }

    fn validate_view_range(&self, view_range: Range, expected_model_range: Range) -> Range {
        self.lines.validate_view_range(view_range, expected_model_range)
    }

    // Model -> View conversion and related methods

    fn convert_model_position_to_view_position(
        &self,
        model_position: Position,
        affinity: Option<PositionAffinity>,
        allow_zero_line_number: Option<bool>,
        below_hidden_ranges: Option<bool>,
    ) -> Position {
        self.lines.convert_model_position_to_view_position(
            model_position.line_number,
            model_position.column,
            affinity.unwrap_or(PositionAffinity::None),
            allow_zero_line_number.unwrap_or(false),
            below_hidden_ranges.unwrap_or(false),
        )
    }

    fn convert_model_range_to_view_range(&self, model_range: Range, affinity: Option<PositionAffinity>) -> Range {
        self.lines.convert_model_range_to_view_range(model_range, affinity.unwrap_or(PositionAffinity::Left))
    }

    fn model_position_is_visible(&self, model_position: Position) -> bool {
        self.lines.model_position_is_visible(model_position.line_number, model_position.column)
    }

    fn get_model_line_view_line_count(&self, model_line_number: i32) -> i32 {
        self.lines.get_model_line_view_line_count(model_line_number)
    }

    fn get_view_line_number_of_model_position(&self, model_line_number: i32, model_column: i32) -> i32 {
        self.lines.get_view_line_number_of_model_position(model_line_number, model_column)
    }
}

#[derive(Clone, Copy)]
enum IndentGuideRepeatOption {
    BlockNone = 0,
    BlockSubsequent = 1,
    BlockAll = 2,
}

/// O `createLineBreaksComputer` de `ViewModelLinesFromModelAsIs`: nenhuma linha quebra.
#[derive(Default)]
struct AsIsLineBreaksComputer {
    result: Vec<Option<ModelLineProjectionData>>,
}

impl LineBreaksComputer for AsIsLineBreaksComputer {
    fn add_request(&mut self, _line_number: i32, _previous_line_break_data: Option<ModelLineProjectionData>) {
        self.result.push(None);
    }

    fn finalize(&mut self) -> Vec<Option<ModelLineProjectionData>> {
        std::mem::take(&mut self.result)
    }
}

/// `ViewModelLinesFromModelAsIs`.
pub struct ViewModelLinesFromModelAsIs {
    pub model: Rc<TextModel>,
}

impl ViewModelLinesFromModelAsIs {
    pub fn new(model: Rc<TextModel>) -> Self {
        Self { model }
    }
}

impl IDisposable for ViewModelLinesFromModelAsIs {
    fn dispose(&self) {}
}

impl IViewModelLines for ViewModelLinesFromModelAsIs {
    fn create_coordinates_converter(&self) -> Box<dyn ICoordinatesConverter> {
        Box::new(IdentityCoordinatesConverter::new(self.model.clone()))
    }

    fn get_hidden_areas(&self) -> Vec<Range> {
        Vec::new()
    }

    fn set_hidden_areas(&self, _ranges: &[Range]) -> bool {
        false
    }

    fn set_tab_size(&self, _new_tab_size: f64) -> bool {
        false
    }

    fn set_wrapping_settings(
        &self,
        _font_info: &FontInfo,
        _wrapping_strategy: WrappingStrategy,
        _wrapping_column: f64,
        _wrapping_indent: WrappingIndent,
        _word_break: WordBreak,
        _use_two_cell_fullwidth_characters: bool,
    ) -> bool {
        false
    }

    fn create_line_breaks_computer<'a>(
        &'a self,
        _context: Option<&'a dyn LineBreaksComputerContext>,
    ) -> Box<dyn LineBreaksComputer + 'a> {
        Box::new(AsIsLineBreaksComputer::default())
    }

    fn on_model_flushed(&self) {}

    fn on_model_lines_deleted(
        &self,
        _version_id: Option<i64>,
        from_line_number: i32,
        to_line_number: i32,
    ) -> Option<ViewLinesDeletedEvent> {
        Some(ViewLinesDeletedEvent::new(from_line_number, to_line_number))
    }

    fn on_model_lines_inserted(
        &self,
        _version_id: Option<i64>,
        from_line_number: i32,
        to_line_number: i32,
        _line_breaks: Vec<Option<ModelLineProjectionData>>,
    ) -> Option<ViewLinesInsertedEvent> {
        Some(ViewLinesInsertedEvent::new(from_line_number, to_line_number))
    }

    fn on_model_line_changed(
        &self,
        _version_id: Option<i64>,
        line_number: i32,
        _line_break_data: Option<ModelLineProjectionData>,
    ) -> ModelLineChangedResult {
        (false, Some(ViewLinesChangedEvent::new(line_number, 1)), None, None)
    }

    fn accept_version_id(&self, _version_id: i64) {}

    fn get_view_line_count(&self) -> i32 {
        self.model.get_line_count()
    }

    fn get_active_indent_guide(&self, view_line_number: i32, _min_line_number: i32, _max_line_number: i32) -> ActiveIndentGuideInfo {
        ActiveIndentGuideInfo { start_line_number: view_line_number, end_line_number: view_line_number, indent: 0 }
    }

    fn get_view_lines_bracket_guides(
        &self,
        start_line_number: i32,
        end_line_number: i32,
        _active_position: Option<Position>,
        _options: BracketGuideOptions,
    ) -> Vec<Vec<IndentGuide>> {
        vec![Vec::new(); (end_line_number - start_line_number + 1).max(0) as usize]
    }

    fn get_view_lines_indent_guides(&self, view_start_line_number: i32, view_end_line_number: i32) -> Vec<i32> {
        vec![0; (view_end_line_number - view_start_line_number + 1).max(0) as usize]
    }

    fn get_view_line_content(&self, view_line_number: i32) -> JsString {
        self.model.get_line_content(view_line_number)
    }

    fn get_view_line_length(&self, view_line_number: i32) -> usize {
        self.model.get_line_length(view_line_number)
    }

    fn get_view_line_min_column(&self, view_line_number: i32) -> i32 {
        self.model.get_line_min_column(view_line_number)
    }

    fn get_view_line_max_column(&self, view_line_number: i32) -> i32 {
        self.model.get_line_max_column(view_line_number)
    }

    fn get_view_line_continues_with_wrapped_line(&self, _view_line_number: i32) -> bool {
        false
    }

    fn get_view_line_data(&self, view_line_number: i32) -> ViewLineData {
        let line_tokens = self.model.tokenization().get_line_tokens(view_line_number);
        let line_content = line_tokens.get_line_content();
        let max_column = line_content.len() as i32 + 1;
        ViewLineData::new(line_content, false, 1, max_column, 0, Rc::new(line_tokens.inflate()), None)
    }

    fn get_view_lines_data(&self, view_start_line_number: i32, view_end_line_number: i32, needed: &[bool]) -> Vec<Option<ViewLineData>> {
        let line_count = self.model.get_line_count();
        let view_start_line_number = view_start_line_number.max(1).min(line_count);
        let view_end_line_number = view_end_line_number.max(1).min(line_count);

        (view_start_line_number..=view_end_line_number)
            .map(|line_number| {
                let idx = (line_number - view_start_line_number) as usize;
                needed.get(idx).copied().unwrap_or(false).then(|| self.get_view_line_data(line_number))
            })
            .collect()
    }

    fn get_decorations_in_range(
        &self,
        range: Range,
        owner_id: i32,
        filter_out_validation: bool,
        filter_font_decorations: bool,
        only_minimap_decorations: bool,
        only_margin_decorations: bool,
    ) -> Vec<ModelDecoration> {
        self.model.get_decorations_in_range(&range, owner_id, filter_out_validation, filter_font_decorations, only_minimap_decorations, only_margin_decorations)
    }

    fn get_injected_text_at(&self, _position: Position) -> Option<InjectedText> {
        // Identity lines collection does not support injected text.
        None
    }

    fn normalize_position(&self, position: Position, affinity: PositionAffinity) -> Position {
        self.model.normalize_position(position, affinity)
    }

    fn get_line_indent_column(&self, line_number: i32) -> i32 {
        self.model.get_line_indent_column(line_number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::font_info::{BareFontInfoOptions, FontInfoOptions};
    use crate::text_model_guides::HorizontalGuidesState;
    use crate::undo_redo_service::UndoRedoService;

    /// O texto do modelo dos casos: três linhas do modelo (10, 2 e 5 caracteres).
    const TEXT: &str = "abcdefghij\nxy\nklmno";

    fn create_model(text: &str) -> Rc<TextModel> {
        TextModel::new(text.into(), "plaintext", TextModel::default_creation_options(), None, UndoRedoService::for_tests())
    }

    fn font_info(font_size: f64) -> FontInfo {
        FontInfo::new(
            FontInfoOptions {
                bare: BareFontInfoOptions {
                    pixel_ratio: 1.0,
                    font_family: "Fira Code".into(),
                    font_weight: "normal".into(),
                    font_size,
                    font_feature_settings: "normal".into(),
                    font_variation_settings: "normal".into(),
                    line_height: 19.0,
                    letter_spacing: 0.0,
                },
                is_monospace: true,
                typical_halfwidth_character_width: 8.0,
                typical_fullwidth_character_width: 16.0,
                can_use_halfwidth_rightwards_arrow: true,
                space_width: 8.0,
                middot_width: 8.0,
                wsmiddot_width: 8.0,
                max_digit_width: 8.0,
            },
            true,
        )
    }

    /// Quebra toda linha mais longa que `width` de `width` em `width` caracteres, sem indentação. As colunas
    /// visíveis são os próprios deslocamentos de quebra.
    struct FixedWidthBreaks {
        width: usize,
    }

    struct FixedWidthComputer<'a> {
        width: usize,
        context: &'a dyn LineBreaksComputerContext,
        line_numbers: Vec<i32>,
    }

    impl LineBreaksComputerFactory for FixedWidthBreaks {
        fn create_line_breaks_computer<'a>(
            &'a self,
            context: &'a dyn LineBreaksComputerContext,
            _font_info: &FontInfo,
            _tab_size: f64,
            _wrapping_column: f64,
            _wrapping_indent: WrappingIndent,
            _word_break: WordBreak,
            _wrap_on_escaped_line_feeds: bool,
            _use_two_cell_fullwidth_characters: bool,
        ) -> Box<dyn LineBreaksComputer + 'a> {
            Box::new(FixedWidthComputer { width: self.width, context, line_numbers: Vec::new() })
        }
    }

    impl LineBreaksComputer for FixedWidthComputer<'_> {
        fn add_request(&mut self, line_number: i32, _previous_line_break_data: Option<ModelLineProjectionData>) {
            self.line_numbers.push(line_number);
        }

        fn finalize(&mut self) -> Vec<Option<ModelLineProjectionData>> {
            let line_numbers = std::mem::take(&mut self.line_numbers);
            line_numbers
                .into_iter()
                .map(|line_number| {
                    let length = self.context.get_line_content(line_number).len();
                    if length <= self.width {
                        return None;
                    }
                    let mut break_offsets: Vec<usize> = (1..).map(|i| i * self.width).take_while(|offset| *offset < length).collect();
                    break_offsets.push(length);
                    let visible_columns = break_offsets.iter().map(|offset| *offset as f64).collect();
                    Some(ModelLineProjectionData::new(None, None, break_offsets, visible_columns, 0))
                })
                .collect()
        }
    }

    fn create_lines(model: &Rc<TextModel>, width: usize) -> Rc<ViewModelLinesFromProjectedModel> {
        let factory: Rc<dyn LineBreaksComputerFactory> = Rc::new(FixedWidthBreaks { width });
        ViewModelLinesFromProjectedModel::new(
            1,
            model.clone(),
            factory.clone(),
            factory,
            font_info(14.0),
            4.0,
            WrappingStrategy::Simple,
            4.0,
            WrappingIndent::None,
            WordBreak::Normal,
            false,
            false,
        )
    }

    fn view_contents(lines: &dyn IViewModelLines) -> Vec<String> {
        (1..=lines.get_view_line_count()).map(|n| lines.get_view_line_content(n).to_string_lossy()).collect()
    }

    // ViewModelLinesFromProjectedModel

    #[test]
    fn projected_wraps_the_model_lines() {
        let model = create_model(TEXT);
        let lines = create_lines(&model, 4);

        assert_eq!(lines.get_view_line_count(), 6);
        assert_eq!(view_contents(&*lines), ["abcd", "efgh", "ij", "xy", "klmn", "o"]);
        assert_eq!(lines.get_view_line_min_column(3), 1);
        assert_eq!(lines.get_view_line_max_column(1), 5);
        assert_eq!(lines.get_view_line_max_column(3), 3);
        assert_eq!(lines.get_view_line_length(5), 4);
        let continues: Vec<bool> = (1..=6).map(|n| lines.get_view_line_continues_with_wrapped_line(n)).collect();
        assert_eq!(continues, [true, true, false, false, true, false]);
        assert_eq!(lines.get_model_line_view_line_count(1), 3);
        assert_eq!(lines.get_model_line_view_line_count(4), 1);
        // Linhas da visão clampadas ao intervalo válido.
        assert_eq!(lines.get_view_line_content(0).to_string_lossy(), "abcd");
        assert_eq!(lines.get_view_line_content(99).to_string_lossy(), "o");
        model.dispose();
    }

    #[test]
    fn projected_converts_positions_between_model_and_view() {
        let model = create_model(TEXT);
        let lines = create_lines(&model, 4);

        assert_eq!(lines.convert_model_position_to_view_position(1, 6, PositionAffinity::None, false, false), Position::new(2, 2));
        assert_eq!(lines.convert_model_position_to_view_position(3, 3, PositionAffinity::None, false, false), Position::new(5, 3));
        assert_eq!(lines.convert_view_position_to_model_position(3, 2), Position::new(1, 10));
        assert_eq!(lines.convert_view_position_to_model_position(6, 1), Position::new(3, 5));
        assert_eq!(lines.get_view_line_number_of_model_position(3, 2), 5);

        // O conversor de coordenadas compartilha as linhas.
        let converter = lines.create_coordinates_converter();
        assert_eq!(converter.convert_view_position_to_model_position(Position::new(3, 2)), Position::new(1, 10));
        assert_eq!(
            converter.convert_model_position_to_view_position(Position::new(1, 6), None, None, None),
            Position::new(2, 2)
        );
        assert_eq!(converter.get_model_line_view_line_count(1), 3);
        assert!(converter.model_position_is_visible(Position::new(2, 1)));
        assert!(!converter.model_position_is_visible(Position::new(4, 1)));
        model.dispose();
    }

    #[test]
    fn projected_view_line_data_comes_from_the_projection() {
        let model = create_model(TEXT);
        let lines = create_lines(&model, 4);

        let data = lines.get_view_line_data(3);
        assert_eq!(data.content.to_string_lossy(), "ij");
        assert_eq!((data.min_column, data.max_column), (1, 3));
        assert!(!data.continues_with_wrapped_line);
        assert_eq!(data.start_visible_column, 8);

        let datas = lines.get_view_lines_data(2, 4, &[true, false, true]);
        assert_eq!(datas.len(), 3);
        assert_eq!(datas[0].as_ref().map(|d| d.content.to_string_lossy()), Some("efgh".to_owned()));
        assert!(datas[1].is_none());
        assert_eq!(datas[2].as_ref().map(|d| d.content.to_string_lossy()), Some("xy".to_owned()));
        model.dispose();
    }

    #[test]
    fn projected_hidden_areas_fold_model_lines() {
        let model = create_model(TEXT);
        let lines = create_lines(&model, 4);

        assert!(lines.set_hidden_areas(&[Range::new(2, 1, 2, 1)]));
        assert_eq!(lines.get_hidden_areas(), [Range::new(2, 1, 2, 1)]);
        assert_eq!(lines.get_view_line_count(), 5);
        assert_eq!(view_contents(&*lines), ["abcd", "efgh", "ij", "klmn", "o"]);
        assert_eq!(lines.get_model_line_view_line_count(2), 0);
        assert!(!lines.model_position_is_visible(2, 1));
        assert!(lines.model_position_is_visible(1, 1));
        // A posição dentro da dobra cai no fim da linha visível anterior.
        assert_eq!(lines.convert_model_position_to_view_position(2, 1, PositionAffinity::None, false, false), Position::new(3, 3));
        // Abaixo da dobra: o começo da linha visível seguinte.
        assert_eq!(lines.convert_model_position_to_view_position(2, 1, PositionAffinity::None, false, true), Position::new(4, 1));
        assert_eq!(lines.get_view_line_number_of_model_position(2, 1), 3);

        // O mesmo conjunto de áreas não muda nada.
        assert!(!lines.set_hidden_areas(&[Range::new(2, 1, 2, 1)]));

        lines.dispose();
        assert!(lines.get_hidden_areas().is_empty());
        model.dispose();
    }

    #[test]
    fn projected_hidden_areas_are_normalized_and_never_hide_everything() {
        let model = create_model(TEXT);
        let lines = create_lines(&model, 4);

        // Áreas que se tocam viram uma só, e só as linhas contam.
        assert!(lines.set_hidden_areas(&[Range::new(3, 5, 3, 5), Range::new(2, 1, 2, 1)]));
        assert_eq!(lines.get_hidden_areas(), [Range::new(2, 1, 3, 1)]);
        assert_eq!(lines.get_view_line_count(), 3);

        // Esconder tudo revela tudo.
        assert!(lines.set_hidden_areas(&[Range::new(1, 1, 3, 1)]));
        assert!(lines.get_hidden_areas().is_empty());
        assert_eq!(lines.get_view_line_count(), 6);
        model.dispose();
    }

    #[test]
    fn projected_model_events_update_the_mapping() {
        let model = create_model(TEXT);
        let lines = create_lines(&model, 4);

        // Versões velhas (a das linhas é a do modelo, 1) e ausentes não mudam nada.
        assert_eq!(lines.on_model_lines_deleted(Some(1), 1, 1), None);
        assert_eq!(lines.on_model_lines_deleted(None, 1, 1), None);
        assert_eq!(lines.on_model_lines_inserted(Some(0), 2, 2, vec![None]), None);
        assert_eq!(lines.on_model_line_changed(Some(1), 1, None), (false, None, None, None));

        // A linha 1 do modelo (3 linhas da visão) deixa de quebrar: 1 linha muda e 2 somem.
        let (mapping_changed, changed, inserted, deleted) = lines.on_model_line_changed(Some(2), 1, None);
        assert!(mapping_changed);
        assert_eq!(changed, Some(ViewLinesChangedEvent::new(1, 1)));
        assert_eq!(inserted, None);
        assert_eq!(deleted, Some(ViewLinesDeletedEvent::new(2, 3)));
        assert_eq!(lines.get_view_line_count(), 4);

        // Uma linha nova do modelo antes da linha 2.
        assert_eq!(lines.on_model_lines_inserted(Some(3), 2, 2, vec![None]), Some(ViewLinesInsertedEvent::new(2, 2)));
        assert_eq!(lines.get_view_line_count(), 5);

        // Remove essa linha de novo.
        assert_eq!(lines.on_model_lines_deleted(Some(4), 2, 2), Some(ViewLinesDeletedEvent::new(2, 2)));
        assert_eq!(lines.get_view_line_count(), 4);
        model.dispose();
    }

    #[test]
    fn projected_settings_rebuild_only_when_something_changed() {
        let model = create_model(TEXT);
        let lines = create_lines(&model, 4);

        assert!(!lines.set_tab_size(4.0));
        assert!(lines.set_tab_size(2.0));
        assert_eq!(lines.get_view_line_count(), 6);

        assert!(!lines.set_wrapping_settings(&font_info(14.0), WrappingStrategy::Simple, 4.0, WrappingIndent::None, WordBreak::Normal, false));
        assert!(lines.set_wrapping_settings(&font_info(14.0), WrappingStrategy::Simple, 8.0, WrappingIndent::None, WordBreak::Normal, false));
        assert!(lines.set_wrapping_settings(&font_info(15.0), WrappingStrategy::Advanced, 8.0, WrappingIndent::Same, WordBreak::KeepAll, true));
        assert_eq!(lines.get_view_line_count(), 6);

        lines.accept_version_id(7);
        assert_eq!(lines.on_model_lines_deleted(Some(7), 1, 1), None);
        model.dispose();
    }

    #[test]
    fn projected_indent_guides_and_active_guide_follow_the_model() {
        let model = create_model("a\n    b\n    c\nd");
        let lines = create_lines(&model, 100);

        assert_eq!(lines.get_view_lines_indent_guides(1, 4), model.guides().get_lines_indent_guides(1, 4));
        let active = lines.get_active_indent_guide(2, 1, 4);
        let expected = model.guides().get_active_indent_guide(2, 1, 4);
        assert_eq!(active, expected);
        assert_eq!(lines.get_line_indent_column(2), 5);

        let options = BracketGuideOptions {
            include_inactive: true,
            horizontal_guides: HorizontalGuidesState::Disabled,
            highlight_active: false,
        };
        assert_eq!(
            lines.get_view_lines_bracket_guides(1, 4, None, options),
            model.guides().get_lines_bracket_guides(1, 4, None, options)
        );
        model.dispose();
    }

    #[test]
    fn projected_wrapped_lines_have_no_indent_column() {
        let model = create_model("    abcdefgh");
        let lines = create_lines(&model, 6);

        assert_eq!(lines.get_view_line_count(), 2);
        assert_eq!(lines.get_line_indent_column(1), 5);
        assert_eq!(lines.get_line_indent_column(2), 0);
        // A linha que continua a anterior bloqueia o guia a partir da quebra.
        assert_eq!(lines.get_view_lines_indent_guides(1, 2).len(), 2);
        model.dispose();
    }

    #[test]
    fn projected_default_line_breaks_context_reads_the_model() {
        let model = create_model(TEXT);
        let lines = create_lines(&model, 4);

        let mut computer = lines.create_line_breaks_computer(None);
        for line_number in 1..=3 {
            computer.add_request(line_number, None);
        }
        let data = computer.finalize();
        assert_eq!(data.len(), 3);
        assert_eq!(data[0].as_ref().map(|d| d.break_offsets.clone()), Some(vec![4, 8, 10]));
        assert!(data[1].is_none());
        assert_eq!(data[2].as_ref().map(|d| d.break_offsets.clone()), Some(vec![4, 5]));
        model.dispose();
    }

    // ViewModelLinesFromModelAsIs

    #[test]
    fn as_is_mirrors_the_model() {
        let model = create_model("foo\nbar baz\n\nx");
        let lines = ViewModelLinesFromModelAsIs::new(model.clone());

        assert_eq!(lines.get_view_line_count(), 4);
        assert_eq!(view_contents(&lines), ["foo", "bar baz", "", "x"]);
        assert_eq!(lines.get_view_line_length(2), 7);
        assert_eq!((lines.get_view_line_min_column(2), lines.get_view_line_max_column(2)), (1, 8));
        assert!(!lines.get_view_line_continues_with_wrapped_line(2));
        assert!(lines.get_hidden_areas().is_empty());
        assert!(!lines.set_hidden_areas(&[Range::new(1, 1, 2, 1)]));
        assert!(!lines.set_tab_size(8.0));
        assert!(!lines.set_wrapping_settings(&font_info(14.0), WrappingStrategy::Advanced, 10.0, WrappingIndent::Same, WordBreak::Normal, false));
        assert_eq!(lines.get_injected_text_at(Position::new(1, 1)), None);
        assert_eq!(lines.get_line_indent_column(2), 1);
        assert_eq!(
            lines.normalize_position(Position::new(2, 3), PositionAffinity::Left),
            Position::new(2, 3)
        );
        lines.dispose();
        model.dispose();
    }

    #[test]
    fn as_is_view_line_data_and_ranges() {
        let model = create_model("foo\nbar baz\n\nx");
        let lines = ViewModelLinesFromModelAsIs::new(model.clone());

        let data = lines.get_view_line_data(2);
        assert_eq!(data.content.to_string_lossy(), "bar baz");
        assert_eq!((data.min_column, data.max_column, data.start_visible_column), (1, 8, 0));
        assert!(!data.continues_with_wrapped_line);
        assert!(data.inline_decorations.is_none());

        let datas = lines.get_view_lines_data(0, 3, &[true, false, true]);
        assert_eq!(datas.len(), 3);
        assert_eq!(datas[0].as_ref().map(|d| d.content.to_string_lossy()), Some("foo".to_owned()));
        assert!(datas[1].is_none());
        assert_eq!(datas[2].as_ref().map(|d| d.content.to_string_lossy()), Some(String::new()));
        // Sem `needed` para a linha, nada é lido.
        assert!(lines.get_view_lines_data(1, 2, &[]).iter().all(Option::is_none));

        assert_eq!(lines.get_view_lines_indent_guides(2, 4), [0, 0, 0]);
        assert_eq!(lines.get_active_indent_guide(3, 1, 4), ActiveIndentGuideInfo { start_line_number: 3, end_line_number: 3, indent: 0 });
        let options = BracketGuideOptions {
            include_inactive: true,
            horizontal_guides: HorizontalGuidesState::Enabled,
            highlight_active: true,
        };
        let guides = lines.get_view_lines_bracket_guides(2, 4, None, options);
        assert_eq!(guides.len(), 3);
        assert!(guides.iter().all(Vec::is_empty));
        model.dispose();
    }

    #[test]
    fn as_is_events_and_converter_are_identity() {
        let model = create_model("foo\nbar baz\n\nx");
        let lines = ViewModelLinesFromModelAsIs::new(model.clone());

        assert_eq!(lines.on_model_lines_deleted(None, 2, 3), Some(ViewLinesDeletedEvent::new(2, 3)));
        assert_eq!(lines.on_model_lines_inserted(None, 2, 3, Vec::new()), Some(ViewLinesInsertedEvent::new(2, 3)));
        assert_eq!(
            lines.on_model_line_changed(None, 2, None),
            (false, Some(ViewLinesChangedEvent::new(2, 1)), None, None)
        );

        let mut computer = lines.create_line_breaks_computer(None);
        computer.add_request(1, None);
        computer.add_request(2, None);
        assert_eq!(computer.finalize(), vec![None, None]);
        assert!(computer.finalize().is_empty());

        let converter = lines.create_coordinates_converter();
        assert_eq!(converter.convert_view_position_to_model_position(Position::new(2, 100)), Position::new(2, 8));
        assert_eq!(converter.get_model_line_view_line_count(2), 1);
        assert_eq!(converter.get_view_line_number_of_model_position(3, 1), 3);
        model.dispose();
    }
}
