//! Porte de `src/sbcharsetprober.js`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::Error;
use crate::charsetprober::{CharsetProber, filter_without_english_letters};
use crate::constants::{DETECTING, NOT_ME};
use crate::hebrewprober::HebrewProber;

/// O objeto-modelo de `src/lang*model.js`.
pub struct SbModel {
    pub char_to_order_map: &'static [u8],
    pub precedence_matrix: &'static [u8],
    pub typical_positive_ratio: f64,
    pub keep_english_letter: bool,
    pub charset_name: &'static str,
}

const SAMPLE_SIZE: u8 = 64;
const SB_ENOUGH_REL_THRESHOLD: usize = 1024;
const NEGATIVE_SHORTCUT_THRESHOLD: f64 = 0.05;
const SYMBOL_CAT_ORDER: u8 = 250;
const NUMBER_OF_SEQ_CAT: usize = 4;
const POSITIVE_CAT: usize = NUMBER_OF_SEQ_CAT - 1;

pub struct SingleByteCharSetProber {
    state: u8,
    model: &'static SbModel,
    reversed: bool, // "true" if we need to reverse every pair in the model lookup
    name_prober: Option<Rc<RefCell<HebrewProber>>>, // Optional auxiliary prober for name decision
    last_order: u8, // char order of last character
    seq_counters: [usize; NUMBER_OF_SEQ_CAT],
    total_seqs: usize,
    total_char: usize,
    freq_char: usize, // characters that fall in our sampling range
}

impl SingleByteCharSetProber {
    pub fn new(model: &'static SbModel, reversed: bool, name_prober: Option<Rc<RefCell<HebrewProber>>>) -> Self {
        let mut prober = Self {
            state: DETECTING,
            model,
            reversed,
            name_prober,
            last_order: 255,
            seq_counters: [0; NUMBER_OF_SEQ_CAT],
            total_seqs: 0,
            total_char: 0,
            freq_char: 0,
        };
        prober.reset();
        prober
    }
}

impl CharsetProber for SingleByteCharSetProber {
    fn reset(&mut self) {
        self.state = DETECTING;
        self.last_order = 255;
        self.seq_counters = [0; NUMBER_OF_SEQ_CAT];
        self.total_seqs = 0;
        self.total_char = 0;
        self.freq_char = 0;
    }

    fn get_charset_name(&self) -> Option<String> {
        match &self.name_prober {
            Some(name_prober) => name_prober.borrow().get_charset_name(),
            None => Some(self.model.charset_name.to_string()),
        }
    }

    fn feed(&mut self, buf: &[u8]) -> Result<u8, Error> {
        let filtered;
        let buf = if self.model.keep_english_letter {
            buf
        } else {
            filtered = filter_without_english_letters(buf);
            &filtered
        };
        if buf.is_empty() {
            return Ok(self.state);
        }
        for &c in buf {
            let order = self.model.char_to_order_map[usize::from(c)];
            if order < SYMBOL_CAT_ORDER {
                self.total_char += 1;
            }
            if order < SAMPLE_SIZE {
                self.freq_char += 1;
                if self.last_order < SAMPLE_SIZE {
                    self.total_seqs += 1;
                    let sample = usize::from(SAMPLE_SIZE);
                    let index = if !self.reversed {
                        usize::from(self.last_order) * sample + usize::from(order)
                    } else {
                        // reverse the order of the letters in the lookup
                        usize::from(order) * sample + usize::from(self.last_order)
                    };
                    self.seq_counters[usize::from(self.model.precedence_matrix[index])] += 1;
                }
            }
            self.last_order = order;
        }

        if self.state == DETECTING && self.total_seqs > SB_ENOUGH_REL_THRESHOLD {
            // Acima de `POSITIVE_SHORTCUT_THRESHOLD` (0.95) o original só registra no log ("we have a winner").
            if self.get_confidence() < NEGATIVE_SHORTCUT_THRESHOLD {
                self.state = NOT_ME;
            }
        }

        Ok(self.state)
    }

    fn get_state(&self) -> u8 {
        self.state
    }

    fn get_confidence(&self) -> f64 {
        let mut r = 0.01;
        if self.total_seqs > 0 {
            r = (1.0 * self.seq_counters[POSITIVE_CAT] as f64) / self.total_seqs as f64 / self.model.typical_positive_ratio;
            r *= self.freq_char as f64 / self.total_char as f64;
            if r >= 1.0 {
                r = 0.99;
            }
        }
        r
    }
}
