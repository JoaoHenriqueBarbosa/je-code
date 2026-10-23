//! Porte de `.../bracketPairsTree/bracketPairsTree.ts`: a árvore de pares de colchetes de um modelo, com as
//! consultas (`getBracketsInRange`, `getBracketPairsInRange`, o primeiro colchete antes e depois de uma
//! posição) e a atualização incremental por edições e por mudanças de tokens.
//!
//! Há duas árvores: a inicial, sem informação de tokens, usada para a colorização inicial rápida, e a que usa
//! tokens. Para evitar cintilação, só se troca da inicial para a com tokens quando a tokenização termina. Como o
//! texto pode ser editado durante a tokenização em segundo plano, as duas são atualizadas.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::arrays::CallbackIterable;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};

use super::ast::{AstNode, AstNodeKind, AstNodeRef};
use super::before_edit_position_mapper::TextEditInfo;
use super::brackets::LanguageAgnosticBracketTokens;
use super::combine_text_edit_infos::combine_text_edit_infos;
use super::length::{
    LENGTH_ZERO, Length, length_add, length_greater_than_equal, length_less_than,
    length_less_than_equal, lengths_to_range, position_to_length, to_length,
};
use super::parser::parse_document;
use super::small_immutable_set::DenseKeyProvider;
use super::tokenizer::{FastTokenizer, TextBufferTokenizer, Token, TokenKind};
use crate::language_brackets_configuration::{BracketKind, OpeningBracketKind};
use crate::language_configuration_registry::ResolvedLanguageConfiguration;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;
use crate::text_model_bracket_pairs::{
    BracketInfo, BracketPairWithMinIndentationInfo, FoundBracket,
};
use crate::text_model_events::{ModelContentChangedEvent, ModelTokensChangedEvent};
use crate::tokenizer_syntax_token_backend::BackgroundTokenizationState;

/// A função que devolve a configuração de uma linguagem.
pub type GetLanguageConfiguration = Rc<dyn Fn(&str) -> Rc<ResolvedLanguageConfiguration>>;

/// `BracketPairsTree`.
pub struct BracketPairsTree {
    base: Rc<Disposable>,
    did_change_emitter: Rc<Emitter<()>>,
    text_model: Weak<TextModel>,

    initial_ast_without_tokens: RefCell<Option<AstNodeRef>>,
    ast_with_tokens: RefCell<Option<AstNodeRef>>,

    brackets: LanguageAgnosticBracketTokens,
    queued_text_edits_for_initial_ast_without_tokens: RefCell<Vec<TextEditInfo>>,
    queued_text_edits: RefCell<Vec<TextEditInfo>>,
}

impl BracketPairsTree {
    pub fn new(
        text_model: &Rc<TextModel>,
        get_language_configuration: GetLanguageConfiguration,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let did_change_emitter = Emitter::new();
        base.register(did_change_emitter.clone());

        let dense_key_provider = Rc::new(DenseKeyProvider::<String>::new());
        let brackets = LanguageAgnosticBracketTokens::new(
            dense_key_provider,
            Box::new(move |id| get_language_configuration(id)),
        );

        let tree = Rc::new(Self {
            base,
            did_change_emitter,
            text_model: Rc::downgrade(text_model),
            initial_ast_without_tokens: RefCell::new(None),
            ast_with_tokens: RefCell::new(None),
            brackets,
            queued_text_edits_for_initial_ast_without_tokens: RefCell::new(Vec::new()),
            queued_text_edits: RefCell::new(Vec::new()),
        });

        if !text_model.tokenization().has_tokens() {
            let brackets = tree
                .brackets
                .get_single_language_bracket_tokens(&text_model.get_language_id());
            let mut tokenizer = FastTokenizer::new(text_model.get_value(), &brackets);
            let ast = parse_document(&mut tokenizer, &[], None, true);
            *tree.initial_ast_without_tokens.borrow_mut() = Some(ast.clone());
            *tree.ast_with_tokens.borrow_mut() = Some(ast);
        } else if text_model.tokenization().background_tokenization_state()
            == BackgroundTokenizationState::Completed
        {
            // Pula a árvore inicial, já que não há cintilação.
            // Cria direto a árvore com informação de tokens.
            *tree.initial_ast_without_tokens.borrow_mut() = None;
            let ast = tree.parse_document_from_text_buffer(&[], None, false);
            *tree.ast_with_tokens.borrow_mut() = Some(ast);
        } else {
            // Já perdemos algumas mudanças de tokens, então não dá para usar o tokenizador rápido + deltas incrementais
            let ast = tree.parse_document_from_text_buffer(&[], None, true);
            *tree.initial_ast_without_tokens.borrow_mut() = Some(ast.clone());
            *tree.ast_with_tokens.borrow_mut() = Some(ast);
        }
        tree
    }

