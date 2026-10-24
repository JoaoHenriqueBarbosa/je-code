//! O trecho de `src/vs/platform/dataChannel/browser/forwardingTelemetryService.ts` que o `editTelemetry` usa:
//! `forwardToChannelIf` e `isCopilotLikeExtension`.
//!
//! Representação: o `forwardToChannelIf(value)` do original devolve um objeto com uma propriedade de chave `Symbol`
//! (`shouldForwardToChannel`) que é espalhada (`...`) no dado do evento; a chave `Symbol` não é enviada pela
//! telemetria e só é lida pelo `DataChannelForwardingTelemetryService`. O [`Dynamic`] do porte não tem chaves
//! `Symbol`, então a marca é um valor à parte ([`ForwardToChannel`]) que acompanha o dado em
//! [`public_log2_forwarding`]. O `DataChannelForwardingTelemetryService` (e o `IDataChannelService`) não foram
//! portados: enquanto isso, a marca não altera o que o serviço de telemetria recebe, exatamente como a chave `Symbol`
//! não altera o que o serviço de telemetria do original envia.

use je_base::dynamic::Dynamic;
use je_platform::telemetry::telemetry::ITelemetryService;

/// A marca de `forwardToChannelIf(value)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForwardToChannel(pub bool);

/// `forwardToChannelIf(value)`.
pub fn forward_to_channel_if(value: bool) -> ForwardToChannel {
    ForwardToChannel(value)
}

/// `isCopilotLikeExtension(extensionId)`.
pub fn is_copilot_like_extension(extension_id: Option<&str>) -> bool {
    let Some(extension_id) = extension_id.filter(|id| !id.is_empty()) else {
        return false;
    };
    let ext_id_lower_case = extension_id.to_lowercase();
    ext_id_lower_case == "github.copilot" || ext_id_lower_case == "github.copilot-chat"
}

/// `telemetryService.publicLog2(eventName, { ...data, ...forwardToChannelIf(forward) })`.
pub fn public_log2_forwarding(telemetry_service: &dyn ITelemetryService, event_name: &str, data: Dynamic, _forward: ForwardToChannel) {
    telemetry_service.public_log2(event_name, Some(data));
}
