//! Porte de `src/vs/editor/contrib/folding/browser/foldingModel.ts`: o modelo de dobras
//! (`FoldingModel`), o provedor de decorações (`IDecorationProvider`), o memento de persistência
//! (`CollapseMemento`) e as funções que recolhem e expandem regiões (`toggleCollapseState`,
//! `setCollapseStateLevelsDown`, `getNextFoldLine` e as demais).
//!
//! Decisões de tradução:
//! - `FoldingModel` é compartilhado (`Rc<FoldingModel>`) com mutabilidade interior, como o objeto do
//!   original: o evento `onDidChange` carrega o próprio modelo (`FoldingModelChangeEvent.model`) e os
//!   ouvintes (o `HiddenRangeModel`) leem as regiões de dentro do evento. Nenhum `RefCell` fica emprestado
//!   enquanto um evento é entregue ou um callback de decorações roda.
//! - As regiões do modelo são um `Rc<FoldingRegions>` trocado inteiro em `updatePost`, e cada
//!   [`FoldingRegion`] guarda o `Rc` de onde veio, como o objeto do original guarda `ranges`.
//! - `IDecorationProvider.changeDecorations<T>(callback): T | null` é genérico e não caberia num objeto de
//!   trait. O `FoldingModel` nunca lê o `T` (só roda o callback pelo efeito), então o callback é
//!   `&mut dyn FnMut(&ModelDecorationsChangeAccessor)` e o retorno é `Option<()>` (`None` é o `null`, o
//!   callback que lançou).
//! - `getDecorationOption` devolve `Rc<ModelDecorationOptions>`: a identidade das opções registradas (que os
//!   testes comparam com `===`) é a do `Rc`.
//! - `IModelDeltaDecoration` é [`ModelDeltaDecoration`] e `IRange` é [`Range`] (o mesmo tipo, com linhas
//!   `i32`); as linhas das regiões são `u32`. A conversão de linha negativa para `u32` vale `0`, que não
//!   casa com nenhuma região, como o `-1` do original.
//! - `getRegionsInside(region, filter)` distingue o filtro por `filter.length === 2` (com nível); aqui são
//!   as duas variantes de [`RegionFilter`].
//! - `levels` (`number`, com `Number.MAX_VALUE` para todos) é `usize` com `usize::MAX`; `lineNumbers` é
//!   `&[u32]`, e o parâmetro opcional de `setCollapseStateLevelsDown` é `Option`.
//! - `toggleCollapseState(regions)` ordena a lista recebida (o original ordena no próprio array) e a manda
//!   no evento.
//! - `hash(string)` de `_getLinesChecksum` é `string_hash` sobre as unidades UTF-16 das duas linhas
//!   concatenadas, e o `%` do `int32` mantém o sinal do dividendo como o do JavaScript.
//! - `applyMemento(state)` não confere `Array.isArray(state)`: o tipo já garante a lista.

use crate::cursor_common::CursorSimpleModel;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::hash::string_hash;
use je_base::lifecycle::IDisposable;

use super::SelectedLines;
use super::folding_ranges::{FoldRange, FoldSource, FoldingRangesInput, FoldingRegion, FoldingRegions};
use je_base::js_regexp::JsRegExp;
use crate::language_configuration::reg_exp_test;
use crate::model::ModelDeltaDecoration;
use crate::range::Range;
use crate::text_model::{ModelDecorationOptions, ModelDecorationsChangeAccessor, TextModel};

/// `IDecorationProvider`.
pub trait IDecorationProvider {
    fn get_decoration_option(&self, is_collapsed: bool, is_hidden: bool, is_manual: bool) -> Rc<ModelDecorationOptions>;

    fn change_decorations(&self, callback: &mut dyn FnMut(&ModelDecorationsChangeAccessor<'_>)) -> Option<()>;

    fn remove_decorations(&self, decoration_ids: &[String]);
}

/// `FoldingModelChangeEvent`.
#[derive(Clone)]
pub struct FoldingModelChangeEvent {
    pub model: Rc<FoldingModel>,
    pub collapse_state_changed: Option<Vec<FoldingRegion>>,
}

/// `ILineMemento`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMemento {
    pub start_line_number: u32,
    pub end_line_number: u32,
    pub checksum: Option<i32>,
    pub is_collapsed: Option<bool>,
    pub source: Option<FoldSource>,
}

/// `CollapseMemento`.
pub type CollapseMemento = Vec<LineMemento>;

/// `-1` e menores viram `0`, que nenhuma região casa.
fn to_line(line_number: i32) -> u32 {
    u32::try_from(line_number).unwrap_or(0)
}

/// `FoldingModel`.
pub struct FoldingModel {
    me: Weak<FoldingModel>,
    text_model: Rc<TextModel>,
    decoration_provider: Rc<dyn IDecorationProvider>,

