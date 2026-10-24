//! `ObjectBindings.cpp` do bun 1.4.2: a leitura de opção que não enxerga o `Object.prototype`
//! (`getIfPropertyExistsPrototypePollutionMitigation`). A busca sobe a cadeia de protótipos do objeto de opções e para
//! antes do `Object.prototype` do global, para `Object.prototype.x = ...` não virar opção de ninguém.

use crate::runtime::host_call::Thrown;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::proxy_object::{object_get, slot_value};

/// `getIfPropertyExistsPrototypePollutionMitigation(globalObject, object, name)`: o valor (o getter roda com o objeto
/// de opções como `this`), ou `None` quando a propriedade não está no objeto nem nos protótipos antes do
/// `Object.prototype`.
pub(crate) fn get_if_property_exists_prototype_pollution_mitigation(
    global_object: &JSGlobalObject,
    object: &ObjectRef,
    property_name: &PropertyName,
) -> Result<Option<JSValue>, Thrown> {
    let object_prototype = global_object.object_prototype().as_value();
    let mut current = object.clone();
    loop {
        if current.type_() == JSType::ProxyObjectType {
            // `ProxyObject::getOwnPropertySlot` com `InternalMethodType::Get` é o `performGet`: o trap `get` responde,
            // e a busca para nele (medido no bun 1.4.2: só os `get` do trap aparecem).
            return object_get(global_object, &current, property_name, object.as_value()).map(Some);
        }
        let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::Get);
        if current.get_own_property_slot(global_object, property_name, &mut slot) {
            return slot_value(global_object, &slot, property_name).map(Some);
        }
        if global_object.vm().exception().is_some() {
            return Err(Thrown::Pending);
        }
        let prototype = current.get_prototype(global_object)?;
        if !prototype.is_object() || prototype == object_prototype {
            return Ok(None);
        }
        let Some(next) = ObjectRef::from_value(&prototype) else { return Ok(None) };
        current = next;
    }
}
