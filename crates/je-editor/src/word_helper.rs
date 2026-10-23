//! Porte de `src/vs/editor/common/core/wordHelper.ts`.
//!
//! O `RegExp` do original é um objeto compartilhado com `lastIndex` mutável, e as funções daqui mexem
//! nele (`getWordAtText` deixa `lastIndex` em 0 no fim): o porte usa `Rc<JsRegExp>`, cujo `lastIndex`
//! fica num `Cell`. O `DEFAULT_WORD_REGEXP` e a lista de configurações padrão são globais do módulo no
//! original; aqui são `thread_local!`, porque o `JsRegExp` não é `Sync` e o modelo vive num thread só.
//!
//! Divergência: com `max_len` ímpar, o original calcula `column - maxLen / 2` em ponto flutuante e a
//! coluna do resultado sai fracionária; aqui a metade é a divisão inteira. Com o padrão (1000), dá no
//! mesmo.

use std::rc::Rc;
use std::time::{Duration, Instant};

use je_base::JsString;
use je_base::errors::{Error, throw};
use je_base::js_string::substring;
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::linked_list::LinkedList;
use zyarr::Match;

use crate::js_reg_exp::JsRegExp;

pub const USUAL_WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

/// `IWordAtPosition`: uma palavra dentro do modelo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordAtPosition {
    /// A palavra.
    pub word: JsString,
    /// A coluna onde a palavra começa.
    pub start_column: i32,
    /// A coluna onde a palavra termina.
    pub end_column: i32,
}

/// `createWordRegExp`: a definição de palavra a partir dos separadores usuais, deixando entrar na
/// palavra os separadores de `allow_in_words`. A padrão fica assim:
/// ```txt
/// /(-?\d*\.\d\w*)|([^\`\~\!\@\#\$\%\^\&\*\(\)\-\=\+\[\{\]\}\\\|\;\:\'\"\,\.\<\>\/\?\s]+)/g
/// ```
pub fn create_word_reg_exp(allow_in_words: &[u16]) -> JsRegExp {
    let mut source = String::from("(-?\\d*\\.\\d\\w*)|([^");
    for sep in USUAL_WORD_SEPARATORS.chars() {
        if allow_in_words.contains(&(sep as u16)) {
            continue;
        }
        source.push('\\');
        source.push(sep);
    }
    source.push_str("\\s]+)");
    JsRegExp::from_str(&source, "g").expect("o padrão gerado pelos separadores usuais é válido")
}

thread_local! {
    /// Pega os números (inclusive os de ponto flutuante) no primeiro grupo e o alfanumérico no segundo.
    static DEFAULT_WORD_REGEXP: Rc<JsRegExp> = Rc::new(create_word_reg_exp(&[]));

    static DEFAULT_CONFIG: LinkedList<GetWordAtTextConfig> = {
        let list = LinkedList::new();
        // O `Removal` descartado deixa o elemento na lista, como o retorno ignorado do original.
        list.unshift(GetWordAtTextConfig { max_len: 1000, window_size: 15, time_budget: 150 });
        list
    };
}

/// `DEFAULT_WORD_REGEXP`: o mesmo objeto a cada chamada (no thread).
pub fn default_word_regexp() -> Rc<JsRegExp> {
    DEFAULT_WORD_REGEXP.with(Rc::clone)
}

/// `ensureValidWordDefinition`: a definição dada com a flag `g` (recriada sem ela, se preciso, com as
/// flags `i`, `m` e `u` que tinha), ou a padrão. Zera o `lastIndex` do resultado.
pub fn ensure_valid_word_definition(word_definition: Option<&Rc<JsRegExp>>) -> Rc<JsRegExp> {
    let result = match word_definition {
        None => default_word_regexp(),
        Some(word_definition) if word_definition.flags().global => Rc::clone(word_definition),
        Some(word_definition) => {
            let original = word_definition.flags();
            let mut flags = String::from("g");
            for (on, letter) in [
                (original.ignore_case, 'i'),
                (original.multiline, 'm'),
                (original.unicode, 'u'),
            ] {
                if on {
                    flags.push(letter);
                }
            }
            // Sem `v` o padrão pode deixar de compilar; o `new RegExp` do original lança aí também.
            Rc::new(
                JsRegExp::new(word_definition.source(), &flags)
                    .unwrap_or_else(|error| throw(Error::named("SyntaxError", error.message()))),
            )
        }
    };

    result.set_last_index(0);

    result
}

