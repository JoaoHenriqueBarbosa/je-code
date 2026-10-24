//! Tradução de `runtime/CyclicModuleRecord.{h,cpp}`, `JSModuleRecord.{h,cpp}` e do que
//! `AbstractModuleRecord.cpp` tem de `getModuleNamespace`, `innerModuleLinking` e `innerModuleEvaluation`:
//! o ciclo de vida de um módulo de código-fonte (instanciar o ambiente, ligar o grafo, avaliar).
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//!
//! - As três classes (`AbstractModuleRecord` > `CyclicModuleRecord` > `JSModuleRecord`) são um só
//!   `Rc<AbstractModuleRecord>`: o que as subclasses acrescentam mora em [`ModuleRecordExt`] (campo `ext`),
//!   porque a `Resolution`, o `JSModuleEnvironment` e o registro de células apontam para a base e o
//!   `dynamicDowncast` do C++ vira `ext.js_module()`. Só existe `JSModuleRecord` como `CyclicModuleRecord`
//!   (`is_cyclic()`); o `SyntheticModuleRecord` (JSON, Text) é o registro sem `JSModuleData`
//!   (`synthetic_module_record.rs`), então o `dynamicDowncast<CyclicModuleRecord>` das regras de
//!   `AbstractModuleRecord.cpp` (`innerModuleLinking`, `innerModuleEvaluation`, `evaluate`) é `is_cyclic()`.
//!   `WebAssemblyModuleRecord` não existe.
//! - `JSModuleRecord` é uma célula do registro central (`CellEntry::ModuleRecord`): o corpo do módulo
//!   recebe o registro como `JSValue` (`Interpreter::executeModuleProgram`).
//! - Erros: a exceção fica pendente no `VM` como no C++ e a função devolve `Err(Thrown::Pending)`; a
//!   lacuna do porte é `Err(Thrown::Unported)`. `JSModuleLoader::attachErrorInfo` (o `ModuleFailure` que
//!   o inspetor lê) não existe.
//! - Top-level await e dependência assíncrona: `executeAsync`, `gatherAvailableAncestors`,
//!   `asyncExecutionFulfilled`/`Rejected` e `JSModuleRecord::execute(capability)` seguem o C++; as tarefas
//!   `AsyncModuleExecutionDone` e `AsyncModuleExecutionResume` e o `asyncModuleResolveEvaluation` são de
//!   `js_microtask.rs` (como em `JSMicrotask.cpp`). O corpo do módulo com `await` suspende e retoma pelo
//!   mesmo `evaluate_body` (estado em `Field::State`).
//! - `import defer`: `gatherAsynchronousTransitiveDependencies`, `readyForSyncExecution` e `evaluateSync`
//!   estão aqui; `ensureDeferredNamespaceEvaluation` é de `js_module_namespace_object.rs`. O
//!   `DynamicImportDefer*` do carregador assíncrono do C++ não existe (o carregamento é síncrono).
//! - `referrerAsyncOrder` e `dynamicImportPromise` (`USE(BUN_JSC_ADDITIONS)`, só para o deadlock de
//!   async) existem, com `importPromiseGatesAsyncDependency`. Sem o `InternalFieldTuple` do contexto
//!   assíncrono do Bun (`AsyncContextSwapScope` não existe), o `unwrapContext` do C++ é a identidade.
//! - `initializeEnvironment` não cria o `ScriptFetcher` (`createImportMetaProperties` recebe a chave do
//!   módulo e a máquina de `ModuleHost` em `js_module_loader.rs`). As mensagens de `SyntaxError` são as da
//!   ramificação `USE(BUN_JSC_ADDITIONS)` (a ligada por `cmakeconfig.h`).
//! - O profiler de tipos e de fluxo de controle (`functionHasExecutedCache()->insertUnexecutedRange`) não
//!   roda: ele só existe com esses profilers ligados.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use crate::bytecode::unlinked_function_executable::UnlinkedFunctionExecutable;
use crate::llint::slow_paths::throw_stack_overflow_error;
use crate::llint::LLIntFailure;
use crate::parser::parser_modes::{
    is_async_function_wrapper_parse_mode, is_async_generator_wrapper_parse_mode, is_generator_wrapper_parse_mode, CodeFeatures,
    IMPORT_META_FEATURE,
};
use crate::parser::source_code::SourceCode;
use crate::parser::source_provider::SourceProviderSourceType;
use crate::runtime::abstract_module_record::{
    AbstractModuleRecord, AbstractModuleRecordRef, AsyncEvaluationOrder, ExportEntryType, Field, ImportEntryType, ModulePhase,
    ModuleRequest, Resolution, ResolutionType, ResumeMode, State, UniquedStringKey,
};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::current_realm::try_current_global_object;
use crate::runtime::error::{create_syntax_error, create_type_error};
use crate::runtime::host_call::Thrown;
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_async_function::JSAsyncFunction;
use crate::runtime::js_async_generator_function::JSAsyncGeneratorFunction;
use crate::runtime::js_function::JSFunction;
use crate::runtime::js_generator_function::JSGeneratorFunction;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_environment::{JSModuleEnvironment, JSModuleEnvironmentRef};
use crate::runtime::js_module_loader::create_import_meta_properties;
use crate::runtime::js_module_namespace_object::{JSModuleNamespaceObject, JSModuleNamespaceObjectRef};
use crate::runtime::js_promise::{JSPromise, JSPromiseRef, Status as PromiseStatus};
use crate::runtime::js_microtask::{async_module_resolve_evaluation, take_pending_exception};
use crate::runtime::microtask::InternalMicrotask;
use crate::runtime::js_async_function_generator::JSAsyncFunctionGenerator;
use crate::runtime::js_async_from_sync_iterator::JSAsyncFromSyncIterator;
use crate::runtime::js_promise_combinators_context::JSPromiseCombinatorsGlobalContext;
use crate::runtime::js_promise_reaction::JSPromiseReactionRef;
use crate::runtime::js_promise_host::PromiseHost;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_symbol_table_object::symbol_table_put;
use crate::runtime::js_value::{js_number_i32, js_tdz_value, JSValue};
use crate::runtime::module_program_executable::ModuleProgramExecutable;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::script_fetch_parameters::ScriptFetchParametersType;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::wtf::text::wtf_string::String as WtfString;

/// `CyclicModuleRecord::Status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    New,
    Unlinked,
    Linking,
    Linked,
    Evaluating,
    EvaluatingAsync,
    Evaluated,
}

/// O desfecho das operações de módulo: `Err(Thrown::Pending)` é a exceção pendente no `VM`.
pub type ModuleResult<T> = Result<T, Thrown>;

/// O que `JSModuleRecord` guarda além da base: `m_sourceCode`, `m_features` e o executável do corpo.
pub struct JSModuleData {
    pub source_code: SourceCode,
    pub features: CodeFeatures,
    /// `m_moduleProgramExecutable` (`clear()` depois que o corpo termina).
    executable: RefCell<Option<Rc<RefCell<ModuleProgramExecutable>>>>,
}

/// Os campos de `CyclicModuleRecord` e `JSModuleRecord` e os caches de namespace do
/// `AbstractModuleRecord`.
#[derive(Default)]
pub struct ModuleRecordExt {
    status: Cell<Status>,
    /// `m_evaluationError` (`None` é o `JSValue()` vazio).
    evaluation_error: RefCell<Option<JSValue>>,
    dfs_ancestor_index: Cell<u32>,
    initialized: Cell<bool>,
    /// `m_cycleRoot`.
    cycle_root: RefCell<Option<AbstractModuleRecordRef>>,
    /// `m_topLevelCapability`.
    top_level_capability: RefCell<Option<JSPromiseRef>>,
    /// `m_asyncCapability` (`AbstractModuleRecord`): a promessa do `await` de topo em andamento.
    async_capability: RefCell<Option<JSPromiseRef>>,
    /// Presente só em `JSModuleRecord`.
    js: OnceCell<JSModuleData>,
    /// `m_moduleNamespaceObject`.
    pub(crate) namespace: RefCell<Option<JSModuleNamespaceObjectRef>>,
    /// `m_deferredNamespaceObject`.
    deferred_namespace: RefCell<Option<JSModuleNamespaceObjectRef>>,
    /// O id do registro central de células (0 antes do registro).
    cell_id: Cell<usize>,
}

/// `JSModuleRecord::create(globalObject, vm, structure, moduleKey, sourceCode, features)`.
pub fn create_js_module_record(module_key: Identifier, source_code: SourceCode, features: CodeFeatures) -> AbstractModuleRecordRef {
    let record = AbstractModuleRecord::create(module_key, SourceProviderSourceType::Module);
    let data = JSModuleData { source_code, features, executable: RefCell::new(None) };
    if record.ext.js.set(data).is_err() {
        unreachable!("JSModuleData já definido");
    }
    register_module_record(&record);
    record
}

/// Registra o registro de módulo como célula (`CellEntry::ModuleRecord`): o `JSValue` que o corpo do
/// módulo e as tarefas de microtask recebem. Vale para o `JSModuleRecord` e o `SyntheticModuleRecord`.
pub(crate) fn register_module_record(record: &AbstractModuleRecordRef) {
    let cell_id = cell_registry::insert(CellEntry::ModuleRecord(Rc::clone(record)));
    record.ext.cell_id.set(cell_id);
}

