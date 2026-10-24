//! Esqueleto de `JavaScriptCore/runtime/VM.h`.
//!
//! Carrega o que o parser e o `Identifier` usam: `propertyNames`, os dois `SymbolRegistry`, o
//! estado de exceção pendente (`m_exception`, `m_terminationException`) e o contador de
//! `DeferTermination`. A tabela de átomos é por thread em `crate::wtf::text::atom_string_impl`,
//! então o `VM` não a carrega.

use crate::bytecode::bytecode_intrinsic_registry::BytecodeIntrinsicRegistry;
use std::cell::{Cell, OnceCell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

thread_local! {
    /// Orçamento de pilha nativa do próximo `VM::new` desta thread (ver `VM::set_thread_stack_budget`).
    static STACK_BUDGET: Cell<usize> = const { Cell::new(1024 * 1024) };
}

use crate::runtime::common_identifiers::CommonIdentifiers;
use crate::runtime::function_has_executed_cache::FunctionHasExecutedCache;
use crate::runtime::js_value::JSValue;
use crate::runtime::type_profiler::{ControlFlowProfiler, TypeProfiler};
use crate::runtime::symbol_registry::{SymbolRegistry, SymbolRegistryType};

/// `class Exception`: a célula mora em `runtime/exception.rs` (registrada no `cell_registry`); a
/// reexportação mantém o caminho `vm::Exception` dos chamadores.
pub use crate::runtime::exception::Exception;

/// `CommonIdentifiers* propertyNames { nullptr }`: o C++ a deixa nula até o corpo do construtor
/// (`propertyNames = new CommonIdentifiers(*this)`), porque o construtor recebe o `VM&`. O
/// `OnceCell` guarda a mesma ordem; ler antes de atribuir é falha de programação.
#[derive(Debug, Default)]
pub struct PropertyNames(OnceCell<Box<CommonIdentifiers>>);

impl Deref for PropertyNames {
    type Target = CommonIdentifiers;

    fn deref(&self) -> &CommonIdentifiers {
        self.0.get().expect("VM::propertyNames lido antes de ser criado")
    }
}

/// As oito `JSSentinel*` do protocolo de iteração rápida além da `fastArrayValuesSentinel` (`VM.h`).
#[derive(Debug)]
struct FastIterationSentinels {
    array_keys: JSValue,
    array_entries: JSValue,
    map_keys: JSValue,
    map_values: JSValue,
    map_entries: JSValue,
    set_values: JSValue,
    set_entries: JSValue,
    string_values: JSValue,
}

/// `class VM`.
#[derive(Debug)]
pub struct VM {
    exception: RefCell<Option<Rc<Exception>>>,
    termination_exception: RefCell<Option<Rc<Exception>>>,
    /// `VMTraps::m_deferTerminationCount`.
    defer_termination_count: Cell<u32>,
    symbol_registry: Box<SymbolRegistry>,
    private_symbol_registry: Box<SymbolRegistry>,
    pub property_names: PropertyNames,
    /// `m_softStackLimit` (`softStackLimit()`): endereço abaixo do qual a pilha da VM é considerada
    /// cheia. 0 até o `StackBounds` real ser portado.
    soft_stack_limit: Cell<usize>,
    /// `m_executingRegExp`: identidade do `RegExp*` em execução (0 é o `nullptr`).
    executing_reg_exp: Cell<usize>,
    /// O primeiro campo do `$asyncContext` do JSC do bun (`AsyncContextData`, a cabeça da lista de `Frame`s do
    /// `AsyncLocalStorage`); `undefined` quando não há contexto.
    async_context: Cell<JSValue>,
    /// `vm.isAsyncContextTrackingEnabled()`: ligado pelo primeiro `AsyncLocalStorage` ou `AsyncResource`
    /// (`setAsyncHooksEnabled(true)`); desligado, a captura e a reinstalação do contexto não acontecem.
    async_context_tracking: Cell<bool>,
    /// `m_bytecodeIntrinsicRegistry`: criado no primeiro uso, porque precisa dos `BuiltinNames`.
    bytecode_intrinsic_registry: std::cell::OnceCell<BytecodeIntrinsicRegistry>,
    /// `sourceProviderCacheMap`: a chave é a identidade do `SourceProvider` (o `RefPtr` guarda o objeto vivo).
    source_provider_cache_map: RefCell<std::collections::HashMap<usize, (Rc<dyn crate::parser::source_provider::SourceProvider>, Rc<RefCell<crate::parser::source_provider_cache::SourceProviderCache>>)>>,
    /// `m_orderedHashTableSentinel`: criada no fim do `VM::new`, como no construtor do C++.
    ordered_hash_table_sentinel: OnceCell<crate::runtime::js_cell_butterfly::JSCellButterflyRef>,
    /// `m_fastAsyncGeneratorSentinel`: criada no fim do `VM::new`, como no construtor do C++.
    fast_async_generator_sentinel: OnceCell<JSValue>,
    /// `m_fastArrayValuesSentinel`: criada no fim do `VM::new`, como no construtor do C++.
    fast_array_values_sentinel: OnceCell<JSValue>,
    /// `m_fastArrayKeysSentinel`, `m_fastArrayEntriesSentinel`, `m_fastMap*Sentinel`, `m_fastSet*Sentinel` e
    /// `m_fastStringValuesSentinel`: criadas junto das demais, no fim do `VM::new`.
    fast_iteration_sentinels: OnceCell<FastIterationSentinels>,
    /// `symbolTableStructure`: criada no `VM::new`, como em `VM.cpp:384`.
    symbol_table_structure: OnceCell<crate::runtime::structure::StructureRef>,
    /// `stringStructure`: criada no `VM::new`, como em `VM.cpp:351`.
    string_structure: OnceCell<crate::runtime::structure::StructureRef>,
    /// `propertyNameEnumeratorStructure`: criada no `VM::new`, como em `VM.cpp:357`.
    property_name_enumerator_structure: OnceCell<crate::runtime::structure::StructureRef>,
    /// `bigIntStructure`: criada no `VM::new`, como em `VM.cpp:353`.
    big_int_structure: OnceCell<crate::runtime::structure::StructureRef>,
    /// `templateObjectDescriptorStructure`: criada no `VM::new`, como em `VM.cpp:399`.
    template_object_descriptor_structure: OnceCell<crate::runtime::structure::StructureRef>,
    /// `rawImmutableButterflyStructure(...)` e `cellButterflyOnlyAtomStringsStructure` (`VM.cpp:387-394`).
    cell_butterfly_structures: OnceCell<crate::runtime::js_cell_butterfly::CellButterflyStructures>,
    /// `m_moduleAsyncEvaluationCount`.
    module_async_evaluation_count: Cell<i64>,
    /// `atomStringToJSStringMap`.
    atom_string_to_js_string_map: crate::runtime::weak_gc_map::AtomStringToJSStringMap,
    /// `const Ref<CompactTDZEnvironmentMap> m_compactVariableMap`.
    compact_variable_map: Rc<crate::parser::variable_environment::CompactTDZEnvironmentMap>,
    /// `std::unique_ptr<BuiltinExecutables> m_builtinExecutables`.
    builtin_executables: crate::runtime::builtin_executables::BuiltinExecutables,
    /// `Heap heap`.
    heap: crate::runtime::heap::Heap,
    /// `codeCache` (`std::unique_ptr<CodeCache>`).
    code_cache: crate::runtime::code_cache::CodeCache,
    /// `std::unique_ptr<TypeProfiler> m_typeProfiler` e `unsigned m_typeProfilerEnabledCount`.
    type_profiler: RefCell<Option<Rc<TypeProfiler>>>,
    type_profiler_enabled_count: Cell<u32>,
    /// `FunctionHasExecutedCache m_functionHasExecutedCache`.
    function_has_executed_cache: FunctionHasExecutedCache,
    /// `std::unique_ptr<ControlFlowProfiler> m_controlFlowProfiler` e `m_controlFlowProfilerEnabledCount`.
    control_flow_profiler: RefCell<Option<Rc<ControlFlowProfiler>>>,
    control_flow_profiler_enabled_count: Cell<u32>,
    /// `bool m_failNextNewCodeBlock { false }`.
    fail_next_new_code_block: Cell<bool>,
    /// `m_currentWeakRefVersion`: o `WeakRef` criado ou lido nesta versão segura o alvo até ela avançar.
    current_weak_ref_version: Cell<u64>,
    /// `bool m_globalConstRedeclarationShouldThrow { true }`.
    global_const_redeclaration_should_throw: Cell<bool>,
    /// `bool m_allowRedeclaringSymbols { false }`.
    allow_redeclaring_symbols: Cell<bool>,
    /// `Exception* m_lastException`: igual a `m_exception` até `clearLastException()`.
    last_exception: RefCell<Option<Rc<Exception>>>,
    /// `void* m_stackLimit`: o limite duro de `isSafeToRecurse()`. 0 até o `StackBounds` real.
    stack_limit: Cell<usize>,
    /// Bytes de pilha LÓGICA ocupados pelas recursões nativas guardadas (parser, ...), na medida do frame que o
    /// C++ teria (ver `stack_cost` e `enter_logical_frame`). Independe do tamanho da thread.
    logical_stack_used: Rc<Cell<usize>>,
    /// `logical_stack_used` na entrada mais recente no VM (`doVMEntry`): o que a pilha lógica gastou depois dela é
    /// a parte nativa ainda não descontada da `CLoopStack` (ver `stack_used` e `enter_vm`).
    logical_entry_mark: Rc<Cell<usize>>,
    /// `void* m_stackPointerAtVMEntry`.
    stack_pointer_at_vm_entry: Cell<usize>,
    /// `VMEntryScope* entryScope`: identidade do escopo mais externo (0 é `nullptr`).
    entry_scope: Cell<usize>,
    /// Gerador das identidades dos `VMEntryScope` (o endereço do C++ não é estável em Rust).
    next_entry_scope_id: Cell<usize>,
    /// `CallFrame* topCallFrame`, `EntryFrame* topEntryFrame`: identidade do frame na `CLoopStack`
    /// (0 é `nullptr`). O `Interpreter` do porte não os mantém (ver o cabeçalho de `interpreter.rs`);
    /// quem os atualizar passa por aqui.
    top_call_frame: Cell<usize>,
    /// O `CodeBlock` e o `BytecodeIndex` da chamada JS em curso para uma função nativa: o que o
    /// `ErrorInstance::finishCreation` lê no `topCallFrame` para apender `(evaluating '...')` aos erros que a
    /// função nativa cria (`Object.keys(null)`, `a.set(null)`). O laço do LLInt o guarda ao entrar no nativo.
    native_call_site: RefCell<Option<(crate::bytecode::code_block::CodeBlockRef, crate::bytecode::bytecode_index::BytecodeIndex)>>,
    /// O nativo em curso foi chamado por `op_tail_call`/`op_tail_call_varargs`: o JSC já trocou o frame do chamador
    /// (`Reflect.apply` chama `target.@apply(...)` em posição de cauda), então a pilha vista do nativo não o mostra.
    /// Salvo e restaurado como `native_call_site`.
    native_call_tail: Cell<bool>,
    top_entry_frame: Cell<usize>,
    /// `SmallStrings smallStrings`.
    pub small_strings: SmallStrings,
    /// `Interpreter interpreter`: criado no primeiro uso, porque aloca a `CLoopStack`.
    interpreter: InterpreterSlot,
    /// `m_defaultMicrotaskQueue` (ver `microtask_queue.rs`).
    pub(crate) default_microtask_queue: crate::runtime::microtask_queue::MicrotaskQueue,
    /// `m_aboutToBeNotifiedRejectedPromises`, `m_onEachMicrotaskTick` e os ganchos do embedder para a
    /// exceção que uma tarefa deixa pendente (ver `microtask_queue.rs`).
    pub(crate) promise_rejection_state: crate::runtime::microtask_queue::PromiseRejectionState,
    /// `DateCache dateCache`: criado no primeiro uso, porque resolve o fuso do processo (`TZ`).
    date_cache: DateCacheSlot,
}

/// `DateCache dateCache` do `VM`: `OnceCell` para a resolução preguiçosa do fuso do processo.
#[derive(Default)]
struct DateCacheSlot(OnceCell<crate::runtime::js_date_math::DateCache>);

impl std::fmt::Debug for DateCacheSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DateCache")
    }
}

