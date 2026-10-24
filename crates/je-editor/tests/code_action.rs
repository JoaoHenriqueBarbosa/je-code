//! Porte de `src/vs/editor/contrib/codeAction/test/browser/codeAction.test.ts` (suíte `CodeAction`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `setup`/`teardown` são [`fixture`] (registro novo e modelo `fooLang` com a URI `untitled:path`) e o descarte do
//!   modelo ao fim de cada caso; `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`.
//! - Os casos `async` são o corpo `async` de [`run_with_faked_timers`], que o executa no event loop do porte.
//! - `staticCodeActionProvider(...actions)` é [`StaticCodeActionProvider`]; os objetos de ação são compartilhados
//!   por referência ([`CodeActionRef`]), como no original, e `Progress.None` é [`ProgressNone`].
//! - `assert.deepStrictEqual(actions, expected)` do primeiro caso compara cada item pela ação (a mesma instância) e
//!   pelo provedor (a mesma instância): [`assert_same_items`].
//! - Os literais `testData.tsLint.*` (objetos com `$ident`, `arguments`, `id` e `title`, usados como ações) são
//!   ações só com o título, que é o que a ordenação e a comparação leem. `testData.spelling.bcd.edit` (um
//!   `WorkspaceEdit` sem `edits`) é um `WorkspaceEdit` vazio, e `testData.command.abc.command` um `Command`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::hierarchical_kind::HierarchicalKind;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::code_action::get_code_actions;
use je_editor::code_action::languages::{
    CodeAction, CodeActionContext, CodeActionList, CodeActionListObject, CodeActionProvider, CodeActionRef,
    CodeActionTriggerType, Command, WorkspaceEdit,
};
use je_editor::code_action::platform::{ActionSet, ProgressNone};
use je_editor::code_action::types::{
    CodeActionFilter, CodeActionItem, CodeActionKind, CodeActionTrigger, CodeActionTriggerSource,
};
use je_editor::code_editor_widget::SelectionOrRange;
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::ProviderResult;
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_platform::markers::{MarkerData, MarkerSeverity};
use support::test_text_model::create_text_model_with;

const LANG_ID: &str = "fooLang";

type Registry = Rc<LanguageFeatureRegistry<dyn CodeActionProvider>>;

/// O corpo `async` do caso; uma rejeição reprova o caso.
fn run(body: impl std::future::Future<Output = Result<(), je_base::errors::Error>> + 'static) {
    run_with_faked_timers(async move { body.await.expect("o caso terminou com erro") });
}

fn action_ref(action: CodeAction) -> CodeActionRef {
    Rc::new(RefCell::new(action))
}

/// `staticCodeActionProvider(...actions)`.
struct StaticCodeActionProvider {
    actions: Vec<CodeActionRef>,
}

