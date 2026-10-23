//! Porte de `src/vs/base/common/tfIdf.ts`: o tf-idf (frequência do termo, inverso da frequência nos
//! documentos) para um conjunto de documentos em que cada um tem um ou mais pedaços de texto.
//!
//! Decisões de tradução:
//! - O texto de entrada é `&str`: o resultado é o mesmo que o de `JsString`, porque só letras entram nos
//!   termos e um surrogate isolado, que o `&str` não representa, nunca é letra (separa palavras, como o
//!   U+FFFD). Os termos e as chaves dos documentos são `String`.
//! - As expressões regulares (`\b\p{Letter}[\p{Letter}\d]{2,}\b` com `gu`, `\p{Letter}{3,}`) rodam no
//!   `zyarr` sobre UTF-16, porque o `\b` do modo unicode sem `i` usa o `\w` ASCII e `\p{Letter}` é a
//!   categoria geral `L`, e nenhum dos dois coincide com os predicados de `char`.
//! - `Map` é [`OrderedMap`] (a ordem de inserção do `Map` do JavaScript), e o embedding esparso, o
//!   `Object.create(null)` do original, é um `Vec` de pares na mesma ordem: a soma de ponto flutuante de
//!   `computeSimilarityScore` percorre os termos da consulta nessa ordem. Nenhum termo começa por dígito,
//!   então a ordem de `Object.entries` (índices inteiros primeiro) é sempre a de inserção.
//! - O `Math.log` do V8 (fdlibm) e o `f64::ln` do Rust podem divergir no último bit; a ordenação dos
//!   resultados só depende disso em empates exatos de pontuação.
//! - `normalizeTfIdfScores` copia o array só por cima (`slice(0)`) e normaliza os mesmos objetos, ou seja,
//!   muda as pontuações do array de entrada. Aqui a entrada fica intacta e a função devolve novas
//!   pontuações normalizadas, num tipo próprio ([`NormalizedTfIdfScore`]).

use std::collections::HashMap;

use zyarr::RegExp;

use crate::arrays::to_ordering;
use crate::cancellation::CancellationToken;
use crate::js_regexp::match_all;
use crate::map::OrderedMap;

type TermFrequencies = OrderedMap<String, usize>;
type DocumentOccurrences = HashMap<String, usize>;
/// Um embedding esparso: o peso de cada palavra, na ordem em que a palavra apareceu na consulta.
type SparseEmbedding = Vec<(String, f64)>;

struct DocumentChunkEntry {
    tf: TermFrequencies,
}

