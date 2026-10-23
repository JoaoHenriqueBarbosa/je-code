//! Porte de `src/vs/platform/webContentExtractor/test/electron-main/webContentCache.test.ts` (suíte `WebContentCache`), caso a caso e na mesma ordem.
//!
//! Divergências: o `Date.now` trocado à mão pelos testes de expiração é o relógio de `je_base::runtime`
//! (`with_faked_timers` e `set_system_time`, que mexe só no `Date.now()`); o `currentTime += x` é um novo
//! `set_system_time` com o tempo inicial somado. `ensureNoDisposablesAreLeakedInTestSuite` não tem o que
//! vigiar (o cache não cria nada descartável). `assert.deepStrictEqual` é `assert_eq!` sobre
//! `Option<WebContentExtractResult>`.

use je_base::runtime::{set_system_time, with_faked_timers};
use je_base::uri::URI;
use je_platform::web_content_extractor::web_content_cache::WebContentCache;
use je_platform::web_content_extractor::web_content_extractor::{WebContentExtractResult, WebContentExtractorOptions};

const START_TIME: u64 = 1_000_000;
const HOUR: u64 = 1000 * 60 * 60;
const MINUTE: u64 = 1000 * 60;

fn uri(text: &str) -> URI {
    URI::parse(text, false)
}

fn ok(result: &str, title: &str) -> WebContentExtractResult {
    WebContentExtractResult::Ok { result: result.to_owned(), title: Some(title.to_owned()) }
}

fn error(message: &str, status_code: u32) -> WebContentExtractResult {
    WebContentExtractResult::Error { error: message.to_owned(), status_code: Some(status_code), result: None, title: None }
}

fn follow_redirects(value: bool) -> WebContentExtractorOptions {
    WebContentExtractorOptions { follow_redirects: Some(value), trusted_domains: None }
}

//#region Basic Cache Operations

#[test]
fn returns_undefined_for_uncached_uri() {
    let cache = WebContentCache::new();
    let uri = uri("https://example.com/page");
    let result = cache.try_get(&uri, None);
    assert_eq!(result, None);
}

#[test]
fn returns_cached_result_for_previously_added_uri() {
    let cache = WebContentCache::new();
    let uri = uri("https://example.com/page");
    let extract_result = ok("Test content", "Test Title");

    cache.add(&uri, None, extract_result.clone());
    let cached = cache.try_get(&uri, None);

    assert_eq!(cached, Some(extract_result));
}

#[test]
fn returns_cached_ok_result() {
    let cache = WebContentCache::new();
    let uri = uri("https://example.com/page");
    let extract_result = ok("Content", "Title");

    cache.add(&uri, None, extract_result.clone());
    let cached = cache.try_get(&uri, None);

    assert_eq!(cached, Some(extract_result));
}

#[test]
fn returns_cached_redirect_result() {
    let cache = WebContentCache::new();
    let uri = uri("https://example.com/old");
    let redirect_uri = self::uri("https://example.com/new");
    let extract_result = WebContentExtractResult::Redirect { to_uri: redirect_uri };

    cache.add(&uri, None, extract_result.clone());
    let cached = cache.try_get(&uri, None);

    assert_eq!(cached, Some(extract_result));
}

#[test]
fn returns_cached_error_result() {
    let cache = WebContentCache::new();
    let uri = uri("https://example.com/error");
    let extract_result = error("Not found", 404);

    cache.add(&uri, None, extract_result.clone());
    let cached = cache.try_get(&uri, None);

    assert_eq!(cached, Some(extract_result));
}

//#endregion

//#region Options-Based Cache Key

#[test]
fn different_options_produce_different_cache_entries() {
    let cache = WebContentCache::new();
    let uri = uri("https://example.com/page");
    let result_with_redirects = ok("With redirects", "Redirects Title");
    let result_without_redirects = ok("Without redirects", "No Redirects Title");

    cache.add(&uri, Some(&follow_redirects(true)), result_with_redirects.clone());
    cache.add(&uri, Some(&follow_redirects(false)), result_without_redirects.clone());

    assert_eq!(cache.try_get(&uri, Some(&follow_redirects(true))), Some(result_with_redirects));
    assert_eq!(cache.try_get(&uri, Some(&follow_redirects(false))), Some(result_without_redirects));
}

#[test]
fn undefined_options_and_follow_redirects_false_use_same_cache_key() {
    let cache = WebContentCache::new();
    let uri = uri("https://example.com/page");
    let extract_result = ok("Content", "Title");

    cache.add(&uri, None, extract_result.clone());

    // Both undefined and { followRedirects: false } should resolve to the same key
    // because !!undefined === false and !!false === false
    assert_eq!(cache.try_get(&uri, None), Some(extract_result.clone()));
    assert_eq!(cache.try_get(&uri, Some(&follow_redirects(false))), Some(extract_result));
}

//#endregion

//#region URI Case Sensitivity

#[test]
fn uri_path_case_is_ignored_for_cache_lookup() {
    let cache = WebContentCache::new();
    let uri1 = uri("https://example.com/Page");
    let uri2 = uri("https://example.com/page");
    let extract_result = ok("Content", "Title");

    cache.add(&uri1, None, extract_result.clone());

    // extUriIgnorePathCase should make these equivalent
    assert_eq!(cache.try_get(&uri2, None), Some(extract_result));
}

