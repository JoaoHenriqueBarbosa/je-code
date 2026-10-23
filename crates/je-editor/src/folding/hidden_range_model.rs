//! Porte de `src/vs/editor/contrib/folding/browser/hiddenRangeModel.ts`: o modelo das linhas escondidas
//! pelas regiões recolhidas (`HiddenRangeModel`).
//!
//! Decisões de tradução:
//! - `HiddenRangeModel` é compartilhado (`Rc<HiddenRangeModel>`) com mutabilidade interior: o ouvinte do
//!   `onDidChange` do `FoldingModel` chama `updateHiddenRanges` no próprio modelo, então guarda só um
//!   `Weak` dele (sem ciclo de referência).
//! - `IRange` é [`Range`] (o mesmo tipo); `hiddenRanges` devolve uma cópia da lista. `Range` é `Copy`, então
//!   "reaproveitar o objeto antigo" do original é copiar o valor.
//! - `findFirstIdxMonotonousOrArrLen(ranges, r => line < r.startLineNumber)` é `partition_point` com o
//!   predicado negado: o primeiro índice em que `line < r.startLineNumber`, ou o tamanho da lista.
//! - `adjustSelections` altera os elementos da lista recebida (`&mut [Selection]`), como o original. O teste
//!   `if (adjustedStartLine)` trata `0` como falso (`startLineNumber - 1` de uma faixa que começa na
//!   linha 1), por isso o `!= 0`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};

use super::folding_model::FoldingModel;
use crate::eol_counter::count_eol;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model_events::ModelContentChangedEvent;

/// `HiddenRangeModel`.
pub struct HiddenRangeModel {
    folding_model: Rc<FoldingModel>,
    hidden_ranges: RefCell<Vec<Range>>,
    folding_model_listener: RefCell<Option<DisposableRef>>,
    update_event_emitter: Rc<Emitter<Vec<Range>>>,
    has_line_changes: Cell<bool>,
}

impl HiddenRangeModel {
    pub fn new(model: Rc<FoldingModel>) -> Rc<HiddenRangeModel> {
        let this = Rc::new_cyclic(|me: &std::rc::Weak<HiddenRangeModel>| {
            let me = me.clone();
            let listener = model.on_did_change().listen(move |_| {
                if let Some(this) = me.upgrade() {
                    this.update_hidden_ranges();
                }
            });
            HiddenRangeModel {
                folding_model: model.clone(),
                hidden_ranges: RefCell::new(Vec::new()),
                folding_model_listener: RefCell::new(Some(listener)),
                update_event_emitter: Emitter::new(),
                has_line_changes: Cell::new(false),
            }
        });
        if model.regions().length() > 0 {
            this.update_hidden_ranges();
        }
        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<Vec<Range>> {
        self.update_event_emitter.event()
    }

    /// `hiddenRanges`.
    pub fn hidden_ranges(&self) -> Vec<Range> {
        self.hidden_ranges.borrow().clone()
    }

    pub fn notify_change_model_content(&self, e: &ModelContentChangedEvent) {
        if !self.hidden_ranges.borrow().is_empty() && !self.has_line_changes.get() {
            self.has_line_changes.set(e.changes.iter().any(|change| {
                change.range.end_line_number != change.range.start_line_number || count_eol(change.text.units()).eol_count != 0
            }));
        }
    }

    fn update_hidden_ranges(&self) {
        let mut update_hidden_areas = false;
        let mut new_hidden_areas: Vec<Range> = Vec::new();
        let mut k = 0usize;

        let mut last_collapsed_start = i64::from(u32::MAX);
        let mut last_collapsed_end: i64 = -1;

        let ranges = self.folding_model.regions();
        let old_ranges_left = {
            let hidden_ranges = self.hidden_ranges.borrow();
            for i in 0..ranges.length() {
                if !ranges.is_collapsed(i) {
                    continue;
                }

                let start_line_number = i64::from(ranges.get_start_line_number(i)) + 1; // the first line is not hidden
                let end_line_number = i64::from(ranges.get_end_line_number(i));
                if last_collapsed_start <= start_line_number && end_line_number <= last_collapsed_end {
                    // ignore ranges contained in collapsed regions
                    continue;
                }

                if !update_hidden_areas
                    && k < hidden_ranges.len()
                    && i64::from(hidden_ranges[k].start_line_number) == start_line_number
                    && i64::from(hidden_ranges[k].end_line_number) == end_line_number
                {
                    // reuse the old ranges
                    new_hidden_areas.push(hidden_ranges[k]);
                    k += 1;
                } else {
                    update_hidden_areas = true;
                    new_hidden_areas.push(Range::new(start_line_number as i32, 1, end_line_number as i32, 1));
                }
                last_collapsed_start = start_line_number;
                last_collapsed_end = end_line_number;
            }
            k < hidden_ranges.len()
        };
        if self.has_line_changes.get() || update_hidden_areas || old_ranges_left {
            self.apply_hidden_ranges(new_hidden_areas);
        }
    }

    fn apply_hidden_ranges(&self, new_hidden_areas: Vec<Range>) {
        *self.hidden_ranges.borrow_mut() = new_hidden_areas.clone();
        self.has_line_changes.set(false);
        self.update_event_emitter.fire(new_hidden_areas);
    }

    pub fn has_ranges(&self) -> bool {
        !self.hidden_ranges.borrow().is_empty()
    }

    pub fn is_hidden(&self, line: i32) -> bool {
        find_range(&self.hidden_ranges.borrow(), line).is_some()
    }

    pub fn adjust_selections(&self, selections: &mut [Selection]) -> bool {
        let mut has_changes = false;
        let editor_model = self.folding_model.text_model();
        let mut last_range: Option<Range> = None;

        let mut adjust_line = |line: i32| -> Option<i32> {
            if !last_range.is_some_and(|range| is_inside(line, &range)) {
                last_range = find_range(&self.hidden_ranges.borrow(), line);
            }
            last_range.map(|range| range.start_line_number - 1)
        };
        for selection in selections.iter_mut() {
            let adjusted_start_line = adjust_line(selection.start_line_number);
            if let Some(adjusted_start_line) = adjusted_start_line.filter(|&line| line != 0) {
                *selection = selection.set_start_position(adjusted_start_line, editor_model.get_line_max_column(adjusted_start_line));
                has_changes = true;
            }
            let adjusted_end_line = adjust_line(selection.end_line_number);
            if let Some(adjusted_end_line) = adjusted_end_line.filter(|&line| line != 0) {
                *selection = selection.set_end_position(adjusted_end_line, editor_model.get_line_max_column(adjusted_end_line));
                has_changes = true;
            }
        }
        has_changes
    }
}

impl IDisposable for HiddenRangeModel {
    fn dispose(&self) {
        if !self.hidden_ranges.borrow().is_empty() {
            *self.hidden_ranges.borrow_mut() = Vec::new();
            self.update_event_emitter.fire(Vec::new());
        }
        let listener = self.folding_model_listener.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
        self.update_event_emitter.dispose();
    }
}

fn is_inside(line: i32, range: &Range) -> bool {
    line >= range.start_line_number && line <= range.end_line_number
}

fn find_range(ranges: &[Range], line: i32) -> Option<Range> {
    let i = ranges.partition_point(|r| line >= r.start_line_number) as i64 - 1;
    if i >= 0 && ranges[i as usize].end_line_number >= line {
        return Some(ranges[i as usize]);
    }
    None
}
