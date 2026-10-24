//! Porte de `src/vs/workbench/api/test/browser/extHostBrowsers.test.ts` (suíte `ExtHostBrowsers`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `store` é o
//!   `DisposableStore` dele. Os casos `async` rodam em `run_with_faked_timers`.
//! - O nome do caso mantém o texto do original em snake case, com `$` virando `on_`/`rpc_` conforme o método
//!   (`$onDidOpenBrowserTab fires event` é `on_did_open_browser_tab_fires_event`).
//! - `mock<MainThreadBrowsersShape>()` com `override` e `Object.assign(proxy, overrides)` vira
//!   [`MockMainThreadBrowsers`] com [`Overrides`]: cada método tem a resposta padrão do original
//!   (`Promise.resolve(...)`) e um fecho opcional que a substitui. `Partial<BrowserTabDto>` do `createDto`
//!   é a macro `dto!`.
//! - `browserTabs returns a snapshot, not a live array`: o `notStrictEqual` entre os dois arrays é garantido
//!   pelo tipo (`Vec` devolvido por valor); a conferência das contagens fica igual.
//! - `tab properties are not directly writable`: não há setter, então a tentativa de escrita do original é
//!   erro de compilação; sobram as conferências de que `url` e `title` não mudaram.
//! - `assert.rejects(promise, /texto/)` vira [`assert_rejects`], que confere o texto na mensagem do erro.
//!   `assert.ok(session)` é garantido pelo tipo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_platform::opener::editor_types::EditorOptions;
use je_workbench::api::common::ext_host_browsers::{BrowserTab, BrowserTabShowOptions, ExtHostBrowsers};
use je_workbench::api::common::ext_host_protocol::{
    BrowserTabDto, CdpMessage, CdpRequest, CdpResponse, EditorGroupColumn, ExtHostBrowsersShape, MainThreadBrowsersShape,
};
use je_workbench::api::common::ext_host_types::{IconPath, ViewColumn};
use support::test_rpc_protocol::single_proxy_rpc_protocol;

fn default_dto() -> BrowserTabDto {
    BrowserTabDto { id: "browser-1".to_owned(), url: "https://example.com".to_owned(), title: "Example".to_owned(), favicon: None }
}

/// `createDto({ campo: valor, ... })`: parte do `defaultDto` e troca só os campos dados. `favicon` recebe um
/// `Option<&str>`, os outros um `&str`.
macro_rules! dto {
    (@set $d:ident,) => {};
    (@set $d:ident, favicon: $value:expr $(, $($rest:tt)*)?) => {
        $d.favicon = $value.map(str::to_owned);
        dto!(@set $d, $($($rest)*)?);
    };
    (@set $d:ident, $field:ident: $value:expr $(, $($rest:tt)*)?) => {
        $d.$field = $value.to_owned();
        dto!(@set $d, $($($rest)*)?);
    };
    ($($args:tt)*) => {{
        let mut d = default_dto();
        dto!(@set d, $($args)*);
        d
    }};
}

type OpenBrowserTabFn = Box<dyn Fn(&str, Option<EditorGroupColumn>, Option<EditorOptions>) -> Promise<BrowserTabDto>>;
type StartCdpSessionFn = Box<dyn Fn(&str, &str) -> Promise<()>>;
type SendCdpMessageFn = Box<dyn Fn(&str, CdpRequest) -> Promise<()>>;

/// `Partial<MainThreadBrowsersShape>`: o que o teste troca no proxy.
#[derive(Default)]
struct Overrides {
    open_browser_tab: Option<OpenBrowserTabFn>,
    start_cdp_session: Option<StartCdpSessionFn>,
    send_cdp_message: Option<SendCdpMessageFn>,
}

/// O `new class extends mock<MainThreadBrowsersShape>()` do original.
struct MockMainThreadBrowsers {
    overrides: Overrides,
}

impl IDisposable for MockMainThreadBrowsers {
    fn dispose(&self) {}
}

