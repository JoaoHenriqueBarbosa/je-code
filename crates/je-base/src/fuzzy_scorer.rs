//! Porte de `src/vs/base/common/fuzzyScorer.ts`: a pontuação fuzzy do quick open, da busca de arquivos e
//! de símbolos (`scoreFuzzy`, `scoreFuzzy2`, `scoreItemFuzzy`, `compareItemsByFuzzyScore`, `prepareQuery`).
//!
//! Texto é `&[u16]` (unidades UTF-16) ou [`JsString`], e as posições e trechos ([`Match`]) são índices
//! UTF-16, como no original.
//!
//! O original lê `isLinux`, `isWindows` e `sep` (do `platform.ts` e do `path.ts`). Aqui isso é o
//! [`Platform`] recebido por [`prepare_query`], [`piece_to_query`], [`pieces_to_query`], [`score_item_fuzzy`]
//! e [`compare_items_by_fuzzy_score`] (o separador é `\` no Windows e `/` nos demais).
//!
//! Diferenças do original:
//! - `string | undefined` das funções de acesso (`getItemLabel` e companhia) é `Option<JsString>`, e um
//!   `item` nulo de `scoreItemFuzzy` é `Option<&T>`. Texto vazio conta como ausente onde o original testa
//!   `!label`, `!!path` ou `description ? ... : ...`.
//! - O [`ItemScore`] e o [`PreparedQuery`] são structs; `IPreparedQuery extends IPreparedQueryPiece` vira o
//!   `Deref` do [`PreparedQuery`] para a peça, então `query.normalized` funciona. O `scoreFuzzy2` aceita um ou
//!   outro por [`QueryArg`].
//! - O `FuzzyScorerCache` (`{ [key: string]: IItemScore }`) é um `HashMap<i32, Rc<ItemScore>>`: a chave é o
//!   hash `int32` do original (que lá vira o nome da propriedade), e o `Rc` mantém a identidade do resultado
//!   em cache, que o original devolve pelo mesmo objeto.
//! - O `NO_ITEM_SCORE` congelado é um `Rc` compartilhado por thread.
//! - `pieceToQuery` e suas duas sobrecargas são [`piece_to_query`] e [`pieces_to_query`].

use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::LazyLock;

use crate::JsString;
use crate::char_code as cc;
use crate::comparers::compare_anything;
use crate::dynamic::Dynamic;
use crate::filters::{FuzzyScoreOptions, Match, create_matches as create_fuzzy_matches, fuzzy_score, is_upper, matches_prefix};
use crate::hash::{ARRAY_HASH_SEED, ConstantStringHash, OBJECT_HASH_SEED, do_hash, number_hash, string_hash};
use crate::js_string::{self, char_code_at, is_js_whitespace, starts_with_at};
use crate::platform::Platform;
use crate::strings::equals_ignore_case;

//#region Fuzzy scorer

/// `FuzzyScore`: a pontuação e as posições do casamento no alvo.
pub type FuzzyScore = (i32, Vec<usize>);
/// `FuzzyScorerCache`.
pub type FuzzyScorerCache = HashMap<i32, Rc<ItemScore>>;

const NO_MATCH: i32 = 0;

fn no_score() -> FuzzyScore {
    (NO_MATCH, Vec::new())
}

/// `scoreFuzzy`.
pub fn score_fuzzy(target: &[u16], query: &[u16], query_lower: &[u16], allow_non_contiguous_matches: bool) -> FuzzyScore {
    if target.is_empty() || query.is_empty() {
        return no_score(); // volta cedo se o alvo ou a consulta não existem
    }

    let target_length = target.len();
    let query_length = query.len();

    if target_length < query_length {
        return no_score(); // é impossível a consulta estar contida no alvo
    }

    let target_lower = JsString::from(target).to_lower_case();
    do_score_fuzzy(query, query_lower, query_length, target, target_lower.units(), target_length, allow_non_contiguous_matches)
}

