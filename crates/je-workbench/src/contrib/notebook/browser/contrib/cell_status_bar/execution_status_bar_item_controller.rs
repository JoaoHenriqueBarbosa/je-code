//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/contrib/cellStatusBar/executionStatusBarItemController.ts`: só o formato
//! da duração de uma célula, `formatCellDuration`. O controlador (`NotebookStatusBarController` e os itens de status) depende do editor
//! e da barra de status de célula.
//!
//! Divergências: `showMilliseconds: boolean = true` não tem valor padrão em Rust, e quem o omitiria no original passa `true`. `number` é
//! `f64`.

/// `formatCellDuration(duration, showMilliseconds = true)`.
pub fn format_cell_duration(duration: f64, show_milliseconds: bool) -> String {
    if show_milliseconds && duration < 1000.0 {
        return format!("{duration}ms");
    }

    let minutes = (duration / 1000.0 / 60.0).floor();
    let seconds = (duration / 1000.0).floor() % 60.0;
    let tenths = ((duration % 1000.0) / 100.0).floor();

    if minutes > 0.0 { format!("{minutes}m {seconds}.{tenths}s") } else { format!("{seconds}.{tenths}s") }
}
