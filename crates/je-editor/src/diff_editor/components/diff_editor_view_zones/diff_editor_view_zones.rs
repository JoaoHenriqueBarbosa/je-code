//! Porte de `src/vs/editor/browser/widget/diffEditor/components/diffEditorViewZones/diffEditorViewZones.ts`: o [`DiffEditorViewZones`],
//! que iguala a altura dos dois editores alinhando as linhas sem mudança, mostra no editor modificado, no modo em linha, o código
//! apagado do modelo original (zonas de visão com o `renderLines` e a margem de [`InlineDiffDeletedCodeMargin`]), alinha o texto
//! movido e sincroniza a rolagem. As zonas só valem depois de entrarem nos editores (`applyViewZones` do widget).
//!
//! Divergências:
//! - **DOM vira dado.** Os `HTMLElement` das zonas são os tipos de nó deste módulo, que o pintor lê por `downcast` do
//!   [`IViewZone::dom_node`] e do [`IViewZone::margin_dom_node`]: [`DiagonalFillNode`] (`div.diagonal-fill`), [`EmptyViewZoneNode`]
//!   (o `div` vazio do preenchimento do topo), [`DeletedCodeNode`] (`div.view-lines.line-delete...`, com o [`RenderLinesResult`] que o
//!   original escreve no `innerHTML`), [`DeletedCodeMarginNode`] (`div.inline-deleted-margin-view-zone`, com a fonte aplicada pelo
//!   `applyFontInfo` e os `delete-sign` posicionados), [`GutterDeleteMarginNode`] (`div.gutter-delete`) e [`RevertChangeArrow`] (a
//!   seta `arrow-revert-change`, cujos ouvintes de `mousedown` e `click` são `on_mouse_down` e `on_click`; o `stopPropagation()` é o
//!   retorno `true`, e depois do descarte da zona nenhum dos dois faz nada, o `removeEventListener`).
//! - **Topo da margem na janela.** O `getDomNodePagePosition(marginDomNode)` da [`InlineDiffDeletedCodeMargin`] só o pintor sabe: o
//!   [`DeletedCodeMarginNode::set_page_top`] grava o valor que a margem consulta.
//! - **Zona e id.** As zonas literais do original são todas a mesma `DiffViewZone` (a que tem `setZoneId` guarda o id numa célula que a
//!   margem do código apagado lê, o `assertReturnsDefined(zoneId)`).
//! - **Serviços e janela.** `@IClipboardService` e `@IContextMenuService` chegam por [`Injectable`], como o `createInstance` do
//!   widget. O `Window` é o [`CodeWindow`] que o criador passa, e o `DiffEditorWidget` é o `Weak` do contexto das features (as
//!   features nascem antes de o `Rc` do widget existir).
//! - **`derived` com `reader.store`.** As zonas são um `derivedDisposable` ([`derived_disposable_with`]): a margem do código apagado,
//!   a seta e os ouvintes dela vão para o `DisposableStore` descartado no recálculo seguinte, como o `alignmentViewZonesDisposables`
//!   mais o `reader.store`.
//! - **Retorno antecipado.** O `return { orig, mod }` do original quando o diff ainda não acompanhou a edição (linha fora do modelo
//!   original) é o [`ControlFlow::Break`] do preenchimento das zonas e o `None` da fila de quebras de linha.
//! - **`Number.MAX_VALUE` e `Number.MAX_SAFE_INTEGER`** dos números de linha são `i32::MAX`/`i64::MAX`: nenhuma linha chega perto.
//! - **`setTimeout`** do preenchimento do topo é o [`runtime::set_timeout`] do `je-base` (o relógio que os testes falsificam).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ops::ControlFlow;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::arrays::ArrayQueue;
use je_base::async_::RunOnceScheduler;
use je_base::browser::dom::CodeWindow;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::observable::observables::derived::derived_owned;
use je_base::observable::observables::observable_from_event::observable_from_event_owned;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value, observable_value_owned};
use je_base::observable::{DebugOwner, IObservable, IReader, Observable, ObservableExt, Settable, SettableExt, autorun};
use je_base::runtime;
use je_base::themables::ThemeIcon;
use je_instantiation::Injectable;
use je_platform::contextview::context_view::IContextMenuService;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor, IScrollEvent};
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::wrapping_info::WRAPPING_INFO;
use crate::config::editor_options::EditorOption;
use crate::config::font_info::FontInfo;
use crate::cursor_common::CursorSimpleModel;
use crate::diff::lines_diff_computer::MovedText;
use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::diff_editor::components::diff_editor_editors::DiffEditorEditors;
use crate::diff_editor::components::diff_editor_view_zones::inline_diff_deleted_code_margin::{
    InlineDiffDeletedCodeMargin, InlineDiffDeletedCodeMarginArgs,
};
use crate::diff_editor::components::diff_editor_view_zones::render_lines::{LineSource, RenderLinesResult, RenderOptions, render_lines};
use crate::diff_editor::diff_editor_widget::DiffEditorWidget;
use crate::diff_editor::feature_contracts::{IDiffEditorViewZones, IsUpdatingViewZones, ObservableViewZonePair};
use crate::diff_editor::registrations_contribution::{diff_decoration_options, diff_remove_icon};
use crate::diff_editor_options::DiffEditorOptions;
use crate::diff_editor_utils::{IObservableViewZone, ViewZoneIdSet, animated_observable, derived_disposable_with, join_combine};
use crate::diff_editor_view_model::{DiffEditorViewModel, DiffMapping};
use crate::editor_browser::IViewZone;
use crate::editor_common::ScrollType;
use crate::inline_decorations::{InlineDecoration, InlineDecorationType};
use crate::line_range::LineRange;
use crate::model_line_projection_data::{LineBreaksComputerContext, ModelLineProjectionData};
use crate::position::Position;
use crate::range::Range;
use crate::services::clipboard_service::IClipboardService;
use crate::text_model::TextModel;
use crate::text_model_events::LineInjectedText;
use crate::tokenizer_syntax_token_backend::BackgroundTokenizationState;
use crate::view_model_types::IViewModel;

// #region Nós de visão (o DOM como dado)

