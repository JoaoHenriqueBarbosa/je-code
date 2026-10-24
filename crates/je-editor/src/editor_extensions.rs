//! Porte de `src/vs/editor/browser/editorExtensions.ts`: as classes-base dos comandos e das ações do editor
//! (`Command`, `MultiCommand`, `ProxyCommand`, `EditorCommand`, `EditorAction`, `MultiEditorAction`,
//! `EditorAction2`), as funções de registro (`registerEditorCommand`, `registerEditorAction`,
//! `registerMultiEditorAction`, `registerInstantiatedEditorAction`, `registerEditorContribution`,
//! `registerDiffEditorContribution`, `registerModelAndPositionCommand`), o `EditorExtensionsRegistry` e os
//! comandos `UndoCommand`, `RedoCommand` e `SelectAllCommand`.
//!
//! O registro de comandos de plataforma é o `CommandsRegistry` de `je_platform::commands`, os atalhos vão para o
//! `KeybindingsRegistry` de `je_platform::keybinding::keybindings_registry`, os menus para o `MenuRegistry` de
//! `je_platform::actions` e as expressões de contexto são `ContextKeyExpression` de `je_platform::contextkey`.
//!
//! Representação:
//! - `abstract class Command` é a struct [`Command`] (os dados, mais `register`) e o trait [`ICommand`] (o
//!   `runCommand` abstrato). `ICommandOptions` é [`CommandOptions`]; `kbOpts` e `menuOpts` (um item ou uma lista)
//!   são sempre `Vec`; `kbExpr?: ContextKeyExpression | null` e `precondition: ContextKeyExpression | undefined`
//!   são `Option<ContextKeyExpression>`.
//! - O `handler` que o `CommandsRegistry` chama (`(accessor, ...args)`) recebe o primeiro argumento como
//!   `args: unknown`. O retorno `void | Promise<void>` de `runCommand` é [`Promise<()>`] (síncrono vira uma
//!   promessa já resolvida, como no trait [`IEditorCommand`] do widget); o `CommandHandler` do registro devolve
//!   `Dynamic`, então a promessa é descartada ao fim do handler e uma rejeição vai para `onUnexpectedError`.
//! - `boolean | Promise<void>` (o retorno de uma implementação de `MultiCommand`/`MultiEditorAction`) é
//!   [`CommandImplementationResult`]: `Bool(true)` e qualquer `Promise` são verdadeiros e param a busca.
//! - `EditorCommand` é o trait [`EditorCommand`] (`base()` mais o [`IEditorCommand`] do widget) e
//!   `EditorCommand.runCommand` é o handler montado por `register_editor_command` a partir de
//!   [`run_editor_command_in_focused_editor`] (o `EditorCommand.runEditorCommand` estático do original).
//!   `EditorCommand.bindToContribution` é [`bind_to_contribution`].
//! - `EditorAction` é o trait [`EditorAction`] (`action_base()` com o rótulo e o alias, mais `run`). O
//!   `runEditorCommand` que o original implementa na classe (`this.run(accessor, editor, args || {})`) é o
//!   `EditorActionCommand` interno, que o registro cria para cada ação.
//! - `EditorAction2` é o trait [`EditorAction2`] e o adaptador [`EditorAction2Adapter`] o liga ao `Action2` de
//!   `je_platform::actions` (`registerAction2(new X())` é `register_action2(Rc::new(EditorAction2Adapter(X)))`).
//! - `EditorContributionRegistry` (privado no original) e o namespace `EditorExtensionsRegistry` são um tipo só,
//!   [`EditorExtensionsRegistry`], com a instância por thread de [`EditorExtensionsRegistry::instance`]. Ele
//!   implementa o [`IEditorExtensionsRegistry`] que o widget consulta. As funções `registerEditorCommand` etc.
//!   acrescentam direto ao estado dele, sem métodos de repasse.
//! - `UndoCommand`, `RedoCommand` e `SelectAllCommand` são criados e registrados (junto com os proxies
//!   `default:undo` e `default:redo`) no primeiro acesso a [`undo_command`], [`redo_command`],
//!   [`select_all_command`] ou [`register_builtin_multi_commands`], que é o que o carregamento do módulo faz.
//!
//! Divergências:
//! - **Serviços.** O original lê `ICodeEditorService` (`getFocusedCodeEditor`/`getActiveCodeEditor`) do
//!   `accessor`. O serviço real ainda não foi portado (`opener_service::ICodeEditorService` é um recorte opaco
//!   sem os dois métodos tipados), então a fatia que os comandos de editor usam é [`ICodeEditorLookup`], com o
//!   id [`I_CODE_EDITOR_LOOKUP`]. O mesmo vale para `ITextModelService` ([`ITextModelService`], só
//!   `createModelReference`). `IModelService` é o recorte de `services_model`.
//! - **`invokeWithinContext` e `contextMatchesRules`.** O widget não tem `IContextKeyService` próprio, então a
//!   precondição de um comando é avaliada com o `IContextKeyService` do `accessor` que o registro entrega (só
//!   quando há precondição; sem ela o resultado é sempre verdadeiro, que é o que `contextMatchesRules(undefined)`
//!   devolve). A precondição de uma ação, usada por `isSupported` do widget, é avaliada pelo avaliador instalado
//!   com [`set_editor_context_matcher`]; sem avaliador instalado ela vale verdadeira.
//! - **Telemetria e log.** `ITelemetryService.publicLog2('editorActionInvoked')` de `EditorAction.reportTelemetry` e
//!   os `ILogService.trace/debug` de `MultiCommand.runCommand` e `EditorAction2.run` não têm porte e não têm
//!   efeito observável: foram omitidos. `getActiveElement()` (DOM) de `MultiCommand.runCommand` é o contexto sem
//!   alvo (`getContext(None)`).
//! - **Diff editor.** `IDiffEditor` e `IDiffEditorContribution` não existem no porte: o editor é opaco
//!   (`Rc<dyn Any>`), como o `CodeEditorHandle` de `opener_service`.
//! - **`registerModelAndPositionCommand`.** Um `Dynamic` não carrega `URI`: o recurso chega na forma de
//!   `toJSON` (objeto com `scheme`, `authority`, `path`, `query` e `fragment`), como no `OpenerService`. O
//!   `URI.isUri` de duck typing é a presença dessas partes como texto. O retorno do handler, no caminho
//!   assíncrono (modelo ainda não aberto), é descartado porque o `CommandHandler` devolve `Dynamic`. O
//!   `args.slice(2)` do caminho assíncrono do original (que passa a lista como um único argumento, sem o
//!   espalhamento do caminho síncrono) foi preservado.
//! - `Registry.add(Extensions.EditorCommonContributions, ...)` (o registro genérico de pontos de extensão) não
//!   existe: o registro é a instância por thread.
//! - `EditorContributionInstantiation` e `EditorContributionCtor` vivem em `code_editor_widget`, de onde são
//!   reexportados (`IEditorContributionDescription` é [`EditorContributionDescription`]).

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors;
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::types::assert_type;
use je_base::uri::{URI, UriComponents};
use je_instantiation::I_INSTANTIATION_SERVICE;
use je_platform::action::{CommandAction, Icon, LocalizedString, Title};
use je_platform::actions::{Action2, Action2Options, AnyMenuItem, MenuId, MenuItem, menu_ids, menu_registry};
use je_platform::commands::{Command as RegisteredCommand, CommandHandler, CommandMetadata, commands_registry};
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression, I_CONTEXT_KEY_SERVICE};
use je_platform::log::{I_LOG_SERVICE, LogService};
use je_platform::keybinding::keybindings_registry::{
    KeybindingRule, KeybindingWeight, Keybindings, PlatformKeybindings, keybindings_registry,
};

