//! A entrada do editor: ponteiro, teclado e texto viram comandos do `CodeEditorWidget` (`trigger` com o id do
//! comando, como a `View` do Monaco faz), e a view só rola e pinta o resultado. Cursores, seleções, digitação com
//! fechamento automático, indentação, desfazer, ações de linha e multicursor vêm do porte do núcleo em `je-editor`.

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::core_commands::core_navigation_commands;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::view_model_types::{IViewModel, PlainTextSource};
use je_editor::cursor_common::CursorSimpleModel;
use je_ui::{Modifiers, PointerButton};

use crate::shortcuts::{EditorContext, Resolution, chord_of};
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

/// O que a última cópia levou à área de transferência: o texto e o que o `ClipboardStoredMetadata` do VS Code guarda
/// para o colar (veio de uma seleção vazia, ou de vários cursores).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopiedText {
    pub text: String,
    pub is_from_empty_selection: bool,
    pub multicursor_text: Option<Vec<String>>,
}

fn position_payload(position: Position) -> Dynamic {
    Dynamic::object([("lineNumber", Dynamic::from(f64::from(position.line_number))), ("column", Dynamic::from(f64::from(position.column)))])
}

/// A carga dos comandos de mouse: a posição do modelo e a da view (diferentes com quebra de linha e dobra).
fn mouse_payload(position: Position, view_position: Position, extra: &[(&str, Dynamic)]) -> Dynamic {
    let mut entries = vec![("position", position_payload(position)), ("viewPosition", position_payload(view_position))];
    entries.extend(extra.iter().map(|(k, v)| (*k, v.clone())));
    Dynamic::object(entries)
}

impl EditorView {
    // #region cursor e seleção

    /// O que mudou nos cursores desde `before`: revela o cursor primário e acorda o piscar.
    fn after_edit(&mut self, before: &[Selection], scroll_before: f64) {
        let moved = self.core.selections() != before;
        if moved && self.folding.has_hidden() {
            // O cursor caiu numa linha escondida (desfazer, colar, API): a região desdobra (`revealCursor`).
            let line = self.core.primary().selection_start_line_number;
            self.folding.reveal_line(&self.core, line);
        }
        if moved {
            self.reveal_cursor();
        } else {
            // Um comando de rolagem (`scrollLineDown`...) mexeu só na rolagem do `ViewModel`.
            let vm_scroll = self.core.widget.get_scroll_top();
            if (vm_scroll - scroll_before).abs() > f64::EPSILON {
                self.set_scroll_top(vm_scroll);
            }
        }
        self.reset_blink();
        self.mark_dirty();
    }

    /// Roda `f` sobre o widget mantendo a rolagem do `ViewModel` igual à da view e revelando o cursor depois.
    fn run_command(&mut self, f: impl FnOnce(&crate::core::EditorCore)) {
        let before = self.core.selections();
        let scroll = self.scroll_top();
        self.core.set_scroll(scroll, self.scroll_left());
        f(&self.core);
        self.after_edit(&before, scroll);
    }

    fn trigger(&mut self, handler: &str, payload: Option<Dynamic>) {
        self.run_command(|core| core.trigger(handler, payload));
    }

    /// Seleciona `range` (âncora no começo) e o revela: no centro se a linha está fora da janela, como o
    /// `revealRangeInCenterIfOutsideViewport` da busca.
    pub fn select_range(&mut self, range: Range) {
        // A linha escondida numa região dobrada é desdobrada antes (a busca e o ir para linha revelam o alvo).
        self.folding.update(&self.core);
        self.folding.select_revealing(&self.core, range);
        self.shared.invalid_all.set(true);
        self.sync_scroll_dimensions_now();
        let (first, last) = self.layout.visible_lines(self.scroll_top());
        let view = self.to_view_range(range);
        self.reveal_cursor();
        if view.start_line_number < first || view.end_line_number > last {
            self.scroll_to_center_line(view.start_line_number);
        }
        self.reset_blink();
        self.mark_dirty();
    }

