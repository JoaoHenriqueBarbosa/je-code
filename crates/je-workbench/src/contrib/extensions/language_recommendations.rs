//! Porte de `src/vs/workbench/contrib/extensions/browser/languageRecommendations.ts`: `productService.languageExtensionTips`.

use std::rc::Rc;

use je_platform::product::IProductService;

use super::product_tips_recommendations::ProductTipsRecommendations;

/// `LanguageRecommendations`.
pub type LanguageRecommendations = ProductTipsRecommendations;

/// `new LanguageRecommendations(productService)`.
pub fn new_language_recommendations(product_service: Rc<dyn IProductService>) -> Rc<LanguageRecommendations> {
    ProductTipsRecommendations::new(product_service, |product| product.language_extension_tips().cloned())
}
