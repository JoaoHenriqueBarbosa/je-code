//! Porte de `src/vs/base/common/validation.ts`: validadores combináveis que checam um valor
//! desconhecido em tempo de execução e descrevem o schema JSON equivalente.
//!
//! Os tipos genéricos do original (`vObjType<T>`, `ValidatorType<T>`) só existem em tempo de
//! compilação do TypeScript; em Rust o conteúdo validado é um [`Dynamic`] (o valor JavaScript) e o
//! schema devolvido por `get_json_schema` é um objeto [`Dynamic`] (ver [`crate::json_schema`]).
//! `validateOrThrow` devolve o [`ValidationError`] em vez de lançar.

use std::fmt;
use std::rc::Rc;

use crate::JsString;
use crate::dynamic::Dynamic;

/// `ValidationError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ValidationError {}

fn error<T>(message: String) -> Result<T, ValidationError> {
    Err(ValidationError { message })
}

/// `IValidator`/`ValidatorBase`: o resultado é o conteúdo validado (uma cópia nova para objetos e
/// arrays) ou o erro.
pub trait Validator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError>;

    fn get_json_schema(&self) -> Dynamic;

    /// `validateOrThrow`.
    fn validate_or_throw(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        self.validate(content)
    }

    /// `validator instanceof UndefinedValidator`, que o `vUnion` usa para omitir o ramo do schema.
    #[doc(hidden)]
    fn is_undefined_validator(&self) -> bool {
        false
    }
}

/// Um validador compartilhável (os combinadores guardam os filhos por referência).
pub type ValidatorRef = Rc<dyn Validator>;

fn schema_object<'a>(entries: impl IntoIterator<Item = (&'a str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

/// `typeof` esperado de um [`TypeofValidator`].
struct TypeofValidator(&'static str);

impl Validator for TypeofValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        if content.type_of() != self.0 {
            return error(format!("Expected {}, but got {}", self.0, content.type_of()));
        }
        Ok(content.clone())
    }

    fn get_json_schema(&self) -> Dynamic {
        schema_object([("type", Dynamic::string(self.0))])
    }
}

pub fn v_string() -> ValidatorRef {
    Rc::new(TypeofValidator("string"))
}

pub fn v_number() -> ValidatorRef {
    Rc::new(TypeofValidator("number"))
}

pub fn v_boolean() -> ValidatorRef {
    Rc::new(TypeofValidator("boolean"))
}

pub fn v_obj_any() -> ValidatorRef {
    Rc::new(TypeofValidator("object"))
}

struct UncheckedValidator;

impl Validator for UncheckedValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        Ok(content.clone())
    }

    fn get_json_schema(&self) -> Dynamic {
        schema_object([])
    }
}

pub fn v_unchecked() -> ValidatorRef {
    Rc::new(UncheckedValidator)
}

struct UndefinedValidator;

impl Validator for UndefinedValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        if !content.is_undefined() {
            return error(format!("Expected undefined, but got {}", content.type_of()));
        }
        Ok(Dynamic::Undefined)
    }

    fn get_json_schema(&self) -> Dynamic {
        schema_object([])
    }

    fn is_undefined_validator(&self) -> bool {
        true
    }
}

pub fn v_undefined() -> ValidatorRef {
    Rc::new(UndefinedValidator)
}

pub fn v_unknown() -> ValidatorRef {
    v_unchecked()
}

/// Uma propriedade de `vObj`: obrigatória, ou embrulhada em `vOptionalProp`.
#[derive(Clone)]
pub enum Property {
    Required(ValidatorRef),
    Optional(ValidatorRef),
}

impl From<ValidatorRef> for Property {
    fn from(validator: ValidatorRef) -> Self {
        Self::Required(validator)
    }
}

/// `vOptionalProp`.
pub fn v_optional_prop(validator: ValidatorRef) -> Property {
    Property::Optional(validator)
}

struct ObjValidator(Vec<(JsString, Property)>);

impl Property {
    fn parts(&self) -> (bool, &ValidatorRef) {
        match self {
            Self::Required(v) => (false, v),
            Self::Optional(v) => (true, v),
        }
    }
}

impl Validator for ObjValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        if content.type_of() != "object" || content.is_null() {
            return error("Expected object".to_string());
        }

        let mut result: Vec<(JsString, Dynamic)> = Vec::new();
        for (key, prop) in &self.0 {
            let field_value = content.get(key);
            let (is_optional, validator) = prop.parts();
            if is_optional && field_value.is_undefined() {
                // Campo opcional ausente: não valida.
                continue;
            }
            match validator.validate(&field_value) {
                Err(e) => return error(format!("Error in property '{key}': {}", e.message)),
                Ok(value) => result.push((key.clone(), value)),
            }
        }
        Ok(Dynamic::object(result))
    }

    fn get_json_schema(&self) -> Dynamic {
        let mut required_fields: Vec<Dynamic> = Vec::new();
        let mut schema_properties: Vec<(JsString, Dynamic)> = Vec::new();
        for (key, prop) in &self.0 {
            let (is_optional, validator) = prop.parts();
            schema_properties.push((key.clone(), validator.get_json_schema()));
            if !is_optional {
                required_fields.push(Dynamic::String(key.clone()));
            }
        }
        let mut entries = vec![("type", Dynamic::string("object")), ("properties", Dynamic::object(schema_properties))];
        if !required_fields.is_empty() {
            entries.push(("required", Dynamic::array(required_fields)));
        }
        schema_object(entries)
    }
}

