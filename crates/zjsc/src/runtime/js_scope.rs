//! Porte de `runtime/JSScope.h`, `JSScope.cpp` e `JSScopeInlines.h`.
//!
//! DIVERGÊNCIAS (heap ausente, camada 3; mesmo padrão de `js_string.rs` e `js_cell_butterfly.rs`):
//!
//! - `JSScope*` é polimórfico no C++ (qualquer subclasse). Aqui é o `enum JSScopeRef`, uma variante
//!   por subclasse já portada (`JSLexicalEnvironment`, `JSGlobalLexicalEnvironment`, `JSGlobalObject`,
//!   `JSWithScope`). Para o `with`, `objectAtScope` devolve o próprio `JSWithScope` e quem consulta
//!   propriedades (`has_property`, `get_property_slot`, `put`, `isUnscopable`) desembrulha o objeto
//!   (`js_with_scope.rs`); `symbol_table()` dele é `None`. O `JSModuleEnvironment`
//!   já é variante, mas o ramo de import de módulo em `collectClosureVariablesUnderTDZ` e o
//!   `resolveImport` de `abstractAccess` esperam o `AbstractModuleRecord` ter registro de células.
//! - O `JSValue::Cell(usize)` de um escopo guarda o `cell_id` do registro central
//!   (`runtime::cell_registry`, `CellEntry::Scope`), que mantém o escopo vivo (sem coleta).
//! - `ConcurrentJSLocker` some (um só mutador): o `RefCell` do `SymbolTable` faz o papel do lock.
//! - `abstractResolve`/`abstractAccess` entram com `ResolveOp` (`get_put_info`). Divergências: o
//!   `operand` de `GlobalVar`/`GlobalLexicalVar` é o `scopeOffset` (o C++ guarda o endereço do slot);
//!   `SymbolTableEntry::watchpointSet()` é sempre nulo no porte; no ramo de propriedade do global
//!   object falta o `ensurePropertyReplacementWatchpointSet` e o `propertyAccessesAreCacheable`, então
//!   `GlobalProperty` sai sempre sem `Structure` (o resultado "sem cache" do C++, que o interpretador
//!   resolve pelo caminho lento), e a propriedade própria é conferida na `Structure` (a tabela estática
//!   preguiçosa ainda não entra).
//! - `constantScopeForCodeBlock(type, CodeBlock*)` recebe o `JSGlobalObject` do `CodeBlock`
//!   (`codeBlock->globalObject()`), o único campo que ela lê.

use std::cell::RefCell;
use std::rc::Rc;

