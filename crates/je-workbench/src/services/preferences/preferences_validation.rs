//! Porte de `src/vs/workbench/services/preferences/common/preferencesValidation.ts`: a validação do valor de um
//! setting contra o esquema da sua propriedade (`IConfigurationPropertySchema`).
//!
//! Divergências:
//! - O esquema é o objeto dinâmico ([`Dynamic`]) que o registro de configuração guarda, e o valor validado também
//!   (`any`). Uma propriedade do registro entra por [`PropertySchema::json`](je_platform::configuration::registry::PropertySchema::json).
//!   Os subesquemas (`items`, `properties`, `patternProperties`, `additionalProperties`) são objetos dinâmicos
//!   soltos, como no original, por isso nada aqui exige uma [`PropertySchema`](je_platform::configuration::registry::PropertySchema).
//! - O validador devolve `String` (a mensagem, ou vazio quando o valor vale). O `string | null` do original nunca
//!   devolve `null` na função de fora, só os validadores internos de array e objeto, que aqui são `Option<String>`.
//! - `nls.localize` é [`localize`]; as mensagens saem em inglês, como no VS Code sem pacote de idioma.
//! - Onde o JavaScript compara tipos mistos com `<=` e `>=` (limites do esquema), a comparação é numérica
//!   ([`to_number`]); a comparação lexicográfica de duas strings não é reproduzida (o esquema JSON só traz números).
//! - `enum` que não é array lança `TypeError` ao chamar `map`/`includes`/`indexOf`, como o JavaScript faz para
//!   objetos e números; a string, que tem `includes` e `indexOf` próprios no JavaScript, também lança aqui.
//! - O `console.error` do `toRegExp` é `eprintln!`.

use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy, json_stringify};
use je_base::errors::{self, Error};
use je_base::js_regexp::JsRegExp;
use je_base::nls::localize;
use je_base::types::{is_object, is_string_array};
use je_platform::js::{for_in_entries, to_js_string, to_number};
use je_theme::color::format::css::parse_hex;

/// `Validator<T>`: uma regra, se vale para este esquema, o teste e a mensagem de quando falha.
struct Validator<T> {
    enabled: bool,
    is_valid: Box<dyn Fn(&T) -> bool>,
    message: String,
}

fn validator<T>(enabled: bool, message: String, is_valid: impl Fn(&T) -> bool + 'static) -> Validator<T> {
    Validator { enabled, is_valid: Box::new(is_valid), message }
}

/// O `.filter(validation => validation.enabled)` do fim de cada lista de validadores.
fn only_enabled<T>(validators: Vec<Validator<T>>) -> Vec<Validator<T>> {
    validators.into_iter().filter(|v| v.enabled).collect()
}

/// `.filter(validator => !validator.isValid(value)).map(validator => validator.message)`.
fn failed_messages<T>(validators: &[Validator<T>], value: &T) -> Vec<String> {
    validators.iter().filter(|v| !(v.is_valid)(value)).map(|v| v.message.clone()).collect()
}

/// O validador de um valor: a mensagem de erro, vazia quando o valor vale.
pub type ValidatorFn = Box<dyn Fn(&Dynamic) -> String>;

/// Os validadores de array e de objeto: `None` é o `null` do original (nada a dizer).
type PartialValidator = Box<dyn Fn(&Dynamic) -> Option<String>>;

/// `String(value)`.
fn text_of(value: &Dynamic) -> String {
    to_js_string(value).to_string_lossy()
}

/// `nls.localize(key, message, ...args)` com os argumentos em valor dinâmico (formatados como `String(arg)`).
fn localized(key: &str, message: &str, args: &[&Dynamic]) -> String {
    let texts: Vec<String> = args.iter().map(|arg| text_of(arg)).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    localize(key, message, &refs)
}

/// `typeof value === 'string' && value === expected`.
fn is_text(value: &Dynamic, expected: &str) -> bool {
    matches!(value, Dynamic::String(text) if text.to_string_lossy() == expected)
}

/// `prop.type === name` (a comparação é com o `type` inteiro, então uma lista de tipos nunca vale).
fn type_is(prop: &Dynamic, name: &str) -> bool {
    is_text(&prop.get_str("type"), name)
}

