//! Porte de `.../bracketPairsTree/parser.ts`: o analisador que constrói a árvore de colchetes a partir dos
//! tokens, reaproveitando os nós da árvore anterior onde o texto não mudou.
//!
//! As árvores construídas sem reaproveitamento são imutáveis.

use je_base::errors::{self, Error};

use super::ast::{
    AstNodeKind, AstNodeRef, InvalidBracketAstNode, ListAstNode, PairAstNode, TextAstNode,
};
use super::before_edit_position_mapper::{BeforeEditPositionMapper, TextEditInfo};
use super::concat23_trees::{concat23_trees, concat23_trees_of_same_height};
use super::length::{length_is_zero, length_less_than};
use super::node_reader::NodeReader;
use super::small_immutable_set::SmallImmutableSet;
use super::tokenizer::{TokenKind, Tokenizer};

/// `parseDocument(tokenizer, edits, oldNode, createImmutableLists)`. As árvores construídas sem
/// reaproveitamento são imutáveis.
pub fn parse_document(
    tokenizer: &mut dyn Tokenizer,
    edits: &[TextEditInfo],
    old_node: Option<AstNodeRef>,
    create_immutable_lists: bool,
) -> AstNodeRef {
    let mut parser = Parser::new(tokenizer, edits, old_node, create_immutable_lists);
    parser.parse_document()
}

struct Parser<'a> {
    tokenizer: &'a mut dyn Tokenizer,
    old_node_reader: Option<NodeReader>,
    position_mapper: BeforeEditPositionMapper,
    items_constructed: usize,
    items_from_cache: usize,
    create_immutable_lists: bool,
}

impl<'a> Parser<'a> {
    fn new(
        tokenizer: &'a mut dyn Tokenizer,
        edits: &[TextEditInfo],
        old_node: Option<AstNodeRef>,
        create_immutable_lists: bool,
    ) -> Self {
        if old_node.is_some() && create_immutable_lists {
            errors::throw(Error::new("Not supported"));
        }

        Self {
            tokenizer,
            old_node_reader: old_node.map(NodeReader::new),
            position_mapper: BeforeEditPositionMapper::new(edits),
            items_constructed: 0,
            items_from_cache: 0,
            create_immutable_lists,
        }
    }

    /// Quantos nós foram construídos na última operação de análise.
    #[allow(dead_code)]
    fn nodes_constructed(&self) -> usize {
        self.items_constructed
    }

    /// Quantos nós foram reaproveitados na última operação de análise.
    #[allow(dead_code)]
    fn nodes_reused(&self) -> usize {
        self.items_from_cache
    }

    fn parse_document(&mut self) -> AstNodeRef {
        self.items_constructed = 0;
        self.items_from_cache = 0;

        let result = self.parse_list(&SmallImmutableSet::get_empty(), 0);
        result.unwrap_or_else(ListAstNode::get_empty)
    }

    fn parse_list(
        &mut self,
        opened_bracket_ids: &SmallImmutableSet,
        level: u32,
    ) -> Option<AstNodeRef> {
        let mut items: Vec<AstNodeRef> = Vec::new();

        loop {
            let child = match self.try_read_child_from_cache(opened_bracket_ids) {
                Some(child) => child,
                None => {
                    let token = self.tokenizer.peek();
                    match token {
                        None => break,
                        Some(token)
                            if token.kind == TokenKind::ClosingBracket
                                && token.bracket_ids.intersects(opened_bracket_ids) =>
                        {
                            break;
                        }
                        Some(_) => {}
                    }
                    self.parse_child(opened_bracket_ids, level + 1)
                }
            };

            if child.kind() == AstNodeKind::List && child.children_length() == 0 {
                continue;
            }

            items.push(child);
        }

        // Quando não há `oldNodeReader`, todos os itens são criados do zero e têm a mesma altura.
        if self.old_node_reader.is_some() {
            concat23_trees(&mut items)
        } else {
            concat23_trees_of_same_height(&mut items, self.create_immutable_lists)
        }
    }

    fn try_read_child_from_cache(
        &mut self,
        opened_bracket_ids: &SmallImmutableSet,
    ) -> Option<AstNodeRef> {
        let old_node_reader = self.old_node_reader.as_mut()?;
        let max_cacheable_length = self
            .position_mapper
            .get_distance_to_next_change(self.tokenizer.offset());
        if max_cacheable_length.is_some_and(length_is_zero) {
            return None;
        }

        let offset_before_change = self
            .position_mapper
            .get_offset_before_change(self.tokenizer.offset());
        let cached_node = old_node_reader.read_longest_node_at(offset_before_change, |cur_node| {
            // A edição pode estender o token final, então não dá para reaproveitar nós que tocam a edição.
            // Se não há mais edição, podemos reaproveitar o nó de qualquer forma.
            if let Some(max_cacheable_length) = max_cacheable_length
                && !length_less_than(cur_node.length(), max_cacheable_length)
            {
                // O nó contém texto editado ou toca texto editado.
                // No segundo caso, os colchetes podem ter sido estendidos (`end` -> `ending`), então nem os nós
                // que apenas tocam podem ser reaproveitados.
                return false;
            }
            cur_node.can_be_reused(opened_bracket_ids)
        });

        let cached_node = cached_node?;
        self.items_from_cache += 1;
        self.tokenizer.skip(cached_node.length());
        Some(cached_node)
    }

    fn parse_child(&mut self, opened_bracket_ids: &SmallImmutableSet, level: u32) -> AstNodeRef {
        self.items_constructed += 1;

        let token = self.tokenizer.read().expect("há um token para ler");

        match token.kind {
            TokenKind::ClosingBracket => {
                InvalidBracketAstNode::create(token.bracket_ids.clone(), token.length)
            }

            TokenKind::Text => token.ast_node.clone().expect("um token de texto tem nó"),

            TokenKind::OpeningBracket => {
                if level > 300 {
                    // Para evitar estouro de pilha
                    return TextAstNode::create(token.length);
                }

                let set = opened_bracket_ids.merge(&token.bracket_ids);
                let child = self.parse_list(&set, level + 1);

                let next_token = self.tokenizer.peek();
                if let Some(next_token) = next_token
                    && next_token.kind == TokenKind::ClosingBracket
                    && (next_token.bracket_id == token.bracket_id
                        || next_token.bracket_ids.intersects(&token.bracket_ids))
                {
                    self.tokenizer.read();
                    PairAstNode::create(
                        token.ast_node.clone().expect("a abertura tem nó"),
                        child,
                        Some(next_token.ast_node.clone().expect("o fechamento tem nó")),
                    )
                } else {
                    PairAstNode::create(
                        token.ast_node.clone().expect("a abertura tem nó"),
                        child,
                        None,
                    )
                }
            }
        }
    }
}
