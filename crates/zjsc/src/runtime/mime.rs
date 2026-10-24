//! `util.MIMEType` e `util.MIMEParams` (`JSMIMEType.cpp` e `JSMIMEParams.cpp` do bun 1.4.2, medidos contra o oráculo em
//! `tests/golden/node_util_mime_main.js`).
//!
//! As duas classes são objetos comuns (sem propriedade própria) com estado nativo numa tabela lateral por célula, como
//! `Request`: o `MIMEParams` guarda um `JSMap` de verdade (nome em minúsculas para o valor) e o `MIMEType` guarda tipo,
//! subtipo e o `MIMEParams`. As classes são criadas por global e entregues ao `util` pelo objeto `host`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::exception_helpers::create_not_a_constructor_error;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_custom_accessor_function::create_host_custom_accessor_getter_function;
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_map::{JSMap, JSMapIterator, JSMapRef};
use crate::runtime::js_module_loader::describe_received;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_undefined, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{create_native_class_with_length, instance_structure, property_key, put_native_accessor, throw_coded_type_error};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{ACCESSOR, DONT_ENUM};
use crate::runtime::property_name::PropertyName;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::web_iterable::{put_enumerable_methods, string_value, units_of, Units};
use crate::wtf::text::wtf_string::String as WtfString;

static TYPE_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "MIMEType", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static PARAMS_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "MIMEParams", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// O estado de um `MIMEType`.
#[derive(Clone)]
struct TypeState {
    type_: Units,
    subtype: Units,
    params: JSValue,
}

thread_local! {
    /// Os `MIMEParams` do programa (o valor codificado da célula) com o `JSMap` deles.
    static PARAMS: RefCell<HashMap<EncodedJSValue, JSValue>> = RefCell::new(HashMap::new());
    /// Os `MIMEType` do programa com o estado.
    static TYPES: RefCell<HashMap<EncodedJSValue, TypeState>> = RefCell::new(HashMap::new());
    /// Os protótipos, guardados na instalação para o `MIMEType` criar o `MIMEParams` dele.
    static PARAMS_PROTOTYPE: RefCell<Option<JSValue>> = const { RefCell::new(None) };
    static TYPE_PROTOTYPE: RefCell<Option<JSValue>> = const { RefCell::new(None) };
}

/// As raízes do coletor: os protótipos.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(PARAMS_PROTOTYPE.with(|prototype| *prototype.borrow()));
    marker.value(TYPE_PROTOTYPE.with(|prototype| *prototype.borrow()));
}

/// As tabelas laterais: o mapa do `MIMEParams` e os parâmetros do `MIMEType` vivem com a célula dona.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    let params = crate::runtime::gc::side_table(&PARAMS, marker, phase, |map, out| out.push(*map));
    let types = crate::runtime::gc::side_table(&TYPES, marker, phase, |state, out| out.push(state.params));
    params | types
}

/// Fim do programa: o que foi guardado é do programa.
pub(crate) fn reset_for_program() {
    let _ = PARAMS.try_with(|table| table.borrow_mut().clear());
    let _ = TYPES.try_with(|table| table.borrow_mut().clear());
}

// -- Auxiliares do HTTP (mime.ts e HTTPParsers.h) --

/// `/[!#$%&'*+\-.^_`|~A-Za-z0-9]/`: um code point de token HTTP.
fn is_token_unit(unit: u16) -> bool {
    unit < 0x80 && (char::from(unit as u8).is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&(unit as u8)))
}

/// O primeiro que não é token HTTP.
fn first_invalid_token(units: &[u16]) -> Option<usize> {
    units.iter().position(|&unit| !is_token_unit(unit))
}

/// `\t`, ` ` e `!-~\u0080-ÿ`: o que vale dentro de um valor de parâmetro.
fn is_quoted_string_unit(unit: u16) -> bool {
    unit == 0x09 || (0x20..=0x7E).contains(&unit) || (0x80..=0xFF).contains(&unit)
}

