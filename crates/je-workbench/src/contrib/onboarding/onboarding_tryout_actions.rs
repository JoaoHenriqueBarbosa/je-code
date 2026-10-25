//! Porte de `src/vs/workbench/contrib/onboarding/common/onboardingTryoutActions.ts`: o payload do tryout guiado e o
//! validador dele.
//!
//! Divergências:
//! - **Só o payload guiado.** `ICommandTryoutPayload`, `IViewTryoutPayload`, `EditorSampleTryoutPayload` e os
//!   validadores `isCommandTryoutPayload`, `isViewTryoutPayload` e `isEditorSampleTryoutPayload` entram com as
//!   apresentações que os consomem (comando, view e amostra de editor).
//! - O `payload` de um cenário é `unknown`; o payload guiado viaja como valor nativo de um objeto
//!   (`ObjectRef::with_host`), como os demais payloads do onboarding. Por isso o `isGuidedTryoutPayload` confere o tipo
//!   nativo: a forma (`launch.kind` texto, `steps` com `id`, `kind` e `payload`, `unavailableMessage` texto opcional)
//!   já é garantida pelo tipo.

use je_base::dynamic::Dynamic;

use super::onboarding_scenario::OnboardingPresentationRef;
use super::onboarding_sequence::OnboardingSequenceStep;

/// `IGuidedTryoutPayload`.
#[derive(Debug, Clone, PartialEq)]
pub struct GuidedTryoutPayload {
    pub launch: OnboardingPresentationRef,
    pub steps: Vec<OnboardingSequenceStep>,
    pub unavailable_message: Option<String>,
}

/// `isGuidedTryoutPayload`.
pub fn is_guided_tryout_payload(value: &Dynamic) -> bool {
    guided_tryout_payload_of(value).is_some()
}

/// O payload guiado de um `payload` de cenário, se for um.
pub fn guided_tryout_payload_of(value: &Dynamic) -> Option<std::rc::Rc<GuidedTryoutPayload>> {
    match value {
        Dynamic::Object(object) => object.host::<GuidedTryoutPayload>(),
        _ => None,
    }
}
