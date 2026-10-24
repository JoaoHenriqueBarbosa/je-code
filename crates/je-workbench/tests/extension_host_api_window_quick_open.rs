//! Teste de ponta a ponta de `vscode.window.showQuickPick` e `vscode.window.showInputBox` (não é porte do oráculo): uma extensão de
//! exemplo em JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), abre seletores e
//! caixas de entrada, e o efeito aparece no `IQuickInputService` do workbench.
//!
//! O lado do workbench é o `MainThreadQuickOpen` REAL, ligado pelo binder do teste sobre um `IQuickInputService` roteirizado
//! (`ScriptedQuickInput`): cada chamada de `pick`, `pick_many` e `input` grava o que o `MainThreadQuickOpen` entregou (título, texto
//! de apoio, itens, opções) e faz o que o roteiro do teste manda no lugar do usuário (focar itens, escolher rótulos, fechar, digitar
//! texto para a validação, ou esperar o token de cancelamento da extensão).
//!
//! - **`showQuickPick`**: itens de texto e objeto (com separador, `description`, `detail`, `picked`), `canPickMany`, `placeHolder`,
//!   `title`, itens por promessa (resolvida e rejeitada), `onDidSelectItem`, fechar sem escolher e cancelar por token.
//! - **`showInputBox`**: `title`, `prompt`, `placeHolder`, `value`, `valueSelection`, `password`, `ignoreFocusOut`, `validateInput`
//!   (texto, objeto com gravidade, promessa, e a que lança) e o cancelamento por token.
//! - **Enumerações**: `QuickPickItemKind`, `InputBoxValidationSeverity` e `QuickInputButtons`.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use harness::{run_extension, Workbench};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::observable::base::IObservable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_service::{ILanguageSelection, LanguageService};
use je_editor::model::TextModelCreationOptions;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::text_model_edit_source::TextModelEditSource;
use je_exthost::in_process::IMainThreadBinder;
use je_platform::quickinput::quick_input::{
    IInputBox, IInputOptions, IKeyMods, IPickOptions, IQuickInputService, IQuickNavigateConfiguration, IQuickPick, IQuickPickItem, IQuickTree, InputValidationResult,
    QuickInputAlignment, QuickInputAlignmentSetting, QuickInputAnchor, QuickInputAnchorPosition, QuickInputButtonRef, QuickInputHideReason, QuickInputRef,
    QuickPickCreateOptions, QuickPickInput, QuickPicks,
};
use je_workbench::api::browser::main_thread_quick_open::MainThreadQuickOpen;
use je_workbench::api::common::ext_host_protocol::{IMainContext, MainContext, MainThreadQuickOpenShape};
use je_workbench::services::editor::custom_editor_label_service::ICustomEditorLabelService;
use je_workbench::test::browser::workbench_test_services::MockLabelService;

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));

  command('t.pickText', async () => {
    const chosen = await vscode.window.showQuickPick(['um', 'dois', 'três'], { placeHolder: 'Escolha um', title: 'Título do seletor' });
    return String(chosen);
  });
  command('t.pickObjects', async () => {
    const chosen = await vscode.window.showQuickPick([
      { label: 'Frutas', kind: vscode.QuickPickItemKind.Separator },
      { label: 'maçã', description: 'vermelha', detail: 'uma fruta', picked: true },
      { label: 'pera' },
    ]);
    return chosen ? chosen.label : 'nada';
  });
  command('t.pickMixed', async () => {
    const chosen = await vscode.window.showQuickPick(['texto', { label: 'objeto', description: 'd' }]);
    return typeof chosen === 'string' ? 'string:' + chosen : 'object:' + chosen.label;
  });
  command('t.pickMany', async () => {
    const chosen = await vscode.window.showQuickPick(['a', 'b', 'c'], { canPickMany: true, ignoreFocusOut: true, matchOnDescription: true, matchOnDetail: true });
    return Array.isArray(chosen) ? chosen.join('|') : String(chosen);
  });
  command('t.pickPromise', async () => {
    const chosen = await vscode.window.showQuickPick(Promise.resolve(['x', 'y']));
    return String(chosen);
  });
  command('t.pickRejected', async () => {
    try {
      await vscode.window.showQuickPick(Promise.reject(new Error('falhou ao listar')));
      return 'sem erro';
    } catch (error) {
      return 'erro:' + error.message;
    }
  });
  command('t.pickInvalid', async () => {
    try {
      await vscode.window.showQuickPick([42]);
      return 'sem erro';
    } catch (error) {
      return 'erro:' + error.message;
    }
  });
  command('t.pickFocus', async () => {
    globalThis.__focused = [];
    const chosen = await vscode.window.showQuickPick(
      [{ label: 'alfa' }, { label: 'beta' }, { label: 'gama' }],
      { onDidSelectItem: (item) => globalThis.__focused.push(item.label) },
    );
    return globalThis.__focused.join(',') + '|' + (chosen ? chosen.label : 'nada');
  });
  command('t.pickClosed', async () => String(await vscode.window.showQuickPick(['a', 'b'])));
  command('t.pickCancelled', async () => {
    const source = new vscode.CancellationTokenSource();
    const pending = vscode.window.showQuickPick(['a', 'b'], {}, source.token);
    source.cancel();
    return String(await pending);
  });

  command('t.input', async () => {
    const value = await vscode.window.showInputBox({
      title: 'Nome',
      prompt: 'Digite o nome',
      placeHolder: 'fulano',
      value: 'abc',
      valueSelection: [1, 2],
      password: true,
      ignoreFocusOut: true,
    });
    return String(value);
  });
  command('t.inputDefault', async () => String(await vscode.window.showInputBox()));
  command('t.inputValidated', async () => {
    const value = await vscode.window.showInputBox({
      validateInput: (text) => {
        if (text.length < 3) { return 'curto'; }
        if (text === 'info') { return { message: 'só informativo', severity: vscode.InputBoxValidationSeverity.Info }; }
        if (text === 'sem gravidade') { return { message: 'mensagem solta' }; }
        if (text === 'async') { return Promise.resolve('assíncrono'); }
        if (text === 'boom') { throw new Error('explodiu'); }
        return null;
      },
    });
    return String(value);
  });
  command('t.inputClosed', async () => String(await vscode.window.showInputBox({ prompt: 'x' })));
  command('t.inputCancelled', async () => {
    const source = new vscode.CancellationTokenSource();
    const pending = vscode.window.showInputBox({ prompt: 'x' }, source.token);
    source.cancel();
    return String(await pending);
  });

  command('t.enums', () => JSON.stringify([
    vscode.QuickPickItemKind.Separator,
    vscode.QuickPickItemKind[-1],
    vscode.QuickPickItemKind.Default,
    vscode.InputBoxValidationSeverity.Error,
    vscode.InputBoxValidationSeverity[2],
    vscode.QuickInputButtons.Back.iconPath.id,
  ]));
};
"#;

