//! Streams web (`ReadableStream`, `WritableStream`, `TransformStream` e as classes ao redor). No bun 1.4.2 não são
//! builtins JS: são C++ nativo em `src/jsc/bindings/webcore/streams/`. Esta fatia é só a FORMA medida em
//! `tests/golden/streams_bun.tsv` (globais, `length`, `name`, chaves do construtor e do protótipo, descritores,
//! `Symbol.toStringTag`, `values === [Symbol.asyncIterator]`, erros do construtor ilegal e de `this` alheio).
//! `CountQueuingStrategy` e `ByteLengthQueuingStrategy` já vivem em `runtime::queuing_strategy`.
//!
//! O molde é o de `queuing_strategy`: construtor e protótipo nativos, `constructor` não enumerável, acessores
//! `get <nome>` enumeráveis e configuráveis, métodos graváveis, enumeráveis e configuráveis, `Symbol(nodejs.util
//! .inspect.custom)` e `@@toStringTag` por último. O brand check fica num `thread_local` (zerado em
//! `reset_for_program`).
//!
//! `ReadableStream` (fonte padrão), `ReadableStreamDefaultController` e `ReadableStreamDefaultReader` já fazem o
//! trabalho, em [`readable`] (fatia 3). `WritableStream`, `WritableStreamDefaultWriter` e
//! `WritableStreamDefaultController` também, em [`writable_js`] sobre o `Core` de [`writable`]. `TransformStream` e
//! `TransformStreamDefaultController` idem, em [`transform_js`] sobre o `Core` de [`transform`], com a arena de
//! promessas e a drenagem de [`effects`] dividas com o `WritableStream`.
//!
//! DIVERGÊNCIAS (todas até a fatia de comportamento):
//!
//! - o resto dos métodos e acessores não faz o trabalho: com `this` de outra classe lançam (ou rejeitam, onde o bun
//!   devolve promessa) o erro medido no bun, `Value of "this" must be of type X` com `ERR_INVALID_THIS`, e com `this`
//!   da classe lançam o mesmo erro;
//! - `new` do leitor BYOB lança o erro de argumento do bun mesmo com um stream válido;
//! - o `Symbol(nodejs.util.inspect.custom)` tem o texto do bun para `ReadableStream`, o controlador padrão, o
//!   `WritableStream`, `TextEncoderStream` e `TextDecoderStream` (medido); nos demais devolve `this` (o texto passa por
//!   `util.inspect`, ainda não portado);
//! - `ReadableStream.from` existe (length 1) e lança o erro de `this` alheio.

mod compression_streams;
mod body_stream;
pub(crate) use body_stream::{bytes_stream, errored_stream, is_readable_stream, read_stream_bytes, stream_used, text_stream, Chunks};
pub(crate) use readable::{lock_stream, tee_of};
mod bytes;
mod effects;
mod pipe;
mod readable;
mod text_streams;
mod trace;
pub(crate) use text_streams::numeric_option as text_numeric_option;
mod transform;
mod transform_js;
mod writable;
mod writable_js;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::body::{consume_stream, Read};
use self::bytes::ReaderKind;
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{create_native_class_with_length, install_global_with_attributes, instance_structure, throw_coded_type_error, throw_native_type_error};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::symbol::Symbol;
use crate::wtf::text::string_impl::StringImpl;
use crate::wtf::text::wtf_string::String as WtfString;

/// Cada classe de stream que o bun expõe como global (fora as duas estratégias de fila).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    ReadableStream,
    DefaultReader,
    ByobReader,
    DefaultController,
    ByteController,
    ByobRequest,
    WritableStream,
    Writer,
    WritableController,
    TransformStream,
    TransformController,
}

