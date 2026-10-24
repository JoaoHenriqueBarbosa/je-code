//! `process.stdout`, `process.stderr` e `process.stdin` do bun 1.4.2, medidos com a saída em pipe (sem TTY).
//!
//! Forma medida de `stdout`/`stderr`: `WriteStream` (`constructor.name`) com as chaves próprias, nesta ordem, `fd`,
//! `_writev`, `flush`, `start`, `pos`, `bytesWritten`, `_write`, `write`, `_construct`, `_events`, `_writableState`,
//! `_maxListeners`, `readable` (`false`), `_type` (`"fs"`), `destroySoon`, `_destroy`, `_final` e `_isStdio` (`true`),
//! todas enumeráveis, graváveis e configuráveis. A cadeia de protótipos é `WriteStream.prototype` (`open`, `close`,
//! `_construct`, `_write`, `_writev`, `_destroy`, `destroySoon`, `autoClose`, `pending`), `Writable.prototype`
//! (`pipe`, `write`, `cork`, `uncork`, `setDefaultEncoding`, `_write`, `_writev`, `end` e os acessores `closed`,
//! `destroyed`, `writable`, ...), `Stream.prototype` (`constructor`, `pipe`, `eventNames`), `EventEmitter.prototype` e
//! `Object.prototype`; os protótipos são compartilhados entre `stdout` e `stderr`. `stdin` é um `ReadStream` com
//! `fd` 0 sobre `Readable` -> `Stream` -> `EventEmitter`.
//!
//! `write(chunk, encoding?, callback?)` se chama `writeFast` (`_write` é `underscoreWriteFast`), `length` 3: string (o
//! `encoding` é ignorado, sai em UTF-8), `Uint8Array`/`ArrayBufferView` ou `ArrayBuffer`; devolve `true`; o callback
//! (o segundo ou o terceiro argumento, o que for função) roda SINCRONAMENTE logo depois da escrita. `undefined` e
//! `null` lançam `ERR_STREAM_NULL_VALUES`, qualquer outro tipo `ERR_INVALID_ARG_TYPE`. A escrita vai ao console host do
//! global, então a ordem em relação ao `console.log` é a da chamada.
//!
//! `cork`/`uncork` contam em `writableCorked` (2 e 1 após dois `cork` e um `uncork`), `setDefaultEncoding` valida o nome
//! (`ERR_UNKNOWN_ENCODING`, `Unknown encoding: x`) e devolve `this`. `end(chunk?, encoding?, callback?)` escreve o
//! pedaço, marca `writableEnded` (e `writable` vira `false`) e devolve `this`. Os ouvintes (`on`, `once`, `off`,
//! `emit`, `listenerCount`, `listeners`, `eventNames`) usam uma `ListenerTable` por descritor.
//!
//! `end` agenda no `nextTick` o callback e o evento `finish` (`writableFinished` vira `true` antes do callback); `write`
//! após `end` devolve `false` e entrega `ERR_STREAM_WRITE_AFTER_END` (`write after end`) assíncrono ao callback e ao
//! evento `error` (sem ouvinte fica sem captura). `pipe` lança `ERR_STREAM_CANNOT_PIPE`; `_construct` chama o callback na
//! hora, `close` chama no tick, `_writev` escreve a lista; `writableBuffer` é `[]`; `destroy` devolve `this`.
//!
//! `_events` e `_eventsCount` refletem os ouvintes reais (um ouvinte é a função, o `once` é um `onceWrapper` com
//! `listener`, vários viram array; `removeAllListeners()` esvazia até as chaves-base), `newListener` é emitido antes do
//! registro, `setMaxListeners` grava `_maxListeners`. `Stream()`/`EventEmitter()` sem `new` lançam o `TypeError` do
//! `this` indefinido, com `new` devolvem `_events`, `_eventsCount`, `_maxListeners`; `Writable()` devolve `_events`,
//! `_writableState`, `_maxListeners`; `WriteStream()` lança `ERR_INVALID_ARG_TYPE` do `path`; `onwrite()` sem argumento
//! lança `ERR_MULTIPLE_CALLBACK`. O callback de `end` recebe `null`; um segundo `end` devolve `this`, o callback antes do
//! `finish` roda antes do evento, depois dele recebe `ERR_STREAM_ALREADY_FINISHED`.
//!
//! Cada stream tem um `Slot` (ouvintes, estado de escrita, fluxo, limite de ouvintes): 0, 1 e 2 são o stdin, o stdout e
//! o stderr; `new Stream()`, `Writable()`, `Readable()`, `WriteStream(path)` e `ReadStream(path)` ganham um `Slot`
//! próprio, achado pelo `encode` do valor, então `st.on('x')` não toca o stdout (`_eventsCount` 1, `["x"]`).
//!
//! stdin: lê o resto do console host em bytes (`read_stdin_rest`, inclusive a última linha sem `\n`) no primeiro
//! `nextTick` depois de `data`/`end`/`close`/`readable`. `isPaused()` e `readableFlowing` seguem o estado medido
//! (início `false`/`null`; `data` `true`/`false`; `pause()` `false`/`true`; `resume()` volta; `readable` trava em
//! `false`/`true`). O `kConstruct` (o quinto de `_eventsCount`) sai num microtask enfileirado na criação do
//! `process.stdin`, depois dos `nextTick` e antes do primeiro microtask do usuário seguinte (5 vira 4 sem ouvinte,
//! 6 vira 5 com `data`).
//!
//! LACUNAS (relato): `WriteStream('/arq')`/`ReadStream('/arq')` montam as chaves medidas mas não abrem o arquivo (e só
//! aceitam texto, não `URL`); o `Readable()` devolve só as três chaves (sem `push`/`read` reais); no `stdin`, `data`
//! sem `setEncoding` entrega `Buffer` em pedaços de 65536 bytes (pipe, medido); com `setEncoding` os mesmos pedaços
//! passam por um `StringDecoder` (`string_decoder.rs`) que guarda o resto de um caractere cortado e o solta antes do
//! `end`; `setEncoding` no meio do fluxo vale do pedaço seguinte, com decodificador novo. Os quatro ouvintes internos de `_events` são no-ops
//! (medido: chamados à mão devolvem `undefined` e não mexem em `flowing`, `isPaused`, `destroyed` nem `_eventsCount`).

use std::cell::{Cell, RefCell};
use std::thread::LocalKey;

use crate::host_function;
use crate::runtime::blob::string_bytes;
use crate::runtime::event_emitter_core::{call_listeners, ListenerTable};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_support::{get_property, prop};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::js_web_assembly::{received_description, thrown_to_value};
use crate::runtime::microtask::InternalMicrotask;
use crate::runtime::microtask_queue::QueuedTask;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::node_error::{throw_coded_error, throw_coded_type_error};
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::process_exit::{event_key, event_value, rethrow};
use crate::runtime::process_object::queue_tick;
use crate::runtime::process_shape::{array_value, text_value, undefined_function};
use crate::runtime::property_attribute::{ACCESSOR, DONT_DELETE, DONT_ENUM};
use crate::runtime::js_string::js_string;
use crate::runtime::node_buffer::Encoding;
use crate::runtime::string_decoder::StringDecoder;
use crate::runtime::text_decoder::input_bytes;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::runtime::timers::{native, native_function_with_constructor, put_accessor_with};

const WRITE_MESSAGE: &str = "write() expects a string, ArrayBufferView, or ArrayBuffer";

/// Os nomes de codificação que `setDefaultEncoding` aceita (os de `Buffer.isEncoding`, sem diferenciar caixa).
const ENCODINGS: &[&str] = &[
    "utf8", "utf-8", "hex", "base64", "base64url", "ascii", "latin1", "binary", "ucs2", "ucs-2", "utf16le", "utf-16le",
];

/// O estado mutável de um stream de escrita.
#[derive(Clone, Copy, Default)]
struct WriteState {
    corked: u32,
    ended: bool,
    finished: bool,
}

/// O estado de fluxo de um stream de leitura (`readableFlowing` e `isPaused`). Medido no stdin: início `null`/`false`;
/// `on('data')` dá `true`/`false`; `pause()` dá `false`/`true`; `resume()` volta a `true`/`false`; `on('readable')` dá
/// `false`/`true` e daí `pause()`/`resume()` não mudam mais nada; `pause()` seguido de `on('data')` fica `false`/`true`.
#[derive(Clone, Copy, Default)]
struct FlowState {
    flowing: Option<bool>,
    paused: bool,
    readable_listening: bool,
}

