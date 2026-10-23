//! Porte de `src/vs/editor/common/languages/linkComputer.ts`: o detector de links do texto (`http://`,
//! `https://` e `file://`), uma máquina de estados sobre unidades UTF-16.
//!
//! Notas de tradução:
//! - `State` é um `u8` com as constantes em [`state`], porque o original aceita estados próprios depois de
//!   `LastKnownState`.
//! - `computeLinks(null)` é `compute_links(None)`; a checagem de `typeof getLineCount` do original não existe,
//!   o trait garante os métodos.
//! - O estado e o classificador compartilhados (`_stateMachine`, `_classifier`) são `OnceLock`.
//! - As colunas são em unidades UTF-16, como no JavaScript.

use std::sync::OnceLock;

use je_base::JsString;
use je_base::char_code as cc;

use crate::character_classifier::CharacterClassifier;
use crate::languages::{ILink, LinkUrl};
use crate::range::Range;

/// `ILinkComputerTarget`.
pub trait ILinkComputerTarget {
    fn get_line_count(&self) -> i32;
    fn get_line_content(&self, line_number: i32) -> JsString;
}

/// `State`.
pub type State = u8;

/// Os valores de `State`.
pub mod state {
    use super::State;

    pub const INVALID: State = 0;
    pub const START: State = 1;
    pub const H: State = 2;
    pub const HT: State = 3;
    pub const HTT: State = 4;
    pub const HTTP: State = 5;
    pub const F: State = 6;
    pub const FI: State = 7;
    pub const FIL: State = 8;
    pub const BEFORE_COLON: State = 9;
    pub const AFTER_COLON: State = 10;
    pub const ALMOST_THERE: State = 11;
    pub const END: State = 12;
    pub const ACCEPT: State = 13;
    /// Marcador: estados próprios podem vir depois.
    pub const LAST_KNOWN_STATE: State = 14;
}

/// `Edge`: (estado de origem, código do caractere, estado de destino).
pub type Edge = (State, i32, State);

/// `Uint8Matrix`.
struct Uint8Matrix {
    data: Vec<u8>,
    cols: usize,
}

impl Uint8Matrix {
    fn new(rows: usize, cols: usize, default_value: u8) -> Self {
        Self { data: vec![default_value; rows * cols], cols }
    }

    fn get(&self, row: usize, col: usize) -> u8 {
        self.data[row * self.cols + col]
    }

    fn set(&mut self, row: usize, col: usize, value: u8) {
        self.data[row * self.cols + col] = value;
    }
}

/// `StateMachine`.
pub struct StateMachine {
    states: Uint8Matrix,
    max_char_code: i32,
}

impl StateMachine {
    pub fn new(edges: &[Edge]) -> Self {
        let mut max_char_code: i32 = 0;
        let mut max_state = state::INVALID;
        for &(from, ch_code, to) in edges {
            if ch_code > max_char_code {
                max_char_code = ch_code;
            }
            if from > max_state {
                max_state = from;
            }
            if to > max_state {
                max_state = to;
            }
        }

        max_char_code += 1;
        let rows = max_state as usize + 1;

        let mut states = Uint8Matrix::new(rows, max_char_code as usize, state::INVALID);
        for &(from, ch_code, to) in edges {
            states.set(from as usize, ch_code as usize, to);
        }

        Self { states, max_char_code }
    }

    pub fn next_state(&self, current_state: State, ch_code: i32) -> State {
        if ch_code < 0 || ch_code >= self.max_char_code {
            return state::INVALID;
        }
        self.states.get(current_state as usize, ch_code as usize)
    }
}

fn edge(from: State, ch_code: u16, to: State) -> Edge {
    (from, ch_code as i32, to)
}

