//! Porte mínimo de `runtime/JSGlobalObject.h`, `JSGlobalObject.cpp` e `JSGlobalObjectInlines.h`: o que
//! o interpretador consulta (`vm`, `globalThis`, `globalScope`, `globalLexicalEnvironment`,
//! `globalScopeExtension`, `globalCallee`, `evalCallee`, as `Structure` de callee, de função e de
//! ativação) e a declaração de `var`/função global (`addSymbolTableEntry`, `canDeclareGlobalVar`,
//! `createGlobalVarBinding`, `canDeclareGlobalFunction`, `createGlobalFunctionBinding`).
//!
//! Fora desta fatia, e por quê: os ~80 construtores, protótipos e `LazyProperty` de `init(vm)`,
//! `initStaticGlobals`/`addStaticGlobals` (`NaN`, `Infinity`, `undefined`), `m_linkTimeConstants`,
//! `m_structureCache`, `m_symbolTableCache`, os conjuntos de watchpoint, `m_globalObjectMethodTable`,
//! `m_microtaskQueue`, o `Debugger`, `getOwnPropertySlot`/`put`/`defineOwnProperty` (escrevem em
//! `PropertySlot` e lançam por `ThrowScope`; a lógica de tabela é `symbol_table_get`/`symbol_table_put`)
//! e o estado de `visitChildren`/`destroy`. Entram com os módulos de que dependem.
//!
//! DIVERGÊNCIAS: `VM* const m_vm` é `Rc<VM>` (o `VM` do porte é um valor, não há ponteiro estável de
//! outro modo). O global object é um `JSScope`, então `m_globalCallee` e os demais callees o têm como
//! escopo (`JSScopeRef::GlobalObject`). `finish_creation` recebe o `FunctionPrototype` já criado
//! (o C++ o cria dentro de `init`, junto de todo o resto, e o `FunctionPrototype` ainda não existe).
//! `activationStructure()` (`m_lexicalEnvironmentStructure`, um `LazyProperty`) é criada junto.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::bytecode_intrinsics_table::{LinkTimeConstant, LINK_TIME_CONSTANT_TABLE};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_module_environment::JSModuleEnvironment;
use crate::runtime::symbol_table::{SymbolTableRef, SymbolTableRefExt};
use crate::runtime::identifier::Identifier;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::js_callee::JSCallee;
use crate::runtime::js_async_function::JSAsyncFunction;
use crate::runtime::js_async_generator_function::JSAsyncGeneratorFunction;
use crate::runtime::js_generator_function::JSGeneratorFunction;
use crate::runtime::js_function::{JSArrowFunction, JSFunction, JSSloppyFunction, JSStrictFunction};
use crate::runtime::js_remote_function::JSRemoteFunction;
use crate::runtime::js_global_lexical_environment::{JSGlobalLexicalEnvironment, JSGlobalLexicalEnvironmentRef};
use crate::runtime::js_lexical_environment::JSLexicalEnvironment;
use crate::runtime::js_with_scope::JSWithScope;
use crate::runtime::reg_exp_object::RegExpObject;
use crate::runtime::js_object::{JSObject, JSObjectHandle, JSObjectRef};
use crate::runtime::js_global_proxy::JSGlobalProxy;
use crate::runtime::indexing_type::{
    array_index_from_indexing_type, IndexingType, ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE, ARRAY_WITH_UNDECIDED, IS_ARRAY,
    NUMBER_OF_ARRAY_INDEXING_MODES,
};
use crate::runtime::internal_function::InternalFunctionRef;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_segmented_variable_object::{JSSegmentedVariableObject, JS_SEGMENTED_VARIABLE_OBJECT_S_INFO};
use crate::runtime::error_messages::READONLY_PROPERTY_WRITE_ERROR;
use crate::runtime::js_global_proxy;
use crate::runtime::js_object::PutError;
use crate::runtime::js_symbol_table_object::{symbol_table_get, symbol_table_put, SymbolTablePut};
use crate::runtime::put_property_slot::PutPropertySlot;
use crate::runtime::js_value::{js_null, js_undefined, JSValue};
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY, ACCESSOR, CUSTOM_ACCESSOR};
use crate::runtime::scope_offset::ScopeOffset;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::symbol_table::SymbolTableEntry;
use crate::runtime::js_type_info::{
    TypeInfo, HAS_STATIC_PROPERTY_TABLE, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_PUT,
};
use crate::runtime::js_type::JSType;
use crate::runtime::var_offset::VarOffset;
use crate::runtime::vm::VM;

