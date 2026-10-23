//! Porte de `src/vs/base/common/managedSettings.ts`: os marketplaces de plugins que a empresa gerencia.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// `IExtraKnownMarketplaceEntry['source']`, discriminado por `source`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum ExtraKnownMarketplaceSource {
    Github {
        repo: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
    },
    Git {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
    },
}

/// `IExtraKnownMarketplaceEntry`: o nome do marketplace (usado como `displayLabel`) e a origem original.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IExtraKnownMarketplaceEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,
    pub source: ExtraKnownMarketplaceSource,
}

/// `IExtraKnownMarketplaceConfigValue`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IExtraKnownMarketplaceConfigValue {
    pub source: String,
    pub auto_update: bool,
}

pub type ExtraKnownMarketplacesConfigDict = IndexMap<String, String>;

/// `IStrictMarketplaceSource['source']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StrictMarketplaceSourceKind {
    Github,
    Git,
    Url,
    Npm,
    File,
    Directory,
    HostPattern,
    PathPattern,
}

/// `IStrictMarketplaceSource`: uma entrada da lista `strictKnownMarketplaces`, discriminada por `source`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IStrictMarketplaceSource {
    pub source: StrictMarketplaceSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
}

/// `string | IExtraKnownMarketplaceEntry`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtraKnownMarketplace {
    Name(String),
    Entry(IExtraKnownMarketplaceEntry),
}

/// Whether a marketplace name would pollute the prototype chain if used as an object key.
fn is_unsafe_marketplace_key(key: &str) -> bool {
    matches!(key, "__proto__" | "constructor" | "prototype")
}

/// `extraKnownMarketplacesToConfigDict`: converte as entradas no dicionário da configuração
/// `chat.plugins.extraMarketplaces`. Entradas sem `autoUpdate` guardam o texto da origem; com `autoUpdate`
/// explícito, um JSON `{ source, autoUpdate }`. Nomes que poluiriam o protótipo são ignorados.
pub fn extra_known_marketplaces_to_config_dict(entries: Option<&[ExtraKnownMarketplace]>) -> Option<ExtraKnownMarketplacesConfigDict> {
    let entries = entries.filter(|e| !e.is_empty())?;
    let mut obj = ExtraKnownMarketplacesConfigDict::new();
    for entry in entries {
        match entry {
            ExtraKnownMarketplace::Name(name) => {
                if is_unsafe_marketplace_key(name) {
                    continue;
                }
                obj.insert(name.clone(), name.clone());
            }
            ExtraKnownMarketplace::Entry(entry) => {
                if is_unsafe_marketplace_key(&entry.name) {
                    continue;
                }
                let (base, git_ref) = match &entry.source {
                    ExtraKnownMarketplaceSource::Github { repo, r#ref } => (repo, r#ref),
                    ExtraKnownMarketplaceSource::Git { url, r#ref } => (url, r#ref),
                };
                let source = match git_ref.as_deref().filter(|r| !r.is_empty()) {
                    Some(git_ref) => format!("{base}#{git_ref}"),
                    None => base.clone(),
                };
                let value = match entry.auto_update {
                    None => source,
                    Some(auto_update) => serde_json::to_string(&IExtraKnownMarketplaceConfigValue { source, auto_update }).expect("serialização de dois campos"),
                };
                obj.insert(entry.name.clone(), value);
            }
        }
    }
    Some(obj)
}
