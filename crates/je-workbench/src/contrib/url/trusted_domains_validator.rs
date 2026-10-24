//! Porte de `src/vs/workbench/contrib/url/browser/trustedDomainsValidator.ts`: o `OpenerValidatorContributions`, o
//! validador registrado no serviço de abertura que exige confiança no workspace para links de comando vindos do
//! workspace e pergunta ao usuário antes de abrir um site que não é de um domínio confiável.
//!
//! Divergências:
//! - `validateLink` é `async` e devolve `Result<bool, Error>`: a rejeição do original (o diálogo ou o tratador de
//!   domínios que falham) vira o `Err`.
//! - `URI | string` é [`OpenResource`].
//! - O construtor devolve `Rc<Self>` porque o validador registrado no [`IOpenerService`] guarda uma referência fraca ao
//!   contribuinte (o original guarda `this` na closure). Os serviços que o original injeta como `undefined!` nos testes
//!   são `Option` e, ausentes, o uso falha com mensagem (o `TypeError` do JavaScript).
//! - `ITrustedDomainService` (`trustedDomainService.ts`) e as funções `readStaticTrustedDomains` e
//!   `configureOpenerTrustedDomainsHandler` (`trustedDomains.ts`) ainda não foram portadas: entram como os contratos
//!   [`ITrustedDomainService`] e [`ITrustedDomainsHandler`]. O segundo reúne o que o original faz com o armazenamento, o
//!   quick input, o serviço de editores, a telemetria e o serviço de instanciação (`IStorageService`,
//!   `IQuickInputService`, `IEditorService`, `ITelemetryService`, `IInstantiationService`), que por isso não são
//!   parâmetros do construtor.

use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::network::{UriOrString, matches_scheme, schemas};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::{
    BaseDialogOptions, DialogDetail, DialogTypeOrSeverity, IDialogService, Prompt, PromptBaseButton, PromptButton, PromptCancel,
    PromptCancelButton,
};
use je_platform::opener::{AllowCommands, IOpenerService, IValidator, OpenOptions, OpenResource};
use je_platform::product::IProductService;
use je_platform::url::is_url_domain_trusted;
use je_platform::workspace_trust::{IWorkspaceTrustManagementService, IWorkspaceTrustRequestService, WorkspaceTrustRequestOptions};

/// `ITrustedDomainService` (`isValid`).
pub trait ITrustedDomainService {
    fn is_valid(&self, resource: &URI) -> bool;
}

/// `readStaticTrustedDomains` e `configureOpenerTrustedDomainsHandler` de `trustedDomains.ts`.
pub trait ITrustedDomainsHandler {
    /// `readStaticTrustedDomains(accessor).trustedDomains`.
    fn read_static_trusted_domains(&self) -> Vec<JsString>;

    /// `configureOpenerTrustedDomainsHandler(trustedDomains, domainToOpen, resource, ...)`: os domínios escolhidos.
    fn configure_opener_trusted_domains(&self, trusted_domains: &[JsString], domain_to_open: &str, resource: &URI) -> Promise<Vec<JsString>>;
}

/// O `{ shouldOpen: (uri, options) => this.validateLink(uri, options) }` registrado no serviço de abertura.
struct LinkValidator(Weak<OpenerValidatorContributions>);

impl IValidator for LinkValidator {
    fn should_open(&self, resource: &OpenResource, open_options: Option<&OpenOptions>) -> Promise<bool> {
        let Some(contributions) = self.0.upgrade() else {
            return Promise::rejected(Error::new("OpenerValidatorContributions is disposed"));
        };
        let resource = resource.clone();
        let open_options = open_options.cloned();
        Promise::with_executor(move |resolver| {
            runtime::spawn_local(async move {
                match contributions.validate_link(resource, open_options.as_ref()).await {
                    Ok(should_open) => resolver.resolve(should_open),
                    Err(error) => resolver.reject(error),
                }
            });
        })
    }
}

