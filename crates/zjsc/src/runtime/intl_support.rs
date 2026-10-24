//! A cola comum das classes do `Intl` (`IntlObject.cpp`, `IntlObjectInlines.h` e os construtores
//! `Intl*Constructor.cpp`): a célula que guarda o estado de cada instância, a instalação de
//! construtor e protótipo, `CanonicalizeLocaleList`, `GetOption` e os objetos de partes.
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//!
//! - O C++ tem uma classe de célula por tipo (`IntlNumberFormat`, `IntlCollator`...). Aqui uma só célula,
//!   [`IntlInstance`], guarda o estado como `Box<dyn Any>` e cada `intl_*.rs` o baixa para a própria
//!   struct: o que o `dynamicDowncast<IntlNumberFormat>` confere é o `downcast_ref` do estado.
//! - `ResolveLocale` com `localeMatcher` `best fit` e `lookup` é o mesmo (`intl_locale_data.rs` explica as
//!   locales cobertas).
//! - O contorno de compatibilidade do ECMA-402 1.0 (`Intl.NumberFormat.call(thisObject)` guardando a
//!   instância sob `intlLegacyConstructedSymbol`) não existe: chamar sem `new` cria uma instância nova.
//! - `JSC_TO_STRING_TAG_WITHOUT_TRANSITION` é o `put_to_string_tag` de `collection_support.rs`.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{create_collection_constructor_with,collection_constructor_structure, put_native_function, put_to_string_tag};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::intl_locale_data::{self, ResolvedLocale};
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_bound_function::JSBoundFunction;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::{js_empty_string, js_string};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::lookup::{HashTableValue, Kind};
use crate::runtime::lookup::{custom_getter_entry, native_entry};
use crate::runtime::property_attribute::{ACCESSOR, CUSTOM_ACCESSOR, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_slot::GetValueFunc;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::{String as WtfString};

/// `const ClassInfo` das instâncias do `Intl` (`"Object"`: o nome da classe de cada uma sai do
/// `@@toStringTag` do protótipo).
pub static INTL_INSTANCE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo` dos construtores (`"Function"`).
static INTL_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

/// A instância de uma classe do `Intl`: `JSNonFinalObject` com o estado da classe e a função `format` ou
/// `compare` já ligada (`m_boundFormat`, `m_boundCompare`).
pub struct IntlInstance {
    base: JSNonFinalObject,
    state: Box<dyn Any>,
    bound: Cell<JSValue>,
}

/// A referência à célula, o `*` do C++.
pub type IntlInstanceRef = Rc<IntlInstance>;

impl std::ops::Deref for IntlInstance {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl IntlInstance {
    /// `createStructure(vm, globalObject, prototype)`: `TypeInfo(ObjectType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
            &INTL_INSTANCE_S_INFO,
        )
    }

    /// `create(vm, structure)` com o estado já inicializado.
    pub fn create(vm: &VM, structure: &StructureRef, state: Box<dyn Any>) -> IntlInstanceRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(IntlInstance {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            state,
            bound: Cell::new(JSValue::empty()),
        });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::IntlInstance(Rc::clone(&cell)));
        cell
    }

    /// `visitChildren`: a função ligada (`m_boundFormat`, `m_boundCompare`) e o que o estado da classe segura. O
    /// estado é `Box<dyn Any>`: as classes cujo estado guarda valores do programa (as de WebAssembly) marcam os dela.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.bound.get());
        crate::runtime::js_web_assembly::visit_state(&*self.state, marker);
        crate::runtime::js_web_assembly_exception::visit_state(&*self.state, marker);
    }

    /// `dynamicDowncast<IntlInstance>(value)`.
    pub fn from_value(value: &JSValue) -> Option<IntlInstanceRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::IntlInstance(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// O estado da classe `T`, ou `None` se a instância é de outra classe.
    pub fn state<T: Any>(&self) -> Option<&T> {
        self.state.downcast_ref::<T>()
    }
}

