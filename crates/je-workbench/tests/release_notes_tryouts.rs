//! Porte de `src/vs/workbench/contrib/update/test/browser/releaseNotesTryouts.test.ts` (suíte `Release notes Try This`),
//! na mesma ordem do original.
//!
//! O documento é a árvore `Element`/`Node` do `html_parser` (sem DOM): `renderReleaseNotesMarkdown` é
//! `render_release_notes_markdown`, o `$('div')` com `safeSetInnerHtml(container, html, config)` é o [`container_of`] (o
//! `sanitize_html` do porte com a mesma configuração), `querySelector`, `querySelectorAll` e `textContent` são percorridos aqui
//! mesmo, e o `click()` do documento é o [`ReleaseNotesTryoutsClient::click`] com o caminho do alvo. O que cada caso afirma
//! sobre o documento renderizado é conferido na árvore; os casos que o original escreve contra a decisão por link
//! ([`ReleaseNotesTryouts::render_link`] e `render_code_span`) e contra as mensagens do webview seguem conferindo o dado.
//!
//! Casos portados por inteiro (mesmas entradas e mesmas afirmações): `renders ready links...`, `hides legacy/shorthand`
//! (os dois), `renders an unavailable explanation` (os dois), `keeps a readable, inert fallback` (os 11), `accepts the ID
//! length boundary`, `posts only changed occurrences`, `activates the selected shorthand occurrence`, `revalidates primary
//! actions`, `runs ready examples`, `coalesces repeated activation`, `resolves the setup command...`, `does not run stale
//! setup actions`, `rejects forged protocol data` (os 7), `ignores messages from an earlier document`, `silently ignores
//! malformed or unknown direct command links`, `restores source focus` (os 3), `does not reclaim focus`, `cancels pending
//! preparation on webview disposal`, `does not activate shorthand inside ...` (os 11), `preserves TOC, setting chips,
//! command links and media sanitization...` e `serialized webview script sends only the document, occurrence, ID and
//! action...` (o JavaScript serializado é o [`ReleaseNotesTryoutsClient`], o lado do documento em Rust).
//!
//! Casos portados por projeção (o que o DOM verificava vira a verificação do dado que o alimenta):
//! - `renders shorthand with literal authored text...`: o rótulo é texto simples (o original passa HTML e markdown pelo
//!   sanitizador); confere o rótulo, o `href`, o nome acessível e que nada é executado.
//! - `keeps malformed shorthand as literal code` (os 13): `render_code_span` devolve `None`, ou seja, o código fica literal.
//! - `accepts the existing shorthand ID alphabet and length boundary`: os `href` que `render_code_span` gera.
//! - `keeps an unknown shorthand ID readable and inert`: o atalho gera o link, e o link vira o texto de fallback.
//! - `a failing availability provider...`: o estado dos três links, o log do erro e que nada roda (o `h1` é markdown).
//! - `TRYOUTS is a renderer capability...`: o texto que o renderer recebe (`prepare_release_notes_markdown`).
//! - `escapes local text...`: os textos locais ficam literais no estado do controle (a sanitização do HTML buscado é DOM).
//! - `skips tryout DOM processing...` e `still removes forged tryout attributes...`: `needs_render` decide se há o que
//!   processar (falso para as notas comuns, verdadeiro para os atributos forjados e para os links de tryout).
//! - `recognizes a tryout link with ... encoded command ID` (os 2): o `%77` é decodificado pelo `URI`; a entidade
//!   `&#119;` já chega decodificada ao `href` (quem decodifica é o parser de HTML), então o segundo caso passa o `href` comum.
//! - `preserves distinct shorthand and legacy labels...` e `updates controls in place...`: os estados que o documento recebe
//!   nas mensagens `releaseNotesTryouts` (rótulo, nome acessível, `aria-disabled`, `hidden`, `href`, mensagem); foco e
//!   `mouseover` do DOM não existem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] do [`Fixture`]; `store.add` vira `guard.add_rc`.
//! - **`await timeout(0)`.** É o `timeout` do `common` e roda em `run_with_real_timers`.
//! - **`deepStrictEqual` de mensagens.** É [`assert_deep_strict_equal`] sobre [`Dynamic`].
//! - **`sinon.stub`.** São os dublês [`TestTryoutService`], [`TestCommandService`], [`TestNotificationService`],
//!   [`TestLogService`] e [`TestWebview`], que guardam o que o original lia dos stubs.
//! - **`instantiationService`.** O `IExtensionService`, o `ILanguageService` e o `SimpleSettingRenderer` (criado a cada
//!   `render`, com o `IPreferencesService` que o caso definiu) são os do [`Fixture`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::JsString;
use je_base::browser::html_parser::{Element, Node};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::html_content::create_command_uri;
use je_base::lifecycle::IDisposable;
use je_base::markdown::dom::inner_html;
use je_base::markdown::sanitize::{AttributeRule, DomSanitizerConfig, Protocols, sanitize_html};
use je_base::promise::{DeferredPromise, Promise};
use je_base::uri::{Field, URI, UriChange};
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::commands::{CommandEvent, ICommandService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::{LogArg, LogLevel, LogMessage, Logger};
use je_platform::notification::{
    INotificationService, NoOpNotification, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage,
    NotificationSource, NotificationSourceFilter, NotificationsFilter, PromptChoiceKind, PromptOptions, Severity, StatusHandle,
    StatusMessageOptions,
};
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_ui::Ui;
use je_widgets::context_view_api::{IContextMenuDelegate, IContextMenuProvider};
use je_workbench::contrib::markdown::markdown_setting_renderer::SimpleSettingRenderer;
use je_workbench::contrib::onboarding::onboarding_scenario::{OnboardingPresentationRef, OnboardingScenario, OnboardingTrigger};
use je_workbench::contrib::onboarding::onboarding_tryout::{
    OnboardingTryoutAvailability, OnboardingTryoutCommand, OnboardingTryoutMetadata, OnboardingTryoutResult,
    OnboardingTryoutSetupAction, OnboardingTryoutTargetWindow, OnboardingTryoutUnavailable, RUN_ONBOARDING_TRYOUT_COMMAND_ID,
};
use je_workbench::contrib::onboarding::onboarding_tryout_service::{
    IOnboardingTryoutService, OnboardingTryoutRunOptions, OnboardingTryoutSource, create_onboarding_tryout_uri,
};
use je_workbench::contrib::update::browser::release_notes_editor::{prepare_release_notes_markdown, render_release_notes_markdown};
use je_workbench::contrib::update::browser::release_notes_tryouts::{
    ReleaseNotesTryoutState, ReleaseNotesTryouts, ReleaseNotesTryoutsClient, ReleaseNotesTryoutsWebview, RenderedLink,
};
use je_workbench::contrib::webview::browser_webview::WebviewMessageReceivedEvent;
use je_workbench::services::extensions::extensions::IExtensionService;
use je_workbench::services::preferences::preferences::{IOpenSettingsOptions, IPreferencesService, ISetting, SettingSchemaType};
use je_workbench::test::browser::workbench_test_services::TestExtensionService;

type RunImplementation = Rc<dyn Fn(&str, CancellationToken) -> Promise<OnboardingTryoutResult>>;

/// O `IOnboardingTryoutService` do original (`mock<IOnboardingTryoutService>`).
struct TestTryoutService {
    scenarios: RefCell<BTreeMap<String, Rc<OnboardingScenario>>>,
    availability: RefCell<HashMap<String, OnboardingTryoutAvailability>>,
    broken: RefCell<HashSet<String>>,
    changes: Rc<Emitter<()>>,
    runs: RefCell<Vec<String>>,
    options: RefCell<Vec<Option<OnboardingTryoutRunOptions>>>,
    run_implementation: RefCell<RunImplementation>,
}

impl TestTryoutService {
    /// `register(id, metadata)`.
    fn register(&self, id: &str, customize: impl FnOnce(&mut OnboardingTryoutMetadata)) {
        let mut metadata = OnboardingTryoutMetadata {
            title: "Local Example".to_string(),
            description: "A local description.".to_string(),
            ..OnboardingTryoutMetadata::default()
        };
        customize(&mut metadata);
        self.scenarios.borrow_mut().insert(
            id.to_string(),
            Rc::new(OnboardingScenario {
                id: id.to_string(),
                seen_key: None,
                when: None,
                trigger: OnboardingTrigger::Command { command_id: RUN_ONBOARDING_TRYOUT_COMMAND_ID.to_string() },
                priority: None,
                presentation: OnboardingPresentationRef { kind: "test".to_string(), payload: Dynamic::Undefined },
                tryout: Some(Rc::new(metadata)),
                experiment: None,
                developer_mode_variations: None,
                repeatable: None,
            }),
        );
    }

    fn set_availability(&self, id: &str, availability: OnboardingTryoutAvailability) {
        self.availability.borrow_mut().insert(id.to_string(), availability);
    }
}

impl IOnboardingTryoutService for TestTryoutService {
    fn on_did_change(&self) -> Event<()> {
        self.changes.event()
    }

    fn get_tryout(&self, id: &str) -> Option<Rc<OnboardingScenario>> {
        self.scenarios.borrow().get(id).cloned()
    }

    fn get_tryouts(&self) -> Vec<Rc<OnboardingScenario>> {
        self.scenarios.borrow().values().cloned().collect()
    }

    fn get_availability(&self, id: &str) -> OnboardingTryoutAvailability {
        assert!(self.scenarios.borrow().contains_key(id), "Only locally registered IDs may be resolved");
        if self.broken.borrow().contains(id) {
            errors::throw(Error::new("Broken provider"));
        }
        self.availability.borrow().get(id).cloned().unwrap_or(OnboardingTryoutAvailability::Ready)
    }

    fn run(&self, id: &str, token: CancellationToken, options: Option<OnboardingTryoutRunOptions>) -> Promise<OnboardingTryoutResult> {
        self.runs.borrow_mut().push(id.to_string());
        self.options.borrow_mut().push(options);
        let implementation = self.run_implementation.borrow().clone();
        implementation(id, token)
    }
}

#[derive(Default)]
struct TestCommandService {
    calls: RefCell<Vec<(String, Vec<Dynamic>)>>,
}

impl ICommandService for TestCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.calls.borrow_mut().push((command_id.to_string(), args));
        Promise::resolved(Dynamic::Undefined)
    }
}

