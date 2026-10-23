//! Porte de `src/vs/platform/notification/common/notificationLegacy.ts`.

/// `LegacyExtensionLinkParsing` (`typeof legacyExtensionLinkParsing`): a capacidade restrita às pontes de
/// notificação e progresso das extensões. O `Symbol` do original vira um tipo sem valores públicos além da
/// constante [`LEGACY_EXTENSION_LINK_PARSING`]; a identidade é o próprio tipo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyExtensionLinkParsing(());

/// `legacyExtensionLinkParsing`.
pub const LEGACY_EXTENSION_LINK_PARSING: LegacyExtensionLinkParsing = LegacyExtensionLinkParsing(());

/// `isLegacyExtensionLinkParsing`: confere a identidade, não a forma nem a veracidade. O tipo já garante que
/// só a capacidade chega aqui; o que o original rejeita em tempo de execução (`true`, `{}`, outro `Symbol`)
/// não é representável.
pub fn is_legacy_extension_link_parsing(value: Option<&LegacyExtensionLinkParsing>) -> bool {
    value.is_some_and(|v| *v == LEGACY_EXTENSION_LINK_PARSING)
}
