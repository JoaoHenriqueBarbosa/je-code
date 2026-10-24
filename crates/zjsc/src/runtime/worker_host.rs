//! Hospedeiro de `Worker`: uma thread do SO por worker, cada uma com o próprio `VM` e `JSGlobalObject`, como o bun
//! (que sobe uma thread com VM próprio por `Worker`). O `VM` é baseado em `Rc` e não é `Send`, e todo o estado do
//! porte (registro de células, canais, timers) é `thread_local`; por isso a thread constrói o VM do zero e só
//! bytes serializados e dados simples cruzam a fronteira. Nenhum `JSValue`, `Rc` ou célula passa de uma thread a outra.
//!
//! Este módulo é a fatia 1 (ver `PLAN.md`, "Worker"): tipos de mensagem, canais, spawn da thread, criação do VM nela e
//! execução do fonte já resolvido. A classe `Worker` do JS (construtor, `postMessage`, `onmessage`, `terminate`,
//! `ref`/`unref`) e a integração com `run_event_loop` chamam a API pública daqui nas fatias seguintes.
//!
//! Canais (`std::sync::mpsc`, um par por worker):
//! - pai para worker: [`ToWorker`] (`Post(bytes)`, `Terminate`);
//! - worker para pai: [`ToParent`] (`Open`, `Post(bytes)`, `Error(dados)`, `Output`, `Close(código)`).
//!
//! A thread principal não bloqueia: o laço de eventos chama [`WorkerHandle::try_recv`] a cada volta (o mesmo ponto onde
//! chama `broadcast_channel::deliver_pending`) e [`holds_event_loop`] decide se o processo continua vivo.
//!
//! LACUNA (fatias seguintes, nada disso é silencioso no JS porque a classe `Worker` ainda não existe):
//! - os bytes de `Post` são a forma serializada do clone estruturado; `structured_clone.rs` hoje clona de `JSValue`
//!   para `JSValue` e ainda não tem `serialize`/`deserialize` para bytes. A fatia que o cria define o formato e liga
//!   `Post` nos dois lados;
//! - dentro da thread, mensagens `Post` do pai só serão entregues quando o laço de eventos do worker consultar o canal
//!   (hoje, com o laço segurado por um ouvinte, a thread espera só `Terminate` e descarta os `Post`);
//! - `Error(dados)` precisa de um gancho em `uncaught_report.rs` que entregue os dados estruturados da exceção, não só
//!   o texto do stderr; hoje o texto do relato segue pelo `Output` de stderr e o código de saída pelo `Close`;
//! - `Terminate` interrompe a thread só quando ela está esperando; um laço de JS em curso precisa checar a bandeira
//!   [`WorkerHandle::terminate_flag`] (a fatia de integração com `timers.rs` a lê).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::runtime::console_host::ConsoleHost;
use crate::runtime::event_target::{create_error_event, create_message_event, create_trusted_event, dispatch_event};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise::{JSPromise, JSPromiseRef};
use crate::runtime::js_promise_host::PromiseHost;
use crate::runtime::js_value::{js_null, js_number, js_undefined, EncodedJSValue, JSValue};
use crate::runtime::structured_clone::value_wire;

/// O próximo `threadId` (o do principal é 0; o primeiro worker é 1, como no bun).
static NEXT_THREAD_ID: AtomicU32 = AtomicU32::new(1);

/// Pilha da thread do worker. O interpretador recursa fundo (parser, bytecode, chamadas JS) e o padrão de 2 MiB do
/// `std::thread` é pequeno; o valor é uma reserva de espaço de endereço (não é memória residente).
/// A conferir na compilação: se o binário principal usa outro tamanho, alinhar com ele.
const WORKER_STACK_BYTES: usize = 256 * 1024 * 1024;

/// Mensagem do pai para o worker.
#[derive(Debug)]
pub enum ToWorker {
    /// `worker.postMessage(valor)`: a forma serializada do clone estruturado do valor.
    Post(Vec<u8>),
    /// `worker.terminate()`: a thread encerra e responde com [`ToParent::Close`].
    Terminate,
}