/// Tudo o que um stream guarda por identidade: os ouvintes e os estados que os descritores 0, 1 e 2 já tinham.
#[derive(Default)]
struct Slot {
    listeners: ListenerTable,
    write: WriteState,
    flow: FlowState,
    /// O `setMaxListeners` (`None` até o primeiro; o padrão do `EventEmitter` é 10).
    max_listeners: Option<i32>,
    /// Os callbacks de `end` chamados antes do `finish` (rodam logo antes do evento).
    pending_end: Vec<JSValue>,
    /// `removeAllListeners()` sem argumento esvazia também as chaves-base de `_events` (medido: `{}`).
    events_cleared: bool,
    /// Os nomes de evento já gravados em `_events` (os que saem ficam `undefined`, como no bun).
    synced_names: Vec<String>,
}

/// Os três primeiros `Slot` são o stdin, o stdout e o stderr; os seguintes são dos objetos de `new Stream()`,
/// `Writable()`, `Readable()`, `WriteStream(path)` e `ReadStream(path)`.
const STDIO_SLOTS: usize = 3;

/// As raízes do coletor: os protótipos, o objeto do stdin, os objetos dos fluxos, os ouvintes e os callbacks de `end`
/// de cada `Slot` (o resto do estado dos fluxos não guarda valores).
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    for prototype in [&READABLE_PROTOTYPE, &WRITABLE_PROTOTYPE, &STREAM_PROTOTYPE, &EMITTER_PROTOTYPE, &WRITE_PROTOTYPE, &READ_PROTOTYPE, &STDIN_OBJECT] {
        marker.value(prototype.with(Cell::get));
    }
    OBJECT_SLOTS.with(|slots| slots.borrow().iter().for_each(|(_, cell_id)| marker.cell(*cell_id)));
    SLOTS.with(|slots| {
        for slot in slots.borrow().iter() {
            slot.listeners.visit_gc_roots(marker);
            marker.values(&slot.pending_end);
        }
    });
}

thread_local! {
    static SLOTS: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
    /// A identidade de cada objeto que não é stdio (o `encode` do valor) e o seu `Slot`.
    static OBJECT_SLOTS: RefCell<Vec<(i64, usize)>> = const { RefCell::new(Vec::new()) };
    /// `Readable.prototype`, para o construtor `Readable`.
    static READABLE_PROTOTYPE: Cell<Option<JSValue>> = const { Cell::new(None) };
    /// `Writable.prototype`, `Stream.prototype` e `EventEmitter.prototype`, para os construtores.
    static WRITABLE_PROTOTYPE: Cell<Option<JSValue>> = const { Cell::new(None) };
    static STREAM_PROTOTYPE: Cell<Option<JSValue>> = const { Cell::new(None) };
    static EMITTER_PROTOTYPE: Cell<Option<JSValue>> = const { Cell::new(None) };
    /// Os protótipos compartilhados: `WriteStream.prototype` e `ReadStream.prototype`.
    static WRITE_PROTOTYPE: Cell<Option<JSValue>> = const { Cell::new(None) };
    static READ_PROTOTYPE: Cell<Option<JSValue>> = const { Cell::new(None) };
}

/// Esquece o estado do programa anterior: os protótipos, os `Slot` e o fluxo do stdin guardam valores de um global que
/// já caiu (o próximo `process` refaria o objeto com o protótipo velho).
pub(crate) fn reset_for_program() {
    // `try_with`: o reset também roda no `Drop` do escopo mais externo, que pode cair durante a destruição
    // dos `thread_local` da thread; um valor já destruído não tem o que esvaziar.
    let _ = SLOTS.try_with(|slots| slots.borrow_mut().clear());
    let _ = OBJECT_SLOTS.try_with(|slots| slots.borrow_mut().clear());
    for prototype in [&READABLE_PROTOTYPE, &WRITABLE_PROTOTYPE, &STREAM_PROTOTYPE, &EMITTER_PROTOTYPE, &WRITE_PROTOTYPE, &READ_PROTOTYPE] {
        let _ = prototype.try_with(|cell| cell.set(None));
    }
    let _ = STDIN_FLOW_QUEUED.try_with(|queued| queued.set(false));
    let _ = STDIN_DECODER.try_with(|decoder| *decoder.borrow_mut() = None);
    let _ = STDIN_CONSTRUCT_PENDING.try_with(|pending| pending.set(true));
    let _ = STDIN_OBJECT.try_with(|object| object.set(None));
}

/// Roda `access` sobre o `Slot` `slot`, criando os que faltam.
fn with_slot<R>(slot: usize, access: impl FnOnce(&mut Slot) -> R) -> R {
    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        if slots.len() <= slot.max(STDIO_SLOTS - 1) {
            slots.resize_with(slot.max(STDIO_SLOTS - 1) + 1, Slot::default);
        }
        access(&mut slots[slot])
    })
}

/// Roda `access` sobre a tabela de ouvintes de `slot`.
fn with_table<R>(slot: usize, access: impl FnOnce(&mut ListenerTable) -> R) -> R {
    with_slot(slot, |slot| access(&mut slot.listeners))
}

/// Dá um `Slot` novo ao objeto `object` (a identidade é o `encode` do valor).
fn register_object(object: JSValue) -> JSValue {
    let slot = OBJECT_SLOTS.with(|objects| {
        let mut objects = objects.borrow_mut();
        let slot = STDIO_SLOTS + objects.len();
        objects.push((object.encode(), slot));
        slot
    });
    with_slot(slot, |_| ());
    object
}

fn state_of(slot: usize) -> WriteState {
    with_slot(slot, |slot| slot.write)
}

fn update_state(slot: usize, change: impl FnOnce(&mut WriteState)) {
    with_slot(slot, |slot| change(&mut slot.write));
}

/// O descritor do console host para escrever do `slot`: o stderr para o 2, o stdout para os demais.
fn host_fd(slot: usize) -> usize {
    if slot == 2 { 2 } else { 1 }
}

/// O `Slot` do stream que é o `this` da chamada: o descritor da propriedade própria `fd` (0, 1 ou 2), senão o do
/// objeto registrado, senão o stdout.
fn fd_of(global_object: &JSGlobalObject, call: &HostCall) -> Result<usize, Thrown> {
    let fd = get_property(global_object, call.this_value(), "fd")?;
    if fd.is_number() {
        return Ok((fd.as_number() as usize).min(2));
    }
    let identity = call.this_value().encode();
    Ok(OBJECT_SLOTS.with(|objects| objects.borrow().iter().find(|(key, _)| *key == identity).map_or(1, |(_, slot)| *slot)))
}


/// Os bytes de `chunk`, ou o erro de tipo do bun.
fn chunk_bytes(global_object: &JSGlobalObject, chunk: JSValue) -> Result<Vec<u8>, Thrown> {
    if chunk.is_undefined_or_null() {
        return Err(throw_coded_type_error(global_object, WRITE_MESSAGE, "ERR_STREAM_NULL_VALUES"));
    }
    if chunk.is_string() {
        return string_bytes(global_object, chunk);
    }
    input_bytes(chunk).ok_or_else(|| throw_coded_type_error(global_object, WRITE_MESSAGE, "ERR_INVALID_ARG_TYPE"))
}

/// Entrega `bytes` ao console host no descritor do `slot` (stderr para o 2, stdout para os demais).
fn write_bytes(global_object: &JSGlobalObject, fd: usize, bytes: &[u8]) {
    if let Some(host) = global_object.console_host() {
        if host_fd(fd) == 1 {
            host.write_stdout(bytes);
        } else {
            host.write_stderr(bytes);
        }
    }
}

/// O primeiro argumento (a partir de `from`) que for função, ou `undefined`.
fn find_callback(call: &HostCall, from: usize) -> JSValue {
    call.arguments().iter().skip(from).find(|argument| argument.is_callable()).copied().unwrap_or_else(JSValue::undefined)
}

/// Chama `callback` (se for função), sem `this` e com `args`.
fn run_callback(global_object: &JSGlobalObject, callback: JSValue, args: &[JSValue]) -> Result<(), Thrown> {
    if callback.is_callable() && call_function(global_object, callback, JSValue::undefined(), args).is_none() {
        return Err(Thrown::Pending);
    }
    Ok(())
}

/// Chama o primeiro argumento (a partir de `from`) que for função, sem `this` e com `args`.
fn call_callback(global_object: &JSGlobalObject, call: &HostCall, from: usize, args: &[JSValue]) -> Result<(), Thrown> {
    run_callback(global_object, find_callback(call, from), args)
}

/// As tarefas que o bun roda no `process.nextTick` depois de `end` e de `write` após `end`.
const TASK_FINISH: i32 = 0;
const TASK_WRITE_AFTER_END: i32 = 1;
const TASK_CALLBACK: i32 = 2;
const TASK_ALREADY_FINISHED: i32 = 3;
/// O `finish` de `end` espera um segundo `nextTick` (medido: um tick do usuário agendado logo depois de `end` roda antes).
const TASK_FINISH_LATE: i32 = 7;

/// Agenda no fim da fila de ticks a tarefa `kind` do stream `stream` (descritor `fd`) com o `callback` do usuário.
fn queue_task(global_object: &JSGlobalObject, fd: usize, kind: i32, callback: JSValue, stream: JSValue) {
    let task = native(global_object, "", 4, deferred_task);
    queue_tick(global_object, task, vec![js_number(fd as i32), js_number(kind), callback, stream]);
}