fn first_invalid_quoted_string(units: &[u16]) -> Option<usize> {
    units.iter().position(|&unit| !is_quoted_string_unit(unit))
}

fn is_whitespace_unit(unit: u16) -> bool {
    matches!(unit, 0x09 | 0x20 | 0x0D | 0x0A)
}

/// `/[^\r\n\t ]|$/`: o índice do primeiro que não é espaço.
fn end_of_leading_whitespace(units: &[u16]) -> usize {
    units.iter().position(|&unit| !is_whitespace_unit(unit)).unwrap_or(units.len())
}

/// `/[\r\n\t ]*$/`: o tamanho sem os espaços do fim.
fn start_of_trailing_whitespace(units: &[u16]) -> usize {
    units.iter().rposition(|&unit| !is_whitespace_unit(unit)).map_or(0, |index| index + 1)
}

fn remove_backslashes(units: &[u16]) -> Units {
    let mut out = Units::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        if units[index] == u16::from(b'\\') && index + 1 < units.len() {
            index += 1;
        }
        out.push(units[index]);
        index += 1;
    }
    out
}

/// `convertToASCIILowercase`.
fn ascii_lowercase(units: &[u16]) -> Units {
    units.iter().map(|&unit| if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) { unit + 32 } else { unit }).collect()
}

fn encode_param_value(units: &[u16], out: &mut Units) {
    if units.is_empty() {
        out.extend_from_slice(&[u16::from(b'"'), u16::from(b'"')]);
        return;
    }
    if first_invalid_token(units).is_none() {
        out.extend_from_slice(units);
        return;
    }
    out.push(u16::from(b'"'));
    for &unit in units {
        if unit == u16::from(b'"') || unit == u16::from(b'\\') {
            out.push(u16::from(b'\\'));
        }
        out.push(unit);
    }
    out.push(u16::from(b'"'));
}