/// O `TestNotificationService` que guarda o texto de cada `notify`.
#[derive(Default)]
struct TestNotificationService {
    notifications: RefCell<Vec<String>>,
}

fn message_text(message: &NotificationMessage) -> String {
    match message {
        NotificationMessage::Text(text) => text.clone(),
        other => format!("{other:?}"),
    }
}

impl TestNotificationService {
    fn record(&self, message: Vec<NotificationMessage>) {
        self.notifications.borrow_mut().push(message_text(&message[0]));
    }
}

struct NoOpStatus;

impl StatusHandle for NoOpStatus {
    fn close(&self) {}
}

impl INotificationService for TestNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        Event::none()
    }
    fn set_filter(&self, _filter: NotificationFilterSetting) {}
    fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
        NotificationsFilter::Off
    }
    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        Vec::new()
    }
    fn remove_filter(&self, _source_id: &str) {}
    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle> {
        self.notifications.borrow_mut().push(message_text(&notification.message));
        Rc::new(NoOpNotification::new())
    }
    fn info(&self, message: Vec<NotificationMessage>) {
        self.record(message);
    }
    fn warn(&self, message: Vec<NotificationMessage>) {
        self.record(message);
    }
    fn error(&self, message: Vec<NotificationMessage>) {
        self.record(message);
    }
    fn prompt(&self, _severity: Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        Rc::new(NoOpNotification::new())
    }
    fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        Rc::new(NoOpStatus)
    }
}

/// O `NullLogService` com o `stub(logService, 'error')`: conta as chamadas de `error`.
#[derive(Default)]
struct TestLogService {
    errors: Cell<usize>,
}

impl IDisposable for TestLogService {
    fn dispose(&self) {}
}

impl Logger for TestLogService {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        Event::none()
    }
    fn get_level(&self) -> LogLevel {
        LogLevel::Info
    }
    fn set_level(&self, _level: LogLevel) {}
    fn trace_args(&self, _message: &str, _args: &[LogArg]) {}
    fn debug_args(&self, _message: &str, _args: &[LogArg]) {}
    fn info_args(&self, _message: &str, _args: &[LogArg]) {}
    fn warn_args(&self, _message: &LogMessage, _args: &[LogArg]) {}
    fn error_args(&self, _message: &LogMessage, _args: &[LogArg]) {
        self.errors.set(self.errors.get() + 1);
    }
    fn flush(&self) {}
}

/// O `IWebview` do original: `onMessage`, `onDidDispose`, `postMessage` (guarda em `messages`) e `focus` (conta).
struct TestWebview {
    on_message: Rc<Emitter<WebviewMessageReceivedEvent>>,
    on_dispose: Rc<Emitter<()>>,
    messages: RefCell<Vec<Dynamic>>,
    focus_count: Cell<usize>,
}

impl ReleaseNotesTryoutsWebview for TestWebview {
    fn on_did_dispose(&self) -> Event<()> {
        self.on_dispose.event()
    }
    fn on_message(&self) -> Event<WebviewMessageReceivedEvent> {
        self.on_message.event()
    }
    fn post_message(&self, message: Dynamic) -> Promise<bool> {
        self.messages.borrow_mut().push(message);
        Promise::resolved(true)
    }
    fn focus(&self) {
        self.focus_count.set(self.focus_count.get() + 1);
    }
}

/// `<Partial<IPreferencesService>>{ getSetting }`: só conhece a configuração que o caso define (`getSetting: () => new
/// class extends mock<ISetting>() { key, value, type }`).
struct TestPreferencesService {
    setting: RefCell<Option<ISetting>>,
}

impl IPreferencesService for TestPreferencesService {
    fn open_user_settings(&self, _options: Option<&IOpenSettingsOptions>) -> Promise<()> {
        panic!("openUserSettings is not a function")
    }

    fn open_application_settings(&self, _options: Option<&IOpenSettingsOptions>) -> Promise<()> {
        panic!("openApplicationSettings is not a function")
    }

    fn get_setting(&self, setting: &str) -> Option<ISetting> {
        self.setting.borrow().clone().filter(|known| known.key == setting)
    }
}

/// `<IContextMenuService>{}` (o `ContextMenuService` do original não é aberto por nenhum caso).
struct TestContextMenuService;

impl IContextMenuProvider for TestContextMenuService {
    fn show_context_menu(&self, _delegate: Rc<dyn IContextMenuDelegate>) {
        panic!("showContextMenu is not a function")
    }
}

/// `IClipboardService` que o `SimpleSettingRenderer` recebe e nenhum caso usa.
struct TestClipboardService;

