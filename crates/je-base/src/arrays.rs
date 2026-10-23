//! Porte de `src/vs/base/common/arrays.ts`.
//!
//! Representação:
//! - `T[]` é `&[T]`/`Vec<T>`; `T | undefined` é `Option<T>`; os comparadores devolvem `f64`
//!   ([`CompareResult`], um `number` do JavaScript), com [`to_ordering`] para ordenar com o `sort_by`
//!   do Rust (`NaN` conta como empate, como no `Array.prototype.sort`). O `sort` do Rust é estável,
//!   como o do JS, mas um comparador que não seja uma ordem total pode derrubá-lo com pânico, ao
//!   contrário do V8.
//! - Onde o TypeScript devolve ou recebe elementos por referência (`T`), as funções que guardam o
//!   elemento em outra coleção pedem `T: Clone`.
//! - Chave de `distinct`/`uniqueFilter`/`withoutDuplicates` é um `K: Hash + Eq` (o `Set` do JS usa
//!   `SameValueZero`; `f64` não é `Hash`, quem precisar usa `to_bits` ou o texto do número).
//! - `T | T[]` é [`OneOrMany`]; `coalesce` e `coalesceInPlace` descartam o que é falso no sentido do
//!   JavaScript (`None`, e também `0`, `""`, `false`, `NaN` quando `T` tem esses valores), via
//!   [`Truthy`]; `coalesceInPlace` não pode mudar o tipo do vetor, então continua `Vec<Option<T>>`
//!   só com `Some`.
//! - `ISplice` (de `sequence.ts`, ainda não portado) é [`Splice`], definido aqui até `sequence.rs`
//!   existir.
//! - `index` devolve um `HashMap` (o objeto-dicionário do JS preservaria a ordem de inserção).
//! - `shuffle` com semente usa `f64::sin` do Rust, que pode diferir do `Math.sin` do V8 na última
//!   casa; só afeta qual permutação sai, não a validade dela.
//!
//! Pendente (dependem de módulos que ainda não existem): `topAsync` (precisa de `async.ts` e de
//! `cancellation.rs`) e `findAsync` (precisa de `Promise.all`).

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use crate::arrays_find::find_first_idx_monotonous_or_arr_len;
use crate::dynamic::Truthy;
use crate::errors::{self, Error};
use crate::js_string::JsString;
use crate::random::random_f64;

/// Um `T | T[]`.
#[derive(Clone, Debug, PartialEq)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

/// Descreve uma troca num array (`ISplice`).
#[derive(Clone, Debug, PartialEq)]
pub struct Splice<T> {
    pub start: usize,
    pub delete_count: usize,
    pub to_insert: Vec<T>,
}

/// Menos que zero: o primeiro é menor; mais que zero: é maior; zero: nenhum dos dois.
pub type CompareResult = f64;

/// `CompareResult` (o `namespace` do original).
pub mod compare_result {
    use super::CompareResult;

    pub fn is_less_than(result: CompareResult) -> bool {
        result < 0.0
    }

    pub fn is_less_than_or_equal(result: CompareResult) -> bool {
        result <= 0.0
    }

    pub fn is_greater_than(result: CompareResult) -> bool {
        result > 0.0
    }

    pub fn is_neither_less_or_greater_than(result: CompareResult) -> bool {
        result == 0.0
    }

    pub const GREATER_THAN: CompareResult = 1.0;
    pub const LESS_THAN: CompareResult = -1.0;
    pub const NEITHER_LESS_OR_GREATER_THAN: CompareResult = 0.0;
}

/// `Comparator<T>` do original, em caixa (para guardar vários, como em [`tie_break_comparators`]).
pub type Comparator<T> = Box<dyn Fn(&T, &T) -> CompareResult>;

