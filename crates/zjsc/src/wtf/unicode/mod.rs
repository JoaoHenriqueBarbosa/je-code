//! Porte de `WTF/wtf/unicode`.
pub mod character_names;
pub mod utf8_conversion;
pub mod case_mapping;
pub mod case_mapping_tables;
pub mod normalization;
pub mod char_direction_table;

/// `u_charDirection` do ICU: a classe bidirecional de `c`, com os valores do enum
/// `UCharDirection`.
pub fn char_direction(c: u32) -> u8 {
    let runs = char_direction_table::CHAR_DIRECTION_RUNS;
    let i = runs.partition_point(|&(start, _)| start <= c);
    runs[i - 1].1
}
pub mod properties_tables;

fn in_ranges(table: &[(u32, u32)], c: u32) -> bool {
    let i = table.partition_point(|&(_, end)| end < c);
    i < table.len() && table[i].0 <= c
}

/// `u_hasBinaryProperty(c, UCHAR_ID_START)`.
pub fn is_id_start(c: u32) -> bool {
    in_ranges(properties_tables::ID_START, c)
}

/// `u_hasBinaryProperty(c, UCHAR_ID_CONTINUE)`.
pub fn is_id_continue(c: u32) -> bool {
    in_ranges(properties_tables::ID_CONTINUE, c)
}

/// `u_charType`: a categoria geral com os valores do enum `UCharCategory`.
pub fn char_category(c: u32) -> u8 {
    let runs = properties_tables::GENERAL_CATEGORY_RUNS;
    runs[runs.partition_point(|&(start, _)| start <= c) - 1].1
}
