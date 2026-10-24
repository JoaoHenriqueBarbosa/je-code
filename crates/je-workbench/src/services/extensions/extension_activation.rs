//! Porte dos tipos de ativação de `src/vs/workbench/services/extensions/common/extensions.ts`:
//! `ExtensionActivationReason` e `MissingExtensionDependency`.
//!
//! O restante de `extensions.ts` (`IExtensionService` e afins) fica em [`super::extensions`]; estes dois tipos ficam
//! aqui porque o ativador do host de extensões e o protocolo `MainThreadExtensionService` os usam e nenhum deles
//! depende do contrato reduzido de lá.

use je_extensions::manifest::ExtensionIdentifier;

/// `ExtensionActivationReason`: por que uma extensão foi ativada.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionActivationReason {
    pub startup: bool,
    pub extension_id: ExtensionIdentifier,
    pub activation_event: String,
}

/// `MissingExtensionDependency`: a dependência (identificador como texto) que faltou numa ativação.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingExtensionDependency {
    pub dependency: String,
}

impl MissingExtensionDependency {
    pub fn new(dependency: impl Into<String>) -> Self {
        Self { dependency: dependency.into() }
    }
}
