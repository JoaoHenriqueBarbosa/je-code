//! Teste de ponta a ponta de `vscode.window.createOutputChannel` (não é porte do oráculo): uma extensão de exemplo em JavaScript,
//! ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), cria canais de saída e escreve neles, e o
//! efeito aparece nos serviços do workbench.
//!
//! O lado do workbench é o `MainThreadOutputService` REAL, ligado pelo binder do teste sobre um `IOutputService` e um `IOutputViews`
//! que gravam o que recebem; o `ExtHostOutputService` do host usa um `ILoggerService` real (`AbstractLoggerService`) cujos loggers
//! gravam o que a extensão escreveu, e um criador de diretórios que lembra dos pedidos (o `OutputEnvironment` do binder).
//!
//! - **Canal comum**: o registro no workbench (`$register`), o texto que chega ao logger, `show`, `append` com o canal visível (que
//!   atualiza o workbench), `clear`, `replace`, `hide` (que fecha a vista do canal ativo) e `dispose` (e o erro `Channel has been closed`
//!   de quem usa o canal depois).
//! - **Canal de log (`{ log: true }`)**: a pasta e o arquivo por extensão, os níveis de `trace` a `error` (o filtro pelo nível), o
//!   `logLevel`, o evento `onDidChangeLogLevel` e a reutilização do canal pedido duas vezes.
//! - **Validações**: o nome vazio e o `languageId` vazio.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use harness::{run_extension, Workbench};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_exthost::in_process::{IMainThreadBinder, OutputEnvironment};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::{AbstractLoggerService, CreateLoggerFn, LogArg, LogLevel, LogMessage, Logger, LoggerOptions, LoggerRef, LoggerService};
use je_workbench::api::browser::main_thread_output_service::{IOutputStatusEntryAccessor, IOutputStatusbar, IOutputViews, MainThreadOutputService, OutputStatusEntry};
use je_workbench::api::common::ext_host_output::IDirectoryCreator;
use je_workbench::api::common::ext_host_protocol::{IMainContext, MainContext, MainThreadOutputServiceShape};
use je_workbench::services::output::output::{output_channel_registry, IOutputChannel, IOutputChannelDescriptor, IOutputService, OutputChannelUpdateMode, OUTPUT_VIEW_ID};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));

  command('t.plain', () => {
    const channel = vscode.window.createOutputChannel('Meu Canal');
    globalThis.__channel = channel;
    channel.append('a');
    channel.appendLine('b');
    return channel.name;
  });
  command('t.language', () => {
    vscode.window.createOutputChannel('Linguagem', 'json');
    return 'ok';
  });
  command('t.trimmedName', () => vscode.window.createOutputChannel('  Aparado  ').name);
  command('t.badName', () => {
    try { vscode.window.createOutputChannel('   '); return 'sem erro'; } catch (error) { return error.message; }
  });
  command('t.badLanguage', () => {
    try { vscode.window.createOutputChannel('X', '  '); return 'sem erro'; } catch (error) { return error.message; }
  });
  command('t.show', () => { globalThis.__channel.show(true); return 'ok'; });
  command('t.showColumn', () => { globalThis.__channel.show(2); return 'ok'; });
  command('t.append', () => { globalThis.__channel.append('c'); return 'ok'; });
  command('t.clear', () => { globalThis.__channel.clear(); return 'ok'; });
  command('t.replace', () => { globalThis.__channel.replace('novo'); return 'ok'; });
  command('t.hide', () => { globalThis.__channel.hide(); return 'ok'; });
  command('t.dispose', () => { globalThis.__channel.dispose(); return 'ok'; });
  command('t.afterDispose', () => {
    try { globalThis.__channel.append('x'); return 'sem erro'; } catch (error) { return error.message; }
  });

  command('t.log', () => {
    const log = vscode.window.createOutputChannel('Log X', { log: true });
    globalThis.__log = log;
    log.trace('t0');
    log.debug('d0');
    log.info('i0');
    log.warn('w0');
    log.error(new Error('boom'));
    return log.name;
  });
  command('t.logAgain', () => vscode.window.createOutputChannel('Log X', { log: true }).name);
  command('t.logSubscribe', () => {
    globalThis.__levels = [];
    globalThis.__subscription = globalThis.__log.onDidChangeLogLevel((level) => globalThis.__levels.push(level));
    return 'ok';
  });
  command('t.logLevel', () => globalThis.__log.logLevel);
  command('t.logLevels', () => globalThis.__levels.join(','));
  command('t.logAfterLevelChange', () => {
    globalThis.__log.debug('d1');
    globalThis.__log.trace('t1');
    return 'ok';
  });
  command('t.logIsNotPlain', () => typeof globalThis.__log.appendLine + ':' + typeof globalThis.__channel.info);
  command('t.logLevelEnum', () => JSON.stringify([vscode.LogLevel.Off, vscode.LogLevel.Trace, vscode.LogLevel.Info, vscode.LogLevel[5]]));
};
"#;