/// `Array.isArray(prop.type) ? prop.type : [prop.type]`: o `None` é o `undefined` (ou um elemento que não é texto,
/// que nunca é igual a nome de tipo).
fn schema_types(prop: &Dynamic) -> Vec<Option<String>> {
    let type_ = prop.get_str("type");
    let items = match &type_ {
        Dynamic::Array(items) => items.to_vec(),
        other => vec![other.clone()],
    };
    items
        .into_iter()
        .map(|item| match item {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        })
        .collect()
}

/// `canBeType(propTypes, ...types)`.
fn can_be_type(prop_types: &[Option<String>], types: &[&str]) -> bool {
    types.iter().any(|t| prop_types.iter().any(|p| p.as_deref() == Some(*t)))
}

/// `canBeType(type, ...kinds) && (type.length === 1 || type.length === 2 && isNullable)`: o tipo é só os
/// `kinds` (e opcionalmente `null`).
fn is_only_type(prop_types: &[Option<String>], kinds: &[&str]) -> bool {
    let is_nullable = can_be_type(prop_types, &["null"]);
    can_be_type(prop_types, kinds) && (prop_types.len() == 1 || prop_types.len() == 2 && is_nullable)
}

fn is_null_or_empty(value: &Dynamic) -> bool {
    is_text(value, "") || value.is_nullish()
}

/// `value.map(...)`, `.includes(...)` e `.indexOf(...)` do `enum`: os elementos do array. Qualquer outro valor lança o
/// `TypeError` do JavaScript (`null` e `undefined` lançam o de leitura de propriedade).
fn array_items(value: &Dynamic, method: &str) -> Vec<Dynamic> {
    match value {
        Dynamic::Array(items) => items.to_vec(),
        other => {
            // em `null` e `undefined` esta leitura já lança o `TypeError` de propriedade
            other.get_str(method);
            errors::throw(Error::named("TypeError", format!("{method} is not a function")))
        }
    }
}

/// `RegExp.prototype.test` sobre um texto UTF-16.
fn test(reg_exp: &JsRegExp, text: &[u16]) -> bool {
    reg_exp.exec(text).is_some()
}

pub fn create_validator(prop: &Dynamic) -> ValidatorFn {
    let type_ = schema_types(prop);
    let is_nullable = can_be_type(&type_, &["null"]);
    let is_numeric = is_only_type(&type_, &["number", "integer"]);

    let numeric_validations = get_numeric_validators(prop);
    let string_validations = get_string_validators(prop);
    let array_validator = get_array_validator(prop);
    let object_validator = get_object_validator(prop);
    let prop = prop.clone();

    Box::new(move |value| {
        if is_nullable && is_null_or_empty(value) {
            return String::new();
        }

        let mut error_messages: Vec<String> = Vec::new();
        for partial in [&array_validator, &object_validator].into_iter().flatten() {
            if let Some(err) = partial(value)
                && !err.is_empty()
            {
                error_messages.push(err);
            }
        }

        if type_is(&prop, "boolean") && !matches!(value, Dynamic::Bool(_)) {
            error_messages.push(localize("validations.booleanIncorrectType", "Incorrect type. Expected \"boolean\".", &[]));
        }

        if is_numeric {
            let number = to_number(value);
            if is_null_or_empty(value) || matches!(value, Dynamic::Bool(_)) || value.is_array() || number.is_nan() {
                error_messages.push(localize("validations.expectedNumeric", "Value must be a number.", &[]));
            } else {
                error_messages.extend(failed_messages(&numeric_validations, &number));
            }
        }

        if type_is(&prop, "string") {
            let enum_ = prop.get_str("enum");
            if enum_.is_truthy() && !is_string_array(&enum_) {
                error_messages.push(localize(
                    "validations.stringIncorrectEnumOptions",
                    "The enum options should be strings, but there is a non-string option. Please file an issue with the extension author.",
                    &[],
                ));
            } else if let Dynamic::String(text) = value {
                error_messages.extend(failed_messages(&string_validations, text));
            } else {
                error_messages.push(localize("validations.stringIncorrectType", "Incorrect type. Expected \"string\".", &[]));
            }
        }

        join_errors(&prop, error_messages)
    })
}

