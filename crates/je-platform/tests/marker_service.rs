//! Porte de `src/vs/platform/markers/test/common/markerService.test.ts` (suíte `Marker Service`), caso a caso e na mesma ordem.
//!
//! Divergências: o `message: undefined/null` do teste `invalid marker data` é a mensagem vazia (o tipo de
//! `MarkerData.message` é `String`); o `randomMarkerData` usa um contador no lugar de `Math.random()`.

mod common;

use std::cell::Cell;

use common::*;
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_platform::markers::marker_service::MarkerService;
use je_platform::markers::{IMarkerService, MarkerCode, MarkerData, MarkerReadOptions, MarkerSeverity, ResourceMarker};

thread_local! {
    static COUNTER: Cell<u64> = const { Cell::new(0) };
}

fn random_marker_data(severity: MarkerSeverity) -> MarkerData {
    let n = COUNTER.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    MarkerData::new(severity, format!("{n:x}"), 1, 1, 1, 1)
}

fn error() -> MarkerData {
    random_marker_data(MarkerSeverity::ERROR)
}

fn uri(text: &str) -> URI {
    URI::parse(text, false)
}

fn owner(name: &str) -> MarkerReadOptions {
    MarkerReadOptions { owner: Some(name.into()), ..Default::default() }
}

fn resource(text: &str) -> MarkerReadOptions {
    MarkerReadOptions { resource: Some(uri(text)), ..Default::default() }
}

fn severities(value: u32) -> MarkerReadOptions {
    MarkerReadOptions { severities: Some(value), ..Default::default() }
}

fn rm(resource: &str, marker: MarkerData) -> ResourceMarker {
    ResourceMarker { resource: uri(resource), marker }
}

#[test]
fn query() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();

    service.change_all("far", vec![rm("file:///c/test/file.cs", random_marker_data(MarkerSeverity::ERROR))]);

    assert_eq!(service.read(None).len(), 1);
    assert_eq!(service.read(Some(&owner("far"))).len(), 1);
    assert_eq!(service.read(Some(&resource("file:///c/test/file.cs"))).len(), 1);
    assert_eq!(service.read(Some(&MarkerReadOptions { owner: Some("far".into()), resource: Some(uri("file:///c/test/file.cs")), ..Default::default() })).len(), 1);

    service.change_all("boo", vec![rm("file:///c/test/file.cs", random_marker_data(MarkerSeverity::WARNING))]);

    assert_eq!(service.read(None).len(), 2);
    assert_eq!(service.read(Some(&owner("far"))).len(), 1);
    assert_eq!(service.read(Some(&owner("boo"))).len(), 1);

    assert_eq!(service.read(Some(&severities(MarkerSeverity::ERROR.0))).len(), 1);
    assert_eq!(service.read(Some(&severities(MarkerSeverity::WARNING.0))).len(), 1);
    assert_eq!(service.read(Some(&severities(MarkerSeverity::HINT.0))).len(), 0);
    assert_eq!(service.read(Some(&severities(MarkerSeverity::ERROR.0 | MarkerSeverity::WARNING.0))).len(), 2);
    service.dispose();
}

#[test]
fn change_one_override() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    service.change_one("far", &uri("file:///path/only.cs"), vec![error()], None);
    assert_eq!(service.read(None).len(), 1);
    assert_eq!(service.read(Some(&owner("far"))).len(), 1);

    service.change_one("boo", &uri("file:///path/only.cs"), vec![error()], None);
    assert_eq!(service.read(None).len(), 2);
    assert_eq!(service.read(Some(&owner("far"))).len(), 1);
    assert_eq!(service.read(Some(&owner("boo"))).len(), 1);

    service.change_one("far", &uri("file:///path/only.cs"), vec![error(), error()], None);
    assert_eq!(service.read(Some(&owner("far"))).len(), 2);
    assert_eq!(service.read(Some(&owner("boo"))).len(), 1);
    service.dispose();
}