    /// O texto que a busca usa de semente (`seedSearchStringFromSelection: always`): a seleção de uma linha ou, sem
    /// seleção, a palavra sob o cursor.
    pub fn seed_search_string(&self) -> String {
        let selection = self.core.primary();
        if selection.is_empty() {
            let word = self.model.get_word_at_position(selection.get_position());
            return word.map_or_else(String::new, |w| w.word.to_string_lossy());
        }
        if selection.start_line_number != selection.end_line_number {
            return String::new();
        }
        self.model.get_value_in_range(&Range::new(selection.start_line_number, selection.start_column, selection.end_line_number, selection.end_column)).to_string_lossy()
    }

    /// Rola o mínimo para o cursor primário aparecer.
    pub fn reveal_cursor(&mut self) {
        self.sync_scroll_dimensions_now();
        let p = self.core.view_model.get_primary_cursor_state().view_state.position;
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
        self.run_command(|core| {
            core_navigation_commands::instance().select_all.run_core_editor_command(&core.view_model, &Dynamic::Undefined);
        });
    }

    /// Troca o documento inteiro por `text` numa edição só (o recarregar do disco: o desfazer volta ao anterior),
    /// mantendo o cursor onde dá.
    pub fn replace_all(&mut self, text: &str) {
        let range = self.model.get_full_model_range();
        let keep = self.core.selections();
        self.run_command(|core| {
            core.widget.push_undo_stop();
            let edits = vec![je_editor::edit_operation::EditOperation::replace(range, Some(JsString::from(text)))];
            core.widget.execute_edits(je_editor::code_editor_widget::ExecuteEditsSource::Name(Some("reload")), edits, Some(je_editor::code_editor_widget::EndCursorState::Selections(keep)));
            core.widget.push_undo_stop();
        });
    }

    pub fn set_cursor(&mut self, position: Position) {
        let p = self.model.validate_position(position);
        self.run_command(|core| core.widget.set_position(p, Some("keyboard")));
    }

    /// `Ln x, Col y` e o tamanho da seleção para a barra de status; com vários cursores, a quantidade deles.
    pub fn status_text(&self) -> String {
        let selections = self.core.selections();
        let primary = self.core.primary();
        let mut text = if selections.len() > 1 {
            format!("{} selections", selections.len())
        } else {
            let p = primary.get_position();
            let line = self.model.get_line_content(p.line_number);
            let column = je_editor::cursor_columns::to_statusbar_column(line.units(), p.column, self.model.get_options().tab_size);
            format!("Ln {}, Col {}", p.line_number, column)
        };
        let selected: usize = selections.iter().filter(|s| !s.is_empty()).map(|s| self.model.get_value_length_in_range(s)).sum();
        if selected > 0 {
            text.push_str(&format!(" ({selected} selected)"));
        }
        text
    }

    /// O intervalo da seleção primária, ordenado.
    pub fn cursor_selection_range(&self) -> Range {
        let s = self.core.primary();
        Range::from_positions(s.get_start_position(), s.get_end_position())
    }

    pub fn cursor_position(&self) -> Position {
        self.core.primary().get_position()
    }

    /// As seleções de todos os cursores.
    pub fn selections(&self) -> Vec<Selection> {
        self.core.selections()
    }

    // #endregion

    // #region mouse

