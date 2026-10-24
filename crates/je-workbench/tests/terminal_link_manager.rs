//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalLinkManager.test.ts` (suíte `TerminalLinkManager`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo, o
//!   `TestConfigurationService` (com `editor` e `terminal.integrated`), o `TerminalConfigurationService` real sobre ele, o
//!   `FakeXterm` (80 colunas por 30 linhas) no lugar do `Terminal` do xterm, o `UriIdentityService` real, o `TestContextService`, o
//!   `TerminalLinkResolver` real e o [`TestLinkManager`]. Os abridores de link (que o original cria no construtor e este teste
//!   nunca usa) não entram: o gerenciador recebe a lista vazia.
//! - **`class TestLinkManager extends TerminalLinkManager`.** Sem herança em Rust, é o gerenciador real com a sobrescrita de
//!   `_getLinksForType` instalada por `set_links_for_type_override` (o `setLinks` guarda o `IDetectedLinks` num `RefCell`).
//! - **`upcastPartial<ITerminalProcessInfo>({ get initialCwd() { return ''; } })` e o store de capacidades vazio.** São o
//!   `PartialProcessInfo` (`tests/support/terminal_contrib_support.rs`) e o `TerminalCapabilityStore` real sem capacidades (o
//!   `get` do original devolve `undefined`).
//! - **`overrideXtermEvent(terminal, 'onScroll' | 'onRender', ...)` e `mockXtermCoreRenderService()`.** O original troca o evento do
//!   xterm por um que guarda o ouvinte, e o `_core._renderService.dimensions` por 8 por 16. Aqui o `FakeXterm` dispara o evento
//!   real (`fire_scroll` e `fire_render`) e tem as dimensões de célula (`set_css_cell_dimensions`).
//! - **`testableLinkManager._showHover = ...`.** É o ponto de substituição `set_show_hover_override`.
//! - **`new MouseEvent('mousemove')`.** É o `TerminalMouseEvent` do porte.
//! - **`runWithFakedTimers({ useFakeTimers: true }, ...)`.** É `run_with_faked_timers`, e `timeout(0)` é o do `common`.
//! - **`linkManager.dispose()` num gerenciador já descartado.** O guarda descarta o gerenciador de novo ao fim; o descarte é idempotente.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use je_instantiation::Injectable;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::html_content::{MarkdownString, TrustedOrOptions};
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, IDisposable, plain_disposable};
use je_base::promise::Promise;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::NullLogService;
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::terminal::browser::terminal::ITerminalConfigurationService;
use je_workbench::contrib::terminal::browser::terminal_configuration_service::TerminalConfigurationService;
use je_workbench::contrib::terminal::browser::terminal_extensions::{IXtermLink, IXtermRawTerminal};
use je_workbench::contrib::terminal::browser::terminal_input_events::TerminalMouseEvent;
use je_workbench::contrib::terminal::browser::widgets::terminal_hover_widget::{Dimensions, ILinkHoverTargetOptions};
use je_workbench::contrib::terminal::browser::widgets::widget_manager::ITerminalWidgetManager;
use je_workbench::contrib::terminal::browser::widgets::widgets::ITerminalWidget;
use je_workbench::contrib::terminal::xterm_buffer::{IBufferCellPosition, IBufferRange};
use je_workbench::contrib::terminal_contrib::links::terminal_link::IInvalidatableLink;
use je_workbench::contrib::terminal_contrib::links::terminal_link_manager::{
    IDetectedLinks, ITerminalLinkManager, LinkKind, RecentLinkKind, TerminalLinkManager, TerminalLinkManagerServices,
};
use je_workbench::contrib::terminal_contrib::links::terminal_link_resolver::TerminalLinkResolver;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestProductService};
use support::fake_xterm::FakeXterm;
use support::terminal_contrib_support::{NoHoverService, NoTunnelService, PanickingNotificationService, PartialProcessInfo};
use support::terminal_link_support::LinkTestFileService;

/// `defaultTerminalConfig` (os campos que o `TerminalConfigurationService` e o gerenciador leem).
fn default_terminal_config() -> Dynamic {
    Dynamic::object([
        ("fontFamily", Dynamic::string("monospace")),
        ("fontWeight", Dynamic::string("normal")),
        ("fontWeightBold", Dynamic::string("normal")),
        ("gpuAcceleration", Dynamic::string("off")),
        ("scrollback", Dynamic::Number(1000.0)),
        ("fastScrollSensitivity", Dynamic::Number(2.0)),
        ("mouseWheelScrollSensitivity", Dynamic::Number(1.0)),
        ("unicodeVersion", Dynamic::string("11")),
        ("wordSeparators", Dynamic::string(" ()[]{}',\"`─‘’“”")),
    ])
}

