//! Porte de `src/vs/workbench/contrib/tasks/test/common/problemCollectors.test.ts` (suíte `ProblemCollectors`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo do caso; o `store.add` do original é
//!   `store.add_rc` sobre o store do guarda.
//! - **`runWithFakedTimers({ useFakeTimers: true }, async () => ...)`** é `run_with_faked_timers`.
//! - **`CountingRegExp extends RegExp`.** O `JsRegExp` do porte conta as chamadas de `exec` (`exec_count`), o que a subclasse
//!   do original faz com `count++`.
//! - **`mock<IModelService>` e `mock<IMarkerService>`.** São dublês que só implementam o que o coletor lê (`onModelAdded` nunca
//!   dispara, `onModelRemoved` e `onMarkerChanged` são os emissores do caso, `getModels` e `read` devolvem listas vazias).
//! - **`mock<ITextModel>` com só `uri`.** O modelo do porte é o `TextModel` concreto; o caso cria um modelo vazio com o recurso
//!   `test:///file.ts`.
//! - **`await timeout(ms)`** espera a promessa do `timeout` do guarda de testes.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_language_configuration_service.rs"]
mod test_language_configuration_service;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::arrays::OneOrMany;
use je_base::event::{Emitter, Event};
use je_base::js_regexp::JsRegExp;
use je_base::lifecycle::DisposableRef;
use je_base::uri::URI;
use je_editor::language_service::ILanguageSelection;
use je_editor::model::{RelaxedTextModelCreationOptions, TextModelCreationOptions};
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::text_model_edit_source::TextModelEditSource;
use je_platform::markers::markers::{IMarkerService, Marker, MarkerChangeOptions, MarkerData, MarkerReadOptions, MarkerStatistics, ResourceMarker};
use je_workbench::contrib::tasks::problem_collectors::{IProblemMatcher, WatchingProblemCollector};
use je_workbench::contrib::tasks::problem_matcher::{
    ApplyToKind, FileLocationKind, ProblemMatcher, ProblemPattern, WatchingMatcher, WatchingPattern,
};
use test_text_model::create_text_model_with;

struct TestModelService {
    model_removed: Rc<Emitter<Rc<TextModel>>>,
}

impl IModelService for TestModelService {
    fn get_model(&self, _resource: &URI) -> Option<Rc<TextModel>> {
        None
    }

    fn get_models(&self) -> Vec<Rc<TextModel>> {
        Vec::new()
    }

    fn on_model_added(&self) -> Event<Rc<TextModel>> {
        Event::none()
    }

    fn on_model_removed(&self) -> Event<Rc<TextModel>> {
        self.model_removed.event()
    }

    fn create_model(&self, _value: TextSource, _language_selection: Option<&dyn ILanguageSelection>, _resource: Option<&URI>, _is_for_simple_widget: bool) -> Rc<TextModel> {
        panic!("IModelService.createModel não é usado por este caso")
    }

    fn update_model(&self, _model: &TextModel, _value: TextSource, _reason: Option<TextModelEditSource>) {
        panic!("IModelService.updateModel não é usado por este caso")
    }

    fn destroy_model(&self, _resource: &URI) {
        panic!("IModelService.destroyModel não é usado por este caso")
    }

    fn get_creation_options(&self, _language: &str, _resource: Option<&URI>, _is_for_simple_widget: bool) -> TextModelCreationOptions {
        panic!("IModelService.getCreationOptions não é usado por este caso")
    }
}

struct TestMarkerService {
    marker_changed: Rc<Emitter<Vec<URI>>>,
}

impl IMarkerService for TestMarkerService {
    fn get_statistics(&self) -> MarkerStatistics {
        panic!("IMarkerService.getStatistics não é usado por este caso")
    }

    fn is_source_truncated(&self, _resource: &URI) -> bool {
        panic!("IMarkerService.isSourceTruncated não é usado por este caso")
    }

    fn change_one(&self, _owner: &str, _resource: &URI, _markers: Vec<MarkerData>, _options: Option<MarkerChangeOptions>) {
        panic!("IMarkerService.changeOne não é usado por este caso")
    }

    fn change_all(&self, _owner: &str, _data: Vec<ResourceMarker>) {
        panic!("IMarkerService.changeAll não é usado por este caso")
    }

    fn remove(&self, _owner: &str, _resources: &[URI]) {
        panic!("IMarkerService.remove não é usado por este caso")
    }

    fn read(&self, _filter: Option<&MarkerReadOptions>) -> Vec<Marker> {
        Vec::new()
    }

    fn install_resource_filter(&self, _resource: &URI, _reason: &str) -> DisposableRef {
        panic!("IMarkerService.installResourceFilter não é usado por este caso")
    }

    fn on_marker_changed(&self) -> Event<Vec<URI>> {
        self.marker_changed.event()
    }
}

#[test]
fn does_not_retain_replayed_lines_when_a_model_is_removed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let model_removed = Emitter::<Rc<TextModel>>::new();
        store.add_rc(model_removed.clone());
        let model_service: Rc<dyn IModelService> = Rc::new(TestModelService { model_removed: model_removed.clone() });

        let marker_changed = Emitter::<Vec<URI>>::new();
        store.add_rc(marker_changed.clone());
        let marker_service: Rc<dyn IMarkerService> = Rc::new(TestMarkerService { marker_changed: marker_changed.clone() });

        let problem_pattern = Rc::new(JsRegExp::from_str("^never$", "").expect("regex válida"));
        let mut problem_matcher = ProblemMatcher::new(
            "test",
            ApplyToKind::AllDocuments,
            FileLocationKind::Absolute,
            OneOrMany::One(ProblemPattern::new(problem_pattern.clone())),
        );
        problem_matcher.watching = Some(WatchingMatcher {
            active_on_start: true,
            begins_pattern: WatchingPattern { regexp: Rc::new(JsRegExp::from_str("^begin$", "").expect("regex válida")), file: None },
            ends_pattern: WatchingPattern { regexp: Rc::new(JsRegExp::from_str("^end$", "").expect("regex válida")), file: None },
        });
        let collector = store.add_rc(WatchingProblemCollector::new(vec![problem_matcher], marker_service, model_service, None, None));
        let resource = URI::parse("test:///file.ts", false);
        let model = store.add_rc(create_text_model_with("", None, RelaxedTextModelCreationOptions::default(), Some(resource.clone())));

        collector.process_line("output");
        timeout(0).await.expect("o timeout resolve");
        let mut match_counts = vec![problem_pattern.exec_count()];

        for _ in 0..2 {
            model_removed.fire(model.clone());
            marker_changed.fire(vec![resource.clone()]);
            timeout(600).await.expect("o timeout resolve");
            match_counts.push(problem_pattern.exec_count());
        }

        assert_eq!(match_counts, vec![1, 2, 3]);
    });
}
