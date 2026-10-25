//! Porte de `src/vs/workbench/services/search/node/textSearchAdapter.ts`: o `TextSearchEngineAdapter`, que liga o
//! `NativeTextSearchManager` ao `RipgrepTextSearchEngine` do processo de busca e serializa os resultados.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `pfs` (o terceiro argumento do `NativeTextSearchManager`) é o disco, o padrão de [`NativeTextSearchManager::new`].
//! - `onResult` e `onMessage` são fechos `Rc<dyn Fn>`; o `pretendOutputChannel` é o [`MessageOutputChannel`], que entrega cada
//!   linha como um [`IProgressMessage`].
//! - `new Promise((resolve, reject) => manager.search(...).then(resolve, reject))` é um `then` sobre a promessa da busca.
//! - `match.resource && match.resource.fsPath` é `fs_path(&match.resource)`: o `resource` é obrigatório no tipo.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use je_platform::resources::fs_path;

use super::ripgrep_search_utils::IOutputChannel;
use super::ripgrep_text_search_engine::RipgrepTextSearchEngine;
use super::search::{IFileMatch, IProgressMessage, ISearchStats, ISerializedFileMatch, ISerializedSearchSuccess, ITextQuery, ITextSearchResult, ITextSearchStats, TextSearchStatsType};
use super::text_search_manager::NativeTextSearchManager;

/// O `pretendOutputChannel`: cada `appendLine(msg)` vira `onMessage({ message: msg })`.
struct MessageOutputChannel(Rc<dyn Fn(IProgressMessage)>);

impl IOutputChannel for MessageOutputChannel {
    fn append_line(&self, msg: &str) {
        (self.0)(IProgressMessage { message: msg.to_owned() });
    }
}

/// `TextSearchEngineAdapter`.
pub struct TextSearchEngineAdapter {
    query: ITextQuery,
    num_threads: Option<i64>,
}

impl TextSearchEngineAdapter {
    pub fn new(query: ITextQuery, num_threads: Option<i64>) -> Self {
        Self { query, num_threads }
    }

    pub fn search(&self, token: &CancellationToken, on_result: Rc<dyn Fn(Vec<ISerializedFileMatch>)>, on_message: Rc<dyn Fn(IProgressMessage)>) -> Promise<ISerializedSearchSuccess> {
        let common = &self.query.common;
        if common.folder_queries.is_empty() && common.extra_file_resources.as_ref().is_none_or(Vec::is_empty) {
            return Promise::resolved(ISerializedSearchSuccess {
                limit_hit: false,
                stats: Some(ISearchStats::Text(ITextSearchStats { stats_type: TextSearchStatsType::SearchProcess })),
                messages: Vec::new(),
            });
        }

        let pretend_output_channel = Rc::new(MessageOutputChannel(on_message));
        let text_search_manager = NativeTextSearchManager::new(self.query.clone(), Rc::new(RipgrepTextSearchEngine::new(pretend_output_channel, self.num_threads)));
        text_search_manager
            .search(Rc::new(move |matches: Vec<IFileMatch>| on_result(matches.iter().map(file_match_to_serialized).collect())), token, None)
            .then(|complete| ISerializedSearchSuccess { limit_hit: complete.limit_hit.unwrap_or(false), stats: complete.stats, messages: complete.messages })
    }
}

fn file_match_to_serialized(file_match: &IFileMatch) -> ISerializedFileMatch {
    ISerializedFileMatch {
        path: fs_path(&file_match.resource),
        results: file_match.results.clone(),
        num_matches: Some(
            file_match
                .results
                .iter()
                .flatten()
                .map(|result| match result {
                    ITextSearchResult::Match(text_match) => text_match.range_locations.len() as i64,
                    ITextSearchResult::Context(_) => 1,
                })
                .sum(),
        ),
    }
}