use crate::parser::variable_environment::{PrivateNameEnvironment, TDZEnvironment};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::get_put_info::{
    is_initialization, make_type, needs_var_injection_checks, GetOrPut, InitializationMode, ResolveOp, ResolveType,
};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_lexical_environment::JSGlobalLexicalEnvironment;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_lexical_environment::JSLexicalEnvironment;
use crate::runtime::js_module_environment::JSModuleEnvironmentRef;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, PutError, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::error::{create_out_of_memory_error, create_stack_overflow_error, create_type_error};
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot};
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::wtf::text::wtf_string::String as WtfString;
use crate::runtime::js_symbol_table_object::{symbol_table_get, symbol_table_put, JSSymbolTableObject, SymbolTablePut};
use crate::wtf::text::string_impl::UniquedKey;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::{js_undefined, JSValue};
use crate::runtime::js_with_scope::JSWithScopeRef;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::is_valid_offset;
use crate::runtime::structure::StructureRef;
use crate::runtime::symbol_table::{ScopeType, SymbolTableRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSScope::s_info`.
pub static JS_SCOPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Scope", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSScope*`: qualquer escopo vivo.
#[derive(Clone, Debug)]
pub enum JSScopeRef {
    LexicalEnvironment(Rc<JSLexicalEnvironment>),
    ModuleEnvironment(JSModuleEnvironmentRef),
    GlobalLexicalEnvironment(Rc<JSGlobalLexicalEnvironment>),
    GlobalObject(Rc<JSGlobalObject>),
    WithScope(JSWithScopeRef),
}

/// `class JSScope : public JSNonFinalObject`.
#[derive(Debug)]
pub struct JSScope {
    base: JSNonFinalObject,
    cell_id: usize,
    /// `m_next`.
    next: RefCell<Option<JSScopeRef>>,
}

impl std::ops::Deref for JSScope {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSScope {
    /// `Base::StructureFlags` (`JSScope` não acrescenta nada).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `JSScope(VM&, Structure*, JSScope* next)`. Reserva o `cell_id` no registro central; quem constrói
    /// a subclasse completa o cadastro com `cell_registry::set(.., CellEntry::Scope(..))`.
    pub(crate) fn new(vm: &VM, structure: StructureRef, next: Option<JSScopeRef>) -> JSScope {
        let cell_id = cell_registry::reserve();
        let base = JSNonFinalObject::new(vm, structure);
        // A base `JSObject` é a mesma célula: `as_value` e `isThisValueAltered` dependem do `cell_id`.
        base.set_cell_id(cell_id);
        JSScope { base, cell_id, next: RefCell::new(next) }
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// Procura o escopo pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSScopeRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::Scope(scope)) => Some(scope),
            _ => None,
        }
    }

    /// `jsCast<JSModuleEnvironment*>(scope)`.
    pub fn as_module_environment(scope: &JSScopeRef) -> JSModuleEnvironmentRef {
        match scope {
            JSScopeRef::ModuleEnvironment(environment) => Rc::clone(environment),
            _ => unreachable!("jsCast<JSModuleEnvironment*> em escopo que não é de módulo"),
        }
    }

    /// `next()`.
    pub fn next(&self) -> Option<JSScopeRef> {
        self.next.borrow().clone()
    }

    /// Corta o elo para o escopo seguinte. Só o desmonte do programa usa (`cell_registry::remove_all_of`): no C++
    /// o `~VM` destrói o heap inteiro e o ciclo escopo léxico -> global -> escopo léxico não existe como posse.
    pub(crate) fn clear_next(&self) {
        let released = self.next.borrow_mut().take();
        drop(released);
    }

    /// `realm()` (`JSCell::realm()`): o `m_realm` da estrutura, que todo escopo tem.
    pub fn realm(&self) -> JSGlobalObjectRef {
        self.structure().realm().expect("a estrutura de um escopo sempre tem realm")
    }

    /// `offsetOfNext()` não existe: o interpretador lê `next()`.
    ///
    /// `objectAtScope(JSScope*)`: o objeto onde as variáveis do escopo vivem. É o próprio escopo, inclusive
    /// para o `with` (o `JSScopeRef` só carrega escopos; veja o cabeçalho do módulo).
    pub fn object_at_scope(scope: &JSScopeRef) -> JSScopeRef {
        scope.clone()
    }
}

impl JSScopeRef {
    /// `Rc::strong_count` da subclasse (ver `CellEntry::strong_count`).
    pub(crate) fn strong_count(&self) -> usize {
        match self {
            JSScopeRef::LexicalEnvironment(scope) => Rc::strong_count(scope),
            JSScopeRef::ModuleEnvironment(scope) => Rc::strong_count(scope),
            JSScopeRef::GlobalLexicalEnvironment(scope) => Rc::strong_count(scope),
            JSScopeRef::GlobalObject(scope) => Rc::strong_count(scope),
            JSScopeRef::WithScope(scope) => Rc::strong_count(scope),
        }
    }

    /// `visitChildren` de cada escopo: as variáveis, o objeto do `with` e o estado do objeto global (a base
    /// `JSObject` e o `next` por `Rc` o coletor já cobre).
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        match self {
            JSScopeRef::LexicalEnvironment(scope) => scope.visit_variables(marker),
            JSScopeRef::ModuleEnvironment(scope) => scope.visit_variables(marker),
            JSScopeRef::GlobalLexicalEnvironment(scope) => scope.visit_variables(marker),
            JSScopeRef::GlobalObject(global_object) => global_object.visit_children(marker),
            JSScopeRef::WithScope(scope) => scope.visit_children(marker),
        }
    }

    /// Acesso à base `JSScope` da subclasse.
    pub fn scope(&self) -> &JSScope {
        match self {
            JSScopeRef::WithScope(with_scope) => with_scope,
            _ => self.symbol_table_object(),
        }
    }

    /// Toda subclasse portada, menos `JSWithScope`, é um `JSSymbolTableObject`.
    pub fn symbol_table_object(&self) -> &JSSymbolTableObject {
        match self {
            JSScopeRef::LexicalEnvironment(environment) => environment,
            JSScopeRef::ModuleEnvironment(environment) => environment,
            JSScopeRef::GlobalLexicalEnvironment(environment) => environment,
            JSScopeRef::GlobalObject(global_object) => global_object,
            JSScopeRef::WithScope(_) => unreachable!("JSWithScope não é um JSSymbolTableObject"),
        }
    }

    /// `JSValue(JSCell*)`: o escopo como célula do registro central.
    pub fn into_js_value(&self) -> JSValue {
        JSValue::from_cell(self.cell_id())
    }

    pub fn cell_id(&self) -> usize {
        self.scope().cell_id()
    }

    /// Igualdade de ponteiro.
    pub fn ptr_eq(&self, other: &JSScopeRef) -> bool {
        self.cell_id() == other.cell_id()
    }

    /// `type()`: o `JSType` do `TypeInfo` da estrutura.
    pub fn js_type(&self) -> JSType {
        match self {
            JSScopeRef::LexicalEnvironment(_) => JSType::LexicalEnvironmentType,
            JSScopeRef::ModuleEnvironment(_) => JSType::ModuleEnvironmentType,
            JSScopeRef::GlobalLexicalEnvironment(_) => JSType::GlobalLexicalEnvironmentType,
            JSScopeRef::GlobalObject(_) => JSType::GlobalObjectType,
            JSScopeRef::WithScope(with_scope) if with_scope.is_strict_eval_activation() => JSType::StrictEvalActivationType,
            JSScopeRef::WithScope(_) => JSType::WithScopeType,
        }
    }

    /// `isJSLexicalEnvironment()` (`LexicalEnvironmentType` ou `ModuleEnvironmentType`).
    pub fn is_js_lexical_environment(&self) -> bool {
        matches!(self, JSScopeRef::LexicalEnvironment(_) | JSScopeRef::ModuleEnvironment(_))
    }

    /// `isGlobalLexicalEnvironment()`.
    pub fn is_global_lexical_environment(&self) -> bool {
        matches!(self, JSScopeRef::GlobalLexicalEnvironment(_))
    }

    /// `isGlobalObject()`.
    pub fn is_global_object(&self) -> bool {
        matches!(self, JSScopeRef::GlobalObject(_))
    }

    /// `next()`.
    pub fn next(&self) -> Option<JSScopeRef> {
        self.scope().next()
    }

    /// `begin()`.
    pub fn begin(&self) -> ScopeChainIterator {
        ScopeChainIterator::new(Some(self.clone()))
    }

    /// `end()`.
    pub fn end(&self) -> ScopeChainIterator {
        ScopeChainIterator::new(None)
    }

    /// `symbolTable()` (JSScope.cpp:416): nula se o escopo não é `JSSymbolTableObject`, o caso do
    /// `JSWithScope`.
    pub fn symbol_table(&self) -> Option<SymbolTableRef> {
        if matches!(self, JSScopeRef::WithScope(_)) {
            return None;
        }
        Some(self.symbol_table_object().symbol_table())
    }

    /// `realm()`.
    pub fn realm(&self) -> JSGlobalObjectRef {
        self.scope().realm()
    }

    /// `globalThis()` (JSGlobalObject.h:1421).
    pub fn global_this(&self) -> Option<crate::runtime::js_object::JSObjectHandle> {
        self.realm().global_this()
    }

    /// `isVarScope()`.
    pub fn is_var_scope(&self) -> bool {
        let JSScopeRef::LexicalEnvironment(environment) = self else { return false };
        environment.symbol_table().borrow().scope_type() == ScopeType::VarScope
    }

    /// `isLexicalScope()`.
    pub fn is_lexical_scope(&self) -> bool {
        let JSScopeRef::LexicalEnvironment(environment) = self else { return false };
        environment.symbol_table().borrow().scope_type() == ScopeType::LexicalScope
    }

    /// `isModuleScope()`: `type() == ModuleEnvironmentType`.
    pub fn is_module_scope(&self) -> bool {
        matches!(self, JSScopeRef::ModuleEnvironment(_))
    }

    /// `isWithScope()`: `type() == WithScopeType` (o `StrictEvalActivation`, que é um `JSWithScope` marcado,
    /// não conta).
    pub fn is_with_scope(&self) -> bool {
        matches!(self, JSScopeRef::WithScope(with_scope) if !with_scope.is_strict_eval_activation())
    }

    /// `StrictEvalActivation`: `type() == StrictEvalActivationType`.
    pub fn is_strict_eval_activation(&self) -> bool {
        matches!(self, JSScopeRef::WithScope(with_scope) if with_scope.is_strict_eval_activation())
    }

    /// `isCatchScope()`.
    pub fn is_catch_scope(&self) -> bool {
        let JSScopeRef::LexicalEnvironment(environment) = self else { return false };
        let scope_type = environment.symbol_table().borrow().scope_type();
        scope_type == ScopeType::CatchScope || scope_type == ScopeType::CatchScopeWithSimpleParameter
    }

    /// `isCatchScopeWithSimpleParameter()`.
    pub fn is_catch_scope_with_simple_parameter(&self) -> bool {
        let JSScopeRef::LexicalEnvironment(environment) = self else { return false };
        environment.symbol_table().borrow().scope_type() == ScopeType::CatchScopeWithSimpleParameter
    }

    /// `isFunctionNameScopeObject()`.
    pub fn is_function_name_scope_object(&self) -> bool {
        let JSScopeRef::LexicalEnvironment(environment) = self else { return false };
        environment.symbol_table().borrow().scope_type() == ScopeType::FunctionNameScope
    }

    /// `isNestedLexicalScope()`.
    pub fn is_nested_lexical_scope(&self) -> bool {
        let JSScopeRef::LexicalEnvironment(environment) = self else { return false };
        environment.symbol_table().borrow().is_nested_lexical_scope()
    }

    /// `symbolTableGet(object, key, slot)` sobre a variante do escopo: `Some((valor, atributos))` se a
    /// `SymbolTable` do escopo tem a chave.
    pub fn symbol_table_get(&self, key: &UniquedKey) -> Option<(JSValue, u32)> {
        match self {
            JSScopeRef::LexicalEnvironment(environment) => symbol_table_get(&**environment, key),
            JSScopeRef::ModuleEnvironment(environment) => environment.symbol_table_get_with_imports(key),
            JSScopeRef::GlobalLexicalEnvironment(environment) => symbol_table_get(&***environment, key),
            JSScopeRef::GlobalObject(global_object) => symbol_table_get(&***global_object, key),
            // `JSWithScope` não é `JSSymbolTableObject`: não há tabela a consultar.
            JSScopeRef::WithScope(_) => None,
        }
    }

    /// `symbolTablePut(object, key, value, shouldThrow, ignoreReadOnlyErrors)` sobre a variante do escopo.
    pub fn symbol_table_put(
        &self,
        key: &UniquedKey,
        value: JSValue,
        should_throw_read_only_error: bool,
        ignore_read_only_errors: bool,
    ) -> SymbolTablePut {
        match self {
            JSScopeRef::LexicalEnvironment(environment) => {
                // `JSLexicalEnvironment::put`: `slot.isStrictMode() || thisObject->isLexicalScope()`.
                let should_throw = should_throw_read_only_error || self.is_lexical_scope();
                symbol_table_put(&**environment, key, value, should_throw, ignore_read_only_errors)
            }
            JSScopeRef::ModuleEnvironment(environment) => {
                environment.symbol_table_put_with_imports(key, value, should_throw_read_only_error, ignore_read_only_errors)
            }
            JSScopeRef::GlobalLexicalEnvironment(environment) => {
                // `JSGlobalLexicalEnvironment::put`: `alwaysThrowWhenAssigningToConstProperty = true`.
                symbol_table_put(&***environment, key, value, true, ignore_read_only_errors)
            }
            JSScopeRef::GlobalObject(global_object) => {
                symbol_table_put(&***global_object, key, value, should_throw_read_only_error, ignore_read_only_errors)
            }
            JSScopeRef::WithScope(_) => SymbolTablePut::NotFound,
        }
    }

    /// `object->hasProperty(globalObject, ident)` sobre o objeto do escopo. A consulta própria que as
    /// subclasses sobrescrevem (`getOwnPropertySlot` por `symbolTableGet`) vem primeiro; o resto é o
    /// `JSObject::hasProperty` da base.
    pub fn has_property(&self, global_object: &JSGlobalObject, ident: &Identifier) -> bool {
        if let Some(uid) = ident.impl_() {
            if self.symbol_table_get(&uid).is_some() {
                return true;
            }
        }
        match self {
            // `JSGlobalLexicalEnvironment::getOwnPropertySlot` só consulta a tabela e não tem protótipo.
            JSScopeRef::GlobalLexicalEnvironment(_) => false,
            // `objectAtScope` do `with` é o objeto embrulhado (veja `js_with_scope.rs`).
            // Pelo `ObjectRef`: o `getOwnPropertySlot` da `JSFunction` materializa `length`/`name`/`prototype`.
            JSScopeRef::WithScope(with_scope) => ObjectRef::from_value(&with_scope.object_value()).is_some_and(|object| {
                let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::HasProperty);
                object.get_property_slot(global_object, &PropertyName::from_identifier(ident), &mut slot)
            }),
            _ => self.scope().has_property(global_object.vm(), &PropertyName::from_identifier(ident)),
        }
    }

    /// `deleteProperty(scope, globalObject, ident)` do escopo resolvido (o `del_by_id` de `delete x`):
    /// `JSSymbolTableObject::deleteProperty` devolve falso para binding da `SymbolTable` (`var` de função,
    /// parâmetro, `let`, `const`, função, `arguments`); o `StrictEvalActivation` devolve falso; o `with`
    /// apaga do objeto embrulhado; o resto é o `JSObject::deleteProperty` da base (o `var` criado por `eval`
    /// sloppy é propriedade comum e some).
    pub fn delete_property(&self, global_object: &JSGlobalObject, ident: &Identifier) -> Result<bool, PutError> {
        if let Some(uid) = ident.impl_() {
            let in_table = match self {
                JSScopeRef::WithScope(_) => false,
                _ => self.symbol_table().is_some_and(|table| table.borrow().contains(&uid)),
            };
            if in_table {
                return Ok(false);
            }
        }
        // `JSLexicalEnvironment::deleteProperty`: `if (propertyName == vm.propertyNames->arguments) return false;`
        // (vale para o `var arguments` que um `eval` sloppy cria no escopo de uma arrow, que não declara `arguments`).
        if matches!(self, JSScopeRef::LexicalEnvironment(_) | JSScopeRef::ModuleEnvironment(_)) && *ident == global_object.vm().property_names.arguments {
            return Ok(false);
        }
        let name = PropertyName::from_identifier(ident);
        let mut slot = crate::runtime::delete_property_slot::DeletePropertySlot::default();
        match self {
            JSScopeRef::WithScope(with_scope) if with_scope.is_strict_eval_activation() => Ok(false),
            JSScopeRef::WithScope(with_scope) => match with_scope.object() {
                // Pelo `ObjectRef`: o `deleteProperty` da `JSFunction` materializa `length`/`name`/`prototype`.
                Some(object) => match ObjectRef::from_value(&object.as_value()) {
                    Some(ObjectRef::Function(function)) => function.delete_property(global_object, &name, &mut slot),
                    _ => object.delete_property(global_object.vm(), &name, &mut slot),
                },
                None => unreachable!("JSWithScope::object(): a célula embrulhada é um objeto (toObject no with)"),
            },
            _ => self.scope().delete_property(global_object.vm(), &name, &mut slot),
        }
    }

    /// `scope->getPropertySlot(globalObject, ident, slot)` seguido de `slot.getValue(...)`: `Some(valor)`
    /// é o `found`. A consulta própria que as subclasses sobrescrevem (`getOwnPropertySlot` por
    /// `symbolTableGet`) vem primeiro; o resto é o `JSObject::getPropertySlot` da base.
    pub fn get_property_slot(&self, global_object: &JSGlobalObject, ident: &Identifier) -> Option<JSValue> {
        if let Some(uid) = ident.impl_() {
            if let Some((value, _attributes)) = self.symbol_table_get(&uid) {
                return Some(value);
            }
        }
        if matches!(self, JSScopeRef::GlobalLexicalEnvironment(_)) {
            // `JSGlobalLexicalEnvironment::getOwnPropertySlot` só consulta a tabela e não tem protótipo.
            return None;
        }
        // `objectAtScope` do `with` é o objeto embrulhado (veja `js_with_scope.rs`).
        let with_object = match self {
            JSScopeRef::WithScope(with_scope) => Some(with_scope.object()?),
            _ => None,
        };
        let object: &JSObject = match &with_object {
            Some(object) => object,
            None => self.scope(),
        };
        let this_value = match &with_object {
            Some(object) => object.as_value(),
            None => self.into_js_value(),
        };
        get_property_slot_on_object(global_object, object, this_value, ident)
    }

    /// `scope->methodTable()->put(scope, globalObject, ident, value, slot)` para o que a `SymbolTable`
    /// não tem: o `JSObject::put` da base, com o `PutPropertySlot(scope, isStrict, context,
    /// isInitialization)`; o `context` vem do chamador (`UnknownContext` no `slow_path_put_to_scope`,
    /// `PutById`/`PutByIdEval` no `put_by_id`). O erro que o C++ lança (`typeError`, estouro de
    /// pilha, falta de memória) é lançado na VM e o retorno é `Ok(false)`, como o `false` do C++ com exceção
    /// pendente; só o `Unported` volta como `Err`.
    pub fn put(
        &self,
        global_object: &JSGlobalObject,
        ident: &Identifier,
        value: JSValue,
        is_strict_mode: bool,
        context: PutContext,
        is_initialization: bool,
    ) -> Result<bool, PutError> {
        // `objectAtScope` do `with` é o objeto embrulhado (veja `js_with_scope.rs`).
        let with_object = match self {
            JSScopeRef::WithScope(with_scope) => Some(with_scope.object().expect("JSWithScope::object(): a célula embrulhada é um objeto (toObject no with)")),
            _ => None,
        };
        let object: &JSObject = match &with_object {
            Some(object) => object,
            None => self.scope(),
        };
        let this_value = match &with_object {
            Some(object) => object.as_value(),
            None => self.into_js_value(),
        };
        put_on_object(global_object, object, this_value, ident, value, is_strict_mode, context, is_initialization)
    }
}

