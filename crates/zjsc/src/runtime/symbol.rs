//! Tradução de `JavaScriptCore/runtime/Symbol.h` e `Symbol.cpp`.
//!
//! DIVERGÊNCIA (heap ausente, camada 3): no C++ `Symbol` é um `JSCell` do GC e o `JSValue` guarda o
//! ponteiro. Aqui o `JSValue::Cell(usize)` guarda o `cell_id` atribuído pelo registro central
//! (`runtime::cell_registry`, espaço único de ids, sem tag própria), que mantém o símbolo vivo, no
//! papel do GC. O `vm.symbolImplToSymbolMap` (`HashMap<SymbolImpl*,
//! Symbol*>` fraco do C++) vive aqui como mapa por thread, chaveado pela identidade do
//! `StringImpl` do símbolo, e fica forte pelo mesmo motivo.
//!
//! Fora desta fatia, porque dependem de `JSGlobalObject`/`ThrowScope`/`SymbolObject`:
//! `toObject`, `toNumber` e o lançamento de `toString`. `to_string` devolve `None` onde o C++
//! chamaria `throwOutOfMemoryError`; quem tem o `JSGlobalObject` lança. `toPrimitive` devolve o
//! próprio símbolo (`to_primitive`). `visitChildren` e `destroy` não existem sem o GC.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::error_type::ErrorTypeWithExtension;
use crate::runtime::js_string::{js_string, JSStringRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::private_name::PrivateName;
use crate::runtime::vm::VM;
use crate::wtf::text::string_concatenate::try_make_string_dyn;
use crate::wtf::text::string_impl::UniquedKey;
use crate::wtf::text::symbol_impl::{RegisteredSymbolImpl, SymbolImpl, S_FLAG_DEFAULT};
use crate::wtf::text::wtf_string::String as WtfString;

/// `class Symbol final : public JSCell`.
#[derive(Debug)]
pub struct Symbol {
    m_private_name: PrivateName,
    m_description: RefCell<Option<JSStringRef>>,
    m_string: RefCell<Option<JSStringRef>>,
    cell_id: usize,
}

/// Referência compartilhada, o `Symbol*` do C++.
pub type SymbolRef = Rc<Symbol>;

thread_local! {
    /// `vm.symbolImplToSymbolMap`: a chave é a identidade do `StringImpl` do `SymbolImpl`.
    static SYMBOL_IMPL_TO_SYMBOL_MAP: RefCell<HashMap<usize, SymbolRef>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): cada `Symbol` do mapa é uma célula (`cell_id`)
/// do programa; no C++ o mapa vive no `VM` e morre com ele.
pub(crate) fn reset_for_program() {
    let taken = SYMBOL_IMPL_TO_SYMBOL_MAP.try_with(|map| std::mem::take(&mut *map.borrow_mut()));
    drop(taken);
}

/// A chave do `symbolImplToSymbolMap` para um `SymbolImpl*`.
fn map_key(uid: &SymbolImpl) -> usize {
    Rc::as_ptr(uid.string_impl()) as usize
}

impl Symbol {
    /// `Symbol(VM&...)` seguido de `allocateCell` e `finishCreation`: registra o símbolo e faz
    /// `vm.symbolImplToSymbolMap.set(&m_privateName.uid(), this)`.
    fn allocate(
        private_name: PrivateName,
        description: Option<JSStringRef>,
    ) -> SymbolRef {
        let cell_id = cell_registry::reserve();
        let symbol = Rc::new(Symbol {
            m_private_name: private_name,
            m_description: RefCell::new(description),
            m_string: RefCell::new(None),
            cell_id,
        });
        cell_registry::set(cell_id, CellEntry::Symbol(Rc::clone(&symbol)));
        SYMBOL_IMPL_TO_SYMBOL_MAP.with(|map| {
            map.borrow_mut().insert(map_key(symbol.uid()), Rc::clone(&symbol));
        });
        symbol
    }

    /// `Symbol::create(VM&)`: o símbolo sem descrição (`SymbolImpl::createNullSymbol()`).
    pub fn create(_vm: &VM) -> SymbolRef {
        Symbol::allocate(PrivateName::new(SymbolImpl::create_null_symbol()), None)
    }

    /// `Symbol::createWithDescription(VM&, const String&)`.
    pub fn create_with_description(_vm: &VM, description: &WtfString) -> SymbolRef {
        Symbol::allocate(PrivateName::with_description(description_impl(description)), None)
    }

    /// `Symbol::createWithDescription(VM&, const String&, JSString*)`.
    pub fn create_with_description_and_string(_vm: &VM, description: &WtfString, string: JSStringRef) -> SymbolRef {
        Symbol::allocate(PrivateName::with_description(description_impl(description)), Some(string))
    }

    /// `Symbol::create(vm, PrivateSymbolImpl::create(*description.impl()).get())` do `createPrivateSymbol`
    /// (`JSGlobalObject.cpp`): um símbolo privado novo, sem descrição própria além da do `uid`.
    pub fn create_private(_vm: &VM, description: &WtfString) -> SymbolRef {
        Symbol::allocate(PrivateName::with_private_symbol(description_impl(description)), None)
    }

    /// `Symbol::create(VM&, SymbolImpl& uid)`: reaproveita o símbolo que já existe para o `uid`.
    pub fn create_with_uid(_vm: &VM, uid: &Rc<SymbolImpl>) -> SymbolRef {
        let existing = SYMBOL_IMPL_TO_SYMBOL_MAP.with(|map| map.borrow().get(&map_key(uid)).cloned());
        if let Some(symbol) = existing {
            return symbol;
        }
        Symbol::allocate(PrivateName::new(Rc::clone(uid)), None)
    }

    /// `Symbol::create(VM&, SymbolImpl& uid)` para o símbolo registrado de `Symbol.for`
    /// (`Symbol::create(vm, symbolRegistry.symbolForKey(string))`): reaproveita o símbolo do `uid`.
    pub fn create_with_registered_uid(_vm: &VM, uid: &Rc<RegisteredSymbolImpl>) -> SymbolRef {
        let existing = SYMBOL_IMPL_TO_SYMBOL_MAP.with(|map| map.borrow().get(&map_key(uid)).cloned());
        if let Some(symbol) = existing {
            return symbol;
        }
        Symbol::allocate(PrivateName::with_registered_symbol(Rc::clone(uid)), None)
    }

    /// O `Symbol` que já existe para uma chave de propriedade de símbolo (o `vm.symbolImplToSymbolMap`
    /// do C++, consultado pelo `UniquedStringImpl*`).
    pub fn find_by_key(key: &UniquedKey) -> Option<SymbolRef> {
        SYMBOL_IMPL_TO_SYMBOL_MAP.with(|map| map.borrow().get(&(Rc::as_ptr(&key.0) as usize)).cloned())
    }

    /// `Symbol::create(vm, static_cast<SymbolImpl&>(*key))`: o `Symbol` da chave, criado sobre o
    /// `StringImpl` dela quando ainda não existe (ver `SymbolImpl::adopt`). `flags` são os do
    /// `SymbolImpl` original (`S_FLAG_DEFAULT` para os símbolos conhecidos).
    pub fn for_key(vm: &VM, key: &UniquedKey) -> SymbolRef {
        if let Some(symbol) = Symbol::find_by_key(key) {
            return symbol;
        }
        Symbol::create_with_uid(vm, &SymbolImpl::adopt(&key.0, S_FLAG_DEFAULT))
    }

    /// Procura o símbolo pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<SymbolRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::Symbol(symbol)) => Some(symbol),
            _ => None,
        }
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// `uid()`.
    pub fn uid(&self) -> &SymbolImpl {
        self.m_private_name.uid()
    }

    /// `privateName()`.
    pub fn private_name(&self) -> PrivateName {
        self.m_private_name.clone()
    }

    /// `description(VM&)`: `None` é o `nullptr` do símbolo nulo.
    pub fn description(&self, vm: &VM) -> Option<JSStringRef> {
        if let Some(string) = self.m_description.borrow().as_ref() {
            return Some(Rc::clone(string));
        }

        let uid = self.uid();
        if uid.is_null_symbol() {
            return None;
        }

        let string = js_string(vm, &WtfString::from(Rc::clone(uid.string_impl())));
        *self.m_description.borrow_mut() = Some(Rc::clone(&string));
        Some(string)
    }

    /// `toPrimitive(JSGlobalObject*, PreferredPrimitiveType)`: o próprio símbolo.
    pub fn to_primitive(&self) -> JSValue {
        JSValue::from_cell(self.cell_id)
    }

    /// `toString(JSGlobalObject*)` sem o lançamento: `None` onde o C++ faz `throwOutOfMemoryError`.
    pub fn to_string(&self, vm: &VM) -> Option<JSStringRef> {
        if let Some(string) = self.m_string.borrow().as_ref() {
            return Some(Rc::clone(string));
        }

        let description = self.try_get_descriptive_string().ok()?;
        let string = js_string(vm, &description);
        debug_assert!(!string.is_rope());
        *self.m_string.borrow_mut() = Some(Rc::clone(&string));
        Some(string)
    }

    /// `tryGetDescriptiveString()`: `Symbol(descrição)`.
    pub fn try_get_descriptive_string(&self) -> Result<WtfString, ErrorTypeWithExtension> {
        let uid = WtfString::from(Rc::clone(self.uid().string_impl()));
        try_make_string_dyn(&[&"Symbol(", &uid, &')']).ok_or(ErrorTypeWithExtension::OutOfMemoryError)
    }
}

