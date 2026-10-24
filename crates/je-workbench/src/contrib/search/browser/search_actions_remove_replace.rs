//! Porte de `src/vs/workbench/contrib/search/browser/searchActionsRemoveReplace.ts`: a navegação da árvore de resultados que
//! decide qual elemento recebe o foco depois de remover ou substituir (`getElementToFocusAfterRemoved` e
//! `getLastNodeFromSameType`, com o `compareLevels` que elas usam).
//!
//! Recortes e divergências:
//! - **Ações e interface.** `RemoveAction`, `ReplaceAction`, `ReplaceAllAction`, `ReplaceAllInFolderAction`, `performReplace` e
//!   `hasToOpenFile` dependem do `SearchView`, do `IViewsService` e do registro de `Action2` (DOM e árvore do workbench), que este
//!   porte não tem; ficam de fora, junto com `ISearchActionContext` e `IFindInFilesArgs`. O caso `searchActions.test.ts` só exercita
//!   as duas funções exportadas.
//! - **`WorkbenchCompressibleAsyncDataTree`.** O que as funções pedem da árvore é o contrato [`ISearchActionsTree`]
//!   (`navigate`, `expand`, `lastVisibleElement` e `getParentElement`), com o [`ITreeNavigator`] do original. O pai de um elemento
//!   que é o `ISearchResult` (que não é um [`RenderableMatch`]) é `None`.
//! - **`async`.** Cada função devolve uma [`Promise`] resolvida por uma tarefa local (o `await` de `expand` cede como no JavaScript).
//! - **`instanceof AITextSearchHeadingImpl`.** É o cabeçalho com `isAIContributed()` verdadeiro, o único tipo de cabeçalho
//!   contribuído por IA da árvore.

use std::future::Future;
use std::rc::Rc;

use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::runtime::spawn_local;

use crate::contrib::search::search_tree_model::search_tree_common::{
    RenderableMatch, array_contains_element_or_parent, is_search_tree_file_match, is_search_tree_folder_match, is_search_tree_match, is_text_search_heading,
};

/// `ITreeNavigator<T>`.
pub trait ITreeNavigator {
    fn current(&self) -> Option<RenderableMatch>;
    fn previous(&mut self) -> Option<RenderableMatch>;
    fn first(&mut self) -> Option<RenderableMatch>;
    fn last(&mut self) -> Option<RenderableMatch>;
    fn next(&mut self) -> Option<RenderableMatch>;
}

/// O que `searchActionsRemoveReplace.ts` usa de `WorkbenchCompressibleAsyncDataTree<ISearchResult, RenderableMatch>`.
pub trait ISearchActionsTree {
    /// `navigate(start?)`.
    fn navigate(&self, start: Option<&RenderableMatch>) -> Box<dyn ITreeNavigator>;
    /// `expand(element)`: se o nó foi expandido.
    fn expand(&self, element: &RenderableMatch) -> Promise<bool>;
    /// `lastVisibleElement`.
    fn last_visible_element(&self) -> Option<RenderableMatch>;
    /// `getParentElement(element)`: `None` quando o pai é o `ISearchResult`.
    fn get_parent_element(&self, element: &RenderableMatch) -> Option<RenderableMatch>;
}

fn is_match(element: &RenderableMatch) -> bool {
    matches!(element, RenderableMatch::Match(match_) if is_search_tree_match(&**match_))
}

fn is_file_match(element: &RenderableMatch) -> bool {
    matches!(element, RenderableMatch::FileMatch(file_match) if is_search_tree_file_match(&**file_match))
}

fn is_folder_match(element: &RenderableMatch) -> bool {
    matches!(element, RenderableMatch::FolderMatch(folder_match) if is_search_tree_folder_match(&**folder_match))
}

/// `navigator.current() instanceof AITextSearchHeadingImpl`.
fn is_ai_text_search_heading(element: &RenderableMatch) -> bool {
    matches!(element, RenderableMatch::Heading(heading) if heading.is_ai_contributed())
}

/// `compareLevels`.
fn compare_levels(elem1: &RenderableMatch, elem2: &RenderableMatch) -> i32 {
    if is_match(elem1) {
        if is_match(elem2) { 0 } else { -1 }
    } else if is_file_match(elem1) {
        if is_match(elem2) {
            1
        } else if is_file_match(elem2) {
            0
        } else {
            -1
        }
    } else if is_folder_match(elem1) {
        if is_text_search_heading(elem2) {
            -1
        } else if is_folder_match(elem2) {
            0
        } else {
            1
        }
    } else if is_text_search_heading(elem2) {
        0
    } else {
        1
    }
}

/// Roda a tarefa `async` e entrega o resultado como [`Promise`].
fn run_async<T: Clone + 'static>(future: impl Future<Output = Result<T, Error>> + 'static) -> Promise<T> {
    let (promise, resolver) = Promise::new();
    spawn_local(async move {
        match future.await {
            Ok(value) => resolver.resolve(value),
            Err(error) => resolver.reject(error),
        }
    });
    promise
}

/// Returns element to focus after removing the given element (`getElementToFocusAfterRemoved`).
pub fn get_element_to_focus_after_removed(
    viewer: &Rc<dyn ISearchActionsTree>,
    element: &RenderableMatch,
    elements_to_remove: &[RenderableMatch],
) -> Promise<Option<RenderableMatch>> {
    let (viewer, element, elements_to_remove) = (viewer.clone(), element.clone(), elements_to_remove.to_vec());
    run_async(async move {
        let mut navigator = viewer.navigate(Some(&element));
        if is_folder_match(&element) {
            while let Some(current) = navigator.next() {
                if !(!is_folder_match(&current) || array_contains_element_or_parent(&current, &elements_to_remove)) {
                    break;
                }
            }
        } else if is_file_match(&element) {
            while let Some(current) = navigator.next() {
                if !(!is_file_match(&current) || array_contains_element_or_parent(&current, &elements_to_remove)) {
                    break;
                }
                // Never expand AI search results by default
                if is_ai_text_search_heading(&current) {
                    return Ok(Some(current));
                }
                viewer.expand(&current).await?;
            }
        } else {
            while let Some(current) = navigator.next() {
                if !(!is_match(&current) || array_contains_element_or_parent(&current, &elements_to_remove)) {
                    break;
                }
                // Never expand AI search results by default
                if is_ai_text_search_heading(&current) {
                    return Ok(Some(current));
                }
                viewer.expand(&current).await?;
            }
        }
        Ok(navigator.current())
    })
}

/// Finds the last element in the tree with the same type as `element` (`getLastNodeFromSameType`).
pub fn get_last_node_from_same_type(viewer: &Rc<dyn ISearchActionsTree>, element: &RenderableMatch) -> Promise<Option<RenderableMatch>> {
    let (viewer, element) = (viewer.clone(), element.clone());
    run_async(async move {
        let mut last_elem = viewer.last_visible_element();

        while let Some(current) = last_elem.clone() {
            let compare_val = compare_levels(&element, &current);
            if compare_val == -1 {
                let expanded = viewer.expand(&current).await?;
                if !expanded {
                    return Ok(Some(current));
                }
                last_elem = viewer.last_visible_element();
            } else if compare_val == 1 {
                match viewer.get_parent_element(&current) {
                    // o pai é o ISearchResult
                    None => break,
                    Some(potential_last_elem) => last_elem = Some(potential_last_elem),
                }
            } else {
                return Ok(Some(current));
            }
        }

        Ok(None)
    })
}
