use std::mem;
use std::string::FromUtf8Error;

use windows::core::*;
use windows::Devices::HumanInterfaceDevice::HidDevice;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsA,
    SetupDiGetDeviceInterfaceDetailA, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_A, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid;
use windows::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND, ERROR_NO_MORE_ITEMS, HWND};

union SpDeviceInterfaceDetailData {
    didd: SP_DEVICE_INTERFACE_DETAIL_DATA_A,
    filler: [u8; 512],
}

struct HDevInfo(pub HDEVINFO);

impl HDevInfo {
    fn setup() -> Result<Self> {
        let guid = unsafe { HidD_GetHidGuid() };

        let hardware_device_info = unsafe {
            SetupDiGetClassDevsA(
                Some(&guid),
                PCSTR::null(),                         // Define no enumerator (global)
                HWND(0),                               // Define no
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE, // Only Devices present
            )?
        };
        Ok(Self(hardware_device_info))
    }
}

impl Drop for HDevInfo {
    fn drop(&mut self) {
        unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

impl SpDeviceInterfaceDetailData {
    fn get_string(&self) -> std::result::Result<String, FromUtf8Error> {
        assert!(unsafe { self.didd.cbSize < (512 - 4) });

        let tmp = unsafe { &self.filler[4..512] };
        let tmp = Vec::from(tmp);
        String::from_utf8(tmp).map(|x| x.trim_matches('\0').to_string())
    }

    fn get_sized_string(&self, len: u32) -> std::result::Result<String, FromUtf8Error> {
        assert!(len <= (512 - 4));
        assert!(len >= 5);

        let tmp = unsafe { &self.filler[4..len as usize - 1] };
        let tmp = Vec::from(tmp);
        String::from_utf8(tmp)
    }
}

pub fn find_hid_decvice(vid: u16, pid: u16) -> Result<String> {
    let search_for = format!(r#"\\?\hid#vid_{:04x}&pid_{:04x}#"#, vid, pid);
    // println!("Looking for '{}'", &search_for);

    let mut device_info_data: SP_DEVICE_INTERFACE_DATA = unsafe { mem::zeroed() };
    let mut function_class_device_data: SpDeviceInterfaceDetailData = unsafe { mem::zeroed() };

    device_info_data.cbSize = mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;

    let guid = unsafe { HidD_GetHidGuid() };

    let hardware_device_info = HDevInfo::setup()?;

    'enum_devs: for i in 0.. {
        if unsafe {
            SetupDiEnumDeviceInterfaces(
                hardware_device_info.0,
                None,
                &guid,
                i,
                &mut device_info_data,
            )
            .as_bool()
        } {
            let mut actual_length = 0;
            let mut did: SP_DEVINFO_DATA;
            unsafe {
                function_class_device_data.filler = mem::zeroed();
                function_class_device_data.didd.cbSize =
                    mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_A>() as u32;
            };
            if unsafe {
                SetupDiGetDeviceInterfaceDetailA(
                    hardware_device_info.0,
                    &device_info_data,
                    Some(&mut function_class_device_data.didd),
                    mem::size_of::<SpDeviceInterfaceDetailData>() as u32,
                    Some(&mut actual_length),
                    None,
                )
            }
            .as_bool()
            {
                let dev = function_class_device_data
                    .get_sized_string(actual_length)
                    .unwrap();

                if dev.starts_with(&search_for) {
                    return Ok(dev);
                }
            }
        } else {
            let last_error = unsafe { GetLastError() };
            if last_error != ERROR_NO_MORE_ITEMS {
                return Err(Error::from(last_error));
            } else {
                break 'enum_devs;
            }
        }
    }

    Err(Error::from(ERROR_NOT_FOUND))
}

pub fn open_hid_device(
    path: &str,
    HasReadAccess: bool,
    HasWriteAccess: bool,
    IsOverlapped: bool,
    IsExclusive: bool,
) -> Result<HidDevice> {
    unimplemented!();
}
