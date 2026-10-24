//! O `FindReplaceState` (`contrib/find/browser/findState.ts`) reduzido ao que o multicursor lê e escreve: os campos,
//! os `Override`, o `change` com o cálculo do evento e o `onFindReplaceStateChange`, linha a linha. Sem o que o
//! multicursor não toca (`changeMatchInfo`, `matchesPosition`, `matchesCount`, `currentMatch`, `canNavigate*`).
//! Serve ao editor de produção (o `Ctrl+D` do `je-editor-view`) e aos testes do multicursor.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use crate::multicursor::{FindOptionOverride, FindReplaceStateChangedEvent, IFindReplaceState, INewFindReplaceState};
use crate::range::Range;

/// `effectiveOptionValue(override, value)`.
fn effective_option_value(option_override: FindOptionOverride, value: bool) -> bool {
    match option_override {
        FindOptionOverride::True => true,
        FindOptionOverride::False => false,
        FindOptionOverride::NotSet => value,
    }
}

/// Os campos privados do `FindReplaceState`.
struct StateData {
    search_string: JsString,
    replace_string: JsString,
    is_revealed: bool,
    is_replace_revealed: bool,
    is_regex: bool,
    is_regex_override: FindOptionOverride,
    whole_word: bool,
    whole_word_override: FindOptionOverride,
    match_case: bool,
    match_case_override: FindOptionOverride,
    preserve_case: bool,
    preserve_case_override: FindOptionOverride,
    search_scope: Option<Vec<Range>>,
    loop_: bool,
    is_searching: bool,
}

impl StateData {
    fn is_regex(&self) -> bool {
        effective_option_value(self.is_regex_override, self.is_regex)
    }

    fn whole_word(&self) -> bool {
        effective_option_value(self.whole_word_override, self.whole_word)
    }

    fn match_case(&self) -> bool {
        effective_option_value(self.match_case_override, self.match_case)
    }

    fn preserve_case(&self) -> bool {
        effective_option_value(self.preserve_case_override, self.preserve_case)
    }
}

/// `FindReplaceState`.
pub struct FindReplaceState {
    base: Rc<Disposable>,
    emitter: Rc<Emitter<FindReplaceStateChangedEvent>>,
    data: RefCell<StateData>,
}

impl FindReplaceState {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let emitter = Emitter::new();
        base.register(emitter.clone());
        Rc::new(Self {
            base,
            emitter,
            data: RefCell::new(StateData {
                search_string: JsString::default(),
                replace_string: JsString::default(),
                is_revealed: false,
                is_replace_revealed: false,
                is_regex: false,
                is_regex_override: FindOptionOverride::NotSet,
                whole_word: false,
                whole_word_override: FindOptionOverride::NotSet,
                match_case: false,
                match_case_override: FindOptionOverride::NotSet,
                preserve_case: false,
                preserve_case_override: FindOptionOverride::NotSet,
                search_scope: None,
                loop_: true,
                is_searching: false,
            }),
        })
    }

    /// `actualIsRegex`.
    pub fn actual_is_regex(&self) -> bool {
        self.data.borrow().is_regex
    }

    /// `actualWholeWord`.
    pub fn actual_whole_word(&self) -> bool {
        self.data.borrow().whole_word
    }

    /// `actualMatchCase`.
    pub fn actual_match_case(&self) -> bool {
        self.data.borrow().match_case
    }

    /// `actualPreserveCase`.
    pub fn actual_preserve_case(&self) -> bool {
        self.data.borrow().preserve_case
    }
}

impl IFindReplaceState for FindReplaceState {
    fn search_string(&self) -> JsString {
        self.data.borrow().search_string.clone()
    }

    fn is_revealed(&self) -> bool {
        self.data.borrow().is_revealed
    }

    fn is_regex(&self) -> bool {
        self.data.borrow().is_regex()
    }

    fn whole_word(&self) -> bool {
        self.data.borrow().whole_word()
    }

    fn match_case(&self) -> bool {
        self.data.borrow().match_case()
    }

    fn search_scope(&self) -> Option<Vec<Range>> {
        self.data.borrow().search_scope.clone()
    }

