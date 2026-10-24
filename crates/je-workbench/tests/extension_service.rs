//! Porte de `src/vs/workbench/services/extensions/test/browser/extensionService.test.ts` (suíte `BrowserExtensionService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()`: o caso não cria nenhum descartável, então não há o que o guarda conferir.
//! - Cada `assert.deepStrictEqual(BrowserExtensionHostKindPicker.pickRunningLocation(kinds, local, remote, None), esperado)`
//!   é uma linha de [`check_row`], que confere as quatro combinações de `(isInstalledLocally, isInstalledRemotely)` na ordem do
//!   original (`false,false`, `false,true`, `true,false`, `true,true`), uma linha por lista de tipos, na ordem do original.
//!
//! **Não portada: a suíte `ExtensionService` (10 casos: `issue #152204`, `Extension host disposed when awaited`,
//! `... not disposed when vetoed (sync)` e `(async)`, `onWillActivateByEvent includes activationKind for Normal activation`
//! e `for Immediate activation`, `Immediate activation includes ready remote extension hosts (issue #297019)`, `... excludes
//! not-ready remote extension hosts`, `... still activates local extension hosts even when not ready` e `Normal activation
//! activates all extension hosts`).** Todos criam um `AbstractExtensionService` (`common/abstractExtensionService.ts`, com o
//! `ExtensionHostManager`, o `IExtensionHost`, o `ExtensionsProposedApi`, o `ResolvedExtensions` e os `IExtensionHostFactory`),
//! que o porte ainda não tem; sem a classe testada não há o que portar, e a suíte entra junto com ela.

use je_extensions::manifest::ExtensionKind::{Ui, Web, Workspace};
use je_extensions::manifest::ExtensionKind;
use je_workbench::services::extensions::browser::extension_service::BrowserExtensionHostKindPicker;
use je_workbench::services::extensions::extension_host_kind::{ExtensionHostKind, ExtensionRunningPreference};

/// As quatro combinações de `(isInstalledLocally, isInstalledRemotely)` do original, na ordem delas.
const COMBINATIONS: [(bool, bool); 4] = [(false, false), (false, true), (true, false), (true, true)];

/// `pickRunningLocation(kinds, local, remote, ExtensionRunningPreference.None)` nas quatro combinações.
fn check_row(kinds: &[ExtensionKind], expected: [Option<ExtensionHostKind>; 4]) {
    for (index, (is_installed_locally, is_installed_remotely)) in COMBINATIONS.into_iter().enumerate() {
        assert_eq!(
            BrowserExtensionHostKindPicker::pick_running_location(kinds, is_installed_locally, is_installed_remotely, ExtensionRunningPreference::None),
            expected[index],
            "kinds: {kinds:?}, isInstalledLocally: {is_installed_locally}, isInstalledRemotely: {is_installed_remotely}"
        );
    }
}

#[test]
fn pick_running_location() {
    const REMOTE: Option<ExtensionHostKind> = Some(ExtensionHostKind::Remote);
    const LOCAL: Option<ExtensionHostKind> = Some(ExtensionHostKind::LocalWebWorker);
    const NONE: Option<ExtensionHostKind> = None;

    check_row(&[], [NONE, NONE, NONE, NONE]);

    check_row(&[Ui], [NONE, REMOTE, NONE, REMOTE]);

    check_row(&[Workspace], [NONE, REMOTE, NONE, REMOTE]);

    check_row(&[Web], [NONE, LOCAL, LOCAL, LOCAL]);

    check_row(&[Ui, Workspace], [NONE, REMOTE, NONE, REMOTE]);
    check_row(&[Workspace, Ui], [NONE, REMOTE, NONE, REMOTE]);

    check_row(&[Web, Workspace], [NONE, LOCAL, LOCAL, LOCAL]);
    check_row(&[Workspace, Web], [NONE, REMOTE, LOCAL, REMOTE]);

    check_row(&[Ui, Web], [NONE, LOCAL, LOCAL, LOCAL]);
    check_row(&[Web, Ui], [NONE, LOCAL, LOCAL, LOCAL]);

    check_row(&[Ui, Web, Workspace], [NONE, LOCAL, LOCAL, LOCAL]);
    check_row(&[Ui, Workspace, Web], [NONE, REMOTE, LOCAL, REMOTE]);

    check_row(&[Web, Ui, Workspace], [NONE, LOCAL, LOCAL, LOCAL]);
    check_row(&[Web, Workspace, Ui], [NONE, LOCAL, LOCAL, LOCAL]);

    check_row(&[Workspace, Ui, Web], [NONE, REMOTE, LOCAL, REMOTE]);
    check_row(&[Workspace, Web, Ui], [NONE, REMOTE, LOCAL, REMOTE]);
}