/// `mock<ICustomEditorLabelService>()`: nenhum caso usa `resourceUri`, então nada o consulta.
struct UnusedCustomEditorLabelService;

impl ICustomEditorLabelService for UnusedCustomEditorLabelService {
    fn on_did_change(&self) -> Event<()> {
        panic!("onDidChange não é usado por este teste")
    }

    fn get_name(&self, _resource: &URI) -> Option<String> {
        panic!("getName não é usado por este teste")
    }
}

/// `mock<IModelService>()`: o `MainThreadQuickOpen` só o usa para os ícones de recurso, que nenhum caso exercita.
struct UnusedModelService;

impl IModelService for UnusedModelService {
    fn get_model(&self, _resource: &URI) -> Option<Rc<TextModel>> {
        panic!("getModel não é usado por este teste")
    }

    fn get_models(&self) -> Vec<Rc<TextModel>> {
        panic!("getModels não é usado por este teste")
    }

    fn on_model_added(&self) -> Event<Rc<TextModel>> {
        panic!("onModelAdded não é usado por este teste")
    }

    fn get_creation_options(&self, _language: &str, _resource: Option<&URI>, _is_for_simple_widget: bool) -> TextModelCreationOptions {
        panic!("getCreationOptions não é usado por este teste")
    }

    fn create_model(&self, _value: TextSource, _language_selection: Option<&dyn ILanguageSelection>, _resource: Option<&URI>, _is_for_simple_widget: bool) -> Rc<TextModel> {
        panic!("createModel não é usado por este teste")
    }

    fn update_model(&self, _model: &TextModel, _value: TextSource, _reason: Option<TextModelEditSource>) {
        panic!("updateModel não é usado por este teste")
    }

