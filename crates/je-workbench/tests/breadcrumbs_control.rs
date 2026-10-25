//! Porte de `src/vs/workbench/test/browser/parts/editor/breadcrumbsControl.test.ts` (suíte `BreadcrumbsControl`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - **Os laços do original.** Os `for` que geram `test(...)` (`input` e `delivery`; `withOutline` e `altKey`; `sideBySide`) viram um
//!   `#[test]` por combinação, com o título do mocha no nome (`clicking a file in the picker` é `clicking_a_file_in_the_picker`),
//!   todos sobre o mesmo corpo parametrizado. São 23 casos gerados pelos 12 blocos `test(...)` do arquivo.
//! - **O `setup`, o `teardown` e o `createControl`.** Estão em `support/breadcrumbs_control_support.rs` ([`Harness`]) com o mapa
//!   completo das divergências: os serviços reais do `workbenchInstantiationService` que o porte tem (`LabelService`,
//!   `TestFileService` com o `resolve` trocado, `TestConfigurationService`, `NativeContextViewService`), os dublês `mock<T>()`, o
//!   DOM como nós do `Ui` (`querySelector` é a busca em pré-ordem, `click()` e `MouseEvent` são `Ui::dispatch`) e o `aria-expanded`
//!   como o estado do twistie da linha.
//! - **`await` e relógio.** Os casos `async` rodam em `run_with_real_timers` (o mocha sem `runWithFakedTimers`); o último, que o
//!   original embrulha em `runWithFakedTimers({ useFakeTimers: true }, ...)`, roda em `run_with_faked_timers`
//!   ([`Timers::Faked`]). O `await` de uma promessa é `.await.unwrap()`.
//! - **`CommandsRegistry.getCommand('breadcrumbs.selectEditor')`.** O original registra os comandos ao importar o módulo; o caso
//!   `Escape returns focus to the owning editor` chama `register_breadcrumbs_commands` (descartado pelo guarda ao fim) e executa o
//!   handler com o `invokeFunction` do serviço de instanciação.
//! - **`assert.deepStrictEqual` de objetos.** É a comparação de tuplas dos mesmos campos, na ordem do original; `picker: null` é
//!   `picker_element().is_none()` (conferido como `picker_present == false`) e o `destination` é o [`Destination`].
//!
//! Os sete últimos testes (`payloads_map_to_editor_groups`, `scrollbar_tables`, `context_keys_have_the_original_names`,
//! `picker_placement_keyboard`, `picker_placement_clamps_to_window_and_mouse_arrow`, `breadcrumbs_service_starts_empty` e
//! `commands_are_registered_with_the_original_ids`) não têm equivalente no original: conferem as contas e os marcadores do porte que
//! os casos acima exercitam.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod editor_input_services;
mod support;
#[path = "support/breadcrumbs_control_support.rs"]
mod breadcrumbs_control_support;
#[path = "support/main_thread_support.rs"]
mod main_thread_support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;

use breadcrumbs_control_support::{Destination, OpenedEditorView, Timers, directory_stat, file_child, folder_child, run_case, GROUP_ID};
use common::timeout;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::scrollable::ScrollbarVisibility;
use je_platform::commands::commands_registry;
use je_platform::resources::uri_file;
use je_workbench::browser::parts::editor::breadcrumbs::{BreadcrumbsService, IBreadcrumbsService};
use je_workbench::browser::parts::editor::breadcrumbs_control::{
    BreadcrumbsControl, EditorGroupTarget, ItemRect, compute_picker_placement, editor_group_target, is_payload_pick, payload_pick, payload_reveal, payload_reveal_aside,
    register_breadcrumbs_commands,
};
use je_workbench::common::editor::GroupIdentifier;
use je_workbench::services::editor::editor_groups_service::{ACTIVE_GROUP, SIDE_GROUP};

/// `for (const input of ['mouse', 'keyboard'] as const)`.
#[derive(Clone, Copy, PartialEq)]
enum PickerInput {
    Mouse,
    Keyboard,
}

/// `for (const delivery of ['before', 'after'] as const)`.
#[derive(Clone, Copy, PartialEq)]
enum Delivery {
    Before,
    After,
}