/// `div.diagonal-fill`: o preenchimento diagonal das zonas de alinhamento (`createFakeLinesDiv`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiagonalFillNode;

/// O `document.createElement('div')` vazio do preenchimento do topo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmptyViewZoneNode;

/// As classes do `deletedCodeDomNode`.
pub const DELETED_CODE_CLASS_NAMES: &str = "view-lines line-delete line-delete-selectable monaco-mouse-cursor-text";

/// O `deletedCodeDomNode`: as linhas apagadas que o `renderLines` renderizou e a margem, de que vem o tratamento de `copy`
/// ([`InlineDiffDeletedCodeMargin::copy_selection`]).
pub struct DeletedCodeNode {
    pub render_lines_result: Rc<RenderLinesResult>,
    pub margin: Rc<InlineDiffDeletedCodeMargin>,
}

/// Um `div.delete-sign` da margem: a posição absoluta que o `setAttribute('style', ...)` do original grava.
#[derive(Clone, Debug, PartialEq)]
pub struct DeleteSign {
    /// As classes (`delete-sign` mais as do ícone `diffRemoveIcon`).
    pub class_name: String,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// O `marginDomNode` do código apagado (`div.inline-deleted-margin-view-zone`).
pub struct DeletedCodeMarginNode {
    /// A fonte que o `applyFontInfo` aplica.
    pub font_info: FontInfo,
    pub delete_signs: Vec<DeleteSign>,
    /// A margem, com a lâmpada de ações (visibilidade, topo e menu).
    pub margin: Rc<InlineDiffDeletedCodeMargin>,
    page_top: Rc<Cell<f64>>,
}

impl DeletedCodeMarginNode {
    /// A classe do `div`.
    pub const CLASS_NAME: &'static str = "inline-deleted-margin-view-zone";

    /// O topo da margem na janela (o `getDomNodePagePosition(marginDomNode).top`), que só o pintor conhece.
    pub fn set_page_top(&self, page_top: f64) {
        self.page_top.set(page_top);
    }
}

/// O `marginDomNode` `div.gutter-delete` da zona de preenchimento do original.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GutterDeleteMarginNode;

/// A seta que reverte uma mudança (`createViewZoneMarginArrow`): o `div` com a `div.arrow-revert-change` dentro.
pub struct RevertChangeArrow {
    diff: DetailedLineRangeMapping,
    editor: Weak<DiffEditorWidget>,
    disposed: Cell<bool>,
}

impl RevertChangeArrow {
    /// As classes da seta: `'arrow-revert-change ' + ThemeIcon.asClassName(Codicon.arrowRight)`.
    pub fn class_name() -> String {
        format!("arrow-revert-change {}", ThemeIcon::from_id("arrow-right").as_class_name())
    }

    /// O ouvinte de `mousedown`: devolve se consumiu o evento (`e.stopPropagation()`).
    pub fn on_mouse_down(&self) -> bool {
        !self.disposed.get()
    }

    /// O ouvinte de `click`: reverte a mudança e devolve se consumiu o evento (`e.stopPropagation()`). Com o widget já descartado, não
    /// reverte nada.
    pub fn on_click(&self) -> bool {
        if self.disposed.get() {
            return false;
        }
        if let Some(editor) = self.editor.upgrade() {
            editor.revert(&self.diff);
        }
        true
    }
}

// #endregion

/// Uma zona de visão do alinhamento. As zonas literais do original diferem só nos campos que preenchem.
struct DiffViewZone {
    after_line_number: i32,
    height_in_px: f64,
    min_width_in_px: Option<f64>,
    dom_node: Rc<dyn Any>,
    margin_dom_node: Option<Rc<dyn Any>>,
    suppress_mouse_down: bool,
    /// O que o `setZoneId` recebe.
    zone_id: Rc<RefCell<Option<String>>>,
}

impl DiffViewZone {
    /// A zona de preenchimento diagonal (`createFakeLinesDiv`) que não captura o mouse e aparece mesmo em áreas escondidas.
    fn filler(after_line_number: i32, height_in_px: f64, margin_dom_node: Option<Rc<dyn Any>>) -> Rc<dyn IObservableViewZone> {
        Rc::new(Self {
            after_line_number,
            height_in_px,
            min_width_in_px: None,
            dom_node: Rc::new(DiagonalFillNode),
            margin_dom_node,
            suppress_mouse_down: true,
            zone_id: Rc::default(),
        })
    }

    /// O preenchimento do topo, com um `div` vazio.
    fn top_padding(height_in_px: f64) -> Rc<dyn IObservableViewZone> {
        Rc::new(Self {
            after_line_number: 0,
            height_in_px,
            min_width_in_px: None,
            dom_node: Rc::new(EmptyViewZoneNode),
            margin_dom_node: None,
            suppress_mouse_down: true,
            zone_id: Rc::default(),
        })
    }
}

impl IViewZone for DiffViewZone {
    fn after_line_number(&self) -> i32 {
        self.after_line_number
    }

    fn show_in_hidden_areas(&self) -> bool {
        true
    }

    fn suppress_mouse_down(&self) -> bool {
        self.suppress_mouse_down
    }

    fn height_in_px(&self) -> Option<f64> {
        Some(self.height_in_px)
    }

    fn min_width_in_px(&self) -> Option<f64> {
        self.min_width_in_px
    }

    fn dom_node(&self) -> Rc<dyn Any> {
        self.dom_node.clone()
    }

    fn margin_dom_node(&self) -> Option<Rc<dyn Any>> {
        self.margin_dom_node.clone()
    }
}

impl IObservableViewZone for DiffViewZone {
    fn set_zone_id(&self, zone_id: &str) {
        *self.zone_id.borrow_mut() = Some(zone_id.to_owned());
    }
}

/// `ILineRangeAlignment`.
#[derive(Clone)]
struct LineRangeAlignment {
    original_range: LineRange,
    modified_range: LineRange,

    // accounts for foreign viewzones and line wrapping
    original_height_in_px: f64,
    modified_height_in_px: f64,

