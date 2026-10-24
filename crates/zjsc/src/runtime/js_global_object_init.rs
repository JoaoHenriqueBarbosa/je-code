//! Porte da parte mínima de `JSGlobalObject::init(VM&)` (`runtime/JSGlobalObject.cpp:1092`) que o
//! marco `1 + 1` e `var x = 1; x + 1` exige: `FunctionPrototype`, `ObjectPrototype`, a ligação entre
//! os dois (`setPrototypeWithoutTransition`), a estrutura de objeto do `Object` constructor, a
//! estrutura de `Array` (com o `ArrayPrototype`) e a única `LinkTimeConstant` sem construtor
//! (`sentinelString`). O restante de `init(vm)` já está em `JSGlobalObject::create` /
//! `finish_creation` (`js_global_object.rs`): `SymbolTable` do global (`Base::finishCreation`),
//! `JSGlobalLexicalEnvironment`, os três callees, `calleeStructure` e todas as estruturas de função.
//!
//! Fora desta fatia, e por quê:
//! - `call`/`apply`/`arguments`/`caller` do `FunctionPrototype`, `toString` do `ObjectPrototype` e
//!   `m_objectProtoValueOfFunction`, e os corpos das funções nativas dos dois
//!   protótipos (ver `function_prototype.rs` e `object_prototype.rs`);
//! - os `LinkTimeConstant` que são construtores, funções ou getters (`Object`, `Array`, `RegExp`,
//!   `Promise`, `String`, `callFunction`, `applyFunction`, `isArray`...): dependem dos mesmos
//!   itens e dos construtores; ficam vazios e `link_time_constant` aborta ao lê-los;
//! - `m_debugger = nullptr` (já é o padrão), os
//!   `LazyProperty` de funções, `GetterSetter` de `@@species`, todos os construtores e protótipos
//!   (Error, Map, Set, Promise, RegExp, Date, JSON, Reflect,
//!   TypedArray, ArrayBuffer, WeakRef, Intl, Temporal, WebAssembly...) e `JS_GLOBAL_OBJECT_ADDITIONS_*`;
//!   `Number`, `Boolean` e `Math` estão aqui (ver `number_constructor.rs`, `boolean_constructor.rs` e
//!   `math_object.rs`);
//! - `convertToDictionary` e o `Debugger`/inspetor/`updateCanFastQueueMicrotask`: só importam
//!   para JIT, inspetor e fila de microtarefa.
//!
//! DIVERGÊNCIAS: `FunctionPrototype` é o `InternalFunction` de `function_prototype.rs` e `ObjectPrototype`
//! o `JSObject` de `object_prototype.rs`; o `ArrayPrototype` é o `JSArray` de `array_prototype.rs`
//! (`ArrayClass`, com as funções nativas). `setPrototypeWithoutTransition` é `set_prototype_direct`
//! (uma transição de protótipo; `Structure` não expõe a versão sem transição). O
//! `FunctionPrototype` nasce antes do global (o C++ o cria dentro de `init`, mas `create` o recebe
//! pronto, ver `js_global_object.rs`), por isso a estrutura dele ganha o `realm` depois.

use std::rc::Rc;

use crate::runtime::array_prototype::ArrayPrototype;
use crate::runtime::bigint_constructor::BigIntConstructor;
use crate::runtime::bigint_object::BigIntObject;
use crate::runtime::bigint_prototype::BigIntPrototype;
use crate::runtime::boolean_constructor::BooleanConstructor;
use crate::runtime::boolean_object::BooleanObject;
use crate::runtime::boolean_prototype::BooleanPrototype;
use crate::runtime::function_prototype::FunctionPrototype;
use crate::runtime::identifier::Identifier;
use crate::runtime::indexing_type::{
    array_index_from_indexing_type, ARRAY_WITH_ARRAY_STORAGE, ARRAY_WITH_CONTIGUOUS, ARRAY_WITH_DOUBLE, ARRAY_WITH_INT32,
    ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE, ARRAY_WITH_UNDECIDED, COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS,
    COPY_ON_WRITE_ARRAY_WITH_DOUBLE, COPY_ON_WRITE_ARRAY_WITH_INT32,
};
use crate::runtime::internal_function::InternalFunctionRef;
use crate::runtime::js_array::JSArray;
use crate::runtime::array_iterator_prototype::ArrayIteratorPrototype;
use crate::runtime::generator_prototype::GeneratorPrototype;
use crate::runtime::iterator_prototype::JSIteratorPrototype;
use crate::runtime::js_array_iterator::JSArrayIterator;
use crate::runtime::js_function::create_builtin_function;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_object::JSFinalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_null, JSValue};
use crate::runtime::math_object::MathObject;
use crate::runtime::number_constructor::NumberConstructor;
use crate::runtime::number_object::NumberObject;
use crate::runtime::number_prototype::NumberPrototype;
use crate::runtime::object_prototype::ObjectPrototype;
use crate::runtime::object_constructor::ObjectConstructor;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::symbol_constructor::SymbolConstructor;
use crate::runtime::symbol_object::SymbolObject;
use crate::runtime::symbol_prototype::SymbolPrototype;
use crate::runtime::string_object::StringObject;
use crate::runtime::string_prototype::StringPrototype;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use std::cell::RefCell;
use crate::bytecode::property_condition::{ObjectPropertyCondition, WatchabilityEffort};
use crate::bytecode::watchpoint::{InlineWatchpointSet, StringFireDetail};
use crate::runtime::js_object::{JSObject, JSObjectHandle};
use crate::runtime::object_adaptive_structure_watchpoint::SpeciesWatchpoint;
use crate::runtime::object_property_change_adaptive_watchpoint::ObjectPropertyChangeAdaptiveWatchpoint;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::wtf::text::wtf_string::String as WtfString;

/// `enum class HasSpeciesProperty : bool { No, Yes }` (JSGlobalObject.h).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HasSpeciesProperty {
    No,
    Yes,
}

