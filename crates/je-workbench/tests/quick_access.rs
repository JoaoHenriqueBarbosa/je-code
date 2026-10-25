//! Porte de `src/vs/workbench/test/browser/quickAccess.test.ts` (suíte `QuickAccess`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (dentro do corpo
//!   assíncrono, que roda em `run_with_real_timers`: [`run_case`]); `disposables.add` vira `guard.add` / `guard.add_rc`. Os
//!   `DisposableStore` locais dos casos (`const disposables = new DisposableStore()`) são os `registrations`.
//! - **`workbenchInstantiationService(undefined, disposables)` e `accessor.quickInputService`.** É o [`Fixture`]: o
//!   `IQuickInputService` é o [`TestQuickInputService`] da bancada (`createQuickPick` devolve o `TestQuickPick` lógico, `show` do
//!   seletor o torna o atual e esconde o anterior, `accept(keyMods)` aceita o item ativo com as teclas dadas), o
//!   `IContextKeyService` é o `MockContextKeyService` (o caso `registry - when condition` cria o `ContextKeyService` real) e o
//!   serviço de instanciação é o real, com os dois serviços na coleção. O `quickAccess` do serviço é o `QuickAccessController`
//!   criado por ele (o original o cria na primeira leitura): `quickAccess.show(...)` é `quick_access().show(...)`.
//! - **Variáveis do módulo (`providerDefaultCalled` etc.).** São um [`Flags`] por thread (cada caso roda na própria thread), então
//!   cada caso começa com todos os sinais falsos, o que o primeiro caso que os usa também assume no original. O `state()` junta as
//!   doze asserções de `Called`, `Canceled` e `Disposed` (nessa ordem) do caso `provider` numa comparação de matriz.
//! - **Classes de provedor com `@IQuickInputService`.** São [`Injectable`] com o [`ServiceImpl`] do tipo de serviço do registro
//!   (`dyn IQuickAccessProvider<dyn Any>`), criados pelo `createInstance` do controlador a partir do [`SyncDescriptor`] do
//!   descritor. O `disposables: DisposableStore` solto do construtor do `TestProviderDefault` não é usado e não existe.
//! - **`store.add(token.onCancellationRequested(...))` e `store.add(toDisposable(...))`.** A ordem dos dois no `DisposableStore`
//!   do provedor não é observável; [`provide_tracked`] registra o `toDisposable` primeiro em todos.
//! - **`setTimeout(() => ...)`.** É `runtime::set_timeout(..., 1)` (o Node arredonda o zero para 1 ms, e o `timeout(1)` dos casos
//!   disputa com ele na ordem de inserção).
//! - **Subclasses de `PickerQuickAccessProvider`.** O `_getPicks` é o [`IPickerQuickAccessSource`] de uma estrutura sem estado, e o
//!   provedor é o [`PickerProvider`], que guarda o `PickerQuickAccessProvider` do prefixo e delega a ele (o `provide`), como o
//!   [`PickerProviderAdapter`] faz para um provedor com opções tipadas. O `Array<IQuickPickItem>` devolvido é o
//!   [`ProvidedPicks::Picks`], a promessa é o [`ProvidedPicks::Promised`], e `{ picks, additionalPicks }` é o
//!   [`ProvidedPicks::FastAndSlow`].
//! - **`IPickerQuickAccessItem` com `accept` e `attach`.** São os [`PickerItemHandlers`] gravados no item; o `keyMods` do `attach`
//!   é guardado como `Option<IKeyMods>` e o `deepStrictEqual` é `assert_eq!`.
//! - **`accessor.quickInputService.quickAccess.pick('fast')` e `result instanceof Promise`.** O tipo de retorno já é uma
//!   [`Promise`]; o caso confere que o provedor rápido foi chamado.
//! - **`defaultPicker.value = '#changed-file'` e o `deepStrictEqual` do objeto.** É `set_value` e `assert_eq!` da tupla
//!   `(value, prefixedProviderCalled, currentPickerPreserved)`; `currentQuickInput === defaultPicker` é a identidade do `Rc`.
//! - **`PickerEditorState can properly restore editors`.** `workbenchInstantiationService`, `createEditorPart`, o `EditorService`
//!   real e o `instantiationService.stub(...)` são o `EditorServiceAccessor` (as divergências dele estão em
//!   `support/editor_service_support.rs`); o painel de teste do recurso de texto é registrado por `register_test_resource_editor`
//!   (o original o registra por efeito do módulo `workbenchTestServices`). `part.activeGroup` é o grupo ativo do
//!   `IEditorGroupsService`, `resource ===` é a igualdade do `URI`, e `selection: new Range(1, 0, 1, 3)` é o
//!   [`TextEditorSelection`]. O ramo `if (part.activeGroup.activeEditorPane?.getSelection)` não roda: o painel lógico de teste não
//!   tem `getSelection` (ver `register_test_resource_editor`), como o painel de recurso do original sem seleção.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "support/test_quick_input.rs"]
mod test_quick_input;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::JsString;
use je_base::async_::{async_fn, timeout as async_timeout};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationService, ServiceCollection, ServiceImpl, SyncDescriptor};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::{ContextKeyExpr, I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::opener::editor_types::EditorOptions;
use je_platform::quickinput::picker_quick_access::{
    FastAndSlowPicks, IPickerQuickAccessSource, PickerItemHandlers, PickerQuickAccessProvider, Picks, PromisedPicks, ProvidedPicks,
};
use je_platform::quickinput::quick_access::{
    IQuickAccessController, IQuickAccessOptions, IQuickAccessProvider, IQuickAccessProviderDescriptor, IQuickAccessProviderRunOptions, IQuickAccessRegistry,
    quick_access_registry,
};
use je_platform::quickinput::quick_access_controller::QuickAccessController;
use je_platform::quickinput::quick_input::{
    I_QUICK_INPUT_SERVICE, IKeyMods, IQuickInputService, IQuickItem, IQuickPick, IQuickPickDidAcceptEvent, IQuickPickItem, QuickInputRef, QuickPickInput,
};
use je_workbench::browser::quickaccess::PickerEditorState;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::{EditorsOrder, TextEditorOptions, UntypedEditorInput};
use je_workbench::contrib::search::browser::anything_quick_access::IPickerEditorState;
use je_workbench::services::editor::editor_groups_service::{IEditorGroup, IEditorGroupsService};
use je_workbench::services::editor::editor_service::IEditorService;
use je_platform::opener::editor_types::TextEditorSelection;
use support::editor_service_support::{EditorServiceAccessor, register_test_resource_editor};
use test_quick_input::TestQuickInputService;

type Descriptor = Rc<IQuickAccessProviderDescriptor>;

//#region Infraestrutura da suíte

/// As variáveis do módulo (`providerDefaultCalled` e companhia), por thread.
#[derive(Default)]
struct Flags {
    provider_default_called: Cell<bool>,
    provider_default_canceled: Cell<bool>,
    provider_default_disposed: Cell<bool>,

    provider1_called: Cell<bool>,
    provider1_canceled: Cell<bool>,
    provider1_disposed: Cell<bool>,

    provider2_called: Cell<bool>,
    provider2_canceled: Cell<bool>,
    provider2_disposed: Cell<bool>,

    provider3_called: Cell<bool>,
    provider3_canceled: Cell<bool>,
    provider3_disposed: Cell<bool>,

    fast_provider_called: Cell<bool>,
    slow_provider_called: Cell<bool>,
    fast_and_slow_provider_called: Cell<bool>,

    slow_provider_canceled: Cell<bool>,
    fast_and_slow_provider_canceled: Cell<bool>,

    attach_test_accept_called: Cell<bool>,
    attach_test_attach_called: Cell<bool>,
    attach_test_attach_key_mods: Cell<Option<IKeyMods>>,

    prefixed_provider_called: Cell<bool>,
    default_picker: RefCell<Option<Rc<dyn IQuickPick>>>,
}

thread_local! {
    static FLAGS: Flags = Flags::default();
}

macro_rules! get {
    ($flag:ident) => {
        FLAGS.with(|flags| flags.$flag.get())
    };
}

macro_rules! set {
    ($flag:ident, $value:expr) => {
        FLAGS.with(|flags| flags.$flag.set($value))
    };
}

/// As doze asserções de `Called`, `Canceled` e `Disposed` do caso `provider`: na ordem
/// `[default, 1, 2, 3]` de cada grupo (`Called`, `Canceled`, `Disposed`).
fn state() -> [bool; 12] {
    [
        get!(provider_default_called),
        get!(provider1_called),
        get!(provider2_called),
        get!(provider3_called),
        get!(provider_default_canceled),
        get!(provider1_canceled),
        get!(provider2_canceled),
        get!(provider3_canceled),
        get!(provider_default_disposed),
        get!(provider1_disposed),
        get!(provider2_disposed),
        get!(provider3_disposed),
    ]
}

/// O `DisposableStore` de um `provide` de teste: `toDisposable` do descarte e o ouvinte do cancelamento.
fn provide_tracked(token: &CancellationToken, on_canceled: impl Fn() + 'static, on_disposed: impl Fn() + 'static) -> Rc<DisposableStore> {
    let store = DisposableStore::new();
    store.add(to_disposable(on_disposed));
    store.add(token.on_cancellation_requested().listen(move |_| on_canceled()));
    store
}

/// Declara o [`ServiceImpl`] de um provedor de teste no tipo de serviço do registro.
macro_rules! provider_service_impl {
    ($ty:ty) => {
        impl ServiceImpl for $ty {
            type Service = dyn IQuickAccessProvider<dyn Any>;

            fn upcast(this: Rc<Self>) -> Rc<dyn IQuickAccessProvider<dyn Any>> {
                this
            }
        }
    };
}

struct TestProviderDefault {
    quick_input_service: Rc<dyn IQuickInputService>,
}

impl Injectable for TestProviderDefault {
    type Args = ();
    type Deps = (Rc<dyn IQuickInputService>,);

    fn create(_: (), (quick_input_service,): Self::Deps) -> Rc<Self> {
        Rc::new(Self { quick_input_service })
    }
}

provider_service_impl!(TestProviderDefault);

impl IQuickAccessProvider<dyn Any> for TestProviderDefault {
    fn provide(&self, _picker: Rc<dyn IQuickPick>, token: CancellationToken, _options: Option<Rc<dyn Any>>) -> DisposableRef {
        set!(provider_default_called, true);
        let store = provide_tracked(&token, || set!(provider_default_canceled, true), || set!(provider_default_disposed, true));

        // bring up provider #3
        let quick_input_service = self.quick_input_service.clone();
        let _ = runtime::set_timeout(move || quick_input_service.quick_access().show(PROVIDER_DESCRIPTOR3_PREFIX, None), 1);

        store
    }
}

struct TestProvider1;

impl Injectable for TestProvider1 {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

provider_service_impl!(TestProvider1);

impl IQuickAccessProvider<dyn Any> for TestProvider1 {
    fn provide(&self, _picker: Rc<dyn IQuickPick>, token: CancellationToken, _options: Option<Rc<dyn Any>>) -> DisposableRef {
        set!(provider1_called, true);
        provide_tracked(&token, || set!(provider1_canceled, true), || set!(provider1_disposed, true))
    }
}

struct TestProvider2;

impl Injectable for TestProvider2 {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

provider_service_impl!(TestProvider2);

impl IQuickAccessProvider<dyn Any> for TestProvider2 {
    fn provide(&self, _picker: Rc<dyn IQuickPick>, token: CancellationToken, _options: Option<Rc<dyn Any>>) -> DisposableRef {
        set!(provider2_called, true);
        provide_tracked(&token, || set!(provider2_canceled, true), || set!(provider2_disposed, true))
    }
}

struct TestProvider3;

impl Injectable for TestProvider3 {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

provider_service_impl!(TestProvider3);

impl IQuickAccessProvider<dyn Any> for TestProvider3 {
    fn provide(&self, picker: Rc<dyn IQuickPick>, token: CancellationToken, _options: Option<Rc<dyn Any>>) -> DisposableRef {
        set!(provider3_called, true);
        let store = provide_tracked(&token, || set!(provider3_canceled, true), || set!(provider3_disposed, true));

        // hide without picking
        let _ = runtime::set_timeout(move || picker.hide(), 1);

        store
    }
}

/// `BoundDefaultProvider` (caso `provider can be bound independently of the input value`).
struct BoundDefaultProvider;

impl Injectable for BoundDefaultProvider {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

provider_service_impl!(BoundDefaultProvider);

impl IQuickAccessProvider<dyn Any> for BoundDefaultProvider {
    fn provide(&self, picker: Rc<dyn IQuickPick>, _token: CancellationToken, _options: Option<Rc<dyn Any>>) -> DisposableRef {
        FLAGS.with(|flags| *flags.default_picker.borrow_mut() = Some(picker));
        to_disposable(|| {})
    }
}

/// `PrefixedProvider`.
struct PrefixedProvider;

impl Injectable for PrefixedProvider {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

provider_service_impl!(PrefixedProvider);

impl IQuickAccessProvider<dyn Any> for PrefixedProvider {
    fn provide(&self, _picker: Rc<dyn IQuickPick>, _token: CancellationToken, _options: Option<Rc<dyn Any>>) -> DisposableRef {
        set!(prefixed_provider_called, true);
        to_disposable(|| {})
    }
}

/// `{ label }` como item do seletor.
fn pick_of(label: &str) -> IQuickPickItem {
    IQuickPickItem { base: IQuickItem { label: JsString::from(label), ..IQuickItem::default() }, ..IQuickPickItem::default() }
}

/// `[{ label }]` como `Picks`.
fn picks_of(label: &str) -> Picks {
    Picks::Items(vec![QuickPickInput::Item(pick_of(label))])
}

/// O `_getPicks` de uma subclasse de `PickerQuickAccessProvider`, com o prefixo que ela passa ao `super`.
trait TestPickSource: IPickerQuickAccessSource<IQuickAccessProviderRunOptions> + Default + 'static {
    const PREFIX: &'static str;
}

/// A subclasse de `PickerQuickAccessProvider<...>` de uma [`TestPickSource`].
struct PickerProvider<S: TestPickSource> {
    provider: PickerQuickAccessProvider,
    source: Rc<S>,
}

impl<S: TestPickSource> Injectable for PickerProvider<S> {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self { provider: PickerQuickAccessProvider::new(S::PREFIX, None), source: Rc::new(S::default()) })
    }
}

impl<S: TestPickSource> ServiceImpl for PickerProvider<S> {
    type Service = dyn IQuickAccessProvider<dyn Any>;

    fn upcast(this: Rc<Self>) -> Rc<dyn IQuickAccessProvider<dyn Any>> {
        this
    }
}

impl<S: TestPickSource> IQuickAccessProvider<dyn Any> for PickerProvider<S> {
    fn as_disposable(&self) -> Option<&dyn IDisposable> {
        Some(&self.provider)
    }

    fn provide(&self, picker: Rc<dyn IQuickPick>, token: CancellationToken, options: Option<Rc<dyn Any>>) -> DisposableRef {
        let run_options = options.and_then(|options| options.downcast::<IQuickAccessProviderRunOptions>().ok());
        self.provider.provide(self.source.clone(), picker, token, run_options)
    }
}

#[derive(Default)]
struct FastSource;

impl TestPickSource for FastSource {
    const PREFIX: &'static str = "fast";
}

impl IPickerQuickAccessSource<IQuickAccessProviderRunOptions> for FastSource {
    fn get_picks(&self, _filter: &str, _disposables: &Rc<DisposableStore>, _token: CancellationToken, _run_options: Option<&Rc<IQuickAccessProviderRunOptions>>) -> ProvidedPicks {
        set!(fast_provider_called, true);

        ProvidedPicks::Picks(picks_of("Fast Pick"))
    }
}

#[derive(Default)]
struct SlowSource;

impl TestPickSource for SlowSource {
    const PREFIX: &'static str = "slow";
}

impl IPickerQuickAccessSource<IQuickAccessProviderRunOptions> for SlowSource {
    fn get_picks(&self, _filter: &str, _disposables: &Rc<DisposableStore>, token: CancellationToken, _run_options: Option<&Rc<IQuickAccessProviderRunOptions>>) -> ProvidedPicks {
        set!(slow_provider_called, true);

        ProvidedPicks::Promised(async_fn(async move {
            async_timeout(1).await?;

            if token.is_cancellation_requested() {
                set!(slow_provider_canceled, true);
            }

            Ok(PromisedPicks::Picks(picks_of("Slow Pick")))
        }))
    }
}

#[derive(Default)]
struct FastAndSlowSource;

impl TestPickSource for FastAndSlowSource {
    const PREFIX: &'static str = "bothFastAndSlow";
}

impl IPickerQuickAccessSource<IQuickAccessProviderRunOptions> for FastAndSlowSource {
    fn get_picks(&self, _filter: &str, _disposables: &Rc<DisposableStore>, token: CancellationToken, _run_options: Option<&Rc<IQuickAccessProviderRunOptions>>) -> ProvidedPicks {
        set!(fast_and_slow_provider_called, true);

        ProvidedPicks::FastAndSlow(FastAndSlowPicks {
            picks: picks_of("Fast Pick"),
            additional_picks: async_fn(async move {
                async_timeout(1).await?;

                if token.is_cancellation_requested() {
                    set!(fast_and_slow_provider_canceled, true);
                }

                Ok(picks_of("Slow Pick"))
            }),
            merge_delay: None,
        })
    }
}

/// `AttachTestQuickPickProvider`: o item tem `accept` e `attach`.
#[derive(Default)]
struct AttachSource;

impl TestPickSource for AttachSource {
    const PREFIX: &'static str = "attach";
}

impl IPickerQuickAccessSource<IQuickAccessProviderRunOptions> for AttachSource {
    fn get_picks(&self, _filter: &str, _disposables: &Rc<DisposableStore>, _token: CancellationToken, _run_options: Option<&Rc<IQuickAccessProviderRunOptions>>) -> ProvidedPicks {
        let mut item = pick_of("Test Item");
        PickerItemHandlers {
            accept: Some(Rc::new(|_: &IKeyMods, _: &IQuickPickDidAcceptEvent| set!(attach_test_accept_called, true))),
            attach: Some(Rc::new(|key_mods: &IKeyMods, _: &IQuickPickDidAcceptEvent| {
                set!(attach_test_attach_called, true);
                set!(attach_test_attach_key_mods, Some(*key_mods));
            })),
            ..PickerItemHandlers::default()
        }
        .attach_to(&mut item);

        ProvidedPicks::Picks(Picks::Items(vec![QuickPickInput::Item(item)]))
    }
}

/// `AttachTestNoAttachProvider`: o item só tem `accept`.
#[derive(Default)]
struct NoAttachSource;

impl TestPickSource for NoAttachSource {
    const PREFIX: &'static str = "noattach";
}

impl IPickerQuickAccessSource<IQuickAccessProviderRunOptions> for NoAttachSource {
    fn get_picks(&self, _filter: &str, _disposables: &Rc<DisposableStore>, _token: CancellationToken, _run_options: Option<&Rc<IQuickAccessProviderRunOptions>>) -> ProvidedPicks {
        let mut item = pick_of("No Attach Item");
        PickerItemHandlers {
            accept: Some(Rc::new(|_: &IKeyMods, _: &IQuickPickDidAcceptEvent| set!(attach_test_accept_called, true))),
            ..PickerItemHandlers::default()
        }
        .attach_to(&mut item);

        ProvidedPicks::Picks(Picks::Items(vec![QuickPickInput::Item(item)]))
    }
}

const PROVIDER_DESCRIPTOR3_PREFIX: &str = "changed";

/// `{ ctor, prefix, helpEntries: [] }`.
fn descriptor<C: ServiceImpl<Service = dyn IQuickAccessProvider<dyn Any>> + Injectable<Args = ()>>(prefix: &str) -> Descriptor {
    Rc::new(IQuickAccessProviderDescriptor {
        ctor: SyncDescriptor::<dyn IQuickAccessProvider<dyn Any>>::new::<C>(()),
        prefix: prefix.to_string(),
        placeholder: None,
        help_entries: Vec::new(),
        context_key: None,
        when: None,
    })
}

/// `providerDescriptorDefault`, `providerDescriptor1`, `providerDescriptor2` e `providerDescriptor3`.
fn provider_descriptors() -> [Descriptor; 4] {
    [
        descriptor::<TestProviderDefault>(""),
        descriptor::<TestProvider1>("test"),
        descriptor::<TestProvider2>("test something"),
        descriptor::<TestProvider3>(PROVIDER_DESCRIPTOR3_PREFIX),
    ]
}

/// `fastProviderDescriptor`, `slowProviderDescriptor` e `fastAndSlowProviderDescriptor`.
fn picker_descriptors() -> [Descriptor; 3] {
    [descriptor::<PickerProvider<FastSource>>("fast"), descriptor::<PickerProvider<SlowSource>>("slow"), descriptor::<PickerProvider<FastAndSlowSource>>("bothFastAndSlow")]
}

/// `registry.getQuickAccessProvider(...) === expected`.
fn is_descriptor(actual: Option<Descriptor>, expected: &Descriptor) -> bool {
    actual.is_some_and(|actual| Rc::ptr_eq(&actual, expected))
}

/// `setup`: o serviço de quick input, o serviço de instanciação e o `QuickAccessController` do `quickAccess`.
struct Fixture {
    service: Rc<TestQuickInputService>,
    context_key_service: Rc<dyn IContextKeyService>,
    quick_access: Rc<dyn IQuickAccessController>,
}

fn setup(guard: &LeakGuard) -> Fixture {
    let service = TestQuickInputService::new();
    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
    let quick_input_service: Rc<dyn IQuickInputService> = service.clone();
    let instantiation_service =
        guard.add_rc(InstantiationService::new(ServiceCollection::new().with(I_QUICK_INPUT_SERVICE, quick_input_service).with(I_CONTEXT_KEY_SERVICE, context_key_service.clone())));

    let controller = guard.add_rc(instantiation_service.create_instance::<QuickAccessController>(()));
    let quick_access: Rc<dyn IQuickAccessController> = controller;
    service.set_quick_access(Some(quick_access.clone()));
    let service_to_release = service.clone();
    guard.add(to_disposable(move || service_to_release.set_quick_access(None)));

    Fixture { service, context_key_service, quick_access }
}

/// O caso assíncrono: o guarda de vazamentos e o corpo no relógio real.
fn run_case<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(LeakGuard) -> Fut + 'static) {
    run_with_real_timers(async move { body(ensure_no_disposables_are_leaked_in_test_suite()).await });
}

//#endregion

#[test]
fn registry() {
    run_case(|guard| async move {
        let fixture = setup(&guard);
        let [provider_descriptor_default, provider_descriptor1, _, _] = provider_descriptors();

        let registry = quick_access_registry();
        let restore = registry.clear();
        let context_key_service = fixture.context_key_service.clone();

        assert!(registry.get_quick_access_provider("test", &*context_key_service).is_none());

        let registrations = DisposableStore::new();

        registrations.add(registry.register_quick_access_provider(provider_descriptor_default.clone()));
        assert!(is_descriptor(registry.get_quick_access_provider("", &*context_key_service), &provider_descriptor_default));
        assert!(is_descriptor(registry.get_quick_access_provider("test", &*context_key_service), &provider_descriptor_default));

        let disposable = registrations.add(registry.register_quick_access_provider(provider_descriptor1.clone()));
        assert!(is_descriptor(registry.get_quick_access_provider("test", &*context_key_service), &provider_descriptor1));

        let providers = registry.get_quick_access_providers(&*context_key_service);
        assert!(providers.iter().any(|provider| provider.prefix == "test"));

        disposable.dispose();
        assert!(is_descriptor(registry.get_quick_access_provider("test", &*context_key_service), &provider_descriptor_default));

        registrations.dispose();
        assert!(registry.get_quick_access_provider("test", &*context_key_service).is_none());

        restore();
    });
}

#[test]
fn registry_when_condition() {
    run_case(|guard| async move {
        let registry = quick_access_registry();
        let restore = registry.clear();

        // Use real ContextKeyService that properly evaluates rules
        let configuration = TestConfigurationService::new(None);
        guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
        let context_key_service = guard.add_rc(ContextKeyService::new(configuration));
        let local_disposables = DisposableStore::new();

        // Create a context key that starts as undefined (falsy)
        let context_key = context_key_service.create_key("testQuickAccessContextKey", Dynamic::Undefined);

        // Register a provider with a when condition that requires testQuickAccessContextKey to be truthy
        let when = ContextKeyExpr::has("testQuickAccessContextKey");
        let provider_with_when = Rc::new(IQuickAccessProviderDescriptor {
            when: Some(when.clone()),
            ..(*descriptor::<TestProvider1>("whentest")).clone()
        });
        local_disposables.add(registry.register_quick_access_provider(provider_with_when.clone()));

        // Verify the expression works with the context key service
        assert!(!context_key_service.context_matches_rules(Some(&when)));

        // Provider with false when condition should not be found
        assert!(registry.get_quick_access_provider("whentest", &*context_key_service).is_none());

        // Should not appear in the list of providers
        let providers = registry.get_quick_access_providers(&*context_key_service);
        assert!(!providers.iter().any(|provider| provider.prefix == "whentest"));

        // Set the context key to true
        context_key.set(Dynamic::Bool(true));

        // Verify the expression now matches
        assert!(context_key_service.context_matches_rules(Some(&when)));

        // Now the provider should be found
        assert!(is_descriptor(registry.get_quick_access_provider("whentest", &*context_key_service), &provider_with_when));

        // Should appear in the list of providers
        let providers = registry.get_quick_access_providers(&*context_key_service);
        assert!(providers.iter().any(|provider| provider.prefix == "whentest"));

        // Set context key back to undefined (falsy)
        context_key.set(Dynamic::Undefined);

        // Provider should not be found again
        assert!(registry.get_quick_access_provider("whentest", &*context_key_service).is_none());

        local_disposables.dispose();

        restore();
    });
}

#[test]
fn provider() {
    run_case(|guard| async move {
        let fixture = setup(&guard);
        let registry = quick_access_registry();
        let restore = registry.clear();

        let registrations = DisposableStore::new();

        for provider_descriptor in provider_descriptors() {
            registrations.add(registry.register_quick_access_provider(provider_descriptor));
        }

        fixture.quick_access.show("test", None);
        assert_eq!(state(), [false, true, false, false, false, false, false, false, false, false, false, false]);
        set!(provider1_called, false);

        fixture.quick_access.show("test something", None);
        assert_eq!(state(), [false, false, true, false, false, true, false, false, false, true, false, false]);
        set!(provider2_called, false);
        set!(provider1_canceled, false);
        set!(provider1_disposed, false);

        fixture.quick_access.show("usedefault", None);
        assert_eq!(state(), [true, false, false, false, false, false, true, false, false, false, true, false]);

        timeout(1).await.unwrap();

        assert!(get!(provider_default_canceled));
        assert!(get!(provider_default_disposed));
        assert!(get!(provider3_called));

        timeout(1).await.unwrap();

        assert!(get!(provider3_canceled));
        assert!(get!(provider3_disposed));

        registrations.dispose();

        restore();
    });
}

#[test]
fn provider_can_be_bound_independently_of_the_input_value() {
    run_case(|guard| async move {
        let fixture = setup(&guard);
        let registry = quick_access_registry();
        let restore = registry.clear();
        let local_disposables = DisposableStore::new();

        local_disposables.add(registry.register_quick_access_provider(descriptor::<BoundDefaultProvider>("")));
        local_disposables.add(registry.register_quick_access_provider(descriptor::<PrefixedProvider>("#")));

        fixture.quick_access.show("#literal-file", Some(IQuickAccessOptions { provider_prefix: Some(String::new()), ..IQuickAccessOptions::default() }));
        let default_picker = FLAGS.with(|flags| flags.default_picker.borrow().clone()).expect("o provedor padrão recebeu o seletor");
        default_picker.set_value("#changed-file".to_string());

        let current_picker_preserved = match fixture.service.current_quick_input() {
            Some(QuickInputRef::QuickPick(current)) => Rc::ptr_eq(&current, &default_picker),
            _ => false,
        };
        assert_eq!(
            (default_picker.value(), get!(prefixed_provider_called), current_picker_preserved),
            ("#changed-file".to_string(), false, true)
        );

        default_picker.hide();
        FLAGS.with(|flags| *flags.default_picker.borrow_mut() = None);
        local_disposables.dispose();
        restore();
    });
}

#[test]
fn quick_pick_access_show() {
    run_case(|guard| async move {
        let fixture = setup(&guard);
        let registry = quick_access_registry();
        let restore = registry.clear();

        let registrations = DisposableStore::new();

        for picker_descriptor in picker_descriptors() {
            registrations.add(registry.register_quick_access_provider(picker_descriptor));
        }

        fixture.quick_access.show("fast", None);
        assert!(get!(fast_provider_called));
        assert!(!get!(slow_provider_called));
        assert!(!get!(fast_and_slow_provider_called));
        set!(fast_provider_called, false);

        fixture.quick_access.show("slow", None);
        timeout(2).await.unwrap();

        assert!(!get!(fast_provider_called));
        assert!(get!(slow_provider_called));
        assert!(!get!(slow_provider_canceled));
        assert!(!get!(fast_and_slow_provider_called));
        set!(slow_provider_called, false);

        fixture.quick_access.show("bothFastAndSlow", None);
        timeout(2).await.unwrap();

        assert!(!get!(fast_provider_called));
        assert!(!get!(slow_provider_called));
        assert!(get!(fast_and_slow_provider_called));
        assert!(!get!(fast_and_slow_provider_canceled));
        set!(fast_and_slow_provider_called, false);

        fixture.quick_access.show("slow", None);
        fixture.quick_access.show("bothFastAndSlow", None);
        fixture.quick_access.show("fast", None);

        assert!(get!(fast_provider_called));
        assert!(get!(slow_provider_called));
        assert!(get!(fast_and_slow_provider_called));

        timeout(2).await.unwrap();
        assert!(get!(slow_provider_canceled));
        assert!(get!(fast_and_slow_provider_canceled));

        registrations.dispose();

        restore();
    });
}

#[test]
fn quick_pick_access_pick() {
    run_case(|guard| async move {
        let fixture = setup(&guard);
        let registry = quick_access_registry();
        let restore = registry.clear();

        let registrations = DisposableStore::new();

        registrations.add(registry.register_quick_access_provider(descriptor::<PickerProvider<FastSource>>("fast")));

        let _result: Promise<Option<Vec<IQuickPickItem>>> = fixture.quick_access.pick("fast", None);
        assert!(get!(fast_provider_called));

        registrations.dispose();

        restore();
    });
}

#[test]
fn picker_editor_state_can_properly_restore_editors() {
    run_case(|guard| async move {
        guard.add(register_test_resource_editor());
        let accessor = EditorServiceAccessor::with_default_backup_service(&guard.store());
        accessor.part.when_ready().await.unwrap();
        let editor_service: Rc<dyn IEditorService> = accessor.editor_service.clone();
        let part: Rc<dyn IEditorGroupsService> = accessor.editor_groups_service.clone();

        let editor_view_state = guard.add_rc(accessor.instantiation_service.create_instance::<PickerEditorState>(()));

        let input1 = (
            URI::parse("foo://bar1", false),
            TextEditorOptions {
                base: EditorOptions { pinned: Some(true), preserve_focus: Some(true), ..EditorOptions::default() },
                selection: Some(TextEditorSelection { start_line_number: 1.0, start_column: 0.0, end_line_number: Some(1.0), end_column: Some(3.0) }),
                ..TextEditorOptions::default()
            },
        );
        let input2 = (
            URI::parse("foo://bar2", false),
            TextEditorOptions {
                base: EditorOptions { pinned: Some(true), ..EditorOptions::default() },
                selection: Some(TextEditorSelection { start_line_number: 1.0, start_column: 0.0, end_line_number: Some(1.0), end_column: Some(3.0) }),
                ..TextEditorOptions::default()
            },
        );
        let untyped = |(resource, options): &(URI, TextEditorOptions)| UntypedEditorInput { resource: Some(resource.clone()), options: Some(options.clone()), ..UntypedEditorInput::default() };
        let input3 = UntypedEditorInput { resource: Some(URI::parse("foo://bar3", false)), ..UntypedEditorInput::default() };
        let input4 = UntypedEditorInput { resource: Some(URI::parse("foo://bar4", false)), ..UntypedEditorInput::default() };

        let editor = editor_service.open_editor(untyped(&input1)).await.unwrap().expect("o editor foi aberto");
        let active_editor_pane = editor_service.active_editor_pane().expect("há um editor ativo");
        assert!(Rc::ptr_eq(&editor, &active_editor_pane));
        editor_view_state.set();
        editor_service.open_editor(untyped(&input2)).await.unwrap();
        editor_view_state.open_transient_editor(input3, None).await.unwrap();
        editor_view_state.open_transient_editor(input4, None).await.unwrap();
        editor_view_state.restore().await.unwrap();

        let active_group = part.active_group();
        assert_eq!(active_group.active_editor().and_then(|editor| editor.resource()), Some(input1.0.clone()));
        assert_eq!(
            active_group.get_editors(EditorsOrder::MostRecentlyActive, None).iter().map(|editor| editor.resource()).collect::<Vec<_>>(),
            vec![Some(input1.0.clone()), Some(input2.0.clone())]
        );
        active_group.close_all_editors(None).await.unwrap();
    });
}

//#region attach dispatch tests

/// `resetAttachState()`.
fn reset_attach_state() {
    set!(attach_test_accept_called, false);
    set!(attach_test_attach_called, false);
    set!(attach_test_attach_key_mods, None);
}

/// Os casos de `attach`: registra o provedor do prefixo, abre o seletor e aceita com as teclas dadas.
fn accept_case(descriptor: Descriptor, prefix: &'static str, key_mods: Option<IKeyMods>, check: impl FnOnce() + 'static) {
    run_case(move |guard| async move {
        let fixture = setup(&guard);
        let registry = quick_access_registry();
        let restore = registry.clear();
        let registrations = DisposableStore::new();

        registrations.add(registry.register_quick_access_provider(descriptor));
        reset_attach_state();

        fixture.quick_access.show(prefix, None);
        fixture.service.accept(key_mods).await.unwrap();

        check();

        registrations.dispose();
        restore();
    });
}

#[test]
fn quick_pick_access_accept_without_modifier_keys_calls_accept_not_attach() {
    accept_case(descriptor::<PickerProvider<AttachSource>>("attach"), "attach", None, || {
        assert!(get!(attach_test_accept_called));
        assert!(!get!(attach_test_attach_called));
    });
}

#[test]
fn quick_pick_access_accept_with_ctrl_cmd_calls_attach_instead_of_accept() {
    let key_mods = IKeyMods { ctrl_cmd: true, alt: false, shift: false };
    accept_case(descriptor::<PickerProvider<AttachSource>>("attach"), "attach", Some(key_mods), move || {
        assert!(!get!(attach_test_accept_called));
        assert!(get!(attach_test_attach_called));
        assert_eq!(get!(attach_test_attach_key_mods), Some(key_mods));
    });
}

#[test]
fn quick_pick_access_accept_with_alt_calls_attach_instead_of_accept() {
    let key_mods = IKeyMods { ctrl_cmd: false, alt: true, shift: false };
    accept_case(descriptor::<PickerProvider<AttachSource>>("attach"), "attach", Some(key_mods), move || {
        assert!(!get!(attach_test_accept_called));
        assert!(get!(attach_test_attach_called));
        assert_eq!(get!(attach_test_attach_key_mods), Some(key_mods));
    });
}

#[test]
fn quick_pick_access_accept_with_modifier_keys_but_no_attach_method_calls_accept() {
    let key_mods = IKeyMods { ctrl_cmd: true, alt: false, shift: false };
    accept_case(descriptor::<PickerProvider<NoAttachSource>>("noattach"), "noattach", Some(key_mods), || {
        assert!(get!(attach_test_accept_called));
        assert!(!get!(attach_test_attach_called));
    });
}

//#endregion