fn do_score_fuzzy(
    query: &[u16],
    query_lower: &[u16],
    query_length: usize,
    target: &[u16],
    target_lower: &[u16],
    target_length: usize,
    allow_non_contiguous_matches: bool,
) -> FuzzyScore {
    let mut scores = vec![0i32; query_length * target_length];
    let mut matches = vec![0i32; query_length * target_length];

    //
    // Monta a matriz de pontuação:
    //
    // A matriz é feita da consulta q e do alvo t. Para cada índice pontua q[i] com t[i] e compara com a
    // pontuação anterior. Se a pontuação é igual ou maior, mantém o casamento. Além da pontuação, guarda
    // também o comprimento dos casamentos consecutivos, para usar como bônus na pontuação.
    //
    //      t   a   r   g   e   t
    //  q
    //  u
    //  e
    //  r
    //  y
    //
    for query_index in 0..query_length {
        let query_index_offset = query_index * target_length;
        let query_index_gt_null = query_index > 0;

        let query_char_at_index = char_code_at(query, query_index as i64);
        let query_lower_char_at_index = char_code_at(query_lower, query_index as i64);

        for target_index in 0..target_length {
            let target_index_gt_null = target_index > 0;

            let current_index = query_index_offset + target_index;
            let left_index = current_index.wrapping_sub(1);
            let diag_index = (query_index_offset + target_index).wrapping_sub(target_length).wrapping_sub(1);

            let left_score = if target_index_gt_null { scores[left_index] } else { 0 };
            let diag_score = if query_index_gt_null && target_index_gt_null { scores[diag_index] } else { 0 };

            let matches_sequence_length = if query_index_gt_null && target_index_gt_null { matches[diag_index] } else { 0 };

            // Se não estamos mais casando o primeiro caractere da consulta, só produzimos uma pontuação se
            // houve uma pontuação antes para o último índice da consulta (olhando o diag_score). Isso garante
            // que a consulta sempre case em sequência com o alvo. Por exemplo, com o alvo "ede" e a consulta
            // "de", sem isso produziríamos uma pontuação alta errada para query[1] ("e") casando com
            // target[0] ("e") por causa do bônus de "começo de palavra".
            let score = if diag_score == 0 && query_index_gt_null {
                0
            } else {
                compute_char_score(query_char_at_index, query_lower_char_at_index, target, target_lower, target_index, matches_sequence_length)
            };

            // Temos uma pontuação e ela é igual ou maior que a da esquerda
            // Casamento: a sequência continua crescendo a partir do valor da diagonal anterior
            // Pontuação: aumenta pelo valor da pontuação da diagonal
            let is_valid_score = score != 0 && diag_score + score >= left_score;
            if is_valid_score
                && (
                    // Não precisamos conferir se é contíguo se permitimos casamentos não contíguos
                    allow_non_contiguous_matches ||
                // Precisamos de um casamento contíguo. Estar num índice acima de 0 na consulta significa que
                // já descobrimos que é contíguo, senão não haveria pontuação
                query_index_gt_null ||
                // por fim, confere se a consulta é totalmente contígua neste índice do alvo
                starts_with_at(target_lower, query_lower, target_index)
                )
            {
                matches[current_index] = matches_sequence_length + 1;
                scores[current_index] = diag_score + score;
            }
            // Ou não temos pontuação, ou ela é menor que a da esquerda
            // Casamento: volta a 0
            // Pontuação: continua da esquerda
            else {
                matches[current_index] = NO_MATCH;
                scores[current_index] = left_score;
            }
        }
    }

    // Recupera as posições (começando do canto inferior direito da matriz)
    let mut positions = Vec::new();
    let mut query_index = query_length as i64 - 1;
    let mut target_index = target_length as i64 - 1;
    while query_index >= 0 && target_index >= 0 {
        let current_index = (query_index as usize) * target_length + target_index as usize;
        if matches[current_index] == NO_MATCH {
            target_index -= 1; // vai para a esquerda
        } else {
            positions.push(target_index as usize);

            // sobe e vai para a esquerda
            query_index -= 1;
            target_index -= 1;
        }
    }

    positions.reverse();
    (scores[query_length * target_length - 1], positions)
}

fn compute_char_score(
    query_char_at_index: Option<u16>,
    query_lower_char_at_index: Option<u16>,
    target: &[u16],
    target_lower: &[u16],
    target_index: usize,
    matches_sequence_length: i32,
) -> i32 {
    let mut score = 0;

    if !consider_as_equal(query_lower_char_at_index, char_code_at(target_lower, target_index as i64)) {
        return score; // os caracteres não casam
    }

    // Bônus de caractere casado
    score += 1;

    // Bônus de casamento consecutivo: sequências de até 3 recebem o bônus inteiro (6) e o resto recebe
    // metade dele. Isso ajuda a reduzir o impulso total de casamentos de sequências longas.
    if matches_sequence_length > 0 {
        score += (matches_sequence_length.min(3) * 6) + ((matches_sequence_length - 3).max(0) * 3);
    }

    // Bônus de mesma caixa
    if query_char_at_index == char_code_at(target, target_index as i64) {
        score += 1;
    }

    // Bônus de começo de palavra
    if target_index == 0 {
        score += 8;
    } else {
        // Bônus de depois de separador
        let separator_bonus = score_separator_at_pos(char_code_at(target, target_index as i64 - 1));
        if separator_bonus != 0 {
            score += separator_bonus;
        }
        // Bônus de maiúscula dentro da palavra (camel case). Só damos este bônus se não estamos numa sequência
        // contígua. Por exemplo:
        // NPE => NullPointerException = bônus
        // HTTP => HTTP = sem bônus
        else if char_code_at(target, target_index as i64).is_some_and(is_upper) && matches_sequence_length == 0 {
            score += 2;
        }
    }

    score
}

fn consider_as_equal(a: Option<u16>, b: Option<u16>) -> bool {
    if a == b {
        return true;
    }

    // Caso especial dos separadores de caminho: ignora as diferenças de plataforma
    if a == Some(cc::Slash) || a == Some(cc::Backslash) {
        return b == Some(cc::Slash) || b == Some(cc::Backslash);
    }

    false
}

