//! Porte de `src/vs/platform/keybinding/test/common/keybindingResolver.test.ts` (suíte `KeybindingResolver`), caso a caso e na mesma ordem.
//!
//! Divergências: o `assert.deepStrictEqual` sobre listas de `ResolvedKeybindingItem` é a igualdade estrutural
//! de `common/keybinding.rs` (rótulo de configuração, acordes de despacho, comando, argumentos, `when`
//! serializado e flags); a identidade dos argumentos de comando (`r.commandArgs === commandArgs`) é a
//! igualdade de valor do `Dynamic`.

mod common;

use std::rc::Rc;

use common::keybinding::{create_us_layout_resolved_keybinding, lists_equal, os};
use common::*;
use je_base::dynamic::Dynamic;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::keybindings::{BinaryKeybinding, Chord, KeyCodeChord, create_simple_keybinding, decode_keybinding};
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression, IContext};
use je_platform::keybinding::keybinding_resolver::{KeybindingResolver, ResolutionResult, ResultKind};
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;

struct TestContext(Dynamic);

impl IContext for TestContext {
    fn get_value(&self, key: &str) -> Dynamic {
        self.0.get_str(key)
    }
}

fn create_context(ctx: &str) -> TestContext {
    TestContext(j(ctx))
}

fn kb(code: KeyCode) -> u32 {
    code.0 as u32
}

fn kb_item(
    keybinding: impl Into<BinaryKeybinding>,
    command: &str,
    command_args: Dynamic,
    when: Option<ContextKeyExpression>,
    is_default: bool,
) -> Rc<ResolvedKeybindingItem> {
    let resolved_keybinding = create_us_layout_resolved_keybinding(keybinding, os());
    ResolvedKeybindingItem::new(resolved_keybinding, Some(command), command_args, when, is_default, None, false, false)
}

fn get_dispatch_str(chord: &KeyCodeChord) -> String {
    USLayoutResolvedKeybinding::get_dispatch_str(chord).unwrap()
}

fn equals(key: &str, value: &str) -> ContextKeyExpression {
    ContextKeyExpr::equals(key, value)
}

fn removals(defaults: Vec<Rc<ResolvedKeybindingItem>>, overrides: Vec<Rc<ResolvedKeybindingItem>>) -> Vec<Rc<ResolvedKeybindingItem>> {
    KeybindingResolver::handle_removals(defaults.into_iter().chain(overrides).collect())
}

fn no_log() -> Rc<dyn Fn(&str)> {
    Rc::new(|_| {})
}

#[test]
fn resolve_key() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let keybinding = KeyMod::CTRL_CMD | KeyMod::SHIFT | kb(KeyCode::KeyZ);
    let runtime_keybinding = create_simple_keybinding(keybinding, os());
    let context_rules = equals("bar", "baz");
    let keybinding_item = kb_item(keybinding, "yes", Dynamic::Null, Some(context_rules.clone()), true);

    assert!(context_rules.evaluate(&create_context("{'bar': 'baz'}")));
    assert!(!context_rules.evaluate(&create_context("{'bar': 'bz'}")));

    let resolver = KeybindingResolver::new(vec![keybinding_item], vec![], no_log());

    let r1 = resolver.resolve(&create_context("{'bar': 'baz'}"), &[], &get_dispatch_str(&runtime_keybinding));
    assert_eq!(r1.kind(), ResultKind::KbFound);
    match r1 {
        ResolutionResult::KbFound { command_id, .. } => assert_eq!(command_id.as_deref(), Some("yes")),
        other => panic!("esperava KbFound, veio {other:?}"),
    }

    let r2 = resolver.resolve(&create_context("{'bar': 'bz'}"), &[], &get_dispatch_str(&runtime_keybinding));
    assert_eq!(r2.kind(), ResultKind::NoMatchingKb);
}

#[test]
fn resolve_key_with_arguments() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let command_args = j("{'text': 'no'}");
    let keybinding = KeyMod::CTRL_CMD | KeyMod::SHIFT | kb(KeyCode::KeyZ);
    let runtime_keybinding = create_simple_keybinding(keybinding, os());
    let context_rules = equals("bar", "baz");
    let keybinding_item = kb_item(keybinding, "yes", command_args.clone(), Some(context_rules), true);

    let resolver = KeybindingResolver::new(vec![keybinding_item], vec![], no_log());

    let r = resolver.resolve(&create_context("{'bar': 'baz'}"), &[], &get_dispatch_str(&runtime_keybinding));
    match r {
        ResolutionResult::KbFound { command_args: actual, .. } => assert!(actual.strict_equals(&command_args)),
        other => panic!("esperava KbFound, veio {other:?}"),
    }
}

