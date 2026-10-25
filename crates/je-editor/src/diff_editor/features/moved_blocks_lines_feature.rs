//! Porte de `src/vs/editor/browser/widget/diffEditor/features/movedBlocksLinesFeature.ts`: as setas entre os dois editores que ligam
//! um bloco de código movido ao lugar de origem e ao de destino, as zonas de visão e os controles ("Code moved to line...",
//! "Compare") que ficam sobre as linhas do bloco, e a escolha do bloco movido ativo a partir do cursor.
//!
//! Divergências:
//! - **DOM vira dado.** O `svg.moved-blocks-lines` é o [`MovedBlocksLinesView`] ([`MovedBlocksLinesFeature::view`]): o estilo do
//!   elemento (`left`, `width`, `height`, gravado pelo autorun de posicionamento) e as setas (`rect.arrow-rectangle`, o `path` e o
//!   `polygon.arrow` de cada uma, em [`MovedBlockLineView`]), que o pintor lê. A classe `currentMove` do `path` e do `polygon` é
//!   [`MovedBlockLineView::current_move`]. O `path` é a poligonal `M 0 from L verticalY from L verticalY to L (right - arrowWidth) to`
//!   e o `polygon` tem os três vértices da ponta.
//! - **`MovedBlockOverlayWidget`.** O `div.diff-moved-code-block` (com o `ActionBar` de duas ações, a legenda desabilitada e o
//!   "Compare") é o [`MovedBlockOverlayView`]; o clique no "Compare" é [`MovedBlockOverlayWidget::run_compare`], que o dono da
//!   janela chama quando o evento nativo chega. O `Codicon.compareChanges` é o `ThemeIcon` `compare-changes`. O `paddingRight`
//!   (`applyStyle`) grava no mesmo dado.
//! - **Identidade de `MovedText`.** O original compara os blocos movidos por identidade (`line.move === model.activeMovedText`);
//!   aqui o `MovedText` do estado do diff é um valor e o ativo é um `Rc` com o mesmo conteúdo, então a comparação é por valor
//!   (dois blocos movidos com o mesmo mapeamento e as mesmas mudanças são o mesmo bloco). O autorun do cursor reaproveita o `Rc` do
//!   bloco ativo anterior quando o valor é igual, para o `setActiveMovedText` não notificar à toa.
//! - **Intervalos em pixels.** O `OffsetRange` dos traços e das trilhas guarda pixels fracionários (`(t1 + t2) / 2`), então são o
//!   [`PixelRange`] e o [`PixelRangeSet`], com a mesma lógica do `OffsetRange` e do `OffsetRangeSet`.
//! - **Layout sem `null`.** `IObservable<EditorLayoutInfo | null>` é `IObservable<EditorLayoutInfo>` (o contrato do widget): a
//!   informação de layout sempre existe, então os ramos `!infoOrig || !infoMod` do original não existem.
//! - **`onDidFocusEditorWidget` com `setTimeout`.** O `observableSignalFromEvent(name, e => ...)` com função de assinatura é o
//!   [`Event`] que assina o evento do editor e dispara um passo adiante, em `setTimeout(0)`.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::rc::Rc;

use je_base::arrays::number_comparator;
use je_base::arrays_find::find_max_idx;
use je_base::event::{Event, Listener};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::observable::change_tracker::FnChangeTracker;
use je_base::observable::observables::observable_from_event::observable_from_event_owned;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value_owned};
use je_base::observable::{
    AnyObservable, DebugNameOptions, DebugOwner, IObservable, IReader, IReaderWithStore, Observable, ObservableExt, SettableExt, autorun,
    autorun_handle_changes, autorun_with_store, const_observable, derived, observable_signal_from_event, recompute_initially_and_on_change,
};
use je_base::runtime::set_timeout;
use je_base::themables::ThemeIcon;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::layout_info::EditorLayoutInfo;
use crate::diff::lines_diff_computer::MovedText;
use crate::diff_editor::components::diff_editor_editors::{DiffEditorEditors, DiffEditorSide};
use crate::diff_editor::feature_contracts::{DiffEditorFeatureContext, IMovedBlocksLinesFeature};
use crate::diff_editor::utils::editor_gutter::{PixelRange, PixelRangeSet};
pub use crate::diff_editor::MOVED_CODE_BLOCK_PADDING;
use crate::diff_editor_utils::{IObservableViewZone, PlaceholderViewZone, ViewZoneOverlayLayout, ViewZoneOverlayWidget, apply_view_zones};
use crate::diff_editor_view_model::DiffEditorViewModel;
use crate::line_range::LineRange;

