//! `TextEncoderStream` e `TextDecoderStream`. No bun/WebKit são `TransformStream` internos (um com o `TextEncoder`, o
//! outro com o `TextDecoder`), expostos por uma classe própria: o protótipo herda de `Object.prototype`, a instância
//! NÃO é um `TransformStream` (`instanceof TransformStream` é `false`), e `readable`/`writable` devolvem sempre as
//! mesmas pontas do `TransformStream` de dentro.
//!
//! O `TransformStream` de dentro é construído pelo construtor global de verdade (guardado na instalação) com um
//! `transformer` nativo (`transform` e `flush` como funções nativas): o estado do codificador (a unidade substituta alta
//! presa entre pedaços) e o do decodificador (`DecoderState` de [`crate::runtime::text_decoder`], com os bytes presos e o
//! BOM) vive aqui, numa tabela por objeto, e o `this` das duas funções é o `transformer`.
//!
//! Medido no bun 1.4.2: o codificador converte o pedaço com `ToString` (um símbolo lança `TypeError` sem `code`), prende
//! uma unidade alta no fim do pedaço até ver o seguinte (uma baixa a completa; qualquer outra faz sair U+FFFD) e o
//! `flush` solta um U+FFFD se sobrou uma; pedaço que não produz byte não enfileira nada. O decodificador exige
//! `BufferSource` (`ERR_INVALID_ARG_TYPE`), decodifica com `stream: true`, não enfileira texto vazio, e no `flush`
//! decodifica sem `stream` (o que sobrou vira U+FFFD, ou `ERR_ENCODING_INVALID_ENCODED_DATA` com `fatal`).
//!
//! O `Symbol(nodejs.util.inspect.custom)` monta `Nome { encoding, ... readable, writable }` como o `util.inspect` do bun
//! (ver [`inspect_text`]); o `util.inspect` em si não existe no porte, então `indentationLvl`, `colors` e `compact`
//! das opções não são lidos.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use super::readable::prototype_of;
use super::transform_js::{tc_enqueue, ts_readable, ts_writable};
use crate::host_function;
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::call_data::{construct, get_construct_data};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use super::trace::{mark_all, Traced};
use crate::runtime::intl_support::{get_property, to_rust_string};
use crate::runtime::structured_clone::is_object_cell;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::thrown_from_llint;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::describe_received;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{create_native_class_with_length, install_global_with_attributes, instance_structure, throw_coded_type_error};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_name::PropertyName;
use crate::runtime::text_decoder::{check_options, decode_chunk, decoder_from_arguments, input_bytes, invalid_encoded_data, text_value, DecoderState};
use crate::runtime::text_encoder::{string_units, uint8_array_from};
use crate::wtf::text::wtf_string::String as WtfString;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Encoder,
    Decoder,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Encoder => "TextEncoderStream",
            Kind::Decoder => "TextDecoderStream",
        }
    }
}

enum Role {
    /// A unidade substituta alta que fechou o pedaço anterior, à espera da baixa.
    Encoder { pending_high: Option<u16> },
    Decoder(DecoderState),
}

struct Entry {
    role: Role,
    /// O `TransformStream` de dentro.
    stream: JSValue,
}

impl Entry {
    fn kind(&self) -> Kind {
        match self.role {
            Role::Encoder { .. } => Kind::Encoder,
            Role::Decoder(_) => Kind::Decoder,
        }
    }
}

type EntryRef = Rc<RefCell<Entry>>;

thread_local! {
    /// A instância e o `transformer` nativo dela (o valor codificado da célula) com o estado.
    static OBJECTS: RefCell<HashMap<EncodedJSValue, EntryRef>> = RefCell::new(HashMap::new());
    /// O construtor global de `TransformStream`, guardado na instalação.
    static TRANSFORM_STREAM: Cell<Option<EncodedJSValue>> = const { Cell::new(None) };
}

impl Traced for Entry {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.stream.trace(out);
    }
}

