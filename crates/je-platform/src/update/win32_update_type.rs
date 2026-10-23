//! Porte de `src/vs/platform/update/electron-main/win32UpdateType.ts`: o tipo de atualização no Windows a
//! partir do `target` do produto. `string | undefined` é `Option<&str>`.

use crate::update::UpdateType;

pub fn is_inno_setup_install(target: Option<&str>) -> bool {
    matches!(target, Some("user" | "system"))
}

pub fn get_win32_update_type(target: Option<&str>) -> UpdateType {
    if is_inno_setup_install(target) { UpdateType::Setup } else { UpdateType::Archive }
}