fn score_separator_at_pos(char_code: Option<u16>) -> i32 {
    match char_code {
        Some(cc::Slash | cc::Backslash) => 5, // prefere separadores de caminho...
        Some(cc::Underline | cc::Dash | cc::Period | cc::Space | cc::SingleQuote | cc::DoubleQuote | cc::Colon) => 4, // ...a outros separadores
        _ => 0,
    }
}

//#endregion

//#region Alternate fuzzy scorer implementation that is e.g. used for symbols

/// `FuzzyScore2`.
pub type FuzzyScore2 = (Option<i32>, Vec<Match>);

fn no_score2() -> FuzzyScore2 {
    (None, Vec::new())
}

/// O `IPreparedQuery | IPreparedQueryPiece` do `scoreFuzzy2`.
pub enum QueryArg<'a> {
    Query(&'a PreparedQuery),
    Piece(&'a PreparedQueryPiece),
}

impl<'a> From<&'a PreparedQuery> for QueryArg<'a> {
    fn from(query: &'a PreparedQuery) -> Self {
        QueryArg::Query(query)
    }
}

impl<'a> From<&'a PreparedQueryPiece> for QueryArg<'a> {
    fn from(piece: &'a PreparedQueryPiece) -> Self {
        QueryArg::Piece(piece)
    }
}

/// `scoreFuzzy2`.
pub fn score_fuzzy2<'a>(target: &[u16], query: impl Into<QueryArg<'a>>, pattern_start: usize, word_start: usize) -> FuzzyScore2 {
    match query.into() {
        // Pontuação: várias entradas
        QueryArg::Query(PreparedQuery { values: Some(values), .. }) if values.len() > 1 => {
            do_score_fuzzy2_multiple(target, values, pattern_start, word_start)
        }
        // Pontuação: uma entrada
        QueryArg::Query(query) => do_score_fuzzy2_single(target, query, pattern_start, word_start),
        QueryArg::Piece(piece) => do_score_fuzzy2_single(target, piece, pattern_start, word_start),
    }
}

fn do_score_fuzzy2_multiple(target: &[u16], query: &[PreparedQueryPiece], pattern_start: usize, word_start: usize) -> FuzzyScore2 {
    let mut total_score = 0;
    let mut total_matches: Vec<Match> = Vec::new();

    for query_piece in query {
        let (score, matches) = do_score_fuzzy2_single(target, query_piece, pattern_start, word_start);
        let Some(score) = score else {
            // se uma única entrada da consulta não casa, volta sem pontuação nenhuma, porque exigimos que
            // todas casem
            return no_score2();
        };

        total_score += score;
        total_matches.extend(matches);
    }

    // se temos uma pontuação, garante que as posições estão em ordem crescente e distintas
    (Some(total_score), normalize_matches(total_matches))
}

fn do_score_fuzzy2_single(target: &[u16], query: &PreparedQueryPiece, pattern_start: usize, word_start: usize) -> FuzzyScore2 {
    let target_lower = JsString::from(target).to_lower_case();
    let score = fuzzy_score(
        query.normalized.units(),
        query.normalized_lowercase.units(),
        pattern_start,
        target,
        target_lower.units(),
        word_start,
        Some(&FuzzyScoreOptions { first_match_can_be_weak: true, boost_full_match: true }),
    );
    let Some(score) = score else { return no_score2() };

    (Some(score[0]), create_fuzzy_matches(Some(&score)))
}

//#endregion

//#region Item (label, description, path) scorer

/// `IItemScore`: a pontuação de um item estruturado que tem um rótulo e uma descrição opcional.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemScore {
    /// Pontuação geral.
    pub score: i32,
    /// Casamentos dentro do rótulo.
    pub label_match: Option<Vec<Match>>,
    /// Casamentos dentro da descrição.
    pub description_match: Option<Vec<Match>>,
}

thread_local! {
    static NO_ITEM_SCORE: Rc<ItemScore> = Rc::new(ItemScore::default());
}

fn no_item_score() -> Rc<ItemScore> {
    NO_ITEM_SCORE.with(Rc::clone)
}

/// `IItemAccessor<T>`.
pub trait ItemAccessor<T> {
    /// Só o rótulo do item a pontuar.
    fn get_item_label(&self, item: &T) -> Option<JsString>;

    /// A descrição opcional do item a pontuar.
    fn get_item_description(&self, item: &T) -> Option<JsString>;

    /// Se o item é um arquivo, o caminho dele a pontuar.
    fn get_item_path(&self, file: &T) -> Option<JsString>;
}

const PATH_IDENTITY_SCORE: i32 = 1 << 18;
const LABEL_PREFIX_SCORE_THRESHOLD: i32 = 1 << 17;
const LABEL_SCORE_THRESHOLD: i32 = 1 << 16;

fn key_hash(name: &str) -> ConstantStringHash {
    ConstantStringHash::new(&name.encode_utf16().collect::<Vec<u16>>())
}