    regions: RefCell<Rc<FoldingRegions>>,
    editor_decoration_ids: RefCell<Vec<String>>,

    update_event_emitter: Rc<Emitter<FoldingModelChangeEvent>>,
}

/// O estado que `toggleCollapseState` carrega entre as chamadas de `updateDecorationsUntil`.
struct ToggleState {
    /// index from [0 ... regions.length]
    k: usize,
    /// end of the range where decorations need to be updated
    dirty_region_end_line: i64,
    /// the end of the last hidden lines
    last_hidden_line: i64,
}

impl FoldingModel {
    pub fn new(text_model: Rc<TextModel>, decoration_provider: Rc<dyn IDecorationProvider>) -> Rc<FoldingModel> {
        Rc::new_cyclic(|me| FoldingModel {
            me: me.clone(),
            text_model,
            decoration_provider,
            regions: RefCell::new(Rc::new(FoldingRegions::new(Vec::new(), Vec::new(), None))),
            editor_decoration_ids: RefCell::new(Vec::new()),
            update_event_emitter: Emitter::new(),
        })
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<FoldingModelChangeEvent> {
        self.update_event_emitter.event()
    }

    /// `regions`.
    pub fn regions(&self) -> Rc<FoldingRegions> {
        self.regions.borrow().clone()
    }

    /// `textModel`.
    pub fn text_model(&self) -> &Rc<TextModel> {
        &self.text_model
    }

    /// `decorationProvider`.
    pub fn decoration_provider(&self) -> &Rc<dyn IDecorationProvider> {
        &self.decoration_provider
    }

    fn this(&self) -> Rc<FoldingModel> {
        self.me.upgrade().expect("o modelo está vivo enquanto um método dele roda")
    }

    pub fn toggle_collapse_state(&self, mut toggled_regions: Vec<FoldingRegion>) {
        if toggled_regions.is_empty() {
            return;
        }
        toggled_regions.sort_by_key(FoldingRegion::region_index);

        let regions = self.regions();
        let ids = self.editor_decoration_ids.borrow().clone();
        let mut processed: HashSet<String> = HashSet::new();
        self.decoration_provider.change_decorations(&mut |accessor| {
            let mut state = ToggleState {
                k: 0,
                dirty_region_end_line: -1,
                last_hidden_line: -1,
            };
            for region in &toggled_regions {
                let index = region.region_index();
                if let Some(editor_decoration_id) = ids.get(index)
                    && !processed.contains(editor_decoration_id)
                {
                    processed.insert(editor_decoration_id.clone());

                    // update all decorations up to current index using the old dirtyRegionEndLine
                    self.update_decorations_until(&regions, &ids, &mut state, accessor, index);

                    let new_collapse_state = !regions.is_collapsed(index);
                    regions.set_collapsed(index, new_collapse_state);

                    state.dirty_region_end_line = state.dirty_region_end_line.max(i64::from(regions.get_end_line_number(index)));
                }
            }
            self.update_decorations_until(&regions, &ids, &mut state, accessor, regions.length());
        });
        self.update_event_emitter.fire(FoldingModelChangeEvent {
            model: self.this(),
            collapse_state_changed: Some(toggled_regions),
        });
    }

    /// O `updateDecorationsUntil` local de `toggleCollapseState`.
    fn update_decorations_until(
        &self,
        regions: &FoldingRegions,
        ids: &[String],
        state: &mut ToggleState,
        accessor: &ModelDecorationsChangeAccessor<'_>,
        index: usize,
    ) {
        while state.k < index {
            let end_line_number = i64::from(regions.get_end_line_number(state.k));
            let is_collapsed = regions.is_collapsed(state.k);
            if end_line_number <= state.dirty_region_end_line {
                let is_manual = regions.get_source(state.k) != FoldSource::Provider;
                if let Some(id) = ids.get(state.k) {
                    accessor.change_decoration_options(
                        id,
                        self.decoration_provider
                            .get_decoration_option(is_collapsed, end_line_number <= state.last_hidden_line, is_manual),
                    );
                }
            }
            if is_collapsed && end_line_number > state.last_hidden_line {
                state.last_hidden_line = end_line_number;
            }
            state.k += 1;
        }
    }

    pub fn remove_manual_ranges(&self, ranges: &[Range]) {
        let regions = self.regions();
        let mut range_indexes_to_remove: HashSet<usize> = HashSet::new();
        let mut remove_all = false;
        for range in ranges {
            if range.is_empty() {
                let mut index = regions.find_range(to_line(range.start_line_number));
                while index != -1 && regions.get_source(index as usize) == FoldSource::Provider {
                    index = regions.get_parent_index(index as usize);
                }
                if index == -1 {
                    remove_all = true;
                } else {
                    range_indexes_to_remove.insert(index as usize);
                }
            }
        }
        let intersects_selection = |fold_range: &FoldRange| {
            ranges.iter().any(|range| {
                !range.is_empty()
                    && !(i64::from(range.start_line_number) > i64::from(fold_range.end_line_number)
                        || i64::from(fold_range.start_line_number) > i64::from(range.end_line_number))
            })
        };
        let mut new_folding_ranges: Vec<FoldRange> = Vec::new();
        for i in 0..regions.length() {
            let fold_range = regions.to_fold_range(i);
            if fold_range.source == FoldSource::Provider
                || (!remove_all && !range_indexes_to_remove.contains(&i) && !intersects_selection(&fold_range))
            {
                new_folding_ranges.push(fold_range);
            }
        }
        self.update_post(FoldingRegions::from_fold_ranges(&new_folding_ranges));
    }

    pub fn update(&self, new_regions: &FoldingRegions, selection: Option<&dyn SelectedLines>) {
        let folded_or_manual_ranges = self.current_folded_or_manual_ranges(selection);
        let new_ranges = FoldingRegions::sanitize_and_merge(
            FoldingRangesInput::Regions(new_regions),
            (&folded_or_manual_ranges).into(),
            Some(to_line(self.text_model.get_line_count())),
            selection,
        );
        self.update_post(FoldingRegions::from_fold_ranges(&new_ranges));
    }

    pub fn update_post(&self, new_regions: FoldingRegions) {
        let mut new_editor_decorations: Vec<ModelDeltaDecoration> = Vec::new();
        let mut last_hidden_line: i64 = -1;
        for index in 0..new_regions.length() {
            let start_line_number = new_regions.get_start_line_number(index) as i32;
            let end_line_number = new_regions.get_end_line_number(index) as i32;
            let is_collapsed = new_regions.is_collapsed(index);
            let is_manual = new_regions.get_source(index) != FoldSource::Provider;
            let decoration_range = Range {
                start_line_number,
                start_column: self.text_model.get_line_max_column(start_line_number),
                end_line_number,
                end_column: self.text_model.get_line_max_column(end_line_number) + 1,
            };
            new_editor_decorations.push(ModelDeltaDecoration {
                range: decoration_range,
                options: self
                    .decoration_provider
                    .get_decoration_option(is_collapsed, i64::from(end_line_number) <= last_hidden_line, is_manual),
            });
            if is_collapsed && i64::from(end_line_number) > last_hidden_line {
                last_hidden_line = i64::from(end_line_number);
            }
        }
        self.decoration_provider.change_decorations(&mut |accessor| {
            let old_ids = self.editor_decoration_ids.borrow().clone();
            let new_ids = accessor.delta_decorations(&old_ids, &new_editor_decorations);
            *self.editor_decoration_ids.borrow_mut() = new_ids;
        });
        *self.regions.borrow_mut() = Rc::new(new_regions);
        self.update_event_emitter.fire(FoldingModelChangeEvent {
            model: self.this(),
            collapse_state_changed: None,
        });
    }

    fn current_folded_or_manual_ranges(&self, selection: Option<&dyn SelectedLines>) -> Vec<FoldRange> {
        let regions = self.regions();
        let ids = self.editor_decoration_ids.borrow().clone();
        let mut folded_ranges: Vec<FoldRange> = Vec::new();
        for i in 0..regions.length() {
            let mut is_collapsed = regions.is_collapsed(i);
            let source = regions.get_source(i);
            if is_collapsed || source != FoldSource::Provider {
                let fold_range = regions.to_fold_range(i);
                let dec_range = ids.get(i).and_then(|id| self.text_model.get_decoration_range(id));
                if let Some(dec_range) = dec_range {
                    if is_collapsed
                        && selection.is_some_and(|s| {
                            s.starts_inside(to_line(dec_range.start_line_number + 1), to_line(dec_range.end_line_number))
                        })
                    {
                        is_collapsed = false; // uncollapse is the range is blocked
                    }
                    folded_ranges.push(FoldRange {
                        start_line_number: to_line(dec_range.start_line_number),
                        end_line_number: to_line(dec_range.end_line_number),
                        r#type: fold_range.r#type,
                        is_collapsed,
                        source,
                    });
                }
            }
        }

        folded_ranges
    }

    /// Collapse state memento, for persistence only
    pub fn get_memento(&self) -> Option<CollapseMemento> {
        let folded_or_manual_ranges = self.current_folded_or_manual_ranges(None);
        let mut result: Vec<LineMemento> = Vec::new();
        let max_line_number = i64::from(self.text_model.get_line_count());
        for range in &folded_or_manual_ranges {
            if range.start_line_number >= range.end_line_number
                || range.start_line_number < 1
                || i64::from(range.end_line_number) > max_line_number
            {
                continue;
            }
            let checksum = self.get_lines_checksum(range.start_line_number + 1, range.end_line_number);
            result.push(LineMemento {
                start_line_number: range.start_line_number,
                end_line_number: range.end_line_number,
                is_collapsed: Some(range.is_collapsed),
                source: Some(range.source),
                checksum: Some(checksum),
            });
        }
        if result.is_empty() { None } else { Some(result) }
    }

    /// Apply persisted state, for persistence only
    pub fn apply_memento(&self, state: &[LineMemento]) {
        let mut ranges_to_restore: Vec<FoldRange> = Vec::new();
        let max_line_number = self.text_model.get_line_count();
        for range in state {
            if range.start_line_number >= range.end_line_number
                || range.start_line_number < 1
                || i64::from(range.end_line_number) > i64::from(max_line_number)
            {
                continue;
            }
            let checksum = self.get_lines_checksum(range.start_line_number + 1, range.end_line_number);
            // `!range.checksum`: ausente ou zero.
            if range.checksum.is_none_or(|c| c == 0) || Some(checksum) == range.checksum {
                ranges_to_restore.push(FoldRange {
                    start_line_number: range.start_line_number,
                    end_line_number: range.end_line_number,
                    r#type: None,
                    is_collapsed: range.is_collapsed.unwrap_or(true),
                    source: range.source.unwrap_or(FoldSource::Provider),
                });
            }
        }

        let regions = self.regions();
        let new_ranges = FoldingRegions::sanitize_and_merge(
            FoldingRangesInput::Regions(&regions),
            (&ranges_to_restore).into(),
            Some(to_line(max_line_number)),
            None,
        );
        self.update_post(FoldingRegions::from_fold_ranges(&new_ranges));
    }

    fn get_lines_checksum(&self, line_number1: u32, line_number2: u32) -> i32 {
        let mut units = self.text_model.get_line_content(line_number1 as i32).units().to_vec();
        units.extend_from_slice(self.text_model.get_line_content(line_number2 as i32).units());
        let h = string_hash(&units, 0);
        h % 1_000_000 // 6 digits is plenty
    }

    pub fn get_all_regions_at_line(&self, line_number: u32, filter: Option<&dyn Fn(&FoldingRegion, usize) -> bool>) -> Vec<FoldingRegion> {
        let regions = self.regions();
        let mut result: Vec<FoldingRegion> = Vec::new();
        let mut index = regions.find_range(line_number);
        let mut level = 1;
        while index >= 0 {
            let current = regions.to_region(index as usize);
            if filter.is_none_or(|f| f(&current, level)) {
                result.push(current.clone());
            }
            level += 1;
            index = current.parent_index();
        }
        result
    }

    pub fn get_region_at_line(&self, line_number: u32) -> Option<FoldingRegion> {
        let regions = self.regions();
        let index = regions.find_range(line_number);
        if index >= 0 {
            return Some(regions.to_region(index as usize));
        }
        None
    }

    pub fn get_regions_inside(&self, region: Option<&FoldingRegion>, filter: Option<RegionFilter<'_>>) -> Vec<FoldingRegion> {
        let regions = self.regions();
        let mut result: Vec<FoldingRegion> = Vec::new();
        let index = region.map_or(0, |r| r.region_index() + 1);
        let end_line_number = region.map_or(u32::MAX, FoldingRegion::end_line_number);

        let mut level_stack: Vec<FoldingRegion> = Vec::new();
        for i in index..regions.length() {
            let current = regions.to_region(i);
            if regions.get_start_line_number(i) >= end_line_number {
                break;
            }
            match &filter {
                Some(RegionFilter::WithLevel(filter)) => {
                    while let Some(top) = level_stack.last()
                        && !current.contained_by(top.to_line_range())
                    {
                        level_stack.pop();
                    }
                    level_stack.push(current.clone());
                    if filter(&current, level_stack.len()) {
                        result.push(current);
                    }
                }
                Some(RegionFilter::Region(filter)) => {
                    if filter(&current) {
                        result.push(current);
                    }
                }
                None => result.push(current),
            }
        }
        result
    }
}

impl IDisposable for FoldingModel {
    fn dispose(&self) {
        let ids = self.editor_decoration_ids.borrow().clone();
        self.decoration_provider.remove_decorations(&ids);
        self.update_event_emitter.dispose();
    }
}

/// `RegionFilter | RegionFilterWithLevel`: o original as distingue por `filter.length === 2`.
#[derive(Clone, Copy)]
pub enum RegionFilter<'a> {
    Region(&'a dyn Fn(&FoldingRegion) -> bool),
    WithLevel(&'a dyn Fn(&FoldingRegion, usize) -> bool),
}

/// Collapse or expand the regions at the given locations
/// @param levels The number of levels. Use 1 to only impact the regions at the location, use `usize::MAX` for all levels.
/// @param line_numbers the location of the regions to collapse or expand, or if not set, all regions in the model.
pub fn toggle_collapse_state(folding_model: &FoldingModel, levels: usize, line_numbers: &[u32]) {
    let mut to_toggle: Vec<FoldingRegion> = Vec::new();
    for &line_number in line_numbers {
        if let Some(region) = folding_model.get_region_at_line(line_number) {
            let do_collapse = !region.is_collapsed();
            to_toggle.push(region.clone());
            if levels > 1 {
                let filter = |r: &FoldingRegion, level: usize| r.is_collapsed() != do_collapse && level < levels;
                let regions_inside = folding_model.get_regions_inside(Some(&region), Some(RegionFilter::WithLevel(&filter)));
                to_toggle.extend(regions_inside);
            }
        }
    }
    folding_model.toggle_collapse_state(to_toggle);
}

/// Collapse or expand the regions at the given locations including all children.
/// @param do_collapse Whether to collapse or expand
/// @param levels The number of levels. Use 1 to only impact the regions at the location, use `usize::MAX` for all levels.
/// @param line_numbers the location of the regions to collapse or expand, or if not set, all regions in the model.
pub fn set_collapse_state_levels_down(folding_model: &FoldingModel, do_collapse: bool, levels: usize, line_numbers: Option<&[u32]>) {
    let mut to_toggle: Vec<FoldingRegion> = Vec::new();
    let filter = |r: &FoldingRegion, level: usize| r.is_collapsed() != do_collapse && level < levels;
    match line_numbers {
        Some(line_numbers) if !line_numbers.is_empty() => {
            for &line_number in line_numbers {
                if let Some(region) = folding_model.get_region_at_line(line_number) {
                    if region.is_collapsed() != do_collapse {
                        to_toggle.push(region.clone());
                    }
                    if levels > 1 {
                        let regions_inside = folding_model.get_regions_inside(Some(&region), Some(RegionFilter::WithLevel(&filter)));
                        to_toggle.extend(regions_inside);
                    }
                }
            }
        }
        _ => {
            let regions_inside = folding_model.get_regions_inside(None, Some(RegionFilter::WithLevel(&filter)));
            to_toggle.extend(regions_inside);
        }
    }
    folding_model.toggle_collapse_state(to_toggle);
}

/// Collapse or expand the regions at the given locations including all parents.
/// @param do_collapse Whether to collapse or expand
/// @param levels The number of levels. Use 1 to only impact the regions at the location, use `usize::MAX` for all levels.
/// @param line_numbers the location of the regions to collapse or expand.
pub fn set_collapse_state_levels_up(folding_model: &FoldingModel, do_collapse: bool, levels: usize, line_numbers: &[u32]) {
    let mut to_toggle: Vec<FoldingRegion> = Vec::new();
    let filter = |region: &FoldingRegion, level: usize| region.is_collapsed() != do_collapse && level <= levels;
    for &line_number in line_numbers {
        let regions = folding_model.get_all_regions_at_line(line_number, Some(&filter));
        to_toggle.extend(regions);
    }
    folding_model.toggle_collapse_state(to_toggle);
}

/// Collapse or expand a region at the given locations. If the inner most region is already collapsed/expanded, uses the first parent instead.
/// @param do_collapse Whether to collapse or expand
/// @param line_numbers the location of the regions to collapse or expand.
pub fn set_collapse_state_up(folding_model: &FoldingModel, do_collapse: bool, line_numbers: &[u32]) {
    let mut to_toggle: Vec<FoldingRegion> = Vec::new();
    let filter = |region: &FoldingRegion, _level: usize| region.is_collapsed() != do_collapse;
    for &line_number in line_numbers {
        let regions = folding_model.get_all_regions_at_line(line_number, Some(&filter));
        if let Some(first) = regions.into_iter().next() {
            to_toggle.push(first);
        }
    }
    folding_model.toggle_collapse_state(to_toggle);
}

/// Folds or unfolds all regions that have a given level, except if they contain one of the blocked lines.
/// @param fold_level level. Level == 1 is the top level
/// @param do_collapse Whether to collapse or expand
pub fn set_collapse_state_at_level(folding_model: &FoldingModel, fold_level: usize, do_collapse: bool, blocked_line_numbers: &[u32]) {
    let filter = |region: &FoldingRegion, level: usize| {
        level == fold_level && region.is_collapsed() != do_collapse && !blocked_line_numbers.iter().any(|&line| region.contains_line(line))
    };
    let to_toggle = folding_model.get_regions_inside(None, Some(RegionFilter::WithLevel(&filter)));
    folding_model.toggle_collapse_state(to_toggle);
}

/// Folds or unfolds all regions, except if they contain or are contained by a region of one of the blocked lines.
/// @param do_collapse Whether to collapse or expand
/// @param blocked_line_numbers the location of regions to not collapse or expand
pub fn set_collapse_state_for_rest(folding_model: &FoldingModel, do_collapse: bool, blocked_line_numbers: &[u32]) {
    let mut filtered_regions: Vec<FoldingRegion> = Vec::new();
    for &line_number in blocked_line_numbers {
        let regions = folding_model.get_all_regions_at_line(line_number, None);
        if let Some(first) = regions.into_iter().next() {
            filtered_regions.push(first);
        }
    }
    let filter = |region: &FoldingRegion| {
        filtered_regions
            .iter()
            .all(|filtered_region| !filtered_region.contained_by(region.to_line_range()) && !region.contained_by(filtered_region.to_line_range()))
            && region.is_collapsed() != do_collapse
    };
    let to_toggle = folding_model.get_regions_inside(None, Some(RegionFilter::Region(&filter)));
    folding_model.toggle_collapse_state(to_toggle);
}

/// Folds all regions for which the lines start with a given regex
/// @param folding_model the folding model
pub fn set_collapse_state_for_matching_lines(folding_model: &FoldingModel, reg_exp: &Rc<JsRegExp>, do_collapse: bool) {
    let editor_model = folding_model.text_model();
    let regions = folding_model.regions();
    let mut to_toggle: Vec<FoldingRegion> = Vec::new();
    for i in (0..regions.length()).rev() {
        if do_collapse != regions.is_collapsed(i) {
            let start_line_number = regions.get_start_line_number(i);
            if reg_exp_test(reg_exp, editor_model.get_line_content(start_line_number as i32).units()) {
                to_toggle.push(regions.to_region(i));
            }
        }
    }
    folding_model.toggle_collapse_state(to_toggle);
}

/// Folds all regions of the given type
/// @param folding_model the folding model
pub fn set_collapse_state_for_type(folding_model: &FoldingModel, r#type: &str, do_collapse: bool) {
    let regions = folding_model.regions();
    let mut to_toggle: Vec<FoldingRegion> = Vec::new();
    for i in (0..regions.length()).rev() {
        if do_collapse != regions.is_collapsed(i) && Some(r#type) == regions.get_type(i) {
            to_toggle.push(regions.to_region(i));
        }
    }
    folding_model.toggle_collapse_state(to_toggle);
}

/// Get line to go to for parent fold of current line
/// @param line_number the current line number
/// @param folding_model the folding model
///
/// @return Parent fold start line
pub fn get_parent_fold_line(line_number: u32, folding_model: &FoldingModel) -> Option<u32> {
    let mut start_line_number: Option<u32> = None;
    if let Some(folding_region) = folding_model.get_region_at_line(line_number) {
        start_line_number = Some(folding_region.start_line_number());
        // If current line is not the start of the current fold, go to top line of current fold. If not, go to parent fold
        if line_number == folding_region.start_line_number() {
            let parent_folding_idx = folding_region.parent_index();
            start_line_number = if parent_folding_idx != -1 {
                Some(folding_model.regions().get_start_line_number(parent_folding_idx as usize))
            } else {
                None
            };
        }
    }
    start_line_number
}

/// Get line to go to for previous fold at the same level of current line
/// @param line_number the current line number
/// @param folding_model the folding model
///
/// @return Previous fold start line
pub fn get_previous_fold_line(line_number: u32, folding_model: &FoldingModel) -> Option<u32> {
    let regions = folding_model.regions();
    let folding_region = folding_model.get_region_at_line(line_number);
    match folding_region {
        // If on the folding range start line, go to previous sibling.
        Some(folding_region) if folding_region.start_line_number() == line_number => {
            // If current line is not the start of the current fold, go to top line of current fold. If not, go to previous fold.
            if line_number != folding_region.start_line_number() {
                return Some(folding_region.start_line_number());
            }
            // Find min line number to stay within parent.
            let expected_parent_index = folding_region.parent_index();
            let min_line_number = if expected_parent_index != -1 {
                regions.get_start_line_number(folding_region.parent_index() as usize)
            } else {
                0
            };

            // Find fold at same level.
            let mut current = Some(folding_region);
            while let Some(folding_region) = current {
                if folding_region.region_index() > 0 {
                    let previous = regions.to_region(folding_region.region_index() - 1);

                    // Keep at same level.
                    if previous.start_line_number() <= min_line_number {
                        return None;
                    } else if previous.parent_index() == expected_parent_index {
                        return Some(previous.start_line_number());
                    }
                    current = Some(previous);
                } else {
                    return None;
                }
            }
        }
        _ => {
            // Go to last fold that's before the current line.
            if regions.length() > 0 {
                let mut current = Some(regions.to_region(regions.length() - 1));
                while let Some(folding_region) = current {
                    // Found fold before current line.
                    if folding_region.start_line_number() < line_number {
                        return Some(folding_region.start_line_number());
                    }
                    current = if folding_region.region_index() > 0 {
                        Some(regions.to_region(folding_region.region_index() - 1))
                    } else {
                        None
                    };
                }
            }
        }
    }
    None
}

/// Get line to go to next fold at the same level of current line
/// @param line_number the current line number
/// @param folding_model the folding model
///
/// @return Next fold start line
pub fn get_next_fold_line(line_number: u32, folding_model: &FoldingModel) -> Option<u32> {
    let regions = folding_model.regions();
    let folding_region = folding_model.get_region_at_line(line_number);
    match folding_region {
        // If on the folding range start line, go to next sibling.
        Some(folding_region) if folding_region.start_line_number() == line_number => {
            // Find max line number to stay within parent.
            let expected_parent_index = folding_region.parent_index();
            let max_line_number = if expected_parent_index != -1 {
                regions.get_end_line_number(folding_region.parent_index() as usize)
            } else if regions.length() == 0 {
                return None;
            } else {
                regions.get_end_line_number(regions.length() - 1)
            };

            // Find fold at same level.
            let mut current = Some(folding_region);
            while let Some(folding_region) = current {
                if folding_region.region_index() < regions.length() {
                    let next = regions.to_region(folding_region.region_index() + 1);

                    // Keep at same level.
                    if next.start_line_number() >= max_line_number {
                        return None;
                    } else if next.parent_index() == expected_parent_index {
                        return Some(next.start_line_number());
                    }
                    current = Some(next);
                } else {
                    return None;
                }
            }
        }
        _ => {
            // Go to first fold that's after the current line.
            if regions.length() > 0 {
                let mut current = Some(regions.to_region(0));
                while let Some(folding_region) = current {
                    // Found fold after current line.
                    if folding_region.start_line_number() > line_number {
                        return Some(folding_region.start_line_number());
                    }
                    current = if folding_region.region_index() < regions.length() {
                        Some(regions.to_region(folding_region.region_index() + 1))
                    } else {
                        None
                    };
                }
            }
        }
    }
    None
}
