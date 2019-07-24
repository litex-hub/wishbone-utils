extern crate libusb;

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, Condvar};
use std::thread;
use std::time::Duration;

use log::error;

use super::bridge::BridgeError;
use super::config::Config;
// use log::debug;

#[derive(Clone)]
pub struct UsbBridge {
    usb_pid: Option<u16>,
    usb_vid: Option<u16>,
    main_tx: Sender<ConnectThreadRequests>,
    main_rx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
}

enum ConnectThreadRequests {
    StartPolling(Option<u16> /* vid */, Option<u16> /* pid */),
    Exit,
    Poke(u32 /* addr */, u32 /* val */),
    Peek(u32 /* addr */),
}

#[derive(PartialEq, Debug)]
enum ConnectThreadResponses {
    OpenedDevice,
    PeekResult(Result<u32, BridgeError>),
    PokeResult(Result<(), BridgeError>),
}

impl UsbBridge {
    pub fn new(cfg: &Config) -> Result<Self, BridgeError> {
        let usb_ctx = libusb::Context::new()?;
        let (main_tx, thread_rx) = channel();
        let cv = Arc::new((Mutex::new(None), Condvar::new()));

        let thr_pid = cfg.usb_pid.clone();
        let thr_vid = cfg.usb_vid.clone();
        let thr_cv = cv.clone();
        thread::spawn(move || {
            Self::usb_connect_thread(usb_ctx, thr_cv, thread_rx, thr_pid, thr_vid, 0x43)
        });

        Ok(UsbBridge {
            usb_pid: cfg.usb_pid.clone(),
            usb_vid: cfg.usb_vid.clone(),
            main_tx,
            main_rx: cv,
        })
    }

    fn device_matches(
        device_desc: &libusb::DeviceDescriptor,
        usb_pid: &Option<u16>,
        usb_vid: &Option<u16>,
    ) -> bool {
        if let Some(pid) = usb_pid {
            if *pid != device_desc.product_id() {
                return false;
            }
        }
        if let Some(vid) = usb_vid {
            if *vid != device_desc.vendor_id() {
                return false;
            }
        }
        true
    }

    pub fn connect(&self) -> Result<(), BridgeError> {
        self.main_tx
            .send(ConnectThreadRequests::StartPolling(
                self.usb_pid.clone(),
                self.usb_vid.clone(),
            ))
            .unwrap();
        loop {
            let &(ref lock, ref cvar) = &*self.main_rx;
            let mut _mtx = lock.lock().unwrap();
            *_mtx = None;
            while _mtx.is_none() {
                _mtx = cvar.wait(_mtx).unwrap();
            }
            match *_mtx {
                Some(ConnectThreadResponses::OpenedDevice) => return Ok(()),
                _ => (),
            }
        }
    }

    fn usb_connect_thread(
        usb_ctx: libusb::Context,
        tx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
        rx: Receiver<ConnectThreadRequests>,
        pid: Option<u16>,
        vid: Option<u16>,
        debug_byte: u8,
    ) {
        let mut pid = pid;
        let mut vid = vid;
        let &(ref response, ref cvar) = &*tx;
        loop {
            let devices = usb_ctx.devices().unwrap();
            for device in devices.iter() {
                let device_desc = device.device_descriptor().unwrap();
                if Self::device_matches(&device_desc, &pid, &vid) {
                    // println!(
                    //     "Opening device {:03} on bus {:03}",
                    //     device.bus_number(),
                    //     device.address()
                    // );
                    let usb = device.open().expect("Unable to open USB device");
                    {
                        *response.lock().unwrap() = Some(ConnectThreadResponses::OpenedDevice);
                        cvar.notify_one();
                    }
                    let mut keep_going = true;
                    while keep_going {
                        let var = rx.recv();
                        match var {
                            Err(e) => panic!("error in connect thread: {}", e),
                            Ok(o) => match o {
                                ConnectThreadRequests::Exit => {
                                    // println!("usb_connect_thread requested exit");
                                    return;
                                }
                                ConnectThreadRequests::StartPolling(p, v) => {
                                    pid = p.clone();
                                    vid = v.clone();
                                }
                                ConnectThreadRequests::Peek(addr) => {
                                    let result = Self::do_peek(&usb, addr, debug_byte);
                                    keep_going = result.is_ok();
                                    *response.lock().unwrap() = Some(ConnectThreadResponses::PeekResult(result));
                                    cvar.notify_one();
                                }
                                ConnectThreadRequests::Poke(addr, val) => {
                                    let result = Self::do_poke(&usb, addr, val, debug_byte);
                                    keep_going = result.is_ok();
                                    *response.lock().unwrap() = Some(ConnectThreadResponses::PokeResult(result));
                                    cvar.notify_one();
                                }
                            },
                        }
                    }
                }
            }
            println!("No device available, pausing");
            thread::park_timeout(Duration::from_millis(500));
            loop {
                match rx.try_recv() {
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => panic!("main thread disconnected"),
                    Ok(m) => match m {
                        ConnectThreadRequests::Exit => {
                            println!("main thread requested exit");
                            return;
                        }
                        ConnectThreadRequests::Peek(_addr) => {
                            *response.lock().unwrap() = Some(ConnectThreadResponses::PeekResult(Err(
                                BridgeError::NotConnected,
                            )));
                            cvar.notify_one();
                        },
                        ConnectThreadRequests::Poke(_addr, _val) => {
                            *response.lock().unwrap() = Some(ConnectThreadResponses::PokeResult(Err(
                                BridgeError::NotConnected,
                            )));
                            cvar.notify_one();
                        },
                        ConnectThreadRequests::StartPolling(p, v) => {
                            pid = p.clone();
                            vid = v.clone();
                        }
                    },
                }
            }
        }
    }