impl IClipboardService for TestClipboardService {
    fn write_text(&self, _text: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn read_text(&self) -> Promise<String> {
        panic!("readText is not a function")
    }

    fn read_find_text(&self) -> Promise<JsString> {
        panic!("readFindText is not a function")
    }

    fn write_find_text(&self, _text: &JsString) -> Promise<()> {
        panic!("writeFindText is not a function")
    }

    fn write_resources(&self, _resources: Vec<URI>) -> Promise<()> {
        panic!("writeResources is not a function")
    }

    fn read_resources(&self) -> Promise<Vec<URI>> {
        panic!("readResources is not a function")
    }

    fn has_resources(&self) -> Promise<bool> {
        panic!("hasResources is not a function")
    }
}

struct Fixture {
    _guard: LeakGuard,
    service: Rc<TestTryoutService>,
    tryouts: Rc<ReleaseNotesTryouts>,
    webview: Rc<TestWebview>,
    commands: Rc<TestCommandService>,
    notifications: Rc<TestNotificationService>,
    log: Rc<TestLogService>,
    can_restore_focus: Rc<Cell<bool>>,
    extension_service: Rc<dyn IExtensionService>,
    language_service: Rc<dyn ILanguageService>,
    configuration_service: Rc<dyn IConfigurationService>,
    preferences: Rc<TestPreferencesService>,
}

impl Fixture {
    /// O `setup` da suíte.
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let changes = guard.add_rc(Emitter::<()>::new());
        let default_run: RunImplementation = Rc::new(|_: &str, _: CancellationToken| Promise::resolved(opened()));
        let service = Rc::new(TestTryoutService {
            scenarios: RefCell::new(BTreeMap::new()),
            availability: RefCell::new(HashMap::new()),
            broken: RefCell::new(HashSet::new()),
            changes,
            runs: RefCell::new(Vec::new()),
            options: RefCell::new(Vec::new()),
            run_implementation: RefCell::new(default_run),
        });
        service.register("sample", |_| {});
        service.register("other", |_| {});
        let commands = Rc::new(TestCommandService::default());
        let notifications = Rc::new(TestNotificationService::default());
        let log = Rc::new(TestLogService::default());
        let tryouts = guard.add_rc(ReleaseNotesTryouts::new(service.clone(), commands.clone(), notifications.clone(), log.clone()));
        let webview = Rc::new(TestWebview {
            on_message: guard.add_rc(Emitter::<WebviewMessageReceivedEvent>::new()),
            on_dispose: guard.add_rc(Emitter::<()>::new()),
            messages: RefCell::new(Vec::new()),
            focus_count: Cell::new(0),
        });
        let language_service = guard.add_rc(LanguageService::new(false));
        let configuration_service = TestConfigurationService::new(None);
        guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
        let configuration_service: Rc<dyn IConfigurationService> = configuration_service;
        Self {
            _guard: guard,
            service,
            tryouts,
            webview,
            commands,
            notifications,
            log,
            can_restore_focus: Rc::new(Cell::new(true)),
            extension_service: TestExtensionService::new(),
            language_service,
            configuration_service,
            preferences: Rc::new(TestPreferencesService { setting: RefCell::new(None) }),
        }
    }

    /// `instantiationService.createInstance(SimpleSettingRenderer)`: o renderizador lê o `IPreferencesService` da hora.
    fn setting_renderer(&self) -> Rc<SimpleSettingRenderer> {
        SimpleSettingRenderer::new(
            Rc::clone(&self.configuration_service),
            Rc::new(TestContextMenuService),
            self.preferences.clone(),
            null_telemetry_service(),
            Rc::new(TestClipboardService),
            Ui::new(),
        )
    }

    /// `render(markdown, enabled)`: o contêiner com o HTML das notas, sanitizado como o `safeSetInnerHtml` do original.
    fn render_document(&self, markdown: &str, enabled: bool) -> Promise<Element> {
        render_release_notes_markdown(
            markdown,
            Rc::clone(&self.extension_service),
            Rc::clone(&self.language_service),
            &self.setting_renderer(),
            Some("stable"),
            enabled.then(|| self.tryouts.clone()),
            None,
        )
        .then(|html| container_of(&html))
    }

    fn fire_message(&self, message: Dynamic) {
        self.webview.on_message.fire(WebviewMessageReceivedEvent { message, transfer: None });
    }

    /// `attach()`.
    fn attach(&self) {
        let can_restore_focus = self.can_restore_focus.clone();
        self.tryouts.attach(self.webview.clone(), Rc::new(move || can_restore_focus.get()));
        self.fire_message(Dynamic::object([
            ("type", Dynamic::string("releaseNotesTryoutsReady")),
            ("documentId", Dynamic::string(&self.tryouts.document_id)),
        ]));
        self.webview.messages.borrow_mut().clear();
    }

    /// `request(action, extra)`: o spread do objeto troca as chaves repetidas e acrescenta as novas.
    fn request(&self, action: &str, extra: Vec<(&str, Dynamic)>) {
        let mut entries: Vec<(String, Dynamic)> = vec![
            ("type".to_string(), Dynamic::string("releaseNotesTryout")),
            ("documentId".to_string(), Dynamic::string(&self.tryouts.document_id)),
            ("id".to_string(), Dynamic::string("sample")),
            ("index".to_string(), Dynamic::Number(0.0)),
            ("action".to_string(), Dynamic::string(action)),
        ];
        for (key, value) in extra {
            match entries.iter_mut().find(|(existing, _)| existing == key) {
                Some(entry) => entry.1 = value,
                None => entries.push((key.to_string(), value)),
            }
        }
        self.fire_message(Dynamic::object(entries));
    }

    fn set_run_implementation(&self, implementation: impl Fn(&str, CancellationToken) -> Promise<OnboardingTryoutResult> + 'static) {
        *self.service.run_implementation.borrow_mut() = Rc::new(implementation);
    }

    fn render(&self, href: &str, text: &str) -> RenderedLink {
        self.tryouts.render_link(href, text, false)
    }

    /// O link de atalho `try(...)`: o `render_code_span` gera o `<a>` e o `render_link` o resolve como o `render`.
    fn render_shorthand(&self, text: &str) -> RenderedLink {
        let html = self.tryouts.render_code_span(text).expect("o atalho é válido");
        let href = html.split("href=\"").nth(1).and_then(|rest| rest.split('"').next()).expect("o link tem href");
        let label = html.split_once('>').and_then(|(_, rest)| rest.strip_suffix("</a>")).expect("o link tem rótulo");
        self.tryouts.render_link(&href.replace("&amp;", "&"), label, true)
    }

    fn messages(&self) -> Dynamic {
        Dynamic::array(self.webview.messages.borrow().iter().cloned())
    }

    fn notifications(&self) -> Vec<String> {
        self.notifications.notifications.borrow().clone()
    }

    fn runs(&self) -> Vec<String> {
        self.service.runs.borrow().clone()
    }

    fn command_count(&self) -> usize {
        self.commands.calls.borrow().len()
    }
}

fn case<F: Future<Output = ()> + 'static>(body: impl FnOnce(Fixture) -> F + 'static) {
    run_with_real_timers(async move { body(Fixture::new()).await });
}

const TRYOUT_ID: &str = "data-release-notes-tryout-id";

/// `element.dataset.releaseNotesTryoutId` (e os outros `data-*`): o valor do atributo.
fn dataset(element: &Element, name: &str) -> Option<String> {
    element.get_attribute(name).map(JsString::to_string_lossy)
}

/// O `$('div')` com `safeSetInnerHtml(container, html, ...)` do `render` do original: o `sanitize_html` com a mesma configuração.
fn container_of(html: &JsString) -> Element {
    let strings = |names: &[&str]| -> Vec<String> { names.iter().map(|name| (*name).to_owned()).collect() };
    let config = DomSanitizerConfig {
        allowed_link_protocols: Some(Protocols::List(strings(&["http", "https", "command", "code-setting"]))),
        allowed_tags_augment: Some(strings(&["button", "nav", "svg", "path"])),
        allowed_attributes_augment: Some(
            strings(&[
                "class",
                "id",
                "hidden",
                "aria-label",
                "aria-disabled",
                "data-release-notes-tryout-id",
                "data-release-notes-tryout-index",
                "viewBox",
                "fill",
                "d",
            ])
            .into_iter()
            .map(AttributeRule::Name)
            .collect(),
        ),
        ..DomSanitizerConfig::default()
    };
    let mut container = Element::new("div", Vec::new());
    container.children = sanitize_html(html.units(), &config);
    container
}

/// `container.querySelectorAll(...)`: os descendentes que casam, em ordem de documento.
fn select_all<'a>(root: &'a Element, matches: &dyn Fn(&Element) -> bool) -> Vec<&'a Element> {
    let mut found = Vec::new();
    for child in root.element_children() {
        if matches(child) {
            found.push(child);
        }
        found.extend(select_all(child, matches));
    }
    found
}

