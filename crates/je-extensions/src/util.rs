//! Porte de `src/vs/platform/extensionManagement/common/extensionManagementUtil.ts` e do que sobrou de
//! `node/extensionManagementUtil.ts` (a leitura do manifesto de um VSIX está em [`crate::vsix`]).

use std::cmp::Ordering;

use crate::fs::ExtFs;
use crate::js::compare_ignore_case;
use crate::management::{
    ExtensionOrPublisher, HostPlatform, MaliciousExtensionInfo, current_arch, get_target_platform,
};
use crate::manifest::{ExtensionIdentifierInfo, ExtensionManifest, TargetPlatform, UNDEFINED_PUBLISHER};

/// `areSameExtensions`: o `uuid` decide quando os dois têm; senão o `id`, sem diferenciar caixa.
pub fn are_same_extensions(a: &ExtensionIdentifierInfo, b: &ExtensionIdentifierInfo) -> bool {
    if let (Some(a_uuid), Some(b_uuid)) = (&a.uuid, &b.uuid)
        && !a_uuid.is_empty()
        && !b_uuid.is_empty()
    {
        return a_uuid == b_uuid;
    }
    if a.id == b.id {
        return true;
    }
    compare_ignore_case(&a.id, &b.id) == Ordering::Equal
}

/// `^([^.]+\..+)-(\d+\.\d+\.\d+)(-(.+))?$` aplicado ao nome da pasta. O `.+` guloso do identificador recua até o
/// último `-` que deixa o resto casar com a versão.
fn match_extension_key(key: &str) -> Option<(String, String, Option<String>)> {
    // O identificador tem de ter um `.` depois de ao menos um caractere que não seja `.`.
    let mut search_from = key.len();
    while let Some(dash) = key[..search_from].rfind('-') {
        let (id, rest) = (&key[..dash], &key[dash + 1..]);
        search_from = dash;
        let id_ok = id.find('.').is_some_and(|dot| dot > 0 && dot + 1 < id.len());
        if !id_ok {
            continue;
        }
        // rest = `\d+\.\d+\.\d+(-.+)?`
        let (version, platform) = match rest.find('-') {
            Some(index) => (&rest[..index], Some(&rest[index + 1..])),
            None => (rest, None),
        };
        let parts: Vec<&str> = version.split('.').collect();
        let numeric = parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        if numeric && platform.is_none_or(|p| !p.is_empty()) {
            return Some((id.to_owned(), version.to_owned(), platform.map(str::to_owned)));
        }
    }
    None
}

/// `ExtensionKey`: `publisher.name-version[-targetPlatform]`, o nome da pasta de uma extensão instalada.
#[derive(Clone, Debug)]
pub struct ExtensionKey {
    pub identifier: ExtensionIdentifierInfo,
    pub version: String,
    pub target_platform: TargetPlatform,
}

impl ExtensionKey {
    pub fn new(identifier: ExtensionIdentifierInfo, version: impl Into<String>, target_platform: TargetPlatform) -> Self {
        Self { identifier, version: version.into(), target_platform }
    }

    /// `ExtensionKey.parse`.
    pub fn parse(key: &str) -> Option<ExtensionKey> {
        let (id, version, platform) = match_extension_key(key)?;
        // `matches[4] as TargetPlatform || undefined`: o texto, qualquer que seja, vira a plataforma.
        let target_platform = match platform {
            Some(platform) => crate::management::to_target_platform_lossy(&platform),
            None => TargetPlatform::Undefined,
        };
        Some(ExtensionKey::new(ExtensionIdentifierInfo::new(id), version, target_platform))
    }

    pub fn id(&self) -> &str {
        &self.identifier.id
    }

    pub fn equals(&self, other: &ExtensionKey) -> bool {
        are_same_extensions(&self.identifier, &other.identifier) && self.version == other.version && self.target_platform == other.target_platform
    }
}

impl std::fmt::Display for ExtensionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.identifier.id, self.version)?;
        if self.target_platform != TargetPlatform::Undefined {
            write!(f, "-{}", self.target_platform)?;
        }
        Ok(())
    }
}

/// `getIdAndVersion`: `publisher.name@1.2.3` ou `@prerelease`.
pub fn get_id_and_version(id: &str) -> (String, Option<String>) {
    // ^([^.]+\..+)@((prerelease)|(\d+\.\d+\.\d+(-.*)?))$
    if let Some(at) = id.rfind('@') {
        let (name, version) = (&id[..at], &id[at + 1..]);
        let name_ok = name.find('.').is_some_and(|dot| dot > 0 && dot + 1 < name.len());
        let version_ok = version == "prerelease" || {
            let (numbers, _pre) = match version.find('-') {
                Some(index) => (&version[..index], Some(&version[index..])),
                None => (version, None),
            };
            let parts: Vec<&str> = numbers.split('.').collect();
            parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        };
        if name_ok && version_ok {
            return (adopt_to_gallery_extension_id(name), Some(version.to_owned()));
        }
    }
    (adopt_to_gallery_extension_id(id), None)
}