/// O fim de `createValidator` e do validador de objeto: as mensagens separadas por espaço, com o `errorMessage` do
/// esquema na frente quando existe.
fn join_errors(prop: &Dynamic, error_messages: Vec<String>) -> String {
    if error_messages.is_empty() {
        return String::new();
    }
    let error_message = prop.get_str("errorMessage");
    if error_message.is_truthy() {
        let mut all = vec![text_of(&error_message)];
        all.extend(error_messages);
        return all.join(" ");
    }
    error_messages.join(" ")
}

/// Devolve uma mensagem de erro se o valor é inválido e não pode ser mostrado na UI de settings para o tipo dado.
pub fn get_invalid_type_error(value: &Dynamic, type_: &Dynamic) -> Option<String> {
    if type_.is_undefined() {
        return None;
    }

    let types = match type_ {
        Dynamic::Array(items) => items.to_vec(),
        other => vec![other.clone()],
    };
    if !types.iter().any(|t| value_validates_as_type(value, t)) {
        let expected = json_stringify(type_).unwrap_or_else(|| "undefined".to_owned());
        return Some(localize("invalidTypeError", "Setting has an invalid type, expected {0}. Fix in JSON.", &[&expected]));
    }

    None
}

fn value_validates_as_type(value: &Dynamic, type_: &Dynamic) -> bool {
    let Dynamic::String(type_) = type_ else {
        // nenhum nome de tipo conhecido
        return true;
    };
    match type_.to_string_lossy().as_str() {
        "boolean" => matches!(value, Dynamic::Bool(_)),
        "object" => value.type_of() == "object" && !value.is_null() && !value.is_array(),
        "null" => value.is_null(),
        "array" => value.is_array(),
        "string" => matches!(value, Dynamic::String(_)),
        "number" | "integer" => matches!(value, Dynamic::Number(_)),
        _ => true,
    }
}

fn to_reg_exp(pattern: &JsString) -> Rc<JsRegExp> {
    // The u flag allows support for better Unicode matching,
    // but deprecates some patterns such as [\s-9]
    // Ref https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Regular_expressions/Character_class#description
    let reg_exp = JsRegExp::new(pattern.units(), "u").or_else(|_| JsRegExp::new(pattern.units(), "")).unwrap_or_else(|_| {
        // If the pattern can't be parsed even without the 'u' flag,
        // just log the error to avoid rendering the entire Settings editor blank.
        // Ref https://github.com/microsoft/vscode/issues/195054
        eprintln!("{} {pattern}", localize("regexParsingError", "Error parsing the following regex both with and without the u flag:", &[]));
        JsRegExp::from_str(".*", "").expect("o padrão .* é uma expressão regular válida")
    });
    Rc::new(reg_exp)
}

/// O `uriRegex` de `getStringValidators`.
const URI_PATTERN: &str = r"^(([^:/?#]+?):)?(\/\/([^/?#]*))?([^?#]*)(\?([^#]*))?(#(.*))?";

