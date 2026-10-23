//! Porte de `src/vs/platform/terminal/common/autoApprove`.
//!
//! Portado até agora: a regra de segurança do erro de parse ([`auto_approve_parse_safety`]), a detecção de
//! expansão atrasada do CMD ([`cmd_delayed_expansion`]) e o parser de escritas do `sed -i`
//! ([`sed_file_write_parser`]). As regras (`gitAutoApproveRules`, `powershellAutoApproveRules`,
//! `sortAutoApproveRules`) ficam para outra fatia.

pub mod auto_approve_parse_safety;
pub mod cmd_delayed_expansion;
pub mod sed_file_write_parser;
