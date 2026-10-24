//! Tradução parcial de `runtime/Lookup.h`: `struct HashTableValue` e `struct HashTable`.
//!
//! DIVERGÊNCIAS:
//!
//! - O C++ guarda as entradas num array gerado pelo `create_hash_table` e o índice de hash compacto
//!   (`CompactHashIndex`, `indexMask`, `seenPropertyAttributes`) ao lado. As tabelas são pequenas, então
//!   `entry` percorre `values` linearmente por nome. A ordem de `values` é a ordem do fonte do `@begin`
//!   (a que `getNonReifiedStaticPropertyNames` percorre), não a ordem de hash.
//! - A `union ValueStorage` com as etiquetas (`AccessorTypeTag`...) é o enum [`Kind`]; o `ValueType`
//!   de depuração do C++ vira o próprio discriminante.
//! - Fora do porte por ora: `DOMJITAttribute`, `DOMJITFunction`, `GetterSetter`, `LazyCellProperty`,
//!   `LazyClassStructure` e `Lexer`, que nenhuma tabela portada usa.

use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, create_builtin_function, JSFunction};
use crate::runtime::js_object::JSObject;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::is_valid_offset;
use crate::runtime::property_slot::PropertySlot;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{
    attributes_for_structure, ACCESSOR, BUILTIN, CONSTANT_INTEGER, CUSTOM_ACCESSOR, DONT_ENUM, FUNCTION, PROPERTY_CALLBACK,
    READ_ONLY,
};
use crate::runtime::custom_getter_setter::CustomGetterSetter;
use crate::runtime::property_slot::{GetValueFunc, PutValueFunc};
use crate::runtime::vm::VM;

/// `LazyPropertyCallback`: `JSValue(VM&, JSObject*)`.
pub type LazyPropertyCallback = fn(&VM, &JSObject) -> JSValue;

/// `BuiltinGenerator` da tabela: o porte identifica o builtin pelo `BuiltinCodeIndex`.
pub type BuiltinGenerator = BuiltinCodeIndex;

/// A `union ValueStorage` de `HashTableValue`, uma variante por etiqueta.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// `NativeFunctionType`: `PropertyAttribute::Function`; `length` é o `argCount`.
    NativeFunction { function: NativeFunction, length: i32 },
    /// `BuiltinGeneratorType`: `PropertyAttribute::Builtin`; `length` é o `argCount`.
    BuiltinGenerator { generator: BuiltinGenerator, length: i32 },
    /// `AccessorType`: `PropertyAttribute::Accessor`; `None` é o `nullptr` (por exemplo, sem setter).
    Accessor { getter: Option<NativeFunction>, setter: Option<NativeFunction> },
    /// `BuiltinAccessorType`.
    BuiltinAccessor { getter: Option<BuiltinGenerator>, setter: Option<BuiltinGenerator> },
    /// `ConstantType`: `PropertyAttribute::ConstantInteger`.
    Constant(i64),
    /// `LazyPropertyType`: `PropertyAttribute::PropertyCallback`.
    LazyProperty(LazyPropertyCallback),
    /// `PropertyAttribute::CustomAccessor` (o `propertyGetter()`/`propertyPutter()` de `HashTableValue`); `None` é o
    /// `nullptr` do setter.
    CustomAccessor { getter: GetValueFunc, setter: Option<PutValueFunc> },
}

/// `struct HashTableValue`.
#[derive(Clone, Copy, Debug)]
pub struct HashTableValue {
    /// `m_key`: o nome da propriedade.
    pub key: &'static str,
    /// `m_attributes`: bits de `PropertyAttribute`.
    pub attributes: u32,
    /// `m_intrinsic`.
    pub intrinsic: Intrinsic,
    /// `m_values`.
    pub kind: Kind,
}

impl HashTableValue {
    /// `propertyGetter()`/`propertyPutter()` e as demais leituras do C++ assumem o atributo certo para a
    /// variante; este confere a coerência que o `ASSERT(m_attributes & ...)` confere.
    pub fn attributes_match_kind(&self) -> bool {
        let required = match self.kind {
            Kind::NativeFunction { .. } => FUNCTION,
            Kind::BuiltinGenerator { .. } => BUILTIN,
            Kind::Accessor { .. } => ACCESSOR,
            Kind::BuiltinAccessor { .. } => BUILTIN | ACCESSOR,
            Kind::Constant(_) => CONSTANT_INTEGER,
            Kind::LazyProperty(_) => PROPERTY_CALLBACK,
            Kind::CustomAccessor { .. } => CUSTOM_ACCESSOR,
        };
        self.attributes & required == required
    }
}

/// Entrada `NativeFunction` com os atributos de tabela (`attributes`, sem o bit `Function`, que entra aqui),
/// o `argCount` e o `Intrinsic` dados. É a forma geral de que as demais `*_entry` de função são casos.
pub const fn native_function_entry(
    key: &'static str,
    attributes: u32,
    function: NativeFunction,
    length: i32,
    intrinsic: Intrinsic,
) -> HashTableValue {
    HashTableValue { key, attributes: attributes | FUNCTION, intrinsic, kind: Kind::NativeFunction { function, length } }
}

