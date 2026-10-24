//! Tradução de `AbstractModuleRecord::resolveExport`, `resolveExportImpl`, `resolveImport` e
//! `ResolveQuery` (`AbstractModuleRecord.cpp`).
//!
//! DIVERGÊNCIAS: ver o cabeçalho de `abstract_module_record.rs`. Aqui, em particular, os métodos
//! recebem `self: &Rc<Self>` porque a `Resolution` guarda o `AbstractModuleRecord*` do próprio
//! registro (identidade por `Rc::ptr_eq`). `hostResolveImportedModule` devolvendo `nullptr` é
//! desreferência nula no C++ (os registros importados já foram carregados antes do `link`); aqui é
//! `expect`. Como `hostResolveImportedModule` não lança, o `ThrowScope` do C++ não aparece.
//! `ResolveQuery` guarda o registro por `Rc` e se compara e se espalha por identidade dele, como
//! `WTF::PtrHash` mais `IdentifierRepHash`. O `verbose` (`dataLog`) não existe.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::runtime::abstract_module_record::{
    AbstractModuleRecord, AbstractModuleRecordRef, ExportEntryType, ImportEntryType, Resolution, ResolutionType,
    UniquedStringKey,
};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;

/// `AbstractModuleRecord::ResolveQuery`.
#[derive(Clone)]
struct ResolveQuery {
    module_record: AbstractModuleRecordRef,
    export_name: UniquedStringKey,
}

impl PartialEq for ResolveQuery {
    /// `ResolveQuery::Hash::equal`.
    fn eq(&self, other: &ResolveQuery) -> bool {
        Rc::ptr_eq(&self.module_record, &other.module_record) && self.export_name == other.export_name
    }
}

impl Eq for ResolveQuery {}

impl Hash for ResolveQuery {
    /// `ResolveQuery::Hash::hash`.
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.module_record).hash(state);
        self.export_name.hash(state);
    }
}

/// O `Task::Type` local de `resolveExportImpl`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskType {
    Query,
    IndirectFallback,
    GatherStars,
}

/// O `Task` local de `resolveExportImpl`.
struct Task {
    query: ResolveQuery,
    type_: TaskType,
}

/// O estado que as lambdas de `resolveExportImpl` capturam por referência.
struct ResolveState {
    pending_tasks: Vec<Task>,
    frames: Vec<Resolution>,
    found_star_links: bool,
}

impl ResolveState {
    /// `resolveNonLocal(query)`: verdadeiro se não houve erro.
    fn resolve_non_local(&mut self, global_object: &JSGlobalObject, query: &ResolveQuery) -> bool {
        let vm = global_object.vm();
        // https://tc39.github.io/ecma262/#sec-resolveexport
        // section 15.2.1.16.3, step 6
        // If the "default" name is not resolved in the current module, we need to throw an error and stop resolution immediately,
        // Rationale to this error: A default export cannot be provided by an export *.
        if query.export_name == vm.property_names.default_keyword.impl_() {
            return false;
        }

        // Enqueue the task to gather the results of the stars.
        // And append the new Resolution frame to gather the local result of the stars.
        self.pending_tasks.push(Task { query: query.clone(), type_: TaskType::GatherStars });
        self.found_star_links = true;
        self.frames.push(Resolution::not_found());

        // Enqueue the tasks in reverse order.
        let star_entries: Vec<_> = query.module_record.star_export_entries().iter().rev().cloned().collect();
        for (star_module_name, star_module_request_type) in star_entries {
            let imported_module_record = query
                .module_record
                .host_resolve_imported_module(
                    &Identifier::from_uid(vm, star_module_name.as_ref()),
                    star_module_request_type,
                )
                .expect("módulo do export * não carregado");
            self.pending_tasks.push(Task {
                query: ResolveQuery { module_record: imported_module_record, export_name: query.export_name.clone() },
                type_: TaskType::Query,
            });
        }
        true
    }

    /// `mergeToCurrentTop(resolution)`: falso se há ambiguidade.
    fn merge_to_current_top(&mut self, resolution: &Resolution) -> bool {
        if resolution.type_ == ResolutionType::NotFound {
            return true;
        }

        let top = self.frames.last_mut().expect("frames vazio");
        if top.type_ == ResolutionType::NotFound {
            *top = resolution.clone();
            return true;
        }

        top.is_same_binding(resolution)
    }
}

