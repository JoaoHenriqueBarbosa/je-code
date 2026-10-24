//! Porte de `src/vs/workbench/services/authentication/browser/authenticationAccessService.ts`: o serviço
//! que guarda, por provedor e conta, quais extensões podem usar a conta.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados (`@IStorageService`, `@IProductService`) são a tupla `Deps` do
//!   [`Injectable`]. `IAuthenticationAccessService` é o trait [`IAuthenticationAccessService`]; a classe
//!   herda de `Disposable`, aqui um `Disposable` guardado no campo `base`.
//! - **`registerSingleton(..., InstantiationType.Delayed)`** vira [`register`], que o chamador roda uma vez
//!   na montagem do workbench (Rust não tem efeito colateral ao carregar o módulo); o proxy de instanciação
//!   atrasada é o [`AuthenticationAccessServiceProxy`].
//! - **Valores.** `AllowedExtension[]` é `Vec<AllowedExtension>` (cópia, como o `JSON.parse` fresco do
//!   original) e `updateAllowedExtensions` recebe `&[AllowedExtension]`. `boolean | undefined` é
//!   `Option<bool>`. `trustedExtensionAuthAccess` (`string[] | Record<string, string[]> | undefined`) é
//!   [`StringsOrDictionary`] opcional, lido do `IProductService` a cada chamada.
//! - **`JSON.parse`.** Um erro de sintaxe ou de leitura no armazenamento dá lista vazia (o `catch` do
//!   original). Um JSON válido que não é array deixa o original lançar `TypeError` em `.find`, fora do
//!   `try`, e aqui lança o mesmo erro com [`errors::throw`].
//! - **`isAccessAllowed`** usa a mesma lista de extensões confiáveis que `readAllowedExtensions` (`array`,
//!   ou `dicionário[provedor]`, ou vazia): no original as duas formas leem a mesma propriedade, só escritas
//!   de outro jeito (`includes` numa e laço na outra), e o resultado é idêntico.

use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::json::Value;
use je_base::json_formatter::json_stringify;
use je_base::jsonc::json_parse;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::product::StringsOrDictionary;
use je_extensions::manifest::ExtensionIdentifier;
use je_instantiation::{InstantiationType, Injectable, delayed_proxy, register_singleton, service_id, service_impl};
use je_platform::product::IProductService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;

use super::authentication::AllowedExtension;

/// O `{ providerId: string; accountName: string }` de `onDidChangeExtensionSessionAccess`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionSessionAccessChange {
    pub provider_id: String,
    pub account_name: String,
}

/// `IAuthenticationAccessService`.
pub trait IAuthenticationAccessService {
    fn on_did_change_extension_session_access(&self) -> Event<ExtensionSessionAccessChange>;

    /// Check extension access to an account.
    ///
    /// - `provider_id`: the id of the authentication provider.
    /// - `account_name`: the account name that access is checked for.
    /// - `extension_id`: the id of the extension requesting access.
    ///
    /// Returns `Some(true)` or `Some(false)` if the user has opted to permanently grant or disallow access,
    /// and `None` if they haven't made a choice yet.
    fn is_access_allowed(&self, provider_id: &str, account_name: &str, extension_id: &str) -> Option<bool>;
    fn read_allowed_extensions(&self, provider_id: &str, account_name: &str) -> Vec<AllowedExtension>;
    fn update_allowed_extensions(&self, provider_id: &str, account_name: &str, extensions: &[AllowedExtension]);
    fn remove_allowed_extensions(&self, provider_id: &str, account_name: &str);
}

delayed_proxy!(pub AuthenticationAccessServiceProxy for dyn IAuthenticationAccessService {
    methods {
        fn is_access_allowed(&self, provider_id: &str, account_name: &str, extension_id: &str) -> Option<bool>;
        fn read_allowed_extensions(&self, provider_id: &str, account_name: &str) -> Vec<AllowedExtension>;
        fn update_allowed_extensions(&self, provider_id: &str, account_name: &str, extensions: &[AllowedExtension]);
        fn remove_allowed_extensions(&self, provider_id: &str, account_name: &str);
    }
    events {
        fn on_did_change_extension_session_access(&self) -> ExtensionSessionAccessChange;
    }
});

service_id!(pub I_AUTHENTICATION_ACCESS_SERVICE: dyn IAuthenticationAccessService = "IAuthenticationAccessService", proxy AuthenticationAccessServiceProxy);

// Pendências anotadas no original por Tyler Leonhardt: mover esta classe para o `MainThreadAuthentication`,
// e decidir se ela deveria guardar só o que é permitido e descartar o que é negado.
/// `AuthenticationAccessService`.
pub struct AuthenticationAccessService {
    base: Rc<Disposable>,
    on_did_change_extension_session_access: Rc<Emitter<ExtensionSessionAccessChange>>,
    storage_service: Rc<dyn StorageService>,
    product_service: Rc<dyn IProductService>,
}

impl Injectable for AuthenticationAccessService {
    type Args = ();
    type Deps = (Rc<dyn StorageService>, Rc<dyn IProductService>);

    fn create(_: (), (storage_service, product_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_extension_session_access = Emitter::new();
        base.register(on_did_change_extension_session_access.clone());
        Rc::new(Self { base, on_did_change_extension_session_access, storage_service, product_service })
    }
}

service_impl!(AuthenticationAccessService => dyn IAuthenticationAccessService, disposable);

/// `registerSingleton(IAuthenticationAccessService, AuthenticationAccessService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<AuthenticationAccessService>(I_AUTHENTICATION_ACCESS_SERVICE, InstantiationType::Delayed);
}

