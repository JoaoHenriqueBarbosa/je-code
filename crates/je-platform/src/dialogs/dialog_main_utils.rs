//! Porte de `src/vs/platform/dialogs/electron-main/dialogMainUtils.ts`: ajusta as opções de um diálogo de mensagem às
//! convenções de cada sistema (a ordem dos botões, o botão padrão e o de cancelar).
//!
//! Decisões de tradução:
//! - **`isLinux`, `isMacintosh` e `isWindows`** vêm do [`Platform`] que a função recebe (o `host`).
//! - **`os.release()`** é [`os_release`]: no Linux o `/proc/sys/kernel/osrelease`, no macOS o `uname -r` (a versão do
//!   Darwin) e vazio nos demais, onde ninguém o lê.
//! - **`deepClone(options)`** é a posse das opções, que a função recebe por valor.

use je_base::labels::mnemonic_button_label;
use je_base::platform::Platform;

use super::dialog_main_service::MessageBoxOptions;
use crate::product::IProductService;

/// `IMassagedMessageBoxOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MassagedMessageBoxOptions {
    /// OS massaged message box options.
    pub options: MessageBoxOptions,

    /// Since the massaged result of the message box options potentially
    /// changes the order of buttons, we have to keep a map of these
    /// changes so that we can still return the correct index to the caller.
    pub button_indeces: Vec<usize>,
}

/// `os.release()` no Linux.
#[cfg(target_os = "linux")]
pub fn os_release() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease").map(|release| release.trim().to_owned()).unwrap_or_default()
}

/// `os.release()` no macOS: a versão do Darwin.
#[cfg(target_os = "macos")]
pub fn os_release() -> String {
    std::process::Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|release| release.trim().to_owned())
        .unwrap_or_default()
}

/// `os.release()` nos demais sistemas.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn os_release() -> String {
    String::new()
}

/// `Number.parseInt(os.release(), 10)`: o número inteiro do começo da versão (`24` em `24.1.0`).
pub fn os_release_major() -> Option<i64> {
    let release = os_release();
    let digits: String = release.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// A utility method to ensure the options for the message box dialog
/// are using properties that are consistent across all platforms and
/// specific to the platform where necessary.
pub fn massage_message_box_options(options: MessageBoxOptions, product_service: &dyn IProductService, host: &Platform) -> MassagedMessageBoxOptions {
    let mut massaged_options = options;

    let mut buttons: Vec<String> = massaged_options.buttons.iter().map(|button| mnemonic_button_label(button, host).with_mnemonic).collect();
    let mut button_indeces: Vec<usize> = (0..buttons.len()).collect();

    let mut default_id: i32 = 0; // by default the first button is default button
    let mut cancel_id: i32 = massaged_options.cancel_id.unwrap_or(buttons.len() as i32 - 1); // by default the last button is cancel button
    let use_legacy_mac_os_button_order = host.is_macintosh && os_release_major().is_some_and(|major| major < 24); // macOS 15 is Darwin 24

    // Apply HIG per OS when more than one button is used
    if buttons.len() > 1 {
        let cancel_button: Option<String> = usize::try_from(cancel_id).ok().and_then(|index| buttons.get(index).cloned());

        if host.is_linux || use_legacy_mac_os_button_order {
            // Linux: the GNOME HIG (https://developer.gnome.org/hig/patterns/feedback/dialogs.html?highlight=dialog)
            // recommend the following:
            // "Always ensure that the cancel button appears first, before the affirmative button. In left-to-right
            //  locales, this is on the left. This button order ensures that users become aware of, and are reminded
            //  of, the ability to cancel prior to encountering the affirmative button."
            //
            // Electron APIs do not reorder buttons for us, so we ensure a reverse order of buttons and a position
            // of the cancel button (if provided) that matches the HIG

            // macOS versions before 15 also require the cancel button at index 1 to preserve their native layout.
            if let Some(cancel_button) = &cancel_button
                && buttons.len() > 1
                && cancel_id != 1
            {
                let cancel_index = cancel_id as usize; // `cancel_button` existe, então o índice é válido
                buttons.remove(cancel_index);
                buttons.insert(1, cancel_button.clone());

                let cancel_button_index = button_indeces.remove(cancel_index);
                button_indeces.insert(1, cancel_button_index);

                cancel_id = 1;
            }

            if host.is_linux && buttons.len() > 1 {
                buttons.reverse();
                button_indeces.reverse();

                default_id = buttons.len() as i32 - 1;
                if cancel_button.is_some() {
                    cancel_id = default_id - 1;
                }
            }
        } else if host.is_windows {
            // Windows: the HIG (https://learn.microsoft.com/en-us/windows/win32/uxguide/win-dialog-box)
            // recommend the following:
            // "One of the following sets of concise commands: Yes/No, Yes/No/Cancel, [Do it]/Cancel,
            //  [Do it]/[Don't do it], [Do it]/[Don't do it]/Cancel."
            //
            // Electron APIs do not reorder buttons for us, so we ensure the position of the cancel button
            // (if provided) that matches the HIG

            if let Some(cancel_button) = &cancel_button
                && buttons.len() > 1
                && cancel_id != buttons.len() as i32 - 1 /* last action */
            {
                let cancel_index = cancel_id as usize; // `cancel_button` existe, então o índice é válido
                buttons.remove(cancel_index);
                buttons.push(cancel_button.clone());

                let button_index = button_indeces.remove(cancel_index);
                button_indeces.push(button_index);

                cancel_id = buttons.len() as i32 - 1;
            }
        }
    }

    massaged_options.buttons = buttons;
    massaged_options.default_id = Some(default_id);
    massaged_options.cancel_id = Some(cancel_id);
    massaged_options.no_link = Some(true);
    massaged_options.title = massaged_options.title.filter(|title| !title.is_empty()).or_else(|| Some(product_service.name_long().to_owned()));

    MassagedMessageBoxOptions { options: massaged_options, button_indeces }
}
