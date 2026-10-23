//! Porte de `src/vs/platform/extensionManagement/test/common/extensionGalleryService.test.ts` (suíte `Extension Gallery Service`), caso a caso e na mesma ordem.
//!
//! Divergências: o `RecordingRequestService` vira um [`HttpClient`] que grava `(método, url)`; o `TestLogService`
//! que coleta `errors` não existe (o `NullLogService` descarta), então o `errors: []` do terceiro caso de
//! `getExtensions` não é afirmado; as identidades de objeto do JavaScript (`result[0] === release`,
//! `result.includes(x)`) viram igualdade por valor nas versões cruas.

use std::cell::RefCell;
use std::rc::Rc;

use je_extensions::allowed::AllowedExtensionsService;
use je_extensions::fs::MemFs;
use je_extensions::gallery::{
    ExtensionGalleryService, ExtensionGalleryServiceParts, GalleryApi, GalleryManifest, GalleryProduct, RawGalleryExtensionBlocking, RawGalleryExtensionProperty, RawGalleryExtensionVersion,
    filter_latest_extension_versions_for_target_platform, get_blocking_info, resource_type, sort_extension_versions,
};
use je_extensions::http::{HttpClient, HttpError, HttpMethod, HttpRequest, HttpResponse};
use je_extensions::management::{ExtensionBlockingInfo, ExtensionBlockingOrigin, ExtensionInfo, ExtensionQueryOptions, ProductVersion};
use je_extensions::manifest::TargetPlatform;
use je_extensions::marketplace::resolve_marketplace_headers;
use je_platform::log::NullLogService;
use serde_json::{Value, json};

const PRODUCT_VERSION: &str = "1.143.1";
const PRODUCT_DATE: &str = "2026-10-22T17:23:22Z";
const LATEST_VERSION_URI: &str = "https://marketplace.test/_apis/public/gallery/publishers/{publisher}/extensions/{name}/latest";
const QUERY_SERVICE_URI: &str = "https://marketplace.test/_apis/public/gallery/extensionquery";

type Recorded = Rc<RefCell<Vec<(String, String)>>>;

struct RecordingRequestService {
    requests: Recorded,
    response: Box<dyn Fn(&HttpRequest) -> HttpResponse>,
}

impl HttpClient for RecordingRequestService {
    fn request(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let method = if request.method == HttpMethod::Get { "GET" } else { "POST" };
        self.requests.borrow_mut().push((method.to_owned(), request.url.clone()));
        Ok((self.response)(request))
    }
}

fn request_context(status_code: u16, body: Value) -> HttpResponse {
    HttpResponse { status: status_code, headers: vec![], body: body.to_string().into_bytes() }
}

fn gallery_query_response(extensions: Vec<Value>) -> Value {
    json!({
        "results": [{
            "extensions": extensions,
            "resultMetadata": [{ "metadataType": "ResultCount", "metadataItems": [{ "name": "TotalCount", "count": extensions.len() }] }]
        }]
    })
}

fn raw_version(version: &str, flags: Option<&str>, pre_release: bool) -> Value {
    let date = "2026-01-01T00:00:00Z";
    let mut properties = vec![json!({ "key": "Microsoft.VisualStudio.Code.Engine", "value": "*" })];
    if pre_release {
        properties.push(json!({ "key": "Microsoft.VisualStudio.Code.PreRelease", "value": "true" }));
    }
    let mut value = json!({
        "version": version,
        "lastUpdated": date,
        "assetUri": format!("https://marketplace.test/assets/publisher/extension/{version}"),
        "fallbackAssetUri": format!("https://marketplace.test/fallback/publisher/extension/{version}"),
        "files": [],
        "properties": properties,
    });
    if let Some(flags) = flags {
        value["flags"] = json!(flags);
    }
    value
}

fn raw_latest_extension(versions: Vec<Value>) -> Value {
    let date = "2026-01-01T00:00:00Z";
    json!({
        "extensionId": "extension-uuid",
        "extensionName": "extension",
        "displayName": "Extension",
        "shortDescription": "Extension",
        "publisher": { "displayName": "Publisher", "publisherId": "publisher-id", "publisherName": "publisher" },
        "versions": versions,
        "statistics": [],
        "tags": [],
        "releaseDate": date,
        "publishedDate": date,
        "lastUpdated": date,
        "categories": [],
        "flags": ""
    })
}

