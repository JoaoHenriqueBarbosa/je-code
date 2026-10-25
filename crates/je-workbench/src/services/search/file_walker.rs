//! Porte de `src/vs/workbench/services/search/node/fileSearch.ts`: o `FileWalker`, que lista os arquivos de cada pasta da
//! consulta por um processo filho (o ripgrep) e aplica o padrão de arquivo, o de inclusão, o de exclusão e os irmãos
//! (`siblingClauses`), e o `Engine` que o expõe como motor de busca.
//!
//! Diferenças de forma em relação ao original:
//! - O processo filho do Node (`childProcess.ChildProcess`) é o [`ChildProcess`], sobre `std::process::Command`: duas
//!   threads leem o stdout e o stderr e o laço de eventos do porte ([`runtime`]) os entrega por um timer de 1 ms, na
//!   ordem do Node (dados, `error`, `close` depois de os dois fluxos fecharem e o processo sair). Um `spawn` que falha
//!   (executável inexistente) não devolve erro na hora: como no Node, o `error` e o `close` chegam depois, pelo ouvinte.
//! - O stdin do filho é nulo (no Node é um pipe que o ripgrep e o `find` nunca leem). `kill()` usa o `Child::kill` do
//!   `std`, que envia SIGKILL no Unix e `TerminateProcess` no Windows (o `kill()` do Node envia SIGTERM; enviar outro
//!   sinal exigiria `unsafe`). `killed` vale o mesmo: verdadeiro quando o envio deu certo com o processo ainda vivo.
//! - `process.on('exit', () => killCmds.forEach(...))` não tem equivalente em Rust (não há gancho de saída sem `unsafe`
//!   ou `libc`): o conjunto global continua existindo, [`kill_all_cmds`] o percorre (o hospedeiro chama no encerramento) e
//!   o `Drop` do [`ChildProcess`] mata e recolhe o filho que ainda estiver vivo, de modo que nenhum fica órfão.
//! - `spawnRipgrepCmd` (`ripgrepFileSearch.ts`) é [`spawn_ripgrep_cmd`]: o `await rgDiskPath()` é o caminho do binário
//!   recebido (o `base/node/ripgrep.ts` não faz parte do porte), e o resto (`getRgArgs`, `cwd`, `spawn`) é igual. O
//!   argumento padrão `spawnRipgrep = spawnRipgrepCmd` do construtor é um argumento obrigatório de [`FileWalker::new`].
//! - O `encoding: BufferEncoding` de `collectStdout`/`readStdout` é sempre UTF-8 (o `StringDecoder('utf8')` é o
//!   [`Utf8Decoder`], que guarda a sequência incompleta do fim de um bloco para o próximo, como o original).
//! - `(err, stdout?, last?)` dos retornos de chamada é `(Option<Error>, Option<&str>, bool)`; `readStdout` entrega um
//!   `Result<String, Error>`. O `done(error | null, isLimitHit)` é `(Option<Error>, bool)`.
//! - `async cmdTraversal` é um método que encadeia a [`Promise`] do `spawnRipgrep` com `then_result`; o `try/catch` de
//!   `call()` em volta dele não tem o que capturar (o corpo `async` nunca lança de forma síncrona), então `call` não
//!   existe.
//! - `IDirectoryEntry extends IRawFileMatch` é o [`DirectoryEntry`] (com `base`, `basename` e `searchPath` sempre
//!   presentes); `rootEntries` e `pathToEntries['.']` são o mesmo array no original, e aqui só existe o mapa (a raiz é a
//!   chave `.`). `hasSiblingFn(() => entries.map(...))` recebe os nomes já coletados da pasta.
//! - `ISearchEngine<T>` e `ISearchEngineSuccess` (de `common/search.ts`, ainda sem porte em `search.rs`) ficam aqui, só com
//!   o que o `Engine` usa. `walker.getStats()` sem `cmdSW` ou `fileWalkSW` (busca nunca iniciada) é o `TypeError` do `!`
//!   do original, aqui um pânico.
//! - O `console.error(errorMessage)` de `walk` é `eprintln!`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use je_base::dynamic::{Dynamic, json_stringify};
use je_base::error_message::to_error_message_of;
use je_base::errors::Error;
use je_base::extpath;
use je_base::fuzzy_scorer::prepare_query;
use je_base::glob::{self, ExprMatch, ExprValue, Expression, GlobOptions, HasSibling, ParsedExpression, Sibling};
use je_base::normalization::normalize_nfc;
use je_base::path::PathModule;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::stopwatch::StopWatch;
use je_base::strings;
use je_base::uri::URI;
use je_platform::resources::{fs_path, host_environment};

use super::ripgrep_file_search::{RgArgs, get_rg_args};
use super::ripgrep_search_utils::{MAX_RIPGREP_SYMLINK_LOOPS, RipgrepSymlinkLoopDetector};
use super::search::{
    IFileQuery, IFolderQuery, IProgressMessage, IRawFileMatch, ISearchEngineStats, ITextSearchCompleteMessage, expr_match_is_truthy, has_sibling_fn, is_file_pattern_match,
};

// ---------------------------------------------------------------------------------------------------------
// O processo filho (`childProcess.ChildProcess`)
// ---------------------------------------------------------------------------------------------------------

/// O intervalo, em milissegundos, com que o laço de eventos confere o que as threads de leitura entregaram.
const POLL_INTERVAL_MS: u64 = 1;

/// O que as threads de leitura mandam para o laço de eventos.
enum StreamChunk {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    StdoutEnd,
    StderrEnd,
}

#[derive(Clone, Copy)]
enum StreamKind {
    Stdout,
    Stderr,
}

/// Os eventos do `ChildProcess` que o `FileWalker` escuta: `stdout.on('data')`, `stderr.on('data')`, `on('error')` e
/// `on('close')` (o código é `None` quando o processo morreu por sinal ou nem chegou a existir).
pub enum ChildEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Error(Error),
    Close(Option<i32>),
}

type ChildListener = Box<dyn FnMut(&ChildEvent)>;

/// `childProcess.ChildProcess`: o filho com stdout e stderr em pipe.
pub struct ChildProcess {
    child: RefCell<Option<Child>>,
    pid: Option<u32>,
    killed: Cell<bool>,
    chunks: Receiver<StreamChunk>,
    open_streams: Cell<u8>,
    stdout_piped: bool,
    stderr_piped: bool,
    spawn_error: RefCell<Option<Error>>,
    listener: RefCell<Option<ChildListener>>,
    polling: Cell<bool>,
    closed: Cell<bool>,
}

/// O código de erro do Node (`ENOENT`, `EACCES`) de uma falha de `spawn`.
fn io_error_code(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT".to_owned(),
        std::io::ErrorKind::PermissionDenied => "EACCES".to_owned(),
        _ => error.to_string(),
    }
}

