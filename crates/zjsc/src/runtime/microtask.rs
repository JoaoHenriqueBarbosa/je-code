//! Tradução de `runtime/Microtask.h`: o `InternalMicrotask`, as constantes e os predicados puros.
//! `QueuedTask` e a fila de microtasks são do heap e ficam fora desta fatia; `SynchronousModuleTask`
//! (`VM.h`) mora aqui porque o `JSPromise` o monta para a fila síncrona do carregador de módulos.
//!
//! `#if ENABLE(WEBASSEMBLY)` vale (o Bun serve WebAssembly em Linux x86_64) e
//! `#if USE(BUN_JSC_ADDITIONS)` vale (`derived/cmakeconfig.h`).

use crate::runtime::js_value::JSValue;

/// `enum class InternalMicrotask : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InternalMicrotask {
    None = 0,
    PromiseResolveThenableJobFast,
    PromiseResolveThenableJobWithInternalMicrotaskFast,

    PromiseResolveThenableJob,
    PromiseResolveThenableJobWithInternalMicrotask,

    PromiseResolveWithoutHandlerJob,
    PromiseFulfillWithoutHandlerJob,

    PromiseRaceResolveJob,
    PromiseAllResolveJob,
    PromiseAllSettledResolveJob,
    PromiseAnyResolveJob,
    PromiseFinallyReactionJob,
    PromiseFinallyAwaitJob,

    PromiseReactionJob,

    AsyncFunctionResume,
    AsyncFromSyncIteratorContinue,
    AsyncFromSyncIteratorDone,
    AsyncGeneratorYieldAwaited,
    AsyncGeneratorBodyCallNormal,
    AsyncGeneratorBodyCallReturn,
    AsyncGeneratorAwaitReturn,
    AsyncGeneratorDriverResume,

    InvokeFunctionJob,
    AsyncModuleExecutionResume,
    AsyncModuleExecutionDone,
    ModuleRegistryFetchSettled,
    ModuleRegistryModuleSettled,
    ModuleGraphLoadingError,
    ModuleLoadStep,
    ModuleLoadTopSettled,
    ModuleLoadTopRejected,
    ModuleLoadSpecifierTransform,
    ModuleLoadCombinedLoadSettled,
    ModuleLoadCombinedStateSettled,
    ModuleLoadLinkEvaluateSettled,
    ModuleLoadReturnRecord,
    ModuleLoadReturnModuleKey,
    ModuleLoadStoreError,
    DynamicImportLoadSettled,
    DynamicImportEvaluateSettled,
    DynamicImportDeferLoadSettled,
    DynamicImportDeferDependencySettled,
    ImportModuleNamespace,
    WebAssemblyCompileStreaming,
    WebAssemblyInstantiateStreaming,
    /// Dispatch must handle everything.
    Opaque,
    /// Bun's performMicrotask function with async context.
    BunPerformMicrotaskJob,
    /// Invoke job function with up to 4 arguments.
    BunInvokeJobWithArguments,
}

impl InternalMicrotask {
    /// Todas as variantes na ordem do enum: o índice é o valor do `uint8_t`.
    const ALL: [InternalMicrotask; 48] = {
        use InternalMicrotask::*;
        [
            None,
            PromiseResolveThenableJobFast,
            PromiseResolveThenableJobWithInternalMicrotaskFast,
            PromiseResolveThenableJob,
            PromiseResolveThenableJobWithInternalMicrotask,
            PromiseResolveWithoutHandlerJob,
            PromiseFulfillWithoutHandlerJob,
            PromiseRaceResolveJob,
            PromiseAllResolveJob,
            PromiseAllSettledResolveJob,
            PromiseAnyResolveJob,
            PromiseFinallyReactionJob,
            PromiseFinallyAwaitJob,
            PromiseReactionJob,
            AsyncFunctionResume,
            AsyncFromSyncIteratorContinue,
            AsyncFromSyncIteratorDone,
            AsyncGeneratorYieldAwaited,
            AsyncGeneratorBodyCallNormal,
            AsyncGeneratorBodyCallReturn,
            AsyncGeneratorAwaitReturn,
            AsyncGeneratorDriverResume,
            InvokeFunctionJob,
            AsyncModuleExecutionResume,
            AsyncModuleExecutionDone,
            ModuleRegistryFetchSettled,
            ModuleRegistryModuleSettled,
            ModuleGraphLoadingError,
            ModuleLoadStep,
            ModuleLoadTopSettled,
            ModuleLoadTopRejected,
            ModuleLoadSpecifierTransform,
            ModuleLoadCombinedLoadSettled,
            ModuleLoadCombinedStateSettled,
            ModuleLoadLinkEvaluateSettled,
            ModuleLoadReturnRecord,
            ModuleLoadReturnModuleKey,
            ModuleLoadStoreError,
            DynamicImportLoadSettled,
            DynamicImportEvaluateSettled,
            DynamicImportDeferLoadSettled,
            DynamicImportDeferDependencySettled,
            ImportModuleNamespace,
            WebAssemblyCompileStreaming,
            WebAssemblyInstantiateStreaming,
            Opaque,
            BunPerformMicrotaskJob,
            BunInvokeJobWithArguments,
        ]
    };

