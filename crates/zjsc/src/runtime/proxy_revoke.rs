//! Porte de `runtime/ProxyRevoke.{h,cpp}` e `ProxyRevokeInlines.h`: a função `revoke` que
//! `Proxy.revocable` devolve (um `InternalFunction` que guarda o `Proxy` a revogar).
//!
//! DIVERGÊNCIAS: `ProxyRevoke` é um `InternalFunction` embutido por composição (`InternalFunction::construct`
//! mais o registro próprio, `CellEntry::ProxyRevoke`), porque o estado `m_proxy` precisa viver na célula
//! que `callFrame->jsCallee()` devolve. `subspaceFor` e `visitChildren` somem (GC).

use std::cell::Cell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::internal_function::{InternalFunction, PropertyAdditionMode, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_null, js_undefined, JSValue};
use crate::runtime::proxy_object::ProxyObject;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo ProxyRevoke::s_info`.
pub static PROXY_REVOKE_S_INFO: ClassInfo =
    ClassInfo { class_name: "ProxyRevoke", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `performProxyRevoke`: revoga o `Proxy` guardado e esquece dele; chamar de novo não faz nada.
fn perform_proxy_revoke_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let proxy_revoke = ProxyRevoke::from_cell_id(call.callee()).expect("jsCallee de performProxyRevoke não é um ProxyRevoke");
    let proxy_value = proxy_revoke.proxy();
    if proxy_value.is_null() {
        return Ok(js_undefined());
    }

    let proxy = ProxyObject::from_value(&proxy_value).expect("ProxyRevoke guarda uma célula que não é ProxyObject");
    proxy.revoke();
    proxy_revoke.set_proxy_to_null();
    Ok(js_undefined())
}

crate::host_function!(perform_proxy_revoke, perform_proxy_revoke_body);

/// `class ProxyRevoke final : public InternalFunction`.
pub struct ProxyRevoke {
    base: InternalFunction,
    /// `m_proxy`: o `ProxyObject`, ou `null` depois de revogar.
    proxy: Cell<JSValue>,
}

/// O `ProxyRevoke*`.
pub type ProxyRevokeRef = Rc<ProxyRevoke>;

impl std::ops::Deref for ProxyRevoke {
    type Target = InternalFunction;

    fn deref(&self) -> &InternalFunction {
        &self.base
    }
}

impl ProxyRevoke {
    /// `visitChildren`: o proxy.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.proxy.get());
    }
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = InternalFunction::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)` (`ProxyRevokeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::InternalFunctionType, ProxyRevoke::STRUCTURE_FLAGS),
            &PROXY_REVOKE_S_INFO,
        )
    }

    /// `create(vm, structure, proxy)`: o construtor e o `finishCreation(vm)` (`length` 0, nome vazio).
    pub fn create(vm: &VM, structure: &StructureRef, proxy: &ProxyObject) -> ProxyRevokeRef {
        let cell_id = cell_registry::reserve();
        let revoke = Rc::new(ProxyRevoke {
            base: InternalFunction::construct(vm, StructureRef::clone(structure), perform_proxy_revoke, None),
            proxy: Cell::new(proxy.as_value()),
        });
        revoke.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::ProxyRevoke(Rc::clone(&revoke)));
        revoke.finish_creation(vm, 0, &WtfString::from_latin1(b""), PropertyAdditionMode::WithStructureTransition);
        revoke
    }

    /// `proxy()`.
    pub fn proxy(&self) -> JSValue {
        self.proxy.get()
    }

    /// `setProxyToNull(vm)`.
    pub fn set_proxy_to_null(&self) {
        self.proxy.set(js_null());
    }

    /// O `ProxyRevoke*` do `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<ProxyRevokeRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::ProxyRevoke(revoke)) => Some(revoke),
            _ => None,
        }
    }
}
