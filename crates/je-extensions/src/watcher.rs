//! Porte de `src/vs/platform/extensionManagement/node/extensionsWatcher.ts`: acompanha quais extensões estão em quais
//! perfis e, quando uma versão sai do último perfil que a tinha, marca a pasta para remoção (o `.obsolete`).
//!
//! O original também observa o `extensions.json` de cada perfil pelo `FileService` para reagir a mudanças feitas por
//! outra janela. Aqui essa entrada é [`ExtensionsWatcher::on_did_extensions_profile_change`], que quem observa o
//! arquivo chama quando ele muda.
//!
//! TODO(integração): ligar o `FileService.watch` do `je-platform` (hoje baseado em `Promise`) a esta chamada, e o
//! `onDidChangeProfiles` do serviço de perfis a [`ExtensionsWatcher::on_did_change_profiles`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::DisposableRef;
use je_platform::log::Logger;

use crate::management::LocalExtension;
use crate::management_service::ExtensionManagementService;
use crate::manifest::{ExtensionIdentifier, ExtensionIdentifierInfo};
use crate::paths;
use crate::profile::{UserDataProfile, UserDataProfiles};
use crate::profile_scanner::{DidProfileExtensionsEvent, ExtensionsProfileScannerService, ProfileExtensionsEvent};
use crate::scanner::ExtensionsScannerService;
use crate::util::get_id_and_version;

/// `DidChangeProfileExtensionsEvent`: o que mudou no `extensions.json` por obra de outra fonte.
#[derive(Clone, Debug)]
pub struct DidChangeProfileExtensionsEvent {
    pub added: Option<ProfileExtensionsChange>,
    pub removed: Option<ProfileExtensionsChange>,
}

#[derive(Clone, Debug)]
pub struct ProfileExtensionsChange {
    pub extensions: Vec<ExtensionIdentifierInfo>,
    pub profile_location: String,
}

/// O estado compartilhado com os ouvintes.
struct State {
    /// `chave (id@versão)` para os perfis (caminhos do `extensions.json`) que a têm.
    all_extensions: RefCell<HashMap<String, HashSet<String>>>,
    management: Rc<ExtensionManagementService>,
    profile_scanner: Rc<ExtensionsProfileScannerService>,
    logger: Rc<dyn Logger>,
    on_did_change_by_another_source: Rc<Emitter<DidChangeProfileExtensionsEvent>>,
}

/// `ExtensionsWatcher`.
pub struct ExtensionsWatcher {
    state: Rc<State>,
    scanner: Rc<ExtensionsScannerService>,
    profiles: UserDataProfiles,
    _listeners: Vec<DisposableRef>,
}

fn get_key(identifier: &ExtensionIdentifierInfo, version: &str) -> String {
    format!("{}@{version}", ExtensionIdentifier::to_key(&identifier.id))
}

fn from_key(key: &str) -> Option<(ExtensionIdentifierInfo, String)> {
    let (id, version) = get_id_and_version(key);
    version.map(|v| (ExtensionIdentifierInfo::new(id), v))
}

impl State {
    fn add_with_key(&self, key: String, profile_location: &str) {
        self.all_extensions.borrow_mut().entry(key).or_default().insert(profile_location.to_owned());
    }

    fn remove_with_key(&self, key: &str, profile_location: &str) {
        let mut all = self.all_extensions.borrow_mut();
        if let Some(profiles) = all.get_mut(key) {
            profiles.remove(profile_location);
            if profiles.is_empty() {
                all.remove(key);
            }
        }
    }

    fn on_add_extensions(&self, event: &ProfileExtensionsEvent) {
        for extension in &event.extensions {
            self.add_with_key(get_key(&extension.identifier, &extension.version), &event.profile_location);
        }
    }

    fn on_did_add_extensions(&self, event: &DidProfileExtensionsEvent) {
        for extension in &event.extensions {
            let key = get_key(&extension.identifier, &extension.version);
            if event.error.is_some() {
                self.remove_with_key(&key, &event.profile_location);
            } else {
                self.add_with_key(key, &event.profile_location);
            }
        }
    }

    fn on_remove_extensions(&self, event: &ProfileExtensionsEvent) {
        for extension in &event.extensions {
            self.remove_with_key(&get_key(&extension.identifier, &extension.version), &event.profile_location);
        }
    }

    fn on_did_remove_extensions(&self, event: &DidProfileExtensionsEvent) {
        let mut to_delete: Vec<LocalExtension> = Vec::new();
        for extension in &event.extensions {
            let key = get_key(&extension.identifier, &extension.version);
            if event.error.is_some() {
                self.add_with_key(key, &event.profile_location);
                continue;
            }
            self.remove_with_key(&key, &event.profile_location);
            if !self.all_extensions.borrow().contains_key(&key) {
                self.logger.debug(&format!("Extension is removed from all profiles {} {}", extension.identifier.id, extension.version));
                match self.management.scan_installed_extension_at_location(&extension.location) {
                    Some(result) => to_delete.push(result),
                    None => self.logger.info(&format!("Extension not found at the location {}", extension.location)),
                }
            }
        }
        if !to_delete.is_empty() {
            self.delete_extensions_not_in_profiles(Some(to_delete));
        }
    }

