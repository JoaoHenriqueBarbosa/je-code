//! Tradução de `runtime/AbstractModuleRecord.h` e da parte de dados de `AbstractModuleRecord.cpp`
//! (`resolveExport`, `resolveImport` e o cache de resolução ficam em `abstract_module_record_resolve`).
//!
//! DIVERGÊNCIAS:
//!
//! - Sem GC o registro é `Rc<AbstractModuleRecord>` (`AbstractModuleRecordRef`) e a identidade do
//!   `AbstractModuleRecord*` é o endereço do `Rc` (`Rc::ptr_eq`). Os campos mutáveis ficam em
//!   `RefCell`/`Cell`, porque o C++ muta o registro por ponteiro. As subclasses (`JSModuleRecord`,
//!   `SyntheticModuleRecord`, `CyclicModuleRecord`) entram por composição: guardam o
//!   `AbstractModuleRecordRef` e o expõem por `Deref`, de modo que a `Resolution` sempre aponta para a
//!   base. `WriteBarrier`, `visitChildren`, `estimatedSize`, `needsDestruction` e `subspaceFor` são
//!   maquinaria de heap e não existem.
//! - `JSInternalFieldObjectImpl<2>` vira o array `internal_fields` (`internalField(Field)`).
//! - `OrderedHashMap`/`OrderedHashSet` do WTF viram `OrderedKeyMap` e `OrderedKeySet` (ordem de
//!   inserção, chave por identidade de `UniquedStringImpl`, como `IdentifierRepHash`).
//! - Não portado, porque depende de classe que ainda não existe: `dump`. O que veio depois:
//!   `CyclicModuleRecord`, `JSModuleRecord`, `SyntheticModuleRecord`, `m_asyncCapability`,
//!   `getModuleNamespace`, o top-level await e os caches de namespace estão em `js_module_record.rs` e
//!   `synthetic_module_record.rs` (campo `ext`), por composição no próprio `AbstractModuleRecord` (um só
//!   `Rc` para a identidade da `Resolution`, do `JSModuleEnvironment` e do registro de células).
//! - `hostResolveImportedModule` não lança no C++ (só consulta `m_loadedModules`), então os
//!   `RETURN_IF_EXCEPTION` de `resolveExport` não têm o que testar e o porte não passa `ThrowScope`.
//! - `Resolution::module_record` e `LoadedModuleRequest::module` são `Option<AbstractModuleRecordRef>`
//!   (o `nullptr` do C++). O ciclo `JSModuleEnvironment` <-> registro vive até o fim da thread, como o
//!   restante das células sem GC.

use std::cell::{Cell, Ref, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::rc::Rc;

use crate::parser::source_provider::SourceProviderSourceType;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_module_environment::JSModuleEnvironmentRef;
use crate::runtime::js_module_record::ModuleRecordExt;
use crate::runtime::js_value::{js_number_i32, js_undefined, JSValue};
use crate::runtime::module_map::{ModuleMap, ModuleMapKey};
use crate::runtime::script_fetch_parameters::{ScriptFetchParametersRef, ScriptFetchParametersType};
use crate::wtf::text::string_impl::UniquedKey;

/// `JSInternalFieldObjectImpl<2>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 2;

pub use crate::runtime::js_generator::{Argument, ResumeMode, State};

/// `enum class Field : uint32_t`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    State = 0,
    Frame = 1,
}

/// A chave de `RefPtr<UniquedStringImpl>` (nula possível), comparada por identidade.
pub type UniquedStringKey = Option<UniquedKey>;

/// `OrderedHashMap<RefPtr<UniquedStringImpl>, V, IdentifierRepHash, ...>`: itera na ordem de
/// inserção; `add` não sobrescreve a chave existente.
#[derive(Clone, Debug)]
pub struct OrderedKeyMap<V> {
    entries: Vec<(UniquedStringKey, V)>,
    index: HashMap<UniquedStringKey, usize>,
}

impl<V> Default for OrderedKeyMap<V> {
    fn default() -> Self {
        OrderedKeyMap { entries: Vec::new(), index: HashMap::new() }
    }
}