/// Emite `event` no stream, propagando a exceção de um ouvinte.
fn emit_on(global_object: &JSGlobalObject, fd: usize, stream: JSValue, event: &str, args: &[JSValue]) -> Result<bool, Thrown> {
    let functions = with_table(fd, |table| table.take_for_emit(event));
    call_listeners(global_object, &functions, stream, args).map_err(|thrown| rethrow(global_object, thrown))
}

/// O corpo das tarefas agendadas: `finish` (callback de `end` e depois o evento), o erro de escrita após `end`
/// (callback com o erro e depois o evento `error`; sem ouvinte o erro fica sem captura) e o callback puro de `close`.
fn deferred_task_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = call.argument(0).as_number() as usize;
    let (kind, callback, stream) = (call.argument(1).as_number() as i32, call.argument(2), call.argument(3));
    match kind {
        TASK_FINISH => queue_task(global_object, fd, TASK_FINISH_LATE, callback, stream),
        TASK_FINISH_LATE => {
            update_state(fd, |state| state.finished = true);
            run_callback(global_object, callback, &[JSValue::null()])?;
            let extra = with_slot(fd, |slot| std::mem::take(&mut slot.pending_end));
            for pending in extra {
                run_callback(global_object, pending, &[JSValue::null()])?;
            }
            emit_on(global_object, fd, stream, "finish", &[])?;
        }
        TASK_ALREADY_FINISHED => {
            let error = thrown_to_value(global_object, throw_coded_error(global_object, "Cannot call end after a stream was finished", "ERR_STREAM_ALREADY_FINISHED"));
            run_callback(global_object, callback, &[error])?;
        }
        TASK_WRITE_AFTER_END => {
            let make = || throw_coded_error(global_object, "write after end", "ERR_STREAM_WRITE_AFTER_END");
            let error = thrown_to_value(global_object, make());
            run_callback(global_object, callback, &[error])?;
            if with_table(fd, |table| table.count("error")) == 0 {
                return Err(make());
            }
            emit_on(global_object, fd, stream, "error", &[error])?;
        }
        TASK_STDIN_FLOW => emit_stdin_flow(global_object, stream)?,
        _ => run_callback(global_object, callback, &[])?,
    }
    Ok(JSValue::undefined())
}
host_function!(deferred_task, deferred_task_body);

/// `write` do descritor `fd` (1 ou 2): o callback roda sincronamente logo depois da escrita; depois de `end` o bun
/// devolve `false` e entrega `ERR_STREAM_WRITE_AFTER_END` assíncrono (sem escrever).
fn write_to(global_object: &JSGlobalObject, call: &HostCall, fd: usize) -> HostResult {
    let bytes = chunk_bytes(global_object, call.argument(0))?;
    if state_of(fd).ended {
        queue_task(global_object, fd, TASK_WRITE_AFTER_END, find_callback(call, 1), call.this_value());
        return Ok(JSValue::Bool(false));
    }
    write_bytes(global_object, fd, &bytes);
    call_callback(global_object, call, 1, &[])?;
    Ok(JSValue::Bool(true))
}

fn stdout_write_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    write_to(global_object, call, 1)
}
host_function!(stdout_write, stdout_write_body);

fn stderr_write_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    write_to(global_object, call, 2)
}
host_function!(stderr_write, stderr_write_body);

/// `Writable.prototype.write`/`_write` herdado: escreve no descritor do `this`.
fn proto_write_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    write_to(global_object, call, fd)
}
host_function!(proto_write, proto_write_body);

/// `Writable.prototype.cork`.
fn cork_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    update_state(fd, |state| state.corked += 1);
    Ok(JSValue::undefined())
}
host_function!(cork, cork_body);

/// `Writable.prototype.uncork`: só desce quando há `cork` pendente.
fn uncork_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    update_state(fd, |state| state.corked = state.corked.saturating_sub(1));
    Ok(JSValue::undefined())
}
host_function!(uncork, uncork_body);

/// O `TypeError` `ERR_UNKNOWN_ENCODING` com o valor já formatado.
fn unknown_encoding(global_object: &JSGlobalObject, shown: &str) -> Thrown {
    let message = format!("Unknown encoding: {shown}");
    crate::runtime::node_error::throw_coded_type_error(global_object, &message, "ERR_UNKNOWN_ENCODING")
}

/// `Writable.prototype.setDefaultEncoding(encoding)`: valida o nome e devolve `this`. Medido: só uma string pode ser um
/// nome; qualquer outro valor (inclusive `undefined` e `null`) é erro, com o valor mostrado pelo `util.inspect`
/// (`5`, `{}`, `[ 1, 2 ]`, `Symbol(x)`, `10n`), sem chamar `toString` do objeto.
fn set_default_encoding_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let encoding = call.argument(0);
    if !encoding.is_string() {
        let shown = crate::runtime::util_inspect::inspect_value(global_object, encoding)?;
        return Err(unknown_encoding(global_object, &shown));
    }
    let name = String::from_utf8_lossy(&string_bytes(global_object, encoding)?).into_owned();
    if !ENCODINGS.contains(&name.to_ascii_lowercase().as_str()) {
        return Err(unknown_encoding(global_object, &name));
    }
    Ok(call.this_value())
}
host_function!(set_default_encoding, set_default_encoding_body);

/// `Writable.prototype.end(chunk?, encoding?, callback?)`: escreve o pedaço, marca o fim, devolve `this` e agenda
/// (no `nextTick`) o callback e o evento `finish`. Medido num segundo `end`: antes do `finish` o callback entra na fila
/// do `finish` (roda antes do evento, com `null`); depois do `finish` o callback recebe `ERR_STREAM_ALREADY_FINISHED`
/// assíncrono; com pedaço (até `''`) vira `ERR_STREAM_WRITE_AFTER_END` assíncrono. Sempre devolve `this`.
fn end_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    let chunk = call.argument(0);
    let has_chunk = !chunk.is_undefined_or_null() && !chunk.is_callable();
    let (state, callback) = (state_of(fd), find_callback(call, 0));
    if !state.ended {
        if has_chunk {
            write_bytes(global_object, fd, &chunk_bytes(global_object, chunk)?);
        }
        update_state(fd, |state| state.ended = true);
        queue_task(global_object, fd, TASK_FINISH, callback, call.this_value());
    } else if has_chunk {
        chunk_bytes(global_object, chunk)?;
        queue_task(global_object, fd, TASK_WRITE_AFTER_END, callback, call.this_value());
    } else if callback.is_callable() {
        if state.finished {
            queue_task(global_object, fd, TASK_ALREADY_FINISHED, callback, call.this_value());
        } else {
            with_slot(fd, |slot| slot.pending_end.push(callback));
        }
    }
    Ok(call.this_value())
}
host_function!(end, end_body);

/// `_final(callback)`: avisa o callback de que não há mais o que escoar.
fn final_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_callback(global_object, call, 0, &[])?;
    Ok(JSValue::undefined())
}
host_function!(stream_final, final_body);

/// `_destroy(error, callback)`: devolve o erro ao callback.
fn destroy_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_callback(global_object, call, 1, &[call.argument(0)])?;
    Ok(JSValue::undefined())
}
host_function!(stream_destroy, destroy_body);

/// `Writable.prototype.destroy()`: o stdio não se destrói (`destroyed` continua `false`); devolve `this`.
fn destroy_method_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(call.this_value())
}
host_function!(destroy_method, destroy_method_body);

/// `WriteStream.prototype._construct(callback)`: chama o callback na hora, devolve `undefined`.
fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_callback(global_object, call, 0, &[])?;
    Ok(JSValue::undefined())
}
host_function!(stream_construct, construct_body);

/// `open()` e `_undestroy()`: no bun medido devolvem `undefined` sem efeito observável (o stdio já nasce aberto).
fn open_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::undefined())
}
host_function!(open, open_body);
host_function!(undestroy, open_body);

/// `WriteStream.prototype.close(callback?)`: devolve `undefined` e chama o callback no `nextTick`.
fn close_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    queue_task(global_object, fd, TASK_CALLBACK, find_callback(call, 0), call.this_value());
    Ok(JSValue::undefined())
}
host_function!(stream_close, close_body);

/// `WriteStream.prototype._writev(chunks, callback)`: escreve cada `chunk` da lista e chama o callback na hora.
fn writev_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    let list = call.argument(0);
    let length = get_property(global_object, list, "length")?;
    for index in 0..(if length.is_number() { length.as_number() as u32 } else { 0 }) {
        let entry = get_property(global_object, list, &index.to_string())?;
        let chunk = get_property(global_object, entry, "chunk")?;
        write_bytes(global_object, fd, &chunk_bytes(global_object, chunk)?);
    }
    call_callback(global_object, call, 1, &[])?;
    Ok(JSValue::undefined())
}
host_function!(stream_writev, writev_body);

