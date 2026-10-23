//! Porte de `src/vs/platform/extensionManagement/test/node/extensionDownloader.test.ts` (suíte `ExtensionDownloader Tests`), caso a caso e na mesma ordem.
//!
//! Divergências: o `InMemoryFileSystemProvider` é o [`MemFs`]; a galeria de mentira (`writes_downloads`) escreve
//! `extension vsix` e `extension signature` nos destinos; `TestExtensionDownloader.validate` (que aceita tudo) é o `with_validator`.

mod common;

use std::rc::Rc;

use common::{FakeGallery, a_gallery_extension};
use je_extensions::downloader::{ExtensionsDownloader, SignatureVerifier, verification_code};
use je_extensions::fs::{ExtFs, MemFs};
use je_extensions::management::{ExtensionManagementError, GalleryExtension, InstallOperation};
use je_extensions::manifest::TargetPlatform;
use je_platform::log::NullLogService;

/// `TestExtensionSignatureVerificationService`: `Some(true)` dá sucesso, `Some(false)` não executou, texto é o código.
enum Verification {
    Success,
    NotExecuted,
    Code(&'static str),
}

struct TestExtensionSignatureVerificationService(Verification);

impl SignatureVerifier for TestExtensionSignatureVerificationService {
    fn verify(&self, _: &str, _: &str, _: &str, _: &str, _: Option<TargetPlatform>) -> Result<Option<String>, ExtensionManagementError> {
        Ok(match &self.0 {
            Verification::Success => Some(verification_code::SUCCESS.to_owned()),
            Verification::NotExecuted => None,
            Verification::Code(code) => Some((*code).to_owned()),
        })
    }
}

fn a_test_object(verification: Verification) -> ExtensionsDownloader {
    let fs: Rc<dyn ExtFs> = Rc::new(MemFs::new());
    ExtensionsDownloader::new(
        "/tests/CachedExtensionVSIXs",
        fs,
        Rc::new(FakeGallery { enabled: true, writes_downloads: true }),
        Some(Rc::new(TestExtensionSignatureVerificationService(verification))),
        NullLogService::shared(),
    )
    .with_validator(Box::new(|_, _, _| Ok(())))
}

fn a_signed(signed: bool) -> GalleryExtension {
    let mut extension = a_gallery_extension(None);
    extension.is_signed = signed;
    extension
}

#[test]
fn download_completes_successfully_if_verification_is_disabled_by_options() {
    let test_object = a_test_object(Verification::Code("error"));
    let actual = test_object.download(&a_signed(true), InstallOperation::Install, false, None).unwrap();
    assert_eq!(actual.verification_status, None);
}

#[test]
fn download_completes_successfully_if_verification_is_disabled_because_the_module_is_not_loaded() {
    let test_object = a_test_object(Verification::NotExecuted);
    let actual = test_object.download(&a_signed(true), InstallOperation::Install, true, None).unwrap();
    assert_eq!(actual.verification_status, None);
}

#[test]
fn download_completes_successfully_if_verification_fails_to_execute() {
    let error_code = "ENOENT";
    let test_object = a_test_object(Verification::Code(error_code));
    let actual = test_object.download(&a_signed(true), InstallOperation::Install, true, None).unwrap();
    assert_eq!(actual.verification_status.as_deref(), Some(error_code));
}

#[test]
fn download_completes_successfully_if_verification_fails() {
    let error_code = "IntegrityCheckFailed";
    let test_object = a_test_object(Verification::Code(error_code));
    let actual = test_object.download(&a_signed(true), InstallOperation::Install, true, None).unwrap();
    assert_eq!(actual.verification_status.as_deref(), Some(error_code));
}

#[test]
fn download_completes_successfully_if_verification_succeeds() {
    let test_object = a_test_object(Verification::Success);
    let actual = test_object.download(&a_signed(true), InstallOperation::Install, true, None).unwrap();
    assert_eq!(actual.verification_status.as_deref(), Some(verification_code::SUCCESS));
}

#[test]
fn download_completes_successfully_for_unsigned_extension() {
    let test_object = a_test_object(Verification::Success);
    let actual = test_object.download(&a_signed(false), InstallOperation::Install, true, None).unwrap();
    assert_eq!(actual.verification_status.as_deref(), Some(verification_code::NOT_SIGNED));
}

#[test]
fn download_completes_successfully_for_an_unsigned_extension_even_when_signature_verification_throws_error() {
    let test_object = a_test_object(Verification::Code("error"));
    let actual = test_object.download(&a_signed(false), InstallOperation::Install, true, None).unwrap();
    assert_eq!(actual.verification_status.as_deref(), Some(verification_code::NOT_SIGNED));
}