/// Como o construtor se comporta.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Construct {
    /// Cria a instância (`ReadableStream`, `WritableStream`, `TransformStream`).
    Instance,
    /// Leitor ou escritor: exige o stream como primeiro argumento (`... constructor requires a ... as its first argument`).
    StreamArgument(&'static str),
    /// `TypeError: Illegal constructor` com `ERR_ILLEGAL_CONSTRUCTOR`.
    Illegal,
}

const KINDS: [Kind; 11] = [
    Kind::ReadableStream,
    Kind::DefaultReader,
    Kind::ByobReader,
    Kind::DefaultController,
    Kind::ByteController,
    Kind::ByobRequest,
    Kind::WritableStream,
    Kind::Writer,
    Kind::WritableController,
    Kind::TransformStream,
    Kind::TransformController,
];

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::ReadableStream => "ReadableStream",
            Kind::DefaultReader => "ReadableStreamDefaultReader",
            Kind::ByobReader => "ReadableStreamBYOBReader",
            Kind::DefaultController => "ReadableStreamDefaultController",
            Kind::ByteController => "ReadableByteStreamController",
            Kind::ByobRequest => "ReadableStreamBYOBRequest",
            Kind::WritableStream => "WritableStream",
            Kind::Writer => "WritableStreamDefaultWriter",
            Kind::WritableController => "WritableStreamDefaultController",
            Kind::TransformStream => "TransformStream",
            Kind::TransformController => "TransformStreamDefaultController",
        }
    }

    fn construct(self) -> Construct {
        match self {
            Kind::ReadableStream | Kind::WritableStream | Kind::TransformStream => Construct::Instance,
            Kind::DefaultReader | Kind::ByobReader => Construct::StreamArgument("ReadableStream"),
            Kind::Writer => Construct::StreamArgument("WritableStream"),
            _ => Construct::Illegal,
        }
    }

    /// `length` do construtor, medido: os leitores e o escritor têm 1, o resto 0.
    fn constructor_length(self) -> u32 {
        u32::from(matches!(self.construct(), Construct::StreamArgument(_)))
    }
}

/// O que um método ou acessor faz enquanto o comportamento não existe.
#[derive(Clone, Copy)]
enum Mode {
    /// Lança `Value of "this" must be of type X`.
    Throw,
    /// Devolve uma promessa rejeitada com o mesmo erro.
    Reject,
    /// O comportamento de verdade, em [`readable`], [`writable_js`] e [`transform_js`].
    Real(fn(&JSGlobalObject, &HostCall) -> HostResult),
}

const fn info(class_name: &'static str, parent: &'static ClassInfo) -> ClassInfo {
    ClassInfo { class_name, parent_class: Some(parent), static_prop_hash_table: None, inherits_js_type_range: None }
}

static PROTOTYPE_INFOS: [ClassInfo; 11] = [
    info("ReadableStream", &JS_NON_FINAL_OBJECT_S_INFO),
    info("ReadableStreamDefaultReader", &JS_NON_FINAL_OBJECT_S_INFO),
    info("ReadableStreamBYOBReader", &JS_NON_FINAL_OBJECT_S_INFO),
    info("ReadableStreamDefaultController", &JS_NON_FINAL_OBJECT_S_INFO),
    info("ReadableByteStreamController", &JS_NON_FINAL_OBJECT_S_INFO),
    info("ReadableStreamBYOBRequest", &JS_NON_FINAL_OBJECT_S_INFO),
    info("WritableStream", &JS_NON_FINAL_OBJECT_S_INFO),
    info("WritableStreamDefaultWriter", &JS_NON_FINAL_OBJECT_S_INFO),
    info("WritableStreamDefaultController", &JS_NON_FINAL_OBJECT_S_INFO),
    info("TransformStream", &JS_NON_FINAL_OBJECT_S_INFO),
    info("TransformStreamDefaultController", &JS_NON_FINAL_OBJECT_S_INFO),
];

/// `const ClassInfo` do construtor (`"Function"`), igual nas classes.
static CONSTRUCTOR_S_INFO: ClassInfo = info("Function", &INTERNAL_FUNCTION_S_INFO);