    /// If this range alignment is a direct result of a diff, then this is the diff's line mapping.
    /// Only used for inline-view.
    diff: Option<DetailedLineRangeMapping>,
}

/// `AdditionalLineHeightInfo`.
#[derive(Clone, Copy)]
struct AdditionalLineHeightInfo {
    line_number: i32,
    height_in_px: f64,
}

/// As duas listas de zonas em construção (`origViewZones` e `modViewZones`).
#[derive(Default)]
struct ZoneLists {
    orig: Vec<Rc<dyn IObservableViewZone>>,
    modified: Vec<Rc<dyn IObservableViewZone>>,
}

impl ZoneLists {
    fn into_pair(self) -> Rc<ObservableViewZonePair> {
        Rc::new(ObservableViewZonePair { orig: self.orig, modified: self.modified })
    }
}

/// O contexto do `ILineBreaksComputer` das linhas apagadas: o texto do modelo original, sem texto injetado.
struct OriginalLineBreaksContext {
    model: Rc<TextModel>,
}

impl LineBreaksComputerContext for OriginalLineBreaksContext {
    fn get_line_content(&self, line_number: i32) -> JsString {
        self.model.get_line_content(line_number)
    }

    fn get_line_injected_text(&self, _line_number: i32) -> Option<Vec<LineInjectedText>> {
        None
    }
}

/// O `lineBreakData` e o `lineBreakDataIdx` do recálculo das zonas: cada linha apagada consome a próxima entrada.
struct DeletedCodeLineBreaks {
    data: Vec<Option<ModelLineProjectionData>>,
    next: usize,
}

impl DeletedCodeLineBreaks {
    /// `lineBreakData[lineBreakDataIdx++]`.
    fn take_next(&mut self) -> Option<ModelLineProjectionData> {
        let data = self.data.get_mut(self.next).and_then(|slot| slot.take());
        self.next += 1;
        data
    }
}

/// O que o recálculo das zonas calcula uma vez e as zonas em linha consomem.
struct InlineRenderEnv {
    original_model: Option<Rc<TextModel>>,
    render_options: RenderOptions,
    modified_line_height: f64,
    might_contain_non_basic_ascii: bool,
    might_contain_rtl: bool,
    /// `diffDeleteDecoration.className!`.
    delete_decoration_class_name: String,
    /// `ThemeIcon.asClassName(diffRemoveIcon)`.
    remove_icon_class_name: String,
}

/// Ensures both editors have the same height by aligning unchanged lines.
/// In inline view mode, inserts viewzones to show deleted code from the original text model in the modified code editor.
/// Synchronizes scrolling.
///
/// Make sure to add the view zones!
pub struct DiffEditorViewZones {
    base: Rc<Disposable>,
    view_zones: IObservable<Rc<ObservableViewZonePair>>,
}

/// Os argumentos estáticos do construtor de `DiffEditorViewZones` (os decorados `@IClipboardService` e `@IContextMenuService` vêm
/// por [`Injectable`]).
pub struct DiffEditorViewZonesArgs {
    pub target_window: Rc<CodeWindow>,
    pub editors: Rc<DiffEditorEditors>,
    pub diff_model: IObservable<Option<Rc<DiffEditorViewModel>>>,
    pub options: Rc<DiffEditorOptions>,
    pub diff_editor_widget: Weak<DiffEditorWidget>,
    pub can_ignore_view_zone_update_event: IsUpdatingViewZones,
    pub orig_view_zones_to_ignore: ViewZoneIdSet,
    pub mod_view_zones_to_ignore: ViewZoneIdSet,
}

impl Injectable for DiffEditorViewZones {
    type Args = DiffEditorViewZonesArgs;
    type Deps = (Rc<dyn IClipboardService>, Rc<dyn IContextMenuService>);

    fn create(args: Self::Args, (clipboard_service, context_menu_service): Self::Deps) -> Rc<Self> {
        Self::new(args, clipboard_service, context_menu_service)
    }
}

impl DiffEditorViewZones {
    #[allow(clippy::too_many_lines)]
    pub fn new(args: DiffEditorViewZonesArgs, clipboard_service: Rc<dyn IClipboardService>, context_menu_service: Rc<dyn IContextMenuService>) -> Rc<Self> {
        let DiffEditorViewZonesArgs {
            target_window,
            editors,
            diff_model,
            options,
            diff_editor_widget,
            can_ignore_view_zone_update_event,
            orig_view_zones_to_ignore,
            mod_view_zones_to_ignore,
        } = args;
        let owner = DebugOwner::new("DiffEditorViewZones");
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());

        let original_top_padding: Rc<ObservableValue<f64, ()>> = observable_value_owned(owner.clone(), 0.0);
        let original_scroll_offset: Rc<ObservableValue<f64, bool>> = observable_value_owned(owner.clone(), 0.0);
        let original_scroll_offset_animated = animated_observable(&target_window, &original_scroll_offset, &store);
        let modified_top_padding: Rc<ObservableValue<f64, ()>> = observable_value_owned(owner.clone(), 0.0);
        let modified_scroll_offset: Rc<ObservableValue<f64, bool>> = observable_value_owned(owner.clone(), 0.0);
        let modified_scroll_offset_animated = animated_observable(&target_window, &modified_scroll_offset, &store);

        let state: Rc<ObservableValue<i32, ()>> = observable_value("invalidateAlignmentsState", 0);

        let update_immediately = {
            let state = state.clone();
            RunOnceScheduler::new(move || state.set(state.get() + 1, None), 0)
        };
        base.register(update_immediately.clone());

        for editor in [&editors.original, &editors.modified] {
            {
                let (update_immediately, can_ignore) = (update_immediately.clone(), can_ignore_view_zone_update_event.clone());
                base.register(editor.on_did_change_view_zones().listen(move |_| {
                    if !can_ignore() {
                        update_immediately.schedule(None);
                    }
                }));
            }
            let update_immediately = update_immediately.clone();
            base.register(editor.on_did_change_configuration().listen(move |args| {
                if args.has_changed(EditorOption::WrappingInfo) || args.has_changed(EditorOption::LineHeight) {
                    update_immediately.schedule(None);
                }
            }));
        }

