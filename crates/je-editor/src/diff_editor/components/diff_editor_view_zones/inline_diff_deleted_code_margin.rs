//! Porte de `src/vs/editor/browser/widget/diffEditor/components/diffEditorViewZones/inlineDiffDeletedCodeMargin.ts`: a margem da zona de
//! visão com o código apagado do modo em linha, com a lâmpada que abre o menu "Copy deleted lines" / "Revert this change".
//!
//! Divergências:
//! - **DOM vira dado.** A margem (`marginDomNode`) e a lâmpada (`_diffActions`) são estado que o pintor lê: [`MARGIN_Z_INDEX`] (o
//!   `zIndex = '10'` da margem), [`InlineDiffDeletedCodeMargin::diff_actions_class_name`] (as classes da lâmpada),
//!   [`InlineDiffDeletedCodeMargin::visibility`] (o `style.visibility`), [`InlineDiffDeletedCodeMargin::diff_actions_top`] (o
//!   `style.top`) e [`InlineDiffDeletedCodeMargin::line_height`] (o `height` e o `lineHeight` do estilo; `right: 0px` e
//!   `position: absolute` são fixos). O `getDomNodePagePosition(marginDomNode)` é o parâmetro [`InlineDiffDeletedCodeMarginArgs::margin_page_top`],
//!   porque só o pintor sabe onde a margem está na janela; o topo da lâmpada na janela é esse valor mais [`InlineDiffDeletedCodeMargin::diff_actions_top`].
//! - **Eventos.** O `mousedown` da lâmpada é [`InlineDiffDeletedCodeMargin::on_diff_actions_mouse_down`]. O `e.event.browserEvent.y` do
//!   `onMouseMove` do editor é o `posy` do evento (o `MouseEvent` do porte não carrega o evento do navegador).
//! - **`_deletedCodeDomNode`.** Era só o alvo do ouvinte de `copy`: o evento de cópia chega pela [`ViewZoneCopySelection`] devolvida em
//!   [`InlineDiffDeletedCodeMargin::copy_selection`].
//! - **Menu de contexto.** `domForShadowRoot` e o `useShadowDOM` não existem (não há shadow DOM); o resto do `showContextMenu`
//!   (âncora, `getActions`, `onHide`, `autoSelectFirstItem`) é o [`DeletedCodeMenuDelegate`]. O `Action` do original, com a função
//!   assíncrona, é a [`Action`] com um `ActionCallback` que devolve a promessa da área de transferência.
//! - **`DiffEditorWidget`.** O `_editor.revert(diff)` sobe o `Weak<DiffEditorWidget>` (as features são criadas antes de o `Rc` do widget
//!   existir, ver `feature_contracts`); com o widget já descartado, a ação não faz nada.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionCallback, ActionRef};
use je_base::dynamic::Dynamic;
use je_base::layout::IAnchor;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_base::themables::ThemeIcon;
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuService};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::cursor_common::CursorSimpleModel;
use crate::config::editor_options::{EditorOption, Opt};
use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::diff_editor::components::diff_editor_view_zones::copy_selection::{
    EnableViewZoneCopySelectionOptions, ViewZoneCopySelection, enable_copy_selection,
};
use crate::diff_editor::components::diff_editor_view_zones::render_lines::RenderLinesResult;
use crate::diff_editor::diff_editor_widget::DiffEditorWidget;
use crate::editor_browser::{EditorMouseEvent, MouseEvent, MouseTarget};
use crate::model::EndOfLineSequence;
use crate::services::clipboard_service::IClipboardService;
use crate::text_model::TextModel;

/// `EditorOption.readOnly`.
const READ_ONLY: Opt<bool> = Opt::new(EditorOption::ReadOnly);

/// O `this._marginDomNode.style.zIndex = '10'`: a margem aparece acima do overlay.
pub const MARGIN_Z_INDEX: i32 = 10;

/// Os argumentos do construtor de `InlineDiffDeletedCodeMargin` (ver as divergências do módulo).
pub struct InlineDiffDeletedCodeMarginArgs {
    /// `_getViewZoneId`.
    pub get_view_zone_id: Rc<dyn Fn() -> String>,
    /// O topo da margem na janela (`getDomNodePagePosition(marginDomNode).top`).
    pub margin_page_top: Rc<dyn Fn() -> f64>,
    pub modified_editor: Rc<CodeEditorWidget>,
    pub diff: DetailedLineRangeMapping,
    pub editor: Weak<DiffEditorWidget>,
    pub render_lines_result: Rc<RenderLinesResult>,
    pub original_text_model: Rc<TextModel>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub clipboard_service: Rc<dyn IClipboardService>,
}

