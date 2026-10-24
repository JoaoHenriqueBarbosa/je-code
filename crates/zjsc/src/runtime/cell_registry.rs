//! Registro central de células: o espaço único de `cell_id` do `JSValue::Cell`.
//!
//! Divergência deliberada do C++: lá uma célula é um ponteiro para o heap do GC e o `JSType` mora no
//! cabeçalho do `JSCell` (`JSCell::type()`). Aqui não há GC ainda, então um único `thread_local` guarda
//! uma `Vec<Option<CellEntry>>` e o `cell_id` é `(índice + 1) << 3`: os três bits baixos ficam zerados
//! (como num ponteiro alinhado), o zero nunca é id válido e o id cabe em `u32` até 2^29 células (o
//! `cell_id as u32` do `CodeBlock` segue válido). Cada entrada guarda o dono (o `VMScope` aberto
//! quando nasceu) e vive até `remove` ou até o fim do escopo dono (`remove_all_of`), que faz o papel do
//! `~VM` do C++ e quebra os ciclos de `Rc`. Sem escopo (testes unitários) vive até o fim da thread.
//!
//! O `JSType` não é guardado ao lado: cada variante de `CellEntry` o devolve por `js_type()`, o papel do
//! cabeçalho do `JSCell`. A conferência de tipo é exata (variante errada dá `None`), o que elimina por
//! construção as colisões de tag dos registros antigos.
//!
//! Quando o `Heap` (`crate::heap`, arena de `CellId(u32)` com coleta por marcação e varredura) chegar, ele
//! assume o lugar do `Vec`: `insert` vira a alocação na arena, `remove` vira a varredura e o id vira
//! `CellId`, sem mexer nos chamadores.
//!
//! Para adicionar um tipo de célula: acrescente uma variante em `CellEntry` com a referência do módulo
//! dele e uma linha em `CellEntry::js_type`. Proibido `thread_local` de célula e tag própria.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::error_instance::ErrorInstanceRef;
use crate::runtime::date_instance::DateInstanceRef;
use crate::runtime::exception::ExceptionRef;
use crate::runtime::internal_function::InternalFunctionRef;
use crate::runtime::js_array_iterator::JSArrayIteratorRef;
use crate::runtime::js_internal_field_object_impl::InternalFields;
use crate::runtime::js_array_buffer::JSArrayBufferRef;
use crate::runtime::js_data_view::JSDataViewRef;
use crate::runtime::js_reg_exp_string_iterator::JSRegExpStringIteratorRef;
use crate::runtime::js_string_iterator::JSStringIteratorRef;
use crate::runtime::js_cell_butterfly::JSCellButterflyRef;
use crate::runtime::js_big_int::JSBigIntRef;
use crate::runtime::js_callee::JSCallee;
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::js_getter_setter::GetterSetterRef;
use crate::runtime::custom_getter_setter::CustomGetterSetterRef;
use crate::runtime::js_async_from_sync_iterator::JSAsyncFromSyncIteratorRef;
use crate::runtime::js_async_function_generator::JSAsyncFunctionGeneratorRef;
use crate::runtime::js_async_generator::JSAsyncGeneratorRef;
use crate::runtime::js_generator::JSGeneratorRef;
use crate::runtime::js_map::{JSMapIteratorRef, JSMapRef};
use crate::runtime::js_set::{JSSetIteratorRef, JSSetRef};
use crate::runtime::js_weak_map::JSWeakMapRef;
use crate::runtime::js_weak_set::JSWeakSetRef;
use crate::runtime::js_object::{JSObject, JSObjectRef};
use crate::runtime::js_promise::JSPromiseRef;
use crate::runtime::js_promise_combinators_context::{JSPromiseCombinatorsContextRef, JSPromiseCombinatorsGlobalContextRef};
use crate::runtime::js_promise_reaction::{JSFullPromiseReactionRef, JSSlimPromiseReactionRef};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_string::JSStringRef;
use crate::runtime::js_template_object_descriptor::JSTemplateObjectDescriptorRef;
use crate::runtime::js_type::JSType;
use crate::runtime::js_wrapper_object::JSWrapperObjectRef;
use crate::runtime::js_global_proxy::JSGlobalProxyRef;
use crate::runtime::proxy_object::ProxyObjectRef;
use crate::runtime::proxy_revoke::ProxyRevokeRef;
use crate::runtime::abstract_module_record::AbstractModuleRecordRef;
use crate::runtime::js_module_namespace_object::JSModuleNamespaceObjectRef;
use crate::runtime::js_arguments_objects::DirectArgumentsRef;
use crate::runtime::js_property_name_enumerator::JSPropertyNameEnumeratorRef;
use crate::runtime::reg_exp::RegExpRef;
use crate::runtime::reg_exp_object::RegExpObjectRef;
use crate::runtime::string_object::StringObjectRef;
use crate::runtime::symbol::SymbolRef;
use crate::runtime::symbol_table::SymbolTableRef;

/// Bits baixos do id, sempre zero.
const CELL_ID_SHIFT: usize = 3;
/// Bits do índice (mais um) no id; acima deles vem a geração do slot.
const INDEX_BITS: usize = 24;
/// A geração dá a volta em 5 bits: o id inteiro cabe nos 32 bits do `HeapRef` dos metadados de bytecode
/// (`WriteBarrier` de 32 bits no porte). Os metadados que guardam id de célula morta são limpos antes da
/// varredura (`CodeBlock::finalize_weak_metadata`), então a volta da geração não ressuscita cache.
const GENERATION_MASK: u16 = (1 << 5) - 1;

/// O id de um slot numa geração.
fn make_cell_id(index: usize, generation: u16) -> usize {
    (((generation as usize) << INDEX_BITS) | (index + 1)) << CELL_ID_SHIFT
}

