//! Porte de `src/vs/platform/browserView/test/common/browserPermissions.test.ts` (suítes
//! `BrowserPermissionStore`, `electronPermissionToCategories`, `isAlwaysAllowedPermission` e
//! `toOriginKey`), caso a caso e na mesma ordem.
//!
//! Divergências: `store.hydrate(snapshot)` recebe o valor dinâmico, então o snapshot tipado passa por
//! `to_dynamic()` e o literal com categoria desconhecida é `j("{...}")`. `deepStrictEqual` sobre
//! `serialize()` compara [`SerializedBrowserPermissionsSnapshot`] (a ordem das chaves não conta, como
//! no original). `ensureNoDisposablesAreLeakedInTestSuite` é o [`LeakGuard`] de cada teste.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, j};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_platform::browser_view::browser_permissions::{
    BrowserPermissionStore, MediaKind, PermissionCategory, PermissionCategoryState, PermissionDecision, SerializedBrowserPermissionsSnapshot,
    electron_permission_to_categories, is_always_allowed_permission, to_origin_key,
};

fn guard() -> LeakGuard {
    ensure_no_disposables_are_leaked_in_test_suite()
}

/// `{ origins: { origem: { categoria: decisão } } }`.
fn snapshot(origins: &[(&str, &[(PermissionCategory, PermissionDecision)])]) -> SerializedBrowserPermissionsSnapshot {
    SerializedBrowserPermissionsSnapshot {
        origins: origins.iter().map(|(origin, categories)| ((*origin).to_owned(), categories.iter().copied().collect())).collect(),
    }
}

fn grant(category: PermissionCategory, state: PermissionDecision) -> PermissionCategoryState {
    PermissionCategoryState { category, state: Some(state) }
}

#[test]
fn get_set_clear_and_origin_normalization() {
    let _guard = guard();
    let store = BrowserPermissionStore::new();

    assert_eq!(store.get_decision("https://example.com", PermissionCategory::Camera), None);

    store.set("https://example.com/some/path?q=1", PermissionCategory::Camera, Some(PermissionDecision::Allow));
    // Path/query are stripped down to the origin key.
    assert_eq!(store.get_decision("https://example.com", PermissionCategory::Camera), Some(PermissionDecision::Allow));
    assert_eq!(store.get_decision("https://example.com:443/x", PermissionCategory::Camera), Some(PermissionDecision::Allow));

    // Passing null clears the recorded decision, falling back to the default.
    store.set("https://example.com", PermissionCategory::Camera, None);
    assert_eq!(store.get_decision("https://example.com", PermissionCategory::Camera), None);
    assert_eq!(store.origins(), Vec::<String>::new());

    store.dispose();
}

#[test]
fn is_allowed_falls_back_to_per_category_default_state_when_unset() {
    let _guard = guard();
    let store = BrowserPermissionStore::new();

    // Categories that prompt default to 'ask' (not allowed); some categories
    // (e.g. Sensors) default to 'allow'.
    assert!(!store.is_allowed("https://a.com", PermissionCategory::Location));
    assert!(store.is_allowed("https://a.com", PermissionCategory::Sensors));
    // Devices default to 'allow' (the chooser itself is the consent gesture).
    assert!(store.is_allowed("https://a.com", PermissionCategory::Devices));

    store.set("https://a.com", PermissionCategory::Location, Some(PermissionDecision::Allow));
    store.set("https://a.com", PermissionCategory::Sensors, Some(PermissionDecision::Deny));
    assert!(store.is_allowed("https://a.com", PermissionCategory::Location));
    assert!(!store.is_allowed("https://a.com", PermissionCategory::Sensors));

    store.dispose();
}

#[test]
fn on_did_change_fires_only_on_real_changes() {
    let _guard = guard();
    let store = BrowserPermissionStore::new();
    let changes = Rc::new(Cell::new(0));
    let counter = changes.clone();
    let listener = store.on_did_change().listen(move |_| counter.set(counter.get() + 1));

    store.set("https://a.com", PermissionCategory::Camera, Some(PermissionDecision::Allow));
    store.set("https://a.com", PermissionCategory::Camera, Some(PermissionDecision::Allow)); // no-op
    store.set_many(
        "https://a.com",
        [
            grant(PermissionCategory::Microphone, PermissionDecision::Deny),
            grant(PermissionCategory::Camera, PermissionDecision::Allow), // no-op
        ],
    );
    store.clear_origin("https://b.com"); // nothing recorded -> no-op

    assert_eq!(changes.get(), 2);
    listener.dispose();
    store.dispose();
}

#[test]
fn serialize_hydrate_round_trips_and_clear_origin_clear() {
    let _guard = guard();
    let store = BrowserPermissionStore::new();
    store.set_many(
        "https://a.com",
        [grant(PermissionCategory::Camera, PermissionDecision::Allow), grant(PermissionCategory::Microphone, PermissionDecision::Deny)],
    );
    store.set("https://b.com", PermissionCategory::Location, Some(PermissionDecision::Allow));

    let serialized = store.serialize();
    assert_eq!(
        serialized,
        snapshot(&[
            ("https://a.com", &[(PermissionCategory::Camera, PermissionDecision::Allow), (PermissionCategory::Microphone, PermissionDecision::Deny)]),
            ("https://b.com", &[(PermissionCategory::Location, PermissionDecision::Allow)]),
        ])
    );

    let mirror = BrowserPermissionStore::new();
    mirror.hydrate(&serialized.to_dynamic());
    assert_eq!(mirror.serialize(), serialized);

    mirror.clear_origin("https://a.com");
    assert_eq!(mirror.origins(), vec!["https://b.com".to_owned()]);
    mirror.clear();
    assert_eq!(mirror.serialize(), snapshot(&[]));

    store.dispose();
    mirror.dispose();
}