/// Um texto que chegou a um logger: o arquivo (caminho), o nível e a mensagem.
#[derive(Clone, Debug, PartialEq)]
struct Written {
    file: String,
    level: LogLevel,
    text: String,
}

type Writes = Rc<RefCell<Vec<Written>>>;

fn message_text(message: &LogMessage) -> String {
    match message {
        LogMessage::Text(text) => text.clone(),
        LogMessage::Error(error) => error.message.clone(),
    }
}

/// O `ILogger` que grava o que escrevem nele (o arquivo de log não existe: o que importa é o texto).
struct RecordingLogger {
    file: String,
    writes: Writes,
    level: Cell<LogLevel>,
    on_did_change_log_level: Rc<Emitter<LogLevel>>,
}

impl RecordingLogger {
    fn record(&self, level: LogLevel, text: &str) {
        self.writes.borrow_mut().push(Written { file: self.file.clone(), level, text: text.to_owned() });
    }
}

impl IDisposable for RecordingLogger {
    fn dispose(&self) {}
}

impl Logger for RecordingLogger {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.on_did_change_log_level.event()
    }

    fn get_level(&self) -> LogLevel {
        self.level.get()
    }

    fn set_level(&self, level: LogLevel) {
        self.level.set(level);
        self.on_did_change_log_level.fire(level);
    }

    fn trace_args(&self, message: &str, _args: &[LogArg]) {
        self.record(LogLevel::Trace, message);
    }

    fn debug_args(&self, message: &str, _args: &[LogArg]) {
        self.record(LogLevel::Debug, message);
    }

    fn info_args(&self, message: &str, _args: &[LogArg]) {
        self.record(LogLevel::Info, message);
    }

    fn warn_args(&self, message: &LogMessage, _args: &[LogArg]) {
        self.record(LogLevel::Warning, &message_text(message));
    }

    fn error_args(&self, message: &LogMessage, _args: &[LogArg]) {
        self.record(LogLevel::Error, &message_text(message));
    }

    fn flush(&self) {}
}

/// O `IDirectoryCreator` que lembra dos diretórios pedidos.
#[derive(Default)]
struct RecordingDirectories {
    created: RefCell<Vec<String>>,
}

impl IDirectoryCreator for RecordingDirectories {
    fn create_directory(&self, uri: &URI) -> Promise<()> {
        self.created.borrow_mut().push(uri.path().to_string_lossy());
        Promise::resolved(())
    }
}

/// O que o workbench fez em cada canal, em texto e na ordem (`show`, `update`, `dispose`).
type Events = Rc<RefCell<Vec<String>>>;

/// O `IOutputChannel` do workbench: grava as atualizações e o descarte.
struct RecordedChannel {
    id: String,
    label: String,
    events: Events,
}

impl IDisposable for RecordedChannel {
    fn dispose(&self) {
        self.events.borrow_mut().push(format!("dispose {}", self.id));
    }
}

