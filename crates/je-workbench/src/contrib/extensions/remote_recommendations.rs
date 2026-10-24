//! Porte de `src/vs/workbench/contrib/extensions/browser/remoteRecommendations.ts`: as extensões de acesso remoto e de
//! workspace virtual que o produto recomenda para a plataforma atual.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::platform::Platform;
use je_base::product::PlatformName;
use je_base::promise::Promise;
use je_platform::product::IProductService;

use super::extension_recommendations::{Activation, ExtensionRecommendation, ExtensionRecommendations};
use crate::services::extension_recommendations::extension_recommendations::{ExtensionRecommendationReason, IExtensionRecommendationReason};

/// `PlatformToString(platform)` como o `PlatformName` do produto.
fn current_platform_name() -> PlatformName {
    use je_base::platform::PlatformKind;
    match Platform::detect().kind {
        PlatformKind::Web => PlatformName::Web,
        PlatformKind::Mac => PlatformName::Mac,
        PlatformKind::Linux => PlatformName::Linux,
        PlatformKind::Windows => PlatformName::Windows,
    }
}

/// `RemoteRecommendations`.
pub struct RemoteRecommendations {
    this: Weak<RemoteRecommendations>,
    activation: Activation,
    recommendations: RefCell<Vec<ExtensionRecommendation>>,
    product_service: Rc<dyn IProductService>,
}

impl RemoteRecommendations {
    /// `constructor(productService)`.
    pub fn new(product_service: Rc<dyn IProductService>) -> Rc<Self> {
        Rc::new_cyclic(|weak| Self { this: weak.clone(), activation: Activation::default(), recommendations: RefCell::new(Vec::new()), product_service })
    }
}

impl ExtensionRecommendations for RemoteRecommendations {
    fn recommendations(&self) -> Vec<ExtensionRecommendation> {
        self.recommendations.borrow().clone()
    }

    fn activation(&self) -> &Activation {
        &self.activation
    }

    fn do_activate(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("o recomendador está vivo enquanto é usado");
        async_fn(async move {
            // `{ ...remoteExtensionTips, ...virtualWorkspaceExtensionTips }`: a chave repetida fica com o segundo.
            let mut extension_tips: indexmap::IndexMap<String, (String, Option<Vec<PlatformName>>)> = indexmap::IndexMap::new();
            if let Some(tips) = this.product_service.remote_extension_tips() {
                for (key, tip) in tips {
                    extension_tips.insert(key.clone(), (tip.extension_id.clone(), tip.supported_platforms.clone()));
                }
            }
            if let Some(tips) = this.product_service.virtual_workspace_extension_tips() {
                for (key, tip) in tips {
                    extension_tips.insert(key.clone(), (tip.extension_id.clone(), tip.supported_platforms.clone()));
                }
            }
            let current_platform = current_platform_name();
            *this.recommendations.borrow_mut() = extension_tips
                .values()
                .filter(|(_, supported_platforms)| supported_platforms.as_ref().is_none_or(|platforms| platforms.contains(&current_platform)))
                .map(|(extension_id, _)| {
                    ExtensionRecommendation::gallery(
                        extension_id.to_lowercase(),
                        IExtensionRecommendationReason { reason_id: ExtensionRecommendationReason::Application, reason_text: String::new() },
                    )
                })
                .collect();
            Ok(())
        })
    }
}