impl AuthenticationAccessService {
    /// A chave de `${providerId}-${accountName}` no armazenamento.
    fn storage_key(provider_id: &str, account_name: &str) -> String {
        format!("{provider_id}-{account_name}")
    }

    /// A lista de `trustedExtensionAuthAccess` que vale para o provedor: o próprio array, ou o
    /// `dicionário[provedor]`, ou vazia.
    fn trusted_extension_ids(&self, provider_id: &str) -> Vec<String> {
        match self.product_service.trusted_extension_auth_access() {
            // Case 1: trustedExtensionAuthAccess is an array
            Some(StringsOrDictionary::Strings(ids)) => ids,
            // Case 2: trustedExtensionAuthAccess is an object
            Some(StringsOrDictionary::Dictionary(dictionary)) => dictionary.get(provider_id).cloned().unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// O trecho `try { ... JSON.parse ... } catch (err) { }` de `readAllowedExtensions`.
    fn read_stored_extensions(&self, provider_id: &str, account_name: &str) -> Vec<AllowedExtension> {
        let key = Self::storage_key(provider_id, account_name);
        let Some(source) = self.storage_service.get(&key, StorageScope::Application, None).filter(|source| !source.is_empty()) else {
            return Vec::new();
        };
        match json_parse(&source.encode_utf16().collect::<Vec<u16>>()) {
            Ok(Value::Array(items)) => items.iter().filter_map(AllowedExtension::from_json).collect(),
            // `trustedExtensions.find` não existe fora de um array.
            Ok(_) => errors::throw(Error::new("trustedExtensions.find is not a function")),
            Err(_) => Vec::new(),
        }
    }

    fn fire_did_change(&self, provider_id: &str, account_name: &str) {
        self.on_did_change_extension_session_access
            .fire(ExtensionSessionAccessChange { provider_id: provider_id.to_owned(), account_name: account_name.to_owned() });
    }
}

impl IAuthenticationAccessService for AuthenticationAccessService {
    fn on_did_change_extension_session_access(&self) -> Event<ExtensionSessionAccessChange> {
        self.on_did_change_extension_session_access.event()
    }

    fn is_access_allowed(&self, provider_id: &str, account_name: &str, extension_id: &str) -> Option<bool> {
        let extension_key = ExtensionIdentifier::to_key(extension_id);
        if self.trusted_extension_ids(provider_id).contains(&extension_key) {
            return Some(true);
        }

        let allow_list = self.read_allowed_extensions(provider_id, account_name);
        let extension_data = allow_list.iter().find(|extension| extension.id == extension_key)?;
        // This property didn't exist on this data previously, inclusion in the list at all indicates allowance
        Some(extension_data.allowed.unwrap_or(true))
    }

    fn read_allowed_extensions(&self, provider_id: &str, account_name: &str) -> Vec<AllowedExtension> {
        let mut trusted_extensions = self.read_stored_extensions(provider_id, account_name);

        // Add trusted extensions from product.json if they're not already in the list
        for extension_id in self.trusted_extension_ids(provider_id) {
            let extension_key = ExtensionIdentifier::to_key(&extension_id);
            match trusted_extensions.iter_mut().find(|extension| extension.id == extension_key) {
                None => {
                    // Add new trusted extension (name will be set by caller if they have extension info)
                    trusted_extensions.push(AllowedExtension {
                        id: extension_key,
                        name: extension_id, // Use original casing for display name
                        allowed: Some(true),
                        trusted: Some(true),
                        ..AllowedExtension::default()
                    });
                }
                Some(existing_extension) => {
                    // Update existing extension to be trusted
                    existing_extension.allowed = Some(true);
                    existing_extension.trusted = Some(true);
                }
            }
        }

        trusted_extensions
    }

    fn update_allowed_extensions(&self, provider_id: &str, account_name: &str, extensions: &[AllowedExtension]) {
        let mut allow_list = self.read_allowed_extensions(provider_id, account_name);
        for extension in extensions {
            let extension_key = ExtensionIdentifier::to_key(&extension.id);
            match allow_list.iter_mut().find(|e| e.id == extension_key) {
                None => allow_list.push(AllowedExtension { id: extension_key, ..extension.clone() }),
                Some(existing) => {
                    existing.allowed = extension.allowed;
                    // Update name if provided and not already set to a proper name
                    if !extension.name.is_empty() && extension.name != extension_key && existing.name != extension.name {
                        existing.name = extension.name.clone();
                    }
                }
            }
        }

        // Filter out trusted extensions before storing - they should only come from product.json, not user storage
        let user_managed_extensions: Vec<Value> =
            allow_list.iter().filter(|extension| extension.trusted != Some(true)).map(AllowedExtension::to_json).collect();
        self.storage_service.store(
            &Self::storage_key(provider_id, account_name),
            StorageValue::String(json_stringify(&Value::Array(user_managed_extensions), None)),
            StorageScope::Application,
            StorageTarget::User,
        );
        self.fire_did_change(provider_id, account_name);
    }

    fn remove_allowed_extensions(&self, provider_id: &str, account_name: &str) {
        self.storage_service.remove(&Self::storage_key(provider_id, account_name), StorageScope::Application);
        self.fire_did_change(provider_id, account_name);
    }
}

impl IDisposable for AuthenticationAccessService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
