//! Porte de `src/vs/workbench/contrib/extensions/browser/webRecommendations.ts`: `productService.webExtensionTips`, só
//! quando o único servidor de extensões é o da web.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::product::IProductService;

use super::extension_recommendations::{Activation, ExtensionRecommendation, ExtensionRecommendations};
use crate::services::extension_management::extension_management::IExtensionManagementServerService;
use crate::services::extension_recommendations::extension_recommendations::{ExtensionRecommendationReason, IExtensionRecommendationReason};

/// `WebRecommendations`.
pub struct WebRecommendations {
    this: Weak<WebRecommendations>,
    activation: Activation,
    recommendations: RefCell<Vec<ExtensionRecommendation>>,
    product_service: Rc<dyn IProductService>,
    extension_management_server_service: Rc<dyn IExtensionManagementServerService>,
}

impl WebRecommendations {
    /// `constructor(productService, extensionManagementServerService)`.
    pub fn new(product_service: Rc<dyn IProductService>, extension_management_server_service: Rc<dyn IExtensionManagementServerService>) -> Rc<Self> {
        Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            activation: Activation::default(),
            recommendations: RefCell::new(Vec::new()),
            product_service,
            extension_management_server_service,
        })
    }
}

impl ExtensionRecommendations for WebRecommendations {
    fn recommendations(&self) -> Vec<ExtensionRecommendation> {
        self.recommendations.borrow().clone()
    }

    fn activation(&self) -> &Activation {
        &self.activation
    }

    fn do_activate(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("o recomendador está vivo enquanto é usado");
        async_fn(async move {
            let servers = &this.extension_management_server_service;
            let is_only_web = servers.web_extension_management_server().is_some()
                && servers.local_extension_management_server().is_none()
                && servers.remote_extension_management_server().is_none();
            if let (true, Some(tips)) = (is_only_web, this.product_service.web_extension_tips()) {
                *this.recommendations.borrow_mut() = tips
                    .iter()
                    .map(|extension_id| {
                        ExtensionRecommendation::gallery(
                            extension_id.to_lowercase(),
                            IExtensionRecommendationReason {
                                reason_id: ExtensionRecommendationReason::Application,
                                reason_text: localize("reason", "This extension is recommended for {0} for the Web", &[this.product_service.name_long()]),
                            },
                        )
                    })
                    .collect();
            }
            Ok(())
        })
    }
}
