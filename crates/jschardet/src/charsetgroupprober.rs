//! Porte de `src/charsetgroupprober.js`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::Error;
use crate::charsetprober::CharsetProber;
use crate::constants::{DETECTING, FOUND_IT, NOT_ME};

/// Um prober que pode ser membro de um grupo e, no caso do hebraico, referenciado por outro prober.
pub type SharedProber = Rc<RefCell<dyn CharsetProber>>;

/// Um prober no formato que o grupo guarda.
pub fn share(prober: impl CharsetProber + 'static) -> SharedProber {
    Rc::new(RefCell::new(prober))
}

struct Member {
    prober: SharedProber,
    active: bool,
}

pub struct CharsetGroupProber {
    state: u8,
    active_num: isize,
    members: Vec<Member>,
    best_guess_prober: Cell<Option<usize>>,
    supported_charset_names: Vec<String>,
}

impl CharsetGroupProber {
    /// O `_mProbers` do grupo e os nomes que ele suporta (o `getCharsetName` de cada membro, no momento da
    /// construção), seguidos do `reset()` que o construtor de cada grupo do original faz.
    pub fn new(probers: Vec<SharedProber>) -> Self {
        let supported_charset_names = probers.iter().filter_map(|prober| prober.borrow().get_charset_name()).collect();
        let members = probers.into_iter().map(|prober| Member { prober, active: false }).collect();
        let mut group = Self { state: DETECTING, active_num: 0, members, best_guess_prober: Cell::new(None), supported_charset_names };
        group.reset();
        group
    }
}

impl CharsetProber for CharsetGroupProber {
    fn reset(&mut self) {
        self.state = DETECTING;
        self.active_num = 0;
        for member in &mut self.members {
            member.prober.borrow_mut().reset();
            member.active = true;
            self.active_num += 1;
        }
        self.best_guess_prober.set(None);
    }

    fn get_charset_name(&self) -> Option<String> {
        if self.best_guess_prober.get().is_none() {
            self.get_confidence();
        }
        let best = self.best_guess_prober.get()?;
        self.members[best].prober.borrow().get_charset_name()
    }

    fn get_supported_charset_names(&self) -> Vec<String> {
        self.supported_charset_names.clone()
    }

    fn feed(&mut self, buf: &[u8]) -> Result<u8, Error> {
        for (index, member) in self.members.iter_mut().enumerate() {
            if !member.active {
                continue;
            }
            let st = member.prober.borrow_mut().feed(buf)?;
            if st == 0 {
                continue;
            }
            if st == FOUND_IT {
                self.best_guess_prober.set(Some(index));
                return Ok(self.state);
            } else if st == NOT_ME {
                member.active = false;
                self.active_num -= 1;
                if self.active_num <= 0 {
                    self.state = NOT_ME;
                    return Ok(self.state);
                }
            }
        }
        Ok(self.state)
    }

    fn get_confidence(&self) -> f64 {
        let st = self.get_state();
        if st == FOUND_IT {
            return 0.99;
        } else if st == NOT_ME {
            return 0.01;
        }
        let mut best_conf = 0.0;
        self.best_guess_prober.set(None);
        for (index, member) in self.members.iter().enumerate() {
            if !member.active {
                continue;
            }
            let cf = member.prober.borrow().get_confidence();
            if best_conf < cf {
                best_conf = cf;
                self.best_guess_prober.set(Some(index));
            }
        }
        if self.best_guess_prober.get().is_none() {
            return 0.0;
        }
        best_conf
    }

    fn get_state(&self) -> u8 {
        self.state
    }
}