impl CodeActionProvider for StaticCodeActionProvider {
    fn provide_code_actions(
        &self,
        _model: &Rc<TextModel>,
        _range: &SelectionOrRange,
        _context: &CodeActionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<dyn CodeActionList>> {
        let list: Rc<dyn CodeActionList> = CodeActionListObject::new(self.actions.clone(), || {});
        Promise::resolved(Some(list))
    }
}

fn static_code_action_provider(actions: Vec<CodeActionRef>) -> Rc<dyn CodeActionProvider> {
    Rc::new(StaticCodeActionProvider { actions })
}

/// `setup()`: o registro novo e o modelo.
fn fixture() -> (Registry, Rc<TextModel>) {
    let registry: Registry = Rc::new(LanguageFeatureRegistry::new(None));
    let model = create_text_model_with(
        "test1\ntest2\ntest3",
        Some(LANG_ID),
        RelaxedTextModelCreationOptions::default(),
        Some(URI::parse("untitled:path", false)),
    );
    (registry, model)
}

fn range() -> SelectionOrRange {
    Range::new(1, 1, 2, 1).into()
}

fn trigger(
    r#type: CodeActionTriggerType,
    trigger_action: CodeActionTriggerSource,
    filter: Option<CodeActionFilter>,
) -> CodeActionTrigger {
    CodeActionTrigger { filter, ..CodeActionTrigger::new(r#type, trigger_action) }
}

fn include(kind: HierarchicalKind) -> Option<CodeActionFilter> {
    Some(CodeActionFilter { include: Some(kind), ..CodeActionFilter::default() })
}

fn titles(actions: &[Rc<CodeActionItem>]) -> Vec<String> {
    actions.iter().map(|item| item.action.borrow().title.clone()).collect()
}

fn assert_same_items(actual: &[Rc<CodeActionItem>], expected: &[Rc<CodeActionItem>]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(Rc::ptr_eq(&actual.action, &expected.action), "ação diferente: {}", actual.action.borrow().title);
        let same_provider = match (&actual.provider, &expected.provider) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        assert!(same_provider);
        assert_eq!(actual.highlight_range.get(), expected.highlight_range.get());
    }
}

// suite('CodeAction')

#[test]
fn code_actions_are_sorted_by_type_38623() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    run(async {
        let (registry, model) = fixture();

        let diagnostic = |severity_message: &str| MarkerData {
            ..MarkerData::new(MarkerSeverity::ERROR, severity_message, 1, 1, 2, 1)
        };
        let diagnostics_abc = {
            let mut action = CodeAction::new("bTitle");
            action.diagnostics = Some(vec![diagnostic("abc")]);
            action_ref(action)
        };
        let diagnostics_bcd = {
            let mut action = CodeAction::new("aTitle");
            action.diagnostics = Some(vec![diagnostic("bcd")]);
            action_ref(action)
        };
        let command_abc = {
            let mut action = CodeAction::new("Extract to inner function in function \"test\"");
            action.command = Some(Command::new("1", "abc"));
            action_ref(action)
        };
        let spelling_bcd = {
            let mut action = CodeAction::new("abc");
            action.diagnostics = Some(Vec::new());
            action.edit = Some(WorkspaceEdit::default());
            action_ref(action)
        };
        let ts_lint_abc = action_ref(CodeAction::new("abc"));
        let ts_lint_bcd = action_ref(CodeAction::new("bcd"));

        let provider = static_code_action_provider(vec![
            command_abc.clone(),
            diagnostics_bcd.clone(),
            spelling_bcd.clone(),
            ts_lint_bcd.clone(),
            ts_lint_abc.clone(),
            diagnostics_abc.clone(),
        ]);

        let registration = registry.register(LANG_ID.into(), provider.clone());

        let expected = vec![
            // CodeActions with a diagnostics array are shown first without further sorting
            CodeActionItem::new(diagnostics_bcd, Some(provider.clone()), None),
            CodeActionItem::new(diagnostics_abc, Some(provider.clone()), None),
            // CodeActions without diagnostics are shown in the given order without any further sorting
            CodeActionItem::new(command_abc, Some(provider.clone()), None),
            CodeActionItem::new(spelling_bcd, Some(provider.clone()), None),
            CodeActionItem::new(ts_lint_bcd, Some(provider.clone()), None),
            CodeActionItem::new(ts_lint_abc, Some(provider.clone()), None),
        ];

        let set = get_code_actions(
            &registry,
            &model,
            range(),
            &trigger(CodeActionTriggerType::Invoke, CodeActionTriggerSource::Default, None),
            Rc::new(ProgressNone),
            &CancellationToken::None,
        )
        .await?;
        let actions = set.valid_actions();
        assert_eq!(actions.len(), 6);
        assert_same_items(&actions, &expected);

        set.dispose();
        registration.dispose();
        model.dispose();
        Ok(())
    });
}

#[test]
fn get_code_actions_should_filter_by_scope() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    run(async {
        let (registry, model) = fixture();
        let provider = static_code_action_provider(vec![
            action_ref(CodeAction::with_kind("a", Some("a"))),
            action_ref(CodeAction::with_kind("b", Some("b"))),
            action_ref(CodeAction::with_kind("a.b", Some("a.b"))),
        ]);

        let registration = registry.register(LANG_ID.into(), provider);

        for (kind, expected) in [("a", vec!["a", "a.b"]), ("a.b", vec!["a.b"]), ("a.b.c", vec![])] {
            let set = get_code_actions(
                &registry,
                &model,
                range(),
                &trigger(CodeActionTriggerType::Auto, CodeActionTriggerSource::Default, include(HierarchicalKind::new(kind))),
                Rc::new(ProgressNone),
                &CancellationToken::None,
            )
            .await?;
            assert_eq!(titles(&set.valid_actions()), expected);
            set.dispose();
        }

        registration.dispose();
        model.dispose();
        Ok(())
    });
}

