//! Porte de `src/vs/workbench/services/search/common/folderQuerySearchTree.ts`: uma árvore de busca ternária
//! com chaves `URI` e casamento de prefixo que respeita `query` e `fragment`, feita para a busca de arquivos
//! (a árvore comum não os suporta, microsoft/vscode#227836).
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `class FolderQuerySearchTree extends TernarySearchTree` vira a composição: a struct guarda a
//!   [`TernarySearchTree`] e expõe só os dois métodos próprios (`findQueryFragmentAwareSubstr` e
//!   `forEachFolderQueryInfo`). O `findSubstr` herdado só é chamado de dentro.
//! - `FolderQueryInfo extends { folder: URI }` é um parâmetro de tipo sem restrição (nada aqui lê `folder`).
//! - o `Map<string, FolderQueryInfo>` de cada pasta base é um [`OrderedMap`] (mesma ordem de inserção).
//! - `forEach` da árvore devolve cópias dos valores, então `FolderQueryInfo` precisa ser `Clone`.
//! - `ignorePathCasing = () => false` é o `None` do argumento opcional.

use std::rc::Rc;

use je_base::js_string::JsString;
use je_base::map::{OrderedMap, ResourceMap};
use je_base::ternary_search_tree::{TernarySearchTree, UriIterator, UriPredicate};
use je_base::uri::{Field, URI, UriChange};

use super::search::IFolderQuery;

/// `{ query: '', fragment: '' }` do `URI#with`.
fn without_query_or_fragment() -> UriChange {
    UriChange { query: Field::Clear, fragment: Field::Clear, ..UriChange::default() }
}

/// `encodeKey`: `query`, seguida de `#fragment`, quando existirem.
fn encode_key(key: &URI) -> JsString {
    let mut encoded = JsString::new();
    if !key.query().is_empty() {
        encoded.push_units(key.query().units());
    }
    if !key.fragment().is_empty() {
        encoded.push_unit(u16::from(b'#'));
        encoded.push_units(key.fragment().units());
    }
    encoded
}

/// `FolderQuerySearchTree`.
pub struct FolderQuerySearchTree<FolderQueryInfo> {
    tree: TernarySearchTree<URI, OrderedMap<JsString, FolderQueryInfo>>,
}

impl<FolderQueryInfo: Clone> FolderQuerySearchTree<FolderQueryInfo> {
    /// O construtor: agrupa as consultas pela pasta sem `query` nem `fragment` e, dentro de cada grupo,
    /// indexa a informação pela chave `query#fragment` da própria consulta.
    pub fn new(
        folder_queries: &[IFolderQuery<URI>],
        get_folder_query_info: impl Fn(&IFolderQuery<URI>, usize) -> FolderQueryInfo,
        ignore_path_casing: Option<UriPredicate>,
    ) -> Self {
        let ignore_path_casing = ignore_path_casing.unwrap_or_else(|| Rc::new(|_: &URI| false));
        let uri_iterator = UriIterator::new(ignore_path_casing, Rc::new(|_: &URI| false));
        let mut tree: TernarySearchTree<URI, OrderedMap<JsString, FolderQueryInfo>> =
            TernarySearchTree::new(Box::new(uri_iterator));

        let mut fq_by_same_base: ResourceMap<Vec<usize>> = ResourceMap::new();
        for (i, fq) in folder_queries.iter().enumerate() {
            let uri_without_query_or_fragment = fq.folder.with(&without_query_or_fragment());
            let mut group = fq_by_same_base.get(&uri_without_query_or_fragment).cloned().unwrap_or_default();
            group.push(i);
            fq_by_same_base.set(uri_without_query_or_fragment, group);
        }
        fq_by_same_base.for_each(|values, key, _| {
            let mut folder_queries_with_queries = OrderedMap::new();
            for &i in values {
                let fq = &folder_queries[i];
                folder_queries_with_queries.insert(encode_key(&fq.folder), get_folder_query_info(fq, i));
            }
            tree.set(key.clone(), folder_queries_with_queries);
        });

        Self { tree }
    }

    /// `findQueryFragmentAwareSubstr`: acha a pasta base mais longa que é prefixo da chave e, dentro dela, a
    /// consulta cujo `query#fragment` é exatamente o da chave.
    pub fn find_query_fragment_aware_substr(&self, key: &URI) -> Option<&FolderQueryInfo> {
        let base_uri_result = self.tree.find_substr(&key.with(&without_query_or_fragment()))?;
        base_uri_result.get(&encode_key(key))
    }

    /// `forEachFolderQueryInfo`.
    pub fn for_each_folder_query_info(&self, mut f: impl FnMut(&FolderQueryInfo)) {
        self.tree.for_each(|elem, _| elem.values().for_each(&mut f));
    }
}
