//! Porte de `src/vs/workbench/contrib/multiDiffEditor/test/browser/multiDiffEditorConfiguration.test.ts` (suíte
//! `MultiDiffEditorConfiguration`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no início de cada caso; o `disposables.add(...)` do
//!   original é `guard.add(...)` e `guard.add_rc(...)`. Os casos `async` rodam no relógio real, como no mocha.
//! - `Registry.as<IConfigurationRegistry>(...).getConfigurationProperties()` capturado no topo do arquivo é a leitura do
//!   registro logo depois de `register_multi_diff_editor_configuration()` e de `register_hide_original_line_numbers_setting()`
//!   (o registro de configuração do Rust é explícito, ver o cabeçalho de `multi_diff_editor.rs`); cada caso roda numa thread
//!   própria, então a captura do original vira o [`registered_properties`] de cada caso. A propriedade
//!   `diffEditor.hideOriginalLineNumbers` é o recorte do esquema do editor (que o `import '.../editorConfigurationSchema.js'`
//!   do original carrega).
//! - `assert.deepStrictEqual` sobre objetos de propriedades é `assert_deep_strict_equal` sobre `Dynamic`; o `undefined` de
//!   `property.policy` e de `tags?.includes(...)` é `Dynamic::Undefined`.
//! - `new class extends mock<IConfigurationChangeEvent>() { affectsConfiguration(c) { return c === key; } }` é o evento real
//!   de `configuration_event_support`, que afeta só a chave dada (o mesmo que a igualdade de chave). O `owner` das funções
//!   `observable...` não existe no porte (só dá nome ao observável), e `store` é um `&DisposableStore`.
//! - `autorun(reader => values.push(x.read(reader)))` é o `autorun` com a lista compartilhada por `Rc<RefCell<..>>`;
//!   `await configurationService.setUserConfiguration(...)` é o `await` da promessa.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod configuration_event_support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use configuration_event_support::fire_configuration_change;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableStore;
use je_base::observable::autorun;
use je_platform::configuration::registry::{ConfigurationScope, PropertyDictionary, PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::storage::{InMemoryStorageService, StorageService};
use je_workbench::contrib::multi_diff_editor::common::multi_diff_editor::{
    DEFAULT_MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT, MULTI_DIFF_EDITOR_EXPERIMENTAL_FILE_LIST_SETTING, MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT_SETTING,
    MultiDiffEditorExperimentalVariant, MultiDiffEditorFileListPosition, MultiDiffEditorFileListViewMode, MultiDiffEditorVariant,
    get_workbench_multi_diff_editor_variant, observable_multi_diff_editor_file_list_position, observable_multi_diff_editor_file_list_view_mode,
    observable_multi_diff_editor_file_list_visible, observable_workbench_multi_diff_editor_variant, register_hide_original_line_numbers_setting,
    register_multi_diff_editor_configuration, set_multi_diff_editor_file_list_position, set_multi_diff_editor_file_list_view_mode,
    toggle_multi_diff_editor_file_list,
};

/// O `configurationProperties` que o arquivo original captura no topo, depois de registrar as configurações.
fn registered_properties() -> PropertyDictionary {
    register_multi_diff_editor_configuration();
    register_hide_original_line_numbers_setting();
    configuration_registry().get_configuration_properties()
}

fn property_of(key: &str) -> PropertySchema {
    registered_properties().get(key).unwrap_or_else(|| panic!("a propriedade {key} não está registrada"))
}

/// `property.tags?.includes('experimental')`.
fn is_experimental(property: &PropertySchema) -> Dynamic {
    match property.get("tags") {
        Dynamic::Array(tags) => Dynamic::Bool((0..tags.len()).any(|index| matches!(tags.get(index), Dynamic::String(tag) if tag == "experimental"))),
        _ => Dynamic::Undefined,
    }
}

fn strings(values: &[&str]) -> Dynamic {
    Dynamic::array(values.iter().map(|value| Dynamic::string(value)))
}

fn new_store(guard: &common::LeakGuard) -> Rc<DisposableStore> {
    guard.add_rc(DisposableStore::new())
}

#[test]
fn registers_the_experimental_variant_setting() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let property = property_of(MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT_SETTING);

    assert_deep_strict_equal(
        &Dynamic::object([
            ("type", property.get("type")),
            ("enum", property.get("enum")),
            ("enumDescriptions", property.get("enumDescriptions")),
            ("default", property.get("default")),
            ("scope", property.get("scope")),
            ("isExperimental", is_experimental(&property)),
            ("experiment", property.get("experiment")),
        ]),
        &Dynamic::object([
            ("type", Dynamic::string("string")),
            ("enum", strings(&["cards", "noCards"])),
            ("enumDescriptions", strings(&["Use the compact card-based variant.", "Use the compact variant without cards."])),
            ("default", Dynamic::string("noCards")),
            ("scope", ConfigurationScope::Window.to_dynamic()),
            ("isExperimental", Dynamic::Bool(true)),
            ("experiment", Dynamic::object([("mode", Dynamic::string("auto"))])),
        ]),
    );
    assert_eq!(DEFAULT_MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT.as_str(), "noCards");
}

