//! Porte de `src/vs/workbench/contrib/search/browser/searchCompare.ts`: a ordem dos elementos da árvore de resultados de
//! busca (`searchMatchComparer` entre irmãos e `searchComparer` entre quaisquer dois elementos).
//!
//! Divergências:
//! - **Argumento padrão.** `sortOrder: SearchSortOrder = SearchSortOrder.Default` não existe em Rust: quem chama passa
//!   [`SearchSortOrder::Default`] (que também é o [`Default`] do tipo).
//! - **Predicados de tipo.** `isSearchTreeFileMatch`, `isSearchTreeFolderMatch` e `isSearchTreeMatch` (que olham o prefixo do
//!   `id()`) são aplicados ao objeto dentro da variante de [`RenderableMatch`] correspondente, e `isIMatchInNotebook` é
//!   [`is_i_match_in_notebook`].
//! - **`isSearchTreeAIFileMatch(elementA) && isSearchTreeAIFileMatch(elementB)`**, no ramo que compara pastas, testa a
//!   existência de `getFullRange()`, que as pastas não têm. O elemento do ramo é sempre uma [`ISearchTreeFolderMatch`], que
//!   não é um [`ISearchTreeAIFileMatch`](crate::contrib::search::ai_search::ai_search_model_base::ISearchTreeAIFileMatch);
//!   o teste nunca é verdadeiro e o ramo `rank` não existe aqui.
//! - **Números.** Os retornos do JavaScript (`number`) são `i64`; as contagens e os índices viram `i64` antes da subtração, e
//!   o `mtime` já é `i64`. Os comparadores de [`je_base::comparers`] trabalham sobre unidades UTF-16 (`&[u16]`) e sobre a
//!   [`Platform`] do processo.
//! - **Variáveis de módulo.** `elemAIndex` e `elemBIndex` (rascunho global do original) são variáveis locais.

use std::rc::Rc;

use je_base::comparers::{compare_file_extensions, compare_file_names, compare_paths};
use je_base::platform::Platform;
use je_editor::range::Range;
use je_platform::resources::fs_path;

use crate::contrib::search::notebook_search::notebook_search_model_base::{IMatchInNotebook, is_i_match_in_notebook};
use crate::contrib::search::search_tree_model::search_tree_common::{
    ISearchTreeFileMatch, ISearchTreeFolderMatch, ISearchTreeMatch, RenderableMatch, create_parent_list, is_search_tree_file_match, is_search_tree_folder_match,
    is_search_tree_match,
};
use crate::services::search::search::SearchSortOrder;