#[test]
fn change_one_all_clears() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    service.change_one("far", &uri("file:///path/only.cs"), vec![error()], None);
    service.change_one("boo", &uri("file:///path/only.cs"), vec![error()], None);
    assert_eq!(service.read(Some(&owner("far"))).len(), 1);
    assert_eq!(service.read(Some(&owner("boo"))).len(), 1);
    assert_eq!(service.read(None).len(), 2);

    service.change_one("far", &uri("file:///path/only.cs"), vec![], None);
    assert_eq!(service.read(Some(&owner("far"))).len(), 0);
    assert_eq!(service.read(Some(&owner("boo"))).len(), 1);
    assert_eq!(service.read(None).len(), 1);

    service.change_all("boo", vec![]);
    assert_eq!(service.read(Some(&owner("far"))).len(), 0);
    assert_eq!(service.read(Some(&owner("boo"))).len(), 0);
    assert_eq!(service.read(None).len(), 0);
    service.dispose();
}

#[test]
fn change_all_sends_event_for_cleared() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    service.change_all("far", vec![rm("file:///d/path", error()), rm("file:///d/path", error())]);
    assert_eq!(service.read(Some(&owner("far"))).len(), 2);

    let observer = service.clone();
    let d = service.on_marker_changed().listen(move |changed_resources| {
        assert_eq!(changed_resources.len(), 1);
        for u in changed_resources {
            assert_eq!(u.to_string(false).to_string_lossy(), "file:///d/path");
        }
        assert_eq!(observer.read(Some(&owner("far"))).len(), 0);
    });

    service.change_all("far", vec![]);
    d.dispose();
    service.dispose();
}

#[test]
fn change_all_merges() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    service.change_all("far", vec![rm("file:///c/test/file.cs", error()), rm("file:///c/test/file.cs", error())]);
    assert_eq!(service.read(Some(&owner("far"))).len(), 2);
    service.dispose();
}

#[test]
fn change_all_must_not_break_integrety_issue_12635() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    service.change_all("far", vec![rm("scheme:path1", error()), rm("scheme:path2", error())]);
    service.change_all("boo", vec![rm("scheme:path1", error())]);
    service.change_all("far", vec![rm("scheme:path1", error()), rm("scheme:path2", error())]);

    assert_eq!(service.read(Some(&owner("far"))).len(), 2);
    assert_eq!(service.read(Some(&resource("scheme:path1"))).len(), 2);
    service.dispose();
}

#[test]
fn invalid_marker_data() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut data = error();
    let service = MarkerService::new();

    // `message = undefined`
    data.message = String::new();
    service.change_one("far", &uri("some:uri/path"), vec![data.clone()], None);
    assert_eq!(service.read(Some(&owner("far"))).len(), 0);

    // `message = null`
    data.message = String::new();
    service.change_one("far", &uri("some:uri/path"), vec![data.clone()], None);
    assert_eq!(service.read(Some(&owner("far"))).len(), 0);

    data.message = "null".to_string();
    service.change_one("far", &uri("some:uri/path"), vec![data], None);
    assert_eq!(service.read(Some(&owner("far"))).len(), 1);
    service.dispose();
}

#[test]
fn map_map_remove_returns_bad_values_issue_13548() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    service.change_one("o", &uri("some:uri/1"), vec![error()], None);
    service.change_one("o", &uri("some:uri/2"), vec![], None);
    service.dispose();
}

#[test]
fn error_code_of_zero_in_markers_get_removed_31275() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut data = MarkerData::new(MarkerSeverity(0), "test", 1, 2, 1, 5);
    data.code = Some(MarkerCode::Text("0".to_string()));
    data.source = Some("me".to_string());

    let service = MarkerService::new();
    service.change_one("far", &uri("some:thing"), vec![data], None);
    let marker = service.read(Some(&resource("some:thing")));

    assert_eq!(marker.len(), 1);
    assert_eq!(marker[0].code, Some(MarkerCode::Text("0".to_string())));
    service.dispose();
}

