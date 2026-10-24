//! Tradução de `WTF/wtf/text/AtomStringImpl.{h,cpp}`, `AtomStringTable.{h,cpp}` e
//! `UniquedStringImpl.{h,cpp}`.
//!
//! Modelo (CONVENTIONS, item 1): a tabela de átomos é por thread (`thread_local!`, o
//! `Thread::currentSingleton().atomStringTable()` do C++). O elemento da tabela é o
//! `Rc<StringImpl>` comparado por conteúdo e espalhado pelo hash da WTF (`StringImpl::hash`). O
//! `AtomStringImpl*` do C++ é o próprio `Rc<StringImpl>` com o bit `is_atom` ligado, então a
//! identidade de um átomo é a identidade do `Rc` (`Rc::ptr_eq`).
//!
//! Remoção (`AtomStringImpl::remove`): o C++ chama `remove` no destrutor do último `RefPtr`. O
//! `StringImpl` não tem `Drop` que alcance a tabela, então a coleta é feita por contagem de
//! referências: uma entrada cujo `Rc::strong_count` é 1 (só a tabela a segura) e que não é estática
//! está morta. A semântica observável é a do C++:
//!
//! - `look_up` e as buscas por conteúdo ignoram entrada morta, como se ela já tivesse saído;
//! - `add` pode reaproveitar uma entrada morta (ninguém guarda o ponteiro antigo, então a
//!   identidade nova é indistinguível de uma criada do zero);
//! - a memória é devolvida por uma varredura amortizada a cada dobra do tamanho da tabela
//!   (`StringTableImpl::sweep_unreferenced`), que também pode ser chamada à mão.
//!
//! O `remove` explícito continua existindo, como no C++.
//!
//! Sem `Lock`: `USE(WEB_THREAD)` não vale no Linux, o `AtomStringTableLocker` é vazio.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::wtf::text::string_hasher;
use crate::wtf::text::string_impl::{CharType, StringImpl};

/// `UniquedStringImpl`: um `StringImpl` internado (átomo ou símbolo), que se compara por ponteiro.
/// Os construtores `CreateSymbol` do C++ são `StringImpl::new_symbol8/16/null` no porte. A chave
/// de identidade é `string_impl::UniquedKey`.
pub type UniquedStringImpl = StringImpl;

// ---------------------------------------------------------------------------------------------
// Comparação por conteúdo (`WTF::equal` de `StringCommon.h` / `StringImpl.h`)
// ---------------------------------------------------------------------------------------------

/// `WTF::equal(const StringImpl*, std::span<const CharacterType>)`: igualdade lógica dos códigos de
/// caractere, sem importar se o buffer é de 8 ou de 16 bits.
pub fn equal_characters<T: CharType>(string: &StringImpl, characters: &[T]) -> bool {
    if string.is_8bit() {
        let own = string.span8();
        own.len() == characters.len() && own.iter().zip(characters).all(|(a, b)| *a as u16 == b.to_u16())
    } else {
        let own = string.span16();
        own.len() == characters.len() && own.iter().zip(characters).all(|(a, b)| *a == b.to_u16())
    }
}

