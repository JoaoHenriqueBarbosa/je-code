//! Porte de `src/vs/workbench/contrib/mergeEditor/browser/model/textModelDiffs.ts`.
//!
//! Notas de tradução:
//! - `class TextModelDiffs extends Disposable` é um `Rc<TextModelDiffs>` com um `Rc<Disposable>` interno
//!   (`_register`), criado com `Rc::new_cyclic` para os ouvintes e o `then` do diff alcançarem o objeto por `Weak`.
//! - `IObservableWithChange<T, C>` é o `IObservable<T>`: o tipo da mudança só existe nos `set`
//!   (`ObservableValue<T, C>`). Os `DetailedLineRangeMapping[]` ficam em `Rc<Vec<...>>`, para o `set` de uma
//!   lista nova disparar sempre (a igualdade estrita do original é a identidade do array, e o `Rc` a dá).
//! - `new Set(diffs).has(d)` do `removeDiffs` compara por identidade do objeto; aqui um diff é identificado pelo
//!   par de intervalos (entrada e saída), que é único numa lista de
//!   diffs (os intervalos de entrada são disjuntos). `removeDiffs` recebe a lista por valor: o `sort` e o `reverse`
//!   do original mutam o array do chamador, e nenhum chamador o relê.
//! - `_barrier.makeExclusiveOrSkip(fn)` vira `run_exclusively_or_skip` dentro do ouvinte.
//! - `cursorStateComputer = () => null` é um fecho que devolve `None`.
//! - `getResultLine` devolve um `number | DetailedLineRangeMapping`: o `enum` [`ResultLine`].

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::control_flow::ReentrancyBarrier;
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::observable::observables::observable_value::{ObservableValue, observable_value};
use je_base::observable::{
    IObservable, IReader, ITransaction, Observable, ObservableSignal, Settable, StrictEquals, autorun, observable_signal, transaction,
};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::model::ValidEditOperation;
use je_editor::text_model::TextModel;
use je_editor::undo_redo::UndoRedoGroup;

use super::diff_computer::IMergeDiffComputer;
use super::editing::LineRangeEdit;
use super::line_range::MergeEditorLineRange;
use super::mapping::DetailedLineRangeMapping;

pub struct TextModelDiffs {
    me: Weak<TextModelDiffs>,
    base: Rc<Disposable>,
    recompute_count: Cell<u32>,
    state: Rc<ObservableValue<TextModelDiffState, TextModelDiffChangeReason>>,
    diffs: Rc<ObservableValue<Rc<Vec<DetailedLineRangeMapping>>, TextModelDiffChangeReason>>,

    barrier: ReentrancyBarrier,
    is_disposed: Rc<Cell<bool>>,
    is_initializing: Cell<bool>,

    base_text_model: Rc<TextModel>,
    text_model: Rc<TextModel>,
    diff_computer: Rc<dyn IMergeDiffComputer>,
}

