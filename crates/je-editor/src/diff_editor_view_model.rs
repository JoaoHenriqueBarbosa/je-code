//! Porte de `src/vs/editor/browser/widget/diffEditor/diffEditorViewModel.ts`: o modelo do editor de diff (`DiffEditorViewModel`),
//! que calcula o diff entre os dois modelos de texto e o mantém em dia, o estado dele (`DiffState`, `DiffMapping`) e as
//! regiões sem mudanças que o editor esconde (`UnchangedRegion`, com o cálculo a partir das mudanças em `fromDiffs`).
//!
//! Divergências:
//! - **Observáveis.** Os `observableValue`/`derived` do original são o [`ObservableValue`] e o `derived` de `je_base::observable`.
//!   Os valores que o JavaScript compara por identidade (objetos e arrays) são `Rc` (a igualdade estrita dos observáveis é a
//!   identidade do `Rc`); um valor novo é um `Rc` novo, como o objeto literal novo do original.
//! - **`UnchangedRegion` é um `Rc`.** A região carrega observáveis e é compartilhada (a lista de regiões, o `isDragged` lido pelo
//!   widget, o `setVisibleRanges` que devolve `this`), então [`UnchangedRegion::new`] e [`UnchangedRegion::from_diffs`] entregam
//!   `Rc<UnchangedRegion>`, e `setVisibleRanges` recebe `self: &Rc<Self>`. `isDragged: undefined | 'bottom' | 'top'` é
//!   `Option<DraggedSide>`.
//! - **Leitores opcionais.** `getHiddenOriginalRange(reader: IReader | undefined)` e afins recebem `Option<&dyn IReader>`.
//! - **`IDiffEditorModel` e `IDiffEditorViewModel`** (`editorCommon.ts`) são o [`DiffEditorModel`] (os dois `ITextModel`, aqui
//!   `Rc<TextModel>`) e o trait [`IDiffEditorViewModel`].
//! - **`@IDiffProviderFactoryService`** é injetado por [`Injectable`]; `model` e `options` são os argumentos estáticos.
//! - **`updateUnchangedRegions(result, tx, reader?)`.** O terceiro parâmetro nunca é passado pelos três chamadores do original
//!   (todos leem sem leitor), então a função não o recebe.
//! - **`autorun(async reader => ...)`.** O autorun do Rust é síncrono: a parte antes do primeiro `await` (todas as leituras de
//!   observáveis, o registro das escutas de edição e a chamada do `computeDiff`) roda no corpo, e o resto (depois do `await`)
//!   roda numa função `async` (`async_fn`) que nunca usa o leitor, como no original. Um erro que não é cancelamento rejeita essa
//!   promessa, que ninguém espera, como no JavaScript.
//! - **`readHotReloadableExport(DefaultLinesDiffComputer, reader)`** e `readHotReloadableExport(optimizeSequenceDiffs, reader)`
//!   só existem para recarga a quente de módulos do desenvolvimento do VS Code e não têm equivalente: não são portados.
//! - **`applyOriginalEdits` e `applyModifiedEdits`** devolvem `undefined` no original (o corpo é um `TODO@hediet` comentado);
//!   são portados como estão, e o diff só é recalculado pelo `compute diff`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::arrays::group_adjacent_by;
use je_base::assert::soft_assert;
use je_base::async_::{RunOnceScheduler, async_fn, catch_if_not_canceled};
use je_base::cancellation::CancellationTokenSource;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::observable::observables::observable_value::{ObservableValue, observable_value_owned};
use je_base::observable::{
    DebugOwner, IObservable, IReader, ISettableObservable, ITransaction, Observable, SettableExt, StrictEquals, autorun, derived, observable_signal,
    observable_signal_from_event, transaction, wait_for_state,
};
use je_base::promise::Promise;
use je_instantiation::Injectable;

use crate::bracket_pairs_text_model_part::bracket_pairs_tree::before_edit_position_mapper::TextEditInfo;
use crate::bracket_pairs_text_model_part::bracket_pairs_tree::combine_text_edit_infos::combine_text_edit_infos;
use crate::cursor_common::CursorSimpleModel;
use crate::diff::document_diff_provider::{DocumentDiff, DocumentDiffProviderOptions, IDocumentDiffProvider};
use crate::diff::lines_diff_computer::MovedText;
use crate::diff::range_mapping::{DetailedLineRangeMapping, LineRangeMapping, RangeMapping};
use crate::diff_editor_options::DiffEditorOptions;
use crate::diff_editor_utils::filter_with_previous;
use crate::diff_provider_factory_service::{DocumentDiffFactoryOptions, IDiffProviderFactoryService};
use crate::line_range::{LineRange, LineRangeSet, SerializedLineRange};
use crate::model::ModelDecorationOptionsInit;
use crate::model::ModelDeltaDecoration;
use crate::text_model::{ModelDecorationOptions, TextModel};
use crate::text_model_events::ModelContentChangedEvent;

/// `IDiffEditorModel`.
#[derive(Clone)]
pub struct DiffEditorModel {
    /// Original model.
    pub original: Rc<TextModel>,
    /// Modified model.
    pub modified: Rc<TextModel>,
}

