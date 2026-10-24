//! Porte de `src/vs/workbench/contrib/extensions/browser/exeBasedRecommendations.ts`: as recomendações das dicas de
//! executáveis instalados na máquina.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::extension_management::extension_tips::{ExecutableBasedExtensionTip, IExtensionTipsService};

use super::extension_recommendations::{Activation, ExtensionRecommendation, ExtensionRecommendations};
use crate::services::extension_recommendations::extension_recommendations::{ExtensionRecommendationReason, IExtensionRecommendationReason};

/// `{ important, others }` de `getRecommendations(exe)`.
pub struct ExeRecommendations {
    pub important: Vec<ExtensionRecommendation>,
    pub others: Vec<ExtensionRecommendation>,
}

/// `ExeBasedRecommendations`.
pub struct ExeBasedRecommendations {
    this: Weak<ExeBasedRecommendations>,
    activation: Activation,
    other_tips: RefCell<Vec<ExecutableBasedExtensionTip>>,
    important_tips: RefCell<Vec<ExecutableBasedExtensionTip>>,
    important_exe_based_recommendations: RefCell<Option<Promise<()>>>,
    extension_tips_service: Rc<dyn IExtensionTipsService>,
}

impl ExeBasedRecommendations {
    /// `constructor(extensionTipsService)`.
    pub fn new(extension_tips_service: Rc<dyn IExtensionTipsService>) -> Rc<Self> {
        Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            activation: Activation::default(),
            other_tips: RefCell::new(Vec::new()),
            important_tips: RefCell::new(Vec::new()),
            important_exe_based_recommendations: RefCell::new(None),
            extension_tips_service,
        })
    }

    /// `get otherRecommendations()`.
    pub fn other_recommendations(&self) -> Vec<ExtensionRecommendation> {
        self.other_tips.borrow().iter().map(Self::to_extension_recommendation).collect()
    }

    /// `get importantRecommendations()`.
    pub fn important_recommendations(&self) -> Vec<ExtensionRecommendation> {
        self.important_tips.borrow().iter().map(Self::to_extension_recommendation).collect()
    }

    /// `getRecommendations(exe)`.
    pub fn get_recommendations(&self, exe: &str) -> ExeRecommendations {
        let of_exe = |tips: &RefCell<Vec<ExecutableBasedExtensionTip>>| -> Vec<ExtensionRecommendation> {
            tips.borrow().iter().filter(|tip| tip.exe_name.to_lowercase() == exe.to_lowercase()).map(Self::to_extension_recommendation).collect()
        };
        ExeRecommendations { important: of_exe(&self.important_tips), others: of_exe(&self.other_tips) }
    }

    /// `fetchImportantExeBasedRecommendations()`: a busca roda uma vez só.
    fn fetch_important_exe_based_recommendations(&self) -> Promise<()> {
        if let Some(promise) = self.important_exe_based_recommendations.borrow().clone() {
            return promise;
        }
        let promise = self.do_fetch_important_exe_based_recommendations();
        *self.important_exe_based_recommendations.borrow_mut() = Some(promise.clone());
        promise
    }

    /// `doFetchImportantExeBasedRecommendations()`.
    fn do_fetch_important_exe_based_recommendations(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("o recomendador está vivo enquanto é usado");
        async_fn(async move {
            let tips = this.extension_tips_service.get_important_executable_based_tips().await?;
            *this.important_tips.borrow_mut() = tips;
            Ok(())
        })
    }

    /// `toExtensionRecommendation(tip)`.
    fn to_extension_recommendation(tip: &ExecutableBasedExtensionTip) -> ExtensionRecommendation {
        ExtensionRecommendation::gallery(
            tip.extension_id.to_lowercase(),
            IExtensionRecommendationReason {
                reason_id: ExtensionRecommendationReason::Executable,
                reason_text: localize("exeBasedRecommendation", "This extension is recommended because you have {0} installed.", &[&tip.exe_friendly_name]),
            },
        )
    }
}

impl ExtensionRecommendations for ExeBasedRecommendations {
    fn recommendations(&self) -> Vec<ExtensionRecommendation> {
        let mut recommendations = self.important_recommendations();
        recommendations.extend(self.other_recommendations());
        recommendations
    }

    fn activation(&self) -> &Activation {
        &self.activation
    }

    fn do_activate(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("o recomendador está vivo enquanto é usado");
        async_fn(async move {
            let others = this.extension_tips_service.get_other_executable_based_tips().await?;
            *this.other_tips.borrow_mut() = others;
            this.fetch_important_exe_based_recommendations().await
        })
    }
}