/// O caminho (índices em `children`) do primeiro descendente que casa: o alvo de um `click()`.
fn path_to(root: &Element, matches: &dyn Fn(&Element) -> bool) -> Option<Vec<usize>> {
    for (index, child) in root.children.iter().enumerate() {
        if let Node::Element(child) = child {
            if matches(child) {
                return Some(vec![index]);
            }
            if let Some(mut rest) = path_to(child, matches) {
                rest.insert(0, index);
                return Some(rest);
            }
        }
    }
    None
}

fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `sampleUri.toString()` e o equivalente para outro ID.
fn tryout_href(id: &str) -> String {
    uri_text(&create_onboarding_tryout_uri(id).expect("o ID é válido"))
}

fn command_uri(args: &[Dynamic]) -> URI {
    create_command_uri(&RUN_ONBOARDING_TRYOUT_COMMAND_ID.encode_utf16().collect::<Vec<_>>(), args)
}

fn tryout_state(link: RenderedLink) -> ReleaseNotesTryoutState {
    match link {
        RenderedLink::Tryout { state, .. } => state,
        other => panic!("esperava um controle de tryout, veio {other:?}"),
    }
}

fn text(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava texto, veio {other:?}"),
    }
}

fn unavailable(message: &str, action: Option<(&str, &str, Vec<Dynamic>)>) -> OnboardingTryoutAvailability {
    OnboardingTryoutAvailability::Unavailable(OnboardingTryoutUnavailable {
        message: message.to_string(),
        action: action.map(|(label, id, arguments)| OnboardingTryoutSetupAction {
            label: label.to_string(),
            command: OnboardingTryoutCommand { id: id.to_string(), arguments: if arguments.is_empty() { None } else { Some(arguments) } },
        }),
    })
}

fn release_notes_options() -> Vec<Option<OnboardingTryoutRunOptions>> {
    vec![Some(OnboardingTryoutRunOptions { source: OnboardingTryoutSource::ReleaseNotes, run_id: None })]
}

fn opened() -> OnboardingTryoutResult {
    OnboardingTryoutResult::Launched { kind: je_workbench::contrib::onboarding::onboarding_tryout::OnboardingTryoutLaunchKind::Opened, target_scope: None }
}

#[test]
fn renders_shorthand_with_literal_authored_text_and_installed_accessible_context() {
    case(|fx| async move {
        fx.service.register("sample", |metadata| {
            metadata.is_ai = Some(true);
            metadata.target_window = Some(OnboardingTryoutTargetWindow::Agents);
        });
        let label = "Explore the model, (`model`)";
        let state = tryout_state(fx.render_shorthand(&format!("try( sample , {label} )")));
        assert_eq!(state.label, label);
        assert_eq!(state.href, tryout_href("sample"));
        assert_eq!(
            state.aria_label,
            format!("{label} Local Example A local description. Opens in the Agents window. Chat examples are prepared for review and are not sent automatically.")
        );
        assert_eq!(state.kind.as_str(), "ready");
        assert!(fx.runs().is_empty());
        assert_eq!(fx.command_count(), 0);
    });
}

fn keeps_malformed_shorthand_as_literal_code(source: String) {
    case(move |fx| async move {
        assert_eq!(fx.tryouts.render_code_span(&source), None);
        assert!(fx.runs().is_empty());
        assert_eq!(fx.command_count(), 0);
    });
}

macro_rules! malformed_shorthand_cases {
    ($($name:ident => $source:expr),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                keeps_malformed_shorthand_as_literal_code($source);
            }
        )*
    };
}

malformed_shorthand_cases! {
    keeps_malformed_shorthand_as_literal_code_without_a_label => "try(sample)".to_string(),
    keeps_malformed_shorthand_as_literal_code_without_an_id => "try(,Missing ID)".to_string(),
    keeps_malformed_shorthand_as_literal_code_with_an_empty_label => "try(sample,)".to_string(),
    keeps_malformed_shorthand_as_literal_code_with_a_blank_label => "try(sample,   )".to_string(),
    keeps_malformed_shorthand_as_literal_code_without_the_closing_parenthesis => "try(sample,Missing close".to_string(),
    keeps_malformed_shorthand_as_literal_code_with_a_path_like_id => "try(../sample,Invalid ID)".to_string(),
    keeps_malformed_shorthand_as_literal_code_with_an_invalid_prefix => "try(.sample,Invalid prefix)".to_string(),
    keeps_malformed_shorthand_as_literal_code_with_whitespace_in_the_id => "try(sample more,Invalid whitespace)".to_string(),
    keeps_malformed_shorthand_as_literal_code_with_a_quoted_id => "try(\"sample\",Quoted ID)".to_string(),
    keeps_malformed_shorthand_as_literal_code_with_an_arguments_payload => "try(sample?arguments,Invalid payload)".to_string(),
    keeps_malformed_shorthand_as_literal_code_with_an_oversized_id => format!("try({},Oversized ID)", "a".repeat(129)),
    keeps_malformed_shorthand_as_literal_code_with_trailing_code => "try(sample,Label) trailing code".to_string(),
    keeps_malformed_shorthand_as_literal_code_with_the_wrong_case => "Try(sample,Wrong case)".to_string(),
}

fn does_not_activate_shorthand_inside(markdown: &'static str) {
    case(move |fx| async move {
        let container = fx.render_document(&format!("{markdown}\n\n`try(other,Outside)`"), true).await.expect("renderReleaseNotesMarkdown");
        let links: Vec<(Option<String>, Option<String>)> = select_all(&container, &|element| element.get_attribute(TRYOUT_ID).is_some())
            .into_iter()
            .map(|element| (dataset(element, TRYOUT_ID), element.query_selector("a").map(|link| link.text_content().to_string_lossy())))
            .collect();
        assert_eq!(links, vec![(Some("other".to_string()), Some("Outside".to_string()))]);
        assert!(fx.runs().is_empty());
    });
}

macro_rules! shorthand_context_cases {
    ($($name:ident => $markdown:expr),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                does_not_activate_shorthand_inside($markdown);
            }
        )*
    };
}

shorthand_context_cases! {
    does_not_activate_shorthand_inside_bare_text => "try(sample,Literal)",
    does_not_activate_shorthand_inside_fenced_code => "```text\n`try(sample,Literal)`\n```",
    does_not_activate_shorthand_inside_indented_code => "    `try(sample,Literal)`",
    does_not_activate_shorthand_inside_a_larger_code_span => "`const example = try(sample,Literal)`",
    does_not_activate_shorthand_inside_a_markdown_link => "[`try(sample,Literal)`](https://example.com)",
    does_not_activate_shorthand_inside_nested_link_formatting => "[**`try(sample,Literal)`**](https://example.com)",
    does_not_activate_shorthand_inside_a_reference_link => "[`try(sample,Literal)`][example]\n\n[example]: https://example.com",
    does_not_activate_shorthand_inside_an_html_link => "<a href=\"https://example.com\">`try(sample,Literal)`</a>",
    does_not_activate_shorthand_inside_an_html_code_element => "<code>`try(sample,Literal)`</code>",
    does_not_activate_shorthand_inside_a_plain_html_code_example => "<code>try(sample,Literal)</code>",
    does_not_activate_shorthand_inside_an_image => "![`try(sample,Literal)`](https://example.com/image.png)",
}

#[test]
fn accepts_the_existing_shorthand_id_alphabet_and_length_boundary() {
    case(|fx| async move {
        let ids = ["a".to_string(), "A.b_C-9".to_string(), "a".repeat(128)];
        for id in &ids {
            fx.service.register(id, |_| {});
        }
        let hrefs: Vec<String> = ids
            .iter()
            .map(|id| {
                let html = fx.tryouts.render_code_span(&format!("try({id},Boundary)")).expect("o ID é aceito");
                html.split("href=\"").nth(1).and_then(|rest| rest.split('"').next()).expect("o link tem href").replace("&amp;", "&")
            })
            .collect();
        assert_eq!(hrefs, ids.iter().map(|id| tryout_href(id)).collect::<Vec<_>>());
    });
}

