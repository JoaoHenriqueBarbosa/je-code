//! Porte de `src/vs/base/test/common/fuzzyScorer.test.ts` (suíte `Fuzzy Scorer`), caso a caso e na mesma ordem.
//!
//! A plataforma é a do processo (`Platform::detect`), que é o `isWindows` e o `sep` que o original lê dos
//! módulos globais. `URI.file` e `fsPath` usam o sabor de caminho dela. Os acessores do original viram
//! `ItemAccessor<URI>`. `scoreItem(item, query, allow, accessor, cache = Object.create(null))` são
//! `score_item` (com o cache) e `score_item_fresh` (sem). `string | undefined` do `description` é
//! `Option<JsString>`. O `scoreItem(null, null!, ...)` do caso `invalid input` passa o texto vazio, que é o
//! que o `prepareQuery` do original faz com um valor que não é string.

mod common;

use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::filters::Match;
use je_base::fuzzy_scorer::{self, FuzzyScore, FuzzyScore2, FuzzyScorerCache, ItemAccessor, ItemScore, PreparedQuery, PreparedQueryPiece};
use je_base::hash::hash;
use je_base::network::schemas;
use je_base::path::{self, PathModule, ProcessEnvironment};
use je_base::platform::Platform;
use je_base::uri::{URI, UriComponents};

fn platform() -> Platform {
    Platform::detect()
}

fn is_windows() -> bool {
    platform().is_windows
}

/// O `sep` do `path.ts`.
fn sep() -> &'static str {
    if is_windows() { "\\" } else { "/" }
}

fn flavor() -> path::Platform {
    platform().path_flavor()
}

fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn pq(query: &str) -> PreparedQuery {
    fuzzy_scorer::prepare_query(&u(query), &platform())
}

fn file(path: &str) -> URI {
    URI::file(path, flavor())
}

fn fs_path(resource: &URI) -> String {
    resource.fs_path(flavor()).to_string_lossy()
}

fn native() -> PathModule<'static> {
    PathModule::native(&ProcessEnvironment)
}

struct ResourceAccessor;

impl ItemAccessor<URI> for ResourceAccessor {
    fn get_item_label(&self, resource: &URI) -> Option<JsString> {
        Some(native().basename(&fs_path(resource), None).into())
    }

    fn get_item_description(&self, resource: &URI) -> Option<JsString> {
        Some(native().dirname(&fs_path(resource)).into())
    }

    fn get_item_path(&self, resource: &URI) -> Option<JsString> {
        Some(fs_path(resource).into())
    }
}

struct ResourceWithSlashAccessor;

impl ItemAccessor<URI> for ResourceWithSlashAccessor {
    fn get_item_label(&self, resource: &URI) -> Option<JsString> {
        ResourceAccessor.get_item_label(resource)
    }

    fn get_item_description(&self, resource: &URI) -> Option<JsString> {
        let posix = PathModule::posix(&ProcessEnvironment);
        Some(posix.normalize(&posix.dirname(&resource.path().to_string_lossy())).into())
    }

    fn get_item_path(&self, resource: &URI) -> Option<JsString> {
        Some(PathModule::posix(&ProcessEnvironment).normalize(&resource.path().to_string_lossy()).into())
    }
}

struct ResourceWithBackslashAccessor;

impl ItemAccessor<URI> for ResourceWithBackslashAccessor {
    fn get_item_label(&self, resource: &URI) -> Option<JsString> {
        ResourceAccessor.get_item_label(resource)
    }

    fn get_item_description(&self, resource: &URI) -> Option<JsString> {
        let win32 = PathModule::win32(&ProcessEnvironment);
        Some(win32.normalize(&win32.dirname(&resource.path().to_string_lossy())).into())
    }

    fn get_item_path(&self, resource: &URI) -> Option<JsString> {
        Some(PathModule::win32(&ProcessEnvironment).normalize(&resource.path().to_string_lossy()).into())
    }
}

struct NullAccessor;

impl ItemAccessor<URI> for NullAccessor {
    fn get_item_label(&self, _: &URI) -> Option<JsString> {
        None
    }

    fn get_item_description(&self, _: &URI) -> Option<JsString> {
        None
    }

    fn get_item_path(&self, _: &URI) -> Option<JsString> {
        None
    }
}

/// `resourceAccessorWithDescription(description)`.
struct ResourceAccessorWithDescription(Option<JsString>);

impl ItemAccessor<URI> for ResourceAccessorWithDescription {
    fn get_item_label(&self, resource: &URI) -> Option<JsString> {
        ResourceAccessor.get_item_label(resource)
    }

    fn get_item_description(&self, _: &URI) -> Option<JsString> {
        self.0.clone()
    }

    fn get_item_path(&self, resource: &URI) -> Option<JsString> {
        ResourceAccessor.get_item_path(resource)
    }
}

fn do_score(target: &str, query: &str, allow_non_contiguous_matches: Option<bool>) -> FuzzyScore {
    let prepared_query = pq(query);

    fuzzy_scorer::score_fuzzy(
        &u(target),
        prepared_query.normalized.units(),
        prepared_query.normalized_lowercase.units(),
        allow_non_contiguous_matches.unwrap_or(!prepared_query.expect_contiguous_match),
    )
}

fn do_score2(target: &str, query: &str, match_offset: usize) -> FuzzyScore2 {
    let prepared_query = pq(query);

    fuzzy_scorer::score_fuzzy2(&u(target), &prepared_query, 0, match_offset)
}

fn score_item_units(
    item: Option<&URI>,
    query: &[u16],
    allow_non_contiguous_matches: bool,
    accessor: &dyn ItemAccessor<URI>,
    cache: &mut FuzzyScorerCache,
) -> Rc<ItemScore> {
    fuzzy_scorer::score_item_fuzzy(item, &fuzzy_scorer::prepare_query(query, &platform()), allow_non_contiguous_matches, accessor, cache, &platform())
}

fn score_item(item: Option<&URI>, query: &str, allow_non_contiguous_matches: bool, accessor: &dyn ItemAccessor<URI>, cache: &mut FuzzyScorerCache) -> Rc<ItemScore> {
    score_item_units(item, &u(query), allow_non_contiguous_matches, accessor, cache)
}

/// `scoreItem` sem o cache do chamador (o valor padrão `Object.create(null)`).
fn score_item_fresh(item: Option<&URI>, query: &str, allow_non_contiguous_matches: bool, accessor: &dyn ItemAccessor<URI>) -> Rc<ItemScore> {
    score_item(item, query, allow_non_contiguous_matches, accessor, &mut HashMap::new())
}

fn compare_items_by_score(item_a: &URI, item_b: &URI, query: &str, allow_non_contiguous_matches: bool, accessor: &dyn ItemAccessor<URI>) -> i32 {
    fuzzy_scorer::compare_items_by_fuzzy_score(item_a, item_b, &pq(query), allow_non_contiguous_matches, accessor, &mut HashMap::new(), &platform())
}

