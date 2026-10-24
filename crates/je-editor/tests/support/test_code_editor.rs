//! Porte de `src/vs/editor/test/browser/testCodeEditor.ts`: o editor de teste (`TestCodeEditor`) e as funções que o
//! criam (`withTestCodeEditor`, `withAsyncTestCodeEditor`, `createTestCodeEditor`, `instantiateTestCodeEditor`,
//! `createCodeEditorServices`).
//!
//! Representação:
//! - `TestCodeEditor extends CodeEditorWidget` é a composição [`TestCodeEditor`] com `Deref` para o
//!   [`CodeEditorWidget`] (o `Rc` do widget é o [`TestCodeEditor::widget`]), então toda a API do `ICodeEditor`
//!   (`get_model`, `set_model`, `execute_edits`, `get_view_model`...) chama direto no editor de teste, com o trait
//!   [`ICodeEditor`] no escopo. `hasTextFocus`/`setHasTextFocus` são os métodos próprios, que valem para quem chama
//!   o editor de teste (um `&dyn ICodeEditor` vê o foco do widget). `ITestCodeEditor extends IActiveCodeEditor` é
//!   o [`ITestCodeEditor`], um sinônimo: o `IActiveCodeEditor` só estreita os tipos de `getModel` e afins para o
//!   caso com modelo, o que o `Option` do porte não distingue.
//! - `_createView` do original devolve `[null, false]` (nunca cria `View`; `hasRealView` falso) só para o widget não
//!   precisar de DOM. O [`CodeEditorWidget`] do porte não tem `View`, então o override não existe. O mesmo vale para o
//!   `TestEditorDomElement` (o contêiner DOM falso que o construtor recebe).
//! - `_createConfiguration` devolve o `TestConfiguration(options)`: [`instantiate_test_code_editor`] cria a
//!   configuração com [`TestConfiguration::new`] e a entrega pronta ao widget.
//! - As opções do editor (`TestCodeEditorCreationOptions extends IEditorOptions`, com `hasTextFocus` e `envConfig`)
//!   são o objeto [`Dynamic`] que a `EditorConfiguration` consome: [`TestCodeEditorCreationOptions`]. O
//!   `ITestEnvConfiguration` é o objeto sob `envConfig` (ver `test_configuration.rs`). As de instanciação
//!   (`serviceCollection` à parte do objeto, que o original apaga com `delete options.serviceCollection`) são o
//!   [`TestCodeEditorInstantiationOptions`].
//! - O argumento `text: ITextModel | string | string[] | ITextBufferFactory` é o [`TestEditorInput`], com `From`
//!   das quatro formas. O modelo criado pelo harness é descartado com ele; o recebido não.
//! - `createInstance(ctor, this)` de `registerAndInstantiateContribution` é o `ctor` recebido: um fecho que cria a
//!   contribuição com o editor e o serviço de instanciação. `runCommand`/`runAction` chamam o `command`/`action` dentro
//!   de `invokeFunction`, e o `void | Promise<void>` é o [`MaybePromise`].
//! - `withTestCodeEditor` e `withAsyncTestCodeEditor` partilham o preparo ([`TestCodeEditorFixture`]), o que
//!   `_withTestCodeEditor` faz com a sobrecarga: o síncrono descarta ao fim do callback e o assíncrono ao resolver
//!   a promessa que o callback devolve (como o `result.then(() => disposables.dispose())`, uma rejeição não descarta).
//! - Os callbacks recebem `(editor, viewModel, instantiationService)`: o terceiro é o [`CodeEditorServices`], que
//!   tem `Deref` para o `TestInstantiationService`.
//!
//! Divergências:
//! - `createCodeEditorServices` registra na coleção só os serviços que o porte tem com identificador:
//!   `INotificationService` (`TestNotificationService`), `ICommandService` (`TestCommandService`) e
//!   `ILanguageService` (`LanguageService`). `ICodeEditorService` (o `TestCodeEditorService` de
//!   `test_code_editor_service.rs`), `ILanguageConfigurationService` (`TestLanguageConfigurationService`),
//!   `IThemeService` (`TestThemeService`) e o `UndoRedoService` não têm identificador no porte: são campos do
//!   [`CodeEditorServices`]. Ficam de fora, por não ter porte: `IAccessibilityService`, `IKeybindingService`
//!   (`MockKeybindingService`), `IClipboardService`, `IEditorWorkerService`, `IOpenerService` (o `OpenerService` está
//!   portado, mas depende do `ICodeEditorService`), `IHoverService`, `IDialogService`, `IUndoRedoService`,
//!   `IConfigurationService` (o `TestConfigurationService` único é o de `je-platform`; o editor lê as opções da
//!   `TestConfiguration`), `ITextResourcePropertiesService`, `ILogService`, `IModelService`, `IContextKeyService`,
//!   `ITelemetryService`, `ILoggerService`, `IDataChannelService`, `IEnvironmentService`,
//!   `ILanguageFeatureDebounceService`, `ILanguageFeaturesService`, `ITreeSitterLibraryService`,
//!   `IInlineCompletionsService`, `IUserInteractionService` e `ITextModelService`.
//! - O descarte dos serviços do original percorre os identificadores e chama `dispose` dos que têm. Aqui cada serviço
//!   que o porte cria e que é descartável entra no `DisposableStore`; um serviço que o chamador injeta pela
//!   `serviceCollection` é do chamador (um `Rc<dyn ...>` do porte não expõe `dispose`).
//! - O widget lê `INotificationService` e `ICommandService` da coleção (o `@INotificationService` do construtor), então um
//!   serviço injetado pelo chamador vale para o editor.
//! - O `EditorExtensionsRegistry` global está vazio ([`NoExtensionsRegistry`]): o editor de teste nasce sem
//!   ações nem contribuições (`contributions: []` já é o padrão do original; as ações do registro global dependem
//!   de módulos de `contrib` que o teste importaria).
//! - O medidor de quebra de linha do DOM (`DOMLineBreaksComputerFactory`) é o monoespaçado: sem DOM, a quebra
//!   `advanced` cai no mesmo cálculo da `simple`. `scheduleAtNextAnimationFrame` é um `setTimeout` de 16 ms e
//!   `runWhenWindowIdle` o `runWhenGlobalIdle` do porte; os testes síncronos nunca os veem rodar.
//! - O modelo criado pelo harness usa só o `UndoRedoService` (`instantiateTextModel` do original também injeta o
//!   `ILanguageService` e o `ILanguageConfigurationService`): a linguagem é a `plaintext`.
//! - `TestCodeEditor.dispose` descarta o widget e depois o que foi pelo `registerDisposable`, a ordem do original
//!   (o `dispose` do `CodeEditorWidget` vem antes do `Disposable.dispose` que descarta o registrado).

