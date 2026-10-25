//! Porte de `src/vs/workbench/services/search/node/ripgrepSearchProvider.ts`: o `RipgrepSearchProvider`, o provedor de
//! texto que roda o `rg` em cada pasta da busca.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `getNumThreads: () => Promise<number | undefined>` é o [`GetNumThreadsFn`]; o `OutputChannel` é qualquer
//!   [`IOutputChannel`] (o `OutputChannel` de `ripgrep_search_utils` é o do host de extensões).
//! - `process.once('exit', () => this.dispose())` não tem equivalente em Rust (não há gancho de saída sem `unsafe`): o
//!   [`RipgrepSearchProvider::dispose`] continua existindo e o hospedeiro o chama no encerramento, como o `kill_all_cmds` de
//!   `file_walker`.
//! - `mergedTokenSource(token)` (uma fonte nova cancelada pelo token) é um `CancellationTokenSource` filho do token; ao fim da
//!   busca (com sucesso ou erro) a fonte sai do conjunto `inProgress` e solta o ouvinte do pai, o que o original deixa
//!   pendurado (sobre erro, o `Set` do original guarda a fonte para sempre).
//! - A "tradução" de `vscode-userdata` para `file` do original (`{ ...extendedOptions, folder: folder.with({ scheme: file }) }`)
//!   acrescenta uma propriedade `folder` que o `RipgrepTextSearchEngine` nunca lê (ele usa `folderOptions.folder`, que
//!   continua no esquema `vscode-userdata`, para o `cwd` e a raiz dos resultados): só a volta dos resultados para o esquema
//!   de origem tem efeito, e é ela que o `progressTranslator` faz.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::js_string::JsString;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{Field, UriChange};
use je_platform::progress::progress::{IProgress, Progress};

use super::ripgrep_search_utils::IOutputChannel;
use super::ripgrep_text_search_engine::{RipgrepTextSearchEngine, merge_text_search_completes};
use super::search_ext_types::{ProviderResult, TextSearchComplete2, TextSearchProvider2, TextSearchProviderOptions, TextSearchQuery2, TextSearchResult2};
use super::search_ext_types_internal::RipgrepTextSearchOptions;

/// `getNumThreads: () => Promise<number | undefined>`.
pub type GetNumThreadsFn = Rc<dyn Fn() -> Promise<Option<i64>>>;

/// O `inProgress` do provedor, compartilhado com as buscas em andamento.
#[derive(Default)]
struct InProgress {
    sources: RefCell<HashMap<u64, CancellationTokenSource>>,
    next_id: Cell<u64>,
}

impl InProgress {
    /// `withToken`.
    fn with_token<T: Clone + 'static>(self: &Rc<Self>, token: &CancellationToken, fn_: impl FnOnce(CancellationToken) -> Promise<T>) -> Promise<T> {
        let merged = CancellationTokenSource::new(Some(token));
        let id = self.next_id.replace(self.next_id.get() + 1);
        self.sources.borrow_mut().insert(id, merged.clone());
        let result = fn_(merged.token());

        let this = self.clone();
        result.finally(move || {
            this.sources.borrow_mut().remove(&id);
            merged.dispose_with(false);
        })
    }
}

/// O `uri.with({ scheme })` de `{ ...data, uri }` do `progressTranslator`.
fn with_uri_scheme(result: TextSearchResult2, scheme: &JsString) -> TextSearchResult2 {
    let change = UriChange { scheme: Field::Set(scheme.clone()), ..UriChange::default() };
    match result {
        TextSearchResult2::Match(mut text_match) => {
            text_match.uri = text_match.uri.with(&change);
            TextSearchResult2::Match(text_match)
        }
        TextSearchResult2::Context(mut context) => {
            context.uri = context.uri.with(&change);
            TextSearchResult2::Context(context)
        }
    }
}

/// `RipgrepSearchProvider`.
pub struct RipgrepSearchProvider {
    output_channel: Rc<dyn IOutputChannel>,
    get_num_threads: GetNumThreadsFn,
    in_progress: Rc<InProgress>,
}

impl RipgrepSearchProvider {
    pub fn new(output_channel: impl IOutputChannel + 'static, get_num_threads: GetNumThreadsFn) -> Self {
        Self { output_channel: Rc::new(output_channel), get_num_threads, in_progress: Rc::default() }
    }

    /// `dispose` (privado no original, chamado por `process.once('exit')`): cancela as buscas em andamento.
    pub fn dispose(&self) {
        let sources: Vec<CancellationTokenSource> = self.in_progress.sources.borrow().values().cloned().collect();
        for engine in sources {
            engine.cancel();
        }
    }
}

impl TextSearchProvider2 for RipgrepSearchProvider {
    fn provide_text_search_results(
        &self,
        query: &TextSearchQuery2,
        options: &TextSearchProviderOptions,
        progress: Rc<dyn IProgress<TextSearchResult2>>,
        token: &CancellationToken,
    ) -> ProviderResult<TextSearchComplete2> {
        let (output_channel, in_progress) = (self.output_channel.clone(), self.in_progress.clone());
        let (query, options, token) = (query.clone(), options.clone(), token.clone());
        (self.get_num_threads)().then_promise(move |num_threads| {
            let engine = RipgrepTextSearchEngine::new(output_channel, num_threads);
            let query = Rc::new(RefCell::new(query));

            let completes: Vec<Promise<TextSearchComplete2>> = options
                .folder_options
                .iter()
                .map(|folder_option| {
                    let extended_options = RipgrepTextSearchOptions {
                        folder_options: folder_option.clone(),
                        num_threads,
                        max_results: Some(options.max_results),
                        preview_options: Some(options.preview_options),
                        max_file_size: options.max_file_size,
                        surrounding_context: Some(options.surrounding_context),
                    };
                    if folder_option.folder.scheme() == schemas::VSCODE_USER_DATA {
                        // Ripgrep search engine can only provide file-scheme results, but we want to use it to search some schemes that are backed by the filesystem, but with some other provider as the frontend,
                        // case in point vscode-userdata. In these cases we translate the query to a file, and translate the results back to the frontend scheme.
                        let scheme = folder_option.folder.scheme().clone();
                        let target = progress.clone();
                        let progress_translator: Rc<dyn IProgress<TextSearchResult2>> =
                            Rc::new(Progress::new(move |data: TextSearchResult2| target.report(with_uri_scheme(data, &scheme))));
                        in_progress.with_token(&token, |token| engine.provide_text_search_results_with_rg_options(&query, extended_options, progress_translator, token))
                    } else {
                        in_progress.with_token(&token, |token| engine.provide_text_search_results_with_rg_options(&query, extended_options, progress.clone(), token))
                    }
                })
                .collect();

            Promise::all(completes).then(|completes| Some(merge_text_search_completes(&completes)))
        })
    }
}
