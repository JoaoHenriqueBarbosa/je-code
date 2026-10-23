//! Porte de `src/vs/editor/common/model/tokens/tokenizerSyntaxTokenBackend.ts` (e do contrato de
//! `abstractSyntaxTokenBackend.ts`): a tokenização do modelo pelo `ITokenizationSupport` registrado para a
//! linguagem, com os tokens em um `ContiguousTokensStore`.
//!
//! O backend por tree-sitter (`TreeSitterSyntaxTokenBackend`) não existe no porte, então não há classe
//! abstrata: `TokenizerSyntaxTokenBackend` é o único backend e o `TokenizationTextModelPart` o usa direto.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::errors;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableMap, IDisposable, MutableDisposable};

use crate::attached_views::{AttachedView, AttachedViewHandler, AttachedViewState, AttachedViews};
use crate::contiguous_multiline_tokens::{ContiguousMultilineTokens, ContiguousMultilineTokensBuilder};
use crate::contiguous_tokens_store::{ChangedLines, ContiguousTokensStore};
use crate::eol_counter::count_eol;
use crate::languages::{
    FontTokensUpdate, IBackgroundTokenizationStore, IBackgroundTokenizer, ILanguageIdCodec, ITokenizationSupport, State,
};
use crate::line_range::LineRange;
use crate::line_tokens::LineTokens;
use crate::position::Position;
use crate::text_model::TextModel;
use crate::text_model_events::{
    ModelContentChangedEvent, ModelFontTokensChangedEvent, ModelTokensChangedEvent, ModelTokensChangedRange,
};
use crate::text_model_tokens::{DefaultBackgroundTokenizer, TokenizerWithStateStoreAndTextModel, TrackingTokenizationStateStore};
use crate::tokenization_registry::tokenization_registry;
use crate::encoded_token_attributes::standard_token_type;

/// `BackgroundTokenizationState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundTokenizationState {
    InProgress = 1,
    Completed = 2,
}

/// `TokenizerSyntaxTokenBackend`.
pub struct TokenizerSyntaxTokenBackend {
    me: Weak<TokenizerSyntaxTokenBackend>,
    base: Rc<Disposable>,
    language_id_codec: Rc<dyn ILanguageIdCodec>,
    text_model: Weak<TextModel>,
    get_language_id: Rc<dyn Fn() -> String>,

    tokenizer: RefCell<Option<Rc<TokenizerWithStateStoreAndTextModel>>>,
    background_tokenization_state: Cell<BackgroundTokenizationState>,
    on_did_change_background_tokenization_state: Rc<Emitter<()>>,
    on_did_change_tokens: Rc<Emitter<ModelTokensChangedEvent>>,
    on_did_change_font_tokens: Rc<Emitter<ModelFontTokensChangedEvent>>,

    default_background_tokenizer: RefCell<Option<Rc<DefaultBackgroundTokenizer>>>,
    background_tokenizer: Rc<MutableDisposable<dyn IBackgroundTokenizer>>,

    tokens: RefCell<ContiguousTokensStore>,
    debug_background_tokens: RefCell<Option<ContiguousTokensStore>>,
    debug_background_states: RefCell<Option<TrackingTokenizationStateStore>>,
    debug_background_tokenizer: Rc<MutableDisposable<dyn IBackgroundTokenizer>>,

    attached_view_states: Rc<DisposableMap<u64, AttachedViewHandler>>,
}

/// O `IBackgroundTokenizationStore` que o backend entrega ao `createBackgroundTokenizer`.
struct BackendStore {
    backend: Weak<TokenizerSyntaxTokenBackend>,
}

impl IBackgroundTokenizationStore for BackendStore {
    fn set_tokens(&self, tokens: Vec<ContiguousMultilineTokens>) {
        if let Some(backend) = self.backend.upgrade() {
            backend.set_tokens(tokens);
        }
    }

    fn set_font_info(&self, changes: FontTokensUpdate) {
        if let Some(backend) = self.backend.upgrade() {
            backend.set_font_info(changes);
        }
    }