fn get_string_validators(prop: &Dynamic) -> Vec<Validator<JsString>> {
    let uri_regex = Rc::new(JsRegExp::from_str(URI_PATTERN, "").expect("o padrão de URI é uma expressão regular válida"));
    let pattern = prop.get_str("pattern");
    let pattern_regex = match &pattern {
        Dynamic::String(text) => Some(to_reg_exp(text)),
        _ => None,
    };
    let max_length = prop.get_str("maxLength");
    let min_length = prop.get_str("minLength");
    let format = prop.get_str("format");
    let is_uri = is_text(&format, "uri") || is_text(&format, "uri-reference");
    let enum_ = prop.get_str("enum");

    let pattern_error_message = prop.get_str("patternErrorMessage");
    let pattern_message = if pattern_error_message.is_truthy() {
        text_of(&pattern_error_message)
    } else {
        localized("validations.regex", "Value must match regex `{0}`.", &[&pattern])
    };
    let enum_options = if enum_.is_truthy() {
        array_items(&enum_, "map").iter().map(|key| format!("\"{}\"", text_of(key))).collect::<Vec<_>>().join(", ")
    } else {
        "[]".to_owned()
    };

    let (max, min) = (to_number(&max_length), to_number(&min_length));
    let (uri_regex_for_shape, uri_regex_for_scheme) = (uri_regex.clone(), uri_regex);

    only_enabled(vec![
        validator(
            !max_length.is_undefined(),
            localized("validations.maxLength", "Value must be {0} or fewer characters long.", &[&max_length]),
            move |value: &JsString| value.len() as f64 <= max,
        ),
        validator(
            !min_length.is_undefined(),
            localized("validations.minLength", "Value must be {0} or more characters long.", &[&min_length]),
            move |value: &JsString| value.len() as f64 >= min,
        ),
        validator(pattern_regex.is_some(), pattern_message, move |value: &JsString| {
            pattern_regex.as_ref().is_some_and(|reg_exp| test(reg_exp, value.units()))
        }),
        validator(
            is_text(&format, "color-hex"),
            localize("validations.colorFormat", "Invalid color format. Use #RGB, #RGBA, #RRGGBB or #RRGGBBAA.", &[]),
            |value: &JsString| parse_hex(&value.to_string_lossy()).is_some(),
        ),
        validator(is_uri, localize("validations.uriEmpty", "URI expected.", &[]), |value: &JsString| !value.is_empty()),
        validator(is_uri, localize("validations.uriMissing", "URI is expected.", &[]), move |value: &JsString| {
            test(&uri_regex_for_shape, value.units())
        }),
        validator(
            is_text(&format, "uri"),
            localize("validations.uriSchemeMissing", "URI with a scheme is expected.", &[]),
            move |value: &JsString| {
                // `matches && matches[2]`: o grupo do esquema participou (e não é vazio, pelo `+?`)
                uri_regex_for_scheme.exec(value.units()).is_some_and(|m| m.captures.get(2).is_some_and(|c| c.as_ref().is_some_and(|r| !r.is_empty())))
            },
        ),
        validator(
            !enum_.is_undefined(),
            localized("validations.invalidStringEnumValue", "Value is not accepted. Valid values: {0}.", &[&Dynamic::string(&enum_options)]),
            move |value: &JsString| {
                array_items(&enum_, "includes").iter().any(|item| matches!(item, Dynamic::String(text) if text == value))
            },
        ),
    ])
}

/// O `exclusiveMax`/`exclusiveMin` de `getNumericValidators`: o booleano do esquema antigo (rascunho 4) liga o limite
/// comum como exclusivo; um número já é o limite exclusivo.
fn exclusive_bound(exclusive: &Dynamic, bound: &Dynamic) -> Dynamic {
    match exclusive {
        Dynamic::Bool(true) => bound.clone(),
        Dynamic::Bool(false) => Dynamic::Undefined,
        other => other.clone(),
    }
}

fn get_numeric_validators(prop: &Dynamic) -> Vec<Validator<f64>> {
    let type_ = schema_types(prop);
    let is_integral = is_only_type(&type_, &["integer"]);
    if !is_only_type(&type_, &["number", "integer"]) {
        return Vec::new();
    }

    let maximum = prop.get_str("maximum");
    let minimum = prop.get_str("minimum");
    let multiple_of = prop.get_str("multipleOf");
    let exclusive_max = exclusive_bound(&prop.get_str("exclusiveMaximum"), &maximum);
    let exclusive_min = exclusive_bound(&prop.get_str("exclusiveMinimum"), &minimum);
    let (max, min, emax, emin, multiple) =
        (to_number(&maximum), to_number(&minimum), to_number(&exclusive_max), to_number(&exclusive_min), to_number(&multiple_of));

    only_enabled(vec![
        validator(
            !exclusive_max.is_undefined() && (maximum.is_undefined() || emax <= max),
            localized("validations.exclusiveMax", "Value must be strictly less than {0}.", &[&exclusive_max]),
            move |value: &f64| *value < emax,
        ),
        validator(
            !exclusive_min.is_undefined() && (minimum.is_undefined() || emin >= min),
            localized("validations.exclusiveMin", "Value must be strictly greater than {0}.", &[&exclusive_min]),
            move |value: &f64| *value > emin,
        ),
        validator(
            !maximum.is_undefined() && (exclusive_max.is_undefined() || emax > max),
            localized("validations.max", "Value must be less than or equal to {0}.", &[&maximum]),
            move |value: &f64| *value <= max,
        ),
        validator(
            !minimum.is_undefined() && (exclusive_min.is_undefined() || emin < min),
            localized("validations.min", "Value must be greater than or equal to {0}.", &[&minimum]),
            move |value: &f64| *value >= min,
        ),
        validator(
            !multiple_of.is_undefined(),
            localized("validations.multipleOf", "Value must be a multiple of {0}.", &[&multiple_of]),
            move |value: &f64| *value % multiple == 0.0,
        ),
        validator(is_integral, localize("validations.expectedInteger", "Value must be an integer.", &[]), |value: &f64| *value % 1.0 == 0.0),
    ])
}