pub use crate::code_editor_widget::{
    EditorContributionCtor, EditorContributionDescription, EditorContributionInstantiation,
};
pub use je_instantiation::ServicesAccessor;

use crate::code_editor_widget::{
    CodeEditorWidget, EditorActionDescriptor, EditorPrecondition, IEditorCommand, IEditorContribution,
    IEditorExtensionsRegistry,
};
use crate::position::Position;
use crate::services_model::I_MODEL_SERVICE;
use crate::text_model::TextModel;
use crate::trigger_inline_edit_commands_registry::TriggerInlineEditCommandsRegistry;

// #region Serviços (recortes)

/// A fatia tipada de `ICodeEditorService` que os comandos de editor usam (ver "Divergências").
pub trait ICodeEditorLookup {
    /// `getFocusedCodeEditor()`: o editor com foco (no texto ou em um widget dele).
    fn get_focused_code_editor(&self) -> Option<Rc<CodeEditorWidget>>;
    /// `getActiveCodeEditor()`: o editor ativo.
    fn get_active_code_editor(&self) -> Option<Rc<CodeEditorWidget>>;
}

je_instantiation::service_id!(pub I_CODE_EDITOR_LOOKUP: dyn ICodeEditorLookup = "codeEditorLookup");

/// `IReference<IResolvedTextEditorModel>`: o modelo resolvido e o descarte da referência.
#[derive(Clone)]
pub struct ModelReference {
    /// `reference.object.textEditorModel`.
    pub object: Rc<TextModel>,
    dispose: DisposableRef,
}

impl ModelReference {
    pub fn new(object: Rc<TextModel>, dispose: DisposableRef) -> Self {
        Self { object, dispose }
    }

    /// `reference.dispose()`.
    pub fn dispose(&self) {
        self.dispose.dispose();
    }
}

/// `ITextModelService`, só com o `createModelReference` que `registerModelAndPositionCommand` usa.
pub trait ITextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<ModelReference>;
}

je_instantiation::service_id!(pub I_TEXT_MODEL_SERVICE: dyn ITextModelService = "textModelService");

// #endregion

// #region Contribuições do diff editor

/// `DiffEditorContributionCtor`: cria a contribuição para um diff editor (opaco, ver "Divergências").
pub type DiffEditorContributionCtor = Rc<dyn Fn(&Rc<dyn Any>) -> Rc<dyn IEditorContribution>>;

/// `IDiffEditorContributionDescription`.
#[derive(Clone)]
pub struct DiffEditorContributionDescription {
    pub id: String,
    pub ctor: DiffEditorContributionCtor,
}

// #endregion

// #region Command

/// `ICommandKeybindingsOptions`: o `IKeybindings` mais `kbExpr`, `weight` e `args`.
#[derive(Clone)]
pub struct CommandKeybindingsOptions {
    pub keybindings: Keybindings,
    pub kb_expr: Option<ContextKeyExpression>,
    pub weight: i64,
    /// Os argumentos padrão do atalho.
    pub args: Dynamic,
}

impl CommandKeybindingsOptions {
    pub fn new(weight: i64, kb_expr: Option<ContextKeyExpression>, keybindings: Keybindings) -> Self {
        Self { keybindings, kb_expr, weight, args: Dynamic::Undefined }
    }
}

/// `ICommandMenuOptions`.
#[derive(Clone)]
pub struct CommandMenuOptions {
    pub menu_id: MenuId,
    pub group: String,
    pub order: f64,
    pub when: Option<ContextKeyExpression>,
    pub title: String,
    pub icon: Option<ThemeIcon>,
}