    fn background_tokenization_finished(&self) {
        let Some(backend) = self.backend.upgrade() else { return };
        if backend.background_tokenization_state.get() == BackgroundTokenizationState::Completed {
            // We already did a full tokenization and don't go back to progressing.
            return;
        }
        backend.background_tokenization_state.set(BackgroundTokenizationState::Completed);
        backend.on_did_change_background_tokenization_state.fire(());
    }

    fn set_end_state(&self, line_number: i32, state: State) {
        let Some(backend) = self.backend.upgrade() else { return };
        let Some(tokenizer) = backend.tokenizer.borrow().clone() else { return };
        let first_invalid_end_state_line_number = tokenizer.store().borrow().get_first_invalid_end_state_line_number();
        // Don't accept states for definitely valid states, the renderer is ahead of the worker!
        if let Some(first) = first_invalid_end_state_line_number
            && line_number >= first
        {
            tokenizer.store().borrow_mut().set_end_state(line_number, state);
        }
    }
}

/// O store do tokenizador de depuração (`backgroundTokenizerShouldOnlyVerifyTokens`).
struct DebugStore {
    backend: Weak<TokenizerSyntaxTokenBackend>,
}

impl IBackgroundTokenizationStore for DebugStore {
    fn set_tokens(&self, tokens: Vec<ContiguousMultilineTokens>) {
        let Some(backend) = self.backend.upgrade() else { return };
        let Some(model) = backend.text_model.upgrade() else { return };
        if let Some(store) = backend.debug_background_tokens.borrow_mut().as_mut() {
            store.set_multiline_tokens(tokens, &model);
        }
    }

    fn set_font_info(&self, changes: FontTokensUpdate) {
        if let Some(backend) = self.backend.upgrade() {
            backend.set_font_info(changes);
        }
    }

    fn background_tokenization_finished(&self) {}

    fn set_end_state(&self, line_number: i32, state: State) {
        let Some(backend) = self.backend.upgrade() else { return };
        if let Some(states) = backend.debug_background_states.borrow_mut().as_mut() {
            states.set_end_state(line_number, state);
        }
    }
}

impl TokenizerSyntaxTokenBackend {
    pub fn new(
        language_id_codec: Rc<dyn ILanguageIdCodec>,
        text_model: Weak<TextModel>,
        get_language_id: Rc<dyn Fn() -> String>,
        attached_views: &Rc<AttachedViews>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_background_tokenization_state = Emitter::new();
        let on_did_change_tokens = Emitter::new();
        let on_did_change_font_tokens = Emitter::new();
        base.register(on_did_change_background_tokenization_state.clone());
        base.register(on_did_change_tokens.clone());
        base.register(on_did_change_font_tokens.clone());
        let background_tokenizer = MutableDisposable::<dyn IBackgroundTokenizer>::new();
        let debug_background_tokenizer = MutableDisposable::<dyn IBackgroundTokenizer>::new();
        let attached_view_states = DisposableMap::<u64, AttachedViewHandler>::new();
        base.register(background_tokenizer.clone());
        base.register(debug_background_tokenizer.clone());
        base.register(attached_view_states.clone());

        let backend = Rc::new_cyclic(|me| TokenizerSyntaxTokenBackend {
            me: me.clone(),
            base,
            tokens: RefCell::new(ContiguousTokensStore::new(language_id_codec.clone())),
            language_id_codec,
            text_model,
            get_language_id,
            tokenizer: RefCell::new(None),
            background_tokenization_state: Cell::new(BackgroundTokenizationState::InProgress),
            on_did_change_background_tokenization_state,
            on_did_change_tokens,
            on_did_change_font_tokens,
            default_background_tokenizer: RefCell::new(None),
            background_tokenizer,
            debug_background_tokens: RefCell::new(None),
            debug_background_states: RefCell::new(None),
            debug_background_tokenizer,
            attached_view_states,
        });

        let weak = backend.me.clone();
        backend.base.register(tokenization_registry().on_did_change().listen(move |e| {
            let Some(backend) = weak.upgrade() else { return };
            let language_id = (backend.get_language_id)();
            if !e.changed_languages.contains(&language_id) {
                return;
            }
            backend.todo_reset_tokenization(true);
        }));

        backend.todo_reset_tokenization(true);

        let weak = backend.me.clone();
        backend.base.register(attached_views.on_did_change_visible_ranges().listen(move |(view, state)| {
            let Some(backend) = weak.upgrade() else { return };
            backend.handle_attached_view_change(view, state.as_ref());
        }));

        backend
    }

