//! Dublês dos serviços que a `NotebookOutlineEntryFactory` recebe nos testes de outline que nunca carregam símbolos de célula
//! (`notebookOutline.test.ts` e `notebookStickyScroll.test.ts`): no original o `instantiationService` entrega os serviços reais, que
//! só entram em jogo no `cacheSymbols`, e esses casos leem as entradas antes dele (ou sem ele).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::outline_model::{IOutlineModelService, OutlineModel};
use je_editor::text_model::TextModel;
use je_workbench::services::textmodel_resolver::resolver_service::{ITextModelService, TextModelReference};

/// O `IOutlineModelService` que a fábrica recebe e nenhum caso usa (os símbolos não são carregados).
pub struct UnusedOutlineModelService;

impl IOutlineModelService for UnusedOutlineModelService {
    fn get_or_create(&self, _model: &Rc<TextModel>, _token: &CancellationToken) -> Promise<Rc<OutlineModel>> {
        unreachable!("os símbolos de célula não são carregados neste caso")
    }

    fn get_debounce_value(&self, _text_model: &TextModel) -> f64 {
        unreachable!("os símbolos de célula não são carregados neste caso")
    }

    fn get_cached_models(&self) -> Vec<Rc<OutlineModel>> {
        Vec::new()
    }
}

/// O `ITextModelService` que a fábrica recebe e nenhum caso usa.
pub struct UnusedTextModelService;

impl ITextModelService for UnusedTextModelService {
    fn create_model_reference(&self, _resource: &URI) -> Promise<TextModelReference> {
        unreachable!("os símbolos de célula não são carregados neste caso")
    }
}