impl IOutputChannel for RecordedChannel {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn uri(&self) -> URI {
        panic!("uri não é usado por este teste")
    }

    fn append(&self, _output: &str) {
        panic!("append não é usado por este teste: o texto chega pelo logger")
    }

    fn clear(&self) {
        panic!("clear não é usado por este teste: o MainThreadOutputService chama update")
    }

    fn replace(&self, _output: &str) {
        panic!("replace não é usado por este teste: o MainThreadOutputService chama update")
    }

    fn update(&self, mode: OutputChannelUpdateMode, till: Option<u64>) {
        self.events.borrow_mut().push(format!("update {} {mode:?} till={till:?}", self.id));
    }
}

/// O `IOutputService`: os canais existem enquanto o registro (`output_channel_registry`) os tem, e `show_channel` torna o canal ativo.
struct RecordingOutputService {
    channels: RefCell<Vec<Rc<RecordedChannel>>>,
    active: RefCell<Option<String>>,
    on_active_output_channel: Rc<Emitter<String>>,
    events: Events,
}

impl IOutputService for RecordingOutputService {
    fn get_channel(&self, id: &str) -> Option<Rc<dyn IOutputChannel>> {
        let known = self.channels.borrow().iter().find(|channel| channel.id == id).cloned();
        if let Some(channel) = known {
            return Some(channel);
        }
        let descriptor = output_channel_registry().get_channel(id)?;
        let channel = Rc::new(RecordedChannel { id: descriptor.id, label: descriptor.label, events: self.events.clone() });
        self.channels.borrow_mut().push(channel.clone());
        Some(channel)
    }

    fn get_active_channel(&self) -> Option<Rc<dyn IOutputChannel>> {
        let active = self.active.borrow().clone();
        active.and_then(|id| self.get_channel(&id))
    }

    fn show_channel(&self, id: &str, preserve_focus: bool) -> Promise<()> {
        self.events.borrow_mut().push(format!("show {id} preserve={preserve_focus}"));
        *self.active.borrow_mut() = Some(id.to_owned());
        self.on_active_output_channel.fire(id.to_owned());
        Promise::resolved(())
    }

    fn on_active_output_channel(&self) -> Event<String> {
        self.on_active_output_channel.event()
    }
}

/// O `IOutputViews`: o painel de saída aberto ou não, e as vistas que o workbench fechou.
struct RecordingViews {
    visible: Cell<bool>,
    closed: RefCell<Vec<String>>,
    changes: Rc<Emitter<String>>,
}

impl IOutputViews for RecordingViews {
    fn is_view_visible(&self, _id: &str) -> bool {
        self.visible.get()
    }

    fn close_view(&self, id: &str) {
        self.closed.borrow_mut().push(id.to_owned());
    }

    fn on_did_change_view_visibility(&self) -> Event<String> {
        self.changes.event()
    }
}

/// O `IOutputStatusbar` que nenhum caso usa (só o `showQuietly` o aciona, e a configuração do teste não o liga).
struct UnusedStatusbar;

impl IOutputStatusbar for UnusedStatusbar {
    fn add_quiet_output_entry(&self, _entry: OutputStatusEntry) -> Rc<dyn IOutputStatusEntryAccessor> {
        panic!("addEntry não é usado por este teste")
    }
}

/// Liga o `MainThreadOutputService` (real) ao host e entrega o ambiente do `ExtHostOutputService`.
struct Binder {
    output: Rc<RecordingOutputService>,
    views: Rc<RecordingViews>,
    directories: Rc<RecordingDirectories>,
    logger_service: Rc<AbstractLoggerService>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let service = MainThreadOutputService::new(&*context, self.output.clone(), self.views.clone(), TestConfigurationService::new(None), Rc::new(UnusedStatusbar));
        let shape: Rc<dyn MainThreadOutputServiceShape> = service.clone();
        context.set(&MainContext::MAIN_THREAD_OUTPUT_SERVICE, shape);
        let client: Rc<dyn IDisposable> = service;
        vec![client]
    }

    fn output_environment(&self) -> Option<OutputEnvironment> {
        Some(OutputEnvironment {
            logs_location: URI::file("/logs", je_platform::resources::host_platform()),
            extension_log_levels: Vec::new(),
            directory_creator: self.directories.clone(),
            logger_service: self.logger_service.clone(),
        })
    }
}