/// `dynamicDowncast<IntlX>(thisValue)` e o `TypeError` de `X.prototype.método called on value that's not a
/// X`: roda `body` com o estado e a instância.
pub fn with_instance<T: Any, R>(
    this_value: JSValue,
    message: &str,
    body: impl FnOnce(&T, &IntlInstance) -> Result<R, Thrown>,
) -> Result<R, Thrown> {
    let instance = IntlInstance::from_value(&this_value).ok_or_else(|| Thrown::type_error(message))?;
    let state = instance.state::<T>().ok_or_else(|| Thrown::type_error(message))?;
    body(state, &instance)
}

/// A criação da instância de `new X(...)`: a estrutura derivada do `new.target` e a célula com o estado.
pub fn construct_instance(
    global_object: &JSGlobalObject,
    call: &HostCall,
    make_state: impl FnOnce(&JSGlobalObject) -> Result<Box<dyn Any>, Thrown>,
) -> HostResult {
    let structure = crate::runtime::collection_support::derived_structure(global_object, call, IntlInstance::create_structure)?;
    create_instance(global_object, &structure, make_state)
}

/// A criação da instância de `X(...)` sem `new` (`callCollator`, `callNumberFormat`, `callDateTimeFormat`): a
/// estrutura vem do `prototype` do próprio construtor e `newTarget()` (que no quadro é o `this`) nunca é lido.
pub fn call_instance(
    global_object: &JSGlobalObject,
    call: &HostCall,
    make_state: impl FnOnce(&JSGlobalObject) -> Result<Box<dyn Any>, Thrown>,
) -> HostResult {
    let structure = crate::runtime::collection_support::callee_structure(global_object, call, IntlInstance::create_structure)?;
    create_instance(global_object, &structure, make_state)
}

fn create_instance(
    global_object: &JSGlobalObject,
    structure: &StructureRef,
    make_state: impl FnOnce(&JSGlobalObject) -> Result<Box<dyn Any>, Thrown>,
) -> HostResult {
    let state = make_state(global_object)?;
    Ok(IntlInstance::create(global_object.vm(), structure, state).as_value())
}

// ---------------------------------------------------------------------------------------------
// Valores e objetos
// ---------------------------------------------------------------------------------------------

/// `jsString(vm, text)`.
pub fn str_value(vm: &VM, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes())))
}

/// `jsString(vm, units)`: o texto em UTF-16, com surrogate solto preservado.
pub fn units_value(vm: &VM, units: &[u16]) -> JSValue {
    JSValue::from_js_string(js_string(vm, &crate::runtime::string_prototype::string_from_units(units)))
}

/// O `PropertyName` de um nome literal.
pub fn prop(vm: &VM, name: &str) -> PropertyName {
    PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes()))
}

/// `constructEmptyObject(globalObject)`.
pub fn new_object(global_object: &JSGlobalObject) -> JSObjectRef {
    JSFinalObject::create(global_object.vm(), &global_object.object_structure_for_object_constructor())
}

/// `object->putDirect(vm, Identifier::fromString(vm, name), value)`.
pub fn put(global_object: &JSGlobalObject, object: &JSObject, name: &str, value: JSValue) {
    let vm = global_object.vm();
    object.put_direct(vm, &prop(vm, name), value, 0);
}

/// `constructArray(globalObject, ArrayWithContiguous, values)`.
pub fn array_of(global_object: &JSGlobalObject, values: &[JSValue]) -> JSValue {
    construct_array(global_object.vm(), &global_object.array_structure(), values).as_value()
}

/// `createArrayFromStringVector`.
pub fn string_array(global_object: &JSGlobalObject, items: &[String]) -> JSValue {
    let vm = global_object.vm();
    let values: Vec<JSValue> = items.iter().map(|item| str_value(vm, item)).collect();
    array_of(global_object, &values)
}

/// `createIntlPartObject(globalObject, type, value)`: `{ type, value }`.
pub fn part_object(global_object: &JSGlobalObject, kind: &str, value: &str) -> JSValue {
    let vm = global_object.vm();
    let object = new_object(global_object);
    put(global_object, &object, "type", str_value(vm, kind));
    put(global_object, &object, "value", str_value(vm, value));
    object.as_value()
}

/// As partes de `formatToParts`: um array de `{ type, value }` a partir de pares (tipo, texto).
pub fn parts_array(global_object: &JSGlobalObject, parts: &[(String, String)]) -> JSValue {
    let values: Vec<JSValue> = parts.iter().map(|(kind, text)| part_object(global_object, kind, text)).collect();
    array_of(global_object, &values)
}