/// As raízes do coletor: o construtor de `TransformStream`, as instâncias (as chaves vivem enquanto o programa
/// existir) e o `TransformStream` de dentro de cada uma.
pub(super) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    if let Some(constructor) = TRANSFORM_STREAM.with(Cell::get) {
        marker.encoded(constructor);
    }
    mark_all(marker, |out| {
        OBJECTS.with(|objects| {
            objects.borrow().iter().for_each(|(key, entry)| {
                out.push(JSValue::decode(*key));
                entry.trace(out);
            })
        })
    });
}

pub(super) fn reset_for_program() {
    let _ = OBJECTS.try_with(|objects| objects.borrow_mut().clear());
    let _ = TRANSFORM_STREAM.try_with(|constructor| constructor.set(None));
}

fn lookup(value: JSValue) -> Option<EntryRef> {
    OBJECTS.with(|objects| objects.borrow().get(&value.encode()).cloned())
}

fn entry_of(value: JSValue, kind: Kind) -> Option<EntryRef> {
    lookup(value).filter(|entry| entry.borrow().kind() == kind)
}

// ---------------------------------------------------------------------------------------------
// Conversão
// ---------------------------------------------------------------------------------------------

/// Os bytes UTF-8 de `units` com a unidade alta presa em `pending_high`: o que fecha o pedaço fica para o próximo.
fn encode_chunk(pending_high: &mut Option<u16>, units: &[u16]) -> Vec<u8> {
    let mut buffer: Vec<u16> = Vec::with_capacity(units.len() + 1);
    buffer.extend(pending_high.take());
    buffer.extend_from_slice(units);
    if let Some(&last) = buffer.last().filter(|unit| (0xD800..0xDC00).contains(*unit)) {
        *pending_high = Some(last);
        buffer.pop();
    }
    String::from_utf16_lossy(&buffer).into_bytes()
}

/// O que `transform` e `flush` enfileiram: bytes (codificador) ou texto (decodificador).
pub(super) enum Produced {
    Bytes(Vec<u8>),
    Text(String),
}

pub(super) fn enqueue(global_object: &JSGlobalObject, controller: JSValue, produced: Produced) -> Result<(), Thrown> {
    let value = match produced {
        Produced::Bytes(bytes) if bytes.is_empty() => return Ok(()),
        Produced::Text(text) if text.is_empty() => return Ok(()),
        Produced::Bytes(bytes) => uint8_array_from(global_object, &bytes)?,
        Produced::Text(text) => text_value(global_object, &text),
    };
    tc_enqueue(global_object, &HostCall::new(controller, vec![value])).map(|_| ())
}

pub(super) fn invalid_chunk(global_object: &JSGlobalObject, chunk: JSValue) -> Thrown {
    let received = describe_received(global_object, chunk).unwrap_or_else(|| "undefined".to_owned());
    throw_coded_type_error(global_object, &format!("The \"chunk\" argument must be of type BufferSource. Received {received}"), "ERR_INVALID_ARG_TYPE")
}

/// `transform(chunk, controller)` do `transformer` nativo.
fn transform_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(entry) = lookup(call.this_value()) else { return Ok(JSValue::undefined()) };
    let chunk = call.argument(0);
    let kind = entry.borrow().kind();
    // A conversão do pedaço roda antes de emprestar o estado: `ToString` pode chamar código do usuário.
    let produced = match kind {
        Kind::Encoder => {
            let units = string_units(global_object, chunk)?;
            match &mut entry.borrow_mut().role {
                Role::Encoder { pending_high } => Produced::Bytes(encode_chunk(pending_high, &units)),
                Role::Decoder(_) => return Ok(JSValue::undefined()),
            }
        }
        Kind::Decoder => {
            let bytes = input_bytes(chunk).ok_or_else(|| invalid_chunk(global_object, chunk))?;
            match &mut entry.borrow_mut().role {
                Role::Decoder(state) => Produced::Text(decode_chunk(state, &bytes, true).map_err(|()| invalid_encoded_data(global_object, state.encoding.name()))?),
                Role::Encoder { .. } => return Ok(JSValue::undefined()),
            }
        }
    };
    enqueue(global_object, call.argument(1), produced)?;
    Ok(JSValue::undefined())
}