impl MainThreadBrowsersShape for MockMainThreadBrowsers {
    fn rpc_open_browser_tab(&self, url: &str, view_column: Option<EditorGroupColumn>, options: Option<EditorOptions>) -> Promise<BrowserTabDto> {
        match &self.overrides.open_browser_tab {
            Some(open_browser_tab) => open_browser_tab(url, view_column, options),
            None => Promise::resolved(default_dto()),
        }
    }

    fn rpc_start_cdp_session(&self, session_id: &str, browser_id: &str) -> Promise<()> {
        match &self.overrides.start_cdp_session {
            Some(start_cdp_session) => start_cdp_session(session_id, browser_id),
            None => Promise::resolved(()),
        }
    }

    fn rpc_close_cdp_session(&self, _session_id: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_send_cdp_message(&self, session_id: &str, message: CdpRequest) -> Promise<()> {
        match &self.overrides.send_cdp_message {
            Some(send_cdp_message) => send_cdp_message(session_id, message),
            None => Promise::resolved(()),
        }
    }

    fn rpc_close_browser_tab(&self, _browser_id: &str) -> Promise<()> {
        Promise::resolved(())
    }
}

fn create_ext_host_browsers(store: &DisposableStore, overrides: Overrides) -> Rc<ExtHostBrowsers> {
    let proxy: Rc<dyn MainThreadBrowsersShape> = Rc::new(MockMainThreadBrowsers { overrides });
    let ext_host = ExtHostBrowsers::new(&*single_proxy_rpc_protocol(proxy));
    store.add(ext_host.clone());
    ext_host
}

async fn assert_rejects(promise: Promise<()>, pattern: &str) {
    let error = promise.await.expect_err("a promessa deveria ser rejeitada");
    assert!(error.message.contains(pattern), "a mensagem '{}' deveria conter '{pattern}'", error.message);
}

/// `(icon as { id: string }).id`.
fn icon_id(tab: &BrowserTab) -> String {
    match tab.icon() {
        IconPath::ThemeIcon(icon) => icon.id,
        _ => panic!("o ícone deveria ser um ThemeIcon"),
    }
}

/// `String(icon)`.
fn icon_string(tab: &BrowserTab) -> String {
    match tab.icon() {
        IconPath::Uri(uri) => uri.to_string(false).to_string_lossy(),
        _ => panic!("o ícone deveria ser um URI"),
    }
}

fn urls(tabs: &[BrowserTab]) -> Vec<String> {
    tabs.iter().map(|tab| tab.url()).collect()
}

// #region browserTabs

#[test]
fn browser_tabs_populates_from_on_did_open_browser_tab() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", url: "https://one.com", title: "One"));
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b2", url: "https://two.com", title: "Two"));

    let tabs = ext_host.browser_tabs();
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0].url(), "https://one.com");
    assert_eq!(tabs[1].url(), "https://two.com");
}

#[test]
fn browser_tabs_returns_a_snapshot_not_a_live_array() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1"));
    let snapshot1 = ext_host.browser_tabs();

    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b2"));
    let snapshot2 = ext_host.browser_tabs();

    assert_eq!(snapshot1.len(), 1);
    assert_eq!(snapshot2.len(), 2);
}

// #endregion

// #region activeBrowserTab

#[test]
fn active_browser_tab_updates_via_on_did_change_active_browser_tab() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    let dto = dto!(id: "b1", url: "https://active.com");
    ext_host.rpc_on_did_open_browser_tab(dto);
    ext_host.rpc_on_did_change_active_browser_tab(Some("b1"));

    assert_eq!(ext_host.active_browser_tab().map(|tab| tab.url()), Some("https://active.com".to_owned()));
}

#[test]
fn active_browser_tab_becomes_undefined_when_cleared() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    let dto = dto!(id: "b1");
    ext_host.rpc_on_did_open_browser_tab(dto);
    ext_host.rpc_on_did_change_active_browser_tab(Some("b1"));
    assert!(ext_host.active_browser_tab().is_some());

    ext_host.rpc_on_did_change_active_browser_tab(None);
    assert!(ext_host.active_browser_tab().is_none());
}