/// Converte o resultado de um comparador do JS no `Ordering` do Rust (`NaN` vira `Equal`).
pub fn to_ordering(result: CompareResult) -> Ordering {
    if result < 0.0 {
        Ordering::Less
    } else if result > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// `array.sort(compare)`.
pub fn sort_by_compare<T>(array: &mut [T], compare: impl Fn(&T, &T) -> CompareResult) {
    array.sort_by(|a, b| to_ordering(compare(a, b)));
}

/// Os índices de `slice(start, end)` do JavaScript (negativos contam do fim), já limitados.
fn slice_bounds(len: usize, start: i64, end: Option<i64>) -> (usize, usize) {
    let len_i = len as i64;
    let resolve = |i: i64| if i < 0 { (len_i + i).max(0) } else { i.min(len_i) } as usize;
    let from = resolve(start);
    let to = end.map_or(len, resolve);
    (from, to.max(from))
}

/// Devolve o último elemento e os N-1 iniciais, `[rest, last]`. Lança se o array estiver vazio.
pub fn tail<T: Clone>(arr: &[T]) -> (Vec<T>, T) {
    match arr.split_last() {
        Some((last, rest)) => (rest.to_vec(), last.clone()),
        None => errors::throw(Error::new("Invalid tail call")),
    }
}

/// `equals(one, other, itemEquals)`: `None` é `undefined`; o mesmo slice (mesmo endereço e
/// tamanho) é igual sem olhar os itens, como `one === other`.
pub fn equals<T>(one: Option<&[T]>, other: Option<&[T]>, item_equals: impl Fn(&T, &T) -> bool) -> bool {
    match (one, other) {
        (Some(a), Some(b)) => {
            if std::ptr::eq(a, b) {
                return true;
            }
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| item_equals(x, y))
        }
        (None, None) => true,
        _ => false,
    }
}

/// Remove o elemento em `index` trocando-o pelo último: mais rápido que `splice`, mas muda a ordem.
pub fn remove_fast_without_keeping_order<T>(array: &mut Vec<T>, index: usize) {
    let last = array.len().wrapping_sub(1);
    if index < last {
        array.swap_remove(index);
    } else {
        array.pop();
    }
}

/// Busca binária num array ordenado. Devolve o índice (>= 0) se achou, senão `-(n + 1)` com `n`
/// a posição de inserção (o `~n` do original).
pub fn binary_search<T>(array: &[T], key: &T, comparator: impl Fn(&T, &T) -> CompareResult) -> i64 {
    binary_search2(array.len(), |i| comparator(&array[i], key))
}

/// Busca binária sobre uma coleção qualquer de `length` elementos; `compare_to_key(i)` diz se o
/// valor em `i` precede (< 0), iguala (0) ou sucede (> 0) a chave. Mesmo resultado de [`binary_search`].
pub fn binary_search2(length: usize, mut compare_to_key: impl FnMut(usize) -> CompareResult) -> i64 {
    let mut low: i64 = 0;
    let mut high: i64 = length as i64 - 1;
    while low <= high {
        let mid = (low + high) / 2;
        let comp = compare_to_key(mid as usize);
        if comp < 0.0 {
            low = mid + 1;
        } else if comp > 0.0 {
            high = mid - 1;
        } else {
            return mid;
        }
    }
    -(low + 1)
}

/// O `nth` menor elemento (começando em 0) pelo quickselect; `data` não precisa estar ordenado.
/// Lança `TypeError` se `nth >= data.len()`.
pub fn quick_select<T: Clone>(nth: usize, data: &[T], compare: impl Fn(&T, &T) -> CompareResult) -> T {
    quick_select_step(nth, data, &compare)
}

fn quick_select_step<T: Clone>(nth: usize, data: &[T], compare: &impl Fn(&T, &T) -> CompareResult) -> T {
    if nth >= data.len() {
        errors::throw(Error::type_error("invalid index"));
    }
    let pivot_value = &data[(data.len() as f64 * random_f64()).floor() as usize];
    let mut lower = Vec::new();
    let mut higher = Vec::new();
    let mut pivots = Vec::new();
    for value in data {
        let val = compare(value, pivot_value);
        if val < 0.0 {
            lower.push(value.clone());
        } else if val > 0.0 {
            higher.push(value.clone());
        } else {
            pivots.push(value.clone());
        }
    }
    if nth < lower.len() {
        quick_select_step(nth, &lower, compare)
    } else if nth < lower.len() + pivots.len() {
        pivots.swap_remove(0)
    } else {
        quick_select_step(nth - (lower.len() + pivots.len()), &higher, compare)
    }
}

