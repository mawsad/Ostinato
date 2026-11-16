use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use std::sync::atomic::{AtomicU64, Ordering};
use tray_icon::{
    menu::{Menu, MenuId, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

pub fn build_ostinato_tray() -> (Option<TrayIcon>, MenuId, MenuId) {
    let menu = Menu::new();
    let item_show = MenuItem::new("Развернуть Ostinato (Wake)", true, None);
    let item_quit = MenuItem::new("Выйти полностью", true, None);
    let show_id = item_show.id().clone();
    let quit_id = item_quit.id().clone();
    let _ = menu.append(&item_show);
    let _ = menu.append(&item_quit);

    let mut rgba = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let idx = (y * 32 + x) * 4;
            let dx = x as f32 - 15.5;
            let dy = y as f32 - 15.5;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= 14.5 {
                if (dist - 10.0).abs() <= 2.3 || dist <= 3.2 {
                    rgba[idx] = 0;
                    rgba[idx + 1] = 135;
                    rgba[idx + 2] = 250;
                    rgba[idx + 3] = 255;
                } else {
                    rgba[idx] = 4;
                    rgba[idx + 1] = 7;
                    rgba[idx + 2] = 14;
                    rgba[idx + 3] = 255;
                }
            }
        }
    }

    let icon = Icon::from_rgba(rgba, 32, 32).ok();
    let mut builder = TrayIconBuilder::new()
        .with_tooltip("Ostinato v3 [Sleep] — ЛКМ: Развернуть | ПКМ: Меню")
        .with_menu(Box::new(menu));
    if let Some(ic) = icon {
        builder = builder.with_icon(ic);
    }

    (builder.build().ok(), show_id, quit_id)
}

static MAIN_HWND: AtomicU64 = AtomicU64::new(0);

#[cfg(windows)]
unsafe extern "system" fn enum_win_proc(
    hwnd: *mut std::ffi::c_void,
    lparam: isize,
) -> i32 {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    };

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == lparam as u32 && IsWindowVisible(hwnd) != 0 {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) != 0 {
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if width > 300 && height > 200 {
                MAIN_HWND.store(hwnd as usize as u64, Ordering::Relaxed);
                return 0;
            }
        }
    }
    1
}

pub fn capture_own_hwnd() {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        unsafe {
            let hwnd = GetForegroundWindow();
            if !hwnd.is_null() {
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid == std::process::id() {
                    MAIN_HWND.store(hwnd as usize as u64, Ordering::SeqCst);
                }
            }
        }
    }
}


pub fn force_hide_window_win32() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
            SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE,
            WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
        };
        if MAIN_HWND.load(Ordering::Relaxed) == 0 {
            capture_own_hwnd();
        }
        let raw = MAIN_HWND.load(Ordering::Relaxed);
        if raw != 0 {
            let hwnd = raw as usize as *mut std::ffi::c_void;
            ShowWindow(hwnd, SW_HIDE);
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let new_ex = (ex & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW;
            SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex as i32);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                -32000,
                -32000,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

pub fn force_wake_window_win32() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow,
            GWL_EXSTYLE, SWP_NOSIZE, SWP_NOZORDER, SW_RESTORE, SW_SHOW, WS_EX_APPWINDOW,
            WS_EX_TOOLWINDOW,
        };
        let raw = MAIN_HWND.load(Ordering::Relaxed);
        if raw != 0 {
            let hwnd = raw as usize as *mut std::ffi::c_void;
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let new_ex = (ex & !WS_EX_TOOLWINDOW) | WS_EX_APPWINDOW;
            SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex as i32);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                180,
                140,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
            ShowWindow(hwnd, SW_SHOW);
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
}

pub fn set_process_eco_qos(enable: bool) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
            BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
            PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            PROCESS_POWER_THROTTLING_STATE,
        };

        let handle = GetCurrentProcess();
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if enable {
                PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            } else {
                0
            },
        };

        let _ = SetProcessInformation(
            handle,
            ProcessPowerThrottling,
            &state as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );

        let prio = if enable {
            BELOW_NORMAL_PRIORITY_CLASS
        } else {
            NORMAL_PRIORITY_CLASS
        };
        SetPriorityClass(handle, prio);
    }
}

pub fn deep_trim_memory() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Memory::SetProcessWorkingSetSizeEx;
        use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let handle = GetCurrentProcess();
        let _ = SetProcessWorkingSetSizeEx(handle, usize::MAX, usize::MAX, 0);
        K32EmptyWorkingSet(handle);
    }
}
pub enum KeyCapture {
    None,
    Cancelled,
    Cleared,
    Combo(String),
}