impl CommandMenuOptions {
    /// Um item sem `when` e sem ícone.
    pub fn new(menu_id: MenuId, group: &str, order: f64, title: String) -> Self {
        Self { menu_id, group: group.to_string(), order, when: None, title, icon: None }
    }
}

/// `ICommandOptions`.
#[derive(Clone)]
pub struct CommandOptions {
    pub id: String,
    pub precondition: Option<ContextKeyExpression>,
    pub kb_opts: Vec<CommandKeybindingsOptions>,
    pub metadata: Option<CommandMetadata>,
    pub menu_opts: Vec<CommandMenuOptions>,
    pub can_trigger_inline_edits: bool,
}

impl CommandOptions {
    /// As opções de um comando só com o `id` (sem precondição, atalho, menu nem metadados).
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            precondition: None,
            kb_opts: Vec::new(),
            metadata: None,
            menu_opts: Vec::new(),
            can_trigger_inline_edits: false,
        }
    }
}

/// Embrulha o `runCommand` de um comando no `CommandHandler` do `CommandsRegistry`: o primeiro argumento é o
/// `args: unknown` e a promessa devolvida é descartada (uma rejeição vai para `onUnexpectedError`).
fn command_handler(run: impl Fn(&ServicesAccessor, &Dynamic) -> Promise<()> + 'static) -> CommandHandler {
    Rc::new(move |accessor, args| {
        let first = args.first().cloned().unwrap_or(Dynamic::Undefined);
        let _ = run(accessor, &first).catch(|error| errors::on_unexpected_error(&error));
        Dynamic::Undefined
    })
}

/// `abstract class Command`: os dados e o registro. O `runCommand` abstrato é o [`ICommand`].
pub struct Command {
    pub id: String,
    pub precondition: Option<ContextKeyExpression>,
    kb_opts: Vec<CommandKeybindingsOptions>,
    menu_opts: Vec<CommandMenuOptions>,
    pub metadata: Option<CommandMetadata>,
    pub can_trigger_inline_edits: bool,
}

impl Command {
    pub fn new(opts: CommandOptions) -> Self {
        Self {
            id: opts.id,
            precondition: opts.precondition,
            kb_opts: opts.kb_opts,
            menu_opts: opts.menu_opts,
            metadata: opts.metadata,
            can_trigger_inline_edits: opts.can_trigger_inline_edits,
        }
    }

    /// `register()`: menus, atalhos, o comando de plataforma (com o `handler` que chama o `runCommand`) e o
    /// registro dos comandos que disparam edições inline.
    pub fn register(&self, handler: CommandHandler) {
        for menu_opts in &self.menu_opts {
            self.register_menu_item(menu_opts);
        }

        for kb_opts in &self.kb_opts {
            let kb_when = match (&self.precondition, &kb_opts.kb_expr) {
                (Some(precondition), Some(kb_expr)) => {
                    ContextKeyExpr::and([Some(kb_expr.clone()), Some(precondition.clone())])
                }
                (Some(precondition), None) => Some(precondition.clone()),
                (None, kb_expr) => kb_expr.clone(),
            };

            keybindings_registry().register_keybinding_rule(KeybindingRule {
                keybindings: kb_opts.keybindings.clone(),
                id: self.id.clone(),
                weight: kb_opts.weight,
                args: kb_opts.args.clone(),
                when: kb_when,
            });
        }

        commands_registry().register_command(RegisteredCommand {
            id: self.id.clone(),
            handler,
            metadata: self.metadata.clone(),
        });

        if self.can_trigger_inline_edits {
            TriggerInlineEditCommandsRegistry::register_command(&self.id);
        }
    }

    /// `_registerMenuItem(item)`.
    fn register_menu_item(&self, item: &CommandMenuOptions) {
        let mut command = CommandAction::new(self.id.clone(), Title::Plain(item.title.clone()));
        command.icon = item.icon.clone().map(Icon::Theme);
        command.precondition = self.precondition.clone();

        let mut menu_item = MenuItem::new(command);
        menu_item.group = Some(item.group.clone());
        menu_item.when = item.when.clone();
        menu_item.order = Some(item.order);
        menu_registry().append_menu_item(&item.menu_id, AnyMenuItem::Menu(menu_item));
    }
}

/// O `runCommand` abstrato de `Command`.
pub trait ICommand {
    /// Os dados do comando.
    fn base(&self) -> &Command;

    /// `runCommand(accessor, args)`.
    fn run_command(&self, accessor: &ServicesAccessor, args: &Dynamic) -> Promise<()>;

    /// `register()`: registra o comando com um `handler` que chama [`ICommand::run_command`].
    fn register(self: &Rc<Self>)
    where
        Self: Sized + 'static,
    {
        let command = Rc::clone(self);
        self.base().register(command_handler(move |accessor, args| command.run_command(accessor, args)));
    }
}

/// `registerCommand(command)` (privada em `editorExtensions.ts` e repetida em `coreCommands.ts`): registra e
/// devolve o comando.
pub fn register_command<C: ICommand + 'static>(command: Rc<C>) -> Rc<C> {
    command.register();
    command
}

// #endregion

// #region MultiplexingCommand

/// O resultado de uma [`CommandImplementation`] (`boolean | Promise<void>`).
pub enum CommandImplementationResult {
    /// `true`: tratou e para a busca. `false`: não tratou, tenta a próxima.
    Bool(bool),
    /// Uma promessa é sempre verdadeira: tratou, e o comando devolve a promessa.
    Promise(Promise<()>),
}

/// `CommandImplementation`: uma implementação alternativa de um comando.
pub type CommandImplementation = Rc<dyn Fn(&ServicesAccessor, &Dynamic) -> CommandImplementationResult>;

