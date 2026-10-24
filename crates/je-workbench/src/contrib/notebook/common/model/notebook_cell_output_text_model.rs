//! Porte de `src/vs/workbench/contrib/notebook/common/model/notebookCellOutputTextModel.ts`: o modelo de uma saída de
//! célula (`NotebookCellOutputTextModel`), com a versão dos dados, a fusão dos itens de fluxo de texto e o rastreio do
//! tamanho dos buffers por versão (para `appendedSinceVersion`).
//!
//! Divergências:
//! - `Disposable` é o campo `base`; o modelo nasce como `Rc<Self>`.
//! - `outputs` devolve uma cópia da lista (o `VSBuffer` é um handle compartilhado, a cópia é barata); o original
//!   devolve a própria lista, que `optimizeOutputItems` e `appendData` mutam, o que aqui é feito no `IOutputDto` interno.
//! - `metadata` é `Option<Dynamic>` (`Record<string, unknown> | undefined`).
//! - `versionedBufferLengths` é `HashMap<mime, HashMap<versionId, tamanho>>`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::mime::is_text_stream_mime;

use crate::contrib::notebook::common::notebook_common::{ICellOutput, IOutputDto, IOutputItemDto, compress_output_item_streams};

pub struct NotebookCellOutputTextModel {
    base: Rc<Disposable>,
    on_did_change_data: Rc<Emitter<()>>,
    raw_output: RefCell<IOutputDto>,
    /// Alternative output id that's reused when the output is updated.
    alternative_output_id: String,
    version_id: Cell<i64>,
    /// mime: versionId: buffer length
    versioned_buffer_lengths: RefCell<HashMap<String, HashMap<i64, usize>>>,
}

impl NotebookCellOutputTextModel {
    pub fn new(raw_output: IOutputDto) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_data = Emitter::<()>::new();
        base.register(on_did_change_data.clone());
        let alternative_output_id = raw_output.output_id.clone();
        Rc::new(Self {
            base,
            on_did_change_data,
            raw_output: RefCell::new(raw_output),
            alternative_output_id,
            version_id: Cell::new(0),
            versioned_buffer_lengths: RefCell::new(HashMap::new()),
        })
    }

    fn track_buffer_lengths(&self) {
        let version_id = self.version_id.get();
        for output in self.outputs() {
            if is_text_stream_mime(&output.mime) {
                self.versioned_buffer_lengths
                    .borrow_mut()
                    .entry(output.mime.clone())
                    .or_default()
                    .insert(version_id, output.data.byte_length());
            }
        }
    }

    fn optimize_output_items(&self) {
        let outputs = self.outputs();
        if outputs.len() > 1 && outputs.iter().all(|item| is_text_stream_mime(&item.mime)) {
            // Look for the mimes in the items, and keep track of their order. Merge the streams into one output item,
            // per mime type.
            let mut mime_outputs: IndexMap<String, Vec<Vec<u8>>> = IndexMap::new();
            for item in &outputs {
                mime_outputs.entry(item.mime.clone()).or_default().push(item.data.to_vec());
            }
            let mut merged: Vec<IOutputItemDto> = Vec::new();
            for (mime, items) in mime_outputs {
                let slices: Vec<&[u8]> = items.iter().map(Vec::as_slice).collect();
                let compression_result = compress_output_item_streams(&slices);
                merged.push(IOutputItemDto { mime, data: compression_result.data });
                if compression_result.did_compression {
                    // we can't rely on knowing buffer lengths if we've erased previous lines
                    self.versioned_buffer_lengths.borrow_mut().clear();
                }
            }
            self.raw_output.borrow_mut().outputs = merged;
        }
    }
}

impl IDisposable for NotebookCellOutputTextModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ICellOutput for NotebookCellOutputTextModel {
    fn version_id(&self) -> i64 {
        self.version_id.get()
    }

    fn outputs(&self) -> Vec<IOutputItemDto> {
        self.raw_output.borrow().outputs.clone()
    }

    fn metadata(&self) -> Option<Dynamic> {
        self.raw_output.borrow().metadata.clone()
    }

    fn output_id(&self) -> String {
        self.raw_output.borrow().output_id.clone()
    }

    fn alternative_output_id(&self) -> String {
        self.alternative_output_id.clone()
    }

    fn on_did_change_data(&self) -> Event<()> {
        self.on_did_change_data.event()
    }

    fn replace_data(&self, raw_data: IOutputDto) {
        self.versioned_buffer_lengths.borrow_mut().clear();
        *self.raw_output.borrow_mut() = raw_data;
        self.optimize_output_items();
        self.version_id.set(self.version_id.get() + 1);
        self.on_did_change_data.fire(());
    }

    fn append_data(&self, items: Vec<IOutputItemDto>) {
        self.track_buffer_lengths();
        self.raw_output.borrow_mut().outputs.extend(items);
        self.optimize_output_items();
        self.version_id.set(self.version_id.get() + 1);
        self.on_did_change_data.fire(());
    }

    fn appended_since_version(&self, version_id: i64, mime: &str) -> Option<VSBuffer> {
        let buffer_length = self.versioned_buffer_lengths.borrow().get(mime).and_then(|lengths| lengths.get(&version_id)).copied();
        let output = self.outputs().into_iter().find(|output| output.mime == mime);
        match (buffer_length, output) {
            // `bufferLength` zero é falso no original.
            (Some(buffer_length), Some(output)) if buffer_length != 0 => Some(output.data.slice(Some(buffer_length as i64), None)),
            _ => None,
        }
    }

    fn as_dto(&self) -> IOutputDto {
        let raw_output = self.raw_output.borrow();
        IOutputDto { metadata: raw_output.metadata.clone(), outputs: raw_output.outputs.clone(), output_id: raw_output.output_id.clone() }
    }

    fn bump_version(&self) {
        self.version_id.set(self.version_id.get() + 1);
    }
}
