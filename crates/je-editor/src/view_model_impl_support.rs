//! Partes de `src/vs/editor/common/viewModel/viewModelImpl.ts` (linhas 1301 a 1479) que ficam fora da classe
//! `ViewModel`: `IBatchableTarget`, `ViewportStart`, `OverviewRulerDecorations`, `HiddenAreasModel` (com
//! `mergeLineRangeArray` e `rangeArraysEqual`) e `StableViewport`. A classe `ViewModel` é portada à parte
//! em `view_model_impl.rs`, que importa este módulo.
//!
//! Divergências:
//! - `IBatchableTarget.batchChanges<T>(cb)` é um método genérico de trait, e `ViewModel` o usa devolvendo o
//!   valor do callback; por isso o trait não é usado como objeto (`dyn`): quem o consome é genérico no alvo.
//! - `ITextModel` é o `Rc<TextModel>` (`_setTrackedRange` é `TextModel::set_tracked_range`, que devolve o id
//!   como `Option<String>`: com um intervalo `Some` o id sempre existe). `IViewModel` tem tipos associados
//!   (ver `view_model_types.rs`), então `ViewportStart::update` é genérico no modelo de visão.
//! - `ViewLayout` (a classe) ainda não tem porte aqui: `StableViewport::recover_viewport_start` recebe o
//!   `IViewLayout`, a interface, que é tudo o que ela usa (`getVerticalOffsetForLineNumber` e
//!   `setScrollPosition`). `getVerticalOffsetForLineNumber(n)` sem o segundo argumento é `None`.
//! - `ViewportStart` muta o próprio estado em `update` e `invalidate`, que recebem `&mut self`; `dispose`
//!   segue o `IDisposable` (`&self`, só mexe no modelo).
//! - `OverviewRulerDecorations._asMap` (cor para grupo) guarda o índice do grupo em `as_array`, porque o
//!   grupo mora no vetor; a ordem de `as_array` é a de inserção, como no original.
//! - `HiddenAreasModel.hiddenAreas` é um `Map<unknown, Range[]>` (itera na ordem de inserção, e `set` numa
//!   chave existente mantém a posição). A chave `unknown` vira o parâmetro de tipo `K: PartialEq`, que o
//!   `ViewModel` escolhe; o mapa é um vetor de pares. `getMergedRanges` recebe `&mut self` porque recalcula
//!   sob demanda, e `Range` é `Copy` e compara por valor (`equalsRange` é o `==`).

use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::scrollable::INewScrollPosition;

use crate::coordinates_converter::ICoordinatesConverter;
use crate::editor_common::ScrollType;
use crate::model::TrackedRangeStickiness;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;
use crate::view_model_types::{IViewLayout, IViewModel, OverviewRulerDecorationsGroup};

/// `IBatchableTarget`.
pub trait IBatchableTarget {
    /// Permite que o alvo aplique em lote as mudanças que o callback introduz.
    fn batch_changes<T>(&self, cb: impl FnOnce() -> T) -> T;
}

/// `ViewportStart`.
pub struct ViewportStart {
    model: Rc<TextModel>,
    view_line_number: i32,
    is_valid: bool,
    model_tracked_range: String,
    start_line_delta: f64,
}

impl ViewportStart {
    pub fn create(model: Rc<TextModel>) -> ViewportStart {
        let viewport_start_line_tracked_range = model
            .set_tracked_range(None, Some(&Range::new(1, 1, 1, 1)), TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges)
            .expect("um intervalo rastreado novo sempre tem id");
        ViewportStart { model, view_line_number: 1, is_valid: false, model_tracked_range: viewport_start_line_tracked_range, start_line_delta: 0.0 }
    }

    pub fn view_line_number(&self) -> i32 {
        self.view_line_number
    }

    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    pub fn model_tracked_range(&self) -> &str {
        &self.model_tracked_range
    }

    pub fn start_line_delta(&self) -> f64 {
        self.start_line_delta
    }

    pub fn update<VM: IViewModel>(&mut self, view_model: &VM, start_line_number: i32) {
        let position = view_model
            .coordinates_converter()
            .convert_view_position_to_model_position(Position::new(start_line_number, view_model.get_line_min_column(start_line_number)));
        let viewport_start_line_tracked_range = view_model
            .model()
            .set_tracked_range(
                Some(&self.model_tracked_range),
                Some(&Range::new(position.line_number, position.column, position.line_number, position.column)),
                TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges,
            )
            .expect("um intervalo rastreado com intervalo novo sempre tem id");
        let view_layout = view_model.view_layout();
        let viewport_start_line_top = view_layout.get_vertical_offset_for_line_number(start_line_number, None);
        let scroll_top = view_layout.get_current_scroll_top();

        self.view_line_number = start_line_number;
        self.is_valid = true;
        self.model_tracked_range = viewport_start_line_tracked_range;
        self.start_line_delta = scroll_top - viewport_start_line_top;
    }