mod handle_keybinding_removals {
    use super::*;

    fn key(code: KeyCode) -> u32 {
        code.0 as u32
    }

    #[test]
    fn simple_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let defaults = vec![kb_item(key(KeyCode::KeyA), "yes1", Dynamic::Null, Some(equals("1", "a")), true)];
        let overrides = vec![kb_item(key(KeyCode::KeyB), "yes2", Dynamic::Null, Some(equals("2", "b")), false)];
        let actual = removals(defaults, overrides);
        assert!(lists_equal(
            &actual,
            &[
                kb_item(key(KeyCode::KeyA), "yes1", Dynamic::Null, Some(equals("1", "a")), true),
                kb_item(key(KeyCode::KeyB), "yes2", Dynamic::Null, Some(equals("2", "b")), false),
            ]
        ));
    }

    #[test]
    fn simple_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let defaults = vec![
            kb_item(key(KeyCode::KeyA), "yes1", Dynamic::Null, Some(equals("1", "a")), true),
            kb_item(key(KeyCode::KeyB), "yes2", Dynamic::Null, Some(equals("2", "b")), true),
        ];
        let overrides = vec![kb_item(key(KeyCode::KeyC), "yes3", Dynamic::Null, Some(equals("3", "c")), false)];
        let actual = removals(defaults, overrides);
        assert!(lists_equal(
            &actual,
            &[
                kb_item(key(KeyCode::KeyA), "yes1", Dynamic::Null, Some(equals("1", "a")), true),
                kb_item(key(KeyCode::KeyB), "yes2", Dynamic::Null, Some(equals("2", "b")), true),
                kb_item(key(KeyCode::KeyC), "yes3", Dynamic::Null, Some(equals("3", "c")), false),
            ]
        ));
    }

    fn two_defaults() -> Vec<Rc<ResolvedKeybindingItem>> {
        vec![
            kb_item(key(KeyCode::KeyA), "yes1", Dynamic::Null, Some(equals("1", "a")), true),
            kb_item(key(KeyCode::KeyB), "yes2", Dynamic::Null, Some(equals("2", "b")), true),
        ]
    }

    #[test]
    fn removal_with_not_matching_when() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let overrides = vec![kb_item(key(KeyCode::KeyA), "-yes1", Dynamic::Null, Some(equals("1", "b")), false)];
        let actual = removals(two_defaults(), overrides);
        assert!(lists_equal(&actual, &two_defaults()));
    }

    #[test]
    fn removal_with_not_matching_keybinding() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let overrides = vec![kb_item(key(KeyCode::KeyB), "-yes1", Dynamic::Null, Some(equals("1", "a")), false)];
        let actual = removals(two_defaults(), overrides);
        assert!(lists_equal(&actual, &two_defaults()));
    }

    fn only_yes2() -> Vec<Rc<ResolvedKeybindingItem>> {
        vec![kb_item(key(KeyCode::KeyB), "yes2", Dynamic::Null, Some(equals("2", "b")), true)]
    }

    #[test]
    fn removal_with_matching_keybinding_and_when() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let overrides = vec![kb_item(key(KeyCode::KeyA), "-yes1", Dynamic::Null, Some(equals("1", "a")), false)];
        let actual = removals(two_defaults(), overrides);
        assert!(lists_equal(&actual, &only_yes2()));
    }

    #[test]
    fn removal_with_unspecified_keybinding() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let overrides = vec![kb_item(0u32, "-yes1", Dynamic::Null, Some(equals("1", "a")), false)];
        let actual = removals(two_defaults(), overrides);
        assert!(lists_equal(&actual, &only_yes2()));
    }

    #[test]
    fn removal_with_unspecified_when() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let overrides = vec![kb_item(key(KeyCode::KeyA), "-yes1", Dynamic::Null, None, false)];
        let actual = removals(two_defaults(), overrides);
        assert!(lists_equal(&actual, &only_yes2()));
    }

    #[test]
    fn removal_with_unspecified_when_and_unspecified_keybinding() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let overrides = vec![kb_item(0u32, "-yes1", Dynamic::Null, None, false)];
        let actual = removals(two_defaults(), overrides);
        assert!(lists_equal(&actual, &only_yes2()));
    }

    #[test]
    fn issue_138997_removal_in_default_list() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let defaults = vec![
            kb_item(key(KeyCode::KeyA), "yes1", Dynamic::Null, None, true),
            kb_item(key(KeyCode::KeyB), "yes2", Dynamic::Null, None, true),
            kb_item(0u32, "-yes1", Dynamic::Null, None, false),
        ];
        let actual = removals(defaults, vec![]);
        assert!(lists_equal(&actual, &[kb_item(key(KeyCode::KeyB), "yes2", Dynamic::Null, None, true)]));
    }

    #[test]
    fn issue_612_issuecomment_222109084_cannot_remove_keybindings_for_commands_with_caret() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let defaults = vec![
            kb_item(key(KeyCode::KeyA), "^yes1", Dynamic::Null, Some(equals("1", "a")), true),
            kb_item(key(KeyCode::KeyB), "yes2", Dynamic::Null, Some(equals("2", "b")), true),
        ];
        let overrides = vec![kb_item(key(KeyCode::KeyA), "-yes1", Dynamic::Null, None, false)];
        let actual = removals(defaults, overrides);
        assert!(lists_equal(&actual, &only_yes2()));
    }

    #[test]
    fn issue_140884_unable_to_reassign_f1_as_keybinding_for_show_all_commands() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let defaults = vec![kb_item(key(KeyCode::KeyA), "command1", Dynamic::Null, None, true)];
        let overrides = vec![
            kb_item(key(KeyCode::KeyA), "-command1", Dynamic::Null, None, false),
            kb_item(key(KeyCode::KeyA), "command1", Dynamic::Null, None, false),
        ];
        let actual = removals(defaults, overrides);
        assert!(lists_equal(&actual, &[kb_item(key(KeyCode::KeyA), "command1", Dynamic::Null, None, false)]));
    }

    #[test]
    fn issue_141638_keyboard_shortcuts_change_when_expression_might_actually_remove_keybinding_in_insiders() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let defaults = vec![kb_item(key(KeyCode::KeyA), "command1", Dynamic::Null, None, true)];
        let overrides = vec![
            kb_item(key(KeyCode::KeyA), "command1", Dynamic::Null, Some(equals("a", "1")), false),
            kb_item(key(KeyCode::KeyA), "-command1", Dynamic::Null, None, false),
        ];
        let actual = removals(defaults, overrides);
        assert!(lists_equal(&actual, &[kb_item(key(KeyCode::KeyA), "command1", Dynamic::Null, Some(equals("a", "1")), false)]));
    }

    #[test]
    fn issue_157751_auto_quoting_of_context_keys_prevents_removal_of_keybindings_via_ui() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let defaults = vec![kb_item(
            key(KeyCode::KeyA),
            "command1",
            Dynamic::Null,
            ContextKeyExpr::deserialize(Some("editorTextFocus && activeEditor != workbench.editor.notebook && editorLangId in julia.supportedLanguageIds")),
            true,
        )];
        let overrides = vec![kb_item(
            key(KeyCode::KeyA),
            "-command1",
            Dynamic::Null,
            ContextKeyExpr::deserialize(Some("editorTextFocus && activeEditor != 'workbench.editor.notebook' && editorLangId in 'julia.supportedLanguageIds'")),
            false,
        )];
        let actual = removals(defaults, overrides);
        assert!(actual.is_empty());
    }

    #[test]
    fn issue_293802_removal_still_matches_when_default_when_clause_becomes_more_specific() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let defaults = vec![kb_item(
            key(KeyCode::KeyA),
            "command1",
            Dynamic::Null,
            ContextKeyExpr::and([ContextKeyExpr::has("inChatInput"), ContextKeyExpr::not("withinEditSessionDiff")]),
            true,
        )];
        let overrides = vec![kb_item(key(KeyCode::KeyA), "-command1", Dynamic::Null, Some(ContextKeyExpr::has("inChatInput")), false)];
        let actual = removals(defaults, overrides);
        assert!(actual.is_empty());
    }

    #[test]
    fn removal_with_more_specific_when_clause_does_not_match_broader_default() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let defaults = vec![kb_item(key(KeyCode::KeyA), "command1", Dynamic::Null, Some(ContextKeyExpr::has("inChatInput")), true)];
        let overrides = vec![kb_item(
            key(KeyCode::KeyA),
            "-command1",
            Dynamic::Null,
            ContextKeyExpr::and([ContextKeyExpr::has("inChatInput"), ContextKeyExpr::not("withinEditSessionDiff")]),
            false,
        )];
        let actual = removals(defaults, overrides);
        assert!(lists_equal(&actual, &[kb_item(key(KeyCode::KeyA), "command1", Dynamic::Null, Some(ContextKeyExpr::has("inChatInput")), true)]));
    }

    #[test]
    fn issue_160604_remove_keybindings_with_when_clause_does_not_work() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let defaults = vec![kb_item(key(KeyCode::KeyA), "command1", Dynamic::Null, None, true)];
        let overrides = vec![kb_item(key(KeyCode::KeyA), "-command1", Dynamic::Null, Some(ContextKeyExpr::true_()), false)];
        let actual = removals(defaults, overrides);
        assert!(actual.is_empty());
    }

    #[test]
    fn context_is_entirely_included() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        enum E<'a> {
            Text(&'a str),
            Expr(ContextKeyExpression),
            Null,
        }
        let to_expr = |expr: E| match expr {
            E::Text(text) => ContextKeyExpr::deserialize(Some(text)),
            E::Expr(expr) => Some(expr),
            E::Null => ContextKeyExpr::deserialize(None),
        };
        let assert_included = |a: E, b: E| assert!(KeybindingResolver::when_is_entirely_included(to_expr(a).as_ref(), to_expr(b).as_ref()));
        let assert_not_included = |a: E, b: E| assert!(!KeybindingResolver::when_is_entirely_included(to_expr(a).as_ref(), to_expr(b).as_ref()));
        use E::{Expr as X, Null, Text as T};

        assert_included(Null, Null);
        assert_included(Null, X(ContextKeyExpr::true_()));
        assert_included(X(ContextKeyExpr::true_()), Null);
        assert_included(X(ContextKeyExpr::true_()), X(ContextKeyExpr::true_()));

        assert_included(T("key1"), Null);
        assert_included(T("key1"), T(""));
        assert_included(T("key1"), T("key1"));
        assert_included(T("key1"), X(ContextKeyExpr::true_()));

        assert_included(T("!key1"), T(""));
        assert_included(T("!key1"), T("!key1"));

        assert_included(T("key2"), T(""));
        assert_included(T("key2"), T("key2"));

        assert_included(T("key1 && key1 && key2 && key2"), T("key2"));
        assert_included(T("key1 && key2"), T("key2"));
        assert_included(T("key1 && key2"), T("key1"));
        assert_included(T("key1 && key2"), T(""));

        assert_included(T("key1"), T("key1 || key2"));
        assert_included(T("key1 || !key1"), T("key2 || !key2"));
        assert_included(T("key1"), T("key1 || key2 && key3"));

        assert_not_included(T("key1"), T("!key1"));
        assert_not_included(T("!key1"), T("key1"));
        assert_not_included(T("key1 && key2"), T("key3"));
        assert_not_included(T("key1 && key2"), T("key4"));
        assert_not_included(T("key1"), T("key2"));
        assert_not_included(T("key1 || key2"), T("key2"));
        assert_not_included(T(""), T("key2"));
        assert_not_included(Null, T("key2"));
    }
}

