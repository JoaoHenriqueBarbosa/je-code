//! Testes de `src/vs/base/common/actions.ts`, que não tem `.test.ts` próprio no VS Code. Cobrem o que o
//! módulo promete: `Action` (eventos de mudança, `run`), `ActionRunner` (ordem dos eventos, erro,
//! ação desabilitada), `Separator.join`/`clean`, `SubmenuAction`, `EmptySubmenuAction`, `toAction` e
//! `getActionForAltMode`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::actions::{
    Action, ActionCallback, ActionRef, ActionRunner, AltModeModifiers, EmptySubmenuAction, IAction, IActionChangeEvent, IActionRunner, Separator, SubmenuAction,
    ToActionProps, get_action_for_alt_mode, to_action,
};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::platform::Platform;
use je_base::promise::MaybePromise;

/// Uma ação simples e sem recursos (a `Action` é um `Disposable` e precisaria ser descartada).
fn action(id: &str) -> ActionRef {
    to_action(ToActionProps {
        id: id.into(),
        label: id.into(),
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(|_| MaybePromise::Value(Dynamic::Undefined)),
    })
}

fn ids(actions: &[ActionRef]) -> Vec<String> {
    actions.iter().map(|a| a.id()).collect()
}

fn sep() -> ActionRef {
    Separator::new()
}

#[test]
fn action_defaults_and_change_events() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let a = ds.add_rc(Action::new("id", "Label", "", true, None));
    let events: Rc<RefCell<Vec<IActionChangeEvent>>> = Rc::default();
    let sink = events.clone();
    ds.add(a.on_did_change().listen(move |e| sink.borrow_mut().push(e.clone())));

    assert_eq!((a.id(), a.label(), a.tooltip(), a.class(), a.enabled(), a.checked()), ("id".into(), "Label".into(), String::new(), Some(String::new()), true, None));

    a.set_label("Label".into());
    a.set_label("Other".into());
    a.set_tooltip("tip".into());
    a.set_class(Some("c".into()));
    a.set_enabled(false);
    a.set_enabled(false);
    a.set_checked(Some(true));

    let events = events.borrow();
    assert_eq!(events.len(), 5);
    assert_eq!(events[0], IActionChangeEvent { label: Some("Other".into()), ..Default::default() });
    assert_eq!(events[1].tooltip.as_deref(), Some("tip"));
    assert_eq!(events[2].class.as_deref(), Some("c"));
    assert_eq!(events[3].enabled, Some(false));
    assert_eq!(events[4].checked, Some(true));
}

#[test]
fn action_run_calls_the_callback_with_the_event_and_waits_for_it() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let ds = Rc::new(ds);
    run_with_faked_timers(async move {
        let received: Rc<RefCell<Option<Dynamic>>> = Rc::default();
        let sink = received.clone();
        let callback: ActionCallback = Rc::new(move |event| {
            *sink.borrow_mut() = Some(event);
            MaybePromise::Value(Dynamic::Undefined)
        });
        let a = ds.add_rc(Action::new("id", "", "", true, Some(callback)));

        a.run(&[Dynamic::string("ctx")]).await.unwrap();

        assert_eq!(*received.borrow(), Some(Dynamic::string("ctx")));
        // sem callback, `run` resolve
        action("x").run(&[]).await.unwrap();
    });
}

#[test]
fn action_runner_fires_will_run_then_did_run_and_reports_the_error() {
    let ds = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let runner = ds.add_rc(ActionRunner::new());
        let log: Rc<RefCell<Vec<String>>> = Rc::default();
        let (will, did) = (log.clone(), log.clone());
        ds.add(runner.on_will_run().listen(move |e| will.borrow_mut().push(format!("will {}", e.action.id()))));
        ds.add(runner.on_did_run().listen(move |e| did.borrow_mut().push(format!("did {} {:?}", e.action.id(), e.error.as_ref().map(|e| e.message.clone())))));

        let ok = action("ok");
        runner.run(&ok, None).await.unwrap();

        let fails = ds.add_rc(Action::new("fails", "", "", true, Some(Rc::new(|_| errors::throw(Error::new("boom"))))));
        let fails: ActionRef = fails;
        runner.run(&fails, None).await.unwrap();

        let disabled = action("disabled");
        disabled.set_enabled(false);
        runner.run(&disabled, None).await.unwrap();

        assert_eq!(*log.borrow(), vec!["will ok", "did ok None", "will fails", "did fails Some(\"boom\")"]);
    });
}