/// `WTF::equal(const StringImpl*, const StringImpl*)`: nulo só é igual a nulo.
pub fn equal_string_impl(a: Option<&StringImpl>, b: Option<&StringImpl>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            if std::ptr::eq(a, b) {
                return true;
            }
            if a.length() != b.length() {
                return false;
            }
            if b.is_8bit() {
                equal_characters(a, b.span8())
            } else {
                equal_characters(a, b.span16())
            }
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// AtomStringTable
// ---------------------------------------------------------------------------------------------

/// Piso do limiar de varredura das entradas mortas.
const MIN_SWEEP_THRESHOLD: usize = 1024;

/// Uma entrada está viva se a tabela não é a única a segurá-la, ou se é estática (imortal).
fn is_live(entry: &Rc<StringImpl>) -> bool {
    entry.is_static() || Rc::strong_count(entry) > 1
}

/// `AtomStringTable::StringTableImpl` (`UncheckedKeyHashSet<StringEntry>`): conjunto de
/// `Rc<StringImpl>` espalhado pelo hash da WTF. O balde é a lista das entradas de mesmo hash,
/// comparadas por conteúdo.
#[derive(Debug)]
pub struct StringTableImpl {
    buckets: HashMap<u32, Vec<Rc<StringImpl>>>,
    size: usize,
    sweep_threshold: usize,
}

impl StringTableImpl {
    fn new() -> StringTableImpl {
        StringTableImpl {
            buckets: HashMap::new(),
            size: 0,
            sweep_threshold: MIN_SWEEP_THRESHOLD,
        }
    }

    /// `HashSet::size()`.
    pub fn size(&self) -> usize {
        self.size
    }

    /// `HashSet::find<Translator>`: a primeira entrada do balde que o predicado aceita.
    fn find_with(&self, hash: u32, matches: impl Fn(&StringImpl) -> bool) -> Option<&Rc<StringImpl>> {
        self.buckets.get(&hash)?.iter().find(|entry| matches(&***entry))
    }

    /// Como `find_with`, mas ignora a entrada morta (a que o C++ já teria removido).
    fn find_live_with(&self, hash: u32, matches: impl Fn(&StringImpl) -> bool) -> Option<&Rc<StringImpl>> {
        self.buckets
            .get(&hash)?
            .iter()
            .find(|entry| is_live(entry) && matches(&***entry))
    }

    /// Insere uma entrada nova (o chamador já verificou que não existe) e, quando a tabela dobra,
    /// recolhe as mortas.
    fn insert(&mut self, hash: u32, string: Rc<StringImpl>) {
        self.buckets.entry(hash).or_default().push(string);
        self.size += 1;
        if self.size >= self.sweep_threshold {
            self.sweep_unreferenced();
            self.sweep_threshold = std::cmp::max(MIN_SWEEP_THRESHOLD, self.size * 2);
        }
    }

    /// `HashSet::remove(iterator)` do `AtomStringImpl::remove`: acha a entrada pelo ponteiro, sem
    /// comparar conteúdo.
    fn remove_pointer(&mut self, hash: u32, string: &Rc<StringImpl>) -> bool {
        let Some(bucket) = self.buckets.get_mut(&hash) else {
            return false;
        };
        let Some(position) = bucket.iter().position(|entry| Rc::ptr_eq(entry, string)) else {
            return false;
        };
        bucket.swap_remove(position);
        if bucket.is_empty() {
            self.buckets.remove(&hash);
        }
        self.size -= 1;
        true
    }

    /// `HashSet::reserveCapacity`.
    pub fn reserve_capacity(&mut self, capacity: usize) {
        self.buckets.reserve(capacity.saturating_sub(self.size));
    }

    /// Tira da tabela as entradas que só ela segura: o equivalente, em lote, das chamadas a
    /// `AtomStringImpl::remove` que o destrutor do último `RefPtr` faria no C++.
    pub fn sweep_unreferenced(&mut self) {
        self.buckets.retain(|_, bucket| {
            bucket.retain(is_live);
            !bucket.is_empty()
        });
        self.size = self.buckets.values().map(Vec::len).sum();
    }
}

/// `class AtomStringTable`.
#[derive(Debug)]
pub struct AtomStringTable {
    m_table: StringTableImpl,
}

impl AtomStringTable {
    pub fn new() -> AtomStringTable {
        AtomStringTable { m_table: StringTableImpl::new() }
    }

    /// `AtomStringTable::table()`.
    pub fn table(&mut self) -> &mut StringTableImpl {
        &mut self.m_table
    }
}

impl Default for AtomStringTable {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AtomStringTable {
    /// `AtomStringTable::~AtomStringTable`: as strings que sobrevivem à tabela deixam de ser
    /// átomos. As estáticas são imortais e o bit delas vem da construção, então ficam como estão.
    fn drop(&mut self) {
        for bucket in self.m_table.buckets.values() {
            for string in bucket {
                if !string.is_static() {
                    string.set_is_atom(false);
                }
            }
        }
    }
}

thread_local! {
    /// `Thread::atomStringTable()`.
    static ATOM_STRING_TABLE: RefCell<AtomStringTable> = RefCell::new(AtomStringTable::new());
}

/// `stringTable()`: a tabela da thread corrente.
fn with_string_table<R>(f: impl FnOnce(&mut StringTableImpl) -> R) -> R {
    ATOM_STRING_TABLE.with(|table| f(table.borrow_mut().table()))
}

// ---------------------------------------------------------------------------------------------
// AtomStringImpl
// ---------------------------------------------------------------------------------------------

/// `HashTranslatorCharBuffer<CharType>`: os caracteres com o hash já calculado.
#[derive(Clone, Copy, Debug)]
pub struct HashTranslatorCharBuffer<'a, T: CharType> {
    pub characters: &'a [T],
    pub hash: u32,
}

impl<'a, T: CharType> HashTranslatorCharBuffer<'a, T> {
    pub fn new(characters: &'a [T]) -> Self {
        HashTranslatorCharBuffer {
            characters,
            hash: string_hasher::compute_hash_and_mask_top8_bits::<T>(characters),
        }
    }
}

/// `addToStringTable<T, HashTranslator>`: acha a entrada igual ou traduz uma nova e a guarda. O
/// `translate` devolve a string já com hash e bit de átomo (o `setHash` + `setIsAtom(true)` dos
/// tradutores do C++; `hash()` calcula o mesmo valor que `setHash(hash)` gravaria).
fn add_to_string_table(
    hash: u32,
    matches: impl Fn(&StringImpl) -> bool,
    translate: impl FnOnce() -> Rc<StringImpl>,
) -> Rc<StringImpl> {
    with_string_table(|table| {
        if let Some(existing) = table.find_with(hash, matches) {
            return existing.clone();
        }
        let string = translate();
        table.insert(hash, string.clone());
        string
    })
}

/// O final comum dos tradutores: grava o hash e marca como átomo.
fn finish_atom(string: Rc<StringImpl>) -> Rc<StringImpl> {
    string.hash();
    string.set_is_atom(true);
    string
}

/// `add(HashTranslatorCharBuffer<CharType>&)` com o tradutor que cria a string (`Latin1BufferTranslator`
/// usa `StringImpl::create`, `UTF16BufferTranslator` usa `create8BitIfPossible`).
fn add_buffer<T: CharType>(
    buffer: &HashTranslatorCharBuffer<T>,
    create: fn(&[T]) -> Rc<StringImpl>,
) -> Rc<StringImpl> {
    if buffer.characters.is_empty() {
        return StringImpl::empty();
    }
    add_to_string_table(
        buffer.hash,
        |entry| equal_characters(entry, buffer.characters),
        || finish_atom(create(buffer.characters)),
    )
}

/// `lookUp(span)`: busca na tabela sem inserir.
fn look_up_buffer<T: CharType>(characters: &[T]) -> Option<Rc<StringImpl>> {
    let buffer = HashTranslatorCharBuffer::new(characters);
    with_string_table(|table| {
        table
            .find_live_with(buffer.hash, |entry| equal_characters(entry, characters))
            .cloned()
    })
}

/// `SubstringTranslator8/16`: o hash e a igualdade são sobre o trecho da base.
fn add_substring_characters<T: CharType>(
    base: &StringImpl,
    characters: &[T],
    start: u32,
    length: u32,
) -> Rc<StringImpl> {
    let hash = string_hasher::compute_hash_and_mask_top8_bits::<T>(characters);
    add_to_string_table(
        hash,
        |entry| equal_characters(entry, characters),
        || finish_atom(StringImpl::create_substring_sharing_impl(base, start, length)),
    )
}

/// `addSymbol`: o átomo de um símbolo é uma cópia normal do conteúdo (`SubstringTranslator` sobre
/// a string inteira), nunca o próprio símbolo.
fn add_symbol(base: &Rc<StringImpl>) -> Rc<StringImpl> {
    debug_assert!(base.length() != 0);
    debug_assert!(base.is_symbol());
    let length = base.length();
    if base.is_8bit() {
        add_substring_characters(base, base.span8(), 0, length)
    } else {
        add_substring_characters(base, base.span16(), 0, length)
    }
}

/// `BufferFromStaticDataTranslator<CharType>`: o C++ aponta o átomo para o buffer estático; o porte
/// copia (`createWithoutCopying` sempre copia).
fn add_from_static_data<T: CharType>(characters: &[T], hash: u32) -> Rc<StringImpl> {
    add_to_string_table(
        hash,
        |entry| equal_characters(entry, characters),
        || finish_atom(StringImpl::create_without_copying_non_empty(characters)),
    )
}

/// `addStatic`.
fn add_static(base: &Rc<StringImpl>) -> Rc<StringImpl> {
    debug_assert!(base.length() != 0);
    debug_assert!(base.is_static());

    // StaticStringImpl com StringAtom: a própria string estática entra na tabela (`StaticStringAtomTranslator`),
    // sem cópia, e todas as threads que a registram compartilham o mesmo ponteiro.
    if base.is_atom() {
        let hash = base.hash();
        return with_string_table(|table| {
            let existing = if base.is_8bit() {
                table.find_with(hash, |entry| equal_characters(entry, base.span8()))
            } else {
                table.find_with(hash, |entry| equal_characters(entry, base.span16()))
            };
            if let Some(existing) = existing {
                return existing.clone();
            }
            table.insert(hash, base.clone());
            base.clone()
        });
    }

    if base.is_8bit() {
        return add_from_static_data(base.span8(), base.hash());
    }
    add_from_static_data(base.span16(), base.hash())
}

/// `addSlowCase`: a string não é átomo, não é nula, e `canBecomeAtom()` vale.
fn add_slow_case(string: &Rc<StringImpl>) -> Rc<StringImpl> {
    // This check is necessary for null symbols.
    // Their length is zero, but they are not AtomStringImpl.
    if string.length() == 0 {
        return StringImpl::empty();
    }

    if string.is_static() {
        return add_static(string);
    }

    if string.is_symbol() {
        return add_symbol(string);
    }

    debug_assert!(
        !string.is_atom(),
        "AtomStringImpl should not hit the slow case if the string is already an atom."
    );

    let hash = string.hash();
    with_string_table(|table| {
        if let Some(existing) = table.find_with(hash, |entry| equal_string_impl(Some(entry), Some(string))) {
            return existing.clone();
        }
        string.set_is_atom(true);
        table.insert(hash, string.clone());
        string.clone()
    })
}

/// `class AtomStringImpl`: só funções associadas, o átomo é o `Rc<StringImpl>` com `is_atom()`.
pub struct AtomStringImpl;

impl AtomStringImpl {
    /// `lookUp(std::span<const Latin1Character>)`.
    pub fn look_up(characters: &[u8]) -> Option<Rc<StringImpl>> {
        look_up_buffer(characters)
    }

    /// `lookUp(std::span<const char16_t>)`.
    pub fn look_up16(characters: &[u16]) -> Option<Rc<StringImpl>> {
        look_up_buffer(characters)
    }

    /// `lookUp(StringImpl*)`.
    pub fn look_up_impl(string: Option<&Rc<StringImpl>>) -> Option<Rc<StringImpl>> {
        let string = string?;
        if string.is_atom() {
            return Some(string.clone());
        }
        Self::look_up_slow_case(string)
    }

    /// `lookUpSlowCase`.
    fn look_up_slow_case(string: &Rc<StringImpl>) -> Option<Rc<StringImpl>> {
        debug_assert!(
            !string.is_atom(),
            "AtomStringImpl objects should return from the fast case."
        );

        if string.length() == 0 {
            return Some(StringImpl::empty());
        }

        let hash = string.hash();
        with_string_table(|table| {
            table
                .find_live_with(hash, |entry| equal_string_impl(Some(entry), Some(string)))
                .cloned()
        })
    }

    /// `AtomStringImpl::remove`: tira da tabela da thread o átomo (já sabido igual ao da tabela, sem
    /// comparar conteúdo).
    pub fn remove(string: &Rc<StringImpl>) {
        debug_assert!(string.is_atom());
        let hash = string.hash();
        let was_removed = with_string_table(|table| table.remove_pointer(hash, string));
        assert!(
            was_removed,
            "The string being removed is an atom in the string table of an other thread!"
        );
    }

    /// `AtomStringImpl::add(std::span<const Latin1Character>)`. O ponteiro nulo do C++ não existe em
    /// fatia de Rust: a fatia vazia dá a string vazia.
    pub fn add(characters: &[u8]) -> Rc<StringImpl> {
        add_buffer(&HashTranslatorCharBuffer::new(characters), StringImpl::create)
    }

    /// `AtomStringImpl::add(std::span<const char16_t>)`: o átomo novo é estreitado para 8 bits
    /// quando possível (`create8BitIfPossible`).
    pub fn add16(characters: &[u16]) -> Rc<StringImpl> {
        add_buffer(&HashTranslatorCharBuffer::new(characters), StringImpl::create8_bit_if_possible)
    }

    /// `AtomStringImpl::add(HashTranslatorCharBuffer<Latin1Character>&)`.
    pub fn add_latin1_buffer(buffer: &HashTranslatorCharBuffer<u8>) -> Rc<StringImpl> {
        add_buffer(buffer, StringImpl::create)
    }

    /// `AtomStringImpl::add(HashTranslatorCharBuffer<char16_t>&)`.
    pub fn add_utf16_buffer(buffer: &HashTranslatorCharBuffer<u16>) -> Rc<StringImpl> {
        add_buffer(buffer, StringImpl::create8_bit_if_possible)
    }

    /// `AtomStringImpl::add(StringImpl*, unsigned offset, unsigned length)`.
    pub fn add_substring(base: Option<&Rc<StringImpl>>, start: u32, length: u32) -> Option<Rc<StringImpl>> {
        let base = base?;

        if length == 0 || start >= base.length() {
            return Some(StringImpl::empty());
        }

        let max_length = base.length() - start;
        let mut length = length;
        if length >= max_length {
            if start == 0 {
                return Some(Self::add_string_impl(base));
            }
            length = max_length;
        }

        let (from, to) = (start as usize, (start + length) as usize);
        if base.is_8bit() {
            Some(add_substring_characters(base, &base.span8()[from..to], start, length))
        } else {
            Some(add_substring_characters(base, &base.span16()[from..to], start, length))
        }
    }

    /// `AtomStringImpl::add(StringImpl&)` (a sobrecarga inline do cabeçalho).
    pub fn add_string_impl(string: &Rc<StringImpl>) -> Rc<StringImpl> {
        if string.is_atom() {
            return string.clone();
        }
        // USE(BUN_JSC_ADDITIONS): TODO do Bun, remover quando os átomos forem do processo todo.
        if !string.can_become_atom() {
            if string.is_8bit() {
                return Self::add(string.span8());
            }
            return Self::add16(string.span16());
        }
        add_slow_case(string)
    }

    /// `AtomStringImpl::add(StringImpl*)`.
    pub fn add_string_impl_option(string: Option<&Rc<StringImpl>>) -> Option<Rc<StringImpl>> {
        Some(Self::add_string_impl(string?))
    }

    /// `AtomStringImpl::add(Ref<StringImpl>&&)`: consome o `Rc`.
    pub fn add_rc(string: Rc<StringImpl>) -> Rc<StringImpl> {
        Self::add_string_impl(&string)
    }

    /// `AtomStringImpl::add(const StaticStringImpl&)`.
    pub fn add_static(string: &Rc<StringImpl>) -> Rc<StringImpl> {
        debug_assert!(string.is_static());
        add_static(string)
    }

    /// `AtomStringImpl::addLiteral` / `add(ASCIILiteral)`.
    pub fn add_literal(characters: &[u8]) -> Rc<StringImpl> {
        debug_assert!(!characters.is_empty());
        let buffer = HashTranslatorCharBuffer::new(characters);
        add_from_static_data(buffer.characters, buffer.hash)
    }

    /// `AtomStringImpl::reserveCapacityForCurrentThread`: quem vai inserir `additional_count`
    /// strings deixa a tabela crescer uma vez só.
    pub fn reserve_capacity_for_current_thread(additional_count: u32) {
        with_string_table(|table| {
            let capacity = table.size() + additional_count as usize;
            table.reserve_capacity(capacity);
        });
    }

    /// `AtomStringImpl::add(std::span<const char8_t>)`: `None` se o UTF-8 for inválido.
    pub fn add_utf8(characters: &[u8]) -> Option<Rc<StringImpl>> {
        if characters.is_ascii() {
            return Some(Self::add(characters));
        }
        let string = StringImpl::create_from_utf8(characters)?;
        Some(Self::add_rc(string))
    }

    /// `isInAtomStringTable` (só usado em `ASSERT`): vale para a entrada viva da thread.
    pub fn is_in_atom_string_table(string: &Rc<StringImpl>) -> bool {
        let hash = string.hash();
        with_string_table(|table| table.find_with(hash, |entry| std::ptr::eq(entry, &**string)).is_some())
    }

    /// Recolhe as entradas que só a tabela da thread segura (ver a nota do módulo).
    pub fn sweep_unreferenced() {
        with_string_table(StringTableImpl::sweep_unreferenced);
    }

    /// Quantas entradas a tabela da thread guarda (mortas ainda não varridas incluídas).
    pub fn table_size() -> usize {
        with_string_table(|table| table.size())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_content_gives_same_pointer() {
        let a = AtomStringImpl::add(b"identifier");
        let b = AtomStringImpl::add(b"identifier");
        assert!(Rc::ptr_eq(&a, &b));
        assert!(a.is_atom());
        assert!(a.is_8bit());
        assert!(a.has_hash());
    }

    #[test]
    fn utf16_latin1_content_narrows_and_matches() {
        let narrow = AtomStringImpl::add(b"abc");
        let wide = AtomStringImpl::add16(&['a' as u16, 'b' as u16, 'c' as u16]);
        assert!(Rc::ptr_eq(&narrow, &wide));
        assert!(wide.is_8bit());

        let euro = AtomStringImpl::add16(&[0x20AC, 0x61]);
        assert!(!euro.is_8bit());
        assert!(Rc::ptr_eq(&euro, &AtomStringImpl::add16(&[0x20AC, 0x61])));
    }

    #[test]
    fn latin1_and_utf16_buffers_hash_the_same() {
        let latin1 = HashTranslatorCharBuffer::new(b"hello".as_slice());
        let wide: Vec<u16> = "hello".encode_utf16().collect();
        let utf16 = HashTranslatorCharBuffer::new(wide.as_slice());
        assert_eq!(latin1.hash, utf16.hash);
    }

    #[test]
    fn empty_is_the_static_empty_atom() {
        let empty = AtomStringImpl::add(b"");
        assert!(Rc::ptr_eq(&empty, &StringImpl::empty()));
        assert!(Rc::ptr_eq(&AtomStringImpl::add16(&[]), &StringImpl::empty()));
    }

    #[test]
    fn plain_string_becomes_the_atom_itself() {
        let string = StringImpl::create(b"plain-string");
        assert!(!string.is_atom());
        let atom = AtomStringImpl::add_string_impl(&string);
        assert!(Rc::ptr_eq(&string, &atom));
        assert!(string.is_atom());

        // Outra string de mesmo conteúdo cai no átomo existente e continua não atômica.
        let other = StringImpl::create(b"plain-string");
        let same = AtomStringImpl::add_string_impl(&other);
        assert!(Rc::ptr_eq(&same, &string));
        assert!(!other.is_atom());
    }

    #[test]
    fn never_atomize_strings_are_copied() {
        let string = StringImpl::create(b"never-atomize");
        string.set_never_atomize();
        let atom = AtomStringImpl::add_string_impl(&string);
        assert!(!Rc::ptr_eq(&string, &atom));
        assert!(atom.is_atom());
        assert!(!string.is_atom());
    }

    #[test]
    fn look_up_does_not_insert() {
        assert!(AtomStringImpl::look_up(b"never-added-xyz").is_none());
        assert!(AtomStringImpl::look_up(b"never-added-xyz").is_none());
        let atom = AtomStringImpl::add(b"now-added-xyz");
        let found = AtomStringImpl::look_up(b"now-added-xyz").unwrap();
        assert!(Rc::ptr_eq(&atom, &found));
        let wide: Vec<u16> = "now-added-xyz".encode_utf16().collect();
        assert!(Rc::ptr_eq(&atom, &AtomStringImpl::look_up16(&wide).unwrap()));

        let plain = StringImpl::create(b"now-added-xyz");
        assert!(Rc::ptr_eq(&atom, &AtomStringImpl::look_up_impl(Some(&plain)).unwrap()));
        assert!(AtomStringImpl::look_up_impl(None).is_none());
        let missing = StringImpl::create(b"missing-xyz");
        assert!(AtomStringImpl::look_up_impl(Some(&missing)).is_none());
        assert!(Rc::ptr_eq(
            &AtomStringImpl::look_up_impl(Some(&StringImpl::empty())).unwrap(),
            &StringImpl::empty()
        ));
    }

    #[test]
    fn dead_atoms_are_invisible_to_look_up_and_swept() {
        let before = AtomStringImpl::table_size();
        let atom = AtomStringImpl::add(b"short-lived-atom");
        assert_eq!(AtomStringImpl::table_size(), before + 1);
        drop(atom);
        // O C++ já teria removido a entrada no destrutor.
        assert!(AtomStringImpl::look_up(b"short-lived-atom").is_none());
        AtomStringImpl::sweep_unreferenced();
        assert_eq!(AtomStringImpl::table_size(), before);
        // Pode ser criado de novo.
        assert!(AtomStringImpl::add(b"short-lived-atom").is_atom());
    }

    #[test]
    fn many_dead_atoms_do_not_grow_the_table_without_bound() {
        for i in 0..5000 {
            let text = format!("transient-{i}");
            drop(AtomStringImpl::add(text.as_bytes()));
        }
        assert!(AtomStringImpl::table_size() < 2 * MIN_SWEEP_THRESHOLD + 1);
    }

    #[test]
    fn explicit_remove() {
        let atom = AtomStringImpl::add(b"to-be-removed");
        AtomStringImpl::remove(&atom);
        assert!(AtomStringImpl::look_up(b"to-be-removed").is_none());
    }

    #[test]
    fn substring_atoms() {
        let base = StringImpl::create(b"abcdefgh");
        let sub = AtomStringImpl::add_substring(Some(&base), 2, 3).unwrap();
        assert_eq!(sub.span8(), b"cde");
        assert!(Rc::ptr_eq(&sub, &AtomStringImpl::add(b"cde")));

        assert!(AtomStringImpl::add_substring(None, 0, 1).is_none());
        assert!(AtomStringImpl::add_substring(Some(&base), 0, 0).unwrap().is_empty());
        assert!(AtomStringImpl::add_substring(Some(&base), 8, 1).unwrap().is_empty());
        // Comprimento além do fim é aparado.
        assert_eq!(AtomStringImpl::add_substring(Some(&base), 6, 100).unwrap().span8(), b"gh");
        // A string inteira é o próprio átomo da base.
        let whole = AtomStringImpl::add_substring(Some(&base), 0, 100).unwrap();
        assert!(Rc::ptr_eq(&whole, &base));
        assert!(base.is_atom());

        let wide_base = StringImpl::create16(&[0x20AC, 0x61, 0x62]);
        let wide_sub = AtomStringImpl::add_substring(Some(&wide_base), 1, 2).unwrap();
        assert!(Rc::ptr_eq(&wide_sub, &AtomStringImpl::add(b"ab")));
    }

    #[test]
    fn symbols_atomize_to_a_plain_copy() {
        let symbol = Rc::new(StringImpl::new_symbol8(b"sym-text"));
        assert!(symbol.is_symbol());
        let atom = AtomStringImpl::add_string_impl(&symbol);
        assert!(!Rc::ptr_eq(&symbol, &atom));
        assert!(atom.is_atom());
        assert!(!atom.is_symbol());
        assert_eq!(atom.span8(), b"sym-text");
        assert!(symbol.is_symbol() && !symbol.is_atom());

        // O símbolo nulo tem comprimento zero e vira a string vazia.
        let null_symbol = Rc::new(StringImpl::new_null_symbol());
        assert!(Rc::ptr_eq(&AtomStringImpl::add_string_impl(&null_symbol), &StringImpl::empty()));
    }

    #[test]
    fn static_strings() {
        let plain_static = StringImpl::create_static_string_impl8(b"static-text");
        let atom = AtomStringImpl::add_static(&plain_static);
        assert!(atom.is_atom());
        assert!(!Rc::ptr_eq(&atom, &plain_static));
        assert!(Rc::ptr_eq(&atom, &AtomStringImpl::add(b"static-text")));
        assert!(Rc::ptr_eq(&atom, &AtomStringImpl::add_string_impl(&plain_static)));

        let literal = AtomStringImpl::add_literal(b"literal-text");
        assert!(Rc::ptr_eq(&literal, &AtomStringImpl::add(b"literal-text")));
    }

    #[test]
    fn utf8_input() {
        assert!(Rc::ptr_eq(&AtomStringImpl::add_utf8(b"ascii").unwrap(), &AtomStringImpl::add(b"ascii")));
        let euro = AtomStringImpl::add_utf8("a\u{20AC}".as_bytes()).unwrap();
        assert_eq!(euro.span16(), &[0x61, 0x20AC]);
        assert!(AtomStringImpl::add_utf8(&[0xFF, 0xFE]).is_none());
        assert!(AtomStringImpl::add_utf8(b"").unwrap().is_empty());
    }

    #[test]
    fn table_is_per_thread() {
        let here = AtomStringImpl::add(b"thread-local-text");
        let handle = std::thread::spawn(|| {
            assert!(AtomStringImpl::look_up(b"thread-local-text").is_none());
            AtomStringImpl::add(b"thread-local-text").is_atom()
        });
        assert!(handle.join().unwrap());
        assert!(AtomStringImpl::is_in_atom_string_table(&here));
    }

    #[test]
    fn content_equality_across_widths() {
        let narrow = StringImpl::create(b"abc");
        let wide = StringImpl::create16(&['a' as u16, 'b' as u16, 'c' as u16]);
        assert!(equal_string_impl(Some(&narrow), Some(&wide)));
        assert!(equal_string_impl(None, None));
        assert!(!equal_string_impl(Some(&narrow), None));
        assert!(!equal_string_impl(Some(&narrow), Some(&StringImpl::create(b"abd"))));
    }
}
