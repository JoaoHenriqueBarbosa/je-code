//! Porte de `src/vs/base/test/common/buffer.test.ts` (suíte `Buffer`), caso a caso e na mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque o `buffer` não cria `IDisposable`. Os
//! testes `async` rodam em `common::run_with_faked_timers`. O `Uint8Array` do original é o `Vec<u8>` e
//! `VSBuffer#buffer` é `to_vec()`.
//!
//! Pendente (só em parte): `performance_issue_with_vsbuffer_slice_76076` verifica o `Buffer#slice` do
//! Node, que não existe aqui; ficam as duas partes de `TypedArray`, com `VSBuffer::copy` no lugar de
//! `slice` (cópia) e `VSBuffer::slice` no lugar de `subarray` (janela). Em `set`, o caso de entrada
//! inválida (`buff.set({})`) não compila em Rust, então não há o que afirmar.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{run_with_faked_timers, timeout};
use je_base::buffer::{
    VSBuffer, buffer_to_readable, buffer_to_stream, buffered_stream_to_buffer, decode_base64, decode_hex, encode_base64,
    encode_hex, new_writeable_buffer_stream, readable_to_buffer, stream_to_buffer,
};
use je_base::errors::{self, Error};
use je_base::stream::{self, ReadableStream, ReadableStreamEvents, StreamListener, WriteableStream};

#[test]
fn issue_71993_vsbuffer_to_string_returns_numbers() {
    let data = vec![1, 2, 3, b'h', b'i', 4, 5];
    let buffer = VSBuffer::wrap(data).slice(Some(3), Some(5));
    assert_eq!(buffer.to_string(), "hi");
}

#[test]
fn issue_251527_vsbuffer_to_string_preserves_bom_character_in_filenames() {
    // BOM character (U+FEFF) is a zero-width character that was being stripped
    // when deserializing messages in the IPC layer. This test verifies that
    // the BOM character is preserved when using VSBuffer.toString().
    let bom_char = '\u{FEFF}';
    let filename = format!("{bom_char}c.txt");
    let buffer = VSBuffer::from_string(&filename);
    let result = buffer.to_string();

    // Verify the BOM character is preserved
    assert_eq!(result, filename);
    assert_eq!(result.encode_utf16().next(), Some(0xFEFF));
}

#[test]
fn buffer_to_readable_readable_to_buffer() {
    let content = "Hello World";
    let mut readable = buffer_to_readable(VSBuffer::from_string(content));

    assert_eq!(readable_to_buffer(&mut readable).to_string(), content);
}

#[test]
fn buffer_to_stream_stream_to_buffer() {
    run_with_faked_timers(async {
        let content = "Hello World";
        let stream = buffer_to_stream(VSBuffer::from_string(content));

        assert_eq!(stream_to_buffer(&*stream).await.unwrap().to_string(), content);
    });
}

#[test]
fn buffered_stream_to_buffer_case() {
    run_with_faked_timers(async {
        let content = "Hello World";
        let stream = stream::peek_stream(buffer_to_stream(VSBuffer::from_string(content)), 1).await.unwrap();

        assert_eq!(buffered_stream_to_buffer(stream).await.unwrap().to_string(), content);
    });
}

/// Os três ouvintes que os testes do `bufferWriteableStream` registram: dados, fim e erros.
#[derive(Clone, Default)]
struct Probe {
    chunks: Rc<RefCell<Vec<VSBuffer>>>,
    ended: Rc<Cell<bool>>,
    errors: Rc<RefCell<Vec<Error>>>,
}

impl Probe {
    fn on_data(&self, stream: &dyn ReadableStreamEvents<VSBuffer>) {
        let chunks = self.chunks.clone();
        stream.on(StreamListener::data(move |data: &VSBuffer| chunks.borrow_mut().push(data.clone())));
    }

    fn on_end(&self, stream: &dyn ReadableStreamEvents<VSBuffer>) {
        let ended = self.ended.clone();
        stream.on(StreamListener::end(move || ended.set(true)));
    }

    fn on_error(&self, stream: &dyn ReadableStreamEvents<VSBuffer>) {
        let errors = self.errors.clone();
        stream.on(StreamListener::error(move |error: &Error| errors.borrow_mut().push(error.clone())));
    }

    fn chunk(&self, index: usize) -> String {
        self.chunks.borrow()[index].to_string()
    }
}