/// Lê o fluxo numa thread e manda cada bloco (e o fim) para o laço de eventos.
fn pump(mut stream: impl Read + Send + 'static, sender: Sender<StreamChunk>, kind: StreamKind) {
    std::thread::spawn(move || {
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
                Ok(read) => {
                    let data = buffer[..read].to_vec();
                    let chunk = match kind {
                        StreamKind::Stdout => StreamChunk::Stdout(data),
                        StreamKind::Stderr => StreamChunk::Stderr(data),
                    };
                    if sender.send(chunk).is_err() {
                        return;
                    }
                }
            }
        }
        let _ = sender.send(match kind {
            StreamKind::Stdout => StreamChunk::StdoutEnd,
            StreamKind::Stderr => StreamChunk::StderrEnd,
        });
    });
}

impl ChildProcess {
    /// `childProcess.spawn(command, args, { cwd })`. Uma falha de `spawn` não é devolvida aqui: o `error` e o `close`
    /// chegam ao ouvinte, como no Node.
    pub fn spawn(command: &mut Command) -> Rc<Self> {
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let program = command.get_program().to_string_lossy().into_owned();
        let (sender, chunks) = mpsc::channel();
        match command.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                let stdout = child.stdout.take();
                let stderr = child.stderr.take();
                let (stdout_piped, stderr_piped) = (stdout.is_some(), stderr.is_some());
                if let Some(stdout) = stdout {
                    pump(stdout, sender.clone(), StreamKind::Stdout);
                }
                if let Some(stderr) = stderr {
                    pump(stderr, sender, StreamKind::Stderr);
                }
                Rc::new(Self {
                    child: RefCell::new(Some(child)),
                    pid: Some(pid),
                    killed: Cell::new(false),
                    chunks,
                    open_streams: Cell::new(u8::from(stdout_piped) + u8::from(stderr_piped)),
                    stdout_piped,
                    stderr_piped,
                    spawn_error: RefCell::new(None),
                    listener: RefCell::new(None),
                    polling: Cell::new(false),
                    closed: Cell::new(false),
                })
            }
            Err(error) => Rc::new(Self {
                child: RefCell::new(None),
                pid: None,
                killed: Cell::new(false),
                chunks,
                open_streams: Cell::new(0),
                stdout_piped: true,
                stderr_piped: true,
                spawn_error: RefCell::new(Some(Error::new(format!("spawn {program} {}", io_error_code(&error))))),
                listener: RefCell::new(None),
                polling: Cell::new(false),
                closed: Cell::new(false),
            }),
        }
    }

    /// `cmd.pid`: `None` quando o `spawn` falhou (o `undefined` do Node).
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// `cmd.killed`.
    pub fn killed(&self) -> bool {
        self.killed.get()
    }

    /// `cmd.stdout !== null`.
    pub fn has_stdout(&self) -> bool {
        self.stdout_piped
    }

    /// `cmd.stderr !== null`.
    pub fn has_stderr(&self) -> bool {
        self.stderr_piped
    }

    /// `cmd.kill()`: verdadeiro se o envio deu certo com o processo ainda vivo.
    pub fn kill(&self) -> bool {
        let mut slot = self.child.borrow_mut();
        let Some(child) = slot.as_mut() else {
            return false;
        };
        if !matches!(child.try_wait(), Ok(None)) {
            return false;
        }
        let sent = child.kill().is_ok();
        if sent {
            self.killed.set(true);
        }
        sent
    }

    /// Liga o ouvinte dos eventos e começa a entregá-los. O que o filho já escreveu antes fica na fila e chega primeiro.
    pub fn listen(self: &Rc<Self>, listener: impl FnMut(&ChildEvent) + 'static) {
        *self.listener.borrow_mut() = Some(Box::new(listener));
        if !self.polling.replace(true) {
            self.schedule_poll();
        }
    }

    fn schedule_poll(self: &Rc<Self>) {
        let this = self.clone();
        runtime::set_timeout(move || this.poll(), POLL_INTERVAL_MS);
    }

    fn emit(&self, event: &ChildEvent) {
        if let Some(listener) = self.listener.borrow_mut().as_mut() {
            listener(event);
        }
    }

    fn finish(&self, code: Option<i32>) {
        self.closed.set(true);
        self.emit(&ChildEvent::Close(code));
        self.listener.borrow_mut().take();
    }

    fn poll(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        let spawn_error = self.spawn_error.borrow_mut().take();
        if let Some(error) = spawn_error {
            self.emit(&ChildEvent::Error(error));
            self.finish(None);
            return;
        }
        loop {
            match self.chunks.try_recv() {
                Ok(StreamChunk::Stdout(data)) => self.emit(&ChildEvent::Stdout(data)),
                Ok(StreamChunk::Stderr(data)) => self.emit(&ChildEvent::Stderr(data)),
                Ok(StreamChunk::StdoutEnd | StreamChunk::StderrEnd) => self.open_streams.set(self.open_streams.get().saturating_sub(1)),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.open_streams.set(0);
                    break;
                }
            }
        }
        if self.open_streams.get() == 0 {
            let status = self.child.borrow_mut().as_mut().and_then(|child| child.try_wait().ok().flatten());
            if let Some(status) = status {
                self.finish(status.code());
                return;
            }
        }
        self.schedule_poll();
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        if let Some(child) = self.child.get_mut().as_mut()
            && matches!(child.try_wait(), Ok(None))
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// O `StringDecoder('utf8')`: `write` devolve o texto completo e guarda a sequência incompleta do fim para o próximo
/// bloco; byte inválido vira U+FFFD.
#[derive(Default)]
struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    fn write(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut decoded = String::new();
        let mut consumed = 0;
        loop {
            match std::str::from_utf8(&self.pending[consumed..]) {
                Ok(valid) => {
                    decoded.push_str(valid);
                    consumed = self.pending.len();
                    break;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    decoded.push_str(std::str::from_utf8(&self.pending[consumed..consumed + valid_up_to]).unwrap_or_default());
                    consumed += valid_up_to;
                    match error.error_len() {
                        Some(invalid_len) => {
                            decoded.push('\u{FFFD}');
                            consumed += invalid_len;
                        }
                        None => break,
                    }
                }
            }
        }
        self.pending.drain(..consumed);
        decoded
    }
}

// ---------------------------------------------------------------------------------------------------------
// Os comandos vivos (`killCmds`)
// ---------------------------------------------------------------------------------------------------------

thread_local! {
    /// O `killCmds` do módulo: os comandos de todos os `FileWalker` da thread.
    static KILL_CMDS: RefCell<BTreeMap<u64, Rc<dyn Fn()>>> = const { RefCell::new(BTreeMap::new()) };
    static NEXT_KILL_ID: Cell<u64> = const { Cell::new(0) };
}

