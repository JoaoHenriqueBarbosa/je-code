//! Tradução de `WTF/wtf/text/SymbolImpl.{h,cpp}`.
//!
//! Modelo: o `SymbolImpl` do C++ é um `StringImpl` com três campos a mais (`m_owner`,
//! `m_hashForSymbolShiftedWithFlagCount`, `m_flags`). Aqui `SymbolImpl` guarda o `Rc<StringImpl>`
//! do símbolo (criado por `StringImpl::new_symbol8/16/null`, com o bit de espécie `Symbol`) mais os
//! campos extras. A identidade do símbolo é a do `Rc<StringImpl>` (`string_impl()`), a mesma chave
//! `UniquedKey` que serve aos átomos. `PrivateSymbolImpl` e `RegisteredSymbolImpl` são embrulhos
//! com `Deref` para `SymbolImpl`.
//!
//! O `m_owner` do C++ mantém vivo o buffer que o símbolo compartilha (`BufferSubstring`); o porte
//! copia o buffer, e o campo guarda a string de origem só para preservar a posse.

use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;

use crate::wtf::text::string_hasher;
use crate::wtf::text::string_impl::StringImpl;

/// `SymbolImpl::Flags`.
pub type Flags = u32;

/// `SymbolImpl::s_flagDefault`.
pub const S_FLAG_DEFAULT: Flags = 0;
/// `SymbolImpl::s_flagIsNullSymbol`.
pub const S_FLAG_IS_NULL_SYMBOL: Flags = 0b001;
/// `SymbolImpl::s_flagIsRegistered`.
pub const S_FLAG_IS_REGISTERED: Flags = 0b010;
/// `SymbolImpl::s_flagIsPrivate`.
pub const S_FLAG_IS_PRIVATE: Flags = 0b100;

/// Identidade do `SymbolRegistry*` (o `CheckedPtr<SymbolRegistry>` do C++). O `SymbolRegistry.h`
/// ainda não foi portado; quando for, o registro entrega o seu id aqui.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolRegistryId(pub usize);

thread_local! {
    /// `s_nextHashForSymbol`. O C++ usa uma variável estática do processo, sem trava; o porte a
    /// mantém por thread, o que dá a mesma sequência em programa de uma thread só.
    static NEXT_HASH_FOR_SYMBOL: Cell<u32> = const { Cell::new(0) };
}

/// `class SymbolImpl`.
#[derive(Debug)]
pub struct SymbolImpl {
    /// O `StringImpl` do símbolo (espécie `Symbol`, nunca átomo).
    string: Rc<StringImpl>,
    /// `m_owner`.
    owner: Rc<StringImpl>,
    // `m_hashForSymbolShiftedWithFlagCount` mora no `StringImpl` do símbolo (ver o campo lá), para
    // `existingSymbolAwareHash()` alcançá-lo. O `RegisteredSymbolImpl::createPrivate` o reescreve.
    /// `m_flags`.
    flags: Flags,
    /// `RegisteredSymbolImpl::m_symbolRegistry`, que vive no derivado no C++; fica aqui para
    /// `symbolRegistry()` responder sem o `static_cast`.
    symbol_registry: Cell<Option<SymbolRegistryId>>,
}

/// Grava no `StringImpl` do símbolo a marca de privado, que o `Identifier::from_uid` lê.
fn mark_if_private(string: &StringImpl, flags: Flags) {
    if flags & S_FLAG_IS_PRIVATE != 0 {
        string.mark_private_symbol();
    }
}

/// `SymbolImpl::nextHashForSymbol()`.
///
/// In addition to the normal hash value, store specialized hash value for symbolized StringImpl*.
/// And don't use the normal hash value for symbolized StringImpl* when they are treated as
/// Identifiers. Unique nature of these symbolized StringImpl* keys means that we don't need them to
/// match any other string (in fact, that's exactly the opposite of what we want!), and the normal
/// hash would lead to lots of conflicts.
fn next_hash_for_symbol() -> u32 {
    NEXT_HASH_FOR_SYMBOL.with(|next| {
        let mut value = next.get();
        value = value.wrapping_add(1 << StringImpl::S_FLAG_COUNT);
        value |= 1u32 << 31;
        next.set(value);
        value
    })
}