thread_local! {
    /// As instâncias do programa (o valor codificado da célula) com a classe.
    static INSTANCES: RefCell<HashMap<EncodedJSValue, Kind>> = RefCell::new(HashMap::new());
}

/// `value` é uma instância criada pelo construtor da classe `kind` (o brand check do `pipeTo`/`pipeThrough`).
fn is_instance(value: JSValue, kind: Kind) -> bool {
    INSTANCES.with(|instances| instances.borrow().get(&value.encode()) == Some(&kind))
}

/// Fim do programa (`cell_registry::reset_program_state`): o que foi guardado é do programa.
/// As raízes do coletor: as instâncias (a chave do brand check) e os protótipos e construtores guardados; cada estado
/// de stream declara os valores JS que segura (`trace::Traced`), então nenhum deles adia a coleta. Streams, leitores,
/// controladores e transformadores ficam como raízes fortes até o fim do programa.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    INSTANCES.with(|instances| instances.borrow().keys().for_each(|instance| marker.encoded(*instance)));
    readable::visit_gc_roots(marker);
    writable_js::visit_gc_roots(marker);
    transform_js::visit_gc_roots(marker);
    text_streams::visit_gc_roots(marker);
    compression_streams::visit_gc_roots(marker);
}

/// As reações nativas são efêmeras da função da reação: o que a closure captura vive enquanto ela viver.
pub(crate) use self::readable::visit_side_table;

pub(crate) fn reset_for_program() {
    let _ = INSTANCES.try_with(|instances| instances.borrow_mut().clear());
    readable::reset_for_program();
    writable_js::reset_for_program();
    transform_js::reset_for_program();
    text_streams::reset_for_program();
    compression_streams::reset_for_program();
}

/// Chama `name` do `stream_iter.js` (o iterador assíncrono e o `ReadableStream.from`, escritos em JavaScript).
fn call_stream_module(global_object: &JSGlobalObject, name: &str, this_value: JSValue, arguments: &[JSValue]) -> HostResult {
    let exports = crate::api::builtin_modules::cached("internal:stream-iter", || {
        let host = crate::api::node_js_host::module_host(global_object, &[])?;
        Ok(crate::api::node_js_host::run_native_factory(global_object, include_str!("stream_iter.js"), "stream-iter", &host))
    })?;
    let function = crate::runtime::string_regexp_support::get_object_property(global_object, exports, &Identifier::from_span(global_object.vm(), name.as_bytes()))?;
    crate::api::node_js_host::call_value(global_object, function, this_value, arguments, "ReadableStream")
}

/// `ReadableStream.prototype.values`: o iterador assíncrono, sobre `getReader`.
fn values_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_stream_module(global_object, "values", call.this_value(), call.arguments())
}

/// `ReadableStream.from(iterável)`.
fn from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_stream_module(global_object, "from", call.this_value(), call.arguments())
}

/// Um objeto com `Symbol.asyncIterator` passado como corpo de `Response` ou `Request` vira o `ReadableStream` de
/// `ReadableStream.from` (medido no bun 1.4.2: o corpo é lido por stream); `None` para qualquer outro valor.
pub(crate) fn async_iterable_stream(global_object: &JSGlobalObject, value: JSValue) -> Result<Option<JSValue>, Thrown> {
    if !value.is_object() {
        return Ok(None);
    }
    let vm = global_object.vm();
    let method = crate::runtime::string_regexp_support::get_object_property(global_object, value, &vm.property_names.async_iterator_symbol)?;
    if method.is_undefined_or_null() {
        return Ok(None);
    }
    call_stream_module(global_object, "from", JSValue::undefined(), &[value]).map(Some)
}

