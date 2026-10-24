//! Porte de `src/chardistribution.js`: análise da distribuição de frequência dos caracteres de dois bytes.

use crate::big5freq::{BIG5_CHAR_TO_FREQ_ORDER, BIG5_TABLE_SIZE, BIG5_TYPICAL_DISTRIBUTION_RATIO};
use crate::euckrfreq::{EUCKR_CHAR_TO_FREQ_ORDER, EUCKR_TABLE_SIZE, EUCKR_TYPICAL_DISTRIBUTION_RATIO};
use crate::euctwfreq::{EUCTW_CHAR_TO_FREQ_ORDER, EUCTW_TABLE_SIZE, EUCTW_TYPICAL_DISTRIBUTION_RATIO};
use crate::gb2312freq::{GB2312_CHAR_TO_FREQ_ORDER, GB2312_TABLE_SIZE, GB2312_TYPICAL_DISTRIBUTION_RATIO};
use crate::jisfreq::{JIS_CHAR_TO_FREQ_ORDER, JIS_TABLE_SIZE, JIS_TYPICAL_DISTRIBUTION_RATIO};

const ENOUGH_DATA_THRESHOLD: usize = 1024;
const SURE_YES: f64 = 0.99;
const SURE_NO: f64 = 0.01;
const MINIMUM_DATA_THRESHOLD: usize = 3;

pub struct CharDistributionAnalysis {
    char_to_freq_order: &'static [u16], // Mapping table to get frequency order from char order (get from GetOrder())
    table_size: usize,                  // Size of above table
    // This is a constant value which varies from language to language, used in calculating confidence.
    // See http://www.mozilla.org/projects/intl/UniversalCharsetDetection.html for further detail.
    typical_distribution_ratio: f64,
    /// O `getOrder` da subclasse: `-1` (ou `NaN` no original) quando o par de bytes não é um caractere válido.
    get_order: fn(&[u8]) -> i64,
    total_chars: usize, // Total characters encountered
    freq_chars: usize,  // The number of characters whose frequency order is less than 512
}

impl CharDistributionAnalysis {
    fn new(char_to_freq_order: &'static [u16], table_size: usize, typical_distribution_ratio: f64, get_order: fn(&[u8]) -> i64) -> Self {
        Self { char_to_freq_order, table_size, typical_distribution_ratio, get_order, total_chars: 0, freq_chars: 0 }
    }

    /// `EUCTWDistributionAnalysis`.
    pub fn euctw() -> Self {
        Self::new(&EUCTW_CHAR_TO_FREQ_ORDER, EUCTW_TABLE_SIZE, EUCTW_TYPICAL_DISTRIBUTION_RATIO, |window| match (window.first(), window.get(1)) {
            (Some(&b0), Some(&b1)) if b0 >= 0xC4 => 94 * (i64::from(b0) - 0xC4) + i64::from(b1) - 0xA1,
            _ => -1,
        })
    }

    /// `EUCKRDistributionAnalysis`.
    pub fn euckr() -> Self {
        Self::new(&EUCKR_CHAR_TO_FREQ_ORDER, EUCKR_TABLE_SIZE, EUCKR_TYPICAL_DISTRIBUTION_RATIO, |window| match (window.first(), window.get(1)) {
            (Some(&b0), Some(&b1)) if b0 >= 0xB0 => 94 * (i64::from(b0) - 0xB0) + i64::from(b1) - 0xA1,
            _ => -1,
        })
    }

    /// `GB2312DistributionAnalysis`.
    pub fn gb2312() -> Self {
        Self::new(&GB2312_CHAR_TO_FREQ_ORDER, GB2312_TABLE_SIZE, GB2312_TYPICAL_DISTRIBUTION_RATIO, |window| match (window.first(), window.get(1)) {
            (Some(&b0), Some(&b1)) if b0 >= 0xB0 && b1 >= 0xA1 => 94 * (i64::from(b0) - 0xB0) + i64::from(b1) - 0xA1,
            _ => -1,
        })
    }

    /// `Big5DistributionAnalysis`.
    pub fn big5() -> Self {
        Self::new(&BIG5_CHAR_TO_FREQ_ORDER, BIG5_TABLE_SIZE, BIG5_TYPICAL_DISTRIBUTION_RATIO, |window| match (window.first(), window.get(1)) {
            (Some(&b0), Some(&b1)) if b0 >= 0xA4 => {
                if b1 >= 0xA1 {
                    157 * (i64::from(b0) - 0xA4) + i64::from(b1) - 0xA1 + 63
                } else {
                    157 * (i64::from(b0) - 0xA4) + i64::from(b1) - 0x40
                }
            }
            _ => -1,
        })
    }

    /// `SJISDistributionAnalysis`.
    pub fn sjis() -> Self {
        Self::new(&JIS_CHAR_TO_FREQ_ORDER, JIS_TABLE_SIZE, JIS_TYPICAL_DISTRIBUTION_RATIO, |window| {
            let Some(&b0) = window.first() else { return -1 };
            let mut order = if (0x81..=0x9F).contains(&b0) {
                188 * (i64::from(b0) - 0x81)
            } else if (0xE0..=0xEF).contains(&b0) {
                188 * (i64::from(b0) - 0xE0 + 31)
            } else {
                return -1;
            };
            let Some(&b1) = window.get(1) else { return -1 };
            order += i64::from(b1) - 0x40;
            if b1 < 0x40 || b1 == 0x7F || b1 > 0xFC {
                order = -1;
            }
            order
        })
    }

    /// `EUCJPDistributionAnalysis`.
    pub fn eucjp() -> Self {
        Self::new(&JIS_CHAR_TO_FREQ_ORDER, JIS_TABLE_SIZE, JIS_TYPICAL_DISTRIBUTION_RATIO, |window| match (window.first(), window.get(1)) {
            (Some(&b0), Some(&b1)) if b0 >= 0xA0 => 94 * (i64::from(b0) - 0xA1) + i64::from(b1) - 0xA1,
            _ => -1,
        })
    }

    /// reset analyser, clear any state
    pub fn reset(&mut self) {
        self.total_chars = 0;
        self.freq_chars = 0;
    }

    /// feed a character with known length
    pub fn feed(&mut self, window: &[u8], char_len: usize) {
        let order = if char_len == 2 { (self.get_order)(window) } else { -1 };
        if order >= 0 {
            self.total_chars += 1;
            if (order as usize) < self.table_size && self.char_to_freq_order.get(order as usize).is_some_and(|&freq| 512 > freq) {
                self.freq_chars += 1;
            }
        }
    }

    /// return confidence based on existing data
    pub fn get_confidence(&self) -> f64 {
        if self.total_chars == 0 || self.freq_chars <= MINIMUM_DATA_THRESHOLD {
            return SURE_NO;
        }
        if self.total_chars != self.freq_chars {
            let r = self.freq_chars as f64 / ((self.total_chars - self.freq_chars) as f64 * self.typical_distribution_ratio);
            if r < SURE_YES {
                return r;
            }
        }

        SURE_YES
    }

    pub fn got_enough_data(&self) -> bool {
        self.total_chars > ENOUGH_DATA_THRESHOLD
    }
}