pub fn capture_key_combo(ctx: &eframe::egui::Context) -> KeyCapture {
    use eframe::egui;
    ctx.input(|i| {
        for event in &i.events {
            if let egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                if *key == egui::Key::Escape {
                    return KeyCapture::Cancelled;
                }
                if !modifiers.ctrl
                    && !modifiers.alt
                    && !modifiers.shift
                    && matches!(*key, egui::Key::Delete | egui::Key::Backspace)
                {
                    return KeyCapture::Cleared;
                }

                let mut parts = Vec::new();
                if modifiers.ctrl {
                    parts.push("Ctrl".to_string());
                }
                if modifiers.alt {
                    parts.push("Alt".to_string());
                }
                if modifiers.shift {
                    parts.push("Shift".to_string());
                }

                let is_numpad = physical_key.is_none()
                    || matches!(
                        (*key, physical_key),
                        (egui::Key::Insert, Some(egui::Key::Num0))
                            | (egui::Key::End, Some(egui::Key::Num1))
                            | (egui::Key::ArrowDown, Some(egui::Key::Num2))
                            | (egui::Key::PageDown, Some(egui::Key::Num3))
                            | (egui::Key::ArrowLeft, Some(egui::Key::Num4))
                            | (egui::Key::ArrowRight, Some(egui::Key::Num6))
                            | (egui::Key::Home, Some(egui::Key::Num7))
                            | (egui::Key::ArrowUp, Some(egui::Key::Num8))
                            | (egui::Key::PageUp, Some(egui::Key::Num9))
                    );

                let chosen = physical_key.unwrap_or(*key);
                let name = match chosen {
                    egui::Key::Num0 => if is_numpad { "Numpad0" } else { "Digit0" }.to_string(),
                    egui::Key::Num1 => if is_numpad { "Numpad1" } else { "Digit1" }.to_string(),
                    egui::Key::Num2 => if is_numpad { "Numpad2" } else { "Digit2" }.to_string(),
                    egui::Key::Num3 => if is_numpad { "Numpad3" } else { "Digit3" }.to_string(),
                    egui::Key::Num4 => if is_numpad { "Numpad4" } else { "Digit4" }.to_string(),
                    egui::Key::Num5 => if is_numpad { "Numpad5" } else { "Digit5" }.to_string(),
                    egui::Key::Num6 => if is_numpad { "Numpad6" } else { "Digit6" }.to_string(),
                    egui::Key::Num7 => if is_numpad { "Numpad7" } else { "Digit7" }.to_string(),
                    egui::Key::Num8 => if is_numpad { "Numpad8" } else { "Digit8" }.to_string(),
                    egui::Key::Num9 => if is_numpad { "Numpad9" } else { "Digit9" }.to_string(),
                    other => format!("{:?}", other),
                };

                parts.push(name);
                return KeyCapture::Combo(parts.join("+"));
            }
        }
        KeyCapture::None
    })
}

