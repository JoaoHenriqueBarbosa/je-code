//! Porte de `src/vs/base/parts/ipc/test/common/webSocketFraming.test.ts` (suíte `WebSocket framing`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução: `assert.throws(fn)` é o `Err` do `Result`, e `assert.throws(fn, /texto/)` confere a
//! mensagem do erro. O `for (const length of [...]) test(...)` do original gera um teste por tamanho, e aqui é
//! um teste (`encodes_and_parses_payload_length`) que percorre os mesmos quatro tamanhos.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::VSBuffer;
use je_base::parts::ipc::web_socket_framing::{
    IWebSocketFrameOptions, IWebSocketFrameParserOptions, WebSocketFrameParser, WebSocketOpcode, encode_web_socket_frame,
};

fn options(opcode: WebSocketOpcode, mask: Option<u32>) -> IWebSocketFrameOptions {
    IWebSocketFrameOptions { mask, ..IWebSocketFrameOptions::new(opcode) }
}

fn parser() -> WebSocketFrameParser {
    WebSocketFrameParser::new(IWebSocketFrameParserOptions::default()).unwrap()
}

fn text(buffer: &VSBuffer) -> String {
    String::from_utf8(buffer.to_vec()).unwrap()
}

#[test]
fn encodes_and_parses_a_masked_client_frame_without_mutating_its_payload() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let payload = VSBuffer::from_string("Hello");
    let encoded = encode_web_socket_frame(&payload, options(WebSocketOpcode::Text, Some(0x0102_0304))).unwrap();
    let frame = parser().accept_chunk(encoded.clone()).unwrap().remove(0);

    assert_eq!(encoded.to_vec(), vec![0x81, 0x85, 0x01, 0x02, 0x03, 0x04, 0x49, 0x67, 0x6f, 0x68, 0x6e]);
    assert_eq!(text(&payload), "Hello");
    assert_eq!((frame.r#final, frame.compressed, frame.opcode, text(&frame.payload), frame.mask), (true, false, WebSocketOpcode::Text, "Hello".to_owned(), Some(0x0102_0304)));
}

#[test]
fn masks_a_three_byte_payload_using_every_remainder_byte() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let payload = VSBuffer::from_byte_array(&[0xaa, 0xbb, 0xcc]);
    let encoded = encode_web_socket_frame(&payload, options(WebSocketOpcode::Binary, Some(0x1234_5678))).unwrap();
    let frame = parser().accept_chunk(encoded.clone()).unwrap().remove(0);

    assert_eq!(encoded.to_vec(), vec![0x82, 0x83, 0x12, 0x34, 0x56, 0x78, 0xb8, 0x8f, 0x9a]);
    assert_eq!(frame.payload.to_vec(), vec![0xaa, 0xbb, 0xcc]);
}

#[test]
fn can_unmask_owned_payload_buffers_in_place() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let encoded = encode_web_socket_frame(&VSBuffer::from_string("owned"), options(WebSocketOpcode::Text, Some(0x1234_5678))).unwrap();
    let mut in_place = WebSocketFrameParser::new(IWebSocketFrameParserOptions { unmask_in_place: Some(true), ..Default::default() }).unwrap();
    let frame = in_place.accept_chunk(encoded.clone()).unwrap().remove(0);

    assert_eq!(text(&frame.payload), "owned");
    assert_eq!(text(&encoded.slice(Some(6), None)), "owned");
}

#[test]
fn preserves_a_present_zero_valued_mask() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let encoded = encode_web_socket_frame(&VSBuffer::from_string("zero"), options(WebSocketOpcode::Text, Some(0))).unwrap();
    let frame = parser().accept_chunk(encoded.clone()).unwrap().remove(0);

    assert_eq!(encoded.read_u_int8(1) & 0b1000_0000, 0b1000_0000);
    assert_eq!(encoded.slice(Some(2), Some(6)).to_vec(), vec![0, 0, 0, 0]);
    assert_eq!(text(&frame.payload), "zero");
    assert_eq!(frame.mask, Some(0));
}

#[test]
fn accepts_frames_across_chunk_boundaries_and_in_coalesced_chunks() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let first = encode_web_socket_frame(&VSBuffer::from_string("first"), options(WebSocketOpcode::Text, None)).unwrap();
    let second = encode_web_socket_frame(&VSBuffer::from_string("second"), options(WebSocketOpcode::Text, None)).unwrap();
    let mut parser = parser();

    let first_part = parser.accept_chunk(first.slice(Some(0), Some(3))).unwrap();
    let remaining = parser.accept_chunk(VSBuffer::concat(&[first.slice(Some(3), None), second], None)).unwrap();

    assert_eq!(first_part.len(), 0);
    assert_eq!(remaining.iter().map(|frame| text(&frame.payload)).collect::<Vec<_>>(), vec!["first", "second"]);
}

#[test]
fn encodes_and_parses_payload_length() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    for length in [125usize, 126, 65_535, 65_536] {
        let payload = VSBuffer::alloc(length);
        for index in 0..payload.byte_length() {
            payload.write_u_int8(index as u8, index);
        }

        let encoded = encode_web_socket_frame(&payload, options(WebSocketOpcode::Binary, None)).unwrap();
        let frame = parser().accept_chunk(encoded.clone()).unwrap().remove(0);

        let expected_header: Vec<u8> = if length < 126 {
            vec![0x82, length as u8]
        } else if length < 1 << 16 {
            vec![0x82, 126, ((length >> 8) & 0xff) as u8, (length & 0xff) as u8]
        } else {
            vec![0x82, 127, 0, 0, 0, 0, ((length >> 24) & 0xff) as u8, ((length >> 16) & 0xff) as u8, ((length >> 8) & 0xff) as u8, (length & 0xff) as u8]
        };
        assert_eq!(encoded.slice(Some(0), Some((encoded.byte_length() - payload.byte_length()) as i64)).to_vec(), expected_header, "length {length}");
        assert_eq!(frame.payload.byte_length(), length);
        assert_eq!(frame.payload.read_u_int8(0), 0);
        assert_eq!(frame.payload.read_u_int8(frame.payload.byte_length() - 1), ((length - 1) & 0xff) as u8);
    }
}

#[test]
fn rejects_invalid_control_frames_reserved_opcodes_and_unsupported_lengths() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(encode_web_socket_frame(&VSBuffer::alloc(0), IWebSocketFrameOptions { r#final: Some(false), ..IWebSocketFrameOptions::new(WebSocketOpcode::Ping) }).is_err());
    assert!(parser().accept_chunk(VSBuffer::from_byte_array(&[0x83, 0x00])).is_err());
    let mut oversized_control = vec![0x89, 0x7e, 0x00, 0x7e];
    oversized_control.extend(std::iter::repeat_n(0, 126));
    assert!(parser().accept_chunk(VSBuffer::from_byte_array(&oversized_control)).is_err());
    assert!(parser().accept_chunk(VSBuffer::from_byte_array(&[0x82, 0x7f, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00])).is_err());
}

#[test]
fn rejects_frames_over_the_configured_payload_limit_after_reading_the_header() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let encoded = encode_web_socket_frame(&VSBuffer::from_string("too large"), options(WebSocketOpcode::Text, None)).unwrap();
    let mut limited = WebSocketFrameParser::new(IWebSocketFrameParserOptions { max_payload_length: Some(4), ..Default::default() }).unwrap();
    let error = limited.accept_chunk(encoded.slice(Some(0), Some(2))).expect_err("devia rejeitar");
    assert!(error.message.contains("configured limit of 4"), "{}", error.message);
}