    fn destroy_model(&self, _resource: &URI) {
        panic!("destroyModel não é usado por este teste")
    }
}

/// Um item como o seletor o recebeu.
#[derive(Debug, PartialEq)]
enum ShownItem {
    Item { label: String, description: Option<String>, detail: Option<String>, picked: Option<bool> },
    Separator(String),
}

impl ShownItem {
    fn plain(label: &str) -> ShownItem {
        ShownItem::Item { label: label.to_owned(), description: None, detail: None, picked: None }
    }
}

/// Um seletor como o `MainThreadQuickOpen` o abriu: as opções e a lista de itens.
#[derive(Debug, Default, PartialEq)]
struct ShownPick {
    title: Option<String>,
    place_holder: Option<String>,
    can_pick_many: Option<bool>,
    ignore_focus_lost: Option<bool>,
    match_on_description: Option<bool>,
    match_on_detail: Option<bool>,
    items: Vec<ShownItem>,
}

/// Uma caixa de entrada como o `MainThreadQuickOpen` a abriu.
#[derive(Debug, PartialEq)]
struct ShownInput {
    title: Option<String>,
    prompt: Option<String>,
    value: Option<String>,
    value_selection: Option<(u32, u32)>,
    place_holder: Option<String>,
    password: Option<bool>,
    ignore_focus_lost: Option<bool>,
    validated: bool,
}

/// O que o usuário faz depois de ver a lista.
#[derive(Clone)]
enum PickOutcome {
    /// Escolhe estes rótulos (um só sem `canPickMany`).
    Choose(Vec<&'static str>),
    /// Fecha o seletor sem escolher.
    Close,
    /// Não faz nada: o seletor só termina se o token da extensão cancelar.
    Wait,
}

/// O roteiro do usuário diante de um seletor: os rótulos que ele focaliza (`onDidSelectItem`) e o desfecho.
#[derive(Clone)]
struct PickScript {
    focus: Vec<&'static str>,
    outcome: PickOutcome,
}

impl PickScript {
    fn choose(labels: &[&'static str]) -> PickScript {
        PickScript { focus: Vec::new(), outcome: PickOutcome::Choose(labels.to_vec()) }
    }

    fn with_focus(mut self, labels: &[&'static str]) -> PickScript {
        self.focus = labels.to_vec();
        self
    }
}

/// O que o usuário faz diante de uma caixa de entrada.
#[derive(Clone)]
enum InputOutcome {
    Accept(&'static str),
    Close,
    Wait,
}

/// O roteiro do usuário diante de uma caixa de entrada: os textos que digita (cada um passa pela validação) e o desfecho.
#[derive(Clone)]
struct InputScript {
    typed: Vec<&'static str>,
    outcome: InputOutcome,
}

/// O resultado de uma validação como texto, para o teste comparar.
fn describe_validation(result: &Result<InputValidationResult, Error>) -> String {
    match result {
        Ok(InputValidationResult::None) => "none".to_owned(),
        Ok(InputValidationResult::Message(message)) => format!("message:{message}"),
        Ok(InputValidationResult::Content { content, severity }) => format!("content:{content}:{severity:?}"),
        Err(error) => format!("error:{}", error.message),
    }
}

fn absent(member: &str) -> ! {
    panic!("{member} não é usado por este teste")
}

/// O `IQuickInputService` que faz o que o roteiro manda no lugar do usuário e grava o que o `MainThreadQuickOpen` entregou.
struct ScriptedQuickInput {
    pick_scripts: RefCell<VecDeque<PickScript>>,
    input_scripts: RefCell<VecDeque<InputScript>>,
    shown_picks: Rc<RefCell<Vec<ShownPick>>>,
    shown_inputs: Rc<RefCell<Vec<ShownInput>>>,
    /// O que `validateInput` produziu para cada texto digitado, na ordem em que a validação assentou.
    validations: Rc<RefCell<Vec<(String, String)>>>,
}

impl ScriptedQuickInput {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            pick_scripts: RefCell::new(VecDeque::new()),
            input_scripts: RefCell::new(VecDeque::new()),
            shown_picks: Rc::default(),
            shown_inputs: Rc::default(),
            validations: Rc::default(),
        })
    }

