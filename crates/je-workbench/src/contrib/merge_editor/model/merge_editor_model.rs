//! Porte de `src/vs/workbench/contrib/mergeEditor/browser/model/mergeEditorModel.ts`.
//!
//! Notas de tradução:
//! - `class MergeEditorModel extends EditorModel` é um `Rc<MergeEditorModel>` com um [`EditorModel`] interno
//!   (`_register`, `isDisposed`), criado com `Rc::new_cyclic` para os ouvintes e os `then` alcançarem o modelo por
//!   `Weak` (o `WeakRef(this)` do original é o próprio `Weak`).
//! - `@ILanguageService` e `@IUndoRedoService` chegam por `new`; `ITextModel` é o `Rc<TextModel>`.
//! - `async` vira `Promise`: `reset`, `getResultValueWithConflictMarkers` e `onInitialized` (guardado depois do
//!   construtor, pois o `initialize` precisa do objeto pronto, e lido por [`MergeEditorModel::on_initialized`]).
//! - O `Map<ModifiedBaseRange, ModifiedBaseRangeData>` do original é chaveado pela identidade do objeto; aqui os
//!   intervalos são `Rc<ModifiedBaseRange>` e [`BaseRangeStates`] guarda os pares na ordem de inserção e busca pelo
//!   endereço (`Rc::ptr_eq`).
//! - `derived` de coleções devolve `Rc<...>` (a igualdade estrita do original é a identidade do objeto).
//! - `ModifiedBaseRangeData.accepted` é um `ObservableValue<ModifiedBaseRangeState>` com a igualdade por valor (ver
//!   `modified_base_range`).
//! - `autorunHandleChanges({ changeTracker: { createChangeSummary: () => undefined, handleChange } }, fn)` usa um
//!   `FnChangeTracker<()>`; o `shouldRecomputeHandledFromAccepted` (variável local capturada) é um `Rc<Cell<bool>>`.
//! - `_markInputAsHandled: boolean | InputNumber` (que o original não lê) é o `enum` [`MarkInputAsHandled`].
//! - O `new WeakRef(ModifiedBaseRangeData)` do `setInputHandled` aponta para a classe, que nunca morre: a checagem
//!   do dado é sempre verdadeira, e o `state` fica capturado forte, como no original.
//! - `chunks.join()` do `getInitialResultValue` junta com vírgula (o separador padrão de `Array.join`).
//! - `Number.MAX_SAFE_INTEGER` da linha do intervalo do `reset` vira `i32::MAX`.
//! - `localize(...)` é o `localize` do je-base.
//! - `leftJoin` (de `utils.ts`) é privado aqui; é o único pedaço de `utils.ts` de que o modelo depende.

use std::any::Any;
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::observable::change_tracker::FnChangeTracker;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value};
use je_base::observable::utils::utils_cancellation::wait_for_state;
use je_base::observable::{
    DebugNameOptions, IObservable, IReader, ITransaction, Observable, ObservableExt, SettableExt, StrictEquals, autorun_handle_changes,
    derived, keep_observed, transaction,
};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::language_service::ILanguageService;
use je_editor::line_range::LineRange;
use je_editor::model::{TextSnapshot, ValidEditOperation};
use je_editor::edit_operation::SingleEditOperation;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_editor::undo_redo::{IUndoRedoService, UndoRedoElement, UndoRedoElementRef, UndoRedoGroup};

use super::diff_computer::IMergeDiffComputer;
use super::line_range::MergeEditorLineRange;
use super::mapping::{DetailedLineRangeMapping, DocumentLineRangeMap, DocumentRangeMap, LineRangeMapping};
use super::modified_base_range::{InputNumber, ModifiedBaseRange, ModifiedBaseRangeState, ModifiedBaseRangeStateKind};
use super::text_model_diffs::{TextModelDiffChangeReason, TextModelDiffState, TextModelDiffs};
use crate::common::editor::editor_model::EditorModel;
use crate::contrib::merge_editor::telemetry::MergeEditorTelemetry;

pub struct InputData {
    pub text_model: Rc<TextModel>,
    pub title: Option<String>,
    pub detail: Option<String>,
    pub description: Option<String>,
}

/// `{ resetResult: boolean }`.
#[derive(Clone, Copy, Debug)]
pub struct MergeEditorModelOptions {
    pub reset_result: bool,
}

/// `boolean | InputNumber` do `_markInputAsHandled` de `setState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkInputAsHandled {
    Bool(bool),
    Input(InputNumber),
}

impl From<bool> for MarkInputAsHandled {
    fn from(value: bool) -> Self {
        MarkInputAsHandled::Bool(value)
    }
}

impl From<InputNumber> for MarkInputAsHandled {
    fn from(value: InputNumber) -> Self {
        MarkInputAsHandled::Input(value)
    }
}

/// O `Map<ModifiedBaseRange, ModifiedBaseRangeData>`: na ordem de inserção, chaveado pela identidade do intervalo.
pub struct BaseRangeStates {
    pub entries: Vec<(Rc<ModifiedBaseRange>, Rc<ModifiedBaseRangeData>)>,
}

impl BaseRangeStates {
    pub fn get(&self, base_range: &Rc<ModifiedBaseRange>) -> Option<&Rc<ModifiedBaseRangeData>> {
        self.entries.iter().find(|(range, _)| Rc::ptr_eq(range, base_range)).map(|(_, data)| data)
    }

    pub fn has(&self, base_range: &Rc<ModifiedBaseRange>) -> bool {
        self.get(base_range).is_some()
    }
}

pub struct MergeEditorModel {
    me: Weak<MergeEditorModel>,
    editor_model: EditorModel,