impl TextModelDiffs {
    pub fn new(base_text_model: Rc<TextModel>, text_model: Rc<TextModel>, diff_computer: Rc<dyn IMergeDiffComputer>) -> Rc<TextModelDiffs> {
        let is_disposed = Rc::new(Cell::new(false));
        let this = Rc::new_cyclic(|me| TextModelDiffs {
            me: me.clone(),
            base: Disposable::new(),
            recompute_count: Cell::new(0),
            state: observable_value("TextModelDiffs.state", TextModelDiffState::Initializing),
            diffs: observable_value("TextModelDiffs.diffs", Rc::new(Vec::new())),
            barrier: ReentrancyBarrier::new(),
            is_disposed: is_disposed.clone(),
            is_initializing: Cell::new(true),
            base_text_model: base_text_model.clone(),
            text_model: text_model.clone(),
            diff_computer,
        });

        let recompute_signal: Rc<ObservableSignal<()>> = observable_signal("recompute");

        let weak = this.me.clone();
        let signal = recompute_signal.clone();
        this.base.register(autorun(move |reader| {
            // Update diff state
            signal.read(reader);
            if let Some(this) = weak.upgrade() {
                this.recompute(reader);
            }
        }));

        for model in [&base_text_model, &text_model] {
            let weak = this.me.clone();
            let signal = recompute_signal.clone();
            this.base.register(model.on_did_change_content().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.barrier.run_exclusively_or_skip(|| signal.trigger(None, ()));
                }
            }));
        }
        this.base.register(to_disposable(move || is_disposed.set(true)));
        this
    }

    pub fn is_applying_change(&self) -> bool {
        self.barrier.is_occupied()
    }

    pub fn state(&self) -> IObservable<TextModelDiffState> {
        self.state.to_observable()
    }

    /// Diffs da base para a entrada.
    pub fn diffs(&self) -> IObservable<Rc<Vec<DetailedLineRangeMapping>>> {
        self.diffs.to_observable()
    }

    fn recompute(&self, reader: &dyn IReader) {
        self.recompute_count.set(self.recompute_count.get() + 1);
        let current_recompute_idx = self.recompute_count.get();

        if self.state.get() == TextModelDiffState::Initializing {
            self.is_initializing.set(true);
        }

        transaction(|tx| {
            // Starting Diff Computation.
            let next = if self.is_initializing.get() { TextModelDiffState::Initializing } else { TextModelDiffState::Updating };
            self.state.set_with_change(next, Some(tx), TextModelDiffChangeReason::Other);
        });

        let result = self.diff_computer.compute_diff(&self.base_text_model, &self.text_model, reader);

        let weak = self.me.clone();
        result.then(move |result| {
            let Some(this) = weak.upgrade() else { return };
            if this.is_disposed.get() {
                return;
            }

            if current_recompute_idx != this.recompute_count.get() {
                // There is a newer recompute call
                return;
            }

            transaction(|tx| {
                // Completed Diff Computation
                match result.diffs {
                    Some(diffs) => {
                        this.state.set_with_change(TextModelDiffState::UpToDate, Some(tx), TextModelDiffChangeReason::TextChange);
                        this.diffs.set_with_change(Rc::new(diffs), Some(tx), TextModelDiffChangeReason::TextChange);
                    }
                    None => {
                        this.state.set_with_change(TextModelDiffState::Error, Some(tx), TextModelDiffChangeReason::TextChange);
                    }
                }
                this.is_initializing.set(false);
            });
        });
    }

    fn ensure_up_to_date(&self) {
        if self.state.get() != TextModelDiffState::UpToDate {
            errors::throw(Error::bug_indicating(Some("Cannot remove diffs when the model is not up to date")));
        }
    }

    pub fn remove_diffs(&self, mut diff_to_removes: Vec<DetailedLineRangeMapping>, transaction: Option<&dyn ITransaction>, group: Option<&UndoRedoGroup>) {
        self.ensure_up_to_date();

        diff_to_removes.sort_by_key(|d| d.input_range().start_line_number);
        diff_to_removes.reverse(); // process from bottom of document upward

        let diffs = self.diffs.get();

        // Validate all diffs-to-remove exist using Set for O(1) lookup
        let to_remove_set: HashSet<DiffKey> = diff_to_removes.iter().map(diff_key).collect();
        if to_remove_set.len() != diff_to_removes.len() {
            errors::throw(Error::bug_indicating(None)); // duplicate entries
        }
        let diffs_set: HashSet<DiffKey> = diffs.iter().map(diff_key).collect();
        for d in &diff_to_removes {
            if !diffs_set.contains(&diff_key(d)) {
                errors::throw(Error::bug_indicating(None));
            }
        }

        // Apply text model edits in reverse document order (bottom-up, safe for line shifting)
        for diff_to_remove in &diff_to_removes {
            self.barrier.run_exclusively_or_throw(|| {
                let edits = diff_to_remove.get_reverse_line_edit().to_edits(self.text_model.get_line_count());
                self.push_edits(&edits, group);
            });
        }

        // Single forward pass: accumulate delta from removed diffs above, apply to remaining diffs below
        let mut cumulative_delta = 0;
        let mut new_diffs: Vec<DetailedLineRangeMapping> = Vec::new();

        for d in diffs.iter() {
            if to_remove_set.contains(&diff_key(d)) {
                cumulative_delta += d.input_range().length() - d.output_range().length();
            } else {
                new_diffs.push(if cumulative_delta != 0 { d.add_output_line_delta(cumulative_delta) } else { d.clone() });
            }
        }

        self.diffs.set_with_change(Rc::new(new_diffs), transaction, TextModelDiffChangeReason::Other);
    }

    /// Edit must be conflict free.
    pub fn apply_edit_relative_to_original(&self, edit: &LineRangeEdit, transaction: Option<&dyn ITransaction>, group: Option<&UndoRedoGroup>) {
        self.ensure_up_to_date();

        let edit_mapping = DetailedLineRangeMapping::new(
            edit.range,
            self.base_text_model.clone(),
            MergeEditorLineRange::from_length(edit.range.start_line_number, edit.new_lines.len() as i32),
            self.text_model.clone(),
            None,
        );

        let mut first_after = false;
        let mut delta = 0;
        let mut new_diffs: Vec<DetailedLineRangeMapping> = Vec::new();
        for diff in self.diffs.get().iter() {
            if diff.input_range().intersects_or_touches(&edit.range) {
                errors::throw(Error::bug_indicating(Some("Edit must be conflict free.")));
            } else if diff.input_range().is_after(&edit.range) {
                if !first_after {
                    first_after = true;
                    new_diffs.push(edit_mapping.add_output_line_delta(delta));
                }

                new_diffs.push(diff.add_output_line_delta(edit.new_lines.len() as i32 - edit.range.length()));
            } else {
                new_diffs.push(diff.clone());
            }

            if !first_after {
                delta += diff.output_range().length() - diff.input_range().length();
            }
        }

        if !first_after {
            new_diffs.push(edit_mapping.add_output_line_delta(delta));
        }

        self.barrier.run_exclusively_or_throw(|| {
            let edits = LineRangeEdit::new(edit.range.delta(delta), edit.new_lines.clone()).to_edits(self.text_model.get_line_count());
            self.push_edits(&edits, group);
        });
        self.diffs.set_with_change(Rc::new(new_diffs), transaction, TextModelDiffChangeReason::Other);
    }

    /// `this.textModel.pushEditOperations(null, edits, () => null, group)`.
    fn push_edits(&self, edits: &[je_editor::edit_operation::SingleEditOperation], group: Option<&UndoRedoGroup>) {
        let mut cursor_state_computer = |_: &[ValidEditOperation]| None;
        self.text_model.push_edit_operations(None, edits, Some(&mut cursor_state_computer), group, None);
    }

    pub fn find_touching_diffs(&self, base_range: &MergeEditorLineRange) -> Vec<DetailedLineRangeMapping> {
        self.diffs.get().iter().filter(|d| d.input_range().intersects_or_touches(base_range)).cloned().collect()
    }

    fn get_result_line(&self, line_number: i32, reader: Option<&dyn IReader>) -> ResultLine {
        let mut offset = 0;
        let diffs = self.diffs.read_opt(reader);
        for diff in diffs.iter() {
            if diff.input_range().contains(line_number) || diff.input_range().end_line_number_exclusive == line_number {
                return ResultLine::Diff(diff.clone());
            } else if diff.input_range().end_line_number_exclusive < line_number {
                offset = diff.mapping.resulting_delta_from_original_to_modified();
            } else {
                break;
            }
        }
        ResultLine::Number(line_number + offset)
    }

    pub fn get_result_line_range(&self, base_range: &MergeEditorLineRange, reader: Option<&dyn IReader>) -> MergeEditorLineRange {
        let start = match self.get_result_line(base_range.start_line_number, reader) {
            ResultLine::Number(n) => n,
            ResultLine::Diff(d) => d.output_range().start_line_number,
        };
        let end_exclusive = match self.get_result_line(base_range.end_line_number_exclusive, reader) {
            ResultLine::Number(n) => n,
            ResultLine::Diff(d) => d.output_range().end_line_number_exclusive,
        };

        MergeEditorLineRange::from_line_numbers(start, end_exclusive)
    }
}

impl IDisposable for TextModelDiffs {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// A identidade de um diff (ver as notas do módulo): os intervalos de entrada e de saída.
type DiffKey = (i32, i32, i32, i32);

fn diff_key(d: &DetailedLineRangeMapping) -> DiffKey {
    (d.input_range().start_line_number, d.input_range().end_line_number_exclusive, d.output_range().start_line_number, d.output_range().end_line_number_exclusive)
}

/// O `number | DetailedLineRangeMapping` de `getResultLine`.
enum ResultLine {
    Number(i32),
    Diff(DetailedLineRangeMapping),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextModelDiffChangeReason {
    Other = 0,
    TextChange = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextModelDiffState {
    Initializing = 1,
    UpToDate = 2,
    Updating = 3,
    Error = 4,
}

impl StrictEquals for TextModelDiffState {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

#[derive(Clone)]
pub struct ITextModelDiffsState {
    pub state: TextModelDiffState,
    pub diffs: Vec<DetailedLineRangeMapping>,
}
