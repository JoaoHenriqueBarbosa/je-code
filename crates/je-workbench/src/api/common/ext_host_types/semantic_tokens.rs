//! `SemanticTokensLegend`, `SemanticTokensBuilder`, `SemanticTokens`, `SemanticTokensEdit` e
//! `SemanticTokensEdits` de `extHostTypes.ts`.
//!
//! Divergências:
//! - As duas sobrecargas de `SemanticTokensBuilder#push` são [`SemanticTokensBuilder::push`] (números) e
//!   [`SemanticTokensBuilder::push_range`] (`Range`, tipo e modificadores por nome); o `illegalArgument()`
//!   de argumentos que não casam com nenhuma é garantido pelo tipo.
//! - `Uint32Array` é `Vec<u32>`. O armazenamento interno usa `i64`, porque os dados fora de ordem guardam
//!   valores absolutos e a conversão final `as u32` reproduz o módulo `2^32` do `Uint32Array`.

use std::collections::HashMap;

use je_base::errors::Error;

use super::position_range::Range;

/// `SemanticTokensLegend`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticTokensLegend {
    pub token_types: Vec<String>,
    pub token_modifiers: Vec<String>,
}

impl SemanticTokensLegend {
    /// `new SemanticTokensLegend(tokenTypes, tokenModifiers = [])`.
    pub fn new(token_types: Vec<String>, token_modifiers: Option<Vec<String>>) -> SemanticTokensLegend {
        SemanticTokensLegend { token_types, token_modifiers: token_modifiers.unwrap_or_default() }
    }
}

/// `SemanticTokens`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticTokens {
    pub result_id: Option<String>,
    pub data: Vec<u32>,
}

impl SemanticTokens {
    pub fn new(data: Vec<u32>, result_id: Option<String>) -> SemanticTokens {
        SemanticTokens { result_id, data }
    }
}

/// `SemanticTokensEdit`.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticTokensEdit {
    pub start: f64,
    pub delete_count: f64,
    pub data: Option<Vec<u32>>,
}

impl SemanticTokensEdit {
    pub fn new(start: f64, delete_count: f64, data: Option<Vec<u32>>) -> SemanticTokensEdit {
        SemanticTokensEdit { start, delete_count, data }
    }
}

/// `SemanticTokensEdits`.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticTokensEdits {
    pub result_id: Option<String>,
    pub edits: Vec<SemanticTokensEdit>,
}

impl SemanticTokensEdits {
    pub fn new(edits: Vec<SemanticTokensEdit>, result_id: Option<String>) -> SemanticTokensEdits {
        SemanticTokensEdits { result_id, edits }
    }
}

/// `SemanticTokensBuilder`: acumula tokens em qualquer ordem e os entrega ordenados e codificados por
/// diferença.
#[derive(Clone, Debug)]
pub struct SemanticTokensBuilder {
    prev_line: i64,
    prev_char: i64,
    data_is_sorted_and_delta_encoded: bool,
    data: Vec<i64>,
    token_type_str_to_int: HashMap<String, i64>,
    token_modifier_str_to_int: HashMap<String, i64>,
    has_legend: bool,
}

impl SemanticTokensBuilder {
    pub fn new(legend: Option<&SemanticTokensLegend>) -> SemanticTokensBuilder {
        let mut builder = SemanticTokensBuilder {
            prev_line: 0,
            prev_char: 0,
            data_is_sorted_and_delta_encoded: true,
            data: Vec::new(),
            token_type_str_to_int: HashMap::new(),
            token_modifier_str_to_int: HashMap::new(),
            has_legend: false,
        };
        if let Some(legend) = legend {
            builder.has_legend = true;
            for (i, token_type) in legend.token_types.iter().enumerate() {
                builder.token_type_str_to_int.insert(token_type.clone(), i as i64);
            }
            for (i, token_modifier) in legend.token_modifiers.iter().enumerate() {
                builder.token_modifier_str_to_int.insert(token_modifier.clone(), i as i64);
            }
        }
        builder
    }

    /// `push(line, char, length, tokenType, tokenModifiers = 0)`.
    pub fn push(&mut self, line: u32, char: u32, length: u32, token_type: u32, token_modifiers: Option<u32>) {
        self.push_encoded(i64::from(line), i64::from(char), i64::from(length), i64::from(token_type), i64::from(token_modifiers.unwrap_or(0)));
    }

