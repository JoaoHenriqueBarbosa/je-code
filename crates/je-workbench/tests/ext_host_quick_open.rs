//! Teste de unidade do `ExtHostQuickOpen` (`extHostQuickOpen.ts`; o oráculo não tem teste dele, então não é porte): o lado da extensão de
//! `showQuickPick`, `showInputBox` e `createQuickPick`/`createInputBox` contra um dublê do `MainThreadQuickOpenShape` que registra as
//! chamadas (o `RecordingQuickOpen`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::{Promise, Resolver};
use je_base::severity::Severity;
use je_exthost::local_context::LocalContext;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::NullLogService;
use je_platform::quickinput::quick_input::{IPickOptions, InputValidationResult};
use je_platform::request::request::{AuthInfo, Credentials};
use je_base::uri::URI;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, ExtHostCommandsShape, ExtHostQuickOpenShape, IExtHostTelemetry, IInputBoxOptions, IMainContext, MainContext, MainThreadCommandsShape, MainThreadQuickOpenShape,
    MainThreadTelemetryShape, QuickOpenShowResult, TransferQuickInput, TransferQuickInputKind, TransferQuickInputParam, TransferQuickPickItemOrSeparator,
};
use je_workbench::api::common::ext_host_quick_open::{
    ExtHostQuickOpen, InputBoxOptions, InputBoxValidation, QuickPickEntry, QuickPickItem, QuickPickOptions,
};
use je_workbench::api::common::ext_host_types::InputBoxValidationSeverity;
use je_workbench::api::common::ext_host_workspace::{IExtHostWorkspaceProvider, WorkspaceFolder};

/// O que o dublê registrou, em texto, na ordem das chamadas.
#[derive(Default)]
struct Log {
    calls: RefCell<Vec<String>>,
    set_items: RefCell<Vec<Vec<TransferQuickPickItemOrSeparator>>>,
    updates: RefCell<Vec<TransferQuickInput>>,
}

/// O `MainThreadQuickOpenShape` que registra as chamadas e deixa o teste decidir quando o widget fecha.
struct RecordingQuickOpen {
    log: Rc<Log>,
    widget: RefCell<Option<Resolver<Option<QuickOpenShowResult>>>>,
    last_input_options: RefCell<Option<(Option<IInputBoxOptions>, bool)>>,
}

impl IDisposable for RecordingQuickOpen {
    fn dispose(&self) {}
}

impl MainThreadQuickOpenShape for RecordingQuickOpen {
    fn rpc_show(&self, instance: u32, options: IPickOptions, _token: CancellationToken) -> Promise<Option<QuickOpenShowResult>> {
        self.log.calls.borrow_mut().push(format!("show {instance} title={:?} many={:?}", options.title, options.can_pick_many));
        let (promise, resolver) = Promise::new();
        *self.widget.borrow_mut() = Some(resolver);
        promise
    }

    fn rpc_set_items(&self, instance: u32, items: Vec<TransferQuickPickItemOrSeparator>) -> Promise<()> {
        self.log.calls.borrow_mut().push(format!("setItems {instance} count={}", items.len()));
        self.log.set_items.borrow_mut().push(items);
        Promise::resolved(())
    }

    fn rpc_set_error(&self, instance: u32, error: Error) -> Promise<()> {
        self.log.calls.borrow_mut().push(format!("setError {instance} {}", error.message));
        Promise::resolved(())
    }

    fn rpc_input(&self, options: Option<IInputBoxOptions>, validate_input: bool, _token: CancellationToken) -> Promise<Option<String>> {
        self.log.calls.borrow_mut().push(format!("input validate={validate_input}"));
        *self.last_input_options.borrow_mut() = Some((options, validate_input));
        Promise::resolved(Some("digitado".to_owned()))
    }

    fn rpc_create_or_update(&self, params: TransferQuickInput) -> Promise<()> {
        self.log.calls.borrow_mut().push(format!("createOrUpdate {} kind={:?} keys={}", params.id, params.kind, params.params.len()));
        self.log.updates.borrow_mut().push(params);
        Promise::resolved(())
    }

    fn rpc_dispose(&self, id: u32) -> Promise<()> {
        self.log.calls.borrow_mut().push(format!("dispose {id}"));
        Promise::resolved(())
    }
}

/// O `MainThreadCommands` que o `ExtHostCommands` pede e que nenhum caso usa.
struct UnusedCommands;

impl IDisposable for UnusedCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for UnusedCommands {
    fn rpc_register_command(&self, _id: &str) {}
    fn rpc_unregister_command(&self, _id: &str) {}
    fn rpc_fire_command_activation_event(&self, _id: &str) {}