/// `Interpreter interpreter` do `VM`: `OnceCell` para a alocação preguiçosa da `CLoopStack`.
#[derive(Default)]
struct InterpreterSlot(OnceCell<crate::interpreter::interpreter::Interpreter>);

impl std::fmt::Debug for InterpreterSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Interpreter")
    }
}

/// `class SmallStrings`. O cache de `JSString` de um caractere e das strings de `commonIdentifiers`
/// não tem efeito observável (a identidade de `JSString` só aparece por `===` em strings, que compara
/// o conteúdo), então só a string vazia, que `jsEmptyString` expõe, é portada. DEPENDE DE GC: o
/// `visitStrongReferences` e o `initializeCommonStrings` ficam sem corpo até haver `Heap` de verdade.
#[derive(Debug, Default)]
pub struct SmallStrings;

/// `class DeferGC`: `Heap::incrementDeferralDepth()` no construtor e
/// `decrementDeferralDepthAndGCIfNeeded()` no destrutor. Sem coletor, só o contador se move.
pub struct DeferGC<'vm> {
    vm: &'vm VM,
}

impl<'vm> DeferGC<'vm> {
    /// `DeferGC(VM&)`.
    pub fn new(vm: &'vm VM) -> DeferGC<'vm> {
        vm.heap().increment_deferral_depth();
        DeferGC { vm }
    }
}

impl Drop for DeferGC<'_> {
    fn drop(&mut self) {
        self.vm.heap().decrement_deferral_depth_and_gc_if_needed();
    }
}

/// `class VMEntryScope`. O C++ guarda `this` em `vm.entryScope` só no escopo mais externo; aqui a
/// identidade é um número. Sem `Thread::registerJSThread`, Wasm, mach exceptions e
/// `executeEntryScopeServicesOnEntry/Exit` (watchdog, timezone, idioma: nada disso existe).
pub struct VMEntryScope<'vm> {
    vm: &'vm VM,
    id: usize,
    global_object: Cell<Option<usize>>,
}

impl<'vm> VMEntryScope<'vm> {
    /// `VMEntryScope(VM&, JSGlobalObject*)`: o `globalObject` é a identidade da célula.
    pub fn new(vm: &'vm VM, global_object: Option<usize>) -> VMEntryScope<'vm> {
        let id = vm.next_entry_scope_id.get() + 1;
        vm.next_entry_scope_id.set(id);
        if vm.entry_scope.get() == 0 {
            vm.entry_scope.set(id);
        }
        vm.clear_last_exception();
        VMEntryScope { vm, id, global_object: Cell::new(global_object) }
    }

    /// `vm()`.
    pub fn vm(&self) -> &VM {
        self.vm
    }

    /// `globalObject()`.
    pub fn global_object(&self) -> Option<usize> {
        self.global_object.get()
    }

    /// `setGlobalObject(JSGlobalObject*)`.
    pub fn set_global_object(&self, global_object: Option<usize>) {
        self.global_object.set(global_object);
    }
}

impl Drop for VMEntryScope<'_> {
    fn drop(&mut self) {
        // `if (m_vm.entryScope != this) return; tearDownSlow()`.
        if self.vm.entry_scope.get() == self.id {
            self.vm.entry_scope.set(0);
        }
    }
}