    fn script_pick(&self, script: PickScript) {
        self.pick_scripts.borrow_mut().push_back(script);
    }

    fn script_input(&self, typed: &[&'static str], outcome: InputOutcome) {
        self.input_scripts.borrow_mut().push_back(InputScript { typed: typed.to_vec(), outcome });
    }

    /// A validação que `validateInput` deu para `typed`.
    fn validation_of(&self, typed: &str) -> String {
        self.validations.borrow().iter().find(|(known, _)| known == typed).map(|(_, result)| result.clone()).unwrap_or_else(|| panic!("o texto '{typed}' não foi validado"))
    }

    /// O corpo comum de `pick` e `pick_many`: grava o seletor e, quando os itens chegam, executa o roteiro. `finish` recebe os itens
    /// escolhidos (`None` é o seletor fechado) ou o erro que rejeitou os itens.
    fn run_pick(&self, picks: QuickPicks, options: Option<&IPickOptions>, token: Option<&CancellationToken>, finish: Rc<dyn Fn(Result<Option<Vec<IQuickPickItem>>, Error>)>) {
        let script = self.pick_scripts.borrow_mut().pop_front().expect("o teste não roteirizou este seletor");
        let options = options.expect("o MainThreadQuickOpen sempre passa as opções");
        let mut shown = ShownPick {
            title: options.title.clone(),
            place_holder: options.place_holder.clone(),
            can_pick_many: options.can_pick_many,
            ignore_focus_lost: options.ignore_focus_lost,
            match_on_description: options.match_on_description,
            match_on_detail: options.match_on_detail,
            items: Vec::new(),
        };
        let on_did_focus = options.on_did_focus.clone();

        // O token cancelado fecha o seletor sem escolha (a primeira resolução vale, as demais não fazem nada).
        if let Some(token) = token {
            if token.is_cancellation_requested() {
                finish(Ok(None));
                return;
            }
            let cancelled = finish.clone();
            let _subscription = token.on_cancellation_requested().listen(move |_| cancelled(Ok(None)));
        }

        let shown_picks = self.shown_picks.clone();
        let _ = picks.into_promise().then_result(move |outcome| {
            let entries = match outcome {
                Ok(entries) => entries,
                Err(error) => {
                    finish(Err(error));
                    return Ok(());
                }
            };
            shown.items = entries.iter().map(shown_item_of).collect();
            shown_picks.borrow_mut().push(shown);
            let items: Vec<IQuickPickItem> = entries
                .into_iter()
                .filter_map(|entry| match entry {
                    QuickPickInput::Item(item) => Some(item),
                    QuickPickInput::Separator(_) => None,
                })
                .collect();
            let find = |label: &str| items.iter().find(|item| item.base.label.to_string_lossy() == label).unwrap_or_else(|| panic!("não há o item '{label}'")).clone();
            if let Some(on_did_focus) = &on_did_focus {
                for label in &script.focus {
                    on_did_focus(&find(label));
                }
            }
            match &script.outcome {
                PickOutcome::Choose(labels) => finish(Ok(Some(labels.iter().map(|label| find(label)).collect()))),
                PickOutcome::Close => finish(Ok(None)),
                PickOutcome::Wait => {}
            }
            Ok(())
        });
    }
}

fn shown_item_of(entry: &QuickPickInput) -> ShownItem {
    match entry {
        QuickPickInput::Item(item) => ShownItem::Item {
            label: item.base.label.to_string_lossy(),
            description: item.base.description.as_ref().map(|description| description.to_string_lossy()),
            detail: item.detail.clone(),
            picked: item.picked,
        },
        QuickPickInput::Separator(separator) => ShownItem::Separator(separator.label.clone().unwrap_or_default()),
    }
}

impl IQuickInputService for ScriptedQuickInput {
    fn pick(&self, picks: QuickPicks, options: Option<&IPickOptions>, token: Option<&CancellationToken>) -> Promise<Option<IQuickPickItem>> {
        let (promise, resolver) = Promise::new();
        self.run_pick(
            picks,
            options,
            token,
            Rc::new(move |result| match result {
                Ok(items) => resolver.resolve(items.and_then(|items| items.into_iter().next())),
                Err(error) => resolver.reject(error),
            }),
        );
        promise
    }

