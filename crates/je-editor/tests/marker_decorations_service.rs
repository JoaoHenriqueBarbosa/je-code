//! Porte de `src/vs/editor/test/common/services/markerDecorationsService.test.ts` (suíte `MarkerDecorationsService`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso, e o `disposables` da
//!   suíte é o próprio guarda. O `setup` (`createModelServices`, `MarkerService` e `MarkerDecorationsService`) é
//!   [`setup`]: o `ModelService` recebe a configuração, o `TestTextResourcePropertiesService` e o `UndoRedoService` de
//!   teste.
//! - `await changed` (`Event.toPromise(markerService.onMarkerChanged)`) é esvaziar as microtarefas, porque o
//!   `MarkerService` dispara `onMarkerChanged` numa microtarefa; o resultado observável é o mesmo.
//! - `assert.deepStrictEqual` é `assert_eq!` sobre tuplas e structs; `IMarkerLimit` é `MarkerLimit`, e
//!   `isMarkersTruncated: number | undefined` é `Option<i64>`.
//! - `disposables.add(model)` é `ds.add_rc(model)`; `serviceDisposables` do último caso é um `DisposableStore` registrado
//!   no guarda antes do modelo, como no original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::runtime::run_microtasks;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_editor::model_service::ModelService;
use je_editor::range::Range;
use je_editor::services::marker_decorations::{IMarkerDecorationsService, MarkerLimit};
use je_editor::services::marker_decorations_service::MarkerDecorationsService;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_editor::undo_redo_service::UndoRedoService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::markers::marker_service::MarkerService;
use je_platform::markers::{IMarkerService, MarkerChangeOptions, MarkerData, MarkerReadOptions, MarkerSeverity};
use support::test_text_resource_properties_service::TestTextResourcePropertiesService;

struct Fixture {
    ds: LeakGuard,
    model_service: Rc<ModelService>,
    marker_service: Rc<MarkerService>,
    decoration_service: Rc<MarkerDecorationsService>,
}

/// O `setup` da suíte.
fn setup() -> Fixture {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let config_service = TestConfigurationService::new(None);
    let _ = config_service.set_user_configuration("files", Dynamic::object([("eol", Dynamic::string("\n"))]), None);
    let configuration_service: Rc<dyn IConfigurationService> = config_service;
    let model_service = ds.add_rc(ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service),
        UndoRedoService::for_tests(),
    ));
    let marker_service = ds.add_rc(MarkerService::new());
    let decoration_service = ds.add_rc(MarkerDecorationsService::new(model_service.clone(), marker_service.clone()));
    Fixture { ds, model_service, marker_service, decoration_service }
}

fn file_uri(path: &str) -> URI {
    URI::file(path, je_base::path::Environment::platform(&je_base::path::ProcessEnvironment))
}

impl Fixture {
    fn create_model_at(&self, uri: &URI) -> Rc<TextModel> {
        let text = vec!["value"; 5100].join("\n");
        self.ds.add_rc(self.model_service.create_model(text.as_str(), None, Some(uri), false))
    }

    fn create_model(&self) -> Rc<TextModel> {
        self.create_model_at(&file_uri("/diagnostics.txt"))
    }

    fn change_markers(&self, model: &TextModel, count: usize, owner: &str, is_truncated: bool) {
        self.marker_service.change_one(owner, &model.uri(), create_markers(count), Some(MarkerChangeOptions { is_truncated }));
        run_microtasks();
    }

    fn limited(&self, uri: &URI) -> Option<i64> {
        self.decoration_service.get_marker_limits(uri).and_then(|limits| limits.is_markers_truncated)
    }

    fn decoration_ids(model: &TextModel) -> Vec<String> {
        model.get_all_decorations(0, false, false).into_iter().map(|decoration| decoration.id).collect()
    }
}

fn create_markers(count: usize) -> Vec<MarkerData> {
    let severities = [MarkerSeverity::ERROR, MarkerSeverity::WARNING, MarkerSeverity::INFO, MarkerSeverity::HINT];
    (0..count)
        .map(|index| MarkerData::new(severities[index % severities.len()], format!("problem {}", index + 1), index as i64 + 1, 1, index as i64 + 1, 2))
        .collect()
}

