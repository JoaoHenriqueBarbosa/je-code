//! O `Lexer.lut.h` do C++: a `mainTable`, gerada pelo `create_hash_table` a partir do mesmo
//! `Keywords.table`. A disposição em baldes de hash do C++ não é observável; o que o Lexer usa é a
//! consulta da palavra para o token, que aqui é a busca na tabela de `keyword_lookup`.

use crate::parser::keyword_lookup::{Keyword, KEYWORDS};

/// `mainTable.entry(identifier)`: a linha do `Keywords.table` cuja palavra é `ident`.
pub fn main_table_entry<C: Copy + Into<u32>>(ident: &[C]) -> Option<&'static Keyword> {
    KEYWORDS.iter().find(|k| {
        k.word.len() == ident.len() && k.word.iter().zip(ident).all(|(&w, &c)| c.into() == w as u32)
    })
}