/// `ICommandImplementationRegistration`.
#[derive(Clone)]
struct CommandImplementationRegistration {
    priority: i32,
    name: String,
    implementation: CommandImplementation,
    when: Option<ContextKeyExpression>,
}

/// `MultiCommand`: um comando com várias implementações, tentadas da maior prioridade para a menor.
pub struct MultiCommand {
    base: Command,
    implementations: Rc<RefCell<Vec<CommandImplementationRegistration>>>,
}

impl MultiCommand {
    pub fn new(opts: CommandOptions) -> Self {
        Self { base: Command::new(opts), implementations: Rc::new(RefCell::new(Vec::new())) }
    }

    /// `addImplementation(priority, name, implementation, when?)`: uma prioridade maior é olhada primeiro.
    pub fn add_implementation(
        &self,
        priority: i32,
        name: &str,
        implementation: CommandImplementation,
        when: Option<ContextKeyExpression>,
    ) -> DisposableRef {
        let added = Rc::clone(&implementation);
        {
            let mut implementations = self.implementations.borrow_mut();
            implementations.push(CommandImplementationRegistration { priority, name: name.to_string(), implementation, when });
            // A ordenação é estável, como a de `Array.prototype.sort`.
            implementations.sort_by(|a, b| b.priority.cmp(&a.priority));
        }
        let implementations = Rc::clone(&self.implementations);
        to_disposable(move || {
            let mut implementations = implementations.borrow_mut();
            if let Some(index) = implementations
                .iter()
                .position(|registration| std::ptr::addr_eq(Rc::as_ptr(&registration.implementation), Rc::as_ptr(&added)))
            {
                implementations.remove(index);
            }
        })
    }
}

impl ICommand for MultiCommand {
    fn base(&self) -> &Command {
        &self.base
    }

    fn run_command(&self, accessor: &ServicesAccessor, args: &Dynamic) -> Promise<()> {
        // O `for...of` do original vê a lista viva; a cópia evita o empréstimo durante a chamada.
        let implementations: Vec<CommandImplementationRegistration> = self.implementations.borrow().clone();
        let log_service = accessor.get(I_LOG_SERVICE);
        log_service.trace(&format!("Executing Command '{}' which has {} bound.", self.base.id, implementations.len()));
        for registration in implementations {
            if let Some(when) = &registration.when {
                let context = accessor.get(I_CONTEXT_KEY_SERVICE).get_context(None);
                if !when.evaluate(&*context) {
                    continue;
                }
            }
            let handled = |log_service: &dyn LogService| {
                log_service.trace(&format!("Command '{}' was handled by '{}'.", self.base.id, registration.name));
            };
            match (registration.implementation)(accessor, args) {
                CommandImplementationResult::Bool(true) => {
                    handled(&*log_service);
                    return Promise::resolved(());
                }
                CommandImplementationResult::Bool(false) => {}
                CommandImplementationResult::Promise(promise) => {
                    handled(&*log_service);
                    return promise;
                }
            }
        }
        log_service.trace(&format!("The Command '{}' was not handled by any implementation.", self.base.id));
        Promise::resolved(())
    }
}

// #endregion

/// `ProxyCommand`: um comando que delega à implementação de outro, para registrar ids diferentes com a mesma
/// implementação.
pub struct ProxyCommand {
    base: Command,
    command: Rc<dyn ICommand>,
}

impl ProxyCommand {
    pub fn new(command: Rc<dyn ICommand>, opts: CommandOptions) -> Self {
        Self { base: Command::new(opts), command }
    }
}

impl ICommand for ProxyCommand {
    fn base(&self) -> &Command {
        &self.base
    }

    fn run_command(&self, accessor: &ServicesAccessor, args: &Dynamic) -> Promise<()> {
        self.command.run_command(accessor, args)
    }
}

// #region EditorCommand

/// `contextMatchesRules(rules ?? undefined)` do `IContextKeyService` do `accessor`.
fn context_matches_rules(accessor: &ServicesAccessor, rules: Option<&ContextKeyExpression>) -> bool {
    rules.is_none() || accessor.get(I_CONTEXT_KEY_SERVICE).context_matches_rules(rules)
}

/// `EditorCommand.runEditorCommand(accessor, args, precondition, runner)` (o método estático): acha o editor com
/// foco ou ativo e, se a precondição vale, roda o `runner` nele.
pub fn run_editor_command_in_focused_editor(
    accessor: &ServicesAccessor,
    args: &Dynamic,
    precondition: Option<&ContextKeyExpression>,
    runner: &dyn Fn(&ServicesAccessor, &Rc<CodeEditorWidget>, &Dynamic) -> Promise<()>,
) -> Promise<()> {
    let code_editor_service = accessor.get(I_CODE_EDITOR_LOOKUP);

    // Find the editor with text focus or active
    let Some(editor) = code_editor_service.get_focused_code_editor().or_else(|| code_editor_service.get_active_code_editor())
    else {
        // well, at least we tried...
        return Promise::resolved(());
    };

    if !context_matches_rules(accessor, precondition) {
        // precondition does not hold
        return Promise::resolved(());
    }

    runner(accessor, &editor, args)
}

/// O `handler` do `CommandsRegistry` de um comando de editor: `EditorCommand.runCommand`.
fn editor_command_handler(command: Rc<dyn IEditorCommand>, precondition: Option<ContextKeyExpression>) -> CommandHandler {
    command_handler(move |accessor, args| {
        run_editor_command_in_focused_editor(accessor, args, precondition.as_ref(), &|accessor, editor, args| {
            command.run_editor_command(Some(accessor), editor, args)
        })
    })
}