/// O que o `process.on('exit')` do original faz: mata todos os comandos que ainda estão registrados.
pub fn kill_all_cmds() {
    let cmds: Vec<Rc<dyn Fn()>> = KILL_CMDS.with(|cmds| cmds.borrow().values().cloned().collect());
    for kill in cmds {
        kill();
    }
}

// ---------------------------------------------------------------------------------------------------------
// O ripgrep
// ---------------------------------------------------------------------------------------------------------

/// O que `spawnRipgrepCmd` devolve.
#[derive(Clone)]
pub struct RipgrepCmd {
    pub cmd: Rc<ChildProcess>,
    pub rg_disk_path: String,
    pub sibling_clauses: Expression,
    pub rg_args: RgArgs,
    pub cwd: String,
}

/// `SpawnRipgrepCmd = typeof spawnRipgrepCmd`.
pub type SpawnRipgrepCmd = Rc<dyn Fn(&IFileQuery, &IFolderQuery, Option<&Expression>, Option<&Expression>, Option<i64>) -> Promise<RipgrepCmd>>;

/// `spawnRipgrepCmd`, com o caminho do binário do ripgrep (o `rgDiskPath()` do original) já resolvido.
pub fn spawn_ripgrep_cmd(rg_disk_path: impl Into<String>) -> SpawnRipgrepCmd {
    let rg_disk_path = rg_disk_path.into();
    Rc::new(
        move |config: &IFileQuery, folder_query: &IFolderQuery, include_pattern: Option<&Expression>, exclude_pattern: Option<&Expression>, num_threads: Option<i64>| {
            let rg_args = get_rg_args(config, folder_query, include_pattern, exclude_pattern, num_threads);
            let cwd = fs_path(&folder_query.folder);
            let cmd = ChildProcess::spawn(Command::new(&rg_disk_path).args(&rg_args.args).current_dir(&cwd));
            Promise::resolved(RipgrepCmd { cmd, rg_disk_path: rg_disk_path.clone(), sibling_clauses: rg_args.sibling_clauses.clone(), rg_args, cwd })
        },
    )
}

/// `OnResult`: `(result: IRawFileMatch) => void`.
pub type OnResult = Rc<dyn Fn(&IRawFileMatch)>;

/// `(message: IProgressMessage) => void`.
pub type OnMessage = Rc<dyn Fn(&IProgressMessage)>;

/// O `cb: (err?: Error) => void` de `cmdTraversal`.
type TraversalCallback = Rc<dyn Fn(Option<Error>)>;

/// O `cb: (err, stdout?, last?) => void` de `collectStdout`.
type CollectCallback = Rc<dyn Fn(Option<Error>, Option<&str>, bool)>;

/// Os argumentos de `cmdTraversal`.
#[derive(Clone)]
struct TraversalArgs {
    folder_query: IFolderQuery,
    num_threads: Option<i64>,
    on_result: OnResult,
    on_message: OnMessage,
    cb: TraversalCallback,
    /// Relative paths that were already reported by an earlier traversal of the folder.
    paths_to_skip: Option<Rc<HashSet<String>>>,
}

/// `IDirectoryEntry`.
struct DirectoryEntry {
    base: String,
    relative_path: String,
    basename: String,
    search_path: String,
}

impl DirectoryEntry {
    /// A entrada como o `IRawFileMatch` que o `matchFile` recebe.
    fn to_raw(&self) -> IRawFileMatch {
        IRawFileMatch { base: Some(self.base.clone()), relative_path: self.relative_path.clone(), search_path: Some(self.search_path.clone()) }
    }
}

/// `IDirectoryTree`: o `rootEntries` é `path_to_entries["."]`.
type DirectoryTree = HashMap<String, Vec<DirectoryEntry>>;

fn initial_directory_tree() -> DirectoryTree {
    let mut tree = DirectoryTree::new();
    tree.insert(".".to_owned(), Vec::new());
    tree
}

