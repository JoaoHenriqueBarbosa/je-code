//! Porte de `src/vs/workbench/contrib/markers/test/browser/markersModel.test.ts` (suíte `MarkersModel Test`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo de cada caso.
//! - **`TestMarkersModel extends MarkersModel`.** Não há herança: [`test_markers_model`] cria o [`MarkersModel`] e agrupa os
//!   marcadores por recurso (`groupBy` na ordem da primeira aparição), chamando `setResourceMarkers` por grupo.
//! - **`IMarker`.** É o `Marker` de `je_platform::markers::markers` (importado como `IMarker`), compartilhado em `Rc`, e o
//!   `assert.strictEqual(actuals[0].marker, marker6)` compara a identidade com `Rc::ptr_eq`. O spread `{ ...aMarker(),
//!   resource: frag1 }` é a cópia do marcador com o recurso trocado.
//! - **`deepStrictEqual` do caso `reset`.** Compara a contagem dos recursos em cache, a lista de recursos e o total no momento
//!   do evento, e a lista `removed` com os mesmos objetos que estavam em cache (identidade e ordem).
//! - **Caso `toString()`.** O texto esperado do `JSON.stringify({ ...marker, resource: marker.resource.path }, null, '\t')` é
//!   escrito por extenso, na ordem das chaves do objeto literal de `aMarker` (a chave `code` atribuída depois vem no fim; a
//!   `relatedInformation` ausente é omitida). O `(testObject as any).relatedInformation = ...` que o original usa para trocar o
//!   `null!` passado ao construtor é a lista de informações relacionadas dada ao construtor.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::collections::group_by;
use je_base::uri::URI;
use je_platform::markers::markers::{Marker as IMarker, MarkerCode, MarkerSeverity, RelatedInformation as IRelatedInformation};
use je_platform::resources::uri_file;
use je_workbench::contrib::markers::markers_model::{Marker, MarkersModel, RelatedInformation, ResourceMarkers};

/// `new TestMarkersModel(markers)`.
fn test_markers_model(guard: &LeakGuard, markers: Vec<Rc<IMarker>>) -> Rc<MarkersModel> {
    let model = guard.add_rc(Rc::new(MarkersModel::new()));
    for (_, group) in group_by(&markers, |marker| marker.resource.to_string(false).to_string_lossy()) {
        let resource = group[0].resource.clone();
        model.set_resource_markers(vec![(resource, group)]);
    }
    model
}

fn compare_resource(a: &Rc<ResourceMarkers>, b: &str) -> bool {
    a.resource.to_string(false) == uri_file(b).to_string(false)
}

/// `aMarker(resource, severity, startLineNumber, startColumn, endLineNumber, endColumn, message, source, relatedInformation)`.
#[allow(clippy::too_many_arguments)]
fn a_marker_full(
    resource: &str,
    severity: MarkerSeverity,
    start_line_number: i64,
    start_column: i64,
    end_line_number: i64,
    end_column: i64,
    message: &str,
    source: &str,
    related_information: Option<Vec<IRelatedInformation>>,
) -> IMarker {
    IMarker {
        owner: "someOwner".to_string(),
        resource: uri_file(resource),
        severity,
        code: None,
        message: message.to_string(),
        source: Some(source.to_string()),
        start_line_number,
        start_column,
        end_line_number,
        end_column,
        model_version_id: None,
        related_information,
        tags: None,
        origin: None,
    }
}

/// `aMarker(resource, severity)`: as posições e a mensagem padrão.
fn a_marker(resource: &str, severity: MarkerSeverity) -> IMarker {
    a_marker_full(resource, severity, 10, 5, 11, 10, "some message", "tslint", None)
}

/// `aMarker()`.
fn a_default_marker() -> IMarker {
    a_marker("some resource", MarkerSeverity::ERROR)
}

/// `anErrorWithRange(startLineNumber)` e as variantes com o mesmo nome por severidade: as demais posições são as padrão.
fn with_range(severity: MarkerSeverity, start_line_number: i64) -> Rc<IMarker> {
    with_full_range(severity, start_line_number, 5, start_line_number + 1, 10, "some message")
}