/// A origem de uma parte de `formatRangeToParts` (`source`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RangeSource {
    Shared,
    StartRange,
    EndRange,
}

impl RangeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            RangeSource::Shared => "shared",
            RangeSource::StartRange => "startRange",
            RangeSource::EndRange => "endRange",
        }
    }
}

/// Uma parte de `formatRangeToParts`: tipo, texto e origem.
pub type RangePart = (String, String, RangeSource);

/// `createIntlPartObjectWithSource(globalObject, type, value, source)`: `{ type, value, source }`.
pub fn part_object_with_source(global_object: &JSGlobalObject, kind: &str, value: &str, source: RangeSource) -> JSValue {
    let vm = global_object.vm();
    let object = new_object(global_object);
    put(global_object, &object, "type", str_value(vm, kind));
    put(global_object, &object, "value", str_value(vm, value));
    put(global_object, &object, "source", str_value(vm, source.as_str()));
    object.as_value()
}

/// As partes de `formatRangeToParts`: um array de `{ type, value, source }`.
pub fn range_parts_array(global_object: &JSGlobalObject, parts: &[RangePart]) -> JSValue {
    let values: Vec<JSValue> =
        parts.iter().map(|(kind, text, source)| part_object_with_source(global_object, kind, text, *source)).collect();
    array_of(global_object, &values)
}

/// O texto como `String` do Rust (um substituto isolado vira U+FFFD).
pub fn wtf_to_rust(text: &WtfString) -> String {
    String::from_utf8_lossy(&text.utf8(ConversionMode::LenientConversion)).into_owned()
}

/// Aplica `map` a cada trecho UTF-16 válido do texto e preserva os substitutos isolados, como o
/// `u_strToUpper`/`u_strToLower` do ICU.
pub fn map_utf16_segments(text: &WtfString, map: impl Fn(&str) -> String) -> WtfString {
    let units = crate::runtime::string_prototype::code_units(text);
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let mut segment = String::new();
    let flush = |segment: &mut String, out: &mut Vec<u16>| {
        if !segment.is_empty() {
            out.extend(map(segment).encode_utf16());
            segment.clear();
        }
    };
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(c) => segment.push(c),
            Err(error) => {
                flush(&mut segment, &mut out);
                out.push(error.unpaired_surrogate());
            }
        }
    }
    flush(&mut segment, &mut out);
    crate::runtime::string_prototype::string_from_units(&out)
}

/// `value.toWTFString(globalObject)` como `String` do Rust, com a exceção pendente como `Err`.
pub fn to_rust_string(global_object: &JSGlobalObject, value: JSValue) -> Result<String, Thrown> {
    let text = value.to_wtf_string();
    let text = pending_or(global_object, text)?;
    Ok(wtf_to_rust(&text))
}

/// `value.toWTFString(globalObject)` sem conversão, com a exceção pendente como `Err`.
pub fn to_wtf_checked(global_object: &JSGlobalObject, value: JSValue) -> Result<WtfString, Thrown> {
    let text = value.to_wtf_string();
    pending_or(global_object, text)
}

/// `value.toNumber(globalObject)` com a exceção pendente como `Err`.
pub fn to_number_checked(global_object: &JSGlobalObject, value: JSValue) -> Result<f64, Thrown> {
    let number = value.to_number();
    pending_or(global_object, number)
}

/// `object->get(globalObject, name)`.
pub fn get_property(global_object: &JSGlobalObject, object: JSValue, name: &str) -> Result<JSValue, Thrown> {
    get_value_property(global_object, object, &prop(global_object.vm(), name))
}

// ---------------------------------------------------------------------------------------------
// Opções
// ---------------------------------------------------------------------------------------------

/// `intlGetOptionsObject`: `undefined` é sem opções, objeto passa, o resto é `TypeError`.
pub fn get_options_object(options: JSValue) -> Result<Option<JSValue>, Thrown> {
    if options.is_undefined() {
        return Ok(None);
    }
    if options.is_object() {
        return Ok(Some(options));
    }
    Err(Thrown::type_error("options argument is not an object or undefined"))
}