/// O estado que a margem compartilha com o ouvinte do mouse e com o menu.
struct MarginState {
    get_view_zone_id: Rc<dyn Fn() -> String>,
    margin_page_top: Rc<dyn Fn() -> f64>,
    modified_editor: Rc<CodeEditorWidget>,
    diff: DetailedLineRangeMapping,
    editor: Weak<DiffEditorWidget>,
    render_lines_result: Rc<RenderLinesResult>,
    original_text_model: Rc<TextModel>,
    context_menu_service: Rc<dyn IContextMenuService>,
    clipboard_service: Rc<dyn IClipboardService>,
    line_height: f64,
    visibility: Cell<bool>,
    diff_actions_top: Cell<f64>,
    /// `currentLineNumberOffset`.
    current_line_number_offset: Cell<i32>,
}

impl MarginState {
    /// `set visibility(_visibility)`.
    fn set_visibility(&self, visibility: bool) {
        self.visibility.set(visibility);
    }

    /// `_updateLightBulbPosition(marginDomNode, y, lineHeight)`.
    fn update_light_bulb_position(&self, y: f64) -> i32 {
        let top = (self.margin_page_top)();
        let offset = y - top;
        let line_number_offset = (offset / self.line_height).floor() as i32;
        self.diff_actions_top.set(f64::from(line_number_offset) * self.line_height);
        let mut acc = 0_usize;
        for (i, &view_line_count) in self.render_lines_result.view_line_counts.iter().enumerate() {
            acc += view_line_count;
            if i64::from(line_number_offset) < acc as i64 {
                return i as i32;
            }
        }
        line_number_offset
    }

    /// As ações do menu: o `getActions` do `showContextMenu`, com as `baseActions` na frente.
    fn build_actions(self: &Rc<Self>, base_actions: &[ActionRef]) -> Vec<ActionRef> {
        let mut actions: Vec<ActionRef> = base_actions.to_vec();
        let diff = &self.diff;
        let is_deletion = diff.modified.is_empty();
        let original_length = diff.original.length();

        // default action
        let label = if is_deletion {
            if original_length > 1 {
                localize("diff.clipboard.copyDeletedLinesContent.label", "Copy deleted lines", &[])
            } else {
                localize("diff.clipboard.copyDeletedLinesContent.single.label", "Copy deleted line", &[])
            }
        } else if original_length > 1 {
            localize("diff.clipboard.copyChangedLinesContent.label", "Copy changed lines", &[])
        } else {
            localize("diff.clipboard.copyChangedLinesContent.single.label", "Copy changed line", &[])
        };
        let state = Rc::clone(self);
        let copy_content: ActionCallback = Rc::new(move |_| {
            let original_text = state.original_text_model.get_value_in_range(&state.diff.original.to_exclusive_range());
            MaybePromise::Promise(state.clipboard_service.write_text(&original_text.to_string_lossy()).then(|()| Dynamic::Undefined))
        });
        actions.push(Action::new("diff.clipboard.copyDeletedContent", &label, "", true, Some(copy_content)) as ActionRef);

        if original_length > 1 {
            let line_number = (diff.original.start_line_number + self.current_line_number_offset.get()).to_string();
            let label = if is_deletion {
                localize("diff.clipboard.copyDeletedLineContent.label", "Copy deleted line ({0})", &[line_number.as_str()])
            } else {
                localize("diff.clipboard.copyChangedLineContent.label", "Copy changed line ({0})", &[line_number.as_str()])
            };
            let state = Rc::clone(self);
            let copy_line: ActionCallback = Rc::new(move |_| {
                let mut line_content = state
                    .original_text_model
                    .get_line_content(state.diff.original.start_line_number + state.current_line_number_offset.get())
                    .to_string_lossy();
                if line_content.is_empty() {
                    // empty line -> new line
                    let eof = state.original_text_model.get_end_of_line_sequence();
                    line_content = if matches!(eof, EndOfLineSequence::Lf) { "\n" } else { "\r\n" }.to_owned();
                }
                MaybePromise::Promise(state.clipboard_service.write_text(&line_content).then(|()| Dynamic::Undefined))
            });
            actions.push(Action::new("diff.clipboard.copyDeletedLineContent", &label, "", true, Some(copy_line)) as ActionRef);
        }
        let read_only = *self.modified_editor.get_options().get(READ_ONLY);
        if !read_only {
            let state = Rc::clone(self);
            let revert: ActionCallback = Rc::new(move |_| {
                if let Some(editor) = state.editor.upgrade() {
                    editor.revert(&state.diff);
                }
                MaybePromise::Value(Dynamic::Undefined)
            });
            actions.push(Action::new("diff.inline.revertChange", &localize("diff.inline.revertChange.label", "Revert this change", &[]), "", true, Some(revert)) as ActionRef);
        }
        actions
    }

    /// `showContextMenu(anchor, baseActions?, onHide?)`.
    fn show_context_menu(self: &Rc<Self>, anchor: IAnchor, base_actions: Option<Vec<ActionRef>>, on_hide: Option<Rc<dyn Fn()>>) {
        self.context_menu_service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(DeletedCodeMenuDelegate {
            anchor,
            state: Rc::clone(self),
            base_actions: base_actions.unwrap_or_default(),
            on_hide,
        })));
    }
}

