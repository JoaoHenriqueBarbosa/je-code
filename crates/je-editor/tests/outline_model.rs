//! Porte de `src/vs/editor/contrib/documentSymbols/test/browser/outlineModel.test.ts` (suíte `OutlineModel`), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `teardown` (`disposables.clear()`) são o `LeakGuard` de
//!   [`run_test`], que também roda o corpo com o relógio virtual (`runWithFakedTimers`) porque os dois primeiros casos
//!   são `async`.
//! - `languageFeaturesService.documentSymbolProvider` (o `LanguageFeaturesService` do módulo, compartilhado entre os
//!   casos) é um [`LanguageFeatureRegistry`] novo por caso.
//! - `createModelServices(disposables)` entrega o `IModelService` que o serviço de outline usa só para ouvir
//!   `onModelRemoved`; o recorte do porte é [`ModelRemovedSource`] e o [`TestModelService`] é o serviço com esse evento,
//!   que nenhum dos casos dispara (os modelos deles não passam pelo serviço).
//! - `mock<IEnvironmentService>()` com `isBuilt` e `isExtensionDevelopment` é [`MockEnvironmentService`]: o que o
//!   caso não lê é `unimplemented!`, como um `mock` sem a propriedade.
//! - `new NullLogService()` é `NullLogService::shared()`, e `new LanguageFeatureDebounceService(log, env)` é
//!   `LanguageFeatureDebounceService::new`.
//! - `createTextModel('foo', undefined, undefined, URI.file('/fome/path.foo'))` é `create_text_model_with` e
//!   `model.applyEdits([{ text, range }])` é `apply_edits` com `EditOperation::replace`.
//! - O provedor literal com `provideDocumentSymbols` é [`CountingProvider`] (conta as chamadas) e [`CancellableProvider`]
//!   (espera o cancelamento do `token` e então resolve com `null`).
//! - `{ pattern: '**/path.foo' }` é um `LanguageFilter` com `pattern`.
//! - `fakeMarker(range)` (um `IMarker` com os campos do intervalo) é um `OutlineMarker` de gravidade `Error`; o `label` e o
//!   `URI` do `new OutlineModel(null!)` e do `new OutlineGroup(..., null!, ...)`, que o caso não lê, são vazios e um URI
//!   qualquer.
//! - A subclasse anônima com `readyForTesting` é o `ready_for_testing` do próprio `OutlineModel` (ver o módulo de produção).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::edit_operation::EditOperation;
use je_editor::language_feature_debounce::LanguageFeatureDebounceService;
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::{DocumentSymbol, DocumentSymbolProvider, ProviderResult, SymbolKind};
use je_editor::language_selector::{FilterPattern, LanguageFilter, LanguageSelector};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::outline_model::{
    IOutlineModelService, ModelRemovedSource, OutlineElement, OutlineGroup, OutlineMarker, OutlineModel, OutlineModelService,
    TreeElement,
};
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_platform::environment::environment::{
    DisableExtensions, EnvironmentService, ExtensionHostDebugParams, ExtensionKind, SyncMode,
};
use je_platform::log::NullLogService;
use je_platform::markers::MarkerSeverity;
use support::test_text_model::create_text_model_with;

/// O que o mocha faz em volta de cada `test(...)` da suíte: o `ensureNoDisposablesAreLeakedInTestSuite()`, o corpo (com o
/// relógio virtual) e o `teardown`, nessa ordem.
fn run_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<LeakGuard>) -> F) {
    let ds = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(body(Rc::clone(&ds)));
}

/// `mock<IEnvironmentService>()` com `isBuilt` e `isExtensionDevelopment`.
struct MockEnvironmentService {
    is_built: bool,
    is_extension_development: bool,
}

