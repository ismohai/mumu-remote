use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
};

pub fn is_mumu_window_title(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    lower.contains("mumu")
}

pub struct WindowInfo {
    pub handle: HWND,
    pub title: String,
}

pub fn find_mumu_window() -> Option<WindowInfo> {
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let windows_ptr = lparam.0 as *mut Vec<WindowInfo>;
        if windows_ptr.is_null() {
            return BOOL(0);
        }
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let len = GetWindowTextLengthW(hwnd);
            if len == 0 {
                return BOOL(1);
            }
            let mut buffer: Vec<u16> = vec![0; (len + 1) as usize];
            let written = GetWindowTextW(hwnd, &mut buffer);
            if written == 0 {
                return BOOL(1);
            }
            let title = String::from_utf16_lossy(&buffer[..written as usize]);
            if is_mumu_window_title(&title) {
                (*windows_ptr).push(WindowInfo {
                    handle: hwnd,
                    title,
                });
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    let mut found = Vec::<WindowInfo>::new();
    unsafe {
        let lparam = LPARAM(&mut found as *mut _ as isize);
        let _ = EnumWindows(Some(enum_proc), lparam);
    }
    found.into_iter().next()
}

pub fn window_client_size(hwnd: HWND) -> Option<(i32, i32)> {
    if hwnd.0.is_null() {
        return None;
    }
    let mut rect = RECT::default();
    let ok = unsafe { GetClientRect(hwnd, &mut rect) };
    if ok.is_err() {
        return None;
    }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::is_mumu_window_title;

    #[test]
    fn detects_basic_mumu_title() {
        assert!(is_mumu_window_title("MuMu"));
    }

    #[test]
    fn detects_common_mumu_window_titles() {
        assert!(is_mumu_window_title("MuMu模拟器"));
        assert!(is_mumu_window_title("MuMu Player"));
        assert!(is_mumu_window_title("mumu"));
    }

    #[test]
    fn rejects_unrelated_titles() {
        assert!(!is_mumu_window_title("Some Other Window"));
        assert!(!is_mumu_window_title("Random App"));
    }
}