    pub base: Rc<TextModel>,
    pub input1: InputData,
    pub input2: InputData,
    pub result_text_model: Rc<TextModel>,
    options: MergeEditorModelOptions,
    pub telemetry: Rc<MergeEditorTelemetry>,
    language_service: Rc<dyn ILanguageService>,
    undo_redo_service: Rc<dyn IUndoRedoService>,

    input1_text_model_diffs: Rc<TextModelDiffs>,
    input2_text_model_diffs: Rc<TextModelDiffs>,
    result_text_model_diffs: Rc<TextModelDiffs>,
    pub modified_base_ranges: IObservable<Rc<Vec<Rc<ModifiedBaseRange>>>>,

    modified_base_range_result_states: IObservable<Rc<BaseRangeStates>>,

    result_snapshot: RefCell<Box<dyn TextSnapshot>>,

    pub base_input1_diffs: IObservable<Rc<Vec<DetailedLineRangeMapping>>>,
    pub base_input2_diffs: IObservable<Rc<Vec<DetailedLineRangeMapping>>>,
    pub base_result_diffs: IObservable<Rc<Vec<DetailedLineRangeMapping>>>,
    pub input1_result_mapping: IObservable<Rc<DocumentLineRangeMap>>,
    pub result_input1_mapping: IObservable<Rc<DocumentLineRangeMap>>,
    pub input2_result_mapping: IObservable<Rc<DocumentLineRangeMap>>,
    pub result_input2_mapping: IObservable<Rc<DocumentLineRangeMap>>,
    pub base_result_mapping: IObservable<Rc<DocumentLineRangeMap>>,
    pub result_base_mapping: IObservable<Rc<DocumentLineRangeMap>>,

    pub diff_computing_state: IObservable<MergeEditorModelState>,
    pub input_diff_computing_state: IObservable<MergeEditorModelState>,
    pub is_up_to_date: IObservable<bool>,

    on_initialized: OnceCell<Promise<()>>,

    first_run: Cell<bool>,
    pub unhandled_conflicts_count: IObservable<usize>,
    pub has_unhandled_conflicts: IObservable<bool>,
}