/// `text()`, `bytes()`, `json()` e `blob()` do stream: lê tudo e resolve (ou rejeita) uma promessa, como o corpo de
/// `Response`; `this` alheio, stream travado e stream já lido rejeitam (medido no bun 1.4.2).
fn read_member(global_object: &JSGlobalObject, call: &HostCall, chunks: Chunks, read: Read) -> HostResult {
    let stream = call.this_value();
    if !is_readable_stream(stream) {
        let thrown = throw_invalid_this(global_object, Kind::ReadableStream);
        return Ok(reject_with_thrown(global_object, thrown));
    }
    if let Some(message) = readable::unavailable_reason(stream) {
        let thrown = throw_coded_type_error(global_object, message, "ERR_INVALID_STATE");
        return Ok(reject_with_thrown(global_object, thrown));
    }
    Ok(consume_stream(global_object, stream, chunks, read))
}

macro_rules! read_members {
    ($( $id:ident => ($chunks:expr, $read:expr) ),* $(,)?) => {
        $(
            fn $id(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                read_member(global_object, call, $chunks, $read)
            }
        )*
    };
}

read_members! {
    rs_text_body => (Chunks::Text, Read::Text),
    rs_bytes_body => (Chunks::Bytes, Read::Bytes),
    rs_json_body => (Chunks::Text, Read::StreamJson),
    rs_blob_body => (Chunks::Stringify, Read::Blob(Vec::new(), None)),
}

fn throw_invalid_this(global_object: &JSGlobalObject, kind: Kind) -> Thrown {
    throw_coded_type_error(global_object, &format!("Value of \"this\" must be of type {}", kind.name()), "ERR_INVALID_THIS")
}

/// Uma promessa rejeitada com o erro que `throw` acabou de lançar (o padrão de `body.rs`).
fn reject_with_thrown(global_object: &JSGlobalObject, _thrown: Thrown) -> JSValue {
    let vm = global_object.vm();
    let error = vm.exception().map_or_else(JSValue::undefined, |exception| exception.value());
    vm.clear_exception();
    JSPromise::rejected_promise(global_object, error).as_value()
}

fn member_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind, mode: Mode) -> HostResult {
    match mode {
        Mode::Real(body) => body(global_object, call),
        Mode::Reject => {
            let thrown = throw_invalid_this(global_object, kind);
            Ok(reject_with_thrown(global_object, thrown))
        }
        Mode::Throw => Err(throw_invalid_this(global_object, kind)),
    }
}

/// `options.depth` numérico menor ou igual a zero: o inspect custom nativo do bun devolve só `Nome [Object]` (medido
/// no 1.4.2 para os streams, as estratégias de fila e o `PerformanceMark`). Sem `depth`, `null` ou não numérico, não.
pub(crate) fn options_depth_exhausted(global_object: &JSGlobalObject, options: JSValue) -> Result<bool, Thrown> {
    Ok(matches!(text_streams::numeric_option(global_object, options, "depth")?, Some(Some(depth)) if depth <= 0.0))
}

/// `[Symbol.for('nodejs.util.inspect.custom')]`: com `depth` negativo devolve `this`; o texto do bun existe para
/// `ReadableStream`, o controlador padrão e o `WritableStream` (ver o cabeçalho), o resto devolve `this`.
fn inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = call.this_value();
    let depth = call.argument(0);
    if depth.is_number() && depth.as_number() < 0.0 {
        return Ok(this_value);
    }
    let exhausted = options_depth_exhausted(global_object, call.argument(1))?;
    if let Some(text) = readable::inspect_text(global_object, this_value, exhausted) {
        return Ok(text);
    }
    if let Some(text) = writable_js::inspect_text(global_object, this_value, exhausted) {
        return Ok(text);
    }
    if let Some(text) = transform_js::inspect_text(global_object, this_value, call.argument(1)) {
        return text;
    }
    if let Some(text) = text_streams::inspect_text(global_object, this_value, call.argument(1)) {
        return text;
    }
    if let Some(text) = compression_streams::inspect_text(global_object, this_value, call.argument(1)) {
        return text;
    }
    Ok(this_value)
}
host_function!(inspect_native, inspect_body);

