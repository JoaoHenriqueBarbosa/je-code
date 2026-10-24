//! Porte de `src/vs/workbench/services/textfile`: `common/encoding.ts` em [`encoding`], com o
//! `iconv-lite` que ele usa em [`iconv_lite`] e as tabelas das páginas de código DOS em [`code_pages`].

pub mod code_pages;
pub mod encoding;
pub mod iconv_lite;