/// O delegado do menu de contexto da lâmpada (`this._contextMenuService.showContextMenu({ getAnchor, onHide, getActions, autoSelectFirstItem })`).
struct DeletedCodeMenuDelegate {
    anchor: IAnchor,
    state: Rc<MarginState>,
    base_actions: Vec<ActionRef>,
    on_hide: Option<Rc<dyn Fn()>>,
}

impl IContextMenuDelegate for DeletedCodeMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        AnchorTarget::Anchor(self.anchor)
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        self.state.build_actions(&self.base_actions)
    }

    fn on_hide(&self, _did_cancel: bool) {
        if let Some(on_hide) = &self.on_hide {
            on_hide();
        }
    }

    fn auto_select_first_item(&self) -> bool {
        true
    }
}

/// `InlineDiffDeletedCodeMargin`.
pub struct InlineDiffDeletedCodeMargin {
    base: Rc<Disposable>,
    state: Rc<MarginState>,
    copy_selection: Rc<ViewZoneCopySelection>,
}

impl InlineDiffDeletedCodeMargin {
    pub fn new(args: InlineDiffDeletedCodeMarginArgs) -> Rc<Self> {
        let InlineDiffDeletedCodeMarginArgs {
            get_view_zone_id,
            margin_page_top,
            modified_editor,
            diff,
            editor,
            render_lines_result,
            original_text_model,
            context_menu_service,
            clipboard_service,
        } = args;

        let line_height = *modified_editor.get_options().get(LINE_HEIGHT);
        let state = Rc::new(MarginState {
            get_view_zone_id,
            margin_page_top,
            modified_editor: Rc::clone(&modified_editor),
            diff: diff.clone(),
            editor,
            render_lines_result: Rc::clone(&render_lines_result),
            original_text_model: Rc::clone(&original_text_model),
            context_menu_service,
            clipboard_service: Rc::clone(&clipboard_service),
            line_height,
            visibility: Cell::new(false),
            diff_actions_top: Cell::new(0.0),
            current_line_number_offset: Cell::new(0),
        });

        let base = Disposable::new();

        base.register(modified_editor.on_mouse_move().listen({
            let state = Rc::clone(&state);
            move |e: &EditorMouseEvent| {
                let in_this_view_zone = matches!(&e.target, MouseTarget::ViewZone(target) if target.detail.view_zone_id == (state.get_view_zone_id)());
                if in_this_view_zone {
                    state.current_line_number_offset.set(state.update_light_bulb_position(e.event.posy));
                    state.set_visibility(true);
                } else {
                    state.set_visibility(false);
                }
            }
        }));

        let copy_selection = enable_copy_selection(EnableViewZoneCopySelectionOptions {
            diff_entry: diff,
            original_model: original_text_model,
            render_lines_result,
            clipboard_service,
        });

        Rc::new(Self { base, state, copy_selection })
    }

    /// As classes da lâmpada: `ThemeIcon.asClassName(Codicon.lightBulb) + ' lightbulb-glyph'`.
    pub fn diff_actions_class_name() -> String {
        format!("{} lightbulb-glyph", ThemeIcon::from_id("light-bulb").as_class_name())
    }

    /// `get visibility()`.
    pub fn visibility(&self) -> bool {
        self.state.visibility.get()
    }

    /// `set visibility(_visibility)`.
    pub fn set_visibility(&self, visibility: bool) {
        self.state.set_visibility(visibility);
    }

    /// O `style.top` da lâmpada, em pixels.
    pub fn diff_actions_top(&self) -> f64 {
        self.state.diff_actions_top.get()
    }

    /// O `height` e o `lineHeight` do estilo da lâmpada.
    pub fn line_height(&self) -> f64 {
        self.state.line_height
    }

    /// Quem recebe o `copy` do pintor na zona de código apagado.
    pub fn copy_selection(&self) -> &Rc<ViewZoneCopySelection> {
        &self.copy_selection
    }

    /// O ouvinte de `mousedown` da lâmpada. Devolve se o evento foi consumido (`e.preventDefault()`).
    pub fn on_diff_actions_mouse_down(&self, e: &MouseEvent) -> bool {
        if !e.left_button {
            return false;
        }

        let state = &self.state;
        let top = (state.margin_page_top)() + state.diff_actions_top.get();
        let height = state.line_height;
        let pad = (state.line_height / 3.0).floor();
        state.show_context_menu(IAnchor { x: e.posx, y: top + height + pad, width: None, height: None }, None, None);
        true
    }
}

impl IDisposable for InlineDiffDeletedCodeMargin {
    fn dispose(&self) {
        self.copy_selection.dispose();
        self.base.dispose();
    }
}