    fn pick_many(&self, picks: QuickPicks, options: Option<&IPickOptions>, token: Option<&CancellationToken>) -> Promise<Option<Vec<IQuickPickItem>>> {
        let (promise, resolver) = Promise::new();
        self.run_pick(
            picks,
            options,
            token,
            Rc::new(move |result| match result {
                Ok(items) => resolver.resolve(items),
                Err(error) => resolver.reject(error),
            }),
        );
        promise
    }

    fn input(&self, options: Option<&IInputOptions>, token: Option<&CancellationToken>) -> Promise<Option<String>> {
        let script = self.input_scripts.borrow_mut().pop_front().expect("o teste não roteirizou esta caixa de entrada");
        let options = options.expect("o MainThreadQuickOpen sempre passa as opções");
        self.shown_inputs.borrow_mut().push(ShownInput {
            title: options.title.clone(),
            prompt: options.prompt.clone(),
            value: options.value.clone(),
            value_selection: options.value_selection,
            place_holder: options.place_holder.clone(),
            password: options.password,
            ignore_focus_lost: options.ignore_focus_lost,
            validated: options.validate_input.is_some(),
        });
        let (promise, resolver) = Promise::new();
        if let Some(token) = token {
            if token.is_cancellation_requested() {
                resolver.resolve(None);
                return promise;
            }
            let cancelled = resolver.clone();
            let _subscription = token.on_cancellation_requested().listen(move |_| cancelled.resolve(None));
        }
        if let Some(validate) = &options.validate_input {
            for typed in &script.typed {
                let (typed, validations) = ((*typed).to_owned(), self.validations.clone());
                let validated = validate(&typed);
                let _ = validated.then_result(move |result| {
                    validations.borrow_mut().push((typed, describe_validation(&result)));
                    Ok(())
                });
            }
        }
        match script.outcome {
            InputOutcome::Accept(text) => resolver.resolve(Some(text.to_owned())),
            InputOutcome::Close => resolver.resolve(None),
            InputOutcome::Wait => {}
        }
        promise
    }

    fn back_button(&self) -> QuickInputButtonRef {
        absent("backButton")
    }

    fn on_show(&self) -> Event<()> {
        absent("onShow")
    }

    fn on_hide(&self) -> Event<()> {
        absent("onHide")
    }

    fn alignment(&self) -> IObservable<QuickInputAlignment> {
        absent("alignment")
    }

    fn with_quick_input_anchor(
        &self,
        _anchor: Option<QuickInputAnchor>,
        _anchor_position: Option<QuickInputAnchorPosition>,
        _operation: Box<dyn FnOnce() -> Promise<Dynamic>>,
    ) -> Promise<Dynamic> {
        absent("withQuickInputAnchor")
    }

    fn create_quick_pick(&self, _options: Option<QuickPickCreateOptions>) -> Rc<dyn IQuickPick> {
        absent("createQuickPick")
    }

    fn create_input_box(&self) -> Rc<dyn IInputBox> {
        absent("createInputBox")
    }

    fn create_quick_tree(&self) -> Rc<dyn IQuickTree> {
        absent("createQuickTree")
    }

    fn focus(&self) {
        absent("focus")
    }

    fn toggle(&self) {
        absent("toggle")
    }

    fn navigate(&self, _next: bool, _quick_navigate: Option<&IQuickNavigateConfiguration>) {
        absent("navigate")
    }

    fn back(&self) -> Promise<()> {
        absent("back")
    }

    fn accept(&self, _key_mods: Option<IKeyMods>) -> Promise<()> {
        absent("accept")
    }

    fn cancel(&self, _reason: Option<QuickInputHideReason>) -> Promise<()> {
        absent("cancel")
    }

    fn toggle_hover(&self) {
        absent("toggleHover")
    }

    fn current_quick_input(&self) -> Option<QuickInputRef> {
        absent("currentQuickInput")
    }

