//! Tradução de `JavaScriptCore/parser/ParserArena.h` e `ParserArena.cpp`.
//!
//! A parte de alocação bruta do C++ (`allocateFreeable`, `allocateDeletable`, os pools de 8000
//! bytes, `ParserArenaDeletable` e o `deallocateObjects`) some: os nós da árvore são `Box`/`Vec`
//! (CONVENTIONS, item 3) e o Rust os libera sozinho. Sobra o que tem comportamento: o
//! `IdentifierArena`, dono dos `Identifier` criados durante o parse, com os caches de
//! identificadores curtos e recentes.
//!
//! Onde o C++ devolve `const Identifier&` para dentro do `SegmentedVector`, o porte devolve um
//! `Identifier` clonado (um `Rc` por baixo, mesma identidade de `StringImpl`); a arena continua
//! dona do original, e os caches guardam índices em `identifiers` no lugar de ponteiros.
//!
//! Dependências ainda não portadas, usadas com estes nomes (conferir quando existirem):
//! `crate::runtime::identifier::Identifier` (`from_string`, `create_latin1`, `from_uid`,
//! `from_int32`, `from_double`, `from_wtf_string`, `equal`, `string`, `r#impl`),
//! `crate::runtime::vm::VM` (`property_names.empty_identifier`, `private_symbol_registry()`),
//! `crate::runtime::js_big_int::JSBigInt`, `crate::runtime::math_common`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::identifier::Identifier;
use crate::runtime::js_big_int::{ErrorParseMode, JSBigInt, ParseIntSign};
use crate::wtf::math_extras::try_convert_to_strict_int32;
use crate::runtime::vm::{DeferTermination, TopExceptionScope, VM};
use crate::wtf::text::string_impl::{CharType, StringImpl};

/// `IdentifierArena::MaximumCachableCharacter`.
pub const MAXIMUM_CACHABLE_CHARACTER: usize = 128;

/// `class IdentifierArena`.
pub struct IdentifierArena {
    /// `m_identifiers` (`SegmentedVector<Identifier, 64>`): só cresce até `clear()`.
    identifiers: Vec<Identifier>,
    /// `m_shortIdentifiers`: índice em `identifiers` do identificador de um caractere só.
    short_identifiers: [Option<usize>; MAXIMUM_CACHABLE_CHARACTER],
    /// `m_recentIdentifiers`: índice do último identificador criado por primeiro caractere.
    recent_identifiers: [Option<usize>; MAXIMUM_CACHABLE_CHARACTER],
}

impl Default for IdentifierArena {
    fn default() -> IdentifierArena {
        IdentifierArena::new()
    }
}

impl IdentifierArena {
    pub fn new() -> IdentifierArena {
        IdentifierArena {
            identifiers: Vec::new(),
            short_identifiers: [None; MAXIMUM_CACHABLE_CHARACTER],
            recent_identifiers: [None; MAXIMUM_CACHABLE_CHARACTER],
        }
    }

    pub fn clear(&mut self) {
        self.identifiers.clear();
        self.short_identifiers = [None; MAXIMUM_CACHABLE_CHARACTER];
        self.recent_identifiers = [None; MAXIMUM_CACHABLE_CHARACTER];
    }

    /// `m_identifiers.append(identifier); return m_identifiers.last();`.
    fn append(&mut self, identifier: Identifier) -> usize {
        self.identifiers.push(identifier);
        self.identifiers.len() - 1
    }

    /// `makeIdentifier(VM&, std::span<const T>)`.
    pub fn make_identifier<T: CharType>(&mut self, vm: &VM, characters: &[T]) -> Identifier {
        if characters.is_empty() {
            return vm.property_names.empty_identifier.clone();
        }
        let front = characters[0].to_u16() as usize;
        if front >= MAXIMUM_CACHABLE_CHARACTER {
            let index = self.append(Identifier::from_span(vm, characters));
            return self.identifiers[index].clone();
        }
        if characters.len() == 1 {
            if let Some(index) = self.short_identifiers[front] {
                return self.identifiers[index].clone();
            }
            let index = self.append(Identifier::from_span(vm, characters));
            self.short_identifiers[front] = Some(index);
            return self.identifiers[index].clone();
        }
        if let Some(index) = self.recent_identifiers[front] {
            if Identifier::equal(self.identifiers[index].impl_(), characters) {
                return self.identifiers[index].clone();
            }
        }
        let index = self.append(Identifier::from_span(vm, characters));
        self.recent_identifiers[front] = Some(index);
        self.identifiers[index].clone()
    }

    /// `makeIdentifier(VM&, SymbolImpl*)`.
    pub fn make_symbol_identifier(&mut self, symbol: &Identifier) -> Identifier {
        debug_assert!(symbol.is_symbol());
        let index = self.append(symbol.clone());
        self.identifiers[index].clone()
    }

    /// `makeEmptyIdentifier(VM&)`.
    pub fn make_empty_identifier(&self, vm: &VM) -> Identifier {
        vm.property_names.empty_identifier.clone()
    }

