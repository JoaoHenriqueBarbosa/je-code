//! Teste de ponta a ponta de `vscode.window.createQuickPick` e `vscode.window.createInputBox` (não é porte do oráculo): uma extensão de
//! exemplo em JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), cria as sessões,
//! muda propriedades, assina os eventos e as fecha.
//!
//! O lado do ExtHost é o `ExtHostQuickOpen` REAL (portado de `extHostQuickOpen.ts`), ligado pelo host em processo. O lado do workbench é
//! um dublê do `MainThreadQuickOpenShape` ([`Screen`]) que faz o papel da thread principal: anota cada `$createOrUpdate` e `$dispose` e,
//! quando o teste manda, entrega ao ExtHost o que o usuário faria diante da interface (`$onDidChangeActive`, `$onDidChangeSelection`,
//! `$onDidChangeValue`, `$onDidTriggerButton`, `$onDidAccept`, `$onDidHide`). O `showQuickPick` e o `showInputBox` têm o teste deles em
//! `extension_host_api_window_quick_open.rs`.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use harness::{run_extension, Workbench};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_exthost::in_process::IMainThreadBinder;
use je_platform::quickinput::quick_input::IPickOptions;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostQuickOpenShape, IInputBoxOptions, IMainContext, MainContext, MainThreadQuickOpenShape, QuickOpenShowResult, TransferQuickInput,
    TransferQuickInputKind, TransferQuickInputParam, TransferQuickPickItemOrSeparator,
};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  const labels = (items) => items.map((item) => item.label).join(',');

  command('t.openPick', () => {
    const pick = vscode.window.createQuickPick();
    globalThis.__pick = pick;
    pick.title = 'Escolha';
    pick.placeholder = 'Digite para filtrar';
    pick.canSelectMany = true;
    pick.busy = true;
    pick.items = [{ label: 'alfa', description: 'a', extra: 1 }, { label: 'beta' }, { label: 'gama' }];
    pick.activeItems = [pick.items[1]];
    pick.selectedItems = [pick.items[0], pick.items[2]];
    pick.buttons = [vscode.QuickInputButtons.Back, { iconPath: vscode.Uri.file('/tmp/mais.svg'), tooltip: 'Mais' }];
    pick.onDidAccept(() => log('accept:' + labels(pick.selectedItems)));
    pick.onDidChangeValue((value) => log('value:' + value + '|' + pick.value));
    pick.onDidChangeActive((items) => log('active:' + labels(items)));
    pick.onDidChangeSelection((items) => log('selection:' + items.map((item) => item.label + (item.extra ? '*' : '')).join(',')));
    pick.onDidTriggerButton((button) => log('button:' + (button.tooltip || button.iconPath.id)));
    pick.onDidHide(() => log('hide'));
    pick.show();
    return 'open';
  });
  command('t.pickState', () => {
    const pick = globalThis.__pick;
    return JSON.stringify({
      title: pick.title,
      placeholder: pick.placeholder,
      canSelectMany: pick.canSelectMany,
      busy: pick.busy,
      value: pick.value,
      items: labels(pick.items),
      active: labels(pick.activeItems),
      selected: labels(pick.selectedItems),
      buttons: pick.buttons.length,
    });
  });
  command('t.pickUpdate', () => {
    const pick = globalThis.__pick;
    pick.value = 'x';
    pick.busy = false;
    pick.items = [{ label: 'so' }];
    return 'ok';
  });
  command('t.pickClose', () => {
    const pick = globalThis.__pick;
    pick.hide();
    pick.dispose();
    return 'closed';
  });
  command('t.pickAfterDispose', () => {
    try {
      globalThis.__pick.value;
      return 'sem erro';
    } catch (error) {
      return 'erro:' + error.message.includes('descartada');
    }
  });

  command('t.openInput', () => {
    const box = vscode.window.createInputBox();
    globalThis.__box = box;
    box.title = 'Nome';
    box.prompt = 'Digite o nome';
    box.placeholder = 'fulano';
    box.password = true;
    box.value = 'abc';
    box.valueSelection = [1, 2];
    box.validationMessage = 'curto';
    box.onDidAccept(() => log('input-accept:' + box.value));
    box.onDidChangeValue((value) => log('input-value:' + value));
    box.show();
    return 'open';
  });
  command('t.inputState', () => {
    const box = globalThis.__box;
    return JSON.stringify({ value: box.value, password: box.password, prompt: box.prompt, selection: box.valueSelection, validation: box.validationMessage });
  });
};
"#;

/// Um `$createOrUpdate` como o dublê o anotou: o id da sessão, o tipo (só no primeiro) e cada chave resumida em texto.
#[derive(Debug, PartialEq)]
struct Update {
    id: u32,
    kind: &'static str,
    params: Vec<String>,
}

