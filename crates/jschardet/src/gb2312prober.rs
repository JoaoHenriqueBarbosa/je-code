//! Porte de `src/gb2312prober.js`.

use crate::chardistribution::CharDistributionAnalysis;
use crate::mbcharsetprober::MultiByteCharSetProber;
use crate::mbcssm::gb2312::GB2312_SM_MODEL;

pub fn gb2312_prober() -> MultiByteCharSetProber {
    MultiByteCharSetProber::new("GB2312", &GB2312_SM_MODEL, CharDistributionAnalysis::gb2312(), None)
}