use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{IdleDeadline, RunWhenIdle, run_when_global_idle};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, plain_disposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::scrollable::ScheduleAtNextAnimationFrame;
use je_editor::code_editor_widget::{
    CodeEditorWidget, CodeEditorWidgetOptions, CodeEditorWidgetServices, EditorActionDescriptor,
    EditorContributionDescription, EditorViewModel, ICodeEditor, IEditorCommand, IEditorContribution,
    IEditorExtensionsRegistry,
};
use je_editor::config::editor_configuration::IEditorConfiguration;
use je_editor::cursor_common::EditorConfiguration as ConfigurationOptions;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE, LanguageService};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::model_line_projection_data::LineBreaksComputerFactory;
use je_editor::monospace_line_breaks_computer::MonospaceLineBreaksComputerFactory;
use je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferFactory;
use je_editor::services::abstract_code_editor_service::ICodeEditorRegistry;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;
use je_editor::view_model_types::IViewModel;
use je_instantiation::{ServiceCollection, ServiceEntry, ServiceId, ServicesAccessor};
use je_platform::commands::{ICommandService, I_COMMAND_SERVICE};
use je_platform::notification::{INotificationService, I_NOTIFICATION_SERVICE};

use super::editor_test_services::TestCommandService;
use super::test_code_editor_service::TestCodeEditorService;
use super::test_configuration::TestConfiguration;
use super::test_instantiation_service::TestInstantiationService;
use super::test_language_configuration_service::TestLanguageConfigurationService;
use super::test_notification_service::TestNotificationService;
use super::test_text_model::instantiate_text_model;
use super::test_theme_service::TestThemeService;

/// `ITestCodeEditor`: o editor de teste (ver o topo do módulo).
pub type ITestCodeEditor = TestCodeEditor;

