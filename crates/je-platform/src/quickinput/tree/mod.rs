//! Porte de `src/vs/platform/quickinput/browser/tree`.
//!
//! Portado: o ordenador ([`quick_input_tree_sorter`]), o filtro ([`quick_input_tree_filter`]), os dados de filtro e o estado
//! agregado de um pai ([`quick_input_tree`]), o delegate de alturas ([`quick_input_delegate`]), o provedor de acessibilidade
//! ([`quick_input_tree_accessibility_provider`]), o renderizador ([`quick_input_tree_renderer`]), o controlador da árvore
//! ([`quick_input_tree_controller`]) e o `QuickTree` ([`quick_tree`]).

pub mod quick_input_delegate;
pub mod quick_input_tree;
pub mod quick_input_tree_accessibility_provider;
pub mod quick_input_tree_controller;
pub mod quick_input_tree_filter;
pub mod quick_input_tree_renderer;
pub mod quick_input_tree_sorter;
pub mod quick_tree;