/// `throwSyntaxError(globalObject, scope, message)`.
pub(crate) fn throw_syntax_error(global_object: &JSGlobalObject, message: &str) -> Thrown {
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, create_syntax_error(global_object, &WtfString::from_utf8(message.as_bytes())));
    Thrown::Pending
}

/// `RETURN_IF_EXCEPTION`.
fn pending_or_ok(global_object: &JSGlobalObject) -> ModuleResult<()> {
    if global_object.vm().exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(())
}

/// A lacuna ou a exceção do interpretador como o desfecho do módulo.
fn thrown_from_llint(failure: LLIntFailure) -> Thrown {
    match failure {
        LLIntFailure::Thrown => Thrown::Pending,
        LLIntFailure::Unported(what) => Thrown::Unported(what),
        LLIntFailure::UnportedOpcode(_) => Thrown::Unported("opcode sem handler no corpo do módulo"),
    }
}

/// `checkSafeToRecurse(globalObject, scope)`.
fn check_safe_to_recurse(global_object: &JSGlobalObject) -> ModuleResult<()> {
    if !global_object.vm().is_safe_to_recurse() {
        throw_stack_overflow_error(global_object);
        return Err(Thrown::Pending);
    }
    Ok(())
}

/// O realm do porte (um só): o corrente, ou o primeiro `JSGlobalObject` registrado quando nenhum
/// `CurrentRealmScope` está aberto (a ligação roda fora do `vmEntryToJavaScript`).
pub(crate) fn realm() -> crate::runtime::js_global_object::JSGlobalObjectRef {
    try_current_global_object().or_else(cell_registry::first_global_object).expect("módulo sem nenhum JSGlobalObject")
}

fn name_of(identifier: &Identifier) -> String {
    String::from_utf8_lossy(&identifier.utf8()).into_owned()
}

/// Conjunto de nomes que lembra a ordem de inserção (o `IdentifierSet` ordenado do C++ só precisa de
/// `add`/`contains`/iteração).
#[derive(Default)]
struct NameSet {
    order: Vec<UniquedStringKey>,
}

impl NameSet {
    fn add(&mut self, key: &UniquedStringKey) {
        if !self.contains(key) {
            self.order.push(key.clone());
        }
    }

    fn contains(&self, key: &UniquedStringKey) -> bool {
        self.order.contains(key)
    }
}