/// `withQuotes(s)`.
fn with_quotes(value: &Dynamic) -> String {
    format!("'{}'", text_of(value))
}

/// `new Set(values).size`: os distintos pelo `SameValueZero` (como o `===`, mas `NaN` é igual a `NaN`).
fn distinct_count(values: &[Dynamic]) -> usize {
    let mut distinct: Vec<&Dynamic> = Vec::new();
    for value in values {
        let seen = distinct.iter().any(|other| match (value, other) {
            (Dynamic::Number(a), Dynamic::Number(b)) if a.is_nan() && b.is_nan() => true,
            _ => value.strict_equals(other),
        });
        if !seen {
            distinct.push(value);
        }
    }
    distinct.len()
}

fn get_array_validator(prop: &Dynamic) -> Option<PartialValidator> {
    if !type_is(prop, "array") {
        return None;
    }
    let items = prop.get_str("items");
    if !items.is_truthy() || items.is_array() || items.get_str("type").is_array() {
        return None;
    }

    let prop = prop.clone();
    Some(Box::new(move |value| {
        if !value.is_truthy() {
            return None;
        }

        let mut message = String::new();

        let Dynamic::Array(array) = value else {
            message += &localize("validations.arrayIncorrectType", "Incorrect type. Expected an array.", &[]);
            message.push('\n');
            return Some(message);
        };

        let array_value = array.to_vec();
        if prop.get_str("uniqueItems").is_truthy() && distinct_count(&array_value) < array_value.len() {
            message += &localize("validations.stringArrayUniqueItems", "Array has duplicate items", &[]);
            message.push('\n');
        }

        let min_items = prop.get_str("minItems");
        if min_items.is_truthy() && (array_value.len() as f64) < to_number(&min_items) {
            message += &localized("validations.stringArrayMinItem", "Array must have at least {0} items", &[&min_items]);
            message.push('\n');
        }

        let max_items = prop.get_str("maxItems");
        if max_items.is_truthy() && (array_value.len() as f64) > to_number(&max_items) {
            message += &localized("validations.stringArrayMaxItem", "Array must have at most {0} items", &[&max_items]);
            message.push('\n');
        }

        let items_type = items.get_str("type");
        if is_text(&items_type, "string") {
            if !is_string_array(value) {
                message += &localize("validations.stringArrayIncorrectType", "Incorrect type. Expected a string array.", &[]);
                message.push('\n');
                return Some(message);
            }

            let pattern = items.get_str("pattern");
            if let Dynamic::String(pattern_text) = &pattern {
                let pattern_regex = to_reg_exp(pattern_text);
                let pattern_error_message = items.get_str("patternErrorMessage");
                for v in &array_value {
                    let Dynamic::String(text) = v else { continue };
                    if !test(&pattern_regex, text.units()) {
                        message += &if pattern_error_message.is_truthy() {
                            text_of(&pattern_error_message)
                        } else {
                            localize(
                                "validations.stringArrayItemPattern",
                                "Value {0} must match regex {1}.",
                                &[&with_quotes(v), &with_quotes(&pattern)],
                            )
                        };
                    }
                }
            }

            let items_enum = items.get_str("enum");
            if items_enum.is_truthy() {
                let enum_values = array_items(&items_enum, "indexOf");
                for v in &array_value {
                    if !enum_values.iter().any(|candidate| candidate.strict_equals(v)) {
                        let options = format!("[{}]", enum_values.iter().map(with_quotes).collect::<Vec<_>>().join(", "));
                        message += &localize("validations.stringArrayItemEnum", "Value {0} is not one of {1}", &[&with_quotes(v), &options]);
                        message.push('\n');
                    }
                }
            }
        } else if is_text(&items_type, "integer") || is_text(&items_type, "number") {
            for v in &array_value {
                let error_message = get_errors_for_schema(&items, v);
                if !error_message.is_empty() {
                    message += &format!("{}: {error_message}\n", text_of(v));
                }
            }
        }

        Some(message)
    }))
}

