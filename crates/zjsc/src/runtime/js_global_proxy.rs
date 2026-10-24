//! Porte de `runtime/JSGlobalProxy.{h,cpp}` e `JSGlobalProxyInlines.h`: o objeto que o `globalThis` e o
//! `this` de nível superior devolvem. Ele não tem propriedades próprias: toda operação de objeto é
//! repassada ao `JSGlobalObject` alvo (`CellEntry::GlobalProxy`, `JSType::GlobalProxyType`).
//!
//! DIVERGÊNCIAS, e por quê:
//! - O `JSObject` do porte não tem tabela de métodos virtual (veja `proxy_object.rs`). Os métodos que o
//!   C++ sobrescreve (`getOwnPropertySlot`, `getOwnPropertySlotByIndex`, `put`, `putByIndex`,
//!   `deleteProperty`, `deletePropertyByIndex`, `defineOwnProperty`, `getOwnPropertyNames`,
//!   `isExtensible`, `preventExtensions`, `setPrototype`, `getPrototype`, e o `isThisValueAltered` de
//!   `JSGlobalProxy.h`) não moram aqui: cada ponto de despacho (`JSObject` em `js_object.rs`, e as
//!   funções `object_*` e `get_property_slot` de `proxy_object.rs`) pergunta [`target_of`] e, havendo
//!   alvo, refaz a mesma chamada sobre o `JSGlobalObject`, como o `thisObject->target()->methodTable()->...`
//!   do C++.
//! - `m_target` é um `JSGlobalObjectRef` (o `WriteBarrier<JSGlobalObject>` sem GC); a variante sem alvo
//!   (`create(vm, structure)`) existe, mas o despacho exige o alvo, como o C++ (que o desreferencia).
//! - `changeGlobalProxyTargetTransition` está em `Structure`; `DeferredStructureTransitionWatchpointFire`
//!   não existe.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{
    TypeInfo, INTERCEPTS_GET_OWN_PROPERTY_SLOT_BY_INDEX_EVEN_WHEN_LENGTH_IS_NOT_ZERO, OVERRIDES_GET_OWN_PROPERTY_NAMES,
    OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_GET_PROTOTYPE, OVERRIDES_IS_EXTENSIBLE, OVERRIDES_PUT,
};
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSGlobalProxy::s_info`.
pub static JS_GLOBAL_PROXY_S_INFO: ClassInfo =
    ClassInfo { class_name: "JSGlobalProxy", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSGlobalProxy : public JSNonFinalObject`.
pub struct JSGlobalProxy {
    base: JSNonFinalObject,
    /// `m_target`.
    target: RefCell<Option<JSGlobalObjectRef>>,
}

/// O `JSGlobalProxy*`.
pub type JSGlobalProxyRef = Rc<JSGlobalProxy>;

impl std::ops::Deref for JSGlobalProxy {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSGlobalProxy {
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot | OverridesGetOwnPropertyNames |
    /// OverridesPut | OverridesGetPrototype | OverridesIsExtensible |
    /// InterceptsGetOwnPropertySlotByIndexEvenWhenLengthIsNotZero`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS
        | OVERRIDES_GET_OWN_PROPERTY_SLOT
        | OVERRIDES_GET_OWN_PROPERTY_NAMES
        | OVERRIDES_PUT
        | OVERRIDES_GET_PROTOTYPE
        | OVERRIDES_IS_EXTENSIBLE
        | INTERCEPTS_GET_OWN_PROPERTY_SLOT_BY_INDEX_EVEN_WHEN_LENGTH_IS_NOT_ZERO;

    /// `createStructure(vm, globalObject, prototype)` (`JSGlobalProxyInlines.h`).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::GlobalProxyType, JSGlobalProxy::STRUCTURE_FLAGS),
            &JS_GLOBAL_PROXY_S_INFO,
        )
    }

    /// `create(vm, structure, globalObject)`: `target` é o `nullptr` da sobrecarga sem alvo quando `None`.
    pub fn create(vm: &VM, structure: StructureRef, target: Option<&JSGlobalObjectRef>) -> JSGlobalProxyRef {
        let cell_id = cell_registry::reserve();
        let proxy = Rc::new(JSGlobalProxy {
            base: JSNonFinalObject::new(vm, structure),
            target: RefCell::new(target.map(Rc::clone)),
        });
        proxy.base.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::GlobalProxy(Rc::clone(&proxy)));
        proxy.finish_creation(vm);
        proxy
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSGlobalProxyRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::GlobalProxy(proxy)) => Some(proxy),
            _ => None,
        }
    }

    /// `target()`.
    pub fn target(&self) -> Option<JSGlobalObjectRef> {
        self.target.borrow().clone()
    }

    /// Solta o alvo no desmonte do programa (`cell_registry::remove_all_of`): o global guarda o proxy
    /// (`m_globalThis`) e o proxy guarda o global, um ciclo de `Rc` que o `~VM` do C++ não tem.
    pub(crate) fn clear_target(&self) {
        let released = self.target.borrow_mut().take();
        drop(released);
    }

    /// `setTarget(vm, globalObject)`: o protótipo do alvo vira o do proxy e a `Structure` troca para a do
    /// novo realm.
    pub fn set_target(&self, vm: &VM, global_object: &JSGlobalObjectRef) {
        *self.target.borrow_mut() = Some(Rc::clone(global_object));
        let target_object: &JSObject = global_object;
        self.set_prototype_direct(vm, target_object.get_prototype_direct());
        let new_structure = Structure::change_global_proxy_target_transition(vm, &self.structure(), global_object);
        self.set_structure(vm, &new_structure);
    }
}

/// O alvo de `object` se ele é um `JSGlobalProxy` (`uncheckedDowncast<JSGlobalProxy>(object)->target()`):
/// o ponto único que cada despacho consulta antes de repassar a operação ao `JSGlobalObject`.
pub fn target_of(object: &JSObject) -> Option<JSGlobalObjectRef> {
    if object.type_() != JSType::GlobalProxyType {
        return None;
    }
    let proxy = JSGlobalProxy::from_cell_id(object.cell_id()).expect("célula do tipo GlobalProxyType que não é JSGlobalProxy");
    Some(proxy.target().expect("JSGlobalProxy sem alvo"))
}

/// `isThisValueAltered(slot, baseObject)` de `JSGlobalProxy.h`: o `this` do slot é o próprio objeto, ou o
/// `JSGlobalProxy` do `base_object` (o único tipo visto como igual ao alvo original).
pub fn is_this_value_altered(this_value: JSValue, base_object: &JSObject) -> bool {
    if this_value == base_object.as_value() {
        return false;
    }
    let Some(this_object) = JSObject::from_value(&this_value) else {
        return true;
    };
    match target_of(&this_object) {
        Some(target) => {
            let target_object: &JSObject = &target;
            target_object.cell_id() != base_object.cell_id()
        }
        None => true,
    }
}
