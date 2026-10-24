//! Porte de `src/vs/platform/workspace/common/canonicalUri.ts`: o serviço que pede às extensões a identidade canônica
//! de um recurso para outro esquema.
//!
//! Divergências: `UriComponents` é a [`URI`] (já revivida); o `_serviceBrand` é o identificador
//! [`I_CANONICAL_URI_SERVICE`]; `ICanonicalUriProvider` é um trait registrado como `Rc<dyn ...>`.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `ICanonicalUriProvider`.
pub trait ICanonicalUriProvider {
    /// O esquema que o provedor atende.
    fn scheme(&self) -> String;

    fn provide_canonical_uri(&self, uri: &URI, target_scheme: &str, token: CancellationToken) -> Promise<Option<URI>>;
}

/// `ICanonicalUriService`.
pub trait ICanonicalUriService {
    fn register_canonical_uri_provider(&self, provider: Rc<dyn ICanonicalUriProvider>) -> DisposableRef;
}

service_id!(pub I_CANONICAL_URI_SERVICE: dyn ICanonicalUriService = "canonicalUriIdentityService");