impl SymbolImpl {
    pub const S_FLAG_DEFAULT: Flags = S_FLAG_DEFAULT;
    pub const S_FLAG_IS_NULL_SYMBOL: Flags = S_FLAG_IS_NULL_SYMBOL;
    pub const S_FLAG_IS_REGISTERED: Flags = S_FLAG_IS_REGISTERED;
    pub const S_FLAG_IS_PRIVATE: Flags = S_FLAG_IS_PRIVATE;

    /// `SymbolImpl(span, Ref<StringImpl>&& base, Flags)`: copia o conteúdo de `rep` para um
    /// símbolo novo, com o dono em `owner`.
    fn with_characters(rep: &StringImpl, owner: Rc<StringImpl>, flags: Flags) -> SymbolImpl {
        let string = if rep.is_8bit() {
            StringImpl::new_symbol8(rep.span8())
        } else {
            StringImpl::new_symbol16(rep.span16())
        };
        mark_if_private(&string, flags);
        string.set_hash_for_symbol_shifted_with_flag_count(next_hash_for_symbol());
        SymbolImpl {
            string: Rc::new(string),
            owner,
            flags,
            symbol_registry: Cell::new(None),
        }
    }

    /// `SymbolImpl(Flags)`: o símbolo nulo.
    fn null_symbol(flags: Flags) -> SymbolImpl {
        let string = StringImpl::new_null_symbol();
        mark_if_private(&string, flags);
        string.set_hash_for_symbol_shifted_with_flag_count(next_hash_for_symbol());
        SymbolImpl {
            string: Rc::new(string),
            owner: StringImpl::empty(),
            flags: flags | S_FLAG_IS_NULL_SYMBOL,
            symbol_registry: Cell::new(None),
        }
    }

    /// O `StringImpl` do símbolo: é a identidade que `AtomString`, `Identifier` e as tabelas usam.
    pub fn string_impl(&self) -> &Rc<StringImpl> {
        &self.string
    }

    /// `hashForSymbol()`.
    pub fn hash_for_symbol(&self) -> u32 {
        self.string.hash_for_symbol_shifted_with_flag_count() >> StringImpl::S_FLAG_COUNT
    }

    /// `m_hashForSymbolShiftedWithFlagCount`.
    pub fn hash_for_symbol_shifted_with_flag_count(&self) -> u32 {
        self.string.hash_for_symbol_shifted_with_flag_count()
    }

    /// `isNullSymbol()`.
    pub fn is_null_symbol(&self) -> bool {
        self.flags & S_FLAG_IS_NULL_SYMBOL != 0
    }

    /// `isRegistered()`.
    pub fn is_registered(&self) -> bool {
        self.flags & S_FLAG_IS_REGISTERED != 0
    }

    /// `isPrivate()`.
    pub fn is_private(&self) -> bool {
        self.flags & S_FLAG_IS_PRIVATE != 0
    }

    /// `m_flags`.
    pub fn flags(&self) -> Flags {
        self.flags
    }

    /// `m_owner`.
    pub fn owner(&self) -> &Rc<StringImpl> {
        &self.owner
    }

    /// `symbolRegistry()`: só o símbolo registrado tem registro.
    pub fn symbol_registry(&self) -> Option<SymbolRegistryId> {
        if self.is_registered() {
            return self.symbol_registry.get();
        }
        None
    }

    /// `SymbolImpl::createNullSymbol()`.
    pub fn create_null_symbol() -> Rc<SymbolImpl> {
        Rc::new(Self::null_symbol(S_FLAG_DEFAULT))
    }

    /// `SymbolImpl::create(StringImpl& rep)`. O C++ toma o dono do buffer quando `rep` é um
    /// substring; o buffer aqui é sempre próprio, então o dono é `rep`.
    pub fn create(rep: &Rc<StringImpl>) -> Rc<SymbolImpl> {
        Rc::new(Self::with_characters(rep, rep.clone(), S_FLAG_DEFAULT))
    }

