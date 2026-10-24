//! Porte da interface `IExtensionGalleryService` (`src/vs/platform/extensionManagement/common/extensionManagement.ts`)
//! na forma assíncrona que o workbench consome (`Promise` e `CancellationToken`), mais o adaptador que a liga ao
//! cliente do marketplace de `je-extensions`.
//!
//! A lógica do `extensionGalleryService.ts` (consulta, filtros, escolha de versão compatível, VSIX, controle de
//! extensões) vive uma vez só em [`je_extensions::gallery::ExtensionGalleryService`], atrás do trait síncrono
//! [`GalleryApi`]; reescrevê-la aqui duplicaria as 2 mil linhas e os 46 casos que a cobrem. O adaptador
//! [`AsyncExtensionGalleryService`] só embrulha cada chamada numa `Promise` já assentada.
//!
//! Diferenças e recortes:
//! - **Rede.** O `IRequestService` do `je-platform` é assíncrono e o `HttpClient` do `je-extensions` é síncrono: ainda
//!   não há ponte entre os dois, então a E/S do marketplace segue pelo `HttpClient`. Trocar a E/S exige separar, no
//!   `ExtensionGalleryService`, a montagem do pedido e a leitura da resposta (já separadas por função) da chamada em si.
//! - **`CancellationToken`.** A chamada síncrona não é interrompível no meio: o token só é conferido antes de
//!   começar, e um token já cancelado rejeita com [`Error::cancellation`] (o `Cancelled` do original).
//! - **`ITranslation`** é o JSON do pacote de tradução (`serde_json::Value`), e o manifesto devolvido por `getManifest`
//!   também é o JSON cru, como em `GalleryApi`.
//! - **`IGalleryExtension`** é [`GalleryExtension`] (sem `telemetryData` e `queryContext`, ver `je-extensions`).
//! - **`query`** devolve um [`IPager`]: a primeira página já vem lida e as demais são consultadas sob demanda.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::paging::IPager;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::fs::ExtFs;
use je_extensions::gallery::{GalleryApi, GalleryResult};
use je_extensions::management::{
    ExtensionGalleryError, ExtensionGalleryErrorCode, ExtensionInfo, ExtensionQueryOptions, ExtensionsControlManifest, GalleryExtension, GalleryExtensionVersion,
    InstallOperation, ProductVersion, QueryOptions, StatisticType,
};
use je_extensions::manifest::{ExtensionIdentifierInfo, TargetPlatform};
use je_instantiation::service_id;
use serde_json::Value;

/// `IGalleryExtension`.
pub type IGalleryExtension = GalleryExtension;

/// `IExtensionGalleryService`.
///
/// Os dois `getExtensions` do original (com e sem `options`) são um só: sem `options` passa-se
/// `ExtensionQueryOptions::default()`.
pub trait IExtensionGalleryService {
    fn is_enabled(&self) -> bool;
    fn query(&self, options: QueryOptions, token: CancellationToken) -> Promise<Rc<dyn IPager<GalleryExtension>>>;
    fn get_extensions(&self, extension_infos: Vec<ExtensionInfo>, options: ExtensionQueryOptions, token: CancellationToken) -> Promise<Vec<GalleryExtension>>;
    fn is_extension_compatible(
        &self,
        extension: &GalleryExtension,
        include_pre_release: bool,
        target_platform: TargetPlatform,
        product_version: Option<ProductVersion>,
    ) -> Promise<bool>;
    fn get_compatible_extension(
        &self,
        extension: &GalleryExtension,
        include_pre_release: bool,
        target_platform: TargetPlatform,
        product_version: Option<ProductVersion>,
    ) -> Promise<Option<GalleryExtension>>;
    fn get_all_compatible_versions(&self, extension_identifier: &ExtensionIdentifierInfo, include_pre_release: bool, target_platform: TargetPlatform) -> Promise<Vec<GalleryExtensionVersion>>;
    fn get_all_versions(&self, extension_identifier: &ExtensionIdentifierInfo) -> Promise<Vec<GalleryExtensionVersion>>;
    fn download(&self, extension: &GalleryExtension, location: &URI, operation: InstallOperation) -> Promise<()>;
    fn download_signature_archive(&self, extension: &GalleryExtension, location: &URI) -> Promise<()>;
    fn report_statistic(&self, publisher: &str, name: &str, version: &str, statistic_type: StatisticType) -> Promise<()>;
    fn get_readme(&self, extension: &GalleryExtension, token: CancellationToken) -> Promise<String>;
    fn get_manifest(&self, extension: &GalleryExtension, token: CancellationToken) -> Promise<Option<Value>>;
    fn get_changelog(&self, extension: &GalleryExtension, token: CancellationToken) -> Promise<String>;
    fn get_core_translation(&self, extension: &GalleryExtension, language_id: &str) -> Promise<Option<Value>>;
    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest>;
}

