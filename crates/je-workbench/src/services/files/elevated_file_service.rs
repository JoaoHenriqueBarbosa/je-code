//! Porte de `src/vs/workbench/services/files/common/elevatedFileService.ts`: o contrato do serviço que escreve um arquivo
//! com privilégios elevados (administrador ou superusuário).
//!
//! Divergências:
//! - **`value`** (`VSBuffer | VSBufferReadable | VSBufferReadableStream`) é o [`FileContentSource`] que o `writeFile` do
//!   serviço de arquivos aceita.
//! - **`_serviceBrand`** não existe: o identificador do serviço é o [`I_ELEVATED_FILE_SERVICE`].

use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{FileStatWithMetadata, WriteFileOptions};

/// `IElevatedFileService`.
pub trait IElevatedFileService {
    /// Whether saving elevated is supported for the provided resource.
    fn is_supported(&self, resource: &URI) -> bool;

    /// Attempts to write to the target resource elevated. This may bring up a dialog to ask for admin username /
    /// password.
    fn write_file_elevated(&self, resource: &URI, value: FileContentSource, options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata>;
}

service_id!(pub I_ELEVATED_FILE_SERVICE: dyn IElevatedFileService = "elevatedFileService");
