use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use log::{debug, error, info};

use crate::BridgeError;

#[derive(Clone, Default)]
pub struct UsbBridgeConfig {
    pid: Option<u16>,
    vid: Option<u16>,
    bus: Option<u8>,
    device: Option<u8>,
}

impl UsbBridgeConfig {
    pub fn new() -> UsbBridgeConfig {
        UsbBridgeConfig { pid: None, vid: None, bus: None, device: None }
    }
    pub fn pid(mut self, pid: Option<u16>) -> UsbBridgeConfig {
        self.pid = pid;
        self
    }
    pub fn vid(mut self, vid: Option<u16>) -> UsbBridgeConfig {
        self.vid = vid;
        self
    }
    pub fn bus(mut self, bus: Option<u8>) -> UsbBridgeConfig {
        self.bus = bus;
        self
    }
    pub fn device(mut self, device: Option<u8>) -> UsbBridgeConfig {
        self.device = device;
        self
    }
}

pub struct UsbBridge {
    usb_pid: Option<u16>,
    usb_vid: Option<u16>,
    main_tx: Sender<ConnectThreadRequests>,
    main_rx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
    mutex: Arc<Mutex<()>>,
    poll_thread: Option<thread::JoinHandle<()>>,
}

enum ConnectThreadRequests {
    StartPolling(Option<u16> /* vid */, Option<u16> /* pid */),
    Exit,
    Poke(u32 /* addr */, u32 /* val */),
    Peek(u32 /* addr */),
}

#[derive(Debug)]
enum ConnectThreadResponses {
    OpenedDevice,
    PeekResult(Result<u32, BridgeError>),
    PokeResult(Result<(), BridgeError>),
    Exiting,
}

impl Clone for UsbBridge {
    fn clone(&self) -> Self {
        UsbBridge {
            usb_pid: self.usb_pid,
            usb_vid: self.usb_vid,
            main_tx: self.main_tx.clone(),
            main_rx: self.main_rx.clone(),
            mutex: self.mutex.clone(),
            poll_thread: None,
        }
    }
}

impl UsbBridge {
    pub fn new(cfg: &UsbBridgeConfig) -> Result<Self, BridgeError> {
        let usb_ctx = libusb_wishbone_tool::Context::new()?;
        let (main_tx, thread_rx) = channel();
        let cv = Arc::new((Mutex::new(None), Condvar::new()));

        let thr_cfg = cfg.clone();
        let thr_cv = cv.clone();
        let poll_thread = Some(thread::spawn(move || {
            Self::usb_poll_thread(
                usb_ctx, thr_cv, thread_rx, thr_cfg, 0x43,
            )
        }));

        Ok(UsbBridge {
            usb_pid: cfg.pid,
            usb_vid: cfg.vid,
            main_tx,
            main_rx: cv,
            mutex: Arc::new(Mutex::new(())),
            poll_thread,
        })
    }

    fn device_matches(
        device: &libusb_wishbone_tool::Device,
        device_desc: &libusb_wishbone_tool::DeviceDescriptor,
        cfg: &UsbBridgeConfig,
    ) -> bool {
        if let Some(pid) = cfg.pid {
            if pid != device_desc.product_id() {
                return false;
            }
        }
        if let Some(vid) = cfg.vid {
            if vid != device_desc.vendor_id() {
                return false;
            }
        }
        if let Some(bus) = cfg.bus {
            if bus != device.bus_number() {
                return false;
            }
        }
        if let Some(device_id) = cfg.device {
            if device_id != device.address() {
                return false;
            }
        }
        true
    }

    pub fn mutex(&self) -> &Arc<Mutex<()>> {
        &self.mutex
    }

    pub fn connect(&self) -> Result<(), BridgeError> {
        self.main_tx
            .send(ConnectThreadRequests::StartPolling(
                self.usb_pid,
                self.usb_vid,
            ))
            .unwrap();
        loop {
            let &(ref lock, ref cvar) = &*self.main_rx;
            let mut _mtx = lock.lock().unwrap();
            *_mtx = None;
            while _mtx.is_none() {
                _mtx = cvar.wait(_mtx).unwrap();
            }
            if let Some(ConnectThreadResponses::OpenedDevice) = _mtx.take() {
                return Ok(());
            }
        }
    }

