//! Auxiliares dos testes de integração do zyarr.
#![allow(dead_code)]

use zyarr::RegExp;

/// O resultado esperado de um casamento: capturas (a posição 0 é o casamento inteiro) e grupos
/// nomeados, em índices UTF-16.
pub struct Expected {
    pub captures: &'static [Option<(usize, usize)>],
    pub groups: &'static [(&'static str, Option<(usize, usize)>)],
}

/// Compila `pattern`/`flags`, executa `exec(input, start)` e compara com o esperado (`None` é sem
/// casamento).
pub fn check(pattern: &str, flags: &str, input: &[u16], start: usize, expected: Option<Expected>) {
    let pattern: Vec<u16> = pattern.encode_utf16().collect();
    check_units(&pattern, flags, input, start, expected);
}

/// Como [`check`], com o padrão em unidades UTF-16 (permite substituto solitário no padrão).
pub fn check_units(pattern: &[u16], flags: &str, input: &[u16], start: usize, expected: Option<Expected>) {
    let flag_units: Vec<u16> = flags.encode_utf16().collect();
    let regexp = RegExp::new(pattern, &flag_units).unwrap_or_else(|error| {
        panic!("/{}/{flags} não compilou: {error}", String::from_utf16_lossy(pattern))
    });
    let pattern = String::from_utf16_lossy(pattern);
    let found = regexp.exec(input, start);
    match (found, expected) {
        (None, None) => {}
        (Some(found), None) => panic!("/{pattern}/{flags} casou {:?}, o esperado era não casar", found.captures),
        (None, Some(expected)) => {
            panic!("/{pattern}/{flags} não casou, o esperado era {:?}", expected.captures)
        }
        (Some(found), Some(expected)) => {
            let captures: Vec<Option<(usize, usize)>> =
                found.captures.iter().map(|range| range.as_ref().map(|r| (r.start, r.end))).collect();
            assert_eq!(captures, expected.captures, "capturas de /{pattern}/{flags}");
            let groups: Vec<(String, Option<(usize, usize)>)> = found
                .groups
                .iter()
                .map(|(name, range)| (name.clone(), range.as_ref().map(|r| (r.start, r.end))))
                .collect();
            let expected_groups: Vec<(String, Option<(usize, usize)>)> =
                expected.groups.iter().map(|(name, range)| (name.to_string(), *range)).collect();
            assert_eq!(groups, expected_groups, "grupos de /{pattern}/{flags}");
        }
    }
}

/// Confere que `pattern`/`flags` não compilam e que a mensagem é exatamente `message`.
pub fn check_error(pattern: &str, flags: &str, message: &str) {
    match RegExp::from_str(pattern, flags) {
        Ok(_) => panic!("/{pattern}/{flags} compilou, o esperado era o erro {message:?}"),
        Err(error) => assert_eq!(error.message(), message, "mensagem de /{pattern}/{flags}"),
    }
}