    fn set_alignment(&self, _alignment: QuickInputAlignmentSetting) {
        absent("setAlignment")
    }
}

/// Liga o `MainThreadQuickOpen` (real) ao host, sobre o `IQuickInputService` roteirizado.
struct Binder {
    quick_input: Rc<ScriptedQuickInput>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let quick_open = MainThreadQuickOpen::new(
            &*context,
            self.quick_input.clone(),
            MockLabelService::new(None),
            Rc::new(UnusedCustomEditorLabelService),
            Rc::new(UnusedModelService),
            LanguageService::new(false),
        );
        let shape: Rc<dyn MainThreadQuickOpenShape> = quick_open.clone();
        context.set(&MainContext::MAIN_THREAD_QUICK_OPEN, shape);
        let client: Rc<dyn IDisposable> = quick_open;
        vec![client]
    }
}

struct Observed {
    quick_input: Rc<ScriptedQuickInput>,
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Observed) {
    let quick_input = ScriptedQuickInput::new();
    (Rc::new(Binder { quick_input: quick_input.clone() }), Observed { quick_input })
}

#[test]
fn quick_pick_with_text_items_sends_the_options_and_resolves_the_chosen_text() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_pick(PickScript::choose(&["dois"]));
        assert_eq!(workbench.execute("t.pickText", Vec::new()), Dynamic::string("dois"));

        let picks = observed.quick_input.shown_picks.borrow();
        assert_eq!(
            *picks,
            vec![ShownPick {
                title: Some("Título do seletor".to_owned()),
                place_holder: Some("Escolha um".to_owned()),
                items: vec![ShownItem::plain("um"), ShownItem::plain("dois"), ShownItem::plain("três")],
                ..ShownPick::default()
            }]
        );
    });
}

#[test]
fn quick_pick_with_object_items_keeps_separators_and_item_details() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        // 'pera' é o terceiro item da lista da extensão (o separador conta no índice): o resultado volta pelo `handle`.
        observed.quick_input.script_pick(PickScript::choose(&["pera"]));
        assert_eq!(workbench.execute("t.pickObjects", Vec::new()), Dynamic::string("pera"));

        let picks = observed.quick_input.shown_picks.borrow();
        assert_eq!(
            picks[0].items,
            vec![
                ShownItem::Separator("Frutas".to_owned()),
                ShownItem::Item { label: "maçã".to_owned(), description: Some("vermelha".to_owned()), detail: Some("uma fruta".to_owned()), picked: Some(true) },
                ShownItem::plain("pera"),
            ]
        );
    });
}

#[test]
fn quick_pick_accepts_text_and_object_items_in_the_same_list() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_pick(PickScript::choose(&["texto"]));
        assert_eq!(workbench.execute("t.pickMixed", Vec::new()), Dynamic::string("string:texto"));
        observed.quick_input.script_pick(PickScript::choose(&["objeto"]));
        assert_eq!(workbench.execute("t.pickMixed", Vec::new()), Dynamic::string("object:objeto"));

        let picks = observed.quick_input.shown_picks.borrow();
        assert_eq!(picks[1].items, vec![
            ShownItem::plain("texto"),
            ShownItem::Item { label: "objeto".to_owned(), description: Some("d".to_owned()), detail: None, picked: None },
        ]);
    });
}

#[test]
fn quick_pick_can_pick_many_resolves_the_list_of_chosen_items() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_pick(PickScript::choose(&["a", "c"]));
        assert_eq!(workbench.execute("t.pickMany", Vec::new()), Dynamic::string("a|c"));

        let picks = observed.quick_input.shown_picks.borrow();
        assert_eq!(picks[0].can_pick_many, Some(true));
        assert_eq!(picks[0].ignore_focus_lost, Some(true), "ignoreFocusOut chega como ignoreFocusLost");
        assert_eq!(picks[0].match_on_description, Some(true));
        assert_eq!(picks[0].match_on_detail, Some(true));
        assert_eq!(picks[0].items, vec![ShownItem::plain("a"), ShownItem::plain("b"), ShownItem::plain("c")]);
    });
}

#[test]
fn quick_pick_waits_for_a_promise_of_items() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_pick(PickScript::choose(&["y"]));
        assert_eq!(workbench.execute("t.pickPromise", Vec::new()), Dynamic::string("y"));
        assert_eq!(observed.quick_input.shown_picks.borrow()[0].items, vec![ShownItem::plain("x"), ShownItem::plain("y")]);
    });
}

#[test]
fn quick_pick_rejects_when_the_promise_of_items_rejects() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_pick(PickScript::choose(&["nunca"]));
        assert_eq!(workbench.execute("t.pickRejected", Vec::new()), Dynamic::string("erro:falhou ao listar"));
        assert!(observed.quick_input.shown_picks.borrow().is_empty(), "o seletor não chegou a receber itens");
    });
}