/// `scope->getPropertySlot(globalObject, ident, slot)` seguido de `slot.getValue(...)` sobre um objeto
/// qualquer. É também o caminho do `get_from_scope` cujo escopo é o objeto do `with` que o
/// `slow_path_resolve_scope` devolve (`JSScope::objectAtScope`), que não é um `JSScope`.
pub fn get_property_slot_on_object(
    global_object: &JSGlobalObject,
    object: &JSObject,
    this_value: JSValue,
    ident: &Identifier,
) -> Option<JSValue> {
    let mut slot = PropertySlot::new(this_value, InternalMethodType::Get);
    let property_name = PropertyName::from_identifier(ident);
    // Pelo `ObjectRef`, para a `JSFunction` materializar `length`/`name`/`prototype` (`with (f) { length }`).
    let found = match ObjectRef::from_value(&object.as_value()) {
        Some(object_ref) => object_ref.get_property_slot(global_object, &property_name, &mut slot),
        None => object.get_property_slot(global_object.vm(), &property_name, &mut slot),
    };
    if found {
        return Some(slot.get_value_for(&property_name));
    }
    None
}

/// `object->methodTable()->put(object, globalObject, ident, value, slot)` do `slow_path_put_to_scope`
/// (ver `JSScopeRef::put`); também o caminho do escopo que é o objeto do `with`.
pub fn put_on_object(
    global_object: &JSGlobalObject,
    object: &JSObject,
    this_value: JSValue,
    ident: &Identifier,
    value: JSValue,
    is_strict_mode: bool,
    context: PutContext,
    is_initialization: bool,
) -> Result<bool, PutError> {
    let mut slot = PutPropertySlot::new(this_value, is_strict_mode, context, is_initialization);
    let property_name = PropertyName::from_identifier(ident);
    // `methodTable()->put` do objeto: o `ProxyObject::put` e o `JSGlobalProxy::put` (repassa ao alvo)
    // sobrescrevem `getPrototype`, e o `JSObject::put` da base não os trata para o próprio `this`.
    let result = if object.type_() == crate::runtime::js_type::JSType::ProxyObjectType {
        crate::runtime::proxy_object::put_from_proxy(object, &property_name, value, &mut slot)
    } else if let Some(target) = crate::runtime::js_global_proxy::target_of(object) {
        // `JSGlobalProxy::put` repassa ao `methodTable()->put` do alvo: o `JSGlobalObject::put`.
        JSGlobalObject::put(&target, global_object.vm(), &property_name, value, &mut slot)
    } else if let (crate::runtime::js_type::JSType::GlobalObjectType, Some(JSScopeRef::GlobalObject(target))) =
        (object.type_(), JSScope::from_cell_id(object.cell_id()))
    {
        // `with (globalThis) { x = 1 }`: o `methodTable()->put` do objeto global é o `JSGlobalObject::put`
        // (`symbolTablePut` antes do `Base::put`), então a escrita chega ao slot da `SymbolTable`.
        JSGlobalObject::put(&target, global_object.vm(), &property_name, value, &mut slot)
    } else if let Some(function) = object.as_value().as_js_function() {
        // `JSFunction::put`: `prototype` e as propriedades preguiçosas (`length`/`name`) materializam antes.
        function.put(global_object, &property_name, value, &mut slot)
    } else {
        object.put(global_object.vm(), &property_name, value, &mut slot)
    };
    let thrown = match result {
        Ok(done) => return Ok(done),
        Err(PutError::TypeError(message)) => create_type_error(global_object, &WtfString::from_latin1(message.as_bytes())),
        Err(PutError::StackOverflow) => create_stack_overflow_error(global_object),
        Err(PutError::OutOfMemory) => create_out_of_memory_error(global_object),
        Err(unported) => return Err(unported),
    };
    let mut throw_scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut throw_scope, thrown);
    Ok(false)
}