/// `IDiffEditorViewModel`.
pub trait IDiffEditorViewModel: IDisposable {
    fn model(&self) -> &DiffEditorModel;

    fn wait_for_diff(&self) -> Promise<()>;
}

/// O estado das regiões sem mudanças: `{ regions, originalDecorationIds, modifiedDecorationIds }`.
pub struct UnchangedRegionsState {
    pub regions: Rc<Vec<Rc<UnchangedRegion>>>,
    pub original_decoration_ids: Vec<String>,
    pub modified_decoration_ids: Vec<String>,
}

/// O provedor de diff atual e o sinal que dispara quando ele muda (`{ diffProvider, onChangeSignal }`).
struct DiffProviderEntry {
    diff_provider: Rc<dyn IDocumentDiffProvider>,
    on_change_signal: IObservable<()>,
}

/// O tipo do observável de um valor com identidade `Rc` e sem mudança tipada.
type Value<T> = Rc<ObservableValue<T, ()>>;

pub struct DiffEditorViewModel {
    base: Rc<Disposable>,
    pub model: DiffEditorModel,
    options: Rc<DiffEditorOptions>,

    pub is_diff_up_to_date: IObservable<bool>,

    pub diff: IObservable<Option<Rc<DiffState>>>,

    unchanged_regions_value: Value<Option<Rc<UnchangedRegionsState>>>,
    pub unchanged_regions: IObservable<Rc<Vec<Rc<UnchangedRegion>>>>,

    pub moved_text_to_compare: Value<Option<Rc<MovedText>>>,
    active_moved_text_value: Value<Option<Rc<MovedText>>>,
    hovered_moved_text_value: Value<Option<Rc<MovedText>>>,
    pub active_moved_text: IObservable<Option<Rc<MovedText>>>,

    cancellation_token_source: Rc<CancellationTokenSource>,
}

impl Injectable for DiffEditorViewModel {
    type Args = (DiffEditorModel, Rc<DiffEditorOptions>);
    type Deps = (Rc<dyn IDiffProviderFactoryService>,);

    fn create((model, options): Self::Args, (diff_provider_factory_service,): Self::Deps) -> Rc<Self> {
        Self::new(model, options, diff_provider_factory_service)
    }
}

/// O `{ range, options: { description: 'unchanged' } }` das decorações das regiões sem mudanças.
fn unchanged_decorations(regions: &[Rc<UnchangedRegion>], range_of: impl Fn(&UnchangedRegion) -> LineRange) -> Vec<ModelDeltaDecoration> {
    let options = ModelDecorationOptions::register(&ModelDecorationOptionsInit { description: "unchanged".to_string(), ..ModelDecorationOptionsInit::default() });
    regions
        .iter()
        .map(|r| ModelDeltaDecoration {
            range: range_of(r).to_inclusive_range().expect("a região sem mudanças tem ao menos uma linha"),
            options: options.clone(),
        })
        .collect()
}

/// `model.<side>.getDecorationRange(id)` de cada decoração, como o intervalo inclusivo de linhas (`undefined` se a decoração sumiu).
fn decoration_line_ranges(model: &TextModel, ids: &[String]) -> Vec<Option<LineRange>> {
    ids.iter().map(|id| model.get_decoration_range(id)).map(|r| r.map(|r| LineRange::from_range_inclusive(&r))).collect()
}

/// `currentSyncedMovedText ? lastDiff.moves.find(m => m.lineRangeMapping.modified.intersect(current.lineRangeMapping.modified)) : undefined`.
fn find_synced_moved_text(current: Option<Rc<MovedText>>, diff: &DocumentDiff) -> Option<Rc<MovedText>> {
    let current = current?;
    diff.moves.iter().find(|m| m.line_range_mapping.modified.intersect(&current.line_range_mapping.modified).is_some()).cloned().map(Rc::new)
}

/// O `Fn` de `updateUnchangedRegions(result, tx)`.
type UpdateUnchangedRegions = Rc<dyn Fn(&DocumentDiff, &dyn ITransaction)>;

/// `applyOriginalEdits` e `applyModifiedEdits` (mesma assinatura).
type ApplyEdits = fn(&DocumentDiff, &[TextEditInfo], &Rc<TextModel>, &Rc<TextModel>) -> Option<DocumentDiff>;