impl EnvironmentService for MockEnvironmentService {
    fn state_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn user_roaming_data_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn keyboard_layout_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn argv_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn untitled_workspaces_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn workspace_storage_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn local_history_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn cache_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn app_shared_data_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn user_data_sync_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn sync(&self) -> Option<SyncMode> {
        unimplemented!("mock")
    }
    fn continue_on(&self) -> Option<String> {
        unimplemented!("mock")
    }
    fn edit_session_id(&self) -> Option<String> {
        unimplemented!("mock")
    }
    fn debug_extension_host(&self) -> ExtensionHostDebugParams {
        unimplemented!("mock")
    }
    fn is_extension_development(&self) -> bool {
        self.is_extension_development
    }
    fn disable_extensions(&self) -> DisableExtensions {
        unimplemented!("mock")
    }
    fn skip_builtin_extensions(&self) -> Vec<String> {
        unimplemented!("mock")
    }
    fn extension_development_location_uri(&self) -> Option<Vec<URI>> {
        unimplemented!("mock")
    }
    fn extension_development_kind(&self) -> Option<Vec<ExtensionKind>> {
        unimplemented!("mock")
    }
    fn extension_tests_location_uri(&self) -> Option<URI> {
        unimplemented!("mock")
    }
    fn logs_home(&self) -> URI {
        unimplemented!("mock")
    }
    fn log_level(&self) -> Option<String> {
        unimplemented!("mock")
    }
    fn extension_log_level(&self) -> Option<Vec<(String, String)>> {
        unimplemented!("mock")
    }
    fn verbose(&self) -> bool {
        unimplemented!("mock")
    }
    fn is_built(&self) -> bool {
        self.is_built
    }
    fn disable_telemetry(&self) -> bool {
        unimplemented!("mock")
    }
    fn disable_experiments(&self) -> bool {
        unimplemented!("mock")
    }
    fn service_machine_id_resource(&self) -> URI {
        unimplemented!("mock")
    }
    fn agent_sessions_workspace(&self) -> URI {
        unimplemented!("mock")
    }
    fn policy_file(&self) -> Option<URI> {
        unimplemented!("mock")
    }
}

/// O `IModelService` do caso, só com `onModelRemoved`.
struct TestModelService {
    on_model_removed: Rc<Emitter<Rc<TextModel>>>,
}

impl ModelRemovedSource for TestModelService {
    fn on_model_removed(&self) -> Event<Rc<TextModel>> {
        self.on_model_removed.event()
    }
}

/// O provedor `{ provideDocumentSymbols() { count += 1; return []; } }`.
struct CountingProvider {
    count: Rc<Cell<i32>>,
}

impl DocumentSymbolProvider for CountingProvider {
    fn provide_document_symbols(&self, _model: &Rc<TextModel>, _token: &CancellationToken) -> ProviderResult<Vec<DocumentSymbol>> {
        self.count.set(self.count.get() + 1);
        Promise::resolved(Some(Vec::new()))
    }
}

/// O provedor que devolve uma promessa que só resolve (com `null`) quando o `token` é cancelado.
struct CancellableProvider {
    is_cancelled: Rc<Cell<bool>>,
}

impl DocumentSymbolProvider for CancellableProvider {
    fn provide_document_symbols(&self, _model: &Rc<TextModel>, token: &CancellationToken) -> ProviderResult<Vec<DocumentSymbol>> {
        let is_cancelled = Rc::clone(&self.is_cancelled);
        Promise::with_executor(move |resolver| {
            let holder: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
            let listener_holder = Rc::clone(&holder);
            let listener = token.on_cancellation_requested().listen(move |_| {
                is_cancelled.set(true);
                resolver.resolve(None);
                let listener = listener_holder.borrow_mut().take();
                if let Some(listener) = listener {
                    listener.dispose();
                }
            });
            *holder.borrow_mut() = Some(listener);
        })
    }
}

/// `{ pattern: '**/path.foo' }`.
fn path_foo_selector() -> LanguageSelector {
    LanguageSelector::Filter(LanguageFilter {
        pattern: Some(FilterPattern::from("**/path.foo")),
        ..LanguageFilter::default()
    })
}

/// Os objetos que os dois primeiros casos montam: o serviço de outline, o modelo e o registro de provedores.
struct OutlineFixture {
    service: OutlineModelService,
    model: Rc<TextModel>,
    registry: LanguageFeatureRegistry<dyn DocumentSymbolProvider>,
}