/// Entrada `DontEnum|Function` sem intrinsic: a linha `fn  JSFunction  DontEnum|Function  N` do `@begin`.
pub const fn native_entry(key: &'static str, function: NativeFunction, length: i32) -> HashTableValue {
    native_function_entry(key, DONT_ENUM, function, length, Intrinsic::NoIntrinsic)
}

/// Entrada `DontEnum|Function` com `Intrinsic`.
pub const fn native_entry_with_intrinsic(key: &'static str, function: NativeFunction, length: i32, intrinsic: Intrinsic) -> HashTableValue {
    native_function_entry(key, DONT_ENUM, function, length, intrinsic)
}

/// Entrada `DontEnum|Builtin` (`JSBuiltin`) com o `argCount` dado.
pub const fn builtin_entry(key: &'static str, generator: BuiltinGenerator, length: i32) -> HashTableValue {
    HashTableValue {
        key,
        attributes: DONT_ENUM | BUILTIN,
        intrinsic: Intrinsic::NoIntrinsic,
        kind: Kind::BuiltinGenerator { generator, length },
    }
}

/// Entrada `DontEnum|ReadOnly|CustomAccessor` sem setter (um getter nativo `GetValueFunc`).
pub const fn custom_getter_entry(key: &'static str, getter: GetValueFunc) -> HashTableValue {
    HashTableValue {
        key,
        attributes: DONT_ENUM | READ_ONLY | CUSTOM_ACCESSOR,
        intrinsic: Intrinsic::NoIntrinsic,
        kind: Kind::CustomAccessor { getter, setter: None },
    }
}

/// Entrada `DontEnum|CustomAccessor` com getter e setter.
pub const fn custom_accessor_entry(key: &'static str, getter: GetValueFunc, setter: PutValueFunc) -> HashTableValue {
    custom_accessor_entry_with_attributes(key, DONT_ENUM, getter, setter)
}

/// Entrada `CustomAccessor` com getter, setter e os atributos de tabela dados (`main  bunObjectMain
/// DontDelete|CustomAccessor` do objeto `Bun`).
pub const fn custom_accessor_entry_with_attributes(key: &'static str, attributes: u32, getter: GetValueFunc, setter: PutValueFunc) -> HashTableValue {
    HashTableValue {
        key,
        attributes: attributes | CUSTOM_ACCESSOR,
        intrinsic: Intrinsic::NoIntrinsic,
        kind: Kind::CustomAccessor { getter, setter: Some(setter) },
    }
}

/// Entrada `DontEnum|PropertyCallback` (`LazyProperty`).
pub const fn lazy_entry(key: &'static str, callback: LazyPropertyCallback) -> HashTableValue {
    lazy_entry_with_attributes(key, DONT_ENUM, callback)
}

/// Entrada `PropertyCallback` (`LazyProperty`) com os atributos de tabela dados (`argv  ...  DontDelete|PropertyCallback`).
pub const fn lazy_entry_with_attributes(key: &'static str, attributes: u32, callback: LazyPropertyCallback) -> HashTableValue {
    HashTableValue { key, attributes: attributes | PROPERTY_CALLBACK, intrinsic: Intrinsic::NoIntrinsic, kind: Kind::LazyProperty(callback) }
}

/// `struct HashTable`.
#[derive(Debug)]
pub struct HashTable {
    /// `classForThis`: usado pelos acessores de atributo para a checagem de tipo.
    pub class_for_this: Option<&'static ClassInfo>,
    /// `values`, na ordem do `@begin` do `.lut.h`.
    pub values: &'static [HashTableValue],
}

impl HashTable {
    /// `numberOfValues`.
    pub fn number_of_values(&self) -> usize {
        self.values.len()
    }

    /// `entry(PropertyName)`: o nome de símbolo nunca casa, então o chamador passa só o nome textual.
    pub fn entry(&self, name: &str) -> Option<&'static HashTableValue> {
        self.values.iter().find(|value| value.key == name)
    }

    /// `begin()`/`end()`: as entradas na ordem do fonte.
    pub fn iter(&self) -> std::slice::Iter<'static, HashTableValue> {
        self.values.iter()
    }
}

