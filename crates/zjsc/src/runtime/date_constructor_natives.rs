//! As funções nativas do construtor `Date` (`DateConstructor.cpp`: `callDate`,
//! `constructWithDateConstructor`, `dateParse`, `dateNow` e `dateUTC`), o `DateConstructor::create` e a
//! instalação de `Date` no global (`m_dateStructure`, a `ClassStructure` de `JSGlobalObject.cpp`).
//!
//! DIVERGÊNCIAS:
//!
//! - `HasStaticPropertyTable` vale no `DateConstructor`: `parse`, `UTC` e `now` ficam em
//!   `DATE_CONSTRUCTOR_TABLE` (`dateConstructorTable`) e reificam no primeiro acesso; `create` põe só
//!   `length`, `name` e `prototype`, como o `finishCreation`.
//! - `JSGlobalObject::jsDateNow()` lê o `overridenDateNow` do `bun test`, que o global do porte não tem;
//!   sem substituição (`NaN`), é o relógio (`js_date_now`).
//! - `dateStructure()` é o acessor do campo `date_structure` do global, preenchido por [`install_date`].

use crate::host_function;
use crate::runtime::date_constructor::{
    construct_date, date_parse, format_date_time_now, js_date_now, milliseconds_from_components, DateConstructor,
    OneArgument, DATE_CONSTRUCTOR_LENGTH, PROTOTYPE_ATTRIBUTES,
};
use crate::runtime::date_instance::DateInstance;
use crate::runtime::date_prototype_natives::{create_date_prototype, install_date_prototype_to_primitive, number_value, string_value};
use crate::runtime::host_call::{pending_or, HostCall, HostResult};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry_with_intrinsic};
use crate::runtime::internal_function::{get_derived_structure_in_realm, InternalFunction, InternalFunctionRef, PropertyAdditionMode};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;
use crate::wtf::date_math::TimeType;

/// `JSGlobalObject::jsDateNow()` sem o `overridenDateNow` do Bun (`NaN` é "sem substituição").
const NO_DATE_NOW_OVERRIDE: f64 = f64::NAN;

/// `callDate` (ECMA 15.9.2): `Date()` sem `new` é o `toString()` do instante atual.
fn call_date_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let text = format_date_time_now(vm.date_cache(), js_date_now(NO_DATE_NOW_OVERRIDE));
    Ok(string_value(vm, &text))
}

/// `constructWithDateConstructor` (ECMA 15.9.3), com o `constructDate`.
fn construct_with_date_constructor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let cache = vm.date_cache();

    let one_argument = if call.argument_count() == 1 {
        let arg0 = call.argument(0);
        Some(match DateInstance::from_value(&arg0) {
            Some(date_instance) => OneArgument::DateInstance(date_instance.internal_number()),
            None => {
                let primitive = pending_or(global_object, arg0.to_primitive())?;
                if primitive.is_string() {
                    OneArgument::String(primitive.as_js_string().value())
                } else {
                    OneArgument::Number(pending_or(global_object, primitive.to_number())?)
                }
            }
        })
    } else {
        None
    };

    let mut convert = |index: usize| pending_or(global_object, call.argument(index).to_number());
    let value = construct_date(cache, call.argument_count(), js_date_now(NO_DATE_NOW_OVERRIDE), one_argument, &mut convert)?;

    let date_structure =
        get_derived_structure_in_realm(global_object, call.new_target(), call.callee(), |realm| realm.date_structure())?;
    Ok(DateInstance::create(vm, date_structure, value).as_value())
}

/// `dateParse`: `Date.parse(string)`.
fn date_parse_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let date_string = pending_or(global_object, call.argument(0).to_wtf_string())?;
    Ok(number_value(date_parse(global_object.vm().date_cache(), &date_string)?))
}

/// `dateNow`: `Date.now()`.
fn date_now_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(number_value(js_date_now(NO_DATE_NOW_OVERRIDE)))
}

/// `dateUTC`: `Date.UTC(...)`, cada argumento pelo `toNumber`.
fn date_utc_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let mut convert = |index: usize| pending_or(global_object, call.argument(index).to_number());
    let time =milliseconds_from_components(global_object.vm().date_cache(), call.argument_count(), TimeType::UTCTime, &mut convert)?;
    Ok(number_value(time))
}

host_function!(call_date, call_date_body);
host_function!(construct_with_date_constructor, construct_with_date_constructor_body);
host_function!(date_parse_function, date_parse_body);
host_function!(date_now_function, date_now_body);
host_function!(date_utc_function, date_utc_body);

/// `dateConstructorTableValues` de `DateConstructor.lut.h`, na ordem do `@begin`.
static DATE_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 3] = [
    native_entry_with_intrinsic("parse", date_parse_function, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("UTC", date_utc_function, 7, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("now", date_now_function, 0, Intrinsic::DateNowIntrinsic),
];

/// `dateConstructorTable`.
pub static DATE_CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &DATE_CONSTRUCTOR_TABLE_VALUES };

impl JSGlobalObject {
    /// `dateStructure()`.
    pub fn date_structure(&self) -> StructureRef {
        self.date_structure.borrow().clone().expect("JSGlobalObject sem dateStructure")
    }
}

/// `DateConstructor::create(vm, structure, datePrototype)`: `DateConstructor(vm, structure)`
/// (`callDate` e `constructWithDateConstructor`) e `finishCreation(vm, datePrototype)`.
pub fn create_date_constructor(
    vm: &VM,
    _global_object: &JSGlobalObject,
    structure: StructureRef,
    date_prototype: &JSObject,
) -> InternalFunctionRef {
    let constructor = InternalFunction::new(vm, structure, call_date, Some(construct_with_date_constructor));
    // As entradas da lut (`parse`, `UTC`, `now`) nascem no primeiro acesso (`reify_static_property`).
    constructor.finish_creation(
        vm,
        DATE_CONSTRUCTOR_LENGTH,
        vm.property_names.date.string().string(),
        PropertyAdditionMode::WithoutStructureTransition,
    );
    constructor.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.prototype), date_prototype.as_value(), PROTOTYPE_ATTRIBUTES);
    constructor
}

/// O `Date` do global (`m_dateStructure`, `DontEnum|ClassStructure`): o protótipo, a estrutura das
/// instâncias, o construtor, o `constructor` do protótipo e a propriedade global.
pub fn install_date(vm: &VM, global_object: &JSGlobalObject, object_prototype: JSValue, function_prototype: JSValue) {
    let date_prototype = create_date_prototype(vm, global_object, object_prototype);
    date_prototype.did_become_prototype(vm);
    *global_object.date_structure.borrow_mut() =
        Some(DateInstance::create_structure(vm, Some(global_object), date_prototype.as_value()));

    let constructor_structure = DateConstructor::create_structure(vm, Some(global_object), function_prototype);
    let date_constructor = create_date_constructor(vm, global_object, constructor_structure, &date_prototype);
    date_prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), date_constructor.as_value(), DONT_ENUM);
    install_date_prototype_to_primitive(vm, global_object, &date_prototype);
    global_object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.date), date_constructor.as_value(), DONT_ENUM);
}