/// A célula guardada, uma variante por tipo concreto. Clonar é clonar o `Rc`.
#[derive(Clone)]
pub enum CellEntry {
    String(JSStringRef),
    RegExp(RegExpRef),
    CellButterfly(JSCellButterflyRef),
    Symbol(SymbolRef),
    SymbolTable(SymbolTableRef),
    TemplateObjectDescriptor(JSTemplateObjectDescriptorRef),
    Scope(JSScopeRef),
    Callee(Rc<JSCallee>),
    Function(JSFunctionRef),
    BigInt(JSBigIntRef),
    Object(JSObjectRef),
    ErrorInstance(ErrorInstanceRef),
    DateInstance(DateInstanceRef),
    RegExpObject(RegExpObjectRef),
    StringObject(StringObjectRef),
    WrapperObject(JSWrapperObjectRef),
    GetterSetter(GetterSetterRef),
    CustomGetterSetter(CustomGetterSetterRef),
    Exception(ExceptionRef),
    InternalFunction(InternalFunctionRef),
    Promise(JSPromiseRef),
    SlimPromiseReaction(JSSlimPromiseReactionRef),
    FullPromiseReaction(JSFullPromiseReactionRef),
    PromiseCombinatorsContext(JSPromiseCombinatorsContextRef),
    PromiseCombinatorsGlobalContext(JSPromiseCombinatorsGlobalContextRef),
    Map(JSMapRef),
    Set(JSSetRef),
    WeakMap(JSWeakMapRef),
    WeakSet(JSWeakSetRef),
    MapIterator(JSMapIteratorRef),
    SetIterator(JSSetIteratorRef),
    Generator(JSGeneratorRef),
    AsyncGenerator(JSAsyncGeneratorRef),
    AsyncFunctionGenerator(JSAsyncFunctionGeneratorRef),
    AsyncFromSyncIterator(JSAsyncFromSyncIteratorRef),
    ArrayIterator(JSArrayIteratorRef),
    ArrayBuffer(JSArrayBufferRef),
    DataView(JSDataViewRef),
    TypedArray(crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayViewRef),
    WeakObjectRef(crate::runtime::js_weak_object_ref::JSWeakObjectRefRef),
    ShadowRealm(crate::runtime::shadow_realm_object::ShadowRealmObjectRef),
    FinalizationRegistry(crate::runtime::js_finalization_registry::JSFinalizationRegistryRef),
    StringIterator(JSStringIteratorRef),
    RegExpStringIterator(JSRegExpStringIteratorRef),
    Proxy(ProxyObjectRef),
    GlobalProxy(JSGlobalProxyRef),
    ProxyRevoke(ProxyRevokeRef),
    ModuleRecord(AbstractModuleRecordRef),
    ModuleNamespace(JSModuleNamespaceObjectRef),
    DirectArguments(DirectArgumentsRef),
    ScopedArguments(crate::runtime::js_scoped_arguments::ScopedArgumentsRef),
    PropertyNameEnumerator(JSPropertyNameEnumeratorRef),
    CallSite(crate::runtime::js_call_site::JSCallSiteRef),
    IntlInstance(crate::runtime::intl_support::IntlInstanceRef),
    TemporalDuration(crate::runtime::temporal_duration::TemporalDurationRef),
    TemporalInstant(crate::runtime::temporal_instant::TemporalInstantRef),
    TemporalPlainDate(crate::runtime::temporal_plain_date::TemporalPlainDateRef),
    TemporalPlainDateTime(crate::runtime::temporal_plain_date_time::TemporalPlainDateTimeRef),
    TemporalPlainTime(crate::runtime::temporal_plain_time::TemporalPlainTimeRef),
    TemporalPlainYearMonth(crate::runtime::temporal_plain_year_month::TemporalPlainYearMonthRef),
    TemporalPlainMonthDay(crate::runtime::temporal_plain_month_day::TemporalPlainMonthDayRef),
    TemporalZonedDateTime(crate::runtime::temporal_zoned_date_time::TemporalZonedDateTimeRef),
    IteratorHelper(crate::runtime::js_iterator_helper::JSIteratorHelperRef),
    WrapForValidIterator(crate::runtime::js_wrap_for_valid_iterator::JSWrapForValidIteratorRef),
    DisposableStack(crate::runtime::js_disposable_stack::JSDisposableStackRef),
    AsyncDisposableStack(crate::runtime::js_async_disposable_stack::JSAsyncDisposableStackRef),
    DOMException(crate::runtime::js_dom_exception::JSDOMExceptionRef),
}

/// `Rc::strong_count` da referência que a variante guarda.
macro_rules! entry_strong_count {
    ($entry:expr; $($variant:ident),* $(,)?) => {
        match $entry {
            $(CellEntry::$variant(cell) => Rc::strong_count(cell),)*
            CellEntry::Scope(scope) => scope.strong_count(),
        }
    };
}

impl CellEntry {
    /// Quantos `Rc` fortes seguram a célula, contando o do registro: mais de um quer dizer que algo fora do
    /// registro (um local Rust, um campo `Rc`, um `thread_local`) a usa, e o coletor a trata como raiz.
    pub(crate) fn strong_count(&self) -> usize {
        entry_strong_count!(self;
            String, RegExp, CellButterfly, Symbol, SymbolTable, TemplateObjectDescriptor, Callee, Function, BigInt,
            Object, ErrorInstance, DateInstance, RegExpObject, StringObject, WrapperObject, GetterSetter,
            CustomGetterSetter, Exception, InternalFunction, Promise, SlimPromiseReaction, FullPromiseReaction,
            PromiseCombinatorsContext, PromiseCombinatorsGlobalContext, Map, Set, WeakMap, WeakSet, MapIterator,
            SetIterator, Generator, AsyncGenerator, AsyncFunctionGenerator, AsyncFromSyncIterator, ArrayIterator,
            ArrayBuffer, DataView, TypedArray, WeakObjectRef, ShadowRealm, FinalizationRegistry, StringIterator,
            RegExpStringIterator, Proxy, GlobalProxy, ProxyRevoke, ModuleRecord, ModuleNamespace, DirectArguments,
            ScopedArguments, PropertyNameEnumerator, CallSite, IntlInstance, TemporalDuration, TemporalInstant,
            TemporalPlainDate, TemporalPlainDateTime, TemporalPlainTime, TemporalPlainYearMonth,
            TemporalPlainMonthDay, TemporalZonedDateTime, IteratorHelper, WrapForValidIterator, DisposableStack,
            AsyncDisposableStack, DOMException,
        )
    }

    /// O que a célula segura além da base `JSObject` e dos campos internos (`visitChildren` de cada classe).
    pub(crate) fn visit_extra_children(&self, marker: &mut crate::runtime::gc::Marker) {
        match self {
            CellEntry::Scope(scope) => scope.visit_children(marker),
            CellEntry::GetterSetter(getter_setter) => getter_setter.visit_children(marker),
            CellEntry::Function(function) => function.visit_children(marker),
            CellEntry::Promise(promise) => promise.visit_children(marker),
            CellEntry::SlimPromiseReaction(reaction) => reaction.visit_children(marker),
            CellEntry::FullPromiseReaction(reaction) => reaction.visit_children(marker),
            CellEntry::PromiseCombinatorsGlobalContext(context) => context.visit_children(marker),
            CellEntry::AsyncFromSyncIterator(iterator) => iterator.visit_children(marker),
            CellEntry::DirectArguments(arguments) => arguments.visit_children(marker),
            CellEntry::ScopedArguments(arguments) => arguments.visit_children(marker),
            CellEntry::ProxyRevoke(revoke) => revoke.visit_children(marker),
            CellEntry::WrapperObject(wrapper) => wrapper.visit_children(marker),
            CellEntry::RegExpObject(reg_exp) => reg_exp.visit_children(marker),
            CellEntry::Exception(exception) => exception.visit_children(marker),
            CellEntry::ErrorInstance(error) => error.visit_children(marker),
            CellEntry::IntlInstance(instance) => instance.visit_children(marker),
            CellEntry::CellButterfly(butterfly) => butterfly.visit_children(marker),
            CellEntry::Map(cell) => visit_table(cell.table(), marker),
            CellEntry::Set(cell) => visit_table(cell.table(), marker),
            CellEntry::WeakObjectRef(weak_ref) => weak_ref.visit_children(marker),
            CellEntry::FinalizationRegistry(registry) => registry.visit_children(marker),
            _ => {}
        }
        // As coleções fracas e as referências fracas voltam depois da marcação forte (efêmeros e finalização).
        if matches!(self, CellEntry::WeakMap(_) | CellEntry::WeakSet(_) | CellEntry::WeakObjectRef(_) | CellEntry::FinalizationRegistry(_) | CellEntry::ErrorInstance(_)) {
            marker.defer_weak(self.clone());
        }
    }