fn text(content: &str) -> VSBuffer {
    VSBuffer::from_string(content)
}

#[test]
fn buffer_writeable_stream_basics_no_error() {
    run_with_faked_timers(async {
        let stream = new_writeable_buffer_stream(None);
        let probe = Probe::default();
        probe.on_data(&*stream);
        probe.on_end(&*stream);
        probe.on_error(&*stream);

        timeout(0).await.unwrap();
        stream.write(text("Hello"));
        timeout(0).await.unwrap();
        stream.end(Some(text("World")));

        assert_eq!(probe.chunks.borrow().len(), 2);
        assert_eq!(probe.chunk(0), "Hello");
        assert_eq!(probe.chunk(1), "World");
        assert!(probe.ended.get());
        assert_eq!(probe.errors.borrow().len(), 0);
    });
}

#[test]
fn buffer_writeable_stream_basics_error() {
    run_with_faked_timers(async {
        let stream = new_writeable_buffer_stream(None);
        let probe = Probe::default();
        probe.on_data(&*stream);
        probe.on_end(&*stream);
        probe.on_error(&*stream);

        timeout(0).await.unwrap();
        stream.write(text("Hello"));
        timeout(0).await.unwrap();
        stream.error(Error::new(""));
        stream.end(None);

        assert_eq!(probe.chunks.borrow().len(), 1);
        assert_eq!(probe.chunk(0), "Hello");
        assert!(probe.ended.get());
        assert_eq!(probe.errors.borrow().len(), 1);
    });
}

#[test]
fn buffer_writeable_stream_buffers_data_when_no_listener() {
    run_with_faked_timers(async {
        let stream = new_writeable_buffer_stream(None);

        timeout(0).await.unwrap();
        stream.write(text("Hello"));
        timeout(0).await.unwrap();
        stream.end(Some(text("World")));

        let probe = Probe::default();
        probe.on_data(&*stream);
        probe.on_end(&*stream);
        probe.on_error(&*stream);

        assert_eq!(probe.chunks.borrow().len(), 1);
        assert_eq!(probe.chunk(0), "HelloWorld");
        assert!(probe.ended.get());
        assert_eq!(probe.errors.borrow().len(), 0);
    });
}

#[test]
fn buffer_writeable_stream_buffers_errors_when_no_listener() {
    run_with_faked_timers(async {
        let stream = new_writeable_buffer_stream(None);

        timeout(0).await.unwrap();
        stream.write(text("Hello"));
        timeout(0).await.unwrap();
        stream.error(Error::new(""));

        let probe = Probe::default();
        probe.on_data(&*stream);
        probe.on_error(&*stream);
        probe.on_end(&*stream);

        stream.end(None);

        assert_eq!(probe.chunks.borrow().len(), 1);
        assert_eq!(probe.chunk(0), "Hello");
        assert!(probe.ended.get());
        assert_eq!(probe.errors.borrow().len(), 1);
    });
}

#[test]
fn buffer_writeable_stream_buffers_end_when_no_listener() {
    run_with_faked_timers(async {
        let stream = new_writeable_buffer_stream(None);

        timeout(0).await.unwrap();
        stream.write(text("Hello"));
        timeout(0).await.unwrap();
        stream.end(Some(text("World")));

        let probe = Probe::default();
        probe.on_end(&*stream);
        probe.on_data(&*stream);
        probe.on_error(&*stream);

        assert_eq!(probe.chunks.borrow().len(), 1);
        assert_eq!(probe.chunk(0), "HelloWorld");
        assert!(probe.ended.get());
        assert_eq!(probe.errors.borrow().len(), 0);
    });
}

#[test]
fn buffer_writeable_stream_nothing_happens_after_end() {
    run_with_faked_timers(async {
        let stream = new_writeable_buffer_stream(None);

        let probe = Probe::default();
        probe.on_data(&*stream);

        timeout(0).await.unwrap();
        stream.write(text("Hello"));
        timeout(0).await.unwrap();
        stream.end(Some(text("World")));

        let after = Probe::default();
        after.on_data(&*stream);
        after.on_error(&*stream);
        after.on_end(&*stream);

        timeout(0).await.unwrap();
        stream.write(text("Hello"));
        timeout(0).await.unwrap();
        stream.error(Error::new(""));
        timeout(0).await.unwrap();
        stream.end(Some(text("World")));

        assert_eq!(after.chunks.borrow().len(), 0);
        assert_eq!(after.errors.borrow().len(), 0);
        assert!(!after.ended.get());

        assert_eq!(probe.chunks.borrow().len(), 2);
        assert_eq!(probe.chunk(0), "Hello");
        assert_eq!(probe.chunk(1), "World");
    });
}