#[test]
fn registers_the_experimental_file_list_setting_as_opt_in() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let property = property_of(MULTI_DIFF_EDITOR_EXPERIMENTAL_FILE_LIST_SETTING);

    assert_deep_strict_equal(
        &Dynamic::object([
            ("type", property.get("type")),
            ("default", property.get("default")),
            ("scope", property.get("scope")),
            ("isExperimental", is_experimental(&property)),
        ]),
        &Dynamic::object([
            ("type", Dynamic::string("boolean")),
            ("default", Dynamic::Bool(false)),
            ("scope", ConfigurationScope::Window.to_dynamic()),
            ("isExperimental", Dynamic::Bool(true)),
        ]),
    );
}

#[test]
fn registers_original_line_number_visibility_as_an_inheritable_setting() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let property = property_of("diffEditor.hideOriginalLineNumbers");

    assert_deep_strict_equal(
        &Dynamic::object([
            ("type", property.get("type")),
            ("default", property.get("default")),
            ("scope", property.scope().map_or(Dynamic::Undefined, ConfigurationScope::to_dynamic)),
            ("policy", property.get("policy")),
        ]),
        &Dynamic::object([
            ("type", strings(&["boolean", "null"])),
            ("default", Dynamic::Null),
            ("scope", ConfigurationScope::LanguageOverridable.to_dynamic()),
            ("policy", Dynamic::Undefined),
        ]),
    );
}

#[test]
fn maps_setting_values_to_compact_variants() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        (
            get_workbench_multi_diff_editor_variant(MultiDiffEditorExperimentalVariant::Cards),
            get_workbench_multi_diff_editor_variant(MultiDiffEditorExperimentalVariant::NoCards),
        ),
        (MultiDiffEditorVariant::Cards, MultiDiffEditorVariant::NoCards)
    );
}

#[test]
fn updates_the_resolved_variant_when_configuration_changes() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration_service = TestConfigurationService::new(Some(Dynamic::object([(MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT_SETTING, Dynamic::string("cards"))])));
        let service: Rc<dyn IConfigurationService> = configuration_service.clone();
        let variant = observable_workbench_multi_diff_editor_variant(service);
        let values: Rc<RefCell<Vec<MultiDiffEditorVariant>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = values.clone();
        guard.add(autorun(move |reader| sink.borrow_mut().push(variant.read(reader))));

        configuration_service.set_user_configuration(MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT_SETTING, Dynamic::string("noCards"), None).await.unwrap();
        fire_configuration_change(&configuration_service, &[MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT_SETTING]);

        assert_eq!(*values.borrow(), vec![MultiDiffEditorVariant::Cards, MultiDiffEditorVariant::NoCards]);
    });
}