impl MergeEditorModel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base: Rc<TextModel>,
        input1: InputData,
        input2: InputData,
        result_text_model: Rc<TextModel>,
        diff_computer: Rc<dyn IMergeDiffComputer>,
        options: MergeEditorModelOptions,
        telemetry: Rc<MergeEditorTelemetry>,
        language_service: Rc<dyn ILanguageService>,
        undo_redo_service: Rc<dyn IUndoRedoService>,
    ) -> Rc<MergeEditorModel> {
        let editor_model = EditorModel::new_inner();
        let input1_text_model_diffs = TextModelDiffs::new(base.clone(), input1.text_model.clone(), diff_computer.clone());
        editor_model.register(input1_text_model_diffs.clone());
        let input2_text_model_diffs = TextModelDiffs::new(base.clone(), input2.text_model.clone(), diff_computer.clone());
        editor_model.register(input2_text_model_diffs.clone());
        let result_text_model_diffs = TextModelDiffs::new(base.clone(), result_text_model.clone(), diff_computer);
        editor_model.register(result_text_model_diffs.clone());

        let base_input1_diffs = input1_text_model_diffs.diffs();
        let base_input2_diffs = input2_text_model_diffs.diffs();
        let base_result_diffs = result_text_model_diffs.diffs();

        let modified_base_ranges: IObservable<Rc<Vec<Rc<ModifiedBaseRange>>>> = {
            let (diffs1, diffs2) = (base_input1_diffs.clone(), base_input2_diffs.clone());
            let (base, input1_text_model, input2_text_model) = (base.clone(), input1.text_model.clone(), input2.text_model.clone());
            derived(move |reader| {
                let input1_diffs = diffs1.read(reader);
                let input2_diffs = diffs2.read(reader);
                Rc::new(
                    ModifiedBaseRange::from_diffs(&input1_diffs, &input2_diffs, &base, &input1_text_model, &input2_text_model)
                        .into_iter()
                        .map(Rc::new)
                        .collect(),
                )
            })
        };
        let modified_base_range_result_states: IObservable<Rc<BaseRangeStates>> = {
            let ranges = modified_base_ranges.clone();
            derived(move |reader| {
                Rc::new(BaseRangeStates {
                    entries: ranges.read(reader).iter().map(|s| (s.clone(), Rc::new(ModifiedBaseRangeData::new(s)))).collect(),
                })
            })
        };
        let result_snapshot = RefCell::new(result_text_model.create_snapshot(false));

        let input1_result_mapping: IObservable<Rc<DocumentLineRangeMap>> = {
            let (diffs, result_diffs, text_model) = (base_input1_diffs.clone(), base_result_diffs.clone(), input1.text_model.clone());
            derived(move |reader| Rc::new(get_input_result_mapping(&diffs.read(reader), &result_diffs.read(reader), text_model.get_line_count())))
        };
        let result_input1_mapping = {
            let mapping = input1_result_mapping.clone();
            derived(move |reader| Rc::new(mapping.read(reader).reverse()))
        };
        let input2_result_mapping: IObservable<Rc<DocumentLineRangeMap>> = {
            let (diffs, result_diffs, text_model) = (base_input2_diffs.clone(), base_result_diffs.clone(), input2.text_model.clone());
            derived(move |reader| Rc::new(get_input_result_mapping(&diffs.read(reader), &result_diffs.read(reader), text_model.get_line_count())))
        };
        let result_input2_mapping = {
            let mapping = input2_result_mapping.clone();
            derived(move |reader| Rc::new(mapping.read(reader).reverse()))
        };
        let base_result_mapping: IObservable<Rc<DocumentLineRangeMap>> = {
            let result_diffs = base_result_diffs.clone();
            derived(move |reader| {
                let diffs: Vec<LineRangeMapping> = result_diffs.read(reader).iter().map(|d| d.mapping).collect();
                let map = DocumentLineRangeMap::new(diffs, -1);
                Rc::new(DocumentLineRangeMap::new(
                    map.line_range_mappings.iter().map(extend_empty_mapping).collect(),
                    map.input_line_count,
                ))
            })
        };
        let result_base_mapping = {
            let mapping = base_result_mapping.clone();
            derived(move |reader| Rc::new(mapping.read(reader).reverse()))
        };
        let diff_computing_state: IObservable<MergeEditorModelState> = {
            let all = [input1_text_model_diffs.clone(), input2_text_model_diffs.clone(), result_text_model_diffs.clone()];
            derived(move |reader| merge_state(all.iter().map(|s| s.state().read(reader))))
        };
        let input_diff_computing_state: IObservable<MergeEditorModelState> = {
            let all = [input1_text_model_diffs.clone(), input2_text_model_diffs.clone()];
            derived(move |reader| merge_state(all.iter().map(|s| s.state().read(reader))))
        };
        let is_up_to_date = {
            let state = diff_computing_state.clone();
            derived(move |reader| state.read(reader) == MergeEditorModelState::UpToDate)
        };

        let unhandled_conflicts_count: IObservable<usize> = {
            let states = modified_base_range_result_states.clone();
            derived(move |reader| {
                let map = states.read(reader);
                let mut unhandled_count = 0;
                for (_key, value) in &map.entries {
                    if !value.handled.read(reader) {
                        unhandled_count += 1;
                    }
                }
                unhandled_count
            })
        };
        let has_unhandled_conflicts = unhandled_conflicts_count.map(|value, _| /* hasUnhandledConflicts */ value > 0);

        editor_model.register(keep_observed(&modified_base_range_result_states));
        editor_model.register(keep_observed(&input1_result_mapping));
        editor_model.register(keep_observed(&input2_result_mapping));

        let this = Rc::new_cyclic(|me| MergeEditorModel {
            me: me.clone(),
            editor_model,
            base,
            input1,
            input2,
            result_text_model,
            options,
            telemetry,
            language_service,
            undo_redo_service,
            input1_text_model_diffs,
            input2_text_model_diffs,
            result_text_model_diffs,
            modified_base_ranges,
            modified_base_range_result_states,
            result_snapshot,
            base_input1_diffs,
            base_input2_diffs,
            base_result_diffs,
            input1_result_mapping,
            result_input1_mapping,
            input2_result_mapping,
            result_input2_mapping,
            base_result_mapping,
            result_base_mapping,
            diff_computing_state,
            input_diff_computing_state,
            is_up_to_date,
            on_initialized: OnceCell::new(),
            first_run: Cell::new(true),
            unhandled_conflicts_count,
            has_unhandled_conflicts,
        });

        let initialize_promise = this.initialize();

        let on_initialized = {
            let initialize_promise = initialize_promise.clone();
            wait_for_state(this.diff_computing_state.clone(), |state| *state == MergeEditorModelState::UpToDate, None, None)
                .then_promise(move |_| initialize_promise)
        };
        this.on_initialized.set(on_initialized).ok().expect("o `onInitialized` é definido uma vez");

        let weak = this.me.clone();
        initialize_promise.then(move |_| {
            let Some(this) = weak.upgrade() else { return };
            let should_recompute_handled_from_accepted = Rc::new(Cell::new(true));
            let tracker = {
                let flag = should_recompute_handled_from_accepted.clone();
                let states = this.modified_base_range_result_states.clone();
                let result_diffs = this.result_text_model_diffs.diffs();
                FnChangeTracker::new(
                    |_| (),
                    move |ctx, _| {
                        if ctx.did_change(&*states) {
                            flag.set(true);
                        }
                        if ctx.did_change(&*result_diffs) {
                            // Ignore non-text changes as we update the state directly
                            ctx.change::<TextModelDiffChangeReason>() == Some(&TextModelDiffChangeReason::TextChange)
                        } else {
                            true
                        }
                    },
                )
            };
            let weak = this.me.clone();
            this.editor_model.register(autorun_handle_changes(
                DebugNameOptions::named("Merge Editor Model: Recompute State From Result"),
                tracker,
                move |reader, _| {
                    // Merge Editor Model: Recompute State From Result
                    let Some(this) = weak.upgrade() else { return };
                    let states = this.modified_base_range_result_states.read(reader);
                    if !this.is_up_to_date.read(reader) {
                        return;
                    }
                    let result_diffs = this.result_text_model_diffs.diffs().read(reader);
                    transaction(|tx| {
                        // Merge Editor Model: Recompute State

                        this.update_base_range_accepted_state(&result_diffs, &states, tx);

                        if should_recompute_handled_from_accepted.get() {
                            should_recompute_handled_from_accepted.set(false);
                            for (_range, observable_state) in &states.entries {
                                let state = observable_state.accepted.get();
                                let handled = !(state.kind() == ModifiedBaseRangeStateKind::Base
                                    || state.kind() == ModifiedBaseRangeStateKind::Unrecognized);
                                observable_state.handled_input1.set(handled, Some(tx));
                                observable_state.handled_input2.set(handled, Some(tx));
                            }
                        }
                    });
                },
            ));
        });

        this
    }

    /// `onInitialized`.
    pub fn on_initialized(&self) -> Promise<()> {
        self.on_initialized.get().expect("definido no construtor").clone()
    }

    pub fn is_disposed(&self) -> bool {
        use crate::common::editor::editor_model::IEditorModel;
        self.editor_model.is_disposed()
    }

    fn initialize(&self) -> Promise<()> {
        if self.options.reset_result { self.reset() } else { Promise::resolved(()) }
    }

    pub fn reset(&self) -> Promise<()> {
        let this = self.me.upgrade().expect("o modelo está vivo");
        wait_for_state(self.input_diff_computing_state.clone(), |state| *state == MergeEditorModelState::UpToDate, None, None).then(move |_| {
            let states = this.modified_base_range_result_states.get();

            transaction(|tx| {
                // Set initial state

                for (range, state) in &states.entries {
                    let new_state: ModifiedBaseRangeState;
                    let handled: bool;
                    if range.input1_diffs.is_empty() {
                        new_state = ModifiedBaseRangeState::Base.with_input_value(InputNumber::Two, true, false);
                        handled = true;
                    } else if range.input2_diffs.is_empty() {
                        new_state = ModifiedBaseRangeState::Base.with_input_value(InputNumber::One, true, false);
                        handled = true;
                    } else if range.is_equal_change {
                        new_state = ModifiedBaseRangeState::Base.with_input_value(InputNumber::One, true, false);
                        handled = true;
                    } else {
                        new_state = ModifiedBaseRangeState::Base;
                        handled = false;
                    }

                    state.accepted.set(new_state, Some(tx));
                    state.computed_from_diffing.set(false);
                    state.previous_non_diffing_state.set(None);
                    state.handled_input1.set(handled, Some(tx));
                    state.handled_input2.set(handled, Some(tx));
                }

                let mut cursor_state_computer = |_: &[ValidEditOperation]| None;
                this.result_text_model.push_edit_operations(
                    None,
                    &[SingleEditOperation::new(Range::new(1, 1, i32::MAX, 1), Some(this.compute_auto_merged_result()), false)],
                    Some(&mut cursor_state_computer),
                    None,
                    None,
                );
            });
        })
    }

    fn compute_auto_merged_result(&self) -> JsString {
        let base_ranges = self.modified_base_ranges.get();

        let base_lines = self.base.get_lines_content();
        let input1_lines = self.input1.text_model.get_lines_content();
        let input2_lines = self.input2.text_model.get_lines_content();

        let mut result_lines: Vec<JsString> = Vec::new();

        let mut base_start_line_number = 1;

        for base_range in base_ranges.iter() {
            append_lines_to_result(&mut result_lines, &base_lines, &MergeEditorLineRange::from_line_numbers(base_start_line_number, base_range.base_range.start_line_number));
            base_start_line_number = base_range.base_range.end_line_number_exclusive;

            if base_range.input1_diffs.is_empty() {
                append_lines_to_result(&mut result_lines, &input2_lines, &base_range.input2_range);
            } else if base_range.input2_diffs.is_empty() {
                append_lines_to_result(&mut result_lines, &input1_lines, &base_range.input1_range);
            } else if base_range.is_equal_change {
                append_lines_to_result(&mut result_lines, &input1_lines, &base_range.input1_range);
            } else {
                append_lines_to_result(&mut result_lines, &base_lines, &base_range.base_range);
            }
        }

        append_lines_to_result(&mut result_lines, &base_lines, &MergeEditorLineRange::from_line_numbers(base_start_line_number, base_lines.len() as i32 + 1));

        join_lines(&result_lines, self.result_text_model.get_eol().as_units())
    }

    pub fn has_base_range(&self, base_range: &Rc<ModifiedBaseRange>) -> bool {
        self.modified_base_range_result_states.get().has(base_range)
    }

    pub fn is_applying_edit_in_result(&self) -> bool {
        self.result_text_model_diffs.is_applying_change()
    }

    pub fn translate_input_range_to_base(&self, input: InputNumber, range: &Range) -> Range {
        let base_input_diffs = match input {
            InputNumber::One => self.base_input1_diffs.get(),
            InputNumber::Two => self.base_input2_diffs.get(),
        };
        let map = DocumentRangeMap::new(base_input_diffs.iter().flat_map(|d| d.range_mappings.iter().copied()).collect(), 0).reverse();
        map.project_range(range).output_range
    }

    pub fn translate_base_range_to_input(&self, input: InputNumber, range: &Range) -> Range {
        let base_input_diffs = match input {
            InputNumber::One => self.base_input1_diffs.get(),
            InputNumber::Two => self.base_input2_diffs.get(),
        };
        let map = DocumentRangeMap::new(base_input_diffs.iter().flat_map(|d| d.range_mappings.iter().copied()).collect(), 0);
        map.project_range(range).output_range
    }

    pub fn get_line_range_in_result(&self, base_range: &MergeEditorLineRange, reader: Option<&dyn IReader>) -> MergeEditorLineRange {
        self.result_text_model_diffs.get_result_line_range(base_range, reader)
    }

    pub fn translate_result_range_to_base(&self, range: &Range) -> Range {
        let map = DocumentRangeMap::new(self.base_result_diffs.get().iter().flat_map(|d| d.range_mappings.iter().copied()).collect(), 0).reverse();
        map.project_range(range).output_range
    }

    pub fn translate_base_range_to_result(&self, range: &Range) -> Range {
        let map = DocumentRangeMap::new(self.base_result_diffs.get().iter().flat_map(|d| d.range_mappings.iter().copied()).collect(), 0);
        map.project_range(range).output_range
    }

    pub fn find_modified_base_ranges_in_range(&self, range_in_base: &MergeEditorLineRange) -> Vec<Rc<ModifiedBaseRange>> {
        // TODO use binary search
        self.modified_base_ranges.get().iter().filter(|r| r.base_range.intersects_or_touches(range_in_base)).cloned().collect()
    }

    fn update_base_range_accepted_state(&self, result_diffs: &[DetailedLineRangeMapping], states: &BaseRangeStates, tx: &dyn ITransaction) {
        let base_range_with_store_and_touching_diffs = left_join(&states.entries, result_diffs, |base_range, diff| {
            if base_range.0.base_range.intersects_or_touches(&diff.input_range()) {
                std::cmp::Ordering::Equal
            } else {
                LineRange::compare_by_start(&base_range.0.base_range, &diff.input_range())
            }
        });

        for (left, rights) in base_range_with_store_and_touching_diffs {
            let new_state = self.compute_state(&left.0, &rights);
            let data = &left.1;
            let old_state = data.accepted.get();
            if !old_state.equals(&new_state) {
                if !self.first_run.get() && !data.computed_from_diffing.get() {
                    // Don't set this on the first run - the first run might be used to restore state.
                    data.computed_from_diffing.set(true);
                    data.previous_non_diffing_state.set(Some(old_state));
                }
                data.accepted.set(new_state, Some(tx));
            }
        }

        if self.first_run.get() {
            self.first_run.set(false);
        }
    }

    fn compute_state(&self, base_range: &ModifiedBaseRange, conflicting_diffs: &[&DetailedLineRangeMapping]) -> ModifiedBaseRangeState {
        if conflicting_diffs.is_empty() {
            return ModifiedBaseRangeState::Base;
        }
        let conflicting_edits: Vec<_> = conflicting_diffs.iter().map(|d| d.get_line_edit()).collect();

        let edits_agree_with_diffs = |diffs: &[DetailedLineRangeMapping]| -> bool {
            let edits: Vec<_> = diffs.iter().map(|d| d.get_line_edit()).collect();
            je_base::arrays::equals(Some(&conflicting_edits), Some(&edits), |a, b| a.equals(b))
        };

        if edits_agree_with_diffs(&base_range.input1_diffs) {
            return ModifiedBaseRangeState::Base.with_input_value(InputNumber::One, true, false);
        }
        if edits_agree_with_diffs(&base_range.input2_diffs) {
            return ModifiedBaseRangeState::Base.with_input_value(InputNumber::Two, true, false);
        }

        let states = [
            ModifiedBaseRangeState::Base.with_input_value(InputNumber::One, true, false).with_input_value(InputNumber::Two, true, true),
            ModifiedBaseRangeState::Base.with_input_value(InputNumber::Two, true, false).with_input_value(InputNumber::One, true, true),
            ModifiedBaseRangeState::Base.with_input_value(InputNumber::One, true, false).with_input_value(InputNumber::Two, true, false),
            ModifiedBaseRangeState::Base.with_input_value(InputNumber::Two, true, false).with_input_value(InputNumber::One, true, false),
        ];

        for s in states {
            if let Some(edit) = base_range.get_edit_for_base(&s).edit {
                let result_range = self.result_text_model_diffs.get_result_line_range(&base_range.base_range, None);
                let existing_lines = result_range.get_lines(&self.result_text_model);

                if je_base::arrays::equals(Some(&edit.new_lines), Some(&existing_lines), |a, b| a == b) {
                    return s;
                }
            }
        }

        ModifiedBaseRangeState::Unrecognized
    }

    fn data_of(&self, base_range: &Rc<ModifiedBaseRange>) -> Option<Rc<ModifiedBaseRangeData>> {
        self.modified_base_range_result_states.get().get(base_range).cloned()
    }

    pub fn get_state(&self, base_range: &Rc<ModifiedBaseRange>) -> IObservable<ModifiedBaseRangeState> {
        let Some(existing_state) = self.data_of(base_range) else {
            errors::throw(Error::bug_indicating(Some("object must be from this instance")));
        };
        existing_state.accepted.to_observable()
    }

    pub fn set_state(
        &self,
        base_range: &Rc<ModifiedBaseRange>,
        state: ModifiedBaseRangeState,
        _mark_input_as_handled: impl Into<MarkInputAsHandled>,
        tx: &dyn ITransaction,
        _push_stack_element: bool,
    ) {
        if !self.is_up_to_date.get() {
            errors::throw(Error::bug_indicating(Some("Cannot set state while updating")));
        }

        let Some(existing_state) = self.data_of(base_range) else {
            errors::throw(Error::bug_indicating(Some("object must be from this instance")));
        };

        let conflicting_diffs = self.result_text_model_diffs.find_touching_diffs(&base_range.base_range);
        let group = UndoRedoGroup::new();
        self.result_text_model_diffs.remove_diffs(conflicting_diffs, Some(tx), Some(&group));

        let edit_for_base = base_range.get_edit_for_base(&state);

        existing_state.accepted.set(edit_for_base.effective_state, Some(tx));
        existing_state.previous_non_diffing_state.set(None);
        existing_state.computed_from_diffing.set(false);

        let input1_handled = existing_state.handled_input1.get();
        let input2_handled = existing_state.handled_input2.get();

        if !input1_handled || !input2_handled {
            let element: Rc<dyn UndoRedoElement> = Rc::new(MarkAsHandledUndoRedoElement {
                resource: self.result_text_model.uri(),
                merge_editor_model_ref: self.me.clone(),
                state_ref: Rc::downgrade(&existing_state),
                input1_handled,
                input2_handled,
            });
            self.undo_redo_service.push_element(UndoRedoElementRef::Resource(element), Some(&group), None);
        }

        if let Some(edit) = edit_for_base.edit {
            self.result_text_model.push_stack_element();
            self.result_text_model_diffs.apply_edit_relative_to_original(&edit, Some(tx), Some(&group));
            self.result_text_model.push_stack_element();
        }

        // always set conflict as handled
        existing_state.handled_input1.set(true, Some(tx));
        existing_state.handled_input2.set(true, Some(tx));
    }

    pub fn reset_dirty_conflicts_to_base(&self) {
        transaction(|tx| {
            // Reset Unknown Base Range States
            self.result_text_model.push_stack_element();
            for range in self.modified_base_ranges.get().iter() {
                if self.get_state(range).get().kind() == ModifiedBaseRangeStateKind::Unrecognized {
                    self.set_state(range, ModifiedBaseRangeState::Base, false, tx, false);
                }
            }
            self.result_text_model.push_stack_element();
        });
    }

    pub fn is_handled(&self, base_range: &Rc<ModifiedBaseRange>) -> IObservable<bool> {
        self.data_of(base_range).expect("o intervalo é deste modelo").handled.clone()
    }

    pub fn is_input_handled(&self, base_range: &Rc<ModifiedBaseRange>, input_number: InputNumber) -> IObservable<bool> {
        let state = self.data_of(base_range).expect("o intervalo é deste modelo");
        match input_number {
            InputNumber::One => state.handled_input1.to_observable(),
            InputNumber::Two => state.handled_input2.to_observable(),
        }
    }

    pub fn set_input_handled(&self, base_range: &Rc<ModifiedBaseRange>, input_number: InputNumber, handled: bool, tx: &dyn ITransaction) {
        let state = self.data_of(base_range).expect("o intervalo é deste modelo");
        if state.handled.get() == handled {
            return;
        }

        let element: Rc<dyn UndoRedoElement> = Rc::new(SetInputHandledUndoRedoElement {
            resource: self.result_text_model.uri(),
            model_ref: self.me.clone(),
            state: state.clone(),
            input_number,
            handled,
        });
        self.undo_redo_service.push_element(UndoRedoElementRef::Resource(element), None, None);

        match input_number {
            InputNumber::One => state.handled_input1.set(handled, Some(tx)),
            InputNumber::Two => state.handled_input2.set(handled, Some(tx)),
        }
    }

    pub fn set_handled(&self, base_range: &Rc<ModifiedBaseRange>, handled: bool, tx: &dyn ITransaction) {
        let state = self.data_of(base_range).expect("o intervalo é deste modelo");
        if state.handled.get() == handled {
            return;
        }

        state.handled_input1.set(handled, Some(tx));
        state.handled_input2.set(handled, Some(tx));
    }

    pub fn set_language_id(&self, language_id: &str, source: Option<&str>) {
        let language = self.language_service.create_by_id(Some(language_id)).language_id();
        self.base.set_language(&language, source);
        self.input1.text_model.set_language(&language, source);
        self.input2.text_model.set_language(&language, source);
        self.result_text_model.set_language(&language, source);
    }

    pub fn get_initial_result_value(&self) -> JsString {
        let mut chunks: Vec<JsString> = Vec::new();
        while let Some(chunk) = self.result_snapshot.borrow_mut().read() {
            chunks.push(chunk);
        }
        join_lines(&chunks, &[u16::from(b',')])
    }

    pub fn get_result_value_with_conflict_markers(&self) -> Promise<JsString> {
        let this = self.me.upgrade().expect("o modelo está vivo");
        wait_for_state(self.diff_computing_state.clone(), |state| *state == MergeEditorModelState::UpToDate, None, None).then(move |_| {
            if this.unhandled_conflicts_count.get() == 0 {
                return this.result_text_model.get_value();
            }

            let result_lines = this.result_text_model.get_lines_content();
            let input1_lines = this.input1.text_model.get_lines_content();
            let input2_lines = this.input2.text_model.get_lines_content();

            let states = this.modified_base_range_result_states.get();

            let mut output_lines: Vec<JsString> = Vec::new();

            let mut result_start_line_number = 1;

            for (range, state) in &states.entries {
                if state.handled.get() {
                    continue;
                }
                let result_range = this.result_text_model_diffs.get_result_line_range(&range.base_range, None);

                append_lines_to_result(
                    &mut output_lines,
                    &result_lines,
                    &MergeEditorLineRange::from_line_numbers(result_start_line_number, result_start_line_number.max(result_range.start_line_number)),
                );
                result_start_line_number = result_range.end_line_number_exclusive;

                output_lines.push("<<<<<<<".into());
                if state.accepted.get().kind() == ModifiedBaseRangeStateKind::Unrecognized {
                    // to prevent loss of data, use modified result as "ours"
                    append_lines_to_result(&mut output_lines, &result_lines, &result_range);
                } else {
                    append_lines_to_result(&mut output_lines, &input1_lines, &range.input1_range);
                }
                output_lines.push("=======".into());
                append_lines_to_result(&mut output_lines, &input2_lines, &range.input2_range);
                output_lines.push(">>>>>>>".into());
            }

            append_lines_to_result(
                &mut output_lines,
                &result_lines,
                &MergeEditorLineRange::from_line_numbers(result_start_line_number, result_lines.len() as i32 + 1),
            );
            join_lines(&output_lines, &[u16::from(b'\n')])
        })
    }

    pub fn conflict_count(&self) -> usize {
        self.modified_base_ranges.get().iter().filter(|r| r.is_conflicting()).count()
    }

    pub fn combinable_conflict_count(&self) -> usize {
        self.modified_base_ranges.get().iter().filter(|r| r.is_conflicting() && r.can_be_combined()).count()
    }

    /// Conta os intervalos em conflito cujo dado satisfaz `predicate` (o `arrayCount` sobre `entries()` do original).
    fn count_conflicts(&self, predicate: impl Fn(&ModifiedBaseRangeData) -> bool) -> usize {
        self.modified_base_range_result_states.get().entries.iter().filter(|(r, s)| r.is_conflicting() && predicate(s)).count()
    }

    pub fn conflicts_resolved_with_base(&self) -> usize {
        self.count_conflicts(|s| s.accepted.get().kind() == ModifiedBaseRangeStateKind::Base)
    }

    pub fn conflicts_resolved_with_input1(&self) -> usize {
        self.count_conflicts(|s| s.accepted.get().kind() == ModifiedBaseRangeStateKind::Input1)
    }

    pub fn conflicts_resolved_with_input2(&self) -> usize {
        self.count_conflicts(|s| s.accepted.get().kind() == ModifiedBaseRangeStateKind::Input2)
    }

    pub fn conflicts_resolved_with_smart_combination(&self) -> usize {
        self.count_conflicts(|s| matches!(s.accepted.get(), ModifiedBaseRangeState::Both { smart_combination: true, .. }))
    }

    pub fn manually_solved_conflict_count_that_equal_none(&self) -> usize {
        self.count_conflicts(|s| s.accepted.get().kind() == ModifiedBaseRangeStateKind::Unrecognized)
    }

    pub fn manually_solved_conflict_count_that_equal_smart_combine(&self) -> usize {
        self.count_conflicts(|s| s.computed_from_diffing.get() && matches!(s.accepted.get(), ModifiedBaseRangeState::Both { smart_combination: true, .. }))
    }

    pub fn manually_solved_conflict_count_that_equal_input1(&self) -> usize {
        self.count_conflicts(|s| s.computed_from_diffing.get() && s.accepted.get().kind() == ModifiedBaseRangeStateKind::Input1)
    }

    pub fn manually_solved_conflict_count_that_equal_input2(&self) -> usize {
        self.count_conflicts(|s| s.computed_from_diffing.get() && s.accepted.get().kind() == ModifiedBaseRangeStateKind::Input2)
    }

    pub fn manually_solved_conflict_count_that_equal_none_and_started_with_base(&self) -> usize {
        self.count_unrecognized_started_with(|previous| previous.kind() == ModifiedBaseRangeStateKind::Base)
    }

    pub fn manually_solved_conflict_count_that_equal_none_and_started_with_input1(&self) -> usize {
        self.count_unrecognized_started_with(|previous| previous.kind() == ModifiedBaseRangeStateKind::Input1)
    }

    pub fn manually_solved_conflict_count_that_equal_none_and_started_with_input2(&self) -> usize {
        self.count_unrecognized_started_with(|previous| previous.kind() == ModifiedBaseRangeStateKind::Input2)
    }

    pub fn manually_solved_conflict_count_that_equal_none_and_started_with_both_non_smart(&self) -> usize {
        self.count_unrecognized_started_with(|previous| matches!(previous, ModifiedBaseRangeState::Both { smart_combination: false, .. }))
    }

    pub fn manually_solved_conflict_count_that_equal_none_and_started_with_both_smart(&self) -> usize {
        self.count_unrecognized_started_with(|previous| matches!(previous, ModifiedBaseRangeState::Both { smart_combination: true, .. }))
    }

    /// Conflitos hoje `unrecognized` cujo `previousNonDiffingState` satisfaz `predicate`.
    fn count_unrecognized_started_with(&self, predicate: impl Fn(&ModifiedBaseRangeState) -> bool) -> usize {
        self.count_conflicts(|s| {
            s.accepted.get().kind() == ModifiedBaseRangeStateKind::Unrecognized && s.previous_non_diffing_state.get().is_some_and(|p| predicate(&p))
        })
    }
}