    pub fn invalidate(&mut self) {
        self.is_valid = false;
    }
}

impl IDisposable for ViewportStart {
    fn dispose(&self) {
        self.model.set_tracked_range(Some(&self.model_tracked_range), None, TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges);
    }
}

/// `OverviewRulerDecorations`.
#[derive(Default)]
pub struct OverviewRulerDecorations {
    /// `_asMap`: a cor para o índice do grupo em `as_array`.
    as_map: HashMap<String, usize>,
    pub as_array: Vec<OverviewRulerDecorationsGroup>,
}

impl OverviewRulerDecorations {
    pub fn accept(&mut self, color: &str, z_index: i32, start_line_number: i32, end_line_number: i32, lane: i32) {
        let Some(&prev_index) = self.as_map.get(color) else {
            self.as_map.insert(color.to_owned(), self.as_array.len());
            self.as_array.push(OverviewRulerDecorationsGroup::new(color.to_owned(), z_index, vec![lane, start_line_number, end_line_number]));
            return;
        };
        let prev_data = &mut self.as_array[prev_index].data;
        let prev_lane = prev_data[prev_data.len() - 3];
        let prev_end_line_number = prev_data[prev_data.len() - 1];
        if prev_lane == lane && prev_end_line_number + 1 >= start_line_number {
            // merge into prev
            if end_line_number > prev_end_line_number {
                let last = prev_data.len() - 1;
                prev_data[last] = end_line_number;
            }
            return;
        }

        // push
        prev_data.extend([lane, start_line_number, end_line_number]);
    }
}

/// `HiddenAreasModel`: `K` é o tipo da fonte (o `unknown` do original).
pub struct HiddenAreasModel<K: PartialEq> {
    hidden_areas: Vec<(K, Vec<Range>)>,
    should_recompute: bool,
    ranges: Vec<Range>,
}

impl<K: PartialEq> Default for HiddenAreasModel<K> {
    fn default() -> Self {
        Self { hidden_areas: Vec::new(), should_recompute: false, ranges: Vec::new() }
    }
}

impl<K: PartialEq> HiddenAreasModel<K> {
    pub fn set_hidden_areas(&mut self, source: K, ranges: Vec<Range>) {
        match self.hidden_areas.iter_mut().find(|(existing_source, _)| *existing_source == source) {
            Some((_, existing)) => {
                if *existing == ranges {
                    return;
                }
                *existing = ranges;
            }
            None => self.hidden_areas.push((source, ranges)),
        }
        self.should_recompute = true;
    }

    /// O vetor devolvido é imutável.
    pub fn get_merged_ranges(&mut self) -> &[Range] {
        if !self.should_recompute {
            return &self.ranges;
        }
        self.should_recompute = false;
        let new_ranges = self.hidden_areas.iter().fold(Vec::new(), |r, (_, hidden_areas)| merge_line_range_array(&r, hidden_areas));
        if self.ranges != new_ranges {
            self.ranges = new_ranges;
        }
        &self.ranges
    }
}

fn merge_line_range_array(arr1: &[Range], arr2: &[Range]) -> Vec<Range> {
    let mut result = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < arr1.len() && j < arr2.len() {
        let item1 = arr1[i];
        let item2 = arr2[j];

        if item1.end_line_number < item2.start_line_number - 1 {
            result.push(item1);
            i += 1;
        } else if item2.end_line_number < item1.start_line_number - 1 {
            result.push(item2);
            j += 1;
        } else {
            let start_line_number = item1.start_line_number.min(item2.start_line_number);
            let end_line_number = item1.end_line_number.max(item2.end_line_number);
            result.push(Range::new(start_line_number, 1, end_line_number, 1));
            i += 1;
            j += 1;
        }
    }
    result.extend_from_slice(&arr1[i..]);
    result.extend_from_slice(&arr2[j..]);
    result
}

/// Mantém um viewport estável tentando manter constante a primeira linha do viewport.
pub struct StableViewport {
    pub viewport_start_model_position: Option<Position>,
    pub start_line_delta: f64,
}

impl StableViewport {
    pub fn new(viewport_start_model_position: Option<Position>, start_line_delta: f64) -> Self {
        Self { viewport_start_model_position, start_line_delta }
    }

    pub fn recover_viewport_start(&self, coordinates_converter: &dyn ICoordinatesConverter, view_layout: &dyn IViewLayout) {
        let Some(model_position) = self.viewport_start_model_position else {
            return;
        };
        let view_position = coordinates_converter.convert_model_position_to_view_position(model_position, None, None, None);
        let view_position_top = view_layout.get_vertical_offset_for_line_number(view_position.line_number, None);
        view_layout.set_scroll_position(
            INewScrollPosition { scroll_left: None, scroll_top: Some(view_position_top + self.start_line_delta) },
            ScrollType::Immediate,
        );
    }
}