        let original_model_tokenization_completed: IObservable<Option<bool>> = {
            let owner = owner.clone();
            diff_model
                .map(move |m, _reader| {
                    m.map(|m| {
                        let model = m.model.original.clone();
                        observable_from_event_owned(owner.clone(), model.on_did_change_tokens(), {
                            let model = model.clone();
                            move |_| model.tokenization().background_tokenization_state() == BackgroundTokenizationState::Completed
                        })
                    })
                })
                .map(|m, reader| m.map(|m| m.read(reader)))
        };

        let alignments: IObservable<Option<Rc<Vec<LineRangeAlignment>>>> = {
            let (diff_model, options, editors, state) = (diff_model.clone(), options.clone(), editors.clone(), state.clone());
            let (orig_ids, mod_ids) = (orig_view_zones_to_ignore.clone(), mod_view_zones_to_ignore.clone());
            derived_owned(owner.clone(), move |reader| {
                // alignments
                let reader: &dyn IReader = reader;
                let diff_model = diff_model.read(reader);
                let diff = diff_model.as_ref().and_then(|m| m.diff.read(reader));
                let (Some(_), Some(diff)) = (diff_model, diff) else { return None };
                state.read(reader);
                let render_side_by_side = options.render_side_by_side.read(reader);
                let inner_hunk_alignment = render_side_by_side;
                Some(Rc::new(compute_range_alignment(
                    &editors.original,
                    &editors.modified,
                    &diff.mappings,
                    &orig_ids.borrow(),
                    &mod_ids.borrow(),
                    inner_hunk_alignment,
                )))
            })
        };

        let alignments_synced_moved_text: IObservable<Option<Rc<Vec<LineRangeAlignment>>>> = {
            let (diff_model, editors, state) = (diff_model.clone(), editors.clone(), state.clone());
            let (orig_ids, mod_ids) = (orig_view_zones_to_ignore.clone(), mod_view_zones_to_ignore.clone());
            derived_owned(owner.clone(), move |reader| {
                // alignmentsSyncedMovedText
                let reader: &dyn IReader = reader;
                let synced_moved_text = diff_model.read(reader).and_then(|m| m.moved_text_to_compare.read(reader))?;
                state.read(reader);
                let mappings: Vec<DiffMapping> = synced_moved_text.changes.iter().map(|c| DiffMapping { line_range_mapping: c.clone() }).collect();
                // TODO dont include alignments outside syncedMovedText
                Some(Rc::new(compute_range_alignment(&editors.original, &editors.modified, &mappings, &orig_ids.borrow(), &mod_ids.borrow(), true)))
            })
        };

        let computer = Rc::new(ViewZonesComputer {
            editors: editors.clone(),
            diff_model: diff_model.clone(),
            options: options.clone(),
            diff_editor_widget,
            clipboard_service,
            context_menu_service,
            original_top_padding: original_top_padding.clone(),
            modified_top_padding: modified_top_padding.clone(),
            alignments,
            alignments_synced_moved_text,
            original_model_tokenization_completed,
        });
        let view_zones = derived_disposable_with(Some(owner.clone()), move |reader| {
            let alignment_view_zones_disposables = DisposableStore::new();
            let pair = computer.compute(reader, &alignment_view_zones_disposables);
            let disposable: DisposableRef = alignment_view_zones_disposables;
            (pair, Some(disposable))
        });

        let ignore_change = Rc::new(Cell::new(false));
        for (source, target) in [(&editors.original, &editors.modified), (&editors.modified, &editors.original)] {
            let (ignore_change, target) = (ignore_change.clone(), target.clone());
            base.register(source.on_did_scroll_change().listen(move |e: &IScrollEvent| {
                if e.scroll_left_changed && !ignore_change.get() {
                    ignore_change.set(true);
                    target.set_scroll_left(e.scroll_left, None);
                    ignore_change.set(false);
                }
            }));
        }

        let original_scroll_top: IObservable<f64> = {
            let editor = editors.original.clone();
            observable_from_event_owned(owner.clone(), editors.original.on_did_scroll_change(), move |_| /* original.getScrollTop */ editor.get_scroll_top())
        };
        let modified_scroll_top: IObservable<f64> = {
            let editor = editors.modified.clone();
            observable_from_event_owned(owner.clone(), editors.modified.on_did_scroll_change(), move |_| /* modified.getScrollTop */ editor.get_scroll_top())
        };

        // origExtraHeight + origOffset - origScrollTop = modExtraHeight + modOffset - modScrollTop

        // origScrollTop = origExtraHeight + origOffset - modExtraHeight - modOffset + modScrollTop
        // modScrollTop = modExtraHeight + modOffset - origExtraHeight - origOffset + origScrollTop

        // origOffset - modOffset = heightOfLines(1..Y) - heightOfLines(1..X)
        // origScrollTop >= 0, modScrollTop >= 0