/// `flush(controller)` do `transformer` nativo.
fn flush_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(entry) = lookup(call.this_value()) else { return Ok(JSValue::undefined()) };
    let produced = match &mut entry.borrow_mut().role {
        // Uma unidade alta que sobrou sai como U+FFFD.
        Role::Encoder { pending_high } => Produced::Bytes(pending_high.take().map_or_else(Vec::new, |_| "\u{FFFD}".as_bytes().to_vec())),
        Role::Decoder(state) => Produced::Text(decode_chunk(state, &[], false).map_err(|()| invalid_encoded_data(global_object, state.encoding.name()))?),
    };
    enqueue(global_object, call.argument(0), produced)?;
    Ok(JSValue::undefined())
}

host_function!(transform_native, transform_body);
host_function!(flush_native, flush_body);

// ---------------------------------------------------------------------------------------------
// Inspeção
// ---------------------------------------------------------------------------------------------

/// Uma opção numérica de `options` (`depth`, `breakLength`): `None` se ausente, `Some(None)` para `null` (sem limite),
/// `Some(Some(n))` para um número.
pub(crate) fn numeric_option(global_object: &JSGlobalObject, options: JSValue, name: &str) -> Result<Option<Option<f64>>, Thrown> {
    if !is_object_cell(&options) {
        return Ok(None);
    }
    let value = get_property(global_object, options, name)?;
    Ok(if value.is_number() {
        Some(Some(value.as_number()))
    } else if value.is_null() {
        Some(None)
    } else {
        None
    })
}

/// O texto das pontas do `TransformStream` (`ReadableStream { ... }`) quando o inspect delas ainda tem fôlego; abaixo
/// disso o `util.inspect` as mostra vazias (`ReadableStream {}`).
fn side_text(global_object: &JSGlobalObject, stream: JSValue, name: &str, side: fn(&JSGlobalObject, &HostCall) -> HostResult, depth: f64) -> Result<String, Thrown> {
    if depth < 0.0 {
        return Ok(format!("{name} {{}}"));
    }
    let end = side(global_object, &HostCall::new(stream, Vec::new()))?;
    let text = super::readable::inspect_text(global_object, end, false).or_else(|| super::writable_js::inspect_text(global_object, end, false));
    text.map_or_else(|| Ok(format!("{name} {{}}")), |text| to_rust_string(global_object, text))
}

/// O `Symbol(nodejs.util.inspect.custom)` das duas classes: `Nome { campos }` como o `util.inspect` do bun monta, com o
/// `depth` das opções menos um (sem `depth`, o padrão 2) e a quebra de linha em `breakLength` (80) do Node. `None` se
/// `value` não é uma instância.
pub(super) fn inspect_text(global_object: &JSGlobalObject, value: JSValue, options: JSValue) -> Option<Result<JSValue, Thrown>> {
    let entry = lookup(value)?;
    Some(inspect_entry(global_object, &entry, options).map(|text| text_value(global_object, &text)))
}

fn inspect_entry(global_object: &JSGlobalObject, entry: &EntryRef, options: JSValue) -> Result<String, Thrown> {
    let (kind, stream) = {
        let entry = entry.borrow();
        (entry.kind(), entry.stream)
    };
    let mut fields = Vec::new();
    match &entry.borrow().role {
        Role::Encoder { .. } => fields.push("encoding: 'utf-8'".to_owned()),
        Role::Decoder(state) => {
            fields.push(format!("encoding: '{}'", state.encoding.name()));
            fields.push(format!("fatal: {}", state.fatal));
            fields.push(format!("ignoreBOM: {}", state.ignore_bom));
        }
    }
    inspect_composite(global_object, kind.name(), stream, fields, Vec::new(), options)
}

/// `reduceToSingleString` do Node (`compact: 3`): numa linha só se couber em `breakLength` (a indentação conta).
fn fits_one_line(fields: &[String], indentation: f64, break_length: f64) -> bool {
    let count = fields.len() as f64;
    let total: f64 = fields.iter().map(|field| field.chars().count() as f64).sum::<f64>() + count + (count + 1.0 + 10.0) + indentation;
    total <= break_length && fields.iter().all(|field| !field.contains('\n'))
}

