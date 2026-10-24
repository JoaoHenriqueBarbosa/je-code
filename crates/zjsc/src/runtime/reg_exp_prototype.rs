//! Porte de `runtime/RegExpPrototype.{h,cpp}`: o `RegExp.prototype` e os corpos de `exec`, `test`,
//! `toString` e dos acessores `flags`, `source`, `global`, `hasIndices`, `ignoreCase`, `multiline`,
//! `dotAll`, `sticky`, `unicode` e `unicodeSets`.
//!
//! ESQUELETO, e o que falta:
//!
//! - Os corpos são funções Rust `(global_object, this_value, args) -> Result<JSValue, PutError>`.
//!   O `finishCreation` do C++ os registra como `NativeFunction` (`putDirectNativeFunctionWithoutTransition`
//!   e `putDirectNativeGetter`); esse registro espera a assinatura final de `native_function.rs`, que
//!   está sendo redesenhada, e por isso `RegExpPrototype::finish_creation` só faz o `finishCreation` da
//!   base. Quando a assinatura sair, cada corpo vira o adaptador de uma linha que lê `thisValue` e
//!   `argument(n)` do `CallFrame`.
//! - `compile` e os `Symbol.*` (`@@match`, `@@matchAll`, `@@replace`, `@@search`, `@@split`, `test`) estão
//!   em `reg_exp_prototype_natives.rs`, com o `regExpExec` genérico.
//! - `thisValue == globalObject->regExpPrototype()` (o getter devolve `undefined` no protótipo) espera
//!   o `regExpPrototype()` do `JSGlobalObject`: sem ele, `this` que não é `RegExpObject` lança o
//!   `TypeError`, inclusive o próprio protótipo.
//! - `toString` sobre `RegExpObject` vai direto ao caminho rápido do C++ (o do `regExpFlagsWatchpointIsValid`)
//!   só no sentido de ler `source` e `flags` com `get`: o `flags` é sempre o `flagsString` genérico, que
//!   lê as oito propriedades (o atalho `Yarr::flagsString(regExp->flags())` do C++ só vale sem
//!   watchpoints invalidados, e aqui não há como saber).
//! - `exec` converte o argumento em `reg_exp_prototype_natives.rs` (`toStringOrNull`: com exceção
//!   pendente devolve `undefined` e não roda o casamento).

use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::identifier::Identifier;
use crate::runtime::string_regexp_support::{get_object_property, to_wtf_string_value};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, PutError, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::reg_exp_object::RegExpObject;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::string_builder::StringBuilder;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::yarr::yarr_flags::{flags_string, FlagSet, Flags};