/// O provedor do caso `should forward requested scope to providers`.
struct ForwardScopeProvider;

impl CodeActionProvider for ForwardScopeProvider {
    fn provide_code_actions(
        &self,
        _model: &Rc<TextModel>,
        _range: &SelectionOrRange,
        context: &CodeActionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<dyn CodeActionList>> {
        let only = context.only.clone();
        let list: Rc<dyn CodeActionList> = CodeActionListObject::new(
            vec![action_ref(CodeAction::with_kind(only.clone().unwrap_or_default(), only.as_deref()))],
            || {},
        );
        Promise::resolved(Some(list))
    }
}

#[test]
fn get_code_actions_should_forward_requested_scope_to_providers() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    run(async {
        let (registry, model) = fixture();
        let registration = registry.register(LANG_ID.into(), Rc::new(ForwardScopeProvider));

        let set = get_code_actions(
            &registry,
            &model,
            range(),
            &trigger(CodeActionTriggerType::Auto, CodeActionTriggerSource::Default, include(HierarchicalKind::new("a"))),
            Rc::new(ProgressNone),
            &CancellationToken::None,
        )
        .await?;
        let actions = set.valid_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action.borrow().title, "a");

        set.dispose();
        registration.dispose();
        model.dispose();
        Ok(())
    });
}

#[test]
fn get_code_actions_should_not_return_source_code_action_by_default() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    run(async {
        let (registry, model) = fixture();
        let provider = static_code_action_provider(vec![
            action_ref(CodeAction::with_kind("a", Some(&CodeActionKind::source().value))),
            action_ref(CodeAction::with_kind("b", Some("b"))),
        ]);

        let registration = registry.register(LANG_ID.into(), provider);

        {
            let set = get_code_actions(
                &registry,
                &model,
                range(),
                &trigger(CodeActionTriggerType::Auto, CodeActionTriggerSource::SourceAction, None),
                Rc::new(ProgressNone),
                &CancellationToken::None,
            )
            .await?;
            let actions = set.valid_actions();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].action.borrow().title, "b");
            set.dispose();
        }

        {
            let filter = Some(CodeActionFilter {
                include: Some(CodeActionKind::source()),
                include_source_actions: Some(true),
                ..CodeActionFilter::default()
            });
            let set = get_code_actions(
                &registry,
                &model,
                range(),
                &trigger(CodeActionTriggerType::Auto, CodeActionTriggerSource::Default, filter),
                Rc::new(ProgressNone),
                &CancellationToken::None,
            )
            .await?;
            let actions = set.valid_actions();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].action.borrow().title, "a");
            set.dispose();
        }

        registration.dispose();
        model.dispose();
        Ok(())
    });
}

#[test]
fn get_code_actions_should_support_filtering_out_some_requested_source_code_actions_84602() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    run(async {
        let (registry, model) = fixture();
        let provider = static_code_action_provider(vec![
            action_ref(CodeAction::with_kind("a", Some(&CodeActionKind::source().value))),
            action_ref(CodeAction::with_kind("b", Some(&CodeActionKind::source().append(&["test"]).value))),
            action_ref(CodeAction::with_kind("c", Some("c"))),
        ]);

        let registration = registry.register(LANG_ID.into(), provider);

        {
            let filter = Some(CodeActionFilter {
                include: Some(CodeActionKind::source().append(&["test"])),
                excludes: Some(vec![CodeActionKind::source()]),
                include_source_actions: Some(true),
                ..CodeActionFilter::default()
            });
            let set = get_code_actions(
                &registry,
                &model,
                range(),
                &trigger(CodeActionTriggerType::Auto, CodeActionTriggerSource::SourceAction, filter),
                Rc::new(ProgressNone),
                &CancellationToken::None,
            )
            .await?;
            let actions = set.valid_actions();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].action.borrow().title, "b");
            set.dispose();
        }

        registration.dispose();
        model.dispose();
        Ok(())
    });
}

