//! Montagem compartilhada dos testes que usam o serviço de gerenciamento: um `MemFs`, o scanner, o scanner de perfil, o
//! serviço de extensões permitidas e uma galeria de mentira.
#![allow(dead_code)]

use std::cell::RefCell;
use std::io::{Cursor, Write};
use std::rc::Rc;

use je_extensions::allowed::AllowedExtensionsService;
use je_extensions::fs::{ExtFs, MemFs};
use je_extensions::gallery::{GalleryApi, GalleryPage, GalleryResult};
use je_extensions::management::{
    ExtensionBlockingInfo, ExtensionInfo, ExtensionQueryOptions, ExtensionsControlManifest, GalleryExtension, GalleryExtensionAsset, GalleryExtensionAssets,
    GalleryExtensionProperties, GalleryExtensionVersion, InstallOperation, ProductVersion, QueryOptions, StatisticType,
};
use je_extensions::management_service::{ExtensionManagementService, ExtensionManagementServiceParts};
use je_extensions::manifest::{ExtensionIdentifierInfo, TargetPlatform};
use je_extensions::paths::join_path;
use je_extensions::profile::UserDataProfiles;
use je_extensions::profile_scanner::ExtensionsProfileScannerService;
use je_extensions::scanner::{
    ExtensionsProduct, ExtensionsScannerService, ExtensionsScannerServiceParts, ScannerEnvironment, ScannerLocations, TranslationsMap,
};
use je_platform::log::NullLogService;
use serde_json::{Value, json};

pub const ROOT: &str = "/ROOT";

/// A galeria de `mock<IExtensionGalleryService>()`: o que o teste não configura não é chamado.
pub struct FakeGallery {
    pub enabled: bool,
    /// Os downloads escrevem `extension vsix` e `extension signature` no destino (a galeria do teste do downloader).
    pub writes_downloads: bool,
}

impl FakeGallery {
    pub fn new(enabled: bool) -> Self {
        Self { enabled, writes_downloads: false }
    }
}

impl GalleryApi for FakeGallery {
    fn is_enabled(&self) -> bool {
        self.enabled
    }
    fn get_extensions(&self, _: &[ExtensionInfo], _: &ExtensionQueryOptions) -> GalleryResult<Vec<GalleryExtension>> {
        Ok(vec![])
    }
    fn is_extension_compatible(&self, _: &GalleryExtension, _: bool, _: TargetPlatform, _: Option<&ProductVersion>) -> bool {
        true
    }
    fn get_compatible_extension(&self, extension: &GalleryExtension, _: bool, _: TargetPlatform, _: Option<&ProductVersion>) -> GalleryResult<Option<GalleryExtension>> {
        Ok(Some(extension.clone()))
    }
    fn query(&self, _: &QueryOptions) -> GalleryResult<GalleryPage> {
        Ok(GalleryPage { first_page: vec![], total: 0, page_size: 0 })
    }
    fn query_page(&self, _: &QueryOptions, _: u32) -> GalleryResult<Vec<GalleryExtension>> {
        Ok(vec![])
    }
    fn download(&self, _: &GalleryExtension, location: &str, _: InstallOperation, fs: &dyn ExtFs) -> GalleryResult<()> {
        if self.writes_downloads {
            fs.write(location, b"extension vsix").unwrap();
        }
        Ok(())
    }
    fn download_signature_archive(&self, _: &GalleryExtension, location: &str, fs: &dyn ExtFs) -> GalleryResult<()> {
        if self.writes_downloads {
            fs.write(location, b"extension signature").unwrap();
        }
        Ok(())
    }
    fn report_statistic(&self, _: &str, _: &str, _: &str, _: StatisticType) {}
    fn get_readme(&self, _: &GalleryExtension) -> GalleryResult<String> {
        Ok(String::new())
    }
    fn get_changelog(&self, _: &GalleryExtension) -> GalleryResult<String> {
        Ok(String::new())
    }
    fn get_manifest(&self, _: &GalleryExtension) -> GalleryResult<Option<Value>> {
        Ok(None)
    }
    fn get_core_translation(&self, _: &GalleryExtension, _: &str) -> GalleryResult<Option<Value>> {
        Ok(None)
    }
    fn get_all_versions(&self, _: &ExtensionIdentifierInfo) -> GalleryResult<Vec<GalleryExtensionVersion>> {
        Ok(vec![])
    }
    fn get_all_compatible_versions(&self, _: &ExtensionIdentifierInfo, _: bool, _: TargetPlatform) -> GalleryResult<Vec<GalleryExtensionVersion>> {
        Ok(vec![])
    }
    fn get_extensions_control_manifest(&self) -> GalleryResult<ExtensionsControlManifest> {
        Ok(ExtensionsControlManifest::default())
    }
}

pub struct Env {
    pub fs: Rc<MemFs>,
    pub management: Rc<ExtensionManagementService>,
    pub scanner: Rc<ExtensionsScannerService>,
    pub profile_scanner: Rc<ExtensionsProfileScannerService>,
    pub profiles: UserDataProfiles,
    pub user_dir: String,
}