fn gallery_manifest() -> GalleryManifest {
    let mut manifest = GalleryManifest::from_product(&json!({ "serviceUrl": "https://example.invalid" })).expect("manifesto");
    manifest.resources = vec![
        (LATEST_VERSION_URI.to_owned(), resource_type::EXTENSION_LATEST_VERSION_URI.to_owned()),
        (QUERY_SERVICE_URI.to_owned(), resource_type::EXTENSION_QUERY_SERVICE.to_owned()),
    ];
    manifest
}

/// Sem o recurso do "latest": só a consulta (`capabilities: { extensionQuery: {} }` com os dois recursos do original
/// já os inclui, então este ajudante só existe para os casos que fixam o `GET` ausente).
fn create_service(response: impl Fn(&HttpRequest) -> HttpResponse + 'static) -> (ExtensionGalleryService, Recorded) {
    let requests: Recorded = Rc::new(RefCell::new(Vec::new()));
    let http = Rc::new(RecordingRequestService { requests: requests.clone(), response: Box::new(response) });
    let allowed = Rc::new(AllowedExtensionsService::new(&[], || None));
    let service = ExtensionGalleryService::new(ExtensionGalleryServiceParts {
        manifest: Some(gallery_manifest()),
        http,
        allowed,
        logger: NullLogService::shared(),
        product: GalleryProduct { version: PRODUCT_VERSION.to_owned(), date: Some(PRODUCT_DATE.to_owned()), name_short: "Code".to_owned(), ..GalleryProduct::default() },
        target_platform: TargetPlatform::LinuxX64,
        common_headers: vec![],
        request_timeout: None,
    });
    (service, requests)
}

fn recorded(requests: &Recorded) -> Vec<(String, String)> {
    requests.borrow().clone()
}

fn pair(method: &str, url: &str) -> (String, String) {
    (method.to_owned(), url.to_owned())
}

fn info(id: &str) -> ExtensionInfo {
    ExtensionInfo { id: id.to_owned(), ..ExtensionInfo::default() }
}

#[test]
fn marketplace_machine_id() {
    let fs = MemFs::new();
    let resource = "/tests/machineid";
    let headers = resolve_marketplace_headers(PRODUCT_VERSION, "Code", true, &fs, resource, None);
    let user_id = |headers: &[(String, String)]| headers.iter().find(|(k, _)| k == "X-Market-User-Id").map(|(_, v)| v.clone());
    let first = user_id(&headers).expect("X-Market-User-Id");
    assert!(je_base::uuid::is_uuid(&first));

    let headers2 = resolve_marketplace_headers(PRODUCT_VERSION, "Code", true, &fs, resource, None);
    assert_eq!(Some(first), user_id(&headers2));
}

#[test]
fn get_extensions_uses_query_api_for_extension_info_without_uuid() {
    let (service, requests) = create_service(|request| {
        if request.method == HttpMethod::Post { request_context(200, gallery_query_response(vec![])) } else { request_context(404, json!({})) }
    });

    let extensions = service.get_extensions(&[info("ms-vscode.visualization-runner")], &ExtensionQueryOptions::default()).unwrap();

    assert_eq!((recorded(&requests), extensions.iter().map(|e| e.identifier.id.clone()).collect::<Vec<_>>()), (vec![pair("POST", QUERY_SERVICE_URI)], vec![]));
}

#[test]
fn get_extensions_uses_latest_resource_api_for_extension_info_with_uuid() {
    let (service, requests) = create_service(|request| {
        if request.method == HttpMethod::Get {
            request_context(200, raw_latest_extension(vec![raw_version("1.0.0", None, false)]))
        } else {
            request_context(200, gallery_query_response(vec![]))
        }
    });

    let extensions = service
        .get_extensions(&[ExtensionInfo { id: "publisher.extension".into(), uuid: Some("extension-uuid".into()), ..ExtensionInfo::default() }], &ExtensionQueryOptions::default())
        .unwrap();

    assert_eq!(recorded(&requests), vec![pair("GET", "https://marketplace.test/_apis/public/gallery/publishers/publisher/extensions/extension/latest")]);
    assert_eq!(
        extensions.iter().map(|e| (e.identifier.id.clone(), e.identifier.uuid.clone(), e.version.clone())).collect::<Vec<_>>(),
        vec![("publisher.extension".to_owned(), Some("extension-uuid".to_owned()), "1.0.0".to_owned())]
    );
}