#[test]
fn keeps_an_unknown_shorthand_id_readable_and_inert() {
    case(|fx| async move {
        let html = fx.tryouts.render_code_span("try(missing,Readable fallback)").expect("o atalho é válido");
        let href = html.split("href=\"").nth(1).and_then(|rest| rest.split('"').next()).expect("o link tem href").replace("&amp;", "&");
        assert_eq!(
            fx.tryouts.render_link(&href, "Readable fallback", true),
            RenderedLink::Fallback("Readable fallback (This example is not available in this version of VS Code.)".to_string())
        );
        assert!(fx.runs().is_empty());
    });
}

#[test]
fn renders_ready_links_with_only_the_registered_id_and_local_contextual_metadata() {
    case(|fx| async move {
        fx.service.register("sample", |metadata| {
            metadata.is_ai = Some(true);
            metadata.target_window = Some(OnboardingTryoutTargetWindow::Agents);
        });
        let state = tryout_state(fx.render(&tryout_href("sample"), "Fetched label"));
        assert_eq!(state.label, "Try This: Local Example");
        assert_eq!(state.href, tryout_href("sample"));
        assert_eq!(
            state.aria_label,
            "Try This: Local Example A local description. Opens in the Agents window. Chat examples are prepared for review and are not sent automatically."
        );
        assert_eq!(state.kind.as_str(), "ready");
        assert!(fx.runs().is_empty());
        assert_eq!(fx.command_count(), 0);
    });
}

#[test]
fn a_failing_availability_provider_leaves_ordinary_content_and_other_examples_usable() {
    case(|fx| async move {
        fx.service.broken.borrow_mut().insert("sample".to_string());
        let sample = tryout_state(fx.render(&tryout_href("sample"), "Try"));
        let other = tryout_state(fx.render(&tryout_href("other"), "Other"));
        let settings = fx.render("command:workbench.action.openSettings", "Settings");
        fx.attach();
        fx.service.changes.fire(());
        fx.request("run", vec![]);
        timeout(0).await;

        assert_eq!((sample.href.as_str(), sample.kind.as_str()), ("", "unavailable"));
        assert_eq!((other.href, other.kind.as_str()), (tryout_href("other"), "ready"));
        assert_eq!(settings, RenderedLink::Unchanged);
        assert_eq!(sample.message, "This feature example could not be loaded.");
        assert!(fx.log.errors.get() > 0);
        assert!(fx.runs().is_empty());
    });
}

fn hides_ai_affordances_and_labels_without_removing_their_update_targets(shorthand: bool) {
    case(move |fx| async move {
        fx.service.register("sample", |metadata| {
            metadata.title = "Hidden AI Example".to_string();
            metadata.is_ai = Some(true);
        });
        fx.service.set_availability("sample", OnboardingTryoutAvailability::Hidden);
        let link = if shorthand { fx.render_shorthand("try(sample,Hidden authored label)") } else { fx.render(&tryout_href("sample"), "Fetched label") };
        let state = tryout_state(link);
        assert_eq!(state.kind.as_str(), "hidden");
        assert_eq!((state.label.as_str(), state.href.as_str(), state.message.as_str()), ("", "", ""));
        let leaked = format!("{state:?}");
        assert!(!leaked.contains("Hidden AI Example"));
        assert!(!leaked.contains("Hidden authored label"));
        assert!(fx.runs().is_empty());
    });
}

#[test]
fn hides_legacy_ai_affordances_and_labels_without_removing_their_update_targets() {
    hides_ai_affordances_and_labels_without_removing_their_update_targets(false);
}

#[test]
fn hides_shorthand_ai_affordances_and_labels_without_removing_their_update_targets() {
    hides_ai_affordances_and_labels_without_removing_their_update_targets(true);
}

fn renders_an_unavailable_explanation(setup: bool) {
    case(move |fx| async move {
        fx.service.set_availability(
            "sample",
            unavailable("Open a workspace first.", setup.then(|| ("Open Workspace", "local.setup", vec![Dynamic::string("local-only")]))),
        );
        let state = tryout_state(fx.render(&tryout_href("sample"), "Fetched label"));
        assert_eq!(state.href, "");
        assert_eq!(state.kind.as_str(), "unavailable");
        assert_eq!(state.message, "Open a workspace first.");
        assert_eq!(state.setup_label.is_empty(), !setup);
        assert_eq!(state.setup_label, if setup { "Open Workspace" } else { "" });
        let exposed = format!("{state:?}");
        assert!(!exposed.contains("local.setup") && !exposed.contains("local-only"));
    });
}

#[test]
fn renders_an_unavailable_explanation_without_a_separate_setup_action() {
    renders_an_unavailable_explanation(false);
}

#[test]
fn renders_an_unavailable_explanation_with_a_separate_setup_action() {
    renders_an_unavailable_explanation(true);
}

fn keeps_a_readable_inert_fallback(uri: URI) {
    case(move |fx| async move {
        match fx.render(&uri_text(&uri), "Readable fallback") {
            RenderedLink::Fallback(fallback) => {
                assert!(fallback.contains("Readable fallback"));
                assert!(fallback.contains("not available") || fallback.contains("invalid"), "sem explicação: {fallback}");
            }
            other => panic!("esperava o fallback inerte, veio {other:?}"),
        }
    });
}

fn sample_uri() -> URI {
    create_onboarding_tryout_uri("sample").expect("o ID é válido")
}

fn sample_uri_with(change: UriChange) -> URI {
    sample_uri().with(&change)
}

#[test]
fn keeps_a_readable_inert_fallback_for_unknown_id() {
    keeps_a_readable_inert_fallback(create_onboarding_tryout_uri("missing").expect("o ID é válido"));
}

#[test]
fn keeps_a_readable_inert_fallback_for_extra_argument() {
    keeps_a_readable_inert_fallback(command_uri(&[Dynamic::string("sample"), Dynamic::string("injected")]));
}

#[test]
fn keeps_a_readable_inert_fallback_for_object_argument() {
    keeps_a_readable_inert_fallback(command_uri(&[Dynamic::object([("id", Dynamic::string("sample"))])]));
}

#[test]
fn keeps_a_readable_inert_fallback_for_no_argument() {
    keeps_a_readable_inert_fallback(command_uri(&[]));
}

#[test]
fn keeps_a_readable_inert_fallback_for_empty_id() {
    keeps_a_readable_inert_fallback(command_uri(&[Dynamic::string("")]));
}

#[test]
fn keeps_a_readable_inert_fallback_for_whitespace_id() {
    keeps_a_readable_inert_fallback(command_uri(&[Dynamic::string(" sample")]));
}

#[test]
fn keeps_a_readable_inert_fallback_for_newline_id() {
    keeps_a_readable_inert_fallback(command_uri(&[Dynamic::string("sample\n")]));
}

#[test]
fn keeps_a_readable_inert_fallback_for_oversized_id() {
    keeps_a_readable_inert_fallback(command_uri(&[Dynamic::string(&"a".repeat(129))]));
}

#[test]
fn keeps_a_readable_inert_fallback_for_invalid_json() {
    keeps_a_readable_inert_fallback(sample_uri_with(UriChange { query: Field::from("%not-json"), ..UriChange::default() }));
}

#[test]
fn keeps_a_readable_inert_fallback_for_non_array_json() {
    keeps_a_readable_inert_fallback(sample_uri_with(UriChange { query: Field::from("\"sample\""), ..UriChange::default() }));
}

#[test]
fn keeps_a_readable_inert_fallback_for_fragment() {
    keeps_a_readable_inert_fallback(sample_uri_with(UriChange { fragment: Field::from("extra"), ..UriChange::default() }));
}

#[test]
fn accepts_the_id_length_boundary_without_treating_prefixes_as_registered_ids() {
    case(|fx| async move {
        let id = "a".repeat(128);
        fx.service.register(&id, |_| {});
        let accepted = fx.render(&tryout_href(&id), "Try");
        let prefix = fx.render(&tryout_href(&format!("{}b", &id[..127])), "No");
        assert_eq!(tryout_state(accepted).href, tryout_href(&id));
        assert!(matches!(prefix, RenderedLink::Fallback(_)));
    });
}