    fn delete_extensions_not_in_profiles(&self, to_delete: Option<Vec<LocalExtension>>) {
        let to_delete = match to_delete {
            Some(list) => list,
            None => match self.management.scan_all_user_installed_extensions() {
                Ok(installed) => installed.into_iter().filter(|e| !self.all_extensions.borrow().contains_key(&get_key(&e.identifier, &e.manifest.version))).collect(),
                Err(error) => {
                    self.logger.error(&error.message);
                    return;
                }
            },
        };
        if !to_delete.is_empty()
            && let Err(error) = self.management.delete_extensions(&to_delete.iter().collect::<Vec<_>>())
        {
            self.logger.error(&error.message);
        }
    }

    fn populate_extensions_from_profile(&self, profile_location: &str) {
        match self.profile_scanner.scan_profile_extensions(profile_location, None) {
            Ok(extensions) => {
                for extension in extensions {
                    self.add_with_key(get_key(&extension.identifier, &extension.version), profile_location);
                }
            }
            Err(error) => self.logger.error(&error.to_string()),
        }
    }
}

impl ExtensionsWatcher {
    /// Registra os ouvintes do scanner de perfil. Chamar [`ExtensionsWatcher::initialize`] em seguida.
    pub fn new(
        management: Rc<ExtensionManagementService>,
        scanner: Rc<ExtensionsScannerService>,
        profile_scanner: Rc<ExtensionsProfileScannerService>,
        profiles: UserDataProfiles,
        logger: Rc<dyn Logger>,
    ) -> Self {
        let state = Rc::new(State {
            all_extensions: RefCell::new(HashMap::new()),
            management,
            profile_scanner: profile_scanner.clone(),
            logger,
            on_did_change_by_another_source: Emitter::new(),
        });
        let listeners = vec![
            profile_scanner.on_add_extensions().listen({
                let state = state.clone();
                move |event| state.on_add_extensions(event)
            }),
            profile_scanner.on_did_add_extensions().listen({
                let state = state.clone();
                move |event| state.on_did_add_extensions(event)
            }),
            profile_scanner.on_remove_extensions().listen({
                let state = state.clone();
                move |event| state.on_remove_extensions(event)
            }),
            profile_scanner.on_did_remove_extensions().listen({
                let state = state.clone();
                move |event| state.on_did_remove_extensions(event)
            }),
        ];
        Self { state, scanner, profiles, _listeners: listeners }
    }

    pub fn on_did_change_extensions_by_another_source(&self) -> Event<DidChangeProfileExtensionsEvent> {
        self.state.on_did_change_by_another_source.event()
    }

    /// `initialize`: garante o `extensions.json` padrão, lê todos os perfis e apaga o que nenhum perfil tem.
    pub fn initialize(&self) {
        if let Err(error) = self.scanner.initialize_default_profile_extensions() {
            self.state.logger.error(&format!("Error while initializing Extensions Watcher {error}"));
            return;
        }
        self.on_did_change_profiles(&self.profiles.profiles);
        self.state.delete_extensions_not_in_profiles(None);
    }

    /// `onDidChangeProfiles`: lê as extensões dos perfis novos.
    pub fn on_did_change_profiles(&self, added: &[UserDataProfile]) {
        for profile in added {
            self.state.populate_extensions_from_profile(&profile.extensions_resource);
        }
    }

    /// `onDidExtensionsProfileChange`: o `extensions.json` de um perfil mudou no disco; compara com o que se sabia.
    pub fn on_did_extensions_profile_change(&self, profile_location: &str) {
        let extensions = match self.state.profile_scanner.scan_profile_extensions(profile_location, None) {
            Ok(extensions) => extensions,
            Err(error) => {
                self.state.logger.error(&error.to_string());
                return;
            }
        };
        let mut added: Vec<ExtensionIdentifierInfo> = Vec::new();
        let mut removed: Vec<ExtensionIdentifierInfo> = Vec::new();
        let cached: HashSet<String> =
            self.state.all_extensions.borrow().iter().filter(|(_, profiles)| profiles.iter().any(|p| paths::is_equal(p, profile_location))).map(|(key, _)| key.clone()).collect();
        let mut keys = HashSet::new();
        for extension in &extensions {
            let key = get_key(&extension.identifier, &extension.version);
            keys.insert(key.clone());
            if !cached.contains(&key) {
                added.push(extension.identifier.clone());
                self.state.add_with_key(key, profile_location);
            }
        }
        for key in cached {
            if !keys.contains(&key)
                && let Some((identifier, _)) = from_key(&key)
            {
                removed.push(identifier);
                self.state.remove_with_key(&key, profile_location);
            }
        }
        if !added.is_empty() || !removed.is_empty() {
            let change = |extensions: Vec<ExtensionIdentifierInfo>| (!extensions.is_empty()).then(|| ProfileExtensionsChange { extensions, profile_location: profile_location.to_owned() });
            self.state.on_did_change_by_another_source.fire(DidChangeProfileExtensionsEvent { added: change(added), removed: change(removed) });
        }
    }
}