#[test]
fn buffer_writeable_stream_pause_resume_simple() {
    run_with_faked_timers(async {
        let stream = new_writeable_buffer_stream(None);
        let probe = Probe::default();
        probe.on_data(&*stream);
        probe.on_end(&*stream);
        probe.on_error(&*stream);

        stream.pause();

        timeout(0).await.unwrap();
        stream.write(text("Hello"));
        timeout(0).await.unwrap();
        stream.end(Some(text("World")));

        assert_eq!(probe.chunks.borrow().len(), 0);
        assert_eq!(probe.errors.borrow().len(), 0);
        assert!(!probe.ended.get());

        stream.resume();

        assert_eq!(probe.chunks.borrow().len(), 1);
        assert_eq!(probe.chunk(0), "HelloWorld");
        assert!(probe.ended.get());
        assert_eq!(probe.errors.borrow().len(), 0);
    });
}

#[test]
fn buffer_writeable_stream_pause_resume_pause_after_first_write() {
    run_with_faked_timers(async {
        let stream = new_writeable_buffer_stream(None);
        let probe = Probe::default();
        probe.on_data(&*stream);
        probe.on_end(&*stream);
        probe.on_error(&*stream);

        timeout(0).await.unwrap();
        stream.write(text("Hello"));

        stream.pause();

        timeout(0).await.unwrap();
        stream.end(Some(text("World")));

        assert_eq!(probe.chunks.borrow().len(), 1);
        assert_eq!(probe.chunk(0), "Hello");
        assert_eq!(probe.errors.borrow().len(), 0);
        assert!(!probe.ended.get());

        stream.resume();

        assert_eq!(probe.chunks.borrow().len(), 2);
        assert_eq!(probe.chunk(0), "Hello");
        assert_eq!(probe.chunk(1), "World");
        assert!(probe.ended.get());
        assert_eq!(probe.errors.borrow().len(), 0);
    });
}

#[test]
fn buffer_writeable_stream_pause_resume_error() {
    run_with_faked_timers(async {
        let stream = new_writeable_buffer_stream(None);
        let probe = Probe::default();
        probe.on_data(&*stream);
        probe.on_end(&*stream);
        probe.on_error(&*stream);

        stream.pause();

        timeout(0).await.unwrap();
        stream.write(text("Hello"));
        timeout(0).await.unwrap();
        stream.error(Error::new(""));
        stream.end(None);

        assert_eq!(probe.chunks.borrow().len(), 0);
        assert!(!probe.ended.get());
        assert_eq!(probe.errors.borrow().len(), 0);

        stream.resume();

        assert_eq!(probe.chunks.borrow().len(), 1);
        assert_eq!(probe.chunk(0), "Hello");
        assert!(probe.ended.get());
        assert_eq!(probe.errors.borrow().len(), 1);
    });
}

#[test]
fn buffer_writeable_stream_destroy() {
    run_with_faked_timers(async {
        let stream = new_writeable_buffer_stream(None);
        let probe = Probe::default();
        probe.on_data(&*stream);
        probe.on_end(&*stream);
        probe.on_error(&*stream);

        stream.destroy();

        timeout(0).await.unwrap();
        stream.write(text("Hello"));
        timeout(0).await.unwrap();
        stream.end(Some(text("World")));

        assert_eq!(probe.chunks.borrow().len(), 0);
        assert!(!probe.ended.get());
        assert_eq!(probe.errors.borrow().len(), 0);
    });
}