/// `class ScopeChainIterator`.
#[derive(Clone, Debug)]
pub struct ScopeChainIterator {
    node: Option<JSScopeRef>,
}

impl ScopeChainIterator {
    pub fn new(node: Option<JSScopeRef>) -> ScopeChainIterator {
        ScopeChainIterator { node }
    }

    /// `get()` (JSScopeInlines.h): `JSScope::objectAtScope(m_node)`.
    pub fn get(&self) -> JSScopeRef {
        JSScope::object_at_scope(self.node.as_ref().expect("ScopeChainIterator::get no fim da cadeia"))
    }

    /// `scope()`.
    pub fn scope(&self) -> Option<&JSScopeRef> {
        self.node.as_ref()
    }

    /// `operator++()`.
    pub fn advance(&mut self) {
        self.node = self.node.as_ref().and_then(|node| node.next());
    }
}

impl PartialEq for ScopeChainIterator {
    fn eq(&self, other: &ScopeChainIterator) -> bool {
        match (&self.node, &other.node) {
            (None, None) => true,
            (Some(a), Some(b)) => a.ptr_eq(b),
            _ => false,
        }
    }
}

/// `isUnscopable` (JSScope.cpp:140). Só o escopo `with` consulta `Symbol.unscopables`
/// (`scope->type() != WithScopeType` devolve falso); `object` é o escopo `with` que `objectAtScope`
/// devolve (veja `js_with_scope.rs`), de onde sai o objeto embrulhado.
fn is_unscopable(global_object: &JSGlobalObject, scope: &JSScopeRef, _object: &JSScopeRef, ident: &Identifier) -> bool {
    let JSScopeRef::WithScope(with_scope) = scope else { return false };
    if with_scope.is_strict_eval_activation() {
        return false;
    }
    let Some(object) = with_scope.object() else { return false };
    let vm = global_object.vm();
    let unscopables = object.get(vm, &PropertyName::from_identifier(&vm.property_names.unscopables_symbol));
    let Some(unscopables) = JSObject::from_value(&unscopables) else { return false };
    unscopables.get(vm, &PropertyName::from_identifier(ident)).to_boolean()
}