/// `cacheResolutionForQuery(query, resolution)`.
fn cache_resolution_for_query(query: &ResolveQuery, resolution: &Resolution) {
    debug_assert!(resolution.type_ == ResolutionType::Resolved);
    query.module_record.cache_resolution(&query.export_name, resolution);
}

impl AbstractModuleRecord {
    /// `resolveExportImpl(globalObject, root)`. Os comentários longos sobre a estratégia de cache (cinco
    /// regras) estão em `AbstractModuleRecord.cpp`; o laço abaixo é o mesmo, com as três tarefas
    /// (`Query`, `IndirectFallback`, `GatherStars`) e os quadros de resolução local.
    fn resolve_export_impl(global_object: &JSGlobalObject, root: ResolveQuery) -> Resolution {
        let vm = global_object.vm();

        let mut state = ResolveState { pending_tasks: Vec::new(), frames: Vec::new(), found_star_links: false };
        let mut resolve_set: HashSet<ResolveQuery> = HashSet::new();

        state.frames.push(Resolution::not_found());

        state.pending_tasks.push(Task { query: root.clone(), type_: TaskType::Query });
        while let Some(task) = state.pending_tasks.pop() {
            let query = &task.query;

            match task.type_ {
                TaskType::Query => {
                    let module_record = Rc::clone(&query.module_record);

                    if !resolve_set.insert(task.query.clone()) {
                        continue;
                    }

                    //  5. Once we see star links, even if we have not yet traversed that star link path, we should disable caching.
                    if !module_record.star_export_entries().is_empty() {
                        state.found_star_links = true;
                    }

                    let Some(export_entry) = module_record.try_get_export_entry(&query.export_name) else {
                        // If there is no matched exported binding in the current module, we need to look
                        // into the stars. We don't probe m_resolutionCache here: the only writer that can
                        // populate (moduleRecord, exportName) while exportEntries has no match for exportName
                        // is the root-cache write (rule #1), which only fires when star traversal produced
                        // Resolved, which in turn requires moduleRecord to have non-empty starExportEntries.
                        if !state.resolve_non_local(global_object, &task.query) {
                            return Resolution::error();
                        }
                        continue;
                    };

                    match export_entry.type_ {
                        ExportEntryType::Local => {
                            debug_assert!(!export_entry.local_name.is_null());
                            let resolution = Resolution {
                                type_: ResolutionType::Resolved,
                                module_record: Some(Rc::clone(&module_record)),
                                local_name: export_entry.local_name.clone(),
                            };
                            if !state.merge_to_current_top(&resolution) {
                                return Resolution::ambiguous();
                            }
                        }

                        ExportEntryType::Indirect => {
                            //  4. Once we follow star links, we should not retrieve the result from the cache and should not cache the result.
                            if !state.found_star_links {
                                if let Some(cached_resolution) =
                                    module_record.try_get_cached_resolution(&query.export_name)
                                {
                                    if !state.merge_to_current_top(&cached_resolution) {
                                        return Resolution::ambiguous();
                                    }
                                    continue;
                                }
                            }

                            let imported_module_record = module_record
                                .host_resolve_imported_module(&export_entry.module_name, export_entry.module_request_type)
                                .expect("módulo do export indireto não carregado");

                            // When the imported module does not produce any resolved binding, we need to look into the stars in the *current*
                            // module. To do this, we append the `IndirectFallback` task to the task queue.
                            state.pending_tasks.push(Task { query: task.query.clone(), type_: TaskType::IndirectFallback });
                            // And append the new Resolution frame to check the indirect export will be resolved or not.
                            state.frames.push(Resolution::not_found());
                            state.pending_tasks.push(Task {
                                query: ResolveQuery {
                                    module_record: imported_module_record,
                                    export_name: export_entry.import_name.impl_(),
                                },
                                type_: TaskType::Query,
                            });
                        }

                        ExportEntryType::Namespace => {
                            let imported_module_record = module_record
                                .host_resolve_imported_module(&export_entry.module_name, export_entry.module_request_type)
                                .expect("módulo do export de namespace não carregado");
                            let resolution = Resolution {
                                type_: ResolutionType::Resolved,
                                module_record: Some(imported_module_record),
                                local_name: vm.property_names.star_namespace_private_name.clone(),
                            };
                            if !state.merge_to_current_top(&resolution) {
                                return Resolution::ambiguous();
                            }
                        }
                    }
                }

                TaskType::IndirectFallback => {
                    let resolution = state.frames.pop().expect("frames vazio");

                    if resolution.type_ == ResolutionType::NotFound {
                        // Indirect export entry does not produce any resolved binding.
                        // So we will investigate the stars.
                        if !state.resolve_non_local(global_object, &task.query) {
                            return Resolution::error();
                        }
                        continue;
                    }

                    debug_assert!(
                        resolution.type_ == ResolutionType::Resolved,
                        "When we see Error and Ambiguous, we immediately return from this loop. So here, only Resolved comes."
                    );

                    //  3. If we don't follow any star links during the resolution, we can see all the traced nodes are cacheable.
                    //  4. Once we follow star links, we should not retrieve the result from the cache and should not cache the result.
                    if !state.found_star_links {
                        cache_resolution_for_query(&task.query, &resolution);
                    }

                    // If indirect export entry produces Resolved, we should merge it to the upper frame.
                    // And do not investigate the stars of the current module.
                    if !state.merge_to_current_top(&resolution) {
                        return Resolution::ambiguous();
                    }
                }

                TaskType::GatherStars => {
                    let resolution = state.frames.pop().expect("frames vazio");
                    debug_assert!(
                        resolution.type_ == ResolutionType::Resolved || resolution.type_ == ResolutionType::NotFound,
                        "When we see Error and Ambiguous, we immediately return from this loop. So here, only Resolved and NotFound comes."
                    );

                    // Merge the star resolution to the upper frame.
                    if !state.merge_to_current_top(&resolution) {
                        return Resolution::ambiguous();
                    }
                }
            }
        }

        debug_assert!(state.frames.len() == 1);
        //  1. The starting point is always cacheable.
        if state.frames[0].type_ == ResolutionType::Resolved {
            cache_resolution_for_query(&root, &state.frames[0]);
        }
        state.frames.swap_remove(0)
    }