/// Ordena uma cópia e agrupa os elementos que o comparador diz iguais.
pub fn group_by<T: Clone>(data: &[T], compare: impl Fn(&T, &T) -> CompareResult) -> Vec<Vec<T>> {
    let mut sorted = data.to_vec();
    sort_by_compare(&mut sorted, &compare);
    let mut result: Vec<Vec<T>> = Vec::new();
    for element in sorted {
        match result.last_mut() {
            Some(group) if compare(&group[0], &element) == 0.0 => group.push(element),
            _ => result.push(vec![element]),
        }
    }
    result
}

/// O gerador de [`group_adjacent_by`].
pub struct GroupAdjacentBy<I: Iterator, F> {
    iter: I,
    should_be_grouped: F,
    current: Option<Vec<I::Item>>,
    last: Option<I::Item>,
}

impl<I, F> Iterator for GroupAdjacentBy<I, F>
where
    I: Iterator,
    I::Item: Clone,
    F: FnMut(&I::Item, &I::Item) -> bool,
{
    type Item = Vec<I::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.iter.next() {
                Some(item) => {
                    let same = self.last.as_ref().is_some_and(|last| (self.should_be_grouped)(last, &item));
                    self.last = Some(item.clone());
                    if same {
                        self.current.as_mut().expect("grupo aberto").push(item);
                    } else if let Some(done) = self.current.replace(vec![item]) {
                        return Some(done);
                    }
                }
                None => return self.current.take(),
            }
        }
    }
}

/// Divide os itens em grupos (não vazios); `should_be_grouped` decide se dois itens consecutivos
/// ficam no mesmo grupo. A ordem é preservada e a produção é preguiçosa.
pub fn group_adjacent_by<I, F>(items: I, should_be_grouped: F) -> GroupAdjacentBy<I::IntoIter, F>
where
    I: IntoIterator,
    I::Item: Clone,
    F: FnMut(&I::Item, &I::Item) -> bool,
{
    GroupAdjacentBy { iter: items.into_iter(), should_be_grouped, current: None, last: None }
}

/// Chama `f(anterior, atual)` para cada par adjacente, incluindo `(None, primeiro)` e `(último, None)`.
pub fn for_each_adjacent<T>(arr: &[T], mut f: impl FnMut(Option<&T>, Option<&T>)) {
    for i in 0..=arr.len() {
        f(if i == 0 { None } else { Some(&arr[i - 1]) }, arr.get(i));
    }
}

/// Chama `f(antes, elemento, depois)` para cada elemento.
pub fn for_each_with_neighbors<T>(arr: &[T], mut f: impl FnMut(Option<&T>, &T, Option<&T>)) {
    for i in 0..arr.len() {
        f(if i == 0 { None } else { Some(&arr[i - 1]) }, &arr[i], arr.get(i + 1));
    }
}

/// `[].concat(...arrays)` para arrays de elementos que não são eles mesmos arrays.
pub fn concat_arrays<T: Clone>(arrays: &[&[T]]) -> Vec<T> {
    arrays.iter().flat_map(|a| a.iter().cloned()).collect()
}