impl<V> OrderedKeyMap<V> {
    /// `add(key, value).isNewEntry`: verdadeiro se a chave é nova (senão nada muda).
    pub fn add(&mut self, key: UniquedStringKey, value: V) -> bool {
        if self.index.contains_key(&key) {
            return false;
        }
        self.index.insert(key.clone(), self.entries.len());
        self.entries.push((key, value));
        true
    }

    /// `find(key)`.
    pub fn get(&self, key: &UniquedStringKey) -> Option<&V> {
        self.index.get(key).map(|&position| &self.entries[position].1)
    }

    /// `size()`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `begin()..end()`, na ordem de inserção.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &(UniquedStringKey, V)> {
        self.entries.iter()
    }

    /// `reserveInitialCapacity(n)`.
    pub fn reserve_initial_capacity(&mut self, capacity: usize) {
        self.entries.reserve(capacity);
        self.index.reserve(capacity);
    }
}

/// `OrderedHashSet<T, Hash>`: ordem de inserção, sem duplicatas.
#[derive(Clone, Debug)]
pub struct OrderedKeySet<T: std::hash::Hash + Eq + Clone> {
    entries: Vec<T>,
    seen: HashSet<T>,
}

impl<T: std::hash::Hash + Eq + Clone> Default for OrderedKeySet<T> {
    fn default() -> Self {
        OrderedKeySet { entries: Vec::new(), seen: HashSet::new() }
    }
}

impl<T: std::hash::Hash + Eq + Clone> OrderedKeySet<T> {
    /// `add(value).isNewEntry`.
    pub fn add(&mut self, value: T) -> bool {
        if !self.seen.insert(value.clone()) {
            return false;
        }
        self.entries.push(value);
        true
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `begin()..end()`, na ordem de inserção.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.entries.iter()
    }
}

/// `ExportEntry::Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportEntryType {
    Local,
    Indirect,
    Namespace,
}

/// `AbstractModuleRecord::ExportEntry`.
#[derive(Clone, Debug)]
pub struct ExportEntry {
    pub type_: ExportEntryType,
    pub module_request_type: ScriptFetchParametersType,
    pub export_name: Identifier,
    pub module_name: Identifier,
    pub import_name: Identifier,
    pub local_name: Identifier,
}

impl ExportEntry {
    /// `ExportEntry::createLocal(exportName, localName)`.
    pub fn create_local(export_name: &Identifier, local_name: &Identifier) -> ExportEntry {
        ExportEntry {
            type_: ExportEntryType::Local,
            module_request_type: ScriptFetchParametersType::JavaScript,
            export_name: export_name.clone(),
            module_name: Identifier::default(),
            import_name: Identifier::default(),
            local_name: local_name.clone(),
        }
    }

    /// `ExportEntry::createIndirect(exportName, importName, moduleName, moduleRequestType)`.
    pub fn create_indirect(
        export_name: &Identifier,
        import_name: &Identifier,
        module_name: &Identifier,
        module_request_type: ScriptFetchParametersType,
    ) -> ExportEntry {
        ExportEntry {
            type_: ExportEntryType::Indirect,
            module_request_type,
            export_name: export_name.clone(),
            module_name: module_name.clone(),
            import_name: import_name.clone(),
            local_name: Identifier::default(),
        }
    }

    /// `ExportEntry::createNamespace(exportName, moduleName, moduleRequestType)`.
    pub fn create_namespace(
        export_name: &Identifier,
        module_name: &Identifier,
        module_request_type: ScriptFetchParametersType,
    ) -> ExportEntry {
        ExportEntry {
            type_: ExportEntryType::Namespace,
            module_request_type,
            export_name: export_name.clone(),
            module_name: module_name.clone(),
            import_name: Identifier::default(),
            local_name: Identifier::default(),
        }
    }
}

/// `enum class ModulePhase : uint8_t { Evaluation, Defer }`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModulePhase {
    Evaluation = 0,
    Defer = 1,
}

