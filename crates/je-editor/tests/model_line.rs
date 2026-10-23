//! Porte de `src/vs/editor/test/common/model/model.line.test.ts` (suítes `ModelLine - getIndentLevel` e `ModelLinesTokens`), caso a caso e na mesma ordem.
//!
//! Cada `suite` do original vira um `mod`.
//!
//! Notas de tradução:
//! - `ManualTokenizationSupport` implementa `ITokenizationSupport` com tokens que o teste empurra por
//!   `set_line_tokens` (via `ContiguousMultilineTokensBuilder` e o `IBackgroundTokenizationStore` que o modelo
//!   entrega ao `createBackgroundTokenizer`).
//! - `assertLineTokens` compara `getEndOffset` e `getClassName` de `LineTokens::inflate`; `TestLineToken`
//!   do original só decodifica os mesmos metadados.
//! - O `TokenizationRegistry` é global por thread (`tokenization_registry()`); cada `#[test]` roda no
//!   próprio thread.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use je_editor::cursor_common::CursorSimpleModel;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_editor::contiguous_multiline_tokens::ContiguousMultilineTokensBuilder;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::encoded_token_attributes::{TokenMetadata, metadata_consts};
use je_editor::languages::{
    EncodedTokenizationResult, IBackgroundTokenizationStore, IBackgroundTokenizer, IState, ITokenizationSupport, State,
    TokenizationResult,
};
use je_editor::line_tokens::{IViewLineTokens, LineTokens};
use je_editor::model_utils::compute_indent_level;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_editor::tokenization_registry::tokenization_registry;
use support::test_text_model::create_text_model_with;

use common::ensure_no_disposables_are_leaked_in_test_suite;

mod support;

mod model_line_get_indent_level {
    use super::*;

    /// `assertIndentLevel(text, expected, tabSize = 4)`.
    fn assert_indent_level(text: &str, expected: i32) {
        let actual = compute_indent_level(JsString::from(text).units(), 4);
        assert_eq!(actual, expected, "{text}");
    }

    #[test]
    fn get_indent_level() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_indent_level("", -1);
        assert_indent_level(" ", -1);
        assert_indent_level("   \t", -1);
        assert_indent_level("Hello", 0);
        assert_indent_level(" Hello", 1);
        assert_indent_level("   Hello", 3);
        assert_indent_level("\tHello", 4);
        assert_indent_level(" \tHello", 4);
        assert_indent_level("  \tHello", 4);
        assert_indent_level("   \tHello", 4);
        assert_indent_level("    \tHello", 8);
        assert_indent_level("     \tHello", 8);
        assert_indent_level("\t Hello", 5);
        assert_indent_level("\t \tHello", 8);
    }
}

/// `TestToken`: um token que começa em `start_offset` com a cor `color`.
#[derive(Clone, Copy)]
struct TestToken {
    start_offset: u32,
    color: u32,
}

fn tt(start_offset: u32, color: u32) -> TestToken {
    TestToken { start_offset, color }
}

/// `TestToken.toTokens(tokens)`.
fn to_tokens(tokens: &[TestToken]) -> Vec<u32> {
    let mut result = Vec::with_capacity(tokens.len() << 1);
    for token in tokens {
        result.push(token.start_offset);
        result.push(token.color << metadata_consts::FOREGROUND_OFFSET);
    }
    result
}

/// `assertLineTokens(actual, expected)`.
fn assert_line_tokens(actual: &LineTokens, expected: &[TestToken]) {
    let mut tmp = to_tokens(expected);
    LineTokens::convert_to_end_offset(&mut tmp, actual.get_line_content().len());
    let expected: Vec<(i32, String)> = (0..tmp.len() >> 1)
        .map(|i| (tmp[i << 1] as i32, TokenMetadata::get_class_name_from_metadata(tmp[(i << 1) + 1])))
        .collect();
    let inflated = actual.inflate();
    let actual: Vec<(i32, String)> =
        (0..inflated.get_count()).map(|i| (inflated.get_end_offset(i), inflated.get_class_name(i))).collect();
    assert_eq!(actual, expected);
}