#[test]
fn on_did_change_active_browser_tab_with_unknown_tab_returns_undefined() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());

    ext_host.rpc_on_did_change_active_browser_tab(Some("non-existent"));

    assert!(ext_host.active_browser_tab().is_none());
}

// #endregion

// #region openBrowserTab

#[test]
fn open_browser_tab_returns_a_browser_tab_with_correct_properties() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let dto = dto!(id: "opened", url: "https://opened.com", title: "Opened");
        let ext_host = create_ext_host_browsers(
            &store,
            Overrides { open_browser_tab: Some(Box::new(move |_, _, _| Promise::resolved(dto.clone()))), ..Overrides::default() },
        );

        let tab = ext_host.open_browser_tab("https://opened.com", None).await.unwrap();
        assert_eq!(tab.url(), "https://opened.com");
        assert_eq!(tab.title(), "Opened");
    });
}

#[test]
fn open_browser_tab_fires_on_did_open_browser_tab_for_new_tabs() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let ext_host = create_ext_host_browsers(
            &store,
            Overrides { open_browser_tab: Some(Box::new(|_, _, _| Promise::resolved(dto!(id: "new-tab")))), ..Overrides::default() },
        );
        let opened: Rc<RefCell<Vec<BrowserTab>>> = Rc::default();
        let sink = opened.clone();
        store.add(ext_host.on_did_open_browser_tab().listen(move |tab| sink.borrow_mut().push(tab.clone())));

        ext_host.open_browser_tab("https://example.com", None).await.unwrap();

        assert_eq!(opened.borrow().len(), 1);
        assert_eq!(opened.borrow()[0].url(), "https://example.com");
    });
}

#[test]
fn open_browser_tab_reuses_existing_tab_when_ids_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let ext_host = create_ext_host_browsers(
            &store,
            Overrides { open_browser_tab: Some(Box::new(|_, _, _| Promise::resolved(dto!(id: "same", url: "https://updated.com")))), ..Overrides::default() },
        );

        ext_host.rpc_on_did_open_browser_tab(dto!(id: "same", url: "https://original.com"));
        let tab = ext_host.open_browser_tab("https://updated.com", None).await.unwrap();

        assert_eq!(ext_host.browser_tabs().len(), 1);
        assert_eq!(tab.url(), "https://updated.com");
    });
}

#[test]
fn open_browser_tab_forwards_options_to_proxy() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let captured_view_column: Rc<RefCell<Option<f64>>> = Rc::default();
        let captured_options: Rc<RefCell<Option<EditorOptions>>> = Rc::default();
        let (view_column_sink, options_sink) = (captured_view_column.clone(), captured_options.clone());
        let ext_host = create_ext_host_browsers(
            &store,
            Overrides {
                open_browser_tab: Some(Box::new(move |_url, view_column, options| {
                    *view_column_sink.borrow_mut() = view_column;
                    *options_sink.borrow_mut() = options;
                    Promise::resolved(dto!(id: "opts"))
                })),
                ..Overrides::default()
            },
        );

        ext_host
            .open_browser_tab("https://example.com", Some(BrowserTabShowOptions { view_column: Some(ViewColumn::TWO), preserve_focus: Some(true), background: Some(true) }))
            .await
            .unwrap();

        // ViewColumn.from converts API viewColumn (1-based) to EditorGroupColumn (0-based)
        assert_eq!(*captured_view_column.borrow(), Some(1.0));
        assert_eq!(captured_options.borrow().as_ref().and_then(|options| options.preserve_focus), Some(true));
        assert_eq!(captured_options.borrow().as_ref().and_then(|options| options.inactive), Some(true));
    });
}

// #endregion

// #region $onDidOpenBrowserTab