/// `intlCoerceOptionsToObject`: `undefined` é sem opções, o resto passa por `toObject`.
pub fn coerce_options_to_object(global_object: &JSGlobalObject, options: JSValue) -> Result<Option<JSValue>, Thrown> {
    if options.is_undefined() {
        return Ok(None);
    }
    let object: ObjectRef = options.to_object(global_object).ok_or(Thrown::Pending)?;
    Ok(Some(object.as_value()))
}

/// `intlStringOption` e `intlOption`: o texto da opção (`None` se `undefined`), conferido contra
/// `allowed` (vazio aceita qualquer texto) com o `RangeError` de `message`.
pub fn option_string(
    global_object: &JSGlobalObject,
    options: Option<JSValue>,
    name: &str,
    allowed: &[&str],
    message: &str,
) -> Result<Option<String>, Thrown> {
    let Some(options) = options else { return Ok(None) };
    let value = get_property(global_object, options, name)?;
    if value.is_undefined() {
        return Ok(None);
    }
    let text = to_rust_string(global_object, value)?;
    if !allowed.is_empty() && !allowed.contains(&text.as_str()) {
        return Err(Thrown::range_error(message));
    }
    Ok(Some(text))
}

/// Um enum de opção do `Intl` com o texto de cada valor (o `std::initializer_list<std::pair<ASCIILiteral,
/// ResultType>>` de `intlOption`): `intl_enum!(Style { Decimal => "decimal", Percent => "percent" })`.
#[macro_export]
macro_rules! intl_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum $name {
            $($variant),+
        }

        impl $crate::runtime::intl_support::IntlEnum for $name {
            const NAMES: &'static [&'static str] = &[$($text),+];

            fn parse(text: &str) -> Option<$name> {
                match text {
                    $($text => Some($name::$variant),)+
                    _ => None,
                }
            }

            fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }
    };
}

/// O que `intl_enum!` implementa.
pub trait IntlEnum: Sized + Copy {
    /// Os textos aceitos, na ordem da declaração.
    const NAMES: &'static [&'static str];
    /// O valor de um texto aceito.
    fn parse(text: &str) -> Option<Self>;
    /// O texto do valor (`styleString`, `notationString`...).
    fn as_str(self) -> &'static str;
}

/// `intlOption<E>(globalObject, options, property, values, notFoundMessage, fallback)` sem o `fallback`:
/// `None` quando a opção é `undefined`.
pub fn option_enum<E: IntlEnum>(
    global_object: &JSGlobalObject,
    options: Option<JSValue>,
    name: &str,
    message: &str,
) -> Result<Option<E>, Thrown> {
    let text = option_string(global_object, options, name, E::NAMES, message)?;
    Ok(text.and_then(|text| E::parse(&text)))
}

/// `intlBooleanOption`: `None` é o `TriState::Indeterminate`.
pub fn option_bool(global_object: &JSGlobalObject, options: Option<JSValue>, name: &str) -> Result<Option<bool>, Thrown> {
    let Some(options) = options else { return Ok(None) };
    let value = get_property(global_object, options, name)?;
    if value.is_undefined() {
        return Ok(None);
    }
    Ok(Some(value.to_boolean()))
}

/// `intlDefaultNumberOption(value, property, minimum, maximum, fallback)`.
pub fn default_number_option(
    global_object: &JSGlobalObject,
    value: JSValue,
    name: &str,
    minimum: u32,
    maximum: u32,
) -> Result<Option<u32>, Thrown> {
    if value.is_undefined() {
        return Ok(None);
    }
    let number = to_number_checked(global_object, value)?;
    if !(number >= f64::from(minimum) && number <= f64::from(maximum)) {
        return Err(Thrown::range_error(&format!("{name} is out of range")));
    }
    Ok(Some(number as u32))
}

/// `intlNumberOption(options, property, minimum, maximum, fallback)`.
pub fn number_option(
    global_object: &JSGlobalObject,
    options: Option<JSValue>,
    name: &str,
    minimum: u32,
    maximum: u32,
) -> Result<Option<u32>, Thrown> {
    let Some(options) = options else { return Ok(None) };
    let value = get_property(global_object, options, name)?;
    default_number_option(global_object, value, name, minimum, maximum)
}