/// `pipe` de um stream que só escreve: `ERR_STREAM_CANNOT_PIPE`.
fn pipe_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_error(global_object, "Cannot pipe, not readable", "ERR_STREAM_CANNOT_PIPE"))
}
host_function!(cannot_pipe, pipe_body);

/// `destroySoon()`: fecha a escrita como `end`.
fn destroy_soon_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    end_body(global_object, call)?;
    Ok(JSValue::undefined())
}
host_function!(destroy_soon, destroy_soon_body);

/// Define um acessor só de leitura de `Writable.prototype` que devolve `$value` (função de `fd` e do estado).
macro_rules! writable_getter {
    ($name:ident, $body:ident, |$global:ident, $fd:ident| $value:expr) => {
        fn $body($global: &JSGlobalObject, call: &HostCall) -> HostResult {
            let $fd = fd_of($global, call)?;
            Ok($value)
        }
        host_function!($name, $body);
    };
}

writable_getter!(get_closed, get_closed_body, |_global, _fd| JSValue::Bool(false));
writable_getter!(get_destroyed, get_destroyed_body, |_global, _fd| JSValue::Bool(false));
writable_getter!(get_writable, get_writable_body, |_global, fd| JSValue::Bool(!state_of(fd).ended));
writable_getter!(get_finished, get_finished_body, |_global, fd| JSValue::Bool(state_of(fd).finished));
writable_getter!(get_object_mode, get_object_mode_body, |_global, _fd| JSValue::Bool(false));
writable_getter!(get_buffer, get_buffer_body, |global_object, _fd| array_value(global_object, &[]));
writable_getter!(get_ended, get_ended_body, |_global, fd| JSValue::Bool(state_of(fd).ended));
writable_getter!(get_need_drain, get_need_drain_body, |_global, _fd| JSValue::Bool(false));
writable_getter!(get_high_water_mark, get_high_water_mark_body, |_global, _fd| js_number(65536));
writable_getter!(get_corked, get_corked_body, |_global, fd| js_number(state_of(fd).corked as i32));
writable_getter!(get_length, get_length_body, |_global, _fd| js_number(0));
writable_getter!(get_errored, get_errored_body, |_global, _fd| JSValue::null());
writable_getter!(get_aborted, get_aborted_body, |_global, _fd| JSValue::Bool(false));

/// Valida o ouvinte e o registra no começo (`prepend`) ou no fim da lista do descritor do `this`.
fn add_listener(global_object: &JSGlobalObject, call: &HostCall, once: bool, prepend: bool) -> HostResult {
    let listener = call.argument(1);
    if !listener.is_callable() {
        let message = format!("The \"listener\" argument must be of type function. Received {}", received_description(global_object, listener));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let fd = fd_of(global_object, call)?;
    let event = event_key(global_object, call.argument(0));
    if with_table(fd, |table| table.count("newListener")) > 0 {
        emit_on(global_object, fd, call.this_value(), "newListener", &[call.argument(0), listener])?;
    }
    with_table(fd, |table| {
        if prepend {
            table.add_front(&event, listener, once);
        } else {
            table.add(&event, listener, once);
        }
    });
    sync_events(global_object, call.this_value(), fd, false);
    if fd == 0 {
        note_stdin_listener(global_object, &event, call.this_value());
    }
    Ok(call.this_value())
}

fn on_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_listener(global_object, call, false, false)
}
host_function!(emitter_on, on_body);

fn once_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_listener(global_object, call, true, false)
}
host_function!(emitter_once, once_body);

fn prepend_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_listener(global_object, call, false, true)
}
host_function!(emitter_prepend, prepend_body);

fn prepend_once_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_listener(global_object, call, true, true)
}
host_function!(emitter_prepend_once, prepend_once_body);

/// `removeAllListeners([event])`: sem argumento esvazia tudo; devolve `this`.
fn remove_all_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    let event = (!call.argument(0).is_undefined()).then(|| event_key(global_object, call.argument(0)));
    let had_listeners = with_table(fd, |table| !table.event_names().is_empty());
    with_table(fd, |table| table.remove_all(event.as_deref()));
    // Como o `removeAllListeners` do node: a contagem que chega a zero troca o `_events` por um objeto vazio.
    let emptied = event.is_some() && had_listeners && with_table(fd, |table| table.event_names().is_empty());
    if event.is_none() || emptied {
        with_slot(fd, |slot| slot.events_cleared = true);
    }
    sync_events(global_object, call.this_value(), fd, true);
    Ok(call.this_value())
}
host_function!(emitter_remove_all, remove_all_body);

/// `setMaxListeners(n)`: guarda o limite e devolve `this`.
fn set_max_listeners_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    let limit = call.argument(0);
    if !limit.is_number() || limit.as_number() < 0.0 || limit.as_number().is_nan() {
        let message = format!("The value of \"n\" is out of range. It must be a non-negative number. Received {}", received_description(global_object, limit));
        return Err(throw_coded_error(global_object, &message, "ERR_OUT_OF_RANGE"));
    }
    with_slot(fd, |slot| slot.max_listeners = Some(limit.as_number() as i32));
    if let Some(target) = JSObject::from_value(&call.this_value()) {
        target.put_direct(global_object.vm(), &prop(global_object.vm(), "_maxListeners"), limit, 0);
    }
    Ok(call.this_value())
}
host_function!(emitter_set_max_listeners, set_max_listeners_body);

fn get_max_listeners_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    Ok(js_number(with_slot(fd, |slot| slot.max_listeners).unwrap_or(10)))
}
host_function!(emitter_get_max_listeners, get_max_listeners_body);

fn off_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    let event = event_key(global_object, call.argument(0));
    with_table(fd, |table| table.remove(&event, call.argument(1)));
    sync_events(global_object, call.this_value(), fd, false);
    Ok(call.this_value())
}
host_function!(emitter_off, off_body);

fn emit_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    let event = event_key(global_object, call.argument(0));
    let args = call.arguments().get(1..).unwrap_or_default();
    Ok(JSValue::Bool(emit_on(global_object, fd, call.this_value(), &event, args)?))
}
host_function!(emitter_emit, emit_body);

fn listener_count_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    let event = event_key(global_object, call.argument(0));
    Ok(js_number(with_table(fd, |table| table.count(&event)) as i32))
}
host_function!(emitter_listener_count, listener_count_body);

fn listeners_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    let event = event_key(global_object, call.argument(0));
    Ok(array_value(global_object, &with_table(fd, |table| table.functions(&event))))
}
host_function!(emitter_listeners, listeners_body);

fn event_names_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let fd = fd_of(global_object, call)?;
    let names: Vec<JSValue> = with_table(fd, |table| table.event_names()).iter().map(|name| event_value(global_object, name)).collect();
    Ok(array_value(global_object, &names))
}
host_function!(emitter_event_names, event_names_body);

/// As chaves-base de `_events` de um `Writable` (medidas: ficam com valor `undefined` até haver ouvinte).
const BASE_EVENTS: [&str; 5] = ["close", "error", "prefinish", "finish", "drain"];
/// As chaves-base de `_events` de um stream de leitura (medido em `Readable()` e `ReadStream(path)`).
const READ_EVENTS: [&str; 5] = ["close", "error", "data", "end", "readable"];

/// `onceWrapper` do node: o valor que `_events` guarda para um ouvinte `once`, com `listener` apontando o original.
fn once_wrapper_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let listener = get_property(global_object, JSValue::from_cell(call.callee()), "listener")?;
    call_function(global_object, listener, call.this_value(), call.arguments()).ok_or(Thrown::Pending)
}
host_function!(once_wrapper_function, once_wrapper_body);

/// O valor de `_events` para um ouvinte: a função, ou o `onceWrapper` dela quando é `once`.
fn event_slot_value(global_object: &JSGlobalObject, function: JSValue, once: bool) -> JSValue {
    if !once {
        return function;
    }
    let wrapper = native(global_object, "onceWrapper", 0, once_wrapper_function);
    if let Some(target) = JSObject::from_value(&wrapper) {
        target.put_direct(global_object.vm(), &prop(global_object.vm(), "listener"), function, 0);
    }
    wrapper
}

/// O `_events` inicial de um `Writable`: as chaves-base com `undefined`, a menos que `removeAllListeners()` tenha esvaziado.
fn base_events(global_object: &JSGlobalObject, cleared: bool) -> JSValue {
    events_with_keys(global_object, if cleared { &[] } else { &BASE_EVENTS })
}

/// Um `_events` (objeto comum, medido no bun: o protótipo não é nulo) com as chaves `names` em `undefined`.
fn events_with_keys(global_object: &JSGlobalObject, names: &[&str]) -> JSValue {
    let vm = global_object.vm();
    let events = construct_empty_object(global_object);
    for name in names {
        events.put_direct(vm, &prop(vm, name), JSValue::undefined(), 0);
    }
    events.as_value()
}