/// Reaplica a quebra de linha do `util.inspect` a um objeto aninhado (`Nome { a, b }`, indentação 2) que o texto das
/// pontas montou numa linha só; os campos (booleanos e nomes de estado) não têm vírgula dentro.
fn reduce_nested(text: &str, break_length: f64) -> String {
    let Some((name, inner)) = text.split_once(" { ").and_then(|(name, rest)| Some((name, rest.strip_suffix(" }")?))) else { return text.to_owned() };
    let fields: Vec<String> = inner.split(", ").map(str::to_owned).collect();
    if fits_one_line(&fields, 2.0, break_length) {
        text.to_owned()
    } else {
        format!("{name} {{\n    {}\n  }}", fields.join(",\n    "))
    }
}

/// `Nome { campos..., readable, writable }` como o `util.inspect` do bun monta para as classes que embrulham um
/// `TransformStream` (as de texto e as de compressão): `fields` são os campos antes das duas pontas.
pub(super) fn inspect_composite(global_object: &JSGlobalObject, name: &str, stream: JSValue, mut fields: Vec<String>, tail: Vec<String>, options: JSValue) -> Result<String, Thrown> {
    let depth = match numeric_option(global_object, options, "depth")? {
        None => 2.0,
        Some(None) => f64::INFINITY,
        Some(Some(depth)) => depth,
    } - 1.0;
    if depth < 0.0 {
        return Ok(format!("{name} [Object]"));
    }
    let break_length = match numeric_option(global_object, options, "breakLength")? {
        None => 80.0,
        Some(None) => f64::INFINITY,
        Some(Some(length)) => length,
    };
    let readable = reduce_nested(&side_text(global_object, stream, "ReadableStream", ts_readable, depth - 1.0)?, break_length);
    let writable = reduce_nested(&side_text(global_object, stream, "WritableStream", ts_writable, depth - 1.0)?, break_length);
    fields.push(format!("readable: {readable}"));
    fields.push(format!("writable: {writable}"));
    fields.extend(tail);
    Ok(if fits_one_line(&fields, 0.0, break_length) {
        format!("{name} {{ {} }}", fields.join(", "))
    } else {
        format!("{name} {{\n  {}\n}}", fields.join(",\n  "))
    })
}

// ---------------------------------------------------------------------------------------------
// Classes
// ---------------------------------------------------------------------------------------------

/// Constrói o `TransformStream` de dentro com o construtor global guardado na instalação.
pub(super) fn construct_inner_stream(global_object: &JSGlobalObject, transformer: JSValue) -> HostResult {
    let constructor = TRANSFORM_STREAM.with(Cell::get).map_or_else(JSValue::undefined, JSValue::decode);
    let data = get_construct_data(constructor);
    if data.is_none() {
        return Err(Thrown::type_error("TransformStream is not a constructor"));
    }
    construct(global_object, constructor, &data, &[transformer], constructor).map_err(thrown_from_llint)
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind) -> HostResult {
    let role = match kind {
        Kind::Encoder => Role::Encoder { pending_high: None },
        Kind::Decoder => {
            // Medido no bun: aqui o tipo de `options` é conferido antes do rótulo (o `TextDecoder` confere o rótulo antes).
            check_options(global_object, call.argument(1))?;
            Role::Decoder(decoder_from_arguments(global_object, call)?)
        }
    };
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let transformer = native_transformer(global_object, transform_native, flush_native);
    let stream = construct_inner_stream(global_object, transformer)?;
    let entry = Rc::new(RefCell::new(Entry { role, stream }));
    OBJECTS.with(|objects| {
        let mut objects = objects.borrow_mut();
        objects.insert(instance.encode(), entry.clone());
        objects.insert(transformer.encode(), entry);
    });
    Ok(instance)
}

/// O `transformer` nativo `{ transform, flush }` que o `TransformStream` de dentro recebe: o `this` das duas funções é
/// o próprio objeto devolvido.
pub(super) fn native_transformer(global_object: &JSGlobalObject, transform: NativeFunction, flush: NativeFunction) -> JSValue {
    let vm = global_object.vm();
    let transformer = JSFinalObject::create(vm, &global_object.object_structure_for_object_constructor());
    for (name, length, function) in [("transform", 2, transform), ("flush", 1, flush)] {
        let created = JSFunction::create_native(
            vm,
            global_object,
            length,
            &WtfString::from_latin1(name.as_bytes()),
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            call_host_function_as_constructor,
        );
        transformer.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes())), created.as_value(), 0);
    }
    transformer.as_value()
}

