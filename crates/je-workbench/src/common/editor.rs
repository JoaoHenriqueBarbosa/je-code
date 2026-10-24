//! Porte parcial de `src/vs/workbench/common/editor.ts`: só o que os serviços de working copy portados leem, o
//! `SaveReason`, o `SaveSource`, as opções de salvar e reverter (`ISaveOptions`, `IRevertOptions`) e o
//! `GroupIdentifier`.
//!
//! O resto do arquivo (`EditorInput`, as capacidades e os eventos de editor, `EditorExtensions`...) entra com o porte
//! do `EditorInput`, que ainda não existe.

/// `SaveReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveReason {
    /// Explicit user gesture.
    Explicit = 1,
    /// Auto save after a timeout.
    Auto = 2,
    /// Auto save after editor focus change.
    FocusChange = 3,
    /// Auto save after window change.
    WindowChange = 4,
}

/// `SaveSource`: o identificador obtido de `SaveSourceRegistry.registerSource()`.
pub type SaveSource = String;

/// `ISaveOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveOptions {
    /// An indicator how the save operation was triggered.
    pub reason: Option<SaveReason>,
    /// An indicator about the source of the save operation. Must use `SaveSourceRegistry.registerSource()` to obtain.
    pub source: Option<SaveSource>,
    /// Forces to save the contents of the working copy again even if the working copy is not dirty.
    pub force: Option<bool>,
    /// Instructs the save operation to skip any save participants.
    pub skip_save_participants: Option<bool>,
    /// A hint as to which file systems should be available for saving.
    pub available_file_systems: Option<Vec<String>>,
}

/// `IRevertOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RevertOptions {
    /// Forces to load the contents of the working copy again even if the working copy is not dirty.
    pub force: Option<bool>,
    /// A soft revert will clear dirty state of a working copy but will not attempt to load it from its persisted
    /// state. This option may be used in scenarios where an editor is closed and where we do not require to load the
    /// contents.
    pub soft: Option<bool>,
}

/// `GroupIdentifier`.
pub type GroupIdentifier = i64;
