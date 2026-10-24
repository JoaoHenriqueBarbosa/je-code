//! Gerado por `scripts/gen-common-identifiers.py` a partir de `builtins/BuiltinNames.h`,
//! `bytecode/BytecodeIntrinsicRegistry.h` e `derived/JavaScriptCore/JSCBuiltins.h`. Não editar à
//! mão. É o que o C++ produz expandindo `JSC_FOREACH_BUILTIN_FUNCTION_NAME`,
//! `JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_PROPERTY_NAME` e
//! `JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_WELL_KNOWN_SYMBOL`: campos, construtor e acessores. O que
//! é escrito à mão (as consultas, `appendExternalName`) fica em `builtin_names.rs`.
//!
//! Os `Symbols::xxxPrivateName` do C++ são `StaticSymbolImpl` globais; aqui cada `BuiltinNames`
//! materializa os seus no construtor e os guarda como `Identifier` em `m_xxx_private_name`.

use crate::runtime::builtin_names::{PrivateNameSet, WellKnownSymbolMap};
use crate::runtime::identifier::Identifier;
use crate::runtime::vm::VM;
use crate::wtf::text::symbol_impl::{StaticSymbolImpl, S_FLAG_DEFAULT, S_FLAG_IS_PRIVATE};

/// `class BuiltinNames`.
#[derive(Debug)]
pub struct BuiltinNames {
    pub(super) m_empty_identifier: Identifier,
    pub(super) m_promise: Identifier,
    pub(super) m_promise_private_name: Identifier,
    pub(super) m_add_disposable_resource: Identifier,
    pub(super) m_add_disposable_resource_private_name: Identifier,
    pub(super) m_adopt: Identifier,
    pub(super) m_adopt_private_name: Identifier,
    pub(super) m_apply: Identifier,
    pub(super) m_apply_private_name: Identifier,
    pub(super) m_array_iterator_next_helper: Identifier,
    pub(super) m_array_iterator_next_helper_private_name: Identifier,
    pub(super) m_async_dispose: Identifier,
    pub(super) m_async_dispose_private_name: Identifier,
    pub(super) m_at: Identifier,
    pub(super) m_at_private_name: Identifier,
    pub(super) m_builtin_map_iterable: Identifier,
    pub(super) m_builtin_map_iterable_private_name: Identifier,
    pub(super) m_builtin_set_iterable: Identifier,
    pub(super) m_builtin_set_iterable_private_name: Identifier,
    pub(super) m_call: Identifier,
    pub(super) m_call_private_name: Identifier,
    pub(super) m_chunks: Identifier,
    pub(super) m_chunks_private_name: Identifier,
    pub(super) m_close_all_iterators: Identifier,
    pub(super) m_close_all_iterators_private_name: Identifier,
    pub(super) m_concat: Identifier,
    pub(super) m_concat_private_name: Identifier,
    pub(super) m_create_array_without_prototype: Identifier,
    pub(super) m_create_array_without_prototype_private_name: Identifier,
    pub(super) m_create_disposable_resource: Identifier,
    pub(super) m_create_disposable_resource_private_name: Identifier,
    pub(super) m_create_inspector_injected_script: Identifier,
    pub(super) m_create_inspector_injected_script_private_name: Identifier,
    pub(super) m_create_object_without_prototype: Identifier,
    pub(super) m_create_object_without_prototype_private_name: Identifier,
    pub(super) m_cross_realm_throw: Identifier,
    pub(super) m_cross_realm_throw_private_name: Identifier,
    pub(super) m_default_async_from_async_array_like: Identifier,
    pub(super) m_default_async_from_async_array_like_private_name: Identifier,
    pub(super) m_default_async_from_async_iterator: Identifier,
    pub(super) m_default_async_from_async_iterator_private_name: Identifier,
    pub(super) m_defer_method: Identifier,
    pub(super) m_defer_method_private_name: Identifier,
    pub(super) m_delete_property: Identifier,
    pub(super) m_delete_property_private_name: Identifier,
    pub(super) m_dispose: Identifier,
    pub(super) m_dispose_private_name: Identifier,
    pub(super) m_dispose_async: Identifier,
    pub(super) m_dispose_async_private_name: Identifier,
    pub(super) m_drop: Identifier,
    pub(super) m_drop_private_name: Identifier,
    pub(super) m_evaluate: Identifier,
    pub(super) m_evaluate_private_name: Identifier,
    pub(super) m_every: Identifier,
    pub(super) m_every_private_name: Identifier,
    pub(super) m_filter: Identifier,
    pub(super) m_filter_private_name: Identifier,
    pub(super) m_find: Identifier,
    pub(super) m_find_private_name: Identifier,
    pub(super) m_find_index: Identifier,
    pub(super) m_find_index_private_name: Identifier,
    pub(super) m_find_last: Identifier,
    pub(super) m_find_last_private_name: Identifier,
    pub(super) m_find_last_index: Identifier,
    pub(super) m_find_last_index_private_name: Identifier,
    pub(super) m_flat_into_array: Identifier,
    pub(super) m_flat_into_array_private_name: Identifier,
    pub(super) m_flat_into_array_with_callback: Identifier,
    pub(super) m_flat_into_array_with_callback_private_name: Identifier,
    pub(super) m_flat_map: Identifier,
    pub(super) m_flat_map_private_name: Identifier,
    pub(super) m_for_each: Identifier,
    pub(super) m_for_each_private_name: Identifier,
    pub(super) m_from: Identifier,
    pub(super) m_from_private_name: Identifier,
    pub(super) m_from_async: Identifier,
    pub(super) m_from_async_private_name: Identifier,
    pub(super) m_from_entries: Identifier,
    pub(super) m_from_entries_private_name: Identifier,
    pub(super) m_generator_resume: Identifier,
    pub(super) m_generator_resume_private_name: Identifier,
    pub(super) m_get: Identifier,
    pub(super) m_get_private_name: Identifier,
    pub(super) m_get_async_dispose_method: Identifier,
    pub(super) m_get_async_dispose_method_private_name: Identifier,
    pub(super) m_get_dispose_method: Identifier,
    pub(super) m_get_dispose_method_private_name: Identifier,
    pub(super) m_get_iterator_flattenable: Identifier,
    pub(super) m_get_iterator_flattenable_private_name: Identifier,
    pub(super) m_get_iterator_sync: Identifier,
    pub(super) m_get_iterator_sync_private_name: Identifier,
    pub(super) m_get_options_object: Identifier,
    pub(super) m_get_options_object_private_name: Identifier,
    pub(super) m_group_by: Identifier,
    pub(super) m_group_by_private_name: Identifier,
    pub(super) m_has: Identifier,
    pub(super) m_has_private_name: Identifier,
    pub(super) m_import_value: Identifier,
    pub(super) m_import_value_private_name: Identifier,
    pub(super) m_iterator_close_all_normal: Identifier,
    pub(super) m_iterator_close_all_normal_private_name: Identifier,
    pub(super) m_iterator_zip: Identifier,
    pub(super) m_iterator_zip_private_name: Identifier,
    pub(super) m_map: Identifier,
    pub(super) m_map_private_name: Identifier,
    pub(super) m_move: Identifier,
    pub(super) m_move_private_name: Identifier,
    pub(super) m_next: Identifier,
    pub(super) m_next_private_name: Identifier,
    pub(super) m_perform_iteration: Identifier,
    pub(super) m_perform_iteration_private_name: Identifier,
    pub(super) m_perform_proxy_object_get: Identifier,
    pub(super) m_perform_proxy_object_get_private_name: Identifier,
    pub(super) m_perform_proxy_object_get_by_val: Identifier,
    pub(super) m_perform_proxy_object_get_by_val_private_name: Identifier,
    pub(super) m_perform_proxy_object_has: Identifier,
    pub(super) m_perform_proxy_object_has_private_name: Identifier,
    pub(super) m_perform_proxy_object_has_by_val: Identifier,
    pub(super) m_perform_proxy_object_has_by_val_private_name: Identifier,
    pub(super) m_perform_proxy_object_set_by_val_sloppy: Identifier,
    pub(super) m_perform_proxy_object_set_by_val_sloppy_private_name: Identifier,
    pub(super) m_perform_proxy_object_set_by_val_strict: Identifier,
    pub(super) m_perform_proxy_object_set_by_val_strict_private_name: Identifier,
    pub(super) m_perform_proxy_object_set_sloppy: Identifier,
    pub(super) m_perform_proxy_object_set_sloppy_private_name: Identifier,
    pub(super) m_perform_proxy_object_set_strict: Identifier,
    pub(super) m_perform_proxy_object_set_strict_private_name: Identifier,
    pub(super) m_reduce: Identifier,
    pub(super) m_reduce_private_name: Identifier,
    pub(super) m_reduce_right: Identifier,
    pub(super) m_reduce_right_private_name: Identifier,
    pub(super) m_remove_first_from_list: Identifier,
    pub(super) m_remove_first_from_list_private_name: Identifier,
    pub(super) m_return: Identifier,
    pub(super) m_return_private_name: Identifier,
    pub(super) m_some: Identifier,
    pub(super) m_some_private_name: Identifier,
    pub(super) m_take: Identifier,
    pub(super) m_take_private_name: Identifier,
    pub(super) m_throw: Identifier,
    pub(super) m_throw_private_name: Identifier,
    pub(super) m_to_locale_string: Identifier,
    pub(super) m_to_locale_string_private_name: Identifier,
    pub(super) m_try: Identifier,
    pub(super) m_try_private_name: Identifier,
    pub(super) m_use: Identifier,
    pub(super) m_use_private_name: Identifier,
    pub(super) m_windows: Identifier,
    pub(super) m_windows_private_name: Identifier,
    pub(super) m_wrap_remote_value: Identifier,
    pub(super) m_wrap_remote_value_private_name: Identifier,
    pub(super) m_wrapped_iterator: Identifier,
    pub(super) m_wrapped_iterator_private_name: Identifier,
    pub(super) m_zip: Identifier,
    pub(super) m_zip_private_name: Identifier,
    pub(super) m_zip_keyed: Identifier,
    pub(super) m_zip_keyed_private_name: Identifier,
    pub(super) m_argument: Identifier,
    pub(super) m_argument_private_name: Identifier,
    pub(super) m_argument_count: Identifier,
    pub(super) m_argument_count_private_name: Identifier,
    pub(super) m_array_push: Identifier,
    pub(super) m_array_push_private_name: Identifier,
    pub(super) m_get_by_id_direct: Identifier,
    pub(super) m_get_by_id_direct_private_name: Identifier,
    pub(super) m_get_by_id_direct_private: Identifier,
    pub(super) m_get_by_id_direct_private_private_name: Identifier,
    pub(super) m_get_by_val_with_this: Identifier,
    pub(super) m_get_by_val_with_this_private_name: Identifier,
    pub(super) m_get_prototype_of: Identifier,
    pub(super) m_get_prototype_of_private_name: Identifier,
    pub(super) m_get_internal_field: Identifier,
    pub(super) m_get_internal_field_private_name: Identifier,
    pub(super) m_get_generator_internal_field: Identifier,
    pub(super) m_get_generator_internal_field_private_name: Identifier,
    pub(super) m_get_iterator_helper_internal_field: Identifier,
    pub(super) m_get_iterator_helper_internal_field_private_name: Identifier,
    pub(super) m_get_async_disposable_stack_internal_field: Identifier,
    pub(super) m_get_async_disposable_stack_internal_field_private_name: Identifier,
    pub(super) m_get_array_iterator_internal_field: Identifier,
    pub(super) m_get_array_iterator_internal_field_private_name: Identifier,
    pub(super) m_get_proxy_internal_field: Identifier,
    pub(super) m_get_proxy_internal_field_private_name: Identifier,
    pub(super) m_get_wrap_for_valid_iterator_internal_field: Identifier,
    pub(super) m_get_wrap_for_valid_iterator_internal_field_private_name: Identifier,
    pub(super) m_get_disposable_stack_internal_field: Identifier,
    pub(super) m_get_disposable_stack_internal_field_private_name: Identifier,
    pub(super) m_id_with_profile: Identifier,
    pub(super) m_id_with_profile_private_name: Identifier,
    pub(super) m_is_async_disposable_stack: Identifier,
    pub(super) m_is_async_disposable_stack_private_name: Identifier,
    pub(super) m_is_object: Identifier,
    pub(super) m_is_object_private_name: Identifier,
    pub(super) m_is_callable: Identifier,
    pub(super) m_is_callable_private_name: Identifier,
    pub(super) m_is_constructor: Identifier,
    pub(super) m_is_constructor_private_name: Identifier,
    pub(super) m_is_js_array: Identifier,
    pub(super) m_is_js_array_private_name: Identifier,
    pub(super) m_is_proxy_object: Identifier,
    pub(super) m_is_proxy_object_private_name: Identifier,
    pub(super) m_is_derived_array: Identifier,
    pub(super) m_is_derived_array_private_name: Identifier,
    pub(super) m_is_generator: Identifier,
    pub(super) m_is_generator_private_name: Identifier,
    pub(super) m_is_iterator_helper: Identifier,
    pub(super) m_is_iterator_helper_private_name: Identifier,
    pub(super) m_is_promise: Identifier,
    pub(super) m_is_promise_private_name: Identifier,
    pub(super) m_is_reg_exp_object: Identifier,
    pub(super) m_is_reg_exp_object_private_name: Identifier,
    pub(super) m_is_map: Identifier,
    pub(super) m_is_map_private_name: Identifier,
    pub(super) m_is_set: Identifier,
    pub(super) m_is_set_private_name: Identifier,
    pub(super) m_is_shadow_realm: Identifier,
    pub(super) m_is_shadow_realm_private_name: Identifier,
    pub(super) m_is_array_iterator: Identifier,
    pub(super) m_is_array_iterator_private_name: Identifier,
    pub(super) m_is_undefined_or_null: Identifier,
    pub(super) m_is_undefined_or_null_private_name: Identifier,
    pub(super) m_is_wrap_for_valid_iterator: Identifier,
    pub(super) m_is_wrap_for_valid_iterator_private_name: Identifier,
    pub(super) m_is_disposable_stack: Identifier,
    pub(super) m_is_disposable_stack_private_name: Identifier,
    pub(super) m_throw_type_error: Identifier,
    pub(super) m_throw_type_error_private_name: Identifier,
    pub(super) m_throw_range_error: Identifier,
    pub(super) m_throw_range_error_private_name: Identifier,
    pub(super) m_throw_out_of_memory_error: Identifier,
    pub(super) m_throw_out_of_memory_error_private_name: Identifier,
    pub(super) m_put_by_id_direct: Identifier,
    pub(super) m_put_by_id_direct_private_name: Identifier,
    pub(super) m_put_by_id_direct_private: Identifier,
    pub(super) m_put_by_id_direct_private_private_name: Identifier,
    pub(super) m_put_by_val_direct: Identifier,
    pub(super) m_put_by_val_direct_private_name: Identifier,
    pub(super) m_put_by_val_with_this_sloppy: Identifier,
    pub(super) m_put_by_val_with_this_sloppy_private_name: Identifier,
    pub(super) m_put_by_val_with_this_strict: Identifier,
    pub(super) m_put_by_val_with_this_strict_private_name: Identifier,
    pub(super) m_put_internal_field: Identifier,
    pub(super) m_put_internal_field_private_name: Identifier,
    pub(super) m_put_generator_internal_field: Identifier,
    pub(super) m_put_generator_internal_field_private_name: Identifier,
    pub(super) m_put_async_disposable_stack_internal_field: Identifier,
    pub(super) m_put_async_disposable_stack_internal_field_private_name: Identifier,
    pub(super) m_put_array_iterator_internal_field: Identifier,
    pub(super) m_put_array_iterator_internal_field_private_name: Identifier,
    pub(super) m_put_disposable_stack_internal_field: Identifier,
    pub(super) m_put_disposable_stack_internal_field_private_name: Identifier,
    pub(super) m_super_sampler_begin: Identifier,
    pub(super) m_super_sampler_begin_private_name: Identifier,
    pub(super) m_super_sampler_end: Identifier,
    pub(super) m_super_sampler_end_private_name: Identifier,
    pub(super) m_to_number: Identifier,
    pub(super) m_to_number_private_name: Identifier,
    pub(super) m_to_string: Identifier,
    pub(super) m_to_string_private_name: Identifier,
    pub(super) m_to_property_key: Identifier,
    pub(super) m_to_property_key_private_name: Identifier,
    pub(super) m_to_object: Identifier,
    pub(super) m_to_object_private_name: Identifier,
    pub(super) m_to_this: Identifier,
    pub(super) m_to_this_private_name: Identifier,
    pub(super) m_must_validate_result_of_proxy_get_and_set_traps: Identifier,
    pub(super) m_must_validate_result_of_proxy_get_and_set_traps_private_name: Identifier,
    pub(super) m_must_validate_result_of_proxy_traps_except_get_and_set: Identifier,
    pub(super) m_must_validate_result_of_proxy_traps_except_get_and_set_private_name: Identifier,
    pub(super) m_new_array_with_size: Identifier,
    pub(super) m_new_array_with_size_private_name: Identifier,
    pub(super) m_new_array_with_species: Identifier,
    pub(super) m_new_array_with_species_private_name: Identifier,
    pub(super) m_new_promise: Identifier,
    pub(super) m_new_promise_private_name: Identifier,
    pub(super) m_iterator_generic_close: Identifier,
    pub(super) m_iterator_generic_close_private_name: Identifier,
    pub(super) m_iterator_generic_next: Identifier,
    pub(super) m_iterator_generic_next_private_name: Identifier,
    pub(super) m_if_abrupt_close_iterator: Identifier,
    pub(super) m_if_abrupt_close_iterator_private_name: Identifier,
    pub(super) m_create_promise: Identifier,
    pub(super) m_create_promise_private_name: Identifier,
    pub(super) m_undefined: Identifier,
    pub(super) m_undefined_private_name: Identifier,
    pub(super) m_infinity: Identifier,
    pub(super) m_infinity_private_name: Identifier,
    pub(super) m_iteration_kind_key: Identifier,
    pub(super) m_iteration_kind_key_private_name: Identifier,
    pub(super) m_iteration_kind_value: Identifier,
    pub(super) m_iteration_kind_value_private_name: Identifier,
    pub(super) m_iteration_kind_entries: Identifier,
    pub(super) m_iteration_kind_entries_private_name: Identifier,
    pub(super) m_max_array_index: Identifier,
    pub(super) m_max_array_index_private_name: Identifier,
    pub(super) m_max_string_length: Identifier,
    pub(super) m_max_string_length_private_name: Identifier,
    pub(super) m_max_safe_integer: Identifier,
    pub(super) m_max_safe_integer_private_name: Identifier,
    pub(super) m_module_fetch: Identifier,
    pub(super) m_module_fetch_private_name: Identifier,
    pub(super) m_module_translate: Identifier,
    pub(super) m_module_translate_private_name: Identifier,
    pub(super) m_module_instantiate: Identifier,
    pub(super) m_module_instantiate_private_name: Identifier,
    pub(super) m_module_satisfy: Identifier,
    pub(super) m_module_satisfy_private_name: Identifier,
    pub(super) m_module_link: Identifier,
    pub(super) m_module_link_private_name: Identifier,
    pub(super) m_module_ready: Identifier,
    pub(super) m_module_ready_private_name: Identifier,
    pub(super) m_proxy_field_target: Identifier,
    pub(super) m_proxy_field_target_private_name: Identifier,
    pub(super) m_proxy_field_handler: Identifier,
    pub(super) m_proxy_field_handler_private_name: Identifier,
    pub(super) m_generator_field_state: Identifier,
    pub(super) m_generator_field_state_private_name: Identifier,
    pub(super) m_generator_field_next: Identifier,
    pub(super) m_generator_field_next_private_name: Identifier,
    pub(super) m_generator_field_this: Identifier,
    pub(super) m_generator_field_this_private_name: Identifier,
    pub(super) m_generator_field_frame: Identifier,
    pub(super) m_generator_field_frame_private_name: Identifier,
    pub(super) m_generator_resume_mode_normal: Identifier,
    pub(super) m_generator_resume_mode_normal_private_name: Identifier,
    pub(super) m_generator_resume_mode_throw: Identifier,
    pub(super) m_generator_resume_mode_throw_private_name: Identifier,
    pub(super) m_generator_resume_mode_return: Identifier,
    pub(super) m_generator_resume_mode_return_private_name: Identifier,
    pub(super) m_generator_state_completed: Identifier,
    pub(super) m_generator_state_completed_private_name: Identifier,
    pub(super) m_generator_state_executing: Identifier,
    pub(super) m_generator_state_executing_private_name: Identifier,
    pub(super) m_generator_state_init: Identifier,
    pub(super) m_generator_state_init_private_name: Identifier,
    pub(super) m_iterator_helper_field_generator: Identifier,
    pub(super) m_iterator_helper_field_generator_private_name: Identifier,
    pub(super) m_iterator_helper_field_underlying_iterator: Identifier,
    pub(super) m_iterator_helper_field_underlying_iterator_private_name: Identifier,
    pub(super) m_array_iterator_field_index: Identifier,
    pub(super) m_array_iterator_field_index_private_name: Identifier,
    pub(super) m_array_iterator_field_iterated_object: Identifier,
    pub(super) m_array_iterator_field_iterated_object_private_name: Identifier,
    pub(super) m_array_iterator_field_kind: Identifier,
    pub(super) m_array_iterator_field_kind_private_name: Identifier,
    pub(super) m_wrap_for_valid_iterator_field_iterated_iterator: Identifier,
    pub(super) m_wrap_for_valid_iterator_field_iterated_iterator_private_name: Identifier,
    pub(super) m_wrap_for_valid_iterator_field_iterated_next_method: Identifier,
    pub(super) m_wrap_for_valid_iterator_field_iterated_next_method_private_name: Identifier,
    pub(super) m_disposable_stack_field_state: Identifier,
    pub(super) m_disposable_stack_field_state_private_name: Identifier,
    pub(super) m_disposable_stack_field_capability: Identifier,
    pub(super) m_disposable_stack_field_capability_private_name: Identifier,
    pub(super) m_disposable_stack_state_pending: Identifier,
    pub(super) m_disposable_stack_state_pending_private_name: Identifier,
    pub(super) m_disposable_stack_state_disposed: Identifier,
    pub(super) m_disposable_stack_state_disposed_private_name: Identifier,
    pub(super) m_async_disposable_stack_field_state: Identifier,
    pub(super) m_async_disposable_stack_field_state_private_name: Identifier,
    pub(super) m_async_disposable_stack_field_capability: Identifier,
    pub(super) m_async_disposable_stack_field_capability_private_name: Identifier,
    pub(super) m_async_disposable_stack_state_pending: Identifier,
    pub(super) m_async_disposable_stack_state_pending_private_name: Identifier,
    pub(super) m_async_disposable_stack_state_disposed: Identifier,
    pub(super) m_async_disposable_stack_state_disposed_private_name: Identifier,
    pub(super) m_internal_microtask_async_from_sync_iterator_continue: Identifier,
    pub(super) m_internal_microtask_async_from_sync_iterator_continue_private_name: Identifier,
    pub(super) m_internal_microtask_async_from_sync_iterator_done: Identifier,
    pub(super) m_internal_microtask_async_from_sync_iterator_done_private_name: Identifier,
    pub(super) m_ordered_hash_table_sentinel: Identifier,
    pub(super) m_ordered_hash_table_sentinel_private_name: Identifier,
    pub(super) m_add: Identifier,
    pub(super) m_add_private_name: Identifier,
    pub(super) m_apply_function: Identifier,
    pub(super) m_apply_function_private_name: Identifier,
    pub(super) m_assert: Identifier,
    pub(super) m_assert_private_name: Identifier,
    pub(super) m_call_function: Identifier,
    pub(super) m_call_function_private_name: Identifier,
    pub(super) m_char_code_at: Identifier,
    pub(super) m_char_code_at_private_name: Identifier,
    pub(super) m_executor: Identifier,
    pub(super) m_executor_private_name: Identifier,
    pub(super) m_iterated_object: Identifier,
    pub(super) m_iterated_object_private_name: Identifier,
    pub(super) m_iterated_string: Identifier,
    pub(super) m_iterated_string_private_name: Identifier,
    pub(super) m_promise_dup: Identifier,
    pub(super) m_promise_dup_private_name: Identifier,
    pub(super) m_object: Identifier,
    pub(super) m_object_private_name: Identifier,
    pub(super) m_number: Identifier,
    pub(super) m_number_private_name: Identifier,
    pub(super) m_array: Identifier,
    pub(super) m_array_private_name: Identifier,
    pub(super) m_array_buffer: Identifier,
    pub(super) m_array_buffer_private_name: Identifier,
    pub(super) m_shadow_realm: Identifier,
    pub(super) m_shadow_realm_private_name: Identifier,
    pub(super) m_reg_exp: Identifier,
    pub(super) m_reg_exp_private_name: Identifier,
    pub(super) m_iterator: Identifier,
    pub(super) m_iterator_private_name: Identifier,
    pub(super) m_min: Identifier,
    pub(super) m_min_private_name: Identifier,
    pub(super) m_create: Identifier,
    pub(super) m_create_private_name: Identifier,
    pub(super) m_define_property: Identifier,
    pub(super) m_define_property_private_name: Identifier,
    pub(super) m_default_promise_then: Identifier,
    pub(super) m_default_promise_then_private_name: Identifier,
    pub(super) m_set: Identifier,
    pub(super) m_set_private_name: Identifier,
    pub(super) m_map_upper: Identifier,
    pub(super) m_map_upper_private_name: Identifier,
    pub(super) m_throw_type_error_function: Identifier,
    pub(super) m_throw_type_error_function_private_name: Identifier,
    pub(super) m_typed_array_length: Identifier,
    pub(super) m_typed_array_length_private_name: Identifier,
    pub(super) m_builtin_log: Identifier,
    pub(super) m_builtin_log_private_name: Identifier,
    pub(super) m_builtin_describe: Identifier,
    pub(super) m_builtin_describe_private_name: Identifier,
    pub(super) m_home_object: Identifier,
    pub(super) m_home_object_private_name: Identifier,
    pub(super) m_resolve_promise: Identifier,
    pub(super) m_resolve_promise_private_name: Identifier,
    pub(super) m_reject_promise: Identifier,
    pub(super) m_reject_promise_private_name: Identifier,
    pub(super) m_fulfill_promise: Identifier,
    pub(super) m_fulfill_promise_private_name: Identifier,
    pub(super) m_mark_promise_as_handled: Identifier,
    pub(super) m_mark_promise_as_handled_private_name: Identifier,
    pub(super) m_is_promise_state_pending: Identifier,
    pub(super) m_is_promise_state_pending_private_name: Identifier,
    pub(super) m_resolve_promise_with_first_resolving_function_call_check: Identifier,
    pub(super) m_resolve_promise_with_first_resolving_function_call_check_private_name: Identifier,
    pub(super) m_reject_promise_with_first_resolving_function_call_check: Identifier,
    pub(super) m_reject_promise_with_first_resolving_function_call_check_private_name: Identifier,
    pub(super) m_fulfill_promise_with_first_resolving_function_call_check: Identifier,
    pub(super) m_fulfill_promise_with_first_resolving_function_call_check_private_name: Identifier,
    pub(super) m_new_resolved_promise: Identifier,
    pub(super) m_new_resolved_promise_private_name: Identifier,
    pub(super) m_new_rejected_promise: Identifier,
    pub(super) m_new_rejected_promise_private_name: Identifier,
    pub(super) m_resolve_with_internal_microtask_for_async_await: Identifier,
    pub(super) m_resolve_with_internal_microtask_for_async_await_private_name: Identifier,
    pub(super) m_async_generator_prototype_next: Identifier,
    pub(super) m_async_generator_prototype_next_private_name: Identifier,
    pub(super) m_async_iterator_prototype_symbol_async_iterator: Identifier,
    pub(super) m_async_iterator_prototype_symbol_async_iterator_private_name: Identifier,
    pub(super) m_async_function_drive: Identifier,
    pub(super) m_async_function_drive_private_name: Identifier,
    pub(super) m_new_handled_rejected_promise: Identifier,
    pub(super) m_new_handled_rejected_promise_private_name: Identifier,
    pub(super) m_promise_return_undefined_on_fulfilled: Identifier,
    pub(super) m_promise_return_undefined_on_fulfilled_private_name: Identifier,
    pub(super) m_promise_resolve: Identifier,
    pub(super) m_promise_resolve_private_name: Identifier,
    pub(super) m_promise_reject: Identifier,
    pub(super) m_promise_reject_private_name: Identifier,
    pub(super) m_promise_resolve_with_then: Identifier,
    pub(super) m_promise_resolve_with_then_private_name: Identifier,
    pub(super) m_perform_promise_then: Identifier,
    pub(super) m_perform_promise_then_private_name: Identifier,
    pub(super) m_resolve: Identifier,
    pub(super) m_resolve_private_name: Identifier,
    pub(super) m_reject: Identifier,
    pub(super) m_reject_private_name: Identifier,
    pub(super) m_push: Identifier,
    pub(super) m_push_private_name: Identifier,
    pub(super) m_repeat_character: Identifier,
    pub(super) m_repeat_character_private_name: Identifier,
    pub(super) m_star_default: Identifier,
    pub(super) m_star_default_private_name: Identifier,
    pub(super) m_star_namespace: Identifier,
    pub(super) m_star_namespace_private_name: Identifier,
    pub(super) m_then: Identifier,
    pub(super) m_then_private_name: Identifier,
    pub(super) m_keys: Identifier,
    pub(super) m_keys_private_name: Identifier,
    pub(super) m_values: Identifier,
    pub(super) m_values_private_name: Identifier,
    pub(super) m_set_dup: Identifier,
    pub(super) m_set_dup_private_name: Identifier,
    pub(super) m_clear: Identifier,
    pub(super) m_clear_private_name: Identifier,
    pub(super) m_defer: Identifier,
    pub(super) m_defer_private_name: Identifier,
    pub(super) m_delete: Identifier,
    pub(super) m_delete_private_name: Identifier,
    pub(super) m_size: Identifier,
    pub(super) m_size_private_name: Identifier,
    pub(super) m_shift: Identifier,
    pub(super) m_shift_private_name: Identifier,
    pub(super) m_static_initializer_block: Identifier,
    pub(super) m_static_initializer_block_private_name: Identifier,
    pub(super) m_int8_array: Identifier,
    pub(super) m_int8_array_private_name: Identifier,
    pub(super) m_int16_array: Identifier,
    pub(super) m_int16_array_private_name: Identifier,
    pub(super) m_int32_array: Identifier,
    pub(super) m_int32_array_private_name: Identifier,
    pub(super) m_uint8_array: Identifier,
    pub(super) m_uint8_array_private_name: Identifier,
    pub(super) m_uint8_clamped_array: Identifier,
    pub(super) m_uint8_clamped_array_private_name: Identifier,
    pub(super) m_uint16_array: Identifier,
    pub(super) m_uint16_array_private_name: Identifier,
    pub(super) m_uint32_array: Identifier,
    pub(super) m_uint32_array_private_name: Identifier,
    pub(super) m_float16_array: Identifier,
    pub(super) m_float16_array_private_name: Identifier,
    pub(super) m_float32_array: Identifier,
    pub(super) m_float32_array_private_name: Identifier,
    pub(super) m_float64_array: Identifier,
    pub(super) m_float64_array_private_name: Identifier,
    pub(super) m_big_int64_array: Identifier,
    pub(super) m_big_int64_array_private_name: Identifier,
    pub(super) m_big_uint64_array: Identifier,
    pub(super) m_big_uint64_array_private_name: Identifier,
    pub(super) m_exec: Identifier,
    pub(super) m_exec_private_name: Identifier,
    pub(super) m_generator: Identifier,
    pub(super) m_generator_private_name: Identifier,
    pub(super) m_generator_next: Identifier,
    pub(super) m_generator_next_private_name: Identifier,
    pub(super) m_generator_state: Identifier,
    pub(super) m_generator_state_private_name: Identifier,
    pub(super) m_generator_frame: Identifier,
    pub(super) m_generator_frame_private_name: Identifier,
    pub(super) m_generator_value: Identifier,
    pub(super) m_generator_value_private_name: Identifier,
    pub(super) m_generator_this: Identifier,
    pub(super) m_generator_this_private_name: Identifier,
    pub(super) m_generator_resume_mode: Identifier,
    pub(super) m_generator_resume_mode_private_name: Identifier,
    pub(super) m_this: Identifier,
    pub(super) m_this_private_name: Identifier,
    pub(super) m_to_integer_or_infinity: Identifier,
    pub(super) m_to_integer_or_infinity_private_name: Identifier,
    pub(super) m_to_length: Identifier,
    pub(super) m_to_length_private_name: Identifier,
    pub(super) m_import_in_realm: Identifier,
    pub(super) m_import_in_realm_private_name: Identifier,
    pub(super) m_eval_function: Identifier,
    pub(super) m_eval_function_private_name: Identifier,
    pub(super) m_eval_in_realm: Identifier,
    pub(super) m_eval_in_realm_private_name: Identifier,
    pub(super) m_move_function_to_realm: Identifier,
    pub(super) m_move_function_to_realm_private_name: Identifier,
    pub(super) m_new_target_local: Identifier,
    pub(super) m_new_target_local_private_name: Identifier,
    pub(super) m_derived_constructor: Identifier,
    pub(super) m_derived_constructor_private_name: Identifier,
    pub(super) m_is_typed_array_view: Identifier,
    pub(super) m_is_typed_array_view_private_name: Identifier,
    pub(super) m_is_shared_typed_array_view: Identifier,
    pub(super) m_is_shared_typed_array_view_private_name: Identifier,
    pub(super) m_is_resizable_or_growable_shared_typed_array_view: Identifier,
    pub(super) m_is_resizable_or_growable_shared_typed_array_view_private_name: Identifier,
    pub(super) m_is_detached: Identifier,
    pub(super) m_is_detached_private_name: Identifier,
    pub(super) m_is_typed_array_out_of_bounds: Identifier,
    pub(super) m_is_typed_array_out_of_bounds_private_name: Identifier,
    pub(super) m_typed_array_from_fast: Identifier,
    pub(super) m_typed_array_from_fast_private_name: Identifier,
    pub(super) m_instance_of: Identifier,
    pub(super) m_instance_of_private_name: Identifier,
    pub(super) m_is_array: Identifier,
    pub(super) m_is_array_private_name: Identifier,
    pub(super) m_same_value: Identifier,
    pub(super) m_same_value_private_name: Identifier,
    pub(super) m_reg_exp_create: Identifier,
    pub(super) m_reg_exp_create_private_name: Identifier,
    pub(super) m_is_reg_exp: Identifier,
    pub(super) m_is_reg_exp_private_name: Identifier,
    pub(super) m_is_finite: Identifier,
    pub(super) m_is_finite_private_name: Identifier,
    pub(super) m_make_type_error: Identifier,
    pub(super) m_make_type_error_private_name: Identifier,
    pub(super) m_aggregate_error: Identifier,
    pub(super) m_aggregate_error_private_name: Identifier,
    pub(super) m_map_storage: Identifier,
    pub(super) m_map_storage_private_name: Identifier,
    pub(super) m_map_iteration_next: Identifier,
    pub(super) m_map_iteration_next_private_name: Identifier,
    pub(super) m_map_iteration_entry: Identifier,
    pub(super) m_map_iteration_entry_private_name: Identifier,
    pub(super) m_map_iteration_entry_key: Identifier,
    pub(super) m_map_iteration_entry_key_private_name: Identifier,
    pub(super) m_map_iteration_entry_value: Identifier,
    pub(super) m_map_iteration_entry_value_private_name: Identifier,
    pub(super) m_set_storage: Identifier,
    pub(super) m_set_storage_private_name: Identifier,
    pub(super) m_set_iteration_next: Identifier,
    pub(super) m_set_iteration_next_private_name: Identifier,
    pub(super) m_set_iteration_entry: Identifier,
    pub(super) m_set_iteration_entry_private_name: Identifier,
    pub(super) m_set_iteration_entry_key: Identifier,
    pub(super) m_set_iteration_entry_key_private_name: Identifier,
    pub(super) m_set_prototype_direct: Identifier,
    pub(super) m_set_prototype_direct_private_name: Identifier,
    pub(super) m_set_prototype_direct_or_throw: Identifier,
    pub(super) m_set_prototype_direct_or_throw_private_name: Identifier,
    pub(super) m_reg_exp_builtin_exec: Identifier,
    pub(super) m_reg_exp_builtin_exec_private_name: Identifier,
    pub(super) m_reg_exp_proto_flags_getter: Identifier,
    pub(super) m_reg_exp_proto_flags_getter_private_name: Identifier,
    pub(super) m_reg_exp_proto_has_indices_getter: Identifier,
    pub(super) m_reg_exp_proto_has_indices_getter_private_name: Identifier,
    pub(super) m_reg_exp_proto_global_getter: Identifier,
    pub(super) m_reg_exp_proto_global_getter_private_name: Identifier,
    pub(super) m_reg_exp_proto_ignore_case_getter: Identifier,
    pub(super) m_reg_exp_proto_ignore_case_getter_private_name: Identifier,
    pub(super) m_reg_exp_proto_multiline_getter: Identifier,
    pub(super) m_reg_exp_proto_multiline_getter_private_name: Identifier,
    pub(super) m_reg_exp_proto_source_getter: Identifier,
    pub(super) m_reg_exp_proto_source_getter_private_name: Identifier,
    pub(super) m_reg_exp_proto_sticky_getter: Identifier,
    pub(super) m_reg_exp_proto_sticky_getter_private_name: Identifier,
    pub(super) m_reg_exp_proto_dot_all_getter: Identifier,
    pub(super) m_reg_exp_proto_dot_all_getter_private_name: Identifier,
    pub(super) m_reg_exp_proto_unicode_getter: Identifier,
    pub(super) m_reg_exp_proto_unicode_getter_private_name: Identifier,
    pub(super) m_reg_exp_proto_unicode_sets_getter: Identifier,
    pub(super) m_reg_exp_proto_unicode_sets_getter_private_name: Identifier,
    pub(super) m_reg_exp_prototype_symbol_match: Identifier,
    pub(super) m_reg_exp_prototype_symbol_match_private_name: Identifier,
    pub(super) m_reg_exp_prototype_symbol_match_all: Identifier,
    pub(super) m_reg_exp_prototype_symbol_match_all_private_name: Identifier,
    pub(super) m_reg_exp_prototype_symbol_replace: Identifier,
    pub(super) m_reg_exp_prototype_symbol_replace_private_name: Identifier,
    pub(super) m_reg_exp_search_fast: Identifier,
    pub(super) m_reg_exp_search_fast_private_name: Identifier,
    pub(super) m_string_includes_internal: Identifier,
    pub(super) m_string_includes_internal_private_name: Identifier,
    pub(super) m_string_index_of_internal: Identifier,
    pub(super) m_string_index_of_internal_private_name: Identifier,
    pub(super) m_string_substring: Identifier,
    pub(super) m_string_substring_private_name: Identifier,
    pub(super) m_handle_negative_proxy_has_trap_result: Identifier,
    pub(super) m_handle_negative_proxy_has_trap_result_private_name: Identifier,
    pub(super) m_handle_positive_proxy_set_trap_result: Identifier,
    pub(super) m_handle_positive_proxy_set_trap_result_private_name: Identifier,
    pub(super) m_handle_proxy_get_trap_result: Identifier,
    pub(super) m_handle_proxy_get_trap_result_private_name: Identifier,
    pub(super) m_import_module: Identifier,
    pub(super) m_import_module_private_name: Identifier,
    pub(super) m_module_fetch_failure_kind: Identifier,
    pub(super) m_module_fetch_failure_kind_private_name: Identifier,
    pub(super) m_module_failure_module_record: Identifier,
    pub(super) m_module_failure_module_record_private_name: Identifier,
    pub(super) m_module_failure_module_key: Identifier,
    pub(super) m_module_failure_module_key_private_name: Identifier,
    pub(super) m_module_failure_module_type: Identifier,
    pub(super) m_module_failure_module_type_private_name: Identifier,
    pub(super) m_module_failure_kind: Identifier,
    pub(super) m_module_failure_kind_private_name: Identifier,
    pub(super) m_copy_data_properties: Identifier,
    pub(super) m_copy_data_properties_private_name: Identifier,
    pub(super) m_clone_object: Identifier,
    pub(super) m_clone_object_private_name: Identifier,
    pub(super) m_meta: Identifier,
    pub(super) m_meta_private_name: Identifier,
    pub(super) m_instance_field_initializer: Identifier,
    pub(super) m_instance_field_initializer_private_name: Identifier,
    pub(super) m_private_brand: Identifier,
    pub(super) m_private_brand_private_name: Identifier,
    pub(super) m_private_class_brand: Identifier,
    pub(super) m_private_class_brand_private_name: Identifier,
    pub(super) m_has_own_property_function: Identifier,
    pub(super) m_has_own_property_function_private_name: Identifier,
    pub(super) m_create_private_symbol: Identifier,
    pub(super) m_create_private_symbol_private_name: Identifier,
    pub(super) m_entries: Identifier,
    pub(super) m_entries_private_name: Identifier,
    pub(super) m_empty_property_name_enumerator: Identifier,
    pub(super) m_empty_property_name_enumerator_private_name: Identifier,
    pub(super) m_sentinel_string: Identifier,
    pub(super) m_sentinel_string_private_name: Identifier,
    pub(super) m_create_remote_function: Identifier,
    pub(super) m_create_remote_function_private_name: Identifier,
    pub(super) m_is_remote_function: Identifier,
    pub(super) m_is_remote_function_private_name: Identifier,
    pub(super) m_array_from_fast_without_map_fn: Identifier,
    pub(super) m_array_from_fast_without_map_fn_private_name: Identifier,
    pub(super) m_json_parse: Identifier,
    pub(super) m_json_parse_private_name: Identifier,
    pub(super) m_json_stringify: Identifier,
    pub(super) m_json_stringify_private_name: Identifier,
    pub(super) m_string: Identifier,
    pub(super) m_string_private_name: Identifier,
    pub(super) m_substr: Identifier,
    pub(super) m_substr_private_name: Identifier,
    pub(super) m_ends_with: Identifier,
    pub(super) m_ends_with_private_name: Identifier,
    pub(super) m_get_own_property_descriptor: Identifier,
    pub(super) m_get_own_property_descriptor_private_name: Identifier,
    pub(super) m_get_own_property_names: Identifier,
    pub(super) m_get_own_property_names_private_name: Identifier,
    pub(super) m_get_own_property_symbols: Identifier,
    pub(super) m_get_own_property_symbols_private_name: Identifier,
    pub(super) m_has_own: Identifier,
    pub(super) m_has_own_private_name: Identifier,
    pub(super) m_index_of: Identifier,
    pub(super) m_index_of_private_name: Identifier,
    pub(super) m_pop: Identifier,
    pub(super) m_pop_private_name: Identifier,
    pub(super) m_async_context: Identifier,
    pub(super) m_async_context_private_name: Identifier,
    pub(super) m_wrap_for_valid_iterator_create: Identifier,
    pub(super) m_wrap_for_valid_iterator_create_private_name: Identifier,
    pub(super) m_async_from_sync_iterator_create: Identifier,
    pub(super) m_async_from_sync_iterator_create_private_name: Identifier,
    pub(super) m_reg_exp_string_iterator_create: Identifier,
    pub(super) m_reg_exp_string_iterator_create_private_name: Identifier,
    pub(super) m_iterator_helper_create: Identifier,
    pub(super) m_iterator_helper_create_private_name: Identifier,
    pub(super) m_own_keys: Identifier,
    pub(super) m_own_keys_private_name: Identifier,
    pub(super) m_includes: Identifier,
    pub(super) m_includes_private_name: Identifier,
    pub(super) m_reference_error: Identifier,
    pub(super) m_reference_error_private_name: Identifier,
    pub(super) m_suppressed_error: Identifier,
    pub(super) m_suppressed_error_private_name: Identifier,
    pub(super) m_disposable_stack: Identifier,
    pub(super) m_disposable_stack_private_name: Identifier,
    pub(super) m_async_disposable_stack: Identifier,
    pub(super) m_async_disposable_stack_private_name: Identifier,
    pub(super) m_enqueue_job: Identifier,
    pub(super) m_enqueue_job_private_name: Identifier,
    pub(super) m_has_instance_symbol: Identifier,
    pub(super) m_has_instance_symbol_private_identifier: Identifier,
    pub(super) m_is_concat_spreadable_symbol: Identifier,
    pub(super) m_is_concat_spreadable_symbol_private_identifier: Identifier,
    pub(super) m_async_iterator_symbol: Identifier,
    pub(super) m_async_iterator_symbol_private_identifier: Identifier,
    pub(super) m_iterator_symbol: Identifier,
    pub(super) m_iterator_symbol_private_identifier: Identifier,
    pub(super) m_match_symbol: Identifier,
    pub(super) m_match_symbol_private_identifier: Identifier,
    pub(super) m_match_all_symbol: Identifier,
    pub(super) m_match_all_symbol_private_identifier: Identifier,
    pub(super) m_replace_symbol: Identifier,
    pub(super) m_replace_symbol_private_identifier: Identifier,
    pub(super) m_search_symbol: Identifier,
    pub(super) m_search_symbol_private_identifier: Identifier,
    pub(super) m_species_symbol: Identifier,
    pub(super) m_species_symbol_private_identifier: Identifier,
    pub(super) m_split_symbol: Identifier,
    pub(super) m_split_symbol_private_identifier: Identifier,
    pub(super) m_to_primitive_symbol: Identifier,
    pub(super) m_to_primitive_symbol_private_identifier: Identifier,
    pub(super) m_to_string_tag_symbol: Identifier,
    pub(super) m_to_string_tag_symbol_private_identifier: Identifier,
    pub(super) m_unscopables_symbol: Identifier,
    pub(super) m_unscopables_symbol_private_identifier: Identifier,
    pub(super) m_dispose_symbol: Identifier,
    pub(super) m_dispose_symbol_private_identifier: Identifier,
    pub(super) m_async_dispose_symbol: Identifier,
    pub(super) m_async_dispose_symbol_private_identifier: Identifier,
    pub(super) m_dollar_vm_name: Identifier,
    pub(super) m_dollar_vm_private_name: Identifier,
    pub(super) m_poly_proto_private_name: Identifier,
    pub(super) m_stack_private_name: Identifier,
    pub(super) m_private_name_set: PrivateNameSet,
    pub(super) m_well_known_symbols_map: WellKnownSymbolMap,
}