fn call_body(global_object: &JSGlobalObject, kind: Kind) -> HostResult {
    let name = kind.name();
    match kind.construct() {
        Construct::Illegal => Err(throw_coded_type_error(global_object, "Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR")),
        _ => Err(throw_coded_type_error(global_object, &format!("Use `new {name}(...)` instead of `{name}(...)`"), "ERR_ILLEGAL_CONSTRUCTOR")),
    }
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind) -> HostResult {
    if kind == Kind::ReadableStream {
        return readable::construct_stream(global_object, call);
    }
    if kind == Kind::WritableStream {
        return writable_js::construct_stream(global_object, call);
    }
    if kind == Kind::TransformStream {
        return transform_js::construct_stream(global_object, call);
    }
    if kind == Kind::Writer {
        if let Some(result) = writable_js::construct_writer(global_object, call) {
            return result;
        }
    }
    if kind == Kind::DefaultReader {
        if let Some(result) = readable::construct_reader(global_object, call, ReaderKind::Default) {
            return result;
        }
    }
    if kind == Kind::ByobReader {
        if let Some(result) = readable::construct_reader(global_object, call, ReaderKind::Byob) {
            return result;
        }
    }
    match kind.construct() {
        Construct::Illegal => Err(throw_coded_type_error(global_object, "Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR")),
        Construct::StreamArgument(stream) => {
            Err(throw_native_type_error(global_object, &format!("{} constructor requires a {stream} as its first argument", kind.name())))
        }
        Construct::Instance => {
            let structure = derived_structure(global_object, call, instance_structure)?;
            let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
            INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode(), kind));
            Ok(instance)
        }
    }
}

/// Gera `call`, `construct` e os membros de uma classe como funções nativas distintas.
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

macro_rules! members {
    ($( $id:ident => ($kind:expr, $mode:expr) ),* $(,)?) => {
        $(
            mod $id {
                use super::*;
                fn body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                    member_body(global_object, call, $kind, $mode)
                }
                host_function!(pub native, body);
            }
        )*
    };
}

class_functions!(c_readable_stream, Kind::ReadableStream);
class_functions!(c_default_reader, Kind::DefaultReader);
class_functions!(c_byob_reader, Kind::ByobReader);
class_functions!(c_default_controller, Kind::DefaultController);
class_functions!(c_byte_controller, Kind::ByteController);
class_functions!(c_byob_request, Kind::ByobRequest);
class_functions!(c_writable_stream, Kind::WritableStream);
class_functions!(c_writer, Kind::Writer);
class_functions!(c_writable_controller, Kind::WritableController);
class_functions!(c_transform_stream, Kind::TransformStream);
class_functions!(c_transform_controller, Kind::TransformController);

