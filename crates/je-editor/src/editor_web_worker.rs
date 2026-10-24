//! Porte parcial de `src/vs/editor/common/services/editorWebWorker.ts` e de `MirrorModel`
//! (`textModelSync/textModelSync.impl.ts`): o `EditorWorker` com o espelho dos modelos (`$acceptNewModel`,
//! `$acceptModelChanged`) e o `$computeWordRanges`, que o teste de `WordDistance` usa no lugar do web worker de
//! verdade. As outras operações do worker (diferenças, edições de espaço em branco, links, cores,
//! `navigateValueSet`, palavras para sugestão) entram junto das contribuições que as usam.
//!
//! Representação:
//! - O worker roda no mesmo thread: os métodos `$...` do original (chamadas de IPC que devolvem `Promise`) são métodos
//!   comuns que recebem os dados já como valores do porte; `IRawModelData` é o [`RawModelData`].
//! - `MirrorModel extends MirrorTextModel` é a composição [`MirrorModel`] sobre o [`MirrorTextModel`]. Dele só entram o
//!   `getLineWords` e o `_wordenize` que o `$computeWordRanges` usa.
//! - `$computeWordRanges(modelUrl, range, wordDef, wordDefFlags)` recebe a definição de palavra como o texto e as flags
//!   do `RegExp` (o IPC não leva o objeto) e devolve a promessa de [`WordRanges`]; `Object.create(null)` é um
//!   `HashMap`. `!isNaN(Number(word.word))` é [`is_numeric_word`].

use std::cell::RefCell;
use std::collections::HashMap;

use je_base::JsString;
use je_base::errors::Error;
use je_base::js_regexp::JsRegExp;
use je_base::js_string::{substring, trim};
use je_base::promise::Promise;
use je_base::uri::URI;

use crate::editor_worker::WordRanges;
use crate::mirror_text_model::MirrorTextModel;
use crate::model::Eol;
use crate::range::Range;
use crate::text_model_events::ModelContentChangedEvent;
use crate::word_helper::WordAtPosition;

/// `IRawModelData`: o modelo que o worker passa a espelhar.
pub struct RawModelData {
    pub url: String,
    pub lines: Vec<JsString>,
    pub eol: Eol,
    pub version_id: i64,
}

/// `MirrorModel`: o espelho de um modelo no worker.
pub struct MirrorModel {
    inner: MirrorTextModel,
}

impl MirrorModel {
    pub fn new(uri: URI, lines: Vec<JsString>, eol: Eol, version_id: i64) -> Self {
        Self { inner: MirrorTextModel::new(Some(uri), lines, eol, version_id) }
    }

    /// `onEvents(e)`.
    pub fn on_events(&mut self, e: &ModelContentChangedEvent) {
        self.inner.on_events(e);
    }

    /// `getLineWords(lineNumber, wordDefinition)`: as palavras da linha, com as colunas em que começam e terminam.
    pub fn get_line_words(&self, line_number: i32, word_definition: &JsRegExp) -> Vec<WordAtPosition> {
        let content = self.inner.lines()[(line_number - 1) as usize].units();
        Self::wordenize(content, word_definition)
            .into_iter()
            .map(|(start, end)| WordAtPosition {
                word: JsString::from(substring(content, start as i64, end as i64)),
                start_column: start as i32 + 1,
                end_column: end as i32 + 1,
            })
            .collect()
    }

    /// `_wordenize(content, wordDefinition)`: o início e o fim (em unidades UTF-16) de cada palavra.
    fn wordenize(content: &[u16], word_definition: &JsRegExp) -> Vec<(usize, usize)> {
        let mut result = Vec::new();

        word_definition.set_last_index(0); // reset lastIndex just to be sure

        while let Some(found) = word_definition.exec(content) {
            if found.end() == found.start() {
                // it did match the empty string
                break;
            }
            result.push((found.start(), found.end()));
        }
        result
    }
}

/// `!isNaN(Number(word))`: a palavra é um número do JavaScript (`Number('')` é 0, então o texto vazio também é).
pub fn is_numeric_word(word: &[u16]) -> bool {
    let text = String::from_utf16_lossy(trim(word));
    if text.is_empty() {
        return true;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return !digits.is_empty() && digits.chars().all(|digit| digit.is_digit(radix));
        }
    }
    if text.strip_prefix(['+', '-']).unwrap_or(&text) == "Infinity" {
        return true;
    }
    // Só dígitos, ponto, expoente e sinal: o `parse` do Rust também aceitaria `inf` e `nan`.
    text.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) && text.parse::<f64>().is_ok()
}

/// `EditorWorker`.
#[derive(Default)]
pub struct EditorWorker {
    models: RefCell<HashMap<String, MirrorModel>>,
}

impl EditorWorker {
    pub fn new() -> Self {
        Self::default()
    }

    /// `$acceptNewModel(data)`.
    pub fn accept_new_model(&self, data: RawModelData) {
        let uri = URI::parse(data.url.as_str(), false);
        self.models.borrow_mut().insert(data.url, MirrorModel::new(uri, data.lines, data.eol, data.version_id));
    }

    /// `$acceptModelChanged(uri, e)`: o modelo precisa já ter sido aceito.
    pub fn accept_model_changed(&self, uri: &str, e: &ModelContentChangedEvent) {
        self.models.borrow_mut().get_mut(uri).expect("o modelo foi aceito pelo worker").on_events(e);
    }

    /// `$computeWordRanges(modelUrl, range, wordDef, wordDefFlags)`: as palavras que não são números, de
    /// `range.start_line_number` até a linha anterior a `range.end_line_number`, e os intervalos de cada uma.
    pub fn compute_word_ranges(&self, model_url: &str, range: Range, word_def: &[u16], word_def_flags: &str) -> Promise<WordRanges> {
        let models = self.models.borrow();
        let Some(model) = models.get(model_url) else {
            return Promise::resolved(WordRanges::new());
        };
        let word_def_reg_exp = match JsRegExp::new(word_def, word_def_flags) {
            Ok(reg_exp) => reg_exp,
            Err(error) => return Promise::rejected(Error::named("SyntaxError", error.message())),
        };
        let mut result = WordRanges::new();
        for line in range.start_line_number..range.end_line_number {
            for word in model.get_line_words(line, &word_def_reg_exp) {
                if is_numeric_word(word.word.units()) {
                    continue;
                }
                result.entry(word.word).or_default().push(Range {
                    start_line_number: line,
                    start_column: word.start_column,
                    end_line_number: line,
                    end_column: word.end_column,
                });
            }
        }
        Promise::resolved(result)
    }
}