/// O corpo de `file picker keeps focus with ${input} input and editor focus delivery ${delivery} loading`.
fn file_picker_keeps_focus(input: PickerInput, delivery: Delivery) {
    run_case(Timers::Real, move |h| async move {
        h.create_control(false, false).await;
        match input {
            PickerInput::Mouse => h.click_last_item(),
            PickerInput::Keyboard => h.select_last(payload_pick()),
        }

        if delivery == Delivery::Before {
            h.deliver_editor_focus();
        }
        h.complete_file_read().await;
        h.wait_for_picker().await;
        if delivery == Delivery::After {
            h.deliver_editor_focus();
        }
        timeout(0).await.unwrap();
        h.assert_picker_focused();
    });
}

#[test]
fn file_picker_keeps_focus_with_mouse_input_and_editor_focus_delivery_before_loading() {
    file_picker_keeps_focus(PickerInput::Mouse, Delivery::Before);
}

#[test]
fn file_picker_keeps_focus_with_mouse_input_and_editor_focus_delivery_after_loading() {
    file_picker_keeps_focus(PickerInput::Mouse, Delivery::After);
}

#[test]
fn file_picker_keeps_focus_with_keyboard_input_and_editor_focus_delivery_before_loading() {
    file_picker_keeps_focus(PickerInput::Keyboard, Delivery::Before);
}

#[test]
fn file_picker_keeps_focus_with_keyboard_input_and_editor_focus_delivery_after_loading() {
    file_picker_keeps_focus(PickerInput::Keyboard, Delivery::After);
}

#[test]
fn outline_picker_activates_its_group_without_focusing_the_editor() {
    run_case(Timers::Real, |h| async move {
        h.create_control(true, false).await;
        h.select_last(payload_pick());
        h.wait_for_picker().await;
        h.deliver_editor_focus();
        timeout(0).await.unwrap();
        h.assert_picker_focused();
    });
}

/// `viewEvents: withOutline ? ['preview', 'clearPreview', ...(altKey ? ['restore'] : [])] : []`.
fn expected_view_events(with_outline: bool, alt_key: bool) -> Vec<String> {
    if !with_outline {
        return Vec::new();
    }
    let mut events = vec!["preview".to_owned(), "clearPreview".to_owned()];
    if alt_key {
        events.push("restore".to_owned());
    }
    events
}

/// O corpo de `${altKey ? 'Alt-clicking' : 'clicking'} a ${withOutline ? 'symbol' : 'file'} in the picker`.
fn clicking_in_the_picker(with_outline: bool, alt_key: bool) {
    run_case(Timers::Real, move |h| async move {
        h.create_control(with_outline, false).await;
        h.select_last(payload_pick());
        h.complete_file_read().await;
        h.wait_for_picker().await;
        let row = h.list_row().expect("o seletor tem uma linha");
        h.click(row, alt_key);
        timeout(0).await.unwrap();
        // `destination: withOutline ? outlineReveals : openEditor.firstCall?.args[1]`
        let destination = if with_outline {
            Destination::Reveals(h.outline_reveals())
        } else {
            Destination::Group(h.opened_editors().first().and_then(|opened| opened.group))
        };
        let picker_present = h.picker_element().is_some();
        let expected_destination = if with_outline { Destination::Reveals(vec![alt_key]) } else { Destination::Group(alt_key.then_some(SIDE_GROUP)) };
        assert_eq!(
            (h.active_group_id(), destination, h.outline_view_events(), picker_present),
            (GROUP_ID, expected_destination, expected_view_events(with_outline, alt_key), false)
        );
    });
}

/// `reveal: withOutline ? outlineReveals : openEditor.firstCall?.args`.
#[derive(Debug, PartialEq)]
enum Reveal {
    /// `outlineReveals`.
    Outline(Vec<bool>),
    /// `[{ resource, options }, group]` da primeira chamada de `openEditor`, ou `undefined` sem chamada.
    Opened(Option<(OpenedEditorView, Option<GroupIdentifier>)>),
}