    /// DIVERGÊNCIA: no C++ um `UniquedStringImpl*` de símbolo É o `SymbolImpl` (`static_cast`). Aqui a
    /// chave de propriedade é só o `StringImpl` do símbolo, então este construtor reconstrói um
    /// `SymbolImpl` que adota o mesmo `StringImpl` (a mesma identidade de chave) quando nenhum
    /// `Symbol` guardou o original, como acontece com os símbolos conhecidos instalados por
    /// `Identifier`. O `StringImpl` precisa ser de espécie símbolo.
    pub fn adopt(string: &Rc<StringImpl>, flags: Flags) -> Rc<SymbolImpl> {
        debug_assert!(string.is_symbol());
        mark_if_private(string, flags);
        // Adotar o mesmo `StringImpl` duas vezes mantém o hash já atribuído a ele.
        if string.hash_for_symbol_shifted_with_flag_count() == 0 {
            string.set_hash_for_symbol_shifted_with_flag_count(next_hash_for_symbol());
        }
        Rc::new(SymbolImpl {
            string: Rc::clone(string),
            owner: StringImpl::empty(),
            flags,
            symbol_registry: Cell::new(None),
        })
    }
}

/// `SymbolImpl::StaticSymbolImpl`: o símbolo de dados estáticos, com o hash do símbolo derivado do
/// conteúdo (`computeLiteralHashAndMaskTop8Bits(...) << s_flagCount`) em vez da ordem de criação.
/// O C++ o constrói em tempo de compilação; aqui `symbol_impl()` o materializa. O bit "estático" do
/// `StringImpl` só o construtor de `string_impl.rs` liga, então o símbolo materializado é
/// indistinguível de um comum pelo `StringImpl` (o hash do símbolo e as flags de `SymbolImpl`, sim,
/// são os do estático).
#[derive(Clone, Debug)]
pub struct StaticSymbolImpl {
    characters: StaticSymbolCharacters,
    flags: Flags,
}