/// Diferença de dois arrays *ordenados*, como as trocas que aplicam o diff.
pub fn sorted_diff<T: Clone>(before: &[T], after: &[T], compare: impl Fn(&T, &T) -> CompareResult) -> Vec<Splice<T>> {
    let mut result: Vec<Splice<T>> = Vec::new();

    fn push_splice<T: Clone>(result: &mut Vec<Splice<T>>, start: usize, delete_count: usize, to_insert: &[T]) {
        if delete_count == 0 && to_insert.is_empty() {
            return;
        }
        match result.last_mut() {
            Some(latest) if latest.start + latest.delete_count == start => {
                latest.delete_count += delete_count;
                latest.to_insert.extend_from_slice(to_insert);
            }
            _ => result.push(Splice { start, delete_count, to_insert: to_insert.to_vec() }),
        }
    }

    let mut before_idx = 0;
    let mut after_idx = 0;
    loop {
        if before_idx == before.len() {
            push_splice(&mut result, before_idx, 0, &after[after_idx..]);
            break;
        }
        if after_idx == after.len() {
            push_splice(&mut result, before_idx, before.len() - before_idx, &[]);
            break;
        }
        let n = compare(&before[before_idx], &after[after_idx]);
        if n == 0.0 {
            before_idx += 1;
            after_idx += 1;
        } else if n < 0.0 {
            // O elemento de `before` é menor: foi removido.
            push_splice(&mut result, before_idx, 1, &[]);
            before_idx += 1;
        } else if n > 0.0 {
            // O elemento de `before` é maior: o de `after` foi adicionado.
            push_splice(&mut result, before_idx, 0, std::slice::from_ref(&after[after_idx]));
            after_idx += 1;
        }
    }
    result
}

/// O que saiu e o que entrou entre dois arrays *ordenados*.
#[derive(Clone, Debug, PartialEq)]
pub struct Delta<T> {
    pub removed: Vec<T>,
    pub added: Vec<T>,
}

/// Calcula o delta de dois arrays *ordenados* em no máximo `min(before.len(), after.len())` passos.
pub fn delta<T: Clone>(before: &[T], after: &[T], compare: impl Fn(&T, &T) -> CompareResult) -> Delta<T> {
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for splice in sorted_diff(before, after, compare) {
        removed.extend_from_slice(&before[splice.start..splice.start + splice.delete_count]);
        added.extend(splice.to_insert);
    }
    Delta { removed, added }
}

/// Os `n` primeiros elementos do array quando ordenado com `compare`; mais rápido que ordenar tudo
/// quando o array é bem maior que `n`.
pub fn top<T: Clone>(array: &[T], compare: impl Fn(&T, &T) -> CompareResult, n: usize) -> Vec<T> {
    if n == 0 {
        return Vec::new();
    }
    let mut result = array[..n.min(array.len())].to_vec();
    sort_by_compare(&mut result, &compare);
    top_step(array, &compare, &mut result, n, array.len());
    result
}

fn top_step<T: Clone>(array: &[T], compare: &impl Fn(&T, &T) -> CompareResult, result: &mut Vec<T>, from: usize, to: usize) {
    let n = result.len();
    for element in array.iter().take(to).skip(from) {
        if compare(element, &result[n - 1]) < 0.0 {
            result.pop();
            let j = find_first_idx_monotonous_or_arr_len(result, |e| compare(element, e) < 0.0, 0, result.len());
            result.insert(j, element.clone());
        }
    }
}

/// Novo array sem os valores falsos (`None`, e `0`, `""`, `false`, `NaN` quando `T` os tem).
pub fn coalesce<T: Truthy + Clone>(array: &[Option<T>]) -> Vec<T> {
    array.iter().filter(|e| e.is_truthy()).filter_map(Clone::clone).collect()
}

/// Remove os valores falsos de `array`, no lugar.
pub fn coalesce_in_place<T: Truthy>(array: &mut Vec<Option<T>>) {
    array.retain(Truthy::is_truthy);
}

/// `array.splice(to, 0, array.splice(from, 1)[0])` (obsoleto no original).
pub fn r#move<T>(array: &mut Vec<T>, from: usize, to: usize) {
    if from < array.len() {
        let item = array.remove(from);
        array.insert(to.min(array.len()), item);
    }
}

/// `!Array.isArray(obj) || obj.length === 0`, com `None` no lugar do que não é array.
pub fn is_falsy_or_empty<T>(obj: Option<&[T]>) -> bool {
    obj.is_none_or(<[T]>::is_empty)
}

/// Verdadeiro se for um array com pelo menos um elemento.
pub fn is_non_empty_array<T>(obj: Option<&[T]>) -> bool {
    obj.is_some_and(|a| !a.is_empty())
}

