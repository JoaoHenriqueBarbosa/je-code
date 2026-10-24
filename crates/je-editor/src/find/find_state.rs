//! Porte de `src/vs/editor/contrib/find/browser/findState.ts`: o estado da busca e da substituição
//! (`FindReplaceState`), o evento de mudança (`FindReplaceStateChangedEvent`), o pedido de mudança
//! (`INewFindReplaceState`) e as sobreposições de opção (`FindOptionOverride`).
//!
//! Representação:
//! - `FindReplaceState<T> extends Disposable` é a struct [`FindReplaceState`], com um [`Disposable`] (o `_register`) e o
//!   estado num `RefCell`. Os métodos trabalham por `&self` e soltam o empréstimo antes de disparar o evento, porque
//!   os ouvintes (o modelo de busca, o controlador) leem o estado e até o mudam de dentro do evento.
//! - O parâmetro de tipo `T extends { update: (value: T) => void }` dos filtros é o trait [`FindReplaceFilters`]; o
//!   padrão `{ update: () => {} }` é [`NoFilters`].
//! - `searchString` e `replaceString` são [`JsString`] (UTF-16, como a busca do modelo), e `matchesPosition` e
//!   `matchesCount` são `usize`.
//! - `searchScope?: Range[] | null` do pedido é `Option<Option<Vec<Range>>>` (`None` é `undefined`, `Some(None)` é
//!   `null`). Uma `Selection` chega como o seu [`Range`], porque `Selection extends Range`.
//! - `changeMatchInfo(..., currentMatch: Range | undefined)` recebe `Option<Range>`, e `updateHistory: boolean = true`
//!   de `change` é `Option<bool>` (`None` vale `true`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use super::find_model::MATCHES_LIMIT;
use crate::range::Range;

/// `FindReplaceStateChangedEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindReplaceStateChangedEvent {
    pub move_cursor: bool,
    pub update_history: bool,

    pub search_string: bool,
    pub replace_string: bool,
    pub is_revealed: bool,
    pub is_replace_revealed: bool,
    pub is_regex: bool,
    pub whole_word: bool,
    pub match_case: bool,
    pub preserve_case: bool,
    pub search_scope: bool,
    pub matches_position: bool,
    pub matches_count: bool,
    pub current_match: bool,
    pub r#loop: bool,
    pub is_searching: bool,
    pub filters: bool,
}

/// `FindOptionOverride`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FindOptionOverride {
    #[default]
    NotSet = 0,
    True = 1,
    False = 2,
}

/// O `{ update: (value: T) => void }` que os filtros do estado cumprem.
pub trait FindReplaceFilters: Clone + 'static {
    /// `update(value)`: absorve os valores do filtro novo.
    fn update(&mut self, value: &Self);
}

/// O padrão `{ update: () => {} }` do parâmetro de tipo dos filtros: nenhum filtro.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoFilters;

impl FindReplaceFilters for NoFilters {
    fn update(&mut self, _value: &NoFilters) {}
}

/// `INewFindReplaceState<T>`: cada campo ausente (`undefined`) deixa o valor atual como está.
#[derive(Clone, Debug)]
pub struct INewFindReplaceState<T: FindReplaceFilters = NoFilters> {
    pub search_string: Option<JsString>,
    pub replace_string: Option<JsString>,
    pub is_revealed: Option<bool>,
    pub is_replace_revealed: Option<bool>,
    pub is_regex: Option<bool>,
    pub is_regex_override: Option<FindOptionOverride>,
    pub whole_word: Option<bool>,
    pub whole_word_override: Option<FindOptionOverride>,
    pub match_case: Option<bool>,
    pub match_case_override: Option<FindOptionOverride>,
    pub preserve_case: Option<bool>,
    pub preserve_case_override: Option<FindOptionOverride>,
    /// `Range[] | null | undefined`.
    pub search_scope: Option<Option<Vec<Range>>>,
    pub r#loop: Option<bool>,
    pub is_searching: Option<bool>,
    pub filters: Option<T>,
}

impl<T: FindReplaceFilters> Default for INewFindReplaceState<T> {
    fn default() -> Self {
        Self {
            search_string: None,
            replace_string: None,
            is_revealed: None,
            is_replace_revealed: None,
            is_regex: None,
            is_regex_override: None,
            whole_word: None,
            whole_word_override: None,
            match_case: None,
            match_case_override: None,
            preserve_case: None,
            preserve_case_override: None,
            search_scope: None,
            r#loop: None,
            is_searching: None,
            filters: None,
        }
    }
}

/// `effectiveOptionValue(override, value)`.
fn effective_option_value(option_override: FindOptionOverride, value: bool) -> bool {
    match option_override {
        FindOptionOverride::True => true,
        FindOptionOverride::False => false,
        FindOptionOverride::NotSet => value,
    }
}