struct Observed {
    output: Rc<RecordingOutputService>,
    views: Rc<RecordingViews>,
    directories: Rc<RecordingDirectories>,
    logger_service: Rc<AbstractLoggerService>,
    writes: Writes,
    /// Os recursos dos loggers criados, na ordem.
    created_files: Rc<RefCell<Vec<URI>>>,
}

impl Observed {
    /// Os textos que chegaram aos loggers cujo arquivo termina em `suffix`.
    fn writes_of(&self, suffix: &str) -> Vec<(LogLevel, String)> {
        self.writes.borrow().iter().filter(|written| written.file.ends_with(suffix)).map(|written| (written.level, written.text.clone())).collect()
    }

    fn events(&self) -> Vec<String> {
        self.output.events.borrow().clone()
    }

    /// O logger criado para o arquivo que termina em `suffix`.
    fn created_file(&self, suffix: &str) -> URI {
        self.created_files.borrow().iter().find(|file| file.path().to_string_lossy().ends_with(suffix)).cloned().unwrap_or_else(|| panic!("nenhum logger para '{suffix}'"))
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Observed) {
    let events: Events = Rc::default();
    let output = Rc::new(RecordingOutputService { channels: RefCell::new(Vec::new()), active: RefCell::new(None), on_active_output_channel: Emitter::new(), events });
    let views = Rc::new(RecordingViews { visible: Cell::new(false), closed: RefCell::new(Vec::new()), changes: Emitter::new() });
    let directories = Rc::new(RecordingDirectories::default());
    let writes: Writes = Rc::default();
    let created_files: Rc<RefCell<Vec<URI>>> = Rc::default();
    let create_logger: CreateLoggerFn = {
        let (writes, created_files) = (writes.clone(), created_files.clone());
        Rc::new(move |resource: &URI, level: LogLevel, _options: &LoggerOptions| -> LoggerRef {
            created_files.borrow_mut().push(resource.clone());
            Rc::new(RecordingLogger { file: resource.path().to_string_lossy(), writes: writes.clone(), level: Cell::new(level), on_did_change_log_level: Emitter::new() })
        })
    };
    let logger_service = AbstractLoggerService::new(LogLevel::Info, URI::file("/logs", je_platform::resources::host_platform()), Vec::new(), create_logger);
    let binder = Binder { output: output.clone(), views: views.clone(), directories: directories.clone(), logger_service: logger_service.clone() };
    (Rc::new(binder), Observed { output, views, directories, logger_service, writes, created_files })
}

/// O descritor que o `MainThreadOutputService` registrou para o canal com este rótulo (o mais recente).
fn descriptor_of(label: &str) -> IOutputChannelDescriptor {
    output_channel_registry().get_channels().into_iter().rev().find(|descriptor| descriptor.label == label).unwrap_or_else(|| panic!("o canal '{label}' não foi registrado"))
}

#[test]
fn a_plain_channel_registers_in_the_workbench_and_writes_through_the_logger() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.plain", Vec::new()), Dynamic::string("Meu Canal"));

        let descriptor = descriptor_of("Meu Canal");
        assert!(!descriptor.log);
        assert_eq!(descriptor.language_id, None);
        assert_eq!(descriptor.extension_id.as_deref(), Some("je.exthost"));
        assert!(descriptor.id.starts_with("extension-output-je.exthost-#") && descriptor.id.ends_with("-Meu Canal"), "id inesperado: {}", descriptor.id);
        let source = descriptor.source.expect("o canal comum tem o arquivo de saída como fonte");
        assert!(source[0].resource.path().to_string_lossy().ends_with("/1-Meu Canal.log"));

