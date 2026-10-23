//! Porte de `RenderedLinesCollection` e da interface `ILine` de
//! `src/vs/editor/browser/view/viewLayer.ts`: a coleção das linhas visíveis do editor, que acompanha
//! inserções, remoções e mudanças de linhas na janela de rolagem.
//!
//! Fora do porte, de propósito: `IVisibleLine`, `VisibleLinesCollection`, `ViewLayerRenderer` e o
//! `FastDomNode`, que são a pintura em DOM do arquivo original. O porte desenha nativo (widgets em Rust,
//! wgpu e rasterizador em software), então não há nó de DOM para manter nem HTML para montar; só a
//! contabilidade das linhas, que é independente de DOM, entra aqui.

use je_base::errors::{throw, Error};

/// `ILine`: uma linha que a coleção avisa quando o conteúdo ou os tokens mudam.
pub trait Line {
    fn on_content_changed(&mut self);
    fn on_tokens_changed(&mut self);
}

/// `ILineFactory<T>`. Qualquer `Fn() -> T` já é uma fábrica.
pub trait LineFactory<T: Line> {
    fn create_line(&self) -> T;
}

impl<T: Line, F: Fn() -> T> LineFactory<T> for F {
    fn create_line(&self) -> T {
        self()
    }
}

/// O `{ fromLineNumber, toLineNumber }` que `onTokensChanged` recebe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineNumberRange {
    pub from_line_number: i32,
    pub to_line_number: i32,
}

pub struct RenderedLinesCollection<T: Line> {
    lines: Vec<T>,
    rend_line_number_start: i32,
    line_factory: Box<dyn LineFactory<T>>,
}

impl<T: Line> RenderedLinesCollection<T> {
    pub fn new(line_factory: impl LineFactory<T> + 'static) -> Self {
        Self { lines: Vec::new(), rend_line_number_start: 1, line_factory: Box::new(line_factory) }
    }

    pub fn flush(&mut self) {
        self.set(1, Vec::new());
    }

    /// `_set`.
    pub fn set(&mut self, rend_line_number_start: i32, lines: Vec<T>) {
        self.lines = lines;
        self.rend_line_number_start = rend_line_number_start;
    }

    /// `_get`.
    pub fn get(&self) -> (i32, &[T]) {
        (self.rend_line_number_start, &self.lines)
    }

    /// Número de linha inclusivo que está dentro da coleção.
    pub fn get_start_line_number(&self) -> i32 {
        self.rend_line_number_start
    }

    /// Número de linha inclusivo que está dentro da coleção.
    pub fn get_end_line_number(&self) -> i32 {
        self.rend_line_number_start + self.lines.len() as i32 - 1
    }

    pub fn get_count(&self) -> usize {
        self.lines.len()
    }

    /// Lança `BugIndicatingError('Illegal value for lineNumber')` fora da coleção.
    pub fn get_line(&self, line_number: i32) -> &T {
        let line_index = self.line_index(line_number);
        &self.lines[line_index]
    }

    fn line_index(&self, line_number: i32) -> usize {
        let line_index = line_number - self.rend_line_number_start;
        if line_index < 0 || line_index as usize >= self.lines.len() {
            throw(Error::bug_indicating(Some("Illegal value for lineNumber")));
        }
        line_index as usize
    }

