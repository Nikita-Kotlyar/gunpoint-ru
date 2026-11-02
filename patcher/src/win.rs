//! Small Win32 layer: registry, folder picker, message box, atomic replace.
//! No external crates; only system DLLs (advapi32, shell32, ole32, user32, kernel32).

use std::ffi::c_void;
use std::path::{Path, PathBuf};

pub const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as isize;
pub const HKEY_LOCAL_MACHINE: isize = 0x8000_0002u32 as isize;
// REG_SZ or REG_EXPAND_SZ
const RRF_RT_REG_SZ: u32 = 0x0000_0002 | 0x0000_0004;

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegGetValueW(
        hkey: isize,
        lp_sub_key: *const u16,
        lp_value: *const u16,
        dw_flags: u32,
        pdw_type: *mut u32,
        pv_data: *mut c_void,
        pcb_data: *mut u32,
    ) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetConsoleTitleW(title: *const u16) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleOutputCP(cp: u32) -> i32;
    fn SetConsoleCP(cp: u32) -> i32;
    fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
}

#[repr(C)]
struct BrowseInfoW {
    hwnd_owner: isize,
    pidl_root: *const c_void,
    psz_display_name: *mut u16,
    lpsz_title: *const u16,
    ul_flags: u32,
    lpfn: Option<unsafe extern "system" fn(isize, u32, isize, isize) -> i32>,
    l_param: isize,
    i_image: i32,
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHBrowseForFolderW(lpbi: *mut BrowseInfoW) -> *mut c_void;
    fn SHGetPathFromIDListW(pidl: *const c_void, psz_path: *mut u16) -> i32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(pv_reserved: *const c_void, dw_co_init: u32) -> i32;
    fn CoTaskMemFree(pv: *const c_void);
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn init_console() {
    unsafe {
        SetConsoleOutputCP(65001);
        SetConsoleCP(65001);
        SetConsoleTitleW(wide("Gunpoint RU Patcher").as_ptr());
    }
}

pub fn reg_sz(root: isize, subkey: &str, value: &str) -> Option<String> {
    let sub = wide(subkey);
    let val = wide(value);
    let mut size: u32 = 0;
    let rc = unsafe {
        RegGetValueW(
            root,
            sub.as_ptr(),
            val.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if rc != 0 || size < 2 {
        return None;
    }
    let mut buf = vec![0u16; (size as usize + 1) / 2];
    let mut ty: u32 = 0;
    let rc = unsafe {
        RegGetValueW(
            root,
            sub.as_ptr(),
            val.as_ptr(),
            RRF_RT_REG_SZ,
            &mut ty,
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };
    if rc != 0 {
        return None;
    }
    while buf.last() == Some(&0) {
        buf.pop();
    }
    Some(String::from_utf16_lossy(&buf))
}

pub fn pick_folder(title: &str) -> Option<PathBuf> {
    unsafe {
        // RPC_E_CHANGED_MODE is harmless; the folder dialog still works.
        let _ = CoInitializeEx(std::ptr::null(), 0x2); // COINIT_APARTMENTTHREADED
        let mut display = [0u16; 260];
        let title_w = wide(title);
        let mut bi = BrowseInfoW {
            hwnd_owner: 0,
            pidl_root: std::ptr::null(),
            psz_display_name: display.as_mut_ptr(),
            lpsz_title: title_w.as_ptr(),
            // BIF_RETURNONLYFSDIRS | BIF_EDITBOX | BIF_NEWDIALOGSTYLE
            ul_flags: 0x1 | 0x10 | 0x40,
            lpfn: None,
            l_param: 0,
            i_image: 0,
        };
        let pidl = SHBrowseForFolderW(&mut bi);
        if pidl.is_null() {
            return None;
        }
        let mut path = [0u16; 260];
        let ok = SHGetPathFromIDListW(pidl, path.as_mut_ptr());
        CoTaskMemFree(pidl);
        if ok == 0 {
            return None;
        }
        let n = path.iter().position(|&c| c == 0).unwrap_or(path.len());
        Some(PathBuf::from(String::from_utf16_lossy(&path[..n])))
    }
}

/// Atomic-ish replace on Windows: MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH.
pub fn move_replace(src: &Path, dst: &Path) -> Result<(), String> {
    let s = wide(&src.to_string_lossy());
    let d = wide(&dst.to_string_lossy());
    let rc = unsafe { MoveFileExW(s.as_ptr(), d.as_ptr(), 0x1 | 0x8) };
    if rc == 0 {
        // Fall back to remove + rename for exotic filesystems.
        let err = std::io::Error::last_os_error();
        std::fs::remove_file(dst).map_err(|e| {
            format!(
                "не удалось заменить {}: {} (MoveFileEx: {})",
                dst.display(),
                e,
                err
            )
        })?;
        std::fs::rename(src, dst)
            .map_err(|e| format!("не удалось переименовать {}: {}", dst.display(), e))?;
    }
    Ok(())
}