/// `const ClassInfo JSGlobalObject::s_info` (`DEFINE_VISIT_CHILDREN`/`CREATE_METHOD_TABLE` ficam de fora).
pub static JS_GLOBAL_OBJECT_S_INFO: ClassInfo = ClassInfo {
    class_name: "JSGlobalObject",
    parent_class: Some(&JS_SEGMENTED_VARIABLE_OBJECT_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

/// `enum class BindingCreationContext : bool { Global, Eval }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingCreationContext {
    Global,
    Eval,
}

/// `JSGlobalObject::FunctionStructures`.
#[derive(Clone, Debug, Default)]
struct FunctionStructures {
    arrow_function_structure: Option<StructureRef>,
    sloppy_function_structure: Option<StructureRef>,
    sloppy_method_structure: Option<StructureRef>,
    strict_function_structure: Option<StructureRef>,
    strict_method_structure: Option<StructureRef>,
}

/// Os `WriteBarrierStructureID` e `LazyProperty<..., Structure>` que o interpretador usa.
#[derive(Clone, Debug, Default)]
struct GlobalObjectStructures {
    callee_structure: Option<StructureRef>,
    error_structure: Option<StructureRef>,
    host_function_structure: Option<StructureRef>,
    remote_function_structure: Option<StructureRef>,
    builtin_functions: FunctionStructures,
    ordinary_functions: FunctionStructures,
    lexical_environment_structure: Option<StructureRef>,
    module_environment_structure: Option<StructureRef>,
    module_namespace_object_structure: Option<StructureRef>,
    with_scope_structure: Option<StructureRef>,
    reg_exp_structure: Option<StructureRef>,
    generator_function_structure: Option<StructureRef>,
    async_function_structure: Option<StructureRef>,
    async_generator_function_structure: Option<StructureRef>,
}

/// `class JSGlobalObject : public JSSegmentedVariableObject`.
#[derive(Debug)]
pub struct JSGlobalObject {
    base: JSSegmentedVariableObject,
    /// `m_vm`.
    vm: Rc<VM>,
    /// `m_globalThis`: o `JSGlobalProxy` (um `JSObject*` qualquer, como no C++).
    global_this: RefCell<Option<JSObjectHandle>>,
    /// `m_globalLexicalEnvironment`.
    global_lexical_environment: RefCell<Option<JSGlobalLexicalEnvironmentRef>>,
    /// `m_globalScopeExtension`.
    global_scope_extension: RefCell<Option<JSScopeRef>>,
    /// `m_globalCallee`, `m_zombieFrameCallee` e `m_evalCallee`.
    global_callee: RefCell<Option<Rc<JSCallee>>>,
    zombie_frame_callee: RefCell<Option<Rc<JSCallee>>>,
    eval_callee: RefCell<Option<Rc<JSCallee>>>,
    /// `m_evalFunction`: o `cell_id` do `JSFunction` de `eval`, que o `eval` direto compara com o callee.
    eval_function: Cell<Option<usize>>,
    structures: RefCell<GlobalObjectStructures>,
    /// `m_objectPrototype`.
    object_prototype: RefCell<Option<JSObjectRef>>,
    /// `m_globalLexicalBindingEpoch { 1 }`.
    global_lexical_binding_epoch: Cell<u32>,
    /// `m_havingABadTimeWatchpointSet->hasBeenInvalidated()` (ver `js_global_object_bad_time.rs`).
    pub(crate) having_a_bad_time: Cell<bool>,
    /// `m_linkTimeConstants`, indexado por `LinkTimeConstant` (o `LazyProperty<JSGlobalObject, JSCell>`
    /// já inicializado; `JSValue::Empty` é o `LazyProperty` ainda sem valor).
    link_time_constants: RefCell<Vec<JSValue>>,
    /// `m_symbolTableCache`, chaveado pelo `cell_id` do `SymbolTable*` original.
    symbol_table_cache: RefCell<HashMap<usize, SymbolTableRef>>,
    /// `m_structureCache`.
    structure_cache: crate::runtime::structure_cache::StructureCache,
    /// `m_regExpGlobalData`.
    reg_exp_global_data: crate::runtime::reg_exp_global_data::RegExpGlobalData,
    /// `m_functionPrototype`, `m_objectStructureForObjectConstructor` e a estrutura de `Array`
    /// (preenchidos por `init`, ver `js_global_object_init.rs`).
    pub(crate) function_prototype: RefCell<Option<InternalFunctionRef>>,
    pub(crate) object_structure_for_object_constructor: RefCell<Option<StructureRef>>,
    /// `m_originalArrayStructureForIndexingShape` e `m_arrayStructureForIndexingShapeDuringAllocation`,
    /// indexados por `arrayIndexFromIndexingType` (`NUMBER_OF_ARRAY_INDEXING_MODES` posições; a de índice 0,
    /// `NoIndexingShape`, nunca é usada). Preenchidos por `init` (`init_array_structures`).
    pub(crate) original_array_structure_for_indexing_shape: RefCell<Vec<Option<StructureRef>>>,
    pub(crate) array_structure_for_indexing_shape_during_allocation: RefCell<Vec<Option<StructureRef>>>,
    /// `m_stringObjectStructure` e o `StringPrototype` (preenchidos por `init`).
    pub(crate) string_object_structure: RefCell<Option<StructureRef>>,
    pub(crate) string_prototype: RefCell<Option<crate::runtime::string_object::StringObjectRef>>,
    /// `m_symbolObjectStructure` (preenchido por `init`).
    pub(crate) symbol_object_structure: RefCell<Option<StructureRef>>,
    /// `m_proxyObjectStructure`, `m_callableProxyObjectStructure` e `m_proxyRevokeStructure` (preenchidos por `init`).
    pub(crate) proxy_object_structure: RefCell<Option<StructureRef>>,
    pub(crate) callable_proxy_object_structure: RefCell<Option<StructureRef>>,
    pub(crate) proxy_revoke_structure: RefCell<Option<StructureRef>>,
    /// `m_numberObjectStructure` e `m_booleanObjectStructure` (preenchidos por `init`).
    pub(crate) number_object_structure: RefCell<Option<StructureRef>>,
    pub(crate) boolean_object_structure: RefCell<Option<StructureRef>>,
    /// `m_bigIntObjectStructure` (preenchido por `init`).
    pub(crate) big_int_object_structure: RefCell<Option<StructureRef>>,
    /// `m_dateStructure` (preenchido por `install_date`).
    pub(crate) date_structure: RefCell<Option<StructureRef>>,
    pub(crate) array_buffer_realm: crate::runtime::js_array_buffer::ArrayBufferRealm,
    /// `m_weakRandom` (`Math.random`).
    pub(crate) weak_random: RefCell<crate::wtf::weak_random::WeakRandom>,
    /// `m_parseIntFunction` e `m_parseFloatFunction` (os `LazyProperty` de `JSFunction`, criados por `init`),
    /// como o `JSValue` da função (o `JSFunction` não tem `Debug`).
    pub(crate) parse_int_function: RefCell<Option<JSValue>>,
    pub(crate) parse_float_function: RefCell<Option<JSValue>>,
    /// `m_objectProtoToStringFunction` (o `LazyProperty` de `JSFunction`, criado por `ObjectPrototype::create`),
    /// como o `JSValue` da função.
    pub(crate) object_proto_to_string_function: RefCell<Option<JSValue>>,
    /// `m_regExpPrototype` (preenchido por `init`).
    pub(crate) reg_exp_prototype: RefCell<Option<crate::runtime::js_object::JSObjectRef>>,
    /// `m_regExpPrimordialPropertiesWatchpointSet` invalidado (`true` = disparou): `RegExp.prototype` teve
    /// `exec`, `flags`, as flags ou um `Symbol.*` escrito, redefinido ou apagado (`JSObject::watch_property_replacement`).
    pub(crate) reg_exp_primordial_properties_fired: std::rc::Rc<std::cell::Cell<bool>>,
    /// `m_arraySpeciesWatchpointSet`: nasce `ClearWatchpoint`; `tryInstallSpeciesWatchpoint` o põe `IsWatched`
    /// (ou o invalida) e os dois watchpoints abaixo o disparam.
    pub(crate) array_species_watchpoint_set: Rc<RefCell<crate::bytecode::watchpoint::InlineWatchpointSet>>,
    /// `m_arrayPrototypeConstructorWatchpoint`.
    pub(crate) array_prototype_constructor_watchpoint:
        RefCell<Option<crate::runtime::object_property_change_adaptive_watchpoint::ObjectPropertyChangeAdaptiveWatchpoint>>,
    /// `m_arrayConstructorSpeciesWatchpoint`.
    pub(crate) array_constructor_species_watchpoint:
        RefCell<Option<crate::runtime::object_property_change_adaptive_watchpoint::ObjectPropertyChangeAdaptiveWatchpoint>>,
    /// `m_promiseSpeciesWatchpointSet`, `m_promisePrototypeConstructorWatchpoint` e
    /// `m_promiseConstructorSpeciesWatchpoint` (JSGlobalObject.cpp:2394).
    pub(crate) promise_species_watchpoint_set: Rc<RefCell<crate::bytecode::watchpoint::InlineWatchpointSet>>,
    pub(crate) promise_prototype_constructor_watchpoint:
        RefCell<Option<crate::runtime::object_property_change_adaptive_watchpoint::ObjectPropertyChangeAdaptiveWatchpoint>>,
    pub(crate) promise_constructor_species_watchpoint:
        RefCell<Option<crate::runtime::object_property_change_adaptive_watchpoint::ObjectPropertyChangeAdaptiveWatchpoint>>,
    /// `m_regExpSpeciesWatchpointSet`, `m_regExpPrototypeConstructorWatchpoint` e
    /// `m_regExpConstructorSpeciesWatchpoint` (JSGlobalObject.cpp:2407).
    pub(crate) reg_exp_species_watchpoint_set: Rc<RefCell<crate::bytecode::watchpoint::InlineWatchpointSet>>,
    pub(crate) reg_exp_prototype_constructor_watchpoint:
        RefCell<Option<crate::runtime::object_property_change_adaptive_watchpoint::ObjectPropertyChangeAdaptiveWatchpoint>>,
    pub(crate) reg_exp_constructor_species_watchpoint:
        RefCell<Option<crate::runtime::object_property_change_adaptive_watchpoint::ObjectPropertyChangeAdaptiveWatchpoint>>,
    /// As estruturas de `ErrorInstance` por tipo (`m_errorStructure` e os `ClassStructure` dos erros nativos).
    pub(crate) error_structures: RefCell<Vec<(crate::runtime::error_type::ErrorType, StructureRef)>>,
    /// As estruturas de `ErrorInstance` de `WebAssembly.CompileError`/`LinkError`/`RuntimeError`
    /// (`JSWebAssemblyCompileError::createStructure` etc.), na ordem de `WasmErrorKind`.
    pub(crate) wasm_error_structures: RefCell<Vec<(crate::runtime::wasm_errors::WasmErrorKind, StructureRef)>>,
    /// O construtor `Error` (o `ErrorConstructor::put`/`deleteProperty` o reconhecem por ele).
    pub(crate) error_constructor: RefCell<Option<JSValue>>,
    /// `std::optional<unsigned> m_stackTraceLimit`: o espelho de `Error.stackTraceLimit` que
    /// `ErrorConstructor::put`/`deleteProperty` mantêm (`None` é o `std::nullopt`); a captura de pilha o lê.
    pub(crate) stack_trace_limit: Cell<Option<u32>>,
    /// `WeakPtr<ConsoleClient> m_consoleClient` (`setConsoleClient`/`consoleClient` em `console_object.rs`).
    pub(crate) console_client: RefCell<Option<std::rc::Weak<dyn crate::runtime::console_object::ConsoleClient>>>,
    /// O console do processo hospedeiro (`console_host.rs`); `None` descarta a saída e deixa o stdin em EOF.
    pub(crate) console_host: RefCell<Option<std::rc::Rc<dyn crate::runtime::console_host::ConsoleHost>>>,
    /// O hospedeiro do DOM deste global (`dom_bridge.rs`): cada frame tem o seu documento.
    pub(crate) self_ref: RefCell<std::rc::Weak<JSGlobalObject>>,
    pub(crate) dom_host: RefCell<Option<std::rc::Rc<dyn crate::runtime::dom_bridge::DomHost>>>,
    /// O sistema de arquivos do processo hospedeiro (`module_fs.rs`); `None`: o `fetch('file://...')` não acha arquivo.
    pub(crate) module_fs: RefCell<Option<std::rc::Rc<dyn crate::runtime::module_fs::ModuleFs>>>,
    /// A `Structure` dos `CallSite` que `Error.prepareStackTrace` recebe (a do `Bun`, no `ZigGlobalObject`;
    /// preenchida por `init_error_classes`).
    pub(crate) call_site_structure: RefCell<Option<StructureRef>>,
    /// `m_generatorPrototype` e `m_asyncGeneratorPrototype` (ainda não preenchidos pelo `init`: dependem do
    /// `m_iteratorPrototype` e do `m_asyncIteratorPrototype`).
    pub(crate) generator_prototype: RefCell<Option<JSObjectRef>>,
    pub(crate) async_generator_prototype: RefCell<Option<JSObjectRef>>,
    /// `m_iteratorPrototype`, `m_arrayIteratorPrototype` e `m_arrayIteratorStructure` (preenchidos por `init`).
    pub(crate) iterator_prototype: RefCell<Option<JSObjectRef>>,
    pub(crate) array_iterator_prototype: RefCell<Option<JSObjectRef>>,
    pub(crate) array_iterator_structure: RefCell<Option<StructureRef>>,
    /// O valor original de `%ArrayIteratorPrototype%.next` (o que o `m_arrayIteratorProtocolWatchpointSet`
    /// vigia no C++), gravado por `init` logo depois de criar o protótipo.
    pub(crate) array_iterator_proto_next: RefCell<Option<JSValue>>,
    /// Os originais do `getIterationMode` de `Map`, `Set` e `String` (ver `iteration_protocol.rs`).
    pub(crate) iteration_protocol: RefCell<crate::runtime::iteration_protocol::IterationProtocolOriginals>,
    /// `m_mapIteratorPrototype`, `m_mapIteratorStructure`, `m_setIteratorPrototype` e `m_setIteratorStructure`
    /// (preenchidos por `init`).
    pub(crate) map_iterator_prototype: RefCell<Option<JSObjectRef>>,
    pub(crate) map_iterator_structure: RefCell<Option<StructureRef>>,
    pub(crate) set_iterator_prototype: RefCell<Option<JSObjectRef>>,
    pub(crate) set_iterator_structure: RefCell<Option<StructureRef>>,
    /// `m_segmentsStructure` e `m_segmentIteratorStructure` (`LazyProperty` do `Intl.Segmenter`; o porte os
    /// preenche em `install_segmenter`). Cada uma tem o protótipo (`%Segments%`, `%SegmentIteratorPrototype%`) como
    /// protótipo das instâncias.
    pub(crate) segments_structure: RefCell<Option<StructureRef>>,
    pub(crate) segment_iterator_structure: RefCell<Option<StructureRef>>,
    /// `m_mapStructure`, `m_setStructure`, `m_weakMapStructure` e `m_weakSetStructure` (a estrutura das
    /// instâncias de cada `ClassStructure`) e o construtor de `Map` e de `Set` (`mapConstructor()` e
    /// `setConstructor()`, preenchidos por `init`).
    pub(crate) map_structure: RefCell<Option<StructureRef>>,
    pub(crate) set_structure: RefCell<Option<StructureRef>>,
    pub(crate) weak_map_structure: RefCell<Option<StructureRef>>,
    pub(crate) weak_set_structure: RefCell<Option<StructureRef>>,
    pub(crate) map_constructor: RefCell<Option<crate::runtime::js_function::JSFunctionRef>>,
    pub(crate) set_constructor: RefCell<Option<crate::runtime::js_function::JSFunctionRef>>,
    /// `m_mapProtoEntriesFunction` e `m_setProtoValuesFunction` (`LazyProperty<JSGlobalObject, JSFunction>`).
    pub(crate) map_proto_entries_function: RefCell<Option<crate::runtime::js_function::JSFunctionRef>>,
    pub(crate) set_proto_values_function: RefCell<Option<crate::runtime::js_function::JSFunctionRef>>,
    /// `m_regExpConstructor`, `m_stringIteratorStructure` e `m_regExpStringIteratorStructure` (ver
    /// `string_regexp_globals.rs`).
    pub(crate) string_regexp_globals: RefCell<crate::runtime::string_regexp_globals::StringRegExpGlobals>,
    /// `m_arrayProtoValuesFunction` (`LazyProperty<JSGlobalObject, JSFunction>`, criada na primeira leitura).
    pub(crate) array_proto_values_function: RefCell<Option<crate::runtime::js_function::JSFunctionRef>>,
    /// `m_throwTypeErrorArgumentsCalleeGetterSetter` (`LazyProperty<JSGlobalObject, GetterSetter>`).
    pub(crate) throw_type_error_arguments_callee_getter_setter:
        RefCell<Option<crate::runtime::js_getter_setter::GetterSetterRef>>,
    /// `m_debugger`: nulo até um `Debugger` ser anexado.
    debugger: RefCell<Option<Rc<crate::debugger::debugger::Debugger>>>,
    /// `m_promiseStructure`, `m_promisePrototype`, `m_promiseConstructor` e companhia (ver `promise_constructor.rs`).
    pub(crate) promise_data: RefCell<crate::runtime::promise_constructor::PromiseGlobalData>,
    /// `m_executableForCachedFunctionExecutableForFunctionConstructor` (`Weak<FunctionExecutable>`).
    executable_for_cached_function_executable_for_function_constructor:
        RefCell<Option<std::rc::Weak<RefCell<crate::runtime::function_executable::FunctionExecutable>>>>,
    /// `m_functionConstructor`, `m_asyncIteratorPrototype`, `m_generatorFunctionPrototype` e companhia (ver
    /// `function_kind_intrinsics.rs`).
    pub(crate) function_kind_data: RefCell<crate::runtime::function_kind_intrinsics::FunctionKindGlobalData>,
    /// `m_iteratorConstructor`, `m_iteratorStructure`, `m_iteratorHelperPrototype` e companhia (ver
    /// `iterator_constructor.rs`).
    pub(crate) iterator_data: RefCell<crate::runtime::iterator_constructor::IteratorGlobalData>,
    /// `m_durationStructure` e as irmãs de `Temporal` (ver `temporal_object.rs`).
    pub(crate) temporal_data: RefCell<crate::runtime::temporal_object::TemporalGlobalData>,
}

/// Referência compartilhada, o `JSGlobalObject*` do C++.
pub type JSGlobalObjectRef = Rc<JSGlobalObject>;

impl std::ops::Deref for JSGlobalObject {
    type Target = JSSegmentedVariableObject;

    fn deref(&self) -> &JSSegmentedVariableObject {
        &self.base
    }
}

impl JSGlobalObject {
    /// `JSGlobalObject::visitChildren`: as variáveis globais e os `JSValue` que o global guarda por id (os
    /// guardados por `Rc` o coletor já trata como raiz; os protótipos das estruturas, `gc::retain_prototype`).
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        self.visit_variables(marker);
        match self.link_time_constants.try_borrow() {
            Ok(constants) => marker.values(&constants),
            Err(_) => marker.abort(),
        }
        if let Some(eval_function) = self.eval_function.get() {
            marker.cell(eval_function);
        }
        for field in [
            &self.parse_int_function,
            &self.parse_float_function,
            &self.object_proto_to_string_function,
            &self.error_constructor,
            &self.array_iterator_proto_next,
        ] {
            match field.try_borrow() {
                Ok(value) => marker.value(*value),
                Err(_) => marker.abort(),
            }
        }
        let (Ok(regexp_globals), Ok(promise_data), Ok(function_kind_data), Ok(iteration_protocol)) = (
            self.string_regexp_globals.try_borrow(),
            self.promise_data.try_borrow(),
            self.function_kind_data.try_borrow(),
            self.iteration_protocol.try_borrow(),
        ) else {
            marker.abort();
            return;
        };
        for value in [
            regexp_globals.reg_exp_constructor,
            promise_data.prototype,
            promise_data.constructor,
            promise_data.then_function,
            promise_data.resolve_function,
            promise_data.species_getter_setter,
            promise_data.unhandled_rejection_callback,
            function_kind_data.async_from_sync_iterator_proto_next_function,
            iteration_protocol.map_iterator_next,
            iteration_protocol.set_iterator_next,
            iteration_protocol.string_iterator_next,
            iteration_protocol.iterator_proto_symbol_iterator_function,
            iteration_protocol.string_proto_symbol_iterator_function,
        ] {
            marker.value(value);
        }
    }

    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable | OverridesGetOwnPropertySlot | OverridesPut`.
    /// O JSC puro também liga `IsImmutablePrototypeExoticObject`, mas o oráculo (o global do bun) não liga:
    /// `Object.setPrototypeOf(globalThis, Function.prototype)` funciona nele, então o flag fica de fora.
    pub const STRUCTURE_FLAGS: u32 =
        JSSegmentedVariableObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE | OVERRIDES_GET_OWN_PROPERTY_SLOT | OVERRIDES_PUT;

    /// `JSGlobalObject(VM&, Structure*, const GlobalObjectMethodTable*)`: `Base(vm, structure, nullptr)`.
    fn new(vm: &Rc<VM>, structure: StructureRef) -> JSGlobalObject {
        JSGlobalObject {
            base: JSSegmentedVariableObject::new(vm, structure, None),
            vm: Rc::clone(vm),
            global_this: RefCell::new(None),
            global_lexical_environment: RefCell::new(None),
            global_scope_extension: RefCell::new(None),
            global_callee: RefCell::new(None),
            zombie_frame_callee: RefCell::new(None),
            eval_callee: RefCell::new(None),
            eval_function: Cell::new(None),
            structures: RefCell::new(GlobalObjectStructures::default()),
            object_prototype: RefCell::new(None),
            global_lexical_binding_epoch: Cell::new(1),
            having_a_bad_time: Cell::new(false),
            link_time_constants: RefCell::new(vec![JSValue::empty(); LINK_TIME_CONSTANT_TABLE.len()]),
            symbol_table_cache: RefCell::new(HashMap::new()),
            structure_cache: crate::runtime::structure_cache::StructureCache::default(),
            reg_exp_global_data: crate::runtime::reg_exp_global_data::RegExpGlobalData::new(),
            function_prototype: RefCell::new(None),
            object_structure_for_object_constructor: RefCell::new(None),
            original_array_structure_for_indexing_shape: RefCell::new(vec![None; NUMBER_OF_ARRAY_INDEXING_MODES as usize]),
            array_structure_for_indexing_shape_during_allocation: RefCell::new(vec![
                None;
                NUMBER_OF_ARRAY_INDEXING_MODES as usize
            ]),
            string_object_structure: RefCell::new(None),
            string_prototype: RefCell::new(None),
            symbol_object_structure: RefCell::new(None),
            proxy_object_structure: RefCell::new(None),
            callable_proxy_object_structure: RefCell::new(None),
            proxy_revoke_structure: RefCell::new(None),
            number_object_structure: RefCell::new(None),
            boolean_object_structure: RefCell::new(None),
            big_int_object_structure: RefCell::new(None),
            date_structure: RefCell::new(None),
            array_buffer_realm: Default::default(),
            weak_random: RefCell::new(crate::wtf::weak_random::WeakRandom::default()),
            parse_int_function: RefCell::new(None),
            parse_float_function: RefCell::new(None),
            object_proto_to_string_function: RefCell::new(None),
            reg_exp_prototype: RefCell::new(None),
            reg_exp_primordial_properties_fired: std::rc::Rc::new(std::cell::Cell::new(false)),
            array_species_watchpoint_set: Rc::new(RefCell::new(crate::bytecode::watchpoint::InlineWatchpointSet::new(
                crate::bytecode::watchpoint::WatchpointState::ClearWatchpoint,
            ))),
            array_prototype_constructor_watchpoint: RefCell::new(None),
            array_constructor_species_watchpoint: RefCell::new(None),
            promise_species_watchpoint_set: Rc::new(RefCell::new(crate::bytecode::watchpoint::InlineWatchpointSet::new(
                crate::bytecode::watchpoint::WatchpointState::ClearWatchpoint,
            ))),
            promise_prototype_constructor_watchpoint: RefCell::new(None),
            promise_constructor_species_watchpoint: RefCell::new(None),
            reg_exp_species_watchpoint_set: Rc::new(RefCell::new(crate::bytecode::watchpoint::InlineWatchpointSet::new(
                crate::bytecode::watchpoint::WatchpointState::ClearWatchpoint,
            ))),
            reg_exp_prototype_constructor_watchpoint: RefCell::new(None),
            reg_exp_constructor_species_watchpoint: RefCell::new(None),
            error_structures: RefCell::new(Vec::new()),
            wasm_error_structures: RefCell::new(Vec::new()),
            error_constructor: RefCell::new(None),
            stack_trace_limit: Cell::new(Some(crate::runtime::options_list::Options::default_error_stack_trace_limit())),
            console_client: RefCell::new(None),
            console_host: RefCell::new(None),
            dom_host: RefCell::new(None),
            self_ref: RefCell::new(std::rc::Weak::new()),
            module_fs: RefCell::new(None),
            call_site_structure: RefCell::new(None),
            generator_prototype: RefCell::new(None),
            async_generator_prototype: RefCell::new(None),
            iterator_prototype: RefCell::new(None),
            array_iterator_prototype: RefCell::new(None),
            array_iterator_structure: RefCell::new(None),
            array_iterator_proto_next: RefCell::new(None),
            iteration_protocol: RefCell::new(Default::default()),
            map_iterator_prototype: RefCell::new(None),
            map_iterator_structure: RefCell::new(None),
            set_iterator_prototype: RefCell::new(None),
            set_iterator_structure: RefCell::new(None),
            segments_structure: RefCell::new(None),
            segment_iterator_structure: RefCell::new(None),
            map_structure: RefCell::new(None),
            set_structure: RefCell::new(None),
            weak_map_structure: RefCell::new(None),
            weak_set_structure: RefCell::new(None),
            map_constructor: RefCell::new(None),
            set_constructor: RefCell::new(None),
            map_proto_entries_function: RefCell::new(None),
            set_proto_values_function: RefCell::new(None),
            string_regexp_globals: RefCell::new(Default::default()),
            array_proto_values_function: RefCell::new(None),
            throw_type_error_arguments_callee_getter_setter: RefCell::new(None),
            debugger: RefCell::new(None),
            promise_data: RefCell::new(crate::runtime::promise_constructor::PromiseGlobalData::default()),
            executable_for_cached_function_executable_for_function_constructor: RefCell::new(None),
            function_kind_data: RefCell::new(Default::default()),
            iterator_data: RefCell::new(Default::default()),
            temporal_data: RefCell::new(Default::default()),
        }
    }

    /// `generatorPrototype()`: invariante do `init(vm)`.
    pub fn generator_prototype(&self) -> JSObjectRef {
        self.generator_prototype.borrow().clone().expect("JSGlobalObject sem generatorPrototype")
    }

    /// `iteratorPrototype()`: invariante do `init(vm)`.
    pub fn iterator_prototype(&self) -> JSObjectRef {
        self.iterator_prototype.borrow().clone().expect("JSGlobalObject sem iteratorPrototype")
    }

    /// `arrayIteratorPrototype()`: invariante do `init(vm)`.
    pub fn array_iterator_prototype(&self) -> JSObjectRef {
        self.array_iterator_prototype.borrow().clone().expect("JSGlobalObject sem arrayIteratorPrototype")
    }

    /// `arrayIteratorStructure()`: invariante do `init(vm)`.
    pub fn array_iterator_structure(&self) -> StructureRef {
        self.array_iterator_structure.borrow().clone().expect("JSGlobalObject sem arrayIteratorStructure")
    }

    /// `arrayIteratorProtocolWatchpointSet().isStillValid()`, no trecho que o `FastArray` do
    /// `getIterationMode` precisa: `%ArrayIteratorPrototype%.next` ainda é o valor original. Não há
    /// watchpoint no porte, então a conferência é direta (e exata, não só "ninguém mexeu").
    pub fn array_iterator_protocol_is_intact(&self) -> bool {
        let Some(original) = *self.array_iterator_proto_next.borrow() else { return false };
        let vm = self.vm();
        self.array_iterator_prototype().get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.next)) == original
    }

    /// `arrayProtoValuesFunction()`: o `LazyProperty` cria a função na primeira leitura.
    pub fn array_proto_values_function(&self) -> crate::runtime::js_function::JSFunctionRef {
        if let Some(function) = self.array_proto_values_function.borrow().as_ref() {
            return function.clone();
        }
        let function = crate::runtime::array_prototype::create_array_proto_values_function(self.vm(), self);
        *self.array_proto_values_function.borrow_mut() = Some(function.clone());
        function
    }

    /// `throwTypeErrorArgumentsCalleeGetterSetter()`: o `%ThrowTypeError%` de `arguments.callee` em modo
    /// estrito; o `LazyProperty` cria o par na primeira leitura.
    pub fn throw_type_error_arguments_callee_getter_setter(&self) -> crate::runtime::js_getter_setter::GetterSetterRef {
        if let Some(accessor) = self.throw_type_error_arguments_callee_getter_setter.borrow().as_ref() {
            return accessor.clone();
        }
        let accessor = crate::runtime::js_global_object_functions_natives::create_throw_type_error_arguments_callee_getter_setter(
            self.vm(),
            self,
        );
        *self.throw_type_error_arguments_callee_getter_setter.borrow_mut() = Some(accessor.clone());
        accessor
    }

    pub fn async_generator_prototype(&self) -> JSObjectRef {
        self.async_generator_prototype.borrow().clone().expect("JSGlobalObject sem asyncGeneratorPrototype")
    }

    /// `objectPrototype()`: invariante do `init(vm)`.
    pub fn object_prototype(&self) -> JSObjectRef {
        self.object_prototype.borrow().clone().expect("JSGlobalObject sem objectPrototype")
    }

    /// `m_objectPrototype.set(vm, this, prototype)`.
    pub fn set_object_prototype(&self, prototype: JSObjectRef) {
        *self.object_prototype.borrow_mut() = Some(prototype);
    }

    /// `globalLexicalBindingEpoch()`.
    pub fn global_lexical_binding_epoch(&self) -> u32 {
        self.global_lexical_binding_epoch.get()
    }

    /// `bumpGlobalLexicalBindingEpoch(VM&)`.
    ///
    /// DIVERGÊNCIA: o `Heap` do porte não tem `codeBlockSet()`, então o laço que chama
    /// `notifyLexicalBindingUpdate()` nos `CodeBlock` deste global object, no estouro da época, não
    /// existe ainda; o limiar padrão (`UINT_MAX`) nunca é atingido na prática.
    pub fn bump_global_lexical_binding_epoch(&self, _vm: &VM) {
        use crate::runtime::options_list::Options;
        let epoch = self.global_lexical_binding_epoch.get().wrapping_add(1);
        if epoch == Options::threshold_for_global_lexical_binding_epoch() {
            // Since the epoch overflows, we should rewrite all the CodeBlock to adjust to the newly started generation.
            self.global_lexical_binding_epoch.set(1);
        } else {
            self.global_lexical_binding_epoch.set(epoch);
        }
    }

    /// `tryGetCachedFunctionExecutableForFunctionConstructor(name, program, sourceOrigin,
    /// sourceTaintedOrigin, sourceURL, startPosition, lexicallyScopedFeatures, mode)`.
    #[allow(clippy::too_many_arguments)]
    pub fn try_get_cached_function_executable_for_function_constructor(
        &self,
        name: &Identifier,
        program: &crate::wtf::text::wtf_string::String,
        source_origin: &crate::runtime::source_origin::SourceOrigin,
        tainted_origin: crate::parser::source_tainted_origin::SourceTaintedOrigin,
        source_url: &crate::wtf::text::wtf_string::String,
        position: &crate::wtf::text::text_position::TextPosition,
        lexically_scoped_features: crate::parser::parser_modes::LexicallyScopedFeatures,
        function_construction_mode: crate::parser::parser_modes::FunctionConstructionMode,
    ) -> Option<Rc<RefCell<crate::runtime::function_executable::FunctionExecutable>>> {
        use crate::parser::source_provider::SourceProvider as _;
        if !self.default_code_generation_mode().is_empty() {
            return None;
        }
        let executable = self
            .executable_for_cached_function_executable_for_function_constructor
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())?;
        {
            let function = executable.borrow();
            let unlinked = function.unlinked_executable().borrow();
            if *name != unlinked.name() {
                return None;
            }
            if lexically_scoped_features != unlinked.lexically_scoped_features() {
                return None;
            }

            let stored_source = function.source();
            if stored_source.first_line().zero_based_int() != 0 {
                return None;
            }

            let offset = crate::runtime::function_constructor::function_constructor_prefix(function_construction_mode).len() as u32
                + name.length();
            if offset as i32 != stored_source.start_column().zero_based_int() {
                return None;
            }
            if program.length() < offset
                || program.substring(offset, program.length() - offset) != stored_source.view()
            {
                return None;
            }

            let stored_provider = stored_source.provider()?;
            if stored_provider.start_position() != *position {
                return None;
            }
            if stored_provider.source_origin() != source_origin {
                return None;
            }
            if stored_provider.source_url() != source_url {
                return None;
            }
            if stored_provider.source_tainted_origin() != tainted_origin {
                return None;
            }
        }
        Some(executable)
    }

    /// `cachedFunctionExecutableForFunctionConstructor(FunctionExecutable*)`.
    pub fn cached_function_executable_for_function_constructor(
        &self,
        executable: &Rc<RefCell<crate::runtime::function_executable::FunctionExecutable>>,
    ) {
        use crate::parser::parser_modes::NO_EVAL_CACHE_FEATURE;
        use crate::parser::source_provider::SourceProvider as _;
        if !self.default_code_generation_mode().is_empty() {
            return;
        }
        {
            let function = executable.borrow();
            if function.source().provider().is_some_and(|provider| provider.could_be_tainted()) {
                return;
            }
            if function.unlinked_executable().borrow().features() & NO_EVAL_CACHE_FEATURE != 0 {
                return;
            }
        }
        *self.executable_for_cached_function_executable_for_function_constructor.borrow_mut() =
            Some(Rc::downgrade(executable));
    }

    /// `linkTimeConstant(LinkTimeConstant)` (JSGlobalObjectInlines.h): o valor do `LazyProperty`, que
    /// o C++ inicializa na primeira leitura e aqui precisa já ter sido posto por `set_link_time_constant`
    /// (os construtores e builtins de `init(vm)` ainda não foram portados).
    pub fn link_time_constant(&self, id: LinkTimeConstant) -> JSValue {
        let value = self.link_time_constants.borrow()[id as usize].clone();
        assert!(!value.is_empty(), "LinkTimeConstant {:?} não inicializada no JSGlobalObject", id);
        value
    }

    /// `m_linkTimeConstants[id].set(vm, this, cell)`.
    pub fn set_link_time_constant(&self, id: LinkTimeConstant, value: JSValue) {
        self.link_time_constants.borrow_mut()[id as usize] = value;
    }

    /// `structureCache()`.
    pub fn structure_cache(&self) -> &crate::runtime::structure_cache::StructureCache {
        &self.structure_cache
    }

    /// `regExpGlobalData()`.
    pub fn reg_exp_global_data(&self) -> &crate::runtime::reg_exp_global_data::RegExpGlobalData {
        &self.reg_exp_global_data
    }

    /// `symbolTableCache().get(symbolTable)`.
    pub fn symbol_table_cache_get(&self, original: &SymbolTableRef) -> Option<SymbolTableRef> {
        self.symbol_table_cache.borrow().get(&original.cell_id()).cloned()
    }

    /// `symbolTableCache().set(symbolTable, clone)`.
    pub fn symbol_table_cache_set(&self, original: &SymbolTableRef, clone: &SymbolTableRef) {
        self.symbol_table_cache.borrow_mut().insert(original.cell_id(), Rc::clone(clone));
    }

    /// `moduleEnvironmentStructure()`: o `LazyProperty` com `JSModuleEnvironment::createStructure(vm, owner)`.
    pub fn module_environment_structure(&self) -> StructureRef {
        if let Some(structure) = self.structures.borrow().module_environment_structure.clone() {
            return structure;
        }
        let structure = JSModuleEnvironment::create_structure(&self.vm, self);
        self.structures.borrow_mut().module_environment_structure = Some(structure.clone());
        structure
    }

    /// `moduleNamespaceObjectStructure()`: o `LazyProperty` com
    /// `JSModuleNamespaceObject::createStructure(vm, owner, jsNull())`. (`moduleRecordStructure()` e
    /// `syntheticModuleRecordStructure()` não existem: os registros de módulo não são `JSObject`.)
    pub fn module_namespace_object_structure(&self) -> StructureRef {
        if let Some(structure) = self.structures.borrow().module_namespace_object_structure.clone() {
            return structure;
        }
        // No bun o protótipo é um objeto próprio com o acessor `__esModule` (e não `null`).
        let prototype = crate::runtime::js_module_namespace_object::create_namespace_prototype(&self.vm, self);
        let structure = crate::runtime::js_module_namespace_object::JSModuleNamespaceObject::create_structure(&self.vm, self, prototype);
        self.structures.borrow_mut().module_namespace_object_structure = Some(structure.clone());
        structure
    }

    /// `withScopeStructure()`: o `LazyProperty` com `JSWithScope::createStructure(vm, owner, jsNull())`.
    pub fn with_scope_structure(&self) -> StructureRef {
        if let Some(structure) = self.structures.borrow().with_scope_structure.clone() {
            return structure;
        }
        let structure = JSWithScope::create_structure(&self.vm, self, js_null());
        self.structures.borrow_mut().with_scope_structure = Some(structure.clone());
        structure
    }

    /// `regExpStructure()`: `RegExpObject::createStructure(vm, owner, regExpPrototype)`. O protótipo é o do
    /// campo `reg_exp_prototype` (preenchido por `init`); antes disso é nulo.
    pub fn reg_exp_structure(&self) -> StructureRef {
        if let Some(structure) = self.structures.borrow().reg_exp_structure.clone() {
            return structure;
        }
        let prototype = self.reg_exp_prototype.borrow().as_ref().map_or_else(js_null, |prototype| prototype.as_value());
        let structure = RegExpObject::create_structure(&self.vm, Some(self), prototype);
        self.structures.borrow_mut().reg_exp_structure = Some(structure.clone());
        structure
    }

    /// `generatorFunctionStructure()`: o `LazyProperty` com `JSGeneratorFunction::createStructure(vm,
    /// owner, generatorFunctionPrototype)`. Antes de `install_function_kind_intrinsics` criar o
    /// `GeneratorFunctionPrototype`, o protótipo é nulo (como o do `errorStructure`).
    pub fn generator_function_structure(&self) -> StructureRef {
        if let Some(structure) = self.structures.borrow().generator_function_structure.clone() {
            return structure;
        }
        let prototype = self.function_kind_prototype_value(|data| data.generator_function_prototype.clone());
        let structure = JSGeneratorFunction::create_structure(&self.vm, self, prototype);
        self.structures.borrow_mut().generator_function_structure = Some(structure.clone());
        structure
    }

    /// `asyncFunctionStructure()`: idem, com `JSAsyncFunction` e `asyncFunctionPrototype`.
    pub fn async_function_structure(&self) -> StructureRef {
        if let Some(structure) = self.structures.borrow().async_function_structure.clone() {
            return structure;
        }
        let prototype = self.function_kind_prototype_value(|data| data.async_function_prototype.clone());
        let structure = JSAsyncFunction::create_structure(&self.vm, self, prototype);
        self.structures.borrow_mut().async_function_structure = Some(structure.clone());
        structure
    }

    /// `asyncGeneratorFunctionStructure()`: idem, com `JSAsyncGeneratorFunction` e
    /// `asyncGeneratorFunctionPrototype`.
    pub fn async_generator_function_structure(&self) -> StructureRef {
        if let Some(structure) = self.structures.borrow().async_generator_function_structure.clone() {
            return structure;
        }
        let prototype = self.function_kind_prototype_value(|data| data.async_generator_function_prototype.clone());
        let structure = JSAsyncGeneratorFunction::create_structure(&self.vm, self, prototype);
        self.structures.borrow_mut().async_generator_function_structure = Some(structure.clone());
        structure
    }

    /// `create(vm, structure)` seguido de `finishCreation(vm)` (a parte que este porte tem):
    /// `Base::finishCreation` (a `SymbolTable`), `structure()->setRealm(vm, this)` e o começo de
    /// `init(vm)`; `function_prototype` é o `m_functionPrototype` (ver o topo do módulo).
    pub fn create(vm: &Rc<VM>, structure: StructureRef, function_prototype: JSValue) -> JSGlobalObjectRef {
        let global_object = Rc::new(JSGlobalObject::new(vm, structure));
        cell_registry::set(global_object.cell_id(), CellEntry::Scope(JSScopeRef::GlobalObject(Rc::clone(&global_object))));
        JSGlobalObject::finish_creation(&global_object, function_prototype);
        global_object
    }

    /// `finishCreation(vm)` e as linhas de `init(vm)` que montam callees e estruturas de função
    /// (JSGlobalObject.cpp:1110-1136).
    fn finish_creation(this: &JSGlobalObjectRef, function_prototype: JSValue) {
        let vm: &VM = &this.vm;
        this.base.finish_creation(vm);
        this.structure().set_realm(vm, this);

        let callee_structure = JSCallee::create_structure(vm, this, js_null());
        this.structures.borrow_mut().callee_structure = Some(callee_structure);

        // `m_errorStructure`: sem `ErrorPrototype` portado, o protótipo é nulo (como o do callee).
        let error_structure = ErrorInstance::create_structure(vm, Some(&**this), js_null());
        this.structures.borrow_mut().error_structure = Some(error_structure);

        this.structures.borrow_mut().lexical_environment_structure = Some(JSLexicalEnvironment::create_structure(vm, this));

        let global_lexical_environment = JSGlobalLexicalEnvironment::create(
            vm,
            JSGlobalLexicalEnvironment::create_structure(vm, this),
            Some(JSScopeRef::GlobalObject(Rc::clone(this))),
        );
        *this.global_lexical_environment.borrow_mut() = Some(global_lexical_environment);

        // Need to create the callee structure (above) before creating the callee.
        *this.global_callee.borrow_mut() = Some(JSCallee::create(vm, this, this.global_scope()));
        *this.eval_callee.borrow_mut() = Some(JSCallee::create(vm, this, this.global_scope()));
        *this.zombie_frame_callee.borrow_mut() = Some(JSCallee::create(vm, this, this.global_scope()));

        this.structures.borrow_mut().host_function_structure = Some(JSFunction::create_structure(vm, this, function_prototype));
        this.structures.borrow_mut().remote_function_structure = Some(JSRemoteFunction::create_structure(vm, this, function_prototype));

        let init_function_structures = || FunctionStructures {
            strict_function_structure: Some(JSStrictFunction::create_structure(vm, this, function_prototype)),
            strict_method_structure: Some(JSStrictFunction::create_structure(vm, this, function_prototype)),
            sloppy_function_structure: Some(JSSloppyFunction::create_structure(vm, this, function_prototype)),
            sloppy_method_structure: Some(JSSloppyFunction::create_structure(vm, this, function_prototype)),
            arrow_function_structure: Some(JSArrowFunction::create_structure(vm, this, function_prototype)),
        };
        let builtin_functions = init_function_structures();
        let ordinary_functions = init_function_structures();
        {
            let mut structures = this.structures.borrow_mut();
            structures.builtin_functions = builtin_functions;
            structures.ordinary_functions = ordinary_functions;
        }

        // `setGlobalThis(vm, JSGlobalProxy::create(vm, JSGlobalProxy::createStructure(vm, this,
        // getPrototypeDirect()), this))`, e a propriedade `globalThis` da `globalObjectTable`
        // (`DontEnum|CellProperty`, o valor do `m_globalThis`).
        let global_this = JSGlobalProxy::create(
            vm,
            JSGlobalProxy::create_structure(vm, Some(&**this), this.get_prototype_direct()),
            Some(this),
        );
        let global_this =
            JSObject::from_cell_id(global_this.cell_id()).expect("JSGlobalProxy não é um JSObject do registro");
        // A propriedade `globalThis` em si entra depois de `eval`, na ordem do bun (ver
        // `js_global_object_functions_natives.rs`).
        this.set_global_this(global_this);
    }

    /// `vm()`.
    pub fn vm(&self) -> &VM {
        &self.vm
    }

    /// `vm()` como `Rc<VM>`: o `VM&` do C++ que os `RefPtr`/`Ref` de quem guarda o `VM` copiam.
    pub fn vm_rc(&self) -> Rc<VM> {
        Rc::clone(&self.vm)
    }

    /// `hasDebugger()`.
    pub fn has_debugger(&self) -> bool {
        self.debugger.borrow().is_some()
    }

    /// `hasInteractiveDebugger()`.
    pub fn has_interactive_debugger(&self) -> bool {
        self.debugger.borrow().as_ref().is_some_and(|debugger| debugger.is_interactively_debugging())
    }

    /// `debugger()`: o C++ devolve `Debugger*` (possivelmente nulo); quem chama testa `hasDebugger()` antes.
    pub fn debugger(&self) -> Rc<crate::debugger::debugger::Debugger> {
        Rc::clone(self.debugger.borrow().as_ref().expect("JSGlobalObject::debugger() sem Debugger anexado"))
    }

    /// `m_debugger = debugger` (`JSGlobalObject::setDebugger`, só a atribuição).
    pub fn set_debugger(&self, debugger: Option<Rc<crate::debugger::debugger::Debugger>>) {
        *self.debugger.borrow_mut() = debugger;
    }

    /// `defaultCodeGenerationMode()`. `vm().typeProfiler()` e `vm().controlFlowProfiler()` ainda não
    /// existem no `VM` do porte (sempre nulos), então só o ramo `Debugger` pode acender.
    pub fn default_code_generation_mode(&self) -> crate::parser::parser_modes::CodeGenerationModeSet {
        use crate::parser::parser_modes::{CodeGenerationMode, CodeGenerationModeSet};
        use crate::runtime::options_list::Options;
        let mut code_generation_mode = CodeGenerationModeSet::default();
        if self.has_interactive_debugger()
            || Options::force_debugger_bytecode_generation()
            || Options::debugger_triggers_breakpoint_exception()
        {
            code_generation_mode.add(CodeGenerationMode::Debugger);
        }
        code_generation_mode
    }

    /// O próprio global como `Rc`, para o código que só recebe `&JSGlobalObject` (um host function de frame).
    pub fn to_ref(&self) -> Option<JSGlobalObjectRef> {
        self.self_ref.borrow().upgrade()
    }

    /// `globalThis()`.
    pub fn global_this(&self) -> Option<JSObjectHandle> {
        self.global_this.borrow().clone()
    }

    /// `setGlobalThis(vm, globalThis)`.
    pub fn set_global_this(&self, global_this: JSObjectHandle) {
        *self.global_this.borrow_mut() = Some(global_this);
    }

    /// `originalArrayStructureForIndexingType(indexingType)`.
    pub fn original_array_structure_for_indexing_type(&self, indexing_type: IndexingType) -> StructureRef {
        debug_assert!(indexing_type & IS_ARRAY != 0);
        self.original_array_structure_for_indexing_shape.borrow()[array_index_from_indexing_type(indexing_type) as usize]
            .clone()
            .expect("JSGlobalObject sem a estrutura de array original dessa forma")
    }

    /// `arrayStructureForIndexingTypeDuringAllocation(indexingType)`: depois do `haveABadTime` é sempre a
    /// de `ArrayWithSlowPutArrayStorage`.
    pub fn array_structure_for_indexing_type_during_allocation(&self, indexing_type: IndexingType) -> StructureRef {
        debug_assert!(indexing_type & IS_ARRAY != 0);
        self.array_structure_for_indexing_shape_during_allocation.borrow()
            [array_index_from_indexing_type(indexing_type) as usize]
            .clone()
            .expect("JSGlobalObject sem a estrutura de array de alocação dessa forma")
    }

    /// `arrayStructureForIndexingTypeDuringAllocation(UndecidedShape)`: a estrutura dos arrays que o
    /// interpretador e os builtins criam vazios e preenchem depois.
    pub fn array_structure(&self) -> StructureRef {
        self.array_structure_for_indexing_type_during_allocation(ARRAY_WITH_UNDECIDED)
    }

    /// `isOriginalArrayStructure(structure)`. Falso enquanto `init` não criou as estruturas.
    pub fn is_original_array_structure(&self, structure: &StructureRef) -> bool {
        let table = self.original_array_structure_for_indexing_shape.borrow();
        table
            .get(array_index_from_indexing_type(structure.indexing_mode() | IS_ARRAY) as usize)
            .and_then(Option::as_ref)
            .is_some_and(|original| Rc::ptr_eq(original, structure))
    }

    /// O trecho de `fireWatchpointAndMakeAllArrayStructuresSlowPut` que troca todas as estruturas de
    /// alocação pela de `ArrayWithSlowPutArrayStorage`.
    pub(crate) fn make_array_structures_during_allocation_slow_put(&self) {
        // Um global criado sem `init` (`api/eval.rs`) ainda não tem a tabela.
        let Some(slow_put) = self.original_array_structure_for_indexing_shape.borrow()
            [array_index_from_indexing_type(ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE) as usize]
            .clone()
        else {
            return;
        };
        for entry in self.array_structure_for_indexing_shape_during_allocation.borrow_mut().iter_mut() {
            *entry = Some(Rc::clone(&slow_put));
        }
    }

    /// `globalLexicalEnvironment()`: invariante do `finishCreation`.
    pub fn global_lexical_environment(&self) -> JSGlobalLexicalEnvironmentRef {
        self.global_lexical_environment.borrow().clone().expect("JSGlobalObject sem globalLexicalEnvironment")
    }

    /// `globalScope()`: o `m_globalLexicalEnvironment`.
    pub fn global_scope(&self) -> JSScopeRef {
        JSScopeRef::GlobalLexicalEnvironment(self.global_lexical_environment())
    }

    /// `globalScopeExtension()`.
    pub fn global_scope_extension(&self) -> Option<JSScopeRef> {
        self.global_scope_extension.borrow().clone()
    }

    /// `setGlobalScopeExtension(scope)`.
    pub fn set_global_scope_extension(&self, scope: JSScopeRef) {
        *self.global_scope_extension.borrow_mut() = Some(scope);
    }

    /// `clearGlobalScopeExtension()`.
    pub fn clear_global_scope_extension(&self) {
        *self.global_scope_extension.borrow_mut() = None;
    }

    /// `globalCallee()`.
    pub fn global_callee(&self) -> Rc<JSCallee> {
        self.global_callee.borrow().clone().expect("JSGlobalObject sem globalCallee")
    }

    /// `evalCallee()`.
    pub fn eval_callee(&self) -> Rc<JSCallee> {
        self.eval_callee.borrow().clone().expect("JSGlobalObject sem evalCallee")
    }

    /// `evalFunction()`: o `cell_id` do `JSFunction` de `eval`, `None` enquanto ninguém o instalou.
    pub fn eval_function(&self) -> Option<usize> {
        self.eval_function.get()
    }

    /// `m_evalFunction.set(vm, this, function)`.
    pub fn set_eval_function(&self, cell_id: usize) {
        self.eval_function.set(Some(cell_id));
    }

    /// `m_zombieFrameCallee`.
    pub fn zombie_frame_callee(&self) -> Rc<JSCallee> {
        self.zombie_frame_callee.borrow().clone().expect("JSGlobalObject sem zombieFrameCallee")
    }

    /// `calleeStructure()`.
    pub fn callee_structure(&self) -> StructureRef {
        self.structures.borrow().callee_structure.clone().expect("JSGlobalObject sem calleeStructure")
    }

    /// `errorStructure(ErrorType)`: a `Structure` do `ErrorInstance` do tipo, com o protótipo do tipo
    /// (`Error.prototype` ou o `NativeErrorPrototype`), quando `init` já os criou; senão a única de antes.
    pub fn error_structure_for(&self, error_type: crate::runtime::error_type::ErrorType) -> StructureRef {
        let per_type = self.error_structures.borrow().iter().find(|(known, _)| *known == error_type).map(|(_, structure)| structure.clone());
        per_type.unwrap_or_else(|| self.error_structure())
    }

    /// `errorStructure(ErrorType)`: a `Structure` do `ErrorInstance` (única até os protótipos por tipo).
    pub fn error_structure(&self) -> StructureRef {
        self.structures.borrow().error_structure.clone().expect("JSGlobalObject sem errorStructure")
    }

    /// `hostFunctionStructure()`.
    pub fn host_function_structure(&self) -> StructureRef {
        self.structures.borrow().host_function_structure.clone().expect("JSGlobalObject sem hostFunctionStructure")
    }

    /// `remoteFunctionStructure()`.
    pub fn remote_function_structure(&self) -> StructureRef {
        self.structures.borrow().remote_function_structure.clone().expect("JSGlobalObject sem remoteFunctionStructure")
    }

    /// `activationStructure()`.
    pub fn activation_structure(&self) -> StructureRef {
        self.structures.borrow().lexical_environment_structure.clone().expect("JSGlobalObject sem activationStructure")
    }

    /// O `FunctionStructures` de `m_builtinFunctions` ou de `m_ordinaryFunctions`.
    fn function_structures(&self, is_builtin: bool) -> FunctionStructures {
        let structures = self.structures.borrow();
        if is_builtin {
            structures.builtin_functions.clone()
        } else {
            structures.ordinary_functions.clone()
        }
    }

    /// `arrowFunctionStructure(isBuiltin)`.
    pub fn arrow_function_structure(&self, is_builtin: bool) -> StructureRef {
        self.function_structures(is_builtin).arrow_function_structure.expect("JSGlobalObject sem arrowFunctionStructure")
    }

    /// `sloppyFunctionStructure(isBuiltin)`.
    pub fn sloppy_function_structure(&self, is_builtin: bool) -> StructureRef {
        self.function_structures(is_builtin).sloppy_function_structure.expect("JSGlobalObject sem sloppyFunctionStructure")
    }

    /// `strictFunctionStructure(isBuiltin)`.
    pub fn strict_function_structure(&self, is_builtin: bool) -> StructureRef {
        self.function_structures(is_builtin).strict_function_structure.expect("JSGlobalObject sem strictFunctionStructure")
    }

    /// `sloppyMethodStructure(isBuiltin)`.
    pub fn sloppy_method_structure(&self, is_builtin: bool) -> StructureRef {
        self.function_structures(is_builtin).sloppy_method_structure.expect("JSGlobalObject sem sloppyMethodStructure")
    }

    /// `strictMethodStructure(isBuiltin)`.
    pub fn strict_method_structure(&self, is_builtin: bool) -> StructureRef {
        self.function_structures(is_builtin).strict_method_structure.expect("JSGlobalObject sem strictMethodStructure")
    }

    /// `createStructure(vm, prototype)` (JSGlobalObjectInlines.h:664).
    pub fn create_structure(vm: &VM, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            None,
            prototype,
            TypeInfo::new(JSType::GlobalObjectType, JSGlobalObject::STRUCTURE_FLAGS),
            &JS_GLOBAL_OBJECT_S_INFO,
        )
    }

    /// `getOwnPropertySlot(this, this, ident, slot)` com `InternalMethodType::GetOwnProperty`, reduzida
    /// aos atributos (o que os chamadores abaixo leem do slot): `Base::getOwnPropertySlot`
    /// (`JSObject`) e depois `symbolTableGet`.
    fn own_property_attributes(&self, ident: &Identifier) -> Option<u32> {
        let property_name = PropertyName::from_identifier(ident);
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::GetOwnProperty);
        if self.get_own_property_slot(&self.vm, &property_name, &mut slot) {
            return Some(slot.attributes());
        }
        let key = ident.impl_()?;
        symbol_table_get(&self.base, &key).map(|(_, attributes)| attributes)
    }

    /// `JSObject::getOwnPropertyDescriptor(globalObject, propertyName, descriptor)` sobre o
    /// `JSGlobalObject::getOwnPropertySlot` (`Base::getOwnPropertySlot` e depois `symbolTableGet`).
    pub fn get_own_property_descriptor(
        &self,
        _global_object: &JSGlobalObject,
        property_name: &PropertyName,
        descriptor: &mut crate::runtime::property_descriptor::PropertyDescriptor,
    ) -> bool {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::GetOwnProperty);
        if self.get_own_property_slot(&self.vm, property_name, &mut slot) {
            descriptor.set_property_slot(&slot, property_name);
            return true;
        }
        let Some(key) = property_name.uid() else { return false };
        match symbol_table_get(&self.base, key) {
            Some((value, attributes)) => {
                descriptor.set_descriptor(value, attributes);
                true
            }
            None => false,
        }
    }

    /// `JSGlobalObject::put(cell, globalObject, propertyName, value, slot)`: a tabela de símbolos
    /// (`symbolTablePutTouchWatchpointSet`) responde antes de `Base::put`. Com o `this` alterado
    /// (`isThisValueAltered`) a tabela só decide entre o erro de somente leitura e o
    /// `definePropertyOnReceiver`.
    pub fn put(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        value: JSValue,
        slot: &mut PutPropertySlot,
    ) -> Result<bool, PutError> {
        let object: &JSObject = self;
        let key = property_name.uid();
        let read_only_failure = |should_throw: bool| {
            if should_throw {
                Err(PutError::TypeError(READONLY_PROPERTY_WRITE_ERROR))
            } else {
                Ok(false)
            }
        };

        if js_global_proxy::is_this_value_altered(slot.this_value(), object) {
            if let Some(key) = key {
                let entry = self.symbol_table().borrow().get(key);
                if !entry.is_null() {
                    if entry.is_read_only() {
                        return read_only_failure(slot.is_strict_mode());
                    }
                    return object.define_property_on_receiver(vm, property_name, value, slot);
                }
            }
            return object.put(vm, property_name, value, slot);
        }

        if let Some(key) = key {
            match symbol_table_put(&**self, key, value, slot.is_strict_mode(), false) {
                SymbolTablePut::Stored => return Ok(true),
                SymbolTablePut::ReadOnly { should_throw } => return read_only_failure(should_throw),
                SymbolTablePut::NotFound => {}
            }
        }
        object.put(vm, property_name, value, slot)
    }

    /// `canDeclareGlobalVar(ident)` (https://tc39.es/ecma262/#sec-candeclareglobalvar).
    pub fn can_declare_global_var(&self, ident: &Identifier) -> bool {
        if self.is_structure_extensible() {
            return true;
        }

        self.own_property_attributes(ident).is_some()
    }

    /// `createGlobalVarBinding<context>(ident)` (https://tc39.es/ecma262/#sec-createglobalvarbinding).
    pub fn create_global_var_binding(&self, context: BindingCreationContext, ident: &Identifier) {
        let has_property = self.own_property_attributes(ident).is_some();
        if has_property {
            return;
        }

        debug_assert!(self.is_structure_extensible());
        match context {
            BindingCreationContext::Global => self.add_symbol_table_entry(ident),
            BindingCreationContext::Eval => {
                self.put_direct(&self.vm, &PropertyName::from_identifier(ident), js_undefined(), 0);
            }
        }
    }

    /// `canDeclareGlobalFunction(ident)` (https://tc39.es/ecma262/#sec-candeclareglobalfunction).
    pub fn can_declare_global_function(&self, ident: &Identifier) -> bool {
        let Some(attributes) = self.own_property_attributes(ident) else {
            return self.is_structure_extensible();
        };

        let is_configurable = attributes & DONT_DELETE == 0;
        if is_configurable {
            return true;
        }
        let is_data_descriptor = attributes & (ACCESSOR | CUSTOM_ACCESSOR) == 0;
        let is_writable_and_enumerable = attributes & (READ_ONLY | DONT_ENUM) == 0;
        is_data_descriptor && is_writable_and_enumerable
    }

    /// `createGlobalFunctionBinding<context>(ident)` (https://tc39.es/ecma262/#sec-createglobalfunctionbinding).
    pub fn create_global_function_binding(&self, context: BindingCreationContext, ident: &Identifier) {
        match self.own_property_attributes(ident) {
            Some(attributes) => {
                if attributes & DONT_DELETE != 0 {
                    debug_assert!(attributes & READ_ONLY == 0);
                    // Nothing to do here: there is either a symbol table entry or non-configurable writable property
                    // on the structure that will be updated with real function by put_to_scope.
                } else {
                    let new_attributes = if context == BindingCreationContext::Global { DONT_DELETE } else { 0 };
                    self.put_direct(&self.vm, &PropertyName::from_identifier(ident), js_undefined(), new_attributes);
                }
            }
            None => {
                debug_assert!(self.is_structure_extensible());
                match context {
                    BindingCreationContext::Global => self.add_symbol_table_entry(ident),
                    BindingCreationContext::Eval => {
                        self.put_direct(&self.vm, &PropertyName::from_identifier(ident), js_undefined(), 0);
                    }
                }
            }
        }
    }

    /// `addSymbolTableEntry(ident)`.
    fn add_symbol_table_entry(&self, ident: &Identifier) {
        let key = ident.impl_().expect("addSymbolTableEntry com Identifier nulo");
        let offset: ScopeOffset = {
            let symbol_table = self.symbol_table();
            let mut symbol_table = symbol_table.borrow_mut();
            debug_assert!(!symbol_table.contains(&key));

            let offset = symbol_table.take_next_scope_offset();
            let mut new_entry = SymbolTableEntry::new(VarOffset::from_scope_offset(offset), 0);
            new_entry.prepare_to_watch();
            symbol_table.add(key, new_entry);
            offset
        };

        let offset_for_assert = self.add_variables(1, js_undefined());
        assert!(offset_for_assert == offset);
    }
}

