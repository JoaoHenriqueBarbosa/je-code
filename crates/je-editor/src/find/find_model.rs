//! Porte de `src/vs/editor/contrib/find/browser/findModel.ts`: o modelo de busca ligado ao modelo de texto do editor
//! (`FindModelBoundToEditorModel`), as chaves de contexto da busca, os atalhos de alternar opções, os ids das ações
//! (`FIND_IDS`) e o limite de ocorrências (`MATCHES_LIMIT`).
//!
//! Representação:
//! - `IActiveCodeEditor` é um [`Weak`] do [`CodeEditorWidget`] (o editor é dono do controlador que é dono deste modelo, um
//!   `Rc` formaria um ciclo). O estado ([`FindReplaceState`]) é um `Rc` compartilhado com o controlador.
//! - `_toDispose` é um [`Disposable`]; `TimeoutTimer` e `RunOnceScheduler` são os de `je_base::async_`. Os ouvintes de
//!   evento seguram um `Weak` do modelo (ver [`super::listen_weak`]).
//! - `research(moveCursor, newFindScope?)` recebe `Option<Option<Vec<Range>>>` (`None` é `undefined`, `Some(None)` é
//!   `null`, e o `Range` solto do original é uma lista de um elemento).
//! - `this._editor.getOption(EditorOption.wordSeparators)` só vale com `wholeWord`: [`FindModelBoundToEditorModel::word_separators`].
//! - As chaves de contexto (`RawContextKey`) são `thread_local` e as funções `context_*` entregam cópias. Os atalhos
//!   (`IKeybindings`) são funções que montam o [`Keybindings`].
//! - `_executeEditorCommand` restaura `_ignoreModelContentChanged` ao sair do escopo (o `finally`), pelo [`IgnoreGuard`].
//!
//! Divergências:
//! - `_largeReplaceAll` usa `String.prototype.replace` com uma `RegExp`: a função de troca recebe, como em JS, os
//!   argumentos `(match, ...groups, offset, string[, groups])` e `buildReplaceString` os lê por posição (o que faz um
//!   `$n` além dos grupos ler o `offset` ou o texto inteiro, como no original); a troca por texto fixo expande `$$`,
//!   `$&`, `` $` ``, `$'`, `$n` e `$<nome>` ([`get_substitution`]), porque o `replace` do JS também expande.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{RunOnceScheduler, TimeoutTimer};
use je_base::js_regexp::{Captures, replace_all};
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::contextkey::{ContextKeyExpression, ContextKeyMeta, RawContextKey};
use je_platform::keybinding::keybindings_registry::{Keybindings, PlatformKeybindings};

use super::find_decorations::FindDecorations;
use super::find_state::{FindReplaceState, FindReplaceStateChangedEvent};
use super::replace_all_command::ReplaceAllCommand;
use super::replace_pattern::{ReplacePattern, parse_replace_string};
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::find::FIND;
use crate::config::editor_options::registry::WORD_SEPARATORS;
use crate::core_commands::key;
use crate::cursor_common::CursorSimpleModel;
use crate::cursor_events::{CursorChangeReason, ICursorPositionChangedEvent};
use crate::editor_common::{Command, ScrollType};
use crate::model::{EndOfLinePreference, FindMatch};
use crate::position::Position;
use crate::range::Range;
use crate::replace_command::{ReplaceCommand, ReplaceCommandThatPreservesSelection};
use crate::selection::Selection;
use crate::text_model::{FindScope, TextModel};
use crate::text_model_search::SearchParams;

thread_local! {
    static CONTEXT_FIND_WIDGET_VISIBLE: RawContextKey = RawContextKey::new("findWidgetVisible", false, ContextKeyMeta::None);
    // Keep ContextKey use of 'Focussed' to not break when clauses
    static CONTEXT_FIND_INPUT_FOCUSED: RawContextKey = RawContextKey::new("findInputFocussed", false, ContextKeyMeta::None);
    static CONTEXT_REPLACE_INPUT_FOCUSED: RawContextKey = RawContextKey::new("replaceInputFocussed", false, ContextKeyMeta::None);
    /// Context key that is true when any element within the Find widget has focus. This includes the Find input,
    /// Replace input, checkboxes, buttons, etc.
    static CONTEXT_FIND_WIDGET_FOCUSED: RawContextKey = RawContextKey::new("findWidgetFocused", false, ContextKeyMeta::None);
}