/// `enum class ImportEntryType` (com `SingleTypeScript` do `USE(BUN_JSC_ADDITIONS)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportEntryType {
    Single,
    /// Se o export correspondente não existe, não emite erro.
    SingleTypeScript,
    Namespace,
}

/// `AbstractModuleRecord::ImportEntry`.
#[derive(Clone, Debug)]
pub struct ImportEntry {
    pub type_: ImportEntryType,
    pub phase: ModulePhase,
    pub module_request_type: ScriptFetchParametersType,
    pub module_request: Identifier,
    pub import_name: Identifier,
    pub local_name: Identifier,
}

/// `StarExportEntry`: `std::pair<RefPtr<UniquedStringImpl>, ScriptFetchParameters::Type>`.
pub type StarExportEntry = (UniquedStringKey, ScriptFetchParametersType);

/// `StarExportEntries`.
pub type StarExportEntries = OrderedKeySet<StarExportEntry>;

/// `ImportEntries`: localName -> `ImportEntry`.
pub type ImportEntries = OrderedKeyMap<ImportEntry>;

/// `ExportEntries`: exportName -> `ExportEntry`.
pub type ExportEntries = OrderedKeyMap<ExportEntry>;

/// `AbstractModuleRecord::ModuleRequest`.
#[derive(Clone, Debug)]
pub struct ModuleRequest {
    pub specifier: Identifier,
    pub attributes: Option<ScriptFetchParametersRef>,
    pub phase: ModulePhase,
}

impl ModuleRequest {
    /// `type(fallback)`.
    pub fn type_(&self, fallback: ScriptFetchParametersType) -> ScriptFetchParametersType {
        match &self.attributes {
            Some(attributes) => attributes.type_(),
            None => fallback,
        }
    }
}

impl PartialEq for ModuleRequest {
    /// `ModuleRequest::operator==`: o especificador e o tipo dos atributos (a fase não entra).
    fn eq(&self, other: &ModuleRequest) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        if self.specifier != other.specifier {
            return false;
        }
        if self.attributes.is_some() != other.attributes.is_some() {
            return false;
        }
        match (&self.attributes, &other.attributes) {
            (Some(a), Some(b)) => a.type_() == b.type_(),
            _ => true,
        }
    }
}

/// `AbstractModuleRecord::LoadedModuleRequest : ModuleRequest`.
#[derive(Clone, Debug)]
pub struct LoadedModuleRequest {
    request: ModuleRequest,
    /// `m_module`.
    pub module: Option<AbstractModuleRecordRef>,
}

impl LoadedModuleRequest {
    /// `LoadedModuleRequest(vm, moduleRequest, loadedModule, owner)`.
    pub fn new(request: ModuleRequest, loaded_module: Option<AbstractModuleRecordRef>) -> LoadedModuleRequest {
        LoadedModuleRequest { request, module: loaded_module }
    }
}

impl Deref for LoadedModuleRequest {
    type Target = ModuleRequest;

    fn deref(&self) -> &ModuleRequest {
        &self.request
    }
}

/// `AbstractModuleRecord::AsyncEvaluationOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsyncEvaluationOrder {
    m_order: i64,
}

impl Default for AsyncEvaluationOrder {
    fn default() -> Self {
        AsyncEvaluationOrder { m_order: AsyncEvaluationOrder::UNSET }
    }
}

impl AsyncEvaluationOrder {
    const UNSET: i64 = -2;
    const DONE: i64 = -1;

    /// `AsyncEvaluationOrder(int64_t order)`.
    pub fn new(order: i64) -> AsyncEvaluationOrder {
        AsyncEvaluationOrder { m_order: order }
    }

    /// `isDone()`.
    pub fn is_done(&self) -> bool {
        self.m_order == AsyncEvaluationOrder::DONE
    }

    /// `isUnset()`.
    pub fn is_unset(&self) -> bool {
        self.m_order == AsyncEvaluationOrder::UNSET
    }

    /// `hasOrder()`.
    pub fn has_order(&self) -> bool {
        self.m_order >= 0
    }

    /// `setDone()`.
    pub fn set_done(&mut self) {
        self.m_order = AsyncEvaluationOrder::DONE;
    }