        {
            // update scroll modified
            let (editors, original_top_padding, modified_top_padding) = (editors.clone(), original_top_padding.clone(), modified_top_padding.clone());
            let (original_scroll_offset_animated, modified_scroll_offset_animated) =
                (original_scroll_offset_animated.clone(), modified_scroll_offset_animated.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let new_scroll_top_modified = original_scroll_top.read(reader)
                    - (original_scroll_offset_animated.get() - modified_scroll_offset_animated.read(reader))
                    - (original_top_padding.get() - modified_top_padding.read(reader));
                if new_scroll_top_modified != editors.modified.get_scroll_top() {
                    editors.modified.set_scroll_top(new_scroll_top_modified, Some(ScrollType::Immediate));
                }
            }));
        }

        {
            // update scroll original
            let (editors, original_top_padding, modified_top_padding) = (editors.clone(), original_top_padding.clone(), modified_top_padding.clone());
            let (original_scroll_offset_animated, modified_scroll_offset_animated) = (original_scroll_offset_animated, modified_scroll_offset_animated);
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let new_scroll_top_original = modified_scroll_top.read(reader)
                    - (modified_scroll_offset_animated.get() - original_scroll_offset_animated.read(reader))
                    - (modified_top_padding.get() - original_top_padding.read(reader));
                if new_scroll_top_original != editors.original.get_scroll_top() {
                    editors.original.set_scroll_top(new_scroll_top_original, Some(ScrollType::Immediate));
                }
            }));
        }

        {
            // update editor top offsets
            let (editors, diff_model) = (editors.clone(), diff_model);
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let moved_text = diff_model.read(reader).and_then(|m| m.moved_text_to_compare.read(reader));

                let mut delta_orig_to_mod = 0.0;
                if let Some(m) = &moved_text {
                    let true_top_original = editors.original.get_top_for_line_number(m.line_range_mapping.original.start_line_number, Some(true))
                        - original_top_padding.get();
                    let true_top_modified = editors.modified.get_top_for_line_number(m.line_range_mapping.modified.start_line_number, Some(true))
                        - modified_top_padding.get();
                    delta_orig_to_mod = true_top_modified - true_top_original;
                }

                if delta_orig_to_mod > 0.0 {
                    modified_top_padding.set(0.0, None);
                    original_top_padding.set(delta_orig_to_mod, None);
                } else if delta_orig_to_mod < 0.0 {
                    modified_top_padding.set(-delta_orig_to_mod, None);
                    original_top_padding.set(0.0, None);
                } else {
                    let (original_top_padding, modified_top_padding) = (original_top_padding.clone(), modified_top_padding.clone());
                    runtime::set_timeout(
                        move || {
                            modified_top_padding.set(0.0, None);
                            original_top_padding.set(0.0, None);
                        },
                        400,
                    );
                }

                if editors.modified.has_text_focus() {
                    original_scroll_offset.set_with_change(modified_scroll_offset.get() - delta_orig_to_mod, None, true);
                } else {
                    modified_scroll_offset.set_with_change(original_scroll_offset.get() + delta_orig_to_mod, None, true);
                }
            }));
        }

        Rc::new(Self { base, view_zones })
    }
}

impl IDiffEditorViewZones for DiffEditorViewZones {
    fn view_zones(&self) -> IObservable<Rc<ObservableViewZonePair>> {
        self.view_zones.clone()
    }
}

impl IDisposable for DiffEditorViewZones {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O corpo do `derived` das zonas (`this.viewZones`) e o que ele lê.
struct ViewZonesComputer {
    editors: Rc<DiffEditorEditors>,
    diff_model: IObservable<Option<Rc<DiffEditorViewModel>>>,
    options: Rc<DiffEditorOptions>,
    diff_editor_widget: Weak<DiffEditorWidget>,
    clipboard_service: Rc<dyn IClipboardService>,
    context_menu_service: Rc<dyn IContextMenuService>,
    original_top_padding: Rc<ObservableValue<f64, ()>>,
    modified_top_padding: Rc<ObservableValue<f64, ()>>,
    alignments: IObservable<Option<Rc<Vec<LineRangeAlignment>>>>,
    alignments_synced_moved_text: IObservable<Option<Rc<Vec<LineRangeAlignment>>>>,
    original_model_tokenization_completed: IObservable<Option<bool>>,
}

impl ViewZonesComputer {
    /// O recálculo das zonas; `alignment_view_zones_disposables` guarda a margem do código apagado e a seta.
    fn compute(&self, reader: &dyn IReader, alignment_view_zones_disposables: &DisposableStore) -> Rc<ObservableViewZonePair> {
        let alignments_val = self.alignments.read(reader).unwrap_or_default();

        let mut zones = ZoneLists::default();

        let modified_top_padding_val = self.modified_top_padding.read(reader);
        if modified_top_padding_val > 0.0 {
            zones.modified.push(DiffViewZone::top_padding(modified_top_padding_val));
        }
        let original_top_padding_val = self.original_top_padding.read(reader);
        if original_top_padding_val > 0.0 {
            zones.orig.push(DiffViewZone::top_padding(original_top_padding_val));
        }

        let render_side_by_side = self.options.render_side_by_side.read(reader);
        let mut line_breaks = DeletedCodeLineBreaks { data: Vec::new(), next: 0 };
        if !render_side_by_side {
            match self.compute_deleted_code_line_breaks(&alignments_val) {
                Some(data) => line_breaks.data = data,
                None => return zones.into_pair(),
            }
        }

        let mod_line_height = *self.editors.modified.get_option(LINE_HEIGHT);

        let synced_moved_text: Option<Rc<MovedText>> = self.diff_model.read(reader).and_then(|m| m.moved_text_to_compare.read(reader));

        let original_model = self.editors.original.get_model();
        let render_options = RenderOptions::from_editor(&*self.editors.modified);
        let decoration_options = diff_decoration_options();
        let env = InlineRenderEnv {
            might_contain_non_basic_ascii: original_model.as_ref().is_some_and(|m| m.might_contain_non_basic_ascii()),
            might_contain_rtl: original_model.as_ref().is_some_and(|m| m.might_contain_rtl()),
            original_model,
            render_options,
            modified_line_height: mod_line_height,
            delete_decoration_class_name: decoration_options.diff_delete_decoration.class_name.clone().expect("diffDeleteDecoration tem className"),
            remove_icon_class_name: diff_remove_icon().as_class_name(),
        };

        for a in alignments_val.iter() {
            let inline_diff = if render_side_by_side {
                None
            } else {
                a.diff.as_ref().filter(|diff| !self.options.use_true_inline_diff_rendering.read(reader) || !allows_true_inline_diff_rendering(diff))
            };
            if let Some(diff) = inline_diff {
                if self.push_inline_deleted_zones(reader, alignment_view_zones_disposables, a, diff, &env, &mut line_breaks, &mut zones).is_break() {
                    return zones.into_pair();
                }
            } else {
                self.push_alignment_zones(reader, alignment_view_zones_disposables, a, synced_moved_text.as_deref(), &mut zones);
            }
        }

        for a in self.alignments_synced_moved_text.read(reader).unwrap_or_default().iter() {
            let Some(synced_moved_text) = &synced_moved_text else { continue };
            if synced_moved_text.line_range_mapping.original.intersect(&a.original_range).is_none()
                || synced_moved_text.line_range_mapping.modified.intersect(&a.modified_range).is_none()
            {
                // ignore unrelated alignments outside the synced moved text
                continue;
            }

            let delta = a.modified_height_in_px - a.original_height_in_px;
            if delta > 0.0 {
                zones.orig.push(DiffViewZone::filler(a.original_range.end_line_number_exclusive - 1, delta, None));
            } else {
                zones.modified.push(DiffViewZone::filler(a.modified_range.end_line_number_exclusive - 1, -delta, None));
            }
        }

        zones.into_pair()
    }