/// Custo, em bytes de pilha lógica, de um nível de cada recursão nativa guardada: o tamanho do frame do ciclo de
/// funções do C++ (release, x86-64) que o `isSafeToRecurse` do mesmo ponto protegeria. NÃO é o frame do Rust (que
/// muda com o perfil de compilação); é a constante que reproduz a profundidade do bun. Calibrada no oráculo:
/// profundidade em que o bun devolve "Stack exhausted" = `VM::logical_stack_limit() / custo`.
pub mod stack_cost {
    /// Um ponto de `failIfStackOverflow` de expressão (`parseExpression`, `parseAssignmentExpression`,
    /// `parsePrimaryExpression`, padrões, corpo de arrow). Medido no bun 1.4.2: `(((1)))` estoura em 3504 níveis, e
    /// cada `(` passa por 3 pontos, logo (5 MiB - 64 KiB) / (3503 * 3) = 492,7 bytes.
    pub const PARSER_LEVEL: usize = 493;
    /// Um ponto de `failIfStackOverflow` de comando (`parseStatement`, `parseStatementListItem`). Medido no bun:
    /// `if(1)` repetido estoura em 5757 níveis (1 ponto por nível: 5177344 / 5756 = 899,5), `{` aninhado em 2880
    /// (2 pontos por nível: 899,0). O frame de `parseStatement` é maior que o do ciclo de expressão.
    pub const STATEMENT_LEVEL: usize = 899;
    /// Um nível de `flatIntoArray`. Medido no bun 1.4.2: `flat(Infinity)` passa com 39903 arrays aninhados dentro
    /// do externo (39904 frames) e estoura com 39904. Com a base `FLAT_BASE` e 129 bytes por nível, o limite cai
    /// exatamente aí (29700 + 39904 * 129 <= 5177344 < 29700 + 39905 * 129).
    pub const FLAT_LEVEL: usize = 129;
    /// O que a chamada de `flat` já gasta antes do primeiro nível (ver `FLAT_LEVEL`).
    pub const FLAT_BASE: usize = 29700;
    /// Um nível do ciclo `join` -> `toString` -> `join` num array aninhado. Medido no bun: `join`/`toString`
    /// passam com 4313 arrays aninhados (4314 níveis) e estouram com 4314 (4314 * 1200 <= 5177344 < 4315 * 1200).
    pub const JOIN_LEVEL: usize = 1200;
    /// Os frames nativos do bun abaixo do primeiro `vmEntryToJavaScript` do programa.
    pub const PROGRAM_ENTRY: usize = 3200;
    /// `JSC::call` -> `Interpreter::executeCall` -> `vmEntryToJavaScript` vindo de uma função nativa.
    pub const VM_REENTRY: usize = 1056;
    /// `JSC::call` -> `Interpreter::executeCall` -> `vmEntryToNative` vindo de uma função nativa: o caminho curto de
    /// `executeCall` para alvo nativo, sem `prepareForExecution` nem `CodeBlock`. Medido no bun sem JIT: uma cadeia
    /// de `bind` chega a 13880 níveis, e cada entrada cobre dois deles (o `boundFunctionCall` de um nível chama o
    /// seguinte, que o `executeCall` desembrulha sem entrar no VM): 2 * 368 B menos o frame de 48 B da entrada.
    pub const VM_NATIVE_REENTRY: usize = 688;
    /// `replaceUsingStringSearch` com callback.
    pub const REPLACE_CALLBACK: usize = 96;
    /// `sortStableSort` e a ordenação com comparador.
    pub const SORT_COMPARATOR: usize = 2512;
    /// `jsonParseSlow` e o `Walker` do reviver.
    pub const JSON_REVIVER: usize = 2880;
    /// `globalFuncEval` -> `Interpreter::executeEval`. Medido no bun sem JIT pela recursão plana com 0 a 30 locais
    /// extras dentro de `(0,eval)(...)`: o orçamento que sobra fica entre 5106640 e 5106656 bytes, 48 a mais que com 448.
    pub const INDIRECT_EVAL: usize = 400;
    /// `toPrimitive` -> `ordinaryToPrimitive` -> `callToPrimitiveFunction`.
    pub const TO_PRIMITIVE: usize = 192;
}

/// O trampolim de entrada no VM: `vmEntryToJavaScript` ou `vmEntryToNative`, ou a chamada que no C++ nem entra no VM
/// (`LLIntCall`, o `callHelper` de um handler do LLInt, ver `Interpreter::call_from_llint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMEntryKind {
    JavaScript,
    Native,
    LLIntCall,
}

/// A marca de `VM::enter_vm`, restaurada ao sair do VM (RAII).
#[derive(Debug)]
pub struct VMEntryMark {
    mark: Rc<Cell<usize>>,
    previous: usize,
}

impl Drop for VMEntryMark {
    fn drop(&mut self) {
        self.mark.set(self.previous);
    }
}

/// Reserva de pilha lógica devolvida ao morrer (RAII), ver `VM::enter_logical_frame`.
#[derive(Debug)]
pub struct LogicalStackFrame {
    used: Rc<Cell<usize>>,
    cost: usize,
}

impl Drop for LogicalStackFrame {
    fn drop(&mut self) {
        self.used.set(self.used.get() - self.cost);
    }
}

thread_local! {
    /// Quantos `VM` desta thread ainda estão vivos (só para teste de vazamento).
    static LIVE_VMS: Cell<usize> = const { Cell::new(0) };
}

/// Número de `VM` vivos nesta thread: sobe em `VM::new`, desce no `Drop`.
pub fn live_vm_count() -> usize {
    LIVE_VMS.with(Cell::get)
}

impl Drop for VM {
    fn drop(&mut self) {
        LIVE_VMS.with(|count| count.set(count.get().saturating_sub(1)));
    }
}

impl Default for VM {
    fn default() -> VM {
        VM::new()
    }
}

impl VM {
    /// Quanto da pilha nativa, abaixo do ponto onde o `VM` nasce, a recursão (JS, parser, JSON) pode
    /// consumir antes de virar `RangeError: Maximum call stack size exceeded`. É o papel do
    /// `StackBounds::recursionLimit`, que o Rust seguro não alcança: a pilha da thread tem pelo menos 2 MiB.
    ///
    /// Relação com as opções do C++: `Options::maxPerThreadStackUsage` (5 MiB) e `softReservedZoneSize`
    /// (128 KiB) pressupõem uma thread de pelo menos 5 MiB, que nem a thread de teste do Rust (2 MiB) tem.
    /// O limite gravado em `VM::new` (`stack_limit` = ponteiro de pilha de então menos este orçamento) é
    /// por isso fixo em 1 MiB, e `is_safe_to_recurse` é só a rede de segurança nativa (ver `logical_stack_limit`)
    /// (10 000) quando o frame nativo de um nível passa de ~100 bytes (não medido, mas `llint_execute` mais
    /// `dispatch_loop` ocupam bem mais que isso). A thread que usar profundidade JS maior precisa de pilha maior
    /// e de `set_stack_limit` com o mesmo cálculo.
    const DEFAULT_STACK_BUDGET: usize = 1024 * 1024;

    /// Troca, só nesta thread, o orçamento que o próximo `VM::new` usa (padrão `DEFAULT_STACK_BUDGET`, 1 MiB).
    /// Quem roda JS numa thread de pilha grande chama isto antes de criar o VM, com o tamanho da pilha menos a
    /// folga dos protetores nativos; o bun alcança 45 609 níveis de `function f(n){return n?1+f(n-1):0}`.
    pub fn set_thread_stack_budget(bytes: usize) {
        STACK_BUDGET.with(|budget| budget.set(bytes));
    }

