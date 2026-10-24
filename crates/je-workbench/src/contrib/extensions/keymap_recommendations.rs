//! Porte de `src/vs/workbench/contrib/extensions/browser/keymapRecommendations.ts`: `productService.keymapExtensionTips`.

use std::rc::Rc;

use je_platform::product::IProductService;

use super::product_tips_recommendations::ProductTipsRecommendations;

/// `KeymapRecommendations`.
pub type KeymapRecommendations = ProductTipsRecommendations;

/// `new KeymapRecommendations(productService)`.
pub fn new_keymap_recommendations(product_service: Rc<dyn IProductService>) -> Rc<KeymapRecommendations> {
    ProductTipsRecommendations::new(product_service, |product| product.keymap_extension_tips().cloned())
}