mod resolve_command {
    use super::*;

    fn k(code: KeyCode) -> u32 {
        code.0 as u32
    }

    fn item(keybinding: impl Into<BinaryKeybinding>, command: &str, when: Option<ContextKeyExpression>) -> Rc<ResolvedKeybindingItem> {
        kb_item(keybinding, command, Dynamic::Null, when, true)
    }

    fn items() -> Vec<Rc<ResolvedKeybindingItem>> {
        let ctrl = KeyMod::CTRL_CMD;
        vec![
            // This one will never match because its "when" is always overwritten by another one
            item(
                k(KeyCode::KeyX),
                "first",
                ContextKeyExpr::and([ContextKeyExpr::equals("key1", true), ContextKeyExpr::not_equals("key2", false)]),
            ),
            // This one always overwrites first
            item(k(KeyCode::KeyX), "second", Some(ContextKeyExpr::equals("key2", true))),
            // This one is a secondary mapping for `second`
            item(k(KeyCode::KeyZ), "second", None),
            // This one sometimes overwrites first
            item(k(KeyCode::KeyX), "third", Some(ContextKeyExpr::equals("key3", true))),
            // This one is always overwritten by another one
            item(ctrl | k(KeyCode::KeyY), "fourth", Some(ContextKeyExpr::equals("key4", true))),
            // This one overwrites with a chord the previous one
            item(key_chord(ctrl | k(KeyCode::KeyY), k(KeyCode::KeyZ)), "fifth", None),
            // This one has no keybinding
            item(0u32, "sixth", None),
            item(key_chord(ctrl | k(KeyCode::KeyK), ctrl | k(KeyCode::KeyU)), "seventh", None),
            item(key_chord(ctrl | k(KeyCode::KeyK), ctrl | k(KeyCode::KeyK)), "seventh", None),
            item(key_chord(ctrl | k(KeyCode::KeyK), ctrl | k(KeyCode::KeyU)), "uncomment lines", None),
            // cmd+k cmd+c
            item(key_chord(ctrl | k(KeyCode::KeyK), ctrl | k(KeyCode::KeyC)), "comment lines", None),
            // cmd+g cmd+c
            item(key_chord(ctrl | k(KeyCode::KeyG), ctrl | k(KeyCode::KeyC)), "unreachablechord", None),
            // cmd+g
            item(ctrl | k(KeyCode::KeyG), "eleven", None),
            // cmd+k a b
            item(vec![ctrl | k(KeyCode::KeyK), k(KeyCode::KeyA), k(KeyCode::KeyB)], "long multi chord", None),
            // cmd+b cmd+c
            item(vec![ctrl | k(KeyCode::KeyB), ctrl | k(KeyCode::KeyC)], "shadowed by long-multi-chord-2", None),
            // cmd+b cmd+c i
            item(vec![ctrl | k(KeyCode::KeyB), ctrl | k(KeyCode::KeyC), k(KeyCode::KeyI)], "long-multi-chord-2", None),
        ]
    }

