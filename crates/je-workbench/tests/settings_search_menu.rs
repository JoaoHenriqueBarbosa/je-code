//! Porte de `src/vs/workbench/contrib/preferences/test/browser/settingsSearchMenu.test.ts` (suíte
//! `SettingsSearchFilterDropdownMenuActionViewItem`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo do caso; o `store` é o [`LeakGuard::store`].
//! - O `SuggestEnabledInput` com `mock<...>()` é o [`StubSearchInput`], que implementa o [`ISuggestEnabledInput`]: guarda
//!   `query` e `focused` e devolve `None` no `inputWidget.getContribution()` (o `SuggestController.get` do original).
//! - O `IContextMenuService` vazio (`mock<IContextMenuService>()`) é o [`NoContextMenu`]; o item recebe também o `Ui` da
//!   árvore de widgets nativa, que o rótulo do dropdown exige.
//! - `assert.deepStrictEqual` sobre o objeto literal é o `assert_eq!` sobre o [`Outcome`]; o `checked` de uma ação é
//!   `Option<bool>`, como no `IAction` portado.
//! - O teste é assíncrono: roda em [`run_with_real_timers`], e `await initial.run()` é o `.await` da promessa.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::actions::{Action, ActionRef, Separator};
use je_editor::suggest::suggest_controller::SuggestController;
use je_ui::Ui;
use je_widgets::action_view_items::IActionViewItemOptions;
use je_widgets::context_view_api::{IContextMenuDelegate, IContextMenuProvider};
use je_workbench::contrib::code_editor::suggest_enabled_input::ISuggestEnabledInput;
use je_workbench::contrib::preferences::settings_search_menu::SettingsSearchFilterDropdownMenuActionViewItem;

struct StubSearchInput {
    query: RefCell<String>,
    focused: Cell<u32>,
}

impl ISuggestEnabledInput for StubSearchInput {
    fn get_value(&self) -> String {
        self.query.borrow().clone()
    }

    fn set_value(&self, value: &str) {
        *self.query.borrow_mut() = value.to_owned();
    }

    fn focus(&self) {
        self.focused.set(self.focused.get() + 1);
    }

    fn suggest_controller(&self) -> Option<Rc<SuggestController>> {
        None
    }
}

struct NoContextMenu;

impl IContextMenuProvider for NoContextMenu {
    fn show_context_menu(&self, _delegate: Rc<dyn IContextMenuDelegate>) {}
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    last_group: Vec<String>,
    entries: usize,
    label: String,
    initial: Option<bool>,
    checked: Option<bool>,
    added: String,
    removed: String,
    focused: u32,
}

#[test]
fn groups_exp_assigned_with_advanced_and_toggles_expassigned_without_removing_other_filters() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_real_timers(async move {
        let input = Rc::new(StubSearchInput { query: RefCell::new("@modified @tag:experimental font".to_owned()), focused: Cell::new(0) });
        let ui = Ui::new();
        let menu = store.add_rc(SettingsSearchFilterDropdownMenuActionViewItem::new(
            &ui,
            store.add_rc(Action::new("filter", "Filter", "", true, None)) as ActionRef,
            IActionViewItemOptions::default(),
            None,
            input.clone(),
            Rc::new(NoContextMenu),
        ));
        let actions = menu.get_actions();
        let last_separator = actions.iter().rposition(|action| action.id() == Separator::ID).map_or(0, |index| index + 1);
        let last_group = actions[last_separator..].iter().map(|action| action.label()).collect::<Vec<_>>();
        let assignment_actions = actions.iter().filter(|action| action.id() == "expAssignmentSettingsSearch").cloned().collect::<Vec<_>>();
        let initial = assignment_actions[0].clone();
        initial.run(&[]).await.unwrap();
        let added = input.query.borrow().clone();
        let checked = menu.get_actions().into_iter().find(|action| action.id() == "expAssignmentSettingsSearch").unwrap();
        checked.run(&[]).await.unwrap();

        assert_eq!(
            Outcome {
                last_group,
                entries: assignment_actions.len(),
                label: initial.label(),
                initial: initial.checked(),
                checked: checked.checked(),
                added,
                removed: input.query.borrow().clone(),
                focused: input.focused.get(),
            },
            Outcome {
                last_group: vec!["Advanced".to_owned(), "ExP Assigned".to_owned()],
                entries: 1,
                label: "ExP Assigned".to_owned(),
                initial: Some(false),
                checked: Some(true),
                added: "@modified @tag:experimental font @tag:expassigned".to_owned(),
                removed: "@modified @tag:experimental font".to_owned(),
                focused: 2,
            }
        );
    });
}