/// O corpo de `picking a ${withOutline ? 'symbol' : 'file'} closes the picker and reveals the selection`.
fn picking_closes_the_picker(with_outline: bool) {
    run_case(Timers::Real, move |h| async move {
        h.create_control(with_outline, false).await;
        h.select_last(payload_pick());
        h.complete_file_read().await;
        h.wait_for_picker().await;
        timeout(0).await.unwrap();
        let row = h.list_row().expect("o seletor tem uma linha");
        h.double_click(row);
        timeout(0).await.unwrap();
        let expected_reveal = if with_outline {
            Reveal::Outline(vec![false])
        } else {
            Reveal::Opened(Some((
                OpenedEditorView {
                    resource: Some(h.resource().to_string(false).to_string_lossy()),
                    preserve_focus: Some(false),
                    pinned: Some(true),
                    reveal_if_visible: Some(true),
                    other_options_set: false,
                },
                None,
            )))
        };
        let reveal = if with_outline { Reveal::Outline(h.outline_reveals()) } else { Reveal::Opened(h.opened_editors().first().map(|opened| (opened.view(), opened.group))) };
        let picker_present = h.picker_element().is_some();
        assert_eq!((h.active_group_id(), reveal, picker_present), (GROUP_ID, expected_reveal, false));
    });
}

/// O corpo de `${withOutline ? 'symbol' : 'file'} quick access activates its group without focusing the editor`.
fn quick_access_activates_its_group(with_outline: bool) {
    run_case(Timers::Real, move |h| async move {
        h.use_quick_pick().await;
        h.create_control(with_outline, false).await;
        h.select_last(payload_pick());
        let prefix = if with_outline { "@" } else { "" };
        assert_eq!((h.active_group_id(), h.editor_focus_calls(), h.quick_access_prefixes()), (GROUP_ID, 0, vec![prefix.to_owned()]));
    });
}

/// O corpo de `revealing a ${withOutline ? 'symbol' : 'file'} ${sideBySide ? 'to the side' : 'in place'} still focuses the editor`.
fn revealing_still_focuses_the_editor(with_outline: bool, side_by_side: bool) {
    run_case(Timers::Real, move |h| async move {
        h.create_control(with_outline, false).await;
        h.select_last(if side_by_side { payload_reveal_aside() } else { payload_reveal() });
        // `destination: withOutline ? outlineReveals : openEditor.lastCall.args[1]`
        let destination = if with_outline {
            Destination::Reveals(h.outline_reveals())
        } else {
            Destination::Group(h.opened_editors().last().and_then(|opened| opened.group))
        };
        let expected_destination = if with_outline {
            Destination::Reveals(vec![side_by_side])
        } else {
            Destination::Group(Some(if side_by_side { SIDE_GROUP } else { ACTIVE_GROUP }))
        };
        assert_eq!((h.active_group_id(), h.editor_focus_calls(), destination), (GROUP_ID, 1, expected_destination));
    });
}

//#region withOutline: false (arquivo)

#[test]
fn clicking_a_file_in_the_picker() {
    clicking_in_the_picker(false, false);
}

#[test]
fn alt_clicking_a_file_in_the_picker() {
    clicking_in_the_picker(false, true);
}

#[test]
fn picking_a_file_closes_the_picker_and_reveals_the_selection() {
    picking_closes_the_picker(false);
}

#[test]
fn file_quick_access_activates_its_group_without_focusing_the_editor() {
    quick_access_activates_its_group(false);
}

#[test]
fn revealing_a_file_in_place_still_focuses_the_editor() {
    revealing_still_focuses_the_editor(false, false);
}

#[test]
fn revealing_a_file_to_the_side_still_focuses_the_editor() {
    revealing_still_focuses_the_editor(false, true);
}

//#endregion

//#region withOutline: true (símbolo)

#[test]
fn clicking_a_symbol_in_the_picker() {
    clicking_in_the_picker(true, false);
}

#[test]
fn alt_clicking_a_symbol_in_the_picker() {
    clicking_in_the_picker(true, true);
}

#[test]
fn picking_a_symbol_closes_the_picker_and_reveals_the_selection() {
    picking_closes_the_picker(true);
}

#[test]
fn symbol_quick_access_activates_its_group_without_focusing_the_editor() {
    quick_access_activates_its_group(true);
}

#[test]
fn revealing_a_symbol_in_place_still_focuses_the_editor() {
    revealing_still_focuses_the_editor(true, false);
}

#[test]
fn revealing_a_symbol_to_the_side_still_focuses_the_editor() {
    revealing_still_focuses_the_editor(true, true);
}

//#endregion