/// `xWithRange(startLineNumber, startColumn, endLineNumber, endColumn, message)`.
fn with_full_range(severity: MarkerSeverity, start_line_number: i64, start_column: i64, end_line_number: i64, end_column: i64, message: &str) -> Rc<IMarker> {
    Rc::new(a_marker_full("some resource", severity, start_line_number, start_column, end_line_number, end_column, message, "tslint", None))
}

/// `{ ...marker, resource }`.
fn with_resource(marker: IMarker, resource: &URI) -> Rc<IMarker> {
    Rc::new(IMarker { resource: resource.clone(), ..marker })
}

/// O texto que o `JSON.stringify(obj, null, '\t')` produz para um objeto de uma chave por linha.
fn json_object(lines: &[&str]) -> String {
    format!("{{\n\t{}\n}}", lines.join(",\n\t"))
}

#[test]
fn marker_ids_are_unique() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let marker1 = with_range(MarkerSeverity::ERROR, 3);
    let marker2 = with_full_range(MarkerSeverity::ERROR, 3, 5, 4, 10, "different message");
    let marker3 = with_range(MarkerSeverity::WARNING, 3);
    let marker4 = with_range(MarkerSeverity::WARNING, 5);

    let test_object = test_markers_model(&guard, vec![marker1, marker2, marker3, marker4]);
    let actuals = test_object.resource_markers()[0].markers();

    assert_eq!(actuals.len(), 4);
    assert_ne!(actuals[0].id, actuals[1].id);
    assert_ne!(actuals[0].id, actuals[2].id);
    assert_ne!(actuals[0].id, actuals[3].id);
    assert_ne!(actuals[1].id, actuals[2].id);
    assert_ne!(actuals[1].id, actuals[3].id);
    assert_ne!(actuals[2].id, actuals[3].id);
}

#[test]
fn reset_clears_cached_resources_before_notifying_listeners() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = test_markers_model(
        &guard,
        vec![Rc::new(a_marker("first", MarkerSeverity::ERROR)), Rc::new(a_marker("second", MarkerSeverity::ERROR))],
    );
    let cached_resources = test_object.resource_markers();

    struct Change {
        resources: Vec<Rc<ResourceMarkers>>,
        removed: Vec<Rc<ResourceMarkers>>,
        total: usize,
    }
    let changes: Rc<RefCell<Vec<Change>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    let model = test_object.clone();
    guard.add(test_object.on_did_change().listen(move |event| {
        sink.borrow_mut().push(Change { resources: model.resource_markers(), removed: event.removed.clone(), total: model.total() });
    }));

    test_object.reset();

    assert_eq!(cached_resources.len(), 2);
    assert!(test_object.resource_markers().is_empty());
    assert_eq!(test_object.total(), 0);
    let changes = changes.borrow();
    assert_eq!(changes.len(), 1);
    assert!(changes[0].resources.is_empty());
    assert_eq!(changes[0].removed.len(), cached_resources.len());
    assert!(changes[0].removed.iter().zip(&cached_resources).all(|(removed, cached)| Rc::ptr_eq(removed, cached)));
    assert_eq!(changes[0].total, 0);
}

#[test]
fn duplicate_markers_from_different_owners_are_deduplicated() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Simulate a task problem matcher and language extension both reporting
    // the same diagnostic for the same file (#244424).
    let mut task_marker = a_marker_full("some resource", MarkerSeverity::ERROR, 10, 5, 11, 10, "some message", "eslint", None);
    task_marker.owner = "taskOwner".to_string();
    let mut extension_marker = a_marker_full("some resource", MarkerSeverity::ERROR, 10, 5, 11, 10, "some message", "eslint", None);
    extension_marker.owner = "extensionOwner".to_string();

    let test_object = test_markers_model(&guard, vec![Rc::new(task_marker), Rc::new(extension_marker)]);
    let actuals = test_object.resource_markers()[0].markers();

    assert_eq!(actuals.len(), 1, "identical markers from different owners should be deduplicated");
    assert_eq!(actuals[0].marker.message, "some message");
}