fn call_body(global_object: &JSGlobalObject, kind: Kind) -> HostResult {
    let name = kind.name();
    Err(throw_coded_type_error(global_object, &format!("Use `new {name}(...)` instead of `{name}(...)`"), "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn invalid_this(global_object: &JSGlobalObject, kind: Kind) -> Thrown {
    throw_coded_type_error(global_object, &format!("Value of \"this\" must be of type {}", kind.name()), "ERR_INVALID_THIS")
}

fn invalid_this_getter(global_object: &JSGlobalObject, kind: Kind, getter: &str) -> Thrown {
    let name = kind.name();
    throw_coded_type_error(global_object, &format!("Can only call {name}.{getter} on instances of {name}"), "ERR_INVALID_THIS")
}

/// `readable` e `writable`: as pontas do `TransformStream` de dentro.
fn side_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind, side: fn(&JSGlobalObject, &HostCall) -> HostResult) -> HostResult {
    let entry = entry_of(call.this_value(), kind).ok_or_else(|| invalid_this(global_object, kind))?;
    let stream = entry.borrow().stream;
    side(global_object, &HostCall::new(stream, Vec::new()))
}

/// `encoding`, `fatal` e `ignoreBOM`: `read` recebe o estado do decodificador (o do codificador é sempre `utf-8`).
fn property_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind, getter: &str, read: impl FnOnce(&JSGlobalObject, &Role) -> JSValue) -> HostResult {
    let entry = entry_of(call.this_value(), kind).ok_or_else(|| match kind {
        Kind::Encoder => invalid_this(global_object, kind),
        Kind::Decoder => invalid_this_getter(global_object, kind, getter),
    })?;
    let value = read(global_object, &entry.borrow().role);
    Ok(value)
}

fn encoding_of(global_object: &JSGlobalObject, role: &Role) -> JSValue {
    match role {
        Role::Encoder { .. } => text_value(global_object, "utf-8"),
        Role::Decoder(state) => text_value(global_object, state.encoding.name()),
    }
}

fn decoder_flag(role: &Role, read: impl FnOnce(&DecoderState) -> bool) -> JSValue {
    match role {
        Role::Decoder(state) => JSValue::Bool(read(state)),
        Role::Encoder { .. } => JSValue::undefined(),
    }
}

/// Gera o par `call`/`construct` e os getters de uma classe como funções nativas distintas.
macro_rules! class_functions {
    ($module:ident, $kind:expr) => {
        mod $module {
            use super::*;
            fn call_b(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
                call_body(global_object, $kind)
            }
            fn construct_b(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                construct_body(global_object, call, $kind)
            }
            host_function!(pub call, call_b);
            host_function!(pub construct, construct_b);
        }
    };
}

macro_rules! getters {
    ($( $id:ident => $body:expr ),* $(,)?) => {
        $(
            mod $id {
                use super::*;
                fn body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                    ($body)(global_object, call)
                }
                host_function!(pub native, body);
            }
        )*
    };
}

class_functions!(c_encoder, Kind::Encoder);
class_functions!(c_decoder, Kind::Decoder);

getters! {
    e_encoding => |g: &JSGlobalObject, c: &HostCall| property_body(g, c, Kind::Encoder, "encoding", encoding_of),
    e_readable => |g: &JSGlobalObject, c: &HostCall| side_body(g, c, Kind::Encoder, ts_readable),
    e_writable => |g: &JSGlobalObject, c: &HostCall| side_body(g, c, Kind::Encoder, ts_writable),
    d_encoding => |g: &JSGlobalObject, c: &HostCall| property_body(g, c, Kind::Decoder, "encoding", encoding_of),
    d_fatal => |g: &JSGlobalObject, c: &HostCall| property_body(g, c, Kind::Decoder, "fatal", |_, role| decoder_flag(role, |state| state.fatal)),
    d_ignore_bom => |g: &JSGlobalObject, c: &HostCall| property_body(g, c, Kind::Decoder, "ignoreBOM", |_, role| decoder_flag(role, |state| state.ignore_bom)),
    d_readable => |g: &JSGlobalObject, c: &HostCall| side_body(g, c, Kind::Decoder, ts_readable),
    d_writable => |g: &JSGlobalObject, c: &HostCall| side_body(g, c, Kind::Decoder, ts_writable),
}

