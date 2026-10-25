//! Porte de `src/vs/workbench/contrib/onboarding/browser/onboardingSamplePresentation.ts`: a apresentação que prepara um
//! texto ou uma comparação de exemplo como modelo de editor virtual (esquema `vscode-onboarding-sample`) e o abre sem
//! tocar em arquivos do usuário.
//!
//! Divergências:
//! - O payload é o [`EditorSampleTryoutPayload`] tipado; [`EditorSampleTryoutPresentation::definition`] monta o
//!   `IOnboardingTryoutPresentationDefinition` com o `isPayload` do original.
//! - `thenRegisterOrDispose(createModelReference(...), store)` é a espera da referência seguida de "descarta se o `store`
//!   já foi descartado, senão guarda nele" (a [`Reference`] é o `IDisposable`).
//! - `provideTextContent` é [`EditorSampleTryoutPresentation::provide`], que devolve o erro como `Result`; o
//!   [`ITextModelContentProvider`] converte o erro em promessa rejeitada.
//! - `extends Disposable` é o `Disposable` guardado em `base`.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::{URI, UriComponents};
use je_editor::language_service::ILanguageService;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_platform::opener::editor_types::EditorOptions;

use super::onboarding_tryout::{
    OnboardingTryoutAvailability, OnboardingTryoutLaunchKind, OnboardingTryoutPreparation, OnboardingTryoutResult,
    OnboardingTryoutRunContext, OnboardingTryoutUnavailable, is_onboarding_tryout_id,
};
use super::onboarding_tryout_actions::{EditorSampleTryoutPayload, editor_sample_tryout_payload_of, typed_presentation_definition};
use super::onboarding_tryout_service::{IOnboardingTryoutService, OnboardingTryoutPresentationDefinition};
use crate::common::editor::{TextEditorOptions, UntypedEditorInput};
use crate::services::editor::editor_service::IEditorService;
use crate::services::textmodel_resolver::resolver_service::{ITextModelContentProvider, ITextModelService};

pub const EDITOR_SAMPLE_TRYOUT_PRESENTATION_KIND: &str = "editorSample";

fn unavailable(key: &str, message: &str) -> OnboardingTryoutUnavailable {
    OnboardingTryoutUnavailable { message: localize(key, message, &[]), action: None }
}

fn sample_unavailable_error() -> Error {
    Error::new(localize(
        "onboarding.tryout.sampleUnavailable",
        "This sample is no longer available. Open its feature example from the release notes again.",
        &[],
    ))
}

/// `URI.from({ scheme: Schemas.vscodeOnboardingSample, path: '/<id>/<part>' })`.
fn sample_uri(id: &str, part: &str) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(schemas::VSCODE_ONBOARDING_SAMPLE)),
            path: Some(JsString::from(format!("/{id}/{part}"))),
            ..UriComponents::default()
        },
        false,
    )
}

/// `thenRegisterOrDispose(this.textModelService.createModelReference(uri), store)`.
async fn reference_into(text_model_service: &dyn ITextModelService, uri: &URI, store: &DisposableStore) -> Result<(), Error> {
    let reference = text_model_service.create_model_reference(uri).await?;
    if store.is_disposed() {
        reference.dispose();
    } else {
        store.add(Rc::new(reference));
    }
    Ok(())
}

/// `EditorSampleTryoutPresentation`.
pub struct EditorSampleTryoutPresentation {
    base: Rc<Disposable>,
    editor_service: Rc<dyn IEditorService>,
    text_model_service: Rc<dyn ITextModelService>,
    model_service: Rc<dyn IModelService>,
    language_service: Rc<dyn ILanguageService>,
    tryout_service: Rc<dyn IOnboardingTryoutService>,
}