fn text_of(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// `Bun::ERR::INVALID_MIME_SYNTAX(scope, globalObject, part, input, position)`: a posição só aparece se existir.
fn invalid_mime_syntax(global_object: &JSGlobalObject, part: &str, input: &[u16], position: Option<usize>) -> Thrown {
    let at = position.map(|position| format!(" at {position}")).unwrap_or_default();
    throw_coded_type_error(global_object, &format!("The MIME syntax for a {part} in \"{}\" is invalid{at}", text_of(input)), "ERR_INVALID_MIME_SYNTAX")
}

/// `parseTypeAndSubtype`: (tipo, subtipo, onde começam os parâmetros).
fn parse_type_and_subtype(global_object: &JSGlobalObject, input: &[u16]) -> Result<(Units, Units, usize), Thrown> {
    let mut position = end_of_leading_whitespace(input);
    let length = input.len();
    let slash = input[position..].iter().position(|&unit| unit == u16::from(b'/')).map(|index| index + position);
    let Some(type_end) = slash else {
        return Err(invalid_mime_syntax(global_object, "type", input, first_invalid_token(&input[position..])));
    };
    let type_view = &input[position..type_end];
    let invalid_type = first_invalid_token(type_view);
    if type_view.is_empty() || invalid_type.is_some() {
        return Err(invalid_mime_syntax(global_object, "type", input, invalid_type));
    }
    let type_ = ascii_lowercase(type_view);
    position = type_end + 1;

    let semicolon = input[position..].iter().position(|&unit| unit == u16::from(b';')).map(|index| index + position);
    let (raw_subtype, params_start) = match semicolon {
        None => (&input[position..], length),
        Some(end) => (&input[position..end], end + 1),
    };
    let subtype_view = &raw_subtype[..start_of_trailing_whitespace(raw_subtype)];
    let invalid_subtype = first_invalid_token(subtype_view);
    if subtype_view.is_empty() || invalid_subtype.is_some() {
        return Err(invalid_mime_syntax(global_object, "subtype", input, invalid_subtype));
    }
    Ok((type_, ascii_lowercase(subtype_view), params_start))
}

/// `parseMIMEParamsString`: preenche o mapa; o primeiro nome vence.
fn parse_params(global_object: &JSGlobalObject, map: &JSMapRef, input: &[u16]) {
    let vm = global_object.vm();
    let length = input.len();
    let find_semicolon = |from: usize| input[from..].iter().position(|&unit| unit == u16::from(b';')).map(|index| index + from);
    let mut position = 0;
    while position < length {
        position += end_of_leading_whitespace(&input[position..]);
        if position >= length {
            break;
        }
        let mut name_end = position;
        while name_end < length && input[name_end] != u16::from(b';') && input[name_end] != u16::from(b'=') {
            name_end += 1;
        }
        let name = ascii_lowercase(&input[position..name_end]);
        position = name_end;

        let value: Units;
        if position < length && input[position] == u16::from(b'=') {
            position += 1;
            if position < length && input[position] == u16::from(b'"') {
                position += 1;
                let value_start = position;
                let mut escaped = false;
                while position < length {
                    let unit = input[position];
                    if escaped {
                        escaped = false;
                    } else if unit == u16::from(b'\\') {
                        escaped = true;
                    } else if unit == u16::from(b'"') {
                        break;
                    }
                    position += 1;
                }
                value = remove_backslashes(&input[value_start..position]);
                if position < length && input[position] == u16::from(b'"') {
                    position += 1;
                } else {
                    position = find_semicolon(position).unwrap_or(length);
                }
            } else {
                let mut value_end = position;
                while value_end < length && input[value_end] != u16::from(b';') {
                    value_end += 1;
                }
                let raw = &input[position..value_end];
                value = raw[..start_of_trailing_whitespace(raw)].to_vec();
                position = value_end;
                if value.is_empty() {
                    continue;
                }
            }
        } else {
            position = find_semicolon(position).unwrap_or(length);
            if position < length && input[position] == u16::from(b';') {
                position += 1;
            }
            continue;
        }

        if name.is_empty() || first_invalid_token(&name).is_some() {
            continue;
        }
        if first_invalid_quoted_string(&value).is_some() {
            continue;
        }
        let key = string_value(global_object, &name);
        let mut table = map.table().borrow_mut();
        if !table.has(key) {
            table.add(key, JSValue::from_js_string(js_string(vm, &WtfString::from_utf16(&value))));
        }
        drop(table);
        if position < length && input[position] == u16::from(b';') {
            position += 1;
        }
    }
}

// -- MIMEParams --

/// Um `MIMEParams` novo sobre o `map`.
fn create_params(global_object: &JSGlobalObject, structure: &crate::runtime::structure::StructureRef, map: &JSMapRef) -> JSValue {
    let instance = JSFinalObject::create(global_object.vm(), structure).as_value();
    PARAMS.with(|table| table.borrow_mut().insert(instance.encode(), map.as_value()));
    instance
}

fn new_map(global_object: &JSGlobalObject) -> JSMapRef {
    JSMap::create(global_object.vm(), &global_object.map_structure())
}

/// `this` como `MIMEParams`: o `JSMap` dele; senão `ERR_INVALID_THIS`.
fn params_map(global_object: &JSGlobalObject, this_value: JSValue) -> Result<JSMapRef, Thrown> {
    PARAMS
        .with(|table| table.borrow().get(&this_value.encode()).copied())
        .and_then(|map| JSMap::from_value(&map))
        .ok_or_else(|| invalid_this(global_object, "MIMEParams", this_value))
}

fn invalid_this(global_object: &JSGlobalObject, class: &str, this_value: JSValue) -> Thrown {
    let received = describe_received(global_object, this_value).map(|text| format!(", but received {text}")).unwrap_or_default();
    throw_coded_type_error(global_object, &format!("Expected this to be instanceof {class}{received}"), "ERR_INVALID_THIS")
}

fn not_a_constructor(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let error = create_not_a_constructor_error(global_object, JSValue::from_cell(call.callee()));
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, error);
    Err(Thrown::Pending)
}