// #region Estado de visão (o DOM como dado)

/// O `rect.arrow-rectangle` de uma seta.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovedBlockRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Uma seta: o retângulo do lado do destino, o `path` e o `polygon.arrow`.
#[derive(Clone, Debug, PartialEq)]
pub struct MovedBlockLineView {
    pub rect: MovedBlockRect,
    /// Os quatro vértices do `path` (`M ... L ... L ... L ...`), sem preenchimento.
    pub path: [(f64, f64); 4],
    /// Os três vértices do `polygon.arrow`.
    pub arrow: [(f64, f64); 3],
    /// A classe `currentMove` do `path` e do `polygon`.
    pub current_move: bool,
}

/// O que o pintor lê do `svg.moved-blocks-lines`: o estilo do elemento e as setas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MovedBlocksLinesView {
    /// `style.left`.
    pub left: f64,
    /// `style.width`.
    pub width: f64,
    /// `style.height`.
    pub height: f64,
    pub lines: Vec<MovedBlockLineView>,
}

/// O que o pintor lê do `MovedBlockOverlayWidget`.
#[derive(Clone, Debug, PartialEq)]
pub struct MovedBlockOverlayView {
    /// `style.paddingRight` do `div.diff-moved-code-block`.
    pub padding_right: f64,
    /// A legenda ("Code moved to line..."), a ação desabilitada do `ActionBar`.
    pub caption: String,
    /// O texto da ação "Compare" (só rótulo, sem ícone).
    pub compare_label: String,
    /// O `Codicon.compareChanges` da ação.
    pub compare_icon: ThemeIcon,
    /// `actionCompare.checked`.
    pub compare_checked: bool,
}

// #endregion

/// Uma seta ainda sem as coordenadas de desenho: `lines` do original.
struct MovedLine {
    range: PixelRange,
    from: f64,
    to: f64,
    from_without_scroll: f64,
    to_without_scroll: f64,
    moved: MovedText,
}

/// `LinesLayout`.
struct LinesLayout {
    track_count: usize,
    track_per_line_idx: Vec<usize>,
}

impl LinesLayout {
    /// `LinesLayout.compute(lines)`.
    fn compute(lines: &[PixelRange]) -> Self {
        let mut sets_per_track: Vec<PixelRangeSet> = Vec::new();
        let mut track_per_line_idx: Vec<usize> = Vec::new();

        for line in lines {
            let found = sets_per_track.iter().position(|set| !set.intersects_strict(*line));
            let track_idx = match found {
                Some(track_idx) => track_idx,
                None => {
                    let max_track_count = 6;
                    if sets_per_track.len() >= max_track_count {
                        find_max_idx(&sets_per_track, |a, b| {
                            number_comparator(&a.intersect_with_range_length(*line), &b.intersect_with_range_length(*line))
                        }) as usize
                    } else {
                        sets_per_track.push(PixelRangeSet::new());
                        sets_per_track.len() - 1
                    }
                }
            };
            sets_per_track[track_idx].add_range(*line);
            track_per_line_idx.push(track_idx);
        }

        Self { track_count: sets_per_track.len(), track_per_line_idx }
    }

    /// `getTrack(lineIdx)`.
    fn get_track(&self, line_idx: usize) -> usize {
        self.track_per_line_idx[line_idx]
    }

    /// `getTrackCount()`.
    fn get_track_count(&self) -> usize {
        self.track_count
    }
}

/// `computeLineStart(range, editor)`.
fn compute_line_start(range: &LineRange, editor: &Rc<CodeEditorWidget>) -> f64 {
    let t1 = editor.get_top_for_line_number(range.start_line_number, Some(true));
    let t2 = editor.get_top_for_line_number(range.end_line_number_exclusive, Some(true));
    (t1 + t2) / 2.0
}

/// O `observableSignalFromEvent('...HasFocus', e => editor.onDidFocusEditorWidget(() => setTimeout(() => e(undefined), 0)))`.
fn focus_signal(name: &str, editor: &Rc<CodeEditorWidget>) -> IObservable<()> {
    let editor = editor.clone();
    observable_signal_from_event(
        name,
        Event::<()>::new(move |listener: Listener<()>| -> DisposableRef {
            editor.on_did_focus_editor_widget().listen(move |_| {
                let listener = listener.clone();
                let _ = set_timeout(move || listener(&()), 0);
            })
        }),
    )
}