#[test]
fn revealing_a_folder_opens_the_next_picker_without_focusing_the_editor() {
    run_case(Timers::Real, |h| async move {
        h.create_control(false, false).await;
        let folder = h.items()[0].clone();
        h.widget().set_selection(Some(&folder), Some(payload_reveal()));
        h.complete_file_read().await;
        h.wait_for_picker().await;
        h.deliver_editor_focus();
        timeout(0).await.unwrap();
        h.assert_picker_focused();
    });
}

#[test]
fn alt_clicking_a_folder_only_expands_the_hierarchy() {
    run_case(Timers::Real, |h| async move {
        let resource = h.resource();
        let folder = uri_file("/path/folder");
        // `stub(instantiationService.get(IFileService), 'resolve').callsFake(async uri => createFileStat(uri, false, false, true, false,
        // uri.path === folder.path ? [{ resource }] : [{ resource: folder, isFile: false, isDirectory: true }]))`
        h.workbench.file_service.stub_resolve({
            let folder = folder.clone();
            move |uri| {
                let children = if uri.path().to_string_lossy() == folder.path().to_string_lossy() { vec![file_child(&resource)] } else { vec![folder_child(&folder)] };
                Promise::resolved(directory_stat(uri, children))
            }
        });
        h.create_control(false, false).await;
        h.select_last(payload_pick());
        h.wait_for_picker().await;
        let row = h.list_row().expect("o seletor tem uma linha");
        h.click(row, true);
        timeout(0).await.unwrap();
        assert_eq!((h.aria_expanded(row), h.opened_editors().len(), h.picker_element().is_some()), (Some("true"), 0, true));
    });
}

#[test]
fn alt_clicking_a_symbol_twistie_only_expands_the_hierarchy() {
    run_case(Timers::Real, |h| async move {
        h.create_control(true, true).await;
        h.select_last(payload_pick());
        h.wait_for_picker().await;
        let row = h.list_row().expect("o seletor tem uma linha");
        let twistie = h.query_selector(row, "monaco-tl-twistie").expect("a linha tem o twistie");
        h.click(twistie, true);
        timeout(0).await.unwrap();
        assert_eq!((h.aria_expanded(row), h.outline_reveals(), h.picker_element().is_some()), (Some("true"), Vec::<bool>::new(), true));
    });
}

#[test]
fn escape_returns_focus_to_the_owning_editor() {
    run_case(Timers::Real, |h| async move {
        // O original registra os comandos ao importar o módulo.
        h.guard.add(register_breadcrumbs_commands());
        h.create_control(false, false).await;
        h.select_last(payload_pick());
        h.complete_file_read().await;
        h.wait_for_picker().await;
        h.invoke_command("breadcrumbs.selectEditor");
        timeout(0).await.unwrap();
        assert_eq!(
            (h.active_group_id(), h.editor_focus_calls(), h.editor_focused(), h.picker_element().is_some()),
            (GROUP_ID, 1, true, false)
        );
    });
}

#[test]
fn moving_focus_outside_still_dismisses_the_picker() {
    run_case(Timers::Real, |h| async move {
        h.create_control(false, false).await;
        h.select_last(payload_pick());
        h.complete_file_read().await;
        h.wait_for_picker().await;
        h.focus_editor();
        timeout(0).await.unwrap();
        assert!(h.picker_element().is_none());
    });
}

#[test]
fn repeatedly_opening_the_picker_in_the_active_group_does_not_focus_the_editor() {
    run_case(Timers::Faked, |h| async move {
        h.create_control(false, false).await;
        // `activeGroup = group`
        h.activate_group(GROUP_ID);
        h.complete_file_read().await;
        for _ in 0..10 {
            h.click_last_item();
            h.wait_for_picker().await;
            h.deliver_editor_focus();
            timeout(0).await.unwrap();
            h.assert_picker_focused();
            h.focus_editor();
            timeout(0).await.unwrap();
        }
    });
}

//#region Contas e marcadores do porte (sem equivalente no original)