members! {
    rs_locked => (Kind::ReadableStream, Mode::Real(readable::rs_locked)),
    rs_cancel => (Kind::ReadableStream, Mode::Real(readable::rs_cancel)),
    rs_get_reader => (Kind::ReadableStream, Mode::Real(readable::rs_get_reader)),
    rs_pipe_through => (Kind::ReadableStream, Mode::Real(pipe::rs_pipe_through)),
    rs_pipe_to => (Kind::ReadableStream, Mode::Real(pipe::rs_pipe_to)),
    rs_tee => (Kind::ReadableStream, Mode::Real(readable::rs_tee)),
    rs_values => (Kind::ReadableStream, Mode::Real(values_body)),
    rs_blob => (Kind::ReadableStream, Mode::Real(rs_blob_body)),
    rs_bytes => (Kind::ReadableStream, Mode::Real(rs_bytes_body)),
    rs_json => (Kind::ReadableStream, Mode::Real(rs_json_body)),
    rs_text => (Kind::ReadableStream, Mode::Real(rs_text_body)),
    rs_from => (Kind::ReadableStream, Mode::Real(from_body)),
    dr_closed => (Kind::DefaultReader, Mode::Real(readable::dr_closed)),
    dr_cancel => (Kind::DefaultReader, Mode::Real(readable::dr_cancel)),
    dr_read => (Kind::DefaultReader, Mode::Real(readable::dr_read)),
    dr_read_many => (Kind::DefaultReader, Mode::Reject),
    dr_release_lock => (Kind::DefaultReader, Mode::Real(readable::dr_release_lock)),
    br_closed => (Kind::ByobReader, Mode::Real(readable::br_closed)),
    br_cancel => (Kind::ByobReader, Mode::Real(readable::br_cancel)),
    br_read => (Kind::ByobReader, Mode::Real(readable::br_read)),
    br_release_lock => (Kind::ByobReader, Mode::Real(readable::br_release_lock)),
    dc_desired_size => (Kind::DefaultController, Mode::Real(readable::dc_desired_size)),
    dc_close => (Kind::DefaultController, Mode::Real(readable::dc_close)),
    dc_enqueue => (Kind::DefaultController, Mode::Real(readable::dc_enqueue)),
    dc_error => (Kind::DefaultController, Mode::Real(readable::dc_error)),
    bc_byob_request => (Kind::ByteController, Mode::Real(readable::bc_byob_request)),
    bc_desired_size => (Kind::ByteController, Mode::Real(readable::bc_desired_size)),
    bc_close => (Kind::ByteController, Mode::Real(readable::bc_close)),
    bc_enqueue => (Kind::ByteController, Mode::Real(readable::bc_enqueue)),
    bc_error => (Kind::ByteController, Mode::Real(readable::bc_error)),
    bq_view => (Kind::ByobRequest, Mode::Real(readable::bq_view)),
    bq_respond => (Kind::ByobRequest, Mode::Real(readable::bq_respond)),
    bq_respond_with_new_view => (Kind::ByobRequest, Mode::Real(readable::bq_respond_with_new_view)),
    ws_locked => (Kind::WritableStream, Mode::Real(writable_js::ws_locked)),
    ws_abort => (Kind::WritableStream, Mode::Real(writable_js::ws_abort)),
    ws_close => (Kind::WritableStream, Mode::Real(writable_js::ws_close)),
    ws_get_writer => (Kind::WritableStream, Mode::Real(writable_js::ws_get_writer)),
    wr_closed => (Kind::Writer, Mode::Real(writable_js::wr_closed)),
    wr_desired_size => (Kind::Writer, Mode::Real(writable_js::wr_desired_size)),
    wr_ready => (Kind::Writer, Mode::Real(writable_js::wr_ready)),
    wr_abort => (Kind::Writer, Mode::Real(writable_js::wr_abort)),
    wr_close => (Kind::Writer, Mode::Real(writable_js::wr_close)),
    wr_release_lock => (Kind::Writer, Mode::Real(writable_js::wr_release_lock)),
    wr_write => (Kind::Writer, Mode::Real(writable_js::wr_write)),
    wc_signal => (Kind::WritableController, Mode::Real(writable_js::wc_signal)),
    wc_error => (Kind::WritableController, Mode::Real(writable_js::wc_error)),
    ts_readable => (Kind::TransformStream, Mode::Real(transform_js::ts_readable)),
    ts_writable => (Kind::TransformStream, Mode::Real(transform_js::ts_writable)),
    tc_desired_size => (Kind::TransformController, Mode::Real(transform_js::tc_desired_size)),
    tc_enqueue => (Kind::TransformController, Mode::Real(transform_js::tc_enqueue)),
    tc_error => (Kind::TransformController, Mode::Real(transform_js::tc_error)),
    tc_terminate => (Kind::TransformController, Mode::Real(transform_js::tc_terminate)),
}

