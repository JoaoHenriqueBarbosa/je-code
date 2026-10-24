//! Porte de `runtime/BigIntPrototype.h`, `BigIntPrototypeInlines.h` e `BigIntPrototype.cpp`: o
//! `BigInt.prototype` (um `JSNonFinalObject` comum) com `toString` (radix), `toLocaleString` e
//! `valueOf`.
//!
//! DIVERGÊNCIAS e lacunas:
//!
//! - `bigIntPrototypeTable` (`toString`, `toLocaleString`, `valueOf`) fica no `ClassInfo` e a `Structure`
//!   leva `HasStaticPropertyTable`: as três reificam no primeiro acesso; só o `Symbol.toStringTag`
//!   (`JSC_TO_STRING_TAG_WITHOUT_TRANSITION`) nasce no `finishCreation`.
//! - `toLocaleString` é o do C++: `IntlNumberFormat::create` + `initializeNumberFormat` com `locales` e
//!   `options` (`intl_number_format.rs`, locales en e pt-BR) e o `format` do BigInt exato; sem
//!   argumentos é o `defaultNumberFormat()` (en-US: agrupamento de milhar por vírgula).
//! - O `Integrity::auditStructureID` do C++ é só auditoria de memória e não existe aqui; o
//!   `singleCharacterString` do `toString` de um dígito é cache de `SmallStrings`, sem efeito
//!   observável.

use crate::host_function;
use crate::runtime::bigint_object::BigIntObject;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_number_format::{numeric_input, to_locale_string};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_big_int_ops::big_int_of;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::JSValue;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::number_prototype::extract_to_string_radix_argument;
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo BigIntPrototype::s_info`.
pub static BIG_INT_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "BigInt",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&BIG_INT_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `bigIntPrototypeTableValues` de `BigIntPrototype.lut.h`, na ordem do `@begin`.
static BIG_INT_PROTOTYPE_TABLE_VALUES: [HashTableValue; 3] = [
    native_entry("toString", big_int_proto_host_to_string, 0),
    native_entry("toLocaleString", big_int_proto_host_to_locale_string, 0),
    native_entry("valueOf", big_int_proto_host_value_of, 0),
];

/// `bigIntPrototypeTable`.
static BIG_INT_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &BIG_INT_PROTOTYPE_TABLE_VALUES };

const THIS_BIG_INT_TYPE_ERROR: &str = "'this' value must be a BigInt or BigIntObject";

/// `toThisBigIntValue(globalObject, thisValue)`: o `JSValue` do BigInt de `this` (o próprio valor, ou o
/// valor interno de um `BigIntObject`), ou o `TypeError`.
fn to_this_big_int_value(this_value: JSValue) -> Result<JSValue, Thrown> {
    if this_value.is_big_int() {
        return Ok(this_value);
    }

    if let Some(big_int_object) = BigIntObject::from_value(&this_value) {
        let big_int = big_int_object.internal_value();
        debug_assert!(big_int.is_big_int());
        return Ok(big_int);
    }

    Err(Thrown::type_error(THIS_BIG_INT_TYPE_ERROR))
}

fn big_int_proto_func_to_string(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = to_this_big_int_value(call.this_value())?;
    let big_int = big_int_of(value).expect("toThisBigIntValue devolveu um valor que não é BigInt");

    let radix = extract_to_string_radix_argument(call.argument(0))?;

    let result_string = big_int.to_string(global_object, radix as u32);
    if global_object.vm().exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &result_string)))
}

fn big_int_proto_func_to_locale_string(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = to_this_big_int_value(call.this_value())?;
    // `toIntlMathematicalValue(globalObject, thisValue)`: o BigInt exato.
    let input = numeric_input(global_object, value)?;
    let text = to_locale_string(global_object, call.argument(0), call.argument(1), &input)?;
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(text.as_bytes()))))
}

fn big_int_proto_func_value_of(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    to_this_big_int_value(call.this_value())
}

host_function!(big_int_proto_host_to_string, big_int_proto_func_to_string);
host_function!(big_int_proto_host_to_locale_string, big_int_proto_func_to_locale_string);
host_function!(big_int_proto_host_value_of, big_int_proto_func_value_of);

/// `class BigIntPrototype final : public JSNonFinalObject`.
pub struct BigIntPrototype;

impl BigIntPrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)` (`BigIntPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, BigIntPrototype::STRUCTURE_FLAGS),
            &BIG_INT_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `BigIntPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        BigIntPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`.
    fn finish_creation(prototype: &JSObject, vm: &VM, _global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        prototype.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol),
            JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(BIG_INT_PROTOTYPE_S_INFO.class_name.as_bytes()))),
            DONT_ENUM | READ_ONLY,
        );
        prototype.structure().set_may_be_prototype(true);
        // `toString`, `toLocaleString` e `valueOf` (`bigIntPrototypeTable`) reificam no primeiro acesso.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::default_number_format::NumericInput;

    #[test]
    fn locale_grouping_uses_commas_every_three_digits() {
        let format = |negative: bool, digits: &str| -> String {
            let input = NumericInput::Decimal { negative, digits: digits.to_string() };
            crate::runtime::default_number_format::format_to_string(
                &crate::runtime::default_number_format::NumberSettings::defaults(crate::runtime::intl_locale_data::Language::English),
                &input,
            )
        };
        assert_eq!(format(false, "0"), "0");
        assert_eq!(format(false, "999"), "999");
        assert_eq!(format(false, "1000"), "1,000");
        assert_eq!(format(true, "1234567"), "-1,234,567");
        assert_eq!(format(false, "123456"), "123,456");
    }
}
