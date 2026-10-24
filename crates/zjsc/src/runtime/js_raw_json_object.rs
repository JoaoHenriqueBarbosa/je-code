//! Tradução de `runtime/JSRawJSONObject.{h,cpp}`: o objeto que `JSON.rawJSON` devolve, congelado, com
//! protótipo `null` e uma única propriedade `rawJSON` (somente leitura, não configurável) que guarda o
//! texto JSON.
//!
//! DIVERGÊNCIAS:
//!
//! - A classe não tem campos próprios (o texto vive na propriedade fora de linha `rawJSON`), então o
//!   objeto é um `JSObject` comum registrado como `CellEntry::Object`, como o `MathObject`. A pergunta
//!   `inherits<JSRawJSONObject>()` é a comparação do `ClassInfo` da `Structure`.
//! - `tryCreate` devolve `nullptr` quando o `Butterfly` não aloca; aqui a alocação não falha, então
//!   `create` não tem o ramo de falta de memória.
//! - `JSGlobalObject::rawJSONObjectStructure()` é um `LazyProperty` do global (`js_global_object.rs`,
//!   fora do alcance desta fatia): quem cria o objeto monta a `Structure` com `create_structure`.

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectHandle, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::JSStringRef;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::{DONT_DELETE, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::FIRST_OUT_OF_LINE_OFFSET;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSRawJSONObject::s_info` (`"Object"`).
pub static JS_RAW_JSON_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `rawJSONObjectRawJSONPropertyOffset`.
pub const RAW_JSON_OBJECT_RAW_JSON_PROPERTY_OFFSET: i32 = FIRST_OUT_OF_LINE_OFFSET;

/// `class JSRawJSONObject final : public JSNonFinalObject`.
pub struct JSRawJSONObject;

impl JSRawJSONObject {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`: a `Structure` já traz a propriedade `rawJSON`
    /// (`ReadOnly | DontDelete`, no primeiro deslocamento fora de linha) e não é extensível.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        let structure = Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, JSRawJSONObject::STRUCTURE_FLAGS),
            &JS_RAW_JSON_OBJECT_S_INFO,
        );
        let offset = structure.add_property_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.raw_json),
            READ_ONLY | DONT_DELETE,
        );
        assert_eq!(offset, RAW_JSON_OBJECT_RAW_JSON_PROPERTY_OFFSET);
        structure.set_did_prevent_extensions(true);
        structure
    }

    /// `tryCreate(vm, structure, string)` mais o `finishCreation`: grava o texto no deslocamento fixo.
    pub fn create(vm: &VM, structure: &StructureRef, string: JSStringRef) -> JSObjectRef {
        let object = JSObject::allocate(vm, structure);
        object.finish_creation(vm);
        object.put_direct_offset(vm, RAW_JSON_OBJECT_RAW_JSON_PROPERTY_OFFSET, JSValue::from_js_string(string));
        object
    }

    /// `value.inherits<JSRawJSONObject>()`.
    pub fn from_value(value: &JSValue) -> Option<JSObjectHandle> {
        JSObject::from_value(value).filter(|object| object.cell().inherits(&JS_RAW_JSON_OBJECT_S_INFO))
    }

    /// `rawJSON(vm)`: a propriedade é somente leitura e não configurável, então o deslocamento nunca muda.
    pub fn raw_json(object: &JSObject) -> JSStringRef {
        object.get_direct(RAW_JSON_OBJECT_RAW_JSON_PROPERTY_OFFSET).as_js_string()
    }
}