#[test]
fn markers_with_different_messages_are_not_deduplicated() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let marker1 = Rc::new(a_marker_full("some resource", MarkerSeverity::ERROR, 10, 5, 11, 10, "message without period", "eslint", None));
    let marker2 = Rc::new(a_marker_full("some resource", MarkerSeverity::ERROR, 10, 5, 11, 10, "message with period.", "eslint", None));

    let test_object = test_markers_model(&guard, vec![marker1, marker2]);
    let actuals = test_object.resource_markers()[0].markers();

    assert_eq!(actuals.len(), 2, "markers with different messages should not be deduplicated");
}

#[test]
fn sort_palces_resources_with_no_errors_at_the_end() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let marker1 = Rc::new(a_marker("a/res1", MarkerSeverity::WARNING));
    let marker2 = Rc::new(a_marker("a/res2", MarkerSeverity::ERROR));
    let marker3 = Rc::new(a_marker("res4", MarkerSeverity::ERROR));
    let marker4 = Rc::new(a_marker("b/res3", MarkerSeverity::ERROR));
    let marker5 = Rc::new(a_marker("res4", MarkerSeverity::ERROR));
    let marker6 = Rc::new(a_marker("c/res2", MarkerSeverity::INFO));
    let test_object = test_markers_model(&guard, vec![marker1, marker2, marker3, marker4, marker5, marker6]);

    let actuals = test_object.resource_markers();

    assert_eq!(5, actuals.len());
    assert!(compare_resource(&actuals[0], "a/res2"));
    assert!(compare_resource(&actuals[1], "b/res3"));
    assert!(compare_resource(&actuals[2], "res4"));
    assert!(compare_resource(&actuals[3], "a/res1"));
    assert!(compare_resource(&actuals[4], "c/res2"));
}

#[test]
fn sort_resources_by_file_path() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let marker1 = Rc::new(a_marker("a/res1", MarkerSeverity::ERROR));
    let marker2 = Rc::new(a_marker("a/res2", MarkerSeverity::ERROR));
    let marker3 = Rc::new(a_marker("res4", MarkerSeverity::ERROR));
    let marker4 = Rc::new(a_marker("b/res3", MarkerSeverity::ERROR));
    let marker5 = Rc::new(a_marker("res4", MarkerSeverity::ERROR));
    let marker6 = Rc::new(a_marker("c/res2", MarkerSeverity::ERROR));
    let test_object = test_markers_model(&guard, vec![marker1, marker2, marker3, marker4, marker5, marker6]);

    let actuals = test_object.resource_markers();

    assert_eq!(5, actuals.len());
    assert!(compare_resource(&actuals[0], "a/res1"));
    assert!(compare_resource(&actuals[1], "a/res2"));
    assert!(compare_resource(&actuals[2], "b/res3"));
    assert!(compare_resource(&actuals[3], "c/res2"));
    assert!(compare_resource(&actuals[4], "res4"));
}

