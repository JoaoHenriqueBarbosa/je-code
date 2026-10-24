//! Porte de `src/escprober.js`.

use crate::Error;
use crate::charsetprober::CharsetProber;
use crate::codingstatemachine::CodingStateMachine;
use crate::constants::{DETECTING, ERROR, FOUND_IT, ITS_ME, NOT_ME};
use crate::escsm::{HZ_SM_MODEL, ISO2022CN_SM_MODEL, ISO2022JP_SM_MODEL, ISO2022KR_SM_MODEL};

struct EscMachine {
    coding_sm: CodingStateMachine,
    active: bool,
}

pub struct EscCharSetProber {
    state: u8,
    coding_sms: Vec<EscMachine>,
    active_sm: isize,
    detected_charset: Option<&'static str>,
}

impl EscCharSetProber {
    pub fn new() -> Self {
        let coding_sms = [&HZ_SM_MODEL, &ISO2022CN_SM_MODEL, &ISO2022JP_SM_MODEL, &ISO2022KR_SM_MODEL]
            .into_iter()
            .map(|model| EscMachine { coding_sm: CodingStateMachine::new(model), active: true })
            .collect::<Vec<_>>();
        let mut prober = Self { state: DETECTING, active_sm: coding_sms.len() as isize, coding_sms, detected_charset: None };
        prober.reset();
        prober
    }
}

impl Default for EscCharSetProber {
    fn default() -> Self {
        Self::new()
    }
}

impl CharsetProber for EscCharSetProber {
    fn reset(&mut self) {
        self.state = DETECTING;
        for machine in &mut self.coding_sms {
            machine.active = true;
            machine.coding_sm.reset();
        }
        self.active_sm = self.coding_sms.len() as isize;
        self.detected_charset = None;
    }

    fn get_charset_name(&self) -> Option<String> {
        self.detected_charset.map(str::to_string)
    }

    fn get_supported_charset_names(&self) -> Vec<String> {
        self.coding_sms.iter().map(|machine| machine.coding_sm.get_coding_state_machine().to_string()).collect()
    }

    fn get_confidence(&self) -> f64 {
        if self.detected_charset.is_some() { 0.99 } else { 0.00 }
    }

    fn feed(&mut self, buf: &[u8]) -> Result<u8, Error> {
        for &c in buf {
            for machine in &mut self.coding_sms {
                if !machine.active {
                    continue;
                }
                let coding_state = machine.coding_sm.next_state(c);
                if coding_state == ERROR {
                    machine.active = false;
                    self.active_sm -= 1;
                    if self.active_sm <= 0 {
                        self.state = NOT_ME;
                        return Ok(self.state);
                    }
                } else if coding_state == ITS_ME {
                    self.state = FOUND_IT;
                    self.detected_charset = Some(machine.coding_sm.get_coding_state_machine());
                    return Ok(self.state);
                }
            }
        }

        Ok(self.state)
    }

    fn get_state(&self) -> u8 {
        self.state
    }
}