#[test]
fn action_runner_with_run_action_overrides_the_execution() {
    let ds = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let ran = Rc::new(Cell::new(0));
        let counter = ran.clone();
        let runner = ds.add_rc(ActionRunner::with_run_action(Rc::new(move |_, _| {
            counter.set(counter.get() + 1);
            je_base::promise::Promise::resolved(())
        })));

        runner.run(&action("a"), None).await.unwrap();

        assert_eq!(ran.get(), 1);
    });
}

#[test]
fn separator_join_skips_empty_lists_and_separates_the_rest() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let joined = Separator::join(&[vec![], vec![action("a")], vec![], vec![action("b"), action("c")], vec![action("d")]]);

    assert_eq!(ids(&joined), vec!["a", Separator::ID, "b", "c", Separator::ID, "d"]);
    assert!(Separator::join(&[vec![], vec![]]).is_empty());
}

#[test]
fn separator_clean_removes_leading_trailing_and_duplicate_separators() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut actions = vec![sep(), sep(), action("a"), sep(), sep(), sep(), action("b"), sep()];

    Separator::clean(&mut actions);

    assert_eq!(ids(&actions), vec!["a", Separator::ID, "b"]);
    let mut only_separators = vec![sep(), sep()];
    assert!(Separator::clean(&mut only_separators).is_empty());
}

#[test]
fn separator_has_the_fixed_properties() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let s: ActionRef = Separator::new();

    assert_eq!((s.id().as_str(), s.label().as_str(), s.class().as_deref(), s.enabled(), s.checked()), ("vs.actions.separator", "", Some("separator"), false, None));
    assert!(s.as_any().is::<Separator>());
}

#[test]
fn submenu_action_keeps_its_actions_and_is_enabled() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let submenu = SubmenuAction::new("sub", "Sub", vec![action("a"), action("b")], Some("css"));

    assert_eq!(ids(submenu.actions()), vec!["a", "b"]);
    assert_eq!((submenu.enabled(), submenu.class(), submenu.tooltip()), (true, Some("css".to_owned()), String::new()));
}

#[test]
fn empty_submenu_action_is_a_disabled_empty_label() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let empty = ds.add_rc(EmptySubmenuAction::create());

    assert_eq!((empty.id().as_str(), empty.label().as_str(), empty.enabled()), ("vs.actions.empty", "(empty)", false));
}

#[test]
fn to_action_fills_the_defaults_and_runs_the_callback() {
    let ds = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let _ = &ds;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sink = calls.clone();
        let a = to_action(ToActionProps {
            id: "id".into(),
            label: "Label".into(),
            tooltip: None,
            enabled: None,
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |args| {
                sink.borrow_mut().push(args.to_vec());
                MaybePromise::Value(Dynamic::string("result"))
            }),
        });

        assert_eq!((a.tooltip().as_str(), a.enabled(), a.class(), a.checked(), a.alt().is_none()), ("Label", true, None, None, true));
        assert_eq!(a.run(&[Dynamic::from(1), Dynamic::from(2)]).await.unwrap(), Dynamic::string("result"));
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(calls.borrow()[0], vec![Dynamic::from(1), Dynamic::from(2)]);
    });
}

#[test]
fn get_action_for_alt_mode_picks_the_alternative_when_allowed() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let alt = action("alt");
    let main = to_action(ToActionProps {
        id: "main".into(),
        label: "main".into(),
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: Some(alt.clone()),
        run: Rc::new(|_| MaybePromise::Value(Dynamic::Undefined)),
    });
    let linux = Platform { is_linux: true, ..Platform::detect() };
    let mac = Platform { is_linux: false, is_windows: false, is_macintosh: true, ..Platform::detect() };
    let pick = |modifiers: AltModeModifiers, host: &Platform| get_action_for_alt_mode(&main, modifiers, host).id();

    assert_eq!(pick(AltModeModifiers { alt_key: true, shift_key: false }, &mac), "alt");
    assert_eq!(pick(AltModeModifiers { alt_key: false, shift_key: true }, &mac), "main");
    assert_eq!(pick(AltModeModifiers { alt_key: false, shift_key: true }, &linux), "alt");
    assert_eq!(pick(AltModeModifiers::default(), &linux), "main");

    alt.set_enabled(false);
    assert_eq!(pick(AltModeModifiers { alt_key: true, shift_key: false }, &mac), "main");
}