        let directories = observed.directories.created.borrow();
        assert_eq!(directories.len(), 1, "a pasta de saídas é criada uma vez");
        assert!(directories[0].starts_with("/logs/output_logging_"), "pasta inesperada: {}", directories[0]);

        // O texto escrito antes de o canal existir espera a criação e chega ao logger na ordem.
        assert_eq!(observed.writes_of("/1-Meu Canal.log"), vec![(LogLevel::Info, "a".to_owned()), (LogLevel::Info, "b\n".to_owned())]);
        assert!(observed.events().is_empty(), "com o painel fechado o workbench não recebe atualizações");
    });
}

#[test]
fn a_channel_with_a_language_id_registers_it() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(workbench.execute("t.language", Vec::new()), Dynamic::string("ok"));
        assert_eq!(descriptor_of("Linguagem").language_id.as_deref(), Some("json"));
    });
}

#[test]
fn the_channel_name_is_trimmed() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(workbench.execute("t.trimmedName", Vec::new()), Dynamic::string("Aparado"));
        assert_eq!(descriptor_of("Aparado").label, "Aparado");
    });
}

#[test]
fn an_empty_name_or_language_id_throws() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(workbench.execute("t.badName", Vec::new()), Dynamic::string("illegal argument `name`. must not be falsy"));
        assert_eq!(workbench.execute("t.badLanguage", Vec::new()), Dynamic::string("illegal argument `languageId`. must not be empty"));
    });
}

#[test]
fn show_append_clear_replace_hide_and_dispose_reach_the_workbench() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.plain", Vec::new()), Dynamic::string("Meu Canal"));
        let id = descriptor_of("Meu Canal").id;

        // `show(true)` revela o canal sem roubar o foco; o painel aberto com o canal ativo o torna visível para o host.
        observed.views.visible.set(true);
        assert_eq!(workbench.execute("t.show", Vec::new()), Dynamic::string("ok"));
        // Visível, cada escrita avisa o workbench (`$update` com `Append`).
        assert_eq!(workbench.execute("t.append", Vec::new()), Dynamic::string("ok"));
        // `clear` e `replace` informam o deslocamento em bytes do que já foi escrito: "a" (1) + "b\n" (2) + "c" (1) = 4.
        assert_eq!(workbench.execute("t.clear", Vec::new()), Dynamic::string("ok"));
        assert_eq!(workbench.execute("t.replace", Vec::new()), Dynamic::string("ok"));
        assert_eq!(
            observed.events(),
            vec![
                format!("show {id} preserve=true"),
                format!("update {id} Append till=None"),
                format!("update {id} Clear till=Some(4)"),
                format!("update {id} Append till=None"),
                format!("update {id} Replace till=Some(4)"),
            ]
        );
        assert_eq!(
            observed.writes_of("/1-Meu Canal.log"),
            vec![(LogLevel::Info, "a".to_owned()), (LogLevel::Info, "b\n".to_owned()), (LogLevel::Info, "c".to_owned()), (LogLevel::Info, "novo".to_owned())]
        );

        // `hide` fecha a vista de saída quando o canal é o ativo.
        assert_eq!(workbench.execute("t.hide", Vec::new()), Dynamic::string("ok"));
        assert_eq!(*observed.views.closed.borrow(), vec![OUTPUT_VIEW_ID.to_owned()]);

        // `dispose` descarta o canal no workbench, e usá-lo depois é erro.
        assert_eq!(workbench.execute("t.dispose", Vec::new()), Dynamic::string("ok"));
        assert_eq!(observed.events().last(), Some(&format!("dispose {id}")));
        assert_eq!(workbench.execute("t.afterDispose", Vec::new()), Dynamic::string("Channel has been closed"));
    });
}