    /// O `deletedCodeLineBreaksComputer`: pede as quebras de linha de todas as linhas apagadas, na ordem dos alinhamentos. `None` é
    /// o retorno antecipado do original (uma linha fora do modelo, porque o diff ainda não acompanhou a edição).
    fn compute_deleted_code_line_breaks(&self, alignments: &[LineRangeAlignment]) -> Option<Vec<Option<ModelLineProjectionData>>> {
        let Some(view_model) = self.editors.modified.get_view_model() else { return Some(Vec::new()) };
        let original_model = self.editors.original.get_model().expect("o editor original tem modelo");
        let context = OriginalLineBreaksContext { model: original_model.clone() };
        let mut computer = view_model.create_line_breaks_computer(Some(&context));
        for a in alignments {
            if a.diff.is_some() {
                for i in a.original_range.start_line_number..a.original_range.end_line_number_exclusive {
                    // `i` can be out of bound when the diff has not been updated yet.
                    // In this case, we do an early return.
                    // TODO@hediet: Fix this by applying the edit directly to the diff model, so that the diff is always valid.
                    if i > original_model.get_line_count() {
                        return None;
                    }
                    computer.add_request(i, None);
                }
            }
        }
        Some(computer.finalize())
    }

    /// O ramo em linha do alinhamento: o código apagado numa zona do editor modificado (com a margem) e o preenchimento no original.
    /// `Break` é o retorno antecipado do original.
    #[allow(clippy::too_many_arguments)]
    fn push_inline_deleted_zones(
        &self,
        reader: &dyn IReader,
        alignment_view_zones_disposables: &DisposableStore,
        a: &LineRangeAlignment,
        diff: &DetailedLineRangeMapping,
        env: &InlineRenderEnv,
        line_breaks: &mut DeletedCodeLineBreaks,
        zones: &mut ZoneLists,
    ) -> ControlFlow<()> {
        let mod_line_height = env.modified_line_height;
        if !a.original_range.is_empty() {
            // Update view-zones once tokenization completes
            let _ = self.original_model_tokenization_completed.read(reader);

            let original_model = env.original_model.as_ref().expect("o editor original tem modelo");
            // `a.originalRange` can be out of bound when the diff has not been updated yet.
            // In this case, we do an early return.
            // TODO@hediet: Fix this by applying the edit directly to the diff model, so that the diff is always valid.
            if a.original_range.end_line_number_exclusive - 1 > original_model.get_line_count() {
                return ControlFlow::Break(());
            }
            let source = LineSource::new(
                a.original_range.map_to_line_array(|l| original_model.tokenization().get_line_tokens(l)),
                a.original_range.map_to_line_array(|_| line_breaks.take_next()),
                env.might_contain_non_basic_ascii,
                env.might_contain_rtl,
            );
            let decorations: Vec<InlineDecoration> = diff
                .inner_changes
                .iter()
                .flatten()
                .map(|i| {
                    InlineDecoration::new(
                        i.original_range.delta(-(diff.original.start_line_number - 1)),
                        env.delete_decoration_class_name.as_str(),
                        InlineDecorationType::Regular,
                    )
                })
                .collect();
            let result = Rc::new(render_lines(source, &env.render_options, &decorations, false));

            let mut delete_signs: Vec<DeleteSign> = Vec::new();
            if self.options.render_indicators.read(reader) {
                for i in 0..result.height_in_lines {
                    delete_signs.push(DeleteSign {
                        class_name: format!("delete-sign {}", env.remove_icon_class_name),
                        top: i as f64 * mod_line_height,
                        width: env.render_options.line_decorations_width,
                        height: mod_line_height,
                    });
                }
            }

            let zone_id: Rc<RefCell<Option<String>>> = Rc::default();
            let page_top = Rc::new(Cell::new(0.0));
            let margin = InlineDiffDeletedCodeMargin::new(InlineDiffDeletedCodeMarginArgs {
                get_view_zone_id: {
                    let zone_id = zone_id.clone();
                    Rc::new(move || zone_id.borrow().clone().expect("a zona de visão já tem id quando a margem o consulta"))
                },
                margin_page_top: {
                    let page_top = page_top.clone();
                    Rc::new(move || page_top.get())
                },
                modified_editor: self.editors.modified.clone(),
                diff: diff.clone(),
                editor: self.diff_editor_widget.clone(),
                render_lines_result: result.clone(),
                original_text_model: original_model.clone(),
                context_menu_service: self.context_menu_service.clone(),
                clipboard_service: self.clipboard_service.clone(),
            });
            alignment_view_zones_disposables.add(margin.clone());

            for (i, &count) in result.view_line_counts.iter().enumerate() {
                // Account for wrapped lines in the (collapsed) original editor (which doesn't wrap lines).
                if count > 1 {
                    zones.orig.push(DiffViewZone::filler(
                        a.original_range.start_line_number + i as i32,
                        (count - 1) as f64 * mod_line_height,
                        None,
                    ));
                }
            }

            zones.modified.push(Rc::new(DiffViewZone {
                after_line_number: a.modified_range.start_line_number - 1,
                height_in_px: result.height_in_lines as f64 * mod_line_height,
                min_width_in_px: Some(result.min_width_in_px),
                dom_node: Rc::new(DeletedCodeNode { render_lines_result: result.clone(), margin: margin.clone() }),
                margin_dom_node: Some(Rc::new(DeletedCodeMarginNode {
                    font_info: env.render_options.font_info.clone(),
                    delete_signs,
                    margin,
                    page_top,
                })),
                suppress_mouse_down: false,
                zone_id,
            }));
        }

        zones.orig.push(DiffViewZone::filler(
            a.original_range.end_line_number_exclusive - 1,
            a.modified_height_in_px,
            Some(Rc::new(GutterDeleteMarginNode)),
        ));
        ControlFlow::Continue(())
    }

