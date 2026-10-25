//! Porte de `src/vs/workbench/contrib/externalUriOpener/test/common/externalUriOpenerService.test.ts` (suíte
//! `ExternalUriOpenerService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `DisposableStore` do
//!   `setup` com o `TestInstantiationService` é o próprio guarda: os serviços são montados direto, sem contêiner de
//!   injeção (os decoradores são argumentos de `ExternalUriOpenerService::new`).
//! - `instantiationService.stub(IConfigurationService, new TestConfigurationService())` é o `TestConfigurationService` da
//!   plataforma sem configuração, e `stub(IOpenerService, { registerExternalOpener: () => Disposable.None })` é o
//!   [`StubOpenerService`], cujos demais métodos falham com `panic!` (o `TypeError` do método ausente no literal).
//! - `MockQuickInputService` é a struct de mesmo nome: `pick` resolve o item do índice escolhido (ou `None` se for um
//!   separador). Nos casos que não fazem `stub(IQuickInputService, ...)` o índice é `None`, e qualquer `pick` falha com
//!   `panic!`, como o serviço que o contêiner não tem. Os demais métodos do `IQuickInputService` falham do mesmo modo.
//! - O `IPreferencesService` que o construtor pede e que nenhum caso usa é o [`MockPreferencesService`].
//! - Os `async *getOpeners` são openers numa lista (ver a divergência de `IExternalOpenerProvider::get_openers` no
//!   módulo de produção), e os literais de `IExternalUriOpener` são a struct [`TestOpener`], com `canOpen` como a
//!   prioridade fixa e `openExternalUri` como um fecho.
//! - `externalUriOpenerService.openExternal(href, ctx, token)` é `IExternalOpener::open_external`, e o `ViewColumn` do
//!   editor da plataforma é `ViewColumn::Beside`.
//! - `assert.deepStrictEqual({ didOpen, openedWithEnabled, viewColumn }, {...})` vira `assert_eq!` sobre a tupla.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{self, DisposableRef};
use je_base::observable::base::IObservable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::languages::ExternalUriOpenerPriority;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_platform::opener::editor_types::ViewColumn;
use je_platform::opener::{
    ExternalOpenerContext, IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource,
    ResolveExternalUriOptions, ResolvedExternalUri,
};
use je_platform::quickinput::quick_input::{
    IInputBox, IPickOptions, IKeyMods, IQuickInputService, IQuickNavigateConfiguration, IQuickPick, IQuickPickItem, IQuickTree,
    QuickInputAlignment, QuickInputAlignmentSetting, QuickInputAnchor, QuickInputAnchorPosition, QuickInputButtonRef,
    QuickInputHideReason, QuickInputRef, QuickPickCreateOptions, QuickPickInput, QuickPicks,
};
use je_workbench::contrib::external_uri_opener::common::external_uri_opener_service::{
    ExternalUriOpenerContext, ExternalUriOpenerService, IExternalOpenerProvider, IExternalUriOpener, IExternalUriOpenerService,
};
use je_workbench::services::preferences::preferences::{IOpenSettingsOptions, IPreferencesService};

/// `registerExternalOpener: () => { return Disposable.None; }`.
struct StubOpenerService;

impl IOpenerService for StubOpenerService {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        panic!("registerOpener is not a function")
    }
    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        panic!("registerValidator is not a function")
    }
    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        panic!("registerExternalUriResolver is not a function")
    }
    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        panic!("setDefaultExternalOpener is not a function")
    }
    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        lifecycle::none()
    }
    fn open(&self, _resource: &OpenResource, _options: Option<&OpenOptions>) -> Promise<bool> {
        panic!("open is not a function")
    }
    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        panic!("resolveExternalUri is not a function")
    }
}

/// `new class extends mock<IPreferencesService>() { }`.
struct MockPreferencesService;

impl IPreferencesService for MockPreferencesService {
    fn open_user_settings(&self, _options: Option<&IOpenSettingsOptions>) -> Promise<()> {
        panic!("openUserSettings is not a function")
    }

    fn open_application_settings(&self, _options: Option<&IOpenSettingsOptions>) -> Promise<()> {
        panic!("openApplicationSettings is not a function")
    }

    fn get_setting(&self, _setting_id: &str) -> Option<je_workbench::services::preferences::preferences::ISetting> {
        panic!("getSetting is not a function")
    }
}