    /// Desfaz os elos `Rc` que a célula tem de volta para o `JSGlobalObject` (ver `remove_all_of`). Só o
    /// desmonte do programa chama: depois disto a célula não executa mais nada.
    fn break_realm_links(&self) {
        match self {
            CellEntry::Scope(scope) => scope.scope().clear_next(),
            CellEntry::Callee(callee) => callee.set_scope(None),
            CellEntry::Function(function) => function.set_scope(None),
            CellEntry::InternalFunction(function) => function.clear_global_object(),
            CellEntry::GlobalProxy(proxy) => proxy.clear_target(),
            _ => {}
        }
        // A árvore de transições da estrutura da célula forma ciclo pai <-> filho (ver
        // `Structure::break_transition_links`); sem isto cada programa deixa as estruturas dele para trás.
        if let Some(object) = self.as_js_object() {
            object.structure().break_transition_links();
        }
    }

    /// Os campos internos da célula (`jsCast<JSInternalFieldObjectImpl<>*>`), ou `None` quando a classe não
    /// é uma `JSInternalFieldObjectImpl` no C++ ou o porte a guarda de outro jeito.
    pub fn internal_fields(&self) -> Option<&dyn InternalFields> {
        let fields: &dyn InternalFields = match self {
            CellEntry::Generator(cell) => &***cell,
            CellEntry::AsyncGenerator(cell) => &***cell,
            CellEntry::AsyncFunctionGenerator(cell) => &***cell,
            CellEntry::IteratorHelper(cell) => &***cell,
            CellEntry::WrapForValidIterator(cell) => &***cell,
            CellEntry::DisposableStack(cell) => &***cell,
            CellEntry::AsyncDisposableStack(cell) => &***cell,
            CellEntry::DOMException(cell) => &***cell,
            CellEntry::Proxy(cell) => &***cell,
            CellEntry::ArrayIterator(cell) => &**cell,
            CellEntry::StringIterator(cell) => &**cell,
            CellEntry::RegExpStringIterator(cell) => &**cell,
            CellEntry::MapIterator(cell) => &**cell,
            CellEntry::SetIterator(cell) => &**cell,
            CellEntry::ModuleRecord(cell) => &**cell,
            _ => return None,
        };
        Some(fields)
    }

    /// `JSCell::type()`: o `JSType` do cabeçalho da célula.
    pub fn js_type(&self) -> JSType {
        match self {
            CellEntry::String(_) => JSType::StringType,
            // `RegExp`, `SymbolTable` e `JSTemplateObjectDescriptor` são `JSCell` puros no C++.
            CellEntry::RegExp(_)
            | CellEntry::SymbolTable(_)
            | CellEntry::TemplateObjectDescriptor(_)
            | CellEntry::Exception(_) => {
                JSType::CellType
            }
            CellEntry::CellButterfly(_) => JSType::JSCellButterflyType,
            CellEntry::Symbol(_) => JSType::SymbolType,
            CellEntry::Scope(scope) => scope.js_type(),
            CellEntry::Callee(_) => JSType::JSCalleeType,
            CellEntry::Function(_) => JSType::JSFunctionType,
            CellEntry::BigInt(_) => JSType::HeapBigIntType,
            // O `JSType` vem do cabeçalho `JSCell` do objeto (o `TypeInfo` da `Structure`).
            CellEntry::Object(object) => object.type_(),
            CellEntry::ErrorInstance(_) => JSType::ErrorInstanceType,
            CellEntry::DateInstance(_) => JSType::JSDateType,
            CellEntry::RegExpObject(_) => JSType::RegExpObjectType,
            // `StringObjectType` ou `DerivedStringObjectType`: o `TypeInfo` da `Structure`.
            CellEntry::StringObject(string_object) => string_object.type_(),
            // `NumberObjectType` ou `BooleanObjectType`: o `TypeInfo` da `Structure`.
            CellEntry::WrapperObject(wrapper_object) => wrapper_object.type_(),
            CellEntry::GetterSetter(_) => JSType::GetterSetterType,
            CellEntry::CustomGetterSetter(_) => JSType::CustomGetterSetterType,
            // `InternalFunctionType` ou `NullSetterFunctionType`: o `TypeInfo` da `Structure`.
            CellEntry::InternalFunction(function) => function.type_(),
            // O `JSType` vem do cabeçalho `JSCell` da promessa (o `TypeInfo` da `Structure`).
            CellEntry::Promise(promise) => promise.type_(),
            CellEntry::SlimPromiseReaction(_) => JSType::JSSlimPromiseReactionType,
            CellEntry::FullPromiseReaction(_) => JSType::JSFullPromiseReactionType,
            CellEntry::PromiseCombinatorsContext(_) => JSType::JSPromiseCombinatorsContextType,
            CellEntry::PromiseCombinatorsGlobalContext(_) => JSType::JSPromiseCombinatorsGlobalContextType,
            CellEntry::Map(_) => JSType::JSMapType,
            CellEntry::Set(_) => JSType::JSSetType,
            CellEntry::WeakMap(_) => JSType::JSWeakMapType,
            CellEntry::WeakSet(_) => JSType::JSWeakSetType,
            CellEntry::MapIterator(_) => JSType::JSMapIteratorType,
            CellEntry::SetIterator(_) => JSType::JSSetIteratorType,
            CellEntry::Generator(_) => JSType::JSGeneratorType,
            CellEntry::AsyncGenerator(_) => JSType::JSAsyncGeneratorType,
            CellEntry::AsyncFunctionGenerator(_) => JSType::JSAsyncFunctionGeneratorType,
            CellEntry::AsyncFromSyncIterator(_) => JSType::JSAsyncFromSyncIteratorType,
            CellEntry::ArrayIterator(_) => JSType::JSArrayIteratorType,
            CellEntry::ArrayBuffer(_) => JSType::ArrayBufferType,
            CellEntry::DataView(_) => JSType::DataViewType,
            // O `JSType` vem do `TypeInfo` da `Structure` (`Int8ArrayType`...).
            CellEntry::TypedArray(view) => view.type_(),
            // `ObjectType`: o `TypeInfo` da `Structure` (sem `JSType` próprio no C++).
            CellEntry::WeakObjectRef(weak_ref) => weak_ref.type_(),
            CellEntry::ShadowRealm(_) => JSType::ShadowRealmType,
            CellEntry::FinalizationRegistry(registry) => registry.type_(),
            CellEntry::StringIterator(_) => JSType::JSStringIteratorType,
            CellEntry::RegExpStringIterator(_) => JSType::JSRegExpStringIteratorType,
            // `ProxyObjectType`: o `TypeInfo` da `Structure`.
            CellEntry::Proxy(proxy) => proxy.type_(),
            // `GlobalProxyType`: o `TypeInfo` da `Structure`.
            CellEntry::GlobalProxy(proxy) => proxy.type_(),
            // `InternalFunctionType`: o `TypeInfo` da `Structure`.
            CellEntry::ProxyRevoke(revoke) => revoke.type_(),
            // `JSModuleRecord` é um `JSInternalFieldObjectImpl<2>` com a `Structure` de `ObjectType`.
            CellEntry::ModuleRecord(_) => JSType::ObjectType,
            CellEntry::ModuleNamespace(_) => JSType::ModuleNamespaceObjectType,
            CellEntry::DirectArguments(_) => JSType::DirectArgumentsType,
            CellEntry::ScopedArguments(_) => JSType::ScopedArgumentsType,
            CellEntry::PropertyNameEnumerator(enumerator) => enumerator.js_type(),
            // `ObjectType`: o `TypeInfo` da `Structure` (sem `JSType` próprio no C++).
            CellEntry::CallSite(call_site) => call_site.type_(),
            // `ObjectType`: o `TypeInfo` da `Structure` (as classes do `Intl` não têm `JSType` próprio).
            CellEntry::IntlInstance(instance) => instance.type_(),
            // `ObjectType`: o `TypeInfo` da `Structure` (sem `JSType` próprio no C++).
            CellEntry::TemporalDuration(duration) => duration.type_(),
            CellEntry::TemporalInstant(instant) => instant.type_(),
            CellEntry::TemporalPlainDate(plain_date) => plain_date.type_(),
            CellEntry::TemporalPlainDateTime(plain_date_time) => plain_date_time.type_(),
            CellEntry::TemporalPlainTime(plain_time) => plain_time.type_(),
            CellEntry::TemporalPlainYearMonth(year_month) => year_month.type_(),
            CellEntry::TemporalPlainMonthDay(month_day) => month_day.type_(),
            CellEntry::TemporalZonedDateTime(zoned_date_time) => zoned_date_time.type_(),
            CellEntry::IteratorHelper(_) => JSType::JSIteratorHelperType,
            CellEntry::WrapForValidIterator(_) => JSType::JSWrapForValidIteratorType,
            CellEntry::DisposableStack(_) => JSType::DisposableStackType,
            CellEntry::AsyncDisposableStack(_) => JSType::AsyncDisposableStackType,
            CellEntry::DOMException(_) => JSType::ObjectType,
        }
    }