#[test]
fn sort_markers_by_severity_line_and_column() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let marker1 = with_full_range(MarkerSeverity::WARNING, 8, 1, 9, 3, "some message");
    let marker2 = with_range(MarkerSeverity::WARNING, 3);
    let marker3 = with_full_range(MarkerSeverity::ERROR, 8, 1, 9, 3, "some message");
    let marker4 = with_range(MarkerSeverity::HINT, 5);
    let marker5 = with_full_range(MarkerSeverity::INFO, 8, 1, 8, 4, "ab");
    let marker6 = with_range(MarkerSeverity::ERROR, 3);
    let marker7 = with_range(MarkerSeverity::ERROR, 5);
    let marker8 = with_range(MarkerSeverity::INFO, 5);
    let marker9 = with_full_range(MarkerSeverity::ERROR, 8, 1, 8, 4, "ab");
    let marker10 = with_range(MarkerSeverity::ERROR, 10);
    let marker11 = with_full_range(MarkerSeverity::ERROR, 8, 1, 8, 4, "ba");
    let marker12 = with_range(MarkerSeverity::HINT, 3);
    let marker13 = with_range(MarkerSeverity::WARNING, 5);
    let marker14 = with_range(MarkerSeverity::ERROR, 4);
    let marker15 = with_full_range(MarkerSeverity::ERROR, 8, 2, 8, 4, "some message");
    let test_object = test_markers_model(
        &guard,
        vec![
            marker1.clone(),
            marker2.clone(),
            marker3.clone(),
            marker4.clone(),
            marker5.clone(),
            marker6.clone(),
            marker7.clone(),
            marker8.clone(),
            marker9.clone(),
            marker10.clone(),
            marker11.clone(),
            marker12.clone(),
            marker13.clone(),
            marker14.clone(),
            marker15.clone(),
        ],
    );

    let actuals = test_object.resource_markers()[0].markers();

    assert!(Rc::ptr_eq(&actuals[0].marker, &marker6));
    assert!(Rc::ptr_eq(&actuals[1].marker, &marker14));
    assert!(Rc::ptr_eq(&actuals[2].marker, &marker7));
    assert!(Rc::ptr_eq(&actuals[3].marker, &marker9));
    assert!(Rc::ptr_eq(&actuals[4].marker, &marker11));
    assert!(Rc::ptr_eq(&actuals[5].marker, &marker3));
    assert!(Rc::ptr_eq(&actuals[6].marker, &marker15));
    assert!(Rc::ptr_eq(&actuals[7].marker, &marker10));
    assert!(Rc::ptr_eq(&actuals[8].marker, &marker2));
    assert!(Rc::ptr_eq(&actuals[9].marker, &marker13));
    assert!(Rc::ptr_eq(&actuals[10].marker, &marker1));
    assert!(Rc::ptr_eq(&actuals[11].marker, &marker8));
    assert!(Rc::ptr_eq(&actuals[12].marker, &marker5));
    assert!(Rc::ptr_eq(&actuals[13].marker, &marker12));
    assert!(Rc::ptr_eq(&actuals[14].marker, &marker4));
}

#[test]
fn to_string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut marker = a_marker("a/res1", MarkerSeverity::ERROR);
    marker.code = Some(MarkerCode::Text("1234".to_string()));
    assert_eq!(
        json_object(&[
            "\"owner\": \"someOwner\"",
            "\"resource\": \"/a/res1\"",
            "\"severity\": 8",
            "\"message\": \"some message\"",
            "\"startLineNumber\": 10",
            "\"startColumn\": 5",
            "\"endLineNumber\": 11",
            "\"endColumn\": 10",
            "\"source\": \"tslint\"",
            "\"code\": \"1234\"",
        ]),
        Marker::new("1", Rc::new(marker), Vec::new()).to_string()
    );

    let marker = a_marker("a/res2", MarkerSeverity::WARNING);
    assert_eq!(
        json_object(&[
            "\"owner\": \"someOwner\"",
            "\"resource\": \"/a/res2\"",
            "\"severity\": 4",
            "\"message\": \"some message\"",
            "\"startLineNumber\": 10",
            "\"startColumn\": 5",
            "\"endLineNumber\": 11",
            "\"endColumn\": 10",
            "\"source\": \"tslint\"",
        ]),
        Marker::new("2", Rc::new(marker), Vec::new()).to_string()
    );

    let marker = a_marker_full("a/res2", MarkerSeverity::INFO, 1, 2, 1, 8, "Info", "", None);
    assert_eq!(
        json_object(&[
            "\"owner\": \"someOwner\"",
            "\"resource\": \"/a/res2\"",
            "\"severity\": 2",
            "\"message\": \"Info\"",
            "\"startLineNumber\": 1",
            "\"startColumn\": 2",
            "\"endLineNumber\": 1",
            "\"endColumn\": 8",
            "\"source\": \"\"",
        ]),
        Marker::new("3", Rc::new(marker), Vec::new()).to_string()
    );

    let marker = a_marker_full("a/res2", MarkerSeverity::HINT, 1, 2, 1, 8, "Ignore message", "Ignore", None);
    assert_eq!(
        json_object(&[
            "\"owner\": \"someOwner\"",
            "\"resource\": \"/a/res2\"",
            "\"severity\": 1",
            "\"message\": \"Ignore message\"",
            "\"startLineNumber\": 1",
            "\"startColumn\": 2",
            "\"endLineNumber\": 1",
            "\"endColumn\": 8",
            "\"source\": \"Ignore\"",
        ]),
        Marker::new("4", Rc::new(marker), Vec::new()).to_string()
    );

    let marker = Rc::new(a_marker_full(
        "a/res2",
        MarkerSeverity::WARNING,
        1,
        2,
        1,
        8,
        "Warning message",
        "",
        Some(vec![IRelatedInformation {
            resource: uri_file("a/res3"),
            message: "some info".to_string(),
            start_line_number: 2,
            start_column: 5,
            end_line_number: 2,
            end_column: 10,
        }]),
    ));
    let related_information: Vec<Rc<RelatedInformation>> =
        marker.related_information.iter().flatten().map(|r| Rc::new(RelatedInformation::new("6", marker.clone(), r.clone()))).collect();
    let test_object = Marker::new("5", marker, related_information);

    assert_eq!(
        json_object(&[
            "\"owner\": \"someOwner\"",
            "\"resource\": \"/a/res2\"",
            "\"severity\": 4",
            "\"message\": \"Warning message\"",
            "\"startLineNumber\": 1",
            "\"startColumn\": 2",
            "\"endLineNumber\": 1",
            "\"endColumn\": 8",
            "\"source\": \"\"",
            "\"relatedInformation\": [\n\t\t{\n\t\t\t\"startLineNumber\": 2,\n\t\t\t\"startColumn\": 5,\n\t\t\t\"endLineNumber\": 2,\n\t\t\t\"endColumn\": 10,\n\t\t\t\"message\": \"some info\",\n\t\t\t\"resource\": \"/a/res3\"\n\t\t}\n\t]",
        ]),
        test_object.to_string()
    );
}