impl IDisposable for MergeEditorModel {
    fn dispose(&self) {
        self.editor_model.dispose();
    }
}

fn get_input_result_mapping(input_lines_diffs: &[DetailedLineRangeMapping], result_diffs: &[DetailedLineRangeMapping], input_line_count: i32) -> DocumentLineRangeMap {
    let to_mappings = |diffs: &[DetailedLineRangeMapping]| diffs.iter().map(|d| d.mapping).collect::<Vec<LineRangeMapping>>();
    let map = DocumentLineRangeMap::between_outputs(&to_mappings(input_lines_diffs), &to_mappings(result_diffs), input_line_count);
    DocumentLineRangeMap::new(map.line_range_mappings.iter().map(extend_empty_mapping).collect(), map.input_line_count)
}

fn extend_empty_mapping(m: &LineRangeMapping) -> LineRangeMapping {
    if m.input_range.is_empty() || m.output_range.is_empty() {
        // We can do this because two adjacent diffs have one line in between.
        LineRangeMapping::new(m.input_range.delta_start(-1), m.output_range.delta_start(-1))
    } else {
        *m
    }
}

/// O que `diffComputingState` e `inputDiffComputingState` calculam dos estados dos diffs.
fn merge_state(states: impl Iterator<Item = TextModelDiffState>) -> MergeEditorModelState {
    let states: Vec<TextModelDiffState> = states.collect();
    if states.contains(&TextModelDiffState::Initializing) {
        return MergeEditorModelState::Initializing;
    }
    if states.contains(&TextModelDiffState::Updating) {
        return MergeEditorModelState::Updating;
    }
    MergeEditorModelState::UpToDate
}