/// `JSScope::resolve(globalObject, scope, ident, returnPredicate, skipPredicate)`: `None` é o `nullptr`
/// com exceção pendente no `VM`.
fn resolve_with(
    global_object: &JSGlobalObject,
    scope: &JSScopeRef,
    ident: &Identifier,
    return_predicate: impl Fn(&JSScopeRef) -> bool,
    skip_predicate: impl Fn(&JSScopeRef) -> bool,
) -> Option<JSScopeRef> {
    let vm = global_object.vm();
    let end = scope.end();
    let mut it = scope.begin();
    loop {
        let scope = it.scope().cloned().expect("JSScope::resolve percorreu além do fim da cadeia");
        let object = it.get();

        // Global scope.
        it.advance();
        if it == end {
            let global_scope_extension = scope.realm().global_scope_extension();
            if let Some(global_scope_extension) = global_scope_extension {
                let has_property = object.has_property(global_object, ident);
                if vm.exception().is_some() {
                    return None;
                }
                if has_property {
                    return Some(object);
                }
                let extension_scope_object = JSScope::object_at_scope(&global_scope_extension);
                let has_property = extension_scope_object.has_property(global_object, ident);
                if vm.exception().is_some() {
                    return None;
                }
                if has_property {
                    return Some(extension_scope_object);
                }
            }
            return Some(object);
        }

        if skip_predicate(&scope) {
            continue;
        }

        let has_property = object.has_property(global_object, ident);
        if vm.exception().is_some() {
            return None;
        }
        if has_property {
            let unscopable = is_unscopable(global_object, &scope, &object, ident);
            // `RETURN_IF_EXCEPTION(throwScope, nullptr)`: o `get` de `Symbol.unscopables` (ou da chave) pode lançar.
            if vm.exception().is_some() {
                return None;
            }
            if !unscopable {
                return Some(object);
            }
        }

        if return_predicate(&scope) {
            return Some(object);
        }
    }
}