impl AbstractModuleRecord {
    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.ext.cell_id.get())
    }

    /// O registro pelo `cell_id` de um `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<AbstractModuleRecordRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::ModuleRecord(record)) => Some(record),
            _ => None,
        }
    }

    /// `dynamicDowncast<JSModuleRecord>(this)`.
    pub fn js_module(&self) -> Option<&JSModuleData> {
        self.ext.js.get()
    }

    /// `JSModuleRecord::sourceCode()`.
    pub fn source_code(&self) -> SourceCode {
        self.js_module().expect("registro que não é JSModuleRecord").source_code.clone()
    }

    /// `JSModuleRecord::features()`.
    pub fn features(&self) -> CodeFeatures {
        self.js_module().expect("registro que não é JSModuleRecord").features
    }

    /// `CyclicModuleRecord::status()`.
    pub fn status(&self) -> Status {
        self.ext.status.get()
    }

    /// `setStatus(newStatus)`.
    pub fn set_status(&self, status: Status) {
        self.ext.status.set(status);
    }

    /// `evaluationError()`.
    pub fn evaluation_error(&self) -> Option<JSValue> {
        self.ext.evaluation_error.borrow().clone()
    }

    /// `cycleRoot()`.
    pub fn cycle_root(&self) -> Option<AbstractModuleRecordRef> {
        self.ext.cycle_root.borrow().clone()
    }

    /// `JSModuleRecord::isTopLevelExecutionFinished()`.
    pub fn is_top_level_execution_finished(&self) -> bool {
        let state = self.internal_field(Field::State);
        !state.is_number() || state.as_int32() == State::Executing as i32
    }

    /// `dynamicDowncast<CyclicModuleRecord>(this)`: só o `JSModuleRecord` é cíclico (ver o cabeçalho).
    pub fn is_cyclic(&self) -> bool {
        self.js_module().is_some()
    }

    /// `CyclicModuleRecord::isSCCEvaluated()`.
    pub fn is_scc_evaluated(&self) -> bool {
        // 1. If module.[[CycleRoot]] is not EMPTY, then
        //   1.a. If module.[[CycleRoot]].[[Status]] is EVALUATED, return true.
        //   1.b. Return false.
        if let Some(root) = self.cycle_root() {
            return root.status() == Status::Evaluated;
        }
        // 2. If module.[[Status]] is EVALUATED, return true.
        // 3. Return false.
        self.status() == Status::Evaluated
    }

    /// `setEvaluationError(vm, error)`.
    fn set_evaluation_error(&self, error: JSValue) {
        *self.ext.evaluation_error.borrow_mut() = Some(error);
    }

    /// `setCycleRoot(vm, newRoot)`.
    fn set_cycle_root(&self, new_root: &AbstractModuleRecordRef) {
        *self.ext.cycle_root.borrow_mut() = Some(Rc::clone(new_root));
    }

    /// `topLevelCapability()`.
    pub fn top_level_capability(&self) -> Option<JSPromiseRef> {
        self.ext.top_level_capability.borrow().clone()
    }

    /// `asyncCapability()`.
    pub fn async_capability(&self) -> Option<JSPromiseRef> {
        self.ext.async_capability.borrow().clone()
    }

    /// `asyncCapability(vm, promise)`.
    fn set_async_capability(&self, promise: &JSPromiseRef) {
        *self.ext.async_capability.borrow_mut() = Some(Rc::clone(promise));
    }

    /// `JSModuleLoader::getImportedModule(referrer, request)` (GetImportedModule): o módulo carregado
    /// para `request`, que `LoadRequestedModules` já garantiu.
    pub fn get_imported_module(&self, request: &ModuleRequest) -> AbstractModuleRecordRef {
        self.host_resolve_imported_module(&request.specifier, request.type_(ScriptFetchParametersType::JavaScript))
            .expect("módulo requisitado não carregado")
    }

    /// `getOrMakeExecutable(globalObject)`: `None` com exceção pendente.
    pub fn get_or_make_executable(&self, global_object: &JSGlobalObject) -> Option<Rc<RefCell<ModuleProgramExecutable>>> {
        let data = self.js_module().expect("registro que não é JSModuleRecord");
        if let Some(executable) = data.executable.borrow().as_ref() {
            return Some(Rc::clone(executable));
        }
        let executable = ModuleProgramExecutable::try_create(global_object, &data.source_code)?;
        *data.executable.borrow_mut() = Some(Rc::clone(&executable));
        Some(executable)
    }

    /// `symbolTablePutTouchWatchpointSet(env, globalObject, name, value, false, true, putResult)`.
    pub(crate) fn put_binding(environment: &JSModuleEnvironmentRef, name: &Identifier, value: JSValue) {
        if let Some(key) = name.impl_() {
            symbol_table_put(&**environment, &key, value, false, true);
        }
    }

    /// O `*namespace*` do ambiente, preenchido com o objeto de namespace (`setModuleEnvironment` e
    /// `getModuleNamespace`).
    pub(crate) fn put_star_namespace(&self, environment: &JSModuleEnvironmentRef, namespace: &JSModuleNamespaceObjectRef) {
        let global_object = realm();
        let name = global_object.vm().property_names.star_namespace_private_name.clone();
        AbstractModuleRecord::put_binding(environment, &name, namespace.as_value());
    }

    /// `AbstractModuleRecord::getModuleNamespace(globalObject, phase, shouldPreventExtensions)`.
    pub fn get_module_namespace(
        self: &Rc<Self>,
        global_object: &JSGlobalObject,
        phase: ModulePhase,
        should_prevent_extensions: bool,
    ) -> ModuleResult<JSModuleNamespaceObjectRef> {
        let vm = global_object.vm();

        // Sem asserção de status: num ciclo, a ligação de `b` resolve `import { n }` para o `export * as n from './c.mjs'` de
        // `a` e cria o namespace de `c` antes de `c` ser visitado (ainda `New`). Só os nomes estáticos de `c` importam aqui.

        if phase == ModulePhase::Defer {
            if let Some(namespace) = self.ext.deferred_namespace.borrow().as_ref() {
                return Ok(Rc::clone(namespace));
            }
        } else if let Some(namespace) = self.ext.namespace.borrow().as_ref() {
            return Ok(Rc::clone(namespace));
        }

        // Step 1 (spec): the unique resolved bindings, the names that need the slow `resolveExport`, and
        // the names the root shadows.
        let mut unique_bindings: Vec<(UniquedStringKey, Resolution)> = Vec::new();
        let mut root_shadowed_names = NameSet::default();
        let mut slow_path_names = NameSet::default();
        let mut resolutions: Vec<(Identifier, Resolution)> = Vec::new();

        let mut export_star_set: Vec<AbstractModuleRecordRef> = Vec::new();
        let mut pending_modules: Vec<AbstractModuleRecordRef> = vec![Rc::clone(self)];

        while let Some(module_record) = pending_modules.pop() {
            if export_star_set.iter().any(|seen| Rc::ptr_eq(seen, &module_record)) {
                continue;
            }
            export_star_set.push(Rc::clone(&module_record));
            let is_root = Rc::ptr_eq(&module_record, self);

            for (export_key, export_entry) in module_record.export_entries().iter() {
                if is_root {
                    root_shadowed_names.add(export_key);
                } else {
                    if vm.property_names.default_keyword == export_entry.export_name {
                        continue;
                    }
                    if root_shadowed_names.contains(export_key) || slow_path_names.contains(export_key) {
                        continue;
                    }
                }

                let candidate = match export_entry.type_ {
                    ExportEntryType::Local => Resolution {
                        type_: ResolutionType::Resolved,
                        module_record: Some(Rc::clone(&module_record)),
                        local_name: export_entry.local_name.clone(),
                    },
                    ExportEntryType::Namespace => {
                        let imported = module_record
                            .host_resolve_imported_module(&export_entry.module_name, export_entry.module_request_type)
                            .expect("módulo do export de namespace não carregado");
                        Resolution {
                            type_: ResolutionType::Resolved,
                            module_record: Some(imported),
                            local_name: vm.property_names.star_namespace_private_name.clone(),
                        }
                    }
                    ExportEntryType::Indirect => {
                        if !is_root {
                            unique_bindings.retain(|(key, _)| key != export_key);
                        }
                        slow_path_names.add(export_key);
                        continue;
                    }
                };

                if is_root {
                    resolutions.push((export_entry.export_name.clone(), candidate));
                    continue;
                }

                match unique_bindings.iter().position(|(key, _)| key == export_key) {
                    None => unique_bindings.push((export_key.clone(), candidate)),
                    Some(position) => {
                        if !unique_bindings[position].1.is_same_binding(&candidate) {
                            slow_path_names.add(export_key);
                            unique_bindings.remove(position);
                        }
                    }
                }
            }

            for (star_module_name, star_module_request_type) in module_record.star_export_entries().iter() {
                let requested = module_record
                    .host_resolve_imported_module(&Identifier::from_uid(vm, star_module_name.as_ref()), *star_module_request_type)
                    .expect("módulo do export * não carregado");
                pending_modules.push(requested);
            }
        }

        for (key, resolution) in &unique_bindings {
            self.cache_resolution(key, resolution);
            resolutions.push((Identifier::from_uid(vm, key.as_ref()), resolution.clone()));
        }

        for name in &slow_path_names.order {
            let ident = Identifier::from_uid(vm, name.as_ref());
            let resolution = self.resolve_export(global_object, &ident);
            match resolution.type_ {
                ResolutionType::NotFound => {
                    if self.is_type_script.get() {
                        continue;
                    }
                    return Err(throw_syntax_error(global_object, &format!("Exported binding name '{}' is not found.", name_of(&ident))));
                }
                ResolutionType::Error => {
                    return Err(throw_syntax_error(
                        global_object,
                        "Exported binding name 'default' cannot be resolved by star export entries.",
                    ));
                }
                ResolutionType::Ambiguous => {}
                ResolutionType::Resolved => resolutions.push((ident, resolution)),
            }
        }

        let namespace = JSModuleNamespaceObject::create(
            global_object,
            self,
            resolutions,
            should_prevent_extensions,
            phase == ModulePhase::Defer,
        );

        if phase == ModulePhase::Defer {
            *self.ext.deferred_namespace.borrow_mut() = Some(Rc::clone(&namespace));
            return Ok(namespace);
        }

        if let Some(environment) = self.module_environment_may_be_null() {
            self.put_star_namespace(&environment, &namespace);
        }
        *self.ext.namespace.borrow_mut() = Some(Rc::clone(&namespace));
        Ok(namespace)
    }

    /// `CyclicModuleRecord::initializeEnvironment(globalObject, scriptFetcher)`.
    pub fn initialize_environment(self: &Rc<Self>, global_object: &JSGlobalObject) -> ModuleResult<()> {
        if self.ext.initialized.get() {
            return Ok(());
        }
        let result = self.initialize_environment_impl(global_object);
        if !self.ext.initialized.get() {
            self.clear_module_environment();
        }
        result
    }

    fn initialize_environment_impl(self: &Rc<Self>, global_object: &JSGlobalObject) -> ModuleResult<()> {
        let vm = global_object.vm();

        // 1. For each ExportEntry Record e of module.[[IndirectExportEntries]], do
        let indirect: Vec<_> =
            self.export_entries().iter().filter(|(_, e)| e.type_ == ExportEntryType::Indirect).map(|(_, e)| e.clone()).collect();
        for entry in &indirect {
            let resolution = self.resolve_export(global_object, &entry.export_name);
            match resolution.type_ {
                ResolutionType::NotFound => {
                    if self.is_type_script.get() {
                        continue;
                    }
                    return Err(throw_syntax_error(
                        global_object,
                        &format!("export '{}' not found in '{}'", name_of(&entry.export_name), name_of(&entry.module_name)),
                    ));
                }
                ResolutionType::Ambiguous => {
                    return Err(throw_syntax_error(
                        global_object,
                        &format!(
                            "Cannot export '{}' multiple times in '{}'",
                            name_of(&entry.export_name),
                            name_of(&entry.module_name)
                        ),
                    ));
                }
                ResolutionType::Error => {
                    return Err(throw_syntax_error(global_object, "export default cannot be used with export *"));
                }
                ResolutionType::Resolved => {}
            }
        }

        // 5. Let env be NewModuleEnvironment(realm.[[GlobalEnv]]).
        let executable = self.get_or_make_executable(global_object).ok_or(Thrown::Pending)?;
        let symbol_table =
            executable.borrow().module_environment_symbol_table().expect("ModuleProgramExecutable sem SymbolTable do ambiente");
        let environment = JSModuleEnvironment::create_for_global_object(
            vm,
            global_object,
            Some(JSScopeRef::GlobalLexicalEnvironment(global_object.global_lexical_environment())),
            symbol_table.clone(),
            js_tdz_value(),
            Some(Rc::clone(self)),
        );
        // 6. Set module.[[Environment]] to env.
        self.set_module_environment(Rc::clone(&environment));

        // 7. For each ImportEntry Record in of module.[[ImportEntries]], do
        let imports: Vec<_> = self.import_entries().iter().map(|(_, entry)| entry.clone()).collect();
        for import in &imports {
            let imported_module = self
                .host_resolve_imported_module(&import.module_request, import.module_request_type)
                .ok_or_else(|| {
                    throw_syntax_error(global_object, &format!("Importing module '{}' is not found.", name_of(&import.module_request)))
                })?;
            if import.type_ == ImportEntryType::Namespace {
                let namespace = imported_module.get_module_namespace(global_object, import.phase, true)?;
                AbstractModuleRecord::put_binding(&environment, &import.local_name, namespace.as_value());
                continue;
            }

            let resolution = imported_module.resolve_export(global_object, &import.import_name);
            match resolution.type_ {
                ResolutionType::NotFound => {
                    if import.type_ == ImportEntryType::SingleTypeScript {
                        continue;
                    }
                    if !(import.local_name.is_null() || import.local_name.is_private_name() || import.local_name.is_symbol()) {
                        let other = imported_module.resolve_export(global_object, &vm.property_names.default_keyword);
                        if other.type_ == ResolutionType::Resolved && other.local_name == import.local_name {
                            return Err(throw_syntax_error(
                                global_object,
                                &format!(
                                    "Export named '{}' not found in module '{}'. Did you mean to import default?",
                                    name_of(&import.import_name),
                                    name_of(imported_module.module_key())
                                ),
                            ));
                        }
                    }
                    return Err(throw_syntax_error(
                        global_object,
                        &format!(
                            "Export named '{}' not found in module '{}'.",
                            name_of(&import.import_name),
                            name_of(imported_module.module_key())
                        ),
                    ));
                }
                ResolutionType::Ambiguous => {
                    return Err(throw_syntax_error(
                        global_object,
                        &format!(
                            "Export named '{}' cannot be resolved due to ambiguous multiple bindings in module '{}'.",
                            name_of(&import.import_name),
                            name_of(imported_module.module_key())
                        ),
                    ));
                }
                ResolutionType::Error => {
                    if !(import.local_name.is_null() || import.local_name.is_private_name() || import.local_name.is_symbol()) {
                        let other = imported_module.resolve_export(global_object, &import.local_name);
                        if other.type_ == ResolutionType::Resolved {
                            return Err(throw_syntax_error(
                                global_object,
                                &format!(
                                    "module '{}' does not have an export named 'default'. Did you mean '{}'?",
                                    name_of(imported_module.module_key()),
                                    name_of(&import.local_name)
                                ),
                            ));
                        }
                    }
                    return Err(throw_syntax_error(
                        global_object,
                        &format!("Missing 'default' export in module '{}'.", name_of(imported_module.module_key())),
                    ));
                }
                ResolutionType::Resolved => {
                    // 7.c.iii. If resolution.[[BindingName]] is NAMESPACE, then
                    if vm.property_names.star_namespace_private_name == resolution.local_name {
                        let target = resolution.module_record.as_ref().expect("Resolution resolvida sem registro");
                        let namespace = target.get_module_namespace(global_object, ModulePhase::Evaluation, true)?;
                        AbstractModuleRecord::put_binding(&environment, &import.local_name, namespace.as_value());
                    }
                    // 7.c.iv. Else: CreateImportBinding is handled through lazy resolution
                    // (`JSModuleEnvironment::symbol_table_get`).
                }
            }
        }

        // 18. Let code be module.[[ECMAScriptCode]].
        let unlinked_code_block = executable.borrow().unlinked_code_block().cloned().expect("ModuleProgramExecutable sem UnlinkedCodeBlock");
        let unlinked_base = unlinked_code_block.borrow().base_ref();
        // 21. For each element d of varDeclarations, do ... initialize "var" and "function" with undefined.
        for (key, _) in unlinked_code_block.borrow().variable_declarations().iter() {
            let entry = symbol_table.borrow().get(key);
            if entry.is_null() || entry.var_offset().is_stack() {
                continue;
            }
            symbol_table_put(&*environment, key, JSValue::undefined(), false, true);
        }

        // 24. For each element d of lexDeclarations, do ... InstantiateFunctionObject.
        let source = executable.borrow().source().clone();
        let number_of_functions = unlinked_base.borrow().number_of_function_decls();
        for index in 0..number_of_functions {
            let unlinked_function = Rc::clone(unlinked_base.borrow().function_decl(index));
            let name = unlinked_function.borrow().name();
            let Some(name_key) = name.impl_() else { continue };
            let entry = symbol_table.borrow().get(&name_key);
            if entry.is_null() || entry.var_offset().is_stack() {
                continue;
            }
            let function_executable = UnlinkedFunctionExecutable::link(
                &unlinked_function,
                vm,
                Some(ScriptExecutableRef::ModuleProgram(Rc::clone(&executable))),
                &source,
                None,
                Intrinsic::NoIntrinsic,
                false,
            );
            let parse_mode = function_executable.borrow().parse_mode();
            let scope = JSScopeRef::ModuleEnvironment(Rc::clone(&environment));
            let function = if is_async_generator_wrapper_parse_mode(parse_mode) {
                JSAsyncGeneratorFunction::create(vm, global_object, &function_executable, scope)
            } else if is_generator_wrapper_parse_mode(parse_mode) {
                JSGeneratorFunction::create(vm, global_object, &function_executable, scope)
            } else if is_async_function_wrapper_parse_mode(parse_mode) {
                JSAsyncFunction::create(vm, global_object, &function_executable, scope)
            } else {
                JSFunction::create(vm, global_object, &function_executable, scope)
            };
            pending_or_ok(global_object)?;
            AbstractModuleRecord::put_binding(&environment, &name, JSValue::from_cell(function.cell_id()));
        }

        if self.features() & IMPORT_META_FEATURE != 0 {
            let meta = create_import_meta_properties(global_object, self.module_key())?;
            AbstractModuleRecord::put_binding(&environment, &vm.property_names.meta_private_name, meta);
        }

        self.ext.initialized.set(true);
        Ok(())
    }

    /// `AbstractModuleRecord::innerModuleLinking(globalObject, stack, index, scriptFetcher)`.
    fn inner_module_linking(
        self: &Rc<Self>,
        global_object: &JSGlobalObject,
        stack: &mut Vec<AbstractModuleRecordRef>,
        index: u32,
    ) -> ModuleResult<u32> {
        // 1. If module is not a Cyclic Module Record, then
        if !self.is_cyclic() {
            // 1.a. Perform ? module.Link().
            self.link(global_object)?;
            // 1.b. Return index.
            return Ok(index);
        }
        match self.status() {
            Status::Linking | Status::Linked | Status::Evaluating | Status::EvaluatingAsync | Status::Evaluated => {
                return Ok(index);
            }
            _ => {}
        }
        debug_assert!(self.status() == Status::Unlinked);
        self.set_status(Status::Linking);
        let module_index = index;
        self.ext.dfs_ancestor_index.set(index);
        let mut index = index + 1;
        stack.push(Rc::clone(self));

        let requests: Vec<_> = self.requested_modules().iter().cloned().collect();
        for request in &requests {
            let required = self.get_imported_module(request);
            check_safe_to_recurse(global_object)?;
            index = required.inner_module_linking(global_object, stack, index)?;
            if required.is_cyclic() && required.status() == Status::Linking {
                self.ext
                    .dfs_ancestor_index
                    .set(self.ext.dfs_ancestor_index.get().min(required.ext.dfs_ancestor_index.get()));
            }
        }

        self.initialize_environment(global_object)?;

        debug_assert!(self.ext.dfs_ancestor_index.get() <= module_index);
        if self.ext.dfs_ancestor_index.get() == module_index {
            loop {
                let required = stack.pop().expect("pilha de ligação vazia");
                required.set_status(Status::Linked);
                if Rc::ptr_eq(&required, self) {
                    break;
                }
            }
        }
        Ok(index)
    }

    /// `AbstractModuleRecord::link(globalObject, scriptFetcher)`: o `CyclicModuleRecord::link`, ou o
    /// `SyntheticModuleRecord::link` (nada a fazer, `Synchronousness::Sync`).
    pub fn link(self: &Rc<Self>, global_object: &JSGlobalObject) -> ModuleResult<()> {
        if !self.is_cyclic() {
            return Ok(());
        }
        debug_assert!(matches!(self.status(), Status::Unlinked | Status::Linked | Status::EvaluatingAsync | Status::Evaluated));
        let mut stack: Vec<AbstractModuleRecordRef> = Vec::new();
        match self.inner_module_linking(global_object, &mut stack, 0) {
            Ok(_) => {
                debug_assert!(stack.is_empty());
                Ok(())
            }
            Err(error) => {
                for module in &stack {
                    debug_assert!(module.status() == Status::Linking);
                    module.set_status(Status::Unlinked);
                }
                Err(error)
            }
        }
    }

    /// `JSModuleRecord::evaluate(globalObject, sentValue, resumeMode)`: roda o corpo do módulo.
    pub fn evaluate_body(
        self: &Rc<Self>,
        global_object: &JSGlobalObject,
        sent_value: JSValue,
        resume_mode: JSValue,
    ) -> ModuleResult<JSValue> {
        let data = self.js_module().expect("registro que não é JSModuleRecord");
        let Some(executable) = data.executable.borrow().clone() else {
            // "Can't evaluate a JSModuleRecord that has no executable".
            return Ok(JSValue::undefined());
        };
        if let Some(error) = self.evaluation_error() {
            let mut scope = ThrowScope::new(global_object.vm());
            throw_exception(global_object, &mut scope, error);
            return Err(Thrown::Pending);
        }

        let global = realm();
        let environment = self.module_environment();
        let result = global_object
            .vm()
            .interpreter()
            .execute_module_program(self, &executable, &global, &environment, sent_value, resume_mode)
            .map_err(thrown_from_llint)?;
        pending_or_ok(global_object)?;

        if self.is_top_level_execution_finished() {
            *data.executable.borrow_mut() = None;
        }
        Ok(result)
    }

    /// `JSModuleRecord::execute(globalObject, capability)`.
    fn execute(self: &Rc<Self>, global_object: &JSGlobalObject, capability: Option<&JSPromiseRef>) -> ModuleResult<()> {
        // ExecuteModule([capability])
        // https://tc39.es/ecma262/#sec-source-text-module-record-execute-module
        // 5. Assert: module has been linked and declarations in its module environment have been instantiated.
        debug_assert!(self.status() as u8 >= Status::Linked as u8);
        let resume_mode = js_number_i32(ResumeMode::NormalMode as i32);
        // 9. If module.[[HasTLA]] is false, then
        if !self.has_tla() {
            // 9.a. Assert: capability is not present.
            debug_assert!(capability.is_none());
            // 9.c. Let result be Completion(Evaluation of module.[[ECMAScriptCode]]).
            // 9.f. If result is an abrupt completion, then 9.f.i. Return ? result.
            self.evaluate_body(global_object, JSValue::undefined(), resume_mode)?;
            return Ok(());
        }
        // 10. Else,
        // 10.a. Assert: capability is a PromiseCapability Record.
        let capability = capability.expect("execute de módulo com top-level await sem capability");
        // 10.b. Perform AsyncBlockStart(capability, module.[[ECMAScriptCode]], moduleContext).
        self.set_async_capability(capability);
        let result = self.evaluate_body(global_object, JSValue::undefined(), resume_mode);
        async_module_resolve_evaluation(global_object, self, result)
    }

    /// `CyclicModuleRecord::executeAsync(globalObject)`.
    fn execute_async(self: &Rc<Self>, global_object: &JSGlobalObject) -> ModuleResult<()> {
        // ExecuteAsyncModule(module)
        // https://tc39.es/ecma262/#sec-execute-async-module
        // 1. Assert: module.[[Status]] is either EVALUATING or EVALUATING-ASYNC.
        debug_assert!(self.status() == Status::Evaluating || self.status() == Status::EvaluatingAsync);
        // 2. Assert: module.[[HasTLA]] is true.
        debug_assert!(self.has_tla());
        // 3. Let capability be ! NewPromiseCapability(%Promise%).
        let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
        // 4. to 8. fulfilledClosure/rejectedClosure e PerformPromiseThen: a tarefa interna
        // `AsyncModuleExecutionDone` (JSMicrotask.cpp) chama `asyncExecutionFulfilled`/`Rejected`.
        promise.perform_promise_then_with_internal_microtask(
            global_object,
            InternalMicrotask::AsyncModuleExecutionDone,
            None,
            self.as_value(),
            JSValue::empty(),
        );
        // 9. Perform ! module.ExecuteModule(capability).
        self.execute(global_object, Some(&promise))
    }

    /// `CyclicModuleRecord::asyncExecutionRejected(globalObject, error)`.
    pub fn async_execution_rejected(self: &Rc<Self>, error: JSValue) {
        // AsyncModuleExecutionRejected(module, error)
        // https://tc39.es/ecma262/#sec-async-module-execution-rejected
        let global_object = realm();
        // The spec specifies this as a recursion, but we use a worklist loop to avoid stack overflow crashes.
        let mut stack: Vec<AbstractModuleRecordRef> = vec![Rc::clone(self)];
        while let Some(module) = stack.pop() {
            // 1. If module.[[Status]] is EVALUATED, then 1.a. Assert 1.b. Return UNUSED.
            if module.status() == Status::Evaluated {
                debug_assert!(module.evaluation_error().is_some());
                continue;
            }
            // 2. to 4. Asserts.
            debug_assert!(module.status() == Status::EvaluatingAsync);
            debug_assert!(module.async_evaluation_order().has_order());
            debug_assert!(module.evaluation_error().is_none());
            // 5. Set module.[[EvaluationError]] to ThrowCompletion(error).
            module.set_evaluation_error(error);
            // 6. Set module.[[Status]] to EVALUATED.
            module.set_status(Status::Evaluated);
            // 7. Set module.[[AsyncEvaluationOrder]] to DONE.
            module.set_async_evaluation_order(AsyncEvaluationOrder::done());
            // 9. If module.[[TopLevelCapability]] is not EMPTY, then
            if let Some(top_level) = module.top_level_capability() {
                // 9.a. Assert: module.[[CycleRoot]] and module are the same Module Record.
                debug_assert!(module.cycle_root().is_some_and(|root| Rc::ptr_eq(&root, &module)));
                // 9.b. Perform ! Call(module.[[TopLevelCapability]].[[Reject]], undefined, « error »).
                top_level.reject(&*global_object, error);
            }
            // 10. For each Cyclic Module Record m of module.[[AsyncParentModules]],
            //     10.a. Perform AsyncModuleExecutionRejected(m, error).
            for parent in module.async_parent_modules().iter().rev() {
                stack.push(Rc::clone(parent));
            }
        }
    }

    /// `CyclicModuleRecord::asyncExecutionFulfilled(globalObject)`.
    pub fn async_execution_fulfilled(self: &Rc<Self>) -> ModuleResult<()> {
        // AsyncModuleExecutionFulfilled(module)
        // https://tc39.es/ecma262/#sec-async-module-execution-fulfilled
        let global_object = realm();
        // 1. If module.[[Status]] is EVALUATED, then 1.a. Assert 1.b. Return UNUSED.
        if self.status() == Status::Evaluated {
            debug_assert!(self.evaluation_error().is_some());
            return Ok(());
        }
        // 2. to 4. Asserts.
        debug_assert!(self.status() == Status::EvaluatingAsync);
        debug_assert!(self.async_evaluation_order().has_order());
        debug_assert!(self.evaluation_error().is_none());
        // 5. Set module.[[AsyncEvaluationOrder]] to DONE.
        self.set_async_evaluation_order(AsyncEvaluationOrder::done());
        // 6. Set module.[[Status]] to EVALUATED.
        self.set_status(Status::Evaluated);
        // 7. If module.[[TopLevelCapability]] is not EMPTY, then
        if let Some(capability) = self.top_level_capability() {
            // 7.a. Assert: module.[[CycleRoot]] and module are the same Module Record.
            debug_assert!(self.cycle_root().is_some_and(|root| Rc::ptr_eq(&root, self)));
            // 7.b. Perform ! Call(module.[[TopLevelCapability]].[[Resolve]], undefined, « undefined »).
            capability.fulfill(&*global_object, JSValue::undefined());
        }
        // 8. Let execList be a new empty List.
        let mut exec_list: Vec<AbstractModuleRecordRef> = Vec::new();
        // 9. Perform GatherAvailableAncestors(module, execList).
        gather_available_ancestors(self, &mut exec_list);
        // 10. Assert: All elements of execList have their [[AsyncEvaluationOrder]] field set to an integer,
        // [[PendingAsyncDependencies]] field set to 0, and [[EvaluationError]] field set to EMPTY.
        debug_assert!(exec_list.iter().all(|element| {
            element.async_evaluation_order().has_order()
                && element.pending_async_dependencies() == Some(0)
                && element.evaluation_error().is_none()
        }));
        // 11. Let sortedExecList be a List whose elements are the elements of execList, sorted by their
        // [[AsyncEvaluationOrder]] field in ascending order.
        exec_list.sort_by_key(|element| element.async_evaluation_order().order());
        // 12. For each Cyclic Module Record m of sortedExecList, do
        for m in &exec_list {
            // 12.a. If m.[[Status]] is EVALUATED, then
            if m.status() == Status::Evaluated {
                // 12.a.i. Assert: m.[[EvaluationError]] is not EMPTY.
                debug_assert!(m.evaluation_error().is_some());
            // 12.b. Else if m.[[HasTLA]] is true, then
            } else if m.has_tla() {
                // 12.b.i. Perform ExecuteAsyncModule(m).
                if let Err(error) = m.execute_async(&global_object) {
                    m.reject_after_failed_execution(&global_object, error)?;
                }
            // 12.c. Else,
            } else {
                // 12.c.i. Let result be m.ExecuteModule().
                // 12.c.ii. If result is an abrupt completion, then
                if let Err(error) = m.execute(&global_object, None) {
                    // 12.c.ii.1. Perform AsyncModuleExecutionRejected(m, result.[[Value]]).
                    m.reject_after_failed_execution(&global_object, error)?;
                // 12.c.iii. Else,
                } else {
                    // 12.c.iii.1. Set m.[[AsyncEvaluationOrder]] to DONE.
                    m.set_async_evaluation_order(AsyncEvaluationOrder::done());
                    // 12.c.iii.2. Set m.[[Status]] to EVALUATED.
                    m.set_status(Status::Evaluated);
                    // 12.c.iii.3. If m.[[TopLevelCapability]] is not EMPTY, then
                    if let Some(capability) = m.top_level_capability() {
                        // 12.c.iii.3.a. Assert: m.[[CycleRoot]] and m are the same Module Record.
                        debug_assert!(m.cycle_root().is_some_and(|root| Rc::ptr_eq(&root, m)));
                        // 12.c.iii.3.b. Perform ! Call(m.[[TopLevelCapability]].[[Resolve]], undefined, « undefined »).
                        capability.fulfill(&*global_object, JSValue::undefined());
                    }
                }
            }
        }
        // 13. Return UNUSED.
        Ok(())
    }

    /// O `if (Exception* exception = scope.exception()) { error = exception->value(); TRY_CLEAR_EXCEPTION;
    /// m->asyncExecutionRejected(globalObject, error); }` de `asyncExecutionFulfilled`: uma exceção
    /// pendente rejeita o módulo; a lacuna do porte sobe.
    fn reject_after_failed_execution(self: &Rc<Self>, global_object: &JSGlobalObject, failure: Thrown) -> ModuleResult<()> {
        match failure {
            Thrown::Pending => {
                let error = take_pending_exception(global_object);
                self.async_execution_rejected(error);
                Ok(())
            }
            other => Err(other),
        }
    }

    /// `AbstractModuleRecord::innerModuleEvaluation(globalObject, stack, index, referrerAsyncOrder,
    /// dynamicImportPromise)`.
    fn inner_module_evaluation(
        self: &Rc<Self>,
        global_object: &JSGlobalObject,
        stack: &mut Vec<AbstractModuleRecordRef>,
        index: u32,
        referrer_async_order: i64,
        dynamic_import_promise: Option<&JSPromiseRef>,
    ) -> ModuleResult<u32> {
        // InnerModuleEvaluation(module, stack, index)
        // https://tc39.es/ecma262/#sec-innermoduleevaluation
        let vm = global_object.vm();

        // 1. If module is not a Cyclic Module Record, then
        if !self.is_cyclic() {
            // 1.a. Perform ? EvaluateModuleSync(module).
            self.evaluate_module_sync();
            // 1.b. Return index.
            return Ok(index);
        }
        // 2. If module.[[Status]] is either EVALUATING-ASYNC or EVALUATED, then
        if matches!(self.status(), Status::EvaluatingAsync | Status::Evaluated) {
            // 2.a. If module.[[EvaluationError]] is EMPTY, return index.
            // 2.b. Otherwise, return ? module.[[EvaluationError]].
            return match self.evaluation_error() {
                None => Ok(index),
                Some(error) => {
                    let mut scope = ThrowScope::new(vm);
                    throw_exception(global_object, &mut scope, error);
                    Err(Thrown::Pending)
                }
            };
        }
        // 3. If module.[[Status]] is EVALUATING, return index.
        if self.status() == Status::Evaluating {
            return Ok(index);
        }
        // 4. Assert: module.[[Status]] is LINKED.
        debug_assert!(self.status() == Status::Linked);

        // 5. Set module.[[Status]] to EVALUATING.
        self.set_status(Status::Evaluating);
        // 6. Let moduleIndex be index.
        let module_index = index;
        // 7. Set module.[[DFSAncestorIndex]] to index.
        self.ext.dfs_ancestor_index.set(index);
        // 8. Set module.[[PendingAsyncDependencies]] to 0.
        self.set_pending_async_dependencies(Some(0));
        // 9. Set index to index + 1.
        let mut index = index + 1;
        // 10. Append module to stack.
        stack.push(Rc::clone(self));

        // https://tc39.es/proposal-defer-import-eval/#sec-innermoduleevaluation
        // 10. Let evaluationList be a new empty List.
        let mut evaluation_list: Vec<AbstractModuleRecordRef> = Vec::new();
        // 11. For each ModuleRequest Record request of module.[[RequestedModules]], do
        for request in self.requested_modules().iter() {
            // 11.a. Let requiredModule be GetImportedModule(module, request).
            let required = self.get_imported_module(request);
            // 11.b. If request.[[Phase]] is defer, then
            if request.phase == ModulePhase::Defer {
                // 11.b.i. Let additionalModules be GatherAsynchronousTransitiveDependencies(requiredModule).
                // 11.b.ii. For each Module Record additionalModule of additionalModules, do
                //   11.b.ii.1. If evaluationList does not contain additionalModule, then append it.
                let mut seen: Vec<AbstractModuleRecordRef> = Vec::new();
                required.gather_asynchronous_transitive_dependencies(&mut evaluation_list, &mut seen);
            } else {
                // 11.c. Else if evaluationList does not contain requiredModule, then
                //   11.c.i. Append requiredModule to evaluationList.
                push_unique(&mut evaluation_list, required);
            }
        }
        // 12. For each Module Record requiredModule of evaluationList, do
        for required in &evaluation_list {
            check_safe_to_recurse(global_object)?;
            // 12.a. Set index to ? InnerModuleEvaluation(requiredModule, stack, index).
            index = required.inner_module_evaluation(global_object, stack, index, referrer_async_order, dynamic_import_promise)?;
            // 12.b. If requiredModule is a Cyclic Module Record, then
            if !required.is_cyclic() {
                continue;
            }
            // Bun extension: require(esm) can re-enter innerModuleEvaluation while an outer DFS is already
            // evaluating one of our transitive deps. That outer module is Evaluating but lives on the OUTER
            // stack, not the local one: the outer DFS owns its lifecycle, so it is a satisfied dependency.
            let dep_in_outer_scc =
                required.status() == Status::Evaluating && !stack.iter().any(|member| Rc::ptr_eq(member, required));
            if dep_in_outer_scc {
                continue;
            }
            // 12.b.i. Assert: requiredModule.[[Status]] is one of EVALUATING, EVALUATING-ASYNC, or EVALUATED.
            debug_assert!(matches!(required.status(), Status::Evaluating | Status::EvaluatingAsync | Status::Evaluated));
            // 12.b.ii. Assert: requiredModule.[[Status]] is EVALUATING if and only if stack contains requiredModule.
            debug_assert!(stack.iter().any(|member| Rc::ptr_eq(member, required)) == (required.status() == Status::Evaluating));
            let mut cyclic = Rc::clone(required);
            // 12.b.iii. If requiredModule.[[Status]] is EVALUATING, then
            if cyclic.status() == Status::Evaluating {
                // 12.b.iii.1. Set module.[[DFSAncestorIndex]] to min(module.[[DFSAncestorIndex]],
                // requiredModule.[[DFSAncestorIndex]]).
                self.ext.dfs_ancestor_index.set(self.ext.dfs_ancestor_index.get().min(cyclic.ext.dfs_ancestor_index.get()));
            // 12.b.iv. Else,
            } else {
                // 12.b.iv.1. Set requiredModule to requiredModule.[[CycleRoot]].
                cyclic = required.cycle_root().expect("módulo concluído sem CycleRoot");
                // 12.b.iv.2. Assert: requiredModule.[[Status]] is either EVALUATING-ASYNC or EVALUATED.
                debug_assert!(matches!(cyclic.status(), Status::EvaluatingAsync | Status::Evaluated));
                // 12.b.iv.3. If requiredModule.[[EvaluationError]] is not empty, return ? requiredModule.[[EvaluationError]].
                if let Some(error) = cyclic.evaluation_error() {
                    let mut scope = ThrowScope::new(vm);
                    throw_exception(global_object, &mut scope, error);
                    return Err(Thrown::Pending);
                }
            }
            // 12.b.v. If requiredModule.[[AsyncEvaluationOrder]] is an integer, then
            if cyclic.async_evaluation_order().has_order() {
                // referrerAsyncOrder covers an import() whose promise reaches the suspended referrer only
                // through native code, where the walk cannot follow.
                let deadlocks = cyclic.async_evaluation_order().order() == referrer_async_order
                    || dynamic_import_promise.is_some_and(|promise| import_promise_gates_async_dependency(promise, &cyclic));
                if !deadlocks {
                    // 12.b.v.1. Set module.[[PendingAsyncDependencies]] to module.[[PendingAsyncDependencies]] + 1.
                    let pending = self.pending_async_dependencies().expect("PendingAsyncDependencies vazio em avaliação");
                    self.set_pending_async_dependencies(Some(pending + 1));
                    // 12.b.v.2. Append module to requiredModule.[[AsyncParentModules]].
                    cyclic.append_async_parent_module(self);
                }
            }
        }
        // 12. If module.[[PendingAsyncDependencies]] > 0 or module.[[HasTLA]] is true, then
        if self.pending_async_dependencies().unwrap_or(0) > 0 || self.has_tla() {
            // 12.a. Assert: module.[[AsyncEvaluationOrder]] is UNSET.
            debug_assert!(self.async_evaluation_order().is_unset());
            // 12.b. Set module.[[AsyncEvaluationOrder]] to IncrementModuleAsyncEvaluationCount().
            self.set_async_evaluation_order(AsyncEvaluationOrder::new(vm.increment_module_async_evaluation_count()));
            // 12.c. If module.[[PendingAsyncDependencies]] = 0, perform ExecuteAsyncModule(module).
            if self.pending_async_dependencies() == Some(0) {
                self.execute_async(global_object)?;
            }
        // 13. Else,
        } else {
            // 13.a. Perform ? module.ExecuteModule().
            self.execute(global_object, None)?;
        }
        // 14. Assert: module occurs exactly once in stack.
        debug_assert!(stack.iter().filter(|member| Rc::ptr_eq(member, self)).count() == 1);
        // 15. Assert: module.[[DFSAncestorIndex]] <= moduleIndex.
        debug_assert!(self.ext.dfs_ancestor_index.get() <= module_index);
        // 16. If module.[[DFSAncestorIndex]] = moduleIndex, then
        if self.ext.dfs_ancestor_index.get() == module_index {
            // 16.b. Repeat, while done is false,
            loop {
                // 16.b.i. Let requiredModule be the last element of stack.
                // 16.b.ii. Remove the last element of stack.
                let required = stack.pop().expect("pilha de avaliação vazia");
                // 16.b.iv. Assert: requiredModule.[[AsyncEvaluationOrder]] is either an integer or UNSET.
                debug_assert!(required.async_evaluation_order().has_order() || required.async_evaluation_order().is_unset());
                // 16.b.v. If requiredModule.[[AsyncEvaluationOrder]] is UNSET, set requiredModule.[[Status]] to EVALUATED.
                // 16.b.vi. Otherwise, set requiredModule.[[Status]] to EVALUATING-ASYNC.
                required.set_status(if required.async_evaluation_order().is_unset() { Status::Evaluated } else { Status::EvaluatingAsync });
                // 16.b.viii. Set requiredModule.[[CycleRoot]] to module.
                required.set_cycle_root(self);
                // 16.b.vii. If requiredModule and module are the same Module Record, set done to true.
                if Rc::ptr_eq(&required, self) {
                    break;
                }
            }
        }
        // 17. Return index.
        Ok(index)
    }

    /// `AbstractModuleRecord::evaluateModuleSync()` de um módulo que não é cíclico: a avaliação do
    /// `SyntheticModuleRecord` é síncrona e devolve `undefined` (nunca uma promessa), então não há o que
    /// materializar nem lançar.
    fn evaluate_module_sync(&self) {
        // EvaluateModuleSync(module)
        // https://tc39.es/ecma262/#sec-EvaluateModuleSync
        debug_assert!(!self.is_cyclic());
    }

    /// `AbstractModuleRecord::gatherAsynchronousTransitiveDependencies(result, seen)`.
    pub fn gather_asynchronous_transitive_dependencies(
        self: &Rc<Self>,
        result: &mut Vec<AbstractModuleRecordRef>,
        seen: &mut Vec<AbstractModuleRecordRef>,
    ) {
        // https://tc39.es/proposal-defer-import-eval/#sec-GatherAsynchronousTransitiveDependencies
        // The spec text is recursive; we use an explicit work list to avoid native stack overflow on deep
        // graphs. Children are pushed in reverse to preserve the spec's pre-order discovery order.
        let mut stack: Vec<AbstractModuleRecordRef> = vec![Rc::clone(self)];
        while let Some(module) = stack.pop() {
            // 3. If seen contains module, return result.
            // 4. Append module to seen.
            if !push_unique(seen, Rc::clone(&module)) {
                continue;
            }
            // 5. If module is not a Cyclic Module Record, return result.
            if !module.is_cyclic() {
                continue;
            }
            // 6. If module.[[Status]] is either EVALUATING or IsModuleSCCEvaluated(module), return result.
            if module.status() == Status::Evaluating || module.is_scc_evaluated() {
                continue;
            }
            // 7. If module.[[HasTLA]] is true, then
            if module.has_tla() {
                // 7.a. Append module to result.
                // 7.b. Return result.
                push_unique(result, module);
                continue;
            }
            // 8. For each ModuleRequest Record request of module.[[RequestedModules]], do
            for request in module.requested_modules().iter().rev() {
                stack.push(module.get_imported_module(request));
            }
        }
        // 9. Return result.
    }

    /// `AbstractModuleRecord::readyForSyncExecution()`.
    pub fn ready_for_sync_execution(self: &Rc<Self>) -> bool {
        // https://tc39.es/proposal-defer-import-eval/#sec-ReadyForSyncExecution
        // The spec text is recursive; we use an explicit work list to avoid native stack overflow.
        let mut seen: Vec<AbstractModuleRecordRef> = Vec::new();
        let mut stack: Vec<AbstractModuleRecordRef> = vec![Rc::clone(self)];
        while let Some(module) = stack.pop() {
            // 1. If module is not a Cyclic Module Record, return true.
            if !module.is_cyclic() {
                continue;
            }
            // 3. If seen contains module, return true.
            // 4. Append module to seen.
            if !push_unique(&mut seen, Rc::clone(&module)) {
                continue;
            }
            // 5. If IsModuleSCCEvaluated(module), return true.
            if module.is_scc_evaluated() {
                continue;
            }
            // 6. If module.[[Status]] is either EVALUATING or EVALUATING-ASYNC, return false.
            if matches!(module.status(), Status::Evaluating | Status::EvaluatingAsync) {
                return false;
            }
            // 7. Assert: module.[[Status]] is LINKED or EVALUATED (a module whose own body has run inside a
            // cycle that is still awaiting; the walk below then reaches its EVALUATING-ASYNC cycle root).
            debug_assert!(matches!(module.status(), Status::Linked | Status::Evaluated));
            // 8. If module.[[HasTLA]] is true, return false.
            if module.has_tla() {
                return false;
            }
            // 9. For each ModuleRequest Record request of module.[[RequestedModules]], do
            for request in module.requested_modules().iter() {
                stack.push(module.get_imported_module(request));
            }
        }
        // 10. Return true.
        true
    }

    /// `AbstractModuleRecord::evaluateSync(globalObject)`.
    pub fn evaluate_sync(self: &Rc<Self>, global_object: &JSGlobalObject) -> ModuleResult<()> {
        // https://tc39.es/proposal-defer-import-eval/#sec-EvaluateModuleSync
        // 1. If ReadyForSyncExecution(module) is false, throw a TypeError exception.
        if !self.ready_for_sync_execution() {
            let mut scope = ThrowScope::new(global_object.vm());
            throw_exception(
                global_object,
                &mut scope,
                create_type_error(global_object, &WtfString::from_utf8(b"Unable to synchronously evaluate deferred module")),
            );
            return Err(Thrown::Pending);
        }
        // 2. Let promise be ! module.Evaluate().
        let promise = self.evaluate(global_object)?;
        // 3. Assert: promise.[[PromiseState]] is either FULFILLED or REJECTED.
        debug_assert!(promise.status() != PromiseStatus::Pending);
        // 4. If promise.[[PromiseState]] is REJECTED, then
        if promise.status() == PromiseStatus::Rejected {
            // 4.a. and 4.b. Mark the promise as handled.
            promise.mark_as_handled();
            // 4.c. Return ThrowCompletion(promise.[[PromiseResult]]).
            let mut scope = ThrowScope::new(global_object.vm());
            throw_exception(global_object, &mut scope, promise.result());
            return Err(Thrown::Pending);
        }
        // 5. Return UNUSED.
        Ok(())
    }

    /// `AbstractModuleRecord::evaluate(globalObject)`: a promessa da capacidade de topo (`referrerAsyncOrder`
    /// `-1`, sem `dynamicImportPromise`).
    pub fn evaluate(self: &Rc<Self>, global_object: &JSGlobalObject) -> ModuleResult<JSPromiseRef> {
        self.evaluate_dynamic(global_object, -1, None)
    }

    /// `AbstractModuleRecord::evaluate(globalObject, referrerAsyncOrder, dynamicImportPromise)`.
    pub fn evaluate_dynamic(
        self: &Rc<Self>,
        global_object: &JSGlobalObject,
        referrer_async_order: i64,
        dynamic_import_promise: Option<&JSPromiseRef>,
    ) -> ModuleResult<JSPromiseRef> {
        if self.is_cyclic() {
            return self.cyclic_evaluate(global_object, referrer_async_order, dynamic_import_promise);
        }
        // `wrap(syntheticRecord->evaluate(globalObject))`: o `undefined` virou uma promessa resolvida.
        let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
        promise.resolve(global_object, JSValue::undefined());
        Ok(promise)
    }

    /// `CyclicModuleRecord::evaluate(globalObject, referrerAsyncOrder, dynamicImportPromise)`: a `JSPromise`
    /// da capacidade de topo.
    fn cyclic_evaluate(
        self: &Rc<Self>,
        global_object: &JSGlobalObject,
        referrer_async_order: i64,
        dynamic_import_promise: Option<&JSPromiseRef>,
    ) -> ModuleResult<JSPromiseRef> {
        // https://tc39.es/ecma262/#sec-moduleevaluation
        let vm = global_object.vm();
        // 2. Assert: module.[[Status]] is one of LINKED, EVALUATING-ASYNC, or EVALUATED.
        debug_assert!(matches!(self.status(), Status::Linked | Status::EvaluatingAsync | Status::Evaluated));

        let mut module = Rc::clone(self);
        // 3. If module.[[Status]] is either EVALUATING-ASYNC or EVALUATED, then
        if matches!(self.status(), Status::EvaluatingAsync | Status::Evaluated) {
            // 3.a. If module.[[CycleRoot]] is not EMPTY, then set module to module.[[CycleRoot]].
            // 3.b. Else, module.[[Status]] is EVALUATED and module.[[EvaluationError]] is a throw completion.
            if let Some(root) = self.cycle_root() {
                module = root;
            } else {
                debug_assert!(self.status() == Status::Evaluated && self.evaluation_error().is_some());
            }
        }
        // 4. If module.[[TopLevelCapability]] is not EMPTY, return module.[[TopLevelCapability]].[[Promise]].
        if let Some(promise) = module.top_level_capability() {
            return Ok(promise);
        }

        // 5. Let stack be a new empty List.
        let mut stack: Vec<AbstractModuleRecordRef> = Vec::new();
        // 6. Let capability be ! NewPromiseCapability(%Promise%).
        let capability = JSPromise::create(vm, &global_object.promise_structure());
        // 7. Set module.[[TopLevelCapability]] to capability.
        *module.ext.top_level_capability.borrow_mut() = Some(Rc::clone(&capability));

        // 8. Let result be Completion(InnerModuleEvaluation(module, stack, 0)).
        match module.inner_module_evaluation(global_object, &mut stack, 0, referrer_async_order, dynamic_import_promise) {
            // 10. Else,
            Ok(_) => {
                // 10.a. Assert: module.[[Status]] is either EVALUATING-ASYNC or EVALUATED.
                debug_assert!(matches!(module.status(), Status::EvaluatingAsync | Status::Evaluated));
                // 10.b. Assert: module.[[EvaluationError]] is EMPTY.
                debug_assert!(module.evaluation_error().is_none());
                // 10.c. If module.[[Status]] is EVALUATED, then
                if module.status() == Status::Evaluated {
                    // 10.c.i. Assert: module.[[AsyncEvaluationOrder]] is either UNSET or DONE.
                    debug_assert!(module.async_evaluation_order().is_unset() || module.async_evaluation_order().is_done());
                    // 10.c.iii. Perform ! Call(capability.[[Resolve]], undefined, « undefined »).
                    capability.fulfill(global_object, JSValue::undefined());
                }
                // 10.d. Assert: stack is empty.
                debug_assert!(stack.is_empty());
                Ok(capability)
            }
            // 9. If result is an abrupt completion, then
            Err(Thrown::Pending) => {
                let value = take_pending_exception(global_object);
                // 9.a. For each Cyclic Module Record m of stack, do
                for member in &stack {
                    // 9.a.i. Assert: m.[[Status]] is EVALUATING.
                    debug_assert!(member.status() == Status::Evaluating);
                    // 9.a.ii. Set m.[[Status]] to EVALUATED.
                    member.set_status(Status::Evaluated);
                    // 9.a.iii. Set m.[[EvaluationError]] to result.
                    member.set_evaluation_error(value);
                }
                // 9.b. Assert: module.[[Status]] is EVALUATED.
                debug_assert!(module.status() == Status::Evaluated);
                // 9.d. Perform ! Call(capability.[[Reject]], undefined, « result.[[Value]] »).
                capability.reject(global_object, value);
                Ok(capability)
            }
            Err(other) => Err(other),
        }
    }
}