/// Um link do xterm com só `range`, `text` e `activate` (`{ range, text, activate: () => Promise.resolve('') }`).
struct TestLink {
    range: IBufferRange,
    text: String,
}

impl IXtermLink for TestLink {
    fn range(&self) -> IBufferRange {
        self.range
    }

    fn text(&self) -> String {
        self.text.clone()
    }

    fn activate(&self, _event: Option<&TerminalMouseEvent>, _text: &str) {}
}

fn test_link(start: (i32, i32), end: (i32, i32), text: &str) -> Rc<TestLink> {
    Rc::new(TestLink {
        range: IBufferRange { start: IBufferCellPosition { x: start.0, y: start.1 }, end: IBufferCellPosition { x: end.0, y: end.1 } },
        text: text.to_owned(),
    })
}

/// `strictEqual(a, b)` entre um link devolvido e o objeto do teste: a mesma identidade.
fn same_link(actual: &Rc<dyn IXtermLink>, expected: &Rc<TestLink>) -> bool {
    std::ptr::eq(Rc::as_ptr(actual).cast::<()>(), Rc::as_ptr(expected).cast::<()>())
}

fn links_text(links: &[Rc<dyn IXtermLink>]) -> Vec<String> {
    links.iter().map(|link| link.text()).collect()
}

/// O `setup` da suíte.
struct Fixture {
    configuration_service: Rc<TestConfigurationService>,
    xterm: Rc<FakeXterm>,
    link_manager: Rc<TerminalLinkManager>,
    /// `TestLinkManager#_links`.
    links: Rc<RefCell<IDetectedLinks>>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    guard: LeakGuard,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration_service = TestConfigurationService::new(Some(Dynamic::object([
            ("editor", Dynamic::object([("fastScrollSensitivity", Dynamic::Number(2.0)), ("mouseWheelScrollSensitivity", Dynamic::Number(1.0))])),
            ("terminal", Dynamic::object([("integrated", default_terminal_config())])),
        ])));
        // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
        guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
        let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();

        let configuration_instantiation = guard.add_rc(InstantiationService::new(ServiceCollection::new().with(I_CONFIGURATION_SERVICE, configuration.clone())));
        let terminal_configuration_service_concrete = guard.add_rc(configuration_instantiation.create_instance::<TerminalConfigurationService>(()));
        let terminal_configuration_service: Rc<dyn ITerminalConfigurationService> = terminal_configuration_service_concrete;

        let file_service = LinkTestFileService::new();
        let uri_identity_service = guard.add_rc(UriIdentityService::create((), (file_service.clone() as Rc<dyn IFileService>,)));
        let workspace_context_service = TestContextService::new(None, None);
        let link_resolver = Rc::new(TerminalLinkResolver::new(file_service));

        let xterm = guard.add_rc(FakeXterm::new(80, 30));
        let capabilities = guard.add_rc(TerminalCapabilityStore::new());
        let link_manager = guard.add_rc(TerminalLinkManager::new(
            xterm.clone() as Rc<dyn IXtermRawTerminal>,
            Rc::new(PartialProcessInfo { initial_cwd: String::new() }),
            capabilities,
            link_resolver,
            Vec::new(),
            TerminalLinkManagerServices {
                configuration_service: configuration,
                notification_service: Rc::new(PanickingNotificationService),
                telemetry_service: Rc::new(NullTelemetryServiceShape),
                terminal_configuration_service,
                log_service: NullLogService::shared(),
                tunnel_service: Rc::new(NoTunnelService),
                hover_service: Rc::new(NoHoverService),
                product_service: Rc::new(TestProductService::default()),
                uri_identity_service,
                workspace_context_service,
            },
        ));

        // `class TestLinkManager`: a sobrescrita de `_getLinksForType`.
        let links = Rc::new(RefCell::new(IDetectedLinks::default()));
        let links_for_override = links.clone();
        link_manager.set_links_for_type_override(Some(Rc::new(move |y: i32, kind: LinkKind| {
            let links = links_for_override.borrow();
            let list = match kind {
                LinkKind::Word => &links.word_links,
                LinkKind::Url => &links.web_links,
                LinkKind::LocalFile => &links.file_links,
                LinkKind::LocalFolder => return Promise::resolved(None),
            };
            Promise::resolved(list.as_ref().and_then(|list| usize::try_from(y).ok().and_then(|index| list.get(index))).map(|link| vec![link.clone()]))
        })));
        Self { configuration_service, xterm, link_manager, links, guard }
    }

    /// `setLinks(links)`.
    fn set_links(&self, links: IDetectedLinks) {
        *self.links.borrow_mut() = links;
    }
}

