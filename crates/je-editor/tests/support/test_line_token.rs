//! Porte de `src/vs/editor/test/common/core/testLineToken.ts`: `TestLineToken`, `TestLineTokens` e
//! `TestLineTokenFactory`.
//!
//! Os métodos que o original deixa lançando `Error('Not implemented')` viram `panic!` com a mesma mensagem.

use std::any::Any;
use std::rc::Rc;

use je_base::JsString;
use je_editor::encoded_token_attributes::{TokenMetadata, TokenPresentation};
use je_editor::languages::ILanguageIdCodec;
use je_editor::line_tokens::IViewLineTokens;

/// Um token de uma linha.
#[derive(Clone, Debug)]
pub struct TestLineToken {
    /// O último índice de caractere do token (exclusivo).
    pub end_index: i32,
    metadata: u32,
}

impl TestLineToken {
    pub fn new(end_index: i32, metadata: u32) -> Self {
        Self { end_index, metadata }
    }

    pub fn get_standard_token_type(&self) -> u32 {
        TokenMetadata::get_token_type(self.metadata)
    }

    pub fn get_foreground(&self) -> u32 {
        TokenMetadata::get_foreground(self.metadata)
    }

    pub fn get_type(&self) -> String {
        TokenMetadata::get_class_name_from_metadata(self.metadata)
    }

    pub fn get_inline_style(&self, color_map: &[String]) -> String {
        TokenMetadata::get_inline_style_from_metadata(self.metadata, color_map)
    }

    pub fn get_presentation(&self) -> TokenPresentation {
        TokenMetadata::get_presentation_from_metadata(self.metadata)
    }

    fn equals_token(a: &TestLineToken, b: &TestLineToken) -> bool {
        a.end_index == b.end_index && a.metadata == b.metadata
    }

    pub fn equals_arr(a: &[TestLineToken], b: &[TestLineToken]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| Self::equals_token(a, b))
    }
}

pub struct TestLineTokens {
    actual: Vec<TestLineToken>,
}

impl TestLineTokens {
    pub fn new(actual: Vec<TestLineToken>) -> Self {
        Self { actual }
    }
}

impl IViewLineTokens for TestLineTokens {
    fn language_id_codec(&self) -> &Rc<dyn ILanguageIdCodec> {
        panic!("Not implemented");
    }

    fn equals(&self, other: &dyn IViewLineTokens) -> bool {
        other
            .as_any()
            .downcast_ref::<TestLineTokens>()
            .is_some_and(|other| TestLineToken::equals_arr(&self.actual, &other.actual))
    }

    fn get_count(&self) -> usize {
        self.actual.len()
    }

    fn get_standard_token_type(&self, token_index: usize) -> u32 {
        self.actual[token_index].get_standard_token_type()
    }

    fn get_foreground(&self, token_index: usize) -> u32 {
        self.actual[token_index].get_foreground()
    }

    fn get_end_offset(&self, token_index: usize) -> i32 {
        self.actual[token_index].end_index
    }

    fn get_class_name(&self, token_index: usize) -> String {
        self.actual[token_index].get_type()
    }

    fn get_inline_style(&self, token_index: usize, color_map: &[String]) -> String {
        self.actual[token_index].get_inline_style(color_map)
    }

    fn get_presentation(&self, token_index: usize) -> TokenPresentation {
        self.actual[token_index].get_presentation()
    }

    fn find_token_index_at_offset(&self, _offset: i32) -> usize {
        panic!("Not implemented");
    }

    fn get_line_content(&self) -> JsString {
        panic!("Not implemented");
    }

    fn get_metadata(&self, _token_index: usize) -> u32 {
        panic!("Method not implemented.");
    }

    fn get_language_id(&self, _token_index: usize) -> String {
        panic!("Method not implemented.");
    }

    fn get_token_text(&self, _token_index: usize) -> JsString {
        panic!("Method not implemented.");
    }

    fn for_each(&self, _callback: &mut dyn FnMut(usize)) {
        panic!("Not implemented");
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct TestLineTokenFactory;

impl TestLineTokenFactory {
    pub fn inflate_arr(tokens: &[u32]) -> Vec<TestLineToken> {
        (0..tokens.len() >> 1)
            .map(|i| TestLineToken::new(tokens[i << 1] as i32, tokens[(i << 1) + 1]))
            .collect()
    }
}
