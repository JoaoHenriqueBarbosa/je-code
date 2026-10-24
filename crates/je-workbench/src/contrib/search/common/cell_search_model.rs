//! Porte de `src/vs/workbench/contrib/search/common/cellSearchModel.ts`: o modelo de busca de uma célula de notebook
//! fechado (o texto da entrada e das saídas em buffers que a busca percorre linha a linha).
//!
//! Divergências:
//! - `Disposable` é o campo `base`. O `IReadonlyTextBuffer` é o [`PieceTreeTextBuffer`] compartilhado (`Rc`); o
//!   `bufferFactory.create(...)` do porte devolve só o buffer, sem o `disposable` extra que o original registra.
//! - `_source` é `source`, e `RawOutputFindMatch.textBuffer` é `text_buffer`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, IDisposable};
use je_editor::model::{DefaultEndOfLine, FindMatch};
use je_editor::piece_tree_text_buffer::PieceTreeTextBuffer;
use je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferBuilder;
use je_editor::range::Range;
use je_editor::text_model_search::SearchParams;

/// `RawOutputFindMatch`.
pub struct RawOutputFindMatch {
    pub text_buffer: Rc<PieceTreeTextBuffer>,
    pub matches: Vec<FindMatch>,
}

/// O buffer de texto (`DefaultEndOfLine.LF`) de um texto.
fn create_text_buffer(text: &str) -> Rc<PieceTreeTextBuffer> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut builder = PieceTreeTextBufferBuilder::new();
    builder.accept_chunk(&units);
    Rc::new(builder.finish(true).create(DefaultEndOfLine::Lf))
}

/// `CellSearchModel`.
pub struct CellSearchModel {
    base: Rc<Disposable>,
    pub source: String,
    input_text_buffer: RefCell<Option<Rc<PieceTreeTextBuffer>>>,
    outputs: Vec<String>,
    output_text_buffers: RefCell<Option<Vec<Rc<PieceTreeTextBuffer>>>>,
}

impl CellSearchModel {
    pub fn new(source: String, input_text_buffer: Option<Rc<PieceTreeTextBuffer>>, outputs: Vec<String>) -> Rc<Self> {
        Rc::new(Self {
            base: Disposable::new(),
            source,
            input_text_buffer: RefCell::new(input_text_buffer),
            outputs,
            output_text_buffers: RefCell::new(None),
        })
    }

    fn get_full_model_range(&self, buffer: &PieceTreeTextBuffer) -> Range {
        let line_count = buffer.get_line_count() as i32;
        Range::new(1, 1, line_count, self.get_line_max_column(buffer, line_count))
    }

    fn get_line_max_column(&self, buffer: &PieceTreeTextBuffer, line_number: i32) -> i32 {
        if line_number < 1 || line_number > buffer.get_line_count() as i32 {
            errors::throw(Error::new("Illegal value for lineNumber"));
        }
        buffer.get_line_length(line_number) as i32 + 1
    }

    pub fn input_text_buffer(&self) -> Rc<PieceTreeTextBuffer> {
        if self.input_text_buffer.borrow().is_none() {
            *self.input_text_buffer.borrow_mut() = Some(create_text_buffer(&self.source));
        }
        self.input_text_buffer.borrow().clone().expect("o buffer de entrada acabou de ser criado")
    }

    pub fn output_text_buffers(&self) -> Vec<Rc<PieceTreeTextBuffer>> {
        if self.output_text_buffers.borrow().is_none() {
            let buffers = self.outputs.iter().map(|output| create_text_buffer(output)).collect();
            *self.output_text_buffers.borrow_mut() = Some(buffers);
        }
        self.output_text_buffers.borrow().clone().expect("os buffers de saída acabaram de ser criados")
    }

    pub fn find_in_inputs(&self, target: &str) -> Vec<FindMatch> {
        let search_params = SearchParams::new(JsString::from(target), false, false, None);
        let Some(search_data) = search_params.parse_search_request() else {
            return Vec::new();
        };
        let input_text_buffer = self.input_text_buffer();
        let full_input_range = self.get_full_model_range(&input_text_buffer);
        input_text_buffer.find_matches_line_by_line(&full_input_range, &search_data, true, 5000)
    }

    pub fn find_in_outputs(&self, target: &str) -> Vec<RawOutputFindMatch> {
        let search_params = SearchParams::new(JsString::from(target), false, false, None);
        let Some(search_data) = search_params.parse_search_request() else {
            return Vec::new();
        };
        self.output_text_buffers()
            .into_iter()
            .filter_map(|buffer| {
                let matches = buffer.find_matches_line_by_line(&self.get_full_model_range(&buffer), &search_data, true, 5000);
                if matches.is_empty() {
                    return None;
                }
                Some(RawOutputFindMatch { text_buffer: buffer, matches })
            })
            .collect()
    }
}

impl IDisposable for CellSearchModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}
