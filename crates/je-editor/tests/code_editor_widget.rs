//! Porte de `src/vs/editor/test/browser/widget/codeEditorWidget.test.ts` (suíte `CodeEditorWidget`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - O `ensureNoDisposablesAreLeakedInTestSuite()` da suíte é o `LeakGuard` de cada caso.
//! - `withTestCodeEditor(text, options, (editor, viewModel, instantiationService) => ...)` é o `with_test_code_editor`
//!   de `support/test_code_editor.rs`; `instantiationService.get(ILanguageService)` é o `get` do `CodeEditorServices`.
//! - `new class extends TestNotificationService { override prompt(...) }` é o [`RecordingNotificationService`]: guarda a
//!   severidade, a mensagem e os rótulos das escolhas de cada `prompt` e repassa ao `TestNotificationService`
//!   (o `super.prompt`). `message instanceof NotificationText` é o `NotificationMessage::Rich`, e `message.nodes.filter(node
//!   => typeof node !== 'string')` são os `LinkedTextNode::Link`.
//! - `{ multiCursorLimit: 1, serviceCollection }` são as opções do editor mais a coleção de serviços do
//!   `TestCodeEditorInstantiationOptions`.
//! - `viewModel.type(text, source)` é `view_model.r#type(&units, source)` (o texto vira UTF-16).
//!
//! Divergências:
//! - O caso `issue #146841: widgets do not keep the DOM of a detached view alive` não tem porte: ele exige uma `View`
//!   real sobre um contêiner DOM (`document.createElement`, `isConnected`, `parentElement`) e os widgets de conteúdo, de
//!   overlay e de margem de glifos (`addContentWidget`, `addOverlayWidget`, `addGlyphMarginWidget`), que o
//!   [`CodeEditorWidget`] do porte não tem (ver "Divergências" no topo de `code_editor_widget.rs`). Ele passa a contar
//!   quando a `View` for portada.

#![allow(dead_code)]

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::linked_text::{Link, LinkedTextNode};
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE};
use je_editor::model::{ModelDecorationOptionsInit, ModelDeltaDecoration};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::ModelDecorationOptions;
use je_editor::text_model_events::ModelContentChangedEvent;
use je_instantiation::ServiceCollection;
use je_platform::notification::notification_message::NotificationText;
use je_platform::notification::{
    INotificationService, I_NOTIFICATION_SERVICE, NoOpNotification, Notification, NotificationFilterSetting,
    NotificationHandle, NotificationMessage, NotificationSource, NotificationSourceFilter, NotificationsFilter,
    PromptChoiceKind, PromptOptions, Severity, StatusHandle, StatusMessageOptions,
};
use support::test_code_editor::{TestCodeEditorInstantiationOptions, with_test_code_editor};
use support::test_notification_service::TestNotificationService;

/// O que um `prompt` registrou: `[severity, message, choices]`, com as escolhas reduzidas aos rótulos.
struct RecordedPrompt {
    severity: Severity,
    message: NotificationMessage,
    actions: Vec<String>,
}

/// `new class extends TestNotificationService { override prompt(...args) { prompts.push(args); return super.prompt(...args); } }`.
#[derive(Default)]
struct RecordingNotificationService {
    inner: TestNotificationService,
    prompts: RefCell<Vec<RecordedPrompt>>,
}