/// Um documento: uma chave e os pedaços de texto.
#[derive(Clone, Debug, PartialEq)]
pub struct TfIdfDocument {
    pub key: String,
    pub text_chunks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TfIdfScore {
    pub key: String,
    /// Um número sem limite.
    pub score: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedTfIdfScore {
    pub key: String,
    /// Um número entre 0 e 1.
    pub score: f64,
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn from_utf16(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

thread_local! {
    /// `/\b\p{Letter}[\p{Letter}\d]{2,}\b/gu`
    static WORD: RegExp = RegExp::from_str(r"\b\p{Letter}[\p{Letter}\d]{2,}\b", "gu").expect("regex de palavra válida");
    /// `/\p{Letter}{3,}/u` (o `g` do original não tem efeito: a regex literal é nova a cada avaliação)
    static THREE_LETTERS: RegExp = RegExp::from_str(r"\p{Letter}{3,}", "u").expect("regex de letras válida");
}

fn count_map_from(values: impl IntoIterator<Item = String>) -> TermFrequencies {
    let mut map = TermFrequencies::new();
    for value in values {
        let count = map.get(&value).copied().unwrap_or(0) + 1;
        map.insert(value, count);
    }
    map
}

/// Quebra uma string em termos (palavras).
fn split_terms(input: &str) -> Vec<String> {
    let normalize = |word: &str| word.to_lowercase();
    let units = utf16(input);
    let mut terms = Vec::new();

    // Only match on words that are at least 3 characters long and start with a letter
    for found in WORD.with(|word| match_all(word, &units)) {
        let word = from_utf16(&units[found.start()..found.end()]);
        terms.push(normalize(&word));

        // `word.replace(/([a-z])([A-Z])/g, '$1 $2').split(/\s+/g)`: a palavra só tem letras e dígitos, então
        // os únicos espaços são os inseridos entre minúscula e maiúscula ASCII (um casamento não se
        // sobrepõe ao seguinte, porque a maiúscula que fecha um não pode abrir outro).
        let mut camel_parts: Vec<String> = vec![String::new()];
        let mut previous: Option<char> = None;
        for c in word.chars() {
            if previous.is_some_and(|p| p.is_ascii_lowercase()) && c.is_ascii_uppercase() {
                camel_parts.push(String::new());
            }
            camel_parts.last_mut().expect("sempre há uma parte").push(c);
            previous = Some(c);
        }
        if camel_parts.len() > 1 {
            for part in &camel_parts {
                let part_units = utf16(part);
                // Require at least 3 letters in the parts of a camel case word
                if part_units.len() > 2 && THREE_LETTERS.with(|re| re.exec(&part_units, 0).is_some()) {
                    terms.push(normalize(part));
                }
            }
        }
    }
    terms
}

/// Conta quantas vezes cada termo (palavra) aparece numa string.
fn term_frequencies(input: &str) -> TermFrequencies {
    count_map_from(split_terms(input))
}

/// Implementação de tf-idf para um conjunto de documentos em que cada um tem um ou mais pedaços de texto.
/// Cada documento é identificado por uma chave, e a pontuação de um documento é a máxima entre os pedaços.
#[derive(Default)]
pub struct TfIdfCalculator {
    /// Total number of chunks
    chunk_count: usize,
    chunk_occurrences: DocumentOccurrences,
    documents: OrderedMap<String, Vec<DocumentChunkEntry>>,
}

impl TfIdfCalculator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn calculate_scores(&self, query: &str, token: &CancellationToken) -> Vec<TfIdfScore> {
        let embedding = self.compute_embedding(query);
        let mut idf_cache: HashMap<String, f64> = HashMap::new();
        let mut scores = Vec::new();
        // For each document, generate one score
        for (key, chunks) in self.documents.iter() {
            if token.is_cancellation_requested() {
                return Vec::new();
            }

            for chunk in chunks {
                let score = self.compute_similarity_score(chunk, &embedding, &mut idf_cache);
                if score > 0.0 {
                    scores.push(TfIdfScore { key: key.clone(), score });
                }
            }
        }

        scores
    }

    pub fn update_documents(&mut self, documents: &[TfIdfDocument]) -> &mut Self {
        for document in documents {
            self.delete_document(&document.key);
        }

        for document in documents {
            let mut chunks = Vec::with_capacity(document.text_chunks.len());
            for text in &document.text_chunks {
                let tf = term_frequencies(text);

                // Update occurrences list
                for (term, _) in tf.iter() {
                    *self.chunk_occurrences.entry(term.clone()).or_insert(0) += 1;
                }

                chunks.push(DocumentChunkEntry { tf });
            }

            self.chunk_count += chunks.len();
            self.documents.insert(document.key.clone(), chunks);
        }
        self
    }

    pub fn delete_document(&mut self, key: &str) {
        let Some(chunks) = self.documents.remove(&key.to_owned()) else {
            return;
        };

        self.chunk_count -= chunks.len();

        // Update term occurrences for the document
        for chunk in &chunks {
            for (term, _) in chunk.tf.iter() {
                if let Some(&current_occurrences) = self.chunk_occurrences.get(term) {
                    if current_occurrences <= 1 {
                        self.chunk_occurrences.remove(term);
                    } else {
                        self.chunk_occurrences.insert(term.clone(), current_occurrences - 1);
                    }
                }
            }
        }
    }

    fn compute_similarity_score(&self, chunk: &DocumentChunkEntry, query_embedding: &SparseEmbedding, idf_cache: &mut HashMap<String, f64>) -> f64 {
        // Compute the dot product between the chunk's embedding and the query embedding

        // Note that the chunk embedding is computed lazily on a per-term basis.
        // This lets us skip a large number of calculations because the majority
        // of chunks do not share any terms with the query.

        let mut sum = 0.0;
        for (term, term_tfidf) in query_embedding {
            let Some(&chunk_tf) = chunk.tf.get(term) else {
                // Term does not appear in chunk so it has no contribution
                continue;
            };

            let chunk_idf = *idf_cache.entry(term.clone()).or_insert_with(|| self.compute_idf(term));

            let chunk_tfidf = chunk_tf as f64 * chunk_idf;
            sum += chunk_tfidf * term_tfidf;
        }
        sum
    }

    fn compute_embedding(&self, input: &str) -> SparseEmbedding {
        let tf = term_frequencies(input);
        self.compute_tfidf(&tf)
    }

    fn compute_idf(&self, term: &str) -> f64 {
        let chunk_occurrences = self.chunk_occurrences.get(term).copied().unwrap_or(0);
        if chunk_occurrences > 0 { ((self.chunk_count + 1) as f64 / chunk_occurrences as f64).ln() } else { 0.0 }
    }

    fn compute_tfidf(&self, term_frequencies: &TermFrequencies) -> SparseEmbedding {
        let mut embedding = SparseEmbedding::new();
        for (word, &occurrences) in term_frequencies.iter() {
            let idf = self.compute_idf(word);
            if idf > 0.0 {
                embedding.push((word.clone(), occurrences as f64 * idf));
            }
        }
        embedding
    }
}

/// Normaliza as pontuações para ficarem entre 0 e 1 e as ordena em ordem decrescente.
/// `scores` são as pontuações de [`TfIdfCalculator::calculate_scores`].
pub fn normalize_tf_idf_scores(scores: &[TfIdfScore]) -> Vec<NormalizedTfIdfScore> {
    let mut result: Vec<NormalizedTfIdfScore> = scores.iter().map(|s| NormalizedTfIdfScore { key: s.key.clone(), score: s.score }).collect();

    // sort descending
    result.sort_by(|a, b| to_ordering(b.score - a.score));

    // normalize
    let max = result.first().map_or(0.0, |s| s.score);
    if max > 0.0 {
        for score in &mut result {
            score.score /= max;
        }
    }

    result
}