    /// `makeLatin1Identifier(VM&, std::span<const char16_t>)`. O ramo de um caractere usa
    /// `fromString` e os demais `createLatin1`, como no C++.
    pub fn make_latin1_identifier(&mut self, vm: &VM, characters: &[u16]) -> Identifier {
        if characters.is_empty() {
            return vm.property_names.empty_identifier.clone();
        }
        let front = characters[0] as usize;
        if front >= MAXIMUM_CACHABLE_CHARACTER {
            let index = self.append(Identifier::create_latin1(vm, characters));
            return self.identifiers[index].clone();
        }
        if characters.len() == 1 {
            if let Some(index) = self.short_identifiers[front] {
                return self.identifiers[index].clone();
            }
            let index = self.append(Identifier::from_span(vm, characters));
            self.short_identifiers[front] = Some(index);
            return self.identifiers[index].clone();
        }
        if let Some(index) = self.recent_identifiers[front] {
            if Identifier::equal(self.identifiers[index].impl_(), characters) {
                return self.identifiers[index].clone();
            }
        }
        let index = self.append(Identifier::create_latin1(vm, characters));
        self.recent_identifiers[front] = Some(index);
        self.identifiers[index].clone()
    }

    /// `makeNumericIdentifier(VM&, double)`.
    pub fn make_numeric_identifier(&mut self, vm: &VM, number: f64) -> Identifier {
        let token = match try_convert_to_strict_int32(number) {
            Some(int32_value) => Identifier::from_i32(vm, int32_value),
            None => Identifier::from_double(vm, number),
        };
        let index = self.append(token);
        self.identifiers[index].clone()
    }

    /// `makeBigIntDecimalIdentifier(VM&, const Identifier&, uint8_t radix)`: o `nullptr` do C++ é
    /// `None`.
    pub fn make_big_int_decimal_identifier(&mut self, vm: &VM, identifier: &Identifier, radix: u8) -> Option<Identifier> {
        if radix == 10 {
            return Some(identifier.clone());
        }

        let _defer_scope = DeferTermination::new(vm);
        let scope = TopExceptionScope::new(vm);
        let big_int = JSBigInt::parse_int(None, vm, &identifier.string(), radix, ErrorParseMode::ThrowExceptions, ParseIntSign::Unsigned);
        scope.assert_no_exception();

        if big_int.is_empty() {
            // Trata falta de memória ou outras falhas devolvendo nulo, já que não há um objeto
            // global para lançar exceções neste escopo.
            return None;
        }

        // FIXME do C++: aloca um JSBigInt só para poder usar `JSBigInt::tryGetString` quando o
        // radix não é 10. Cria pressão sobre o GC, mas só ocorre com literal BigInt como nome de
        // propriedade, o que é raro. https://bugs.webkit.org/show_bug.cgi?id=207627
        //
        // `USE(BIGINT32)` é 0 em PlatformUse.h, então só existe o caminho do BigInt no heap.
        let heap_big_int = big_int.as_heap_big_int();

        let index = self.append(Identifier::from_string(vm, &JSBigInt::try_get_string(vm, heap_big_int, 10)));
        Some(self.identifiers[index].clone())
    }

    /// `makePrivateIdentifier(VM&, ASCIILiteral, unsigned)`.
    pub fn make_private_identifier(&mut self, vm: &VM, prefix: &str, identifier: u32) -> Identifier {
        let symbol_name = format!("{}{}", prefix, identifier);
        let symbol = vm.private_symbol_registry().symbol_for_key(&StringImpl::create(symbol_name.as_bytes()));
        let index = self.append(Identifier::from_uid_symbol(&symbol));
        self.identifiers[index].clone()
    }
}

/// `class ParserArena`. Só carrega o `IdentifierArena` (criado sob demanda).
///
/// O C++ entrega ao `Lexer` um ponteiro cru para o `IdentifierArena` (`m_arena = &arena->identifierArena()`),
/// e o `Lexer` o usa enquanto o `ParserArena` vive. No porte a arena de identificadores é
/// compartilhada (`Rc<RefCell<..>>`): o `Lexer` guarda um clone do `Rc`, sem empréstimo.
#[derive(Default)]
pub struct ParserArena {
    identifier_arena: Option<Rc<RefCell<IdentifierArena>>>,
}

impl ParserArena {
    pub fn new() -> ParserArena {
        ParserArena { identifier_arena: None }
    }

    /// `swap(ParserArena&)`: o C++ também troca os pools e os objetos deletáveis, que não existem
    /// aqui.
    pub fn swap(&mut self, other_arena: &mut ParserArena) {
        std::mem::swap(&mut self.identifier_arena, &mut other_arena.identifier_arena);
    }

    /// `identifierArena()`: a arena compartilhada, criada na primeira chamada. Quem usa a arena
    /// pega o empréstimo com `borrow_mut()` pelo tempo da operação.
    pub fn identifier_arena(&mut self) -> Rc<RefCell<IdentifierArena>> {
        Rc::clone(self.identifier_arena.get_or_insert_with(|| Rc::new(RefCell::new(IdentifierArena::new()))))
    }

    /// `identifierArena()` para quem só tem `&ParserArena` (o `ScopeNode` emprestado do bytecompiler).
    /// O parser sempre criou a arena de identificadores antes de entregar a árvore (o `Lexer` a
    /// pede no construtor), então a criação sob demanda não acontece aqui.
    pub fn shared_identifier_arena(&self) -> Rc<RefCell<IdentifierArena>> {
        Rc::clone(
            self.identifier_arena
                .as_ref()
                .expect("ParserArena entregue pelo parser sem IdentifierArena"),
        )
    }
}