/// Tira os duplicados mantendo o primeiro de cada chave.
pub fn distinct<T: Clone, K: Hash + Eq>(array: &[T], key_fn: impl Fn(&T) -> K) -> Vec<T> {
    let mut filter = unique_filter(key_fn);
    array.iter().filter(|e| filter(e)).cloned().collect()
}

/// Um filtro que deixa passar só o primeiro elemento de cada chave.
pub fn unique_filter<T, K: Hash + Eq>(key_fn: impl Fn(&T) -> K) -> impl FnMut(&T) -> bool {
    let mut seen = HashSet::new();
    move |element| seen.insert(key_fn(element))
}

/// Tamanho do prefixo comum, com a igualdade dada.
pub fn common_prefix_length<T>(one: &[T], other: &[T], equals: impl Fn(&T, &T) -> bool) -> usize {
    one.iter().zip(other).take_while(|(a, b)| equals(a, b)).count()
}

/// `range(to)`: `0..to`, ou `0, -1, ...` descendo até (sem incluir) `to` se `to` é negativo.
pub fn range(to: i64) -> Vec<i64> {
    range_from(0, to)
}

/// `range(from, to)`: de `from` até `to` (exclusivo), subindo ou descendo.
pub fn range_from(from: i64, to: i64) -> Vec<i64> {
    if from <= to { (from..to).collect() } else { (to + 1..=from).rev().collect() }
}

/// `index(array, indexer, mapper)`: dicionário da chave de cada elemento para o valor mapeado;
/// chave repetida fica com o último.
pub fn index<T, R>(array: &[T], indexer: impl Fn(&T) -> JsString, mapper: impl Fn(&T) -> R) -> HashMap<JsString, R> {
    array.iter().map(|t| (indexer(t), mapper(t))).collect()
}

/// Insere `element` no fim. Devolve a função que o remove de novo (a primeira ocorrência igual,
/// se ainda houver). Obsoleto no original: na maioria dos casos use um `HashSet`.
pub fn insert<T: PartialEq + Clone + 'static>(array: &Rc<RefCell<Vec<T>>>, element: T) -> impl Fn() {
    array.borrow_mut().push(element.clone());
    let array = array.clone();
    move || {
        remove(&mut array.borrow_mut(), &element);
    }
}

/// Remove a primeira ocorrência de `element`, se existir, e a devolve.
pub fn remove<T: PartialEq>(array: &mut Vec<T>, element: &T) -> Option<T> {
    array.iter().position(|e| e == element).map(|i| array.remove(i))
}

/// Insere `insert_arr` em `target` na posição `insert_index` (índice como o de `slice`), num novo array.
pub fn array_insert<T: Clone>(target: &[T], insert_index: i64, insert_arr: &[T]) -> Vec<T> {
    let (split, _) = slice_bounds(target.len(), insert_index, None);
    let mut result = Vec::with_capacity(target.len() + insert_arr.len());
    result.extend_from_slice(&target[..split]);
    result.extend_from_slice(insert_arr);
    result.extend_from_slice(&target[split..]);
    result
}

/// Embaralha com Fisher-Yates. Com `seed` o resultado é determinístico (o gerador do original,
/// baseado em `sin`).
pub fn shuffle<T>(array: &mut [T], seed: Option<f64>) {
    let mut seed = seed;
    let mut rand = move || match seed.as_mut() {
        Some(s) => {
            let x = s.sin() * 179_426_549.0;
            *s += 1.0;
            x - x.floor()
        }
        None => random_f64(),
    };
    for i in (1..array.len()).rev() {
        let j = (rand() * (i + 1) as f64).floor() as usize;
        array.swap(i, j);
    }
}

/// Põe `value` no início do array, se estiver nele.
pub fn push_to_start<T: PartialEq>(arr: &mut Vec<T>, value: &T) {
    if let Some(index) = arr.iter().position(|e| e == value) {
        let item = arr.remove(index);
        arr.insert(0, item);
    }
}