#[cfg(test)]
mod put_tests {
    use super::*;
    use crate::api::eval::new_global_object;
    use crate::runtime::put_property_slot::PutContext;
    use crate::wtf::text::wtf_string::String as WtfString;

    fn ident(vm: &VM, name: &[u8]) -> Identifier {
        Identifier::from_string(vm, &WtfString::from_latin1(name))
    }

    fn slot_for(global_object: &JSGlobalObject, strict: bool) -> PutPropertySlot {
        PutPropertySlot::new(global_object.as_value(), strict, PutContext::UnknownContext, false)
    }

    #[test]
    fn put_writes_the_symbol_table_variable_before_base_put() {
        let (vm, global_object) = new_global_object();
        let x = ident(&vm, b"x");
        global_object.create_global_var_binding(BindingCreationContext::Global, &x);
        let name = PropertyName::from_identifier(&x);

        let mut slot = slot_for(&global_object, true);
        assert_eq!(global_object.put(&vm, &name, JSValue::Int32(5), &mut slot), Ok(true));

        let key = x.impl_().unwrap();
        assert_eq!(symbol_table_get(&**global_object, &key).map(|(value, _)| value), Some(JSValue::Int32(5)));
        // `JSGlobalObject::getOwnPropertySlot` acaba em `symbolTableGet`: a variável é própria do global
        // mesmo sem entrar na estrutura, e o valor lido é o que o `symbolTablePut` guardou.
        assert!(global_object.has_own_property(&vm, &name));
        assert_eq!(global_object.get(&vm, &name), JSValue::Int32(5));
    }

