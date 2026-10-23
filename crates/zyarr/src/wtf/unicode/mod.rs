//! Porte de `WTF/wtf/unicode`, só o que o YARR usa.
pub mod case_mapping;
pub mod case_mapping_tables;
pub mod character_names;
pub mod properties_tables;
pub mod utf8_conversion;

/// `u_charType`: a categoria geral com os valores do enum `UCharCategory`.
pub fn char_category(c: u32) -> u8 {
    let runs = properties_tables::GENERAL_CATEGORY_RUNS;
    runs[runs.partition_point(|&(start, _)| start <= c) - 1].1
}