    fn usb_poll_thread(
        usb_ctx: libusb_wishbone_tool::Context,
        tx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
        rx: Receiver<ConnectThreadRequests>,
        mut cfg: UsbBridgeConfig,
        debug_byte: u8,
    ) {
        let mut print_waiting_message = true;
        let mut first_open = true;
        let &(ref response, ref cvar) = &*tx;
        loop {
            let devices = usb_ctx.devices().unwrap();
            for device in devices.iter() {
                let device_desc = device.device_descriptor().unwrap();
                if Self::device_matches(&device, &device_desc, &cfg) {
                    let usb = match device.open() {
                        Ok(o) => {
                            info!(
                                "opened USB device device {:03} on bus {:03}",
                                device.address(),
                                device.bus_number()
                            );
                            if first_open {
                                *response.lock().unwrap() =
                                    Some(ConnectThreadResponses::OpenedDevice);
                                cvar.notify_one();
                                first_open = false;
                            }
                            print_waiting_message = true;
                            o
                        }
                        Err(e) => {
                            error!("unable to open usb device: {:?}", e);
                            continue;
                        }
                    };
                    let mut keep_going = true;
                    while keep_going {
                        let var = rx.recv();
                        match var {
                            Err(e) => panic!("error in connect thread: {}", e),
                            Ok(o) => match o {
                                ConnectThreadRequests::Exit => {
                                    debug!("usb_poll_thread requested exit");
                                    *response.lock().unwrap() =
                                        Some(ConnectThreadResponses::Exiting);
                                    cvar.notify_one();
                                    return;
                                }
                                ConnectThreadRequests::StartPolling(p, v) => {
                                    cfg.pid = p;
                                    cfg.vid = v;
                                }
                                ConnectThreadRequests::Peek(addr) => {
                                    let result = Self::do_peek(&usb, addr, debug_byte);
                                    keep_going = result.is_ok();
                                    *response.lock().unwrap() =
                                        Some(ConnectThreadResponses::PeekResult(result));
                                    cvar.notify_one();
                                }
                                ConnectThreadRequests::Poke(addr, val) => {
                                    let result = Self::do_poke(&usb, addr, val, debug_byte);
                                    keep_going = result.is_ok();
                                    *response.lock().unwrap() =
                                        Some(ConnectThreadResponses::PokeResult(result));
                                    cvar.notify_one();
                                }
                            },
                        }
                    }
                }
            }

            // Only print out the message the first time.
            // This value gets re-set to `true` whenever there
            // is a successful USB connection.
            if print_waiting_message {
                info!("waiting for target device");
                print_waiting_message = false;
            }
            thread::park_timeout(Duration::from_millis(500));

            // Respond to any messages in the buffer with NotConnected.  As soon
            // as the channel is empty, loop back to the start of this function.
            loop {
                match rx.try_recv() {
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => panic!("main thread disconnected"),
                    Ok(m) => match m {
                        ConnectThreadRequests::Exit => {
                            debug!("main thread requested exit");
                            *response.lock().unwrap() = Some(ConnectThreadResponses::Exiting);
                            cvar.notify_one();
                            return;
                        }
                        ConnectThreadRequests::Peek(_addr) => {
                            *response.lock().unwrap() = Some(ConnectThreadResponses::PeekResult(
                                Err(BridgeError::NotConnected),
                            ));
                            cvar.notify_one();
                        }
                        ConnectThreadRequests::Poke(_addr, _val) => {
                            *response.lock().unwrap() = Some(ConnectThreadResponses::PokeResult(
                                Err(BridgeError::NotConnected),
                            ));
                            cvar.notify_one();
                        }
                        ConnectThreadRequests::StartPolling(p, v) => {
                            cfg.pid = p;
                            cfg.vid = v;
                        }
                    },
                }
            }
        }
    }

    fn do_poke(
        usb: &libusb_wishbone_tool::DeviceHandle,
        addr: u32,
        value: u32,
        debug_byte: u8,
    ) -> Result<(), BridgeError> {
        let mut data_val = [0; 4];
        data_val[0] = (value & 0xff) as u8;
        data_val[1] = ((value >> 8) & 0xff) as u8;
        data_val[2] = ((value >> 16) & 0xff) as u8;
        data_val[3] = ((value >> 24) & 0xff) as u8;
        match usb.write_control(
            debug_byte,
            0,
            (addr & 0xffff) as u16,
            ((addr >> 16) & 0xffff) as u16,
            &data_val,
            Duration::from_millis(100),
        ) {
            Err(e) => {
                debug!("POKE @ {:08x}: usb error {:?}", addr, e);
                Err(BridgeError::USBError(e))
            }
            Ok(len) => {
                if len != 4 {
                    debug!(
                        "POKE @ {:08x}: length error: expected 4 bytes, got {} bytes",
                        addr, len
                    );
                    Err(BridgeError::LengthError(4, len))
                } else {
                    debug!("POKE @ {:08x} -> {:08x}", addr, value);
                    Ok(())
                }
            }
        }
    }

    fn do_peek(
        usb: &libusb_wishbone_tool::DeviceHandle,
        addr: u32,
        debug_byte: u8,
    ) -> Result<u32, BridgeError> {
        let mut data_val = [0; 512];
        match usb.read_control(
            0x80 | debug_byte,
            0,
            (addr & 0xffff) as u16,
            ((addr >> 16) & 0xffff) as u16,
            &mut data_val,
            Duration::from_millis(500),
        ) {
            Err(e) => {
                debug!("PEEK @ {:08x}: usb error {:?}", addr, e);
                Err(BridgeError::USBError(e))
            }
            Ok(len) => {
                if len != 4 {
                    debug!(
                        "PEEK @ {:08x}: length error: expected 4 bytes, got {} bytes",
                        addr, len
                    );
                    Err(BridgeError::LengthError(4, len))
                } else {
                    let value = ((data_val[3] as u32) << 24)
                        | ((data_val[2] as u32) << 16)
                        | ((data_val[1] as u32) << 8)
                        | (data_val[0] as u32);
                    debug!("PEEK @ {:08x} = {:08x}", addr, value);
                    Ok(value)
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
        // If this is the last reference to the bridge, tell the control thread
        // to exit.
        let sc = Arc::strong_count(&self.mutex);
        let wc = Arc::weak_count(&self.mutex);
        debug!("strong count: {}  weak count: {}", sc, wc);
        if (sc + wc) <= 1 {
            let &(ref lock, ref cvar) = &*self.main_rx;
            let mut mtx = lock.lock().unwrap();
            self.main_tx
                .send(ConnectThreadRequests::Exit)
                .expect("Unable to send Exit request to thread");

            // Get a response back from the poll thread
            while mtx.is_none() {
                mtx = cvar.wait(mtx).unwrap();
            }
            match mtx.take() {
                Some(ConnectThreadResponses::Exiting) => (),
                e => {
                    error!("unexpected bridge exit response: {:?}", e);
                }
            }
            if let Some(pt) = self.poll_thread.take() {
                pt.join().expect("Unable to join polling thread");
            }
        }
    }
}
