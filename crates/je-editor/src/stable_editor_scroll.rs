//! Porte de `src/vs/editor/browser/stableEditorScroll.ts`: guarda a posição de rolagem do editor antes de uma
//! edição e a restaura depois (`StableEditorScrollState` e `StableEditorBottomScrollState`).
//!
//! Representação:
//! - O editor é o [`CodeEditorWidget`] (`ICodeEditor` não é objeto de trait, tem métodos genéricos).
//! - `Position | null` é `Option<Position>`.
//!
//! Divergências:
//! - `editor.hasPendingScrollAnimation()` não existe no widget do porte (a rolagem suave ainda não tem animação
//!   pendente), então vale sempre `false`.

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::editor_common::ScrollType;
use crate::position::Position;

/// `hasPendingScrollAnimation()` (ver "Divergências").
fn has_pending_scroll_animation(_editor: &CodeEditorWidget) -> bool {
    false
}

/// `StableEditorScrollState`.
pub struct StableEditorScrollState {
    initial_scroll_top: f64,
    initial_content_height: f64,
    visible_position: Option<Position>,
    visible_position_scroll_delta: f64,
    cursor_position: Option<Position>,
}

impl StableEditorScrollState {
    pub fn capture(editor: &CodeEditorWidget) -> Self {
        if editor.get_scroll_top() == 0.0 || has_pending_scroll_animation(editor) {
            // Never mess with the scroll top if the editor is at the top of the file or if there is a pending scroll animation
            return Self::new(editor.get_scroll_top(), editor.get_content_height(), None, 0.0, None);
        }

        let mut visible_position: Option<Position> = None;
        let mut visible_position_scroll_delta = 0.0;
        let visible_ranges = editor.get_visible_ranges();
        if let Some(first) = visible_ranges.first() {
            let position = first.get_start_position();
            let visible_position_scroll_top = editor.get_top_for_position(position.line_number, position.column);
            visible_position_scroll_delta = editor.get_scroll_top() - visible_position_scroll_top;
            visible_position = Some(position);
        }
        Self::new(
            editor.get_scroll_top(),
            editor.get_content_height(),
            visible_position,
            visible_position_scroll_delta,
            editor.get_position(),
        )
    }

    fn new(
        initial_scroll_top: f64,
        initial_content_height: f64,
        visible_position: Option<Position>,
        visible_position_scroll_delta: f64,
        cursor_position: Option<Position>,
    ) -> Self {
        Self { initial_scroll_top, initial_content_height, visible_position, visible_position_scroll_delta, cursor_position }
    }

    pub fn restore(&self, editor: &CodeEditorWidget) {
        if self.initial_content_height == editor.get_content_height() && self.initial_scroll_top == editor.get_scroll_top() {
            // The editor's content height and scroll top haven't changed, so we don't need to do anything
            return;
        }

        if let Some(visible_position) = self.visible_position {
            let visible_position_scroll_top = editor.get_top_for_position(visible_position.line_number, visible_position.column);
            editor.set_scroll_top(visible_position_scroll_top + self.visible_position_scroll_delta, None);
        }
    }

    pub fn restore_relative_vertical_position_of_cursor(&self, editor: &CodeEditorWidget) {
        if self.initial_content_height == editor.get_content_height() && self.initial_scroll_top == editor.get_scroll_top() {
            // The editor's content height and scroll top haven't changed, so we don't need to do anything
            return;
        }

        let current_cursor_position = editor.get_position();

        let (Some(cursor_position), Some(current_cursor_position)) = (self.cursor_position, current_cursor_position) else {
            return;
        };

        let offset = editor.get_top_for_line_number(current_cursor_position.line_number, None)
            - editor.get_top_for_line_number(cursor_position.line_number, None);
        editor.set_scroll_top(editor.get_scroll_top() + offset, Some(ScrollType::Immediate));
    }
}

/// `StableEditorBottomScrollState`.
pub struct StableEditorBottomScrollState {
    initial_scroll_top: f64,
    initial_content_height: f64,
    visible_position: Option<Position>,
    visible_position_scroll_delta: f64,
}

impl StableEditorBottomScrollState {
    pub fn capture(editor: &CodeEditorWidget) -> Self {
        if has_pending_scroll_animation(editor) {
            // Never mess with the scroll if there is a pending scroll animation
            return Self::new(editor.get_scroll_top(), editor.get_content_height(), None, 0.0);
        }

        let mut visible_position: Option<Position> = None;
        let mut visible_position_scroll_delta = 0.0;
        let visible_ranges = editor.get_visible_ranges();
        if let Some(last) = visible_ranges.last() {
            let position = last.get_end_position();
            let visible_position_scroll_bottom = editor.get_bottom_for_line_number(position.line_number, None);
            visible_position_scroll_delta = visible_position_scroll_bottom - editor.get_scroll_top();
            visible_position = Some(position);
        }
        Self::new(editor.get_scroll_top(), editor.get_content_height(), visible_position, visible_position_scroll_delta)
    }

    fn new(
        initial_scroll_top: f64,
        initial_content_height: f64,
        visible_position: Option<Position>,
        visible_position_scroll_delta: f64,
    ) -> Self {
        Self { initial_scroll_top, initial_content_height, visible_position, visible_position_scroll_delta }
    }

    pub fn restore(&self, editor: &CodeEditorWidget) {
        if self.initial_content_height == editor.get_content_height() && self.initial_scroll_top == editor.get_scroll_top() {
            // The editor's content height and scroll top haven't changed, so we don't need to do anything
            return;
        }

        if let Some(visible_position) = self.visible_position {
            let visible_position_scroll_bottom = editor.get_bottom_for_line_number(visible_position.line_number, None);
            editor.set_scroll_top(visible_position_scroll_bottom - self.visible_position_scroll_delta, Some(ScrollType::Immediate));
        }
    }
}
