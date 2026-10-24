//! `extHostTypeConverters.ts`: `IconPath`, a conversão entre o `vscode.IconPath` e o `IconPathDto` do protocolo.
//!
//! Divergências:
//! - O original aceita, por razões históricas, textos no lugar de URIs no `IconPath` (`'/path/to/icon.png'`,
//!   `{ dark: '/dark.png' }`), e um objeto qualquer que o tipo não prevê. A entrada de [`IconPath::from`] é o
//!   [`IconPathInput`], que tem esses casos: [`IconPathInput::Path`] e o [`IconPathInput::Object`] (um objeto com os campos
//!   `light` e `dark` opcionais, cada um [`UriOrPath`]). Um objeto sem a chave `dark` (o `{ foo: 'bar' }` e o `{ light }` do
//!   original) é um `Object` com `dark` ausente. Um `Path` vazio é falso e vira `None`, como o `!value` do original.
//! - `IconPath.to` devolve o [`IconPath`](crate::api::common::ext_host_types::IconPath) de `ext_host_types`. O
//!   `isUriComponents(value)` e o `URI.revive` do original são o próprio [`URI`], pois um `UriComponents` já é um [`URI`] aqui.
//! - `fromThemeIcon` devolve o próprio `ThemeIcon`, que é um `languages.IconPath`.

use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_platform::resources::uri_file;

use crate::api::common::ext_host_protocol::IconPathDto;
use crate::api::common::ext_host_types as types;

/// `Uri | string`: um dos lados de `{ light, dark }` na entrada.
#[derive(Clone, Debug)]
pub enum UriOrPath {
    Uri(URI),
    Path(String),
}

/// O que `IconPath.from` recebe: um `vscode.IconPath` e as formas históricas que ele tolera.
#[derive(Clone, Debug)]
pub enum IconPathInput {
    ThemeIcon(ThemeIcon),
    Uri(URI),
    Path(String),
    /// Um objeto: só vale com `dark`; `light` ausente repete `dark`.
    Object { light: Option<UriOrPath>, dark: Option<UriOrPath> },
}

impl UriOrPath {
    /// `typeof value === 'string' ? URI.file(value) : value`.
    fn into_uri(self) -> URI {
        match self {
            UriOrPath::Uri(uri) => uri,
            UriOrPath::Path(path) => uri_file(&path),
        }
    }
}

/// `typeConverters.IconPath`.
pub struct IconPath;

impl IconPath {
    /// `IconPath.fromThemeIcon`.
    pub fn from_theme_icon(icon_path: ThemeIcon) -> ThemeIcon {
        icon_path
    }

    /// `IconPath.from`.
    pub fn from(value: Option<IconPathInput>) -> Option<IconPathDto> {
        match value? {
            IconPathInput::ThemeIcon(icon) => Some(IconPathDto::ThemeIcon(icon)),
            IconPathInput::Uri(uri) => Some(IconPathDto::Uri(uri)),
            IconPathInput::Path(path) if path.is_empty() => None,
            IconPathInput::Path(path) => Some(IconPathDto::Uri(uri_file(&path))),
            IconPathInput::Object { light, dark } => {
                let dark = dark?.into_uri();
                let light = light.map(UriOrPath::into_uri).unwrap_or_else(|| dark.clone());
                Some(IconPathDto::Themed { light, dark })
            }
        }
    }

    /// `IconPath.to`.
    pub fn to(value: Option<IconPathDto>) -> Option<types::IconPath> {
        Some(match value? {
            IconPathDto::ThemeIcon(icon) => types::IconPath::ThemeIcon(icon),
            IconPathDto::Uri(uri) => types::IconPath::Uri(uri),
            IconPathDto::Themed { light, dark } => types::IconPath::LightDark { light, dark },
        })
    }
}
