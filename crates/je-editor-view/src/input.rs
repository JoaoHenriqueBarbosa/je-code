//! A entrada do editor: ponteiro, teclado e texto viram movimento do cursor, seleção e edições do modelo.
//!
//! As edições passam por `TextModel::push_edit_operations` (com a seleção de antes e o calculador da seleção
//! de depois, como o `cursor.ts` do Monaco), então desfazer e refazer restauram a seleção. Só o mínimo do
//! `Cursor` do Monaco vive aqui; o porte inteiro de `src/vs/editor/common/cursor` entra depois.
// TODO(integração): trocar a lógica de digitação, exclusão e indentação pelo porte de `cursor` do Monaco
// (TypeOperations, DeleteOperations, auto fechamento, vários cursores).

use je_editor::cursor_common::CursorSimpleModel;
use je_base::js_string::JsString;
use je_base::strings::get_leading_whitespace;
use je_editor::cursor_columns::{column_from_visible_column, prev_render_tab_stop, visible_column_from_column};
use je_editor::edit_operation::SingleEditOperation;
use je_editor::model::{Eol, ValidEditOperation};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_ui::{Modifiers, PointerButton};

use crate::cursor::{self, CursorState, select};
use crate::view::{DragMode, EditorView};

/// As teclas que o editor trata (as que não são texto).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorKey {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    /// Uma tecla de caractere, com o modificador (Ctrl+Z, Ctrl+A...), em minúscula.
    Char(char),
}

/// O tipo da última edição, para decidir onde o desfazer corta (`EditOperationType` do Monaco).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    DeleteLeft,
    DeleteRight,
    Other,
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

fn eol_text(model: &je_editor::text_model::TextModel) -> &'static str {
    match model.get_eol() {
        Eol::Lf => "\n",
        Eol::CrLf => "\r\n",
    }
}

fn replace(range: Range, text: Option<&str>) -> SingleEditOperation {
    SingleEditOperation {
        range,
        text: text.filter(|t| !t.is_empty()).map(js),
        force_move_markers: false,
        identifier: None,
        is_auto_whitespace_edit: false,
        is_tracked: false,
    }
}

/// O fim do intervalo da última operação inversa: onde o cursor fica depois de digitar ou apagar.
fn end_of_last(inverse: &[ValidEditOperation]) -> Selection {
    let p = inverse.last().map_or(Position::new(1, 1), |op| op.range.get_end_position());
    Selection::new(p.line_number, p.column, p.line_number, p.column)
}

impl EditorView {
    // #region cursor e seleção

    /// Troca a seleção sem editar (movimento): corta o desfazer na próxima edição e revela o cursor.
    pub(crate) fn set_selection(&mut self, selection: Selection, desired: Option<i32>) {
        self.cursor = CursorState { selection, desired_visual_column: desired };
        self.moved_since_edit = true;
        self.after_cursor_change();
    }

    pub(crate) fn after_cursor_change(&mut self) {
        self.reset_blink();
        self.reveal_cursor();
        self.mark_dirty();
    }

    /// Seleciona `range` (âncora no começo) e o revela: no centro se a linha está fora da janela, como o
    /// `revealRangeInCenterIfOutsideViewport` da busca.
    pub fn select_range(&mut self, range: Range) {
        let (first, last) = self.layout.visible_lines(self.scroll_top());
        self.set_selection(select(range.get_start_position(), range.get_end_position()), None);
        if range.start_line_number < first || range.end_line_number > last {
            self.scroll_to_center_line(range.start_line_number);
        }
    }

    /// O texto que a busca usa de semente (`seedSearchStringFromSelection: always`): a seleção de uma linha ou, sem
    /// seleção, a palavra sob o cursor.
    pub fn seed_search_string(&self) -> String {
        let selection = self.cursor.selection;
        let range = if selection.is_empty() { cursor::word_range_at(&self.model, self.cursor.position()) } else { Range::new(selection.start_line_number, selection.start_column, selection.end_line_number, selection.end_column) };
        if range.start_line_number != range.end_line_number || range.is_empty() {
            return String::new();
        }
        let text = self.model.get_value_in_range(&range).to_string_lossy();
        // Uma "palavra" de pontuação ou de espaço não serve de semente.
        if selection.is_empty() && !text.chars().any(char::is_alphanumeric) { String::new() } else { text }
    }

