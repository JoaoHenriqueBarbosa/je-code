//! Porte de `src/mbcharsetprober.js`. O mesmo prober serve às cinco codificações de `big5prober.js`,
//! `eucjpprober.js`, `euckrprober.js`, `euctwprober.js`, `gb2312prober.js` e `sjisprober.js`: as duas
//! japonesas só acrescentam o analisador de contexto, que o `feed` e o `getConfidence` do original
//! repetiam quase sem diferença.

use crate::Error;
use crate::chardistribution::CharDistributionAnalysis;
use crate::charsetprober::{CharsetProber, js_slice};
use crate::codingstatemachine::{CodingStateMachine, SmModel};
use crate::constants::{DETECTING, ERROR, FOUND_IT, ITS_ME, NOT_ME, SHORTCUT_THRESHOLD, START};
use crate::jpcntx::JapaneseContextAnalysis;

pub struct MultiByteCharSetProber {
    charset_name: &'static str,
    state: u8,
    coding_sm: CodingStateMachine,
    distribution_analyzer: CharDistributionAnalysis,
    context_analyzer: Option<JapaneseContextAnalysis>,
    last_char: [u8; 2],
}

impl MultiByteCharSetProber {
    pub fn new(
        charset_name: &'static str,
        sm_model: &'static SmModel,
        distribution_analyzer: CharDistributionAnalysis,
        context_analyzer: Option<JapaneseContextAnalysis>,
    ) -> Self {
        let mut prober = Self {
            charset_name,
            state: DETECTING,
            coding_sm: CodingStateMachine::new(sm_model),
            distribution_analyzer,
            context_analyzer,
            last_char: [0, 0],
        };
        prober.reset();
        prober
    }
}

impl CharsetProber for MultiByteCharSetProber {
    fn reset(&mut self) {
        self.state = DETECTING;
        self.coding_sm.reset();
        self.distribution_analyzer.reset();
        if let Some(context_analyzer) = &mut self.context_analyzer {
            context_analyzer.reset();
        }
        self.last_char = [0, 0];
    }

    fn get_charset_name(&self) -> Option<String> {
        Some(self.charset_name.to_string())
    }

    fn feed(&mut self, buf: &[u8]) -> Result<u8, Error> {
        for i in 0..buf.len() {
            let coding_state = self.coding_sm.next_state(buf[i]);
            if coding_state == ERROR {
                self.state = NOT_ME;
                break;
            } else if coding_state == ITS_ME {
                self.state = FOUND_IT;
                break;
            } else if coding_state == START {
                let char_len = self.coding_sm.get_current_char_len();
                if i == 0 {
                    self.last_char[1] = buf[0];
                    if let Some(context_analyzer) = &mut self.context_analyzer {
                        context_analyzer.feed_from_last_char(&self.last_char, char_len)?;
                    }
                    self.distribution_analyzer.feed(&self.last_char, char_len);
                } else {
                    if let Some(context_analyzer) = &mut self.context_analyzer {
                        context_analyzer.feed_from_buffer(buf, i, char_len)?;
                    }
                    self.distribution_analyzer.feed(js_slice(buf, i as i64 - 1, Some(i as i64 + 1)), char_len);
                }
            }
        }

        if let Some(&last) = buf.last() {
            self.last_char[0] = last;
        }

        if self.state == DETECTING {
            let got_enough_data = match &self.context_analyzer {
                Some(context_analyzer) => context_analyzer.got_enough_data(),
                None => self.distribution_analyzer.got_enough_data(),
            };
            if got_enough_data && self.get_confidence() > SHORTCUT_THRESHOLD {
                self.state = FOUND_IT;
            }
        }

        Ok(self.state)
    }

    fn get_state(&self) -> u8 {
        self.state
    }

    fn get_confidence(&self) -> f64 {
        let distribution_confidence = self.distribution_analyzer.get_confidence();
        match &self.context_analyzer {
            Some(context_analyzer) => context_analyzer.get_confidence().max(distribution_confidence),
            None => distribution_confidence,
        }
    }
}
