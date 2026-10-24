//! Porte de `src/vs/platform/uriIdentity/test/common/uriIdentityService.test.ts` (suíte `URI Identity`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - **`FakeFileService`.** É o `mock<IFileService>()` do original com `onDidChangeFileSystemProviderCapabilities`,
//!   `onDidChangeFileSystemProviderRegistrations`, `hasProvider` e `hasCapability` sobrescritos. Os demais métodos do
//!   `IFileService` não existem no `mock` (chamá-los lança `TypeError`), então aqui falham com um pânico.
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O `setup` é declarado antes do
//!   `ensure...`, então o serviço criado nele não é rastreado, e o `teardown` (`_service.dispose()`) roda antes da
//!   conferência de vazamento. É o [`Fixture`]: o serviço nasce antes do guarda, e o `Drop` o descarta antes de o guarda
//!   conferir.
//! - **Identidade.** `assert.strictEqual(a, b)` sobre URIs (a mesma instância) é [`URI::ptr_eq`];
//!   `assert.deepStrictEqual` compara o texto das URIs, e `Math.floor`/`Math.ceil` de potências de dois são exatos.

mod common;

use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::Injectable;
use je_platform::files::files::{
    FileChangesEvent, FileContent, FileStatWithMetadata, FileSystemProviderCapabilities, FileSystemProviderCapabilitiesChangeEvent, FileSystemProviderRef,
    FileSystemProviderRegistrationEvent, ReadFileOptions, ResolveFileOptions,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;

/// `FakeFileService extends mock<IFileService>()`.
struct FakeFileService {
    data: HashMap<String, FileSystemProviderCapabilities>,
}

impl FakeFileService {
    fn not_mocked(method: &str) -> ! {
        errors::throw(Error::type_error(format!("this.fileService.{method} is not a function")))
    }
}

impl IFileService for FakeFileService {
    fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        Self::not_mocked("onDidFilesChange")
    }

    fn get_provider(&self, _scheme: &str) -> Option<FileSystemProviderRef> {
        Self::not_mocked("getProvider")
    }

    fn resolve_with_metadata(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        Self::not_mocked("resolve")
    }

    fn exists(&self, _resource: &URI) -> Promise<bool> {
        Self::not_mocked("exists")
    }

    fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        Event::none()
    }

    fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        Event::none()
    }

    fn has_provider(&self, uri: &URI) -> bool {
        self.data.contains_key(&uri.scheme().to_string_lossy())
    }

    fn has_capability(&self, uri: &URI, flag: FileSystemProviderCapabilities) -> bool {
        let mask = self.data.get(&uri.scheme().to_string_lossy()).copied().unwrap_or(FileSystemProviderCapabilities::NONE);
        mask.intersects(flag)
    }

    fn read_file(&self, _resource: &URI, _options: Option<ReadFileOptions>, _token: Option<CancellationToken>) -> Promise<FileContent> {
        Self::not_mocked("readFile")
    }

    fn resolve(&self, _resource: &URI, _options: Option<ResolveFileOptions>) -> Promise<je_platform::files::files::FileStat> {
        Self::not_mocked("resolve")
    }

    fn create_folder(&self, _resource: &URI) -> Promise<FileStatWithMetadata> {
        Self::not_mocked("createFolder")
    }

    fn watch(&self, _resource: &URI, _options: Option<je_platform::files::files::WatchOptions>) -> je_base::lifecycle::DisposableRef {
        Self::not_mocked("watch")
    }
}

/// O `setup` (o serviço) e o `ensureNoDisposablesAreLeakedInTestSuite()` (o guarda), com o `teardown` no `Drop`.
struct Fixture {
    service: Rc<UriIdentityService>,
    _guard: LeakGuard,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.service.dispose();
    }
}

fn fixture() -> Fixture {
    // setup
    let file_service = FakeFileService {
        data: HashMap::from([
            ("bar".to_string(), FileSystemProviderCapabilities::PATH_CASE_SENSITIVE),
            ("foo".to_string(), FileSystemProviderCapabilities::NONE),
        ]),
    };
    let service = UriIdentityService::create((), (Rc::new(file_service),));
    // ensureNoDisposablesAreLeakedInTestSuite()
    Fixture { service, _guard: ensure_no_disposables_are_leaked_in_test_suite() }
}

fn parse(text: &str) -> URI {
    URI::parse(text, false)
}

fn text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn with_fragment(uri: &URI, fragment: &str) -> URI {
    uri.with(&UriChange { fragment: Field::Set(fragment.into()), ..UriChange::default() })
}

fn get_uri(i: usize) -> URI {
    parse(&format!("foo://bar/{i}"))
}

fn assert_canonical(service: &UriIdentityService, input: &URI, expected: &URI) {
    let actual = service.as_canonical_uri(input);
    assert_eq!(text(&actual), text(expected));
    assert!(service.ext_uri().is_equal(Some(&actual), Some(expected), false));
}

#[test]
fn ext_uri_is_equal() {
    let fixture = fixture();
    let service = &fixture.service;

    let a = parse("foo://bar/bang");
    let a1 = parse("foo://bar/BANG");
    let b = parse("bar://bar/bang");
    let b1 = parse("bar://bar/BANG");

    assert!(service.ext_uri().is_equal(Some(&a), Some(&a1), false));
    assert!(service.ext_uri().is_equal(Some(&a1), Some(&a), false));

    assert!(!service.ext_uri().is_equal(Some(&b), Some(&b1), false));
    assert!(!service.ext_uri().is_equal(Some(&b1), Some(&b), false));
}

