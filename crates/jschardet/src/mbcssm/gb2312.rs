//! Porte de `src/mbcssm/gb2312.js`.

use crate::codingstatemachine::SmModel;
use crate::constants::{ERROR as E, ITS_ME as M, START as S};

const GB2312_CLS: [u8; 256] = [
    1,1,1,1,1,1,1,1,  // 00 - 07
    1,1,1,1,1,1,0,0,  // 08 - 0f
    1,1,1,1,1,1,1,1,  // 10 - 17
    1,1,1,0,1,1,1,1,  // 18 - 1f
    1,1,1,1,1,1,1,1,  // 20 - 27
    1,1,1,1,1,1,1,1,  // 28 - 2f
    3,3,3,3,3,3,3,3,  // 30 - 37
    3,3,1,1,1,1,1,1,  // 38 - 3f
    2,2,2,2,2,2,2,2,  // 40 - 47
    2,2,2,2,2,2,2,2,  // 48 - 4f
    2,2,2,2,2,2,2,2,  // 50 - 57
    2,2,2,2,2,2,2,2,  // 58 - 5f
    2,2,2,2,2,2,2,2,  // 60 - 67
    2,2,2,2,2,2,2,2,  // 68 - 6f
    2,2,2,2,2,2,2,2,  // 70 - 77
    2,2,2,2,2,2,2,4,  // 78 - 7f
    5,6,6,6,6,6,6,6,  // 80 - 87
    6,6,6,6,6,6,6,6,  // 88 - 8f
    6,6,6,6,6,6,6,6,  // 90 - 97
    6,6,6,6,6,6,6,6,  // 98 - 9f
    6,6,6,6,6,6,6,6,  // a0 - a7
    6,6,6,6,6,6,6,6,  // a8 - af
    6,6,6,6,6,6,6,6,  // b0 - b7
    6,6,6,6,6,6,6,6,  // b8 - bf
    6,6,6,6,6,6,6,6,  // c0 - c7
    6,6,6,6,6,6,6,6,  // c8 - cf
    6,6,6,6,6,6,6,6,  // d0 - d7
    6,6,6,6,6,6,6,6,  // d8 - df
    6,6,6,6,6,6,6,6,  // e0 - e7
    6,6,6,6,6,6,6,6,  // e8 - ef
    6,6,6,6,6,6,6,6,  // f0 - f7
    6,6,6,6,6,6,6,0,  // f8 - ff
];

const GB2312_ST: [u8; 48] = [
    E,S,S,S,S,S,3,E, //00-07
    E,E,E,E,E,E,M,M, //08-0f
    M,M,M,M,M,E,E,S, //10-17
    4,E,S,S,E,E,E,E, //18-1f
    E,E,5,E,E,E,M,E, //20-27
    E,E,S,S,S,S,S,S, //28-2f
];

// To be accurate, the length of class 6 can be either 2 or 4.
// But it is not necessary to discriminate between the two since
// it is used for frequency analysis only, and we are validing
// each code range there as well. So it is safe to set it to be
// 2 here.
const GB2312_CHAR_LEN_TABLE: [u8; 7] = [0, 1, 1, 1, 1, 1, 2];

pub static GB2312_SM_MODEL: SmModel = SmModel {
    class_table: &GB2312_CLS,
    class_factor: 7,
    state_table: &GB2312_ST,
    char_len_table: &GB2312_CHAR_LEN_TABLE,
    name: "GB2312",
};