//#endregion

//#region Cache Expiration

#[test]
fn expired_success_entries_are_not_returned() {
    with_faked_timers(|| {
        let cache = WebContentCache::new();
        let uri = uri("https://example.com/page");
        let extract_result = ok("Content", "Title");

        // Mock Date.now to control expiration
        set_system_time(START_TIME);
        cache.add(&uri, None, extract_result);

        // Move time forward past the 24-hour success cache duration
        set_system_time(START_TIME + 24 * HOUR + 1); // 24 hours + 1ms

        let cached = cache.try_get(&uri, None);
        assert_eq!(cached, None);
    });
}

#[test]
fn expired_error_entries_are_not_returned() {
    with_faked_timers(|| {
        let cache = WebContentCache::new();
        let uri = uri("https://example.com/error");
        let extract_result = error("Server error", 500);

        set_system_time(START_TIME);
        cache.add(&uri, None, extract_result);

        // Move time forward past the 5-minute error cache duration
        set_system_time(START_TIME + 5 * MINUTE + 1); // 5 minutes + 1ms

        let cached = cache.try_get(&uri, None);
        assert_eq!(cached, None);
    });
}

#[test]
fn non_expired_success_entries_are_returned() {
    with_faked_timers(|| {
        let cache = WebContentCache::new();
        let uri = uri("https://example.com/page");
        let extract_result = ok("Content", "Title");

        set_system_time(START_TIME);
        cache.add(&uri, None, extract_result.clone());

        // Move time forward but stay within the 24-hour success cache duration
        set_system_time(START_TIME + 23 * HOUR); // 23 hours

        let cached = cache.try_get(&uri, None);
        assert_eq!(cached, Some(extract_result));
    });
}

#[test]
fn non_expired_error_entries_are_returned() {
    with_faked_timers(|| {
        let cache = WebContentCache::new();
        let uri = uri("https://example.com/error");
        let extract_result = error("Server error", 500);

        set_system_time(START_TIME);
        cache.add(&uri, None, extract_result.clone());

        // Move time forward but stay within the 5-minute error cache duration
        set_system_time(START_TIME + 4 * MINUTE); // 4 minutes

        let cached = cache.try_get(&uri, None);
        assert_eq!(cached, Some(extract_result));
    });
}

#[test]
fn redirect_results_use_success_cache_duration() {
    with_faked_timers(|| {
        let cache = WebContentCache::new();
        let uri = uri("https://example.com/old");
        let extract_result = WebContentExtractResult::Redirect { to_uri: self::uri("https://example.com/new") };

        set_system_time(START_TIME);
        cache.add(&uri, None, extract_result.clone());

        // Move time forward past error duration but within success duration
        set_system_time(START_TIME + HOUR); // 1 hour (past 5 min error, within 24 hour success)

        let cached = cache.try_get(&uri, None);
        assert_eq!(cached, Some(extract_result));
    });
}

//#endregion

//#region Cache Overwrite

#[test]
fn adding_same_uri_overwrites_previous_entry() {
    let cache = WebContentCache::new();
    let uri = uri("https://example.com/page");
    let first_result = ok("First content", "First Title");
    let second_result = ok("Second content", "Second Title");

    cache.add(&uri, None, first_result);
    cache.add(&uri, None, second_result.clone());

    let cached = cache.try_get(&uri, None);
    assert_eq!(cached, Some(second_result));
}

//#endregion

//#region Different URI Components

#[test]
fn different_hosts_produce_different_cache_entries() {
    let cache = WebContentCache::new();
    let uri1 = uri("https://example.com/page");
    let uri2 = uri("https://other.com/page");
    let result1 = ok("Example content", "Example Title");
    let result2 = ok("Other content", "Other Title");

    cache.add(&uri1, None, result1.clone());
    cache.add(&uri2, None, result2.clone());

    assert_eq!(cache.try_get(&uri1, None), Some(result1));
    assert_eq!(cache.try_get(&uri2, None), Some(result2));
}

#[test]
fn different_paths_produce_different_cache_entries() {
    let cache = WebContentCache::new();
    let uri1 = uri("https://example.com/page1");
    let uri2 = uri("https://example.com/page2");
    let result1 = ok("Page 1 content", "Page 1 Title");
    let result2 = ok("Page 2 content", "Page 2 Title");

    cache.add(&uri1, None, result1.clone());
    cache.add(&uri2, None, result2.clone());

    assert_eq!(cache.try_get(&uri1, None), Some(result1));
    assert_eq!(cache.try_get(&uri2, None), Some(result2));
}

#[test]
fn different_query_strings_produce_different_cache_entries() {
    let cache = WebContentCache::new();
    let uri1 = uri("https://example.com/page?a=1");
    let uri2 = uri("https://example.com/page?a=2");
    let result1 = ok("Query 1 content", "Query 1 Title");
    let result2 = ok("Query 2 content", "Query 2 Title");

    cache.add(&uri1, None, result1.clone());
    cache.add(&uri2, None, result2.clone());

    assert_eq!(cache.try_get(&uri1, None), Some(result1));
    assert_eq!(cache.try_get(&uri2, None), Some(result2));
}

//#endregion