impl JSScope {
    /// `abstractAccess` (JSScope.cpp:58): `true` se achou informação suficiente para terminar a otimização.
    #[allow(clippy::too_many_arguments)]
    fn abstract_access(
        global_object: &JSGlobalObject,
        scope: &JSScopeRef,
        ident: &Identifier,
        get_or_put: GetOrPut,
        depth: u32,
        needs_var_injection_checks: &mut bool,
        op: &mut ResolveOp,
        initialization_mode: InitializationMode,
    ) -> bool {
        let vm = global_object.vm();
        // `symbolTable->find(locker, ident.impl())`: `(isReadOnly, scopeOffset)` da entrada, se existe.
        let find_entry = |symbol_table: &SymbolTableRef| {
            let key = ident.impl_()?;
            let symbol_table = symbol_table.borrow();
            symbol_table.find(&key).map(|entry| (entry.is_read_only(), entry.scope_offset().offset() as usize))
        };

        if scope.is_js_lexical_environment() {
            let symbol_table = scope.symbol_table_object().symbol_table();
            if let Some((read_only, offset)) = find_entry(&symbol_table) {
                if read_only && get_or_put == GetOrPut::Put {
                    // We know the property will be at this lexical environment scope, but we don't know how to cache it.
                    *op = ResolveOp::dynamic();
                    return true;
                }
                *op = ResolveOp::new(
                    make_type(ResolveType::ClosureVar, *needs_var_injection_checks),
                    depth,
                    None,
                    Some(scope.clone()),
                    None,
                    offset,
                );
                return true;
            }

            // O ramo `ModuleEnvironmentType` (`moduleRecord()->resolveImport`) espera o
            // `AbstractModuleRecord` ter registro de células: `module_record()` é só o `cell_id`.

            if symbol_table.borrow().uses_sloppy_eval() {
                *needs_var_injection_checks = true;
            }
            return false;
        }

        if scope.is_global_lexical_environment() {
            let symbol_table = scope.symbol_table_object().symbol_table();
            let Some((read_only, offset)) = find_entry(&symbol_table) else { return false };
            if get_or_put == GetOrPut::Put && read_only && !is_initialization(initialization_mode) {
                // We know the property will be at global lexical environment, but we don't know how to cache it.
                *op = ResolveOp::dynamic();
                return true;
            }

            // We can force const Initialization to always go down the fast path. It is provably impossible to construct
            // a program that needs a var injection check here (any other let/const/class would be a duplicate of this
            // in the global scope, and an eval in the global scope that defined a const would also be a duplicate).
            // We still need to make the slow path correct for when we need to fire a watchpoint.
            let resolve_type = if initialization_mode == InitializationMode::ConstInitialization {
                ResolveType::GlobalLexicalVar
            } else {
                make_type(ResolveType::GlobalLexicalVar, *needs_var_injection_checks)
            };
            *op = ResolveOp::new(resolve_type, depth, None, None, None, offset);
            return true;
        }

        if let JSScopeRef::GlobalObject(scope_global_object) = scope {
            let symbol_table = scope.symbol_table_object().symbol_table();
            if let Some((read_only, offset)) = find_entry(&symbol_table) {
                if get_or_put == GetOrPut::Put && read_only {
                    // We know the property will be at global scope, but we don't know how to cache it.
                    *op = ResolveOp::dynamic();
                    return true;
                }
                *op = ResolveOp::new(
                    make_type(ResolveType::GlobalVar, *needs_var_injection_checks),
                    depth,
                    None,
                    None,
                    None,
                    offset,
                );
                return true;
            }

            let property_name = PropertyName::from_identifier(ident);
            let has_own_property = is_valid_offset(scope_global_object.structure().get(vm, &property_name));
            if !has_own_property {
                *op = ResolveOp::new(make_type(ResolveType::UnresolvedProperty, *needs_var_injection_checks), 0, None, None, None, 0);
                return true;
            }

            // We know the property will be at global scope, but we don't know how to cache it: o porte não
            // tem `ensurePropertyReplacementWatchpointSet` nem `propertyAccessesAreCacheable`.
            debug_assert!(scope.next().is_none());
            *op = ResolveOp::new(make_type(ResolveType::GlobalProperty, *needs_var_injection_checks), 0, None, None, None, 0);
            return true;
        }

        *op = ResolveOp::dynamic();
        true
    }