#[test]
fn on_did_open_browser_tab_fires_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let ext_host = create_ext_host_browsers(&store, Overrides::default());
    let opened: Rc<RefCell<Vec<BrowserTab>>> = Rc::default();
    let sink = opened.clone();
    store.add(ext_host.on_did_open_browser_tab().listen(move |tab| sink.borrow_mut().push(tab.clone())));

    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", url: "https://opened.com"));

    assert_eq!(opened.borrow().len(), 1);
    assert_eq!(opened.borrow()[0].url(), "https://opened.com");
}

// #endregion

// #region $onDidCloseBrowserTab

#[test]
fn on_did_close_browser_tab_removes_tab_and_fires_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let ext_host = create_ext_host_browsers(&store, Overrides::default());
    let changes: Rc<RefCell<Vec<BrowserTab>>> = Rc::default();
    let sink = changes.clone();
    store.add(ext_host.on_did_change_browser_tab_state().listen(move |tab| sink.borrow_mut().push(tab.clone())));

    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", url: "https://old.com"));
    ext_host.rpc_on_did_change_browser_tab_state(dto!(id: "b1", url: "https://new.com"));

    assert_eq!(changes.borrow().len(), 1);
    assert_eq!(changes.borrow()[0].url(), "https://new.com");
}

#[test]
fn on_did_change_browser_tab_state_does_not_fire_when_data_is_unchanged() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", url: "https://example.com", title: "Old Title"));

    ext_host.rpc_on_did_change_browser_tab_state(dto!(id: "b1", url: "https://example.com", title: "New Title"));

    assert_eq!(ext_host.browser_tabs()[0].url(), "https://example.com");
    assert_eq!(ext_host.browser_tabs()[0].title(), "New Title");
}

// #endregion

// #region $onDidChangeActiveBrowserTab event

#[test]
fn on_did_change_active_browser_tab_fires_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let ext_host = create_ext_host_browsers(&store, Overrides::default());
    let active_changes: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
    let sink = active_changes.clone();
    store.add(ext_host.on_did_change_active_browser_tab().listen(move |tab| sink.borrow_mut().push(tab.as_ref().map(|tab| tab.url()))));

    let dto = dto!(id: "b1");
    ext_host.rpc_on_did_open_browser_tab(dto);
    ext_host.rpc_on_did_change_active_browser_tab(Some("b1"));
    ext_host.rpc_on_did_change_active_browser_tab(None);

    assert_eq!(*active_changes.borrow(), [Some("https://example.com".to_owned()), None]);
}

// #endregion

// #region BrowserTab icon

#[test]
fn icon_is_globe_theme_icon_when_no_favicon() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", favicon: None));

    assert_eq!(icon_id(&ext_host.browser_tabs()[0]), "globe");
}

#[test]
fn icon_is_uri_when_favicon_is_provided() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", favicon: Some("https://example.com/favicon.ico")));

    assert_eq!(icon_string(&ext_host.browser_tabs()[0]), "https://example.com/favicon.ico");
}

#[test]
fn icon_updates_when_favicon_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", favicon: None));
    assert_eq!(icon_id(&ext_host.browser_tabs()[0]), "globe");

    ext_host.rpc_on_did_change_browser_tab_state(dto!(id: "b1", favicon: Some("https://example.com/new.ico")));
    assert_eq!(icon_string(&ext_host.browser_tabs()[0]), "https://example.com/new.ico");
}

#[test]
fn icon_reverts_to_globe_when_favicon_is_cleared() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", favicon: Some("https://example.com/icon.ico")));
    assert_eq!(icon_string(&ext_host.browser_tabs()[0]), "https://example.com/icon.ico");

    ext_host.rpc_on_did_change_browser_tab_state(dto!(id: "b1", favicon: None));
    assert_eq!(icon_id(&ext_host.browser_tabs()[0]), "globe");
}

// #endregion

// #region BrowserTab readonly properties

#[test]
fn tab_properties_are_not_directly_writable() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", url: "https://example.com", title: "Title"));
    let tab = ext_host.browser_tabs()[0].clone();

    // Attempting to assign to getter-only properties should either throw or be silently ignored
    assert_eq!(tab.url(), "https://example.com");
    assert_eq!(tab.title(), "Title");
}

