//! Porte de `src/vs/editor/test/common/services/semanticTokensDto.test.ts` (suíte `SemanticTokensDto`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::VSBuffer;
use je_editor::semantic_tokens_dto::{
    decode_semantic_tokens_dto, encode_semantic_tokens_dto, DeltaSemanticTokensDto, FullSemanticTokensDto, SemanticTokensDelta,
    SemanticTokensDto,
};

fn assert_equal_full(actual: &FullSemanticTokensDto, expected: &FullSemanticTokensDto) {
    assert_eq!(actual, expected);
}

fn assert_equal_delta(actual: &DeltaSemanticTokensDto, expected: &DeltaSemanticTokensDto) {
    assert_eq!(actual, expected);
}

fn test_round_trip(value: &SemanticTokensDto) {
    let decoded = decode_semantic_tokens_dto(&encode_semantic_tokens_dto(value));
    match (value, &decoded) {
        (SemanticTokensDto::Full(expected), SemanticTokensDto::Full(actual)) => assert_equal_full(actual, expected),
        (SemanticTokensDto::Delta(expected), SemanticTokensDto::Delta(actual)) => assert_equal_delta(actual, expected),
        _ => panic!("wrong type"),
    }
}

const PACKED: u32 = (1 << 24) + (2 << 16) + (3 << 8) + 4;

#[test]
fn full_encoding() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_round_trip(&SemanticTokensDto::Full(FullSemanticTokensDto { id: 12, data: vec![PACKED] }));
}

#[test]
fn delta_encoding() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_round_trip(&SemanticTokensDto::Delta(DeltaSemanticTokensDto {
        id: 12,
        deltas: vec![
            SemanticTokensDelta { start: 0, delete_count: 4, data: None },
            SemanticTokensDelta { start: 15, delete_count: 0, data: Some(vec![PACKED]) },
            SemanticTokensDelta { start: 27, delete_count: 5, data: Some(vec![PACKED, 1, 2, 3, 4, 5, 6, 7, 8, 9]) },
        ],
    }));
}

#[test]
fn partial_array_buffer() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let shared_arr: Vec<u32> = vec![PACKED, 1, 2, 3, 4, 5, PACKED];
    test_round_trip(&SemanticTokensDto::Delta(DeltaSemanticTokensDto {
        id: 12,
        deltas: vec![
            SemanticTokensDelta { start: 0, delete_count: 4, data: Some(shared_arr[0..1].to_vec()) },
            SemanticTokensDelta { start: 15, delete_count: 0, data: Some(shared_arr[1..].to_vec()) },
        ],
    }));
}

#[test]
fn issue_94521_unusual_backing_array_buffer() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    fn wrap_and_slice_uint8_arry(buff: &[u8], prefix_length: usize, suffix_length: usize) -> VSBuffer {
        let mut wrapped = vec![0u8; prefix_length + buff.len() + suffix_length];
        wrapped[prefix_length..prefix_length + buff.len()].copy_from_slice(buff);
        VSBuffer::wrap(wrapped).slice(Some(prefix_length as i64), Some((prefix_length + buff.len()) as i64))
    }
    fn wrap_and_slice(buff: &VSBuffer, prefix_length: usize, suffix_length: usize) -> VSBuffer {
        wrap_and_slice_uint8_arry(&buff.to_vec(), prefix_length, suffix_length)
    }
    fn decode_full(buff: &VSBuffer) -> FullSemanticTokensDto {
        match decode_semantic_tokens_dto(buff) {
            SemanticTokensDto::Full(full) => full,
            SemanticTokensDto::Delta(_) => panic!("wrong type"),
        }
    }
    let dto = FullSemanticTokensDto { id: 5, data: vec![1, 2, 3, 4, 5] };
    let encoded = encode_semantic_tokens_dto(&SemanticTokensDto::Full(dto.clone()));

    // with misaligned prefix and misaligned suffix
    assert_equal_full(&decode_full(&wrap_and_slice(&encoded, 1, 1)), &dto);
    // with misaligned prefix and aligned suffix
    assert_equal_full(&decode_full(&wrap_and_slice(&encoded, 1, 4)), &dto);
    // with aligned prefix and misaligned suffix
    assert_equal_full(&decode_full(&wrap_and_slice(&encoded, 4, 1)), &dto);
    // with aligned prefix and aligned suffix
    assert_equal_full(&decode_full(&wrap_and_slice(&encoded, 4, 4)), &dto);
}