/// Põe `value` no fim do array, se estiver nele.
pub fn push_to_end<T: PartialEq>(arr: &mut Vec<T>, value: &T) {
    if let Some(index) = arr.iter().position(|e| e == value) {
        let item = arr.remove(index);
        arr.push(item);
    }
}

pub fn push_many<T: Clone>(arr: &mut Vec<T>, items: &[T]) {
    arr.extend_from_slice(items);
}

/// Aplica `f` ao item ou a cada item.
pub fn map_array_or_not<T, U>(items: OneOrMany<T>, f: impl Fn(T) -> U) -> OneOrMany<U> {
    match items {
        OneOrMany::One(item) => OneOrMany::One(f(item)),
        OneOrMany::Many(items) => OneOrMany::Many(items.into_iter().map(f).collect()),
    }
}

/// `map` que descarta o que `f` devolve como `None` (o `undefined` do original).
pub fn map_filter<T, U>(array: &[T], f: impl Fn(&T) -> Option<U>) -> Vec<U> {
    array.iter().filter_map(f).collect()
}

/// Os elementos sem repetição, na ordem da primeira ocorrência (`Array.from(new Set(array))`).
pub fn without_duplicates<T: Clone + Hash + Eq>(array: &[T]) -> Vec<T> {
    distinct(array, T::clone)
}

/// `asArray(x)`: o item vira um array de um elemento.
pub fn as_array<T>(x: OneOrMany<T>) -> Vec<T> {
    match x {
        OneOrMany::One(item) => vec![item],
        OneOrMany::Many(items) => items,
    }
}

/// Um elemento aleatório, `None` se o array é vazio.
pub fn get_random_element<T>(arr: &[T]) -> Option<&T> {
    arr.get((random_f64() * arr.len() as f64).floor() as usize)
}

/// O índice inicial efetivo, com a regra do `splice`/`slice` nativos: acima do tamanho vale o
/// tamanho; negativo conta do fim, com piso em 0.
fn actual_start_index(len: usize, start: i64) -> usize {
    if start < 0 { (start + len as i64).max(0) as usize } else { (start as usize).min(len) }
}

/// Insere `new_items` a partir de `start`.
pub fn insert_into<T: Clone>(array: &mut Vec<T>, start: i64, new_items: &[T]) {
    let start_idx = actual_start_index(array.len(), start);
    array.splice(start_idx..start_idx, new_items.iter().cloned());
}

/// Remove `delete_count` elementos a partir de `start` e insere `new_items` no lugar, devolvendo
/// os removidos. Alternativa ao `splice` nativo que não estoura a pilha com muitos itens.
pub fn splice<T: Clone>(array: &mut Vec<T>, start: i64, delete_count: usize, new_items: &[T]) -> Vec<T> {
    let index = actual_start_index(array.len(), start);
    let end = index.saturating_add(delete_count).min(array.len());
    array.splice(index..end, new_items.iter().cloned()).collect()
}

/// Comparador que olha só o que `selector` extrai.
pub fn compare_by<A, B>(selector: impl Fn(&A) -> B, comparator: impl Fn(&B, &B) -> CompareResult) -> impl Fn(&A, &A) -> CompareResult {
    move |a, b| comparator(&selector(a), &selector(b))
}

/// Usa os comparadores em ordem até um deles desempatar.
pub fn tie_break_comparators<T>(comparators: Vec<Comparator<T>>) -> impl Fn(&T, &T) -> CompareResult {
    move |item1, item2| {
        for comparator in &comparators {
            let result = comparator(item1, item2);
            if !compare_result::is_neither_less_or_greater_than(result) {
                return result;
            }
        }
        compare_result::NEITHER_LESS_OR_GREATER_THAN
    }
}

/// A ordem natural dos números.
pub fn number_comparator(a: &f64, b: &f64) -> CompareResult {
    a - b
}

pub fn boolean_comparator(a: &bool, b: &bool) -> CompareResult {
    number_comparator(&f64::from(u8::from(*a)), &f64::from(u8::from(*b)))
}

pub fn reverse_order<T>(comparator: impl Fn(&T, &T) -> CompareResult) -> impl Fn(&T, &T) -> CompareResult {
    move |a, b| -comparator(a, b)
}