    /// O ramo de alinhamento puro: o preenchimento que falta no lado mais baixo (com a seta de reverter, quando o lado modificado está
    /// vazio).
    fn push_alignment_zones(
        &self,
        reader: &dyn IReader,
        alignment_view_zones_disposables: &DisposableStore,
        a: &LineRangeAlignment,
        synced_moved_text: Option<&MovedText>,
        zones: &mut ZoneLists,
    ) {
        let delta = a.modified_height_in_px - a.original_height_in_px;
        if delta > 0.0 {
            if synced_moved_text.is_some_and(|m| m.line_range_mapping.original.delta(-1).delta_length(2).contains(a.original_range.end_line_number_exclusive - 1)) {
                return;
            }

            zones.orig.push(DiffViewZone::filler(a.original_range.end_line_number_exclusive - 1, delta, None));
        } else {
            if synced_moved_text.is_some_and(|m| m.line_range_mapping.modified.delta(-1).delta_length(2).contains(a.modified_range.end_line_number_exclusive - 1)) {
                return;
            }

            let margin_dom_node: Option<Rc<dyn Any>> = match &a.diff {
                Some(diff) if diff.modified.is_empty() && self.options.should_render_old_revert_arrows.read(reader) => {
                    let arrow = Rc::new(RevertChangeArrow { diff: diff.clone(), editor: self.diff_editor_widget.clone(), disposed: Cell::new(false) });
                    let arrow_for_dispose = arrow.clone();
                    alignment_view_zones_disposables.add(to_disposable(move || arrow_for_dispose.disposed.set(true)));
                    Some(arrow as Rc<dyn Any>)
                }
                _ => None,
            };

            zones.modified.push(DiffViewZone::filler(a.modified_range.end_line_number_exclusive - 1, -delta, margin_dom_node));
        }
    }
}

/// `computeRangeAlignment(...)`: o estado e as duas filas de alturas extras do cálculo.
struct AlignmentComputer<'a> {
    original_line_height_overrides: ArrayQueue<'a, AdditionalLineHeightInfo>,
    modified_line_height_overrides: ArrayQueue<'a, AdditionalLineHeightInfo>,
    orig_line_height: f64,
    mod_line_height: f64,
    result: Vec<LineRangeAlignment>,
    last_original_line_number: i32,
    last_modified_line_number: i32,
}

/// As variáveis `first`, `lastModLineNumber` e `lastOrigLineNumber` de cada diff.
struct EmitCursor {
    first: bool,
    last_mod_line_number: i32,
    last_orig_line_number: i32,
}

impl AlignmentComputer<'_> {
    /// `handleAlignmentsOutsideOfDiffs(untilOriginalLineNumberExclusive, untilModifiedLineNumberExclusive)`.
    fn handle_alignments_outside_of_diffs(&mut self, until_original_line_number_exclusive: i32, until_modified_line_number_exclusive: i32) {
        loop {
            let mut orig_next = self.original_line_height_overrides.peek();
            let mut mod_next = self.modified_line_height_overrides.peek();
            if orig_next.is_some_and(|o| o.line_number >= until_original_line_number_exclusive) {
                orig_next = None;
            }
            if mod_next.is_some_and(|m| m.line_number >= until_modified_line_number_exclusive) {
                mod_next = None;
            }
            if orig_next.is_none() && mod_next.is_none() {
                break;
            }

            let dist_orig = orig_next.map_or(i64::MAX, |o| i64::from(o.line_number - self.last_original_line_number));
            let dist_next = mod_next.map_or(i64::MAX, |m| i64::from(m.line_number - self.last_modified_line_number));

            if dist_orig < dist_next {
                self.original_line_height_overrides.dequeue();
                mod_next = Some(AdditionalLineHeightInfo {
                    line_number: orig_next.expect("dist_orig < dist_next implica orig_next").line_number - self.last_original_line_number
                        + self.last_modified_line_number,
                    height_in_px: 0.0,
                });
            } else if dist_orig > dist_next {
                self.modified_line_height_overrides.dequeue();
                orig_next = Some(AdditionalLineHeightInfo {
                    line_number: mod_next.expect("dist_orig > dist_next implica mod_next").line_number - self.last_modified_line_number
                        + self.last_original_line_number,
                    height_in_px: 0.0,
                });
            } else {
                self.original_line_height_overrides.dequeue();
                self.modified_line_height_overrides.dequeue();
            }

            let (orig_next, mod_next) = (orig_next.expect("o alinhamento tem lado original"), mod_next.expect("o alinhamento tem lado modificado"));
            self.result.push(LineRangeAlignment {
                original_range: LineRange::of_length(orig_next.line_number, 1),
                modified_range: LineRange::of_length(mod_next.line_number, 1),
                original_height_in_px: self.orig_line_height + orig_next.height_in_px,
                modified_height_in_px: self.mod_line_height + mod_next.height_in_px,
                diff: None,
            });
        }
    }

    /// `emitAlignment(origLineNumberExclusive, modLineNumberExclusive, forceAlignment = false)`.
    fn emit_alignment(
        &mut self,
        cursor: &mut EmitCursor,
        mapping: &DiffMapping,
        orig_line_number_exclusive: i32,
        mod_line_number_exclusive: i32,
        force_alignment: bool,
    ) {
        if orig_line_number_exclusive < cursor.last_orig_line_number || mod_line_number_exclusive < cursor.last_mod_line_number {
            return;
        }
        if cursor.first {
            cursor.first = false;
        } else if !force_alignment
            && (orig_line_number_exclusive == cursor.last_orig_line_number || mod_line_number_exclusive == cursor.last_mod_line_number)
        {
            // This causes a re-alignment of an already aligned line.
            // However, we don't care for the final alignment.
            return;
        }
        let original_range = LineRange::new(cursor.last_orig_line_number, orig_line_number_exclusive);
        let modified_range = LineRange::new(cursor.last_mod_line_number, mod_line_number_exclusive);
        if original_range.is_empty() && modified_range.is_empty() {
            return;
        }

        let original_additional_height: f64 = self
            .original_line_height_overrides
            .take_while(|v| v.line_number < orig_line_number_exclusive)
            .map_or(0.0, |v| v.iter().map(|c| c.height_in_px).sum());
        let modified_additional_height: f64 = self
            .modified_line_height_overrides
            .take_while(|v| v.line_number < mod_line_number_exclusive)
            .map_or(0.0, |v| v.iter().map(|c| c.height_in_px).sum());

        self.result.push(LineRangeAlignment {
            original_range,
            modified_range,
            original_height_in_px: f64::from(original_range.length()) * self.orig_line_height + original_additional_height,
            modified_height_in_px: f64::from(modified_range.length()) * self.mod_line_height + modified_additional_height,
            diff: Some(mapping.line_range_mapping.clone()),
        });

        cursor.last_orig_line_number = orig_line_number_exclusive;
        cursor.last_mod_line_number = mod_line_number_exclusive;
    }
}