/// `appendLinesToResult`: uma linha fora da fonte (o `undefined` do original) vira texto vazio no `join`.
fn append_lines_to_result(result_lines: &mut Vec<JsString>, source: &[JsString], line_range: &MergeEditorLineRange) {
    for i in line_range.start_line_number..line_range.end_line_number_exclusive {
        result_lines.push(usize::try_from(i - 1).ok().and_then(|index| source.get(index)).cloned().unwrap_or_default());
    }
}

fn join_lines(lines: &[JsString], separator: &[u16]) -> JsString {
    let mut text = JsString::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            text.push_units(separator);
        }
        text.push_units(line.units());
    }
    text
}

/// `leftJoin` de `utils.ts`: para cada elemento da esquerda, os da direita que `compare` iguala (a direita é consumida
/// em ordem, como no `ArrayQueue.takeWhile`).
fn left_join<'l, 'r, L, R>(left: &'l [L], right: &'r [R], compare: impl Fn(&L, &R) -> std::cmp::Ordering) -> Vec<(&'l L, Vec<&'r R>)> {
    let mut first_idx = 0;
    let mut result = Vec::new();
    for left_element in left {
        while first_idx < right.len() && compare(left_element, &right[first_idx]) == std::cmp::Ordering::Greater {
            first_idx += 1;
        }
        let start_idx = first_idx;
        while first_idx < right.len() && compare(left_element, &right[first_idx]) == std::cmp::Ordering::Equal {
            first_idx += 1;
        }
        result.push((left_element, right[start_idx..first_idx].iter().collect()));
    }
    result
}

