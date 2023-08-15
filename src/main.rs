#![windows_subsystem = "windows"]

use std::cmp::min;
use std::mem;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_OEM_4, VK_OEM_6, VK_OEM_PLUS, VK_SPACE,
};
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoA, RegisterRawInputDevices, HRAWINPUT, RAWINPUT,
    RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDI_DEVICENAME, RID_INPUT,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconA, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NOTIFYICONDATAA,
};
use windows::{
    core::*, Win32::Foundation::*, Win32::Graphics::Gdi::ValidateRect,
    Win32::System::LibraryLoader::GetModuleHandleA, Win32::UI::WindowsAndMessaging::*,
};
use winrt_notification::{Duration, Sound, Toast};

const APPWM_ICONNOTIFY: u32 = WM_APP + 1;

union RawInputWrapper {
    ri: RAWINPUT,
    _data: [u8; 1024],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct ContourHidEvent {
    id: u8,
    jog: i8,
    wheel: u8,
    _fill: u8,
    keys: u16,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct SystemState {
    last: ContourHidEvent,
    scroll_zoom: u8,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum ContourEvents {
    Jog(i8),
    WheelLeft,
    WheelRight,
    ButtonUp(u16),
    ButtonDown(u16),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Scroll {
    Left(u8),
    Right(u8),
}

impl SystemState {
    fn update(&mut self, new: ContourHidEvent) -> Vec<ContourEvents> {
        let mut evt = Vec::new();
        if self.last.id != 0 {
            self.last.wheel = new.wheel;
        }

        if self.last.jog != new.jog {
            evt.push(ContourEvents::Jog(new.jog));
        }
        if self.last.wheel != new.wheel {
            let mut delta = new.wheel as i16 - self.last.wheel as i16;
            if delta > 128 {
                delta -= 256
            }
            if delta < -128 {
                delta += 256
            }
            evt.push(if delta < 0 {
                ContourEvents::WheelLeft
            } else {
                ContourEvents::WheelRight
            });
        }
        if self.last.keys != new.keys {
            for k in 0u16..15 {
                let last_key = self.last.keys & (1 << k) != 0;
                let new_key = new.keys & (1 << k) != 0;
                match (last_key, new_key) {
                    (false, true) => {
                        evt.push(ContourEvents::ButtonDown(k));
                    }
                    (true, false) => {
                        evt.push(ContourEvents::ButtonUp(k));
                    }
                    _ => (),
                }
            }
        }

        self.last = new;
        evt
    }
}

const CONTOUR_ID: &str = r#"\\?\hid#vid_0b33&pid_0030#"#;

fn main() {
    match xmain() {
        Ok(()) => {}
        Err(msg) => message("Error", msg.to_string().as_str()),
    }
}

fn xmain() -> Result<()> {
    let instance = unsafe { GetModuleHandleA(None) }?;
    debug_assert!(instance.0 != 0);

    let window_class = s!("contour_control_window");

    let wc = WNDCLASSA {
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hInstance: instance,
        lpszClassName: window_class,

        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        ..Default::default()
    };

    let atom = unsafe { RegisterClassA(&wc) };
    debug_assert!(atom != 0);

    let wnd = unsafe {
        CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            window_class,
            s!("Contour Controller"),
            WS_OVERLAPPEDWINDOW, /*| WS_VISIBLE*/
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            instance,
            None,
        )
    };

    let devices: [RAWINPUTDEVICE; 1] = [RAWINPUTDEVICE {
        usUsagePage: 0x000C,
        usUsage: 0x0001,
        dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
        hwndTarget: wnd,
    }];

    unsafe { RegisterRawInputDevices(&devices, mem::size_of_val(&devices) as u32) };

    let mut message = MSG::default();

    register_icon(wnd);

    while unsafe { GetMessageA(&mut message, None, 0, 0) }.into() {
        unsafe { DispatchMessageA(&message) };
    }

    Ok(())
}

static mut GLOBAL_STATE: SystemState = SystemState {
    scroll_zoom: 0,
    last: ContourHidEvent {
        id: 0xFF,
        jog: 0,
        wheel: 0,
        _fill: 0,
        keys: 0,
    },
};

extern "system" fn wndproc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_PAINT => {
            println!("WM_PAINT");
            unsafe { ValidateRect(window, None) };
            LRESULT(0)
        }
        WM_DESTROY => {
            println!("WM_DESTROY");
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }

        APPWM_ICONNOTIFY => match lparam.0 as u32 {
            WM_LBUTTONUP => {
                println!("WM_NOTIFY WM_LBUTTONUP");
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }

            _ => {
                println!("WM_NOTIFY OTHER");
                LRESULT(0)
            }
        },

        WM_INPUT => {
            //  println!("WM_INPUT");
            let mut data: RawInputWrapper = unsafe { mem::zeroed() };
            let mut size = mem::size_of::<RawInputWrapper>() as u32;
            let rc = unsafe {
                GetRawInputData(
                    HRAWINPUT(lparam.0),
                    RID_INPUT,
                    Some(&mut data as *mut RawInputWrapper as *mut ::core::ffi::c_void),
                    &mut size,
                    mem::size_of::<RAWINPUTHEADER>() as u32,
                )
            };
            if rc < 1 {
                return LRESULT(0);
            }
            let dev = unsafe { data.ri.header.hDevice };
            let mut name = [0u8; 1024];
            let mut dlen = 1024u32;
            let rc = unsafe {
                GetRawInputDeviceInfoA(
                    dev,
                    RIDI_DEVICENAME,
                    Some(name.as_mut_ptr() as *mut ::core::ffi::c_void),
                    &mut dlen,
                )
            };
            if rc < 1 {
                return LRESULT(0);
            }
            let devn = String::from_utf8_lossy(&name[..rc as usize]).to_lowercase();
            if devn.starts_with(CONTOUR_ID) && unsafe { data.ri.data.hid.dwSizeHid == 6 } {
                process_contour_event(&mut data);
            } else {
                println!("OtherDev");
            }

            LRESULT(0)
        }
        _ => unsafe { DefWindowProcA(window, message, wparam, lparam) },
    }
}

fn process_contour_event(data: &mut RawInputWrapper) {
    let hiddata = unsafe { *(data.ri.data.hid.bRawData.as_ptr() as *const ContourHidEvent) };
    println!("HID: {:X?}/{}", hiddata, unsafe {
        data.ri.data.hid.dwCount
    });
    // let mut P = unsafe { (PLAYER.as_ref()) }.unwrap();
    let evts = unsafe { GLOBAL_STATE.update(hiddata) };

    println!("EVT={:?}", &evts);
    for evt in evts {
        match evt {
            ContourEvents::Jog(x) => {
                if x < 0 {
                    send_key(VK_OEM_4); // [
                }
                if x > 0 {
                    send_key(VK_OEM_6); // ]
                }
            }
            ContourEvents::WheelLeft => {
                let zoom = unsafe { GLOBAL_STATE.scroll_zoom };
                send_h_wheel(Scroll::Left(1 << zoom));
            }
            ContourEvents::WheelRight => {
                let zoom = unsafe { GLOBAL_STATE.scroll_zoom };
                send_h_wheel(Scroll::Right(1 << zoom));
            }
            ContourEvents::ButtonUp(b) => match b {
                0..=3 => unsafe {
                    GLOBAL_STATE.scroll_zoom = b as u8;
                    message("Info", format!("Scroll speed {}", 1 << b).as_str());
                },
                6 => send_key(VK_SPACE),
                13 | 14 => {
                    send_key(VK_OEM_PLUS);
                    message("Info", "Playback speed normal");
                }
                _ => {}
            },
            ContourEvents::ButtonDown(_) => {}
        }
    }
}

fn send_key(key: VIRTUAL_KEY) {
    let vlc = unsafe { FindWindowA(s!("Qt5QWindowIcon"), None) };

    if vlc.0 > 0 {
        println!("Found VLC, sending {:?}", key);

        unsafe { PostMessageA(vlc, WM_KEYDOWN, WPARAM(key.0 as usize), LPARAM(1)) };

        unsafe {
            PostMessageA(
                vlc,
                WM_KEYUP,
                WPARAM(key.0 as usize),
                LPARAM(1 | 1 << 30 | 1 << 31),
            )
        };
    } else {
        println!("No VLC");
    }
}

fn send_h_wheel(scroll: Scroll) {
    let vlc = unsafe { FindWindowA(s!("Qt5QWindowIcon"), None) };

    if vlc.0 > 0 {
        println!("Found VLC, sending mouse {:?}", scroll);

        let (dir, steps) = match scroll {
            Scroll::Left(n) => (-1, n),
            Scroll::Right(n) => (1, n),
        };
        let ev = (dir as u16 as usize) << 16;
        for _ in 0..steps {
            unsafe { PostMessageA(vlc, WM_MOUSEHWHEEL, WPARAM(ev), LPARAM(0)) };
        }
    } else {
        println!("No VLC");
    }
}

fn register_icon(hwnd: HWND) {
    let icon = unsafe { LoadIconA(None, PCSTR(IDI_INFORMATION as *const u8)).unwrap() };
    let mut nid = NOTIFYICONDATAA {
        cbSize: mem::size_of::<NOTIFYICONDATAA>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_TIP | NIF_MESSAGE,
        uCallbackMessage: APPWM_ICONNOTIFY,
        hIcon: icon,
        szTip: [0; 128],
        dwState: Default::default(),
        dwStateMask: 0,
        szInfo: [0; 256],
        Anonymous: Default::default(),
        szInfoTitle: [0; 64],
        dwInfoFlags: Default::default(),
        guidItem: Default::default(),
        hBalloonIcon: Default::default(),
    };

    fill_slice(nid.szTip.as_mut_slice(), "Contour Control");

    unsafe { Shell_NotifyIconA(NIM_ADD, &nid) };
}

fn fill_slice(s: &mut [u8], data: &str) {
    let data = data.as_bytes();
    let len = min(data.len(), s.len());
    s[..len].copy_from_slice(&data[..len]);
}

fn message(title: &str, text: &str) {
    Toast::new(Toast::POWERSHELL_APP_ID)
        .title(title)
        .text1(text)
        .sound(Some(Sound::SMS))
        .duration(Duration::Short)
        .show()
        .expect("unable to toast");
}
