//! Porte de `src/vs/platform/userDataProfile/electron-main/userDataProfilesHandler.ts`: o `UserDataProfilesHandler`, que
//! solta a associação de uma janela com o perfil transitório quando a janela carrega outro conteúdo ou fecha (e limpa os
//! perfis transitórios que sobraram), e que, 30 segundos depois de criado, desfaz as associações de janelas vazias que
//! não estão mais abertas.
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; por isso o `handler` é o
//! mesmo, ligado direto aos eventos do ciclo de vida e às janelas.
//!
//! Decisões de tradução:
//! - **`ILifecycleMainService` e `IWindowsMainService`** (ainda sem porte: dependem de `BrowserWindow`) são os contratos
//!   [`LifecycleMainService`] e [`WindowsMainService`], só com o que o `handler` lê: `onWillLoadWindow`,
//!   `onBeforeCloseWindow` e `getWindows()`.
//! - **`ICodeWindow`** é [`CodeWindow`] mais [`ProfilesWindow`]: o contrato do porte dos workspaces tem `openedWorkspace`,
//!   e o `handler` lê também `backupPath` e `isExtensionDevelopmentHost`, que [`ProfilesWindow`] acrescenta.
//! - **`LoadReason`** (de `window.ts` de `electron-main`) é [`LoadReason`], com os mesmos três valores.
//! - **`IUserDataProfilesMainService`** é o [`UserDataProfilesMainService`] (ver a decisão no arquivo dele).
//! - **`unsetProfileForWorkspace`** é `async` no original, mas só espera `cleanUpTransientProfiles()` no fim: devolve a
//!   promessa dele. O segundo `if (profile.isTransient)` do original repete o primeiro e é sempre verdadeiro, então
//!   não aparece.
//! - **`toWorkspaceIdentifier(window.backupPath, window.isExtensionDevelopmentHost)`** é o
//!   [`to_empty_workspace_identifier`], que devolve o identificador da janela vazia.

use std::rc::Rc;

use je_base::async_::RunOnceScheduler;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;

use super::{IUserDataProfilesService, UserDataProfilesMainService};
use crate::workspace::{AnyWorkspaceIdentifier, to_empty_workspace_identifier};
use crate::workspaces::workspaces_management_main_service::CodeWindow;

/// `LoadReason` (`window.ts`): por que a janela está carregando.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadReason {
    /// The window is loaded for the first time.
    Initial = 1,
    /// The window is loaded into a different workspace context.
    Load = 2,
    /// The window is reloaded.
    Reload = 3,
}

/// O que o `handler` lê de `ICodeWindow` além do [`CodeWindow`].
pub trait ProfilesWindow: CodeWindow {
    /// `backupPath`.
    fn backup_path(&self) -> Option<String>;

    /// `isExtensionDevelopmentHost`.
    fn is_extension_development_host(&self) -> bool;
}

/// `IWindowLoadEvent` (o `onWillLoadWindow`), com o que o `handler` lê.
#[derive(Clone)]
pub struct WindowLoadEvent {
    pub window: Rc<dyn ProfilesWindow>,
    pub reason: LoadReason,
}

/// `ILifecycleMainService`, com os eventos que o `handler` ouve.
pub trait LifecycleMainService {
    /// `onWillLoadWindow`.
    fn on_will_load_window(&self) -> Event<WindowLoadEvent>;

    /// `onBeforeCloseWindow`.
    fn on_before_close_window(&self) -> Event<Rc<dyn ProfilesWindow>>;
}

/// `IWindowsMainService`, com o que o `handler` lê.
pub trait WindowsMainService {
    /// `getWindows()`.
    fn get_windows(&self) -> Vec<Rc<dyn ProfilesWindow>>;
}

/// `UserDataProfilesHandler`.
pub struct UserDataProfilesHandler {
    base: Rc<Disposable>,
}

impl UserDataProfilesHandler {
    pub fn new(lifecycle_main_service: Rc<dyn LifecycleMainService>, user_data_profiles_service: Rc<UserDataProfilesMainService>, windows_main_service: Rc<dyn WindowsMainService>) -> Rc<Self> {
        let base = Disposable::new();

        base.register(lifecycle_main_service.on_will_load_window().listen({
            let user_data_profiles_service = user_data_profiles_service.clone();
            move |e| {
                if e.reason == LoadReason::Load {
                    let _ = unset_profile_for_workspace(&user_data_profiles_service, e.window.as_ref());
                }
            }
        }));
        base.register(lifecycle_main_service.on_before_close_window().listen({
            let user_data_profiles_service = user_data_profiles_service.clone();
            move |window| {
                let _ = unset_profile_for_workspace(&user_data_profiles_service, window.as_ref());
            }
        }));
        let scheduler = RunOnceScheduler::new(move || clean_up_empty_window_associations(&user_data_profiles_service, windows_main_service.as_ref()), 30 * 1000 /* after 30s */);
        base.register(scheduler.clone());
        scheduler.schedule(None);

        Rc::new(Self { base })
    }
}

impl IDisposable for UserDataProfilesHandler {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `unsetProfileForWorkspace`.
fn unset_profile_for_workspace(user_data_profiles_service: &UserDataProfilesMainService, window: &dyn ProfilesWindow) -> Promise<()> {
    let workspace = get_workspace(window);
    let profile = user_data_profiles_service.get_profile_for_workspace(&workspace);
    match profile {
        Some(profile) if profile.is_transient == Some(true) => {
            user_data_profiles_service.unset_workspace(&workspace, true);
            user_data_profiles_service.clean_up_transient_profiles()
        }
        _ => Promise::resolved(()),
    }
}

/// `getWorkspace`.
fn get_workspace(window: &dyn ProfilesWindow) -> AnyWorkspaceIdentifier {
    window
        .opened_workspace()
        .unwrap_or_else(|| AnyWorkspaceIdentifier::Empty(to_empty_workspace_identifier(window.backup_path().as_deref(), window.is_extension_development_host())))
}

/// `cleanUpEmptyWindowAssociations`.
fn clean_up_empty_window_associations(user_data_profiles_service: &UserDataProfilesMainService, windows_main_service: &dyn WindowsMainService) {
    let associated_empty_windows = user_data_profiles_service.get_associated_empty_windows();
    if associated_empty_windows.is_empty() {
        return;
    }
    let opened_workspaces: Vec<AnyWorkspaceIdentifier> = windows_main_service.get_windows().iter().map(|window| get_workspace(window.as_ref())).collect();
    for associated_empty_window in associated_empty_windows {
        if opened_workspaces.iter().any(|opened_workspace| opened_workspace.id() == associated_empty_window.id) {
            continue;
        }
        user_data_profiles_service.unset_workspace(&AnyWorkspaceIdentifier::Empty(associated_empty_window), false);
    }
}
