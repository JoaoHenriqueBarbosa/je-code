//! Porte parcial de `src/vs/workbench/contrib/debug/common/debugVisualizers.ts`: o contrato `IDebugVisualizerService` que o
//! `MainThreadDebugService` usa para registrar visualizadores e árvores, com os dois `Handle` que ele recebe.
//!
//! Divergências:
//! - Ficam de fora o `DebugVisualizer`, o `DebugVisualizerService` (que depende do `IContextKeyService`, do `IExtensionService`,
//!   do ponto de extensão `debugVisualizers` e do modelo de variáveis do `debugModel.ts`) e os membros `getApplicableFor`,
//!   `getVisualizedNodeFor`, `getVisualizedChildren` e `editTreeItem` do trait, porque nenhum chamador portado os usa e eles
//!   pedem `IExpression`, `VisualizedExpression` e `IReference`.
//! - Os métodos dos `Handle` são fechos compartilhados (`Rc<dyn Fn>`); o `editItem?` opcional é `Option`.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_extensions::manifest::ExtensionIdentifier;

use super::debug::{IDebugVisualization, IDebugVisualizationContext, IDebugVisualizationTreeItem, MainThreadDebugVisualization};

/// `VisualizerHandle`.
#[derive(Clone)]
pub struct VisualizerHandle {
    pub id: String,
    pub extension_id: ExtensionIdentifier,
    pub provide_debug_visualizers: Rc<dyn Fn(IDebugVisualizationContext, CancellationToken) -> Promise<Vec<IDebugVisualization>>>,
    pub resolve_debug_visualizer: Rc<dyn Fn(&IDebugVisualization, CancellationToken) -> Promise<MainThreadDebugVisualization>>,
    pub execute_debug_visualizer_command: Rc<dyn Fn(u64) -> Promise<()>>,
    pub dispose_debug_visualizers: Rc<dyn Fn(Vec<u64>)>,
}

/// `VisualizerTreeHandle`.
#[derive(Clone)]
pub struct VisualizerTreeHandle {
    pub get_tree_item: Rc<dyn Fn(IDebugVisualizationContext) -> Promise<Option<IDebugVisualizationTreeItem>>>,
    pub get_children: Rc<dyn Fn(u64) -> Promise<Vec<IDebugVisualizationTreeItem>>>,
    pub dispose_item: Rc<dyn Fn(u64)>,
    pub edit_item: Option<Rc<dyn Fn(u64, &str) -> Promise<Option<IDebugVisualizationTreeItem>>>>,
}

/// `IDebugVisualizerService`.
pub trait IDebugVisualizerService {
    /// `register(handle)`: registers a new visualizer (called from the main thread debug service).
    fn register(&self, handle: VisualizerHandle) -> DisposableRef;

    /// `registerTree(treeId, handle)`: registers a new visualizer tree.
    fn register_tree(&self, tree_id: &str, handle: VisualizerTreeHandle) -> DisposableRef;
}
