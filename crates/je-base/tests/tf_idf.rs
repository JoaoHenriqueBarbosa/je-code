//! Porte de `src/vs/base/test/common/tfIdf.test.ts` (suíte `TF-IDF Calculator`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `a.key.localeCompare(b.key)` é o `Collator` padrão do porte (`je_base::collator`).
//! - `makeDocument(key, content)` recebe `string | string[]`: aqui `&[&str]`, com um elemento para o texto único.
//! - `permutate` devolve os arranjos como `Vec<Vec<T>>`, na mesma ordem do original.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::cancellation::CancellationToken;
use je_base::collator::{Collator, CollatorOptions};
use je_base::tf_idf::{TfIdfCalculator, TfIdfDocument, TfIdfScore};

/// Generates all permutations of an array.
///
/// This is useful for testing to make sure order does not effect the result.
fn permutate<T: Clone>(arr: &[T]) -> Vec<Vec<T>> {
    if arr.is_empty() {
        return vec![Vec::new()];
    }

    let mut result = Vec::new();

    for i in 0..arr.len() {
        let rest: Vec<T> = arr[..i].iter().chain(&arr[i + 1..]).cloned().collect();
        for permutation in permutate(&rest) {
            let mut item = vec![arr[i].clone()];
            item.extend(permutation);
            result.push(item);
        }
    }

    result
}

fn assert_score_orders_equal(mut actual_scores: Vec<TfIdfScore>, expected_score_keys: &[&str]) {
    let collator = Collator::new(None, CollatorOptions::default());
    actual_scores.sort_by(|a, b| {
        let by_score = b.score - a.score;
        let order = if by_score != 0.0 && !by_score.is_nan() {
            by_score
        } else {
            let a_key: Vec<u16> = a.key.encode_utf16().collect();
            let b_key: Vec<u16> = b.key.encode_utf16().collect();
            f64::from(collator.compare(&a_key, &b_key))
        };
        je_base::arrays::to_ordering(order)
    });
    assert_eq!(actual_scores.len(), expected_score_keys.len());
    for (actual, expected) in actual_scores.iter().zip(expected_score_keys) {
        assert_eq!(actual.key, *expected);
    }
}

fn make_document(key: &str, content: &[&str]) -> TfIdfDocument {
    TfIdfDocument { key: key.to_owned(), text_chunks: content.iter().map(ToString::to_string).collect() }
}

fn calculate(docs: &[TfIdfDocument], query: &str) -> Vec<TfIdfScore> {
    let mut tfidf = TfIdfCalculator::new();
    tfidf.update_documents(docs);
    tfidf.calculate_scores(query, &CancellationToken::None)
}

#[test]
fn should_return_no_scores_when_no_documents_are_given() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let tfidf = TfIdfCalculator::new();
    let scores = tfidf.calculate_scores("something", &CancellationToken::None);
    assert_score_orders_equal(scores, &[]);
}

#[test]
fn should_return_no_scores_for_term_not_in_document() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let scores = calculate(&[make_document("A", &["cat dog fish"])], "elepant");
    assert_score_orders_equal(scores, &[]);
}

#[test]
fn should_return_scores_for_document_with_exact_match() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for docs in permutate(&[make_document("A", &["cat dog cat"]), make_document("B", &["cat fish"])]) {
        let scores = calculate(&docs, "dog");
        assert_score_orders_equal(scores, &["A"]);
    }
}

#[test]
fn should_return_document_with_more_matches_first() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for docs in permutate(&[make_document("/A", &["cat dog cat"]), make_document("/B", &["cat fish"]), make_document("/C", &["frog"])]) {
        let scores = calculate(&docs, "cat");
        assert_score_orders_equal(scores, &["/A", "/B"]);
    }
}

#[test]
fn should_return_document_with_more_matches_first_when_term_appears_in_all_documents() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for docs in permutate(&[make_document("/A", &["cat dog cat cat"]), make_document("/B", &["cat fish"]), make_document("/C", &["frog cat cat"])]) {
        let scores = calculate(&docs, "cat");
        assert_score_orders_equal(scores, &["/A", "/C", "/B"]);
    }
}

#[test]
fn should_weigh_less_common_term_higher() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for docs in permutate(&[
        make_document("/A", &["cat dog cat"]),
        make_document("/B", &["fish"]),
        make_document("/C", &["cat cat cat cat"]),
        make_document("/D", &["cat fish"]),
    ]) {
        let scores = calculate(&docs, "cat the dog");
        assert_score_orders_equal(scores, &["/A", "/C", "/D"]);
    }
}

#[test]
fn should_weigh_chunks_with_less_common_terms_higher() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let docs_a = || make_document("/A", &["cat dog cat", "fish"]);
    let docs_b = || make_document("/B", &["cat cat cat cat dog", "dog"]);

    for docs in permutate(&[docs_a(), docs_b()]) {
        let scores = calculate(&docs, "cat");
        assert_score_orders_equal(scores, &["/B", "/A"]);
    }

    for docs in permutate(&[docs_a(), docs_b()]) {
        let scores = calculate(&docs, "dog");
        assert_score_orders_equal(scores, &["/A", "/B", "/B"]);
    }

    for docs in permutate(&[docs_a(), docs_b()]) {
        let scores = calculate(&docs, "cat the dog");
        assert_score_orders_equal(scores, &["/B", "/A", "/B"]);
    }

    for docs in permutate(&[docs_a(), docs_b()]) {
        let scores = calculate(&docs, "lake fish");
        assert_score_orders_equal(scores, &["/A"]);
    }
}

#[test]
fn should_ignore_case_and_punctuation() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for docs in permutate(&[make_document("/A", &["Cat doG.cat"]), make_document("/B", &["cAt fiSH"]), make_document("/C", &["frOg"])]) {
        let scores = calculate(&docs, ". ,CaT!  ");
        assert_score_orders_equal(scores, &["/A", "/B"]);
    }
}

#[test]
fn should_match_on_camel_case_words() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    for docs in permutate(&[make_document("/A", &["catDog cat"]), make_document("/B", &["fishCatFish"]), make_document("/C", &["frogcat"])]) {
        let scores = calculate(&docs, "catDOG");
        assert_score_orders_equal(scores, &["/A", "/B"]);
    }
}

#[test]
fn should_not_match_document_after_delete() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let doc_a = make_document("/A", &["cat dog cat"]);
    let doc_b = make_document("/B", &["cat fish"]);
    let doc_c = make_document("/C", &["frog"]);

    let mut tfidf = TfIdfCalculator::new();
    tfidf.update_documents(&[doc_a.clone(), doc_b.clone(), doc_c.clone()]);
    let mut scores = tfidf.calculate_scores("cat", &CancellationToken::None);
    assert_score_orders_equal(scores, &["/A", "/B"]);

    tfidf.delete_document(&doc_a.key);
    scores = tfidf.calculate_scores("cat", &CancellationToken::None);
    assert_score_orders_equal(scores, &["/B"]);

    tfidf.delete_document(&doc_c.key);
    scores = tfidf.calculate_scores("cat", &CancellationToken::None);
    assert_score_orders_equal(scores, &["/B"]);

    tfidf.delete_document(&doc_b.key);
    scores = tfidf.calculate_scores("cat", &CancellationToken::None);
    assert_score_orders_equal(scores, &[]);
}