/// A leitura de `localeMatcher`: só a validação (`lookup` e `best fit` dão o mesmo resultado).
pub fn read_locale_matcher(global_object: &JSGlobalObject, options: Option<JSValue>) -> Result<(), Thrown> {
    option_string(
        global_object,
        options,
        "localeMatcher",
        &["lookup", "best fit"],
        "localeMatcher must be either \"lookup\" or \"best fit\"",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Locales
// ---------------------------------------------------------------------------------------------

/// `canonicalizeLocaleList(globalObject, locales)`.
pub fn canonicalize_locale_list(global_object: &JSGlobalObject, locales: JSValue) -> Result<Vec<String>, Thrown> {
    let mut seen: Vec<String> = Vec::new();
    if locales.is_undefined() {
        return Ok(seen);
    }

    let list = if locales.is_string() || crate::runtime::intl_locale::locale_tag_of(&locales).is_some() {
        array_of(global_object, &[locales])
    } else {
        locales.to_object(global_object).ok_or(Thrown::Pending)?.as_value()
    };

    let length_value = get_property(global_object, list, "length")?;
    let length = length_value.to_integer_or_infinity();
    let length = pending_or(global_object, length)?;
    let length = if length <= 0.0 { 0.0 } else { length.min(9_007_199_254_740_991.0) };

    let vm = global_object.vm();
    let object = list.as_object();
    let mut index = 0.0;
    while index < length {
        let present = crate::runtime::proxy_object::object_has_property(
            global_object,
            &object,
            &PropertyName::from_identifier(&Identifier::from_string(vm, &WtfString::number_f64(index))),
        )?;
        if present {
            let value = crate::runtime::proxy_object::object_get(
                global_object,
                &object,
                &PropertyName::from_identifier(&Identifier::from_string(vm, &WtfString::number_f64(index))),
                list,
            )?;
            if !value.is_string() && !value.is_object() {
                return Err(Thrown::type_error("locale value must be a string or object"));
            }
            let tag = match crate::runtime::intl_locale::locale_tag_of(&value) {
                Some(tag) => tag,
                None => to_rust_string(global_object, value)?,
            };
            let Some(canonical) = intl_locale_data::canonicalize_tag(&tag) else {
                return Err(Thrown::range_error(&format!("invalid language tag: {tag}")));
            };
            if !seen.contains(&canonical) {
                seen.push(canonical);
            }
        }
        index += 1.0;
    }
    Ok(seen)
}

/// `canonicalizeLocaleList` e `ResolveLocale` com as chaves `-u-` relevantes da classe.
pub fn resolve_locale_from(
    global_object: &JSGlobalObject,
    locales: JSValue,
    relevant_keys: &[&str],
) -> Result<ResolvedLocale, Thrown> {
    let requested = canonicalize_locale_list(global_object, locales)?;
    Ok(intl_locale_data::resolve_locale(&requested, relevant_keys))
}

/// `supportedLocalesOf(locales [, options])` das classes com a função estática.
fn supported_locales_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let requested = canonicalize_locale_list(global_object, call.argument(0))?;
    let options = coerce_options_to_object(global_object, call.argument(1))?;
    read_locale_matcher(global_object, options)?;
    Ok(string_array(global_object, &intl_locale_data::supported_locales(&requested)))
}

crate::host_function!(supported_locales_of, supported_locales_of_body);

// ---------------------------------------------------------------------------------------------
// Instalação
// ---------------------------------------------------------------------------------------------

/// O que muda de uma classe do `Intl` para outra na criação do construtor.
pub struct IntlClass {
    /// `"NumberFormat"`.
    pub name: &'static str,
    /// O `length` do construtor (0, e 1 em `Locale`).
    pub length: u32,
    /// Tem `supportedLocalesOf` (todas menos `Locale`).
    pub has_supported_locales_of: bool,
    pub call: NativeFunction,
    pub construct: NativeFunction,
}

impl IntlClass {
    /// Cria protótipo e construtor, liga `constructor`, `@@toStringTag` e `supportedLocalesOf` e põe o
    /// construtor em `Intl` (`DontEnum`). Devolve o protótipo, para o chamador instalar os métodos.
    pub fn install(&self, global_object: &JSGlobalObject, intl: &JSObject) -> JSObjectRef {
        self.install_with(global_object, intl, |_| {})
    }

    /// `install`, com `members` rodando no protótipo antes de `constructor` entrar (a ordem das chaves próprias
    /// do protótipo, como em `WebAssembly.Exception`: `getArg`, `is`, `stack`, `constructor`).
    pub fn install_with(&self, global_object: &JSGlobalObject, intl: &JSObject, members: impl FnOnce(&JSObject)) -> JSObjectRef {
        self.install_with_statics(global_object, intl, |_| {}, members)
    }

    /// `install_with`, com `statics` rodando no construtor logo depois de `supportedLocalesOf` e antes de
    /// `length`/`name`/`prototype` (a tabela estática do C++ enumera antes das propriedades reificadas, como
    /// `WebAssembly.Module`: `customSections`, `imports`, `exports`).
    pub fn install_with_statics(
        &self,
        global_object: &JSGlobalObject,
        intl: &JSObject,
        statics: impl FnOnce(&JSObject),
        members: impl FnOnce(&JSObject),
    ) -> JSObjectRef {
        self.install_with_prototype_info(global_object, intl, None, &INTL_CONSTRUCTOR_S_INFO, statics, members)
    }

    /// `install_with`, com o protótipo de `ClassInfo` próprio e tabela estática (`intl*PrototypeTable`): o
    /// `Structure` do protótipo leva `HasStaticPropertyTable` e as entradas da tabela reificam no primeiro acesso,
    /// depois do `@@toStringTag` eager e antes do `constructor` na ordem de `ownKeys` do bun.
    pub fn install_with_table(&self, global_object: &JSGlobalObject, intl: &JSObject, prototype_info: &'static ClassInfo) -> JSObjectRef {
        self.install_with_prototype_info(global_object, intl, Some(prototype_info), &INTL_CONSTRUCTOR_S_INFO, |_| {}, |_| {})
    }

    /// `install_with_statics` com o `ClassInfo` do construtor próprio e tabela estática (`WebAssemblyModuleConstructor`).
    pub fn install_with_constructor_info(
        &self,
        global_object: &JSGlobalObject,
        intl: &JSObject,
        info: &'static ClassInfo,
        members: impl FnOnce(&JSObject),
    ) -> JSObjectRef {
        self.install_with_prototype_info(global_object, intl, None, info, |_| {}, members)
    }

    fn install_with_prototype_info(
        &self,
        global_object: &JSGlobalObject,
        intl: &JSObject,
        prototype_info: Option<&'static ClassInfo>,
        info: &'static ClassInfo,
        statics: impl FnOnce(&JSObject),
        members: impl FnOnce(&JSObject),
    ) -> JSObjectRef {
        let vm = global_object.vm();
        let object_prototype = global_object.object_prototype();
        let prototype_structure = match prototype_info {
            None => JSObject::create_structure(vm, Some(global_object), object_prototype.as_value()),
            Some(info) => Structure::create(
                vm,
                Some(global_object),
                object_prototype.as_value(),
                TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE),
                info,
            ),
        };
        let prototype = JSObject::allocate(vm, &prototype_structure);
        prototype.finish_creation(vm);
        put_to_string_tag(vm, &prototype, &format!("Intl.{}", self.name));

        let function_prototype = global_object.function_prototype();
        let constructor_structure =
            collection_constructor_structure(vm, global_object, function_prototype.as_value(), info);
        let has_supported_locales_of = self.has_supported_locales_of;
        let constructor = create_collection_constructor_with(
            vm,
            global_object,
            constructor_structure,
            &prototype,
            self.name,
            self.length,
            self.call,
            self.construct,
            false,
            |constructor| {
                if has_supported_locales_of {
                    put_method_on(global_object, constructor, "supportedLocalesOf", 1, supported_locales_of);
                }
                statics(constructor);
            },
        );
        members(&prototype);
        prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
        intl.put_direct(vm, &prop(vm, self.name), constructor.as_value(), DONT_ENUM);
        prototype
    }
}