/// `abstract class EditorCommand`: os dados mais o `runEditorCommand` do [`IEditorCommand`].
pub trait EditorCommand: IEditorCommand {
    /// Os dados do comando.
    fn base(&self) -> &Command;
}

/// `IContributionCommandOptions<T>`: as opções mais o `handler`.
pub struct ContributionCommandOptions<T: 'static> {
    pub command: CommandOptions,
    pub handler: Rc<dyn Fn(&T, &Dynamic)>,
}

/// O `controllerGetter` de `bindToContribution`.
pub type ControllerGetter<T> = Rc<dyn Fn(&Rc<CodeEditorWidget>) -> Option<Rc<T>>>;

/// O comando que `EditorCommand.bindToContribution` cria: roda o `handler` no controlador do editor.
pub struct EditorControllerCommand<T: 'static> {
    base: Command,
    callback: Rc<dyn Fn(&T, &Dynamic)>,
    controller_getter: ControllerGetter<T>,
}

impl<T: 'static> IEditorCommand for EditorControllerCommand<T> {
    fn run_editor_command(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        if let Some(controller) = (self.controller_getter)(editor) {
            (self.callback)(&controller, args);
        }
        Promise::resolved(())
    }
}

impl<T: 'static> EditorCommand for EditorControllerCommand<T> {
    fn base(&self) -> &Command {
        &self.base
    }
}

/// `EditorCommand.bindToContribution(controllerGetter)`: devolve o construtor dos comandos ligados a uma
/// contribuição.
pub fn bind_to_contribution<T: 'static>(
    controller_getter: ControllerGetter<T>,
) -> impl Fn(ContributionCommandOptions<T>) -> EditorControllerCommand<T> {
    move |opts| EditorControllerCommand {
        base: Command::new(opts.command),
        callback: opts.handler,
        controller_getter: Rc::clone(&controller_getter),
    }
}

// #endregion EditorCommand

// #region EditorAction

/// `IEditorActionContextMenuOptions`.
#[derive(Clone)]
pub struct EditorActionContextMenuOptions {
    pub group: String,
    pub order: f64,
    pub when: Option<ContextKeyExpression>,
    pub menu_id: Option<MenuId>,
}

/// O rótulo de `IActionOptions`: `{ label: ILocalizedString; alias?: string }` ou `{ label: string; alias: string }`.
#[derive(Clone)]
pub enum ActionLabel {
    Localized { label: LocalizedString, alias: Option<String> },
    Plain { label: String, alias: String },
}

/// `IActionOptions`: as opções do comando, o menu de contexto e o rótulo.
pub struct ActionOptions {
    pub command: CommandOptions,
    pub context_menu_opts: Vec<EditorActionContextMenuOptions>,
    pub label: ActionLabel,
}

/// Os dados de `abstract class EditorAction`: o comando, o rótulo e o alias.
pub struct EditorActionBase {
    pub command: Command,
    pub label: String,
    pub alias: String,
}

impl EditorActionBase {
    /// O construtor de `EditorAction`, com o `convertOptions`: cada item de `contextMenuOpts` vira um item de
    /// menu (menu `EditorContext` por padrão, título igual ao rótulo, `when` somado à precondição).
    pub fn new(opts: ActionOptions) -> Self {
        let ActionOptions { mut command, context_menu_opts, label } = opts;

        let (label_text, alias) = match label {
            ActionLabel::Plain { label, alias } => (label, alias),
            ActionLabel::Localized { label, alias } => {
                let alias = alias.unwrap_or(label.original);
                (label.value, alias)
            }
        };

        for item in context_menu_opts {
            command.menu_opts.push(CommandMenuOptions {
                menu_id: item.menu_id.unwrap_or_else(menu_ids::editor_context),
                group: item.group,
                order: item.order,
                when: ContextKeyExpr::and([command.precondition.clone(), item.when]),
                title: label_text.clone(),
                icon: None,
            });
        }

        Self { command: Command::new(command), label: label_text, alias }
    }
}

/// `abstract class EditorAction`: o `run` abstrato.
pub trait EditorAction {
    /// Os dados da ação.
    fn action_base(&self) -> &EditorActionBase;

    /// `run(accessor, editor, args)`: o `args` já vale `args || {}`.
    fn run(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()>;
}

/// O `runEditorCommand` de `EditorAction`: `this.run(accessor, editor, args || {})`.
struct EditorActionCommand {
    action: Rc<dyn EditorAction>,
}

impl IEditorCommand for EditorActionCommand {
    fn run_editor_command(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        if args.is_truthy() {
            self.action.run(accessor, editor, args)
        } else {
            self.action.run(accessor, editor, &Dynamic::object(std::iter::empty::<(&str, Dynamic)>()))
        }
    }
}

/// `EditorActionImplementation`: uma implementação alternativa de uma ação.
pub type EditorActionImplementation =
    Rc<dyn Fn(Option<&ServicesAccessor>, &Rc<CodeEditorWidget>, &Dynamic) -> CommandImplementationResult>;

/// `MultiEditorAction`: uma ação com várias implementações, tentadas da maior prioridade para a menor.
pub struct MultiEditorAction {
    action: EditorActionBase,
    implementations: Rc<RefCell<Vec<(i32, EditorActionImplementation)>>>,
}

impl MultiEditorAction {
    pub fn new(opts: ActionOptions) -> Self {
        Self { action: EditorActionBase::new(opts), implementations: Rc::new(RefCell::new(Vec::new())) }
    }