    fn resolver() -> KeybindingResolver {
        KeybindingResolver::new(items(), vec![], no_log())
    }

    fn test_kb_lookup_by_command(resolver: &KeybindingResolver, command_id: &str, expected_keys: Vec<BinaryKeybinding>) {
        // Test lookup
        let lookup_result = resolver.lookup_keybindings(command_id);
        assert_eq!(lookup_result.len(), expected_keys.len(), "Length mismatch @ commandId {command_id}");
        for (i, result) in lookup_result.iter().enumerate() {
            let expected = create_us_layout_resolved_keybinding(expected_keys[i].clone(), os()).unwrap();
            assert_eq!(
                result.resolved_keybinding.as_ref().unwrap().get_user_settings_label(),
                expected.get_user_settings_label(),
                "value mismatch @ commandId {command_id}"
            );
        }
    }

    fn test_resolve(resolver: &KeybindingResolver, ctx: &dyn IContext, expected_key: impl Into<BinaryKeybinding>, command_id: &str) {
        let expected_keybinding = decode_keybinding(expected_key, os()).unwrap().unwrap();
        let mut previous_chord: Vec<String> = Vec::new();
        let len = expected_keybinding.chords.len();
        for (i, chord) in expected_keybinding.chords.iter().enumerate() {
            let Chord::KeyCode(chord) = chord else { panic!("esperava acorde por keyCode") };
            let chord = get_dispatch_str(chord);
            let result = resolver.resolve(ctx, &previous_chord, &chord);
            if i == len - 1 {
                // if it's the final chord, then we should find a valid command, and there should not be a chord.
                match result {
                    ResolutionResult::KbFound { command_id: found, .. } => {
                        assert_eq!(found.as_deref(), Some(command_id), "Enters multi chord for {command_id} at chord {i}");
                    }
                    other => panic!("Enters multi chord for {command_id} at chord {i}: {other:?}"),
                }
            } else if i > 0 {
                // if this is an intermediate chord, we should not find a valid command, and there should be an
                // open chord we continue.
                assert_eq!(result.kind(), ResultKind::MoreChordsNeeded, "Continues multi chord for {command_id} at chord {i}");
            } else {
                // if it's not the final chord and not an intermediate, then we should not find a valid command,
                // and we should enter a chord.
                assert_eq!(result.kind(), ResultKind::MoreChordsNeeded, "Enters multi chord for {command_id} at chord {i}");
            }
            previous_chord.push(chord);
        }
    }