    /// A base `JSObject` da célula, se ela é um objeto (`JSValue::getObject()`). Cada variante que embute
    /// um `JSObject` por composição ganha um braço aqui quando for portada (os callees, funções e
    /// escopos embutem `JSNonFinalObject`, que faz `Deref` para `JSObject`). Toda `CellEntry::Scope` devolve
    /// `Some`, como `JSScope : JSNonFinalObject` no C++, mas um escopo nunca é valor de JS: quem precisa
    /// distingui-lo (`to_this`, `to_this_strict`, `with_object_operand`, `scope_base`, `delete`) testa
    /// `CellEntry::Scope`/`JSScope::from_cell_id` ANTES de chamar `from_value`.
    pub fn as_js_object(&self) -> Option<&JSObject> {
        match self {
            CellEntry::Object(object) => Some(&**object),
            // `JSFunction : JSCallee : JSNonFinalObject : JSObject`: o `Deref` encadeado chega na base.
            CellEntry::Function(function) => {
                let object: &JSObject = function;
                Some(object)
            }
            CellEntry::Callee(callee) => {
                let object: &JSObject = callee;
                Some(object)
            }
            CellEntry::ErrorInstance(error) => {
                let object: &JSObject = error;
                Some(object)
            }
            CellEntry::RegExpObject(reg_exp_object) => {
                let object: &JSObject = reg_exp_object;
                Some(object)
            }
            CellEntry::DateInstance(date_instance) => {
                let object: &JSObject = date_instance;
                Some(object)
            }
            CellEntry::StringObject(string_object) => {
                let object: &JSObject = string_object;
                Some(object)
            }
            CellEntry::WrapperObject(wrapper_object) => {
                let object: &JSObject = wrapper_object;
                Some(object)
            }
            CellEntry::InternalFunction(function) => {
                let object: &JSObject = function;
                Some(object)
            }
            CellEntry::Promise(promise) => {
                let object: &JSObject = promise;
                Some(object)
            }
            CellEntry::Map(map) => {
                let object: &JSObject = map;
                Some(object)
            }
            CellEntry::Set(set) => {
                let object: &JSObject = set;
                Some(object)
            }
            CellEntry::WeakMap(weak_map) => {
                let object: &JSObject = weak_map;
                Some(object)
            }
            CellEntry::WeakSet(weak_set) => {
                let object: &JSObject = weak_set;
                Some(object)
            }
            CellEntry::MapIterator(iterator) => {
                let object: &JSObject = iterator;
                Some(object)
            }
            CellEntry::SetIterator(iterator) => {
                let object: &JSObject = iterator;
                Some(object)
            }
            CellEntry::Generator(generator) => {
                let object: &JSObject = generator;
                Some(object)
            }
            CellEntry::AsyncGenerator(generator) => {
                let object: &JSObject = generator;
                Some(object)
            }
            CellEntry::AsyncFunctionGenerator(generator) => {
                let object: &JSObject = generator;
                Some(object)
            }
            CellEntry::AsyncFromSyncIterator(iterator) => {
                let object: &JSObject = iterator;
                Some(object)
            }
            CellEntry::ArrayIterator(iterator) => {
                let object: &JSObject = iterator;
                Some(object)
            }
            CellEntry::ArrayBuffer(array_buffer) => {
                let object: &JSObject = array_buffer;
                Some(object)
            }
            CellEntry::DataView(data_view) => {
                let object: &JSObject = data_view;
                Some(object)
            }
            CellEntry::TypedArray(view) => {
                let object: &JSObject = view;
                Some(object)
            }
            CellEntry::WeakObjectRef(weak_ref) => {
                let object: &JSObject = weak_ref;
                Some(object)
            }
            CellEntry::ShadowRealm(realm) => {
                let object: &JSObject = realm;
                Some(object)
            }
            CellEntry::FinalizationRegistry(registry) => {
                let object: &JSObject = registry;
                Some(object)
            }
            CellEntry::StringIterator(iterator) => {
                let object: &JSObject = iterator;
                Some(object)
            }
            CellEntry::RegExpStringIterator(iterator) => {
                let object: &JSObject = iterator;
                Some(object)
            }
            CellEntry::Proxy(proxy) => {
                let object: &JSObject = proxy;
                Some(object)
            }
            CellEntry::GlobalProxy(proxy) => {
                let object: &JSObject = proxy;
                Some(object)
            }
            CellEntry::ProxyRevoke(revoke) => {
                let object: &JSObject = revoke;
                Some(object)
            }
            CellEntry::ModuleNamespace(namespace) => {
                let object: &JSObject = namespace;
                Some(object)
            }
            CellEntry::DirectArguments(arguments) => {
                let object: &JSObject = arguments;
                Some(object)
            }
            CellEntry::ScopedArguments(arguments) => {
                let object: &JSObject = arguments;
                Some(object)
            }
            CellEntry::CallSite(call_site) => {
                let object: &JSObject = call_site;
                Some(object)
            }
            CellEntry::IntlInstance(instance) => {
                let object: &JSObject = instance;
                Some(object)
            }
            CellEntry::TemporalDuration(duration) => {
                let object: &JSObject = duration;
                Some(object)
            }
            CellEntry::TemporalInstant(instant) => {
                let object: &JSObject = instant;
                Some(object)
            }
            CellEntry::TemporalPlainDate(plain_date) => {
                let object: &JSObject = plain_date;
                Some(object)
            }
            CellEntry::TemporalPlainDateTime(plain_date_time) => {
                let object: &JSObject = plain_date_time;
                Some(object)
            }
            CellEntry::TemporalPlainTime(plain_time) => {
                let object: &JSObject = plain_time;
                Some(object)
            }
            CellEntry::TemporalPlainYearMonth(year_month) => {
                let object: &JSObject = year_month;
                Some(object)
            }
            CellEntry::TemporalPlainMonthDay(month_day) => {
                let object: &JSObject = month_day;
                Some(object)
            }
            CellEntry::TemporalZonedDateTime(zoned_date_time) => {
                let object: &JSObject = zoned_date_time;
                Some(object)
            }
            CellEntry::IteratorHelper(helper) => {
                let object: &JSObject = helper;
                Some(object)
            }
            CellEntry::WrapForValidIterator(iterator) => {
                let object: &JSObject = iterator;
                Some(object)
            }
            CellEntry::DisposableStack(stack) => {
                let object: &JSObject = stack;
                Some(object)
            }
            CellEntry::AsyncDisposableStack(stack) => {
                let object: &JSObject = stack;
                Some(object)
            }
            CellEntry::DOMException(exception) => {
                let object: &JSObject = exception;
                Some(object)
            }
            // O objeto global é uma célula `Scope`: `this` de função sloppy com null/undefined e
            // `Function.prototype.apply(null, ...)` o tratam como objeto comum.
            CellEntry::Scope(JSScopeRef::GlobalObject(global_object)) => {
                let object: &JSObject = global_object;
                Some(object)
            }
            // `JSScope : JSNonFinalObject : JSObject`: o ambiente léxico, o de módulo, o global léxico e o `with`
            // são objetos no C++. Quem precisa do `getOwnPropertySlot` com a `SymbolTable` (get/put por id) passa
            // por `JSScopeRef::get_property_slot`/`put` antes (`slow_paths_object::scope_base`).
            CellEntry::Scope(scope) => {
                let object: &JSObject = scope.scope();
                Some(object)
            }
            _ => None,
        }
    }
}

