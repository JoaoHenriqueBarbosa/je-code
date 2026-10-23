//! Porte de `src/vs/editor/common/model/bracketPairsTextModelPart/bracketPairsImpl.ts`: a parte do modelo de
//! texto que responde às consultas de colchetes (`getBracketPairsInRange`, `getBracketsInRange`,
//! `matchBracket`, `findEnclosingBrackets`, `findPrevBracket`, `findNextBracket`, ...).
//!
//! Até 5 milhões de caracteres (`canBuildAST`) as respostas vêm da árvore de colchetes
//! ([`bracket_pairs_tree`]); acima disso, da busca linha a linha com as regex de `RichEditBrackets`.
//!
//! Divergências:
//! - `BracketSearchCanceled` do original é o `Err(Canceled)` do resultado das buscas.
//! - Os textos de colchete das consultas entram como `&str` (nos testes são ASCII); o modelo é UTF-16.
//! - O `MutableDisposable<IReference<BracketPairsTree>>` é um `Option<Rc<BracketPairsTree>>` com o
//!   disposable da inscrição no evento da árvore; as duas coisas são descartadas ao limpar.

pub mod bracket_pairs_tree;
pub mod colorized_bracket_pairs_decoration_provider;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Instant;

use je_base::JsString;
use je_base::arrays::{CallbackIterable, compare_by};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};

use self::bracket_pairs_tree::bracket_pairs_tree::BracketPairsTree;
use crate::language_brackets_configuration::LanguageBracketsConfiguration;
use crate::language_configuration_registry::{
    ILanguageConfigurationService, LanguageConfigurationServiceChangeEvent,
};
use crate::line_tokens::IViewLineTokens;
use crate::model::ModelOptionsChangedEvent;
use crate::position::Position;
use crate::range::Range;
use crate::rich_edit_brackets::{BracketsUtils, RichEditBracket, RichEditBrackets};
use crate::supports::ignore_brackets_in_token;
use crate::text_model::TextModel;
use crate::text_model_bracket_pairs::{
    BracketInfo, BracketPairInfo, BracketPairWithMinIndentationInfo, FoundBracket,
};
use crate::text_model_events::{
    ModelContentChangedEvent, ModelLanguageChangedEvent, ModelTokensChangedEvent,
};

/// `BracketSearchCanceled`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BracketSearchCanceled;

type SearchResult<T> = Result<Option<T>, BracketSearchCanceled>;

type ContinueBracketSearchPredicate<'a> = &'a dyn Fn() -> bool;

fn create_time_based_continue_bracket_search_predicate(
    max_duration: Option<u64>,
) -> Box<dyn Fn() -> bool> {
    match max_duration {
        None => Box::new(|| true),
        Some(max_duration) => {
            let start_time = Instant::now();
            Box::new(move || start_time.elapsed().as_millis() as u64 <= max_duration)
        }
    }
}

fn strip_bracket_search_canceled<T>(result: SearchResult<T>) -> Option<T> {
    result.unwrap_or(None)
}

fn lowered(units: &[u16]) -> JsString {
    JsString::from(units).to_lower_case()
}

fn sub(text: &JsString, start_column: i32, end_column: i32) -> JsString {
    lowered(&text.units()[(start_column - 1) as usize..(end_column - 1) as usize])
}

/// `BracketPairsTextModelPart`.
pub struct BracketPairsTextModelPart {
    base: Rc<Disposable>,
    text_model: Weak<TextModel>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    me: Weak<BracketPairsTextModelPart>,

    bracket_pairs_tree: RefCell<Option<Rc<BracketPairsTree>>>,
    tree_listener: RefCell<Option<DisposableRef>>,
    on_did_change_emitter: Rc<Emitter<()>>,
    brackets_requested: Cell<bool>,
}

impl BracketPairsTextModelPart {
    pub fn new(
        text_model: Weak<TextModel>,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_emitter = Emitter::new();
        base.register(on_did_change_emitter.clone());
        Rc::new_cyclic(|me| Self {
            base,
            text_model,
            language_configuration_service,
            me: me.clone(),
            bracket_pairs_tree: RefCell::new(None),
            tree_listener: RefCell::new(None),
            on_did_change_emitter,
            brackets_requested: Cell::new(false),
        })
    }

    fn model(&self) -> Rc<TextModel> {
        self.text_model
            .upgrade()
            .expect("o modelo de texto foi descartado")
    }