/// Como a ordem das setas pede: primeiro as que descem (`from <= to`), e dentro de cada grupo pela posição de origem (ou de destino).
fn compare_lines(a: &MovedLine, b: &MovedLine) -> Ordering {
    let a_up = a.from_without_scroll > a.to_without_scroll;
    let b_up = b.from_without_scroll > b.to_without_scroll;
    a_up.cmp(&b_up).then_with(|| {
        let key_a = if a_up { a.from_without_scroll } else { -a.to_without_scroll };
        let key_b = if b_up { b.from_without_scroll } else { -b.to_without_scroll };
        key_a.partial_cmp(&key_b).unwrap_or(Ordering::Equal)
    })
}

/// `MovedBlocksLinesFeature`.
pub struct MovedBlocksLinesFeature {
    base: Rc<Disposable>,
    width: Rc<ObservableValue<f64, ()>>,
    element: Rc<RefCell<MovedBlocksLinesView>>,
    overlays: Rc<RefCell<Vec<Rc<MovedBlockOverlayWidget>>>>,
}

impl MovedBlocksLinesFeature {
    /// `new MovedBlocksLinesFeature(rootElement, diffModel, originalEditorLayoutInfo, modifiedEditorLayoutInfo, editors)`, com
    /// `rootElement`, `diffModel` e `editors` do contexto.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        context: &DiffEditorFeatureContext,
        original_editor_layout_info: IObservable<EditorLayoutInfo>,
        modified_editor_layout_info: IObservable<EditorLayoutInfo>,
    ) -> Rc<Self> {
        let owner = DebugOwner::new("MovedBlocksLinesFeature");
        let base = Disposable::new();
        let editors: Rc<DiffEditorEditors> = context.editors.clone();
        let diff_model = context.diff_model.clone();

        let original_scroll_top = {
            let editor = editors.original.clone();
            observable_from_event_owned(owner.clone(), editors.original.on_did_scroll_change(), move |_| editor.get_scroll_top())
        };
        let modified_scroll_top = {
            let editor = editors.modified.clone();
            observable_from_event_owned(owner.clone(), editors.modified.on_did_scroll_change(), move |_| editor.get_scroll_top())
        };
        let view_zones_changed = observable_signal_from_event("onDidChangeViewZones", editors.modified.on_did_change_view_zones());
        let width = observable_value_owned::<f64, ()>(owner.clone(), 0.0);
        let modified_view_zones_changed_signal =
            observable_signal_from_event("modified.onDidChangeViewZones", editors.modified.on_did_change_view_zones());
        let original_view_zones_changed_signal =
            observable_signal_from_event("original.onDidChangeViewZones", editors.original.on_did_change_view_zones());

        let element: Rc<RefCell<MovedBlocksLinesView>> = Rc::default();

        let state = {
            let (diff_model, element, width, editors) = (diff_model.clone(), element.clone(), width.clone(), editors.clone());
            let (original_layout, modified_layout) = (original_editor_layout_info.clone(), modified_editor_layout_info.clone());
            derived(move |derived_reader| {
                // state
                let reader: &dyn IReader = derived_reader;

                element.borrow_mut().lines.clear();
                let model = diff_model.read(reader);
                let moves: Vec<MovedText> =
                    model.as_ref().and_then(|model| model.diff.read(reader)).map(|diff| diff.moved_texts.clone()).unwrap_or_default();
                let Some(model) = model.filter(|_| !moves.is_empty()) else {
                    width.set(0.0, None);
                    return;
                };

                view_zones_changed.read(reader);

                let info_orig = original_layout.read(reader);
                let info_mod = modified_layout.read(reader);

                modified_view_zones_changed_signal.read(reader);
                original_view_zones_changed_signal.read(reader);

                let mut lines: Vec<MovedLine> = moves
                    .into_iter()
                    .map(|moved| {
                        let start = compute_line_start(&moved.line_range_mapping.original, &editors.original);
                        let start_offset = original_scroll_top.read(reader);
                        let end = compute_line_start(&moved.line_range_mapping.modified, &editors.modified);
                        let end_offset = modified_scroll_top.read(reader);

                        let from = start - start_offset;
                        let to = end - end_offset;

                        let top = start.min(end);
                        let bottom = start.max(end);

                        MovedLine {
                            range: PixelRange { start: top, end_exclusive: bottom },
                            from,
                            to,
                            from_without_scroll: start,
                            to_without_scroll: end,
                            moved,
                        }
                    })
                    .collect();

                lines.sort_by(compare_lines);

                let ranges: Vec<PixelRange> = lines.iter().map(|l| l.range).collect();
                let layout = LinesLayout::compute(&ranges);

                let padding = 10.0;
                let line_area_left = info_orig.vertical_scrollbar_width;
                let line_area_width = (layout.get_track_count() as f64 - 1.0) * 10.0 + padding * 2.0;
                let total_width = line_area_left + line_area_width + (info_mod.content_left - f64::from(MOVED_CODE_BLOCK_PADDING));

                for (idx, line) in lines.iter().enumerate() {
                    let track = layout.get_track(idx);
                    let vertical_y = line_area_left + padding + track as f64 * 10.0;

                    let arrow_height = 15.0;
                    let arrow_width = 15.0;
                    let right = total_width;

                    let rect_width = info_mod.glyph_margin_width + info_mod.line_numbers_width;
                    let rect_height = 18.0;

                    element.borrow_mut().lines.push(MovedBlockLineView {
                        rect: MovedBlockRect { x: right - rect_width, y: line.to - rect_height / 2.0, width: rect_width, height: rect_height },
                        path: [(0.0, line.from), (vertical_y, line.from), (vertical_y, line.to), (right - arrow_width, line.to)],
                        arrow: [(right - arrow_width, line.to - arrow_height / 2.0), (right, line.to), (right - arrow_width, line.to + arrow_height / 2.0)],
                        current_move: false,
                    });

                    let (element, model, moved) = (element.clone(), model.clone(), line.moved.clone());
                    derived_reader.store().add(autorun(move |reader| {
                        let reader: &dyn IReader = reader;
                        let current_move = model.active_moved_text.read(reader).as_deref() == Some(&moved);
                        if let Some(view) = element.borrow_mut().lines.get_mut(idx) {
                            view.current_move = current_move;
                        }
                    }));

                    // O original deixa comentado o `mouseenter`/`mouseleave` do `path` chamando `model.setHoveredMovedText(...)`.
                }

                width.set(line_area_width, None);
            })
        };

        {
            // update moved blocks lines positioning
            let (element, width, original_layout, modified_layout) =
                (element.clone(), width.clone(), original_editor_layout_info.clone(), modified_editor_layout_info.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let info = original_layout.read(reader);
                let info2 = modified_layout.read(reader);

                let mut element = element.borrow_mut();
                element.left = info.width - info.vertical_scrollbar_width;
                element.height = info.height;
                element.width = info.vertical_scrollbar_width + info2.content_left - f64::from(MOVED_CODE_BLOCK_PADDING) + width.read(reader);
            }));
        }

        base.register(recompute_initially_and_on_change(&state, None));

        let moved_block_view_zones: IObservable<Rc<Vec<MovedBlockViewZones>>> = {
            let diff_model = diff_model.clone();
            derived(move |reader| {
                let reader: &dyn IReader = reader;
                let model = diff_model.read(reader);
                let Some(diff) = model.and_then(|model| model.diff.read(reader)) else { return Rc::new(Vec::new()) };
                Rc::new(
                    diff.moved_texts
                        .iter()
                        .map(|moved| MovedBlockViewZones {
                            moved: Rc::new(moved.clone()),
                            original: PlaceholderViewZone::new(const_observable(moved.line_range_mapping.original.start_line_number - 1), 18.0),
                            modified: PlaceholderViewZone::new(const_observable(moved.line_range_mapping.modified.start_line_number - 1), 18.0),
                        })
                        .collect(),
                )
            })
        };

        base.register(apply_view_zones(
            &editors.original,
            moved_block_view_zones.map(|zones, _| {
                // movedBlockViewZones.original
                Rc::new(zones.iter().map(|z| z.original.clone() as Rc<dyn IObservableViewZone>).collect::<Vec<_>>())
            }),
            None,
            None,
        ));
        base.register(apply_view_zones(
            &editors.modified,
            moved_block_view_zones.map(|zones, _| {
                // movedBlockViewZones.modified
                Rc::new(zones.iter().map(|z| z.modified.clone() as Rc<dyn IObservableViewZone>).collect::<Vec<_>>())
            }),
            None,
            None,
        ));

        let overlays: Rc<RefCell<Vec<Rc<MovedBlockOverlayWidget>>>> = Rc::default();
        {
            let (editors, diff_model, overlays) = (editors.clone(), diff_model.clone(), overlays.clone());
            base.register(autorun_with_store(move |reader, store| {
                let blocks = moved_block_view_zones.read(reader);
                let mut current: Vec<Rc<MovedBlockOverlayWidget>> = Vec::new();
                for b in blocks.iter() {
                    let model = diff_model.get().expect("o modelo do diff existe quando há blocos movidos");
                    let original = MovedBlockOverlayWidget::new(&editors.original, &b.original, b.moved.clone(), DiffEditorSide::Original, model.clone());
                    store.add(original.clone());
                    current.push(original);
                    let modified = MovedBlockOverlayWidget::new(&editors.modified, &b.modified, b.moved.clone(), DiffEditorSide::Modified, model.clone());
                    store.add(modified.clone());
                    current.push(modified);
                }
                *overlays.borrow_mut() = current;
            }));
        }

        let original_has_focus = focus_signal("original.onDidFocusEditorWidget", &editors.original);
        let modified_has_focus = focus_signal("modified.onDidFocusEditorWidget", &editors.modified);

        let last_changed_editor = Rc::new(Cell::new(DiffEditorSide::Modified));
        let last_active: Rc<RefCell<Option<Rc<MovedText>>>> = Rc::default();
        {
            let (editors, diff_model) = (editors.clone(), diff_model.clone());
            let (original_has_focus_id, modified_has_focus_id) = (original_has_focus.id(), modified_has_focus.id());
            let tracker_last_changed_editor = last_changed_editor.clone();
            base.register(autorun_handle_changes::<()>(
                DebugNameOptions::named("MovedBlocksLines.setActiveMovedTextFromCursor"),
                FnChangeTracker::<()>::new(
                    |_| (),
                    move |ctx, _| {
                        let changed = ctx.changed_observable().id();
                        if changed == original_has_focus_id {
                            tracker_last_changed_editor.set(DiffEditorSide::Original);
                        }
                        if changed == modified_has_focus_id {
                            tracker_last_changed_editor.set(DiffEditorSide::Modified);
                        }
                        true
                    },
                ),
                move |reader, _| {
                    // MovedBlocksLines.setActiveMovedTextFromCursor
                    original_has_focus.read(reader);
                    modified_has_focus.read(reader);

                    let Some(m) = diff_model.read(reader) else { return };
                    let diff = m.diff.read(reader);

                    let mut moved_text: Option<MovedText> = None;

                    if let Some(diff) = &diff {
                        match last_changed_editor.get() {
                            DiffEditorSide::Original => {
                                let original_pos = editors.original_cursor.read(reader);
                                moved_text = diff.moved_texts.iter().find(|mt| mt.line_range_mapping.original.contains(original_pos.line_number)).cloned();
                            }
                            DiffEditorSide::Modified => {
                                let modified_pos = editors.modified_cursor.read(reader);
                                moved_text = diff.moved_texts.iter().find(|mt| mt.line_range_mapping.modified.contains(modified_pos.line_number)).cloned();
                            }
                        }
                    }

                    if moved_text.as_ref() != m.moved_text_to_compare.get().as_deref() {
                        m.moved_text_to_compare.set(None, None);
                    }
                    let active: Option<Rc<MovedText>> = moved_text.map(|found| match last_active.borrow().as_ref() {
                        Some(last) if **last == found => last.clone(),
                        _ => Rc::new(found),
                    });
                    *last_active.borrow_mut() = active.clone();
                    m.set_active_moved_text(active);
                },
            ));
        }

        {
            let element = element.clone();
            base.register(to_disposable(move || element.borrow_mut().lines.clear()));
        }

        Rc::new(Self { base, width, element, overlays })
    }

    /// O que o pintor lê do `svg.moved-blocks-lines`.
    pub fn view(&self) -> MovedBlocksLinesView {
        self.element.borrow().clone()
    }

    /// Os controles sobre os blocos movidos (dois por bloco: o do original e o do modificado).
    pub fn overlays(&self) -> Vec<Rc<MovedBlockOverlayWidget>> {
        self.overlays.borrow().clone()
    }
}