/// Dono das células criadas fora de qualquer `VMScope` (testes unitários, uso avulso): nunca é liberado
/// em bloco.
const NO_OWNER: u32 = 0;

/// Marca o slot cuja célula foi liberada por `remove_all_of`: só estes slots, quando no fim da `Vec`,
/// são cortados (o id volta a ser alocável). Um `remove` avulso nunca marca, então segue sem reuso.
const RELEASED_OWNER: u32 = u32::MAX;

/// O registro da thread: as células, o dono de cada uma (em paralelo) e o escopo de VM corrente.
struct Registry {
    cells: Vec<Option<CellEntry>>,
    owners: Vec<u32>,
    /// A geração de cada slot: sobe quando o coletor o varre, e o id velho deixa de achar a célula nova.
    generations: Vec<u16>,
    /// Os slots varridos pelo coletor, prontos para a próxima alocação (a free list do `MarkedBlock`).
    free: Vec<u32>,
    /// Alocações desde o início da thread: o relógio do gatilho do coletor.
    allocations: usize,
    /// O dono das próximas células: o `VMScope` mais interno ainda aberto, ou `NO_OWNER`.
    current_owner: u32,
    next_owner: u32,
}

impl Registry {
    const fn new() -> Registry {
        Registry {
            cells: Vec::new(),
            owners: Vec::new(),
            generations: Vec::new(),
            free: Vec::new(),
            allocations: 0,
            current_owner: NO_OWNER,
            next_owner: 1,
        }
    }

    fn push(&mut self, entry: Option<CellEntry>) -> usize {
        self.allocations += 1;
        if let Some(entry) = &entry {
            crate::runtime::heap_size::did_allocate(crate::runtime::heap_size::allocation_bytes(entry));
        }
        let index = match self.free.pop() {
            Some(index) => {
                let index = index as usize;
                self.cells[index] = entry;
                self.owners[index] = self.current_owner;
                index
            }
            None => {
                let index = self.cells.len();
                assert!(index < (1 << INDEX_BITS) - 1, "registro de células esgotado");
                self.cells.push(entry);
                self.owners.push(self.current_owner);
                self.generations.push(0);
                index
            }
        };
        let cell_id = make_cell_id(index, self.generations[index]);
        // A geração dá a volta: o id pode ser o de uma lápide antiga.
        #[cfg(debug_assertions)]
        crate::runtime::gc::forget_tombstone(cell_id);
        cell_id
    }

    /// O índice do id, se ele é da geração corrente do slot.
    fn slot(&self, cell_id: usize) -> Option<usize> {
        let (index, generation) = decode_cell_id(cell_id)?;
        (self.generations.get(index) == Some(&generation)).then_some(index)
    }
}

thread_local! {
    static CELLS: RefCell<Registry> = const { RefCell::new(Registry::new()) };
    /// O escopo do último programa terminado nesta thread, ainda vivo para o chamador ler o resultado.
    static RETIRED: RefCell<Option<VMScope>> = const { RefCell::new(None) };
}

/// A tabela de uma coleção para o coletor; emprestada (no meio de uma escrita) a coleta desiste.
fn visit_table(table: &RefCell<crate::runtime::js_ordered_hash_table::OrderedTable>, marker: &mut crate::runtime::gc::Marker) {
    match table.try_borrow() {
        Ok(table) => table.visit_children(marker),
        Err(_) => marker.abort(),
    }
}

