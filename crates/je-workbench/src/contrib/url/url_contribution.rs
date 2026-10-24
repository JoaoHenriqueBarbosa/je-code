//! Porte de `src/vs/workbench/contrib/url/browser/url.contribution.ts`: a ação `workbench.action.url.openUrl` ("Open URL").
//!
//! Divergências:
//! - **Registro explícito.** O `registerAction2(OpenUrlAction)` roda no carregamento do módulo; aqui é
//!   [`register_open_url_action`], chamada por quem monta o workbench.
//! - **`run` devolve `Promise<void>`.** O `Action2::run` do porte não devolve valor, então o corpo vive em [`open_url`],
//!   que devolve a `Promise` do `input(...).then(...)` (o `return` do original), e o `run` a descarta. Quem confere a
//!   espera pelo resultado chama [`open_url`].
//! - `openerService.open(input, { allowContributedOpeners: true })` não é esperado nem tem o erro tratado, como no original
//!   (a promessa é descartada).
//! - O restante do arquivo (o comando de configurar domínios confiáveis, o `OpenerValidatorContributions`, o provedor de
//!   sistema de arquivos dos domínios confiáveis, o `ExternalUriResolverContribution`, a configuração
//!   `workbench.trustedDomains.promptInTrustedWorkspace` e o `TrustedDomainService`) depende de módulos que ainda não foram
//!   portados e entra com cada um deles.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableRef;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_instantiation::ServicesAccessor;
use je_platform::action::action_common_categories::Categories;
use je_platform::action::{Category, CommandAction, CommandActionTitle, Title, localize2};
use je_platform::actions::actions::{Action2, Action2Options, register_action2};
use je_platform::opener::{AllowContributedOpeners, I_OPENER_SERVICE, OpenOptions, OpenResource};
use je_platform::quickinput::quick_input::{I_QUICK_INPUT_SERVICE, IInputOptions};
use je_platform::storage::{I_STORAGE_SERVICE, StorageScope, StorageTarget};
use je_storage::StorageValue;

/// `OpenUrlAction.STORAGE_KEY`.
pub const OPEN_URL_STORAGE_KEY: &str = "workbench.action.url.openUrl.lastInput";

/// `OpenUrlAction`.
struct OpenUrlAction;

impl Action2 for OpenUrlAction {
    fn desc(&self) -> Action2Options {
        let title = localize2("openUrl", "Open URL");
        Action2Options {
            command: CommandAction {
                category: Some(Category::Localized(Categories::new().developer)),
                ..CommandAction::new(
                    "workbench.action.url.openUrl",
                    Title::Localized(CommandActionTitle { value: title.value, original: title.original, mnemonic_title: None }),
                )
            },
            menu: Vec::new(),
            keybinding: Vec::new(),
            f1: true,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let _ = open_url(accessor);
    }
}

/// O corpo de `OpenUrlAction.run`: pede a URL ao usuário, abre com os openers externos contribuídos e guarda a entrada.
pub fn open_url(accessor: &ServicesAccessor) -> Promise<()> {
    let quick_input_service = accessor.get(I_QUICK_INPUT_SERVICE);
    let opener_service = accessor.get(I_OPENER_SERVICE);
    let storage_service = accessor.get(I_STORAGE_SERVICE);

    let value = storage_service.get(OPEN_URL_STORAGE_KEY, StorageScope::Workspace, Some("")).unwrap_or_default();

    let options = IInputOptions { prompt: Some(localize("urlToOpen", "URL to open", &[])), value: Some(value), ..IInputOptions::default() };
    quick_input_service.input(Some(&options), None).then(move |input| {
        if let Some(input) = input.filter(|input| !input.is_empty()) {
            let _ = opener_service.open(
                &OpenResource::from(input.as_str()),
                Some(&OpenOptions { allow_contributed_openers: Some(AllowContributedOpeners::Bool(true)), ..OpenOptions::default() }),
            );
            storage_service.store(OPEN_URL_STORAGE_KEY, StorageValue::from(input.as_str()), StorageScope::Workspace, StorageTarget::Machine);
        }
    })
}

/// `registerAction2(OpenUrlAction)`.
pub fn register_open_url_action() -> DisposableRef {
    register_action2(Rc::new(OpenUrlAction))
}
