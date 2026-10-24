//! O que `keymapRecommendations.ts` e `languageRecommendations.ts` têm em comum: uma lista de identificadores do produto
//! (`keymapExtensionTips`, `languageExtensionTips`) vira recomendações do motivo `Application`, em minúsculas e sem texto.
//! As duas classes do original só diferem no campo do produto que leem, que aqui é o parâmetro `select`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::promise::Promise;
use je_platform::product::IProductService;

use super::extension_recommendations::{Activation, ExtensionRecommendation, ExtensionRecommendations};
use crate::services::extension_recommendations::extension_recommendations::{ExtensionRecommendationReason, IExtensionRecommendationReason};

/// O campo do produto que o recomendador lê (`productService.keymapExtensionTips`...).
pub type ProductTipsSelector = fn(&dyn IProductService) -> Option<Vec<String>>;

/// `KeymapRecommendations` e `LanguageRecommendations`.
pub struct ProductTipsRecommendations {
    this: Weak<ProductTipsRecommendations>,
    activation: Activation,
    recommendations: RefCell<Vec<ExtensionRecommendation>>,
    product_service: Rc<dyn IProductService>,
    select: ProductTipsSelector,
}

impl ProductTipsRecommendations {
    /// `constructor(productService)`.
    pub fn new(product_service: Rc<dyn IProductService>, select: ProductTipsSelector) -> Rc<Self> {
        Rc::new_cyclic(|weak| Self { this: weak.clone(), activation: Activation::default(), recommendations: RefCell::new(Vec::new()), product_service, select })
    }
}

impl ExtensionRecommendations for ProductTipsRecommendations {
    fn recommendations(&self) -> Vec<ExtensionRecommendation> {
        self.recommendations.borrow().clone()
    }

    fn activation(&self) -> &Activation {
        &self.activation
    }

    fn do_activate(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("o recomendador está vivo enquanto é usado");
        async_fn(async move {
            if let Some(tips) = (this.select)(this.product_service.as_ref()) {
                *this.recommendations.borrow_mut() = tips
                    .iter()
                    .map(|extension_id| {
                        ExtensionRecommendation::gallery(
                            extension_id.to_lowercase(),
                            IExtensionRecommendationReason { reason_id: ExtensionRecommendationReason::Application, reason_text: String::new() },
                        )
                    })
                    .collect();
            }
            Ok(())
        })
    }
}
