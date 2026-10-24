//! Gerado por `scripts/gen-options.py` a partir de `upstream/JavaScriptCore/runtime/OptionsList.h`
//! (e de `derived/JavaScriptCore/JSCWebPreferenceOptions.h`). Não editar à mão.

use super::options::{
    Availability, GCLogLevel, OSLogType, OptionAlias, OptionInfo, OptionRange, OptionType,
};
use super::options::{MAXIMUM_NUMBER_OF_FTL_COMPILER_THREADS, assert_enabled, can_use_jit_cage, can_use_wasm, compute_number_of_gc_markers, compute_number_of_worker_threads, compute_priority_delta_of_worker_threads, default_quick_dfg_tier_up_threshold_factor, default_quick_ftl_tier_up_threshold_factor, default_relaxed_profile_coverage_factor_for_quick_dfg_tier_up, default_tcsm_value, has_capacity_to_use_large_gigacage, ipint_enabled_by_default, jit_enabled_by_default};

/// `NumberOfOptions`.
pub const NUMBER_OF_OPTIONS: usize = 580;

/// `OptionsStorage`: um campo por opção do `FOR_EACH_JSC_OPTION`, na ordem da lista.
#[derive(Clone, Debug)]
pub struct Options {
    pub use_kern_tcsm: bool,
    pub validate_options: bool,
    pub dump_options: u32,
    pub config_file: Option<String>,
    pub use_ll_int: bool,
    pub use_jit: bool,
    pub use_baseline_jit: bool,
    pub use_dfgjit: bool,
    pub use_reg_exp_jit: bool,
    pub use_domjit: bool,
    pub use_reg_exp_lookbehind_jit: bool,
    pub use_reg_exp_alternation_factoring: bool,
    pub use_reg_exp_alternation_dispatch: bool,
    pub reg_exp_dispatch_max_inline_literal_length: u32,
    pub report_must_succeed_executable_allocations: bool,
    pub use_v8_date_parser: bool,
    pub show_private_scripts_in_stack_traces: bool,
    pub eval_mode: bool,
    pub use_ffiic_stub: bool,
    pub use_ffi_call_in_dfg: bool,
    pub use_ffi_direct_call: bool,
    pub dump_ffi_disassembly: bool,
    pub verbose_ffi: bool,
    pub max_per_thread_stack_usage: u32,
    pub soft_reserved_zone_size: u32,
    pub reserved_zone_size: u32,
    pub crash_on_disallowed_vm_entry: bool,
    pub crash_if_cant_allocate_jit_memory: bool,
    pub structure_heap_size_in_kb: u32,
    pub jit_memory_reservation_size: u32,
    pub jit_memory_reservation_address: usize,
    pub force_code_block_liveness: bool,
    pub force_ic_failure: bool,
    pub force_unlinked_dfg: bool,
    pub repatch_count_for_cool_down: u32,
    pub initial_cool_down_count: u32,
    pub repatch_buffering_countdown: u32,
    pub initial_repatch_buffering_countdown: u32,
    pub dump_generated_bytecodes: bool,
    pub dump_bytecode_liveness_results: bool,
    pub validate_bytecode: bool,
    pub force_debugger_bytecode_generation: bool,
    pub debugger_triggers_breakpoint_exception: bool,
    pub verbose_wasm_debugger: bool,
    pub enable_wasm_debugger: bool,
    pub verbose_wasm_type_cleanup: bool,
    pub dump_bytecodes_before_generatorification: bool,
    pub switch_jump_table_amount_threshold: u32,
    pub use_function_dot_arguments: bool,
    pub use_tail_calls: bool,
    pub optimize_recursive_tail_calls: bool,
    pub always_use_shadow_chicken: bool,
    pub shadow_chicken_log_size: u32,
    pub shadow_chicken_max_tail_deleted_frames_size: u32,
    pub use_os_log: OSLogType,
    pub need_disassembly_support: bool,
    pub dump_disassembly: bool,
    pub log_jit: bool,
    pub dump_baseline_disassembly: bool,
    pub dump_dfg_disassembly: bool,
    pub dump_ftl_disassembly: bool,
    pub dump_cssjit_disassembly: bool,
    pub dump_reg_exp_disassembly: bool,
    pub trace_reg_exp_jit_execution: bool,
    pub verify_reg_exp_jit_reads: bool,
    pub dump_wasm_disassembly: bool,
    pub dump_wasm_source_file_name: Option<String>,
    pub wasm_omg_functions_to_dump: Option<String>,
    pub dump_bbq_disassembly: bool,
    pub dump_omg_disassembly: bool,
    pub use_jit_dump: bool,
    pub use_gdb_jit_info: bool,
    pub use_text_markers: bool,
    pub jit_dump_directory: Option<String>,
    pub use_ir_dump: bool,
    pub ir_dump_directory: Option<String>,
    pub use_source_code_dump: bool,
    pub source_code_dump_directory: Option<String>,
    pub text_markers_directory: Option<String>,
    pub bytecode_range_to_jit_compile: OptionRange,
    pub bytecode_range_to_dfg_compile: OptionRange,
    pub bytecode_range_to_ftl_compile: OptionRange,
    pub jit_allowlist: Option<String>,
    pub dfg_allowlist: Option<String>,
    pub ftl_allowlist: Option<String>,
    pub bbq_allowlist: Option<String>,
    pub omg_allowlist: Option<String>,
    pub loop_unrolling_allowlist: Option<String>,
    pub dump_graph_allowlist: Option<String>,
    pub dump_source_at_dfg_time: bool,
    pub dump_bytecode_at_dfg_time: bool,
    pub dump_graph_after_parsing: bool,
    pub dump_graph_at_each_phase: bool,
    pub dump_dfg_graph_at_each_phase: bool,
    pub dump_dfgftl_graph_at_each_phase: bool,
    pub dump_b3_graph_at_each_phase: bool,
    pub dump_air_graph_at_each_phase: bool,
    pub verbose_dfg_bytecode_parsing: bool,
    pub safepoint_before_each_phase: bool,
    pub verbose_compilation: bool,
    pub verbose_ftl_compilation: bool,
    pub log_compilation_changes: bool,
    pub print_each_osr_exit: bool,
    pub print_each_dfgftl_inline_call: bool,
    pub use_jit_asserts: bool,
    pub validate_does_gc: bool,
    pub validate_graph: bool,
    pub validate_graph_at_each_phase: bool,
    pub verbose_validation_failure: bool,
    pub verbose_osr: bool,
    pub verbose_dfgosr_exit: bool,
    pub verbose_ftlosr_exit: bool,
    pub verbose_call_link: bool,
    pub verbose_compilation_queue: bool,
    pub report_compile_times: bool,
    pub report_baseline_compile_times: bool,
    pub report_dfg_compile_times: bool,
    pub report_ftl_compile_times: bool,
    pub report_total_compile_times: bool,
    pub report_total_phase_times: bool,
    pub report_parse_times: bool,
    pub report_bytecode_compile_times: bool,
    pub report_bytecode_cache_decode_times: bool,
    pub count_parse_times: bool,
    pub verbose_exit_profile: bool,
    pub verbose_cfa: bool,
    pub verbose_dfg_failure: bool,
    pub verbose_ftl_to_js_thunk: bool,
    pub verbose_ftl_failure: bool,
    pub test_the_ftl: bool,
    pub verbose_sanitize_stack: bool,
    pub use_generational_gc: bool,
    pub use_concurrent_gc: bool,
    pub collect_continuously: bool,
    pub collect_continuously_period_ms: f64,
    pub force_fenced_barrier: bool,
    pub verbose_visit_race: bool,
    pub optimize_parallel_slot_visitors_for_stopped_mutator: bool,
    pub verbose_heap_snapshot_logging: bool,
    pub large_heap_size: u32,
    pub medium_heap_size: u32,
    pub small_heap_size: u32,
    pub small_heap_ram_fraction: f64,
    pub small_heap_growth_factor: f64,
    pub medium_heap_ram_fraction: f64,
    pub medium_heap_growth_factor: f64,
    pub large_heap_growth_factor: f64,
    pub mini_vm_heap_growth_factor: f64,
    pub heap_growth_steepness_factor: f64,
    pub heap_growth_max_increase: f64,
    pub min_eden_to_old_generation_ratio: f64,
    pub heap_growth_function_threshold_in_mb: u32,
    pub critical_gc_memory_threshold: f64,
    pub custom_full_gc_callback_bail_threshold: f64,
    pub minimum_mutator_utilization: f64,
    pub maximum_mutator_utilization: f64,
    pub epsilon_mutator_utilization: f64,
    pub concurrent_gc_max_headroom: f64,
    pub concurrent_gc_period_ms: f64,
    pub use_stochastic_mutator_scheduler: bool,
    pub minimum_gc_pause_ms: f64,
    pub gc_pause_scale: f64,
    pub gc_increment_bytes: f64,
    pub gc_increment_max_bytes: f64,
    pub gc_increment_scale: f64,
    pub use_warm_up_marked_blocks: bool,
    pub warm_up_marked_block_count: u32,
    pub warm_up_marked_block_start_after_blocks: u32,
    pub warm_up_marked_block_idle_timeout: f64,
    pub scribble_free_cells: bool,
    pub decommit_unused_marked_block_pages: bool,
    pub decommit_unused_marked_block_pages_after_eden_collections: bool,
    pub size_class_progression: f64,
    pub precise_allocation_cutoff: u32,
    pub dump_size_classes: bool,
    pub steal_empty_blocks_from_other_allocators: bool,
    pub eagerly_update_top_call_frame: bool,
    pub dump_zapped_cell_crash_data: bool,
    pub use_osr_entry_to_dfg: bool,
    pub use_osr_entry_to_ftl: bool,
    pub use_ftljit: bool,
    pub validate_ftlosr_exit_liveness: bool,
    pub poison_dead_osr_exit_variables: bool,
    pub default_b3_opt_level: u32,
    pub b3_always_fails_before_compile: bool,
    pub b3_always_fails_before_link: bool,
    pub validate_serialized_value: bool,
    pub ftl_crashes: bool,
    pub clobber_all_regs_in_ftlic_slow_path: bool,
    pub use_jit_debug_assertions: bool,
    pub use_access_inlining: bool,
    pub max_access_variant_list_size: u32,
    pub threshold_for_undesired_megamorphic_access_variant_list_size: f64,
    pub use_polyvariant_devirtualization: bool,
    pub use_polymorphic_access_inlining: bool,
    pub max_polymorphic_access_inlining_list_size: u32,
    pub use_polymorphic_call_inlining: bool,
    pub use_polymorphic_call_inlining_for_non_stub_status: bool,
    pub max_polymorphic_call_variant_list_size: u32,
    pub max_polymorphic_call_variant_list_size_for_top_tier: u32,
    pub max_polymorphic_call_variant_list_size_for_wasm_to_js: u32,
    pub max_polymorphic_call_variants_for_inlining: u32,
    pub frequent_call_threshold: u32,
    pub minimum_call_to_known_rate: f64,
    pub create_pre_headers: bool,
    pub use_mov_hint_removal: bool,
    pub use_put_stack_sinking: bool,
    pub use_object_allocation_sinking: bool,
    pub verbose_object_allocation_sinking: bool,
    pub use_value_rep_elimination: bool,
    pub use_arity_fixup_inlining: bool,
    pub log_executable_allocation: bool,
    pub max_dfg_nodes_in_basic_block_for_precise_analysis: u32,
    pub use_concurrent_jit: bool,
    pub min_number_of_worklist_threads: u32,
    pub max_number_of_worklist_threads: u32,
    pub number_of_baseline_compiler_threads: u32,
    pub number_of_dfg_compiler_threads: u32,
    pub number_of_ftl_compiler_threads: u32,
    pub number_of_wasm_compiler_threads: u32,
    pub worklist_load_factor: u32,
    pub worklist_baseline_load_weight: u32,
    pub worklist_dfg_load_weight: u32,
    pub worklist_ftl_load_weight: u32,
    pub priority_delta_of_dfg_compiler_threads: i32,
    pub priority_delta_of_ftl_compiler_threads: i32,
    pub priority_delta_of_wasm_compiler_threads: i32,
    pub use_profiler: bool,
    pub dump_profiler_data_at_exit: bool,
    pub disassemble_baseline_for_profiler: bool,
    pub abbreviate_source_code_for_profiler: u32,
    pub use_architecture_specific_optimizations: bool,
    pub break_on_throw: bool,
    pub maximum_optimization_candidate_bytecode_cost: u32,
    pub maximum_cached_assembler_buffer_size: u32,
    pub maximum_function_for_call_inline_candidate_bytecode_cost_for_dfg: u32,
    pub maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_dfg: u32,
    pub maximum_function_for_construct_inline_candidate_bytecode_cost_for_dfg: u32,
    pub maximum_function_for_call_inline_candidate_bytecode_cost_for_ftl: u32,
    pub maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_ftl: u32,
    pub maximum_function_for_construct_inline_candidate_bytecode_cost_for_ftl: u32,
    pub maximum_ftl_candidate_bytecode_cost: u32,
    pub ratio_ftl_nodes_to_bytecode_cost: f64,
    pub maximum_inlining_depth: u32,
    pub maximum_inlining_recursion: u32,
    pub maximum_inlining_caller_bytecode_cost: u32,
    pub use_global_inlining_planner: bool,
    pub global_inlining_plan_budget_for_dfg: u32,
    pub global_inlining_plan_budget_for_ftl: u32,
    pub maximum_global_inlining_plan_sites: u32,
    pub inlining_plan_tier_bonus_base: f64,
    pub inlining_plan_tier_bonus_power_for_ftl: f64,
    pub inlining_plan_tier_bonus_power_for_dfg: f64,
    pub inlining_plan_tier_bonus_power_for_baseline: f64,
    pub inlining_plan_depth_penalty: f64,
    pub maximum_varargs_for_inlining: u32,
    pub maximum_binary_string_switch_case_length: u32,
    pub maximum_binary_string_switch_total_length: u32,
    pub maximum_inline_string_switch_case_count: u32,
    pub maximum_reg_exp_test_inline_codesize: u32,
    pub maximum_reg_exp_jit_code_size: u32,
    pub wasm_inlining_maximum_depth: u32,
    pub wasm_inlining_maximum_wasm_callee_size: u32,
    pub wasm_inlining_maximum_count: u32,
    pub wasm_inlining_minimum_budget: u32,
    pub wasm_inlining_factor: u32,
    pub wasm_inlining_budget: u32,
    pub wasm_inlining_large_function_growth_factor: f64,
    pub wasm_inlining_tiny_function_threshold: u32,
    pub wasm_inlining_small_function_threshold: u32,
    pub jit_policy_scale: f64,
    pub number_of_super_and_performance_cores_override: i32,
    pub dfg_threshold_scale_for_few_performance_cores: f64,
    pub ftl_threshold_scale_for_few_performance_cores: f64,
    pub force_eager_compilation: bool,
    pub threshold_for_jit_after_warm_up: i32,
    pub threshold_for_jit_soon: i32,
    pub threshold_for_optimize_after_warm_up: i32,
    pub threshold_for_optimize_after_long_warm_up: i32,
    pub threshold_for_optimize_soon: i32,
    pub execution_counter_increment_for_loop: i32,
    pub execution_counter_increment_for_entry: i32,
    pub threshold_for_ftl_optimize_after_warm_up: i32,
    pub threshold_for_ftl_optimize_soon: i32,
    pub ftl_tier_up_counter_increment_for_loop: i32,
    pub ftl_tier_up_counter_increment_for_return: i32,
    pub ftl_osr_entry_failure_count_for_reoptimization: u32,
    pub ftl_osr_entry_retry_threshold: u32,
    pub eval_threshold_multiplier: i32,
    pub maximum_eval_cacheable_source_length: u32,
    pub maximum_execution_counts_between_checkpoints_for_baseline: i32,
    pub maximum_execution_counts_between_checkpoints_for_upper_tiers: i32,
    pub high_cost_baseline_profiling_function_bytecode_cost: i32,
    pub value_profile_filling_rate_monitoring_bytecode_cost: i32,
    pub likely_to_take_slow_case_minimum_count: u32,
    pub could_take_slow_case_minimum_count: u32,
    pub osr_exit_count_for_reoptimization: u32,
    pub osr_exit_count_for_reoptimization_from_loop: u32,
    pub reoptimization_retry_counter_max: u32,
    pub minimum_optimization_delay: u32,
    pub maximum_optimization_delay: u32,
    pub desired_profile_liveness_rate: f64,
    pub desired_profile_fullness_rate: f64,
    pub quick_dfg_tier_up_threshold_factor: f64,
    pub relaxed_profile_coverage_factor_for_quick_dfg_tier_up: f64,
    pub quick_ftl_tier_up_threshold_factor: f64,
    pub double_vote_ratio_for_double_format: f64,
    pub structure_check_vote_ratio_for_hoisting: f64,
    pub check_array_vote_ratio_for_hoisting: f64,
    pub maximum_direct_call_stack_size: u32,
    pub minimum_number_of_scans_between_rebalance: u32,
    pub number_of_gc_markers: u32,
    pub use_parallel_marking_constraint_solver: bool,
    pub opaque_root_merge_threshold: u32,
    pub max_heap_size_as_ram_size_multiple: u32,
    pub min_heap_utilization: f64,
    pub min_marked_block_utilization: f64,
    pub slow_path_allocs_between_g_cs: u32,
    pub max_reg_exp_stack_size: u32,
    pub percent_cpu_per_mb_for_full_timer: f64,
    pub percent_cpu_per_mb_for_eden_timer: f64,
    pub collection_timer_max_percent_cpu: f64,
    pub force_weak_random_seed: bool,
    pub forced_weak_random_seed: u32,
    pub always_have_a_bad_time: bool,
    pub allow_double_shape: bool,
    pub use_zombie_mode: bool,
    pub use_immortal_objects: bool,
    pub sweep_synchronously: bool,
    pub max_single_allocation_size: u32,
    pub log_gc: GCLogLevel,
    pub use_gc: bool,
    pub use_global_gc: bool,
    pub gc_at_end: bool,
    pub force_gc_slow_paths: bool,
    pub force_did_defer_gc_work: bool,
    pub gc_max_heap_size: u32,
    pub force_ram_size: usize,
    pub record_gc_pause_times: bool,
    pub dump_heap_statistics_at_vm_destruction: bool,
    pub enable_strong_ref_tracker: bool,
    pub dump_heap_on_low_memory: bool,
    pub force_code_block_to_jettison_due_to_old_age: bool,
    pub use_eager_code_block_jettison_timing: bool,
    pub use_execution_count_for_code_block_aging: bool,
    pub optimized_code_aging_quiet_allocation_mb: u32,
    pub optimized_code_aging_quiet_seconds: f64,
    pub code_block_aging_lease_multiplier: f64,
    pub use_lean_bytecode_cache_decoder: bool,
    pub use_borrowed_bytecode_from_cache: bool,
    pub disk_cache_payload_is_persistent_for_testing: bool,
    pub verify_bytecode_cache_checksums: bool,
    pub use_type_profiler: bool,
    pub use_control_flow_profiler: bool,
    pub use_sampling_profiler: bool,
    pub sample_interval: u32,
    pub collect_extra_sampling_profiler_data: bool,
    pub sampling_profiler_top_functions_count: u32,
    pub sampling_profiler_top_bytecodes_count: u32,
    pub sampling_profiler_ignore_external_source_id: bool,
    pub sampling_profiler_path: Option<String>,
    pub sample_c_code: bool,
    pub always_generate_pc_to_code_origin_map: bool,
    pub random_integrity_audit_rate: f64,
    pub verify_gc: bool,
    pub verbose_verify_gc: bool,
    pub verify_heap: bool,
    pub number_of_gc_cycles_to_record_for_verification: u32,
    pub exception_stack_trace_limit: u32,
    pub default_error_stack_trace_limit: u32,
    pub exit_on_resource_exhaustion: bool,
    pub use_exception_fuzz: bool,
    pub fire_exception_fuzz_at: u32,
    pub fuzz_atomic_jit_memcpy: bool,
    pub validate_dfg_exception_handling: bool,
    pub dump_simulated_throws: bool,
    pub validate_exception_checks: bool,
    pub unexpected_exception_stack_trace_limit: u32,
    pub validate_dfg_clobberize: bool,
    pub validate_bounds_check_elimination: bool,
    pub validate_dfg_may_exit: bool,
    pub validate_vm_entry_callee_saves: bool,
    pub use_executable_allocation_fuzz: bool,
    pub fire_executable_allocation_fuzz_at: u32,
    pub fire_executable_allocation_fuzz_at_or_after: u32,
    pub fire_executable_allocation_fuzz_randomly: bool,
    pub fire_executable_allocation_fuzz_randomly_probability: f64,
    pub verbose_executable_allocation_fuzz: bool,
    pub zero_executable_memory_on_free: bool,
    pub use_osr_exit_fuzz: bool,
    pub fire_osr_exit_fuzz_at_static: u32,
    pub fire_osr_exit_fuzz_at: u32,
    pub fire_osr_exit_fuzz_at_or_after: u32,
    pub verbose_osr_exit_fuzz: bool,
    pub use_loljit: bool,
    pub verbose_lol_allocation: bool,
    pub seed_of_vm_random_for_fuzzer: u32,
    pub use_randomizing_fuzzer_agent: bool,
    pub seed_of_randomizing_fuzzer_agent: u32,
    pub dump_fuzzer_agent_predictions: bool,
    pub use_double_prediction_fuzzer_agent: bool,
    pub use_file_based_fuzzer_agent: bool,
    pub use_prediction_file_creating_fuzzer_agent: bool,
    pub require_prediction_for_file_based_fuzzer_agent: bool,
    pub fuzzer_predictions_file: Option<String>,
    pub use_narrowing_number_prediction_fuzzer_agent: bool,
    pub use_widening_number_prediction_fuzzer_agent: bool,
    pub log_phase_times: bool,
    pub rare_block_penalty: f64,
    pub air_greedy_reg_alloc_verbose: bool,
    pub air_greedy_reg_alloc_dump_function: Option<String>,
    pub air_greedy_reg_alloc_split_multiplier: f64,
    pub air_greedy_reg_alloc_split_around_loops: bool,
    pub air_greedy_reg_alloc_loop_split_max_loop_fraction: f64,
    pub air_greedy_reg_alloc_spills_everything: bool,
    pub air_dump_phase_stats: bool,
    pub air_validate_greed_reg_alloc: bool,
    pub air_randomize_regs: bool,
    pub air_randomize_regs_seed: u32,
    pub coalesce_spill_slots: bool,
    pub log_air_register_pressure: bool,
    pub use_b3_tail_dup: bool,
    pub max_b3_tail_dup_block_size: u32,
    pub max_b3_tail_dup_block_successors: u32,
    pub use_b3_hoist_loop_invariant_values: bool,
    pub use_b3_canonicalize_pre_post_increments: bool,
    pub use_b3_eliminate_wasm_gc_allocations: bool,
    pub use_b3_reduce_strength_fixpoint: bool,
    pub use_air_optimize_paired_load_store: bool,
    pub use_dollar_vm: bool,
    pub function_overrides: Option<String>,
    pub watchdog: u32,
    pub use_polling_traps: bool,
    pub force_trap_aware_stack_checks: bool,
    pub use_mach_for_exceptions: bool,
    pub allow_non_sp_tagging: bool,
    pub use_ic_stats: bool,
    pub use_fuzzer_mode: bool,
    pub prototype_hit_count_for_ll_int_caching: u32,
    pub dump_compiled_reg_exp_patterns: bool,
    pub verbose_reg_exp_compilation: bool,
    pub dump_module_record: bool,
    pub dump_module_loading_state: bool,
    pub expose_internal_module_loader: bool,
    pub expose_private_identifiers: bool,
    pub use_super_sampler: bool,
    pub use_source_provider_cache: bool,
    pub use_code_cache: bool,
    pub use_wasm: bool,
    pub fail_to_compile_wasm_code: bool,
    pub wasm_small_partial_compile_limit: usize,
    pub wasm_large_partial_compile_limit: usize,
    pub wasm_omg_optimization_level: u32,
    pub use_wasm_byte_loop_replacement: bool,
    pub use_bbq_tier_up_checks: bool,
    pub use_wasm_osr: bool,
    pub threshold_for_bbq_optimize_after_warm_up: i32,
    pub threshold_for_bbq_optimize_soon: i32,
    pub threshold_for_omg_optimize_after_warm_up: i32,
    pub threshold_for_omg_optimize_soon: i32,
    pub maximum_omg_candidate_cost: u32,
    pub omg_tier_up_counter_increment_for_loop: i32,
    pub omg_tier_up_counter_increment_for_entry: i32,
    pub wasm_omg_entry_increment_size_reference: i32,
    pub use_wasm_fast_memory: bool,
    pub log_wasm_memory: bool,
    pub wasm_fast_memory_redzone_pages: u32,
    pub crash_if_wasm_cant_fast_memory: bool,
    pub crash_on_failed_wasm_validate: bool,
    pub max_num_wasm_fast_memories: u32,
    pub verbose_bbqjit_allocation: bool,
    pub verbose_bbqjit_instructions: bool,
    pub disable_bbq_consts: bool,
    pub use_bbqjit: bool,
    pub use_omgjit: bool,
    pub wasm_function_index_range_to_compile: OptionRange,
    pub use_eager_wasm_module_hashing: bool,
    pub use_array_allocation_profiling: bool,
    pub force_poly_proto: bool,
    pub force_mini_vm_mode: bool,
    pub use_trace_points: bool,
    pub use_compiler_signpost: bool,
    pub use_gc_signpost: bool,
    pub trace_ll_int_execution: bool,
    pub trace_ll_int_slow_path: bool,
    pub trace_baseline_jit_execution: bool,
    pub threshold_for_global_lexical_binding_epoch: u32,
    pub disk_cache_path: Option<String>,
    pub verbose_disk_cache: bool,
    pub force_disk_cache: bool,
    pub validate_abstract_interpreter_state: bool,
    pub validate_abstract_interpreter_state_probability: f64,
    pub dump_jit_memory_path: Option<String>,
    pub dump_jit_memory_flush_interval: f64,
    pub use_unlinked_code_block_jettisoning: bool,
    pub force_osr_exit_to_ll_int: bool,
    pub get_by_val_ic_max_number_of_identifiers: u32,
    pub use_randomizing_executable_island_allocation: bool,
    pub expose_profilers_on_global_object: bool,
    pub allow_unsupported_tiers: bool,
    pub return_early_from_infinite_loops_for_fuzzing: bool,
    pub early_return_from_infinite_loops_limit: usize,
    pub use_licm_fuzzing: bool,
    pub seed_for_licm_fuzzer: u32,
    pub allow_hoisting_licm_probability: f64,
    pub expose_custom_setters_on_global_object_for_testing: bool,
    pub use_jit_cage: bool,
    pub use_allocation_profiling: bool,
    pub allocation_profiling_mode: u32,
    pub dump_baseline_jit_size_statistics: bool,
    pub dump_dfgjit_size_statistics: bool,
    pub use_loop_unrolling: bool,
    pub use_partial_loop_unrolling: bool,
    pub verbose_loop_unrolling: bool,
    pub disallow_loop_unrolling_for_non_innermost: bool,
    pub max_loop_unrolling_count: u32,
    pub max_loop_unrolling_body_node_size: u32,
    pub max_loop_unrolling_iteration_count: u32,
    pub max_partial_loop_unrolling_body_node_size: u32,
    pub max_partial_loop_unrolling_iteration_count: u32,
    pub max_numeric_hot_loop_size: u32,
    pub max_integer_range_optimization_relationships_per_node: u32,
    pub max_integer_range_optimization_work: u32,
    pub print_each_unrolled_loop: bool,
    pub verbose_executable_pool_allocation: bool,
    pub use_handler_ic_in_ftl: bool,
    pub use_ll_int_i_cs: bool,
    pub use_baseline_jit_code_sharing: bool,
    pub libpas_scavenge_continuously: bool,
    pub libpas_force_pgm_with_rate: u32,
    pub use_wasm_fault_signal_handler: bool,
    pub dump_unlinked_dfg_validation: bool,
    pub dump_wasm_opcode_statistics: bool,
    pub dump_wasm_warnings: bool,
    pub use_recursive_json_parse: bool,
    pub threshold_for_string_replace_cache: u32,
    pub use_wasm_ip_int: bool,
    pub use_wasm_ip_int_prologue_osr: bool,
    pub use_wasm_ip_int_loop_osr: bool,
    pub use_wasm_ip_int_epilogue_osr: bool,
    pub use_wasm_ip_int_simd: bool,
    pub trace_wasm_ip_int_execution: bool,
    pub force_all_functions_to_use_simd: bool,
    pub use_omg_inlining: bool,
    pub free_retired_wasm_code: bool,
    pub use_array_allocation_sinking: bool,
    pub dump_ftl_code_size: bool,
    pub dump_optimization_tracing: bool,
    pub dump_ion_graph: bool,
    pub ion_graph_directory: Option<String>,
    pub marked_block_dump_info_count: u32,
    pub use_async_stack_trace: bool,
    pub use_big_int_math_methods: bool,
    pub use_explicit_resource_management: bool,
    pub use_import_defer: bool,
    pub use_import_text: bool,
    pub use_iterator_chunking: bool,
    pub use_iterator_includes: bool,
    pub use_iterator_join: bool,
    pub use_iterator_sequencing: bool,
    pub use_json_source_text_access: bool,
    pub use_jspi: bool,
    pub use_joint_iteration: bool,
    pub use_more_currency_display_choices: bool,
    pub use_promise_is_promise: bool,
    pub use_reg_exp_buffer_boundaries: bool,
    pub use_shadow_realm: bool,
    pub use_temporal: bool,
    pub use_wasm_js_string_builtins: bool,
    pub use_wasm_js_types: bool,
    pub use_wasm_memory64: bool,
    pub use_wasm_memory_to_buffer_ap_is: bool,
    pub use_wasm_multi_memory: bool,
    pub use_wasm_relaxed_simd: bool,
    pub use_wasm_simd: bool,
    pub use_wasm_tail_calls: bool,
    pub use_wasm_wide_arithmetic: bool,
    pub disallow_mixed_wasm_exceptions: bool,
    pub use_shared_array_buffer: bool,
    pub use_trusted_types: bool,
}

impl Default for Options {
    /// Os padrões do `FOR_EACH_JSC_OPTION`, na ordem da lista (um padrão pode ler opção anterior).
    fn default() -> Self {
        let use_kern_tcsm: bool = default_tcsm_value();
        let validate_options: bool = false;
        let dump_options: u32 = 0;
        let config_file: Option<String> = None;
        let use_ll_int: bool = true;
        let use_jit: bool = jit_enabled_by_default();
        let use_baseline_jit: bool = true;
        let use_dfgjit: bool = jit_enabled_by_default();
        let use_reg_exp_jit: bool = jit_enabled_by_default();
        let use_domjit: bool = jit_enabled_by_default();
        let use_reg_exp_lookbehind_jit: bool = true;
        let use_reg_exp_alternation_factoring: bool = true;
        let use_reg_exp_alternation_dispatch: bool = true;
        let reg_exp_dispatch_max_inline_literal_length: u32 = 32;
        let report_must_succeed_executable_allocations: bool = false;
        // O Bun liga a opção ao criar o global object (ZigGlobalObject.cpp:316: Options::useV8DateParser() = true).
        let use_v8_date_parser: bool = true;
        let show_private_scripts_in_stack_traces: bool = false;
        let eval_mode: bool = false;
        let use_ffiic_stub: bool = true;
        let use_ffi_call_in_dfg: bool = true;
        let use_ffi_direct_call: bool = true;
        let dump_ffi_disassembly: bool = false;
        let verbose_ffi: bool = false;
        let max_per_thread_stack_usage: u32 = 5 * 1048576;
        let soft_reserved_zone_size: u32 = 128 * 1024;
        let reserved_zone_size: u32 = 64 * 1024;
        let crash_on_disallowed_vm_entry: bool = assert_enabled();
        let crash_if_cant_allocate_jit_memory: bool = false;
        let structure_heap_size_in_kb: u32 = 0;
        let jit_memory_reservation_size: u32 = 0;
        let jit_memory_reservation_address: usize = 0;
        let force_code_block_liveness: bool = false;
        let force_ic_failure: bool = false;
        let force_unlinked_dfg: bool = false;
        let repatch_count_for_cool_down: u32 = 8;
        let initial_cool_down_count: u32 = 20;
        let repatch_buffering_countdown: u32 = 6;
        let initial_repatch_buffering_countdown: u32 = 6;
        let dump_generated_bytecodes: bool = false;
        let dump_bytecode_liveness_results: bool = false;
        let validate_bytecode: bool = false;
        let force_debugger_bytecode_generation: bool = false;
        let debugger_triggers_breakpoint_exception: bool = false;
        let verbose_wasm_debugger: bool = false;
        let enable_wasm_debugger: bool = false;
        let verbose_wasm_type_cleanup: bool = false;
        let dump_bytecodes_before_generatorification: bool = false;
        let switch_jump_table_amount_threshold: u32 = 15;
        let use_function_dot_arguments: bool = true;
        let use_tail_calls: bool = true;
        let optimize_recursive_tail_calls: bool = true;
        let always_use_shadow_chicken: bool = false;
        let shadow_chicken_log_size: u32 = 1000;
        let shadow_chicken_max_tail_deleted_frames_size: u32 = 128;
        let use_os_log: OSLogType = OSLogType::None;
        let need_disassembly_support: bool = false;
        let dump_disassembly: bool = false;
        let log_jit: bool = false;
        let dump_baseline_disassembly: bool = false;
        let dump_dfg_disassembly: bool = false;
        let dump_ftl_disassembly: bool = false;
        let dump_cssjit_disassembly: bool = false;
        let dump_reg_exp_disassembly: bool = false;
        let trace_reg_exp_jit_execution: bool = false;
        let verify_reg_exp_jit_reads: bool = false;
        let dump_wasm_disassembly: bool = false;
        let dump_wasm_source_file_name: Option<String> = None;
        let wasm_omg_functions_to_dump: Option<String> = None;
        let dump_bbq_disassembly: bool = false;
        let dump_omg_disassembly: bool = false;
        let use_jit_dump: bool = false;
        let use_gdb_jit_info: bool = false;
        let use_text_markers: bool = false;
        let jit_dump_directory: Option<String> = None;
        let use_ir_dump: bool = false;
        let ir_dump_directory: Option<String> = None;
        let use_source_code_dump: bool = false;
        let source_code_dump_directory: Option<String> = None;
        let text_markers_directory: Option<String> = None;
        let bytecode_range_to_jit_compile: OptionRange = OptionRange::default();
        let bytecode_range_to_dfg_compile: OptionRange = OptionRange::default();
        let bytecode_range_to_ftl_compile: OptionRange = OptionRange::default();
        let jit_allowlist: Option<String> = None;
        let dfg_allowlist: Option<String> = None;
        let ftl_allowlist: Option<String> = None;
        let bbq_allowlist: Option<String> = None;
        let omg_allowlist: Option<String> = None;
        let loop_unrolling_allowlist: Option<String> = None;
        let dump_graph_allowlist: Option<String> = None;
        let dump_source_at_dfg_time: bool = false;
        let dump_bytecode_at_dfg_time: bool = false;
        let dump_graph_after_parsing: bool = false;
        let dump_graph_at_each_phase: bool = false;
        let dump_dfg_graph_at_each_phase: bool = false;
        let dump_dfgftl_graph_at_each_phase: bool = false;
        let dump_b3_graph_at_each_phase: bool = false;
        let dump_air_graph_at_each_phase: bool = false;
        let verbose_dfg_bytecode_parsing: bool = false;
        let safepoint_before_each_phase: bool = true;
        let verbose_compilation: bool = false;
        let verbose_ftl_compilation: bool = false;
        let log_compilation_changes: bool = false;
        let print_each_osr_exit: bool = false;
        let print_each_dfgftl_inline_call: bool = false;
        let use_jit_asserts: bool = assert_enabled();
        let validate_does_gc: bool = assert_enabled();
        let validate_graph: bool = false;
        let validate_graph_at_each_phase: bool = false;
        let verbose_validation_failure: bool = false;
        let verbose_osr: bool = false;
        let verbose_dfgosr_exit: bool = false;
        let verbose_ftlosr_exit: bool = false;
        let verbose_call_link: bool = false;
        let verbose_compilation_queue: bool = false;
        let report_compile_times: bool = false;
        let report_baseline_compile_times: bool = false;
        let report_dfg_compile_times: bool = false;
        let report_ftl_compile_times: bool = false;
        let report_total_compile_times: bool = false;
        let report_total_phase_times: bool = false;
        let report_parse_times: bool = false;
        let report_bytecode_compile_times: bool = false;
        let report_bytecode_cache_decode_times: bool = false;
        let count_parse_times: bool = false;
        let verbose_exit_profile: bool = false;
        let verbose_cfa: bool = false;
        let verbose_dfg_failure: bool = false;
        let verbose_ftl_to_js_thunk: bool = false;
        let verbose_ftl_failure: bool = false;
        let test_the_ftl: bool = false;
        let verbose_sanitize_stack: bool = false;
        let use_generational_gc: bool = true;
        let use_concurrent_gc: bool = true;
        let collect_continuously: bool = false;
        let collect_continuously_period_ms: f64 = 1.0;
        let force_fenced_barrier: bool = false;
        let verbose_visit_race: bool = false;
        let optimize_parallel_slot_visitors_for_stopped_mutator: bool = false;
        let verbose_heap_snapshot_logging: bool = true;
        let large_heap_size: u32 = 32 * 1024 * 1024;
        let medium_heap_size: u32 = 4 * 1024 * 1024;
        let small_heap_size: u32 = 1 * 1024 * 1024;
        let small_heap_ram_fraction: f64 = 0.25;
        let small_heap_growth_factor: f64 = 2.0;
        let medium_heap_ram_fraction: f64 = 0.5;
        let medium_heap_growth_factor: f64 = 1.5;
        let large_heap_growth_factor: f64 = 1.24;
        let mini_vm_heap_growth_factor: f64 = 1.20;
        let heap_growth_steepness_factor: f64 = 2.00;
        let heap_growth_max_increase: f64 = 3.00;
        let min_eden_to_old_generation_ratio: f64 = 1.0 / 3.0;
        let heap_growth_function_threshold_in_mb: u32 = 16 * 1024;
        let critical_gc_memory_threshold: f64 = 0.80;
        let custom_full_gc_callback_bail_threshold: f64 = -1.0;
        let minimum_mutator_utilization: f64 = 0.0;
        let maximum_mutator_utilization: f64 = 0.7;
        let epsilon_mutator_utilization: f64 = 0.01;
        let concurrent_gc_max_headroom: f64 = 1.5;
        let concurrent_gc_period_ms: f64 = 2.0;
        let use_stochastic_mutator_scheduler: bool = true;
        let minimum_gc_pause_ms: f64 = 0.3;
        let gc_pause_scale: f64 = 0.3;
        let gc_increment_bytes: f64 = 10000.0;
        let gc_increment_max_bytes: f64 = 100000.0;
        let gc_increment_scale: f64 = 0.0;
        let use_warm_up_marked_blocks: bool = true;
        let warm_up_marked_block_count: u32 = 32;
        let warm_up_marked_block_start_after_blocks: u32 = 64;
        let warm_up_marked_block_idle_timeout: f64 = 10.0;
        let scribble_free_cells: bool = false;
        let decommit_unused_marked_block_pages: bool = true;
        let decommit_unused_marked_block_pages_after_eden_collections: bool = false;
        let size_class_progression: f64 = 1.4;
        let precise_allocation_cutoff: u32 = 100000;
        let dump_size_classes: bool = false;
        let steal_empty_blocks_from_other_allocators: bool = true;
        let eagerly_update_top_call_frame: bool = false;
        let dump_zapped_cell_crash_data: bool = false;
        let use_osr_entry_to_dfg: bool = true;
        let use_osr_entry_to_ftl: bool = true;
        let use_ftljit: bool = true;
        let validate_ftlosr_exit_liveness: bool = false;
        let poison_dead_osr_exit_variables: bool = assert_enabled();
        let default_b3_opt_level: u32 = 2;
        let b3_always_fails_before_compile: bool = false;
        let b3_always_fails_before_link: bool = false;
        let validate_serialized_value: bool = false;
        let ftl_crashes: bool = false;
        let clobber_all_regs_in_ftlic_slow_path: bool = assert_enabled();
        let use_jit_debug_assertions: bool = assert_enabled();
        let use_access_inlining: bool = true;
        let max_access_variant_list_size: u32 = 8;
        let threshold_for_undesired_megamorphic_access_variant_list_size: f64 = 0.5;
        let use_polyvariant_devirtualization: bool = true;
        let use_polymorphic_access_inlining: bool = true;
        let max_polymorphic_access_inlining_list_size: u32 = 8;
        let use_polymorphic_call_inlining: bool = true;
        let use_polymorphic_call_inlining_for_non_stub_status: bool = false;
        let max_polymorphic_call_variant_list_size: u32 = 8;
        let max_polymorphic_call_variant_list_size_for_top_tier: u32 = 5;
        let max_polymorphic_call_variant_list_size_for_wasm_to_js: u32 = 5;
        let max_polymorphic_call_variants_for_inlining: u32 = 5;
        let frequent_call_threshold: u32 = 2;
        let minimum_call_to_known_rate: f64 = 0.51;
        let create_pre_headers: bool = true;
        let use_mov_hint_removal: bool = true;
        let use_put_stack_sinking: bool = true;
        let use_object_allocation_sinking: bool = true;
        let verbose_object_allocation_sinking: bool = false;
        let use_value_rep_elimination: bool = true;
        let use_arity_fixup_inlining: bool = true;
        let log_executable_allocation: bool = false;
        let max_dfg_nodes_in_basic_block_for_precise_analysis: u32 = 20000;
        let use_concurrent_jit: bool = true;
        let min_number_of_worklist_threads: u32 = compute_number_of_worker_threads(3, 2);
        let max_number_of_worklist_threads: u32 = compute_number_of_worker_threads(3, 2);
        let number_of_baseline_compiler_threads: u32 = compute_number_of_worker_threads(3, 2);
        let number_of_dfg_compiler_threads: u32 = compute_number_of_worker_threads(3, 2) - 1;
        let number_of_ftl_compiler_threads: u32 = compute_number_of_worker_threads(MAXIMUM_NUMBER_OF_FTL_COMPILER_THREADS, 2) - 1;
        let number_of_wasm_compiler_threads: u32 = compute_number_of_worker_threads(i32::MAX, 2) - 1;
        let worklist_load_factor: u32 = 1;
        let worklist_baseline_load_weight: u32 = 1;
        let worklist_dfg_load_weight: u32 = 1;
        let worklist_ftl_load_weight: u32 = 1;
        let priority_delta_of_dfg_compiler_threads: i32 = compute_priority_delta_of_worker_threads(-1, 0);
        let priority_delta_of_ftl_compiler_threads: i32 = compute_priority_delta_of_worker_threads(-2, 0);
        let priority_delta_of_wasm_compiler_threads: i32 = compute_priority_delta_of_worker_threads(-1, 0);
        let use_profiler: bool = false;
        let dump_profiler_data_at_exit: bool = false;
        let disassemble_baseline_for_profiler: bool = true;
        let abbreviate_source_code_for_profiler: u32 = 0;
        let use_architecture_specific_optimizations: bool = true;
        let break_on_throw: bool = false;
        let maximum_optimization_candidate_bytecode_cost: u32 = 100000;
        let maximum_cached_assembler_buffer_size: u32 = 1 * 1048576;
        let maximum_function_for_call_inline_candidate_bytecode_cost_for_dfg: u32 = 80;
        let maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_dfg: u32 = 80;
        let maximum_function_for_construct_inline_candidate_bytecode_cost_for_dfg: u32 = 80;
        let maximum_function_for_call_inline_candidate_bytecode_cost_for_ftl: u32 = 170;
        let maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_ftl: u32 = 100;
        let maximum_function_for_construct_inline_candidate_bytecode_cost_for_ftl: u32 = 100;
        let maximum_ftl_candidate_bytecode_cost: u32 = 60000;
        let ratio_ftl_nodes_to_bytecode_cost: f64 = 1.9;
        let maximum_inlining_depth: u32 = 5;
        let maximum_inlining_recursion: u32 = 2;
        let maximum_inlining_caller_bytecode_cost: u32 = 10000;
        let use_global_inlining_planner: bool = true;
        let global_inlining_plan_budget_for_dfg: u32 = 2500;
        let global_inlining_plan_budget_for_ftl: u32 = 12000;
        let maximum_global_inlining_plan_sites: u32 = 20000;
        let inlining_plan_tier_bonus_base: f64 = 2.0;
        let inlining_plan_tier_bonus_power_for_ftl: f64 = 3.0;
        let inlining_plan_tier_bonus_power_for_dfg: f64 = 2.0;
        let inlining_plan_tier_bonus_power_for_baseline: f64 = 1.0;
        let inlining_plan_depth_penalty: f64 = 1.5;
        let maximum_varargs_for_inlining: u32 = 100;
        let maximum_binary_string_switch_case_length: u32 = 50;
        let maximum_binary_string_switch_total_length: u32 = 2000;
        let maximum_inline_string_switch_case_count: u32 = 64;
        let maximum_reg_exp_test_inline_codesize: u32 = 500;
        let maximum_reg_exp_jit_code_size: u32 = 16 * 1048576;
        let wasm_inlining_maximum_depth: u32 = 7;
        let wasm_inlining_maximum_wasm_callee_size: u32 = 500;
        let wasm_inlining_maximum_count: u32 = 60;
        let wasm_inlining_minimum_budget: u32 = 50;
        let wasm_inlining_factor: u32 = 5;
        let wasm_inlining_budget: u32 = 6000;
        let wasm_inlining_large_function_growth_factor: f64 = 1.4;
        let wasm_inlining_tiny_function_threshold: u32 = 12;
        let wasm_inlining_small_function_threshold: u32 = 50;
        let jit_policy_scale: f64 = 1.0;
        let number_of_super_and_performance_cores_override: i32 = 0;
        let dfg_threshold_scale_for_few_performance_cores: f64 = 2.0;
        let ftl_threshold_scale_for_few_performance_cores: f64 = 1.5;
        let force_eager_compilation: bool = false;
        let threshold_for_jit_after_warm_up: i32 = 500;
        let threshold_for_jit_soon: i32 = 100;
        let threshold_for_optimize_after_warm_up: i32 = 1000;
        let threshold_for_optimize_after_long_warm_up: i32 = 1000;
        let threshold_for_optimize_soon: i32 = 1000;
        let execution_counter_increment_for_loop: i32 = 1;
        let execution_counter_increment_for_entry: i32 = 15;
        let threshold_for_ftl_optimize_after_warm_up: i32 = 64000;
        let threshold_for_ftl_optimize_soon: i32 = 1000;
        let ftl_tier_up_counter_increment_for_loop: i32 = 1;
        let ftl_tier_up_counter_increment_for_return: i32 = 15;
        let ftl_osr_entry_failure_count_for_reoptimization: u32 = 15;
        let ftl_osr_entry_retry_threshold: u32 = 100;
        let eval_threshold_multiplier: i32 = 10;
        let maximum_eval_cacheable_source_length: u32 = 256;
        let maximum_execution_counts_between_checkpoints_for_baseline: i32 = 1000;
        let maximum_execution_counts_between_checkpoints_for_upper_tiers: i32 = 30000;
        let high_cost_baseline_profiling_function_bytecode_cost: i32 = 10000;
        let value_profile_filling_rate_monitoring_bytecode_cost: i32 = 5000;
        let likely_to_take_slow_case_minimum_count: u32 = 20;
        let could_take_slow_case_minimum_count: u32 = 10;
        let osr_exit_count_for_reoptimization: u32 = 100;
        let osr_exit_count_for_reoptimization_from_loop: u32 = 5;
        let reoptimization_retry_counter_max: u32 = 0;
        let minimum_optimization_delay: u32 = 1;
        let maximum_optimization_delay: u32 = 5;
        let desired_profile_liveness_rate: f64 = 0.75;
        let desired_profile_fullness_rate: f64 = 0.35;
        let quick_dfg_tier_up_threshold_factor: f64 = default_quick_dfg_tier_up_threshold_factor();
        let relaxed_profile_coverage_factor_for_quick_dfg_tier_up: f64 = default_relaxed_profile_coverage_factor_for_quick_dfg_tier_up();
        let quick_ftl_tier_up_threshold_factor: f64 = default_quick_ftl_tier_up_threshold_factor();
        let double_vote_ratio_for_double_format: f64 = 2.0;
        let structure_check_vote_ratio_for_hoisting: f64 = 1.0;
        let check_array_vote_ratio_for_hoisting: f64 = 1.0;
        let maximum_direct_call_stack_size: u32 = 200;
        let minimum_number_of_scans_between_rebalance: u32 = 100;
        let number_of_gc_markers: u32 = compute_number_of_gc_markers(8);
        let use_parallel_marking_constraint_solver: bool = true;
        let opaque_root_merge_threshold: u32 = 1000;
        let max_heap_size_as_ram_size_multiple: u32 = 0;
        let min_heap_utilization: f64 = 0.8;
        let min_marked_block_utilization: f64 = 0.9;
        let slow_path_allocs_between_g_cs: u32 = 0;
        let max_reg_exp_stack_size: u32 = 192 * 1048576;
        let percent_cpu_per_mb_for_full_timer: f64 = 0.0003125;
        let percent_cpu_per_mb_for_eden_timer: f64 = 0.0025;
        let collection_timer_max_percent_cpu: f64 = 0.10;
        let force_weak_random_seed: bool = false;
        let forced_weak_random_seed: u32 = 0;
        let always_have_a_bad_time: bool = false;
        let allow_double_shape: bool = true;
        let use_zombie_mode: bool = false;
        let use_immortal_objects: bool = false;
        let sweep_synchronously: bool = false;
        let max_single_allocation_size: u32 = 0;
        let log_gc: GCLogLevel = GCLogLevel::None;
        let use_gc: bool = true;
        let use_global_gc: bool = false;
        let gc_at_end: bool = false;
        let force_gc_slow_paths: bool = false;
        let force_did_defer_gc_work: bool = false;
        let gc_max_heap_size: u32 = 0;
        let force_ram_size: usize = 0;
        let record_gc_pause_times: bool = false;
        let dump_heap_statistics_at_vm_destruction: bool = false;
        let enable_strong_ref_tracker: bool = false;
        let dump_heap_on_low_memory: bool = false;
        let force_code_block_to_jettison_due_to_old_age: bool = false;
        let use_eager_code_block_jettison_timing: bool = false;
        let use_execution_count_for_code_block_aging: bool = true;
        let optimized_code_aging_quiet_allocation_mb: u32 = 1;
        let optimized_code_aging_quiet_seconds: f64 = 30.0;
        let code_block_aging_lease_multiplier: f64 = 3.0;
        let use_lean_bytecode_cache_decoder: bool = true;
        let use_borrowed_bytecode_from_cache: bool = true;
        let disk_cache_payload_is_persistent_for_testing: bool = false;
        let verify_bytecode_cache_checksums: bool = true;
        let use_type_profiler: bool = false;
        let use_control_flow_profiler: bool = false;
        let use_sampling_profiler: bool = false;
        let sample_interval: u32 = 1000;
        let collect_extra_sampling_profiler_data: bool = false;
        let sampling_profiler_top_functions_count: u32 = 12;
        let sampling_profiler_top_bytecodes_count: u32 = 40;
        let sampling_profiler_ignore_external_source_id: bool = false;
        let sampling_profiler_path: Option<String> = None;
        let sample_c_code: bool = false;
        let always_generate_pc_to_code_origin_map: bool = false;
        let random_integrity_audit_rate: f64 = 0.05;
        let verify_gc: bool = false;
        let verbose_verify_gc: bool = false;
        let verify_heap: bool = false;
        let number_of_gc_cycles_to_record_for_verification: u32 = 3;
        let exception_stack_trace_limit: u32 = 100;
        let default_error_stack_trace_limit: u32 = 100;
        let exit_on_resource_exhaustion: bool = false;
        let use_exception_fuzz: bool = false;
        let fire_exception_fuzz_at: u32 = 0;
        let fuzz_atomic_jit_memcpy: bool = false;
        let validate_dfg_exception_handling: bool = assert_enabled();
        let dump_simulated_throws: bool = false;
        let validate_exception_checks: bool = false;
        let unexpected_exception_stack_trace_limit: u32 = 100;
        let validate_dfg_clobberize: bool = false;
        let validate_bounds_check_elimination: bool = false;
        let validate_dfg_may_exit: bool = assert_enabled();
        let validate_vm_entry_callee_saves: bool = false;
        let use_executable_allocation_fuzz: bool = false;
        let fire_executable_allocation_fuzz_at: u32 = 0;
        let fire_executable_allocation_fuzz_at_or_after: u32 = 0;
        let fire_executable_allocation_fuzz_randomly: bool = false;
        let fire_executable_allocation_fuzz_randomly_probability: f64 = 0.1;
        let verbose_executable_allocation_fuzz: bool = false;
        let zero_executable_memory_on_free: bool = false;
        let use_osr_exit_fuzz: bool = false;
        let fire_osr_exit_fuzz_at_static: u32 = 0;
        let fire_osr_exit_fuzz_at: u32 = 0;
        let fire_osr_exit_fuzz_at_or_after: u32 = 0;
        let verbose_osr_exit_fuzz: bool = true;
        let use_loljit: bool = false;
        let verbose_lol_allocation: bool = false;
        let seed_of_vm_random_for_fuzzer: u32 = 0;
        let use_randomizing_fuzzer_agent: bool = false;
        let seed_of_randomizing_fuzzer_agent: u32 = 1;
        let dump_fuzzer_agent_predictions: bool = false;
        let use_double_prediction_fuzzer_agent: bool = false;
        let use_file_based_fuzzer_agent: bool = false;
        let use_prediction_file_creating_fuzzer_agent: bool = false;
        let require_prediction_for_file_based_fuzzer_agent: bool = false;
        let fuzzer_predictions_file: Option<String> = None;
        let use_narrowing_number_prediction_fuzzer_agent: bool = false;
        let use_widening_number_prediction_fuzzer_agent: bool = false;
        let log_phase_times: bool = false;
        let rare_block_penalty: f64 = 0.001;
        let air_greedy_reg_alloc_verbose: bool = false;
        let air_greedy_reg_alloc_dump_function: Option<String> = None;
        let air_greedy_reg_alloc_split_multiplier: f64 = 2.0;
        let air_greedy_reg_alloc_split_around_loops: bool = false;
        let air_greedy_reg_alloc_loop_split_max_loop_fraction: f64 = 0.75;
        let air_greedy_reg_alloc_spills_everything: bool = false;
        let air_dump_phase_stats: bool = false;
        let air_validate_greed_reg_alloc: bool = assert_enabled();
        let air_randomize_regs: bool = false;
        let air_randomize_regs_seed: u32 = 0;
        let coalesce_spill_slots: bool = true;
        let log_air_register_pressure: bool = false;
        let use_b3_tail_dup: bool = true;
        let max_b3_tail_dup_block_size: u32 = 3;
        let max_b3_tail_dup_block_successors: u32 = 3;
        let use_b3_hoist_loop_invariant_values: bool = true;
        let use_b3_canonicalize_pre_post_increments: bool = false;
        let use_b3_eliminate_wasm_gc_allocations: bool = true;
        let use_b3_reduce_strength_fixpoint: bool = false;
        let use_air_optimize_paired_load_store: bool = true;
        let use_dollar_vm: bool = false;
        let function_overrides: Option<String> = None;
        let watchdog: u32 = 0;
        let use_polling_traps: bool = false;
        let force_trap_aware_stack_checks: bool = false;
        let use_mach_for_exceptions: bool = true;
        let allow_non_sp_tagging: bool = true;
        let use_ic_stats: bool = false;
        let use_fuzzer_mode: bool = false;
        let prototype_hit_count_for_ll_int_caching: u32 = 2;
        let dump_compiled_reg_exp_patterns: bool = false;
        let verbose_reg_exp_compilation: bool = false;
        let dump_module_record: bool = false;
        let dump_module_loading_state: bool = false;
        let expose_internal_module_loader: bool = false;
        let expose_private_identifiers: bool = false;
        let use_super_sampler: bool = false;
        let use_source_provider_cache: bool = true;
        let use_code_cache: bool = true;
        let use_wasm: bool = can_use_wasm();
        let fail_to_compile_wasm_code: bool = false;
        let wasm_small_partial_compile_limit: usize = 5000;
        let wasm_large_partial_compile_limit: usize = 20000;
        let wasm_omg_optimization_level: u32 = default_b3_opt_level;
        let use_wasm_byte_loop_replacement: bool = true;
        let use_bbq_tier_up_checks: bool = true;
        let use_wasm_osr: bool = true;
        let threshold_for_bbq_optimize_after_warm_up: i32 = 150;
        let threshold_for_bbq_optimize_soon: i32 = 50;
        let threshold_for_omg_optimize_after_warm_up: i32 = 50000;
        let threshold_for_omg_optimize_soon: i32 = 500;
        let maximum_omg_candidate_cost: u32 = 100000;
        let omg_tier_up_counter_increment_for_loop: i32 = 1;
        let omg_tier_up_counter_increment_for_entry: i32 = 15;
        let wasm_omg_entry_increment_size_reference: i32 = 128;
        let use_wasm_fast_memory: bool = true;
        let log_wasm_memory: bool = false;
        let wasm_fast_memory_redzone_pages: u32 = 128;
        let crash_if_wasm_cant_fast_memory: bool = false;
        let crash_on_failed_wasm_validate: bool = false;
        let max_num_wasm_fast_memories: u32 = if has_capacity_to_use_large_gigacage() { 8 } else { 3 };
        let verbose_bbqjit_allocation: bool = false;
        let verbose_bbqjit_instructions: bool = false;
        let disable_bbq_consts: bool = false;
        let use_bbqjit: bool = true;
        let use_omgjit: bool = true;
        let wasm_function_index_range_to_compile: OptionRange = OptionRange::default();
        let use_eager_wasm_module_hashing: bool = false;
        let use_array_allocation_profiling: bool = true;
        let force_poly_proto: bool = false;
        let force_mini_vm_mode: bool = false;
        let use_trace_points: bool = false;
        let use_compiler_signpost: bool = false;
        let use_gc_signpost: bool = false;
        let trace_ll_int_execution: bool = false;
        let trace_ll_int_slow_path: bool = false;
        let trace_baseline_jit_execution: bool = false;
        let threshold_for_global_lexical_binding_epoch: u32 = u32::MAX;
        let disk_cache_path: Option<String> = None;
        let verbose_disk_cache: bool = false;
        let force_disk_cache: bool = false;
        let validate_abstract_interpreter_state: bool = false;
        let validate_abstract_interpreter_state_probability: f64 = 0.5;
        let dump_jit_memory_path: Option<String> = None;
        let dump_jit_memory_flush_interval: f64 = 10.0;
        let use_unlinked_code_block_jettisoning: bool = false;
        let force_osr_exit_to_ll_int: bool = false;
        let get_by_val_ic_max_number_of_identifiers: u32 = 4;
        let use_randomizing_executable_island_allocation: bool = false;
        let expose_profilers_on_global_object: bool = false;
        let allow_unsupported_tiers: bool = false;
        let return_early_from_infinite_loops_for_fuzzing: bool = false;
        let early_return_from_infinite_loops_limit: usize = 1300000000;
        let use_licm_fuzzing: bool = false;
        let seed_for_licm_fuzzer: u32 = 424242;
        let allow_hoisting_licm_probability: f64 = 0.5;
        let expose_custom_setters_on_global_object_for_testing: bool = false;
        let use_jit_cage: bool = can_use_jit_cage();
        let use_allocation_profiling: bool = false;
        let allocation_profiling_mode: u32 = 0;
        let dump_baseline_jit_size_statistics: bool = false;
        let dump_dfgjit_size_statistics: bool = false;
        let use_loop_unrolling: bool = true;
        let use_partial_loop_unrolling: bool = true;
        let verbose_loop_unrolling: bool = false;
        let disallow_loop_unrolling_for_non_innermost: bool = true;
        let max_loop_unrolling_count: u32 = 5;
        let max_loop_unrolling_body_node_size: u32 = 200;
        let max_loop_unrolling_iteration_count: u32 = 4;
        let max_partial_loop_unrolling_body_node_size: u32 = 70;
        let max_partial_loop_unrolling_iteration_count: u32 = 4;
        let max_numeric_hot_loop_size: u32 = 225;
        let max_integer_range_optimization_relationships_per_node: u32 = 24;
        let max_integer_range_optimization_work: u32 = 50000000;
        let print_each_unrolled_loop: bool = false;
        let verbose_executable_pool_allocation: bool = false;
        let use_handler_ic_in_ftl: bool = false;
        let use_ll_int_i_cs: bool = true;
        let use_baseline_jit_code_sharing: bool = jit_enabled_by_default();
        let libpas_scavenge_continuously: bool = false;
        let libpas_force_pgm_with_rate: u32 = 0;
        let use_wasm_fault_signal_handler: bool = true;
        let dump_unlinked_dfg_validation: bool = false;
        let dump_wasm_opcode_statistics: bool = false;
        let dump_wasm_warnings: bool = false;
        let use_recursive_json_parse: bool = true;
        let threshold_for_string_replace_cache: u32 = 0x1000;
        let use_wasm_ip_int: bool = ipint_enabled_by_default();
        let use_wasm_ip_int_prologue_osr: bool = true;
        let use_wasm_ip_int_loop_osr: bool = true;
        let use_wasm_ip_int_epilogue_osr: bool = true;
        let use_wasm_ip_int_simd: bool = true;
        let trace_wasm_ip_int_execution: bool = false;
        let force_all_functions_to_use_simd: bool = false;
        let use_omg_inlining: bool = true;
        let free_retired_wasm_code: bool = true;
        let use_array_allocation_sinking: bool = true;
        let dump_ftl_code_size: bool = false;
        let dump_optimization_tracing: bool = false;
        let dump_ion_graph: bool = false;
        let ion_graph_directory: Option<String> = None;
        let marked_block_dump_info_count: u32 = 0;
        let use_async_stack_trace: bool = true;
        let use_big_int_math_methods: bool = false;
        let use_explicit_resource_management: bool = true;
        let use_import_defer: bool = true;
        let use_import_text: bool = true;
        let use_iterator_chunking: bool = true;
        let use_iterator_includes: bool = true;
        let use_iterator_join: bool = true;
        let use_iterator_sequencing: bool = true;
        let use_json_source_text_access: bool = true;
        let use_jspi: bool = true;
        let use_joint_iteration: bool = true;
        let use_more_currency_display_choices: bool = false;
        let use_promise_is_promise: bool = false;
        let use_reg_exp_buffer_boundaries: bool = false;
        let use_shadow_realm: bool = false;
        let use_temporal: bool = true;
        let use_wasm_js_string_builtins: bool = true;
        let use_wasm_js_types: bool = false;
        let use_wasm_memory64: bool = true;
        let use_wasm_memory_to_buffer_ap_is: bool = true;
        let use_wasm_multi_memory: bool = true;
        let use_wasm_relaxed_simd: bool = true;
        let use_wasm_simd: bool = true;
        let use_wasm_tail_calls: bool = true;
        let use_wasm_wide_arithmetic: bool = false;
        let disallow_mixed_wasm_exceptions: bool = true;
        let use_shared_array_buffer: bool = false;
        let use_trusted_types: bool = true;
        Options { use_kern_tcsm, validate_options, dump_options, config_file, use_ll_int, use_jit, use_baseline_jit, use_dfgjit, use_reg_exp_jit, use_domjit, use_reg_exp_lookbehind_jit, use_reg_exp_alternation_factoring, use_reg_exp_alternation_dispatch, reg_exp_dispatch_max_inline_literal_length, report_must_succeed_executable_allocations, use_v8_date_parser, show_private_scripts_in_stack_traces, eval_mode, use_ffiic_stub, use_ffi_call_in_dfg, use_ffi_direct_call, dump_ffi_disassembly, verbose_ffi, max_per_thread_stack_usage, soft_reserved_zone_size, reserved_zone_size, crash_on_disallowed_vm_entry, crash_if_cant_allocate_jit_memory, structure_heap_size_in_kb, jit_memory_reservation_size, jit_memory_reservation_address, force_code_block_liveness, force_ic_failure, force_unlinked_dfg, repatch_count_for_cool_down, initial_cool_down_count, repatch_buffering_countdown, initial_repatch_buffering_countdown, dump_generated_bytecodes, dump_bytecode_liveness_results, validate_bytecode, force_debugger_bytecode_generation, debugger_triggers_breakpoint_exception, verbose_wasm_debugger, enable_wasm_debugger, verbose_wasm_type_cleanup, dump_bytecodes_before_generatorification, switch_jump_table_amount_threshold, use_function_dot_arguments, use_tail_calls, optimize_recursive_tail_calls, always_use_shadow_chicken, shadow_chicken_log_size, shadow_chicken_max_tail_deleted_frames_size, use_os_log, need_disassembly_support, dump_disassembly, log_jit, dump_baseline_disassembly, dump_dfg_disassembly, dump_ftl_disassembly, dump_cssjit_disassembly, dump_reg_exp_disassembly, trace_reg_exp_jit_execution, verify_reg_exp_jit_reads, dump_wasm_disassembly, dump_wasm_source_file_name, wasm_omg_functions_to_dump, dump_bbq_disassembly, dump_omg_disassembly, use_jit_dump, use_gdb_jit_info, use_text_markers, jit_dump_directory, use_ir_dump, ir_dump_directory, use_source_code_dump, source_code_dump_directory, text_markers_directory, bytecode_range_to_jit_compile, bytecode_range_to_dfg_compile, bytecode_range_to_ftl_compile, jit_allowlist, dfg_allowlist, ftl_allowlist, bbq_allowlist, omg_allowlist, loop_unrolling_allowlist, dump_graph_allowlist, dump_source_at_dfg_time, dump_bytecode_at_dfg_time, dump_graph_after_parsing, dump_graph_at_each_phase, dump_dfg_graph_at_each_phase, dump_dfgftl_graph_at_each_phase, dump_b3_graph_at_each_phase, dump_air_graph_at_each_phase, verbose_dfg_bytecode_parsing, safepoint_before_each_phase, verbose_compilation, verbose_ftl_compilation, log_compilation_changes, print_each_osr_exit, print_each_dfgftl_inline_call, use_jit_asserts, validate_does_gc, validate_graph, validate_graph_at_each_phase, verbose_validation_failure, verbose_osr, verbose_dfgosr_exit, verbose_ftlosr_exit, verbose_call_link, verbose_compilation_queue, report_compile_times, report_baseline_compile_times, report_dfg_compile_times, report_ftl_compile_times, report_total_compile_times, report_total_phase_times, report_parse_times, report_bytecode_compile_times, report_bytecode_cache_decode_times, count_parse_times, verbose_exit_profile, verbose_cfa, verbose_dfg_failure, verbose_ftl_to_js_thunk, verbose_ftl_failure, test_the_ftl, verbose_sanitize_stack, use_generational_gc, use_concurrent_gc, collect_continuously, collect_continuously_period_ms, force_fenced_barrier, verbose_visit_race, optimize_parallel_slot_visitors_for_stopped_mutator, verbose_heap_snapshot_logging, large_heap_size, medium_heap_size, small_heap_size, small_heap_ram_fraction, small_heap_growth_factor, medium_heap_ram_fraction, medium_heap_growth_factor, large_heap_growth_factor, mini_vm_heap_growth_factor, heap_growth_steepness_factor, heap_growth_max_increase, min_eden_to_old_generation_ratio, heap_growth_function_threshold_in_mb, critical_gc_memory_threshold, custom_full_gc_callback_bail_threshold, minimum_mutator_utilization, maximum_mutator_utilization, epsilon_mutator_utilization, concurrent_gc_max_headroom, concurrent_gc_period_ms, use_stochastic_mutator_scheduler, minimum_gc_pause_ms, gc_pause_scale, gc_increment_bytes, gc_increment_max_bytes, gc_increment_scale, use_warm_up_marked_blocks, warm_up_marked_block_count, warm_up_marked_block_start_after_blocks, warm_up_marked_block_idle_timeout, scribble_free_cells, decommit_unused_marked_block_pages, decommit_unused_marked_block_pages_after_eden_collections, size_class_progression, precise_allocation_cutoff, dump_size_classes, steal_empty_blocks_from_other_allocators, eagerly_update_top_call_frame, dump_zapped_cell_crash_data, use_osr_entry_to_dfg, use_osr_entry_to_ftl, use_ftljit, validate_ftlosr_exit_liveness, poison_dead_osr_exit_variables, default_b3_opt_level, b3_always_fails_before_compile, b3_always_fails_before_link, validate_serialized_value, ftl_crashes, clobber_all_regs_in_ftlic_slow_path, use_jit_debug_assertions, use_access_inlining, max_access_variant_list_size, threshold_for_undesired_megamorphic_access_variant_list_size, use_polyvariant_devirtualization, use_polymorphic_access_inlining, max_polymorphic_access_inlining_list_size, use_polymorphic_call_inlining, use_polymorphic_call_inlining_for_non_stub_status, max_polymorphic_call_variant_list_size, max_polymorphic_call_variant_list_size_for_top_tier, max_polymorphic_call_variant_list_size_for_wasm_to_js, max_polymorphic_call_variants_for_inlining, frequent_call_threshold, minimum_call_to_known_rate, create_pre_headers, use_mov_hint_removal, use_put_stack_sinking, use_object_allocation_sinking, verbose_object_allocation_sinking, use_value_rep_elimination, use_arity_fixup_inlining, log_executable_allocation, max_dfg_nodes_in_basic_block_for_precise_analysis, use_concurrent_jit, min_number_of_worklist_threads, max_number_of_worklist_threads, number_of_baseline_compiler_threads, number_of_dfg_compiler_threads, number_of_ftl_compiler_threads, number_of_wasm_compiler_threads, worklist_load_factor, worklist_baseline_load_weight, worklist_dfg_load_weight, worklist_ftl_load_weight, priority_delta_of_dfg_compiler_threads, priority_delta_of_ftl_compiler_threads, priority_delta_of_wasm_compiler_threads, use_profiler, dump_profiler_data_at_exit, disassemble_baseline_for_profiler, abbreviate_source_code_for_profiler, use_architecture_specific_optimizations, break_on_throw, maximum_optimization_candidate_bytecode_cost, maximum_cached_assembler_buffer_size, maximum_function_for_call_inline_candidate_bytecode_cost_for_dfg, maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_dfg, maximum_function_for_construct_inline_candidate_bytecode_cost_for_dfg, maximum_function_for_call_inline_candidate_bytecode_cost_for_ftl, maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_ftl, maximum_function_for_construct_inline_candidate_bytecode_cost_for_ftl, maximum_ftl_candidate_bytecode_cost, ratio_ftl_nodes_to_bytecode_cost, maximum_inlining_depth, maximum_inlining_recursion, maximum_inlining_caller_bytecode_cost, use_global_inlining_planner, global_inlining_plan_budget_for_dfg, global_inlining_plan_budget_for_ftl, maximum_global_inlining_plan_sites, inlining_plan_tier_bonus_base, inlining_plan_tier_bonus_power_for_ftl, inlining_plan_tier_bonus_power_for_dfg, inlining_plan_tier_bonus_power_for_baseline, inlining_plan_depth_penalty, maximum_varargs_for_inlining, maximum_binary_string_switch_case_length, maximum_binary_string_switch_total_length, maximum_inline_string_switch_case_count, maximum_reg_exp_test_inline_codesize, maximum_reg_exp_jit_code_size, wasm_inlining_maximum_depth, wasm_inlining_maximum_wasm_callee_size, wasm_inlining_maximum_count, wasm_inlining_minimum_budget, wasm_inlining_factor, wasm_inlining_budget, wasm_inlining_large_function_growth_factor, wasm_inlining_tiny_function_threshold, wasm_inlining_small_function_threshold, jit_policy_scale, number_of_super_and_performance_cores_override, dfg_threshold_scale_for_few_performance_cores, ftl_threshold_scale_for_few_performance_cores, force_eager_compilation, threshold_for_jit_after_warm_up, threshold_for_jit_soon, threshold_for_optimize_after_warm_up, threshold_for_optimize_after_long_warm_up, threshold_for_optimize_soon, execution_counter_increment_for_loop, execution_counter_increment_for_entry, threshold_for_ftl_optimize_after_warm_up, threshold_for_ftl_optimize_soon, ftl_tier_up_counter_increment_for_loop, ftl_tier_up_counter_increment_for_return, ftl_osr_entry_failure_count_for_reoptimization, ftl_osr_entry_retry_threshold, eval_threshold_multiplier, maximum_eval_cacheable_source_length, maximum_execution_counts_between_checkpoints_for_baseline, maximum_execution_counts_between_checkpoints_for_upper_tiers, high_cost_baseline_profiling_function_bytecode_cost, value_profile_filling_rate_monitoring_bytecode_cost, likely_to_take_slow_case_minimum_count, could_take_slow_case_minimum_count, osr_exit_count_for_reoptimization, osr_exit_count_for_reoptimization_from_loop, reoptimization_retry_counter_max, minimum_optimization_delay, maximum_optimization_delay, desired_profile_liveness_rate, desired_profile_fullness_rate, quick_dfg_tier_up_threshold_factor, relaxed_profile_coverage_factor_for_quick_dfg_tier_up, quick_ftl_tier_up_threshold_factor, double_vote_ratio_for_double_format, structure_check_vote_ratio_for_hoisting, check_array_vote_ratio_for_hoisting, maximum_direct_call_stack_size, minimum_number_of_scans_between_rebalance, number_of_gc_markers, use_parallel_marking_constraint_solver, opaque_root_merge_threshold, max_heap_size_as_ram_size_multiple, min_heap_utilization, min_marked_block_utilization, slow_path_allocs_between_g_cs, max_reg_exp_stack_size, percent_cpu_per_mb_for_full_timer, percent_cpu_per_mb_for_eden_timer, collection_timer_max_percent_cpu, force_weak_random_seed, forced_weak_random_seed, always_have_a_bad_time, allow_double_shape, use_zombie_mode, use_immortal_objects, sweep_synchronously, max_single_allocation_size, log_gc, use_gc, use_global_gc, gc_at_end, force_gc_slow_paths, force_did_defer_gc_work, gc_max_heap_size, force_ram_size, record_gc_pause_times, dump_heap_statistics_at_vm_destruction, enable_strong_ref_tracker, dump_heap_on_low_memory, force_code_block_to_jettison_due_to_old_age, use_eager_code_block_jettison_timing, use_execution_count_for_code_block_aging, optimized_code_aging_quiet_allocation_mb, optimized_code_aging_quiet_seconds, code_block_aging_lease_multiplier, use_lean_bytecode_cache_decoder, use_borrowed_bytecode_from_cache, disk_cache_payload_is_persistent_for_testing, verify_bytecode_cache_checksums, use_type_profiler, use_control_flow_profiler, use_sampling_profiler, sample_interval, collect_extra_sampling_profiler_data, sampling_profiler_top_functions_count, sampling_profiler_top_bytecodes_count, sampling_profiler_ignore_external_source_id, sampling_profiler_path, sample_c_code, always_generate_pc_to_code_origin_map, random_integrity_audit_rate, verify_gc, verbose_verify_gc, verify_heap, number_of_gc_cycles_to_record_for_verification, exception_stack_trace_limit, default_error_stack_trace_limit, exit_on_resource_exhaustion, use_exception_fuzz, fire_exception_fuzz_at, fuzz_atomic_jit_memcpy, validate_dfg_exception_handling, dump_simulated_throws, validate_exception_checks, unexpected_exception_stack_trace_limit, validate_dfg_clobberize, validate_bounds_check_elimination, validate_dfg_may_exit, validate_vm_entry_callee_saves, use_executable_allocation_fuzz, fire_executable_allocation_fuzz_at, fire_executable_allocation_fuzz_at_or_after, fire_executable_allocation_fuzz_randomly, fire_executable_allocation_fuzz_randomly_probability, verbose_executable_allocation_fuzz, zero_executable_memory_on_free, use_osr_exit_fuzz, fire_osr_exit_fuzz_at_static, fire_osr_exit_fuzz_at, fire_osr_exit_fuzz_at_or_after, verbose_osr_exit_fuzz, use_loljit, verbose_lol_allocation, seed_of_vm_random_for_fuzzer, use_randomizing_fuzzer_agent, seed_of_randomizing_fuzzer_agent, dump_fuzzer_agent_predictions, use_double_prediction_fuzzer_agent, use_file_based_fuzzer_agent, use_prediction_file_creating_fuzzer_agent, require_prediction_for_file_based_fuzzer_agent, fuzzer_predictions_file, use_narrowing_number_prediction_fuzzer_agent, use_widening_number_prediction_fuzzer_agent, log_phase_times, rare_block_penalty, air_greedy_reg_alloc_verbose, air_greedy_reg_alloc_dump_function, air_greedy_reg_alloc_split_multiplier, air_greedy_reg_alloc_split_around_loops, air_greedy_reg_alloc_loop_split_max_loop_fraction, air_greedy_reg_alloc_spills_everything, air_dump_phase_stats, air_validate_greed_reg_alloc, air_randomize_regs, air_randomize_regs_seed, coalesce_spill_slots, log_air_register_pressure, use_b3_tail_dup, max_b3_tail_dup_block_size, max_b3_tail_dup_block_successors, use_b3_hoist_loop_invariant_values, use_b3_canonicalize_pre_post_increments, use_b3_eliminate_wasm_gc_allocations, use_b3_reduce_strength_fixpoint, use_air_optimize_paired_load_store, use_dollar_vm, function_overrides, watchdog, use_polling_traps, force_trap_aware_stack_checks, use_mach_for_exceptions, allow_non_sp_tagging, use_ic_stats, use_fuzzer_mode, prototype_hit_count_for_ll_int_caching, dump_compiled_reg_exp_patterns, verbose_reg_exp_compilation, dump_module_record, dump_module_loading_state, expose_internal_module_loader, expose_private_identifiers, use_super_sampler, use_source_provider_cache, use_code_cache, use_wasm, fail_to_compile_wasm_code, wasm_small_partial_compile_limit, wasm_large_partial_compile_limit, wasm_omg_optimization_level, use_wasm_byte_loop_replacement, use_bbq_tier_up_checks, use_wasm_osr, threshold_for_bbq_optimize_after_warm_up, threshold_for_bbq_optimize_soon, threshold_for_omg_optimize_after_warm_up, threshold_for_omg_optimize_soon, maximum_omg_candidate_cost, omg_tier_up_counter_increment_for_loop, omg_tier_up_counter_increment_for_entry, wasm_omg_entry_increment_size_reference, use_wasm_fast_memory, log_wasm_memory, wasm_fast_memory_redzone_pages, crash_if_wasm_cant_fast_memory, crash_on_failed_wasm_validate, max_num_wasm_fast_memories, verbose_bbqjit_allocation, verbose_bbqjit_instructions, disable_bbq_consts, use_bbqjit, use_omgjit, wasm_function_index_range_to_compile, use_eager_wasm_module_hashing, use_array_allocation_profiling, force_poly_proto, force_mini_vm_mode, use_trace_points, use_compiler_signpost, use_gc_signpost, trace_ll_int_execution, trace_ll_int_slow_path, trace_baseline_jit_execution, threshold_for_global_lexical_binding_epoch, disk_cache_path, verbose_disk_cache, force_disk_cache, validate_abstract_interpreter_state, validate_abstract_interpreter_state_probability, dump_jit_memory_path, dump_jit_memory_flush_interval, use_unlinked_code_block_jettisoning, force_osr_exit_to_ll_int, get_by_val_ic_max_number_of_identifiers, use_randomizing_executable_island_allocation, expose_profilers_on_global_object, allow_unsupported_tiers, return_early_from_infinite_loops_for_fuzzing, early_return_from_infinite_loops_limit, use_licm_fuzzing, seed_for_licm_fuzzer, allow_hoisting_licm_probability, expose_custom_setters_on_global_object_for_testing, use_jit_cage, use_allocation_profiling, allocation_profiling_mode, dump_baseline_jit_size_statistics, dump_dfgjit_size_statistics, use_loop_unrolling, use_partial_loop_unrolling, verbose_loop_unrolling, disallow_loop_unrolling_for_non_innermost, max_loop_unrolling_count, max_loop_unrolling_body_node_size, max_loop_unrolling_iteration_count, max_partial_loop_unrolling_body_node_size, max_partial_loop_unrolling_iteration_count, max_numeric_hot_loop_size, max_integer_range_optimization_relationships_per_node, max_integer_range_optimization_work, print_each_unrolled_loop, verbose_executable_pool_allocation, use_handler_ic_in_ftl, use_ll_int_i_cs, use_baseline_jit_code_sharing, libpas_scavenge_continuously, libpas_force_pgm_with_rate, use_wasm_fault_signal_handler, dump_unlinked_dfg_validation, dump_wasm_opcode_statistics, dump_wasm_warnings, use_recursive_json_parse, threshold_for_string_replace_cache, use_wasm_ip_int, use_wasm_ip_int_prologue_osr, use_wasm_ip_int_loop_osr, use_wasm_ip_int_epilogue_osr, use_wasm_ip_int_simd, trace_wasm_ip_int_execution, force_all_functions_to_use_simd, use_omg_inlining, free_retired_wasm_code, use_array_allocation_sinking, dump_ftl_code_size, dump_optimization_tracing, dump_ion_graph, ion_graph_directory, marked_block_dump_info_count, use_async_stack_trace, use_big_int_math_methods, use_explicit_resource_management, use_import_defer, use_import_text, use_iterator_chunking, use_iterator_includes, use_iterator_join, use_iterator_sequencing, use_json_source_text_access, use_jspi, use_joint_iteration, use_more_currency_display_choices, use_promise_is_promise, use_reg_exp_buffer_boundaries, use_shadow_realm, use_temporal, use_wasm_js_string_builtins, use_wasm_js_types, use_wasm_memory64, use_wasm_memory_to_buffer_ap_is, use_wasm_multi_memory, use_wasm_relaxed_simd, use_wasm_simd, use_wasm_tail_calls, use_wasm_wide_arithmetic, disallow_mixed_wasm_exceptions, use_shared_array_buffer, use_trusted_types }
    }
}

impl Options {
    /// Opção `useKernTCSM`.
    pub fn use_kern_tcsm() -> bool {
        Options::with(|options| options.use_kern_tcsm)
    }

    /// `Options::useKernTCSM() = value`.
    pub fn set_use_kern_tcsm(value: bool) {
        Options::with_mut(|options| options.use_kern_tcsm = value);
    }

    /// Opção `validateOptions`.
    pub fn validate_options() -> bool {
        Options::with(|options| options.validate_options)
    }

    /// `Options::validateOptions() = value`.
    pub fn set_validate_options(value: bool) {
        Options::with_mut(|options| options.validate_options = value);
    }

    /// Opção `dumpOptions`.
    pub fn dump_options() -> u32 {
        Options::with(|options| options.dump_options)
    }

    /// `Options::dumpOptions() = value`.
    pub fn set_dump_options(value: u32) {
        Options::with_mut(|options| options.dump_options = value);
    }

    /// Opção `configFile`.
    pub fn config_file() -> Option<String> {
        Options::with(|options| options.config_file.clone())
    }

    /// `Options::configFile() = value`.
    pub fn set_config_file(value: Option<String>) {
        Options::with_mut(|options| options.config_file = value);
    }

    /// Opção `useLLInt`.
    pub fn use_ll_int() -> bool {
        Options::with(|options| options.use_ll_int)
    }

    /// `Options::useLLInt() = value`.
    pub fn set_use_ll_int(value: bool) {
        Options::with_mut(|options| options.use_ll_int = value);
    }

    /// Opção `useJIT`.
    pub fn use_jit() -> bool {
        Options::with(|options| options.use_jit)
    }

    /// `Options::useJIT() = value`.
    pub fn set_use_jit(value: bool) {
        Options::with_mut(|options| options.use_jit = value);
    }

    /// Opção `useBaselineJIT`.
    pub fn use_baseline_jit() -> bool {
        Options::with(|options| options.use_baseline_jit)
    }

    /// `Options::useBaselineJIT() = value`.
    pub fn set_use_baseline_jit(value: bool) {
        Options::with_mut(|options| options.use_baseline_jit = value);
    }

    /// Opção `useDFGJIT`.
    pub fn use_dfgjit() -> bool {
        Options::with(|options| options.use_dfgjit)
    }

    /// `Options::useDFGJIT() = value`.
    pub fn set_use_dfgjit(value: bool) {
        Options::with_mut(|options| options.use_dfgjit = value);
    }

    /// Opção `useRegExpJIT`.
    pub fn use_reg_exp_jit() -> bool {
        Options::with(|options| options.use_reg_exp_jit)
    }

    /// `Options::useRegExpJIT() = value`.
    pub fn set_use_reg_exp_jit(value: bool) {
        Options::with_mut(|options| options.use_reg_exp_jit = value);
    }

    /// Opção `useDOMJIT`.
    pub fn use_domjit() -> bool {
        Options::with(|options| options.use_domjit)
    }

    /// `Options::useDOMJIT() = value`.
    pub fn set_use_domjit(value: bool) {
        Options::with_mut(|options| options.use_domjit = value);
    }

    /// Opção `useRegExpLookbehindJIT`.
    pub fn use_reg_exp_lookbehind_jit() -> bool {
        Options::with(|options| options.use_reg_exp_lookbehind_jit)
    }

    /// `Options::useRegExpLookbehindJIT() = value`.
    pub fn set_use_reg_exp_lookbehind_jit(value: bool) {
        Options::with_mut(|options| options.use_reg_exp_lookbehind_jit = value);
    }

    /// Opção `useRegExpAlternationFactoring`.
    pub fn use_reg_exp_alternation_factoring() -> bool {
        Options::with(|options| options.use_reg_exp_alternation_factoring)
    }

    /// `Options::useRegExpAlternationFactoring() = value`.
    pub fn set_use_reg_exp_alternation_factoring(value: bool) {
        Options::with_mut(|options| options.use_reg_exp_alternation_factoring = value);
    }

    /// Opção `useRegExpAlternationDispatch`.
    pub fn use_reg_exp_alternation_dispatch() -> bool {
        Options::with(|options| options.use_reg_exp_alternation_dispatch)
    }

    /// `Options::useRegExpAlternationDispatch() = value`.
    pub fn set_use_reg_exp_alternation_dispatch(value: bool) {
        Options::with_mut(|options| options.use_reg_exp_alternation_dispatch = value);
    }

    /// Opção `regExpDispatchMaxInlineLiteralLength`.
    pub fn reg_exp_dispatch_max_inline_literal_length() -> u32 {
        Options::with(|options| options.reg_exp_dispatch_max_inline_literal_length)
    }

    /// `Options::regExpDispatchMaxInlineLiteralLength() = value`.
    pub fn set_reg_exp_dispatch_max_inline_literal_length(value: u32) {
        Options::with_mut(|options| options.reg_exp_dispatch_max_inline_literal_length = value);
    }

    /// Opção `reportMustSucceedExecutableAllocations`.
    pub fn report_must_succeed_executable_allocations() -> bool {
        Options::with(|options| options.report_must_succeed_executable_allocations)
    }

    /// `Options::reportMustSucceedExecutableAllocations() = value`.
    pub fn set_report_must_succeed_executable_allocations(value: bool) {
        Options::with_mut(|options| options.report_must_succeed_executable_allocations = value);
    }

    /// Opção `useV8DateParser`.
    pub fn use_v8_date_parser() -> bool {
        Options::with(|options| options.use_v8_date_parser)
    }

    /// `Options::useV8DateParser() = value`.
    pub fn set_use_v8_date_parser(value: bool) {
        Options::with_mut(|options| options.use_v8_date_parser = value);
    }

    /// Opção `showPrivateScriptsInStackTraces`.
    pub fn show_private_scripts_in_stack_traces() -> bool {
        Options::with(|options| options.show_private_scripts_in_stack_traces)
    }

    /// `Options::showPrivateScriptsInStackTraces() = value`.
    pub fn set_show_private_scripts_in_stack_traces(value: bool) {
        Options::with_mut(|options| options.show_private_scripts_in_stack_traces = value);
    }

    /// Opção `evalMode`.
    pub fn eval_mode() -> bool {
        Options::with(|options| options.eval_mode)
    }

    /// `Options::evalMode() = value`.
    pub fn set_eval_mode(value: bool) {
        Options::with_mut(|options| options.eval_mode = value);
    }

    /// Opção `useFFIICStub`.
    pub fn use_ffiic_stub() -> bool {
        Options::with(|options| options.use_ffiic_stub)
    }

    /// `Options::useFFIICStub() = value`.
    pub fn set_use_ffiic_stub(value: bool) {
        Options::with_mut(|options| options.use_ffiic_stub = value);
    }

    /// Opção `useFFICallInDFG`.
    pub fn use_ffi_call_in_dfg() -> bool {
        Options::with(|options| options.use_ffi_call_in_dfg)
    }

    /// `Options::useFFICallInDFG() = value`.
    pub fn set_use_ffi_call_in_dfg(value: bool) {
        Options::with_mut(|options| options.use_ffi_call_in_dfg = value);
    }

    /// Opção `useFFIDirectCall`.
    pub fn use_ffi_direct_call() -> bool {
        Options::with(|options| options.use_ffi_direct_call)
    }

    /// `Options::useFFIDirectCall() = value`.
    pub fn set_use_ffi_direct_call(value: bool) {
        Options::with_mut(|options| options.use_ffi_direct_call = value);
    }

    /// Opção `dumpFFIDisassembly`.
    pub fn dump_ffi_disassembly() -> bool {
        Options::with(|options| options.dump_ffi_disassembly)
    }

    /// `Options::dumpFFIDisassembly() = value`.
    pub fn set_dump_ffi_disassembly(value: bool) {
        Options::with_mut(|options| options.dump_ffi_disassembly = value);
    }

    /// Opção `verboseFFI`.
    pub fn verbose_ffi() -> bool {
        Options::with(|options| options.verbose_ffi)
    }

    /// `Options::verboseFFI() = value`.
    pub fn set_verbose_ffi(value: bool) {
        Options::with_mut(|options| options.verbose_ffi = value);
    }

    /// Opção `maxPerThreadStackUsage`.
    pub fn max_per_thread_stack_usage() -> u32 {
        Options::with(|options| options.max_per_thread_stack_usage)
    }

    /// `Options::maxPerThreadStackUsage() = value`.
    pub fn set_max_per_thread_stack_usage(value: u32) {
        Options::with_mut(|options| options.max_per_thread_stack_usage = value);
    }

    /// Opção `softReservedZoneSize`.
    pub fn soft_reserved_zone_size() -> u32 {
        Options::with(|options| options.soft_reserved_zone_size)
    }

    /// `Options::softReservedZoneSize() = value`.
    pub fn set_soft_reserved_zone_size(value: u32) {
        Options::with_mut(|options| options.soft_reserved_zone_size = value);
    }

    /// Opção `reservedZoneSize`.
    pub fn reserved_zone_size() -> u32 {
        Options::with(|options| options.reserved_zone_size)
    }

    /// `Options::reservedZoneSize() = value`.
    pub fn set_reserved_zone_size(value: u32) {
        Options::with_mut(|options| options.reserved_zone_size = value);
    }

    /// Opção `crashOnDisallowedVMEntry`.
    pub fn crash_on_disallowed_vm_entry() -> bool {
        Options::with(|options| options.crash_on_disallowed_vm_entry)
    }

    /// `Options::crashOnDisallowedVMEntry() = value`.
    pub fn set_crash_on_disallowed_vm_entry(value: bool) {
        Options::with_mut(|options| options.crash_on_disallowed_vm_entry = value);
    }

    /// Opção `crashIfCantAllocateJITMemory`.
    pub fn crash_if_cant_allocate_jit_memory() -> bool {
        Options::with(|options| options.crash_if_cant_allocate_jit_memory)
    }

    /// `Options::crashIfCantAllocateJITMemory() = value`.
    pub fn set_crash_if_cant_allocate_jit_memory(value: bool) {
        Options::with_mut(|options| options.crash_if_cant_allocate_jit_memory = value);
    }

    /// Opção `structureHeapSizeInKB`.
    pub fn structure_heap_size_in_kb() -> u32 {
        Options::with(|options| options.structure_heap_size_in_kb)
    }

    /// `Options::structureHeapSizeInKB() = value`.
    pub fn set_structure_heap_size_in_kb(value: u32) {
        Options::with_mut(|options| options.structure_heap_size_in_kb = value);
    }

    /// Opção `jitMemoryReservationSize`.
    pub fn jit_memory_reservation_size() -> u32 {
        Options::with(|options| options.jit_memory_reservation_size)
    }

    /// `Options::jitMemoryReservationSize() = value`.
    pub fn set_jit_memory_reservation_size(value: u32) {
        Options::with_mut(|options| options.jit_memory_reservation_size = value);
    }

    /// Opção `jitMemoryReservationAddress`.
    pub fn jit_memory_reservation_address() -> usize {
        Options::with(|options| options.jit_memory_reservation_address)
    }

    /// `Options::jitMemoryReservationAddress() = value`.
    pub fn set_jit_memory_reservation_address(value: usize) {
        Options::with_mut(|options| options.jit_memory_reservation_address = value);
    }

    /// Opção `forceCodeBlockLiveness`.
    pub fn force_code_block_liveness() -> bool {
        Options::with(|options| options.force_code_block_liveness)
    }

    /// `Options::forceCodeBlockLiveness() = value`.
    pub fn set_force_code_block_liveness(value: bool) {
        Options::with_mut(|options| options.force_code_block_liveness = value);
    }

    /// Opção `forceICFailure`.
    pub fn force_ic_failure() -> bool {
        Options::with(|options| options.force_ic_failure)
    }

    /// `Options::forceICFailure() = value`.
    pub fn set_force_ic_failure(value: bool) {
        Options::with_mut(|options| options.force_ic_failure = value);
    }

    /// Opção `forceUnlinkedDFG`.
    pub fn force_unlinked_dfg() -> bool {
        Options::with(|options| options.force_unlinked_dfg)
    }

    /// `Options::forceUnlinkedDFG() = value`.
    pub fn set_force_unlinked_dfg(value: bool) {
        Options::with_mut(|options| options.force_unlinked_dfg = value);
    }

    /// Opção `repatchCountForCoolDown`.
    pub fn repatch_count_for_cool_down() -> u32 {
        Options::with(|options| options.repatch_count_for_cool_down)
    }

    /// `Options::repatchCountForCoolDown() = value`.
    pub fn set_repatch_count_for_cool_down(value: u32) {
        Options::with_mut(|options| options.repatch_count_for_cool_down = value);
    }

    /// Opção `initialCoolDownCount`.
    pub fn initial_cool_down_count() -> u32 {
        Options::with(|options| options.initial_cool_down_count)
    }

    /// `Options::initialCoolDownCount() = value`.
    pub fn set_initial_cool_down_count(value: u32) {
        Options::with_mut(|options| options.initial_cool_down_count = value);
    }

    /// Opção `repatchBufferingCountdown`.
    pub fn repatch_buffering_countdown() -> u32 {
        Options::with(|options| options.repatch_buffering_countdown)
    }

    /// `Options::repatchBufferingCountdown() = value`.
    pub fn set_repatch_buffering_countdown(value: u32) {
        Options::with_mut(|options| options.repatch_buffering_countdown = value);
    }

    /// Opção `initialRepatchBufferingCountdown`.
    pub fn initial_repatch_buffering_countdown() -> u32 {
        Options::with(|options| options.initial_repatch_buffering_countdown)
    }

    /// `Options::initialRepatchBufferingCountdown() = value`.
    pub fn set_initial_repatch_buffering_countdown(value: u32) {
        Options::with_mut(|options| options.initial_repatch_buffering_countdown = value);
    }

    /// Opção `dumpGeneratedBytecodes`.
    pub fn dump_generated_bytecodes() -> bool {
        Options::with(|options| options.dump_generated_bytecodes)
    }

    /// `Options::dumpGeneratedBytecodes() = value`.
    pub fn set_dump_generated_bytecodes(value: bool) {
        Options::with_mut(|options| options.dump_generated_bytecodes = value);
    }

    /// Opção `dumpBytecodeLivenessResults`.
    pub fn dump_bytecode_liveness_results() -> bool {
        Options::with(|options| options.dump_bytecode_liveness_results)
    }

    /// `Options::dumpBytecodeLivenessResults() = value`.
    pub fn set_dump_bytecode_liveness_results(value: bool) {
        Options::with_mut(|options| options.dump_bytecode_liveness_results = value);
    }

    /// Opção `validateBytecode`.
    pub fn validate_bytecode() -> bool {
        Options::with(|options| options.validate_bytecode)
    }

    /// `Options::validateBytecode() = value`.
    pub fn set_validate_bytecode(value: bool) {
        Options::with_mut(|options| options.validate_bytecode = value);
    }

    /// Opção `forceDebuggerBytecodeGeneration`.
    pub fn force_debugger_bytecode_generation() -> bool {
        Options::with(|options| options.force_debugger_bytecode_generation)
    }

    /// `Options::forceDebuggerBytecodeGeneration() = value`.
    pub fn set_force_debugger_bytecode_generation(value: bool) {
        Options::with_mut(|options| options.force_debugger_bytecode_generation = value);
    }

    /// Opção `debuggerTriggersBreakpointException`.
    pub fn debugger_triggers_breakpoint_exception() -> bool {
        Options::with(|options| options.debugger_triggers_breakpoint_exception)
    }

    /// `Options::debuggerTriggersBreakpointException() = value`.
    pub fn set_debugger_triggers_breakpoint_exception(value: bool) {
        Options::with_mut(|options| options.debugger_triggers_breakpoint_exception = value);
    }

    /// Opção `verboseWasmDebugger`.
    pub fn verbose_wasm_debugger() -> bool {
        Options::with(|options| options.verbose_wasm_debugger)
    }

    /// `Options::verboseWasmDebugger() = value`.
    pub fn set_verbose_wasm_debugger(value: bool) {
        Options::with_mut(|options| options.verbose_wasm_debugger = value);
    }

    /// Opção `enableWasmDebugger`.
    pub fn enable_wasm_debugger() -> bool {
        Options::with(|options| options.enable_wasm_debugger)
    }

    /// `Options::enableWasmDebugger() = value`.
    pub fn set_enable_wasm_debugger(value: bool) {
        Options::with_mut(|options| options.enable_wasm_debugger = value);
    }

    /// Opção `verboseWasmTypeCleanup`.
    pub fn verbose_wasm_type_cleanup() -> bool {
        Options::with(|options| options.verbose_wasm_type_cleanup)
    }

    /// `Options::verboseWasmTypeCleanup() = value`.
    pub fn set_verbose_wasm_type_cleanup(value: bool) {
        Options::with_mut(|options| options.verbose_wasm_type_cleanup = value);
    }

    /// Opção `dumpBytecodesBeforeGeneratorification`.
    pub fn dump_bytecodes_before_generatorification() -> bool {
        Options::with(|options| options.dump_bytecodes_before_generatorification)
    }

    /// `Options::dumpBytecodesBeforeGeneratorification() = value`.
    pub fn set_dump_bytecodes_before_generatorification(value: bool) {
        Options::with_mut(|options| options.dump_bytecodes_before_generatorification = value);
    }

    /// Opção `switchJumpTableAmountThreshold`.
    pub fn switch_jump_table_amount_threshold() -> u32 {
        Options::with(|options| options.switch_jump_table_amount_threshold)
    }

    /// `Options::switchJumpTableAmountThreshold() = value`.
    pub fn set_switch_jump_table_amount_threshold(value: u32) {
        Options::with_mut(|options| options.switch_jump_table_amount_threshold = value);
    }

    /// Opção `useFunctionDotArguments`.
    pub fn use_function_dot_arguments() -> bool {
        Options::with(|options| options.use_function_dot_arguments)
    }

    /// `Options::useFunctionDotArguments() = value`.
    pub fn set_use_function_dot_arguments(value: bool) {
        Options::with_mut(|options| options.use_function_dot_arguments = value);
    }

    /// Opção `useTailCalls`.
    pub fn use_tail_calls() -> bool {
        Options::with(|options| options.use_tail_calls)
    }

    /// `Options::useTailCalls() = value`.
    pub fn set_use_tail_calls(value: bool) {
        Options::with_mut(|options| options.use_tail_calls = value);
    }

    /// Opção `optimizeRecursiveTailCalls`.
    pub fn optimize_recursive_tail_calls() -> bool {
        Options::with(|options| options.optimize_recursive_tail_calls)
    }

    /// `Options::optimizeRecursiveTailCalls() = value`.
    pub fn set_optimize_recursive_tail_calls(value: bool) {
        Options::with_mut(|options| options.optimize_recursive_tail_calls = value);
    }

    /// Opção `alwaysUseShadowChicken`.
    pub fn always_use_shadow_chicken() -> bool {
        Options::with(|options| options.always_use_shadow_chicken)
    }

    /// `Options::alwaysUseShadowChicken() = value`.
    pub fn set_always_use_shadow_chicken(value: bool) {
        Options::with_mut(|options| options.always_use_shadow_chicken = value);
    }

    /// Opção `shadowChickenLogSize`.
    pub fn shadow_chicken_log_size() -> u32 {
        Options::with(|options| options.shadow_chicken_log_size)
    }

    /// `Options::shadowChickenLogSize() = value`.
    pub fn set_shadow_chicken_log_size(value: u32) {
        Options::with_mut(|options| options.shadow_chicken_log_size = value);
    }

    /// Opção `shadowChickenMaxTailDeletedFramesSize`.
    pub fn shadow_chicken_max_tail_deleted_frames_size() -> u32 {
        Options::with(|options| options.shadow_chicken_max_tail_deleted_frames_size)
    }

    /// `Options::shadowChickenMaxTailDeletedFramesSize() = value`.
    pub fn set_shadow_chicken_max_tail_deleted_frames_size(value: u32) {
        Options::with_mut(|options| options.shadow_chicken_max_tail_deleted_frames_size = value);
    }

    /// Opção `useOSLog`.
    pub fn use_os_log() -> OSLogType {
        Options::with(|options| options.use_os_log)
    }

    /// `Options::useOSLog() = value`.
    pub fn set_use_os_log(value: OSLogType) {
        Options::with_mut(|options| options.use_os_log = value);
    }

    /// Opção `needDisassemblySupport`.
    pub fn need_disassembly_support() -> bool {
        Options::with(|options| options.need_disassembly_support)
    }

    /// `Options::needDisassemblySupport() = value`.
    pub fn set_need_disassembly_support(value: bool) {
        Options::with_mut(|options| options.need_disassembly_support = value);
    }

    /// Opção `dumpDisassembly`.
    pub fn dump_disassembly() -> bool {
        Options::with(|options| options.dump_disassembly)
    }

    /// `Options::dumpDisassembly() = value`.
    pub fn set_dump_disassembly(value: bool) {
        Options::with_mut(|options| options.dump_disassembly = value);
    }

    /// Opção `logJIT`.
    pub fn log_jit() -> bool {
        Options::with(|options| options.log_jit)
    }

    /// `Options::logJIT() = value`.
    pub fn set_log_jit(value: bool) {
        Options::with_mut(|options| options.log_jit = value);
    }

    /// Opção `dumpBaselineDisassembly`.
    pub fn dump_baseline_disassembly() -> bool {
        Options::with(|options| options.dump_baseline_disassembly)
    }

    /// `Options::dumpBaselineDisassembly() = value`.
    pub fn set_dump_baseline_disassembly(value: bool) {
        Options::with_mut(|options| options.dump_baseline_disassembly = value);
    }

    /// Opção `dumpDFGDisassembly`.
    pub fn dump_dfg_disassembly() -> bool {
        Options::with(|options| options.dump_dfg_disassembly)
    }

    /// `Options::dumpDFGDisassembly() = value`.
    pub fn set_dump_dfg_disassembly(value: bool) {
        Options::with_mut(|options| options.dump_dfg_disassembly = value);
    }

    /// Opção `dumpFTLDisassembly`.
    pub fn dump_ftl_disassembly() -> bool {
        Options::with(|options| options.dump_ftl_disassembly)
    }

    /// `Options::dumpFTLDisassembly() = value`.
    pub fn set_dump_ftl_disassembly(value: bool) {
        Options::with_mut(|options| options.dump_ftl_disassembly = value);
    }

    /// Opção `dumpCSSJITDisassembly`.
    pub fn dump_cssjit_disassembly() -> bool {
        Options::with(|options| options.dump_cssjit_disassembly)
    }

    /// `Options::dumpCSSJITDisassembly() = value`.
    pub fn set_dump_cssjit_disassembly(value: bool) {
        Options::with_mut(|options| options.dump_cssjit_disassembly = value);
    }

    /// Opção `dumpRegExpDisassembly`.
    pub fn dump_reg_exp_disassembly() -> bool {
        Options::with(|options| options.dump_reg_exp_disassembly)
    }

    /// `Options::dumpRegExpDisassembly() = value`.
    pub fn set_dump_reg_exp_disassembly(value: bool) {
        Options::with_mut(|options| options.dump_reg_exp_disassembly = value);
    }

    /// Opção `traceRegExpJITExecution`.
    pub fn trace_reg_exp_jit_execution() -> bool {
        Options::with(|options| options.trace_reg_exp_jit_execution)
    }

    /// `Options::traceRegExpJITExecution() = value`.
    pub fn set_trace_reg_exp_jit_execution(value: bool) {
        Options::with_mut(|options| options.trace_reg_exp_jit_execution = value);
    }

    /// Opção `verifyRegExpJITReads`.
    pub fn verify_reg_exp_jit_reads() -> bool {
        Options::with(|options| options.verify_reg_exp_jit_reads)
    }

    /// `Options::verifyRegExpJITReads() = value`.
    pub fn set_verify_reg_exp_jit_reads(value: bool) {
        Options::with_mut(|options| options.verify_reg_exp_jit_reads = value);
    }

    /// Opção `dumpWasmDisassembly`.
    pub fn dump_wasm_disassembly() -> bool {
        Options::with(|options| options.dump_wasm_disassembly)
    }

    /// `Options::dumpWasmDisassembly() = value`.
    pub fn set_dump_wasm_disassembly(value: bool) {
        Options::with_mut(|options| options.dump_wasm_disassembly = value);
    }

    /// Opção `dumpWasmSourceFileName`.
    pub fn dump_wasm_source_file_name() -> Option<String> {
        Options::with(|options| options.dump_wasm_source_file_name.clone())
    }

    /// `Options::dumpWasmSourceFileName() = value`.
    pub fn set_dump_wasm_source_file_name(value: Option<String>) {
        Options::with_mut(|options| options.dump_wasm_source_file_name = value);
    }

    /// Opção `wasmOMGFunctionsToDump`.
    pub fn wasm_omg_functions_to_dump() -> Option<String> {
        Options::with(|options| options.wasm_omg_functions_to_dump.clone())
    }

    /// `Options::wasmOMGFunctionsToDump() = value`.
    pub fn set_wasm_omg_functions_to_dump(value: Option<String>) {
        Options::with_mut(|options| options.wasm_omg_functions_to_dump = value);
    }

    /// Opção `dumpBBQDisassembly`.
    pub fn dump_bbq_disassembly() -> bool {
        Options::with(|options| options.dump_bbq_disassembly)
    }

    /// `Options::dumpBBQDisassembly() = value`.
    pub fn set_dump_bbq_disassembly(value: bool) {
        Options::with_mut(|options| options.dump_bbq_disassembly = value);
    }

    /// Opção `dumpOMGDisassembly`.
    pub fn dump_omg_disassembly() -> bool {
        Options::with(|options| options.dump_omg_disassembly)
    }

    /// `Options::dumpOMGDisassembly() = value`.
    pub fn set_dump_omg_disassembly(value: bool) {
        Options::with_mut(|options| options.dump_omg_disassembly = value);
    }

    /// Opção `useJITDump`.
    pub fn use_jit_dump() -> bool {
        Options::with(|options| options.use_jit_dump)
    }

    /// `Options::useJITDump() = value`.
    pub fn set_use_jit_dump(value: bool) {
        Options::with_mut(|options| options.use_jit_dump = value);
    }

    /// Opção `useGdbJITInfo`.
    pub fn use_gdb_jit_info() -> bool {
        Options::with(|options| options.use_gdb_jit_info)
    }

    /// `Options::useGdbJITInfo() = value`.
    pub fn set_use_gdb_jit_info(value: bool) {
        Options::with_mut(|options| options.use_gdb_jit_info = value);
    }

    /// Opção `useTextMarkers`.
    pub fn use_text_markers() -> bool {
        Options::with(|options| options.use_text_markers)
    }

    /// `Options::useTextMarkers() = value`.
    pub fn set_use_text_markers(value: bool) {
        Options::with_mut(|options| options.use_text_markers = value);
    }

    /// Opção `jitDumpDirectory`.
    pub fn jit_dump_directory() -> Option<String> {
        Options::with(|options| options.jit_dump_directory.clone())
    }

    /// `Options::jitDumpDirectory() = value`.
    pub fn set_jit_dump_directory(value: Option<String>) {
        Options::with_mut(|options| options.jit_dump_directory = value);
    }

    /// Opção `useIRDump`.
    pub fn use_ir_dump() -> bool {
        Options::with(|options| options.use_ir_dump)
    }

    /// `Options::useIRDump() = value`.
    pub fn set_use_ir_dump(value: bool) {
        Options::with_mut(|options| options.use_ir_dump = value);
    }

    /// Opção `irDumpDirectory`.
    pub fn ir_dump_directory() -> Option<String> {
        Options::with(|options| options.ir_dump_directory.clone())
    }

    /// `Options::irDumpDirectory() = value`.
    pub fn set_ir_dump_directory(value: Option<String>) {
        Options::with_mut(|options| options.ir_dump_directory = value);
    }

    /// Opção `useSourceCodeDump`.
    pub fn use_source_code_dump() -> bool {
        Options::with(|options| options.use_source_code_dump)
    }

    /// `Options::useSourceCodeDump() = value`.
    pub fn set_use_source_code_dump(value: bool) {
        Options::with_mut(|options| options.use_source_code_dump = value);
    }

    /// Opção `sourceCodeDumpDirectory`.
    pub fn source_code_dump_directory() -> Option<String> {
        Options::with(|options| options.source_code_dump_directory.clone())
    }

    /// `Options::sourceCodeDumpDirectory() = value`.
    pub fn set_source_code_dump_directory(value: Option<String>) {
        Options::with_mut(|options| options.source_code_dump_directory = value);
    }

    /// Opção `textMarkersDirectory`.
    pub fn text_markers_directory() -> Option<String> {
        Options::with(|options| options.text_markers_directory.clone())
    }

    /// `Options::textMarkersDirectory() = value`.
    pub fn set_text_markers_directory(value: Option<String>) {
        Options::with_mut(|options| options.text_markers_directory = value);
    }

    /// Opção `bytecodeRangeToJITCompile`.
    pub fn bytecode_range_to_jit_compile() -> OptionRange {
        Options::with(|options| options.bytecode_range_to_jit_compile.clone())
    }

    /// `Options::bytecodeRangeToJITCompile() = value`.
    pub fn set_bytecode_range_to_jit_compile(value: OptionRange) {
        Options::with_mut(|options| options.bytecode_range_to_jit_compile = value);
    }

    /// Opção `bytecodeRangeToDFGCompile`.
    pub fn bytecode_range_to_dfg_compile() -> OptionRange {
        Options::with(|options| options.bytecode_range_to_dfg_compile.clone())
    }

    /// `Options::bytecodeRangeToDFGCompile() = value`.
    pub fn set_bytecode_range_to_dfg_compile(value: OptionRange) {
        Options::with_mut(|options| options.bytecode_range_to_dfg_compile = value);
    }

    /// Opção `bytecodeRangeToFTLCompile`.
    pub fn bytecode_range_to_ftl_compile() -> OptionRange {
        Options::with(|options| options.bytecode_range_to_ftl_compile.clone())
    }

    /// `Options::bytecodeRangeToFTLCompile() = value`.
    pub fn set_bytecode_range_to_ftl_compile(value: OptionRange) {
        Options::with_mut(|options| options.bytecode_range_to_ftl_compile = value);
    }

    /// Opção `jitAllowlist`.
    pub fn jit_allowlist() -> Option<String> {
        Options::with(|options| options.jit_allowlist.clone())
    }

    /// `Options::jitAllowlist() = value`.
    pub fn set_jit_allowlist(value: Option<String>) {
        Options::with_mut(|options| options.jit_allowlist = value);
    }

    /// Opção `dfgAllowlist`.
    pub fn dfg_allowlist() -> Option<String> {
        Options::with(|options| options.dfg_allowlist.clone())
    }

    /// `Options::dfgAllowlist() = value`.
    pub fn set_dfg_allowlist(value: Option<String>) {
        Options::with_mut(|options| options.dfg_allowlist = value);
    }

    /// Opção `ftlAllowlist`.
    pub fn ftl_allowlist() -> Option<String> {
        Options::with(|options| options.ftl_allowlist.clone())
    }

    /// `Options::ftlAllowlist() = value`.
    pub fn set_ftl_allowlist(value: Option<String>) {
        Options::with_mut(|options| options.ftl_allowlist = value);
    }

    /// Opção `bbqAllowlist`.
    pub fn bbq_allowlist() -> Option<String> {
        Options::with(|options| options.bbq_allowlist.clone())
    }

    /// `Options::bbqAllowlist() = value`.
    pub fn set_bbq_allowlist(value: Option<String>) {
        Options::with_mut(|options| options.bbq_allowlist = value);
    }

    /// Opção `omgAllowlist`.
    pub fn omg_allowlist() -> Option<String> {
        Options::with(|options| options.omg_allowlist.clone())
    }

    /// `Options::omgAllowlist() = value`.
    pub fn set_omg_allowlist(value: Option<String>) {
        Options::with_mut(|options| options.omg_allowlist = value);
    }

    /// Opção `loopUnrollingAllowlist`.
    pub fn loop_unrolling_allowlist() -> Option<String> {
        Options::with(|options| options.loop_unrolling_allowlist.clone())
    }

    /// `Options::loopUnrollingAllowlist() = value`.
    pub fn set_loop_unrolling_allowlist(value: Option<String>) {
        Options::with_mut(|options| options.loop_unrolling_allowlist = value);
    }

    /// Opção `dumpGraphAllowlist`.
    pub fn dump_graph_allowlist() -> Option<String> {
        Options::with(|options| options.dump_graph_allowlist.clone())
    }

    /// `Options::dumpGraphAllowlist() = value`.
    pub fn set_dump_graph_allowlist(value: Option<String>) {
        Options::with_mut(|options| options.dump_graph_allowlist = value);
    }

    /// Opção `dumpSourceAtDFGTime`.
    pub fn dump_source_at_dfg_time() -> bool {
        Options::with(|options| options.dump_source_at_dfg_time)
    }

    /// `Options::dumpSourceAtDFGTime() = value`.
    pub fn set_dump_source_at_dfg_time(value: bool) {
        Options::with_mut(|options| options.dump_source_at_dfg_time = value);
    }

    /// Opção `dumpBytecodeAtDFGTime`.
    pub fn dump_bytecode_at_dfg_time() -> bool {
        Options::with(|options| options.dump_bytecode_at_dfg_time)
    }

    /// `Options::dumpBytecodeAtDFGTime() = value`.
    pub fn set_dump_bytecode_at_dfg_time(value: bool) {
        Options::with_mut(|options| options.dump_bytecode_at_dfg_time = value);
    }

    /// Opção `dumpGraphAfterParsing`.
    pub fn dump_graph_after_parsing() -> bool {
        Options::with(|options| options.dump_graph_after_parsing)
    }

    /// `Options::dumpGraphAfterParsing() = value`.
    pub fn set_dump_graph_after_parsing(value: bool) {
        Options::with_mut(|options| options.dump_graph_after_parsing = value);
    }

    /// Opção `dumpGraphAtEachPhase`.
    pub fn dump_graph_at_each_phase() -> bool {
        Options::with(|options| options.dump_graph_at_each_phase)
    }

    /// `Options::dumpGraphAtEachPhase() = value`.
    pub fn set_dump_graph_at_each_phase(value: bool) {
        Options::with_mut(|options| options.dump_graph_at_each_phase = value);
    }

    /// Opção `dumpDFGGraphAtEachPhase`.
    pub fn dump_dfg_graph_at_each_phase() -> bool {
        Options::with(|options| options.dump_dfg_graph_at_each_phase)
    }

    /// `Options::dumpDFGGraphAtEachPhase() = value`.
    pub fn set_dump_dfg_graph_at_each_phase(value: bool) {
        Options::with_mut(|options| options.dump_dfg_graph_at_each_phase = value);
    }

    /// Opção `dumpDFGFTLGraphAtEachPhase`.
    pub fn dump_dfgftl_graph_at_each_phase() -> bool {
        Options::with(|options| options.dump_dfgftl_graph_at_each_phase)
    }

    /// `Options::dumpDFGFTLGraphAtEachPhase() = value`.
    pub fn set_dump_dfgftl_graph_at_each_phase(value: bool) {
        Options::with_mut(|options| options.dump_dfgftl_graph_at_each_phase = value);
    }

    /// Opção `dumpB3GraphAtEachPhase`.
    pub fn dump_b3_graph_at_each_phase() -> bool {
        Options::with(|options| options.dump_b3_graph_at_each_phase)
    }

    /// `Options::dumpB3GraphAtEachPhase() = value`.
    pub fn set_dump_b3_graph_at_each_phase(value: bool) {
        Options::with_mut(|options| options.dump_b3_graph_at_each_phase = value);
    }

    /// Opção `dumpAirGraphAtEachPhase`.
    pub fn dump_air_graph_at_each_phase() -> bool {
        Options::with(|options| options.dump_air_graph_at_each_phase)
    }

    /// `Options::dumpAirGraphAtEachPhase() = value`.
    pub fn set_dump_air_graph_at_each_phase(value: bool) {
        Options::with_mut(|options| options.dump_air_graph_at_each_phase = value);
    }

    /// Opção `verboseDFGBytecodeParsing`.
    pub fn verbose_dfg_bytecode_parsing() -> bool {
        Options::with(|options| options.verbose_dfg_bytecode_parsing)
    }

    /// `Options::verboseDFGBytecodeParsing() = value`.
    pub fn set_verbose_dfg_bytecode_parsing(value: bool) {
        Options::with_mut(|options| options.verbose_dfg_bytecode_parsing = value);
    }

    /// Opção `safepointBeforeEachPhase`.
    pub fn safepoint_before_each_phase() -> bool {
        Options::with(|options| options.safepoint_before_each_phase)
    }

    /// `Options::safepointBeforeEachPhase() = value`.
    pub fn set_safepoint_before_each_phase(value: bool) {
        Options::with_mut(|options| options.safepoint_before_each_phase = value);
    }

    /// Opção `verboseCompilation`.
    pub fn verbose_compilation() -> bool {
        Options::with(|options| options.verbose_compilation)
    }

    /// `Options::verboseCompilation() = value`.
    pub fn set_verbose_compilation(value: bool) {
        Options::with_mut(|options| options.verbose_compilation = value);
    }

    /// Opção `verboseFTLCompilation`.
    pub fn verbose_ftl_compilation() -> bool {
        Options::with(|options| options.verbose_ftl_compilation)
    }

    /// `Options::verboseFTLCompilation() = value`.
    pub fn set_verbose_ftl_compilation(value: bool) {
        Options::with_mut(|options| options.verbose_ftl_compilation = value);
    }

    /// Opção `logCompilationChanges`.
    pub fn log_compilation_changes() -> bool {
        Options::with(|options| options.log_compilation_changes)
    }

    /// `Options::logCompilationChanges() = value`.
    pub fn set_log_compilation_changes(value: bool) {
        Options::with_mut(|options| options.log_compilation_changes = value);
    }

    /// Opção `printEachOSRExit`.
    pub fn print_each_osr_exit() -> bool {
        Options::with(|options| options.print_each_osr_exit)
    }

    /// `Options::printEachOSRExit() = value`.
    pub fn set_print_each_osr_exit(value: bool) {
        Options::with_mut(|options| options.print_each_osr_exit = value);
    }

    /// Opção `printEachDFGFTLInlineCall`.
    pub fn print_each_dfgftl_inline_call() -> bool {
        Options::with(|options| options.print_each_dfgftl_inline_call)
    }

    /// `Options::printEachDFGFTLInlineCall() = value`.
    pub fn set_print_each_dfgftl_inline_call(value: bool) {
        Options::with_mut(|options| options.print_each_dfgftl_inline_call = value);
    }

    /// Opção `useJITAsserts`.
    pub fn use_jit_asserts() -> bool {
        Options::with(|options| options.use_jit_asserts)
    }

    /// `Options::useJITAsserts() = value`.
    pub fn set_use_jit_asserts(value: bool) {
        Options::with_mut(|options| options.use_jit_asserts = value);
    }

    /// Opção `validateDoesGC`.
    pub fn validate_does_gc() -> bool {
        Options::with(|options| options.validate_does_gc)
    }

    /// `Options::validateDoesGC() = value`.
    pub fn set_validate_does_gc(value: bool) {
        Options::with_mut(|options| options.validate_does_gc = value);
    }

    /// Opção `validateGraph`.
    pub fn validate_graph() -> bool {
        Options::with(|options| options.validate_graph)
    }

    /// `Options::validateGraph() = value`.
    pub fn set_validate_graph(value: bool) {
        Options::with_mut(|options| options.validate_graph = value);
    }

    /// Opção `validateGraphAtEachPhase`.
    pub fn validate_graph_at_each_phase() -> bool {
        Options::with(|options| options.validate_graph_at_each_phase)
    }

    /// `Options::validateGraphAtEachPhase() = value`.
    pub fn set_validate_graph_at_each_phase(value: bool) {
        Options::with_mut(|options| options.validate_graph_at_each_phase = value);
    }

    /// Opção `verboseValidationFailure`.
    pub fn verbose_validation_failure() -> bool {
        Options::with(|options| options.verbose_validation_failure)
    }

    /// `Options::verboseValidationFailure() = value`.
    pub fn set_verbose_validation_failure(value: bool) {
        Options::with_mut(|options| options.verbose_validation_failure = value);
    }

    /// Opção `verboseOSR`.
    pub fn verbose_osr() -> bool {
        Options::with(|options| options.verbose_osr)
    }

    /// `Options::verboseOSR() = value`.
    pub fn set_verbose_osr(value: bool) {
        Options::with_mut(|options| options.verbose_osr = value);
    }

    /// Opção `verboseDFGOSRExit`.
    pub fn verbose_dfgosr_exit() -> bool {
        Options::with(|options| options.verbose_dfgosr_exit)
    }

    /// `Options::verboseDFGOSRExit() = value`.
    pub fn set_verbose_dfgosr_exit(value: bool) {
        Options::with_mut(|options| options.verbose_dfgosr_exit = value);
    }

    /// Opção `verboseFTLOSRExit`.
    pub fn verbose_ftlosr_exit() -> bool {
        Options::with(|options| options.verbose_ftlosr_exit)
    }

    /// `Options::verboseFTLOSRExit() = value`.
    pub fn set_verbose_ftlosr_exit(value: bool) {
        Options::with_mut(|options| options.verbose_ftlosr_exit = value);
    }

    /// Opção `verboseCallLink`.
    pub fn verbose_call_link() -> bool {
        Options::with(|options| options.verbose_call_link)
    }

    /// `Options::verboseCallLink() = value`.
    pub fn set_verbose_call_link(value: bool) {
        Options::with_mut(|options| options.verbose_call_link = value);
    }

    /// Opção `verboseCompilationQueue`.
    pub fn verbose_compilation_queue() -> bool {
        Options::with(|options| options.verbose_compilation_queue)
    }

    /// `Options::verboseCompilationQueue() = value`.
    pub fn set_verbose_compilation_queue(value: bool) {
        Options::with_mut(|options| options.verbose_compilation_queue = value);
    }

    /// Opção `reportCompileTimes`.
    pub fn report_compile_times() -> bool {
        Options::with(|options| options.report_compile_times)
    }

    /// `Options::reportCompileTimes() = value`.
    pub fn set_report_compile_times(value: bool) {
        Options::with_mut(|options| options.report_compile_times = value);
    }

    /// Opção `reportBaselineCompileTimes`.
    pub fn report_baseline_compile_times() -> bool {
        Options::with(|options| options.report_baseline_compile_times)
    }

    /// `Options::reportBaselineCompileTimes() = value`.
    pub fn set_report_baseline_compile_times(value: bool) {
        Options::with_mut(|options| options.report_baseline_compile_times = value);
    }

    /// Opção `reportDFGCompileTimes`.
    pub fn report_dfg_compile_times() -> bool {
        Options::with(|options| options.report_dfg_compile_times)
    }

    /// `Options::reportDFGCompileTimes() = value`.
    pub fn set_report_dfg_compile_times(value: bool) {
        Options::with_mut(|options| options.report_dfg_compile_times = value);
    }

    /// Opção `reportFTLCompileTimes`.
    pub fn report_ftl_compile_times() -> bool {
        Options::with(|options| options.report_ftl_compile_times)
    }

    /// `Options::reportFTLCompileTimes() = value`.
    pub fn set_report_ftl_compile_times(value: bool) {
        Options::with_mut(|options| options.report_ftl_compile_times = value);
    }

    /// Opção `reportTotalCompileTimes`.
    pub fn report_total_compile_times() -> bool {
        Options::with(|options| options.report_total_compile_times)
    }

    /// `Options::reportTotalCompileTimes() = value`.
    pub fn set_report_total_compile_times(value: bool) {
        Options::with_mut(|options| options.report_total_compile_times = value);
    }

    /// Opção `reportTotalPhaseTimes`.
    pub fn report_total_phase_times() -> bool {
        Options::with(|options| options.report_total_phase_times)
    }

    /// `Options::reportTotalPhaseTimes() = value`.
    pub fn set_report_total_phase_times(value: bool) {
        Options::with_mut(|options| options.report_total_phase_times = value);
    }

    /// Opção `reportParseTimes`.
    pub fn report_parse_times() -> bool {
        Options::with(|options| options.report_parse_times)
    }

    /// `Options::reportParseTimes() = value`.
    pub fn set_report_parse_times(value: bool) {
        Options::with_mut(|options| options.report_parse_times = value);
    }

    /// Opção `reportBytecodeCompileTimes`.
    pub fn report_bytecode_compile_times() -> bool {
        Options::with(|options| options.report_bytecode_compile_times)
    }

    /// `Options::reportBytecodeCompileTimes() = value`.
    pub fn set_report_bytecode_compile_times(value: bool) {
        Options::with_mut(|options| options.report_bytecode_compile_times = value);
    }

    /// Opção `reportBytecodeCacheDecodeTimes`.
    pub fn report_bytecode_cache_decode_times() -> bool {
        Options::with(|options| options.report_bytecode_cache_decode_times)
    }

    /// `Options::reportBytecodeCacheDecodeTimes() = value`.
    pub fn set_report_bytecode_cache_decode_times(value: bool) {
        Options::with_mut(|options| options.report_bytecode_cache_decode_times = value);
    }

    /// Opção `countParseTimes`.
    pub fn count_parse_times() -> bool {
        Options::with(|options| options.count_parse_times)
    }

    /// `Options::countParseTimes() = value`.
    pub fn set_count_parse_times(value: bool) {
        Options::with_mut(|options| options.count_parse_times = value);
    }

    /// Opção `verboseExitProfile`.
    pub fn verbose_exit_profile() -> bool {
        Options::with(|options| options.verbose_exit_profile)
    }

    /// `Options::verboseExitProfile() = value`.
    pub fn set_verbose_exit_profile(value: bool) {
        Options::with_mut(|options| options.verbose_exit_profile = value);
    }

    /// Opção `verboseCFA`.
    pub fn verbose_cfa() -> bool {
        Options::with(|options| options.verbose_cfa)
    }

    /// `Options::verboseCFA() = value`.
    pub fn set_verbose_cfa(value: bool) {
        Options::with_mut(|options| options.verbose_cfa = value);
    }

    /// Opção `verboseDFGFailure`.
    pub fn verbose_dfg_failure() -> bool {
        Options::with(|options| options.verbose_dfg_failure)
    }

    /// `Options::verboseDFGFailure() = value`.
    pub fn set_verbose_dfg_failure(value: bool) {
        Options::with_mut(|options| options.verbose_dfg_failure = value);
    }

    /// Opção `verboseFTLToJSThunk`.
    pub fn verbose_ftl_to_js_thunk() -> bool {
        Options::with(|options| options.verbose_ftl_to_js_thunk)
    }

    /// `Options::verboseFTLToJSThunk() = value`.
    pub fn set_verbose_ftl_to_js_thunk(value: bool) {
        Options::with_mut(|options| options.verbose_ftl_to_js_thunk = value);
    }

    /// Opção `verboseFTLFailure`.
    pub fn verbose_ftl_failure() -> bool {
        Options::with(|options| options.verbose_ftl_failure)
    }

    /// `Options::verboseFTLFailure() = value`.
    pub fn set_verbose_ftl_failure(value: bool) {
        Options::with_mut(|options| options.verbose_ftl_failure = value);
    }

    /// Opção `testTheFTL`.
    pub fn test_the_ftl() -> bool {
        Options::with(|options| options.test_the_ftl)
    }

    /// `Options::testTheFTL() = value`.
    pub fn set_test_the_ftl(value: bool) {
        Options::with_mut(|options| options.test_the_ftl = value);
    }

    /// Opção `verboseSanitizeStack`.
    pub fn verbose_sanitize_stack() -> bool {
        Options::with(|options| options.verbose_sanitize_stack)
    }

    /// `Options::verboseSanitizeStack() = value`.
    pub fn set_verbose_sanitize_stack(value: bool) {
        Options::with_mut(|options| options.verbose_sanitize_stack = value);
    }

    /// Opção `useGenerationalGC`.
    pub fn use_generational_gc() -> bool {
        Options::with(|options| options.use_generational_gc)
    }

    /// `Options::useGenerationalGC() = value`.
    pub fn set_use_generational_gc(value: bool) {
        Options::with_mut(|options| options.use_generational_gc = value);
    }

    /// Opção `useConcurrentGC`.
    pub fn use_concurrent_gc() -> bool {
        Options::with(|options| options.use_concurrent_gc)
    }

    /// `Options::useConcurrentGC() = value`.
    pub fn set_use_concurrent_gc(value: bool) {
        Options::with_mut(|options| options.use_concurrent_gc = value);
    }

    /// Opção `collectContinuously`.
    pub fn collect_continuously() -> bool {
        Options::with(|options| options.collect_continuously)
    }

    /// `Options::collectContinuously() = value`.
    pub fn set_collect_continuously(value: bool) {
        Options::with_mut(|options| options.collect_continuously = value);
    }

    /// Opção `collectContinuouslyPeriodMS`.
    pub fn collect_continuously_period_ms() -> f64 {
        Options::with(|options| options.collect_continuously_period_ms)
    }

    /// `Options::collectContinuouslyPeriodMS() = value`.
    pub fn set_collect_continuously_period_ms(value: f64) {
        Options::with_mut(|options| options.collect_continuously_period_ms = value);
    }

    /// Opção `forceFencedBarrier`.
    pub fn force_fenced_barrier() -> bool {
        Options::with(|options| options.force_fenced_barrier)
    }

    /// `Options::forceFencedBarrier() = value`.
    pub fn set_force_fenced_barrier(value: bool) {
        Options::with_mut(|options| options.force_fenced_barrier = value);
    }

    /// Opção `verboseVisitRace`.
    pub fn verbose_visit_race() -> bool {
        Options::with(|options| options.verbose_visit_race)
    }

    /// `Options::verboseVisitRace() = value`.
    pub fn set_verbose_visit_race(value: bool) {
        Options::with_mut(|options| options.verbose_visit_race = value);
    }

    /// Opção `optimizeParallelSlotVisitorsForStoppedMutator`.
    pub fn optimize_parallel_slot_visitors_for_stopped_mutator() -> bool {
        Options::with(|options| options.optimize_parallel_slot_visitors_for_stopped_mutator)
    }

    /// `Options::optimizeParallelSlotVisitorsForStoppedMutator() = value`.
    pub fn set_optimize_parallel_slot_visitors_for_stopped_mutator(value: bool) {
        Options::with_mut(|options| options.optimize_parallel_slot_visitors_for_stopped_mutator = value);
    }

    /// Opção `verboseHeapSnapshotLogging`.
    pub fn verbose_heap_snapshot_logging() -> bool {
        Options::with(|options| options.verbose_heap_snapshot_logging)
    }

    /// `Options::verboseHeapSnapshotLogging() = value`.
    pub fn set_verbose_heap_snapshot_logging(value: bool) {
        Options::with_mut(|options| options.verbose_heap_snapshot_logging = value);
    }

    /// Opção `largeHeapSize`.
    pub fn large_heap_size() -> u32 {
        Options::with(|options| options.large_heap_size)
    }

    /// `Options::largeHeapSize() = value`.
    pub fn set_large_heap_size(value: u32) {
        Options::with_mut(|options| options.large_heap_size = value);
    }

    /// Opção `mediumHeapSize`.
    pub fn medium_heap_size() -> u32 {
        Options::with(|options| options.medium_heap_size)
    }

    /// `Options::mediumHeapSize() = value`.
    pub fn set_medium_heap_size(value: u32) {
        Options::with_mut(|options| options.medium_heap_size = value);
    }

    /// Opção `smallHeapSize`.
    pub fn small_heap_size() -> u32 {
        Options::with(|options| options.small_heap_size)
    }

    /// `Options::smallHeapSize() = value`.
    pub fn set_small_heap_size(value: u32) {
        Options::with_mut(|options| options.small_heap_size = value);
    }

    /// Opção `smallHeapRAMFraction`.
    pub fn small_heap_ram_fraction() -> f64 {
        Options::with(|options| options.small_heap_ram_fraction)
    }

    /// `Options::smallHeapRAMFraction() = value`.
    pub fn set_small_heap_ram_fraction(value: f64) {
        Options::with_mut(|options| options.small_heap_ram_fraction = value);
    }

    /// Opção `smallHeapGrowthFactor`.
    pub fn small_heap_growth_factor() -> f64 {
        Options::with(|options| options.small_heap_growth_factor)
    }

    /// `Options::smallHeapGrowthFactor() = value`.
    pub fn set_small_heap_growth_factor(value: f64) {
        Options::with_mut(|options| options.small_heap_growth_factor = value);
    }

    /// Opção `mediumHeapRAMFraction`.
    pub fn medium_heap_ram_fraction() -> f64 {
        Options::with(|options| options.medium_heap_ram_fraction)
    }

    /// `Options::mediumHeapRAMFraction() = value`.
    pub fn set_medium_heap_ram_fraction(value: f64) {
        Options::with_mut(|options| options.medium_heap_ram_fraction = value);
    }

    /// Opção `mediumHeapGrowthFactor`.
    pub fn medium_heap_growth_factor() -> f64 {
        Options::with(|options| options.medium_heap_growth_factor)
    }

    /// `Options::mediumHeapGrowthFactor() = value`.
    pub fn set_medium_heap_growth_factor(value: f64) {
        Options::with_mut(|options| options.medium_heap_growth_factor = value);
    }

    /// Opção `largeHeapGrowthFactor`.
    pub fn large_heap_growth_factor() -> f64 {
        Options::with(|options| options.large_heap_growth_factor)
    }

    /// `Options::largeHeapGrowthFactor() = value`.
    pub fn set_large_heap_growth_factor(value: f64) {
        Options::with_mut(|options| options.large_heap_growth_factor = value);
    }

    /// Opção `miniVMHeapGrowthFactor`.
    pub fn mini_vm_heap_growth_factor() -> f64 {
        Options::with(|options| options.mini_vm_heap_growth_factor)
    }

    /// `Options::miniVMHeapGrowthFactor() = value`.
    pub fn set_mini_vm_heap_growth_factor(value: f64) {
        Options::with_mut(|options| options.mini_vm_heap_growth_factor = value);
    }

    /// Opção `heapGrowthSteepnessFactor`.
    pub fn heap_growth_steepness_factor() -> f64 {
        Options::with(|options| options.heap_growth_steepness_factor)
    }

    /// `Options::heapGrowthSteepnessFactor() = value`.
    pub fn set_heap_growth_steepness_factor(value: f64) {
        Options::with_mut(|options| options.heap_growth_steepness_factor = value);
    }

    /// Opção `heapGrowthMaxIncrease`.
    pub fn heap_growth_max_increase() -> f64 {
        Options::with(|options| options.heap_growth_max_increase)
    }

    /// `Options::heapGrowthMaxIncrease() = value`.
    pub fn set_heap_growth_max_increase(value: f64) {
        Options::with_mut(|options| options.heap_growth_max_increase = value);
    }

    /// Opção `minEdenToOldGenerationRatio`.
    pub fn min_eden_to_old_generation_ratio() -> f64 {
        Options::with(|options| options.min_eden_to_old_generation_ratio)
    }

    /// `Options::minEdenToOldGenerationRatio() = value`.
    pub fn set_min_eden_to_old_generation_ratio(value: f64) {
        Options::with_mut(|options| options.min_eden_to_old_generation_ratio = value);
    }

    /// Opção `heapGrowthFunctionThresholdInMB`.
    pub fn heap_growth_function_threshold_in_mb() -> u32 {
        Options::with(|options| options.heap_growth_function_threshold_in_mb)
    }

    /// `Options::heapGrowthFunctionThresholdInMB() = value`.
    pub fn set_heap_growth_function_threshold_in_mb(value: u32) {
        Options::with_mut(|options| options.heap_growth_function_threshold_in_mb = value);
    }

    /// Opção `criticalGCMemoryThreshold`.
    pub fn critical_gc_memory_threshold() -> f64 {
        Options::with(|options| options.critical_gc_memory_threshold)
    }

    /// `Options::criticalGCMemoryThreshold() = value`.
    pub fn set_critical_gc_memory_threshold(value: f64) {
        Options::with_mut(|options| options.critical_gc_memory_threshold = value);
    }

    /// Opção `customFullGCCallbackBailThreshold`.
    pub fn custom_full_gc_callback_bail_threshold() -> f64 {
        Options::with(|options| options.custom_full_gc_callback_bail_threshold)
    }

    /// `Options::customFullGCCallbackBailThreshold() = value`.
    pub fn set_custom_full_gc_callback_bail_threshold(value: f64) {
        Options::with_mut(|options| options.custom_full_gc_callback_bail_threshold = value);
    }

    /// Opção `minimumMutatorUtilization`.
    pub fn minimum_mutator_utilization() -> f64 {
        Options::with(|options| options.minimum_mutator_utilization)
    }

    /// `Options::minimumMutatorUtilization() = value`.
    pub fn set_minimum_mutator_utilization(value: f64) {
        Options::with_mut(|options| options.minimum_mutator_utilization = value);
    }

    /// Opção `maximumMutatorUtilization`.
    pub fn maximum_mutator_utilization() -> f64 {
        Options::with(|options| options.maximum_mutator_utilization)
    }

    /// `Options::maximumMutatorUtilization() = value`.
    pub fn set_maximum_mutator_utilization(value: f64) {
        Options::with_mut(|options| options.maximum_mutator_utilization = value);
    }

    /// Opção `epsilonMutatorUtilization`.
    pub fn epsilon_mutator_utilization() -> f64 {
        Options::with(|options| options.epsilon_mutator_utilization)
    }

    /// `Options::epsilonMutatorUtilization() = value`.
    pub fn set_epsilon_mutator_utilization(value: f64) {
        Options::with_mut(|options| options.epsilon_mutator_utilization = value);
    }

    /// Opção `concurrentGCMaxHeadroom`.
    pub fn concurrent_gc_max_headroom() -> f64 {
        Options::with(|options| options.concurrent_gc_max_headroom)
    }

    /// `Options::concurrentGCMaxHeadroom() = value`.
    pub fn set_concurrent_gc_max_headroom(value: f64) {
        Options::with_mut(|options| options.concurrent_gc_max_headroom = value);
    }

    /// Opção `concurrentGCPeriodMS`.
    pub fn concurrent_gc_period_ms() -> f64 {
        Options::with(|options| options.concurrent_gc_period_ms)
    }

    /// `Options::concurrentGCPeriodMS() = value`.
    pub fn set_concurrent_gc_period_ms(value: f64) {
        Options::with_mut(|options| options.concurrent_gc_period_ms = value);
    }

    /// Opção `useStochasticMutatorScheduler`.
    pub fn use_stochastic_mutator_scheduler() -> bool {
        Options::with(|options| options.use_stochastic_mutator_scheduler)
    }

    /// `Options::useStochasticMutatorScheduler() = value`.
    pub fn set_use_stochastic_mutator_scheduler(value: bool) {
        Options::with_mut(|options| options.use_stochastic_mutator_scheduler = value);
    }

    /// Opção `minimumGCPauseMS`.
    pub fn minimum_gc_pause_ms() -> f64 {
        Options::with(|options| options.minimum_gc_pause_ms)
    }

    /// `Options::minimumGCPauseMS() = value`.
    pub fn set_minimum_gc_pause_ms(value: f64) {
        Options::with_mut(|options| options.minimum_gc_pause_ms = value);
    }

    /// Opção `gcPauseScale`.
    pub fn gc_pause_scale() -> f64 {
        Options::with(|options| options.gc_pause_scale)
    }

    /// `Options::gcPauseScale() = value`.
    pub fn set_gc_pause_scale(value: f64) {
        Options::with_mut(|options| options.gc_pause_scale = value);
    }

    /// Opção `gcIncrementBytes`.
    pub fn gc_increment_bytes() -> f64 {
        Options::with(|options| options.gc_increment_bytes)
    }

    /// `Options::gcIncrementBytes() = value`.
    pub fn set_gc_increment_bytes(value: f64) {
        Options::with_mut(|options| options.gc_increment_bytes = value);
    }

    /// Opção `gcIncrementMaxBytes`.
    pub fn gc_increment_max_bytes() -> f64 {
        Options::with(|options| options.gc_increment_max_bytes)
    }

    /// `Options::gcIncrementMaxBytes() = value`.
    pub fn set_gc_increment_max_bytes(value: f64) {
        Options::with_mut(|options| options.gc_increment_max_bytes = value);
    }

    /// Opção `gcIncrementScale`.
    pub fn gc_increment_scale() -> f64 {
        Options::with(|options| options.gc_increment_scale)
    }

    /// `Options::gcIncrementScale() = value`.
    pub fn set_gc_increment_scale(value: f64) {
        Options::with_mut(|options| options.gc_increment_scale = value);
    }

    /// Opção `useWarmUpMarkedBlocks`.
    pub fn use_warm_up_marked_blocks() -> bool {
        Options::with(|options| options.use_warm_up_marked_blocks)
    }

    /// `Options::useWarmUpMarkedBlocks() = value`.
    pub fn set_use_warm_up_marked_blocks(value: bool) {
        Options::with_mut(|options| options.use_warm_up_marked_blocks = value);
    }

    /// Opção `warmUpMarkedBlockCount`.
    pub fn warm_up_marked_block_count() -> u32 {
        Options::with(|options| options.warm_up_marked_block_count)
    }

    /// `Options::warmUpMarkedBlockCount() = value`.
    pub fn set_warm_up_marked_block_count(value: u32) {
        Options::with_mut(|options| options.warm_up_marked_block_count = value);
    }

    /// Opção `warmUpMarkedBlockStartAfterBlocks`.
    pub fn warm_up_marked_block_start_after_blocks() -> u32 {
        Options::with(|options| options.warm_up_marked_block_start_after_blocks)
    }

    /// `Options::warmUpMarkedBlockStartAfterBlocks() = value`.
    pub fn set_warm_up_marked_block_start_after_blocks(value: u32) {
        Options::with_mut(|options| options.warm_up_marked_block_start_after_blocks = value);
    }

    /// Opção `warmUpMarkedBlockIdleTimeout`.
    pub fn warm_up_marked_block_idle_timeout() -> f64 {
        Options::with(|options| options.warm_up_marked_block_idle_timeout)
    }

    /// `Options::warmUpMarkedBlockIdleTimeout() = value`.
    pub fn set_warm_up_marked_block_idle_timeout(value: f64) {
        Options::with_mut(|options| options.warm_up_marked_block_idle_timeout = value);
    }

    /// Opção `scribbleFreeCells`.
    pub fn scribble_free_cells() -> bool {
        Options::with(|options| options.scribble_free_cells)
    }

    /// `Options::scribbleFreeCells() = value`.
    pub fn set_scribble_free_cells(value: bool) {
        Options::with_mut(|options| options.scribble_free_cells = value);
    }

    /// Opção `decommitUnusedMarkedBlockPages`.
    pub fn decommit_unused_marked_block_pages() -> bool {
        Options::with(|options| options.decommit_unused_marked_block_pages)
    }

    /// `Options::decommitUnusedMarkedBlockPages() = value`.
    pub fn set_decommit_unused_marked_block_pages(value: bool) {
        Options::with_mut(|options| options.decommit_unused_marked_block_pages = value);
    }

    /// Opção `decommitUnusedMarkedBlockPagesAfterEdenCollections`.
    pub fn decommit_unused_marked_block_pages_after_eden_collections() -> bool {
        Options::with(|options| options.decommit_unused_marked_block_pages_after_eden_collections)
    }

    /// `Options::decommitUnusedMarkedBlockPagesAfterEdenCollections() = value`.
    pub fn set_decommit_unused_marked_block_pages_after_eden_collections(value: bool) {
        Options::with_mut(|options| options.decommit_unused_marked_block_pages_after_eden_collections = value);
    }

    /// Opção `sizeClassProgression`.
    pub fn size_class_progression() -> f64 {
        Options::with(|options| options.size_class_progression)
    }

    /// `Options::sizeClassProgression() = value`.
    pub fn set_size_class_progression(value: f64) {
        Options::with_mut(|options| options.size_class_progression = value);
    }

    /// Opção `preciseAllocationCutoff`.
    pub fn precise_allocation_cutoff() -> u32 {
        Options::with(|options| options.precise_allocation_cutoff)
    }

    /// `Options::preciseAllocationCutoff() = value`.
    pub fn set_precise_allocation_cutoff(value: u32) {
        Options::with_mut(|options| options.precise_allocation_cutoff = value);
    }

    /// Opção `dumpSizeClasses`.
    pub fn dump_size_classes() -> bool {
        Options::with(|options| options.dump_size_classes)
    }

    /// `Options::dumpSizeClasses() = value`.
    pub fn set_dump_size_classes(value: bool) {
        Options::with_mut(|options| options.dump_size_classes = value);
    }

    /// Opção `stealEmptyBlocksFromOtherAllocators`.
    pub fn steal_empty_blocks_from_other_allocators() -> bool {
        Options::with(|options| options.steal_empty_blocks_from_other_allocators)
    }

    /// `Options::stealEmptyBlocksFromOtherAllocators() = value`.
    pub fn set_steal_empty_blocks_from_other_allocators(value: bool) {
        Options::with_mut(|options| options.steal_empty_blocks_from_other_allocators = value);
    }

    /// Opção `eagerlyUpdateTopCallFrame`.
    pub fn eagerly_update_top_call_frame() -> bool {
        Options::with(|options| options.eagerly_update_top_call_frame)
    }

    /// `Options::eagerlyUpdateTopCallFrame() = value`.
    pub fn set_eagerly_update_top_call_frame(value: bool) {
        Options::with_mut(|options| options.eagerly_update_top_call_frame = value);
    }

    /// Opção `dumpZappedCellCrashData`.
    pub fn dump_zapped_cell_crash_data() -> bool {
        Options::with(|options| options.dump_zapped_cell_crash_data)
    }

    /// `Options::dumpZappedCellCrashData() = value`.
    pub fn set_dump_zapped_cell_crash_data(value: bool) {
        Options::with_mut(|options| options.dump_zapped_cell_crash_data = value);
    }

    /// Opção `useOSREntryToDFG`.
    pub fn use_osr_entry_to_dfg() -> bool {
        Options::with(|options| options.use_osr_entry_to_dfg)
    }

    /// `Options::useOSREntryToDFG() = value`.
    pub fn set_use_osr_entry_to_dfg(value: bool) {
        Options::with_mut(|options| options.use_osr_entry_to_dfg = value);
    }

    /// Opção `useOSREntryToFTL`.
    pub fn use_osr_entry_to_ftl() -> bool {
        Options::with(|options| options.use_osr_entry_to_ftl)
    }

    /// `Options::useOSREntryToFTL() = value`.
    pub fn set_use_osr_entry_to_ftl(value: bool) {
        Options::with_mut(|options| options.use_osr_entry_to_ftl = value);
    }

    /// Opção `useFTLJIT`.
    pub fn use_ftljit() -> bool {
        Options::with(|options| options.use_ftljit)
    }

    /// `Options::useFTLJIT() = value`.
    pub fn set_use_ftljit(value: bool) {
        Options::with_mut(|options| options.use_ftljit = value);
    }

    /// Opção `validateFTLOSRExitLiveness`.
    pub fn validate_ftlosr_exit_liveness() -> bool {
        Options::with(|options| options.validate_ftlosr_exit_liveness)
    }

    /// `Options::validateFTLOSRExitLiveness() = value`.
    pub fn set_validate_ftlosr_exit_liveness(value: bool) {
        Options::with_mut(|options| options.validate_ftlosr_exit_liveness = value);
    }

    /// Opção `poisonDeadOSRExitVariables`.
    pub fn poison_dead_osr_exit_variables() -> bool {
        Options::with(|options| options.poison_dead_osr_exit_variables)
    }

    /// `Options::poisonDeadOSRExitVariables() = value`.
    pub fn set_poison_dead_osr_exit_variables(value: bool) {
        Options::with_mut(|options| options.poison_dead_osr_exit_variables = value);
    }

    /// Opção `defaultB3OptLevel`.
    pub fn default_b3_opt_level() -> u32 {
        Options::with(|options| options.default_b3_opt_level)
    }

    /// `Options::defaultB3OptLevel() = value`.
    pub fn set_default_b3_opt_level(value: u32) {
        Options::with_mut(|options| options.default_b3_opt_level = value);
    }

    /// Opção `b3AlwaysFailsBeforeCompile`.
    pub fn b3_always_fails_before_compile() -> bool {
        Options::with(|options| options.b3_always_fails_before_compile)
    }

    /// `Options::b3AlwaysFailsBeforeCompile() = value`.
    pub fn set_b3_always_fails_before_compile(value: bool) {
        Options::with_mut(|options| options.b3_always_fails_before_compile = value);
    }

    /// Opção `b3AlwaysFailsBeforeLink`.
    pub fn b3_always_fails_before_link() -> bool {
        Options::with(|options| options.b3_always_fails_before_link)
    }

    /// `Options::b3AlwaysFailsBeforeLink() = value`.
    pub fn set_b3_always_fails_before_link(value: bool) {
        Options::with_mut(|options| options.b3_always_fails_before_link = value);
    }

    /// Opção `validateSerializedValue`.
    pub fn validate_serialized_value() -> bool {
        Options::with(|options| options.validate_serialized_value)
    }

    /// `Options::validateSerializedValue() = value`.
    pub fn set_validate_serialized_value(value: bool) {
        Options::with_mut(|options| options.validate_serialized_value = value);
    }

    /// Opção `ftlCrashes`.
    pub fn ftl_crashes() -> bool {
        Options::with(|options| options.ftl_crashes)
    }

    /// `Options::ftlCrashes() = value`.
    pub fn set_ftl_crashes(value: bool) {
        Options::with_mut(|options| options.ftl_crashes = value);
    }

    /// Opção `clobberAllRegsInFTLICSlowPath`.
    pub fn clobber_all_regs_in_ftlic_slow_path() -> bool {
        Options::with(|options| options.clobber_all_regs_in_ftlic_slow_path)
    }

    /// `Options::clobberAllRegsInFTLICSlowPath() = value`.
    pub fn set_clobber_all_regs_in_ftlic_slow_path(value: bool) {
        Options::with_mut(|options| options.clobber_all_regs_in_ftlic_slow_path = value);
    }

    /// Opção `useJITDebugAssertions`.
    pub fn use_jit_debug_assertions() -> bool {
        Options::with(|options| options.use_jit_debug_assertions)
    }

    /// `Options::useJITDebugAssertions() = value`.
    pub fn set_use_jit_debug_assertions(value: bool) {
        Options::with_mut(|options| options.use_jit_debug_assertions = value);
    }

    /// Opção `useAccessInlining`.
    pub fn use_access_inlining() -> bool {
        Options::with(|options| options.use_access_inlining)
    }

    /// `Options::useAccessInlining() = value`.
    pub fn set_use_access_inlining(value: bool) {
        Options::with_mut(|options| options.use_access_inlining = value);
    }

    /// Opção `maxAccessVariantListSize`.
    pub fn max_access_variant_list_size() -> u32 {
        Options::with(|options| options.max_access_variant_list_size)
    }

    /// `Options::maxAccessVariantListSize() = value`.
    pub fn set_max_access_variant_list_size(value: u32) {
        Options::with_mut(|options| options.max_access_variant_list_size = value);
    }

    /// Opção `thresholdForUndesiredMegamorphicAccessVariantListSize`.
    pub fn threshold_for_undesired_megamorphic_access_variant_list_size() -> f64 {
        Options::with(|options| options.threshold_for_undesired_megamorphic_access_variant_list_size)
    }

    /// `Options::thresholdForUndesiredMegamorphicAccessVariantListSize() = value`.
    pub fn set_threshold_for_undesired_megamorphic_access_variant_list_size(value: f64) {
        Options::with_mut(|options| options.threshold_for_undesired_megamorphic_access_variant_list_size = value);
    }

    /// Opção `usePolyvariantDevirtualization`.
    pub fn use_polyvariant_devirtualization() -> bool {
        Options::with(|options| options.use_polyvariant_devirtualization)
    }

    /// `Options::usePolyvariantDevirtualization() = value`.
    pub fn set_use_polyvariant_devirtualization(value: bool) {
        Options::with_mut(|options| options.use_polyvariant_devirtualization = value);
    }

    /// Opção `usePolymorphicAccessInlining`.
    pub fn use_polymorphic_access_inlining() -> bool {
        Options::with(|options| options.use_polymorphic_access_inlining)
    }

    /// `Options::usePolymorphicAccessInlining() = value`.
    pub fn set_use_polymorphic_access_inlining(value: bool) {
        Options::with_mut(|options| options.use_polymorphic_access_inlining = value);
    }

    /// Opção `maxPolymorphicAccessInliningListSize`.
    pub fn max_polymorphic_access_inlining_list_size() -> u32 {
        Options::with(|options| options.max_polymorphic_access_inlining_list_size)
    }

    /// `Options::maxPolymorphicAccessInliningListSize() = value`.
    pub fn set_max_polymorphic_access_inlining_list_size(value: u32) {
        Options::with_mut(|options| options.max_polymorphic_access_inlining_list_size = value);
    }

    /// Opção `usePolymorphicCallInlining`.
    pub fn use_polymorphic_call_inlining() -> bool {
        Options::with(|options| options.use_polymorphic_call_inlining)
    }

    /// `Options::usePolymorphicCallInlining() = value`.
    pub fn set_use_polymorphic_call_inlining(value: bool) {
        Options::with_mut(|options| options.use_polymorphic_call_inlining = value);
    }

    /// Opção `usePolymorphicCallInliningForNonStubStatus`.
    pub fn use_polymorphic_call_inlining_for_non_stub_status() -> bool {
        Options::with(|options| options.use_polymorphic_call_inlining_for_non_stub_status)
    }

    /// `Options::usePolymorphicCallInliningForNonStubStatus() = value`.
    pub fn set_use_polymorphic_call_inlining_for_non_stub_status(value: bool) {
        Options::with_mut(|options| options.use_polymorphic_call_inlining_for_non_stub_status = value);
    }

    /// Opção `maxPolymorphicCallVariantListSize`.
    pub fn max_polymorphic_call_variant_list_size() -> u32 {
        Options::with(|options| options.max_polymorphic_call_variant_list_size)
    }

    /// `Options::maxPolymorphicCallVariantListSize() = value`.
    pub fn set_max_polymorphic_call_variant_list_size(value: u32) {
        Options::with_mut(|options| options.max_polymorphic_call_variant_list_size = value);
    }

    /// Opção `maxPolymorphicCallVariantListSizeForTopTier`.
    pub fn max_polymorphic_call_variant_list_size_for_top_tier() -> u32 {
        Options::with(|options| options.max_polymorphic_call_variant_list_size_for_top_tier)
    }

    /// `Options::maxPolymorphicCallVariantListSizeForTopTier() = value`.
    pub fn set_max_polymorphic_call_variant_list_size_for_top_tier(value: u32) {
        Options::with_mut(|options| options.max_polymorphic_call_variant_list_size_for_top_tier = value);
    }

    /// Opção `maxPolymorphicCallVariantListSizeForWasmToJS`.
    pub fn max_polymorphic_call_variant_list_size_for_wasm_to_js() -> u32 {
        Options::with(|options| options.max_polymorphic_call_variant_list_size_for_wasm_to_js)
    }

    /// `Options::maxPolymorphicCallVariantListSizeForWasmToJS() = value`.
    pub fn set_max_polymorphic_call_variant_list_size_for_wasm_to_js(value: u32) {
        Options::with_mut(|options| options.max_polymorphic_call_variant_list_size_for_wasm_to_js = value);
    }

    /// Opção `maxPolymorphicCallVariantsForInlining`.
    pub fn max_polymorphic_call_variants_for_inlining() -> u32 {
        Options::with(|options| options.max_polymorphic_call_variants_for_inlining)
    }

    /// `Options::maxPolymorphicCallVariantsForInlining() = value`.
    pub fn set_max_polymorphic_call_variants_for_inlining(value: u32) {
        Options::with_mut(|options| options.max_polymorphic_call_variants_for_inlining = value);
    }

    /// Opção `frequentCallThreshold`.
    pub fn frequent_call_threshold() -> u32 {
        Options::with(|options| options.frequent_call_threshold)
    }

    /// `Options::frequentCallThreshold() = value`.
    pub fn set_frequent_call_threshold(value: u32) {
        Options::with_mut(|options| options.frequent_call_threshold = value);
    }

    /// Opção `minimumCallToKnownRate`.
    pub fn minimum_call_to_known_rate() -> f64 {
        Options::with(|options| options.minimum_call_to_known_rate)
    }

    /// `Options::minimumCallToKnownRate() = value`.
    pub fn set_minimum_call_to_known_rate(value: f64) {
        Options::with_mut(|options| options.minimum_call_to_known_rate = value);
    }

    /// Opção `createPreHeaders`.
    pub fn create_pre_headers() -> bool {
        Options::with(|options| options.create_pre_headers)
    }

    /// `Options::createPreHeaders() = value`.
    pub fn set_create_pre_headers(value: bool) {
        Options::with_mut(|options| options.create_pre_headers = value);
    }

    /// Opção `useMovHintRemoval`.
    pub fn use_mov_hint_removal() -> bool {
        Options::with(|options| options.use_mov_hint_removal)
    }

    /// `Options::useMovHintRemoval() = value`.
    pub fn set_use_mov_hint_removal(value: bool) {
        Options::with_mut(|options| options.use_mov_hint_removal = value);
    }

    /// Opção `usePutStackSinking`.
    pub fn use_put_stack_sinking() -> bool {
        Options::with(|options| options.use_put_stack_sinking)
    }

    /// `Options::usePutStackSinking() = value`.
    pub fn set_use_put_stack_sinking(value: bool) {
        Options::with_mut(|options| options.use_put_stack_sinking = value);
    }

    /// Opção `useObjectAllocationSinking`.
    pub fn use_object_allocation_sinking() -> bool {
        Options::with(|options| options.use_object_allocation_sinking)
    }

    /// `Options::useObjectAllocationSinking() = value`.
    pub fn set_use_object_allocation_sinking(value: bool) {
        Options::with_mut(|options| options.use_object_allocation_sinking = value);
    }

    /// Opção `verboseObjectAllocationSinking`.
    pub fn verbose_object_allocation_sinking() -> bool {
        Options::with(|options| options.verbose_object_allocation_sinking)
    }

    /// `Options::verboseObjectAllocationSinking() = value`.
    pub fn set_verbose_object_allocation_sinking(value: bool) {
        Options::with_mut(|options| options.verbose_object_allocation_sinking = value);
    }

    /// Opção `useValueRepElimination`.
    pub fn use_value_rep_elimination() -> bool {
        Options::with(|options| options.use_value_rep_elimination)
    }

    /// `Options::useValueRepElimination() = value`.
    pub fn set_use_value_rep_elimination(value: bool) {
        Options::with_mut(|options| options.use_value_rep_elimination = value);
    }

    /// Opção `useArityFixupInlining`.
    pub fn use_arity_fixup_inlining() -> bool {
        Options::with(|options| options.use_arity_fixup_inlining)
    }

    /// `Options::useArityFixupInlining() = value`.
    pub fn set_use_arity_fixup_inlining(value: bool) {
        Options::with_mut(|options| options.use_arity_fixup_inlining = value);
    }

    /// Opção `logExecutableAllocation`.
    pub fn log_executable_allocation() -> bool {
        Options::with(|options| options.log_executable_allocation)
    }

    /// `Options::logExecutableAllocation() = value`.
    pub fn set_log_executable_allocation(value: bool) {
        Options::with_mut(|options| options.log_executable_allocation = value);
    }

    /// Opção `maxDFGNodesInBasicBlockForPreciseAnalysis`.
    pub fn max_dfg_nodes_in_basic_block_for_precise_analysis() -> u32 {
        Options::with(|options| options.max_dfg_nodes_in_basic_block_for_precise_analysis)
    }

    /// `Options::maxDFGNodesInBasicBlockForPreciseAnalysis() = value`.
    pub fn set_max_dfg_nodes_in_basic_block_for_precise_analysis(value: u32) {
        Options::with_mut(|options| options.max_dfg_nodes_in_basic_block_for_precise_analysis = value);
    }

    /// Opção `useConcurrentJIT`.
    pub fn use_concurrent_jit() -> bool {
        Options::with(|options| options.use_concurrent_jit)
    }

    /// `Options::useConcurrentJIT() = value`.
    pub fn set_use_concurrent_jit(value: bool) {
        Options::with_mut(|options| options.use_concurrent_jit = value);
    }

    /// Opção `minNumberOfWorklistThreads`.
    pub fn min_number_of_worklist_threads() -> u32 {
        Options::with(|options| options.min_number_of_worklist_threads)
    }

    /// `Options::minNumberOfWorklistThreads() = value`.
    pub fn set_min_number_of_worklist_threads(value: u32) {
        Options::with_mut(|options| options.min_number_of_worklist_threads = value);
    }

    /// Opção `maxNumberOfWorklistThreads`.
    pub fn max_number_of_worklist_threads() -> u32 {
        Options::with(|options| options.max_number_of_worklist_threads)
    }

    /// `Options::maxNumberOfWorklistThreads() = value`.
    pub fn set_max_number_of_worklist_threads(value: u32) {
        Options::with_mut(|options| options.max_number_of_worklist_threads = value);
    }

    /// Opção `numberOfBaselineCompilerThreads`.
    pub fn number_of_baseline_compiler_threads() -> u32 {
        Options::with(|options| options.number_of_baseline_compiler_threads)
    }

    /// `Options::numberOfBaselineCompilerThreads() = value`.
    pub fn set_number_of_baseline_compiler_threads(value: u32) {
        Options::with_mut(|options| options.number_of_baseline_compiler_threads = value);
    }

    /// Opção `numberOfDFGCompilerThreads`.
    pub fn number_of_dfg_compiler_threads() -> u32 {
        Options::with(|options| options.number_of_dfg_compiler_threads)
    }

    /// `Options::numberOfDFGCompilerThreads() = value`.
    pub fn set_number_of_dfg_compiler_threads(value: u32) {
        Options::with_mut(|options| options.number_of_dfg_compiler_threads = value);
    }

    /// Opção `numberOfFTLCompilerThreads`.
    pub fn number_of_ftl_compiler_threads() -> u32 {
        Options::with(|options| options.number_of_ftl_compiler_threads)
    }

    /// `Options::numberOfFTLCompilerThreads() = value`.
    pub fn set_number_of_ftl_compiler_threads(value: u32) {
        Options::with_mut(|options| options.number_of_ftl_compiler_threads = value);
    }

    /// Opção `numberOfWasmCompilerThreads`.
    pub fn number_of_wasm_compiler_threads() -> u32 {
        Options::with(|options| options.number_of_wasm_compiler_threads)
    }

    /// `Options::numberOfWasmCompilerThreads() = value`.
    pub fn set_number_of_wasm_compiler_threads(value: u32) {
        Options::with_mut(|options| options.number_of_wasm_compiler_threads = value);
    }

    /// Opção `worklistLoadFactor`.
    pub fn worklist_load_factor() -> u32 {
        Options::with(|options| options.worklist_load_factor)
    }

    /// `Options::worklistLoadFactor() = value`.
    pub fn set_worklist_load_factor(value: u32) {
        Options::with_mut(|options| options.worklist_load_factor = value);
    }

    /// Opção `worklistBaselineLoadWeight`.
    pub fn worklist_baseline_load_weight() -> u32 {
        Options::with(|options| options.worklist_baseline_load_weight)
    }

    /// `Options::worklistBaselineLoadWeight() = value`.
    pub fn set_worklist_baseline_load_weight(value: u32) {
        Options::with_mut(|options| options.worklist_baseline_load_weight = value);
    }

    /// Opção `worklistDFGLoadWeight`.
    pub fn worklist_dfg_load_weight() -> u32 {
        Options::with(|options| options.worklist_dfg_load_weight)
    }

    /// `Options::worklistDFGLoadWeight() = value`.
    pub fn set_worklist_dfg_load_weight(value: u32) {
        Options::with_mut(|options| options.worklist_dfg_load_weight = value);
    }

    /// Opção `worklistFTLLoadWeight`.
    pub fn worklist_ftl_load_weight() -> u32 {
        Options::with(|options| options.worklist_ftl_load_weight)
    }

    /// `Options::worklistFTLLoadWeight() = value`.
    pub fn set_worklist_ftl_load_weight(value: u32) {
        Options::with_mut(|options| options.worklist_ftl_load_weight = value);
    }

    /// Opção `priorityDeltaOfDFGCompilerThreads`.
    pub fn priority_delta_of_dfg_compiler_threads() -> i32 {
        Options::with(|options| options.priority_delta_of_dfg_compiler_threads)
    }

    /// `Options::priorityDeltaOfDFGCompilerThreads() = value`.
    pub fn set_priority_delta_of_dfg_compiler_threads(value: i32) {
        Options::with_mut(|options| options.priority_delta_of_dfg_compiler_threads = value);
    }

    /// Opção `priorityDeltaOfFTLCompilerThreads`.
    pub fn priority_delta_of_ftl_compiler_threads() -> i32 {
        Options::with(|options| options.priority_delta_of_ftl_compiler_threads)
    }

    /// `Options::priorityDeltaOfFTLCompilerThreads() = value`.
    pub fn set_priority_delta_of_ftl_compiler_threads(value: i32) {
        Options::with_mut(|options| options.priority_delta_of_ftl_compiler_threads = value);
    }

    /// Opção `priorityDeltaOfWasmCompilerThreads`.
    pub fn priority_delta_of_wasm_compiler_threads() -> i32 {
        Options::with(|options| options.priority_delta_of_wasm_compiler_threads)
    }

    /// `Options::priorityDeltaOfWasmCompilerThreads() = value`.
    pub fn set_priority_delta_of_wasm_compiler_threads(value: i32) {
        Options::with_mut(|options| options.priority_delta_of_wasm_compiler_threads = value);
    }

    /// Opção `useProfiler`.
    pub fn use_profiler() -> bool {
        Options::with(|options| options.use_profiler)
    }

    /// `Options::useProfiler() = value`.
    pub fn set_use_profiler(value: bool) {
        Options::with_mut(|options| options.use_profiler = value);
    }

    /// Opção `dumpProfilerDataAtExit`.
    pub fn dump_profiler_data_at_exit() -> bool {
        Options::with(|options| options.dump_profiler_data_at_exit)
    }

    /// `Options::dumpProfilerDataAtExit() = value`.
    pub fn set_dump_profiler_data_at_exit(value: bool) {
        Options::with_mut(|options| options.dump_profiler_data_at_exit = value);
    }

    /// Opção `disassembleBaselineForProfiler`.
    pub fn disassemble_baseline_for_profiler() -> bool {
        Options::with(|options| options.disassemble_baseline_for_profiler)
    }

    /// `Options::disassembleBaselineForProfiler() = value`.
    pub fn set_disassemble_baseline_for_profiler(value: bool) {
        Options::with_mut(|options| options.disassemble_baseline_for_profiler = value);
    }

    /// Opção `abbreviateSourceCodeForProfiler`.
    pub fn abbreviate_source_code_for_profiler() -> u32 {
        Options::with(|options| options.abbreviate_source_code_for_profiler)
    }

    /// `Options::abbreviateSourceCodeForProfiler() = value`.
    pub fn set_abbreviate_source_code_for_profiler(value: u32) {
        Options::with_mut(|options| options.abbreviate_source_code_for_profiler = value);
    }

    /// Opção `useArchitectureSpecificOptimizations`.
    pub fn use_architecture_specific_optimizations() -> bool {
        Options::with(|options| options.use_architecture_specific_optimizations)
    }

    /// `Options::useArchitectureSpecificOptimizations() = value`.
    pub fn set_use_architecture_specific_optimizations(value: bool) {
        Options::with_mut(|options| options.use_architecture_specific_optimizations = value);
    }

    /// Opção `breakOnThrow`.
    pub fn break_on_throw() -> bool {
        Options::with(|options| options.break_on_throw)
    }

    /// `Options::breakOnThrow() = value`.
    pub fn set_break_on_throw(value: bool) {
        Options::with_mut(|options| options.break_on_throw = value);
    }

    /// Opção `maximumOptimizationCandidateBytecodeCost`.
    pub fn maximum_optimization_candidate_bytecode_cost() -> u32 {
        Options::with(|options| options.maximum_optimization_candidate_bytecode_cost)
    }

    /// `Options::maximumOptimizationCandidateBytecodeCost() = value`.
    pub fn set_maximum_optimization_candidate_bytecode_cost(value: u32) {
        Options::with_mut(|options| options.maximum_optimization_candidate_bytecode_cost = value);
    }

    /// Opção `maximumCachedAssemblerBufferSize`.
    pub fn maximum_cached_assembler_buffer_size() -> u32 {
        Options::with(|options| options.maximum_cached_assembler_buffer_size)
    }

    /// `Options::maximumCachedAssemblerBufferSize() = value`.
    pub fn set_maximum_cached_assembler_buffer_size(value: u32) {
        Options::with_mut(|options| options.maximum_cached_assembler_buffer_size = value);
    }

    /// Opção `maximumFunctionForCallInlineCandidateBytecodeCostForDFG`.
    pub fn maximum_function_for_call_inline_candidate_bytecode_cost_for_dfg() -> u32 {
        Options::with(|options| options.maximum_function_for_call_inline_candidate_bytecode_cost_for_dfg)
    }

    /// `Options::maximumFunctionForCallInlineCandidateBytecodeCostForDFG() = value`.
    pub fn set_maximum_function_for_call_inline_candidate_bytecode_cost_for_dfg(value: u32) {
        Options::with_mut(|options| options.maximum_function_for_call_inline_candidate_bytecode_cost_for_dfg = value);
    }

    /// Opção `maximumFunctionForClosureCallInlineCandidateBytecodeCostForDFG`.
    pub fn maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_dfg() -> u32 {
        Options::with(|options| options.maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_dfg)
    }

    /// `Options::maximumFunctionForClosureCallInlineCandidateBytecodeCostForDFG() = value`.
    pub fn set_maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_dfg(value: u32) {
        Options::with_mut(|options| options.maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_dfg = value);
    }

    /// Opção `maximumFunctionForConstructInlineCandidateBytecodeCostForDFG`.
    pub fn maximum_function_for_construct_inline_candidate_bytecode_cost_for_dfg() -> u32 {
        Options::with(|options| options.maximum_function_for_construct_inline_candidate_bytecode_cost_for_dfg)
    }

    /// `Options::maximumFunctionForConstructInlineCandidateBytecodeCostForDFG() = value`.
    pub fn set_maximum_function_for_construct_inline_candidate_bytecode_cost_for_dfg(value: u32) {
        Options::with_mut(|options| options.maximum_function_for_construct_inline_candidate_bytecode_cost_for_dfg = value);
    }

    /// Opção `maximumFunctionForCallInlineCandidateBytecodeCostForFTL`.
    pub fn maximum_function_for_call_inline_candidate_bytecode_cost_for_ftl() -> u32 {
        Options::with(|options| options.maximum_function_for_call_inline_candidate_bytecode_cost_for_ftl)
    }

    /// `Options::maximumFunctionForCallInlineCandidateBytecodeCostForFTL() = value`.
    pub fn set_maximum_function_for_call_inline_candidate_bytecode_cost_for_ftl(value: u32) {
        Options::with_mut(|options| options.maximum_function_for_call_inline_candidate_bytecode_cost_for_ftl = value);
    }

    /// Opção `maximumFunctionForClosureCallInlineCandidateBytecodeCostForFTL`.
    pub fn maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_ftl() -> u32 {
        Options::with(|options| options.maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_ftl)
    }

    /// `Options::maximumFunctionForClosureCallInlineCandidateBytecodeCostForFTL() = value`.
    pub fn set_maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_ftl(value: u32) {
        Options::with_mut(|options| options.maximum_function_for_closure_call_inline_candidate_bytecode_cost_for_ftl = value);
    }

    /// Opção `maximumFunctionForConstructInlineCandidateBytecodeCostForFTL`.
    pub fn maximum_function_for_construct_inline_candidate_bytecode_cost_for_ftl() -> u32 {
        Options::with(|options| options.maximum_function_for_construct_inline_candidate_bytecode_cost_for_ftl)
    }

    /// `Options::maximumFunctionForConstructInlineCandidateBytecodeCostForFTL() = value`.
    pub fn set_maximum_function_for_construct_inline_candidate_bytecode_cost_for_ftl(value: u32) {
        Options::with_mut(|options| options.maximum_function_for_construct_inline_candidate_bytecode_cost_for_ftl = value);
    }

    /// Opção `maximumFTLCandidateBytecodeCost`.
    pub fn maximum_ftl_candidate_bytecode_cost() -> u32 {
        Options::with(|options| options.maximum_ftl_candidate_bytecode_cost)
    }

    /// `Options::maximumFTLCandidateBytecodeCost() = value`.
    pub fn set_maximum_ftl_candidate_bytecode_cost(value: u32) {
        Options::with_mut(|options| options.maximum_ftl_candidate_bytecode_cost = value);
    }

    /// Opção `ratioFTLNodesToBytecodeCost`.
    pub fn ratio_ftl_nodes_to_bytecode_cost() -> f64 {
        Options::with(|options| options.ratio_ftl_nodes_to_bytecode_cost)
    }

    /// `Options::ratioFTLNodesToBytecodeCost() = value`.
    pub fn set_ratio_ftl_nodes_to_bytecode_cost(value: f64) {
        Options::with_mut(|options| options.ratio_ftl_nodes_to_bytecode_cost = value);
    }

    /// Opção `maximumInliningDepth`.
    pub fn maximum_inlining_depth() -> u32 {
        Options::with(|options| options.maximum_inlining_depth)
    }

    /// `Options::maximumInliningDepth() = value`.
    pub fn set_maximum_inlining_depth(value: u32) {
        Options::with_mut(|options| options.maximum_inlining_depth = value);
    }

    /// Opção `maximumInliningRecursion`.
    pub fn maximum_inlining_recursion() -> u32 {
        Options::with(|options| options.maximum_inlining_recursion)
    }

    /// `Options::maximumInliningRecursion() = value`.
    pub fn set_maximum_inlining_recursion(value: u32) {
        Options::with_mut(|options| options.maximum_inlining_recursion = value);
    }

    /// Opção `maximumInliningCallerBytecodeCost`.
    pub fn maximum_inlining_caller_bytecode_cost() -> u32 {
        Options::with(|options| options.maximum_inlining_caller_bytecode_cost)
    }

    /// `Options::maximumInliningCallerBytecodeCost() = value`.
    pub fn set_maximum_inlining_caller_bytecode_cost(value: u32) {
        Options::with_mut(|options| options.maximum_inlining_caller_bytecode_cost = value);
    }

    /// Opção `useGlobalInliningPlanner`.
    pub fn use_global_inlining_planner() -> bool {
        Options::with(|options| options.use_global_inlining_planner)
    }

    /// `Options::useGlobalInliningPlanner() = value`.
    pub fn set_use_global_inlining_planner(value: bool) {
        Options::with_mut(|options| options.use_global_inlining_planner = value);
    }

    /// Opção `globalInliningPlanBudgetForDFG`.
    pub fn global_inlining_plan_budget_for_dfg() -> u32 {
        Options::with(|options| options.global_inlining_plan_budget_for_dfg)
    }

    /// `Options::globalInliningPlanBudgetForDFG() = value`.
    pub fn set_global_inlining_plan_budget_for_dfg(value: u32) {
        Options::with_mut(|options| options.global_inlining_plan_budget_for_dfg = value);
    }

    /// Opção `globalInliningPlanBudgetForFTL`.
    pub fn global_inlining_plan_budget_for_ftl() -> u32 {
        Options::with(|options| options.global_inlining_plan_budget_for_ftl)
    }

    /// `Options::globalInliningPlanBudgetForFTL() = value`.
    pub fn set_global_inlining_plan_budget_for_ftl(value: u32) {
        Options::with_mut(|options| options.global_inlining_plan_budget_for_ftl = value);
    }

    /// Opção `maximumGlobalInliningPlanSites`.
    pub fn maximum_global_inlining_plan_sites() -> u32 {
        Options::with(|options| options.maximum_global_inlining_plan_sites)
    }

    /// `Options::maximumGlobalInliningPlanSites() = value`.
    pub fn set_maximum_global_inlining_plan_sites(value: u32) {
        Options::with_mut(|options| options.maximum_global_inlining_plan_sites = value);
    }

    /// Opção `inliningPlanTierBonusBase`.
    pub fn inlining_plan_tier_bonus_base() -> f64 {
        Options::with(|options| options.inlining_plan_tier_bonus_base)
    }

    /// `Options::inliningPlanTierBonusBase() = value`.
    pub fn set_inlining_plan_tier_bonus_base(value: f64) {
        Options::with_mut(|options| options.inlining_plan_tier_bonus_base = value);
    }

    /// Opção `inliningPlanTierBonusPowerForFTL`.
    pub fn inlining_plan_tier_bonus_power_for_ftl() -> f64 {
        Options::with(|options| options.inlining_plan_tier_bonus_power_for_ftl)
    }

    /// `Options::inliningPlanTierBonusPowerForFTL() = value`.
    pub fn set_inlining_plan_tier_bonus_power_for_ftl(value: f64) {
        Options::with_mut(|options| options.inlining_plan_tier_bonus_power_for_ftl = value);
    }

    /// Opção `inliningPlanTierBonusPowerForDFG`.
    pub fn inlining_plan_tier_bonus_power_for_dfg() -> f64 {
        Options::with(|options| options.inlining_plan_tier_bonus_power_for_dfg)
    }

    /// `Options::inliningPlanTierBonusPowerForDFG() = value`.
    pub fn set_inlining_plan_tier_bonus_power_for_dfg(value: f64) {
        Options::with_mut(|options| options.inlining_plan_tier_bonus_power_for_dfg = value);
    }

    /// Opção `inliningPlanTierBonusPowerForBaseline`.
    pub fn inlining_plan_tier_bonus_power_for_baseline() -> f64 {
        Options::with(|options| options.inlining_plan_tier_bonus_power_for_baseline)
    }

    /// `Options::inliningPlanTierBonusPowerForBaseline() = value`.
    pub fn set_inlining_plan_tier_bonus_power_for_baseline(value: f64) {
        Options::with_mut(|options| options.inlining_plan_tier_bonus_power_for_baseline = value);
    }

    /// Opção `inliningPlanDepthPenalty`.
    pub fn inlining_plan_depth_penalty() -> f64 {
        Options::with(|options| options.inlining_plan_depth_penalty)
    }

    /// `Options::inliningPlanDepthPenalty() = value`.
    pub fn set_inlining_plan_depth_penalty(value: f64) {
        Options::with_mut(|options| options.inlining_plan_depth_penalty = value);
    }

    /// Opção `maximumVarargsForInlining`.
    pub fn maximum_varargs_for_inlining() -> u32 {
        Options::with(|options| options.maximum_varargs_for_inlining)
    }

    /// `Options::maximumVarargsForInlining() = value`.
    pub fn set_maximum_varargs_for_inlining(value: u32) {
        Options::with_mut(|options| options.maximum_varargs_for_inlining = value);
    }

    /// Opção `maximumBinaryStringSwitchCaseLength`.
    pub fn maximum_binary_string_switch_case_length() -> u32 {
        Options::with(|options| options.maximum_binary_string_switch_case_length)
    }

    /// `Options::maximumBinaryStringSwitchCaseLength() = value`.
    pub fn set_maximum_binary_string_switch_case_length(value: u32) {
        Options::with_mut(|options| options.maximum_binary_string_switch_case_length = value);
    }

    /// Opção `maximumBinaryStringSwitchTotalLength`.
    pub fn maximum_binary_string_switch_total_length() -> u32 {
        Options::with(|options| options.maximum_binary_string_switch_total_length)
    }

    /// `Options::maximumBinaryStringSwitchTotalLength() = value`.
    pub fn set_maximum_binary_string_switch_total_length(value: u32) {
        Options::with_mut(|options| options.maximum_binary_string_switch_total_length = value);
    }

    /// Opção `maximumInlineStringSwitchCaseCount`.
    pub fn maximum_inline_string_switch_case_count() -> u32 {
        Options::with(|options| options.maximum_inline_string_switch_case_count)
    }

    /// `Options::maximumInlineStringSwitchCaseCount() = value`.
    pub fn set_maximum_inline_string_switch_case_count(value: u32) {
        Options::with_mut(|options| options.maximum_inline_string_switch_case_count = value);
    }

    /// Opção `maximumRegExpTestInlineCodesize`.
    pub fn maximum_reg_exp_test_inline_codesize() -> u32 {
        Options::with(|options| options.maximum_reg_exp_test_inline_codesize)
    }

    /// `Options::maximumRegExpTestInlineCodesize() = value`.
    pub fn set_maximum_reg_exp_test_inline_codesize(value: u32) {
        Options::with_mut(|options| options.maximum_reg_exp_test_inline_codesize = value);
    }

    /// Opção `maximumRegExpJITCodeSize`.
    pub fn maximum_reg_exp_jit_code_size() -> u32 {
        Options::with(|options| options.maximum_reg_exp_jit_code_size)
    }

    /// `Options::maximumRegExpJITCodeSize() = value`.
    pub fn set_maximum_reg_exp_jit_code_size(value: u32) {
        Options::with_mut(|options| options.maximum_reg_exp_jit_code_size = value);
    }

    /// Opção `wasmInliningMaximumDepth`.
    pub fn wasm_inlining_maximum_depth() -> u32 {
        Options::with(|options| options.wasm_inlining_maximum_depth)
    }

    /// `Options::wasmInliningMaximumDepth() = value`.
    pub fn set_wasm_inlining_maximum_depth(value: u32) {
        Options::with_mut(|options| options.wasm_inlining_maximum_depth = value);
    }

    /// Opção `wasmInliningMaximumWasmCalleeSize`.
    pub fn wasm_inlining_maximum_wasm_callee_size() -> u32 {
        Options::with(|options| options.wasm_inlining_maximum_wasm_callee_size)
    }

    /// `Options::wasmInliningMaximumWasmCalleeSize() = value`.
    pub fn set_wasm_inlining_maximum_wasm_callee_size(value: u32) {
        Options::with_mut(|options| options.wasm_inlining_maximum_wasm_callee_size = value);
    }

    /// Opção `wasmInliningMaximumCount`.
    pub fn wasm_inlining_maximum_count() -> u32 {
        Options::with(|options| options.wasm_inlining_maximum_count)
    }

    /// `Options::wasmInliningMaximumCount() = value`.
    pub fn set_wasm_inlining_maximum_count(value: u32) {
        Options::with_mut(|options| options.wasm_inlining_maximum_count = value);
    }

    /// Opção `wasmInliningMinimumBudget`.
    pub fn wasm_inlining_minimum_budget() -> u32 {
        Options::with(|options| options.wasm_inlining_minimum_budget)
    }

    /// `Options::wasmInliningMinimumBudget() = value`.
    pub fn set_wasm_inlining_minimum_budget(value: u32) {
        Options::with_mut(|options| options.wasm_inlining_minimum_budget = value);
    }

    /// Opção `wasmInliningFactor`.
    pub fn wasm_inlining_factor() -> u32 {
        Options::with(|options| options.wasm_inlining_factor)
    }

    /// `Options::wasmInliningFactor() = value`.
    pub fn set_wasm_inlining_factor(value: u32) {
        Options::with_mut(|options| options.wasm_inlining_factor = value);
    }

    /// Opção `wasmInliningBudget`.
    pub fn wasm_inlining_budget() -> u32 {
        Options::with(|options| options.wasm_inlining_budget)
    }

    /// `Options::wasmInliningBudget() = value`.
    pub fn set_wasm_inlining_budget(value: u32) {
        Options::with_mut(|options| options.wasm_inlining_budget = value);
    }

    /// Opção `wasmInliningLargeFunctionGrowthFactor`.
    pub fn wasm_inlining_large_function_growth_factor() -> f64 {
        Options::with(|options| options.wasm_inlining_large_function_growth_factor)
    }

    /// `Options::wasmInliningLargeFunctionGrowthFactor() = value`.
    pub fn set_wasm_inlining_large_function_growth_factor(value: f64) {
        Options::with_mut(|options| options.wasm_inlining_large_function_growth_factor = value);
    }

    /// Opção `wasmInliningTinyFunctionThreshold`.
    pub fn wasm_inlining_tiny_function_threshold() -> u32 {
        Options::with(|options| options.wasm_inlining_tiny_function_threshold)
    }

    /// `Options::wasmInliningTinyFunctionThreshold() = value`.
    pub fn set_wasm_inlining_tiny_function_threshold(value: u32) {
        Options::with_mut(|options| options.wasm_inlining_tiny_function_threshold = value);
    }

    /// Opção `wasmInliningSmallFunctionThreshold`.
    pub fn wasm_inlining_small_function_threshold() -> u32 {
        Options::with(|options| options.wasm_inlining_small_function_threshold)
    }

    /// `Options::wasmInliningSmallFunctionThreshold() = value`.
    pub fn set_wasm_inlining_small_function_threshold(value: u32) {
        Options::with_mut(|options| options.wasm_inlining_small_function_threshold = value);
    }

    /// Opção `jitPolicyScale`.
    pub fn jit_policy_scale() -> f64 {
        Options::with(|options| options.jit_policy_scale)
    }

    /// `Options::jitPolicyScale() = value`.
    pub fn set_jit_policy_scale(value: f64) {
        Options::with_mut(|options| options.jit_policy_scale = value);
    }

    /// Opção `numberOfSuperAndPerformanceCoresOverride`.
    pub fn number_of_super_and_performance_cores_override() -> i32 {
        Options::with(|options| options.number_of_super_and_performance_cores_override)
    }

    /// `Options::numberOfSuperAndPerformanceCoresOverride() = value`.
    pub fn set_number_of_super_and_performance_cores_override(value: i32) {
        Options::with_mut(|options| options.number_of_super_and_performance_cores_override = value);
    }

    /// Opção `dfgThresholdScaleForFewPerformanceCores`.
    pub fn dfg_threshold_scale_for_few_performance_cores() -> f64 {
        Options::with(|options| options.dfg_threshold_scale_for_few_performance_cores)
    }

    /// `Options::dfgThresholdScaleForFewPerformanceCores() = value`.
    pub fn set_dfg_threshold_scale_for_few_performance_cores(value: f64) {
        Options::with_mut(|options| options.dfg_threshold_scale_for_few_performance_cores = value);
    }

    /// Opção `ftlThresholdScaleForFewPerformanceCores`.
    pub fn ftl_threshold_scale_for_few_performance_cores() -> f64 {
        Options::with(|options| options.ftl_threshold_scale_for_few_performance_cores)
    }

    /// `Options::ftlThresholdScaleForFewPerformanceCores() = value`.
    pub fn set_ftl_threshold_scale_for_few_performance_cores(value: f64) {
        Options::with_mut(|options| options.ftl_threshold_scale_for_few_performance_cores = value);
    }

    /// Opção `forceEagerCompilation`.
    pub fn force_eager_compilation() -> bool {
        Options::with(|options| options.force_eager_compilation)
    }

    /// `Options::forceEagerCompilation() = value`.
    pub fn set_force_eager_compilation(value: bool) {
        Options::with_mut(|options| options.force_eager_compilation = value);
    }

    /// Opção `thresholdForJITAfterWarmUp`.
    pub fn threshold_for_jit_after_warm_up() -> i32 {
        Options::with(|options| options.threshold_for_jit_after_warm_up)
    }

    /// `Options::thresholdForJITAfterWarmUp() = value`.
    pub fn set_threshold_for_jit_after_warm_up(value: i32) {
        Options::with_mut(|options| options.threshold_for_jit_after_warm_up = value);
    }

    /// Opção `thresholdForJITSoon`.
    pub fn threshold_for_jit_soon() -> i32 {
        Options::with(|options| options.threshold_for_jit_soon)
    }

    /// `Options::thresholdForJITSoon() = value`.
    pub fn set_threshold_for_jit_soon(value: i32) {
        Options::with_mut(|options| options.threshold_for_jit_soon = value);
    }

    /// Opção `thresholdForOptimizeAfterWarmUp`.
    pub fn threshold_for_optimize_after_warm_up() -> i32 {
        Options::with(|options| options.threshold_for_optimize_after_warm_up)
    }

    /// `Options::thresholdForOptimizeAfterWarmUp() = value`.
    pub fn set_threshold_for_optimize_after_warm_up(value: i32) {
        Options::with_mut(|options| options.threshold_for_optimize_after_warm_up = value);
    }

    /// Opção `thresholdForOptimizeAfterLongWarmUp`.
    pub fn threshold_for_optimize_after_long_warm_up() -> i32 {
        Options::with(|options| options.threshold_for_optimize_after_long_warm_up)
    }

    /// `Options::thresholdForOptimizeAfterLongWarmUp() = value`.
    pub fn set_threshold_for_optimize_after_long_warm_up(value: i32) {
        Options::with_mut(|options| options.threshold_for_optimize_after_long_warm_up = value);
    }

    /// Opção `thresholdForOptimizeSoon`.
    pub fn threshold_for_optimize_soon() -> i32 {
        Options::with(|options| options.threshold_for_optimize_soon)
    }

    /// `Options::thresholdForOptimizeSoon() = value`.
    pub fn set_threshold_for_optimize_soon(value: i32) {
        Options::with_mut(|options| options.threshold_for_optimize_soon = value);
    }

    /// Opção `executionCounterIncrementForLoop`.
    pub fn execution_counter_increment_for_loop() -> i32 {
        Options::with(|options| options.execution_counter_increment_for_loop)
    }

    /// `Options::executionCounterIncrementForLoop() = value`.
    pub fn set_execution_counter_increment_for_loop(value: i32) {
        Options::with_mut(|options| options.execution_counter_increment_for_loop = value);
    }

    /// Opção `executionCounterIncrementForEntry`.
    pub fn execution_counter_increment_for_entry() -> i32 {
        Options::with(|options| options.execution_counter_increment_for_entry)
    }

    /// `Options::executionCounterIncrementForEntry() = value`.
    pub fn set_execution_counter_increment_for_entry(value: i32) {
        Options::with_mut(|options| options.execution_counter_increment_for_entry = value);
    }

    /// Opção `thresholdForFTLOptimizeAfterWarmUp`.
    pub fn threshold_for_ftl_optimize_after_warm_up() -> i32 {
        Options::with(|options| options.threshold_for_ftl_optimize_after_warm_up)
    }

    /// `Options::thresholdForFTLOptimizeAfterWarmUp() = value`.
    pub fn set_threshold_for_ftl_optimize_after_warm_up(value: i32) {
        Options::with_mut(|options| options.threshold_for_ftl_optimize_after_warm_up = value);
    }

    /// Opção `thresholdForFTLOptimizeSoon`.
    pub fn threshold_for_ftl_optimize_soon() -> i32 {
        Options::with(|options| options.threshold_for_ftl_optimize_soon)
    }

    /// `Options::thresholdForFTLOptimizeSoon() = value`.
    pub fn set_threshold_for_ftl_optimize_soon(value: i32) {
        Options::with_mut(|options| options.threshold_for_ftl_optimize_soon = value);
    }

    /// Opção `ftlTierUpCounterIncrementForLoop`.
    pub fn ftl_tier_up_counter_increment_for_loop() -> i32 {
        Options::with(|options| options.ftl_tier_up_counter_increment_for_loop)
    }

    /// `Options::ftlTierUpCounterIncrementForLoop() = value`.
    pub fn set_ftl_tier_up_counter_increment_for_loop(value: i32) {
        Options::with_mut(|options| options.ftl_tier_up_counter_increment_for_loop = value);
    }

    /// Opção `ftlTierUpCounterIncrementForReturn`.
    pub fn ftl_tier_up_counter_increment_for_return() -> i32 {
        Options::with(|options| options.ftl_tier_up_counter_increment_for_return)
    }

    /// `Options::ftlTierUpCounterIncrementForReturn() = value`.
    pub fn set_ftl_tier_up_counter_increment_for_return(value: i32) {
        Options::with_mut(|options| options.ftl_tier_up_counter_increment_for_return = value);
    }

    /// Opção `ftlOSREntryFailureCountForReoptimization`.
    pub fn ftl_osr_entry_failure_count_for_reoptimization() -> u32 {
        Options::with(|options| options.ftl_osr_entry_failure_count_for_reoptimization)
    }

    /// `Options::ftlOSREntryFailureCountForReoptimization() = value`.
    pub fn set_ftl_osr_entry_failure_count_for_reoptimization(value: u32) {
        Options::with_mut(|options| options.ftl_osr_entry_failure_count_for_reoptimization = value);
    }

    /// Opção `ftlOSREntryRetryThreshold`.
    pub fn ftl_osr_entry_retry_threshold() -> u32 {
        Options::with(|options| options.ftl_osr_entry_retry_threshold)
    }

    /// `Options::ftlOSREntryRetryThreshold() = value`.
    pub fn set_ftl_osr_entry_retry_threshold(value: u32) {
        Options::with_mut(|options| options.ftl_osr_entry_retry_threshold = value);
    }

    /// Opção `evalThresholdMultiplier`.
    pub fn eval_threshold_multiplier() -> i32 {
        Options::with(|options| options.eval_threshold_multiplier)
    }

    /// `Options::evalThresholdMultiplier() = value`.
    pub fn set_eval_threshold_multiplier(value: i32) {
        Options::with_mut(|options| options.eval_threshold_multiplier = value);
    }

    /// Opção `maximumEvalCacheableSourceLength`.
    pub fn maximum_eval_cacheable_source_length() -> u32 {
        Options::with(|options| options.maximum_eval_cacheable_source_length)
    }

    /// `Options::maximumEvalCacheableSourceLength() = value`.
    pub fn set_maximum_eval_cacheable_source_length(value: u32) {
        Options::with_mut(|options| options.maximum_eval_cacheable_source_length = value);
    }

    /// Opção `maximumExecutionCountsBetweenCheckpointsForBaseline`.
    pub fn maximum_execution_counts_between_checkpoints_for_baseline() -> i32 {
        Options::with(|options| options.maximum_execution_counts_between_checkpoints_for_baseline)
    }

    /// `Options::maximumExecutionCountsBetweenCheckpointsForBaseline() = value`.
    pub fn set_maximum_execution_counts_between_checkpoints_for_baseline(value: i32) {
        Options::with_mut(|options| options.maximum_execution_counts_between_checkpoints_for_baseline = value);
    }

    /// Opção `maximumExecutionCountsBetweenCheckpointsForUpperTiers`.
    pub fn maximum_execution_counts_between_checkpoints_for_upper_tiers() -> i32 {
        Options::with(|options| options.maximum_execution_counts_between_checkpoints_for_upper_tiers)
    }

    /// `Options::maximumExecutionCountsBetweenCheckpointsForUpperTiers() = value`.
    pub fn set_maximum_execution_counts_between_checkpoints_for_upper_tiers(value: i32) {
        Options::with_mut(|options| options.maximum_execution_counts_between_checkpoints_for_upper_tiers = value);
    }

    /// Opção `highCostBaselineProfilingFunctionBytecodeCost`.
    pub fn high_cost_baseline_profiling_function_bytecode_cost() -> i32 {
        Options::with(|options| options.high_cost_baseline_profiling_function_bytecode_cost)
    }

    /// `Options::highCostBaselineProfilingFunctionBytecodeCost() = value`.
    pub fn set_high_cost_baseline_profiling_function_bytecode_cost(value: i32) {
        Options::with_mut(|options| options.high_cost_baseline_profiling_function_bytecode_cost = value);
    }

    /// Opção `valueProfileFillingRateMonitoringBytecodeCost`.
    pub fn value_profile_filling_rate_monitoring_bytecode_cost() -> i32 {
        Options::with(|options| options.value_profile_filling_rate_monitoring_bytecode_cost)
    }

    /// `Options::valueProfileFillingRateMonitoringBytecodeCost() = value`.
    pub fn set_value_profile_filling_rate_monitoring_bytecode_cost(value: i32) {
        Options::with_mut(|options| options.value_profile_filling_rate_monitoring_bytecode_cost = value);
    }

    /// Opção `likelyToTakeSlowCaseMinimumCount`.
    pub fn likely_to_take_slow_case_minimum_count() -> u32 {
        Options::with(|options| options.likely_to_take_slow_case_minimum_count)
    }

    /// `Options::likelyToTakeSlowCaseMinimumCount() = value`.
    pub fn set_likely_to_take_slow_case_minimum_count(value: u32) {
        Options::with_mut(|options| options.likely_to_take_slow_case_minimum_count = value);
    }

    /// Opção `couldTakeSlowCaseMinimumCount`.
    pub fn could_take_slow_case_minimum_count() -> u32 {
        Options::with(|options| options.could_take_slow_case_minimum_count)
    }

    /// `Options::couldTakeSlowCaseMinimumCount() = value`.
    pub fn set_could_take_slow_case_minimum_count(value: u32) {
        Options::with_mut(|options| options.could_take_slow_case_minimum_count = value);
    }

    /// Opção `osrExitCountForReoptimization`.
    pub fn osr_exit_count_for_reoptimization() -> u32 {
        Options::with(|options| options.osr_exit_count_for_reoptimization)
    }

    /// `Options::osrExitCountForReoptimization() = value`.
    pub fn set_osr_exit_count_for_reoptimization(value: u32) {
        Options::with_mut(|options| options.osr_exit_count_for_reoptimization = value);
    }

    /// Opção `osrExitCountForReoptimizationFromLoop`.
    pub fn osr_exit_count_for_reoptimization_from_loop() -> u32 {
        Options::with(|options| options.osr_exit_count_for_reoptimization_from_loop)
    }

    /// `Options::osrExitCountForReoptimizationFromLoop() = value`.
    pub fn set_osr_exit_count_for_reoptimization_from_loop(value: u32) {
        Options::with_mut(|options| options.osr_exit_count_for_reoptimization_from_loop = value);
    }

    /// Opção `reoptimizationRetryCounterMax`.
    pub fn reoptimization_retry_counter_max() -> u32 {
        Options::with(|options| options.reoptimization_retry_counter_max)
    }

    /// `Options::reoptimizationRetryCounterMax() = value`.
    pub fn set_reoptimization_retry_counter_max(value: u32) {
        Options::with_mut(|options| options.reoptimization_retry_counter_max = value);
    }

    /// Opção `minimumOptimizationDelay`.
    pub fn minimum_optimization_delay() -> u32 {
        Options::with(|options| options.minimum_optimization_delay)
    }

    /// `Options::minimumOptimizationDelay() = value`.
    pub fn set_minimum_optimization_delay(value: u32) {
        Options::with_mut(|options| options.minimum_optimization_delay = value);
    }

    /// Opção `maximumOptimizationDelay`.
    pub fn maximum_optimization_delay() -> u32 {
        Options::with(|options| options.maximum_optimization_delay)
    }

    /// `Options::maximumOptimizationDelay() = value`.
    pub fn set_maximum_optimization_delay(value: u32) {
        Options::with_mut(|options| options.maximum_optimization_delay = value);
    }

    /// Opção `desiredProfileLivenessRate`.
    pub fn desired_profile_liveness_rate() -> f64 {
        Options::with(|options| options.desired_profile_liveness_rate)
    }

    /// `Options::desiredProfileLivenessRate() = value`.
    pub fn set_desired_profile_liveness_rate(value: f64) {
        Options::with_mut(|options| options.desired_profile_liveness_rate = value);
    }

    /// Opção `desiredProfileFullnessRate`.
    pub fn desired_profile_fullness_rate() -> f64 {
        Options::with(|options| options.desired_profile_fullness_rate)
    }

    /// `Options::desiredProfileFullnessRate() = value`.
    pub fn set_desired_profile_fullness_rate(value: f64) {
        Options::with_mut(|options| options.desired_profile_fullness_rate = value);
    }

    /// Opção `quickDFGTierUpThresholdFactor`.
    pub fn quick_dfg_tier_up_threshold_factor() -> f64 {
        Options::with(|options| options.quick_dfg_tier_up_threshold_factor)
    }

    /// `Options::quickDFGTierUpThresholdFactor() = value`.
    pub fn set_quick_dfg_tier_up_threshold_factor(value: f64) {
        Options::with_mut(|options| options.quick_dfg_tier_up_threshold_factor = value);
    }

    /// Opção `relaxedProfileCoverageFactorForQuickDFGTierUp`.
    pub fn relaxed_profile_coverage_factor_for_quick_dfg_tier_up() -> f64 {
        Options::with(|options| options.relaxed_profile_coverage_factor_for_quick_dfg_tier_up)
    }

    /// `Options::relaxedProfileCoverageFactorForQuickDFGTierUp() = value`.
    pub fn set_relaxed_profile_coverage_factor_for_quick_dfg_tier_up(value: f64) {
        Options::with_mut(|options| options.relaxed_profile_coverage_factor_for_quick_dfg_tier_up = value);
    }

    /// Opção `quickFTLTierUpThresholdFactor`.
    pub fn quick_ftl_tier_up_threshold_factor() -> f64 {
        Options::with(|options| options.quick_ftl_tier_up_threshold_factor)
    }

    /// `Options::quickFTLTierUpThresholdFactor() = value`.
    pub fn set_quick_ftl_tier_up_threshold_factor(value: f64) {
        Options::with_mut(|options| options.quick_ftl_tier_up_threshold_factor = value);
    }

    /// Opção `doubleVoteRatioForDoubleFormat`.
    pub fn double_vote_ratio_for_double_format() -> f64 {
        Options::with(|options| options.double_vote_ratio_for_double_format)
    }

    /// `Options::doubleVoteRatioForDoubleFormat() = value`.
    pub fn set_double_vote_ratio_for_double_format(value: f64) {
        Options::with_mut(|options| options.double_vote_ratio_for_double_format = value);
    }

    /// Opção `structureCheckVoteRatioForHoisting`.
    pub fn structure_check_vote_ratio_for_hoisting() -> f64 {
        Options::with(|options| options.structure_check_vote_ratio_for_hoisting)
    }

    /// `Options::structureCheckVoteRatioForHoisting() = value`.
    pub fn set_structure_check_vote_ratio_for_hoisting(value: f64) {
        Options::with_mut(|options| options.structure_check_vote_ratio_for_hoisting = value);
    }

    /// Opção `checkArrayVoteRatioForHoisting`.
    pub fn check_array_vote_ratio_for_hoisting() -> f64 {
        Options::with(|options| options.check_array_vote_ratio_for_hoisting)
    }

    /// `Options::checkArrayVoteRatioForHoisting() = value`.
    pub fn set_check_array_vote_ratio_for_hoisting(value: f64) {
        Options::with_mut(|options| options.check_array_vote_ratio_for_hoisting = value);
    }

    /// Opção `maximumDirectCallStackSize`.
    pub fn maximum_direct_call_stack_size() -> u32 {
        Options::with(|options| options.maximum_direct_call_stack_size)
    }

    /// `Options::maximumDirectCallStackSize() = value`.
    pub fn set_maximum_direct_call_stack_size(value: u32) {
        Options::with_mut(|options| options.maximum_direct_call_stack_size = value);
    }

    /// Opção `minimumNumberOfScansBetweenRebalance`.
    pub fn minimum_number_of_scans_between_rebalance() -> u32 {
        Options::with(|options| options.minimum_number_of_scans_between_rebalance)
    }

    /// `Options::minimumNumberOfScansBetweenRebalance() = value`.
    pub fn set_minimum_number_of_scans_between_rebalance(value: u32) {
        Options::with_mut(|options| options.minimum_number_of_scans_between_rebalance = value);
    }

    /// Opção `numberOfGCMarkers`.
    pub fn number_of_gc_markers() -> u32 {
        Options::with(|options| options.number_of_gc_markers)
    }

    /// `Options::numberOfGCMarkers() = value`.
    pub fn set_number_of_gc_markers(value: u32) {
        Options::with_mut(|options| options.number_of_gc_markers = value);
    }

    /// Opção `useParallelMarkingConstraintSolver`.
    pub fn use_parallel_marking_constraint_solver() -> bool {
        Options::with(|options| options.use_parallel_marking_constraint_solver)
    }

    /// `Options::useParallelMarkingConstraintSolver() = value`.
    pub fn set_use_parallel_marking_constraint_solver(value: bool) {
        Options::with_mut(|options| options.use_parallel_marking_constraint_solver = value);
    }

    /// Opção `opaqueRootMergeThreshold`.
    pub fn opaque_root_merge_threshold() -> u32 {
        Options::with(|options| options.opaque_root_merge_threshold)
    }

    /// `Options::opaqueRootMergeThreshold() = value`.
    pub fn set_opaque_root_merge_threshold(value: u32) {
        Options::with_mut(|options| options.opaque_root_merge_threshold = value);
    }

    /// Opção `maxHeapSizeAsRAMSizeMultiple`.
    pub fn max_heap_size_as_ram_size_multiple() -> u32 {
        Options::with(|options| options.max_heap_size_as_ram_size_multiple)
    }

    /// `Options::maxHeapSizeAsRAMSizeMultiple() = value`.
    pub fn set_max_heap_size_as_ram_size_multiple(value: u32) {
        Options::with_mut(|options| options.max_heap_size_as_ram_size_multiple = value);
    }

    /// Opção `minHeapUtilization`.
    pub fn min_heap_utilization() -> f64 {
        Options::with(|options| options.min_heap_utilization)
    }

    /// `Options::minHeapUtilization() = value`.
    pub fn set_min_heap_utilization(value: f64) {
        Options::with_mut(|options| options.min_heap_utilization = value);
    }

    /// Opção `minMarkedBlockUtilization`.
    pub fn min_marked_block_utilization() -> f64 {
        Options::with(|options| options.min_marked_block_utilization)
    }

    /// `Options::minMarkedBlockUtilization() = value`.
    pub fn set_min_marked_block_utilization(value: f64) {
        Options::with_mut(|options| options.min_marked_block_utilization = value);
    }

    /// Opção `slowPathAllocsBetweenGCs`.
    pub fn slow_path_allocs_between_g_cs() -> u32 {
        Options::with(|options| options.slow_path_allocs_between_g_cs)
    }

    /// `Options::slowPathAllocsBetweenGCs() = value`.
    pub fn set_slow_path_allocs_between_g_cs(value: u32) {
        Options::with_mut(|options| options.slow_path_allocs_between_g_cs = value);
    }

    /// Opção `maxRegExpStackSize`.
    pub fn max_reg_exp_stack_size() -> u32 {
        Options::with(|options| options.max_reg_exp_stack_size)
    }

    /// `Options::maxRegExpStackSize() = value`.
    pub fn set_max_reg_exp_stack_size(value: u32) {
        Options::with_mut(|options| options.max_reg_exp_stack_size = value);
    }

    /// Opção `percentCPUPerMBForFullTimer`.
    pub fn percent_cpu_per_mb_for_full_timer() -> f64 {
        Options::with(|options| options.percent_cpu_per_mb_for_full_timer)
    }

    /// `Options::percentCPUPerMBForFullTimer() = value`.
    pub fn set_percent_cpu_per_mb_for_full_timer(value: f64) {
        Options::with_mut(|options| options.percent_cpu_per_mb_for_full_timer = value);
    }

    /// Opção `percentCPUPerMBForEdenTimer`.
    pub fn percent_cpu_per_mb_for_eden_timer() -> f64 {
        Options::with(|options| options.percent_cpu_per_mb_for_eden_timer)
    }

    /// `Options::percentCPUPerMBForEdenTimer() = value`.
    pub fn set_percent_cpu_per_mb_for_eden_timer(value: f64) {
        Options::with_mut(|options| options.percent_cpu_per_mb_for_eden_timer = value);
    }

    /// Opção `collectionTimerMaxPercentCPU`.
    pub fn collection_timer_max_percent_cpu() -> f64 {
        Options::with(|options| options.collection_timer_max_percent_cpu)
    }

    /// `Options::collectionTimerMaxPercentCPU() = value`.
    pub fn set_collection_timer_max_percent_cpu(value: f64) {
        Options::with_mut(|options| options.collection_timer_max_percent_cpu = value);
    }

    /// Opção `forceWeakRandomSeed`.
    pub fn force_weak_random_seed() -> bool {
        Options::with(|options| options.force_weak_random_seed)
    }

    /// `Options::forceWeakRandomSeed() = value`.
    pub fn set_force_weak_random_seed(value: bool) {
        Options::with_mut(|options| options.force_weak_random_seed = value);
    }

    /// Opção `forcedWeakRandomSeed`.
    pub fn forced_weak_random_seed() -> u32 {
        Options::with(|options| options.forced_weak_random_seed)
    }

    /// `Options::forcedWeakRandomSeed() = value`.
    pub fn set_forced_weak_random_seed(value: u32) {
        Options::with_mut(|options| options.forced_weak_random_seed = value);
    }

    /// Opção `alwaysHaveABadTime`.
    pub fn always_have_a_bad_time() -> bool {
        Options::with(|options| options.always_have_a_bad_time)
    }

    /// `Options::alwaysHaveABadTime() = value`.
    pub fn set_always_have_a_bad_time(value: bool) {
        Options::with_mut(|options| options.always_have_a_bad_time = value);
    }

    /// Opção `allowDoubleShape`.
    pub fn allow_double_shape() -> bool {
        Options::with(|options| options.allow_double_shape)
    }

    /// `Options::allowDoubleShape() = value`.
    pub fn set_allow_double_shape(value: bool) {
        Options::with_mut(|options| options.allow_double_shape = value);
    }

    /// Opção `useZombieMode`.
    pub fn use_zombie_mode() -> bool {
        Options::with(|options| options.use_zombie_mode)
    }

    /// `Options::useZombieMode() = value`.
    pub fn set_use_zombie_mode(value: bool) {
        Options::with_mut(|options| options.use_zombie_mode = value);
    }

    /// Opção `useImmortalObjects`.
    pub fn use_immortal_objects() -> bool {
        Options::with(|options| options.use_immortal_objects)
    }

    /// `Options::useImmortalObjects() = value`.
    pub fn set_use_immortal_objects(value: bool) {
        Options::with_mut(|options| options.use_immortal_objects = value);
    }

    /// Opção `sweepSynchronously`.
    pub fn sweep_synchronously() -> bool {
        Options::with(|options| options.sweep_synchronously)
    }

    /// `Options::sweepSynchronously() = value`.
    pub fn set_sweep_synchronously(value: bool) {
        Options::with_mut(|options| options.sweep_synchronously = value);
    }

    /// Opção `maxSingleAllocationSize`.
    pub fn max_single_allocation_size() -> u32 {
        Options::with(|options| options.max_single_allocation_size)
    }

    /// `Options::maxSingleAllocationSize() = value`.
    pub fn set_max_single_allocation_size(value: u32) {
        Options::with_mut(|options| options.max_single_allocation_size = value);
    }

    /// Opção `logGC`.
    pub fn log_gc() -> GCLogLevel {
        Options::with(|options| options.log_gc)
    }

    /// `Options::logGC() = value`.
    pub fn set_log_gc(value: GCLogLevel) {
        Options::with_mut(|options| options.log_gc = value);
    }

    /// Opção `useGC`.
    pub fn use_gc() -> bool {
        Options::with(|options| options.use_gc)
    }

    /// `Options::useGC() = value`.
    pub fn set_use_gc(value: bool) {
        Options::with_mut(|options| options.use_gc = value);
    }

    /// Opção `useGlobalGC`.
    pub fn use_global_gc() -> bool {
        Options::with(|options| options.use_global_gc)
    }

    /// `Options::useGlobalGC() = value`.
    pub fn set_use_global_gc(value: bool) {
        Options::with_mut(|options| options.use_global_gc = value);
    }

    /// Opção `gcAtEnd`.
    pub fn gc_at_end() -> bool {
        Options::with(|options| options.gc_at_end)
    }

    /// `Options::gcAtEnd() = value`.
    pub fn set_gc_at_end(value: bool) {
        Options::with_mut(|options| options.gc_at_end = value);
    }

    /// Opção `forceGCSlowPaths`.
    pub fn force_gc_slow_paths() -> bool {
        Options::with(|options| options.force_gc_slow_paths)
    }

    /// `Options::forceGCSlowPaths() = value`.
    pub fn set_force_gc_slow_paths(value: bool) {
        Options::with_mut(|options| options.force_gc_slow_paths = value);
    }

    /// Opção `forceDidDeferGCWork`.
    pub fn force_did_defer_gc_work() -> bool {
        Options::with(|options| options.force_did_defer_gc_work)
    }

    /// `Options::forceDidDeferGCWork() = value`.
    pub fn set_force_did_defer_gc_work(value: bool) {
        Options::with_mut(|options| options.force_did_defer_gc_work = value);
    }

    /// Opção `gcMaxHeapSize`.
    pub fn gc_max_heap_size() -> u32 {
        Options::with(|options| options.gc_max_heap_size)
    }

    /// `Options::gcMaxHeapSize() = value`.
    pub fn set_gc_max_heap_size(value: u32) {
        Options::with_mut(|options| options.gc_max_heap_size = value);
    }

    /// Opção `forceRAMSize`.
    pub fn force_ram_size() -> usize {
        Options::with(|options| options.force_ram_size)
    }

    /// `Options::forceRAMSize() = value`.
    pub fn set_force_ram_size(value: usize) {
        Options::with_mut(|options| options.force_ram_size = value);
    }

    /// Opção `recordGCPauseTimes`.
    pub fn record_gc_pause_times() -> bool {
        Options::with(|options| options.record_gc_pause_times)
    }

    /// `Options::recordGCPauseTimes() = value`.
    pub fn set_record_gc_pause_times(value: bool) {
        Options::with_mut(|options| options.record_gc_pause_times = value);
    }

    /// Opção `dumpHeapStatisticsAtVMDestruction`.
    pub fn dump_heap_statistics_at_vm_destruction() -> bool {
        Options::with(|options| options.dump_heap_statistics_at_vm_destruction)
    }

    /// `Options::dumpHeapStatisticsAtVMDestruction() = value`.
    pub fn set_dump_heap_statistics_at_vm_destruction(value: bool) {
        Options::with_mut(|options| options.dump_heap_statistics_at_vm_destruction = value);
    }

    /// Opção `enableStrongRefTracker`.
    pub fn enable_strong_ref_tracker() -> bool {
        Options::with(|options| options.enable_strong_ref_tracker)
    }

    /// `Options::enableStrongRefTracker() = value`.
    pub fn set_enable_strong_ref_tracker(value: bool) {
        Options::with_mut(|options| options.enable_strong_ref_tracker = value);
    }

    /// Opção `dumpHeapOnLowMemory`.
    pub fn dump_heap_on_low_memory() -> bool {
        Options::with(|options| options.dump_heap_on_low_memory)
    }

    /// `Options::dumpHeapOnLowMemory() = value`.
    pub fn set_dump_heap_on_low_memory(value: bool) {
        Options::with_mut(|options| options.dump_heap_on_low_memory = value);
    }

    /// Opção `forceCodeBlockToJettisonDueToOldAge`.
    pub fn force_code_block_to_jettison_due_to_old_age() -> bool {
        Options::with(|options| options.force_code_block_to_jettison_due_to_old_age)
    }

    /// `Options::forceCodeBlockToJettisonDueToOldAge() = value`.
    pub fn set_force_code_block_to_jettison_due_to_old_age(value: bool) {
        Options::with_mut(|options| options.force_code_block_to_jettison_due_to_old_age = value);
    }

    /// Opção `useEagerCodeBlockJettisonTiming`.
    pub fn use_eager_code_block_jettison_timing() -> bool {
        Options::with(|options| options.use_eager_code_block_jettison_timing)
    }

    /// `Options::useEagerCodeBlockJettisonTiming() = value`.
    pub fn set_use_eager_code_block_jettison_timing(value: bool) {
        Options::with_mut(|options| options.use_eager_code_block_jettison_timing = value);
    }

    /// Opção `useExecutionCountForCodeBlockAging`.
    pub fn use_execution_count_for_code_block_aging() -> bool {
        Options::with(|options| options.use_execution_count_for_code_block_aging)
    }

    /// `Options::useExecutionCountForCodeBlockAging() = value`.
    pub fn set_use_execution_count_for_code_block_aging(value: bool) {
        Options::with_mut(|options| options.use_execution_count_for_code_block_aging = value);
    }

    /// Opção `optimizedCodeAgingQuietAllocationMB`.
    pub fn optimized_code_aging_quiet_allocation_mb() -> u32 {
        Options::with(|options| options.optimized_code_aging_quiet_allocation_mb)
    }

    /// `Options::optimizedCodeAgingQuietAllocationMB() = value`.
    pub fn set_optimized_code_aging_quiet_allocation_mb(value: u32) {
        Options::with_mut(|options| options.optimized_code_aging_quiet_allocation_mb = value);
    }

    /// Opção `optimizedCodeAgingQuietSeconds`.
    pub fn optimized_code_aging_quiet_seconds() -> f64 {
        Options::with(|options| options.optimized_code_aging_quiet_seconds)
    }

    /// `Options::optimizedCodeAgingQuietSeconds() = value`.
    pub fn set_optimized_code_aging_quiet_seconds(value: f64) {
        Options::with_mut(|options| options.optimized_code_aging_quiet_seconds = value);
    }

    /// Opção `codeBlockAgingLeaseMultiplier`.
    pub fn code_block_aging_lease_multiplier() -> f64 {
        Options::with(|options| options.code_block_aging_lease_multiplier)
    }

    /// `Options::codeBlockAgingLeaseMultiplier() = value`.
    pub fn set_code_block_aging_lease_multiplier(value: f64) {
        Options::with_mut(|options| options.code_block_aging_lease_multiplier = value);
    }

    /// Opção `useLeanBytecodeCacheDecoder`.
    pub fn use_lean_bytecode_cache_decoder() -> bool {
        Options::with(|options| options.use_lean_bytecode_cache_decoder)
    }

    /// `Options::useLeanBytecodeCacheDecoder() = value`.
    pub fn set_use_lean_bytecode_cache_decoder(value: bool) {
        Options::with_mut(|options| options.use_lean_bytecode_cache_decoder = value);
    }

    /// Opção `useBorrowedBytecodeFromCache`.
    pub fn use_borrowed_bytecode_from_cache() -> bool {
        Options::with(|options| options.use_borrowed_bytecode_from_cache)
    }

    /// `Options::useBorrowedBytecodeFromCache() = value`.
    pub fn set_use_borrowed_bytecode_from_cache(value: bool) {
        Options::with_mut(|options| options.use_borrowed_bytecode_from_cache = value);
    }

    /// Opção `diskCachePayloadIsPersistentForTesting`.
    pub fn disk_cache_payload_is_persistent_for_testing() -> bool {
        Options::with(|options| options.disk_cache_payload_is_persistent_for_testing)
    }

    /// `Options::diskCachePayloadIsPersistentForTesting() = value`.
    pub fn set_disk_cache_payload_is_persistent_for_testing(value: bool) {
        Options::with_mut(|options| options.disk_cache_payload_is_persistent_for_testing = value);
    }

    /// Opção `verifyBytecodeCacheChecksums`.
    pub fn verify_bytecode_cache_checksums() -> bool {
        Options::with(|options| options.verify_bytecode_cache_checksums)
    }

    /// `Options::verifyBytecodeCacheChecksums() = value`.
    pub fn set_verify_bytecode_cache_checksums(value: bool) {
        Options::with_mut(|options| options.verify_bytecode_cache_checksums = value);
    }

    /// Opção `useTypeProfiler`.
    pub fn use_type_profiler() -> bool {
        Options::with(|options| options.use_type_profiler)
    }

    /// `Options::useTypeProfiler() = value`.
    pub fn set_use_type_profiler(value: bool) {
        Options::with_mut(|options| options.use_type_profiler = value);
    }

    /// Opção `useControlFlowProfiler`.
    pub fn use_control_flow_profiler() -> bool {
        Options::with(|options| options.use_control_flow_profiler)
    }

    /// `Options::useControlFlowProfiler() = value`.
    pub fn set_use_control_flow_profiler(value: bool) {
        Options::with_mut(|options| options.use_control_flow_profiler = value);
    }

    /// Opção `useSamplingProfiler`.
    pub fn use_sampling_profiler() -> bool {
        Options::with(|options| options.use_sampling_profiler)
    }

    /// `Options::useSamplingProfiler() = value`.
    pub fn set_use_sampling_profiler(value: bool) {
        Options::with_mut(|options| options.use_sampling_profiler = value);
    }

    /// Opção `sampleInterval`.
    pub fn sample_interval() -> u32 {
        Options::with(|options| options.sample_interval)
    }

    /// `Options::sampleInterval() = value`.
    pub fn set_sample_interval(value: u32) {
        Options::with_mut(|options| options.sample_interval = value);
    }

    /// Opção `collectExtraSamplingProfilerData`.
    pub fn collect_extra_sampling_profiler_data() -> bool {
        Options::with(|options| options.collect_extra_sampling_profiler_data)
    }

    /// `Options::collectExtraSamplingProfilerData() = value`.
    pub fn set_collect_extra_sampling_profiler_data(value: bool) {
        Options::with_mut(|options| options.collect_extra_sampling_profiler_data = value);
    }

    /// Opção `samplingProfilerTopFunctionsCount`.
    pub fn sampling_profiler_top_functions_count() -> u32 {
        Options::with(|options| options.sampling_profiler_top_functions_count)
    }

    /// `Options::samplingProfilerTopFunctionsCount() = value`.
    pub fn set_sampling_profiler_top_functions_count(value: u32) {
        Options::with_mut(|options| options.sampling_profiler_top_functions_count = value);
    }

    /// Opção `samplingProfilerTopBytecodesCount`.
    pub fn sampling_profiler_top_bytecodes_count() -> u32 {
        Options::with(|options| options.sampling_profiler_top_bytecodes_count)
    }

    /// `Options::samplingProfilerTopBytecodesCount() = value`.
    pub fn set_sampling_profiler_top_bytecodes_count(value: u32) {
        Options::with_mut(|options| options.sampling_profiler_top_bytecodes_count = value);
    }

    /// Opção `samplingProfilerIgnoreExternalSourceID`.
    pub fn sampling_profiler_ignore_external_source_id() -> bool {
        Options::with(|options| options.sampling_profiler_ignore_external_source_id)
    }

    /// `Options::samplingProfilerIgnoreExternalSourceID() = value`.
    pub fn set_sampling_profiler_ignore_external_source_id(value: bool) {
        Options::with_mut(|options| options.sampling_profiler_ignore_external_source_id = value);
    }

    /// Opção `samplingProfilerPath`.
    pub fn sampling_profiler_path() -> Option<String> {
        Options::with(|options| options.sampling_profiler_path.clone())
    }

    /// `Options::samplingProfilerPath() = value`.
    pub fn set_sampling_profiler_path(value: Option<String>) {
        Options::with_mut(|options| options.sampling_profiler_path = value);
    }

    /// Opção `sampleCCode`.
    pub fn sample_c_code() -> bool {
        Options::with(|options| options.sample_c_code)
    }

    /// `Options::sampleCCode() = value`.
    pub fn set_sample_c_code(value: bool) {
        Options::with_mut(|options| options.sample_c_code = value);
    }

    /// Opção `alwaysGeneratePCToCodeOriginMap`.
    pub fn always_generate_pc_to_code_origin_map() -> bool {
        Options::with(|options| options.always_generate_pc_to_code_origin_map)
    }

    /// `Options::alwaysGeneratePCToCodeOriginMap() = value`.
    pub fn set_always_generate_pc_to_code_origin_map(value: bool) {
        Options::with_mut(|options| options.always_generate_pc_to_code_origin_map = value);
    }

    /// Opção `randomIntegrityAuditRate`.
    pub fn random_integrity_audit_rate() -> f64 {
        Options::with(|options| options.random_integrity_audit_rate)
    }

    /// `Options::randomIntegrityAuditRate() = value`.
    pub fn set_random_integrity_audit_rate(value: f64) {
        Options::with_mut(|options| options.random_integrity_audit_rate = value);
    }

    /// Opção `verifyGC`.
    pub fn verify_gc() -> bool {
        Options::with(|options| options.verify_gc)
    }

    /// `Options::verifyGC() = value`.
    pub fn set_verify_gc(value: bool) {
        Options::with_mut(|options| options.verify_gc = value);
    }

    /// Opção `verboseVerifyGC`.
    pub fn verbose_verify_gc() -> bool {
        Options::with(|options| options.verbose_verify_gc)
    }

    /// `Options::verboseVerifyGC() = value`.
    pub fn set_verbose_verify_gc(value: bool) {
        Options::with_mut(|options| options.verbose_verify_gc = value);
    }

    /// Opção `verifyHeap`.
    pub fn verify_heap() -> bool {
        Options::with(|options| options.verify_heap)
    }

    /// `Options::verifyHeap() = value`.
    pub fn set_verify_heap(value: bool) {
        Options::with_mut(|options| options.verify_heap = value);
    }

    /// Opção `numberOfGCCyclesToRecordForVerification`.
    pub fn number_of_gc_cycles_to_record_for_verification() -> u32 {
        Options::with(|options| options.number_of_gc_cycles_to_record_for_verification)
    }

    /// `Options::numberOfGCCyclesToRecordForVerification() = value`.
    pub fn set_number_of_gc_cycles_to_record_for_verification(value: u32) {
        Options::with_mut(|options| options.number_of_gc_cycles_to_record_for_verification = value);
    }

    /// Opção `exceptionStackTraceLimit`.
    pub fn exception_stack_trace_limit() -> u32 {
        Options::with(|options| options.exception_stack_trace_limit)
    }

    /// `Options::exceptionStackTraceLimit() = value`.
    pub fn set_exception_stack_trace_limit(value: u32) {
        Options::with_mut(|options| options.exception_stack_trace_limit = value);
    }

    /// Opção `defaultErrorStackTraceLimit`.
    pub fn default_error_stack_trace_limit() -> u32 {
        Options::with(|options| options.default_error_stack_trace_limit)
    }

    /// `Options::defaultErrorStackTraceLimit() = value`.
    pub fn set_default_error_stack_trace_limit(value: u32) {
        Options::with_mut(|options| options.default_error_stack_trace_limit = value);
    }

    /// Opção `exitOnResourceExhaustion`.
    pub fn exit_on_resource_exhaustion() -> bool {
        Options::with(|options| options.exit_on_resource_exhaustion)
    }

    /// `Options::exitOnResourceExhaustion() = value`.
    pub fn set_exit_on_resource_exhaustion(value: bool) {
        Options::with_mut(|options| options.exit_on_resource_exhaustion = value);
    }

    /// Opção `useExceptionFuzz`.
    pub fn use_exception_fuzz() -> bool {
        Options::with(|options| options.use_exception_fuzz)
    }

    /// `Options::useExceptionFuzz() = value`.
    pub fn set_use_exception_fuzz(value: bool) {
        Options::with_mut(|options| options.use_exception_fuzz = value);
    }

    /// Opção `fireExceptionFuzzAt`.
    pub fn fire_exception_fuzz_at() -> u32 {
        Options::with(|options| options.fire_exception_fuzz_at)
    }

    /// `Options::fireExceptionFuzzAt() = value`.
    pub fn set_fire_exception_fuzz_at(value: u32) {
        Options::with_mut(|options| options.fire_exception_fuzz_at = value);
    }

    /// Opção `fuzzAtomicJITMemcpy`.
    pub fn fuzz_atomic_jit_memcpy() -> bool {
        Options::with(|options| options.fuzz_atomic_jit_memcpy)
    }

    /// `Options::fuzzAtomicJITMemcpy() = value`.
    pub fn set_fuzz_atomic_jit_memcpy(value: bool) {
        Options::with_mut(|options| options.fuzz_atomic_jit_memcpy = value);
    }

    /// Opção `validateDFGExceptionHandling`.
    pub fn validate_dfg_exception_handling() -> bool {
        Options::with(|options| options.validate_dfg_exception_handling)
    }

    /// `Options::validateDFGExceptionHandling() = value`.
    pub fn set_validate_dfg_exception_handling(value: bool) {
        Options::with_mut(|options| options.validate_dfg_exception_handling = value);
    }

    /// Opção `dumpSimulatedThrows`.
    pub fn dump_simulated_throws() -> bool {
        Options::with(|options| options.dump_simulated_throws)
    }

    /// `Options::dumpSimulatedThrows() = value`.
    pub fn set_dump_simulated_throws(value: bool) {
        Options::with_mut(|options| options.dump_simulated_throws = value);
    }

    /// Opção `validateExceptionChecks`.
    pub fn validate_exception_checks() -> bool {
        Options::with(|options| options.validate_exception_checks)
    }

    /// `Options::validateExceptionChecks() = value`.
    pub fn set_validate_exception_checks(value: bool) {
        Options::with_mut(|options| options.validate_exception_checks = value);
    }

    /// Opção `unexpectedExceptionStackTraceLimit`.
    pub fn unexpected_exception_stack_trace_limit() -> u32 {
        Options::with(|options| options.unexpected_exception_stack_trace_limit)
    }

    /// `Options::unexpectedExceptionStackTraceLimit() = value`.
    pub fn set_unexpected_exception_stack_trace_limit(value: u32) {
        Options::with_mut(|options| options.unexpected_exception_stack_trace_limit = value);
    }

    /// Opção `validateDFGClobberize`.
    pub fn validate_dfg_clobberize() -> bool {
        Options::with(|options| options.validate_dfg_clobberize)
    }

    /// `Options::validateDFGClobberize() = value`.
    pub fn set_validate_dfg_clobberize(value: bool) {
        Options::with_mut(|options| options.validate_dfg_clobberize = value);
    }

    /// Opção `validateBoundsCheckElimination`.
    pub fn validate_bounds_check_elimination() -> bool {
        Options::with(|options| options.validate_bounds_check_elimination)
    }

    /// `Options::validateBoundsCheckElimination() = value`.
    pub fn set_validate_bounds_check_elimination(value: bool) {
        Options::with_mut(|options| options.validate_bounds_check_elimination = value);
    }

    /// Opção `validateDFGMayExit`.
    pub fn validate_dfg_may_exit() -> bool {
        Options::with(|options| options.validate_dfg_may_exit)
    }

    /// `Options::validateDFGMayExit() = value`.
    pub fn set_validate_dfg_may_exit(value: bool) {
        Options::with_mut(|options| options.validate_dfg_may_exit = value);
    }

    /// Opção `validateVMEntryCalleeSaves`.
    pub fn validate_vm_entry_callee_saves() -> bool {
        Options::with(|options| options.validate_vm_entry_callee_saves)
    }

    /// `Options::validateVMEntryCalleeSaves() = value`.
    pub fn set_validate_vm_entry_callee_saves(value: bool) {
        Options::with_mut(|options| options.validate_vm_entry_callee_saves = value);
    }

    /// Opção `useExecutableAllocationFuzz`.
    pub fn use_executable_allocation_fuzz() -> bool {
        Options::with(|options| options.use_executable_allocation_fuzz)
    }

    /// `Options::useExecutableAllocationFuzz() = value`.
    pub fn set_use_executable_allocation_fuzz(value: bool) {
        Options::with_mut(|options| options.use_executable_allocation_fuzz = value);
    }

    /// Opção `fireExecutableAllocationFuzzAt`.
    pub fn fire_executable_allocation_fuzz_at() -> u32 {
        Options::with(|options| options.fire_executable_allocation_fuzz_at)
    }

    /// `Options::fireExecutableAllocationFuzzAt() = value`.
    pub fn set_fire_executable_allocation_fuzz_at(value: u32) {
        Options::with_mut(|options| options.fire_executable_allocation_fuzz_at = value);
    }

    /// Opção `fireExecutableAllocationFuzzAtOrAfter`.
    pub fn fire_executable_allocation_fuzz_at_or_after() -> u32 {
        Options::with(|options| options.fire_executable_allocation_fuzz_at_or_after)
    }

    /// `Options::fireExecutableAllocationFuzzAtOrAfter() = value`.
    pub fn set_fire_executable_allocation_fuzz_at_or_after(value: u32) {
        Options::with_mut(|options| options.fire_executable_allocation_fuzz_at_or_after = value);
    }

    /// Opção `fireExecutableAllocationFuzzRandomly`.
    pub fn fire_executable_allocation_fuzz_randomly() -> bool {
        Options::with(|options| options.fire_executable_allocation_fuzz_randomly)
    }

    /// `Options::fireExecutableAllocationFuzzRandomly() = value`.
    pub fn set_fire_executable_allocation_fuzz_randomly(value: bool) {
        Options::with_mut(|options| options.fire_executable_allocation_fuzz_randomly = value);
    }

    /// Opção `fireExecutableAllocationFuzzRandomlyProbability`.
    pub fn fire_executable_allocation_fuzz_randomly_probability() -> f64 {
        Options::with(|options| options.fire_executable_allocation_fuzz_randomly_probability)
    }

    /// `Options::fireExecutableAllocationFuzzRandomlyProbability() = value`.
    pub fn set_fire_executable_allocation_fuzz_randomly_probability(value: f64) {
        Options::with_mut(|options| options.fire_executable_allocation_fuzz_randomly_probability = value);
    }

    /// Opção `verboseExecutableAllocationFuzz`.
    pub fn verbose_executable_allocation_fuzz() -> bool {
        Options::with(|options| options.verbose_executable_allocation_fuzz)
    }

    /// `Options::verboseExecutableAllocationFuzz() = value`.
    pub fn set_verbose_executable_allocation_fuzz(value: bool) {
        Options::with_mut(|options| options.verbose_executable_allocation_fuzz = value);
    }

    /// Opção `zeroExecutableMemoryOnFree`.
    pub fn zero_executable_memory_on_free() -> bool {
        Options::with(|options| options.zero_executable_memory_on_free)
    }

    /// `Options::zeroExecutableMemoryOnFree() = value`.
    pub fn set_zero_executable_memory_on_free(value: bool) {
        Options::with_mut(|options| options.zero_executable_memory_on_free = value);
    }

    /// Opção `useOSRExitFuzz`.
    pub fn use_osr_exit_fuzz() -> bool {
        Options::with(|options| options.use_osr_exit_fuzz)
    }

    /// `Options::useOSRExitFuzz() = value`.
    pub fn set_use_osr_exit_fuzz(value: bool) {
        Options::with_mut(|options| options.use_osr_exit_fuzz = value);
    }

    /// Opção `fireOSRExitFuzzAtStatic`.
    pub fn fire_osr_exit_fuzz_at_static() -> u32 {
        Options::with(|options| options.fire_osr_exit_fuzz_at_static)
    }

    /// `Options::fireOSRExitFuzzAtStatic() = value`.
    pub fn set_fire_osr_exit_fuzz_at_static(value: u32) {
        Options::with_mut(|options| options.fire_osr_exit_fuzz_at_static = value);
    }

    /// Opção `fireOSRExitFuzzAt`.
    pub fn fire_osr_exit_fuzz_at() -> u32 {
        Options::with(|options| options.fire_osr_exit_fuzz_at)
    }

    /// `Options::fireOSRExitFuzzAt() = value`.
    pub fn set_fire_osr_exit_fuzz_at(value: u32) {
        Options::with_mut(|options| options.fire_osr_exit_fuzz_at = value);
    }

    /// Opção `fireOSRExitFuzzAtOrAfter`.
    pub fn fire_osr_exit_fuzz_at_or_after() -> u32 {
        Options::with(|options| options.fire_osr_exit_fuzz_at_or_after)
    }

    /// `Options::fireOSRExitFuzzAtOrAfter() = value`.
    pub fn set_fire_osr_exit_fuzz_at_or_after(value: u32) {
        Options::with_mut(|options| options.fire_osr_exit_fuzz_at_or_after = value);
    }

    /// Opção `verboseOSRExitFuzz`.
    pub fn verbose_osr_exit_fuzz() -> bool {
        Options::with(|options| options.verbose_osr_exit_fuzz)
    }

    /// `Options::verboseOSRExitFuzz() = value`.
    pub fn set_verbose_osr_exit_fuzz(value: bool) {
        Options::with_mut(|options| options.verbose_osr_exit_fuzz = value);
    }

    /// Opção `useLOLJIT`.
    pub fn use_loljit() -> bool {
        Options::with(|options| options.use_loljit)
    }

    /// `Options::useLOLJIT() = value`.
    pub fn set_use_loljit(value: bool) {
        Options::with_mut(|options| options.use_loljit = value);
    }

    /// Opção `verboseLOLAllocation`.
    pub fn verbose_lol_allocation() -> bool {
        Options::with(|options| options.verbose_lol_allocation)
    }

    /// `Options::verboseLOLAllocation() = value`.
    pub fn set_verbose_lol_allocation(value: bool) {
        Options::with_mut(|options| options.verbose_lol_allocation = value);
    }

    /// Opção `seedOfVMRandomForFuzzer`.
    pub fn seed_of_vm_random_for_fuzzer() -> u32 {
        Options::with(|options| options.seed_of_vm_random_for_fuzzer)
    }

    /// `Options::seedOfVMRandomForFuzzer() = value`.
    pub fn set_seed_of_vm_random_for_fuzzer(value: u32) {
        Options::with_mut(|options| options.seed_of_vm_random_for_fuzzer = value);
    }

    /// Opção `useRandomizingFuzzerAgent`.
    pub fn use_randomizing_fuzzer_agent() -> bool {
        Options::with(|options| options.use_randomizing_fuzzer_agent)
    }

    /// `Options::useRandomizingFuzzerAgent() = value`.
    pub fn set_use_randomizing_fuzzer_agent(value: bool) {
        Options::with_mut(|options| options.use_randomizing_fuzzer_agent = value);
    }

    /// Opção `seedOfRandomizingFuzzerAgent`.
    pub fn seed_of_randomizing_fuzzer_agent() -> u32 {
        Options::with(|options| options.seed_of_randomizing_fuzzer_agent)
    }

    /// `Options::seedOfRandomizingFuzzerAgent() = value`.
    pub fn set_seed_of_randomizing_fuzzer_agent(value: u32) {
        Options::with_mut(|options| options.seed_of_randomizing_fuzzer_agent = value);
    }

    /// Opção `dumpFuzzerAgentPredictions`.
    pub fn dump_fuzzer_agent_predictions() -> bool {
        Options::with(|options| options.dump_fuzzer_agent_predictions)
    }

    /// `Options::dumpFuzzerAgentPredictions() = value`.
    pub fn set_dump_fuzzer_agent_predictions(value: bool) {
        Options::with_mut(|options| options.dump_fuzzer_agent_predictions = value);
    }

    /// Opção `useDoublePredictionFuzzerAgent`.
    pub fn use_double_prediction_fuzzer_agent() -> bool {
        Options::with(|options| options.use_double_prediction_fuzzer_agent)
    }

    /// `Options::useDoublePredictionFuzzerAgent() = value`.
    pub fn set_use_double_prediction_fuzzer_agent(value: bool) {
        Options::with_mut(|options| options.use_double_prediction_fuzzer_agent = value);
    }

    /// Opção `useFileBasedFuzzerAgent`.
    pub fn use_file_based_fuzzer_agent() -> bool {
        Options::with(|options| options.use_file_based_fuzzer_agent)
    }

    /// `Options::useFileBasedFuzzerAgent() = value`.
    pub fn set_use_file_based_fuzzer_agent(value: bool) {
        Options::with_mut(|options| options.use_file_based_fuzzer_agent = value);
    }

    /// Opção `usePredictionFileCreatingFuzzerAgent`.
    pub fn use_prediction_file_creating_fuzzer_agent() -> bool {
        Options::with(|options| options.use_prediction_file_creating_fuzzer_agent)
    }

    /// `Options::usePredictionFileCreatingFuzzerAgent() = value`.
    pub fn set_use_prediction_file_creating_fuzzer_agent(value: bool) {
        Options::with_mut(|options| options.use_prediction_file_creating_fuzzer_agent = value);
    }

    /// Opção `requirePredictionForFileBasedFuzzerAgent`.
    pub fn require_prediction_for_file_based_fuzzer_agent() -> bool {
        Options::with(|options| options.require_prediction_for_file_based_fuzzer_agent)
    }

    /// `Options::requirePredictionForFileBasedFuzzerAgent() = value`.
    pub fn set_require_prediction_for_file_based_fuzzer_agent(value: bool) {
        Options::with_mut(|options| options.require_prediction_for_file_based_fuzzer_agent = value);
    }

    /// Opção `fuzzerPredictionsFile`.
    pub fn fuzzer_predictions_file() -> Option<String> {
        Options::with(|options| options.fuzzer_predictions_file.clone())
    }

    /// `Options::fuzzerPredictionsFile() = value`.
    pub fn set_fuzzer_predictions_file(value: Option<String>) {
        Options::with_mut(|options| options.fuzzer_predictions_file = value);
    }

    /// Opção `useNarrowingNumberPredictionFuzzerAgent`.
    pub fn use_narrowing_number_prediction_fuzzer_agent() -> bool {
        Options::with(|options| options.use_narrowing_number_prediction_fuzzer_agent)
    }

    /// `Options::useNarrowingNumberPredictionFuzzerAgent() = value`.
    pub fn set_use_narrowing_number_prediction_fuzzer_agent(value: bool) {
        Options::with_mut(|options| options.use_narrowing_number_prediction_fuzzer_agent = value);
    }

    /// Opção `useWideningNumberPredictionFuzzerAgent`.
    pub fn use_widening_number_prediction_fuzzer_agent() -> bool {
        Options::with(|options| options.use_widening_number_prediction_fuzzer_agent)
    }

    /// `Options::useWideningNumberPredictionFuzzerAgent() = value`.
    pub fn set_use_widening_number_prediction_fuzzer_agent(value: bool) {
        Options::with_mut(|options| options.use_widening_number_prediction_fuzzer_agent = value);
    }

    /// Opção `logPhaseTimes`.
    pub fn log_phase_times() -> bool {
        Options::with(|options| options.log_phase_times)
    }

    /// `Options::logPhaseTimes() = value`.
    pub fn set_log_phase_times(value: bool) {
        Options::with_mut(|options| options.log_phase_times = value);
    }

    /// Opção `rareBlockPenalty`.
    pub fn rare_block_penalty() -> f64 {
        Options::with(|options| options.rare_block_penalty)
    }

    /// `Options::rareBlockPenalty() = value`.
    pub fn set_rare_block_penalty(value: f64) {
        Options::with_mut(|options| options.rare_block_penalty = value);
    }

    /// Opção `airGreedyRegAllocVerbose`.
    pub fn air_greedy_reg_alloc_verbose() -> bool {
        Options::with(|options| options.air_greedy_reg_alloc_verbose)
    }

    /// `Options::airGreedyRegAllocVerbose() = value`.
    pub fn set_air_greedy_reg_alloc_verbose(value: bool) {
        Options::with_mut(|options| options.air_greedy_reg_alloc_verbose = value);
    }

    /// Opção `airGreedyRegAllocDumpFunction`.
    pub fn air_greedy_reg_alloc_dump_function() -> Option<String> {
        Options::with(|options| options.air_greedy_reg_alloc_dump_function.clone())
    }

    /// `Options::airGreedyRegAllocDumpFunction() = value`.
    pub fn set_air_greedy_reg_alloc_dump_function(value: Option<String>) {
        Options::with_mut(|options| options.air_greedy_reg_alloc_dump_function = value);
    }

    /// Opção `airGreedyRegAllocSplitMultiplier`.
    pub fn air_greedy_reg_alloc_split_multiplier() -> f64 {
        Options::with(|options| options.air_greedy_reg_alloc_split_multiplier)
    }

    /// `Options::airGreedyRegAllocSplitMultiplier() = value`.
    pub fn set_air_greedy_reg_alloc_split_multiplier(value: f64) {
        Options::with_mut(|options| options.air_greedy_reg_alloc_split_multiplier = value);
    }

    /// Opção `airGreedyRegAllocSplitAroundLoops`.
    pub fn air_greedy_reg_alloc_split_around_loops() -> bool {
        Options::with(|options| options.air_greedy_reg_alloc_split_around_loops)
    }

    /// `Options::airGreedyRegAllocSplitAroundLoops() = value`.
    pub fn set_air_greedy_reg_alloc_split_around_loops(value: bool) {
        Options::with_mut(|options| options.air_greedy_reg_alloc_split_around_loops = value);
    }

    /// Opção `airGreedyRegAllocLoopSplitMaxLoopFraction`.
    pub fn air_greedy_reg_alloc_loop_split_max_loop_fraction() -> f64 {
        Options::with(|options| options.air_greedy_reg_alloc_loop_split_max_loop_fraction)
    }

    /// `Options::airGreedyRegAllocLoopSplitMaxLoopFraction() = value`.
    pub fn set_air_greedy_reg_alloc_loop_split_max_loop_fraction(value: f64) {
        Options::with_mut(|options| options.air_greedy_reg_alloc_loop_split_max_loop_fraction = value);
    }

    /// Opção `airGreedyRegAllocSpillsEverything`.
    pub fn air_greedy_reg_alloc_spills_everything() -> bool {
        Options::with(|options| options.air_greedy_reg_alloc_spills_everything)
    }

    /// `Options::airGreedyRegAllocSpillsEverything() = value`.
    pub fn set_air_greedy_reg_alloc_spills_everything(value: bool) {
        Options::with_mut(|options| options.air_greedy_reg_alloc_spills_everything = value);
    }

    /// Opção `airDumpPhaseStats`.
    pub fn air_dump_phase_stats() -> bool {
        Options::with(|options| options.air_dump_phase_stats)
    }

    /// `Options::airDumpPhaseStats() = value`.
    pub fn set_air_dump_phase_stats(value: bool) {
        Options::with_mut(|options| options.air_dump_phase_stats = value);
    }

    /// Opção `airValidateGreedRegAlloc`.
    pub fn air_validate_greed_reg_alloc() -> bool {
        Options::with(|options| options.air_validate_greed_reg_alloc)
    }

    /// `Options::airValidateGreedRegAlloc() = value`.
    pub fn set_air_validate_greed_reg_alloc(value: bool) {
        Options::with_mut(|options| options.air_validate_greed_reg_alloc = value);
    }

    /// Opção `airRandomizeRegs`.
    pub fn air_randomize_regs() -> bool {
        Options::with(|options| options.air_randomize_regs)
    }

    /// `Options::airRandomizeRegs() = value`.
    pub fn set_air_randomize_regs(value: bool) {
        Options::with_mut(|options| options.air_randomize_regs = value);
    }

    /// Opção `airRandomizeRegsSeed`.
    pub fn air_randomize_regs_seed() -> u32 {
        Options::with(|options| options.air_randomize_regs_seed)
    }

    /// `Options::airRandomizeRegsSeed() = value`.
    pub fn set_air_randomize_regs_seed(value: u32) {
        Options::with_mut(|options| options.air_randomize_regs_seed = value);
    }

    /// Opção `coalesceSpillSlots`.
    pub fn coalesce_spill_slots() -> bool {
        Options::with(|options| options.coalesce_spill_slots)
    }

    /// `Options::coalesceSpillSlots() = value`.
    pub fn set_coalesce_spill_slots(value: bool) {
        Options::with_mut(|options| options.coalesce_spill_slots = value);
    }

    /// Opção `logAirRegisterPressure`.
    pub fn log_air_register_pressure() -> bool {
        Options::with(|options| options.log_air_register_pressure)
    }

    /// `Options::logAirRegisterPressure() = value`.
    pub fn set_log_air_register_pressure(value: bool) {
        Options::with_mut(|options| options.log_air_register_pressure = value);
    }

    /// Opção `useB3TailDup`.
    pub fn use_b3_tail_dup() -> bool {
        Options::with(|options| options.use_b3_tail_dup)
    }

    /// `Options::useB3TailDup() = value`.
    pub fn set_use_b3_tail_dup(value: bool) {
        Options::with_mut(|options| options.use_b3_tail_dup = value);
    }

    /// Opção `maxB3TailDupBlockSize`.
    pub fn max_b3_tail_dup_block_size() -> u32 {
        Options::with(|options| options.max_b3_tail_dup_block_size)
    }

    /// `Options::maxB3TailDupBlockSize() = value`.
    pub fn set_max_b3_tail_dup_block_size(value: u32) {
        Options::with_mut(|options| options.max_b3_tail_dup_block_size = value);
    }

    /// Opção `maxB3TailDupBlockSuccessors`.
    pub fn max_b3_tail_dup_block_successors() -> u32 {
        Options::with(|options| options.max_b3_tail_dup_block_successors)
    }

    /// `Options::maxB3TailDupBlockSuccessors() = value`.
    pub fn set_max_b3_tail_dup_block_successors(value: u32) {
        Options::with_mut(|options| options.max_b3_tail_dup_block_successors = value);
    }

    /// Opção `useB3HoistLoopInvariantValues`.
    pub fn use_b3_hoist_loop_invariant_values() -> bool {
        Options::with(|options| options.use_b3_hoist_loop_invariant_values)
    }

    /// `Options::useB3HoistLoopInvariantValues() = value`.
    pub fn set_use_b3_hoist_loop_invariant_values(value: bool) {
        Options::with_mut(|options| options.use_b3_hoist_loop_invariant_values = value);
    }

    /// Opção `useB3CanonicalizePrePostIncrements`.
    pub fn use_b3_canonicalize_pre_post_increments() -> bool {
        Options::with(|options| options.use_b3_canonicalize_pre_post_increments)
    }

    /// `Options::useB3CanonicalizePrePostIncrements() = value`.
    pub fn set_use_b3_canonicalize_pre_post_increments(value: bool) {
        Options::with_mut(|options| options.use_b3_canonicalize_pre_post_increments = value);
    }

    /// Opção `useB3EliminateWasmGCAllocations`.
    pub fn use_b3_eliminate_wasm_gc_allocations() -> bool {
        Options::with(|options| options.use_b3_eliminate_wasm_gc_allocations)
    }

    /// `Options::useB3EliminateWasmGCAllocations() = value`.
    pub fn set_use_b3_eliminate_wasm_gc_allocations(value: bool) {
        Options::with_mut(|options| options.use_b3_eliminate_wasm_gc_allocations = value);
    }

    /// Opção `useB3ReduceStrengthFixpoint`.
    pub fn use_b3_reduce_strength_fixpoint() -> bool {
        Options::with(|options| options.use_b3_reduce_strength_fixpoint)
    }

    /// `Options::useB3ReduceStrengthFixpoint() = value`.
    pub fn set_use_b3_reduce_strength_fixpoint(value: bool) {
        Options::with_mut(|options| options.use_b3_reduce_strength_fixpoint = value);
    }

    /// Opção `useAirOptimizePairedLoadStore`.
    pub fn use_air_optimize_paired_load_store() -> bool {
        Options::with(|options| options.use_air_optimize_paired_load_store)
    }

    /// `Options::useAirOptimizePairedLoadStore() = value`.
    pub fn set_use_air_optimize_paired_load_store(value: bool) {
        Options::with_mut(|options| options.use_air_optimize_paired_load_store = value);
    }

    /// Opção `useDollarVM`.
    pub fn use_dollar_vm() -> bool {
        Options::with(|options| options.use_dollar_vm)
    }

    /// `Options::useDollarVM() = value`.
    pub fn set_use_dollar_vm(value: bool) {
        Options::with_mut(|options| options.use_dollar_vm = value);
    }

    /// Opção `functionOverrides`.
    pub fn function_overrides() -> Option<String> {
        Options::with(|options| options.function_overrides.clone())
    }

    /// `Options::functionOverrides() = value`.
    pub fn set_function_overrides(value: Option<String>) {
        Options::with_mut(|options| options.function_overrides = value);
    }

    /// Opção `watchdog`.
    pub fn watchdog() -> u32 {
        Options::with(|options| options.watchdog)
    }

    /// `Options::watchdog() = value`.
    pub fn set_watchdog(value: u32) {
        Options::with_mut(|options| options.watchdog = value);
    }

    /// Opção `usePollingTraps`.
    pub fn use_polling_traps() -> bool {
        Options::with(|options| options.use_polling_traps)
    }

    /// `Options::usePollingTraps() = value`.
    pub fn set_use_polling_traps(value: bool) {
        Options::with_mut(|options| options.use_polling_traps = value);
    }

    /// Opção `forceTrapAwareStackChecks`.
    pub fn force_trap_aware_stack_checks() -> bool {
        Options::with(|options| options.force_trap_aware_stack_checks)
    }

    /// `Options::forceTrapAwareStackChecks() = value`.
    pub fn set_force_trap_aware_stack_checks(value: bool) {
        Options::with_mut(|options| options.force_trap_aware_stack_checks = value);
    }

    /// Opção `useMachForExceptions`.
    pub fn use_mach_for_exceptions() -> bool {
        Options::with(|options| options.use_mach_for_exceptions)
    }

    /// `Options::useMachForExceptions() = value`.
    pub fn set_use_mach_for_exceptions(value: bool) {
        Options::with_mut(|options| options.use_mach_for_exceptions = value);
    }

    /// Opção `allowNonSPTagging`.
    pub fn allow_non_sp_tagging() -> bool {
        Options::with(|options| options.allow_non_sp_tagging)
    }

    /// `Options::allowNonSPTagging() = value`.
    pub fn set_allow_non_sp_tagging(value: bool) {
        Options::with_mut(|options| options.allow_non_sp_tagging = value);
    }

    /// Opção `useICStats`.
    pub fn use_ic_stats() -> bool {
        Options::with(|options| options.use_ic_stats)
    }

    /// `Options::useICStats() = value`.
    pub fn set_use_ic_stats(value: bool) {
        Options::with_mut(|options| options.use_ic_stats = value);
    }

    /// Opção `useFuzzerMode`.
    pub fn use_fuzzer_mode() -> bool {
        Options::with(|options| options.use_fuzzer_mode)
    }

    /// `Options::useFuzzerMode() = value`.
    pub fn set_use_fuzzer_mode(value: bool) {
        Options::with_mut(|options| options.use_fuzzer_mode = value);
    }

    /// Opção `prototypeHitCountForLLIntCaching`.
    pub fn prototype_hit_count_for_ll_int_caching() -> u32 {
        Options::with(|options| options.prototype_hit_count_for_ll_int_caching)
    }

    /// `Options::prototypeHitCountForLLIntCaching() = value`.
    pub fn set_prototype_hit_count_for_ll_int_caching(value: u32) {
        Options::with_mut(|options| options.prototype_hit_count_for_ll_int_caching = value);
    }

    /// Opção `dumpCompiledRegExpPatterns`.
    pub fn dump_compiled_reg_exp_patterns() -> bool {
        Options::with(|options| options.dump_compiled_reg_exp_patterns)
    }

    /// `Options::dumpCompiledRegExpPatterns() = value`.
    pub fn set_dump_compiled_reg_exp_patterns(value: bool) {
        Options::with_mut(|options| options.dump_compiled_reg_exp_patterns = value);
    }

    /// Opção `verboseRegExpCompilation`.
    pub fn verbose_reg_exp_compilation() -> bool {
        Options::with(|options| options.verbose_reg_exp_compilation)
    }

    /// `Options::verboseRegExpCompilation() = value`.
    pub fn set_verbose_reg_exp_compilation(value: bool) {
        Options::with_mut(|options| options.verbose_reg_exp_compilation = value);
    }

    /// Opção `dumpModuleRecord`.
    pub fn dump_module_record() -> bool {
        Options::with(|options| options.dump_module_record)
    }

    /// `Options::dumpModuleRecord() = value`.
    pub fn set_dump_module_record(value: bool) {
        Options::with_mut(|options| options.dump_module_record = value);
    }

    /// Opção `dumpModuleLoadingState`.
    pub fn dump_module_loading_state() -> bool {
        Options::with(|options| options.dump_module_loading_state)
    }

    /// `Options::dumpModuleLoadingState() = value`.
    pub fn set_dump_module_loading_state(value: bool) {
        Options::with_mut(|options| options.dump_module_loading_state = value);
    }

    /// Opção `exposeInternalModuleLoader`.
    pub fn expose_internal_module_loader() -> bool {
        Options::with(|options| options.expose_internal_module_loader)
    }

    /// `Options::exposeInternalModuleLoader() = value`.
    pub fn set_expose_internal_module_loader(value: bool) {
        Options::with_mut(|options| options.expose_internal_module_loader = value);
    }

    /// Opção `exposePrivateIdentifiers`.
    pub fn expose_private_identifiers() -> bool {
        Options::with(|options| options.expose_private_identifiers)
    }

    /// `Options::exposePrivateIdentifiers() = value`.
    pub fn set_expose_private_identifiers(value: bool) {
        Options::with_mut(|options| options.expose_private_identifiers = value);
    }

    /// Opção `useSuperSampler`.
    pub fn use_super_sampler() -> bool {
        Options::with(|options| options.use_super_sampler)
    }

    /// `Options::useSuperSampler() = value`.
    pub fn set_use_super_sampler(value: bool) {
        Options::with_mut(|options| options.use_super_sampler = value);
    }

    /// Opção `useSourceProviderCache`.
    pub fn use_source_provider_cache() -> bool {
        Options::with(|options| options.use_source_provider_cache)
    }

    /// `Options::useSourceProviderCache() = value`.
    pub fn set_use_source_provider_cache(value: bool) {
        Options::with_mut(|options| options.use_source_provider_cache = value);
    }

    /// Opção `useCodeCache`.
    pub fn use_code_cache() -> bool {
        Options::with(|options| options.use_code_cache)
    }

    /// `Options::useCodeCache() = value`.
    pub fn set_use_code_cache(value: bool) {
        Options::with_mut(|options| options.use_code_cache = value);
    }

    /// Opção `useWasm`.
    pub fn use_wasm() -> bool {
        Options::with(|options| options.use_wasm)
    }

    /// `Options::useWasm() = value`.
    pub fn set_use_wasm(value: bool) {
        Options::with_mut(|options| options.use_wasm = value);
    }

    /// Opção `failToCompileWasmCode`.
    pub fn fail_to_compile_wasm_code() -> bool {
        Options::with(|options| options.fail_to_compile_wasm_code)
    }

    /// `Options::failToCompileWasmCode() = value`.
    pub fn set_fail_to_compile_wasm_code(value: bool) {
        Options::with_mut(|options| options.fail_to_compile_wasm_code = value);
    }

    /// Opção `wasmSmallPartialCompileLimit`.
    pub fn wasm_small_partial_compile_limit() -> usize {
        Options::with(|options| options.wasm_small_partial_compile_limit)
    }

    /// `Options::wasmSmallPartialCompileLimit() = value`.
    pub fn set_wasm_small_partial_compile_limit(value: usize) {
        Options::with_mut(|options| options.wasm_small_partial_compile_limit = value);
    }

    /// Opção `wasmLargePartialCompileLimit`.
    pub fn wasm_large_partial_compile_limit() -> usize {
        Options::with(|options| options.wasm_large_partial_compile_limit)
    }

    /// `Options::wasmLargePartialCompileLimit() = value`.
    pub fn set_wasm_large_partial_compile_limit(value: usize) {
        Options::with_mut(|options| options.wasm_large_partial_compile_limit = value);
    }

    /// Opção `wasmOMGOptimizationLevel`.
    pub fn wasm_omg_optimization_level() -> u32 {
        Options::with(|options| options.wasm_omg_optimization_level)
    }

    /// `Options::wasmOMGOptimizationLevel() = value`.
    pub fn set_wasm_omg_optimization_level(value: u32) {
        Options::with_mut(|options| options.wasm_omg_optimization_level = value);
    }

    /// Opção `useWasmByteLoopReplacement`.
    pub fn use_wasm_byte_loop_replacement() -> bool {
        Options::with(|options| options.use_wasm_byte_loop_replacement)
    }

    /// `Options::useWasmByteLoopReplacement() = value`.
    pub fn set_use_wasm_byte_loop_replacement(value: bool) {
        Options::with_mut(|options| options.use_wasm_byte_loop_replacement = value);
    }

    /// Opção `useBBQTierUpChecks`.
    pub fn use_bbq_tier_up_checks() -> bool {
        Options::with(|options| options.use_bbq_tier_up_checks)
    }

    /// `Options::useBBQTierUpChecks() = value`.
    pub fn set_use_bbq_tier_up_checks(value: bool) {
        Options::with_mut(|options| options.use_bbq_tier_up_checks = value);
    }

    /// Opção `useWasmOSR`.
    pub fn use_wasm_osr() -> bool {
        Options::with(|options| options.use_wasm_osr)
    }

    /// `Options::useWasmOSR() = value`.
    pub fn set_use_wasm_osr(value: bool) {
        Options::with_mut(|options| options.use_wasm_osr = value);
    }

    /// Opção `thresholdForBBQOptimizeAfterWarmUp`.
    pub fn threshold_for_bbq_optimize_after_warm_up() -> i32 {
        Options::with(|options| options.threshold_for_bbq_optimize_after_warm_up)
    }

    /// `Options::thresholdForBBQOptimizeAfterWarmUp() = value`.
    pub fn set_threshold_for_bbq_optimize_after_warm_up(value: i32) {
        Options::with_mut(|options| options.threshold_for_bbq_optimize_after_warm_up = value);
    }

    /// Opção `thresholdForBBQOptimizeSoon`.
    pub fn threshold_for_bbq_optimize_soon() -> i32 {
        Options::with(|options| options.threshold_for_bbq_optimize_soon)
    }

    /// `Options::thresholdForBBQOptimizeSoon() = value`.
    pub fn set_threshold_for_bbq_optimize_soon(value: i32) {
        Options::with_mut(|options| options.threshold_for_bbq_optimize_soon = value);
    }

    /// Opção `thresholdForOMGOptimizeAfterWarmUp`.
    pub fn threshold_for_omg_optimize_after_warm_up() -> i32 {
        Options::with(|options| options.threshold_for_omg_optimize_after_warm_up)
    }

    /// `Options::thresholdForOMGOptimizeAfterWarmUp() = value`.
    pub fn set_threshold_for_omg_optimize_after_warm_up(value: i32) {
        Options::with_mut(|options| options.threshold_for_omg_optimize_after_warm_up = value);
    }

    /// Opção `thresholdForOMGOptimizeSoon`.
    pub fn threshold_for_omg_optimize_soon() -> i32 {
        Options::with(|options| options.threshold_for_omg_optimize_soon)
    }

    /// `Options::thresholdForOMGOptimizeSoon() = value`.
    pub fn set_threshold_for_omg_optimize_soon(value: i32) {
        Options::with_mut(|options| options.threshold_for_omg_optimize_soon = value);
    }

    /// Opção `maximumOMGCandidateCost`.
    pub fn maximum_omg_candidate_cost() -> u32 {
        Options::with(|options| options.maximum_omg_candidate_cost)
    }

    /// `Options::maximumOMGCandidateCost() = value`.
    pub fn set_maximum_omg_candidate_cost(value: u32) {
        Options::with_mut(|options| options.maximum_omg_candidate_cost = value);
    }

    /// Opção `omgTierUpCounterIncrementForLoop`.
    pub fn omg_tier_up_counter_increment_for_loop() -> i32 {
        Options::with(|options| options.omg_tier_up_counter_increment_for_loop)
    }

    /// `Options::omgTierUpCounterIncrementForLoop() = value`.
    pub fn set_omg_tier_up_counter_increment_for_loop(value: i32) {
        Options::with_mut(|options| options.omg_tier_up_counter_increment_for_loop = value);
    }

    /// Opção `omgTierUpCounterIncrementForEntry`.
    pub fn omg_tier_up_counter_increment_for_entry() -> i32 {
        Options::with(|options| options.omg_tier_up_counter_increment_for_entry)
    }

    /// `Options::omgTierUpCounterIncrementForEntry() = value`.
    pub fn set_omg_tier_up_counter_increment_for_entry(value: i32) {
        Options::with_mut(|options| options.omg_tier_up_counter_increment_for_entry = value);
    }

    /// Opção `wasmOMGEntryIncrementSizeReference`.
    pub fn wasm_omg_entry_increment_size_reference() -> i32 {
        Options::with(|options| options.wasm_omg_entry_increment_size_reference)
    }

    /// `Options::wasmOMGEntryIncrementSizeReference() = value`.
    pub fn set_wasm_omg_entry_increment_size_reference(value: i32) {
        Options::with_mut(|options| options.wasm_omg_entry_increment_size_reference = value);
    }

    /// Opção `useWasmFastMemory`.
    pub fn use_wasm_fast_memory() -> bool {
        Options::with(|options| options.use_wasm_fast_memory)
    }

    /// `Options::useWasmFastMemory() = value`.
    pub fn set_use_wasm_fast_memory(value: bool) {
        Options::with_mut(|options| options.use_wasm_fast_memory = value);
    }

    /// Opção `logWasmMemory`.
    pub fn log_wasm_memory() -> bool {
        Options::with(|options| options.log_wasm_memory)
    }

    /// `Options::logWasmMemory() = value`.
    pub fn set_log_wasm_memory(value: bool) {
        Options::with_mut(|options| options.log_wasm_memory = value);
    }

    /// Opção `wasmFastMemoryRedzonePages`.
    pub fn wasm_fast_memory_redzone_pages() -> u32 {
        Options::with(|options| options.wasm_fast_memory_redzone_pages)
    }

    /// `Options::wasmFastMemoryRedzonePages() = value`.
    pub fn set_wasm_fast_memory_redzone_pages(value: u32) {
        Options::with_mut(|options| options.wasm_fast_memory_redzone_pages = value);
    }

    /// Opção `crashIfWasmCantFastMemory`.
    pub fn crash_if_wasm_cant_fast_memory() -> bool {
        Options::with(|options| options.crash_if_wasm_cant_fast_memory)
    }

    /// `Options::crashIfWasmCantFastMemory() = value`.
    pub fn set_crash_if_wasm_cant_fast_memory(value: bool) {
        Options::with_mut(|options| options.crash_if_wasm_cant_fast_memory = value);
    }

    /// Opção `crashOnFailedWasmValidate`.
    pub fn crash_on_failed_wasm_validate() -> bool {
        Options::with(|options| options.crash_on_failed_wasm_validate)
    }

    /// `Options::crashOnFailedWasmValidate() = value`.
    pub fn set_crash_on_failed_wasm_validate(value: bool) {
        Options::with_mut(|options| options.crash_on_failed_wasm_validate = value);
    }

    /// Opção `maxNumWasmFastMemories`.
    pub fn max_num_wasm_fast_memories() -> u32 {
        Options::with(|options| options.max_num_wasm_fast_memories)
    }

    /// `Options::maxNumWasmFastMemories() = value`.
    pub fn set_max_num_wasm_fast_memories(value: u32) {
        Options::with_mut(|options| options.max_num_wasm_fast_memories = value);
    }

    /// Opção `verboseBBQJITAllocation`.
    pub fn verbose_bbqjit_allocation() -> bool {
        Options::with(|options| options.verbose_bbqjit_allocation)
    }

    /// `Options::verboseBBQJITAllocation() = value`.
    pub fn set_verbose_bbqjit_allocation(value: bool) {
        Options::with_mut(|options| options.verbose_bbqjit_allocation = value);
    }

    /// Opção `verboseBBQJITInstructions`.
    pub fn verbose_bbqjit_instructions() -> bool {
        Options::with(|options| options.verbose_bbqjit_instructions)
    }

    /// `Options::verboseBBQJITInstructions() = value`.
    pub fn set_verbose_bbqjit_instructions(value: bool) {
        Options::with_mut(|options| options.verbose_bbqjit_instructions = value);
    }

    /// Opção `disableBBQConsts`.
    pub fn disable_bbq_consts() -> bool {
        Options::with(|options| options.disable_bbq_consts)
    }

    /// `Options::disableBBQConsts() = value`.
    pub fn set_disable_bbq_consts(value: bool) {
        Options::with_mut(|options| options.disable_bbq_consts = value);
    }

    /// Opção `useBBQJIT`.
    pub fn use_bbqjit() -> bool {
        Options::with(|options| options.use_bbqjit)
    }

    /// `Options::useBBQJIT() = value`.
    pub fn set_use_bbqjit(value: bool) {
        Options::with_mut(|options| options.use_bbqjit = value);
    }

    /// Opção `useOMGJIT`.
    pub fn use_omgjit() -> bool {
        Options::with(|options| options.use_omgjit)
    }

    /// `Options::useOMGJIT() = value`.
    pub fn set_use_omgjit(value: bool) {
        Options::with_mut(|options| options.use_omgjit = value);
    }

    /// Opção `wasmFunctionIndexRangeToCompile`.
    pub fn wasm_function_index_range_to_compile() -> OptionRange {
        Options::with(|options| options.wasm_function_index_range_to_compile.clone())
    }

    /// `Options::wasmFunctionIndexRangeToCompile() = value`.
    pub fn set_wasm_function_index_range_to_compile(value: OptionRange) {
        Options::with_mut(|options| options.wasm_function_index_range_to_compile = value);
    }

    /// Opção `useEagerWasmModuleHashing`.
    pub fn use_eager_wasm_module_hashing() -> bool {
        Options::with(|options| options.use_eager_wasm_module_hashing)
    }

    /// `Options::useEagerWasmModuleHashing() = value`.
    pub fn set_use_eager_wasm_module_hashing(value: bool) {
        Options::with_mut(|options| options.use_eager_wasm_module_hashing = value);
    }

    /// Opção `useArrayAllocationProfiling`.
    pub fn use_array_allocation_profiling() -> bool {
        Options::with(|options| options.use_array_allocation_profiling)
    }

    /// `Options::useArrayAllocationProfiling() = value`.
    pub fn set_use_array_allocation_profiling(value: bool) {
        Options::with_mut(|options| options.use_array_allocation_profiling = value);
    }

    /// Opção `forcePolyProto`.
    pub fn force_poly_proto() -> bool {
        Options::with(|options| options.force_poly_proto)
    }

    /// `Options::forcePolyProto() = value`.
    pub fn set_force_poly_proto(value: bool) {
        Options::with_mut(|options| options.force_poly_proto = value);
    }

    /// Opção `forceMiniVMMode`.
    pub fn force_mini_vm_mode() -> bool {
        Options::with(|options| options.force_mini_vm_mode)
    }

    /// `Options::forceMiniVMMode() = value`.
    pub fn set_force_mini_vm_mode(value: bool) {
        Options::with_mut(|options| options.force_mini_vm_mode = value);
    }

    /// Opção `useTracePoints`.
    pub fn use_trace_points() -> bool {
        Options::with(|options| options.use_trace_points)
    }

    /// `Options::useTracePoints() = value`.
    pub fn set_use_trace_points(value: bool) {
        Options::with_mut(|options| options.use_trace_points = value);
    }

    /// Opção `useCompilerSignpost`.
    pub fn use_compiler_signpost() -> bool {
        Options::with(|options| options.use_compiler_signpost)
    }

    /// `Options::useCompilerSignpost() = value`.
    pub fn set_use_compiler_signpost(value: bool) {
        Options::with_mut(|options| options.use_compiler_signpost = value);
    }

    /// Opção `useGCSignpost`.
    pub fn use_gc_signpost() -> bool {
        Options::with(|options| options.use_gc_signpost)
    }

    /// `Options::useGCSignpost() = value`.
    pub fn set_use_gc_signpost(value: bool) {
        Options::with_mut(|options| options.use_gc_signpost = value);
    }

    /// Opção `traceLLIntExecution`.
    pub fn trace_ll_int_execution() -> bool {
        Options::with(|options| options.trace_ll_int_execution)
    }

    /// `Options::traceLLIntExecution() = value`.
    pub fn set_trace_ll_int_execution(value: bool) {
        Options::with_mut(|options| options.trace_ll_int_execution = value);
    }

    /// Opção `traceLLIntSlowPath`.
    pub fn trace_ll_int_slow_path() -> bool {
        Options::with(|options| options.trace_ll_int_slow_path)
    }

    /// `Options::traceLLIntSlowPath() = value`.
    pub fn set_trace_ll_int_slow_path(value: bool) {
        Options::with_mut(|options| options.trace_ll_int_slow_path = value);
    }

    /// Opção `traceBaselineJITExecution`.
    pub fn trace_baseline_jit_execution() -> bool {
        Options::with(|options| options.trace_baseline_jit_execution)
    }

    /// `Options::traceBaselineJITExecution() = value`.
    pub fn set_trace_baseline_jit_execution(value: bool) {
        Options::with_mut(|options| options.trace_baseline_jit_execution = value);
    }

    /// Opção `thresholdForGlobalLexicalBindingEpoch`.
    pub fn threshold_for_global_lexical_binding_epoch() -> u32 {
        Options::with(|options| options.threshold_for_global_lexical_binding_epoch)
    }

    /// `Options::thresholdForGlobalLexicalBindingEpoch() = value`.
    pub fn set_threshold_for_global_lexical_binding_epoch(value: u32) {
        Options::with_mut(|options| options.threshold_for_global_lexical_binding_epoch = value);
    }

    /// Opção `diskCachePath`.
    pub fn disk_cache_path() -> Option<String> {
        Options::with(|options| options.disk_cache_path.clone())
    }

    /// `Options::diskCachePath() = value`.
    pub fn set_disk_cache_path(value: Option<String>) {
        Options::with_mut(|options| options.disk_cache_path = value);
    }

    /// Opção `verboseDiskCache`.
    pub fn verbose_disk_cache() -> bool {
        Options::with(|options| options.verbose_disk_cache)
    }

    /// `Options::verboseDiskCache() = value`.
    pub fn set_verbose_disk_cache(value: bool) {
        Options::with_mut(|options| options.verbose_disk_cache = value);
    }

    /// Opção `forceDiskCache`.
    pub fn force_disk_cache() -> bool {
        Options::with(|options| options.force_disk_cache)
    }

    /// `Options::forceDiskCache() = value`.
    pub fn set_force_disk_cache(value: bool) {
        Options::with_mut(|options| options.force_disk_cache = value);
    }

    /// Opção `validateAbstractInterpreterState`.
    pub fn validate_abstract_interpreter_state() -> bool {
        Options::with(|options| options.validate_abstract_interpreter_state)
    }

    /// `Options::validateAbstractInterpreterState() = value`.
    pub fn set_validate_abstract_interpreter_state(value: bool) {
        Options::with_mut(|options| options.validate_abstract_interpreter_state = value);
    }

    /// Opção `validateAbstractInterpreterStateProbability`.
    pub fn validate_abstract_interpreter_state_probability() -> f64 {
        Options::with(|options| options.validate_abstract_interpreter_state_probability)
    }

    /// `Options::validateAbstractInterpreterStateProbability() = value`.
    pub fn set_validate_abstract_interpreter_state_probability(value: f64) {
        Options::with_mut(|options| options.validate_abstract_interpreter_state_probability = value);
    }

    /// Opção `dumpJITMemoryPath`.
    pub fn dump_jit_memory_path() -> Option<String> {
        Options::with(|options| options.dump_jit_memory_path.clone())
    }

    /// `Options::dumpJITMemoryPath() = value`.
    pub fn set_dump_jit_memory_path(value: Option<String>) {
        Options::with_mut(|options| options.dump_jit_memory_path = value);
    }

    /// Opção `dumpJITMemoryFlushInterval`.
    pub fn dump_jit_memory_flush_interval() -> f64 {
        Options::with(|options| options.dump_jit_memory_flush_interval)
    }

    /// `Options::dumpJITMemoryFlushInterval() = value`.
    pub fn set_dump_jit_memory_flush_interval(value: f64) {
        Options::with_mut(|options| options.dump_jit_memory_flush_interval = value);
    }

    /// Opção `useUnlinkedCodeBlockJettisoning`.
    pub fn use_unlinked_code_block_jettisoning() -> bool {
        Options::with(|options| options.use_unlinked_code_block_jettisoning)
    }

    /// `Options::useUnlinkedCodeBlockJettisoning() = value`.
    pub fn set_use_unlinked_code_block_jettisoning(value: bool) {
        Options::with_mut(|options| options.use_unlinked_code_block_jettisoning = value);
    }

    /// Opção `forceOSRExitToLLInt`.
    pub fn force_osr_exit_to_ll_int() -> bool {
        Options::with(|options| options.force_osr_exit_to_ll_int)
    }

    /// `Options::forceOSRExitToLLInt() = value`.
    pub fn set_force_osr_exit_to_ll_int(value: bool) {
        Options::with_mut(|options| options.force_osr_exit_to_ll_int = value);
    }

    /// Opção `getByValICMaxNumberOfIdentifiers`.
    pub fn get_by_val_ic_max_number_of_identifiers() -> u32 {
        Options::with(|options| options.get_by_val_ic_max_number_of_identifiers)
    }

    /// `Options::getByValICMaxNumberOfIdentifiers() = value`.
    pub fn set_get_by_val_ic_max_number_of_identifiers(value: u32) {
        Options::with_mut(|options| options.get_by_val_ic_max_number_of_identifiers = value);
    }

    /// Opção `useRandomizingExecutableIslandAllocation`.
    pub fn use_randomizing_executable_island_allocation() -> bool {
        Options::with(|options| options.use_randomizing_executable_island_allocation)
    }

    /// `Options::useRandomizingExecutableIslandAllocation() = value`.
    pub fn set_use_randomizing_executable_island_allocation(value: bool) {
        Options::with_mut(|options| options.use_randomizing_executable_island_allocation = value);
    }

    /// Opção `exposeProfilersOnGlobalObject`.
    pub fn expose_profilers_on_global_object() -> bool {
        Options::with(|options| options.expose_profilers_on_global_object)
    }

    /// `Options::exposeProfilersOnGlobalObject() = value`.
    pub fn set_expose_profilers_on_global_object(value: bool) {
        Options::with_mut(|options| options.expose_profilers_on_global_object = value);
    }

    /// Opção `allowUnsupportedTiers`.
    pub fn allow_unsupported_tiers() -> bool {
        Options::with(|options| options.allow_unsupported_tiers)
    }

    /// `Options::allowUnsupportedTiers() = value`.
    pub fn set_allow_unsupported_tiers(value: bool) {
        Options::with_mut(|options| options.allow_unsupported_tiers = value);
    }

    /// Opção `returnEarlyFromInfiniteLoopsForFuzzing`.
    pub fn return_early_from_infinite_loops_for_fuzzing() -> bool {
        Options::with(|options| options.return_early_from_infinite_loops_for_fuzzing)
    }

    /// `Options::returnEarlyFromInfiniteLoopsForFuzzing() = value`.
    pub fn set_return_early_from_infinite_loops_for_fuzzing(value: bool) {
        Options::with_mut(|options| options.return_early_from_infinite_loops_for_fuzzing = value);
    }

    /// Opção `earlyReturnFromInfiniteLoopsLimit`.
    pub fn early_return_from_infinite_loops_limit() -> usize {
        Options::with(|options| options.early_return_from_infinite_loops_limit)
    }

    /// `Options::earlyReturnFromInfiniteLoopsLimit() = value`.
    pub fn set_early_return_from_infinite_loops_limit(value: usize) {
        Options::with_mut(|options| options.early_return_from_infinite_loops_limit = value);
    }

    /// Opção `useLICMFuzzing`.
    pub fn use_licm_fuzzing() -> bool {
        Options::with(|options| options.use_licm_fuzzing)
    }

    /// `Options::useLICMFuzzing() = value`.
    pub fn set_use_licm_fuzzing(value: bool) {
        Options::with_mut(|options| options.use_licm_fuzzing = value);
    }

    /// Opção `seedForLICMFuzzer`.
    pub fn seed_for_licm_fuzzer() -> u32 {
        Options::with(|options| options.seed_for_licm_fuzzer)
    }

    /// `Options::seedForLICMFuzzer() = value`.
    pub fn set_seed_for_licm_fuzzer(value: u32) {
        Options::with_mut(|options| options.seed_for_licm_fuzzer = value);
    }

    /// Opção `allowHoistingLICMProbability`.
    pub fn allow_hoisting_licm_probability() -> f64 {
        Options::with(|options| options.allow_hoisting_licm_probability)
    }

    /// `Options::allowHoistingLICMProbability() = value`.
    pub fn set_allow_hoisting_licm_probability(value: f64) {
        Options::with_mut(|options| options.allow_hoisting_licm_probability = value);
    }

    /// Opção `exposeCustomSettersOnGlobalObjectForTesting`.
    pub fn expose_custom_setters_on_global_object_for_testing() -> bool {
        Options::with(|options| options.expose_custom_setters_on_global_object_for_testing)
    }

    /// `Options::exposeCustomSettersOnGlobalObjectForTesting() = value`.
    pub fn set_expose_custom_setters_on_global_object_for_testing(value: bool) {
        Options::with_mut(|options| options.expose_custom_setters_on_global_object_for_testing = value);
    }

    /// Opção `useJITCage`.
    pub fn use_jit_cage() -> bool {
        Options::with(|options| options.use_jit_cage)
    }

    /// `Options::useJITCage() = value`.
    pub fn set_use_jit_cage(value: bool) {
        Options::with_mut(|options| options.use_jit_cage = value);
    }

    /// Opção `useAllocationProfiling`.
    pub fn use_allocation_profiling() -> bool {
        Options::with(|options| options.use_allocation_profiling)
    }

    /// `Options::useAllocationProfiling() = value`.
    pub fn set_use_allocation_profiling(value: bool) {
        Options::with_mut(|options| options.use_allocation_profiling = value);
    }

    /// Opção `allocationProfilingMode`.
    pub fn allocation_profiling_mode() -> u32 {
        Options::with(|options| options.allocation_profiling_mode)
    }

    /// `Options::allocationProfilingMode() = value`.
    pub fn set_allocation_profiling_mode(value: u32) {
        Options::with_mut(|options| options.allocation_profiling_mode = value);
    }

    /// Opção `dumpBaselineJITSizeStatistics`.
    pub fn dump_baseline_jit_size_statistics() -> bool {
        Options::with(|options| options.dump_baseline_jit_size_statistics)
    }

    /// `Options::dumpBaselineJITSizeStatistics() = value`.
    pub fn set_dump_baseline_jit_size_statistics(value: bool) {
        Options::with_mut(|options| options.dump_baseline_jit_size_statistics = value);
    }

    /// Opção `dumpDFGJITSizeStatistics`.
    pub fn dump_dfgjit_size_statistics() -> bool {
        Options::with(|options| options.dump_dfgjit_size_statistics)
    }

    /// `Options::dumpDFGJITSizeStatistics() = value`.
    pub fn set_dump_dfgjit_size_statistics(value: bool) {
        Options::with_mut(|options| options.dump_dfgjit_size_statistics = value);
    }

    /// Opção `useLoopUnrolling`.
    pub fn use_loop_unrolling() -> bool {
        Options::with(|options| options.use_loop_unrolling)
    }

    /// `Options::useLoopUnrolling() = value`.
    pub fn set_use_loop_unrolling(value: bool) {
        Options::with_mut(|options| options.use_loop_unrolling = value);
    }

    /// Opção `usePartialLoopUnrolling`.
    pub fn use_partial_loop_unrolling() -> bool {
        Options::with(|options| options.use_partial_loop_unrolling)
    }

    /// `Options::usePartialLoopUnrolling() = value`.
    pub fn set_use_partial_loop_unrolling(value: bool) {
        Options::with_mut(|options| options.use_partial_loop_unrolling = value);
    }

    /// Opção `verboseLoopUnrolling`.
    pub fn verbose_loop_unrolling() -> bool {
        Options::with(|options| options.verbose_loop_unrolling)
    }

    /// `Options::verboseLoopUnrolling() = value`.
    pub fn set_verbose_loop_unrolling(value: bool) {
        Options::with_mut(|options| options.verbose_loop_unrolling = value);
    }

    /// Opção `disallowLoopUnrollingForNonInnermost`.
    pub fn disallow_loop_unrolling_for_non_innermost() -> bool {
        Options::with(|options| options.disallow_loop_unrolling_for_non_innermost)
    }

    /// `Options::disallowLoopUnrollingForNonInnermost() = value`.
    pub fn set_disallow_loop_unrolling_for_non_innermost(value: bool) {
        Options::with_mut(|options| options.disallow_loop_unrolling_for_non_innermost = value);
    }

    /// Opção `maxLoopUnrollingCount`.
    pub fn max_loop_unrolling_count() -> u32 {
        Options::with(|options| options.max_loop_unrolling_count)
    }

    /// `Options::maxLoopUnrollingCount() = value`.
    pub fn set_max_loop_unrolling_count(value: u32) {
        Options::with_mut(|options| options.max_loop_unrolling_count = value);
    }

    /// Opção `maxLoopUnrollingBodyNodeSize`.
    pub fn max_loop_unrolling_body_node_size() -> u32 {
        Options::with(|options| options.max_loop_unrolling_body_node_size)
    }

    /// `Options::maxLoopUnrollingBodyNodeSize() = value`.
    pub fn set_max_loop_unrolling_body_node_size(value: u32) {
        Options::with_mut(|options| options.max_loop_unrolling_body_node_size = value);
    }

    /// Opção `maxLoopUnrollingIterationCount`.
    pub fn max_loop_unrolling_iteration_count() -> u32 {
        Options::with(|options| options.max_loop_unrolling_iteration_count)
    }

    /// `Options::maxLoopUnrollingIterationCount() = value`.
    pub fn set_max_loop_unrolling_iteration_count(value: u32) {
        Options::with_mut(|options| options.max_loop_unrolling_iteration_count = value);
    }

    /// Opção `maxPartialLoopUnrollingBodyNodeSize`.
    pub fn max_partial_loop_unrolling_body_node_size() -> u32 {
        Options::with(|options| options.max_partial_loop_unrolling_body_node_size)
    }

    /// `Options::maxPartialLoopUnrollingBodyNodeSize() = value`.
    pub fn set_max_partial_loop_unrolling_body_node_size(value: u32) {
        Options::with_mut(|options| options.max_partial_loop_unrolling_body_node_size = value);
    }

    /// Opção `maxPartialLoopUnrollingIterationCount`.
    pub fn max_partial_loop_unrolling_iteration_count() -> u32 {
        Options::with(|options| options.max_partial_loop_unrolling_iteration_count)
    }

    /// `Options::maxPartialLoopUnrollingIterationCount() = value`.
    pub fn set_max_partial_loop_unrolling_iteration_count(value: u32) {
        Options::with_mut(|options| options.max_partial_loop_unrolling_iteration_count = value);
    }

    /// Opção `maxNumericHotLoopSize`.
    pub fn max_numeric_hot_loop_size() -> u32 {
        Options::with(|options| options.max_numeric_hot_loop_size)
    }

    /// `Options::maxNumericHotLoopSize() = value`.
    pub fn set_max_numeric_hot_loop_size(value: u32) {
        Options::with_mut(|options| options.max_numeric_hot_loop_size = value);
    }

    /// Opção `maxIntegerRangeOptimizationRelationshipsPerNode`.
    pub fn max_integer_range_optimization_relationships_per_node() -> u32 {
        Options::with(|options| options.max_integer_range_optimization_relationships_per_node)
    }

    /// `Options::maxIntegerRangeOptimizationRelationshipsPerNode() = value`.
    pub fn set_max_integer_range_optimization_relationships_per_node(value: u32) {
        Options::with_mut(|options| options.max_integer_range_optimization_relationships_per_node = value);
    }

    /// Opção `maxIntegerRangeOptimizationWork`.
    pub fn max_integer_range_optimization_work() -> u32 {
        Options::with(|options| options.max_integer_range_optimization_work)
    }

    /// `Options::maxIntegerRangeOptimizationWork() = value`.
    pub fn set_max_integer_range_optimization_work(value: u32) {
        Options::with_mut(|options| options.max_integer_range_optimization_work = value);
    }

    /// Opção `printEachUnrolledLoop`.
    pub fn print_each_unrolled_loop() -> bool {
        Options::with(|options| options.print_each_unrolled_loop)
    }

    /// `Options::printEachUnrolledLoop() = value`.
    pub fn set_print_each_unrolled_loop(value: bool) {
        Options::with_mut(|options| options.print_each_unrolled_loop = value);
    }

    /// Opção `verboseExecutablePoolAllocation`.
    pub fn verbose_executable_pool_allocation() -> bool {
        Options::with(|options| options.verbose_executable_pool_allocation)
    }

    /// `Options::verboseExecutablePoolAllocation() = value`.
    pub fn set_verbose_executable_pool_allocation(value: bool) {
        Options::with_mut(|options| options.verbose_executable_pool_allocation = value);
    }

    /// Opção `useHandlerICInFTL`.
    pub fn use_handler_ic_in_ftl() -> bool {
        Options::with(|options| options.use_handler_ic_in_ftl)
    }

    /// `Options::useHandlerICInFTL() = value`.
    pub fn set_use_handler_ic_in_ftl(value: bool) {
        Options::with_mut(|options| options.use_handler_ic_in_ftl = value);
    }

    /// Opção `useLLIntICs`.
    pub fn use_ll_int_i_cs() -> bool {
        Options::with(|options| options.use_ll_int_i_cs)
    }

    /// `Options::useLLIntICs() = value`.
    pub fn set_use_ll_int_i_cs(value: bool) {
        Options::with_mut(|options| options.use_ll_int_i_cs = value);
    }

    /// Opção `useBaselineJITCodeSharing`.
    pub fn use_baseline_jit_code_sharing() -> bool {
        Options::with(|options| options.use_baseline_jit_code_sharing)
    }

    /// `Options::useBaselineJITCodeSharing() = value`.
    pub fn set_use_baseline_jit_code_sharing(value: bool) {
        Options::with_mut(|options| options.use_baseline_jit_code_sharing = value);
    }

    /// Opção `libpasScavengeContinuously`.
    pub fn libpas_scavenge_continuously() -> bool {
        Options::with(|options| options.libpas_scavenge_continuously)
    }

    /// `Options::libpasScavengeContinuously() = value`.
    pub fn set_libpas_scavenge_continuously(value: bool) {
        Options::with_mut(|options| options.libpas_scavenge_continuously = value);
    }

    /// Opção `libpasForcePGMWithRate`.
    pub fn libpas_force_pgm_with_rate() -> u32 {
        Options::with(|options| options.libpas_force_pgm_with_rate)
    }

    /// `Options::libpasForcePGMWithRate() = value`.
    pub fn set_libpas_force_pgm_with_rate(value: u32) {
        Options::with_mut(|options| options.libpas_force_pgm_with_rate = value);
    }

    /// Opção `useWasmFaultSignalHandler`.
    pub fn use_wasm_fault_signal_handler() -> bool {
        Options::with(|options| options.use_wasm_fault_signal_handler)
    }

    /// `Options::useWasmFaultSignalHandler() = value`.
    pub fn set_use_wasm_fault_signal_handler(value: bool) {
        Options::with_mut(|options| options.use_wasm_fault_signal_handler = value);
    }

    /// Opção `dumpUnlinkedDFGValidation`.
    pub fn dump_unlinked_dfg_validation() -> bool {
        Options::with(|options| options.dump_unlinked_dfg_validation)
    }

    /// `Options::dumpUnlinkedDFGValidation() = value`.
    pub fn set_dump_unlinked_dfg_validation(value: bool) {
        Options::with_mut(|options| options.dump_unlinked_dfg_validation = value);
    }

    /// Opção `dumpWasmOpcodeStatistics`.
    pub fn dump_wasm_opcode_statistics() -> bool {
        Options::with(|options| options.dump_wasm_opcode_statistics)
    }

    /// `Options::dumpWasmOpcodeStatistics() = value`.
    pub fn set_dump_wasm_opcode_statistics(value: bool) {
        Options::with_mut(|options| options.dump_wasm_opcode_statistics = value);
    }

    /// Opção `dumpWasmWarnings`.
    pub fn dump_wasm_warnings() -> bool {
        Options::with(|options| options.dump_wasm_warnings)
    }

    /// `Options::dumpWasmWarnings() = value`.
    pub fn set_dump_wasm_warnings(value: bool) {
        Options::with_mut(|options| options.dump_wasm_warnings = value);
    }

    /// Opção `useRecursiveJSONParse`.
    pub fn use_recursive_json_parse() -> bool {
        Options::with(|options| options.use_recursive_json_parse)
    }

    /// `Options::useRecursiveJSONParse() = value`.
    pub fn set_use_recursive_json_parse(value: bool) {
        Options::with_mut(|options| options.use_recursive_json_parse = value);
    }

    /// Opção `thresholdForStringReplaceCache`.
    pub fn threshold_for_string_replace_cache() -> u32 {
        Options::with(|options| options.threshold_for_string_replace_cache)
    }

    /// `Options::thresholdForStringReplaceCache() = value`.
    pub fn set_threshold_for_string_replace_cache(value: u32) {
        Options::with_mut(|options| options.threshold_for_string_replace_cache = value);
    }

    /// Opção `useWasmIPInt`.
    pub fn use_wasm_ip_int() -> bool {
        Options::with(|options| options.use_wasm_ip_int)
    }

    /// `Options::useWasmIPInt() = value`.
    pub fn set_use_wasm_ip_int(value: bool) {
        Options::with_mut(|options| options.use_wasm_ip_int = value);
    }

    /// Opção `useWasmIPIntPrologueOSR`.
    pub fn use_wasm_ip_int_prologue_osr() -> bool {
        Options::with(|options| options.use_wasm_ip_int_prologue_osr)
    }

    /// `Options::useWasmIPIntPrologueOSR() = value`.
    pub fn set_use_wasm_ip_int_prologue_osr(value: bool) {
        Options::with_mut(|options| options.use_wasm_ip_int_prologue_osr = value);
    }

    /// Opção `useWasmIPIntLoopOSR`.
    pub fn use_wasm_ip_int_loop_osr() -> bool {
        Options::with(|options| options.use_wasm_ip_int_loop_osr)
    }

    /// `Options::useWasmIPIntLoopOSR() = value`.
    pub fn set_use_wasm_ip_int_loop_osr(value: bool) {
        Options::with_mut(|options| options.use_wasm_ip_int_loop_osr = value);
    }

    /// Opção `useWasmIPIntEpilogueOSR`.
    pub fn use_wasm_ip_int_epilogue_osr() -> bool {
        Options::with(|options| options.use_wasm_ip_int_epilogue_osr)
    }

    /// `Options::useWasmIPIntEpilogueOSR() = value`.
    pub fn set_use_wasm_ip_int_epilogue_osr(value: bool) {
        Options::with_mut(|options| options.use_wasm_ip_int_epilogue_osr = value);
    }

    /// Opção `useWasmIPIntSIMD`.
    pub fn use_wasm_ip_int_simd() -> bool {
        Options::with(|options| options.use_wasm_ip_int_simd)
    }

    /// `Options::useWasmIPIntSIMD() = value`.
    pub fn set_use_wasm_ip_int_simd(value: bool) {
        Options::with_mut(|options| options.use_wasm_ip_int_simd = value);
    }

    /// Opção `traceWasmIPIntExecution`.
    pub fn trace_wasm_ip_int_execution() -> bool {
        Options::with(|options| options.trace_wasm_ip_int_execution)
    }

    /// `Options::traceWasmIPIntExecution() = value`.
    pub fn set_trace_wasm_ip_int_execution(value: bool) {
        Options::with_mut(|options| options.trace_wasm_ip_int_execution = value);
    }

    /// Opção `forceAllFunctionsToUseSIMD`.
    pub fn force_all_functions_to_use_simd() -> bool {
        Options::with(|options| options.force_all_functions_to_use_simd)
    }

    /// `Options::forceAllFunctionsToUseSIMD() = value`.
    pub fn set_force_all_functions_to_use_simd(value: bool) {
        Options::with_mut(|options| options.force_all_functions_to_use_simd = value);
    }

    /// Opção `useOMGInlining`.
    pub fn use_omg_inlining() -> bool {
        Options::with(|options| options.use_omg_inlining)
    }

    /// `Options::useOMGInlining() = value`.
    pub fn set_use_omg_inlining(value: bool) {
        Options::with_mut(|options| options.use_omg_inlining = value);
    }

    /// Opção `freeRetiredWasmCode`.
    pub fn free_retired_wasm_code() -> bool {
        Options::with(|options| options.free_retired_wasm_code)
    }

    /// `Options::freeRetiredWasmCode() = value`.
    pub fn set_free_retired_wasm_code(value: bool) {
        Options::with_mut(|options| options.free_retired_wasm_code = value);
    }

    /// Opção `useArrayAllocationSinking`.
    pub fn use_array_allocation_sinking() -> bool {
        Options::with(|options| options.use_array_allocation_sinking)
    }

    /// `Options::useArrayAllocationSinking() = value`.
    pub fn set_use_array_allocation_sinking(value: bool) {
        Options::with_mut(|options| options.use_array_allocation_sinking = value);
    }

    /// Opção `dumpFTLCodeSize`.
    pub fn dump_ftl_code_size() -> bool {
        Options::with(|options| options.dump_ftl_code_size)
    }

    /// `Options::dumpFTLCodeSize() = value`.
    pub fn set_dump_ftl_code_size(value: bool) {
        Options::with_mut(|options| options.dump_ftl_code_size = value);
    }

    /// Opção `dumpOptimizationTracing`.
    pub fn dump_optimization_tracing() -> bool {
        Options::with(|options| options.dump_optimization_tracing)
    }

    /// `Options::dumpOptimizationTracing() = value`.
    pub fn set_dump_optimization_tracing(value: bool) {
        Options::with_mut(|options| options.dump_optimization_tracing = value);
    }

    /// Opção `dumpIonGraph`.
    pub fn dump_ion_graph() -> bool {
        Options::with(|options| options.dump_ion_graph)
    }

    /// `Options::dumpIonGraph() = value`.
    pub fn set_dump_ion_graph(value: bool) {
        Options::with_mut(|options| options.dump_ion_graph = value);
    }

    /// Opção `ionGraphDirectory`.
    pub fn ion_graph_directory() -> Option<String> {
        Options::with(|options| options.ion_graph_directory.clone())
    }

    /// `Options::ionGraphDirectory() = value`.
    pub fn set_ion_graph_directory(value: Option<String>) {
        Options::with_mut(|options| options.ion_graph_directory = value);
    }

    /// Opção `markedBlockDumpInfoCount`.
    pub fn marked_block_dump_info_count() -> u32 {
        Options::with(|options| options.marked_block_dump_info_count)
    }

    /// `Options::markedBlockDumpInfoCount() = value`.
    pub fn set_marked_block_dump_info_count(value: u32) {
        Options::with_mut(|options| options.marked_block_dump_info_count = value);
    }

    /// Opção `useAsyncStackTrace`.
    pub fn use_async_stack_trace() -> bool {
        Options::with(|options| options.use_async_stack_trace)
    }

    /// `Options::useAsyncStackTrace() = value`.
    pub fn set_use_async_stack_trace(value: bool) {
        Options::with_mut(|options| options.use_async_stack_trace = value);
    }

    /// Opção `useBigIntMathMethods`.
    pub fn use_big_int_math_methods() -> bool {
        Options::with(|options| options.use_big_int_math_methods)
    }

    /// `Options::useBigIntMathMethods() = value`.
    pub fn set_use_big_int_math_methods(value: bool) {
        Options::with_mut(|options| options.use_big_int_math_methods = value);
    }

    /// Opção `useExplicitResourceManagement`.
    pub fn use_explicit_resource_management() -> bool {
        Options::with(|options| options.use_explicit_resource_management)
    }

    /// `Options::useExplicitResourceManagement() = value`.
    pub fn set_use_explicit_resource_management(value: bool) {
        Options::with_mut(|options| options.use_explicit_resource_management = value);
    }

    /// Opção `useImportDefer`.
    pub fn use_import_defer() -> bool {
        Options::with(|options| options.use_import_defer)
    }

    /// `Options::useImportDefer() = value`.
    pub fn set_use_import_defer(value: bool) {
        Options::with_mut(|options| options.use_import_defer = value);
    }

    /// Opção `useImportText`.
    pub fn use_import_text() -> bool {
        Options::with(|options| options.use_import_text)
    }

    /// `Options::useImportText() = value`.
    pub fn set_use_import_text(value: bool) {
        Options::with_mut(|options| options.use_import_text = value);
    }

    /// Opção `useIteratorChunking`.
    pub fn use_iterator_chunking() -> bool {
        Options::with(|options| options.use_iterator_chunking)
    }

    /// `Options::useIteratorChunking() = value`.
    pub fn set_use_iterator_chunking(value: bool) {
        Options::with_mut(|options| options.use_iterator_chunking = value);
    }

    /// Opção `useIteratorIncludes`.
    pub fn use_iterator_includes() -> bool {
        Options::with(|options| options.use_iterator_includes)
    }

    /// `Options::useIteratorIncludes() = value`.
    pub fn set_use_iterator_includes(value: bool) {
        Options::with_mut(|options| options.use_iterator_includes = value);
    }

    /// Opção `useIteratorJoin`.
    pub fn use_iterator_join() -> bool {
        Options::with(|options| options.use_iterator_join)
    }

    /// `Options::useIteratorJoin() = value`.
    pub fn set_use_iterator_join(value: bool) {
        Options::with_mut(|options| options.use_iterator_join = value);
    }

    /// Opção `useIteratorSequencing`.
    pub fn use_iterator_sequencing() -> bool {
        Options::with(|options| options.use_iterator_sequencing)
    }

    /// `Options::useIteratorSequencing() = value`.
    pub fn set_use_iterator_sequencing(value: bool) {
        Options::with_mut(|options| options.use_iterator_sequencing = value);
    }

    /// Opção `useJSONSourceTextAccess`.
    pub fn use_json_source_text_access() -> bool {
        Options::with(|options| options.use_json_source_text_access)
    }

    /// `Options::useJSONSourceTextAccess() = value`.
    pub fn set_use_json_source_text_access(value: bool) {
        Options::with_mut(|options| options.use_json_source_text_access = value);
    }

    /// Opção `useJSPI`.
    pub fn use_jspi() -> bool {
        Options::with(|options| options.use_jspi)
    }

    /// `Options::useJSPI() = value`.
    pub fn set_use_jspi(value: bool) {
        Options::with_mut(|options| options.use_jspi = value);
    }

    /// Opção `useJointIteration`.
    pub fn use_joint_iteration() -> bool {
        Options::with(|options| options.use_joint_iteration)
    }

    /// `Options::useJointIteration() = value`.
    pub fn set_use_joint_iteration(value: bool) {
        Options::with_mut(|options| options.use_joint_iteration = value);
    }

    /// Opção `useMoreCurrencyDisplayChoices`.
    pub fn use_more_currency_display_choices() -> bool {
        Options::with(|options| options.use_more_currency_display_choices)
    }

    /// `Options::useMoreCurrencyDisplayChoices() = value`.
    pub fn set_use_more_currency_display_choices(value: bool) {
        Options::with_mut(|options| options.use_more_currency_display_choices = value);
    }

    /// Opção `usePromiseIsPromise`.
    pub fn use_promise_is_promise() -> bool {
        Options::with(|options| options.use_promise_is_promise)
    }

    /// `Options::usePromiseIsPromise() = value`.
    pub fn set_use_promise_is_promise(value: bool) {
        Options::with_mut(|options| options.use_promise_is_promise = value);
    }

    /// Opção `useRegExpBufferBoundaries`.
    pub fn use_reg_exp_buffer_boundaries() -> bool {
        Options::with(|options| options.use_reg_exp_buffer_boundaries)
    }

    /// `Options::useRegExpBufferBoundaries() = value`.
    pub fn set_use_reg_exp_buffer_boundaries(value: bool) {
        Options::with_mut(|options| options.use_reg_exp_buffer_boundaries = value);
    }

    /// Opção `useShadowRealm`.
    pub fn use_shadow_realm() -> bool {
        Options::with(|options| options.use_shadow_realm)
    }

    /// `Options::useShadowRealm() = value`.
    pub fn set_use_shadow_realm(value: bool) {
        Options::with_mut(|options| options.use_shadow_realm = value);
    }

    /// Opção `useTemporal`.
    pub fn use_temporal() -> bool {
        Options::with(|options| options.use_temporal)
    }

    /// `Options::useTemporal() = value`.
    pub fn set_use_temporal(value: bool) {
        Options::with_mut(|options| options.use_temporal = value);
    }

    /// Opção `useWasmJSStringBuiltins`.
    pub fn use_wasm_js_string_builtins() -> bool {
        Options::with(|options| options.use_wasm_js_string_builtins)
    }

    /// `Options::useWasmJSStringBuiltins() = value`.
    pub fn set_use_wasm_js_string_builtins(value: bool) {
        Options::with_mut(|options| options.use_wasm_js_string_builtins = value);
    }

    /// Opção `useWasmJSTypes`.
    pub fn use_wasm_js_types() -> bool {
        Options::with(|options| options.use_wasm_js_types)
    }

    /// `Options::useWasmJSTypes() = value`.
    pub fn set_use_wasm_js_types(value: bool) {
        Options::with_mut(|options| options.use_wasm_js_types = value);
    }

    /// Opção `useWasmMemory64`.
    pub fn use_wasm_memory64() -> bool {
        Options::with(|options| options.use_wasm_memory64)
    }

    /// `Options::useWasmMemory64() = value`.
    pub fn set_use_wasm_memory64(value: bool) {
        Options::with_mut(|options| options.use_wasm_memory64 = value);
    }

    /// Opção `useWasmMemoryToBufferAPIs`.
    pub fn use_wasm_memory_to_buffer_ap_is() -> bool {
        Options::with(|options| options.use_wasm_memory_to_buffer_ap_is)
    }

    /// `Options::useWasmMemoryToBufferAPIs() = value`.
    pub fn set_use_wasm_memory_to_buffer_ap_is(value: bool) {
        Options::with_mut(|options| options.use_wasm_memory_to_buffer_ap_is = value);
    }

    /// Opção `useWasmMultiMemory`.
    pub fn use_wasm_multi_memory() -> bool {
        Options::with(|options| options.use_wasm_multi_memory)
    }

    /// `Options::useWasmMultiMemory() = value`.
    pub fn set_use_wasm_multi_memory(value: bool) {
        Options::with_mut(|options| options.use_wasm_multi_memory = value);
    }

    /// Opção `useWasmRelaxedSIMD`.
    pub fn use_wasm_relaxed_simd() -> bool {
        Options::with(|options| options.use_wasm_relaxed_simd)
    }

    /// `Options::useWasmRelaxedSIMD() = value`.
    pub fn set_use_wasm_relaxed_simd(value: bool) {
        Options::with_mut(|options| options.use_wasm_relaxed_simd = value);
    }

    /// Opção `useWasmSIMD`.
    pub fn use_wasm_simd() -> bool {
        Options::with(|options| options.use_wasm_simd)
    }

    /// `Options::useWasmSIMD() = value`.
    pub fn set_use_wasm_simd(value: bool) {
        Options::with_mut(|options| options.use_wasm_simd = value);
    }

    /// Opção `useWasmTailCalls`.
    pub fn use_wasm_tail_calls() -> bool {
        Options::with(|options| options.use_wasm_tail_calls)
    }

    /// `Options::useWasmTailCalls() = value`.
    pub fn set_use_wasm_tail_calls(value: bool) {
        Options::with_mut(|options| options.use_wasm_tail_calls = value);
    }

    /// Opção `useWasmWideArithmetic`.
    pub fn use_wasm_wide_arithmetic() -> bool {
        Options::with(|options| options.use_wasm_wide_arithmetic)
    }

    /// `Options::useWasmWideArithmetic() = value`.
    pub fn set_use_wasm_wide_arithmetic(value: bool) {
        Options::with_mut(|options| options.use_wasm_wide_arithmetic = value);
    }

    /// Opção `disallowMixedWasmExceptions`.
    pub fn disallow_mixed_wasm_exceptions() -> bool {
        Options::with(|options| options.disallow_mixed_wasm_exceptions)
    }

    /// `Options::disallowMixedWasmExceptions() = value`.
    pub fn set_disallow_mixed_wasm_exceptions(value: bool) {
        Options::with_mut(|options| options.disallow_mixed_wasm_exceptions = value);
    }

    /// Opção `useSharedArrayBuffer`.
    pub fn use_shared_array_buffer() -> bool {
        Options::with(|options| options.use_shared_array_buffer)
    }

    /// `Options::useSharedArrayBuffer() = value`.
    pub fn set_use_shared_array_buffer(value: bool) {
        Options::with_mut(|options| options.use_shared_array_buffer = value);
    }

    /// Opção `useTrustedTypes`.
    pub fn use_trusted_types() -> bool {
        Options::with(|options| options.use_trusted_types)
    }

    /// `Options::useTrustedTypes() = value`.
    pub fn set_use_trusted_types(value: bool) {
        Options::with_mut(|options| options.use_trusted_types = value);
    }
}

/// Nome original, tipo, disponibilidade e descrição de cada opção, na ordem de `Options`.
pub static OPTIONS_TABLE: [OptionInfo; NUMBER_OF_OPTIONS] = [
    OptionInfo { name: "useKernTCSM", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Note: this needs to go before other options since they depend on this value.") },
    OptionInfo { name: "validateOptions", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("crashes if mis-typed JSC options were passed to the VM") },
    OptionInfo { name: "dumpOptions", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("dumps JSC options (0 = None, 1 = Overridden only, 2 = All, 3 = Verbose)") },
    OptionInfo { name: "configFile", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("file to configure JSC options and logging location") },
    OptionInfo { name: "useLLInt", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows the LLINT to be used if true") },
    OptionInfo { name: "useJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows the executable pages to be allocated for JIT and thunks if true") },
    OptionInfo { name: "useBaselineJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows the baseline JIT to be used if true") },
    OptionInfo { name: "useDFGJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows the DFG JIT to be used if true") },
    OptionInfo { name: "useRegExpJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows the RegExp JIT to be used if true") },
    OptionInfo { name: "useDOMJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows the DOMJIT to be used if true") },
    OptionInfo { name: "useRegExpLookbehindJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows patterns containing lookbehind assertions to use the RegExp JIT") },
    OptionInfo { name: "useRegExpAlternationFactoring", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("factors shared prefixes out of wide alternations and folds wide top-level alternations into a group") },
    OptionInfo { name: "useRegExpAlternationDispatch", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("lets the RegExp JIT dispatch a group's alternatives on their first character and compare short literal alternatives inline") },
    OptionInfo { name: "regExpDispatchMaxInlineLiteralLength", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("longest literal alternative (up to the JIT's ceiling of 32) the RegExp JIT compares inline inside a first-character dispatch chain; 0 disables inline literals") },
    OptionInfo { name: "reportMustSucceedExecutableAllocations", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useV8DateParser", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "showPrivateScriptsInStackTraces", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Show private scripts in stack traces.") },
    OptionInfo { name: "evalMode", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Set to true for less aggressive function call completion value discarding.") },
    OptionInfo { name: "useFFIICStub", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("install per-function FFI IC stubs") },
    OptionInfo { name: "useFFICallInDFG", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allow Call -> CallFFI in DFG/FTL") },
    OptionInfo { name: "useFFIDirectCall", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("FTL calls the native FFI target directly (no invoke thunk)") },
    OptionInfo { name: "dumpFFIDisassembly", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("disassemble generated FFI thunks/stubs") },
    OptionInfo { name: "verboseFFI", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dataLog on FFI thunk/stub/signature creation") },
    OptionInfo { name: "maxPerThreadStackUsage", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Max allowed stack usage by the VM") },
    OptionInfo { name: "softReservedZoneSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("A buffer greater than reservedZoneSize that reserves space for stringifying exceptions.") },
    OptionInfo { name: "reservedZoneSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("The amount of stack space we guarantee to our clients (and to interal VM code that does not call out to clients).") },
    OptionInfo { name: "crashOnDisallowedVMEntry", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Forces a crash if we attempt to enter the VM when disallowed") },
    OptionInfo { name: "crashIfCantAllocateJITMemory", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "structureHeapSizeInKB", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Override for Structure Heap size (in KBs) if non-zero") },
    OptionInfo { name: "jitMemoryReservationSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Set this number to change the executable allocation size in ExecutableAllocatorFixedVMPool. (In bytes.)") },
    OptionInfo { name: "jitMemoryReservationAddress", option_type: OptionType::Size, availability: Availability::Restricted, description: Some("If non-zero, we will attempt to allocate JIT memory at the address provided and crash if we cannot.") },
    OptionInfo { name: "forceCodeBlockLiveness", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "forceICFailure", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "forceUnlinkedDFG", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "repatchCountForCoolDown", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "initialCoolDownCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "repatchBufferingCountdown", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "initialRepatchBufferingCountdown", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpGeneratedBytecodes", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpBytecodeLivenessResults", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "validateBytecode", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "forceDebuggerBytecodeGeneration", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "debuggerTriggersBreakpointException", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Using the debugger statement will trigger an breakpoint exception (Useful when lldbing)") },
    OptionInfo { name: "verboseWasmDebugger", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "enableWasmDebugger", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseWasmTypeCleanup", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Log per-invocation counts from Wasm::TypeInformation::tryCleanup (scanned / live / reclaimed).") },
    OptionInfo { name: "dumpBytecodesBeforeGeneratorification", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "switchJumpTableAmountThreshold", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "useFunctionDotArguments", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useTailCalls", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "optimizeRecursiveTailCalls", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "alwaysUseShadowChicken", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "shadowChickenLogSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "shadowChickenMaxTailDeletedFramesSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "useOSLog", option_type: OptionType::OSLogType, availability: Availability::Normal, description: Some("Log dataLog()s to os_log instead of stderr") },
    OptionInfo { name: "needDisassemblySupport", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpDisassembly", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps disassembly of all JIT compiled code upon compilation") },
    OptionInfo { name: "logJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpBaselineDisassembly", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps disassembly of Baseline function upon compilation") },
    OptionInfo { name: "dumpDFGDisassembly", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps disassembly of DFG function upon compilation") },
    OptionInfo { name: "dumpFTLDisassembly", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps disassembly of FTL function upon compilation") },
    OptionInfo { name: "dumpCSSJITDisassembly", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps disassembly of CSS Selector JIT upon compilation") },
    OptionInfo { name: "dumpRegExpDisassembly", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps disassembly of RegExp upon compilation") },
    OptionInfo { name: "traceRegExpJITExecution", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("traces RegExp JIT execution at reentry points") },
    OptionInfo { name: "verifyRegExpJITReads", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("checks, before every load the RegExp JIT makes from the subject string, that the address lies within the subject (crashes otherwise); a fuzzing aid") },
    OptionInfo { name: "dumpWasmDisassembly", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps disassembly of all wasm code upon compilation") },
    OptionInfo { name: "dumpWasmSourceFileName", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("log every wasm module validation, and dump source bytes to <filename>.0.wasm, <filename>.1.wasm, etc...") },
    OptionInfo { name: "wasmOMGFunctionsToDump", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("file with newline separated list of function indices to dump IR/disassembly for, if no such file exists, the function index itself") },
    OptionInfo { name: "dumpBBQDisassembly", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps disassembly of BBQ wasm code upon compilation") },
    OptionInfo { name: "dumpOMGDisassembly", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps disassembly of OMG wasm code upon compilation") },
    OptionInfo { name: "useJITDump", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("generates JITDump side-data") },
    OptionInfo { name: "useGdbJITInfo", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("generates GDB JIT API side-data; to use with lldb on macos, add `settings set plugin.jit-loader.gdb.enable on` to .lldbinit") },
    OptionInfo { name: "useTextMarkers", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("generates text markers side-data") },
    OptionInfo { name: "jitDumpDirectory", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("Directory to place JITDump") },
    OptionInfo { name: "useIRDump", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("generates IR dump files and JIT_CODE_DEBUG_INFO in JITDump") },
    OptionInfo { name: "irDumpDirectory", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("Directory to place IR dump files") },
    OptionInfo { name: "useSourceCodeDump", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("generates source code debug info in JITDump") },
    OptionInfo { name: "sourceCodeDumpDirectory", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("Directory to place dumped source files") },
    OptionInfo { name: "textMarkersDirectory", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("Directory to place MarkerTxt") },
    OptionInfo { name: "bytecodeRangeToJITCompile", option_type: OptionType::OptionRange, availability: Availability::Normal, description: Some("bytecode size range to allow compilation on, e.g. 1:100") },
    OptionInfo { name: "bytecodeRangeToDFGCompile", option_type: OptionType::OptionRange, availability: Availability::Normal, description: Some("bytecode size range to allow DFG compilation on, e.g. 1:100") },
    OptionInfo { name: "bytecodeRangeToFTLCompile", option_type: OptionType::OptionRange, availability: Availability::Normal, description: Some("bytecode size range to allow FTL compilation on, e.g. 1:100") },
    OptionInfo { name: "jitAllowlist", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("file with newline separated list of function signatures to allow compilation on or, if no such file exists, the function signature to allow") },
    OptionInfo { name: "dfgAllowlist", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("file with newline separated list of function signatures to allow DFG compilation on or, if no such file exists, the function signature to allow") },
    OptionInfo { name: "ftlAllowlist", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("file with newline separated list of function signatures to allow FTL compilation on or, if no such file exists, the function signature to allow") },
    OptionInfo { name: "bbqAllowlist", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("file with newline separated list of function indices to allow BBQ compilation on or, if no such file exists, the function index to allow") },
    OptionInfo { name: "omgAllowlist", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("file with newline separated list of function indices to allow OMG compilation on or, if no such file exists, the function index to allow") },
    OptionInfo { name: "loopUnrollingAllowlist", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("file with newline separated list of function signatures to allow loop unrolling on or, if no such file exists, the function signature to allow") },
    OptionInfo { name: "dumpGraphAllowlist", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("file with newline separated list of function signatures to filter graph dumps without restricting JIT compilation, or if no such file exists, the function signature to allow (affects dumpGraphAtEachPhase, dumpDFGGraphAtEachPhase, and dumpDFGFTLGraphAtEachPhase)") },
    OptionInfo { name: "dumpSourceAtDFGTime", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps source code of JS function being DFG compiled") },
    OptionInfo { name: "dumpBytecodeAtDFGTime", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps bytecode of JS function being DFG compiled") },
    OptionInfo { name: "dumpGraphAfterParsing", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpGraphAtEachPhase", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpDFGGraphAtEachPhase", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps the DFG graph at each phase of DFG compilation (note this excludes DFG graphs during FTL compilation)") },
    OptionInfo { name: "dumpDFGFTLGraphAtEachPhase", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps the DFG graph at each phase of DFG compilation when compiling FTL code") },
    OptionInfo { name: "dumpB3GraphAtEachPhase", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps the B3 graph at each phase of compilation") },
    OptionInfo { name: "dumpAirGraphAtEachPhase", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps the Air graph at each phase of compilation") },
    OptionInfo { name: "verboseDFGBytecodeParsing", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "safepointBeforeEachPhase", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseCompilation", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseFTLCompilation", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "logCompilationChanges", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "printEachOSRExit", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "printEachDFGFTLInlineCall", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useJITAsserts", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "validateDoesGC", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "validateGraph", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "validateGraphAtEachPhase", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseValidationFailure", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseOSR", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseDFGOSRExit", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseFTLOSRExit", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseCallLink", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseCompilationQueue", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "reportCompileTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps JS function signature and the time it took to compile in all tiers") },
    OptionInfo { name: "reportBaselineCompileTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps JS function signature and the time it took to BaselineJIT compile") },
    OptionInfo { name: "reportDFGCompileTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps JS function signature and the time it took to DFG and FTL compile") },
    OptionInfo { name: "reportFTLCompileTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps JS function signature and the time it took to FTL compile") },
    OptionInfo { name: "reportTotalCompileTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "reportTotalPhaseTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("This prints phase times at the end of running script inside jsc.cpp") },
    OptionInfo { name: "reportParseTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps JS function signature and the time it took to parse") },
    OptionInfo { name: "reportBytecodeCompileTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps JS function signature and the time it took to bytecode compile") },
    OptionInfo { name: "reportBytecodeCacheDecodeTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("dumps the time it took to decode bytecode from the disk cache") },
    OptionInfo { name: "countParseTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("counts parse times") },
    OptionInfo { name: "verboseExitProfile", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseCFA", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseDFGFailure", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseFTLToJSThunk", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseFTLFailure", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "testTheFTL", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseSanitizeStack", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useGenerationalGC", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useConcurrentGC", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "collectContinuously", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "collectContinuouslyPeriodMS", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "forceFencedBarrier", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseVisitRace", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "optimizeParallelSlotVisitorsForStoppedMutator", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseHeapSnapshotLogging", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "largeHeapSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "mediumHeapSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "smallHeapSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "smallHeapRAMFraction", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "smallHeapGrowthFactor", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "mediumHeapRAMFraction", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "mediumHeapGrowthFactor", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "largeHeapGrowthFactor", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "miniVMHeapGrowthFactor", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "heapGrowthSteepnessFactor", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "heapGrowthMaxIncrease", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "minEdenToOldGenerationRatio", option_type: OptionType::Double, availability: Availability::Normal, description: Some("after an eden GC, schedule a full collection if remainingHeapSize / maxHeapSize falls below this; bounds the usable heap growth factor below at 1 / (1 - value)") },
    OptionInfo { name: "heapGrowthFunctionThresholdInMB", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "criticalGCMemoryThreshold", option_type: OptionType::Double, availability: Availability::Normal, description: Some("percent memory in use the GC considers critical.  The collector is much more aggressive above this threshold") },
    OptionInfo { name: "customFullGCCallbackBailThreshold", option_type: OptionType::Double, availability: Availability::Normal, description: Some("percent of memory paged out before we bail out of timer based Full GCs. -1.0 means use (maxHeapGrowthFactor - 1)") },
    OptionInfo { name: "minimumMutatorUtilization", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumMutatorUtilization", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "epsilonMutatorUtilization", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "concurrentGCMaxHeadroom", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "concurrentGCPeriodMS", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "useStochasticMutatorScheduler", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "minimumGCPauseMS", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "gcPauseScale", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "gcIncrementBytes", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "gcIncrementMaxBytes", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "gcIncrementScale", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "useWarmUpMarkedBlocks", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("hand MarkedBlock allocation pages that a helper thread already made resident") },
    OptionInfo { name: "warmUpMarkedBlockCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("how many MarkedBlocks the helper thread keeps ready with their pages already resident; 0 turns it off") },
    OptionInfo { name: "warmUpMarkedBlockStartAfterBlocks", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("how many MarkedBlocks the process allocates before the helper thread starts; a program that stops before that never creates it") },
    OptionInfo { name: "warmUpMarkedBlockIdleTimeout", option_type: OptionType::Double, availability: Availability::Normal, description: Some("seconds without a MarkedBlock request before the helper thread releases what it is holding and shuts down") },
    OptionInfo { name: "scribbleFreeCells", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "decommitUnusedMarkedBlockPages", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("after sweeping a MarkedBlock, return its interior OS pages that hold no live cell to the OS (only where OS pages are smaller than a MarkedBlock)") },
    OptionInfo { name: "decommitUnusedMarkedBlockPagesAfterEdenCollections", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("also do it for blocks swept after an eden collection (mostly young blocks that are refilled straight away)") },
    OptionInfo { name: "sizeClassProgression", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "preciseAllocationCutoff", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpSizeClasses", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "stealEmptyBlocksFromOtherAllocators", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "eagerlyUpdateTopCallFrame", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpZappedCellCrashData", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useOSREntryToDFG", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useOSREntryToFTL", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useFTLJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows the FTL JIT to be used if true") },
    OptionInfo { name: "validateFTLOSRExitLiveness", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "poisonDeadOSRExitVariables", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Put an unmapped cell-like pointer (poisonedDeadOSRExitValue) into dead OSR exit values rather than jsUndefined, so accidental reads of dead variables crash at the access site") },
    OptionInfo { name: "defaultB3OptLevel", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "b3AlwaysFailsBeforeCompile", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "b3AlwaysFailsBeforeLink", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "validateSerializedValue", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "ftlCrashes", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "clobberAllRegsInFTLICSlowPath", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useJITDebugAssertions", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useAccessInlining", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxAccessVariantListSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForUndesiredMegamorphicAccessVariantListSize", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "usePolyvariantDevirtualization", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "usePolymorphicAccessInlining", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxPolymorphicAccessInliningListSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "usePolymorphicCallInlining", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "usePolymorphicCallInliningForNonStubStatus", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxPolymorphicCallVariantListSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxPolymorphicCallVariantListSizeForTopTier", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxPolymorphicCallVariantListSizeForWasmToJS", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxPolymorphicCallVariantsForInlining", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "frequentCallThreshold", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "minimumCallToKnownRate", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "createPreHeaders", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useMovHintRemoval", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "usePutStackSinking", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useObjectAllocationSinking", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseObjectAllocationSinking", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useValueRepElimination", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useArityFixupInlining", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "logExecutableAllocation", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxDFGNodesInBasicBlockForPreciseAnalysis", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Disable precise but costly analysis and give conservative results if the number of DFG nodes in a block exceeds this threshold") },
    OptionInfo { name: "useConcurrentJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows the DFG / FTL compilation in threads other than the executing JS thread") },
    OptionInfo { name: "minNumberOfWorklistThreads", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxNumberOfWorklistThreads", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "numberOfBaselineCompilerThreads", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "numberOfDFGCompilerThreads", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "numberOfFTLCompilerThreads", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "numberOfWasmCompilerThreads", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "worklistLoadFactor", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "worklistBaselineLoadWeight", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "worklistDFGLoadWeight", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "worklistFTLLoadWeight", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "priorityDeltaOfDFGCompilerThreads", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "priorityDeltaOfFTLCompilerThreads", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "priorityDeltaOfWasmCompilerThreads", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "useProfiler", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpProfilerDataAtExit", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "disassembleBaselineForProfiler", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "abbreviateSourceCodeForProfiler", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "useArchitectureSpecificOptimizations", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "breakOnThrow", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumOptimizationCandidateBytecodeCost", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumCachedAssemblerBufferSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Assembler scratch buffers larger than this are freed after compilation instead of being cached per thread (0 = cache any size)") },
    OptionInfo { name: "maximumFunctionForCallInlineCandidateBytecodeCostForDFG", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumFunctionForClosureCallInlineCandidateBytecodeCostForDFG", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumFunctionForConstructInlineCandidateBytecodeCostForDFG", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumFunctionForCallInlineCandidateBytecodeCostForFTL", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumFunctionForClosureCallInlineCandidateBytecodeCostForFTL", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumFunctionForConstructInlineCandidateBytecodeCostForFTL", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumFTLCandidateBytecodeCost", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "ratioFTLNodesToBytecodeCost", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Ratio converting FTL # of DFG nodes to approx bytecode cost") },
    OptionInfo { name: "maximumInliningDepth", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("maximum allowed inlining depth.  Depth of 1 means no inlining") },
    OptionInfo { name: "maximumInliningRecursion", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumInliningCallerBytecodeCost", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "useGlobalInliningPlanner", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Survey and rank every inlining candidate before parsing and spend one compilation-wide budget on the best of them, instead of deciding each call site in bytecode order") },
    OptionInfo { name: "globalInliningPlanBudgetForDFG", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Total callee bytecode cost the DFG may plan to inline in one compilation") },
    OptionInfo { name: "globalInliningPlanBudgetForFTL", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Total callee bytecode cost the FTL may plan to inline in one compilation") },
    OptionInfo { name: "maximumGlobalInliningPlanSites", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Cap on how many call sites one inlining plan will survey") },
    OptionInfo { name: "inliningPlanTierBonusBase", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Multiplicative benefit per tier the callee has reached (LLInt, Baseline, DFG, FTL) when ranking inlining candidates") },
    OptionInfo { name: "inliningPlanTierBonusPowerForFTL", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Base for the bonus multiplier for FTL callees") },
    OptionInfo { name: "inliningPlanTierBonusPowerForDFG", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Base for the bonus multiplier for DFG callees") },
    OptionInfo { name: "inliningPlanTierBonusPowerForBaseline", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Base for the bonus multiplier for Baseline callees") },
    OptionInfo { name: "inliningPlanDepthPenalty", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Divisive benefit penalty per level of inline-stack nesting when ranking inlining candidates") },
    OptionInfo { name: "maximumVarargsForInlining", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumBinaryStringSwitchCaseLength", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumBinaryStringSwitchTotalLength", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumInlineStringSwitchCaseCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Maximum number of cases for which the baseline JIT dispatches op_switch_string inline instead of calling out.") },
    OptionInfo { name: "maximumRegExpTestInlineCodesize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Maximum code size in bytes for inlined RegExp.test JIT code.") },
    OptionInfo { name: "maximumRegExpJITCodeSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Maximum generated code size in bytes for RegExp JIT compilation before falling back to the interpreter.") },
    OptionInfo { name: "wasmInliningMaximumDepth", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Maximum inlining depth to consider inlining a wasm function.") },
    OptionInfo { name: "wasmInliningMaximumWasmCalleeSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Maximum wasm size in bytes to consider inlining a wasm function.") },
    OptionInfo { name: "wasmInliningMaximumCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Maximum inlining count to consider inlining a wasm function.") },
    OptionInfo { name: "wasmInliningMinimumBudget", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Minimum budget for which the wasmInliningFactor does not apply") },
    OptionInfo { name: "wasmInliningFactor", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Maximum multiple budget in comparison to initial wasm size") },
    OptionInfo { name: "wasmInliningBudget", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Maximum budget that allows inlining more") },
    OptionInfo { name: "wasmInliningLargeFunctionGrowthFactor", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Minimum growth factor (multiplied by initial wasm size) that bounds the large-function inlining budget.") },
    OptionInfo { name: "wasmInliningTinyFunctionThreshold", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Wasm size threshold for tiny wasm functions") },
    OptionInfo { name: "wasmInliningSmallFunctionThreshold", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Wasm size threshold for small wasm functions") },
    OptionInfo { name: "jitPolicyScale", option_type: OptionType::Double, availability: Availability::Normal, description: Some("scale JIT thresholds to this specified ratio between 0.0 (compile ASAP) and 1.0 (compile like normal).") },
    OptionInfo { name: "numberOfSuperAndPerformanceCoresOverride", option_type: OptionType::Int32, availability: Availability::Normal, description: Some("If non-zero, overrides the number of Super and Performance (i.e. non-Efficiency) cores reported by the hardware; 0 means use the value reported by the hardware.") },
    OptionInfo { name: "dfgThresholdScaleForFewPerformanceCores", option_type: OptionType::Double, availability: Availability::Normal, description: Some("On Apple silicon Macs with few Super and Performance cores, scale the DFG tier-up thresholds (thresholdForOptimize*) by this factor.") },
    OptionInfo { name: "ftlThresholdScaleForFewPerformanceCores", option_type: OptionType::Double, availability: Availability::Normal, description: Some("On Apple silicon Macs with few Super and Performance cores, scale the FTL tier-up thresholds (thresholdForFTLOptimize*) by this factor.") },
    OptionInfo { name: "forceEagerCompilation", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForJITAfterWarmUp", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForJITSoon", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForOptimizeAfterWarmUp", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForOptimizeAfterLongWarmUp", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForOptimizeSoon", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "executionCounterIncrementForLoop", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "executionCounterIncrementForEntry", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForFTLOptimizeAfterWarmUp", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForFTLOptimizeSoon", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "ftlTierUpCounterIncrementForLoop", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "ftlTierUpCounterIncrementForReturn", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "ftlOSREntryFailureCountForReoptimization", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "ftlOSREntryRetryThreshold", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "evalThresholdMultiplier", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumEvalCacheableSourceLength", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumExecutionCountsBetweenCheckpointsForBaseline", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumExecutionCountsBetweenCheckpointsForUpperTiers", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "highCostBaselineProfilingFunctionBytecodeCost", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "valueProfileFillingRateMonitoringBytecodeCost", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "likelyToTakeSlowCaseMinimumCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "couldTakeSlowCaseMinimumCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "osrExitCountForReoptimization", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "osrExitCountForReoptimizationFromLoop", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "reoptimizationRetryCounterMax", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "minimumOptimizationDelay", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumOptimizationDelay", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "desiredProfileLivenessRate", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "desiredProfileFullnessRate", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "quickDFGTierUpThresholdFactor", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Threshold factor for quick DFG tier-up") },
    OptionInfo { name: "relaxedProfileCoverageFactorForQuickDFGTierUp", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Profile coverage scaling factor for quick DFG tier-up") },
    OptionInfo { name: "quickFTLTierUpThresholdFactor", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Threshold factor for quick FTL tier-up") },
    OptionInfo { name: "doubleVoteRatioForDoubleFormat", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "structureCheckVoteRatioForHoisting", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "checkArrayVoteRatioForHoisting", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumDirectCallStackSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "minimumNumberOfScansBetweenRebalance", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "numberOfGCMarkers", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "useParallelMarkingConstraintSolver", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "opaqueRootMergeThreshold", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxHeapSizeAsRAMSizeMultiple", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "minHeapUtilization", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "minMarkedBlockUtilization", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "slowPathAllocsBetweenGCs", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("force a GC on every Nth slow path alloc, where N is specified by this option") },
    OptionInfo { name: "maxRegExpStackSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "percentCPUPerMBForFullTimer", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "percentCPUPerMBForEdenTimer", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "collectionTimerMaxPercentCPU", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "forceWeakRandomSeed", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "forcedWeakRandomSeed", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "alwaysHaveABadTime", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("debugging option to test HaveABadTime mode") },
    OptionInfo { name: "allowDoubleShape", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("debugging option to test disabling use of DoubleShape") },
    OptionInfo { name: "useZombieMode", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("debugging option to scribble over dead objects with 0xbadbeef0") },
    OptionInfo { name: "useImmortalObjects", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("debugging option to keep all objects alive forever") },
    OptionInfo { name: "sweepSynchronously", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("debugging option to sweep all dead objects synchronously at GC end before resuming mutator") },
    OptionInfo { name: "maxSingleAllocationSize", option_type: OptionType::Unsigned, availability: Availability::Configurable, description: Some("debugging option to limit individual allocations to a max size (0 = limit not set, N = limit size in bytes)") },
    OptionInfo { name: "logGC", option_type: OptionType::GCLogLevel, availability: Availability::Normal, description: Some("debugging option to log GC activity (0 = None, 1 = Basic, 2 = Verbose)") },
    OptionInfo { name: "useGC", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useGlobalGC", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "gcAtEnd", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, the jsc CLI will do a GC before exiting") },
    OptionInfo { name: "forceGCSlowPaths", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, we will force all JIT fast allocations down their slow paths.") },
    OptionInfo { name: "forceDidDeferGCWork", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, we will force all DeferGC destructions to perform a GC.") },
    OptionInfo { name: "gcMaxHeapSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "forceRAMSize", option_type: OptionType::Size, availability: Availability::Normal, description: None },
    OptionInfo { name: "recordGCPauseTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpHeapStatisticsAtVMDestruction", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "enableStrongRefTracker", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable logging of live Strong<*> values. Use alongside $vm.triggerMemoryPressure() and dumpHeapOnLowMemory.") },
    OptionInfo { name: "dumpHeapOnLowMemory", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Dump a heap dump when the memory handler is triggered. Use alongside $vm.triggerMemoryPressure() and enableStrongRefTracker.") },
    OptionInfo { name: "forceCodeBlockToJettisonDueToOldAge", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, this means that anytime we can jettison a CodeBlock due to old age, we do.") },
    OptionInfo { name: "useEagerCodeBlockJettisonTiming", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, the time slices for jettisoning a CodeBlock due to old age are shrunk significantly.") },
    OptionInfo { name: "useExecutionCountForCodeBlockAging", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, an LLInt/Baseline CodeBlock whose execution counter has advanced since the last old-age check is treated as still in use and its TTL is renewed instead of being jettisoned.") },
    OptionInfo { name: "optimizedCodeAgingQuietAllocationMB", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("A collection that finds more than this much allocated since the last one that did marks the mutator as active for the aging of FTL code (and DFG code without a tier-up counter); an embedder-tagged idle collection lets such code go once nothing has been active for optimizedCodeAgingQuietSeconds. 0 = such code never ages out.") },
    OptionInfo { name: "optimizedCodeAgingQuietSeconds", option_type: OptionType::Double, availability: Availability::Normal, description: Some("How long since the last active collection (and since the code was installed) before an idle collection lets such code go (capped at the tier's TTL under useEagerCodeBlockJettisonTiming).") },
    OptionInfo { name: "codeBlockAgingLeaseMultiplier", option_type: OptionType::Double, availability: Availability::Normal, description: Some("When useExecutionCountForCodeBlockAging proves a CodeBlock is still active, renew its old-age TTL to this many multiples of timeToLive for its tier.") },
    OptionInfo { name: "useLeanBytecodeCacheDecoder", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, the bytecode cache Decoder skips bookkeeping that is only needed for decoded objects shared by multiple references.") },
    OptionInfo { name: "useBorrowedBytecodeFromCache", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, instruction streams and expression info decoded from a persistent (mmap'd/embedded) bytecode cache alias the cache instead of copying it.") },
    OptionInfo { name: "diskCachePayloadIsPersistentForTesting", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("jsc shell: keep files mapped from diskCachePath for the life of the process and mark them persistent, so useBorrowedBytecodeFromCache applies to them.") },
    OptionInfo { name: "verifyBytecodeCacheChecksums", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("check each code block's CRC when it is decoded from a bytecode cache and fall back to generating it from source on a mismatch") },
    OptionInfo { name: "useTypeProfiler", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useControlFlowProfiler", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useSamplingProfiler", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "sampleInterval", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Time between stack traces in microseconds.") },
    OptionInfo { name: "collectExtraSamplingProfilerData", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("This corresponds to the JSC shell's --sample option, or if we're wanting to use the sampling profiler via the Debug menu in the browser.") },
    OptionInfo { name: "samplingProfilerTopFunctionsCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Number of top functions to report when using the command line interface.") },
    OptionInfo { name: "samplingProfilerTopBytecodesCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Number of top bytecodes to report when using the command line interface.") },
    OptionInfo { name: "samplingProfilerIgnoreExternalSourceID", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Ignore external source ID when aggregating results from sampling profiler") },
    OptionInfo { name: "samplingProfilerPath", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("The path to the directory to write sampiling profiler output to. This probably will not work with WK2 unless the path is in the sandbox.") },
    OptionInfo { name: "sampleCCode", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Causes the sampling profiler to record profiling data for C frames.") },
    OptionInfo { name: "alwaysGeneratePCToCodeOriginMap", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("This will make sure we always generate a PCToCodeOriginMap for JITed code.") },
    OptionInfo { name: "randomIntegrityAuditRate", option_type: OptionType::Double, availability: Availability::Normal, description: Some("Probability of random integrity audits [0.0 - 1.0]") },
    OptionInfo { name: "verifyGC", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseVerifyGC", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verifyHeap", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "numberOfGCCyclesToRecordForVerification", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "exceptionStackTraceLimit", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Stack trace limit for internal Exception object") },
    OptionInfo { name: "defaultErrorStackTraceLimit", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("The default value for Error.stackTraceLimit") },
    OptionInfo { name: "exitOnResourceExhaustion", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useExceptionFuzz", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "fireExceptionFuzzAt", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "fuzzAtomicJITMemcpy", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "validateDFGExceptionHandling", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Causes the DFG to emit code validating exception handling for each node that can exit") },
    OptionInfo { name: "dumpSimulatedThrows", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Dumps the call stack of the last simulated throw if exception scope verification fails") },
    OptionInfo { name: "validateExceptionChecks", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Verifies that needed exception checks are performed.") },
    OptionInfo { name: "unexpectedExceptionStackTraceLimit", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Stack trace limit for debugging unexpected exceptions observed in the VM") },
    OptionInfo { name: "validateDFGClobberize", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Emits code in the DFG/FTL to validate the Clobberize phase") },
    OptionInfo { name: "validateBoundsCheckElimination", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Emits code in the DFG/FTL to validate bounds check elimination") },
    OptionInfo { name: "validateDFGMayExit", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Emits code in the DFG/FTL to validate the MayExit phase") },
    OptionInfo { name: "validateVMEntryCalleeSaves", option_type: OptionType::Bool, availability: Availability::Configurable, description: Some("Causes vmEntryToJavaScript to validate VMEntry callee saves are properly restored") },
    OptionInfo { name: "useExecutableAllocationFuzz", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "fireExecutableAllocationFuzzAt", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "fireExecutableAllocationFuzzAtOrAfter", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "fireExecutableAllocationFuzzRandomly", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "fireExecutableAllocationFuzzRandomlyProbability", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseExecutableAllocationFuzz", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "zeroExecutableMemoryOnFree", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("0 out instructions when freeing JIT memory.") },
    OptionInfo { name: "useOSRExitFuzz", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "fireOSRExitFuzzAtStatic", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "fireOSRExitFuzzAt", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "fireOSRExitFuzzAtOrAfter", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseOSRExitFuzz", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useLOLJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Use LOL instead of Baseline") },
    OptionInfo { name: "verboseLOLAllocation", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Log info about LOL's register allocation state") },
    OptionInfo { name: "seedOfVMRandomForFuzzer", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("0 means not fuzzing this; use a cryptographically random seed") },
    OptionInfo { name: "useRandomizingFuzzerAgent", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "seedOfRandomizingFuzzerAgent", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpFuzzerAgentPredictions", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useDoublePredictionFuzzerAgent", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useFileBasedFuzzerAgent", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "usePredictionFileCreatingFuzzerAgent", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "requirePredictionForFileBasedFuzzerAgent", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "fuzzerPredictionsFile", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("file with list of predictions for FileBasedFuzzerAgent") },
    OptionInfo { name: "useNarrowingNumberPredictionFuzzerAgent", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useWideningNumberPredictionFuzzerAgent", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "logPhaseTimes", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "rareBlockPenalty", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "airGreedyRegAllocVerbose", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "airGreedyRegAllocDumpFunction", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("dump greedy register allocator state and IR for functions matching this substring") },
    OptionInfo { name: "airGreedyRegAllocSplitMultiplier", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "airGreedyRegAllocSplitAroundLoops", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "airGreedyRegAllocLoopSplitMaxLoopFraction", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "airGreedyRegAllocSpillsEverything", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "airDumpPhaseStats", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "airValidateGreedRegAlloc", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "airRandomizeRegs", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "airRandomizeRegsSeed", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "coalesceSpillSlots", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "logAirRegisterPressure", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useB3TailDup", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxB3TailDupBlockSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxB3TailDupBlockSuccessors", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "useB3HoistLoopInvariantValues", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useB3CanonicalizePrePostIncrements", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useB3EliminateWasmGCAllocations", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("eliminate non-escaping wasm-GC struct allocations in B3") },
    OptionInfo { name: "useB3ReduceStrengthFixpoint", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("iterate B3 reduceStrength to a fixpoint instead of a single pass (for debugging)") },
    OptionInfo { name: "useAirOptimizePairedLoadStore", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useDollarVM", option_type: OptionType::Bool, availability: Availability::Restricted, description: Some("installs the $vm debugging tool in global objects") },
    OptionInfo { name: "functionOverrides", option_type: OptionType::OptionString, availability: Availability::Restricted, description: Some("file with debugging overrides for function bodies") },
    OptionInfo { name: "watchdog", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("watchdog timeout (0 = Disabled, N = a timeout period of N milliseconds)") },
    OptionInfo { name: "usePollingTraps", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("use polling (instead of signalling) VM traps") },
    OptionInfo { name: "forceTrapAwareStackChecks", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("force trap aware stack checks to be taken for testing") },
    OptionInfo { name: "useMachForExceptions", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Use mach exceptions rather than signals to handle faults and pass thread messages. (This does nothing on platforms without mach)") },
    OptionInfo { name: "allowNonSPTagging", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allow use of the pacib instruction instead of just pacibsp (This can break lldb/posix signals as it puts live data below SP)") },
    OptionInfo { name: "useICStats", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useFuzzerMode", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "prototypeHitCountForLLIntCaching", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Number of prototype property hits before caching a prototype in the LLInt. A count of 0 means never cache.") },
    OptionInfo { name: "dumpCompiledRegExpPatterns", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseRegExpCompilation", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpModuleRecord", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpModuleLoadingState", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "exposeInternalModuleLoader", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("expose the internal module loader object to the global space for debugging") },
    OptionInfo { name: "exposePrivateIdentifiers", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow non-builtin scripts to use private identifiers. Mostly useful to expose @superSamplerBegin/End intrinsics for profiling") },
    OptionInfo { name: "useSuperSampler", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useSourceProviderCache", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If false, the parser will not use the source provider cache. It's good to verify everything works when this is false. Because the cache is so successful, it can mask bugs.") },
    OptionInfo { name: "useCodeCache", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If false, the unlinked byte code cache will not be used.") },
    OptionInfo { name: "useWasm", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Expose the Wasm global object.") },
    OptionInfo { name: "failToCompileWasmCode", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, no Wasm::Plan will sucessfully compile a function.") },
    OptionInfo { name: "wasmSmallPartialCompileLimit", option_type: OptionType::Size, availability: Availability::Normal, description: Some("Limit on the number of bytes a Wasm::Plan::compile should attempt for small wasm binary before checking for other work.") },
    OptionInfo { name: "wasmLargePartialCompileLimit", option_type: OptionType::Size, availability: Availability::Normal, description: Some("Limit on the number of bytes a Wasm::Plan::compile should attempt for large wasm binary before checking for other work.") },
    OptionInfo { name: "wasmOMGOptimizationLevel", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("B3 Optimization level for OMG Web Assembly module compilations.") },
    OptionInfo { name: "useWasmByteLoopReplacement", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, OMG replaces a loop that copies or fills linear memory one byte per iteration with the equivalent bulk memory operation.") },
    OptionInfo { name: "useBBQTierUpChecks", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enables tier up checks for our BBQ code.") },
    OptionInfo { name: "useWasmOSR", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForBBQOptimizeAfterWarmUp", option_type: OptionType::Int32, availability: Availability::Normal, description: Some("The count before we tier up a function to BBQ.") },
    OptionInfo { name: "thresholdForBBQOptimizeSoon", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForOMGOptimizeAfterWarmUp", option_type: OptionType::Int32, availability: Availability::Normal, description: Some("The count before we tier up a function to OMG.") },
    OptionInfo { name: "thresholdForOMGOptimizeSoon", option_type: OptionType::Int32, availability: Availability::Normal, description: None },
    OptionInfo { name: "maximumOMGCandidateCost", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "omgTierUpCounterIncrementForLoop", option_type: OptionType::Int32, availability: Availability::Normal, description: Some("The amount the tier up counter is incremented on each loop backedge.") },
    OptionInfo { name: "omgTierUpCounterIncrementForEntry", option_type: OptionType::Int32, availability: Availability::Normal, description: Some("The amount the tier up counter is incremented on each function entry.") },
    OptionInfo { name: "wasmOMGEntryIncrementSizeReference", option_type: OptionType::Int32, availability: Availability::Normal, description: Some("If non-zero, the BBQ->OMG function-entry tier-up increment is scaled down for functions whose bytecode size is below this reference (work-proportional tier-up): increment = clamp(entryIncrement * size / reference, 1, entryIncrement). 0 disables (flat increment).") },
    OptionInfo { name: "useWasmFastMemory", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, we will try to use a 32-bit address space with a signal handler to bounds check wasm memory.") },
    OptionInfo { name: "logWasmMemory", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "wasmFastMemoryRedzonePages", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Wasm fast memories use 4GiB virtual allocations, plus a redzone (counted as multiple of 64KiB Wasm pages) at the end to catch reg+imm accesses which exceed 32-bit, anything beyond the redzone is explicitly bounds-checked") },
    OptionInfo { name: "crashIfWasmCantFastMemory", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, we will crash if we can't obtain fast memory for wasm.") },
    OptionInfo { name: "crashOnFailedWasmValidate", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, we will crash if we can't validate a wasm module instead of throwing an exception.") },
    OptionInfo { name: "maxNumWasmFastMemories", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseBBQJITAllocation", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Logs extra information about register allocation during BBQ JIT") },
    OptionInfo { name: "verboseBBQJITInstructions", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Logs instruction information during BBQ JIT") },
    OptionInfo { name: "disableBBQConsts", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Wasm <type>.const instructions in BBQ JIT won't lower to a const BBQ::Value") },
    OptionInfo { name: "useBBQJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows the BBQ JIT to be used if true") },
    OptionInfo { name: "useOMGJIT", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("allows the OMG JIT to be used if true") },
    OptionInfo { name: "wasmFunctionIndexRangeToCompile", option_type: OptionType::OptionRange, availability: Availability::Normal, description: Some("wasm function index range to allow compilation on, e.g. 1:100") },
    OptionInfo { name: "useEagerWasmModuleHashing", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Unnamed Wasm modules are identified in backtraces through their hash, if available.") },
    OptionInfo { name: "useArrayAllocationProfiling", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, we will use our normal array allocation profiling. If false, the allocation profile will always claim to be undecided.") },
    OptionInfo { name: "forcePolyProto", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, create_this will always create an object with a poly proto structure.") },
    OptionInfo { name: "forceMiniVMMode", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, it will force mini VM mode on.") },
    OptionInfo { name: "useTracePoints", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useCompilerSignpost", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useGCSignpost", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "traceLLIntExecution", option_type: OptionType::Bool, availability: Availability::Configurable, description: None },
    OptionInfo { name: "traceLLIntSlowPath", option_type: OptionType::Bool, availability: Availability::Configurable, description: None },
    OptionInfo { name: "traceBaselineJITExecution", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForGlobalLexicalBindingEpoch", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Threshold for global lexical binding epoch. If the epoch reaches to this value, CodeBlock metadata for scope operations will be revised globally. It needs to be greater than 1.") },
    OptionInfo { name: "diskCachePath", option_type: OptionType::OptionString, availability: Availability::Restricted, description: None },
    OptionInfo { name: "verboseDiskCache", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, we will log cache hits and misses.") },
    OptionInfo { name: "forceDiskCache", option_type: OptionType::Bool, availability: Availability::Restricted, description: None },
    OptionInfo { name: "validateAbstractInterpreterState", option_type: OptionType::Bool, availability: Availability::Restricted, description: None },
    OptionInfo { name: "validateAbstractInterpreterStateProbability", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpJITMemoryPath", option_type: OptionType::OptionString, availability: Availability::Restricted, description: None },
    OptionInfo { name: "dumpJITMemoryFlushInterval", option_type: OptionType::Double, availability: Availability::Restricted, description: Some("Maximum time in between flushes of the JIT memory dump in seconds.") },
    OptionInfo { name: "useUnlinkedCodeBlockJettisoning", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, UnlinkedCodeBlock can be jettisoned.") },
    OptionInfo { name: "forceOSRExitToLLInt", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, we always exit to the LLInt. If false, we exit to whatever is most convenient.") },
    OptionInfo { name: "getByValICMaxNumberOfIdentifiers", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Number of identifiers we see in the LLInt that could cause us to bail on generating an IC for get_by_val.") },
    OptionInfo { name: "useRandomizingExecutableIslandAllocation", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("For the arm64 ExecutableAllocator, if true, select which region to use randomly. This is useful for testing that jump islands work.") },
    OptionInfo { name: "exposeProfilersOnGlobalObject", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, we will expose functions to enable/disable both the sampling profiler and the super sampler") },
    OptionInfo { name: "allowUnsupportedTiers", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("If true, we will not disable DFG or FTL when an experimental feature is enabled.") },
    OptionInfo { name: "returnEarlyFromInfiniteLoopsForFuzzing", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "earlyReturnFromInfiniteLoopsLimit", option_type: OptionType::Size, availability: Availability::Normal, description: Some("When returnEarlyFromInfiniteLoopsForFuzzing is true, this determines the number of executions a loop can run for before just returning. This is helpful for the fuzzer so it doesn't get stuck in infinite loops.") },
    OptionInfo { name: "useLICMFuzzing", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "seedForLICMFuzzer", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "allowHoistingLICMProbability", option_type: OptionType::Double, availability: Availability::Normal, description: None },
    OptionInfo { name: "exposeCustomSettersOnGlobalObjectForTesting", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useJITCage", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useAllocationProfiling", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allows toggling of bmalloc/libPAS allocation profiling features at JSC launch.") },
    OptionInfo { name: "allocationProfilingMode", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Allows custom arguments to be passed to bmalloc/libPAS allocation profiling features at JSC launch.") },
    OptionInfo { name: "dumpBaselineJITSizeStatistics", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpDFGJITSizeStatistics", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useLoopUnrolling", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "usePartialLoopUnrolling", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseLoopUnrolling", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "disallowLoopUnrollingForNonInnermost", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxLoopUnrollingCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxLoopUnrollingBodyNodeSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxLoopUnrollingIterationCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxPartialLoopUnrollingBodyNodeSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxPartialLoopUnrollingIterationCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxNumericHotLoopSize", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "maxIntegerRangeOptimizationRelationshipsPerNode", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("How many relationships IRO keeps about any one node, 0 for no cap.") },
    OptionInfo { name: "maxIntegerRangeOptimizationWork", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Give up threshold for IRO") },
    OptionInfo { name: "printEachUnrolledLoop", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "verboseExecutablePoolAllocation", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useHandlerICInFTL", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useLLIntICs", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Use property and call ICs in LLInt code.") },
    OptionInfo { name: "useBaselineJITCodeSharing", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "libpasScavengeContinuously", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "libpasForcePGMWithRate", option_type: OptionType::Unsigned, availability: Availability::Normal, description: Some("Forces on probablistic guard malloc and guards allocations with a rate 1/N (0 is disabled)") },
    OptionInfo { name: "useWasmFaultSignalHandler", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpUnlinkedDFGValidation", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpWasmOpcodeStatistics", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpWasmWarnings", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useRecursiveJSONParse", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "thresholdForStringReplaceCache", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "useWasmIPInt", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Use the in-place interpereter for WASM instead of LLInt.") },
    OptionInfo { name: "useWasmIPIntPrologueOSR", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow IPInt to tier up during function prologues") },
    OptionInfo { name: "useWasmIPIntLoopOSR", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow IPInt to tier up during loop iterations") },
    OptionInfo { name: "useWasmIPIntEpilogueOSR", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow IPInt to tier up during function epilogues") },
    OptionInfo { name: "useWasmIPIntSIMD", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow IPInt to interpret SIMD code") },
    OptionInfo { name: "traceWasmIPIntExecution", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "forceAllFunctionsToUseSIMD", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Force all functions to act conservatively w.r.t fp/vector registers for testing.") },
    OptionInfo { name: "useOMGInlining", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Use OMG inlining") },
    OptionInfo { name: "freeRetiredWasmCode", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("free BBQ/OMG-OSR wasm code once it's no longer reachable.") },
    OptionInfo { name: "useArrayAllocationSinking", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpFTLCodeSize", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpOptimizationTracing", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "dumpIonGraph", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "ionGraphDirectory", option_type: OptionType::OptionString, availability: Availability::Normal, description: Some("Directory to place IonGraph") },
    OptionInfo { name: "markedBlockDumpInfoCount", option_type: OptionType::Unsigned, availability: Availability::Normal, description: None },
    OptionInfo { name: "useAsyncStackTrace", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable async stack traces") },
    OptionInfo { name: "useBigIntMathMethods", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable BigInt math helper methods.") },
    OptionInfo { name: "useExplicitResourceManagement", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable explicit resource management builtins and syntax.") },
    OptionInfo { name: "useImportDefer", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable deferred module import.") },
    OptionInfo { name: "useImportText", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable text module import.") },
    OptionInfo { name: "useIteratorChunking", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Expose the Iterator.prototype.chunks and Iterator.prototype.windows methods.") },
    OptionInfo { name: "useIteratorIncludes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Expose the Iterator.includes method.") },
    OptionInfo { name: "useIteratorJoin", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Expose the Iterator.prototype.join method.") },
    OptionInfo { name: "useIteratorSequencing", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Expose the Iterator.concat method.") },
    OptionInfo { name: "useJSONSourceTextAccess", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Expose JSON source text access feature.") },
    OptionInfo { name: "useJSPI", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable the implementation of JavaScript Promise Integration.") },
    OptionInfo { name: "useJointIteration", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Expose the Iterator.zip and Iterator.zipKeyed methods") },
    OptionInfo { name: "useMoreCurrencyDisplayChoices", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable more currencyDisplay choices for Intl.NumberFormat") },
    OptionInfo { name: "usePromiseIsPromise", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Expose the Promise.isPromise method.") },
    OptionInfo { name: "useRegExpBufferBoundaries", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow the use in regular expressions of \\A, \\z and \\Z from the RegExp Buffer Boundaries proposal") },
    OptionInfo { name: "useShadowRealm", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Expose the ShadowRealm object.") },
    OptionInfo { name: "useTemporal", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Expose the Temporal object.") },
    OptionInfo { name: "useWasmJSStringBuiltins", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable the implementation of the JS String Builtins proposal.") },
    OptionInfo { name: "useWasmJSTypes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable the js-types proposal: type() on WebAssembly.Memory/Table/Global/Tag, and the type field on WebAssembly.Module.imports/exports descriptors.") },
    OptionInfo { name: "useWasmMemory64", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow the Memory64 proposal for WebAssembly.") },
    OptionInfo { name: "useWasmMemoryToBufferAPIs", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable the toFixedLengthBuffer() and toResizableBuffer() Wasm Memory.prototype functions.") },
    OptionInfo { name: "useWasmMultiMemory", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow wasm code to access multiple linear memories") },
    OptionInfo { name: "useWasmRelaxedSIMD", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow the relaxed simd instructions and types from the wasm relaxed simd spec.") },
    OptionInfo { name: "useWasmSIMD", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow the new simd instructions and types from the wasm simd spec.") },
    OptionInfo { name: "useWasmTailCalls", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow the new instructions from the wasm tail calls spec.") },
    OptionInfo { name: "useWasmWideArithmetic", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Allow the wide arithmetic instructions from the wasm wide-arithmetic spec.") },
    OptionInfo { name: "disallowMixedWasmExceptions", option_type: OptionType::Bool, availability: Availability::Restricted, description: Some("Disallow using both legacy and modern (try_table) wasm exception specs in the same module.") },
    OptionInfo { name: "useSharedArrayBuffer", option_type: OptionType::Bool, availability: Availability::Normal, description: None },
    OptionInfo { name: "useTrustedTypes", option_type: OptionType::Bool, availability: Availability::Normal, description: Some("Enable trusted types eval protection feature.") },
];

/// `FOR_EACH_JSC_ALIASED_OPTION`: nome antigo, opção que ele aponta e se o valor se inverte.
pub static OPTIONS_ALIASES: [OptionAlias; 32] = [
    OptionAlias { name: "enableFunctionDotArguments", target: "useFunctionDotArguments", inverted: false },
    OptionAlias { name: "enableTailCalls", target: "useTailCalls", inverted: false },
    OptionAlias { name: "showDisassembly", target: "dumpDisassembly", inverted: false },
    OptionAlias { name: "showDFGDisassembly", target: "dumpDFGDisassembly", inverted: false },
    OptionAlias { name: "showFTLDisassembly", target: "dumpFTLDisassembly", inverted: false },
    OptionAlias { name: "dumpGraphAtEachDFGFTLPhase", target: "dumpDFGFTLGraphAtEachPhase", inverted: false },
    OptionAlias { name: "dumpGraphAtEachDFGPhase", target: "dumpDFGGraphAtEachPhase", inverted: false },
    OptionAlias { name: "dumpGraphAtEachB3Phase", target: "dumpB3GraphAtEachPhase", inverted: false },
    OptionAlias { name: "dumpGraphAtEachAirPhase", target: "dumpAirGraphAtEachPhase", inverted: false },
    OptionAlias { name: "alwaysDoFullCollection", target: "useGenerationalGC", inverted: true },
    OptionAlias { name: "enableOSREntryToDFG", target: "useOSREntryToDFG", inverted: false },
    OptionAlias { name: "enableOSREntryToFTL", target: "useOSREntryToFTL", inverted: false },
    OptionAlias { name: "enableAccessInlining", target: "useAccessInlining", inverted: false },
    OptionAlias { name: "enablePolyvariantDevirtualization", target: "usePolyvariantDevirtualization", inverted: false },
    OptionAlias { name: "enablePolymorphicAccessInlining", target: "usePolymorphicAccessInlining", inverted: false },
    OptionAlias { name: "enablePolymorphicCallInlining", target: "usePolymorphicCallInlining", inverted: false },
    OptionAlias { name: "enableObjectAllocationSinking", target: "useObjectAllocationSinking", inverted: false },
    OptionAlias { name: "enableConcurrentJIT", target: "useConcurrentJIT", inverted: false },
    OptionAlias { name: "enableProfiler", target: "useProfiler", inverted: false },
    OptionAlias { name: "enableArchitectureSpecificOptimizations", target: "useArchitectureSpecificOptimizations", inverted: false },
    OptionAlias { name: "objectsAreImmortal", target: "useImmortalObjects", inverted: false },
    OptionAlias { name: "disableGC", target: "useGC", inverted: true },
    OptionAlias { name: "enableTypeProfiler", target: "useTypeProfiler", inverted: false },
    OptionAlias { name: "enableControlFlowProfiler", target: "useControlFlowProfiler", inverted: false },
    OptionAlias { name: "enableExceptionFuzz", target: "useExceptionFuzz", inverted: false },
    OptionAlias { name: "enableExecutableAllocationFuzz", target: "useExecutableAllocationFuzz", inverted: false },
    OptionAlias { name: "enableOSRExitFuzz", target: "useOSRExitFuzz", inverted: false },
    OptionAlias { name: "enableDollarVM", target: "useDollarVM", inverted: false },
    OptionAlias { name: "maximumOptimizationCandidateInstructionCount", target: "maximumOptimizationCandidateBytecodeCost", inverted: false },
    OptionAlias { name: "maximumFTLCandidateInstructionCount", target: "maximumFTLCandidateBytecodeCost", inverted: false },
    OptionAlias { name: "maximumInliningCallerSize", target: "maximumInliningCallerBytecodeCost", inverted: false },
    OptionAlias { name: "validateBCE", target: "validateBoundsCheckElimination", inverted: false },
];