pub fn setup(gallery: Rc<dyn GalleryApi>, allowed_config: Option<Value>) -> Env {
    let fs = Rc::new(MemFs::new());
    let system = join_path(ROOT, &["system"]);
    let user_dir = join_path(ROOT, &["extensions"]);
    let profiles = UserDataProfiles::with_default(&user_dir, ROOT, &join_path(ROOT, &["cache"]));
    let logger = NullLogService::shared();
    let profile_scanner = Rc::new(ExtensionsProfileScannerService::new(&user_dir, fs.clone(), profiles.clone(), logger.clone()));
    let product = ExtensionsProduct { version: "1.100.0".to_owned(), name_long: "Code".to_owned(), ..ExtensionsProduct::default() };
    let scanner = Rc::new(ExtensionsScannerService::new(ExtensionsScannerServiceParts {
        locations: ScannerLocations {
            system_extensions_location: system.clone(),
            user_extensions_location: user_dir.clone(),
            extensions_control_location: join_path(ROOT, &[".vscode-oss-dev", "extensions", "control.json"]),
            dev_builtin_extensions_location: join_path(ROOT, &[".build", "builtInExtensions"]),
        },
        environment: ScannerEnvironment { is_built: true, language: "en".to_owned(), ..ScannerEnvironment::default() },
        product: product.clone(),
        current_profile: profiles.default_profile.clone(),
        profiles: profiles.clone(),
        fs: fs.clone(),
        logger: logger.clone(),
        profile_scanner: profile_scanner.clone(),
        get_translations: Box::new(|_| TranslationsMap::new()),
    }));
    fs.create_dir_all(&system).unwrap();
    fs.create_dir_all(&user_dir).unwrap();
    let config = Rc::new(RefCell::new(allowed_config));
    let allowed = Rc::new(AllowedExtensionsService::new(&[], move || config.borrow().clone()));
    let management = Rc::new(ExtensionManagementService::new(ExtensionManagementServiceParts {
        fs: fs.clone(),
        logger,
        scanner: scanner.clone(),
        profile_scanner: profile_scanner.clone(),
        profiles: profiles.clone(),
        current_profile_location: profiles.default_profile.extensions_resource.clone(),
        product,
        allowed,
        gallery,
        downloads_dir: join_path(ROOT, &["cache", "CachedExtensionVSIXs"]),
        target_platform: TargetPlatform::LinuxX64,
        global_storage_home: join_path(ROOT, &["User", "globalStorage"]),
    }));
    Env { fs, management, scanner, profile_scanner, profiles, user_dir }
}

pub fn a_gallery_extension(blocking_info: Option<ExtensionBlockingInfo>) -> GalleryExtension {
    let asset = || GalleryExtensionAsset { uri: String::new(), fallback_uri: String::new() };
    GalleryExtension {
        name: "extension".to_owned(),
        identifier: ExtensionIdentifierInfo::with_uuid("publisher.extension", "extension-uuid"),
        version: "1.0.0".to_owned(),
        display_name: "Extension".to_owned(),
        publisher_id: "publisher-id".to_owned(),
        publisher: "publisher".to_owned(),
        publisher_display_name: "Publisher".to_owned(),
        publisher_domain: None,
        publisher_link: None,
        publisher_sponsor_link: None,
        description: String::new(),
        install_count: 0.0,
        rating: 0.0,
        rating_count: 0.0,
        categories: vec![],
        tags: vec![],
        release_date: 0.0,
        last_updated: 0.0,
        preview: false,
        private: false,
        blocking_info,
        has_pre_release_version: false,
        has_release_version: true,
        is_signed: false,
        all_target_platforms: vec![TargetPlatform::Universal],
        assets: GalleryExtensionAssets { manifest: None, readme: None, changelog: None, license: None, repository: None, download: asset(), icon: None, signature: None, core_translations: vec![] },
        properties: GalleryExtensionProperties {
            dependencies: Some(vec![]),
            extension_pack: None,
            engine: None,
            enabled_api_proposals: None,
            localized_languages: None,
            target_platform: TargetPlatform::Universal,
            is_pre_release_version: false,
            executes_code: None,
        },
        details_link: None,
        rating_link: None,
        support_link: None,
    }
}

/// Monta um VSIX: `extension.vsixmanifest`, `[Content_Types].xml` e `extension/<arquivos>`.
pub fn build_vsix(manifest: &Value, extra_files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("extension.vsixmanifest", options).unwrap();
    writer.write_all(b"<PackageManifest/>").unwrap();
    writer.start_file("[Content_Types].xml", options).unwrap();
    writer.write_all(b"<Types/>").unwrap();
    writer.start_file("extension/package.json", options).unwrap();
    writer.write_all(manifest.to_string().as_bytes()).unwrap();
    for (name, contents) in extra_files {
        writer.start_file(format!("extension/{name}"), options).unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

pub fn a_manifest(publisher: &str, name: &str, version: &str) -> Value {
    json!({ "name": name, "publisher": publisher, "version": version, "engines": { "vscode": "^1.50.0" }, "main": "./out/main.js", "activationEvents": ["*"] })
}