    fn many(values: &[u32]) -> BinaryKeybinding {
        BinaryKeybinding::Chords(values.to_vec())
    }

    fn one(value: u32) -> BinaryKeybinding {
        BinaryKeybinding::Number(value)
    }

    #[test]
    fn resolve_command_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_kb_lookup_by_command(&resolver(), "first", vec![]);
    }

    #[test]
    fn resolve_command_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let r = resolver();
        test_kb_lookup_by_command(&r, "second", vec![one(k(KeyCode::KeyZ)), one(k(KeyCode::KeyX))]);
        test_resolve(&r, &create_context("{'key2': true}"), k(KeyCode::KeyX), "second");
        test_resolve(&r, &create_context("{}"), k(KeyCode::KeyZ), "second");
    }

    #[test]
    fn resolve_command_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let r = resolver();
        test_kb_lookup_by_command(&r, "third", vec![one(k(KeyCode::KeyX))]);
        test_resolve(&r, &create_context("{'key3': true}"), k(KeyCode::KeyX), "third");
    }

    #[test]
    fn resolve_command_4() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_kb_lookup_by_command(&resolver(), "fourth", vec![]);
    }

    #[test]
    fn resolve_command_5() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let r = resolver();
        let chord = key_chord(KeyMod::CTRL_CMD | k(KeyCode::KeyY), k(KeyCode::KeyZ));
        test_kb_lookup_by_command(&r, "fifth", vec![one(chord)]);
        test_resolve(&r, &create_context("{}"), chord, "fifth");
    }

    #[test]
    fn resolve_command_6() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let r = resolver();
        let chord = key_chord(KeyMod::CTRL_CMD | k(KeyCode::KeyK), KeyMod::CTRL_CMD | k(KeyCode::KeyK));
        test_kb_lookup_by_command(&r, "seventh", vec![one(chord)]);
        test_resolve(&r, &create_context("{}"), chord, "seventh");
    }

    #[test]
    fn resolve_command_7() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let r = resolver();
        let chord = key_chord(KeyMod::CTRL_CMD | k(KeyCode::KeyK), KeyMod::CTRL_CMD | k(KeyCode::KeyU));
        test_kb_lookup_by_command(&r, "uncomment lines", vec![one(chord)]);
        test_resolve(&r, &create_context("{}"), chord, "uncomment lines");
    }

    #[test]
    fn resolve_command_8() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let r = resolver();
        let chord = key_chord(KeyMod::CTRL_CMD | k(KeyCode::KeyK), KeyMod::CTRL_CMD | k(KeyCode::KeyC));
        test_kb_lookup_by_command(&r, "comment lines", vec![one(chord)]);
        test_resolve(&r, &create_context("{}"), chord, "comment lines");
    }

    #[test]
    fn resolve_command_9() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_kb_lookup_by_command(&resolver(), "unreachablechord", vec![]);
    }

    #[test]
    fn resolve_command_10() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let r = resolver();
        let key = KeyMod::CTRL_CMD | k(KeyCode::KeyG);
        test_kb_lookup_by_command(&r, "eleven", vec![one(key)]);
        test_resolve(&r, &create_context("{}"), key, "eleven");
    }

    #[test]
    fn resolve_command_11() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_kb_lookup_by_command(&resolver(), "sixth", vec![]);
    }

    #[test]
    fn resolve_command_12() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let r = resolver();
        let chords = [KeyMod::CTRL_CMD | k(KeyCode::KeyK), k(KeyCode::KeyA), k(KeyCode::KeyB)];
        test_kb_lookup_by_command(&r, "long multi chord", vec![many(&chords)]);
        test_resolve(&r, &create_context("{}"), many(&chords), "long multi chord");
    }

    #[test]
    fn kbs_having_common_prefix_the_one_defined_later_is_returned() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let r = resolver();
        let chords = [KeyMod::CTRL_CMD | k(KeyCode::KeyB), KeyMod::CTRL_CMD | k(KeyCode::KeyC), k(KeyCode::KeyI)];
        test_resolve(&r, &create_context("{}"), many(&chords), "long-multi-chord-2");
    }
}
