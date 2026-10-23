//! Casos que NÃO são do original: o `dialogs.ts` não tem `.test.ts` em `common` (os do diretório são do `electron-main`
//! e do `electron-browser`, que não foram portados). Cobrem a lógica comum do `AbstractDialogHandler` e o
//! `getFileNamesMessage`, com os valores que o código do original produz.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::*;
use je_base::dynamic::Dynamic;
use je_base::promise::MaybePromise;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_platform::dialogs::{
    AbstractDialogHandler, BaseDialogOptions, Confirmation, DialogType, DialogTypeOrSeverity, FileNameOrResource, Input, Prompt, PromptBaseButton, PromptButton,
    PromptCancel, PromptCancelButton, get_file_names_message,
};

fn confirmation(primary: Option<&str>, cancel: Option<&str>) -> Confirmation {
    Confirmation { base: BaseDialogOptions::new("m"), primary_button: primary.map(Into::into), cancel_button: cancel.map(Into::into) }
}

fn button(label: &str, value: f64, ran: &Rc<Cell<u32>>) -> PromptButton {
    let ran = ran.clone();
    PromptButton {
        label: label.to_string(),
        base: PromptBaseButton {
            run: Rc::new(move |_| {
                ran.set(ran.get() + 1);
                MaybePromise::Value(Dynamic::Number(value))
            }),
        },
    }
}

#[test]
fn confirmation_and_input_buttons_default_to_yes_ok_and_cancel() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(AbstractDialogHandler::get_confirmation_buttons(&confirmation(None, None)), vec!["&&Yes", "Cancel"]);
    assert_eq!(AbstractDialogHandler::get_confirmation_buttons(&confirmation(Some("Go"), Some("Stop"))), vec!["Go", "Stop"]);
    let input = Input { confirmation: confirmation(None, None), inputs: vec![] };
    assert_eq!(AbstractDialogHandler::get_input_buttons(&input), vec!["&&OK", "Cancel"]);
}

#[test]
fn prompt_buttons_put_the_cancel_button_last_and_fall_back_to_ok() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ran = Rc::new(Cell::new(0));
    let mut prompt = Prompt { base: BaseDialogOptions::new("m"), buttons: Some(vec![button("A", 1.0, &ran), button("B", 2.0, &ran)]), cancel_button: None };
    assert_eq!(AbstractDialogHandler::get_prompt_buttons(&prompt), vec!["A", "B"]);

    prompt.cancel_button = Some(PromptCancel::Default);
    assert_eq!(AbstractDialogHandler::get_prompt_buttons(&prompt), vec!["A", "B", "Cancel"]);
    prompt.cancel_button = Some(PromptCancel::Label("Never".into()));
    assert_eq!(AbstractDialogHandler::get_prompt_buttons(&prompt), vec!["A", "B", "Never"]);
    prompt.cancel_button = Some(PromptCancel::Button(PromptCancelButton { label: None, base: button("x", 3.0, &ran).base }));
    assert_eq!(AbstractDialogHandler::get_prompt_buttons(&prompt), vec!["A", "B", "Cancel"]);

    let empty = Prompt { base: BaseDialogOptions::new("m"), buttons: None, cancel_button: None };
    assert_eq!(AbstractDialogHandler::get_prompt_buttons(&empty), vec!["&&OK"]);
}

#[test]
fn prompt_result_runs_the_chosen_button_including_the_custom_cancel() {
    run_with_faked_timers(async {
        let ran = Rc::new(Cell::new(0));
        let prompt = Prompt {
            base: BaseDialogOptions::new("m"),
            buttons: Some(vec![button("A", 1.0, &ran), button("B", 2.0, &ran)]),
            cancel_button: Some(PromptCancel::Button(PromptCancelButton { label: None, base: button("c", 9.0, &ran).base })),
        };
        let second = AbstractDialogHandler::get_prompt_result(&prompt, 1, Some(true));
        assert_eq!(second.checkbox_checked, Some(true));
        assert_eq!(second.result.unwrap().await.unwrap(), Dynamic::Number(2.0));
        let cancel = AbstractDialogHandler::get_prompt_result(&prompt, 2, None);
        assert_eq!(cancel.result.unwrap().await.unwrap(), Dynamic::Number(9.0));
        assert_eq!(ran.get(), 2);
    });
}

#[test]
fn dialog_type_maps_severity() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let map = |t| AbstractDialogHandler::get_dialog_type(Some(t));
    assert_eq!(map(DialogTypeOrSeverity::Severity(Severity::Info)), Some(DialogType::Info));
    assert_eq!(map(DialogTypeOrSeverity::Severity(Severity::Error)), Some(DialogType::Error));
    assert_eq!(map(DialogTypeOrSeverity::Severity(Severity::Warning)), Some(DialogType::Warning));
    assert_eq!(map(DialogTypeOrSeverity::Severity(Severity::Ignore)), Some(DialogType::None));
    assert_eq!(map(DialogTypeOrSeverity::Type(DialogType::Question)), Some(DialogType::Question));
    assert_eq!(AbstractDialogHandler::get_dialog_type(None), None);
}

#[test]
fn get_file_names_message_truncates_after_ten() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let names = |n: usize| (0..n).map(|i| FileNameOrResource::Name(format!("f{i}"))).collect::<Vec<_>>();
    assert_eq!(get_file_names_message(&names(2)), "f0\nf1\n");
    assert_eq!(get_file_names_message(&names(11)), "f0\nf1\nf2\nf3\nf4\nf5\nf6\nf7\nf8\nf9\n...1 additional file not shown\n");
    assert!(get_file_names_message(&names(13)).ends_with("...3 additional files not shown\n"));
    let resource = FileNameOrResource::Resource(URI::parse("file:///a/b/c.txt", false));
    assert_eq!(get_file_names_message(&[resource]), "c.txt\n");
}