#[test]
fn get_extensions_falls_back_to_query_api_when_latest_resource_response_omits_files() {
    let raw_extension = raw_latest_extension(vec![raw_version("1.0.0", None, false)]);
    let mut invalid_latest = raw_extension.clone();
    for version in invalid_latest["versions"].as_array_mut().unwrap() {
        version.as_object_mut().unwrap().remove("files");
    }
    let (service, requests) = create_service(move |request| {
        if request.method == HttpMethod::Get { request_context(200, invalid_latest.clone()) } else { request_context(200, gallery_query_response(vec![raw_extension.clone()])) }
    });

    let extensions = service
        .get_extensions(&[ExtensionInfo { id: "publisher.extension".into(), uuid: Some("extension-uuid".into()), ..ExtensionInfo::default() }], &ExtensionQueryOptions::default())
        .unwrap();

    assert_eq!(
        recorded(&requests),
        vec![pair("GET", "https://marketplace.test/_apis/public/gallery/publishers/publisher/extensions/extension/latest"), pair("POST", QUERY_SERVICE_URI)]
    );
    assert_eq!(
        extensions.iter().map(|e| (e.identifier.id.clone(), e.identifier.uuid.clone(), e.version.clone())).collect::<Vec<_>>(),
        vec![("publisher.extension".to_owned(), Some("extension-uuid".to_owned()), "1.0.0".to_owned())]
    );
}

#[test]
fn get_extensions_skips_a_blocked_latest_version_in_display_queries() {
    let request_count = Rc::new(RefCell::new(0));
    let counter = request_count.clone();
    let (service, requests) = create_service(move |_| {
        let first = *counter.borrow() == 0;
        *counter.borrow_mut() += 1;
        request_context(
            200,
            gallery_query_response(vec![if first {
                raw_latest_extension(vec![raw_version("2.0.0", Some("blocked"), false)])
            } else {
                raw_latest_extension(vec![raw_version("2.0.0", Some("blocked"), false), raw_version("1.0.0", None, false)])
            }]),
        )
    });

    let extensions = service.get_extensions(&[info("publisher.extension")], &ExtensionQueryOptions { target_platform: Some(TargetPlatform::Universal), ..ExtensionQueryOptions::default() }).unwrap();

    assert_eq!(extensions.iter().map(|e| (e.version.clone(), e.blocking_info)).collect::<Vec<_>>(), vec![("1.0.0".to_owned(), None)]);
    assert_eq!(recorded(&requests), vec![pair("POST", QUERY_SERVICE_URI), pair("POST", QUERY_SERVICE_URI)]);
}

#[test]
fn get_extensions_fallback_skips_a_blocked_latest_version() {
    let request_count = Rc::new(RefCell::new(0));
    let counter = request_count.clone();
    let (service, requests) = create_service(move |_| {
        let first = *counter.borrow() == 0;
        *counter.borrow_mut() += 1;
        request_context(
            200,
            gallery_query_response(vec![if first {
                raw_latest_extension(vec![raw_version("2.0.0", Some("blocked"), true)])
            } else {
                raw_latest_extension(vec![raw_version("2.0.0", Some("blocked"), true), raw_version("1.0.0", None, true)])
            }]),
        )
    });

    let extensions = service.get_extensions(&[info("publisher.extension")], &ExtensionQueryOptions::default()).unwrap();

    assert_eq!(extensions.iter().map(|e| e.version.clone()).collect::<Vec<_>>(), vec!["1.0.0".to_owned()]);
    assert_eq!(recorded(&requests), vec![pair("POST", QUERY_SERVICE_URI), pair("POST", QUERY_SERVICE_URI)]);
}

