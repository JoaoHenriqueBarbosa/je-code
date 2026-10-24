//! Porte de `src/vs/editor/test/common/viewModel/lineBreakData.test.ts` (suíte `Editor ViewModel - LineBreakData`), caso a
//! caso e na mesma ordem (as suítes aninhadas `Injected Text 1`, `2` e `3` viram os prefixos `injected_text_N_`).
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de `je-base/tests/common`, criado no começo de cada
//!   caso.
//! - Os `const data` das suítes aninhadas (criados uma vez por suíte no original) são funções que criam o dado a cada
//!   caso; `ModelLineProjectionData` não guarda estado mutável, então o resultado é o mesmo.
//! - `mapTextToInjectedTextOptions` é `ModelDecorationInjectedTextOptions.from({ content })`, que no porte é
//!   `normalize_injected_text_options` de `text_model`; o valor sai do `Rc` por clonagem.
//! - `[]` (lista vazia, não `null`) nas injeções de `Basic` é `Some(vec![])`.
//! - `translateToOutputPosition(i)` sem afinidade usa o padrão do original, `PositionAffinity.None`.
//! - `OutputPosition.toString()` é o `Display` (`linha:deslocamento`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::model::{InjectedTextOptions, PositionAffinity};
use je_editor::model_line_projection_data::ModelLineProjectionData;
use je_editor::text_model::normalize_injected_text_options;

fn sequence(length: usize) -> Vec<usize> {
    (0..length).collect()
}

fn test_inverse(data: &ModelLineProjectionData) {
    for i in 0..100 {
        let output = data.translate_to_output_position(i, PositionAffinity::None);
        assert_eq!(data.translate_to_input_offset(output.output_line_index, output.output_offset), i);
    }
}

fn get_input_offsets(data: &ModelLineProjectionData, output_line_idx: usize) -> Vec<usize> {
    sequence(20).into_iter().map(|i| data.translate_to_input_offset(output_line_idx, i)).collect()
}

fn get_output_offsets(data: &ModelLineProjectionData, affinity: PositionAffinity) -> Vec<String> {
    sequence(25).into_iter().map(|i| data.translate_to_output_position(i, affinity).to_string()).collect()
}

fn map_text_to_injected_text_options(arr: &[&str]) -> Vec<InjectedTextOptions> {
    arr.iter()
        .map(|content| {
            let options = InjectedTextOptions { content: (*content).to_string(), ..Default::default() };
            (*normalize_injected_text_options(&options)).clone()
        })
        .collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn injected_data(offsets: Vec<usize>, texts: &[&str], wrapped_text_indent_length: usize) -> ModelLineProjectionData {
    ModelLineProjectionData::new(
        Some(offsets),
        Some(map_text_to_injected_text_options(texts)),
        vec![10, 100],
        vec![],
        wrapped_text_indent_length,
    )
}

fn injected_text_1() -> ModelLineProjectionData {
    injected_data(vec![2, 3, 10], &["1", "22", "333"], 10)
}

fn injected_text_2() -> ModelLineProjectionData {
    injected_data(vec![2, 2, 6], &["1", "22", "333"], 0)
}

fn injected_text_3() -> ModelLineProjectionData {
    injected_data(vec![2, 2, 7], &["1", "22", "333"], 0)
}

#[test]
fn basic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let data = ModelLineProjectionData::new(Some(vec![]), Some(vec![]), vec![100], vec![0.0], 10);

    assert_eq!(data.translate_to_input_offset(0, 50), 50);
    assert_eq!(data.translate_to_input_offset(1, 60), 150);
}

#[test]
fn injected_text_1_get_input_offset_of_output_position() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let data = injected_text_1();
    // For every view model position, what is the model position?
    assert_eq!(get_input_offsets(&data, 0), vec![0, 1, 2, 2, 3, 3, 3, 4, 5, 6, 7, 8, 9, 10, 10, 10, 10, 11, 12, 13]);
    assert_eq!(get_input_offsets(&data, 1), vec![7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 8, 9, 10, 10, 10, 10, 11, 12, 13]);
}

#[test]
fn injected_text_1_get_output_position_of_input_offset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let data = injected_text_1();
    data.translate_to_output_position(20, PositionAffinity::None);
    assert_eq!(
        get_output_offsets(&data, PositionAffinity::None),
        strings(&[
            "0:0", "0:1", "0:2", "0:4", "0:7", "0:8", "0:9", "1:10", "1:11", "1:12", "1:13", "1:17", "1:18", "1:19",
            "1:20", "1:21", "1:22", "1:23", "1:24", "1:25", "1:26", "1:27", "1:28", "1:29", "1:30",
        ])
    );

    assert_eq!(
        get_output_offsets(&data, PositionAffinity::Left),
        strings(&[
            "0:0", "0:1", "0:2", "0:4", "0:7", "0:8", "0:9", "0:10", "1:11", "1:12", "1:13", "1:17", "1:18", "1:19",
            "1:20", "1:21", "1:22", "1:23", "1:24", "1:25", "1:26", "1:27", "1:28", "1:29", "1:30",
        ])
    );

    assert_eq!(
        get_output_offsets(&data, PositionAffinity::Right),
        strings(&[
            "0:0", "0:1", "0:3", "0:6", "0:7", "0:8", "0:9", "1:10", "1:11", "1:12", "1:16", "1:17", "1:18", "1:19",
            "1:20", "1:21", "1:22", "1:23", "1:24", "1:25", "1:26", "1:27", "1:28", "1:29", "1:30",
        ])
    );
}

#[test]
fn injected_text_1_get_input_offset_of_output_position_is_inverse_of_get_output_position_of_input_offset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_inverse(&injected_text_1());
}

#[test]
fn injected_text_1_normalization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let data = injected_text_1();
    let actual: Vec<String> = sequence(25)
        .into_iter()
        .map(|v| data.normalize_output_position(1, v, PositionAffinity::Right).to_string())
        .collect();
    assert_eq!(
        actual,
        strings(&[
            "1:0", "1:1", "1:2", "1:3", "1:4", "1:5", "1:6", "1:7", "1:8", "1:9", "1:10", "1:11", "1:12", "1:16",
            "1:16", "1:16", "1:16", "1:17", "1:18", "1:19", "1:20", "1:21", "1:22", "1:23", "1:24",
        ])
    );
}

#[test]
fn injected_text_2_get_input_offset_of_output_position() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let data = injected_text_2();
    assert_eq!(get_input_offsets(&data, 0), vec![0, 1, 2, 2, 2, 2, 3, 4, 5, 6, 6, 6, 6, 7, 8, 9, 10, 11, 12, 13]);
    assert_eq!(
        get_input_offsets(&data, 1),
        vec![6, 6, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23]
    );
}

#[test]
fn injected_text_2_get_input_offset_of_output_position_is_inverse_of_get_output_position_of_input_offset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_inverse(&injected_text_2());
}

#[test]
fn injected_text_3_get_input_offset_of_output_position() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let data = injected_text_3();
    assert_eq!(get_input_offsets(&data, 0), vec![0, 1, 2, 2, 2, 2, 3, 4, 5, 6, 7, 7, 7, 7, 8, 9, 10, 11, 12, 13]);
    assert_eq!(
        get_input_offsets(&data, 1),
        vec![7, 7, 7, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23]
    );
}

#[test]
fn injected_text_3_get_input_offset_of_output_position_is_inverse_of_get_output_position_of_input_offset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    test_inverse(&injected_text_3());
}