thread_local! {
    static PLATFORM: Platform = Platform::detect();
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn count_difference(a: usize, b: usize) -> i64 {
    i64::try_from(a).unwrap_or(i64::MAX) - i64::try_from(b).unwrap_or(i64::MAX)
}

/// O que as ordens de contagem, tipo e nome leem de uma pasta ou de um arquivo.
trait SortKey {
    fn sort_count(&self) -> usize;
    fn sort_name(&self) -> String;
}

impl SortKey for Rc<dyn ISearchTreeFolderMatch> {
    fn sort_count(&self) -> usize {
        ISearchTreeFolderMatch::count(self.as_ref())
    }

    fn sort_name(&self) -> String {
        ISearchTreeFolderMatch::name(self.as_ref())
    }
}

impl SortKey for Rc<dyn ISearchTreeFileMatch> {
    fn sort_count(&self) -> usize {
        ISearchTreeFileMatch::count(self.as_ref())
    }

    fn sort_name(&self) -> String {
        ISearchTreeFileMatch::name(self.as_ref())
    }
}

/// As quatro ordens que pastas e arquivos têm em comum (`CountDescending`, `CountAscending`, `Type` e `FileNames`); `None`
/// para as demais, que cada ramo trata a seu modo.
fn compare_by_shared_order<T: SortKey>(element_a: &T, element_b: &T, sort_order: SearchSortOrder) -> Option<i64> {
    match sort_order {
        SearchSortOrder::CountDescending => Some(count_difference(element_b.sort_count(), element_a.sort_count())),
        SearchSortOrder::CountAscending => Some(count_difference(element_a.sort_count(), element_b.sort_count())),
        SearchSortOrder::Type => Some(i64::from(compare_file_extensions(Some(&units(&element_a.sort_name())), Some(&units(&element_b.sort_name()))))),
        SearchSortOrder::FileNames => Some(i64::from(compare_file_names(Some(&units(&element_a.sort_name())), Some(&units(&element_b.sort_name()))))),
        SearchSortOrder::Default | SearchSortOrder::Modified => None,
    }
}

/// `comparePaths(a.resource.fsPath, b.resource.fsPath) || compareFileNames(a.name(), b.name())`.
fn compare_by_path_then_name(path_a: &str, name_a: &str, path_b: &str, name_b: &str) -> i64 {
    let by_path = PLATFORM.with(|platform| compare_paths(&units(path_a), &units(path_b), false, platform));
    if by_path != 0 {
        return i64::from(by_path);
    }
    i64::from(compare_file_names(Some(&units(name_a)), Some(&units(name_b))))
}

fn as_folder_match(element: &RenderableMatch) -> Option<&Rc<dyn ISearchTreeFolderMatch>> {
    match element {
        RenderableMatch::FolderMatch(folder_match) if is_search_tree_folder_match(folder_match.as_ref()) => Some(folder_match),
        _ => None,
    }
}

fn as_file_match(element: &RenderableMatch) -> Option<&Rc<dyn ISearchTreeFileMatch>> {
    match element {
        RenderableMatch::FileMatch(file_match) if is_search_tree_file_match(file_match.as_ref()) => Some(file_match),
        _ => None,
    }
}

fn as_match(element: &RenderableMatch) -> Option<&Rc<dyn ISearchTreeMatch>> {
    match element {
        RenderableMatch::Match(match_) if is_search_tree_match(match_.as_ref()) => Some(match_),
        _ => None,
    }
}

/// `searchMatchComparer`: compara instâncias do mesmo tipo de casamento. Tipos diferentes não deveriam ser irmãos e a
/// ordem entre eles é indefinida.
pub fn search_match_comparer(element_a: &RenderableMatch, element_b: &RenderableMatch, sort_order: SearchSortOrder) -> i64 {
    let (file_a, file_b) = (as_file_match(element_a), as_file_match(element_b));
    let (folder_a, folder_b) = (as_folder_match(element_a), as_folder_match(element_b));

    if file_a.is_some() && folder_b.is_some() {
        return 1;
    }

    if file_b.is_some() && folder_a.is_some() {
        return -1;
    }

    if let (Some(folder_a), Some(folder_b)) = (folder_a, folder_b) {
        let elem_a_index = folder_a.index();
        let elem_b_index = folder_b.index();
        if elem_a_index != -1 && elem_b_index != -1 {
            return i64::from(elem_a_index) - i64::from(elem_b_index);
        }

        if let Some(result) = compare_by_shared_order(folder_a, folder_b, sort_order) {
            return result;
        }
        // Fall through otherwise (`Default` e `Modified`).
        let (Some(resource_a), Some(resource_b)) = (folder_a.resource(), folder_b.resource()) else {
            return 0;
        };
        return compare_by_path_then_name(&fs_path(&resource_a), &folder_a.name(), &fs_path(&resource_b), &folder_b.name());
    }

    if let (Some(file_a), Some(file_b)) = (file_a, file_b) {
        if let Some(result) = compare_by_shared_order(file_a, file_b, sort_order) {
            return result;
        }
        if sort_order == SearchSortOrder::Modified {
            if let (Some(file_stat_a), Some(file_stat_b)) = (file_a.file_stat(), file_b.file_stat()) {
                return file_stat_b.mtime() - file_stat_a.mtime();
            }
        }
        // Fall through otherwise (`Default`, e `Modified` sem os dois `stat`).
        return compare_by_path_then_name(&fs_path(&file_a.resource()), &file_a.name(), &fs_path(&file_b.resource()), &file_b.name());
    }

    if let (Some(match_a), Some(match_b)) = (as_match(element_a), as_match(element_b)) {
        if let (Some(notebook_a), Some(notebook_b)) = (is_i_match_in_notebook(match_a), is_i_match_in_notebook(match_b)) {
            return compare_notebook_pos(&notebook_a, &notebook_b);
        }

        return i64::from(Range::compare_ranges_using_starts(Some(&match_a.range()), Some(&match_b.range())));
    }

    0
}

fn compare_notebook_pos(match1: &Rc<dyn IMatchInNotebook>, match2: &Rc<dyn IMatchInNotebook>) -> i64 {
    if match1.cell_index() == match2.cell_index() {
        match (match1.webview_index(), match2.webview_index()) {
            (Some(webview_index1), Some(webview_index2)) => count_difference(webview_index1, webview_index2),
            (None, None) => i64::from(Range::compare_ranges_using_starts(Some(&match1.range()), Some(&match2.range()))),
            // webview matches should always be after content matches
            (Some(_), None) => 1,
            (None, Some(_)) => -1,
        }
    } else if match1.cell_index() < match2.cell_index() {
        -1
    } else {
        1
    }
}

/// `searchComparer`: compara quaisquer dois elementos da árvore, subindo pela hierarquia até os primeiros ancestrais
/// diferentes e comparando esses.
pub fn search_comparer(element_a: &RenderableMatch, element_b: &RenderableMatch, sort_order: SearchSortOrder) -> i64 {
    let elem_a_parents = create_parent_list(element_a);
    let elem_b_parents = create_parent_list(element_b);

    let mut i = elem_a_parents.len() as isize - 1;
    let mut j = elem_b_parents.len() as isize - 1;
    while i >= 0 && j >= 0 {
        let (parent_a, parent_b) = (&elem_a_parents[i as usize], &elem_b_parents[j as usize]);
        if parent_a.id() != parent_b.id() {
            return search_match_comparer(parent_a, parent_b, sort_order);
        }
        i -= 1;
        j -= 1;
    }
    let elem_a_at_end = i == 0;
    let elem_b_at_end = j == 0;

    if elem_a_at_end && !elem_b_at_end {
        1
    } else if !elem_a_at_end && elem_b_at_end {
        -1
    } else {
        0
    }
}