/// Atualiza `_events` e `_eventsCount` do `stream` (descritor `fd`) a partir da tabela de ouvintes: um ouvinte é a função
/// (ou o `onceWrapper`), vários são um array, e a contagem é o número de eventos com ouvinte. Medido: `on`/`once`/`off`
/// mexem no MESMO objeto `_events` (o evento sem ouvinte fica com a chave e valor `undefined`); `removeAllListeners`
/// (`replace`) troca o objeto por um novo.
fn sync_events(global_object: &JSGlobalObject, stream: JSValue, fd: usize, replace: bool) {
    let vm = global_object.vm();
    let cleared = with_slot(fd, |slot| slot.events_cleared);
    let current = get_property(global_object, stream, "_events").ok().filter(|value| JSObject::from_value(value).is_some());
    let events = match current {
        Some(existing) if !replace => existing,
        _ => {
            with_slot(fd, |slot| slot.synced_names.clear());
            base_events(global_object, cleared)
        }
    };
    let names = with_table(fd, |table| table.event_names());
    if let Some(target) = JSObject::from_value(&events) {
        let gone: Vec<String> = with_slot(fd, |slot| slot.synced_names.iter().filter(|old| !names.contains(old)).cloned().collect());
        for name in &gone {
            target.put_direct(vm, &prop(vm, name), JSValue::undefined(), 0);
        }
        for name in &names {
            let entries = with_table(fd, |table| table.entries_of(name));
            let values: Vec<JSValue> = entries.iter().map(|(function, once)| event_slot_value(global_object, *function, *once)).collect();
            let value = if values.len() == 1 { values[0] } else { array_value(global_object, &values) };
            target.put_direct(vm, &prop(vm, name), value, 0);
        }
        with_slot(fd, |slot| slot.synced_names = names.clone());
    }
    if let Some(target) = JSObject::from_value(&stream) {
        target.put_direct(vm, &prop(vm, "_events"), events, 0);
        // O stdin carrega o `kConstruct` (chave símbolo) além dos nomes de `_events`, até o consumir.
        let extra = i32::from(fd == 0 && STDIN_CONSTRUCT_PENDING.with(Cell::get));
        target.put_direct(vm, &prop(vm, "_eventsCount"), js_number(names.len() as i32 + extra), 0);
    }
}

/// O objeto que `new Stream()`/`new EventEmitter()` devolve: `_events` vazio, `_eventsCount` 0 e `_maxListeners`
/// `undefined`, nesta ordem (medido); sem `new` o node lança o `TypeError` do `this` indefinido.
fn new_emitter_object(global_object: &JSGlobalObject, call: &HostCall, slot: &'static LocalKey<Cell<Option<JSValue>>>) -> HostResult {
    // Com `new` o alvo é uma função; numa chamada comum ele é o `this`, que o principal sloppy do porte deixa ser o global.
    if !call.new_target().is_callable() {
        return Err(Thrown::type_error("undefined is not an object (evaluating 'this._events')"));
    }
    let vm = global_object.vm();
    let object = object_with_prototype(global_object, slot.with(Cell::get));
    object.put_direct(vm, &prop(vm, "_events"), construct_empty_object(global_object).as_value(), 0);
    object.put_direct(vm, &prop(vm, "_eventsCount"), js_number(0), 0);
    object.put_direct(vm, &prop(vm, "_maxListeners"), JSValue::undefined(), 0);
    Ok(register_object(object.as_value()))
}

fn stream_constructor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    new_emitter_object(global_object, call, &STREAM_PROTOTYPE)
}
host_function!(stream_constructor, stream_constructor_body);

fn emitter_constructor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    new_emitter_object(global_object, call, &EMITTER_PROTOTYPE)
}
host_function!(emitter_constructor, emitter_constructor_body);

/// `Writable()` (com ou sem `new`): objeto com `_events`, `_writableState` e `_maxListeners`, nesta ordem (medido).
fn writable_constructor_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let object = object_with_prototype(global_object, WRITABLE_PROTOTYPE.with(Cell::get));
    object.put_direct(vm, &prop(vm, "_events"), base_events(global_object, false), 0);
    object.put_direct(vm, &prop(vm, "_writableState"), writable_state(global_object), 0);
    object.put_direct(vm, &prop(vm, "_maxListeners"), JSValue::undefined(), 0);
    Ok(register_object(object.as_value()))
}
host_function!(writable_constructor, writable_constructor_body);

/// Valida o `path` de `WriteStream`/`ReadStream`: só texto passa; o resto lança `ERR_INVALID_ARG_TYPE` (medido).
fn path_argument(global_object: &JSGlobalObject, call: &HostCall) -> Result<JSValue, Thrown> {
    let path = call.argument(0);
    if path.is_string() {
        return Ok(path);
    }
    let message = format!("The \"path\" argument must be of type string or URL. Received {}", received_description(global_object, path));
    Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"))
}

/// `options.flags` (texto) do segundo argumento, ou `default`.
fn flags_option(global_object: &JSGlobalObject, call: &HostCall, default: &str) -> Result<JSValue, Thrown> {
    let options = call.argument(1);
    if JSObject::from_value(&options).is_some() {
        let flags = get_property(global_object, options, "flags")?;
        if flags.is_string() {
            return Ok(flags);
        }
    }
    Ok(text_value(global_object.vm(), default))
}

/// `WriteStream(path, options?)` (com ou sem `new`): sem caminho de texto lança `ERR_INVALID_ARG_TYPE`; com caminho
/// devolve o objeto com as chaves medidas (`fd` `null` até abrir, `flags` `w`, `mode` 438). A abertura do arquivo
/// não é feita aqui (lacuna).
fn write_stream_constructor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let path = path_argument(global_object, call)?;
    let flags = flags_option(global_object, call, "w")?;
    let vm = global_object.vm();
    let stream = object_with_prototype(global_object, WRITE_PROTOTYPE.with(Cell::get));
    let undefined = JSValue::undefined();
    let put = |name: &str, value: JSValue| stream.put_direct(vm, &prop(vm, name), value, 0);
    put("fd", JSValue::null());
    put("path", path);
    put("flags", flags);
    put("mode", js_number(438));
    put("_writev", undefined);
    put("flush", JSValue::Bool(false));
    put("start", undefined);
    put("pos", undefined);
    put("bytesWritten", js_number(0));
    put("_events", base_events(global_object, false));
    put("_writableState", writable_state(global_object));
    put("_maxListeners", undefined);
    put("_eventsCount", js_number(1));
    Ok(register_object(stream.as_value()))
}
host_function!(write_stream_constructor, write_stream_constructor_body);

/// `_readableState` de um stream de leitura sem dados (campos visíveis medidos).
fn readable_state(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let state = construct_empty_object(global_object);
    state.put_direct(vm, &prop(vm, "highWaterMark"), js_number(65536), 0);
    state.put_direct(vm, &prop(vm, "buffer"), array_value(global_object, &[]), 0);
    state.put_direct(vm, &prop(vm, "bufferIndex"), js_number(0), 0);
    state.put_direct(vm, &prop(vm, "length"), js_number(0), 0);
    state.put_direct(vm, &prop(vm, "pipes"), array_value(global_object, &[]), 0);
    state.put_direct(vm, &prop(vm, "awaitDrainWriters"), JSValue::null(), 0);
    state.as_value()
}

/// `Readable()` (com ou sem `new`): objeto com `_events` (`close`, `error`, `data`, `end`, `readable`), `_readableState`
/// e `_maxListeners`, nesta ordem (medido).
fn readable_constructor_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let object = object_with_prototype(global_object, READABLE_PROTOTYPE.with(Cell::get));
    object.put_direct(vm, &prop(vm, "_events"), events_with_keys(global_object, &READ_EVENTS), 0);
    object.put_direct(vm, &prop(vm, "_readableState"), readable_state(global_object), 0);
    object.put_direct(vm, &prop(vm, "_maxListeners"), JSValue::undefined(), 0);
    Ok(register_object(object.as_value()))
}
host_function!(readable_constructor, readable_constructor_body);

/// `ReadStream(path, options?)` (com ou sem `new`): sem caminho de texto lança `ERR_INVALID_ARG_TYPE`; com caminho
/// devolve o objeto com as chaves medidas (`flags` `r`, `end` `null`).
fn read_stream_constructor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let path = path_argument(global_object, call)?;
    let flags = flags_option(global_object, call, "r")?;
    let vm = global_object.vm();
    let stream = object_with_prototype(global_object, READ_PROTOTYPE.with(Cell::get));
    let undefined = JSValue::undefined();
    let put = |name: &str, value: JSValue| stream.put_direct(vm, &prop(vm, name), value, 0);
    put("fd", JSValue::null());
    put("path", path);
    put("flags", flags);
    put("mode", js_number(438));
    put("start", undefined);
    put("end", JSValue::null());
    put("pos", undefined);
    put("bytesRead", js_number(0));
    put("_events", events_with_keys(global_object, &READ_EVENTS));
    put("_readableState", readable_state(global_object));
    put("_maxListeners", undefined);
    put("_eventsCount", js_number(1));
    Ok(register_object(stream.as_value()))
}
host_function!(read_stream_constructor, read_stream_constructor_body);

