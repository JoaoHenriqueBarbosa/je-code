//! Porte de `src/vs/workbench/contrib/testing/common`: o núcleo de resultados de testes (tipos, estados, resultados ao
//! vivo e hidratados, serviço e armazenamento de resultados, perfis de execução, exclusões) e o que ele precisa da coleção
//! de itens de teste.

pub mod constants;
pub mod get_computed_state;
pub mod main_thread_test_collection;
pub mod observable_value;
pub mod serialized_json;
pub mod stored_value;
pub mod test_coverage;
pub mod test_exclusions;
pub mod test_explorer_filter_state;
pub mod test_id;
pub mod test_item_collection;
pub mod test_profile_service;
pub mod test_result;
pub mod test_result_service;
pub mod test_result_storage;
pub mod test_service;
pub mod test_types;
pub mod testing_context_keys;
pub mod testing_states;
