//! Porte de `src/vs/platform/dialogs/test/electron-main/dialogMainService.test.ts` (suíte `DialogMainService`).
//!
//! Notas de tradução:
//! - O `import electron from 'electron'` do serviço é o trait `NativeDialogs`; este caso nunca mostra um diálogo, então o
//!   `NativeDialogs` do teste é o duplo que falha se for chamado.
//! - O `{ id: 1 } as BrowserWindow` é o `BrowserWindow { id: 1 }`; o `getWindowDialogQueue` e o `windowDialogQueues.size`, que o
//!   original alcança por um `as unknown as`, são `get_window_dialog_queue` e `window_dialog_queue_count`.
//! - O `async () => ({ response: 0, checkboxChecked: false })` é a fábrica que devolve a promessa já resolvida.

mod common;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::promise::Promise;
use je_platform::dialogs::dialog_main_service::{
    BrowserWindow, DialogMainService, DialogResult, MessageBoxOptions, MessageBoxReturnValue, NativeDialogs, OpenDialogOptions, OpenDialogReturnValue, SaveDialogOptions,
    SaveDialogReturnValue,
};
use je_platform::log::NullLogService;
use je_platform::product::ProductService;

struct TestNativeDialogs;

impl NativeDialogs for TestNativeDialogs {
    fn get_focused_window(&self) -> Option<BrowserWindow> {
        panic!("Method not implemented.")
    }

    fn show_message_box(&self, _window: Option<BrowserWindow>, _options: MessageBoxOptions) -> Promise<MessageBoxReturnValue> {
        panic!("Method not implemented.")
    }

    fn show_save_dialog(&self, _window: Option<BrowserWindow>, _options: SaveDialogOptions) -> Promise<SaveDialogReturnValue> {
        panic!("Method not implemented.")
    }

    fn show_open_dialog(&self, _window: Option<BrowserWindow>, _options: OpenDialogOptions) -> Promise<OpenDialogReturnValue> {
        panic!("Method not implemented.")
    }
}

#[test]
fn discards_a_per_window_dialog_queue_after_it_drains() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async {
        let product_service = ProductService { name_long: "Test".to_owned(), ..ProductService::default() };
        let service = DialogMainService::new(NullLogService::shared(), Rc::new(product_service), Rc::new(TestNativeDialogs));
        let window = BrowserWindow { id: 1 };
        let queue = service.get_window_dialog_queue(Some(window));

        assert_eq!(service.window_dialog_queue_count(), 1);
        queue.queue(|| Promise::resolved(DialogResult::from(MessageBoxReturnValue { response: 0, checkbox_checked: false }))).await.unwrap();
        queue.when_idle().await.unwrap();
        assert_eq!(service.window_dialog_queue_count(), 0);
    });
}
