//! Porte de `src/vs/platform/extensionManagement/node/extensionDownloader.ts`: baixa o VSIX (e o arquivo de assinatura)
//! para a pasta de downloads, com no máximo duas tentativas extras, valida o zip, verifica a assinatura e guarda
//! no máximo 20 VSIX em cache (o mais novo de cada extensão).
//!
//! A verificação de assinatura (`IExtensionSignatureVerificationService`) é o trait [`SignatureVerifier`]. O VS Code
//! a implementa com o módulo nativo `@vscode/vsce-sign`; o je-code ainda não tem implementação (ver o
//! `TODO(integração)` em [`crate::management_service`]), e o chamador passa `None`, que é o caso "módulo não carregado".

use std::io::Cursor;
use std::rc::Rc;

use je_platform::log::Logger;

use crate::fs::{ExtFs, is_not_found};
use crate::gallery::GalleryApi;
use crate::management::{ExtensionManagementError, ExtensionManagementErrorCode, GalleryExtension, InstallOperation, to_extension_management_error};
use crate::manifest::TargetPlatform;
use crate::paths;
use crate::util::{ExtensionKey, group_by_extension};
use crate::vsix;

const SIGNATURE_ARCHIVE_EXTENSION: &str = ".sigzip";
const CORRUPT_ZIP_MESSAGE: &str = "Corrupted ZIP";

/// `ExtensionSignatureVerificationCode`: os códigos que importam ao fluxo. O resultado de um verificador é um texto
/// livre (o original aceita códigos fora do enum, como `ENOENT`).
pub mod verification_code {
    pub const NOT_SIGNED: &str = "NotSigned";
    pub const SUCCESS: &str = "Success";
    pub const PACKAGE_IS_INVALID_ZIP: &str = "PackageIsInvalidZip";
    pub const SIGNATURE_ARCHIVE_IS_INVALID_ZIP: &str = "SignatureArchiveIsInvalidZip";
}

/// `IExtensionSignatureVerificationService`.
pub trait SignatureVerifier {
    /// `None` quando a verificação não executou (o módulo nativo não está carregado).
    fn verify(&self, extension_id: &str, version: &str, vsix_path: &str, signature_archive_path: &str, client_target_platform: Option<TargetPlatform>) -> Result<Option<String>, ExtensionManagementError>;
}

/// O resultado de [`ExtensionsDownloader::download`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadedExtension {
    pub location: String,
    pub verification_status: Option<String>,
}

/// `validate(zipPath, filePath)`: o zip abre e tem a entrada.
pub type ZipValidator = Box<dyn Fn(&dyn ExtFs, &str, &str) -> Result<(), ExtensionManagementError>>;

fn default_validator(fs: &dyn ExtFs, zip_path: &str, entry: &str) -> Result<(), ExtensionManagementError> {
    let data = fs.read(zip_path).map_err(|e| to_extension_management_error(&e, ExtensionManagementErrorCode::Extract))?;
    vsix::read_entry(Cursor::new(data), entry).map(|_| ())
}

/// `ExtensionsDownloader`.
pub struct ExtensionsDownloader {
    pub extensions_download_dir: String,
    extensions_trash_dir: String,
    cache: usize,
    fs: Rc<dyn ExtFs>,
    gallery: Rc<dyn GalleryApi>,
    verifier: Option<Rc<dyn SignatureVerifier>>,
    logger: Rc<dyn Logger>,
    validate: ZipValidator,
}

impl ExtensionsDownloader {
    pub fn new(extensions_download_dir: &str, fs: Rc<dyn ExtFs>, gallery: Rc<dyn GalleryApi>, verifier: Option<Rc<dyn SignatureVerifier>>, logger: Rc<dyn Logger>) -> Self {
        let downloader = Self {
            extensions_download_dir: extensions_download_dir.to_owned(),
            extensions_trash_dir: paths::join_path(extensions_download_dir, &[".trash"]),
            cache: 20, // Cache 20 downloaded VSIX files
            fs,
            gallery,
            verifier,
            logger,
            validate: Box::new(default_validator),
        };
        downloader.clean_up();
        downloader
    }

    /// Troca a validação do zip (os testes a trocam por uma que aceita tudo).
    pub fn with_validator(mut self, validate: ZipValidator) -> Self {
        self.validate = validate;
        self
    }

