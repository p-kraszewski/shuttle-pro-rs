use lazy_static::lazy_static;
use std::{mem, slice, sync};

use windows::core::*;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoA, RegisterRawInputDevices, HRAWINPUT, RAWINPUT,
    RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDI_DEVICENAME, RID_INPUT,
};
use windows::{
    core::*, Win32::Foundation::*, Win32::Graphics::Gdi::ValidateRect,
    Win32::System::LibraryLoader::GetModuleHandleA, Win32::UI::WindowsAndMessaging::*,
};

use shuttle_pro_rs::hid;

union RawInputWrapper {
    ri: RAWINPUT,
    data: [u8; 1024],
}

impl RawInputWrapper {
    fn parse_hid(&self) -> Vec<Vec<u8>> {
        Vec::new()
    }
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
struct ContourHid {
    last: ContourHidEvent,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum ContourEvents {
    Jog(i8),
    WheelLeft,
    WheelRight,
    ButtonUp(u16),
    ButtonDown(u16),
}

impl ContourHid {
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

static mut PLAYER: Option<vlc::MediaPlayer> = None;

fn main() -> Result<()> {
    let dev = hid::find_hid_decvice(0x0b33, 0x0030)?;
    println!("DEV={}", dev);

    let instance = unsafe { GetModuleHandleA(None) }?;
    debug_assert!(instance.0 != 0);

    let window_class = s!("window");

    let wc = WNDCLASSA {
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hInstance: instance.into(),
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
            s!("This is a sample window"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
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

    // Create an instance
    let instance = vlc::Instance::new().unwrap();
    // Create a media from a file
    let md = vlc::Media::new_path(
        &instance,
        r#"E:\VLC\Introducing Rain - Free Blender rig.mkv"#,
    )
    .unwrap();
    // Create a media player
    let mdp = vlc::MediaPlayer::new(&instance).unwrap();
    mdp.set_media(&md);
    mdp.play();
    unsafe { PLAYER = Some(mdp) };

    // Start playing
    // mdp.play().unwrap();

    while unsafe { GetMessageA(&mut message, None, 0, 0) }.into() {
        unsafe { DispatchMessageA(&message) };
    }

    Ok(())
}

static mut OLD_KEYS: ContourHid = ContourHid {
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
                let hiddata =
                    unsafe { *(data.ri.data.hid.bRawData.as_ptr() as *const ContourHidEvent) };
                println!("HID: {:X?}/{}", hiddata, unsafe {
                    data.ri.data.hid.dwCount
                });
                let mut P = unsafe { (PLAYER.as_ref()) }.unwrap();
                let evts = unsafe { OLD_KEYS.update(hiddata) };
                println!("EVT={:?}", &evts);
                for evt in evts {
                    match evt {
                        ContourEvents::Jog(x) => {
                            let refrate = 2.0_f32;
                            let rate = refrate.powf(x as f32 / 2.0);
                            P.set_rate(rate);
                            println!("Playback rate {}/{}", x, rate);
                        }
                        ContourEvents::WheelLeft => {
                            if let Some(pos) = P.get_time() {
                                if pos > 1000 {
                                    P.set_time(pos - 1000);
                                } else {
                                    P.set_time(0)
                                }
                            }
                        }
                        ContourEvents::WheelRight => {
                            if let Some(pos) = P.get_time() {
                                P.set_time(pos + 1000);
                            }
                        }
                        ContourEvents::ButtonUp(b) => match b {
                            6 => P.set_pause(P.is_playing()),
                            _ => (),
                        },
                        ContourEvents::ButtonDown(_) => {}
                    }
                }
                // if hiddata.keys&0x0001 !=0{
                //     P.set_pause(P.is_playing());
                // }
            } else {
                println!("OtherDev");
            }

            LRESULT(0)
        }
        _ => unsafe { DefWindowProcA(window, message, wparam, lparam) },
    }
}

fn send_msg(cmd: u32) {
    let vlc = unsafe { FindWindowA(s!("Qt5QWindowIcon"), None) };
    //let vlc = unsafe{FindWindowA(s!("SkinWindowClass"), None)};

    if vlc.0 > 0 {
        //println!("Found VLC");
        let rc = unsafe { SendMessageA(vlc, WM_COMMAND, WPARAM(cmd as usize), None) };
        println!("Found VLC, rc={}", rc.0);
    } else {
        println!("No VLC");
    }
}
