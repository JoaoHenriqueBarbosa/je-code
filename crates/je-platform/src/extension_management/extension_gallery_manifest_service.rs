//! Porte de `src/vs/platform/extensionManagement/common/extensionGalleryManifestService.ts`: o serviço base do
//! manifesto da galeria, que monta o manifesto a partir da galeria declarada no produto (`extensionsGallery`).
//!
//! Divergências:
//! - A classe é herdada pelo serviço do workbench (`WorkbenchExtensionGalleryManifestService`) e pelo do navegador, que
//!   sobrescrevem o estado e o manifesto e chamam `super.getExtensionGalleryManifest()`. Sem herança, eles guardam um
//!   `Rc<ExtensionGalleryManifestService>` e chamam o [`IExtensionGalleryManifestService`] dele no lugar do `super`. O
//!   `_register` e o `_store` são [`ExtensionGalleryManifestService::store`], que o descarte do serviço descarta.
//! - `IProductService.extensionsGallery` é [`IProductService::extensions_gallery`]. `publisherUrl` e `itemUrl`, que o
//!   [`IExtensionsGallery`](je_base::product::IExtensionsGallery) do porte não declara, são lidos das chaves que ele
//!   guarda em `extra`; a verdade de uma URL (`if (extensionsGallery.itemUrl)`) é "texto não vazio".
//! - Os nomes dos filtros e das ordenações (`FilterType` e `SortBy` de `extensionManagement.ts`) são os textos de
//!   `je_extensions::management::FilterType::as_str` e `SortBy::as_str`: `je-extensions` depende de `je-platform`, então a
//!   tabela repete os textos aqui em vez de importar o tipo.
//! - O número dos flags (`0x10000`...) é `f64`, como o `number` do original.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::promise::Promise;

use super::extension_gallery_manifest::{
    ExtensionGalleryManifestCapabilities, ExtensionGalleryManifestResource, ExtensionGalleryManifestStatus, ExtensionGalleryResourceType,
    ExtensionQueryCapabilities, ExtensionQueryCapabilityValue, Flag, IExtensionGalleryManifest, IExtensionGalleryManifestService,
    SigningCapabilities,
};
use crate::product::IProductService;

/// `ExtensionGalleryConfig`: a galeria do produto, só com o que o manifesto lê.
struct ExtensionGalleryConfig<'a> {
    service_url: &'a str,
    item_url: Option<&'a str>,
    publisher_url: Option<&'a str>,
    resource_url_template: &'a str,
}

/// Os `FilterType` (nome, valor) que o manifesto declara.
const FILTERING: [(&str, f64); 8] = [
    ("Tag", 1.0),
    ("ExtensionId", 4.0),
    ("Category", 5.0),
    ("ExtensionName", 7.0),
    ("Target", 8.0),
    ("Featured", 9.0),
    ("SearchText", 10.0),
    ("ExcludeWithFlags", 12.0),
];

/// Os `SortBy` (nome, valor) que o manifesto declara.
const SORTING: [(&str, f64); 8] = [
    ("NoneOrRelevance", 0.0),
    ("LastUpdatedDate", 1.0),
    ("Title", 2.0),
    ("PublisherName", 3.0),
    ("InstallCount", 4.0),
    ("AverageRating", 6.0),
    ("PublishedDate", 10.0),
    ("WeightedRating", 12.0),
];

/// Os `Flag` (e o valor de cada um) que o manifesto declara.
const FLAGS: [(Flag, f64); 14] = [
    (Flag::None, 0x0 as f64),
    (Flag::IncludeVersions, 0x1 as f64),
    (Flag::IncludeFiles, 0x2 as f64),
    (Flag::IncludeCategoryAndTags, 0x4 as f64),
    (Flag::IncludeSharedAccounts, 0x8 as f64),
    (Flag::IncludeVersionProperties, 0x10 as f64),
    (Flag::ExcludeNonValidated, 0x20 as f64),
    (Flag::IncludeInstallationTargets, 0x40 as f64),
    (Flag::IncludeAssetUri, 0x80 as f64),
    (Flag::IncludeStatistics, 0x100 as f64),
    (Flag::IncludeLatestVersionOnly, 0x200 as f64),
    (Flag::Unpublished, 0x1000 as f64),
    (Flag::IncludeNameConflictInfo, 0x8000 as f64),
    (Flag::IncludeLatestPrereleaseAndStableVersionOnly, 0x10000 as f64),
];

