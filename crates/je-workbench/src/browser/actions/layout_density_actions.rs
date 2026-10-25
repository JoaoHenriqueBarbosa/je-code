//! Porte de `src/vs/workbench/browser/actions/layoutDensityActions.ts`: o submenu `LayoutDensityMenu` e as duas ações
//! que escolhem a densidade do layout (`workbench.action.setLayoutDensity.default` e `.compact`).
//!
//! Divergências:
//! - **Registro explícito.** Os `registerAction2` rodam no carregamento do módulo; aqui são
//!   [`register_layout_density_actions`], chamada por quem monta o workbench.
//! - **`run` devolve `Promise<void>`.** O `Action2::run` do porte não devolve valor, então o corpo vive em
//!   [`set_layout_density`], que devolve a `Promise` do `updateValue`, e o `run` a descarta. Quem confere a espera pelo
//!   `updateValue` (o `await` do comando no original) chama [`set_layout_density`].
//! - `new MenuId('LayoutDensityMenu')` é o `MenuId::for_id` (o mesmo identificador, sem lançar se a função for chamada
//!   de novo no mesmo thread).
//! - `ServicesAccessor` e `accessor.get(IWorkbenchEnvironmentService)` são `accessor.get(I_WORKBENCH_ENVIRONMENT_SERVICE)`.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, combined_disposable};
use je_base::promise::Promise;
use je_instantiation::ServicesAccessor;
use je_platform::action::{CommandAction, CommandActionTitle, Title, Toggled, localize2};
use je_platform::actions::actions::{Action2, Action2MenuItem, Action2Options, MenuId, register_action2};
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::service::{IConfigurationService, I_CONFIGURATION_SERVICE};
use je_platform::contextkey::contextkey::ContextKeyExpr;

use crate::services::environment::environment_service::{I_WORKBENCH_ENVIRONMENT_SERVICE, IWorkbenchEnvironmentService};
use crate::services::layout::layout_service::{LayoutSettings, ModernUiDensity};

/// `LayoutDensityMenu`.
#[allow(non_snake_case)]
pub fn LayoutDensityMenu() -> MenuId {
    MenuId::for_id("LayoutDensityMenu")
}

/// `layoutDensityOptions`: a densidade, a chave e o texto do título de cada opção, na ordem do original.
fn layout_density_options() -> [(ModernUiDensity, &'static str, &'static str); 2] {
    [(ModernUiDensity::Default, "layoutDensityDefault", "Default"), (ModernUiDensity::Compact, "layoutDensityCompact", "Compact")]
}

/// O `registerAction2(class extends Action2 { ... })` de uma opção.
struct SetLayoutDensityAction {
    density: ModernUiDensity,
    title_key: &'static str,
    title_message: &'static str,
    index: usize,
}

impl Action2 for SetLayoutDensityAction {
    fn desc(&self) -> Action2Options {
        let localized = localize2(self.title_key, self.title_message);
        Action2Options {
            command: CommandAction {
                toggled: Some(Toggled::Expression(ContextKeyExpr::equals(&format!("config.{}", LayoutSettings::ModernUiDensity.as_str()), self.density.as_str()))),
                ..CommandAction::new(
                    format!("workbench.action.setLayoutDensity.{}", self.density.as_str()),
                    Title::Localized(CommandActionTitle { value: localized.value, original: localized.original, mnemonic_title: None }),
                )
            },
            menu: vec![Action2MenuItem {
                id: LayoutDensityMenu(),
                null_precondition: false,
                alt: None,
                when: None,
                group: None,
                order: Some((self.index + 1) as f64),
                is_hidden_by_default: None,
            }],
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let _ = set_layout_density(accessor.get(I_CONFIGURATION_SERVICE).as_ref(), accessor.get(I_WORKBENCH_ENVIRONMENT_SERVICE).as_ref(), self.density);
    }
}

/// O corpo de `run`: na janela de sessões grava no workspace, nas outras no alvo padrão.
pub fn set_layout_density(
    configuration_service: &dyn IConfigurationService,
    environment_service: &dyn IWorkbenchEnvironmentService,
    density: ModernUiDensity,
) -> Promise<()> {
    let value = Dynamic::string(density.as_str());
    if environment_service.is_sessions_window() {
        return configuration_service.update_value(LayoutSettings::ModernUiDensity.as_str(), value, None, Some(ConfigurationTarget::Workspace));
    }
    configuration_service.update_value(LayoutSettings::ModernUiDensity.as_str(), value, None, None)
}

/// Registra as duas ações (o laço `for` do original).
pub fn register_layout_density_actions() -> DisposableRef {
    combined_disposable(
        layout_density_options()
            .into_iter()
            .enumerate()
            .map(|(index, (density, title_key, title_message))| {
                register_action2(Rc::new(SetLayoutDensityAction { density, title_key, title_message, index }))
            })
            .collect(),
    )
}