/// `OpenerValidatorContributions`.
pub struct OpenerValidatorContributions {
    dialog_service: Option<Rc<dyn IDialogService>>,
    product_service: Option<Rc<dyn IProductService>>,
    clipboard_service: Option<Rc<dyn IClipboardService>>,
    configuration_service: Option<Rc<dyn IConfigurationService>>,
    workspace_trust_service: Rc<dyn IWorkspaceTrustManagementService>,
    workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService>,
    trusted_domain_service: Option<Rc<dyn ITrustedDomainService>>,
    trusted_domains_handler: Option<Rc<dyn ITrustedDomainsHandler>>,
}

fn required<'a, T: ?Sized>(service: &'a Option<Rc<T>>, name: &str) -> &'a Rc<T> {
    service.as_ref().unwrap_or_else(|| panic!("TypeError: {name} is undefined"))
}

impl OpenerValidatorContributions {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        opener_service: &Rc<dyn IOpenerService>,
        dialog_service: Option<Rc<dyn IDialogService>>,
        product_service: Option<Rc<dyn IProductService>>,
        clipboard_service: Option<Rc<dyn IClipboardService>>,
        configuration_service: Option<Rc<dyn IConfigurationService>>,
        workspace_trust_service: Rc<dyn IWorkspaceTrustManagementService>,
        workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService>,
        trusted_domain_service: Option<Rc<dyn ITrustedDomainService>>,
        trusted_domains_handler: Option<Rc<dyn ITrustedDomainsHandler>>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|weak| {
            // O registro vive tanto quanto o serviço de abertura, como no original (o retorno não é guardado).
            let _ = opener_service.register_validator(Rc::new(LinkValidator(weak.clone())));
            Self {
                dialog_service,
                product_service,
                clipboard_service,
                configuration_service,
                workspace_trust_service,
                workspace_trust_request_service,
                trusted_domain_service,
                trusted_domains_handler,
            }
        })
    }

    /// `validateLink`.
    pub async fn validate_link(&self, resource: OpenResource, open_options: Option<&OpenOptions>) -> Result<bool, Error> {
        let resource_uri = match &resource {
            OpenResource::Text(text) => URI::parse(text.clone(), false),
            OpenResource::Uri(uri) => uri.clone(),
        };

        let allow_commands = match open_options.and_then(|options| options.allow_commands.as_ref()) {
            Some(AllowCommands::Bool(true)) => true,
            Some(AllowCommands::Only(commands)) => commands.iter().any(|command| command == resource_uri.path()),
            _ => false,
        };
        if matches_scheme(UriOrString::Uri(&resource_uri), schemas::COMMAND) && !self.workspace_trust_service.is_workspace_trusted() && allow_commands {
            let mut message: Option<String> = None;
            if open_options.and_then(|options| options.from_workspace).unwrap_or(false) {
                message = Some(localize("commandWorkspaceTrust", "Running a command from a link requires trusting this workspace.", &[]));
            }

            if let Some(message) = message {
                let trusted = self
                    .workspace_trust_request_service
                    .request_workspace_trust(Some(WorkspaceTrustRequestOptions { buttons: None, message: Some(message) }))
                    .await?;
                return Ok(trusted == Some(true));
            }
        }

        if !matches_scheme(UriOrString::Uri(&resource_uri), schemas::HTTP) && !matches_scheme(UriOrString::Uri(&resource_uri), schemas::HTTPS) {
            return Ok(true);
        }

        if open_options.and_then(|options| options.from_workspace).unwrap_or(false)
            && self.workspace_trust_service.is_workspace_trusted()
            && !required(&self.configuration_service, "configurationService")
                .get_value(Some("workbench.trustedDomains.promptInTrustedWorkspace"), None)
                .is_truthy()
        {
            return Ok(true);
        }

        let original_resource = resource;

        if required(&self.trusted_domain_service, "trustedDomainService").is_valid(&resource_uri) {
            return Ok(true);
        }

        let scheme = resource_uri.scheme().clone();
        let authority = resource_uri.authority().clone();
        let mut formatted_link: Vec<u16> = format!("{scheme}://{authority}{}", resource_uri.path()).encode_utf16().collect();

        let query = resource_uri.query();
        let fragment = resource_uri.fragment();
        let mut link_tail: Vec<u16> = Vec::new();
        if !query.is_empty() {
            link_tail.push(u16::from(b'?'));
            link_tail.extend_from_slice(query.units());
        }
        if !fragment.is_empty() {
            link_tail.push(u16::from(b'#'));
            link_tail.extend_from_slice(fragment.units());
        }

        let remaining_length = 60usize.saturating_sub(formatted_link.len());
        let link_tail_length_to_keep = remaining_length.max(5).min(link_tail.len());

        if link_tail_length_to_keep == link_tail.len() {
            formatted_link.extend_from_slice(&link_tail);
        } else {
            // keep the first char ? or #
            // add ... and keep the tail end as much as possible
            formatted_link.push(link_tail[0]);
            formatted_link.extend("...".encode_utf16());
            formatted_link.extend_from_slice(&link_tail[link_tail.len() - link_tail_length_to_keep + 1..]);
        }

        let original_text = match &original_resource {
            OpenResource::Text(text) => Some(text.to_string_lossy()),
            OpenResource::Uri(_) => None,
        };
        let detail = original_text.clone().unwrap_or_else(|| JsString::from_units(formatted_link).to_string_lossy());

        let clipboard_service = self.clipboard_service.clone();
        let copy_text = original_text.unwrap_or_else(|| resource_uri.to_string(true).to_string_lossy());
        let handler = self.trusted_domains_handler.clone();
        let configure_uri = resource_uri.clone();
        let domain_to_open = format!("{scheme}://{authority}");

        let result = required(&self.dialog_service, "dialogService")
            .prompt(Prompt {
                base: BaseDialogOptions {
                    dialog_type: Some(DialogTypeOrSeverity::Severity(Severity::Info)),
                    detail: Some(DialogDetail::Text(detail)),
                    ..BaseDialogOptions::new(localize(
                        "openExternalLinkAt",
                        "Do you want {0} to open the external website?",
                        &[required(&self.product_service, "productService").name_short()],
                    ))
                },
                buttons: Some(vec![
                    PromptButton {
                        label: localize("open", "&&Open", &[]),
                        base: PromptBaseButton { run: Rc::new(|_| MaybePromise::Value(Dynamic::Bool(true))) },
                    },
                    PromptButton {
                        label: localize("copy", "&&Copy", &[]),
                        base: PromptBaseButton {
                            run: Rc::new(move |_| {
                                let _ = required(&clipboard_service, "clipboardService").write_text(&copy_text);
                                MaybePromise::Value(Dynamic::Bool(false))
                            }),
                        },
                    },
                    PromptButton {
                        label: localize("configureTrustedDomains", "Configure &&Trusted Domains", &[]),
                        base: PromptBaseButton {
                            run: Rc::new(move |_| {
                                let handler = required(&handler, "trustedDomainsHandler").clone();
                                let trusted_domains = handler.read_static_trusted_domains();
                                let configure_uri = configure_uri.clone();
                                let picked_domains = handler.configure_opener_trusted_domains(&trusted_domains, &domain_to_open, &configure_uri);
                                MaybePromise::Promise(picked_domains.then(move |picked_domains| {
                                    // Trust all domains
                                    if picked_domains.iter().any(|domain| *domain == "*") {
                                        return Dynamic::Bool(true);
                                    }
                                    // Trust current domain
                                    Dynamic::Bool(is_url_domain_trusted(&configure_uri, &picked_domains))
                                }))
                            }),
                        },
                    },
                ]),
                cancel_button: Some(PromptCancel::Button(PromptCancelButton {
                    label: None,
                    base: PromptBaseButton { run: Rc::new(|_| MaybePromise::Value(Dynamic::Bool(false))) },
                })),
            })
            .await?;

        Ok(matches!(result.result, Some(Dynamic::Bool(true))))
    }
}