/// Define o `ClassInfo` de um protótipo `Intl` com tabela estática (`IntlXPrototype::s_info`, pai
/// `JSNonFinalObject`): `intl_prototype_s_info!(PLURAL_RULES_PROTOTYPE_S_INFO, "Intl.PluralRules", [entradas...])`.
/// As entradas seguem a ordem do `@begin` (`native_entry`, `custom_getter_entry`, de `lookup`).
#[macro_export]
macro_rules! intl_prototype_s_info {
    ($info:ident, $class_name:literal, [$($entry:expr),+ $(,)?]) => {
        static $info: $crate::runtime::class_info::ClassInfo = $crate::runtime::class_info::ClassInfo {
            class_name: $class_name,
            parent_class: Some(&$crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO),
            static_prop_hash_table: Some(&$crate::runtime::lookup::HashTable {
                class_for_this: None,
                values: &[$($entry),+],
            }),
            inherits_js_type_range: None,
        };
    };
}

/// As cinco entradas que `numberFormatPrototypeTable` e `dateTimeFormatPrototypeTable` têm em comum, na ordem do
/// `@begin`: `format`, `formatRange` (2), `formatRangeToParts` (2), `formatToParts` (1), `resolvedOptions` (0).
pub const fn intl_format_prototype_values(
    format_getter: GetValueFunc,
    format_range: NativeFunction,
    format_range_to_parts: NativeFunction,
    format_to_parts: NativeFunction,
    resolved_options: NativeFunction,
) -> [HashTableValue; 5] {
    [
        custom_getter_entry("format", format_getter),
        native_entry("formatRange", format_range, 2),
        native_entry("formatRangeToParts", format_range_to_parts, 2),
        native_entry("formatToParts", format_to_parts, 1),
        native_entry("resolvedOptions", resolved_options, 0),
    ]
}