    fn rpc_execute_command(&self, id: &str, _args: CommandArgs, _retry: bool) -> Promise<Dynamic> {
        panic!("o teste não executa o comando {id}")
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        Promise::resolved(Vec::new())
    }
}

struct SilentTelemetry;

impl IDisposable for SilentTelemetry {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for SilentTelemetry {
    fn rpc_public_log(&self, _event_name: &str, _data: Option<Dynamic>) {}
    fn rpc_public_log2(&self, _event_name: &str, _data: Option<Dynamic>) {}
}

struct SilentExtHostTelemetry;

impl IExtHostTelemetry for SilentExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        true
    }
}

/// O `IExtHostWorkspaceProvider` que o `showWorkspaceFolderPick` consulta: nenhum caso o usa.
struct UnusedWorkspace;

impl IExtHostWorkspaceProvider for UnusedWorkspace {
    fn get_workspace_folder2(&self, _uri: &URI, _resolve_parent: Option<bool>) -> Promise<Option<Rc<WorkspaceFolder>>> {
        panic!("não exercitado")
    }

    fn resolve_workspace_folder(&self, _uri: &URI) -> Promise<Option<Rc<WorkspaceFolder>>> {
        panic!("não exercitado")
    }

    fn get_workspace_folders2(&self) -> Promise<Option<Vec<Rc<WorkspaceFolder>>>> {
        Promise::resolved(None)
    }

    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        panic!("não exercitado")
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        panic!("não exercitado")
    }

    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        panic!("não exercitado")
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        panic!("não exercitado")
    }
}

struct Fixture {
    quick_open: Rc<ExtHostQuickOpen>,
    proxy: Rc<RecordingQuickOpen>,
    log: Rc<Log>,
}

fn fixture() -> Fixture {
    let log = Rc::new(Log::default());
    let proxy = Rc::new(RecordingQuickOpen { log: log.clone(), widget: RefCell::new(None), last_input_options: RefCell::new(None) });
    let context = LocalContext::new();
    let quick_shape: Rc<dyn MainThreadQuickOpenShape> = proxy.clone();
    context.set_erased(MainContext::MAIN_THREAD_QUICK_OPEN.sid, Rc::new(quick_shape));
    let commands_shape: Rc<dyn MainThreadCommandsShape> = Rc::new(UnusedCommands);
    context.set_erased(MainContext::MAIN_THREAD_COMMANDS.sid, Rc::new(commands_shape));
    let telemetry_shape: Rc<dyn MainThreadTelemetryShape> = Rc::new(SilentTelemetry);
    context.set_erased(MainContext::MAIN_THREAD_TELEMETRY.sid, Rc::new(telemetry_shape));
    let context_ref: &dyn IMainContext = &*context;
    let commands = ExtHostCommands::new(context_ref, NullLogService::shared(), Rc::new(SilentExtHostTelemetry));
    let _: Rc<dyn ExtHostCommandsShape> = commands.clone();
    let quick_open = ExtHostQuickOpen::new(context_ref, Rc::new(UnusedWorkspace), commands);
    Fixture { quick_open, proxy, log }
}

fn pump() {
    for _ in 0..16 {
        je_base::runtime::turn();
    }
}

fn texts(labels: &[&str]) -> Promise<Vec<QuickPickEntry>> {
    Promise::resolved(labels.iter().map(|label| QuickPickEntry::Text((*label).to_owned())).collect())
}

#[test]
fn show_quick_pick_sends_the_items_and_resolves_with_the_chosen_handle() {
    let fixture = fixture();
    let result = fixture.quick_open.show_quick_pick(
        texts(&["um", "dois"]),
        Some(QuickPickOptions { title: Some("Escolha".to_owned()), can_pick_many: Some(false), ..QuickPickOptions::default() }),
        CancellationToken::None,
    );
    pump();
    assert_eq!(*fixture.log.calls.borrow(), vec!["show 1 title=Some(\"Escolha\") many=Some(false)".to_owned(), "setItems 1 count=2".to_owned()]);
    let sent = fixture.log.set_items.borrow();
    let TransferQuickPickItemOrSeparator::Item(second) = &sent[0][1] else { panic!("esperava um item") };
    assert_eq!((second.handle, second.label.as_deref()), (1, Some("dois")));

    fixture.proxy.widget.borrow().as_ref().expect("o widget abriu").resolve(Some(QuickOpenShowResult::One(1)));
    pump();
    assert_eq!(result.result().expect("assentou").expect("sem erro"), Some(QuickOpenShowResult::One(1)));
}

