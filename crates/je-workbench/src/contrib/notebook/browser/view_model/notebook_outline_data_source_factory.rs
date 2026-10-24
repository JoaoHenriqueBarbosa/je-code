//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/notebookOutlineDataSourceFactory.ts`: a fábrica que entrega, por editor,
//! a fonte de dados do outline (`NotebookCellOutlineDataSource`) por referência contada. A fonte de dados nasce na primeira
//! referência do editor e é descartada quando a última é solta.
//!
//! Divergências:
//! - `NotebookCellOutlineDataSourceReferenceCollection` (a `ReferenceCollection` com o `@IInstantiationService`) é o
//!   [`ReferenceCollection`] sobre o [`DataSourceObjects`] (que só descarta o objeto); os serviços que o `createInstance(
//!   NotebookCellOutlineDataSource, editor)` injetava ficam na fábrica. O `createReferencedObject(_key, editor)` recebe o editor,
//!   então a criação vai pelo `acquire_with`; o `create_referenced_object(key)` sem argumentos do contrato nunca é chamado e lança.
//! - `editor: INotebookEditor` é o `Rc<dyn IActiveNotebookEditor>` (ver `notebook_outline_data_source.rs`).

use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::{IDisposable, Reference, ReferenceCollection, ReferencedObjects};
use je_platform::configuration::service::IConfigurationService;
use je_platform::markers::markers::IMarkerService;

use super::notebook_outline_data_source::NotebookCellOutlineDataSource;
use super::notebook_outline_entry_factory::INotebookOutlineEntryFactory;
use crate::contrib::notebook::browser::controller::IActiveNotebookEditor;

/// O descarte do objeto referenciado (`destroyReferencedObject`).
struct DataSourceObjects;

impl ReferencedObjects<Rc<NotebookCellOutlineDataSource>> for DataSourceObjects {
    fn create_referenced_object(&self, _key: &str) -> Rc<NotebookCellOutlineDataSource> {
        errors::throw(Error::new("NotebookCellOutlineDataSourceReferenceCollection.createReferencedObject exige o editor"))
    }

    fn destroy_referenced_object(&self, _key: &str, object: &Rc<NotebookCellOutlineDataSource>) {
        object.dispose();
    }
}

/// `INotebookCellOutlineDataSourceFactory`.
pub trait INotebookCellOutlineDataSourceFactory {
    fn get_or_create(&self, editor: Rc<dyn IActiveNotebookEditor>) -> Reference<Rc<NotebookCellOutlineDataSource>>;
}

je_instantiation::service_id!(pub I_NOTEBOOK_CELL_OUTLINE_DATA_SOURCE_FACTORY: dyn INotebookCellOutlineDataSourceFactory = "INotebookCellOutlineDataSourceFactory");

/// `NotebookCellOutlineDataSourceFactory`.
pub struct NotebookCellOutlineDataSourceFactory {
    data: ReferenceCollection<Rc<NotebookCellOutlineDataSource>, DataSourceObjects>,
    marker_service: Rc<dyn IMarkerService>,
    configuration_service: Rc<dyn IConfigurationService>,
    outline_entry_factory: Rc<dyn INotebookOutlineEntryFactory>,
}

impl NotebookCellOutlineDataSourceFactory {
    pub fn new(
        marker_service: Rc<dyn IMarkerService>,
        configuration_service: Rc<dyn IConfigurationService>,
        outline_entry_factory: Rc<dyn INotebookOutlineEntryFactory>,
    ) -> Self {
        Self { data: ReferenceCollection::new(DataSourceObjects), marker_service, configuration_service, outline_entry_factory }
    }
}

impl INotebookCellOutlineDataSourceFactory for NotebookCellOutlineDataSourceFactory {
    fn get_or_create(&self, editor: Rc<dyn IActiveNotebookEditor>) -> Reference<Rc<NotebookCellOutlineDataSource>> {
        let key = editor.get_id();
        self.data.acquire_with(&key, || {
            NotebookCellOutlineDataSource::new(editor, self.marker_service.clone(), self.configuration_service.clone(), self.outline_entry_factory.clone())
        })
    }
}