/// `[...].sort((r1, r2) => compareItemsByScore(r1, r2, query, true, accessor))`: a ordenação é estável,
/// como a do JavaScript.
fn sort_by_score<'a, Q: AsRef<str> + ?Sized>(items: &[&'a URI], query: &Q, allow_non_contiguous_matches: bool, accessor: &dyn ItemAccessor<URI>) -> Vec<&'a URI> {
    let mut sorted = items.to_vec();
    sorted.sort_by(|r1, r2| compare_items_by_score(r1, r2, query.as_ref(), allow_non_contiguous_matches, accessor).cmp(&0));
    sorted
}

#[test]
fn score_fuzzy() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let target = "HelLo-World";

    let scores: Vec<FuzzyScore> = vec![
        do_score(target, "HelLo-World", Some(true)), // direct case match
        do_score(target, "hello-world", Some(true)), // direct mix-case match
        do_score(target, "HW", Some(true)), // direct case prefix (multiple)
        do_score(target, "hw", Some(true)), // direct mix-case prefix (multiple)
        do_score(target, "H", Some(true)), // direct case prefix
        do_score(target, "h", Some(true)), // direct mix-case prefix
        do_score(target, "W", Some(true)), // direct case word prefix
        do_score(target, "Ld", Some(true)), // in-string case match (multiple)
        do_score(target, "ld", Some(true)), // in-string mix-case match (consecutive, avoids scattered hit)
        do_score(target, "w", Some(true)), // direct mix-case word prefix
        do_score(target, "L", Some(true)), // in-string case match
        do_score(target, "l", Some(true)), // in-string mix-case match
        do_score(target, "4", Some(true)), // no match
    ];

    // Assert scoring order
    let mut sorted_scores = scores.clone();
    sorted_scores.sort_by_key(|score| std::cmp::Reverse(score.0));
    assert_eq!(scores, sorted_scores);

    // Assert scoring positions
    // let positions = scores[0][1];
    // assert.strictEqual(positions.length, 'HelLo-World'.length);

    // positions = scores[2][1];
    // assert.strictEqual(positions.length, 'HW'.length);
    // assert.strictEqual(positions[0], 0);
    // assert.strictEqual(positions[1], 6);
}

#[test]
fn score_non_fuzzy() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let target = "HelLo-World";

    assert!(do_score(target, "HelLo-World", Some(false)).0 > 0);
    assert_eq!(do_score(target, "HelLo-World", Some(false)).1.len(), u("HelLo-World").len());

    assert!(do_score(target, "hello-world", Some(false)).0 > 0);
    assert_eq!(do_score(target, "HW", Some(false)).0, 0);
    assert!(do_score(target, "h", Some(false)).0 > 0);
    assert!(do_score(target, "ello", Some(false)).0 > 0);
    assert!(do_score(target, "ld", Some(false)).0 > 0);
    assert_eq!(do_score(target, "eo", Some(false)).0, 0);
}

#[test]
fn score_item_matches_are_proper() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut res = score_item_fresh(None, "something", true, &ResourceAccessor);
    assert!(res.score == 0);

    let resource = file("/xyz/some/path/someFile123.txt");

    res = score_item_fresh(Some(&resource), "something", true, &NullAccessor);
    assert!(res.score == 0);

    // Path Identity
    let identity_res = score_item_fresh(Some(&resource), &ResourceAccessor.get_item_path(&resource).unwrap().to_string_lossy(), true, &ResourceAccessor);
    assert!(identity_res.score != 0);
    assert_eq!(identity_res.description_match.as_ref().unwrap().len(), 1);
    assert_eq!(identity_res.label_match.as_ref().unwrap().len(), 1);
    assert_eq!(identity_res.description_match.as_ref().unwrap()[0].start, 0);
    assert_eq!(identity_res.description_match.as_ref().unwrap()[0].end, ResourceAccessor.get_item_description(&resource).unwrap().len());
    assert_eq!(identity_res.label_match.as_ref().unwrap()[0].start, 0);
    assert_eq!(identity_res.label_match.as_ref().unwrap()[0].end, ResourceAccessor.get_item_label(&resource).unwrap().len());

    // Basename Prefix
    let basename_prefix_res = score_item_fresh(Some(&resource), "som", true, &ResourceAccessor);
    assert!(basename_prefix_res.score != 0);
    assert!(basename_prefix_res.description_match.is_none());
    assert_eq!(basename_prefix_res.label_match.as_ref().unwrap().len(), 1);
    assert_eq!(basename_prefix_res.label_match.as_ref().unwrap()[0].start, 0);
    assert_eq!(basename_prefix_res.label_match.as_ref().unwrap()[0].end, u("som").len());

    // Basename Camelcase
    let basename_camelcase_res = score_item_fresh(Some(&resource), "sF", true, &ResourceAccessor);
    assert!(basename_camelcase_res.score != 0);
    assert!(basename_camelcase_res.description_match.is_none());
    assert_eq!(basename_camelcase_res.label_match.as_ref().unwrap().len(), 2);
    assert_eq!(basename_camelcase_res.label_match.as_ref().unwrap()[0].start, 0);
    assert_eq!(basename_camelcase_res.label_match.as_ref().unwrap()[0].end, 1);
    assert_eq!(basename_camelcase_res.label_match.as_ref().unwrap()[1].start, 4);
    assert_eq!(basename_camelcase_res.label_match.as_ref().unwrap()[1].end, 5);

    // Basename Match
    let basename_res = score_item_fresh(Some(&resource), "of", true, &ResourceAccessor);
    assert!(basename_res.score != 0);
    assert!(basename_res.description_match.is_none());
    assert_eq!(basename_res.label_match.as_ref().unwrap().len(), 2);
    assert_eq!(basename_res.label_match.as_ref().unwrap()[0].start, 1);
    assert_eq!(basename_res.label_match.as_ref().unwrap()[0].end, 2);
    assert_eq!(basename_res.label_match.as_ref().unwrap()[1].start, 4);
    assert_eq!(basename_res.label_match.as_ref().unwrap()[1].end, 5);

    // Path Match
    let path_res = score_item_fresh(Some(&resource), "xyz123", true, &ResourceAccessor);
    assert!(path_res.score != 0);
    assert!(path_res.description_match.is_some());
    assert!(path_res.label_match.is_some());
    assert_eq!(path_res.label_match.as_ref().unwrap().len(), 1);
    assert_eq!(path_res.label_match.as_ref().unwrap()[0].start, 8);
    assert_eq!(path_res.label_match.as_ref().unwrap()[0].end, 11);
    assert_eq!(path_res.description_match.as_ref().unwrap().len(), 1);
    assert_eq!(path_res.description_match.as_ref().unwrap()[0].start, 1);
    assert_eq!(path_res.description_match.as_ref().unwrap()[0].end, 4);

    // Ellipsis Match
    let ellipsis_res = score_item_fresh(Some(&resource), "…me/path/someFile123.txt", true, &ResourceAccessor);
    assert!(ellipsis_res.score != 0);
    assert!(path_res.description_match.is_some());
    assert!(path_res.label_match.is_some());
    assert_eq!(path_res.label_match.as_ref().unwrap().len(), 1);
    assert_eq!(path_res.label_match.as_ref().unwrap()[0].start, 8);
    assert_eq!(path_res.label_match.as_ref().unwrap()[0].end, 11);
    assert_eq!(path_res.description_match.as_ref().unwrap().len(), 1);
    assert_eq!(path_res.description_match.as_ref().unwrap()[0].start, 1);
    assert_eq!(path_res.description_match.as_ref().unwrap()[0].end, 4);

    // No Match
    let no_res = score_item_fresh(Some(&resource), "987", true, &ResourceAccessor);
    assert!(no_res.score == 0);
    assert!(no_res.label_match.is_none());
    assert!(no_res.description_match.is_none());

    // No Exact Match
    let no_exact_res = score_item_fresh(Some(&resource), "\"sF\"", true, &ResourceAccessor);
    assert!(no_exact_res.score == 0);
    assert!(no_exact_res.label_match.is_none());
    assert!(no_exact_res.description_match.is_none());
    assert_eq!(no_res.score, no_exact_res.score);

    // Verify Scores
    assert!(identity_res.score > basename_prefix_res.score);
    assert!(basename_prefix_res.score > basename_res.score);
    assert!(basename_res.score > path_res.score);
    assert!(path_res.score > no_res.score);
}