/// `onwrite` ligado do `_writableState`: sem argumento lança `ERR_MULTIPLE_CALLBACK`; com um argumento devolve
/// `undefined` e `pendingcb` fica como estava (medido com `1`).
fn onwrite_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() == 0 {
        return Err(throw_coded_error(global_object, "Callback called multiple times", "ERR_MULTIPLE_CALLBACK"));
    }
    Ok(JSValue::undefined())
}
host_function!(onwrite, onwrite_body);

/// Um objeto vazio com protótipo `prototype` (ou o `Object.prototype` quando `None`).
fn object_with_prototype(global_object: &JSGlobalObject, prototype: Option<JSValue>) -> crate::runtime::js_object::JSObjectRef {
    let object = construct_empty_object(global_object);
    if let Some(prototype) = prototype {
        object.set_prototype_direct(global_object.vm(), prototype);
    }
    object
}

/// Pendura em `target` os `(nome, length, função)`, enumeráveis (como os métodos do `Writable` do bun).
fn put_methods(global_object: &JSGlobalObject, target: &crate::runtime::js_object::JSObject, methods: &[(&str, u32, NativeFunction)]) {
    let vm = global_object.vm();
    for (name, length, function) in methods {
        target.put_direct(vm, &prop(vm, name), native(global_object, name, *length, *function), 0);
    }
}

/// `constructor` não enumerável de um protótipo da cadeia.
fn put_constructor(global_object: &JSGlobalObject, prototype: &crate::runtime::js_object::JSObject, name: &str, length: u32, function: NativeFunction) {
    let vm = global_object.vm();
    // Com ou sem `new` a função faz o mesmo (medido: `new WriteStream()` valida `path` como a chamada comum).
    let constructor = native_function_with_constructor(global_object, name, length, function, function);
    // Medido: `prototype` é uma propriedade de dados comum de função (gravável, configurável, não enumerável), depois de
    // `length` e `name`, e `C.prototype` é o protótipo do objeto criado.
    constructor.put_direct(vm, &prop(vm, "prototype"), prototype.as_value(), DONT_ENUM);
    prototype.put_direct(vm, &prop(vm, "constructor"), constructor.as_value(), DONT_ENUM);
}

/// `EventEmitter.prototype` -> `Stream.prototype`, pela ordem de baixo para cima; devolve o `Stream.prototype`.
fn build_stream_prototype(global_object: &JSGlobalObject) -> JSValue {
    let emitter = object_with_prototype(global_object, None);
    put_constructor(global_object, &emitter, "EventEmitter", 1, emitter_constructor);
    EMITTER_PROTOTYPE.with(|cell| cell.set(Some(emitter.as_value())));
    put_methods(
        global_object,
        &emitter,
        &[
            ("addListener", 2, emitter_on as NativeFunction),
            ("on", 2, emitter_on as NativeFunction),
            ("once", 2, emitter_once as NativeFunction),
            ("removeListener", 2, emitter_off as NativeFunction),
            ("off", 2, emitter_off as NativeFunction),
            ("emit", 1, emitter_emit as NativeFunction),
            ("listeners", 1, emitter_listeners as NativeFunction),
            ("rawListeners", 1, emitter_listeners as NativeFunction),
            ("listenerCount", 1, emitter_listener_count as NativeFunction),
            ("eventNames", 0, emitter_event_names as NativeFunction),
            ("prependListener", 2, emitter_prepend as NativeFunction),
            ("prependOnceListener", 2, emitter_prepend_once as NativeFunction),
            ("removeAllListeners", 1, emitter_remove_all as NativeFunction),
            ("setMaxListeners", 1, emitter_set_max_listeners as NativeFunction),
            ("getMaxListeners", 0, emitter_get_max_listeners as NativeFunction),
        ],
    );
    let stream = object_with_prototype(global_object, Some(emitter.as_value()));
    // O padrão do protótipo do `EventEmitter` do bun (`EventEmitterPrototype._eventsCount = 0`): um stream que ainda
    // não registrou ouvinte lê 0 e não `undefined` (`process.stdout._eventsCount`).
    let vm = global_object.vm();
    emitter.put_direct(vm, &prop(vm, "_eventsCount"), js_number(0), 0);
    put_constructor(global_object, &stream, "Stream", 1, stream_constructor);
    STREAM_PROTOTYPE.with(|cell| cell.set(Some(stream.as_value())));
    put_methods(global_object, &stream, &[("pipe", 2, cannot_pipe as NativeFunction), ("eventNames", 0, emitter_event_names as NativeFunction)]);
    stream.as_value()
}

/// `Writable.prototype` e, sobre ele, `WriteStream.prototype`; devolve o segundo.
fn build_write_prototype(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let writable = object_with_prototype(global_object, Some(build_stream_prototype(global_object)));
    put_constructor(global_object, &writable, "Writable", 1, writable_constructor);
    WRITABLE_PROTOTYPE.with(|cell| cell.set(Some(writable.as_value())));
    put_methods(
        global_object,
        &writable,
        &[
            ("pipe", 2, cannot_pipe as NativeFunction),
            ("write", 3, proto_write as NativeFunction),
            ("cork", 0, cork as NativeFunction),
            ("uncork", 0, uncork as NativeFunction),
            ("setDefaultEncoding", 1, set_default_encoding as NativeFunction),
            ("_write", 3, proto_write as NativeFunction),
        ],
    );
    writable.put_direct(vm, &prop(vm, "_writev"), JSValue::null(), 0);
    put_methods(global_object, &writable, &[("end", 3, end as NativeFunction)]);
    let getters: [(&str, NativeFunction); 13] = [
        ("closed", get_closed),
        ("destroyed", get_destroyed),
        ("writable", get_writable),
        ("writableFinished", get_finished),
        ("writableObjectMode", get_object_mode),
        ("writableBuffer", get_buffer),
        ("writableEnded", get_ended),
        ("writableNeedDrain", get_need_drain),
        ("writableHighWaterMark", get_high_water_mark),
        ("writableCorked", get_corked),
        ("writableLength", get_length),
        ("errored", get_errored),
        ("writableAborted", get_aborted),
    ];
    for (name, getter) in getters {
        put_accessor_with(global_object, &writable, name, getter, None, DONT_ENUM | DONT_DELETE);
    }
    put_methods(
        global_object,
        &writable,
        &[("destroy", 2, destroy_method as NativeFunction), ("_undestroy", 0, undestroy as NativeFunction), ("_destroy", 2, stream_destroy as NativeFunction)],
    );
    let write_stream = object_with_prototype(global_object, Some(writable.as_value()));
    put_constructor(global_object, &write_stream, "WriteStream", 2, write_stream_constructor);
    put_methods(
        global_object,
        &write_stream,
        &[
            ("open", 0, open as NativeFunction),
            ("_construct", 1, stream_construct as NativeFunction),
            ("_write", 3, proto_write as NativeFunction),
            ("_writev", 2, stream_writev as NativeFunction),
            ("_destroy", 2, stream_destroy as NativeFunction),
            ("close", 1, stream_close as NativeFunction),
            ("destroySoon", 3, destroy_soon as NativeFunction),
        ],
    );
    put_accessor_with(global_object, &write_stream, "autoClose", get_need_drain, None, DONT_ENUM | DONT_DELETE);
    put_accessor_with(global_object, &write_stream, "pending", get_need_drain, None, DONT_ENUM | DONT_DELETE);
    write_stream.as_value()
}

/// `Readable.prototype` e, sobre ele, `ReadStream.prototype`; devolve o segundo. Só os nomes da cadeia (o conteúdo do
/// `Readable` é lacuna).
fn build_read_prototype(global_object: &JSGlobalObject) -> JSValue {
    let readable = object_with_prototype(global_object, Some(build_stream_prototype(global_object)));
    put_constructor(global_object, &readable, "Readable", 2, readable_constructor);
    put_methods(global_object, &readable, &[("setEncoding", 1, readable_set_encoding)]);
    let vm = global_object.vm();
    readable.put_direct(vm, &prop(vm, "isPaused"), native(global_object, "", 0, readable_is_paused), 0);
    put_readable_flowing(global_object, &readable);
    // Medido: getters do `Readable.prototype`; o stdio nunca se destrói nem termina.
    put_accessor_with(global_object, &readable, "destroyed", readable_destroyed, None, DONT_ENUM);
    put_accessor_with(global_object, &readable, "readable", readable_readable, None, DONT_ENUM);
    READABLE_PROTOTYPE.with(|cell| cell.set(Some(readable.as_value())));
    let read_stream = object_with_prototype(global_object, Some(readable.as_value()));
    put_constructor(global_object, &read_stream, "ReadStream", 2, read_stream_constructor);
    read_stream.as_value()
}

