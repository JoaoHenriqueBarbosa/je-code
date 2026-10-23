//! Porte de `src/vs/platform/action/common/action.ts`: os tipos que descrevem uma ação de comando.
//!
//! Divergências: `localize2` (do `nls.ts`) vive aqui até o `je_base::nls` ter o equivalente
//! (TODO(integração)); `ISerializableCommandAction` (`UriDto<ICommandAction>`) não é portado, porque o
//! dado não atravessa fronteira de processo neste porte.

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;

use crate::commands::CommandMetadata;
use crate::contextkey::ContextKeyExpression;

/// `ILocalizedString`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LocalizedString {
    /// O valor localizado.
    pub value: String,
    /// O valor original (não localizado).
    pub original: String,
}

/// `localize2(key, message)`: o valor localizado e o original.
pub fn localize2(key: &str, message: &str) -> LocalizedString {
    LocalizedString { value: localize(key, message, &[]), original: message.to_string() }
}

/// `isLocalizedString`: um objeto com `original` e `value` de texto.
pub fn is_localized_string(thing: &Dynamic) -> bool {
    matches!(thing, Dynamic::Object(_))
        && matches!(thing.get_str("original"), Dynamic::String(_))
        && matches!(thing.get_str("value"), Dynamic::String(_))
}

/// `ICommandActionTitle`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CommandActionTitle {
    pub value: String,
    pub original: String,
    /// O título com a designação do mnemônico (`&&` precede o mnemônico).
    pub mnemonic_title: Option<String>,
}

/// `string | ICommandActionTitle`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Title {
    Plain(String),
    Localized(CommandActionTitle),
}

impl From<&str> for Title {
    fn from(value: &str) -> Self {
        Title::Plain(value.to_string())
    }
}

/// `Icon`: `{ dark?, light? } | ThemeIcon`.
#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    Uris { dark: Option<URI>, light: Option<URI> },
    Theme(ThemeIcon),
}

/// `ICommandActionToggleInfo`.
#[derive(Clone, Debug)]
pub struct CommandActionToggleInfo {
    /// A condição que marca a ação como alternada.
    pub condition: ContextKeyExpression,
    pub icon: Option<Icon>,
    pub tooltip: Option<String>,
    /// O título que combina com uma marca de seleção.
    pub title: Option<String>,
    /// Como `title`, com a designação do mnemônico.
    pub mnemonic_title: Option<String>,
}

/// `ContextKeyExpression | ICommandActionToggleInfo`.
#[derive(Clone, Debug)]
pub enum Toggled {
    Expression(ContextKeyExpression),
    Info(CommandActionToggleInfo),
}

/// `isICommandActionToggleInfo`.
pub fn is_command_action_toggle_info(thing: Option<&Toggled>) -> bool {
    matches!(thing, Some(Toggled::Info(_)))
}

/// `ICommandActionSource`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandActionSource {
    pub id: String,
    pub title: String,
}

/// `keyof typeof Categories | ILocalizedString | string`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Category {
    /// Uma chave de `Categories` (`View`, `Help`...) ou um texto qualquer: o original os distingue só pelo tipo.
    Text(String),
    Localized(LocalizedString),
}

impl From<&str> for Category {
    fn from(value: &str) -> Self {
        Category::Text(value.to_string())
    }
}

/// `string | ILocalizedString`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tooltip {
    Plain(String),
    Localized(LocalizedString),
}

/// `ICommandAction`.
#[derive(Clone)]
pub struct CommandAction {
    pub id: String,
    pub title: Title,
    pub short_title: Option<Title>,
    /// Metadados do comando: usados na API, em atalhos sem outra interface e na busca da paleta.
    pub metadata: Option<CommandMetadata>,
    pub category: Option<Category>,
    pub tooltip: Option<Tooltip>,
    pub icon: Option<Icon>,
    pub source: Option<CommandActionSource>,
    /// Controla se a ação está habilitada (item de menu em cinza, comando que não pode ser invocado).
    pub precondition: Option<ContextKeyExpression>,
    /// A ação é alternável: a expressão que reflete o estado, ou as informações de alternância.
    pub toggled: Option<Toggled>,
}

impl CommandAction {
    pub fn new(id: impl Into<String>, title: impl Into<Title>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            short_title: None,
            metadata: None,
            category: None,
            tooltip: None,
            icon: None,
            source: None,
            precondition: None,
            toggled: None,
        }
    }
}
