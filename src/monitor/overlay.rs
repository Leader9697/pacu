//! Overlay milik Pacu: jendela kecil transparan yang selalu di atas dan tidak bisa diklik, di
//! pojok layar tempat game berjalan. Tidak menyuntik apa pun ke game, jadi aman untuk anti-cheat,
//! tetapi hanya terlihat di mode Windowed / Borderless (bukan Fullscreen eksklusif). Untuk
//! Fullscreen eksklusif, pakai mode RTSS.
//!
//! Ctrl+Shift+F10 menyembunyikan / menampilkan overlay (berlaku juga untuk mode RTSS).

use super::osd::Line;
use crate::config::Corner;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Default)]
pub struct State {
    pub lines: Vec<Line>,
    pub corner: Option<Corner>,
    pub scale: f32,
    pub visible: bool,
}

pub struct Overlay {
    state: Arc<Mutex<State>>,
    /// Diubah oleh hotkey Ctrl+Shift+F10.
    pub user_hidden: Arc<AtomicBool>,
    #[cfg(windows)]
    hwnd: Arc<std::sync::atomic::AtomicIsize>,
}

impl Overlay {
    pub fn new() -> Overlay {
        let o = Overlay {
            state: Arc::new(Mutex::new(State { scale: 1.0, ..Default::default() })),
            user_hidden: Arc::new(AtomicBool::new(false)),
            #[cfg(windows)]
            hwnd: Arc::new(std::sync::atomic::AtomicIsize::new(0)),
        };
        #[cfg(windows)]
        imp::spawn(o.state.clone(), o.user_hidden.clone(), o.hwnd.clone());
        o
    }

    pub fn hidden_by_user(&self) -> bool {
        self.user_hidden.load(Ordering::Relaxed)
    }

    /// Memperbarui isi dan posisi. `visible` false menyembunyikan jendela.
    pub fn update(&self, lines: Vec<Line>, corner: Corner, scale: f32, visible: bool) {
        {
            let mut s = self.state.lock();
            if !visible && !s.visible {
                return;
            }
            s.lines = lines;
            s.corner = Some(corner);
            s.scale = scale.clamp(0.6, 2.5);
            s.visible = visible;
        }
        #[cfg(windows)]
        imp::poke(&self.hwnd);
    }
}

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::atomic::AtomicIsize;
    use std::sync::OnceLock;
    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_F10};
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WM_POKE: u32 = WM_APP + 1;
    const PAD: i32 = 10;

    struct Ctx {
        state: Arc<Mutex<State>>,
        user_hidden: Arc<AtomicBool>,
    }

    static CTX: OnceLock<Ctx> = OnceLock::new();

    pub fn spawn(state: Arc<Mutex<State>>, user_hidden: Arc<AtomicBool>, hwnd_out: Arc<AtomicIsize>) {
        if CTX.set(Ctx { state, user_hidden }).is_err() {
            return;
        }
        let _ = std::thread::Builder::new().name("pacu-overlay".into()).spawn(move || unsafe {
            let inst = GetModuleHandleW(None).unwrap_or_default();
            let class = w!("PacuOverlay");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            let Ok(hwnd) = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("Pacu Overlay"),
                WS_POPUP,
                0,
                0,
                10,
                10,
                None,
                None,
                Some(inst.into()),
                None,
            ) else {
                return;
            };
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 215, LWA_ALPHA);
            let _ = RegisterHotKey(Some(hwnd), 1, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, VK_F10.0 as u32);
            hwnd_out.store(hwnd.0 as isize, Ordering::Relaxed);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
    }

    pub fn poke(hwnd: &AtomicIsize) {
        let h = hwnd.load(Ordering::Relaxed);
        if h != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(h as _)), WM_POKE, WPARAM(0), LPARAM(0));
            }
        }
    }

    fn rgb(c: u32) -> COLORREF {
        COLORREF(((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF))
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    unsafe fn font(scale: f32, dpi: u32) -> HFONT {
        let h = -(15.0 * scale * dpi as f32 / 96.0).round() as i32;
        CreateFontW(h, 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"))
    }

    unsafe fn text_size(dc: HDC, s: &str) -> SIZE {
        let mut sz = SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &wide(s), &mut sz);
        sz
    }

    /// (lebar kolom label, tinggi baris, ukuran jendela)
    unsafe fn layout(dc: HDC, lines: &[Line]) -> (i32, i32, i32, i32) {
        let label_w = lines.iter().map(|l| text_size(dc, l.label).cx).max().unwrap_or(0) + PAD;
        let value_w = lines.iter().map(|l| text_size(dc, &l.value).cx).max().unwrap_or(0);
        let line_h = text_size(dc, "Ag").cy + 2;
        (label_w, line_h, label_w + value_w + PAD * 2, line_h * lines.len() as i32 + PAD * 2 - 4)
    }

    unsafe fn place(hwnd: HWND) {
        let ctx = CTX.get().unwrap();
        let st = ctx.state.lock();
        let show = st.visible && !st.lines.is_empty() && !ctx.user_hidden.load(Ordering::Relaxed);
        if !show {
            let _ = ShowWindow(hwnd, SW_HIDE);
            return;
        }
        // Monitor tempat jendela di depan (game) berada.
        let fg = GetForegroundWindow();
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96);
        let dc = GetDC(Some(hwnd));
        let f = font(st.scale, dpi);
        let old = SelectObject(dc, f.into());
        let (_, _, w, h) = layout(dc, &st.lines);
        SelectObject(dc, old);
        let _ = DeleteObject(f.into());
        ReleaseDC(Some(hwnd), dc);
        let r = mi.rcMonitor;
        let m = 12;
        let (x, y) = match st.corner.unwrap_or(Corner::TopLeft) {
            Corner::TopLeft => (r.left + m, r.top + m),
            Corner::TopRight => (r.right - w - m, r.top + m),
            Corner::BottomLeft => (r.left + m, r.bottom - h - m),
            Corner::BottomRight => (r.right - w - m, r.bottom - h - m),
        };
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let _ = InvalidateRect(Some(hwnd), None, true);
    }

    unsafe fn paint(hwnd: HWND) {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let bg = CreateSolidBrush(rgb(0x0C0E14));
        FillRect(dc, &rc, bg);
        let _ = DeleteObject(bg.into());
        let st = CTX.get().unwrap().state.lock();
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96);
        let f = font(st.scale, dpi);
        let old = SelectObject(dc, f.into());
        SetBkMode(dc, TRANSPARENT);
        let (label_w, line_h, _, _) = layout(dc, &st.lines);
        for (i, l) in st.lines.iter().enumerate() {
            let y = PAD - 2 + i as i32 * line_h;
            SetTextColor(dc, rgb(l.color));
            let _ = TextOutW(dc, PAD, y, &wide(l.label));
            SetTextColor(dc, rgb(0xF3F4F6));
            let _ = TextOutW(dc, PAD + label_w, y, &wide(&l.value));
        }
        SelectObject(dc, old);
        let _ = DeleteObject(f.into());
        let _ = EndPaint(hwnd, &ps);
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_POKE => {
                place(hwnd);
                LRESULT(0)
            }
            WM_HOTKEY => {
                if let Some(c) = CTX.get() {
                    c.user_hidden.fetch_xor(true, Ordering::Relaxed);
                }
                place(hwnd);
                LRESULT(0)
            }
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_NCHITTEST => LRESULT(-1), // HTTRANSPARENT
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