/// `IGetWordAtTextConfig`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GetWordAtTextConfig {
    pub max_len: i32,
    pub window_size: i32,
    /// Em milissegundos.
    pub time_budget: u64,
}

/// `setDefaultGetWordAtTextConfig`: passa a valer `value` até o descarte do retorno.
pub fn set_default_get_word_at_text_config(value: GetWordAtTextConfig) -> DisposableRef {
    let rm = DEFAULT_CONFIG.with(|list| list.unshift(value));
    to_disposable(move || rm.remove())
}

/// `getWordAtText`: a palavra de `word_definition` que contém `column` em `text`, que começa em
/// `text_offset` na linha.
pub fn get_word_at_text(
    column: i32,
    word_definition: &Rc<JsRegExp>,
    text: &[u16],
    text_offset: i32,
    config: Option<&GetWordAtTextConfig>,
) -> Option<WordAtPosition> {
    // Garante a flag `g`; sem ela o laço não termina.
    let word_definition = ensure_valid_word_definition(Some(word_definition));

    let config = match config {
        Some(config) => *config,
        None => DEFAULT_CONFIG
            .with(|list| list.iter().next())
            .expect("a lista de configurações padrão nunca fica vazia"),
    };

    if text.len() > config.max_len as usize {
        // Não joga textos tão longos no regexp: usa um trecho em que a palavra tem de estar.
        let mut start = column - config.max_len / 2;
        let mut text_offset = text_offset;
        if start < 0 {
            start = 0;
        } else {
            text_offset += start;
        }
        let text = substring(
            text,
            i64::from(start),
            i64::from(column + config.max_len / 2),
        );
        return get_word_at_text(column, &word_definition, text, text_offset, Some(&config));
    }

    let t1 = Instant::now();
    let time_budget = Duration::from_millis(config.time_budget);
    let pos = column - 1 - text_offset;

    let mut prev_regex_index = -1;
    let mut found: Option<Match> = None;

    for i in 1.. {
        // Confere o orçamento de tempo.
        if t1.elapsed() >= time_budget {
            break;
        }

        // Volta o ponto onde o regexp começa a casar, e sabe onde parar para não repetir as buscas
        // anteriores.
        let regex_index = pos - config.window_size * i;
        word_definition.set_last_index(regex_index.max(0) as usize);
        let this_match =
            find_regex_match_enclosing_position(&word_definition, text, pos, prev_regex_index);

        if this_match.is_none() && found.is_some() {
            // Para: já há resultado.
            break;
        }

        found = this_match;

        // Para: a busca já partiu do começo.
        if regex_index <= 0 {
            break;
        }
        prev_regex_index = regex_index;
    }

    let found = found?;
    let start_column = text_offset + 1 + found.start() as i32;
    let result = WordAtPosition {
        word: JsString::from(&text[found.start()..found.end()]),
        start_column,
        end_column: start_column + (found.end() - found.start()) as i32,
    };
    word_definition.set_last_index(0);
    Some(result)
}

fn find_regex_match_enclosing_position(
    word_definition: &JsRegExp,
    text: &[u16],
    pos: i32,
    stop_pos: i32,
) -> Option<Match> {
    while let Some(found) = word_definition.exec(text) {
        let match_index = found.start() as i32;
        if match_index <= pos && word_definition.last_index() as i32 >= pos {
            return Some(found);
        } else if stop_pos > 0 && match_index > stop_pos {
            return None;
        }
    }
    None
}