    /// `addImplementation(priority, implementation)`: uma prioridade maior é olhada primeiro.
    pub fn add_implementation(&self, priority: i32, implementation: EditorActionImplementation) -> DisposableRef {
        let added = Rc::clone(&implementation);
        {
            let mut implementations = self.implementations.borrow_mut();
            implementations.push((priority, implementation));
            implementations.sort_by(|a, b| b.0.cmp(&a.0));
        }
        let implementations = Rc::clone(&self.implementations);
        to_disposable(move || {
            let mut implementations = implementations.borrow_mut();
            if let Some(index) =
                implementations.iter().position(|(_, implementation)| std::ptr::addr_eq(Rc::as_ptr(implementation), Rc::as_ptr(&added)))
            {
                implementations.remove(index);
            }
        })
    }
}

impl EditorAction for MultiEditorAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        let implementations: Vec<(i32, EditorActionImplementation)> = self.implementations.borrow().clone();
        for (_, implementation) in implementations {
            match implementation(accessor, editor, args) {
                CommandImplementationResult::Bool(true) => return Promise::resolved(()),
                CommandImplementationResult::Bool(false) => {}
                CommandImplementationResult::Promise(promise) => return promise,
            }
        }
        Promise::resolved(())
    }
}

// #endregion EditorAction

// #region EditorAction2

/// `abstract class EditorAction2`: o `runEditorCommand` abstrato (o `desc` é o do `Action2`).
pub trait EditorAction2 {
    /// `desc`.
    fn desc(&self) -> Action2Options;

    /// `runEditorCommand(accessor, editor, ...args)`.
    fn run_editor_command(&self, accessor: &ServicesAccessor, editor: &Rc<CodeEditorWidget>, args: &[Dynamic]);
}

/// Liga um [`EditorAction2`] ao `Action2` do registro de ações, com o `run` de `EditorAction2`.
pub struct EditorAction2Adapter<A: EditorAction2>(pub A);

impl<A: EditorAction2> Action2 for EditorAction2Adapter<A> {
    fn desc(&self) -> Action2Options {
        self.0.desc()
    }

    fn run(&self, accessor: &ServicesAccessor, args: &[Dynamic]) {
        // Find the editor with text focus or active
        let code_editor_service = accessor.get(I_CODE_EDITOR_LOOKUP);
        let Some(editor) =
            code_editor_service.get_focused_code_editor().or_else(|| code_editor_service.get_active_code_editor())
        else {
            // well, at least we tried...
            return;
        };

        // precondition does hold
        let precondition = self.0.desc().command.precondition;
        if !context_matches_rules(accessor, precondition.as_ref()) {
            return;
        }
        self.0.run_editor_command(accessor, &editor, args);
    }
}

// #endregion

// #region Registro de comandos, ações e contribuições

/// `Position.isIPosition(obj)` e `Position.lift(obj)`: um objeto com `lineNumber` e `column` numéricos.
pub fn position_from_dynamic(value: &Dynamic) -> Option<Position> {
    if value.is_nullish() {
        return None;
    }
    match (value.get_str("lineNumber"), value.get_str("column")) {
        (Dynamic::Number(line_number), Dynamic::Number(column)) => Some(Position::new(line_number as i32, column as i32)),
        _ => None,
    }
}

/// `URI.isUri(thing)` e a leitura do recurso (ver "Divergências"): as partes de `toJSON` como texto.
fn uri_from_dynamic(value: &Dynamic) -> Option<URI> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    // Uma parte ausente (o `toJSON` omite as vazias) vale texto vazio; outro tipo não é um URI.
    let part = |key: &str| -> Option<Option<JsString>> {
        match value.get_str(key) {
            Dynamic::Undefined => Some(None),
            Dynamic::String(text) => Some(Some(text)),
            _ => None,
        }
    };
    let components = UriComponents {
        scheme: part("scheme")?,
        authority: part("authority")?,
        path: part("path")?,
        query: part("query")?,
        fragment: part("fragment")?,
    };
    Some(URI::from(&components, false))
}

/// Descarta a referência ao sair do escopo, mesmo se o handler lançar (o `finally` do original).
struct DisposeOnDrop(ModelReference);

impl Drop for DisposeOnDrop {
    fn drop(&mut self) {
        self.0.dispose();
    }
}

/// O handler de `registerModelAndPositionCommand`: `(accessor, model, position, ...args)`.
pub type ModelAndPositionHandler = Rc<dyn Fn(&ServicesAccessor, &Rc<TextModel>, Position, &[Dynamic]) -> Dynamic>;

/// `registerModelAndPositionCommand(id, handler)`.
pub fn register_model_and_position_command(id: &str, handler: ModelAndPositionHandler) {
    let command_handler: CommandHandler = Rc::new(move |accessor, args| {
        let insta_service = accessor.get(I_INSTANTIATION_SERVICE);

        let arg = |index: usize| args.get(index).cloned().unwrap_or(Dynamic::Undefined);
        let resource = uri_from_dynamic(&arg(0));
        assert_type(&resource.is_some(), None);
        let position = position_from_dynamic(&arg(1));
        assert_type(&position.is_some(), None);
        let (Some(resource), Some(position)) = (resource, position) else {
            return Dynamic::Undefined;
        };
        let rest: Vec<Dynamic> = args.iter().skip(2).cloned().collect();

        if let Some(model) = accessor.get(I_MODEL_SERVICE).get_model(&resource) {
            return insta_service.invoke_function(|accessor| handler(accessor, &model, position, &rest));
        }

        let handler = Rc::clone(&handler);
        let _ = accessor
            .get(I_TEXT_MODEL_SERVICE)
            .create_model_reference(&resource)
            .then(move |reference| {
                let reference = DisposeOnDrop(reference);
                let rest_as_one_argument = [Dynamic::array(rest)];
                insta_service.invoke_function(|accessor| {
                    handler(accessor, &reference.0.object, position, &rest_as_one_argument)
                })
            })
            .catch(|error| {
                errors::on_unexpected_error(&error);
                Dynamic::Undefined
            });
        Dynamic::Undefined
    });
    commands_registry().register_command(RegisteredCommand { id: id.to_string(), handler: command_handler, metadata: None });
}