#[test]
fn show_quick_pick_sends_separators_and_item_properties() {
    let fixture = fixture();
    let items = Promise::resolved(vec![
        QuickPickEntry::Item(Rc::new(QuickPickItem { label: "Grupo".to_owned(), is_separator: true, ..QuickPickItem::default() })),
        QuickPickEntry::Item(Rc::new(QuickPickItem {
            label: "arquivo".to_owned(),
            description: Some("desc".to_owned()),
            detail: Some("detalhe".to_owned()),
            picked: Some(true),
            ..QuickPickItem::default()
        })),
    ]);
    let _ = fixture.quick_open.show_quick_pick(items, None, CancellationToken::None);
    pump();
    let sent = fixture.log.set_items.borrow();
    assert!(matches!(&sent[0][0], TransferQuickPickItemOrSeparator::Separator(separator) if separator.label.as_deref() == Some("Grupo")));
    let TransferQuickPickItemOrSeparator::Item(item) = &sent[0][1] else { panic!("esperava um item") };
    assert_eq!((item.description.as_deref(), item.detail.as_deref(), item.picked, item.handle), (Some("desc"), Some("detalhe"), Some(true), 1));
}

#[test]
fn closing_the_widget_before_the_items_arrive_resolves_undefined_without_sending_items() {
    let fixture = fixture();
    let (items, resolver) = Promise::<Vec<QuickPickEntry>>::new();
    let result = fixture.quick_open.show_quick_pick(items, None, CancellationToken::None);
    fixture.proxy.widget.borrow().as_ref().expect("o widget abriu").resolve(None);
    pump();
    resolver.resolve(vec![QuickPickEntry::Text("tarde".to_owned())]);
    pump();
    assert_eq!(result.result().expect("assentou").expect("sem erro"), None);
    assert!(fixture.log.set_items.borrow().is_empty());
}

#[test]
fn a_failing_items_promise_reports_the_error_to_the_workbench_and_rejects() {
    let fixture = fixture();
    let result = fixture.quick_open.show_quick_pick(Promise::rejected(Error::new("falhou")), None, CancellationToken::None);
    pump();
    assert_eq!(result.result().expect("assentou").expect_err("rejeita").message, "falhou");
    assert!(fixture.log.calls.borrow().iter().any(|call| call == "setError 1 falhou"));
}

#[test]
fn a_cancelled_widget_resolves_undefined() {
    let fixture = fixture();
    let (items, _resolver) = Promise::<Vec<QuickPickEntry>>::new();
    let result = fixture.quick_open.show_quick_pick(items, None, CancellationToken::None);
    fixture.proxy.widget.borrow().as_ref().expect("o widget abriu").reject(Error::cancellation());
    pump();
    assert_eq!(result.result().expect("assentou").expect("a cancelação não rejeita"), None);
}

#[test]
fn on_did_select_item_receives_the_handle_until_the_next_pick() {
    let fixture = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _ = fixture.quick_open.show_quick_pick(
        texts(&["a", "b"]),
        Some(QuickPickOptions { on_did_select_item: Some(Rc::new(move |handle| sink.borrow_mut().push(handle))), ..QuickPickOptions::default() }),
        CancellationToken::None,
    );
    pump();
    fixture.quick_open.rpc_on_item_selected(1);
    assert_eq!(*seen.borrow(), vec![1]);
    let _ = fixture.quick_open.show_quick_pick(texts(&["c"]), None, CancellationToken::None);
    fixture.quick_open.rpc_on_item_selected(0);
    assert_eq!(*seen.borrow(), vec![1], "o `onDidSelectItem` da chamada anterior foi limpo");
}

#[test]
fn show_input_passes_the_options_and_validation_maps_severities() {
    let fixture = fixture();
    let options = InputBoxOptions {
        options: IInputBoxOptions { title: Some("Nome".to_owned()), ..IInputBoxOptions::default() },
        validate_input: Some(Rc::new(|input: &str| {
            je_base::promise::MaybePromise::Value(match input {
                "" => InputBoxValidation::None,
                "texto" => InputBoxValidation::Text("inválido".to_owned()),
                "aviso" => InputBoxValidation::Message { message: "atenção".to_owned(), severity: Some(InputBoxValidationSeverity::Warning) },
                _ => InputBoxValidation::Message { message: "sem gravidade".to_owned(), severity: None },
            })
        })),
    };
    let result = fixture.quick_open.show_input(Some(options), CancellationToken::None);
    pump();
    assert_eq!(result.result().expect("assentou").expect("sem erro"), Some("digitado".to_owned()));
    let (sent, validate) = fixture.proxy.last_input_options.borrow().clone().expect("input foi chamado");
    assert_eq!((sent.and_then(|options| options.title), validate), (Some("Nome".to_owned()), true));

    let validated = |input: &str| fixture.quick_open.rpc_validate_input(input).result().expect("assentou").expect("sem erro");
    assert_eq!(validated(""), InputValidationResult::None);
    assert_eq!(validated("texto"), InputValidationResult::Message("inválido".to_owned()));
    assert_eq!(validated("aviso"), InputValidationResult::Content { content: "atenção".to_owned(), severity: Severity::Warning });
    assert_eq!(validated("outro"), InputValidationResult::Content { content: "sem gravidade".to_owned(), severity: Severity::Error });
}

