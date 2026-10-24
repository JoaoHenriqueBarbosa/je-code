//! Porte de `src/vs/platform/encryption/common/encryptionService.ts`: o contrato de cifra do que o `SecretStorageService`
//! guarda (`IEncryptionService`) e os nomes dos provedores de chaves do sistema (`KnownStorageProvider`).
//!
//! Decisões de tradução:
//! - **O original não cifra nada aqui.** `IEncryptionService` é só o contrato: a implementação real (o `safeStorage` do
//!   Electron, o chaveiro do sistema) fica fora do porte, e quem consome recebe o trait. Os testes do original usam
//!   dublês que implementam o mesmo contrato.
//! - **`ICommonEncryptionService`, `IEncryptionService` e `IEncryptionMainService`** são os traits
//!   [`ICommonEncryptionService`], [`IEncryptionService`] (que o estende) e [`IEncryptionMainService`] (que o estende sem
//!   acrescentar nada, e vale para qualquer [`IEncryptionService`]). Os identificadores são
//!   [`I_ENCRYPTION_SERVICE`] e [`I_ENCRYPTION_MAIN_SERVICE`]. As promessas são [`Promise`]. O `_serviceBrand` não existe.
//! - **`const enum PasswordStoreCLIOption` e `KnownStorageProvider`** são enums com o texto do original em `as_str`.
//!   `isKwallet` e `isGnome` recebem o texto do backend (`string`) e comparam com esses valores.

use je_base::promise::Promise;
use je_instantiation::service_id;

/// `ICommonEncryptionService`.
pub trait ICommonEncryptionService {
    /// `encrypt(value)`.
    fn encrypt(&self, value: &str) -> Promise<String>;

    /// `decrypt(value)`.
    fn decrypt(&self, value: &str) -> Promise<String>;

    /// `isEncryptionAvailable()`.
    fn is_encryption_available(&self) -> Promise<bool>;
}

/// `IEncryptionService`.
pub trait IEncryptionService: ICommonEncryptionService {
    /// `setUsePlainTextEncryption()`.
    fn set_use_plain_text_encryption(&self) -> Promise<()>;

    /// `getKeyStorageProvider()`.
    fn get_key_storage_provider(&self) -> Promise<KnownStorageProvider>;
}

service_id!(pub I_ENCRYPTION_SERVICE: dyn IEncryptionService = "encryptionService");

/// `IEncryptionMainService`: o mesmo contrato, para o processo principal.
pub trait IEncryptionMainService: IEncryptionService {}

impl<T: IEncryptionService + ?Sized> IEncryptionMainService for T {}

service_id!(pub I_ENCRYPTION_MAIN_SERVICE: dyn IEncryptionMainService = "encryptionMainService");

/// `PasswordStoreCLIOption`: os valores do argumento de linha de comando `password-store`. Note que não são os mesmos
/// valores que o `getSelectedStorageBackend` da API `safeStorage` devolve (ver [`KnownStorageProvider`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordStoreCLIOption {
    Kwallet,
    Kwallet5,
    GnomeLibsecret,
    Basic,
}

impl PasswordStoreCLIOption {
    /// O valor textual do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kwallet => "kwallet",
            Self::Kwallet5 => "kwallet5",
            Self::GnomeLibsecret => "gnome-libsecret",
            Self::Basic => "basic",
        }
    }
}

/// `KnownStorageProvider`: os valores que o `getSelectedStorageBackend` da API `safeStorage` devolve, mais os de Windows
/// e macOS, que ele não devolve mas foram acrescentados para completar as plataformas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownStorageProvider {
    Unknown,
    BasicText,

    // Linux
    GnomeAny,
    GnomeLibsecret,
    GnomeKeyring,
    Kwallet,
    Kwallet5,
    Kwallet6,

    // Windows
    Dplib,

    // macOS
    KeychainAccess,
}

impl KnownStorageProvider {
    /// O valor textual do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::BasicText => "basic_text",
            Self::GnomeAny => "gnome_any",
            Self::GnomeLibsecret => "gnome_libsecret",
            Self::GnomeKeyring => "gnome_keyring",
            Self::Kwallet => "kwallet",
            Self::Kwallet5 => "kwallet5",
            Self::Kwallet6 => "kwallet6",
            Self::Dplib => "dpapi",
            Self::KeychainAccess => "keychain_access",
        }
    }
}

/// `isKwallet(backend)`.
pub fn is_kwallet(backend: &str) -> bool {
    [KnownStorageProvider::Kwallet, KnownStorageProvider::Kwallet5, KnownStorageProvider::Kwallet6].iter().any(|provider| backend == provider.as_str())
}

/// `isGnome(backend)`.
pub fn is_gnome(backend: &str) -> bool {
    [KnownStorageProvider::GnomeAny, KnownStorageProvider::GnomeLibsecret, KnownStorageProvider::GnomeKeyring].iter().any(|provider| backend == provider.as_str())
}