    /// Folga entre o limite duro e o suave (`softStackLimit`), reservada para o código que roda depois do
    /// teste de pilha (tratamento do erro, desenrolar de frames).
    const SOFT_STACK_MARGIN: usize = 64 * 1024;

    /// O endereço de uma local faz o papel de `currentStackPointer()`.
    #[inline(never)]
    fn current_stack_pointer() -> usize {
        let marker = 0u8;
        std::ptr::addr_of!(marker) as usize
    }

    /// `VM::VM(VmType, HeapType, WTF::RunLoop*, bool*)`, só no que este porte tem.
    pub fn new() -> VM {
        let budget = STACK_BUDGET.with(|budget| budget.get());
        let hard_stack_limit = VM::current_stack_pointer().saturating_sub(budget);
        let vm = VM {
            exception: RefCell::new(None),
            termination_exception: RefCell::new(None),
            defer_termination_count: Cell::new(0),
            symbol_registry: Box::new(SymbolRegistry::new(SymbolRegistryType::PublicSymbol)),
            private_symbol_registry: Box::new(SymbolRegistry::new(SymbolRegistryType::PrivateSymbol)),
            property_names: PropertyNames::default(),
            soft_stack_limit: Cell::new(hard_stack_limit.saturating_add(VM::SOFT_STACK_MARGIN)),
            executing_reg_exp: Cell::new(0),
            async_context: Cell::new(JSValue::undefined()),
            async_context_tracking: Cell::new(false),
            bytecode_intrinsic_registry: std::cell::OnceCell::new(),
            source_provider_cache_map: RefCell::new(std::collections::HashMap::new()),
            symbol_table_structure: OnceCell::new(),
            string_structure: OnceCell::new(),
            property_name_enumerator_structure: OnceCell::new(),
            big_int_structure: OnceCell::new(),
            template_object_descriptor_structure: OnceCell::new(),
            cell_butterfly_structures: OnceCell::new(),
            ordered_hash_table_sentinel: OnceCell::new(),
            fast_async_generator_sentinel: OnceCell::new(),
            fast_array_values_sentinel: OnceCell::new(),
            fast_iteration_sentinels: OnceCell::new(),
            module_async_evaluation_count: Cell::new(0),
            atom_string_to_js_string_map: crate::runtime::weak_gc_map::AtomStringToJSStringMap::default(),
            compact_variable_map: crate::parser::variable_environment::CompactTDZEnvironmentMap::new(),
            builtin_executables: crate::runtime::builtin_executables::BuiltinExecutables::new(),
            heap: crate::runtime::heap::Heap::new(),
            code_cache: crate::runtime::code_cache::CodeCache::default(),
            type_profiler: RefCell::new(None),
            type_profiler_enabled_count: Cell::new(0),
            function_has_executed_cache: FunctionHasExecutedCache::default(),
            control_flow_profiler: RefCell::new(None),
            control_flow_profiler_enabled_count: Cell::new(0),
            fail_next_new_code_block: Cell::new(false),
            current_weak_ref_version: Cell::new(0),
            global_const_redeclaration_should_throw: Cell::new(true),
            allow_redeclaring_symbols: Cell::new(false),
            last_exception: RefCell::new(None),
            stack_limit: Cell::new(hard_stack_limit),
            logical_stack_used: Rc::new(Cell::new(0)),
            logical_entry_mark: Rc::new(Cell::new(0)),
            stack_pointer_at_vm_entry: Cell::new(0),
            entry_scope: Cell::new(0),
            next_entry_scope_id: Cell::new(0),
            top_call_frame: Cell::new(0),
            native_call_site: RefCell::new(None),
            native_call_tail: Cell::new(false),
            top_entry_frame: Cell::new(0),
            small_strings: SmallStrings,
            interpreter: InterpreterSlot::default(),
            default_microtask_queue: crate::runtime::microtask_queue::MicrotaskQueue::default(),
            promise_rejection_state: crate::runtime::microtask_queue::PromiseRejectionState::default(),
            date_cache: DateCacheSlot::default(),
        };
        LIVE_VMS.with(|count| count.set(count.get() + 1));
        // VM.cpp:351: `stringStructure.setWithoutWriteBarrier(JSString::createStructure(*this, nullptr, jsNull()))`.
        // Antes de qualquer célula string (`smallStrings`, `CommonIdentifiers`) que a use.
        let string_structure = crate::runtime::js_string::JSString::create_structure(&vm, None, crate::runtime::js_value::js_null());
        assert!(vm.string_structure.set(string_structure).is_ok());
        let property_names = Box::new(CommonIdentifiers::new(&vm));
        assert!(vm.property_names.0.set(property_names).is_ok());
        // VM.cpp:357.
        let property_name_enumerator_structure = crate::runtime::js_property_name_enumerator::JSPropertyNameEnumerator::create_structure(
            &vm,
            None,
            crate::runtime::js_value::js_null(),
        );
        assert!(vm.property_name_enumerator_structure.set(property_name_enumerator_structure).is_ok());
        // VM.cpp:353: `bigIntStructure.setWithoutWriteBarrier(JSBigInt::createStructure(*this, nullptr, jsNull()))`.
        let big_int_structure = crate::runtime::js_big_int::JSBigInt::create_structure(&vm, None, crate::runtime::js_value::js_null());
        assert!(vm.big_int_structure.set(big_int_structure).is_ok());
        // VM.cpp:384: `symbolTableStructure.setWithoutWriteBarrier(SymbolTable::createStructure(*this, nullptr, jsNull()))`.
        let symbol_table_structure = crate::runtime::symbol_table::SymbolTable::create_structure(&vm, None, crate::runtime::js_value::js_null());
        assert!(vm.symbol_table_structure.set(symbol_table_structure).is_ok());
        // VM.cpp:387-394.
        let cell_butterfly_structures = crate::runtime::js_cell_butterfly::CellButterflyStructures::create(&vm);
        assert!(vm.cell_butterfly_structures.set(cell_butterfly_structures).is_ok());
        // VM.cpp:399.
        let template_object_descriptor_structure = crate::runtime::js_template_object_descriptor::JSTemplateObjectDescriptor::create_structure(
            &vm,
            None,
            crate::runtime::js_value::js_null(),
        );
        assert!(vm.template_object_descriptor_structure.set(template_object_descriptor_structure).is_ok());
        // VM.cpp:414: `m_orderedHashTableSentinel.setWithoutWriteBarrier(JSOrderedHashMap::createSentinel(*this))`.
        let sentinel = crate::runtime::js_ordered_hash_table::JSOrderedHashMap::create_sentinel(&vm);
        assert!(vm.ordered_hash_table_sentinel.set(sentinel).is_ok());
        // VM.cpp:429: `m_fastAsyncGeneratorSentinel.setWithoutWriteBarrier(JSSentinel::create(*this, sentinelStructure))`.
        assert!(vm.fast_async_generator_sentinel.set(crate::runtime::js_sentinel::JSSentinel::create_for_vm(&vm)).is_ok());
        // VM.cpp: `m_fastArrayValuesSentinel.setWithoutWriteBarrier(JSSentinel::create(*this, sentinelStructure))`.
        assert!(vm.fast_array_values_sentinel.set(crate::runtime::js_sentinel::JSSentinel::create_for_vm(&vm)).is_ok());
        let sentinel = || crate::runtime::js_sentinel::JSSentinel::create_for_vm(&vm);
        let fast_iteration_sentinels = FastIterationSentinels {
            array_keys: sentinel(),
            array_entries: sentinel(),
            map_keys: sentinel(),
            map_values: sentinel(),
            map_entries: sentinel(),
            set_values: sentinel(),
            set_entries: sentinel(),
            string_values: sentinel(),
        };
        assert!(vm.fast_iteration_sentinels.set(fast_iteration_sentinels).is_ok());
        vm
    }