/// Reproduz o hash genérico da chave do cache sem montar a forma aninhada do objeto.
fn get_cache_hash(label: &JsString, description: Option<&JsString>, allow_non_contiguous_matches: bool, query: &PreparedQuery) -> i32 {
    static ALLOW_NON_CONTIGUOUS_MATCHES_KEY_HASH: LazyLock<ConstantStringHash> = LazyLock::new(|| key_hash("allowNonContiguousMatches"));
    static DESCRIPTION_KEY_HASH: LazyLock<ConstantStringHash> = LazyLock::new(|| key_hash("description"));
    static LABEL_KEY_HASH: LazyLock<ConstantStringHash> = LazyLock::new(|| key_hash("label"));
    static VALUES_KEY_HASH: LazyLock<ConstantStringHash> = LazyLock::new(|| key_hash("values"));
    static EXPECT_CONTIGUOUS_MATCH_KEY_HASH: LazyLock<ConstantStringHash> = LazyLock::new(|| key_hash("expectContiguousMatch"));
    static VALUE_KEY_HASH: LazyLock<ConstantStringHash> = LazyLock::new(|| key_hash("value"));

    let mut hash_val = number_hash(f64::from(OBJECT_HASH_SEED), 0);
    hash_val = string_hash(query.normalized.units(), hash_val);

    // Preserva a ordem de propriedades ordenadas do `objectHash`, inclusive os campos de cada peça da consulta.
    hash_val = number_hash(f64::from(OBJECT_HASH_SEED), hash_val);

    hash_val = ALLOW_NON_CONTIGUOUS_MATCHES_KEY_HASH.apply(hash_val);
    hash_val = do_hash(&Dynamic::Bool(allow_non_contiguous_matches), hash_val);

    hash_val = DESCRIPTION_KEY_HASH.apply(hash_val);
    hash_val = do_hash(&description.map_or(Dynamic::Undefined, |description| Dynamic::String(description.clone())), hash_val);

    hash_val = LABEL_KEY_HASH.apply(hash_val);
    hash_val = do_hash(&Dynamic::String(label.clone()), hash_val);

    hash_val = VALUES_KEY_HASH.apply(hash_val);
    hash_val = number_hash(f64::from(ARRAY_HASH_SEED), hash_val);
    let values: &[PreparedQueryPiece] = match &query.values {
        Some(values) => values,
        None => std::slice::from_ref(&query.piece),
    };
    for value in values {
        hash_val = number_hash(f64::from(OBJECT_HASH_SEED), hash_val);
        hash_val = EXPECT_CONTIGUOUS_MATCH_KEY_HASH.apply(hash_val);
        hash_val = do_hash(&Dynamic::Bool(value.expect_contiguous_match), hash_val);
        hash_val = VALUE_KEY_HASH.apply(hash_val);
        hash_val = do_hash(&Dynamic::String(value.normalized.clone()), hash_val);
    }

    hash_val
}

/// `scoreItemFuzzy`.
pub fn score_item_fuzzy<T>(
    item: Option<&T>,
    query: &PreparedQuery,
    allow_non_contiguous_matches: bool,
    accessor: &dyn ItemAccessor<T>,
    cache: &mut FuzzyScorerCache,
    platform: &Platform,
) -> Rc<ItemScore> {
    let Some(item) = item else { return no_item_score() };
    if query.normalized.is_empty() {
        return no_item_score(); // precisamos de um item e de uma consulta para pontuar, no mínimo
    }

    let Some(label) = accessor.get_item_label(item).filter(|label| !label.is_empty()) else {
        return no_item_score(); // precisamos de um rótulo, no mínimo
    };

    let description = accessor.get_item_description(item);

    // para acelerar a pontuação, guardamos o resultado em cache com um hash único baseado em:
    // - rótulo
    // - descrição (se houver)
    // - se o casamento não contíguo está ligado ou não
    // - hash dos valores (normalizados) da consulta
    let cache_hash = get_cache_hash(&label, description.as_ref(), allow_non_contiguous_matches, query);
    if let Some(cached) = cache.get(&cache_hash) {
        return cached.clone();
    }

    let item_score = Rc::new(do_score_item_fuzzy(&label, description.as_ref(), accessor.get_item_path(item).as_ref(), query, allow_non_contiguous_matches, platform));
    cache.insert(cache_hash, item_score.clone());

    item_score
}

/// `description ? [...] : undefined`: texto vazio é ausente.
fn non_empty(text: Option<&JsString>) -> Option<&JsString> {
    text.filter(|text| !text.is_empty())
}

fn separator(platform: &Platform) -> &'static str {
    if platform.is_windows { "\\" } else { "/" }
}

fn do_score_item_fuzzy(
    label: &JsString,
    description: Option<&JsString>,
    path: Option<&JsString>,
    query: &PreparedQuery,
    allow_non_contiguous_matches: bool,
    platform: &Platform,
) -> ItemScore {
    let path = non_empty(path);
    let prefer_label_matches = path.is_none() || !query.contains_path_separator;

    // Trata as correspondências de identidade com o caminho completo como as mais altas
    if let Some(path) = path
        && (if platform.is_linux {
            query.path_normalized == *path
        } else {
            equals_ignore_case(query.path_normalized.units(), path.units())
        })
    {
        return ItemScore {
            score: PATH_IDENTITY_SCORE,
            label_match: Some(vec![Match { start: 0, end: label.len() }]),
            description_match: non_empty(description).map(|description| vec![Match { start: 0, end: description.len() }]),
        };
    }

    // Pontuação: várias entradas
    if let Some(values) = query.values.as_ref().filter(|values| values.len() > 1) {
        return do_score_item_fuzzy_multiple(label, description, path, values, prefer_label_matches, allow_non_contiguous_matches, platform);
    }

    // Pontuação: uma entrada
    do_score_item_fuzzy_single(label, description, path, &query.piece, prefer_label_matches, allow_non_contiguous_matches, platform)
}