    /// `order()`.
    pub fn order(&self) -> i64 {
        debug_assert!(self.has_order());
        self.m_order
    }

    /// `order(int64_t)`.
    pub fn set_order(&mut self, order: i64) -> &mut AsyncEvaluationOrder {
        debug_assert!(order >= 0);
        self.m_order = order;
        self
    }

    /// `done()`.
    pub fn done() -> AsyncEvaluationOrder {
        AsyncEvaluationOrder { m_order: AsyncEvaluationOrder::DONE }
    }
}

/// `Resolution::Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionType {
    Resolved,
    NotFound,
    Ambiguous,
    Error,
}

/// `AbstractModuleRecord::Resolution`.
#[derive(Clone, Debug)]
pub struct Resolution {
    pub type_: ResolutionType,
    pub module_record: Option<AbstractModuleRecordRef>,
    pub local_name: Identifier,
}

impl Resolution {
    /// `Resolution::notFound()`.
    pub fn not_found() -> Resolution {
        Resolution { type_: ResolutionType::NotFound, module_record: None, local_name: Identifier::default() }
    }

    /// `Resolution::error()`.
    pub fn error() -> Resolution {
        Resolution { type_: ResolutionType::Error, module_record: None, local_name: Identifier::default() }
    }

    /// `Resolution::ambiguous()`.
    pub fn ambiguous() -> Resolution {
        Resolution { type_: ResolutionType::Ambiguous, module_record: None, local_name: Identifier::default() }
    }

    /// `isSameBinding(other)`: mesmo registro (por identidade) e mesmo `localName`.
    pub fn is_same_binding(&self, other: &Resolution) -> bool {
        let same_record = match (&self.module_record, &other.module_record) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        same_record && self.local_name == other.local_name
    }
}

/// `AbstractModuleRecord*`.
pub type AbstractModuleRecordRef = Rc<AbstractModuleRecord>;

/// `class AbstractModuleRecord : public JSInternalFieldObjectImpl<2>`.
pub struct AbstractModuleRecord {
    /// `m_moduleKey`.
    m_module_key: Identifier,
    /// `m_importEntries`.
    m_import_entries: RefCell<ImportEntries>,
    /// `m_exportEntries`.
    m_export_entries: RefCell<ExportEntries>,
    /// `m_starExportEntries`.
    m_star_export_entries: RefCell<StarExportEntries>,
    /// `m_requestedModules`.
    m_requested_modules: RefCell<Vec<ModuleRequest>>,
    /// `m_resolutionCache`.
    pub(crate) m_resolution_cache: RefCell<HashMap<UniquedStringKey, Resolution>>,
    /// `m_moduleEnvironment`.
    m_module_environment: RefCell<Option<JSModuleEnvironmentRef>>,
    /// `m_loadedModules`.
    m_loaded_modules: RefCell<ModuleMap<LoadedModuleRequest>>,
    /// `m_asyncParentModules`.
    m_async_parent_modules: RefCell<Vec<AbstractModuleRecordRef>>,
    /// `m_asyncEvaluationOrder`.
    m_async_evaluation_order: Cell<AsyncEvaluationOrder>,
    /// `m_pendingAsyncDependencies`.
    m_pending_async_dependencies: Cell<Option<i32>>,
    /// `m_hasTLA`.
    m_has_tla: Cell<bool>,
    /// `m_sourceType`.
    m_source_type: SourceProviderSourceType,
    /// `m_isTypeScript` (`USE(BUN_JSC_ADDITIONS)`).
    pub is_type_script: Cell<bool>,
    /// Os campos internos do `JSInternalFieldObjectImpl<2>`.
    m_internal_fields: RefCell<[JSValue; NUMBER_OF_INTERNAL_FIELDS as usize]>,
    /// O que as subclasses `CyclicModuleRecord` e `JSModuleRecord` e os caches `m_moduleNamespaceObject`
    /// e `m_deferredNamespaceObject` acrescentam (ver `js_module_record.rs`).
    pub(crate) ext: ModuleRecordExt,
}

impl std::fmt::Debug for AbstractModuleRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AbstractModuleRecord").field("module_key", &self.m_module_key).finish_non_exhaustive()
    }
}