/// `computeRangeAlignment(originalEditor, modifiedEditor, diffs, originalEditorAlignmentViewZones, modifiedEditorAlignmentViewZones,
/// innerHunkAlignment)`.
fn compute_range_alignment(
    original_editor: &CodeEditorWidget,
    modified_editor: &CodeEditorWidget,
    diffs: &[DiffMapping],
    original_editor_alignment_view_zones: &HashSet<String>,
    modified_editor_alignment_view_zones: &HashSet<String>,
    inner_hunk_alignment: bool,
) -> Vec<LineRangeAlignment> {
    let original_heights = get_additional_line_heights(original_editor, original_editor_alignment_view_zones);
    let modified_heights = get_additional_line_heights(modified_editor, modified_editor_alignment_view_zones);

    let mut computer = AlignmentComputer {
        original_line_height_overrides: ArrayQueue::new(&original_heights),
        modified_line_height_overrides: ArrayQueue::new(&modified_heights),
        orig_line_height: *original_editor.get_option(LINE_HEIGHT),
        mod_line_height: *modified_editor.get_option(LINE_HEIGHT),
        result: Vec::new(),
        last_original_line_number: 0,
        last_modified_line_number: 0,
    };

    for m in diffs {
        let c = &m.line_range_mapping;
        computer.handle_alignments_outside_of_diffs(c.original.start_line_number, c.modified.start_line_number);

        let mut cursor = EmitCursor {
            first: true,
            last_mod_line_number: c.modified.start_line_number,
            last_orig_line_number: c.original.start_line_number,
        };

        if inner_hunk_alignment {
            for i in c.inner_changes.iter().flatten() {
                if i.original_range.start_column > 1 && i.modified_range.start_column > 1 {
                    // There is some unmodified text on this line before the diff
                    computer.emit_alignment(&mut cursor, m, i.original_range.start_line_number, i.modified_range.start_line_number, false);
                }
                let original_model = original_editor.get_model().expect("o editor original tem modelo");
                // When the diff is invalid, the ranges might be out of bounds (this should be fixed in the diff model by applying edits directly).
                let max_column = if i.original_range.end_line_number <= original_model.get_line_count() {
                    original_model.get_line_max_column(i.original_range.end_line_number)
                } else {
                    i32::MAX
                };
                if i.original_range.end_column < max_column {
                    // There is some unmodified text on this line after the diff
                    computer.emit_alignment(&mut cursor, m, i.original_range.end_line_number, i.modified_range.end_line_number, false);
                }
            }
        }

        computer.emit_alignment(&mut cursor, m, c.original.end_line_number_exclusive, c.modified.end_line_number_exclusive, true);

        computer.last_original_line_number = c.original.end_line_number_exclusive;
        computer.last_modified_line_number = c.modified.end_line_number_exclusive;
    }
    computer.handle_alignments_outside_of_diffs(i32::MAX, i32::MAX);

    computer.result
}

/// `getAdditionalLineHeights(editor, viewZonesToIgnore)`: as alturas extras (zonas de visão de terceiros e linhas quebradas) por
/// número de linha do modelo, em ordem.
fn get_additional_line_heights(editor: &CodeEditorWidget, view_zones_to_ignore: &HashSet<String>) -> Vec<AdditionalLineHeightInfo> {
    let mut view_zone_heights: Vec<AdditionalLineHeightInfo> = Vec::new();
    let mut wrapping_zone_heights: Vec<AdditionalLineHeightInfo> = Vec::new();

    let has_wrapping = editor.get_option(WRAPPING_INFO).wrapping_column != -1.0;
    let coordinates_converter = editor.get_view_model().expect("o editor do diff tem modelo de visão").coordinates_converter.clone();
    let editor_line_height = *editor.get_option(LINE_HEIGHT);
    if has_wrapping {
        for i in 1..=editor.get_model().expect("o editor do diff tem modelo").get_line_count() {
            let line_count = coordinates_converter.get_model_line_view_line_count(i);
            if line_count > 1 {
                wrapping_zone_heights.push(AdditionalLineHeightInfo { line_number: i, height_in_px: editor_line_height * f64::from(line_count - 1) });
            }
        }
    }

    for w in editor.get_whitespaces() {
        if view_zones_to_ignore.contains(&w.id) {
            continue;
        }
        let model_line_number = if w.after_line_number == 0 {
            0
        } else {
            coordinates_converter.convert_view_position_to_model_position(Position::new(w.after_line_number, 1)).line_number
        };
        view_zone_heights.push(AdditionalLineHeightInfo { line_number: model_line_number, height_in_px: f64::from(w.height) });
    }

    join_combine(&view_zone_heights, &wrapping_zone_heights, |v| v.line_number, |v1, v2| AdditionalLineHeightInfo {
        line_number: v1.line_number,
        height_in_px: v1.height_in_px + v2.height_in_px,
    })
}

/// `allowsTrueInlineDiffRendering(mapping)`.
pub fn allows_true_inline_diff_rendering(mapping: &DetailedLineRangeMapping) -> bool {
    let Some(inner_changes) = &mapping.inner_changes else {
        return false;
    };
    inner_changes.iter().all(|c| {
        (range_is_single_line(&c.modified_range) && range_is_single_line(&c.original_range)) || c.original_range == Range::new(1, 1, 1, 1)
    })
}

/// `rangeIsSingleLine(range)`.
pub fn range_is_single_line(range: &Range) -> bool {
    range.start_line_number == range.end_line_number
}