fn do_score_item_fuzzy_multiple(
    label: &JsString,
    description: Option<&JsString>,
    path: Option<&JsString>,
    query: &[PreparedQueryPiece],
    prefer_label_matches: bool,
    allow_non_contiguous_matches: bool,
    platform: &Platform,
) -> ItemScore {
    let mut total_score = 0;
    let mut total_label_matches: Vec<Match> = Vec::new();
    let mut total_description_matches: Vec<Match> = Vec::new();

    for query_piece in query {
        let ItemScore { score, label_match, description_match } =
            do_score_item_fuzzy_single(label, description, path, query_piece, prefer_label_matches, allow_non_contiguous_matches, platform);
        if score == NO_MATCH {
            // se uma única entrada da consulta não casa, volta sem pontuação nenhuma, porque exigimos que
            // todas casem
            return ItemScore::default();
        }

        total_score += score;
        if let Some(label_match) = label_match {
            total_label_matches.extend(label_match);
        }

        if let Some(description_match) = description_match {
            total_description_matches.extend(description_match);
        }
    }

    // se temos uma pontuação, garante que as posições estão em ordem crescente e distintas
    ItemScore {
        score: total_score,
        label_match: Some(normalize_matches(total_label_matches)),
        description_match: Some(normalize_matches(total_description_matches)),
    }
}

fn do_score_item_fuzzy_single(
    label: &JsString,
    description: Option<&JsString>,
    path: Option<&JsString>,
    query: &PreparedQueryPiece,
    prefer_label_matches: bool,
    allow_non_contiguous_matches: bool,
    platform: &Platform,
) -> ItemScore {
    let description = non_empty(description);

    // Prefere os casamentos no rótulo se mandaram ou se não há descrição
    if prefer_label_matches || description.is_none() {
        let (label_score, label_positions) = score_fuzzy(
            label.units(),
            query.normalized.units(),
            query.normalized_lowercase.units(),
            allow_non_contiguous_matches && !query.expect_contiguous_match,
        );
        if label_score != 0 {
            // Se há um casamento de prefixo no rótulo, damos uma pontuação base muito maior para elevar esses
            // casamentos acima dos outros. Isso garante que digitar o nome de um arquivo vença resultados que
            // estão em algum lugar do rótulo, mas não no começo.
            let label_prefix_match = matches_prefix(query.normalized.units(), label.units());
            let base_score = if label_prefix_match.is_some() {
                // Damos outro impulso aos rótulos curtos, por exemplo com os arquivos "window.ts" e
                // "windowActions.ts" e a consulta "window", queremos que "window.ts" receba uma pontuação maior.
                // Para isso calculamos a porcentagem que a consulta ocupa no rótulo e somamos à pontuação base.
                let prefix_length_boost = ((query.normalized.len() as f64 / label.len() as f64) * 100.0).round() as i32;
                LABEL_PREFIX_SCORE_THRESHOLD + prefix_length_boost
            } else {
                LABEL_SCORE_THRESHOLD
            };

            return ItemScore {
                score: base_score + label_score,
                label_match: Some(label_prefix_match.unwrap_or_else(|| create_matches(&label_positions))),
                description_match: None,
            };
        }
    }

    // Por fim calcula as pontuações de descrição mais rótulo, se temos uma descrição
    if let Some(description) = description {
        let mut description_prefix = description.clone();
        if path.is_some() {
            description_prefix.push_units(&separator(platform).encode_utf16().collect::<Vec<_>>()); // supõe que é um caminho de arquivo
        }

        let description_prefix_length = description_prefix.len();
        let mut description_and_label = description_prefix;
        description_and_label.push_units(label.units());

        let (label_description_score, label_description_positions) = score_fuzzy(
            description_and_label.units(),
            query.normalized.units(),
            query.normalized_lowercase.units(),
            allow_non_contiguous_matches && !query.expect_contiguous_match,
        );
        if label_description_score != 0 {
            let label_description_matches = create_matches(&label_description_positions);
            let mut label_match: Vec<Match> = Vec::new();
            let mut description_match: Vec<Match> = Vec::new();

            // Temos de dividir os casamentos de volta entre o rótulo e a descrição
            for h in label_description_matches {
                // O casamento cobre o rótulo e a descrição, precisamos dividi-lo
                if h.start < description_prefix_length && h.end > description_prefix_length {
                    label_match.push(Match { start: 0, end: h.end - description_prefix_length });
                    description_match.push(Match { start: h.start, end: description_prefix_length });
                }
                // Casamento na parte do rótulo
                else if h.start >= description_prefix_length {
                    label_match.push(Match { start: h.start - description_prefix_length, end: h.end - description_prefix_length });
                }
                // Casamento na parte da descrição
                else {
                    description_match.push(h);
                }
            }

            return ItemScore { score: label_description_score, label_match: Some(label_match), description_match: Some(description_match) };
        }
    }

    ItemScore::default()
}