    pub fn did_language_change(&self, language_id: &str) -> bool {
        self.brackets.did_language_change(language_id)
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.did_change_emitter.event()
    }

    fn model(&self) -> Rc<TextModel> {
        self.text_model
            .upgrade()
            .expect("o modelo de texto foi descartado")
    }

    // #region TextModel events

    pub fn handle_did_change_background_tokenization_state(&self) {
        if self.model().tokenization().background_tokenization_state()
            == BackgroundTokenizationState::Completed
        {
            let was_undefined = self.initial_ast_without_tokens.borrow().is_none();
            // Limpa a árvore inicial, já que agora podemos usar a árvore com informação de tokens.
            *self.initial_ast_without_tokens.borrow_mut() = None;

            if !was_undefined {
                self.did_change_emitter.fire(());
            }
        }
    }

    pub fn handle_did_change_tokens(&self, e: &ModelTokensChangedEvent) {
        let edits: Vec<TextEditInfo> = e
            .ranges
            .iter()
            .map(|r| {
                TextEditInfo::new(
                    to_length(i64::from(r.from_line_number) - 1, 0),
                    to_length(i64::from(r.to_line_number), 0),
                    to_length(i64::from(r.to_line_number - r.from_line_number) + 1, 0),
                )
            })
            .collect();
        self.handle_edits(&edits, true);

        if self.initial_ast_without_tokens.borrow().is_none() {
            self.did_change_emitter.fire(());
        }
    }

    pub fn handle_content_changed(&self, change: &ModelContentChangedEvent) {
        let edits = TextEditInfo::from_model_content_changes(&change.changes);
        self.handle_edits(&edits, false);
    }

    fn handle_edits(&self, edits: &[TextEditInfo], token_change: bool) {
        // Enfileira as edições de forma preguiçosa e só as aplica quando a árvore é acessada.
        let result = combine_text_edit_infos(&self.queued_text_edits.borrow(), edits);
        *self.queued_text_edits.borrow_mut() = result;

        if self.initial_ast_without_tokens.borrow().is_some() && !token_change {
            let combined = combine_text_edit_infos(
                &self
                    .queued_text_edits_for_initial_ast_without_tokens
                    .borrow(),
                edits,
            );
            *self
                .queued_text_edits_for_initial_ast_without_tokens
                .borrow_mut() = combined;
        }
    }

    // #endregion

    fn flush_queue(&self) {
        if !self.queued_text_edits.borrow().is_empty() {
            let queued = std::mem::take(&mut *self.queued_text_edits.borrow_mut());
            let previous = self.ast_with_tokens.borrow().clone();
            let ast = self.parse_document_from_text_buffer(&queued, previous, false);
            *self.ast_with_tokens.borrow_mut() = Some(ast);
        }
        if !self
            .queued_text_edits_for_initial_ast_without_tokens
            .borrow()
            .is_empty()
        {
            let queued = std::mem::take(
                &mut *self
                    .queued_text_edits_for_initial_ast_without_tokens
                    .borrow_mut(),
            );
            let previous = self.initial_ast_without_tokens.borrow().clone();
            if let Some(previous) = previous {
                let ast = self.parse_document_from_text_buffer(&queued, Some(previous), false);
                *self.initial_ast_without_tokens.borrow_mut() = Some(ast);
            }
        }
    }