#[test]
fn tryouts_is_a_renderer_capability_and_preserves_legacy_and_quality_fallbacks() {
    let link = format!("[Try]({})", tryout_href("sample"));
    let markdown = format!(
        "Before\n<!-- %IF TRYOUTS %\n{link}\n%ENDIF % -->\n<!-- %IF STABLE %\nStable fallback\n%ENDIF % -->\n<!-- %IF INSIDERS %\nInsiders only\n%ENDIF % -->\nAfter"
    );
    let legacy = prepare_release_notes_markdown(&markdown, Some("stable"), false);
    let integrated = prepare_release_notes_markdown(&markdown, Some("stable"), true);
    assert!(!legacy.contains(&link));
    assert!(legacy.contains("Stable fallback"));
    assert!(integrated.contains(&link));
    assert!(integrated.contains("Stable fallback"));
    assert!(!integrated.contains("Insiders only"));
}

#[test]
fn escapes_local_text_and_sanitizes_fetched_html_before_recognizing_raw_links() {
    case(|fx| async move {
        fx.service.register("sample", |metadata| {
            metadata.title = "<img src=x onerror=alert(1)>".to_string();
            metadata.description = "\"<script>alert(2)</script>&".to_string();
        });
        fx.service.set_availability("sample", unavailable("<svg onload=alert(3)>", Some(("<img onerror=alert(4)>", "local.setup", vec![]))));
        let state = tryout_state(fx.render(&tryout_href("sample"), "Try"));
        assert_eq!(fx.render("javascript:alert(7)", "Bad"), RenderedLink::Unchanged);
        assert_eq!(state.id, "sample");
        assert_eq!(state.label, "Try This: <img src=x onerror=alert(1)>");
        assert_eq!(state.message, "<svg onload=alert(3)>");
        assert_eq!(state.setup_label, "<img onerror=alert(4)>");
    });
}

#[test]
fn preserves_toc_setting_chips_command_links_and_media_sanitization_with_the_integration() {
    case(|fx| async move {
        *fx.preferences.setting.borrow_mut() = Some(ISetting {
            key: "editor.wordWrap".to_string(),
            value: Dynamic::string("off"),
            r#type: Some(SettingSchemaType::Single("string".to_string())),
            ..ISetting::default()
        });
        let markdown = "<!-- TOC\n<nav id=\"toc-nav\"><a href=\"#heading\">Heading</a></nav>\nNavigation End -->\n## Heading\n[Settings](command:workbench.action.openSettings) [Summary](command:summarize.release.notes)\n`setting(editor.wordWrap:on)`\n<svg viewBox=\"0 0 16 16\"><path fill=\"currentColor\" d=\"M0 0\"></path></svg><img src=\"https://example.com/image.png\" onerror=\"alert(1)\">";
        let legacy = fx.render_document(markdown, false).await.expect("renderReleaseNotesMarkdown");
        let integrated = fx.render_document(markdown, true).await.expect("renderReleaseNotesMarkdown");
        assert_eq!(inner_html(&integrated).to_string_lossy(), inner_html(&legacy).to_string_lossy());
    });
}

#[test]
fn skips_tryout_dom_processing_when_sanitized_notes_contain_no_tryout_links() {
    case(|fx| async move {
        let notes = "<h1>Ordinary notes</h1>\n<p><a href=\"command:workbench.action.openSettings\">Settings</a></p>\n<img src=\"https://example.com/image.png\">\n<pre><code class=\"language-text\">command:workbench.action.onboarding.tryFeature\n</code></pre>";
        assert!(!fx.tryouts.needs_render(notes));
        assert_eq!(fx.render("command:workbench.action.openSettings", "Settings"), RenderedLink::Unchanged);
    });
}

#[test]
fn still_removes_forged_tryout_attributes_when_there_are_no_tryout_links() {
    case(|fx| async move {
        let forged = "<span data-release-notes-tryout-id=\"sample\" data-release-notes-tryout-index=\"0\"><a href=\"command:workbench.action.openSettings\">Settings</a></span>";
        assert!(fx.tryouts.needs_render(forged));
        assert!(fx.tryouts.needs_render("<span DATA-release-notes-tryout-index = \"0\"></span>"));
        assert!(!fx.tryouts.needs_render("<span class=\"data-release-notes-tryout-id\">x</span>"));
        assert!(fx.tryouts.needs_render(&format!("<a HREF=\"{}\">Try</a>", tryout_href("sample"))));
        assert!(fx.tryouts.needs_render(&format!("<a href=\"{}\">Bad</a>", uri_text(&command_uri(&[Dynamic::string("sample"), Dynamic::string("injected")])))));
        assert_eq!(fx.render("command:workbench.action.openSettings", "Settings"), RenderedLink::Unchanged);
        assert!(fx.runs().is_empty());
    });
}

fn recognizes_a_tryout_link_with_an_encoded_command_id(href: String) {
    case(move |fx| async move {
        let state = tryout_state(fx.render(&href, "Fetched label"));
        assert_eq!(state.id, "sample");
        assert_eq!(state.label, "Try This: Local Example");
    });
}

#[test]
fn recognizes_a_tryout_link_with_percent_encoded_workbench_in_its_encoded_command_id() {
    recognizes_a_tryout_link_with_an_encoded_command_id(tryout_href("sample").replacen("workbench", "%77orkbench", 1));
}

#[test]
fn recognizes_a_tryout_link_with_an_html_entity_for_workbench_in_its_encoded_command_id() {
    recognizes_a_tryout_link_with_an_encoded_command_id(tryout_href("sample"));
}

fn state_dynamic(index: usize, kind: &str, label: &str, aria_label: &str, href: &str, message: &str, setup_label: &str, setup_aria_label: &str) -> Dynamic {
    Dynamic::object([
        ("id", Dynamic::string("sample")),
        ("index", Dynamic::Number(index as f64)),
        ("kind", Dynamic::string(kind)),
        ("label", Dynamic::string(label)),
        ("ariaLabel", Dynamic::string(aria_label)),
        ("href", Dynamic::string(href)),
        ("message", Dynamic::string(message)),
        ("setupLabel", Dynamic::string(setup_label)),
        ("setupAriaLabel", Dynamic::string(setup_aria_label)),
    ])
}

#[test]
fn posts_only_changed_occurrences_and_no_compiled_commands_on_availability_changes() {
    case(|fx| async move {
        fx.render(&tryout_href("sample"), "Try");
        fx.render(&tryout_href("other"), "Other");
        fx.render(&tryout_href("sample"), "Again");
        fx.attach();
        fx.service.changes.fire(());
        fx.service.set_availability("sample", unavailable("Sign in first.", Some(("Sign In", "local.signIn", vec![Dynamic::string("secret-local")]))));
        fx.service.changes.fire(());
        fx.service.changes.fire(());
        let states = [0, 2].map(|index| {
            state_dynamic(
                index,
                "unavailable",
                "Try This: Local Example",
                "Try This: Local Example A local description. Sign in first.",
                "",
                "Sign in first.",
                "Sign In",
                "Sign In for Local Example",
            )
        });
        let expected = Dynamic::array([Dynamic::object([
            ("type", Dynamic::string("releaseNotesTryouts")),
            ("documentId", Dynamic::string(&fx.tryouts.document_id)),
            ("states", Dynamic::array(states)),
        ])]);
        assert_deep_strict_equal(&fx.messages(), &expected);
    });
}

/// O que o documento mostra de um link depois de aplicar as mensagens: rótulo, nome acessível e `kind`.
#[derive(Clone)]
struct LinkView {
    label: String,
    aria_label: String,
    kind: String,
}

impl LinkView {
    fn from_state(state: &ReleaseNotesTryoutState) -> Self {
        Self { label: state.label.clone(), aria_label: state.aria_label.clone(), kind: state.kind.as_str().to_string() }
    }