    /// `typeProfiler()`: `None` é o `nullptr`.
    pub fn type_profiler(&self) -> Option<Rc<TypeProfiler>> {
        self.type_profiler.borrow().clone()
    }

    /// `controlFlowProfiler()`: `None` é o `nullptr`.
    pub fn control_flow_profiler(&self) -> Option<Rc<ControlFlowProfiler>> {
        self.control_flow_profiler.borrow().clone()
    }

    /// `functionHasExecutedCache()`.
    pub fn function_has_executed_cache(&self) -> &FunctionHasExecutedCache {
        &self.function_has_executed_cache
    }

    /// `enableTypeProfiler()`: verdadeiro quando é preciso recompilar (primeira ativação).
    pub fn enable_type_profiler(&self) -> bool {
        let needs_to_recompile = self.type_profiler_enabled_count.get() == 0;
        if needs_to_recompile {
            *self.type_profiler.borrow_mut() = Some(Rc::new(TypeProfiler::default()));
        }
        self.type_profiler_enabled_count.set(self.type_profiler_enabled_count.get() + 1);
        needs_to_recompile
    }

    /// `disableTypeProfiler()`.
    pub fn disable_type_profiler(&self) -> bool {
        let count = self.type_profiler_enabled_count.get();
        assert!(count > 0);
        self.type_profiler_enabled_count.set(count - 1);
        if count == 1 {
            *self.type_profiler.borrow_mut() = None;
        }
        count == 1
    }

    /// `enableControlFlowProfiler()`.
    pub fn enable_control_flow_profiler(&self) -> bool {
        let needs_to_recompile = self.control_flow_profiler_enabled_count.get() == 0;
        if needs_to_recompile {
            *self.control_flow_profiler.borrow_mut() = Some(Rc::new(ControlFlowProfiler::default()));
        }
        self.control_flow_profiler_enabled_count.set(self.control_flow_profiler_enabled_count.get() + 1);
        needs_to_recompile
    }

    /// `disableControlFlowProfiler()`.
    pub fn disable_control_flow_profiler(&self) -> bool {
        let count = self.control_flow_profiler_enabled_count.get();
        assert!(count > 0);
        self.control_flow_profiler_enabled_count.set(count - 1);
        if count == 1 {
            *self.control_flow_profiler.borrow_mut() = None;
        }
        count == 1
    }

    /// `Heap::collectNow`/`collectSync` (a API Rust que o `Bun.gc` do oráculo usaria): coleta completa e
    /// devolve quantas células foram varridas. Fora do interpretador a pilha de registradores não é raiz.
    pub fn collect_garbage(&self) -> usize {
        crate::runtime::gc::collect(None, self)
    }

    /// As raízes que o `VM` guarda por `cell_id` (`VM::visitAggregate`): as sentinelas de iteração rápida e
    /// a fila de microtarefas.
    pub(crate) fn visit_gc_roots(&self, marker: &mut crate::runtime::gc::Marker) {
        for sentinel in [&self.fast_async_generator_sentinel, &self.fast_array_values_sentinel] {
            if let Some(value) = sentinel.get() {
                marker.value(*value);
            }
        }
        if let Some(s) = self.fast_iteration_sentinels.get() {
            marker.values(&[
                s.array_keys, s.array_entries, s.map_keys, s.map_values,
                s.map_entries, s.set_values, s.set_entries, s.string_values,
            ]);
        }
        self.default_microtask_queue.visit_children(marker);
        // `m_asyncContextData`: o contexto assíncrono corrente (a cabeça da lista de `Frame`s do `AsyncLocalStorage`).
        marker.value(self.async_context.get());
    }

    /// `currentWeakRefVersion()`.
    pub fn current_weak_ref_version(&self) -> u64 {
        self.current_weak_ref_version.get()
    }

    /// `finalizeSynchronousJSExecution()`: fim do trabalho síncrono e das microtarefas de uma tarefa; os
    /// `WeakRef`s lidos até aqui deixam de segurar o alvo (o `ClearKeptObjects` da especificação).
    pub fn finalize_synchronous_js_execution(&self) {
        self.current_weak_ref_version.set(self.current_weak_ref_version.get() + 1);
    }

    /// `setFailNextNewCodeBlock()`.
    pub fn set_fail_next_new_code_block(&self) {
        self.fail_next_new_code_block.set(true);
    }

    /// `getAndClearFailNextNewCodeBlock()`.
    pub fn get_and_clear_fail_next_new_code_block(&self) -> bool {
        self.fail_next_new_code_block.replace(false)
    }

    /// `setGlobalConstRedeclarationShouldThrow(bool)`.
    pub fn set_global_const_redeclaration_should_throw(&self, should_throw: bool) {
        self.global_const_redeclaration_should_throw.set(should_throw);
    }

    /// `globalConstRedeclarationShouldThrow()`.
    pub fn global_const_redeclaration_should_throw(&self) -> bool {
        self.global_const_redeclaration_should_throw.get()
    }

    /// `setAllowRedeclaringSymbols(bool)`.
    pub fn set_allow_redeclaring_symbols(&self, allow: bool) {
        self.allow_redeclaring_symbols.set(allow);
    }

    /// `allowRedeclaringSymbols()`.
    pub fn allow_redeclaring_symbols(&self) -> bool {
        self.allow_redeclaring_symbols.get()
    }

    /// `stringStructure`.
    pub fn string_structure(&self) -> crate::runtime::structure::StructureRef {
        Rc::clone(self.string_structure.get().expect("VM::stringStructure lido antes de ser criada"))
    }

    /// `propertyNameEnumeratorStructure`.
    pub fn property_name_enumerator_structure(&self) -> crate::runtime::structure::StructureRef {
        Rc::clone(self.property_name_enumerator_structure.get().expect("VM::propertyNameEnumeratorStructure lido antes de ser criada"))
    }

    /// `bigIntStructure`.
    pub fn big_int_structure(&self) -> crate::runtime::structure::StructureRef {
        Rc::clone(self.big_int_structure.get().expect("VM::bigIntStructure lido antes de ser criada"))
    }

    /// `templateObjectDescriptorStructure`.
    pub fn template_object_descriptor_structure(&self) -> crate::runtime::structure::StructureRef {
        Rc::clone(self.template_object_descriptor_structure.get().expect("VM::templateObjectDescriptorStructure lido antes de ser criada"))
    }

    /// As estruturas de `JSCellButterfly` (`rawImmutableButterflyStructure`, `cellButterflyOnlyAtomStringsStructure`).
    pub(crate) fn cell_butterfly_structures(&self) -> &crate::runtime::js_cell_butterfly::CellButterflyStructures {
        self.cell_butterfly_structures.get().expect("VM::rawImmutableButterflyStructure lido antes de ser criada")
    }

    /// `symbolTableStructure`.
    pub fn symbol_table_structure(&self) -> crate::runtime::structure::StructureRef {
        Rc::clone(self.symbol_table_structure.get().expect("VM::symbolTableStructure lido antes de ser criada"))
    }

    /// `orderedHashTableSentinel()`: o `JSCell*` como `cell_id` (o que `JSValue::from_cell` guarda).
    pub fn ordered_hash_table_sentinel(&self) -> usize {
        self.ordered_hash_table_sentinel.get().expect("VM::orderedHashTableSentinel lido antes de ser criado").cell_id()
    }

    /// `fastAsyncGeneratorSentinel()`: a célula como `JSValue`.
    pub fn fast_async_generator_sentinel(&self) -> JSValue {
        *self.fast_async_generator_sentinel.get().expect("VM::fastAsyncGeneratorSentinel lido antes de ser criado")
    }

