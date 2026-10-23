//! Testes extras do `ProgressBar` (o original só tem o caso `Progress Bar`, em `progress_bar.rs`): o estado
//! de classes, a largura do `.progress-bit`, a animação `progress` por quadro e o atraso de exibição.

mod support;

use je_base::lifecycle::IDisposable;
use je_base::runtime;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role, Ui};
use je_widgets::dom::create_el;
use je_widgets::progress_bar::{IProgressBarOptions, IProgressBarStyles, ProgressBar};
use je_render::Rgba;
use support::ensure_no_disposables_are_leaked_in_test_suite;

/// Um contêiner de 400 px de largura sob a raiz.
fn fixture(ui: &Ui) -> NodeId {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    let fixture = create_el(&mut tree, root, Role::Generic, "");
    tree.set_bounds(fixture, Rect::new(0.0, 0.0, 400.0, 20.0));
    fixture
}

fn bit_rect(ui: &Ui, bar: &ProgressBar) -> Rect {
    ui.tree().bounds(bar.bit())
}

#[test]
fn starts_hidden_with_the_default_color_and_a_two_pixel_container() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let bar = ProgressBar::new(&ui, fixture(&ui), None);
    let tree = ui.tree();
    assert!(tree.has_class(bar.get_container(), "monaco-progress-container"));
    assert_eq!(tree.role(bar.get_container()), Role::ProgressBar);
    assert_eq!(tree.label(bar.get_container()), Some("Progress"));
    assert_eq!(tree.bounds(bar.get_container()).width(), 400.0);
    assert_eq!(tree.bounds(bar.get_container()).height(), 2.0);
    assert!(!tree.style(bar.bit()).visible, "sem `.active` o `.progress-bit` é `display: none`");
    assert_eq!(tree.style(bar.bit()).background, Some(Rgba::opaque(0x0E, 0x70, 0xC0)));
    drop(tree);
    bar.dispose();
}

#[test]
fn uses_the_aria_label_and_the_themed_color() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let color = Rgba::opaque(1, 2, 3);
    let options = IProgressBarOptions { styles: IProgressBarStyles { progress_bar_background: Some(color) }, aria_label: Some("  Carregando  ".to_owned()) };
    let bar = ProgressBar::new(&ui, fixture(&ui), Some(&options));
    assert_eq!(ui.tree().label(bar.get_container()), Some("Carregando"));
    assert_eq!(ui.tree().style(bar.bit()).background, Some(color));
    bar.dispose();
}

#[test]
fn worked_fills_a_share_of_the_total_and_aria_values_follow() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let bar = ProgressBar::new(&ui, fixture(&ui), None);
    assert!(!bar.has_total());
    bar.total(200.0);
    assert!(bar.has_total());
    assert_eq!(bar.aria_value_max(), Some(200.0));

    bar.worked(50.0);
    assert!(ui.tree().has_class(bar.get_container(), "active") && ui.tree().has_class(bar.get_container(), "discrete"));
    assert_eq!(bit_rect(&ui, &bar), Rect::new(0.0, 0.0, 100.0, 2.0), "50 de 200 são 25 % de 400 px");
    assert_eq!(bar.aria_value_now(), Some(50.0));

    bar.worked(0.0);
    assert_eq!(bit_rect(&ui, &bar).width(), 102.0, "`worked` soma no mínimo 1");
    bar.set_worked(500.0);
    assert_eq!(bit_rect(&ui, &bar).width(), 400.0, "limitado ao total");
    assert_eq!(bar.aria_value_now(), Some(500.0), "o `aria-valuenow` é o valor recebido");
    bar.dispose();
}

#[test]
fn without_total_the_scale_is_one_hundred() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let bar = ProgressBar::new(&ui, fixture(&ui), None);
    bar.set_worked(25.0);
    assert_eq!(bit_rect(&ui, &bar).width(), 100.0);
    bar.total(0.0);
    bar.set_worked(50.0);
    assert_eq!(bit_rect(&ui, &bar).width(), 200.0, "`totalWork || 100`: zero vale 100");
    bar.dispose();
}

#[test]
fn done_fills_the_bar_and_turns_it_off_after_200_ms() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let fixture = fixture(&ui);
    runtime::with_faked_timers(|| {
        let bar = ProgressBar::new(&ui, fixture, None);
        bar.set_worked(10.0);
        bar.done();
        assert!(ui.tree().has_class(bar.get_container(), "done"));
        assert_eq!(bit_rect(&ui, &bar).width(), 400.0);
        assert!(ui.tree().style(bar.bit()).visible);
        runtime::advance_fake_time(200);
        assert!(!ui.tree().has_class(bar.get_container(), "active"));
        assert!(!ui.tree().style(bar.bit()).visible);
        assert!(!bar.has_total());
        bar.dispose();
    });
}

#[test]
fn stop_turns_off_at_once() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let bar = ProgressBar::new(&ui, fixture(&ui), None);
    bar.set_worked(10.0);
    bar.stop();
    assert!(!ui.tree().has_class(bar.get_container(), "active"));
    assert!(!ui.tree().style(bar.bit()).visible);
    bar.dispose();
}