struct LineState {
    line_number: i32,
}

impl IState for LineState {
    fn clone_state(&self) -> State {
        Rc::new(LineState { line_number: self.line_number })
    }

    fn equals(&self, other: &dyn IState) -> bool {
        other.as_any().downcast_ref::<LineState>().is_some_and(|o| o.line_number == self.line_number)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

type Stores = Rc<RefCell<Vec<Rc<dyn IBackgroundTokenizationStore>>>>;

#[derive(Default)]
struct ManualTokenizationSupport {
    tokens: RefCell<HashMap<i32, Vec<u32>>>,
    stores: Stores,
}

impl ManualTokenizationSupport {
    fn set_line_tokens(&self, line_number: i32, tokens: Vec<u32>) {
        let stores: Vec<_> = self.stores.borrow().clone();
        for store in stores {
            let mut builder = ContiguousMultilineTokensBuilder::new();
            builder.add(line_number, tokens.clone());
            store.set_tokens(builder.finalize());
        }
    }
}

struct ManualBackgroundTokenizer {
    store: Rc<dyn IBackgroundTokenizationStore>,
    stores: Stores,
}

impl IDisposable for ManualBackgroundTokenizer {
    fn dispose(&self) {
        self.stores.borrow_mut().retain(|s| !Rc::ptr_eq(s, &self.store));
    }
}

impl IBackgroundTokenizer for ManualBackgroundTokenizer {
    fn request_tokens(&self, _start_line_number: i32, _end_line_number_exclusive: i32) {}
}

impl ITokenizationSupport for ManualTokenizationSupport {
    fn get_initial_state(&self) -> State {
        Rc::new(LineState { line_number: 1 })
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        panic!("Error")
    }

    fn tokenize_encoded(&self, _line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        let s = state.as_any().downcast_ref::<LineState>().expect("LineState");
        EncodedTokenizationResult {
            tokens: self.tokens.borrow().get(&s.line_number).cloned().expect("tokens da linha"),
            font_info: Vec::new(),
            end_state: Rc::new(LineState { line_number: s.line_number + 1 }),
        }
    }

    /// Can be/return undefined if default background tokenization should be used.
    fn create_background_tokenizer(
        &self,
        _text_model: &Rc<TextModel>,
        store: Rc<dyn IBackgroundTokenizationStore>,
    ) -> Option<Rc<dyn IBackgroundTokenizer>> {
        self.stores.borrow_mut().push(store.clone());
        Some(Rc::new(ManualBackgroundTokenizer { store, stores: self.stores.clone() }))
    }
}

/// `IBufferLineState`.
struct BufferLineState {
    text: &'static str,
    tokens: Vec<TestToken>,
}

fn line_state(text: &'static str, tokens: Vec<TestToken>) -> BufferLineState {
    BufferLineState { text, tokens }
}

/// `IEdit`.
struct Edit {
    range: Range,
    text: &'static str,
}

fn edit(range: Range, text: &'static str) -> Edit {
    Edit { range, text }
}

/// `ILineEdit`.
struct LineEdit {
    start_column: i32,
    end_column: i32,
    text: &'static str,
}

fn line_edit(start_column: i32, end_column: i32, text: &'static str) -> LineEdit {
    LineEdit { start_column, end_column, text }
}

/// Registra o suporte manual para a linguagem `test`, devolvendo-o junto do registro.
fn register_manual_support() -> (Rc<ManualTokenizationSupport>, je_base::lifecycle::DisposableRef) {
    let support = Rc::new(ManualTokenizationSupport::default());
    let registration = tokenization_registry().register("test", support.clone());
    (support, registration)
}

fn apply_edits(model: &TextModel, edits: &[Edit]) {
    let operations: Vec<SingleEditOperation> = edits
        .iter()
        .map(|ed| SingleEditOperation::new(ed.range, Some(JsString::from(ed.text)), false))
        .collect();
    model.apply_edits(&operations);
}

/// `testApplyEdits(initial, edits, expected)`.
fn test_apply_edits(initial: Vec<BufferLineState>, edits: Vec<Edit>, expected: Vec<BufferLineState>) {
    let initial_text = initial.iter().map(|el| el.text).collect::<Vec<_>>().join("\n");

    let (support, registration) = register_manual_support();

    let model = create_text_model_with(initial_text.as_str(), Some("test"), Default::default(), None);
    let _view = model.on_before_attached();
    for (line_index, line) in initial.iter().enumerate() {
        let line_text_length = model.get_line_max_column(line_index as i32 + 1) - 1;
        let mut tokens = to_tokens(&line.tokens);
        LineTokens::convert_to_end_offset(&mut tokens, line_text_length as usize);
        support.set_line_tokens(line_index as i32 + 1, tokens);
    }

    apply_edits(&model, &edits);

    for (line_index, expected_line) in expected.iter().enumerate() {
        let actual_line = model.get_line_content(line_index as i32 + 1);
        let actual_tokens = model.tokenization().get_line_tokens(line_index as i32 + 1);
        assert_eq!(actual_line, expected_line.text);
        assert_line_tokens(&actual_tokens, &expected_line.tokens);
    }

    model.dispose();
    registration.dispose();
}

/// `testLineEditTokens(initialText, initialTokens, edits, expectedText, expectedTokens)`.
fn test_line_edit_tokens(
    initial_text: &'static str,
    initial_tokens: Vec<TestToken>,
    edits: Vec<LineEdit>,
    expected_text: &'static str,
    expected_tokens: Vec<TestToken>,
) {
    test_apply_edits(
        vec![line_state(initial_text, initial_tokens)],
        edits.iter().map(|ed| edit(Range::new(1, ed.start_column, 1, ed.end_column), ed.text)).collect(),
        vec![line_state(expected_text, expected_tokens)],
    );
}

/// `testLineSplitTokens(initialText, initialTokens, splitColumn, expectedText1, expectedText2, expectedTokens)`.
fn test_line_split_tokens(
    initial_text: &'static str,
    initial_tokens: Vec<TestToken>,
    split_column: i32,
    expected_text1: &'static str,
    expected_text2: &'static str,
    expected_tokens: Vec<TestToken>,
) {
    test_apply_edits(
        vec![line_state(initial_text, initial_tokens)],
        vec![edit(Range::new(1, split_column, 1, split_column), "\n")],
        vec![line_state(expected_text1, expected_tokens), line_state(expected_text2, vec![tt(0, 1)])],
    );
}

/// `testLineAppendTokens(aText, aTokens, bText, bTokens, expectedText, expectedTokens)`.
fn test_line_append_tokens(
    a_text: &'static str,
    a_tokens: Vec<TestToken>,
    b_text: &'static str,
    b_tokens: Vec<TestToken>,
    expected_text: &'static str,
    expected_tokens: Vec<TestToken>,
) {
    test_apply_edits(
        vec![line_state(a_text, a_tokens), line_state(b_text, b_tokens)],
        vec![edit(Range::new(1, a_text.encode_utf16().count() as i32 + 1, 2, 1), "")],
        vec![line_state(expected_text, expected_tokens)],
    );
}

mod model_lines_tokens {
    use super::*;

    #[test]
    fn single_delete_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 1, 1, 2), "")], vec![line_state("ello world", vec![tt(0, 1), tt(4, 2), tt(5, 3)])]);
    }

    #[test]
    fn single_delete_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("helloworld", vec![tt(0, 1), tt(5, 2)])], vec![edit(Range::new(1, 3, 1, 8), "")], vec![line_state("herld", vec![tt(0, 1), tt(2, 2)])]);
    }

    #[test]
    fn single_delete_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 1, 1, 6), "")], vec![line_state(" world", vec![tt(0, 2), tt(1, 3)])]);
    }

    #[test]
    fn single_delete_4() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 2, 1, 7), "")], vec![line_state("hworld", vec![tt(0, 1), tt(1, 3)])]);
    }

    #[test]
    fn single_delete_5() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 1, 1, 12), "")], vec![line_state("", vec![tt(0, 1)])]);
    }

    #[test]
    fn multi_delete_6() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)]), line_state("hello world", vec![tt(0, 4), tt(5, 5), tt(6, 6)]), line_state("hello world", vec![tt(0, 7), tt(5, 8), tt(6, 9)])], vec![edit(Range::new(1, 6, 3, 6), "")], vec![line_state("hello world", vec![tt(0, 1), tt(5, 8), tt(6, 9)])]);
    }

    #[test]
    fn multi_delete_7() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)]), line_state("hello world", vec![tt(0, 4), tt(5, 5), tt(6, 6)]), line_state("hello world", vec![tt(0, 7), tt(5, 8), tt(6, 9)])], vec![edit(Range::new(1, 12, 3, 12), "")], vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])]);
    }

    #[test]
    fn multi_delete_8() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)]), line_state("hello world", vec![tt(0, 4), tt(5, 5), tt(6, 6)]), line_state("hello world", vec![tt(0, 7), tt(5, 8), tt(6, 9)])], vec![edit(Range::new(1, 1, 3, 1), "")], vec![line_state("hello world", vec![tt(0, 7), tt(5, 8), tt(6, 9)])]);
    }

    #[test]
    fn multi_delete_9() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)]), line_state("hello world", vec![tt(0, 4), tt(5, 5), tt(6, 6)]), line_state("hello world", vec![tt(0, 7), tt(5, 8), tt(6, 9)])], vec![edit(Range::new(1, 12, 3, 1), "")], vec![line_state("hello worldhello world", vec![tt(0, 1), tt(5, 2), tt(6, 3), tt(11, 7), tt(16, 8), tt(17, 9)])]);
    }

    #[test]
    fn single_insert_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 1, 1, 1), "xx")], vec![line_state("xxhello world", vec![tt(0, 1), tt(7, 2), tt(8, 3)])]);
    }

    #[test]
    fn single_insert_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 2, 1, 2), "xx")], vec![line_state("hxxello world", vec![tt(0, 1), tt(7, 2), tt(8, 3)])]);
    }

    #[test]
    fn single_insert_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 6, 1, 6), "xx")], vec![line_state("helloxx world", vec![tt(0, 1), tt(7, 2), tt(8, 3)])]);
    }

    #[test]
    fn single_insert_4() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 7, 1, 7), "xx")], vec![line_state("hello xxworld", vec![tt(0, 1), tt(5, 2), tt(8, 3)])]);
    }

    #[test]
    fn single_insert_5() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 12, 1, 12), "xx")], vec![line_state("hello worldxx", vec![tt(0, 1), tt(5, 2), tt(6, 3)])]);
    }

    #[test]
    fn multi_insert_6() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 1, 1, 1), "\n")], vec![line_state("", vec![tt(0, 1)]), line_state("hello world", vec![tt(0, 1)])]);
    }

    #[test]
    fn multi_insert_7() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 12, 1, 12), "\n")], vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)]), line_state("", vec![tt(0, 1)])]);
    }

    #[test]
    fn multi_insert_8() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)])], vec![edit(Range::new(1, 7, 1, 7), "\n")], vec![line_state("hello ", vec![tt(0, 1), tt(5, 2)]), line_state("world", vec![tt(0, 1)])]);
    }

    #[test]
    fn multi_insert_9() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_apply_edits(vec![line_state("hello world", vec![tt(0, 1), tt(5, 2), tt(6, 3)]), line_state("hello world", vec![tt(0, 4), tt(5, 5), tt(6, 6)])], vec![edit(Range::new(1, 7, 1, 7), "xx\nyy")], vec![line_state("hello xx", vec![tt(0, 1), tt(5, 2)]), line_state("yyworld", vec![tt(0, 1)]), line_state("hello world", vec![tt(0, 4), tt(5, 5), tt(6, 6)])]);
    }

    #[test]
    fn insertion_on_empty_line() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (support, registration) = register_manual_support();

        let model = create_text_model_with("some text", Some("test"), Default::default(), None);
        let mut tokens = to_tokens(&[tt(0, 1)]);
        LineTokens::convert_to_end_offset(&mut tokens, (model.get_line_max_column(1) - 1) as usize);
        support.set_line_tokens(1, tokens);

        apply_edits(&model, &[edit(Range::new(1, 1, 1, 10), "")]);

        support.set_line_tokens(1, Vec::new());

        apply_edits(&model, &[edit(Range::new(1, 1, 1, 1), "a")]);

        let actual_tokens = model.tokenization().get_line_tokens(1);
        assert_line_tokens(&actual_tokens, &[tt(0, 1)]);

        model.dispose();
        registration.dispose();
    }

    #[test]
    fn updates_tokens_on_insertion_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(1, 1, "a")], "aabcd efgh", vec![tt(0, 1), tt(5, 2), tt(6, 3)]);
    }

    #[test]
    fn updates_tokens_on_insertion_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("aabcd efgh", vec![tt(0, 1), tt(5, 2), tt(6, 3)], vec![line_edit(2, 2, "x")], "axabcd efgh", vec![tt(0, 1), tt(6, 2), tt(7, 3)]);
    }

    #[test]
    fn updates_tokens_on_insertion_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("axabcd efgh", vec![tt(0, 1), tt(6, 2), tt(7, 3)], vec![line_edit(3, 3, "stu")], "axstuabcd efgh", vec![tt(0, 1), tt(9, 2), tt(10, 3)]);
    }

    #[test]
    fn updates_tokens_on_insertion_4() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("axstuabcd efgh", vec![tt(0, 1), tt(9, 2), tt(10, 3)], vec![line_edit(10, 10, "\t")], "axstuabcd\t efgh", vec![tt(0, 1), tt(10, 2), tt(11, 3)]);
    }

    #[test]
    fn updates_tokens_on_insertion_5() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("axstuabcd\t efgh", vec![tt(0, 1), tt(10, 2), tt(11, 3)], vec![line_edit(12, 12, "dd")], "axstuabcd\t ddefgh", vec![tt(0, 1), tt(10, 2), tt(13, 3)]);
    }

    #[test]
    fn updates_tokens_on_insertion_6() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("axstuabcd\t ddefgh", vec![tt(0, 1), tt(10, 2), tt(13, 3)], vec![line_edit(18, 18, "xyz")], "axstuabcd\t ddefghxyz", vec![tt(0, 1), tt(10, 2), tt(13, 3)]);
    }

    #[test]
    fn updates_tokens_on_insertion_7() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("axstuabcd\t ddefghxyz", vec![tt(0, 1), tt(10, 2), tt(13, 3)], vec![line_edit(1, 1, "x")], "xaxstuabcd\t ddefghxyz", vec![tt(0, 1), tt(11, 2), tt(14, 3)]);
    }

    #[test]
    fn updates_tokens_on_insertion_8() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("xaxstuabcd\t ddefghxyz", vec![tt(0, 1), tt(11, 2), tt(14, 3)], vec![line_edit(22, 22, "x")], "xaxstuabcd\t ddefghxyzx", vec![tt(0, 1), tt(11, 2), tt(14, 3)]);
    }

    #[test]
    fn updates_tokens_on_insertion_9() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("xaxstuabcd\t ddefghxyzx", vec![tt(0, 1), tt(11, 2), tt(14, 3)], vec![line_edit(2, 2, "")], "xaxstuabcd\t ddefghxyzx", vec![tt(0, 1), tt(11, 2), tt(14, 3)]);
    }

    #[test]
    fn updates_tokens_on_insertion_10() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("", vec![], vec![line_edit(1, 1, "a")], "a", vec![tt(0, 1)]);
    }

    #[test]
    fn delete_second_token_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcdefghij", vec![tt(0, 1), tt(3, 2), tt(6, 3)], vec![line_edit(4, 7, "")], "abcghij", vec![tt(0, 1), tt(3, 3)]);
    }

    #[test]
    fn insert_right_before_second_token() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcdefghij", vec![tt(0, 1), tt(3, 2), tt(6, 3)], vec![line_edit(4, 4, "hello")], "abchellodefghij", vec![tt(0, 1), tt(8, 2), tt(11, 3)]);
    }

    #[test]
    fn delete_first_char() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(1, 2, "")], "bcd efgh", vec![tt(0, 1), tt(3, 2), tt(4, 3)]);
    }

    #[test]
    fn delete_2nd_and_3rd_chars() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(2, 4, "")], "ad efgh", vec![tt(0, 1), tt(2, 2), tt(3, 3)]);
    }

    #[test]
    fn delete_first_token() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(1, 5, "")], " efgh", vec![tt(0, 2), tt(1, 3)]);
    }

    #[test]
    fn delete_second_token() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(5, 6, "")], "abcdefgh", vec![tt(0, 1), tt(4, 3)]);
    }

    #[test]
    fn delete_second_token_a_bit_of_the_third_one() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(5, 7, "")], "abcdfgh", vec![tt(0, 1), tt(4, 3)]);
    }

    #[test]
    fn delete_second_and_third_token() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(5, 10, "")], "abcd", vec![tt(0, 1)]);
    }

    #[test]
    fn delete_everything() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(1, 10, "")], "", vec![tt(0, 1)]);
    }

    #[test]
    fn noop() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(1, 1, "")], "abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)]);
    }

    #[test]
    fn equivalent_to_deleting_first_two_chars() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(1, 3, "")], "cd efgh", vec![tt(0, 1), tt(2, 2), tt(3, 3)]);
    }

    #[test]
    fn equivalent_to_deleting_from_5_to_the_end() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], vec![line_edit(5, 10, "")], "abcd", vec![tt(0, 1)]);
    }

    #[test]
    fn updates_tokens_on_replace_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("Hello world, ciao", vec![tt(0, 1), tt(5, 0), tt(6, 2), tt(11, 0), tt(13, 0)], vec![line_edit(1, 6, "Hi")], "Hi world, ciao", vec![tt(0, 0), tt(3, 2), tt(8, 0), tt(10, 0)]);
    }

    #[test]
    fn updates_tokens_on_replace_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_edit_tokens("Hello world, ciao", vec![tt(0, 1), tt(5, 0), tt(6, 2), tt(11, 0), tt(13, 0)], vec![line_edit(1, 6, "Hi"), line_edit(8, 12, "my friends")], "Hi wmy friends, ciao", vec![tt(0, 0), tt(3, 2), tt(14, 0), tt(16, 0)]);
    }

    #[test]
    fn split_at_the_beginning() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_split_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], 1, "", "abcd efgh", vec![tt(0, 1)]);
    }

    #[test]
    fn split_at_the_end() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_split_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], 10, "abcd efgh", "", vec![tt(0, 1), tt(4, 2), tt(5, 3)]);
    }

    #[test]
    fn split_inthe_middle_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_split_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], 5, "abcd", " efgh", vec![tt(0, 1)]);
    }

    #[test]
    fn split_inthe_middle_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_split_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], 6, "abcd ", "efgh", vec![tt(0, 1), tt(4, 2)]);
    }

    #[test]
    fn append_empty_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_append_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], "", vec![], "abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)]);
    }

    #[test]
    fn append_empty_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_append_tokens("", vec![], "abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], "abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)]);
    }

    #[test]
    fn append_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_append_tokens("abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)], "abcd efgh", vec![tt(0, 4), tt(4, 5), tt(5, 6)], "abcd efghabcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3), tt(9, 4), tt(13, 5), tt(14, 6)]);
    }

    #[test]
    fn append_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_append_tokens("abcd ", vec![tt(0, 1), tt(4, 2)], "efgh", vec![tt(0, 3)], "abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)]);
    }

    #[test]
    fn append_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_line_append_tokens("abcd", vec![tt(0, 1)], " efgh", vec![tt(0, 2), tt(1, 3)], "abcd efgh", vec![tt(0, 1), tt(4, 2), tt(5, 3)]);
    }
}