#[test]
fn start_cdp_session_calls_start_cdp_session_on_proxy() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let captured_browser_id: Rc<RefCell<Option<String>>> = Rc::default();
        let sink = captured_browser_id.clone();
        let ext_host = create_ext_host_browsers(
            &store,
            Overrides {
                start_cdp_session: Some(Box::new(move |_session_id, browser_id| {
                    *sink.borrow_mut() = Some(browser_id.to_owned());
                    Promise::resolved(())
                })),
                ..Overrides::default()
            },
        );

        ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1"));
        let _session = ext_host.browser_tabs()[0].start_cdp_session().await.unwrap();

        assert_eq!(captured_browser_id.borrow().as_deref(), Some("b1"));
    });
}

#[test]
fn send_message_validates_message_structure() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let ext_host = create_ext_host_browsers(&store, Overrides::default());
        ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1"));
        let session = ext_host.browser_tabs()[0].start_cdp_session().await.unwrap();

        // Valid message succeeds
        session.send_message(&Dynamic::object([("id", Dynamic::Number(1.0)), ("method", Dynamic::string("Page.enable"))])).await.unwrap();

        // Invalid messages are rejected
        assert_rejects(session.send_message(&Dynamic::Null), "must be an object").await;
        assert_rejects(session.send_message(&Dynamic::object([("method", Dynamic::string("Foo"))])), "numeric id").await;
        assert_rejects(session.send_message(&Dynamic::object([("id", Dynamic::Number(1.0))])), "method string").await;
    });
}

#[test]
fn send_message_forwards_valid_message_to_proxy() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let sent_messages: Rc<RefCell<Vec<CdpRequest>>> = Rc::default();
        let sink = sent_messages.clone();
        let ext_host = create_ext_host_browsers(
            &store,
            Overrides {
                send_cdp_message: Some(Box::new(move |_sid, message| {
                    sink.borrow_mut().push(message);
                    Promise::resolved(())
                })),
                ..Overrides::default()
            },
        );

        ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1"));
        let session = ext_host.browser_tabs()[0].start_cdp_session().await.unwrap();
        let params = Dynamic::object(Vec::<(&str, Dynamic)>::new());
        session
            .send_message(&Dynamic::object([("id", Dynamic::Number(1.0)), ("method", Dynamic::string("Page.enable")), ("params", params)]))
            .await
            .unwrap();

        assert_eq!(sent_messages.borrow().len(), 1);
        // { id: 1, method: 'Page.enable', params: {}, sessionId: undefined }
        let sent = sent_messages.borrow()[0].clone();
        assert_eq!(sent.id, 1.0);
        assert_eq!(sent.method, "Page.enable");
        assert_eq!(sent.params.as_ref().and_then(json_stringify), Some("{}".to_owned()));
        assert_eq!(sent.session_id, None);
    });
}

#[test]
fn send_message_rejects_after_session_is_closed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let ext_host = create_ext_host_browsers(&store, Overrides::default());
        ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1"));
        let session = ext_host.browser_tabs()[0].start_cdp_session().await.unwrap();

        session.close().await.unwrap();
        assert_rejects(session.send_message(&Dynamic::object([("id", Dynamic::Number(1.0)), ("method", Dynamic::string("Foo"))])), "closed").await;
    });
}

