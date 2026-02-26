use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

#[derive(Debug)]
pub struct Frame {
    pub width: i32,
    pub height: i32,
    pub bgra: Vec<u8>,
}

#[derive(Debug)]
pub struct CaptureError;

pub type CaptureResult<T> = Result<T, CaptureError>;

pub fn capture_window(hwnd: HWND) -> CaptureResult<Frame> {
    if hwnd.0.is_null() {
        return Err(CaptureError);
    }
    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return Err(CaptureError);
        }

        let src_x = rect.left;
        let src_y = rect.top;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return Err(CaptureError);
        }

        let desktop = HWND(std::ptr::null_mut());
        let hdc_screen = GetDC(desktop);
        if hdc_screen.0.is_null() {
            return Err(CaptureError);
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        if hdc_mem.0.is_null() {
            ReleaseDC(desktop, hdc_screen);
            return Err(CaptureError);
        }
        let hbmp = CreateCompatibleBitmap(hdc_screen, width, height);
        if hbmp.0.is_null() {
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(desktop, hdc_screen);
            return Err(CaptureError);
        }
        let old_obj = SelectObject(hdc_mem, hbmp);
        if old_obj.0.is_null() {
            let _ = DeleteObject(hbmp);
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(desktop, hdc_screen);
            return Err(CaptureError);
        }

        if BitBlt(
            hdc_mem, 0, 0, width, height, hdc_screen, src_x, src_y, SRCCOPY,
        )
        .is_err()
        {
            SelectObject(hdc_mem, old_obj);
            let _ = DeleteObject(hbmp);
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(desktop, hdc_screen);
            return Err(CaptureError);
        }

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        let stride = (width * 4) as usize;
        let buf_size = stride * height as usize;
        let mut buffer = vec![0u8; buf_size];

        let scan_lines = GetDIBits(
            hdc_mem,
            hbmp,
            0,
            height as u32,
            Some(buffer.as_mut_ptr().cast()),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(hdc_mem, old_obj);
        let _ = DeleteObject(hbmp);
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(desktop, hdc_screen);

        if scan_lines == 0 {
            return Err(CaptureError);
        }

        Ok(Frame {
            width,
            height,
            bgra: buffer,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::capture_window;
    use windows::Win32::Foundation::HWND;

    #[test]
    fn invalid_hwnd_returns_err() {
        let hwnd = HWND(std::ptr::null_mut());
        let result = capture_window(hwnd);
        assert!(result.is_err());
    }
}