static ENCODER_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "TextEncoderStream", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static DECODER_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "TextDecoderStream", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// Instala uma classe de stream embrulhada: construtor nativo de `length` argumentos, getters, inspect custom e
/// `@@toStringTag`, e o global.
pub(super) fn install_wrapped_class(
    global_object: &JSGlobalObject,
    name: &str,
    info: &'static ClassInfo,
    length: u32,
    call: NativeFunction,
    construct: NativeFunction,
    getters: &[(&str, NativeFunction)],
) {
    let vm = global_object.vm();
    let (prototype, constructor) = create_native_class_with_length(global_object, info, &CONSTRUCTOR_S_INFO, name, length, call, construct);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), crate::runtime::property_attribute::DONT_ENUM);
    for &(getter_name, getter) in getters {
        put_native_getter(vm, global_object, &prototype, getter_name, getter, Intrinsic::NoIntrinsic, 0);
    }
    super::put_inspect_custom(global_object, &prototype);
    put_to_string_tag(vm, &prototype, name);
    install_global_with_attributes(global_object, name, constructor.as_value(), 0);
}

fn install_class(global_object: &JSGlobalObject, kind: Kind, call: NativeFunction, construct: NativeFunction, getters: &[(&str, NativeFunction)]) {
    let info = if kind == Kind::Encoder { &ENCODER_PROTOTYPE_S_INFO } else { &DECODER_PROTOTYPE_S_INFO };
    install_wrapped_class(global_object, kind.name(), info, 0, call, construct, getters);
}

/// Instala `TextEncoderStream` e `TextDecoderStream` (depois das classes de stream: o construtor de `TransformStream` já existe).
pub(super) fn install(global_object: &JSGlobalObject) {
    let transform_stream = get_property(global_object, prototype_of("TransformStream"), "constructor").unwrap_or_else(|_| JSValue::undefined());
    TRANSFORM_STREAM.with(|constructor| constructor.set(Some(transform_stream.encode())));
    install_class(
        global_object,
        Kind::Encoder,
        c_encoder::call,
        c_encoder::construct,
        &[("encoding", e_encoding::native), ("readable", e_readable::native), ("writable", e_writable::native)],
    );
    install_class(
        global_object,
        Kind::Decoder,
        c_decoder::call,
        c_decoder::construct,
        &[
            ("encoding", d_encoding::native),
            ("fatal", d_fatal::native),
            ("ignoreBOM", d_ignore_bom::native),
            ("readable", d_readable::native),
            ("writable", d_writable::native),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(chunks: &[&str]) -> Vec<Vec<u8>> {
        let mut pending = None;
        chunks.iter().map(|chunk| encode_chunk(&mut pending, &chunk.encode_utf16().collect::<Vec<u16>>())).collect()
    }

    #[test]
    fn split_surrogate_pair_is_joined() {
        let units = [[0xD83Du16], [0xDE00]];
        let mut pending = None;
        assert_eq!(encode_chunk(&mut pending, &units[0]), Vec::<u8>::new());
        assert_eq!(encode_chunk(&mut pending, &units[1]), vec![0xF0, 0x9F, 0x98, 0x80]);
    }

    #[test]
    fn lone_high_then_other_becomes_replacement() {
        let mut pending = None;
        assert_eq!(encode_chunk(&mut pending, &[0xD83D]), Vec::<u8>::new());
        assert_eq!(encode_chunk(&mut pending, &[0x78]), vec![0xEF, 0xBF, 0xBD, 0x78]);
        assert_eq!(encode(&["a", ""]), vec![vec![0x61], vec![]]);
    }
}
