//! Porte de `src/vs/workbench/services/extensions/common/extensionsProposedApi.ts`: quais propostas de API cada extensão pode
//! usar (a lista do `package.json`, a do `product.json` e a linha de comando).
//!
//! Divergências:
//! - `IWorkbenchEnvironmentService`, `IProductService` e `ILogService` são os traits do porte; os campos do produto que o
//!   original lê (`quality`, `extensionEnabledApiProposals`) vêm do [`IProductService`] (o segundo é um método com corpo
//!   padrão `None`).
//! - `updateEnabledApiProposals` recebe `&mut [SharedExtensionDescription]` e muta cada descrição (`Mutable<IExtensionDescription>`
//!   do original) com `Rc::make_mut`: as descrições que o serviço de extensões passa ainda não foram compartilhadas, e o
//!   serviço aplica a atualização antes de registrá-las (no original, o registro guarda a mesma referência que é mutada depois).
//! - `allApiProposals[name]` é [`is_api_proposal`].
//! - Não portado: o `ApiProposalsMarkdowneRenderer` e o `registerExtensionFeature({ id: 'enabledApiProposals' ... })` (a
//!   aba de funcionalidades do editor de extensões, `extensionFeatures.ts`, que o porte não tem).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::{LogServiceRef, Logger};
use je_platform::product::IProductService;

use super::extension_description_registry::SharedExtensionDescription;
use super::extensions_api_proposals::is_api_proposal;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// `ExtensionsProposedApi`.
pub struct ExtensionsProposedApi {
    log_service: LogServiceRef,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    env_enables_proposed_api_for_all: bool,
    env_enabled_extensions: HashSet<String>,
    product_enabled_extensions: RefCell<HashMap<String, Vec<String>>>,
}

impl ExtensionsProposedApi {
    pub fn new(log_service: LogServiceRef, environment_service: Rc<dyn IWorkbenchEnvironmentService>, product_service: &dyn IProductService) -> Self {
        let extension_enabled_proposed_api = environment_service.extension_enabled_proposed_api();
        let env_enabled_extensions: HashSet<String> =
            extension_enabled_proposed_api.iter().flatten().map(|id| ExtensionIdentifier::to_key(id)).collect();

        let env_enables_proposed_api_for_all = !environment_service.is_built() // always allow proposed API when running out of sources
            || (environment_service.is_extension_development() && product_service.quality() != Some("stable")) // do not allow proposed API against stable builds when developing an extension
            || (env_enabled_extensions.is_empty() && extension_enabled_proposed_api.is_some()); // always allow proposed API if --enable-proposed-api is provided without extension ID

        let mut product_enabled_extensions: HashMap<String, Vec<String>> = HashMap::new();

        // NEW world - product.json spells out what proposals each extension can use
        if let Some(product_proposals) = product_service.extension_enabled_api_proposals() {
            for (k, value) in product_proposals {
                let key = ExtensionIdentifier::to_key(k);
                let proposal_names: Vec<String> = value
                    .iter()
                    .filter(|name| {
                        if !is_api_proposal(name) {
                            log_service.warn(&format!("Via 'product.json#extensionEnabledApiProposals' extension '{key}' wants API proposal '{name}' but that proposal DOES NOT EXIST. Likely, the proposal has been finalized (check 'vscode.d.ts') or was abandoned."));
                            return false;
                        }
                        true
                    })
                    .cloned()
                    .collect();
                product_enabled_extensions.insert(key, proposal_names);
            }
        }

        Self { log_service, environment_service, env_enables_proposed_api_for_all, env_enabled_extensions, product_enabled_extensions: RefCell::new(product_enabled_extensions) }
    }

    /// `updateEnabledApiProposals(extensions)`.
    pub fn update_enabled_api_proposals(&self, extensions: &mut [SharedExtensionDescription]) {
        for extension in extensions {
            self.do_update_enabled_api_proposals(Rc::make_mut(extension));
        }
    }

    fn do_update_enabled_api_proposals(&self, extension: &mut je_extensions::scanner::ExtensionDescription) {
        let key = ExtensionIdentifier::to_key(&extension.identifier.value);

        // warn about invalid proposal and remove them from the list
        if let Some(proposals) = extension.manifest.enabled_api_proposals.as_mut().filter(|proposals| !proposals.is_empty()) {
            proposals.retain(|name| {
                let result = is_api_proposal(name);
                if !result {
                    self.log_service.error(&format!("Extension '{key}' wants API proposal '{name}' but that proposal DOES NOT EXIST. Likely, the proposal has been finalized (check 'vscode.d.ts') or was abandoned."));
                }
                result
            });
        }

        let product_enabled_proposals = self.product_enabled_extensions.borrow().get(&key).cloned();
        if let Some(mut product_enabled_proposals) = product_enabled_proposals {
            // NOTE that proposals that are listed in product.json override whatever is declared in the extension
            // itself. This is needed for us to know what proposals are used "in the wild". Merging product.json-proposals
            // and extension-proposals would break that.

            // check for difference between product.json-declaration and package.json-declaration
            let product_set: HashSet<&String> = product_enabled_proposals.iter().collect();
            let mut extension_set: Vec<&String> = Vec::new();
            for name in extension.manifest.enabled_api_proposals.iter().flatten() {
                if !extension_set.contains(&name) {
                    extension_set.push(name);
                }
            }
            let diff: Vec<String> = extension_set.iter().filter(|a| !product_set.contains(**a)).map(|a| (*a).clone()).collect();
            if !diff.is_empty() {
                self.log_service.error(&format!(
                    "Extension '{key}' appears in product.json but enables LESS API proposals than the extension wants.\npackage.json (LOSES): {}\nproduct.json (WINS): {}\nDELTA: {}",
                    extension_set.iter().map(|name| name.as_str()).collect::<Vec<_>>().join("\n"),
                    product_enabled_proposals.join("\n"),
                    diff.join("\n"),
                ));

                if self.environment_service.is_extension_development() {
                    self.log_service.error(&format!(
                        "Proceeding with EXTRA proposals ({}) because extension is in development mode. Still, this EXTENSION WILL BE BROKEN unless product.json is updated.",
                        diff.join(", ")
                    ));
                    product_enabled_proposals.extend(diff.iter().cloned());
                    self.product_enabled_extensions.borrow_mut().insert(key.clone(), product_enabled_proposals.clone());
                }
            }

            extension.manifest.enabled_api_proposals = Some(product_enabled_proposals);
            return;
        }

        if self.env_enables_proposed_api_for_all || self.env_enabled_extensions.contains(&key) {
            // proposed API usage is not restricted and allowed just like the extension
            // has declared it
            return;
        }

        if !extension.is_builtin && extension.manifest.enabled_api_proposals.as_ref().is_some_and(|proposals| !proposals.is_empty()) {
            // restrictive: extension cannot use proposed API in this context and its declaration is nulled
            let declared = extension.manifest.enabled_api_proposals.as_ref().map(|proposals| proposals.join(", ")).filter(|joined| !joined.is_empty()).unwrap_or_else(|| "*".to_owned());
            self.log_service.error(&format!(
                "Extension '{} CANNOT USE these API proposals '{declared}'. You MUST start in extension development mode or use the --enable-proposed-api command line flag",
                extension.identifier.value
            ));
            extension.manifest.enabled_api_proposals = Some(Vec::new());
        }
    }
}
