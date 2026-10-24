//! Porte de `src/euctwprober.js`.

use crate::chardistribution::CharDistributionAnalysis;
use crate::mbcharsetprober::MultiByteCharSetProber;
use crate::mbcssm::euctw::EUCTW_SM_MODEL;

pub fn euctw_prober() -> MultiByteCharSetProber {
    MultiByteCharSetProber::new("EUC-TW", &EUCTW_SM_MODEL, CharDistributionAnalysis::euctw(), None)
}