impl INotificationService for RecordingNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        self.inner.on_did_change_filter()
    }

    fn set_filter(&self, filter: NotificationFilterSetting) {
        self.inner.set_filter(filter);
    }

    fn get_filter(&self, source: Option<&NotificationSource>) -> NotificationsFilter {
        self.inner.get_filter(source)
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        self.inner.get_filters()
    }

    fn remove_filter(&self, source_id: &str) {
        self.inner.remove_filter(source_id);
    }

    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle> {
        self.inner.notify(notification)
    }

    fn info(&self, message: Vec<NotificationMessage>) {
        self.inner.info(message);
    }

    fn warn(&self, message: Vec<NotificationMessage>) {
        self.inner.warn(message);
    }

    fn error(&self, message: Vec<NotificationMessage>) {
        self.inner.error(message);
    }

    fn prompt(
        &self,
        severity: Severity,
        message: NotificationMessage,
        choices: Vec<PromptChoiceKind>,
        options: Option<PromptOptions>,
    ) -> Rc<dyn NotificationHandle> {
        let actions = choices
            .iter()
            .map(|choice| match choice {
                PromptChoiceKind::Plain(choice) => choice.base.label.clone(),
                PromptChoiceKind::WithMenu(choice) => choice.base.label.clone(),
            })
            .collect();
        self.prompts.borrow_mut().push(RecordedPrompt { severity, message: message.clone(), actions });
        self.inner.prompt(severity, message, choices, options)
    }

    fn status(&self, message: NotificationMessage, options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        self.inner.status(message, options)
    }
}

/// `e.changes.reduce((aggr, c) => [...aggr, c.text, c.rangeOffset, c.rangeLength], []).join(', ')`.
fn changes_to_string(e: &ModelContentChangedEvent) -> String {
    let mut parts: Vec<String> = Vec::new();
    for change in &e.changes {
        parts.push(change.text.to_string());
        parts.push(change.range_offset.to_string());
        parts.push(change.range_length.to_string());
    }
    parts.join(", ")
}

/// O texto de `viewModel.type(text, ...)` em UTF-16.
fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `[{ range: new Range(1, 1, 1, 1), options: { description: 'test' } }]`.
fn test_decoration() -> Vec<ModelDeltaDecoration> {
    vec![ModelDeltaDecoration {
        range: Range::new(1, 1, 1, 1),
        options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "test".to_owned(),
            ..Default::default()
        }),
    }]
}

fn default_options() -> TestCodeEditorInstantiationOptions {
    TestCodeEditorInstantiationOptions::default()
}

#[test]
fn preserves_the_documentation_link_when_the_cursor_limit_is_reached() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let notification_service = Rc::new(RecordingNotificationService::default());
    let service_collection = ServiceCollection::default();
    let registered: Rc<dyn INotificationService> = notification_service.clone();
    service_collection.set(I_NOTIFICATION_SERVICE, registered);
    let options = TestCodeEditorInstantiationOptions {
        options: Dynamic::object([("multiCursorLimit", Dynamic::Number(1.0))]),
        service_collection: Some(service_collection),
    };
    with_test_code_editor("first\nsecond", options, |editor, _, _| {
        editor.set_selections(&[Selection::new(1, 2, 1, 2), Selection::new(2, 2, 2, 2)], None, None);
        let prompts = notification_service.prompts.borrow();
        assert_eq!(prompts.len(), 1);
        let prompt = &prompts[0];
        let NotificationMessage::Rich(message) = &prompt.message else {
            panic!("a mensagem deveria ser um NotificationText");
        };
        let links: Vec<(String, String, bool)> = NotificationText::nodes(message)
            .iter()
            .filter_map(|node| match node {
                LinkedTextNode::Link(Link { label, href, title }) => Some((label.to_string(), href.to_string(), title.is_some())),
                _ => None,
            })
            .collect();
        assert!(matches!(prompt.severity, Severity::Warning));
        assert_eq!(
            message.to_string(),
            "The number of cursors has been limited to 1. Consider using find and replace for larger changes or increase the editor multi cursor limit setting."
        );
        assert_eq!(
            links,
            vec![(
                "find and replace".to_owned(),
                "https://code.visualstudio.com/docs/editor/codebasics#_find-and-replace".to_owned(),
                false
            )]
        );
        assert_eq!(prompt.actions, vec!["Find and Replace".to_owned(), "Increase Multi Cursor Limit".to_owned()]);
    });
}

#[test]
fn on_did_change_model_decorations() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor("", default_options(), |editor, view_model, _| {
        let disposables = DisposableStore::new();

        let invoked = Rc::new(RefCell::new(false));
        let flag = invoked.clone();
        disposables.add(editor.on_did_change_model_decorations().listen(move |_| {
            *flag.borrow_mut() = true;
        }));

        view_model.model.delta_decorations(&[], &test_decoration());

        assert!(*invoked.borrow());

        disposables.dispose();
    });
}

