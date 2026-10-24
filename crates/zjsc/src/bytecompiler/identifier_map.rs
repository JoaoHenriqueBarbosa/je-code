//! `IdentifierMap` e `BorrowedIdentifierMap` de `runtime/Identifier.h` (as duas `typedef`s do fim do
//! header), que o `BytecodeGenerator` usa em `m_identifierMap`.
//!
//! No C++ a chave é `RefPtr<UniquedStringImpl>` com `IdentifierRepHash` (hash e igualdade pela
//! identidade do ponteiro, nulo permitido) e o valor é `int`, com `IdentifierMapIndexHashTraits`
//! (`emptyValue()` é `INT_MAX`, só o marcador de bucket vazio da tabela aberta). `UniquedKey` já
//! tem hash e igualdade pela identidade do `Rc<StringImpl>`, o `Option` leva o nulo do `RefPtr`, e o
//! `HashMap` do Rust dispensa o marcador de bucket vazio.
//!
//! O valor é `u32`: o índice guardado é sempre `m_codeBlock->numberOfIdentifiers()`, nunca negativo.
//! O `UncheckedKeyHashMap` do C++ só difere do `HashMap` da WTF na checagem de iteração, que não
//! existe aqui.

use std::collections::HashMap;

use crate::wtf::text::string_impl::UniquedKey;

/// `IdentifierMap`.
pub type IdentifierMap = HashMap<Option<UniquedKey>, u32>;

/// `BorrowedIdentifierMap`: o C++ guarda `UniquedStringImpl*` sem contar referência. O porte não
/// tem ponteiro cru, então a chave é a mesma de `IdentifierMap` e quem usa mantém o `Identifier`
/// vivo pelo tempo do mapa.
pub type BorrowedIdentifierMap = IdentifierMap;
