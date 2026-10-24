//! Porte de `src/vs/workbench/services/extensionManagement/browser/extensionGalleryManifestService.ts`: o serviço do
//! manifesto da galeria no workbench do navegador. É o serviço base (o manifesto que o produto declara), que também
//! empurra o manifesto ao servidor remoto, quando há conexão.
//!
//! Divergências:
//! - O `extends ExtensionGalleryManifestService` é o campo `base`; `this._register` é o [`ExtensionGalleryManifestService::store`]
//!   dele. Como o serviço não sobrescreve nada, o trait [`IExtensionGalleryManifestService`] só repassa ao `base`.
//! - As dependências por injeção entram em [`WebExtensionGalleryManifestService::new`]. O `IRemoteAgentService` é o recorte
//!   [`IRemoteAgentConnectionProvider`] de [`super::extension_gallery_contracts`]. `registerSingleton(..., InstantiationType.Delayed)`
//!   não tem equivalente: quem monta o workbench cria o serviço.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::marshalling::Marshalled;
use je_base::promise::Promise;
use je_platform::extension_management::extension_gallery_manifest::{
    ExtensionGalleryManifestStatus, IExtensionGalleryManifest, IExtensionGalleryManifestService,
};
use je_platform::extension_management::extension_gallery_manifest_service::ExtensionGalleryManifestService;
use je_platform::product::IProductService;

use super::extension_gallery_contracts::IRemoteAgentConnectionProvider;
use super::extension_gallery_manifest_service::EXTENSION_GALLERY_MANIFEST_CHANNEL;

/// `WebExtensionGalleryManifestService`.
pub struct WebExtensionGalleryManifestService {
    base: Rc<ExtensionGalleryManifestService>,
}

impl WebExtensionGalleryManifestService {
    pub fn new(product_service: Rc<dyn IProductService>, remote_agent_service: &dyn IRemoteAgentConnectionProvider) -> Self {
        let base = ExtensionGalleryManifestService::new(product_service);
        if let Some(remote_connection) = remote_agent_service.get_connection() {
            let channel = remote_connection.get_channel(EXTENSION_GALLERY_MANIFEST_CHANNEL);
            let service = base.clone();
            let _ = base.get_extension_gallery_manifest().then(move |manifest| {
                let set_manifest = {
                    let channel = channel.clone();
                    move |manifest: &Option<IExtensionGalleryManifest>| {
                        channel.call("setExtensionGalleryManifest", Marshalled::Array(vec![manifest.as_ref().map_or(Marshalled::Null, IExtensionGalleryManifest::to_marshalled)]), CancellationToken::None);
                    }
                };
                set_manifest(&manifest);
                service.store().add(service.on_did_change_extension_gallery_manifest().listen(move |manifest| set_manifest(manifest)));
            });
        }
        Self { base }
    }
}

impl IDisposable for WebExtensionGalleryManifestService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IExtensionGalleryManifestService for WebExtensionGalleryManifestService {
    fn extension_gallery_manifest_status(&self) -> ExtensionGalleryManifestStatus {
        self.base.extension_gallery_manifest_status()
    }

    fn on_did_change_extension_gallery_manifest_status(&self) -> Event<ExtensionGalleryManifestStatus> {
        self.base.on_did_change_extension_gallery_manifest_status()
    }

    fn on_did_change_extension_gallery_manifest(&self) -> Event<Option<IExtensionGalleryManifest>> {
        self.base.on_did_change_extension_gallery_manifest()
    }

    fn get_extension_gallery_manifest(&self) -> Promise<Option<IExtensionGalleryManifest>> {
        self.base.get_extension_gallery_manifest()
    }
}
