//! Porte de `src/vs/base/node/crypto.ts`: a conferência do SHA-256 de um arquivo.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::errors::Error;

/// `checksum`: calcula o SHA-256 do arquivo e o compara (em hexadecimal minúsculo) com `sha256hash`. Devolve
/// `Err` com a mensagem `Hash mismatch` se diferem, ou o erro de leitura do arquivo.
///
/// O original lê por `fs.createReadStream` e é assíncrono; aqui a leitura é bloqueante, e quem precisa de
/// assíncrono a executa fora do laço de eventos.
pub fn checksum(path: &Path, sha256hash: Option<&str>) -> Result<(), Error> {
    let mut input = File::open(path).map_err(|e| Error::new(e.to_string()))?;
    let mut hash = Sha256::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut chunk).map_err(|e| Error::new(e.to_string()))?;
        if read == 0 {
            break;
        }
        hash.update(&chunk[..read]);
    }
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();

    if sha256hash != Some(digest.as_str()) {
        return Err(Error::new("Hash mismatch"));
    }
    Ok(())
}