    /// `fastArrayValuesSentinel()`: a célula como `JSValue`.
    pub fn fast_array_values_sentinel(&self) -> JSValue {
        *self.fast_array_values_sentinel.get().expect("VM::fastArrayValuesSentinel lido antes de ser criado")
    }

    fn fast_iteration_sentinels(&self) -> &FastIterationSentinels {
        self.fast_iteration_sentinels.get().expect("VM::fast*Sentinel lido antes de ser criado")
    }

    /// `fastArrayKeysSentinel()`.
    pub fn fast_array_keys_sentinel(&self) -> JSValue {
        self.fast_iteration_sentinels().array_keys
    }

    /// `fastArrayEntriesSentinel()`.
    pub fn fast_array_entries_sentinel(&self) -> JSValue {
        self.fast_iteration_sentinels().array_entries
    }

    /// `fastMapKeysSentinel()`.
    pub fn fast_map_keys_sentinel(&self) -> JSValue {
        self.fast_iteration_sentinels().map_keys
    }

    /// `fastMapValuesSentinel()`.
    pub fn fast_map_values_sentinel(&self) -> JSValue {
        self.fast_iteration_sentinels().map_values
    }

    /// `fastMapEntriesSentinel()`.
    pub fn fast_map_entries_sentinel(&self) -> JSValue {
        self.fast_iteration_sentinels().map_entries
    }

    /// `fastSetValuesSentinel()`.
    pub fn fast_set_values_sentinel(&self) -> JSValue {
        self.fast_iteration_sentinels().set_values
    }

    /// `fastSetEntriesSentinel()`.
    pub fn fast_set_entries_sentinel(&self) -> JSValue {
        self.fast_iteration_sentinels().set_entries
    }

    /// `fastStringValuesSentinel()`.
    pub fn fast_string_values_sentinel(&self) -> JSValue {
        self.fast_iteration_sentinels().string_values
    }

    /// O teste `JSCell::m_type == SentinelType` do `.asm` de `op_iterator_next` sobre o `next`, restrito às
    /// sentinelas do protocolo síncrono (a do gerador assíncrono nunca chega em `op_iterator_next`).
    pub fn is_fast_iteration_sentinel(&self, value: JSValue) -> bool {
        let sentinels = self.fast_iteration_sentinels();
        value == self.fast_array_values_sentinel()
            || [
                sentinels.array_keys,
                sentinels.array_entries,
                sentinels.map_keys,
                sentinels.map_values,
                sentinels.map_entries,
                sentinels.set_values,
                sentinels.set_entries,
                sentinels.string_values,
            ]
            .contains(&value)
    }

    /// `incrementModuleAsyncEvaluationCount()`: devolve o valor anterior (`m_moduleAsyncEvaluationCount++`).
    pub fn increment_module_async_evaluation_count(&self) -> i64 {
        let count = self.module_async_evaluation_count.get();
        self.module_async_evaluation_count.set(count + 1);
        count
    }

    /// `atomStringToJSStringMap`.
    pub fn atom_string_to_js_string_map(&self) -> &crate::runtime::weak_gc_map::AtomStringToJSStringMap {
        &self.atom_string_to_js_string_map
    }

    /// `m_compactVariableMap`: privado no C++ (o `BytecodeGenerator` é `friend` do `VM`), daí o acessor.
    pub fn compact_variable_map(&self) -> &Rc<crate::parser::variable_environment::CompactTDZEnvironmentMap> {
        &self.compact_variable_map
    }

    /// `codeCache()`.
    pub fn code_cache(&self) -> &crate::runtime::code_cache::CodeCache {
        &self.code_cache
    }

    /// `heap` (membro público do C++).
    pub fn heap(&self) -> &crate::runtime::heap::Heap {
        &self.heap
    }

    /// `builtinExecutables()`.
    pub fn builtin_executables(&self) -> &crate::runtime::builtin_executables::BuiltinExecutables {
        &self.builtin_executables
    }

    /// `bytecodeIntrinsicRegistry()`.
    pub fn bytecode_intrinsic_registry(&self) -> &BytecodeIntrinsicRegistry {
        self.bytecode_intrinsic_registry.get_or_init(|| {
            let builtin_names = self.property_names.builtin_names();
            BytecodeIntrinsicRegistry::new(|name| builtin_names.look_up_private_name(name.as_bytes())?.impl_())
        })
    }

    /// `addSourceProviderCache(SourceProvider*)`.
    pub fn add_source_provider_cache(
        &self,
        source_provider: &Rc<dyn crate::parser::source_provider::SourceProvider>,
    ) -> Rc<RefCell<crate::parser::source_provider_cache::SourceProviderCache>> {
        let key = Rc::as_ptr(source_provider) as *const () as usize;
        let mut map = self.source_provider_cache_map.borrow_mut();
        let entry = map.entry(key).or_insert_with(|| {
            let length = source_provider.source().length();
            (
                source_provider.clone(),
                Rc::new(RefCell::new(crate::parser::source_provider_cache::SourceProviderCache::create(length))),
            )
        });
        entry.1.clone()
    }

    /// `clearSourceProviderCaches()`.
    pub fn clear_source_provider_caches(&self) {
        self.source_provider_cache_map.borrow_mut().clear();
    }

    /// `propertyNames` (o `CommonIdentifiers*`): o campo `property_names` faz `Deref`, este é o
    /// acessor que o gerador de bytecode chama.
    pub fn property_names(&self) -> &CommonIdentifiers {
        &self.property_names
    }

    /// `isSafeToRecurse()`: `isSafeToRecurse(m_stackLimit)` mais o orçamento lógico.
    ///
    /// Dois testes independentes: (1) a pilha nativa da thread (`m_stackLimit`, de `set_thread_stack_budget`), que
    /// só existe para o processo não cair nativamente e nunca deve ser o que decide em teste; (2) a pilha lógica
    /// (`logical_stack_limit`), a medida fiel ao C++: `maxPerThreadStackUsage - reservedZoneSize` em bytes dos
    /// frames que o C++ teria, somados pelas recursões que chamam `enter_logical_frame`.
    pub fn is_safe_to_recurse(&self) -> bool {
        self.stack_used() < VM::logical_stack_limit() && self.is_safe_to_recurse_with(self.stack_limit.get())
    }

    /// `Options::maxPerThreadStackUsage() - Options::reservedZoneSize()`: o que `StackBounds::recursionLimit(start,
    /// maxUserStack, reservedZoneSize)` deixa de pilha de usuário a partir da entrada no VM (VM.cpp `updateStackLimits`).
    pub fn logical_stack_limit() -> usize {
        let max_user_stack = crate::runtime::options::Options::max_per_thread_stack_usage() as usize;
        let reserved_zone = (crate::runtime::options::Options::reserved_zone_size() as usize).min(max_user_stack);
        max_user_stack - reserved_zone
    }

    /// A pilha única do C++, em bytes: os registradores da `CLoopStack` até o `topCallFrame` (que já incluem o custo
    /// nativo de cada entrada no VM, ver `enter_vm`) mais o que a pilha lógica gastou desde a última entrada. No bun
    /// os frames do LLInt e os do C++ dividem a mesma pilha nativa e o mesmo limite.
    pub fn stack_used(&self) -> usize {
        let top = self.top_call_frame.get();
        let js_bytes = if top == 0 {
            0
        } else {
            crate::interpreter::cloop_stack::CLoopStack::reservation_registers().saturating_sub(top) * size_of::<crate::runtime::js_value::EncodedJSValue>()
        };
        js_bytes + (self.logical_stack_used.get() - self.logical_entry_mark.get())
    }

