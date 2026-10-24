//! Porte de `src/vs/platform/uriIdentity/common/uriIdentity.ts`: o contrato `IUriIdentityService`.
//!
//! A implementação (`UriIdentityService`, com o cache de URIs canônicas e a leitura do sistema de arquivos) é o
//! [`uri_identity_service`](super::uri_identity_service).

use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_instantiation::{delayed_proxy, service_id};

/// `IUriIdentityService`.
pub trait IUriIdentityService {
    /// Uri extensions that are aware of casing. Devolve uma cópia: o [`ExtUri`] guarda o predicado de caixa num
    /// `Rc`, então a cópia continua lendo o mesmo estado.
    fn ext_uri(&self) -> ExtUri;

    /// Returns a canonical uri for the given resource. Different uris can point to the same resource. That's
    /// because of casing or missing normalization, e.g the following uris are different but refer to the same
    /// document (because windows paths are not case-sensitive)
    ///
    /// ```txt
    /// file:///c:/foo/bar.txt
    /// file:///c:/FOO/BAR.txt
    /// ```
    ///
    /// This function should be invoked when feeding uris into the system that represent the truth, e.g document
    /// uris or marker-to-document associations etc. This function should NOT be called to pretty print a label nor
    /// to sanitize a uri.
    ///
    /// Samples:
    ///
    /// | in | out | |
    /// |---|---|---|
    /// | `file:///foo/bar/../bar` | `file:///foo/bar` | n/a |
    /// | `file:///foo/bar/../bar#frag` | `file:///foo/bar#frag` | keep fragment |
    /// | `file:///foo/BAR` | `file:///foo/bar` | assume ignore case |
    /// | `file:///foo/bar/../BAR?q=2` | `file:///foo/BAR?q=2` | query makes it a different document |
    fn as_canonical_uri(&self, uri: &URI) -> URI;
}

delayed_proxy!(pub UriIdentityServiceProxy for dyn IUriIdentityService {
    methods {
        fn ext_uri(&self) -> ExtUri;
        fn as_canonical_uri(&self, uri: &URI) -> URI;
    }
    events {}
});

service_id!(pub I_URI_IDENTITY_SERVICE: dyn IUriIdentityService = "IUriIdentityService", proxy UriIdentityServiceProxy);
