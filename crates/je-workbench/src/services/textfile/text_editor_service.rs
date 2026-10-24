//! Porte parcial de `src/vs/workbench/services/textfile/common/textEditorService.ts`: o contrato `ITextEditorService`,
//! que cria entradas de editor de texto a partir de uma entrada sem tipo.
//!
//! Divergências:
//! - **Sobrecargas.** As duas sobrecargas de `createTextEditor` e de `resolveTextEditor` (`IUntypedEditorInput` devolve
//!   `EditorInput` e `IUntypedFileEditorInput` devolve `IFileEditorInput`) são um método só por operação: o
//!   `IFileEditorInput` ainda não foi portado, então o contrato devolve a [`EditorInputRef`].
//! - **Classe.** A classe `TextEditorService` (que escolhe entre arquivo, sem título e recurso virtual) entra com o porte
//!   dela, junto do `IFileEditorInput`; este módulo é só o contrato que o `UntitledTextEditorInputSerializer` e o
//!   `UntitledTextEditorWorkingCopyEditorHandler` leem.

use je_base::promise::Promise;
use je_instantiation::service_id;

use crate::common::editor::{EditorInputRef, UntypedEditorInput};

/// `ITextEditorService`.
pub trait ITextEditorService {
    /// `createTextEditor(input)`. @deprecated this method should not be used, rather consider using `IEditorResolverService`
    /// instead with `DEFAULT_EDITOR_ASSOCIATION.id`.
    fn create_text_editor(&self, input: UntypedEditorInput) -> EditorInputRef;

    /// A way to create text editor inputs from an untyped editor input. Depending on the passed in input this will be:
    /// - a `IFileEditorInput` for file resources
    /// - a `UntitledEditorInput` for untitled resources
    /// - a `TextResourceEditorInput` for virtual resources
    fn resolve_text_editor(&self, input: UntypedEditorInput) -> Promise<EditorInputRef>;
}

service_id!(pub I_TEXT_EDITOR_SERVICE: dyn ITextEditorService = "textEditorService");