fn compatible_options() -> ExtensionQueryOptions {
    ExtensionQueryOptions {
        compatible: true,
        target_platform: Some(TargetPlatform::Universal),
        product_version: Some(ProductVersion { version: PRODUCT_VERSION.to_owned(), date: Some(PRODUCT_DATE.to_owned()) }),
        ..ExtensionQueryOptions::default()
    }
}

#[test]
fn get_extensions_skips_a_blocked_latest_version_in_compatible_queries() {
    let raw_extension = raw_latest_extension(vec![raw_version("2.0.0", Some("blocked"), false), raw_version("1.0.0", None, false)]);
    let (service, _requests) = create_service(move |_| request_context(200, gallery_query_response(vec![raw_extension.clone()])));

    let extensions = service.get_extensions(&[info("publisher.extension")], &compatible_options()).unwrap();

    assert_eq!(extensions.iter().map(|e| (e.version.clone(), e.blocking_info)).collect::<Vec<_>>(), vec![("1.0.0".to_owned(), None)]);
}

#[test]
fn get_extensions_returns_no_compatible_extension_when_every_version_is_blocked() {
    let raw_extension = raw_latest_extension(vec![raw_version("2.0.0", Some("blocked"), false), raw_version("1.0.0", Some("blocked"), false)]);
    let (service, requests) = create_service(move |_| request_context(200, gallery_query_response(vec![raw_extension.clone()])));

    let extensions = service.get_extensions(&[info("publisher.extension")], &compatible_options()).unwrap();

    assert!(extensions.is_empty());
    assert_eq!(recorded(&requests), vec![pair("POST", QUERY_SERVICE_URI), pair("POST", QUERY_SERVICE_URI)]);
}

#[test]
fn get_extensions_returns_no_result_for_an_explicitly_requested_blocked_version() {
    let raw_extension = raw_latest_extension(vec![raw_version("2.0.0", Some("blocked"), false), raw_version("1.0.0", None, false)]);
    let (service, _requests) = create_service(move |_| request_context(200, gallery_query_response(vec![raw_extension.clone()])));

    let extensions = service
        .get_extensions(
            &[ExtensionInfo { id: "publisher.extension".into(), version: Some("2.0.0".into()), ..ExtensionInfo::default() }],
            &ExtensionQueryOptions { target_platform: Some(TargetPlatform::Universal), ..ExtensionQueryOptions::default() },
        )
        .unwrap();

    assert!(extensions.is_empty());
}

#[test]
fn get_extensions_can_return_blocking_metadata_for_an_explicitly_requested_version() {
    let raw_extension = raw_latest_extension(vec![raw_version("2.0.0", Some("blocked"), false), raw_version("1.0.0", None, false)]);
    let (service, _requests) = create_service(move |_| request_context(200, gallery_query_response(vec![raw_extension.clone()])));

    let extensions = service
        .get_extensions(
            &[ExtensionInfo { id: "publisher.extension".into(), version: Some("2.0.0".into()), ..ExtensionInfo::default() }],
            &ExtensionQueryOptions { target_platform: Some(TargetPlatform::Universal), include_blocked_versions: true, ..ExtensionQueryOptions::default() },
        )
        .unwrap();

    assert_eq!(
        extensions.iter().map(|e| (e.version.clone(), e.blocking_info)).collect::<Vec<_>>(),
        vec![("2.0.0".to_owned(), Some(ExtensionBlockingInfo { origin: ExtensionBlockingOrigin::Service }))]
    );
}

fn a_extension_version(version: &str, target_platform: Option<TargetPlatform>) -> RawGalleryExtensionVersion {
    RawGalleryExtensionVersion { version: version.to_owned(), target_platform: target_platform.map(|p| p.as_str().to_owned()), ..RawGalleryExtensionVersion::default() }
}

fn a_pre_release_extension_version(version: &str, target_platform: Option<TargetPlatform>) -> RawGalleryExtensionVersion {
    RawGalleryExtensionVersion {
        properties: Some(vec![RawGalleryExtensionProperty { key: "Microsoft.VisualStudio.Code.PreRelease".into(), value: "true".into() }]),
        ..a_extension_version(version, target_platform)
    }
}

