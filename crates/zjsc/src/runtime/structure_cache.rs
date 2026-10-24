//! Porte de `runtime/StructureCache.h/.cpp` e `runtime/PrototypeKey.h`: a estrutura canônica com que um
//! objeto é alocado quando herda de um dado protótipo.
//!
//! Divergências:
//!
//! - `WeakGCMap<PrototypeKey, Structure>` vira um `HashMap` forte: sem coleta, a entrada nunca morre.
//!   O `m_lock` some (um fio só) e `emptyObjectStructureConcurrently` (consulta de compilador
//!   concorrente) fica sem porte, porque não há compilador.
//! - A chave `PrototypeKey` guarda a identidade do protótipo (`cell_id`), a do `FunctionExecutable*`,
//!   o `inlineCapacity` e o `ClassInfo*` (o endereço da `static`).
//! - `makePolyProtoStructure` e o `FunctionExecutable*` entram só com o poly proto
//!   (`Structure::create(PolyProto, ...)` ainda não existe): quem pede poly proto cai na `assert!`.
//! - `Structure::create` recebe o `JSGlobalObject*` (o `m_realm`).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::indexing_type::{
    has_indexed_properties, IndexingType, INDEXING_SHAPE_MASK, SLOW_PUT_ARRAY_STORAGE_SHAPE,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSObject, JS_FINAL_OBJECT_INFO};
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::structure::{Structure, StructureRef};

/// `enum class ShouldCacheStructure : bool { No, Yes }` (`StructureCache.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShouldCacheStructure {
    No,
    Yes,
}

/// `class PrototypeKey`: `m_prototype` (0 é o `nullptr` do poly proto), `m_executable` (0 sem executable),
/// `m_inlineCapacity` e `m_classInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrototypeKey {
    prototype: usize,
    executable: usize,
    inline_capacity: u32,
    class_info: usize,
}

impl PrototypeKey {
    pub fn new(prototype: Option<&JSObject>, executable: usize, inline_capacity: u32, class_info: &'static ClassInfo) -> PrototypeKey {
        PrototypeKey {
            prototype: prototype.map_or(0, |prototype| prototype.cell_id()),
            executable,
            inline_capacity,
            class_info: class_info as *const ClassInfo as usize,
        }
    }
}

/// `class StructureCache`.
#[derive(Debug, Default)]
pub struct StructureCache {
    structures: RefCell<HashMap<PrototypeKey, StructureRef>>,
}

impl StructureCache {
    /// `clear()`.
    pub fn clear(&self) {
        self.structures.borrow_mut().clear();
    }

    /// `forEach(functor)` como um instantâneo das estruturas (o functor pode limpar o cache).
    pub fn structures(&self) -> Vec<StructureRef> {
        self.structures.borrow().values().cloned().collect()
    }

    /// `createEmptyStructure` (privado), sem o `FunctionExecutable*`.
    fn create_empty_structure(
        &self,
        global_object: &JSGlobalObject,
        prototype: &JSObject,
        type_info: TypeInfo,
        class_info: &'static ClassInfo,
        indexing_type: IndexingType,
        inline_capacity: u32,
        make_poly_proto_structure: bool,
        should_cache_structure: ShouldCacheStructure,
    ) -> StructureRef {
        // Sem `Structure::create(PolyProto, ...)` neste porte (ver o topo).
        assert!(!make_poly_proto_structure, "StructureCache: poly proto ainda não portado");
        let vm = global_object.vm();

        // Toda entrada mono-proto foi inserida logo depois de `didBecomePrototype()` marcar o protótipo,
        // e o bit nunca é limpo: um objeto que nunca foi protótipo não pode estar na tabela.
        let may_be_in_cache = prototype.may_be_prototype();

        let key = PrototypeKey::new(Some(prototype), 0, inline_capacity, class_info);
        if may_be_in_cache {
            if let Some(structure) = self.structures.borrow().get(&key) {
                debug_assert!(structure.has_mono_proto());
                debug_assert!(prototype.may_be_prototype());
                return structure.clone();
            }
        }

        prototype.did_become_prototype(vm);

        let structure = Structure::create_with_indexing_type(
            vm,
            Some(global_object),
            prototype.as_value(),
            type_info,
            class_info,
            indexing_type,
            inline_capacity,
        );
        if should_cache_structure == ShouldCacheStructure::Yes {
            self.structures.borrow_mut().insert(key, structure.clone());
        }
        structure
    }

    /// `emptyStructureForPrototypeFromBaseStructure(globalObject, prototype, baseStructure,
    /// shouldCacheStructure)`.
    pub fn empty_structure_for_prototype_from_base_structure(
        &self,
        global_object: &JSGlobalObject,
        prototype: &JSObject,
        base_structure: &StructureRef,
        should_cache_structure: ShouldCacheStructure,
    ) -> StructureRef {
        // We currently do not have inline capacity static analysis for subclasses and all internal function
        // constructors have a default inline capacity of 0.
        let mut indexing_type = base_structure.indexing_type();
        if prototype.any_object_in_chain_may_intercept_indexed_accesses() && has_indexed_properties(indexing_type) {
            indexing_type = (indexing_type & !INDEXING_SHAPE_MASK) | SLOW_PUT_ARRAY_STORAGE_SHAPE;
        }

        self.create_empty_structure(
            global_object,
            prototype,
            base_structure.type_info(),
            base_structure.class_info(),
            indexing_type,
            0,
            false,
            should_cache_structure,
        )
    }

    /// `emptyObjectStructureForPrototype(globalObject, prototype, inlineCapacity, makePolyProtoStructure)`.
    pub fn empty_object_structure_for_prototype(
        &self,
        global_object: &JSGlobalObject,
        prototype: &JSObject,
        inline_capacity: u32,
        make_poly_proto_structure: bool,
    ) -> StructureRef {
        self.create_empty_structure(
            global_object,
            prototype,
            JSFinalObject::type_info(),
            &JS_FINAL_OBJECT_INFO,
            JSFinalObject::DEFAULT_INDEXING_TYPE,
            inline_capacity,
            make_poly_proto_structure,
            ShouldCacheStructure::Yes,
        )
    }
}