#[test]
fn score_item_multiple() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("/xyz/some/path/someFile123.txt");

    let res1 = score_item_fresh(Some(&resource), "xyz some", true, &ResourceAccessor);
    assert!(res1.score != 0);
    assert_eq!(res1.label_match.as_ref().map(Vec::len), Some(1));
    assert_eq!(res1.label_match.as_ref().unwrap()[0].start, 0);
    assert_eq!(res1.label_match.as_ref().unwrap()[0].end, 4);
    assert_eq!(res1.description_match.as_ref().map(Vec::len), Some(1));
    assert_eq!(res1.description_match.as_ref().unwrap()[0].start, 1);
    assert_eq!(res1.description_match.as_ref().unwrap()[0].end, 4);

    let res2 = score_item_fresh(Some(&resource), "some xyz", true, &ResourceAccessor);
    assert!(res2.score != 0);
    assert_eq!(res1.score, res2.score);
    assert_eq!(res2.label_match.as_ref().map(Vec::len), Some(1));
    assert_eq!(res2.label_match.as_ref().unwrap()[0].start, 0);
    assert_eq!(res2.label_match.as_ref().unwrap()[0].end, 4);
    assert_eq!(res2.description_match.as_ref().map(Vec::len), Some(1));
    assert_eq!(res2.description_match.as_ref().unwrap()[0].start, 1);
    assert_eq!(res2.description_match.as_ref().unwrap()[0].end, 4);

    let res3 = score_item_fresh(Some(&resource), "some xyz file file123", true, &ResourceAccessor);
    assert!(res3.score != 0);
    assert!(res3.score > res2.score);
    assert_eq!(res3.label_match.as_ref().map(Vec::len), Some(1));
    assert_eq!(res3.label_match.as_ref().unwrap()[0].start, 0);
    assert_eq!(res3.label_match.as_ref().unwrap()[0].end, 11);
    assert_eq!(res3.description_match.as_ref().map(Vec::len), Some(1));
    assert_eq!(res3.description_match.as_ref().unwrap()[0].start, 1);
    assert_eq!(res3.description_match.as_ref().unwrap()[0].end, 4);

    let res4 = score_item_fresh(Some(&resource), "path z y", true, &ResourceAccessor);
    assert!(res4.score != 0);
    assert!(res4.score < res2.score);
    assert_eq!(res4.label_match.as_ref().map(Vec::len), Some(0));
    assert_eq!(res4.description_match.as_ref().map(Vec::len), Some(2));
    assert_eq!(res4.description_match.as_ref().unwrap()[0].start, 2);
    assert_eq!(res4.description_match.as_ref().unwrap()[0].end, 4);
    assert_eq!(res4.description_match.as_ref().unwrap()[1].start, 10);
    assert_eq!(res4.description_match.as_ref().unwrap()[1].end, 14);
}

#[test]
fn score_item_multiple_with_cache_yields_different_results() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("/xyz/some/path/someFile123.txt");
    let mut cache = FuzzyScorerCache::new();
    let res1 = score_item(Some(&resource), "xyz sm", true, &ResourceAccessor, &mut cache);
    assert!(res1.score != 0);

    // from the cache's perspective this should be a totally different query
    let res2 = score_item(Some(&resource), "xyz \"sm\"", true, &ResourceAccessor, &mut cache);
    assert!(res2.score == 0);
}

#[test]
fn score_item_cache_key_matches_the_hash_of_the_scoring_inputs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("/xyz/some/path/someFile123.txt");

    let expected_cache_key = |query_value: &[u16], allow_non_contiguous_matches: bool, description: Option<&JsString>| -> i32 {
        let query = fuzzy_scorer::prepare_query(query_value, &platform());
        let values: Vec<&PreparedQueryPiece> = match &query.values {
            Some(values) => values.iter().collect(),
            None => vec![&query.piece],
        };

        let values = Dynamic::array(values.iter().map(|value| {
            Dynamic::object([
                ("value", Dynamic::String(value.normalized.clone())),
                ("expectContiguousMatch", Dynamic::Bool(value.expect_contiguous_match)),
            ])
        }));
        hash(&Dynamic::object([(
            query.normalized.clone(),
            Dynamic::object([
                ("values", values),
                ("label", Dynamic::String(ResourceAccessor.get_item_label(&resource).unwrap())),
                ("description", description.map_or(Dynamic::Undefined, |d| Dynamic::String(d.clone()))),
                ("allowNonContiguousMatches", Dynamic::Bool(allow_non_contiguous_matches)),
            ]),
        )]))
    };

    let query_values: [Vec<u16>; 15] = [
        u("some"),
        u("xyz sm"),
        u("xyz \"sm\""),
        u(""),
        u("ünïcödé"),
        u("null\u{0}char"),
        u("tab\there"),
        u("0"),
        u("01"),
        u("4294967295"),
        u("__proto__"),
        u("constructor"),
        vec![0xD83D, 0xDE80],
        vec![0xD800],
        vec![0xDFFF],
    ];
    let descriptions: [Option<JsString>; 5] = [
        None,
        Some(JsString::from("")),
        ResourceAccessor.get_item_description(&resource),
        Some(JsString::from("\u{0}:[]")),
        Some(JsString::from_units(vec![0xD83D, 0xDE80, 0xD800])),
    ];
    for query_value in &query_values {
        for description in &descriptions {
            for allow_non_contiguous_matches in [true, false] {
                let mut cache = FuzzyScorerCache::new();
                score_item_units(Some(&resource), query_value, allow_non_contiguous_matches, &ResourceAccessorWithDescription(description.clone()), &mut cache);

                let expected: Vec<i32> = if query_value.is_empty() {
                    Vec::new()
                } else {
                    vec![expected_cache_key(query_value, allow_non_contiguous_matches, description.as_ref())]
                };
                assert_eq!(
                    cache.keys().copied().collect::<Vec<_>>(),
                    expected,
                    "unexpected cache key for {:?}",
                    (query_value, description, allow_non_contiguous_matches)
                );
            }
        }
    }
}