#[test]
fn performance_issue_with_vsbuffer_slice_76076() {
    // TypedArray#slice creates a copy
    {
        let unit = VSBuffer::wrap(vec![10, 20, 30, 40]);
        let u2 = unit.copy().slice(Some(1), Some(3));
        assert_eq!(unit.read_u_int8(1), 20);
        assert_eq!(u2.read_u_int8(0), 20);

        unit.write_u_int8(17, 1); // modify unit, NOT b2
        assert_eq!(unit.read_u_int8(1), 17);
        assert_eq!(u2.read_u_int8(0), 20);
    }

    // TypedArray#subarray creates a view
    {
        let unit = VSBuffer::wrap(vec![10, 20, 30, 40]);
        let u2 = unit.slice(Some(1), Some(3));
        assert_eq!(unit.read_u_int8(1), 20);
        assert_eq!(u2.read_u_int8(0), 20);

        unit.write_u_int8(17, 1); // modify unit AND b2
        assert_eq!(unit.read_u_int8(1), 17);
        assert_eq!(u2.read_u_int8(0), 17);
    }
}

#[test]
fn index_of() {
    let haystack = VSBuffer::from_string("abcaabbccaaabbbccc");
    assert_eq!(haystack.index_of(&VSBuffer::from_string(""), 0), 0);
    assert_eq!(haystack.index_of(&VSBuffer::from_string(&"a".repeat(100)), 0), -1);

    assert_eq!(haystack.index_of(&VSBuffer::from_string("a"), 0), 0);
    assert_eq!(haystack.index_of(&VSBuffer::from_string("c"), 0), 2);
    assert_eq!(haystack.index_of(&VSBuffer::from_string("c"), 4), 7);

    assert_eq!(haystack.index_of(&VSBuffer::from_string("abcaa"), 0), 0);
    assert_eq!(haystack.index_of(&VSBuffer::from_string("caaab"), 0), 8);
    assert_eq!(haystack.index_of(&VSBuffer::from_string("ccc"), 0), 15);
    assert_eq!(haystack.index_of(&VSBuffer::from_string("cc"), 9), 15);

    assert_eq!(haystack.index_of(&VSBuffer::from_string("cccb"), 0), -1);
}

#[test]
fn wrap() {
    let actual = vec![1, 2, 3];
    let wrapped = VSBuffer::wrap(actual);
    assert_eq!(wrapped.byte_length(), 3);
    assert_eq!(wrapped.to_vec(), vec![1, 2, 3]);
}

#[test]
fn from_string() {
    let value = "Hello World";
    let buff = VSBuffer::from_string(value);
    assert_eq!(buff.to_string(), value);
}

#[test]
fn from_byte_array() {
    let array = [1, 2, 3, 4, 5];
    let buff = VSBuffer::from_byte_array(&array);
    assert_eq!(buff.byte_length(), array.len());
    assert_eq!(buff.to_vec(), array.to_vec());
}

#[test]
fn concat() {
    let chunks = [VSBuffer::from_string("abc"), VSBuffer::from_string("def"), VSBuffer::from_string("ghi")];

    // Test without total length
    let result1 = VSBuffer::concat(&chunks, None);
    assert_eq!(result1.to_string(), "abcdefghi");

    // Test with total length
    let result2 = VSBuffer::concat(&chunks, Some(9));
    assert_eq!(result2.to_string(), "abcdefghi");
}

#[test]
fn clone() {
    let original = VSBuffer::from_string("test");
    let clone = original.copy();

    assert!(!original.shares_storage_with(&clone));
    assert_eq!(original.to_vec(), clone.to_vec());
}

#[test]
fn slice() {
    let buff = VSBuffer::from_string("Hello World");

    let slice1 = buff.slice(Some(0), Some(5));
    assert_eq!(slice1.to_string(), "Hello");

    let slice2 = buff.slice(Some(6), None);
    assert_eq!(slice2.to_string(), "World");
}

#[test]
fn set() {
    let buff = VSBuffer::alloc(5);

    // Test setting from VSBuffer
    buff.set(&VSBuffer::from_string("ab"), 0);
    assert_eq!(&buff.to_string()[0..2], "ab");

    // Test setting from Uint8Array
    buff.set_bytes(&[99, 100], 2); // 'cd'
    assert_eq!(&buff.to_string()[2..4], "cd");

    // Fora dos bytes lança (o `RangeError` do `TypedArray#set`).
    assert!(errors::catch(|| buff.set_bytes(&[1, 2], 4)).is_err());
}

#[test]
fn equals() {
    let buff1 = VSBuffer::from_string("test");
    let buff2 = VSBuffer::from_string("test");
    let buff3 = VSBuffer::from_string("different");
    let buff4 = VSBuffer::from_string("tes1");

    assert!(buff1.equals(&buff1));
    assert!(buff1.equals(&buff2));
    assert!(!buff1.equals(&buff3));
    assert!(!buff1.equals(&buff4));
}