    /// Rola o mínimo para o cursor aparecer.
    pub fn reveal_cursor(&mut self) {
        let p = self.cursor.position();
        let lh = self.layout.line_height();
        let top = f64::from(p.line_number - 1) * lh;
        let scroll_top = self.scroll_top();
        if top < scroll_top {
            self.set_scroll_top(top);
        } else if top + lh > scroll_top + self.layout.height {
            self.set_scroll_top(top + lh - self.layout.height);
        }
        let x = self.column_x(p.line_number, p.column);
        let (left, width) = (self.scroll_left(), self.layout.content_width());
        let margin = 30.0f64.min(width / 4.0);
        let target = if x < left { Some((x - margin).max(0.0)) } else if x + 2.0 > left + width { Some(x + 2.0 - width + margin) } else { None };
        if let Some(target) = target {
            self.scroll_by(target - left, 0.0);
        }
    }

    pub fn select_all(&mut self) {
        let model = &self.model;
        let last = model.get_line_count();
        let selection = Selection::new(1, 1, last, model.get_line_max_column(last));
        self.set_selection(selection, None);
    }

    pub fn set_cursor(&mut self, position: Position) {
        let p = self.model.validate_position(position);
        self.set_selection(select(p, p), None);
    }

    /// `Ln x, Col y` e o tamanho da seleção para a barra de status.
    pub fn status_text(&self) -> String {
        let p = self.cursor.position();
        let line = self.model.get_line_content(p.line_number);
        let column = je_editor::cursor_columns::to_statusbar_column(line.units(), p.column, self.model.get_options().tab_size);
        let mut text = format!("Ln {}, Col {}", p.line_number, column);
        if !self.cursor.is_empty() {
            let selected = self.model.get_value_length_in_range(&self.cursor.selection);
            text.push_str(&format!(" ({selected} selected)"));
        }
        text
    }

    /// O intervalo da seleção atual, ordenado.
    pub fn cursor_selection_range(&self) -> Range {
        self.selection_range()
    }

    pub fn cursor_position(&self) -> Position {
        self.cursor.position()
    }

    // #endregion

    // #region mouse

    /// Botão do mouse apertado. `x` e `y` são relativos à view; `click_count` 1, 2 (palavra) ou 3 (linha).
    pub fn mouse_down(&mut self, x: f64, y: f64, button: PointerButton, click_count: u32, mods: Modifiers) {
        if button != PointerButton::Primary {
            return;
        }
        let scrollbar_left = self.layout.width - crate::layout::ViewLayout::SCROLLBAR_WIDTH;
        if x >= scrollbar_left {
            // A barra vertical: o botão arrasta; o resto da trilha leva o botão para o ponteiro (centrado nele).
            let (top, size) = self.scrollbar_slider();
            if y < top || y >= top + size {
                self.scroll_to_slider_top(y - size / 2.0);
            }
            let grab = y - self.scrollbar_slider().0;
            self.drag = Some(DragMode::Scrollbar(grab));
            return;
        }
        if x >= self.layout.text_right() {
            let line = self.minimap_line_at(y);
            self.scroll_to_center_line(line);
            self.drag = Some(DragMode::Minimap);
            return;
        }
        let position = self.position_at(x, y);
        let in_margin = x < self.layout.content_left() - crate::layout::LINE_DECORATIONS_WIDTH;
        let count = if in_margin { 3 } else { click_count.clamp(1, 3) };
        match count {
            1 => {
                let anchor = if mods.shift { self.cursor.anchor() } else { position };
                self.drag = Some(DragMode::Chars);
                self.set_selection(select(anchor, position), None);
            }
            2 => {
                let word = cursor::word_range_at(&self.model, position);
                self.drag = Some(DragMode::Words(word));
                self.set_selection(select(word.get_start_position(), word.get_end_position()), None);
            }
            _ => {
                let line = cursor::line_range_at(&self.model, position.line_number);
                self.drag = Some(DragMode::Lines(line));
                self.set_selection(select(line.get_start_position(), line.get_end_position()), None);
            }
        }
    }