fn create_fixture(ds: &LeakGuard) -> OutlineFixture {
    let registry: LanguageFeatureRegistry<dyn DocumentSymbolProvider> = LanguageFeatureRegistry::new(None);
    let env_service = MockEnvironmentService { is_built: true, is_extension_development: false };
    let model_service = TestModelService { on_model_removed: ds.add_rc(Emitter::new()) };
    let service = OutlineModelService::new(
        registry.clone(),
        &LanguageFeatureDebounceService::new(NullLogService::shared(), &env_service),
        &model_service,
    );
    let model = create_text_model_with(
        "foo",
        None,
        RelaxedTextModelCreationOptions::default(),
        Some(URI::file("/fome/path.foo", Platform::Posix)),
    );
    OutlineFixture { service, model, registry }
}

#[test]
fn outline_model_create_cached() {
    run_test(|ds| async move {
        let OutlineFixture { service, model, registry } = create_fixture(&ds);

        let count = Rc::new(Cell::new(0));
        let reg = registry.register(path_foo_selector(), Rc::new(CountingProvider { count: Rc::clone(&count) }));

        service.get_or_create(&model, &CancellationToken::None).await.expect("a criação não rejeita");
        assert_eq!(count.get(), 1);

        // cached
        service.get_or_create(&model, &CancellationToken::None).await.expect("a criação não rejeita");
        assert_eq!(count.get(), 1);

        // new version
        model.apply_edits(&[EditOperation::replace(Range::new(1, 1, 1, 1), Some(JsString::from("XXX")))]);
        service.get_or_create(&model, &CancellationToken::None).await.expect("a criação não rejeita");
        assert_eq!(count.get(), 2);

        reg.dispose();
        model.dispose();
        service.dispose();
    });
}

#[test]
fn outline_model_create_cached_cancel() {
    run_test(|ds| async move {
        let OutlineFixture { service, model, registry } = create_fixture(&ds);
        let is_cancelled = Rc::new(Cell::new(false));

        let reg = registry.register(
            path_foo_selector(),
            Rc::new(CancellableProvider { is_cancelled: Rc::clone(&is_cancelled) }),
        );

        assert!(!is_cancelled.get());
        let s1 = CancellationTokenSource::new(None);
        let _ = service.get_or_create(&model, &s1.token());
        let s2 = CancellationTokenSource::new(None);
        let _ = service.get_or_create(&model, &s2.token());

        s1.cancel();
        assert!(!is_cancelled.get());

        s2.cancel();
        assert!(is_cancelled.get());

        reg.dispose();
        model.dispose();
        service.dispose();
    });
}

/// `fakeSymbolInformation(range, name = 'foo')`.
fn fake_symbol_information(range: Range) -> DocumentSymbol {
    DocumentSymbol {
        name: "foo".to_owned(),
        detail: "fake".to_owned(),
        kind: SymbolKind::Boolean,
        tags: Vec::new(),
        container_name: None,
        selection_range: range,
        range,
        children: None,
    }
}

/// `fakeMarker(range)`.
fn fake_marker(range: Range) -> OutlineMarker {
    OutlineMarker { range, severity: MarkerSeverity::ERROR }
}

/// `parent.children.set(child.id, child)` de um `OutlineElement`.
fn set_child(parent: &TreeElement, child: &Rc<OutlineElement>) {
    parent.children().borrow_mut().insert(child.id.clone(), TreeElement::Element(Rc::clone(child)));
}

/// `data.sort(Range.compareRangesUsingStarts)`.
fn sort_by_starts(data: &mut [OutlineMarker]) {
    data.sort_by(|a, b| Range::compare_ranges_using_starts(Some(&a.range), Some(&b.range)).cmp(&0));
}

#[test]
fn outline_element_update_marker() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let e0 = OutlineElement::new("foo1", None, fake_symbol_information(Range::new(1, 1, 1, 10)));
    let e1 = OutlineElement::new("foo2", None, fake_symbol_information(Range::new(2, 1, 5, 1)));
    let e2 = OutlineElement::new("foo3", None, fake_symbol_information(Range::new(6, 1, 10, 10)));

    let group = OutlineGroup::new("group", None, "", 1);
    let group_element = TreeElement::Group(Rc::clone(&group));
    set_child(&group_element, &e0);
    set_child(&group_element, &e1);
    set_child(&group_element, &e2);

    let mut data = vec![
        fake_marker(Range::new(6, 1, 6, 7)),
        fake_marker(Range::new(1, 1, 1, 4)),
        fake_marker(Range::new(10, 2, 14, 1)),
    ];
    sort_by_starts(&mut data); // model does this

    group.update_marker(&mut data);
    assert_eq!(data.len(), 0); // all 'stolen'
    assert_eq!(e0.marker().expect("e0 tem marcador").count, 1);
    assert_eq!(e1.marker(), None);
    assert_eq!(e2.marker().expect("e2 tem marcador").count, 2);

    group.update_marker(&mut Vec::new());
    assert_eq!(e0.marker(), None);
    assert_eq!(e1.marker(), None);
    assert_eq!(e2.marker(), None);
}

