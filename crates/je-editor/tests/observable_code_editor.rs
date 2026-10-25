//! Porte de `src/vs/editor/test/browser/widget/observableCodeEditor.test.ts` (suíte `CodeEditorWidget`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso.
//! - `withEditorSetupTestFixture(preSetupCallback, cb)` é [`with_editor_setup_test_fixture`]; `withTestFixture(cb)` é
//!   a mesma função sem o `preSetupCallback`. Os `let derived; let log;` dos dois casos de interação são células
//!   (`Rc<RefCell<Option<..>>>`) preenchidas pelo `cb`, que o ouvinte registrado no `preSetupCallback` lê depois.
//! - `derivedHandleChanges` com `createChangeSummary: () => undefined` é o `FnChangeTracker` com resumo `()`, e o
//!   `context.changedObservable` comparado com `obsEditor.selections`, `versionId` e `onDidType` é a comparação das
//!   identidades dos objetos (`AnyObservable::id`).
//! - `formatChange` é [`format_change`]: o `JSON.stringify` da mudança com o replacer do original (um `Range`, e a
//!   `Selection` que o estende, vira `toString()`; `false` e array vazio somem). As mudanças do porte são o
//!   `ModelContentChangedEvent` e o `ICursorSelectionChangedEvent` dentro de um `Rc<dyn Any>` (`versionId` e `selections`),
//!   o texto digitado (`onDidType`) e a mudança indefinida (`undefined`, que `JSON.stringify` imprime como `undefined`).
//! - `editor.setPosition(new Position(1, 2))` é `editor.set_position(Position::new(1, 2), None)`, e o terceiro argumento
//!   de `editor.trigger` é o `TypePayload`.

#![allow(dead_code)]

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::DisposableStore;
use je_base::observable::base::{AnyObservable, IObservable, Observable};
use je_base::observable::change_tracker::{FnChangeTracker, IChangeContext, IChangeTracker};
use je_base::observable::debug_name::DebugNameOptions;
use je_base::observable::observables::derived::{DerivedHandleChangesOptions, IDerivedReader, derived_handle_changes};
use je_base::observable::ObservableExt;
use je_editor::code_editor_widget::{EditorViewModel, Handler, ICodeEditor, TypePayload};
use je_editor::cursor_events::ICursorSelectionChangedEvent;
use je_editor::model::Eol;
use je_editor::observable_code_editor::{ObservableCodeEditor, observable_code_editor};
use je_editor::position::Position;
use je_editor::text_model_events::ModelContentChangedEvent;
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor};

/// `class Log`.
#[derive(Default)]
struct Log {
    entries: RefCell<Vec<String>>,
}

impl Log {
    fn log(&self, message: impl Into<String>) {
        self.entries.borrow_mut().push(message.into());
    }

    fn get_and_clear_entries(&self) -> Vec<String> {
        std::mem::take(&mut *self.entries.borrow_mut())
    }
}

/// `{ editor, viewModel, log, derived }`.
struct FixtureArgs {
    editor: Rc<TestCodeEditor>,
    view_model: Rc<EditorViewModel>,
    log: Rc<Log>,
    derived: IObservable<String>,
}

/// O `JSON.stringify` de uma string.
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// O `JSON.stringify` de uma lista de seleções (cada uma vira o `toString()`).
fn json_selections(selections: &[je_editor::selection::Selection]) -> String {
    format!("[{}]", selections.iter().map(|s| json_string(&s.to_string())).collect::<Vec<_>>().join(","))
}

