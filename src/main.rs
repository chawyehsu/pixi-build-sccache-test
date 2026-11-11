#[cfg(target_os = "windows")]
extern crate windows_sys as windows;

#[cfg(target_os = "windows")]
mod target_windows {

use std::ffi::c_void;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::slice;

use super::windows::Win32::UI::Shell;

pub fn known_folder(folder_id: windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let mut path_ptr: windows::core::PWSTR = std::ptr::null_mut();
        let result = Shell::SHGetKnownFolderPath(
            &folder_id,
            0,
            std::ptr::null_mut(),
            &mut path_ptr
        );
        if result == 0 {
            let len = windows::Win32::Globalization::lstrlenW(path_ptr) as usize;
            let path = slice::from_raw_parts(path_ptr, len);
            let ostr: OsString = OsStringExt::from_wide(path);
            windows::Win32::System::Com::CoTaskMemFree(path_ptr as *const c_void);
            Some(PathBuf::from(ostr))
        } else {
            println!("Failed to get known folder path: {}", result);
            windows::Win32::System::Com::CoTaskMemFree(path_ptr as *const c_void);
            None
        }
    }
}

pub fn known_folder_profile() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_Profile)
}

pub fn known_folder_roaming_app_data() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_RoamingAppData)
}

pub fn known_folder_local_app_data() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_LocalAppData)
}

pub fn known_folder_music() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_Music)
}

pub fn known_folder_desktop() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_Desktop)
}

pub fn known_folder_documents() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_Documents)
}

pub fn known_folder_downloads() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_Downloads)
}

pub fn known_folder_pictures() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_Pictures)
}

pub fn known_folder_public() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_Public)
}
pub fn known_folder_templates() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_Templates)
}
pub fn known_folder_videos() -> Option<PathBuf> {
    known_folder(Shell::FOLDERID_Videos)
}

}

fn main() {
    println!("Hello, world!");

    #[cfg(target_os = "windows")]
    {
        if let Some(path) = target_windows::known_folder_local_app_data() {
            println!("Local App Data folder: {:?}", path);
        } else {
            println!("Failed to get Local App Data folder");
        }

        if let Some(path) = target_windows::known_folder_roaming_app_data() {
            println!("Roaming App Data folder: {:?}", path);
        } else {
            println!("Failed to get Roaming App Data folder");
        }
    }
}