impl DiffEditorViewModel {
    pub fn new(model: DiffEditorModel, options: Rc<DiffEditorOptions>, diff_provider_factory_service: Rc<dyn IDiffProviderFactoryService>) -> Rc<Self> {
        let owner = DebugOwner::new("DiffEditorViewModel");
        let base = Disposable::new();

        let is_diff_up_to_date_value = observable_value_owned::<bool, ()>(owner.clone(), false);
        let last_diff: Rc<RefCell<Option<DocumentDiff>>> = Rc::new(RefCell::new(None));
        let diff_value = observable_value_owned::<Option<Rc<DiffState>>, ()>(owner.clone(), None);
        let unchanged_regions_value = observable_value_owned::<Option<Rc<UnchangedRegionsState>>, ()>(owner.clone(), None);
        let moved_text_to_compare = observable_value_owned::<Option<Rc<MovedText>>, ()>(owner.clone(), None);
        let active_moved_text_value = observable_value_owned::<Option<Rc<MovedText>>, ()>(owner.clone(), None);
        let hovered_moved_text_value = observable_value_owned::<Option<Rc<MovedText>>, ()>(owner.clone(), None);

        let unchanged_regions = {
            let (hide_unchanged_regions, regions) = (options.hide_unchanged_regions.clone(), unchanged_regions_value.clone());
            derived(move |r| {
                if hide_unchanged_regions.read(r) {
                    regions.read(r).map(|state| state.regions.clone()).unwrap_or_else(|| Rc::new(Vec::new()))
                } else {
                    // Reset state
                    transaction(|tx| {
                        if let Some(state) = regions.get() {
                            for region in state.regions.iter() {
                                region.collapse_all(Some(tx));
                            }
                        }
                    });
                    Rc::new(Vec::new())
                }
            })
        };

        let active_moved_text = {
            let (to_compare, hovered, active) = (moved_text_to_compare.clone(), hovered_moved_text_value.clone(), active_moved_text_value.clone());
            derived(move |r| to_compare.read(r).or_else(|| hovered.read(r)).or_else(|| active.read(r)))
        };

        let cancellation_token_source = Rc::new(CancellationTokenSource::new(None));

        let diff_provider = {
            let (factory, options) = (diff_provider_factory_service, options.clone());
            derived(move |reader| {
                let diff_provider = factory.create_diff_provider(DocumentDiffFactoryOptions { diff_algorithm: Some(options.diff_algorithm.read(reader)) });
                let on_change_signal = observable_signal_from_event("onDidChange", diff_provider.on_did_change());
                Rc::new(DiffProviderEntry { diff_provider, on_change_signal })
            })
        };

        {
            let cancellation_token_source = cancellation_token_source.clone();
            base.register(to_disposable(move || cancellation_token_source.cancel()));
        }

        let content_changed_signal = observable_signal::<()>("contentChangedSignal");
        let debouncer = {
            let signal = content_changed_signal.clone();
            RunOnceScheduler::new(move || signal.trigger(None, ()), 200)
        };
        base.register(debouncer.clone());

        // `autorun`: collapse touching unchanged ranges
        {
            let (model, regions_value) = (model.clone(), unchanged_regions_value.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;

                let Some(last_unchanged_regions) = regions_value.read(reader) else {
                    return;
                };
                if last_unchanged_regions.regions.iter().any(|r| r.is_dragged.read(reader).is_some()) {
                    return;
                }

                let orig_ranges = decoration_line_ranges(&model.original, &last_unchanged_regions.original_decoration_ids);
                let mod_ranges = decoration_line_ranges(&model.modified, &last_unchanged_regions.modified_decoration_ids);
                let updated_last_unchanged_regions: Vec<Rc<UnchangedRegion>> = last_unchanged_regions
                    .regions
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, r)| {
                        let (orig, modified) = (orig_ranges[idx]?, mod_ranges[idx]?);
                        Some(UnchangedRegion::new(
                            orig.start_line_number,
                            modified.start_line_number,
                            orig.length(),
                            r.visible_line_count_top.read(reader),
                            r.visible_line_count_bottom.read(reader),
                        ))
                    })
                    .collect();

                let mut new_ranges: Vec<Rc<UnchangedRegion>> = Vec::new();

                let mut did_change = false;
                for touching in group_adjacent_by(updated_last_unchanged_regions, |a, b| {
                    a.get_hidden_modified_range(Some(reader)).end_line_number_exclusive == b.get_hidden_modified_range(Some(reader)).start_line_number
                }) {
                    if touching.len() > 1 {
                        did_change = true;
                        let sum_line_count: i32 = touching.iter().map(|r| r.line_count).sum();
                        let r = UnchangedRegion::new(
                            touching[0].original_line_number,
                            touching[0].modified_line_number,
                            sum_line_count,
                            touching[0].visible_line_count_top.get(),
                            touching[touching.len() - 1].visible_line_count_bottom.get(),
                        );
                        new_ranges.push(r);
                    } else {
                        new_ranges.push(touching[0].clone());
                    }
                }
                if did_change {
                    let original_decoration_ids = model.original.delta_decorations(
                        &last_unchanged_regions.original_decoration_ids,
                        &unchanged_decorations(&new_ranges, UnchangedRegion::original_unchanged_range),
                        0,
                    );
                    let modified_decoration_ids = model.modified.delta_decorations(
                        &last_unchanged_regions.modified_decoration_ids,
                        &unchanged_decorations(&new_ranges, UnchangedRegion::modified_unchanged_range),
                        0,
                    );

                    transaction(|tx| {
                        regions_value.set(
                            Some(Rc::new(UnchangedRegionsState { regions: Rc::new(new_ranges), original_decoration_ids, modified_decoration_ids })),
                            Some(tx),
                        );
                    });
                }
            }));
        }

        let update_unchanged_regions: UpdateUnchangedRegions = {
            let (model, options, regions_value) = (model.clone(), options.clone(), unchanged_regions_value.clone());
            Rc::new(move |result: &DocumentDiff, tx: &dyn ITransaction| {
                let original_line_count = model.original.get_line_count();
                let modified_line_count = model.modified.get_line_count();
                let new_unchanged_regions = UnchangedRegion::from_diffs(
                    &result.changes,
                    original_line_count,
                    modified_line_count,
                    options.hide_unchanged_regions_minimum_line_count.get(),
                    options.hide_unchanged_regions_context_line_count.get(),
                );

                // Transfer state from cur state
                let mut visible_regions: Option<Vec<LineRangeMapping>> = None;

                let last_unchanged_regions = regions_value.get();
                if let Some(last) = &last_unchanged_regions {
                    let orig_ranges = decoration_line_ranges(&model.original, &last.original_decoration_ids);
                    let mod_ranges = decoration_line_ranges(&model.modified, &last.modified_decoration_ids);
                    let candidates: Vec<Rc<UnchangedRegion>> = last
                        .regions
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, r)| {
                            let (orig, modified) = (orig_ranges[idx]?, mod_ranges[idx]?);
                            let length = orig.length();
                            Some(UnchangedRegion::new(
                                orig.start_line_number,
                                modified.start_line_number,
                                length,
                                // The visible area can shrink by edits -> we have to account for this
                                r.visible_line_count_top.get().min(length),
                                r.visible_line_count_bottom.get().min(length - r.visible_line_count_top.get()),
                            ))
                        })
                        .collect();
                    let updated_last_unchanged_regions = filter_with_previous(&candidates, |cur, prev| {
                        prev.is_none_or(|prev| {
                            cur.modified_line_number >= prev.modified_line_number + prev.line_count
                                && cur.original_line_number >= prev.original_line_number + prev.line_count
                        })
                    });

                    let hidden_regions: Vec<LineRangeMapping> = updated_last_unchanged_regions
                        .iter()
                        .map(|r| LineRangeMapping::new(r.get_hidden_original_range(None), r.get_hidden_modified_range(None)))
                        .collect();
                    let hidden_regions = LineRangeMapping::clip(&hidden_regions, LineRange::of_length(1, original_line_count), LineRange::of_length(1, modified_line_count));
                    visible_regions = Some(LineRangeMapping::inverse(&hidden_regions, original_line_count, modified_line_count));
                }

                let mut new_unchanged_regions2: Vec<Rc<UnchangedRegion>> = Vec::new();
                if let Some(visible_regions) = &visible_regions {
                    for r in &new_unchanged_regions {
                        let intersecting: Vec<LineRangeMapping> = visible_regions
                            .iter()
                            .filter(|f| f.original.intersects_strict(&r.original_unchanged_range()) && f.modified.intersects_strict(&r.modified_unchanged_range()))
                            .copied()
                            .collect();
                        new_unchanged_regions2.extend(r.set_visible_ranges(&intersecting, tx));
                    }
                } else {
                    new_unchanged_regions2.extend(new_unchanged_regions.iter().cloned());
                }

                let no_ids: Vec<String> = Vec::new();
                let original_decoration_ids = model.original.delta_decorations(
                    last_unchanged_regions.as_ref().map_or(&no_ids, |l| &l.original_decoration_ids),
                    &unchanged_decorations(&new_unchanged_regions2, UnchangedRegion::original_unchanged_range),
                    0,
                );
                let modified_decoration_ids = model.modified.delta_decorations(
                    last_unchanged_regions.as_ref().map_or(&no_ids, |l| &l.modified_decoration_ids),
                    &unchanged_decorations(&new_unchanged_regions2, UnchangedRegion::modified_unchanged_range),
                    0,
                );

                regions_value.set(
                    Some(Rc::new(UnchangedRegionsState { regions: Rc::new(new_unchanged_regions2), original_decoration_ids, modified_decoration_ids })),
                    Some(tx),
                );
            })
        };

        // As escutas de mudança de conteúdo: cada uma aplica as edições ao diff atual (que hoje nunca as aplica, ver o topo do módulo)
        // e marca o diff como desatualizado.
        let content_listener = |text_model: &Rc<TextModel>, apply_edits: ApplyEdits| {
            let (model, last_diff, diff_value, moved_text_to_compare, update, is_diff_up_to_date, debouncer) = (
                model.clone(),
                last_diff.clone(),
                diff_value.clone(),
                moved_text_to_compare.clone(),
                update_unchanged_regions.clone(),
                is_diff_up_to_date_value.clone(),
                debouncer.clone(),
            );
            base.register(text_model.on_did_change_content().listen(move |e: &ModelContentChangedEvent| {
                if diff_value.get().is_some() {
                    let text_edits = TextEditInfo::from_model_content_changes(&e.changes);
                    let current = last_diff.borrow().clone().expect("_lastDiff existe quando há diff");
                    if let Some(result) = apply_edits(&current, &text_edits, &model.original, &model.modified) {
                        *last_diff.borrow_mut() = Some(result.clone());
                        transaction(|tx| {
                            diff_value.set(Some(Rc::new(DiffState::from_diff_result(&result))), Some(tx));
                            update(&result, tx);
                            let current_synced_moved_text = moved_text_to_compare.get();
                            moved_text_to_compare.set(find_synced_moved_text(current_synced_moved_text, &result), Some(tx));
                        });
                    }
                }

                is_diff_up_to_date.set(false, None);
                debouncer.schedule(None);
            }));
        };
        content_listener(&model.modified, apply_modified_edits);
        content_listener(&model.original, apply_original_edits);

        // `autorun`: compute diff
        {
            let (model, options, last_diff, diff_value, moved_text_to_compare, update, is_diff_up_to_date, debouncer, content_changed_signal, token_source) = (
                model.clone(),
                options.clone(),
                last_diff.clone(),
                diff_value.clone(),
                moved_text_to_compare.clone(),
                update_unchanged_regions.clone(),
                is_diff_up_to_date_value.clone(),
                debouncer.clone(),
                content_changed_signal.clone(),
                cancellation_token_source.clone(),
            );
            base.register(autorun(move |reader| {
                let store = reader.store();
                let reader: &dyn IReader = reader;

                // So that they get recomputed when these settings change
                options.hide_unchanged_regions_minimum_line_count.read(reader);
                options.hide_unchanged_regions_context_line_count.read(reader);

                debouncer.cancel();
                content_changed_signal.read(reader);
                let document_diff_provider = diff_provider.read(reader);
                document_diff_provider.on_change_signal.read(reader);

                is_diff_up_to_date.set(false, None);

                let original_text_edit_infos: Rc<RefCell<Vec<TextEditInfo>>> = Rc::default();
                let modified_text_edit_infos: Rc<RefCell<Vec<TextEditInfo>>> = Rc::default();
                for (text_model, infos) in [(&model.original, &original_text_edit_infos), (&model.modified, &modified_text_edit_infos)] {
                    let infos = infos.clone();
                    store.add(text_model.on_did_change_content().listen(move |e: &ModelContentChangedEvent| {
                        let edits = TextEditInfo::from_model_content_changes(&e.changes);
                        let combined = combine_text_edit_infos(&infos.borrow(), &edits);
                        *infos.borrow_mut() = combined;
                    }));
                }

                let compute_diff = document_diff_provider.diff_provider.compute_diff(
                    &model.original,
                    &model.modified,
                    &DocumentDiffProviderOptions {
                        ignore_trim_whitespace: options.ignore_trim_whitespace.read(reader),
                        max_computation_time_ms: options.max_computation_time_ms.read(reader),
                        compute_moves: options.show_moves.read(reader),
                        extend_to_subwords: None,
                    },
                    &token_source.token(),
                );
                let result = catch_if_not_canceled(&compute_diff);

                let (model, last_diff, diff_value, moved_text_to_compare, update, is_diff_up_to_date, token_source) = (
                    model.clone(),
                    last_diff.clone(),
                    diff_value.clone(),
                    moved_text_to_compare.clone(),
                    update.clone(),
                    is_diff_up_to_date.clone(),
                    token_source.clone(),
                );
                async_fn(async move {
                    let Some(result) = result.await? else {
                        return Ok(());
                    };
                    if token_source.token().is_cancellation_requested() {
                        return Ok(());
                    }
                    if model.original.is_disposed() || model.modified.is_disposed() {
                        // TODO@hediet fishy?
                        return Ok(());
                    }
                    let result = normalize_document_diff(&result, &model.original, &model.modified);
                    let result = apply_original_edits(&result, &original_text_edit_infos.borrow(), &model.original, &model.modified).unwrap_or(result);
                    let result = apply_modified_edits(&result, &modified_text_edit_infos.borrow(), &model.original, &model.modified).unwrap_or(result);

                    transaction(|tx| {
                        // write diff result
                        update(&result, tx);

                        *last_diff.borrow_mut() = Some(result.clone());
                        let state = DiffState::from_diff_result(&result);
                        diff_value.set(Some(Rc::new(state)), Some(tx));
                        is_diff_up_to_date.set(true, Some(tx));
                        let current_synced_moved_text = moved_text_to_compare.get();
                        moved_text_to_compare.set(find_synced_moved_text(current_synced_moved_text, &result), Some(tx));
                    });
                    Ok(())
                });
            }));
        }

        let is_diff_up_to_date: IObservable<bool> = is_diff_up_to_date_value.clone();
        let diff: IObservable<Option<Rc<DiffState>>> = diff_value.clone();
        Rc::new(Self {
            base,
            model,
            options,
            is_diff_up_to_date,
            diff,
            unchanged_regions_value,
            unchanged_regions,
            moved_text_to_compare,
            active_moved_text_value,
            hovered_moved_text_value,
            active_moved_text,
            cancellation_token_source,
        })
    }

    pub fn set_active_moved_text(&self, moved_text: Option<Rc<MovedText>>) {
        self.active_moved_text_value.set(moved_text, None);
    }

    pub fn set_hovered_moved_text(&self, moved_text: Option<Rc<MovedText>>) {
        self.hovered_moved_text_value.set(moved_text, None);
    }

    /// As opções do editor de diff que este view model observa.
    pub fn options(&self) -> &Rc<DiffEditorOptions> {
        &self.options
    }

    /// A região escondida que contém a linha `line_number` de `hidden_range_of`, se houver, entre as regiões atuais.
    fn hidden_region_containing(&self, line_number: i32, hidden_range_of: impl Fn(&UnchangedRegion) -> LineRange) -> Option<Rc<UnchangedRegion>> {
        if self.diff.get().is_some_and(|diff| diff.mappings.is_empty()) {
            return None;
        }
        let state = self.unchanged_regions_value.get()?;
        state.regions.iter().find(|r| hidden_range_of(r).contains(line_number)).cloned()
    }

    pub fn ensure_modified_line_is_visible(&self, line_number: i32, preference: RevealPreference, tx: Option<&dyn ITransaction>) {
        if let Some(region) = self.hidden_region_containing(line_number, |r| r.get_hidden_modified_range(None)) {
            region.show_modified_line(line_number, preference, tx);
        }
    }

    pub fn ensure_original_line_is_visible(&self, line_number: i32, preference: RevealPreference, tx: Option<&dyn ITransaction>) {
        if let Some(region) = self.hidden_region_containing(line_number, |r| r.get_hidden_original_range(None)) {
            region.show_original_line(line_number, preference, tx);
        }
    }

    pub fn serialize_state(&self) -> SerializedState {
        let regions = self.unchanged_regions_value.get();
        SerializedState {
            collapsed_regions: regions.map(|state| {
                state.regions.iter().map(|r| SerializedCollapsedRegion { range: r.get_hidden_modified_range(None).serialize() }).collect()
            }),
        }
    }

    pub fn restore_serialized_state(&self, state: &SerializedState) {
        let ranges: Option<Vec<LineRange>> =
            state.collapsed_regions.as_ref().map(|regions| regions.iter().map(|r| LineRange::deserialize(r.range)).collect());
        let regions = self.unchanged_regions_value.get();
        let (Some(regions), Some(ranges)) = (regions, ranges) else {
            return;
        };
        transaction(|tx| {
            for r in regions.regions.iter() {
                for range in &ranges {
                    if r.modified_unchanged_range().intersect(range).is_some() {
                        r.set_hidden_modified_range(*range, tx);
                        break;
                    }
                }
            }
        });
    }
}