    /// `abstractResolve(globalObject, depthOffset, scope, ident, getOrPut, unlinkedType, initializationMode)`.
    pub fn abstract_resolve(
        global_object: &JSGlobalObject,
        depth_offset: u32,
        scope: &JSScopeRef,
        ident: &Identifier,
        get_or_put: GetOrPut,
        unlinked_type: ResolveType,
        initialization_mode: InitializationMode,
    ) -> ResolveOp {
        let mut op = ResolveOp::dynamic();
        if unlinked_type == ResolveType::Dynamic {
            return op;
        }

        let mut needs_checks = needs_var_injection_checks(unlinked_type);
        let mut depth = depth_offset;
        let mut scope = Some(scope.clone());
        while let Some(current) = scope {
            let success = JSScope::abstract_access(
                global_object,
                &current,
                ident,
                get_or_put,
                depth,
                &mut needs_checks,
                &mut op,
                initialization_mode,
            );
            if success {
                break;
            }
            depth += 1;
            scope = current.next();
        }
        op
    }

    /// `resolve(globalObject, scope, ident)`.
    pub fn resolve(global_object: &JSGlobalObject, scope: &JSScopeRef, ident: &Identifier) -> Option<JSScopeRef> {
        resolve_with(global_object, scope, ident, |_| false, |_| false)
    }

