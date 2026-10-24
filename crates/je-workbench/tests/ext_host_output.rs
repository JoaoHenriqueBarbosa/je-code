//! Teste de unidade do `ExtHostOutputService` (`extHostOutput.ts`; o oráculo não tem teste dele, então não é porte) contra dublês: o
//! `MainThreadOutputServiceShape` que registra as chamadas, um `ILoggerService` que devolve loggers que gravam o que recebem e um
//! criador de diretórios.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_exthost::local_context::LocalContext;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::{
    DidChangeLoggersEvent, LogArg, LogLevel, LogMessage, LogServiceRef, Logger, LoggerRef, LoggerResource, LoggerService, LoggerServiceLogLevelChange, NullLogService,
    ResourceOrId, LoggerOptions,
};
use je_workbench::api::common::ext_host_output::{ExtHostOutputService, IDirectoryCreator, OutputChannelOptions};
use je_workbench::api::common::ext_host_protocol::{ExtHostOutputServiceShape, MainContext, MainThreadOutputServiceShape};
use je_workbench::services::output::output::OutputChannelUpdateMode;

type Writes = Rc<RefCell<Vec<(LogLevel, String)>>>;

/// O `ILogger` que grava o que escrevem nele.
struct RecordingLogger {
    writes: Writes,
    level: Cell<LogLevel>,
    on_did_change_log_level: Rc<Emitter<LogLevel>>,
    flushes: Rc<Cell<u32>>,
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
        self.writes.borrow_mut().push((LogLevel::Trace, message.to_owned()));
    }

    fn debug_args(&self, message: &str, _args: &[LogArg]) {
        self.writes.borrow_mut().push((LogLevel::Debug, message.to_owned()));
    }

    fn info_args(&self, message: &str, _args: &[LogArg]) {
        self.writes.borrow_mut().push((LogLevel::Info, message.to_owned()));
    }

    fn warn_args(&self, message: &LogMessage, _args: &[LogArg]) {
        self.writes.borrow_mut().push((LogLevel::Warning, format!("{message:?}")));
    }

    fn error_args(&self, message: &LogMessage, _args: &[LogArg]) {
        self.writes.borrow_mut().push((LogLevel::Error, format!("{message:?}")));
    }

    fn flush(&self) {
        self.flushes.set(self.flushes.get() + 1);
    }
}

/// O `ILoggerService` que cria `RecordingLogger` e lembra dos recursos criados e retirados.
struct TestLoggerService {
    writes: Writes,
    flushes: Rc<Cell<u32>>,
    created: RefCell<Vec<(URI, LoggerOptions)>>,
    deregistered: RefCell<Vec<URI>>,
    levels: RefCell<Vec<(URI, LogLevel)>>,
}

impl IDisposable for TestLoggerService {
    fn dispose(&self) {}
}

impl LoggerService for TestLoggerService {
    fn create_logger(&self, id_or_resource: ResourceOrId, options: Option<LoggerOptions>) -> LoggerRef {
        let ResourceOrId::Resource(resource) = id_or_resource else { panic!("o teste só cria loggers por recurso") };
        self.created.borrow_mut().push((resource, options.unwrap_or_default()));
        Rc::new(RecordingLogger { writes: self.writes.clone(), level: Cell::new(LogLevel::Info), on_did_change_log_level: Emitter::new(), flushes: self.flushes.clone() })
    }

    fn get_logger(&self, _resource_or_id: &ResourceOrId) -> Option<LoggerRef> {
        None
    }

    fn on_did_change_log_level(&self) -> Event<LoggerServiceLogLevelChange> {
        Event::none()
    }

    fn set_default_log_level(&self, _level: LogLevel) {}

    fn set_log_level(&self, _resource: &URI, _level: LogLevel) {}

    fn get_log_level(&self, _resource: Option<&URI>) -> LogLevel {
        LogLevel::Info
    }

    fn on_did_change_visibility(&self) -> Event<(URI, bool)> {
        Event::none()
    }

    fn set_visibility(&self, _resource_or_id: &ResourceOrId, _visible: bool) {}

    fn on_did_change_loggers(&self) -> Event<DidChangeLoggersEvent> {
        Event::none()
    }

    fn register_logger(&self, _resource: LoggerResource) {}

    fn deregister_logger(&self, id_or_resource: &ResourceOrId) {
        if let ResourceOrId::Resource(resource) = id_or_resource {
            self.deregistered.borrow_mut().push(resource.clone());
        }
    }

    fn get_registered_loggers(&self) -> Vec<LoggerResource> {
        Vec::new()
    }

