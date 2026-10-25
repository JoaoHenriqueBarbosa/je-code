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
//!   [`LifecycleMainService`] e [`WindowsMainService`], só com o que os consumidores portados leem: `onWillLoadWindow`,
//!   `onBeforeCloseWindow` e `getWindows()` (o `handler`), `onWillShutdown` e `when(phase)` (o `StorageMainService`).
//!   `ShutdownReason`, `ShutdownEvent` e `LifecycleMainPhase` são os de `lifecycleMainService.ts`.
//! - **`ICodeWindow`** é [`CodeWindow`] mais [`ProfilesWindow`]: o contrato do porte dos workspaces tem `openedWorkspace`,
//!   e o `handler` lê também `backupPath` e `isExtensionDevelopmentHost`, que [`ProfilesWindow`] acrescenta.
//! - **`LoadReason`** (de `window.ts` de `electron-main`) é [`LoadReason`], com os mesmos três valores.
//! - **`IUserDataProfilesMainService`** é o [`UserDataProfilesMainService`] (ver a decisão no arquivo dele).
//! - **`unsetProfileForWorkspace`** é `async` no original, mas só espera `cleanUpTransientProfiles()` no fim: devolve a
//!   promessa dele. O segundo `if (profile.isTransient)` do original repete o primeiro e é sempre verdadeiro, então
//!   não aparece.
//! - **`toWorkspaceIdentifier(window.backupPath, window.isExtensionDevelopmentHost)`** é o
//!   [`to_empty_workspace_identifier`], que devolve o identificador da janela vazia.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::RunOnceScheduler;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;

use super::{IUserDataProfilesService, UserDataProfile, UserDataProfilesMainService};
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

    /// `profile`: o perfil da janela (o `StorageMainService` esquenta o armazém dele quando a janela carrega).
    fn profile(&self) -> Option<UserDataProfile>;
}

/// `IWindowLoadEvent` (o `onWillLoadWindow`), com o que o `handler` e o `StorageMainService` leem.
#[derive(Clone)]
pub struct WindowLoadEvent {
    pub window: Rc<dyn ProfilesWindow>,
    /// `workspace`: o workspace que a janela vai carregar, se houver.
    pub workspace: Option<AnyWorkspaceIdentifier>,
    pub reason: LoadReason,
}

/// `ShutdownReason` (`lifecycleMainService.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownReason {
    /// The application exits normally.
    Quit = 1,
    /// The application exits abnormally and is being killed with an exit code (e.g. from integration test run).
    Kill,
}

/// `LifecycleMainPhase` (`lifecycleMainService.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LifecycleMainPhase {
    /// The first phase signals that we are about to startup.
    Starting = 1,
    /// Services are ready and first window is about to open.
    Ready = 2,
    /// This phase signals a point in time after the window has opened and is typically the best place to do work that is not required for the window to open.
    AfterWindowOpen = 3,
    /// The last phase after a window has opened and some time has passed (2-5 seconds).
    Eventually = 4,
}

/// `ShutdownEvent` (`lifecycleMainService.ts`): o motivo e o `join`, que bloqueia o encerramento até as promessas assentarem.
#[derive(Clone)]
pub struct ShutdownEvent {
    pub reason: ShutdownReason,
    joiners: Rc<RefCell<Vec<(String, Promise<()>)>>>,
}

impl ShutdownEvent {
    pub fn new(reason: ShutdownReason) -> Self {
        Self { reason, joiners: Rc::new(RefCell::new(Vec::new())) }
    }

    /// `join(id, promise)`.
    pub fn join(&self, id: &str, promise: Promise<()>) {
        self.joiners.borrow_mut().push((id.to_string(), promise));
    }

    /// As promessas que os ouvintes juntaram, com o id de cada uma.
    pub fn joiners(&self) -> Vec<(String, Promise<()>)> {
        self.joiners.borrow().clone()
    }
}

/// `ILifecycleMainService`, com o que o `handler` e o `StorageMainService` leem.
pub trait LifecycleMainService {
    /// `onWillLoadWindow`.
    fn on_will_load_window(&self) -> Event<WindowLoadEvent>;

    /// `onBeforeCloseWindow`.
    fn on_before_close_window(&self) -> Event<Rc<dyn ProfilesWindow>>;

    /// `onWillShutdown`.
    fn on_will_shutdown(&self) -> Event<ShutdownEvent>;

    /// `when(phase)`: assenta quando o ciclo de vida chega à fase (ou já passou dela).
    fn when(&self, phase: LifecycleMainPhase) -> Promise<()>;

    /// `quit(willRestart)`: resolve `true` quando o encerramento foi vetado.
    fn quit(&self, will_restart: bool) -> Promise<bool>;
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
