//! O pedaço de `IUserDataProfilesService` (`vs/platform/userDataProfile`) de que o gerenciamento de extensões
//! precisa: onde fica o `extensions.json` e o cache de cada perfil. O serviço completo de perfis de dados do
//! usuário (criar, apagar, trocar de perfil, sincronizar) é outro porte.
//!
//! TODO(integração): trocar por `je_platform` quando o `userDataProfile` for portado.

use crate::paths;

/// `IUserDataProfile`, reduzido.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserDataProfile {
    pub name: String,
    pub is_default: bool,
    /// A pasta do perfil (`location`): para o padrão, o `User` da pasta de dados roaming.
    pub location: String,
    /// O `extensions.json` do perfil (`extensionsResource`).
    pub extensions_resource: String,
    /// A pasta de cache do perfil (`cacheHome`).
    pub cache_home: String,
}

/// `IUserDataProfilesService`, reduzido ao perfil padrão e à lista de perfis.
#[derive(Clone, Debug)]
pub struct UserDataProfiles {
    pub default_profile: UserDataProfile,
    pub profiles: Vec<UserDataProfile>,
}

impl UserDataProfiles {
    /// O perfil padrão do VS Code: o `extensions.json` mora na pasta de extensões do usuário
    /// (`<extensionsPath>/extensions.json`), e o cache em `<cacheHome>/CachedProfilesData/__default__profile__`
    /// (`cache_home` é o `environmentService.cacheHome`, a pasta de dados do usuário).
    pub fn with_default(extensions_path: &str, user_roaming_data_home: &str, cache_home: &str) -> Self {
        let default_profile = UserDataProfile {
            name: "Default".to_owned(),
            is_default: true,
            location: paths::join_path(user_roaming_data_home, &["User"]),
            extensions_resource: paths::join_path(extensions_path, &["extensions.json"]),
            cache_home: paths::join_path(cache_home, &["CachedProfilesData", "__default__profile__"]),
        };
        Self { profiles: vec![default_profile.clone()], default_profile }
    }
}