    /// `resolveExport(globalObject, exportName)`.
    pub fn resolve_export(self: &Rc<Self>, global_object: &JSGlobalObject, export_name: &Identifier) -> Resolution {
        let vm = global_object.vm();
        let export_name_key = export_name.impl_();

        // Local / Namespace exports are trivially derivable from m_exportEntries.
        // m_resolutionCache only holds results that actually amortise costly traversals, Indirect resolutions and star-resolved results.
        if let Some(entry) = self.try_get_export_entry(&export_name_key) {
            match entry.type_ {
                ExportEntryType::Local => {
                    debug_assert!(!entry.local_name.is_null());
                    return Resolution {
                        type_: ResolutionType::Resolved,
                        module_record: Some(Rc::clone(self)),
                        local_name: entry.local_name,
                    };
                }
                ExportEntryType::Namespace => {
                    let imported_module_record = self
                        .host_resolve_imported_module(&entry.module_name, entry.module_request_type)
                        .expect("módulo do export de namespace não carregado");
                    return Resolution {
                        type_: ResolutionType::Resolved,
                        module_record: Some(imported_module_record),
                        local_name: vm.property_names.star_namespace_private_name.clone(),
                    };
                }
                ExportEntryType::Indirect => {
                    if let Some(cached_resolution) = self.try_get_cached_resolution(&export_name_key) {
                        return cached_resolution;
                    }
                }
            }
        } else if !self.star_export_entries().is_empty() {
            // When there is no matching export entry, cache can exist only when we found star-resolved results.
            // Thus, if there is no star export entries, cache never exists.
            if let Some(cached_resolution) = self.try_get_cached_resolution(&export_name_key) {
                return cached_resolution;
            }
        }

        AbstractModuleRecord::resolve_export_impl(
            global_object,
            ResolveQuery { module_record: Rc::clone(self), export_name: export_name_key },
        )
    }

    /// `resolveImport(globalObject, localName)`.
    pub fn resolve_import(self: &Rc<Self>, global_object: &JSGlobalObject, local_name: &Identifier) -> Resolution {
        let Some(import_entry) = self.try_get_import_entry(&local_name.impl_()) else {
            return Resolution::not_found();
        };

        if import_entry.type_ == ImportEntryType::Namespace {
            return Resolution::not_found();
        }

        let imported_module: Option<AbstractModuleRecordRef> =
            self.host_resolve_imported_module(&import_entry.module_request, import_entry.module_request_type);
        imported_module
            .expect("módulo importado não carregado")
            .resolve_export(global_object, &import_entry.import_name)
    }
}