/// O identificador de `ICodeEditorService` na `serviceCollection` dos testes (o `[ICodeEditorService, codeEditorService]` do
/// original). O serviço que o chamador injeta por ele é o registro de editores do widget; sem ele o widget não registra.
pub const I_CODE_EDITOR_SERVICE: ServiceId<TestCodeEditorService> = ServiceId::new("codeEditorService");

/// `ITestEditorCommand`.
pub trait ITestEditorCommand {
    /// `runEditorCommand(accessor, editor, args?)`.
    fn run_editor_command(
        &self,
        accessor: &ServicesAccessor,
        editor: &Rc<CodeEditorWidget>,
        args: Option<&Dynamic>,
    ) -> MaybePromise<()>;
}

/// `ITestEditorCommand` de um `IEditorCommand` (os comandos de palavra, por exemplo): o `command.runEditorCommand(accessor,
/// editor, null)` do `runEditorCommand` das suítes de teste, com o `null` chegando ao comando como `Dynamic::Null`.
pub struct EditorCommandAdapter<'a, C: IEditorCommand>(pub &'a C);

impl<C: IEditorCommand> ITestEditorCommand for EditorCommandAdapter<'_, C> {
    fn run_editor_command(
        &self,
        accessor: &ServicesAccessor,
        editor: &Rc<CodeEditorWidget>,
        args: Option<&Dynamic>,
    ) -> MaybePromise<()> {
        let args = args.cloned().unwrap_or(Dynamic::Null);
        MaybePromise::Promise(IEditorCommand::run_editor_command(self.0, Some(accessor), editor, &args))
    }
}

/// `runEditorCommand(editor, command)` das suítes de teste: `command.runEditorCommand(serviceAccessor, editor, null)`.
pub fn run_editor_command(editor: &TestCodeEditor, command: &impl IEditorCommand) {
    editor.run_command(&EditorCommandAdapter(command), None);
}

/// `ITestEditorAction`.
pub trait ITestEditorAction {
    /// `run(accessor, editor, args?)`.
    fn run(&self, accessor: &ServicesAccessor, editor: &Rc<CodeEditorWidget>, args: Option<&Dynamic>) -> MaybePromise<()>;
}

/// O `EditorExtensionsRegistry` sem registros (ver "Divergências" no topo do módulo).
struct NoExtensionsRegistry;

impl IEditorExtensionsRegistry for NoExtensionsRegistry {
    fn get_editor_contributions(&self) -> Vec<EditorContributionDescription> {
        Vec::new()
    }

    fn get_editor_actions(&self) -> Vec<EditorActionDescriptor> {
        Vec::new()
    }

    fn get_editor_command(&self, _command_id: &str) -> Option<Rc<dyn IEditorCommand>> {
        None
    }
}

/// `TestCodeEditor`.
pub struct TestCodeEditor {
    widget: Rc<CodeEditorWidget>,
    /// O `_instantiationService` injetado no construtor.
    services: Rc<CodeEditorServices>,
    /// `_hasTextFocus`.
    has_text_focus: Cell<bool>,
    /// O que `registerDisposable` registra: descartado depois do widget.
    disposables: Rc<DisposableStore>,
}

impl Deref for TestCodeEditor {
    type Target = CodeEditorWidget;

    fn deref(&self) -> &CodeEditorWidget {
        &self.widget
    }
}

impl TestCodeEditor {
    /// O `CodeEditorWidget` compartilhado (o `this` que o original passa a `runCommand` e às contribuições).
    pub fn widget(&self) -> &Rc<CodeEditorWidget> {
        &self.widget
    }

    /// `setHasTextFocus(hasTextFocus)`.
    pub fn set_has_text_focus(&self, has_text_focus: bool) {
        self.has_text_focus.set(has_text_focus);
    }

    /// `hasTextFocus()`.
    pub fn has_text_focus(&self) -> bool {
        self.has_text_focus.get()
    }