#[test]
fn model_version_id_is_preserved_on_imarker_when_present_in_imarker_data() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    let res = uri("file:///path/file.ts");

    // Test with modelVersionId present
    let mut data_with_version = error();
    data_with_version.model_version_id = Some(42);
    service.change_one("owner", &res, vec![data_with_version], None);

    let read_resource = MarkerReadOptions { resource: Some(res.clone()), ..Default::default() };
    let markers_with_version = service.read(Some(&read_resource));
    assert_eq!(markers_with_version.len(), 1);
    assert_eq!(markers_with_version[0].model_version_id, Some(42));

    // Test without modelVersionId (should be undefined)
    service.change_one("owner", &res, vec![error()], None);

    let markers_without_version = service.read(Some(&read_resource));
    assert_eq!(markers_without_version.len(), 1);
    assert_eq!(markers_without_version[0].model_version_id, None);
    service.dispose();
}

#[test]
fn tracks_diagnostic_truncation_per_resource_across_owners() {
    use je_platform::markers::MarkerChangeOptions;
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    let res = uri("file:///path/file.ts");
    let other_resource = uri("file:///path/other.ts");

    service.change_one("first", &res, vec![error()], Some(MarkerChangeOptions { is_truncated: true }));
    service.change_one("second", &res, vec![error()], None);
    service.change_one("first", &res, vec![error()], None);
    service.change_one("first", &other_resource, vec![error()], Some(MarkerChangeOptions { is_truncated: true }));

    let after_change_all = [service.is_source_truncated(&res), service.is_source_truncated(&other_resource)];
    service.change_all("first", vec![]);

    assert_eq!(
        (after_change_all, [service.is_source_truncated(&res), service.is_source_truncated(&other_resource)]),
        ([false, true], [false, false])
    );
    service.dispose();
}

#[test]
fn resource_filter_hides_markers_for_the_filtered_resource() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    let resource1 = uri("file:///path/file1.cs");
    let resource2 = uri("file:///path/file2.cs");

    // Add markers to both resources
    service.change_one("owner1", &resource1, vec![error()], None);
    service.change_one("owner1", &resource2, vec![error()], None);

    let of = |r: &URI| MarkerReadOptions { resource: Some(r.clone()), ..Default::default() };

    // Verify both resources have markers
    assert_eq!(service.read(None).len(), 2);
    assert_eq!(service.read(Some(&of(&resource1))).len(), 1);
    assert_eq!(service.read(Some(&of(&resource2))).len(), 1);

    // Install filter for resource1
    let filter = service.install_resource_filter(&resource1, "Test filter");

    // Verify resource1 markers are filtered out, but have 1 info marker instead
    assert_eq!(service.read(None).len(), 2); // 1 real + 1 info
    assert_eq!(service.read(Some(&of(&resource1))).len(), 1); // 1 info
    assert_eq!(service.read(Some(&of(&resource2))).len(), 1);

    // Dispose filter
    filter.dispose();

    // Verify resource1 markers are visible again
    assert_eq!(service.read(None).len(), 2);
    assert_eq!(service.read(Some(&of(&resource1))).len(), 1);
    assert_eq!(service.read(Some(&of(&resource2))).len(), 1);
    service.dispose();
}

#[test]
fn resource_filter_hides_markers_for_the_filtered_resource_unless_explicit_read() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    let resource1 = uri("file:///path/file1.cs");
    let resource2 = uri("file:///path/file2.cs");

    service.change_one("owner1", &resource1, vec![error()], None);
    service.change_one("owner1", &resource2, vec![error()], None);

    let of = |r: &URI, ignore: bool| MarkerReadOptions { resource: Some(r.clone()), ignore_resource_filters: ignore, ..Default::default() };

    assert_eq!(service.read(None).len(), 2);
    assert_eq!(service.read(Some(&of(&resource1, false))).len(), 1);
    assert_eq!(service.read(Some(&of(&resource2, false))).len(), 1);

    // Install filter for resource1
    let filter = service.install_resource_filter(&resource1, "Test filter");

    // Verify resource1 markers are filtered out, but have 1 info marker instead
    assert_eq!(service.read(None).len(), 2); // 1 real + 1 info
    assert_eq!(service.read(Some(&of(&resource1, false))).len(), 1); // 1 info
    assert_eq!(service.read(Some(&of(&resource2, false))).len(), 1);

    // Verify resource1 markers are visible again
    assert_eq!(service.read(Some(&MarkerReadOptions { ignore_resource_filters: true, ..Default::default() })).len(), 2);
    assert_eq!(service.read(Some(&of(&resource1, true))).len(), 1);
    assert_eq!(service.read(Some(&of(&resource1, true)))[0].severity, MarkerSeverity::ERROR);
    assert_eq!(service.read(Some(&of(&resource2, true))).len(), 1);
    assert_eq!(service.read(Some(&of(&resource2, true)))[0].severity, MarkerSeverity::ERROR);

    // Dispose filter
    filter.dispose();
    service.dispose();
}

