//! Testes do `tfIdf.ts` que o `.test.ts` do VS Code não cobre: `normalizeTfIdfScores`, o cancelamento e a
//! atualização de um documento existente.

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::tf_idf::{TfIdfCalculator, TfIdfDocument, TfIdfScore, normalize_tf_idf_scores};

fn document(key: &str, text: &str) -> TfIdfDocument {
    TfIdfDocument { key: key.to_owned(), text_chunks: vec![text.to_owned()] }
}

fn score(key: &str, score: f64) -> TfIdfScore {
    TfIdfScore { key: key.to_owned(), score }
}

#[test]
fn normalize_sorts_descending_and_scales_to_the_maximum() {
    let input = vec![score("a", 1.0), score("b", 4.0), score("c", 2.0)];

    let normalized = normalize_tf_idf_scores(&input);

    assert_eq!(normalized.iter().map(|s| (s.key.as_str(), s.score)).collect::<Vec<_>>(), vec![("b", 1.0), ("c", 0.5), ("a", 0.25)]);
    assert_eq!(input[0].score, 1.0, "a entrada não é alterada");
    assert!(normalize_tf_idf_scores(&[]).is_empty());
    assert_eq!(normalize_tf_idf_scores(&[score("z", 0.0)])[0].score, 0.0);
}

#[test]
fn a_cancelled_token_gives_no_scores() {
    let mut tfidf = TfIdfCalculator::new();
    tfidf.update_documents(&[document("/A", "cat dog"), document("/B", "fish")]);
    let source = CancellationTokenSource::new(None);
    source.cancel();

    assert!(tfidf.calculate_scores("cat", &source.token()).is_empty());
    assert_eq!(tfidf.calculate_scores("cat", &CancellationToken::None).len(), 1);
}

#[test]
fn updating_a_document_replaces_its_chunks() {
    let mut tfidf = TfIdfCalculator::new();
    tfidf.update_documents(&[document("/A", "cat"), document("/B", "fish")]);
    tfidf.update_documents(&[document("/A", "dog")]);

    assert!(tfidf.calculate_scores("cat", &CancellationToken::None).is_empty());
    assert_eq!(tfidf.calculate_scores("dog", &CancellationToken::None)[0].key, "/A");
}