/// Comparador que trata `None` (o `undefined`) como o menor valor.
pub fn compare_undefined_smallest<T>(comparator: impl Fn(&T, &T) -> CompareResult) -> impl Fn(&Option<T>, &Option<T>) -> CompareResult {
    move |a, b| match (a, b) {
        (None, None) => compare_result::NEITHER_LESS_OR_GREATER_THAN,
        (None, Some(_)) => compare_result::LESS_THAN,
        (Some(_), None) => compare_result::GREATER_THAN,
        (Some(a), Some(b)) => comparator(a, b),
    }
}

/// Fila apoiada num slice, que se consome pelas duas pontas.
pub struct ArrayQueue<'a, T> {
    items: &'a [T],
    first_idx: i64,
    last_idx: i64,
}

impl<'a, T: Clone> ArrayQueue<'a, T> {
    /// Fila apoiada no slice dado, em O(1).
    pub fn new(items: &'a [T]) -> Self {
        Self { items, first_idx: 0, last_idx: items.len() as i64 - 1 }
    }

    pub fn length(&self) -> i64 {
        self.last_idx - self.first_idx + 1
    }

    fn at(&self, index: i64) -> Option<&T> {
        usize::try_from(index).ok().and_then(|i| self.items.get(i))
    }

    /// Consome do início enquanto `predicate` valer; `None` se nada foi consumido.
    pub fn take_while(&mut self, predicate: impl Fn(&T) -> bool) -> Option<Vec<T>> {
        let mut start_idx = self.first_idx;
        while start_idx <= self.last_idx && predicate(&self.items[start_idx as usize]) {
            start_idx += 1;
        }
        let result = (start_idx != self.first_idx).then(|| self.items[self.first_idx as usize..start_idx as usize].to_vec());
        self.first_idx = start_idx;
        result
    }

    /// Consome do fim enquanto `predicate` valer; `None` se nada foi consumido. O resultado tem a
    /// mesma ordem do array de base.
    pub fn take_from_end_while(&mut self, predicate: impl Fn(&T) -> bool) -> Option<Vec<T>> {
        let mut end_idx = self.last_idx;
        while end_idx >= self.first_idx && predicate(&self.items[end_idx as usize]) {
            end_idx -= 1;
        }
        let result = (end_idx != self.last_idx).then(|| self.items[(end_idx + 1) as usize..(self.last_idx + 1) as usize].to_vec());
        self.last_idx = end_idx;
        result
    }

    pub fn peek(&self) -> Option<T> {
        if self.length() == 0 { None } else { self.at(self.first_idx).cloned() }
    }

    pub fn peek_last(&self) -> Option<T> {
        if self.length() == 0 { None } else { self.at(self.last_idx).cloned() }
    }

    /// Como o original, não confere se a fila está vazia: avança o início de qualquer jeito.
    pub fn dequeue(&mut self) -> Option<T> {
        let result = self.at(self.first_idx).cloned();
        self.first_idx += 1;
        result
    }

    /// Como [`ArrayQueue::dequeue`], pela outra ponta.
    pub fn remove_last(&mut self) -> Option<T> {
        let result = self.at(self.last_idx).cloned();
        self.last_idx -= 1;
        result
    }

    pub fn take_count(&mut self, count: usize) -> Vec<T> {
        let (from, to) = slice_bounds(self.items.len(), self.first_idx, Some(self.first_idx.saturating_add(count as i64)));
        self.first_idx += count as i64;
        self.items[from..to].to_vec()
    }
}

/// Mais rápido que um iterador sobre array para dado calculado sob demanda.
pub struct CallbackIterable<T: 'static> {
    iterate: Iterate<T>,
}

type Iterate<T> = Rc<dyn Fn(&mut dyn FnMut(T) -> bool)>;

impl<T: 'static> Clone for CallbackIterable<T> {
    fn clone(&self) -> Self {
        Self { iterate: self.iterate.clone() }
    }
}