#[test]
fn resource_filter_affects_all_filter_combinations() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    let res = uri("file:///path/file.cs");

    service.change_one("owner1", &res, vec![random_marker_data(MarkerSeverity::ERROR)], None);
    service.change_one("owner2", &res, vec![random_marker_data(MarkerSeverity::WARNING)], None);

    let of_resource = MarkerReadOptions { resource: Some(res.clone()), ..Default::default() };
    let of_owner_resource = |name: &str| MarkerReadOptions { owner: Some(name.into()), resource: Some(res.clone()), ..Default::default() };

    // Verify initial state
    assert_eq!(service.read(None).len(), 2);
    assert_eq!(service.read(Some(&of_resource)).len(), 2);
    assert_eq!(service.read(Some(&owner("owner1"))).len(), 1);
    assert_eq!(service.read(Some(&owner("owner2"))).len(), 1);
    assert_eq!(service.read(Some(&of_owner_resource("owner1"))).len(), 1);
    assert_eq!(service.read(Some(&severities(MarkerSeverity::ERROR.0))).len(), 1);
    assert_eq!(service.read(Some(&severities(MarkerSeverity::WARNING.0))).len(), 1);

    // Install filter
    let filter = service.install_resource_filter(&res, "Filter reason");

    // Verify information marker is shown for resource queries
    assert_eq!(service.read(None).len(), 1); // 1 info marker
    assert_eq!(service.read(Some(&of_resource)).len(), 1); // 1 info marker
    assert_eq!(service.read(Some(&owner("owner1"))).len(), 1); // 1 info marker
    assert_eq!(service.read(Some(&owner("owner2"))).len(), 1); // 1 info marker

    // Verify owner+resource query returns an info marker for filtered resources
    let owner_resource_markers = service.read(Some(&of_owner_resource("owner1")));
    assert_eq!(owner_resource_markers.len(), 1);
    assert_eq!(owner_resource_markers[0].severity, MarkerSeverity::INFO);
    assert_eq!(owner_resource_markers[0].owner, "markersFilter");

    assert_eq!(service.read(Some(&severities(MarkerSeverity::ERROR.0))).len(), 1); // 1 info marker
    assert_eq!(service.read(Some(&severities(MarkerSeverity::WARNING.0))).len(), 1); // 1 info marker
    assert_eq!(service.read(Some(&severities(MarkerSeverity::INFO.0))).len(), 1); // Our info marker

    // Remove filter and verify markers are visible again
    filter.dispose();
    assert_eq!(service.read(None).len(), 2);
    service.dispose();
}

#[test]
fn multiple_filters_for_same_resource_are_handled_correctly() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    let res = uri("file:///path/file.cs");
    let of_resource = MarkerReadOptions { resource: Some(res.clone()), ..Default::default() };

    // Add marker to resource
    service.change_one("owner1", &res, vec![error()], None);

    // Verify resource has markers
    assert_eq!(service.read(None).len(), 1);
    assert_eq!(service.read(Some(&of_resource)).len(), 1);

    // Install two filters for the same resource
    let filter1 = service.install_resource_filter(&res, "First filter");
    let filter2 = service.install_resource_filter(&res, "Second filter");

    // Verify resource markers are filtered out but info marker is shown
    assert_eq!(service.read(None).len(), 1); // 1 info marker
    assert_eq!(service.read(Some(&of_resource)).len(), 1); // 1 info marker

    // Dispose only one filter
    filter1.dispose();

    // Verify resource markers are still filtered out because one filter remains
    assert_eq!(service.read(None).len(), 1); // still 1 info marker
    assert_eq!(service.read(Some(&of_resource)).len(), 1); // still 1 info marker

    // Dispose the second filter
    filter2.dispose();

    // Now all filters are gone, so markers should be visible again
    assert_eq!(service.read(None).len(), 1);
    assert_eq!(service.read(Some(&of_resource)).len(), 1);
    service.dispose();
}

