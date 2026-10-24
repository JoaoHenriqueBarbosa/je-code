//! Porte de `JavaScriptCore/bytecompiler/NodesCodegen.cpp`.
//!
//! Módulo hospedeiro: as fatias `nodes_codegen_cpp*.rs` são juntadas por `include!` na ordem do
//! `.cpp`. Todas escrevem caminhos completos (`crate::...`, `std::...`), então nenhum `use` é
//! necessário aqui; as definições de tipo e as funções auxiliares de cada fatia (`Cpp2Reg`,
//! `cpp3b_same_register`, `cpp5b_process_clause_list`, ...) ficam no escopo comum deste módulo,
//! que é o que permite a uma fatia chamar o auxiliar de outra.

#![allow(clippy::too_many_arguments)]

// Linhas 1 a 540.
include!("nodes_codegen_cpp1.rs");
// Linhas 555 a 1152.
include!("nodes_codegen_cpp1b.rs");
// Linhas 1154 a 2227.
include!("nodes_codegen_cpp2.rs");
// Linhas 2228 a 2414.
include!("nodes_codegen_cpp3.rs");
// Linhas 2410 a 3373.
include!("nodes_codegen_cpp3b.rs");
// Linhas 3375 a 3631.
include!("nodes_codegen_cpp4.rs");
// Linhas 3633 a 4333.
include!("nodes_codegen_cpp4b.rs");
// Linhas 4335 a 4429.
include!("nodes_codegen_cpp5.rs");
// Linhas 4431 a 4846.
include!("nodes_codegen_cpp5b.rs");
// Linhas 4848 a 5191.
include!("nodes_codegen_cpp5c.rs");
// Linhas 5193 a 5532.
include!("nodes_codegen_cpp5d.rs");
// Linhas 5535 a 5980.
include!("nodes_codegen_cpp6.rs");
// Linhas 5982 a 6473.
include!("nodes_codegen_cpp7.rs");
// Despacho dos métodos virtuais de ExpressionNode e StatementNode sobre os enums.
include!("nodes_codegen_dispatch.rs");