/// O estado de um [`FindReplaceState`].
struct StateInner<T: FindReplaceFilters> {
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
    matches_position: usize,
    matches_count: usize,
    current_match: Option<Range>,
    r#loop: bool,
    is_searching: bool,
    filters: Option<T>,
}

impl<T: FindReplaceFilters> StateInner<T> {
    fn effective_is_regex(&self) -> bool {
        effective_option_value(self.is_regex_override, self.is_regex)
    }

    fn effective_whole_word(&self) -> bool {
        effective_option_value(self.whole_word_override, self.whole_word)
    }

    fn effective_match_case(&self) -> bool {
        effective_option_value(self.match_case_override, self.match_case)
    }

    fn effective_preserve_case(&self) -> bool {
        effective_option_value(self.preserve_case_override, self.preserve_case)
    }
}

/// `FindReplaceState`.
pub struct FindReplaceState<T: FindReplaceFilters = NoFilters> {
    base: Rc<Disposable>,
    inner: RefCell<StateInner<T>>,
    on_find_replace_state_change: Rc<Emitter<FindReplaceStateChangedEvent>>,
}

impl<T: FindReplaceFilters> FindReplaceState<T> {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_find_replace_state_change = Emitter::new();
        base.register(on_find_replace_state_change.clone());
        Rc::new(Self {
            base,
            inner: RefCell::new(StateInner {
                search_string: JsString::new(),
                replace_string: JsString::new(),
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
                matches_position: 0,
                matches_count: 0,
                current_match: None,
                r#loop: true,
                is_searching: false,
                filters: None,
            }),
            on_find_replace_state_change,
        })
    }

    pub fn search_string(&self) -> JsString {
        self.inner.borrow().search_string.clone()
    }

    pub fn replace_string(&self) -> JsString {
        self.inner.borrow().replace_string.clone()
    }

    pub fn is_revealed(&self) -> bool {
        self.inner.borrow().is_revealed
    }

    pub fn is_replace_revealed(&self) -> bool {
        self.inner.borrow().is_replace_revealed
    }

    pub fn is_regex(&self) -> bool {
        self.inner.borrow().effective_is_regex()
    }

    pub fn whole_word(&self) -> bool {
        self.inner.borrow().effective_whole_word()
    }

    pub fn match_case(&self) -> bool {
        self.inner.borrow().effective_match_case()
    }

    pub fn preserve_case(&self) -> bool {
        self.inner.borrow().effective_preserve_case()
    }

    pub fn actual_is_regex(&self) -> bool {
        self.inner.borrow().is_regex
    }

    pub fn actual_whole_word(&self) -> bool {
        self.inner.borrow().whole_word
    }

    pub fn actual_match_case(&self) -> bool {
        self.inner.borrow().match_case
    }

    pub fn actual_preserve_case(&self) -> bool {
        self.inner.borrow().preserve_case
    }

    pub fn search_scope(&self) -> Option<Vec<Range>> {
        self.inner.borrow().search_scope.clone()
    }

    pub fn matches_position(&self) -> usize {
        self.inner.borrow().matches_position
    }

    pub fn matches_count(&self) -> usize {
        self.inner.borrow().matches_count
    }

    pub fn current_match(&self) -> Option<Range> {
        self.inner.borrow().current_match
    }

    pub fn is_searching(&self) -> bool {
        self.inner.borrow().is_searching
    }

    pub fn filters(&self) -> Option<T> {
        self.inner.borrow().filters.clone()
    }

    /// `onFindReplaceStateChange`.
    pub fn on_find_replace_state_change(&self) -> Event<FindReplaceStateChangedEvent> {
        self.on_find_replace_state_change.event()
    }

    /// `changeMatchInfo(matchesPosition, matchesCount, currentMatch)`.
    pub fn change_match_info(&self, matches_position: usize, matches_count: usize, current_match: Option<Range>) {
        let mut change_event = FindReplaceStateChangedEvent::default();
        let mut something_changed = false;

        {
            let mut inner = self.inner.borrow_mut();
            let mut matches_position = matches_position;
            if matches_count == 0 {
                matches_position = 0;
            }
            if matches_position > matches_count {
                matches_position = matches_count;
            }

            if inner.matches_position != matches_position {
                inner.matches_position = matches_position;
                change_event.matches_position = true;
                something_changed = true;
            }
            if inner.matches_count != matches_count {
                inner.matches_count = matches_count;
                change_event.matches_count = true;
                something_changed = true;
            }

            if let Some(current_match) = current_match
                && inner.current_match != Some(current_match)
            {
                inner.current_match = Some(current_match);
                change_event.current_match = true;
                something_changed = true;
            }
        }

        if something_changed {
            self.on_find_replace_state_change.fire(change_event);
        }
    }

    /// `change(newState, moveCursor, updateHistory = true)`.
    pub fn change(&self, new_state: INewFindReplaceState<T>, move_cursor: bool, update_history: Option<bool>) {
        let mut change_event = FindReplaceStateChangedEvent {
            move_cursor,
            update_history: update_history.unwrap_or(true),
            ..FindReplaceStateChangedEvent::default()
        };
        let mut something_changed = false;

        {
            let mut inner = self.inner.borrow_mut();
            let old_effective_is_regex = inner.effective_is_regex();
            let old_effective_whole_words = inner.effective_whole_word();
            let old_effective_match_case = inner.effective_match_case();
            let old_effective_preserve_case = inner.effective_preserve_case();

            if let Some(search_string) = new_state.search_string
                && inner.search_string != search_string
            {
                inner.search_string = search_string;
                change_event.search_string = true;
                something_changed = true;
            }
            if let Some(replace_string) = new_state.replace_string
                && inner.replace_string != replace_string
            {
                inner.replace_string = replace_string;
                change_event.replace_string = true;
                something_changed = true;
            }
            if let Some(is_revealed) = new_state.is_revealed
                && inner.is_revealed != is_revealed
            {
                inner.is_revealed = is_revealed;
                change_event.is_revealed = true;
                something_changed = true;
            }
            if let Some(is_replace_revealed) = new_state.is_replace_revealed
                && inner.is_replace_revealed != is_replace_revealed
            {
                inner.is_replace_revealed = is_replace_revealed;
                change_event.is_replace_revealed = true;
                something_changed = true;
            }
            if let Some(is_regex) = new_state.is_regex {
                inner.is_regex = is_regex;
            }
            if let Some(whole_word) = new_state.whole_word {
                inner.whole_word = whole_word;
            }
            if let Some(match_case) = new_state.match_case {
                inner.match_case = match_case;
            }
            if let Some(preserve_case) = new_state.preserve_case {
                inner.preserve_case = preserve_case;
            }
            if let Some(new_search_scope) = new_state.search_scope {
                // `!newScope?.every(n => this._searchScope?.some(e => !equalsRange(e, n)))`
                let keeps_current = match &new_search_scope {
                    None => false,
                    Some(new_scopes) => new_scopes.iter().all(|new_scope| {
                        inner.search_scope.as_ref().is_some_and(|existing| existing.iter().any(|scope| scope != new_scope))
                    }),
                };
                if !keeps_current {
                    inner.search_scope = new_search_scope;
                    change_event.search_scope = true;
                    something_changed = true;
                }
            }
            if let Some(r#loop) = new_state.r#loop
                && inner.r#loop != r#loop
            {
                inner.r#loop = r#loop;
                change_event.r#loop = true;
                something_changed = true;
            }

            if let Some(is_searching) = new_state.is_searching
                && inner.is_searching != is_searching
            {
                inner.is_searching = is_searching;
                change_event.is_searching = true;
                something_changed = true;
            }

            if let Some(filters) = new_state.filters {
                match inner.filters.as_mut() {
                    Some(current) => current.update(&filters),
                    None => inner.filters = Some(filters),
                }

                change_event.filters = true;
                something_changed = true;
            }

            // Overrides get set when they explicitly come in and get reset anytime something else changes
            inner.is_regex_override = new_state.is_regex_override.unwrap_or_default();
            inner.whole_word_override = new_state.whole_word_override.unwrap_or_default();
            inner.match_case_override = new_state.match_case_override.unwrap_or_default();
            inner.preserve_case_override = new_state.preserve_case_override.unwrap_or_default();

            if old_effective_is_regex != inner.effective_is_regex() {
                something_changed = true;
                change_event.is_regex = true;
            }
            if old_effective_whole_words != inner.effective_whole_word() {
                something_changed = true;
                change_event.whole_word = true;
            }
            if old_effective_match_case != inner.effective_match_case() {
                something_changed = true;
                change_event.match_case = true;
            }

            if old_effective_preserve_case != inner.effective_preserve_case() {
                something_changed = true;
                change_event.preserve_case = true;
            }
        }

        if something_changed {
            self.on_find_replace_state_change.fire(change_event);
        }
    }

    pub fn can_navigate_back(&self) -> bool {
        self.can_navigate_in_loop() || self.matches_position() != 1
    }

    pub fn can_navigate_forward(&self) -> bool {
        self.can_navigate_in_loop() || self.matches_position() < self.matches_count()
    }

    fn can_navigate_in_loop(&self) -> bool {
        let inner = self.inner.borrow();
        inner.r#loop || inner.matches_count >= MATCHES_LIMIT
    }
}

impl<T: FindReplaceFilters> IDisposable for FindReplaceState<T> {
    fn dispose(&self) {
        self.base.dispose();
    }
}