impl IDisposable for DiffEditorViewModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IDiffEditorViewModel for DiffEditorViewModel {
    fn model(&self) -> &DiffEditorModel {
        &self.model
    }

    fn wait_for_diff(&self) -> Promise<()> {
        let wait = wait_for_state(self.is_diff_up_to_date.clone(), |s| *s, None, Some(self.cancellation_token_source.token()));
        async_fn(async move {
            catch_if_not_canceled(&wait).await?;
            Ok(())
        })
    }
}

fn normalize_document_diff(diff: &DocumentDiff, original: &TextModel, modified: &TextModel) -> DocumentDiff {
    DocumentDiff {
        changes: diff
            .changes
            .iter()
            .map(|c| {
                DetailedLineRangeMapping::new(
                    c.original,
                    c.modified,
                    c.inner_changes.as_ref().map(|inner| inner.iter().map(|i| normalize_range_mapping(i, original, modified)).collect()),
                )
            })
            .collect(),
        moves: diff.moves.clone(),
        identical: diff.identical,
        quit_early: diff.quit_early,
    }
}

fn normalize_range_mapping(range_mapping: &RangeMapping, original: &TextModel, modified: &TextModel) -> RangeMapping {
    let mut original_range = range_mapping.original_range;
    let mut modified_range = range_mapping.modified_range;
    if original_range.start_column == 1
        && modified_range.start_column == 1
        && (original_range.end_column != 1 || modified_range.end_column != 1)
        && original_range.end_column == original.get_line_max_column(original_range.end_line_number)
        && modified_range.end_column == modified.get_line_max_column(modified_range.end_line_number)
        && original_range.end_line_number < original.get_line_count()
        && modified_range.end_line_number < modified.get_line_count()
    {
        original_range = original_range.set_end_position(original_range.end_line_number + 1, 1);
        modified_range = modified_range.set_end_position(modified_range.end_line_number + 1, 1);
    }
    RangeMapping::new(original_range, modified_range)
}

