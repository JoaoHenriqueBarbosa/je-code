//! Porte de `src/latin1prober.js`.

use crate::Error;
use crate::charsetprober::{CharsetProber, remove_xml_tags};
use crate::constants::{DETECTING, NOT_ME};

const UDF: u8 = 0; // undefined
const OTH: u8 = 1; // other
const ASC: u8 = 2; // ascii capital letter
const ASS: u8 = 3; // ascii small letter
const ACV: u8 = 4; // accent capital vowel
const ACO: u8 = 5; // accent capital other
const ASV: u8 = 6; // accent small vowel
const ASO: u8 = 7; // accent small other

const LATIN1_CHAR_TO_CLASS: [u8; 256] = [
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // 00 - 07
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // 08 - 0F
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // 10 - 17
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // 18 - 1F
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // 20 - 27
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // 28 - 2F
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // 30 - 37
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // 38 - 3F
    OTH, ASC, ASC, ASC, ASC, ASC, ASC, ASC,   // 40 - 47
    ASC, ASC, ASC, ASC, ASC, ASC, ASC, ASC,   // 48 - 4F
    ASC, ASC, ASC, ASC, ASC, ASC, ASC, ASC,   // 50 - 57
    ASC, ASC, ASC, OTH, OTH, OTH, OTH, OTH,   // 58 - 5F
    OTH, ASS, ASS, ASS, ASS, ASS, ASS, ASS,   // 60 - 67
    ASS, ASS, ASS, ASS, ASS, ASS, ASS, ASS,   // 68 - 6F
    ASS, ASS, ASS, ASS, ASS, ASS, ASS, ASS,   // 70 - 77
    ASS, ASS, ASS, OTH, OTH, OTH, OTH, OTH,   // 78 - 7F
    OTH, UDF, OTH, ASO, OTH, OTH, OTH, OTH,   // 80 - 87
    OTH, OTH, ACO, OTH, ACO, UDF, ACO, UDF,   // 88 - 8F
    UDF, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // 90 - 97
    OTH, OTH, ASO, OTH, ASO, UDF, ASO, ACO,   // 98 - 9F
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // A0 - A7
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // A8 - AF
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // B0 - B7
    OTH, OTH, OTH, OTH, OTH, OTH, OTH, OTH,   // B8 - BF
    ACV, ACV, ACV, ACV, ACV, ACV, ACO, ACO,   // C0 - C7
    ACV, ACV, ACV, ACV, ACV, ACV, ACV, ACV,   // C8 - CF
    ACO, ACO, ACV, ACV, ACV, ACV, ACV, OTH,   // D0 - D7
    ACV, ACV, ACV, ACV, ACV, ACO, ACO, ACO,   // D8 - DF
    ASV, ASV, ASV, ASV, ASV, ASV, ASO, ASO,   // E0 - E7
    ASV, ASV, ASV, ASV, ASV, ASV, ASV, ASV,   // E8 - EF
    ASO, ASO, ASV, ASV, ASV, ASV, ASV, OTH,   // F0 - F7
    ASV, ASV, ASV, ASV, ASV, ASO, ASO, ASO,   // F8 - FF
];

// 0 : illegal
// 1 : very unlikely
// 2 : normal
// 3 : very likely
const LATIN1_CLASS_MODEL: [u8; 64] = [
// UDF OTH ASC ASS ACV ACO ASV ASO
   0,  0,  0,  0,  0,  0,  0,  0,  // UDF
   0,  3,  3,  3,  3,  3,  3,  3,  // OTH
   0,  3,  3,  3,  3,  3,  3,  3,  // ASC
   0,  3,  3,  3,  1,  1,  3,  3,  // ASS
   0,  3,  3,  3,  1,  2,  1,  2,  // ACV
   0,  3,  3,  3,  3,  3,  3,  3,  // ACO
   0,  3,  1,  3,  1,  1,  1,  3,  // ASV
   0,  3,  1,  3,  1,  1,  3,  3,  // ASO
];

const FREQ_CAT_NUM: usize = 4;
const CLASS_NUM: usize = 8; // total classes

pub struct Latin1Prober {
    state: u8,
    last_char_class: u8,
    freq_counter: [usize; FREQ_CAT_NUM],
}

impl Latin1Prober {
    pub fn new() -> Self {
        Self { state: DETECTING, last_char_class: OTH, freq_counter: [0; FREQ_CAT_NUM] }
    }
}

impl Default for Latin1Prober {
    fn default() -> Self {
        Self::new()
    }
}

impl CharsetProber for Latin1Prober {
    fn reset(&mut self) {
        self.last_char_class = OTH;
        self.freq_counter = [0; FREQ_CAT_NUM];
        self.state = DETECTING;
    }

    fn get_charset_name(&self) -> Option<String> {
        Some("windows-1252".to_string())
    }

    fn feed(&mut self, buf: &[u8]) -> Result<u8, Error> {
        for c in remove_xml_tags(buf) {
            let char_class = LATIN1_CHAR_TO_CLASS[usize::from(c)];
            let freq = LATIN1_CLASS_MODEL[usize::from(self.last_char_class) * CLASS_NUM + usize::from(char_class)];
            if freq == 0 {
                self.state = NOT_ME;
                break;
            }
            self.freq_counter[usize::from(freq)] += 1;
            self.last_char_class = char_class;
        }

        Ok(self.state)
    }

    fn get_state(&self) -> u8 {
        self.state
    }

    fn get_confidence(&self) -> f64 {
        if self.state == NOT_ME {
            return 0.01;
        }

        let total: usize = self.freq_counter.iter().sum();
        let mut confidence = if (total as f64) < 0.01 {
            0.0
        } else {
            (self.freq_counter[3] as f64 / total as f64) - (self.freq_counter[1] as f64 * 20.0 / total as f64)
        };
        if confidence < 0.0 {
            confidence = 0.0;
        }
        // lower the confidence of latin1 so that other more accurate detector
        // can take priority.
        //
        // antonio.afonso: need to change this otherwise languages like pt, es, fr using latin1 will never be detected.
        confidence * 0.95
    }
}
