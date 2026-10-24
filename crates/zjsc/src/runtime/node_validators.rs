//! Os validadores de argumento do bun 1.4.2 (`runtime/node/util/validators.rs`): o `ERR_INVALID_ARG_TYPE` e o
//! `ERR_INVALID_ARG_VALUE` de propriedade, com o texto `The "nome" property must be of type tipo, got typeof` (o do
//! bun, que difere do `Received ...` do Node). Quem valida opções de um módulo nativo (`util.parseArgs`, ...) usa
//! estes, sem texto próprio.

use crate::api::eval::typeof_for_property_error;
use crate::runtime::host_call::Thrown;
use crate::runtime::js_array::JSArray;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::node_error::throw_coded_type_error;
use crate::runtime::proxy_object::object_get_own_property_descriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::identifier::Identifier;
use crate::runtime::string_regexp_support::get_object_property;

/// `ERR_INVALID_ARG_TYPE` com a mensagem pronta.
pub(crate) fn invalid_arg_type_message(global_object: &JSGlobalObject, message: &str) -> Thrown {
    throw_coded_type_error(global_object, message, "ERR_INVALID_ARG_TYPE")
}

/// `ERR_INVALID_ARG_VALUE` com a mensagem pronta.
pub(crate) fn invalid_arg_value_message(global_object: &JSGlobalObject, message: &str) -> Thrown {
    throw_coded_type_error(global_object, message, "ERR_INVALID_ARG_VALUE")
}

/// `throwErrInvalidArgType(name, expected, value)`: `The "name" property must be of type expected, got typeof`.
pub(crate) fn invalid_arg_type(global_object: &JSGlobalObject, name: &str, expected: &str, value: JSValue) -> Thrown {
    let message = format!("The \"{name}\" property must be of type {expected}, got {}", typeof_for_property_error(value));
    invalid_arg_type_message(global_object, &message)
}

pub(crate) fn is_array(value: JSValue) -> bool {
    JSArray::from_value(&value).is_some()
}

pub(crate) fn validate_string(global_object: &JSGlobalObject, value: JSValue, name: &str) -> Result<(), Thrown> {
    if value.is_string() {
        Ok(())
    } else {
        Err(invalid_arg_type(global_object, name, "string", value))
    }
}

pub(crate) fn validate_boolean(global_object: &JSGlobalObject, value: JSValue, name: &str) -> Result<bool, Thrown> {
    if value.is_boolean() {
        Ok(value.as_boolean())
    } else {
        Err(invalid_arg_type(global_object, name, "boolean", value))
    }
}

/// `validateObject(value, name)` sem opções: `null`, array e não objeto recusam (a função é objeto).
pub(crate) fn validate_object(global_object: &JSGlobalObject, value: JSValue, name: &str) -> Result<(), Thrown> {
    if value.is_null() || is_array(value) || !value.is_object() {
        return Err(invalid_arg_type(global_object, name, "object", value));
    }
    Ok(())
}

/// `validateArray(value, name, minLength)`.
pub(crate) fn validate_array(global_object: &JSGlobalObject, value: JSValue, name: &str, min_length: Option<i32>) -> Result<(), Thrown> {
    if !is_array(value) {
        let message = format!("The \"{name}\" property must be an instance of Array, got {}", typeof_for_property_error(value));
        return Err(invalid_arg_type_message(global_object, &message));
    }
    if let Some(min_length) = min_length {
        if i64::from(array_length(value)) < i64::from(min_length) {
            return Err(invalid_arg_value_message(global_object, &format!("{name} must be longer than {min_length}")));
        }
    }
    Ok(())
}

pub(crate) fn array_length(array: JSValue) -> u32 {
    JSArray::from_value(&array).map_or(0, |array| array.length())
}

/// O elemento `index` de um array, com o `get` completo (buraco lê o protótipo, acessor roda).
pub(crate) fn get_index(global_object: &JSGlobalObject, array: JSValue, index: u32) -> Result<JSValue, Thrown> {
    let name = Identifier::from_span(global_object.vm(), index.to_string().as_bytes());
    get_object_property(global_object, array, &name)
}

/// Os elementos de `array` (um array já validado) na ordem.
pub(crate) fn array_items(global_object: &JSGlobalObject, array: JSValue) -> Result<Vec<JSValue>, Thrown> {
    (0..array_length(array)).map(|index| get_index(global_object, array, index)).collect()
}

/// `validateStringArray(value, name)`: o texto do erro de elemento usa o tipo do array, como o bun (o `value` do laço
/// é o array, não o elemento), e devolve a contagem.
pub(crate) fn validate_string_array(global_object: &JSGlobalObject, value: JSValue, name: &str) -> Result<usize, Thrown> {
    validate_typed_array(global_object, value, name, "string", JSValue::is_string)
}

/// `validateBooleanArray(value, name)`, com a mesma regra do texto de `validate_string_array`.
pub(crate) fn validate_boolean_array(global_object: &JSGlobalObject, value: JSValue, name: &str) -> Result<usize, Thrown> {
    validate_typed_array(global_object, value, name, "boolean", JSValue::is_boolean)
}

fn validate_typed_array(global_object: &JSGlobalObject, value: JSValue, name: &str, expected: &str, has_type: fn(&JSValue) -> bool) -> Result<usize, Thrown> {
    validate_array(global_object, value, name, None)?;
    let items = array_items(global_object, value)?;
    for (index, item) in items.iter().enumerate() {
        if !has_type(item) {
            return Err(invalid_arg_type(global_object, &format!("{name}[{index}]"), expected, value));
        }
    }
    Ok(items.len())
}

/// `value.getOwn(name)`: o valor da propriedade própria `name` (com o `get` completo), ou `None` quando não há própria.
/// Um primitivo vira objeto (`ToObject`) e não tem a propriedade; `null` e `undefined` lançam o `TypeError` do motor.
pub(crate) fn get_own(global_object: &JSGlobalObject, object: JSValue, name: &str) -> Result<Option<JSValue>, Thrown> {
    let identifier = Identifier::from_span(global_object.vm(), name.as_bytes());
    get_own_by_identifier(global_object, object, &identifier)
}

/// `get_own` com a chave já em `Identifier`.
pub(crate) fn get_own_by_identifier(global_object: &JSGlobalObject, object: JSValue, identifier: &Identifier) -> Result<Option<JSValue>, Thrown> {
    if !object.is_object() {
        if object.is_null() || object.is_undefined() {
            return match object.to_object(global_object) {
                None => Err(Thrown::Pending),
                Some(_) => Ok(None),
            };
        }
        return Ok(None);
    }
    let property = PropertyName::from_identifier(identifier);
    if object_get_own_property_descriptor(global_object, &object.as_object(), &property)?.is_none() {
        return Ok(None);
    }
    get_object_property(global_object, object, identifier).map(Some)
}