    /// Medido no bun 1.4.2 com `Object.preventExtensions(globalThis)`: `var x1` e `function f1(){}` novos lançam
    /// TypeError; `var Array` passa (própria) e `function Array(){}` passa só porque `Array` é configurável.
    #[test]
    fn can_declare_on_non_extensible_global_matches_bun() {
        let (vm, global_object) = new_global_object();
        global_object.prevent_extensions(&vm);
        assert!(!global_object.is_structure_extensible());

        let fresh = ident(&vm, b"x1");
        assert!(!global_object.can_declare_global_var(&fresh));
        assert!(!global_object.can_declare_global_function(&fresh));
        let array = ident(&vm, b"Array");
        assert!(global_object.can_declare_global_var(&array));
        assert!(global_object.can_declare_global_function(&array));
    }

    #[test]
    fn put_falls_back_to_base_put_without_a_symbol_table_entry() {
        let (vm, global_object) = new_global_object();
        let name = PropertyName::from_identifier(&ident(&vm, b"y"));

        let mut slot = slot_for(&global_object, false);
        assert_eq!(global_object.put(&vm, &name, JSValue::Int32(7), &mut slot), Ok(true));
        assert_eq!(global_object.get(&vm, &name), JSValue::Int32(7));
    }

    #[test]
    fn put_on_a_read_only_symbol_table_entry_throws_only_in_strict_mode() {
        let (vm, global_object) = new_global_object();
        let z = ident(&vm, b"z");
        global_object.create_global_var_binding(BindingCreationContext::Global, &z);
        let key = z.impl_().unwrap();
        {
            let symbol_table = global_object.symbol_table();
            let mut symbol_table = symbol_table.borrow_mut();
            let offset = symbol_table.get(&key).scope_offset();
            symbol_table.set(key.clone(), SymbolTableEntry::new(VarOffset::from_scope_offset(offset), READ_ONLY));
        }
        let name = PropertyName::from_identifier(&z);

        let mut sloppy = slot_for(&global_object, false);
        assert_eq!(global_object.put(&vm, &name, JSValue::Int32(1), &mut sloppy), Ok(false));
        let mut strict = slot_for(&global_object, true);
        assert_eq!(
            global_object.put(&vm, &name, JSValue::Int32(1), &mut strict),
            Err(PutError::TypeError(READONLY_PROPERTY_WRITE_ERROR))
        );
    }
}
