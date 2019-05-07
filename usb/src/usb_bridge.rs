extern crate libusb;

use std::time::Duration;
use std::thread;

use super::bridge::BridgeError;
use super::config::Config;

pub struct UsbBridge<'a> {
    usb_ctx: libusb::Context,
    usb: Option<libusb::DeviceHandle<'a>>,
    usb_pid: Option<u16>,
    usb_vid: Option<u16>,
}

impl<'a> UsbBridge<'a> {
    pub fn new(cfg: &Config) -> Result<Self, BridgeError> {
        let usb_ctx = libusb::Context::new().unwrap();
        Ok(UsbBridge { usb_ctx, usb: None, usb_pid: cfg.usb_pid.clone(), usb_vid: cfg.usb_vid.clone(), })
    }

    fn device_matches(&self, device_desc: &libusb::DeviceDescriptor) -> bool {
        if let Some(pid) = self.usb_pid {
            if pid != device_desc.product_id() {
                return false;
            }
        }
        if let Some(vid) = self.usb_vid {
            if vid != device_desc.vendor_id() {
                return false;
            }
        }
        true
    }

    pub fn usb_reconnect(&self) -> Result<(), BridgeError> {
        loop {
            let ctx = &self.usb_ctx;
            let devices = ctx.devices().unwrap();
            for device in devices.iter() {
                let device_desc = device.device_descriptor().unwrap();
                if self.device_matches(&device_desc) {
                    println!(
                        "Opening device {:03} on bus {:03}",
                        device.bus_number(),
                        device.address()
                    );
                    self.usb = Some(device.open().unwrap());
                    return Ok(())
                }
            }
            thread::park_timeout(Duration::from_millis(500));
        }
    }

    pub fn poke(&self, addr: u32, value: u32) -> Result<u32, BridgeError> {
        let mut data_val = [0; 4];
        let result = match self.usb {
            None => return Err(BridgeError::NotConnected),
            Some(usb) => {
                data_val[0] = ((value >> 0) & 0xff) as u8;
                data_val[1] = ((value >> 8) & 0xff) as u8;
                data_val[2] = ((value >> 16) & 0xff) as u8;
                data_val[3] = ((value >> 24) & 0xff) as u8;
                usb.write_control(
                    0x43,
                    0,
                    ((addr >> 0) & 0xffff) as u16,
                    ((addr >> 16) & 0xffff) as u16,
                    &data_val,
                    Duration::from_millis(500),
                )
            }
        };
        match result {
            Err(e) => Err(BridgeError::USBError(e)),
            Ok(len) => {
                if len != 4 {
                    Err(BridgeError::LengthError(4, len))
                } else {
                    Ok(((data_val[3] as u32) << 24)
                        | ((data_val[2] as u32) << 16)
                        | ((data_val[1] as u32) << 8)
                        | ((data_val[0] as u32) << 0))
                }
            }
        }
    }

    pub fn peek(&self, addr: u32) -> Result<u32, BridgeError> {
        let mut data_val = [0; 4];
        let result = match self.usb {
            None => return Err(BridgeError::NotConnected),
            Some(usb) => usb.read_control(
                0xc3,
                0,
                ((addr >> 0) & 0xffff) as u16,
                ((addr >> 16) & 0xffff) as u16,
                &mut data_val,
                Duration::from_millis(500),
            ),
        };
        match result {
            Err(e) => Err(BridgeError::USBError(e)),
            Ok(len) => {
                if len != 4 {
                    Err(BridgeError::LengthError(4, len))
                } else {
                    Ok(((data_val[3] as u32) << 24)
                        | ((data_val[2] as u32) << 16)
                        | ((data_val[1] as u32) << 8)
                        | ((data_val[0] as u32) << 0))
                }
            }
        }
    }
}