fn links_of(links: &[&Rc<TestLink>]) -> Option<Vec<Rc<dyn IXtermLink>>> {
    Some(links.iter().map(|link| (*link).clone() as Rc<dyn IXtermLink>).collect())
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('registerExternalLinkProvider')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn should_not_leak_disposables_if_the_link_manager_is_already_disposed() {
    let f = Fixture::new();
    f.link_manager.set_external_provide_links_cb(Some(Rc::new(|_line: &str| Promise::resolved(None))));
    f.link_manager.dispose();
    f.link_manager.set_external_provide_links_cb(Some(Rc::new(|_line: &str| Promise::resolved(None))));
    let _ = &f.guard;
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('OSC 8 hover')
// ---------------------------------------------------------------------------------------------------------------------

/// `{ start: { x: 1, y: 1 }, end: { x: 10, y: 1 } }`.
fn hover_range() -> IBufferRange {
    IBufferRange { start: IBufferCellPosition { x: 1, y: 1 }, end: IBufferCellPosition { x: 10, y: 1 } }
}

#[test]
fn should_cancel_delayed_tooltip_when_leave_happens_before_hover_delay() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.configuration_service.set_user_configuration("workbench.hover.delay", Dynamic::Number(10.0), None).await.unwrap();
        let link_handler = f.xterm.link_handler().expect("Expected linkHandler with hover/leave callbacks");
        let hover = link_handler.hover.clone().expect("Expected linkHandler with hover/leave callbacks");
        let leave = link_handler.leave.clone().expect("Expected linkHandler with hover/leave callbacks");
        let hover_shown_count = Rc::new(Cell::new(0));
        let counter = hover_shown_count.clone();
        f.link_manager.set_show_hover_override(Some(Rc::new(move |_options, _text, _actions, _handler, _link| {
            counter.set(counter.get() + 1);
            None
        })));
        let range = hover_range();
        let event = TerminalMouseEvent::new("mousemove");
        hover(&event, "http://example.com", &range);
        leave(&event, "http://example.com", &range);
        timeout(0).await.unwrap();
        assert_eq!(hover_shown_count.get(), 0);
    });
}

/// `assertHoverDismissedOnEvent(overrideEvent)`: dispara o hover, espera o agendador de 0 ms, dispara o evento do xterm e confere
/// que o hover foi descartado.
async fn assert_hover_dismissed_on_event(fire_event: impl FnOnce(&FakeXterm)) {
    let f = Fixture::new();
    f.configuration_service.set_user_configuration("workbench.hover.delay", Dynamic::Number(0.0), None).await.unwrap();
    let link_handler = f.xterm.link_handler().expect("Expected linkHandler with hover callback");
    let hover = link_handler.hover.clone().expect("Expected linkHandler with hover callback");
    let hover_disposed = Rc::new(Cell::new(false));
    let disposed = hover_disposed.clone();
    f.link_manager.set_show_hover_override(Some(Rc::new(move |_options, _text, _actions, _handler, _link| {
        let disposed = disposed.clone();
        Some(plain_disposable(move || disposed.set(true)))
    })));
    // `mockXtermCoreRenderService()`
    f.xterm.set_css_cell_dimensions(8.0, 16.0);
    let range = hover_range();
    hover(&TerminalMouseEvent::new("mousemove"), "http://example.com", &range);
    timeout(0).await.unwrap();
    assert!(!hover_disposed.get());
    fire_event(&f.xterm);
    assert!(hover_disposed.get());
}

#[test]
fn should_dismiss_shown_tooltip_on_scroll() {
    run_with_faked_timers(async {
        assert_hover_dismissed_on_event(|xterm| xterm.fire_scroll(1)).await;
    });
}

