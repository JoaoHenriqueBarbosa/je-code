//! Leitura e extração de VSIX (o zip de uma extensão): porte do `getManifest` de `node/extensionManagementUtil.ts` e
//! do `extract` de `base/node/zip.ts` (com `sourcePath: 'extension'`).
//!
//! O VSIX tem `extension.vsixmanifest`, `[Content_Types].xml` e a pasta `extension/` com o conteúdo da extensão; só ela
//! é extraída, sem o prefixo. Entradas com caminho que escapa da pasta (`..`, absoluto) são recusadas.
//!
//! Divergência: entradas que são link simbólico são ignoradas (o original as cria); um VSIX de extensão do marketplace
//! não os usa.

use std::io::{Read, Seek};

use serde_json::Value;
use zip::ZipArchive;
use zip::result::ZipError;

use crate::fs::ExtFs;
use crate::management::{ExtensionManagementError, ExtensionManagementErrorCode};
use crate::paths;

const SOURCE_PATH: &str = "extension";

/// `fromExtractError`: o tipo de falha do zip vira o código de erro da gerência.
pub fn from_zip_error(error: &ZipError) -> ExtensionManagementError {
    let code = match error {
        ZipError::InvalidArchive(_) | ZipError::UnsupportedArchive(_) => ExtensionManagementErrorCode::CorruptZip,
        _ => ExtensionManagementErrorCode::Extract,
    };
    ExtensionManagementError::new(error.to_string(), code)
}

fn open<R: Read + Seek>(reader: R) -> Result<ZipArchive<R>, ExtensionManagementError> {
    ZipArchive::new(reader).map_err(|e| from_zip_error(&e))
}

/// `buffer(vsixPath, 'extension/package.json')`: o conteúdo de uma entrada.
pub fn read_entry<R: Read + Seek>(reader: R, entry_path: &str) -> Result<Vec<u8>, ExtensionManagementError> {
    let mut archive = open(reader)?;
    let mut file = archive.by_name(entry_path).map_err(|e| match e {
        ZipError::FileNotFound => ExtensionManagementError::new(format!("{entry_path} not found inside the zip"), ExtensionManagementErrorCode::Extract),
        other => from_zip_error(&other),
    })?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|e| ExtensionManagementError::new(e.to_string(), ExtensionManagementErrorCode::Extract))?;
    Ok(data)
}

/// `getManifest(vsixPath)`: o `extension/package.json` do VSIX, como JSON cru.
pub fn get_manifest<R: Read + Seek>(reader: R) -> Result<Value, ExtensionManagementError> {
    let data = read_entry(reader, "extension/package.json")?;
    let text = String::from_utf8_lossy(&data);
    serde_json::from_str(crate::json_util::strip_bom(&text))
        .map_err(|_| ExtensionManagementError::new("VSIX invalid: package.json is not a JSON file.", ExtensionManagementErrorCode::Invalid))
}

/// `extract(zipPath, targetPath, { sourcePath: 'extension', overwrite: true })`: extrai `extension/*` para `target`.
pub fn extract<R: Read + Seek>(reader: R, fs: &dyn ExtFs, target: &str) -> Result<(), ExtensionManagementError> {
    let mut archive = open(reader)?;
    let io_error = |e: std::io::Error| ExtensionManagementError::new(e.to_string(), ExtensionManagementErrorCode::Extract);
    fs.create_dir_all(target).map_err(io_error)?;
    let mut extracted_any = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| from_zip_error(&e))?;
        let Some(enclosed) = entry.enclosed_name() else {
            return Err(ExtensionManagementError::new(format!("invalid file name in the zip: {}", entry.name()), ExtensionManagementErrorCode::Extract));
        };
        let name = enclosed.to_string_lossy().replace('\\', "/");
        let Some(relative) = name.strip_prefix(SOURCE_PATH).and_then(|rest| rest.strip_prefix('/')) else { continue };
        if relative.is_empty() {
            continue;
        }
        extracted_any = true;
        let destination = paths::join_path(target, &[relative]);
        if entry.is_dir() {
            fs.create_dir_all(&destination).map_err(io_error)?;
            continue;
        }
        if entry.is_symlink() {
            continue;
        }
        let mut data = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry.read_to_end(&mut data).map_err(|e| {
            // Um dado truncado no meio de uma entrada é um zip incompleto.
            let code = if e.kind() == std::io::ErrorKind::UnexpectedEof { ExtensionManagementErrorCode::IncompleteZip } else { ExtensionManagementErrorCode::CorruptZip };
            ExtensionManagementError::new(e.to_string(), code)
        })?;
        fs.write(&destination, &data).map_err(io_error)?;
        if let Some(mode) = entry.unix_mode() {
            fs.set_mode(&destination, mode).map_err(io_error)?;
        }
    }
    if !extracted_any {
        return Err(ExtensionManagementError::new(format!("{SOURCE_PATH} not found inside the zip"), ExtensionManagementErrorCode::Extract));
    }
    Ok(())
}