fn create_matches(offsets: &[usize]) -> Vec<Match> {
    let mut ret: Vec<Match> = Vec::new();

    for &pos in offsets {
        match ret.last_mut() {
            Some(last) if last.end == pos => last.end += 1,
            _ => ret.push(Match { start: pos, end: pos + 1 }),
        }
    }

    ret
}

fn normalize_matches(mut matches: Vec<Match>) -> Vec<Match> {
    // ordena os casamentos pelo começo para poder normalizar
    matches.sort_by_key(|m| m.start);

    // junta os casamentos que se sobrepõem
    let mut normalized_matches: Vec<Match> = Vec::new();
    for m in matches {
        // se não há casamento atual ou os casamentos não se sobrepõem, ficamos com ele como está e o
        // lembramos para juntar depois
        match normalized_matches.last_mut() {
            Some(current_match) if match_overlaps(current_match, &m) => {
                // senão juntamos os casamentos
                current_match.start = current_match.start.min(m.start);
                current_match.end = current_match.end.max(m.end);
            }
            _ => normalized_matches.push(m),
        }
    }

    normalized_matches
}

fn match_overlaps(match_a: &Match, match_b: &Match) -> bool {
    if match_a.end < match_b.start {
        return false; // A termina antes de B começar
    }

    if match_b.end < match_a.start {
        return false; // B termina antes de A começar
    }

    true
}

//#endregion

//#region Comparers

/// `compareItemsByFuzzyScore`.
pub fn compare_items_by_fuzzy_score<T>(
    item_a: &T,
    item_b: &T,
    query: &PreparedQuery,
    allow_non_contiguous_matches: bool,
    accessor: &dyn ItemAccessor<T>,
    cache: &mut FuzzyScorerCache,
    platform: &Platform,
) -> i32 {
    let item_score_a = score_item_fuzzy(Some(item_a), query, allow_non_contiguous_matches, accessor, cache, platform);
    let item_score_b = score_item_fuzzy(Some(item_b), query, allow_non_contiguous_matches, accessor, cache, platform);

    let score_a = item_score_a.score;
    let score_b = item_score_b.score;

    // 1.) os casamentos de identidade têm a pontuação mais alta
    if (score_a == PATH_IDENTITY_SCORE || score_b == PATH_IDENTITY_SCORE) && score_a != score_b {
        return if score_a == PATH_IDENTITY_SCORE { -1 } else { 1 };
    }

    // 2.) casamentos no rótulo valem mais que casamentos em rótulo mais descrição
    if score_a > LABEL_SCORE_THRESHOLD || score_b > LABEL_SCORE_THRESHOLD {
        if score_a != score_b {
            return if score_a > score_b { -1 } else { 1 };
        }

        // prefere casamentos mais compactos a mais longos no rótulo (a não ser que seja um casamento de
        // prefixo, em que os prefixos mais longos é que são preferidos)
        if score_a < LABEL_PREFIX_SCORE_THRESHOLD && score_b < LABEL_PREFIX_SCORE_THRESHOLD {
            let compared_by_match_length = compare_by_match_length(item_score_a.label_match.as_deref(), item_score_b.label_match.as_deref());
            if compared_by_match_length != 0 {
                return compared_by_match_length;
            }
        }

        // prefere rótulos mais curtos a mais longos
        let label_a = accessor.get_item_label(item_a).unwrap_or_default();
        let label_b = accessor.get_item_label(item_b).unwrap_or_default();
        if label_a.len() != label_b.len() {
            return label_a.len() as i32 - label_b.len() as i32;
        }
    }

    // 3.) compara pela pontuação no rótulo mais descrição
    if score_a != score_b {
        return if score_a > score_b { -1 } else { 1 };
    }

    // 4.) pontuações idênticas: prefere casamentos no rótulo a casamentos fora dele
    let item_a_has_label_matches = item_score_a.label_match.as_ref().is_some_and(|m| !m.is_empty());
    let item_b_has_label_matches = item_score_b.label_match.as_ref().is_some_and(|m| !m.is_empty());
    if item_a_has_label_matches && !item_b_has_label_matches {
        return -1;
    } else if item_b_has_label_matches && !item_a_has_label_matches {
        return 1;
    }

    // 5.) pontuações idênticas: prefere casamentos mais compactos (rótulo e descrição)
    let item_a_match_distance = compute_label_and_description_match_distance(item_a, &item_score_a, accessor);
    let item_b_match_distance = compute_label_and_description_match_distance(item_b, &item_score_b, accessor);
    if item_a_match_distance != 0 && item_b_match_distance != 0 && item_a_match_distance != item_b_match_distance {
        return if item_b_match_distance > item_a_match_distance { -1 } else { 1 };
    }

    // 6.) pontuações idênticas: começa a usar a comparação de reserva
    fallback_compare(item_a, item_b, query, accessor)
}