    /// `resolveScopeForHoistingFuncDeclInEval`: o valor vazio é o `{ }` do `RETURN_IF_EXCEPTION`.
    pub fn resolve_scope_for_hoisting_func_decl_in_eval(
        global_object: &JSGlobalObject,
        scope: &JSScopeRef,
        ident: &Identifier,
    ) -> JSValue {
        let return_predicate = |scope: &JSScopeRef| scope.is_var_scope();
        let skip_predicate = |scope: &JSScopeRef| scope.is_with_scope() || scope.is_catch_scope_with_simple_parameter();
        let Some(object) = resolve_with(global_object, scope, ident, return_predicate, skip_predicate) else {
            return JSValue::empty();
        };

        // `dynamicDowncast<JSScope>(object)` sempre acerta: todo objeto de escopo é um `JSScope`.
        let result = match object.symbol_table() {
            Some(scope_symbol_table) => {
                object.is_global_object() || scope_symbol_table.borrow().scope_type() == ScopeType::VarScope
            }
            None => false,
        };

        if result {
            JSValue::from_cell(object.cell_id())
        } else {
            js_undefined()
        }
    }

    /// `hasConstantScope(ResolveType)`: o `JSScope.h` a declara e o `JSScopeInlines`/`.cpp` só a
    /// definem no ramo do JIT; o interpretador não a chama.
    ///
    /// `constantScopeForCodeBlock(type, codeBlock)`: o escopo constante do tipo de resolução.
    pub fn constant_scope_for_code_block(resolve_type: ResolveType, global_object: &JSGlobalObjectRef) -> Option<JSScopeRef> {
        match resolve_type {
            ResolveType::GlobalProperty
            | ResolveType::GlobalVar
            | ResolveType::GlobalPropertyWithVarInjectionChecks
            | ResolveType::GlobalVarWithVarInjectionChecks => Some(JSScopeRef::GlobalObject(Rc::clone(global_object))),
            ResolveType::GlobalLexicalVarWithVarInjectionChecks | ResolveType::GlobalLexicalVar => {
                Some(JSScopeRef::GlobalLexicalEnvironment(global_object.global_lexical_environment()))
            }
            _ => None,
        }
    }

    /// `collectClosureVariablesUnderTDZ`.
    pub fn collect_closure_variables_under_tdz(
        scope: Option<JSScopeRef>,
        result: &mut TDZEnvironment,
        private_name_environment: &mut PrivateNameEnvironment,
    ) {
        let mut scope = scope;
        while let Some(current) = scope {
            scope = current.next();
            if !current.is_lexical_scope() && !current.is_catch_scope() {
                continue;
            }

            // O ramo `isModuleScope()` (imports do `AbstractModuleRecord`) espera o `JSModuleEnvironment`.

            let symbol_table = current.symbol_table().expect("escopo léxico sem SymbolTable");
            let symbol_table = symbol_table.borrow();
            debug_assert!(matches!(
                symbol_table.scope_type(),
                ScopeType::LexicalScope | ScopeType::CatchScope | ScopeType::CatchScopeWithSimpleParameter
            ));
            for (key, _) in symbol_table.iter() {
                result.add(key, ());
            }

            if symbol_table.has_private_names() {
                for (key, value) in symbol_table.private_names() {
                    private_name_environment.add(key, value.clone());
                }
            }
        }
    }
}
