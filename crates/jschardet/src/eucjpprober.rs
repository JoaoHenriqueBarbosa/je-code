//! Porte de `src/eucjpprober.js`.

use crate::chardistribution::CharDistributionAnalysis;
use crate::jpcntx::JapaneseContextAnalysis;
use crate::mbcharsetprober::MultiByteCharSetProber;
use crate::mbcssm::eucjp::EUCJP_SM_MODEL;

pub fn eucjp_prober() -> MultiByteCharSetProber {
    MultiByteCharSetProber::new("EUC-JP", &EUCJP_SM_MODEL, CharDistributionAnalysis::eucjp(), Some(JapaneseContextAnalysis::euc_jp()))
}
