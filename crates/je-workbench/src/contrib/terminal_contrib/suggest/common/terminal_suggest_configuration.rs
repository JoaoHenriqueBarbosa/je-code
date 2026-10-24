//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/suggest/common/terminalSuggestConfiguration.ts`:
//! os identificadores das configurações ([`TerminalSuggestSettingId`]), as extensões executáveis padrão do
//! Windows, o nome da seção e o registro dinâmico da configuração dos provedores
//! ([`register_terminal_suggest_providers_configuration`]). O restante do arquivo (a interface
//! `ITerminalSuggestConfiguration` e o nó estático `terminalSuggestConfiguration`) não é lido pelo serviço de
//! completions e fica para o porte da configuração.
//!
//! Divergências:
//! - **`let terminalSuggestProvidersConfiguration`** (a variável do módulo) é um `thread_local!`, como o registro de
//!   configuração do porte.
//! - **`registerTerminalSuggestProvidersConfiguration()` ao carregar o módulo.** O Rust não executa código ao carregar um
//!   módulo: quem monta o registro de configuração do terminal chama a função sem argumentos, como o original faz ao importar.
//! - **`providers?: Map<string, ITerminalSuggestProviderInfo>`** é um `Option<BTreeMap<..>>` (as chaves são ordenadas de
//!   qualquer jeito). O original acrescenta `lsp` ao `Map` recebido; aqui o mapa é do chamador por valor, então o acréscimo
//!   não é visível fora da função.

use std::cell::RefCell;
use std::collections::BTreeMap;

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, PropertySchema, configuration_registry};

/// `ITerminalSuggestProviderInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITerminalSuggestProviderInfo {
    pub id: String,
    pub description: Option<String>,
}

/// `TerminalSuggestSettingId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalSuggestSettingId {
    Enabled,
    QuickSuggestions,
    SuggestOnTriggerCharacters,
    RunOnEnter,
    WindowsExecutableExtensions,
    Providers,
    ShowStatusBar,
    CdPath,
    InlineSuggestion,
    UpArrowNavigatesHistory,
    SelectionMode,
    InsertTrailingSpace,
}

impl TerminalSuggestSettingId {
    /// O valor de texto do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "terminal.integrated.suggest.enabled",
            Self::QuickSuggestions => "terminal.integrated.suggest.quickSuggestions",
            Self::SuggestOnTriggerCharacters => "terminal.integrated.suggest.suggestOnTriggerCharacters",
            Self::RunOnEnter => "terminal.integrated.suggest.runOnEnter",
            Self::WindowsExecutableExtensions => "terminal.integrated.suggest.windowsExecutableExtensions",
            Self::Providers => "terminal.integrated.suggest.providers",
            Self::ShowStatusBar => "terminal.integrated.suggest.showStatusBar",
            Self::CdPath => "terminal.integrated.suggest.cdPath",
            Self::InlineSuggestion => "terminal.integrated.suggest.inlineSuggestion",
            Self::UpArrowNavigatesHistory => "terminal.integrated.suggest.upArrowNavigatesHistory",
            Self::SelectionMode => "terminal.integrated.suggest.selectionMode",
            Self::InsertTrailingSpace => "terminal.integrated.suggest.insertTrailingSpace",
        }
    }
}

/// `windowsDefaultExecutableExtensions`.
pub const WINDOWS_DEFAULT_EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe", // Executable file
    "bat", // Batch file
    "cmd", // Command script
    "com", // Command file
    "msi", // Windows Installer package
    "ps1", // PowerShell script
    "vbs", // VBScript file
    "js",  // JScript file
    "jar", // Java Archive (requires Java runtime)
    "py",  // Python script (requires Python interpreter)
    "rb",  // Ruby script (requires Ruby interpreter)
    "pl",  // Perl script (requires Perl interpreter)
    "sh",  // Shell script (via WSL or third-party tools)
];

/// `terminalSuggestConfigSection`.
pub const TERMINAL_SUGGEST_CONFIG_SECTION: &str = "terminal.integrated.suggest";

thread_local! {
    /// `let terminalSuggestProvidersConfiguration: IConfigurationNode | undefined`.
    static TERMINAL_SUGGEST_PROVIDERS_CONFIGURATION: RefCell<Option<ConfigurationNode>> = const { RefCell::new(None) };
}

/// `registerTerminalSuggestProvidersConfiguration(providers?)`.
pub fn register_terminal_suggest_providers_configuration(providers: Option<BTreeMap<String, ITerminalSuggestProviderInfo>>) {
    let old_providers_configuration = TERMINAL_SUGGEST_PROVIDERS_CONFIGURATION.with(|configuration| configuration.borrow().clone());

    let mut providers = providers.unwrap_or_default();
    providers.entry("lsp".to_owned()).or_insert_with(|| ITerminalSuggestProviderInfo {
        id: "lsp".to_owned(),
        description: Some(localize("suggest.provider.lsp.description", "Show suggestions from language servers.", &[])),
    });

    // `Array.from(providers.keys()).sort()`: o `BTreeMap` já itera em ordem.
    let mut providers_properties: Vec<(String, Dynamic)> = Vec::new();
    let mut default_value: Vec<(String, Dynamic)> = Vec::new();
    for (id, info) in &providers {
        let default = id != "lsp";
        let description = info.description.clone().unwrap_or_else(|| localize("suggest.provider.title", "Show suggestions from {0}.", &[id.as_str()]));
        providers_properties.push((
            id.clone(),
            Dynamic::object([("type", Dynamic::string("boolean")), ("default", Dynamic::Bool(default)), ("description", Dynamic::string(&description))]),
        ));
        default_value.push((id.clone(), Dynamic::Bool(default)));
    }

    let terminal_suggest_providers_configuration = ConfigurationNode::new(
        ConfigurationNodeData {
            id: Some("terminalSuggestProviders".to_owned()),
            order: Some(100.0),
            title: Some(localize("terminalSuggestProvidersConfigurationTitle", "Terminal Suggest Providers", &[])),
            type_: Some(Dynamic::string("object")),
            ..Default::default()
        }
        .with_properties([(
            TerminalSuggestSettingId::Providers.as_str(),
            PropertySchema::from_entries([
                ("restricted", Dynamic::Bool(true)),
                (
                    "markdownDescription",
                    Dynamic::string(&localize(
                        "suggest.providersEnabledByDefault",
                        "Controls which suggestions automatically show up while typing. Suggestion providers are enabled by default.",
                        &[],
                    )),
                ),
                ("type", Dynamic::string("object")),
                ("properties", Dynamic::object(providers_properties)),
                ("default", Dynamic::object(default_value)),
                ("tags", Dynamic::array([Dynamic::string("preview")])),
                ("additionalProperties", Dynamic::Bool(false)),
            ]),
        )]),
    );
    TERMINAL_SUGGEST_PROVIDERS_CONFIGURATION.with(|configuration| *configuration.borrow_mut() = Some(terminal_suggest_providers_configuration.clone()));

    configuration_registry().update_configurations(
        std::slice::from_ref(&terminal_suggest_providers_configuration),
        old_providers_configuration.as_slice(),
    );
}
