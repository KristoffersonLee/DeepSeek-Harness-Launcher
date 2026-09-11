//! DWM 标题栏 / 边框 / 文字着色，跟随 Harness 主题。
//!
//! v4 使用 4 个属性：`DWMWA_USE_IMMERSIVE_DARK_MODE(20)`、`DWMWA_BORDER_COLOR(34)`、
//! `DWMWA_CAPTION_COLOR(35)`、`DWMWA_TEXT_COLOR(36)`。此处全部保留。

use core::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
};

/// DWMWA_BORDER_COLOR 常量（Win11 22H2+，windows crate 可能未导出）。
const DWMWA_BORDER_COLOR: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(34);
/// DWMWA_TEXT_COLOR 常量（Win11 22H2+，windows crate 可能未导出）。
const DWMWA_TEXT_COLOR: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(36);

/// 设置深/浅色标题栏。
pub fn set_dark_mode(hwnd: HWND, dark: bool) -> windows::core::Result<()> {
    unsafe {
        let v: i32 = if dark { 1 } else { 0 };
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &v as *const _ as *const c_void,
            std::mem::size_of::<i32>() as u32,
        )
    }
}

/// 设置标题栏 / 边框 / 文字颜色（COLORREF 0x00BBGGRR）。
///
/// ## 为什么每一项都**尽力而为**（不再用 `?` 提前返回）
///
/// `DWMWA_CAPTION_COLOR` / `DWMWA_BORDER_COLOR` / `DWMWA_TEXT_COLOR` 需要
/// Windows 11 22H2+；在 Windows 10 上 `DwmSetWindowAttribute` 返回 `E_INVALIDARG`。
/// 旧实现用 `?` 传播第一个错误 → 后面的属性**一个都不再尝试**，
/// 而且所有调用方都丢弃了返回值，于是这条降级路径完全静默。
/// 现在逐项尝试、逐项忽略不支持的错误：能生效的生效，不能生效的安静跳过。
pub fn set_titlebar_colors(
    hwnd: HWND,
    color_rgb: (u8, u8, u8),
    dark: bool,
) -> windows::core::Result<()> {
    unsafe {
        let colorref: u32 =
            (color_rgb.2 as u32) << 16 | (color_rgb.1 as u32) << 8 | color_rgb.0 as u32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &colorref as *const _ as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &colorref as *const _ as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );
        let text: u32 = if dark { 0x00FFFFFF } else { 0x00000000 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            &text as *const _ as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );
        Ok(())
    }
}

/// 判断颜色是否为深色（亮度 < 128）。
pub fn is_dark_color(r: u8, g: u8, b: u8) -> bool {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) < 128.0
}

/// 系统当前是否为「深色应用模式」（读取 `AppsUseLightTheme`）。
///
/// 用途：窗口**刚创建、页面还没加载完**时给标题栏一个正确初值——否则深色系统下
/// 会先闪一下浅色标题栏（旧实现固定传 `false`）。页面加载完成后由异步主题采样
/// 覆盖为页面真实背景色。
///
/// 读不到值时按浅色处理（保守，且是 Windows 的默认值）。
pub fn is_system_dark() -> bool {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let name: Vec<u16> = "AppsUseLightTheme"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut value: u32 = 1;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(subkey.as_ptr()),
            windows::core::PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        );
        // 注意：该值的语义是 "LightTheme"，0 表示深色
        status.is_ok() && value == 0
    }
}