/// `CONTEXT_FIND_WIDGET_VISIBLE`.
pub fn context_find_widget_visible() -> RawContextKey {
    CONTEXT_FIND_WIDGET_VISIBLE.with(Clone::clone)
}

/// `CONTEXT_FIND_WIDGET_NOT_VISIBLE`.
pub fn context_find_widget_not_visible() -> ContextKeyExpression {
    context_find_widget_visible().to_negated()
}

/// `CONTEXT_FIND_INPUT_FOCUSED`.
pub fn context_find_input_focused() -> RawContextKey {
    CONTEXT_FIND_INPUT_FOCUSED.with(Clone::clone)
}

/// `CONTEXT_REPLACE_INPUT_FOCUSED`.
pub fn context_replace_input_focused() -> RawContextKey {
    CONTEXT_REPLACE_INPUT_FOCUSED.with(Clone::clone)
}

/// `CONTEXT_FIND_WIDGET_FOCUSED`.
pub fn context_find_widget_focused() -> RawContextKey {
    CONTEXT_FIND_WIDGET_FOCUSED.with(Clone::clone)
}

/// O atalho `Alt+tecla` (`CtrlCmd+Alt+tecla` no macOS) de alternar uma opção da busca.
fn alt_toggle_keybinding(key_code: KeyCode) -> Keybindings {
    Keybindings {
        primary: Some(key(KeyMod::ALT, key_code)),
        mac: Some(PlatformKeybindings { primary: Some(key(KeyMod::CTRL_CMD | KeyMod::ALT, key_code)), secondary: None }),
        ..Keybindings::default()
    }
}

/// `ToggleCaseSensitiveKeybinding`.
pub fn toggle_case_sensitive_keybinding() -> Keybindings {
    alt_toggle_keybinding(KeyCode::KeyC)
}

/// `ToggleWholeWordKeybinding`.
pub fn toggle_whole_word_keybinding() -> Keybindings {
    alt_toggle_keybinding(KeyCode::KeyW)
}

/// `ToggleRegexKeybinding`.
pub fn toggle_regex_keybinding() -> Keybindings {
    alt_toggle_keybinding(KeyCode::KeyR)
}

/// `ToggleSearchScopeKeybinding`.
pub fn toggle_search_scope_keybinding() -> Keybindings {
    alt_toggle_keybinding(KeyCode::KeyL)
}

/// `TogglePreserveCaseKeybinding`.
pub fn toggle_preserve_case_keybinding() -> Keybindings {
    alt_toggle_keybinding(KeyCode::KeyP)
}

/// `FIND_IDS`.
pub mod find_ids {
    pub const START_FIND_ACTION: &str = "actions.find";
    pub const START_FIND_WITH_SELECTION: &str = "actions.findWithSelection";
    pub const START_FIND_WITH_ARGS: &str = "editor.actions.findWithArgs";
    pub const NEXT_MATCH_FIND_ACTION: &str = "editor.action.nextMatchFindAction";
    pub const PREVIOUS_MATCH_FIND_ACTION: &str = "editor.action.previousMatchFindAction";
    pub const GO_TO_MATCH_FIND_ACTION: &str = "editor.action.goToMatchFindAction";
    pub const NEXT_SELECTION_MATCH_FIND_ACTION: &str = "editor.action.nextSelectionMatchFindAction";
    pub const PREVIOUS_SELECTION_MATCH_FIND_ACTION: &str = "editor.action.previousSelectionMatchFindAction";
    pub const START_FIND_REPLACE_ACTION: &str = "editor.action.startFindReplaceAction";
    pub const CLOSE_FIND_WIDGET_COMMAND: &str = "closeFindWidget";
    pub const TOGGLE_CASE_SENSITIVE_COMMAND: &str = "toggleFindCaseSensitive";
    pub const TOGGLE_WHOLE_WORD_COMMAND: &str = "toggleFindWholeWord";
    pub const TOGGLE_REGEX_COMMAND: &str = "toggleFindRegex";
    pub const TOGGLE_SEARCH_SCOPE_COMMAND: &str = "toggleFindInSelection";
    pub const TOGGLE_PRESERVE_CASE_COMMAND: &str = "togglePreserveCase";
    pub const REPLACE_ONE_ACTION: &str = "editor.action.replaceOne";
    pub const REPLACE_ALL_ACTION: &str = "editor.action.replaceAll";
    pub const SELECT_ALL_MATCHES_ACTION: &str = "editor.action.selectAllMatches";
}