fn compute_label_and_description_match_distance<T>(item: &T, score: &ItemScore, accessor: &dyn ItemAccessor<T>) -> i64 {
    let mut match_start = -1i64;
    let mut match_end = -1i64;

    let label_match = score.label_match.as_deref().filter(|m| !m.is_empty());
    let description_match = score.description_match.as_deref().filter(|m| !m.is_empty());

    // Se há casamentos na descrição, o começo é o primeiro casamento da descrição
    if let Some(description_match) = description_match {
        match_start = description_match[0].start as i64;
    }
    // Senão, o começo é o primeiro casamento do rótulo
    else if let Some(label_match) = label_match {
        match_start = label_match[0].start as i64;
    }

    // Se há casamento no rótulo, o fim é o último casamento do rótulo. Se havia casamento na descrição,
    // somamos o comprimento da descrição ao fim para indicar isso.
    if let Some(label_match) = label_match {
        match_end = label_match[label_match.len() - 1].end as i64;
        if description_match.is_some()
            && let Some(item_description) = accessor.get_item_description(item).filter(|d| !d.is_empty())
        {
            match_end += item_description.len() as i64;
        }
    }
    // Se há só casamento na descrição, o fim é o último casamento da descrição
    else if let Some(description_match) = description_match {
        match_end = description_match[description_match.len() - 1].end as i64;
    }

    match_end - match_start
}

fn compare_by_match_length(matches_a: Option<&[Match]>, matches_b: Option<&[Match]>) -> i32 {
    let matches_a = matches_a.filter(|m| !m.is_empty());
    let matches_b = matches_b.filter(|m| !m.is_empty());
    let (Some(matches_a), Some(matches_b)) = (matches_a, matches_b) else {
        // garante não comparar mal quando os casamentos não foram dados
        return match (matches_a, matches_b) {
            (None, None) => 0,
            (_, None) => -1,
            (None, _) => 1,
            _ => unreachable!(),
        };
    };

    // Calcula o comprimento do casamento de A (do primeiro ao último)
    let match_length_a = matches_a[matches_a.len() - 1].end - matches_a[0].start;

    // Calcula o comprimento do casamento de B (do primeiro ao último)
    let match_length_b = matches_b[matches_b.len() - 1].end - matches_b[0].start;

    // Prefere o casamento mais curto
    if match_length_a == match_length_b {
        0
    } else if match_length_b < match_length_a {
        1
    } else {
        -1
    }
}

fn fallback_compare<T>(item_a: &T, item_b: &T, query: &PreparedQuery, accessor: &dyn ItemAccessor<T>) -> i32 {
    // confere o comprimento de rótulo mais descrição e prefere o mais curto
    let label_a = accessor.get_item_label(item_a).unwrap_or_default();
    let label_b = accessor.get_item_label(item_b).unwrap_or_default();

    let description_a = accessor.get_item_description(item_a).filter(|d| !d.is_empty());
    let description_b = accessor.get_item_description(item_b).filter(|d| !d.is_empty());

    let label_description_a_length = label_a.len() + description_a.as_ref().map_or(0, JsString::len);
    let label_description_b_length = label_b.len() + description_b.as_ref().map_or(0, JsString::len);

    if label_description_a_length != label_description_b_length {
        return label_description_a_length as i32 - label_description_b_length as i32;
    }

    // confere o comprimento do caminho e prefere o mais curto
    let path_a = accessor.get_item_path(item_a).filter(|p| !p.is_empty());
    let path_b = accessor.get_item_path(item_b).filter(|p| !p.is_empty());

    if let (Some(path_a), Some(path_b)) = (&path_a, &path_b)
        && path_a.len() != path_b.len()
    {
        return path_a.len() as i32 - path_b.len() as i32;
    }

    // 7.) por fim temos pontuações e comprimentos iguais, e recorremos ao comparador

    // compara pelo rótulo
    if label_a != label_b {
        return compare_anything(label_a.units(), label_b.units(), query.normalized.units());
    }

    // compara pela descrição
    if let (Some(description_a), Some(description_b)) = (&description_a, &description_b)
        && description_a != description_b
    {
        return compare_anything(description_a.units(), description_b.units(), query.normalized.units());
    }

    // compara pelo caminho
    if let (Some(path_a), Some(path_b)) = (&path_a, &path_b)
        && path_a != path_b
    {
        return compare_anything(path_a.units(), path_b.units(), query.normalized.units());
    }

    // igual
    0
}

//#endregion

//#region Query Normalizer

/// `IPreparedQueryPiece`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedQueryPiece {
    /// A consulta original, como veio de entrada.
    pub original: JsString,
    pub original_lowercase: JsString,

    /// O original normalizado para os separadores da plataforma:
    /// - Windows: \
    /// - Posix: /
    pub path_normalized: JsString,

    /// Além do caminho normalizado, sem espaços em branco, curingas, aspas, reticências e `#` final.
    pub normalized: JsString,
    pub normalized_lowercase: JsString,

    /// A consulta está entre aspas, o que quer dizer que precisa ser um subtexto da entrada. Em outras
    /// palavras, não se usa casamento fuzzy.
    pub expect_contiguous_match: bool,
}