    /// O ponteiro se moveu com o botão apertado: estende a seleção conforme o modo do clique.
    pub fn mouse_drag(&mut self, x: f64, y: f64) {
        let Some(mode) = self.drag else { return };
        match mode {
            DragMode::Scrollbar(grab) => {
                self.scroll_to_slider_top(y - grab);
                return;
            }
            DragMode::Minimap => {
                let line = self.minimap_line_at(y.clamp(0.0, self.layout.height));
                self.scroll_to_center_line(line);
                return;
            }
            _ => {}
        }
        // Arrastar além das bordas rola a view.
        let overshoot = if y < 0.0 { y } else if y > self.layout.height { y - self.layout.height } else { 0.0 };
        if overshoot != 0.0 {
            self.scroll_by(0.0, overshoot / 2.0);
        }
        let position = self.position_at(x, y.clamp(0.0, self.layout.height - 1.0));
        let selection = match mode {
            DragMode::Chars => select(self.cursor.anchor(), position),
            DragMode::Words(initial) => {
                let word = cursor::word_range_at(&self.model, position);
                if position.is_before(&initial.get_start_position()) {
                    select(initial.get_end_position(), word.get_start_position())
                } else {
                    select(initial.get_start_position(), word.get_end_position())
                }
            }
            DragMode::Lines(initial) => {
                let line = cursor::line_range_at(&self.model, position.line_number);
                if position.line_number < initial.start_line_number {
                    select(initial.get_end_position(), line.get_start_position())
                } else {
                    select(initial.get_start_position(), line.get_end_position())
                }
            }
            DragMode::Scrollbar(_) | DragMode::Minimap => return,
        };
        if selection != self.cursor.selection {
            self.set_selection(selection, None);
        }
    }

    pub fn mouse_up(&mut self) {
        self.drag = None;
    }

    // #endregion

    // #region teclado

    /// Uma tecla de navegação ou de comando. Devolve `true` se o editor a tratou.
    pub fn key_down(&mut self, key: EditorKey, mods: Modifiers) -> bool {
        let ctrl = mods.ctrl || mods.meta;
        match key {
            EditorKey::Char(c) if ctrl => self.command(c, mods.shift),
            EditorKey::Char(_) => false,
            EditorKey::Left | EditorKey::Right | EditorKey::Up | EditorKey::Down | EditorKey::Home | EditorKey::End | EditorKey::PageUp | EditorKey::PageDown => {
                self.navigate(key, mods);
                true
            }
            EditorKey::Backspace => {
                self.delete_left(ctrl);
                true
            }
            EditorKey::Delete => {
                self.delete_right(ctrl);
                true
            }
            EditorKey::Enter => {
                self.new_line();
                true
            }
            EditorKey::Tab => {
                self.tab(mods.shift);
                true
            }
            EditorKey::Escape => {
                // Esc recolhe a seleção na posição do cursor.
                if !self.cursor.is_empty() {
                    let p = self.cursor.position();
                    self.set_selection(select(p, p), None);
                }
                true
            }
        }
    }

    fn command(&mut self, c: char, shift: bool) -> bool {
        match c {
            'a' => self.select_all(),
            'z' if shift => self.redo(),
            'z' => self.undo(),
            'y' => self.redo(),
            'c' => self.copy(),
            'x' => self.cut(),
            'v' => self.paste(),
            _ => return false,
        }
        true
    }