#[test]
fn read_write_methods() {
    let buff = VSBuffer::alloc(8);

    // Test UInt32BE
    buff.write_u_int32_be(0x12345678, 0);
    assert_eq!(buff.read_u_int32_be(0), 0x12345678);

    // Test UInt32LE
    buff.write_u_int32_le(0x12345678, 4);
    assert_eq!(buff.read_u_int32_le(4), 0x12345678);

    // Test UInt8
    let buff2 = VSBuffer::alloc(1);
    buff2.write_u_int8(123, 0);
    assert_eq!(buff2.read_u_int8(0), 123);
}

mod encoding {
    use super::*;

    /*
    Generated with:

    const crypto = require('crypto');

    for (let i = 0; i < 16; i++) {
        const buf =  crypto.randomBytes(i);
        console.log(`[new Uint8Array([${Array.from(buf).join(', ')}]), '${buf.toString('base64')}'],`)
    }

    */

    #[allow(clippy::type_complexity)]
    fn test_cases() -> Vec<(Vec<u8>, &'static str, &'static str)> {
        vec![
            (vec![], "", ""),
            (vec![77], "TQ==", "4d"),
            (vec![230, 138], "5oo=", "e68a"),
            (vec![104, 98, 82], "aGJS", "686252"),
            (vec![92, 114, 57, 209], "XHI50Q==", "5c7239d1"),
            (vec![238, 51, 1, 240, 124], "7jMB8Hw=", "ee3301f07c"),
            (vec![96, 54, 130, 79, 47, 179], "YDaCTy+z", "6036824f2fb3"),
            (vec![91, 22, 68, 217, 68, 117, 116], "WxZE2UR1dA==", "5b1644d9447574"),
            (vec![184, 227, 214, 171, 244, 175, 141, 53], "uOPWq/SvjTU=", "b8e3d6abf4af8d35"),
            (vec![53, 98, 93, 130, 71, 117, 191, 137, 156], "NWJdgkd1v4mc", "35625d824775bf899c"),
            (vec![154, 156, 60, 102, 232, 197, 92, 25, 124, 98], "mpw8ZujFXBl8Yg==", "9a9c3c66e8c55c197c62"),
            (vec![152, 131, 106, 234, 17, 183, 164, 245, 252, 67, 26], "mINq6hG3pPX8Qxo=", "98836aea11b7a4f5fc431a"),
            (vec![232, 254, 194, 234, 16, 42, 86, 135, 117, 61, 179, 4], "6P7C6hAqVod1PbME", "e8fec2ea102a5687753db304"),
            (vec![4, 199, 85, 172, 125, 171, 172, 219, 61, 47, 78, 155, 127], "BMdVrH2rrNs9L06bfw==", "04c755ac7dabacdb3d2f4e9b7f"),
            (vec![189, 67, 62, 189, 87, 171, 27, 164, 87, 142, 126, 113, 23, 182], "vUM+vVerG6RXjn5xF7Y=", "bd433ebd57ab1ba4578e7e7117b6"),
            (
                vec![153, 156, 145, 240, 228, 200, 199, 158, 40, 167, 97, 52, 217, 148, 43],
                "mZyR8OTIx54op2E02ZQr",
                "999c91f0e4c8c79e28a76134d9942b",
            ),
        ]
    }

    #[test]
    fn encodes_base64() {
        for (bytes, expected, _) in test_cases() {
            assert_eq!(encode_base64(&VSBuffer::wrap(bytes), true, false), expected);
        }
    }

    #[test]
    fn decodes_base64() {
        for (expected, encoded, _) in test_cases() {
            assert_eq!(decode_base64(encoded).to_vec(), expected);
        }
    }

    #[test]
    fn encodes_hex() {
        for (bytes, _, expected) in test_cases() {
            assert_eq!(encode_hex(&VSBuffer::wrap(bytes)), expected);
        }
    }

    #[test]
    fn decodes_hex() {
        for (expected, _, encoded) in test_cases() {
            assert_eq!(decode_hex(encoded).to_vec(), expected);
        }
    }

    #[test]
    fn throws_error_on_invalid_encoding() {
        assert!(errors::catch(|| decode_base64("invalid!")).is_err());
        assert!(errors::catch(|| decode_hex("invalid!")).is_err());
    }
}
