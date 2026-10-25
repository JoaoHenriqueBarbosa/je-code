//! Porte de `src/vs/platform/dialogs/test/electron-main/dialogMainUtils.test.ts` (suíte `Dialog`).
//!
//! Notas de tradução:
//! - O `isWindows`, o `isMacintosh` e o `isLinux` do `platform.ts` são o [`Platform`] do processo, e o
//!   `Number.parseInt(release(), 10)` é `os_release_major` (o número do começo de `os.release()`, ausente quando a versão não
//!   começa com dígito, o `NaN` que nunca é `>= 24`).
//! - O `IMassagedMessageBoxOptions` é o `MassagedMessageBoxOptions`; o `deepEqual` sobre os índices e os botões é a igualdade
//!   dos vetores.
//! - O `product` do original é o `product.json` do VS Code; aqui é o [`ProductService`] padrão com o `nameLong` `Test`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::platform::Platform;
use je_platform::dialogs::dialog_main_service::MessageBoxOptions;
use je_platform::dialogs::dialog_main_utils::{MassagedMessageBoxOptions, massage_message_box_options, os_release_major};
use je_platform::product::ProductService;

fn assert_options(massaged: &MassagedMessageBoxOptions, buttons: &[&str], default_id: i32, cancel_id: i32, indeces: &[usize]) {
    assert_eq!(massaged.options.buttons, buttons);
    assert_eq!(massaged.options.default_id, Some(default_id));
    assert_eq!(massaged.options.cancel_id, Some(cancel_id));
    assert_eq!(massaged.button_indeces, indeces);
}