#[test]
fn score_item_cache_keys_separate_items_queries_and_match_modes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mut cache = FuzzyScorerCache::new();

    // same label, different description
    score_item(Some(&file("/xyz/some/path/someFile123.txt")), "some", true, &ResourceAccessor, &mut cache);
    score_item(Some(&file("/xyz/other/path/someFile123.txt")), "some", true, &ResourceAccessor, &mut cache);

    // same item, different queries (incl. multi piece and quoted/contiguous)
    let resource = file("/xyz/some/path/someFile123.txt");
    score_item(Some(&resource), "somefile", true, &ResourceAccessor, &mut cache);
    score_item(Some(&resource), "xyz sm", true, &ResourceAccessor, &mut cache);
    score_item(Some(&resource), "xyz \"sm\"", true, &ResourceAccessor, &mut cache);
    score_item(Some(&resource), "sm xyz", true, &ResourceAccessor, &mut cache);

    // same item and query, different match mode
    score_item(Some(&resource), "somefile", false, &ResourceAccessor, &mut cache);

    // an item without description
    score_item(Some(&file("/someFile123.txt")), "some", true, &ResourceAccessorWithDescription(None), &mut cache);
    score_item(Some(&file("/someFile123.txt")), "some", true, &ResourceAccessorWithDescription(Some(JsString::from(""))), &mut cache);

    assert_eq!(cache.len(), 9, "each distinct scoring input needs its own cache entry");
}

#[test]
fn score_item_repeated_scoring_hits_the_cache_and_preserves_the_result() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("/xyz/some/path/someFile123.txt");

    for query_value in ["somefile", "xyz sm", "xyz \"sm\"", "ünïcödé"] {
        let mut cache = FuzzyScorerCache::new();
        let cold = score_item(Some(&resource), query_value, true, &ResourceAccessor, &mut cache);
        let warm = score_item(Some(&resource), query_value, true, &ResourceAccessor, &mut cache);

        assert_eq!(cache.len(), 1);
        assert!(Rc::ptr_eq(&warm, &cold), "warm scoring should return the cached result");
    }
}

#[test]
fn score_item_mutating_a_prepared_query_is_reflected_in_the_cache_key() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("/xyz/some/path/someFile123.txt");
    let mut cache = FuzzyScorerCache::new();

    let mut query = pq("xyz sm");
    let before = fuzzy_scorer::score_item_fuzzy(Some(&resource), &query, true, &ResourceAccessor, &mut cache, &platform());
    assert!(before.score != 0);

    // callers own the prepared query object, so a mutated query must not reuse the previous entry
    let values = query.values.as_mut().unwrap();
    values[1] = PreparedQueryPiece { expect_contiguous_match: true, ..values[1].clone() };
    let after = fuzzy_scorer::score_item_fuzzy(Some(&resource), &query, true, &ResourceAccessor, &mut cache, &platform());

    assert_eq!(cache.len(), 2);
    assert_eq!(*after, *score_item_fresh(Some(&resource), "xyz \"sm\"", true, &ResourceAccessor));
}

#[test]
fn score_item_invalid_input() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    // `scoreItem(null, null!, ...)`: a consulta nula do original (`typeof original !== 'string'` vira '')
    // é o texto vazio, que o tipo `&str` representa.
    let mut res = score_item_fresh(None, "", true, &ResourceAccessor);
    assert_eq!(res.score, 0);

    res = score_item_fresh(None, "null", true, &ResourceAccessor);
    assert_eq!(res.score, 0);
}

#[test]
fn score_item_optimize_for_file_paths() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("/xyz/others/spath/some/xsp/file123.txt");

    // xsp is more relevant to the end of the file path even though it matches
    // fuzzy also in the beginning. we verify the more relevant match at the
    // end gets returned.
    let path_res = score_item_fresh(Some(&resource), "xspfile123", true, &ResourceAccessor);
    assert!(path_res.score != 0);
    assert!(path_res.description_match.is_some());
    assert!(path_res.label_match.is_some());
    assert_eq!(path_res.label_match.as_ref().unwrap().len(), 1);
    assert_eq!(path_res.label_match.as_ref().unwrap()[0].start, 0);
    assert_eq!(path_res.label_match.as_ref().unwrap()[0].end, 7);
    assert_eq!(path_res.description_match.as_ref().unwrap().len(), 1);
    assert_eq!(path_res.description_match.as_ref().unwrap()[0].start, 23);
    assert_eq!(path_res.description_match.as_ref().unwrap()[0].end, 26);
}

#[test]
fn score_item_avoid_match_scattering_bug_36119() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("projects/ui/cula/ats/target.mk");

    let path_res = score_item_fresh(Some(&resource), "tcltarget.mk", true, &ResourceAccessor);
    assert!(path_res.score != 0);
    assert!(path_res.description_match.is_some());
    assert!(path_res.label_match.is_some());
    assert_eq!(path_res.label_match.as_ref().unwrap().len(), 1);
    assert_eq!(path_res.label_match.as_ref().unwrap()[0].start, 0);
    assert_eq!(path_res.label_match.as_ref().unwrap()[0].end, 9);
}

#[test]
fn score_item_prefers_more_compact_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("/1a111d1/11a1d1/something.txt");

    // expect "ad" to be matched towards the end of the file because the
    // match is more compact
    let res = score_item_fresh(Some(&resource), "ad", true, &ResourceAccessor);
    assert!(res.score != 0);
    assert!(res.description_match.is_some());
    assert!(res.label_match.as_ref().unwrap().is_empty());
    assert_eq!(res.description_match.as_ref().unwrap().len(), 2);
    assert_eq!(res.description_match.as_ref().unwrap()[0].start, 11);
    assert_eq!(res.description_match.as_ref().unwrap()[0].end, 12);
    assert_eq!(res.description_match.as_ref().unwrap()[1].start, 13);
    assert_eq!(res.description_match.as_ref().unwrap()[1].end, 14);
}

#[test]
fn score_item_proper_target_offset() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("etem");

    let res = score_item_fresh(Some(&resource), "teem", true, &ResourceAccessor);
    assert!(res.score == 0);
}

