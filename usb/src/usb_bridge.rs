extern crate libusb;

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use super::bridge::BridgeError;
use super::config::Config;

pub struct UsbBridge {
    usb_pid: Option<u16>,
    usb_vid: Option<u16>,
    main_tx: Sender<ConnectThreadRequests>,
    main_rx: Receiver<ConnectThreadResponses>,
}

enum ConnectThreadRequests {
    StartPolling(Option<u16>, Option<u16>),
    Exit,
    Poke(u32 /* addr */, u32 /* val */),
    Peek(u32 /* addr */),
}

enum ConnectThreadResponses {
    OpenedDevice,
    PeekResult(Result<u32, BridgeError>),
    PokeResult(Result<(), BridgeError>),
}

impl UsbBridge {
    pub fn new(cfg: &Config) -> Result<Self, BridgeError> {
        let usb_ctx = libusb::Context::new()?;
        let (thread_tx, main_rx) = channel();
        let (main_tx, thread_rx) = channel();

        let thr_pid = cfg.usb_pid.clone();
        let thr_vid = cfg.usb_vid.clone();
        thread::spawn(move || {
            Self::usb_connect_thread(usb_ctx, thread_tx, thread_rx, thr_pid, thr_vid)
        });

        Ok(UsbBridge {
            usb_pid: cfg.usb_pid.clone(),
            usb_vid: cfg.usb_vid.clone(),
            main_tx,
            main_rx,
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
            match self.main_rx.recv() {
                Ok(ConnectThreadResponses::OpenedDevice) => return Ok(()),
                Ok(_) => (),
                Err(_) => return Err(BridgeError::NotConnected),
            }
        }
    }

    fn usb_connect_thread(
        usb_ctx: libusb::Context,
        tx: Sender<ConnectThreadResponses>,
        rx: Receiver<ConnectThreadRequests>,
        pid: Option<u16>,
        vid: Option<u16>,
    ) {
        let mut pid = pid;
        let mut vid = vid;
        loop {
            let devices = usb_ctx.devices().unwrap();
            for device in devices.iter() {
                let device_desc = device.device_descriptor().unwrap();
                if Self::device_matches(&device_desc, &pid, &vid) {
                    println!(
                        "Opening device {:03} on bus {:03}",
                        device.bus_number(),
                        device.address()
                    );
                    let usb = device.open().unwrap();
                    tx.send(ConnectThreadResponses::OpenedDevice)
                        .expect("Couldn't post message to main thread");
                    let mut keep_going = true;
                    while keep_going {
                        let var = rx.recv();
                        match var {
                            Err(e) => panic!("error in connect thread: {}", e),
                            Ok(o) => match o {
                                ConnectThreadRequests::Exit => {
                                    println!("usb_connect_thread requested exit");
                                    return;
                                }
                                ConnectThreadRequests::StartPolling(p, v) => {
                                    pid = p.clone();
                                    vid = v.clone();
                                }
                                ConnectThreadRequests::Peek(addr) => {
                                    let result = Self::do_peek(&usb, addr);
                                    keep_going = result.is_ok();
                                    tx.send(ConnectThreadResponses::PeekResult(result))
                                        .expect("Couldn't post peek response to main thread");
                                }
                                ConnectThreadRequests::Poke(addr, val) => {
                                    let result = Self::do_poke(&usb, addr, val);
                                    keep_going = result.is_ok();
                                    tx.send(ConnectThreadResponses::PokeResult(result))
                                        .expect("Couldn't post poke response to main thread");
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
                        ConnectThreadRequests::Peek(_addr) => tx
                            .send(ConnectThreadResponses::PeekResult(Err(
                                BridgeError::NotConnected,
                            )))
                            .expect("Couldn't respond to peek request"),
                        ConnectThreadRequests::Poke(_addr, _val) => tx
                            .send(ConnectThreadResponses::PokeResult(Err(
                                BridgeError::NotConnected,
                            )))
                            .expect("Couldn't respond to poke request"),
                        ConnectThreadRequests::StartPolling(p, v) => {
                            pid = p.clone();
                            vid = v.clone();
                        }
                    },
                }
            }
        }
    }

    fn do_poke(usb: &libusb::DeviceHandle, addr: u32, value: u32) -> Result<(), BridgeError> {
        let mut data_val = [0; 4];
        data_val[0] = ((value >> 0) & 0xff) as u8;
        data_val[1] = ((value >> 8) & 0xff) as u8;
        data_val[2] = ((value >> 16) & 0xff) as u8;
        data_val[3] = ((value >> 24) & 0xff) as u8;
        match usb.write_control(
            0x43,
            0,
            ((addr >> 0) & 0xffff) as u16,
            ((addr >> 16) & 0xffff) as u16,
            &data_val,
            Duration::from_millis(500),
        ) {
            Err(e) => Err(BridgeError::USBError(e)),
            Ok(len) => {
                if len != 4 {
                    Err(BridgeError::LengthError(4, len))
                } else {
                    Ok(())
                    /*((data_val[3] as u32) << 24)
                    | ((data_val[2] as u32) << 16)
                    | ((data_val[1] as u32) << 8)
                    | ((data_val[0] as u32) << 0))*/
                }
            }
        }
    }

    fn do_peek(usb: &libusb::DeviceHandle, addr: u32) -> Result<u32, BridgeError> {
        let mut data_val = [0; 4];
        match usb.read_control(
            0xc3,
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
        self.main_tx.send(ConnectThreadRequests::Poke(addr, value)).expect("Unable to send poke to connect thread");
        loop {
            let result = self.main_rx.recv().expect("Unable to receive poke from connect thread");
            if let ConnectThreadResponses::PokeResult(r) = result {
                return r;
            }
        }
    }

    pub fn peek(&self, addr: u32) -> Result<u32, BridgeError> {
        self.main_tx.send(ConnectThreadRequests::Peek(addr)).expect("Unable to send peek to connect thread");
        loop {
            let result = self.main_rx.recv().expect("Unable to receive peek from connect thread");
            if let ConnectThreadResponses::PeekResult(r) = result {
                return r;
            }
        }
    }
}