#[test]
fn massage_message_box_options_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let test_product_service = ProductService { name_long: "Test".to_owned(), ..ProductService::default() };
    let host = Platform::detect();

    // `massageMessageBoxOptions({ buttons: ['1', ..., 'N'], cancelId, message: 'message' }, testProductService)`
    let massage = |button_count: usize, cancel_id: Option<i32>| {
        massage_message_box_options(
            MessageBoxOptions { buttons: (1..=button_count).map(|number| number.to_string()).collect(), cancel_id, message: "message".to_owned(), ..MessageBoxOptions::default() },
            &test_product_service,
            &host,
        )
    };

    // All platforms
    let all_platforms_massaged_options = massage_message_box_options(
        MessageBoxOptions { buttons: vec!["1".to_owned()], message: "message".to_owned(), ..MessageBoxOptions::default() },
        &test_product_service,
        &host,
    );
    assert_eq!(all_platforms_massaged_options.options.title.as_deref(), Some("Test"));
    assert_eq!(all_platforms_massaged_options.options.message, "message");
    assert_eq!(all_platforms_massaged_options.options.no_link, Some(true));

    // Specific cases

    let one_button_no_cancel = massage(1, None);
    let one_button_cancel_0 = massage(1, Some(0));
    let one_button_cancel_1 = massage(1, Some(1));
    let one_button_negative_cancel = massage(1, Some(-1));

    let two_button_no_cancel = massage(2, None);
    let two_button_cancel_0 = massage(2, Some(0));
    let two_button_cancel_1 = massage(2, Some(1));
    let two_button_cancel_2 = massage(2, Some(2));
    let two_button_negative_cancel = massage(2, Some(-1));

    let three_button_no_cancel = massage(3, None);
    let three_button_cancel_0 = massage(3, Some(0));
    let three_button_cancel_1 = massage(3, Some(1));
    let three_button_cancel_2 = massage(3, Some(2));
    let three_button_cancel_3 = massage(3, Some(3));
    let three_button_negative_cancel = massage(3, Some(-1));

    let four_button_no_cancel = massage(4, None);
    let four_button_cancel_0 = massage(4, Some(0));
    let four_button_cancel_1 = massage(4, Some(1));
    let four_button_cancel_2 = massage(4, Some(2));
    let four_button_cancel_3 = massage(4, Some(3));
    let four_button_cancel_4 = massage(4, Some(4));
    let four_button_negative_cancel = massage(4, Some(-1));

    if host.is_windows {
        assert_options(&one_button_no_cancel, &["1"], 0, 0, &[0]);
        assert_options(&one_button_cancel_0, &["1"], 0, 0, &[0]);
        assert_options(&one_button_cancel_1, &["1"], 0, 1, &[0]);
        assert_options(&one_button_negative_cancel, &["1"], 0, -1, &[0]);

        assert_options(&two_button_no_cancel, &["1", "2"], 0, 1, &[0, 1]);
        assert_options(&two_button_cancel_0, &["2", "1"], 0, 1, &[1, 0]);
        assert_options(&two_button_cancel_1, &["1", "2"], 0, 1, &[0, 1]);
        assert_options(&two_button_cancel_2, &["1", "2"], 0, 2, &[0, 1]);
        assert_options(&two_button_negative_cancel, &["1", "2"], 0, -1, &[0, 1]);

        assert_options(&three_button_no_cancel, &["1", "2", "3"], 0, 2, &[0, 1, 2]);
        assert_options(&three_button_cancel_0, &["2", "3", "1"], 0, 2, &[1, 2, 0]);
        assert_options(&three_button_cancel_1, &["1", "3", "2"], 0, 2, &[0, 2, 1]);
        assert_options(&three_button_cancel_2, &["1", "2", "3"], 0, 2, &[0, 1, 2]);
        assert_options(&three_button_cancel_3, &["1", "2", "3"], 0, 3, &[0, 1, 2]);
        assert_options(&three_button_negative_cancel, &["1", "2", "3"], 0, -1, &[0, 1, 2]);

        assert_options(&four_button_no_cancel, &["1", "2", "3", "4"], 0, 3, &[0, 1, 2, 3]);
        assert_options(&four_button_cancel_0, &["2", "3", "4", "1"], 0, 3, &[1, 2, 3, 0]);
        assert_options(&four_button_cancel_1, &["1", "3", "4", "2"], 0, 3, &[0, 2, 3, 1]);
        assert_options(&four_button_cancel_2, &["1", "2", "4", "3"], 0, 3, &[0, 1, 3, 2]);
        assert_options(&four_button_cancel_3, &["1", "2", "3", "4"], 0, 3, &[0, 1, 2, 3]);
        assert_options(&four_button_cancel_4, &["1", "2", "3", "4"], 0, 4, &[0, 1, 2, 3]);
        assert_options(&four_button_negative_cancel, &["1", "2", "3", "4"], 0, -1, &[0, 1, 2, 3]);
    } else if host.is_macintosh && os_release_major().is_some_and(|major| major >= 24) {
        assert_options(&one_button_no_cancel, &["1"], 0, 0, &[0]);
        assert_options(&one_button_cancel_0, &["1"], 0, 0, &[0]);
        assert_options(&one_button_cancel_1, &["1"], 0, 1, &[0]);
        assert_options(&one_button_negative_cancel, &["1"], 0, -1, &[0]);

        assert_options(&two_button_no_cancel, &["1", "2"], 0, 1, &[0, 1]);
        assert_options(&two_button_cancel_0, &["1", "2"], 0, 0, &[0, 1]);
        assert_options(&two_button_cancel_1, &["1", "2"], 0, 1, &[0, 1]);
        assert_options(&two_button_cancel_2, &["1", "2"], 0, 2, &[0, 1]);
        assert_options(&two_button_negative_cancel, &["1", "2"], 0, -1, &[0, 1]);

        assert_options(&three_button_no_cancel, &["1", "2", "3"], 0, 2, &[0, 1, 2]);
        assert_options(&three_button_cancel_0, &["1", "2", "3"], 0, 0, &[0, 1, 2]);
        assert_options(&three_button_cancel_1, &["1", "2", "3"], 0, 1, &[0, 1, 2]);
        assert_options(&three_button_cancel_2, &["1", "2", "3"], 0, 2, &[0, 1, 2]);
        assert_options(&three_button_cancel_3, &["1", "2", "3"], 0, 3, &[0, 1, 2]);
        assert_options(&three_button_negative_cancel, &["1", "2", "3"], 0, -1, &[0, 1, 2]);

        assert_options(&four_button_no_cancel, &["1", "2", "3", "4"], 0, 3, &[0, 1, 2, 3]);
        assert_options(&four_button_cancel_0, &["1", "2", "3", "4"], 0, 0, &[0, 1, 2, 3]);
        assert_options(&four_button_cancel_1, &["1", "2", "3", "4"], 0, 1, &[0, 1, 2, 3]);
        assert_options(&four_button_cancel_2, &["1", "2", "3", "4"], 0, 2, &[0, 1, 2, 3]);
        assert_options(&four_button_cancel_3, &["1", "2", "3", "4"], 0, 3, &[0, 1, 2, 3]);
        assert_options(&four_button_cancel_4, &["1", "2", "3", "4"], 0, 4, &[0, 1, 2, 3]);
        assert_options(&four_button_negative_cancel, &["1", "2", "3", "4"], 0, -1, &[0, 1, 2, 3]);
    } else if host.is_macintosh {
        assert_options(&one_button_no_cancel, &["1"], 0, 0, &[0]);
        assert_options(&one_button_cancel_0, &["1"], 0, 0, &[0]);
        assert_options(&one_button_cancel_1, &["1"], 0, 1, &[0]);
        assert_options(&one_button_negative_cancel, &["1"], 0, -1, &[0]);

        assert_options(&two_button_no_cancel, &["1", "2"], 0, 1, &[0, 1]);
        assert_options(&two_button_cancel_0, &["2", "1"], 0, 1, &[1, 0]);
        assert_options(&two_button_cancel_1, &["1", "2"], 0, 1, &[0, 1]);
        assert_options(&two_button_cancel_2, &["1", "2"], 0, 2, &[0, 1]);
        assert_options(&two_button_negative_cancel, &["1", "2"], 0, -1, &[0, 1]);

        assert_options(&three_button_no_cancel, &["1", "3", "2"], 0, 1, &[0, 2, 1]);
        assert_options(&three_button_cancel_0, &["2", "1", "3"], 0, 1, &[1, 0, 2]);
        assert_options(&three_button_cancel_1, &["1", "2", "3"], 0, 1, &[0, 1, 2]);
        assert_options(&three_button_cancel_2, &["1", "3", "2"], 0, 1, &[0, 2, 1]);
        assert_options(&three_button_cancel_3, &["1", "2", "3"], 0, 3, &[0, 1, 2]);
        assert_options(&three_button_negative_cancel, &["1", "2", "3"], 0, -1, &[0, 1, 2]);

        assert_options(&four_button_no_cancel, &["1", "4", "2", "3"], 0, 1, &[0, 3, 1, 2]);
        assert_options(&four_button_cancel_0, &["2", "1", "3", "4"], 0, 1, &[1, 0, 2, 3]);
        assert_options(&four_button_cancel_1, &["1", "2", "3", "4"], 0, 1, &[0, 1, 2, 3]);
        assert_options(&four_button_cancel_2, &["1", "3", "2", "4"], 0, 1, &[0, 2, 1, 3]);
        assert_options(&four_button_cancel_3, &["1", "4", "2", "3"], 0, 1, &[0, 3, 1, 2]);
        assert_options(&four_button_cancel_4, &["1", "2", "3", "4"], 0, 4, &[0, 1, 2, 3]);
        assert_options(&four_button_negative_cancel, &["1", "2", "3", "4"], 0, -1, &[0, 1, 2, 3]);
    } else if host.is_linux {
        assert_options(&one_button_no_cancel, &["1"], 0, 0, &[0]);
        assert_options(&one_button_cancel_0, &["1"], 0, 0, &[0]);
        assert_options(&one_button_cancel_1, &["1"], 0, 1, &[0]);
        assert_options(&one_button_negative_cancel, &["1"], 0, -1, &[0]);

        assert_options(&two_button_no_cancel, &["2", "1"], 1, 0, &[1, 0]);
        assert_options(&two_button_cancel_0, &["1", "2"], 1, 0, &[0, 1]);
        assert_options(&two_button_cancel_1, &["2", "1"], 1, 0, &[1, 0]);
        assert_options(&two_button_cancel_2, &["2", "1"], 1, 2, &[1, 0]);
        assert_options(&two_button_negative_cancel, &["2", "1"], 1, -1, &[1, 0]);

        assert_options(&three_button_no_cancel, &["2", "3", "1"], 2, 1, &[1, 2, 0]);
        assert_options(&three_button_cancel_0, &["3", "1", "2"], 2, 1, &[2, 0, 1]);
        assert_options(&three_button_cancel_1, &["3", "2", "1"], 2, 1, &[2, 1, 0]);
        assert_options(&three_button_cancel_2, &["2", "3", "1"], 2, 1, &[1, 2, 0]);
        assert_options(&three_button_cancel_3, &["3", "2", "1"], 2, 3, &[2, 1, 0]);
        assert_options(&three_button_negative_cancel, &["3", "2", "1"], 2, -1, &[2, 1, 0]);

        assert_options(&four_button_no_cancel, &["3", "2", "4", "1"], 3, 2, &[2, 1, 3, 0]);
        assert_options(&four_button_cancel_0, &["4", "3", "1", "2"], 3, 2, &[3, 2, 0, 1]);
        assert_options(&four_button_cancel_1, &["4", "3", "2", "1"], 3, 2, &[3, 2, 1, 0]);
        assert_options(&four_button_cancel_2, &["4", "2", "3", "1"], 3, 2, &[3, 1, 2, 0]);
        assert_options(&four_button_cancel_3, &["3", "2", "4", "1"], 3, 2, &[2, 1, 3, 0]);
        assert_options(&four_button_cancel_4, &["4", "3", "2", "1"], 3, 4, &[3, 2, 1, 0]);
        assert_options(&four_button_negative_cancel, &["4", "3", "2", "1"], 3, -1, &[3, 2, 1, 0]);
    }
}
