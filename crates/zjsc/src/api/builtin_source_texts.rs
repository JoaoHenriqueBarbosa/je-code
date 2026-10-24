//! O texto que o bun 1.4.2 mostra no `Function.prototype.toString` das funções de JavaScript dos módulos embutidos.
//!
//! No bun cada módulo embutido é o fonte empacotado (`bundle-modules.ts`: sintaxe minificada com `!0`, nomes com sufixo
//! do bundler, `@intrínsecos`), e `String(fn)` mostra o recorte dele. O fonte embutido aqui é outro (escrito à mão ou
//! gerado dos `.ts`, com os intrínsecos trocados), então o recorte sairia diferente. A tabela
//! `builtin_source_texts.txt` (gerada no bun do oráculo por `scripts/gen-builtin-source-texts.js`) guarda o texto do
//! bun de cada função alcançável a partir das exportações do módulo, pelo caminho que leva a ela; ao instalar o módulo
//! o texto é posto no executável da função (o `m_asString` do `FunctionExecutable`), que o `toString` devolve em vez
//! do recorte.
//!
//! Divergência: só as funções alcançáveis pelas exportações (propriedades, `prototype` e acessores, até a profundidade
//! 3) levam o texto do bun. Uma função interna que nenhum caminho público alcança, vista por um agente só por meio de
//! um objeto que ela devolve, ainda mostra o texto do nosso fonte.

use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::wtf::text::wtf_string::String as WtfString;

const TABLE: &str = include_str!("builtin_source_texts.txt");

/// Separa os segmentos de um caminho na tabela.
const SEGMENT_SEPARATOR: char = '\u{1f}';

/// Como um segmento lê a propriedade: o valor de dados, o getter ou o setter.
enum Slot {
    Value,
    Getter,
    Setter,
}

/// Desfaz o escape do texto da tabela (`\\`, `\n`, `\r` e `\t`).
fn unescape(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        match chars.next() {
            Some('n') => result.push('\n'),
            Some('r') => result.push('\r'),
            Some('t') => result.push('\t'),
            Some(other) => result.push(other),
            None => result.push('\\'),
        }
    }
    result
}

/// As linhas `caminho<TAB>texto` da seção de `module`.
fn section(module: &str) -> impl Iterator<Item = (&'static str, &'static str)> {
    let header = format!("@@ {module}");
    let mut lines = TABLE.lines();
    for line in lines.by_ref() {
        if line == header {
            break;
        }
    }
    lines.take_while(|line| !line.starts_with("@@ ")).filter_map(|line| line.split_once('\t'))
}

/// A propriedade que `segment` nomeia em `object`, sem chamar acessor.
fn step(global_object: &JSGlobalObject, object: JSValue, segment: &str) -> Option<JSValue> {
    let (name, slot) = if let Some(name) = segment.strip_prefix("<get ").and_then(|rest| rest.strip_suffix('>')) {
        (name, Slot::Getter)
    } else if let Some(name) = segment.strip_prefix("<set ").and_then(|rest| rest.strip_suffix('>')) {
        (name, Slot::Setter)
    } else {
        (segment, Slot::Value)
    };
    let vm = global_object.vm();
    let holder = ObjectRef::from_value(&object)?;
    let mut descriptor = PropertyDescriptor::default();
    if !holder.get_own_property_descriptor(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes())), &mut descriptor) {
        return None;
    }
    match slot {
        Slot::Value if !descriptor.is_accessor_descriptor() => Some(descriptor.value()),
        Slot::Getter if descriptor.is_accessor_descriptor() => Some(descriptor.getter()),
        Slot::Setter if descriptor.is_accessor_descriptor() => Some(descriptor.setter()),
        _ => None,
    }
}

/// Põe o texto do bun nas funções de `exports`, o módulo `module` recém-instalado. Módulo sem seção na tabela não muda.
pub(crate) fn apply(global_object: &JSGlobalObject, module: &str, exports: JSValue) {
    let vm = global_object.vm();
    for (path, text) in section(module) {
        let mut current = Some(exports);
        if !path.is_empty() {
            for segment in path.split(SEGMENT_SEPARATOR) {
                current = current.and_then(|value| step(global_object, value, segment));
            }
        }
        let Some(function) = current.and_then(|value| value.as_js_function()).filter(|function| !function.is_host_function()) else { continue };
        let text = js_string(vm, &WtfString::from_utf8(unescape(text).as_bytes()));
        crate::runtime::function_executable::FunctionExecutable::override_as_string(&function.js_executable(), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_undoes_the_table_escapes() {
        assert_eq!(unescape("a\\nb\\r\\tc\\\\d"), "a\nb\r\tc\\d");
    }

    /// O `string_decoder` do bun é nativo (`[native code]`), mas a seção dele existe: a nossa é de JavaScript.
    #[test]
    fn every_module_section_has_entries_with_a_tab() {
        for module in ["url", "events", "stream", "querystring", "timers", "timers/promises", "string_decoder", "async_hooks", "util"] {
            assert!(section(module).next().is_some(), "a seção de {module} está vazia");
        }
        assert!(section("nao_existe").next().is_none());
    }
}