    /// `isPure = false`: é bem mais rápido, a árvore anterior é reaproveitada (e alterada) no lugar.
    fn parse_document_from_text_buffer(
        &self,
        edits: &[TextEditInfo],
        previous_ast: Option<AstNodeRef>,
        immutable: bool,
    ) -> AstNodeRef {
        let model = self.model();
        let mut tokenizer = TextBufferTokenizer::new(&*model, &self.brackets);
        parse_document(&mut tokenizer, edits, previous_ast, immutable)
    }

    fn current_node(&self) -> AstNodeRef {
        self.initial_ast_without_tokens
            .borrow()
            .clone()
            .or_else(|| self.ast_with_tokens.borrow().clone())
            .expect("há uma árvore")
    }

    pub fn get_brackets_in_range(
        &self,
        range: Range,
        only_colorized_brackets: bool,
    ) -> CallbackIterable<BracketInfo> {
        self.flush_queue();

        let start_offset = to_length(
            i64::from(range.start_line_number) - 1,
            i64::from(range.start_column) - 1,
        );
        let end_offset = to_length(
            i64::from(range.end_line_number) - 1,
            i64::from(range.end_column) - 1,
        );

        let node = self.current_node();
        CallbackIterable::new(move |cb| {
            let mut level_per_bracket_type: HashMap<JsString, i64> = HashMap::new();
            collect_brackets(
                node.clone(),
                LENGTH_ZERO,
                node.length(),
                start_offset,
                end_offset,
                cb,
                0,
                0,
                &mut level_per_bracket_type,
                only_colorized_brackets,
                false,
            );
        })
    }

    pub fn get_bracket_pairs_in_range(
        &self,
        range: Range,
        include_min_indentation: bool,
    ) -> CallbackIterable<BracketPairWithMinIndentationInfo> {
        self.flush_queue();

        let start_length = position_to_length(range.get_start_position());
        let end_length = position_to_length(range.get_end_position());

        let node = self.current_node();
        let text_model = self.model();
        CallbackIterable::new(move |cb| {
            let mut context = CollectBracketPairsContext {
                push: cb,
                include_min_indentation,
                text_model: &text_model,
            };
            let mut level_per_bracket_type: HashMap<JsString, i64> = HashMap::new();
            collect_bracket_pairs(
                node.clone(),
                LENGTH_ZERO,
                node.length(),
                start_length,
                end_length,
                &mut context,
                0,
                &mut level_per_bracket_type,
            );
        })
    }

    pub fn has_unmatched_closing_bracket_after(
        &self,
        position: Position,
        opening_bracket: &OpeningBracketKind,
    ) -> bool {
        let model = self.model();
        // Adia as verificações semânticas de colchetes até a tokenização estar precisa em todo o modelo.
        if !model
            .tokenization()
            .has_accurate_tokens_for_line(model.get_line_count())
        {
            return false;
        }

        self.flush_queue();

        let Some(opening_bracket_token) = self.brackets.get_token(
            opening_bracket.bracket_text.units(),
            &opening_bracket.language_id,
        ) else {
            return false;
        };
        if opening_bracket_token.kind != TokenKind::OpeningBracket {
            return false;
        }

        let node = self
            .ast_with_tokens
            .borrow()
            .clone()
            .expect("há uma árvore com tokens");
        has_unmatched_closing_bracket_after(
            &node,
            position_to_length(position),
            &opening_bracket_token,
        )
    }

    pub fn get_first_bracket_after(&self, position: Position) -> Option<FoundBracket> {
        self.flush_queue();

        let node = self.current_node();
        get_first_bracket_after(
            &node,
            LENGTH_ZERO,
            node.length(),
            position_to_length(position),
        )
    }

