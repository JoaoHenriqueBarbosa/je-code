//! Porte parcial de `src/vs/workbench/contrib/testing/browser/explorerProjections/display.ts`: só o que as projeções
//! usam, o delimitador entre os rótulos de um item na lista plana.

/// `flatTestItemDelimiter`: ` › ` (U+203A) entre os rótulos da cadeia de pais.
pub const FLAT_TEST_ITEM_DELIMITER: &str = " \u{203A} ";