    fn do_poke(
        usb: &libusb::DeviceHandle,
        addr: u32,
        value: u32,
        debug_byte: u8,
    ) -> Result<(), BridgeError> {
        // debug!("POKE @ {:08x}", addr);
        let mut data_val = [0; 4];
        data_val[0] = ((value >> 0) & 0xff) as u8;
        data_val[1] = ((value >> 8) & 0xff) as u8;
        data_val[2] = ((value >> 16) & 0xff) as u8;
        data_val[3] = ((value >> 24) & 0xff) as u8;
        match usb.write_control(
            debug_byte,
            0,
            ((addr >> 0) & 0xffff) as u16,
            ((addr >> 16) & 0xffff) as u16,
            &data_val,
            Duration::from_millis(100),
        ) {
            Err(e) => Err(BridgeError::USBError(e)),
            Ok(len) => {
                if len != 4 {
                    Err(BridgeError::LengthError(4, len))
                } else {
                    Ok(())
                }
            }
        }
    }

    fn do_peek(usb: &libusb::DeviceHandle, addr: u32, debug_byte: u8) -> Result<u32, BridgeError> {
        let mut data_val = [0; 512];
        // debug!("PEEK @ {:08x}", addr);
        match usb.read_control(
            0x80 | debug_byte,
            0,
            ((addr >> 0) & 0xffff) as u16,
            ((addr >> 16) & 0xffff) as u16,
            &mut data_val,
            Duration::from_millis(500),
        ) {
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

    pub fn poke(&self, addr: u32, value: u32) -> Result<(), BridgeError> {
        let &(ref lock, ref cvar) = &*self.main_rx;
        let mut _mtx = lock.lock().unwrap();
        self.main_tx
            .send(ConnectThreadRequests::Poke(addr, value))
            .expect("Unable to send poke to connect thread");
        *_mtx = None;
        while _mtx.is_none() {
            _mtx = cvar.wait(_mtx).unwrap();
        }
        match _mtx.take() {
            Some(ConnectThreadResponses::PokeResult(r)) => Ok(r?),
            e => {
                error!("unexpected bridge poke response: {:?}", e);
                Err(BridgeError::WrongResponse)
            }
        }
    }

    pub fn peek(&self, addr: u32) -> Result<u32, BridgeError> {
        let &(ref lock, ref cvar) = &*self.main_rx;
        let mut _mtx = lock.lock().unwrap();
        self.main_tx
            .send(ConnectThreadRequests::Peek(addr))
            .expect("Unable to send peek to connect thread");
        *_mtx = None;
        while _mtx.is_none() {
            _mtx = cvar.wait(_mtx).unwrap();
        }
        match _mtx.take() {
            Some(ConnectThreadResponses::PeekResult(r)) => Ok(r?),
            e => {
                error!("unexpected bridge peek response: {:?}", e);
                Err(BridgeError::WrongResponse)
            }
        }
    }
}

impl Drop for UsbBridge {
    fn drop(&mut self) {
        let &(ref lock, ref _cvar) = &*self.main_rx;
        let mut _mtx = lock.lock().unwrap();
        self.main_tx
            .send(ConnectThreadRequests::Exit)
            .expect("Unable to send Exit request to thread");
    }
}