#[test]
fn show_with_a_view_column_does_not_preserve_the_focus() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.plain", Vec::new()), Dynamic::string("Meu Canal"));
        let id = descriptor_of("Meu Canal").id;
        observed.views.visible.set(true);
        assert_eq!(workbench.execute("t.showColumn", Vec::new()), Dynamic::string("ok"));
        assert_eq!(observed.events(), vec![format!("show {id} preserve=false")]);
    });
}

#[test]
fn a_log_channel_writes_to_the_extension_log_file_and_filters_by_level() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        // O nível do canal é o do logger criado (`Info`, o padrão do serviço): `trace` e `debug` não chegam.
        assert_eq!(workbench.execute("t.log", Vec::new()), Dynamic::string("Log X"));
        // O `logLevel` só vale o do logger depois de o canal real existir (antes disso é o nível do serviço de log do host).
        assert_eq!(workbench.execute("t.logLevel", Vec::new()), Dynamic::Number(3.0));

        assert!(observed.directories.created.borrow().iter().any(|directory| directory == "/logs/je.exthost"), "a pasta de logs da extensão é criada");
        let written = observed.writes_of("/je.exthost/Log X.log");
        assert_eq!(written.len(), 3, "só info, warn e error passam do filtro: {written:?}");
        assert_eq!(written[0], (LogLevel::Info, "i0".to_owned()));
        assert_eq!(written[1], (LogLevel::Warning, "w0".to_owned()));
        assert_eq!(written[2].0, LogLevel::Error);
        assert!(written[2].1.contains("boom"), "o Error vira a pilha ou nome: mensagem: {}", written[2].1);
        assert!(observed.events().is_empty(), "o canal de log não é registrado no `$register` do canal comum");
    });
}

#[test]
fn asking_for_the_same_log_channel_twice_reuses_the_logger() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.log", Vec::new()), Dynamic::string("Log X"));
        assert_eq!(workbench.execute("t.logAgain", Vec::new()), Dynamic::string("Log X"));
        let creations = observed.created_files.borrow().iter().filter(|file| file.path().to_string_lossy().ends_with("/je.exthost/Log X.log")).count();
        assert_eq!(creations, 1, "o canal de log com o mesmo nome é o mesmo");
    });
}

#[test]
fn a_log_channel_follows_the_level_of_its_logger() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.log", Vec::new()), Dynamic::string("Log X"));
        assert_eq!(workbench.execute("t.logSubscribe", Vec::new()), Dynamic::string("ok"));

        // O usuário baixa o nível do logger do canal para `Debug`: `logLevel` muda e `onDidChangeLogLevel` dispara.
        let file = observed.created_file("/je.exthost/Log X.log");
        observed.logger_service.set_log_level(&file, LogLevel::Debug);
        workbench.pump();
        assert_eq!(workbench.execute("t.logLevel", Vec::new()), Dynamic::Number(2.0));
        assert_eq!(workbench.execute("t.logLevels", Vec::new()), Dynamic::string("2"));

        // Com `Debug` o `debug` passa e o `trace` continua barrado.
        assert_eq!(workbench.execute("t.logAfterLevelChange", Vec::new()), Dynamic::string("ok"));
        let written = observed.writes_of("/je.exthost/Log X.log");
        assert!(written.contains(&(LogLevel::Debug, "d1".to_owned())));
        assert!(!written.contains(&(LogLevel::Trace, "t1".to_owned())));
    });
}

#[test]
fn log_channels_have_the_log_methods_and_plain_channels_do_not() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(workbench.execute("t.plain", Vec::new()), Dynamic::string("Meu Canal"));
        assert_eq!(workbench.execute("t.log", Vec::new()), Dynamic::string("Log X"));
        assert_eq!(workbench.execute("t.logIsNotPlain", Vec::new()), Dynamic::string("function:undefined"));
    });
}

#[test]
fn the_log_level_enumeration_is_exposed_in_both_directions() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(workbench.execute("t.logLevelEnum", Vec::new()), Dynamic::string(r#"[0,1,3,"Error"]"#));
    });
}