/// `SerializedState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedState {
    pub collapsed_regions: Option<Vec<SerializedCollapsedRegion>>,
}

/// `{ range: ISerializedLineRange }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedCollapsedRegion {
    pub range: SerializedLineRange,
}

/// `DiffState`.
#[derive(Clone, Debug)]
pub struct DiffState {
    pub mappings: Vec<DiffMapping>,
    pub moved_texts: Vec<MovedText>,
    pub identical: bool,
    pub quit_early: bool,
}

impl DiffState {
    pub fn from_diff_result(result: &DocumentDiff) -> DiffState {
        DiffState {
            mappings: result.changes.iter().map(|c| DiffMapping { line_range_mapping: c.clone() }).collect(),
            moved_texts: result.moves.clone(),
            identical: result.identical,
            quit_early: result.quit_early,
        }
    }
}

/// `DiffMapping`.
#[derive(Clone, Debug)]
pub struct DiffMapping {
    pub line_range_mapping: DetailedLineRangeMapping,
}

/// `'bottom' | 'top'` de `isDragged`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraggedSide {
    Bottom,
    Top,
}

impl StrictEquals for DraggedSide {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// `UnchangedRegion`.
pub struct UnchangedRegion {
    pub original_line_number: i32,
    pub modified_line_number: i32,
    pub line_count: i32,
    pub visible_line_count_top: ISettableObservable<i32>,
    pub visible_line_count_bottom: ISettableObservable<i32>,
    should_hide_controls: IObservable<bool>,
    pub is_dragged: ISettableObservable<Option<DraggedSide>>,
}

impl UnchangedRegion {
    pub fn from_diffs(
        changes: &[DetailedLineRangeMapping],
        original_line_count: i32,
        modified_line_count: i32,
        min_hidden_line_count: i32,
        min_context: i32,
    ) -> Vec<Rc<UnchangedRegion>> {
        let mappings: Vec<LineRangeMapping> = changes.iter().map(|change| LineRangeMapping::new(change.original, change.modified)).collect();
        let inversed_mappings = LineRangeMapping::inverse(&mappings, original_line_count, modified_line_count);
        let mut result: Vec<Rc<UnchangedRegion>> = Vec::new();

        for mapping in inversed_mappings {
            let mut orig_start = mapping.original.start_line_number;
            let mut mod_start = mapping.modified.start_line_number;
            let mut length = mapping.original.length();

            let at_start = orig_start == 1 && mod_start == 1;
            let at_end = orig_start + length == original_line_count + 1 && mod_start + length == modified_line_count + 1;

            if (at_start || at_end) && length >= min_context + min_hidden_line_count {
                if at_start && !at_end {
                    length -= min_context;
                }
                if at_end && !at_start {
                    orig_start += min_context;
                    mod_start += min_context;
                    length -= min_context;
                }
                result.push(UnchangedRegion::new(orig_start, mod_start, length, 0, 0));
            } else if length >= min_context * 2 + min_hidden_line_count {
                orig_start += min_context;
                mod_start += min_context;
                length -= min_context * 2;
                result.push(UnchangedRegion::new(orig_start, mod_start, length, 0, 0));
            }
        }

        result
    }