#[test]
fn quick_pick_sessions_batch_updates_and_route_events_by_session_id() {
    let fixture = fixture();
    let pick = fixture.quick_open.create_quick_pick();
    let id = pick.input().id();
    let (first, second) = (Rc::new(QuickPickItem { label: "a".to_owned(), ..QuickPickItem::default() }), Rc::new(QuickPickItem { label: "b".to_owned(), ..QuickPickItem::default() }));

    // Antes de `show()` nada é enviado: as mudanças ficam acumuladas.
    pick.input().set_title(Some("Sessão".to_owned()));
    pick.set_items(vec![first.clone(), second.clone()]);
    assert!(fixture.log.updates.borrow().is_empty());

    // `show()` envia tudo de uma vez, com o tipo do controlador.
    pick.input().show();
    {
        let updates = fixture.log.updates.borrow();
        assert_eq!(updates.len(), 1);
        assert_eq!((updates[0].id, updates[0].kind), (id, Some(TransferQuickInputKind::QuickPick)));
        assert!(updates[0].params.iter().any(|param| matches!(param, TransferQuickInputParam::Visible(true))));
        assert!(updates[0].params.iter().any(|param| matches!(param, TransferQuickInputParam::Items(Some(items)) if items.len() == 2)));
        assert!(updates[0].params.iter().any(|param| matches!(param, TransferQuickInputParam::Title(Some(title)) if title == "Sessão")));
    }

    // Depois de visível, as mudanças seguintes vão juntas no laço seguinte (um `createOrUpdate` só).
    pick.set_can_select_many(true);
    pick.input().set_busy(true);
    assert_eq!(fixture.log.updates.borrow().len(), 1);
    pump();
    assert_eq!(fixture.log.updates.borrow().len(), 2);
    assert_eq!(fixture.log.updates.borrow()[1].params.len(), 2);

    // Os eventos do workbench chegam à sessão certa e devolvem os itens da extensão por identidade.
    let selected = Rc::new(RefCell::new(Vec::new()));
    let sink = selected.clone();
    let _subscription = pick.on_did_change_selection().listen(move |items| sink.borrow_mut().push(items.iter().map(|item| item.label.clone()).collect::<Vec<_>>()));
    fixture.quick_open.rpc_on_did_change_selection(id, vec![1, 7]);
    fixture.quick_open.rpc_on_did_change_selection(id + 100, vec![0]);
    assert_eq!(*selected.borrow(), vec![vec!["b".to_owned()]], "o handle 7 não existe e o id 100 é de outra sessão");
    assert!(Rc::ptr_eq(&pick.selected_items()[0], &second));

    pick.set_active_items(vec![first.clone(), Rc::new(QuickPickItem::default())]);
    assert_eq!(pick.active_items().len(), 1, "o item que não está na lista é descartado");

    // `dispose()` esconde, avisa o workbench e tira a sessão do registro.
    pick.input().dispose();
    assert!(fixture.log.calls.borrow().iter().any(|call| call == &format!("dispose {id}")));
    fixture.quick_open.rpc_on_did_change_selection(id, vec![0]);
    assert_eq!(selected.borrow().len(), 1, "a sessão descartada não recebe mais eventos");
}

#[test]
fn input_box_validation_message_becomes_message_and_severity() {
    let fixture = fixture();
    let input_box = fixture.quick_open.create_input_box();
    input_box.set_validation_message(Some(InputBoxValidation::Text("ruim".to_owned())));
    input_box.input().show();
    let updates = fixture.log.updates.borrow();
    assert_eq!(updates[0].kind, Some(TransferQuickInputKind::InputBox));
    assert!(updates[0].params.iter().any(|param| matches!(param, TransferQuickInputParam::ValidationMessage(Some(message)) if message == "ruim")));
    assert!(updates[0].params.iter().any(|param| matches!(param, TransferQuickInputParam::Severity(Severity::Error))));
}