impl AbstractModuleRecord {
    /// `initialValues()`: `{ jsNumber(State::Init), jsUndefined() }`.
    pub fn initial_values() -> [JSValue; NUMBER_OF_INTERNAL_FIELDS as usize] {
        [js_number_i32(State::Init as i32), js_undefined()]
    }

    /// `AbstractModuleRecord(vm, structure, moduleKey, sourceType)` mais `finishCreation`, que escreve
    /// os `initialValues()` nos campos internos.
    pub fn create(module_key: Identifier, source_type: SourceProviderSourceType) -> AbstractModuleRecordRef {
        Rc::new(AbstractModuleRecord {
            m_module_key: module_key,
            m_import_entries: RefCell::new(ImportEntries::default()),
            m_export_entries: RefCell::new(ExportEntries::default()),
            m_star_export_entries: RefCell::new(StarExportEntries::default()),
            m_requested_modules: RefCell::new(Vec::new()),
            m_resolution_cache: RefCell::new(HashMap::new()),
            m_module_environment: RefCell::new(None),
            m_loaded_modules: RefCell::new(ModuleMap::new()),
            m_async_parent_modules: RefCell::new(Vec::new()),
            m_async_evaluation_order: Cell::new(AsyncEvaluationOrder::default()),
            m_pending_async_dependencies: Cell::new(None),
            m_has_tla: Cell::new(false),
            m_source_type: source_type,
            is_type_script: Cell::new(false),
            m_internal_fields: RefCell::new(AbstractModuleRecord::initial_values()),
            ext: ModuleRecordExt::default(),
        })
    }

    /// `appendRequestedModule(moduleName, attributes, phase)`.
    pub fn append_requested_module(
        &self,
        module_name: &Identifier,
        attributes: Option<ScriptFetchParametersRef>,
        phase: ModulePhase,
    ) {
        self.m_requested_modules.borrow_mut().push(ModuleRequest {
            specifier: module_name.clone(),
            attributes,
            phase,
        });
    }

    /// `reserveCapacity(requestedModules, importEntries, exportEntries)` (`USE(BUN_JSC_ADDITIONS)`).
    pub fn reserve_capacity(&self, requested_modules: usize, import_entries: usize, export_entries: usize) {
        self.m_requested_modules.borrow_mut().reserve_exact(requested_modules);
        self.m_import_entries.borrow_mut().reserve_initial_capacity(import_entries);
        self.m_export_entries.borrow_mut().reserve_initial_capacity(export_entries);
    }

    /// `addStarExportEntry(moduleName, moduleRequestType)`.
    pub fn add_star_export_entry(&self, module_name: &Identifier, module_request_type: ScriptFetchParametersType) {
        self.m_star_export_entries.borrow_mut().add((module_name.impl_(), module_request_type));
    }

    /// `addImportEntry(entry)`: o parser garante que `localName` é novo.
    pub fn add_import_entry(&self, entry: &ImportEntry) {
        let is_new_entry = self.m_import_entries.borrow_mut().add(entry.local_name.impl_(), entry.clone());
        debug_assert!(is_new_entry, "Duplicate import entry name");
    }

    /// `addExportEntry(entry)`: o parser garante que `exportName` é novo.
    pub fn add_export_entry(&self, entry: &ExportEntry) {
        let is_new_entry = self.m_export_entries.borrow_mut().add(entry.export_name.impl_(), entry.clone());
        debug_assert!(is_new_entry, "Duplicate export entry name");
    }

    /// `tryGetImportEntry(localName)`.
    pub fn try_get_import_entry(&self, local_name: &UniquedStringKey) -> Option<ImportEntry> {
        self.m_import_entries.borrow().get(local_name).cloned()
    }

    /// `tryGetExportEntry(exportName)`.
    pub fn try_get_export_entry(&self, export_name: &UniquedStringKey) -> Option<ExportEntry> {
        self.m_export_entries.borrow().get(export_name).cloned()
    }

    /// `moduleKey()`.
    pub fn module_key(&self) -> &Identifier {
        &self.m_module_key
    }

