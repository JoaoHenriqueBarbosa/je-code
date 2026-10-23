//! Porte de `src/vs/base/node/unc.ts`: o host de um caminho UNC do Windows.
//!
//! A lista de hosts permitidos (`process.uncHostAllowlist`) e o `process.restrictUNCAccess` são propriedades
//! de um build próprio do Node/Electron e não existem num processo Rust; as funções do original que só leem ou
//! escrevem essas propriedades (`getUNCHostAllowlist`, `addUNCHostToAllowlist`, `disableUNCAccessRestrictions`
//! e `isUNCAccessRestrictionsDisabled`) não foram portadas.

/// `getUNCHost`: o host de um caminho `\\host\...`, `\\.\UNC\host\...` ou `\\?\UNC\host\...`. `None` se o texto
/// não é um caminho UNC com um host seguido de um componente de caminho.
pub fn get_unc_host(maybe_unc_path: Option<&str>) -> Option<String> {
    let maybe_unc_path = maybe_unc_path?; // require a valid string

    let unc_roots = [
        "\\\\.\\UNC\\", // DOS Device paths (https://learn.microsoft.com/en-us/dotnet/standard/io/file-path-formats)
        "\\\\?\\UNC\\",
        "\\\\", // standard UNC path
    ];

    for unc_root in unc_roots {
        if !maybe_unc_path.starts_with(unc_root) {
            continue; // not matching any of our expected UNC roots
        }

        let Some(index_of_unc_path) = maybe_unc_path[unc_root.len()..].find('\\').map(|i| i + unc_root.len()) else {
            continue; // no path component found
        };

        let host_candidate = &maybe_unc_path[unc_root.len()..index_of_unc_path];
        if !host_candidate.is_empty() {
            return Some(host_candidate.to_owned());
        }
    }

    None
}