/// Acrescenta `module` se ainda não está em `list` (por identidade): o `OrderedHashSet::add` e o
/// `HashSet::add(...).isNewEntry`. Devolve se entrou.
fn push_unique(list: &mut Vec<AbstractModuleRecordRef>, module: AbstractModuleRecordRef) -> bool {
    if list.iter().any(|member| Rc::ptr_eq(member, &module)) {
        return false;
    }
    list.push(module);
    true
}

/// `gatherAvailableAncestors(module, execList)`.
fn gather_available_ancestors(module: &AbstractModuleRecordRef, exec_list: &mut Vec<AbstractModuleRecordRef>) {
    // GatherAvailableAncestors(module, execList)
    // https://tc39.es/ecma262/#sec-gather-available-ancestors
    //
    // We replace the spec's "execList does not contain m" membership test with
    // "m.[[PendingAsyncDependencies]] != 0" so the loop is O(N) instead of O(N^2). The spec specifies this as
    // a recursion, but we use a worklist loop to avoid stack overflow crashes.
    let mut worklist: Vec<AbstractModuleRecordRef> = vec![Rc::clone(module)];
    while let Some(record) = worklist.pop() {
        // 1. For each Cyclic Module Record m of module.[[AsyncParentModules]], do
        let parents: Vec<AbstractModuleRecordRef> = record.async_parent_modules().clone();
        for m in &parents {
            // 1.a. If execList does not contain m and m.[[CycleRoot]].[[EvaluationError]] is empty, then
            // (Probable spec bug: https://github.com/tc39/ecma262/issues/3766. We need an additional check
            // here that m.[[CycleRoot]] isn't empty.)
            debug_assert!(m.cycle_root().is_some() || m.evaluation_error().is_some());
            let Some(root) = m.cycle_root() else { continue };
            if root.evaluation_error().is_some() {
                continue;
            }
            let pending = m.pending_async_dependencies().expect("PendingAsyncDependencies vazio em ancestral assíncrono");
            // Verify the invariant in debug: execList contains m iff pending == 0 (under cycleRoot OK).
            debug_assert!(exec_list.iter().any(|member| Rc::ptr_eq(member, m)) == (pending == 0));
            if pending == 0 {
                continue;
            }
            // 1.a.i. Assert: m.[[Status]] is EVALUATING-ASYNC.
            debug_assert!(m.status() == Status::EvaluatingAsync);
            // 1.a.ii. Assert: m.[[EvaluationError]] is EMPTY.
            debug_assert!(m.evaluation_error().is_none());
            // 1.a.iii. Assert: m.[[AsyncEvaluationOrder]] is an integer.
            debug_assert!(m.async_evaluation_order().has_order());
            // 1.a.v. Set m.[[PendingAsyncDependencies]] to m.[[PendingAsyncDependencies]] - 1.
            let new_dependencies = pending - 1;
            m.set_pending_async_dependencies(Some(new_dependencies));
            // 1.a.vi. If m.[[PendingAsyncDependencies]] = 0, then
            if new_dependencies == 0 {
                // 1.a.vi.1. Append m to execList.
                exec_list.push(Rc::clone(m));
                // 1.a.vi.2. If m.[[HasTLA]] is false, perform GatherAvailableAncestors(m, execList).
                if !m.has_tla() {
                    worklist.push(Rc::clone(m));
                }
            }
        }
    }
    // 2. Return UNUSED.
}