/// O protótipo compartilhado guardado em `slot`, construído na primeira chamada.
fn shared_prototype(slot: &'static LocalKey<Cell<Option<JSValue>>>, build: impl FnOnce() -> JSValue) -> JSValue {
    match slot.with(Cell::get) {
        Some(prototype) => prototype,
        None => {
            let prototype = build();
            slot.with(|cell| cell.set(Some(prototype)));
            prototype
        }
    }
}

/// O `_writableState` de um stream de escrita sem TTY (campos visíveis medidos).
fn writable_state(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let state = construct_empty_object(global_object);
    state.put_direct(vm, &prop(vm, "highWaterMark"), js_number(65536), 0);
    state.put_direct(vm, &prop(vm, "length"), js_number(0), 0);
    state.put_direct(vm, &prop(vm, "corked"), js_number(0), 0);
    state.put_direct(vm, &prop(vm, "onwrite"), native(global_object, "bound onwrite", 1, onwrite), 0);
    for name in ["writelen", "bufferedIndex", "pendingcb"] {
        state.put_direct(vm, &prop(vm, name), js_number(0), 0);
    }
    state.as_value()
}

/// Os eventos que o bun liga sozinho no stdin, na ordem de `_events`.
const INTERNAL_STDIN_EVENTS: [&str; 4] = ["close", "end", "resume", "pause"];
/// `_eventsCount` inicial do stdin: os quatro internos mais o `kConstruct`.
const STDIN_BASE_EVENTS_COUNT: i32 = 5;
/// Tarefa do `nextTick` que lê o stdin do host e emite `data`, `end` e `close`.
const TASK_STDIN_FLOW: i32 = 4;

thread_local! {
    /// O stdin já tem a leitura agendada (acontece na primeira vez que alguém liga `data`, `end`, `close` ou `readable`).
    static STDIN_FLOW_QUEUED: Cell<bool> = const { Cell::new(false) };
    /// O decodificador do último `setEncoding`: o `data` entrega texto (sem ele o bun entrega `Buffer`). Um novo
    /// `setEncoding` troca o decodificador sem soltar o resto do anterior (medido).
    static STDIN_DECODER: RefCell<Option<StringDecoder>> = const { RefCell::new(None) };
    /// O `kConstruct` ainda está em `_events` do stdin (conta em `_eventsCount`); sai no primeiro microtask.
    static STDIN_CONSTRUCT_PENDING: Cell<bool> = const { Cell::new(true) };
    /// O objeto `process.stdin`, para o microtask que consome o `kConstruct`.
    static STDIN_OBJECT: Cell<Option<JSValue>> = const { Cell::new(None) };
}

/// Reage a um ouvinte novo do stdin: agenda uma única vez a leitura (`data`, `end`, `close`, `readable`) e move o
/// estado de fluxo (`data` liga o fluxo a menos que `pause()` ou `readable` o tenham travado; `readable` o trava).
fn note_stdin_listener(global_object: &JSGlobalObject, event: &str, stream: JSValue) {
    match event {
        "data" => with_slot(0, |slot| {
            if slot.flow.flowing != Some(false) {
                slot.flow = FlowState { flowing: Some(true), paused: false, ..slot.flow };
            }
        }),
        "readable" => with_slot(0, |slot| slot.flow = FlowState { flowing: Some(false), paused: true, readable_listening: true }),
        _ => {}
    }
    if !matches!(event, "data" | "end" | "close" | "readable") || STDIN_FLOW_QUEUED.with(|queued| queued.replace(true)) {
        return;
    }
    queue_task(global_object, 0, TASK_STDIN_FLOW, JSValue::undefined(), stream);
}

/// O microtask que o bun roda logo depois do primeiro `nextTick` (medido: `_eventsCount` 5 no tick, 4 no microtask
/// do usuário seguinte; com `data` ligado 6 e depois 5): o `kConstruct` sai de `_events`.
fn stdin_construct_done_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    STDIN_CONSTRUCT_PENDING.with(|pending| pending.set(false));
    if let Some(stream) = STDIN_OBJECT.with(Cell::get) {
        sync_events(global_object, stream, 0, false);
    }
    Ok(JSValue::undefined())
}
host_function!(stdin_construct_done, stdin_construct_done_body);

/// Tamanho dos pedaços que o bun entrega no `data` de um stdin que é pipe (medido com 200 KB: 65536, 65536, 65536 e
/// 3392). Com arquivo o bun entrega tudo num pedaço só (200000); o stdin do sandbox é o pipe do hospedeiro.
const STDIN_PIPE_CHUNK: usize = 65536;

/// Lê todo o stdin do host (bytes, inclusive a última linha sem `\n`) e emite `data`, `end` e `close`.
/// Medido: com EOF vêm só `end` e `close`; com `ab\n` e `setEncoding('utf8')`, `data "ab\n"`, `end`, `close`; sem
/// `setEncoding` cada `data` é um `Buffer` (pedaços de 65536 bytes num pipe).
fn emit_stdin_flow(global_object: &JSGlobalObject, stream: JSValue) -> Result<(), Thrown> {
    let bytes = global_object.console_host().map(|host| host.read_stdin_rest()).unwrap_or_default();
    for chunk in bytes.chunks(STDIN_PIPE_CHUNK) {
        // O decodificador é consultado a cada pedaço: um `setEncoding` feito dentro do `data` vale do pedaço seguinte.
        let decoded = STDIN_DECODER.with(|decoder| decoder.borrow_mut().as_mut().map(|decoder| decoder.write(chunk)));
        match decoded {
            Some(units) if units.is_empty() => {}
            Some(units) => {
                emit_on(global_object, 0, stream, "data", &[units_value(global_object, &units)])?;
            }
            None => {
                let buffer = crate::runtime::node_buffer::buffer_from_bytes(global_object, chunk)?;
                emit_on(global_object, 0, stream, "data", &[buffer])?;
            }
        }
    }
    // O resto de um caractere cortado no fim do fluxo sai num último `data`, antes do `end` (medido).
    let rest = STDIN_DECODER.with(|decoder| decoder.borrow_mut().as_mut().map(StringDecoder::end)).unwrap_or_default();
    if !rest.is_empty() {
        emit_on(global_object, 0, stream, "data", &[units_value(global_object, &rest)])?;
    }
    emit_on(global_object, 0, stream, "end", &[])?;
    emit_on(global_object, 0, stream, "close", &[])?;
    Ok(())
}

/// `Readable.prototype.setEncoding(name)`: devolve `this`; com um nome válido o `data` passa a entregar texto.
fn set_encoding_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Medido: `undefined` e `null` valem `utf8`; o resto passa pela conversão ToString (símbolo e `toString` que lança
    // propagam o erro do motor) e o texto resultante é o nome, mostrado cru na mensagem.
    let encoding = call.argument(0);
    let name = if encoding.is_undefined_or_null() {
        String::from("utf8")
    } else {
        let text = crate::runtime::string_regexp_support::to_wtf_string_value(global_object, encoding)?;
        let units: Vec<u16> = (0..text.length()).map(|index| text.code_unit_at(index)).collect();
        String::from_utf16_lossy(&units)
    };
    if !ENCODINGS.contains(&name.to_ascii_lowercase().as_str()) {
        return Err(unknown_encoding(global_object, &name));
    }
    let kind = Encoding::parse(&name).unwrap_or(Encoding::Utf8);
    STDIN_DECODER.with(|decoder| *decoder.borrow_mut() = Some(StringDecoder::new(kind)));
    Ok(call.this_value())
}

/// Uma string do motor com as unidades UTF-16 (o `utf16le` pode trazer substituto solto).
fn units_value(global_object: &JSGlobalObject, units: &[u16]) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf16(units)))
}
host_function!(readable_set_encoding, set_encoding_body);

/// `stdin.pause()`: devolve `this`; trava o fluxo (`readableFlowing` `false`, `isPaused()` `true`).
fn stdin_pause_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_slot(0, |slot| slot.flow = FlowState { flowing: Some(false), paused: true, ..slot.flow });
    Ok(call.this_value())
}
host_function!(stdin_pause, stdin_pause_body);

/// `stdin.resume()`: devolve `this`; sem ouvinte de `readable` liga o fluxo, com ele nada muda (medido).
fn stdin_resume_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_slot(0, |slot| {
        if !slot.flow.readable_listening {
            slot.flow = FlowState { flowing: Some(true), paused: false, ..slot.flow };
        }
    });
    Ok(call.this_value())
}
host_function!(stdin_resume, stdin_resume_body);