    /// Botão do mouse apertado. `x` e `y` são relativos à view; `click_count` 1, 2 (palavra) ou 3 (linha). Com Alt
    /// (`multiCursorModifier: altKey`) o clique acrescenta um cursor.
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
        // O chevron de dobra da margem.
        if click_count == 1
            && let Some(line) = self.chevron_line_at(x, y)
        {
            self.toggle_fold_at(line, mods.shift);
            self.reveal_cursor();
            return;
        }
        let view_position = self.view_position_at(x, y);
        let position = self.to_model(view_position);
        let in_margin = x < self.layout.content_left() - crate::layout::LINE_DECORATIONS_WIDTH;
        let count = if in_margin { 3 } else { click_count.clamp(1, 3) };
        let source = ("source", Dynamic::from("mouse"));
        let last = mods.alt && !in_margin;
        let (handler, drag) = match (count, last, mods.shift) {
            (1, true, _) => ("createCursor", DragMode::LastCursorChars),
            (1, false, true) => ("_moveToSelect", DragMode::Chars),
            (1, false, false) => ("_moveTo", DragMode::Chars),
            (2, true, _) => ("lastCursorWordSelect", DragMode::LastCursorWords),
            (2, false, _) => ("_wordSelect", DragMode::Words),
            (_, true, _) => ("lastCursorLineSelect", DragMode::LastCursorLines),
            (_, false, _) => ("_lineSelect", DragMode::Lines),
        };
        self.drag = Some(drag);
        self.core_mouse(handler, mouse_payload(position, view_position, &[source]));
        // O clique no fim de uma linha dobrada (na reticência) desdobra a região.
        if count == 1
            && !last
            && let Some(line) = self.collapsed_line_end(position)
        {
            self.toggle_fold_at(line, false);
        }
    }

    fn core_mouse(&mut self, handler: &str, payload: Dynamic) {
        let before = self.core.selections();
        let scroll = self.scroll_top();
        self.core.set_scroll(scroll, self.scroll_left());
        self.core.widget.trigger(Some("mouse"), handler, Some(payload));
        self.after_edit(&before, scroll);
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
        let view_position = self.view_position_at(x, y.clamp(0.0, self.layout.height - 1.0));
        let position = self.to_model(view_position);
        let source = ("source", Dynamic::from("mouse"));
        let handler = match mode {
            DragMode::Chars => "_moveToSelect",
            DragMode::Words => "_wordSelectDrag",
            DragMode::Lines => "_lineSelectDrag",
            DragMode::LastCursorChars => "_lastCursorMoveToSelect",
            DragMode::LastCursorWords => "lastCursorWordSelect",
            DragMode::LastCursorLines => "lastCursorLineSelectDrag",
            DragMode::Scrollbar(_) | DragMode::Minimap => return,
        };
        self.core_mouse(handler, mouse_payload(position, view_position, &[source]));
    }

    pub fn mouse_up(&mut self) {
        self.drag = None;
    }

    // #endregion

    // #region teclado

    /// Uma tecla de navegação ou de comando. Devolve `true` se o editor a tratou.
    pub fn key_down(&mut self, key: EditorKey, mods: Modifiers) -> bool {
        let ctrl = mods.ctrl || mods.meta;
        // As teclas da área de transferência são do sistema (o `copy`, `cut` e `paste` do navegador).
        if let EditorKey::Char(c) = key
            && ctrl
            && !mods.alt
            && !self.shortcuts.is_pending()
        {
            match (c, mods.shift) {
                ('c', false) => {
                    self.copy();
                    return true;
                }
                ('x', false) => {
                    self.cut();
                    return true;
                }
                ('v', false) => {
                    self.paste();
                    return true;
                }
                _ => {}
            }
        }
        let selections = self.core.selections();
        let context = EditorContext { has_selection: selections.iter().any(|s| !s.is_empty()), has_multiple_selections: selections.len() > 1, language_id: self.model.get_language_id() };
        match self.shortcuts.resolve(chord_of(key, mods), &context) {
            Resolution::Command(id, args) => {
                self.run_shortcut(&id, args);
                true
            }
            Resolution::Pending => true,
            Resolution::None => match key {
                EditorKey::Enter => {
                    self.type_text("\n");
                    true
                }
                _ => false,
            },
        }
    }

    /// Roda o comando de um atalho: os que o VS Code liga ao `MultiCommand` (desfazer, refazer, selecionar tudo e a
    /// área de transferência) e os que a view trata vão pelas implementações dela, os do editor pelo `trigger` do
    /// widget, e os que não são do editor (um `workbench.*` do `keybindings.json`) ficam para o shell
    /// ([`EditorView::take_workbench_command`]).
    fn run_shortcut(&mut self, id: &str, args: Dynamic) {
        match id {
            "undo" | "default:undo" => self.undo(),
            "redo" | "default:redo" => self.redo(),
            "editor.action.selectAll" => self.select_all(),
            "editor.action.clipboardCopyAction" => self.copy(),
            "editor.action.clipboardCutAction" => self.cut(),
            "editor.action.clipboardPasteAction" => self.paste(),
            "editor.fold" => self.fold(),
            "editor.unfold" => self.unfold(),
            "editor.foldAll" => self.fold_all(),
            "editor.unfoldAll" => self.unfold_all(),
            "editor.action.toggleWordWrap" => self.toggle_word_wrap(),
            other if self.core.knows_command(other) => self.trigger(other, if args.is_nullish() { None } else { Some(args) }),
            other => self.workbench_commands.push_back((other.to_owned(), args)),
        }
    }

    /// O próximo comando de atalho que não é do editor (o shell roda: `workbench.action.quickOpen`...), com os
    /// argumentos do `keybindings.json`.
    pub fn take_workbench_command(&mut self) -> Option<(String, Dynamic)> {
        self.workbench_commands.pop_front()
    }

    // #endregion

    // #region edição

    /// Digita texto nos cursores (o caractere vindo do sistema de entrada ou do colar).
    pub fn type_text(&mut self, text: &str) {
        if text.is_empty() || text.chars().all(char::is_control) && !text.contains('\n') && !text.contains('\t') {
            return;
        }
        self.shortcuts.cancel();
        self.run_command(|core| core.type_text(text));
    }

    // #endregion

    // #region participantes do salvamento

    /// Os participantes do salvamento do VS Code (`saveParticipants.ts`), na ordem em que ele os registra:
    /// `files.trimTrailingWhitespace`, `files.insertFinalNewline` e `files.trimFinalNewlines`, cada um como uma
    /// edição que o desfazer volta. O shell chama antes de gravar; `auto_saved` é o salvamento automático, que não
    /// mexe nas linhas dos cursores.
    pub fn run_save_participants(&mut self, auto_saved: bool) {
        use je_editor::edit_operation::EditOperation;
        let settings = self.settings.clone();
        let model = self.model.clone();
        let before = self.core.selections();
        let mut changed = false;
        if settings.trim_trailing_whitespace {
            let mut cursors: Vec<Position> = if auto_saved { before.iter().map(|s| s.get_position()).collect() } else { Vec::new() };
            let ops = je_editor::trim_trailing_whitespace_command::trim_trailing_whitespace(&model, &mut cursors, settings.trim_in_regex_and_strings);
            if !ops.is_empty() {
                let keep = before.clone();
                let mut computer = move |_: &[je_editor::model::ValidEditOperation]| Some(keep.clone());
                model.push_edit_operations(Some(before.clone()), &ops, Some(&mut computer), None, None);
                changed = true;
            }
        }
        if settings.insert_final_newline {
            let line_count = model.get_line_count();
            let last = model.get_line_content(line_count);
            if je_base::strings::last_non_whitespace_index(last.units(), last.len() as i64 - 1).is_some() {
                let eol = JsString::from_units(model.get_eol().as_units().to_vec());
                let edits = vec![EditOperation::insert(Position::new(line_count, model.get_line_max_column(line_count)), eol)];
                let selections = self.core.selections();
                self.core.widget.execute_edits(je_editor::code_editor_widget::ExecuteEditsSource::Name(Some("insertFinalNewLine")), edits, Some(je_editor::code_editor_widget::EndCursorState::Selections(selections)));
                changed = true;
            }
        }
        if settings.trim_final_newlines && model.get_line_count() > 1 {
            let line_count = model.get_line_count();
            let selections = self.core.selections();
            let cannot_touch = if auto_saved { selections.iter().map(|s| s.position_line_number).max().unwrap_or(0) } else { 0 };
            let last_non_empty = (1..=line_count).rev().find(|l| model.get_line_length(*l) > 0).unwrap_or(0);
            let from = (last_non_empty + 1).max(cannot_touch + 1);
            let range = model.validate_range(&Range::new(from, 1, line_count, model.get_line_max_column(line_count)));
            if !range.is_empty() {
                let keep = selections.clone();
                let mut computer = move |_: &[je_editor::model::ValidEditOperation]| Some(keep.clone());
                model.push_edit_operations(Some(selections.clone()), &[EditOperation::delete(range)], Some(&mut computer), None, None);
                self.core.widget.set_selections(&selections, Some("api"), None);
                changed = true;
            }
        }
        if changed {
            self.mark_dirty();
        }
    }

    // #endregion

    // #region desfazer, refazer e área de transferência

    pub fn undo(&mut self) {
        self.run_command(|core| {
            core_editing_undo(core, false);
        });
    }

    pub fn redo(&mut self) {
        self.run_command(|core| {
            core_editing_undo(core, true);
        });
    }

    /// O que copiar leva (`getDataToCopy`): o texto das seleções, ou das linhas dos cursores se todas as seleções
    /// são vazias (`emptySelectionClipboard`).
    fn data_to_copy(&self) -> CopiedText {
        let selections = self.core.selections();
        let ranges: Vec<Range> = selections.iter().map(|s| Range::from_positions(s.get_start_position(), s.get_end_position())).collect();
        let copied = self.core.view_model.get_plain_text_to_copy(&ranges, true, false);
        let eol = if self.model.get_eol() == je_editor::model::Eol::CrLf { "\r\n" } else { "\n" };
        let (text, multicursor_text) = match copied.source_text {
            PlainTextSource::Text(text) => (text.to_string_lossy(), None),
            PlainTextSource::Lines(lines) => {
                let lines: Vec<String> = lines.iter().map(JsString::to_string_lossy).collect();
                (lines.join(eol), Some(lines))
            }
        };
        CopiedText { text, is_from_empty_selection: selections.len() == 1 && selections[0].is_empty(), multicursor_text }
    }

    pub fn copy(&mut self) {
        let data = self.data_to_copy();
        self.clipboard.set_text(&data.text);
        self.copied = Some(data);
    }

    pub fn cut(&mut self) {
        self.copy();
        self.run_command(|core| core.trigger("cut", None));
    }

    pub fn paste(&mut self) {
        let text = self.clipboard.text();
        if text.is_empty() {
            return;
        }
        let meta = self.copied.clone().filter(|c| c.text == text);
        let mut entries = vec![("text", Dynamic::from(JsString::from(text.as_str())))];
        if let Some(meta) = meta {
            entries.push(("pasteOnNewLine", Dynamic::Bool(meta.is_from_empty_selection)));
            if let Some(lines) = meta.multicursor_text {
                entries.push(("multicursorText", Dynamic::array(lines.iter().map(|l| Dynamic::from(JsString::from(l.as_str()))))));
            }
        }
        self.shortcuts.cancel();
        self.run_command(|core| core.trigger("paste", Some(Dynamic::object(entries))));
    }

    // #endregion
}

/// `CoreEditingCommands.Undo` e `Redo`: o `model.undo()` do comando de núcleo.
fn core_editing_undo(core: &crate::core::EditorCore, redo: bool) {
    use je_editor::core_commands::{EditorOrNativeTextInputCommand, core_editing_commands};
    let commands = core_editing_commands::instance();
    let command = if redo { &commands.redo } else { &commands.undo };
    let _ = command.run_editor_command(None, &core.widget, &Dynamic::Undefined);
}
