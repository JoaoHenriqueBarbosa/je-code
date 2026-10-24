//! Porte de `src/hebrewprober.js`.
//!
//! Este prober não reconhece língua nem codificação sozinho. Ele é o auxiliar dos dois
//! `SingleByteCharSetProber` do hebraico (o lógico, `windows-1255`, e o visual, `ISO-8859-8`, que compartilham
//! o modelo `Win1255HebrewModel`): conta letras finais para decidir entre os dois nomes. O texto longo sobre
//! hebraico lógico e visual do original continua valendo: as letras finais (kaf, mem, nun, pe e tsadi)
//! aparecem no fim da palavra no texto lógico e no começo no visual, e o nome sai desse placar combinado com
//! a confiança dos dois probers de modelo.

use std::cell::RefCell;
use std::rc::Weak;

use crate::Error;
use crate::charsetprober::{CharsetProber, filter_high_bit_only};
use crate::constants::{DETECTING, NOT_ME};
use crate::sbcharsetprober::SingleByteCharSetProber;

// windows-1255 / ISO-8859-8 code points of interest
const FINAL_KAF: u8 = 0xea;
const NORMAL_KAF: u8 = 0xeb;
const FINAL_MEM: u8 = 0xed;
const NORMAL_MEM: u8 = 0xee;
const FINAL_NUN: u8 = 0xef;
const NORMAL_NUN: u8 = 0xf0;
const FINAL_PE: u8 = 0xf3;
const NORMAL_PE: u8 = 0xf4;
const FINAL_TSADI: u8 = 0xf5;

// Minimum Visual vs Logical final letter score difference.
// If the difference is below this, don't rely solely on the final letter score distance.
const MIN_FINAL_CHAR_DISTANCE: i64 = 5;

// Minimum Visual vs Logical model score difference.
// If the difference is below this, don't rely at all on the model score distance.
const MIN_MODEL_DISTANCE: f64 = 0.01;

const VISUAL_HEBREW_NAME: &str = "ISO-8859-8";
const LOGICAL_HEBREW_NAME: &str = "windows-1255";

type ModelProber = Weak<RefCell<SingleByteCharSetProber>>;

pub struct HebrewProber {
    logical_prober: Option<ModelProber>,
    visual_prober: Option<ModelProber>,
    final_char_logical_score: i64,
    final_char_visual_score: i64,
    // The two last characters seen in the previous buffer,
    // mPrev and mBeforePrev are initialized to space in order to simulate a word
    // delimiter at the beginning of the data
    prev: u8,
    before_prev: u8,
}

impl HebrewProber {
    pub fn new() -> Self {
        Self { logical_prober: None, visual_prober: None, final_char_logical_score: 0, final_char_visual_score: 0, prev: b' ', before_prev: b' ' }
    }

    /// These probers are owned by the group prober.
    pub fn set_model_probers(&mut self, logical_prober: ModelProber, visual_prober: ModelProber) {
        self.logical_prober = Some(logical_prober);
        self.visual_prober = Some(visual_prober);
    }

    fn is_final(c: u8) -> bool {
        [FINAL_KAF, FINAL_MEM, FINAL_NUN, FINAL_PE, FINAL_TSADI].contains(&c)
    }

    fn is_non_final(c: u8) -> bool {
        // The normal Tsadi is not a good Non-Final letter due to words like
        // 'lechotet' (to chat) containing an apostrophe after the tsadi. This
        // apostrophe is converted to a space in FilterWithoutEnglishLetters causing
        // the Non-Final tsadi to appear at an end of a word even though this is not
        // the case in the original text.
        // The letters Pe and Kaf rarely display a related behavior of not being a
        // good Non-Final letter. Words like 'Pop', 'Winamp' and 'Mubarak' for
        // example legally end with a Non-Final Pe or Kaf. However, the benefit of
        // these letters as Non-Final letters outweighs the damage since these words
        // are quite rare.
        [NORMAL_KAF, NORMAL_MEM, NORMAL_NUN, NORMAL_PE].contains(&c)
    }

    fn model_confidence(prober: &Option<ModelProber>) -> f64 {
        prober.as_ref().and_then(Weak::upgrade).map_or(0.0, |prober| prober.borrow().get_confidence())
    }

    fn model_state(prober: &Option<ModelProber>) -> u8 {
        prober.as_ref().and_then(Weak::upgrade).map_or(DETECTING, |prober| prober.borrow().get_state())
    }
}