    fn get_registered_logger(&self, resource: &URI) -> Option<LoggerResource> {
        self.levels.borrow().iter().find(|(known, _)| known == resource).map(|(resource, level)| {
            let mut registered = LoggerResource::new(resource.clone(), "x");
            registered.log_level = Some(*level);
            registered
        })
    }
}

/// O `MainThreadOutputServiceShape` que registra as chamadas.
#[derive(Default)]
struct RecordingOutput {
    calls: RefCell<Vec<String>>,
}

impl IDisposable for RecordingOutput {
    fn dispose(&self) {}
}

impl MainThreadOutputServiceShape for RecordingOutput {
    fn rpc_register(&self, label: &str, file: URI, language_id: Option<String>, extension_id: &str) -> Promise<String> {
        let id = format!("canal-{}", self.calls.borrow().len());
        self.calls.borrow_mut().push(format!("register {label} lang={language_id:?} ext={extension_id} file={}", file.path()));
        Promise::resolved(id)
    }

    fn rpc_update(&self, channel_id: &str, mode: OutputChannelUpdateMode, till: Option<u64>) -> Promise<()> {
        self.calls.borrow_mut().push(format!("update {channel_id} {mode:?} till={till:?}"));
        Promise::resolved(())
    }

    fn rpc_reveal(&self, channel_id: &str, preserve_focus: bool) -> Promise<()> {
        self.calls.borrow_mut().push(format!("reveal {channel_id} preserve={preserve_focus}"));
        Promise::resolved(())
    }

    fn rpc_close(&self, channel_id: &str) -> Promise<()> {
        self.calls.borrow_mut().push(format!("close {channel_id}"));
        Promise::resolved(())
    }