    /// `download`.
    pub fn download(&self, extension: &GalleryExtension, operation: InstallOperation, verify_signature: bool, client_target_platform: Option<TargetPlatform>) -> Result<DownloadedExtension, ExtensionManagementError> {
        let location = self.download_vsix(extension, operation)?;
        if !verify_signature {
            return Ok(DownloadedExtension { location, verification_status: None });
        }
        if !extension.is_signed {
            return Ok(DownloadedExtension { location, verification_status: Some(verification_code::NOT_SIGNED.to_owned()) });
        }
        let mut signature_archive: Option<String> = None;
        let outcome = (|| {
            let archive = self.download_signature_archive(extension)?;
            signature_archive = Some(archive.clone());
            let status = match &self.verifier {
                Some(verifier) => verifier.verify(&extension.identifier.id, &extension.version, &location, &archive, client_target_platform)?,
                None => None,
            };
            if matches!(status.as_deref(), Some(verification_code::PACKAGE_IS_INVALID_ZIP | verification_code::SIGNATURE_ARCHIVE_IS_INVALID_ZIP)) {
                if let Err(error) = self.delete(&location) {
                    self.logger.error(&error.to_string());
                }
                return Err(ExtensionManagementError::new(CORRUPT_ZIP_MESSAGE, ExtensionManagementErrorCode::CorruptZip));
            }
            Ok(DownloadedExtension { location: location.clone(), verification_status: status })
        })();
        if outcome.is_err()
            && let Err(error) = self.delete(&location)
        {
            self.logger.error(&error.to_string());
        }
        if let Some(archive) = signature_archive
            && let Err(error) = self.delete(&archive)
        {
            self.logger.error(&error.to_string());
        }
        outcome
    }

    fn download_vsix(&self, extension: &GalleryExtension, operation: InstallOperation) -> Result<String, ExtensionManagementError> {
        let location = paths::join_path(&self.extensions_download_dir, &[&Self::get_name(extension)]);
        self.do_download(extension, "vsix", 2, || {
            self.download_file(extension, &location, |target| {
                self.gallery.download(extension, target, operation, self.fs.as_ref()).map_err(|e| ExtensionManagementError::new(e.message, ExtensionManagementErrorCode::Download))
            })?;
            if let Err(error) = (self.validate)(self.fs.as_ref(), &location, "extension/package.json") {
                if let Err(e) = self.fs.remove(&location, false) {
                    self.logger.warn(&format!("Error while deleting: {location} {e}"));
                }
                return Err(error);
            }
            Ok(())
        })
        .map_err(|e| ExtensionManagementError::new(e.message, ExtensionManagementErrorCode::Download))?;
        Ok(location)
    }

    fn download_signature_archive(&self, extension: &GalleryExtension) -> Result<String, ExtensionManagementError> {
        let location = paths::join_path(&self.extensions_download_dir, &[&format!("{}{SIGNATURE_ARCHIVE_EXTENSION}", Self::get_name(extension))]);
        self.do_download(extension, "sigzip", 2, || {
            self.gallery.download_signature_archive(extension, &location, self.fs.as_ref()).map_err(|e| ExtensionManagementError::new(e.message, ExtensionManagementErrorCode::DownloadSignature))?;
            if let Err(error) = (self.validate)(self.fs.as_ref(), &location, ".signature.p7s") {
                if let Err(e) = self.fs.remove(&location, false) {
                    self.logger.warn(&format!("Error while deleting: {location} {e}"));
                }
                return Err(error);
            }
            Ok(())
        })
        .map_err(|e| ExtensionManagementError::new(e.message, ExtensionManagementErrorCode::DownloadSignature))?;
        Ok(location)
    }

    /// `downloadFile`: baixa para um nome temporário e renomeia, para ninguém ler um arquivo pela metade.
    fn download_file(&self, extension: &GalleryExtension, location: &str, download: impl Fn(&str) -> Result<(), ExtensionManagementError>) -> Result<(), ExtensionManagementError> {
        if self.fs.exists(location) {
            return Ok(());
        }
        let temp_location = paths::join_path(&self.extensions_download_dir, &[&format!(".{}", je_base::uuid::generate_uuid())]);
        if let Err(error) = download(&temp_location) {
            let _ = self.fs.remove(&temp_location, false);
            return Err(error);
        }
        if let Err(error) = self.fs.rename(&temp_location, location) {
            let _ = self.fs.remove(&temp_location, false);
            if self.fs.exists(location) {
                self.logger.info(&format!("Rename failed because the file was downloaded by another source. So ignoring renaming. {} {location}", extension.identifier.id));
            } else {
                self.logger.info(&format!("Rename failed because of {error}. Deleted the file from downloaded location {temp_location}"));
                return Err(to_extension_management_error(&error, ExtensionManagementErrorCode::Rename));
            }
        }
        Ok(())
    }

