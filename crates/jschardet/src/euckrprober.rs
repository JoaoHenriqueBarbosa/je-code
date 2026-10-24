//! Porte de `src/euckrprober.js`.

use crate::chardistribution::CharDistributionAnalysis;
use crate::mbcharsetprober::MultiByteCharSetProber;
use crate::mbcssm::euckr::EUCKR_SM_MODEL;

pub fn euckr_prober() -> MultiByteCharSetProber {
    MultiByteCharSetProber::new("EUC-KR", &EUCKR_SM_MODEL, CharDistributionAnalysis::euckr(), None)
}