fn params_construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let structure = derived_structure(global_object, call, instance_structure)?;
    Ok(create_params(global_object, &structure, &new_map(global_object)))
}

fn call_params_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    not_a_constructor(global_object, call)
}

/// O nome (primeiro argumento) como texto: `toWTFString` de `argument(0)`.
fn name_argument(global_object: &JSGlobalObject, call: &HostCall) -> Result<JSValue, Thrown> {
    Ok(string_value(global_object, &units_of(global_object, call.argument(0))?))
}

fn params_get_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let map = params_map(global_object, call.this_value())?;
    let name = name_argument(global_object, call)?;
    let table = map.table().borrow();
    Ok(table.get_entry(name).unwrap_or(JSValue::null()))
}

fn params_has_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let map = params_map(global_object, call.this_value())?;
    let name = name_argument(global_object, call)?;
    let has = map.table().borrow().has(name);
    Ok(JSValue::Bool(has))
}

fn params_set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let map = params_map(global_object, call.this_value())?;
    let name = units_of(global_object, call.argument(0))?;
    let value = units_of(global_object, call.argument(1))?;
    let invalid_name = first_invalid_token(&name);
    if name.is_empty() || invalid_name.is_some() {
        return Err(invalid_mime_syntax(global_object, "parameter name", &name, invalid_name));
    }
    if let Some(index) = first_invalid_quoted_string(&value) {
        return Err(invalid_mime_syntax(global_object, "parameter value", &value, Some(index)));
    }
    map.table().borrow_mut().add(string_value(global_object, &name), string_value(global_object, &value));
    Ok(JSValue::undefined())
}

fn params_delete_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let map = params_map(global_object, call.this_value())?;
    let name = name_argument(global_object, call)?;
    map.table().borrow_mut().remove(name);
    Ok(JSValue::undefined())
}

/// `serialize`: `nome=valor` separados por `;`, o valor entre aspas quando não é só token.
fn serialize_params(global_object: &JSGlobalObject, map: &JSMapRef) -> Result<Units, Thrown> {
    let entries: Vec<(JSValue, JSValue)> = {
        let cursor = map.table().borrow_mut().new_cursor();
        std::iter::from_fn(|| map.table().borrow().next_entry(&cursor)).collect()
    };
    let mut out = Units::new();
    for (index, (key, value)) in entries.into_iter().enumerate() {
        if index > 0 {
            out.push(u16::from(b';'));
        }
        out.extend(units_of(global_object, key)?);
        out.push(u16::from(b'='));
        encode_param_value(&units_of(global_object, value)?, &mut out);
    }
    Ok(out)
}

fn params_to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let map = params_map(global_object, call.this_value())?;
    Ok(string_value(global_object, &serialize_params(global_object, &map)?))
}

fn params_iterator(global_object: &JSGlobalObject, call: &HostCall, kind: IterationKind) -> HostResult {
    let map = params_map(global_object, call.this_value())?;
    Ok(JSMapIterator::create(global_object.vm(), &global_object.map_iterator_structure(), &map, kind).as_value())
}

fn params_entries_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    params_iterator(global_object, call, IterationKind::Entries)
}

fn params_keys_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    params_iterator(global_object, call, IterationKind::Keys)
}

fn params_values_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    params_iterator(global_object, call, IterationKind::Values)
}

// -- MIMEType --

fn type_state(global_object: &JSGlobalObject, this_value: JSValue) -> Result<TypeState, Thrown> {
    TYPES.with(|table| table.borrow().get(&this_value.encode()).cloned()).ok_or_else(|| invalid_this(global_object, "MIMEType", this_value))
}