/// O `with` do `path` nativo do sistema.
fn with_native_path<R>(f: impl FnOnce(&PathModule<'_>) -> R) -> R {
    let env = host_environment();
    f(&PathModule::native(env.as_ref()))
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `strings.equals(a, b, ignoreCase)` sobre texto.
fn strings_equals(a: &str, b: &str, ignore_case: Option<bool>) -> bool {
    strings::equals(Some(&utf16(a)), Some(&utf16(b)), ignore_case.unwrap_or(false))
}

/// `Object.assign(target, source || {})`.
fn assign(target: &mut Expression, source: Option<&Expression>) {
    if let Some(source) = source {
        for (pattern, value) in source.entries() {
            target.insert(pattern.clone(), value.clone());
        }
    }
}

/// O `trim()` do JavaScript (espaços Unicode, terminadores de linha e a marca de ordem de bytes).
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// O `JSON.stringify` dos `siblingClauses`.
fn sibling_clauses_json(expression: &Expression) -> String {
    let entries: Vec<(String, Dynamic)> = expression
        .entries()
        .iter()
        .map(|(pattern, value)| {
            let value = match value {
                ExprValue::Bool(enabled) => Dynamic::Bool(*enabled),
                ExprValue::When(when) => Dynamic::object([("when", Dynamic::string(when))]),
                ExprValue::Other => Dynamic::object(Vec::<(&str, Dynamic)>::new()),
            };
            (pattern.clone(), value)
        })
        .collect();
    json_stringify(&Dynamic::object(entries)).unwrap_or_default()
}

/// A palavra do original que decide se o stderr do ripgrep é um erro a mostrar.
fn uppercase_first(text: &str) -> String {
    strings::uppercase_first_letter(&utf16(text)).to_string_lossy()
}

/// `rgErrorMsgForDisplay`.
fn rg_error_msg_for_display(msg: &str) -> Option<String> {
    let trimmed = js_trim(msg);
    let lines: Vec<&str> = trimmed.split('\n').collect();
    let first_line = js_trim(lines[0]);

    if first_line.starts_with("Error parsing regex") {
        return Some(first_line.to_owned());
    }

    if first_line.starts_with("regex parse error") {
        return Some(uppercase_first(js_trim(lines[lines.len() - 1])));
    }

    if first_line.starts_with("error parsing glob") || first_line.starts_with("unsupported encoding") {
        // Uppercase first letter
        return Some(uppercase_first(first_line));
    }

    if first_line == "Literal '\\n' not allowed." {
        // I won't localize this because none of the Ripgrep error messages are localized
        return Some("Literal '\\n' currently not supported".to_owned());
    }

    if first_line.starts_with("Literal ") {
        // Other unsupported chars
        return Some(first_line.to_owned());
    }

    None
}

/// `AbsoluteAndRelativeParsedExpression`: uma interface sobre duas `ParsedExpression`, uma para as expressões absolutas e
/// outra para as relativas. Elas não "precisam" ficar separadas, mas assim não se faz `path.join` para cada arquivo
/// buscado.
struct AbsoluteAndRelativeParsedExpression {
    expression: Expression,
    root: String,
    absolute_parsed_expr: Option<ParsedExpression>,
    relative_parsed_expr: Option<ParsedExpression>,
}

impl AbsoluteAndRelativeParsedExpression {
    fn new(expression: Expression, root: &str, ignore_case: Option<bool>) -> Self {
        // Split the IExpression into its absolute and relative components, and glob.parse them separately.
        let mut absolute_glob_expr: Option<Expression> = None;
        let mut relative_glob_expr: Option<Expression> = None;
        for (key, value) in expression.entries() {
            if matches!(value, ExprValue::Bool(false)) {
                continue;
            }
            let target = if with_native_path(|path| path.is_absolute(key)) { &mut absolute_glob_expr } else { &mut relative_glob_expr };
            target.get_or_insert_with(Expression::new).insert(key.clone(), value.clone());
        }

        let glob_options = GlobOptions { trim_for_exclusions: true, ignore_case, ..GlobOptions::default() };
        let absolute_parsed_expr = absolute_glob_expr.map(|expr| glob::parse_expression(&expr, &glob_options));
        let relative_parsed_expr = relative_glob_expr.map(|expr| glob::parse_expression(&expr, &glob_options));
        Self { expression, root: root.to_owned(), absolute_parsed_expr, relative_parsed_expr }
    }

    /// `test`: o primeiro resultado verdadeiro (o `||` do original), ou o do padrão absoluto.
    fn test(&self, test_path: &str, basename: Option<&str>, has_sibling: Option<&dyn Fn(&str) -> bool>) -> ExprMatch {
        let adapted = has_sibling.map(|has_sibling| move |name: &str| Sibling::Ready(has_sibling(name)));
        let adapted: Option<HasSibling<'_>> = adapted.as_ref().map(|has_sibling| has_sibling as HasSibling<'_>);
        if let Some(relative) = &self.relative_parsed_expr {
            let result = relative.matches(test_path, basename, adapted);
            if expr_match_is_truthy(&result) {
                return result;
            }
        }
        match &self.absolute_parsed_expr {
            Some(absolute) => absolute.matches(&with_native_path(|path| path.join(&[self.root.as_str(), test_path])), basename, adapted),
            None => ExprMatch::None,
        }
    }

    fn get_basename_terms(&self) -> Vec<String> {
        let mut basename_terms = Vec::new();
        if let Some(absolute) = &self.absolute_parsed_expr {
            basename_terms.extend(glob::get_basename_terms(absolute));
        }

        if let Some(relative) = &self.relative_parsed_expr {
            basename_terms.extend(glob::get_basename_terms(relative));
        }

        basename_terms
    }

    fn get_path_terms(&self) -> Vec<String> {
        let mut path_terms = Vec::new();
        if let Some(absolute) = &self.absolute_parsed_expr {
            path_terms.extend(glob::get_path_terms(absolute));
        }

        if let Some(relative) = &self.relative_parsed_expr {
            path_terms.extend(glob::get_path_terms(relative));
        }

        path_terms
    }
}

/// O `{ ...; cb }` do `parallel`: o estado compartilhado entre os retornos de cada item.
struct ParallelState {
    errors: RefCell<Vec<Option<Error>>>,
    did_error_occur: Cell<bool>,
    done_count: Cell<usize>,
    callback: RefCell<Option<Box<dyn FnOnce(Option<Vec<Option<Error>>>)>>>,
}

/// `parallel`: roda `run` em todos os itens e chama `callback` quando todos terminaram, com os erros (se algum falhou).
fn parallel<T>(list: Vec<T>, run: impl Fn(T, Box<dyn FnOnce(Option<Error>)>), callback: impl FnOnce(Option<Vec<Option<Error>>>) + 'static) {
    let len = list.len();
    if len == 0 {
        return callback(None);
    }

    let state = Rc::new(ParallelState {
        errors: RefCell::new((0..len).map(|_| None).collect()),
        did_error_occur: Cell::new(false),
        done_count: Cell::new(0),
        callback: RefCell::new(Some(Box::new(callback))),
    });
    for (index, item) in list.into_iter().enumerate() {
        let state = state.clone();
        run(
            item,
            Box::new(move |error: Option<Error>| {
                if let Some(error) = error {
                    state.did_error_occur.set(true);
                    state.errors.borrow_mut()[index] = Some(error);
                }

                state.done_count.set(state.done_count.get() + 1);
                if state.done_count.get() == len {
                    let callback = state.callback.borrow_mut().take();
                    if let Some(callback) = callback {
                        let errors = state.did_error_occur.get().then(|| std::mem::take(&mut *state.errors.borrow_mut()));
                        callback(errors);
                    }
                }
            }),
        );
    }
}

/// `add` de `addDirectoryEntries`: registra o arquivo e, antes dele, as pastas que o contêm.
fn add_directory_entry(path_to_entries: &mut DirectoryTree, base: &str, relative_path: &str, search_path_of: &dyn Fn(&str) -> String) {
    let (basename, dirname) = with_native_path(|path| (path.basename(relative_path, None), path.dirname(relative_path)));
    if !path_to_entries.contains_key(&dirname) {
        path_to_entries.insert(dirname.clone(), Vec::new());
        add_directory_entry(path_to_entries, base, &dirname, search_path_of);
    }
    if let Some(entries) = path_to_entries.get_mut(&dirname) {
        entries.push(DirectoryEntry { base: base.to_owned(), relative_path: relative_path.to_owned(), basename, search_path: search_path_of(relative_path) });
    }
}

// ---------------------------------------------------------------------------------------------------------
// FileWalker
// ---------------------------------------------------------------------------------------------------------

pub struct FileWalker {
    config: IFileQuery,
    file_pattern: String,
    normalized_file_pattern_lowercase: Option<String>,
    include_pattern: Option<ParsedExpression>,
    max_results: Option<i64>,
    exists: bool,
    is_limit_hit: Cell<bool>,
    result_count: Cell<i64>,
    is_canceled: Cell<bool>,
    file_walk_sw: RefCell<Option<StopWatch>>,
    directories_walked: Cell<usize>,
    files_walked: Cell<usize>,
    errors: RefCell<Vec<String>>,
    cmd_sw: RefCell<Option<StopWatch>>,
    cmd_result_count: Cell<usize>,
    kill_cmds: RefCell<BTreeMap<u64, Rc<dyn Fn()>>>,
    folder_exclude_patterns: HashMap<String, AbsoluteAndRelativeParsedExpression>,
    global_exclude_pattern: Option<ParsedExpression>,
    spawn_ripgrep: SpawnRipgrepCmd,
}

impl FileWalker {
    pub fn new(config: &IFileQuery, spawn_ripgrep: SpawnRipgrepCmd) -> Rc<Self> {
        let config = config.clone();
        let file_pattern = config.file_pattern.clone().unwrap_or_default();
        let ignore_glob_case = config.common.ignore_glob_case.unwrap_or(false);
        let glob_options = if ignore_glob_case { GlobOptions { ignore_case: Some(true), ..GlobOptions::default() } } else { GlobOptions::default() };
        let include_pattern = config.common.include_pattern.as_ref().map(|pattern| glob::parse_expression(pattern, &glob_options));
        let max_results = config.common.max_results.filter(|max_results| *max_results != 0);
        let exists = config.exists.unwrap_or(false);

        let normalized_file_pattern_lowercase = if file_pattern.is_empty() || config.should_glob_match_file_pattern.unwrap_or(false) {
            None
        } else {
            Some(prepare_query(&utf16(&file_pattern), &Platform::detect()).normalized_lowercase.to_string_lossy()).filter(|normalized| !normalized.is_empty())
        };

        let global_exclude_pattern = config.common.exclude_pattern.as_ref().map(|pattern| glob::parse_expression(pattern, &glob_options));
        let mut folder_exclude_patterns = HashMap::new();

        let root_folders: Vec<String> = config.common.folder_queries.iter().map(|root_folder_query| fs_path(&root_folder_query.folder)).collect();
        for folder_query in &config.common.folder_queries {
            let mut folder_exclude_expression = Expression::new(); // todo: consider exclude baseURI

            match folder_query.exclude_pattern.as_deref() {
                Some(exclude_patterns) if !exclude_patterns.is_empty() => {
                    for exclude_pattern in exclude_patterns {
                        assign(&mut folder_exclude_expression, Some(&exclude_pattern.pattern));
                        assign(&mut folder_exclude_expression, config.common.exclude_pattern.as_ref());
                    }
                }
                _ => assign(&mut folder_exclude_expression, config.common.exclude_pattern.as_ref()),
            }

            // Add excludes for other root folders
            let fq_path = fs_path(&folder_query.folder);
            for other_root_folder in root_folders.iter().filter(|root_folder| **root_folder != fq_path) {
                // Exclude nested root folders
                let env = host_environment();
                if extpath::is_equal_or_parent(other_root_folder, &fq_path, ignore_glob_case, false, env.as_ref()) {
                    folder_exclude_expression.insert(with_native_path(|path| path.relative(&fq_path, other_root_folder)), true);
                }
            }

            let parsed = AbsoluteAndRelativeParsedExpression::new(folder_exclude_expression, &fq_path, config.common.ignore_glob_case);
            folder_exclude_patterns.insert(fq_path, parsed);
        }

        Rc::new(Self {
            config,
            file_pattern,
            normalized_file_pattern_lowercase,
            include_pattern,
            max_results,
            exists,
            is_limit_hit: Cell::new(false),
            result_count: Cell::new(0),
            is_canceled: Cell::new(false),
            file_walk_sw: RefCell::new(None),
            directories_walked: Cell::new(0),
            files_walked: Cell::new(0),
            errors: RefCell::new(Vec::new()),
            cmd_sw: RefCell::new(None),
            cmd_result_count: Cell::new(0),
            kill_cmds: RefCell::new(BTreeMap::new()),
            folder_exclude_patterns,
            global_exclude_pattern,
            spawn_ripgrep,
        })
    }

    /// As mensagens de erro que `walk` acumulou (o `errors` do original, que só ele escreve).
    pub fn errors(&self) -> Vec<String> {
        self.errors.borrow().clone()
    }

    pub fn cancel(&self) {
        self.is_canceled.set(true);
        let cmds: Vec<Rc<dyn Fn()>> = self.kill_cmds.borrow().values().cloned().collect();
        for kill in cmds {
            kill();
        }
    }

    pub fn walk(
        self: &Rc<Self>,
        folder_queries: &[IFolderQuery],
        extra_files: &[URI],
        num_threads: Option<i64>,
        on_result: OnResult,
        on_message: OnMessage,
        done: impl FnOnce(Option<Error>, bool) + 'static,
    ) {
        *self.file_walk_sw.borrow_mut() = Some(StopWatch::create(Some(false)));

        // Support that the file pattern is a full path to a file that exists
        if self.is_canceled.get() {
            return done(None, self.is_limit_hit.get());
        }

        // For each extra file
        for extra_file_path in extra_files {
            let extra_fs_path = fs_path(extra_file_path);
            let basename = with_native_path(|path| path.basename(&extra_fs_path, None));
            if self.global_exclude_pattern.as_ref().is_some_and(|exclude| expr_match_is_truthy(&exclude.matches(&extra_fs_path, Some(&basename), None))) {
                continue; // excluded
            }

            // File: Check for match on file pattern and include pattern
            self.match_file(&*on_result, &IRawFileMatch { base: None, relative_path: extra_fs_path, search_path: None /* no workspace relative path */ });
        }

        *self.cmd_sw.borrow_mut() = Some(StopWatch::create(Some(false)));

        // For each root folder
        let this = self.clone();
        parallel(
            folder_queries.to_vec(),
            |folder_query: IFolderQuery, root_folder_done: Box<dyn FnOnce(Option<Error>)>| {
                let this = self.clone();
                let root_folder_done = RefCell::new(Some(root_folder_done));
                self.cmd_traversal(TraversalArgs {
                    folder_query,
                    num_threads,
                    on_result: on_result.clone(),
                    on_message: on_message.clone(),
                    cb: Rc::new(move |err: Option<Error>| {
                        let Some(root_folder_done) = root_folder_done.borrow_mut().take() else {
                            return;
                        };
                        if let Some(err) = &err {
                            let error_message = to_error_message_of(err, false);
                            eprintln!("{error_message}");
                            this.errors.borrow_mut().push(error_message);
                        }
                        root_folder_done(err);
                    }),
                    paths_to_skip: None,
                });
            },
            move |errors| {
                if let Some(sw) = this.file_walk_sw.borrow_mut().as_mut() {
                    sw.stop();
                }
                let err = errors.and_then(|errors| errors.into_iter().flatten().next());
                done(err, this.is_limit_hit.get());
            },
        );
    }

    fn register_kill_cmd(&self, kill_cmd: &Rc<dyn Fn()>) -> u64 {
        let id = NEXT_KILL_ID.with(|next| next.replace(next.get() + 1));
        self.kill_cmds.borrow_mut().insert(id, kill_cmd.clone());
        KILL_CMDS.with(|cmds| cmds.borrow_mut().insert(id, kill_cmd.clone()));
        id
    }

    fn unregister_kill_cmd(&self, id: u64) {
        self.kill_cmds.borrow_mut().remove(&id);
        KILL_CMDS.with(|cmds| cmds.borrow_mut().remove(&id));
    }

    /// `cmdTraversal`: `pathsToSkip` são os caminhos relativos que uma travessia anterior da pasta já relatou.
    fn cmd_traversal(self: &Rc<Self>, args: TraversalArgs) {
        let root_folder = fs_path(&args.folder_query.folder);
        let exclude_expression = self.folder_exclude_patterns.get(&root_folder).expect("folderExcludePatterns").expression.clone();

        let spawned = (self.spawn_ripgrep)(&self.config, &args.folder_query, self.config.common.include_pattern.as_ref(), Some(&exclude_expression), args.num_threads);
        let this = self.clone();
        let _ = spawned.then_result(move |result| {
            match result {
                Ok(ripgrep) => this.run_traversal(args, root_folder, ripgrep),
                Err(err) => (args.cb)(Some(err)),
            }
            Ok(())
        });
    }

    /// O que `cmdTraversal` faz depois que o `spawnRipgrep` resolveu.
    fn run_traversal(self: &Rc<Self>, args: TraversalArgs, root_folder: String, ripgrep: RipgrepCmd) {
        let is_mac = cfg!(target_os = "macos");
        let leftover = Rc::new(RefCell::new(String::new()));
        let tree = Rc::new(RefCell::new(initial_directory_tree()));

        let cmd = ripgrep.cmd.clone();
        let kill_cmd: Rc<dyn Fn()> = {
            let cmd = cmd.clone();
            Rc::new(move || {
                if cmd.pid().is_some() {
                    cmd.kill();
                }
            })
        };
        let kill_id = self.register_kill_cmd(&kill_cmd);

        let done: Rc<dyn Fn(Option<Error>)> = {
            let this = self.clone();
            let cb = args.cb.clone();
            let fired = Cell::new(false);
            Rc::new(move |err: Option<Error>| {
                if fired.replace(true) {
                    return;
                }
                this.unregister_kill_cmd(kill_id);
                cb(err);
            })
        };

        if self.is_canceled.get() {
            let done = done.clone();
            cmd.listen(move |event| {
                if matches!(event, ChildEvent::Close(_)) {
                    done(None);
                }
            });
            kill_cmd();
            return;
        }
        let no_siblings_clauses = ripgrep.sibling_clauses.is_empty();

        let escaped_args = ripgrep.rg_args.args.iter().map(|arg| if arg.starts_with('-') { arg.clone() } else { format!("'{arg}'") }).collect::<Vec<_>>().join(" ");

        // `if (ripgrep.rgArgs.siblingClauses)` é sempre verdadeiro: o objeto (mesmo vazio) é truthy.
        let rg_cmd = format!(
            "{} {}\n - cwd: {}\n - Sibling clauses: {}",
            ripgrep.rg_disk_path,
            escaped_args,
            ripgrep.cwd,
            sibling_clauses_json(&ripgrep.rg_args.sibling_clauses)
        );
        (args.on_message)(&IProgressMessage { message: rg_cmd });

        // When ripgrep reports too many symbolic link loops, search the folder again without following
        // symbolic links and skip the paths that were already reported
        let symlink_loop_detector = (!args.folder_query.ignore_symlinks.unwrap_or(false)).then(|| Rc::new(RefCell::new(RipgrepSymlinkLoopDetector::new())));
        let reported_paths: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let report_result: OnResult = if symlink_loop_detector.is_some() {
            let reported_paths = reported_paths.clone();
            let on_result = args.on_result.clone();
            Rc::new(move |result: &IRawFileMatch| {
                reported_paths.borrow_mut().push(result.relative_path.clone());
                on_result(result);
            })
        } else {
            args.on_result.clone()
        };
        let is_retrying = Rc::new(Cell::new(false));
        let retry_without_following_symlinks: Rc<dyn Fn()> = {
            let this = self.clone();
            let is_retrying = is_retrying.clone();
            let kill_cmd = kill_cmd.clone();
            let reported_paths = reported_paths.clone();
            let args = args.clone();
            let root_folder = root_folder.clone();
            Rc::new(move || {
                is_retrying.set(true);
                this.unregister_kill_cmd(kill_id);
                kill_cmd();
                (args.on_message)(&IProgressMessage {
                    message: format!("ripgrep reported more than {MAX_RIPGREP_SYMLINK_LOOPS} symbolic link loops, searching {root_folder} again without following symbolic links"),
                });
                this.cmd_traversal(TraversalArgs {
                    folder_query: IFolderQuery { ignore_symlinks: Some(true), ..args.folder_query.clone() },
                    paths_to_skip: Some(Rc::new(reported_paths.borrow().iter().cloned().collect())),
                    ..args.clone()
                });
            })
        };

        self.cmd_result_count.set(0);
        let on_collected: CollectCallback = {
            let this = self.clone();
            let done = done.clone();
            let is_retrying = is_retrying.clone();
            let leftover = leftover.clone();
            let tree = tree.clone();
            let report_result = report_result.clone();
            let on_result = args.on_result.clone();
            let folder_query = args.folder_query.clone();
            let root_folder = root_folder.clone();
            let paths_to_skip = args.paths_to_skip.clone();
            let kill_cmd = kill_cmd.clone();
            Rc::new(move |err: Option<Error>, stdout: Option<&str>, last: bool| {
                if is_retrying.get() {
                    return;
                }
                if err.is_some() {
                    done(err);
                    return;
                }
                if this.is_limit_hit.get() {
                    done(None);
                    return;
                }

                // Mac: uses NFD unicode form on disk, but we want NFC
                let stdout = stdout.unwrap_or_default();
                let chunk = if is_mac { normalize_nfc(&utf16(stdout)).to_string_lossy() } else { stdout.to_owned() };
                let normalized = format!("{}{}", leftover.borrow(), chunk);
                let mut relative_files: Vec<String> = normalized.split('\n').map(str::to_owned).collect();

                if last {
                    let n = relative_files.len();
                    relative_files[n - 1] = js_trim(&relative_files[n - 1]).to_owned();
                    if relative_files[n - 1].is_empty() {
                        relative_files.pop();
                    }
                } else {
                    *leftover.borrow_mut() = relative_files.pop().unwrap_or_default();
                }

                if relative_files.first().is_some_and(|first| first.contains('\n')) {
                    done(Some(Error::new("Splitting up files failed")));
                    return;
                }

                if let Some(paths_to_skip) = &paths_to_skip {
                    relative_files.retain(|relative_path| !paths_to_skip.contains(relative_path));
                }

                this.cmd_result_count.set(this.cmd_result_count.get() + relative_files.len());

                if no_siblings_clauses {
                    for relative_path in relative_files {
                        let search_path = this.get_search_path(&folder_query, &relative_path);
                        this.match_file(&*report_result, &IRawFileMatch { base: Some(root_folder.clone()), relative_path, search_path: Some(search_path) });
                        if this.is_limit_hit.get() {
                            kill_cmd();
                            break;
                        }
                    }
                    if last || this.is_limit_hit.get() {
                        done(None);
                    }

                    return;
                }

                // Observação do original: otimizar as cláusulas de irmãos com o ripgrep aqui.
                this.add_directory_entries(&folder_query, &mut tree.borrow_mut(), &root_folder, &relative_files, &*report_result);

                if last {
                    this.match_directory_tree(&tree.borrow(), &root_folder, &*on_result);
                    done(None);
                }
            })
        };

        let on_stderr: Option<Box<dyn Fn(&str)>> = symlink_loop_detector.map(|detector| {
            let this = self.clone();
            let is_retrying = is_retrying.clone();
            let retry = retry_without_following_symlinks.clone();
            Box::new(move |stderr: &str| {
                if !is_retrying.get() && !this.is_canceled.get() && !this.is_limit_hit.get() && detector.borrow_mut().handle_stderr(stderr) {
                    retry();
                }
            }) as Box<dyn Fn(&str)>
        });
        self.collect_stdout(&cmd, &args.on_message, on_collected, on_stderr);
    }

    /// Public for testing.
    pub fn spawn_find_cmd(&self, folder_query: &IFolderQuery) -> Rc<ChildProcess> {
        let cwd = fs_path(&folder_query.folder);
        let exclude_pattern = self.folder_exclude_patterns.get(&cwd).expect("folderExcludePatterns");
        let basenames = exclude_pattern.get_basename_terms();
        let path_terms = exclude_pattern.get_path_terms();
        let mut args: Vec<String> = vec!["-L".to_owned(), ".".to_owned()];
        if !basenames.is_empty() || !path_terms.is_empty() {
            args.extend(["-not", "(", "("].map(str::to_owned));
            for basename in basenames {
                args.push("-name".to_owned());
                args.push(basename);
                args.push("-o".to_owned());
            }
            for path in path_terms {
                args.push("-path".to_owned());
                args.push(path);
                args.push("-o".to_owned());
            }
            args.pop();
            args.extend([")", "-prune", ")"].map(str::to_owned));
        }
        args.extend(["-type", "f"].map(str::to_owned));
        ChildProcess::spawn(Command::new("find").args(&args).current_dir(&cwd))
    }

    /// Public for testing.
    pub fn read_stdout(self: &Rc<Self>, cmd: &Rc<ChildProcess>, cb: impl FnOnce(Result<String, Error>) + 'static) {
        let all = Rc::new(RefCell::new(String::new()));
        let cb = Rc::new(RefCell::new(Some(cb)));
        let no_message: OnMessage = Rc::new(|_: &IProgressMessage| {});
        self.collect_stdout(
            cmd,
            &no_message,
            Rc::new(move |err: Option<Error>, stdout: Option<&str>, last: bool| {
                if let Some(err) = err {
                    if let Some(cb) = cb.borrow_mut().take() {
                        cb(Err(err));
                    }
                    return;
                }

                all.borrow_mut().push_str(stdout.unwrap_or("undefined"));
                if last && let Some(cb) = cb.borrow_mut().take() {
                    cb(Ok(all.borrow().clone()));
                }
            }),
            None,
        );
    }

    fn stop_cmd_sw(&self) {
        if let Some(sw) = self.cmd_sw.borrow_mut().as_mut() {
            sw.stop();
        }
    }

    fn collect_stdout(self: &Rc<Self>, cmd: &Rc<ChildProcess>, on_message: &OnMessage, cb: CollectCallback, on_stderr: Option<Box<dyn Fn(&str)>>) {
        let finished = Rc::new(Cell::new(false));
        let on_data: CollectCallback = {
            let this = self.clone();
            let finished = finished.clone();
            Rc::new(move |err: Option<Error>, stdout: Option<&str>, last: bool| {
                if finished.get() {
                    return;
                }
                if err.is_some() || last {
                    finished.set(true);

                    this.stop_cmd_sw();
                }
                cb(err, stdout, last);
            })
        };

        let got_data = Rc::new(Cell::new(false));
        if !cmd.has_stdout() {
            // Should be non-null, but #38195
            on_message(&IProgressMessage { message: "stdout is null".to_owned() });
        }

        let stderr_buffers: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        if !cmd.has_stderr() {
            // Should be non-null, but #38195
            on_message(&IProgressMessage { message: "stderr is null".to_owned() });
        }

        let mut stdout_decoder = Utf8Decoder::default();
        let mut stderr_decoder = Utf8Decoder::default();
        let this = self.clone();
        cmd.listen(move |event| match event {
            ChildEvent::Stdout(data) => {
                let text = stdout_decoder.write(data);
                on_data(None, Some(text.as_str()), false);
                got_data.set(true);
            }
            ChildEvent::Stderr(data) => {
                stderr_buffers.borrow_mut().push(data.clone());
                if let Some(on_stderr) = &on_stderr {
                    on_stderr(&stderr_decoder.write(data));
                }
            }
            ChildEvent::Error(err) => on_data(Some(err.clone()), None, false),
            ChildEvent::Close(code) => {
                // ripgrep returns code=1 when no results are found
                let stderr_text = if got_data.get() { String::new() } else { decode_data(&stderr_buffers.borrow()) };
                if !got_data.get() && !stderr_text.is_empty() && rg_error_msg_for_display(&stderr_text).is_some_and(|message| !message.is_empty()) {
                    let code = code.map_or_else(|| "null".to_owned(), |code| code.to_string());
                    on_data(Some(Error::new(format!("command failed with error code {code}: {}", decode_data(&stderr_buffers.borrow())))), None, false);
                } else {
                    if this.exists && *code == Some(0) {
                        this.is_limit_hit.set(true);
                    }
                    on_data(None, Some(""), true);
                }
            }
        });
    }

    fn add_directory_entries(
        &self,
        folder_query: &IFolderQuery,
        path_to_entries: &mut DirectoryTree,
        base: &str,
        relative_files: &[String],
        on_result: &dyn Fn(&IRawFileMatch),
    ) {
        // Support relative paths to files from a root resource (ignores excludes)
        let file_pattern_match = if self.file_pattern.is_empty() {
            None
        } else {
            relative_files.iter().find(|file| strings_equals(file, &self.file_pattern, self.config.common.ignore_glob_case))
        };
        if let Some(file_pattern_match) = file_pattern_match {
            self.match_file(
                on_result,
                &IRawFileMatch {
                    base: Some(base.to_owned()),
                    relative_path: file_pattern_match.clone(),
                    search_path: Some(self.get_search_path(folder_query, file_pattern_match)),
                },
            );
        }

        let search_path_of = |relative_path: &str| self.get_search_path(folder_query, relative_path);
        for relative_path in relative_files {
            add_directory_entry(path_to_entries, base, relative_path, &search_path_of);
        }
    }

    fn match_directory_tree(&self, path_to_entries: &DirectoryTree, root_folder: &str, on_result: &dyn Fn(&IRawFileMatch)) {
        let exclude_pattern = self.folder_exclude_patterns.get(root_folder).expect("folderExcludePatterns");
        if let Some(root_entries) = path_to_entries.get(".") {
            self.match_directory(root_entries, path_to_entries, exclude_pattern, on_result);
        }
    }

    /// `matchDirectory`.
    fn match_directory(&self, entries: &[DirectoryEntry], path_to_entries: &DirectoryTree, exclude_pattern: &AbsoluteAndRelativeParsedExpression, on_result: &dyn Fn(&IRawFileMatch)) {
        let ignore_glob_case = self.config.common.ignore_glob_case;
        self.directories_walked.set(self.directories_walked.get() + 1);
        let basenames: Vec<String> = entries.iter().map(|entry| entry.basename.clone()).collect();
        let has_sibling = has_sibling_fn(Some(Rc::new(move || basenames.clone())));
        for entry in entries {
            let DirectoryEntry { relative_path, basename, .. } = entry;

            // Check exclude pattern
            // If the user searches for the exact file name, we adjust the glob matching
            // to ignore filtering by siblings because the user seems to know what they
            // are searching for and we want to include the result in that case anyway
            let sibling_filter = if strings_equals(&self.file_pattern, basename, ignore_glob_case) { None } else { has_sibling.as_deref() };
            if expr_match_is_truthy(&exclude_pattern.test(relative_path, Some(basename), sibling_filter)) {
                continue;
            }

            if let Some(sub) = path_to_entries.get(relative_path) {
                self.match_directory(sub, path_to_entries, exclude_pattern, on_result);
            } else {
                self.files_walked.set(self.files_walked.get() + 1);
                if strings_equals(relative_path, &self.file_pattern, ignore_glob_case) {
                    continue; // ignore file if its path matches with the file pattern because that is already matched above
                }

                self.match_file(on_result, &entry.to_raw());
            }

            if self.is_limit_hit.get() {
                break;
            }
        }
    }

    pub fn get_stats(&self) -> ISearchEngineStats {
        let elapsed = |sw: &RefCell<Option<StopWatch>>, name: &str| sw.borrow().as_ref().unwrap_or_else(|| panic!("{name}")).elapsed();
        ISearchEngineStats {
            cmd_time: elapsed(&self.cmd_sw, "cmdSW"),
            file_walk_time: elapsed(&self.file_walk_sw, "fileWalkSW"),
            directories_walked: self.directories_walked.get() as f64,
            files_walked: self.files_walked.get() as f64,
            cmd_result_count: Some(self.cmd_result_count.get() as f64),
        }
    }

    fn match_file(&self, on_result: &dyn Fn(&IRawFileMatch), candidate: &IRawFileMatch) {
        let included = self.include_pattern.as_ref().is_none_or(|include| {
            let basename = with_native_path(|path| path.basename(&candidate.relative_path, None));
            expr_match_is_truthy(&include.matches(&candidate.relative_path, Some(&basename), None))
        });
        if self.is_file_match(candidate) && included {
            self.result_count.set(self.result_count.get() + 1);

            if self.exists || self.max_results.is_some_and(|max_results| self.result_count.get() > max_results) {
                self.is_limit_hit.set(true);
            }

            if !self.is_limit_hit.get() {
                on_result(candidate);
            }
        }
    }

    fn is_file_match(&self, candidate: &IRawFileMatch) -> bool {
        // Check for search pattern
        if !self.file_pattern.is_empty() {
            if self.file_pattern == "*" {
                return true; // support the all-matching wildcard
            }

            return match &self.normalized_file_pattern_lowercase {
                Some(normalized) => is_file_pattern_match(candidate, normalized, true, false),
                None => is_file_pattern_match(candidate, &self.file_pattern, false, self.config.common.ignore_glob_case.unwrap_or(false)),
            };
        }

        // No patterns means we match all
        true
    }

    /// If we're searching for files in multiple workspace folders, then better prepend the
    /// name of the workspace folder to the path of the file. This way we'll be able to
    /// better filter files that are all on the top of a workspace folder and have all the
    /// same name. A typical example are `package.json` or `README.md` files.
    fn get_search_path(&self, folder_query: &IFolderQuery, relative_path: &str) -> String {
        match folder_query.folder_name.as_deref() {
            Some(folder_name) if !folder_name.is_empty() => with_native_path(|path| path.join(&[folder_name, relative_path])),
            _ => relative_path.to_owned(),
        }
    }
}

/// `decodeData`: um `StringDecoder` só para todos os blocos.
fn decode_data(buffers: &[Vec<u8>]) -> String {
    let mut decoder = Utf8Decoder::default();
    buffers.iter().map(|buffer| decoder.write(buffer)).collect::<Vec<_>>().join("")
}

// ---------------------------------------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------------------------------------

/// `ISearchEngineSuccess`.
pub struct ISearchEngineSuccess {
    pub limit_hit: bool,
    pub messages: Vec<ITextSearchCompleteMessage>,
    pub stats: ISearchEngineStats,
}

/// `ISearchEngine<T>`.
pub trait ISearchEngine<T> {
    fn search(&self, on_result: Rc<dyn Fn(&T)>, on_progress: OnMessage, done: Box<dyn FnOnce(Option<Error>, ISearchEngineSuccess)>);
    fn cancel(&self);
}

pub struct Engine {
    folder_queries: Vec<IFolderQuery>,
    extra_files: Vec<URI>,
    walker: Rc<FileWalker>,
    num_threads: Option<i64>,
}

impl Engine {
    pub fn new(config: &IFileQuery, num_threads: Option<i64>, spawn_ripgrep: SpawnRipgrepCmd) -> Self {
        Self {
            folder_queries: config.common.folder_queries.clone(),
            extra_files: config.common.extra_file_resources.clone().unwrap_or_default(),
            walker: FileWalker::new(config, spawn_ripgrep),
            num_threads,
        }
    }
}

impl ISearchEngine<IRawFileMatch> for Engine {
    fn search(&self, on_result: OnResult, on_progress: OnMessage, done: Box<dyn FnOnce(Option<Error>, ISearchEngineSuccess)>) {
        let walker = self.walker.clone();
        self.walker.walk(&self.folder_queries, &self.extra_files, self.num_threads, on_result, on_progress, move |err, is_limit_hit| {
            done(err, ISearchEngineSuccess { limit_hit: is_limit_hit, stats: walker.get_stats(), messages: Vec::new() });
        });
    }

    fn cancel(&self) {
        self.walker.cancel();
    }
}