/// `getExtensionId`.
pub fn get_extension_id(publisher: &str, name: &str) -> String {
    format!("{publisher}.{name}")
}

/// `adoptToGalleryExtensionId`.
pub fn adopt_to_gallery_extension_id(id: &str) -> String {
    id.to_lowercase()
}

/// `getGalleryExtensionId`.
pub fn get_gallery_extension_id(publisher: Option<&str>, name: &str) -> String {
    // `publisher ?? UNDEFINED_PUBLISHER`: só `undefined`/`null` caem no padrão, a string vazia não.
    adopt_to_gallery_extension_id(&get_extension_id(publisher.unwrap_or(UNDEFINED_PUBLISHER), name))
}

/// `groupByExtension`: agrupa por extensão, na ordem do primeiro aparecimento.
pub fn group_by_extension<T>(extensions: Vec<T>, get_identifier: impl Fn(&T) -> &ExtensionIdentifierInfo) -> Vec<Vec<T>> {
    let mut groups: Vec<Vec<T>> = Vec::new();
    for extension in extensions {
        let position = groups.iter().position(|group| group.iter().any(|e| are_same_extensions(get_identifier(e), get_identifier(&extension))));
        match position {
            Some(index) => groups[index].push(extension),
            None => groups.push(vec![extension]),
        }
    }
    groups
}

/// `BetterMergeId`.
pub const BETTER_MERGE_ID: &str = "pprice.better-merge";

/// `isAlpineLinux`: `ID=alpine` no `os-release`.
fn is_alpine_linux(fs: &dyn ExtFs) -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let content = fs.read_to_string("/etc/os-release").or_else(|_| fs.read_to_string("/usr/lib/os-release")).ok();
    // `/^ID=([^\u001b\r\n]*)/m`
    content.is_some_and(|content| {
        content.lines().any(|line| line.strip_prefix("ID=").is_some_and(|value| !value.contains('\u{1b}') && value.trim_end_matches('\r') == "alpine"))
    })
}

/// `computeTargetPlatform`.
pub fn compute_target_platform(fs: &dyn ExtFs) -> TargetPlatform {
    let host = if is_alpine_linux(fs) { HostPlatform::Alpine } else { HostPlatform::current() };
    get_target_platform(host, Some(current_arch()))
}

/// `findMatchingMaliciousEntry`.
pub fn find_matching_malicious_entry<'a>(
    identifier: &ExtensionIdentifierInfo,
    malicious: &'a [MaliciousExtensionInfo],
) -> Option<&'a MaliciousExtensionInfo> {
    malicious.iter().find(|entry| match &entry.extension_or_publisher {
        ExtensionOrPublisher::Publisher(publisher) => {
            compare_ignore_case(identifier.id.split('.').next().unwrap_or(""), publisher) == Ordering::Equal
        }
        ExtensionOrPublisher::Extension(other) => are_same_extensions(identifier, other),
    })
}

/// `isMalicious`.
pub fn is_malicious(identifier: &ExtensionIdentifierInfo, malicious: &[MaliciousExtensionInfo]) -> bool {
    find_matching_malicious_entry(identifier, malicious).is_some()
}

/// `getExtensionDependencies`: as dependências (diretas e transitivas) de uma extensão entre as instaladas, na ordem
/// em que são descobertas. Uma dependência que não está instalada, ou que casa com mais de uma instalada, não entra
/// (e as dependências dela também não). O original trabalha sobre `IExtension` e só lê o manifesto da extensão cujas
/// dependências se querem (`extension`, o manifesto dela); aqui o tipo das instaladas é qualquer um de que se leiam o
/// manifesto e o identificador (`manifest_of` e `identifier_of`).
pub fn get_extension_dependencies<T: Clone>(
    installed_extensions: &[T],
    extension: &ExtensionManifest,
    manifest_of: impl Fn(&T) -> &ExtensionManifest,
    identifier_of: impl Fn(&T) -> &ExtensionIdentifierInfo,
) -> Vec<T> {
    let mut dependencies: Vec<T> = Vec::new();
    let mut extensions: std::collections::VecDeque<String> = extension.extension_dependencies.clone().unwrap_or_default().into();

    while let Some(id) = extensions.pop_front() {
        let wanted = ExtensionIdentifierInfo::new(id.clone());
        if !id.is_empty() && dependencies.iter().all(|e| !are_same_extensions(identifier_of(e), &wanted)) {
            let matching: Vec<&T> = installed_extensions.iter().filter(|e| are_same_extensions(identifier_of(e), &wanted)).collect();
            if let [only] = matching.as_slice() {
                dependencies.push((*only).clone());
                extensions.extend(manifest_of(*only).extension_dependencies.clone().unwrap_or_default());
            }
        }
    }

    dependencies
}
