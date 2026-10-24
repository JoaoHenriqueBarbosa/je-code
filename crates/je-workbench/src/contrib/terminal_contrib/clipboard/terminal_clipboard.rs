//! Porte de `src/vs/workbench/contrib/terminalContrib/clipboard/browser/terminalClipboard.ts`: a decisão de colar texto de
//! várias linhas no terminal (o aviso de colagem, o corte da quebra de linha final e a colagem em uma linha só).
//!
//! Decisões de tradução:
//! - `ServicesAccessor` (`accessor.get(...)`) são os parâmetros `configuration_service` e `dialog_service` de
//!   [`should_paste_terminal_text`].
//! - `Promise<boolean | { modifiedText: string }>` é um [`Promise`] de [`ShouldPasteTerminalText`].
//! - `dialogService.prompt<{ confirmed, singleLine }>` devolve o resultado dos botões como [`Dynamic`] (o objeto
//!   `{ confirmed, singleLine }`), lido pelas chaves.
//! - `text.split(/\r?\n/)` e `text.replace(/\r?\n/g, '')` são varreduras à mão que casam o mesmo texto; `trim()` e os
//!   comprimentos (`length`, `slice`) contam unidades UTF-16, como o `string` do original.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::js_string::{self, JsString};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::{
    BaseDialogOptions, Checkbox, DialogDetail, DialogType, DialogTypeOrSeverity, IDialogService, Prompt, PromptBaseButton, PromptButton, PromptCancel,
};
use je_platform::terminal::terminal_settings::TerminalSettingId;

/// O resultado de `shouldPasteTerminalText`: `boolean | { modifiedText: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShouldPasteTerminalText {
    /// Cola o texto como está (`true`) ou não cola (`false`).
    Paste(bool),
    /// Cola o texto modificado.
    ModifiedText(String),
}

/// `'auto' | 'always' | 'never'`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MultiLinePasteWarning {
    Auto,
    Always,
    Never,
}

/// `text.split(/\r?\n/)`.
fn split_lines(text: &str) -> Vec<String> {
    let mut parts: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let last = parts.len() - 1;
    for part in &mut parts[..last] {
        if part.ends_with('\r') {
            part.pop();
        }
    }
    parts
}

/// `parseConfigValue(value)`.
fn parse_config_value(value: &Dynamic) -> MultiLinePasteWarning {
    match value {
        // Valid value
        Dynamic::String(text) if *text == "auto" => MultiLinePasteWarning::Auto,
        Dynamic::String(text) if *text == "always" => MultiLinePasteWarning::Always,
        Dynamic::String(text) if *text == "never" => MultiLinePasteWarning::Never,
        // Legacy backwards compatibility
        Dynamic::Bool(true) => MultiLinePasteWarning::Auto,
        Dynamic::Bool(false) => MultiLinePasteWarning::Never,
        // Invalid value fallback
        _ => MultiLinePasteWarning::Auto,
    }
}

/// `{ confirmed, singleLine }` de um botão do diálogo.
fn button_result(confirmed: bool, single_line: bool) -> MaybePromise<Dynamic> {
    MaybePromise::Value(Dynamic::object([("confirmed", Dynamic::Bool(confirmed)), ("singleLine", Dynamic::Bool(single_line))]))
}

pub fn should_paste_terminal_text(
    configuration_service: Rc<dyn IConfigurationService>,
    dialog_service: Rc<dyn IDialogService>,
    text: &str,
    bracketed_paste_mode: Option<bool>,
) -> Promise<ShouldPasteTerminalText> {
    let text = text.to_owned();
    async_fn(async move {
        // If the clipboard has only one line, a warning should never show
        let text_for_lines = split_lines(&text);
        if text_for_lines.len() == 1 {
            return Ok(ShouldPasteTerminalText::Paste(true));
        }

        // Get config value
        let config_value = parse_config_value(&configuration_service.get_value(Some(TerminalSettingId::EnableMultiLinePasteWarning.as_str()), None));

        // Never show it
        if config_value == MultiLinePasteWarning::Never {
            return Ok(ShouldPasteTerminalText::Paste(true));
        }

        // Special edge cases to not show for auto
        if config_value == MultiLinePasteWarning::Auto {
            // Ignore check if the shell is in bracketed paste mode (ie. the shell can handle multi-line
            // text).
            if bracketed_paste_mode == Some(true) {
                return Ok(ShouldPasteTerminalText::Paste(true));
            }

            // When a command is copied with a trailing new line, strip the trailing newline so the
            // command is not automatically executed on paste. This mitigates a clipboard hijacking
            // scenario where a malicious page replaces clipboard contents with a command followed by
            // a newline; pasting (especially via right click) would otherwise immediately execute it.
            // The user can still review the pasted text and press Enter to run it.
            if text_for_lines.len() == 2 && js_string::trim(JsString::from(text_for_lines[1].as_str()).units()).is_empty() {
                return Ok(ShouldPasteTerminalText::ModifiedText(text_for_lines[0].clone()));
            }
        }

        let display_items_count = 3;
        let max_preview_line_length = 30;

        let mut detail = localize("preview", "Preview:", &[]);
        for line in text_for_lines.iter().take(display_items_count) {
            let line_text = JsString::from(line.as_str());
            let cleaned_line = if line_text.len() > max_preview_line_length {
                format!("{}…", line_text.slice(0, max_preview_line_length).to_string_lossy())
            } else {
                line.clone()
            };
            detail.push_str(&format!("\n{cleaned_line}"));
        }

        if text_for_lines.len() > display_items_count {
            detail.push_str("\n…");
        }

        let prompt_result = dialog_service
            .prompt(Prompt {
                base: BaseDialogOptions {
                    dialog_type: Some(DialogTypeOrSeverity::Type(DialogType::Warning)),
                    detail: Some(DialogDetail::Text(detail)),
                    checkbox: Some(Checkbox { label: localize("doNotAskAgain", "Do not ask me again", &[]), checked: None }),
                    ..BaseDialogOptions::new(localize(
                        "confirmMoveTrashMessageFilesAndDirectories",
                        "Are you sure you want to paste {0} lines of text into the terminal?",
                        &[&text_for_lines.len().to_string()],
                    ))
                },
                buttons: Some(vec![
                    PromptButton {
                        label: localize("multiLinePasteButton", "&&Paste", &[]),
                        base: PromptBaseButton { run: Rc::new(|_| button_result(true, false)) },
                    },
                    PromptButton {
                        label: localize("multiLinePasteButton.oneLine", "Paste as &&one line", &[]),
                        base: PromptBaseButton { run: Rc::new(|_| button_result(true, true)) },
                    },
                ]),
                cancel_button: Some(PromptCancel::Default),
            })
            .await?;

        let Some(result) = prompt_result.result else {
            return Ok(ShouldPasteTerminalText::Paste(false));
        };
        if result.is_nullish() {
            return Ok(ShouldPasteTerminalText::Paste(false));
        }
        let confirmed = result.get_str("confirmed").is_truthy();

        if confirmed && prompt_result.checkbox_checked == Some(true) {
            configuration_service.update_value(TerminalSettingId::EnableMultiLinePasteWarning.as_str(), Dynamic::string("never"), None, None).await?;
        }

        if result.get_str("singleLine").is_truthy() {
            return Ok(ShouldPasteTerminalText::ModifiedText(text.replace("\r\n", "").replace('\n', "")));
        }

        Ok(ShouldPasteTerminalText::Paste(confirmed))
    })
}
