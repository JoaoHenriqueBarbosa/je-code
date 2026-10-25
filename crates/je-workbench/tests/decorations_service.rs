//! Porte de `src/vs/workbench/services/decorations/test/browser/decorationsService.test.ts` (suíte `DecorationsService`), caso a caso e na mesma ordem.
//!
//! Casos que ficaram de fora:
//! - `Falls back to lower-weight decoration color when a theme color is undefined`: mede a cor computada de um `div` no DOM
//!   (`document.createElement`, `getComputedStyle(element, '::after')`), o que a interface nativa não tem. As regras de estilo
//!   que o serviço gera estão em `DecorationsService::style_rules` e a cascata de `var(--vscode-..., fallback)` é do
//!   renderizador.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O `setup` (o `service`) roda antes do guarda de
//!   vazamentos, e o `teardown` (`service.dispose()`) antes da conferência dele; o [`Env`] cria o serviço antes do
//!   [`LeakGuard`] e descarta o serviço no `Drop`, antes dos campos.
//! - **`new class extends mock<IUriIdentityService>() { override extUri = resources.extUri }`.** É o
//!   [`TestUriIdentityService`] com o `ext_uri()` global. **`new TestThemeService()`** é o [`UnthemedProductIconTheme`], o
//!   único pedaço do tema que o serviço lê (ver as divergências de `decorations_service.rs`); como no `TestThemeService`, o
//!   tema de ícones não define nenhum ícone.
//! - **Provedores.** `new class implements IDecorationsProvider { ... }` e os literais de objeto são o [`TestProvider`]:
//!   o rótulo, o evento e o fecho de `provideDecorations`.
//! - **`new Promise(resolve => setTimeout(() => resolve(...)))`** é `Promise::with_executor` com `runtime::set_timeout(.., 0)`.
//! - **Asserções dentro de ouvintes de evento.** Um `assert` que falha no ouvinte, no original, vira erro do `mocha`; aqui o
//!   ouvinte guarda o que observou e a conferência vem depois do `await`.
//! - **`Decorations not showing up for second root folder #48502`** deixa um `setTimeout` de 10 ms pendente ao fim do caso
//!   (o original deixa o timer do `mocha` correr depois do `teardown`); o caso roda em `run_with_faked_timers`, que o esvazia.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::resources::ExtUri;
use je_base::runtime;
use je_base::themables::ThemeIcon;
use je_base::uri::{Field, URI, UriChange};
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_workbench::services::decorations::decorations::{DecorationData, DecorationLetter, IDecorationsProvider, IDecorationsService};
use je_theme::icon_registry::{IconContribution, IconDefinition};
use je_workbench::services::decorations::decorations_service::{DecorationsService, IProductIconTheme};

/// O `mock<IUriIdentityService>()` com `extUri = resources.extUri`.
struct TestUriIdentityService {
    ext_uri: ExtUri,
}