/// Fim de vida de um VM: o equivalente de `~VM` com `heap.lastChanceToFinalize()` do C++. Sem GC, as
/// células de um VM formam ciclos de `Rc` (global -> VM -> interpretador -> global, objeto -> protótipo
/// -> construtor -> objeto), que só o registro enxerga; soltar todas as entradas do dono quebra os ciclos.
///
/// Todas as células criadas enquanto o escopo é o mais interno ficam marcadas com o id dele. Ao cair, o
/// escopo solta todas e devolve ao registro os ids do fim da `Vec` (corta o rabo), então um programa novo
/// recomeça do mesmo tamanho e a memória não cresce com o número de programas.
///
/// Contrato: depois do `Drop` nenhum `JSValue::Cell` criado sob o escopo pode ser usado (`get` dá `None`
/// ou, se o id foi reaproveitado, outra célula). Por isso o resultado de um programa precisa ser lido antes
/// do escopo cair; `run_program` o mantém vivo até o próximo programa da thread (ver lá).
pub struct VMScope {
    id: u32,
    previous_owner: u32,
    restored: bool,
}

impl VMScope {
    /// Abre um escopo: as células criadas daqui em diante (até o escopo fechar) pertencem a ele. Escopos
    /// aninham: o interno devolve o dono ao externo quando fecha.
    pub fn enter() -> VMScope {
        CELLS.with(|registry| {
            let mut registry = registry.borrow_mut();
            let id = registry.next_owner;
            // Pula 0 e o marcador de liberado se o contador der a volta.
            registry.next_owner = if id >= RELEASED_OWNER - 1 { 1 } else { id + 1 };
            let previous_owner = std::mem::replace(&mut registry.current_owner, id);
            VMScope { id, previous_owner, restored: false }
        })
    }

    /// O id do dono (diagnóstico e testes).
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Devolve o dono corrente ao escopo externo, sem soltar as células.
    fn restore_owner(&mut self) {
        if !self.restored {
            self.restored = true;
            // `try_with`: o `Drop` pode rodar na destruição dos `thread_local` da thread.
            let _ = CELLS.try_with(|registry| registry.borrow_mut().current_owner = self.previous_owner);
        }
    }

    /// Encerra a criação de células do escopo e o guarda em `RETIRED`. As células dele seguem legíveis até
    /// o próximo `run_program` (que chama `release_retired`) ou o fim da thread.
    pub fn retire(mut self) {
        self.restore_owner();
        let previous = RETIRED.with(|slot| slot.borrow_mut().replace(self));
        // Solta fora do `borrow` do slot: o `Drop` dos objetos pode rodar código que chega ao registro.
        drop(previous);
    }
}

/// Destrói o escopo do último programa terminado, se houver.
pub fn release_retired() {
    let retired = RETIRED.with(|slot| slot.borrow_mut().take());
    drop(retired);
}

impl Drop for VMScope {
    fn drop(&mut self) {
        let outermost = self.previous_owner == NO_OWNER;
        self.restore_owner();
        // O estado por thread que guarda `cell_id`, `JSValue::Cell` ou `Rc` de célula é solto antes das
        // células, e só quando o escopo mais externo cai: um escopo interno não pode apagar o do externo.
        if outermost {
            reset_program_state();
        }
        remove_all_of(self.id);
    }
}

/// Esvazia todo `thread_local!` que referencia células de um programa. No C++ esse estado mora no `VM`
/// (`symbolImplToSymbolMap`, `RegExpCache`, `SmallStrings`, `m_waiterLists`...) e morre com ele; o porte o
/// guarda por thread e esta é a lista de quem precisa cair junto. Quem criar um `thread_local!` que guarde
/// id de célula, `JSValue` ou `Rc` de célula acrescenta aqui um `reset_for_program` dele (ver a auditoria
/// de `thread_local!` em `PLAN.md`). O que independe do VM (átomos, contadores de id, opções, fuso,
/// registro de tipos canônicos do wasm) não entra.
fn reset_program_state() {
    crate::runtime::gc::reset_for_program();
    crate::runtime::current_realm::reset_for_program();
    crate::runtime::number_prototype::reset_for_program();
    crate::runtime::stack_frame::reset_for_program();
    crate::runtime::dynamic_code_guard::reset_for_program();
    crate::wtf::date_math::reset_for_program();
    crate::runtime::global_named_lookup::reset_for_program();
    crate::runtime::cpu_clock::reset_for_program();
    crate::parser::source_provider::reset_for_program();
    crate::runtime::time_clamper::reset_for_program();
    crate::runtime::js_string::reset_for_program();
    crate::runtime::symbol::reset_for_program();
    crate::runtime::reg_exp::reset_for_program();
    crate::runtime::ordered_hash_table_storage::reset_for_program();
    crate::runtime::array_prototype::reset_for_program();
    crate::runtime::js_call_site::reset_for_program();
    crate::runtime::js_arguments_objects::reset_for_program();
    crate::runtime::js_module_loader::reset_for_program();
    crate::runtime::js_dom_exception::reset_for_program();
    crate::runtime::text_encoder::reset_for_program();
    crate::runtime::web_iterable::reset_for_program();
    crate::runtime::url::reset_for_program();
    crate::runtime::url_pattern::reset_for_program();
    crate::runtime::blob::reset_for_program();
    crate::runtime::response::reset_for_program();
    crate::runtime::request::reset_for_program();
    crate::runtime::mime::reset_for_program();
    crate::runtime::event_target::reset_for_program();
    crate::runtime::abort_signal::reset_for_program();
    crate::runtime::broadcast_channel::reset_for_program();
    crate::runtime::message_channel::reset_for_program();
    crate::runtime::worker_host::reset_for_program();
    crate::runtime::crypto::reset_for_program();
    crate::runtime::node_crypto::reset_for_program();
    crate::runtime::queuing_strategy::reset_for_program();
    crate::runtime::streams::reset_for_program();
    crate::runtime::performance::reset_for_program();
    crate::runtime::node_error::reset_for_program();
    crate::api::eval::reset_for_program();
    crate::runtime::text_decoder::reset_for_program();
    crate::runtime::node_buffer::reset_for_program();
    crate::runtime::node_fs::reset_for_program();
    crate::runtime::waiter_list_manager::reset_for_program();
    crate::runtime::timers::reset_for_program();
    crate::runtime::net_loop::reset_for_program();
    crate::runtime::process_exit::reset_for_program();
    crate::runtime::process_shape::reset_for_program();
    crate::runtime::process_system::reset_for_program();
    crate::runtime::process_stdio::reset_for_program();
    crate::runtime::process_object::reset_for_program();
    crate::runtime::js_web_assembly::reset_for_program();
    crate::runtime::js_web_assembly_gc_object::reset_for_program();
    crate::runtime::js_web_assembly_jspi::reset_for_program();
    crate::wasm::wasm_instance::reset_for_program();
    crate::interpreter::unwind::reset_for_program();
}