    /// Entrada no VM (`doVMEntry`): devolve quantos bytes de `CLoopStack` o frame de entrada deve pular, que são os
    /// frames nativos que o C++ empilha entre o chamador e o trampolim (`stack_cost::PROGRAM_ENTRY` na primeira
    /// entrada; vindo de função nativa, `stack_cost::VM_REENTRY` para `vmEntryToJavaScript` e
    /// `stack_cost::VM_NATIVE_REENTRY` para `vmEntryToNative`) mais a pilha lógica gasta desde a entrada anterior. A
    /// marca volta ao valor antigo quando o `VMEntryMark` morre.
    pub fn enter_vm(&self, kind: VMEntryKind) -> (usize, VMEntryMark) {
        let cost = match (self.top_call_frame.get() == 0, kind) {
            (true, _) => stack_cost::PROGRAM_ENTRY,
            (false, VMEntryKind::JavaScript) => stack_cost::VM_REENTRY,
            (false, VMEntryKind::Native) => stack_cost::VM_NATIVE_REENTRY,
            (false, VMEntryKind::LLIntCall) => 0,
        };
        let used = self.logical_stack_used.get();
        let previous = self.logical_entry_mark.replace(used);
        (cost + (used - previous), VMEntryMark { mark: Rc::clone(&self.logical_entry_mark), previous })
    }

    /// Entra num frame de recursão nativa de `cost` bytes lógicos (ver `stack_cost`). `None` quando o orçamento
    /// lógico ou a pilha nativa acabou: o chamador responde como o C++ responde a `!isSafeToRecurse()`. O frame
    /// sai da conta quando o `LogicalStackFrame` morre, em qualquer caminho de saída.
    pub fn enter_logical_frame(&self, cost: usize) -> Option<LogicalStackFrame> {
        let used = self.logical_stack_used.get();
        if self.stack_used().saturating_add(cost) > VM::logical_stack_limit() || !self.is_safe_to_recurse_with(self.stack_limit.get()) {
            return None;
        }
        self.logical_stack_used.set(used + cost);
        Some(LogicalStackFrame { used: Rc::clone(&self.logical_stack_used), cost })
    }

    /// `isSafeToRecurse(void* stackLimit)`: o endereço de uma local aproxima o ponteiro de pilha
    /// (`currentStackPointer()`).
    pub fn is_safe_to_recurse_with(&self, stack_limit: usize) -> bool {
        let marker = 0u8;
        (&marker as *const u8 as usize) >= stack_limit
    }

    /// `stackLimit()`.
    pub fn stack_limit(&self) -> usize {
        self.stack_limit.get()
    }

    /// `m_stackLimit = ...`: o `updateStackLimits()` calcula o valor com `StackBounds::recursionLimit`,
    /// que depende dos limites reais da thread (não portados); o chamador passa o resultado.
    pub fn set_stack_limit(&self, limit: usize) {
        self.stack_limit.set(limit);
    }

    /// `m_stackPointerAtVMEntry`.
    pub fn stack_pointer_at_vm_entry(&self) -> usize {
        self.stack_pointer_at_vm_entry.get()
    }

    /// `setStackPointerAtVMEntry(void*)`: grava o ponteiro; o `updateStackLimits()` fica a cargo de quem
    /// conhece o `StackBounds`.
    pub fn set_stack_pointer_at_vm_entry(&self, sp: usize) {
        self.stack_pointer_at_vm_entry.set(sp);
    }

    /// `isEntered()`: `!!entryScope`.
    pub fn is_entered(&self) -> bool {
        self.entry_scope.get() != 0
    }

    /// `topCallFrame`.
    pub fn top_call_frame(&self) -> usize {
        self.top_call_frame.get()
    }

    /// `topCallFrame = callFrame`.
    pub fn set_top_call_frame(&self, call_frame: usize) {
        self.top_call_frame.set(call_frame);
    }

    /// Troca a chamada JS em curso para uma função nativa (ver `native_call_site`) e devolve a anterior,
    /// que quem entrou no nativo restaura ao sair.
    pub fn replace_native_call_site(
        &self,
        site: Option<(crate::bytecode::code_block::CodeBlockRef, crate::bytecode::bytecode_index::BytecodeIndex)>,
    ) -> Option<(crate::bytecode::code_block::CodeBlockRef, crate::bytecode::bytecode_index::BytecodeIndex)> {
        self.native_call_site.replace(site)
    }

    /// A chamada JS em curso para uma função nativa, se há uma.
    pub fn native_call_site(
        &self,
    ) -> Option<(crate::bytecode::code_block::CodeBlockRef, crate::bytecode::bytecode_index::BytecodeIndex)> {
        self.native_call_site.borrow().clone()
    }

    /// Marca se o nativo em curso foi chamado em tail call (ver `native_call_tail`) e devolve a marca anterior,
    /// que quem entrou no nativo restaura ao sair.
    pub fn replace_native_call_tail(&self, tail: bool) -> bool {
        self.native_call_tail.replace(tail)
    }

    /// O nativo em curso foi chamado em tail call: o frame logo abaixo dele já foi trocado.
    pub fn native_call_tail(&self) -> bool {
        self.native_call_tail.get()
    }

    /// `topEntryFrame`.
    pub fn top_entry_frame(&self) -> usize {
        self.top_entry_frame.get()
    }

    /// `topEntryFrame = entryFrame`.
    pub fn set_top_entry_frame(&self, entry_frame: usize) {
        self.top_entry_frame.set(entry_frame);
    }

    /// `interpreter` (membro do `VM` no C++): a alça do interpretador único do `VM`. Cada chamada
    /// devolve uma alça nova sobre a mesma pilha, `sp` e tabela de `CodeBlock`s, então o laço em
    /// execução e uma chamada reentrante (getter, setter, callback nativo) usam o mesmo estado.
    pub fn interpreter(&self) -> crate::interpreter::interpreter::Interpreter {
        self.interpreter.0.get_or_init(crate::interpreter::interpreter::Interpreter::new).clone()
    }

    /// `lastException()`.
    pub fn last_exception(&self) -> Option<Rc<Exception>> {
        self.last_exception.borrow().clone()
    }

    /// `clearLastException()`.
    pub fn clear_last_exception(&self) {
        *self.last_exception.borrow_mut() = None;
    }

    /// O que `~VM` desfaz com `heap.lastChanceToFinalize()`: tudo o que o `VM` segura por `Rc` e que alcança de
    /// volta o `JSGlobalObject` (e por ele o próprio `VM`). No C++ esses elos são arestas do GC ou ponteiros
    /// crus não-donos; aqui cada um é um ciclo `Rc`. São eles: `m_exception`/`m_lastException`/
    /// `m_terminationException` (a pilha capturada guarda `CodeBlock`), `native_call_site` (`CodeBlock`) e a
    /// tabela de `CodeBlock`s do `Interpreter`. Depois disto, soltos o global e os locais do programa, o
    /// `strong_count` do `VM` chega a zero.
    pub fn last_chance_to_finalize(&self) {
        *self.exception.borrow_mut() = None;
        *self.last_exception.borrow_mut() = None;
        *self.termination_exception.borrow_mut() = None;
        *self.native_call_site.borrow_mut() = None;
        self.source_provider_cache_map.borrow_mut().clear();
        if let Some(interpreter) = self.interpreter.0.get() {
            interpreter.finalize_code_blocks();
        }
    }

    /// `exceptionForInspection()`.
    pub fn exception_for_inspection(&self) -> Option<Rc<Exception>> {
        self.exception()
    }

    /// `softStackLimit()`.
    pub fn soft_stack_limit(&self) -> usize {
        self.soft_stack_limit.get()
    }

    /// `setSoftStackLimit` (a parte que grava `m_softStackLimit`).
    pub fn set_soft_stack_limit(&self, limit: usize) {
        self.soft_stack_limit.set(limit);
    }