/// O `IModelContentChangedEvent` na ordem das propriedades do original, sem os `false` e os arrays vazios.
fn format_content_event(event: &ModelContentChangedEvent) -> String {
    let mut fields: Vec<String> = Vec::new();
    if !event.changes.is_empty() {
        let changes = event
            .changes
            .iter()
            .map(|change| {
                format!(
                    "{{\"range\":{},\"rangeLength\":{},\"text\":{},\"rangeOffset\":{}}}",
                    json_string(&change.range.to_string()),
                    change.range_length,
                    json_string(&change.text.to_string_lossy()),
                    change.range_offset
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        fields.push(format!("\"changes\":[{changes}]"));
    }
    fields.push(format!("\"eol\":{}", json_string(if event.eol == Eol::Lf { "\n" } else { "\r\n" })));
    fields.push(format!("\"versionId\":{}", event.version_id));
    for (name, value) in [
        ("isUndoing", event.is_undoing),
        ("isRedoing", event.is_redoing),
        ("isFlush", event.is_flush),
        ("isEolChange", event.is_eol_change),
    ] {
        if value {
            fields.push(format!("\"{name}\":true"));
        }
    }
    if !event.detailed_reasons.is_empty() {
        let reasons = event
            .detailed_reasons
            .iter()
            .map(|reason| {
                let metadata = reason
                    .metadata
                    .iter()
                    .filter_map(|(key, value)| value.as_ref().map(|value| format!("{}:{}", json_string(key), json_string(value))))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{\"metadata\":{{{metadata}}}}}")
            })
            .collect::<Vec<_>>()
            .join(",");
        fields.push(format!("\"detailedReasons\":[{reasons}]"));
    }
    if !event.detailed_reasons_change_lengths.is_empty() {
        let lengths = event.detailed_reasons_change_lengths.iter().map(usize::to_string).collect::<Vec<_>>().join(",");
        fields.push(format!("\"detailedReasonsChangeLengths\":[{lengths}]"));
    }
    format!("{{{}}}", fields.join(","))
}

/// O `ICursorSelectionChangedEvent` na ordem das propriedades do original.
fn format_selection_event(event: &ICursorSelectionChangedEvent) -> String {
    let mut fields = vec![format!("\"selection\":{}", json_string(&event.selection.to_string()))];
    if !event.secondary_selections.is_empty() {
        fields.push(format!("\"secondarySelections\":{}", json_selections(&event.secondary_selections)));
    }
    fields.push(format!("\"modelVersionId\":{}", event.model_version_id));
    match &event.old_selections {
        None => fields.push("\"oldSelections\":null".to_owned()),
        Some(old) if !old.is_empty() => fields.push(format!("\"oldSelections\":{}", json_selections(old))),
        Some(_) => {}
    }
    fields.push(format!("\"oldModelVersionId\":{}", event.old_model_version_id));
    fields.push(format!("\"source\":{}", json_string(&event.source)));
    fields.push(format!("\"reason\":{}", event.reason as i32));
    format!("{{{}}}", fields.join(","))
}

/// `formatChange(change)`.
fn format_change(context: &dyn IChangeContext) -> String {
    let change = context.change_any();
    if let Some(inner) = change.downcast_ref::<Rc<dyn Any>>() {
        if let Some(event) = (**inner).downcast_ref::<ModelContentChangedEvent>() {
            return format_content_event(event);
        }
        if let Some(event) = (**inner).downcast_ref::<ICursorSelectionChangedEvent>() {
            return format_selection_event(event);
        }
    }
    if let Some(text) = change.downcast_ref::<JsString>() {
        return json_string(&text.to_string_lossy());
    }
    "undefined".to_owned()
}

/// `observableName(obs, obsEditor)`.
fn observable_name(context: &dyn IChangeContext, obs_editor: &ObservableCodeEditor) -> &'static str {
    let changed = context.changed_observable().id();
    if changed == obs_editor.selections.id() {
        "editor.selections"
    } else if changed == obs_editor.version_id.id() {
        "editor.versionId"
    } else if changed == obs_editor.on_did_type.id() {
        "editor.onDidType"
    } else {
        "unknown"
    }
}

fn with_test_fixture(cb: impl FnOnce(&FixtureArgs)) {
    with_editor_setup_test_fixture(|_, _| {}, cb);
}

fn with_editor_setup_test_fixture(pre_setup_callback: impl FnOnce(&Rc<TestCodeEditor>, &Rc<DisposableStore>), cb: impl FnOnce(&FixtureArgs)) {
    with_test_code_editor("hello world", TestCodeEditorInstantiationOptions::default(), |editor, view_model, _services| {
        let disposables = DisposableStore::new();
        pre_setup_callback(editor, &disposables);
        let obs_editor = observable_code_editor(editor.widget());
        let log = Rc::new(Log::default());

        let change_tracker: Rc<dyn IChangeTracker<()>> = FnChangeTracker::new(
            |_previous: Option<&()>| (),
            {
                let (obs_editor, log) = (Rc::clone(&obs_editor), Rc::clone(&log));
                move |context: &dyn IChangeContext, _summary: &()| {
                    let obs_name = observable_name(context, &obs_editor);
                    log.log(format!("handle change: {obs_name} {}", format_change(context)));
                    true
                }
            },
        );
        let derived = derived_handle_changes(
            DerivedHandleChangesOptions { debug: DebugNameOptions::default(), change_tracker, equality_comparer: None },
            {
                let (obs_editor, log) = (Rc::clone(&obs_editor), Rc::clone(&log));
                move |reader: &dyn IDerivedReader<()>, _summary: &()| {
                    let version_id = obs_editor.version_id.read(reader);
                    let selection = obs_editor
                        .selections
                        .read(reader)
                        .map(|selections| selections.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", "));
                    obs_editor.on_did_type.read(reader);

                    let str = format!(
                        "running derived: selection: {}, value: {}",
                        selection.unwrap_or_else(|| "undefined".to_owned()),
                        version_id.map_or_else(|| "null".to_owned(), |version_id| version_id.to_string()),
                    );
                    log.log(str.clone());
                    str
                }
            },
        );

        derived.recompute_initially_and_on_change(&disposables, None);
        assert_eq!(log.get_and_clear_entries(), vec!["running derived: selection: [1,1 -> 1,1], value: 1"]);

        cb(&FixtureArgs { editor: Rc::clone(editor), view_model: Rc::clone(view_model), log, derived });

        disposables.dispose();
    });
}

#[test]
fn set_position() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_fixture(|args| {
        args.editor.set_position(Position::new(1, 2), None);

        assert_eq!(
            args.log.get_and_clear_entries(),
            vec![
                r#"handle change: editor.selections {"selection":"[1,2 -> 1,2]","modelVersionId":1,"oldSelections":["[1,1 -> 1,1]"],"oldModelVersionId":1,"source":"api","reason":0}"#,
                "running derived: selection: [1,2 -> 1,2], value: 1",
            ]
        );
    });
}