    /// `static_cast<InternalMicrotask>(uint8_t)`: o `JSSlimPromiseReaction` guarda a tarefa no byte
    /// `m_next.type()`. `None` (de `Option`) se o byte não é de nenhuma variante.
    pub fn from_u8(value: u8) -> Option<InternalMicrotask> {
        InternalMicrotask::ALL.get(value as usize).copied()
    }
}

/// `maxMicrotaskArguments` (`USE(BUN_JSC_ADDITIONS)`).
pub const MAX_MICROTASK_ARGUMENTS: u32 = 4;

/// `VM::SynchronousModuleTask`: a tarefa que `VM::m_synchronousModuleQueue` recebe no lugar da fila
/// global de microtasks (os mesmos quatro argumentos que `JSGlobalObject::queueMicrotask` leva).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SynchronousModuleTask {
    pub task: InternalMicrotask,
    pub payload: u8,
    /// `arg0`...`arg3`; os que a chamada não passa ficam vazios (`JSValue()`).
    pub arguments: [JSValue; MAX_MICROTASK_ARGUMENTS as usize],
}

impl SynchronousModuleTask {
    /// Preenche com `JSValue()` os argumentos que faltam.
    pub fn new(task: InternalMicrotask, payload: u8, arguments: &[JSValue]) -> SynchronousModuleTask {
        assert!(arguments.len() <= MAX_MICROTASK_ARGUMENTS as usize);
        let mut padded = [JSValue::empty(); MAX_MICROTASK_ARGUMENTS as usize];
        padded[..arguments.len()].copy_from_slice(arguments);
        SynchronousModuleTask { task, payload, arguments: padded }
    }
}

/// `promiseReactionJobAsyncContextFlag`: OR'ed into the payload (a `JSPromise::Status`) of a
/// `PromiseReactionJob` whose fourth argument is the async context captured by `performPromiseThen`.
pub const PROMISE_REACTION_JOB_ASYNC_CONTEXT_FLAG: u8 = 0x80;

/// `isModuleLoaderInternalMicrotask(InternalMicrotask)`: o bloco contíguo de tarefas do pipeline do
/// carregador de módulos, mais `PromiseFulfillWithoutHandlerJob`.
pub const fn is_module_loader_internal_microtask(task: InternalMicrotask) -> bool {
    if task as u8 == InternalMicrotask::PromiseFulfillWithoutHandlerJob as u8 {
        return true;
    }
    task as u8 >= InternalMicrotask::AsyncModuleExecutionResume as u8
        && task as u8 <= InternalMicrotask::ImportModuleNamespace as u8
}

/// `promiseReactionPacksGlobalContextAndIndex(InternalMicrotask)`: os jobs de elemento de
/// `Promise.all`/`allSettled`/`any`, cuja reação empacota (célula de contexto global, índice).
pub const fn promise_reaction_packs_global_context_and_index(task: InternalMicrotask) -> bool {
    const {
        assert!(
            InternalMicrotask::PromiseAllSettledResolveJob as u8 == InternalMicrotask::PromiseAllResolveJob as u8 + 1
        );
        assert!(
            InternalMicrotask::PromiseAnyResolveJob as u8 == InternalMicrotask::PromiseAllSettledResolveJob as u8 + 1
        );
    }
    task as u8 >= InternalMicrotask::PromiseAllResolveJob as u8
        && task as u8 <= InternalMicrotask::PromiseAnyResolveJob as u8
}

/// `enum class QueuedTaskResult : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueuedTaskResult {
    Executed,
    Discard,
    Suspended,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_u8_follows_the_enum_order() {
        for (index, task) in InternalMicrotask::ALL.iter().enumerate() {
            assert_eq!(*task as u8 as usize, index);
            assert_eq!(InternalMicrotask::from_u8(index as u8), Some(*task));
        }
        assert_eq!(InternalMicrotask::from_u8(48), None);
        assert_eq!(InternalMicrotask::from_u8(47), Some(InternalMicrotask::BunInvokeJobWithArguments));
    }

    #[test]
    fn synchronous_module_task_pads_with_empty() {
        let task = SynchronousModuleTask::new(InternalMicrotask::PromiseReactionJob, 1, &[JSValue::undefined()]);
        assert_eq!(task.arguments[0], JSValue::undefined());
        assert!(task.arguments[3].is_empty());
    }
}