/// Máquina de estados de `http://`, `https://` e `file://`.
fn get_state_machine() -> &'static StateMachine {
    static MACHINE: OnceLock<StateMachine> = OnceLock::new();
    MACHINE.get_or_init(|| {
        StateMachine::new(&[
            edge(state::START, cc::h, state::H),
            edge(state::START, cc::H, state::H),
            edge(state::START, cc::f, state::F),
            edge(state::START, cc::F, state::F),
            edge(state::H, cc::t, state::HT),
            edge(state::H, cc::T, state::HT),
            edge(state::HT, cc::t, state::HTT),
            edge(state::HT, cc::T, state::HTT),
            edge(state::HTT, cc::p, state::HTTP),
            edge(state::HTT, cc::P, state::HTTP),
            edge(state::HTTP, cc::s, state::BEFORE_COLON),
            edge(state::HTTP, cc::S, state::BEFORE_COLON),
            edge(state::HTTP, cc::Colon, state::AFTER_COLON),
            edge(state::F, cc::i, state::FI),
            edge(state::F, cc::I, state::FI),
            edge(state::FI, cc::l, state::FIL),
            edge(state::FI, cc::L, state::FIL),
            edge(state::FIL, cc::e, state::BEFORE_COLON),
            edge(state::FIL, cc::E, state::BEFORE_COLON),
            edge(state::BEFORE_COLON, cc::Colon, state::AFTER_COLON),
            edge(state::AFTER_COLON, cc::Slash, state::ALMOST_THERE),
            edge(state::ALMOST_THERE, cc::Slash, state::END),
        ])
    })
}

/// `CharacterClass`.
type CharacterClass = u8;
const CLASS_NONE: CharacterClass = 0;
const CLASS_FORCE_TERMINATION: CharacterClass = 1;
const CLASS_CANNOT_END_IN: CharacterClass = 2;

fn get_classifier() -> &'static CharacterClassifier {
    static CLASSIFIER: OnceLock<CharacterClassifier> = OnceLock::new();
    CLASSIFIER.get_or_init(|| {
        let mut classifier = CharacterClassifier::new(CLASS_NONE);

        let force_termination_characters = " \t<>'\"、。｡､，．：；‘〈「『〔（［｛｢｣｝］）〕』」〉’｀～…|";
        for unit in force_termination_characters.encode_utf16() {
            classifier.set(unit as i32, CLASS_FORCE_TERMINATION);
        }

        for unit in ".,;:".encode_utf16() {
            classifier.set(unit as i32, CLASS_CANNOT_END_IN);
        }
        classifier
    })
}

/// `LinkComputer`.
pub struct LinkComputer;

impl LinkComputer {
    fn create_link(
        classifier: &CharacterClassifier,
        line: &[u16],
        line_number: i32,
        link_begin_index: usize,
        link_end_index: usize,
    ) -> ILink {
        // Não deixa o link terminar em certos caracteres.
        let mut last_included_char_index = link_end_index as isize - 1;
        loop {
            let ch_code = line[last_included_char_index as usize];
            let ch_class = classifier.get(ch_code as i32);
            if ch_class != CLASS_CANNOT_END_IN {
                break;
            }
            last_included_char_index -= 1;
            if last_included_char_index <= link_begin_index as isize {
                break;
            }
        }

        // Links entre parênteses, colchetes e chaves.
        if link_begin_index > 0 {
            let char_code_before_link = line[link_begin_index - 1];
            let last_char_code_in_link = line[last_included_char_index as usize];

            if (char_code_before_link == cc::OpenParen && last_char_code_in_link == cc::CloseParen)
                || (char_code_before_link == cc::OpenSquareBracket
                    && last_char_code_in_link == cc::CloseSquareBracket)
                || (char_code_before_link == cc::OpenCurlyBrace
                    && last_char_code_in_link == cc::CloseCurlyBrace)
            {
                // Não termina em `)` se `(` vem antes do início do link (idem `]` e `}`).
                last_included_char_index -= 1;
            }
        }

        ILink {
            range: Range {
                start_line_number: line_number,
                start_column: link_begin_index as i32 + 1,
                end_line_number: line_number,
                end_column: last_included_char_index as i32 + 2,
            },
            url: Some(LinkUrl::Text(JsString::from(
                &line[link_begin_index..(last_included_char_index + 1) as usize],
            ))),
            tooltip: None,
        }
    }