#[test]
fn as_canonical_uri_casing() {
    let fixture = fixture();
    let service = &fixture.service;

    let a = parse("foo://bar/bang");
    let a1 = parse("foo://bar/BANG");
    let b = parse("bar://bar/bang");
    let b1 = parse("bar://bar/BANG");

    assert_canonical(service, &a, &a);
    assert_canonical(service, &a1, &a);

    assert_canonical(service, &b, &b);
    assert_canonical(service, &b1, &b1); // case sensitive
}

#[test]
fn as_canonical_uri_normalization() {
    let fixture = fixture();
    let service = &fixture.service;

    let a = parse("foo://bar/bang");
    assert_canonical(service, &a, &a);
    assert_canonical(service, &parse("foo://bar/./bang"), &a);
    assert_canonical(service, &parse("foo://bar/./bang"), &a);
    assert_canonical(service, &parse("foo://bar/./foo/../bang"), &a);
}

#[test]
fn as_canonical_uri_keep_fragement() {
    let fixture = fixture();
    let service = &fixture.service;

    let a = parse("foo://bar/bang");

    assert_canonical(service, &a, &a);
    assert_canonical(service, &parse("foo://bar/./bang#frag"), &with_fragment(&a, "frag"));
    assert_canonical(service, &parse("foo://bar/./bang#frag"), &with_fragment(&a, "frag"));
    assert_canonical(service, &parse("foo://bar/./bang#frag"), &with_fragment(&a, "frag"));
    assert_canonical(service, &parse("foo://bar/./foo/../bang#frag"), &with_fragment(&a, "frag"));

    let b = parse("foo://bar/bazz#frag");
    assert_canonical(service, &b, &b);
    assert_canonical(service, &parse("foo://bar/bazz"), &with_fragment(&b, ""));
    assert_canonical(service, &parse("foo://bar/BAZZ#DDD"), &with_fragment(&b, "DDD")); // lower-case path, but fragment is kept
}

#[test]
fn perf_clears_cache_when_overflown_with_respect_to_access_time() {
    let fixture = fixture();
    let service = &fixture.service;

    const CACHE_SIZE: usize = 1 << 16;

    const FIRST: usize = 0;
    const SECOND: usize = 1;
    let first_cached = service.as_canonical_uri(&get_uri(FIRST));
    let second_cached = service.as_canonical_uri(&get_uri(SECOND));
    for i in 2..CACHE_SIZE - 1 {
        service.as_canonical_uri(&get_uri(i));
    }

    // Assert that the first URI is still the same object.
    assert!(URI::ptr_eq(&service.as_canonical_uri(&get_uri(FIRST)), &first_cached));

    // Clear the cache.
    service.as_canonical_uri(&get_uri(CACHE_SIZE - 1));

    // First URI should still be the same object.
    assert!(URI::ptr_eq(&service.as_canonical_uri(&get_uri(FIRST)), &first_cached));
    // But the second URI should be a new object, since it was evicted.
    assert!(!URI::ptr_eq(&service.as_canonical_uri(&get_uri(SECOND)), &second_cached));
}

#[test]
fn perf_preserves_order_of_access_time_on_cache_cleanup() {
    let fixture = fixture();
    let service = &fixture.service;

    const SIZE: usize = 1 << 16;

    const FIRST: usize = 0;
    let first_cached = service.as_canonical_uri(&get_uri(FIRST));
    for i in 1..SIZE - 2 {
        service.as_canonical_uri(&get_uri(i));
    }
    const LAST: usize = SIZE - 2;
    let last_cached = service.as_canonical_uri(&get_uri(LAST));

    // Clear the cache.
    service.as_canonical_uri(&get_uri(SIZE - 1));

    // Batch 2
    const BATCH2_FIRST: usize = SIZE;
    let batch2_first_cached = service.as_canonical_uri(&get_uri(BATCH2_FIRST));
    const BATCH2_SECOND: usize = SIZE + 1;
    let batch2_second_cached = service.as_canonical_uri(&get_uri(BATCH2_SECOND));
    const BATCH2_THIRD: usize = SIZE + 2;
    let batch2_third_cached = service.as_canonical_uri(&get_uri(BATCH2_THIRD));
    for i in SIZE + 3..SIZE + SIZE / 2 - 1 {
        service.as_canonical_uri(&get_uri(i));
    }
    const BATCH2_LAST: usize = SIZE + SIZE / 2;
    let batch2_last_cached = service.as_canonical_uri(&get_uri(BATCH2_LAST));

    // Clean up the cache.
    service.as_canonical_uri(&get_uri(SIZE + SIZE.div_ceil(2) + 1));

    // Both URIs from the first batch should be evicted.
    assert!(!URI::ptr_eq(&service.as_canonical_uri(&get_uri(FIRST)), &first_cached));
    assert!(!URI::ptr_eq(&service.as_canonical_uri(&get_uri(LAST)), &last_cached));

    // But the URIs from the second batch should still be the same objects.
    // Except for the first one, which is removed as a median value.
    assert!(!URI::ptr_eq(&service.as_canonical_uri(&get_uri(BATCH2_FIRST)), &batch2_first_cached));
    assert_eq!(text(&service.as_canonical_uri(&get_uri(BATCH2_SECOND))), text(&batch2_second_cached));
    assert_eq!(text(&service.as_canonical_uri(&get_uri(BATCH2_THIRD))), text(&batch2_third_cached));
    assert_eq!(text(&service.as_canonical_uri(&get_uri(BATCH2_LAST))), text(&batch2_last_cached));
}

#[test]
fn perf_cpu_pegged_after_some_builds_194853() {
    let fixture = fixture();
    let service = &fixture.service;

    let n = 100 + (1usize << 16);
    for i in 0..n {
        let uri = parse(&format!("foo://bar/{i}"));
        let uri2 = service.as_canonical_uri(&uri);

        // assert.ok(uri2): a URI é sempre um objeto verdadeiro
        assert!(!uri2.scheme().is_empty());
    }
}
