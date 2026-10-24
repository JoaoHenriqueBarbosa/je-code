//! Porte de `src/mbcssm/sjis.js`.

use crate::codingstatemachine::SmModel;
use crate::constants::{ERROR as E, ITS_ME as M, START as S};

const SJIS_CLS: [u8; 256] = [
    1,1,1,1,1,1,1,1,  // 00 - 07
    1,1,1,1,1,1,0,0,  // 08 - 0f
    1,1,1,1,1,1,1,1,  // 10 - 17
    1,1,1,0,1,1,1,1,  // 18 - 1f
    1,1,1,1,1,1,1,1,  // 20 - 27
    1,1,1,1,1,1,1,1,  // 28 - 2f
    1,1,1,1,1,1,1,1,  // 30 - 37
    1,1,1,1,1,1,1,1,  // 38 - 3f
    2,2,2,2,2,2,2,2,  // 40 - 47
    2,2,2,2,2,2,2,2,  // 48 - 4f
    2,2,2,2,2,2,2,2,  // 50 - 57
    2,2,2,2,2,2,2,2,  // 58 - 5f
    2,2,2,2,2,2,2,2,  // 60 - 67
    2,2,2,2,2,2,2,2,  // 68 - 6f
    2,2,2,2,2,2,2,2,  // 70 - 77
    2,2,2,2,2,2,2,1,  // 78 - 7f
    3,3,3,3,3,3,3,3,  // 80 - 87
    3,3,3,3,3,3,3,3,  // 88 - 8f
    3,3,3,3,3,3,3,3,  // 90 - 97
    3,3,3,3,3,3,3,3,  // 98 - 9f
    // 0xa0 is illegal in sjis encoding, but some pages does
    // contain such byte. We need to be more consts.error forgiven.
    2,2,2,2,2,2,2,2,  // a0 - a7
    2,2,2,2,2,2,2,2,  // a8 - af
    2,2,2,2,2,2,2,2,  // b0 - b7
    2,2,2,2,2,2,2,2,  // b8 - bf
    2,2,2,2,2,2,2,2,  // c0 - c7
    2,2,2,2,2,2,2,2,  // c8 - cf
    2,2,2,2,2,2,2,2,  // d0 - d7
    2,2,2,2,2,2,2,2,  // d8 - df
    3,3,3,3,3,3,3,3,  // e0 - e7
    3,3,3,3,3,4,4,4,  // e8 - ef
    3,3,3,3,3,3,3,3,  // f0 - f7
    3,3,3,3,3,0,0,0,  // f8 - ff
];

const SJIS_ST: [u8; 24] = [
    E,S,S,3,E,E,E,E, //00-07
    E,E,E,E,M,M,M,M, //08-0f
    M,M,E,E,S,S,S,S, //10-17
];

const SJIS_CHAR_LEN_TABLE: [u8; 6] = [0, 1, 1, 2, 0, 0];

pub static SJIS_SM_MODEL: SmModel = SmModel {
    class_table: &SJIS_CLS,
    class_factor: 6,
    state_table: &SJIS_ST,
    char_len_table: &SJIS_CHAR_LEN_TABLE,
    name: "Shift_JIS",
};