pub fn parse_hotkey_str(combo: &str) -> Option<HotKey> {
    let mut mods = Modifiers::empty();
    let mut key_code: Option<Code> = None;

    for raw in combo.split('+') {
        let p = raw.trim();
        if p.is_empty() {
            continue;
        }
        match p {
            "Ctrl" => mods |= Modifiers::CONTROL,
            "Alt" => mods |= Modifiers::ALT,
            "Shift" => mods |= Modifiers::SHIFT,
            "Space" => key_code = Some(Code::Space),
            "ArrowRight" | "Right" => key_code = Some(Code::ArrowRight),
            "ArrowLeft" | "Left" => key_code = Some(Code::ArrowLeft),
            "ArrowUp" | "Up" => key_code = Some(Code::ArrowUp),
            "ArrowDown" | "Down" => key_code = Some(Code::ArrowDown),
            "Enter" => key_code = Some(Code::Enter),
            "Tab" => key_code = Some(Code::Tab),
            "Backspace" => key_code = Some(Code::Backspace),
            "Insert" => key_code = Some(Code::Insert),
            "Delete" => key_code = Some(Code::Delete),
            "Home" => key_code = Some(Code::Home),
            "End" => key_code = Some(Code::End),
            "PageUp" => key_code = Some(Code::PageUp),
            "PageDown" => key_code = Some(Code::PageDown),
            "Minus" => key_code = Some(Code::Minus),
            "Equals" | "Plus" => key_code = Some(Code::Equal),
            "Digit0" | "0" => key_code = Some(Code::Digit0),
            "Digit1" | "1" => key_code = Some(Code::Digit1),
            "Digit2" | "2" => key_code = Some(Code::Digit2),
            "Digit3" | "3" => key_code = Some(Code::Digit3),
            "Digit4" | "4" => key_code = Some(Code::Digit4),
            "Digit5" | "5" => key_code = Some(Code::Digit5),
            "Digit6" | "6" => key_code = Some(Code::Digit6),
            "Digit7" | "7" => key_code = Some(Code::Digit7),
            "Digit8" | "8" => key_code = Some(Code::Digit8),
            "Digit9" | "9" => key_code = Some(Code::Digit9),
            "Numpad0" | "Num0" => key_code = Some(Code::Numpad0),
            "Numpad1" | "Num1" => key_code = Some(Code::Numpad1),
            "Numpad2" | "Num2" => key_code = Some(Code::Numpad2),
            "Numpad3" | "Num3" => key_code = Some(Code::Numpad3),
            "Numpad4" | "Num4" => key_code = Some(Code::Numpad4),
            "Numpad5" | "Num5" => key_code = Some(Code::Numpad5),
            "Numpad6" | "Num6" => key_code = Some(Code::Numpad6),
            "Numpad7" | "Num7" => key_code = Some(Code::Numpad7),
            "Numpad8" | "Num8" => key_code = Some(Code::Numpad8),
            "Numpad9" | "Num9" => key_code = Some(Code::Numpad9),
            "F1" => key_code = Some(Code::F1),
            "F2" => key_code = Some(Code::F2),
            "F3" => key_code = Some(Code::F3),
            "F4" => key_code = Some(Code::F4),
            "F5" => key_code = Some(Code::F5),
            "F6" => key_code = Some(Code::F6),
            "F7" => key_code = Some(Code::F7),
            "F8" => key_code = Some(Code::F8),
            "F9" => key_code = Some(Code::F9),
            "F10" => key_code = Some(Code::F10),
            "F11" => key_code = Some(Code::F11),
            "F12" => key_code = Some(Code::F12),
            "A" | "KeyA" => key_code = Some(Code::KeyA),
            "B" | "KeyB" => key_code = Some(Code::KeyB),
            "C" | "KeyC" => key_code = Some(Code::KeyC),
            "D" | "KeyD" => key_code = Some(Code::KeyD),
            "E" | "KeyE" => key_code = Some(Code::KeyE),
            "F" | "KeyF" => key_code = Some(Code::KeyF),
            "G" | "KeyG" => key_code = Some(Code::KeyG),
            "H" | "KeyH" => key_code = Some(Code::KeyH),
            "I" | "KeyI" => key_code = Some(Code::KeyI),
            "J" | "KeyJ" => key_code = Some(Code::KeyJ),
            "K" | "KeyK" => key_code = Some(Code::KeyK),
            "L" | "KeyL" => key_code = Some(Code::KeyL),
            "M" | "KeyM" => key_code = Some(Code::KeyM),
            "N" | "KeyN" => key_code = Some(Code::KeyN),
            "O" | "KeyO" => key_code = Some(Code::KeyO),
            "P" | "KeyP" => key_code = Some(Code::KeyP),
            "Q" | "KeyQ" => key_code = Some(Code::KeyQ),
            "R" | "KeyR" => key_code = Some(Code::KeyR),
            "S" | "KeyS" => key_code = Some(Code::KeyS),
            "T" | "KeyT" => key_code = Some(Code::KeyT),
            "U" | "KeyU" => key_code = Some(Code::KeyU),
            "V" | "KeyV" => key_code = Some(Code::KeyV),
            "W" | "KeyW" => key_code = Some(Code::KeyW),
            "X" | "KeyX" => key_code = Some(Code::KeyX),
            "Y" | "KeyY" => key_code = Some(Code::KeyY),
            "Z" | "KeyZ" => key_code = Some(Code::KeyZ),
            _ => {}
        }
    }

    let code = key_code?;
    let m_opt = if mods.is_empty() { None } else { Some(mods) };
    Some(HotKey::new(m_opt, code))
}

pub fn minimize_window_win32() {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_MINIMIZE};
        let saved = MAIN_HWND.load(Ordering::SeqCst);
        if saved != 0 {
            unsafe {
                ShowWindow(saved as usize as *mut std::ffi::c_void, SW_MINIMIZE);
            }
        }
    }
}