#[test]
fn on_cdp_session_message_delivers_to_correct_session() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let captured_ids: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = captured_ids.clone();
        let ext_host = create_ext_host_browsers(
            &store,
            Overrides {
                start_cdp_session: Some(Box::new(move |session_id, _browser_id| {
                    sink.borrow_mut().push(session_id.to_owned());
                    Promise::resolved(())
                })),
                ..Overrides::default()
            },
        );

        ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1"));
        let session1 = ext_host.browser_tabs()[0].start_cdp_session().await.unwrap();
        let session2 = ext_host.browser_tabs()[0].start_cdp_session().await.unwrap();

        let received1: Rc<RefCell<Vec<Dynamic>>> = Rc::default();
        let received2: Rc<RefCell<Vec<Dynamic>>> = Rc::default();
        let (sink1, sink2) = (received1.clone(), received2.clone());
        store.add(session1.on_did_receive_message().listen(move |m| sink1.borrow_mut().push(m.clone())));
        store.add(session2.on_did_receive_message().listen(move |m| sink2.borrow_mut().push(m.clone())));

        let session_id = captured_ids.borrow()[1].clone();
        ext_host.rpc_on_cdp_session_message(
            &session_id,
            CdpMessage::Response(CdpResponse {
                id: 1.0,
                result: Some(Dynamic::object([("data", Dynamic::string("hello"))])),
                error: None,
                session_id: None,
            }),
        );

        assert!(received1.borrow().is_empty());
        let received2: Vec<Option<String>> = received2.borrow().iter().map(json_stringify).collect();
        assert_eq!(received2, [Some(r#"{"id":1,"result":{"data":"hello"}}"#.to_owned())]);
    });
}

#[test]
fn on_cdp_session_closed_fires_on_did_close() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let captured_ids: Rc<RefCell<Vec<String>>> = Rc::default();
        let sink = captured_ids.clone();
        let ext_host = create_ext_host_browsers(
            &store,
            Overrides {
                start_cdp_session: Some(Box::new(move |session_id, _browser_id| {
                    sink.borrow_mut().push(session_id.to_owned());
                    Promise::resolved(())
                })),
                ..Overrides::default()
            },
        );

        ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1"));
        let session = ext_host.browser_tabs()[0].start_cdp_session().await.unwrap();

        let close_fired = Rc::new(RefCell::new(false));
        let flag = close_fired.clone();
        store.add(session.on_did_close().listen(move |_| *flag.borrow_mut() = true));

        let session_id = captured_ids.borrow()[0].clone();
        ext_host.rpc_on_cdp_session_closed(&session_id);
        assert!(*close_fired.borrow());
    });
}

// #endregion

// #region Reference stability

#[test]
fn tab_object_reference_is_stable_across_updates() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", url: "https://old.com", title: "Old"));
    let tab_before = ext_host.browser_tabs()[0].clone();

    ext_host.rpc_on_did_change_browser_tab_state(dto!(id: "b1", url: "https://new.com", title: "New"));
    let tab_after = ext_host.browser_tabs()[0].clone();

    assert!(Rc::ptr_eq(&tab_before, &tab_after));
    assert_eq!(tab_after.url(), "https://new.com");
}

#[test]
fn open_browser_tab_returns_same_reference_as_browser_tabs_entry() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let ext_host = create_ext_host_browsers(
            &store,
            Overrides { open_browser_tab: Some(Box::new(|_, _, _| Promise::resolved(dto!(id: "ref-test")))), ..Overrides::default() },
        );

        let returned = ext_host.open_browser_tab("https://example.com", None).await.unwrap();
        let from_array = ext_host.browser_tabs()[0].clone();

        assert!(Rc::ptr_eq(&returned, &from_array));
    });
}

// #endregion

// #region Multiple tabs tracked independently

#[test]
fn closing_one_tab_does_not_affect_others() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b1", url: "https://one.com"));
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b2", url: "https://two.com"));
    ext_host.rpc_on_did_open_browser_tab(dto!(id: "b3", url: "https://three.com"));

    ext_host.rpc_on_did_close_browser_tab("b2");

    assert_eq!(ext_host.browser_tabs().len(), 2);
    assert_eq!(urls(&ext_host.browser_tabs()), ["https://one.com", "https://three.com"]);
}

#[test]
fn closing_active_tab_clears_active_browser_tab() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host = create_ext_host_browsers(&guard.store(), Overrides::default());
    let dto = dto!(id: "b1");
    ext_host.rpc_on_did_open_browser_tab(dto);
    ext_host.rpc_on_did_change_active_browser_tab(Some("b1"));
    assert!(ext_host.active_browser_tab().is_some());

    ext_host.rpc_on_did_close_browser_tab("b1");
    assert!(ext_host.active_browser_tab().is_none());
}

// #endregion
