//! Porte de `src/vs/editor/contrib/stickyScroll/browser/stickyScrollActions.ts`: as seis ações do sticky scroll
//! (`ToggleStickyScroll`, `FocusStickyScroll`, `SelectNextStickyScrollLine`, `SelectPreviousStickyScrollLine`,
//! `GoToStickyScrollLine` e `SelectEditor`).
//!
//! Representação:
//! - `class X extends EditorAction2` é o trait [`EditorAction2`]. [`ToggleStickyScroll`] e [`FocusStickyScroll`] são structs
//!   próprias. As quatro ações que valem só com o foco no sticky scroll (`SelectNextStickyScrollLine`,
//!   `SelectPreviousStickyScrollLine`, `GoToStickyScrollLine` e `SelectEditor`) diferem apenas no id, no título, na tecla e
//!   no método do controlador que chamam, então são uma struct só, [`StickyScrollFocusedAction`], com o
//!   [`StickyScrollFocusedActionKind`] como dado (o mesmo molde de `FoldingAction`).
//! - `StickyScrollController.get(editor)?.m()` é `StickyScrollController::get(editor).map(|controller| controller.m())`.
//! - `title: { ...localize2(key, msg), mnemonicTitle: localize({ key, comment }, msg) }` é um `Title::Localized` com o
//!   `mnemonic_title` (`localized_title_with_mnemonic` de `editor_extensions`); `title: localize2(...)` é um
//!   `Title::Localized` sem ele (`localized_title`).
//! - `configurationService.getValue('editor.stickyScroll.enabled')` e `updateValue(...)` são os do
//!   `IConfigurationService` de `je_platform`; a promessa de `updateValue` não é aguardada, como no original.

use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};
use je_base::key_codes::KeyCode;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_instantiation::ServicesAccessor;
use je_platform::action::action_common_categories::Categories;
use je_platform::action::{Category, CommandAction, CommandActionToggleInfo, Toggled, localize2};
use je_platform::actions::menu_ids::{command_palette, menubar_appearance_menu, sticky_scroll_context};
use je_platform::actions::{Action2Keybinding, Action2Options};
use je_platform::commands::{CommandDescription, CommandMetadata};
use je_platform::configuration::service::I_CONFIGURATION_SERVICE;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};

use super::sticky_scroll_controller::{IStickyScrollController, StickyScrollController};
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::editor_context_keys::{sticky_scroll_focused, sticky_scroll_visible};
use crate::editor_extensions::{EditorAction2, action2_menu_item as menu_item, localized_title, localized_title_with_mnemonic};

/// `EditorContextKeys.stickyScrollFocused.isEqualTo(true)`.
fn sticky_scroll_is_focused() -> ContextKeyExpression {
    sticky_scroll_focused().is_equal_to(true)
}

/// `ToggleStickyScroll`.
pub struct ToggleStickyScroll;

