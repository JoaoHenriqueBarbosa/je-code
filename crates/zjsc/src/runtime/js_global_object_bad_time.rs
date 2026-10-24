//! Porte de `JSGlobalObject::haveABadTime` e dos auxiliares do namespace privado de `JSGlobalObject.cpp`
//! (`GlobalObjectDependencyFinder`, `ObjectsWithBrokenIndexingFinder` nos dois modos), com
//! `isHavingABadTime`, `clearStructureCache` e `fireWatchpointAndMakeAllArrayStructuresSlowPut`.
//!
//! A varredura do heap (`forEachLiveCell`) é a do `cell_registry` (`live_cells`): cada objeto vivo que o
//! registro conhece passa pelo finder. Só os `JSObject` que podem ter indexação (função, objeto comum e
//! as demais células-objeto, mais o `JSGlobalObject` pelo ramo dele) entram; o escopo léxico, o callee e
//! afins têm indexação zero e nunca seriam achados.
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//! - `m_havingABadTimeWatchpointSet` e `m_structureCacheClearedWatchpointSet` são o `bool`
//!   `having_a_bad_time` e a limpeza do `StructureCache` (não há JIT nem watchpoint de compilador a
//!   disparar).
//! - A tabela `m_arrayStructureForIndexingShapeDuringAllocation` do `JSGlobalObject` passa a ser toda a
//!   estrutura de `ArrayWithSlowPutArrayStorage` (`make_array_structures_during_allocation_slow_put`).
//!   Faltam as estruturas `SlowPut` de `RegExpMatchesArray` e de `ClonedArguments`, que o porte não guarda
//!   por forma: o `RegExpMatchesArray` nasce da estrutura de array do realm.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::indexing_type::{has_slow_put_array_storage, IndexingType};
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_object::JSObject;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `hasBrokenIndexing(IndexingType)`: tem indexação e não é `SlowPutArrayStorage`.
fn has_broken_indexing(indexing_type: IndexingType) -> bool {
    indexing_type != 0 && !has_slow_put_array_storage(indexing_type)
}

/// O `JSObject*` de uma célula viva que o finder visita (`isJSCellKind(kind) && cell->isObject()`).
fn object_of(entry: &CellEntry) -> Option<&JSObject> {
    match entry {
        CellEntry::Function(function) => {
            let object: &JSObject = function;
            Some(object)
        }
        CellEntry::Scope(JSScopeRef::GlobalObject(global_object)) => {
            let object: &JSObject = global_object;
            Some(object)
        }
        other => other.as_js_object(),
    }
}

/// `object->realmMayBeNull()` como o `cell_id` do `JSGlobalObject`.
fn realm_id(object: &JSObject) -> Option<usize> {
    object.structure().realm().map(|global_object| global_object.cell_id())
}

/// `asObject(object->getPrototypeDirect())`; `None` é o fim da cadeia (`null`).
fn prototype_of(object: &JSObject) -> Option<ObjectRef> {
    ObjectRef::from_value(&object.get_prototype_direct())
}

/// O `JSGlobalObject` de um `cell_id`.
fn global_object_by_id(cell_id: usize) -> Option<JSGlobalObjectRef> {
    match cell_registry::get(cell_id)? {
        CellEntry::Scope(JSScopeRef::GlobalObject(global_object)) => Some(global_object),
        _ => None,
    }
}

/// `GlobalObjectDependencyFinder`: quais globais dependem de qual (o global `A` depende do `B` quando um
/// objeto de `A` tem um protótipo de `B` na cadeia).
#[derive(Default)]
struct GlobalObjectDependencyFinder {
    dependencies: HashMap<usize, HashSet<usize>>,
}

impl GlobalObjectDependencyFinder {
    /// `dependentsFor(key)`.
    fn dependents_for(&self, key: usize) -> Option<&HashSet<usize>> {
        self.dependencies.get(&key)
    }

    /// `visit(JSObject*)`.
    fn visit(&mut self, object: &JSObject) {
        if !object.may_be_prototype() {
            return;
        }
        let Some(object_global_object) = realm_id(object) else {
            debug_assert!(object.get_prototype_direct().is_null());
            return;
        };

        let mut chain: Option<ObjectRef> = None;
        loop {
            let current: &JSObject = match &chain {
                Some(current) => current,
                None => object,
            };
            let Some(prototype) = prototype_of(current) else { return };
            if let Some(proto_global_object) = realm_id(&prototype) {
                if proto_global_object != object_global_object {
                    self.dependencies.entry(proto_global_object).or_default().insert(object_global_object);
                }
            }
            chain = Some(prototype);
        }
    }
}