#[test]
fn resource_filter_with_reason_shows_info_marker_when_markers_are_filtered() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    let res = uri("file:///path/file.cs");
    let of_resource = MarkerReadOptions { resource: Some(res.clone()), ..Default::default() };

    // Add error and warning to the resource
    service.change_one("owner1", &res, vec![random_marker_data(MarkerSeverity::ERROR), random_marker_data(MarkerSeverity::WARNING)], None);

    // Verify initial state
    assert_eq!(service.read(None).len(), 2);
    assert_eq!(service.read(Some(&of_resource)).len(), 2);

    // Apply a filter with reason
    let filter_reason = "Test filter reason";
    let filter = service.install_resource_filter(&res, filter_reason);

    // Verify that we get a single info marker with our reason
    let markers = service.read(Some(&of_resource));
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].severity, MarkerSeverity::INFO);
    assert!(markers[0].message.contains(filter_reason));

    // Remove filter and verify the original markers are back
    filter.dispose();
    assert_eq!(service.read(Some(&of_resource)).len(), 2);
    service.dispose();
}

#[test]
fn reading_all_markers_shows_info_marker_for_filtered_resources() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    let resource1 = uri("file:///path/file1.cs");
    let resource2 = uri("file:///path/file2.cs");

    // Add markers to both resources
    service.change_one("owner1", &resource1, vec![error()], None);
    service.change_one("owner1", &resource2, vec![error()], None);

    // Verify initial state
    assert_eq!(service.read(None).len(), 2);

    // Filter one resource with a reason
    let filter_reason = "Resource is being edited";
    let filter = service.install_resource_filter(&resource1, filter_reason);

    // Read all markers
    let all_markers = service.read(None);

    // Should have 2 markers - one real marker and one info marker
    assert_eq!(all_markers.len(), 2);

    // Find the info marker
    let info_marker = all_markers.iter().find(|marker| marker.owner == "markersFilter" && marker.severity == MarkerSeverity::INFO);

    // Verify the info marker
    let info_marker = info_marker.expect("esperava o marcador de informação do filtro");
    assert_eq!(info_marker.resource.to_string(false), resource1.to_string(false));
    assert!(info_marker.message.contains(filter_reason));

    // Remove filter
    filter.dispose();
    service.dispose();
}

#[test]
fn out_of_order_filter_disposal_works_correctly() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = MarkerService::new();
    let res = uri("file:///path/file.cs");
    let of_resource = MarkerReadOptions { resource: Some(res.clone()), ..Default::default() };

    // Add marker to resource
    service.change_one("owner1", &res, vec![error()], None);

    // Verify resource has markers
    assert_eq!(service.read(None).len(), 1);
    assert_eq!(service.read(Some(&of_resource)).len(), 1);

    // Install three filters for the same resource
    let filter1 = service.install_resource_filter(&res, "First filter");
    let filter2 = service.install_resource_filter(&res, "Second filter");
    let filter3 = service.install_resource_filter(&res, "Third filter");

    // Verify resource markers are filtered out but info marker is shown
    assert_eq!(service.read(None).len(), 1); // 1 info marker
    assert_eq!(service.read(Some(&of_resource)).len(), 1); // 1 info marker

    // Dispose filters in a different order than they were created
    filter2.dispose(); // Remove the second filter first

    // Verify resource markers are still filtered out with 2 filters remaining
    assert_eq!(service.read(None).len(), 1); // still 1 info marker
    assert_eq!(service.read(Some(&of_resource)).len(), 1); // still 1 info marker

    // Check if message contains the correct count of filters
    let markers = service.read(Some(&of_resource));
    assert!(markers[0].message.contains("Problems are paused because"));

    // Remove remaining filters in any order
    filter3.dispose();
    filter1.dispose();

    // Now all filters are gone, so markers should be visible again
    assert_eq!(service.read(None).len(), 1);
    assert_eq!(service.read(Some(&of_resource)).len(), 1);
    service.dispose();
}