    fn do_download(&self, extension: &GalleryExtension, name: &str, retries: u32, download: impl Fn() -> Result<(), ExtensionManagementError>) -> Result<u32, ExtensionManagementError> {
        let mut attempts = 1;
        loop {
            match download() {
                Ok(()) => return Ok(attempts),
                Err(error) => {
                    let exhausted = attempts > retries;
                    attempts += 1;
                    if exhausted {
                        return Err(error);
                    }
                    self.logger.warn(&format!("Failed downloading {name}. {error}. Retry again... {}", extension.identifier.id));
                }
            }
        }
    }

    /// `delete`: manda para a lixeira dos downloads (apagada na próxima limpeza) ou apaga.
    pub fn delete(&self, location: &str) -> Result<(), std::io::Error> {
        match location.strip_prefix(&format!("{}/", self.extensions_download_dir.trim_end_matches('/'))) {
            Some(relative) if !relative.is_empty() => {
                let target = paths::join_path(&self.extensions_trash_dir, &[relative]);
                if let Some(parent) = target.rsplit_once('/') {
                    self.fs.create_dir_all(parent.0)?;
                }
                self.fs.remove(&target, true).or_else(|e| if is_not_found(&e) { Ok(()) } else { Err(e) })?;
                self.fs.rename(location, &target)
            }
            _ => self.fs.remove(location, false),
        }
    }

    /// `cleanUp`: esvazia a lixeira, apaga os arquivos de assinatura e as versões antigas, e mantém só `cache` VSIX.
    fn clean_up(&self) {
        let outcome = (|| -> Result<(), std::io::Error> {
            if !self.fs.exists(&self.extensions_download_dir) {
                self.logger.trace("Extension VSIX downloads cache dir does not exist");
                return Ok(());
            }
            if let Err(error) = self.fs.remove(&self.extensions_trash_dir, true)
                && !is_not_found(&error)
            {
                self.logger.error(&error.to_string());
            }
            let mut to_delete: Vec<String> = Vec::new();
            let mut vsixs: Vec<(ExtensionKey, String, u64)> = Vec::new();
            for entry in self.fs.read_dir(&self.extensions_download_dir)? {
                let resource = paths::join_path(&self.extensions_download_dir, &[&entry.name]);
                if entry.name.ends_with(SIGNATURE_ARCHIVE_EXTENSION) {
                    to_delete.push(resource);
                } else if let Some(key) = ExtensionKey::parse(&entry.name) {
                    let mtime = self.fs.stat(&resource)?.mtime;
                    vsixs.push((key, resource, mtime));
                }
            }
            let mut distinct: Vec<(String, u64)> = Vec::new();
            for mut group in group_by_extension(vsixs, |(key, _, _)| &key.identifier) {
                group.sort_by(|a, b| je_base::semver::rcompare(&a.0.version, &b.0.version));
                to_delete.extend(group.iter().skip(1).map(|(_, resource, _)| resource.clone())); // Delete outdated extensions
                distinct.push((group[0].1.clone(), group[0].2));
            }
            distinct.sort_by_key(|(_, mtime)| *mtime); // sort by modified time
            let excess = distinct.len().saturating_sub(self.cache);
            to_delete.extend(distinct.into_iter().take(excess).map(|(resource, _)| resource)); // Retain minimum cacheSize and delete the rest
            for resource in to_delete {
                self.logger.trace(&format!("Deleting from cache {resource}"));
                let _ = self.fs.remove(&resource, false);
            }
            Ok(())
        })();
        if let Err(error) = outcome {
            self.logger.error(&error.to_string());
        }
    }

    fn get_name(extension: &GalleryExtension) -> String {
        ExtensionKey::new(extension.identifier.clone(), extension.version.clone(), extension.properties.target_platform).to_string().to_lowercase()
    }
}