#[test]
fn shows_the_file_list_while_the_setting_is_on_and_the_shared_toggle_has_not_hidden_it() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration_service = TestConfigurationService::new(Some(Dynamic::object([(MULTI_DIFF_EDITOR_EXPERIMENTAL_FILE_LIST_SETTING, Dynamic::Bool(true))])));
        let service: Rc<dyn IConfigurationService> = configuration_service.clone();
        let storage_service: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
        let store = new_store(&guard);
        let first_editor = observable_multi_diff_editor_file_list_visible(service.clone(), storage_service.clone(), &store);
        let second_editor = observable_multi_diff_editor_file_list_visible(service, storage_service.clone(), &store);
        let first_values: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let second_values: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let (first_sink, second_sink) = (first_values.clone(), second_values.clone());
        guard.add(autorun(move |reader| first_sink.borrow_mut().push(first_editor.read(reader))));
        guard.add(autorun(move |reader| second_sink.borrow_mut().push(second_editor.read(reader))));

        toggle_multi_diff_editor_file_list(storage_service.as_ref());
        toggle_multi_diff_editor_file_list(storage_service.as_ref());
        configuration_service.set_user_configuration(MULTI_DIFF_EDITOR_EXPERIMENTAL_FILE_LIST_SETTING, Dynamic::Bool(false), None).await.unwrap();
        fire_configuration_change(&configuration_service, &[MULTI_DIFF_EDITOR_EXPERIMENTAL_FILE_LIST_SETTING]);
        toggle_multi_diff_editor_file_list(storage_service.as_ref());
        toggle_multi_diff_editor_file_list(storage_service.as_ref());

        assert_eq!(*first_values.borrow(), vec![true, false, true, false]);
        assert_eq!(*second_values.borrow(), vec![true, false, true, false]);
    });
}

#[test]
fn shares_the_file_list_view_mode_between_editors_and_defaults_to_a_list() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let storage_service: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
    let store = new_store(&guard);
    let first_editor = observable_multi_diff_editor_file_list_view_mode(storage_service.clone(), &store);
    let second_editor = observable_multi_diff_editor_file_list_view_mode(storage_service.clone(), &store);
    let first_values: Rc<RefCell<Vec<MultiDiffEditorFileListViewMode>>> = Rc::new(RefCell::new(Vec::new()));
    let second_values: Rc<RefCell<Vec<MultiDiffEditorFileListViewMode>>> = Rc::new(RefCell::new(Vec::new()));
    let (first_sink, second_sink) = (first_values.clone(), second_values.clone());
    guard.add(autorun(move |reader| first_sink.borrow_mut().push(first_editor.read(reader))));
    guard.add(autorun(move |reader| second_sink.borrow_mut().push(second_editor.read(reader))));

    set_multi_diff_editor_file_list_view_mode(storage_service.as_ref(), MultiDiffEditorFileListViewMode::Tree);
    set_multi_diff_editor_file_list_view_mode(storage_service.as_ref(), MultiDiffEditorFileListViewMode::Tree);
    set_multi_diff_editor_file_list_view_mode(storage_service.as_ref(), MultiDiffEditorFileListViewMode::List);

    let expected = vec![MultiDiffEditorFileListViewMode::List, MultiDiffEditorFileListViewMode::Tree, MultiDiffEditorFileListViewMode::List];
    assert_eq!(*first_values.borrow(), expected);
    assert_eq!(*second_values.borrow(), expected);
}

#[test]
fn shares_and_remembers_the_file_list_position_defaulting_to_the_right() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let storage_service: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
    let store = new_store(&guard);
    let first_editor = observable_multi_diff_editor_file_list_position(storage_service.clone(), &store);
    let second_editor = observable_multi_diff_editor_file_list_position(storage_service.clone(), &store);
    let first_values: Rc<RefCell<Vec<MultiDiffEditorFileListPosition>>> = Rc::new(RefCell::new(Vec::new()));
    let second_values: Rc<RefCell<Vec<MultiDiffEditorFileListPosition>>> = Rc::new(RefCell::new(Vec::new()));
    let (first_sink, second_sink) = (first_values.clone(), second_values.clone());
    guard.add(autorun(move |reader| first_sink.borrow_mut().push(first_editor.read(reader))));
    guard.add(autorun(move |reader| second_sink.borrow_mut().push(second_editor.read(reader))));

    set_multi_diff_editor_file_list_position(storage_service.as_ref(), MultiDiffEditorFileListPosition::Left);
    set_multi_diff_editor_file_list_position(storage_service.as_ref(), MultiDiffEditorFileListPosition::Left);
    let restored = observable_multi_diff_editor_file_list_position(storage_service.clone(), &store).get();
    set_multi_diff_editor_file_list_position(storage_service.as_ref(), MultiDiffEditorFileListPosition::Right);

    let expected = vec![MultiDiffEditorFileListPosition::Right, MultiDiffEditorFileListPosition::Left, MultiDiffEditorFileListPosition::Right];
    assert_eq!(*first_values.borrow(), expected);
    assert_eq!(*second_values.borrow(), expected);
    assert_eq!(restored, MultiDiffEditorFileListPosition::Left);
}