#[test]
fn infinite_follows_the_progress_keyframes_frame_by_frame() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let fixture = fixture(&ui);
    runtime::with_faked_timers(|| {
        let bar = ProgressBar::new(&ui, fixture, None);
        bar.infinite();
        assert!(ui.tree().has_class(bar.get_container(), "infinite") && ui.tree().has_class(bar.get_container(), "active"));
        // A fase 0: `translateX(0) scaleX(1)` sobre a largura de 2 % (8 px).
        assert_eq!(bit_rect(&ui, &bar), Rect::new(0.0, 0.0, 8.0, 2.0));
        assert!(ui.tree().style(bar.bit()).visible);

        // A fase 0,25 (1 s de 4 s): metade do primeiro trecho, `translateX(1250%) scaleX(2)`: centro em 4 + 100.
        runtime::advance_fake_time(1000);
        assert_eq!(ui.run_animation_frame(), 1);
        assert_eq!(bit_rect(&ui, &bar), Rect::new(96.0, 0.0, 112.0, 2.0));

        // A fase 0,5: `translateX(2500%) scaleX(3)`: centro em 4 + 200, largura 24.
        runtime::advance_fake_time(1000);
        ui.run_animation_frame();
        assert_eq!(bit_rect(&ui, &bar), Rect::new(192.0, 0.0, 216.0, 2.0));

        // A fase 0,75: de 2500 % a 4900 % e de 3 a 1: `translateX(3700%) scaleX(2)`: centro em 4 + 296 (300), de 292 a 308.
        runtime::advance_fake_time(1000);
        ui.run_animation_frame();
        assert_eq!(bit_rect(&ui, &bar), Rect::new(292.0, 0.0, 308.0, 2.0));

        // A volta ao começo (4 s).
        runtime::advance_fake_time(1000);
        ui.run_animation_frame();
        assert_eq!(bit_rect(&ui, &bar), Rect::new(0.0, 0.0, 8.0, 2.0));
        bar.dispose();
    });
}

#[test]
fn infinite_turns_into_long_running_steps_after_ten_seconds() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let fixture = fixture(&ui);
    runtime::with_faked_timers(|| {
        let bar = ProgressBar::new(&ui, fixture, None);
        bar.infinite();
        runtime::advance_fake_time(10_000);
        assert!(ui.tree().has_class(bar.get_container(), "infinite-long-running"));
        // 10 s é a fase 0,5 (2,5 ciclos): o `steps(100)` do trecho que acaba de começar.
        ui.run_animation_frame();
        assert_eq!(bit_rect(&ui, &bar), Rect::new(192.0, 0.0, 216.0, 2.0));
        // 10,01 s: 0,01 s depois, 0,5 % do segundo trecho (2 s) cai no primeiro degrau (0): quantizado.
        runtime::advance_fake_time(10);
        ui.run_animation_frame();
        assert_eq!(bit_rect(&ui, &bar), Rect::new(192.0, 0.0, 216.0, 2.0));
        bar.dispose();
    });
}

#[test]
fn done_on_an_infinite_bar_fades_it_out() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let fixture = fixture(&ui);
    runtime::with_faked_timers(|| {
        let bar = ProgressBar::new(&ui, fixture, None);
        bar.infinite();
        bar.done();
        assert_eq!(ui.tree().style(bar.bit()).opacity, 0.0);
        runtime::advance_fake_time(200);
        assert_eq!(ui.tree().style(bar.bit()).opacity, 1.0);
        assert!(!ui.tree().has_class(bar.get_container(), "infinite"));
        bar.dispose();
    });
}

#[test]
fn show_and_hide_toggle_the_container_with_an_optional_delay() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let fixture = fixture(&ui);
    runtime::with_faked_timers(|| {
        let bar = ProgressBar::new(&ui, fixture, None);
        bar.hide();
        assert!(!ui.tree().style(bar.get_container()).visible);
        bar.show(Some(300));
        assert!(!ui.tree().style(bar.get_container()).visible);
        runtime::advance_fake_time(299);
        assert!(!ui.tree().style(bar.get_container()).visible);
        runtime::advance_fake_time(1);
        assert!(ui.tree().style(bar.get_container()).visible);
        bar.hide();
        bar.show(None);
        assert!(ui.tree().style(bar.get_container()).visible);
        bar.dispose();
    });
}

#[test]
fn two_infinite_bars_animate_independently() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let fixture = fixture(&ui);
    runtime::with_faked_timers(|| {
        let (first, second) = (ProgressBar::new(&ui, fixture, None), ProgressBar::new(&ui, fixture, None));
        first.infinite();
        second.infinite();
        runtime::advance_fake_time(1000);
        assert_eq!(ui.run_animation_frame(), 2);
        assert_eq!(bit_rect(&ui, &first), Rect::new(96.0, 0.0, 112.0, 2.0));
        assert_eq!(bit_rect(&ui, &second), Rect::new(96.0, 0.0, 112.0, 2.0));
        first.dispose();
        runtime::advance_fake_time(1000);
        assert_eq!(ui.run_animation_frame(), 1, "a barra descartada não anima mais");
        assert_eq!(bit_rect(&ui, &first), Rect::new(96.0, 0.0, 112.0, 2.0));
        assert_eq!(bit_rect(&ui, &second), Rect::new(192.0, 0.0, 216.0, 2.0));
        second.dispose();
    });
}
