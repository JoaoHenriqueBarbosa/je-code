//! Porte de `src/vs/base/node/nodeStreams.ts`: o `StreamSplitter`.
//!
//! O original é um `Transform` do Node.js. O porte não tem o módulo `stream` do Node, e o equivalente da
//! forma dele é o `WriteableStreamImpl` de `stream.rs` (o stream do VS Code que emula a API do Node):
//!
//! - o lado de leitura do `Transform` é um [`WriteableStreamImpl`] de `Vec<u8>`, e cada `this.push(...)`
//!   do original é um `write` nele. Os chunks ficam em buffer até alguém ligar um ouvinte de dados
//!   (o `pipe`), como um `Readable` do Node em modo pausado;
//! - o lado de escrita do `Transform` é [`StreamSplitter::write`] (que roda o `_transform`) e
//!   [`StreamSplitter::end`] (que roda o `_flush`, fecha o stream e chama o callback, como o
//!   `end(callback)` do Node chama o callback no `finish`);
//! - `pipe(writable)` recebe o destino como um fecho que consome um chunk (o `write(chunk, enc, callback)`
//!   de um `Writable` do Node, sem o encoding nem o callback, que aqui são síncronos).
//!
//! Divergências de forma:
//!
//! - o original guarda o separador como `number` (um byte) ou `Buffer`, e usa `Buffer#indexOf` para o
//!   primeiro caso e `binaryIndexOf` para o segundo. `binary_index_of` já trata o separador de um byte
//!   como `indexOf`, então o porte guarda sempre um `Vec<u8>` e usa uma busca só;
//! - `this.buffer.slice(...)` do Node é uma vista, e aqui é uma cópia;
//! - um separador vazio faz o original girar para sempre (cada volta empurra um chunk vazio e não avança);
//!   aqui o construtor recusa com `assert!`;
//! - o `Buffer.concat` do original é o `extend_from_slice`; nenhum `RefCell` fica emprestado enquanto um
//!   ouvinte roda, então um ouvinte pode chamar `write` no próprio splitter.

use std::cell::RefCell;
use std::rc::Rc;

use crate::buffer::binary_index_of;
use crate::stream::{ReadableStreamEvents, StreamListener, WriteableStream, WriteableStreamImpl, new_writeable_stream};

/// O argumento `splitter: string | number | Buffer` do construtor, já em bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSplitterSeparator(Vec<u8>);

impl From<&str> for StreamSplitterSeparator {
    fn from(splitter: &str) -> Self {
        Self(splitter.as_bytes().to_vec())
    }
}

impl From<u8> for StreamSplitterSeparator {
    fn from(splitter: u8) -> Self {
        Self(vec![splitter])
    }
}

impl From<&[u8]> for StreamSplitterSeparator {
    fn from(splitter: &[u8]) -> Self {
        Self(splitter.to_vec())
    }
}

impl From<Vec<u8>> for StreamSplitterSeparator {
    fn from(splitter: Vec<u8>) -> Self {
        Self(splitter)
    }
}

/// A Transform stream that splits the input on the "splitter" substring.
/// The resulting chunks will contain (and trail with) the splitter match.
/// The last chunk when the stream ends will be emitted even if a splitter
/// is not encountered.
pub struct StreamSplitter {
    buffer: RefCell<Option<Vec<u8>>>,
    splitter: Vec<u8>,
    output: Rc<WriteableStreamImpl<Vec<u8>>>,
}

impl StreamSplitter {
    pub fn new(splitter: impl Into<StreamSplitterSeparator>) -> Self {
        let StreamSplitterSeparator(splitter) = splitter.into();
        assert!(!splitter.is_empty(), "StreamSplitter: o separador não pode ser vazio");
        Self { buffer: RefCell::new(None), splitter, output: new_writeable_stream(None, None) }
    }

    /// O lado de leitura do `Transform`: os chunks já separados.
    pub fn output(&self) -> Rc<WriteableStreamImpl<Vec<u8>>> {
        self.output.clone()
    }

    /// `pipe(writable)`: cada chunk separado vai para `writable`, em ordem.
    pub fn pipe(&self, writable: impl Fn(&[u8]) + 'static) {
        self.output.on(StreamListener::data(move |chunk: &Vec<u8>| writable(chunk)));
    }

    /// `write(chunk)`: roda o `_transform` do original.
    pub fn write(&self, chunk: impl AsRef<[u8]>) {
        let chunk = chunk.as_ref();
        let buffer = match self.buffer.borrow_mut().take() {
            None => chunk.to_vec(),
            Some(mut buffer) => {
                buffer.extend_from_slice(chunk);
                buffer
            }
        };

        let mut offset = 0;
        while offset < buffer.len() {
            let Ok(index) = usize::try_from(binary_index_of(&buffer, &self.splitter, offset)) else {
                break;
            };

            let end = index + self.splitter.len();
            self.output.write(buffer[offset..end].to_vec());
            offset = end;
        }

        *self.buffer.borrow_mut() = (offset != buffer.len()).then(|| buffer[offset..].to_vec());
    }

    /// `end(callback)`: roda o `_flush` do original (o resto sem separador sai como último chunk), fecha o
    /// lado de leitura e chama o `callback`.
    pub fn end(&self, callback: impl FnOnce()) {
        let rest = self.buffer.borrow_mut().take();
        if let Some(rest) = rest {
            self.output.write(rest);
        }
        self.output.end(None);

        callback();
    }
}