#[test]
fn markers_for_same_document_but_different_fragment() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = test_markers_model(&guard, vec![with_range(MarkerSeverity::ERROR, 1)]);

    assert_eq!(model.total(), 1);

    let document = URI::parse("foo://test/path/file", false);
    let frag1 = URI::parse("foo://test/path/file#1", false);
    let frag2 = URI::parse("foo://test/path/file#two", false);

    model.set_resource_markers(vec![(
        document.clone(),
        vec![with_resource(a_default_marker(), &frag1), with_resource(a_default_marker(), &frag2)],
    )]);

    assert_eq!(model.total(), 3);
    let a = model.get_resource_markers(&document).expect("há marcadores do documento");
    let b = model.get_resource_markers(&frag1).expect("há marcadores do fragmento 1");
    let c = model.get_resource_markers(&frag2).expect("há marcadores do fragmento two");
    assert!(Rc::ptr_eq(&a, &b));
    assert!(Rc::ptr_eq(&a, &c));

    model.set_resource_markers(vec![(document.clone(), vec![with_resource(a_default_marker(), &frag2)])]);
    assert_eq!(model.total(), 2);
}

#[test]
fn problems_are_no_sorted_correctly_99135() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = test_markers_model(&guard, Vec::new());
    assert_eq!(model.total(), 0);

    let document = URI::parse("foo://test/path/file", false);
    let frag1 = URI::parse("foo://test/path/file#1", false);
    let frag2 = URI::parse("foo://test/path/file#2", false);

    model.set_resource_markers(vec![(
        frag1.clone(),
        vec![with_resource(a_default_marker(), &frag1), with_resource(a_marker("some resource", MarkerSeverity::WARNING), &frag1)],
    )]);

    model.set_resource_markers(vec![(frag2.clone(), vec![with_resource(a_default_marker(), &frag2)])]);

    assert_eq!(model.total(), 3);
    let markers = model.get_resource_markers(&document).map(|resource_markers| resource_markers.markers()).expect("há marcadores do documento");
    assert_eq!(markers.iter().map(|m| m.marker.severity).collect::<Vec<_>>(), vec![MarkerSeverity::ERROR, MarkerSeverity::ERROR, MarkerSeverity::WARNING]);
    assert_eq!(
        markers.iter().map(|m| m.marker.resource.to_string(false)).collect::<Vec<_>>(),
        vec![frag1.to_string(false), frag2.to_string(false), frag1.to_string(false)]
    );
}
