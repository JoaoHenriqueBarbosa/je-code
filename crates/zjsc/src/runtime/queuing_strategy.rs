//! `CountQueuingStrategy` e `ByteLengthQueuingStrategy` do global. O JavaScriptCore não os define: quem os
//! instala é o bun (WebCore), como propriedades de dados `writable`, `configurable` e NÃO enumeráveis (ao
//! contrário de `TextEncoder`). As duas classes têm a mesma forma, por isso um só módulo com um [`Kind`].
//! Medido no bun 1.4.2:
//!
//! - construtor nativo `length` 1, `name` igual ao da classe, chaves próprias `length`, `name`, `prototype`;
//! - protótipo, nesta ordem: `constructor` (não enumerável), os acessores `highWaterMark` e `size` (enumeráveis,
//!   configuráveis, getters nativos `get highWaterMark` e `get size`, sem setter), `Symbol(nodejs.util.inspect.custom)`
//!   e `@@toStringTag` (o nome da classe, não gravável);
//! - a instância não tem propriedade própria; `highWaterMark` devolve o número guardado no construtor;
//!   `size` devolve sempre a mesma função (`name` "size", `length` 0 em `CountQueuingStrategy`, 1 em
//!   `ByteLengthQueuingStrategy`), compartilhada por todas as instâncias da classe;
//! - `new X(init)`: sem argumento, `TypeError: Not enough arguments`; `init` que é `undefined`, `null` ou objeto
//!   sem `highWaterMark` (ou com ele `undefined`), `TypeError: QueuingStrategyInit requires a 'highWaterMark'
//!   member`; qualquer outro não objeto, `TypeError: The QueuingStrategyInit argument must be an object`; o
//!   membro passa por `ToNumber` sem restrição (`'7'` dá 7, `NaN`, `-1` e `Infinity` valem); `Symbol` dá
//!   `Cannot convert a symbol to a number`, `BigInt` dá `Conversion from 'BigInt' to 'number' is not allowed.`;
//! - sem `new`: ``Use `new X(...)` instead of `X(...)` `` com `code` `ERR_ILLEGAL_CONSTRUCTOR`; `this` que não é da
//!   classe em `highWaterMark`/`size` (os getters): `Value of "this" must be of type X`, `ERR_INVALID_THIS`;
//! - `size` de `CountQueuingStrategy` devolve 1 com qualquer argumento e `this`; o de `ByteLengthQueuingStrategy`
//!   devolve `chunk.byteLength` sem conversão.
//!
//! - `size(chunk)` de `ByteLengthQueuingStrategy` com `undefined`/`null`: `TypeError` `null is not an object
//!   (evaluating '<texto da chamada>')`; número, string, booleano e `BigInt` devolvem `undefined`;
//! - `Symbol.for('nodejs.util.inspect.custom')` no protótipo: propriedade de dados `writable`, `configurable`, não
//!   enumerável, função própria de cada classe (`name` "anonymous", `length` 2) que devolve `Name { highWaterMark: N }`
//!   para instância da classe e o próprio `this` para qualquer outro valor.
//!
//! DIVERGÊNCIAS:
//!
//! - a propriedade global entra no fim da ordem de chaves se o `ORDER` ainda não a conhece (ele conhece as duas);
//! - o brand check e o valor guardado ficam em `thread_local` (zerado em `reset_for_program`), como `TextEncoder`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value_conversions::number_to_string_radix10;
use crate::runtime::symbol::Symbol;
use crate::wtf::text::string_impl::StringImpl;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intl_support::{get_property, to_number_checked};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{js_number, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{create_native_class_with_length, install_global_with_attributes, instance_structure, throw_coded_type_error, throw_native_type_error};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::wtf::text::wtf_string::String as WtfString;

/// Qual das duas classes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Count,
    ByteLength,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Count => "CountQueuingStrategy",
            Kind::ByteLength => "ByteLengthQueuingStrategy",
        }
    }

    /// `length` da função `size`.
    fn size_length(self) -> u32 {
        match self {
            Kind::Count => 0,
            Kind::ByteLength => 1,
        }
    }
}

