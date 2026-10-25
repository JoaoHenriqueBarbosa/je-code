//! Teste de `breadcrumbsControl.ts`. Os 12 casos de `breadcrumbsControl.test.ts` (`file picker keeps focus with {mouse,
//! keyboard} input and editor focus delivery {before,after} loading`, `outline picker activates its group without focusing
//! the editor`, `{Alt-clicking,clicking} a {symbol,file} in the picker`, `picking a {symbol,file} closes the picker and
//! reveals the selection`, `{symbol,file} quick access activates its group without focusing the editor`, `revealing a
//! {symbol,file} {to the side,in place} still focuses the editor`, `revealing a folder opens the next picker without
//! focusing the editor`, `Alt-clicking a folder only expands the hierarchy`, `Alt-clicking a symbol twistie only expands
//! the hierarchy`, `Escape returns focus to the owning editor`, `moving focus outside still dismisses the picker` e
//! `repeatedly opening the picker in the active group does not focus the editor`) dirigem o controle sobre o
//! `workbenchInstantiationService` (cerca de quinze serviços reais: `IContextViewService`, `IFileService` com `resolve`
//! substituído, `IConfigurationService`, `IEditorService`, `IThemeService`, `IDecorationsService`, `ITextFileService`...)
//! e disparam `MouseEvent` de clique, de clique duplo e de Alt em linhas das árvores dos seletores, além de `FocusEvent`
//! de foco e perda. O porte ainda não tem esse harness (`workbench_instantiation_service`, o `IContextViewService` sobre
//! `NodeId` sem implementação de menu, e a injeção de `PointerData` com `altKey` em linhas de árvore do `je-ui`), então os
//! 12 casos entram quando ele existir; nenhum foi excluído do oráculo.
//!
//! Aqui ficam as contas e os marcadores que esses casos exercitam (payloads, destino de grupo, geometria do picker), o serviço
//! `BreadcrumbsService`, o registro dos comandos (`CommandsRegistry.getCommand('breadcrumbs.selectEditor')` do caso
//! `Escape returns focus to the owning editor`) e o rótulo do seletor de tipo de editor.

use je_base::lifecycle::IDisposable;
use je_base::scrollable::ScrollbarVisibility;
use je_platform::commands::commands_registry;
use je_workbench::browser::parts::editor::breadcrumbs::{BreadcrumbsService, IBreadcrumbsService};
use je_workbench::browser::parts::editor::breadcrumbs_control::{
    BreadcrumbsControl, EditorGroupTarget, register_breadcrumbs_commands, ItemRect, compute_picker_placement, editor_group_target, is_payload_pick, payload_pick, payload_reveal,
    payload_reveal_aside,
};

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