fn capability_values<'a>(entries: impl IntoIterator<Item = (&'a str, f64)>) -> Vec<ExtensionQueryCapabilityValue> {
    entries.into_iter().map(|(name, value)| ExtensionQueryCapabilityValue { name: name.to_owned(), value }).collect()
}

fn resource(id: String, r#type: ExtensionGalleryResourceType) -> ExtensionGalleryManifestResource {
    ExtensionGalleryManifestResource { id, r#type: r#type.as_str().to_owned() }
}

/// `ExtensionGalleryManifestService`.
pub struct ExtensionGalleryManifestService {
    base: Rc<Disposable>,
    /// `this._store`.
    store: Rc<DisposableStore>,
    product_service: Rc<dyn IProductService>,
}

impl ExtensionGalleryManifestService {
    pub fn new(product_service: Rc<dyn IProductService>) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        Rc::new(Self { base, store, product_service })
    }

    /// `this._store`: o que o serviço registra com `_register` e que o descarte dele descarta.
    pub fn store(&self) -> &Rc<DisposableStore> {
        &self.store
    }

    /// `this.productService` (`protected` no original).
    pub fn product_service(&self) -> &Rc<dyn IProductService> {
        &self.product_service
    }

    /// A galeria do produto, se declara a URL do serviço.
    fn gallery_config(&self) -> Option<ExtensionGalleryConfig<'_>> {
        let gallery = self.product_service.extensions_gallery()?;
        if gallery.service_url.is_empty() {
            return None;
        }
        let url = |key: &str| gallery.extra.get(key).and_then(|value| value.as_str()).filter(|url| !url.is_empty());
        Some(ExtensionGalleryConfig {
            service_url: &gallery.service_url,
            item_url: url("itemUrl"),
            publisher_url: url("publisherUrl"),
            resource_url_template: &gallery.resource_url_template,
        })
    }
}

impl IDisposable for ExtensionGalleryManifestService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IExtensionGalleryManifestService for ExtensionGalleryManifestService {
    fn extension_gallery_manifest_status(&self) -> ExtensionGalleryManifestStatus {
        if self.gallery_config().is_some() { ExtensionGalleryManifestStatus::Available } else { ExtensionGalleryManifestStatus::Unavailable }
    }

    fn on_did_change_extension_gallery_manifest_status(&self) -> Event<ExtensionGalleryManifestStatus> {
        Event::none()
    }

    fn on_did_change_extension_gallery_manifest(&self) -> Event<Option<IExtensionGalleryManifest>> {
        Event::none()
    }

    fn get_extension_gallery_manifest(&self) -> Promise<Option<IExtensionGalleryManifest>> {
        let Some(gallery) = self.gallery_config() else {
            return Promise::resolved(None);
        };

        let mut resources = vec![
            resource(format!("{}/extensionquery", gallery.service_url), ExtensionGalleryResourceType::ExtensionQueryService),
            resource(format!("{}/vscode/{{publisher}}/{{name}}/latest", gallery.service_url), ExtensionGalleryResourceType::ExtensionLatestVersionUri),
            resource(
                format!("{}/publishers/{{publisher}}/extensions/{{name}}/{{version}}/stats?statType={{statTypeName}}", gallery.service_url),
                ExtensionGalleryResourceType::ExtensionStatisticsUri,
            ),
        ];

        if let Some(publisher_url) = gallery.publisher_url {
            resources.push(resource(format!("{publisher_url}/{{publisher}}"), ExtensionGalleryResourceType::PublisherViewUri));
        }

        if let Some(item_url) = gallery.item_url {
            resources.push(resource(format!("{item_url}?itemName={{publisher}}.{{name}}"), ExtensionGalleryResourceType::ExtensionDetailsViewUri));
            resources.push(resource(format!("{item_url}?itemName={{publisher}}.{{name}}&ssr=false#review-details"), ExtensionGalleryResourceType::ExtensionRatingViewUri));
        }

        if !gallery.resource_url_template.is_empty() {
            resources.push(resource(gallery.resource_url_template.to_owned(), ExtensionGalleryResourceType::ExtensionResourceUri));
        }

        Promise::resolved(Some(IExtensionGalleryManifest {
            version: String::new(),
            resources,
            capabilities: ExtensionGalleryManifestCapabilities {
                extension_query: ExtensionQueryCapabilities {
                    filtering: Some(capability_values(FILTERING)),
                    sorting: Some(capability_values(SORTING)),
                    flags: Some(capability_values(FLAGS.map(|(flag, value)| (flag.as_str(), value)))),
                },
                signing: Some(SigningCapabilities { all_public_repository_signed: true, all_private_repository_signed: None }),
                extensions: None,
            },
        }))
    }
}