impl JSGlobalObject {
    /// `JSGlobalObject::init(VM&)` mais o `create` de `JSGlobalObject::create(vm, structure)`: cria o
    /// `FunctionPrototype`, o global (que monta a `SymbolTable`, o ambiente léxico global, os callees e
    /// as estruturas de função, ver `finish_creation`) e, sobre ele, o `ObjectPrototype` e o resto da
    /// lista do cabeçalho deste módulo (JSGlobalObject.cpp:1110-1214 e 1466).
    pub fn init(vm: &Rc<VM>) -> JSGlobalObjectRef {
        // m_functionPrototype: "The real prototype will be set once ObjectPrototype is created."
        let function_prototype_structure = FunctionPrototype::create_structure(vm, None, js_null());
        let function_prototype = FunctionPrototype::create(vm, Rc::clone(&function_prototype_structure));

        let global_structure = JSGlobalObject::create_structure(vm, js_null());
        let global_object = JSGlobalObject::create(vm, global_structure, function_prototype.as_value());
        *global_object.self_ref.borrow_mut() = Rc::downgrade(&global_object);
        // `NaN`, `Infinity` e `undefined` vivem só na `SymbolTable` do global (`init_static_globals`), como no
        // C++; a `Structure` do global não os tem. O realm vai na `Structure` atual do `Function.prototype` e
        // na raiz.
        function_prototype_structure.set_realm(vm, &global_object);
        function_prototype.structure().set_realm(vm, &global_object);
        function_prototype.set_global_object(Rc::clone(&global_object));
        *global_object.function_prototype.borrow_mut() = Some(Rc::clone(&function_prototype));

        // m_objectPrototype, e "We have to manually set this here because we make it a prototype without
        // transition below" (`didBecomePrototype`).
        let object_prototype_structure = ObjectPrototype::create_structure(vm, &global_object, js_null());
        let object_prototype = ObjectPrototype::create(vm, &global_object, &object_prototype_structure);
        object_prototype.did_become_prototype(vm);
        global_object.set_object_prototype(Rc::clone(&object_prototype));
        // O acessor `__proto__` do `Object.prototype` (`GetterSetter` de `globalFuncProtoGetter/Setter`).
        crate::runtime::js_global_object_functions_natives::add_underscore_proto_accessor(&global_object, &object_prototype);
        // m_functionPrototype->structure()->setPrototypeWithoutTransition(vm, m_objectPrototype.get());
        function_prototype.set_prototype_direct(vm, object_prototype.as_value());
        // O próprio objeto global herda de `Object.prototype` (`globalThis.hasOwnProperty`).
        global_object.set_prototype_direct(vm, object_prototype.as_value());
        // O `JSGlobalProxy` (`globalThis`) nasceu com o protótipo que o global tinha então (o `Function.prototype`
        // do `create`); `JSGlobalProxy::setTarget` copia o do alvo, e o do alvo só ficou certo agora.
        if let Some(global_this) = global_object.global_this() {
            global_this.set_prototype_direct(vm, object_prototype.as_value());
        }
        // m_functionPrototype->addFunctionProperties(...): `call` e `apply` (builtins) ficam de fora, ver
        // `function_prototype.rs`.
        FunctionPrototype::add_function_properties(&function_prototype, vm, &global_object);

        // `globalObjectTable` (`isNaN`, `parseInt`, `encodeURI`...) e `m_parseIntFunction`/`m_parseFloatFunction`.
        // O bun lista `isNaN`...`parseFloat` logo depois de `Infinity`, `undefined`, `NaN`, antes de qualquer
        // construtor, por isso entram aqui (só precisam da `hostFunctionStructure` e do `globalThis`).
        crate::runtime::js_global_object_functions_natives::add_global_functions(&global_object);

        // m_objectStructureForObjectConstructor.
        let object_structure = global_object.structure_cache().empty_object_structure_for_prototype(
            &global_object,
            &object_prototype,
            JSFinalObject::DEFAULT_INLINE_CAPACITY,
            false,
        );
        *global_object.object_structure_for_object_constructor.borrow_mut() = Some(object_structure);

        // m_arrayPrototype (`ArrayPrototype::create(vm, this, ArrayPrototype::createStructure(vm, this,
        // m_objectPrototype))`) e a estrutura de array sobre ele (`JSArray::createStructure(vm, this,
        // m_arrayPrototype, ArrayWithUndecided)`). O `Array` constructor e o `@@species` esperam o global
        // guardar o construtor (ver `array_constructor.rs`).
        let array_prototype_structure = ArrayPrototype::create_structure(vm, &global_object, object_prototype.as_value());
        let array_prototype = ArrayPrototype::create(vm, &global_object, &array_prototype_structure);
        array_prototype.did_become_prototype(vm);
        global_object.init_array_structures(vm, array_prototype.as_value());

        // m_stringPrototype (`StringPrototype::create(vm, this, StringPrototype::createStructure(vm, this,
        // m_objectPrototype))`, um `StringObject` com a string vazia) e `m_stringObjectStructure`. As
        // funções e o `constructor` do protótipo esperam a `NativeFunction` que alcança a pilha.
        let string_prototype_structure =
            StringPrototype::create_structure(vm, Some(&global_object), object_prototype.as_value());
        let string_prototype = StringPrototype::create(vm, &global_object, string_prototype_structure);
        string_prototype.did_become_prototype(vm);
        let string_object_structure = StringObject::create_structure(vm, Some(&global_object), string_prototype.as_value());
        *global_object.string_object_structure.borrow_mut() = Some(string_object_structure);
        *global_object.string_prototype.borrow_mut() = Some(Rc::clone(&string_prototype));

        // `String` (`StringConstructor::create`, que liga o `constructor` do protótipo) e a propriedade global.
        let string_constructor_structure = crate::runtime::string_constructor_natives::StringConstructor::create_structure(
            vm,
            &global_object,
            function_prototype.as_value(),
        );
        let string_constructor = crate::runtime::string_constructor_natives::StringConstructor::create(
            vm,
            &global_object,
            string_constructor_structure,
            &string_prototype,
        );
        global_object.put_direct(
            vm,
            &crate::runtime::property_name::PropertyName::from_identifier(&Identifier::from_span(vm, b"String".as_slice())),
            string_constructor.as_value(),
            crate::runtime::property_attribute::DONT_ENUM,
        );

        // m_regExpPrototype e o `RegExp` (que liga o `constructor` do protótipo) com a propriedade global.
        // A estrutura de `RegExpObject` (`regExpStructure()`) é preguiçosa e lê o protótipo do campo.
        let reg_exp_prototype_structure =
            crate::runtime::reg_exp_prototype::RegExpPrototype::create_structure(vm, &global_object, object_prototype.as_value());
        let reg_exp_prototype =
            crate::runtime::reg_exp_prototype::RegExpPrototype::create(vm, &global_object, &reg_exp_prototype_structure);
        reg_exp_prototype.did_become_prototype(vm);
        *global_object.reg_exp_prototype.borrow_mut() = Some(Rc::clone(&reg_exp_prototype));
        // `installObjectPropertyChangeAdaptiveWatchpoint(..., m_regExpPrimordialPropertiesWatchpointSet)`
        // para as quinze propriedades de `JSGlobalObject::init`.
        {
            let names = &vm.property_names;
            let watched: Vec<PropertyName> = [
                &names.exec,
                &names.flags,
                &names.dot_all,
                &names.global,
                &names.has_indices,
                &names.ignore_case,
                &names.multiline,
                &names.sticky,
                &names.unicode,
                &names.unicode_sets,
                &names.replace_symbol,
                &names.match_symbol,
                &names.search_symbol,
                &names.match_all_symbol,
                &names.split_symbol,
            ]
            .into_iter()
            .map(PropertyName::from_identifier)
            .collect();
            reg_exp_prototype.watch_property_replacement(vm, &watched, &global_object.reg_exp_primordial_properties_fired);
        }
        let reg_exp_constructor_structure = crate::runtime::reg_exp_prototype_natives::RegExpConstructor::create_structure(
            vm,
            &global_object,
            function_prototype.as_value(),
        );
        let reg_exp_constructor =
            crate::runtime::reg_exp_prototype_natives::RegExpConstructor::create(vm, reg_exp_constructor_structure, &reg_exp_prototype);
        global_object.string_regexp_globals.borrow_mut().reg_exp_constructor = Some(reg_exp_constructor.as_value());
        global_object.put_direct(
            vm,
            &crate::runtime::property_name::PropertyName::from_identifier(&Identifier::from_span(vm, b"RegExp".as_slice())),
            reg_exp_constructor.as_value(),
            crate::runtime::property_attribute::DONT_ENUM,
        );

        // `Error`, `Error.prototype`, os seis nativos e as estruturas de `ErrorInstance` por tipo.
        crate::runtime::error_natives::init_error_classes(
            vm,
            &global_object,
            object_prototype.as_value(),
            function_prototype.as_value(),
        );
        crate::runtime::aggregate_error::install_aggregate_error(&global_object);
        crate::runtime::suppressed_error::install_suppressed_error(&global_object);

        // As funções globais (`globalObjectTable`) já entraram depois de `NaN`; ver acima.

        // m_symbolPrototype (`SymbolPrototype::create(vm, this, SymbolPrototype::createStructure(vm, this,
        // m_objectPrototype))`), m_symbolObjectStructure e `Symbol` (`SymbolConstructor`), e depois o
        // `Object` (`ObjectConstructor`) com o `constructor` do `Object.prototype`. A ordem do C++
        // (JSGlobalObject.cpp, `init`) só importa para o que cada `finishCreation` lê do global: o
        // protótipo da função (já ligado acima) e o `ObjectPrototype`.
        let symbol_prototype_structure = SymbolPrototype::create_structure(vm, &global_object, object_prototype.as_value());
        let symbol_prototype = SymbolPrototype::create(vm, &global_object, &symbol_prototype_structure);
        let symbol_object_structure = SymbolObject::create_structure(vm, Some(&global_object), symbol_prototype.as_value());
        *global_object.symbol_object_structure.borrow_mut() = Some(symbol_object_structure);

        let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);
        let function_prototype_value = function_prototype.as_value();
        let symbol_constructor_structure = SymbolConstructor::create_structure(vm, &global_object, function_prototype_value);
        let symbol_constructor =
            SymbolConstructor::create(vm, &global_object, symbol_constructor_structure, &symbol_prototype);
        symbol_prototype.put_direct(vm, &constructor_name, symbol_constructor.as_value(), DONT_ENUM);
        SymbolPrototype::install_symbol_keyed_properties(&symbol_prototype, vm, &global_object);
        global_object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.symbol), symbol_constructor.as_value(), DONT_ENUM);

        let object_constructor_structure = ObjectConstructor::create_structure(vm, &global_object, function_prototype_value);
        let object_constructor =
            ObjectConstructor::create(vm, &global_object, object_constructor_structure, &object_prototype);
        object_prototype.put_direct(vm, &constructor_name, object_constructor.as_value(), DONT_ENUM);
        global_object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.object), object_constructor.as_value(), DONT_ENUM);

        // `Array` (`ArrayConstructor`, o `m_arrayStructure` `ClassStructure`): o `constructor` do
        // `Array.prototype` e a propriedade global `DontEnum`.
        let array_constructor_structure =
            crate::runtime::array_constructor::ArrayConstructor::create_structure(vm, &global_object, function_prototype_value);
        let array_constructor = crate::runtime::array_constructor::ArrayConstructor::create(
            vm,
            &global_object,
            array_constructor_structure,
            &array_prototype,
        );
        array_prototype.put_direct(vm, &constructor_name, array_constructor.as_value(), DONT_ENUM);
        global_object.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"Array".as_slice())), array_constructor.as_value(), DONT_ENUM);

        // `Number` e `Boolean` (os `LazyClassStructure`: protótipo, estrutura do objeto e construtor, com o
        // `constructor` do protótipo ligado por `setConstructor`) e a propriedade global de cada um. O
        // `Number` lê `parseInt` e `parseFloat` do global, por isso vem depois de
        // `add_global_functions`.
        let number_prototype_structure = NumberPrototype::create_structure(vm, Some(&global_object), object_prototype.as_value());
        let number_prototype = NumberPrototype::create(vm, &global_object, number_prototype_structure);
        number_prototype.did_become_prototype(vm);
        let number_object_structure = NumberObject::create_structure(vm, Some(&global_object), number_prototype.as_value());
        *global_object.number_object_structure.borrow_mut() = Some(number_object_structure);
        let number_constructor = NumberConstructor::create(vm, &global_object, &number_prototype);
        number_prototype.put_direct(vm, &constructor_name, number_constructor.as_value(), DONT_ENUM);
        global_object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.number), number_constructor.as_value(), DONT_ENUM);

        let boolean_prototype_structure = BooleanPrototype::create_structure(vm, Some(&global_object), object_prototype.as_value());
        let boolean_prototype = BooleanPrototype::create(vm, &global_object, boolean_prototype_structure);
        boolean_prototype.did_become_prototype(vm);
        let boolean_object_structure = BooleanObject::create_structure(vm, Some(&global_object), boolean_prototype.as_value());
        *global_object.boolean_object_structure.borrow_mut() = Some(boolean_object_structure);
        let boolean_constructor = BooleanConstructor::create(vm, &global_object, &boolean_prototype);
        boolean_prototype.put_direct(vm, &constructor_name, boolean_constructor.as_value(), DONT_ENUM);
        global_object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.boolean), boolean_constructor.as_value(), DONT_ENUM);

        // `BigInt` (`m_bigIntObjectStructure`): protótipo, estrutura do objeto, construtor e propriedade global.
        let big_int_prototype_structure = BigIntPrototype::create_structure(vm, &global_object, object_prototype.as_value());
        let big_int_prototype = BigIntPrototype::create(vm, &global_object, &big_int_prototype_structure);
        big_int_prototype.did_become_prototype(vm);
        let big_int_object_structure = BigIntObject::create_structure(vm, Some(&global_object), big_int_prototype.as_value());
        *global_object.big_int_object_structure.borrow_mut() = Some(big_int_object_structure);
        let big_int_constructor = BigIntConstructor::create(vm, &global_object, function_prototype_value, &big_int_prototype);
        big_int_prototype.put_direct(vm, &constructor_name, big_int_constructor.as_value(), DONT_ENUM);
        global_object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.big_int), big_int_constructor.as_value(), DONT_ENUM);

        // `Date` (`m_dateStructure`, `ClassStructure`): protótipo, estrutura, construtor e propriedade global.
        crate::runtime::date_constructor_natives::install_date(vm, &global_object, object_prototype.as_value(), function_prototype.as_value());
        global_object.array_buffer_realm.init(vm, &global_object, &object_prototype, function_prototype.as_value(), crate::runtime::options::Options::use_shared_array_buffer());

        // `Math` (`createMathProperty`, `MathObject::create(vm, global, MathObject::createStructure(vm,
        // global, global->objectPrototype()))`): o C++ cria o objeto na primeira leitura da propriedade
        // `DontEnum|PropertyCallback`, aqui ele nasce junto do global.
        let math_structure = MathObject::create_structure(vm, &global_object, object_prototype.as_value());
        let math_object = MathObject::create(vm, &global_object, &math_structure);
        let math_name = Identifier::from_string(vm, &WtfString::from_latin1(b"Math"));
        global_object.put_direct(vm, &PropertyName::from_identifier(&math_name), math_object.as_value(), DONT_ENUM);

        // m_iteratorPrototype, m_generatorPrototype, m_arrayIteratorPrototype e m_arrayIteratorStructure, na
        // ordem de JSGlobalObject.cpp:1426-1447 (os demais iteradores e os assíncronos ainda não existem).
        let iterator_prototype_structure = JSIteratorPrototype::create_structure(vm, &global_object, object_prototype.as_value());
        let iterator_prototype = JSIteratorPrototype::create(vm, &global_object, &iterator_prototype_structure);
        *global_object.iterator_prototype.borrow_mut() = Some(Rc::clone(&iterator_prototype));
        // `m_iteratorProtoSymbolIteratorFunction`: o `@@iterator` que o `finishCreation` do protótipo instalou.
        global_object.iteration_protocol.borrow_mut().iterator_proto_symbol_iterator_function =
            Some(iterator_prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.iterator_symbol)));

        let generator_prototype_structure = GeneratorPrototype::create_structure(vm, &global_object, iterator_prototype.as_value());
        let generator_prototype = GeneratorPrototype::create(vm, &global_object, &generator_prototype_structure);
        *global_object.generator_prototype.borrow_mut() = Some(generator_prototype);
        crate::runtime::function_kind_intrinsics::install_function_kind_intrinsics(
            vm,
            &global_object,
            object_prototype.as_value(),
            &function_prototype,
            iterator_prototype.as_value(),
        );
        crate::runtime::iterator_constructor::install_iterator_classes(vm, &global_object, function_prototype.as_value());

        let array_iterator_prototype_structure =
            ArrayIteratorPrototype::create_structure(vm, &global_object, iterator_prototype.as_value());
        let array_iterator_prototype = ArrayIteratorPrototype::create(vm, &global_object, &array_iterator_prototype_structure);
        let array_iterator_structure =
            JSArrayIterator::create_structure(vm, Some(&global_object), array_iterator_prototype.as_value());
        *global_object.array_iterator_prototype.borrow_mut() = Some(array_iterator_prototype);
        *global_object.array_iterator_proto_next.borrow_mut() = Some(
            global_object
                .array_iterator_prototype()
                .get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.next)),
        );
        *global_object.array_iterator_structure.borrow_mut() = Some(array_iterator_structure);

        // m_proxyObjectStructure, m_callableProxyObjectStructure, m_proxyRevokeStructure e `Proxy`
        // (`ProxyConstructor`, `putDirectWithoutTransition` no global como `DontEnum`).
        *global_object.proxy_object_structure.borrow_mut() =
            Some(crate::runtime::proxy_object::ProxyObject::create_structure(vm, Some(&global_object), js_null(), false));
        *global_object.callable_proxy_object_structure.borrow_mut() =
            Some(crate::runtime::proxy_object::ProxyObject::create_structure(vm, Some(&global_object), js_null(), true));
        *global_object.proxy_revoke_structure.borrow_mut() = Some(crate::runtime::proxy_revoke::ProxyRevoke::create_structure(
            vm,
            Some(&global_object),
            function_prototype.as_value(),
        ));
        let proxy_constructor_structure = crate::runtime::proxy_constructor::ProxyConstructor::create_structure(
            vm,
            Some(&global_object),
            function_prototype.as_value(),
        );
        let proxy_constructor = crate::runtime::proxy_constructor::ProxyConstructor::create(vm, proxy_constructor_structure);
        global_object.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"Proxy")), proxy_constructor.as_value(), DONT_ENUM);

        // `Reflect`, `JSON`, `Map`, `Set`, `WeakMap`, `WeakSet` e os iteradores de `Map` e `Set`.
        global_object.install_json_reflect_and_collections(vm, &object_prototype, &function_prototype, &iterator_prototype);
        crate::runtime::weak_ref_globals::install_weak_refs(&global_object, &object_prototype, function_prototype.as_value());
        crate::runtime::atomics_object::install_atomics(&global_object, &object_prototype);
        crate::runtime::intl_object::install_intl(&global_object);
        if crate::runtime::options::Options::use_wasm() {
            let web_assembly = crate::runtime::js_web_assembly::install_web_assembly(&global_object);
            global_object.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"WebAssembly")), web_assembly.as_value(), DONT_ENUM);
        }
        if crate::runtime::options::Options::use_temporal() {
            crate::runtime::temporal_object::install_temporal(&global_object, &object_prototype);
        }
        crate::runtime::console_object::install_console(&global_object);
        crate::runtime::shadow_realm_globals::install_shadow_realm(&global_object);
        global_object.init_static_globals();
        if crate::runtime::options::Options::use_explicit_resource_management() {
            crate::runtime::disposable_stack_globals::install_disposable_stacks(&global_object);
        }

        // m_stringIteratorPrototype, m_stringIteratorStructure, m_regExpStringIteratorPrototype e
        // m_regExpStringIteratorStructure (JSGlobalObject.cpp:1450-1464).
        let string_iterator_prototype_structure =
            crate::runtime::string_iterator_prototype::StringIteratorPrototype::create_structure(
                vm,
                &global_object,
                iterator_prototype.as_value(),
            );
        let string_iterator_prototype = crate::runtime::string_iterator_prototype::StringIteratorPrototype::create(
            vm,
            &global_object,
            &string_iterator_prototype_structure,
        );
        let string_iterator_structure = crate::runtime::js_string_iterator::JSStringIterator::create_structure(
            vm,
            Some(&global_object),
            string_iterator_prototype.as_value(),
        );
        {
            let mut protocol = global_object.iteration_protocol.borrow_mut();
            protocol.string_iterator_next =
                Some(string_iterator_prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.next)));
            protocol.string_iterator_prototype = Some(Rc::clone(&string_iterator_prototype));
        }
        let reg_exp_string_iterator_prototype_structure =
            crate::runtime::reg_exp_string_iterator_prototype::RegExpStringIteratorPrototype::create_structure(
                vm,
                &global_object,
                iterator_prototype.as_value(),
            );
        let reg_exp_string_iterator_prototype =
            crate::runtime::reg_exp_string_iterator_prototype::RegExpStringIteratorPrototype::create(
                vm,
                &global_object,
                &reg_exp_string_iterator_prototype_structure,
            );
        let reg_exp_string_iterator_structure = crate::runtime::js_reg_exp_string_iterator::JSRegExpStringIterator::create_structure(
            vm,
            Some(&global_object),
            reg_exp_string_iterator_prototype.as_value(),
        );
        {
            let mut globals = global_object.string_regexp_globals.borrow_mut();
            globals.string_iterator_structure = Some(string_iterator_structure);
            globals.reg_exp_string_iterator_structure = Some(reg_exp_string_iterator_structure);
        }

        // `INIT_PRIVATE_GLOBAL(arrayIteratorNextHelper, ...)` (o `LazyProperty` do C++, criado aqui de uma vez).
        let array_iterator_next_helper = create_builtin_function(
            vm,
            &global_object,
            crate::runtime::builtins_source::BuiltinCodeIndex::ArrayIteratorPrototypeArrayIteratorNextHelperCode,
        );
        global_object.set_link_time_constant(LinkTimeConstant::ArrayIteratorNextHelper, array_iterator_next_helper.as_value());

        // `INIT_PRIVATE_GLOBAL` dos quatro `@linkTimeConstant` de `DisposableStackPrototype.js`, que o
        // bytecompiler e os builtins de DisposableStack/AsyncDisposableStack usam.
        {
            use crate::runtime::builtins_source::BuiltinCodeIndex as Code;
            for (constant, index) in [
                (LinkTimeConstant::AddDisposableResource, Code::DisposableStackPrototypeAddDisposableResourceCode),
                (LinkTimeConstant::CreateDisposableResource, Code::DisposableStackPrototypeCreateDisposableResourceCode),
                (LinkTimeConstant::GetDisposeMethod, Code::DisposableStackPrototypeGetDisposeMethodCode),
                (LinkTimeConstant::GetAsyncDisposeMethod, Code::DisposableStackPrototypeGetAsyncDisposeMethodCode),
            ] {
                let function = create_builtin_function(vm, &global_object, index);
                global_object.set_link_time_constant(constant, function.as_value());
            }
        }

        global_object.init_promise();
        // JSGlobalObject.cpp:2390: `tryInstallSpeciesWatchpoint(arrayPrototype(), arrayConstructor, ...)`, depois de
        // todos os construtores existirem e antes de `installSaneChainWatchpoints`; o Promise (2394) e o RegExp
        // (2407) vêm logo depois, na mesma região.
        {
            let prototype = crate::runtime::js_object::JSObject::from_value(&array_prototype.as_value()).expect("Array.prototype é objeto");
            let constructor = crate::runtime::js_object::JSObject::from_value(&array_constructor.as_value()).expect("Array é objeto");
            // `arraySpeciesGetterSetter()`: aqui o `GetterSetter` que o próprio `ArrayConstructor` instalou em
            // `@@species` (ver o cabeçalho de `array_constructor.rs`).
            let mut species_slot = PropertySlot::new(constructor.as_value(), InternalMethodType::VMInquiry);
            constructor.get_own_property_slot(vm, &PropertyName::from_identifier(&vm.property_names.species_symbol), &mut species_slot);
            let species_getter_setter = constructor.get_direct(species_slot.cached_offset());
            global_object.try_install_species_watchpoint(
                vm,
                &prototype,
                &constructor,
                &global_object.array_prototype_constructor_watchpoint,
                &global_object.array_constructor_species_watchpoint,
                &global_object.array_species_watchpoint_set,
                HasSpeciesProperty::Yes,
                species_getter_setter,
            );
        }
        // JSGlobalObject.cpp:2394: Promise, com o `promiseSpeciesGetterSetter()` que `init_promise` guardou.
        {
            let (prototype_value, constructor_value, species_getter_setter) = {
                let data = global_object.promise_data.borrow();
                (
                    data.prototype.expect("Promise.prototype criado por init_promise"),
                    data.constructor.expect("Promise criado por init_promise"),
                    data.species_getter_setter.expect("Promise[@@species] criado por init_promise"),
                )
            };
            let prototype = crate::runtime::js_object::JSObject::from_value(&prototype_value).expect("Promise.prototype é objeto");
            let constructor = crate::runtime::js_object::JSObject::from_value(&constructor_value).expect("Promise é objeto");
            global_object.try_install_species_watchpoint(
                vm,
                &prototype,
                &constructor,
                &global_object.promise_prototype_constructor_watchpoint,
                &global_object.promise_constructor_species_watchpoint,
                &global_object.promise_species_watchpoint_set,
                HasSpeciesProperty::Yes,
                species_getter_setter,
            );
        }
        // JSGlobalObject.cpp:2399-2408: RegExp. Só instala se `RegExp[@@species]` for acessor; senão o set fica
        // em `ClearWatchpoint`, como no C++.
        {
            let prototype = crate::runtime::js_object::JSObject::from_value(&reg_exp_prototype.as_value()).expect("RegExp.prototype é objeto");
            let constructor = crate::runtime::js_object::JSObject::from_value(&reg_exp_constructor.as_value()).expect("RegExp é objeto");
            let mut species_slot = PropertySlot::new(constructor.as_value(), InternalMethodType::VMInquiry);
            let found = constructor.get_own_property_slot(vm, &PropertyName::from_identifier(&vm.property_names.species_symbol), &mut species_slot);
            if found && species_slot.is_accessor() {
                let species_getter_setter = constructor.get_direct(species_slot.cached_offset());
                global_object.try_install_species_watchpoint(
                    vm,
                    &prototype,
                    &constructor,
                    &global_object.reg_exp_prototype_constructor_watchpoint,
                    &global_object.reg_exp_constructor_species_watchpoint,
                    &global_object.reg_exp_species_watchpoint_set,
                    HasSpeciesProperty::Yes,
                    species_getter_setter,
                );
            }
        }
        // m_linkTimeConstants[LinkTimeConstant::sentinelString].set(vm, this, vm.smallStrings.sentinelString()).
        // SmallStrings::initialize: JSString::create(vm, AtomStringImpl::add("$")), o texto é átomo.
        let sentinel_string = js_string(vm, crate::wtf::text::atom_string::AtomString::from_latin1(b"$").string());
        global_object.set_link_time_constant(LinkTimeConstant::SentinelString, JSValue::from_cell(sentinel_string.cell_id()));

        // m_linkTimeConstants[LinkTimeConstant::emptyPropertyNameEnumerator].set(vm, this, vm.emptyPropertyNameEnumerator()).
        global_object.set_link_time_constant(
            LinkTimeConstant::EmptyPropertyNameEnumerator,
            crate::runtime::js_property_name_enumerator::JSPropertyNameEnumerator::create(vm, 0, 0, Vec::new()).as_value(),
        );
        crate::runtime::js_global_object_link_time_constants::init_link_time_constants(&global_object);
        // ZigGlobalObject.cpp:2883: o host acrescenta 23 globais privados à SymbolTable depois do `init` do JSC.
        global_object.init_host_static_globals();
        crate::runtime::js_module_loader::install_message_classes(&global_object);
        crate::runtime::js_dom_exception::install_dom_exception(&global_object);
        crate::runtime::text_encoder::install_text_encoder(&global_object);
        crate::runtime::event_target::install_event_target(&global_object);
        crate::runtime::text_decoder::install_text_decoder(&global_object);
        crate::runtime::queuing_strategy::install_queuing_strategies(&global_object);
        crate::runtime::streams::install_streams(&global_object);
        crate::runtime::performance::install_performance(&global_object);
        crate::runtime::url_search_params::install_url_search_params(&global_object);
        crate::runtime::url::install_url(&global_object);
        crate::runtime::url_pattern::install_url_pattern(&global_object);
        crate::runtime::blob::install_blob(&global_object);
        crate::runtime::file::install_file(&global_object);
        crate::runtime::form_data::install_form_data(&global_object);
        crate::runtime::headers::install_headers(&global_object);
        crate::runtime::response::install_response(&global_object);
        crate::runtime::request::install_request(&global_object);
        crate::runtime::fetch::install_fetch(&global_object);
        // Só depois de todos os globais do bun existirem: a ordem de `Reflect.ownKeys(globalThis)` mistura os
        // do JSC e os do bun, então a reordenação é a última palavra sobre as posições.
        crate::runtime::global_aliases::install_global(&global_object);
        // O global é um `EventTarget` (`addEventListener` & cia entram antes da reordenação).
        crate::runtime::event_target::install_global_event_target(&global_object);
        crate::runtime::navigator::install_navigator(&global_object);
        crate::runtime::bun_global::install_bun(&global_object);
        crate::runtime::crypto::install_crypto(&global_object);
        crate::runtime::process_object::complete_process(&global_object);
        // `Buffer` entra antes da reordenação, que o põe depois de `Blob` (a posição vem da lista `ORDER`).
        crate::runtime::node_buffer::install_buffer(&global_object);
        global_object.reorder_standard_globals(vm);
        // `self` é acessor, e a reordenação não move acessores: instala depois, na posição do bun.
        crate::runtime::global_aliases::install_self(&global_object);
        // O global é um `EventTarget`: depois de `self` as chaves terminam em `onmessage, onerror`.
        crate::runtime::event_target::install_global_event_handlers(&global_object);

        global_object
    }

    /// Reordena as propriedades globais na ordem de `Object.getOwnPropertyNames(globalThis)` do bun (medida
    /// por `scripts/gen-global-order-golden.js`, ver `wip-notes/own-keys-audit.md`); nomes que o porte não
    /// tem são ignorados, então a lista inclui os do bun que ele ainda não instala. Os objetos continuam nascendo na ordem das dependências do
    /// `init`; só a posição da propriedade no global muda: cada uma é removida e regravada com os mesmos
    /// atributos, na sequência da lista. Acessores e propriedades de callback ficam onde estão.
    fn reorder_standard_globals(&self, vm: &VM) {
        use crate::runtime::property_attribute::{ACCESSOR, CUSTOM_ACCESSOR, CUSTOM_VALUE, PROPERTY_CALLBACK};
        use crate::runtime::property_offset::is_valid_offset;
        const ORDER: &[&[u8]] = &[
            b"Infinity", b"undefined", b"NaN", b"addEventListener", b"alert", b"atob", b"btoa",
            b"clearImmediate", b"clearInterval", b"clearTimeout", b"confirm", b"dispatchEvent",
            b"fetch", b"postMessage", b"prompt", b"queueMicrotask", b"removeEventListener",
            b"reportError", b"setImmediate", b"setInterval", b"setTimeout", b"structuredClone",
            b"global", b"Bun", b"File", b"crypto", b"navigator", b"performance", b"process", b"Blob",
            b"Buffer", b"BuildError", b"BuildMessage", b"Crypto", b"HTMLRewriter", b"Request",
            b"ResolveError", b"ResolveMessage", b"Response", b"TextDecoder", b"AbortController",
            b"AbortSignal", b"BroadcastChannel", b"ByteLengthQueuingStrategy", b"CloseEvent",
            b"CompressionStream", b"CountQueuingStrategy", b"CryptoKey", b"CustomEvent",
            b"DecompressionStream", b"DOMException", b"ErrorEvent", b"Event", b"EventTarget",
            b"FormData", b"Headers", b"MessageChannel", b"MessageEvent", b"MessagePort", b"Performance",
            b"PerformanceEntry", b"PerformanceMark", b"PerformanceMeasure", b"PerformanceObserver",
            b"PerformanceObserverEntryList", b"PerformanceResourceTiming", b"PerformanceServerTiming",
            b"PerformanceTiming", b"ReadableByteStreamController", b"ReadableStream",
            b"ReadableStreamBYOBReader", b"ReadableStreamBYOBRequest",
            b"ReadableStreamDefaultController", b"ReadableStreamDefaultReader", b"SubtleCrypto",
            b"TextDecoderStream", b"TextEncoder", b"TextEncoderStream", b"TransformStream",
            b"TransformStreamDefaultController", b"URL", b"URLPattern", b"URLSearchParams",
            b"WebSocket", b"Worker", b"WritableStream", b"WritableStreamDefaultController",
            b"WritableStreamDefaultWriter", b"isNaN", b"isFinite", b"escape", b"unescape", b"decodeURI",
            b"decodeURIComponent", b"encodeURI", b"encodeURIComponent", b"eval", b"globalThis",
            b"parseInt", b"parseFloat", b"ArrayBuffer", b"EvalError", b"RangeError", b"ReferenceError",
            b"SyntaxError", b"TypeError", b"URIError", b"AggregateError", b"SuppressedError", b"Proxy",
            b"Reflect", b"JSON", b"Math", b"Atomics", b"WebAssembly", b"console", b"Int8Array",
            b"Int16Array", b"Int32Array", b"Uint8Array", b"Uint8ClampedArray", b"Uint16Array",
            b"Uint32Array", b"Float16Array", b"Float32Array", b"Float64Array", b"BigInt64Array",
            b"BigUint64Array", b"DataView", b"Date", b"Error", b"Boolean", b"Map", b"Number", b"Set",
            b"WeakMap", b"WeakSet", b"WeakRef", b"FinalizationRegistry", b"Object", b"Function",
            b"Array", b"RegExp", b"Iterator", b"SharedArrayBuffer", b"DisposableStack",
            b"AsyncDisposableStack", b"String", b"Promise", b"BigInt", b"Symbol", b"Intl", b"Temporal",
            b"ShadowRealm", b"self", b"onmessage", b"onerror",
        ];
        for name in ORDER {
            let property_name = PropertyName::from_identifier(&Identifier::from_span(vm, *name));
            let (offset, attributes) = self.get_direct_offset_with_attributes(vm, &property_name);
            if !is_valid_offset(offset) || attributes & (ACCESSOR | CUSTOM_ACCESSOR | CUSTOM_VALUE | PROPERTY_CALLBACK) != 0 {
                continue;
            }
            let value = self.get_direct(offset);
            let structure = self.structure();
            if structure.is_uncacheable_dictionary() {
                let removed = structure.remove_property_without_transition(vm, &property_name);
                if is_valid_offset(removed) {
                    self.put_direct_offset(vm, removed, JSValue::empty());
                }
            } else {
                let (new_structure, removed) = crate::runtime::structure::Structure::remove_property_transition(vm, &structure, &property_name);
                crate::runtime::js_object::JSObject::set_structure(self, vm, &new_structure);
                if is_valid_offset(removed) {
                    self.put_direct_offset(vm, removed, JSValue::empty());
                }
            }
            self.put_direct(vm, &property_name, value, attributes);
        }
    }

    /// `functionPrototype()`.
    pub fn function_prototype(&self) -> InternalFunctionRef {
        self.function_prototype.borrow().clone().expect("JSGlobalObject sem functionPrototype")
    }

    /// `objectStructureForObjectConstructor()`.
    pub fn object_structure_for_object_constructor(&self) -> StructureRef {
        self.object_structure_for_object_constructor
            .borrow()
            .clone()
            .expect("JSGlobalObject sem objectStructureForObjectConstructor")
    }

    /// `stringObjectStructure()`.
    pub fn string_object_structure(&self) -> StructureRef {
        self.string_object_structure.borrow().clone().expect("JSGlobalObject sem stringObjectStructure")
    }

    /// `symbolObjectStructure()`.
    pub fn symbol_object_structure(&self) -> StructureRef {
        self.symbol_object_structure.borrow().clone().expect("JSGlobalObject sem symbolObjectStructure")
    }

    /// `proxyObjectStructure()`.
    pub fn proxy_object_structure(&self) -> StructureRef {
        self.proxy_object_structure.borrow().clone().expect("JSGlobalObject sem proxyObjectStructure")
    }

    /// `callableProxyObjectStructure()`.
    pub fn callable_proxy_object_structure(&self) -> StructureRef {
        self.callable_proxy_object_structure.borrow().clone().expect("JSGlobalObject sem callableProxyObjectStructure")
    }

    /// `proxyRevokeStructure()`.
    pub fn proxy_revoke_structure(&self) -> StructureRef {
        self.proxy_revoke_structure.borrow().clone().expect("JSGlobalObject sem proxyRevokeStructure")
    }

    /// `numberObjectStructure()`.
    pub fn number_object_structure(&self) -> StructureRef {
        self.number_object_structure.borrow().clone().expect("JSGlobalObject sem numberObjectStructure")
    }

    /// `bigIntObjectStructure()`.
    pub fn big_int_object_structure(&self) -> StructureRef {
        self.big_int_object_structure.borrow().clone().expect("JSGlobalObject sem bigIntObjectStructure")
    }

    /// `booleanObjectStructure()`.
    pub fn boolean_object_structure(&self) -> StructureRef {
        self.boolean_object_structure.borrow().clone().expect("JSGlobalObject sem booleanObjectStructure")
    }

    /// `weakRandomNumber()`.
    pub fn weak_random_number(&self) -> f64 {
        self.weak_random.borrow_mut().get()
    }

    /// O `Math.random` do realm passa a ser o do V8 semeado com `seed` (o navegador com `--random-seed`).
    pub fn set_v8_random_seed(&self, seed: u64) {
        *self.weak_random.borrow_mut() = crate::wtf::weak_random::WeakRandom::v8_seeded(seed);
    }

    /// `stringPrototype()`.
    pub fn string_prototype(&self) -> crate::runtime::string_object::StringObjectRef {
        self.string_prototype.borrow().clone().expect("JSGlobalObject sem stringPrototype")
    }

    /// A parte de `init(vm)` que preenche `m_originalArrayStructureForIndexingShape` (uma estrutura de
    /// array por forma, sobre o `ArrayPrototype`) e copia a tabela para
    /// `m_arrayStructureForIndexingShapeDuringAllocation` (JSGlobalObject.cpp:1339-1357). Sem
    /// `Options::allowDoubleShape` as formas `Double` reusam a de `Contiguous`.
    /// `JSGlobalObject::tryInstallSpeciesWatchpoint<SpeciesWatchpoint>` (JSGlobalObject.cpp:3337) com
    /// `HasSpeciesProperty::Yes` ou `No` (o `No` é o dos TypedArrays: o `@@species` fica no `%TypedArray%`, e o