    /// `m_executingRegExp`, como identidade (0 é `nullptr`).
    pub fn executing_reg_exp(&self) -> usize {
        self.executing_reg_exp.get()
    }

    /// `AsyncContextSwapScope::current(vm, globalObject)`: o contexto assíncrono corrente, `undefined` quando o
    /// rastreamento está desligado ou não há contexto.
    pub fn async_context(&self) -> JSValue {
        if self.async_context_tracking.get() {
            self.async_context.get()
        } else {
            JSValue::undefined()
        }
    }

    /// `AsyncContextFrame::withAsyncContextIfNeeded`: o contexto que um callback agendado agora deve reinstalar ao
    /// rodar, ou `empty` quando o rastreamento está desligado (o callback roda sob o contexto de quem o chama).
    pub fn captured_async_context(&self) -> JSValue {
        if self.async_context_tracking.get() {
            self.async_context.get()
        } else {
            JSValue::empty()
        }
    }

    /// `$putInternalField($asyncContext, 0, context)`.
    pub fn set_async_context(&self, context: JSValue) {
        self.async_context.set(context);
    }

    /// `vm.isAsyncContextTrackingEnabled()`.
    pub fn async_context_tracking_enabled(&self) -> bool {
        self.async_context_tracking.get()
    }

    /// `vm.setAsyncContextTrackingEnabled(enabled)` (`jsSetAsyncHooksEnabled`).
    pub fn set_async_context_tracking(&self, enabled: bool) {
        self.async_context_tracking.set(enabled);
    }

    /// `m_executingRegExp = regExp`.
    pub fn set_executing_reg_exp(&self, reg_exp: usize) {
        self.executing_reg_exp.set(reg_exp);
    }

    /// `symbolRegistry()`.
    pub fn symbol_registry(&self) -> &SymbolRegistry {
        &self.symbol_registry
    }

    /// `dateCache`: o fuso é o do processo (`TZ`), resolvido na primeira leitura.
    /// Se o fuso mudou desde a última leitura (`Emulation.setTimezoneOverride`, por exemplo), os caches são
    /// limpos antes de devolvê-lo: o `hasTimeZoneChange` que o JSC confere na entrada do `VM`.
    pub fn date_cache(&self) -> &crate::runtime::js_date_math::DateCache {
        let cache = self.date_cache.0.get_or_init(|| {
            crate::runtime::js_date_math::DateCache::new(Box::new(crate::runtime::process_time_zone::ProcessTimeZone::new()))
        });
        if cache.has_time_zone_change() {
            cache.clear_for_time_zone_change();
        }
        cache
    }

    /// `privateSymbolRegistry()`.
    pub fn private_symbol_registry(&self) -> &SymbolRegistry {
        &self.private_symbol_registry
    }

    /// `exception()`.
    pub fn exception(&self) -> Option<Rc<Exception>> {
        self.exception.borrow().clone()
    }

    /// `exception()` sem o clone do `Rc`: o teste barato de exceção pendente dos laços de despacho.
    pub fn has_exception(&self) -> bool {
        self.exception.borrow().is_some()
    }

    /// `clearException()`. O `traps().clearTrap(NeedExceptionHandling)` fica para o `VMTraps`.
    pub fn clear_exception(&self) {
        *self.exception.borrow_mut() = None;
    }

    /// `ensureTerminationException()`.
    pub fn ensure_termination_exception(&self) -> Rc<Exception> {
        // O C++ cria `Exception::create(*this, TerminatedExecutionError::create(*this))`; sem essa
        // célula, o valor é o vazio, e a identidade é o que `isTerminationException` compara.
        Rc::clone(self.termination_exception.borrow_mut().get_or_insert_with(|| Exception::create(self, JSValue::empty())))
    }

    /// `isTerminationException(Exception*)`.
    pub fn is_termination_exception(&self, exception: &Rc<Exception>) -> bool {
        self.termination_exception.borrow().as_ref().is_some_and(|termination| Rc::ptr_eq(termination, exception))
    }

    /// `hasPendingTerminationException()`.
    pub fn has_pending_termination_exception(&self) -> bool {
        self.exception().is_some_and(|exception| self.is_termination_exception(&exception))
    }

    /// `VM::throwException(JSGlobalObject*, Exception*)`: o `notifyDebuggerOfExceptionToBeThrown`, o
    /// `breakOnThrow` e a captura de pilha de verificação ficam para o `Interpreter`.
    pub fn throw_exception(&self, _global_object: &crate::runtime::js_global_object::JSGlobalObject, exception_to_throw: Rc<Exception>) -> Rc<Exception> {
        // The TerminationException should never be overridden.
        if self.has_pending_termination_exception() {
            return self.exception().expect("terminação pendente sem exceção");
        }
        self.set_exception(Rc::clone(&exception_to_throw));
        exception_to_throw
    }

    /// `m_exception = exception`, o que `VM::throwException` faz no fim.
    pub fn set_exception(&self, exception: Rc<Exception>) {
        // `m_exception = exception; m_lastException = exception;` (o `fireTrap(NeedExceptionHandling)` é do `VMTraps`).
        *self.last_exception.borrow_mut() = Some(Rc::clone(&exception));
        *self.exception.borrow_mut() = Some(exception);
    }

    /// `traps().deferTermination(...)`: só o contador; o caminho lento (`deferTerminationSlow`)
    /// pertence ao `VMTraps`.
    fn defer_termination(&self) {
        self.defer_termination_count.set(self.defer_termination_count.get() + 1);
    }

    /// `traps().undoDeferTermination(...)`.
    fn undo_defer_termination(&self) {
        debug_assert!(self.defer_termination_count.get() > 0);
        self.defer_termination_count.set(self.defer_termination_count.get() - 1);
    }
}

/// `class DeferTermination<DeferUntilEndOfScope>`: o `~DeferTermination` desfaz no `Drop`.
pub struct DeferTermination<'vm> {
    vm: &'vm VM,
}

impl<'vm> DeferTermination<'vm> {
    pub fn new(vm: &'vm VM) -> DeferTermination<'vm> {
        vm.defer_termination();
        DeferTermination { vm }
    }
}

impl Drop for DeferTermination<'_> {
    fn drop(&mut self) {
        self.vm.undo_defer_termination();
    }
}

/// `class TopExceptionScope` (build sem `EXCEPTION_SCOPE_VERIFICATION`, que é o caso do
/// `ExceptionScope` simples: só guarda o `VM&`).
pub struct TopExceptionScope<'vm> {
    vm: &'vm VM,
}

impl<'vm> TopExceptionScope<'vm> {
    pub fn new(vm: &'vm VM) -> TopExceptionScope<'vm> {
        TopExceptionScope { vm }
    }

    /// `exception()`.
    pub fn exception(&self) -> Option<Rc<Exception>> {
        self.vm.exception()
    }

    /// `assertNoException()`.
    pub fn assert_no_exception(&self) {
        debug_assert!(self.exception().is_none());
    }

    /// `releaseAssertNoException()`.
    pub fn release_assert_no_exception(&self) {
        assert!(self.exception().is_none());
    }

    /// `assertNoExceptionExceptTermination()`.
    pub fn assert_no_exception_except_termination(&self) {
        debug_assert!(self.exception().is_none() || self.vm.has_pending_termination_exception());
    }

    /// `releaseAssertNoExceptionExceptTermination()`.
    pub fn release_assert_no_exception_except_termination(&self) {
        assert!(self.exception().is_none() || self.vm.has_pending_termination_exception());
    }

    /// `clearException()`.
    pub fn clear_exception(&self) {
        self.vm.clear_exception();
    }

    /// `clearExceptionExceptTermination()`.
    pub fn clear_exception_except_termination(&self) -> bool {
        if self.vm.has_pending_termination_exception() {
            return false;
        }
        self.vm.clear_exception();
        true
    }
}