#[test]
fn outline_element_update_marker_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let p = OutlineElement::new("A", None, fake_symbol_information(Range::new(1, 1, 11, 1)));
    let c1 = OutlineElement::new("A/B", None, fake_symbol_information(Range::new(2, 4, 5, 4)));
    let c2 = OutlineElement::new("A/C", None, fake_symbol_information(Range::new(6, 4, 9, 4)));

    let group = OutlineGroup::new("group", None, "", 1);
    set_child(&TreeElement::Group(Rc::clone(&group)), &p);
    let p_element = TreeElement::Element(Rc::clone(&p));
    set_child(&p_element, &c1);
    set_child(&p_element, &c2);

    let mut data = vec![fake_marker(Range::new(2, 4, 5, 4))];

    group.update_marker(&mut data);
    assert_eq!(p.marker().expect("p tem marcador").count, 0);
    assert_eq!(c1.marker().expect("c1 tem marcador").count, 1);
    assert_eq!(c2.marker(), None);

    data = vec![
        fake_marker(Range::new(2, 4, 5, 4)),
        fake_marker(Range::new(2, 6, 2, 8)),
        fake_marker(Range::new(7, 6, 7, 8)),
    ];
    group.update_marker(&mut data);
    assert_eq!(p.marker().expect("p tem marcador").count, 0);
    assert_eq!(c1.marker().expect("c1 tem marcador").count, 2);
    assert_eq!(c2.marker().expect("c2 tem marcador").count, 1);

    data = vec![fake_marker(Range::new(1, 4, 1, 11)), fake_marker(Range::new(7, 6, 7, 8))];
    group.update_marker(&mut data);
    assert_eq!(p.marker().expect("p tem marcador").count, 1);
    assert_eq!(c1.marker(), None);
    assert_eq!(c2.marker().expect("c2 tem marcador").count, 1);
}

#[test]
fn outline_element_update_marker_multiple_groups() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let model = OutlineModel::new(URI::parse("null:", false));
    let model_element = TreeElement::Model(Rc::clone(&model));

    let g1 = OutlineGroup::new("g1", Some(&model_element), "", 1);
    let g1_element = TreeElement::Group(Rc::clone(&g1));
    model.children().borrow_mut().insert("g1".to_owned(), g1_element.clone());
    let c1 = OutlineElement::new("c1", Some(&g1_element), fake_symbol_information(Range::new(1, 1, 11, 1)));
    set_child(&g1_element, &c1);

    let g2 = OutlineGroup::new("g2", Some(&model_element), "", 1);
    let g2_element = TreeElement::Group(Rc::clone(&g2));
    model.children().borrow_mut().insert("g2".to_owned(), g2_element.clone());
    let c2 = OutlineElement::new("c2", Some(&g2_element), fake_symbol_information(Range::new(1, 1, 7, 1)));
    set_child(&g2_element, &c2);
    let c2_element = TreeElement::Element(Rc::clone(&c2));
    let c2_1 = OutlineElement::new("c2.1", Some(&c2_element), fake_symbol_information(Range::new(1, 3, 2, 19)));
    set_child(&c2_element, &c2_1);
    let c2_2 = OutlineElement::new("c2.2", Some(&c2_element), fake_symbol_information(Range::new(4, 1, 6, 10)));
    set_child(&c2_element, &c2_2);

    model.ready_for_testing();

    let mut data = vec![fake_marker(Range::new(1, 1, 2, 8)), fake_marker(Range::new(6, 1, 6, 98))];

    model.update_marker(&mut data);

    assert_eq!(c1.marker().expect("c1 tem marcador").count, 2);
    assert_eq!(c2_1.marker().expect("c2.1 tem marcador").count, 1);
    assert_eq!(c2_2.marker().expect("c2.2 tem marcador").count, 1);
}
