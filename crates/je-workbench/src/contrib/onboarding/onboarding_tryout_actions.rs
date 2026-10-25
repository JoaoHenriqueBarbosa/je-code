//! Porte de `src/vs/workbench/contrib/onboarding/common/onboardingTryoutActions.ts` (os payloads dos tryouts e os
//! validadores) e de `browser/onboardingTryoutActions.ts` (as apresentações de comando e de view).
//!
//! Divergências:
//! - **Payload tipado e validador sobre [`Dynamic`].** O `payload` de um cenário é `unknown`. Os validadores
//!   (`isCommandTryoutPayload`, `isViewTryoutPayload`, `isEditorSampleTryoutPayload`) conferem a forma do valor
//!   dinâmico como o original e, quando ela vale, o `*_of` devolve o payload tipado. O payload guiado viaja como valor
//!   nativo de um objeto (`ObjectRef::with_host`), como os demais payloads do onboarding; por isso o
//!   `isGuidedTryoutPayload` confere o tipo nativo: a forma já é garantida pelo tipo.
//! - **Definição de apresentação.** As apresentações têm `get_availability` e `prepare` sobre o payload tipado e
//!   [`typed_presentation_definition`] monta o [`OnboardingTryoutPresentationDefinition`] que o registro consome, com o
//!   `isPayload` do original.
//! - `IViewsService.openView(...)` e `openViewContainer(...)` devolvem `Promise<bool>` (`true` quando o original devolve
//!   uma view, `false` quando devolve `null`).
//! - `async prepare` e `async run` são [`async_fn`]; as promessas não podem emprestar `self`, então os serviços entram
//!   clonados no fecho.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{self, Event};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::actions::menu_registry;
use je_platform::commands::commands::{ICommandService, commands_registry};
use je_platform::contextkey::contextkey::IContextKeyService;

use super::onboarding_scenario::OnboardingPresentationRef;
use super::onboarding_sequence::OnboardingSequenceStep;
use super::onboarding_tryout::{
    OnboardingTryoutAvailability, OnboardingTryoutLaunchKind, OnboardingTryoutPreparation, OnboardingTryoutResult,
    OnboardingTryoutRunContext, OnboardingTryoutUnavailable, RUN_ONBOARDING_TRYOUT_COMMAND_ID, is_onboarding_target_scope,
};
use super::onboarding_tryout_service::OnboardingTryoutPresentationDefinition;
use crate::common::views::IViewDescriptorService;
use crate::services::views::views_service::IViewsService;

pub const COMMAND_TRYOUT_PRESENTATION_KIND: &str = "command";
pub const VIEW_TRYOUT_PRESENTATION_KIND: &str = "openView";

/// `ICommandTryoutPayload`.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandTryoutPayload {
    pub command_id: String,
    pub arguments: Option<Vec<Dynamic>>,
    /// Usa o `{ targetScope }` devolvido pelo comando para ligar os passos guiados seguintes a uma instância da UI.
    pub capture_target_scope: Option<bool>,
}

/// O `target: 'view' | 'container'` de `IViewTryoutPayload`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewTryoutTarget {
    View,
    Container,
}

/// `IViewTryoutPayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewTryoutPayload {
    pub id: String,
    pub target: ViewTryoutTarget,
    pub focus: Option<bool>,
}

/// `EditorSampleTryoutPayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorSampleTryoutPayload {
    Text { title: String, text: String, language_id: Option<String> },
    Diff { title: String, original: String, modified: String, language_id: Option<String> },
}

impl EditorSampleTryoutPayload {
    pub fn title(&self) -> &str {
        match self {
            Self::Text { title, .. } | Self::Diff { title, .. } => title,
        }
    }

    pub fn language_id(&self) -> Option<&str> {
        match self {
            Self::Text { language_id, .. } | Self::Diff { language_id, .. } => language_id.as_deref(),
        }
    }

    /// O objeto literal do original (`{ type, title, ..., languageId? }`).
    pub fn to_dynamic(&self) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = match self {
            Self::Text { title, text, .. } => {
                vec![("type", Dynamic::string("text")), ("title", Dynamic::string(title)), ("text", Dynamic::string(text))]
            }
            Self::Diff { title, original, modified, .. } => vec![
                ("type", Dynamic::string("diff")),
                ("title", Dynamic::string(title)),
                ("original", Dynamic::string(original)),
                ("modified", Dynamic::string(modified)),
            ],
        };
        if let Some(language_id) = self.language_id() {
            entries.push(("languageId", Dynamic::string(language_id)));
        }
        Dynamic::object(entries)
    }
}

