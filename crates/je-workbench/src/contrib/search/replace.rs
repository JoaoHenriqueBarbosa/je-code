//! Porte de `src/vs/workbench/contrib/search/browser/replace.ts`: o contrato `IReplaceService`.
//!
//! Divergências:
//! - As sobrecargas de `replace` são [`IReplaceService::replace_match`] (`replace(match)`) e
//!   [`IReplaceService::replace_files`] (`replace(files, progress?)`).
//! - `IProgress<IProgressStep>` é o [`IProgress`]`<`[`ProgressStep`]`>` do workbench, e `Promise<any>` é `Promise<()>`.

use std::rc::Rc;

use je_base::promise::Promise;
use je_instantiation::service_id;

use super::search_tree_model::search_tree_common::{FileMatchOrMatch, ISearchTreeFileMatch, ISearchTreeMatch};
use crate::services::textfile::collaborators::{IProgress, ProgressStep};

/// `IReplaceService`.
pub trait IReplaceService {
    /// Replaces the given match in the file that match belongs to.
    fn replace_match(&self, match_: &Rc<dyn ISearchTreeMatch>) -> Promise<()>;

    /// Replace all the matches from the given file matches in the files. You can also pass the progress runner to update
    /// the progress of replacing.
    fn replace_files(&self, files: &[Rc<dyn ISearchTreeFileMatch>], progress: Option<&dyn IProgress<ProgressStep>>) -> Promise<()>;

    /// Opens the replace preview for given file match or match.
    fn open_replace_preview(&self, element: &FileMatchOrMatch, preserve_focus: Option<bool>, side_by_side: Option<bool>, pinned: Option<bool>) -> Promise<()>;

    /// Update the replace preview for the given file. If `override` is `true`, then replace preview is constructed from
    /// source model.
    fn update_replace_preview(&self, file: &Rc<dyn ISearchTreeFileMatch>, override_: Option<bool>) -> Promise<()>;
}

service_id!(pub I_REPLACE_SERVICE: dyn IReplaceService = "replaceService");