impl EditorAction2 for ToggleStickyScroll {
    fn desc(&self) -> Action2Options {
        let toggled = CommandActionToggleInfo {
            condition: ContextKeyExpr::equals("config.editor.stickyScroll.enabled", true),
            icon: None,
            tooltip: None,
            title: Some(localize("stickyScroll", "Sticky Scroll", &[])),
            mnemonic_title: Some(localize("miStickyScroll", "&&Sticky Scroll", &[])),
        };
        Action2Options {
            command: CommandAction {
                metadata: Some(CommandMetadata::new(CommandDescription::Localized(localize2(
                    "toggleEditorStickyScroll.description",
                    "Toggle/enable the editor sticky scroll which shows the nested scopes at the top of the viewport",
                )))),
                category: Some(Category::Localized(Categories::new().view)),
                toggled: Some(Toggled::Info(toggled)),
                ..CommandAction::new(
                    "editor.action.toggleStickyScroll",
                    localized_title_with_mnemonic(
                        "toggleEditorStickyScroll",
                        "Toggle Editor Sticky Scroll",
                        "mitoggleStickyScroll",
                        "&&Toggle Editor Sticky Scroll",
                    ),
                )
            },
            menu: vec![
                menu_item(command_palette(), None, None),
                menu_item(menubar_appearance_menu(), Some("4_editor"), Some(3.0)),
                menu_item(sticky_scroll_context(), None, None),
            ],
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run_editor_command(&self, accessor: &ServicesAccessor, editor: &Rc<CodeEditorWidget>, _args: &[Dynamic]) -> Promise<()> {
        let configuration_service = accessor.get(I_CONFIGURATION_SERVICE);
        let new_value = !configuration_service.get_value(Some("editor.stickyScroll.enabled"), None).is_truthy();
        let is_focused = StickyScrollController::get(editor).map(|controller| controller.is_focused());
        let _ = configuration_service.update_value("editor.stickyScroll.enabled", Dynamic::Bool(new_value), None, None);
        if is_focused == Some(true) {
            editor.focus();
        }
        Promise::resolved(())
    }
}

/// `FocusStickyScroll`.
pub struct FocusStickyScroll;

impl EditorAction2 for FocusStickyScroll {
    fn desc(&self) -> Action2Options {
        Action2Options {
            command: CommandAction {
                precondition: ContextKeyExpr::and([
                    Some(ContextKeyExpr::has("config.editor.stickyScroll.enabled")),
                    Some(sticky_scroll_visible().expr()),
                ]),
                ..CommandAction::new(
                    "editor.action.focusStickyScroll",
                    localized_title_with_mnemonic(
                        "focusStickyScroll",
                        "Focus Editor Sticky Scroll",
                        "mifocusEditorStickyScroll",
                        "&&Focus Editor Sticky Scroll",
                    ),
                )
            },
            menu: vec![menu_item(command_palette(), None, None)],
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run_editor_command(&self, _accessor: &ServicesAccessor, editor: &Rc<CodeEditorWidget>, _args: &[Dynamic]) -> Promise<()> {
        if let Some(controller) = StickyScrollController::get(editor) {
            controller.focus();
        }
        Promise::resolved(())
    }
}

/// `SelectNextStickyScrollLine`, `SelectPreviousStickyScrollLine`, `GoToStickyScrollLine` e `SelectEditor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StickyScrollFocusedActionKind {
    SelectNextStickyScrollLine,
    SelectPreviousStickyScrollLine,
    GoToStickyScrollLine,
    SelectEditor,
}

impl StickyScrollFocusedActionKind {
    /// As quatro ações, na ordem em que o original as registra (a de `SelectEditor` por último).
    pub const REGISTERED: [StickyScrollFocusedActionKind; 4] = [
        StickyScrollFocusedActionKind::SelectPreviousStickyScrollLine,
        StickyScrollFocusedActionKind::SelectNextStickyScrollLine,
        StickyScrollFocusedActionKind::GoToStickyScrollLine,
        StickyScrollFocusedActionKind::SelectEditor,
    ];

    /// O id do comando, a chave e o texto do título e a tecla.
    fn id_title_and_key(self) -> (&'static str, &'static str, &'static str, KeyCode) {
        match self {
            Self::SelectNextStickyScrollLine => (
                "editor.action.selectNextStickyScrollLine",
                "selectNextStickyScrollLine.title",
                "Select the next editor sticky scroll line",
                KeyCode::DownArrow,
            ),
            Self::SelectPreviousStickyScrollLine => (
                "editor.action.selectPreviousStickyScrollLine",
                "selectPreviousStickyScrollLine.title",
                "Select the previous sticky scroll line",
                KeyCode::UpArrow,
            ),
            Self::GoToStickyScrollLine => (
                "editor.action.goToFocusedStickyScrollLine",
                "goToFocusedStickyScrollLine.title",
                "Go to the focused sticky scroll line",
                KeyCode::Enter,
            ),
            Self::SelectEditor => ("editor.action.selectEditor", "selectEditor.title", "Select Editor", KeyCode::Escape),
        }
    }
}

/// A ação de uma [`StickyScrollFocusedActionKind`].
pub struct StickyScrollFocusedAction {
    pub kind: StickyScrollFocusedActionKind,
}

impl EditorAction2 for StickyScrollFocusedAction {
    fn desc(&self) -> Action2Options {
        let (id, title_key, title, key) = self.kind.id_title_and_key();
        Action2Options {
            command: CommandAction { precondition: Some(sticky_scroll_is_focused()), ..CommandAction::new(id, localized_title(title_key, title)) },
            menu: Vec::new(),
            keybinding: vec![Action2Keybinding {
                keybindings: Keybindings { primary: Some(key.bits()), ..Keybindings::default() },
                weight: KeybindingWeight::EDITOR_CONTRIB,
                args: Dynamic::Undefined,
                when: None,
            }],
            f1: false,
        }
    }

    fn run_editor_command(&self, _accessor: &ServicesAccessor, editor: &Rc<CodeEditorWidget>, _args: &[Dynamic]) -> Promise<()> {
        if let Some(controller) = StickyScrollController::get(editor) {
            match self.kind {
                StickyScrollFocusedActionKind::SelectNextStickyScrollLine => controller.focus_next(),
                StickyScrollFocusedActionKind::SelectPreviousStickyScrollLine => controller.focus_previous(),
                StickyScrollFocusedActionKind::GoToStickyScrollLine => controller.go_to_focused(),
                StickyScrollFocusedActionKind::SelectEditor => controller.select_editor(),
            }
        }
        Promise::resolved(())
    }
}
