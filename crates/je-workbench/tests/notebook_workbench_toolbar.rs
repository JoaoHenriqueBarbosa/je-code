//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookWorkbenchToolbar.test.ts` (suítes `Workbench Toolbar calculateActions (strategy always + never)` e `Workbench Toolbar Dynamic calculateActions (strategy dynamic)`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso, depois das ações que o
//!   original cria na definição da suíte (fora do rastreio de vazamentos); o `setup` que as registra no store é
//!   [`setup`]. As ações de dentro de cada caso são registradas com `disposables.add_rc`.
//! - `assert.deepEqual` entre ações e modelos compara a forma estrutural ([`action_key`]: `id`, `label` e se é
//!   `Separator`) mais `size`, `visible` e `renderLabel` ([`assert_models_eq`]); duas `new Separator()` são iguais.
//! - As funções recebem os modelos por valor (o original muta os mesmos objetos), então cada chamada recebe um clone.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::actions::{Action, ActionRef, IAction, Separator};
use je_workbench::contrib::notebook::browser::view_parts::notebook_editor_toolbar::{
    IActionModel, workbench_calculate_actions, workbench_dynamic_calculate_actions,
};

fn new_action(id: &str, label: &str) -> Rc<Action> {
    Action::new(id, label, "", true, None)
}

fn model<A: IAction + 'static>(action: &Rc<A>, size: f64) -> IActionModel {
    IActionModel { action: action.clone(), size, visible: true, render_label: true }
}

fn separator_model() -> IActionModel {
    IActionModel { action: Separator::new(), size: 1.0, visible: true, render_label: true }
}

fn action_key(action: &ActionRef) -> (String, String, bool) {
    (action.id(), action.label(), action.as_any().is::<Separator>())
}

fn assert_actions_eq(actual: &[ActionRef], expected: &[ActionRef]) {
    let actual: Vec<_> = actual.iter().map(action_key).collect();
    let expected: Vec<_> = expected.iter().map(action_key).collect();
    assert_eq!(actual, expected);
}

fn assert_models_eq(actual: &[IActionModel], expected: &[IActionModel]) {
    let key = |m: &IActionModel| (action_key(&m.action), m.size, m.visible, m.render_label);
    let actual: Vec<_> = actual.iter().map(key).collect();
    let expected: Vec<_> = expected.iter().map(key).collect();
    assert_eq!(actual, expected);
}

/// `defaultSecondaryActionModels` (criados antes do rastreio) e o `setup` que os registra no store.
fn setup() -> (Vec<IActionModel>, LeakGuard) {
    let actions: Vec<Rc<Action>> = (0..3).map(|i| new_action(&format!("secondaryAction{i}"), &format!("Secondary Action {i}"))).collect();
    let secondary: Vec<IActionModel> = actions.iter().map(|action| model(action, 50.0)).collect();
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    for action in &actions {
        add_action(&disposables, action);
    }
    (secondary, disposables)
}

/// `disposables.add(new Action(...))`: registra a ação e a devolve.
fn add_action(disposables: &LeakGuard, action: &Rc<Action>) -> Rc<Action> {
    disposables.add_rc(action.clone())
}

fn secondary_actions(models: &[IActionModel]) -> Vec<ActionRef> {
    models.iter().map(|m| m.action.clone()).collect()
}

/// As três ações e os tamanhos 50 de `action0`, `action1`, `action2` registradas no store.
fn three_actions(disposables: &LeakGuard) -> Vec<IActionModel> {
    (0..3).map(|i| model(&add_action(disposables, &new_action(&format!("action{i}"), &format!("Action {i}"))), 50.0)).collect()
}

mod workbench_toolbar_calculate_actions_strategy_always_and_never {
    use super::*;

    #[test]
    fn should_return_empty_primary_and_secondary_actions_when_given_empty_initial_actions() {
        let (_secondary_models, _disposables) = setup();
        let result = workbench_calculate_actions(Vec::new(), Vec::new(), 100.0);
        assert_models_eq(&result.primary_actions, &[]);
        assert_actions_eq(&result.secondary_actions, &[]);
    }