    fn handle_attached_view_change(&self, view: &Rc<AttachedView>, state: Option<&AttachedViewState>) {
        match state {
            Some(state) => {
                let existing = match self.attached_view_states.get(&view.id()) {
                    Some(existing) => existing,
                    None => {
                        let weak_backend = self.me.clone();
                        let handler_slot: Rc<RefCell<Option<Weak<AttachedViewHandler>>>> = Rc::new(RefCell::new(None));
                        let slot = handler_slot.clone();
                        let handler = AttachedViewHandler::new(move || {
                            let Some(backend) = weak_backend.upgrade() else { return };
                            let ranges = slot.borrow().as_ref().and_then(Weak::upgrade).map(|h| h.line_ranges()).unwrap_or_default();
                            backend.refresh_ranges(&ranges);
                        });
                        *handler_slot.borrow_mut() = Some(Rc::downgrade(&handler));
                        self.attached_view_states.set(view.id(), handler.clone(), false);
                        handler
                    }
                };
                existing.handle_state_change(state);
            }
            None => self.attached_view_states.delete_and_dispose(&view.id()),
        }
    }

    fn model(&self) -> Rc<TextModel> {
        self.text_model.upgrade().expect("o modelo de texto foi descartado")
    }

    pub fn background_tokenization_state(&self) -> BackgroundTokenizationState {
        self.background_tokenization_state.get()
    }

    pub fn on_did_change_background_tokenization_state(&self) -> Event<()> {
        self.on_did_change_background_tokenization_state.event()
    }

    pub fn on_did_change_tokens(&self) -> Event<ModelTokensChangedEvent> {
        self.on_did_change_tokens.event()
    }

    pub fn on_did_change_font_tokens(&self) -> Event<ModelFontTokensChangedEvent> {
        self.on_did_change_font_tokens.event()
    }

    pub fn todo_reset_tokenization(&self, fire_token_change_event: bool) {
        let model = self.model();
        self.tokens.borrow_mut().flush();
        if let Some(t) = self.debug_background_tokens.borrow_mut().as_mut() {
            t.flush();
        }
        if self.debug_background_states.borrow().is_some() {
            *self.debug_background_states.borrow_mut() = Some(TrackingTokenizationStateStore::new(model.get_line_count()));
        }
        if fire_token_change_event {
            self.on_did_change_tokens.fire(ModelTokensChangedEvent {
                semantic_tokens_applied: false,
                ranges: vec![ModelTokensChangedRange { from_line_number: 1, to_line_number: model.get_line_count() }],
            });
        }

        let initialize_tokenization = || -> Option<Rc<dyn ITokenizationSupport>> {
            if model.is_too_large_for_tokenization() {
                return None;
            }
            let tokenization_support = tokenization_registry().get(&(self.get_language_id)())?;
            // `getInitialState()` pode lançar: o erro vai ao `onUnexpectedError` e não há tokenização.
            match errors::catch(|| tokenization_support.get_initial_state()) {
                Ok(_) => Some(tokenization_support),
                Err(e) => {
                    errors::on_unexpected_error(&e);
                    None
                }
            }
        };

        let tokenization_support = initialize_tokenization();
        *self.tokenizer.borrow_mut() = tokenization_support.as_ref().map(|support| {
            Rc::new(TokenizerWithStateStoreAndTextModel::new(
                model.get_line_count(),
                support.clone(),
                self.text_model.clone(),
                self.language_id_codec.clone(),
            ))
        });

        self.background_tokenizer.clear();

        *self.default_background_tokenizer.borrow_mut() = None;
        let tokenizer = self.tokenizer.borrow().clone();
        if let (Some(tokenizer), Some(support)) = (tokenizer, tokenization_support) {
            let store: Rc<dyn IBackgroundTokenizationStore> = Rc::new(BackendStore { backend: self.me.clone() });

            if !support.background_tokenizer_should_only_verify_tokens()
                && let Some(created) = support.create_background_tokenizer(&model, store.clone())
            {
                self.background_tokenizer.set(Some(created));
            }
            if self.background_tokenizer.value().is_none() && !model.is_too_large_for_tokenization() {
                let default = DefaultBackgroundTokenizer::new(tokenizer.clone(), store);
                self.background_tokenizer.set(Some(default.clone() as Rc<dyn IBackgroundTokenizer>));
                *self.default_background_tokenizer.borrow_mut() = Some(default.clone());
                default.handle_changes();
            }

            if support.background_tokenizer_should_only_verify_tokens() {
                *self.debug_background_tokens.borrow_mut() = Some(ContiguousTokensStore::new(self.language_id_codec.clone()));
                *self.debug_background_states.borrow_mut() = Some(TrackingTokenizationStateStore::new(model.get_line_count()));
                self.debug_background_tokenizer.clear();
                let debug_store: Rc<dyn IBackgroundTokenizationStore> = Rc::new(DebugStore { backend: self.me.clone() });
                self.debug_background_tokenizer.set(support.create_background_tokenizer(&model, debug_store));
            } else {
                *self.debug_background_tokens.borrow_mut() = None;
                *self.debug_background_states.borrow_mut() = None;
                self.debug_background_tokenizer.clear();
            }
        }

        self.refresh_all_visible_line_tokens();
    }