service_id!(pub I_EXTENSION_GALLERY_SERVICE: dyn IExtensionGalleryService = "extensionGalleryService");

/// O `name` do `ExtensionGalleryError` do original é o próprio código.
fn gallery_code_name(code: ExtensionGalleryErrorCode) -> &'static str {
    match code {
        ExtensionGalleryErrorCode::Timeout => "Timeout",
        ExtensionGalleryErrorCode::Cancelled => "Cancelled",
        ExtensionGalleryErrorCode::ClientError => "ClientError",
        ExtensionGalleryErrorCode::ServerError => "ServerError",
        ExtensionGalleryErrorCode::Failed => "Failed",
        ExtensionGalleryErrorCode::DownloadFailedWriting => "DownloadFailedWriting",
        ExtensionGalleryErrorCode::Offline => "Offline",
    }
}

/// Um `ExtensionGalleryError` como o `Error` que a `Promise` rejeita: `name` é o código, `message` a mensagem, e o
/// código também fica na propriedade `code`. O `Cancelled` vira o erro de cancelamento, que o resto do workbench
/// reconhece por `is_cancellation_error`.
fn to_error(error: &ExtensionGalleryError) -> Error {
    if error.code == ExtensionGalleryErrorCode::Cancelled {
        return Error::cancellation();
    }
    let name = gallery_code_name(error.code);
    Error::named(name, error.message.clone()).with_prop("code", name)
}

/// Assenta a chamada síncrona numa `Promise`; um token já cancelado rejeita antes de chamar.
fn settle<T: Clone + 'static>(token: Option<&CancellationToken>, call: impl FnOnce() -> GalleryResult<T>) -> Promise<T> {
    if token.is_some_and(|token| token.is_cancellation_requested()) {
        return Promise::rejected(Error::cancellation());
    }
    match call() {
        Ok(value) => Promise::resolved(value),
        Err(error) => Promise::rejected(to_error(&error)),
    }
}

/// O caminho de uma URI de arquivo, que é o que o `ExtFs` do `je-extensions` recebe.
fn uri_path(uri: &URI) -> String {
    uri.path().to_string()
}

/// O `IPager` da consulta: a primeira página vem lida e as outras são pedidas ao serviço pelo índice.
struct GalleryPager {
    service: Rc<dyn GalleryApi>,
    options: QueryOptions,
    first_page: Vec<GalleryExtension>,
    total: usize,
    page_size: usize,
}

impl IPager<GalleryExtension> for GalleryPager {
    fn first_page(&self) -> Vec<GalleryExtension> {
        self.first_page.clone()
    }

    fn total(&self) -> usize {
        self.total
    }

    fn page_size(&self) -> usize {
        self.page_size
    }

    fn get_page(&self, page_index: usize, cancellation_token: CancellationToken) -> Promise<Vec<GalleryExtension>> {
        if page_index == 0 {
            return settle(Some(&cancellation_token), || Ok(self.first_page.clone()));
        }
        settle(Some(&cancellation_token), || self.service.query_page(&self.options, page_index as u32))
    }
}

/// O [`IExtensionGalleryService`] sobre o cliente do marketplace de `je-extensions` (ver a nota do módulo).
pub struct AsyncExtensionGalleryService {
    service: Rc<dyn GalleryApi>,
    fs: Rc<dyn ExtFs>,
}

