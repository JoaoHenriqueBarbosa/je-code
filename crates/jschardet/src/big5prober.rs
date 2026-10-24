//! Porte de `src/big5prober.js`.

use crate::chardistribution::CharDistributionAnalysis;
use crate::mbcharsetprober::MultiByteCharSetProber;
use crate::mbcssm::big5::BIG5_SM_MODEL;

pub fn big5_prober() -> MultiByteCharSetProber {
    MultiByteCharSetProber::new("Big5", &BIG5_SM_MODEL, CharDistributionAnalysis::big5(), None)
}