    pub fn handle_did_change_attached(&self) {
        let default = self.default_background_tokenizer.borrow().clone();
        if let Some(d) = default {
            d.handle_changes();
        }
    }

    pub fn handle_did_change_content(&self, e: &ModelContentChangedEvent) {
        if e.is_flush {
            // Don't fire the event, as the view might not have got the text change event yet
            self.todo_reset_tokenization(false);
        } else if !e.is_eol_change {
            // We don't have to do anything on an EOL change
            for c in &e.changes {
                let counted = count_eol(c.text.units());
                self.tokens.borrow_mut().accept_edit(&c.range, counted.eol_count, counted.first_line_length);
                if let Some(t) = self.debug_background_tokens.borrow_mut().as_mut() {
                    t.accept_edit(&c.range, counted.eol_count, counted.first_line_length);
                }
            }
            if let Some(s) = self.debug_background_states.borrow_mut().as_mut() {
                s.accept_changes(&e.changes);
            }

            if let Some(tokenizer) = self.tokenizer.borrow().as_ref() {
                tokenizer.store().borrow_mut().accept_changes(&e.changes);
            }
            let default = self.default_background_tokenizer.borrow().clone();
            if let Some(d) = default {
                d.handle_changes();
            }
        }
    }

    fn set_tokens(&self, tokens: Vec<ContiguousMultilineTokens>) -> Vec<ChangedLines> {
        let model = self.model();
        let changes = self.tokens.borrow_mut().set_multiline_tokens(tokens, &model);

        if !changes.is_empty() {
            self.on_did_change_tokens.fire(ModelTokensChangedEvent {
                semantic_tokens_applied: false,
                ranges: changes
                    .iter()
                    .map(|c| ModelTokensChangedRange { from_line_number: c.from_line_number, to_line_number: c.to_line_number })
                    .collect(),
            });
        }
        changes
    }

    fn set_font_info(&self, changes: FontTokensUpdate) {
        self.on_did_change_font_tokens.fire(ModelFontTokensChangedEvent { changes });
    }

    fn refresh_all_visible_line_tokens(&self) {
        let ranges: Vec<Vec<LineRange>> = self.attached_view_states.values().iter().map(|s| s.line_ranges()).collect();
        let joined = LineRange::join_many(&ranges);
        self.refresh_ranges(&joined);
    }

    fn refresh_ranges(&self, ranges: &[LineRange]) {
        for range in ranges {
            self.refresh_range(range.start_line_number, range.end_line_number_exclusive - 1);
        }
    }