/// `JSC_NATIVE_FUNCTION_WITHOUT_TRANSITION(name, function, DontEnum, length, Public)`.
pub fn put_method_on(global_object: &JSGlobalObject, object: &JSObject, name: &str, length: u32, function: NativeFunction) {
    let vm = global_object.vm();
    put_native_function(vm, global_object, object, &Identifier::from_span(vm, name.as_bytes()), length, function, Intrinsic::NoIntrinsic);
}

/// O acessor `name` (`CustomAccessor` no C++): `DontEnum|ReadOnly`, com o `JSFunction` `"get name"`.
pub fn put_getter_on(global_object: &JSGlobalObject, object: &JSObject, name: &str, getter: NativeFunction) {
    let vm = global_object.vm();
    let function = crate::runtime::js_custom_accessor_function::create_host_custom_accessor_getter_function(vm, global_object, name, getter);
    let accessor = GetterSetter::create_from_values(vm, function.as_value(), JSValue::undefined());
    object.put_direct_non_index_accessor_without_transition(vm, &prop(vm, name), &accessor, DONT_ENUM | ACCESSOR);
}

/// A função `format` ou `compare` ligada à instância (`JSBoundFunction::create(vm, globalObject, target,
/// instance, nullptr, length, jsEmptyString(vm))`), criada uma vez e devolvida sempre a mesma.
pub fn bound_function(
    global_object: &JSGlobalObject,
    instance: &IntlInstance,
    target: NativeFunction,
    name: &str,
    length: u32,
) -> HostResult {
    let cached = instance.bound.get();
    if !cached.is_empty() {
        return Ok(cached);
    }
    let vm = global_object.vm();
    let target_function = JSFunction::create_native(
        vm,
        global_object,
        length,
        &WtfString::from_utf8(name.as_bytes()),
        target,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    let bound = JSBoundFunction::create(
        vm,
        global_object,
        target_function.as_value(),
        instance.as_value(),
        &[],
        f64::from(length),
        Some(js_empty_string(vm)),
    )
    .ok_or(Thrown::OutOfMemory)?;
    // Medido no bun: a propriedade `name` da função ligada é a string vazia (sem o prefixo `bound `).
    bound.ensure_rare_data(vm).set_has_reified_name();
    bound.put_direct(vm, &prop(vm, "name"), JSValue::from_cell(js_empty_string(vm).cell_id()), DONT_ENUM | READ_ONLY);
    instance.bound.set(bound.as_value());
    Ok(bound.as_value())
}

/// `jsNumber(n)` de um inteiro sem sinal, para o `resolvedOptions`.
pub fn number_value(n: u32) -> JSValue {
    js_number(f64::from(n))
}