#[test]
fn on_did_change_model_language() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor("", default_options(), |editor, view_model, instantiation_service| {
        let language_service = instantiation_service.get(I_LANGUAGE_SERVICE);
        let disposables = DisposableStore::new();
        disposables.add(language_service.register_language(LanguageExtensionPoint::new("testMode")));

        let invoked = Rc::new(RefCell::new(false));
        let flag = invoked.clone();
        disposables.add(editor.on_did_change_model_language().listen(move |_| {
            *flag.borrow_mut() = true;
        }));

        view_model.model.set_language("testMode", None);

        assert!(*invoked.borrow());

        disposables.dispose();
    });
}

#[test]
fn on_did_change_model_language_configuration() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor("", default_options(), |editor, view_model, instantiation_service| {
        let language_configuration_service = &instantiation_service.language_configuration_service;
        let language_service = instantiation_service.get(I_LANGUAGE_SERVICE);
        let disposables = DisposableStore::new();
        disposables.add(language_service.register_language(LanguageExtensionPoint::new("testMode")));
        view_model.model.set_language("testMode", None);

        let invoked = Rc::new(RefCell::new(false));
        let flag = invoked.clone();
        disposables.add(editor.on_did_change_model_language_configuration().listen(move |_| {
            *flag.borrow_mut() = true;
        }));

        disposables.add(language_configuration_service.register(
            "testMode",
            LanguageConfiguration { brackets: Some(vec![("(".to_owned(), ")".to_owned())]), ..Default::default() },
            None,
        ));

        assert!(*invoked.borrow());

        disposables.dispose();
    });
}

#[test]
fn on_did_change_model_content() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor("", default_options(), |editor, view_model, _| {
        let disposables = DisposableStore::new();

        let invoked = Rc::new(RefCell::new(false));
        let flag = invoked.clone();
        disposables.add(editor.on_did_change_model_content().listen(move |_| {
            *flag.borrow_mut() = true;
        }));

        view_model.r#type(&u("hello"), Some("test"));

        assert!(*invoked.borrow());

        disposables.dispose();
    });
}

#[test]
fn on_did_change_model_options() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor("", default_options(), |editor, view_model, _| {
        let disposables = DisposableStore::new();

        let invoked = Rc::new(RefCell::new(false));
        let flag = invoked.clone();
        disposables.add(editor.on_did_change_model_options().listen(move |_| {
            *flag.borrow_mut() = true;
        }));

        view_model.model.update_options(je_editor::model::TextModelUpdateOptions {
            tab_size: Some(3),
            ..Default::default()
        });

        assert!(*invoked.borrow());

        disposables.dispose();
    });
}

#[test]
fn issue_145872_model_change_events_are_emitted_before_the_selection_updates() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor("", default_options(), |editor, view_model, _| {
        let disposables = DisposableStore::new();

        let observed_selection: Rc<RefCell<Option<Selection>>> = Rc::new(RefCell::new(None));
        let observed = observed_selection.clone();
        let widget = editor.widget().clone();
        disposables.add(editor.on_did_change_model_content().listen(move |_| {
            *observed.borrow_mut() = widget.get_selection();
        }));

        view_model.r#type(&u("hello"), Some("test"));

        assert_eq!(*observed_selection.borrow(), Some(Selection::new(1, 6, 1, 6)));

        disposables.dispose();
    });
}

#[test]
fn monaco_editor_issue_2774_wrong_order_of_events_on_did_change_model_content_and_on_did_change_cursor_selection_on_redo() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor("", default_options(), |editor, view_model, _| {
        let disposables = DisposableStore::new();

        let calls: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let content_calls = calls.clone();
        disposables.add(editor.on_did_change_model_content().listen(move |e| {
            content_calls.borrow_mut().push(format!("contentchange({})", changes_to_string(e)));
        }));
        let cursor_calls = calls.clone();
        disposables.add(editor.on_did_change_cursor_selection().listen(move |e| {
            cursor_calls
                .borrow_mut()
                .push(format!("cursorchange({}, {})", e.selection.position_line_number, e.selection.position_column));
        }));

        view_model.r#type(&u("a"), Some("test"));
        view_model.model.undo();
        view_model.model.redo();

        assert_eq!(
            *calls.borrow(),
            vec![
                "contentchange(a, 0, 0)",
                "cursorchange(1, 2)",
                "contentchange(, 0, 1)",
                "cursorchange(1, 1)",
                "contentchange(a, 0, 0)",
                "cursorchange(1, 2)",
            ]
        );

        disposables.dispose();
    });
}

