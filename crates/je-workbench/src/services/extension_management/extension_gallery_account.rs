//! Porte de `src/vs/workbench/services/extensionManagement/common/extensionGalleryAccount.ts`: o contrato da conta que
//! dá acesso ao marketplace privado (a metade de autenticação do acesso) e o do provedor que a resolve.
//!
//! Divergências: `ExtensionGalleryAccountStatus` (`const enum` de valor textual) é um enum com `as_str`;
//! `IExtensionGalleryAccount.accessToken?` é `Option<String>`.

use std::rc::Rc;

use je_base::event::Event;
use je_base::promise::Promise;
use je_instantiation::service_id;

/// `IExtensionGalleryAccount`: `accessToken` só vem quando o provedor autentica com um bearer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IExtensionGalleryAccount {
    pub access_token: Option<String>,
}

/// `ExtensionGalleryAccountStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionGalleryAccountStatus {
    /// None signed in, or several with no choice made.
    SignedOut,
    Ineligible,
    Eligible,
    /// Could not be resolved: a transient auth failure, not a sign-out.
    Unknown,
}

impl ExtensionGalleryAccountStatus {
    /// O texto do estado, como o original o grava.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SignedOut => "signedOut",
            Self::Ineligible => "ineligible",
            Self::Eligible => "eligible",
            Self::Unknown => "unknown",
        }
    }
}

/// `IExtensionGalleryAccountProvider`: the authentication half of marketplace access. Implementations live in the
/// Electron layer and are supplied through [`IExtensionGalleryAccountService::set_account_provider`], so the service
/// itself never depends on authentication.
pub trait IExtensionGalleryAccountProvider {
    fn account_status(&self) -> ExtensionGalleryAccountStatus;
    fn on_did_change_account_status(&self) -> Event<ExtensionGalleryAccountStatus>;
    fn on_did_change_account(&self) -> Event<()>;

    /// Never prompts. Check [`IExtensionGalleryAccountProvider::account_status`] for whether the account may actually
    /// be used.
    fn get_account(&self) -> Promise<Option<IExtensionGalleryAccount>>;

    /// Interactive. The provider owns account selection and how the session is obtained.
    fn sign_in(&self) -> Promise<()>;
}

/// `IExtensionGalleryAccountService`: identity and entitlement for the Private Marketplace. Knows nothing about URLs or
/// HTTP.
pub trait IExtensionGalleryAccountService {
    fn account_status(&self) -> ExtensionGalleryAccountStatus;
    fn on_did_change_account_status(&self) -> Event<ExtensionGalleryAccountStatus>;
    fn on_did_change_account(&self) -> Event<()>;

    /// Never prompts. Check [`IExtensionGalleryAccountService::account_status`] for whether the account may actually
    /// be used.
    fn get_account(&self) -> Promise<Option<IExtensionGalleryAccount>>;

    /// Interactive sign-in for whichever provider the deployment configured.
    fn sign_in(&self) -> Promise<()>;

    fn set_account_provider(&self, provider: Rc<dyn IExtensionGalleryAccountProvider>);
}

service_id!(pub I_EXTENSION_GALLERY_ACCOUNT_SERVICE: dyn IExtensionGalleryAccountService = "extensionGalleryAccountService");