fn type_construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let structure = derived_structure(global_object, call, instance_structure)?;
    let input = units_of(global_object, call.argument(0))?;
    let (type_, subtype, params_start) = parse_type_and_subtype(global_object, &input)?;

    let map = new_map(global_object);
    let _map = crate::runtime::gc::RootScope::new(&[map.as_value()]);
    parse_params(global_object, &map, &input[params_start..]);
    let params_prototype = PARAMS_PROTOTYPE.with(|slot| slot.borrow().expect("MIMEParams sem protótipo (instalação)"));
    let params_structure = instance_structure(vm, Some(global_object), params_prototype);
    let params = create_params(global_object, &params_structure, &map);
    let _params = crate::runtime::gc::RootScope::new(&[params]);

    let instance = JSFinalObject::create(vm, &structure).as_value();
    TYPES.with(|table| table.borrow_mut().insert(instance.encode(), TypeState { type_, subtype, params }));
    Ok(instance)
}

fn call_type_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    not_a_constructor(global_object, call)
}

fn type_getter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(string_value(global_object, &type_state(global_object, call.this_value())?.type_))
}

fn subtype_getter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(string_value(global_object, &type_state(global_object, call.this_value())?.subtype))
}

fn essence_getter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = type_state(global_object, call.this_value())?;
    let mut essence = state.type_;
    essence.push(u16::from(b'/'));
    essence.extend(state.subtype);
    Ok(string_value(global_object, &essence))
}

fn params_getter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(type_state(global_object, call.this_value())?.params)
}

/// O setter de `type` e de `subtype`: texto, token não vazio, minúsculas.
fn set_token(global_object: &JSGlobalObject, call: &HostCall, part: &str, assign: impl FnOnce(&mut TypeState, Units)) -> HostResult {
    type_state(global_object, call.this_value())?;
    let text = units_of(global_object, call.argument(0))?;
    let invalid = first_invalid_token(&text);
    if text.is_empty() || invalid.is_some() {
        return Err(invalid_mime_syntax(global_object, part, &text, invalid));
    }
    let lowered = ascii_lowercase(&text);
    TYPES.with(|table| {
        if let Some(state) = table.borrow_mut().get_mut(&call.this_value().encode()) {
            assign(state, lowered);
        }
    });
    Ok(JSValue::undefined())
}

fn type_setter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_token(global_object, call, "type", |state, value| state.type_ = value)
}

fn subtype_setter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_token(global_object, call, "subtype", |state, value| state.subtype = value)
}

fn type_to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = type_state(global_object, call.this_value())?;
    let params = match JSMap::from_value(&PARAMS.with(|table| table.borrow().get(&state.params.encode()).copied().unwrap_or(JSValue::undefined()))) {
        Some(map) => serialize_params(global_object, &map)?,
        None => Units::new(),
    };
    let mut out = state.type_;
    out.push(u16::from(b'/'));
    out.extend(state.subtype);
    if !params.is_empty() {
        out.push(u16::from(b';'));
        out.extend(params);
    }
    Ok(string_value(global_object, &out))
}

host_function!(call_params_fn, call_params_body);
host_function!(construct_params_fn, params_construct_body);
host_function!(params_get_fn, params_get_body);
host_function!(params_has_fn, params_has_body);
host_function!(params_set_fn, params_set_body);
host_function!(params_delete_fn, params_delete_body);
host_function!(params_to_string_fn, params_to_string_body);
host_function!(params_entries_fn, params_entries_body);
host_function!(params_keys_fn, params_keys_body);
host_function!(params_values_fn, params_values_body);
host_function!(call_type_fn, call_type_body);
host_function!(construct_type_fn, type_construct_body);
host_function!(type_getter_fn, type_getter_body);
host_function!(type_setter_fn, type_setter_body);
host_function!(subtype_getter_fn, subtype_getter_body);
host_function!(subtype_setter_fn, subtype_setter_body);
host_function!(essence_getter_fn, essence_getter_body);
host_function!(params_getter_fn, params_getter_body);
host_function!(type_to_string_fn, type_to_string_body);