    pub fn get_first_bracket_before(&self, position: Position) -> Option<FoundBracket> {
        self.flush_queue();

        let node = self.current_node();
        get_first_bracket_before(
            &node,
            LENGTH_ZERO,
            node.length(),
            position_to_length(position),
        )
    }
}

impl IDisposable for BracketPairsTree {
    fn dispose(&self) {
        IDisposable::dispose(&*self.base);
    }
}

fn has_unmatched_closing_bracket_after(
    root: &AstNodeRef,
    position_length: Length,
    opening_bracket_token: &Token,
) -> bool {
    let opening_bracket_ids = &opening_bracket_token.bracket_ids;
    if !root
        .missing_opening_bracket_ids()
        .intersects(opening_bracket_ids)
    {
        return false;
    }

    let mut nodes_to_visit: Vec<(AstNodeRef, Length)> = vec![(root.clone(), LENGTH_ZERO)];

    while let Some((node, offset)) = nodes_to_visit.pop() {
        // Os nós UnexpectedClosingBracket representam fechamentos globalmente sem par.
        if node.kind() == AstNodeKind::UnexpectedClosingBracket {
            // Ignora os fechamentos sem par antes do cursor, porque a nova abertura não pode casar com eles.
            if length_greater_than_equal(offset, position_length) {
                return true;
            }
            continue;
        }

        let mut child_offset = offset;
        for child in node.children() {
            // child.length é o trecho de texto que o filho representa, não o número de descendentes
            let child_end_offset = length_add(child_offset, child.length());
            let child_ends_after_position =
                length_greater_than_equal(child_end_offset, position_length);
            let child_can_contain_matching_orphan = child
                .missing_opening_bracket_ids()
                .intersects(opening_bracket_ids);

            // Visita só os filhos depois do cursor que podem conter o órfão correspondente
            if child_ends_after_position && child_can_contain_matching_orphan {
                nodes_to_visit.push((child, child_offset));
            }
            child_offset = child_end_offset;
        }
    }

    false
}

fn found_bracket(
    node: &AstNode,
    node_offset_start: Length,
    node_offset_end: Length,
) -> Option<FoundBracket> {
    let bracket = node.as_bracket()?;
    Some(FoundBracket {
        range: lengths_to_range(node_offset_start, node_offset_end),
        bracket_info: (*bracket.bracket_info).clone(),
    })
}

fn get_first_bracket_before(
    node: &AstNodeRef,
    mut node_offset_start: Length,
    mut node_offset_end: Length,
    position: Length,
) -> Option<FoundBracket> {
    match node.kind() {
        AstNodeKind::List | AstNodeKind::Pair => {
            let children = node.children();
            let mut lengths: Vec<(Length, Length)> = Vec::with_capacity(children.len());
            for child in &children {
                node_offset_end = length_add(node_offset_start, child.length());
                lengths.push((node_offset_start, node_offset_end));
                node_offset_start = node_offset_end;
            }

            for i in (0..lengths.len()).rev() {
                let (start, end) = lengths[i];
                if length_less_than(start, position)
                    && let Some(result) =
                        get_first_bracket_before(&children[i], start, end, position)
                {
                    return Some(result);
                }
            }

            None
        }
        AstNodeKind::UnexpectedClosingBracket => None,
        AstNodeKind::Bracket => found_bracket(node, node_offset_start, node_offset_end),
        AstNodeKind::Text => None,
    }
}