fn describe_text(value: &Option<String>) -> String {
    value.clone().unwrap_or_default()
}

fn describe(param: &TransferQuickInputParam) -> String {
    match param {
        TransferQuickInputParam::Title(title) => format!("title={}", describe_text(title)),
        TransferQuickInputParam::Placeholder(placeholder) => format!("placeholder={}", describe_text(placeholder)),
        TransferQuickInputParam::Prompt(prompt) => format!("prompt={}", describe_text(prompt)),
        TransferQuickInputParam::CanSelectMany(value) => format!("canSelectMany={value}"),
        TransferQuickInputParam::Busy(value) => format!("busy={value}"),
        TransferQuickInputParam::Password(value) => format!("password={value}"),
        TransferQuickInputParam::Visible(value) => format!("visible={value}"),
        TransferQuickInputParam::Value(value) => format!("value={value}"),
        TransferQuickInputParam::ValueSelection(selection) => match selection {
            Some((start, end)) => format!("valueSelection={start}-{end}"),
            None => "valueSelection=none".to_owned(),
        },
        TransferQuickInputParam::ValidationMessage(message) => format!("validation={}", describe_text(message)),
        TransferQuickInputParam::Severity(severity) => format!("severity={severity:?}"),
        TransferQuickInputParam::Items(items) => {
            let labels: Vec<String> = items
                .iter()
                .flatten()
                .map(|item| match item {
                    TransferQuickPickItemOrSeparator::Item(item) => item.label.clone().unwrap_or_default(),
                    TransferQuickPickItemOrSeparator::Separator(separator) => format!("--{}", separator.label.clone().unwrap_or_default()),
                })
                .collect();
            format!("items={}", labels.join(","))
        }
        TransferQuickInputParam::ActiveItems(handles) => format!("active={}", handles.iter().map(u32::to_string).collect::<Vec<_>>().join(",")),
        TransferQuickInputParam::SelectedItems(handles) => format!("selected={}", handles.iter().map(u32::to_string).collect::<Vec<_>>().join(",")),
        TransferQuickInputParam::Buttons(buttons) => {
            format!("buttons={}", buttons.iter().map(|button| format!("{}:{}", button.handle, describe_text(&button.tooltip))).collect::<Vec<_>>().join(","))
        }
        _ => "other".to_owned(),
    }
}

/// O papel da thread principal: o `MainThreadQuickOpenShape` que anota as sessões.
struct Screen {
    ext_host: RefCell<Option<Rc<dyn ExtHostQuickOpenShape>>>,
    updates: RefCell<Vec<Update>>,
    disposed: RefCell<Vec<u32>>,
}

impl Screen {
    fn new() -> Rc<Self> {
        Rc::new(Self { ext_host: RefCell::new(None), updates: RefCell::new(Vec::new()), disposed: RefCell::new(Vec::new()) })
    }

    fn ext_host(&self) -> Rc<dyn ExtHostQuickOpenShape> {
        self.ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    /// O id da sessão do primeiro `$createOrUpdate` (a única sessão aberta pelo caso).
    fn session_id(&self) -> u32 {
        self.updates.borrow().first().expect("a extensão abriu uma sessão").id
    }

    fn params_of(&self, index: usize) -> Vec<String> {
        self.updates.borrow()[index].params.clone()
    }
}

impl IDisposable for Screen {
    fn dispose(&self) {}
}

impl MainThreadQuickOpenShape for Screen {
    fn rpc_show(&self, _instance: u32, _options: IPickOptions, _token: CancellationToken) -> Promise<Option<QuickOpenShowResult>> {
        Promise::resolved(None)
    }

    fn rpc_set_items(&self, _instance: u32, _items: Vec<TransferQuickPickItemOrSeparator>) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_set_error(&self, _instance: u32, _error: Error) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_input(&self, _options: Option<IInputBoxOptions>, _validate_input: bool, _token: CancellationToken) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn rpc_create_or_update(&self, params: TransferQuickInput) -> Promise<()> {
        let kind = match params.kind {
            Some(TransferQuickInputKind::QuickPick) => "quickPick",
            Some(TransferQuickInputKind::InputBox) => "inputBox",
            _ => "-",
        };
        self.updates.borrow_mut().push(Update { id: params.id, kind, params: params.params.iter().map(describe).collect() });
        Promise::resolved(())
    }