/// `const ClassInfo RegExpPrototype::s_info` (`"Object"`, base `JSNonFinalObject`).
pub static REG_EXP_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class RegExpPrototype : public JSNonFinalObject`.
pub struct RegExpPrototype;

impl RegExpPrototype {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
            &REG_EXP_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: o construtor e o `finishCreation` (a tabela de funções está
    /// em `reg_exp_prototype_natives.rs`).
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        crate::runtime::reg_exp_prototype_natives::add_reg_exp_prototype_properties(&prototype, vm, global_object);
        prototype
    }
}

/// `RegExpObject` por trás do `thisValue` (`dynamicDowncast<RegExpObject>`).
fn as_reg_exp_object(this_value: JSValue) -> Option<std::rc::Rc<RegExpObject>> {
    match this_value {
        JSValue::Cell(cell_id) => RegExpObject::from_cell_id(cell_id),
        _ => None,
    }
}

pub const BUILTIN_EXEC_NOT_REG_EXP: &str = "Builtin RegExp exec can only be called on a RegExp object";

/// `escapePattern` (RegExp.cpp), a parte de `RegExp::escapedPattern`: `(?:)` para o padrão vazio, `/`
/// fora de colchetes e terminadores de linha escapados.
pub fn escaped_pattern(pattern: &WtfString) -> WtfString {
    if pattern.length() == 0 {
        return WtfString::from_latin1(b"(?:)");
    }
    let is_line_terminator = |ch: u16| matches!(ch, 0x0A | 0x0D | 0x2028 | 0x2029);
    let mut previous_was_backslash = false;
    let mut in_brackets = false;
    let mut result = StringBuilder::new();
    let mut escaped_any = false;
    for i in 0..pattern.length() {
        let ch = pattern.code_unit_at(i);
        if !previous_was_backslash {
            if in_brackets {
                if ch == u16::from(b']') {
                    in_brackets = false;
                }
            } else if ch == u16::from(b'/') {
                result.append_latin1_character(b'\\');
                escaped_any = true;
            } else if ch == u16::from(b'[') {
                in_brackets = true;
            }
        }
        if is_line_terminator(ch) {
            escaped_any = true;
            if !previous_was_backslash {
                result.append_latin1_character(b'\\');
            }
            match ch {
                0x0A => result.append_latin1_character(b'n'),
                0x0D => result.append_latin1_character(b'r'),
                0x2028 => result.append_latin1(b"u2028"),
                _ => result.append_latin1(b"u2029"),
            }
        } else {
            result.append_character(ch);
        }
        previous_was_backslash = !previous_was_backslash && ch == u16::from(b'\\');
    }
    if !escaped_any {
        return pattern.clone();
    }
    result.to_string().clone()
}

/// `regExpProtoGetterSource`.
pub fn reg_exp_proto_getter_source(global_object: &JSGlobalObject, this_value: JSValue) -> Result<JSValue, PutError> {
    let Some(reg_exp) = as_reg_exp_object(this_value) else {
        return Err(PutError::TypeError("The RegExp.prototype.source getter can only be called on a RegExp object"));
    };
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &escaped_pattern(reg_exp.reg_exp().pattern()))))
}

/// `flagsString(globalObject, regexp)` (`RegExpPrototype.cpp`): lê as oito propriedades booleanas do
/// objeto, na ordem de `JSC_REGEXP_FLAGS`; `Err(Pending)` se um getter lança.
fn generic_flags_string(global_object: &JSGlobalObject, object: JSValue) -> Result<FlagSet, Thrown> {
    let property_names = &global_object.vm().property_names;
    let readers: [(&Identifier, Flags); 8] = [
        (&property_names.has_indices, Flags::HasIndices),
        (&property_names.global, Flags::Global),
        (&property_names.ignore_case, Flags::IgnoreCase),
        (&property_names.multiline, Flags::Multiline),
        (&property_names.dot_all, Flags::DotAll),
        (&property_names.unicode, Flags::Unicode),
        (&property_names.unicode_sets, Flags::UnicodeSets),
        (&property_names.sticky, Flags::Sticky),
    ];
    let mut flags = FlagSet::default();
    for (name, flag) in readers {
        if get_object_property(global_object, object, name)?.to_boolean() {
            flags.add(flag);
        }
    }
    Ok(flags)
}

/// `regExpProtoGetterFlags`: sempre o `flagsString` genérico. O C++ tem um atalho para o `RegExpObject`
/// primordial (`regExpFlagsWatchpointIsValid`, `Yarr::flagsString(regExp->flags())`), que só vale
/// enquanto ninguém redefiniu os acessores do protótipo nem pôs propriedade própria; sem os watchpoints
/// o caminho genérico é o único que mantém o resultado observável igual nos dois casos.
pub fn reg_exp_proto_getter_flags(global_object: &JSGlobalObject, this_value: JSValue) -> Result<JSValue, Thrown> {
    if JSObject::from_value(&this_value).is_none() {
        return Err(Thrown::type_error("The RegExp.prototype.flags getter can only be called on an object"));
    }
    let flags = flags_string(generic_flags_string(global_object, this_value)?);
    let length = flags.iter().position(|&byte| byte == 0).unwrap_or(flags.len());
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(&flags[..length]))))
}

/// Os oito acessores booleanos (`global`, `hasIndices`, ...): a flag que cada um lê e a mensagem de
/// quando `this` não é `RegExpObject`.
pub const FLAG_GETTERS: [(&str, Flags, &str); 8] = [
    ("global", Flags::Global, "The RegExp.prototype.global getter can only be called on a RegExp object"),
    ("hasIndices", Flags::HasIndices, "The RegExp.prototype.hasIndices getter can only be called on a RegExp object"),
    ("ignoreCase", Flags::IgnoreCase, "The RegExp.prototype.ignoreCase getter can only be called on a RegExp object"),
    ("multiline", Flags::Multiline, "The RegExp.prototype.multiline getter can only be called on a RegExp object"),
    ("dotAll", Flags::DotAll, "The RegExp.prototype.dotAll getter can only be called on a RegExp object"),
    ("sticky", Flags::Sticky, "The RegExp.prototype.sticky getter can only be called on a RegExp object"),
    ("unicode", Flags::Unicode, "The RegExp.prototype.unicode getter can only be called on a RegExp object"),
    ("unicodeSets", Flags::UnicodeSets, "The RegExp.prototype.unicodeSets getter can only be called on a RegExp object"),
];

/// `regExpProtoGetterGlobal`, `...HasIndices`, `...IgnoreCase`, `...Multiline`, `...DotAll`,
/// `...Sticky`, `...Unicode` e `...UnicodeSets`: `name` é uma das chaves de `FLAG_GETTERS`.
pub fn reg_exp_proto_getter_flag(name: &str, this_value: JSValue) -> Result<JSValue, PutError> {
    let (_, flag, message) = FLAG_GETTERS.iter().find(|(getter, _, _)| *getter == name).expect("acessor de flag desconhecido");
    let Some(reg_exp) = as_reg_exp_object(this_value) else {
        return Err(PutError::TypeError(message));
    };
    Ok(js_boolean(reg_exp.reg_exp().flags().contains(*flag)))
}

/// `regExpProtoFuncToString`: `'/' + source + '/' + flags`, lendo `source` e `flags` do objeto com `get`
/// (um `this` que não é `RegExpObject` e tem as próprias propriedades também serve).
pub fn reg_exp_proto_func_to_string(global_object: &JSGlobalObject, this_value: JSValue) -> Result<JSValue, Thrown> {
    if JSObject::from_value(&this_value).is_none() {
        return Err(Thrown::type_error("Type error"));
    }
    let vm = global_object.vm();
    let source_value = get_object_property(global_object, this_value, &vm.property_names.source)?;
    let source = to_wtf_string_value(global_object, source_value)?;
    let flags_value = get_object_property(global_object, this_value, &vm.property_names.flags)?;
    let flags = to_wtf_string_value(global_object, flags_value)?;
    let mut builder = StringBuilder::new();
    builder.append_latin1_character(b'/');
    builder.append_string(&source);
    builder.append_latin1_character(b'/');
    builder.append_string(&flags);
    Ok(JSValue::from_js_string(js_string(vm, builder.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::string_prototype::{code_units, string_from_units};

    fn escaped(pattern: &str) -> std::string::String {
        let units: Vec<u16> = pattern.encode_utf16().collect();
        std::string::String::from_utf16_lossy(&code_units(&escaped_pattern(&string_from_units(&units))))
    }

    #[test]
    fn empty_pattern_is_a_non_capturing_group() {
        assert_eq!(escaped(""), "(?:)");
    }

    #[test]
    fn slash_is_escaped_outside_brackets_only() {
        assert_eq!(escaped("a/b"), "a\\/b");
        assert_eq!(escaped("[/]"), "[/]");
        assert_eq!(escaped("[a]/"), "[a]\\/");
        // Uma barra já escapada fica como está.
        assert_eq!(escaped("a\\/b"), "a\\/b");
        // O `\\` do par não escapa o `/` seguinte.
        assert_eq!(escaped("\\\\/"), "\\\\\\/");
    }

    #[test]
    fn line_terminators_become_escapes() {
        assert_eq!(escaped("\n"), "\\n");
        assert_eq!(escaped("\r"), "\\r");
        assert_eq!(escaped("\u{2028}"), "\\u2028");
        assert_eq!(escaped("\u{2029}"), "\\u2029");
        // Com a barra invertida antes, o escape não ganha outra.
        assert_eq!(escaped("\\\n"), "\\n");
    }

    #[test]
    fn plain_patterns_are_returned_untouched() {
        assert_eq!(escaped("a(b|c)*"), "a(b|c)*");
        assert_eq!(escaped("[\\]/]"), "[\\]/]");
    }
}