    /// `registerAndInstantiateContribution(id, ctor)`: `ctor` é o `createInstance(ctor, this)`.
    pub fn register_and_instantiate_contribution<T: IEditorContribution + 'static>(
        &self,
        id: &str,
        ctor: impl FnOnce(&Rc<CodeEditorWidget>, &TestInstantiationService) -> Rc<T>,
    ) -> Rc<T> {
        let contribution = ctor(&self.widget, &self.services);
        let registered: Rc<dyn IEditorContribution> = contribution.clone();
        self.widget.contributions().set(id, registered);
        contribution
    }

    /// `registerDisposable(disposable)`.
    pub fn register_disposable(&self, disposable: DisposableRef) {
        self.disposables.add(disposable);
    }

    /// `runCommand(command, args?)`.
    pub fn run_command(&self, command: &dyn ITestEditorCommand, args: Option<&Dynamic>) -> MaybePromise<()> {
        self.services.invoke_function(|accessor| command.run_editor_command(accessor, &self.widget, args))
    }

    /// `runAction(action, args?)`.
    pub fn run_action(&self, action: &dyn ITestEditorAction, args: Option<&Dynamic>) -> MaybePromise<()> {
        self.services.invoke_function(|accessor| action.run(accessor, &self.widget, args))
    }
}

impl IDisposable for TestCodeEditor {
    fn dispose(&self) {
        self.widget.dispose();
        self.disposables.dispose();
    }
}

/// `TestCodeEditorCreationOptions`: o objeto de opções do editor (`IEditorOptions`), mais `hasTextFocus` (padrão
/// `true`) e `envConfig`. É o objeto [`Dynamic`] que a `TestConfiguration` lê.
pub type TestCodeEditorCreationOptions = Dynamic;

/// `TestCodeEditorInstantiationOptions`: as opções de criação mais a `serviceCollection` (os serviços a usar).
pub struct TestCodeEditorInstantiationOptions {
    pub options: TestCodeEditorCreationOptions,
    pub service_collection: Option<ServiceCollection>,
}

impl TestCodeEditorInstantiationOptions {
    /// Com `options` e sem coleção de serviços.
    pub fn new(options: TestCodeEditorCreationOptions) -> Self {
        Self { options, service_collection: None }
    }
}

impl Default for TestCodeEditorInstantiationOptions {
    /// O `{}` do original.
    fn default() -> Self {
        Self::new(empty_options())
    }
}

fn empty_options() -> TestCodeEditorCreationOptions {
    Dynamic::object(std::iter::empty::<(&str, Dynamic)>())
}

/// `ITextModel | string | string[] | ITextBufferFactory`.
pub enum TestEditorInput {
    Model(Rc<TextModel>),
    Text(String),
    Lines(Vec<String>),
    Factory(PieceTreeTextBufferFactory),
}

impl From<Rc<TextModel>> for TestEditorInput {
    fn from(model: Rc<TextModel>) -> Self {
        TestEditorInput::Model(model)
    }
}

impl From<&str> for TestEditorInput {
    fn from(text: &str) -> Self {
        TestEditorInput::Text(text.to_owned())
    }
}

impl From<String> for TestEditorInput {
    fn from(text: String) -> Self {
        TestEditorInput::Text(text)
    }
}

impl From<&[&str]> for TestEditorInput {
    fn from(lines: &[&str]) -> Self {
        TestEditorInput::Lines(lines.iter().map(|line| (*line).to_owned()).collect())
    }
}

impl From<Vec<String>> for TestEditorInput {
    fn from(lines: Vec<String>) -> Self {
        TestEditorInput::Lines(lines)
    }
}

impl From<PieceTreeTextBufferFactory> for TestEditorInput {
    fn from(factory: PieceTreeTextBufferFactory) -> Self {
        TestEditorInput::Factory(factory)
    }
}

/// O `TestInstantiationService` que `createCodeEditorServices` devolve, com os serviços de teste que não têm
/// identificador no porte (ver "Divergências" no topo do módulo).
pub struct CodeEditorServices {
    instantiation_service: Rc<TestInstantiationService>,
    pub undo_redo_service: Rc<UndoRedoService>,
    pub language_configuration_service: Rc<TestLanguageConfigurationService>,
    pub theme_service: Rc<TestThemeService>,
    /// O `ICodeEditorService` (`TestCodeEditorService`).
    pub code_editor_service: Rc<TestCodeEditorService>,
    /// O `code_editor_service` veio da `serviceCollection` do chamador: só então o widget o registra (o dublê
    /// criado aqui não tem observador, e criar o registro à toa deixaria um `Disposable` sem dono).
    code_editor_service_injected: bool,
}