    /// `moduleType()`.
    pub fn module_type(&self) -> ScriptFetchParametersType {
        match self.m_source_type {
            SourceProviderSourceType::Text => ScriptFetchParametersType::Text,
            SourceProviderSourceType::JSON => ScriptFetchParametersType::JSON,
            SourceProviderSourceType::Module
            | SourceProviderSourceType::Program
            | SourceProviderSourceType::Synthetic
            | SourceProviderSourceType::BunTranspiledModule => ScriptFetchParametersType::JavaScript,
            SourceProviderSourceType::WebAssembly => ScriptFetchParametersType::WebAssembly,
            SourceProviderSourceType::ImportMap => unreachable!("RELEASE_ASSERT_NOT_REACHED"),
        }
    }

    /// `requestedModules()`.
    pub fn requested_modules(&self) -> Ref<'_, Vec<ModuleRequest>> {
        self.m_requested_modules.borrow()
    }

    /// `loadedModules()`.
    pub fn loaded_modules(&self) -> &RefCell<ModuleMap<LoadedModuleRequest>> {
        &self.m_loaded_modules
    }

    /// `exportEntries()`.
    pub fn export_entries(&self) -> Ref<'_, ExportEntries> {
        self.m_export_entries.borrow()
    }

    /// `importEntries()`.
    pub fn import_entries(&self) -> Ref<'_, ImportEntries> {
        self.m_import_entries.borrow()
    }

    /// `starExportEntries()`.
    pub fn star_export_entries(&self) -> Ref<'_, StarExportEntries> {
        self.m_star_export_entries.borrow()
    }

    /// `asyncParentModules()`.
    pub fn async_parent_modules(&self) -> Ref<'_, Vec<AbstractModuleRecordRef>> {
        self.m_async_parent_modules.borrow()
    }

    /// `asyncEvaluationOrder()`.
    pub fn async_evaluation_order(&self) -> AsyncEvaluationOrder {
        self.m_async_evaluation_order.get()
    }

    /// `setAsyncEvaluationOrder(newOrder)`.
    pub fn set_async_evaluation_order(&self, new_order: AsyncEvaluationOrder) {
        self.m_async_evaluation_order.set(new_order);
    }

    /// `pendingAsyncDependencies()`.
    pub fn pending_async_dependencies(&self) -> Option<i32> {
        self.m_pending_async_dependencies.get()
    }

    /// `setPendingAsyncDependencies(newDependencies)`.
    pub fn set_pending_async_dependencies(&self, new_dependencies: Option<i32>) {
        self.m_pending_async_dependencies.set(new_dependencies);
    }

    /// `hasTLA()`.
    pub fn has_tla(&self) -> bool {
        self.m_has_tla.get()
    }

    /// `setHasTLA(has)`.
    pub fn set_has_tla(&self, has: bool) {
        self.m_has_tla.set(has);
    }

    /// `appendAsyncParentModule(vm, parentModule)`.
    pub fn append_async_parent_module(&self, parent_module: &AbstractModuleRecordRef) {
        self.m_async_parent_modules.borrow_mut().push(Rc::clone(parent_module));
    }

    /// `moduleEnvironment()`: o ASSERT do C++ vira `expect`.
    pub fn module_environment(&self) -> JSModuleEnvironmentRef {
        Rc::clone(self.m_module_environment.borrow().as_ref().expect("m_moduleEnvironment nulo"))
    }

    /// `moduleEnvironmentMayBeNull()`.
    pub fn module_environment_may_be_null(&self) -> Option<JSModuleEnvironmentRef> {
        self.m_module_environment.borrow().clone()
    }

    /// `setModuleEnvironment(globalObject, moduleEnvironment)`. Se o namespace já foi materializado, o
    /// slot `*namespace*` do ambiente novo recebe o objeto.
    pub fn set_module_environment(&self, module_environment: JSModuleEnvironmentRef) {
        debug_assert!(self.m_module_environment.borrow().is_none());
        if let Some(namespace) = self.ext.namespace.borrow().as_ref() {
            self.put_star_namespace(&module_environment, namespace);
        }
        *self.m_module_environment.borrow_mut() = Some(module_environment);
    }

    /// `m_moduleEnvironment.clear()`.
    pub(crate) fn clear_module_environment(&self) {
        *self.m_module_environment.borrow_mut() = None;
    }

    /// `internalField(Field)`.
    pub fn internal_field(&self, field: Field) -> JSValue {
        self.m_internal_fields.borrow()[field as usize]
    }

    /// `internalField(Field).set(value)`.
    pub fn set_internal_field(&self, field: Field, value: JSValue) {
        self.m_internal_fields.borrow_mut()[field as usize] = value;
    }

    /// `hostResolveImportedModule(globalObject, moduleName, moduleRequestType)`: consulta
    /// `m_loadedModules`; o `nullptr` é `None`.
    pub fn host_resolve_imported_module(
        &self,
        module_name: &Identifier,
        module_request_type: ScriptFetchParametersType,
    ) -> Option<AbstractModuleRecordRef> {
        let key: ModuleMapKey = (module_name.impl_(), module_request_type);
        self.m_loaded_modules.borrow().get(&key).and_then(|loaded| loaded.module.clone())
    }

    /// `setImportedModule(globalObject, request, record)`: reaproveita o `ModuleRequest` original
    /// (especificador e atributos), então o bucket é o de `request.type()`.
    pub fn set_imported_module(&self, request: &ModuleRequest, record: &AbstractModuleRecordRef) {
        let key: ModuleMapKey = (request.specifier.impl_(), request.type_(ScriptFetchParametersType::JavaScript));
        self.m_loaded_modules
            .borrow_mut()
            .insert(key, LoadedModuleRequest::new(request.clone(), Some(Rc::clone(record))));
    }

    /// `tryGetCachedResolution(exportName)`.
    pub(crate) fn try_get_cached_resolution(&self, export_name: &UniquedStringKey) -> Option<Resolution> {
        self.m_resolution_cache.borrow().get(export_name).cloned()
    }

    /// `cacheResolution(exportName, resolution)`: `HashMap::add` não sobrescreve.
    pub(crate) fn cache_resolution(&self, export_name: &UniquedStringKey, resolution: &Resolution) {
        self.m_resolution_cache
            .borrow_mut()
            .entry(export_name.clone())
            .or_insert_with(|| resolution.clone());
    }
}