    /// `[label, ariaLabel, aria-disabled, hidden]`.
    fn snapshot(&self) -> (String, String, &'static str, bool) {
        (self.label.clone(), self.aria_label.clone(), if self.kind == "ready" { "false" } else { "true" }, self.kind == "hidden")
    }
}

/// O `window.dispatchEvent(new MessageEvent('message', ...))` do documento: aplica os estados postados.
fn apply_posted_states(fx: &Fixture, views: &mut [LinkView]) {
    let messages: Vec<Dynamic> = fx.webview.messages.borrow_mut().drain(..).collect();
    for message in messages {
        if text(&message.get_str("type")) != "releaseNotesTryouts" {
            continue;
        }
        let Dynamic::Array(states) = message.get_str("states") else { panic!("esperava a lista de estados") };
        for state in states.to_vec() {
            let Dynamic::Number(index) = state.get_str("index") else { panic!("esperava o índice") };
            views[index as usize] = LinkView {
                label: text(&state.get_str("label")),
                aria_label: text(&state.get_str("ariaLabel")),
                kind: text(&state.get_str("kind")),
            };
        }
    }
}

#[test]
fn preserves_distinct_shorthand_and_legacy_labels_through_availability_changes() {
    case(|fx| async move {
        let first = tryout_state(fx.render_shorthand("try(sample,First Example)"));
        let second = tryout_state(fx.render_shorthand("try(sample,Second Example)"));
        let legacy = tryout_state(fx.render(&tryout_href("sample"), "Legacy"));
        fx.attach();
        let mut views = vec![LinkView::from_state(&first), LinkView::from_state(&second), LinkView::from_state(&legacy)];
        let snapshot = |views: &[LinkView]| views.iter().map(LinkView::snapshot).collect::<Vec<_>>();
        let mut update = |fx: &Fixture, availability: OnboardingTryoutAvailability| {
            fx.service.set_availability("sample", availability);
            fx.service.changes.fire(());
            apply_posted_states(fx, &mut views);
            snapshot(&views)
        };
        let initial = {
            let initial = vec![LinkView::from_state(&first), LinkView::from_state(&second), LinkView::from_state(&legacy)];
            snapshot(&initial)
        };
        let setup_required = update(&fx, unavailable("Setup required.", None));
        let hidden = update(&fx, OnboardingTryoutAvailability::Hidden);
        let ready_again = update(&fx, OnboardingTryoutAvailability::Ready);

        let labels = ["First Example", "Second Example", "Try This: Local Example"];
        let names = [
            "First Example Local Example A local description.",
            "Second Example Local Example A local description.",
            "Try This: Local Example A local description.",
        ];
        let ready: Vec<_> = (0..3).map(|index| (labels[index].to_string(), names[index].to_string(), "false", false)).collect();
        assert_eq!(initial, ready);
        assert_eq!(
            setup_required,
            (0..3).map(|index| (labels[index].to_string(), format!("{} Setup required.", names[index]), "true", false)).collect::<Vec<_>>()
        );
        assert_eq!(hidden, (0..3).map(|_| (String::new(), String::new(), "true", true)).collect::<Vec<_>>());
        assert_eq!(ready_again, ready);
        assert!(fx.runs().is_empty());
        assert_eq!(fx.command_count(), 0);
    });
}

#[test]
fn activates_the_selected_shorthand_occurrence_with_release_note_attribution() {
    case(|fx| async move {
        fx.render_shorthand("try(sample,First Example)");
        fx.render_shorthand("try(sample,Second Example)");
        fx.attach();
        fx.request("run", vec![("index", Dynamic::Number(1.0))]);
        timeout(0).await;
        assert_eq!(fx.runs(), vec!["sample".to_string()]);
        assert_eq!(*fx.service.options.borrow(), release_notes_options());
        assert_eq!(fx.command_count(), 0);
    });
}

#[test]
fn revalidates_primary_actions_even_when_availability_changes_without_a_notification() {
    case(|fx| async move {
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        fx.service.set_availability("sample", unavailable("No longer available.", None));
        fx.request("run", vec![]);
        timeout(0).await;
        assert!(fx.runs().is_empty());
        assert_eq!(fx.command_count(), 0);
        assert_eq!(fx.notifications().first().map(String::as_str), Some("No longer available."));
        assert_eq!(fx.webview.focus_count.get(), 1);
    });
}

#[test]
fn runs_ready_examples_only_after_an_explicit_validated_activation() {
    case(|fx| async move {
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        fx.request("run", vec![]);
        timeout(0).await;
        assert_eq!(fx.runs(), vec!["sample".to_string()]);
        assert_eq!(*fx.service.options.borrow(), release_notes_options());
        assert_eq!(fx.command_count(), 0);
        assert_eq!(fx.webview.focus_count.get(), 0);
    });
}

#[test]
fn coalesces_repeated_activation_without_cancelling_the_pending_example() {
    case(|fx| async move {
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        let pending = Rc::new(DeferredPromise::<OnboardingTryoutResult>::new());
        let token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));
        {
            let (pending, token) = (pending.clone(), token.clone());
            fx.set_run_implementation(move |_, source_token| {
                *token.borrow_mut() = Some(source_token);
                pending.promise.clone()
            });
        }
        fx.request("run", vec![]);
        fx.request("run", vec![]);
        let cancelled_before_completion = token.borrow().as_ref().map(CancellationToken::is_cancellation_requested);
        pending.complete(opened()).await.unwrap();
        timeout(0).await;
        assert_eq!(fx.runs(), vec!["sample".to_string()]);
        assert_eq!(cancelled_before_completion, Some(false));
        assert_eq!(fx.webview.focus_count.get(), 0);
    });
}

#[test]
fn resolves_the_setup_command_and_arguments_afresh_and_never_runs_the_example_afterwards() {
    case(|fx| async move {
        fx.service.set_availability("sample", unavailable("Setup needed.", Some(("Set Up", "old.setup", vec![Dynamic::string("old")]))));
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        fx.service.set_availability(
            "sample",
            unavailable("Setup needed.", Some(("Set Up", "current.setup", vec![Dynamic::string("compiled"), Dynamic::Number(42.0)]))),
        );
        fx.request("setup", vec![]);
        timeout(0).await;
        let calls = fx.commands.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "current.setup");
        assert_deep_strict_equal(&Dynamic::array(calls[0].1.clone()), &Dynamic::array([Dynamic::string("compiled"), Dynamic::Number(42.0)]));
        assert!(fx.runs().is_empty());
        assert_eq!(fx.webview.focus_count.get(), 0);
    });
}

#[test]
fn does_not_run_stale_setup_actions_after_an_example_becomes_ready_or_hidden() {
    case(|fx| async move {
        fx.service.set_availability("sample", unavailable("Setup needed.", Some(("Set Up", "local.setup", vec![]))));
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        fx.service.set_availability("sample", OnboardingTryoutAvailability::Ready);
        fx.request("setup", vec![]);
        timeout(0).await;
        fx.service.set_availability("sample", OnboardingTryoutAvailability::Hidden);
        fx.request("setup", vec![]);
        timeout(0).await;
        assert!(fx.runs().is_empty());
        assert_eq!(fx.command_count(), 0);
    });
}