/// O `Slot` do `this` quando ele é um stream do porte (descritor numérico ou objeto registrado); `None` para um
/// objeto qualquer com `_readableState`.
fn slot_of(call: &HostCall, global_object: &JSGlobalObject) -> Option<usize> {
    let fd = get_property(global_object, call.this_value(), "fd").ok()?;
    if fd.is_number() {
        return Some((fd.as_number() as usize).min(2));
    }
    let identity = call.this_value().encode();
    OBJECT_SLOTS.with(|objects| objects.borrow().iter().find(|(key, _)| *key == identity).map(|(_, slot)| *slot))
}

/// O `this._readableState` que `isPaused` e `readableFlowing` leem; sem ele o bun lança o `TypeError` do `this`.
fn readable_state_of(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = call.this_value();
    let state = if this.is_undefined_or_null() { JSValue::undefined() } else { get_property(global_object, this, "_readableState")? };
    if state.is_undefined_or_null() {
        return Err(Thrown::type_error("undefined is not an object (evaluating 'this._readableState')"));
    }
    Ok(state)
}

/// `Readable.prototype.isPaused()`. No bun lê os bits de `_readableState`; num objeto qualquer (sem esses bits) dá
/// sempre `false` (medido com `{flowing:false}`, `{paused:true}` e `{}`).
fn is_paused_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    readable_state_of(global_object, call)?;
    Ok(JSValue::Bool(slot_of(call, global_object).is_some_and(|slot| with_slot(slot, |slot| slot.flow.paused))))
}
host_function!(readable_is_paused, is_paused_body);

/// Getter `readableFlowing` (nome `get`, medido): `null`, `true` ou `false`; num objeto qualquer, o `flowing` do estado.
fn readable_flowing_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = readable_state_of(global_object, call)?;
    match slot_of(call, global_object) {
        Some(slot) => Ok(with_slot(slot, |slot| slot.flow.flowing).map_or_else(JSValue::null, JSValue::Bool)),
        None => get_property(global_object, state, "flowing"),
    }
}
host_function!(readable_flowing, readable_flowing_body);

/// Setter `readableFlowing` (nome `set`, 1 argumento): grava em `_readableState.flowing` sem criar chave própria no
/// stream; sem `_readableState` é no-op (medido: `p.set.call({}, true)` não lança). `isPaused()` não muda.
fn set_readable_flowing_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = call.this_value();
    let state = if this.is_undefined_or_null() { JSValue::undefined() } else { get_property(global_object, this, "_readableState")? };
    let Some(state_object) = JSObject::from_value(&state) else {
        return Ok(JSValue::undefined());
    };
    let value = call.argument(0);
    match slot_of(call, global_object) {
        Some(slot) if value.is_boolean() || value.is_null() => {
            with_slot(slot, |slot| slot.flow.flowing = if value.is_null() { None } else { Some(value.as_boolean()) });
        }
        Some(_) => {}
        None => {
            state_object.put_direct(global_object.vm(), &prop(global_object.vm(), "flowing"), value, 0);
        }
    }
    Ok(JSValue::undefined())
}
host_function!(set_readable_flowing, set_readable_flowing_body);

/// Instala o acessor `readableFlowing` em `target` com os nomes `get` e `set` do bun.
fn put_readable_flowing(global_object: &JSGlobalObject, target: &JSObject) {
    let vm = global_object.vm();
    let getter = native(global_object, "get", 0, readable_flowing);
    let setter = native(global_object, "set", 1, set_readable_flowing);
    let accessor = GetterSetter::create_from_values(vm, getter, setter);
    target.put_direct_non_index_accessor_without_transition(vm, &prop(vm, "readableFlowing"), &accessor, DONT_ENUM | DONT_DELETE | ACCESSOR);
}

/// `stdin.read(size?)` sem dados no buffer (EOF imediato): `null` (medido).
fn stdin_read_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::null())
}
host_function!(stdin_read, stdin_read_body);

fn stdin_state_flowing_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(with_slot(0, |slot| slot.flow.flowing).map_or_else(JSValue::null, JSValue::Bool))
}
host_function!(stdin_state_flowing, stdin_state_flowing_body);

fn stdin_state_set_flowing_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    if value.is_boolean() || value.is_null() {
        with_slot(0, |slot| slot.flow.flowing = if value.is_null() { None } else { Some(value.as_boolean()) });
    }
    Ok(JSValue::undefined())
}
host_function!(stdin_state_set_flowing, stdin_state_set_flowing_body);

fn readable_destroyed_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::Bool(false))
}
host_function!(readable_destroyed, readable_destroyed_body);

fn readable_readable_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::Bool(true))
}
host_function!(readable_readable, readable_readable_body);

/// O stream do descritor `fd`: `WriteStream` para 1 e 2, `ReadStream` para 0.
pub(crate) fn create(global_object: &JSGlobalObject, fd: u8) -> JSValue {
    let vm = global_object.vm();
    let stream = construct_empty_object(global_object);
    if fd == 0 {
        stream.set_prototype_direct(vm, shared_prototype(&READ_PROTOTYPE, || build_read_prototype(global_object)));
        let undefined = JSValue::undefined();
        let put = |name: &str, value: JSValue| stream.put_direct(vm, &prop(vm, name), value, 0);
        // Medido com `</dev/null`: sem stdin real o sandbox entrega EOF, e as chaves próprias seguem esta ordem.
        put("fd", js_number(fd));
        put("start", undefined);
        put("end", js_number(f64::INFINITY));
        put("pos", undefined);
        put("bytesRead", js_number(0));
        let events = events_with_keys(global_object, &["close", "error", "data", "end", "readable", "resume", "pause"]);
        if let Some(table) = JSObject::from_value(&events) {
            // Medido: cada um é uma função de nome vazio, `length` 0 e `toString` `function () { [native code] }`,
            // e `listeners(nome)` devolve exatamente uma delas; o mesmo objeto vive em `_events` e na tabela.
            for name in INTERNAL_STDIN_EVENTS {
                let internal = native(global_object, "", 0, undefined_function);
                table.put_direct(vm, &prop(vm, name), internal, 0);
                with_table(0, |listeners| listeners.add(name, internal, false));
            }
        }
        put("_events", events);
        let state = readable_state(global_object);
        if let Some(state_object) = JSObject::from_value(&state) {
            // Medido: `_readableState.flowing` do stdin acompanha `readableFlowing` (`null`, `true` ou `false`), sem ser chave listada.
            put_accessor_with(global_object, &state_object, "flowing", stdin_state_flowing, Some(stdin_state_set_flowing), DONT_ENUM);
        }
        put("_readableState", state);
        put("_maxListeners", undefined);
        // Medido: 5 = os quatro ouvintes internos mais o `kConstruct` (chave símbolo que `_events` não lista).
        put("_eventsCount", js_number(STDIN_BASE_EVENTS_COUNT));
        put("on", native(global_object, "", 2, emitter_on));
        put("addListener", native(global_object, "", 2, emitter_on));
        put("pause", native(global_object, "", 0, stdin_pause));
        put("resume", native(global_object, "", 0, stdin_resume));
        put("read", native(global_object, "", 1, stdin_read));
        put("_read", native(global_object, "triggerRead", 1, undefined_function));
        STDIN_OBJECT.with(|cell| cell.set(Some(stream.as_value())));
        let done = native(global_object, "", 0, stdin_construct_done);
        global_object.vm().default_microtask_queue.enqueue(QueuedTask::new(global_object.cell_id(), InternalMicrotask::InvokeFunctionJob, 0, &[done]));
        return stream.as_value();
    }
    stream.set_prototype_direct(vm, shared_prototype(&WRITE_PROTOTYPE, || build_write_prototype(global_object)));
    let (write, underscore_write): (NativeFunction, NativeFunction) = if fd == 1 { (stdout_write, stdout_write) } else { (stderr_write, stderr_write) };
    let undefined = JSValue::undefined();
    let put = |name: &str, value: JSValue| stream.put_direct(vm, &prop(vm, name), value, 0);
    put("fd", js_number(fd));
    put("_writev", undefined);
    put("flush", JSValue::Bool(false));
    put("start", undefined);
    put("pos", undefined);
    put("bytesWritten", js_number(0));
    put("_write", native(global_object, "underscoreWriteFast", 3, underscore_write));
    put("write", native(global_object, "writeFast", 3, write));
    put("_construct", undefined);
    put("_events", base_events(global_object, false));
    put("_writableState", writable_state(global_object));
    put("_maxListeners", undefined);
    put("readable", JSValue::Bool(false));
    put("_type", text_value(vm, "fs"));
    put("destroySoon", native(global_object, "", 2, destroy_soon));
    put("_destroy", native(global_object, "", 2, stream_destroy));
    put("_final", native(global_object, "", 1, stream_final));
    put("_isStdio", JSValue::Bool(true));
    stream.as_value()
}