/// `BadTimeFinderMode` com o que cada modo carrega (`m_globalObject` ou `m_globalObjects`).
enum BadTimeScope<'a> {
    SingleGlobal(&'a JSGlobalObject),
    MultipleGlobals(&'a HashSet<usize>),
}

/// `ObjectsWithBrokenIndexingFinder<mode>`.
struct ObjectsWithBrokenIndexingFinder<'a> {
    scope: BadTimeScope<'a>,
    found_objects: Vec<CellEntry>,
    needs_multi_globals_scan: bool,
}

impl<'a> ObjectsWithBrokenIndexingFinder<'a> {
    fn new(scope: BadTimeScope<'a>) -> Self {
        ObjectsWithBrokenIndexingFinder { scope, found_objects: Vec::new(), needs_multi_globals_scan: false }
    }

    fn is_single_global(&self) -> bool {
        matches!(self.scope, BadTimeScope::SingleGlobal(_))
    }

    /// O realm (`cell_id` ou nenhum) é um dos globais afetados.
    fn is_affected_realm(&self, realm: Option<usize>) -> bool {
        match &self.scope {
            BadTimeScope::SingleGlobal(global_object) => realm == Some(global_object.cell_id()),
            BadTimeScope::MultipleGlobals(globals) => realm.is_some_and(|id| globals.contains(&id)),
        }
    }

    /// O lambda `isInAffectedGlobalObject`.
    fn is_in_affected_global_object(&mut self, object: &JSObject) -> bool {
        let single_global = self.is_single_global();
        let object_realm = realm_id(object);
        if single_global && self.is_affected_realm(object_realm) {
            return true;
        }
        let object_may_be_prototype = single_global && object.may_be_prototype();

        let mut chain: Option<ObjectRef> = None;
        loop {
            let current: &JSObject = match &chain {
                Some(current) => current,
                None => object,
            };
            let current_realm = realm_id(current);
            if single_global && object_may_be_prototype && current_realm != object_realm {
                self.needs_multi_globals_scan = true;
            }
            if self.is_affected_realm(current_realm) {
                return true;
            }
            match prototype_of(current) {
                Some(next) => chain = Some(next),
                None => return false,
            }
        }
    }

    /// O lambda `checkStructureHasRelevantGlobalObject`.
    fn check_structure_has_relevant_global_object(&mut self, structure: &StructureRef) -> bool {
        if !has_broken_indexing(structure.indexing_type()) {
            return false;
        }
        if self.is_affected_realm(structure.realm().map(|global_object| global_object.cell_id())) {
            return true;
        }
        let prototype = structure.stored_prototype();
        structure.has_mono_proto()
            && !prototype.is_null()
            && ObjectRef::from_value(&prototype).is_some_and(|prototype| self.is_in_affected_global_object(&prototype))
    }

    /// `visit(JSObject*)`: `true` é `IterationStatus::Done`.
    fn visit(&mut self, vm: &VM, entry: &CellEntry, object: &JSObject) -> bool {
        let single_global = self.is_single_global();

        if let CellEntry::Function(function) = entry {
            if let Some(rare_data) = function.rare_data() {
                if let Some(structure) = rare_data.internal_function_allocation_structure() {
                    let is_relevant_global_object = self.check_structure_has_relevant_global_object(&structure);
                    if single_global && self.needs_multi_globals_scan {
                        return true; // Bailing early and let the MultipleGlobals path handle everything.
                    }
                    if is_relevant_global_object {
                        rare_data.clear_internal_function_allocation_profile(vm, "have a bad time breaking internal function allocation");
                    }
                }
            }
        }

        if let CellEntry::Scope(JSScopeRef::GlobalObject(global_object)) = entry {
            // If this globalObject is already having a bad time, then structures in its StructureCache
            // does not affect on this new JSGlobalObject's haveABadTime since they are already slow mode.
            if !global_object.is_having_a_bad_time() {
                let mut will_clear = false;
                for structure in global_object.structure_cache().structures() {
                    let is_relevant_global_object = self.check_structure_has_relevant_global_object(&structure);
                    if single_global && self.needs_multi_globals_scan {
                        return true;
                    }
                    if is_relevant_global_object {
                        will_clear = true;
                    }
                }
                if single_global && self.needs_multi_globals_scan {
                    return true;
                }

                // StructureCache contains Structures which is no longer valid after relevant JSGlobalObject's haveABadTime.
                // We do not make such a JSGlobalObject status haveABadTime since still its own objects are intact.
                if will_clear {
                    global_object.clear_structure_cache(vm);
                }
            }
        }

        // Run this filter first, since it's cheap, and ought to filter out a lot of objects.
        if !has_broken_indexing(object.cell().indexing_type()) {
            return false;
        }
        if self.is_in_affected_global_object(object) {
            self.found_objects.push(entry.clone());
        }
        single_global && self.needs_multi_globals_scan
    }