/// Dados de uma exceção não tratada no worker, o que o evento `error` (`ErrorEvent`) do pai precisa.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerErrorData {
    /// `error.message`.
    pub message: String,
    /// Nome do arquivo do worker onde a exceção nasceu.
    pub filename: String,
    /// Linha (1 a partir de 1) e coluna da exceção; 0 quando desconhecidas.
    pub line: u32,
    pub column: u32,
    /// `error.name` e `error.stack`, para o pai reconstruir o objeto de erro; vazios quando o valor lançado não era um `Error`.
    pub name: String,
    pub stack: String,
    /// O valor lançado quando não é um `Error`, já serializado como no `Post`.
    pub thrown: Option<Vec<u8>>,
}

/// Mensagem do worker para o pai.
#[derive(Debug)]
pub enum ToParent {
    /// A thread está de pé e o VM foi criado: o pai despacha o evento `open` antes da primeira mensagem.
    Open,
    /// `self.postMessage(valor)` / `parentPort.postMessage(valor)`, serializado.
    Post(Vec<u8>),
    /// Exceção não tratada no worker: vira o evento `error` no pai.
    Error(WorkerErrorData),
    /// Bytes escritos no stdout (`stderr == false`) ou no stderr do worker; o pai os repassa ao console dele.
    Output { stderr: bool, bytes: Vec<u8> },
    /// A thread acabou com esse código de saída; o pai despacha `close` e solta o worker.
    Close(i32),
}

/// O que `new Worker(...)` já resolveu antes de subir a thread.
#[derive(Debug, Clone)]
pub struct WorkerSpec {
    /// O fonte do script do worker, já lido e resolvido pelo pai (arquivo, `data:` ou `blob:`).
    pub source: String,
    /// A URL/caminho do script, que vira o nome de arquivo das pilhas e do `import.meta.url`.
    pub url: String,
    /// `options.name`, vazio por padrão; vira o nome da thread do SO.
    pub name: String,
}

/// O lado do pai de um worker vivo.
#[derive(Debug)]
pub struct WorkerHandle {
    thread_id: u32,
    to_worker: Sender<ToWorker>,
    from_worker: Receiver<ToParent>,
    join: Option<JoinHandle<()>>,
    terminate_flag: Arc<AtomicBool>,
    refed: bool,
    closed: bool,
}

/// Sobe a thread do worker. Falha só se o SO não conseguir criar a thread.
pub fn spawn(spec: WorkerSpec) -> std::io::Result<WorkerHandle> {
    let thread_id = NEXT_THREAD_ID.fetch_add(1, Ordering::SeqCst);
    let (to_worker, worker_inbox) = mpsc::channel::<ToWorker>();
    let (parent_tx, from_worker) = mpsc::channel::<ToParent>();
    let terminate_flag = Arc::new(AtomicBool::new(false));
    let worker_flag = Arc::clone(&terminate_flag);
    let thread_name = if spec.name.is_empty() { format!("worker-{thread_id}") } else { spec.name.clone() };
    let join = std::thread::Builder::new()
        .name(thread_name)
        .stack_size(WORKER_STACK_BYTES)
        .spawn(move || run_worker_thread(spec, worker_inbox, parent_tx, worker_flag))?;
    Ok(WorkerHandle { thread_id, to_worker, from_worker, join: Some(join), terminate_flag, refed: true, closed: false })
}

impl WorkerHandle {
    /// O `worker.threadId`.
    pub fn thread_id(&self) -> u32 {
        self.thread_id
    }

    /// `worker.postMessage`: entrega os bytes serializados à thread. `false` quando o worker já acabou
    /// (a mensagem se perde, como no bun depois de `terminate`).
    pub fn post(&self, bytes: Vec<u8>) -> bool {
        !self.closed && self.to_worker.send(ToWorker::Post(bytes)).is_ok()
    }

    /// `worker.terminate()`: levanta a bandeira (lida pelo laço do worker) e pede o encerramento. Idempotente.
    pub fn terminate(&self) {
        self.terminate_flag.store(true, Ordering::SeqCst);
        // Thread que já saiu: o `send` falha e não há mais nada a avisar.
        let _ = self.to_worker.send(ToWorker::Terminate);
    }