/// `registerEditorCommand(editorCommand)`: registra o comando (menus, atalhos, comando de plataforma) e o guarda
/// pelo id para o `getEditorCommand`.
pub fn register_editor_command<C: EditorCommand + 'static>(editor_command: Rc<C>) -> Rc<C> {
    let command: Rc<dyn IEditorCommand> = editor_command.clone();
    let base = editor_command.base();
    base.register(editor_command_handler(Rc::clone(&command), base.precondition.clone()));
    EditorExtensionsRegistry::instance().editor_commands.borrow_mut().insert(base.id.clone(), command);
    editor_command
}

/// `registerInstantiatedEditorAction(editorAction)`: registra a ação como comando e a guarda para o
/// `getEditorActions`.
pub fn register_instantiated_editor_action(editor_action: Rc<dyn EditorAction>) {
    let base = editor_action.action_base();
    let command: Rc<dyn IEditorCommand> = Rc::new(EditorActionCommand { action: Rc::clone(&editor_action) });
    base.command.register(editor_command_handler(Rc::clone(&command), base.command.precondition.clone()));
    EditorExtensionsRegistry::instance().editor_actions.borrow_mut().push(EditorActionDescriptor {
        id: base.command.id.clone(),
        label: base.label.clone(),
        alias: base.alias.clone(),
        metadata: base.command.metadata.clone(),
        precondition: editor_precondition(base.command.precondition.clone()),
        command,
    });
}

/// `registerEditorAction(ctor)`: cria a ação (`new ctor()`) e a registra.
pub fn register_editor_action<A: EditorAction + 'static>(ctor: impl FnOnce() -> A) -> Rc<A> {
    let action = Rc::new(ctor());
    register_instantiated_editor_action(action.clone());
    action
}

/// `registerMultiEditorAction(action)`.
pub fn register_multi_editor_action(action: Rc<MultiEditorAction>) -> Rc<MultiEditorAction> {
    register_instantiated_editor_action(action.clone());
    action
}

/// `registerEditorContribution(id, ctor, instantiation)`: contribuições têm o ciclo de vida preso a um editor.
pub fn register_editor_contribution(id: &str, ctor: EditorContributionCtor, instantiation: EditorContributionInstantiation) {
    EditorExtensionsRegistry::instance()
        .editor_contributions
        .borrow_mut()
        .push(EditorContributionDescription { id: id.to_string(), ctor, instantiation });
}

/// `registerDiffEditorContribution(id, ctor)`.
pub fn register_diff_editor_contribution(id: &str, ctor: DiffEditorContributionCtor) {
    EditorExtensionsRegistry::instance()
        .diff_editor_contributions
        .borrow_mut()
        .push(DiffEditorContributionDescription { id: id.to_string(), ctor });
}

/// O avaliador das precondições de ação (o `contextMatchesRules` do `IContextKeyService` do editor).
pub type EditorContextMatcher = Rc<dyn Fn(&CodeEditorWidget, &ContextKeyExpression) -> bool>;

thread_local! {
    static EDITOR_CONTEXT_MATCHER: RefCell<Option<EditorContextMatcher>> = const { RefCell::new(None) };
}

/// Instala (ou remove, com `None`) o avaliador das precondições de ação (ver "Divergências").
pub fn set_editor_context_matcher(matcher: Option<EditorContextMatcher>) {
    EDITOR_CONTEXT_MATCHER.with(|slot| *slot.borrow_mut() = matcher);
}

/// A precondição de uma ação no formato do widget: sem expressão a ação é sempre suportada.
fn editor_precondition(precondition: Option<ContextKeyExpression>) -> Option<EditorPrecondition> {
    precondition.map(|expression| -> EditorPrecondition {
        Rc::new(move |editor: &CodeEditorWidget| {
            let matcher = EDITOR_CONTEXT_MATCHER.with(|slot| slot.borrow().clone());
            matcher.is_none_or(|matcher| matcher(editor, &expression))
        })
    })
}

/// `EditorContributionRegistry` (e o namespace `EditorExtensionsRegistry`): as contribuições, as ações e os
/// comandos de editor registrados.
pub struct EditorExtensionsRegistry {
    pub(crate) editor_contributions: RefCell<Vec<EditorContributionDescription>>,
    pub(crate) diff_editor_contributions: RefCell<Vec<DiffEditorContributionDescription>>,
    pub(crate) editor_actions: RefCell<Vec<EditorActionDescriptor>>,
    pub(crate) editor_commands: RefCell<HashMap<String, Rc<dyn IEditorCommand>>>,
}

thread_local! {
    /// `EditorContributionRegistry.INSTANCE`.
    static EDITOR_EXTENSIONS_REGISTRY: Rc<EditorExtensionsRegistry> = Rc::new(EditorExtensionsRegistry {
        editor_contributions: RefCell::new(Vec::new()),
        diff_editor_contributions: RefCell::new(Vec::new()),
        editor_actions: RefCell::new(Vec::new()),
        editor_commands: RefCell::new(HashMap::new()),
    });
}

impl EditorExtensionsRegistry {
    /// A instância por thread.
    pub fn instance() -> Rc<EditorExtensionsRegistry> {
        EDITOR_EXTENSIONS_REGISTRY.with(Rc::clone)
    }