/// O acesso por índice de `JSInternalFieldObjectImpl<2>` (`op_get_internal_field`/`op_put_internal_field`).
impl crate::runtime::js_internal_field_object_impl::InternalFields for AbstractModuleRecord {
    fn field_count(&self) -> u32 {
        self.m_internal_fields.borrow().len() as u32
    }

    fn field(&self, index: u32) -> JSValue {
        self.m_internal_fields.borrow()[index as usize]
    }

    fn set_field(&self, index: u32, value: JSValue) {
        self.m_internal_fields.borrow_mut()[index as usize] = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(Field::Frame as u32 + 1, NUMBER_OF_INTERNAL_FIELDS);
    }

    #[test]
    fn async_evaluation_order_states() {
        let mut order = AsyncEvaluationOrder::default();
        assert!(order.is_unset() && !order.has_order() && !order.is_done());
        order.set_order(3);
        assert!(order.has_order());
        assert_eq!(order.order(), 3);
        order.set_done();
        assert!(order.is_done() && !order.has_order());
        assert!(AsyncEvaluationOrder::done().is_done());
    }

    #[test]
    fn ordered_map_keeps_insertion_order_and_first_value() {
        let mut map = OrderedKeyMap::<u32>::default();
        assert!(map.add(None, 1));
        assert!(!map.add(None, 2));
        assert_eq!(map.get(&None), Some(&1));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn module_request_equality_ignores_phase() {
        let a = ModuleRequest { specifier: Identifier::default(), attributes: None, phase: ModulePhase::Evaluation };
        let b = ModuleRequest { specifier: Identifier::default(), attributes: None, phase: ModulePhase::Defer };
        assert_eq!(a, b);
    }
}
