//! Porte de `runtime/ShadowRealmObject.{h,cpp}` e `ShadowRealmObjectInlines.h`: a célula de `ShadowRealm`
//! (`JSNonFinalObject` que guarda o `JSGlobalObject` do reino sombra).
//!
//! DIVERGÊNCIAS:
//! - Sem GC não há `visitChildren`, `subspaceFor` nem `WriteBarrier`: o global é um `Rc`.
//! - `globalObject->globalObjectMethodTable()->deriveShadowRealmGlobalObject(globalObject)` é gancho do
//!   embedder (o JavaScriptCore não o define; o `jsc.cpp` cria um `GlobalObject` novo). O porte não tem
//!   `GlobalObjectMethodTable`, então `derive_shadow_realm_global_object` cria um global completo novo
//!   com `JSGlobalObject::init`, o mesmo que o shell faz.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo ShadowRealmObject::s_info`.
pub static SHADOW_REALM_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "ShadowRealm", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class ShadowRealmObject final : public JSNonFinalObject`.
pub struct ShadowRealmObject {
    base: JSNonFinalObject,
    /// `m_globalObject`.
    global_object: RefCell<Option<JSGlobalObjectRef>>,
}

/// A referência à célula, o `*` do C++.
pub type ShadowRealmObjectRef = Rc<ShadowRealmObject>;

impl std::ops::Deref for ShadowRealmObject {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

/// `JSGlobalObject::deriveShadowRealmGlobalObject` do embedder: um global novo, completo.
pub fn derive_shadow_realm_global_object(global_object: &JSGlobalObject) -> JSGlobalObjectRef {
    JSGlobalObject::init(&global_object.vm_rc())
}

impl ShadowRealmObject {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`: `TypeInfo(ShadowRealmType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ShadowRealmType, ShadowRealmObject::STRUCTURE_FLAGS),
            &SHADOW_REALM_OBJECT_S_INFO,
        )
    }

    /// `create(vm, structure, globalObject)`: o construtor, o `finishCreation` (`@@toStringTag`) e o
    /// `m_globalObject` derivado.
    pub fn create(vm: &VM, structure: &StructureRef, global_object: &JSGlobalObject) -> ShadowRealmObjectRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(ShadowRealmObject {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            global_object: RefCell::new(None),
        });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::ShadowRealm(Rc::clone(&cell)));
        // `finishCreation`: `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        put_to_string_tag(vm, &cell, SHADOW_REALM_OBJECT_S_INFO.class_name);
        *cell.global_object.borrow_mut() = Some(derive_shadow_realm_global_object(global_object));
        cell
    }

    /// `dynamicDowncast<ShadowRealmObject>(value)`.
    pub fn from_value(value: &JSValue) -> Option<ShadowRealmObjectRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::ShadowRealm(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// `globalObject()`.
    pub fn global_object(&self) -> JSGlobalObjectRef {
        self.global_object.borrow().clone().expect("ShadowRealmObject sem m_globalObject")
    }
}
