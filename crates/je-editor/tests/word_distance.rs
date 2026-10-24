//! Porte de `src/vs/editor/contrib/suggest/test/browser/wordDistance.test.ts` (suíte `suggest, word distance`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `setup`/`teardown` e `ensureNoDisposablesAreLeakedInTestSuite()` são o `LeakGuard` e o preparo do corpo do caso:
//!   a linguagem `bracketMode` com os colchetes `{}`, `[]` e `()`, o modelo `function abc(aa, ab){\na\n}` com a URI
//!   `test:///some.path`, o editor de teste com `suggest.localityBonus` ligado e o cursor em (2, 2). O `disposables`
//!   do original é o `store` do `LeakGuard`.
//! - O modelo usa os serviços de linguagem de `create_language_model_services` (o `ILanguageService` e o
//!   `ILanguageConfigurationService` que o `TextModel` lê), e o editor de teste usa os serviços do
//!   `create_code_editor_services`: no original os dois são a mesma instância, no porte são separadas (o editor lê do
//!   serviço de configuração de linguagem dele só o que o editor pede, nunca os colchetes do modelo).
//! - A subclasse anônima de `EditorWorkerService` é [`TestEditorWorkerService`]: o `canComputeWordRanges` real é
//!   `canSyncModel(modelService, resource)`, com o `modelService` falso do caso que só conhece o modelo do caso; o
//!   `computeWordRanges` é o `override`, que chama o `$computeWordRanges` do [`EditorWorker`] com o texto e as flags do
//!   `DEFAULT_WORD_REGEXP` (`g`). O `model.onDidChangeContent(e => worker.$acceptModelChanged(...))` é o ouvinte
//!   registrado no `store`.
//! - `createSuggestItem(label, overwriteBefore, position)` do arquivo é `create_suggest_item_with` de `tests/support`
//!   com `kind: 0` (`Method`) e a posição dada; o caso só lê o `completion` do item.
//! - O caso é `async` (o `WordDistance.create` e o provedor de colchetes cedem a vez): o corpo `async` de
//!   [`run_with_faked_timers`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::to_disposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::editor_web_worker::{EditorWorker, RawModelData};
use je_editor::editor_worker::{IEditorWorkerService, WordRanges};
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService as _;
use je_editor::language_feature_registry::should_synchronize_model;
use je_editor::language_service::ILanguageService as _;
use je_editor::languages::{CompletionItemKind, CompletionLabel};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::suggest::suggest::CompletionItem;
use je_editor::suggest::word_distance::WordDistance;
use je_editor::text_model::{TextModel, TextModelServices};
use je_editor::text_model_events::ModelContentChangedEvent;
use je_editor::word_helper::default_word_regexp;
use support::suggest_test_utils::{SuggestItemOptions, create_suggest_item_with};
use support::test_code_editor::{TestCodeEditorInstantiationOptions, create_code_editor_services, instantiate_test_code_editor};
use support::test_text_model::{create_language_model_services, resolve_options};

/// `languageId`.
const LANGUAGE_ID: &str = "bracketMode";

/// A subclasse anônima de `EditorWorkerService` do `setup` (ver as notas no topo do arquivo).
struct TestEditorWorkerService {
    model: Rc<TextModel>,
    worker: Rc<EditorWorker>,
}

impl IEditorWorkerService for TestEditorWorkerService {
    fn can_compute_word_ranges(&self, resource: &URI) -> bool {
        resource.to_string(false) == self.model.uri().to_string(false) && should_synchronize_model(&self.model)
    }

    fn compute_word_ranges(&self, resource: &URI, range: Range) -> Promise<Option<WordRanges>> {
        let word_def = default_word_regexp().source().to_vec();
        self.worker
            .compute_word_ranges(&resource.to_string(false).to_string_lossy(), range, &word_def, "g")
            .then(Some)
    }
}

/// `createSuggestItem(label, overwriteBefore, position)`.
fn create_suggest_item(label: &str, overwrite_before: i32, position: Position) -> CompletionItem {
    create_suggest_item_with(
        CompletionLabel::Text(label.to_string()),
        overwrite_before,
        SuggestItemOptions { kind: CompletionItemKind::Method, position, ..SuggestItemOptions::default() },
    )
}

// suite('suggest, word distance')

#[test]
fn suggest_locality_bonus_can_boost_current_word_90515() {
    run_with_faked_timers(async move {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        // setup
        let services = create_language_model_services(&ds);
        ds.add(services.language_service.register_language(LanguageExtensionPoint::new(LANGUAGE_ID)));
        ds.add(services.language_configuration_service.register(
            LANGUAGE_ID,
            LanguageConfiguration {
                brackets: Some(vec![
                    ("{".to_owned(), "}".to_owned()),
                    ("[".to_owned(), "]".to_owned()),
                    ("(".to_owned(), ")".to_owned()),
                ]),
                ..LanguageConfiguration::default()
            },
            None,
        ));

        let model = TextModel::new_with_language_service(
            "function abc(aa, ab){\na\n}".into(),
            LANGUAGE_ID,
            resolve_options(&RelaxedTextModelCreationOptions::default()),
            Some(URI::parse("test:///some.path", false)),
            services.undo_redo_service.clone(),
            Some(TextModelServices {
                language_service: services.language_service.clone(),
                language_configuration_service: services.language_configuration_service.clone(),
            }),
        );
        let owned_model = model.clone();
        ds.add(to_disposable(move || owned_model.dispose()));

        let editor_services = create_code_editor_services(&ds.store(), None);
        let editor = instantiate_test_code_editor(&editor_services, Some(model.clone()), &TestCodeEditorInstantiationOptions::default().options);
        ds.add_rc(editor.clone());
        editor.update_options(Some(&Dynamic::object([("suggest", Dynamic::object([("localityBonus", Dynamic::Bool(true))]))])));
        editor.set_position(Position::new(2, 2), None);

        let worker = Rc::new(EditorWorker::new());
        let url = model.uri().to_string(false).to_string_lossy();
        worker.accept_new_model(RawModelData {
            url: url.clone(),
            lines: model.get_lines_content(),
            eol: model.get_eol(),
            version_id: model.get_version_id(),
        });
        let listened_worker = worker.clone();
        ds.add(model.on_did_change_content().listen(move |e: &ModelContentChangedEvent| listened_worker.accept_model_changed(&url, e)));
        let service: Rc<dyn IEditorWorkerService> = Rc::new(TestEditorWorkerService { model: model.clone(), worker });

        let distance = <dyn WordDistance>::create(&service, editor.widget()).await.expect("WordDistance.create");

        // test('Suggest locality bonus can boost current word #90515')
        let pos = Position::new(2, 2);
        let d1 = distance.distance(pos, &create_suggest_item("a", 1, pos).completion.borrow());
        let d2 = distance.distance(pos, &create_suggest_item("aa", 1, pos).completion.borrow());
        let d3 = distance.distance(pos, &create_suggest_item("ab", 1, pos).completion.borrow());

        assert!(d1 > d2);
        assert!(d2 == d3);
    });
}