/// Registra os quatro ouvintes (`listener1` e `listener2`, de conteúdo e de seleção) dos dois casos `issue #146174`;
/// `on_first_content` roda dentro do primeiro ouvinte de conteúdo, depois do registro da chamada.
fn check_events_delivered_in_order(on_first_content: impl Fn() + 'static, editor_widget: &je_editor::code_editor_widget::CodeEditorWidget, type_text: impl FnOnce()) {
    let disposables = DisposableStore::new();

    let calls: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let listener = |name: &'static str| {
        let content_calls = calls.clone();
        let cursor_calls = calls.clone();
        (
            move |e: &ModelContentChangedEvent| {
                content_calls.borrow_mut().push(format!("{name} - contentchange({})", changes_to_string(e)));
            },
            move |e: &je_editor::cursor_events::ICursorSelectionChangedEvent| {
                cursor_calls
                    .borrow_mut()
                    .push(format!("{name} - cursorchange({}, {})", e.selection.position_line_number, e.selection.position_column));
            },
        )
    };

    let (content1, cursor1) = listener("listener1");
    disposables.add(editor_widget.on_did_change_model_content().listen(move |e| {
        content1(e);
        on_first_content();
    }));
    disposables.add(editor_widget.on_did_change_cursor_selection().listen(cursor1));
    let (content2, cursor2) = listener("listener2");
    disposables.add(editor_widget.on_did_change_model_content().listen(content2));
    disposables.add(editor_widget.on_did_change_cursor_selection().listen(cursor2));

    type_text();

    assert_eq!(
        *calls.borrow(),
        vec![
            "listener1 - contentchange(a, 0, 0)",
            "listener2 - contentchange(a, 0, 0)",
            "listener1 - cursorchange(1, 2)",
            "listener2 - cursorchange(1, 2)",
        ]
    );

    disposables.dispose();
}

#[test]
fn issue_146174_events_delivered_out_of_order_when_adding_decorations_in_content_change_listener_1_of_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor("", default_options(), |editor, view_model, _| {
        check_events_delivered_in_order(|| {}, editor, || view_model.r#type(&u("a"), Some("test")));
    });
}

#[test]
fn issue_146174_events_delivered_out_of_order_when_adding_decorations_in_content_change_listener_2_of_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor("", default_options(), |editor, view_model, _| {
        let widget = editor.widget().clone();
        check_events_delivered_in_order(
            move || {
                widget.change_decorations(|change_accessor| {
                    change_accessor.delta_decorations(&[], &test_decoration());
                });
            },
            editor,
            || view_model.r#type(&u("a"), Some("test")),
        );
    });
}

#[test]
fn get_bottom_for_line_number_should_handle_invalid_line_numbers_gracefully() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor("line1\nline2\nline3", default_options(), |editor, _, _| {
        // Test with lineNumber greater than line count
        let result1 = editor.get_bottom_for_line_number(100, None);
        assert!(result1 >= 0.0, "Should return a valid position for out-of-bounds line number");

        // Test with lineNumber less than 1
        let result2 = editor.get_bottom_for_line_number(0, None);
        assert!(result2 >= 0.0, "Should return a valid position for line number 0");

        // Test with negative lineNumber
        let result3 = editor.get_bottom_for_line_number(-5, None);
        assert!(result3 >= 0.0, "Should return a valid position for negative line number");

        // Test with valid lineNumber should still work
        let result4 = editor.get_bottom_for_line_number(2, None);
        assert!(result4 > 0.0, "Should return a valid position for valid line number");
    });
}
