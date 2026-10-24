//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/randomService.ts`: o serviço que gera os UUIDs, para os
//! testes poderem trocá-lo por um determinístico.

use std::rc::Rc;

use je_base::uuid::{generate_uuid, prefixed_uuid};
use je_instantiation::{Injectable, service_id, service_impl};

/// `IRandomService`.
pub trait IRandomService {
    /// `generateUuid()`.
    fn generate_uuid(&self) -> String;

    /// `generatePrefixedUuid(prefix)`. O prefixo deve ter 3 letras.
    fn generate_prefixed_uuid(&self, prefix: &str) -> String;
}

service_id!(pub I_RANDOM_SERVICE: dyn IRandomService = "randomService");

/// `RandomService`.
pub struct RandomService;

impl IRandomService for RandomService {
    fn generate_uuid(&self) -> String {
        generate_uuid()
    }

    /// Namespace should be 3 letter.
    fn generate_prefixed_uuid(&self, namespace: &str) -> String {
        prefixed_uuid(namespace)
    }
}

impl Injectable for RandomService {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

service_impl!(RandomService => dyn IRandomService);