/// `IPreparedQuery`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedQuery {
    /// A consulta como peça (o `IPreparedQuery extends IPreparedQueryPiece` do original).
    pub piece: PreparedQueryPiece,

    /// A consulta dividida por espaços em peças.
    pub values: Option<Vec<PreparedQueryPiece>>,

    /// Se a consulta contém separador(es) de caminho ou não.
    pub contains_path_separator: bool,
}

impl Deref for PreparedQuery {
    type Target = PreparedQueryPiece;

    fn deref(&self) -> &PreparedQueryPiece {
        &self.piece
    }
}

/// Se uma consulta está entre aspas, o usuário não quer usar a busca fuzzy para ela.
fn query_expects_exact_match(query: &[u16]) -> bool {
    query.starts_with(&[cc::DoubleQuote]) && query.ends_with(&[cc::DoubleQuote])
}

const MULTIPLE_QUERY_VALUES_SEPARATOR: u16 = cc::Space;

/// `prepareQuery`: prepara um valor de busca para a pontuação, tirando os caracteres indesejados e
/// permitindo pontuar várias peças separadas por espaço.
pub fn prepare_query(original: &[u16], platform: &Platform) -> PreparedQuery {
    let original_lowercase = JsString::from(original).to_lower_case();
    let (path_normalized, normalized, normalized_lowercase) = normalize_query(original, platform);
    let separator: Vec<u16> = separator(platform).encode_utf16().collect();
    let contains_path_separator = js_string::index_of(path_normalized.units(), &separator, 0).is_some();
    let expect_exact_match = query_expects_exact_match(original);

    let mut values: Option<Vec<PreparedQueryPiece>> = None;

    let original_split: Vec<&[u16]> = original.split(|&unit| unit == MULTIPLE_QUERY_VALUES_SEPARATOR).collect();
    if original_split.len() > 1 {
        for original_piece in original_split {
            let expect_exact_match_piece = query_expects_exact_match(original_piece);
            let (path_normalized_piece, normalized_piece, normalized_lowercase_piece) = normalize_query(original_piece, platform);

            if !normalized_piece.is_empty() {
                values.get_or_insert_with(Vec::new).push(PreparedQueryPiece {
                    original: JsString::from(original_piece),
                    original_lowercase: JsString::from(original_piece).to_lower_case(),
                    path_normalized: path_normalized_piece,
                    normalized: normalized_piece,
                    normalized_lowercase: normalized_lowercase_piece,
                    expect_contiguous_match: expect_exact_match_piece,
                });
            }
        }
    }

    PreparedQuery {
        piece: PreparedQueryPiece {
            original: JsString::from(original),
            original_lowercase,
            path_normalized,
            normalized,
            normalized_lowercase,
            expect_contiguous_match: expect_exact_match,
        },
        values,
        contains_path_separator,
    }
}

/// `(pathNormalized, normalized, normalizedLowercase)`.
fn normalize_query(original: &[u16], platform: &Platform) -> (JsString, JsString, JsString) {
    let path_normalized: Vec<u16> = if platform.is_windows {
        // Ajuda os usuários do Windows a buscar caminhos usando a barra
        original.iter().map(|&unit| if unit == cc::Slash { cc::Backslash } else { unit }).collect()
    } else {
        // Ajuda os usuários de macOS/Linux a buscar caminhos usando a barra invertida
        original.iter().map(|&unit| if unit == cc::Backslash { cc::Slash } else { unit }).collect()
    };

    // tira certos caracteres que ajudam a achar resultados melhores:
    // - aspas: usadas na busca de casamento exato
    // - curingas: usados no casamento fuzzy
    // - espaços em branco: usados para separar consultas
    // - reticências: às vezes usadas para indicar quaisquer segmentos de caminho
    // - `#` final: usado por alguns servidores de linguagem (como o rust-analyzer) como modificador de consulta
    let mut normalized: Vec<u16> =
        path_normalized.iter().copied().filter(|&unit| !(unit == cc::Asterisk || unit == 0x2026 || unit == cc::DoubleQuote || is_js_whitespace(unit))).collect();
    // `/(?<=.)#$/`: um `#` no fim com algum caractere antes (os terminadores de linha já saíram com o `\s`)
    if normalized.len() >= 2 && normalized.last() == Some(&cc::Hash) {
        normalized.pop();
    }

    let normalized = JsString::from_units(normalized);
    let normalized_lowercase = normalized.to_lower_case();
    (JsString::from_units(path_normalized), normalized, normalized_lowercase)
}

/// `pieceToQuery` para uma peça.
pub fn piece_to_query(piece: &PreparedQueryPiece, platform: &Platform) -> PreparedQuery {
    prepare_query(piece.original.units(), platform)
}

/// `pieceToQuery` para várias peças.
pub fn pieces_to_query(pieces: &[PreparedQueryPiece], platform: &Platform) -> PreparedQuery {
    let mut joined = JsString::new();
    for (i, piece) in pieces.iter().enumerate() {
        if i > 0 {
            joined.push_unit(MULTIPLE_QUERY_VALUES_SEPARATOR);
        }
        joined.push_units(piece.original.units());
    }
    prepare_query(joined.units(), platform)
}

//#endregion
