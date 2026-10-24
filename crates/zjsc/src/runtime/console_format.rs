//! O formatador de objetos do `console.log` do sandbox: arrays e objetos simples impressos como o bun 1.4.2.
//!
//! A especificação executável é `scripts/console-object-model.js`; este módulo é o mesmo algoritmo sobre `JSValue`, e o
//! golden `console_object_bun.tsv` confere os dois contra o bun. A quebra de linha é a do fonte do bun, sem constante
//! ajustada: `Formatter::print_array`, `print_object`, `print_object_depth_exceeded`, `print_object_tail`,
//! `PropertyIteratorCtx::{handle_first_property, write_property_key}`, `WrappedWriter::{good_time_for_a_new_line,
//! print_comma, reset_line}` em `src/jsc/ConsoleObject.rs` (`estimated_line_length`, limite 80, `always_newline`). Resumo:
//! objeto com propriedades sempre em várias linhas, `[Object ...]` além de duas camadas, `[Circular]`, array em linha até
//! 10 elementos (`[ 1, 2 ]`), buracos como `empty item`, no máximo 100 elementos, e a quebra de linha governada por uma
//! estimativa de largura (`est`) que vale para a chamada inteira de `console.log` (por isso o [`Formatter`] vive em
//! `format_line` e é avisado do texto dos outros argumentos por [`Formatter::note`]).
//!
//! Também portados: função e classe (`print_function`, `print_class`: nome calculado e, para função, o `@@toStringTag`
//! do protótipo como `AsyncFunction`, `GeneratorFunction`, `AsyncGeneratorFunction`; sem propriedades próprias), `Map`
//! e `Set` (`print_map_like`, `print_set`, `WeakMap` e `WeakSet` saem vazios porque não têm `size`), os iteradores
//! (`print_map_iterator_like`, que não consome o iterador) e as caixas de primitivo (`print_double`, `print_boolean`,
//! `print_string`). Nenhum deles passa pela checagem de profundidade de objeto.
//!
//! Também: `Date` (`print_json`: o `JSON.stringify` sem aspas, `Invalid Date` para `null`), `RegExp` (`Tag::String`, o
//! `ToString` do objeto), `Promise` (`print_promise`), o nome de classe antes do `{` (`get_object_name`: `calculatedClassName`,
//! `@@toStringTag` string, `[Object: null prototype]`), `Symbol` e `BigInt` encaixotados, `WeakRef` e `FinalizationRegistry`
//! (as chaves do protótipo imediato, `forEachPropertyImpl`). `%s` com `String` encaixotado e `%o`/`%O` com objeto (também
//! função e classe) passam por aqui, chamados de `console_client.rs`.
//!
//! LACUNAS: `Error`, typed arrays, getters (`[Getter]`), `Proxy`, as propriedades enumeráveis herdadas do protótipo de um
//! objeto comum e as não enumeráveis próprias ainda saem como o bun não os imprime (como objeto comum). Aninhamento acima de
//! 512 níveis vira `[Array]` para não estourar a pilha do host. A estimativa de um `bigint` de nível superior conta o `n`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// As raízes do coletor: os objetos que os formatadores de fora estão imprimindo.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    ENCLOSING_SEEN.with(|seen| seen.borrow().iter().for_each(|cell_id| marker.cell(*cell_id)));
}

thread_local! {
    /// Os objetos que os formatadores de fora estão imprimindo quando um `Error` aninhado é relatado (ver
    /// `Formatter::inside_error_report`).
    static ENCLOSING_SEEN: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

use crate::host_function;
use crate::runtime::call_data::{call, get_call_data, get_construct_data, CallData};
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_boolean, js_number, js_undefined};
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::symbol::Symbol;
use crate::wtf::text::string_impl::StringImpl;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::runtime::body::Body;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::console_client::push_plain;
use crate::runtime::exception_helpers::calculated_class_name;
use crate::runtime::js_map::JSMapIterator;
use crate::runtime::js_ordered_hash_table::{iterator_step, IteratorStep};
use crate::runtime::js_set::JSSetIterator;
use crate::runtime::js_type::JSType;
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PropertyNameMode};
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::uncaught_report::append_error_report;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise::Status;
use crate::runtime::js_value::JSValue;
use crate::runtime::json_object::{json_stringify, throw_json_error};
use crate::runtime::literal_parser::{wtf_string_to_units, JsonHost, JsonKey};
use crate::runtime::object_constructor::own_property_keys;
use crate::runtime::property_name::PropertyName;
use crate::runtime::symbol::as_symbol;

const EMPTY_ARRAY: usize = 2;
const MAX_ITEMS: usize = 100;
const MAX_DEPTH: usize = 2;
const MAX_NESTING: usize = 512;
const LINE_LIMIT: usize = 80;

/// O valor de um campo de `Blob`, `File`, `Request` ou `Response` no relato do bun.
enum WebField {
    /// Texto entre aspas.
    Quoted(Vec<u16>),
    /// Número ou booleano, como está.
    Raw(String),
    /// O objeto `Headers`, formatado como valor aninhado.
    Headers(JSValue),
}

/// O tamanho no cabeçalho de `Blob (...)` (`bun.fmt.size`, medido no bun 1.4.2): a unidade sai de `log2(n) / 9` inteiro
/// (abaixo de 512 são bytes, a partir de 512 KB, de 2^18 MB, de 2^27 GB), a base é 1000, e o número leva uma casa quando
/// a parte fracionária (em ponto flutuante) é menor que 0,1, senão duas. Zero é `0 KB`.
fn format_byte_size(size: u64) -> String {
    if size == 0 {
        return "0 KB".to_string();
    }
    let magnitude = ((63 - size.leading_zeros()) / 9) as usize;
    if magnitude == 0 {
        return format!("{size} bytes");
    }
    let value = size as f64 / 1000f64.powi(magnitude as i32);
    let text = if value - value.floor() < 0.1 { format!("{:.1}", (value * 10.0).round() / 10.0) } else { format!("{:.2}", (value * 100.0).round() / 100.0) };
    format!("{text} {}B", b" KMGTPEZ"[magnitude.min(7)] as char)
}

fn ascii(out: &mut Vec<u16>, text: &str) {
    out.extend(text.bytes().map(u16::from));
}

fn has_exception(global_object: &JSGlobalObject) -> bool {
    global_object.vm().exception().is_some()
}

/// A unidade `index` é metade de um par substituto cuja outra metade não está ao lado.
fn is_lone_surrogate(text: &[u16], index: usize) -> bool {
    match text[index] {
        0xD800..=0xDBFF => !text.get(index + 1).is_some_and(|next| (0xDC00..=0xDFFF).contains(next)),
        0xDC00..=0xDFFF => !(index > 0 && (0xD800..=0xDBFF).contains(&text[index - 1])),
        _ => false,
    }
}

/// O comprimento (em unidades de 16 bits) de `JSON.stringify(texto)`: as aspas, `\"`, `\\` e os cinco controles curtos em
/// duas unidades, os demais controles e a metade de par substituto sem a outra em seis.
fn json_quoted_length(text: &[u16]) -> usize {
    let mut length = 2;
    for (index, &unit) in text.iter().enumerate() {
        length += match unit {
            0x22 | 0x5C | 0x08 | 0x09 | 0x0A | 0x0C | 0x0D => 2,
            control if control < 0x20 => 6,
            _ if is_lone_surrogate(text, index) => 6,
            _ => 1,
        };
    }
    length
}

/// `"texto"` com as escapadas do bun: `\n \t \r \b \f \" \\` e `\uXXXX` (maiúsculo) para os demais controles.
fn push_quoted(out: &mut Vec<u16>, text: &[u16]) {
    out.push(u16::from(b'"'));
    for (index, &unit) in text.iter().enumerate() {
        match unit {
            0x0A => ascii(out, "\\n"),
            0x09 => ascii(out, "\\t"),
            0x0D => ascii(out, "\\r"),
            0x08 => ascii(out, "\\b"),
            0x0C => ascii(out, "\\f"),
            0x22 => ascii(out, "\\\""),
            0x5C => ascii(out, "\\\\"),
            control if control < 0x20 => ascii(out, &format!("\\u{control:04X}")),
            // Metade de par substituto sem a outra: `\udXXX` em minúsculas (medido: `Bun.inspect("\ude40")`).
            _ if is_lone_surrogate(text, index) => ascii(out, &format!("\\u{unit:04x}")),
            other => out.push(other),
        }
    }
    out.push(u16::from(b'"'));
}

/// A chave de propriedade sem aspas só se for um identificador ASCII (`[A-Za-z_$][A-Za-z0-9_$]*`).
fn is_bare_key(text: &[u16]) -> bool {
    let is_start = |unit: u16| u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$');
    let is_part = |unit: u16| u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$');
    text.first().is_some_and(|&first| is_start(first)) && text.iter().all(|&unit| is_part(unit))
}

/// Uma chave própria enumerável já formatada, com o nome para ler o valor.
struct Key {
    text: Vec<u16>,
    /// Quanto a chave soma à estimativa de largura (`write_property_key`, ConsoleObject.rs): identificador `len + 1`,
    /// com aspas `len + 2` (o `len` é o da chave crua, sem escapadas), símbolo `1 + "[Symbol()]:".len() + len`.
    est: usize,
    name: PropertyName,
    /// A chave é o símbolo `@@toStringTag`: o caminho rápido do bun (`forEachPropertyImpl`) o descarta quando o objeto tem
    /// qualquer outra propriedade.
    is_tag: bool,
    /// Identificador sem aspas, e chave de símbolo: escolhem as cores de [`Formatter::push_key`].
    bare: bool,
    symbol: bool,
}