/// `propertyNames?.pattern`.
fn property_names_pattern(property_names: &Dynamic) -> Dynamic {
    if property_names.is_nullish() { Dynamic::Undefined } else { property_names.get_str("pattern") }
}

fn get_object_validator(prop: &Dynamic) -> Option<PartialValidator> {
    if !type_is(prop, "object") {
        return None;
    }
    let properties = prop.get_str("properties");
    let pattern_properties = prop.get_str("patternProperties");
    let additional_properties = prop.get_str("additionalProperties");
    let property_names = prop.get_str("propertyNames");
    let prop = prop.clone();

    Some(Box::new(move |value| {
        if !value.is_truthy() {
            return None;
        }

        let mut error_messages: Vec<String> = Vec::new();
        let mut property_names_error_shown = false;

        if !is_object(value) {
            error_messages.push(localize("validations.objectIncorrectType", "Incorrect type. Expected an object.", &[]));
        } else {
            for (key, data) in for_in_entries(value) {
                // Validate propertyNames.pattern - show error message once
                let names_pattern = property_names_pattern(&property_names);
                if names_pattern.is_truthy() && !property_names_error_shown {
                    let pattern_regex = to_reg_exp(&to_js_string(&names_pattern));
                    if !test(&pattern_regex, key.units()) {
                        let pattern_error_message = property_names.get_str("patternErrorMessage");
                        let error_message = if pattern_error_message.is_truthy() {
                            text_of(&pattern_error_message)
                        } else {
                            localized("validations.propertyNamePattern", "Property name must match pattern `{0}`.", &[&names_pattern])
                        };
                        error_messages.push(error_message + "\n");
                        property_names_error_shown = true;
                    }
                }

                if properties.is_truthy() && properties.has_property(&key) {
                    let error_message = get_errors_for_schema(&properties.get(&key), &data);
                    if !error_message.is_empty() {
                        error_messages.push(format!("{key}: {error_message}\n"));
                    }
                    continue;
                }

                if pattern_properties.is_truthy() {
                    let matching = for_in_entries(&pattern_properties).into_iter().find(|(pattern, _)| {
                        // `RegExp(pattern)` lança `SyntaxError` para um padrão inválido, sem o fallback do `toRegExp`
                        let reg_exp = JsRegExp::new(pattern.units(), "")
                            .unwrap_or_else(|error| errors::throw(Error::named("SyntaxError", error.to_string())));
                        test(&reg_exp, key.units())
                    });
                    if let Some((_, schema)) = matching {
                        let error_message = get_errors_for_schema(&schema, &data);
                        if !error_message.is_empty() {
                            error_messages.push(format!("{key}: {error_message}\n"));
                        }
                        continue;
                    }
                }

                if matches!(additional_properties, Dynamic::Bool(false)) {
                    error_messages.push(localize("validations.objectPattern", "Property {0} is not allowed.\n", &[&key.to_string_lossy()]));
                } else if additional_properties.type_of() == "object" {
                    let error_message = get_errors_for_schema(&additional_properties, &data);
                    if !error_message.is_empty() {
                        error_messages.push(format!("{key}: {error_message}\n"));
                    }
                }
            }
        }

        Some(join_errors(&prop, error_messages))
    }))
}

/// Valida um único nome de propriedade contra o `propertyNames.pattern` do esquema. Devolve `true` se a chave vale.
pub fn validate_property_name(property_names: &Dynamic, key: &JsString) -> bool {
    let pattern = property_names_pattern(property_names);
    if !pattern.is_truthy() {
        return true;
    }
    test(&to_reg_exp(&to_js_string(&pattern)), key.units())
}

fn get_errors_for_schema(property_schema: &Dynamic, data: &Dynamic) -> String {
    create_validator(property_schema)(data)
}