/// Um membro do protótipo: acessor `get <nome>` ou método de `length` dado.
enum Member {
    Getter(&'static str, NativeFunction),
    Method(&'static str, u32, NativeFunction),
}

use Member::{Getter, Method};

/// Os construtores e os membros de cada classe, na ordem medida em `Reflect.ownKeys(X.prototype)`.
fn class_table(kind: Kind) -> (NativeFunction, NativeFunction, Vec<Member>) {
    match kind {
        Kind::ReadableStream => (
            c_readable_stream::call,
            c_readable_stream::construct,
            vec![
                Getter("locked", rs_locked::native),
                Method("cancel", 0, rs_cancel::native),
                Method("getReader", 0, rs_get_reader::native),
                Method("pipeThrough", 1, rs_pipe_through::native),
                Method("pipeTo", 1, rs_pipe_to::native),
                Method("tee", 0, rs_tee::native),
                Method("values", 0, rs_values::native),
                Method("blob", 0, rs_blob::native),
                Method("bytes", 0, rs_bytes::native),
                Method("json", 0, rs_json::native),
                Method("text", 0, rs_text::native),
            ],
        ),
        Kind::DefaultReader => (
            c_default_reader::call,
            c_default_reader::construct,
            vec![
                Getter("closed", dr_closed::native),
                Method("cancel", 0, dr_cancel::native),
                Method("read", 0, dr_read::native),
                Method("readMany", 0, dr_read_many::native),
                Method("releaseLock", 0, dr_release_lock::native),
            ],
        ),
        Kind::ByobReader => (
            c_byob_reader::call,
            c_byob_reader::construct,
            vec![
                Getter("closed", br_closed::native),
                Method("cancel", 0, br_cancel::native),
                Method("read", 1, br_read::native),
                Method("releaseLock", 0, br_release_lock::native),
            ],
        ),
        Kind::DefaultController => (
            c_default_controller::call,
            c_default_controller::construct,
            vec![
                Getter("desiredSize", dc_desired_size::native),
                Method("close", 0, dc_close::native),
                Method("enqueue", 0, dc_enqueue::native),
                Method("error", 0, dc_error::native),
            ],
        ),
        Kind::ByteController => (
            c_byte_controller::call,
            c_byte_controller::construct,
            vec![
                Getter("byobRequest", bc_byob_request::native),
                Getter("desiredSize", bc_desired_size::native),
                Method("close", 0, bc_close::native),
                Method("enqueue", 1, bc_enqueue::native),
                Method("error", 0, bc_error::native),
            ],
        ),
        Kind::ByobRequest => (
            c_byob_request::call,
            c_byob_request::construct,
            vec![
                Getter("view", bq_view::native),
                Method("respond", 1, bq_respond::native),
                Method("respondWithNewView", 1, bq_respond_with_new_view::native),
            ],
        ),
        Kind::WritableStream => (
            c_writable_stream::call,
            c_writable_stream::construct,
            vec![
                Getter("locked", ws_locked::native),
                Method("abort", 0, ws_abort::native),
                Method("close", 0, ws_close::native),
                Method("getWriter", 0, ws_get_writer::native),
            ],
        ),
        Kind::Writer => (
            c_writer::call,
            c_writer::construct,
            vec![
                Getter("closed", wr_closed::native),
                Getter("desiredSize", wr_desired_size::native),
                Getter("ready", wr_ready::native),
                Method("abort", 0, wr_abort::native),
                Method("close", 0, wr_close::native),
                Method("releaseLock", 0, wr_release_lock::native),
                Method("write", 0, wr_write::native),
            ],
        ),
        Kind::WritableController => (
            c_writable_controller::call,
            c_writable_controller::construct,
            vec![Getter("signal", wc_signal::native), Method("error", 0, wc_error::native)],
        ),
        Kind::TransformStream => (
            c_transform_stream::call,
            c_transform_stream::construct,
            vec![Getter("readable", ts_readable::native), Getter("writable", ts_writable::native)],
        ),
        Kind::TransformController => (
            c_transform_controller::call,
            c_transform_controller::construct,
            vec![
                Getter("desiredSize", tc_desired_size::native),
                Method("enqueue", 0, tc_enqueue::native),
                Method("error", 0, tc_error::native),
                Method("terminate", 0, tc_terminate::native),
            ],
        ),
    }
}

/// Instala os globais de stream. A ordem das chaves do `globalThis` já é fixada pelo `ORDER` de `js_global_object_init`.
pub fn install_streams(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    for (index, kind) in KINDS.into_iter().enumerate() {
        let (call, construct, members) = class_table(kind);
        let (prototype, constructor) =
            create_native_class_with_length(global_object, &PROTOTYPE_INFOS[index], &CONSTRUCTOR_S_INFO, kind.name(), kind.constructor_length(), call, construct);
        prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
        readable::register_prototype(kind.name(), prototype.as_value());
        let mut values_function = None;
        for member in members {
            match member {
                Getter(name, getter) => put_native_getter(vm, global_object, &prototype, name, getter, Intrinsic::NoIntrinsic, 0),
                Method(name, length, function) => {
                    let created = put_direct_native_function_without_transition(
                        vm,
                        global_object,
                        &prototype,
                        &Identifier::from_span(vm, name.as_bytes()),
                        length,
                        function,
                        ImplementationVisibility::Public,
                        Intrinsic::NoIntrinsic,
                        0,
                    );
                    if kind == Kind::ReadableStream && name == "values" {
                        values_function = Some(created.as_value());
                    }
                }
            }
        }
        if let Some(values) = values_function {
            prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.async_iterator_symbol), values, DONT_ENUM);
        }
        put_inspect_custom(global_object, &prototype);
        put_to_string_tag(vm, &prototype, kind.name());
        if kind == Kind::ReadableStream {
            put_direct_native_function_without_transition(
                vm,
                global_object,
                &constructor,
                &Identifier::from_span(vm, b"from"),
                1,
                rs_from::native,
                ImplementationVisibility::Public,
                Intrinsic::NoIntrinsic,
                0,
            );
        }
        install_global_with_attributes(global_object, kind.name(), constructor.as_value(), DONT_ENUM);
    }
    text_streams::install(global_object);
    compression_streams::install(global_object);
}