    fn rpc_dispose(&self, channel_id: &str) -> Promise<()> {
        self.calls.borrow_mut().push(format!("dispose {channel_id}"));
        Promise::resolved(())
    }
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

struct Fixture {
    service: Rc<ExtHostOutputService>,
    proxy: Rc<RecordingOutput>,
    loggers: Rc<TestLoggerService>,
    directories: Rc<RecordingDirectories>,
}

fn fixture() -> Fixture {
    let proxy = Rc::new(RecordingOutput::default());
    let context = LocalContext::new();
    let shape: Rc<dyn MainThreadOutputServiceShape> = proxy.clone();
    context.set_erased(MainContext::MAIN_THREAD_OUTPUT_SERVICE.sid, Rc::new(shape));
    let loggers = Rc::new(TestLoggerService {
        writes: Rc::new(RefCell::new(Vec::new())),
        flushes: Rc::new(Cell::new(0)),
        created: RefCell::new(Vec::new()),
        deregistered: RefCell::new(Vec::new()),
        levels: RefCell::new(Vec::new()),
    });
    let directories = Rc::new(RecordingDirectories::default());
    let log_service: LogServiceRef = NullLogService::shared();
    let context_ref: &dyn je_workbench::api::common::ext_host_protocol::IMainContext = &*context;
    let service = ExtHostOutputService::new(
        context_ref,
        URI::file("/logs", je_platform::resources::host_platform()),
        Vec::new(),
        directories.clone(),
        loggers.clone(),
        log_service,
    );
    Fixture { service, proxy, loggers, directories }
}

fn pump() {
    for _ in 0..16 {
        je_base::runtime::turn();
    }
}

fn extension() -> ExtensionIdentifier {
    ExtensionIdentifier::new("pub.ext")
}

#[test]
fn a_plain_channel_registers_creates_its_directory_and_writes_through_the_logger() {
    let fixture = fixture();
    let channel = fixture.service.create_output_channel("Meu Canal", OutputChannelOptions::LanguageId("log".to_owned()), &extension()).expect("cria");
    pump();
    assert_eq!(channel.name(), "Meu Canal");
    assert_eq!(fixture.directories.created.borrow().len(), 1, "a pasta de saídas é criada uma vez");
    assert!(fixture.proxy.calls.borrow()[0].starts_with("register Meu Canal lang=Some(\"log\") ext=pub.ext file=/logs/output_logging_"));
    assert!(fixture.proxy.calls.borrow()[0].ends_with("/1-Meu Canal.log"));

    channel.append_line("olá").expect("escreve");
    channel.append("x").expect("escreve");
    pump();
    assert_eq!(*fixture.loggers.writes.borrow(), vec![(LogLevel::Info, "olá\n".to_owned()), (LogLevel::Info, "x".to_owned())]);

    // `clear` descarrega o logger e informa o deslocamento (em bytes: "olá\n" tem 5, "x" tem 1).
    channel.clear().expect("limpa");
    pump();
    assert!(fixture.proxy.calls.borrow().iter().any(|call| call == "update canal-0 Clear till=Some(6)"));

    channel.replace("novo").expect("troca");
    pump();
    assert!(fixture.proxy.calls.borrow().iter().any(|call| call == "update canal-0 Replace till=Some(6)"));
    assert_eq!(fixture.loggers.writes.borrow().last().expect("escreveu").1, "novo");
}

#[test]
fn only_the_visible_channel_streams_updates_and_show_and_hide_reach_the_workbench() {
    let fixture = fixture();
    let channel = fixture.service.create_output_channel("A", OutputChannelOptions::None, &extension()).expect("cria");
    pump();
    channel.append("antes").expect("escreve");
    pump();
    assert!(!fixture.proxy.calls.borrow().iter().any(|call| call.starts_with("update")), "invisível: sem atualização");

    fixture.service.rpc_set_visible_channel(Some("canal-0".to_owned()));
    channel.append("depois").expect("escreve");
    pump();
    assert!(fixture.proxy.calls.borrow().iter().any(|call| call == "update canal-0 Append till=None"));

    channel.show(Some(je_workbench::api::common::ext_host_output::ShowArgument::PreserveFocus(true)), None).expect("mostra");
    channel.hide().expect("esconde");
    pump();
    let calls = fixture.proxy.calls.borrow();
    assert!(calls.iter().any(|call| call == "reveal canal-0 preserve=true"));
    assert!(calls.iter().any(|call| call == "close canal-0"));
}

#[test]
fn a_disposed_channel_rejects_further_use_and_releases_the_logger() {
    let fixture = fixture();
    let channel = fixture.service.create_output_channel("B", OutputChannelOptions::None, &extension()).expect("cria");
    pump();
    channel.dispose();
    assert_eq!(channel.append("x").expect_err("fechado").message, "Channel has been closed");
    assert!(fixture.proxy.calls.borrow().iter().any(|call| call == "dispose canal-0"));
    assert_eq!(fixture.loggers.deregistered.borrow().len(), 1);
    assert!(fixture.service.channel("canal-0").is_none());
}

#[test]
fn illegal_names_and_language_ids_are_rejected() {
    let fixture = fixture();
    assert_eq!(fixture.service.create_output_channel("   ", OutputChannelOptions::None, &extension()).err().expect("erro").message, "illegal argument `name`. must not be falsy");
    assert_eq!(
        fixture.service.create_output_channel("ok", OutputChannelOptions::LanguageId(" ".to_owned()), &extension()).err().expect("erro").message,
        "illegal argument `languageId`. must not be empty"
    );
}

#[test]
fn log_channels_filter_by_level_share_the_instance_per_file_and_honor_the_registered_level() {
    let fixture = fixture();
    let first = fixture.service.create_output_channel("Servidor", OutputChannelOptions::Log, &extension()).expect("cria");
    let again = fixture.service.create_output_channel("Servidor", OutputChannelOptions::Log, &extension()).expect("cria");
    assert!(Rc::ptr_eq(&first, &again), "o mesmo arquivo de log devolve o mesmo canal");
    pump();
    assert_eq!(first.log_level(), Some(LogLevel::Info));
    assert!(fixture.directories.created.borrow().iter().any(|path| path == "/logs/pub.ext"));
    assert_eq!(fixture.loggers.created.borrow()[0].1.id.as_deref(), Some("pub.ext.Servidor.log"));

    first.debug("ruído").expect("escreve");
    first.warn("atenção").expect("escreve");
    first.append_line("linha").expect("escreve");
    pump();
    let writes = fixture.loggers.writes.borrow();
    assert!(!writes.iter().any(|(_, text)| text == "ruído"), "debug fica abaixo do nível info");
    assert!(writes.iter().any(|(level, text)| *level == LogLevel::Warning && text.contains("atenção")));
    assert!(writes.iter().any(|(level, text)| *level == LogLevel::Info && text == "linha"), "no canal de log `appendLine` não acrescenta quebra de linha");
    drop(writes);

    // O nível configurado pelo usuário para esse arquivo vence o padrão.
    let registered = fixture.service.create_output_channel("Outro", OutputChannelOptions::Log, &extension());
    assert!(registered.is_ok());
    let file = URI::file("/logs/pub.ext/Configurado.log", je_platform::resources::host_platform());
    fixture.loggers.levels.borrow_mut().push((file, LogLevel::Trace));
    let configured = fixture.service.create_output_channel("Configurado", OutputChannelOptions::Log, &extension()).expect("cria");
    assert_eq!(configured.log_level(), Some(LogLevel::Trace));
}