#[test]
fn quick_pick_rejects_an_item_that_is_neither_text_nor_object() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_pick(PickScript::choose(&["nunca"]));
        assert_eq!(workbench.execute("t.pickInvalid", Vec::new()), Dynamic::string("erro:showQuickPick: cada item precisa ser texto ou um objeto com label"));
    });
}

#[test]
fn quick_pick_reports_the_focused_items_to_on_did_select_item() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_pick(PickScript::choose(&["gama"]).with_focus(&["alfa", "gama"]));
        assert_eq!(workbench.execute("t.pickFocus", Vec::new()), Dynamic::string("alfa,gama|gama"));
    });
}

#[test]
fn quick_pick_closed_without_a_choice_resolves_undefined() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_pick(PickScript { focus: Vec::new(), outcome: PickOutcome::Close });
        assert_eq!(workbench.execute("t.pickClosed", Vec::new()), Dynamic::string("undefined"));
    });
}

#[test]
fn quick_pick_cancelled_by_the_token_resolves_undefined() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_pick(PickScript { focus: Vec::new(), outcome: PickOutcome::Wait });
        assert_eq!(workbench.execute("t.pickCancelled", Vec::new()), Dynamic::string("undefined"));
    });
}

#[test]
fn input_box_sends_every_option_and_resolves_the_typed_text() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_input(&[], InputOutcome::Accept("fulano de tal"));
        assert_eq!(workbench.execute("t.input", Vec::new()), Dynamic::string("fulano de tal"));

        assert_eq!(
            *observed.quick_input.shown_inputs.borrow(),
            vec![ShownInput {
                title: Some("Nome".to_owned()),
                prompt: Some("Digite o nome".to_owned()),
                value: Some("abc".to_owned()),
                value_selection: Some((1, 2)),
                place_holder: Some("fulano".to_owned()),
                password: Some(true),
                ignore_focus_lost: Some(true),
                validated: false,
            }]
        );
    });
}

#[test]
fn input_box_without_options_sends_none() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_input(&[], InputOutcome::Accept("x"));
        assert_eq!(workbench.execute("t.inputDefault", Vec::new()), Dynamic::string("x"));
        assert_eq!(
            *observed.quick_input.shown_inputs.borrow(),
            vec![ShownInput { title: None, prompt: None, value: None, value_selection: None, place_holder: None, password: None, ignore_focus_lost: None, validated: false }]
        );
    });
}

#[test]
fn input_box_validate_input_runs_in_the_extension_for_every_typed_text() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_input(&["ab", "abc", "info", "sem gravidade", "async", "boom"], InputOutcome::Accept("abc"));
        assert_eq!(workbench.execute("t.inputValidated", Vec::new()), Dynamic::string("abc"));
        workbench.pump();

        assert!(observed.quick_input.shown_inputs.borrow()[0].validated, "a função de validação viaja como o sinal `validateInput`");
        let validation = |typed: &str| observed.quick_input.validation_of(typed);
        assert_eq!(validation("ab"), "message:curto", "texto: a mensagem de erro");
        assert_eq!(validation("abc"), "none", "null: a entrada é válida");
        assert_eq!(validation("info"), "content:só informativo:Info", "objeto com gravidade");
        assert_eq!(validation("sem gravidade"), "content:mensagem solta:Error", "objeto sem gravidade com mensagem é erro");
        assert_eq!(validation("async"), "message:assíncrono", "promessa de texto");
        assert_eq!(validation("boom"), "error:explodiu", "a exceção da extensão rejeita a validação");
    });
}

#[test]
fn input_box_closed_without_text_resolves_undefined() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_input(&[], InputOutcome::Close);
        assert_eq!(workbench.execute("t.inputClosed", Vec::new()), Dynamic::string("undefined"));
    });
}

#[test]
fn input_box_cancelled_by_the_token_resolves_undefined() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        observed.quick_input.script_input(&[], InputOutcome::Wait);
        assert_eq!(workbench.execute("t.inputCancelled", Vec::new()), Dynamic::string("undefined"));
    });
}

#[test]
fn quick_input_enumerations_are_exposed_in_both_directions() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        assert_eq!(workbench.execute("t.enums", Vec::new()), Dynamic::string(r#"[-1,"Separator",0,3,"Warning","arrow-left"]"#));
    });
}
