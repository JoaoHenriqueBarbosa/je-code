//! Porte de `src/vs/workbench/api/browser/mainThreadLanguages.ts`: a ponte entre o `ILanguageService` do workbench e `vscode.languages`
//! (lista de linguagens, troca de linguagem de um documento, token numa posição, realce de sintaxe completo e itens de status de
//! linguagem).
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadLanguages)` e o `IExtHostContext` do construtor: o construtor recebe o
//!   [`IMainContext`] e pede o proxy de `ExtHostLanguages`.
//! - `ILanguageStatusService` (`addStatus`) não está portado: fica como o trait estreito [`ILanguageStatusRegistry`].
//! - O que o original faz com o `IModelService` (os tokens de uma linha, `tokenizeIfCheap`, o tipo padrão de token) e com o
//!   `ITextMateTokenizationService` e o `TokenizationRegistry` (`$computeFullSyntaxHighlighting`) depende de partes de tokenização que
//!   o porte expõe por outro caminho: ficam atrás do trait [`ILanguageTokens`], que o aplicativo implementa sobre o modelo de texto e a
//!   gramática TextMate (`tokens_at_position` devolve o `{ type, range }` já em coordenadas de linha e coluna de base 1).
//! - `$setLanguageStatus` e `$removeLanguageStatus` (`DisposableMap<number>`) são um mapa de [`DisposableRef`] por handle.
//! - `async` de `$changeLanguage`: a promessa do `createModelReference` encadeia o `setLanguage` e a liberação da referência, que
//!   roda também quando o `setLanguage` falha (o `finally`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_service::ILanguageService;
use je_editor::position::Position as EditorPosition;
use je_editor::services::theme_service::IThemeService;

use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostLanguagesShape, ILanguageStatus, IMainContext, ISyntaxHighlightingResultDto, MainThreadLanguagesShape, TokenAtPositionDto,
};
use crate::services::textmodel_resolver::resolver_service::ITextModelService;

/// `ILanguageStatusService.addStatus(status)`: o registro devolve o descartável que retira o status.
pub trait ILanguageStatusRegistry {
    fn add_status(&self, status: ILanguageStatus) -> DisposableRef;
}

/// A tokenização que o original lê do modelo de texto e da gramática (ver as divergências do módulo).
pub trait ILanguageTokens {
    /// `$tokensAtPosition`: `None` quando não há modelo para o recurso.
    fn tokens_at_position(&self, resource: &URI, position: EditorPosition) -> Option<TokenAtPositionDto>;

    /// `$computeFullSyntaxHighlighting`.
    fn compute_full_syntax_highlighting(&self, source: &str, language_id: &str) -> Promise<ISyntaxHighlightingResultDto>;
}

/// `MainThreadLanguages`.
pub struct MainThreadLanguages {
    base: Rc<Disposable>,
    language_service: Rc<dyn ILanguageService>,
    resolver_service: Rc<dyn ITextModelService>,
    language_status_service: Rc<dyn ILanguageStatusRegistry>,
    tokens: Rc<dyn ILanguageTokens>,
    status: RefCell<HashMap<u32, DisposableRef>>,
}

impl MainThreadLanguages {
    pub fn new(
        ext_host_context: &dyn IMainContext,
        language_service: Rc<dyn ILanguageService>,
        resolver_service: Rc<dyn ITextModelService>,
        language_status_service: Rc<dyn ILanguageStatusRegistry>,
        tokens: Rc<dyn ILanguageTokens>,
        theme_service: Rc<dyn IThemeService>,
    ) -> Rc<Self> {
        let proxy: Rc<dyn ExtHostLanguagesShape> = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_LANGUAGES);
        let base = Disposable::new();

        proxy.rpc_accept_language_ids(language_service.get_registered_language_ids());
        {
            let (proxy, language_service) = (proxy.clone(), language_service.clone());
            base.register(language_service.clone().on_did_change().listen(move |_| proxy.rpc_accept_language_ids(language_service.get_registered_language_ids())));
        }
        {
            let proxy = proxy.clone();
            base.register(theme_service.on_did_color_theme_change().listen(move |_| proxy.rpc_accept_syntax_highlighting_theme_changed()));
        }

        Rc::new(Self { base, language_service, resolver_service, language_status_service, tokens, status: RefCell::new(HashMap::new()) })
    }
}

impl IDisposable for MainThreadLanguages {
    fn dispose(&self) {
        for (_, status) in self.status.borrow_mut().drain() {
            status.dispose();
        }
        self.base.dispose();
    }
}

impl MainThreadLanguagesShape for MainThreadLanguages {
    fn rpc_change_language(&self, resource: URI, language_id: &str) -> Promise<()> {
        if !self.language_service.is_registered_language_id(Some(language_id)) {
            return Promise::rejected(Error::new(format!("Unknown language id: {language_id}")));
        }

        let language_service = self.language_service.clone();
        let language_id = language_id.to_owned();
        self.resolver_service.create_model_reference(&resource).then_result(move |outcome| {
            let reference = outcome?;
            let result = errors::catch(|| {
                if let Some(model) = reference.object.text_editor_model() {
                    // `setLanguage(this._languageService.createById(languageId))`
                    let selection = language_service.create_by_id(Some(&language_id));
                    model.set_language(&selection.language_id(), None);
                }
            });
            reference.dispose();
            result
        })
    }

    fn rpc_tokens_at_position(&self, resource: URI, position: EditorPosition) -> Promise<Option<TokenAtPositionDto>> {
        Promise::resolved(self.tokens.tokens_at_position(&resource, position))
    }

    fn rpc_compute_full_syntax_highlighting(&self, source: &str, language_id: &str) -> Promise<ISyntaxHighlightingResultDto> {
        self.tokens.compute_full_syntax_highlighting(source, language_id)
    }

    // --- language status

    fn rpc_set_language_status(&self, handle: u32, status: ILanguageStatus) {
        let registered = self.language_status_service.add_status(status);
        // `DisposableMap.set` descarta o anterior do mesmo handle
        if let Some(previous) = self.status.borrow_mut().insert(handle, registered) {
            previous.dispose();
        }
    }

    fn rpc_remove_language_status(&self, handle: u32) {
        if let Some(status) = self.status.borrow_mut().remove(&handle) {
            status.dispose();
        }
    }
}