    fn navigate(&mut self, key: EditorKey, mods: Modifiers) {
        let ctrl = mods.ctrl || mods.meta;
        let model = self.model.clone();
        let current = self.cursor;
        let (position, desired) = (current.position(), current.desired_visual_column);
        let page = ((self.layout.height / self.layout.line_height()).floor() as i32 - 1).max(1);
        // Sem Shift, Esquerda e Direita com seleção só a colapsam nas pontas.
        if !mods.shift && !current.is_empty() && matches!(key, EditorKey::Left | EditorKey::Right) && !ctrl {
            let target = if key == EditorKey::Left { current.selection.get_start_position() } else { current.selection.get_end_position() };
            self.set_selection(select(target, target), None);
            return;
        }
        let (target, keep_desired) = match key {
            EditorKey::Left if ctrl => (cursor::word_left(&model, position), None),
            EditorKey::Right if ctrl => (cursor::word_right(&model, position), None),
            EditorKey::Left => (cursor::left_of(&model, position), None),
            EditorKey::Right => (cursor::right_of(&model, position), None),
            EditorKey::Up | EditorKey::Down | EditorKey::PageUp | EditorKey::PageDown => {
                let delta = match key {
                    EditorKey::Up => -1,
                    EditorKey::Down => 1,
                    EditorKey::PageUp => -page,
                    _ => page,
                };
                let (p, d) = cursor::vertical(&model, position, delta, desired);
                (p, Some(d))
            }
            EditorKey::Home if ctrl => (Position::new(1, 1), None),
            EditorKey::End if ctrl => {
                let last = model.get_line_count();
                (Position::new(last, model.get_line_max_column(last)), None)
            }
            EditorKey::Home => (cursor::home(&model, position), None),
            _ => (cursor::end(&model, position), None),
        };
        let anchor = if mods.shift { current.anchor() } else { target };
        self.set_selection(select(anchor, target), keep_desired);
    }

    // #endregion

    // #region edição

    fn begin_edit(&mut self, kind: EditKind, boundary: bool) {
        let cut = self.moved_since_edit || self.last_edit != Some(kind) || kind == EditKind::Other || boundary;
        if cut {
            self.model.push_stack_element();
        }
        self.last_edit = Some(kind);
        self.moved_since_edit = false;
    }

    /// Aplica as operações com a seleção de antes e o cálculo da seleção de depois.
    fn run_edit(&mut self, ops: &[SingleEditOperation], after: &mut dyn FnMut(&[ValidEditOperation]) -> Selection) {
        let before = vec![self.cursor.selection];
        let mut computer = |inverse: &[ValidEditOperation]| Some(vec![after(inverse)]);
        let result = self.model.push_edit_operations(Some(before), ops, Some(&mut computer), None, None);
        // O evento de conteúdo já deixou a mesma seleção; descarta para o desfazer seguinte não confundir.
        self.take_resulting_selection();
        if let Some(selection) = result.and_then(|v| v.first().copied()) {
            self.cursor = CursorState { selection, desired_visual_column: None };
        }
        self.after_cursor_change();
    }

    /// Digita texto na seleção (o caractere vindo do sistema de entrada ou do colar).
    pub fn type_text(&mut self, text: &str) {
        if text.is_empty() || text.chars().all(char::is_control) && !text.contains('\n') && !text.contains('\t') {
            return;
        }
        let text = self.normalize_eol(text);
        // Espaço depois de texto fecha o grupo de desfazer (`TypingFirstSpace`).
        let p = self.cursor.position();
        let line = self.model.get_line_content(p.line_number);
        let after_word = p.column > 1 && line.units().get((p.column - 2) as usize).is_some_and(|&u| u != 32 && u != 9);
        let boundary = text.chars().all(|c| c == ' ') && after_word;
        self.begin_edit(EditKind::Typing, boundary);
        let range = self.cursor.selection.get_start_position();
        let range = Range::from_positions(range, self.cursor.selection.get_end_position());
        self.run_edit(&[replace(range, Some(&text))], &mut |inv| end_of_last(inv));
    }