impl Deref for CodeEditorServices {
    type Target = TestInstantiationService;

    fn deref(&self) -> &TestInstantiationService {
        &self.instantiation_service
    }
}

/// O `define`/`defineInstance` de `createCodeEditorServices`: só registra se a coleção ainda não tem o serviço.
fn define<T: ?Sized + 'static>(services: &ServiceCollection, id: ServiceId<T>, instance: Rc<T>) {
    if !services.has(id) {
        services.set(id, instance);
    }
}

/// `createCodeEditorServices(disposables, services)`.
pub fn create_code_editor_services(disposables: &DisposableStore, services: Option<ServiceCollection>) -> Rc<CodeEditorServices> {
    let services = services.unwrap_or_default();

    let language_service = LanguageService::new(false);
    let language_service_instance: Rc<dyn ILanguageService> = language_service.clone();
    define(&services, I_LANGUAGE_SERVICE, language_service_instance);
    let notification_service: Rc<dyn INotificationService> = Rc::new(TestNotificationService);
    define(&services, I_NOTIFICATION_SERVICE, notification_service);
    let command_service = TestCommandService::new();
    let command_service_instance: Rc<dyn ICommandService> = command_service.clone();
    define(&services, I_COMMAND_SERVICE, command_service_instance);

    let injected_code_editor_service = match services.get(I_CODE_EDITOR_SERVICE) {
        Some(ServiceEntry::Instance(instance)) => Some(instance),
        _ => None,
    };

    let instantiation_service = TestInstantiationService::with_options(services, true, None, false);
    command_service.bind_instantiation_service(Rc::downgrade(&instantiation_service));

    let language_configuration_service = TestLanguageConfigurationService::new();
    let theme_service = TestThemeService::new();

    disposables.add_rc(instantiation_service.clone());
    disposables.add_rc(language_service);
    disposables.add_rc(command_service);
    disposables.add_rc(language_configuration_service.clone());
    disposables.add_rc(theme_service.clone());

    Rc::new(CodeEditorServices {
        instantiation_service,
        undo_redo_service: UndoRedoService::for_tests(),
        language_configuration_service,
        theme_service,
        code_editor_service_injected: injected_code_editor_service.is_some(),
        code_editor_service: injected_code_editor_service.unwrap_or_else(|| Rc::new(TestCodeEditorService::default())),
    })
}

/// `scheduleAtNextAnimationFrame` do editor de teste (ver "Divergências" no topo do módulo).
fn schedule_at_next_animation_frame() -> ScheduleAtNextAnimationFrame {
    Rc::new(|callback| {
        let cancelled = Rc::new(Cell::new(false));
        let flag = cancelled.clone();
        runtime::set_timeout(
            move || {
                if !flag.get() {
                    callback();
                }
            },
            16,
        );
        plain_disposable(move || cancelled.set(true))
    })
}

/// `runWhenWindowIdle` do editor de teste: o `runWhenGlobalIdle` do porte.
fn run_when_idle() -> RunWhenIdle {
    Rc::new(|runner: Box<dyn FnOnce(IdleDeadline)>, timeout| run_when_global_idle(runner, timeout))
}

/// `instantiateTestCodeEditor(instantiationService, model, options)`.
pub fn instantiate_test_code_editor(
    services: &Rc<CodeEditorServices>,
    model: Option<Rc<TextModel>>,
    options: &TestCodeEditorCreationOptions,
) -> Rc<TestCodeEditor> {
    // `TestCodeEditor._createConfiguration`.
    let configuration: Rc<dyn IEditorConfiguration> = TestConfiguration::new(options);
    let dom_line_breaks_computer_factory: Rc<dyn LineBreaksComputerFactory> =
        Rc::new(MonospaceLineBreaksComputerFactory::create(ConfigurationOptions::options(&*configuration)));

    // `contributions: []`
    let widget = CodeEditorWidget::new(
        configuration,
        CodeEditorWidgetOptions { contributions: Some(Vec::new()), telemetry_data: None },
        CodeEditorWidgetServices {
            language_configuration_service: services.language_configuration_service.clone(),
            theme_service: services.theme_service.clone(),
            notification_service: services.get(I_NOTIFICATION_SERVICE),
            command_service: services.get(I_COMMAND_SERVICE),
            extensions_registry: Rc::new(NoExtensionsRegistry),
            dom_line_breaks_computer_factory,
            schedule_at_next_animation_frame: schedule_at_next_animation_frame(),
            run_when_idle: run_when_idle(),
            code_editor_service: services
                .code_editor_service_injected
                .then(|| -> Rc<dyn ICodeEditorRegistry> { services.code_editor_service.registry().clone() }),
        },
    );
    let editor = Rc::new(TestCodeEditor {
        widget,
        services: services.clone(),
        has_text_focus: Cell::new(false),
        disposables: DisposableStore::new(),
    });

    // `if (typeof options.hasTextFocus === 'undefined') { options.hasTextFocus = true; }`
    let has_text_focus = match options.get_str("hasTextFocus") {
        Dynamic::Undefined => {
            if let Dynamic::Object(object) = options {
                object.set(&JsString::from("hasTextFocus"), Dynamic::from(true));
            }
            true
        }
        value => value.is_truthy(),
    };
    editor.set_has_text_focus(has_text_focus);
    editor.set_model(model);
    if let Some(view_model) = editor.get_view_model() {
        view_model.set_has_focus(has_text_focus);
    }
    editor
}