#[test]
fn should_dismiss_shown_tooltip_on_render() {
    run_with_faked_timers(async {
        assert_hover_dismissed_on_event(|xterm| xterm.fire_render(0, 5)).await;
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('link hover invalidation')
// ---------------------------------------------------------------------------------------------------------------------

/// `upcastPartial<TerminalLink>({ onInvalidated })`.
struct InvalidatableLink {
    on_invalidated: Rc<Emitter<()>>,
}

impl IInvalidatableLink for InvalidatableLink {
    fn on_invalidated(&self) -> Event<()> {
        self.on_invalidated.event()
    }
}

/// O gerenciador de widgets falso: registra o descarte de cada hover anexado e descarta o widget.
struct RecordingWidgetManager {
    disposed_attached: Rc<RefCell<Vec<bool>>>,
}

impl ITerminalWidgetManager for RecordingWidgetManager {
    fn attach_widget(&self, widget: Rc<dyn ITerminalWidget>) -> Option<DisposableRef> {
        let index = {
            let mut disposed_attached = self.disposed_attached.borrow_mut();
            disposed_attached.push(false);
            disposed_attached.len() - 1
        };
        let disposed_attached = self.disposed_attached.clone();
        Some(plain_disposable(move || {
            disposed_attached.borrow_mut()[index] = true;
            widget.dispose();
        }))
    }
}

#[test]
fn replacing_or_invalidating_a_link_hover_disposes_the_previous_hover_and_its_invalidation_listener() {
    let f = Fixture::new();

    // Fake widget manager that records disposal of each attached hover and disposes the widget
    let disposed_attached: Rc<RefCell<Vec<bool>>> = Rc::default();
    f.link_manager.set_widget_manager(Rc::new(RecordingWidgetManager { disposed_attached: disposed_attached.clone() }));

    let on_invalidated1 = f.guard.add_rc(Emitter::<()>::new());
    let on_invalidated2 = f.guard.add_rc(Emitter::<()>::new());
    let link1: Rc<dyn IInvalidatableLink> = Rc::new(InvalidatableLink { on_invalidated: on_invalidated1 });
    let link2: Rc<dyn IInvalidatableLink> = Rc::new(InvalidatableLink { on_invalidated: on_invalidated2.clone() });
    let target_options = ILinkHoverTargetOptions {
        viewport_range: hover_range(),
        cell_dimensions: Dimensions { width: 0.0, height: 0.0 },
        terminal_dimensions: Dimensions { width: 0.0, height: 0.0 },
        modifier_down_callback: None,
        modifier_up_callback: None,
    };
    let hover_text = || MarkdownString::new(Some(JsString::from("hover")), TrustedOrOptions::Trusted(false));

    // Showing a second link hover should dispose the first, then invalidating the second disposes it
    f.link_manager.show_hover(target_options.clone(), hover_text(), None, Rc::new(|_url: &str| {}), Some(link1));
    f.link_manager.show_hover(target_options, hover_text(), None, Rc::new(|_url: &str| {}), Some(link2));
    on_invalidated2.fire(());

    assert_eq!(*disposed_attached.borrow(), vec![true, true]);
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('getLinks and open recent link')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn should_return_no_links() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let links = f.link_manager.get_links().await.unwrap();
        assert!(links.viewport.web_links.is_empty());
        assert!(links.viewport.word_links.is_empty());
        assert!(links.viewport.file_links.is_empty());
        assert!(f.link_manager.open_recent_link(RecentLinkKind::Url).await.unwrap().is_none());
        assert!(f.link_manager.open_recent_link(RecentLinkKind::LocalFile).await.unwrap().is_none());
    });
}

#[test]
fn should_return_word_links_in_order() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let link1 = test_link((1, 1), (14, 1), "1_我是学生.txt");
        let link2 = test_link((1, 1), (14, 1), "2_我是学生.txt");
        f.set_links(IDetectedLinks { word_links: links_of(&[&link1, &link2]), ..IDetectedLinks::default() });
        let links = f.link_manager.get_links().await.unwrap();
        assert_eq!(links_text(&links.viewport.word_links), vec![link2.text.clone(), link1.text.clone()]);
        assert!(f.link_manager.open_recent_link(RecentLinkKind::Url).await.unwrap().is_none());
        assert!(f.link_manager.open_recent_link(RecentLinkKind::LocalFile).await.unwrap().is_none());
    });
}

#[test]
fn should_return_web_links_in_order() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let link1 = test_link((5, 1), (40, 1), "https://foo.bar/[this is foo site 1]");
        let link2 = test_link((5, 2), (40, 2), "https://foo.bar/[this is foo site 2]");
        f.set_links(IDetectedLinks { web_links: links_of(&[&link1, &link2]), ..IDetectedLinks::default() });
        let links = f.link_manager.get_links().await.unwrap();
        assert_eq!(links_text(&links.viewport.web_links), vec![link2.text.clone(), link1.text.clone()]);
        let web_link = f.link_manager.open_recent_link(RecentLinkKind::Url).await.unwrap().expect("o link web existe");
        assert!(same_link(&web_link, &link2));
        assert!(f.link_manager.open_recent_link(RecentLinkKind::LocalFile).await.unwrap().is_none());
    });
}

#[test]
fn should_return_file_links_in_order() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        let link1 = test_link((1, 1), (32, 1), "file:///C:/users/test/file_1.txt");
        let link2 = test_link((1, 2), (32, 2), "file:///C:/users/test/file_2.txt");
        f.set_links(IDetectedLinks { file_links: links_of(&[&link1, &link2]), ..IDetectedLinks::default() });
        let links = f.link_manager.get_links().await.unwrap();
        assert_eq!(links_text(&links.viewport.file_links), vec![link2.text.clone(), link1.text.clone()]);
        assert!(f.link_manager.open_recent_link(RecentLinkKind::Url).await.unwrap().is_none());
        f.set_links(IDetectedLinks { file_links: links_of(&[&link2]), ..IDetectedLinks::default() });
        let file_link = f.link_manager.open_recent_link(RecentLinkKind::LocalFile).await.unwrap().expect("o link de arquivo existe");
        assert!(same_link(&file_link, &link2));
    });
}