fn v(version: &str) -> RawGalleryExtensionVersion {
    a_extension_version(version, None)
}

fn vp(version: &str, target_platform: TargetPlatform) -> RawGalleryExtensionVersion {
    a_extension_version(version, Some(target_platform))
}

fn pre(version: &str) -> RawGalleryExtensionVersion {
    a_pre_release_extension_version(version, None)
}

fn prep(version: &str, target_platform: TargetPlatform) -> RawGalleryExtensionVersion {
    a_pre_release_extension_version(version, Some(target_platform))
}

#[test]
fn sorting_single_extension_version_without_target_platform() {
    let mut actual = vec![v("1.1.2")];
    let expected = actual.clone();
    sort_extension_versions(&mut actual, TargetPlatform::DarwinX64);
    assert_eq!(actual, expected);
}

#[test]
fn sorting_single_extension_version_with_preferred_target_platform() {
    let mut actual = vec![vp("1.1.2", TargetPlatform::DarwinX64)];
    let expected = actual.clone();
    sort_extension_versions(&mut actual, TargetPlatform::DarwinX64);
    assert_eq!(actual, expected);
}

#[test]
fn sorting_single_extension_version_with_not_compatible_target_platform() {
    let mut actual = vec![vp("1.1.2", TargetPlatform::DarwinArm64)];
    let expected = actual.clone();
    sort_extension_versions(&mut actual, TargetPlatform::Win32X64);
    assert_eq!(actual, expected);
}

#[test]
fn sorting_multiple_extension_versions_without_target_platforms() {
    let mut actual = vec![v("1.2.4"), v("1.1.3"), v("1.1.2"), v("1.1.1")];
    let expected = actual.clone();
    sort_extension_versions(&mut actual, TargetPlatform::Win32Arm64);
    assert_eq!(actual, expected);
}

#[test]
fn sorting_multiple_extension_versions_with_target_platforms_1() {
    let mut actual = vec![
        vp("1.2.4", TargetPlatform::DarwinArm64),
        vp("1.2.4", TargetPlatform::Win32Arm64),
        vp("1.2.4", TargetPlatform::LinuxArm64),
        v("1.1.3"),
        v("1.1.2"),
        v("1.1.1"),
    ];
    let expected = vec![actual[1].clone(), actual[0].clone(), actual[2].clone(), actual[3].clone(), actual[4].clone(), actual[5].clone()];
    sort_extension_versions(&mut actual, TargetPlatform::Win32Arm64);
    assert_eq!(actual, expected);
}

#[test]
fn sorting_multiple_extension_versions_with_target_platforms_2() {
    let mut actual = vec![
        v("1.2.4"),
        vp("1.2.3", TargetPlatform::DarwinArm64),
        vp("1.2.3", TargetPlatform::Win32Arm64),
        vp("1.2.3", TargetPlatform::LinuxArm64),
        v("1.1.2"),
        v("1.1.1"),
    ];
    let expected = vec![actual[0].clone(), actual[3].clone(), actual[1].clone(), actual[2].clone(), actual[4].clone(), actual[5].clone()];
    sort_extension_versions(&mut actual, TargetPlatform::LinuxArm64);
    assert_eq!(actual, expected);
}

#[test]
fn sorting_multiple_extension_versions_with_target_platforms_3() {
    let mut actual = vec![v("1.2.4"), v("1.1.2"), v("1.1.1"), vp("1.0.0", TargetPlatform::DarwinArm64), vp("1.0.0", TargetPlatform::Win32Arm64)];
    let expected = vec![actual[0].clone(), actual[1].clone(), actual[2].clone(), actual[4].clone(), actual[3].clone()];
    sort_extension_versions(&mut actual, TargetPlatform::Win32Arm64);
    assert_eq!(actual, expected);
}

use TargetPlatform::{DarwinX64, LinuxArm64, LinuxX64, Win32X64};

fn filter(versions: &[RawGalleryExtensionVersion], target: TargetPlatform, all: &[TargetPlatform]) -> Vec<RawGalleryExtensionVersion> {
    filter_latest_extension_versions_for_target_platform(versions, target, all)
}