    /// `forEachLiveCell(iterationScope, finder)`.
    fn scan_live_cells(&mut self, vm: &VM) {
        for entry in cell_registry::live_cells() {
            let Some(object) = object_of(&entry) else { continue };
            if self.visit(vm, &entry, object) {
                return;
            }
        }
    }
}

impl JSGlobalObject {
    /// `isHavingABadTime()`.
    pub fn is_having_a_bad_time(&self) -> bool {
        self.having_a_bad_time.get()
    }

    /// `clearStructureCache(vm)`.
    pub fn clear_structure_cache(&self, _vm: &VM) {
        self.structure_cache().clear(); // We may be caching array structures in here.
    }

    /// `fireWatchpointAndMakeAllArrayStructuresSlowPut(vm)` (ver as lacunas do cabeçalho).
    pub fn fire_watchpoint_and_make_all_array_structures_slow_put(&self, vm: &VM) {
        if self.is_having_a_bad_time() {
            return;
        }
        // This must happen first, because the compiler thread may race with haveABadTime.
        self.clear_structure_cache(vm);

        // Make sure that all JSArray allocations that load the appropriate structure from
        // this object now load a structure that uses SlowPut.
        self.make_array_structures_during_allocation_slow_put();
        self.having_a_bad_time.set(true);
    }

    /// `haveABadTime(vm)`.
    pub fn have_a_bad_time(&self, vm: &VM) {
        if self.is_having_a_bad_time() {
            return;
        }

        // Step 1: fire this global's HaveABadTime watchpoint and convert its array structures.
        self.fire_watchpoint_and_make_all_array_structures_slow_put(vm);

        // Step 2, optimistically assuming only this global is affected: the single-global scan.
        let mut finder = ObjectsWithBrokenIndexingFinder::new(BadTimeScope::SingleGlobal(self));
        finder.scan_live_cells(vm);
        let mut found_objects = std::mem::take(&mut finder.found_objects);

        if finder.needs_multi_globals_scan {
            // Find all globals that will also have a bad time as a side effect of this global having a bad time.
            let mut dependencies = GlobalObjectDependencyFinder::default();
            for entry in cell_registry::live_cells() {
                if let Some(object) = object_of(&entry) {
                    dependencies.visit(object);
                }
            }

            let mut globals_having_a_bad_time: HashSet<usize> = HashSet::new();
            let mut globals: VecDeque<usize> = VecDeque::from([self.cell_id()]);
            while let Some(global_id) = globals.pop_front() {
                let Some(global_object) = global_object_by_id(global_id) else { continue };
                global_object.fire_watchpoint_and_make_all_array_structures_slow_put(vm); // Step 1 above.
                if globals_having_a_bad_time.insert(global_id) {
                    if let Some(dependents) = dependencies.dependents_for(global_id) {
                        globals.extend(dependents.iter().copied());
                    }
                }
            }

            let mut finder = ObjectsWithBrokenIndexingFinder::new(BadTimeScope::MultipleGlobals(&globals_having_a_bad_time));
            finder.scan_live_cells(vm); // Step 2 above.
            found_objects = std::mem::take(&mut finder.found_objects);
        }

        while let Some(entry) = found_objects.pop() {
            let Some(object) = object_of(&entry) else { continue };
            debug_assert!(has_broken_indexing(object.cell().indexing_type()));
            object.switch_to_slow_put_array_storage(vm);
        }
    }
}