    fn change(&self, new_state: &INewFindReplaceState, move_cursor: bool) {
        let mut change_event = FindReplaceStateChangedEvent { move_cursor, update_history: true, ..Default::default() };
        let mut something_changed = false;

        {
            let mut data = self.data.borrow_mut();
            let old_effective_is_regex = data.is_regex();
            let old_effective_whole_words = data.whole_word();
            let old_effective_match_case = data.match_case();
            let old_effective_preserve_case = data.preserve_case();

            if let Some(search_string) = &new_state.search_string
                && data.search_string != *search_string
            {
                data.search_string = search_string.clone();
                change_event.search_string = true;
                something_changed = true;
            }
            if let Some(replace_string) = &new_state.replace_string
                && data.replace_string != *replace_string
            {
                data.replace_string = replace_string.clone();
                change_event.replace_string = true;
                something_changed = true;
            }
            if let Some(is_revealed) = new_state.is_revealed
                && data.is_revealed != is_revealed
            {
                data.is_revealed = is_revealed;
                change_event.is_revealed = true;
                something_changed = true;
            }
            if let Some(is_replace_revealed) = new_state.is_replace_revealed
                && data.is_replace_revealed != is_replace_revealed
            {
                data.is_replace_revealed = is_replace_revealed;
                change_event.is_replace_revealed = true;
                something_changed = true;
            }
            if let Some(is_regex) = new_state.is_regex {
                data.is_regex = is_regex;
            }
            if let Some(whole_word) = new_state.whole_word {
                data.whole_word = whole_word;
            }
            if let Some(match_case) = new_state.match_case {
                data.match_case = match_case;
            }
            if let Some(preserve_case) = new_state.preserve_case {
                data.preserve_case = preserve_case;
            }
            if let Some(new_search_scope) = &new_state.search_scope {
                // `!newState.searchScope?.every(newScope => this._searchScope?.some(existing => !equals(existing, newScope)))`:
                // um escopo `null` dá `undefined`, e `!undefined` é verdadeiro.
                let every = new_search_scope.as_ref().map(|new_scopes| {
                    new_scopes.iter().all(|new_scope| {
                        data.search_scope.as_ref().is_some_and(|existing| existing.iter().any(|scope| scope != new_scope))
                    })
                });
                if !every.unwrap_or(false) {
                    data.search_scope = new_search_scope.clone();
                    change_event.search_scope = true;
                    something_changed = true;
                }
            }
            if let Some(loop_) = new_state.r#loop
                && data.loop_ != loop_
            {
                data.loop_ = loop_;
                change_event.r#loop = true;
                something_changed = true;
            }
            if let Some(is_searching) = new_state.is_searching
                && data.is_searching != is_searching
            {
                data.is_searching = is_searching;
                change_event.is_searching = true;
                something_changed = true;
            }

            // Overrides get set when they explicitly come in and get reset anytime something else changes
            data.is_regex_override = new_state.is_regex_override.unwrap_or(FindOptionOverride::NotSet);
            data.whole_word_override = new_state.whole_word_override.unwrap_or(FindOptionOverride::NotSet);
            data.match_case_override = new_state.match_case_override.unwrap_or(FindOptionOverride::NotSet);
            data.preserve_case_override = new_state.preserve_case_override.unwrap_or(FindOptionOverride::NotSet);

            if old_effective_is_regex != data.is_regex() {
                something_changed = true;
                change_event.is_regex = true;
            }
            if old_effective_whole_words != data.whole_word() {
                something_changed = true;
                change_event.whole_word = true;
            }
            if old_effective_match_case != data.match_case() {
                something_changed = true;
                change_event.match_case = true;
            }
            if old_effective_preserve_case != data.preserve_case() {
                something_changed = true;
                change_event.preserve_case = true;
            }
        }

        if something_changed {
            self.emitter.fire(change_event);
        }
    }

    fn on_find_replace_state_change(&self) -> Event<FindReplaceStateChangedEvent> {
        self.emitter.event()
    }
}

impl IDisposable for FindReplaceState {
    fn dispose(&self) {
        self.base.dispose();
    }
}
