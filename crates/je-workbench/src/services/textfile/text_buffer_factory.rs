//! `createTextBufferFactoryFromStream` e `createTextBufferFactoryFromSnapshot` de `src/vs/editor/common/model/textModel.ts`
//! sobre o [`TextBufferFactoryRef`] (o `ITextBufferFactory` compartilhado e consumido uma vez que o porte do serviço de
//! arquivos de texto usa).
//!
//! Divergência: o `PieceTreeTextBufferFactory` não é `Clone` (criar o buffer o consome) e uma [`Promise`] guarda valores
//! clonáveis, então a promessa entrega o [`TextBufferFactoryRef`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::stream::{IStreamListener, ReadableStream, ReadableStreamEvents, listen_stream};
use je_base::promise::Promise;
use je_editor::model::TextSnapshot;
use je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferBuilder;
use je_editor::text_model::create_text_buffer_factory_from_snapshot;

use crate::services::textfile::textfiles::TextBufferFactoryRef;

/// `createTextBufferFactoryFromStream(stream)`: junta os trechos de texto do stream num `ITextBufferFactory`.
pub fn create_text_buffer_factory_from_stream(stream: &dyn ReadableStream<String>) -> Promise<TextBufferFactoryRef> {
    let (promise, resolver) = Promise::new();
    let builder = Rc::new(RefCell::new(Some(PieceTreeTextBufferBuilder::new())));
    let done = Rc::new(Cell::new(false));

    let events: &dyn ReadableStreamEvents<String> = stream;
    let (data_builder, error_done, error_resolver, end_builder, end_done, end_resolver) =
        (builder.clone(), done.clone(), resolver.clone(), builder, done, resolver);
    listen_stream(
        events,
        IStreamListener {
            on_data: Rc::new(move |chunk: &String| {
                if let Some(builder) = data_builder.borrow_mut().as_mut() {
                    let units: Vec<u16> = chunk.encode_utf16().collect();
                    builder.accept_chunk(&units);
                }
            }),
            on_error: Rc::new(move |error| {
                if !error_done.get() {
                    error_done.set(true);
                    error_resolver.reject(error.clone());
                }
            }),
            on_end: Rc::new(move || {
                if !end_done.get() {
                    end_done.set(true);
                    if let Some(builder) = end_builder.borrow_mut().take() {
                        end_resolver.resolve(TextBufferFactoryRef::new(builder.finish(true)));
                    }
                }
            }),
        },
        None,
    );

    promise
}

/// `createTextBufferFactoryFromSnapshot(snapshot)`.
pub fn text_buffer_factory_from_snapshot(snapshot: &mut dyn TextSnapshot) -> TextBufferFactoryRef {
    TextBufferFactoryRef::new(create_text_buffer_factory_from_snapshot(snapshot))
}
