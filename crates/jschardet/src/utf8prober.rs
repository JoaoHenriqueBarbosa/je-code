//! Porte de `src/utf8prober.js`.

use crate::Error;
use crate::charsetprober::CharsetProber;
use crate::codingstatemachine::CodingStateMachine;
use crate::constants::{DETECTING, ERROR, FOUND_IT, ITS_ME, NOT_ME, SHORTCUT_THRESHOLD, START};
use crate::mbcssm::utf8::UTF8_SM_MODEL;

const ONE_CHAR_PROB: f64 = 0.5;

pub struct UTF8Prober {
    state: u8,
    coding_sm: CodingStateMachine,
    num_of_mb_char: usize,
    mb_char_len: usize,
    full_len: usize,
    basic_ascii_len: usize,
}

impl UTF8Prober {
    pub fn new() -> Self {
        let mut prober =
            Self { state: DETECTING, coding_sm: CodingStateMachine::new(&UTF8_SM_MODEL), num_of_mb_char: 0, mb_char_len: 0, full_len: 0, basic_ascii_len: 0 };
        prober.reset();
        prober
    }
}

impl Default for UTF8Prober {
    fn default() -> Self {
        Self::new()
    }
}

impl CharsetProber for UTF8Prober {
    fn reset(&mut self) {
        self.state = DETECTING;
        self.coding_sm.reset();
        self.num_of_mb_char = 0;
        self.mb_char_len = 0;
        self.full_len = 0;
        self.basic_ascii_len = 0;
    }

    fn get_charset_name(&self) -> Option<String> {
        Some("UTF-8".to_string())
    }

    fn feed(&mut self, buf: &[u8]) -> Result<u8, Error> {
        self.full_len += buf.len();
        for &c in buf {
            let coding_state = self.coding_sm.next_state(c);
            if coding_state == ERROR {
                self.state = NOT_ME;
                break;
            } else if coding_state == ITS_ME {
                self.state = FOUND_IT;
                break;
            } else if coding_state == START {
                if self.coding_sm.get_current_char_len() >= 2 {
                    self.num_of_mb_char += 1;
                    self.mb_char_len += self.coding_sm.get_current_char_len();
                } else if c < 128 {
                    // codes higher than 127 are extended ASCII
                    self.basic_ascii_len += 1;
                }
            }
        }

        if self.state == DETECTING && self.get_confidence() > SHORTCUT_THRESHOLD {
            self.state = FOUND_IT;
        }

        Ok(self.state)
    }

    fn get_state(&self) -> u8 {
        self.state
    }

    fn get_confidence(&self) -> f64 {
        let mut unlike = 0.99;
        let mut mb_char_ratio = 0.0;
        let non_basic_ascii_len = self.full_len - self.basic_ascii_len;
        if non_basic_ascii_len > 0 {
            mb_char_ratio = self.mb_char_len as f64 / non_basic_ascii_len as f64;
        }
        if self.num_of_mb_char < 6 && mb_char_ratio <= 0.6 {
            unlike *= ONE_CHAR_PROB.powi(self.num_of_mb_char as i32);
            1.0 - unlike
        } else {
            unlike
        }
    }
}