    pub fn compute_links(model: &dyn ILinkComputerTarget, state_machine: &StateMachine) -> Vec<ILink> {
        let classifier = get_classifier();

        let mut result: Vec<ILink> = Vec::new();
        let line_count = model.get_line_count();
        for i in 1..=line_count {
            let line_string = model.get_line_content(i);
            let line = line_string.units();
            let len = line.len();

            let mut j: usize = 0;
            let mut link_begin_index: usize = 0;
            let mut link_begin_ch_code: u16 = 0;
            let mut st = state::START;
            let mut has_open_parens = false;
            let mut has_open_square_bracket = false;
            let mut in_square_brackets = false;
            let mut has_open_curly_bracket = false;

            while j < len {
                let mut reset_state_machine = false;
                let ch_code = line[j];

                if st == state::ACCEPT {
                    let ch_class: CharacterClass = match ch_code {
                        cc::OpenParen => {
                            has_open_parens = true;
                            CLASS_NONE
                        }
                        cc::CloseParen => {
                            if has_open_parens { CLASS_NONE } else { CLASS_FORCE_TERMINATION }
                        }
                        cc::OpenSquareBracket => {
                            in_square_brackets = true;
                            has_open_square_bracket = true;
                            CLASS_NONE
                        }
                        cc::CloseSquareBracket => {
                            in_square_brackets = false;
                            if has_open_square_bracket { CLASS_NONE } else { CLASS_FORCE_TERMINATION }
                        }
                        cc::OpenCurlyBrace => {
                            has_open_curly_bracket = true;
                            CLASS_NONE
                        }
                        cc::CloseCurlyBrace => {
                            if has_open_curly_bracket { CLASS_NONE } else { CLASS_FORCE_TERMINATION }
                        }

                        // As três regras seguintes fazem `'`, `"` e `` ` `` serem aceitos dentro do link
                        // só se o link estiver cercado por outra aspa.
                        cc::SingleQuote | cc::DoubleQuote | cc::BackTick => {
                            if link_begin_ch_code == ch_code {
                                CLASS_FORCE_TERMINATION
                            } else if link_begin_ch_code == cc::SingleQuote
                                || link_begin_ch_code == cc::DoubleQuote
                                || link_begin_ch_code == cc::BackTick
                            {
                                CLASS_NONE
                            } else {
                                CLASS_FORCE_TERMINATION
                            }
                        }
                        // `*` termina o link se o link começou com `*`.
                        cc::Asterisk => {
                            if link_begin_ch_code == cc::Asterisk { CLASS_FORCE_TERMINATION } else { CLASS_NONE }
                        }
                        // Espaço é permitido entre `[` e `]`.
                        cc::Space => {
                            if in_square_brackets { CLASS_NONE } else { CLASS_FORCE_TERMINATION }
                        }
                        _ => classifier.get(ch_code as i32),
                    };

                    if ch_class == CLASS_FORCE_TERMINATION {
                        result.push(Self::create_link(classifier, line, i, link_begin_index, j));
                        reset_state_machine = true;
                    }
                } else if st == state::END {
                    let ch_class: CharacterClass = if ch_code == cc::OpenSquareBracket {
                        // A autoridade pode ter endereços IPv6, que contêm `[` e `]`.
                        has_open_square_bracket = true;
                        CLASS_NONE
                    } else {
                        classifier.get(ch_code as i32)
                    };

                    if ch_class == CLASS_FORCE_TERMINATION {
                        reset_state_machine = true;
                    } else {
                        st = state::ACCEPT;
                    }
                } else {
                    st = state_machine.next_state(st, ch_code as i32);
                    if st == state::INVALID {
                        reset_state_machine = true;
                    }
                }

                if reset_state_machine {
                    st = state::START;
                    has_open_parens = false;
                    has_open_square_bracket = false;
                    has_open_curly_bracket = false;

                    // Registra onde o link começou.
                    link_begin_index = j + 1;
                    link_begin_ch_code = ch_code;
                }

                j += 1;
            }

            if st == state::ACCEPT {
                result.push(Self::create_link(classifier, line, i, link_begin_index, len));
            }
        }

        result
    }
}

/// `computeLinks`: todos os links do documento. Operação cara, não deve rodar na thread da UI.
pub fn compute_links(model: Option<&dyn ILinkComputerTarget>) -> Vec<ILink> {
    match model {
        None => Vec::new(),
        Some(model) => LinkComputer::compute_links(model, get_state_machine()),
    }
}
