//! Porte de `src/vs/base/common/iterator.ts` (o `namespace Iterable`).
//!
//! Um `Iterable<T>` do JavaScript é `IntoIterator` em Rust, e o `Iterator` do std já faz o que
//! várias funções do original fazem. O que NÃO está aqui, e o equivalente:
//! - `is`: não existe (o tipo diz se é iterável);
//! - `empty`/`single`/`from`/`reverse`: `std::iter::empty`, `std::iter::once`,
//!   `Option::into_iter().flatten()`, `slice.iter().rev()`;
//! - `filter` (sem índice), `reduce` e `length`: `Iterator::filter`, `fold` e `count`;
//! - `asyncToArray` e `asyncToArrayFlat`: dependem de `AsyncIterable`/`Promise.all`, pendentes.
//!
//! As funções portadas são as que têm semântica própria: o índice passado ao predicado, o resultado
//! para "vazio ou ausente", a divisão de `consume` e as regras de `slice`. Predicados que no original
//! devolvem `unknown` aceitam qualquer [`Truthy`]. `concat` recebe cada argumento como [`OneOrMany`];
//! a `string` solta do JavaScript, que ali seria percorrida caractere a caractere por ser iterável,
//! não é tratada (passe os caracteres).

use std::iter::Fuse;

use crate::arrays::OneOrMany;
use crate::dynamic::Truthy;

/// Itera sobre o único elemento ou sobre os elementos.
pub fn wrap<T>(iterable_or_element: OneOrMany<T>) -> std::vec::IntoIter<T> {
    match iterable_or_element {
        OneOrMany::One(element) => vec![element].into_iter(),
        OneOrMany::Many(elements) => elements.into_iter(),
    }
}

/// Verdadeiro se ausente (`None`) ou sem elementos.
pub fn is_empty<I: IntoIterator>(iterable: Option<I>) -> bool {
    iterable.is_none_or(|i| i.into_iter().next().is_none())
}

/// O primeiro elemento, se houver.
pub fn first<I: IntoIterator>(iterable: I) -> Option<I::Item> {
    iterable.into_iter().next()
}

/// Algum elemento satisfaz o predicado (que recebe também o índice)? Para no primeiro que satisfaz.
pub fn some<I: IntoIterator, R: Truthy>(iterable: I, mut predicate: impl FnMut(I::Item, usize) -> R) -> bool {
    iterable.into_iter().enumerate().any(|(i, t)| predicate(t, i).is_truthy())
}

/// Todos os elementos satisfazem o predicado? Vale para vazio. Para no primeiro que falha.
pub fn every<I: IntoIterator, R: Truthy>(iterable: I, mut predicate: impl FnMut(I::Item, usize) -> R) -> bool {
    iterable.into_iter().enumerate().all(|(i, t)| predicate(t, i).is_truthy())
}

/// O primeiro elemento que satisfaz o predicado.
pub fn find<I: IntoIterator>(iterable: I, predicate: impl Fn(&I::Item) -> bool) -> Option<I::Item> {
    iterable.into_iter().find(|t| predicate(t))
}

/// `map` preguiçoso cujo callback recebe também o índice.
pub fn map<I: IntoIterator, R>(iterable: I, mut f: impl FnMut(I::Item, usize) -> R) -> impl Iterator<Item = R> {
    iterable.into_iter().enumerate().map(move |(i, t)| f(t, i))
}

/// `flatMap` preguiçoso cujo callback recebe também o índice.
pub fn flat_map<I: IntoIterator, J: IntoIterator>(iterable: I, mut f: impl FnMut(I::Item, usize) -> J) -> impl Iterator<Item = J::Item> {
    iterable.into_iter().enumerate().flat_map(move |(i, t)| f(t, i))
}

/// Concatena elementos soltos e coleções, nesta ordem.
pub fn concat<T>(iterables: Vec<OneOrMany<T>>) -> impl Iterator<Item = T> {
    iterables.into_iter().flat_map(wrap)
}

/// Fatia do array com a semântica de `array.slice(from, to)`, preguiçosa.
pub fn slice<T>(arr: &[T], from: i64, to: Option<i64>) -> impl Iterator<Item = &T> {
    let len = arr.len() as i64;
    let mut from = from;
    if from < -len {
        from = 0;
    }
    if from < 0 {
        from += len;
    }
    let mut to = to.unwrap_or(len);
    if to < 0 {
        to += len;
    } else if to > len {
        to = len;
    }
    let from = from.min(len).max(0) as usize;
    let to = to.max(0) as usize;
    arr.iter().take(to).skip(from)
}

/// Consome até `at_most` elementos (`None` = todos) e devolve os consumidos e um iterador para o resto.
pub fn consume<I: IntoIterator>(iterable: I, at_most: Option<usize>) -> (Vec<I::Item>, Fuse<I::IntoIter>) {
    let mut iterator = iterable.into_iter().fuse();
    let mut consumed = Vec::new();
    for _ in 0..at_most.unwrap_or(usize::MAX) {
        match iterator.next() {
            Some(item) => consumed.push(item),
            None => break,
        }
    }
    (consumed, iterator)
}