#[test]
fn score_item_proper_target_offset_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("ede");

    let res = score_item_fresh(Some(&resource), "de", true, &ResourceAccessor);

    assert_eq!(res.label_match.as_ref().unwrap().len(), 1);
    assert_eq!(res.label_match.as_ref().unwrap()[0].start, 1);
    assert_eq!(res.label_match.as_ref().unwrap()[0].end, 3);
}

#[test]
fn score_item_proper_target_offset_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("/src/vs/editor/browser/viewParts/lineNumbers/flipped-cursor-2x.svg");

    let res = score_item_fresh(Some(&resource), "debug", true, &ResourceAccessor);

    assert_eq!(res.description_match.as_ref().unwrap().len(), 3);
    assert_eq!(res.description_match.as_ref().unwrap()[0].start, 9);
    assert_eq!(res.description_match.as_ref().unwrap()[0].end, 10);
    assert_eq!(res.description_match.as_ref().unwrap()[1].start, 36);
    assert_eq!(res.description_match.as_ref().unwrap()[1].end, 37);
    assert_eq!(res.description_match.as_ref().unwrap()[2].start, 40);
    assert_eq!(res.description_match.as_ref().unwrap()[2].end, 41);

    assert_eq!(res.label_match.as_ref().unwrap().len(), 2);
    assert_eq!(res.label_match.as_ref().unwrap()[0].start, 9);
    assert_eq!(res.label_match.as_ref().unwrap()[0].end, 10);
    assert_eq!(res.label_match.as_ref().unwrap()[1].start, 20);
    assert_eq!(res.label_match.as_ref().unwrap()[1].end, 21);
}

#[test]
fn score_item_no_match_unless_query_contained_in_sequence() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource = file("abcde");

    let res = score_item_fresh(Some(&resource), "edcda", true, &ResourceAccessor);
    assert!(res.score == 0);
}

#[test]
fn score_item_match_if_using_slash_or_backslash_local_remote_resource() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let local_resource = file("abcde/super/duper");
    let remote_resource = URI::from(
        &UriComponents { scheme: Some(JsString::from(schemas::VSCODE_REMOTE)), path: Some(JsString::from("abcde/super/duper")), ..UriComponents::default() },
        false,
    );

    for resource in [local_resource, remote_resource] {
        let mut res = score_item_fresh(Some(&resource), "abcde\\super\\duper", true, &ResourceAccessor);
        assert!(res.score != 0);

        res = score_item_fresh(Some(&resource), "abcde\\super\\duper", true, &ResourceWithSlashAccessor);
        assert!(res.score != 0);

        res = score_item_fresh(Some(&resource), "abcde\\super\\duper", true, &ResourceWithBackslashAccessor);
        assert!(res.score != 0);

        res = score_item_fresh(Some(&resource), "abcde/super/duper", true, &ResourceAccessor);
        assert!(res.score != 0);

        res = score_item_fresh(Some(&resource), "abcde/super/duper", true, &ResourceWithSlashAccessor);
        assert!(res.score != 0);

        res = score_item_fresh(Some(&resource), "abcde/super/duper", true, &ResourceWithBackslashAccessor);
        assert!(res.score != 0);
    }
}

#[test]
fn score_item_ensure_upper_case_bonus_only_applies_on_non_consecutive_matches_bug_134723() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_with_upper = file("ASDFasdfasdf");
    let resource_all_lower = file("asdfasdfasdf");

    assert!(score_item_fresh(Some(&resource_all_lower), "asdf", true, &ResourceAccessor).score > score_item_fresh(Some(&resource_with_upper), "asdf", true, &ResourceAccessor).score);
}

#[test]
fn compare_items_by_score_identity() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("/some/path/fileA.txt");
    let resource_b = file("/some/path/other/fileB.txt");
    let resource_c = file("/unrelated/some/path/other/fileC.txt");

    // Full resource A path
    let mut query = ResourceAccessor.get_item_path(&resource_a).unwrap().to_string_lossy();

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    // Full resource B path
    query = ResourceAccessor.get_item_path(&resource_b).unwrap().to_string_lossy();

    res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));
}

#[test]
fn compare_files_by_score_basename_prefix() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("/some/path/fileA.txt");
    let resource_b = file("/some/path/other/fileB.txt");
    let resource_c = file("/unrelated/some/path/other/fileC.txt");

    // Full resource A basename
    let mut query = ResourceAccessor.get_item_label(&resource_a).unwrap().to_string_lossy();

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    // Full resource B basename
    query = ResourceAccessor.get_item_label(&resource_b).unwrap().to_string_lossy();

    res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));
}

#[test]
fn compare_files_by_score_basename_camelcase() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("/some/path/fileA.txt");
    let resource_b = file("/some/path/other/fileB.txt");
    let resource_c = file("/unrelated/some/path/other/fileC.txt");

    // resource A camelcase
    let mut query = "fA";

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    // resource B camelcase
    query = "fB";

    res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));
}

#[test]
fn compare_files_by_score_basename_scores() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("/some/path/fileA.txt");
    let resource_b = file("/some/path/other/fileB.txt");
    let resource_c = file("/unrelated/some/path/other/fileC.txt");

    // Resource A part of basename
    let mut query = "fileA";

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    // Resource B part of basename
    query = "fileB";

    res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));
}

#[test]
fn compare_files_by_score_path_scores() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("/some/path/fileA.txt");
    let resource_b = file("/some/path/other/fileB.txt");
    let resource_c = file("/unrelated/some/path/other/fileC.txt");

    // Resource A part of path
    let mut query = "pathfileA";

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    // Resource B part of path
    query = "pathfileB";

    res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));
}

#[test]
fn compare_files_by_score_prefer_shorter_basenames() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("/some/path/fileA.txt");
    let resource_b = file("/some/path/other/fileBLonger.txt");
    let resource_c = file("/unrelated/the/path/other/fileC.txt");

    // Resource A part of path
    let query = "somepath";

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));
}

#[test]
fn compare_files_by_score_prefer_shorter_basenames_match_on_basename() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("/some/path/fileA.txt");
    let resource_b = file("/some/path/other/fileBLonger.txt");
    let resource_c = file("/unrelated/the/path/other/fileC.txt");

    // Resource A part of path
    let query = "file";

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_c));
    assert!(std::ptr::eq(res[2], &resource_b));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_c));
    assert!(std::ptr::eq(res[2], &resource_b));
}

#[test]
fn compare_files_by_score_prefer_shorter_paths() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("/some/path/fileA.txt");
    let resource_b = file("/some/path/other/fileB.txt");
    let resource_c = file("/unrelated/some/path/other/fileC.txt");

    // Resource A part of path
    let query = "somepath";

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
    assert!(std::ptr::eq(res[2], &resource_c));
}

#[test]
fn compare_files_by_score_prefer_shorter_paths_bug_17443() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("config/test/t1.js");
    let resource_b = file("config/test.js");
    let resource_c = file("config/test/t2.js");

    let query = "co/te";

    let res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
    assert!(std::ptr::eq(res[2], &resource_c));
}