fn rejects_forged_protocol_data(extra: Vec<(&'static str, Dynamic)>) {
    case(move |fx| async move {
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        fx.request("setup", extra);
        timeout(0).await;
        assert!(fx.runs().is_empty());
        assert_eq!(fx.command_count(), 0);
        assert_eq!(fx.notifications().len(), 1);
    });
}

#[test]
fn rejects_forged_protocol_data_with_a_command() {
    rejects_forged_protocol_data(vec![(
        "command",
        Dynamic::object([("id", Dynamic::string("injected.command")), ("arguments", Dynamic::array([Dynamic::string("bad")]))]),
    )]);
}

#[test]
fn rejects_forged_protocol_data_with_arguments() {
    rejects_forged_protocol_data(vec![("arguments", Dynamic::array([Dynamic::string("bad")]))]);
}

#[test]
fn rejects_forged_protocol_data_with_another_registered_id() {
    rejects_forged_protocol_data(vec![("id", Dynamic::string("other"))]);
}

#[test]
fn rejects_forged_protocol_data_with_a_missing_id() {
    rejects_forged_protocol_data(vec![("id", Dynamic::string("missing"))]);
}

#[test]
fn rejects_forged_protocol_data_with_a_padded_id() {
    rejects_forged_protocol_data(vec![("id", Dynamic::string("sample ")), ("index", Dynamic::Number(0.0))]);
}

#[test]
fn rejects_forged_protocol_data_with_a_negative_index() {
    rejects_forged_protocol_data(vec![("index", Dynamic::Number(-1.0))]);
}

#[test]
fn rejects_forged_protocol_data_with_an_arbitrary_action() {
    rejects_forged_protocol_data(vec![("action", Dynamic::string("arbitrary"))]);
}

#[test]
fn ignores_messages_from_an_earlier_document_in_a_reused_webview() {
    case(|fx| async move {
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        fx.request("run", vec![("documentId", Dynamic::string("previous-document"))]);
        timeout(0).await;
        assert!(fx.runs().is_empty());
        assert!(fx.webview.messages.borrow().is_empty());
        assert!(fx.notifications().is_empty());
        assert_eq!(fx.webview.focus_count.get(), 0);
    });
}

#[test]
fn silently_ignores_malformed_or_unknown_direct_command_links_without_falling_through_to_arbitrary_dispatch() {
    case(|fx| async move {
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        fx.tryouts.open_link(&command_uri(&[Dynamic::string("sample"), Dynamic::string("injected")])).await.unwrap();
        fx.tryouts.open_link(&create_onboarding_tryout_uri("missing").unwrap()).await.unwrap();
        assert!(fx.runs().is_empty());
        assert_eq!(fx.command_count(), 0);
        assert!(fx.notifications().is_empty());
    });
}

fn restores_source_focus_for_a_result(kind: &'static str) {
    case(move |fx| async move {
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        fx.set_run_implementation(move |_, _| match kind {
            "failure" => Promise::rejected(Error::new("Could not prepare the example.")),
            "unavailable" => Promise::resolved(OnboardingTryoutResult::Unavailable(OnboardingTryoutUnavailable {
                message: "Target closed.".to_string(),
                action: None,
            })),
            _ => Promise::resolved(OnboardingTryoutResult::Cancelled),
        });
        fx.request("run", vec![]);
        timeout(0).await;
        let expected = Dynamic::array([Dynamic::object([
            ("type", Dynamic::string("releaseNotesTryoutFocus")),
            ("documentId", Dynamic::string(&fx.tryouts.document_id)),
            ("index", Dynamic::Number(0.0)),
            ("action", Dynamic::string("run")),
        ])]);
        assert_eq!(fx.webview.focus_count.get(), 1);
        assert_deep_strict_equal(&fx.messages(), &expected);
        assert_eq!(fx.notifications().len(), if kind == "cancelled" { 0 } else { 1 });
    });
}

#[test]
fn restores_source_focus_for_a_cancelled_result() {
    restores_source_focus_for_a_result("cancelled");
}

#[test]
fn restores_source_focus_for_a_unavailable_result() {
    restores_source_focus_for_a_result("unavailable");
}

#[test]
fn restores_source_focus_for_a_failure_result() {
    restores_source_focus_for_a_result("failure");
}

#[test]
fn does_not_reclaim_focus_after_the_user_leaves_the_source_editor() {
    case(|fx| async move {
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        fx.can_restore_focus.set(false);
        fx.set_run_implementation(|_, _| Promise::resolved(OnboardingTryoutResult::Cancelled));
        fx.request("run", vec![]);
        timeout(0).await;
        assert_eq!(fx.webview.focus_count.get(), 0);
        assert!(fx.webview.messages.borrow().is_empty());
    });
}

#[test]
fn cancels_pending_preparation_on_webview_disposal_and_suppresses_all_late_effects() {
    case(|fx| async move {
        fx.render(&tryout_href("sample"), "Fetched label");
        fx.attach();
        let pending = Rc::new(DeferredPromise::<OnboardingTryoutResult>::new());
        let token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));
        {
            let (pending, token) = (pending.clone(), token.clone());
            fx.set_run_implementation(move |_, source_token| {
                *token.borrow_mut() = Some(source_token);
                pending.promise.clone()
            });
        }
        fx.request("run", vec![]);
        fx.webview.on_dispose.fire(());
        fx.service.changes.fire(());
        pending
            .complete(OnboardingTryoutResult::Unavailable(OnboardingTryoutUnavailable { message: "Late result.".to_string(), action: None }))
            .await
            .unwrap();
        timeout(0).await;
        assert_eq!(token.borrow().as_ref().map(CancellationToken::is_cancellation_requested), Some(true));
        assert!(fx.webview.messages.borrow().is_empty());
        assert!(fx.notifications().is_empty());
        assert_eq!(fx.webview.focus_count.get(), 0);
    });
}

#[test]
fn updates_controls_in_place_without_executing_on_focus_or_hover() {
    case(|fx| async move {
        let initial = tryout_state(fx.render(&tryout_href("sample"), "Fetched label"));
        fx.attach();
        fx.service.set_availability("sample", unavailable("A new explanation.", None));
        fx.service.changes.fire(());
        let messages = fx.webview.messages.borrow().clone();
        assert_eq!(messages.len(), 1);
        let Dynamic::Array(states) = messages[0].get_str("states") else { panic!("esperava a lista de estados") };
        let state = states.get(0);
        assert_eq!(initial.href, tryout_href("sample"));
        assert_eq!(state.get_str("index").strict_equals(&Dynamic::Number(0.0)), true);
        assert_eq!(text(&state.get_str("href")), "");
        assert_eq!(text(&state.get_str("message")), "A new explanation.");
        assert!(fx.runs().is_empty());
    });
}

#[test]
fn serialized_webview_script_sends_only_the_document_occurrence_id_and_action_on_activation() {
    case(|fx| async move {
        fx.service.set_availability("sample", unavailable("Setup needed.", Some(("Set Up", "local.setup", vec![]))));
        let container = fx
            .render_document(&format!("`try(sample,Set Up Example)` [Ready]({})", tryout_href("other")), true)
            .await
            .expect("renderReleaseNotesMarkdown");
        let client_messages: Rc<RefCell<Vec<Dynamic>>> = Rc::new(RefCell::new(Vec::new()));
        let client = {
            let client_messages = client_messages.clone();
            ReleaseNotesTryoutsClient::initialize(&fx.tryouts.document_id, Rc::new(move |message| client_messages.borrow_mut().push(message)))
        };
        let button = path_to(&container, &|element| element.tag_name == "button").expect("o controle tem o botão de preparação");
        let link = path_to(&container, &|element| element.tag_name == "a" && element.get_attribute("href").is_some()).expect("há um link com href");
        assert!(client.click(&container, &button));
        assert!(client.click(&container, &link));
        client.dispose();

        let message = |kind: &str, id: &str, index: f64, action: &str| {
            Dynamic::object([
                ("type", Dynamic::string(kind)),
                ("documentId", Dynamic::string(&fx.tryouts.document_id)),
                ("id", Dynamic::string(id)),
                ("index", Dynamic::Number(index)),
                ("action", Dynamic::string(action)),
            ])
        };
        let expected = Dynamic::array([
            Dynamic::object([("type", Dynamic::string("releaseNotesTryoutsReady")), ("documentId", Dynamic::string(&fx.tryouts.document_id))]),
            message("releaseNotesTryout", "sample", 0.0, "setup"),
            message("releaseNotesTryout", "other", 1.0, "run"),
        ]);
        assert_deep_strict_equal(&Dynamic::array(client_messages.borrow().iter().cloned()), &expected);
    });
}
