//! Porte de `src/vs/workbench/contrib/editTelemetry/test/browser/editSourceCategories.test.ts` (suíte `Edit Telemetry
//! Source Categories`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no início do caso.
//! - O objeto `sources` (chaves em ordem) é uma lista de pares `(chave, origem)`; o `Object.fromEntries(...)` com
//!   `assert.deepStrictEqual` é a comparação, na mesma ordem das chaves, com a lista de pares `(chave, categoria)`.
//! - As categorias textuais (`'otherAI'`, `'inlineCompletionsCopilot'`, ...) são as variantes de
//!   [`EditTelemetryCategory`]. `new ProviderId(a, b, c)` é `ProviderId::new(Some(a), Some(b), Some(c))`; os campos
//!   `undefined` das fábricas são `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::code_action::languages::ProviderId;
use je_editor::text_model_edit_source::{ChatApplyEditsData, EditSources, InlineCompletionAcceptData};
use je_workbench::contrib::edit_telemetry::browser::helpers::document_with_annotated_edits::EditSourceBase;
use je_workbench::contrib::edit_telemetry::browser::telemetry::edit_source_tracking_impl::{EditTelemetryCategory, get_edit_telemetry_category};

fn provider(extension_id: &str, provider_id: &str) -> Option<ProviderId> {
    Some(ProviderId::new(Some(extension_id.to_owned()), Some("1.0.0".to_owned()), Some(provider_id.to_owned())))
}

fn accept(nes: bool, request_uuid: &str, provider_id: Option<ProviderId>) -> je_editor::text_model_edit_source::TextModelEditSource {
    EditSources::inline_completion_accept(&InlineCompletionAcceptData {
        nes,
        request_uuid: request_uuid.to_owned(),
        language_id: "typescript".to_owned(),
        provider_id,
        correlation_id: None,
    })
}

#[test]
fn maps_every_edit_source_category() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let sources = vec![
        (
            "chat",
            EditSources::chat_apply_edits(&ChatApplyEditsData {
                model_id: None,
                auto_tier: None,
                fusion_model: None,
                session_id: None,
                request_id: None,
                language_id: "typescript".to_owned(),
                mode: Some("agent".to_owned()),
                extension_id: None,
                code_block_suggestion_id: None,
                origin: None,
                harness: None,
                agent_session_id: None,
                chat_session_id: None,
            }),
        ),
        ("copilotCompletion", accept(false, "request-1", provider("github.copilot", "completions"))),
        ("copilotChatCompletion", accept(false, "request-2", provider("github.copilot-chat", "completions"))),
        ("nes", accept(true, "request-3", provider("github.copilot-chat", "nes"))),
        ("inlineNesProvider", accept(false, "request-4", provider("github.copilot-chat", "nes"))),
        ("otherCompletion", accept(false, "request-5", provider("other.extension", "other"))),
        ("user", EditSources::cursor("type", None)),
        ("snippet", EditSources::snippet()),
        ("format", EditSources::unknown(Some("formatEditsCommand"))),
        ("external", EditSources::reload_from_disk()),
        ("unknown", EditSources::unknown(None)),
    ];

    let actual: Vec<(&str, EditTelemetryCategory)> =
        sources.iter().map(|(key, source)| (*key, get_edit_telemetry_category(&EditSourceBase::create(source)))).collect();

    assert_eq!(
        actual,
        vec![
            ("chat", EditTelemetryCategory::OtherAi),
            ("copilotCompletion", EditTelemetryCategory::InlineCompletionsCopilot),
            ("copilotChatCompletion", EditTelemetryCategory::InlineCompletionsCopilot),
            ("nes", EditTelemetryCategory::Nes),
            ("inlineNesProvider", EditTelemetryCategory::InlineCompletionsNes),
            ("otherCompletion", EditTelemetryCategory::InlineCompletionsOther),
            ("user", EditTelemetryCategory::User),
            ("snippet", EditTelemetryCategory::Ide),
            ("format", EditTelemetryCategory::Ide),
            ("external", EditTelemetryCategory::External),
            ("unknown", EditTelemetryCategory::Unknown),
        ]
    );
}