    pub fn new(original_line_number: i32, modified_line_number: i32, line_count: i32, visible_line_count_top: i32, visible_line_count_bottom: i32) -> Rc<Self> {
        let owner = DebugOwner::new("UnchangedRegion");
        let top = observable_value_owned::<i32, ()>(owner.clone(), 0);
        let bottom = observable_value_owned::<i32, ()>(owner.clone(), 0);
        let is_dragged = observable_value_owned::<Option<DraggedSide>, ()>(owner.clone(), None);
        let should_hide_controls = {
            let (top, bottom, is_dragged) = (top.clone(), bottom.clone(), is_dragged.clone());
            derived(move |reader| top.read(reader) + bottom.read(reader) == line_count && is_dragged.read(reader).is_none())
        };

        let visible_line_count_top2 = visible_line_count_top.min(line_count).max(0);
        let visible_line_count_bottom2 = visible_line_count_bottom.min(line_count - visible_line_count_top).max(0);

        soft_assert(visible_line_count_top == visible_line_count_top2, None);
        soft_assert(visible_line_count_bottom == visible_line_count_bottom2, None);

        top.set(visible_line_count_top2, None);
        bottom.set(visible_line_count_bottom2, None);

        Rc::new(Self {
            original_line_number,
            modified_line_number,
            line_count,
            visible_line_count_top: top,
            visible_line_count_bottom: bottom,
            should_hide_controls,
            is_dragged,
        })
    }