impl BuiltinNames {
    /// `BuiltinNames(VM&, CommonIdentifiers*)`: só o `emptyIdentifier` do `CommonIdentifiers` é lido.
    pub fn new(vm: &VM, empty_identifier: &Identifier) -> BuiltinNames {
        let public = |name: &'static [u8]| Identifier::from_span(vm, name);
        let private = |name: &'static [u8]| {
            Identifier::from_uid_symbol(&StaticSymbolImpl::new8(name, S_FLAG_IS_PRIVATE).symbol_impl())
        };
        let well_known = |name: &'static [u8]| {
            Identifier::from_uid_symbol(&StaticSymbolImpl::new8(name, S_FLAG_DEFAULT).symbol_impl())
        };
        let mut this = BuiltinNames {
            m_empty_identifier: empty_identifier.clone(),
            m_promise: public(b"Promise"),
            m_promise_private_name: private(b"Promise"),
            m_add_disposable_resource: public(b"addDisposableResource"),
            m_add_disposable_resource_private_name: private(b"addDisposableResource"),
            m_adopt: public(b"adopt"),
            m_adopt_private_name: private(b"adopt"),
            m_apply: public(b"apply"),
            m_apply_private_name: private(b"apply"),
            m_array_iterator_next_helper: public(b"arrayIteratorNextHelper"),
            m_array_iterator_next_helper_private_name: private(b"arrayIteratorNextHelper"),
            m_async_dispose: public(b"asyncDispose"),
            m_async_dispose_private_name: private(b"asyncDispose"),
            m_at: public(b"at"),
            m_at_private_name: private(b"at"),
            m_builtin_map_iterable: public(b"builtinMapIterable"),
            m_builtin_map_iterable_private_name: private(b"builtinMapIterable"),
            m_builtin_set_iterable: public(b"builtinSetIterable"),
            m_builtin_set_iterable_private_name: private(b"builtinSetIterable"),
            m_call: public(b"call"),
            m_call_private_name: private(b"call"),
            m_chunks: public(b"chunks"),
            m_chunks_private_name: private(b"chunks"),
            m_close_all_iterators: public(b"closeAllIterators"),
            m_close_all_iterators_private_name: private(b"closeAllIterators"),
            m_concat: public(b"concat"),
            m_concat_private_name: private(b"concat"),
            m_create_array_without_prototype: public(b"createArrayWithoutPrototype"),
            m_create_array_without_prototype_private_name: private(b"createArrayWithoutPrototype"),
            m_create_disposable_resource: public(b"createDisposableResource"),
            m_create_disposable_resource_private_name: private(b"createDisposableResource"),
            m_create_inspector_injected_script: public(b"createInspectorInjectedScript"),
            m_create_inspector_injected_script_private_name: private(b"createInspectorInjectedScript"),
            m_create_object_without_prototype: public(b"createObjectWithoutPrototype"),
            m_create_object_without_prototype_private_name: private(b"createObjectWithoutPrototype"),
            m_cross_realm_throw: public(b"crossRealmThrow"),
            m_cross_realm_throw_private_name: private(b"crossRealmThrow"),
            m_default_async_from_async_array_like: public(b"defaultAsyncFromAsyncArrayLike"),
            m_default_async_from_async_array_like_private_name: private(b"defaultAsyncFromAsyncArrayLike"),
            m_default_async_from_async_iterator: public(b"defaultAsyncFromAsyncIterator"),
            m_default_async_from_async_iterator_private_name: private(b"defaultAsyncFromAsyncIterator"),
            m_defer_method: public(b"deferMethod"),
            m_defer_method_private_name: private(b"deferMethod"),
            m_delete_property: public(b"deleteProperty"),
            m_delete_property_private_name: private(b"deleteProperty"),
            m_dispose: public(b"dispose"),
            m_dispose_private_name: private(b"dispose"),
            m_dispose_async: public(b"disposeAsync"),
            m_dispose_async_private_name: private(b"disposeAsync"),
            m_drop: public(b"drop"),
            m_drop_private_name: private(b"drop"),
            m_evaluate: public(b"evaluate"),
            m_evaluate_private_name: private(b"evaluate"),
            m_every: public(b"every"),
            m_every_private_name: private(b"every"),
            m_filter: public(b"filter"),
            m_filter_private_name: private(b"filter"),
            m_find: public(b"find"),
            m_find_private_name: private(b"find"),
            m_find_index: public(b"findIndex"),
            m_find_index_private_name: private(b"findIndex"),
            m_find_last: public(b"findLast"),
            m_find_last_private_name: private(b"findLast"),
            m_find_last_index: public(b"findLastIndex"),
            m_find_last_index_private_name: private(b"findLastIndex"),
            m_flat_into_array: public(b"flatIntoArray"),
            m_flat_into_array_private_name: private(b"flatIntoArray"),
            m_flat_into_array_with_callback: public(b"flatIntoArrayWithCallback"),
            m_flat_into_array_with_callback_private_name: private(b"flatIntoArrayWithCallback"),
            m_flat_map: public(b"flatMap"),
            m_flat_map_private_name: private(b"flatMap"),
            m_for_each: public(b"forEach"),
            m_for_each_private_name: private(b"forEach"),
            m_from: public(b"from"),
            m_from_private_name: private(b"from"),
            m_from_async: public(b"fromAsync"),
            m_from_async_private_name: private(b"fromAsync"),
            m_from_entries: public(b"fromEntries"),
            m_from_entries_private_name: private(b"fromEntries"),
            m_generator_resume: public(b"generatorResume"),
            m_generator_resume_private_name: private(b"generatorResume"),
            m_get: public(b"get"),
            m_get_private_name: private(b"get"),
            m_get_async_dispose_method: public(b"getAsyncDisposeMethod"),
            m_get_async_dispose_method_private_name: private(b"getAsyncDisposeMethod"),
            m_get_dispose_method: public(b"getDisposeMethod"),
            m_get_dispose_method_private_name: private(b"getDisposeMethod"),
            m_get_iterator_flattenable: public(b"getIteratorFlattenable"),
            m_get_iterator_flattenable_private_name: private(b"getIteratorFlattenable"),
            m_get_iterator_sync: public(b"getIteratorSync"),
            m_get_iterator_sync_private_name: private(b"getIteratorSync"),
            m_get_options_object: public(b"getOptionsObject"),
            m_get_options_object_private_name: private(b"getOptionsObject"),
            m_group_by: public(b"groupBy"),
            m_group_by_private_name: private(b"groupBy"),
            m_has: public(b"has"),
            m_has_private_name: private(b"has"),
            m_import_value: public(b"importValue"),
            m_import_value_private_name: private(b"importValue"),
            m_iterator_close_all_normal: public(b"iteratorCloseAllNormal"),
            m_iterator_close_all_normal_private_name: private(b"iteratorCloseAllNormal"),
            m_iterator_zip: public(b"iteratorZip"),
            m_iterator_zip_private_name: private(b"iteratorZip"),
            m_map: public(b"map"),
            m_map_private_name: private(b"map"),
            m_move: public(b"move"),
            m_move_private_name: private(b"move"),
            m_next: public(b"next"),
            m_next_private_name: private(b"next"),
            m_perform_iteration: public(b"performIteration"),
            m_perform_iteration_private_name: private(b"performIteration"),
            m_perform_proxy_object_get: public(b"performProxyObjectGet"),
            m_perform_proxy_object_get_private_name: private(b"performProxyObjectGet"),
            m_perform_proxy_object_get_by_val: public(b"performProxyObjectGetByVal"),
            m_perform_proxy_object_get_by_val_private_name: private(b"performProxyObjectGetByVal"),
            m_perform_proxy_object_has: public(b"performProxyObjectHas"),
            m_perform_proxy_object_has_private_name: private(b"performProxyObjectHas"),
            m_perform_proxy_object_has_by_val: public(b"performProxyObjectHasByVal"),
            m_perform_proxy_object_has_by_val_private_name: private(b"performProxyObjectHasByVal"),
            m_perform_proxy_object_set_by_val_sloppy: public(b"performProxyObjectSetByValSloppy"),
            m_perform_proxy_object_set_by_val_sloppy_private_name: private(b"performProxyObjectSetByValSloppy"),
            m_perform_proxy_object_set_by_val_strict: public(b"performProxyObjectSetByValStrict"),
            m_perform_proxy_object_set_by_val_strict_private_name: private(b"performProxyObjectSetByValStrict"),
            m_perform_proxy_object_set_sloppy: public(b"performProxyObjectSetSloppy"),
            m_perform_proxy_object_set_sloppy_private_name: private(b"performProxyObjectSetSloppy"),
            m_perform_proxy_object_set_strict: public(b"performProxyObjectSetStrict"),
            m_perform_proxy_object_set_strict_private_name: private(b"performProxyObjectSetStrict"),
            m_reduce: public(b"reduce"),
            m_reduce_private_name: private(b"reduce"),
            m_reduce_right: public(b"reduceRight"),
            m_reduce_right_private_name: private(b"reduceRight"),
            m_remove_first_from_list: public(b"removeFirstFromList"),
            m_remove_first_from_list_private_name: private(b"removeFirstFromList"),
            m_return: public(b"return"),
            m_return_private_name: private(b"return"),
            m_some: public(b"some"),
            m_some_private_name: private(b"some"),
            m_take: public(b"take"),
            m_take_private_name: private(b"take"),
            m_throw: public(b"throw"),
            m_throw_private_name: private(b"throw"),
            m_to_locale_string: public(b"toLocaleString"),
            m_to_locale_string_private_name: private(b"toLocaleString"),
            m_try: public(b"try"),
            m_try_private_name: private(b"try"),
            m_use: public(b"use"),
            m_use_private_name: private(b"use"),
            m_windows: public(b"windows"),
            m_windows_private_name: private(b"windows"),
            m_wrap_remote_value: public(b"wrapRemoteValue"),
            m_wrap_remote_value_private_name: private(b"wrapRemoteValue"),
            m_wrapped_iterator: public(b"wrappedIterator"),
            m_wrapped_iterator_private_name: private(b"wrappedIterator"),
            m_zip: public(b"zip"),
            m_zip_private_name: private(b"zip"),
            m_zip_keyed: public(b"zipKeyed"),
            m_zip_keyed_private_name: private(b"zipKeyed"),
            m_argument: public(b"argument"),
            m_argument_private_name: private(b"argument"),
            m_argument_count: public(b"argumentCount"),
            m_argument_count_private_name: private(b"argumentCount"),
            m_array_push: public(b"arrayPush"),
            m_array_push_private_name: private(b"arrayPush"),
            m_get_by_id_direct: public(b"getByIdDirect"),
            m_get_by_id_direct_private_name: private(b"getByIdDirect"),
            m_get_by_id_direct_private: public(b"getByIdDirectPrivate"),
            m_get_by_id_direct_private_private_name: private(b"getByIdDirectPrivate"),
            m_get_by_val_with_this: public(b"getByValWithThis"),
            m_get_by_val_with_this_private_name: private(b"getByValWithThis"),
            m_get_prototype_of: public(b"getPrototypeOf"),
            m_get_prototype_of_private_name: private(b"getPrototypeOf"),
            m_get_internal_field: public(b"getInternalField"),
            m_get_internal_field_private_name: private(b"getInternalField"),
            m_get_generator_internal_field: public(b"getGeneratorInternalField"),
            m_get_generator_internal_field_private_name: private(b"getGeneratorInternalField"),
            m_get_iterator_helper_internal_field: public(b"getIteratorHelperInternalField"),
            m_get_iterator_helper_internal_field_private_name: private(b"getIteratorHelperInternalField"),
            m_get_async_disposable_stack_internal_field: public(b"getAsyncDisposableStackInternalField"),
            m_get_async_disposable_stack_internal_field_private_name: private(b"getAsyncDisposableStackInternalField"),
            m_get_array_iterator_internal_field: public(b"getArrayIteratorInternalField"),
            m_get_array_iterator_internal_field_private_name: private(b"getArrayIteratorInternalField"),
            m_get_proxy_internal_field: public(b"getProxyInternalField"),
            m_get_proxy_internal_field_private_name: private(b"getProxyInternalField"),
            m_get_wrap_for_valid_iterator_internal_field: public(b"getWrapForValidIteratorInternalField"),
            m_get_wrap_for_valid_iterator_internal_field_private_name: private(b"getWrapForValidIteratorInternalField"),
            m_get_disposable_stack_internal_field: public(b"getDisposableStackInternalField"),
            m_get_disposable_stack_internal_field_private_name: private(b"getDisposableStackInternalField"),
            m_id_with_profile: public(b"idWithProfile"),
            m_id_with_profile_private_name: private(b"idWithProfile"),
            m_is_async_disposable_stack: public(b"isAsyncDisposableStack"),
            m_is_async_disposable_stack_private_name: private(b"isAsyncDisposableStack"),
            m_is_object: public(b"isObject"),
            m_is_object_private_name: private(b"isObject"),
            m_is_callable: public(b"isCallable"),
            m_is_callable_private_name: private(b"isCallable"),
            m_is_constructor: public(b"isConstructor"),
            m_is_constructor_private_name: private(b"isConstructor"),
            m_is_js_array: public(b"isJSArray"),
            m_is_js_array_private_name: private(b"isJSArray"),
            m_is_proxy_object: public(b"isProxyObject"),
            m_is_proxy_object_private_name: private(b"isProxyObject"),
            m_is_derived_array: public(b"isDerivedArray"),
            m_is_derived_array_private_name: private(b"isDerivedArray"),
            m_is_generator: public(b"isGenerator"),
            m_is_generator_private_name: private(b"isGenerator"),
            m_is_iterator_helper: public(b"isIteratorHelper"),
            m_is_iterator_helper_private_name: private(b"isIteratorHelper"),
            m_is_promise: public(b"isPromise"),
            m_is_promise_private_name: private(b"isPromise"),
            m_is_reg_exp_object: public(b"isRegExpObject"),
            m_is_reg_exp_object_private_name: private(b"isRegExpObject"),
            m_is_map: public(b"isMap"),
            m_is_map_private_name: private(b"isMap"),
            m_is_set: public(b"isSet"),
            m_is_set_private_name: private(b"isSet"),
            m_is_shadow_realm: public(b"isShadowRealm"),
            m_is_shadow_realm_private_name: private(b"isShadowRealm"),
            m_is_array_iterator: public(b"isArrayIterator"),
            m_is_array_iterator_private_name: private(b"isArrayIterator"),
            m_is_undefined_or_null: public(b"isUndefinedOrNull"),
            m_is_undefined_or_null_private_name: private(b"isUndefinedOrNull"),
            m_is_wrap_for_valid_iterator: public(b"isWrapForValidIterator"),
            m_is_wrap_for_valid_iterator_private_name: private(b"isWrapForValidIterator"),
            m_is_disposable_stack: public(b"isDisposableStack"),
            m_is_disposable_stack_private_name: private(b"isDisposableStack"),
            m_throw_type_error: public(b"throwTypeError"),
            m_throw_type_error_private_name: private(b"throwTypeError"),
            m_throw_range_error: public(b"throwRangeError"),
            m_throw_range_error_private_name: private(b"throwRangeError"),
            m_throw_out_of_memory_error: public(b"throwOutOfMemoryError"),
            m_throw_out_of_memory_error_private_name: private(b"throwOutOfMemoryError"),
            m_put_by_id_direct: public(b"putByIdDirect"),
            m_put_by_id_direct_private_name: private(b"putByIdDirect"),
            m_put_by_id_direct_private: public(b"putByIdDirectPrivate"),
            m_put_by_id_direct_private_private_name: private(b"putByIdDirectPrivate"),
            m_put_by_val_direct: public(b"putByValDirect"),
            m_put_by_val_direct_private_name: private(b"putByValDirect"),
            m_put_by_val_with_this_sloppy: public(b"putByValWithThisSloppy"),
            m_put_by_val_with_this_sloppy_private_name: private(b"putByValWithThisSloppy"),
            m_put_by_val_with_this_strict: public(b"putByValWithThisStrict"),
            m_put_by_val_with_this_strict_private_name: private(b"putByValWithThisStrict"),
            m_put_internal_field: public(b"putInternalField"),
            m_put_internal_field_private_name: private(b"putInternalField"),
            m_put_generator_internal_field: public(b"putGeneratorInternalField"),
            m_put_generator_internal_field_private_name: private(b"putGeneratorInternalField"),
            m_put_async_disposable_stack_internal_field: public(b"putAsyncDisposableStackInternalField"),
            m_put_async_disposable_stack_internal_field_private_name: private(b"putAsyncDisposableStackInternalField"),
            m_put_array_iterator_internal_field: public(b"putArrayIteratorInternalField"),
            m_put_array_iterator_internal_field_private_name: private(b"putArrayIteratorInternalField"),
            m_put_disposable_stack_internal_field: public(b"putDisposableStackInternalField"),
            m_put_disposable_stack_internal_field_private_name: private(b"putDisposableStackInternalField"),
            m_super_sampler_begin: public(b"superSamplerBegin"),
            m_super_sampler_begin_private_name: private(b"superSamplerBegin"),
            m_super_sampler_end: public(b"superSamplerEnd"),
            m_super_sampler_end_private_name: private(b"superSamplerEnd"),
            m_to_number: public(b"toNumber"),
            m_to_number_private_name: private(b"toNumber"),
            m_to_string: public(b"toString"),
            m_to_string_private_name: private(b"toString"),
            m_to_property_key: public(b"toPropertyKey"),
            m_to_property_key_private_name: private(b"toPropertyKey"),
            m_to_object: public(b"toObject"),
            m_to_object_private_name: private(b"toObject"),
            m_to_this: public(b"toThis"),
            m_to_this_private_name: private(b"toThis"),
            m_must_validate_result_of_proxy_get_and_set_traps: public(b"mustValidateResultOfProxyGetAndSetTraps"),
            m_must_validate_result_of_proxy_get_and_set_traps_private_name: private(b"mustValidateResultOfProxyGetAndSetTraps"),
            m_must_validate_result_of_proxy_traps_except_get_and_set: public(b"mustValidateResultOfProxyTrapsExceptGetAndSet"),
            m_must_validate_result_of_proxy_traps_except_get_and_set_private_name: private(b"mustValidateResultOfProxyTrapsExceptGetAndSet"),
            m_new_array_with_size: public(b"newArrayWithSize"),
            m_new_array_with_size_private_name: private(b"newArrayWithSize"),
            m_new_array_with_species: public(b"newArrayWithSpecies"),
            m_new_array_with_species_private_name: private(b"newArrayWithSpecies"),
            m_new_promise: public(b"newPromise"),
            m_new_promise_private_name: private(b"newPromise"),
            m_iterator_generic_close: public(b"iteratorGenericClose"),
            m_iterator_generic_close_private_name: private(b"iteratorGenericClose"),
            m_iterator_generic_next: public(b"iteratorGenericNext"),
            m_iterator_generic_next_private_name: private(b"iteratorGenericNext"),
            m_if_abrupt_close_iterator: public(b"ifAbruptCloseIterator"),
            m_if_abrupt_close_iterator_private_name: private(b"ifAbruptCloseIterator"),
            m_create_promise: public(b"createPromise"),
            m_create_promise_private_name: private(b"createPromise"),
            m_undefined: public(b"undefined"),
            m_undefined_private_name: private(b"undefined"),
            m_infinity: public(b"Infinity"),
            m_infinity_private_name: private(b"Infinity"),
            m_iteration_kind_key: public(b"iterationKindKey"),
            m_iteration_kind_key_private_name: private(b"iterationKindKey"),
            m_iteration_kind_value: public(b"iterationKindValue"),
            m_iteration_kind_value_private_name: private(b"iterationKindValue"),
            m_iteration_kind_entries: public(b"iterationKindEntries"),
            m_iteration_kind_entries_private_name: private(b"iterationKindEntries"),
            m_max_array_index: public(b"MAX_ARRAY_INDEX"),
            m_max_array_index_private_name: private(b"MAX_ARRAY_INDEX"),
            m_max_string_length: public(b"MAX_STRING_LENGTH"),
            m_max_string_length_private_name: private(b"MAX_STRING_LENGTH"),
            m_max_safe_integer: public(b"MAX_SAFE_INTEGER"),
            m_max_safe_integer_private_name: private(b"MAX_SAFE_INTEGER"),
            m_module_fetch: public(b"ModuleFetch"),
            m_module_fetch_private_name: private(b"ModuleFetch"),
            m_module_translate: public(b"ModuleTranslate"),
            m_module_translate_private_name: private(b"ModuleTranslate"),
            m_module_instantiate: public(b"ModuleInstantiate"),
            m_module_instantiate_private_name: private(b"ModuleInstantiate"),
            m_module_satisfy: public(b"ModuleSatisfy"),
            m_module_satisfy_private_name: private(b"ModuleSatisfy"),
            m_module_link: public(b"ModuleLink"),
            m_module_link_private_name: private(b"ModuleLink"),
            m_module_ready: public(b"ModuleReady"),
            m_module_ready_private_name: private(b"ModuleReady"),
            m_proxy_field_target: public(b"proxyFieldTarget"),
            m_proxy_field_target_private_name: private(b"proxyFieldTarget"),
            m_proxy_field_handler: public(b"proxyFieldHandler"),
            m_proxy_field_handler_private_name: private(b"proxyFieldHandler"),
            m_generator_field_state: public(b"generatorFieldState"),
            m_generator_field_state_private_name: private(b"generatorFieldState"),
            m_generator_field_next: public(b"generatorFieldNext"),
            m_generator_field_next_private_name: private(b"generatorFieldNext"),
            m_generator_field_this: public(b"generatorFieldThis"),
            m_generator_field_this_private_name: private(b"generatorFieldThis"),
            m_generator_field_frame: public(b"generatorFieldFrame"),
            m_generator_field_frame_private_name: private(b"generatorFieldFrame"),
            m_generator_resume_mode_normal: public(b"GeneratorResumeModeNormal"),
            m_generator_resume_mode_normal_private_name: private(b"GeneratorResumeModeNormal"),
            m_generator_resume_mode_throw: public(b"GeneratorResumeModeThrow"),
            m_generator_resume_mode_throw_private_name: private(b"GeneratorResumeModeThrow"),
            m_generator_resume_mode_return: public(b"GeneratorResumeModeReturn"),
            m_generator_resume_mode_return_private_name: private(b"GeneratorResumeModeReturn"),
            m_generator_state_completed: public(b"GeneratorStateCompleted"),
            m_generator_state_completed_private_name: private(b"GeneratorStateCompleted"),
            m_generator_state_executing: public(b"GeneratorStateExecuting"),
            m_generator_state_executing_private_name: private(b"GeneratorStateExecuting"),
            m_generator_state_init: public(b"GeneratorStateInit"),
            m_generator_state_init_private_name: private(b"GeneratorStateInit"),
            m_iterator_helper_field_generator: public(b"iteratorHelperFieldGenerator"),
            m_iterator_helper_field_generator_private_name: private(b"iteratorHelperFieldGenerator"),
            m_iterator_helper_field_underlying_iterator: public(b"iteratorHelperFieldUnderlyingIterator"),
            m_iterator_helper_field_underlying_iterator_private_name: private(b"iteratorHelperFieldUnderlyingIterator"),
            m_array_iterator_field_index: public(b"arrayIteratorFieldIndex"),
            m_array_iterator_field_index_private_name: private(b"arrayIteratorFieldIndex"),
            m_array_iterator_field_iterated_object: public(b"arrayIteratorFieldIteratedObject"),
            m_array_iterator_field_iterated_object_private_name: private(b"arrayIteratorFieldIteratedObject"),
            m_array_iterator_field_kind: public(b"arrayIteratorFieldKind"),
            m_array_iterator_field_kind_private_name: private(b"arrayIteratorFieldKind"),
            m_wrap_for_valid_iterator_field_iterated_iterator: public(b"wrapForValidIteratorFieldIteratedIterator"),
            m_wrap_for_valid_iterator_field_iterated_iterator_private_name: private(b"wrapForValidIteratorFieldIteratedIterator"),
            m_wrap_for_valid_iterator_field_iterated_next_method: public(b"wrapForValidIteratorFieldIteratedNextMethod"),
            m_wrap_for_valid_iterator_field_iterated_next_method_private_name: private(b"wrapForValidIteratorFieldIteratedNextMethod"),
            m_disposable_stack_field_state: public(b"disposableStackFieldState"),
            m_disposable_stack_field_state_private_name: private(b"disposableStackFieldState"),
            m_disposable_stack_field_capability: public(b"disposableStackFieldCapability"),
            m_disposable_stack_field_capability_private_name: private(b"disposableStackFieldCapability"),
            m_disposable_stack_state_pending: public(b"DisposableStackStatePending"),
            m_disposable_stack_state_pending_private_name: private(b"DisposableStackStatePending"),
            m_disposable_stack_state_disposed: public(b"DisposableStackStateDisposed"),
            m_disposable_stack_state_disposed_private_name: private(b"DisposableStackStateDisposed"),
            m_async_disposable_stack_field_state: public(b"asyncDisposableStackFieldState"),
            m_async_disposable_stack_field_state_private_name: private(b"asyncDisposableStackFieldState"),
            m_async_disposable_stack_field_capability: public(b"asyncDisposableStackFieldCapability"),
            m_async_disposable_stack_field_capability_private_name: private(b"asyncDisposableStackFieldCapability"),
            m_async_disposable_stack_state_pending: public(b"AsyncDisposableStackStatePending"),
            m_async_disposable_stack_state_pending_private_name: private(b"AsyncDisposableStackStatePending"),
            m_async_disposable_stack_state_disposed: public(b"AsyncDisposableStackStateDisposed"),
            m_async_disposable_stack_state_disposed_private_name: private(b"AsyncDisposableStackStateDisposed"),
            m_internal_microtask_async_from_sync_iterator_continue: public(b"InternalMicrotaskAsyncFromSyncIteratorContinue"),
            m_internal_microtask_async_from_sync_iterator_continue_private_name: private(b"InternalMicrotaskAsyncFromSyncIteratorContinue"),
            m_internal_microtask_async_from_sync_iterator_done: public(b"InternalMicrotaskAsyncFromSyncIteratorDone"),
            m_internal_microtask_async_from_sync_iterator_done_private_name: private(b"InternalMicrotaskAsyncFromSyncIteratorDone"),
            m_ordered_hash_table_sentinel: public(b"orderedHashTableSentinel"),
            m_ordered_hash_table_sentinel_private_name: private(b"orderedHashTableSentinel"),
            m_add: public(b"add"),
            m_add_private_name: private(b"add"),
            m_apply_function: public(b"applyFunction"),
            m_apply_function_private_name: private(b"applyFunction"),
            m_assert: public(b"assert"),
            m_assert_private_name: private(b"assert"),
            m_call_function: public(b"callFunction"),
            m_call_function_private_name: private(b"callFunction"),
            m_char_code_at: public(b"charCodeAt"),
            m_char_code_at_private_name: private(b"charCodeAt"),
            m_executor: public(b"executor"),
            m_executor_private_name: private(b"executor"),
            m_iterated_object: public(b"iteratedObject"),
            m_iterated_object_private_name: private(b"iteratedObject"),
            m_iterated_string: public(b"iteratedString"),
            m_iterated_string_private_name: private(b"iteratedString"),
            m_promise_dup: public(b"promise"),
            m_promise_dup_private_name: private(b"promise"),
            m_object: public(b"Object"),
            m_object_private_name: private(b"Object"),
            m_number: public(b"Number"),
            m_number_private_name: private(b"Number"),
            m_array: public(b"Array"),
            m_array_private_name: private(b"Array"),
            m_array_buffer: public(b"ArrayBuffer"),
            m_array_buffer_private_name: private(b"ArrayBuffer"),
            m_shadow_realm: public(b"ShadowRealm"),
            m_shadow_realm_private_name: private(b"ShadowRealm"),
            m_reg_exp: public(b"RegExp"),
            m_reg_exp_private_name: private(b"RegExp"),
            m_iterator: public(b"Iterator"),
            m_iterator_private_name: private(b"Iterator"),
            m_min: public(b"min"),
            m_min_private_name: private(b"min"),
            m_create: public(b"create"),
            m_create_private_name: private(b"create"),
            m_define_property: public(b"defineProperty"),
            m_define_property_private_name: private(b"defineProperty"),
            m_default_promise_then: public(b"defaultPromiseThen"),
            m_default_promise_then_private_name: private(b"defaultPromiseThen"),
            m_set: public(b"Set"),
            m_set_private_name: private(b"Set"),
            m_map_upper: public(b"Map"),
            m_map_upper_private_name: private(b"Map"),
            m_throw_type_error_function: public(b"throwTypeErrorFunction"),
            m_throw_type_error_function_private_name: private(b"throwTypeErrorFunction"),
            m_typed_array_length: public(b"typedArrayLength"),
            m_typed_array_length_private_name: private(b"typedArrayLength"),
            m_builtin_log: public(b"BuiltinLog"),
            m_builtin_log_private_name: private(b"BuiltinLog"),
            m_builtin_describe: public(b"BuiltinDescribe"),
            m_builtin_describe_private_name: private(b"BuiltinDescribe"),
            m_home_object: public(b"homeObject"),
            m_home_object_private_name: private(b"homeObject"),
            m_resolve_promise: public(b"resolvePromise"),
            m_resolve_promise_private_name: private(b"resolvePromise"),
            m_reject_promise: public(b"rejectPromise"),
            m_reject_promise_private_name: private(b"rejectPromise"),
            m_fulfill_promise: public(b"fulfillPromise"),
            m_fulfill_promise_private_name: private(b"fulfillPromise"),
            m_mark_promise_as_handled: public(b"markPromiseAsHandled"),
            m_mark_promise_as_handled_private_name: private(b"markPromiseAsHandled"),
            m_is_promise_state_pending: public(b"isPromiseStatePending"),
            m_is_promise_state_pending_private_name: private(b"isPromiseStatePending"),
            m_resolve_promise_with_first_resolving_function_call_check: public(b"resolvePromiseWithFirstResolvingFunctionCallCheck"),
            m_resolve_promise_with_first_resolving_function_call_check_private_name: private(b"resolvePromiseWithFirstResolvingFunctionCallCheck"),
            m_reject_promise_with_first_resolving_function_call_check: public(b"rejectPromiseWithFirstResolvingFunctionCallCheck"),
            m_reject_promise_with_first_resolving_function_call_check_private_name: private(b"rejectPromiseWithFirstResolvingFunctionCallCheck"),
            m_fulfill_promise_with_first_resolving_function_call_check: public(b"fulfillPromiseWithFirstResolvingFunctionCallCheck"),
            m_fulfill_promise_with_first_resolving_function_call_check_private_name: private(b"fulfillPromiseWithFirstResolvingFunctionCallCheck"),
            m_new_resolved_promise: public(b"newResolvedPromise"),
            m_new_resolved_promise_private_name: private(b"newResolvedPromise"),
            m_new_rejected_promise: public(b"newRejectedPromise"),
            m_new_rejected_promise_private_name: private(b"newRejectedPromise"),
            m_resolve_with_internal_microtask_for_async_await: public(b"resolveWithInternalMicrotaskForAsyncAwait"),
            m_resolve_with_internal_microtask_for_async_await_private_name: private(b"resolveWithInternalMicrotaskForAsyncAwait"),
            m_async_generator_prototype_next: public(b"asyncGeneratorPrototypeNext"),
            m_async_generator_prototype_next_private_name: private(b"asyncGeneratorPrototypeNext"),
            m_async_iterator_prototype_symbol_async_iterator: public(b"asyncIteratorPrototypeSymbolAsyncIterator"),
            m_async_iterator_prototype_symbol_async_iterator_private_name: private(b"asyncIteratorPrototypeSymbolAsyncIterator"),
            m_async_function_drive: public(b"asyncFunctionDrive"),
            m_async_function_drive_private_name: private(b"asyncFunctionDrive"),
            m_new_handled_rejected_promise: public(b"newHandledRejectedPromise"),
            m_new_handled_rejected_promise_private_name: private(b"newHandledRejectedPromise"),
            m_promise_return_undefined_on_fulfilled: public(b"promiseReturnUndefinedOnFulfilled"),
            m_promise_return_undefined_on_fulfilled_private_name: private(b"promiseReturnUndefinedOnFulfilled"),
            m_promise_resolve: public(b"promiseResolve"),
            m_promise_resolve_private_name: private(b"promiseResolve"),
            m_promise_reject: public(b"promiseReject"),
            m_promise_reject_private_name: private(b"promiseReject"),
            m_promise_resolve_with_then: public(b"promiseResolveWithThen"),
            m_promise_resolve_with_then_private_name: private(b"promiseResolveWithThen"),
            m_perform_promise_then: public(b"performPromiseThen"),
            m_perform_promise_then_private_name: private(b"performPromiseThen"),
            m_resolve: public(b"resolve"),
            m_resolve_private_name: private(b"resolve"),
            m_reject: public(b"reject"),
            m_reject_private_name: private(b"reject"),
            m_push: public(b"push"),
            m_push_private_name: private(b"push"),
            m_repeat_character: public(b"repeatCharacter"),
            m_repeat_character_private_name: private(b"repeatCharacter"),
            m_star_default: public(b"starDefault"),
            m_star_default_private_name: private(b"starDefault"),
            m_star_namespace: public(b"starNamespace"),
            m_star_namespace_private_name: private(b"starNamespace"),
            m_then: public(b"then"),
            m_then_private_name: private(b"then"),
            m_keys: public(b"keys"),
            m_keys_private_name: private(b"keys"),
            m_values: public(b"values"),
            m_values_private_name: private(b"values"),
            m_set_dup: public(b"set"),
            m_set_dup_private_name: private(b"set"),
            m_clear: public(b"clear"),
            m_clear_private_name: private(b"clear"),
            m_defer: public(b"defer"),
            m_defer_private_name: private(b"defer"),
            m_delete: public(b"delete"),
            m_delete_private_name: private(b"delete"),
            m_size: public(b"size"),
            m_size_private_name: private(b"size"),
            m_shift: public(b"shift"),
            m_shift_private_name: private(b"shift"),
            m_static_initializer_block: public(b"staticInitializerBlock"),
            m_static_initializer_block_private_name: private(b"staticInitializerBlock"),
            m_int8_array: public(b"Int8Array"),
            m_int8_array_private_name: private(b"Int8Array"),
            m_int16_array: public(b"Int16Array"),
            m_int16_array_private_name: private(b"Int16Array"),
            m_int32_array: public(b"Int32Array"),
            m_int32_array_private_name: private(b"Int32Array"),
            m_uint8_array: public(b"Uint8Array"),
            m_uint8_array_private_name: private(b"Uint8Array"),
            m_uint8_clamped_array: public(b"Uint8ClampedArray"),
            m_uint8_clamped_array_private_name: private(b"Uint8ClampedArray"),
            m_uint16_array: public(b"Uint16Array"),
            m_uint16_array_private_name: private(b"Uint16Array"),
            m_uint32_array: public(b"Uint32Array"),
            m_uint32_array_private_name: private(b"Uint32Array"),
            m_float16_array: public(b"Float16Array"),
            m_float16_array_private_name: private(b"Float16Array"),
            m_float32_array: public(b"Float32Array"),
            m_float32_array_private_name: private(b"Float32Array"),
            m_float64_array: public(b"Float64Array"),
            m_float64_array_private_name: private(b"Float64Array"),
            m_big_int64_array: public(b"BigInt64Array"),
            m_big_int64_array_private_name: private(b"BigInt64Array"),
            m_big_uint64_array: public(b"BigUint64Array"),
            m_big_uint64_array_private_name: private(b"BigUint64Array"),
            m_exec: public(b"exec"),
            m_exec_private_name: private(b"exec"),
            m_generator: public(b"generator"),
            m_generator_private_name: private(b"generator"),
            m_generator_next: public(b"generatorNext"),
            m_generator_next_private_name: private(b"generatorNext"),
            m_generator_state: public(b"generatorState"),
            m_generator_state_private_name: private(b"generatorState"),
            m_generator_frame: public(b"generatorFrame"),
            m_generator_frame_private_name: private(b"generatorFrame"),
            m_generator_value: public(b"generatorValue"),
            m_generator_value_private_name: private(b"generatorValue"),
            m_generator_this: public(b"generatorThis"),
            m_generator_this_private_name: private(b"generatorThis"),
            m_generator_resume_mode: public(b"generatorResumeMode"),
            m_generator_resume_mode_private_name: private(b"generatorResumeMode"),
            m_this: public(b"this"),
            m_this_private_name: private(b"this"),
            m_to_integer_or_infinity: public(b"toIntegerOrInfinity"),
            m_to_integer_or_infinity_private_name: private(b"toIntegerOrInfinity"),
            m_to_length: public(b"toLength"),
            m_to_length_private_name: private(b"toLength"),
            m_import_in_realm: public(b"importInRealm"),
            m_import_in_realm_private_name: private(b"importInRealm"),
            m_eval_function: public(b"evalFunction"),
            m_eval_function_private_name: private(b"evalFunction"),
            m_eval_in_realm: public(b"evalInRealm"),
            m_eval_in_realm_private_name: private(b"evalInRealm"),
            m_move_function_to_realm: public(b"moveFunctionToRealm"),
            m_move_function_to_realm_private_name: private(b"moveFunctionToRealm"),
            m_new_target_local: public(b"newTargetLocal"),
            m_new_target_local_private_name: private(b"newTargetLocal"),
            m_derived_constructor: public(b"derivedConstructor"),
            m_derived_constructor_private_name: private(b"derivedConstructor"),
            m_is_typed_array_view: public(b"isTypedArrayView"),
            m_is_typed_array_view_private_name: private(b"isTypedArrayView"),
            m_is_shared_typed_array_view: public(b"isSharedTypedArrayView"),
            m_is_shared_typed_array_view_private_name: private(b"isSharedTypedArrayView"),
            m_is_resizable_or_growable_shared_typed_array_view: public(b"isResizableOrGrowableSharedTypedArrayView"),
            m_is_resizable_or_growable_shared_typed_array_view_private_name: private(b"isResizableOrGrowableSharedTypedArrayView"),
            m_is_detached: public(b"isDetached"),
            m_is_detached_private_name: private(b"isDetached"),
            m_is_typed_array_out_of_bounds: public(b"isTypedArrayOutOfBounds"),
            m_is_typed_array_out_of_bounds_private_name: private(b"isTypedArrayOutOfBounds"),
            m_typed_array_from_fast: public(b"typedArrayFromFast"),
            m_typed_array_from_fast_private_name: private(b"typedArrayFromFast"),
            m_instance_of: public(b"instanceOf"),
            m_instance_of_private_name: private(b"instanceOf"),
            m_is_array: public(b"isArray"),
            m_is_array_private_name: private(b"isArray"),
            m_same_value: public(b"sameValue"),
            m_same_value_private_name: private(b"sameValue"),
            m_reg_exp_create: public(b"regExpCreate"),
            m_reg_exp_create_private_name: private(b"regExpCreate"),
            m_is_reg_exp: public(b"isRegExp"),
            m_is_reg_exp_private_name: private(b"isRegExp"),
            m_is_finite: public(b"isFinite"),
            m_is_finite_private_name: private(b"isFinite"),
            m_make_type_error: public(b"makeTypeError"),
            m_make_type_error_private_name: private(b"makeTypeError"),
            m_aggregate_error: public(b"AggregateError"),
            m_aggregate_error_private_name: private(b"AggregateError"),
            m_map_storage: public(b"mapStorage"),
            m_map_storage_private_name: private(b"mapStorage"),
            m_map_iteration_next: public(b"mapIterationNext"),
            m_map_iteration_next_private_name: private(b"mapIterationNext"),
            m_map_iteration_entry: public(b"mapIterationEntry"),
            m_map_iteration_entry_private_name: private(b"mapIterationEntry"),
            m_map_iteration_entry_key: public(b"mapIterationEntryKey"),
            m_map_iteration_entry_key_private_name: private(b"mapIterationEntryKey"),
            m_map_iteration_entry_value: public(b"mapIterationEntryValue"),
            m_map_iteration_entry_value_private_name: private(b"mapIterationEntryValue"),
            m_set_storage: public(b"setStorage"),
            m_set_storage_private_name: private(b"setStorage"),
            m_set_iteration_next: public(b"setIterationNext"),
            m_set_iteration_next_private_name: private(b"setIterationNext"),
            m_set_iteration_entry: public(b"setIterationEntry"),
            m_set_iteration_entry_private_name: private(b"setIterationEntry"),
            m_set_iteration_entry_key: public(b"setIterationEntryKey"),
            m_set_iteration_entry_key_private_name: private(b"setIterationEntryKey"),
            m_set_prototype_direct: public(b"setPrototypeDirect"),
            m_set_prototype_direct_private_name: private(b"setPrototypeDirect"),
            m_set_prototype_direct_or_throw: public(b"setPrototypeDirectOrThrow"),
            m_set_prototype_direct_or_throw_private_name: private(b"setPrototypeDirectOrThrow"),
            m_reg_exp_builtin_exec: public(b"regExpBuiltinExec"),
            m_reg_exp_builtin_exec_private_name: private(b"regExpBuiltinExec"),
            m_reg_exp_proto_flags_getter: public(b"regExpProtoFlagsGetter"),
            m_reg_exp_proto_flags_getter_private_name: private(b"regExpProtoFlagsGetter"),
            m_reg_exp_proto_has_indices_getter: public(b"regExpProtoHasIndicesGetter"),
            m_reg_exp_proto_has_indices_getter_private_name: private(b"regExpProtoHasIndicesGetter"),
            m_reg_exp_proto_global_getter: public(b"regExpProtoGlobalGetter"),
            m_reg_exp_proto_global_getter_private_name: private(b"regExpProtoGlobalGetter"),
            m_reg_exp_proto_ignore_case_getter: public(b"regExpProtoIgnoreCaseGetter"),
            m_reg_exp_proto_ignore_case_getter_private_name: private(b"regExpProtoIgnoreCaseGetter"),
            m_reg_exp_proto_multiline_getter: public(b"regExpProtoMultilineGetter"),
            m_reg_exp_proto_multiline_getter_private_name: private(b"regExpProtoMultilineGetter"),
            m_reg_exp_proto_source_getter: public(b"regExpProtoSourceGetter"),
            m_reg_exp_proto_source_getter_private_name: private(b"regExpProtoSourceGetter"),
            m_reg_exp_proto_sticky_getter: public(b"regExpProtoStickyGetter"),
            m_reg_exp_proto_sticky_getter_private_name: private(b"regExpProtoStickyGetter"),
            m_reg_exp_proto_dot_all_getter: public(b"regExpProtoDotAllGetter"),
            m_reg_exp_proto_dot_all_getter_private_name: private(b"regExpProtoDotAllGetter"),
            m_reg_exp_proto_unicode_getter: public(b"regExpProtoUnicodeGetter"),
            m_reg_exp_proto_unicode_getter_private_name: private(b"regExpProtoUnicodeGetter"),
            m_reg_exp_proto_unicode_sets_getter: public(b"regExpProtoUnicodeSetsGetter"),
            m_reg_exp_proto_unicode_sets_getter_private_name: private(b"regExpProtoUnicodeSetsGetter"),
            m_reg_exp_prototype_symbol_match: public(b"regExpPrototypeSymbolMatch"),
            m_reg_exp_prototype_symbol_match_private_name: private(b"regExpPrototypeSymbolMatch"),
            m_reg_exp_prototype_symbol_match_all: public(b"regExpPrototypeSymbolMatchAll"),
            m_reg_exp_prototype_symbol_match_all_private_name: private(b"regExpPrototypeSymbolMatchAll"),
            m_reg_exp_prototype_symbol_replace: public(b"regExpPrototypeSymbolReplace"),
            m_reg_exp_prototype_symbol_replace_private_name: private(b"regExpPrototypeSymbolReplace"),
            m_reg_exp_search_fast: public(b"regExpSearchFast"),
            m_reg_exp_search_fast_private_name: private(b"regExpSearchFast"),
            m_string_includes_internal: public(b"stringIncludesInternal"),
            m_string_includes_internal_private_name: private(b"stringIncludesInternal"),
            m_string_index_of_internal: public(b"stringIndexOfInternal"),
            m_string_index_of_internal_private_name: private(b"stringIndexOfInternal"),
            m_string_substring: public(b"stringSubstring"),
            m_string_substring_private_name: private(b"stringSubstring"),
            m_handle_negative_proxy_has_trap_result: public(b"handleNegativeProxyHasTrapResult"),
            m_handle_negative_proxy_has_trap_result_private_name: private(b"handleNegativeProxyHasTrapResult"),
            m_handle_positive_proxy_set_trap_result: public(b"handlePositiveProxySetTrapResult"),
            m_handle_positive_proxy_set_trap_result_private_name: private(b"handlePositiveProxySetTrapResult"),
            m_handle_proxy_get_trap_result: public(b"handleProxyGetTrapResult"),
            m_handle_proxy_get_trap_result_private_name: private(b"handleProxyGetTrapResult"),
            m_import_module: public(b"importModule"),
            m_import_module_private_name: private(b"importModule"),
            m_module_fetch_failure_kind: public(b"moduleFetchFailureKind"),
            m_module_fetch_failure_kind_private_name: private(b"moduleFetchFailureKind"),
            m_module_failure_module_record: public(b"moduleFailureModuleRecord"),
            m_module_failure_module_record_private_name: private(b"moduleFailureModuleRecord"),
            m_module_failure_module_key: public(b"moduleFailureModuleKey"),
            m_module_failure_module_key_private_name: private(b"moduleFailureModuleKey"),
            m_module_failure_module_type: public(b"moduleFailureModuleType"),
            m_module_failure_module_type_private_name: private(b"moduleFailureModuleType"),
            m_module_failure_kind: public(b"moduleFailureKind"),
            m_module_failure_kind_private_name: private(b"moduleFailureKind"),
            m_copy_data_properties: public(b"copyDataProperties"),
            m_copy_data_properties_private_name: private(b"copyDataProperties"),
            m_clone_object: public(b"cloneObject"),
            m_clone_object_private_name: private(b"cloneObject"),
            m_meta: public(b"meta"),
            m_meta_private_name: private(b"meta"),
            m_instance_field_initializer: public(b"instanceFieldInitializer"),
            m_instance_field_initializer_private_name: private(b"instanceFieldInitializer"),
            m_private_brand: public(b"privateBrand"),
            m_private_brand_private_name: private(b"privateBrand"),
            m_private_class_brand: public(b"privateClassBrand"),
            m_private_class_brand_private_name: private(b"privateClassBrand"),
            m_has_own_property_function: public(b"hasOwnPropertyFunction"),
            m_has_own_property_function_private_name: private(b"hasOwnPropertyFunction"),
            m_create_private_symbol: public(b"createPrivateSymbol"),
            m_create_private_symbol_private_name: private(b"createPrivateSymbol"),
            m_entries: public(b"entries"),
            m_entries_private_name: private(b"entries"),
            m_empty_property_name_enumerator: public(b"emptyPropertyNameEnumerator"),
            m_empty_property_name_enumerator_private_name: private(b"emptyPropertyNameEnumerator"),
            m_sentinel_string: public(b"sentinelString"),
            m_sentinel_string_private_name: private(b"sentinelString"),
            m_create_remote_function: public(b"createRemoteFunction"),
            m_create_remote_function_private_name: private(b"createRemoteFunction"),
            m_is_remote_function: public(b"isRemoteFunction"),
            m_is_remote_function_private_name: private(b"isRemoteFunction"),
            m_array_from_fast_without_map_fn: public(b"arrayFromFastWithoutMapFn"),
            m_array_from_fast_without_map_fn_private_name: private(b"arrayFromFastWithoutMapFn"),
            m_json_parse: public(b"jsonParse"),
            m_json_parse_private_name: private(b"jsonParse"),
            m_json_stringify: public(b"jsonStringify"),
            m_json_stringify_private_name: private(b"jsonStringify"),
            m_string: public(b"String"),
            m_string_private_name: private(b"String"),
            m_substr: public(b"substr"),
            m_substr_private_name: private(b"substr"),
            m_ends_with: public(b"endsWith"),
            m_ends_with_private_name: private(b"endsWith"),
            m_get_own_property_descriptor: public(b"getOwnPropertyDescriptor"),
            m_get_own_property_descriptor_private_name: private(b"getOwnPropertyDescriptor"),
            m_get_own_property_names: public(b"getOwnPropertyNames"),
            m_get_own_property_names_private_name: private(b"getOwnPropertyNames"),
            m_get_own_property_symbols: public(b"getOwnPropertySymbols"),
            m_get_own_property_symbols_private_name: private(b"getOwnPropertySymbols"),
            m_has_own: public(b"hasOwn"),
            m_has_own_private_name: private(b"hasOwn"),
            m_index_of: public(b"indexOf"),
            m_index_of_private_name: private(b"indexOf"),
            m_pop: public(b"pop"),
            m_pop_private_name: private(b"pop"),
            m_async_context: public(b"asyncContext"),
            m_async_context_private_name: private(b"asyncContext"),
            m_wrap_for_valid_iterator_create: public(b"wrapForValidIteratorCreate"),
            m_wrap_for_valid_iterator_create_private_name: private(b"wrapForValidIteratorCreate"),
            m_async_from_sync_iterator_create: public(b"asyncFromSyncIteratorCreate"),
            m_async_from_sync_iterator_create_private_name: private(b"asyncFromSyncIteratorCreate"),
            m_reg_exp_string_iterator_create: public(b"regExpStringIteratorCreate"),
            m_reg_exp_string_iterator_create_private_name: private(b"regExpStringIteratorCreate"),
            m_iterator_helper_create: public(b"iteratorHelperCreate"),
            m_iterator_helper_create_private_name: private(b"iteratorHelperCreate"),
            m_own_keys: public(b"ownKeys"),
            m_own_keys_private_name: private(b"ownKeys"),
            m_includes: public(b"includes"),
            m_includes_private_name: private(b"includes"),
            m_reference_error: public(b"ReferenceError"),
            m_reference_error_private_name: private(b"ReferenceError"),
            m_suppressed_error: public(b"SuppressedError"),
            m_suppressed_error_private_name: private(b"SuppressedError"),
            m_disposable_stack: public(b"DisposableStack"),
            m_disposable_stack_private_name: private(b"DisposableStack"),
            m_async_disposable_stack: public(b"AsyncDisposableStack"),
            m_async_disposable_stack_private_name: private(b"AsyncDisposableStack"),
            m_enqueue_job: public(b"enqueueJob"),
            m_enqueue_job_private_name: private(b"enqueueJob"),
            m_has_instance_symbol: well_known(b"Symbol.hasInstance"),
            m_has_instance_symbol_private_identifier: public(b"hasInstance"),
            m_is_concat_spreadable_symbol: well_known(b"Symbol.isConcatSpreadable"),
            m_is_concat_spreadable_symbol_private_identifier: public(b"isConcatSpreadable"),
            m_async_iterator_symbol: well_known(b"Symbol.asyncIterator"),
            m_async_iterator_symbol_private_identifier: public(b"asyncIterator"),
            m_iterator_symbol: well_known(b"Symbol.iterator"),
            m_iterator_symbol_private_identifier: public(b"iterator"),
            m_match_symbol: well_known(b"Symbol.match"),
            m_match_symbol_private_identifier: public(b"match"),
            m_match_all_symbol: well_known(b"Symbol.matchAll"),
            m_match_all_symbol_private_identifier: public(b"matchAll"),
            m_replace_symbol: well_known(b"Symbol.replace"),
            m_replace_symbol_private_identifier: public(b"replace"),
            m_search_symbol: well_known(b"Symbol.search"),
            m_search_symbol_private_identifier: public(b"search"),
            m_species_symbol: well_known(b"Symbol.species"),
            m_species_symbol_private_identifier: public(b"species"),
            m_split_symbol: well_known(b"Symbol.split"),
            m_split_symbol_private_identifier: public(b"split"),
            m_to_primitive_symbol: well_known(b"Symbol.toPrimitive"),
            m_to_primitive_symbol_private_identifier: public(b"toPrimitive"),
            m_to_string_tag_symbol: well_known(b"Symbol.toStringTag"),
            m_to_string_tag_symbol_private_identifier: public(b"toStringTag"),
            m_unscopables_symbol: well_known(b"Symbol.unscopables"),
            m_unscopables_symbol_private_identifier: public(b"unscopables"),
            m_dispose_symbol: well_known(b"Symbol.dispose"),
            m_dispose_symbol_private_identifier: public(b"dispose"),
            m_async_dispose_symbol: well_known(b"Symbol.asyncDispose"),
            m_async_dispose_symbol_private_identifier: public(b"asyncDispose"),
            m_dollar_vm_name: public(b"$vm"),
            m_dollar_vm_private_name: private(b"$vm"),
            m_poly_proto_private_name: private(b"PolyProto"),
            m_stack_private_name: private(b"stack"),
            m_private_name_set: PrivateNameSet::new(),
            m_well_known_symbols_map: WellKnownSymbolMap::new(),
        };

        // `m_privateNameSet.reserveInitialCapacity(1024)`.
        this.m_private_name_set.reserve(1024);

        let private_names: Vec<Identifier> = vec![
            this.m_promise_private_name.clone(),
            this.m_add_disposable_resource_private_name.clone(),
            this.m_adopt_private_name.clone(),
            this.m_apply_private_name.clone(),
            this.m_array_iterator_next_helper_private_name.clone(),
            this.m_async_dispose_private_name.clone(),
            this.m_at_private_name.clone(),
            this.m_builtin_map_iterable_private_name.clone(),
            this.m_builtin_set_iterable_private_name.clone(),
            this.m_call_private_name.clone(),
            this.m_chunks_private_name.clone(),
            this.m_close_all_iterators_private_name.clone(),
            this.m_concat_private_name.clone(),
            this.m_create_array_without_prototype_private_name.clone(),
            this.m_create_disposable_resource_private_name.clone(),
            this.m_create_inspector_injected_script_private_name.clone(),
            this.m_create_object_without_prototype_private_name.clone(),
            this.m_cross_realm_throw_private_name.clone(),
            this.m_default_async_from_async_array_like_private_name.clone(),
            this.m_default_async_from_async_iterator_private_name.clone(),
            this.m_defer_method_private_name.clone(),
            this.m_delete_property_private_name.clone(),
            this.m_dispose_private_name.clone(),
            this.m_dispose_async_private_name.clone(),
            this.m_drop_private_name.clone(),
            this.m_evaluate_private_name.clone(),
            this.m_every_private_name.clone(),
            this.m_filter_private_name.clone(),
            this.m_find_private_name.clone(),
            this.m_find_index_private_name.clone(),
            this.m_find_last_private_name.clone(),
            this.m_find_last_index_private_name.clone(),
            this.m_flat_into_array_private_name.clone(),
            this.m_flat_into_array_with_callback_private_name.clone(),
            this.m_flat_map_private_name.clone(),
            this.m_for_each_private_name.clone(),
            this.m_from_private_name.clone(),
            this.m_from_async_private_name.clone(),
            this.m_from_entries_private_name.clone(),
            this.m_generator_resume_private_name.clone(),
            this.m_get_private_name.clone(),
            this.m_get_async_dispose_method_private_name.clone(),
            this.m_get_dispose_method_private_name.clone(),
            this.m_get_iterator_flattenable_private_name.clone(),
            this.m_get_iterator_sync_private_name.clone(),
            this.m_get_options_object_private_name.clone(),
            this.m_group_by_private_name.clone(),
            this.m_has_private_name.clone(),
            this.m_import_value_private_name.clone(),
            this.m_iterator_close_all_normal_private_name.clone(),
            this.m_iterator_zip_private_name.clone(),
            this.m_map_private_name.clone(),
            this.m_move_private_name.clone(),
            this.m_next_private_name.clone(),
            this.m_perform_iteration_private_name.clone(),
            this.m_perform_proxy_object_get_private_name.clone(),
            this.m_perform_proxy_object_get_by_val_private_name.clone(),
            this.m_perform_proxy_object_has_private_name.clone(),
            this.m_perform_proxy_object_has_by_val_private_name.clone(),
            this.m_perform_proxy_object_set_by_val_sloppy_private_name.clone(),
            this.m_perform_proxy_object_set_by_val_strict_private_name.clone(),
            this.m_perform_proxy_object_set_sloppy_private_name.clone(),
            this.m_perform_proxy_object_set_strict_private_name.clone(),
            this.m_reduce_private_name.clone(),
            this.m_reduce_right_private_name.clone(),
            this.m_remove_first_from_list_private_name.clone(),
            this.m_return_private_name.clone(),
            this.m_some_private_name.clone(),
            this.m_take_private_name.clone(),
            this.m_throw_private_name.clone(),
            this.m_to_locale_string_private_name.clone(),
            this.m_try_private_name.clone(),
            this.m_use_private_name.clone(),
            this.m_windows_private_name.clone(),
            this.m_wrap_remote_value_private_name.clone(),
            this.m_wrapped_iterator_private_name.clone(),
            this.m_zip_private_name.clone(),
            this.m_zip_keyed_private_name.clone(),
            this.m_argument_private_name.clone(),
            this.m_argument_count_private_name.clone(),
            this.m_array_push_private_name.clone(),
            this.m_get_by_id_direct_private_name.clone(),
            this.m_get_by_id_direct_private_private_name.clone(),
            this.m_get_by_val_with_this_private_name.clone(),
            this.m_get_prototype_of_private_name.clone(),
            this.m_get_internal_field_private_name.clone(),
            this.m_get_generator_internal_field_private_name.clone(),
            this.m_get_iterator_helper_internal_field_private_name.clone(),
            this.m_get_async_disposable_stack_internal_field_private_name.clone(),
            this.m_get_array_iterator_internal_field_private_name.clone(),
            this.m_get_proxy_internal_field_private_name.clone(),
            this.m_get_wrap_for_valid_iterator_internal_field_private_name.clone(),
            this.m_get_disposable_stack_internal_field_private_name.clone(),
            this.m_id_with_profile_private_name.clone(),
            this.m_is_async_disposable_stack_private_name.clone(),
            this.m_is_object_private_name.clone(),
            this.m_is_callable_private_name.clone(),
            this.m_is_constructor_private_name.clone(),
            this.m_is_js_array_private_name.clone(),
            this.m_is_proxy_object_private_name.clone(),
            this.m_is_derived_array_private_name.clone(),
            this.m_is_generator_private_name.clone(),
            this.m_is_iterator_helper_private_name.clone(),
            this.m_is_promise_private_name.clone(),
            this.m_is_reg_exp_object_private_name.clone(),
            this.m_is_map_private_name.clone(),
            this.m_is_set_private_name.clone(),
            this.m_is_shadow_realm_private_name.clone(),
            this.m_is_array_iterator_private_name.clone(),
            this.m_is_undefined_or_null_private_name.clone(),
            this.m_is_wrap_for_valid_iterator_private_name.clone(),
            this.m_is_disposable_stack_private_name.clone(),
            this.m_throw_type_error_private_name.clone(),
            this.m_throw_range_error_private_name.clone(),
            this.m_throw_out_of_memory_error_private_name.clone(),
            this.m_put_by_id_direct_private_name.clone(),
            this.m_put_by_id_direct_private_private_name.clone(),
            this.m_put_by_val_direct_private_name.clone(),
            this.m_put_by_val_with_this_sloppy_private_name.clone(),
            this.m_put_by_val_with_this_strict_private_name.clone(),
            this.m_put_internal_field_private_name.clone(),
            this.m_put_generator_internal_field_private_name.clone(),
            this.m_put_async_disposable_stack_internal_field_private_name.clone(),
            this.m_put_array_iterator_internal_field_private_name.clone(),
            this.m_put_disposable_stack_internal_field_private_name.clone(),
            this.m_super_sampler_begin_private_name.clone(),
            this.m_super_sampler_end_private_name.clone(),
            this.m_to_number_private_name.clone(),
            this.m_to_string_private_name.clone(),
            this.m_to_property_key_private_name.clone(),
            this.m_to_object_private_name.clone(),
            this.m_to_this_private_name.clone(),
            this.m_must_validate_result_of_proxy_get_and_set_traps_private_name.clone(),
            this.m_must_validate_result_of_proxy_traps_except_get_and_set_private_name.clone(),
            this.m_new_array_with_size_private_name.clone(),
            this.m_new_array_with_species_private_name.clone(),
            this.m_new_promise_private_name.clone(),
            this.m_iterator_generic_close_private_name.clone(),
            this.m_iterator_generic_next_private_name.clone(),
            this.m_if_abrupt_close_iterator_private_name.clone(),
            this.m_create_promise_private_name.clone(),
            this.m_undefined_private_name.clone(),
            this.m_infinity_private_name.clone(),
            this.m_iteration_kind_key_private_name.clone(),
            this.m_iteration_kind_value_private_name.clone(),
            this.m_iteration_kind_entries_private_name.clone(),
            this.m_max_array_index_private_name.clone(),
            this.m_max_string_length_private_name.clone(),
            this.m_max_safe_integer_private_name.clone(),
            this.m_module_fetch_private_name.clone(),
            this.m_module_translate_private_name.clone(),
            this.m_module_instantiate_private_name.clone(),
            this.m_module_satisfy_private_name.clone(),
            this.m_module_link_private_name.clone(),
            this.m_module_ready_private_name.clone(),
            this.m_proxy_field_target_private_name.clone(),
            this.m_proxy_field_handler_private_name.clone(),
            this.m_generator_field_state_private_name.clone(),
            this.m_generator_field_next_private_name.clone(),
            this.m_generator_field_this_private_name.clone(),
            this.m_generator_field_frame_private_name.clone(),
            this.m_generator_resume_mode_normal_private_name.clone(),
            this.m_generator_resume_mode_throw_private_name.clone(),
            this.m_generator_resume_mode_return_private_name.clone(),
            this.m_generator_state_completed_private_name.clone(),
            this.m_generator_state_executing_private_name.clone(),
            this.m_generator_state_init_private_name.clone(),
            this.m_iterator_helper_field_generator_private_name.clone(),
            this.m_iterator_helper_field_underlying_iterator_private_name.clone(),
            this.m_array_iterator_field_index_private_name.clone(),
            this.m_array_iterator_field_iterated_object_private_name.clone(),
            this.m_array_iterator_field_kind_private_name.clone(),
            this.m_wrap_for_valid_iterator_field_iterated_iterator_private_name.clone(),
            this.m_wrap_for_valid_iterator_field_iterated_next_method_private_name.clone(),
            this.m_disposable_stack_field_state_private_name.clone(),
            this.m_disposable_stack_field_capability_private_name.clone(),
            this.m_disposable_stack_state_pending_private_name.clone(),
            this.m_disposable_stack_state_disposed_private_name.clone(),
            this.m_async_disposable_stack_field_state_private_name.clone(),
            this.m_async_disposable_stack_field_capability_private_name.clone(),
            this.m_async_disposable_stack_state_pending_private_name.clone(),
            this.m_async_disposable_stack_state_disposed_private_name.clone(),
            this.m_internal_microtask_async_from_sync_iterator_continue_private_name.clone(),
            this.m_internal_microtask_async_from_sync_iterator_done_private_name.clone(),
            this.m_ordered_hash_table_sentinel_private_name.clone(),
            this.m_add_private_name.clone(),
            this.m_apply_function_private_name.clone(),
            this.m_assert_private_name.clone(),
            this.m_call_function_private_name.clone(),
            this.m_char_code_at_private_name.clone(),
            this.m_executor_private_name.clone(),
            this.m_iterated_object_private_name.clone(),
            this.m_iterated_string_private_name.clone(),
            this.m_promise_dup_private_name.clone(),
            this.m_object_private_name.clone(),
            this.m_number_private_name.clone(),
            this.m_array_private_name.clone(),
            this.m_array_buffer_private_name.clone(),
            this.m_shadow_realm_private_name.clone(),
            this.m_reg_exp_private_name.clone(),
            this.m_iterator_private_name.clone(),
            this.m_min_private_name.clone(),
            this.m_create_private_name.clone(),
            this.m_define_property_private_name.clone(),
            this.m_default_promise_then_private_name.clone(),
            this.m_set_private_name.clone(),
            this.m_map_upper_private_name.clone(),
            this.m_throw_type_error_function_private_name.clone(),
            this.m_typed_array_length_private_name.clone(),
            this.m_builtin_log_private_name.clone(),
            this.m_builtin_describe_private_name.clone(),
            this.m_home_object_private_name.clone(),
            this.m_resolve_promise_private_name.clone(),
            this.m_reject_promise_private_name.clone(),
            this.m_fulfill_promise_private_name.clone(),
            this.m_mark_promise_as_handled_private_name.clone(),
            this.m_is_promise_state_pending_private_name.clone(),
            this.m_resolve_promise_with_first_resolving_function_call_check_private_name.clone(),
            this.m_reject_promise_with_first_resolving_function_call_check_private_name.clone(),
            this.m_fulfill_promise_with_first_resolving_function_call_check_private_name.clone(),
            this.m_new_resolved_promise_private_name.clone(),
            this.m_new_rejected_promise_private_name.clone(),
            this.m_resolve_with_internal_microtask_for_async_await_private_name.clone(),
            this.m_async_generator_prototype_next_private_name.clone(),
            this.m_async_iterator_prototype_symbol_async_iterator_private_name.clone(),
            this.m_async_function_drive_private_name.clone(),
            this.m_new_handled_rejected_promise_private_name.clone(),
            this.m_promise_return_undefined_on_fulfilled_private_name.clone(),
            this.m_promise_resolve_private_name.clone(),
            this.m_promise_reject_private_name.clone(),
            this.m_promise_resolve_with_then_private_name.clone(),
            this.m_perform_promise_then_private_name.clone(),
            this.m_resolve_private_name.clone(),
            this.m_reject_private_name.clone(),
            this.m_push_private_name.clone(),
            this.m_repeat_character_private_name.clone(),
            this.m_star_default_private_name.clone(),
            this.m_star_namespace_private_name.clone(),
            this.m_then_private_name.clone(),
            this.m_keys_private_name.clone(),
            this.m_values_private_name.clone(),
            this.m_set_dup_private_name.clone(),
            this.m_clear_private_name.clone(),
            this.m_defer_private_name.clone(),
            this.m_delete_private_name.clone(),
            this.m_size_private_name.clone(),
            this.m_shift_private_name.clone(),
            this.m_static_initializer_block_private_name.clone(),
            this.m_int8_array_private_name.clone(),
            this.m_int16_array_private_name.clone(),
            this.m_int32_array_private_name.clone(),
            this.m_uint8_array_private_name.clone(),
            this.m_uint8_clamped_array_private_name.clone(),
            this.m_uint16_array_private_name.clone(),
            this.m_uint32_array_private_name.clone(),
            this.m_float16_array_private_name.clone(),
            this.m_float32_array_private_name.clone(),
            this.m_float64_array_private_name.clone(),
            this.m_big_int64_array_private_name.clone(),
            this.m_big_uint64_array_private_name.clone(),
            this.m_exec_private_name.clone(),
            this.m_generator_private_name.clone(),
            this.m_generator_next_private_name.clone(),
            this.m_generator_state_private_name.clone(),
            this.m_generator_frame_private_name.clone(),
            this.m_generator_value_private_name.clone(),
            this.m_generator_this_private_name.clone(),
            this.m_generator_resume_mode_private_name.clone(),
            this.m_this_private_name.clone(),
            this.m_to_integer_or_infinity_private_name.clone(),
            this.m_to_length_private_name.clone(),
            this.m_import_in_realm_private_name.clone(),
            this.m_eval_function_private_name.clone(),
            this.m_eval_in_realm_private_name.clone(),
            this.m_move_function_to_realm_private_name.clone(),
            this.m_new_target_local_private_name.clone(),
            this.m_derived_constructor_private_name.clone(),
            this.m_is_typed_array_view_private_name.clone(),
            this.m_is_shared_typed_array_view_private_name.clone(),
            this.m_is_resizable_or_growable_shared_typed_array_view_private_name.clone(),
            this.m_is_detached_private_name.clone(),
            this.m_is_typed_array_out_of_bounds_private_name.clone(),
            this.m_typed_array_from_fast_private_name.clone(),
            this.m_instance_of_private_name.clone(),
            this.m_is_array_private_name.clone(),
            this.m_same_value_private_name.clone(),
            this.m_reg_exp_create_private_name.clone(),
            this.m_is_reg_exp_private_name.clone(),
            this.m_is_finite_private_name.clone(),
            this.m_make_type_error_private_name.clone(),
            this.m_aggregate_error_private_name.clone(),
            this.m_map_storage_private_name.clone(),
            this.m_map_iteration_next_private_name.clone(),
            this.m_map_iteration_entry_private_name.clone(),
            this.m_map_iteration_entry_key_private_name.clone(),
            this.m_map_iteration_entry_value_private_name.clone(),
            this.m_set_storage_private_name.clone(),
            this.m_set_iteration_next_private_name.clone(),
            this.m_set_iteration_entry_private_name.clone(),
            this.m_set_iteration_entry_key_private_name.clone(),
            this.m_set_prototype_direct_private_name.clone(),
            this.m_set_prototype_direct_or_throw_private_name.clone(),
            this.m_reg_exp_builtin_exec_private_name.clone(),
            this.m_reg_exp_proto_flags_getter_private_name.clone(),
            this.m_reg_exp_proto_has_indices_getter_private_name.clone(),
            this.m_reg_exp_proto_global_getter_private_name.clone(),
            this.m_reg_exp_proto_ignore_case_getter_private_name.clone(),
            this.m_reg_exp_proto_multiline_getter_private_name.clone(),
            this.m_reg_exp_proto_source_getter_private_name.clone(),
            this.m_reg_exp_proto_sticky_getter_private_name.clone(),
            this.m_reg_exp_proto_dot_all_getter_private_name.clone(),
            this.m_reg_exp_proto_unicode_getter_private_name.clone(),
            this.m_reg_exp_proto_unicode_sets_getter_private_name.clone(),
            this.m_reg_exp_prototype_symbol_match_private_name.clone(),
            this.m_reg_exp_prototype_symbol_match_all_private_name.clone(),
            this.m_reg_exp_prototype_symbol_replace_private_name.clone(),
            this.m_reg_exp_search_fast_private_name.clone(),
            this.m_string_includes_internal_private_name.clone(),
            this.m_string_index_of_internal_private_name.clone(),
            this.m_string_substring_private_name.clone(),
            this.m_handle_negative_proxy_has_trap_result_private_name.clone(),
            this.m_handle_positive_proxy_set_trap_result_private_name.clone(),
            this.m_handle_proxy_get_trap_result_private_name.clone(),
            this.m_import_module_private_name.clone(),
            this.m_module_fetch_failure_kind_private_name.clone(),
            this.m_module_failure_module_record_private_name.clone(),
            this.m_module_failure_module_key_private_name.clone(),
            this.m_module_failure_module_type_private_name.clone(),
            this.m_module_failure_kind_private_name.clone(),
            this.m_copy_data_properties_private_name.clone(),
            this.m_clone_object_private_name.clone(),
            this.m_meta_private_name.clone(),
            this.m_instance_field_initializer_private_name.clone(),
            this.m_private_brand_private_name.clone(),
            this.m_private_class_brand_private_name.clone(),
            this.m_has_own_property_function_private_name.clone(),
            this.m_create_private_symbol_private_name.clone(),
            this.m_entries_private_name.clone(),
            this.m_empty_property_name_enumerator_private_name.clone(),
            this.m_sentinel_string_private_name.clone(),
            this.m_create_remote_function_private_name.clone(),
            this.m_is_remote_function_private_name.clone(),
            this.m_array_from_fast_without_map_fn_private_name.clone(),
            this.m_json_parse_private_name.clone(),
            this.m_json_stringify_private_name.clone(),
            this.m_string_private_name.clone(),
            this.m_substr_private_name.clone(),
            this.m_ends_with_private_name.clone(),
            this.m_get_own_property_descriptor_private_name.clone(),
            this.m_get_own_property_names_private_name.clone(),
            this.m_get_own_property_symbols_private_name.clone(),
            this.m_has_own_private_name.clone(),
            this.m_index_of_private_name.clone(),
            this.m_pop_private_name.clone(),
            this.m_async_context_private_name.clone(),
            this.m_wrap_for_valid_iterator_create_private_name.clone(),
            this.m_async_from_sync_iterator_create_private_name.clone(),
            this.m_reg_exp_string_iterator_create_private_name.clone(),
            this.m_iterator_helper_create_private_name.clone(),
            this.m_own_keys_private_name.clone(),
            this.m_includes_private_name.clone(),
            this.m_reference_error_private_name.clone(),
            this.m_suppressed_error_private_name.clone(),
            this.m_disposable_stack_private_name.clone(),
            this.m_async_disposable_stack_private_name.clone(),
            this.m_enqueue_job_private_name.clone(),
        ];
        for private_name in &private_names {
            this.insert_private_name(private_name);
        }
        let well_known_symbols: Vec<(Identifier, Identifier)> = vec![
            (this.m_has_instance_symbol_private_identifier.clone(), this.m_has_instance_symbol.clone()),
            (this.m_is_concat_spreadable_symbol_private_identifier.clone(), this.m_is_concat_spreadable_symbol.clone()),
            (this.m_async_iterator_symbol_private_identifier.clone(), this.m_async_iterator_symbol.clone()),
            (this.m_iterator_symbol_private_identifier.clone(), this.m_iterator_symbol.clone()),
            (this.m_match_symbol_private_identifier.clone(), this.m_match_symbol.clone()),
            (this.m_match_all_symbol_private_identifier.clone(), this.m_match_all_symbol.clone()),
            (this.m_replace_symbol_private_identifier.clone(), this.m_replace_symbol.clone()),
            (this.m_search_symbol_private_identifier.clone(), this.m_search_symbol.clone()),
            (this.m_species_symbol_private_identifier.clone(), this.m_species_symbol.clone()),
            (this.m_split_symbol_private_identifier.clone(), this.m_split_symbol.clone()),
            (this.m_to_primitive_symbol_private_identifier.clone(), this.m_to_primitive_symbol.clone()),
            (this.m_to_string_tag_symbol_private_identifier.clone(), this.m_to_string_tag_symbol.clone()),
            (this.m_unscopables_symbol_private_identifier.clone(), this.m_unscopables_symbol.clone()),
            (this.m_dispose_symbol_private_identifier.clone(), this.m_dispose_symbol.clone()),
            (this.m_async_dispose_symbol_private_identifier.clone(), this.m_async_dispose_symbol.clone()),
        ];
        for (key, symbol) in &well_known_symbols {
            this.add_well_known_symbol(key, symbol);
        }
        let dollar_vm = this.m_dollar_vm_private_name.clone();
        this.insert_private_name(&dollar_vm);
        this
    }

    /// `PromisePublicName()`.
    pub fn promise_public_name(&self) -> &Identifier {
        &self.m_promise
    }

    /// `PromisePrivateName()`: `Identifier::fromUid(Symbols::PromisePrivateName)`.
    pub fn promise_private_name(&self) -> Identifier {
        self.m_promise_private_name.clone()
    }

    /// `addDisposableResourcePublicName()`.
    pub fn add_disposable_resource_public_name(&self) -> &Identifier {
        &self.m_add_disposable_resource
    }

    /// `addDisposableResourcePrivateName()`: `Identifier::fromUid(Symbols::addDisposableResourcePrivateName)`.
    pub fn add_disposable_resource_private_name(&self) -> Identifier {
        self.m_add_disposable_resource_private_name.clone()
    }

    /// `adoptPublicName()`.
    pub fn adopt_public_name(&self) -> &Identifier {
        &self.m_adopt
    }

    /// `adoptPrivateName()`: `Identifier::fromUid(Symbols::adoptPrivateName)`.
    pub fn adopt_private_name(&self) -> Identifier {
        self.m_adopt_private_name.clone()
    }

    /// `applyPublicName()`.
    pub fn apply_public_name(&self) -> &Identifier {
        &self.m_apply
    }

    /// `applyPrivateName()`: `Identifier::fromUid(Symbols::applyPrivateName)`.
    pub fn apply_private_name(&self) -> Identifier {
        self.m_apply_private_name.clone()
    }

    /// `arrayIteratorNextHelperPublicName()`.
    pub fn array_iterator_next_helper_public_name(&self) -> &Identifier {
        &self.m_array_iterator_next_helper
    }

    /// `arrayIteratorNextHelperPrivateName()`: `Identifier::fromUid(Symbols::arrayIteratorNextHelperPrivateName)`.
    pub fn array_iterator_next_helper_private_name(&self) -> Identifier {
        self.m_array_iterator_next_helper_private_name.clone()
    }

    /// `asyncDisposePublicName()`.
    pub fn async_dispose_public_name(&self) -> &Identifier {
        &self.m_async_dispose
    }

    /// `asyncDisposePrivateName()`: `Identifier::fromUid(Symbols::asyncDisposePrivateName)`.
    pub fn async_dispose_private_name(&self) -> Identifier {
        self.m_async_dispose_private_name.clone()
    }

    /// `atPublicName()`.
    pub fn at_public_name(&self) -> &Identifier {
        &self.m_at
    }

    /// `atPrivateName()`: `Identifier::fromUid(Symbols::atPrivateName)`.
    pub fn at_private_name(&self) -> Identifier {
        self.m_at_private_name.clone()
    }

    /// `builtinMapIterablePublicName()`.
    pub fn builtin_map_iterable_public_name(&self) -> &Identifier {
        &self.m_builtin_map_iterable
    }

    /// `builtinMapIterablePrivateName()`: `Identifier::fromUid(Symbols::builtinMapIterablePrivateName)`.
    pub fn builtin_map_iterable_private_name(&self) -> Identifier {
        self.m_builtin_map_iterable_private_name.clone()
    }

    /// `builtinSetIterablePublicName()`.
    pub fn builtin_set_iterable_public_name(&self) -> &Identifier {
        &self.m_builtin_set_iterable
    }

    /// `builtinSetIterablePrivateName()`: `Identifier::fromUid(Symbols::builtinSetIterablePrivateName)`.
    pub fn builtin_set_iterable_private_name(&self) -> Identifier {
        self.m_builtin_set_iterable_private_name.clone()
    }

    /// `callPublicName()`.
    pub fn call_public_name(&self) -> &Identifier {
        &self.m_call
    }

    /// `callPrivateName()`: `Identifier::fromUid(Symbols::callPrivateName)`.
    pub fn call_private_name(&self) -> Identifier {
        self.m_call_private_name.clone()
    }

    /// `chunksPublicName()`.
    pub fn chunks_public_name(&self) -> &Identifier {
        &self.m_chunks
    }

    /// `chunksPrivateName()`: `Identifier::fromUid(Symbols::chunksPrivateName)`.
    pub fn chunks_private_name(&self) -> Identifier {
        self.m_chunks_private_name.clone()
    }

    /// `closeAllIteratorsPublicName()`.
    pub fn close_all_iterators_public_name(&self) -> &Identifier {
        &self.m_close_all_iterators
    }

    /// `closeAllIteratorsPrivateName()`: `Identifier::fromUid(Symbols::closeAllIteratorsPrivateName)`.
    pub fn close_all_iterators_private_name(&self) -> Identifier {
        self.m_close_all_iterators_private_name.clone()
    }

    /// `concatPublicName()`.
    pub fn concat_public_name(&self) -> &Identifier {
        &self.m_concat
    }

    /// `concatPrivateName()`: `Identifier::fromUid(Symbols::concatPrivateName)`.
    pub fn concat_private_name(&self) -> Identifier {
        self.m_concat_private_name.clone()
    }

    /// `createArrayWithoutPrototypePublicName()`.
    pub fn create_array_without_prototype_public_name(&self) -> &Identifier {
        &self.m_create_array_without_prototype
    }

    /// `createArrayWithoutPrototypePrivateName()`: `Identifier::fromUid(Symbols::createArrayWithoutPrototypePrivateName)`.
    pub fn create_array_without_prototype_private_name(&self) -> Identifier {
        self.m_create_array_without_prototype_private_name.clone()
    }

    /// `createDisposableResourcePublicName()`.
    pub fn create_disposable_resource_public_name(&self) -> &Identifier {
        &self.m_create_disposable_resource
    }

    /// `createDisposableResourcePrivateName()`: `Identifier::fromUid(Symbols::createDisposableResourcePrivateName)`.
    pub fn create_disposable_resource_private_name(&self) -> Identifier {
        self.m_create_disposable_resource_private_name.clone()
    }

    /// `createInspectorInjectedScriptPublicName()`.
    pub fn create_inspector_injected_script_public_name(&self) -> &Identifier {
        &self.m_create_inspector_injected_script
    }

    /// `createInspectorInjectedScriptPrivateName()`: `Identifier::fromUid(Symbols::createInspectorInjectedScriptPrivateName)`.
    pub fn create_inspector_injected_script_private_name(&self) -> Identifier {
        self.m_create_inspector_injected_script_private_name.clone()
    }

    /// `createObjectWithoutPrototypePublicName()`.
    pub fn create_object_without_prototype_public_name(&self) -> &Identifier {
        &self.m_create_object_without_prototype
    }

    /// `createObjectWithoutPrototypePrivateName()`: `Identifier::fromUid(Symbols::createObjectWithoutPrototypePrivateName)`.
    pub fn create_object_without_prototype_private_name(&self) -> Identifier {
        self.m_create_object_without_prototype_private_name.clone()
    }

    /// `crossRealmThrowPublicName()`.
    pub fn cross_realm_throw_public_name(&self) -> &Identifier {
        &self.m_cross_realm_throw
    }

    /// `crossRealmThrowPrivateName()`: `Identifier::fromUid(Symbols::crossRealmThrowPrivateName)`.
    pub fn cross_realm_throw_private_name(&self) -> Identifier {
        self.m_cross_realm_throw_private_name.clone()
    }

    /// `defaultAsyncFromAsyncArrayLikePublicName()`.
    pub fn default_async_from_async_array_like_public_name(&self) -> &Identifier {
        &self.m_default_async_from_async_array_like
    }

    /// `defaultAsyncFromAsyncArrayLikePrivateName()`: `Identifier::fromUid(Symbols::defaultAsyncFromAsyncArrayLikePrivateName)`.
    pub fn default_async_from_async_array_like_private_name(&self) -> Identifier {
        self.m_default_async_from_async_array_like_private_name.clone()
    }

    /// `defaultAsyncFromAsyncIteratorPublicName()`.
    pub fn default_async_from_async_iterator_public_name(&self) -> &Identifier {
        &self.m_default_async_from_async_iterator
    }

    /// `defaultAsyncFromAsyncIteratorPrivateName()`: `Identifier::fromUid(Symbols::defaultAsyncFromAsyncIteratorPrivateName)`.
    pub fn default_async_from_async_iterator_private_name(&self) -> Identifier {
        self.m_default_async_from_async_iterator_private_name.clone()
    }

    /// `deferMethodPublicName()`.
    pub fn defer_method_public_name(&self) -> &Identifier {
        &self.m_defer_method
    }

    /// `deferMethodPrivateName()`: `Identifier::fromUid(Symbols::deferMethodPrivateName)`.
    pub fn defer_method_private_name(&self) -> Identifier {
        self.m_defer_method_private_name.clone()
    }

    /// `deletePropertyPublicName()`.
    pub fn delete_property_public_name(&self) -> &Identifier {
        &self.m_delete_property
    }

    /// `deletePropertyPrivateName()`: `Identifier::fromUid(Symbols::deletePropertyPrivateName)`.
    pub fn delete_property_private_name(&self) -> Identifier {
        self.m_delete_property_private_name.clone()
    }

    /// `disposePublicName()`.
    pub fn dispose_public_name(&self) -> &Identifier {
        &self.m_dispose
    }

    /// `disposePrivateName()`: `Identifier::fromUid(Symbols::disposePrivateName)`.
    pub fn dispose_private_name(&self) -> Identifier {
        self.m_dispose_private_name.clone()
    }

    /// `disposeAsyncPublicName()`.
    pub fn dispose_async_public_name(&self) -> &Identifier {
        &self.m_dispose_async
    }

    /// `disposeAsyncPrivateName()`: `Identifier::fromUid(Symbols::disposeAsyncPrivateName)`.
    pub fn dispose_async_private_name(&self) -> Identifier {
        self.m_dispose_async_private_name.clone()
    }

    /// `dropPublicName()`.
    pub fn drop_public_name(&self) -> &Identifier {
        &self.m_drop
    }

    /// `dropPrivateName()`: `Identifier::fromUid(Symbols::dropPrivateName)`.
    pub fn drop_private_name(&self) -> Identifier {
        self.m_drop_private_name.clone()
    }

    /// `evaluatePublicName()`.
    pub fn evaluate_public_name(&self) -> &Identifier {
        &self.m_evaluate
    }

    /// `evaluatePrivateName()`: `Identifier::fromUid(Symbols::evaluatePrivateName)`.
    pub fn evaluate_private_name(&self) -> Identifier {
        self.m_evaluate_private_name.clone()
    }

    /// `everyPublicName()`.
    pub fn every_public_name(&self) -> &Identifier {
        &self.m_every
    }

    /// `everyPrivateName()`: `Identifier::fromUid(Symbols::everyPrivateName)`.
    pub fn every_private_name(&self) -> Identifier {
        self.m_every_private_name.clone()
    }

    /// `filterPublicName()`.
    pub fn filter_public_name(&self) -> &Identifier {
        &self.m_filter
    }

    /// `filterPrivateName()`: `Identifier::fromUid(Symbols::filterPrivateName)`.
    pub fn filter_private_name(&self) -> Identifier {
        self.m_filter_private_name.clone()
    }

    /// `findPublicName()`.
    pub fn find_public_name(&self) -> &Identifier {
        &self.m_find
    }

    /// `findPrivateName()`: `Identifier::fromUid(Symbols::findPrivateName)`.
    pub fn find_private_name(&self) -> Identifier {
        self.m_find_private_name.clone()
    }

    /// `findIndexPublicName()`.
    pub fn find_index_public_name(&self) -> &Identifier {
        &self.m_find_index
    }

    /// `findIndexPrivateName()`: `Identifier::fromUid(Symbols::findIndexPrivateName)`.
    pub fn find_index_private_name(&self) -> Identifier {
        self.m_find_index_private_name.clone()
    }

    /// `findLastPublicName()`.
    pub fn find_last_public_name(&self) -> &Identifier {
        &self.m_find_last
    }

    /// `findLastPrivateName()`: `Identifier::fromUid(Symbols::findLastPrivateName)`.
    pub fn find_last_private_name(&self) -> Identifier {
        self.m_find_last_private_name.clone()
    }

    /// `findLastIndexPublicName()`.
    pub fn find_last_index_public_name(&self) -> &Identifier {
        &self.m_find_last_index
    }

    /// `findLastIndexPrivateName()`: `Identifier::fromUid(Symbols::findLastIndexPrivateName)`.
    pub fn find_last_index_private_name(&self) -> Identifier {
        self.m_find_last_index_private_name.clone()
    }

    /// `flatIntoArrayPublicName()`.
    pub fn flat_into_array_public_name(&self) -> &Identifier {
        &self.m_flat_into_array
    }

    /// `flatIntoArrayPrivateName()`: `Identifier::fromUid(Symbols::flatIntoArrayPrivateName)`.
    pub fn flat_into_array_private_name(&self) -> Identifier {
        self.m_flat_into_array_private_name.clone()
    }

    /// `flatIntoArrayWithCallbackPublicName()`.
    pub fn flat_into_array_with_callback_public_name(&self) -> &Identifier {
        &self.m_flat_into_array_with_callback
    }

    /// `flatIntoArrayWithCallbackPrivateName()`: `Identifier::fromUid(Symbols::flatIntoArrayWithCallbackPrivateName)`.
    pub fn flat_into_array_with_callback_private_name(&self) -> Identifier {
        self.m_flat_into_array_with_callback_private_name.clone()
    }

    /// `flatMapPublicName()`.
    pub fn flat_map_public_name(&self) -> &Identifier {
        &self.m_flat_map
    }

    /// `flatMapPrivateName()`: `Identifier::fromUid(Symbols::flatMapPrivateName)`.
    pub fn flat_map_private_name(&self) -> Identifier {
        self.m_flat_map_private_name.clone()
    }

    /// `forEachPublicName()`.
    pub fn for_each_public_name(&self) -> &Identifier {
        &self.m_for_each
    }

    /// `forEachPrivateName()`: `Identifier::fromUid(Symbols::forEachPrivateName)`.
    pub fn for_each_private_name(&self) -> Identifier {
        self.m_for_each_private_name.clone()
    }

    /// `fromPublicName()`.
    pub fn from_public_name(&self) -> &Identifier {
        &self.m_from
    }

    /// `fromPrivateName()`: `Identifier::fromUid(Symbols::fromPrivateName)`.
    pub fn from_private_name(&self) -> Identifier {
        self.m_from_private_name.clone()
    }

    /// `fromAsyncPublicName()`.
    pub fn from_async_public_name(&self) -> &Identifier {
        &self.m_from_async
    }

    /// `fromAsyncPrivateName()`: `Identifier::fromUid(Symbols::fromAsyncPrivateName)`.
    pub fn from_async_private_name(&self) -> Identifier {
        self.m_from_async_private_name.clone()
    }

    /// `fromEntriesPublicName()`.
    pub fn from_entries_public_name(&self) -> &Identifier {
        &self.m_from_entries
    }

    /// `fromEntriesPrivateName()`: `Identifier::fromUid(Symbols::fromEntriesPrivateName)`.
    pub fn from_entries_private_name(&self) -> Identifier {
        self.m_from_entries_private_name.clone()
    }

    /// `generatorResumePublicName()`.
    pub fn generator_resume_public_name(&self) -> &Identifier {
        &self.m_generator_resume
    }

    /// `generatorResumePrivateName()`: `Identifier::fromUid(Symbols::generatorResumePrivateName)`.
    pub fn generator_resume_private_name(&self) -> Identifier {
        self.m_generator_resume_private_name.clone()
    }

    /// `getPublicName()`.
    pub fn get_public_name(&self) -> &Identifier {
        &self.m_get
    }

    /// `getPrivateName()`: `Identifier::fromUid(Symbols::getPrivateName)`.
    pub fn get_private_name(&self) -> Identifier {
        self.m_get_private_name.clone()
    }

    /// `getAsyncDisposeMethodPublicName()`.
    pub fn get_async_dispose_method_public_name(&self) -> &Identifier {
        &self.m_get_async_dispose_method
    }

    /// `getAsyncDisposeMethodPrivateName()`: `Identifier::fromUid(Symbols::getAsyncDisposeMethodPrivateName)`.
    pub fn get_async_dispose_method_private_name(&self) -> Identifier {
        self.m_get_async_dispose_method_private_name.clone()
    }

    /// `getDisposeMethodPublicName()`.
    pub fn get_dispose_method_public_name(&self) -> &Identifier {
        &self.m_get_dispose_method
    }

    /// `getDisposeMethodPrivateName()`: `Identifier::fromUid(Symbols::getDisposeMethodPrivateName)`.
    pub fn get_dispose_method_private_name(&self) -> Identifier {
        self.m_get_dispose_method_private_name.clone()
    }

    /// `getIteratorFlattenablePublicName()`.
    pub fn get_iterator_flattenable_public_name(&self) -> &Identifier {
        &self.m_get_iterator_flattenable
    }

    /// `getIteratorFlattenablePrivateName()`: `Identifier::fromUid(Symbols::getIteratorFlattenablePrivateName)`.
    pub fn get_iterator_flattenable_private_name(&self) -> Identifier {
        self.m_get_iterator_flattenable_private_name.clone()
    }

    /// `getIteratorSyncPublicName()`.
    pub fn get_iterator_sync_public_name(&self) -> &Identifier {
        &self.m_get_iterator_sync
    }

    /// `getIteratorSyncPrivateName()`: `Identifier::fromUid(Symbols::getIteratorSyncPrivateName)`.
    pub fn get_iterator_sync_private_name(&self) -> Identifier {
        self.m_get_iterator_sync_private_name.clone()
    }

    /// `getOptionsObjectPublicName()`.
    pub fn get_options_object_public_name(&self) -> &Identifier {
        &self.m_get_options_object
    }

    /// `getOptionsObjectPrivateName()`: `Identifier::fromUid(Symbols::getOptionsObjectPrivateName)`.
    pub fn get_options_object_private_name(&self) -> Identifier {
        self.m_get_options_object_private_name.clone()
    }

    /// `groupByPublicName()`.
    pub fn group_by_public_name(&self) -> &Identifier {
        &self.m_group_by
    }

    /// `groupByPrivateName()`: `Identifier::fromUid(Symbols::groupByPrivateName)`.
    pub fn group_by_private_name(&self) -> Identifier {
        self.m_group_by_private_name.clone()
    }

    /// `hasPublicName()`.
    pub fn has_public_name(&self) -> &Identifier {
        &self.m_has
    }

    /// `hasPrivateName()`: `Identifier::fromUid(Symbols::hasPrivateName)`.
    pub fn has_private_name(&self) -> Identifier {
        self.m_has_private_name.clone()
    }

    /// `importValuePublicName()`.
    pub fn import_value_public_name(&self) -> &Identifier {
        &self.m_import_value
    }

    /// `importValuePrivateName()`: `Identifier::fromUid(Symbols::importValuePrivateName)`.
    pub fn import_value_private_name(&self) -> Identifier {
        self.m_import_value_private_name.clone()
    }

    /// `iteratorCloseAllNormalPublicName()`.
    pub fn iterator_close_all_normal_public_name(&self) -> &Identifier {
        &self.m_iterator_close_all_normal
    }

    /// `iteratorCloseAllNormalPrivateName()`: `Identifier::fromUid(Symbols::iteratorCloseAllNormalPrivateName)`.
    pub fn iterator_close_all_normal_private_name(&self) -> Identifier {
        self.m_iterator_close_all_normal_private_name.clone()
    }

    /// `iteratorZipPublicName()`.
    pub fn iterator_zip_public_name(&self) -> &Identifier {
        &self.m_iterator_zip
    }

    /// `iteratorZipPrivateName()`: `Identifier::fromUid(Symbols::iteratorZipPrivateName)`.
    pub fn iterator_zip_private_name(&self) -> Identifier {
        self.m_iterator_zip_private_name.clone()
    }

    /// `mapPublicName()`.
    pub fn map_public_name(&self) -> &Identifier {
        &self.m_map
    }

    /// `mapPrivateName()`: `Identifier::fromUid(Symbols::mapPrivateName)`.
    pub fn map_private_name(&self) -> Identifier {
        self.m_map_private_name.clone()
    }

    /// `movePublicName()`.
    pub fn move_public_name(&self) -> &Identifier {
        &self.m_move
    }

    /// `movePrivateName()`: `Identifier::fromUid(Symbols::movePrivateName)`.
    pub fn move_private_name(&self) -> Identifier {
        self.m_move_private_name.clone()
    }

    /// `nextPublicName()`.
    pub fn next_public_name(&self) -> &Identifier {
        &self.m_next
    }

    /// `nextPrivateName()`: `Identifier::fromUid(Symbols::nextPrivateName)`.
    pub fn next_private_name(&self) -> Identifier {
        self.m_next_private_name.clone()
    }

    /// `performIterationPublicName()`.
    pub fn perform_iteration_public_name(&self) -> &Identifier {
        &self.m_perform_iteration
    }

    /// `performIterationPrivateName()`: `Identifier::fromUid(Symbols::performIterationPrivateName)`.
    pub fn perform_iteration_private_name(&self) -> Identifier {
        self.m_perform_iteration_private_name.clone()
    }

    /// `performProxyObjectGetPublicName()`.
    pub fn perform_proxy_object_get_public_name(&self) -> &Identifier {
        &self.m_perform_proxy_object_get
    }

    /// `performProxyObjectGetPrivateName()`: `Identifier::fromUid(Symbols::performProxyObjectGetPrivateName)`.
    pub fn perform_proxy_object_get_private_name(&self) -> Identifier {
        self.m_perform_proxy_object_get_private_name.clone()
    }

    /// `performProxyObjectGetByValPublicName()`.
    pub fn perform_proxy_object_get_by_val_public_name(&self) -> &Identifier {
        &self.m_perform_proxy_object_get_by_val
    }

    /// `performProxyObjectGetByValPrivateName()`: `Identifier::fromUid(Symbols::performProxyObjectGetByValPrivateName)`.
    pub fn perform_proxy_object_get_by_val_private_name(&self) -> Identifier {
        self.m_perform_proxy_object_get_by_val_private_name.clone()
    }

    /// `performProxyObjectHasPublicName()`.
    pub fn perform_proxy_object_has_public_name(&self) -> &Identifier {
        &self.m_perform_proxy_object_has
    }

    /// `performProxyObjectHasPrivateName()`: `Identifier::fromUid(Symbols::performProxyObjectHasPrivateName)`.
    pub fn perform_proxy_object_has_private_name(&self) -> Identifier {
        self.m_perform_proxy_object_has_private_name.clone()
    }

    /// `performProxyObjectHasByValPublicName()`.
    pub fn perform_proxy_object_has_by_val_public_name(&self) -> &Identifier {
        &self.m_perform_proxy_object_has_by_val
    }

    /// `performProxyObjectHasByValPrivateName()`: `Identifier::fromUid(Symbols::performProxyObjectHasByValPrivateName)`.
    pub fn perform_proxy_object_has_by_val_private_name(&self) -> Identifier {
        self.m_perform_proxy_object_has_by_val_private_name.clone()
    }

    /// `performProxyObjectSetByValSloppyPublicName()`.
    pub fn perform_proxy_object_set_by_val_sloppy_public_name(&self) -> &Identifier {
        &self.m_perform_proxy_object_set_by_val_sloppy
    }

    /// `performProxyObjectSetByValSloppyPrivateName()`: `Identifier::fromUid(Symbols::performProxyObjectSetByValSloppyPrivateName)`.
    pub fn perform_proxy_object_set_by_val_sloppy_private_name(&self) -> Identifier {
        self.m_perform_proxy_object_set_by_val_sloppy_private_name.clone()
    }

    /// `performProxyObjectSetByValStrictPublicName()`.
    pub fn perform_proxy_object_set_by_val_strict_public_name(&self) -> &Identifier {
        &self.m_perform_proxy_object_set_by_val_strict
    }

    /// `performProxyObjectSetByValStrictPrivateName()`: `Identifier::fromUid(Symbols::performProxyObjectSetByValStrictPrivateName)`.
    pub fn perform_proxy_object_set_by_val_strict_private_name(&self) -> Identifier {
        self.m_perform_proxy_object_set_by_val_strict_private_name.clone()
    }

    /// `performProxyObjectSetSloppyPublicName()`.
    pub fn perform_proxy_object_set_sloppy_public_name(&self) -> &Identifier {
        &self.m_perform_proxy_object_set_sloppy
    }

    /// `performProxyObjectSetSloppyPrivateName()`: `Identifier::fromUid(Symbols::performProxyObjectSetSloppyPrivateName)`.
    pub fn perform_proxy_object_set_sloppy_private_name(&self) -> Identifier {
        self.m_perform_proxy_object_set_sloppy_private_name.clone()
    }

    /// `performProxyObjectSetStrictPublicName()`.
    pub fn perform_proxy_object_set_strict_public_name(&self) -> &Identifier {
        &self.m_perform_proxy_object_set_strict
    }

    /// `performProxyObjectSetStrictPrivateName()`: `Identifier::fromUid(Symbols::performProxyObjectSetStrictPrivateName)`.
    pub fn perform_proxy_object_set_strict_private_name(&self) -> Identifier {
        self.m_perform_proxy_object_set_strict_private_name.clone()
    }

    /// `reducePublicName()`.
    pub fn reduce_public_name(&self) -> &Identifier {
        &self.m_reduce
    }

    /// `reducePrivateName()`: `Identifier::fromUid(Symbols::reducePrivateName)`.
    pub fn reduce_private_name(&self) -> Identifier {
        self.m_reduce_private_name.clone()
    }

    /// `reduceRightPublicName()`.
    pub fn reduce_right_public_name(&self) -> &Identifier {
        &self.m_reduce_right
    }

    /// `reduceRightPrivateName()`: `Identifier::fromUid(Symbols::reduceRightPrivateName)`.
    pub fn reduce_right_private_name(&self) -> Identifier {
        self.m_reduce_right_private_name.clone()
    }

    /// `removeFirstFromListPublicName()`.
    pub fn remove_first_from_list_public_name(&self) -> &Identifier {
        &self.m_remove_first_from_list
    }

    /// `removeFirstFromListPrivateName()`: `Identifier::fromUid(Symbols::removeFirstFromListPrivateName)`.
    pub fn remove_first_from_list_private_name(&self) -> Identifier {
        self.m_remove_first_from_list_private_name.clone()
    }

    /// `returnPublicName()`.
    pub fn return_public_name(&self) -> &Identifier {
        &self.m_return
    }

    /// `returnPrivateName()`: `Identifier::fromUid(Symbols::returnPrivateName)`.
    pub fn return_private_name(&self) -> Identifier {
        self.m_return_private_name.clone()
    }

    /// `somePublicName()`.
    pub fn some_public_name(&self) -> &Identifier {
        &self.m_some
    }

    /// `somePrivateName()`: `Identifier::fromUid(Symbols::somePrivateName)`.
    pub fn some_private_name(&self) -> Identifier {
        self.m_some_private_name.clone()
    }

    /// `takePublicName()`.
    pub fn take_public_name(&self) -> &Identifier {
        &self.m_take
    }

    /// `takePrivateName()`: `Identifier::fromUid(Symbols::takePrivateName)`.
    pub fn take_private_name(&self) -> Identifier {
        self.m_take_private_name.clone()
    }

    /// `throwPublicName()`.
    pub fn throw_public_name(&self) -> &Identifier {
        &self.m_throw
    }

    /// `throwPrivateName()`: `Identifier::fromUid(Symbols::throwPrivateName)`.
    pub fn throw_private_name(&self) -> Identifier {
        self.m_throw_private_name.clone()
    }

    /// `toLocaleStringPublicName()`.
    pub fn to_locale_string_public_name(&self) -> &Identifier {
        &self.m_to_locale_string
    }

    /// `toLocaleStringPrivateName()`: `Identifier::fromUid(Symbols::toLocaleStringPrivateName)`.
    pub fn to_locale_string_private_name(&self) -> Identifier {
        self.m_to_locale_string_private_name.clone()
    }

    /// `tryPublicName()`.
    pub fn try_public_name(&self) -> &Identifier {
        &self.m_try
    }

    /// `tryPrivateName()`: `Identifier::fromUid(Symbols::tryPrivateName)`.
    pub fn try_private_name(&self) -> Identifier {
        self.m_try_private_name.clone()
    }

    /// `usePublicName()`.
    pub fn use_public_name(&self) -> &Identifier {
        &self.m_use
    }

    /// `usePrivateName()`: `Identifier::fromUid(Symbols::usePrivateName)`.
    pub fn use_private_name(&self) -> Identifier {
        self.m_use_private_name.clone()
    }

    /// `windowsPublicName()`.
    pub fn windows_public_name(&self) -> &Identifier {
        &self.m_windows
    }

    /// `windowsPrivateName()`: `Identifier::fromUid(Symbols::windowsPrivateName)`.
    pub fn windows_private_name(&self) -> Identifier {
        self.m_windows_private_name.clone()
    }

    /// `wrapRemoteValuePublicName()`.
    pub fn wrap_remote_value_public_name(&self) -> &Identifier {
        &self.m_wrap_remote_value
    }

    /// `wrapRemoteValuePrivateName()`: `Identifier::fromUid(Symbols::wrapRemoteValuePrivateName)`.
    pub fn wrap_remote_value_private_name(&self) -> Identifier {
        self.m_wrap_remote_value_private_name.clone()
    }

    /// `wrappedIteratorPublicName()`.
    pub fn wrapped_iterator_public_name(&self) -> &Identifier {
        &self.m_wrapped_iterator
    }

    /// `wrappedIteratorPrivateName()`: `Identifier::fromUid(Symbols::wrappedIteratorPrivateName)`.
    pub fn wrapped_iterator_private_name(&self) -> Identifier {
        self.m_wrapped_iterator_private_name.clone()
    }

    /// `zipPublicName()`.
    pub fn zip_public_name(&self) -> &Identifier {
        &self.m_zip
    }

    /// `zipPrivateName()`: `Identifier::fromUid(Symbols::zipPrivateName)`.
    pub fn zip_private_name(&self) -> Identifier {
        self.m_zip_private_name.clone()
    }

    /// `zipKeyedPublicName()`.
    pub fn zip_keyed_public_name(&self) -> &Identifier {
        &self.m_zip_keyed
    }

    /// `zipKeyedPrivateName()`: `Identifier::fromUid(Symbols::zipKeyedPrivateName)`.
    pub fn zip_keyed_private_name(&self) -> Identifier {
        self.m_zip_keyed_private_name.clone()
    }

    /// `argumentPublicName()`.
    pub fn argument_public_name(&self) -> &Identifier {
        &self.m_argument
    }

    /// `argumentPrivateName()`: `Identifier::fromUid(Symbols::argumentPrivateName)`.
    pub fn argument_private_name(&self) -> Identifier {
        self.m_argument_private_name.clone()
    }

    /// `argumentCountPublicName()`.
    pub fn argument_count_public_name(&self) -> &Identifier {
        &self.m_argument_count
    }

    /// `argumentCountPrivateName()`: `Identifier::fromUid(Symbols::argumentCountPrivateName)`.
    pub fn argument_count_private_name(&self) -> Identifier {
        self.m_argument_count_private_name.clone()
    }

    /// `arrayPushPublicName()`.
    pub fn array_push_public_name(&self) -> &Identifier {
        &self.m_array_push
    }

    /// `arrayPushPrivateName()`: `Identifier::fromUid(Symbols::arrayPushPrivateName)`.
    pub fn array_push_private_name(&self) -> Identifier {
        self.m_array_push_private_name.clone()
    }

    /// `getByIdDirectPublicName()`.
    pub fn get_by_id_direct_public_name(&self) -> &Identifier {
        &self.m_get_by_id_direct
    }

    /// `getByIdDirectPrivateName()`: `Identifier::fromUid(Symbols::getByIdDirectPrivateName)`.
    pub fn get_by_id_direct_private_name(&self) -> Identifier {
        self.m_get_by_id_direct_private_name.clone()
    }

    /// `getByIdDirectPrivatePublicName()`.
    pub fn get_by_id_direct_private_public_name(&self) -> &Identifier {
        &self.m_get_by_id_direct_private
    }

    /// `getByIdDirectPrivatePrivateName()`: `Identifier::fromUid(Symbols::getByIdDirectPrivatePrivateName)`.
    pub fn get_by_id_direct_private_private_name(&self) -> Identifier {
        self.m_get_by_id_direct_private_private_name.clone()
    }

    /// `getByValWithThisPublicName()`.
    pub fn get_by_val_with_this_public_name(&self) -> &Identifier {
        &self.m_get_by_val_with_this
    }

    /// `getByValWithThisPrivateName()`: `Identifier::fromUid(Symbols::getByValWithThisPrivateName)`.
    pub fn get_by_val_with_this_private_name(&self) -> Identifier {
        self.m_get_by_val_with_this_private_name.clone()
    }

    /// `getPrototypeOfPublicName()`.
    pub fn get_prototype_of_public_name(&self) -> &Identifier {
        &self.m_get_prototype_of
    }

    /// `getPrototypeOfPrivateName()`: `Identifier::fromUid(Symbols::getPrototypeOfPrivateName)`.
    pub fn get_prototype_of_private_name(&self) -> Identifier {
        self.m_get_prototype_of_private_name.clone()
    }

    /// `getInternalFieldPublicName()`.
    pub fn get_internal_field_public_name(&self) -> &Identifier {
        &self.m_get_internal_field
    }

    /// `getInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::getInternalFieldPrivateName)`.
    pub fn get_internal_field_private_name(&self) -> Identifier {
        self.m_get_internal_field_private_name.clone()
    }

    /// `getGeneratorInternalFieldPublicName()`.
    pub fn get_generator_internal_field_public_name(&self) -> &Identifier {
        &self.m_get_generator_internal_field
    }

    /// `getGeneratorInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::getGeneratorInternalFieldPrivateName)`.
    pub fn get_generator_internal_field_private_name(&self) -> Identifier {
        self.m_get_generator_internal_field_private_name.clone()
    }

    /// `getIteratorHelperInternalFieldPublicName()`.
    pub fn get_iterator_helper_internal_field_public_name(&self) -> &Identifier {
        &self.m_get_iterator_helper_internal_field
    }

    /// `getIteratorHelperInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::getIteratorHelperInternalFieldPrivateName)`.
    pub fn get_iterator_helper_internal_field_private_name(&self) -> Identifier {
        self.m_get_iterator_helper_internal_field_private_name.clone()
    }

    /// `getAsyncDisposableStackInternalFieldPublicName()`.
    pub fn get_async_disposable_stack_internal_field_public_name(&self) -> &Identifier {
        &self.m_get_async_disposable_stack_internal_field
    }

    /// `getAsyncDisposableStackInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::getAsyncDisposableStackInternalFieldPrivateName)`.
    pub fn get_async_disposable_stack_internal_field_private_name(&self) -> Identifier {
        self.m_get_async_disposable_stack_internal_field_private_name.clone()
    }

    /// `getArrayIteratorInternalFieldPublicName()`.
    pub fn get_array_iterator_internal_field_public_name(&self) -> &Identifier {
        &self.m_get_array_iterator_internal_field
    }

    /// `getArrayIteratorInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::getArrayIteratorInternalFieldPrivateName)`.
    pub fn get_array_iterator_internal_field_private_name(&self) -> Identifier {
        self.m_get_array_iterator_internal_field_private_name.clone()
    }

    /// `getProxyInternalFieldPublicName()`.
    pub fn get_proxy_internal_field_public_name(&self) -> &Identifier {
        &self.m_get_proxy_internal_field
    }

    /// `getProxyInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::getProxyInternalFieldPrivateName)`.
    pub fn get_proxy_internal_field_private_name(&self) -> Identifier {
        self.m_get_proxy_internal_field_private_name.clone()
    }

    /// `getWrapForValidIteratorInternalFieldPublicName()`.
    pub fn get_wrap_for_valid_iterator_internal_field_public_name(&self) -> &Identifier {
        &self.m_get_wrap_for_valid_iterator_internal_field
    }

    /// `getWrapForValidIteratorInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::getWrapForValidIteratorInternalFieldPrivateName)`.
    pub fn get_wrap_for_valid_iterator_internal_field_private_name(&self) -> Identifier {
        self.m_get_wrap_for_valid_iterator_internal_field_private_name.clone()
    }

    /// `getDisposableStackInternalFieldPublicName()`.
    pub fn get_disposable_stack_internal_field_public_name(&self) -> &Identifier {
        &self.m_get_disposable_stack_internal_field
    }

    /// `getDisposableStackInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::getDisposableStackInternalFieldPrivateName)`.
    pub fn get_disposable_stack_internal_field_private_name(&self) -> Identifier {
        self.m_get_disposable_stack_internal_field_private_name.clone()
    }

    /// `idWithProfilePublicName()`.
    pub fn id_with_profile_public_name(&self) -> &Identifier {
        &self.m_id_with_profile
    }

    /// `idWithProfilePrivateName()`: `Identifier::fromUid(Symbols::idWithProfilePrivateName)`.
    pub fn id_with_profile_private_name(&self) -> Identifier {
        self.m_id_with_profile_private_name.clone()
    }

    /// `isAsyncDisposableStackPublicName()`.
    pub fn is_async_disposable_stack_public_name(&self) -> &Identifier {
        &self.m_is_async_disposable_stack
    }

    /// `isAsyncDisposableStackPrivateName()`: `Identifier::fromUid(Symbols::isAsyncDisposableStackPrivateName)`.
    pub fn is_async_disposable_stack_private_name(&self) -> Identifier {
        self.m_is_async_disposable_stack_private_name.clone()
    }

    /// `isObjectPublicName()`.
    pub fn is_object_public_name(&self) -> &Identifier {
        &self.m_is_object
    }

    /// `isObjectPrivateName()`: `Identifier::fromUid(Symbols::isObjectPrivateName)`.
    pub fn is_object_private_name(&self) -> Identifier {
        self.m_is_object_private_name.clone()
    }

    /// `isCallablePublicName()`.
    pub fn is_callable_public_name(&self) -> &Identifier {
        &self.m_is_callable
    }

    /// `isCallablePrivateName()`: `Identifier::fromUid(Symbols::isCallablePrivateName)`.
    pub fn is_callable_private_name(&self) -> Identifier {
        self.m_is_callable_private_name.clone()
    }

    /// `isConstructorPublicName()`.
    pub fn is_constructor_public_name(&self) -> &Identifier {
        &self.m_is_constructor
    }

    /// `isConstructorPrivateName()`: `Identifier::fromUid(Symbols::isConstructorPrivateName)`.
    pub fn is_constructor_private_name(&self) -> Identifier {
        self.m_is_constructor_private_name.clone()
    }

    /// `isJSArrayPublicName()`.
    pub fn is_js_array_public_name(&self) -> &Identifier {
        &self.m_is_js_array
    }

    /// `isJSArrayPrivateName()`: `Identifier::fromUid(Symbols::isJSArrayPrivateName)`.
    pub fn is_js_array_private_name(&self) -> Identifier {
        self.m_is_js_array_private_name.clone()
    }

    /// `isProxyObjectPublicName()`.
    pub fn is_proxy_object_public_name(&self) -> &Identifier {
        &self.m_is_proxy_object
    }

    /// `isProxyObjectPrivateName()`: `Identifier::fromUid(Symbols::isProxyObjectPrivateName)`.
    pub fn is_proxy_object_private_name(&self) -> Identifier {
        self.m_is_proxy_object_private_name.clone()
    }

    /// `isDerivedArrayPublicName()`.
    pub fn is_derived_array_public_name(&self) -> &Identifier {
        &self.m_is_derived_array
    }

    /// `isDerivedArrayPrivateName()`: `Identifier::fromUid(Symbols::isDerivedArrayPrivateName)`.
    pub fn is_derived_array_private_name(&self) -> Identifier {
        self.m_is_derived_array_private_name.clone()
    }

    /// `isGeneratorPublicName()`.
    pub fn is_generator_public_name(&self) -> &Identifier {
        &self.m_is_generator
    }

    /// `isGeneratorPrivateName()`: `Identifier::fromUid(Symbols::isGeneratorPrivateName)`.
    pub fn is_generator_private_name(&self) -> Identifier {
        self.m_is_generator_private_name.clone()
    }

    /// `isIteratorHelperPublicName()`.
    pub fn is_iterator_helper_public_name(&self) -> &Identifier {
        &self.m_is_iterator_helper
    }

    /// `isIteratorHelperPrivateName()`: `Identifier::fromUid(Symbols::isIteratorHelperPrivateName)`.
    pub fn is_iterator_helper_private_name(&self) -> Identifier {
        self.m_is_iterator_helper_private_name.clone()
    }

    /// `isPromisePublicName()`.
    pub fn is_promise_public_name(&self) -> &Identifier {
        &self.m_is_promise
    }

    /// `isPromisePrivateName()`: `Identifier::fromUid(Symbols::isPromisePrivateName)`.
    pub fn is_promise_private_name(&self) -> Identifier {
        self.m_is_promise_private_name.clone()
    }

    /// `isRegExpObjectPublicName()`.
    pub fn is_reg_exp_object_public_name(&self) -> &Identifier {
        &self.m_is_reg_exp_object
    }

    /// `isRegExpObjectPrivateName()`: `Identifier::fromUid(Symbols::isRegExpObjectPrivateName)`.
    pub fn is_reg_exp_object_private_name(&self) -> Identifier {
        self.m_is_reg_exp_object_private_name.clone()
    }

    /// `isMapPublicName()`.
    pub fn is_map_public_name(&self) -> &Identifier {
        &self.m_is_map
    }

    /// `isMapPrivateName()`: `Identifier::fromUid(Symbols::isMapPrivateName)`.
    pub fn is_map_private_name(&self) -> Identifier {
        self.m_is_map_private_name.clone()
    }

    /// `isSetPublicName()`.
    pub fn is_set_public_name(&self) -> &Identifier {
        &self.m_is_set
    }

    /// `isSetPrivateName()`: `Identifier::fromUid(Symbols::isSetPrivateName)`.
    pub fn is_set_private_name(&self) -> Identifier {
        self.m_is_set_private_name.clone()
    }

    /// `isShadowRealmPublicName()`.
    pub fn is_shadow_realm_public_name(&self) -> &Identifier {
        &self.m_is_shadow_realm
    }

    /// `isShadowRealmPrivateName()`: `Identifier::fromUid(Symbols::isShadowRealmPrivateName)`.
    pub fn is_shadow_realm_private_name(&self) -> Identifier {
        self.m_is_shadow_realm_private_name.clone()
    }

    /// `isArrayIteratorPublicName()`.
    pub fn is_array_iterator_public_name(&self) -> &Identifier {
        &self.m_is_array_iterator
    }

    /// `isArrayIteratorPrivateName()`: `Identifier::fromUid(Symbols::isArrayIteratorPrivateName)`.
    pub fn is_array_iterator_private_name(&self) -> Identifier {
        self.m_is_array_iterator_private_name.clone()
    }

    /// `isUndefinedOrNullPublicName()`.
    pub fn is_undefined_or_null_public_name(&self) -> &Identifier {
        &self.m_is_undefined_or_null
    }

    /// `isUndefinedOrNullPrivateName()`: `Identifier::fromUid(Symbols::isUndefinedOrNullPrivateName)`.
    pub fn is_undefined_or_null_private_name(&self) -> Identifier {
        self.m_is_undefined_or_null_private_name.clone()
    }

    /// `isWrapForValidIteratorPublicName()`.
    pub fn is_wrap_for_valid_iterator_public_name(&self) -> &Identifier {
        &self.m_is_wrap_for_valid_iterator
    }

    /// `isWrapForValidIteratorPrivateName()`: `Identifier::fromUid(Symbols::isWrapForValidIteratorPrivateName)`.
    pub fn is_wrap_for_valid_iterator_private_name(&self) -> Identifier {
        self.m_is_wrap_for_valid_iterator_private_name.clone()
    }

    /// `isDisposableStackPublicName()`.
    pub fn is_disposable_stack_public_name(&self) -> &Identifier {
        &self.m_is_disposable_stack
    }

    /// `isDisposableStackPrivateName()`: `Identifier::fromUid(Symbols::isDisposableStackPrivateName)`.
    pub fn is_disposable_stack_private_name(&self) -> Identifier {
        self.m_is_disposable_stack_private_name.clone()
    }

    /// `throwTypeErrorPublicName()`.
    pub fn throw_type_error_public_name(&self) -> &Identifier {
        &self.m_throw_type_error
    }

    /// `throwTypeErrorPrivateName()`: `Identifier::fromUid(Symbols::throwTypeErrorPrivateName)`.
    pub fn throw_type_error_private_name(&self) -> Identifier {
        self.m_throw_type_error_private_name.clone()
    }

    /// `throwRangeErrorPublicName()`.
    pub fn throw_range_error_public_name(&self) -> &Identifier {
        &self.m_throw_range_error
    }

    /// `throwRangeErrorPrivateName()`: `Identifier::fromUid(Symbols::throwRangeErrorPrivateName)`.
    pub fn throw_range_error_private_name(&self) -> Identifier {
        self.m_throw_range_error_private_name.clone()
    }

    /// `throwOutOfMemoryErrorPublicName()`.
    pub fn throw_out_of_memory_error_public_name(&self) -> &Identifier {
        &self.m_throw_out_of_memory_error
    }

    /// `throwOutOfMemoryErrorPrivateName()`: `Identifier::fromUid(Symbols::throwOutOfMemoryErrorPrivateName)`.
    pub fn throw_out_of_memory_error_private_name(&self) -> Identifier {
        self.m_throw_out_of_memory_error_private_name.clone()
    }

    /// `putByIdDirectPublicName()`.
    pub fn put_by_id_direct_public_name(&self) -> &Identifier {
        &self.m_put_by_id_direct
    }

    /// `putByIdDirectPrivateName()`: `Identifier::fromUid(Symbols::putByIdDirectPrivateName)`.
    pub fn put_by_id_direct_private_name(&self) -> Identifier {
        self.m_put_by_id_direct_private_name.clone()
    }

    /// `putByIdDirectPrivatePublicName()`.
    pub fn put_by_id_direct_private_public_name(&self) -> &Identifier {
        &self.m_put_by_id_direct_private
    }

    /// `putByIdDirectPrivatePrivateName()`: `Identifier::fromUid(Symbols::putByIdDirectPrivatePrivateName)`.
    pub fn put_by_id_direct_private_private_name(&self) -> Identifier {
        self.m_put_by_id_direct_private_private_name.clone()
    }

    /// `putByValDirectPublicName()`.
    pub fn put_by_val_direct_public_name(&self) -> &Identifier {
        &self.m_put_by_val_direct
    }

    /// `putByValDirectPrivateName()`: `Identifier::fromUid(Symbols::putByValDirectPrivateName)`.
    pub fn put_by_val_direct_private_name(&self) -> Identifier {
        self.m_put_by_val_direct_private_name.clone()
    }

    /// `putByValWithThisSloppyPublicName()`.
    pub fn put_by_val_with_this_sloppy_public_name(&self) -> &Identifier {
        &self.m_put_by_val_with_this_sloppy
    }

    /// `putByValWithThisSloppyPrivateName()`: `Identifier::fromUid(Symbols::putByValWithThisSloppyPrivateName)`.
    pub fn put_by_val_with_this_sloppy_private_name(&self) -> Identifier {
        self.m_put_by_val_with_this_sloppy_private_name.clone()
    }

    /// `putByValWithThisStrictPublicName()`.
    pub fn put_by_val_with_this_strict_public_name(&self) -> &Identifier {
        &self.m_put_by_val_with_this_strict
    }

    /// `putByValWithThisStrictPrivateName()`: `Identifier::fromUid(Symbols::putByValWithThisStrictPrivateName)`.
    pub fn put_by_val_with_this_strict_private_name(&self) -> Identifier {
        self.m_put_by_val_with_this_strict_private_name.clone()
    }

    /// `putInternalFieldPublicName()`.
    pub fn put_internal_field_public_name(&self) -> &Identifier {
        &self.m_put_internal_field
    }

    /// `putInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::putInternalFieldPrivateName)`.
    pub fn put_internal_field_private_name(&self) -> Identifier {
        self.m_put_internal_field_private_name.clone()
    }

    /// `putGeneratorInternalFieldPublicName()`.
    pub fn put_generator_internal_field_public_name(&self) -> &Identifier {
        &self.m_put_generator_internal_field
    }

    /// `putGeneratorInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::putGeneratorInternalFieldPrivateName)`.
    pub fn put_generator_internal_field_private_name(&self) -> Identifier {
        self.m_put_generator_internal_field_private_name.clone()
    }

    /// `putAsyncDisposableStackInternalFieldPublicName()`.
    pub fn put_async_disposable_stack_internal_field_public_name(&self) -> &Identifier {
        &self.m_put_async_disposable_stack_internal_field
    }

    /// `putAsyncDisposableStackInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::putAsyncDisposableStackInternalFieldPrivateName)`.
    pub fn put_async_disposable_stack_internal_field_private_name(&self) -> Identifier {
        self.m_put_async_disposable_stack_internal_field_private_name.clone()
    }

    /// `putArrayIteratorInternalFieldPublicName()`.
    pub fn put_array_iterator_internal_field_public_name(&self) -> &Identifier {
        &self.m_put_array_iterator_internal_field
    }

    /// `putArrayIteratorInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::putArrayIteratorInternalFieldPrivateName)`.
    pub fn put_array_iterator_internal_field_private_name(&self) -> Identifier {
        self.m_put_array_iterator_internal_field_private_name.clone()
    }

    /// `putDisposableStackInternalFieldPublicName()`.
    pub fn put_disposable_stack_internal_field_public_name(&self) -> &Identifier {
        &self.m_put_disposable_stack_internal_field
    }

    /// `putDisposableStackInternalFieldPrivateName()`: `Identifier::fromUid(Symbols::putDisposableStackInternalFieldPrivateName)`.
    pub fn put_disposable_stack_internal_field_private_name(&self) -> Identifier {
        self.m_put_disposable_stack_internal_field_private_name.clone()
    }

    /// `superSamplerBeginPublicName()`.
    pub fn super_sampler_begin_public_name(&self) -> &Identifier {
        &self.m_super_sampler_begin
    }

    /// `superSamplerBeginPrivateName()`: `Identifier::fromUid(Symbols::superSamplerBeginPrivateName)`.
    pub fn super_sampler_begin_private_name(&self) -> Identifier {
        self.m_super_sampler_begin_private_name.clone()
    }

    /// `superSamplerEndPublicName()`.
    pub fn super_sampler_end_public_name(&self) -> &Identifier {
        &self.m_super_sampler_end
    }

    /// `superSamplerEndPrivateName()`: `Identifier::fromUid(Symbols::superSamplerEndPrivateName)`.
    pub fn super_sampler_end_private_name(&self) -> Identifier {
        self.m_super_sampler_end_private_name.clone()
    }

    /// `toNumberPublicName()`.
    pub fn to_number_public_name(&self) -> &Identifier {
        &self.m_to_number
    }

    /// `toNumberPrivateName()`: `Identifier::fromUid(Symbols::toNumberPrivateName)`.
    pub fn to_number_private_name(&self) -> Identifier {
        self.m_to_number_private_name.clone()
    }

    /// `toStringPublicName()`.
    pub fn to_string_public_name(&self) -> &Identifier {
        &self.m_to_string
    }

    /// `toStringPrivateName()`: `Identifier::fromUid(Symbols::toStringPrivateName)`.
    pub fn to_string_private_name(&self) -> Identifier {
        self.m_to_string_private_name.clone()
    }

    /// `toPropertyKeyPublicName()`.
    pub fn to_property_key_public_name(&self) -> &Identifier {
        &self.m_to_property_key
    }

    /// `toPropertyKeyPrivateName()`: `Identifier::fromUid(Symbols::toPropertyKeyPrivateName)`.
    pub fn to_property_key_private_name(&self) -> Identifier {
        self.m_to_property_key_private_name.clone()
    }

    /// `toObjectPublicName()`.
    pub fn to_object_public_name(&self) -> &Identifier {
        &self.m_to_object
    }

    /// `toObjectPrivateName()`: `Identifier::fromUid(Symbols::toObjectPrivateName)`.
    pub fn to_object_private_name(&self) -> Identifier {
        self.m_to_object_private_name.clone()
    }

    /// `toThisPublicName()`.
    pub fn to_this_public_name(&self) -> &Identifier {
        &self.m_to_this
    }

    /// `toThisPrivateName()`: `Identifier::fromUid(Symbols::toThisPrivateName)`.
    pub fn to_this_private_name(&self) -> Identifier {
        self.m_to_this_private_name.clone()
    }

    /// `mustValidateResultOfProxyGetAndSetTrapsPublicName()`.
    pub fn must_validate_result_of_proxy_get_and_set_traps_public_name(&self) -> &Identifier {
        &self.m_must_validate_result_of_proxy_get_and_set_traps
    }

    /// `mustValidateResultOfProxyGetAndSetTrapsPrivateName()`: `Identifier::fromUid(Symbols::mustValidateResultOfProxyGetAndSetTrapsPrivateName)`.
    pub fn must_validate_result_of_proxy_get_and_set_traps_private_name(&self) -> Identifier {
        self.m_must_validate_result_of_proxy_get_and_set_traps_private_name.clone()
    }

    /// `mustValidateResultOfProxyTrapsExceptGetAndSetPublicName()`.
    pub fn must_validate_result_of_proxy_traps_except_get_and_set_public_name(&self) -> &Identifier {
        &self.m_must_validate_result_of_proxy_traps_except_get_and_set
    }

    /// `mustValidateResultOfProxyTrapsExceptGetAndSetPrivateName()`: `Identifier::fromUid(Symbols::mustValidateResultOfProxyTrapsExceptGetAndSetPrivateName)`.
    pub fn must_validate_result_of_proxy_traps_except_get_and_set_private_name(&self) -> Identifier {
        self.m_must_validate_result_of_proxy_traps_except_get_and_set_private_name.clone()
    }

    /// `newArrayWithSizePublicName()`.
    pub fn new_array_with_size_public_name(&self) -> &Identifier {
        &self.m_new_array_with_size
    }

    /// `newArrayWithSizePrivateName()`: `Identifier::fromUid(Symbols::newArrayWithSizePrivateName)`.
    pub fn new_array_with_size_private_name(&self) -> Identifier {
        self.m_new_array_with_size_private_name.clone()
    }

    /// `newArrayWithSpeciesPublicName()`.
    pub fn new_array_with_species_public_name(&self) -> &Identifier {
        &self.m_new_array_with_species
    }

    /// `newArrayWithSpeciesPrivateName()`: `Identifier::fromUid(Symbols::newArrayWithSpeciesPrivateName)`.
    pub fn new_array_with_species_private_name(&self) -> Identifier {
        self.m_new_array_with_species_private_name.clone()
    }

    /// `newPromisePublicName()`.
    pub fn new_promise_public_name(&self) -> &Identifier {
        &self.m_new_promise
    }

    /// `newPromisePrivateName()`: `Identifier::fromUid(Symbols::newPromisePrivateName)`.
    pub fn new_promise_private_name(&self) -> Identifier {
        self.m_new_promise_private_name.clone()
    }

    /// `iteratorGenericClosePublicName()`.
    pub fn iterator_generic_close_public_name(&self) -> &Identifier {
        &self.m_iterator_generic_close
    }

    /// `iteratorGenericClosePrivateName()`: `Identifier::fromUid(Symbols::iteratorGenericClosePrivateName)`.
    pub fn iterator_generic_close_private_name(&self) -> Identifier {
        self.m_iterator_generic_close_private_name.clone()
    }

    /// `iteratorGenericNextPublicName()`.
    pub fn iterator_generic_next_public_name(&self) -> &Identifier {
        &self.m_iterator_generic_next
    }

    /// `iteratorGenericNextPrivateName()`: `Identifier::fromUid(Symbols::iteratorGenericNextPrivateName)`.
    pub fn iterator_generic_next_private_name(&self) -> Identifier {
        self.m_iterator_generic_next_private_name.clone()
    }

    /// `ifAbruptCloseIteratorPublicName()`.
    pub fn if_abrupt_close_iterator_public_name(&self) -> &Identifier {
        &self.m_if_abrupt_close_iterator
    }

    /// `ifAbruptCloseIteratorPrivateName()`: `Identifier::fromUid(Symbols::ifAbruptCloseIteratorPrivateName)`.
    pub fn if_abrupt_close_iterator_private_name(&self) -> Identifier {
        self.m_if_abrupt_close_iterator_private_name.clone()
    }

    /// `createPromisePublicName()`.
    pub fn create_promise_public_name(&self) -> &Identifier {
        &self.m_create_promise
    }

    /// `createPromisePrivateName()`: `Identifier::fromUid(Symbols::createPromisePrivateName)`.
    pub fn create_promise_private_name(&self) -> Identifier {
        self.m_create_promise_private_name.clone()
    }

    /// `undefinedPublicName()`.
    pub fn undefined_public_name(&self) -> &Identifier {
        &self.m_undefined
    }

    /// `undefinedPrivateName()`: `Identifier::fromUid(Symbols::undefinedPrivateName)`.
    pub fn undefined_private_name(&self) -> Identifier {
        self.m_undefined_private_name.clone()
    }

    /// `InfinityPublicName()`.
    pub fn infinity_public_name(&self) -> &Identifier {
        &self.m_infinity
    }

    /// `InfinityPrivateName()`: `Identifier::fromUid(Symbols::InfinityPrivateName)`.
    pub fn infinity_private_name(&self) -> Identifier {
        self.m_infinity_private_name.clone()
    }

    /// `iterationKindKeyPublicName()`.
    pub fn iteration_kind_key_public_name(&self) -> &Identifier {
        &self.m_iteration_kind_key
    }

    /// `iterationKindKeyPrivateName()`: `Identifier::fromUid(Symbols::iterationKindKeyPrivateName)`.
    pub fn iteration_kind_key_private_name(&self) -> Identifier {
        self.m_iteration_kind_key_private_name.clone()
    }

    /// `iterationKindValuePublicName()`.
    pub fn iteration_kind_value_public_name(&self) -> &Identifier {
        &self.m_iteration_kind_value
    }

    /// `iterationKindValuePrivateName()`: `Identifier::fromUid(Symbols::iterationKindValuePrivateName)`.
    pub fn iteration_kind_value_private_name(&self) -> Identifier {
        self.m_iteration_kind_value_private_name.clone()
    }

    /// `iterationKindEntriesPublicName()`.
    pub fn iteration_kind_entries_public_name(&self) -> &Identifier {
        &self.m_iteration_kind_entries
    }

    /// `iterationKindEntriesPrivateName()`: `Identifier::fromUid(Symbols::iterationKindEntriesPrivateName)`.
    pub fn iteration_kind_entries_private_name(&self) -> Identifier {
        self.m_iteration_kind_entries_private_name.clone()
    }

    /// `MAX_ARRAY_INDEXPublicName()`.
    pub fn max_array_index_public_name(&self) -> &Identifier {
        &self.m_max_array_index
    }

    /// `MAX_ARRAY_INDEXPrivateName()`: `Identifier::fromUid(Symbols::MAX_ARRAY_INDEXPrivateName)`.
    pub fn max_array_index_private_name(&self) -> Identifier {
        self.m_max_array_index_private_name.clone()
    }

    /// `MAX_STRING_LENGTHPublicName()`.
    pub fn max_string_length_public_name(&self) -> &Identifier {
        &self.m_max_string_length
    }

    /// `MAX_STRING_LENGTHPrivateName()`: `Identifier::fromUid(Symbols::MAX_STRING_LENGTHPrivateName)`.
    pub fn max_string_length_private_name(&self) -> Identifier {
        self.m_max_string_length_private_name.clone()
    }

    /// `MAX_SAFE_INTEGERPublicName()`.
    pub fn max_safe_integer_public_name(&self) -> &Identifier {
        &self.m_max_safe_integer
    }

    /// `MAX_SAFE_INTEGERPrivateName()`: `Identifier::fromUid(Symbols::MAX_SAFE_INTEGERPrivateName)`.
    pub fn max_safe_integer_private_name(&self) -> Identifier {
        self.m_max_safe_integer_private_name.clone()
    }

    /// `ModuleFetchPublicName()`.
    pub fn module_fetch_public_name(&self) -> &Identifier {
        &self.m_module_fetch
    }

    /// `ModuleFetchPrivateName()`: `Identifier::fromUid(Symbols::ModuleFetchPrivateName)`.
    pub fn module_fetch_private_name(&self) -> Identifier {
        self.m_module_fetch_private_name.clone()
    }

    /// `ModuleTranslatePublicName()`.
    pub fn module_translate_public_name(&self) -> &Identifier {
        &self.m_module_translate
    }

    /// `ModuleTranslatePrivateName()`: `Identifier::fromUid(Symbols::ModuleTranslatePrivateName)`.
    pub fn module_translate_private_name(&self) -> Identifier {
        self.m_module_translate_private_name.clone()
    }

    /// `ModuleInstantiatePublicName()`.
    pub fn module_instantiate_public_name(&self) -> &Identifier {
        &self.m_module_instantiate
    }

    /// `ModuleInstantiatePrivateName()`: `Identifier::fromUid(Symbols::ModuleInstantiatePrivateName)`.
    pub fn module_instantiate_private_name(&self) -> Identifier {
        self.m_module_instantiate_private_name.clone()
    }

    /// `ModuleSatisfyPublicName()`.
    pub fn module_satisfy_public_name(&self) -> &Identifier {
        &self.m_module_satisfy
    }

    /// `ModuleSatisfyPrivateName()`: `Identifier::fromUid(Symbols::ModuleSatisfyPrivateName)`.
    pub fn module_satisfy_private_name(&self) -> Identifier {
        self.m_module_satisfy_private_name.clone()
    }

    /// `ModuleLinkPublicName()`.
    pub fn module_link_public_name(&self) -> &Identifier {
        &self.m_module_link
    }

    /// `ModuleLinkPrivateName()`: `Identifier::fromUid(Symbols::ModuleLinkPrivateName)`.
    pub fn module_link_private_name(&self) -> Identifier {
        self.m_module_link_private_name.clone()
    }

    /// `ModuleReadyPublicName()`.
    pub fn module_ready_public_name(&self) -> &Identifier {
        &self.m_module_ready
    }

    /// `ModuleReadyPrivateName()`: `Identifier::fromUid(Symbols::ModuleReadyPrivateName)`.
    pub fn module_ready_private_name(&self) -> Identifier {
        self.m_module_ready_private_name.clone()
    }

    /// `proxyFieldTargetPublicName()`.
    pub fn proxy_field_target_public_name(&self) -> &Identifier {
        &self.m_proxy_field_target
    }

    /// `proxyFieldTargetPrivateName()`: `Identifier::fromUid(Symbols::proxyFieldTargetPrivateName)`.
    pub fn proxy_field_target_private_name(&self) -> Identifier {
        self.m_proxy_field_target_private_name.clone()
    }

    /// `proxyFieldHandlerPublicName()`.
    pub fn proxy_field_handler_public_name(&self) -> &Identifier {
        &self.m_proxy_field_handler
    }

    /// `proxyFieldHandlerPrivateName()`: `Identifier::fromUid(Symbols::proxyFieldHandlerPrivateName)`.
    pub fn proxy_field_handler_private_name(&self) -> Identifier {
        self.m_proxy_field_handler_private_name.clone()
    }

    /// `generatorFieldStatePublicName()`.
    pub fn generator_field_state_public_name(&self) -> &Identifier {
        &self.m_generator_field_state
    }

    /// `generatorFieldStatePrivateName()`: `Identifier::fromUid(Symbols::generatorFieldStatePrivateName)`.
    pub fn generator_field_state_private_name(&self) -> Identifier {
        self.m_generator_field_state_private_name.clone()
    }

    /// `generatorFieldNextPublicName()`.
    pub fn generator_field_next_public_name(&self) -> &Identifier {
        &self.m_generator_field_next
    }

    /// `generatorFieldNextPrivateName()`: `Identifier::fromUid(Symbols::generatorFieldNextPrivateName)`.
    pub fn generator_field_next_private_name(&self) -> Identifier {
        self.m_generator_field_next_private_name.clone()
    }

    /// `generatorFieldThisPublicName()`.
    pub fn generator_field_this_public_name(&self) -> &Identifier {
        &self.m_generator_field_this
    }

    /// `generatorFieldThisPrivateName()`: `Identifier::fromUid(Symbols::generatorFieldThisPrivateName)`.
    pub fn generator_field_this_private_name(&self) -> Identifier {
        self.m_generator_field_this_private_name.clone()
    }

    /// `generatorFieldFramePublicName()`.
    pub fn generator_field_frame_public_name(&self) -> &Identifier {
        &self.m_generator_field_frame
    }

    /// `generatorFieldFramePrivateName()`: `Identifier::fromUid(Symbols::generatorFieldFramePrivateName)`.
    pub fn generator_field_frame_private_name(&self) -> Identifier {
        self.m_generator_field_frame_private_name.clone()
    }

    /// `GeneratorResumeModeNormalPublicName()`.
    pub fn generator_resume_mode_normal_public_name(&self) -> &Identifier {
        &self.m_generator_resume_mode_normal
    }

    /// `GeneratorResumeModeNormalPrivateName()`: `Identifier::fromUid(Symbols::GeneratorResumeModeNormalPrivateName)`.
    pub fn generator_resume_mode_normal_private_name(&self) -> Identifier {
        self.m_generator_resume_mode_normal_private_name.clone()
    }

    /// `GeneratorResumeModeThrowPublicName()`.
    pub fn generator_resume_mode_throw_public_name(&self) -> &Identifier {
        &self.m_generator_resume_mode_throw
    }

    /// `GeneratorResumeModeThrowPrivateName()`: `Identifier::fromUid(Symbols::GeneratorResumeModeThrowPrivateName)`.
    pub fn generator_resume_mode_throw_private_name(&self) -> Identifier {
        self.m_generator_resume_mode_throw_private_name.clone()
    }

    /// `GeneratorResumeModeReturnPublicName()`.
    pub fn generator_resume_mode_return_public_name(&self) -> &Identifier {
        &self.m_generator_resume_mode_return
    }

    /// `GeneratorResumeModeReturnPrivateName()`: `Identifier::fromUid(Symbols::GeneratorResumeModeReturnPrivateName)`.
    pub fn generator_resume_mode_return_private_name(&self) -> Identifier {
        self.m_generator_resume_mode_return_private_name.clone()
    }

    /// `GeneratorStateCompletedPublicName()`.
    pub fn generator_state_completed_public_name(&self) -> &Identifier {
        &self.m_generator_state_completed
    }

    /// `GeneratorStateCompletedPrivateName()`: `Identifier::fromUid(Symbols::GeneratorStateCompletedPrivateName)`.
    pub fn generator_state_completed_private_name(&self) -> Identifier {
        self.m_generator_state_completed_private_name.clone()
    }

    /// `GeneratorStateExecutingPublicName()`.
    pub fn generator_state_executing_public_name(&self) -> &Identifier {
        &self.m_generator_state_executing
    }

    /// `GeneratorStateExecutingPrivateName()`: `Identifier::fromUid(Symbols::GeneratorStateExecutingPrivateName)`.
    pub fn generator_state_executing_private_name(&self) -> Identifier {
        self.m_generator_state_executing_private_name.clone()
    }

    /// `GeneratorStateInitPublicName()`.
    pub fn generator_state_init_public_name(&self) -> &Identifier {
        &self.m_generator_state_init
    }

    /// `GeneratorStateInitPrivateName()`: `Identifier::fromUid(Symbols::GeneratorStateInitPrivateName)`.
    pub fn generator_state_init_private_name(&self) -> Identifier {
        self.m_generator_state_init_private_name.clone()
    }

    /// `iteratorHelperFieldGeneratorPublicName()`.
    pub fn iterator_helper_field_generator_public_name(&self) -> &Identifier {
        &self.m_iterator_helper_field_generator
    }

    /// `iteratorHelperFieldGeneratorPrivateName()`: `Identifier::fromUid(Symbols::iteratorHelperFieldGeneratorPrivateName)`.
    pub fn iterator_helper_field_generator_private_name(&self) -> Identifier {
        self.m_iterator_helper_field_generator_private_name.clone()
    }

    /// `iteratorHelperFieldUnderlyingIteratorPublicName()`.
    pub fn iterator_helper_field_underlying_iterator_public_name(&self) -> &Identifier {
        &self.m_iterator_helper_field_underlying_iterator
    }

    /// `iteratorHelperFieldUnderlyingIteratorPrivateName()`: `Identifier::fromUid(Symbols::iteratorHelperFieldUnderlyingIteratorPrivateName)`.
    pub fn iterator_helper_field_underlying_iterator_private_name(&self) -> Identifier {
        self.m_iterator_helper_field_underlying_iterator_private_name.clone()
    }

    /// `arrayIteratorFieldIndexPublicName()`.
    pub fn array_iterator_field_index_public_name(&self) -> &Identifier {
        &self.m_array_iterator_field_index
    }

    /// `arrayIteratorFieldIndexPrivateName()`: `Identifier::fromUid(Symbols::arrayIteratorFieldIndexPrivateName)`.
    pub fn array_iterator_field_index_private_name(&self) -> Identifier {
        self.m_array_iterator_field_index_private_name.clone()
    }

    /// `arrayIteratorFieldIteratedObjectPublicName()`.
    pub fn array_iterator_field_iterated_object_public_name(&self) -> &Identifier {
        &self.m_array_iterator_field_iterated_object
    }

    /// `arrayIteratorFieldIteratedObjectPrivateName()`: `Identifier::fromUid(Symbols::arrayIteratorFieldIteratedObjectPrivateName)`.
    pub fn array_iterator_field_iterated_object_private_name(&self) -> Identifier {
        self.m_array_iterator_field_iterated_object_private_name.clone()
    }

    /// `arrayIteratorFieldKindPublicName()`.
    pub fn array_iterator_field_kind_public_name(&self) -> &Identifier {
        &self.m_array_iterator_field_kind
    }

    /// `arrayIteratorFieldKindPrivateName()`: `Identifier::fromUid(Symbols::arrayIteratorFieldKindPrivateName)`.
    pub fn array_iterator_field_kind_private_name(&self) -> Identifier {
        self.m_array_iterator_field_kind_private_name.clone()
    }

    /// `wrapForValidIteratorFieldIteratedIteratorPublicName()`.
    pub fn wrap_for_valid_iterator_field_iterated_iterator_public_name(&self) -> &Identifier {
        &self.m_wrap_for_valid_iterator_field_iterated_iterator
    }

    /// `wrapForValidIteratorFieldIteratedIteratorPrivateName()`: `Identifier::fromUid(Symbols::wrapForValidIteratorFieldIteratedIteratorPrivateName)`.
    pub fn wrap_for_valid_iterator_field_iterated_iterator_private_name(&self) -> Identifier {
        self.m_wrap_for_valid_iterator_field_iterated_iterator_private_name.clone()
    }

    /// `wrapForValidIteratorFieldIteratedNextMethodPublicName()`.
    pub fn wrap_for_valid_iterator_field_iterated_next_method_public_name(&self) -> &Identifier {
        &self.m_wrap_for_valid_iterator_field_iterated_next_method
    }

    /// `wrapForValidIteratorFieldIteratedNextMethodPrivateName()`: `Identifier::fromUid(Symbols::wrapForValidIteratorFieldIteratedNextMethodPrivateName)`.
    pub fn wrap_for_valid_iterator_field_iterated_next_method_private_name(&self) -> Identifier {
        self.m_wrap_for_valid_iterator_field_iterated_next_method_private_name.clone()
    }

    /// `disposableStackFieldStatePublicName()`.
    pub fn disposable_stack_field_state_public_name(&self) -> &Identifier {
        &self.m_disposable_stack_field_state
    }

    /// `disposableStackFieldStatePrivateName()`: `Identifier::fromUid(Symbols::disposableStackFieldStatePrivateName)`.
    pub fn disposable_stack_field_state_private_name(&self) -> Identifier {
        self.m_disposable_stack_field_state_private_name.clone()
    }

    /// `disposableStackFieldCapabilityPublicName()`.
    pub fn disposable_stack_field_capability_public_name(&self) -> &Identifier {
        &self.m_disposable_stack_field_capability
    }

    /// `disposableStackFieldCapabilityPrivateName()`: `Identifier::fromUid(Symbols::disposableStackFieldCapabilityPrivateName)`.
    pub fn disposable_stack_field_capability_private_name(&self) -> Identifier {
        self.m_disposable_stack_field_capability_private_name.clone()
    }

    /// `DisposableStackStatePendingPublicName()`.
    pub fn disposable_stack_state_pending_public_name(&self) -> &Identifier {
        &self.m_disposable_stack_state_pending
    }

    /// `DisposableStackStatePendingPrivateName()`: `Identifier::fromUid(Symbols::DisposableStackStatePendingPrivateName)`.
    pub fn disposable_stack_state_pending_private_name(&self) -> Identifier {
        self.m_disposable_stack_state_pending_private_name.clone()
    }

    /// `DisposableStackStateDisposedPublicName()`.
    pub fn disposable_stack_state_disposed_public_name(&self) -> &Identifier {
        &self.m_disposable_stack_state_disposed
    }

    /// `DisposableStackStateDisposedPrivateName()`: `Identifier::fromUid(Symbols::DisposableStackStateDisposedPrivateName)`.
    pub fn disposable_stack_state_disposed_private_name(&self) -> Identifier {
        self.m_disposable_stack_state_disposed_private_name.clone()
    }

    /// `asyncDisposableStackFieldStatePublicName()`.
    pub fn async_disposable_stack_field_state_public_name(&self) -> &Identifier {
        &self.m_async_disposable_stack_field_state
    }

    /// `asyncDisposableStackFieldStatePrivateName()`: `Identifier::fromUid(Symbols::asyncDisposableStackFieldStatePrivateName)`.
    pub fn async_disposable_stack_field_state_private_name(&self) -> Identifier {
        self.m_async_disposable_stack_field_state_private_name.clone()
    }

    /// `asyncDisposableStackFieldCapabilityPublicName()`.
    pub fn async_disposable_stack_field_capability_public_name(&self) -> &Identifier {
        &self.m_async_disposable_stack_field_capability
    }

    /// `asyncDisposableStackFieldCapabilityPrivateName()`: `Identifier::fromUid(Symbols::asyncDisposableStackFieldCapabilityPrivateName)`.
    pub fn async_disposable_stack_field_capability_private_name(&self) -> Identifier {
        self.m_async_disposable_stack_field_capability_private_name.clone()
    }

    /// `AsyncDisposableStackStatePendingPublicName()`.
    pub fn async_disposable_stack_state_pending_public_name(&self) -> &Identifier {
        &self.m_async_disposable_stack_state_pending
    }

    /// `AsyncDisposableStackStatePendingPrivateName()`: `Identifier::fromUid(Symbols::AsyncDisposableStackStatePendingPrivateName)`.
    pub fn async_disposable_stack_state_pending_private_name(&self) -> Identifier {
        self.m_async_disposable_stack_state_pending_private_name.clone()
    }

    /// `AsyncDisposableStackStateDisposedPublicName()`.
    pub fn async_disposable_stack_state_disposed_public_name(&self) -> &Identifier {
        &self.m_async_disposable_stack_state_disposed
    }

    /// `AsyncDisposableStackStateDisposedPrivateName()`: `Identifier::fromUid(Symbols::AsyncDisposableStackStateDisposedPrivateName)`.
    pub fn async_disposable_stack_state_disposed_private_name(&self) -> Identifier {
        self.m_async_disposable_stack_state_disposed_private_name.clone()
    }

    /// `InternalMicrotaskAsyncFromSyncIteratorContinuePublicName()`.
    pub fn internal_microtask_async_from_sync_iterator_continue_public_name(&self) -> &Identifier {
        &self.m_internal_microtask_async_from_sync_iterator_continue
    }

    /// `InternalMicrotaskAsyncFromSyncIteratorContinuePrivateName()`: `Identifier::fromUid(Symbols::InternalMicrotaskAsyncFromSyncIteratorContinuePrivateName)`.
    pub fn internal_microtask_async_from_sync_iterator_continue_private_name(&self) -> Identifier {
        self.m_internal_microtask_async_from_sync_iterator_continue_private_name.clone()
    }

    /// `InternalMicrotaskAsyncFromSyncIteratorDonePublicName()`.
    pub fn internal_microtask_async_from_sync_iterator_done_public_name(&self) -> &Identifier {
        &self.m_internal_microtask_async_from_sync_iterator_done
    }

    /// `InternalMicrotaskAsyncFromSyncIteratorDonePrivateName()`: `Identifier::fromUid(Symbols::InternalMicrotaskAsyncFromSyncIteratorDonePrivateName)`.
    pub fn internal_microtask_async_from_sync_iterator_done_private_name(&self) -> Identifier {
        self.m_internal_microtask_async_from_sync_iterator_done_private_name.clone()
    }

    /// `orderedHashTableSentinelPublicName()`.
    pub fn ordered_hash_table_sentinel_public_name(&self) -> &Identifier {
        &self.m_ordered_hash_table_sentinel
    }

    /// `orderedHashTableSentinelPrivateName()`: `Identifier::fromUid(Symbols::orderedHashTableSentinelPrivateName)`.
    pub fn ordered_hash_table_sentinel_private_name(&self) -> Identifier {
        self.m_ordered_hash_table_sentinel_private_name.clone()
    }

    /// `addPublicName()`.
    pub fn add_public_name(&self) -> &Identifier {
        &self.m_add
    }

    /// `addPrivateName()`: `Identifier::fromUid(Symbols::addPrivateName)`.
    pub fn add_private_name(&self) -> Identifier {
        self.m_add_private_name.clone()
    }

    /// `applyFunctionPublicName()`.
    pub fn apply_function_public_name(&self) -> &Identifier {
        &self.m_apply_function
    }

    /// `applyFunctionPrivateName()`: `Identifier::fromUid(Symbols::applyFunctionPrivateName)`.
    pub fn apply_function_private_name(&self) -> Identifier {
        self.m_apply_function_private_name.clone()
    }

    /// `assertPublicName()`.
    pub fn assert_public_name(&self) -> &Identifier {
        &self.m_assert
    }

    /// `assertPrivateName()`: `Identifier::fromUid(Symbols::assertPrivateName)`.
    pub fn assert_private_name(&self) -> Identifier {
        self.m_assert_private_name.clone()
    }

    /// `callFunctionPublicName()`.
    pub fn call_function_public_name(&self) -> &Identifier {
        &self.m_call_function
    }

    /// `callFunctionPrivateName()`: `Identifier::fromUid(Symbols::callFunctionPrivateName)`.
    pub fn call_function_private_name(&self) -> Identifier {
        self.m_call_function_private_name.clone()
    }

    /// `charCodeAtPublicName()`.
    pub fn char_code_at_public_name(&self) -> &Identifier {
        &self.m_char_code_at
    }

    /// `charCodeAtPrivateName()`: `Identifier::fromUid(Symbols::charCodeAtPrivateName)`.
    pub fn char_code_at_private_name(&self) -> Identifier {
        self.m_char_code_at_private_name.clone()
    }

    /// `executorPublicName()`.
    pub fn executor_public_name(&self) -> &Identifier {
        &self.m_executor
    }

    /// `executorPrivateName()`: `Identifier::fromUid(Symbols::executorPrivateName)`.
    pub fn executor_private_name(&self) -> Identifier {
        self.m_executor_private_name.clone()
    }

    /// `iteratedObjectPublicName()`.
    pub fn iterated_object_public_name(&self) -> &Identifier {
        &self.m_iterated_object
    }

    /// `iteratedObjectPrivateName()`: `Identifier::fromUid(Symbols::iteratedObjectPrivateName)`.
    pub fn iterated_object_private_name(&self) -> Identifier {
        self.m_iterated_object_private_name.clone()
    }

    /// `iteratedStringPublicName()`.
    pub fn iterated_string_public_name(&self) -> &Identifier {
        &self.m_iterated_string
    }

    /// `iteratedStringPrivateName()`: `Identifier::fromUid(Symbols::iteratedStringPrivateName)`.
    pub fn iterated_string_private_name(&self) -> Identifier {
        self.m_iterated_string_private_name.clone()
    }

    /// `promisePublicName()`.
    pub fn promise_dup_public_name(&self) -> &Identifier {
        &self.m_promise_dup
    }

    /// `promisePrivateName()`: `Identifier::fromUid(Symbols::promisePrivateName)`.
    pub fn promise_dup_private_name(&self) -> Identifier {
        self.m_promise_dup_private_name.clone()
    }

    /// `ObjectPublicName()`.
    pub fn object_public_name(&self) -> &Identifier {
        &self.m_object
    }

    /// `ObjectPrivateName()`: `Identifier::fromUid(Symbols::ObjectPrivateName)`.
    pub fn object_private_name(&self) -> Identifier {
        self.m_object_private_name.clone()
    }

    /// `NumberPublicName()`.
    pub fn number_public_name(&self) -> &Identifier {
        &self.m_number
    }

    /// `NumberPrivateName()`: `Identifier::fromUid(Symbols::NumberPrivateName)`.
    pub fn number_private_name(&self) -> Identifier {
        self.m_number_private_name.clone()
    }

    /// `ArrayPublicName()`.
    pub fn array_public_name(&self) -> &Identifier {
        &self.m_array
    }

    /// `ArrayPrivateName()`: `Identifier::fromUid(Symbols::ArrayPrivateName)`.
    pub fn array_private_name(&self) -> Identifier {
        self.m_array_private_name.clone()
    }

    /// `ArrayBufferPublicName()`.
    pub fn array_buffer_public_name(&self) -> &Identifier {
        &self.m_array_buffer
    }

    /// `ArrayBufferPrivateName()`: `Identifier::fromUid(Symbols::ArrayBufferPrivateName)`.
    pub fn array_buffer_private_name(&self) -> Identifier {
        self.m_array_buffer_private_name.clone()
    }

    /// `ShadowRealmPublicName()`.
    pub fn shadow_realm_public_name(&self) -> &Identifier {
        &self.m_shadow_realm
    }

    /// `ShadowRealmPrivateName()`: `Identifier::fromUid(Symbols::ShadowRealmPrivateName)`.
    pub fn shadow_realm_private_name(&self) -> Identifier {
        self.m_shadow_realm_private_name.clone()
    }

    /// `RegExpPublicName()`.
    pub fn reg_exp_public_name(&self) -> &Identifier {
        &self.m_reg_exp
    }

    /// `RegExpPrivateName()`: `Identifier::fromUid(Symbols::RegExpPrivateName)`.
    pub fn reg_exp_private_name(&self) -> Identifier {
        self.m_reg_exp_private_name.clone()
    }

    /// `IteratorPublicName()`.
    pub fn iterator_public_name(&self) -> &Identifier {
        &self.m_iterator
    }

    /// `IteratorPrivateName()`: `Identifier::fromUid(Symbols::IteratorPrivateName)`.
    pub fn iterator_private_name(&self) -> Identifier {
        self.m_iterator_private_name.clone()
    }

    /// `minPublicName()`.
    pub fn min_public_name(&self) -> &Identifier {
        &self.m_min
    }

    /// `minPrivateName()`: `Identifier::fromUid(Symbols::minPrivateName)`.
    pub fn min_private_name(&self) -> Identifier {
        self.m_min_private_name.clone()
    }

    /// `createPublicName()`.
    pub fn create_public_name(&self) -> &Identifier {
        &self.m_create
    }

    /// `createPrivateName()`: `Identifier::fromUid(Symbols::createPrivateName)`.
    pub fn create_private_name(&self) -> Identifier {
        self.m_create_private_name.clone()
    }

    /// `definePropertyPublicName()`.
    pub fn define_property_public_name(&self) -> &Identifier {
        &self.m_define_property
    }

    /// `definePropertyPrivateName()`: `Identifier::fromUid(Symbols::definePropertyPrivateName)`.
    pub fn define_property_private_name(&self) -> Identifier {
        self.m_define_property_private_name.clone()
    }

    /// `defaultPromiseThenPublicName()`.
    pub fn default_promise_then_public_name(&self) -> &Identifier {
        &self.m_default_promise_then
    }

    /// `defaultPromiseThenPrivateName()`: `Identifier::fromUid(Symbols::defaultPromiseThenPrivateName)`.
    pub fn default_promise_then_private_name(&self) -> Identifier {
        self.m_default_promise_then_private_name.clone()
    }

    /// `SetPublicName()`.
    pub fn set_public_name(&self) -> &Identifier {
        &self.m_set
    }

    /// `SetPrivateName()`: `Identifier::fromUid(Symbols::SetPrivateName)`.
    pub fn set_private_name(&self) -> Identifier {
        self.m_set_private_name.clone()
    }

    /// `MapPublicName()`.
    pub fn map_upper_public_name(&self) -> &Identifier {
        &self.m_map_upper
    }

    /// `MapPrivateName()`: `Identifier::fromUid(Symbols::MapPrivateName)`.
    pub fn map_upper_private_name(&self) -> Identifier {
        self.m_map_upper_private_name.clone()
    }

    /// `throwTypeErrorFunctionPublicName()`.
    pub fn throw_type_error_function_public_name(&self) -> &Identifier {
        &self.m_throw_type_error_function
    }

    /// `throwTypeErrorFunctionPrivateName()`: `Identifier::fromUid(Symbols::throwTypeErrorFunctionPrivateName)`.
    pub fn throw_type_error_function_private_name(&self) -> Identifier {
        self.m_throw_type_error_function_private_name.clone()
    }

    /// `typedArrayLengthPublicName()`.
    pub fn typed_array_length_public_name(&self) -> &Identifier {
        &self.m_typed_array_length
    }

    /// `typedArrayLengthPrivateName()`: `Identifier::fromUid(Symbols::typedArrayLengthPrivateName)`.
    pub fn typed_array_length_private_name(&self) -> Identifier {
        self.m_typed_array_length_private_name.clone()
    }

    /// `BuiltinLogPublicName()`.
    pub fn builtin_log_public_name(&self) -> &Identifier {
        &self.m_builtin_log
    }

    /// `BuiltinLogPrivateName()`: `Identifier::fromUid(Symbols::BuiltinLogPrivateName)`.
    pub fn builtin_log_private_name(&self) -> Identifier {
        self.m_builtin_log_private_name.clone()
    }

    /// `BuiltinDescribePublicName()`.
    pub fn builtin_describe_public_name(&self) -> &Identifier {
        &self.m_builtin_describe
    }

    /// `BuiltinDescribePrivateName()`: `Identifier::fromUid(Symbols::BuiltinDescribePrivateName)`.
    pub fn builtin_describe_private_name(&self) -> Identifier {
        self.m_builtin_describe_private_name.clone()
    }

    /// `homeObjectPublicName()`.
    pub fn home_object_public_name(&self) -> &Identifier {
        &self.m_home_object
    }

    /// `homeObjectPrivateName()`: `Identifier::fromUid(Symbols::homeObjectPrivateName)`.
    pub fn home_object_private_name(&self) -> Identifier {
        self.m_home_object_private_name.clone()
    }

    /// `resolvePromisePublicName()`.
    pub fn resolve_promise_public_name(&self) -> &Identifier {
        &self.m_resolve_promise
    }

    /// `resolvePromisePrivateName()`: `Identifier::fromUid(Symbols::resolvePromisePrivateName)`.
    pub fn resolve_promise_private_name(&self) -> Identifier {
        self.m_resolve_promise_private_name.clone()
    }

    /// `rejectPromisePublicName()`.
    pub fn reject_promise_public_name(&self) -> &Identifier {
        &self.m_reject_promise
    }

    /// `rejectPromisePrivateName()`: `Identifier::fromUid(Symbols::rejectPromisePrivateName)`.
    pub fn reject_promise_private_name(&self) -> Identifier {
        self.m_reject_promise_private_name.clone()
    }

    /// `fulfillPromisePublicName()`.
    pub fn fulfill_promise_public_name(&self) -> &Identifier {
        &self.m_fulfill_promise
    }

    /// `fulfillPromisePrivateName()`: `Identifier::fromUid(Symbols::fulfillPromisePrivateName)`.
    pub fn fulfill_promise_private_name(&self) -> Identifier {
        self.m_fulfill_promise_private_name.clone()
    }

    /// `markPromiseAsHandledPublicName()`.
    pub fn mark_promise_as_handled_public_name(&self) -> &Identifier {
        &self.m_mark_promise_as_handled
    }

    /// `markPromiseAsHandledPrivateName()`: `Identifier::fromUid(Symbols::markPromiseAsHandledPrivateName)`.
    pub fn mark_promise_as_handled_private_name(&self) -> Identifier {
        self.m_mark_promise_as_handled_private_name.clone()
    }

    /// `isPromiseStatePendingPublicName()`.
    pub fn is_promise_state_pending_public_name(&self) -> &Identifier {
        &self.m_is_promise_state_pending
    }

    /// `isPromiseStatePendingPrivateName()`: `Identifier::fromUid(Symbols::isPromiseStatePendingPrivateName)`.
    pub fn is_promise_state_pending_private_name(&self) -> Identifier {
        self.m_is_promise_state_pending_private_name.clone()
    }

    /// `resolvePromiseWithFirstResolvingFunctionCallCheckPublicName()`.
    pub fn resolve_promise_with_first_resolving_function_call_check_public_name(&self) -> &Identifier {
        &self.m_resolve_promise_with_first_resolving_function_call_check
    }

    /// `resolvePromiseWithFirstResolvingFunctionCallCheckPrivateName()`: `Identifier::fromUid(Symbols::resolvePromiseWithFirstResolvingFunctionCallCheckPrivateName)`.
    pub fn resolve_promise_with_first_resolving_function_call_check_private_name(&self) -> Identifier {
        self.m_resolve_promise_with_first_resolving_function_call_check_private_name.clone()
    }

    /// `rejectPromiseWithFirstResolvingFunctionCallCheckPublicName()`.
    pub fn reject_promise_with_first_resolving_function_call_check_public_name(&self) -> &Identifier {
        &self.m_reject_promise_with_first_resolving_function_call_check
    }

    /// `rejectPromiseWithFirstResolvingFunctionCallCheckPrivateName()`: `Identifier::fromUid(Symbols::rejectPromiseWithFirstResolvingFunctionCallCheckPrivateName)`.
    pub fn reject_promise_with_first_resolving_function_call_check_private_name(&self) -> Identifier {
        self.m_reject_promise_with_first_resolving_function_call_check_private_name.clone()
    }

    /// `fulfillPromiseWithFirstResolvingFunctionCallCheckPublicName()`.
    pub fn fulfill_promise_with_first_resolving_function_call_check_public_name(&self) -> &Identifier {
        &self.m_fulfill_promise_with_first_resolving_function_call_check
    }

    /// `fulfillPromiseWithFirstResolvingFunctionCallCheckPrivateName()`: `Identifier::fromUid(Symbols::fulfillPromiseWithFirstResolvingFunctionCallCheckPrivateName)`.
    pub fn fulfill_promise_with_first_resolving_function_call_check_private_name(&self) -> Identifier {
        self.m_fulfill_promise_with_first_resolving_function_call_check_private_name.clone()
    }

    /// `newResolvedPromisePublicName()`.
    pub fn new_resolved_promise_public_name(&self) -> &Identifier {
        &self.m_new_resolved_promise
    }

    /// `newResolvedPromisePrivateName()`: `Identifier::fromUid(Symbols::newResolvedPromisePrivateName)`.
    pub fn new_resolved_promise_private_name(&self) -> Identifier {
        self.m_new_resolved_promise_private_name.clone()
    }

    /// `newRejectedPromisePublicName()`.
    pub fn new_rejected_promise_public_name(&self) -> &Identifier {
        &self.m_new_rejected_promise
    }

    /// `newRejectedPromisePrivateName()`: `Identifier::fromUid(Symbols::newRejectedPromisePrivateName)`.
    pub fn new_rejected_promise_private_name(&self) -> Identifier {
        self.m_new_rejected_promise_private_name.clone()
    }

    /// `resolveWithInternalMicrotaskForAsyncAwaitPublicName()`.
    pub fn resolve_with_internal_microtask_for_async_await_public_name(&self) -> &Identifier {
        &self.m_resolve_with_internal_microtask_for_async_await
    }

    /// `resolveWithInternalMicrotaskForAsyncAwaitPrivateName()`: `Identifier::fromUid(Symbols::resolveWithInternalMicrotaskForAsyncAwaitPrivateName)`.
    pub fn resolve_with_internal_microtask_for_async_await_private_name(&self) -> Identifier {
        self.m_resolve_with_internal_microtask_for_async_await_private_name.clone()
    }

    /// `asyncGeneratorPrototypeNextPublicName()`.
    pub fn async_generator_prototype_next_public_name(&self) -> &Identifier {
        &self.m_async_generator_prototype_next
    }

    /// `asyncGeneratorPrototypeNextPrivateName()`: `Identifier::fromUid(Symbols::asyncGeneratorPrototypeNextPrivateName)`.
    pub fn async_generator_prototype_next_private_name(&self) -> Identifier {
        self.m_async_generator_prototype_next_private_name.clone()
    }

    /// `asyncIteratorPrototypeSymbolAsyncIteratorPublicName()`.
    pub fn async_iterator_prototype_symbol_async_iterator_public_name(&self) -> &Identifier {
        &self.m_async_iterator_prototype_symbol_async_iterator
    }

    /// `asyncIteratorPrototypeSymbolAsyncIteratorPrivateName()`: `Identifier::fromUid(Symbols::asyncIteratorPrototypeSymbolAsyncIteratorPrivateName)`.
    pub fn async_iterator_prototype_symbol_async_iterator_private_name(&self) -> Identifier {
        self.m_async_iterator_prototype_symbol_async_iterator_private_name.clone()
    }

    /// `asyncFunctionDrivePublicName()`.
    pub fn async_function_drive_public_name(&self) -> &Identifier {
        &self.m_async_function_drive
    }

    /// `asyncFunctionDrivePrivateName()`: `Identifier::fromUid(Symbols::asyncFunctionDrivePrivateName)`.
    pub fn async_function_drive_private_name(&self) -> Identifier {
        self.m_async_function_drive_private_name.clone()
    }

    /// `newHandledRejectedPromisePublicName()`.
    pub fn new_handled_rejected_promise_public_name(&self) -> &Identifier {
        &self.m_new_handled_rejected_promise
    }

    /// `newHandledRejectedPromisePrivateName()`: `Identifier::fromUid(Symbols::newHandledRejectedPromisePrivateName)`.
    pub fn new_handled_rejected_promise_private_name(&self) -> Identifier {
        self.m_new_handled_rejected_promise_private_name.clone()
    }

    /// `promiseReturnUndefinedOnFulfilledPublicName()`.
    pub fn promise_return_undefined_on_fulfilled_public_name(&self) -> &Identifier {
        &self.m_promise_return_undefined_on_fulfilled
    }

    /// `promiseReturnUndefinedOnFulfilledPrivateName()`: `Identifier::fromUid(Symbols::promiseReturnUndefinedOnFulfilledPrivateName)`.
    pub fn promise_return_undefined_on_fulfilled_private_name(&self) -> Identifier {
        self.m_promise_return_undefined_on_fulfilled_private_name.clone()
    }

    /// `promiseResolvePublicName()`.
    pub fn promise_resolve_public_name(&self) -> &Identifier {
        &self.m_promise_resolve
    }

    /// `promiseResolvePrivateName()`: `Identifier::fromUid(Symbols::promiseResolvePrivateName)`.
    pub fn promise_resolve_private_name(&self) -> Identifier {
        self.m_promise_resolve_private_name.clone()
    }

    /// `promiseRejectPublicName()`.
    pub fn promise_reject_public_name(&self) -> &Identifier {
        &self.m_promise_reject
    }

    /// `promiseRejectPrivateName()`: `Identifier::fromUid(Symbols::promiseRejectPrivateName)`.
    pub fn promise_reject_private_name(&self) -> Identifier {
        self.m_promise_reject_private_name.clone()
    }

    /// `promiseResolveWithThenPublicName()`.
    pub fn promise_resolve_with_then_public_name(&self) -> &Identifier {
        &self.m_promise_resolve_with_then
    }

    /// `promiseResolveWithThenPrivateName()`: `Identifier::fromUid(Symbols::promiseResolveWithThenPrivateName)`.
    pub fn promise_resolve_with_then_private_name(&self) -> Identifier {
        self.m_promise_resolve_with_then_private_name.clone()
    }

    /// `performPromiseThenPublicName()`.
    pub fn perform_promise_then_public_name(&self) -> &Identifier {
        &self.m_perform_promise_then
    }

    /// `performPromiseThenPrivateName()`: `Identifier::fromUid(Symbols::performPromiseThenPrivateName)`.
    pub fn perform_promise_then_private_name(&self) -> Identifier {
        self.m_perform_promise_then_private_name.clone()
    }

    /// `resolvePublicName()`.
    pub fn resolve_public_name(&self) -> &Identifier {
        &self.m_resolve
    }

    /// `resolvePrivateName()`: `Identifier::fromUid(Symbols::resolvePrivateName)`.
    pub fn resolve_private_name(&self) -> Identifier {
        self.m_resolve_private_name.clone()
    }

    /// `rejectPublicName()`.
    pub fn reject_public_name(&self) -> &Identifier {
        &self.m_reject
    }

    /// `rejectPrivateName()`: `Identifier::fromUid(Symbols::rejectPrivateName)`.
    pub fn reject_private_name(&self) -> Identifier {
        self.m_reject_private_name.clone()
    }

    /// `pushPublicName()`.
    pub fn push_public_name(&self) -> &Identifier {
        &self.m_push
    }

    /// `pushPrivateName()`: `Identifier::fromUid(Symbols::pushPrivateName)`.
    pub fn push_private_name(&self) -> Identifier {
        self.m_push_private_name.clone()
    }

    /// `repeatCharacterPublicName()`.
    pub fn repeat_character_public_name(&self) -> &Identifier {
        &self.m_repeat_character
    }

    /// `repeatCharacterPrivateName()`: `Identifier::fromUid(Symbols::repeatCharacterPrivateName)`.
    pub fn repeat_character_private_name(&self) -> Identifier {
        self.m_repeat_character_private_name.clone()
    }

    /// `starDefaultPublicName()`.
    pub fn star_default_public_name(&self) -> &Identifier {
        &self.m_star_default
    }

    /// `starDefaultPrivateName()`: `Identifier::fromUid(Symbols::starDefaultPrivateName)`.
    pub fn star_default_private_name(&self) -> Identifier {
        self.m_star_default_private_name.clone()
    }

    /// `starNamespacePublicName()`.
    pub fn star_namespace_public_name(&self) -> &Identifier {
        &self.m_star_namespace
    }

    /// `starNamespacePrivateName()`: `Identifier::fromUid(Symbols::starNamespacePrivateName)`.
    pub fn star_namespace_private_name(&self) -> Identifier {
        self.m_star_namespace_private_name.clone()
    }

    /// `thenPublicName()`.
    pub fn then_public_name(&self) -> &Identifier {
        &self.m_then
    }

    /// `thenPrivateName()`: `Identifier::fromUid(Symbols::thenPrivateName)`.
    pub fn then_private_name(&self) -> Identifier {
        self.m_then_private_name.clone()
    }

    /// `keysPublicName()`.
    pub fn keys_public_name(&self) -> &Identifier {
        &self.m_keys
    }

    /// `keysPrivateName()`: `Identifier::fromUid(Symbols::keysPrivateName)`.
    pub fn keys_private_name(&self) -> Identifier {
        self.m_keys_private_name.clone()
    }

    /// `valuesPublicName()`.
    pub fn values_public_name(&self) -> &Identifier {
        &self.m_values
    }

    /// `valuesPrivateName()`: `Identifier::fromUid(Symbols::valuesPrivateName)`.
    pub fn values_private_name(&self) -> Identifier {
        self.m_values_private_name.clone()
    }

    /// `setPublicName()`.
    pub fn set_dup_public_name(&self) -> &Identifier {
        &self.m_set_dup
    }

    /// `setPrivateName()`: `Identifier::fromUid(Symbols::setPrivateName)`.
    pub fn set_dup_private_name(&self) -> Identifier {
        self.m_set_dup_private_name.clone()
    }

    /// `clearPublicName()`.
    pub fn clear_public_name(&self) -> &Identifier {
        &self.m_clear
    }

    /// `clearPrivateName()`: `Identifier::fromUid(Symbols::clearPrivateName)`.
    pub fn clear_private_name(&self) -> Identifier {
        self.m_clear_private_name.clone()
    }

    /// `deferPublicName()`.
    pub fn defer_public_name(&self) -> &Identifier {
        &self.m_defer
    }

    /// `deferPrivateName()`: `Identifier::fromUid(Symbols::deferPrivateName)`.
    pub fn defer_private_name(&self) -> Identifier {
        self.m_defer_private_name.clone()
    }

    /// `deletePublicName()`.
    pub fn delete_public_name(&self) -> &Identifier {
        &self.m_delete
    }

    /// `deletePrivateName()`: `Identifier::fromUid(Symbols::deletePrivateName)`.
    pub fn delete_private_name(&self) -> Identifier {
        self.m_delete_private_name.clone()
    }

    /// `sizePublicName()`.
    pub fn size_public_name(&self) -> &Identifier {
        &self.m_size
    }

    /// `sizePrivateName()`: `Identifier::fromUid(Symbols::sizePrivateName)`.
    pub fn size_private_name(&self) -> Identifier {
        self.m_size_private_name.clone()
    }

    /// `shiftPublicName()`.
    pub fn shift_public_name(&self) -> &Identifier {
        &self.m_shift
    }

    /// `shiftPrivateName()`: `Identifier::fromUid(Symbols::shiftPrivateName)`.
    pub fn shift_private_name(&self) -> Identifier {
        self.m_shift_private_name.clone()
    }

    /// `staticInitializerBlockPublicName()`.
    pub fn static_initializer_block_public_name(&self) -> &Identifier {
        &self.m_static_initializer_block
    }

    /// `staticInitializerBlockPrivateName()`: `Identifier::fromUid(Symbols::staticInitializerBlockPrivateName)`.
    pub fn static_initializer_block_private_name(&self) -> Identifier {
        self.m_static_initializer_block_private_name.clone()
    }

    /// `Int8ArrayPublicName()`.
    pub fn int8_array_public_name(&self) -> &Identifier {
        &self.m_int8_array
    }

    /// `Int8ArrayPrivateName()`: `Identifier::fromUid(Symbols::Int8ArrayPrivateName)`.
    pub fn int8_array_private_name(&self) -> Identifier {
        self.m_int8_array_private_name.clone()
    }

    /// `Int16ArrayPublicName()`.
    pub fn int16_array_public_name(&self) -> &Identifier {
        &self.m_int16_array
    }

    /// `Int16ArrayPrivateName()`: `Identifier::fromUid(Symbols::Int16ArrayPrivateName)`.
    pub fn int16_array_private_name(&self) -> Identifier {
        self.m_int16_array_private_name.clone()
    }

    /// `Int32ArrayPublicName()`.
    pub fn int32_array_public_name(&self) -> &Identifier {
        &self.m_int32_array
    }

    /// `Int32ArrayPrivateName()`: `Identifier::fromUid(Symbols::Int32ArrayPrivateName)`.
    pub fn int32_array_private_name(&self) -> Identifier {
        self.m_int32_array_private_name.clone()
    }

    /// `Uint8ArrayPublicName()`.
    pub fn uint8_array_public_name(&self) -> &Identifier {
        &self.m_uint8_array
    }

    /// `Uint8ArrayPrivateName()`: `Identifier::fromUid(Symbols::Uint8ArrayPrivateName)`.
    pub fn uint8_array_private_name(&self) -> Identifier {
        self.m_uint8_array_private_name.clone()
    }

    /// `Uint8ClampedArrayPublicName()`.
    pub fn uint8_clamped_array_public_name(&self) -> &Identifier {
        &self.m_uint8_clamped_array
    }

    /// `Uint8ClampedArrayPrivateName()`: `Identifier::fromUid(Symbols::Uint8ClampedArrayPrivateName)`.
    pub fn uint8_clamped_array_private_name(&self) -> Identifier {
        self.m_uint8_clamped_array_private_name.clone()
    }

    /// `Uint16ArrayPublicName()`.
    pub fn uint16_array_public_name(&self) -> &Identifier {
        &self.m_uint16_array
    }

    /// `Uint16ArrayPrivateName()`: `Identifier::fromUid(Symbols::Uint16ArrayPrivateName)`.
    pub fn uint16_array_private_name(&self) -> Identifier {
        self.m_uint16_array_private_name.clone()
    }

    /// `Uint32ArrayPublicName()`.
    pub fn uint32_array_public_name(&self) -> &Identifier {
        &self.m_uint32_array
    }

    /// `Uint32ArrayPrivateName()`: `Identifier::fromUid(Symbols::Uint32ArrayPrivateName)`.
    pub fn uint32_array_private_name(&self) -> Identifier {
        self.m_uint32_array_private_name.clone()
    }

    /// `Float16ArrayPublicName()`.
    pub fn float16_array_public_name(&self) -> &Identifier {
        &self.m_float16_array
    }

    /// `Float16ArrayPrivateName()`: `Identifier::fromUid(Symbols::Float16ArrayPrivateName)`.
    pub fn float16_array_private_name(&self) -> Identifier {
        self.m_float16_array_private_name.clone()
    }

    /// `Float32ArrayPublicName()`.
    pub fn float32_array_public_name(&self) -> &Identifier {
        &self.m_float32_array
    }

    /// `Float32ArrayPrivateName()`: `Identifier::fromUid(Symbols::Float32ArrayPrivateName)`.
    pub fn float32_array_private_name(&self) -> Identifier {
        self.m_float32_array_private_name.clone()
    }

    /// `Float64ArrayPublicName()`.
    pub fn float64_array_public_name(&self) -> &Identifier {
        &self.m_float64_array
    }

    /// `Float64ArrayPrivateName()`: `Identifier::fromUid(Symbols::Float64ArrayPrivateName)`.
    pub fn float64_array_private_name(&self) -> Identifier {
        self.m_float64_array_private_name.clone()
    }

    /// `BigInt64ArrayPublicName()`.
    pub fn big_int64_array_public_name(&self) -> &Identifier {
        &self.m_big_int64_array
    }

    /// `BigInt64ArrayPrivateName()`: `Identifier::fromUid(Symbols::BigInt64ArrayPrivateName)`.
    pub fn big_int64_array_private_name(&self) -> Identifier {
        self.m_big_int64_array_private_name.clone()
    }

    /// `BigUint64ArrayPublicName()`.
    pub fn big_uint64_array_public_name(&self) -> &Identifier {
        &self.m_big_uint64_array
    }

    /// `BigUint64ArrayPrivateName()`: `Identifier::fromUid(Symbols::BigUint64ArrayPrivateName)`.
    pub fn big_uint64_array_private_name(&self) -> Identifier {
        self.m_big_uint64_array_private_name.clone()
    }

    /// `execPublicName()`.
    pub fn exec_public_name(&self) -> &Identifier {
        &self.m_exec
    }

    /// `execPrivateName()`: `Identifier::fromUid(Symbols::execPrivateName)`.
    pub fn exec_private_name(&self) -> Identifier {
        self.m_exec_private_name.clone()
    }

    /// `generatorPublicName()`.
    pub fn generator_public_name(&self) -> &Identifier {
        &self.m_generator
    }

    /// `generatorPrivateName()`: `Identifier::fromUid(Symbols::generatorPrivateName)`.
    pub fn generator_private_name(&self) -> Identifier {
        self.m_generator_private_name.clone()
    }

    /// `generatorNextPublicName()`.
    pub fn generator_next_public_name(&self) -> &Identifier {
        &self.m_generator_next
    }

    /// `generatorNextPrivateName()`: `Identifier::fromUid(Symbols::generatorNextPrivateName)`.
    pub fn generator_next_private_name(&self) -> Identifier {
        self.m_generator_next_private_name.clone()
    }

    /// `generatorStatePublicName()`.
    pub fn generator_state_public_name(&self) -> &Identifier {
        &self.m_generator_state
    }

    /// `generatorStatePrivateName()`: `Identifier::fromUid(Symbols::generatorStatePrivateName)`.
    pub fn generator_state_private_name(&self) -> Identifier {
        self.m_generator_state_private_name.clone()
    }

    /// `generatorFramePublicName()`.
    pub fn generator_frame_public_name(&self) -> &Identifier {
        &self.m_generator_frame
    }

    /// `generatorFramePrivateName()`: `Identifier::fromUid(Symbols::generatorFramePrivateName)`.
    pub fn generator_frame_private_name(&self) -> Identifier {
        self.m_generator_frame_private_name.clone()
    }

    /// `generatorValuePublicName()`.
    pub fn generator_value_public_name(&self) -> &Identifier {
        &self.m_generator_value
    }

    /// `generatorValuePrivateName()`: `Identifier::fromUid(Symbols::generatorValuePrivateName)`.
    pub fn generator_value_private_name(&self) -> Identifier {
        self.m_generator_value_private_name.clone()
    }

    /// `generatorThisPublicName()`.
    pub fn generator_this_public_name(&self) -> &Identifier {
        &self.m_generator_this
    }

    /// `generatorThisPrivateName()`: `Identifier::fromUid(Symbols::generatorThisPrivateName)`.
    pub fn generator_this_private_name(&self) -> Identifier {
        self.m_generator_this_private_name.clone()
    }

    /// `generatorResumeModePublicName()`.
    pub fn generator_resume_mode_public_name(&self) -> &Identifier {
        &self.m_generator_resume_mode
    }

    /// `generatorResumeModePrivateName()`: `Identifier::fromUid(Symbols::generatorResumeModePrivateName)`.
    pub fn generator_resume_mode_private_name(&self) -> Identifier {
        self.m_generator_resume_mode_private_name.clone()
    }

    /// `thisPublicName()`.
    pub fn this_public_name(&self) -> &Identifier {
        &self.m_this
    }

    /// `thisPrivateName()`: `Identifier::fromUid(Symbols::thisPrivateName)`.
    pub fn this_private_name(&self) -> Identifier {
        self.m_this_private_name.clone()
    }

    /// `toIntegerOrInfinityPublicName()`.
    pub fn to_integer_or_infinity_public_name(&self) -> &Identifier {
        &self.m_to_integer_or_infinity
    }

    /// `toIntegerOrInfinityPrivateName()`: `Identifier::fromUid(Symbols::toIntegerOrInfinityPrivateName)`.
    pub fn to_integer_or_infinity_private_name(&self) -> Identifier {
        self.m_to_integer_or_infinity_private_name.clone()
    }

    /// `toLengthPublicName()`.
    pub fn to_length_public_name(&self) -> &Identifier {
        &self.m_to_length
    }

    /// `toLengthPrivateName()`: `Identifier::fromUid(Symbols::toLengthPrivateName)`.
    pub fn to_length_private_name(&self) -> Identifier {
        self.m_to_length_private_name.clone()
    }

    /// `importInRealmPublicName()`.
    pub fn import_in_realm_public_name(&self) -> &Identifier {
        &self.m_import_in_realm
    }

    /// `importInRealmPrivateName()`: `Identifier::fromUid(Symbols::importInRealmPrivateName)`.
    pub fn import_in_realm_private_name(&self) -> Identifier {
        self.m_import_in_realm_private_name.clone()
    }

    /// `evalFunctionPublicName()`.
    pub fn eval_function_public_name(&self) -> &Identifier {
        &self.m_eval_function
    }

    /// `evalFunctionPrivateName()`: `Identifier::fromUid(Symbols::evalFunctionPrivateName)`.
    pub fn eval_function_private_name(&self) -> Identifier {
        self.m_eval_function_private_name.clone()
    }

    /// `evalInRealmPublicName()`.
    pub fn eval_in_realm_public_name(&self) -> &Identifier {
        &self.m_eval_in_realm
    }

    /// `evalInRealmPrivateName()`: `Identifier::fromUid(Symbols::evalInRealmPrivateName)`.
    pub fn eval_in_realm_private_name(&self) -> Identifier {
        self.m_eval_in_realm_private_name.clone()
    }

    /// `moveFunctionToRealmPublicName()`.
    pub fn move_function_to_realm_public_name(&self) -> &Identifier {
        &self.m_move_function_to_realm
    }

    /// `moveFunctionToRealmPrivateName()`: `Identifier::fromUid(Symbols::moveFunctionToRealmPrivateName)`.
    pub fn move_function_to_realm_private_name(&self) -> Identifier {
        self.m_move_function_to_realm_private_name.clone()
    }

    /// `newTargetLocalPublicName()`.
    pub fn new_target_local_public_name(&self) -> &Identifier {
        &self.m_new_target_local
    }

    /// `newTargetLocalPrivateName()`: `Identifier::fromUid(Symbols::newTargetLocalPrivateName)`.
    pub fn new_target_local_private_name(&self) -> Identifier {
        self.m_new_target_local_private_name.clone()
    }

    /// `derivedConstructorPublicName()`.
    pub fn derived_constructor_public_name(&self) -> &Identifier {
        &self.m_derived_constructor
    }

    /// `derivedConstructorPrivateName()`: `Identifier::fromUid(Symbols::derivedConstructorPrivateName)`.
    pub fn derived_constructor_private_name(&self) -> Identifier {
        self.m_derived_constructor_private_name.clone()
    }

    /// `isTypedArrayViewPublicName()`.
    pub fn is_typed_array_view_public_name(&self) -> &Identifier {
        &self.m_is_typed_array_view
    }

    /// `isTypedArrayViewPrivateName()`: `Identifier::fromUid(Symbols::isTypedArrayViewPrivateName)`.
    pub fn is_typed_array_view_private_name(&self) -> Identifier {
        self.m_is_typed_array_view_private_name.clone()
    }

    /// `isSharedTypedArrayViewPublicName()`.
    pub fn is_shared_typed_array_view_public_name(&self) -> &Identifier {
        &self.m_is_shared_typed_array_view
    }

    /// `isSharedTypedArrayViewPrivateName()`: `Identifier::fromUid(Symbols::isSharedTypedArrayViewPrivateName)`.
    pub fn is_shared_typed_array_view_private_name(&self) -> Identifier {
        self.m_is_shared_typed_array_view_private_name.clone()
    }

    /// `isResizableOrGrowableSharedTypedArrayViewPublicName()`.
    pub fn is_resizable_or_growable_shared_typed_array_view_public_name(&self) -> &Identifier {
        &self.m_is_resizable_or_growable_shared_typed_array_view
    }

    /// `isResizableOrGrowableSharedTypedArrayViewPrivateName()`: `Identifier::fromUid(Symbols::isResizableOrGrowableSharedTypedArrayViewPrivateName)`.
    pub fn is_resizable_or_growable_shared_typed_array_view_private_name(&self) -> Identifier {
        self.m_is_resizable_or_growable_shared_typed_array_view_private_name.clone()
    }

    /// `isDetachedPublicName()`.
    pub fn is_detached_public_name(&self) -> &Identifier {
        &self.m_is_detached
    }

    /// `isDetachedPrivateName()`: `Identifier::fromUid(Symbols::isDetachedPrivateName)`.
    pub fn is_detached_private_name(&self) -> Identifier {
        self.m_is_detached_private_name.clone()
    }

    /// `isTypedArrayOutOfBoundsPublicName()`.
    pub fn is_typed_array_out_of_bounds_public_name(&self) -> &Identifier {
        &self.m_is_typed_array_out_of_bounds
    }

    /// `isTypedArrayOutOfBoundsPrivateName()`: `Identifier::fromUid(Symbols::isTypedArrayOutOfBoundsPrivateName)`.
    pub fn is_typed_array_out_of_bounds_private_name(&self) -> Identifier {
        self.m_is_typed_array_out_of_bounds_private_name.clone()
    }

    /// `typedArrayFromFastPublicName()`.
    pub fn typed_array_from_fast_public_name(&self) -> &Identifier {
        &self.m_typed_array_from_fast
    }

    /// `typedArrayFromFastPrivateName()`: `Identifier::fromUid(Symbols::typedArrayFromFastPrivateName)`.
    pub fn typed_array_from_fast_private_name(&self) -> Identifier {
        self.m_typed_array_from_fast_private_name.clone()
    }

    /// `instanceOfPublicName()`.
    pub fn instance_of_public_name(&self) -> &Identifier {
        &self.m_instance_of
    }

    /// `instanceOfPrivateName()`: `Identifier::fromUid(Symbols::instanceOfPrivateName)`.
    pub fn instance_of_private_name(&self) -> Identifier {
        self.m_instance_of_private_name.clone()
    }

    /// `isArrayPublicName()`.
    pub fn is_array_public_name(&self) -> &Identifier {
        &self.m_is_array
    }

    /// `isArrayPrivateName()`: `Identifier::fromUid(Symbols::isArrayPrivateName)`.
    pub fn is_array_private_name(&self) -> Identifier {
        self.m_is_array_private_name.clone()
    }

    /// `sameValuePublicName()`.
    pub fn same_value_public_name(&self) -> &Identifier {
        &self.m_same_value
    }

    /// `sameValuePrivateName()`: `Identifier::fromUid(Symbols::sameValuePrivateName)`.
    pub fn same_value_private_name(&self) -> Identifier {
        self.m_same_value_private_name.clone()
    }

    /// `regExpCreatePublicName()`.
    pub fn reg_exp_create_public_name(&self) -> &Identifier {
        &self.m_reg_exp_create
    }

    /// `regExpCreatePrivateName()`: `Identifier::fromUid(Symbols::regExpCreatePrivateName)`.
    pub fn reg_exp_create_private_name(&self) -> Identifier {
        self.m_reg_exp_create_private_name.clone()
    }

    /// `isRegExpPublicName()`.
    pub fn is_reg_exp_public_name(&self) -> &Identifier {
        &self.m_is_reg_exp
    }

    /// `isRegExpPrivateName()`: `Identifier::fromUid(Symbols::isRegExpPrivateName)`.
    pub fn is_reg_exp_private_name(&self) -> Identifier {
        self.m_is_reg_exp_private_name.clone()
    }

    /// `isFinitePublicName()`.
    pub fn is_finite_public_name(&self) -> &Identifier {
        &self.m_is_finite
    }

    /// `isFinitePrivateName()`: `Identifier::fromUid(Symbols::isFinitePrivateName)`.
    pub fn is_finite_private_name(&self) -> Identifier {
        self.m_is_finite_private_name.clone()
    }

    /// `makeTypeErrorPublicName()`.
    pub fn make_type_error_public_name(&self) -> &Identifier {
        &self.m_make_type_error
    }

    /// `makeTypeErrorPrivateName()`: `Identifier::fromUid(Symbols::makeTypeErrorPrivateName)`.
    pub fn make_type_error_private_name(&self) -> Identifier {
        self.m_make_type_error_private_name.clone()
    }

    /// `AggregateErrorPublicName()`.
    pub fn aggregate_error_public_name(&self) -> &Identifier {
        &self.m_aggregate_error
    }

    /// `AggregateErrorPrivateName()`: `Identifier::fromUid(Symbols::AggregateErrorPrivateName)`.
    pub fn aggregate_error_private_name(&self) -> Identifier {
        self.m_aggregate_error_private_name.clone()
    }

    /// `mapStoragePublicName()`.
    pub fn map_storage_public_name(&self) -> &Identifier {
        &self.m_map_storage
    }

    /// `mapStoragePrivateName()`: `Identifier::fromUid(Symbols::mapStoragePrivateName)`.
    pub fn map_storage_private_name(&self) -> Identifier {
        self.m_map_storage_private_name.clone()
    }

    /// `mapIterationNextPublicName()`.
    pub fn map_iteration_next_public_name(&self) -> &Identifier {
        &self.m_map_iteration_next
    }

    /// `mapIterationNextPrivateName()`: `Identifier::fromUid(Symbols::mapIterationNextPrivateName)`.
    pub fn map_iteration_next_private_name(&self) -> Identifier {
        self.m_map_iteration_next_private_name.clone()
    }

    /// `mapIterationEntryPublicName()`.
    pub fn map_iteration_entry_public_name(&self) -> &Identifier {
        &self.m_map_iteration_entry
    }

    /// `mapIterationEntryPrivateName()`: `Identifier::fromUid(Symbols::mapIterationEntryPrivateName)`.
    pub fn map_iteration_entry_private_name(&self) -> Identifier {
        self.m_map_iteration_entry_private_name.clone()
    }

    /// `mapIterationEntryKeyPublicName()`.
    pub fn map_iteration_entry_key_public_name(&self) -> &Identifier {
        &self.m_map_iteration_entry_key
    }

    /// `mapIterationEntryKeyPrivateName()`: `Identifier::fromUid(Symbols::mapIterationEntryKeyPrivateName)`.
    pub fn map_iteration_entry_key_private_name(&self) -> Identifier {
        self.m_map_iteration_entry_key_private_name.clone()
    }

    /// `mapIterationEntryValuePublicName()`.
    pub fn map_iteration_entry_value_public_name(&self) -> &Identifier {
        &self.m_map_iteration_entry_value
    }

    /// `mapIterationEntryValuePrivateName()`: `Identifier::fromUid(Symbols::mapIterationEntryValuePrivateName)`.
    pub fn map_iteration_entry_value_private_name(&self) -> Identifier {
        self.m_map_iteration_entry_value_private_name.clone()
    }

    /// `setStoragePublicName()`.
    pub fn set_storage_public_name(&self) -> &Identifier {
        &self.m_set_storage
    }

    /// `setStoragePrivateName()`: `Identifier::fromUid(Symbols::setStoragePrivateName)`.
    pub fn set_storage_private_name(&self) -> Identifier {
        self.m_set_storage_private_name.clone()
    }

    /// `setIterationNextPublicName()`.
    pub fn set_iteration_next_public_name(&self) -> &Identifier {
        &self.m_set_iteration_next
    }

    /// `setIterationNextPrivateName()`: `Identifier::fromUid(Symbols::setIterationNextPrivateName)`.
    pub fn set_iteration_next_private_name(&self) -> Identifier {
        self.m_set_iteration_next_private_name.clone()
    }

    /// `setIterationEntryPublicName()`.
    pub fn set_iteration_entry_public_name(&self) -> &Identifier {
        &self.m_set_iteration_entry
    }

    /// `setIterationEntryPrivateName()`: `Identifier::fromUid(Symbols::setIterationEntryPrivateName)`.
    pub fn set_iteration_entry_private_name(&self) -> Identifier {
        self.m_set_iteration_entry_private_name.clone()
    }

    /// `setIterationEntryKeyPublicName()`.
    pub fn set_iteration_entry_key_public_name(&self) -> &Identifier {
        &self.m_set_iteration_entry_key
    }

    /// `setIterationEntryKeyPrivateName()`: `Identifier::fromUid(Symbols::setIterationEntryKeyPrivateName)`.
    pub fn set_iteration_entry_key_private_name(&self) -> Identifier {
        self.m_set_iteration_entry_key_private_name.clone()
    }

    /// `setPrototypeDirectPublicName()`.
    pub fn set_prototype_direct_public_name(&self) -> &Identifier {
        &self.m_set_prototype_direct
    }

    /// `setPrototypeDirectPrivateName()`: `Identifier::fromUid(Symbols::setPrototypeDirectPrivateName)`.
    pub fn set_prototype_direct_private_name(&self) -> Identifier {
        self.m_set_prototype_direct_private_name.clone()
    }

    /// `setPrototypeDirectOrThrowPublicName()`.
    pub fn set_prototype_direct_or_throw_public_name(&self) -> &Identifier {
        &self.m_set_prototype_direct_or_throw
    }

    /// `setPrototypeDirectOrThrowPrivateName()`: `Identifier::fromUid(Symbols::setPrototypeDirectOrThrowPrivateName)`.
    pub fn set_prototype_direct_or_throw_private_name(&self) -> Identifier {
        self.m_set_prototype_direct_or_throw_private_name.clone()
    }

    /// `regExpBuiltinExecPublicName()`.
    pub fn reg_exp_builtin_exec_public_name(&self) -> &Identifier {
        &self.m_reg_exp_builtin_exec
    }

    /// `regExpBuiltinExecPrivateName()`: `Identifier::fromUid(Symbols::regExpBuiltinExecPrivateName)`.
    pub fn reg_exp_builtin_exec_private_name(&self) -> Identifier {
        self.m_reg_exp_builtin_exec_private_name.clone()
    }

    /// `regExpProtoFlagsGetterPublicName()`.
    pub fn reg_exp_proto_flags_getter_public_name(&self) -> &Identifier {
        &self.m_reg_exp_proto_flags_getter
    }

    /// `regExpProtoFlagsGetterPrivateName()`: `Identifier::fromUid(Symbols::regExpProtoFlagsGetterPrivateName)`.
    pub fn reg_exp_proto_flags_getter_private_name(&self) -> Identifier {
        self.m_reg_exp_proto_flags_getter_private_name.clone()
    }

    /// `regExpProtoHasIndicesGetterPublicName()`.
    pub fn reg_exp_proto_has_indices_getter_public_name(&self) -> &Identifier {
        &self.m_reg_exp_proto_has_indices_getter
    }

    /// `regExpProtoHasIndicesGetterPrivateName()`: `Identifier::fromUid(Symbols::regExpProtoHasIndicesGetterPrivateName)`.
    pub fn reg_exp_proto_has_indices_getter_private_name(&self) -> Identifier {
        self.m_reg_exp_proto_has_indices_getter_private_name.clone()
    }

    /// `regExpProtoGlobalGetterPublicName()`.
    pub fn reg_exp_proto_global_getter_public_name(&self) -> &Identifier {
        &self.m_reg_exp_proto_global_getter
    }

    /// `regExpProtoGlobalGetterPrivateName()`: `Identifier::fromUid(Symbols::regExpProtoGlobalGetterPrivateName)`.
    pub fn reg_exp_proto_global_getter_private_name(&self) -> Identifier {
        self.m_reg_exp_proto_global_getter_private_name.clone()
    }

    /// `regExpProtoIgnoreCaseGetterPublicName()`.
    pub fn reg_exp_proto_ignore_case_getter_public_name(&self) -> &Identifier {
        &self.m_reg_exp_proto_ignore_case_getter
    }

    /// `regExpProtoIgnoreCaseGetterPrivateName()`: `Identifier::fromUid(Symbols::regExpProtoIgnoreCaseGetterPrivateName)`.
    pub fn reg_exp_proto_ignore_case_getter_private_name(&self) -> Identifier {
        self.m_reg_exp_proto_ignore_case_getter_private_name.clone()
    }

    /// `regExpProtoMultilineGetterPublicName()`.
    pub fn reg_exp_proto_multiline_getter_public_name(&self) -> &Identifier {
        &self.m_reg_exp_proto_multiline_getter
    }

    /// `regExpProtoMultilineGetterPrivateName()`: `Identifier::fromUid(Symbols::regExpProtoMultilineGetterPrivateName)`.
    pub fn reg_exp_proto_multiline_getter_private_name(&self) -> Identifier {
        self.m_reg_exp_proto_multiline_getter_private_name.clone()
    }

    /// `regExpProtoSourceGetterPublicName()`.
    pub fn reg_exp_proto_source_getter_public_name(&self) -> &Identifier {
        &self.m_reg_exp_proto_source_getter
    }

    /// `regExpProtoSourceGetterPrivateName()`: `Identifier::fromUid(Symbols::regExpProtoSourceGetterPrivateName)`.
    pub fn reg_exp_proto_source_getter_private_name(&self) -> Identifier {
        self.m_reg_exp_proto_source_getter_private_name.clone()
    }

    /// `regExpProtoStickyGetterPublicName()`.
    pub fn reg_exp_proto_sticky_getter_public_name(&self) -> &Identifier {
        &self.m_reg_exp_proto_sticky_getter
    }

    /// `regExpProtoStickyGetterPrivateName()`: `Identifier::fromUid(Symbols::regExpProtoStickyGetterPrivateName)`.
    pub fn reg_exp_proto_sticky_getter_private_name(&self) -> Identifier {
        self.m_reg_exp_proto_sticky_getter_private_name.clone()
    }

    /// `regExpProtoDotAllGetterPublicName()`.
    pub fn reg_exp_proto_dot_all_getter_public_name(&self) -> &Identifier {
        &self.m_reg_exp_proto_dot_all_getter
    }

    /// `regExpProtoDotAllGetterPrivateName()`: `Identifier::fromUid(Symbols::regExpProtoDotAllGetterPrivateName)`.
    pub fn reg_exp_proto_dot_all_getter_private_name(&self) -> Identifier {
        self.m_reg_exp_proto_dot_all_getter_private_name.clone()
    }

    /// `regExpProtoUnicodeGetterPublicName()`.
    pub fn reg_exp_proto_unicode_getter_public_name(&self) -> &Identifier {
        &self.m_reg_exp_proto_unicode_getter
    }

    /// `regExpProtoUnicodeGetterPrivateName()`: `Identifier::fromUid(Symbols::regExpProtoUnicodeGetterPrivateName)`.
    pub fn reg_exp_proto_unicode_getter_private_name(&self) -> Identifier {
        self.m_reg_exp_proto_unicode_getter_private_name.clone()
    }

    /// `regExpProtoUnicodeSetsGetterPublicName()`.
    pub fn reg_exp_proto_unicode_sets_getter_public_name(&self) -> &Identifier {
        &self.m_reg_exp_proto_unicode_sets_getter
    }

    /// `regExpProtoUnicodeSetsGetterPrivateName()`: `Identifier::fromUid(Symbols::regExpProtoUnicodeSetsGetterPrivateName)`.
    pub fn reg_exp_proto_unicode_sets_getter_private_name(&self) -> Identifier {
        self.m_reg_exp_proto_unicode_sets_getter_private_name.clone()
    }

    /// `regExpPrototypeSymbolMatchPublicName()`.
    pub fn reg_exp_prototype_symbol_match_public_name(&self) -> &Identifier {
        &self.m_reg_exp_prototype_symbol_match
    }

    /// `regExpPrototypeSymbolMatchPrivateName()`: `Identifier::fromUid(Symbols::regExpPrototypeSymbolMatchPrivateName)`.
    pub fn reg_exp_prototype_symbol_match_private_name(&self) -> Identifier {
        self.m_reg_exp_prototype_symbol_match_private_name.clone()
    }

    /// `regExpPrototypeSymbolMatchAllPublicName()`.
    pub fn reg_exp_prototype_symbol_match_all_public_name(&self) -> &Identifier {
        &self.m_reg_exp_prototype_symbol_match_all
    }

    /// `regExpPrototypeSymbolMatchAllPrivateName()`: `Identifier::fromUid(Symbols::regExpPrototypeSymbolMatchAllPrivateName)`.
    pub fn reg_exp_prototype_symbol_match_all_private_name(&self) -> Identifier {
        self.m_reg_exp_prototype_symbol_match_all_private_name.clone()
    }

    /// `regExpPrototypeSymbolReplacePublicName()`.
    pub fn reg_exp_prototype_symbol_replace_public_name(&self) -> &Identifier {
        &self.m_reg_exp_prototype_symbol_replace
    }

    /// `regExpPrototypeSymbolReplacePrivateName()`: `Identifier::fromUid(Symbols::regExpPrototypeSymbolReplacePrivateName)`.
    pub fn reg_exp_prototype_symbol_replace_private_name(&self) -> Identifier {
        self.m_reg_exp_prototype_symbol_replace_private_name.clone()
    }

    /// `regExpSearchFastPublicName()`.
    pub fn reg_exp_search_fast_public_name(&self) -> &Identifier {
        &self.m_reg_exp_search_fast
    }

    /// `regExpSearchFastPrivateName()`: `Identifier::fromUid(Symbols::regExpSearchFastPrivateName)`.
    pub fn reg_exp_search_fast_private_name(&self) -> Identifier {
        self.m_reg_exp_search_fast_private_name.clone()
    }

    /// `stringIncludesInternalPublicName()`.
    pub fn string_includes_internal_public_name(&self) -> &Identifier {
        &self.m_string_includes_internal
    }

    /// `stringIncludesInternalPrivateName()`: `Identifier::fromUid(Symbols::stringIncludesInternalPrivateName)`.
    pub fn string_includes_internal_private_name(&self) -> Identifier {
        self.m_string_includes_internal_private_name.clone()
    }

    /// `stringIndexOfInternalPublicName()`.
    pub fn string_index_of_internal_public_name(&self) -> &Identifier {
        &self.m_string_index_of_internal
    }

    /// `stringIndexOfInternalPrivateName()`: `Identifier::fromUid(Symbols::stringIndexOfInternalPrivateName)`.
    pub fn string_index_of_internal_private_name(&self) -> Identifier {
        self.m_string_index_of_internal_private_name.clone()
    }

    /// `stringSubstringPublicName()`.
    pub fn string_substring_public_name(&self) -> &Identifier {
        &self.m_string_substring
    }

    /// `stringSubstringPrivateName()`: `Identifier::fromUid(Symbols::stringSubstringPrivateName)`.
    pub fn string_substring_private_name(&self) -> Identifier {
        self.m_string_substring_private_name.clone()
    }

    /// `handleNegativeProxyHasTrapResultPublicName()`.
    pub fn handle_negative_proxy_has_trap_result_public_name(&self) -> &Identifier {
        &self.m_handle_negative_proxy_has_trap_result
    }

    /// `handleNegativeProxyHasTrapResultPrivateName()`: `Identifier::fromUid(Symbols::handleNegativeProxyHasTrapResultPrivateName)`.
    pub fn handle_negative_proxy_has_trap_result_private_name(&self) -> Identifier {
        self.m_handle_negative_proxy_has_trap_result_private_name.clone()
    }

    /// `handlePositiveProxySetTrapResultPublicName()`.
    pub fn handle_positive_proxy_set_trap_result_public_name(&self) -> &Identifier {
        &self.m_handle_positive_proxy_set_trap_result
    }

    /// `handlePositiveProxySetTrapResultPrivateName()`: `Identifier::fromUid(Symbols::handlePositiveProxySetTrapResultPrivateName)`.
    pub fn handle_positive_proxy_set_trap_result_private_name(&self) -> Identifier {
        self.m_handle_positive_proxy_set_trap_result_private_name.clone()
    }

    /// `handleProxyGetTrapResultPublicName()`.
    pub fn handle_proxy_get_trap_result_public_name(&self) -> &Identifier {
        &self.m_handle_proxy_get_trap_result
    }

    /// `handleProxyGetTrapResultPrivateName()`: `Identifier::fromUid(Symbols::handleProxyGetTrapResultPrivateName)`.
    pub fn handle_proxy_get_trap_result_private_name(&self) -> Identifier {
        self.m_handle_proxy_get_trap_result_private_name.clone()
    }

    /// `importModulePublicName()`.
    pub fn import_module_public_name(&self) -> &Identifier {
        &self.m_import_module
    }

    /// `importModulePrivateName()`: `Identifier::fromUid(Symbols::importModulePrivateName)`.
    pub fn import_module_private_name(&self) -> Identifier {
        self.m_import_module_private_name.clone()
    }

    /// `moduleFetchFailureKindPublicName()`.
    pub fn module_fetch_failure_kind_public_name(&self) -> &Identifier {
        &self.m_module_fetch_failure_kind
    }

    /// `moduleFetchFailureKindPrivateName()`: `Identifier::fromUid(Symbols::moduleFetchFailureKindPrivateName)`.
    pub fn module_fetch_failure_kind_private_name(&self) -> Identifier {
        self.m_module_fetch_failure_kind_private_name.clone()
    }

    /// `moduleFailureModuleRecordPublicName()`.
    pub fn module_failure_module_record_public_name(&self) -> &Identifier {
        &self.m_module_failure_module_record
    }

    /// `moduleFailureModuleRecordPrivateName()`: `Identifier::fromUid(Symbols::moduleFailureModuleRecordPrivateName)`.
    pub fn module_failure_module_record_private_name(&self) -> Identifier {
        self.m_module_failure_module_record_private_name.clone()
    }

    /// `moduleFailureModuleKeyPublicName()`.
    pub fn module_failure_module_key_public_name(&self) -> &Identifier {
        &self.m_module_failure_module_key
    }

    /// `moduleFailureModuleKeyPrivateName()`: `Identifier::fromUid(Symbols::moduleFailureModuleKeyPrivateName)`.
    pub fn module_failure_module_key_private_name(&self) -> Identifier {
        self.m_module_failure_module_key_private_name.clone()
    }

    /// `moduleFailureModuleTypePublicName()`.
    pub fn module_failure_module_type_public_name(&self) -> &Identifier {
        &self.m_module_failure_module_type
    }

    /// `moduleFailureModuleTypePrivateName()`: `Identifier::fromUid(Symbols::moduleFailureModuleTypePrivateName)`.
    pub fn module_failure_module_type_private_name(&self) -> Identifier {
        self.m_module_failure_module_type_private_name.clone()
    }

    /// `moduleFailureKindPublicName()`.
    pub fn module_failure_kind_public_name(&self) -> &Identifier {
        &self.m_module_failure_kind
    }

    /// `moduleFailureKindPrivateName()`: `Identifier::fromUid(Symbols::moduleFailureKindPrivateName)`.
    pub fn module_failure_kind_private_name(&self) -> Identifier {
        self.m_module_failure_kind_private_name.clone()
    }

    /// `copyDataPropertiesPublicName()`.
    pub fn copy_data_properties_public_name(&self) -> &Identifier {
        &self.m_copy_data_properties
    }

    /// `copyDataPropertiesPrivateName()`: `Identifier::fromUid(Symbols::copyDataPropertiesPrivateName)`.
    pub fn copy_data_properties_private_name(&self) -> Identifier {
        self.m_copy_data_properties_private_name.clone()
    }

    /// `cloneObjectPublicName()`.
    pub fn clone_object_public_name(&self) -> &Identifier {
        &self.m_clone_object
    }

    /// `cloneObjectPrivateName()`: `Identifier::fromUid(Symbols::cloneObjectPrivateName)`.
    pub fn clone_object_private_name(&self) -> Identifier {
        self.m_clone_object_private_name.clone()
    }

    /// `metaPublicName()`.
    pub fn meta_public_name(&self) -> &Identifier {
        &self.m_meta
    }

    /// `metaPrivateName()`: `Identifier::fromUid(Symbols::metaPrivateName)`.
    pub fn meta_private_name(&self) -> Identifier {
        self.m_meta_private_name.clone()
    }

    /// `instanceFieldInitializerPublicName()`.
    pub fn instance_field_initializer_public_name(&self) -> &Identifier {
        &self.m_instance_field_initializer
    }

    /// `instanceFieldInitializerPrivateName()`: `Identifier::fromUid(Symbols::instanceFieldInitializerPrivateName)`.
    pub fn instance_field_initializer_private_name(&self) -> Identifier {
        self.m_instance_field_initializer_private_name.clone()
    }

    /// `privateBrandPublicName()`.
    pub fn private_brand_public_name(&self) -> &Identifier {
        &self.m_private_brand
    }

    /// `privateBrandPrivateName()`: `Identifier::fromUid(Symbols::privateBrandPrivateName)`.
    pub fn private_brand_private_name(&self) -> Identifier {
        self.m_private_brand_private_name.clone()
    }

    /// `privateClassBrandPublicName()`.
    pub fn private_class_brand_public_name(&self) -> &Identifier {
        &self.m_private_class_brand
    }

    /// `privateClassBrandPrivateName()`: `Identifier::fromUid(Symbols::privateClassBrandPrivateName)`.
    pub fn private_class_brand_private_name(&self) -> Identifier {
        self.m_private_class_brand_private_name.clone()
    }

    /// `hasOwnPropertyFunctionPublicName()`.
    pub fn has_own_property_function_public_name(&self) -> &Identifier {
        &self.m_has_own_property_function
    }

    /// `hasOwnPropertyFunctionPrivateName()`: `Identifier::fromUid(Symbols::hasOwnPropertyFunctionPrivateName)`.
    pub fn has_own_property_function_private_name(&self) -> Identifier {
        self.m_has_own_property_function_private_name.clone()
    }

    /// `createPrivateSymbolPublicName()`.
    pub fn create_private_symbol_public_name(&self) -> &Identifier {
        &self.m_create_private_symbol
    }

    /// `createPrivateSymbolPrivateName()`: `Identifier::fromUid(Symbols::createPrivateSymbolPrivateName)`.
    pub fn create_private_symbol_private_name(&self) -> Identifier {
        self.m_create_private_symbol_private_name.clone()
    }

    /// `entriesPublicName()`.
    pub fn entries_public_name(&self) -> &Identifier {
        &self.m_entries
    }

    /// `entriesPrivateName()`: `Identifier::fromUid(Symbols::entriesPrivateName)`.
    pub fn entries_private_name(&self) -> Identifier {
        self.m_entries_private_name.clone()
    }

    /// `emptyPropertyNameEnumeratorPublicName()`.
    pub fn empty_property_name_enumerator_public_name(&self) -> &Identifier {
        &self.m_empty_property_name_enumerator
    }

    /// `emptyPropertyNameEnumeratorPrivateName()`: `Identifier::fromUid(Symbols::emptyPropertyNameEnumeratorPrivateName)`.
    pub fn empty_property_name_enumerator_private_name(&self) -> Identifier {
        self.m_empty_property_name_enumerator_private_name.clone()
    }

    /// `sentinelStringPublicName()`.
    pub fn sentinel_string_public_name(&self) -> &Identifier {
        &self.m_sentinel_string
    }

    /// `sentinelStringPrivateName()`: `Identifier::fromUid(Symbols::sentinelStringPrivateName)`.
    pub fn sentinel_string_private_name(&self) -> Identifier {
        self.m_sentinel_string_private_name.clone()
    }

    /// `createRemoteFunctionPublicName()`.
    pub fn create_remote_function_public_name(&self) -> &Identifier {
        &self.m_create_remote_function
    }

    /// `createRemoteFunctionPrivateName()`: `Identifier::fromUid(Symbols::createRemoteFunctionPrivateName)`.
    pub fn create_remote_function_private_name(&self) -> Identifier {
        self.m_create_remote_function_private_name.clone()
    }

    /// `isRemoteFunctionPublicName()`.
    pub fn is_remote_function_public_name(&self) -> &Identifier {
        &self.m_is_remote_function
    }

    /// `isRemoteFunctionPrivateName()`: `Identifier::fromUid(Symbols::isRemoteFunctionPrivateName)`.
    pub fn is_remote_function_private_name(&self) -> Identifier {
        self.m_is_remote_function_private_name.clone()
    }

    /// `arrayFromFastWithoutMapFnPublicName()`.
    pub fn array_from_fast_without_map_fn_public_name(&self) -> &Identifier {
        &self.m_array_from_fast_without_map_fn
    }

    /// `arrayFromFastWithoutMapFnPrivateName()`: `Identifier::fromUid(Symbols::arrayFromFastWithoutMapFnPrivateName)`.
    pub fn array_from_fast_without_map_fn_private_name(&self) -> Identifier {
        self.m_array_from_fast_without_map_fn_private_name.clone()
    }

    /// `jsonParsePublicName()`.
    pub fn json_parse_public_name(&self) -> &Identifier {
        &self.m_json_parse
    }

    /// `jsonParsePrivateName()`: `Identifier::fromUid(Symbols::jsonParsePrivateName)`.
    pub fn json_parse_private_name(&self) -> Identifier {
        self.m_json_parse_private_name.clone()
    }

    /// `jsonStringifyPublicName()`.
    pub fn json_stringify_public_name(&self) -> &Identifier {
        &self.m_json_stringify
    }

    /// `jsonStringifyPrivateName()`: `Identifier::fromUid(Symbols::jsonStringifyPrivateName)`.
    pub fn json_stringify_private_name(&self) -> Identifier {
        self.m_json_stringify_private_name.clone()
    }

    /// `StringPublicName()`.
    pub fn string_public_name(&self) -> &Identifier {
        &self.m_string
    }

    /// `StringPrivateName()`: `Identifier::fromUid(Symbols::StringPrivateName)`.
    pub fn string_private_name(&self) -> Identifier {
        self.m_string_private_name.clone()
    }

    /// `substrPublicName()`.
    pub fn substr_public_name(&self) -> &Identifier {
        &self.m_substr
    }

    /// `substrPrivateName()`: `Identifier::fromUid(Symbols::substrPrivateName)`.
    pub fn substr_private_name(&self) -> Identifier {
        self.m_substr_private_name.clone()
    }

    /// `endsWithPublicName()`.
    pub fn ends_with_public_name(&self) -> &Identifier {
        &self.m_ends_with
    }

    /// `endsWithPrivateName()`: `Identifier::fromUid(Symbols::endsWithPrivateName)`.
    pub fn ends_with_private_name(&self) -> Identifier {
        self.m_ends_with_private_name.clone()
    }

    /// `getOwnPropertyDescriptorPublicName()`.
    pub fn get_own_property_descriptor_public_name(&self) -> &Identifier {
        &self.m_get_own_property_descriptor
    }

    /// `getOwnPropertyDescriptorPrivateName()`: `Identifier::fromUid(Symbols::getOwnPropertyDescriptorPrivateName)`.
    pub fn get_own_property_descriptor_private_name(&self) -> Identifier {
        self.m_get_own_property_descriptor_private_name.clone()
    }

    /// `getOwnPropertyNamesPublicName()`.
    pub fn get_own_property_names_public_name(&self) -> &Identifier {
        &self.m_get_own_property_names
    }

    /// `getOwnPropertyNamesPrivateName()`: `Identifier::fromUid(Symbols::getOwnPropertyNamesPrivateName)`.
    pub fn get_own_property_names_private_name(&self) -> Identifier {
        self.m_get_own_property_names_private_name.clone()
    }

    /// `getOwnPropertySymbolsPublicName()`.
    pub fn get_own_property_symbols_public_name(&self) -> &Identifier {
        &self.m_get_own_property_symbols
    }

    /// `getOwnPropertySymbolsPrivateName()`: `Identifier::fromUid(Symbols::getOwnPropertySymbolsPrivateName)`.
    pub fn get_own_property_symbols_private_name(&self) -> Identifier {
        self.m_get_own_property_symbols_private_name.clone()
    }

    /// `hasOwnPublicName()`.
    pub fn has_own_public_name(&self) -> &Identifier {
        &self.m_has_own
    }

    /// `hasOwnPrivateName()`: `Identifier::fromUid(Symbols::hasOwnPrivateName)`.
    pub fn has_own_private_name(&self) -> Identifier {
        self.m_has_own_private_name.clone()
    }

    /// `indexOfPublicName()`.
    pub fn index_of_public_name(&self) -> &Identifier {
        &self.m_index_of
    }

    /// `indexOfPrivateName()`: `Identifier::fromUid(Symbols::indexOfPrivateName)`.
    pub fn index_of_private_name(&self) -> Identifier {
        self.m_index_of_private_name.clone()
    }

    /// `popPublicName()`.
    pub fn pop_public_name(&self) -> &Identifier {
        &self.m_pop
    }

    /// `popPrivateName()`: `Identifier::fromUid(Symbols::popPrivateName)`.
    pub fn pop_private_name(&self) -> Identifier {
        self.m_pop_private_name.clone()
    }

    /// `asyncContextPublicName()`.
    pub fn async_context_public_name(&self) -> &Identifier {
        &self.m_async_context
    }

    /// `asyncContextPrivateName()`: `Identifier::fromUid(Symbols::asyncContextPrivateName)`.
    pub fn async_context_private_name(&self) -> Identifier {
        self.m_async_context_private_name.clone()
    }

    /// `wrapForValidIteratorCreatePublicName()`.
    pub fn wrap_for_valid_iterator_create_public_name(&self) -> &Identifier {
        &self.m_wrap_for_valid_iterator_create
    }

    /// `wrapForValidIteratorCreatePrivateName()`: `Identifier::fromUid(Symbols::wrapForValidIteratorCreatePrivateName)`.
    pub fn wrap_for_valid_iterator_create_private_name(&self) -> Identifier {
        self.m_wrap_for_valid_iterator_create_private_name.clone()
    }

    /// `asyncFromSyncIteratorCreatePublicName()`.
    pub fn async_from_sync_iterator_create_public_name(&self) -> &Identifier {
        &self.m_async_from_sync_iterator_create
    }

    /// `asyncFromSyncIteratorCreatePrivateName()`: `Identifier::fromUid(Symbols::asyncFromSyncIteratorCreatePrivateName)`.
    pub fn async_from_sync_iterator_create_private_name(&self) -> Identifier {
        self.m_async_from_sync_iterator_create_private_name.clone()
    }

    /// `regExpStringIteratorCreatePublicName()`.
    pub fn reg_exp_string_iterator_create_public_name(&self) -> &Identifier {
        &self.m_reg_exp_string_iterator_create
    }

    /// `regExpStringIteratorCreatePrivateName()`: `Identifier::fromUid(Symbols::regExpStringIteratorCreatePrivateName)`.
    pub fn reg_exp_string_iterator_create_private_name(&self) -> Identifier {
        self.m_reg_exp_string_iterator_create_private_name.clone()
    }

    /// `iteratorHelperCreatePublicName()`.
    pub fn iterator_helper_create_public_name(&self) -> &Identifier {
        &self.m_iterator_helper_create
    }

    /// `iteratorHelperCreatePrivateName()`: `Identifier::fromUid(Symbols::iteratorHelperCreatePrivateName)`.
    pub fn iterator_helper_create_private_name(&self) -> Identifier {
        self.m_iterator_helper_create_private_name.clone()
    }

    /// `ownKeysPublicName()`.
    pub fn own_keys_public_name(&self) -> &Identifier {
        &self.m_own_keys
    }

    /// `ownKeysPrivateName()`: `Identifier::fromUid(Symbols::ownKeysPrivateName)`.
    pub fn own_keys_private_name(&self) -> Identifier {
        self.m_own_keys_private_name.clone()
    }

    /// `includesPublicName()`.
    pub fn includes_public_name(&self) -> &Identifier {
        &self.m_includes
    }

    /// `includesPrivateName()`: `Identifier::fromUid(Symbols::includesPrivateName)`.
    pub fn includes_private_name(&self) -> Identifier {
        self.m_includes_private_name.clone()
    }

    /// `ReferenceErrorPublicName()`.
    pub fn reference_error_public_name(&self) -> &Identifier {
        &self.m_reference_error
    }

    /// `ReferenceErrorPrivateName()`: `Identifier::fromUid(Symbols::ReferenceErrorPrivateName)`.
    pub fn reference_error_private_name(&self) -> Identifier {
        self.m_reference_error_private_name.clone()
    }

    /// `SuppressedErrorPublicName()`.
    pub fn suppressed_error_public_name(&self) -> &Identifier {
        &self.m_suppressed_error
    }

    /// `SuppressedErrorPrivateName()`: `Identifier::fromUid(Symbols::SuppressedErrorPrivateName)`.
    pub fn suppressed_error_private_name(&self) -> Identifier {
        self.m_suppressed_error_private_name.clone()
    }

    /// `DisposableStackPublicName()`.
    pub fn disposable_stack_public_name(&self) -> &Identifier {
        &self.m_disposable_stack
    }

    /// `DisposableStackPrivateName()`: `Identifier::fromUid(Symbols::DisposableStackPrivateName)`.
    pub fn disposable_stack_private_name(&self) -> Identifier {
        self.m_disposable_stack_private_name.clone()
    }

    /// `AsyncDisposableStackPublicName()`.
    pub fn async_disposable_stack_public_name(&self) -> &Identifier {
        &self.m_async_disposable_stack
    }

    /// `AsyncDisposableStackPrivateName()`: `Identifier::fromUid(Symbols::AsyncDisposableStackPrivateName)`.
    pub fn async_disposable_stack_private_name(&self) -> Identifier {
        self.m_async_disposable_stack_private_name.clone()
    }

    /// `enqueueJobPublicName()`.
    pub fn enqueue_job_public_name(&self) -> &Identifier {
        &self.m_enqueue_job
    }

    /// `enqueueJobPrivateName()`: `Identifier::fromUid(Symbols::enqueueJobPrivateName)`.
    pub fn enqueue_job_private_name(&self) -> Identifier {
        self.m_enqueue_job_private_name.clone()
    }

    /// `hasInstanceSymbol()`.
    pub fn has_instance_symbol(&self) -> &Identifier {
        &self.m_has_instance_symbol
    }

    /// `isConcatSpreadableSymbol()`.
    pub fn is_concat_spreadable_symbol(&self) -> &Identifier {
        &self.m_is_concat_spreadable_symbol
    }

    /// `asyncIteratorSymbol()`.
    pub fn async_iterator_symbol(&self) -> &Identifier {
        &self.m_async_iterator_symbol
    }

    /// `iteratorSymbol()`.
    pub fn iterator_symbol(&self) -> &Identifier {
        &self.m_iterator_symbol
    }

    /// `matchSymbol()`.
    pub fn match_symbol(&self) -> &Identifier {
        &self.m_match_symbol
    }

    /// `matchAllSymbol()`.
    pub fn match_all_symbol(&self) -> &Identifier {
        &self.m_match_all_symbol
    }

    /// `replaceSymbol()`.
    pub fn replace_symbol(&self) -> &Identifier {
        &self.m_replace_symbol
    }

    /// `searchSymbol()`.
    pub fn search_symbol(&self) -> &Identifier {
        &self.m_search_symbol
    }

    /// `speciesSymbol()`.
    pub fn species_symbol(&self) -> &Identifier {
        &self.m_species_symbol
    }

    /// `splitSymbol()`.
    pub fn split_symbol(&self) -> &Identifier {
        &self.m_split_symbol
    }

    /// `toPrimitiveSymbol()`.
    pub fn to_primitive_symbol(&self) -> &Identifier {
        &self.m_to_primitive_symbol
    }

    /// `toStringTagSymbol()`.
    pub fn to_string_tag_symbol(&self) -> &Identifier {
        &self.m_to_string_tag_symbol
    }

    /// `unscopablesSymbol()`.
    pub fn unscopables_symbol(&self) -> &Identifier {
        &self.m_unscopables_symbol
    }

    /// `disposeSymbol()`.
    pub fn dispose_symbol(&self) -> &Identifier {
        &self.m_dispose_symbol
    }

    /// `asyncDisposeSymbol()`.
    pub fn async_dispose_symbol(&self) -> &Identifier {
        &self.m_async_dispose_symbol
    }

    /// `dollarVMPublicName()`.
    pub fn dollar_vm_public_name(&self) -> &Identifier {
        &self.m_dollar_vm_name
    }

    /// `dollarVMPrivateName()`.
    pub fn dollar_vm_private_name(&self) -> &Identifier {
        &self.m_dollar_vm_private_name
    }

    /// `polyProtoName()`.
    pub fn poly_proto_name(&self) -> &Identifier {
        &self.m_poly_proto_private_name
    }

    /// `stackPrivateName()`.
    pub fn stack_private_name(&self) -> &Identifier {
        &self.m_stack_private_name
    }
}