/// `[Symbol.for('nodejs.util.inspect.custom')]` (`anonymous`, length 2, não enumerável) no protótipo `prototype`.
fn put_inspect_custom(global_object: &JSGlobalObject, prototype: &crate::runtime::js_object::JSObject) {
    put_inspect_custom_with(global_object, prototype, inspect_native);
}

/// O mesmo símbolo com um corpo nativo próprio (`BroadcastChannel`, `PerformanceMark`...).
pub(crate) fn put_inspect_custom_with(global_object: &JSGlobalObject, prototype: &crate::runtime::js_object::JSObject, body: NativeFunction) {
    put_inspect_custom_named(global_object, prototype, "anonymous", body);
}

/// O mesmo símbolo com o `name` da função escolhido (`PerformanceEntry` usa `[nodejs.util.inspect.custom]`, medido).
pub(crate) fn put_inspect_custom_named(global_object: &JSGlobalObject, prototype: &crate::runtime::js_object::JSObject, name: &str, body: NativeFunction) {
    let vm = global_object.vm();
    let inspect_function = JSFunction::create_native(
        vm,
        global_object,
        2,
        &WtfString::from_latin1(name.as_bytes()),
        body,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    let registered = vm.symbol_registry().symbol_for_key(&StringImpl::create(b"nodejs.util.inspect.custom"));
    let inspect_symbol = Symbol::create_with_registered_uid(vm, &registered);
    prototype.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_private_name(&inspect_symbol.private_name())), inspect_function.as_value(), DONT_ENUM);
}