/// `createTestCodeEditor(model, options)`.
pub fn create_test_code_editor(model: Option<Rc<TextModel>>, options: TestCodeEditorInstantiationOptions) -> Rc<TestCodeEditor> {
    let disposables = DisposableStore::new();
    let TestCodeEditorInstantiationOptions { options, service_collection } = options;
    let services = create_code_editor_services(&disposables, service_collection);

    let editor = instantiate_test_code_editor(&services, model, &options);
    editor.register_disposable(disposables);
    editor
}

/// O que `_withTestCodeEditor` prepara antes de chamar o callback.
struct TestCodeEditorFixture {
    disposables: Rc<DisposableStore>,
    services: Rc<CodeEditorServices>,
    editor: Rc<TestCodeEditor>,
    view_model: Rc<EditorViewModel>,
}

impl TestCodeEditorFixture {
    fn new(arg: TestEditorInput, options: TestCodeEditorInstantiationOptions) -> Self {
        let disposables = DisposableStore::new();
        let TestCodeEditorInstantiationOptions { options, service_collection } = options;
        let services = create_code_editor_services(&disposables, service_collection);

        // create a model if necessary
        let create_model = |source: TextSource| {
            let model = instantiate_text_model(
                services.undo_redo_service.clone(),
                source,
                None,
                RelaxedTextModelCreationOptions::default(),
                None,
            );
            let owned = model.clone();
            disposables.add(to_disposable(move || owned.dispose()));
            model
        };
        let model = match arg {
            TestEditorInput::Model(model) => model,
            TestEditorInput::Text(text) => create_model(text.into()),
            TestEditorInput::Lines(lines) => create_model(lines.join("\n").into()),
            TestEditorInput::Factory(factory) => create_model(factory.into()),
        };

        let editor = instantiate_test_code_editor(&services, Some(model), &options);
        disposables.add_rc(editor.clone());
        let view_model = editor.get_view_model().expect("o editor de teste tem o modelo ligado");
        view_model.set_has_focus(true);
        Self { disposables, services, editor, view_model }
    }
}

/// `withTestCodeEditor(text, options, callback)`.
pub fn with_test_code_editor(
    text: impl Into<TestEditorInput>,
    options: TestCodeEditorInstantiationOptions,
    callback: impl FnOnce(&Rc<TestCodeEditor>, &Rc<EditorViewModel>, &Rc<CodeEditorServices>),
) {
    let fixture = TestCodeEditorFixture::new(text.into(), options);
    callback(&fixture.editor, &fixture.view_model, &fixture.services);
    fixture.disposables.dispose();
}

/// `withAsyncTestCodeEditor(text, options, callback)`.
pub fn with_async_test_code_editor(
    text: impl Into<TestEditorInput>,
    options: TestCodeEditorInstantiationOptions,
    callback: impl FnOnce(Rc<TestCodeEditor>, Rc<EditorViewModel>, Rc<CodeEditorServices>) -> Promise<()>,
) -> Promise<()> {
    let TestCodeEditorFixture { disposables, services, editor, view_model } = TestCodeEditorFixture::new(text.into(), options);
    callback(editor, view_model, services).then(move |()| disposables.dispose())
}