impl<T: 'static> CallbackIterable<T> {
    /// `iterate` chama o callback para cada item e para quando ele devolve `false`.
    pub fn new(iterate: impl Fn(&mut dyn FnMut(T) -> bool) + 'static) -> Self {
        Self { iterate: Rc::new(iterate) }
    }

    pub fn empty() -> Self {
        Self::new(|_| {})
    }

    pub fn iterate(&self, callback: &mut dyn FnMut(T) -> bool) {
        (self.iterate)(callback);
    }

    pub fn for_each(&self, mut handler: impl FnMut(T)) {
        self.iterate(&mut |item| {
            handler(item);
            true
        });
    }

    pub fn to_array(&self) -> Vec<T> {
        let mut result = Vec::new();
        self.for_each(|item| result.push(item));
        result
    }

    pub fn filter(&self, predicate: impl Fn(&T) -> bool + 'static) -> CallbackIterable<T> {
        let this = self.clone();
        CallbackIterable::new(move |cb| this.iterate(&mut |item| if predicate(&item) { cb(item) } else { true }))
    }

    pub fn map<R: 'static>(&self, map_fn: impl Fn(T) -> R + 'static) -> CallbackIterable<R> {
        let this = self.clone();
        CallbackIterable::new(move |cb| this.iterate(&mut |item| cb(map_fn(item))))
    }

    pub fn some(&self, predicate: impl Fn(&T) -> bool) -> bool {
        let mut result = false;
        self.iterate(&mut |item| {
            result = predicate(&item);
            !result
        });
        result
    }

    pub fn find_first(&self, predicate: impl Fn(&T) -> bool) -> Option<T> {
        let mut result = None;
        self.iterate(&mut |item| {
            if predicate(&item) {
                result = Some(item);
                return false;
            }
            true
        });
        result
    }

    pub fn find_last(&self, predicate: impl Fn(&T) -> bool) -> Option<T> {
        let mut result = None;
        self.iterate(&mut |item| {
            if predicate(&item) {
                result = Some(item);
            }
            true
        });
        result
    }

    /// O último item que é maior ou igual a todos os outros.
    pub fn find_last_max_by(&self, comparator: impl Fn(&T, &T) -> CompareResult) -> Option<T> {
        let mut result: Option<T> = None;
        self.iterate(&mut |item| {
            if result.as_ref().is_none_or(|current| compare_result::is_greater_than(comparator(&item, current))) {
                result = Some(item);
            }
            true
        });
        result
    }
}

/// Um rearranjo de itens de um array.
#[derive(Clone, Debug, PartialEq)]
pub struct Permutation {
    index_map: Vec<usize>,
}

impl Permutation {
    pub fn new(index_map: Vec<usize>) -> Self {
        Self { index_map }
    }

    /// A permutação que ordena `arr` com `compare_fn` (ordenação estável dos índices).
    pub fn create_sort_permutation<T>(arr: &[T], compare_fn: impl Fn(&T, &T) -> CompareResult) -> Self {
        let mut sort_indices: Vec<usize> = (0..arr.len()).collect();
        sort_indices.sort_by(|&a, &b| to_ordering(compare_fn(&arr[a], &arr[b])));
        Self::new(sort_indices)
    }

    /// Novo array com os elementos de `arr` rearranjados por esta permutação.
    pub fn apply<T: Clone>(&self, arr: &[T]) -> Vec<T> {
        (0..arr.len()).map(|index| arr[self.index_map[index]].clone()).collect()
    }

    /// A permutação que desfaz esta.
    pub fn inverse(&self) -> Permutation {
        let mut inverse_index_map = self.index_map.clone();
        for (i, &target) in self.index_map.iter().enumerate() {
            inverse_index_map[target] = i;
        }
        Permutation::new(inverse_index_map)
    }
}

pub fn sum(array: &[f64]) -> f64 {
    array.iter().fold(0.0, |acc, value| acc + value)
}

pub fn sum_by<T>(array: &[T], selector: impl Fn(&T) -> f64) -> f64 {
    array.iter().fold(0.0, |acc, value| acc + selector(value))
}