/// `vObj`: as propriedades na ordem em que o original as iteraria (`for ... in`).
pub fn v_obj<K: Into<JsString>>(properties: impl IntoIterator<Item = (K, Property)>) -> ValidatorRef {
    let properties: Vec<(JsString, Property)> = properties.into_iter().map(|(k, p)| (k.into(), p)).collect();
    // Objetos JavaScript iteram primeiro as chaves que são índices de array: reaproveita a ordenação do `Dynamic`.
    let order = Dynamic::object(properties.iter().map(|(k, _)| (k.clone(), Dynamic::Null))).own_keys();
    let mut ordered: Vec<(JsString, Property)> = Vec::with_capacity(properties.len());
    for key in order {
        // Chave repetida: a última atribuição vale, na posição da primeira.
        if let Some((_, p)) = properties.iter().rev().find(|(k, _)| *k == key) {
            ordered.push((key, p.clone()));
        }
    }
    Rc::new(ObjValidator(ordered))
}

struct ArrayValidator(ValidatorRef);

impl Validator for ArrayValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        let Dynamic::Array(array) = content else {
            return error("Expected array".to_string());
        };
        let mut result = Vec::new();
        for (i, item) in array.to_vec().iter().enumerate() {
            match self.0.validate(item) {
                Err(e) => return error(format!("Error in element {i}: {}", e.message)),
                Ok(value) => result.push(value),
            }
        }
        Ok(Dynamic::array(result))
    }

    fn get_json_schema(&self) -> Dynamic {
        schema_object([("type", Dynamic::string("array")), ("items", self.0.get_json_schema())])
    }
}

pub fn v_array(validator: ValidatorRef) -> ValidatorRef {
    Rc::new(ArrayValidator(validator))
}

struct TupleValidator(Vec<ValidatorRef>);

impl Validator for TupleValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        let Dynamic::Array(array) = content else {
            return error("Expected array".to_string());
        };
        let items = array.to_vec();
        if items.len() != self.0.len() {
            return error(format!("Expected tuple of length {}, but got {}", self.0.len(), items.len()));
        }
        let mut result = Vec::new();
        for (i, validator) in self.0.iter().enumerate() {
            match validator.validate(&items[i]) {
                Err(e) => return error(format!("Error in element {i}: {}", e.message)),
                Ok(value) => result.push(value),
            }
        }
        Ok(Dynamic::array(result))
    }

    fn get_json_schema(&self) -> Dynamic {
        schema_object([
            ("type", Dynamic::string("array")),
            ("items", Dynamic::array(self.0.iter().map(|v| v.get_json_schema()))),
        ])
    }
}

pub fn v_tuple(validators: impl IntoIterator<Item = ValidatorRef>) -> ValidatorRef {
    Rc::new(TupleValidator(validators.into_iter().collect()))
}

struct UnionValidator(Vec<ValidatorRef>);

impl Validator for UnionValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        let mut last_error: Option<ValidationError> = None;
        for validator in &self.0 {
            match validator.validate(content) {
                Ok(value) => return Ok(value),
                Err(e) => last_error = Some(e),
            }
        }
        // Sem validadores o original devolve `{ content: undefined, error: undefined }`: sucesso com `undefined`.
        match last_error {
            Some(e) => Err(e),
            None => Ok(Dynamic::Undefined),
        }
    }

    fn get_json_schema(&self) -> Dynamic {
        schema_object([(
            "oneOf",
            Dynamic::array(self.0.iter().filter(|v| !v.is_undefined_validator()).map(|v| v.get_json_schema())),
        )])
    }
}

pub fn v_union(validators: impl IntoIterator<Item = ValidatorRef>) -> ValidatorRef {
    Rc::new(UnionValidator(validators.into_iter().collect()))
}

struct EnumValidator(Vec<JsString>);

impl Validator for EnumValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        if let Dynamic::String(s) = content
            && self.0.contains(s)
        {
            return Ok(content.clone());
        }
        let values: Vec<String> = self.0.iter().map(JsString::to_string_lossy).collect();
        error(format!("Expected one of: {}", values.join(", ")))
    }

    fn get_json_schema(&self) -> Dynamic {
        schema_object([("enum", Dynamic::array(self.0.iter().map(|v| Dynamic::String(v.clone()))))])
    }
}

pub fn v_enum<S: Into<JsString>>(values: impl IntoIterator<Item = S>) -> ValidatorRef {
    Rc::new(EnumValidator(values.into_iter().map(Into::into).collect()))
}

struct LiteralValidator(JsString);

impl Validator for LiteralValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        match content {
            Dynamic::String(s) if *s == self.0 => Ok(content.clone()),
            _ => error(format!("Expected: {}", self.0)),
        }
    }

    fn get_json_schema(&self) -> Dynamic {
        schema_object([("const", Dynamic::String(self.0.clone()))])
    }
}

pub fn v_literal(value: impl Into<JsString>) -> ValidatorRef {
    Rc::new(LiteralValidator(value.into()))
}

struct LazyValidator(Box<dyn Fn() -> ValidatorRef>);

impl Validator for LazyValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        (self.0)().validate(content)
    }

    fn get_json_schema(&self) -> Dynamic {
        (self.0)().get_json_schema()
    }
}

pub fn v_lazy(f: impl Fn() -> ValidatorRef + 'static) -> ValidatorRef {
    Rc::new(LazyValidator(Box::new(f)))
}

struct UseRefSchemaValidator {
    reference: JsString,
    validator: ValidatorRef,
}

impl Validator for UseRefSchemaValidator {
    fn validate(&self, content: &Dynamic) -> Result<Dynamic, ValidationError> {
        self.validator.validate(content)
    }

    fn get_json_schema(&self) -> Dynamic {
        schema_object([("$ref", Dynamic::String(self.reference.clone()))])
    }
}

pub fn v_with_json_schema_ref(reference: impl Into<JsString>, validator: ValidatorRef) -> ValidatorRef {
    Rc::new(UseRefSchemaValidator { reference: reference.into(), validator })
}