/// Um atributo só de leitura escrito à mão no bun (`CustomAccessor` sem setter e sem `ReadOnly`): o acessor exposto tem
/// `set` indefinido, e a escrita é ignorada em vez de lançar.
fn put_attribute_ignoring_put(global_object: &JSGlobalObject, prototype: &crate::runtime::js_object::JSObject, name: &str, getter: NativeFunction) {
    let vm = global_object.vm();
    let function = create_host_custom_accessor_getter_function(vm, global_object, name, getter);
    if let Some(custom) = function.custom_accessor.get() {
        custom.set_ignores_put();
    }
    let accessor = GetterSetter::create_from_values(vm, function.as_value(), js_undefined());
    prototype.put_direct_non_index_accessor_without_transition(vm, &property_key(vm, name), &accessor, ACCESSOR);
}

/// Instala as duas classes e devolve `(MIMEType, MIMEParams)`.
pub(crate) fn install_mime_classes(global_object: &JSGlobalObject) -> (JSValue, JSValue) {
    let vm = global_object.vm();
    let to_json = PropertyName::from_identifier(&Identifier::from_span(vm, b"toJSON"));
    let to_string = PropertyName::from_identifier(&vm.property_names.to_string);
    let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);

    let (params_prototype, params_constructor) =
        create_native_class_with_length(global_object, &PARAMS_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "MIMEParams", 0, call_params_fn, construct_params_fn);
    let params_methods: [(&str, u32, NativeFunction); 8] = [
        ("get", 1, params_get_fn),
        ("has", 1, params_has_fn),
        ("set", 2, params_set_fn),
        ("delete", 1, params_delete_fn),
        ("toString", 0, params_to_string_fn),
        ("entries", 0, params_entries_fn),
        ("keys", 0, params_keys_fn),
        ("values", 0, params_values_fn),
    ];
    put_enumerable_methods(global_object, &params_prototype, &params_methods);
    let params_to_string = params_prototype.get_direct_by_name(vm, &to_string);
    // O `@@iterator` entra antes do `toJSON` (a ordem de inserção que o `console.log` mostra).
    let entries = params_prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"entries")));
    params_prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.iterator_symbol), entries, DONT_ENUM);
    params_prototype.put_direct(vm, &to_json, params_to_string, 0);
    params_prototype.put_direct(vm, &constructor_name, params_constructor.as_value(), DONT_ENUM);
    put_to_string_tag(vm, &params_prototype, "MIMEParams");
    PARAMS_PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(params_prototype.as_value()));

    let (type_prototype, type_constructor) =
        create_native_class_with_length(global_object, &TYPE_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "MIMEType", 1, call_type_fn, construct_type_fn);
    put_native_accessor(vm, global_object, &type_prototype, "type", type_getter_fn, Some(type_setter_fn), 0);
    put_native_accessor(vm, global_object, &type_prototype, "subtype", subtype_getter_fn, Some(subtype_setter_fn), 0);
    put_attribute_ignoring_put(global_object, &type_prototype, "essence", essence_getter_fn);
    put_attribute_ignoring_put(global_object, &type_prototype, "params", params_getter_fn);
    put_enumerable_methods(global_object, &type_prototype, &[("toString", 0, type_to_string_fn)]);
    let type_to_string = type_prototype.get_direct_by_name(vm, &to_string);
    type_prototype.put_direct(vm, &to_json, type_to_string, 0);
    type_prototype.put_direct(vm, &constructor_name, type_constructor.as_value(), DONT_ENUM);
    put_to_string_tag(vm, &type_prototype, "MIMEType");
    TYPE_PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(type_prototype.as_value()));

    (type_constructor.as_value(), params_constructor.as_value())
}