impl IUriIdentityService for TestUriIdentityService {
    fn ext_uri(&self) -> ExtUri {
        self.ext_uri.clone()
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

/// `UnthemedProductIconTheme` do `TestThemeService`: não define nenhum ícone.
struct UnthemedProductIconTheme;

impl IProductIconTheme for UnthemedProductIconTheme {
    fn get_icon(&self, _contribution: &IconContribution) -> Option<IconDefinition> {
        None
    }
}

type Provide = dyn Fn(&URI, CancellationToken) -> MaybePromise<Option<DecorationData>>;

/// Um `IDecorationsProvider` de teste.
struct TestProvider {
    label: String,
    on_did_change: Event<Vec<URI>>,
    provide: Box<Provide>,
}

impl TestProvider {
    fn new(label: &str, on_did_change: Event<Vec<URI>>, provide: impl Fn(&URI, CancellationToken) -> MaybePromise<Option<DecorationData>> + 'static) -> Rc<dyn IDecorationsProvider> {
        Rc::new(Self { label: label.to_string(), on_did_change, provide: Box::new(provide) })
    }
}

impl IDecorationsProvider for TestProvider {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn on_did_change(&self) -> Event<Vec<URI>> {
        self.on_did_change.clone()
    }

    fn provide_decorations(&self, uri: &URI, token: CancellationToken) -> MaybePromise<Option<DecorationData>> {
        (self.provide)(uri, token)
    }
}

/// `setup` e `teardown`.
struct Env {
    service: Rc<DecorationsService>,
    guard: LeakGuard,
}

impl Env {
    fn new() -> Self {
        let service = DecorationsService::new(Rc::new(TestUriIdentityService { ext_uri: ext_uri() }), Rc::new(UnthemedProductIconTheme));
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        Self { service, guard }
    }
}

impl Drop for Env {
    /// `teardown`: `service.dispose()`.
    fn drop(&mut self) {
        self.service.dispose();
    }
}

fn parse(text: &str) -> URI {
    URI::parse(text, false)
}

fn with_path(uri: &URI, path: &str) -> URI {
    uri.with(&UriChange { path: Field::Set(JsString::from(path)), ..UriChange::default() })
}

fn path_of(uri: &URI) -> String {
    uri.path().to_string_lossy()
}

fn sync(data: DecorationData) -> MaybePromise<Option<DecorationData>> {
    MaybePromise::Value(Some(data))
}

/// `Event.None`.
fn none() -> Event<Vec<URI>> {
    Event::none()
}

#[test]
fn async_provider_async_evented_result() {
    run_with_faked_timers(async {
        let env = Env::new();
        let uri = parse("foo:bar");
        let call_counter = Rc::new(Cell::new(0));

        let counter = call_counter.clone();
        let reg = env.service.register_decorations_provider(TestProvider::new("Test", none(), move |_uri, _token| {
            counter.set(counter.get() + 1);
            MaybePromise::Promise(Promise::with_executor(|resolver| {
                runtime::set_timeout(
                    move || {
                        resolver.resolve(Some(DecorationData {
                            color: Some("someBlue".to_string()),
                            tooltip: Some("T".to_string()),
                            strikethrough: Some(true),
                            ..DecorationData::default()
                        }))
                    },
                    0,
                );
            }))
        }));

        // trigger -> async
        assert!(env.service.get_decoration(&uri, false).is_none());
        assert_eq!(call_counter.get(), 1);

        // event when result is computed
        let e = env.service.on_did_change_decorations().to_promise(None).await.unwrap();
        assert!(e.affects_resource(&uri));
        // sync result
        assert_eq!(env.service.get_decoration(&uri, false).unwrap().tooltip(), "T");
        assert!(env.service.get_decoration(&uri, false).unwrap().strikethrough());
        assert_eq!(call_counter.get(), 1);

        reg.dispose();
    });
}

#[test]
fn sync_provider_sync_result() {
    let env = Env::new();
    let uri = parse("foo:bar");
    let call_counter = Rc::new(Cell::new(0));

    let counter = call_counter.clone();
    let reg = env.service.register_decorations_provider(TestProvider::new("Test", none(), move |_uri, _token| {
        counter.set(counter.get() + 1);
        sync(DecorationData { color: Some("someBlue".to_string()), tooltip: Some("Z".to_string()), ..DecorationData::default() })
    }));

    // trigger -> sync
    assert_eq!(env.service.get_decoration(&uri, false).unwrap().tooltip(), "Z");
    assert!(!env.service.get_decoration(&uri, false).unwrap().strikethrough());
    assert_eq!(call_counter.get(), 1);

    reg.dispose();
}

#[test]
fn classifies_text_icon_and_bubbled_badges() {
    let env = Env::new();
    let text_uri = parse("file:///text.txt");
    let icon_uri = parse("file:///icon.txt");
    let parent_uri = parse("file:///folder/");
    let child_uri = parse("file:///folder/child.txt");
    let decoration_data: Vec<(JsString, DecorationData)> = vec![
        (text_uri.to_string(false), DecorationData { letter: Some(DecorationLetter::Text("M".to_string())), ..DecorationData::default() }),
        (icon_uri.to_string(false), DecorationData { letter: Some(DecorationLetter::Icon(ThemeIcon::from_id("lock-small"))), ..DecorationData::default() }),
        (child_uri.to_string(false), DecorationData { letter: Some(DecorationLetter::Text("M".to_string())), bubble: Some(true), ..DecorationData::default() }),
    ];
    let registration = env.service.register_decorations_provider(TestProvider::new("Badge types", none(), move |resource, _token| {
        let key = resource.to_string(false);
        MaybePromise::Value(decoration_data.iter().find(|(uri, _)| *uri == key).map(|(_, data)| data.clone()))
    }));

    let text_decoration = env.service.get_decoration(&text_uri, false).unwrap();
    let icon_decoration = env.service.get_decoration(&icon_uri, false).unwrap();
    if let Some(child_decoration) = env.service.get_decoration(&child_uri, false) {
        child_decoration.dispose();
    }
    let bubble_decoration = env.service.get_decoration(&parent_uri, true).unwrap();

    assert_eq!(
        [text_decoration.is_text_badge(), icon_decoration.is_text_badge(), bubble_decoration.is_text_badge()],
        [true, false, false]
    );

    text_decoration.dispose();
    icon_decoration.dispose();
    bubble_decoration.dispose();
    registration.dispose();
}

#[test]
fn clear_decorations_on_provider_dispose() {
    run_with_faked_timers(async {
        let env = Env::new();
        let uri = parse("foo:bar");
        let call_counter = Rc::new(Cell::new(0));

        let counter = call_counter.clone();
        let reg = env.service.register_decorations_provider(TestProvider::new("Test", none(), move |_uri, _token| {
            counter.set(counter.get() + 1);
            sync(DecorationData { color: Some("someBlue".to_string()), tooltip: Some("J".to_string()), ..DecorationData::default() })
        }));

        // trigger -> sync
        assert_eq!(env.service.get_decoration(&uri, false).unwrap().tooltip(), "J");
        assert_eq!(call_counter.get(), 1);

        // un-register -> ensure good event
        let observed: Rc<RefCell<Option<(bool, bool, i32)>>> = Rc::new(RefCell::new(None));
        let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        let (promise, resolver) = Promise::<()>::new();
        {
            let (service, uri, observed, counter, own_listener) = (env.service.clone(), uri.clone(), observed.clone(), call_counter.clone(), listener.clone());
            *listener.borrow_mut() = Some(env.service.on_did_change_decorations().listen(move |e| {
                *observed.borrow_mut() = Some((e.affects_resource(&uri), service.get_decoration(&uri, false).is_none(), counter.get()));
                if let Some(l) = own_listener.borrow_mut().take() {
                    l.dispose();
                }
                resolver.resolve(());
            }));
        }
        reg.dispose(); // will clear all data
        promise.await.unwrap();
        let did_see_event = observed.borrow().is_some();
        assert_eq!(*observed.borrow(), Some((true, true, 1)));
        assert!(did_see_event);
    });
}

#[test]
fn no_default_bubbling() {
    let env = Env::new();

    let mut reg = env.service.register_decorations_provider(TestProvider::new("Test", none(), |uri, _token| {
        MaybePromise::Value(path_of(uri).contains(".txt").then(|| DecorationData { tooltip: Some(".txt".to_string()), weight: Some(17), ..DecorationData::default() }))
    }));

    let child_uri = parse("file:///some/path/some/file.txt");

    let mut deco = env.service.get_decoration(&child_uri, false);
    assert_eq!(deco.unwrap().tooltip(), ".txt");

    deco = env.service.get_decoration(&with_path(&child_uri, "some/path/"), true);
    assert!(deco.is_none());
    reg.dispose();

    // bubble
    reg = env.service.register_decorations_provider(TestProvider::new("Test", none(), |uri, _token| {
        MaybePromise::Value(path_of(uri).contains(".txt").then(|| DecorationData {
            tooltip: Some(".txt.bubble".to_string()),
            weight: Some(71),
            bubble: Some(true),
            ..DecorationData::default()
        }))
    }));

    deco = env.service.get_decoration(&child_uri, false);
    assert_eq!(deco.unwrap().tooltip(), ".txt.bubble");

    // `typeof deco.tooltip === 'string'`: a decoração existe, e a dica de quem só contém itens enfatizados é um texto.
    deco = env.service.get_decoration(&with_path(&child_uri, "some/path/"), true);
    assert!(deco.is_some());
    reg.dispose();
}

#[test]
fn decorations_not_showing_up_for_second_root_folder_48502() {
    run_with_faked_timers(async {
        let env = Env::new();
        let cancel_count = Rc::new(Cell::new(0));
        let call_count = Rc::new(Cell::new(0));

        let on_did_change = Emitter::<Vec<URI>>::new();
        let provider = {
            let (cancel_count, call_count, guard_store) = (cancel_count.clone(), call_count.clone(), env.guard.store());
            TestProvider::new("foo", on_did_change.event(), move |_uri, token| {
                let cancel_count = cancel_count.clone();
                guard_store.add(token.on_cancellation_requested().listen(move |_| cancel_count.set(cancel_count.get() + 1)));

                let call_count = call_count.clone();
                MaybePromise::Promise(Promise::with_executor(move |resolver| {
                    call_count.set(call_count.get() + 1);
                    runtime::set_timeout(
                        move || resolver.resolve(Some(DecorationData { letter: Some(DecorationLetter::Text("foo".to_string())), ..DecorationData::default() })),
                        10,
                    );
                }))
            })
        };

        let reg = env.service.register_decorations_provider(provider);

        let uri = parse("foo://bar");
        let d1 = env.service.get_decoration(&uri, false);

        on_did_change.fire(vec![uri.clone()]);
        let d2 = env.service.get_decoration(&uri, false);

        assert_eq!(cancel_count.get(), 1);
        assert_eq!(call_count.get(), 2);

        if let Some(d1) = d1 {
            d1.dispose();
        }
        if let Some(d2) = d2 {
            d2.dispose();
        }
        reg.dispose();
        on_did_change.dispose();
    });
}

#[test]
fn decorations_not_bubbling_48745() {
    let env = Env::new();

    let reg = env.service.register_decorations_provider(TestProvider::new("Test", none(), |uri, _token| {
        if path_of(uri).ends_with("hello") {
            sync(DecorationData { tooltip: Some("FOO".to_string()), weight: Some(17), bubble: Some(true), ..DecorationData::default() })
        } else {
            // `new Promise<IDecorationData>(_resolve => { })`
            MaybePromise::Promise(Promise::<Option<DecorationData>>::new().0)
        }
    }));

    let data1 = env.service.get_decoration(&parse("a:b/"), true);
    assert!(data1.is_none());

    let data2 = env.service.get_decoration(&parse("a:b/c.hello"), false).unwrap();
    assert!(!data2.tooltip().is_empty());

    let data3 = env.service.get_decoration(&parse("a:b/"), true);
    assert!(data3.is_some());

    reg.dispose();
}

#[test]
fn folder_decorations_dont_go_away_when_file_with_problems_is_deleted_61919_part1() {
    let env = Env::new();

    let emitter = Emitter::<Vec<URI>>::new();
    let gone = Rc::new(Cell::new(false));
    let provider_gone = gone.clone();
    let reg = env.service.register_decorations_provider(TestProvider::new("Test", emitter.event(), move |uri, _token| {
        if !provider_gone.get() && path_of(uri).ends_with("file.ts") {
            return sync(DecorationData { tooltip: Some("FOO".to_string()), weight: Some(17), bubble: Some(true), ..DecorationData::default() });
        }
        MaybePromise::Value(None)
    }));

    let uri = parse("foo:/folder/file.ts");
    let uri2 = parse("foo:/folder/");
    let mut data = env.service.get_decoration(&uri, true);
    assert_eq!(data.unwrap().tooltip(), "FOO");

    data = env.service.get_decoration(&uri2, true);
    assert!(!data.unwrap().tooltip().is_empty()); // emphazied items...

    gone.set(true);
    emitter.fire(vec![uri.clone()]);

    data = env.service.get_decoration(&uri, true);
    assert!(data.is_none());

    data = env.service.get_decoration(&uri2, true);
    assert!(data.is_none());

    reg.dispose();
    emitter.dispose();
}

#[test]
fn folder_decorations_dont_go_away_when_file_with_problems_is_deleted_61919_part2() {
    run_with_faked_timers(async {
        let env = Env::new();

        let emitter = Emitter::<Vec<URI>>::new();
        let gone = Rc::new(Cell::new(false));
        let provider_gone = gone.clone();
        let reg = env.service.register_decorations_provider(TestProvider::new("Test", emitter.event(), move |uri, _token| {
            if !provider_gone.get() && path_of(uri).ends_with("file.ts") {
                return sync(DecorationData { tooltip: Some("FOO".to_string()), weight: Some(17), bubble: Some(true), ..DecorationData::default() });
            }
            MaybePromise::Value(None)
        }));

        let uri = parse("foo:/folder/file.ts");
        let uri2 = parse("foo:/folder/");
        let mut data = env.service.get_decoration(&uri, true);
        assert_eq!(data.unwrap().tooltip(), "FOO");

        data = env.service.get_decoration(&uri2, true);
        assert!(!data.unwrap().tooltip().is_empty()); // emphazied items...

        let (promise, resolver) = Promise::<()>::new();
        let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        {
            let (uri, uri2, own_listener, reg, resolver) = (uri.clone(), uri2.clone(), listener.clone(), reg.clone(), resolver);
            *listener.borrow_mut() = Some(env.service.on_did_change_decorations().listen(move |e| {
                if let Some(l) = own_listener.borrow_mut().take() {
                    l.dispose();
                }
                if e.affects_resource(&uri) && e.affects_resource(&uri2) {
                    resolver.resolve(());
                } else {
                    resolver.reject(je_base::errors::Error::new("a mudança não afeta os dois recursos"));
                }
                reg.dispose();
            }));
        }
        gone.set(true);
        emitter.fire(vec![uri.clone()]);
        promise.await.unwrap();
        emitter.dispose();
    });
}

#[test]
fn file_decoration_provider_intermittently_fails_133210() {
    let env = Env::new();

    let invoke_order: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    for label in ["Provider-1", "Provider-2"] {
        let invoke_order = invoke_order.clone();
        env.guard.add(env.service.register_decorations_provider(TestProvider::new(label, none(), move |_uri, _token| {
            invoke_order.borrow_mut().push(label.to_string());
            MaybePromise::Value(None)
        })));
    }

    env.service.get_decoration(&parse("test://me/path"), false);

    assert_eq!(*invoke_order.borrow(), vec!["Provider-2".to_string(), "Provider-1".to_string()]);
}