/// `MATCHES_LIMIT`.
pub const MATCHES_LIMIT: usize = 19999;
const RESEARCH_DELAY: u64 = 240;
/// `Constants.MAX_SAFE_SMALL_INTEGER` como limite de resultados.
const MAX_RESULT_COUNT: usize = je_base::uint::constants::MAX_SAFE_SMALL_INTEGER as usize;

/// Restaura `_ignoreModelContentChanged` ao sair do escopo (o `finally` de `_executeEditorCommand`).
struct IgnoreGuard<'a>(&'a Cell<bool>);

impl<'a> IgnoreGuard<'a> {
    fn new(flag: &'a Cell<bool>) -> Self {
        flag.set(true);
        Self(flag)
    }
}

impl Drop for IgnoreGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// `GetSubstitution` de `String.prototype.replace(regex, string)`: expande `$$`, `$&`, `` $` ``, `$'`, `$n`, `$nn` e
/// `$<nome>` na `replacement` de um casamento de `input`.
fn get_substitution(captures: &Captures<'_>, replacement: &[u16]) -> JsString {
    const DOLLAR: u16 = b'$' as u16;
    let found = captures.matched;
    let input = captures.input;
    let position = found.start();
    let match_end = found.end();
    let capture_count = found.captures.len() - 1;
    let is_digit = |unit: u16| (u16::from(b'0')..=u16::from(b'9')).contains(&unit);
    let len = replacement.len();
    let mut out = JsString::new();
    let mut i = 0;
    while i < len {
        let unit = replacement[i];
        if unit != DOLLAR || i + 1 >= len {
            out.push_unit(unit);
            i += 1;
            continue;
        }
        let next = replacement[i + 1];
        if next == DOLLAR {
            out.push_unit(DOLLAR);
            i += 2;
        } else if next == u16::from(b'&') {
            out.push_units(&input[position..match_end]);
            i += 2;
        } else if next == u16::from(b'`') {
            out.push_units(&input[..position]);
            i += 2;
        } else if next == u16::from(b'\'') {
            out.push_units(&input[match_end.min(input.len())..]);
            i += 2;
        } else if is_digit(next) {
            let first_digit = usize::from(next - u16::from(b'0'));
            let mut digit_count = 1;
            let mut index = first_digit;
            if let Some(&second) = replacement.get(i + 2).filter(|&&unit| is_digit(unit)) {
                digit_count = 2;
                index = first_digit * 10 + usize::from(second - u16::from(b'0'));
                if index > capture_count {
                    digit_count = 1;
                    index = first_digit;
                }
            }
            if (1..=capture_count).contains(&index) {
                if let Some(group) = captures.group(index) {
                    out.push_units(group);
                }
            } else {
                out.push_units(&replacement[i..i + 1 + digit_count]);
            }
            i += 1 + digit_count;
        } else if next == u16::from(b'<') && !found.groups.is_empty() {
            match replacement[i + 2..].iter().position(|&unit| unit == u16::from(b'>')) {
                Some(offset) => {
                    let end = i + 2 + offset;
                    let name = String::from_utf16_lossy(&replacement[i + 2..end]);
                    if let Some(range) = found.group(&name) {
                        out.push_units(&input[range]);
                    }
                    i = end + 1;
                }
                None => {
                    out.push_unit(DOLLAR);
                    i += 1;
                }
            }
        } else {
            out.push_unit(DOLLAR);
            i += 1;
        }
    }
    out
}