    fn normalize_eol(&self, text: &str) -> String {
        let eol = eol_text(&self.model);
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    out.push_str(eol);
                }
                '\n' => out.push_str(eol),
                c => out.push(c),
            }
        }
        out
    }

    fn selection_range(&self) -> Range {
        Range::from_positions(self.cursor.selection.get_start_position(), self.cursor.selection.get_end_position())
    }

    fn new_line(&mut self) {
        self.begin_edit(EditKind::Other, true);
        let start = self.cursor.selection.get_start_position();
        let line = self.model.get_line_content(start.line_number);
        // A mesma indentação da linha (só a parte antes do cursor).
        let indent = String::from_utf16_lossy(get_leading_whitespace(line.units(), 0, (start.column - 1) as usize));
        let text = format!("{}{indent}", eol_text(&self.model));
        let mut op = replace(self.selection_range(), Some(&text));
        op.is_auto_whitespace_edit = true;
        self.run_edit(&[op], &mut |inv| end_of_last(inv));
    }

    fn delete_left(&mut self, by_word: bool) {
        let model = self.model.clone();
        let sel = self.cursor.selection;
        let range = if !sel.is_empty() {
            self.selection_range()
        } else {
            let p = sel.get_position();
            let line = model.get_line_content(p.line_number);
            let tab = model.get_options().tab_size;
            let indent_size = model.get_options().indent_size.max(1);
            let first = model.get_line_first_non_whitespace_column(p.line_number);
            let last_indent_column = if first == 0 { model.get_line_max_column(p.line_number) } else { first };
            if by_word {
                Range::from_positions(cursor::word_left(&model, p), p)
            } else if p.column > 1 && p.column <= last_indent_column && model.get_options().insert_spaces {
                // Dentro da indentação, apaga até a parada de tabulação anterior.
                let from = visible_column_from_column(line.units(), p.column, tab);
                let to = column_from_visible_column(line.units(), prev_render_tab_stop(from, indent_size), tab);
                Range::new(p.line_number, to.min(p.column - 1), p.line_number, p.column)
            } else {
                Range::from_positions(cursor::left_of(&model, p), p)
            }
        };
        if range.is_empty() {
            return;
        }
        self.begin_edit(EditKind::DeleteLeft, false);
        self.run_edit(&[replace(range, None)], &mut |inv| end_of_last(inv));
    }

    fn delete_right(&mut self, by_word: bool) {
        let model = self.model.clone();
        let sel = self.cursor.selection;
        let range = if !sel.is_empty() {
            self.selection_range()
        } else {
            let p = sel.get_position();
            let target = if by_word { cursor::word_right(&model, p) } else { cursor::right_of(&model, p) };
            Range::from_positions(p, target)
        };
        if range.is_empty() {
            return;
        }
        self.begin_edit(EditKind::DeleteRight, false);
        self.run_edit(&[replace(range, None)], &mut |inv| end_of_last(inv));
    }

    fn indent_unit(&self) -> String {
        let options = self.model.get_options();
        if options.insert_spaces { " ".repeat(options.indent_size.max(1) as usize) } else { "\t".to_owned() }
    }

    fn tab(&mut self, outdent: bool) {
        let sel = self.cursor.selection;
        let (start_line, end_line) = (sel.start_line_number, sel.end_line_number);
        let multi_line = start_line != end_line;
        if outdent {
            self.outdent_lines(start_line, end_line);
        } else if multi_line {
            self.indent_lines(start_line, end_line);
        } else {
            let options = self.model.get_options();
            let text = if options.insert_spaces {
                let line = self.model.get_line_content(sel.start_line_number);
                let from = visible_column_from_column(line.units(), sel.start_column, options.tab_size);
                let size = options.indent_size.max(1);
                " ".repeat((size - from % size) as usize)
            } else {
                "\t".to_owned()
            };
            self.begin_edit(EditKind::Other, true);
            self.run_edit(&[replace(self.selection_range(), Some(&text))], &mut |inv| end_of_last(inv));
        }
    }

    /// As linhas que a indentação em bloco atinge (a última fica de fora se a seleção termina na coluna 1).
    fn block_lines(&self, start_line: i32, end_line: i32) -> (i32, i32) {
        let sel = self.cursor.selection;
        let end = if end_line > start_line && sel.end_column == 1 { end_line - 1 } else { end_line };
        (start_line, end)
    }

    fn indent_lines(&mut self, start_line: i32, end_line: i32) {
        let (from, to) = self.block_lines(start_line, end_line);
        let unit = self.indent_unit();
        let len = unit.encode_utf16().count() as i32;
        let ops: Vec<_> = (from..=to).filter(|&l| self.model.get_line_length(l) > 0).map(|l| replace(Range::new(l, 1, l, 1), Some(&unit))).collect();
        let sel = self.cursor.selection;
        let shift = |p: Position| if self.model.get_line_length(p.line_number) > 0 && (from..=to).contains(&p.line_number) { Position::new(p.line_number, p.column + len) } else { p };
        let new = Selection::new(shift(sel.get_selection_start()).line_number, shift(sel.get_selection_start()).column, shift(sel.get_position()).line_number, shift(sel.get_position()).column);
        self.begin_edit(EditKind::Other, true);
        self.run_edit(&ops, &mut |_| new);
    }

    fn outdent_lines(&mut self, start_line: i32, end_line: i32) {
        let (from, to) = self.block_lines(start_line, end_line);
        let options = self.model.get_options();
        let indent_size = options.indent_size.max(1) as usize;
        let mut removed = std::collections::HashMap::new();
        let mut ops = Vec::new();
        for l in from..=to {
            let line = self.model.get_line_content(l);
            let units = line.units();
            let n = match units.first() {
                Some(&9) => 1,
                _ => units.iter().take(indent_size).take_while(|&&u| u == 32).count(),
            };
            if n > 0 {
                removed.insert(l, n as i32);
                ops.push(replace(Range::new(l, 1, l, n as i32 + 1), None));
            }
        }
        if ops.is_empty() {
            return;
        }
        let sel = self.cursor.selection;
        let shift = |p: Position| Position::new(p.line_number, (p.column - removed.get(&p.line_number).copied().unwrap_or(0)).max(1));
        let (a, b) = (shift(sel.get_selection_start()), shift(sel.get_position()));
        let new = Selection::new(a.line_number, a.column, b.line_number, b.column);
        self.begin_edit(EditKind::Other, true);
        self.run_edit(&ops, &mut |_| new);
    }

    // #endregion

    // #region desfazer, refazer e área de transferência

    pub fn undo(&mut self) {
        if !self.model.can_undo() {
            return;
        }
        self.model.undo();
        self.after_history_change();
    }

    pub fn redo(&mut self) {
        if !self.model.can_redo() {
            return;
        }
        self.model.redo();
        self.after_history_change();
    }

    fn after_history_change(&mut self) {
        self.last_edit = None;
        self.moved_since_edit = true;
        if let Some(selection) = self.take_resulting_selection() {
            let selection = self.validated(selection);
            self.cursor = CursorState { selection, desired_visual_column: None };
        } else {
            let p = self.model.validate_position(self.cursor.position());
            self.cursor = CursorState::at(p);
        }
        self.after_cursor_change();
    }

    fn validated(&self, s: Selection) -> Selection {
        let a = self.model.validate_position(s.get_selection_start());
        let b = self.model.validate_position(s.get_position());
        select(a, b)
    }

    /// O texto que copiar leva: a seleção ou, sem ela, a linha inteira com a quebra (`emptySelectionClipboard`).
    fn text_to_copy(&self) -> (String, Range) {
        if self.cursor.is_empty() {
            let line = self.cursor.position().line_number;
            let content = self.model.get_line_content(line).to_string_lossy();
            let range = cursor::line_range_at(&self.model, line);
            (format!("{content}{}", eol_text(&self.model)), range)
        } else {
            (self.model.get_value_in_range(&self.cursor.selection).to_string_lossy(), self.selection_range())
        }
    }

    pub fn copy(&mut self) {
        let (text, _) = self.text_to_copy();
        self.clipboard.set_text(&text);
    }

    pub fn cut(&mut self) {
        let (text, mut range) = self.text_to_copy();
        self.clipboard.set_text(&text);
        if self.cursor.is_empty() && range.start_line_number == range.end_line_number && range.start_line_number > 1 {
            // Última linha: leva a quebra anterior junto para não sobrar uma linha vazia.
            let prev = range.start_line_number - 1;
            range = Range::new(prev, self.model.get_line_max_column(prev), range.end_line_number, range.end_column);
        }
        if range.is_empty() {
            return;
        }
        self.begin_edit(EditKind::Other, true);
        self.run_edit(&[replace(range, None)], &mut |inv| end_of_last(inv));
    }

    pub fn paste(&mut self) {
        let text = self.clipboard.text();
        if text.is_empty() {
            return;
        }
        let text = self.normalize_eol(&text);
        self.begin_edit(EditKind::Other, true);
        self.run_edit(&[replace(self.selection_range(), Some(&text))], &mut |inv| end_of_last(inv));
    }

    // #endregion
}