    /// `getSomeEditorContributions(ids)`.
    pub fn get_some_editor_contributions(&self, ids: &[&str]) -> Vec<EditorContributionDescription> {
        self.get_editor_contributions().into_iter().filter(|contribution| ids.contains(&contribution.id.as_str())).collect()
    }

    /// `getDiffEditorContributions()`.
    pub fn get_diff_editor_contributions(&self) -> Vec<DiffEditorContributionDescription> {
        self.diff_editor_contributions.borrow().clone()
    }
}

impl IEditorExtensionsRegistry for EditorExtensionsRegistry {
    fn get_editor_contributions(&self) -> Vec<EditorContributionDescription> {
        self.editor_contributions.borrow().clone()
    }

    fn get_editor_actions(&self) -> Vec<EditorActionDescriptor> {
        self.editor_actions.borrow().clone()
    }

    fn get_editor_command(&self, command_id: &str) -> Option<Rc<dyn IEditorCommand>> {
        self.editor_commands.borrow().get(command_id).cloned()
    }
}

// #endregion

// #region Undo, Redo e SelectAll

/// `UndoCommand`, `RedoCommand` e `SelectAllCommand`.
struct BuiltinMultiCommands {
    undo: Rc<MultiCommand>,
    redo: Rc<MultiCommand>,
    select_all: Rc<MultiCommand>,
}

thread_local! {
    static BUILTIN_MULTI_COMMANDS: BuiltinMultiCommands = create_builtin_multi_commands();
}

fn create_builtin_multi_commands() -> BuiltinMultiCommands {
    let ctrl_cmd = KeyMod::CTRL_CMD;
    let shift = KeyMod::SHIFT;

    let undo = register_command(Rc::new(MultiCommand::new(CommandOptions {
        kb_opts: vec![CommandKeybindingsOptions::new(
            KeybindingWeight::EDITOR_CORE,
            None,
            Keybindings { primary: Some(ctrl_cmd | KeyCode::KeyZ.bits()), ..Keybindings::default() },
        )],
        menu_opts: vec![
            CommandMenuOptions::new(menu_ids::menubar_edit_menu(), "1_do", 1.0, localize("miUndo", "&&Undo", &[])),
            CommandMenuOptions::new(menu_ids::command_palette(), "", 1.0, localize("undo", "Undo", &[])),
            CommandMenuOptions::new(menu_ids::simple_editor_context(), "1_do", 1.0, localize("undo", "Undo", &[])),
        ],
        ..CommandOptions::new("undo")
    })));

    register_command(Rc::new(ProxyCommand::new(undo.clone(), CommandOptions::new("default:undo"))));

    let redo = register_command(Rc::new(MultiCommand::new(CommandOptions {
        kb_opts: vec![CommandKeybindingsOptions::new(
            KeybindingWeight::EDITOR_CORE,
            None,
            Keybindings {
                primary: Some(ctrl_cmd | KeyCode::KeyY.bits()),
                secondary: Some(vec![ctrl_cmd | shift | KeyCode::KeyZ.bits()]),
                mac: Some(PlatformKeybindings { primary: Some(ctrl_cmd | shift | KeyCode::KeyZ.bits()), secondary: None }),
                ..Keybindings::default()
            },
        )],
        menu_opts: vec![
            CommandMenuOptions::new(menu_ids::menubar_edit_menu(), "1_do", 2.0, localize("miRedo", "&&Redo", &[])),
            CommandMenuOptions::new(menu_ids::command_palette(), "", 1.0, localize("redo", "Redo", &[])),
            CommandMenuOptions::new(menu_ids::simple_editor_context(), "1_do", 2.0, localize("redo", "Redo", &[])),
        ],
        ..CommandOptions::new("redo")
    })));

    register_command(Rc::new(ProxyCommand::new(redo.clone(), CommandOptions::new("default:redo"))));

    let select_all = register_command(Rc::new(MultiCommand::new(CommandOptions {
        kb_opts: vec![CommandKeybindingsOptions::new(
            KeybindingWeight::EDITOR_CORE,
            None,
            Keybindings { primary: Some(ctrl_cmd | KeyCode::KeyA.bits()), ..Keybindings::default() },
        )],
        menu_opts: vec![
            CommandMenuOptions::new(menu_ids::menubar_selection_menu(), "1_basic", 1.0, localize("miSelectAll", "&&Select All", &[])),
            CommandMenuOptions::new(menu_ids::command_palette(), "", 1.0, localize("selectAll", "Select All", &[])),
            CommandMenuOptions::new(menu_ids::simple_editor_context(), "9_select", 1.0, localize("selectAll", "Select All", &[])),
        ],
        ..CommandOptions::new("editor.action.selectAll")
    })));

    BuiltinMultiCommands { undo, redo, select_all }
}

/// `UndoCommand`.
pub fn undo_command() -> Rc<MultiCommand> {
    BUILTIN_MULTI_COMMANDS.with(|commands| Rc::clone(&commands.undo))
}

/// `RedoCommand`.
pub fn redo_command() -> Rc<MultiCommand> {
    BUILTIN_MULTI_COMMANDS.with(|commands| Rc::clone(&commands.redo))
}

/// `SelectAllCommand`.
pub fn select_all_command() -> Rc<MultiCommand> {
    BUILTIN_MULTI_COMMANDS.with(|commands| Rc::clone(&commands.select_all))
}

/// Registra `undo`, `default:undo`, `redo`, `default:redo` e `editor.action.selectAll` (o que o carregamento do
/// módulo faz no original). É idempotente: o registro acontece uma vez por thread.
pub fn register_builtin_multi_commands() {
    BUILTIN_MULTI_COMMANDS.with(|_| {});
}

// #endregion
