//! Porte de `src/vs/editor/common/model/mirrorTextModel.ts`.
//!
//! O espelho do texto de um modelo que vive do outro lado de uma fronteira (o worker de extensões, o
//! web worker do editor): começa com as linhas e acompanha o modelo aplicando os eventos de mudança.
//!
//! Os campos `protected` do original, que as subclasses (`MirrorModel` do worker e afins) leem, viram os
//! acessores [`MirrorTextModel::lines`], [`MirrorTextModel::eol`], [`MirrorTextModel::line_starts`] e
//! [`MirrorTextModel::ensure_line_starts`]; quem estende passa a compor. O evento é o
//! [`ModelContentChangedEvent`] do modelo, que traz tudo o que o `IModelChangedEvent` do original traz.
//! O fim de linha é sempre `\n` ou `\r\n` ([`Eol`]), então o `e.eol &&` do original (que ignora o fim de
//! linha vazio) não tem equivalente.

use std::cell::RefCell;

use je_base::JsString;
use je_base::js_string::substring;
use je_base::strings::split_lines;
use je_base::uri::URI;

use crate::model::Eol;
use crate::position::Position;
use crate::prefix_sum_computer::PrefixSumComputer;
use crate::range::Range;
use crate::text_model_events::ModelContentChangedEvent;

/// `MirrorTextModel`.
#[derive(Debug)]
pub struct MirrorTextModel {
    uri: Option<URI>,
    lines: Vec<JsString>,
    eol: Eol,
    version_id: i64,
    line_starts: Option<PrefixSumComputer>,
    cached_text_value: RefCell<Option<JsString>>,
}

/// `str.substring(start)` com o índice do JavaScript (negativo vira 0, além do fim dá vazio).
fn substring_from(str: &[u16], start: i32) -> &[u16] {
    substring(str, i64::from(start), str.len() as i64)
}

fn concat(parts: &[&[u16]]) -> JsString {
    JsString::from_units(parts.concat())
}

impl MirrorTextModel {
    pub fn new(uri: Option<URI>, lines: Vec<JsString>, eol: Eol, version_id: i64) -> Self {
        Self {
            uri,
            lines,
            eol,
            version_id,
            line_starts: None,
            cached_text_value: RefCell::new(None),
        }
    }

    pub fn dispose(&mut self) {
        self.lines.clear();
    }

    /// `version` (o único membro de `IMirrorTextModel`).
    pub fn version(&self) -> i64 {
        self.version_id
    }

    pub fn uri(&self) -> Option<&URI> {
        self.uri.as_ref()
    }

    pub fn lines(&self) -> &[JsString] {
        &self.lines
    }

    pub fn eol(&self) -> Eol {
        self.eol
    }

    /// A soma de prefixo dos comprimentos de linha (com o fim de linha), se já foi calculada.
    pub fn line_starts(&self) -> Option<&PrefixSumComputer> {
        self.line_starts.as_ref()
    }

    pub fn get_text(&self) -> JsString {
        self.cached_text_value
            .borrow_mut()
            .get_or_insert_with(|| {
                let lines: Vec<&[u16]> = self.lines.iter().map(JsString::units).collect();
                JsString::from_units(lines.join(self.eol.as_units()))
            })
            .clone()
    }

    pub fn on_events(&mut self, e: &ModelContentChangedEvent) {
        if e.eol != self.eol {
            self.eol = e.eol;
            self.line_starts = None;
        }

        // Atualiza as linhas.
        for change in &e.changes {
            self.accept_delete_range(&change.range);
            self.accept_insert_text(
                Position::new(change.range.start_line_number, change.range.start_column),
                &change.text,
            );
        }

        self.version_id = e.version_id;
        *self.cached_text_value.get_mut() = None;
    }

    /// `_ensureLineStarts`: calcula a soma de prefixo dos comprimentos de linha, se ainda não existe.
    pub fn ensure_line_starts(&mut self) -> &PrefixSumComputer {
        let eol_length = self.eol.len();
        let lines = &self.lines;
        self.line_starts.get_or_insert_with(|| {
            PrefixSumComputer::new(
                lines
                    .iter()
                    .map(|line| (line.len() + eol_length) as u32)
                    .collect(),
            )
        })
    }

    /// Toda mudança no texto de uma linha passa por aqui.
    fn set_line_text(&mut self, line_index: usize, new_value: JsString) {
        let length = (new_value.len() + self.eol.len()) as u32;
        self.lines[line_index] = new_value;
        if let Some(line_starts) = &mut self.line_starts {
            // Atualiza a soma de prefixo.
            line_starts.set_value(line_index, length);
        }
    }

    fn accept_delete_range(&mut self, range: &Range) {
        let start_index = (range.start_line_number - 1) as usize;
        let start_line = self.lines[start_index].units();
        let before = substring(start_line, 0, i64::from(range.start_column - 1));

        if range.start_line_number == range.end_line_number {
            if range.start_column == range.end_column {
                // Nada a apagar.
                return;
            }
            // Apaga o texto na linha.
            let new_value = concat(&[before, substring_from(start_line, range.end_column - 1)]);
            self.set_line_text(start_index, new_value);
            return;
        }

        // Junta ao que sobra da primeira linha o que sobra da última.
        let end_line = self.lines[(range.end_line_number - 1) as usize].units();
        let new_value = concat(&[before, substring_from(end_line, range.end_column - 1)]);
        self.set_line_text(start_index, new_value);

        // Apaga as linhas do meio.
        let from = (range.start_line_number as usize).min(self.lines.len());
        let count = (range.end_line_number - range.start_line_number) as usize;
        self.lines.drain(from..(from + count).min(self.lines.len()));
        if let Some(line_starts) = &mut self.line_starts {
            // Atualiza a soma de prefixo.
            line_starts.remove_values(range.start_line_number as usize, count);
        }
    }

    fn accept_insert_text(&mut self, position: Position, insert_text: &JsString) {
        if insert_text.is_empty() {
            // Nada a inserir.
            return;
        }
        let line_index = (position.line_number - 1) as usize;
        let line = self.lines[line_index].units();
        let before = substring(line, 0, i64::from(position.column - 1));
        let after = substring_from(line, position.column - 1);
        let insert_lines = split_lines(insert_text.units());
        if let [single] = insert_lines[..] {
            // Insere texto numa linha só.
            let new_value = concat(&[before, single, after]);
            self.set_line_text(line_index, new_value);
            return;
        }

        // Leva para o fim do texto inserido o que sobrava da primeira linha.
        let last = insert_lines.len() - 1;
        let new_lines: Vec<JsString> = insert_lines[1..]
            .iter()
            .enumerate()
            .map(|(i, &text)| {
                if i + 1 == last {
                    concat(&[text, after])
                } else {
                    JsString::from(text)
                }
            })
            .collect();

        // Troca o que sobrava da primeira linha pela primeira linha do texto inserido.
        let new_value = concat(&[before, insert_lines[0]]);
        self.set_line_text(line_index, new_value);

        // Insere as linhas novas e guarda os comprimentos.
        let eol_length = self.eol.len();
        let new_lengths: Vec<u32> = new_lines
            .iter()
            .map(|line| (line.len() + eol_length) as u32)
            .collect();
        let at = (position.line_number as usize).min(self.lines.len());
        self.lines.splice(at..at, new_lines);

        if let Some(line_starts) = &mut self.line_starts {
            // Atualiza a soma de prefixo.
            line_starts.insert_values(position.line_number as usize, &new_lengths);
        }
    }
}
