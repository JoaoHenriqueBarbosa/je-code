//! Porte de `.../bracketPairsTree/nodeReader.ts`: encontra com eficiência o maior filho num dado
//! deslocamento de um nó fixo. Os deslocamentos pedidos têm de crescer monotonicamente.

use je_base::errors::{self, Error};

use super::ast::AstNodeRef;
use super::length::{LENGTH_ZERO, Length, length_add, length_less_than};

/// `NodeReader`.
pub struct NodeReader {
    next_nodes: Vec<AstNodeRef>,
    offsets: Vec<Length>,
    idxs: Vec<usize>,
    last_offset: Length,
}

impl NodeReader {
    pub fn new(node: AstNodeRef) -> Self {
        Self {
            next_nodes: vec![node],
            offsets: vec![LENGTH_ZERO],
            idxs: Vec::new(),
            last_offset: LENGTH_ZERO,
        }
    }

    /// Devolve o maior nó em `offset` que satisfaz o predicado. `offset` tem de ser maior ou igual ao último
    /// deslocamento com que este método foi chamado!
    pub fn read_longest_node_at(
        &mut self,
        offset: Length,
        predicate: impl Fn(&AstNodeRef) -> bool,
    ) -> Option<AstNodeRef> {
        if length_less_than(offset, self.last_offset) {
            errors::throw(Error::new("Invalid offset"));
        }
        self.last_offset = offset;

        // Acha o maior de todos os nós mais próximos do deslocamento atual.
        loop {
            let cur_node = self.next_nodes.last()?.clone();

            let cur_node_offset = *self.offsets.last().expect("há um deslocamento por nó");

            if length_less_than(offset, cur_node_offset) {
                // O próximo melhor nó ainda não chegou.
                // O leitor tem de avançar antes de atingir um nó em cache.
                return None;
            }

            if length_less_than(cur_node_offset, offset) {
                // O leitor está à frente do nó atual.
                if length_add(cur_node_offset, cur_node.length()) <= offset {
                    // O leitor está depois do fim do nó atual.
                    self.next_node_after_current();
                } else {
                    // O leitor está em algum ponto do nó atual.
                    let next_child_idx = get_next_child_idx(&cur_node, None);
                    if let Some(next_child_idx) = next_child_idx {
                        // Vai para o primeiro filho e repete.
                        self.next_nodes
                            .push(cur_node.get_child(next_child_idx).expect("o filho existe"));
                        self.offsets.push(cur_node_offset);
                        self.idxs.push(next_child_idx);
                    } else {
                        // Não tem filhos
                        self.next_node_after_current();
                    }
                }
            } else {
                // readerOffsetBeforeChange === curNodeOffset
                if predicate(&cur_node) {
                    self.next_node_after_current();
                    return Some(cur_node);
                } else {
                    let next_child_idx = get_next_child_idx(&cur_node, None);
                    // procura um nó menor
                    match next_child_idx {
                        None => {
                            // Não há nó menor.
                            self.next_node_after_current();
                            return None;
                        }
                        Some(next_child_idx) => {
                            // Desce ao primeiro filho e repete.
                            self.next_nodes
                                .push(cur_node.get_child(next_child_idx).expect("o filho existe"));
                            self.offsets.push(cur_node_offset);
                            self.idxs.push(next_child_idx);
                        }
                    }
                }
            }
        }
    }

    // Navega até o maior nó que continua depois do nó atual.
    fn next_node_after_current(&mut self) {
        loop {
            let current_offset = self.offsets.last().copied();
            let current_node = self.next_nodes.last().cloned();

            self.next_nodes.pop();
            self.offsets.pop();

            let Some(&last_idx) = self.idxs.last() else {
                // Acabamos de tirar o nó raiz, não há próximo nó.
                break;
            };

            // O pai não é `undefined`, porque `idxs` não está vazio
            let parent = self.next_nodes.last().cloned().expect("há um pai");
            let next_child_idx = get_next_child_idx(&parent, Some(last_idx));
            if let Some(next_child_idx) = next_child_idx {
                self.next_nodes
                    .push(parent.get_child(next_child_idx).expect("o filho existe"));
                self.offsets.push(length_add(
                    current_offset.expect("há um deslocamento"),
                    current_node.expect("há um nó").length(),
                ));
                *self.idxs.last_mut().expect("idxs não está vazio") = next_child_idx;
                break;
            } else {
                self.idxs.pop();
            }
            // Consumimos o pai por completo.
            // O nó atual agora é o pai, então chame nextNodeAfterCurrent de novo
        }
    }
}

fn get_next_child_idx(node: &AstNodeRef, cur_idx: Option<usize>) -> Option<usize> {
    let mut cur_idx = cur_idx.map_or(-1_i64, |i| i as i64);
    loop {
        cur_idx += 1;
        if cur_idx >= node.children_length() as i64 {
            return None;
        }
        if node.get_child(cur_idx as usize).is_some() {
            return Some(cur_idx as usize);
        }
    }
}