    /// `onDidChange`: disparado quando os pares de colchetes mudam, por texto ou por configuração.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change_emitter.event()
    }

    fn can_build_ast(&self) -> bool {
        let max_supported_document_length = /* máximo de linhas */ 50_000 * /* média de colunas */ 100;
        self.model().get_value_length() <= max_supported_document_length
    }

    fn clear_tree(&self) {
        if let Some(listener) = self.tree_listener.borrow_mut().take() {
            listener.dispose();
        }
        if let Some(tree) = self.bracket_pairs_tree.borrow_mut().take() {
            tree.dispose();
        }
    }

    fn tree(&self) -> Option<Rc<BracketPairsTree>> {
        self.bracket_pairs_tree.borrow().clone()
    }

    // #region TextModel events

    pub fn handle_language_configuration_service_change(
        &self,
        e: &LanguageConfigurationServiceChangeEvent,
    ) {
        let language_id_empty = e.language_id.as_deref().is_none_or(str::is_empty);
        let did_change = !language_id_empty
            && self
                .tree()
                .is_some_and(|t| t.did_language_change(e.language_id.as_deref().unwrap_or("")));
        if language_id_empty || did_change {
            self.clear_tree();
            self.update_bracket_pairs_tree();
        }
    }

    pub fn handle_did_change_options(&self, _e: &ModelOptionsChangedEvent) {
        self.clear_tree();
        self.update_bracket_pairs_tree();
    }

    pub fn handle_did_change_language(&self, _e: &ModelLanguageChangedEvent) {
        self.clear_tree();
        self.update_bracket_pairs_tree();
    }

    pub fn handle_did_change_content(&self, change: &ModelContentChangedEvent) {
        if let Some(tree) = self.tree() {
            tree.handle_content_changed(change);
        }
    }

    pub fn handle_did_change_background_tokenization_state(&self) {
        if let Some(tree) = self.tree() {
            tree.handle_did_change_background_tokenization_state();
        }
    }

    pub fn handle_did_change_tokens(&self, e: &ModelTokensChangedEvent) {
        if let Some(tree) = self.tree() {
            tree.handle_did_change_tokens(e);
        }
    }

    // #endregion

    fn update_bracket_pairs_tree(&self) {
        if self.brackets_requested.get() && self.can_build_ast() {
            if self.tree().is_none() {
                let service = self.language_configuration_service.clone();
                let tree = BracketPairsTree::new(
                    &self.model(),
                    Rc::new(move |language_id| service.get_language_configuration(language_id)),
                );
                let me = self.me.clone();
                let listener = tree.on_did_change().listen(move |_| {
                    if let Some(me) = me.upgrade() {
                        me.on_did_change_emitter.fire(());
                    }
                });
                *self.tree_listener.borrow_mut() = Some(listener);
                *self.bracket_pairs_tree.borrow_mut() = Some(tree);
                self.on_did_change_emitter.fire(());
            }
        } else if self.tree().is_some() {
            self.clear_tree();
            // Importante: não dispare se não houve mudança!
            self.on_did_change_emitter.fire(());
        }
    }

    /// Todos os pares de colchetes que cruzam o intervalo, ordenados pela posição inicial.
    pub fn get_bracket_pairs_in_range(&self, range: Range) -> CallbackIterable<BracketPairInfo> {
        self.brackets_requested.set(true);
        self.update_bracket_pairs_tree();
        match self.tree() {
            Some(tree) => tree.get_bracket_pairs_in_range(range, false),
            None => CallbackIterable::empty(),
        }
    }

    pub fn get_bracket_pairs_in_range_with_min_indentation(
        &self,
        range: Range,
    ) -> CallbackIterable<BracketPairWithMinIndentationInfo> {
        self.brackets_requested.set(true);
        self.update_bracket_pairs_tree();
        match self.tree() {
            Some(tree) => tree.get_bracket_pairs_in_range(range, true),
            None => CallbackIterable::empty(),
        }
    }

    pub fn get_brackets_in_range(
        &self,
        range: Range,
        only_colorized_brackets: bool,
    ) -> CallbackIterable<BracketInfo> {
        self.brackets_requested.set(true);
        self.update_bracket_pairs_tree();
        match self.tree() {
            Some(tree) => tree.get_brackets_in_range(range, only_colorized_brackets),
            None => CallbackIterable::empty(),
        }
    }

    pub fn has_unmatched_closing_bracket_after(
        &self,
        position: Position,
        opening_bracket: &str,
    ) -> bool {
        let model = self.model();
        let position = model.validate_position(position);
        let language_id = model.get_language_id_at_position(position.line_number, position.column);
        let text: Vec<u16> = opening_bracket.encode_utf16().collect();
        let configuration = self
            .language_configuration_service
            .get_language_configuration(&language_id);
        let Some(opening_bracket_info) = configuration.brackets_new.get_opening_bracket_info(&text)
        else {
            return false;
        };
        self.brackets_requested.set(true);
        self.update_bracket_pairs_tree();
        self.tree()
            .is_some_and(|t| t.has_unmatched_closing_bracket_after(position, opening_bracket_info))
    }

    pub fn find_matching_bracket_up(
        &self,
        bracket: &str,
        position: Position,
        max_duration: Option<u64>,
    ) -> Option<Range> {
        let model = self.model();
        let validated = model.validate_position(position);
        let language_id =
            model.get_language_id_at_position(validated.line_number, validated.column);

        if self.can_build_ast() {
            let text: Vec<u16> = bracket.encode_utf16().collect();
            let configuration = self
                .language_configuration_service
                .get_language_configuration(&language_id);
            let closing_bracket_info = configuration
                .brackets_new
                .get_closing_bracket_info(&text)?
                .clone();

            let bracket_pair = self
                .get_bracket_pairs_in_range(Range::from_positions(position, position))
                .find_last(|b| closing_bracket_info.closes(&b.opening_bracket_info()));

            bracket_pair.map(|b| b.opening_bracket_range)
        } else {
            // Volta para o código antigo de casamento de colchetes:
            let bracket = lowered(&bracket.encode_utf16().collect::<Vec<u16>>());

            let configuration = self
                .language_configuration_service
                .get_language_configuration(&language_id);
            let brackets_support = configuration.brackets()?;

            let data = brackets_support.text_is_bracket.get(&bracket)?;

            let predicate = create_time_based_continue_bracket_search_predicate(max_duration);
            strip_bracket_search_canceled(self.find_matching_bracket_up_legacy(
                data,
                validated,
                &*predicate,
            ))
        }
    }

    pub fn match_bracket(
        &self,
        position: Position,
        max_duration: Option<u64>,
    ) -> Option<(Range, Range)> {
        if self.can_build_ast() {
            let bracket_pair = self
                .get_bracket_pairs_in_range(Range::from_positions(position, position))
                .filter(move |item| {
                    item.closing_bracket_range.is_some()
                        && (item.opening_bracket_range.contains_position(&position)
                            || item
                                .closing_bracket_range
                                .is_some_and(|r| r.contains_position(&position)))
                })
                .find_last_max_by(compare_by(
                    move |item: &BracketPairInfo| {
                        if item.opening_bracket_range.contains_position(&position) {
                            item.opening_bracket_range
                        } else {
                            item.closing_bracket_range.expect("tem fechamento")
                        }
                    },
                    |a: &Range, b: &Range| {
                        Range::compare_ranges_using_starts(Some(a), Some(b)).into()
                    },
                ));
            bracket_pair.map(|b| {
                (
                    b.opening_bracket_range,
                    b.closing_bracket_range.expect("tem fechamento"),
                )
            })
        } else {
            // Volta para o código antigo de casamento de colchetes:
            let predicate = create_time_based_continue_bracket_search_predicate(max_duration);
            let validated = self.model().validate_position(position);
            self.match_bracket_legacy(validated, &*predicate)
        }
    }

    fn establish_bracket_search_offsets(
        &self,
        position: Position,
        line_tokens: &crate::line_tokens::LineTokens,
        mode_brackets: &RichEditBrackets,
        token_index: usize,
    ) -> (i32, i32) {
        let token_count = line_tokens.get_count();
        let current_language_id = line_tokens.get_language_id(token_index);

        // limita a busca para não ir antes de `maxBracketLength`
        let mut search_start_offset =
            (position.column - 1 - mode_brackets.max_bracket_length as i32).max(0);
        for i in (0..token_index).rev() {
            let token_end_offset = line_tokens.get_end_offset(i);
            if token_end_offset <= search_start_offset {
                break;
            }
            if ignore_brackets_in_token(line_tokens.get_standard_token_type(i))
                || line_tokens.get_language_id(i) != current_language_id
            {
                search_start_offset = token_end_offset;
                break;
            }
        }

        // limita a busca para não ir depois de `maxBracketLength`
        let mut search_end_offset = (line_tokens.get_line_content().len() as i32)
            .min(position.column - 1 + mode_brackets.max_bracket_length as i32);
        for i in token_index + 1..token_count {
            let token_start_offset = line_tokens.get_start_offset(i);
            if token_start_offset >= search_end_offset {
                break;
            }
            if ignore_brackets_in_token(line_tokens.get_standard_token_type(i))
                || line_tokens.get_language_id(i) != current_language_id
            {
                search_end_offset = token_start_offset;
                break;
            }
        }

        (search_start_offset, search_end_offset)
    }

    fn match_bracket_legacy(
        &self,
        position: Position,
        continue_search_predicate: ContinueBracketSearchPredicate,
    ) -> Option<(Range, Range)> {
        let model = self.model();
        let line_number = position.line_number;
        let line_tokens = model.tokenization().get_line_tokens(line_number);
        let line_text = model.get_line_content(line_number);

        let token_index = line_tokens.find_token_index_at_offset(position.column - 1);
        let current_mode_brackets = self
            .language_configuration_service
            .get_language_configuration(&line_tokens.get_language_id(token_index))
            .brackets();

        // confere que o token não deve ser ignorado
        if let Some(current_mode_brackets) = &current_mode_brackets
            && !ignore_brackets_in_token(line_tokens.get_standard_token_type(token_index))
        {
            let (mut search_start_offset, search_end_offset) = self
                .establish_bracket_search_offsets(
                    position,
                    &line_tokens,
                    current_mode_brackets,
                    token_index,
                );

            // pode ser que [início do token -> fim do token] contenha vários colchetes
            // `best_result` guarda o resultado mais à direita
            let mut best_result: Option<(Range, Range)> = None;
            // quando não há mais colchetes neste texto, o laço termina
            while let Some(found_bracket) = BracketsUtils::find_next_bracket_in_range(
                &current_mode_brackets.forward_regex,
                line_number,
                line_text.units(),
                search_start_offset,
                search_end_offset,
            ) {
                // confere que não achamos um colchete longe demais da posição
                if found_bracket.start_column <= position.column
                    && position.column <= found_bracket.end_column
                {
                    let found_bracket_text = sub(
                        &line_text,
                        found_bracket.start_column,
                        found_bracket.end_column,
                    );
                    let data = current_mode_brackets
                        .text_is_bracket
                        .get(&found_bracket_text)
                        .cloned();
                    let is_open = current_mode_brackets
                        .text_is_open_bracket
                        .get(&found_bracket_text)
                        .copied()
                        .unwrap_or(false);
                    match self.match_found_bracket(
                        found_bracket,
                        data.as_deref(),
                        is_open,
                        continue_search_predicate,
                    ) {
                        Err(BracketSearchCanceled) => return None,
                        Ok(Some(r)) => best_result = Some(r),
                        Ok(None) => {}
                    }
                }

                search_start_offset = found_bracket.end_column - 1;
            }

            if best_result.is_some() {
                return best_result;
            }
        }

        // Se a posição está entre dois tokens, tenta também o token anterior
        if token_index > 0 && line_tokens.get_start_offset(token_index) == position.column - 1 {
            let prev_token_index = token_index - 1;
            let prev_mode_brackets = self
                .language_configuration_service
                .get_language_configuration(&line_tokens.get_language_id(prev_token_index))
                .brackets();

            // confere que o token anterior não deve ser ignorado
            if let Some(prev_mode_brackets) = &prev_mode_brackets
                && !ignore_brackets_in_token(line_tokens.get_standard_token_type(prev_token_index))
            {
                let (search_start_offset, search_end_offset) = self
                    .establish_bracket_search_offsets(
                        position,
                        &line_tokens,
                        prev_mode_brackets,
                        prev_token_index,
                    );

                let found_bracket = BracketsUtils::find_prev_bracket_in_range(
                    &prev_mode_brackets.reversed_regex,
                    line_number,
                    line_text.units(),
                    search_start_offset,
                    search_end_offset,
                );

                // confere que não achamos um colchete longe demais da posição
                if let Some(found_bracket) = found_bracket
                    && found_bracket.start_column <= position.column
                    && position.column <= found_bracket.end_column
                {
                    let found_bracket_text = sub(
                        &line_text,
                        found_bracket.start_column,
                        found_bracket.end_column,
                    );
                    let data = prev_mode_brackets
                        .text_is_bracket
                        .get(&found_bracket_text)
                        .cloned();
                    let is_open = prev_mode_brackets
                        .text_is_open_bracket
                        .get(&found_bracket_text)
                        .copied()
                        .unwrap_or(false);
                    match self.match_found_bracket(
                        found_bracket,
                        data.as_deref(),
                        is_open,
                        continue_search_predicate,
                    ) {
                        Err(BracketSearchCanceled) => return None,
                        Ok(Some(r)) => return Some(r),
                        Ok(None) => {}
                    }
                }
            }
        }

        None
    }

    fn match_found_bracket(
        &self,
        found_bracket: Range,
        data: Option<&RichEditBracket>,
        is_open: bool,
        continue_search_predicate: ContinueBracketSearchPredicate,
    ) -> SearchResult<(Range, Range)> {
        let Some(data) = data else { return Ok(None) };

        let matched = if is_open {
            self.find_matching_bracket_down(
                data,
                found_bracket.get_end_position(),
                continue_search_predicate,
            )?
        } else {
            self.find_matching_bracket_up_legacy(
                data,
                found_bracket.get_start_position(),
                continue_search_predicate,
            )?
        };

        Ok(matched.map(|matched| (found_bracket, matched)))
    }

    fn find_matching_bracket_up_legacy(
        &self,
        bracket: &RichEditBracket,
        position: Position,
        continue_search_predicate: ContinueBracketSearchPredicate,
    ) -> SearchResult<Range> {
        let model = self.model();
        let language_id = &bracket.language_id;
        let reversed_bracket_regex = &bracket.reversed_regex;
        let mut count: i32 = -1;

        let mut total_call_count: usize = 0;
        let mut search_prev_matching_bracket_in_range = |line_number: i32,
                                                         line_text: &JsString,
                                                         search_start_offset: i32,
                                                         mut search_end_offset: i32|
         -> SearchResult<Range> {
            loop {
                total_call_count += 1;
                if total_call_count.is_multiple_of(100) && !continue_search_predicate() {
                    return Err(BracketSearchCanceled);
                }
                let Some(r) = BracketsUtils::find_prev_bracket_in_range(
                    reversed_bracket_regex,
                    line_number,
                    line_text.units(),
                    search_start_offset,
                    search_end_offset,
                ) else {
                    break;
                };

                let hit_text = sub(line_text, r.start_column, r.end_column);
                if bracket.is_open(hit_text.units()) {
                    count += 1;
                } else if bracket.is_close(hit_text.units()) {
                    count -= 1;
                }

                if count == 0 {
                    return Ok(Some(r));
                }

                search_end_offset = r.start_column - 1;
            }

            Ok(None)
        };

        let mut line_number = position.line_number;
        while line_number >= 1 {
            let line_tokens = model.tokenization().get_line_tokens(line_number);
            let token_count = line_tokens.get_count();
            let line_text = model.get_line_content(line_number);

            let mut token_index = token_count as i64 - 1;
            let mut search_start_offset = line_text.len() as i32;
            let mut search_end_offset = line_text.len() as i32;
            if line_number == position.line_number {
                token_index = line_tokens.find_token_index_at_offset(position.column - 1) as i64;
                search_start_offset = position.column - 1;
                search_end_offset = position.column - 1;
            }

            let mut prev_search_in_token = true;
            while token_index >= 0 {
                let ti = token_index as usize;
                let search_in_token = line_tokens.get_language_id(ti) == *language_id
                    && !ignore_brackets_in_token(line_tokens.get_standard_token_type(ti));

                if search_in_token {
                    // este token deve ser pesquisado
                    if prev_search_in_token {
                        // o token anterior deve ser pesquisado, basta estender searchStartOffset
                        search_start_offset = line_tokens.get_start_offset(ti);
                    } else {
                        // o token anterior não deve ser pesquisado
                        search_start_offset = line_tokens.get_start_offset(ti);
                        search_end_offset = line_tokens.get_end_offset(ti);
                    }
                } else {
                    // este token não deve ser pesquisado
                    if prev_search_in_token
                        && search_start_offset != search_end_offset
                        && let Some(r) = search_prev_matching_bracket_in_range(
                            line_number,
                            &line_text,
                            search_start_offset,
                            search_end_offset,
                        )?
                    {
                        return Ok(Some(r));
                    }
                }

                prev_search_in_token = search_in_token;
                token_index -= 1;
            }

            if prev_search_in_token
                && search_start_offset != search_end_offset
                && let Some(r) = search_prev_matching_bracket_in_range(
                    line_number,
                    &line_text,
                    search_start_offset,
                    search_end_offset,
                )?
            {
                return Ok(Some(r));
            }
            line_number -= 1;
        }

        Ok(None)
    }

    fn find_matching_bracket_down(
        &self,
        bracket: &RichEditBracket,
        position: Position,
        continue_search_predicate: ContinueBracketSearchPredicate,
    ) -> SearchResult<Range> {
        let model = self.model();
        let language_id = &bracket.language_id;
        let bracket_regex = &bracket.forward_regex;
        let mut count: i32 = 1;

        let mut total_call_count: usize = 0;
        let mut search_next_matching_bracket_in_range = |line_number: i32,
                                                         line_text: &JsString,
                                                         mut search_start_offset: i32,
                                                         search_end_offset: i32|
         -> SearchResult<Range> {
            loop {
                total_call_count += 1;
                if total_call_count.is_multiple_of(100) && !continue_search_predicate() {
                    return Err(BracketSearchCanceled);
                }
                let Some(r) = BracketsUtils::find_next_bracket_in_range(
                    bracket_regex,
                    line_number,
                    line_text.units(),
                    search_start_offset,
                    search_end_offset,
                ) else {
                    break;
                };

                let hit_text = sub(line_text, r.start_column, r.end_column);
                if bracket.is_open(hit_text.units()) {
                    count += 1;
                } else if bracket.is_close(hit_text.units()) {
                    count -= 1;
                }

                if count == 0 {
                    return Ok(Some(r));
                }

                search_start_offset = r.end_column - 1;
            }

            Ok(None)
        };

        let line_count = model.get_line_count();
        for line_number in position.line_number..=line_count {
            let line_tokens = model.tokenization().get_line_tokens(line_number);
            let token_count = line_tokens.get_count();
            let line_text = model.get_line_content(line_number);

            let mut token_index = 0;
            let mut search_start_offset = 0;
            let mut search_end_offset = 0;
            if line_number == position.line_number {
                token_index = line_tokens.find_token_index_at_offset(position.column - 1);
                search_start_offset = position.column - 1;
                search_end_offset = position.column - 1;
            }

            let mut prev_search_in_token = true;
            while token_index < token_count {
                let search_in_token = line_tokens.get_language_id(token_index) == *language_id
                    && !ignore_brackets_in_token(line_tokens.get_standard_token_type(token_index));

                if search_in_token {
                    // este token deve ser pesquisado
                    if prev_search_in_token {
                        // o token anterior deve ser pesquisado, basta estender searchEndOffset
                        search_end_offset = line_tokens.get_end_offset(token_index);
                    } else {
                        // o token anterior não deve ser pesquisado
                        search_start_offset = line_tokens.get_start_offset(token_index);
                        search_end_offset = line_tokens.get_end_offset(token_index);
                    }
                } else {
                    // este token não deve ser pesquisado
                    if prev_search_in_token
                        && search_start_offset != search_end_offset
                        && let Some(r) = search_next_matching_bracket_in_range(
                            line_number,
                            &line_text,
                            search_start_offset,
                            search_end_offset,
                        )?
                    {
                        return Ok(Some(r));
                    }
                }

                prev_search_in_token = search_in_token;
                token_index += 1;
            }

            if prev_search_in_token
                && search_start_offset != search_end_offset
                && let Some(r) = search_next_matching_bracket_in_range(
                    line_number,
                    &line_text,
                    search_start_offset,
                    search_end_offset,
                )?
            {
                return Ok(Some(r));
            }
        }

        Ok(None)
    }

    fn to_found_bracket(
        &self,
        bracket_config: &LanguageBracketsConfiguration,
        r: Range,
    ) -> Option<FoundBracket> {
        let text = lowered(self.model().get_value_in_range(&r).units());
        let bracket_info = bracket_config.get_bracket_info(text.units())?;
        Some(FoundBracket {
            range: r,
            bracket_info,
        })
    }

    /// Procura, de trás para frente ou para frente, o primeiro colchete a partir de `position` na busca
    /// linha a linha (a de `prev`/`next` acima de `canBuildAST`).
    fn find_bracket_legacy(&self, position: Position, forward: bool) -> Option<FoundBracket> {
        let model = self.model();
        let line_count = model.get_line_count();

        let mut language_id: Option<String> = None;
        let mut mode_brackets: Option<Rc<RichEditBrackets>> = None;
        let mut bracket_config: Option<
            Rc<crate::language_configuration_registry::ResolvedLanguageConfiguration>,
        > = None;

        let regex_of = |mb: &RichEditBrackets| {
            if forward {
                mb.forward_regex.clone()
            } else {
                mb.reversed_regex.clone()
            }
        };
        let find_in_range = |mb: &RichEditBrackets,
                             line_number: i32,
                             line_text: &JsString,
                             start: i32,
                             end: i32| {
            if forward {
                BracketsUtils::find_next_bracket_in_range(
                    &regex_of(mb),
                    line_number,
                    line_text.units(),
                    start,
                    end,
                )
            } else {
                BracketsUtils::find_prev_bracket_in_range(
                    &regex_of(mb),
                    line_number,
                    line_text.units(),
                    start,
                    end,
                )
            }
        };

        let mut line_number = position.line_number;
        while if forward {
            line_number <= line_count
        } else {
            line_number >= 1
        } {
            let line_tokens = model.tokenization().get_line_tokens(line_number);
            let token_count = line_tokens.get_count();
            let line_text = model.get_line_content(line_number);

            let mut token_index: i64 = if forward { 0 } else { token_count as i64 - 1 };
            let mut search_start_offset = if forward { 0 } else { line_text.len() as i32 };
            let mut search_end_offset = search_start_offset;
            if line_number == position.line_number {
                token_index = line_tokens.find_token_index_at_offset(position.column - 1) as i64;
                search_start_offset = position.column - 1;
                search_end_offset = position.column - 1;
                let token_language_id = line_tokens.get_language_id(token_index as usize);
                if language_id.as_deref() != Some(token_language_id.as_str()) {
                    let configuration = self
                        .language_configuration_service
                        .get_language_configuration(&token_language_id);
                    mode_brackets = configuration.brackets();
                    bracket_config = Some(configuration);
                    language_id = Some(token_language_id);
                }
            }

            let mut prev_search_in_token = true;
            while if forward {
                token_index < token_count as i64
            } else {
                token_index >= 0
            } {
                let ti = token_index as usize;
                let token_language_id = line_tokens.get_language_id(ti);

                if language_id.as_deref() != Some(token_language_id.as_str()) {
                    // mudança de linguagem!
                    if let (Some(mb), Some(config)) = (&mode_brackets, &bracket_config)
                        && prev_search_in_token
                        && search_start_offset != search_end_offset
                    {
                        if let Some(r) = find_in_range(
                            mb,
                            line_number,
                            &line_text,
                            search_start_offset,
                            search_end_offset,
                        ) {
                            return self.to_found_bracket(&config.brackets_new, r);
                        }
                        prev_search_in_token = false;
                    }
                    let configuration = self
                        .language_configuration_service
                        .get_language_configuration(&token_language_id);
                    mode_brackets = configuration.brackets();
                    bracket_config = Some(configuration);
                    language_id = Some(token_language_id);
                }

                let search_in_token = mode_brackets.is_some()
                    && !ignore_brackets_in_token(line_tokens.get_standard_token_type(ti));

                if search_in_token {
                    // este token deve ser pesquisado
                    if prev_search_in_token {
                        // o token anterior deve ser pesquisado, basta estender o limite
                        if forward {
                            search_end_offset = line_tokens.get_end_offset(ti);
                        } else {
                            search_start_offset = line_tokens.get_start_offset(ti);
                        }
                    } else {
                        // o token anterior não deve ser pesquisado
                        search_start_offset = line_tokens.get_start_offset(ti);
                        search_end_offset = line_tokens.get_end_offset(ti);
                    }
                } else if let (Some(mb), Some(config)) = (&mode_brackets, &bracket_config)
                    && prev_search_in_token
                    && search_start_offset != search_end_offset
                {
                    // este token não deve ser pesquisado
                    if let Some(r) = find_in_range(
                        mb,
                        line_number,
                        &line_text,
                        search_start_offset,
                        search_end_offset,
                    ) {
                        return self.to_found_bracket(&config.brackets_new, r);
                    }
                }

                prev_search_in_token = search_in_token;
                token_index += if forward { 1 } else { -1 };
            }

            if let (Some(mb), Some(config)) = (&mode_brackets, &bracket_config)
                && prev_search_in_token
                && search_start_offset != search_end_offset
                && let Some(r) = find_in_range(
                    mb,
                    line_number,
                    &line_text,
                    search_start_offset,
                    search_end_offset,
                )
            {
                return self.to_found_bracket(&config.brackets_new, r);
            }

            line_number += if forward { 1 } else { -1 };
        }

        None
    }

    pub fn find_prev_bracket(&self, position: Position) -> Option<FoundBracket> {
        let position = self.model().validate_position(position);

        if self.can_build_ast() {
            self.brackets_requested.set(true);
            self.update_bracket_pairs_tree();
            return self
                .tree()
                .and_then(|t| t.get_first_bracket_before(position));
        }

        self.find_bracket_legacy(position, false)
    }

    pub fn find_next_bracket(&self, position: Position) -> Option<FoundBracket> {
        let position = self.model().validate_position(position);

        if self.can_build_ast() {
            self.brackets_requested.set(true);
            self.update_bracket_pairs_tree();
            return self
                .tree()
                .and_then(|t| t.get_first_bracket_after(position));
        }

        self.find_bracket_legacy(position, true)
    }

    pub fn find_enclosing_brackets(
        &self,
        position: Position,
        max_duration: Option<u64>,
    ) -> Option<(Range, Range)> {
        let model = self.model();
        let position = model.validate_position(position);

        if self.can_build_ast() {
            let range = Range::from_positions(position, position);
            let bracket_pair = self
                .get_bracket_pairs_in_range(Range::from_positions(position, position))
                .find_last(move |item| {
                    item.closing_bracket_range.is_some() && item.range.strict_contains_range(&range)
                });
            return bracket_pair.map(|b| {
                (
                    b.opening_bracket_range,
                    b.closing_bracket_range.expect("tem fechamento"),
                )
            });
        }

        let predicate = create_time_based_continue_bracket_search_predicate(max_duration);
        let continue_search_predicate: ContinueBracketSearchPredicate = &*predicate;
        let line_count = model.get_line_count();
        let mut saved_counts: std::collections::HashMap<String, Vec<i32>> =
            std::collections::HashMap::new();

        let mut total_call_count: usize = 0;

        let mut language_id: Option<String> = None;
        let mut mode_brackets: Option<Rc<RichEditBrackets>> = None;

        // A busca num trecho de texto de uma linguagem. Os contadores são os da linguagem corrente.
        let mut search_in_range = |mode_brackets: &RichEditBrackets,
                                   counts: &mut Vec<i32>,
                                   line_number: i32,
                                   line_text: &JsString,
                                   mut search_start_offset: i32,
                                   search_end_offset: i32|
         -> SearchResult<(Range, Range)> {
            loop {
                total_call_count += 1;
                if total_call_count.is_multiple_of(100) && !continue_search_predicate() {
                    return Err(BracketSearchCanceled);
                }
                let Some(r) = BracketsUtils::find_next_bracket_in_range(
                    &mode_brackets.forward_regex,
                    line_number,
                    line_text.units(),
                    search_start_offset,
                    search_end_offset,
                ) else {
                    break;
                };

                let hit_text = sub(line_text, r.start_column, r.end_column);
                if let Some(bracket) = mode_brackets.text_is_bracket.get(&hit_text) {
                    if bracket.is_open(hit_text.units()) {
                        counts[bracket.index] += 1;
                    } else if bracket.is_close(hit_text.units()) {
                        counts[bracket.index] -= 1;
                    }

                    if counts[bracket.index] == -1 {
                        return self.match_found_bracket(
                            r,
                            Some(bracket),
                            false,
                            continue_search_predicate,
                        );
                    }
                }

                search_start_offset = r.end_column - 1;
            }
            Ok(None)
        };

        let reset_counts =
            |saved_counts: &mut std::collections::HashMap<String, Vec<i32>>,
             language_id: &str,
             mode_brackets: &Option<Rc<RichEditBrackets>>| {
                saved_counts
                    .entry(language_id.to_owned())
                    .or_insert_with(|| {
                        vec![0; mode_brackets.as_ref().map_or(0, |m| m.brackets.len())]
                    });
            };

        for line_number in position.line_number..=line_count {
            let line_tokens = model.tokenization().get_line_tokens(line_number);
            let token_count = line_tokens.get_count();
            let line_text = model.get_line_content(line_number);

            let mut token_index = 0;
            let mut search_start_offset = 0;
            let mut search_end_offset = 0;
            if line_number == position.line_number {
                token_index = line_tokens.find_token_index_at_offset(position.column - 1);
                search_start_offset = position.column - 1;
                search_end_offset = position.column - 1;
                let token_language_id = line_tokens.get_language_id(token_index);
                if language_id.as_deref() != Some(token_language_id.as_str()) {
                    mode_brackets = self
                        .language_configuration_service
                        .get_language_configuration(&token_language_id)
                        .brackets();
                    reset_counts(&mut saved_counts, &token_language_id, &mode_brackets);
                    language_id = Some(token_language_id);
                }
            }

            let mut prev_search_in_token = true;
            while token_index < token_count {
                let token_language_id = line_tokens.get_language_id(token_index);

                if language_id.as_deref() != Some(token_language_id.as_str()) {
                    // mudança de linguagem!
                    if let Some(mb) = &mode_brackets
                        && prev_search_in_token
                        && search_start_offset != search_end_offset
                    {
                        let counts = saved_counts
                            .get_mut(language_id.as_deref().unwrap_or(""))
                            .expect("contadores da linguagem");
                        let r = search_in_range(
                            mb,
                            counts,
                            line_number,
                            &line_text,
                            search_start_offset,
                            search_end_offset,
                        );
                        match r {
                            Ok(Some(r)) => return Some(r),
                            Err(BracketSearchCanceled) => return None,
                            Ok(None) => {}
                        }
                        prev_search_in_token = false;
                    }
                    mode_brackets = self
                        .language_configuration_service
                        .get_language_configuration(&token_language_id)
                        .brackets();
                    reset_counts(&mut saved_counts, &token_language_id, &mode_brackets);
                    language_id = Some(token_language_id);
                }

                let search_in_token = mode_brackets.is_some()
                    && !ignore_brackets_in_token(line_tokens.get_standard_token_type(token_index));
                if search_in_token {
                    // este token deve ser pesquisado
                    if prev_search_in_token {
                        // o token anterior deve ser pesquisado, basta estender searchEndOffset
                        search_end_offset = line_tokens.get_end_offset(token_index);
                    } else {
                        // o token anterior não deve ser pesquisado
                        search_start_offset = line_tokens.get_start_offset(token_index);
                        search_end_offset = line_tokens.get_end_offset(token_index);
                    }
                } else if let Some(mb) = &mode_brackets
                    && prev_search_in_token
                    && search_start_offset != search_end_offset
                {
                    // este token não deve ser pesquisado
                    let counts = saved_counts
                        .get_mut(language_id.as_deref().unwrap_or(""))
                        .expect("contadores da linguagem");
                    let r = search_in_range(
                        mb,
                        counts,
                        line_number,
                        &line_text,
                        search_start_offset,
                        search_end_offset,
                    );
                    match r {
                        Ok(Some(r)) => return Some(r),
                        Err(BracketSearchCanceled) => return None,
                        Ok(None) => {}
                    }
                }

                prev_search_in_token = search_in_token;
                token_index += 1;
            }

            if let Some(mb) = &mode_brackets
                && prev_search_in_token
                && search_start_offset != search_end_offset
            {
                let counts = saved_counts
                    .get_mut(language_id.as_deref().unwrap_or(""))
                    .expect("contadores da linguagem");
                let r = search_in_range(
                    mb,
                    counts,
                    line_number,
                    &line_text,
                    search_start_offset,
                    search_end_offset,
                );
                match r {
                    Ok(Some(r)) => return Some(r),
                    Err(BracketSearchCanceled) => return None,
                    Ok(None) => {}
                }
            }
        }

        None
    }
}

impl IDisposable for BracketPairsTextModelPart {
    fn dispose(&self) {
        self.clear_tree();
        IDisposable::dispose(&*self.base);
    }
}