/// `IGuidedTryoutPayload`.
#[derive(Debug, Clone, PartialEq)]
pub struct GuidedTryoutPayload {
    pub launch: OnboardingPresentationRef,
    pub steps: Vec<OnboardingSequenceStep>,
    pub unavailable_message: Option<String>,
}

/// `typeof value === 'object' && value !== null`.
fn is_object_like(value: &Dynamic) -> bool {
    matches!(value, Dynamic::Object(_) | Dynamic::Array(_))
}

/// `typeof value.key === 'string'`.
fn text_of(value: &Dynamic, key: &str) -> Option<String> {
    match value.get_str(key) {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// A propriedade ausente ou `undefined` é `Some(None)`; o valor do tipo certo é `Some(Some(_))`; o de outro tipo é `None`.
fn optional_of<T>(value: &Dynamic, key: &str, pick: impl Fn(Dynamic) -> Option<T>) -> Option<Option<T>> {
    match value.get_str(key) {
        Dynamic::Undefined => Some(None),
        other => pick(other).map(Some),
    }
}

fn pick_bool(value: Dynamic) -> Option<bool> {
    match value {
        Dynamic::Bool(flag) => Some(flag),
        _ => None,
    }
}

fn pick_text(value: Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `isCommandTryoutPayload`, devolvendo o payload tipado.
pub fn command_tryout_payload_of(value: &Dynamic) -> Option<CommandTryoutPayload> {
    if !is_object_like(value) {
        return None;
    }
    let command_id = text_of(value, "commandId").filter(|id| !id.is_empty())?;
    let arguments = optional_of(value, "arguments", |arguments| match arguments {
        Dynamic::Array(items) => Some(items.to_vec()),
        _ => None,
    })?;
    let capture_target_scope = optional_of(value, "captureTargetScope", pick_bool)?;
    Some(CommandTryoutPayload { command_id, arguments, capture_target_scope })
}

/// `isCommandTryoutPayload`.
pub fn is_command_tryout_payload(value: &Dynamic) -> bool {
    command_tryout_payload_of(value).is_some()
}

/// `isViewTryoutPayload`, devolvendo o payload tipado.
pub fn view_tryout_payload_of(value: &Dynamic) -> Option<ViewTryoutPayload> {
    if !is_object_like(value) {
        return None;
    }
    let id = text_of(value, "id").filter(|id| !id.is_empty())?;
    let target = match text_of(value, "target")?.as_str() {
        "view" => ViewTryoutTarget::View,
        "container" => ViewTryoutTarget::Container,
        _ => return None,
    };
    let focus = optional_of(value, "focus", pick_bool)?;
    Some(ViewTryoutPayload { id, target, focus })
}

/// `isViewTryoutPayload`.
pub fn is_view_tryout_payload(value: &Dynamic) -> bool {
    view_tryout_payload_of(value).is_some()
}

/// `isEditorSampleTryoutPayload`, devolvendo o payload tipado.
pub fn editor_sample_tryout_payload_of(value: &Dynamic) -> Option<EditorSampleTryoutPayload> {
    if !is_object_like(value) {
        return None;
    }
    let title = text_of(value, "title")?;
    let language_id = optional_of(value, "languageId", pick_text)?;
    match text_of(value, "type")?.as_str() {
        "text" => Some(EditorSampleTryoutPayload::Text { title, text: text_of(value, "text")?, language_id }),
        "diff" => Some(EditorSampleTryoutPayload::Diff {
            title,
            original: text_of(value, "original")?,
            modified: text_of(value, "modified")?,
            language_id,
        }),
        _ => None,
    }
}

/// `isEditorSampleTryoutPayload`.
pub fn is_editor_sample_tryout_payload(value: &Dynamic) -> bool {
    editor_sample_tryout_payload_of(value).is_some()
}

/// `isGuidedTryoutPayload`.
pub fn is_guided_tryout_payload(value: &Dynamic) -> bool {
    guided_tryout_payload_of(value).is_some()
}

/// O payload guiado de um `payload` de cenário, se for um.
pub fn guided_tryout_payload_of(value: &Dynamic) -> Option<Rc<GuidedTryoutPayload>> {
    match value {
        Dynamic::Object(object) => object.host::<GuidedTryoutPayload>(),
        _ => None,
    }
}

/// O `IOnboardingTryoutPresentationDefinition<TPayload>` de uma apresentação com payload tipado: o `isPayload` é a
/// conversão do valor dinâmico, e o payload que a conversão aceitou chega tipado a `get_availability` e `prepare`.
pub fn typed_presentation_definition<P: 'static>(
    kind: &str,
    on_did_change_availability: Event<()>,
    parse: fn(&Dynamic) -> Option<P>,
    get_availability: impl Fn(&P) -> OnboardingTryoutAvailability + 'static,
    prepare: impl Fn(&P, &OnboardingTryoutRunContext) -> Promise<OnboardingTryoutPreparation> + 'static,
) -> OnboardingTryoutPresentationDefinition {
    let typed = move |value: &Dynamic| parse(value).unwrap_or_else(|| errors::throw(Error::new("Invalid tryout payload.")));
    let typed_for_prepare = typed;
    OnboardingTryoutPresentationDefinition {
        kind: kind.to_string(),
        on_did_change_availability: Some(on_did_change_availability),
        is_payload: Rc::new(move |value: &Dynamic| parse(value).is_some()),
        get_availability: Rc::new(move |value: &Dynamic| get_availability(&typed(value))),
        prepare: Rc::new(move |value: &Dynamic, context: &OnboardingTryoutRunContext| prepare(&typed_for_prepare(value), context)),
    }
}

fn unavailable(message: String) -> OnboardingTryoutUnavailable {
    OnboardingTryoutUnavailable { message, action: None }
}

fn unavailable_availability(message: &str, key: &str) -> OnboardingTryoutAvailability {
    OnboardingTryoutAvailability::Unavailable(unavailable(localize(key, message, &[])))
}

/// O `{ kind: 'unavailable', message }` que o `run` devolve no lugar do resultado.
fn as_result(availability: OnboardingTryoutAvailability) -> Option<OnboardingTryoutResult> {
    match availability {
        OnboardingTryoutAvailability::Unavailable(reason) => Some(OnboardingTryoutResult::Unavailable(reason)),
        _ => None,
    }
}

fn command_availability(context_keys: &dyn IContextKeyService, payload: &CommandTryoutPayload) -> OnboardingTryoutAvailability {
    let precondition = menu_registry().get_command(&payload.command_id).and_then(|command| command.precondition);
    if payload.command_id == RUN_ONBOARDING_TRYOUT_COMMAND_ID
        || commands_registry().get_command(&payload.command_id).is_none()
        || precondition.as_ref().is_some_and(|rules| !context_keys.context_matches_rules(Some(rules)))
    {
        return unavailable_availability("This command is not available in the current context.", "onboarding.tryout.commandUnavailable");
    }
    OnboardingTryoutAvailability::Ready
}

/// `CommandTryoutPresentation`.
pub struct CommandTryoutPresentation {
    command_service: Rc<dyn ICommandService>,
    context_key_service: Rc<dyn IContextKeyService>,
}

impl CommandTryoutPresentation {
    pub fn new(command_service: Rc<dyn ICommandService>, context_key_service: Rc<dyn IContextKeyService>) -> Rc<Self> {
        Rc::new(Self { command_service, context_key_service })
    }

    /// `onDidChangeAvailability`.
    pub fn on_did_change_availability(&self) -> Event<()> {
        event::any(vec![
            self.context_key_service.on_did_change_context().map(|_| (), None),
            commands_registry().on_did_register_command().map(|_| (), None),
        ])
    }

    pub fn get_availability(&self, payload: &CommandTryoutPayload) -> OnboardingTryoutAvailability {
        command_availability(&*self.context_key_service, payload)
    }

    pub fn prepare(&self, payload: &CommandTryoutPayload, context: &OnboardingTryoutRunContext) -> Promise<OnboardingTryoutPreparation> {
        let (payload, token) = (payload.clone(), context.token.clone());
        let (command_service, context_keys) = (self.command_service.clone(), self.context_key_service.clone());
        Promise::resolved(OnboardingTryoutPreparation::Ready {
            run: Rc::new(move || {
                let (payload, token) = (payload.clone(), token.clone());
                let (command_service, context_keys) = (command_service.clone(), context_keys.clone());
                async_fn(async move {
                    if token.is_cancellation_requested() {
                        return Ok(OnboardingTryoutResult::Cancelled);
                    }
                    if let Some(result) = as_result(command_availability(&*context_keys, &payload)) {
                        return Ok(result);
                    }
                    let result = command_service.execute_command(&payload.command_id, payload.arguments.clone().unwrap_or_default()).await?;
                    if payload.capture_target_scope != Some(true) {
                        return Ok(OnboardingTryoutResult::Launched { kind: OnboardingTryoutLaunchKind::Executed, target_scope: None });
                    }
                    let target_scope = match &result {
                        Dynamic::Object(_) => pick_text(result.get_str("targetScope")).filter(|scope| is_onboarding_target_scope(scope)),
                        _ => None,
                    };
                    Ok(match target_scope {
                        Some(scope) => OnboardingTryoutResult::Launched { kind: OnboardingTryoutLaunchKind::Executed, target_scope: Some(scope) },
                        None => OnboardingTryoutResult::Unavailable(unavailable(localize(
                            "onboarding.tryout.targetUnavailable",
                            "The example opened, but its target is no longer available.",
                            &[],
                        ))),
                    })
                })
            }),
        })
    }

    /// O `IOnboardingTryoutPresentationDefinition` desta apresentação.
    pub fn definition(self: &Rc<Self>) -> OnboardingTryoutPresentationDefinition {
        let (for_availability, for_prepare) = (self.clone(), self.clone());
        typed_presentation_definition(
            COMMAND_TRYOUT_PRESENTATION_KIND,
            self.on_did_change_availability(),
            command_tryout_payload_of,
            move |payload| for_availability.get_availability(payload),
            move |payload, context| for_prepare.prepare(payload, context),
        )
    }
}

fn view_availability(
    view_descriptor_service: &dyn IViewDescriptorService,
    context_keys: &dyn IContextKeyService,
    payload: &ViewTryoutPayload,
) -> OnboardingTryoutAvailability {
    let available = match payload.target {
        ViewTryoutTarget::View => view_descriptor_service
            .get_view_descriptor_by_id(&payload.id)
            .is_some_and(|descriptor| descriptor.when.as_ref().is_none_or(|rules| context_keys.context_matches_rules(Some(rules)))),
        ViewTryoutTarget::Container => view_descriptor_service.get_view_container_by_id(&payload.id).is_some(),
    };
    if available {
        OnboardingTryoutAvailability::Ready
    } else {
        unavailable_availability("This view is not available in the current window.", "onboarding.tryout.viewUnavailable")
    }
}

/// `ViewTryoutPresentation`.
pub struct ViewTryoutPresentation {
    views_service: Rc<dyn IViewsService>,
    view_descriptor_service: Rc<dyn IViewDescriptorService>,
    context_key_service: Rc<dyn IContextKeyService>,
}

impl ViewTryoutPresentation {
    pub fn new(
        views_service: Rc<dyn IViewsService>,
        view_descriptor_service: Rc<dyn IViewDescriptorService>,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Rc<Self> {
        Rc::new(Self { views_service, view_descriptor_service, context_key_service })
    }

    /// `onDidChangeAvailability`.
    pub fn on_did_change_availability(&self) -> Event<()> {
        event::any(vec![
            self.view_descriptor_service.on_did_change_view_containers().map(|_| (), None),
            self.view_descriptor_service.on_did_change_container().map(|_| (), None),
            self.context_key_service.on_did_change_context().map(|_| (), None),
        ])
    }

    pub fn get_availability(&self, payload: &ViewTryoutPayload) -> OnboardingTryoutAvailability {
        view_availability(&*self.view_descriptor_service, &*self.context_key_service, payload)
    }

    pub fn prepare(&self, payload: &ViewTryoutPayload, context: &OnboardingTryoutRunContext) -> Promise<OnboardingTryoutPreparation> {
        let (payload, token) = (payload.clone(), context.token.clone());
        let (views_service, descriptors, context_keys) =
            (self.views_service.clone(), self.view_descriptor_service.clone(), self.context_key_service.clone());
        Promise::resolved(OnboardingTryoutPreparation::Ready {
            run: Rc::new(move || {
                let (payload, token) = (payload.clone(), token.clone());
                let (views_service, descriptors, context_keys) = (views_service.clone(), descriptors.clone(), context_keys.clone());
                async_fn(async move {
                    if token.is_cancellation_requested() {
                        return Ok(OnboardingTryoutResult::Cancelled);
                    }
                    if let Some(result) = as_result(view_availability(&*descriptors, &*context_keys, &payload)) {
                        return Ok(result);
                    }
                    let focus = payload.focus.unwrap_or(true);
                    let opened = match payload.target {
                        ViewTryoutTarget::View => views_service.try_open_view(&payload.id, focus).await?,
                        ViewTryoutTarget::Container => views_service.open_view_container(&payload.id, focus).await?,
                    };
                    Ok(if opened {
                        OnboardingTryoutResult::Launched { kind: OnboardingTryoutLaunchKind::Opened, target_scope: None }
                    } else {
                        OnboardingTryoutResult::Unavailable(unavailable(localize(
                            "onboarding.tryout.viewFailed",
                            "The example's view could not be opened.",
                            &[],
                        )))
                    })
                })
            }),
        })
    }

    /// O `IOnboardingTryoutPresentationDefinition` desta apresentação.
    pub fn definition(self: &Rc<Self>) -> OnboardingTryoutPresentationDefinition {
        let (for_availability, for_prepare) = (self.clone(), self.clone());
        typed_presentation_definition(
            VIEW_TRYOUT_PRESENTATION_KIND,
            self.on_did_change_availability(),
            view_tryout_payload_of,
            move |payload| for_availability.get_availability(payload),
            move |payload, context| for_prepare.prepare(payload, context),
        )
    }
}