    /// A bandeira de encerramento, que o laço de eventos do worker consulta entre tarefas.
    pub fn terminate_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.terminate_flag)
    }

    /// Próxima mensagem do worker, sem bloquear. Depois de `Close` (ou da thread morrer sem avisar) o worker fica
    /// fechado: a thread é juntada e as chamadas seguintes devolvem `None`.
    pub fn try_recv(&mut self) -> Option<ToParent> {
        if self.closed {
            return None;
        }
        let message = match self.from_worker.try_recv() {
            Ok(message) => message,
            Err(TryRecvError::Empty) => return None,
            // A thread terminou sem enviar `Close` (pânico): vira um fechamento com código 1.
            Err(TryRecvError::Disconnected) => ToParent::Close(1),
        };
        if matches!(message, ToParent::Close(_)) {
            self.closed = true;
            if let Some(join) = self.join.take() {
                // Pânico da thread já virou o `Close(1)` acima; o resultado do join não traz mais nada.
                let _ = join.join();
            }
        }
        Some(message)
    }

    /// `worker.ref()` / `worker.unref()`.
    pub fn set_refed(&mut self, refed: bool) {
        self.refed = refed;
    }

    /// `true` quando este worker segura o processo do pai vivo: aberto e com `ref` (o padrão).
    pub fn holds_event_loop(&self) -> bool {
        !self.closed && self.refed
    }

    /// `true` depois do `Close` entregue por [`WorkerHandle::try_recv`].
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

/// O console do worker: tudo o que ele escreve segue para o pai como [`ToParent::Output`]; não há stdin.
#[derive(Debug)]
struct ChannelConsole {
    parent: Sender<ToParent>,
}

impl ConsoleHost for ChannelConsole {
    fn write_stdout(&self, bytes: &[u8]) {
        // Pai que já soltou o worker não ouve mais: a saída se perde.
        let _ = self.parent.send(ToParent::Output { stderr: false, bytes: bytes.to_vec() });
    }

    fn write_stderr(&self, bytes: &[u8]) {
        let _ = self.parent.send(ToParent::Output { stderr: true, bytes: bytes.to_vec() });
    }

    fn read_stdin_line(&self) -> Option<String> {
        None
    }

    fn read_stdin_rest(&self) -> Vec<u8> {
        Vec::new()
    }
}

/// O corpo da thread: cria o VM nela (mesmo caminho de `evaluate_main_script`: `run_program`, `new_global_object`,
/// console instalado, relatores de exceção, script, microtarefas e laço de eventos), roda o fonte e responde `Close`.
fn run_worker_thread(spec: WorkerSpec, inbox: Receiver<ToWorker>, parent: Sender<ToParent>, terminate_flag: Arc<AtomicBool>) {
    let _ = parent.send(ToParent::Open);
    let console: Rc<dyn ConsoleHost> = Rc::new(ChannelConsole { parent: parent.clone() });
    let (code, held) = if terminate_flag.load(Ordering::SeqCst) {
        // `terminate()` chamado antes de a thread começar a rodar o script.
        (0, false)
    } else {
        crate::api::eval::evaluate_main_script_reporting_hold(&spec.source, &spec.url, console)
    };
    if held && !terminate_flag.load(Ordering::SeqCst) {
        // O script terminou mas um ouvinte de `message` (ou fonte com `ref`) segura o worker vivo: ele só acaba por
        // `Terminate` ou quando o pai solta o canal. Os `Post` ainda não são entregues (ver LACUNA no cabeçalho).
        wait_for_terminate(&inbox);
    }
    let _ = parent.send(ToParent::Close(code));
}

/// Bloqueia até chegar `Terminate` ou até o pai soltar o canal.
fn wait_for_terminate(inbox: &Receiver<ToWorker>) {
    while let Ok(message) = inbox.recv() {
        if matches!(message, ToWorker::Terminate) {
            return;
        }
    }
}

/// Um `Worker` do pai: a instância JS, o lado do pai da thread (ausente quando a resolução do script falhou) e o que
/// o pai ainda deve a ela.
struct Entry {
    instance: EncodedJSValue,
    /// `None` quando o script não pôde ser resolvido: não há thread, só o evento `error` a entregar.
    handle: Option<WorkerHandle>,
    /// A falha de resolução, entregue como evento `error` na primeira volta do laço e seguida de `close`.
    resolve_failure: Option<String>,
    /// A promise que `terminate()` devolveu, resolvida quando o `Close` chega.
    terminate_promise: Option<JSPromiseRef>,
    refed: bool,
}