/// `reifyStaticProperty(vm, classInfo, propertyName, value, thisObj)`, as etiquetas `NativeFunction`,
/// `BuiltinGenerator`, `CustomAccessor` e `LazyProperty`: grava em `this_object` (um `putDirect`, com transição)
/// o valor da entrada com os atributos da tabela sem os bits só de tabela. As demais etiquetas (`Accessor`,
/// `BuiltinAccessor`, `Constant`) ainda não são reificadas: devolvem `false` sem tocar no objeto. Devolve se gravou.
pub fn reify_static_property(vm: &VM, this_object: &JSObject, entry: &HashTableValue) -> bool {
    let Some(global_object) = this_object.structure().realm() else {
        return false;
    };
    let attributes = attributes_for_structure(entry.attributes);
    let name = Identifier::from_span(vm, entry.key.as_bytes());
    let property_name = PropertyName::from_identifier(&name);
    match entry.kind {
        Kind::BuiltinGenerator { generator, .. } => {
            let function = create_builtin_function(vm, &global_object, generator);
            this_object.put_direct(vm, &property_name, function.as_value(), attributes);
            true
        }
        Kind::NativeFunction { function, length } => {
            let function = JSFunction::create_native(
                vm,
                &global_object,
                length as u32,
                name.string().string(),
                function,
                ImplementationVisibility::Public,
                entry.intrinsic,
                call_host_function_as_constructor,
            );
            this_object.put_direct(vm, &property_name, function.as_value(), attributes);
            true
        }
        Kind::CustomAccessor { getter, setter } => {
            let custom = CustomGetterSetter::create(vm, getter, setter);
            this_object.put_direct_custom_accessor(vm, &property_name, &custom, attributes);
            true
        }
        Kind::LazyProperty(callback) => {
            // `PropertyCallback`: o valor nasce na primeira leitura e entra com os atributos da tabela.
            // O callback pode já ter gravado o nome (o `install_*` do `Intl` faz `putDirect`); gravar de novo
            // o mesmo valor com os mesmos atributos só substitui no lugar.
            let value = callback(vm, this_object);
            this_object.put_direct(vm, &property_name, value, attributes);
            true
        }
        Kind::Accessor { .. } | Kind::BuiltinAccessor { .. } | Kind::Constant(_) => false,
    }
}

/// `getStaticPropertySlotFromTable(vm, classInfo, table, thisObj, propertyName, slot)` +
/// `setUpStaticFunctionSlot`: se `table` tem o nome e a `Structure` ainda não o tem, reifica a entrada;
/// depois preenche `slot` a partir da `Structure`. Devolve `false` quando as propriedades estáticas já estão
/// reificadas (`staticPropertiesReified`), quando o nome não está na tabela, ou quando a etiqueta ainda não
/// é reificável. Ainda sem chamador: o gancho em `JSObject::get_own_property_slot` é a fatia seguinte.
pub fn get_static_property_slot_from_table(
    vm: &VM,
    table: &HashTable,
    this_object: &JSObject,
    property_name: &PropertyName,
    slot: &mut PropertySlot,
) -> bool {
    if this_object.structure().static_properties_reified() {
        return false;
    }
    let Some(uid) = property_name.public_name() else {
        return false;
    };
    let key = Identifier::from_uid(vm, Some(uid)).utf8();
    let Some(entry) = std::str::from_utf8(&key).ok().and_then(|key| table.entry(key)) else {
        return false;
    };
    let structure = this_object.structure();
    // `setCacheableCustom` direto da tabela: o acessor customizado só entra na `Structure` quando alguém o
    // define ou reifica tudo; ler não reifica.
    if let Kind::CustomAccessor { getter, setter } = entry.kind {
        if !is_valid_offset(structure.get_with_attributes(vm, property_name).0) {
            slot.set_cacheable_custom(this_object, attributes_for_structure(entry.attributes), getter, setter);
            return true;
        }
    }
    if !is_valid_offset(structure.get_with_attributes(vm, property_name).0) && !reify_static_property(vm, this_object, entry) {
        return false;
    }
    let structure = this_object.structure();
    this_object.get_own_non_index_property_slot(vm, &structure, property_name, slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::property_attribute::DONT_ENUM;

    const NOT_A_FUNCTION_ENTRY: HashTableValue =
        HashTableValue { key: "answer", attributes: DONT_ENUM | CONSTANT_INTEGER, intrinsic: Intrinsic::NoIntrinsic, kind: Kind::Constant(42) };
    const WRONG_ENTRY: HashTableValue =
        HashTableValue { key: "bad", attributes: DONT_ENUM, intrinsic: Intrinsic::NoIntrinsic, kind: Kind::Constant(1) };
    static VALUES: [HashTableValue; 2] = [NOT_A_FUNCTION_ENTRY, WRONG_ENTRY];
    static TABLE: HashTable = HashTable { class_for_this: None, values: &VALUES };

    #[test]
    fn entry_finds_by_name_and_keeps_source_order() {
        assert_eq!(TABLE.number_of_values(), 2);
        assert!(matches!(TABLE.entry("answer").map(|value| value.kind), Some(Kind::Constant(42))));
        assert!(TABLE.entry("missing").is_none());
        let keys: Vec<&str> = TABLE.iter().map(|value| value.key).collect();
        assert_eq!(keys, ["answer", "bad"]);
    }

    #[test]
    fn attributes_must_carry_the_kind_bit() {
        assert!(VALUES[0].attributes_match_kind());
        assert!(!VALUES[1].attributes_match_kind());
    }
}