#[derive(Clone, Debug)]
enum StaticSymbolCharacters {
    Latin1(&'static [u8]),
    Utf16(&'static [u16]),
}

impl StaticSymbolImpl {
    /// `StaticSymbolImpl(ASCIILiteral, Flags)`.
    pub const fn new8(literal: &'static [u8], flags: Flags) -> StaticSymbolImpl {
        StaticSymbolImpl {
            characters: StaticSymbolCharacters::Latin1(literal),
            flags,
        }
    }

    /// `StaticSymbolImpl(const char16_t (&)[N], Flags)`: `characters` sem o terminador nulo.
    pub const fn new16(characters: &'static [u16], flags: Flags) -> StaticSymbolImpl {
        StaticSymbolImpl {
            characters: StaticSymbolCharacters::Utf16(characters),
            flags,
        }
    }

    /// `operator SymbolImpl&()`.
    pub fn symbol_impl(&self) -> Rc<SymbolImpl> {
        let (string, hash) = match &self.characters {
            StaticSymbolCharacters::Latin1(characters) => (
                StringImpl::new_symbol8(characters),
                string_hasher::compute_literal_hash_and_mask_top8_bits::<u8>(characters),
            ),
            StaticSymbolCharacters::Utf16(characters) => (
                StringImpl::new_symbol16(characters),
                string_hasher::compute_literal_hash_and_mask_top8_bits::<u16>(characters),
            ),
        };
        mark_if_private(&string, self.flags);
        string.set_hash_for_symbol_shifted_with_flag_count(hash << StringImpl::S_FLAG_COUNT);
        Rc::new(SymbolImpl {
            string: Rc::new(string),
            owner: StringImpl::empty(),
            flags: self.flags,
            symbol_registry: Cell::new(None),
        })
    }
}

/// `class PrivateSymbolImpl`.
#[derive(Debug)]
pub struct PrivateSymbolImpl {
    symbol: SymbolImpl,
}

impl PrivateSymbolImpl {
    /// `PrivateSymbolImpl::create(StringImpl& rep)`.
    pub fn create(rep: &Rc<StringImpl>) -> Rc<PrivateSymbolImpl> {
        Rc::new(PrivateSymbolImpl {
            symbol: SymbolImpl::with_characters(rep, rep.clone(), S_FLAG_IS_PRIVATE),
        })
    }
}

impl Deref for PrivateSymbolImpl {
    type Target = SymbolImpl;

    fn deref(&self) -> &SymbolImpl {
        &self.symbol
    }
}

/// `class RegisteredSymbolImpl`.
#[derive(Debug)]
pub struct RegisteredSymbolImpl {
    symbol: SymbolImpl,
}

impl RegisteredSymbolImpl {
    /// `RegisteredSymbolImpl(span, base, registry, flags)`.
    fn new(rep: &Rc<StringImpl>, registry: SymbolRegistryId, flags: Flags) -> RegisteredSymbolImpl {
        let symbol = SymbolImpl::with_characters(rep, rep.clone(), flags);
        symbol.symbol_registry.set(Some(registry));
        RegisteredSymbolImpl { symbol }
    }

    /// `RegisteredSymbolImpl::create(StringImpl& rep, SymbolRegistry&)`.
    pub(crate) fn create(rep: &Rc<StringImpl>, symbol_registry: SymbolRegistryId) -> Rc<RegisteredSymbolImpl> {
        Rc::new(Self::new(rep, symbol_registry, S_FLAG_IS_REGISTERED))
    }

    /// `RegisteredSymbolImpl::createPrivate(StringImpl& rep, SymbolRegistry&)`.
    pub(crate) fn create_private(rep: &Rc<StringImpl>, symbol_registry: SymbolRegistryId) -> Rc<RegisteredSymbolImpl> {
        let symbol = Self::new(rep, symbol_registry, S_FLAG_IS_REGISTERED | S_FLAG_IS_PRIVATE);
        // The private registry's symbols are synthetic names never visible to script (the parser's
        // names for computed class members), each the only one in the registry with its contents;
        // so, like a StaticSymbolImpl, one can hash by its contents -- which the registry's lookup
        // just computed -- instead of by creation order. That keeps the iteration order of the
        // parser's variable environments, which reaches generated bytecode, the same in every
        // process.
        symbol
            .symbol
            .string
            .set_hash_for_symbol_shifted_with_flag_count(rep.hash() << StringImpl::S_FLAG_COUNT);
        Rc::new(symbol)
    }

    /// `symbolRegistry()` do derivado.
    pub(crate) fn symbol_registry(&self) -> Option<SymbolRegistryId> {
        self.symbol.symbol_registry.get()
    }

    /// `clearSymbolRegistry()`.
    pub(crate) fn clear_symbol_registry(&self) {
        self.symbol.symbol_registry.set(None);
    }
}

impl Deref for RegisteredSymbolImpl {
    type Target = SymbolImpl;

    fn deref(&self) -> &SymbolImpl {
        &self.symbol
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_values_match_cpp() {
        assert_eq!(S_FLAG_DEFAULT, 0);
        assert_eq!(S_FLAG_IS_NULL_SYMBOL, 0b001);
        assert_eq!(S_FLAG_IS_REGISTERED, 0b010);
        assert_eq!(S_FLAG_IS_PRIVATE, 0b100);
    }

    #[test]
    fn hash_sequence_counts_in_shifted_steps() {
        let first = next_hash_for_symbol();
        let second = next_hash_for_symbol();
        assert_eq!(first, (1 << StringImpl::S_FLAG_COUNT) | (1 << 31));
        assert_eq!(second, (2 << StringImpl::S_FLAG_COUNT) | (1 << 31));
        assert_eq!(first >> StringImpl::S_FLAG_COUNT, (1 << 23) | 1);
    }

    #[test]
    fn existing_symbol_aware_hash_follows_the_counter_not_the_address() {
        let rep = StringImpl::create(b"priv");
        let a = SymbolImpl::create(&rep);
        let b = SymbolImpl::create(&rep);
        let (ha, hb) = (a.string_impl().existing_symbol_aware_hash(), b.string_impl().existing_symbol_aware_hash());
        assert_eq!(ha, a.hash_for_symbol());
        assert_eq!(hb, b.hash_for_symbol());
        // Mesmo conteúdo, símbolos distintos: o contador avança de um em um.
        assert_eq!(hb, ha + 1);
        assert_eq!(a.string_impl().symbol_aware_hash(), ha);
        // Fora de símbolo vale o hash do conteúdo.
        assert_eq!(rep.existing_symbol_aware_hash(), rep.existing_hash());
        // Adotar de novo mantém o hash.
        let again = SymbolImpl::adopt(a.string_impl(), S_FLAG_DEFAULT);
        assert_eq!(again.hash_for_symbol(), ha);
    }

    #[test]
    fn create_copies_content_and_is_a_symbol() {
        let rep = StringImpl::create(b"description");
        let symbol = SymbolImpl::create(&rep);
        let string = symbol.string_impl();
        assert!(string.is_symbol());
        assert!(!string.is_atom());
        assert!(string.is_8bit());
        assert_eq!(string.span8(), b"description");
        assert!(!Rc::ptr_eq(string, &rep));
        assert!(Rc::ptr_eq(symbol.owner(), &rep));
        assert!(!symbol.is_null_symbol() && !symbol.is_registered() && !symbol.is_private());
        assert!(symbol.symbol_registry().is_none());
        assert!(symbol.hash_for_symbol() >> 23 & 1 == 1);

        let wide = StringImpl::create16(&[0x20AC, 0x61]);
        let wide_symbol = SymbolImpl::create(&wide);
        assert_eq!(wide_symbol.string_impl().span16(), &[0x20AC, 0x61]);
    }

    #[test]
    fn each_symbol_is_unique_with_its_own_hash() {
        let rep = StringImpl::create(b"same");
        let a = SymbolImpl::create(&rep);
        let b = SymbolImpl::create(&rep);
        assert!(!Rc::ptr_eq(a.string_impl(), b.string_impl()));
        assert_ne!(a.hash_for_symbol(), b.hash_for_symbol());
    }

    #[test]
    fn null_symbol() {
        let symbol = SymbolImpl::create_null_symbol();
        assert!(symbol.is_null_symbol());
        assert!(symbol.string_impl().is_symbol());
        assert_eq!(symbol.string_impl().length(), 0);
        assert!(Rc::ptr_eq(symbol.owner(), &StringImpl::empty()));
    }

    #[test]
    fn private_symbol() {
        let rep = StringImpl::create(b"#field");
        let symbol = PrivateSymbolImpl::create(&rep);
        assert!(symbol.is_private());
        assert!(!symbol.is_registered());
        assert!(symbol.string_impl().is_symbol());
    }

    #[test]
    fn registered_symbols() {
        let rep = StringImpl::create(b"registered");
        let registry = SymbolRegistryId(7);
        let symbol = RegisteredSymbolImpl::create(&rep, registry);
        assert!(symbol.is_registered() && !symbol.is_private());
        assert_eq!(SymbolImpl::symbol_registry(&symbol), Some(registry));
        assert_eq!(symbol.flags(), S_FLAG_IS_REGISTERED);
        symbol.clear_symbol_registry();
        assert!(SymbolImpl::symbol_registry(&symbol).is_none());
        assert!(RegisteredSymbolImpl::symbol_registry(&symbol).is_none());

        let private = RegisteredSymbolImpl::create_private(&rep, registry);
        assert_eq!(private.flags(), S_FLAG_IS_REGISTERED | S_FLAG_IS_PRIVATE);
        // Hash pelo conteúdo, não pela ordem de criação.
        assert_eq!(private.hash_for_symbol(), rep.hash());
        let again = RegisteredSymbolImpl::create_private(&rep, registry);
        assert_eq!(private.hash_for_symbol(), again.hash_for_symbol());
        assert_eq!(
            private.hash_for_symbol_shifted_with_flag_count(),
            rep.hash() << StringImpl::S_FLAG_COUNT
        );
    }

    #[test]
    fn static_symbols_hash_by_content() {
        static LITERAL: StaticSymbolImpl = StaticSymbolImpl::new8(b"static-sym", S_FLAG_IS_PRIVATE);
        let symbol = LITERAL.symbol_impl();
        assert!(symbol.is_private());
        assert_eq!(symbol.string_impl().span8(), b"static-sym");
        let expected = string_hasher::compute_literal_hash_and_mask_top8_bits::<u8>(b"static-sym");
        assert_eq!(symbol.hash_for_symbol(), expected);
        assert_eq!(LITERAL.symbol_impl().hash_for_symbol(), expected);

        static WIDE: StaticSymbolImpl = StaticSymbolImpl::new16(&[0x20AC, 0x61], S_FLAG_DEFAULT);
        assert_eq!(WIDE.symbol_impl().string_impl().span16(), &[0x20AC, 0x61]);
    }
}
