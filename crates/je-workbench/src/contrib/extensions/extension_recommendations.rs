//! Porte de `src/vs/workbench/contrib/extensions/browser/extensionRecommendations.ts`: o tipo de uma recomendação e a
//! classe base dos recomendadores, que ativa uma vez só.
//!
//! Notas de tradução:
//! - **`ExtensionRecommendations` (classe abstrata).** É o trait [`ExtensionRecommendations`]: cada recomendador guarda uma
//!   [`Activation`] (o `_activationPromise`) e implementa `do_activate`; `activate` e `activated` vêm do trait.
//! - **`GalleryExtensionRecommendation | ResourceExtensionRecommendation`** é [`ExtensionRecommendation`], com a extensão como
//!   [`RecommendedExtension`]; os recomendadores que só devolvem identificadores da galeria usam
//!   [`GalleryExtensionRecommendation`] (`extension: string`).

use std::cell::RefCell;

use je_base::promise::Promise;
use je_platform::extension_recommendations::extension_recommendations::RecommendedExtension;

use crate::services::extension_recommendations::extension_recommendations::IExtensionRecommendationReason;

/// `GalleryExtensionRecommendation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GalleryExtensionRecommendation {
    pub extension: String,
    pub reason: IExtensionRecommendationReason,
}

/// `ExtensionRecommendation` (`GalleryExtensionRecommendation | ResourceExtensionRecommendation`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionRecommendation {
    pub extension: RecommendedExtension,
    pub reason: IExtensionRecommendationReason,
}

impl ExtensionRecommendation {
    /// Uma recomendação de extensão da galeria.
    pub fn gallery(extension: impl Into<String>, reason: IExtensionRecommendationReason) -> Self {
        Self { extension: RecommendedExtension::Gallery(extension.into()), reason }
    }
}

impl From<GalleryExtensionRecommendation> for ExtensionRecommendation {
    fn from(recommendation: GalleryExtensionRecommendation) -> Self {
        Self::gallery(recommendation.extension, recommendation.reason)
    }
}

/// O `_activationPromise` da classe base.
#[derive(Default)]
pub struct Activation {
    promise: RefCell<Option<Promise<()>>>,
}

/// `ExtensionRecommendations`.
pub trait ExtensionRecommendations {
    /// `recommendations`.
    fn recommendations(&self) -> Vec<ExtensionRecommendation>;

    /// A ativação desta instância.
    fn activation(&self) -> &Activation;

    /// `doActivate()`.
    fn do_activate(&self) -> Promise<()>;

    /// `get activated()`.
    fn activated(&self) -> bool {
        self.activation().promise.borrow().is_some()
    }

    /// `activate()`: `doActivate` roda uma vez, e as chamadas seguintes recebem a mesma promessa.
    fn activate(&self) -> Promise<()> {
        if let Some(promise) = self.activation().promise.borrow().clone() {
            return promise;
        }
        let promise = self.do_activate();
        *self.activation().promise.borrow_mut() = Some(promise.clone());
        promise
    }
}