#[test]
fn payloads_map_to_editor_groups() {
    assert_eq!(editor_group_target(Some(&payload_reveal_aside())), Some(EditorGroupTarget::Side));
    assert_eq!(editor_group_target(Some(&payload_reveal())), Some(EditorGroupTarget::Active));
    assert_eq!(editor_group_target(Some(&payload_pick())), None);
    assert_eq!(editor_group_target(None), None);
    assert!(is_payload_pick(Some(&payload_pick())));
    assert!(!is_payload_pick(Some(&payload_reveal())));
    assert!(!is_payload_pick(None));
}

#[test]
fn scrollbar_tables() {
    assert_eq!(BreadcrumbsControl::HEIGHT, 22.0);
    assert_eq!(BreadcrumbsControl::scrollbar_size("default"), Some(3.0));
    assert_eq!(BreadcrumbsControl::scrollbar_size("large"), Some(8.0));
    assert_eq!(BreadcrumbsControl::scrollbar_size("x"), None);
    assert_eq!(BreadcrumbsControl::scrollbar_visibility("auto"), Some(ScrollbarVisibility::Auto));
    assert_eq!(BreadcrumbsControl::scrollbar_visibility("visible"), Some(ScrollbarVisibility::Visible));
    assert_eq!(BreadcrumbsControl::scrollbar_visibility("hidden"), Some(ScrollbarVisibility::Hidden));
}

#[test]
fn context_keys_have_the_original_names() {
    assert_eq!(BreadcrumbsControl::ck_breadcrumbs_possible().key(), "breadcrumbsPossible");
    assert_eq!(BreadcrumbsControl::ck_breadcrumbs_visible().key(), "breadcrumbsVisible");
    assert_eq!(BreadcrumbsControl::ck_breadcrumbs_active().key(), "breadcrumbsActive");
    assert_eq!(BreadcrumbsControl::ck_breadcrumbs_has_symbols().key(), "breadcrumbsHasSymbols");
}

#[test]
fn picker_placement_keyboard() {
    // janela 1000x800; item em (100, 0) 80x22: seta a 30% da largura do item, sem corte de altura
    let placement = compute_picker_placement(1000.0, 800.0, ItemRect { left: 100.0, top: 0.0, width: 80.0, height: 22.0 }, None);
    assert_eq!(placement.x, 100.0);
    assert_eq!(placement.y, 30.0);
    assert_eq!(placement.max_height, 300.0);
    // 992 / 4.17 = 237,89 < 240: o piso `Math.max(240, ...)` do original vence
    assert_eq!(placement.width, 240.0);
    assert_eq!(placement.arrow_size, 8.0);
    assert_eq!(placement.arrow_offset, 24.0);
}

#[test]
fn picker_placement_clamps_to_window_and_mouse_arrow() {
    // item colado na direita: x recua para caber; a altura encolhe perto da base
    let placement = compute_picker_placement(1000.0, 400.0, ItemRect { left: 950.0, top: 350.0, width: 40.0, height: 22.0 }, Some(960.0));
    let width = 240.0_f64.max(992.0 / 4.17);
    assert_eq!(placement.width, width);
    assert_eq!(placement.x, 992.0 - width);
    assert_eq!(placement.y, 380.0);
    assert_eq!(placement.max_height, 400.0 - 380.0 - 30.0);
    assert!(placement.arrow_offset <= width - 16.0);
    assert!(placement.arrow_offset >= 0.0);
}

#[test]
fn breadcrumbs_service_starts_empty() {
    let service = BreadcrumbsService::new();
    assert!(service.get_widget(1).is_none());
}

#[test]
fn commands_are_registered_with_the_original_ids() {
    let registrations = register_breadcrumbs_commands();
    for id in [
        "breadcrumbs.toggle",
        "breadcrumbs.focusAndSelect",
        "breadcrumbs.focus",
        "breadcrumbs.toggleToOn",
        "breadcrumbs.focusNext",
        "breadcrumbs.focusPrevious",
        "breadcrumbs.focusNextWithPicker",
        "breadcrumbs.focusPreviousWithPicker",
        "breadcrumbs.selectFocused",
        "breadcrumbs.revealFocused",
        "breadcrumbs.selectEditor",
        "breadcrumbs.revealFocusedFromTreeAside",
        "breadcrumbs.copyPath",
    ] {
        assert!(commands_registry().get_command(id).is_some(), "comando {id} não registrado");
    }
    registrations.dispose();
    assert!(commands_registry().get_command("breadcrumbs.selectEditor").is_none());
}

//#endregion