    fn refresh_range(&self, start_line_number: i32, end_line_number: i32) {
        let Some(tokenizer) = self.tokenizer.borrow().clone() else { return };
        let model = self.model();

        let start_line_number = start_line_number.min(model.get_line_count()).max(1);
        let end_line_number = end_line_number.min(model.get_line_count());

        let mut builder = ContiguousMultilineTokensBuilder::new();
        let heuristic_tokens = tokenizer.tokenize_heuristically(&mut builder, start_line_number, end_line_number);
        let changed_tokens = self.set_tokens(builder.finalize());

        if heuristic_tokens {
            // Mark the heuristic tokens as invalid, so that the background tokenizer overwrites them.
            for c in &changed_tokens {
                if let Some(bg) = self.background_tokenizer.value() {
                    bg.request_tokens(c.from_line_number, c.to_line_number + 1);
                }
            }
        }

        let default = self.default_background_tokenizer.borrow().clone();
        if let Some(d) = default {
            d.check_finished();
        }
    }

    pub fn force_tokenization(&self, line_number: i32) {
        let mut builder = ContiguousMultilineTokensBuilder::new();
        let tokenizer = self.tokenizer.borrow().clone();
        if let Some(t) = tokenizer {
            t.update_tokens_until_line(&mut builder, line_number);
        }
        self.set_tokens(builder.finalize());
        let default = self.default_background_tokenizer.borrow().clone();
        if let Some(d) = default {
            d.check_finished();
        }
    }

    pub fn has_accurate_tokens_for_line(&self, line_number: i32) -> bool {
        match self.tokenizer.borrow().as_ref() {
            None => true,
            Some(t) => t.has_accurate_tokens_for_line(line_number),
        }
    }

    pub fn is_cheap_to_tokenize(&self, line_number: i32) -> bool {
        match self.tokenizer.borrow().as_ref() {
            None => true,
            Some(t) => t.is_cheap_to_tokenize(line_number),
        }
    }

    /// `tokenizeIfCheap(lineNumber)`.
    pub fn tokenize_if_cheap(&self, line_number: i32) {
        if self.is_cheap_to_tokenize(line_number) {
            self.force_tokenization(line_number);
        }
    }

    pub fn get_line_tokens(&self, line_number: i32) -> LineTokens {
        let model = self.model();
        let line_text = model.get_line_content(line_number);
        let language_id = model.get_language_id();
        let result = self.tokens.borrow().get_tokens(&language_id, (line_number - 1) as usize, line_text.clone());
        let tokenizer = self.tokenizer.borrow().clone();
        if let (Some(debug_tokens), Some(debug_states), Some(tokenizer)) =
            (&*self.debug_background_tokens.borrow(), &*self.debug_background_states.borrow(), tokenizer)
            && debug_states.get_first_invalid_end_state_line_number_or_max() > line_number
            && tokenizer.store().borrow().get_first_invalid_end_state_line_number_or_max() > line_number
        {
            let background_result = debug_tokens.get_tokens(&language_id, (line_number - 1) as usize, line_text);
            if !result.equals_line_tokens(&background_result)
                && let Some(bg) = self.debug_background_tokenizer.value()
                && bg.has_report_mismatching_tokens()
            {
                bg.report_mismatching_tokens(line_number);
            }
        }
        result
    }

    pub fn get_token_type_if_inserting_character(&self, line_number: i32, column: i32, character: &JsString) -> u32 {
        let Some(tokenizer) = self.tokenizer.borrow().clone() else { return standard_token_type::OTHER };
        let position = self.model().validate_position(Position::new(line_number, column));
        self.force_tokenization(position.line_number);
        tokenizer.get_token_type_if_inserting_character(position, character)
    }

    pub fn tokenize_lines_at(&self, line_number: i32, lines: &[JsString]) -> Option<Vec<LineTokens>> {
        let tokenizer = self.tokenizer.borrow().clone()?;
        self.force_tokenization(line_number);
        tokenizer.tokenize_lines_at(line_number, lines)
    }

    pub fn has_tokens(&self) -> bool {
        self.tokens.borrow().has_tokens()
    }
}

impl IDisposable for TokenizerSyntaxTokenBackend {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}
