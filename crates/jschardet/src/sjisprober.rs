//! Porte de `src/sjisprober.js`.

use crate::chardistribution::CharDistributionAnalysis;
use crate::jpcntx::JapaneseContextAnalysis;
use crate::mbcharsetprober::MultiByteCharSetProber;
use crate::mbcssm::sjis::SJIS_SM_MODEL;

pub fn sjis_prober() -> MultiByteCharSetProber {
    MultiByteCharSetProber::new("SHIFT_JIS", &SJIS_SM_MODEL, CharDistributionAnalysis::sjis(), Some(JapaneseContextAnalysis::sjis()))
}
