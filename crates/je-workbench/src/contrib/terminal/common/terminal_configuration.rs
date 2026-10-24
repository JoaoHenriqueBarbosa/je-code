//! Porte parcial de `src/vs/workbench/contrib/terminal/common/terminalConfiguration.ts`: o dicionário `terminalConfiguration`
//! com a configuração `terminal.integrated.allowedLinkSchemes` (a única que a suíte `Terminal configuration` lê, e a que
//! carrega a restrição de segurança `restricted: true`).
//!
//! O resto do arquivo (as demais cerca de 120 propriedades, `registerTerminalConfiguration`, `defaultTerminalFontSize` e a
//! migração `terminal.integrated.enableBell`) depende de peças ainda não portadas e entra com elas:
//! `terminalColorSchema` e `terminalIconSchema` (`platform/terminal/common/terminalPlatformConfiguration.ts`),
//! `terminalContribConfiguration` e `TerminalContribSettingId` (`terminal/terminalContribExports.ts` e a configuração de
//! cada `terminalContrib`), as constantes de `terminal/common/terminal.ts`, o `product.quality` e o registro de migrações de
//! configuração (`IConfigurationMigrationRegistry`).
//!
//! Divergências:
//! - **`IStringDictionary<IConfigurationPropertySchema>`** é a lista de pares `(chave, propriedade)` na ordem do original;
//!   cada propriedade é uma [`PropertySchema`] (esquema como objeto dinâmico, ver `registry.rs`). O dicionário é criado uma
//!   vez por thread, e quem o consulta recebe a mesma propriedade (o objeto compartilhado do módulo do original).

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::configuration::registry::PropertySchema;
use je_platform::terminal::terminal_settings::TerminalSettingId;

thread_local! {
    static TERMINAL_CONFIGURATION: Vec<(&'static str, PropertySchema)> = vec![(
        TerminalSettingId::AllowedLinkSchemes.as_str(),
        PropertySchema::from_entries([
            ("restricted", Dynamic::Bool(true)),
            (
                "description",
                Dynamic::string(&localize(
                    "terminal.integrated.allowedLinkSchemes",
                    "An array of strings containing the URI schemes that the terminal is allowed to open links for. By default, only a small subset of possible schemes are allowed for security reasons.",
                    &[],
                )),
            ),
            ("type", Dynamic::string("array")),
            ("items", Dynamic::object([("type", Dynamic::string("string"))])),
            (
                "default",
                Dynamic::array(["file", "http", "https", "mailto", "vscode", "vscode-insiders"].map(Dynamic::string)),
            ),
        ]),
    )];
}

/// `terminalConfiguration`: os pares `(chave, propriedade)` na ordem do original.
pub fn terminal_configuration() -> Vec<(&'static str, PropertySchema)> {
    TERMINAL_CONFIGURATION.with(Clone::clone)
}

/// `terminalConfiguration[key]`.
pub fn terminal_configuration_property(key: &str) -> Option<PropertySchema> {
    TERMINAL_CONFIGURATION.with(|entries| entries.iter().find(|(entry_key, _)| *entry_key == key).map(|(_, property)| property.clone()))
}