/// `asSymbol(JSValue)`: invariante de `isSymbol`, como o `ASSERT` do C++.
pub fn as_symbol(value: JSValue) -> SymbolRef {
    match value {
        JSValue::Cell(cell_id) => Symbol::from_cell_id(cell_id).expect("as_symbol em célula que não é Symbol"),
        _ => unreachable!("as_symbol em valor que não é célula"),
    }
}

/// O `StringImpl` de um `const String&` de descrição. A string nula do WTF não chega aqui: o C++
/// também a desreferencia (`description.impl()` com `*`) em `PrivateName(DescriptionTag, ...)`.
fn description_impl(description: &WtfString) -> &Rc<crate::wtf::text::string_impl::StringImpl> {
    description.impl_().expect("Symbol com descrição nula")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn description(text: &[u8]) -> WtfString {
        WtfString::from_latin1(text)
    }

    #[test]
    fn create_has_no_description() {
        let vm = VM::new();
        let symbol = Symbol::create(&vm);
        assert!(symbol.uid().is_null_symbol());
        assert!(symbol.description(&vm).is_none());
        assert_eq!(symbol.try_get_descriptive_string().unwrap().span8(), b"Symbol()");
    }

    #[test]
    fn description_and_to_string_are_cached() {
        let vm = VM::new();
        let symbol = Symbol::create_with_description(&vm, &description(b"foo"));
        let first = symbol.description(&vm).unwrap();
        assert!(Rc::ptr_eq(&first, &symbol.description(&vm).unwrap()));
        assert_eq!(first.value().span8(), b"foo");
        let string = symbol.to_string(&vm).unwrap();
        assert_eq!(string.value().span8(), b"Symbol(foo)");
        assert!(Rc::ptr_eq(&string, &symbol.to_string(&vm).unwrap()));
    }

    #[test]
    fn create_with_uid_reuses_the_symbol() {
        let vm = VM::new();
        let uid = SymbolImpl::create(&crate::wtf::text::string_impl::StringImpl::create(b"x"));
        let a = Symbol::create_with_uid(&vm, &uid);
        let b = Symbol::create_with_uid(&vm, &uid);
        assert!(Rc::ptr_eq(&a, &b));
        let other = Symbol::create_with_description(&vm, &description(b"x"));
        assert!(!Rc::ptr_eq(&a, &other));
    }

    #[test]
    fn cell_id_round_trips_through_js_value() {
        let vm = VM::new();
        let symbol = Symbol::create(&vm);
        let value = symbol.to_primitive();
        assert!(Rc::ptr_eq(&as_symbol(value), &symbol));
        assert!(Symbol::from_cell_id(symbol.cell_id() ^ 1).is_none());
        assert!(crate::runtime::js_string::JSString::from_cell_id(symbol.cell_id()).is_none());
    }
}