    pub fn original_unchanged_range(&self) -> LineRange {
        LineRange::of_length(self.original_line_number, self.line_count)
    }

    pub fn modified_unchanged_range(&self) -> LineRange {
        LineRange::of_length(self.modified_line_number, self.line_count)
    }

    pub fn set_visible_ranges(self: &Rc<Self>, visible_ranges: &[LineRangeMapping], tx: &dyn ITransaction) -> Vec<Rc<UnchangedRegion>> {
        let mut result: Vec<Rc<UnchangedRegion>> = Vec::new();

        let hidden_modified = LineRangeSet::new(visible_ranges.iter().map(|r| r.modified).collect()).subtract_from(&self.modified_unchanged_range());

        let mut original_start_line_number = self.original_line_number;
        let mut modified_start_line_number = self.modified_line_number;
        let modified_end_line_number_ex = self.modified_line_number + self.line_count;
        if hidden_modified.ranges().is_empty() {
            self.show_all(Some(tx));
            result.push(self.clone());
        } else {
            let mut i = 0;
            for r in hidden_modified.ranges() {
                let is_last = i == hidden_modified.ranges().len() - 1;
                i += 1;

                let length = (if is_last { modified_end_line_number_ex } else { r.end_line_number_exclusive }) - modified_start_line_number;

                let new_r = UnchangedRegion::new(original_start_line_number, modified_start_line_number, length, 0, 0);
                new_r.set_hidden_modified_range(*r, tx);
                result.push(new_r.clone());

                original_start_line_number = new_r.original_unchanged_range().end_line_number_exclusive;
                modified_start_line_number = new_r.modified_unchanged_range().end_line_number_exclusive;
            }
        }

        result
    }