    #[test]
    fn should_return_all_primary_actions_when_they_fit_within_the_container_width() {
        let (secondary_models, disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let actions = three_actions(&disposables);
        let result = workbench_calculate_actions(actions.clone(), default_secondary_actions.clone(), 200.0);
        assert_models_eq(&result.primary_actions, &actions);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }

    #[test]
    fn should_move_actions_to_secondary_when_they_do_not_fit_within_the_container_width() {
        let (secondary_models, disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let actions = three_actions(&disposables);
        let result = workbench_calculate_actions(actions.clone(), default_secondary_actions.clone(), 100.0);
        assert_models_eq(&result.primary_actions, &[actions[0].clone()]);
        let mut expected = vec![actions[1].action.clone(), actions[2].action.clone(), Separator::new()];
        expected.extend(default_secondary_actions);
        assert_actions_eq(&result.secondary_actions, &expected);
    }

    #[test]
    fn should_ignore_second_separator_when_two_separators_are_in_a_row() {
        let (secondary_models, disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let actions = vec![
            model(&add_action(&disposables, &new_action("action0", "Action 0")), 50.0),
            separator_model(),
            separator_model(),
            model(&add_action(&disposables, &new_action("action1", "Action 1")), 50.0),
        ];
        let result = workbench_calculate_actions(actions.clone(), default_secondary_actions.clone(), 125.0);
        assert_models_eq(&result.primary_actions, &[actions[0].clone(), actions[1].clone(), actions[3].clone()]);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }

    #[test]
    fn should_ignore_separators_when_they_are_at_the_end_of_the_resulting_primary_actions() {
        let (secondary_models, disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let actions = vec![
            model(&add_action(&disposables, &new_action("action0", "Action 0")), 50.0),
            separator_model(),
            model(&add_action(&disposables, &new_action("action1", "Action 1")), 50.0),
            separator_model(),
        ];
        let result = workbench_calculate_actions(actions.clone(), default_secondary_actions.clone(), 200.0);
        assert_models_eq(&result.primary_actions, &[actions[0].clone(), actions[1].clone(), actions[2].clone()]);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }

    #[test]
    fn should_keep_actions_with_size_0_in_primary_actions() {
        let (secondary_models, disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let mut actions = three_actions(&disposables);
        actions.push(model(&add_action(&disposables, &new_action("action3", "Action 3")), 0.0));
        let result = workbench_calculate_actions(actions.clone(), default_secondary_actions.clone(), 116.0);
        assert_models_eq(&result.primary_actions, &[actions[0].clone(), actions[1].clone(), actions[3].clone()]);
        let mut expected = vec![actions[2].action.clone(), Separator::new()];
        expected.extend(default_secondary_actions);
        assert_actions_eq(&result.secondary_actions, &expected);
    }

    #[test]
    fn should_not_render_separator_if_preceeded_by_size_0_action_s() {
        let (secondary_models, disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let actions = vec![
            model(&add_action(&disposables, &new_action("action0", "Action 0")), 0.0),
            separator_model(),
            model(&add_action(&disposables, &new_action("action1", "Action 1")), 50.0),
        ];
        let result = workbench_calculate_actions(actions.clone(), default_secondary_actions.clone(), 116.0);
        assert_models_eq(&result.primary_actions, &[actions[0].clone(), actions[2].clone()]);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }

    #[test]
    fn should_not_render_second_separator_if_space_between_is_hidden_size_0_actions() {
        let (secondary_models, disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let actions = vec![
            model(&add_action(&disposables, &new_action("action0", "Action 0")), 50.0),
            separator_model(),
            model(&add_action(&disposables, &new_action("action1", "Action 1")), 0.0),
            model(&add_action(&disposables, &new_action("action2", "Action 2")), 0.0),
            separator_model(),
            model(&add_action(&disposables, &new_action("action3", "Action 3")), 50.0),
        ];
        let result = workbench_calculate_actions(actions.clone(), default_secondary_actions.clone(), 300.0);
        assert_models_eq(
            &result.primary_actions,
            &[actions[0].clone(), actions[1].clone(), actions[2].clone(), actions[3].clone(), actions[5].clone()],
        );
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }
}

mod workbench_toolbar_dynamic_calculate_actions_strategy_dynamic {
    use super::*;

    /// `actionTemplate`: criado na definição da suíte, fora do rastreio de vazamentos.
    fn action_template() -> Vec<Rc<Action>> {
        (0..4).map(|i| new_action(&format!("action{i}"), &format!("Action {i}"))).collect()
    }

    fn with_label(mut m: IActionModel, render_label: bool) -> IActionModel {
        m.render_label = render_label;
        m
    }

    #[test]
    fn should_return_empty_primary_and_secondary_actions_when_given_empty_initial_actions() {
        let (_secondary_models, _disposables) = setup();
        let result = workbench_dynamic_calculate_actions(Vec::new(), Vec::new(), 100.0);
        assert_models_eq(&result.primary_actions, &[]);
        assert_actions_eq(&result.secondary_actions, &[]);
    }

    #[test]
    fn should_return_all_primary_actions_as_visiblewhen_they_fit_within_the_container_width() {
        let template = action_template();
        let (secondary_models, _disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let container_size = 200.0;
        let input = vec![model(&template[0], 50.0), model(&template[1], 50.0), model(&template[2], 50.0)];
        let expected = vec![model(&template[0], 50.0), model(&template[1], 50.0), model(&template[2], 50.0)];
        let result = workbench_dynamic_calculate_actions(input, default_secondary_actions.clone(), container_size);
        assert_models_eq(&result.primary_actions, &expected);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }

    #[test]
    fn actions_all_within_a_group_that_cannot_all_fit_will_be_icon_only() {
        let template = action_template();
        let (secondary_models, _disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let container_size = 150.0;
        let input = vec![model(&template[0], 50.0), model(&template[1], 50.0), model(&template[2], 50.0)];
        let expected = vec![
            with_label(model(&template[0], 50.0), false),
            with_label(model(&template[1], 50.0), false),
            with_label(model(&template[2], 50.0), false),
        ];

        let result = workbench_dynamic_calculate_actions(input, default_secondary_actions.clone(), container_size);
        assert_models_eq(&result.primary_actions, &expected);
        assert_actions_eq(&result.secondary_actions, &secondary_actions(&secondary_models));
    }

    #[test]
    fn should_ignore_second_separator_when_two_separators_are_in_a_row() {
        let template = action_template();
        let (secondary_models, _disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let container_size = 200.0;
        let input = vec![model(&template[0], 50.0), separator_model(), separator_model(), model(&template[1], 50.0)];
        let expected = vec![model(&template[0], 50.0), separator_model(), model(&template[1], 50.0)];
        let result = workbench_dynamic_calculate_actions(input, default_secondary_actions.clone(), container_size);
        assert_models_eq(&result.primary_actions, &expected);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }

    #[test]
    fn check_label_visibility_in_different_groupings() {
        let template = action_template();
        let (secondary_models, _disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let container_size = 150.0;
        let actions = vec![model(&template[0], 50.0), separator_model(), model(&template[1], 50.0), model(&template[2], 50.0)];
        let expected_output_actions = vec![
            model(&template[0], 50.0),
            separator_model(),
            with_label(model(&template[1], 50.0), false),
            with_label(model(&template[2], 50.0), false),
        ];

        let result = workbench_dynamic_calculate_actions(actions, default_secondary_actions.clone(), container_size);
        assert_models_eq(&result.primary_actions, &expected_output_actions);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }

    #[test]
    fn should_ignore_separators_when_they_are_at_the_end_of_the_resulting_primary_actions() {
        let template = action_template();
        let (secondary_models, _disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let container_size = 200.0;
        let input = vec![model(&template[0], 50.0), separator_model(), model(&template[1], 50.0), separator_model()];
        let expected = vec![model(&template[0], 50.0), separator_model(), model(&template[1], 50.0)];
        let result = workbench_dynamic_calculate_actions(input, default_secondary_actions.clone(), container_size);
        assert_models_eq(&result.primary_actions, &expected);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }

    #[test]
    fn should_keep_actions_with_size_0_in_primary_actions() {
        let template = action_template();
        let (secondary_models, _disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let container_size = 170.0;
        let input = vec![
            model(&template[0], 50.0),
            model(&template[1], 50.0),
            separator_model(),
            model(&template[2], 50.0),
            model(&template[3], 0.0),
        ];
        let expected = vec![
            model(&template[0], 50.0),
            model(&template[1], 50.0),
            separator_model(),
            with_label(model(&template[2], 50.0), false),
            with_label(model(&template[3], 0.0), false),
        ];
        let result = workbench_dynamic_calculate_actions(input, default_secondary_actions.clone(), container_size);
        assert_models_eq(&result.primary_actions, &expected);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }

    #[test]
    fn should_not_render_separator_if_preceeded_by_size_0_action_s_but_keep_size_0_action_in_primary() {
        let template = action_template();
        let (secondary_models, _disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let container_size = 116.0;
        let input = vec![
            model(&template[0], 0.0), // hidden
            separator_model(),        // sep
            model(&template[1], 50.0), // visible
        ];
        let expected = vec![
            model(&template[0], 0.0),  // hidden
            model(&template[1], 50.0), // visible
        ];
        let result = workbench_dynamic_calculate_actions(input, default_secondary_actions.clone(), container_size);
        assert_models_eq(&result.primary_actions, &expected);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }

    #[test]
    fn should_not_render_second_separator_if_space_between_is_hidden_size_0_actions() {
        let template = action_template();
        let (secondary_models, _disposables) = setup();
        let default_secondary_actions = secondary_actions(&secondary_models);
        let container_size = 300.0;
        let input = vec![
            model(&template[0], 50.0),
            separator_model(),
            model(&template[1], 0.0),
            model(&template[2], 0.0),
            separator_model(),
            model(&template[3], 50.0),
        ];
        let expected = vec![
            model(&template[0], 50.0),
            separator_model(),
            model(&template[1], 0.0),
            model(&template[2], 0.0),
            // remove separator here
            model(&template[3], 50.0),
        ];
        let result = workbench_dynamic_calculate_actions(input, default_secondary_actions.clone(), container_size);
        assert_models_eq(&result.primary_actions, &expected);
        assert_actions_eq(&result.secondary_actions, &default_secondary_actions);
    }
}