    fn rpc_dispose(&self, session_id: u32) -> Promise<()> {
        self.disposed.borrow_mut().push(session_id);
        Promise::resolved(())
    }
}

/// Liga o dublê da thread principal ao host: registra o cliente e entrega a ele o proxy do ExtHost.
struct Binder {
    screen: Rc<Screen>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        *self.screen.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_QUICK_OPEN));
        let screen: Rc<dyn MainThreadQuickOpenShape> = self.screen.clone();
        context.set(&MainContext::MAIN_THREAD_QUICK_OPEN, screen);
        Vec::new()
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<Screen>) {
    let screen = Screen::new();
    (Rc::new(Binder { screen: screen.clone() }), screen)
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn create_quick_pick_sends_every_property_in_one_update_when_shown() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, screen| {
        assert_eq!(run(workbench, "t.openPick"), Dynamic::string("open"));

        let updates = screen.updates.borrow();
        assert_eq!(updates.len(), 1, "as propriedades acumulam até o `show`, e um `$createOrUpdate` as leva");
        assert_eq!(updates[0].kind, "quickPick");
        assert_eq!(
            updates[0].params,
            strings(&[
                "title=Escolha",
                "placeholder=Digite para filtrar",
                "canSelectMany=true",
                "busy=true",
                "items=alfa,beta,gama",
                "active=1",
                "selected=0,2",
                "buttons=-1:,1:Mais",
                "visible=true",
            ]),
            "activeItems e selectedItems viajam como os handles dos itens que a extensão passou; o Back é o handle -1"
        );
    });
}

#[test]
fn quick_pick_events_carry_the_items_the_extension_passed() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, screen| {
        run(workbench, "t.openPick");
        let id = screen.session_id();

        screen.ext_host().rpc_on_did_change_active(id, vec![0]);
        screen.ext_host().rpc_on_did_change_selection(id, vec![0, 1]);
        screen.ext_host().rpc_on_did_change_value(id, "be");
        screen.ext_host().rpc_on_did_trigger_button(id, Some(-1), None);
        screen.ext_host().rpc_on_did_trigger_button(id, Some(1), None);
        screen.ext_host().rpc_on_did_accept(id);
        screen.ext_host().rpc_on_did_hide(id);
        workbench.pump();

        assert_eq!(
            events(workbench),
            r#"["active:alfa","selection:alfa*,beta","value:be|be","button:arrow-left","button:Mais","accept:alfa,beta","hide"]"#,
            "os itens dos eventos são as cópias da extensão (com `extra`), o valor já está atualizado no `onDidChangeValue`"
        );
        assert_eq!(
            run(workbench, "t.pickState"),
            Dynamic::string(r#"{"title":"Escolha","placeholder":"Digite para filtrar","canSelectMany":true,"busy":true,"value":"be","items":"alfa,beta,gama","active":"alfa","selected":"alfa,beta","buttons":2}"#)
        );
    });
}

#[test]
fn changes_after_show_are_batched_and_dispose_releases_the_session() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, screen| {
        run(workbench, "t.openPick");
        let id = screen.session_id();

        run(workbench, "t.pickUpdate");
        std::thread::sleep(Duration::from_millis(50));
        workbench.pump();
        assert_eq!(screen.params_of(1), strings(&["value=x", "busy=false", "items=so"]), "as mudanças de um mesmo laço viajam juntas");

        assert_eq!(run(workbench, "t.pickClose"), Dynamic::string("closed"));
        assert_eq!(screen.updates.borrow().last().expect("há atualizações").params, strings(&["visible=false"]));
        assert_eq!(*screen.disposed.borrow(), vec![id]);
        assert_eq!(events(workbench), r#"["hide"]"#, "o dispose avisa o onDidHide de uma sessão visível");
        assert_eq!(run(workbench, "t.pickAfterDispose"), Dynamic::string("erro:true"));
    });
}

#[test]
fn create_input_box_sends_its_properties_and_forwards_value_and_accept() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, screen| {
        assert_eq!(run(workbench, "t.openInput"), Dynamic::string("open"));
        {
            let updates = screen.updates.borrow();
            assert_eq!(updates.len(), 1);
            assert_eq!(updates[0].kind, "inputBox");
            assert_eq!(
                updates[0].params,
                strings(&[
                    "title=Nome",
                    "prompt=Digite o nome",
                    "placeholder=fulano",
                    "password=true",
                    "value=abc",
                    "valueSelection=1-2",
                    "validation=curto",
                    "severity=Error",
                    "visible=true",
                ])
            );
        }

        let id = screen.session_id();
        screen.ext_host().rpc_on_did_change_value(id, "abcd");
        screen.ext_host().rpc_on_did_accept(id);
        workbench.pump();
        assert_eq!(events(workbench), r#"["input-value:abcd","input-accept:abcd"]"#);
        assert_eq!(
            run(workbench, "t.inputState"),
            Dynamic::string(r#"{"value":"abcd","password":true,"prompt":"Digite o nome","selection":[1,2],"validation":"curto"}"#)
        );
    });
}