    pub fn should_hide_controls(&self, reader: Option<&dyn IReader>) -> bool {
        self.should_hide_controls.read_opt(reader)
    }

    pub fn get_hidden_original_range(&self, reader: Option<&dyn IReader>) -> LineRange {
        LineRange::of_length(
            self.original_line_number + self.visible_line_count_top.read_opt(reader),
            self.line_count - self.visible_line_count_top.read_opt(reader) - self.visible_line_count_bottom.read_opt(reader),
        )
    }

    pub fn get_hidden_modified_range(&self, reader: Option<&dyn IReader>) -> LineRange {
        LineRange::of_length(
            self.modified_line_number + self.visible_line_count_top.read_opt(reader),
            self.line_count - self.visible_line_count_top.read_opt(reader) - self.visible_line_count_bottom.read_opt(reader),
        )
    }

    pub fn set_hidden_modified_range(&self, range: LineRange, tx: &dyn ITransaction) {
        let visible_line_count_top = range.start_line_number - self.modified_line_number;
        let visible_line_count_bottom = (self.modified_line_number + self.line_count) - range.end_line_number_exclusive;
        self.set_state(visible_line_count_top, visible_line_count_bottom, Some(tx));
    }

    pub fn get_max_visible_line_count_top(&self) -> i32 {
        self.line_count - self.visible_line_count_bottom.get()
    }

    pub fn get_max_visible_line_count_bottom(&self) -> i32 {
        self.line_count - self.visible_line_count_top.get()
    }

    /// `showMoreAbove(count = 10, tx)`.
    pub fn show_more_above(&self, count: Option<i32>, tx: Option<&dyn ITransaction>) {
        let count = count.unwrap_or(10);
        let max_visible_line_count_top = self.get_max_visible_line_count_top();
        self.visible_line_count_top.set((self.visible_line_count_top.get() + count).min(max_visible_line_count_top), tx);
    }

    /// `showMoreBelow(count = 10, tx)`.
    pub fn show_more_below(&self, count: Option<i32>, tx: Option<&dyn ITransaction>) {
        let count = count.unwrap_or(10);
        let max_visible_line_count_bottom = self.line_count - self.visible_line_count_top.get();
        self.visible_line_count_bottom.set((self.visible_line_count_bottom.get() + count).min(max_visible_line_count_bottom), tx);
    }

    pub fn show_all(&self, tx: Option<&dyn ITransaction>) {
        self.visible_line_count_bottom.set(self.line_count - self.visible_line_count_top.get(), tx);
    }

    pub fn show_modified_line(&self, line_number: i32, preference: RevealPreference, tx: Option<&dyn ITransaction>) {
        let top = line_number + 1 - (self.modified_line_number + self.visible_line_count_top.get());
        let bottom = (self.modified_line_number - self.visible_line_count_bottom.get() + self.line_count) - line_number;
        if (preference == RevealPreference::FromCloserSide && top < bottom) || preference == RevealPreference::FromTop {
            self.visible_line_count_top.set(self.visible_line_count_top.get() + top, tx);
        } else {
            self.visible_line_count_bottom.set(self.visible_line_count_bottom.get() + bottom, tx);
        }
    }

    pub fn show_original_line(&self, line_number: i32, preference: RevealPreference, tx: Option<&dyn ITransaction>) {
        let top = line_number - self.original_line_number;
        let bottom = (self.original_line_number + self.line_count) - line_number;
        if (preference == RevealPreference::FromCloserSide && top < bottom) || preference == RevealPreference::FromTop {
            self.visible_line_count_top.set((self.visible_line_count_top.get() + bottom - top).min(self.get_max_visible_line_count_top()), tx);
        } else {
            self.visible_line_count_bottom.set((self.visible_line_count_bottom.get() + top - bottom).min(self.get_max_visible_line_count_bottom()), tx);
        }
    }

    pub fn collapse_all(&self, tx: Option<&dyn ITransaction>) {
        self.visible_line_count_top.set(0, tx);
        self.visible_line_count_bottom.set(0, tx);
    }

    pub fn set_state(&self, visible_line_count_top: i32, visible_line_count_bottom: i32, tx: Option<&dyn ITransaction>) {
        let visible_line_count_top = visible_line_count_top.min(self.line_count).max(0);
        let visible_line_count_bottom = visible_line_count_bottom.min(self.line_count - visible_line_count_top).max(0);

        self.visible_line_count_top.set(visible_line_count_top, tx);
        self.visible_line_count_bottom.set(visible_line_count_bottom, tx);
    }
}

/// `RevealPreference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealPreference {
    FromCloserSide,
    FromTop,
    FromBottom,
}

fn apply_original_edits(_diff: &DocumentDiff, _text_edits: &[TextEditInfo], _original_text_model: &Rc<TextModel>, _modified_text_model: &Rc<TextModel>) -> Option<DocumentDiff> {
    None
}

fn apply_modified_edits(_diff: &DocumentDiff, _text_edits: &[TextEditInfo], _original_text_model: &Rc<TextModel>, _modified_text_model: &Rc<TextModel>) -> Option<DocumentDiff> {
    None
}