fn get_first_bracket_after(
    node: &AstNodeRef,
    mut node_offset_start: Length,
    mut node_offset_end: Length,
    position: Length,
) -> Option<FoundBracket> {
    match node.kind() {
        AstNodeKind::List | AstNodeKind::Pair => {
            for child in node.children() {
                node_offset_end = length_add(node_offset_start, child.length());

                if length_less_than(position, node_offset_end)
                    && let Some(result) = get_first_bracket_after(
                        &child,
                        node_offset_start,
                        node_offset_end,
                        position,
                    )
                {
                    return Some(result);
                }

                node_offset_start = node_offset_end;
            }

            None
        }
        AstNodeKind::UnexpectedClosingBracket => None,
        AstNodeKind::Bracket => found_bracket(node, node_offset_start, node_offset_end),
        AstNodeKind::Text => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_brackets(
    mut node: AstNodeRef,
    mut node_offset_start: Length,
    mut node_offset_end: Length,
    start_offset: Length,
    end_offset: Length,
    push: &mut dyn FnMut(BracketInfo) -> bool,
    mut level: i64,
    mut nesting_level_of_equal_bracket_type: i64,
    level_per_bracket_type: &mut HashMap<JsString, i64>,
    only_colorized_brackets: bool,
    parent_pair_is_incomplete: bool,
) -> bool {
    if level > 200 {
        return true;
    }

    'while_loop: loop {
        match &*node.clone() {
            AstNode::List(list) => {
                let child_count = list.children_length();
                for i in 0..child_count {
                    let Some(child) = list.get_child(i) else {
                        continue;
                    };
                    node_offset_end = length_add(node_offset_start, child.length());
                    if length_less_than_equal(node_offset_start, end_offset)
                        && length_greater_than_equal(node_offset_end, start_offset)
                    {
                        let child_ends_after_end =
                            length_greater_than_equal(node_offset_end, end_offset);
                        if child_ends_after_end {
                            // Nenhum filho depois deste na janela pedida, não recursa
                            node = child;
                            continue 'while_loop;
                        }

                        let should_continue = collect_brackets(
                            child,
                            node_offset_start,
                            node_offset_end,
                            start_offset,
                            end_offset,
                            push,
                            level,
                            0,
                            level_per_bracket_type,
                            only_colorized_brackets,
                            false,
                        );
                        if !should_continue {
                            return false;
                        }
                    }
                    node_offset_start = node_offset_end;
                }
                return true;
            }
            AstNode::Pair(pair) => {
                let colorize = !only_colorized_brackets
                    || pair.closing_bracket.is_none()
                    || match (
                        &*pair.closing().expect("tem fechamento").bracket_info,
                        &*pair.opening().bracket_info,
                    ) {
                        (BracketKind::Closing(closing), BracketKind::Opening(opening)) => {
                            closing.closes_colorized(opening)
                        }
                        _ => false,
                    };

                let opening_text = pair.opening().text().clone();
                let existing = level_per_bracket_type
                    .get(&opening_text)
                    .copied()
                    .unwrap_or(0);
                let level_per_bracket = existing;
                if colorize {
                    level_per_bracket_type.insert(opening_text.clone(), existing + 1);
                }

                let child_count = 3;
                for i in 0..child_count {
                    let Some(child) = node.get_child(i) else {
                        continue;
                    };
                    node_offset_end = length_add(node_offset_start, child.length());
                    if length_less_than_equal(node_offset_start, end_offset)
                        && length_greater_than_equal(node_offset_end, start_offset)
                    {
                        let child_ends_after_end =
                            length_greater_than_equal(node_offset_end, end_offset);
                        if child_ends_after_end && child.kind() != AstNodeKind::Bracket {
                            // Nenhum filho depois deste na janela pedida, não recursa
                            // Não faz isto para colchetes, por causa dos não fechados/não abertos
                            node = child;
                            if colorize {
                                level += 1;
                                nesting_level_of_equal_bracket_type = level_per_bracket + 1;
                            } else {
                                nesting_level_of_equal_bracket_type = level_per_bracket;
                            }
                            continue 'while_loop;
                        }

                        if colorize
                            || child.kind() != AstNodeKind::Bracket
                            || pair.closing_bracket.is_none()
                        {
                            let should_continue = collect_brackets(
                                child,
                                node_offset_start,
                                node_offset_end,
                                start_offset,
                                end_offset,
                                push,
                                if colorize { level + 1 } else { level },
                                if colorize {
                                    level_per_bracket + 1
                                } else {
                                    level_per_bracket
                                },
                                level_per_bracket_type,
                                only_colorized_brackets,
                                pair.closing_bracket.is_none(),
                            );
                            if !should_continue {
                                return false;
                            }
                        }
                    }

                    node_offset_start = node_offset_end;
                }

                level_per_bracket_type.insert(opening_text, level_per_bracket);

                return true;
            }
            AstNode::UnexpectedClosingBracket(_) => {
                let range = lengths_to_range(node_offset_start, node_offset_end);
                return push(BracketInfo {
                    range,
                    nesting_level: level - 1,
                    nesting_level_of_equal_bracket_type: 0,
                    is_invalid: true,
                });
            }
            AstNode::Bracket(_) => {
                let range = lengths_to_range(node_offset_start, node_offset_end);
                return push(BracketInfo {
                    range,
                    nesting_level: level - 1,
                    nesting_level_of_equal_bracket_type: nesting_level_of_equal_bracket_type - 1,
                    is_invalid: parent_pair_is_incomplete,
                });
            }
            AstNode::Text(_) => return true,
        }
    }
}

struct CollectBracketPairsContext<'a> {
    push: &'a mut dyn FnMut(BracketPairWithMinIndentationInfo) -> bool,
    include_min_indentation: bool,
    text_model: &'a TextModel,
}