impl AsyncExtensionGalleryService {
    /// `fs` é onde `download` e `download_signature_archive` gravam o arquivo.
    pub fn new(service: Rc<dyn GalleryApi>, fs: Rc<dyn ExtFs>) -> Self {
        Self { service, fs }
    }
}

impl IExtensionGalleryService for AsyncExtensionGalleryService {
    fn is_enabled(&self) -> bool {
        self.service.is_enabled()
    }

    fn query(&self, options: QueryOptions, token: CancellationToken) -> Promise<Rc<dyn IPager<GalleryExtension>>> {
        let service = self.service.clone();
        settle(Some(&token), move || {
            let page = service.query(&options)?;
            let pager: Rc<dyn IPager<GalleryExtension>> = Rc::new(GalleryPager {
                service: service.clone(),
                options: options.clone(),
                first_page: page.first_page,
                total: page.total as usize,
                page_size: page.page_size as usize,
            });
            Ok(pager)
        })
    }

    fn get_extensions(&self, extension_infos: Vec<ExtensionInfo>, options: ExtensionQueryOptions, token: CancellationToken) -> Promise<Vec<GalleryExtension>> {
        settle(Some(&token), || self.service.get_extensions(&extension_infos, &options))
    }

    fn is_extension_compatible(
        &self,
        extension: &GalleryExtension,
        include_pre_release: bool,
        target_platform: TargetPlatform,
        product_version: Option<ProductVersion>,
    ) -> Promise<bool> {
        Promise::resolved(self.service.is_extension_compatible(extension, include_pre_release, target_platform, product_version.as_ref()))
    }

    fn get_compatible_extension(
        &self,
        extension: &GalleryExtension,
        include_pre_release: bool,
        target_platform: TargetPlatform,
        product_version: Option<ProductVersion>,
    ) -> Promise<Option<GalleryExtension>> {
        settle(None, || self.service.get_compatible_extension(extension, include_pre_release, target_platform, product_version.as_ref()))
    }

    fn get_all_compatible_versions(&self, extension_identifier: &ExtensionIdentifierInfo, include_pre_release: bool, target_platform: TargetPlatform) -> Promise<Vec<GalleryExtensionVersion>> {
        settle(None, || self.service.get_all_compatible_versions(extension_identifier, include_pre_release, target_platform))
    }

    fn get_all_versions(&self, extension_identifier: &ExtensionIdentifierInfo) -> Promise<Vec<GalleryExtensionVersion>> {
        settle(None, || self.service.get_all_versions(extension_identifier))
    }

    fn download(&self, extension: &GalleryExtension, location: &URI, operation: InstallOperation) -> Promise<()> {
        settle(None, || self.service.download(extension, &uri_path(location), operation, &*self.fs))
    }

    fn download_signature_archive(&self, extension: &GalleryExtension, location: &URI) -> Promise<()> {
        settle(None, || self.service.download_signature_archive(extension, &uri_path(location), &*self.fs))
    }

    fn report_statistic(&self, publisher: &str, name: &str, version: &str, statistic_type: StatisticType) -> Promise<()> {
        self.service.report_statistic(publisher, name, version, statistic_type);
        Promise::resolved(())
    }

    fn get_readme(&self, extension: &GalleryExtension, token: CancellationToken) -> Promise<String> {
        settle(Some(&token), || self.service.get_readme(extension))
    }

    fn get_manifest(&self, extension: &GalleryExtension, token: CancellationToken) -> Promise<Option<Value>> {
        settle(Some(&token), || self.service.get_manifest(extension))
    }

    fn get_changelog(&self, extension: &GalleryExtension, token: CancellationToken) -> Promise<String> {
        settle(Some(&token), || self.service.get_changelog(extension))
    }

    fn get_core_translation(&self, extension: &GalleryExtension, language_id: &str) -> Promise<Option<Value>> {
        settle(None, || self.service.get_core_translation(extension, language_id))
    }

    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        settle(None, || self.service.get_extensions_control_manifest())
    }
}