#[test]
fn hydrate_ignores_unknown_categories_and_bad_input() {
    let _guard = guard();
    let store = BrowserPermissionStore::new();
    store.hydrate(&j("{ 'origins': { 'https://a.com': { 'camera': 'allow', 'bogus': 'allow' } } }"));
    assert_eq!(store.serialize(), snapshot(&[("https://a.com", &[(PermissionCategory::Camera, PermissionDecision::Allow)])]));

    store.hydrate(&Dynamic::Undefined);
    assert_eq!(store.serialize(), snapshot(&[]));

    store.dispose();
}

#[test]
fn maps_known_permissions_and_media_disambiguation() {
    let _guard = guard();
    assert_eq!(electron_permission_to_categories("geolocation", None), vec![PermissionCategory::Location]);
    assert_eq!(electron_permission_to_categories("notifications", None), vec![PermissionCategory::Notifications]);
    assert_eq!(electron_permission_to_categories("sensors", None), vec![PermissionCategory::Sensors]);
    // USB, Serial, and HID all map to the single Devices category.
    assert_eq!(electron_permission_to_categories("usb", None), vec![PermissionCategory::Devices]);
    assert_eq!(electron_permission_to_categories("serial", None), vec![PermissionCategory::Devices]);
    assert_eq!(electron_permission_to_categories("hid", None), vec![PermissionCategory::Devices]);
    assert_eq!(electron_permission_to_categories("unrecognized", None), Vec::<PermissionCategory>::new());

    // `media` resolves from hints, defaulting to both when none are given.
    assert_eq!(electron_permission_to_categories("media", None), vec![PermissionCategory::Camera, PermissionCategory::Microphone]);
    assert_eq!(electron_permission_to_categories("media", Some(&[MediaKind::Video])), vec![PermissionCategory::Camera]);
    assert_eq!(electron_permission_to_categories("media", Some(&[MediaKind::Audio])), vec![PermissionCategory::Microphone]);
}

#[test]
fn allows_legacy_and_split_local_network_permissions() {
    let _guard = guard();
    let permissions = ["local-network-access", "local-network", "loopback-network"];
    assert_eq!(permissions.map(is_always_allowed_permission), [true, true, true]);
}

#[test]
fn does_not_bypass_managed_or_unrecognized_permissions() {
    let _guard = guard();
    let permissions = ["geolocation", "media", "unrecognized"];
    assert_eq!(permissions.map(is_always_allowed_permission), [false, false, false]);
}

#[test]
fn normalizes_urls_and_tolerates_bad_input() {
    let _guard = guard();
    assert_eq!(to_origin_key(Some("https://example.com/a/b?c=1#d")), "https://example.com");
    assert_eq!(to_origin_key(Some("http://host:8080/x")), "http://host:8080");
    // file: URLs have no real origin, so they key off scheme + full path
    // with query and fragment stripped.
    assert_eq!(to_origin_key(Some("file:///home/user/page.html?x=1#y")), "file:///home/user/page.html");
    assert_eq!(to_origin_key(Some("file:///C:/Users/me/index.html")), "file:///C:/Users/me/index.html");
    assert_eq!(to_origin_key(None), "");
    assert_eq!(to_origin_key(Some("  not a url  ")), "not a url");
    // Whitespace is trimmed before parsing so valid URLs still normalize.
    assert_eq!(to_origin_key(Some("  https://example.com/a  ")), "https://example.com");
    // Opaque origins reported as the literal "null" have no real host.
    assert_eq!(to_origin_key(Some("null")), "");
    assert_eq!(to_origin_key(Some("   ")), "");
}

#[test]
fn returns_stable_keys_for_file_inherited_and_opaque_origins() {
    let _guard = guard();
    let cases = [
        ("https://example.com:443/path?query#fragment", "https://example.com"),
        ("file:///home/user/page.html?query#fragment", "file:///home/user/page.html"),
        ("file://server/share/page.html?query#fragment", "file://server/share/page.html"),
        ("blob:https://example.com/id", "https://example.com"),
        ("about:blank", ""),
        ("about://blank", ""),
        ("about:srcdoc", ""),
        ("data:text/html,hello", ""),
        ("blob:null/id", ""),
        ("custom://host/path", ""),
        ("null", ""),
        ("", ""),
    ];

    let actual: Vec<(String, String)> = cases
        .iter()
        .map(|(url, _)| {
            let key = to_origin_key(Some(*url));
            let again = to_origin_key(Some(key.as_str()));
            (key, again)
        })
        .collect();
    let expected: Vec<(String, String)> = cases.iter().map(|(_, key)| ((*key).to_owned(), (*key).to_owned())).collect();
    assert_eq!(actual, expected);
}
