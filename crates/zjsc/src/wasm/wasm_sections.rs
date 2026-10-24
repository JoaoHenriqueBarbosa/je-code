//! Tradução de `wasm/WasmSections.h`.

/// `FOR_EACH_KNOWN_WASM_SECTION(macro)`: (nome, id, número de ordenação, descrição).
pub const KNOWN_SECTIONS: [(Section, u8, u32, &str); 13] = [
    (Section::Type, 1, 1, "Function signature declarations"),
    (Section::Import, 2, 2, "Import declarations"),
    (Section::Function, 3, 3, "Function declarations"),
    (Section::Table, 4, 4, "Indirect function table and other tables"),
    (Section::Memory, 5, 5, "Memory attributes"),
    (Section::Global, 6, 7, "Global declarations"),
    (Section::Export, 7, 8, "Exports"),
    (Section::Start, 8, 9, "Start function declaration"),
    (Section::Element, 9, 10, "Elements section"),
    (Section::Code, 10, 12, "Function bodies (code)"),
    (Section::Data, 11, 13, "Data segments"),
    (Section::DataCount, 12, 11, "Data count"),
    (Section::Exception, 13, 6, "Exception declarations"),
];

/// `enum class Section : uint8_t`. `Begin` é menor que toda seção real e `Custom` é maior; só
/// funciona porque os números das seções crescem monotonicamente. `Begin` não é uma seção real,
/// serve de marcador para validar a ordem.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Begin = 0,
    Type = 1,
    Import = 2,
    Function = 3,
    Table = 4,
    Memory = 5,
    Global = 6,
    Export = 7,
    Start = 8,
    Element = 9,
    Code = 10,
    Data = 11,
    DataCount = 12,
    Exception = 13,
    Custom = 14,
}

/// `orderingNumber`: a ordem exigida no módulo difere do id (DataCount vem antes de Code, Exception
/// antes de Global).
pub fn ordering_number(section: Section) -> u32 {
    KNOWN_SECTIONS
        .iter()
        .find(|(known, ..)| *known == section)
        .map_or(section as u32, |(_, _, ordering, _)| *ordering)
}

/// `isKnownSection`.
pub fn is_known_section(section: i64) -> bool {
    KNOWN_SECTIONS.iter().any(|(_, id, ..)| i64::from(*id) == section)
}

/// `decodeSection`: o byte 0 é `Custom`; um id desconhecido falha.
pub fn decode_section(section_byte: u8) -> Option<Section> {
    if section_byte == 0 {
        return Some(Section::Custom);
    }
    KNOWN_SECTIONS.iter().find(|(_, id, ..)| *id == section_byte).map(|(section, ..)| *section)
}

/// `validateOrder`.
pub fn validate_order(previous_known: Section, next: Section) -> bool {
    debug_assert!(is_known_section(i64::from(previous_known as u8)) || previous_known == Section::Begin);
    ordering_number(previous_known) < ordering_number(next)
}

/// `makeString(Section)`.
pub fn section_name(section: Section) -> &'static str {
    match section {
        Section::Begin => "Begin",
        Section::Custom => "Custom",
        Section::Type => "Type",
        Section::Import => "Import",
        Section::Function => "Function",
        Section::Table => "Table",
        Section::Memory => "Memory",
        Section::Global => "Global",
        Section::Export => "Export",
        Section::Start => "Start",
        Section::Element => "Element",
        Section::Code => "Code",
        Section::Data => "Data",
        Section::DataCount => "DataCount",
        Section::Exception => "Exception",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_follows_the_ordering_number() {
        assert!(validate_order(Section::Begin, Section::Type));
        // Exception (13) vem antes de Global (6) na ordem do módulo.
        assert!(validate_order(Section::Memory, Section::Exception));
        assert!(validate_order(Section::Exception, Section::Global));
        assert!(!validate_order(Section::Global, Section::Exception));
        // DataCount (12) vem entre Element e Code.
        assert!(validate_order(Section::Element, Section::DataCount));
        assert!(validate_order(Section::DataCount, Section::Code));
        assert!(!validate_order(Section::Type, Section::Type));
    }

    #[test]
    fn decode_rejects_unknown_ids() {
        assert_eq!(decode_section(0), Some(Section::Custom));
        assert_eq!(decode_section(13), Some(Section::Exception));
        assert_eq!(decode_section(14), None);
        assert_eq!(decode_section(0x7f), None);
    }
}