#[test]
fn keyboard_type() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_fixture(|args| {
        args.editor.trigger(Some("keyboard"), Handler::TYPE, Some(TypePayload { text: JsString::from("abc") }.to_dynamic()));

        assert_eq!(
            args.log.get_and_clear_entries(),
            vec![
                r#"handle change: editor.onDidType "abc""#,
                r#"handle change: editor.versionId {"changes":[{"range":"[1,1 -> 1,1]","rangeLength":0,"text":"a","rangeOffset":0}],"eol":"\n","versionId":2,"detailedReasons":[{"metadata":{"source":"cursor","kind":"type","detailedSource":"keyboard"}}],"detailedReasonsChangeLengths":[1]}"#,
                r#"handle change: editor.versionId {"changes":[{"range":"[1,2 -> 1,2]","rangeLength":0,"text":"b","rangeOffset":1}],"eol":"\n","versionId":3,"detailedReasons":[{"metadata":{"source":"cursor","kind":"type","detailedSource":"keyboard"}}],"detailedReasonsChangeLengths":[1]}"#,
                r#"handle change: editor.versionId {"changes":[{"range":"[1,3 -> 1,3]","rangeLength":0,"text":"c","rangeOffset":2}],"eol":"\n","versionId":4,"detailedReasons":[{"metadata":{"source":"cursor","kind":"type","detailedSource":"keyboard"}}],"detailedReasonsChangeLengths":[1]}"#,
                r#"handle change: editor.selections {"selection":"[1,4 -> 1,4]","modelVersionId":4,"oldSelections":["[1,1 -> 1,1]"],"oldModelVersionId":1,"source":"keyboard","reason":0}"#,
                "running derived: selection: [1,4 -> 1,4], value: 4",
            ]
        );
    });
}

#[test]
fn keyboard_type_and_set_position() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_fixture(|args| {
        args.editor.trigger(Some("keyboard"), Handler::TYPE, Some(TypePayload { text: JsString::from("abc") }.to_dynamic()));

        assert_eq!(
            args.log.get_and_clear_entries(),
            vec![
                r#"handle change: editor.onDidType "abc""#,
                r#"handle change: editor.versionId {"changes":[{"range":"[1,1 -> 1,1]","rangeLength":0,"text":"a","rangeOffset":0}],"eol":"\n","versionId":2,"detailedReasons":[{"metadata":{"source":"cursor","kind":"type","detailedSource":"keyboard"}}],"detailedReasonsChangeLengths":[1]}"#,
                r#"handle change: editor.versionId {"changes":[{"range":"[1,2 -> 1,2]","rangeLength":0,"text":"b","rangeOffset":1}],"eol":"\n","versionId":3,"detailedReasons":[{"metadata":{"source":"cursor","kind":"type","detailedSource":"keyboard"}}],"detailedReasonsChangeLengths":[1]}"#,
                r#"handle change: editor.versionId {"changes":[{"range":"[1,3 -> 1,3]","rangeLength":0,"text":"c","rangeOffset":2}],"eol":"\n","versionId":4,"detailedReasons":[{"metadata":{"source":"cursor","kind":"type","detailedSource":"keyboard"}}],"detailedReasonsChangeLengths":[1]}"#,
                r#"handle change: editor.selections {"selection":"[1,4 -> 1,4]","modelVersionId":4,"oldSelections":["[1,1 -> 1,1]"],"oldModelVersionId":1,"source":"keyboard","reason":0}"#,
                "running derived: selection: [1,4 -> 1,4], value: 4",
            ]
        );

        args.editor.set_position(Position::new(1, 5), Some("test"));

        assert_eq!(
            args.log.get_and_clear_entries(),
            vec![
                r#"handle change: editor.selections {"selection":"[1,5 -> 1,5]","modelVersionId":4,"oldSelections":["[1,4 -> 1,4]"],"oldModelVersionId":4,"source":"test","reason":0}"#,
                "running derived: selection: [1,5 -> 1,5], value: 4",
            ]
        );
    });
}

