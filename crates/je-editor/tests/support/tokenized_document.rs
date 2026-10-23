//! Porte de `TokenizedDocument` e `TokenInfo`, exportados por
//! `src/vs/editor/test/common/model/bracketPairColorizer/tokenizer.test.ts` e usados também por
//! `getBracketPairsInRange.test.ts`: um documento com os tokens dados por linha e o suporte de tokenização
//! que os devolve.

use std::any::Any;
use std::rc::Rc;

use je_base::JsString;
use je_editor::encoded_token_attributes::metadata_consts as mc;
use je_editor::languages::{EncodedTokenizationResult, IState, ITokenizationSupport, State, TokenizationResult};

/// `TokenInfo`.
#[derive(Clone)]
pub struct TokenInfo {
    pub text: JsString,
    pub language_id: u32,
    pub token_type: u32,
    pub has_balanced_brackets: bool,
}

impl TokenInfo {
    pub fn new(text: &str, language_id: u32, token_type: u32, has_balanced_brackets: bool) -> Self {
        Self { text: JsString::from(text), language_id, token_type, has_balanced_brackets }
    }

    pub fn get_metadata(&self) -> u32 {
        ((self.language_id << mc::LANGUAGEID_OFFSET) | (self.token_type << mc::TOKEN_TYPE_OFFSET))
            | if self.has_balanced_brackets { mc::BALANCED_BRACKETS_MASK } else { 0 }
    }

    pub fn with_text(&self, text: JsString) -> TokenInfo {
        TokenInfo { text, language_id: self.language_id, token_type: self.token_type, has_balanced_brackets: self.has_balanced_brackets }
    }
}

/// `TokenizedDocument`.
pub struct TokenizedDocument {
    tokens_by_line: Rc<Vec<Vec<TokenInfo>>>,
}

impl TokenizedDocument {
    pub fn new(tokens: Vec<TokenInfo>) -> Self {
        let mut tokens_by_line: Vec<Vec<TokenInfo>> = Vec::new();
        let mut cur_line: Vec<TokenInfo> = Vec::new();

        for token in &tokens {
            // `token.text.split('\n')`
            let mut lines: Vec<JsString> = token.text.units().split(|&u| u == 10).map(JsString::from).collect();
            let mut first = true;
            while !lines.is_empty() {
                if !first {
                    tokens_by_line.push(std::mem::take(&mut cur_line));
                } else {
                    first = false;
                }

                // Como no original, lê o primeiro e remove o último.
                if !lines[0].is_empty() {
                    cur_line.push(token.with_text(lines[0].clone()));
                }
                lines.pop();
            }
        }

        tokens_by_line.push(cur_line);

        Self { tokens_by_line: Rc::new(tokens_by_line) }
    }

    pub fn get_text(&self) -> JsString {
        let mut result = JsString::new();
        for (i, line) in self.tokens_by_line.iter().enumerate() {
            if i > 0 {
                result.push_unit(10);
            }
            for token in line {
                result.push_units(token.text.units());
            }
        }
        result
    }

    pub fn get_tokenization_support(&self) -> Rc<DocumentTokenizationSupport> {
        Rc::new(DocumentTokenizationSupport { tokens_by_line: self.tokens_by_line.clone() })
    }
}

struct DocState {
    line_number: usize,
}

impl IState for DocState {
    fn clone_state(&self) -> State {
        Rc::new(DocState { line_number: self.line_number })
    }

    fn equals(&self, other: &dyn IState) -> bool {
        other.as_any().downcast_ref::<DocState>().is_some_and(|o| o.line_number == self.line_number)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// O `ITokenizationSupport` de um [`TokenizedDocument`].
pub struct DocumentTokenizationSupport {
    tokens_by_line: Rc<Vec<Vec<TokenInfo>>>,
}

impl ITokenizationSupport for DocumentTokenizationSupport {
    fn get_initial_state(&self) -> State {
        Rc::new(DocState { line_number: 0 })
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        panic!("Method not implemented.");
    }

    fn tokenize_encoded(&self, _line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        let state2 = state.as_any().downcast_ref::<DocState>().expect("estado do documento");
        let tokens = &self.tokens_by_line[state2.line_number];
        let mut arr: Vec<u32> = Vec::new();
        let mut offset = 0u32;
        for t in tokens {
            arr.push(offset);
            arr.push(t.get_metadata());
            offset += t.text.len() as u32;
        }

        EncodedTokenizationResult { tokens: arr, font_info: Vec::new(), end_state: Rc::new(DocState { line_number: state2.line_number + 1 }) }
    }
}