#[test]
fn should_return_empty_array_for_empty_input() {
    assert_eq!(filter(&[], Win32X64, &[Win32X64]), vec![]);
}

#[test]
fn should_return_single_version_when_only_one_version_provided() {
    let versions = vec![vp("1.0.0", Win32X64)];
    assert_eq!(filter(&versions, Win32X64, &[Win32X64]), versions);
}

#[test]
fn should_include_latest_release_and_latest_pre_release_versions_for_same_platform() {
    let release = vp("1.0.0", Win32X64);
    let prerelease = prep("0.9.0", Win32X64);
    let result = filter(&[release.clone(), prerelease.clone()], Win32X64, &[Win32X64]);
    // Should include both since they have different version numbers
    assert_eq!(result, vec![release, prerelease]);
}

#[test]
fn should_include_latest_prerelease_and_latest_release_versions_for_same_platform() {
    let prerelease = prep("1.1.0", Win32X64);
    let release = vp("1.0.0", Win32X64);
    let result = filter(&[prerelease.clone(), release.clone()], Win32X64, &[Win32X64]);
    assert_eq!(result, vec![prerelease, release]);
}

#[test]
fn should_include_one_version_per_target_platform_for_release_versions() {
    let (version1, version2, version3) = (vp("1.0.0", Win32X64), vp("1.0.0", DarwinX64), vp("1.0.0", LinuxX64));
    let result = filter(&[version1.clone(), version2.clone(), version3.clone()], Win32X64, &[Win32X64, DarwinX64, LinuxX64]);
    // Should include all three versions: WIN32_X64 (compatible, first of type) + DARWIN_X64 & LINUX_X64 (non-compatible)
    assert_eq!(result.len(), 3);
    assert!(result.contains(&version1));
    assert!(result.contains(&version2));
    assert!(result.contains(&version3));
}

#[test]
fn should_handle_versions_without_target_platform_undefined() {
    let (version1, version2) = (v("1.0.0"), v("0.9.0"));
    let result = filter(&[version1.clone(), version2], Win32X64, &[Win32X64]);
    // Should only include the first version since they both have UNDEFINED platform
    assert_eq!(result, vec![version1]);
}

#[test]
fn should_handle_mixed_release_and_pre_release_versions_across_multiple_platforms() {
    let (release_win, release_mac) = (vp("1.0.0", Win32X64), vp("1.0.0", DarwinX64));
    let (pre_release_win, pre_release_mac) = (prep("1.1.0", Win32X64), prep("1.1.0", DarwinX64));
    let versions = [release_win.clone(), release_mac.clone(), pre_release_win.clone(), pre_release_mac.clone()];
    let result = filter(&versions, Win32X64, &[Win32X64, DarwinX64]);
    // Should include: WIN32_X64 compatible (release + prerelease) + DARWIN_X64 non-compatible (all versions)
    assert_eq!(result.len(), 4);
    for expected in [release_win, release_mac, pre_release_win, pre_release_mac] {
        assert!(result.contains(&expected));
    }
}

#[test]
fn should_handle_complex_scenario_with_multiple_versions_and_platforms() {
    let versions = vec![
        vp("2.0.0", Win32X64),
        vp("2.0.0", DarwinX64),
        prep("2.1.0", Win32X64),
        prep("2.1.0", LinuxX64),
        v("2.0.0"),  // No platform specified
        pre("2.1.0"), // Pre-release, no platform specified
    ];
    let result = filter(&versions, Win32X64, &[Win32X64, DarwinX64, LinuxX64]);
    // Should include: WIN32_X64 release + prerelease, DARWIN_X64 (non-compatible), LINUX_X64 (non-compatible)
    assert_eq!(result.len(), 4);
    for version in &versions[..4] {
        assert!(result.contains(version));
    }
}

#[test]
fn should_keep_only_first_compatible_version_when_specific_platform_comes_before_undefined() {
    let versions = vec![vp("1.0.0", Win32X64), v("1.0.0")];
    let result = filter(&versions, Win32X64, &[Win32X64, DarwinX64]);
    assert_eq!(result.len(), 1);
    assert!(result.contains(&versions[0]));
}