#[test]
fn listener_interaction_unforced() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let derived: Rc<RefCell<Option<IObservable<String>>>> = Rc::default();
    let log: Rc<RefCell<Option<Rc<Log>>>> = Rc::default();
    with_editor_setup_test_fixture(
        {
            let (derived, log) = (Rc::clone(&derived), Rc::clone(&log));
            move |editor, disposables| {
                disposables.add(editor.on_did_change_model_content().listen(move |_| {
                    let log = log.borrow().clone().expect("o log existe antes da primeira digitação");
                    let derived = derived.borrow().clone().expect("o derived existe antes da primeira digitação");
                    log.log(">>> before get");
                    derived.get();
                    log.log("<<< after get");
                }));
            }
        },
        |args| {
            *derived.borrow_mut() = Some(Rc::clone(&args.derived));
            *log.borrow_mut() = Some(Rc::clone(&args.log));

            args.editor.trigger(Some("keyboard"), Handler::TYPE, Some(TypePayload { text: JsString::from("a") }.to_dynamic()));
            assert_eq!(
                args.log.get_and_clear_entries(),
                vec![
                    ">>> before get",
                    "<<< after get",
                    r#"handle change: editor.onDidType "a""#,
                    r#"handle change: editor.versionId {"changes":[{"range":"[1,1 -> 1,1]","rangeLength":0,"text":"a","rangeOffset":0}],"eol":"\n","versionId":2,"detailedReasons":[{"metadata":{"source":"cursor","kind":"type","detailedSource":"keyboard"}}],"detailedReasonsChangeLengths":[1]}"#,
                    r#"handle change: editor.selections {"selection":"[1,2 -> 1,2]","modelVersionId":2,"oldSelections":["[1,1 -> 1,1]"],"oldModelVersionId":1,"source":"keyboard","reason":0}"#,
                    "running derived: selection: [1,2 -> 1,2], value: 2",
                ]
            );
            derived.borrow_mut().take();
            log.borrow_mut().take();
        },
    );
}

#[test]
fn listener_interaction() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let derived: Rc<RefCell<Option<IObservable<String>>>> = Rc::default();
    let log: Rc<RefCell<Option<Rc<Log>>>> = Rc::default();
    with_editor_setup_test_fixture(
        {
            let (derived, log) = (Rc::clone(&derived), Rc::clone(&log));
            move |editor, disposables| {
                let widget = Rc::clone(editor.widget());
                disposables.add(editor.on_did_change_model_content().listen(move |_| {
                    let log = log.borrow().clone().expect("o log existe antes da primeira digitação");
                    let derived = derived.borrow().clone().expect("o derived existe antes da primeira digitação");
                    log.log(">>> before forceUpdate");
                    observable_code_editor(&widget).force_update();

                    log.log(">>> before get");
                    derived.get();
                    log.log("<<< after get");
                }));
            }
        },
        |args| {
            *derived.borrow_mut() = Some(Rc::clone(&args.derived));
            *log.borrow_mut() = Some(Rc::clone(&args.log));

            args.editor.trigger(Some("keyboard"), Handler::TYPE, Some(TypePayload { text: JsString::from("a") }.to_dynamic()));

            assert_eq!(
                args.log.get_and_clear_entries(),
                vec![
                    ">>> before forceUpdate",
                    ">>> before get",
                    "handle change: editor.versionId undefined",
                    "running derived: selection: [1,2 -> 1,2], value: 2",
                    "<<< after get",
                    r#"handle change: editor.onDidType "a""#,
                    r#"handle change: editor.versionId {"changes":[{"range":"[1,1 -> 1,1]","rangeLength":0,"text":"a","rangeOffset":0}],"eol":"\n","versionId":2,"detailedReasons":[{"metadata":{"source":"cursor","kind":"type","detailedSource":"keyboard"}}],"detailedReasonsChangeLengths":[1]}"#,
                    r#"handle change: editor.selections {"selection":"[1,2 -> 1,2]","modelVersionId":2,"oldSelections":["[1,1 -> 1,1]"],"oldModelVersionId":1,"source":"keyboard","reason":0}"#,
                    "running derived: selection: [1,2 -> 1,2], value: 2",
                ]
            );
            derived.borrow_mut().take();
            log.borrow_mut().take();
        },
    );
}