/// A chave de nome `name` (índice ou texto), no formato do `write_property_key`.
fn name_key(global_object: &JSGlobalObject, name: &crate::wtf::text::wtf_string::String) -> Key {
    let units = wtf_string_to_units(name);
    let mut text = Vec::new();
    let bare = is_bare_key(&units);
    let est = if bare {
        text.extend_from_slice(&units);
        units.len() + 1
    } else {
        push_quoted(&mut text, &units);
        units.len() + 2
    };
    Key { text, est, name: PropertyName::from_identifier(&Identifier::from_string(global_object.vm(), name)), is_tag: false, bare, symbol: false }
}

/// A chave de símbolo `symbol` (`[Symbol(desc)]`), `None` com exceção pendente.
fn symbol_key(global_object: &JSGlobalObject, symbol: JSValue) -> Option<Key> {
    let identifier = symbol.to_property_key(global_object)?;
    let description = as_symbol(symbol).try_get_descriptive_string().unwrap_or_default();
    let description = wtf_string_to_units(&description);
    // O texto descritivo é `Symbol(desc)`: a descrição tem 8 unidades a menos.
    let est = 1 + "[Symbol()]:".len() + description.len().saturating_sub("Symbol()".len());
    let mut text = vec![u16::from(b'[')];
    text.extend(description);
    text.push(u16::from(b']'));
    let is_tag = identifier == global_object.vm().property_names.to_string_tag_symbol;
    Some(Key { text, est, name: PropertyName::from_identifier(&identifier), is_tag, bare: false, symbol: true })
}

/// `units` é o texto ASCII `text`.
fn same_text(units: &[u16], text: &str) -> bool {
    units.iter().copied().eq(text.bytes().map(u16::from))
}

/// `get_object_name` (ConsoleObject.rs): o nome de classe do objeto, `[Object: null prototype]` para o protótipo nulo, e
/// nada para o objeto comum. `getClassName` do bun é o `calculatedClassName` (o texto vazio cai no `className` do objeto
/// comum, `Object`).
fn object_name(value: JSValue) -> Option<Vec<u16>> {
    let object = value.as_object();
    let name = wtf_string_to_units(&calculated_class_name(&object));
    if !name.is_empty() && !same_text(&name, "Object") {
        return Some(name);
    }
    if object.get_prototype_direct().is_null() {
        return Some("[Object: null prototype]".bytes().map(u16::from).collect());
    }
    None
}

/// Classes nativas do bun (WebCore) que, além da profundidade, saem como `[Nome ...]` (medido no bun 1.4.2). As que
/// imprimem `Nome [Object]` (URL, streams, `PerformanceMark`...) seguem outro caminho e não entram aqui.
const NATIVE_DEPTH_LABELS: &[&str] = &[
    "AbortController",
    "AbortSignal",
    "CloseEvent",
    "Crypto",
    "CustomEvent",
    "DOMException",
    "ErrorEvent",
    "Event",
    "EventTarget",
    "MessageChannel",
    "MessageEvent",
    "MessagePort",
    "Performance",
    "PerformanceObserver",
    "SubtleCrypto",
    "TextDecoder",
    "TextEncoder",
];

/// O nome da classe nativa de `value`: o primeiro `ClassInfo` da cadeia de protótipos (o próprio objeto inclusive, como
/// `Event.prototype`) que seja de uma classe de `NATIVE_DEPTH_LABELS`. A subclasse do usuário herda o nome da base.
fn native_class_label(value: JSValue) -> Option<&'static str> {
    let mut current = value;
    while current.is_object() {
        let object = current.as_object();
        let name = object.class_info().class_name;
        if let Some(label) = NATIVE_DEPTH_LABELS.iter().copied().find(|label| *label == name) {
            return Some(label);
        }
        current = object.get_prototype_direct();
    }
    None
}

/// Objetos cujas propriedades herdadas do protótipo imediato o bun também imprime (`Symbol` e `BigInt` encaixotados,
/// `WeakRef`, `FinalizationRegistry`): a instância não tem estrutura própria e o `forEachPropertyImpl` desce ao protótipo
/// e lista todas as chaves dele, enumeráveis ou não.
fn prints_prototype_members(id: usize) -> bool {
    match cell_registry::get(id) {
        None => true,
        // O objeto comum registrado (`Object.create`, instância de classe do usuário) é o mesmo caso do não registrado.
        Some(CellEntry::Object(_)) => true,
        // `DOMException`: `code`, `name`, `message`, as constantes `*_ERR` e `toString` vêm do protótipo.
        Some(CellEntry::DOMException(_)) => true,
        Some(CellEntry::WrapperObject(wrapper)) => wrapper.type_() == JSType::ObjectType,
        Some(CellEntry::WeakObjectRef(_) | CellEntry::FinalizationRegistry(_)) => true,
        _ => false,
    }
}

/// O estado de uma chamada de `console.log`: indentação, estimativa de largura e os objetos em impressão.
pub struct Formatter {
    indent: usize,
    est: usize,
    depth: usize,
    seen: Vec<usize>,
    max_depth: usize,
    /// `Formatter.single_line` do bun: nunca quebra linha (`good_time_for_a_new_line` não é consultado).
    single_line: bool,
    /// `enable_colors` do bun: as sequências ANSI de `Output::pretty_fmt` (`<r>` zera, `<d>` 2, `<yellow>` 33...).
    colors: bool,
}

/// O tamanho visível de `units`: sem as sequências `ESC [ ... m` (o `visible_width_exclude_ansi_colors` do bun).
pub fn visible_len(units: &[u16]) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < units.len() {
        if units[index] == 0x1B && units.get(index + 1) == Some(&u16::from(b'[')) {
            while index < units.len() && units[index] != u16::from(b'm') {
                index += 1;
            }
            index += 1;
            continue;
        }
        count += 1;
        index += 1;
    }
    count
}

impl Formatter {
    pub fn new() -> Formatter {
        Formatter { indent: 0, est: 0, depth: 0, seen: Vec::new(), max_depth: MAX_DEPTH, single_line: false, colors: false }
    }

    /// Liga as cores (`console.dir(x, { colors: true })`).
    pub fn with_colors(mut self, colors: bool) -> Formatter {
        self.colors = colors;
        self
    }

    pub fn colors(&self) -> bool {
        self.colors
    }

    /// O recuo inicial: `default_indent` do console (um nível por `console.group`), que o bun copia para `Formatter::indent`.
    pub fn at_indent(mut self, levels: usize) -> Formatter {
        self.indent = levels;
        self
    }

    /// `<r>` + a cor `code`, se há cores.
    fn open(&self, out: &mut Vec<u16>, code: &str) {
        if self.colors {
            ascii(out, &format!("\x1b[0m\x1b[{code}m"));
        }
    }

    /// `<r>`, se há cores.
    fn close(&self, out: &mut Vec<u16>) {
        if self.colors {
            ascii(out, "\x1b[0m");
        }
    }