#[test]
fn should_handle_higher_version_with_specific_platform_vs_lower_version_with_universal_platform() {
    let (higher_specific, lower_universal) = (vp("2.0.0", Win32X64), v("1.5.0"));
    let result = filter(&[higher_specific.clone(), lower_universal.clone()], Win32X64, &[Win32X64, DarwinX64]);
    assert_eq!(result.len(), 1);
    assert!(result.contains(&higher_specific));
    assert!(!result.contains(&lower_universal));
}

#[test]
fn should_handle_higher_version_with_universal_platform_vs_lower_version_with_specific_platform() {
    let (higher_universal, lower_specific) = (v("2.0.0"), vp("1.0.0", Win32X64));
    let result = filter(&[higher_universal.clone(), lower_specific.clone()], Win32X64, &[Win32X64, DarwinX64]);
    assert_eq!(result.len(), 1);
    assert!(result.contains(&higher_universal));
    assert!(!result.contains(&lower_specific));
}

#[test]
fn should_handle_multiple_specific_platforms_vs_universal_platform_with_version_differences() {
    let versions = vec![vp("2.0.0", Win32X64), vp("1.9.0", DarwinX64), v("1.8.0")];
    let result = filter(&versions, Win32X64, &[Win32X64, DarwinX64, LinuxX64]);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&versions[0]));
    assert!(result.contains(&versions[1]));
}

#[test]
fn should_include_universal_platform_when_no_specific_platforms_conflict() {
    let (universal, specific) = (v("1.0.0"), vp("1.0.0", LinuxArm64));
    let result = filter(&[universal.clone(), specific.clone()], Win32X64, &[Win32X64, DarwinX64]);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&universal));
    assert!(result.contains(&specific));
}

#[test]
fn should_include_all_non_compatible_platform_versions() {
    let (version1, version2, version3) = (vp("1.0.0", Win32X64), vp("1.0.0", DarwinX64), prep("1.1.0", LinuxX64));
    let result = filter(&[version1, version2.clone(), version3.clone()], Win32X64, &[Win32X64, DarwinX64, LinuxX64]);
    assert!(result.contains(&version2));
    assert!(result.contains(&version3));
}

#[test]
fn should_prefer_specific_target_platform_over_undefined_when_same_version_exists_for_both() {
    let (undefined_version, specific) = (v("1.0.0"), vp("1.0.0", Win32X64));
    let result = filter(&[undefined_version.clone(), specific.clone()], Win32X64, &[Win32X64]);
    assert_eq!(result, vec![specific]);
    assert!(!result.contains(&undefined_version));
}

#[test]
fn should_replace_undefined_pre_release_with_specific_platform_pre_release() {
    let (undefined_pre, specific_pre) = (pre("1.0.0"), prep("1.0.0", Win32X64));
    let result = filter(&[undefined_pre.clone(), specific_pre.clone()], Win32X64, &[Win32X64]);
    assert_eq!(result, vec![specific_pre]);
    assert!(!result.contains(&undefined_pre));
}

#[test]
fn should_handle_explicit_universal_platform() {
    let (universal, specific) = (vp("1.0.0", TargetPlatform::Universal), vp("1.0.0", Win32X64));
    let result = filter(&[universal.clone(), specific.clone()], Win32X64, &[Win32X64]);
    assert_eq!(result, vec![specific]);
    assert!(!result.contains(&universal));
}

#[test]
fn should_handle_both_release_and_pre_release_with_same_version_replacement() {
    let (undefined_pre, specific_pre) = (pre("1.1.0"), prep("1.1.0", Win32X64));
    let (undefined_release, specific_release) = (v("1.0.0"), vp("1.0.0", Win32X64));
    let result = filter(&[undefined_pre.clone(), specific_pre.clone(), undefined_release.clone(), specific_release.clone()], Win32X64, &[Win32X64]);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&specific_release));
    assert!(result.contains(&specific_pre));
    assert!(!result.contains(&undefined_release));
    assert!(!result.contains(&undefined_pre));
}