/// `importPromiseGatesAsyncDependency(importPromise, dependency)` (`USE(BUN_JSC_ADDITIONS)`): o `import()`
/// de `importPromise` só se resolve depois que `dependency` termina, porque a continuação que o espera é
/// o próprio `dependency` (ou um descendente assíncrono dele) suspenso num `await`.
fn import_promise_gates_async_dependency(import_promise: &JSPromiseRef, dependency: &AbstractModuleRecordRef) -> bool {
    // Does resuming `module` eventually resume `dependency`? Walks the async parents.
    let resumes_dependency = |module: &AbstractModuleRecordRef| -> bool {
        let mut seen: Vec<AbstractModuleRecordRef> = Vec::new();
        let mut work: Vec<AbstractModuleRecordRef> = vec![Rc::clone(module)];
        while let Some(current) = work.pop() {
            if Rc::ptr_eq(&current, dependency) {
                return true;
            }
            if !push_unique(&mut seen, Rc::clone(&current)) {
                continue;
            }
            for parent in current.async_parent_modules().iter() {
                work.push(Rc::clone(parent));
            }
        }
        false
    };

    let cell_of = |value: JSValue| -> Option<JSValue> { if value.is_empty() || !value.is_cell() { None } else { Some(value) } };

    let mut seen: Vec<JSPromiseRef> = Vec::new();
    let mut work: Vec<JSPromiseRef> = vec![Rc::clone(import_promise)];
    let mut found = false;

    // `follow(value)`: se é uma promessa, entra na lista de trabalho.
    let follow = |work: &mut Vec<JSPromiseRef>, value: JSValue| {
        if let Some(promise) = cell_of(value).and_then(|cell| JSPromise::from_value(&cell)) {
            work.push(promise);
        }
    };

    // A promise, or the async function or module body that an await or for-await resumes.
    let follow_promise_or_driver = |work: &mut Vec<JSPromiseRef>, found: &mut bool, cell: Option<JSValue>| {
        let Some(cell) = cell else { return };
        if let Some(promise) = JSPromise::from_value(&cell) {
            work.push(promise);
        } else if let Some(generator) = JSAsyncFunctionGenerator::from_value(&cell) {
            follow(work, generator.context());
        } else if let Some(module) = AbstractModuleRecord::from_cell_id(cell.as_cell()) {
            *found = resumes_dependency(&module);
        }
    };

    const MAX_PROMISES: usize = 4096;
    while let Some(promise) = work.pop() {
        if found {
            break;
        }
        if promise.status() != PromiseStatus::Pending {
            continue;
        }
        if seen.iter().any(|member| Rc::ptr_eq(member, &promise)) {
            continue;
        }
        seen.push(Rc::clone(&promise));
        if seen.len() > MAX_PROMISES {
            return false;
        }
        let mut visit_reaction = |task: InternalMicrotask, cell: JSValue, context: JSValue| -> bool {
            match task {
                InternalMicrotask::AsyncFunctionResume
                | InternalMicrotask::AsyncModuleExecutionResume
                | InternalMicrotask::AsyncGeneratorDriverResume => {
                    follow_promise_or_driver(&mut work, &mut found, cell_of(context));
                }
                InternalMicrotask::AsyncFromSyncIteratorContinue | InternalMicrotask::AsyncFromSyncIteratorDone => {
                    // for-await over sync values that are promises: the pending step settles the iterator's
                    // result promise, or resumes its driver, with this promise's value.
                    let iterator = cell_of(context).and_then(|cell| JSAsyncFromSyncIterator::from_value(&cell));
                    if let Some(target) = iterator.and_then(|iterator| iterator.target()) {
                        follow_promise_or_driver(&mut work, &mut found, cell_of(target));
                    }
                }
                InternalMicrotask::PromiseFinallyReactionJob | InternalMicrotask::PromiseFinallyAwaitJob => {
                    // The context record holds the promise that .finally() returned.
                    if let Some(JSPromiseReactionRef::Slim(record)) = cell_of(context).and_then(|cell| JSPromiseReactionRef::from_value(&cell)) {
                        follow(&mut work, record.promise());
                    }
                }
                InternalMicrotask::PromiseAllResolveJob | InternalMicrotask::PromiseAllSettledResolveJob => {
                    if let Some(global_context) = cell_of(cell).and_then(|cell| JSPromiseCombinatorsGlobalContext::from_value(&cell)) {
                        follow(&mut work, global_context.promise());
                    }
                }
                InternalMicrotask::None
                | InternalMicrotask::PromiseResolveThenableJobFast
                | InternalMicrotask::PromiseResolveThenableJobWithInternalMicrotaskFast
                | InternalMicrotask::PromiseResolveThenableJob
                | InternalMicrotask::PromiseResolveThenableJobWithInternalMicrotask
                | InternalMicrotask::PromiseResolveWithoutHandlerJob
                | InternalMicrotask::PromiseFulfillWithoutHandlerJob
                | InternalMicrotask::PromiseReactionJob
                | InternalMicrotask::ModuleLoadStep
                | InternalMicrotask::ModuleLoadTopSettled
                | InternalMicrotask::ModuleLoadTopRejected
                | InternalMicrotask::ModuleLoadSpecifierTransform
                | InternalMicrotask::ModuleLoadCombinedLoadSettled
                | InternalMicrotask::ModuleLoadCombinedStateSettled
                | InternalMicrotask::ModuleLoadLinkEvaluateSettled
                | InternalMicrotask::ModuleLoadReturnRecord
                | InternalMicrotask::ModuleLoadReturnModuleKey
                | InternalMicrotask::ModuleLoadStoreError
                | InternalMicrotask::ImportModuleNamespace
                | InternalMicrotask::DynamicImportLoadSettled
                | InternalMicrotask::DynamicImportEvaluateSettled
                | InternalMicrotask::DynamicImportDeferLoadSettled
                | InternalMicrotask::DynamicImportDeferDependencySettled => follow(&mut work, cell),
                _ => {}
            }
            !found
        };
        promise.for_each_pending_reaction(&mut visit_reaction);
    }
    found
}