static COUNT_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "CountQueuingStrategy", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static BYTE_LENGTH_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "ByteLengthQueuingStrategy", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo` do construtor (`"Function"`), igual nas duas classes.
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

thread_local! {
    /// As instâncias do programa (o valor codificado da célula) com a classe e o `highWaterMark`.
    static INSTANCES: RefCell<HashMap<EncodedJSValue, (Kind, f64)>> = RefCell::new(HashMap::new());
    /// A função `size` de cada classe, criada no primeiro acesso.
    static SIZE_FUNCTIONS: RefCell<HashMap<Kind, EncodedJSValue>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): o que foi guardado é do programa.
/// As raízes do coletor: as instâncias (a chave do brand check não pode ser reaproveitada) e as funções `size`.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    INSTANCES.with(|instances| instances.borrow().keys().for_each(|instance| marker.encoded(*instance)));
    SIZE_FUNCTIONS.with(|functions| functions.borrow().values().for_each(|function| marker.encoded(*function)));
}

pub(crate) fn reset_for_program() {
    let _ = INSTANCES.try_with(|instances| instances.borrow_mut().clear());
    let _ = SIZE_FUNCTIONS.try_with(|functions| functions.borrow_mut().clear());
}

/// O `highWaterMark` de `this`, se for instância de `kind`; senão o `TypeError` de `this` inválido.
fn high_water_mark_of(global_object: &JSGlobalObject, call: &HostCall, kind: Kind) -> Result<f64, Thrown> {
    let found = INSTANCES.with(|instances| instances.borrow().get(&call.this_value().encode()).copied());
    match found {
        Some((found_kind, high_water_mark)) if found_kind == kind => Ok(high_water_mark),
        _ => Err(throw_coded_type_error(global_object, &format!("Value of \"this\" must be of type {}", kind.name()), "ERR_INVALID_THIS")),
    }
}

fn call_body(global_object: &JSGlobalObject, kind: Kind) -> HostResult {
    let name = kind.name();
    Err(throw_coded_type_error(global_object, &format!("Use `new {name}(...)` instead of `{name}(...)`"), "ERR_ILLEGAL_CONSTRUCTOR"))
}

/// `new X(init)`: o dicionário `QueuingStrategyInit` com o membro obrigatório `highWaterMark`.
fn construct_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind) -> HostResult {
    if call.argument_count() < 1 {
        return Err(throw_native_type_error(global_object, "Not enough arguments"));
    }
    let init = call.argument(0);
    if !init.is_undefined_or_null() && !init.is_object() {
        return Err(throw_native_type_error(global_object, "The QueuingStrategyInit argument must be an object"));
    }
    let member = if init.is_object() { get_property(global_object, init, "highWaterMark")? } else { JSValue::undefined() };
    if member.is_undefined() {
        return Err(throw_native_type_error(global_object, "QueuingStrategyInit requires a 'highWaterMark' member"));
    }
    if member.is_big_int() {
        return Err(throw_native_type_error(global_object, "Conversion from 'BigInt' to 'number' is not allowed."));
    }
    let high_water_mark = to_number_checked(global_object, member)?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode(), (kind, high_water_mark)));
    Ok(instance)
}

fn high_water_mark_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind) -> HostResult {
    Ok(js_number(high_water_mark_of(global_object, call, kind)?))
}

/// O getter `size`: sempre a mesma função da classe.
fn size_getter_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind, size_function: crate::runtime::native_function::NativeFunction) -> HostResult {
    high_water_mark_of(global_object, call, kind)?;
    if let Some(existing) = SIZE_FUNCTIONS.with(|functions| functions.borrow().get(&kind).copied()) {
        return Ok(JSValue::decode(existing));
    }
    let vm = global_object.vm();
    let function = JSFunction::create_native(
        vm,
        global_object,
        kind.size_length(),
        &WtfString::from_latin1(b"size"),
        size_function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    SIZE_FUNCTIONS.with(|functions| functions.borrow_mut().insert(kind, function.as_value().encode()));
    Ok(function.as_value())
}

fn count_size_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(1))
}

/// `chunk.byteLength` do `size` em JS do bun: `undefined` e `null` dão o `TypeError` de `createNotAnObjectError`
/// (o texto da chamada de quem invoca entra em `(evaluating '...')`), o resto passa pelo wrapper do primitivo.
fn byte_length_size_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    get_property(global_object, call.argument(0), "byteLength")
}

/// `[Symbol.for('nodejs.util.inspect.custom')]`: `Name { highWaterMark: N }` para instância da classe e o próprio
/// `this` para qualquer outro valor (medido no bun 1.4.2).
fn inspect_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind) -> HostResult {
    let this_value = call.this_value();
    let found = INSTANCES.with(|instances| instances.borrow().get(&this_value.encode()).copied());
    let Some((found_kind, high_water_mark)) = found else { return Ok(this_value) };
    if found_kind != kind {
        return Ok(this_value);
    }
    let vm = global_object.vm();
    if crate::runtime::streams::options_depth_exhausted(global_object, call.argument(1))? {
        let text = format!("{} [Object]", kind.name());
        return Ok(JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(text.as_bytes()))));
    }
    let number = if high_water_mark == 0.0 && high_water_mark.is_sign_negative() {
        "-0".to_string()
    } else {
        crate::runtime::js_module_loader::rust_string(&number_to_string_radix10(vm, high_water_mark).value())
    };
    let text = format!("{} {{ highWaterMark: {number} }}", kind.name());
    Ok(JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(text.as_bytes()))))
}

/// Define as seis funções nativas de uma classe (`call`, `construct`, os dois getters, `size` e o `inspect`).
macro_rules! strategy_class {
    ($module:ident, $kind:expr, $size_body:path) => {
        mod $module {
            use super::*;
            fn call_b(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
                call_body(global_object, $kind)
            }
            fn construct_b(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                construct_body(global_object, call, $kind)
            }
            fn hwm_b(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                high_water_mark_body(global_object, call, $kind)
            }
            fn size_getter_b(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                size_getter_body(global_object, call, $kind, size)
            }
            fn inspect_b(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                inspect_body(global_object, call, $kind)
            }
            host_function!(pub inspect, inspect_b);
            host_function!(pub call, call_b);
            host_function!(pub construct, construct_b);
            host_function!(pub high_water_mark, hwm_b);
            host_function!(pub size_getter, size_getter_b);
            host_function!(pub size, $size_body);
        }
    };
}

strategy_class!(count, Kind::Count, count_size_body);
strategy_class!(byte_length, Kind::ByteLength, byte_length_size_body);

/// Instala `CountQueuingStrategy` e `ByteLengthQueuingStrategy` no global.
pub fn install_queuing_strategies(global_object: &JSGlobalObject) {
    use crate::runtime::native_function::NativeFunction;
    let vm = global_object.vm();
    let classes: [(Kind, &'static ClassInfo, NativeFunction, NativeFunction, NativeFunction, NativeFunction, NativeFunction); 2] = [
        (Kind::Count, &COUNT_PROTOTYPE_S_INFO, count::call, count::construct, count::high_water_mark, count::size_getter, count::inspect),
        (Kind::ByteLength, &BYTE_LENGTH_PROTOTYPE_S_INFO, byte_length::call, byte_length::construct, byte_length::high_water_mark, byte_length::size_getter, byte_length::inspect),
    ];
    for (kind, prototype_info, call, construct, high_water_mark, size_getter, inspect) in classes {
        let (prototype, constructor) = create_native_class_with_length(global_object, prototype_info, &CONSTRUCTOR_S_INFO, kind.name(), 1, call, construct);
        prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
        put_native_getter(vm, global_object, &prototype, "highWaterMark", high_water_mark, Intrinsic::NoIntrinsic, 0);
        put_native_getter(vm, global_object, &prototype, "size", size_getter, Intrinsic::NoIntrinsic, 0);
        let inspect_function = JSFunction::create_native(
            vm,
            global_object,
            2,
            &WtfString::from_latin1(b"anonymous"),
            inspect,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            call_host_function_as_constructor,
        );
        let registered = vm.symbol_registry().symbol_for_key(&StringImpl::create(b"nodejs.util.inspect.custom"));
        let inspect_symbol = Symbol::create_with_registered_uid(vm, &registered);
        prototype.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_private_name(&inspect_symbol.private_name())), inspect_function.as_value(), DONT_ENUM);
        put_to_string_tag(vm, &prototype, kind.name());
        install_global_with_attributes(global_object, kind.name(), constructor.as_value(), DONT_ENUM);
    }
}