#[test]
fn should_not_replace_when_specific_platform_is_for_different_platform() {
    let (undefined_version, specific_darwin) = (v("1.0.0"), vp("1.0.0", DarwinX64));
    let result = filter(&[undefined_version.clone(), specific_darwin.clone()], Win32X64, &[Win32X64, DarwinX64]);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&undefined_version));
    assert!(result.contains(&specific_darwin));
}

#[test]
fn should_handle_replacement_with_non_compatible_versions_in_between() {
    let (undefined_version, specific, non_compatible) = (v("1.0.0"), vp("1.0.0", Win32X64), vp("0.9.0", LinuxArm64));
    let result = filter(&[undefined_version.clone(), specific.clone(), non_compatible.clone()], Win32X64, &[Win32X64, DarwinX64]);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&specific));
    assert!(result.contains(&non_compatible));
    assert!(!result.contains(&undefined_version));
}

#[test]
fn should_filter_versions_for_linux_x64_target_platform_with_mixed_universal_and_platform_specific_versions() {
    let versions = vec![pre("0.15.0"), v("0.14.0"), vp("0.6.0", LinuxX64), prep("0.5.1", LinuxX64)];
    let result = filter(&versions, LinuxX64, &[LinuxX64]);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&versions[0]));
    assert!(result.contains(&versions[1]));
}

fn blocking(origin: Option<Value>) -> RawGalleryExtensionBlocking {
    RawGalleryExtensionBlocking { origin }
}

fn origin_of(info: Option<ExtensionBlockingInfo>) -> Option<ExtensionBlockingOrigin> {
    info.map(|i| i.origin)
}

const NOT_BLOCKED: &str = "public, validated";

#[test]
fn absent_flag_and_object_is_not_blocked() {
    assert_eq!(get_blocking_info(NOT_BLOCKED, None, None, None), None);
}

#[test]
fn extension_flag_without_object_is_blocked() {
    assert_eq!(get_blocking_info("public, validated, blocked", None, None, None), Some(ExtensionBlockingInfo { origin: ExtensionBlockingOrigin::Service }));
}

#[test]
fn extension_object_without_flag_is_blocked() {
    assert_eq!(get_blocking_info("public", Some(&blocking(Some(json!("policy")))), None, None), Some(ExtensionBlockingInfo { origin: ExtensionBlockingOrigin::Policy }));
}

#[test]
fn a_blocked_version_blocks_the_extension_in_hand_by_flag_or_by_object() {
    let by_flag = origin_of(get_blocking_info(NOT_BLOCKED, None, Some("validated, blocked"), None));
    let by_object = origin_of(get_blocking_info(NOT_BLOCKED, None, None, Some(&blocking(Some(json!("policy"))))));
    assert_eq!(vec![by_flag, by_object], vec![Some(ExtensionBlockingOrigin::Service), Some(ExtensionBlockingOrigin::Policy)]);
}

#[test]
fn an_extension_block_outranks_a_version_block() {
    assert_eq!(
        get_blocking_info("blocked", Some(&blocking(Some(json!("service")))), Some("blocked"), Some(&blocking(Some(json!("policy"))))),
        Some(ExtensionBlockingInfo { origin: ExtensionBlockingOrigin::Service })
    );
}

#[test]
fn unrecognized_absent_and_wrong_typed_origins_degrade_to_service_without_failing() {
    let origins = [Some(json!("tenant")), None, Some(json!(5)), Some(Value::Null), Some(json!({}))];
    let actual: Vec<Option<ExtensionBlockingOrigin>> = origins.into_iter().map(|origin| origin_of(get_blocking_info("blocked", Some(&blocking(origin)), None, None))).collect();
    assert_eq!(actual, vec![Some(ExtensionBlockingOrigin::Service); 5]);
}

#[test]
fn unknown_flag_tokens_do_not_fail_parsing() {
    assert_eq!(get_blocking_info("public, quarantined, blocked, embargoed", None, None, None), Some(ExtensionBlockingInfo { origin: ExtensionBlockingOrigin::Service }));
}

#[test]
fn blocked_is_matched_as_a_whole_token() {
    assert_eq!(get_blocking_info("public, unblocked", None, Some("unblocked"), None), None);
}