#[test]
fn compare_files_by_score_prefer_matches_in_label_over_description_if_scores_are_otherwise_equal() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("parts/quick/arrow-left-dark.svg");
    let resource_b = file("parts/quickopen/quickopen.ts");

    let query = "partsquick";

    let res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
}

#[test]
fn compare_files_by_score_prefer_camel_case_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("config/test/NullPointerException.java");
    let resource_b = file("config/test/nopointerexception.java");

    for query in ["npe", "NPE"] {
        let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_a));
        assert!(std::ptr::eq(res[1], &resource_b));

        res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_a));
        assert!(std::ptr::eq(res[1], &resource_b));
    }
}

#[test]
fn compare_files_by_score_prefer_more_compact_camel_case_matches() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("config/test/openthisAnythingHandler.js");
    let resource_b = file("config/test/openthisisnotsorelevantforthequeryAnyHand.js");

    let query = "AH";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
}

#[test]
fn compare_files_by_score_prefer_more_compact_matches_label() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("config/test/examasdaple.js");
    let resource_b = file("config/test/exampleasdaasd.ts");

    let query = "xp";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
}

#[test]
fn compare_files_by_score_prefer_more_compact_matches_path() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("config/test/examasdaple/file.js");
    let resource_b = file("config/test/exampleasdaasd/file.ts");

    let query = "xp";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
}

#[test]
fn compare_files_by_score_prefer_more_compact_matches_label_and_path() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("config/example/thisfile.ts");
    let resource_b = file("config/24234243244/example/file.js");

    let query = "exfile";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_34210() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("node_modules1/bundle/lib/model/modules/ot1/index.js");
    let resource_b = file("node_modules1/bundle/lib/model/modules/un1/index.js");
    let resource_c = file("node_modules1/bundle/lib/model/modules/modu1/index.js");
    let resource_d = file("node_modules1/bundle/lib/model/modules/oddl1/index.js");

    let mut query = if is_windows() { "modu1\\index.js" } else { "modu1/index.js" };

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c, &resource_d], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a, &resource_d], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_c));

    query = if is_windows() { "un1\\index.js" } else { "un1/index.js" };

    res = sort_by_score(&[&resource_a, &resource_b, &resource_c, &resource_d], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a, &resource_d], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_21019_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("app/containers/Services/NetworkData/ServiceDetails/ServiceLoad/index.js");
    let resource_b = file("app/containers/Services/NetworkData/ServiceDetails/ServiceDistribution/index.js");
    let resource_c = file("app/containers/Services/NetworkData/ServiceDetailTabs/ServiceTabs/StatVideo/index.js");

    let query = "StatVideoindex";

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_c));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_21019_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("src/build-helper/store/redux.ts");
    let resource_b = file("src/repository/store/redux.ts");

    let query = "reproreduxts";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_26649() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("photobook/src/components/AddPagesButton/index.js");
    let resource_b = file("photobook/src/components/ApprovalPageHeader/index.js");
    let resource_c = file("photobook/src/canvasComponents/BookPage/index.js");

    let query = "bookpageIndex";

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_c));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_c));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_33247() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("ui/src/utils/constants.js");
    let resource_b = file("ui/src/ui/Icons/index.js");

    let query = if is_windows() { "ui\\icons" } else { "ui/icons" };

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_33247_comment() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("ui/src/components/IDInput/index.js");
    let resource_b = file("ui/src/ui/Input/index.js");

    let query = if is_windows() { "ui\\input\\index" } else { "ui/input/index" };

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_36166() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("django/contrib/sites/locale/ga/LC_MESSAGES/django.mo");
    let resource_b = file("django/core/signals.py");

    let query = "djancosig";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_32918() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("adsys/protected/config.php");
    let resource_b = file("adsys/protected/framework/smarty/sysplugins/smarty_internal_config.php");
    let resource_c = file("duowanVideo/wap/protected/config.php");

    let query = "protectedconfig.php";

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_c));
    assert!(std::ptr::eq(res[2], &resource_b));

    res = sort_by_score(&[&resource_c, &resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_c));
    assert!(std::ptr::eq(res[2], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_14879() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("pkg/search/gradient/testdata/constraint_attrMatchString.yml");
    let resource_b = file("cmd/gradient/main.go");

    let query = "gradientmain";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_14727_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("alpha-beta-cappa.txt");
    let resource_b = file("abc.txt");

    let query = "abc";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_14727_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("xerxes-yak-zubba/index.js");
    let resource_b = file("xyz/index.js");

    let query = "xyz";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_18381() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("AssymblyInfo.cs");
    let resource_b = file("IAsynchronousTask.java");

    let query = "async";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_35572() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("static/app/source/angluar/-admin/-organization/-settings/layout/layout.js");
    let resource_b = file("static/app/source/angular/-admin/-project/-settings/_settings/settings.js");

    let query = "partisettings";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_36810() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("Trilby.TrilbyTV.Web.Portal/Views/Systems/Index.cshtml");
    let resource_b = file("Trilby.TrilbyTV.Web.Portal/Areas/Admins/Views/Tips/Index.cshtml");

    let query = "tipsindex.cshtml";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_prefer_shorter_hit_bug_20546() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("editor/core/components/tests/list-view-spec.js");
    let resource_b = file("editor/core/components/list-view.js");

    let query = "listview";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_avoid_match_scattering_bug_12095() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("src/vs/workbench/contrib/files/common/explorerViewModel.ts");
    let resource_b = file("src/vs/workbench/contrib/files/browser/views/explorerView.ts");
    let resource_c = file("src/vs/workbench/contrib/files/browser/views/explorerViewer.ts");

    let query = "filesexplorerview.ts";

    let mut res = sort_by_score(&[&resource_a, &resource_b, &resource_c], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_a, &resource_c, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_prefer_case_match_bug_96122() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("lists.php");
    let resource_b = file("lib/Lists.php");

    let query = "Lists.php";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
}

#[test]
fn compare_files_by_score_prefer_shorter_match_bug_103052_foo_bar() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("app/emails/foo.bar.js");
    let resource_b = file("app/emails/other-footer.other-bar.js");

    for query in ["foo bar", "foobar"] {
        let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_a));
        assert!(std::ptr::eq(res[1], &resource_b));

        res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_a));
        assert!(std::ptr::eq(res[1], &resource_b));
    }
}

#[test]
fn compare_files_by_score_prefer_shorter_match_bug_103052_payment_model() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("app/components/payment/payment.model.js");
    let resource_b = file("app/components/online-payments-history/online-payments-history.model.js");

    for query in ["payment model", "paymentmodel"] {
        let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_a));
        assert!(std::ptr::eq(res[1], &resource_b));

        res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_a));
        assert!(std::ptr::eq(res[1], &resource_b));
    }
}

