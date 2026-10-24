//! `extHostTypeConverters.ts`: `DocumentLink`, a conversão do `languages.ILink` do protocolo para o `vscode.DocumentLink`.
//!
//! Divergências:
//! - `DocumentLink.from` não entra: quem o chama (`ExtHostLanguageFeatures`) tem o conversor próprio em
//!   `ext_host_language_features_converters`.
//! - `typeof link.url === 'string' ? URI.parse(link.url, true) : URI.revive(link.url)` dentro do `try { } catch { }` é o
//!   [`LinkUrl`] com o `URI.parse` estrito sob [`errors::catch`] (o `URI.revive` de um [`URI`] devolve o próprio valor).
//! - O `ILink` que cruza o RPC é um [`Dynamic`] (`{ range, url?, tooltip? }`, o `toJSON` do `Link` de `getLinks.ts` sem o
//!   `cacheId`): [`DocumentLink::to_dto`] e [`DocumentLink::from_dto`] são as duas pontas.

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::uri::URI;
use je_editor::languages::{ILink, LinkUrl};

use super::geometry::Range;
use crate::api::common::ext_host_types as types;
use crate::api::common::ext_host_values::{ApiValue, is_instance_of};

/// `typeConverters.DocumentLink`.
pub struct DocumentLink;

impl DocumentLink {
    pub fn to(link: &ILink) -> Result<types::DocumentLink, Error> {
        let target = match &link.url {
            Some(LinkUrl::Text(url)) if !url.is_empty() => errors::catch(|| URI::parse(url.clone(), true)).ok(),
            Some(LinkUrl::Uri(url)) => Some(url.clone()),
            _ => None,
        };
        let mut result = types::DocumentLink::new(Range::to(&link.range)?, target)?;
        result.tooltip = link.tooltip.clone();
        Ok(result)
    }

    /// O `languages.ILink` como o DTO do RPC.
    pub fn to_dto(link: &ILink) -> Dynamic {
        let url = link.url.as_ref().map(|url| match url {
            LinkUrl::Uri(uri) => uri.to_dynamic(),
            LinkUrl::Text(text) => Dynamic::String(text.clone()),
        });
        let entries = [("range", Some(Range::to_dto(&link.range))), ("url", url), ("tooltip", link.tooltip.as_deref().map(Dynamic::string))];
        Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item))))
    }

    /// A leitura do DTO de [`DocumentLink::to_dto`]; `None` se o valor não tem um `range` válido.
    pub fn from_dto(value: &Dynamic) -> Option<ILink> {
        let url = match value.get_str("url") {
            Dynamic::String(text) => Some(LinkUrl::Text(text)),
            uri if is_instance_of(&uri, "Uri") => URI::from_dynamic(&uri).ok().map(LinkUrl::Uri),
            _ => None,
        };
        let tooltip = match value.get_str("tooltip") {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        };
        Some(ILink { range: Range::from_dto(&value.get_str("range"))?, url, tooltip, cache_id: None })
    }
}