    /// Devolve as linhas que saíram da coleção.
    pub fn on_lines_deleted(&mut self, delete_from_line_number: i32, delete_to_line_number: i32) -> Option<Vec<T>> {
        if self.get_count() == 0 {
            // no lines
            return None;
        }

        let start_line_number = self.get_start_line_number();
        let end_line_number = self.get_end_line_number();

        if delete_to_line_number < start_line_number {
            // deleting above the viewport
            let delete_cnt = delete_to_line_number - delete_from_line_number + 1;
            self.rend_line_number_start -= delete_cnt;
            return None;
        }

        if delete_from_line_number > end_line_number {
            // deleted below the viewport
            return None;
        }

        // Record what needs to be deleted
        let mut delete_start_index = 0usize;
        let mut delete_count = 0usize;
        for line_number in start_line_number..=end_line_number {
            let line_index = (line_number - self.rend_line_number_start) as usize;

            if delete_from_line_number <= line_number && line_number <= delete_to_line_number {
                // this is a line to be deleted
                if delete_count == 0 {
                    // this is the first line to be deleted
                    delete_start_index = line_index;
                    delete_count = 1;
                } else {
                    delete_count += 1;
                }
            }
        }

        // Adjust this._rendLineNumberStart for lines deleted above
        if delete_from_line_number < start_line_number {
            // Something was deleted above
            let delete_above_count = if delete_to_line_number < start_line_number {
                // the entire deleted lines are above
                delete_to_line_number - delete_from_line_number + 1
            } else {
                start_line_number - delete_from_line_number
            };

            self.rend_line_number_start -= delete_above_count;
        }

        Some(self.lines.drain(delete_start_index..delete_start_index + delete_count).collect())
    }

    pub fn on_lines_changed(&mut self, change_from_line_number: i32, change_count: i32) -> bool {
        let change_to_line_number = change_from_line_number + change_count - 1;
        if self.get_count() == 0 {
            // no lines
            return false;
        }

        let start_line_number = self.get_start_line_number();
        let end_line_number = self.get_end_line_number();

        let mut someone_notified = false;

        for changed_line_number in change_from_line_number..=change_to_line_number {
            if changed_line_number >= start_line_number && changed_line_number <= end_line_number {
                // Notify the line
                let index = (changed_line_number - self.rend_line_number_start) as usize;
                self.lines[index].on_content_changed();
                someone_notified = true;
            }
        }

        someone_notified
    }

    pub fn on_lines_inserted(&mut self, insert_from_line_number: i32, insert_to_line_number: i32) -> Option<Vec<T>> {
        if self.get_count() == 0 {
            // no lines
            return None;
        }

        let insert_cnt = insert_to_line_number - insert_from_line_number + 1;
        let start_line_number = self.get_start_line_number();
        let end_line_number = self.get_end_line_number();

        if insert_from_line_number <= start_line_number {
            // inserting above the viewport
            self.rend_line_number_start += insert_cnt;
            return None;
        }

        if insert_from_line_number > end_line_number {
            // inserting below the viewport
            return None;
        }

        if insert_cnt + insert_from_line_number > end_line_number {
            // insert inside the viewport in such a way that all remaining lines are pushed outside
            let from = (insert_from_line_number - self.rend_line_number_start) as usize;
            return Some(self.lines.split_off(from));
        }

        // insert inside the viewport, push out some lines, but not all remaining lines
        let insert_cnt = insert_cnt as usize;
        let insert_index = (insert_from_line_number - self.rend_line_number_start) as usize;
        let mut lines = std::mem::take(&mut self.lines);
        let deleted_lines = lines.split_off(lines.len() - insert_cnt);
        let after_lines = lines.split_off(insert_index);
        lines.extend((0..insert_cnt).map(|_| self.line_factory.create_line()));
        lines.extend(after_lines);
        self.lines = lines;

        Some(deleted_lines)
    }

    pub fn on_tokens_changed(&mut self, ranges: &[LineNumberRange]) -> bool {
        if self.get_count() == 0 {
            // no lines
            return false;
        }

        let start_line_number = self.get_start_line_number();
        let end_line_number = self.get_end_line_number();

        let mut notified_someone = false;
        for rng in ranges {
            if rng.to_line_number < start_line_number || rng.from_line_number > end_line_number {
                // range outside viewport
                continue;
            }

            let from = start_line_number.max(rng.from_line_number);
            let to = end_line_number.min(rng.to_line_number);

            for line_number in from..=to {
                let line_index = (line_number - self.rend_line_number_start) as usize;
                self.lines[line_index].on_tokens_changed();
                notified_someone = true;
            }
        }

        notified_someone
    }
}
