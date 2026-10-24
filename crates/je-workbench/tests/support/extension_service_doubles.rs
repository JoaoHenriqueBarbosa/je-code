//! Os dublês de `workbenchTestServices.ts` e `testDialogService.ts` que o `AbstractExtensionService` pede no construtor, usados por
//! `extension_service.rs` (porte de `extensionService.test.ts`) e por `extension_host_manager.rs`.
//!
//! - `TestWorkbenchExtensionEnablementService` e `TestWorkbenchExtensionManagementService` são os dublês de `workbenchTestServices.ts`.
//! - `TestRemoteAuthorityResolverService` é o `RemoteAuthorityResolverService` do navegador, que o porte não tem e que só o
//!   `_resolveAuthorityAgain` lê: rejeita qualquer resolução.
//! - `TestDialogService` é o de `platform/dialogs/test/common/testDialogService.ts`: `confirm` resolve `{ confirmed: false }`.

use je_base::errors::Error;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::management::ExtensionsControlManifest;
use je_platform::dialogs::{Confirmation, ConfirmationResult, IDialogService, Input, InputResult, Prompt, PromptResult};
use je_platform::remote::remote_authority_resolver::{IRemoteAuthorityResolverService, ResolverResult};
use je_workbench::services::extension_management::extension_management::{
    DidChangeProfileEvent, DidUninstallExtensionEvent, EnablementState, ExtensionRef, IExtension, IExtensionManagementService, InstallExtensionResult,
    IProfileAwareExtensionManagementService, IWorkbenchExtensionEnablementService, IWorkbenchExtensionManagementService, WorkspaceTypeOverrides,
};

/// `TestWorkbenchExtensionEnablementService` (`workbenchTestServices.ts`).
pub struct TestWorkbenchExtensionEnablementService;

impl IWorkbenchExtensionEnablementService for TestWorkbenchExtensionEnablementService {
    fn on_enablement_changed(&self) -> Event<Vec<ExtensionRef>> {
        Event::none()
    }

    fn get_enablement_state(&self, _extension: &IExtension) -> EnablementState {
        EnablementState::EnabledGlobally
    }

    /// Um estado por extensão pedida (`EnabledGlobally`). Para a lista vazia (o caso de `extensionService.test.ts`, que não resolve
    /// nenhuma extensão) o resultado é o mesmo do dublê original; o teste de integração do host real precisa que a extensão passe.
    fn get_enablement_states(&self, extensions: &[ExtensionRef], _workspace_type_overrides: WorkspaceTypeOverrides) -> Vec<EnablementState> {
        extensions.iter().map(|_| EnablementState::EnabledGlobally).collect()
    }

    fn get_dependencies_enablement_states(&self, _extension: &IExtension) -> Vec<(ExtensionRef, EnablementState)> {
        Vec::new()
    }

    fn can_change_enablement(&self, _extension: &IExtension) -> bool {
        true
    }

    fn can_change_workspace_enablement(&self, _extension: &IExtension) -> bool {
        true
    }

    fn is_enabled(&self, _extension: &IExtension) -> bool {
        true
    }

    fn is_enabled_enablement_state(&self, _enablement_state: EnablementState) -> bool {
        true
    }

    fn is_disabled_globally(&self, _extension: &IExtension) -> bool {
        false
    }

    fn set_enablement(&self, _extensions: Vec<ExtensionRef>, _state: EnablementState) -> Promise<Vec<bool>> {
        Promise::resolved(Vec::new())
    }

    fn update_extensions_enablements_when_workspace_trust_changes(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `TestWorkbenchExtensionManagementService` (`workbenchTestServices.ts`), com os métodos que o contrato do porte tem.
pub struct TestWorkbenchExtensionManagementService;

impl IExtensionManagementService for TestWorkbenchExtensionManagementService {
    fn on_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        Event::none()
    }

    fn on_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        Event::none()
    }

    fn get_installed(&self) -> Promise<Vec<ExtensionRef>> {
        Promise::resolved(Vec::new())
    }

    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        Promise::rejected(Error::new("Method not implemented."))
    }
}

impl IProfileAwareExtensionManagementService for TestWorkbenchExtensionManagementService {
    fn on_profile_aware_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        Event::none()
    }

    fn on_profile_aware_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        Event::none()
    }

    fn on_did_change_profile(&self) -> Event<DidChangeProfileEvent> {
        Event::none()
    }
}

impl IWorkbenchExtensionManagementService for TestWorkbenchExtensionManagementService {
    fn get_installed_workspace_extensions(&self, _include_invalid: bool) -> Promise<Vec<ExtensionRef>> {
        Promise::resolved(Vec::new())
    }
}

/// O `RemoteAuthorityResolverService` do navegador (ver a nota do módulo): rejeita qualquer resolução.
pub struct TestRemoteAuthorityResolverService;

impl IRemoteAuthorityResolverService for TestRemoteAuthorityResolverService {
    fn resolve_authority(&self, _authority: &str) -> Promise<ResolverResult> {
        Promise::rejected(Error::new("Method not implemented."))
    }

    fn get_canonical_uri(&self, _uri: &URI) -> Promise<URI> {
        Promise::rejected(Error::new("Method not implemented."))
    }
}

/// `TestDialogService` (`platform/dialogs/test/common/testDialogService.ts`): `confirm` resolve `{ confirmed: false }`.
pub struct TestDialogService;

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: false })
    }

    fn prompt(&self, _prompt: Prompt) -> Promise<PromptResult> {
        Promise::resolved(PromptResult { checkbox_checked: None, result: None })
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        Promise::resolved(InputResult { confirmation: ConfirmationResult { checkbox_checked: None, confirmed: true }, values: Some(Vec::new()) })
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn about(&self) -> Promise<()> {
        Promise::resolved(())
    }
}
