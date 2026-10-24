//! Dublês de serviço dos testes de `contrib/extensions` (`test/electron-browser/*.test.ts`).
//!
//! O original monta cada caso num `TestInstantiationService` que só declara os serviços que o caso usa; os demais
//! decoradores ficam `undefined` e o caso nunca os chama. Aqui o construtor de [`Extension`] exige todos os serviços,
//! então este módulo traz o dublê da galeria (`IExtensionGalleryService`) que recusa qualquer chamada e a montagem do
//! [`ExtensionServices`] com os serviços de teste do workbench.

use std::rc::Rc;

use je_base::network::{FileAccess, FileAccessContext, RemoteAuthorities};
use je_base::path::ProcessEnvironment;
use je_extensions::fs::ExtFs;
use je_extensions::gallery::{GalleryApi, GalleryPage, GalleryResult};
use je_extensions::management::{
    ExtensionGalleryError, ExtensionGalleryErrorCode, ExtensionInfo, ExtensionQueryOptions, ExtensionsControlManifest, GalleryExtension,
    GalleryExtensionVersion, InstallOperation, ProductVersion, QueryOptions, StatisticType,
};
use je_extensions::manifest::{ExtensionIdentifierInfo, TargetPlatform};
use je_platform::log::NullLogService;
use je_platform::product::ProductService;
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_workbench::contrib::extensions::extensions_workbench_service::ExtensionServices;
use serde_json::Value;
use je_platform::files::i_file_service::IFileService;

/// Uma galeria sem marketplace: toda consulta falha com "gallery is not available" e `isEnabled` é falso. É o estado da
/// galeria quando o produto não declara `extensionsGallery`.
pub struct UnavailableGalleryService;

fn unavailable<T>() -> GalleryResult<T> {
    Err(ExtensionGalleryError { message: "gallery is not available".to_owned(), code: ExtensionGalleryErrorCode::Failed })
}

impl GalleryApi for UnavailableGalleryService {
    fn is_enabled(&self) -> bool {
        false
    }

    fn get_extensions(&self, _extension_infos: &[ExtensionInfo], _options: &ExtensionQueryOptions) -> GalleryResult<Vec<GalleryExtension>> {
        unavailable()
    }

    fn is_extension_compatible(
        &self,
        _extension: &GalleryExtension,
        _include_pre_release: bool,
        _target_platform: TargetPlatform,
        _product_version: Option<&ProductVersion>,
    ) -> bool {
        false
    }

    fn get_compatible_extension(
        &self,
        _extension: &GalleryExtension,
        _include_pre_release: bool,
        _target_platform: TargetPlatform,
        _product_version: Option<&ProductVersion>,
    ) -> GalleryResult<Option<GalleryExtension>> {
        unavailable()
    }

    fn query(&self, _options: &QueryOptions) -> GalleryResult<GalleryPage> {
        unavailable()
    }

    fn query_page(&self, _options: &QueryOptions, _page_index: u32) -> GalleryResult<Vec<GalleryExtension>> {
        unavailable()
    }

    fn download(&self, _extension: &GalleryExtension, _location: &str, _operation: InstallOperation, _fs: &dyn ExtFs) -> GalleryResult<()> {
        unavailable()
    }

    fn download_signature_archive(&self, _extension: &GalleryExtension, _location: &str, _fs: &dyn ExtFs) -> GalleryResult<()> {
        unavailable()
    }

    fn report_statistic(&self, _publisher: &str, _name: &str, _version: &str, _statistic: StatisticType) {}

    fn get_readme(&self, _extension: &GalleryExtension) -> GalleryResult<String> {
        unavailable()
    }

    fn get_changelog(&self, _extension: &GalleryExtension) -> GalleryResult<String> {
        unavailable()
    }

    fn get_manifest(&self, _extension: &GalleryExtension) -> GalleryResult<Option<Value>> {
        unavailable()
    }

    fn get_core_translation(&self, _extension: &GalleryExtension, _language_id: &str) -> GalleryResult<Option<Value>> {
        unavailable()
    }

    fn get_all_versions(&self, _identifier: &ExtensionIdentifierInfo) -> GalleryResult<Vec<GalleryExtensionVersion>> {
        unavailable()
    }

    fn get_all_compatible_versions(
        &self,
        _identifier: &ExtensionIdentifierInfo,
        _include_pre_release: bool,
        _target_platform: TargetPlatform,
    ) -> GalleryResult<Vec<GalleryExtensionVersion>> {
        unavailable()
    }

    fn get_extensions_control_manifest(&self) -> GalleryResult<ExtensionsControlManifest> {
        unavailable()
    }
}

/// Os serviços de uma `Extension` num caso de teste: a galeria indisponível, telemetria nula, log nulo, o
/// serviço de arquivos dado (o `TestFileService`, que o caso entrega ao guarda de vazamento), o produto dado e um
/// `FileAccess` sem raiz de arquivos.
pub fn extension_services(file_service: Rc<dyn IFileService>, product_service: ProductService) -> ExtensionServices {
    ExtensionServices {
        gallery_service: Rc::new(UnavailableGalleryService),
        telemetry_service: Rc::new(NullTelemetryServiceShape),
        log_service: NullLogService::shared(),
        file_service,
        product_service: Rc::new(product_service),
        file_access: Rc::new(FileAccess::new(Rc::new(ProcessEnvironment), FileAccessContext::default(), RemoteAuthorities::new(false))),
    }
}