impl EditorSampleTryoutPresentation {
    pub fn new(
        editor_service: Rc<dyn IEditorService>,
        text_model_service: Rc<dyn ITextModelService>,
        model_service: Rc<dyn IModelService>,
        language_service: Rc<dyn ILanguageService>,
        tryout_service: Rc<dyn IOnboardingTryoutService>,
    ) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), editor_service, text_model_service, model_service, language_service, tryout_service })
    }

    /// `onDidChangeAvailability`.
    pub fn on_did_change_availability(&self) -> Event<()> {
        self.language_service.on_did_change()
    }

    pub fn get_availability(&self, payload: &EditorSampleTryoutPayload) -> OnboardingTryoutAvailability {
        match payload.language_id().filter(|id| !id.is_empty()) {
            Some(language_id) if !self.language_service.is_registered_language_id(Some(language_id)) => {
                OnboardingTryoutAvailability::Unavailable(unavailable(
                    "onboarding.tryout.languageUnavailable",
                    "Language support for this example is not available yet.",
                ))
            }
            _ => OnboardingTryoutAvailability::Ready,
        }
    }

    pub fn prepare(&self, payload: &EditorSampleTryoutPayload, context: &OnboardingTryoutRunContext) -> Promise<OnboardingTryoutPreparation> {
        let payload = payload.clone();
        let (id, store, token) = (context.id.clone(), context.store.clone(), context.token.clone());
        let (editor_service, text_model_service) = (self.editor_service.clone(), self.text_model_service.clone());
        async_fn(async move {
            let diff = matches!(payload, EditorSampleTryoutPayload::Diff { .. });
            let original = sample_uri(&id, if diff { "original" } else { "text" });
            let modified = diff.then(|| sample_uri(&id, "modified"));

            reference_into(&*text_model_service, &original, &store).await?;
            if token.is_cancellation_requested() || store.is_disposed() {
                return Ok(OnboardingTryoutPreparation::Cancelled);
            }
            if let Some(modified) = &modified {
                reference_into(&*text_model_service, modified, &store).await?;
            }
            if token.is_cancellation_requested() || store.is_disposed() {
                return Ok(OnboardingTryoutPreparation::Cancelled);
            }

            Ok(OnboardingTryoutPreparation::Ready {
                run: Rc::new(move || {
                    let (payload, token, original, modified) = (payload.clone(), token.clone(), original.clone(), modified.clone());
                    let editor_service = editor_service.clone();
                    async_fn(async move {
                        if token.is_cancellation_requested() {
                            return Ok(OnboardingTryoutResult::Cancelled);
                        }
                        let options = Some(TextEditorOptions::from(EditorOptions { pinned: Some(true), ..EditorOptions::default() }));
                        let resource_input = |resource: URI| UntypedEditorInput { resource: Some(resource), ..UntypedEditorInput::default() };
                        let input = match modified {
                            Some(modified) => UntypedEditorInput {
                                original: Some(Box::new(resource_input(original))),
                                modified: Some(Box::new(resource_input(modified))),
                                label: Some(payload.title().to_string()),
                                options,
                                ..UntypedEditorInput::default()
                            },
                            None => UntypedEditorInput {
                                resource: Some(original),
                                label: Some(payload.title().to_string()),
                                options,
                                ..UntypedEditorInput::default()
                            },
                        };
                        let pane = editor_service.open_editor(input).await?;
                        Ok(match pane {
                            Some(_) => OnboardingTryoutResult::Launched { kind: OnboardingTryoutLaunchKind::Opened, target_scope: None },
                            None => OnboardingTryoutResult::Unavailable(unavailable(
                                "onboarding.tryout.sampleFailed",
                                "The example editor could not be opened.",
                            )),
                        })
                    })
                }),
            })
        })
    }

    /// `provideTextContent(resource)`: só resolve conteúdo de exemplo registrado.
    pub fn provide(&self, resource: &URI) -> Result<Rc<TextModel>, Error> {
        let path = resource.path().to_string_lossy();
        let mut parts = path.split('/').skip(1);
        let (id, part, has_extra) = (parts.next(), parts.next(), parts.next().is_some());
        let blank = |text: &JsString| text.to_string_lossy().is_empty();
        let (Some(id), Some(part)) = (id, part) else { return Err(sample_unavailable_error()) };
        if resource.scheme().to_string_lossy() != schemas::VSCODE_ONBOARDING_SAMPLE
            || !blank(resource.authority())
            || !blank(resource.query())
            || !blank(resource.fragment())
            || has_extra
            || !is_onboarding_tryout_id(id)
            || !matches!(part, "text" | "original" | "modified")
        {
            return Err(sample_unavailable_error());
        }
        if let Some(model) = self.model_service.get_model(resource) {
            return Ok(model);
        }
        let scenario = self.tryout_service.get_tryout(id);
        let payload = scenario
            .as_ref()
            .filter(|scenario| scenario.presentation.kind == EDITOR_SAMPLE_TRYOUT_PRESENTATION_KIND)
            .and_then(|scenario| editor_sample_tryout_payload_of(&scenario.presentation.payload))
            .ok_or_else(sample_unavailable_error)?;
        let text = match (&payload, part) {
            (EditorSampleTryoutPayload::Text { text, .. }, "text") => text,
            (EditorSampleTryoutPayload::Diff { original, .. }, "original") => original,
            (EditorSampleTryoutPayload::Diff { modified, .. }, "modified") => modified,
            _ => return Err(sample_unavailable_error()),
        };
        let selection = self.language_service.create_by_id(Some(payload.language_id().unwrap_or("plaintext")));
        Ok(self.model_service.create_model(TextSource::from(text.as_str()), Some(&*selection), Some(resource), false))
    }

    /// O `IOnboardingTryoutPresentationDefinition` desta apresentação.
    pub fn definition(self: &Rc<Self>) -> OnboardingTryoutPresentationDefinition {
        let (for_availability, for_prepare) = (self.clone(), self.clone());
        typed_presentation_definition(
            EDITOR_SAMPLE_TRYOUT_PRESENTATION_KIND,
            self.on_did_change_availability(),
            editor_sample_tryout_payload_of,
            move |payload| for_availability.get_availability(payload),
            move |payload, context| for_prepare.prepare(payload, context),
        )
    }
}

impl ITextModelContentProvider for EditorSampleTryoutPresentation {
    fn provide_text_content(&self, resource: &URI) -> MaybePromise<Option<Rc<TextModel>>> {
        match self.provide(resource) {
            Ok(model) => MaybePromise::Value(Some(model)),
            Err(error) => MaybePromise::Promise(Promise::rejected(error)),
        }
    }
}

impl IDisposable for EditorSampleTryoutPresentation {
    fn dispose(&self) {
        self.base.dispose();
    }
}