thread_local! {
    /// Os `Worker` vivos do pai, na ordem de criação.
    static WORKERS: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

/// Teto de mensagens entregues por worker numa chamada de [`deliver_pending`].
const MAX_MESSAGES_PER_WORKER: usize = 1_000_000;

/// Fim do programa: os workers e as promises guardam objetos do programa. A thread de cada um é sinalizada para encerrar.
pub(crate) fn reset_for_program() {
    let entries = WORKERS.try_with(|workers| std::mem::take(&mut *workers.borrow_mut()));
    if let Ok(entries) = entries {
        for entry in &entries {
            if let Some(handle) = &entry.handle {
                handle.terminate();
            }
        }
        drop(entries);
    }
}

/// Registra um `Worker` recém-construído. Com `Err(mensagem)` não há thread: o `error` sai na próxima volta do laço.
pub(crate) fn register(instance: JSValue, spawned: Result<WorkerHandle, String>) {
    let entry = match spawned {
        Ok(handle) => Entry { instance: instance.encode(), handle: Some(handle), resolve_failure: None, terminate_promise: None, refed: true },
        Err(message) => Entry { instance: instance.encode(), handle: None, resolve_failure: Some(message), terminate_promise: None, refed: true },
    };
    WORKERS.with(|workers| workers.borrow_mut().push(entry));
}

fn with_entry<R>(instance: JSValue, f: impl FnOnce(&mut Entry) -> R) -> Option<R> {
    WORKERS.with(|workers| workers.borrow_mut().iter_mut().find(|entry| entry.instance == instance.encode()).map(f))
}

/// `true` quando `instance` é um `Worker` criado por este módulo (ainda vivo ou não fechado).
pub(crate) fn is_worker(instance: JSValue) -> bool {
    with_entry(instance, |_| ()).is_some()
}

/// `worker.threadId`: o id da thread, ou 0 quando a resolução do script falhou e não houve thread.
pub(crate) fn thread_id_of(instance: JSValue) -> Option<u32> {
    with_entry(instance, |entry| entry.handle.as_ref().map_or(0, WorkerHandle::thread_id))
}

/// `worker.postMessage`: entrega os bytes serializados. A mensagem se perde quando o worker já acabou.
pub(crate) fn post_to(instance: JSValue, bytes: Vec<u8>) {
    with_entry(instance, |entry| entry.handle.as_ref().map(|handle| handle.post(bytes)));
}

/// `worker.ref()` / `worker.unref()`.
pub(crate) fn set_ref(instance: JSValue, refed: bool) {
    with_entry(instance, |entry| {
        entry.refed = refed;
        if let Some(handle) = entry.handle.as_mut() {
            handle.set_refed(refed);
        }
    });
}

/// `worker.terminate()`: pede o encerramento da thread e devolve a promise que o `Close` resolve. Se o worker já
/// fechou (ou nunca teve thread), a promise nasce resolvida.
pub(crate) fn terminate(global_object: &JSGlobalObject, instance: JSValue) -> JSValue {
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    let pending = with_entry(instance, |entry| match &entry.handle {
        Some(handle) if !handle.is_closed() => {
            handle.terminate();
            entry.terminate_promise = Some(promise.clone());
            true
        }
        _ => false,
    })
    .unwrap_or(false);
    if !pending {
        promise.resolve(global_object, js_undefined());
    }
    promise.as_value()
}

/// `true` quando algum worker segura o processo do pai: vivo e com `ref`, ou com a falha de resolução ainda por entregar.
pub(crate) fn holds_event_loop() -> bool {
    WORKERS.with(|workers| {
        workers.borrow().iter().any(|entry| entry.resolve_failure.is_some() || entry.handle.as_ref().is_some_and(|handle| entry.refed && handle.holds_event_loop()))
    })
}

/// O que o pai fez com uma mensagem do filho.
enum Step {
    /// Nada a ler agora.
    Idle,
    /// Uma mensagem foi tratada.
    Done,
}

/// Despacha `kind` (`open`, `close`, `messageerror`) em `target` e esvazia as microtasks.
fn dispatch_plain(global_object: &JSGlobalObject, target: JSValue, kind: &str) {
    if let Ok(event) = create_trusted_event(global_object, kind) {
        // A exceção de um ouvinte já foi para o relatório de erros não capturados dentro de `dispatch_event`.
        let _ = dispatch_event(global_object, target, event);
    }
    global_object.vm().drain_microtasks();
}

fn dispatch_error(global_object: &JSGlobalObject, target: JSValue, data: &WorkerErrorData) {
    let error = match &data.thrown {
        Some(bytes) => value_wire::deserialize(global_object, bytes, &[], &[]).unwrap_or_else(|_| js_null()),
        None => js_null(),
    };
    if let Ok(event) = create_error_event(global_object, &data.message, &data.filename, data.line, data.column, error) {
        let _ = dispatch_event(global_object, target, event);
    }
    global_object.vm().drain_microtasks();
}

fn dispatch_post(global_object: &JSGlobalObject, target: JSValue, bytes: &[u8]) {
    match value_wire::deserialize(global_object, bytes, &[], &[]) {
        Ok(data) => {
            if let Ok(event) = create_message_event(global_object, data, &[]) {
                let _ = dispatch_event(global_object, target, event);
            }
            global_object.vm().drain_microtasks();
        }
        Err(_) => dispatch_plain(global_object, target, "messageerror"),
    }
}

/// Trata uma mensagem do filho de `instance`, se houver; o fim do worker (`Close`) tira a entrada do registro.
fn deliver_one(global_object: &JSGlobalObject, instance: EncodedJSValue) -> Step {
    let target = JSValue::decode(instance);
    let received = WORKERS.with(|workers| {
        let mut workers = workers.borrow_mut();
        let entry = workers.iter_mut().find(|entry| entry.instance == instance)?;
        if let Some(message) = entry.resolve_failure.take() {
            return Some(Received::ResolveFailure(message));
        }
        entry.handle.as_mut()?.try_recv().map(Received::Message)
    });
    let Some(received) = received else { return Step::Idle };
    match received {
        Received::ResolveFailure(message) => {
            let data = WorkerErrorData { message, ..WorkerErrorData::default() };
            dispatch_error(global_object, target, &data);
            finish(global_object, instance, target, 1);
        }
        Received::Message(ToParent::Open) => dispatch_plain(global_object, target, "open"),
        Received::Message(ToParent::Post(bytes)) => dispatch_post(global_object, target, &bytes),
        Received::Message(ToParent::Error(data)) => dispatch_error(global_object, target, &data),
        Received::Message(ToParent::Output { stderr, bytes }) => {
            if let Some(console) = global_object.console_host() {
                if stderr {
                    console.write_stderr(&bytes);
                } else {
                    console.write_stdout(&bytes);
                }
            }
        }
        Received::Message(ToParent::Close(code)) => finish(global_object, instance, target, code),
    }
    Step::Done
}

enum Received {
    ResolveFailure(String),
    Message(ToParent),
}

/// O worker acabou: `close` no objeto, a promise de `terminate()` resolvida e a entrada solta.
fn finish(global_object: &JSGlobalObject, instance: EncodedJSValue, target: JSValue, code: i32) {
    let promise = WORKERS.with(|workers| {
        let mut workers = workers.borrow_mut();
        let index = workers.iter().position(|entry| entry.instance == instance)?;
        workers.remove(index).terminate_promise
    });
    dispatch_plain(global_object, target, "close");
    if let Some(promise) = promise {
        promise.resolve(global_object, js_number(f64::from(code)));
        global_object.vm().drain_microtasks();
    }
}

/// A tarefa do host que entrega o que os filhos mandaram (chamada por `timers.rs::run_event_loop_until_held` nos
/// mesmos pontos de `broadcast_channel::deliver_pending`). Devolve `true` quando tratou alguma mensagem.
pub(crate) fn deliver_pending(global_object: &JSGlobalObject) -> bool {
    let mut progressed = false;
    let instances: Vec<EncodedJSValue> = WORKERS.with(|workers| workers.borrow().iter().map(|entry| entry.instance).collect());
    for instance in instances {
        for _ in 0..MAX_MESSAGES_PER_WORKER {
            match deliver_one(global_object, instance) {
                Step::Idle => break,
                Step::Done => progressed = true,
            }
        }
    }
    progressed
}
