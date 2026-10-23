//! Os módulos da árvore de pares de colchetes (`bracketPairsTree/`).

pub mod ast;
pub mod before_edit_position_mapper;
#[allow(clippy::module_inception)]
pub mod bracket_pairs_tree;
pub mod brackets;
pub mod combine_text_edit_infos;
pub mod concat23_trees;
pub mod length;
pub mod node_reader;
pub mod parser;
pub mod small_immutable_set;
pub mod tokenizer;