/// construtor concreto só pode herdá-lo, então a condição é `absence`). Confere que
    /// `prototype.constructor` é o `constructor` e que o `@@species` dele é o acessor primordial, vigia as
    /// duas propriedades para substituição e instala os dois `ObjectPropertyChangeAdaptiveWatchpoint` sobre o
    /// `speciesWatchpointSet`; se qualquer conferência falha, invalida o set.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_install_species_watchpoint<SW: crate::runtime::object_adaptive_structure_watchpoint::SpeciesWatchpoint>(
        &self,
        vm: &VM,
        prototype: &JSObjectHandle,
        constructor: &JSObjectHandle,
        constructor_watchpoint: &RefCell<Option<ObjectPropertyChangeAdaptiveWatchpoint>>,
        species_watchpoint: &RefCell<Option<SW>>,
        species_watchpoint_set: &Rc<RefCell<InlineWatchpointSet>>,
        has_species_property: HasSpeciesProperty,
        species_getter_setter: JSValue,
    ) {
        assert!(constructor_watchpoint.borrow().is_none());
        assert!(species_watchpoint.borrow().is_none());

        // Só inicializamos uma vez, então achatar as estruturas não tem custo real.
        let mut prototype_structure = prototype.structure();
        if prototype_structure.is_dictionary() {
            prototype_structure = Structure::flatten_dictionary_structure(&prototype_structure, vm, prototype);
        }
        assert!(!prototype_structure.is_dictionary());

        let invalidate_watchpoint = || {
            species_watchpoint_set
                .borrow_mut()
                .invalidate(vm, &StringFireDetail::new("Was not able to set up species watchpoint."));
        };

        let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);
        let mut constructor_slot = PropertySlot::new(prototype.as_value(), InternalMethodType::VMInquiry);
        prototype.get_own_property_slot(vm, &constructor_name, &mut constructor_slot);
        if constructor_slot.slot_base() != Some(prototype.cell_id())
            || !constructor_slot.is_cacheable_value()
            || constructor_slot.get_value() != constructor.as_value()
        {
            invalidate_watchpoint();
            return;
        }

        let mut constructor_structure = constructor.structure();
        if constructor_structure.is_dictionary() {
            constructor_structure = Structure::flatten_dictionary_structure(&constructor_structure, vm, constructor);
        }

        let species_name = PropertyName::from_identifier(&vm.property_names.species_symbol);
        let mut species_slot = PropertySlot::new(constructor.as_value(), InternalMethodType::VMInquiry);
        constructor.get_own_property_slot(vm, &species_name, &mut species_slot);
        match has_species_property {
            HasSpeciesProperty::Yes => {
                // `speciesSlot.isCacheableGetter()` e `getterSetter() != speciesGetterSetter`: o `GetterSetter` é a
                // célula guardada no offset do `@@species`.
                if species_slot.slot_base() != Some(constructor.cell_id())
                    || !species_slot.is_cacheable()
                    || !species_slot.is_accessor()
                    || constructor.get_direct(species_slot.cached_offset()) != species_getter_setter
                {
                    invalidate_watchpoint();
                    return;
                }
            }
            HasSpeciesProperty::No => {
                if !species_slot.is_unset() {
                    invalidate_watchpoint();
                    return;
                }
            }
        }

        // Agora, vigiar que essas condições continuam valendo.
        prototype_structure.start_watching_property_for_replacements(vm, constructor_slot.cached_offset());
        if has_species_property == HasSpeciesProperty::Yes {
            constructor_structure.start_watching_property_for_replacements(vm, species_slot.cached_offset());
        }

        let constructor_condition = ObjectPropertyCondition::equivalence(
            prototype.clone(),
            vm.property_names.constructor.impl_().expect("constructor tem uid"),
            constructor.as_value(),
        );
        let species_uid = vm.property_names.species_symbol.impl_().expect("@@species tem uid");
        let species_condition = match has_species_property {
            HasSpeciesProperty::Yes => ObjectPropertyCondition::equivalence(constructor.clone(), species_uid, species_getter_setter),
            HasSpeciesProperty::No => {
                ObjectPropertyCondition::absence(constructor.clone(), species_uid, JSObject::from_value(&constructor.get_prototype_direct()))
            }
        };

        if !constructor_condition.is_watchable(vm, WatchabilityEffort::MakeNoChanges)
            || !species_condition.is_watchable(vm, WatchabilityEffort::MakeNoChanges)
        {
            invalidate_watchpoint();
            return;
        }

        // Só o DFG vigia isto, e só quando o set está em `IsWatched`.
        assert!(!species_watchpoint_set.borrow().is_being_watched());
        species_watchpoint_set.borrow_mut().touch_with_reason(vm, "Set up species watchpoint.");

        let constructor_adaptive = ObjectPropertyChangeAdaptiveWatchpoint::new(constructor_condition, Rc::clone(species_watchpoint_set));
        constructor_adaptive.install(vm);
        *constructor_watchpoint.borrow_mut() = Some(constructor_adaptive);

        let species_adaptive = SW::create(species_condition, Rc::clone(species_watchpoint_set));
        species_adaptive.install(vm);
        *species_watchpoint.borrow_mut() = Some(species_adaptive);
    }

    fn init_array_structures(&self, vm: &VM, array_prototype: JSValue) {
        let create = |indexing_type| JSArray::create_structure(vm, Some(self), array_prototype, indexing_type);
        let contiguous = create(ARRAY_WITH_CONTIGUOUS);
        let copy_on_write_contiguous = create(COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS);
        let allow_double_shape = crate::runtime::options::Options::with(|options| options.allow_double_shape);
        let mut table = self.original_array_structure_for_indexing_shape.borrow_mut();
        let mut set = |indexing_type, structure| table[array_index_from_indexing_type(indexing_type) as usize] = Some(structure);
        set(ARRAY_WITH_UNDECIDED, create(ARRAY_WITH_UNDECIDED));
        set(ARRAY_WITH_INT32, create(ARRAY_WITH_INT32));
        set(
            ARRAY_WITH_DOUBLE,
            if allow_double_shape { create(ARRAY_WITH_DOUBLE) } else { Rc::clone(&contiguous) },
        );
        set(ARRAY_WITH_CONTIGUOUS, contiguous);
        set(ARRAY_WITH_ARRAY_STORAGE, create(ARRAY_WITH_ARRAY_STORAGE));
        set(ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE, create(ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE));
        set(COPY_ON_WRITE_ARRAY_WITH_INT32, create(COPY_ON_WRITE_ARRAY_WITH_INT32));
        set(
            COPY_ON_WRITE_ARRAY_WITH_DOUBLE,
            if allow_double_shape {
                create(COPY_ON_WRITE_ARRAY_WITH_DOUBLE)
            } else {
                Rc::clone(&copy_on_write_contiguous)
            },
        );
        set(COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS, copy_on_write_contiguous);
        *self.array_structure_for_indexing_shape_during_allocation.borrow_mut() = table.clone();
    }

    /// `mapIteratorPrototype()`, `mapIteratorStructure()`, `setIteratorPrototype()` e `setIteratorStructure()`.
    pub fn map_iterator_prototype(&self) -> crate::runtime::js_object::JSObjectRef {
        self.map_iterator_prototype.borrow().clone().expect("JSGlobalObject sem mapIteratorPrototype")
    }

    pub fn map_iterator_structure(&self) -> StructureRef {
        self.map_iterator_structure.borrow().clone().expect("JSGlobalObject sem mapIteratorStructure")
    }

    pub fn set_iterator_prototype(&self) -> crate::runtime::js_object::JSObjectRef {
        self.set_iterator_prototype.borrow().clone().expect("JSGlobalObject sem setIteratorPrototype")
    }

    pub fn set_iterator_structure(&self) -> StructureRef {
        self.set_iterator_structure.borrow().clone().expect("JSGlobalObject sem setIteratorStructure")
    }

    /// `mapStructure()`, `setStructure()`, `weakMapStructure()` e `weakSetStructure()`.
    pub fn map_structure(&self) -> StructureRef {
        self.map_structure.borrow().clone().expect("JSGlobalObject sem mapStructure")
    }

    pub fn set_structure(&self) -> StructureRef {
        self.set_structure.borrow().clone().expect("JSGlobalObject sem setStructure")
    }

    pub fn weak_map_structure(&self) -> StructureRef {
        self.weak_map_structure.borrow().clone().expect("JSGlobalObject sem weakMapStructure")
    }

    pub fn weak_set_structure(&self) -> StructureRef {
        self.weak_set_structure.borrow().clone().expect("JSGlobalObject sem weakSetStructure")
    }

    /// `mapConstructor()` e `setConstructor()`.
    pub fn map_constructor(&self) -> crate::runtime::js_function::JSFunctionRef {
        self.map_constructor.borrow().clone().expect("JSGlobalObject sem mapConstructor")
    }

    pub fn set_constructor(&self) -> crate::runtime::js_function::JSFunctionRef {
        self.set_constructor.borrow().clone().expect("JSGlobalObject sem setConstructor")
    }

    /// `mapProtoEntriesFunction()`: o `LazyProperty` cria a função na primeira leitura.
    pub fn map_proto_entries_function(&self) -> crate::runtime::js_function::JSFunctionRef {
        if let Some(function) = self.map_proto_entries_function.borrow().as_ref() {
            return function.clone();
        }
        let function = crate::runtime::map_prototype::create_map_proto_entries_function(self.vm(), self);
        *self.map_proto_entries_function.borrow_mut() = Some(function.clone());
        function
    }

    /// `setProtoValuesFunction()`: o `LazyProperty` cria a função na primeira leitura.
    pub fn set_proto_values_function(&self) -> crate::runtime::js_function::JSFunctionRef {
        if let Some(function) = self.set_proto_values_function.borrow().as_ref() {
            return function.clone();
        }
        let function = crate::runtime::set_prototype::create_set_proto_values_function(self.vm(), self);
        *self.set_proto_values_function.borrow_mut() = Some(function.clone());
        function
    }

    /// Os iteradores de `Map` e `Set` (JSGlobalObject.cpp:1449-1457), `Reflect`, `JSON` e os quatro
    /// `ClassStructure` `Map`, `Set`, `WeakMap` e `WeakSet` (protótipo, estrutura das instâncias e
    /// construtor, com o `constructor` do protótipo ligado por `setConstructor`), cada um com a
    /// propriedade global `DontEnum`. `Map` e `Set` também são os `LinkTimeConstant::Map` e `Set`, e o
    /// `Map` ganha o `groupBy` (JS embutido, `builtins/MapConstructor.js`).
    ///
    /// DIVERGÊNCIA: o C++ cria `Reflect`, `JSON` e as classes na primeira leitura da propriedade
    /// (`PropertyCallback`, `ClassStructure`); aqui nascem junto do global.
    pub(crate) fn install_json_reflect_and_collections(
        &self,
        vm: &VM,
        object_prototype: &crate::runtime::js_object::JSObjectRef,
        function_prototype: &InternalFunctionRef,
        iterator_prototype: &crate::runtime::js_object::JSObjectRef,
    ) {
        use crate::runtime::builtins_source::BuiltinCodeIndex;
        use crate::runtime::js_function::put_direct_builtin_function_without_transition;
        use crate::runtime::js_map::{JSMap, JSMapIterator};
        use crate::runtime::js_set::{JSSet, JSSetIterator};
        use crate::runtime::js_weak_map::JSWeakMap;
        use crate::runtime::js_weak_set::JSWeakSet;
        use crate::runtime::map_constructor::MapConstructor;
        use crate::runtime::map_iterator_prototype::MapIteratorPrototype;
        use crate::runtime::map_prototype::MapPrototype;
        use crate::runtime::reflect_object::ReflectObject;
        use crate::runtime::set_constructor::SetConstructor;
        use crate::runtime::set_iterator_prototype::SetIteratorPrototype;
        use crate::runtime::set_prototype::SetPrototype;
        use crate::runtime::weak_map_constructor::WeakMapConstructor;
        use crate::runtime::weak_map_prototype::WeakMapPrototype;
        use crate::runtime::weak_set_constructor::WeakSetConstructor;
        use crate::runtime::weak_set_prototype::WeakSetPrototype;

        let object_prototype_value = object_prototype.as_value();
        let function_prototype_value = function_prototype.as_value();
        let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);
        let put_global = |name: &[u8], value: JSValue| {
            self.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, name)), value, DONT_ENUM);
        };

        // m_mapIteratorPrototype, m_mapIteratorStructure, m_setIteratorPrototype e m_setIteratorStructure.
        let map_iterator_prototype_structure = MapIteratorPrototype::create_structure(vm, self, iterator_prototype.as_value());
        let map_iterator_prototype = MapIteratorPrototype::create(vm, self, &map_iterator_prototype_structure);
        let map_iterator_structure = JSMapIterator::create_structure(vm, Some(self), map_iterator_prototype.as_value());
        let next_name = PropertyName::from_identifier(&vm.property_names.next);
        self.iteration_protocol.borrow_mut().map_iterator_next = Some(map_iterator_prototype.get_direct_by_name(vm, &next_name));
        *self.map_iterator_prototype.borrow_mut() = Some(map_iterator_prototype);
        *self.map_iterator_structure.borrow_mut() = Some(map_iterator_structure);

        let set_iterator_prototype_structure = SetIteratorPrototype::create_structure(vm, self, iterator_prototype.as_value());
        let set_iterator_prototype = SetIteratorPrototype::create(vm, self, &set_iterator_prototype_structure);
        let set_iterator_structure = JSSetIterator::create_structure(vm, Some(self), set_iterator_prototype.as_value());
        self.iteration_protocol.borrow_mut().set_iterator_next = Some(set_iterator_prototype.get_direct_by_name(vm, &next_name));
        *self.set_iterator_prototype.borrow_mut() = Some(set_iterator_prototype);
        *self.set_iterator_structure.borrow_mut() = Some(set_iterator_structure);

        // `Reflect` (`createReflectProperty`) e `JSON` (`createJSONProperty`).
        let reflect_structure = ReflectObject::create_structure(vm, self, object_prototype_value);
        let reflect_object = ReflectObject::create(vm, self, &reflect_structure);
        self.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.reflect), reflect_object.as_value(), DONT_ENUM);
        put_global(b"JSON", crate::runtime::json_object_native::create_json_object(self).as_value());

        // `Map`.
        let map_prototype_structure = MapPrototype::create_structure(vm, self, object_prototype_value);
        let map_prototype = MapPrototype::create(vm, self, &map_prototype_structure);
        map_prototype.did_become_prototype(vm);
        *self.map_structure.borrow_mut() = Some(JSMap::create_structure(vm, Some(self), map_prototype.as_value()));
        let map_constructor_structure = MapConstructor::create_structure(vm, self, function_prototype_value);
        let map_constructor = MapConstructor::create(vm, self, map_constructor_structure, &map_prototype);
        map_prototype.put_direct(vm, &constructor_name, map_constructor.as_value(), DONT_ENUM);
        put_direct_builtin_function_without_transition(
            vm,
            self,
            &map_constructor,
            vm.property_names.builtin_names().group_by_public_name(),
            BuiltinCodeIndex::MapConstructorGroupByCode,
            DONT_ENUM,
        );
        self.set_link_time_constant(LinkTimeConstant::Map, map_constructor.as_value());
        put_global(b"Map", map_constructor.as_value());
        *self.map_constructor.borrow_mut() = Some(map_constructor);

        // `Set`.
        let set_prototype_structure = SetPrototype::create_structure(vm, self, object_prototype_value);
        let set_prototype = SetPrototype::create(vm, self, &set_prototype_structure);
        set_prototype.did_become_prototype(vm);
        *self.set_structure.borrow_mut() = Some(JSSet::create_structure(vm, Some(self), set_prototype.as_value()));
        let set_constructor_structure = SetConstructor::create_structure(vm, self, function_prototype_value);
        let set_constructor = SetConstructor::create(vm, self, set_constructor_structure, &set_prototype);
        set_prototype.put_direct(vm, &constructor_name, set_constructor.as_value(), DONT_ENUM);
        self.set_link_time_constant(LinkTimeConstant::Set, set_constructor.as_value());
        put_global(b"Set", set_constructor.as_value());
        *self.set_constructor.borrow_mut() = Some(set_constructor);

        // `WeakMap`.
        let weak_map_prototype_structure = WeakMapPrototype::create_structure(vm, self, object_prototype_value);
        let weak_map_prototype = WeakMapPrototype::create(vm, self, &weak_map_prototype_structure);
        weak_map_prototype.did_become_prototype(vm);
        *self.weak_map_structure.borrow_mut() = Some(JSWeakMap::create_structure(vm, Some(self), weak_map_prototype.as_value()));
        let weak_map_constructor_structure = WeakMapConstructor::create_structure(vm, self, function_prototype_value);
        let weak_map_constructor = WeakMapConstructor::create(vm, self, weak_map_constructor_structure, &weak_map_prototype);
        weak_map_prototype.put_direct(vm, &constructor_name, weak_map_constructor.as_value(), DONT_ENUM);
        put_global(b"WeakMap", weak_map_constructor.as_value());

        // `WeakSet`.
        let weak_set_prototype_structure = WeakSetPrototype::create_structure(vm, self, object_prototype_value);
        let weak_set_prototype = WeakSetPrototype::create(vm, self, &weak_set_prototype_structure);
        weak_set_prototype.did_become_prototype(vm);
        *self.weak_set_structure.borrow_mut() = Some(JSWeakSet::create_structure(vm, Some(self), weak_set_prototype.as_value()));
        let weak_set_constructor_structure = WeakSetConstructor::create_structure(vm, self, function_prototype_value);
        let weak_set_constructor = WeakSetConstructor::create(vm, self, weak_set_constructor_structure, &weak_set_prototype);
        weak_set_prototype.put_direct(vm, &constructor_name, weak_set_constructor.as_value(), DONT_ENUM);
        put_global(b"WeakSet", weak_set_constructor.as_value());
    }
}