/// Roda um programa completo (cria o VM e o global dentro de `body`, devolve o resultado) sob um
/// `VMScope`. Quando `body` volta, o `VM`, o `JSGlobalObject` e todos os locais dele já caíram; o escopo
/// então é aposentado, não destruído: o `JSValue` devolvido (e o da exceção) ainda aponta para células do
/// escopo e o chamador o lê depois (`to_wtf_string` no harness). O escopo é destruído quando o próximo
/// programa desta thread começa (ou no fim da thread), então no máximo um programa fica retido. Em
/// pânico, o `Drop` do escopo solta tudo ao desenrolar a pilha.
pub fn run_program<T>(body: impl FnOnce() -> T) -> T {
    // O programa anterior morre antes de o novo nascer: assim `first_global_object` (o reino de quem
    // converte fora de execução) não enxerga o global de um programa velho antes do corrente.
    release_retired();
    let scope = VMScope::enter();
    let result = body();
    scope.retire();
    result
}

/// Solta todas as células do dono e corta do fim da `Vec` os slots liberados. Devolve quantas soltou.
pub fn remove_all_of(owner: u32) -> usize {
    if owner == NO_OWNER || owner == RELEASED_OWNER {
        return 0;
    }
    // Tira as entradas com o registro emprestado e solta depois: o `Drop` em cascata de um objeto pode
    // chamar `get`/`remove` e não pode encontrar o `RefCell` emprestado.
    let Ok(released) = CELLS.try_with(|registry| {
        let mut registry = registry.borrow_mut();
        let Registry { cells, owners, generations, free, .. } = &mut *registry;
        let mut released = Vec::new();
        for (slot, slot_owner) in cells.iter_mut().zip(owners.iter_mut()) {
            if *slot_owner == owner {
                *slot_owner = RELEASED_OWNER;
                released.extend(slot.take());
            }
        }
        // Slots liberados no fim voltam a ser alocáveis: nenhum id válido aponta para eles depois do
        // contrato do `VMScope`.
        while owners.last() == Some(&RELEASED_OWNER) {
            owners.pop();
            cells.pop();
            generations.pop();
        }
        // Slot varrido pelo coletor e depois liberado com o dono não volta pela free list: ou saiu com o rabo, ou
        // ficou com o dono liberado, como os demais do programa.
        free.retain(|&index| owners.get(index as usize).is_some_and(|&slot_owner| slot_owner != RELEASED_OWNER));
        released
    }) else {
        return 0;
    };
    // O `~VM` do C++ destrói o heap inteiro de uma vez; aqui a posse é por `Rc` e o global forma ciclos com as
    // células que guarda (callee, função, construtor, escopo léxico, proxy do `globalThis`). Corta os elos de
    // volta ao global antes de soltar, ou o global (e com ele o `VM`) nunca chega a zero referências.
    for entry in &released {
        entry.break_realm_links();
    }
    let count = released.len();
    drop(released);
    count
}

/// Índice e geração de um id; `None` para zero ou para bits sujos.
fn decode_cell_id(cell_id: usize) -> Option<(usize, u16)> {
    if cell_id & ((1 << CELL_ID_SHIFT) - 1) != 0 {
        return None;
    }
    let raw = cell_id >> CELL_ID_SHIFT;
    let generation = raw >> INDEX_BITS;
    if generation > GENERATION_MASK as usize {
        return None;
    }
    Some(((raw & ((1 << INDEX_BITS) - 1)).checked_sub(1)?, generation as u16))
}

/// O índice da `Vec` de um id da geração corrente (o que o coletor marca).
pub(crate) fn index_of(cell_id: usize) -> Option<usize> {
    CELLS.with(|registry| registry.borrow().slot(cell_id))
}

/// Guarda a célula e devolve o `cell_id`.
pub fn insert(entry: CellEntry) -> usize {
    CELLS.with(|registry| registry.borrow_mut().push(Some(entry)))
}

/// Reserva um id sem célula, para tipos que guardam o próprio `cell_id` no construtor (dois passos,
/// `Rc::new_cyclic`). Preencha depois com `set`; até lá `get` e `cell_type` dão `None`.
pub fn reserve() -> usize {
    CELLS.with(|registry| registry.borrow_mut().push(None))
}

/// Preenche (ou troca) a entrada de um id vindo de `insert`/`reserve`. `false` se o id não existe.
pub fn set(cell_id: usize, entry: CellEntry) -> bool {
    // A célula reservada só vira alocação de verdade aqui (o `reserve` não tem tipo nem tamanho ainda).
    let bytes = crate::runtime::heap_size::allocation_bytes(&entry);
    CELLS.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(index) = registry.slot(cell_id) else { return false };
        if registry.cells[index].is_none() {
            crate::runtime::heap_size::did_allocate(bytes);
        }
        registry.cells[index] = Some(entry);
        true
    })
}

/// A célula do id (clone do `Rc`), ou `None` se o id é inválido, foi removido ou não foi preenchido.
pub fn get(cell_id: usize) -> Option<CellEntry> {
    let entry = get_for_marking(cell_id);
    #[cfg(debug_assertions)]
    if entry.is_none() {
        crate::runtime::gc::check_not_swept(cell_id);
    }
    entry
}

/// `get` sem a conferência de lápide: o coletor lê ids conservadores, que podem cair num slot varrido.
pub(crate) fn get_for_marking(cell_id: usize) -> Option<CellEntry> {
    CELLS.with(|registry| {
        let registry = registry.borrow();
        registry.slot(cell_id).and_then(|index| registry.cells[index].clone())
    })
}

/// Quantas alocações a thread fez: o relógio do gatilho do coletor.
pub(crate) fn allocation_count() -> usize {
    CELLS.try_with(|registry| registry.borrow().allocations).unwrap_or(0)
}

/// O tamanho do registro e as células que algum `Rc` fora do registro segura (contagem forte maior que 1),
/// raízes do coletor.
pub(crate) fn pinned_cells() -> (usize, Vec<usize>) {
    CELLS.with(|registry| {
        let registry = registry.borrow();
        let pinned = registry
            .cells
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                slot.as_ref().filter(|entry| entry.strong_count() > 1).map(|_| make_cell_id(index, registry.generations[index]))
            })
            .collect();
        (registry.cells.len(), pinned)
    })
}

/// A varredura do coletor: solta toda célula não marcada e devolve os ids soltos com o tipo de cada um. O slot
/// sobe de geração e entra na free list; as entradas caem fora do empréstimo do registro, porque o `Drop` em
/// cascata pode chegar a ele.
pub(crate) fn sweep(marked: &[bool]) -> Vec<(usize, JSType)> {
    let released: Vec<(usize, CellEntry)> = CELLS.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Registry { cells, generations, free, .. } = &mut *registry;
        let mut released = Vec::new();
        for (index, slot) in cells.iter_mut().enumerate() {
            if slot.is_some() && !marked.get(index).copied().unwrap_or(true) {
                if let Some(entry) = slot.take() {
                    released.push((make_cell_id(index, generations[index]), entry));
                    generations[index] = (generations[index] + 1) & GENERATION_MASK;
                    free.push(index as u32);
                }
            }
        }
        released
    });
    let swept = released.iter().map(|(cell_id, entry)| (*cell_id, entry.js_type())).collect();
    drop(released);
    swept
}