impl IMovedBlocksLinesFeature for MovedBlocksLinesFeature {
    fn width(&self) -> IObservable<f64> {
        self.width.to_observable()
    }
}

impl IDisposable for MovedBlocksLinesFeature {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `{ move, original, modified }` de `movedBlockViewZones`.
struct MovedBlockViewZones {
    moved: Rc<MovedText>,
    original: Rc<PlaceholderViewZone>,
    modified: Rc<PlaceholderViewZone>,
}

/// `MovedBlockOverlayWidget`.
pub struct MovedBlockOverlayWidget {
    base: ViewZoneOverlayWidget,
    editor: Rc<CodeEditorWidget>,
    moved: Rc<MovedText>,
    diff_model: Rc<DiffEditorViewModel>,
    view: Rc<RefCell<MovedBlockOverlayView>>,
}

impl MovedBlockOverlayWidget {
    /// `new MovedBlockOverlayWidget(editor, viewZone, move, kind, diffModel)`.
    pub fn new(
        editor: &Rc<CodeEditorWidget>,
        view_zone: &PlaceholderViewZone,
        moved: Rc<MovedText>,
        kind: DiffEditorSide,
        diff_model: Rc<DiffEditorViewModel>,
    ) -> Rc<Self> {
        let base = ViewZoneOverlayWidget::new(editor, view_zone);

        let editor_layout = {
            let editor = editor.clone();
            observable_from_event_owned(DebugOwner::new("MovedBlockOverlayWidget"), editor.on_did_layout_change(), move |_| editor.get_layout_info())
        };

        let mapping = &moved.line_range_mapping;
        let (to_start, to_end) = (mapping.modified.start_line_number.to_string(), (mapping.modified.end_line_number_exclusive - 1).to_string());
        let (from_start, from_end) = (mapping.original.start_line_number.to_string(), (mapping.original.end_line_number_exclusive - 1).to_string());
        let (to_args, from_args) = ([to_start.as_str(), to_end.as_str()], [from_start.as_str(), from_end.as_str()]);
        let caption = if !moved.changes.is_empty() {
            if kind == DiffEditorSide::Original {
                localize("codeMovedToWithChanges", "Code moved with changes to line {0}-{1}", &to_args)
            } else {
                localize("codeMovedFromWithChanges", "Code moved with changes from line {0}-{1}", &from_args)
            }
        } else if kind == DiffEditorSide::Original {
            localize("codeMovedTo", "Code moved to line {0}-{1}", &to_args)
        } else {
            localize("codeMovedFrom", "Code moved from line {0}-{1}", &from_args)
        };

        let view = Rc::new(RefCell::new(MovedBlockOverlayView {
            padding_right: 0.0,
            caption,
            compare_label: "Compare".to_owned(),
            compare_icon: ThemeIcon::from_id("compare-changes"),
            compare_checked: false,
        }));

        {
            // applyStyle(this._nodes.root, { paddingRight: editorLayout.map(l => l.verticalScrollbarWidth) })
            let (view, padding_right) = (view.clone(), editor_layout.map(|l, _| l.vertical_scrollbar_width));
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                view.borrow_mut().padding_right = padding_right.read(reader);
            }));
        }
        {
            let (view, diff_model, moved) = (view.clone(), diff_model.clone(), moved.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let is_active = diff_model.moved_text_to_compare.read(reader).as_deref() == Some(&*moved);
                view.borrow_mut().compare_checked = is_active;
            }));
        }

        Rc::new(Self { base, editor: editor.clone(), moved, diff_model, view })
    }

    /// O que o pintor lê do controle.
    pub fn view(&self) -> MovedBlockOverlayView {
        self.view.borrow().clone()
    }

    /// O topo e a altura do overlay, como a zona de visão os deu.
    pub fn layout(&self) -> ViewZoneOverlayLayout {
        self.base.layout()
    }

    /// O id do overlay no editor.
    pub fn overlay_widget_id(&self) -> &str {
        self.base.overlay_widget_id()
    }

    /// A ação "Compare" (o `run` do `actionCompare`): foca o editor e alterna o bloco a comparar.
    pub fn run_compare(&self) {
        self.editor.focus();
        let is_current = self.diff_model.moved_text_to_compare.get().as_deref() == Some(&*self.moved);
        self.diff_model.moved_text_to_compare.set(if is_current { None } else { Some(self.moved.clone()) }, None);
    }
}

impl IDisposable for MovedBlockOverlayWidget {
    fn dispose(&self) {
        self.base.dispose();
    }
}