#[allow(clippy::too_many_arguments)]
fn collect_bracket_pairs(
    node: AstNodeRef,
    mut node_offset_start: Length,
    mut node_offset_end: Length,
    start_offset: Length,
    end_offset: Length,
    context: &mut CollectBracketPairsContext<'_>,
    level: i64,
    level_per_bracket_type: &mut HashMap<JsString, i64>,
) -> bool {
    if level > 200 {
        return true;
    }

    let mut should_continue = true;

    if let AstNode::Pair(pair) = &*node {
        let opening_text = pair.opening().text().clone();
        let existing = level_per_bracket_type
            .get(&opening_text)
            .copied()
            .unwrap_or(0);
        let level_per_bracket = existing;
        level_per_bracket_type.insert(opening_text.clone(), existing + 1);

        let opening_bracket_end = length_add(node_offset_start, pair.opening_bracket.length());
        let mut min_indentation = -1;
        if context.include_min_indentation {
            min_indentation = node.compute_min_indentation(node_offset_start, context.text_model);
        }

        should_continue = (context.push)(BracketPairWithMinIndentationInfo::new(
            lengths_to_range(node_offset_start, node_offset_end),
            lengths_to_range(node_offset_start, opening_bracket_end),
            pair.closing_bracket.as_ref().map(|_| {
                lengths_to_range(
                    length_add(
                        opening_bracket_end,
                        pair.child.as_ref().map_or(LENGTH_ZERO, |c| c.length()),
                    ),
                    node_offset_end,
                )
            }),
            level,
            level_per_bracket,
            node.clone(),
            min_indentation,
        ));

        node_offset_start = opening_bracket_end;
        if should_continue && let Some(child) = &pair.child {
            node_offset_end = length_add(node_offset_start, child.length());
            if length_less_than_equal(node_offset_start, end_offset)
                && length_greater_than_equal(node_offset_end, start_offset)
            {
                should_continue = collect_bracket_pairs(
                    child.clone(),
                    node_offset_start,
                    node_offset_end,
                    start_offset,
                    end_offset,
                    context,
                    level + 1,
                    level_per_bracket_type,
                );
                if !should_continue {
                    return false;
                }
            }
        }

        level_per_bracket_type.insert(opening_text, level_per_bracket);
    } else {
        let mut cur_offset = node_offset_start;
        for child in node.children() {
            let child_offset = cur_offset;
            cur_offset = length_add(cur_offset, child.length());

            if length_less_than_equal(child_offset, end_offset)
                && length_less_than_equal(start_offset, cur_offset)
            {
                should_continue = collect_bracket_pairs(
                    child,
                    child_offset,
                    cur_offset,
                    start_offset,
                    end_offset,
                    context,
                    level,
                    level_per_bracket_type,
                );
                if !should_continue {
                    return false;
                }
            }
        }
    }

    should_continue
}