impl Default for HebrewProber {
    fn default() -> Self {
        Self::new()
    }
}

impl CharsetProber for HebrewProber {
    fn reset(&mut self) {
        self.final_char_logical_score = 0;
        self.final_char_visual_score = 0;
        self.prev = b' ';
        self.before_prev = b' ';
    }

    fn feed(&mut self, buf: &[u8]) -> Result<u8, Error> {
        // Final letter analysis for logical-visual decision.
        // Look for evidence that the received buffer is either logical Hebrew or
        // visual Hebrew.
        // The following cases are checked:
        // 1) A word longer than 1 letter, ending with a final letter. This is an
        //    indication that the text is laid out "naturally" since the final letter
        //    really appears at the end. +1 for logical score.
        // 2) A word longer than 1 letter, ending with a Non-Final letter. In normal
        //    Hebrew, words ending with Kaf, Mem, Nun, Pe or Tsadi, should not end with
        //    the Non-Final form of that letter. Exceptions to this rule are mentioned
        //    above in isNonFinal(). This is an indication that the text is laid out
        //    backwards. +1 for visual score
        // 3) A word longer than 1 letter, starting with a final letter. Final letters
        //    should not appear at the beginning of a word. This is an indication that
        //    the text is laid out backwards. +1 for visual score.
        //
        // The visual score and logical score are accumulated throughout the text and
        // are finally checked against each other in GetCharSetName().
        // No checking for final letters in the middle of words is done since that case
        // is not an indication for either Logical or Visual text.
        //
        // We automatically filter out all 7-bit characters (replace them with spaces)
        // so the word boundary detection works properly. [MAP]

        if self.get_state() == NOT_ME {
            // Both model probers say it's not them. No reason to continue.
            return Ok(NOT_ME);
        }

        for cur in filter_high_bit_only(buf) {
            if cur == b' ' {
                // We stand on a space - a word just ended
                if self.before_prev != b' ' {
                    // next-to-last char was not a space so self._mPrev is not a 1 letter word
                    if Self::is_final(self.prev) {
                        // case (1) [-2:not space][-1:final letter][cur:space]
                        self.final_char_logical_score += 1;
                    } else if Self::is_non_final(self.prev) {
                        // case (2) [-2:not space][-1:Non-Final letter][cur:space]
                        self.final_char_visual_score += 1;
                    }
                }
            } else if self.before_prev == b' ' && Self::is_final(self.prev) {
                // Not standing on a space
                // case (3) [-2:space][-1:final letter][cur:not space]
                self.final_char_visual_score += 1;
            }
            self.before_prev = self.prev;
            self.prev = cur;
        }
        // Forever detecting, till the end or until both model probers return eNotMe (handled above)
        Ok(DETECTING)
    }

    fn get_charset_name(&self) -> Option<String> {
        // Make the decision: is it Logical or Visual?
        // If the final letter score distance is dominant enough, rely on it.
        let finalsub = self.final_char_logical_score - self.final_char_visual_score;
        if finalsub >= MIN_FINAL_CHAR_DISTANCE {
            return Some(LOGICAL_HEBREW_NAME.to_string());
        }
        if finalsub <= -MIN_FINAL_CHAR_DISTANCE {
            return Some(VISUAL_HEBREW_NAME.to_string());
        }

        // It's not dominant enough, try to rely on the model scores instead.
        let modelsub = Self::model_confidence(&self.logical_prober) - Self::model_confidence(&self.visual_prober);
        if modelsub > MIN_MODEL_DISTANCE {
            return Some(LOGICAL_HEBREW_NAME.to_string());
        }
        if modelsub < -MIN_MODEL_DISTANCE {
            return Some(VISUAL_HEBREW_NAME.to_string());
        }

        // Still no good, back to final letter distance, maybe it'll save the day.
        if finalsub < 0 {
            return Some(VISUAL_HEBREW_NAME.to_string());
        }

        // (finalsub > 0 - Logical) or (don't know what to do) default to Logical.
        Some(LOGICAL_HEBREW_NAME.to_string())
    }

    fn get_state(&self) -> u8 {
        // Remain active as long as any of the model probers are active.
        if Self::model_state(&self.logical_prober) == NOT_ME && Self::model_state(&self.visual_prober) == NOT_ME {
            return NOT_ME;
        }
        DETECTING
    }

    fn get_confidence(&self) -> f64 {
        0.0
    }
}