#[test]
fn compare_files_by_score_prefer_shorter_match_bug_103052_color() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("app/constants/color.js");
    let resource_b = file("app/components/model/input/pick-avatar-color.js");

    for query in ["color js", "colorjs"] {
        let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_a));
        assert!(std::ptr::eq(res[1], &resource_b));

        res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_a));
        assert!(std::ptr::eq(res[1], &resource_b));
    }
}

#[test]
fn compare_files_by_score_prefer_strict_case_prefix() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("app/constants/color.js");
    let resource_b = file("app/components/model/input/Color.js");

    let mut query = "Color";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));

    query = "color";

    res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
}

#[test]
fn compare_files_by_score_prefer_prefix_bug_103052() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("test/smoke/src/main.ts");
    let resource_b = file("src/vs/editor/common/services/semantikTokensProviderStyling.ts");

    let query = "smoke main.ts";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_a));
    assert!(std::ptr::eq(res[1], &resource_b));
}

#[test]
fn compare_files_by_score_boost_better_prefix_match_if_multiple_queries_are_used() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("src/vs/workbench/services/host/browser/browserHostService.ts");
    let resource_b = file("src/vs/workbench/browser/workbench.ts");

    for query in ["workbench.ts browser", "browser workbench.ts", "browser workbench", "workbench browser"] {
        let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_b));
        assert!(std::ptr::eq(res[1], &resource_a));

        res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_b));
        assert!(std::ptr::eq(res[1], &resource_a));
    }
}

#[test]
fn compare_files_by_score_boost_shorter_prefix_match_if_multiple_queries_are_used() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("src/vs/workbench/node/actions/windowActions.ts");
    let resource_b = file("src/vs/workbench/electron-node/window.ts");

    for query in ["window node", "window.ts node"] {
        let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_b));
        assert!(std::ptr::eq(res[1], &resource_a));

        res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
        assert!(std::ptr::eq(res[0], &resource_b));
        assert!(std::ptr::eq(res[1], &resource_a));
    }
}

#[test]
fn compare_files_by_score_skip_preference_on_label_match_when_using_path_sep() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("djangosite/ufrela/def.py");
    let resource_b = file("djangosite/urls/default.py");

    let query = "url/def";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
}

#[test]
fn compare_files_by_score_boost_shorter_prefix_match_if_multiple_queries_are_used_99171() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("mesh_editor_lifetime_job.h");
    let resource_b = file("lifetime_job.h");

    let query = "m life, life m";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
}

#[test]
fn compare_files_by_score_boost_consecutive_matches_in_the_beginning_over_end() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let resource_a = file("src/vs/server/node/extensionHostStatusService.ts");
    let resource_b = file("src/vs/workbench/browser/parts/notifications/notificationsStatus.ts");

    let query = "notStatus";

    let mut res = sort_by_score(&[&resource_a, &resource_b], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));

    res = sort_by_score(&[&resource_b, &resource_a], &query, true, &ResourceAccessor);
    assert!(std::ptr::eq(res[0], &resource_b));
    assert!(std::ptr::eq(res[1], &resource_a));
}

#[test]
fn prepare_query() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(pq(" f*a ").normalized, "fa");
    assert_eq!(pq(" f…a ").normalized, "fa");
    assert_eq!(pq("main#").normalized, "main");
    assert_eq!(pq("main#").original, "main#");
    assert_eq!(pq("foo*").normalized, "foo");
    assert_eq!(pq("foo*").original, "foo*");
    assert_eq!(pq("model Tester.ts").original, "model Tester.ts");
    assert_eq!(pq("model Tester.ts").original_lowercase, "model Tester.ts".to_lowercase().as_str());
    assert_eq!(pq("model Tester.ts").normalized, "modelTester.ts");
    assert!(!pq("model Tester.ts").expect_contiguous_match); // doesn't have quotes in it
    assert_eq!(pq("Model Tester.ts").normalized_lowercase, "modeltester.ts");
    assert!(!pq("ModelTester.ts").contains_path_separator);
    assert!(pq(&format!("Model{}Tester.ts", sep())).contains_path_separator);
    assert!(pq("\"hello\"").expect_contiguous_match);
    assert_eq!(pq("\"hello\"").normalized, "hello");

    // with spaces
    let mut query = pq("He*llo World");
    assert_eq!(query.original, "He*llo World");
    assert_eq!(query.normalized, "HelloWorld");
    assert_eq!(query.normalized_lowercase, "HelloWorld".to_lowercase().as_str());
    assert_eq!(query.values.as_ref().map(Vec::len), Some(2));
    assert_eq!(query.values.as_ref().unwrap()[0].original, "He*llo");
    assert_eq!(query.values.as_ref().unwrap()[0].normalized, "Hello");
    assert_eq!(query.values.as_ref().unwrap()[0].normalized_lowercase, "Hello".to_lowercase().as_str());
    assert_eq!(query.values.as_ref().unwrap()[1].original, "World");
    assert_eq!(query.values.as_ref().unwrap()[1].normalized, "World");
    assert_eq!(query.values.as_ref().unwrap()[1].normalized_lowercase, "World".to_lowercase().as_str());

    let restored_query = fuzzy_scorer::pieces_to_query(query.values.as_ref().unwrap(), &platform());
    assert_eq!(restored_query.original, query.original);
    assert_eq!(restored_query.values.as_ref().map(Vec::len), query.values.as_ref().map(Vec::len));
    assert_eq!(restored_query.contains_path_separator, query.contains_path_separator);

    // with spaces that are empty
    query = pq(" Hello   World  \t");
    assert_eq!(query.original, " Hello   World  \t");
    assert_eq!(query.original_lowercase, " Hello   World  \t".to_lowercase().as_str());
    assert_eq!(query.normalized, "HelloWorld");
    assert_eq!(query.normalized_lowercase, "HelloWorld".to_lowercase().as_str());
    assert_eq!(query.values.as_ref().map(Vec::len), Some(2));
    assert_eq!(query.values.as_ref().unwrap()[0].original, "Hello");
    assert_eq!(query.values.as_ref().unwrap()[0].original_lowercase, "Hello".to_lowercase().as_str());
    assert_eq!(query.values.as_ref().unwrap()[0].normalized, "Hello");
    assert_eq!(query.values.as_ref().unwrap()[0].normalized_lowercase, "Hello".to_lowercase().as_str());
    assert_eq!(query.values.as_ref().unwrap()[1].original, "World");
    assert_eq!(query.values.as_ref().unwrap()[1].original_lowercase, "World".to_lowercase().as_str());
    assert_eq!(query.values.as_ref().unwrap()[1].normalized, "World");
    assert_eq!(query.values.as_ref().unwrap()[1].normalized_lowercase, "World".to_lowercase().as_str());

    // Path related
    if is_windows() {
        assert_eq!(pq("C:\\some\\path").path_normalized, "C:\\some\\path");
        assert_eq!(pq("C:\\some\\path").normalized, "C:\\some\\path");
        assert!(pq("C:\\some\\path").contains_path_separator);
        assert_eq!(pq("C:/some/path").path_normalized, "C:\\some\\path");
        assert_eq!(pq("C:/some/path").normalized, "C:\\some\\path");
        assert!(pq("C:/some/path").contains_path_separator);
    } else {
        assert_eq!(pq("/some/path").path_normalized, "/some/path");
        assert_eq!(pq("/some/path").normalized, "/some/path");
        assert!(pq("/some/path").contains_path_separator);
        assert_eq!(pq("\\some\\path").path_normalized, "/some/path");
        assert_eq!(pq("\\some\\path").normalized, "/some/path");
        assert!(pq("\\some\\path").contains_path_separator);
    }
}