/// `MockQuickInputService`.
struct MockQuickInputService {
    /// `pickIndex`; `None` é o serviço que o caso não registrou.
    pick_index: Option<usize>,
}

impl IQuickInputService for MockQuickInputService {
    fn pick(&self, picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<IQuickPickItem>> {
        let Some(pick_index) = self.pick_index else {
            panic!("IQuickInputService is not registered")
        };
        let picks = match picks {
            QuickPicks::Items(items) => Promise::resolved(items),
            QuickPicks::Promised(promised) => promised,
        };
        async_fn(async move {
            let resolved_picks = picks.await?;
            Ok(match resolved_picks.into_iter().nth(pick_index) {
                Some(QuickPickInput::Item(item)) => Some(item),
                _ => None,
            })
        })
    }

    fn back_button(&self) -> QuickInputButtonRef {
        panic!("backButton is not defined")
    }

    fn on_show(&self) -> Event<()> {
        panic!("onShow is not defined")
    }

    fn on_hide(&self) -> Event<()> {
        panic!("onHide is not defined")
    }

    fn alignment(&self) -> IObservable<QuickInputAlignment> {
        panic!("alignment is not defined")
    }

    fn with_quick_input_anchor(
        &self,
        _anchor: Option<QuickInputAnchor>,
        _anchor_position: Option<QuickInputAnchorPosition>,
        _operation: Box<dyn FnOnce() -> Promise<Dynamic>>,
    ) -> Promise<Dynamic> {
        panic!("withQuickInputAnchor is not a function")
    }

    fn pick_many(&self, _picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<Vec<IQuickPickItem>>> {
        panic!("pick is not a function")
    }

    fn input(&self, _options: Option<&je_platform::quickinput::quick_input::IInputOptions>, _token: Option<&CancellationToken>) -> Promise<Option<String>> {
        panic!("input is not a function")
    }

    fn create_quick_pick(&self, _options: Option<QuickPickCreateOptions>) -> Rc<dyn IQuickPick> {
        panic!("createQuickPick is not a function")
    }

    fn create_input_box(&self) -> Rc<dyn IInputBox> {
        panic!("createInputBox is not a function")
    }

    fn create_quick_tree(&self) -> Rc<dyn IQuickTree> {
        panic!("createQuickTree is not a function")
    }

    fn focus(&self) {
        panic!("focus is not a function")
    }

    fn toggle(&self) {
        panic!("toggle is not a function")
    }

    fn navigate(&self, _next: bool, _quick_navigate: Option<&IQuickNavigateConfiguration>) {
        panic!("navigate is not a function")
    }

    fn back(&self) -> Promise<()> {
        panic!("back is not a function")
    }

    fn accept(&self, _key_mods: Option<IKeyMods>) -> Promise<()> {
        panic!("accept is not a function")
    }

    fn cancel(&self, _reason: Option<QuickInputHideReason>) -> Promise<()> {
        panic!("cancel is not a function")
    }

    fn toggle_hover(&self) {
        panic!("toggleHover is not a function")
    }

    fn current_quick_input(&self) -> Option<QuickInputRef> {
        panic!("currentQuickInput is not defined")
    }

    fn set_alignment(&self, _alignment: QuickInputAlignmentSetting) {
        panic!("setAlignment is not a function")
    }
}

/// `{ id, label, canOpen: async () => priority, openExternalUri: ... }`.
struct TestOpener {
    id: &'static str,
    label: &'static str,
    priority: ExternalUriOpenerPriority,
    on_open: Box<dyn Fn(&URI, &ExternalUriOpenerContext)>,
}

impl TestOpener {
    fn new(id: &'static str, label: &'static str, priority: ExternalUriOpenerPriority, on_open: impl Fn(&URI, &ExternalUriOpenerContext) + 'static) -> Rc<dyn IExternalUriOpener> {
        Rc::new(Self { id, label, priority, on_open: Box::new(on_open) })
    }
}

impl IExternalUriOpener for TestOpener {
    fn id(&self) -> String {
        self.id.to_string()
    }

    fn label(&self) -> String {
        self.label.to_string()
    }

    fn can_open(&self, _uri: &URI, _token: &CancellationToken) -> Promise<ExternalUriOpenerPriority> {
        Promise::resolved(self.priority)
    }

    fn open_external_uri(&self, uri: &URI, ctx: &ExternalUriOpenerContext, _token: &CancellationToken) -> Promise<bool> {
        (self.on_open)(uri, ctx);
        Promise::resolved(true)
    }
}

/// `new class implements IExternalOpenerProvider { async *getOpeners() { yield ...; } }`.
struct TestOpenerProvider {
    openers: Vec<Rc<dyn IExternalUriOpener>>,
}

impl IExternalOpenerProvider for TestOpenerProvider {
    fn get_openers(&self, _target_uri: &URI) -> Promise<Vec<Rc<dyn IExternalUriOpener>>> {
        Promise::resolved(self.openers.clone())
    }
}

/// O `setup` da suíte, com o serviço de quick input do caso (`None` quando o caso não faz `stub(IQuickInputService, ...)`).
fn create_service(ds: &LeakGuard, pick_index: Option<usize>) -> Rc<ExternalUriOpenerService> {
    let configuration_service = TestConfigurationService::new(None);
    ds.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service;
    let preferences_service: Rc<dyn IPreferencesService> = Rc::new(MockPreferencesService);
    let quick_input_service: Rc<dyn IQuickInputService> = Rc::new(MockQuickInputService { pick_index });
    ds.add_rc(ExternalUriOpenerService::new(Rc::new(StubOpenerService), configuration, NullLogService::shared(), preferences_service, quick_input_service))
}

fn context(source_uri: &URI, view_column: Option<ViewColumn>) -> ExternalOpenerContext {
    ExternalOpenerContext { source_uri: source_uri.clone(), preferred_opener_id: None, view_column }
}

#[test]
fn should_not_open_if_there_are_no_openers() {
    run_with_faked_timers(async move {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let external_uri_opener_service = create_service(&ds, None);

        ds.add(external_uri_opener_service.register_external_opener_provider(Rc::new(TestOpenerProvider { openers: Vec::new() })));

        let uri = URI::parse("http://contoso.com", false);
        let did_open = external_uri_opener_service.open_external(&uri.to_string(false), &context(&uri, None), &CancellationToken::None).await.expect("openExternal");
        assert_eq!(did_open, false);
    });
}

#[test]
fn should_prompt_if_there_is_at_least_one_enabled_opener() {
    run_with_faked_timers(async move {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        // instantiationService.stub(IQuickInputService, new MockQuickInputService(0));
        let external_uri_opener_service = create_service(&ds, Some(0));

        let opened_with_enabled = Rc::new(Cell::new(false));
        let view_column: Rc<RefCell<Option<ViewColumn>>> = Rc::default();
        let (opened, column) = (Rc::clone(&opened_with_enabled), Rc::clone(&view_column));
        ds.add(external_uri_opener_service.register_external_opener_provider(Rc::new(TestOpenerProvider {
            openers: vec![
                TestOpener::new("disabled-id", "disabled", ExternalUriOpenerPriority::None, |_, _| {}),
                TestOpener::new("enabled-id", "enabled", ExternalUriOpenerPriority::Default, move |_, ctx| {
                    opened.set(true);
                    *column.borrow_mut() = ctx.view_column;
                }),
            ],
        })));

        let uri = URI::parse("http://contoso.com", false);
        let did_open = external_uri_opener_service
            .open_external(&uri.to_string(false), &context(&uri, Some(ViewColumn::Beside)), &CancellationToken::None)
            .await
            .expect("openExternal");
        let actual = (did_open, opened_with_enabled.get(), *view_column.borrow());
        assert_eq!(actual, (true, true, Some(ViewColumn::Beside)));
    });
}

#[test]
fn should_automatically_pick_single_preferred_opener_without_prompt() {
    run_with_faked_timers(async move {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let external_uri_opener_service = create_service(&ds, None);

        let opened_with_preferred = Rc::new(Cell::new(false));
        let opened = Rc::clone(&opened_with_preferred);
        ds.add(external_uri_opener_service.register_external_opener_provider(Rc::new(TestOpenerProvider {
            openers: vec![
                TestOpener::new("other-id", "other", ExternalUriOpenerPriority::Default, |_, _| {}),
                TestOpener::new("preferred-id", "preferred", ExternalUriOpenerPriority::Preferred, move |_, _| opened.set(true)),
            ],
        })));

        let uri = URI::parse("http://contoso.com", false);
        let did_open = external_uri_opener_service.open_external(&uri.to_string(false), &context(&uri, None), &CancellationToken::None).await.expect("openExternal");
        assert_eq!(did_open, true);
        assert_eq!(opened_with_preferred.get(), true);
    });
}