/// `JSCell::type()` do id, ou `None` se não há célula.
pub fn cell_type(cell_id: usize) -> Option<JSType> {
    get_for_marking(cell_id).as_ref().map(CellEntry::js_type)
}

/// O `JSGlobalObject` registrado mais antigo ainda vivo: o reino de quem converte um valor fora de
/// qualquer execução (ver `current_realm`). Com `run_program` é o do programa corrente (ou do retido).
pub fn first_global_object() -> Option<crate::runtime::js_global_object::JSGlobalObjectRef> {
    CELLS.with(|registry| {
        registry.borrow().cells.iter().flatten().find_map(|entry| match entry {
            CellEntry::Scope(JSScopeRef::GlobalObject(global_object)) => Some(Rc::clone(global_object)),
            _ => None,
        })
    })
}

/// `forEachLiveCell(iterationScope, functor)`: as células vivas na ordem de alocação, como um instantâneo
/// (o functor pode alocar ou soltar células sem invalidar a iteração).
pub fn live_cells() -> Vec<CellEntry> {
    CELLS.with(|registry| registry.borrow().cells.iter().flatten().cloned().collect())
}

/// Quantas células estão vivas no registro (entradas preenchidas e não removidas). Serve de medida para
/// testes de vazamento e, depois, para o gatilho do coletor.
pub fn live_cell_count() -> usize {
    CELLS.with(|registry| registry.borrow().cells.iter().flatten().count())
}

/// Bytes do armazenamento indexado (butterfly) de todos os objetos vivos no registro, somados na hora.
/// Não conta um objeto que sobreviva fora do registro (essa diferença é o que `live_buffer_bytes` e a
/// contagem de células ajudam a separar).
pub fn live_butterfly_bytes() -> usize {
    CELLS.with(|registry| {
        registry.borrow().cells.iter().flatten().filter_map(|entry| entry.as_js_object()).map(|object| object.indexed_storage_bytes()).sum()
    })
}

/// O tamanho do heap vivo em classes de alocação (`Heap::m_totalBytesVisited` mais a memória extra): o que a política
/// de `heap_size` usa para dimensionar o ciclo seguinte.
pub(crate) fn live_heap_bytes() -> usize {
    CELLS.with(|registry| registry.borrow().cells.iter().flatten().map(crate::runtime::heap_size::cell_bytes).sum())
}

/// Solta a célula e devolve a entrada. O índice não é reaproveitado (o slot guarda o dono original, que
/// `remove_all_of` não corta), então um id velho nunca passa a apontar para outra célula.
pub fn remove(cell_id: usize) -> Option<CellEntry> {
    CELLS.with(|registry| {
        let mut registry = registry.borrow_mut();
        let index = registry.slot(cell_id)?;
        registry.cells[index].take()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_string::js_string;
    use crate::runtime::vm::VM;
    use crate::wtf::text::wtf_string::String as WtfString;

    #[test]
    fn insert_get_roundtrip_and_id_shape() {
        let vm = VM::new();
        let string = js_string(&vm, &WtfString::from_latin1(b"abc"));
        let id = insert(CellEntry::String(Rc::clone(&string)));
        assert_ne!(id, 0);
        assert_eq!(id & 7, 0);
        assert!(id <= u32::MAX as usize);
        match get(id) {
            Some(CellEntry::String(found)) => assert!(Rc::ptr_eq(&found, &string)),
            _ => panic!("entrada errada"),
        }
    }

    #[test]
    fn ids_are_distinct() {
        let vm = VM::new();
        let a = insert(CellEntry::String(js_string(&vm, &WtfString::from_latin1(b"a"))));
        let b = insert(CellEntry::Symbol(crate::runtime::symbol::Symbol::create(&vm)));
        assert_ne!(a, b);
    }

    #[test]
    fn cell_type_comes_from_the_entry() {
        let vm = VM::new();
        let string = insert(CellEntry::String(js_string(&vm, &WtfString::from_latin1(b"x"))));
        let symbol = insert(CellEntry::Symbol(crate::runtime::symbol::Symbol::create(&vm)));
        assert_eq!(cell_type(string), Some(JSType::StringType));
        assert_eq!(cell_type(symbol), Some(JSType::SymbolType));
    }

    #[test]
    fn wrong_type_and_invalid_ids() {
        let vm = VM::new();
        let id = insert(CellEntry::Symbol(crate::runtime::symbol::Symbol::create(&vm)));
        assert!(!matches!(get(id), Some(CellEntry::String(_))));
        assert!(get(0).is_none());
        assert!(get(id | 1).is_none());
        assert!(get(id + (1 << 20)).is_none());
        assert_eq!(cell_type(0), None);
    }

    #[test]
    fn scope_releases_its_cells_and_trims_ids() {
        let vm = VM::new();
        let outside = insert(CellEntry::Symbol(crate::runtime::symbol::Symbol::create(&vm)));
        let before = live_cell_count();
        let scope = VMScope::enter();
        let first = insert(CellEntry::Symbol(crate::runtime::symbol::Symbol::create(&vm)));
        let reserved = reserve();
        assert!(get(first).is_some());
        drop(scope);
        assert!(get(first).is_none());
        assert!(get(reserved).is_none());
        assert_eq!(live_cell_count(), before);
        assert!(get(outside).is_some());
        // O rabo liberado volta a ser alocável: o próximo escopo recomeça no mesmo id.
        let again = VMScope::enter();
        let second = insert(CellEntry::Symbol(crate::runtime::symbol::Symbol::create(&vm)));
        assert_eq!(second, first);
        drop(again);
    }

    #[test]
    fn run_program_keeps_the_last_program_until_the_next_one() {
        let first = run_program(|| insert(CellEntry::Symbol(crate::runtime::symbol::Symbol::create(&VM::new()))));
        assert!(get(first).is_some());
        run_program(|| ());
        assert!(get(first).is_none());
        release_retired();
    }

    #[test]
    fn reserve_set_and_remove() {
        let vm = VM::new();
        let id = reserve();
        assert!(get(id).is_none());
        assert!(set(id, CellEntry::Symbol(crate::runtime::symbol::Symbol::create(&vm))));
        assert_eq!(cell_type(id), Some(JSType::SymbolType));
        assert!(remove(id).is_some());
        assert!(get(id).is_none());
        assert!(remove(id).is_none());
        assert!(!set(0, CellEntry::Symbol(crate::runtime::symbol::Symbol::create(&vm))));
    }
}