#[test]
fn reports_overflow_only_above_5000_while_preserving_all_diagnostics() {
    let f = setup();
    let model = f.create_model();
    let mut states = Vec::new();
    for count in [4999, 5000, 5001, 5100, 5000, 0] {
        f.change_markers(&model, count, "test", false);
        let read = MarkerReadOptions { resource: Some(model.uri()), ..Default::default() };
        states.push((f.marker_service.read(Some(&read)).len(), f.decoration_service.get_live_markers(&model.uri()).len(), f.limited(&model.uri())));
    }

    assert_eq!(
        states,
        vec![
            (4999, 4999, None),
            (5000, 5000, None),
            (5001, 5000, Some(5000)),
            (5100, 5000, Some(5000)),
            (5000, 5000, None),
            (0, 0, None),
        ]
    );
}

#[test]
fn separately_notifies_when_only_the_limit_changes_without_replacing_decorations() {
    let f = setup();
    let model = f.create_model();
    f.change_markers(&model, 5000, "test", false);
    let decoration_ids = Fixture::decoration_ids(&model);
    let marker_changes: Rc<RefCell<Vec<URI>>> = Rc::default();
    let changes = marker_changes.clone();
    f.ds.add(f.decoration_service.on_did_change_marker().listen(move |model| changes.borrow_mut().push(model.uri())));
    let exceeded_limit_changes: Rc<RefCell<Vec<(URI, Option<i64>)>>> = Rc::default();
    let limit_changes = exceeded_limit_changes.clone();
    let service = Rc::downgrade(&f.decoration_service);
    f.ds.add(f.decoration_service.on_did_change_marker_limit().listen(move |model| {
        let limited = service.upgrade().and_then(|service| service.get_marker_limits(&model.uri())).and_then(|limits| limits.is_markers_truncated);
        limit_changes.borrow_mut().push((model.uri(), limited));
    }));

    f.change_markers(&model, 1, "overflow", false);
    f.change_markers(&model, 2, "overflow", false);
    f.change_markers(&model, 0, "overflow", false);

    assert_eq!(*marker_changes.borrow(), Vec::<URI>::new());
    assert_eq!(*exceeded_limit_changes.borrow(), vec![(model.uri(), Some(5000)), (model.uri(), None)]);
    assert_eq!(Fixture::decoration_ids(&model), decoration_ids);
}

#[test]
fn reports_diagnostic_truncation_independently_of_the_decoration_limit() {
    let f = setup();
    let model = f.create_model();
    f.change_markers(&model, 5001, "test", false);
    let decoration_ids = Fixture::decoration_ids(&model);
    let marker_changes: Rc<RefCell<Vec<URI>>> = Rc::default();
    let changes = marker_changes.clone();
    f.ds.add(f.decoration_service.on_did_change_marker().listen(move |model| changes.borrow_mut().push(model.uri())));
    let limit_changes: Rc<RefCell<Vec<Option<MarkerLimit>>>> = Rc::default();
    let limits = limit_changes.clone();
    let service = Rc::downgrade(&f.decoration_service);
    f.ds.add(f.decoration_service.on_did_change_marker_limit().listen(move |model| {
        limits.borrow_mut().push(service.upgrade().and_then(|service| service.get_marker_limits(&model.uri())));
    }));

    f.change_markers(&model, 1, "truncated", true);
    let truncated_decoration_ids = Fixture::decoration_ids(&model);
    f.change_markers(&model, 1, "truncated", false);
    let restored_decoration_ids = Fixture::decoration_ids(&model);
    f.change_markers(&model, 0, "test", false);
    f.change_markers(&model, 1, "truncated", true);

    let limit = |is_markers_source_truncated, is_markers_truncated| Some(MarkerLimit { is_markers_source_truncated, is_markers_truncated });
    assert_eq!(*limit_changes.borrow(), vec![limit(true, Some(5000)), limit(false, Some(5000)), limit(false, None), limit(true, None)]);
    assert_eq!(*marker_changes.borrow(), vec![model.uri(), model.uri()]);
    let decorations_unchanged_while_over_limit = [truncated_decoration_ids, restored_decoration_ids].iter().all(|ids| *ids == decoration_ids);
    assert!(decorations_unchanged_while_over_limit);
}