#[test]
fn fuzzy_score2_matching() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let target = "HelLo-World";

    for offset in [0, 3] {
        let (mut score, mut matches) = do_score2(&if offset == 0 { target.to_string() } else { format!("123{target}") }, "HelLo-World", offset);

        assert!(score.is_some_and(|score| score != 0));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, offset);
        assert_eq!(matches[0].end, target.len() + offset);

        (score, matches) = do_score2(&if offset == 0 { target.to_string() } else { format!("123{target}") }, "HW", offset);

        assert!(score.is_some_and(|score| score != 0));
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].start, offset);
        assert_eq!(matches[0].end, 1 + offset);
        assert_eq!(matches[1].start, 6 + offset);
        assert_eq!(matches[1].end, 7 + offset);
    }
}

#[test]
fn fuzzy_score2_multiple_queries() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let target = "HelLo-World";

    let (first_single_score, first_single_matches) = do_score2(target, "HelLo", 0);
    let (second_single_score, second_single_matches) = do_score2(target, "World", 0);
    let first_and_second_single_matches: Vec<Match> = first_single_matches.iter().chain(&second_single_matches).copied().collect();

    let assert_score = |multi_score: Option<i32>, multi_matches: &[Match]| {
        assert!(multi_score.unwrap_or(0) >= (first_single_score.unwrap_or(0) + second_single_score.unwrap_or(0)));
        for (i, multi_match) in multi_matches.iter().enumerate() {
            let first_and_second_single_match = first_and_second_single_matches.get(i).expect("assert.fail()");
            assert_eq!(multi_match.start, first_and_second_single_match.start);
            assert_eq!(multi_match.end, first_and_second_single_match.end);
        }
    };

    let assert_no_score = |multi_score: Option<i32>, multi_matches: &[Match]| {
        assert_eq!(multi_score, None);
        assert_eq!(multi_matches.len(), 0);
    };

    let (multi_score, multi_matches) = do_score2(target, "HelLo World", 0);
    assert_score(multi_score, &multi_matches);

    let (multi_score, multi_matches) = do_score2(target, "World HelLo", 0);
    assert_score(multi_score, &multi_matches);

    let (multi_score, multi_matches) = do_score2(target, "World HelLo World", 0);
    assert_score(multi_score, &multi_matches);

    let (multi_score, multi_matches) = do_score2(target, "World HelLo Nothing", 0);
    assert_no_score(multi_score, &multi_matches);

    let (multi_score, multi_matches) = do_score2(target, "More Nothing", 0);
    assert_no_score(multi_score, &multi_matches);
}

#[test]
fn fuzzy_score2_95716() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let target = "# ❌ Wow";

    let score = do_score2(target, "❌", 0);
    assert!(score.0.is_some());
    assert!(!score.1.is_empty());
}

#[test]
fn using_quotes_should_expect_contiguous_matches_match() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    // missing the "i" in the query
    assert_eq!(do_score("contiguous", "\"contguous\"", None).0, 0);

    let score = do_score("contiguous", "\"contiguous\"", None);
    assert!(score.0 > 0);
}

#[test]
fn using_quotes_should_highlight_contiguous_indexes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let score = do_score("2021-7-26.md", "\"26\"", None);
    assert_eq!(score.0, 14);

    // The indexes of the 2 and 6 of "26"
    assert_eq!(score.1[0], 7);
    assert_eq!(score.1[1], 8);
}

#[test]
fn workspace_symbol_search_with_special_characters() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    // Simulates the scenario from the issue where rust-analyzer uses # and * as query modifiers
    // The original query (with special chars) should reach the language server
    // but normalized query (without special chars) should be used for fuzzy matching

    // Test #: User types "main#", language server returns "main" symbol
    let mut query = pq("main#");
    assert_eq!(query.original, "main#"); // Sent to language server
    assert_eq!(query.normalized, "main"); // Used for fuzzy matching
    let (mut score, mut matches) = do_score2("main", "main#", 0);
    assert!(score.is_some_and(|score| score > 0), "Should match \"main\" symbol when query is \"main#\"");
    assert!(!matches.is_empty());

    // Test *: User types "foo*", language server returns "foo" symbol
    query = pq("foo*");
    assert_eq!(query.original, "foo*"); // Sent to language server
    assert_eq!(query.normalized, "foo"); // Used for fuzzy matching
    (score, matches) = do_score2("foo", "foo*", 0);
    assert!(score.is_some_and(|score| score > 0), "Should match \"foo\" symbol when query is \"foo*\"");
    assert!(!matches.is_empty());

    // Test both: User types "MyClass#*", should match "MyClass"
    query = pq("MyClass#*");
    assert_eq!(query.original, "MyClass#*");
    assert_eq!(query.normalized, "MyClass");
    (score, matches) = do_score2("MyClass", "MyClass#*", 0);
    assert!(score.is_some_and(|score| score > 0), "Should match \"MyClass\" symbol when query is \"MyClass#*\"");
    assert!(!matches.is_empty());

    // Test fuzzy matching still works: User types "MC#", should match "MyClass"
    query = pq("MC#");
    assert_eq!(query.original, "MC#");
    assert_eq!(query.normalized, "MC");
    (score, matches) = do_score2("MyClass", "MC#", 0);
    assert!(score.is_some_and(|score| score > 0), "Should fuzzy match \"MyClass\" symbol when query is \"MC#\"");
    assert!(!matches.is_empty());

    // Make sure leading # or # in the middle are not removed.
    query = pq("#SpecialFunction");
    assert_eq!(query.original, "#SpecialFunction");
    assert_eq!(query.normalized, "#SpecialFunction");
    (score, matches) = do_score2("#SpecialFunction", "#SpecialFunction", 0);
    assert!(score.is_some_and(|score| score > 0), "Should match \"#SpecialFunction\" symbol when query is \"#SpecialFunction\"");
    assert!(!matches.is_empty());

    // Make sure standalone # is not removed
    query = pq("#");
    assert_eq!(query.original, "#");
    assert_eq!(query.normalized, "#", "Standalone # should not be removed");
    (score, matches) = do_score2("#", "#", 0);
    assert!(score.is_some_and(|score| score > 0), "Should match \"#\" symbol when query is \"#\"");
    assert!(!matches.is_empty());
}