/// O segundo provedor do caso `no invoke a provider that has been excluded`.
struct SubTypeProvider {
    sub_type: String,
    did_invoke: Rc<Cell<bool>>,
}

impl CodeActionProvider for SubTypeProvider {
    fn provided_code_action_kinds(&self) -> Option<Vec<String>> {
        Some(vec![self.sub_type.clone()])
    }

    fn provide_code_actions(
        &self,
        _model: &Rc<TextModel>,
        _range: &SelectionOrRange,
        _context: &CodeActionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<dyn CodeActionList>> {
        self.did_invoke.set(true);
        let list: Rc<dyn CodeActionList> =
            CodeActionListObject::new(vec![action_ref(CodeAction::with_kind("x", Some(&self.sub_type)))], || {});
        Promise::resolved(Some(list))
    }
}

#[test]
fn get_code_actions_no_invoke_a_provider_that_has_been_excluded_84602() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    run(async {
        let (registry, model) = fixture();
        let base_type = CodeActionKind::refactor();
        let sub_type = CodeActionKind::refactor().append(&["sub"]);

        let first = registry.register(
            LANG_ID.into(),
            static_code_action_provider(vec![action_ref(CodeAction::with_kind("a", Some(&base_type.value)))]),
        );

        let did_invoke = Rc::new(Cell::new(false));
        let second = registry.register(
            LANG_ID.into(),
            Rc::new(SubTypeProvider { sub_type: sub_type.value.clone(), did_invoke: did_invoke.clone() }),
        );

        {
            let filter = Some(CodeActionFilter {
                include: Some(base_type),
                excludes: Some(vec![sub_type]),
                ..CodeActionFilter::default()
            });
            let set = get_code_actions(
                &registry,
                &model,
                range(),
                &trigger(CodeActionTriggerType::Auto, CodeActionTriggerSource::Refactor, filter),
                Rc::new(ProgressNone),
                &CancellationToken::None,
            )
            .await?;
            let actions = set.valid_actions();
            assert!(!did_invoke.get());
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].action.borrow().title, "a");
            set.dispose();
        }

        first.dispose();
        second.dispose();
        model.dispose();
        Ok(())
    });
}

/// O provedor do caso `should not invoke code action providers filtered out by providedCodeActionKinds`.
struct RefactorOnlyProvider {
    was_invoked: Rc<Cell<bool>>,
}

impl CodeActionProvider for RefactorOnlyProvider {
    fn provided_code_action_kinds(&self) -> Option<Vec<String>> {
        Some(vec![CodeActionKind::refactor().value])
    }

    fn provide_code_actions(
        &self,
        _model: &Rc<TextModel>,
        _range: &SelectionOrRange,
        _context: &CodeActionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<dyn CodeActionList>> {
        self.was_invoked.set(true);
        let list: Rc<dyn CodeActionList> = CodeActionListObject::new(Vec::new(), || {});
        Promise::resolved(Some(list))
    }
}

#[test]
fn get_code_actions_should_not_invoke_code_action_providers_filtered_out_by_provided_code_action_kinds() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    run(async {
        let (registry, model) = fixture();
        let was_invoked = Rc::new(Cell::new(false));
        let registration = registry.register(LANG_ID.into(), Rc::new(RefactorOnlyProvider { was_invoked: was_invoked.clone() }));

        let set = get_code_actions(
            &registry,
            &model,
            range(),
            &trigger(CodeActionTriggerType::Auto, CodeActionTriggerSource::Refactor, include(CodeActionKind::quick_fix())),
            Rc::new(ProgressNone),
            &CancellationToken::None,
        )
        .await?;
        assert_eq!(set.valid_actions().len(), 0);
        assert!(!was_invoked.get());

        set.dispose();
        registration.dispose();
        model.dispose();
        Ok(())
    });
}