    /// Um primitivo que não é string (`print_integer`, `print_double`, `print_bigint`, `print_boolean`, `print_null`,
    /// `print_undefined`, `print_symbol`): amarelo, `undefined` apagado, símbolo azul. Devolve o tamanho do texto puro.
    pub fn push_primitive(&self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> Option<usize> {
        self.open(out, if value.is_undefined() { "2" } else if value.is_symbol() { "34" } else { "33" });
        let before = out.len();
        let pushed = push_plain(global_object, out, value);
        let length = out.len() - before;
        self.close(out);
        pushed.then_some(length)
    }

    /// A chave de propriedade (`write_property_key`): `<r>nome<d>:<r> `, entre aspas `<r><green>"nome"<r><d>:<r> `,
    /// símbolo `<r><d>[<r><blue>Symbol(d)<r><d>]:<r> `; com valor string, `<r><green>` abre a cor do valor.
    fn push_key(&self, out: &mut Vec<u16>, key: &Key, string_value: bool) {
        if !self.colors {
            out.extend_from_slice(&key.text);
            ascii(out, ": ");
            return;
        }
        if key.symbol {
            ascii(out, "\x1b[0m\x1b[2m[\x1b[0m\x1b[34m");
            out.extend_from_slice(&key.text[1..key.text.len() - 1]);
            ascii(out, "\x1b[0m\x1b[2m]:\x1b[0m ");
        } else if key.bare {
            ascii(out, "\x1b[0m");
            out.extend_from_slice(&key.text);
            ascii(out, "\x1b[2m:\x1b[0m ");
        } else {
            ascii(out, "\x1b[0m\x1b[32m");
            out.extend_from_slice(&key.text);
            ascii(out, "\x1b[0m\x1b[2m:\x1b[0m ");
        }
        if string_value {
            ascii(out, "\x1b[0m\x1b[32m");
        }
    }

    /// O formatador de `Bun__inspect_singleline`: `max_depth: u16::MAX`, `single_line: true`.
    pub fn single_line() -> Formatter {
        Formatter { max_depth: usize::from(u16::MAX), single_line: true, ..Formatter::new() }
    }

    /// O formatador de célula do `TablePrinter::init` (ConsoleObject.rs): `single_line` e `max_depth: 5`, com o recuo
    /// inicial do grupo de `console.group`.
    pub fn table_cell(levels: usize) -> Formatter {
        Formatter { indent: levels, single_line: true, max_depth: 5, ..Formatter::new() }
    }

    /// Um formatador que já está em `depth` e só desce até `max_depth` (o relato de exceção formata as propriedades do
    /// erro com `depth + 1` e `max_depth = 1`, `print_error_instance_body`).
    pub fn with_depth(depth: usize, max_depth: usize) -> Formatter {
        Formatter { depth, max_depth, ..Formatter::new() }
    }

    /// O formatador das propriedades de um `Error` dentro de outro valor: o bun usa o mesmo `Formatter` (um só mapa de
    /// visitados), então os objetos que os níveis de fora estão imprimindo continuam valendo como `[Circular]`.
    pub(crate) fn inside_error_report(mut self) -> Formatter {
        self.seen = ENCLOSING_SEEN.with(|seen| seen.borrow().clone());
        self
    }

    /// Soma à estimativa o texto que quem chama escreveu (outros argumentos e o espaço entre eles).
    pub fn note(&mut self, count: usize) {
        self.est += count;
    }

    /// Imprime `value` (um objeto não chamável) em `out`. `false` com exceção pendente.
    pub fn push_object(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> bool {
        self.format(global_object, out, value)
    }

    /// `good_time_for_a_new_line` (ConsoleObject.rs, `impl Formatter`): se a linha estimada passa de 80, zera para
    /// `indent * 2` e devolve `true`.
    fn good_time(&mut self) -> bool {
        if !self.single_line && self.est > LINE_LIMIT {
            self.reset();
            return true;
        }
        false
    }

    /// `WrappedWriter::print_comma`: a vírgula soma 1.
    fn comma(&mut self, out: &mut Vec<u16>) {
        ascii(out, if self.colors { "\x1b[0m\x1b[2m,\x1b[0m" } else { "," });
        self.est += 1;
    }

    /// O separador entre dois itens de array: quebra de linha se `good_time_for_a_new_line`, senão um espaço (soma 1).
    /// Devolve `true` quando quebrou (o que vira `was_good_time` no `print_array`).
    fn separator(&mut self, out: &mut Vec<u16>) -> bool {
        if self.good_time() {
            ascii(out, "\n");
            self.pad(out);
            true
        } else {
            self.est += 1;
            ascii(out, " ");
            false
        }
    }

    /// `empty item` / `N x empty items`: soma `"empty item".len()`, ou os dígitos de N mais `" x empty items".len()`.
    fn hole(&mut self, out: &mut Vec<u16>, run: u64) {
        let text = if run == 1 { "empty item".to_string() } else { format!("{run} x empty items") };
        self.est += text.len();
        self.open(out, "2");
        ascii(out, &text);
        self.close(out);
    }

    fn reset(&mut self) {
        self.est = self.indent * 2;
    }

    fn pad(&self, out: &mut Vec<u16>) {
        out.extend(std::iter::repeat(u16::from(b' ')).take(self.indent * 2));
    }

    /// `print_object_depth_exceeded`: quebra de linha se a estimativa passou, e `[Nome ...]` em ciano. Os objetos web
    /// (`URLSearchParams`, `FormData`, eventos) levam o próprio nome; os demais, `Object`.
    fn push_depth_exceeded(&mut self, out: &mut Vec<u16>, label: &str) {
        if self.good_time() {
            ascii(out, "\n");
            self.pad(out);
        }
        self.open(out, "36");
        ascii(out, &format!("[{label} ...]"));
        self.close(out);
    }

    fn format(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> bool {
        if value.is_string() {
            let text = wtf_string_to_units(&value.to_wtf_string());
            self.est += text.len();
            // Uma string de 16 bits sai pelo `print_json` do bun, que soma ao estimador o comprimento do `JSON.stringify`
            // (aspas e escapadas incluídas) depois do comprimento da string; a de 8 bits, não.
            if text.iter().any(|&unit| unit > 0xFF) {
                self.est += json_quoted_length(&text);
            }
            // `print_string`: a string vazia sai sem cor; senão `<r><green>"..."<r>`.
            if text.is_empty() {
                push_quoted(out, &text);
            } else {
                self.open(out, "32");
                push_quoted(out, &text);
                self.close(out);
            }
            return !has_exception(global_object);
        }
        if value.is_object() && value.is_callable() {
            // `print_function` e `print_class`: `<cyan>[...]<r>`, sem o `<r>` antes.
            if self.colors {
                ascii(out, "\x1b[36m");
            }
            let done = self.format_callable(global_object, out, value);
            self.close(out);
            return done;
        }
        if !value.is_object() {
            let Some(length) = self.push_primitive(global_object, out, value) else { return false };
            // O `n` do bigint não entra na estimativa.
            self.est += length - usize::from(value.is_big_int());
            return true;
        }
        let id = value.as_cell();
        if self.seen.contains(&id) {
            self.open(out, "36");
            ascii(out, "[Circular]");
            self.close(out);
            return true;
        }
        if let Some(done) = self.format_web_pairs(global_object, out, value, id) {
            return done;
        }
        if let Some(done) = self.format_event_pairs(global_object, out, value, id) {
            return done;
        }
        if let Some(done) = self.format_web_body(global_object, out, value, id) {
            return done;
        }
        if let Some(done) = self.custom_inspect(global_object, out, value) {
            return done;
        }
        if let Some(error) = ErrorInstance::from_cell_id(id) {
            // `Tag::Error` (`print_error` do ConsoleObject.rs): `print_errorlike_object`, o mesmo relato do erro não capturado
            // (trecho do fonte, `^`, cabeçalho, propriedades, frames), sem o rodapé.
            let mut report = Vec::new();
            let outer = ENCLOSING_SEEN.with(|seen| std::mem::replace(&mut *seen.borrow_mut(), self.seen.clone()));
            append_error_report(&mut report, global_object, "", error, self.colors);
            ENCLOSING_SEEN.with(|seen| *seen.borrow_mut() = outer);
            out.extend(String::from_utf8_lossy(&report).encode_utf16());
            return !has_exception(global_object);
        }
        if let Some(done) = self.format_builtin(global_object, out, value, id) {
            return done;
        }
        let Ok(is_array) = global_object.is_array(value) else { return false };
        if !is_array {
            // `print_object` (ConsoleObject.rs): `always_newline = ... || self.good_time_for_a_new_line()` roda antes do teste
            // de profundidade e zera a linha estimada se ela passou de 80.
            self.good_time();
            // `print_object_depth_exceeded`: `else if self.always_newline_scope || self.good_time_for_a_new_line()` escreve
            // `\n` e a indentação; a segunda consulta só acusa se `indent * 2` já passa de 80.
            if self.depth > self.max_depth {
                self.push_depth_exceeded(out, native_class_label(value).unwrap_or("Object"));
                return true;
            }
        }
        if self.depth >= MAX_NESTING {
            ascii(out, "[Array]");
            return true;
        }
        self.seen.push(id);
        self.depth += 1;
        let done = if is_array { self.format_array(global_object, out, value) } else { self.format_object(global_object, out, value) };
        self.depth -= 1;
        self.seen.pop();
        done
    }

    /// `Symbol.for('nodejs.util.inspect.custom')` do objeto: chama o método com `(depth, options, inspect)` e usa o retorno.
    /// A string sai crua (sem reindentação), o próprio objeto devolvido cai no formato comum e qualquer outro valor é
    /// formatado de novo. `None` quando não há método chamável; `Some(false)` com exceção pendente (o que o bun deixa
    /// escapar do `console.log`). O terceiro argumento (`util.inspect`) vai `undefined`: o porte ainda não tem `node:util`.
    fn custom_inspect(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> Option<bool> {
        let remaining = js_number(self.max_depth.saturating_sub(self.depth) as f64);
        let result = match call_custom_inspect(global_object, value, remaining, js_number(self.max_depth as f64), self.colors)? {
            Some(result) => result,
            None => return Some(false),
        };
        if result.is_string() {
            let text = wtf_string_to_units(&result.to_wtf_string());
            self.est += text.len();
            out.extend_from_slice(&text);
            return Some(true);
        }
        if result.is_object() && result.as_cell() == value.as_cell() {
            return None;
        }
        Some(self.format(global_object, out, result))
    }

    /// As chaves próprias: índices enumeráveis (só para objeto), nomes na ordem de criação e depois símbolos. Os nomes e
    /// os símbolos incluem os não enumeráveis: o `forEachPropertyImpl` percorre a estrutura inteira (medido no bun 1.4.2,
    /// `Object.defineProperty({}, 'x', {value: 1})` sai `{ x: 1 }`).
    fn keys(global_object: &JSGlobalObject, value: JSValue, is_array: bool) -> Option<Vec<Key>> {
        let vm = global_object.vm();
        let mut keys = Vec::new();
        for key in global_object.own_enumerable_string_keys(value).ok()? {
            if let JsonKey::Index(index) = key {
                if !is_array {
                    let mut text = Vec::new();
                    push_quoted(&mut text, &index.to_string().bytes().map(u16::from).collect::<Vec<u16>>());
                    keys.push(Key { text, est: index.to_string().len() + 2, name: PropertyName::from_identifier(&Identifier::from_u32(vm, index)), is_tag: false, bare: false, symbol: false });
                }
            }
        }
        let object = value.as_object();
        let names = own_property_keys(global_object, &object, PropertyNameMode::Strings, DontEnumPropertiesMode::Include).ok()?;
        for index in 0..names.length() {
            let name = names.get_by_index(vm, index).to_wtf_string();
            // O `length` do array não está na estrutura.
            if name.impl_().is_some_and(|text| crate::runtime::identifier::parse_index_impl(text).is_some()) || (is_array && same_text(&wtf_string_to_units(&name), "length")) {
                continue;
            }
            keys.push(name_key(global_object, &name));
        }
        let symbols = own_property_keys(global_object, &object, PropertyNameMode::Symbols, DontEnumPropertiesMode::Include).ok()?;
        for index in 0..symbols.length() {
            let key = symbol_key(global_object, symbols.get_by_index(vm, index))?;
            // O `@@toStringTag` não enumerável fica de fora (ele só dá o nome antes do `{`).
            let mut descriptor = crate::runtime::property_descriptor::PropertyDescriptor::default();
            if key.is_tag && !(object.get_own_property_descriptor(vm, &key.name, &mut descriptor) && descriptor.enumerable()) {
                continue;
            }
            keys.push(key);
        }
        // O caminho rápido do bun percorre a `Structure` em ordem de inserção, com os símbolos entre as chaves de texto
        // (`{a: 1, [sym]: 2, b: 3}` sai nessa ordem, medido no bun 1.4.2); o `[[OwnPropertyKeys]]` daria texto e depois símbolos.
        if keys.iter().any(|key| key.symbol) {
            let structure = object.structure();
            if !object.has_non_reified_static_properties(&structure) && structure.can_perform_fast_property_enumeration() {
                let order: Vec<_> = structure.properties_with_privacy().into_iter().filter(|entry| !entry.3).map(|entry| entry.0).collect();
                let position = |key: &Key| key.name.uid().and_then(|uid| order.iter().position(|candidate| candidate == uid));
                // As chaves fora da estrutura (índices) ficam na frente, na ordem em que vieram.
                keys.sort_by_key(|key| position(key).map_or(0, |index| index + 1));
            }
        }
        Some(keys)
    }

    /// Os membros que o bun lista além dos próprios (`forEachPropertyImpl`): sobe a cadeia de protótipos, no máximo cinco
    /// níveis e sem chegar ao `Object.prototype`, e acrescenta toda chave de cada protótipo (enumerável ou não), menos
    /// `constructor`, `__proto__` e `__esModule` (o `@@toStringTag` sai depois, em `format_object`, se houver outra chave). O valor sai do `get` na própria instância (o acessor roda com ela).
    fn prototype_keys(global_object: &JSGlobalObject, value: JSValue, keys: &mut Vec<Key>) -> Option<()> {
        // O orçamento do `forEachPropertyImpl`: o caminho rápido (estrutura sem acessor, sem índice e sem `__proto__`)
        // confere `prototypeCount++ < 5` depois de cada nível, o lento confere antes; por isso uma vez no lento só se
        // visitam quatro protótipos, e no rápido cinco. Um nível rápido sem nenhum acerto cai no lento no mesmo nível, e
        // um objeto sem propriedade própria começa já no primeiro protótipo, com o orçamento em um.
        let stop = |level: JSValue| level.as_cell() == global_object.object_prototype().as_value().as_cell() || level.is_callable();
        let mut count = 0;
        let mut level = value;
        let mut fast = Formatter::level_is_fast(global_object, level)?;
        if fast && Formatter::own_property_count(global_object, level)? == 0 {
            fast = false;
            let proto = level.as_object().get_prototype_direct();
            if proto.is_object() {
                level = proto;
                fast = Formatter::level_is_fast(global_object, proto)?;
                count = 1;
            }
        }
        while fast {
            let hits = Formatter::add_level_keys(global_object, level, level.as_cell() == value.as_cell(), keys, true)?;
            if !hits {
                break;
            }
            let go = count < 5;
            count += 1;
            let proto = level.as_object().get_prototype_direct();
            if !go || !proto.is_object() || stop(proto) {
                return Some(());
            }
            level = proto;
            fast = Formatter::level_is_fast(global_object, level)?;
        }
        while level.is_object() && !stop(level) && {
            let go = count < 5;
            count += 1;
            go
        } {
            Formatter::add_level_keys(global_object, level, level.as_cell() == value.as_cell(), keys, false)?;
            if level.is_callable() {
                break;
            }
            level = level.as_object().get_prototype_direct();
        }
        Some(())
    }

    /// `canPerformFastPropertyEnumerationForIterationBun`, até onde o modelo enxerga: sem acessor próprio, sem índices (array,
    /// typed array) e sem a chave `__proto__`.
    fn level_is_fast(global_object: &JSGlobalObject, level: JSValue) -> Option<bool> {
        let vm = global_object.vm();
        if global_object.is_array(level).ok()? || cell_registry::get(level.as_cell()).is_some_and(|entry| !matches!(entry, CellEntry::WrapperObject(_) | CellEntry::Object(_))) {
            return Some(false);
        }
        let object = level.as_object();
        let names = own_property_keys(global_object, &object, PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Include).ok()?;
        for index in 0..names.length() {
            let name = names.get_by_index(vm, index);
            if name.is_symbol() {
                continue;
            }
            let key = name_key(global_object, &name.to_wtf_string());
            let mut descriptor = crate::runtime::property_descriptor::PropertyDescriptor::default();
            if same_text(&key.text, "__proto__") || (object.get_own_property_descriptor(vm, &key.name, &mut descriptor) && descriptor.is_accessor_descriptor()) {
                return Some(false);
            }
        }
        Some(true)
    }

    /// Quantas chaves próprias (strings e símbolos, enumeráveis ou não) o objeto tem.
    fn own_property_count(global_object: &JSGlobalObject, level: JSValue) -> Option<u32> {
        let object = level.as_object();
        Some(own_property_keys(global_object, &object, PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Include).ok()?.length())
    }

    /// Acrescenta as chaves de um nível da cadeia (as do próprio objeto já estão em `keys`) e diz se houve acerto: no caminho
    /// rápido, uma chave que não seja `constructor`, `__proto__`, `@@toStringTag` nem `__esModule` (de protótipo) e ainda
    /// não vista.
    fn add_level_keys(global_object: &JSGlobalObject, level: JSValue, is_own: bool, keys: &mut Vec<Key>, fast: bool) -> Option<bool> {
        let vm = global_object.vm();
        let object = level.as_object();
        let names = own_property_keys(global_object, &object, PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Include).ok()?;
        let mut level_keys = Vec::with_capacity(names.length() as usize);
        for index in 0..names.length() {
            let name = names.get_by_index(vm, index);
            level_keys.push(if name.is_symbol() { symbol_key(global_object, name)? } else { name_key(global_object, &name.to_wtf_string()) });
        }
        // O caminho rápido percorre a estrutura (`structure->forEachProperty`) na ordem de inserção, com os símbolos
        // intercalados; o `ownPropertyKeys` põe os símbolos no fim (medido no bun 1.4.2: o protótipo `{[Symbol('s')]: 1, a: 2}`
        // sai com o símbolo primeiro).
        if fast {
            let order: Vec<_> = object.structure().properties_with_privacy().into_iter().filter(|entry| !entry.3).map(|entry| entry.0).collect();
            level_keys.sort_by_key(|key| key.name.uid().and_then(|uid| order.iter().position(|known| known == uid)).unwrap_or(usize::MAX));
        }
        let mut hits = false;
        for key in level_keys {
            let filtered = same_text(&key.text, "constructor") || same_text(&key.text, "__proto__") || (!is_own && same_text(&key.text, "__esModule"));
            if filtered || (fast && key.is_tag) || (!is_own && keys.iter().any(|known| known.text == key.text)) {
                continue;
            }
            hits = true;
            // O caminho lento descarta o `@@toStringTag` só quando ele não é enumerável.
            if !is_own && !fast && key.is_tag {
                let mut descriptor = crate::runtime::property_descriptor::PropertyDescriptor::default();
                if !(object.get_own_property_descriptor(vm, &key.name, &mut descriptor) && descriptor.enumerable()) {
                    continue;
                }
            }
            if !is_own {
                keys.push(key);
            }
        }
        Some(hits)
    }

    /// `[Getter]`, `[Setter]` ou `[Getter/Setter]` (`print_getter_setter`) quando `name` é um acessor, próprio ou herdado:
    /// o `forEachPropertyImpl` resolve a chave com `getPropertySlot` na cadeia, então vale o primeiro dono dela.
    fn accessor_label(global_object: &JSGlobalObject, object: &crate::runtime::js_object::JSObject, name: &PropertyName) -> Option<&'static str> {
        let vm = global_object.vm();
        let mut descriptor = crate::runtime::property_descriptor::PropertyDescriptor::default();
        if !object.get_own_property_descriptor(vm, name, &mut descriptor) {
            let prototype = object.get_prototype_direct();
            return if prototype.is_object() { Formatter::accessor_label(global_object, &prototype.as_object(), name) } else { None };
        }
        if !descriptor.is_accessor_descriptor() {
            return None;
        }
        // Atributo nativo do WebCore (`aborted`, `type`, `code`...): no bun é um `CustomGetterSetter` que o formatador lê
        // como valor, não um `[Getter]`.
        if descriptor.getter_present() && is_custom_accessor_function(descriptor.getter()) {
            return None;
        }
        match (descriptor.getter_object().is_some(), descriptor.setter_object().is_some()) {
            (true, true) => Some("[Getter/Setter]"),
            (true, false) => Some("[Getter]"),
            (false, true) => Some("[Setter]"),
            (false, false) => None,
        }
    }

    /// `name` é um atributo nativo (`CustomGetterSetter`) do objeto ou de um protótipo dele.
    fn has_custom_accessor(global_object: &JSGlobalObject, object: &crate::runtime::js_object::JSObject, name: &PropertyName) -> bool {
        let mut descriptor = crate::runtime::property_descriptor::PropertyDescriptor::default();
        if !object.get_own_property_descriptor(global_object.vm(), name, &mut descriptor) {
            let prototype = object.get_prototype_direct();
            return prototype.is_object() && Formatter::has_custom_accessor(global_object, &prototype.as_object(), name);
        }
        descriptor.is_accessor_descriptor() && descriptor.getter_present() && is_custom_accessor_function(descriptor.getter())
    }

    fn format_object(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> bool {
        let Some(mut keys) = Formatter::keys(global_object, value, false) else { return false };
        if prints_prototype_members(value.as_cell()) && Formatter::prototype_keys(global_object, value, &mut keys).is_none() {
            return false;
        }
        // O `forEachPropertyImpl` pula a chave `constructor` também quando ela é própria (medido no bun 1.4.2: `{constructor: 5}`
        // sai `{}`, com getter, com protótipo nulo e dentro de array).
        keys.retain(|key| key.symbol || !same_text(&key.text, "constructor"));
        // O caminho rápido do bun descarta `@@toStringTag`; ele só aparece se for a única chave do objeto.
        if keys.iter().any(|key| !key.is_tag) {
            keys.retain(|key| !key.is_tag);
        }
        let object = value.as_object();
        // `handle_first_property` e `print_object_tail`: o nome vem antes do `{`, com um espaço.
        if let Some(name) = object_name(value) {
            out.extend_from_slice(&name);
            ascii(out, " ");
        }
        if keys.is_empty() {
            ascii(out, "{}");
            return true;
        }
        ascii(out, if self.single_line { "{ " } else { "{\n" });
        // `handle_first_property` (ConsoleObject.rs): a primeira propriedade fixa a linha em `indent * 2 + 1`, com o indent
        // ainda sem o incremento; as seguintes vêm depois de vírgula (+1), `\n`, indentação e `reset_line`.
        self.est = self.indent * 2 + 1;
        self.indent += 1;
        for (position, key) in keys.iter().enumerate() {
            if position > 0 {
                self.comma(out);
                if self.single_line {
                    self.est += 1;
                    ascii(out, " ");
                } else {
                    ascii(out, "\n");
                    self.reset();
                }
            }
            if !self.single_line {
                self.pad(out);
            }
            self.est += key.est;
            if let Some(label) = Formatter::accessor_label(global_object, &object, &key.name) {
                self.push_key(out, key, false);
                ascii(out, label);
                self.est += label.len();
                continue;
            }
            let mut property = object.get(global_object, &key.name);
            // O atributo nativo do WebCore que lança (o getter lido com o protótipo como `this`, `console.log(MIMEType.prototype)`)
            // sai `undefined` no bun, sem propagar a exceção (medido no bun 1.4.2).
            if has_exception(global_object) && Formatter::has_custom_accessor(global_object, &object, &key.name) {
                global_object.vm().clear_exception();
                property = JSValue::undefined();
            }
            if has_exception(global_object) {
                self.push_key(out, key, false);
                return false;
            }
            let string_value = self.colors && property.is_string();
            self.push_key(out, key, string_value);
            if !self.format(global_object, out, property) {
                return false;
            }
            // `for_each`: com valor string, um `<r>` fecha a cor aberta pela chave.
            if string_value {
                self.close(out);
            }
        }
        self.indent -= 1;
        // `print_object_tail`: `print_comma` (+1), `\n`, indentação, `}` (+1 sem zerar a linha).
        if self.single_line {
            ascii(out, " }");
            self.est += 2;
            return true;
        }
        self.comma(out);
        ascii(out, "\n");
        self.pad(out);
        ascii(out, "}");
        self.est += 1;
        true
    }

    /// `Headers`, `URLSearchParams` e `FormData`: o bun imprime o objeto de `toJSON` com o nome da classe, chaves sempre
    /// entre aspas e uma vírgula depois de cada par, antes de consultar o `inspect.custom`. `None` para outro valor.
    fn format_web_pairs(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue, id: usize) -> Option<bool> {
        let name = crate::runtime::web_iterable::class_name_of(value)?;
        let json = match name {
            "Headers" => crate::runtime::headers::json_object_of(global_object, value),
            "URLSearchParams" => crate::runtime::url_search_params::json_object_of(global_object, value),
            "FormData" => crate::runtime::form_data::json_object_of(global_object, value),
            _ => None,
        }?;
        let keys = Formatter::keys(global_object, json, false)?;
        // Além da profundidade, `Headers` imprime o nome e `[Object ...]`; os outros dois, `[Nome ...]`, e o `FormData`
        // vazio cai no relato genérico (`[Object ...]`).
        if self.depth > self.max_depth {
            self.good_time();
            if name == "Headers" {
                ascii(out, "Headers ");
                self.push_depth_exceeded(out, "Object");
            } else {
                self.push_depth_exceeded(out, if name == "FormData" && keys.is_empty() { "Object" } else { name });
            }
            return Some(true);
        }
        ascii(out, name);
        ascii(out, " ");
        if keys.is_empty() {
            ascii(out, "{}");
            return Some(true);
        }
        ascii(out, "{\n");
        self.est = self.indent * 2 + 1;
        self.seen.push(id);
        self.depth += 1;
        self.indent += 1;
        let object = json.as_object();
        for (position, key) in keys.iter().enumerate() {
            if position > 0 {
                ascii(out, "\n");
                self.reset();
            }
            self.pad(out);
            let mut text = Vec::new();
            if key.bare {
                push_quoted(&mut text, &key.text);
            } else {
                text.extend_from_slice(&key.text);
            }
            self.est += key.est;
            let quoted = Key { text, est: key.est, name: key.name.clone(), is_tag: false, bare: false, symbol: false };
            let property = object.get(global_object, &key.name);
            let string_value = self.colors && property.is_string();
            self.push_key(out, &quoted, string_value);
            if !self.format(global_object, out, property) {
                return Some(false);
            }
            if string_value {
                self.close(out);
            }
            self.comma(out);
        }
        self.indent -= 1;
        self.depth -= 1;
        self.seen.pop();
        ascii(out, "\n");
        self.pad(out);
        ascii(out, "}");
        self.est += 1;
        Some(true)
    }

    /// `MessageEvent` e `ErrorEvent`: o bun imprime só `type` e os campos de dados (`event_target::console_pairs`), um por
    /// linha com vírgula depois de cada um. `None` para outro valor.
    fn format_event_pairs(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue, id: usize) -> Option<bool> {
        let (name, pairs) = crate::runtime::event_target::console_pairs(global_object, value)?;
        let vm = global_object.vm();
        if self.depth > self.max_depth {
            self.good_time();
            self.push_depth_exceeded(out, name);
            return Some(true);
        }
        ascii(out, name);
        ascii(out, " {\n");
        self.est = self.indent * 2 + 1;
        self.seen.push(id);
        self.depth += 1;
        self.indent += 1;
        for (position, (field, property)) in pairs.into_iter().enumerate() {
            if position > 0 {
                ascii(out, "\n");
                self.reset();
            }
            self.pad(out);
            let key = Key { text: field.bytes().map(u16::from).collect(), est: field.len() + 1, name: PropertyName::from_identifier(&Identifier::from_span(vm, field.as_bytes())), is_tag: false, bare: true, symbol: false };
            self.est += key.est;
            let string_value = self.colors && property.is_string();
            self.push_key(out, &key, string_value);
            if !self.format(global_object, out, property) {
                return Some(false);
            }
            if string_value {
                self.close(out);
            }
            self.comma(out);
        }
        self.indent -= 1;
        self.depth -= 1;
        self.seen.pop();
        ascii(out, "\n");
        self.pad(out);
        ascii(out, "}");
        self.est += 1;
        Some(true)
    }

    /// `Blob`, `File`, `Request` e `Response` (sem cores): `Nome (tamanho) { campo: valor, ... }`, o `Blob (tamanho)` do
    /// corpo como última linha. `None` para outro valor (ou com cores, que esta fatia ainda não porta).
    fn format_web_body(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue, id: usize) -> Option<bool> {
        let latin1 = |bytes: &[u8]| -> Vec<u16> { bytes.iter().map(|&byte| u16::from(byte)).collect() };
        let body_parts = |body: &Option<Body>| -> (usize, Option<(usize, Vec<u8>)>) {
            match body {
                Some(body) if !body.used => {
                    let shown_type = if body.implied_type { Vec::new() } else { body.blob_type.clone() };
                    (body.bytes.len(), (!body.bytes.is_empty()).then(|| (body.bytes.len(), shown_type)))
                }
                _ => (0, None),
            }
        };
        if let Some(state) = crate::runtime::blob::state_of(value) {
            let file_ref = crate::runtime::blob_file::display_path(value).map(|shown| format!("FileRef ({shown})"));
            let mut fields = Vec::new();
            if let Some(name) = state.name.as_ref().filter(|_| file_ref.is_none()) {
                fields.push(("name", WebField::Quoted(name.clone())));
            }
            if !state.content_type.is_empty() {
                fields.push(("type", WebField::Quoted(latin1(&state.content_type))));
            }
            if let Some(modified) = state.last_modified.filter(|_| state.is_file) {
                fields.push(("lastModified", WebField::Raw(format!("{modified}"))));
            }
            let label = file_ref.as_deref().unwrap_or(if state.is_file { "File" } else { "Blob" });
            return Some(self.push_web_object(global_object, out, id, label, state.bytes.len(), fields, None, false));
        }
        if let Some(state) = crate::runtime::request::state_of(value) {
            let (size, nested) = body_parts(&state.body);
            let fields = vec![
                ("method", WebField::Quoted(state.method.encode_utf16().collect())),
                ("url", WebField::Quoted(state.url.clone())),
                ("headers", WebField::Headers(state.headers)),
            ];
            return Some(self.push_web_object(global_object, out, id, "Request", size, fields, nested, false));
        }
        let state = crate::runtime::response::state_of(value)?;
        let (size, nested) = body_parts(&state.body);
        let fields = vec![
            ("ok", WebField::Raw((200..300).contains(&state.status).to_string())),
            ("url", WebField::Quoted(state.url.clone())),
            ("status", WebField::Raw(state.status.to_string())),
            ("statusText", WebField::Quoted(state.status_text.clone())),
            ("headers", WebField::Headers(state.headers)),
            ("redirected", WebField::Raw("false".to_string())),
            ("bodyUsed", WebField::Raw(state.body.as_ref().is_some_and(|body| body.used).to_string())),
        ];
        Some(self.push_web_object(global_object, out, id, "Response", size, fields, nested, true))
    }

    /// O texto de `Nome (tamanho)`, as chaves com os campos (um por linha, `fields` vazio e sem corpo: sem chaves) e o
    /// `Blob (tamanho)` do corpo no fim, recursivo (o tipo do corpo é um campo dele). `comma_before_body`: o bun põe vírgula
    /// depois do último campo de `Response`, mas não de `Request`.
    #[allow(clippy::too_many_arguments)]
    fn push_web_object(
        &mut self,
        global_object: &JSGlobalObject,
        out: &mut Vec<u16>,
        id: usize,
        label: &str,
        size: usize,
        fields: Vec<(&str, WebField)>,
        nested: Option<(usize, Vec<u8>)>,
        comma_before_body: bool,
    ) -> bool {
        // `Blob` e `File` pintam o cabeçalho (`<r>Blob<r> (<yellow>3 bytes<r>)`); `Request` e `Response` o imprimem cru.
        let blob_like = label == "Blob" || label == "File" || label.starts_with("FileRef");
        if label.starts_with("FileRef") {
            // Blob de arquivo: `FileRef ("<caminho>")`, sem o tamanho (medido no bun 1.4.2).
            ascii(out, label);
        } else if self.colors && blob_like {
            ascii(out, &format!("\x1b[0m{label}\x1b[0m (\x1b[33m{}\x1b[0m)", format_byte_size(size as u64)));
        } else {
            ascii(out, &format!("{label} ({})", format_byte_size(size as u64)));
        }
        if fields.is_empty() && nested.is_none() {
            return true;
        }
        ascii(out, " {\n");
        self.est = self.indent * 2 + 1;
        self.seen.push(id);
        // Os campos saem na profundidade do próprio objeto (medido no bun 1.4.2: `console.dir(response, {depth: 0})`
        // imprime os `Headers` por inteiro, e `{k: response}` com `depth: 0` os resume como `Headers [Object ...]`).
        self.indent += 1;
        let count = fields.len();
        for (position, (name, field)) in fields.into_iter().enumerate() {
            if position > 0 {
                ascii(out, "\n");
                self.reset();
            }
            self.pad(out);
            if !self.colors {
                ascii(out, &format!("{name}: "));
            } else if blob_like {
                ascii(out, &format!("{name}\x1b[2m:\x1b[0m "));
            } else {
                ascii(out, &format!("\x1b[0m{name}\x1b[2m:\x1b[0m "));
            }
            self.est += name.len() + 2;
            match field {
                WebField::Quoted(units) => {
                    self.est += units.len();
                    if !self.colors {
                        push_quoted(out, &units);
                    } else {
                        let mut quoted = Vec::new();
                        push_quoted(&mut quoted, &units);
                        let inner = &quoted[1..quoted.len() - 1];
                        // As cores de cada campo medidas no bun: `Blob`/`File` verde; `method` cru; `url` e `statusText` em negrito.
                        let (before, after) = match (label, name) {
                            (_, _) if blob_like => ("\x1b[32m\"", "\"\x1b[0m"),
                            ("Request", "method") => ("\"", "\""),
                            ("Request", _) => ("\"\x1b[1m", "\x1b[0m\""),
                            (_, "url") => ("\"\x1b[0m\x1b[1m", "\x1b[0m\""),
                            _ => ("\x1b[0m\"\x1b[1m", "\x1b[0m\""),
                        };
                        ascii(out, before);
                        out.extend_from_slice(inner);
                        ascii(out, after);
                    }
                }
                WebField::Raw(text) => {
                    self.est += text.len();
                    if !self.colors {
                        ascii(out, &text);
                    } else if blob_like {
                        ascii(out, &format!("\x1b[33m{text}\x1b[0m"));
                    } else {
                        ascii(out, &format!("\x1b[0m\x1b[33m{text}\x1b[0m"));
                    }
                }
                WebField::Headers(headers) => {
                    self.close(out);
                    if !self.format(global_object, out, headers) {
                        return false;
                    }
                }
            }
            if position + 1 < count || (nested.is_some() && comma_before_body) {
                self.comma(out);
            }
        }
        if let Some((body_size, body_type)) = nested {
            ascii(out, "\n");
            self.reset();
            self.pad(out);
            let type_fields = if body_type.is_empty() { Vec::new() } else { vec![("type", WebField::Quoted(body_type.iter().map(|&byte| u16::from(byte)).collect()))] };
            if !self.push_web_object(global_object, out, id, "Blob", body_size, type_fields, None, false) {
                return false;
            }
        }
        self.indent -= 1;
        self.seen.pop();
        ascii(out, "\n");
        self.pad(out);
        ascii(out, "}");
        self.est += 1;
        true
    }

    fn format_array(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> bool {
        let Ok(length) = global_object.length_of_array_like(value) else { return false };
        let vm = global_object.vm();
        let object = value.as_object();
        let read = |index: u64| -> Option<JSValue> {
            let name = PropertyName::from_identifier(&Identifier::from_u32(vm, u32::try_from(index).ok()?));
            let element = object.get(global_object, &name);
            if has_exception(global_object) { None } else { Some(element) }
        };
        let present = |index: u64| u32::try_from(index).is_ok_and(|index| object.has_own_property_by_index(vm, index));
        self.print_array(global_object, out, length, &read, &present, Some(value))
    }

    /// `print_array` (ConsoleObject.rs), linha a linha. `extra` é o array cujas propriedades não indexadas entram no fim.
    fn print_array(
        &mut self,
        global_object: &JSGlobalObject,
        out: &mut Vec<u16>,
        length: u64,
        read: &dyn Fn(u64) -> Option<JSValue>,
        present: &dyn Fn(u64) -> bool,
        extra: Option<JSValue>,
    ) -> bool {
        if length == 0 {
            self.est += EMPTY_ARRAY;
            ascii(out, "[]");
            return true;
        }
        // `print_array` (ConsoleObject.rs), linha a linha.
        let mut good = !self.single_line && length > 10;
        self.indent += 1;
        self.est += 2;
        let first = if present(0) {
            let Some(first) = read(0) else { return false };
            Some(first)
        } else {
            None
        };
        // `!tag.is_primitive()`: função, classe, objeto, `Map` e `Set` quebram; caixas de número, booleano e string não.
        good = good || !self.single_line && first.is_some_and(|first| !is_primitive_tag(first)) || self.good_time();
        ascii(out, "[");
        if good {
            self.reset();
            ascii(out, "\n");
            self.pad(out);
            self.est += 1;
        } else {
            ascii(out, " ");
            self.est += 2;
        }
        let mut empty_start: Option<u64> = None;
        match first {
            Some(first) => {
                if !self.format(global_object, out, first) {
                    return false;
                }
                if first.is_string() {
                    self.close(out);
                }
            }
            None => empty_start = Some(0),
        }
        let mut index = 1;
        let mut non_empty = 1;
        while index < length {
            if !present(index) {
                empty_start.get_or_insert(index);
                index += 1;
                continue;
            }
            if non_empty >= MAX_ITEMS {
                // "we want the line break to be unconditional here"
                self.comma(out);
                ascii(out, "\n");
                self.est = 0;
                self.pad(out);
                self.est += "... N more items".len();
                ascii(out, &format!("... {} more items", length - index));
                break;
            }
            non_empty += 1;
            if let Some(start) = empty_start.take() {
                if start > 0 {
                    self.comma(out);
                    good |= self.separator(out);
                }
                self.hole(out, index - start);
            }
            self.comma(out);
            good |= self.separator(out);
            let Some(element) = read(index) else { return false };
            if !self.format(global_object, out, element) {
                return false;
            }
            if element.is_string() {
                self.close(out);
            }
            index += 1;
        }
        if let Some(start) = empty_start.take() {
            if start > 0 {
                self.comma(out);
                good |= self.separator(out);
            }
            self.hole(out, length - start);
        }
        // Propriedades não indexadas: `always_newline = good_time_for_a_new_line()` (zera a linha se passou de 80).
        let always_newline = self.good_time();
        let keys = match extra {
            Some(array) => match Formatter::keys(global_object, array, true) {
                Some(keys) => keys,
                None => return false,
            },
            None => Vec::new(),
        };
        let object = extra.map(|array| array.as_object());
        for key in &keys {
            self.comma(out);
            if always_newline || self.good_time() {
                ascii(out, "\n");
                self.pad(out);
                self.reset();
            } else {
                self.est += 1;
                ascii(out, " ");
            }
            self.est += key.est;
            let property = object.as_ref().expect("chave sem array").get(global_object, &key.name);
            if has_exception(global_object) {
                self.push_key(out, key, false);
                return false;
            }
            let string_value = self.colors && property.is_string();
            self.push_key(out, key, string_value);
            if !self.format(global_object, out, property) {
                return false;
            }
            if string_value {
                self.close(out);
            }
        }
        self.indent -= 1;
        if good || self.good_time() {
            self.reset();
            ascii(out, "\n");
            self.pad(out);
            ascii(out, "]");
            self.reset();
            self.est += 1;
        } else {
            ascii(out, " ]");
            self.est += 2;
        }
        true
    }

    /// Função e classe (`Tag::Function` e `Tag::Class` do bun): sem propriedades próprias.
    fn format_callable(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> bool {
        let name = callable_name(global_object, value);
        let object = value.as_object();
        let proto = object.get_prototype_direct();
        if is_class(value) {
            // `print_class`: a soma é o tamanho dos dois nomes, e `extends` só aparece se o pai também é classe.
            let proto_name = if proto.is_object() && is_class(proto) { callable_name(global_object, proto) } else { Vec::new() };
            self.est += name.len() + proto_name.len();
            ascii(out, "[class ");
            if name.is_empty() {
                ascii(out, "(anonymous)");
            } else {
                out.extend_from_slice(&name);
            }
            if !proto_name.is_empty() {
                ascii(out, " extends ");
                out.extend_from_slice(&proto_name);
            }
            ascii(out, "]");
            return true;
        }
        // `print_function`: `func_name` é o nome do protótipo (`AsyncFunction`...), vazio para `Function.prototype`.
        let func_name = function_kind_name(global_object, proto);
        ascii(out, "[");
        if name.is_empty() || func_name == name {
            if func_name.is_empty() {
                ascii(out, "Function");
            } else {
                out.extend_from_slice(&func_name);
            }
        } else {
            if func_name.is_empty() {
                ascii(out, "Function");
            } else {
                out.extend_from_slice(&func_name);
            }
            ascii(out, ": ");
            out.extend_from_slice(&name);
        }
        ascii(out, "]");
        !has_exception(global_object)
    }

    /// `print_typed_array` (ConsoleObject.rs) para elementos de um byte: `Nome(N) [ a, b ]`. Só a vírgula e o espaço entre
    /// elementos somam à estimativa de linha; no máximo 513 elementos, depois `, ... N more`.
    fn format_byte_elements(&mut self, out: &mut Vec<u16>, name: &str, bytes: &[u8]) -> bool {
        ascii(out, &format!("{name}({}) [", bytes.len()));
        if bytes.is_empty() {
            ascii(out, "]");
            return true;
        }
        ascii(out, " ");
        const MAX: usize = 512;
        for (index, byte) in bytes.iter().take(MAX + 1).enumerate() {
            if index > 0 {
                self.comma(out);
                self.est += 1;
                ascii(out, " ");
            }
            ascii(out, &byte.to_string());
        }
        if bytes.len() > MAX + 1 {
            ascii(out, &format!(", ... {} more", bytes.len() - MAX - 1));
        }
        ascii(out, " ]");
        true
    }

    /// `Map`, `Set`, seus iteradores e as caixas de primitivo. `None` quando o valor segue o caminho de objeto comum.
    fn format_builtin(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue, id: usize) -> Option<bool> {
        let entry = cell_registry::get(id)?;
        let guarded = |formatter: &Formatter, out: &mut Vec<u16>| -> bool {
            if formatter.depth >= MAX_NESTING {
                ascii(out, "[Array]");
                return false;
            }
            true
        };
        match entry {
            CellEntry::WrapperObject(wrapper) if matches!(wrapper.type_(), JSType::NumberObjectType | JSType::BooleanObjectType) => {
                let label = if wrapper.type_() == JSType::NumberObjectType { "Number" } else { "Boolean" };
                let class_name = wtf_string_to_units(&calculated_class_name(&value.as_object()));
                // `to_js_string_view` do bun: o `ToString` do objeto (sem protótipo ele lança `No default value`).
                let text = wtf_string_to_units(&value.to_wtf_string());
                if has_exception(global_object) {
                    return Some(false);
                }
                // `print_double` soma `name + value + 4` (ou `12`), `print_boolean` soma `value + 11` (ou `name + value + 14`).
                let plain = class_name == label.bytes().map(u16::from).collect::<Vec<u16>>();
                self.est += if label == "Number" {
                    class_name.len() + text.len() + if plain { 4 } else { "[Number ():]".len() }
                } else if plain {
                    text.len() + "[Boolean: ]".len()
                } else {
                    text.len() + class_name.len() + "[Boolean (): ]".len()
                };
                ascii(out, "[");
                ascii(out, label);
                if !plain {
                    ascii(out, " (");
                    out.extend_from_slice(&class_name);
                    ascii(out, ")");
                }
                ascii(out, ": ");
                out.extend_from_slice(&text);
                ascii(out, "]");
                Some(true)
            }
            CellEntry::StringObject(_) => {
                // `BunString::from_js`: o `ToString` do objeto, que pode lançar.
                let text = wtf_string_to_units(&value.to_wtf_string());
                if has_exception(global_object) {
                    return Some(false);
                }
                self.est += text.len();
                // Na raiz (`quote_strings` falso) a caixa leva o rótulo; aninhada vira só a string entre aspas.
                if self.depth == 0 {
                    ascii(out, "[String: ");
                    push_quoted(out, &text);
                    ascii(out, "]");
                } else {
                    push_quoted(out, &text);
                }
                Some(true)
            }
            CellEntry::Map(_) | CellEntry::WeakMap(_) | CellEntry::Set(_) | CellEntry::WeakSet(_) => {
                if !guarded(self, out) {
                    return Some(true);
                }
                let (label, is_set) = match entry {
                    CellEntry::Map(_) => ("Map", false),
                    CellEntry::WeakMap(_) => ("WeakMap", false),
                    CellEntry::Set(_) => ("Set", true),
                    _ => ("WeakSet", true),
                };
                Some(self.format_collection(global_object, out, value, id, label, is_set))
            }
            CellEntry::MapIterator(iterator) => {
                if !guarded(self, out) {
                    return Some(true);
                }
                let cursor = Rc::new(Cell::new(iterator.entry()));
                let mut items = Vec::new();
                loop {
                    match iterator_step(iterator.iterated_object().table(), &cursor, iterator.kind(), false) {
                        IteratorStep::Done => break,
                        step => items.push(step),
                    }
                }
                Some(self.format_iterator(global_object, out, "MapIterator", items))
            }
            CellEntry::SetIterator(iterator) => {
                if !guarded(self, out) {
                    return Some(true);
                }
                let cursor = Rc::new(Cell::new(iterator.entry()));
                let mut items = Vec::new();
                loop {
                    match iterator_step(iterator.iterated_object().table(), &cursor, iterator.kind(), true) {
                        IteratorStep::Done => break,
                        step => items.push(step),
                    }
                }
                Some(self.format_iterator(global_object, out, "SetIterator", items))
            }
            CellEntry::ArrayBuffer(buffer) => {
                let name = if buffer.impl_().is_shared() { "SharedArrayBuffer" } else { "ArrayBuffer" };
                let bytes = buffer.impl_().with_bytes(<[u8]>::to_vec);
                Some(self.format_byte_elements(out, name, &bytes))
            }
            CellEntry::TypedArray(view) if matches!(view.type_(), JSType::Uint8ArrayType | JSType::Uint8ClampedArrayType) => {
                let name = if view.type_() == JSType::Uint8ArrayType { "Uint8Array" } else { "Uint8ClampedArray" };
                let mut bytes = Vec::new();
                for index in 0..view.length() {
                    match view.get_index_quickly(index) {
                        Ok(element) => bytes.push(element.as_number() as u8),
                        Err(_) => return Some(false),
                    }
                }
                Some(self.format_byte_elements(out, name, &bytes))
            }
            CellEntry::DateInstance(_) => Some(self.format_date(global_object, out, value)),
            CellEntry::RegExpObject(_) => {
                // `Tag::String` para `RegExpObject` (`print_string`, sem aspas): o `ToString` do objeto, que chama o `toString`
                // do usuário e lança sem protótipo.
                let text = wtf_string_to_units(&value.to_wtf_string());
                if has_exception(global_object) {
                    return Some(false);
                }
                self.est += text.len();
                if self.colors {
                    ascii(out, "\x1b[0m\x1b[31m");
                }
                out.extend_from_slice(&text);
                self.close(out);
                Some(true)
            }
            CellEntry::Promise(promise) => {
                // `print_promise`: a escrita direta não soma à estimativa; só a consulta de quebra, antes do `Promise {`.
                if self.good_time() {
                    ascii(out, "\n");
                    self.pad(out);
                }
                ascii(out, "Promise { ");
                ascii(out, match promise.status() {
                    Status::Pending => "<pending>",
                    Status::Fulfilled => "<resolved>",
                    Status::Rejected => "<rejected>",
                });
                ascii(out, " }");
                Some(true)
            }
            _ => None,
        }
    }

    /// `print_json` para `JSDate`: o `JSON.stringify` do objeto (chama `toJSON`, portanto o `toISOString` do usuário e
    /// pode lançar), sem as aspas; `null` é `Invalid Date`, e o texto de até duas unidades (o `{}` do protótipo nulo)
    /// sai como veio. A estimativa soma o texto com aspas.
    fn format_date(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> bool {
        let text = match json_stringify(global_object, value, JSValue::undefined(), JSValue::undefined()) {
            Ok(Some(text)) => wtf_string_to_units(&text),
            Ok(None) => Vec::new(),
            Err(error) => {
                throw_json_error(global_object, error);
                return false;
            }
        };
        self.est += text.len();
        self.open(out, "35");
        if same_text(&text, "null") {
            ascii(out, "Invalid Date");
        } else if text.len() > 2 {
            out.extend_from_slice(&text[1..text.len() - 1]);
        } else {
            out.extend_from_slice(&text);
        }
        self.close(out);
        true
    }

    /// `print_map_like` e `print_set`: `Nome(n) {`, uma entrada por linha (`chave: valor,` ou `valor,`), `}`. O tamanho
    /// vem da propriedade `size` (protótipo nulo ou `WeakMap` dão 0 e saem como `Nome {}`).
    fn format_collection(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue, id: usize, label: &str, is_set: bool) -> bool {
        let vm = global_object.vm();
        let size = value.as_object().get(global_object, &PropertyName::from_identifier(&vm.property_names.size));
        if has_exception(global_object) {
            return false;
        }
        let length = if size.is_int32() {
            size.as_int32()
        } else if size.is_number() {
            size.as_number() as i32
        } else {
            0
        };
        if length == 0 {
            ascii(out, label);
            ascii(out, " {}");
            return true;
        }
        ascii(out, &format!("{label}({length}) {{\n"));
        let mut entries = Vec::new();
        let cursor = Rc::new(Cell::new(0));
        match cell_registry::get(id) {
            Some(CellEntry::Map(map)) => {
                while let Some(entry) = map.table().borrow().next_entry(&cursor) {
                    entries.push(entry);
                }
            }
            Some(CellEntry::Set(set)) => {
                while let Some(entry) = set.table().borrow().next_entry(&cursor) {
                    entries.push(entry);
                }
            }
            _ => {}
        }
        self.indent += 1;
        self.depth += 1;
        self.seen.push(id);
        for (key, entry_value) in entries {
            self.pad(out);
            if !self.format(global_object, out, key) {
                return false;
            }
            if !is_set {
                ascii(out, ": ");
                if !self.format(global_object, out, entry_value) {
                    return false;
                }
            }
            self.comma(out);
            ascii(out, "\n");
        }
        self.seen.pop();
        self.depth -= 1;
        self.indent -= 1;
        self.pad(out);
        ascii(out, "}");
        true
    }

    /// `print_map_iterator_like`: `Nome { ` e, por item, `\n`, indentação, valor e vírgula; `entries` imprime o par como array.
    fn format_iterator(&mut self, global_object: &JSGlobalObject, out: &mut Vec<u16>, label: &str, items: Vec<IteratorStep>) -> bool {
        ascii(out, label);
        ascii(out, " { ");
        self.indent += 1;
        self.depth += 1;
        let count = items.len();
        for item in items {
            ascii(out, "\n");
            self.pad(out);
            let done = match item {
                IteratorStep::Item(item) => self.format(global_object, out, item),
                IteratorStep::Entry(key, entry_value) => {
                    self.depth += 1;
                    let pair = [key, entry_value];
                    let done = self.print_array(global_object, out, 2, &|index| Some(pair[index as usize]), &|_| true, None);
                    self.depth -= 1;
                    done
                }
                IteratorStep::Done => true,
            };
            if !done {
                return false;
            }
            self.comma(out);
        }
        self.depth -= 1;
        self.indent -= 1;
        if count > 0 {
            ascii(out, "\n");
        }
        self.pad(out);
        ascii(out, "}");
        true
    }
}

/// `!tag.is_primitive()` negado: qualquer valor que não é objeto, e as caixas de número, booleano e string.
/// O `options.stylize(text, style)` do inspect custom: o texto como veio sem cores; com `this.colors`, o par de códigos de
/// `util.inspect.styles` do estilo (`string` verde, `number` amarelo, `undefined` cinza, `null` negrito...).
/// Chama o `Symbol.for('nodejs.util.inspect.custom')` do objeto com `(remaining, options, undefined)`, onde `options`
/// leva `stylize`, `depth` (`option_depth`) e `colors`. `None` quando não há método chamável; `Some(None)` com exceção
/// pendente; `Some(Some(retorno))` com o que o método devolveu. Serve o `console.log` e o `util.inspect`.
pub(crate) fn call_custom_inspect(global_object: &JSGlobalObject, value: JSValue, remaining: JSValue, option_depth: JSValue, colors: bool) -> Option<Option<JSValue>> {
    let vm = global_object.vm();
    let registered = vm.symbol_registry().symbol_for_key(&StringImpl::create(b"nodejs.util.inspect.custom"));
    let symbol = Symbol::create_with_registered_uid(vm, &registered).to_primitive();
    let Some(identifier) = symbol.to_property_key(global_object) else { return Some(None) };
    let method = value.as_object().get(global_object, &PropertyName::from_identifier(&identifier));
    if has_exception(global_object) {
        return Some(None);
    }
    let call_data = get_call_data(method);
    if !method.is_object() || call_data.is_none() {
        return None;
    }
    let options = construct_empty_object(global_object);
    let stylize = JSFunction::create_native(
        vm,
        global_object,
        2,
        &WtfString::from_latin1(b"stylize"),
        stylize_option,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    let named = |name: &str| PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes()));
    options.put_direct(vm, &named("stylize"), stylize.as_value(), 0);
    options.put_direct(vm, &named("depth"), option_depth, 0);
    options.put_direct(vm, &named("colors"), js_boolean(colors), 0);
    let Ok(result) = call(global_object, method, &call_data, value, &[remaining, options.as_value(), js_undefined()]) else { return Some(None) };
    if has_exception(global_object) {
        return Some(None);
    }
    Some(Some(result))
}

fn stylize_option_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let text = call.argument(0).to_wtf_string();
    let this = call.this_value();
    let colors = this.is_object() && {
        let name = PropertyName::from_identifier(&Identifier::from_span(vm, b"colors"));
        this.as_object().get(global_object, &name).is_true()
    };
    let style = if call.argument(1).is_string() { wtf_string_to_units(&call.argument(1).to_wtf_string()) } else { Vec::new() };
    let pair = ["special:36:39", "number:33:39", "bigint:33:39", "boolean:33:39", "undefined:90:39", "null:1:22", "string:32:39", "symbol:32:39", "date:35:39", "regexp:31:39", "module:4:24"]
        .iter()
        .find_map(|entry| entry.split_once(':').filter(|(name, _)| same_text(&style, name)).map(|(_, codes)| codes));
    let mut out = Vec::new();
    let units = wtf_string_to_units(&text);
    match (colors, pair.and_then(|codes| codes.split_once(':'))) {
        (true, Some((open, close))) => {
            ascii(&mut out, &format!("\x1b[{open}m"));
            out.extend_from_slice(&units);
            ascii(&mut out, &format!("\x1b[{close}m"));
        }
        _ => out.extend_from_slice(&units),
    }
    Ok(JSValue::from_js_string(js_string(vm, &WtfString::from_utf16(&out))))
}

host_function!(stylize_option, stylize_option_body);

fn is_primitive_tag(value: JSValue) -> bool {
    if !value.is_object() {
        return true;
    }
    if value.is_callable() {
        return false;
    }
    match cell_registry::get(value.as_cell()) {
        // O `RegExp` sai como `Tag::String` (ver `format_builtin`), primitivo: `[ /a/ ]` numa linha (medido no bun 1.4.2).
        Some(CellEntry::StringObject(_) | CellEntry::RegExpObject(_)) => true,
        Some(CellEntry::WrapperObject(wrapper)) => matches!(wrapper.type_(), JSType::NumberObjectType | JSType::BooleanObjectType),
        _ => false,
    }
}

/// `JSValue::is_class` do bun: construtor de classe do JS, ou função nativa construtível (`Object`, `Map`...). A função
/// ligada nunca é classe.
/// `getter` é a função de leitura de um atributo nativo (`CustomAccessor`), criada por `put_native_accessor`.
fn is_custom_accessor_function(getter: JSValue) -> bool {
    getter.is_cell() && matches!(cell_registry::get(getter.as_cell()), Some(CellEntry::Function(function)) if function.custom_accessor.get().is_some())
}

pub(crate) fn is_class(value: JSValue) -> bool {
    if !value.is_callable() {
        return false;
    }
    let constructible = || !matches!(get_construct_data(value), CallData::None);
    match cell_registry::get(value.as_cell()) {
        Some(CellEntry::Function(function)) => {
            if function.as_bound_function().is_some() {
                false
            } else if function.is_host_function() {
                constructible()
            } else {
                let executable = function.js_executable();
                let executable = executable.borrow();
                executable.is_class() || executable.is_class_constructor_function()
            }
        }
        Some(CellEntry::InternalFunction(_)) => constructible(),
        _ => false,
    }
}

/// `getName` do bun: o nome calculado (`displayName`, nome do executável, `ecmaName`); a função ligada usa o do alvo.
pub(crate) fn callable_name(global_object: &JSGlobalObject, value: JSValue) -> Vec<u16> {
    let vm = global_object.vm();
    match cell_registry::get(value.as_cell()) {
        Some(CellEntry::Function(mut function)) => {
            loop {
                let target = function.as_bound_function().map(|bound| bound.target_function());
                match target {
                    None => break,
                    Some(target) => match target.as_js_function() {
                        Some(next) => function = next,
                        None => return Vec::new(),
                    },
                }
            }
            wtf_string_to_units(&function.calculated_display_name(vm, global_object))
        }
        Some(CellEntry::InternalFunction(function)) => wtf_string_to_units(&function.calculated_display_name(vm)),
        _ => Vec::new(),
    }
}

/// O nome do protótipo da função: `AsyncFunction`, `GeneratorFunction` ou `AsyncGeneratorFunction` (o `@@toStringTag`
/// do protótipo), vazio para `Function.prototype`, protótipo nulo ou qualquer outro objeto.
pub(crate) fn function_kind_name(global_object: &JSGlobalObject, proto: JSValue) -> Vec<u16> {
    if !proto.is_object() {
        return Vec::new();
    }
    let vm = global_object.vm();
    let tag = proto.as_object().get(global_object, &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol));
    if has_exception(global_object) || !tag.is_string() {
        return Vec::new();
    }
    let units = wtf_string_to_units(&tag.to_wtf_string());
    let known = ["AsyncFunction", "GeneratorFunction", "AsyncGeneratorFunction"];
    if known.iter().any(|name| name.bytes().map(u16::from).eq(units.iter().copied())) {
        units
    } else {
        Vec::new()
    }
}