pub struct ModifiedBaseRangeData {
    pub accepted: Rc<ObservableValue<ModifiedBaseRangeState>>,
    pub handled_input1: Rc<ObservableValue<bool>>,
    pub handled_input2: Rc<ObservableValue<bool>>,

    pub computed_from_diffing: Cell<bool>,
    pub previous_non_diffing_state: Cell<Option<ModifiedBaseRangeState>>,

    pub handled: IObservable<bool>,
}

impl ModifiedBaseRangeData {
    fn new(base_range: &ModifiedBaseRange) -> Self {
        let accepted = observable_value(&format!("BaseRangeState{}", base_range.base_range), ModifiedBaseRangeState::Base);
        let handled_input1 = observable_value(&format!("BaseRangeHandledState{}.Input1", base_range.base_range), false);
        let handled_input2 = observable_value(&format!("BaseRangeHandledState{}.Input2", base_range.base_range), false);
        let handled = {
            let (input1, input2) = (handled_input1.clone(), handled_input2.clone());
            derived(move |reader| input1.read(reader) && input2.read(reader))
        };
        Self {
            accepted,
            handled_input1,
            handled_input2,
            computed_from_diffing: Cell::new(false),
            previous_non_diffing_state: Cell::new(None),
            handled,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeEditorModelState {
    Initializing = 1,
    UpToDate = 2,
    Updating = 3,
}

impl StrictEquals for MergeEditorModelState {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

struct MarkAsHandledUndoRedoElement {
    resource: URI,
    merge_editor_model_ref: Weak<MergeEditorModel>,
    state_ref: Weak<ModifiedBaseRangeData>,
    input1_handled: bool,
    input2_handled: bool,
}

impl UndoRedoElement for MarkAsHandledUndoRedoElement {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn label(&self) -> String {
        localize("undoMarkAsHandled", "Undo Mark As Handled", &[])
    }

    fn code(&self) -> String {
        "undoMarkAsHandled".to_string()
    }

    fn redo(&self) {
        let Some(merge_editor_model) = self.merge_editor_model_ref.upgrade() else { return };
        if merge_editor_model.is_disposed() {
            return;
        }
        let Some(state) = self.state_ref.upgrade() else { return };
        transaction(|tx| {
            state.handled_input1.set(true, Some(tx));
            state.handled_input2.set(true, Some(tx));
        });
    }

    fn undo(&self) {
        let Some(merge_editor_model) = self.merge_editor_model_ref.upgrade() else { return };
        if merge_editor_model.is_disposed() {
            return;
        }
        let Some(state) = self.state_ref.upgrade() else { return };
        transaction(|tx| {
            state.handled_input1.set(self.input1_handled, Some(tx));
            state.handled_input2.set(self.input2_handled, Some(tx));
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct SetInputHandledUndoRedoElement {
    resource: URI,
    model_ref: Weak<MergeEditorModel>,
    state: Rc<ModifiedBaseRangeData>,
    input_number: InputNumber,
    handled: bool,
}

impl SetInputHandledUndoRedoElement {
    /// O corpo comum de `redo` (`handled`) e `undo` (`!handled`).
    fn apply(&self, value: bool) {
        let model = self.model_ref.upgrade();
        if model.is_some_and(|model| !model.is_disposed()) {
            transaction(|tx| match self.input_number {
                InputNumber::One => self.state.handled_input1.set(value, Some(tx)),
                InputNumber::Two => self.state.handled_input2.set(value, Some(tx)),
            });
        }
    }
}

impl UndoRedoElement for SetInputHandledUndoRedoElement {
    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn label(&self) -> String {
        localize("setInputHandled", "Set Input Handled", &[])
    }

    fn code(&self) -> String {
        "setInputHandled".to_string()
    }

    fn redo(&self) {
        self.apply(self.handled);
    }

    fn undo(&self) {
        self.apply(!self.handled);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