/// O maior número que segue um `$` na cadeia de substituição (o maior índice de grupo que ela pode ler).
fn max_referenced_group_index(replace_string: &[u16]) -> usize {
    let mut max_index = 0usize;
    let mut i = 0;
    while i < replace_string.len() {
        if replace_string[i] == u16::from(b'$') {
            let mut value = 0usize;
            let mut j = i + 1;
            while j < replace_string.len() && (u16::from(b'0')..=u16::from(b'9')).contains(&replace_string[j]) {
                value = value.saturating_mul(10).saturating_add(usize::from(replace_string[j] - u16::from(b'0')));
                j += 1;
            }
            max_index = max_index.max(value);
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    max_index
}

/// `FindModelBoundToEditorModel`.
pub struct FindModelBoundToEditorModel {
    editor: Weak<CodeEditorWidget>,
    state: Rc<FindReplaceState>,
    to_dispose: Rc<Disposable>,
    decorations: Rc<FindDecorations>,
    ignore_model_content_changed: Cell<bool>,
    start_searching_timer: Rc<TimeoutTimer>,
    update_decorations_scheduler: Rc<RunOnceScheduler>,
    is_disposed: Cell<bool>,
}

impl FindModelBoundToEditorModel {
    pub fn new(editor: &Rc<CodeEditorWidget>, state: Rc<FindReplaceState>) -> Rc<Self> {
        let this = Rc::new_cyclic(|me: &Weak<Self>| {
            let to_dispose = Disposable::new();
            let decorations = Rc::new(FindDecorations::new(editor));
            to_dispose.register(decorations.clone());

            let runner_me = me.clone();
            let update_decorations_scheduler = RunOnceScheduler::new(
                move || {
                    let Some(this) = runner_me.upgrade() else {
                        return;
                    };
                    if !this.editor().has_model() {
                        return;
                    }
                    this.research(false, None);
                },
                100,
            );
            to_dispose.register(update_decorations_scheduler.clone());

            Self {
                editor: Rc::downgrade(editor),
                state,
                to_dispose,
                decorations,
                ignore_model_content_changed: Cell::new(false),
                start_searching_timer: TimeoutTimer::new(),
                update_decorations_scheduler,
                is_disposed: Cell::new(false),
            }
        });

        this.to_dispose.register(super::listen_weak(editor.on_did_change_cursor_position(), &this, |this, e: &ICursorPositionChangedEvent| {
            if matches!(e.reason, CursorChangeReason::Explicit | CursorChangeReason::Undo | CursorChangeReason::Redo) {
                this.decorations.set_start_position(this.editor().get_position().expect("o editor com modelo tem posição"));
            }
        }));

        this.to_dispose.register(super::listen_weak(editor.on_did_change_model_content(), &this, |this, e| {
            if this.ignore_model_content_changed.get() {
                return;
            }
            if e.is_flush {
                // a model.setValue() was called
                this.decorations.reset();
            }
            this.decorations.set_start_position(this.editor().get_position().expect("o editor com modelo tem posição"));
            this.update_decorations_scheduler.schedule(None);
        }));

        this.to_dispose
            .register(super::listen_weak(this.state.on_find_replace_state_change(), &this, |this, e| this.on_state_changed(e)));

        let search_scope = this.state.search_scope();
        this.research(false, Some(search_scope));
        this
    }

    /// O editor, que vive enquanto o modelo de busca existe.
    fn editor(&self) -> Rc<CodeEditorWidget> {
        self.editor.upgrade().expect("o editor vive enquanto o modelo de busca existe")
    }

    /// O modelo de texto do editor (o modelo de busca só existe com modelo).
    fn model(&self) -> Rc<TextModel> {
        self.editor().get_model().expect("o editor tem modelo enquanto o modelo de busca existe")
    }

    /// `this._state.wholeWord ? this._editor.getOption(EditorOption.wordSeparators) : null`.
    fn word_separators<'a>(&self, editor: &'a Rc<CodeEditorWidget>) -> Option<&'a str> {
        if self.state.whole_word() { Some(editor.get_option(WORD_SEPARATORS).as_str()) } else { None }
    }

    fn on_state_changed(self: &Rc<Self>, e: &FindReplaceStateChangedEvent) {
        if self.is_disposed.get() {
            // The find model is disposed during a find state changed event
            return;
        }
        if !self.editor().has_model() {
            // The find model will be disposed momentarily
            return;
        }
        if e.search_string || e.is_replace_revealed || e.is_regex || e.whole_word || e.match_case || e.search_scope {
            let model = self.model();
            let e = *e;

            if model.is_too_large_for_syncing() {
                self.start_searching_timer.cancel();

                let me = Rc::downgrade(self);
                self.start_searching_timer.set_if_not_set(
                    move || {
                        if let Some(this) = me.upgrade() {
                            this.research_after_state_change(&e);
                        }
                    },
                    RESEARCH_DELAY,
                );
            } else {
                self.research_after_state_change(&e);
            }
        }
    }

    /// O `if (e.searchScope) { research(e.moveCursor, searchScope) } else { research(e.moveCursor) }` de `_onStateChanged`.
    fn research_after_state_change(&self, e: &FindReplaceStateChangedEvent) {
        if e.search_scope {
            self.research(e.move_cursor, Some(self.state.search_scope()));
        } else {
            self.research(e.move_cursor, None);
        }
    }

    /// `_getSearchRange(model, findScope)`.
    fn get_search_range(model: &TextModel, find_scope: Option<Range>) -> Range {
        // If we have set now or before a find scope, use it for computing the search range
        if let Some(find_scope) = find_scope {
            return find_scope;
        }

        model.get_full_model_range()
    }

    fn research(&self, move_cursor: bool, new_find_scope: Option<Option<Vec<Range>>>) {
        let editor = self.editor();
        let model = self.model();
        let mut find_scopes: Option<Vec<Range>> = match new_find_scope {
            Some(new_find_scope) => new_find_scope,
            None => self.decorations.get_find_scopes(),
        };
        if let Some(scopes) = find_scopes.as_mut() {
            for find_scope in scopes.iter_mut() {
                if find_scope.start_line_number != find_scope.end_line_number {
                    let mut end_line_number = find_scope.end_line_number;

                    if find_scope.end_column == 1 {
                        end_line_number -= 1;
                    }

                    *find_scope = Range::new(find_scope.start_line_number, 1, end_line_number, model.get_line_max_column(end_line_number));
                }
            }
        }

        let find_matches = self.find_matches(find_scopes.as_deref(), false, MATCHES_LIMIT);
        self.decorations.set(&find_matches, find_scopes.as_deref());

        let editor_selection: Range = *editor.get_selection().expect("o editor com modelo tem seleção");
        let mut current_matches_position = self.decorations.get_current_matches_position(editor_selection);
        if current_matches_position == 0 && !find_matches.is_empty() {
            // current selection is not on top of a match
            // try to find its nearest result from the top of the document
            let match_after_selection = find_matches
                .iter()
                .position(|find_match| Range::compare_ranges_using_starts(Some(&find_match.range), Some(&editor_selection)) >= 0)
                .unwrap_or(find_matches.len());
            if match_after_selection > 0 {
                // match position is one based
                current_matches_position = match_after_selection;
            }
        }

        self.state.change_match_info(current_matches_position, self.decorations.get_count(), None);

        if move_cursor && editor.get_option(FIND).cursor_move_on_type {
            self.move_to_next_match_from(self.decorations.get_start_position());
        }
    }

    fn has_matches(&self) -> bool {
        self.state.matches_count() > 0
    }

    fn cannot_find(&self) -> bool {
        if !self.has_matches() {
            if let Some(find_scope) = self.decorations.get_find_scope() {
                // Reveal the selection so user is reminded that 'selection find' is on.
                self.editor().reveal_range_in_center_if_outside_viewport(find_scope, Some(ScrollType::Smooth));
            }
            return true;
        }
        false
    }

    fn set_current_find_match(&self, find_match: Range) {
        let matches_position = self.decorations.set_current_find_match(Some(find_match));
        self.state.change_match_info(matches_position, self.decorations.get_count(), Some(find_match));

        let editor = self.editor();
        editor.set_selection(find_match.into(), None);
        editor.reveal_range_in_center_if_outside_viewport(find_match, Some(ScrollType::Smooth));
    }

    /// O `isUsingLineStops` de `_prevSearchPosition` e de `_nextSearchPosition`.
    fn is_using_line_stops(&self) -> bool {
        let search_string = self.state.search_string();
        self.state.is_regex() && (search_string.units().contains(&u16::from(b'^')) || search_string.units().contains(&u16::from(b'$')))
    }

    fn prev_search_position(&self, before: Position) -> Position {
        let is_using_line_stops = self.is_using_line_stops();
        let Position { mut line_number, mut column } = before;
        let model = self.model();

        if is_using_line_stops || column == 1 {
            if line_number == 1 {
                line_number = model.get_line_count();
            } else {
                line_number -= 1;
            }
            column = model.get_line_max_column(line_number);
        } else {
            column -= 1;
        }

        Position::new(line_number, column)
    }

    fn move_to_prev_match_from(&self, before: Position, is_recursed: bool) {
        let mut before = before;
        if !self.state.can_navigate_back() {
            // we are beyond the first matched find result
            // instead of doing nothing, we should refocus the first item
            if let Some(next_match_range) = self.decorations.match_after_position(before) {
                self.set_current_find_match(next_match_range);
            }
            return;
        }
        if self.decorations.get_count() < MATCHES_LIMIT {
            let mut prev_match_range = self.decorations.match_before_position(before);

            if let Some(range) = prev_match_range
                && range.is_empty()
                && range.get_start_position() == before
            {
                before = self.prev_search_position(before);
                prev_match_range = self.decorations.match_before_position(before);
            }

            if let Some(prev_match_range) = prev_match_range {
                self.set_current_find_match(prev_match_range);
            }

            return;
        }

        if self.cannot_find() {
            return;
        }

        let find_scope = self.decorations.get_find_scope();
        let editor = self.editor();
        let model = self.model();
        let search_range = Self::get_search_range(&model, find_scope);

        // ...(----)...|...
        if search_range.get_end_position().is_before(&before) {
            before = search_range.get_end_position();
        }

        // ...|...(----)...
        if before.is_before(&search_range.get_start_position()) {
            before = search_range.get_end_position();
        }

        let mut position = before;

        let search_string = self.state.search_string();
        let (is_regex, match_case) = (self.state.is_regex(), self.state.match_case());
        let word_separators = self.word_separators(&editor);
        let mut prev_match = model.find_previous_match(search_string.clone(), position, is_regex, match_case, word_separators, false);

        if let Some(found) = &prev_match
            && found.range.is_empty()
            && found.range.get_start_position() == position
        {
            // Looks like we're stuck at this position, unacceptable!
            position = self.prev_search_position(position);
            prev_match = model.find_previous_match(search_string, position, is_regex, match_case, word_separators, false);
        }

        let Some(prev_match) = prev_match else {
            // there is precisely one match and selection is on top of it
            return;
        };

        if !is_recursed && !search_range.contains_range(&prev_match.range) {
            return self.move_to_prev_match_from(prev_match.range.get_start_position(), true);
        }

        self.set_current_find_match(prev_match.range);
    }

    pub fn move_to_prev_match(&self) {
        let selection = self.editor().get_selection().expect("o editor com modelo tem seleção");
        self.move_to_prev_match_from(selection.get_start_position(), false);
    }

    fn next_search_position(&self, after: Position) -> Position {
        let is_using_line_stops = self.is_using_line_stops();

        let Position { mut line_number, mut column } = after;
        let model = self.model();

        if is_using_line_stops || column == model.get_line_max_column(line_number) {
            if line_number == model.get_line_count() {
                line_number = 1;
            } else {
                line_number += 1;
            }
            column = 1;
        } else {
            column += 1;
        }

        Position::new(line_number, column)
    }

    fn move_to_next_match_from(&self, after: Position) {
        let mut after = after;
        if !self.state.can_navigate_forward() {
            // we are beyond the last matched find result
            // instead of doing nothing, we should refocus the last item
            if let Some(prev_match_range) = self.decorations.match_before_position(after) {
                self.set_current_find_match(prev_match_range);
            }
            return;
        }
        if self.decorations.get_count() < MATCHES_LIMIT {
            let mut next_match_range = self.decorations.match_after_position(after);

            if let Some(range) = next_match_range
                && range.is_empty()
                && range.get_start_position() == after
            {
                // Looks like we're stuck at this position, unacceptable!
                after = self.next_search_position(after);
                next_match_range = self.decorations.match_after_position(after);
            }
            if let Some(next_match_range) = next_match_range {
                self.set_current_find_match(next_match_range);
            }

            return;
        }

        if let Some(next_match) = self.get_next_match(after, false, true, false) {
            self.set_current_find_match(next_match.range);
        }
    }

    fn get_next_match(&self, after: Position, capture_matches: bool, force_move: bool, is_recursed: bool) -> Option<FindMatch> {
        if self.cannot_find() {
            return None;
        }

        let mut after = after;
        let find_scope = self.decorations.get_find_scope();
        let editor = self.editor();
        let model = self.model();
        let search_range = Self::get_search_range(&model, find_scope);

        // ...(----)...|...
        if search_range.get_end_position().is_before(&after) {
            after = search_range.get_start_position();
        }

        // ...|...(----)...
        if after.is_before(&search_range.get_start_position()) {
            after = search_range.get_start_position();
        }

        let mut position = after;

        let search_string = self.state.search_string();
        let (is_regex, match_case) = (self.state.is_regex(), self.state.match_case());
        let word_separators = self.word_separators(&editor);
        let mut next_match = model.find_next_match(search_string.clone(), position, is_regex, match_case, word_separators, capture_matches);

        if force_move
            && let Some(found) = &next_match
            && found.range.is_empty()
            && found.range.get_start_position() == position
        {
            // Looks like we're stuck at this position, unacceptable!
            position = self.next_search_position(position);
            next_match = model.find_next_match(search_string, position, is_regex, match_case, word_separators, capture_matches);
        }

        // there is precisely one match and selection is on top of it
        let next_match = next_match?;

        if !is_recursed && !search_range.contains_range(&next_match.range) {
            return self.get_next_match(next_match.range.get_end_position(), capture_matches, force_move, true);
        }

        Some(next_match)
    }

    pub fn move_to_next_match(&self) {
        let selection = self.editor().get_selection().expect("o editor com modelo tem seleção");
        self.move_to_next_match_from(selection.get_end_position());
    }

    pub fn move_to_match(&self, index: usize) {
        if let Some(decoration_range) = self.decorations.get_decoration_range_at(index) {
            self.set_current_find_match(decoration_range);
        }
    }

    fn get_replace_pattern(&self) -> ReplacePattern {
        if self.state.is_regex() {
            return parse_replace_string(self.state.replace_string().units());
        }
        ReplacePattern::from_static_value(self.state.replace_string())
    }

    pub fn replace(&self) {
        if !self.has_matches() {
            return;
        }

        let replace_pattern = self.get_replace_pattern();
        let editor = self.editor();
        let selection = editor.get_selection().expect("o editor com modelo tem seleção");
        if let Some(next_match) = self.get_next_match(selection.get_start_position(), true, false, false) {
            if *selection == next_match.range {
                // selection sits on a find match => replace it!
                let replace_string = replace_pattern.build_replace_string(next_match.matches.as_deref(), self.state.preserve_case());

                let command = ReplaceCommand::new(*selection, replace_string.clone(), None);

                self.execute_editor_command("replace", Box::new(command));

                self.decorations.set_start_position(Position::new(
                    selection.start_line_number,
                    selection.start_column + replace_string.len() as i32,
                ));
                self.research(true, None);
            } else {
                self.decorations.set_start_position(editor.get_position().expect("o editor com modelo tem posição"));
                self.set_current_find_match(next_match.range);
            }
        }
    }

    fn find_matches(&self, find_scopes: Option<&[Range]>, capture_matches: bool, limit_result_count: usize) -> Vec<FindMatch> {
        let editor = self.editor();
        let model = self.model();
        let search_ranges: Vec<Range> = match find_scopes {
            Some(scopes) => scopes.iter().map(|scope| Self::get_search_range(&model, Some(*scope))).collect(),
            None => vec![Self::get_search_range(&model, None)],
        };

        model.find_matches(
            self.state.search_string(),
            FindScope::Ranges(search_ranges),
            self.state.is_regex(),
            self.state.match_case(),
            self.word_separators(&editor),
            capture_matches,
            limit_result_count,
        )
    }

    pub fn replace_all(&self) {
        if !self.has_matches() {
            return;
        }

        let find_scopes = self.decorations.get_find_scopes();

        if find_scopes.is_none() && self.state.matches_count() >= MATCHES_LIMIT {
            // Doing a replace on the entire file that is over ${MATCHES_LIMIT} matches
            self.large_replace_all();
        } else {
            self.regular_replace_all(find_scopes.as_deref());
        }

        self.research(false, None);
    }

    fn large_replace_all(&self) {
        let editor = self.editor();
        let model = self.model();
        let search_params =
            SearchParams::new(self.state.search_string(), self.state.is_regex(), self.state.match_case(), self.word_separators(&editor));
        let Some(search_data) = search_params.parse_search_request() else {
            return;
        };

        let flags = search_data.regex.flags();
        let modifiers = if !flags.multiline {
            let mut modifiers = String::from("mu");
            if flags.ignore_case {
                modifiers.push('i');
            }
            if flags.global {
                modifiers.push('g');
            }
            modifiers
        } else {
            flags.to_flags_string()
        };
        let modifiers: Vec<u16> = modifiers.encode_utf16().collect();
        let search_regex =
            zyarr::RegExp::new(search_data.regex.source(), &modifiers).expect("a regex de uma busca já aceita continua válida");

        let model_text = model.get_value_with(EndOfLinePreference::Lf, false);
        let full_model_range = model.get_full_model_range();

        let replace_pattern = self.get_replace_pattern();
        let preserve_case = self.state.preserve_case();

        let result_text = if replace_pattern.has_replacement_patterns() || preserve_case {
            // O texto inteiro (o argumento `string` da função de troca) só é lido por um `$n` que passa dos grupos e do
            // `offset`: copiá-lo a cada casamento custaria o tamanho do arquivo vezes o número de casamentos.
            let max_referenced_index = max_referenced_group_index(self.state.replace_string().units());
            replace_all(&search_regex, model_text.units(), |captures| {
                // `arguments`: (match, ...groups, offset, string[, groups])
                let mut arguments: Vec<Option<JsString>> =
                    (0..captures.matched.captures.len()).map(|n| captures.group(n).map(JsString::from)).collect();
                arguments.push(Some(JsString::from(captures.index().to_string())));
                if max_referenced_index >= arguments.len() {
                    arguments.push(Some(model_text.clone()));
                    if !captures.matched.groups.is_empty() {
                        arguments.push(Some(JsString::from("[object Object]")));
                    }
                }
                replace_pattern.build_replace_string(Some(&arguments), preserve_case)
            })
        } else {
            let replacement = replace_pattern.build_replace_string(None, preserve_case);
            replace_all(&search_regex, model_text.units(), |captures| get_substitution(captures, replacement.units()))
        };

        let command = ReplaceCommandThatPreservesSelection::new(
            full_model_range,
            result_text,
            editor.get_selection().expect("o editor com modelo tem seleção"),
            None,
        );
        self.execute_editor_command("replaceAll", Box::new(command));
    }

    fn regular_replace_all(&self, find_scopes: Option<&[Range]>) {
        let replace_pattern = self.get_replace_pattern();
        // Get all the ranges (even more than the highlighted ones)
        let matches = self.find_matches(find_scopes, replace_pattern.has_replacement_patterns() || self.state.preserve_case(), MAX_RESULT_COUNT);

        let replace_strings: Vec<JsString> = matches
            .iter()
            .map(|find_match| replace_pattern.build_replace_string(find_match.matches.as_deref(), self.state.preserve_case()))
            .collect();

        let command = ReplaceAllCommand::new(
            self.editor().get_selection().expect("o editor com modelo tem seleção"),
            matches.iter().map(|find_match| find_match.range).collect(),
            replace_strings,
        );
        self.execute_editor_command("replaceAll", Box::new(command));
    }

    pub fn select_all_matches(&self) {
        if !self.has_matches() {
            return;
        }

        let find_scopes = self.decorations.get_find_scopes();

        // Get all the ranges (even more than the highlighted ones)
        let matches = self.find_matches(find_scopes.as_deref(), false, MAX_RESULT_COUNT);
        let mut selections: Vec<Selection> = matches
            .iter()
            .map(|find_match| {
                let range = find_match.range;
                Selection::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column)
            })
            .collect();

        // If one of the ranges is the editor selection, then maintain it as primary
        let editor = self.editor();
        let editor_selection = editor.get_selection().expect("o editor com modelo tem seleção");
        if let Some(index) = selections.iter().position(|selection| **selection == *editor_selection) {
            selections.remove(index);
            selections.insert(0, editor_selection);
        }

        editor.set_selections(&selections, None, None);
    }

    fn execute_editor_command(&self, source: &str, command: Box<dyn Command>) {
        let _guard = IgnoreGuard::new(&self.ignore_model_content_changed);
        let editor = self.editor();
        editor.push_undo_stop();
        editor.execute_command(Some(source), command);
        editor.push_undo_stop();
    }
}

impl IDisposable for FindModelBoundToEditorModel {
    fn dispose(&self) {
        self.is_disposed.set(true);
        self.start_searching_timer.dispose();
        self.to_dispose.dispose();
    }
}