    /// `push(range, tokenType, tokenModifiers?)`: exige a legenda do construtor.
    pub fn push_range(&mut self, range: &Range, token_type: &str, token_modifiers: Option<&[&str]>) -> Result<(), Error> {
        if !self.has_legend {
            return Err(Error::new("Legend must be provided in constructor"));
        }
        if range.start().line() != range.end().line() {
            return Err(Error::new("`range` cannot span multiple lines"));
        }
        let Some(&n_token_type) = self.token_type_str_to_int.get(token_type) else {
            return Err(Error::new("`tokenType` is not in the provided legend"));
        };
        let line = range.start().line() as i64;
        let char = range.start().character() as i64;
        let length = (range.end().character() - range.start().character()) as i64;
        let mut n_token_modifiers: u32 = 0;
        for token_modifier in token_modifiers.unwrap_or_default() {
            let Some(&n_token_modifier) = self.token_modifier_str_to_int.get(*token_modifier) else {
                return Err(Error::new("`tokenModifier` is not in the provided legend"));
            };
            // `(1 << n) >>> 0`: o deslocamento do JavaScript é módulo 32
            n_token_modifiers |= 1u32.wrapping_shl(n_token_modifier as u32);
        }
        self.push_encoded(line, char, length, n_token_type, i64::from(n_token_modifiers));
        Ok(())
    }

    fn push_encoded(&mut self, line: i64, char: i64, length: i64, token_type: i64, token_modifiers: i64) {
        if self.data_is_sorted_and_delta_encoded && (line < self.prev_line || (line == self.prev_line && char < self.prev_char)) {
            // as chamadas a `push` estavam em ordem e deixaram de estar
            self.data_is_sorted_and_delta_encoded = false;

            // tira a codificação por diferença dos dados
            let token_count = self.data.len() / 5;
            let mut prev_line = 0;
            let mut prev_char = 0;
            for i in 0..token_count {
                let mut line = self.data[5 * i];
                let mut char = self.data[5 * i + 1];

                if line == 0 {
                    // na mesma linha do token anterior
                    line = prev_line;
                    char += prev_char;
                } else {
                    // numa linha diferente da do token anterior
                    line += prev_line;
                }

                self.data[5 * i] = line;
                self.data[5 * i + 1] = char;

                prev_line = line;
                prev_char = char;
            }
        }

        let mut push_line = line;
        let mut push_char = char;
        if self.data_is_sorted_and_delta_encoded && !self.data.is_empty() {
            push_line -= self.prev_line;
            if push_line == 0 {
                push_char -= self.prev_char;
            }
        }

        self.data.extend_from_slice(&[push_line, push_char, length, token_type, token_modifiers]);

        self.prev_line = line;
        self.prev_char = char;
    }

    fn sort_and_delta_encode(data: &[i64]) -> Vec<u32> {
        let token_count = data.len() / 5;
        let mut pos: Vec<usize> = (0..token_count).collect();
        // `Array#sort` é estável, como o `sort_by`
        pos.sort_by(|&a, &b| (data[5 * a], data[5 * a + 1]).cmp(&(data[5 * b], data[5 * b + 1])));
        let mut result = vec![0u32; data.len()];
        let mut prev_line = 0;
        let mut prev_char = 0;
        for (i, &source) in pos.iter().enumerate() {
            let src_offset = 5 * source;
            let line = data[src_offset];
            let char = data[src_offset + 1];

            let push_line = line - prev_line;
            let push_char = if push_line == 0 { char - prev_char } else { char };

            let dst_offset = 5 * i;
            result[dst_offset] = push_line as u32;
            result[dst_offset + 1] = push_char as u32;
            result[dst_offset + 2] = data[src_offset + 2] as u32;
            result[dst_offset + 3] = data[src_offset + 3] as u32;
            result[dst_offset + 4] = data[src_offset + 4] as u32;

            prev_line = line;
            prev_char = char;
        }
        result
    }

    pub fn build(&self, result_id: Option<&str>) -> SemanticTokens {
        let data = if self.data_is_sorted_and_delta_encoded {
            self.data.iter().map(|&value| value as u32).collect()
        } else {
            SemanticTokensBuilder::sort_and_delta_encode(&self.data)
        };
        SemanticTokens::new(data, result_id.map(str::to_owned))
    }
}