#[test]
fn honors_resource_filters_and_restores_the_limit_when_filtering_ends() {
    let f = setup();
    let model = f.create_model();
    f.change_markers(&model, 5100, "test", false);

    let filter = f.ds.add(f.marker_service.install_resource_filter(&model.uri(), "test"));
    run_microtasks();
    let read = MarkerReadOptions { resource: Some(model.uri()), ignore_resource_filters: true, ..Default::default() };
    let filtered_state = (f.limited(&model.uri()), f.decoration_service.get_live_markers(&model.uri()).len(), f.marker_service.read(Some(&read)).len());

    filter.dispose();
    run_microtasks();

    assert_eq!(filtered_state, (None, 1, 5100));
    assert_eq!((f.limited(&model.uri()), f.decoration_service.get_live_markers(&model.uri()).len()), (Some(5000), 5000));
}

#[test]
fn preserves_suppression_after_applying_the_decoration_limit() {
    let f = setup();
    let model = f.create_model();
    f.change_markers(&model, 5001, "test", false);
    let suppression = f.ds.add(f.decoration_service.add_marker_suppression(&model.uri(), Range::new(1, 1, 100, 2)));
    let lines: Vec<i32> = f.decoration_service.get_live_markers(&model.uri()).iter().map(|(range, _)| range.start_line_number).collect();
    let suppressed_state = (f.limited(&model.uri()), lines);

    suppression.dispose();

    assert_eq!(suppressed_state, (Some(5000), (101..=5000).collect::<Vec<i32>>()));
    assert_eq!((f.limited(&model.uri()), f.decoration_service.get_live_markers(&model.uri()).len()), (Some(5000), 5000));
}

#[test]
fn keeps_limits_separate_for_resources_with_the_same_path() {
    let f = setup();
    let local = f.create_model();
    let remote_uri = URI::from(
        &UriComponents { scheme: Some("vscode-remote".into()), authority: Some("first".into()), path: Some(local.uri().path().clone()), ..Default::default() },
        false,
    );
    let remote = f.create_model_at(&remote_uri);
    let other_remote = f.create_model_at(&remote.uri().with(&UriChange { authority: Field::Set("second".into()), ..Default::default() }));
    f.change_markers(&local, 5001, "test", false);
    f.change_markers(&remote, 5000, "test", false);
    f.change_markers(&other_remote, 5100, "test", false);

    let limits: Vec<Option<i64>> = [&local, &remote, &other_remote].iter().map(|model| f.limited(&model.uri())).collect();
    assert_eq!(limits, vec![Some(5000), None, Some(5000)]);
}

#[test]
fn clears_the_limit_on_model_disposal_and_restores_it_when_reopening() {
    let f = setup();
    let model = f.create_model();
    f.change_markers(&model, 5001, "test", false);
    model.dispose();
    let after_disposal = f.limited(&model.uri());
    let reopened = f.create_model_at(&model.uri());

    assert_eq!(after_disposal, None);
    assert_eq!(f.limited(&reopened.uri()), Some(5000));
    assert_eq!(f.decoration_service.get_live_markers(&reopened.uri()).len(), 5000);
}

#[test]
fn initializes_limits_for_existing_models_and_diagnostics() {
    let f = setup();
    let service_disposables = f.ds.add_rc(DisposableStore::new());
    let model = f.create_model();
    f.change_markers(&model, 5001, "test", false);
    f.decoration_service.dispose();
    let restored_service = service_disposables.add_rc(MarkerDecorationsService::new(f.model_service.clone(), f.marker_service.clone()));

    assert_eq!(restored_service.get_marker_limits(&model.uri()).and_then(|limits| limits.is_markers_truncated), Some(5000));
    assert_eq!(restored_service.get_live_markers(&model.uri()).len(), 5000);
}
