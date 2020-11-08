use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use log::{debug, error, info};

use crate::{Bridge, BridgeConfig, BridgeError};

/// Connect to a target device via USB.
#[derive(Clone, Default, Debug)]
pub struct UsbBridge {
    /// If specified, indicate the USB product ID to match.
    pid: Option<u16>,

    /// If specified, indicate the USB vendor ID to match.
    vid: Option<u16>,

    /// If specified, indicate the USB bus number to look for.
    bus: Option<u8>,

    /// If specified, indicate the USB device number to look for.
    device: Option<u8>,
}

/// A builder to create a connection to a target via USB. You should
/// specify at least a USB VID or PID in order to avoid connecting
/// to any random device on your system.
///
/// ```no_run
/// use wishbone_bridge::UsbBridge;
/// let bridge = UsbBridge::new().pid(0x1234).create().unwrap();
/// ```
impl UsbBridge {
    /// Create a new `UsbBridge` object that will attempt to connect to
    /// any USB device on the system.
    pub fn new() -> UsbBridge {
        UsbBridge {
            pid: None,
            vid: None,
            bus: None,
            device: None,
        }
    }

    /// Specify a USB PID to connect to.
    pub fn pid(&mut self, pid: u16) -> &mut UsbBridge {
        self.pid = Some(pid);
        self
    }

    /// Specify a USB VID to connect to.
    pub fn vid(&mut self, vid: u16) -> &mut UsbBridge {
        self.vid = Some(vid);
        self
    }

    /// Limit connections to a specific USB bus number.
    pub fn bus(&mut self, bus: u8) -> &mut UsbBridge {
        self.bus = Some(bus);
        self
    }

    /// Limit connections to a specific USB device.
    pub fn device(&mut self, device: u8) -> &mut UsbBridge {
        self.device = Some(device);
        self
    }

    /// Create a bridge based on the current configuration.
    pub fn create(&self) -> Result<Bridge, BridgeError> {
        Bridge::new(BridgeConfig::UsbBridge(self.clone()))
    }
}

pub struct UsbBridgeInner {
    usb_pid: Option<u16>,
    usb_vid: Option<u16>,
    main_tx: Sender<ConnectThreadRequests>,
    main_rx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
    mutex: Arc<Mutex<()>>,
    poll_thread: Option<thread::JoinHandle<()>>,
}

#[derive(Debug)]
enum ConnectThreadRequests {
    StartPolling(Option<u16> /* vid */, Option<u16> /* pid */),
    Exit,
    Poke(u32 /* addr */, u32 /* val */),
    Peek(u32 /* addr */),
    BurstRead(u32 /* addr */, u32 /* len */),
    BurstWrite(u32 /* addr */, Vec<u8> /* write data */)
}

#[derive(Debug)]
enum ConnectThreadResponses {
    OpenedDevice,
    PeekResult(Result<u32, BridgeError>),
    BurstReadResult(Result<Vec<u8>, BridgeError>),
    BurstWriteResult(Result<(), BridgeError>),
    PokeResult(Result<(), BridgeError>),
    Exiting,
}

impl Clone for UsbBridgeInner {
    fn clone(&self) -> Self {
        UsbBridgeInner {
            usb_pid: self.usb_pid,
            usb_vid: self.usb_vid,
            main_tx: self.main_tx.clone(),
            main_rx: self.main_rx.clone(),
            mutex: self.mutex.clone(),
            poll_thread: None,
        }
    }
}

impl UsbBridgeInner {
    pub fn new(cfg: &UsbBridge) -> Result<Self, BridgeError> {
        let usb_ctx = libusb_wishbone_tool::Context::new()?;
        let (main_tx, thread_rx) = channel();
        let cv = Arc::new((Mutex::new(None), Condvar::new()));

        let thr_cfg = cfg.clone();
        let thr_cv = cv.clone();
        let poll_thread = Some(thread::spawn(move || {
            Self::usb_poll_thread(usb_ctx, thr_cv, thread_rx, thr_cfg, 0x43)
        }));

        Ok(UsbBridgeInner {
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
        cfg: &UsbBridge,
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
        loop {
            let &(ref lock, ref cvar) = &*self.main_rx;
            let mut _mtx = lock.lock().unwrap();
            self.main_tx
            .send(ConnectThreadRequests::StartPolling(
                self.usb_pid,
                self.usb_vid,
            ))
            .unwrap();
            *_mtx = None;
            while _mtx.is_none() {
                _mtx = cvar.wait(_mtx).unwrap();
            }
            let result = _mtx.take();
            if let Some(ConnectThreadResponses::OpenedDevice) = result {
                return Ok(());
            }
        }
    }

    fn usb_poll_thread(
        usb_ctx: libusb_wishbone_tool::Context,
        tx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
        rx: Receiver<ConnectThreadRequests>,
        mut cfg: UsbBridge,
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
                                ConnectThreadRequests::BurstRead(addr, len) => {
                                    let result = Self::do_burst_read(&usb, addr, len, debug_byte);
                                    keep_going = result.is_ok();
                                    *response.lock().unwrap() =
                                        Some(ConnectThreadResponses::BurstReadResult(result));
                                    cvar.notify_one();
                                }
                                ConnectThreadRequests::BurstWrite(addr, data) => {
                                    let result = Self::do_burst_write(&usb, addr, data, debug_byte);
                                    keep_going = result.is_ok();
                                    *response.lock().unwrap() =
                                        Some(ConnectThreadResponses::BurstWriteResult(result));
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
                        ConnectThreadRequests::BurstRead(_addr, _len) => {
                            *response.lock().unwrap() = Some(ConnectThreadResponses::BurstReadResult(
                                Err(BridgeError::NotConnected),
                            ));
                            cvar.notify_one();
                        }
                        ConnectThreadRequests::BurstWrite(_addr, _data) => {
                            *response.lock().unwrap() = Some(ConnectThreadResponses::BurstWriteResult(
                                Err(BridgeError::NotConnected),
                            ));
                            cvar.notify_one();
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

    fn do_burst_write(
        usb: &libusb_wishbone_tool::DeviceHandle,
        addr: u32,
        data: Vec<u8>,
        debug_byte: u8,
    ) -> Result<(), BridgeError> {
        if data.len() == 0 {
            return Ok(());
        }

        let maxlen = 4096; // spec says...1023 max? but 4096 works.

        let packet_count = data.len() / maxlen + if (data.len() % maxlen) != 0 { 1 } else { 0 };
        for pkt_num in 0..packet_count {
            let cur_addr = addr as usize + pkt_num * maxlen;
            let bufsize = if pkt_num  == (packet_count - 1) {
                if data.len() % maxlen != 0 {
                    data.len() % maxlen
                } else {
                    maxlen
                }
            } else {
                maxlen
            };
            match usb.write_control(
                debug_byte,
                0,
                (cur_addr & 0xffff) as u16,
                ((cur_addr >> 16) & 0xffff) as u16,
                &data[pkt_num * maxlen..pkt_num * maxlen + bufsize],
                Duration::from_millis(500),
            ) {
                Err(e) => {
                    debug!("BURST_WRITE @ {:08x}: usb error {:?}", addr, e);
                    return Err(BridgeError::USBError(e));
                }
                Ok(retlen) => {
                    if retlen != bufsize as usize {
                        debug!(
                            "BURST_WRITE @ {:08x}: length error: expected {} bytes, got {} bytes",
                            addr, bufsize, retlen
                        );
                        return Err(BridgeError::LengthError(bufsize as usize, retlen));
                    }
                }
            }
        }
        Ok(())
    }

    fn do_peek(
        usb: &libusb_wishbone_tool::DeviceHandle,
        addr: u32,
        debug_byte: u8,
    ) -> Result<u32, BridgeError> {
        let mut data_val = [0; 4];
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

    fn do_burst_read(
        usb: &libusb_wishbone_tool::DeviceHandle,
        addr: u32,
        len: u32,
        debug_byte: u8,
    ) -> Result<Vec<u8>, BridgeError> {
        let mut data_val = vec![];

        if len == 0 {
            return Ok(data_val);
        }

        let maxlen = 4096; // spec says...1023 max? but 4096 works.

        let packet_count = len / maxlen + if (len % maxlen) != 0 { 1 } else { 0 };
        for pkt_num in 0..packet_count {
            let cur_addr = addr + pkt_num * maxlen;
            let bufsize = if pkt_num  == (packet_count - 1) {
                if len % maxlen != 0 {
                    len % maxlen
                } else {
                    maxlen
                }
            } else {
                maxlen
            };
            let mut buffer = vec![0; bufsize as usize];
            match usb.read_control(
                0x80 | debug_byte,
                0,
                (cur_addr & 0xffff) as u16,
                ((cur_addr >> 16) & 0xffff) as u16,
                &mut buffer,
                Duration::from_millis(500),
            ) {
                Err(e) => {
                    debug!("BURST_READ @ {:08x}: usb error {:?}", addr, e);
                    return Err(BridgeError::USBError(e));
                }
                Ok(retlen) => {
                    if retlen != bufsize as usize {
                        debug!(
                            "BURST_READ @ {:08x}: length error: expected {} bytes, got {} bytes",
                            addr, bufsize, retlen
                        );
                        return Err(BridgeError::LengthError(bufsize as usize, retlen));
                    } else {
                        for i in 0..data_val.len() {
                            if (i % 16) == 0 {
                               debug!("\nBURST_READ @ {:08x}: ", addr as usize + i);
                            }
                            debug!("{:02x} ", data_val[i]);
                        }
                        data_val.append(&mut buffer);
                    }
                }
            }
        }
        Ok(data_val)
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

    pub fn burst_read(&self, addr: u32, len: u32) -> Result<Vec<u8>, BridgeError> {
        let &(ref lock, ref cvar) = &*self.main_rx;
        let mut _mtx = lock.lock().unwrap();
        self.main_tx
            .send(ConnectThreadRequests::BurstRead(addr, len))
            .expect("Unable to send burst read to connect thread");
        *_mtx = None;
        while _mtx.is_none() {
            _mtx = cvar.wait(_mtx).unwrap();
        }
        match _mtx.take() {
            Some(ConnectThreadResponses::BurstReadResult(r)) => Ok(r?),
            e => {
                error!("unexpected bridge burst reed response: {:?}", e);
                Err(BridgeError::WrongResponse)
            }
        }
    }

    pub fn burst_write(&self, addr: u32, data: &[u8]) -> Result<(), BridgeError> {
        let &(ref lock, ref cvar) = &*self.main_rx;
        let mut _mtx = lock.lock().unwrap();
        let local_data = data.to_vec();
        self.main_tx
            .send(ConnectThreadRequests::BurstWrite(addr, local_data))
            .expect("Unable to send burst write to connect thread");
        *_mtx = None;
        while _mtx.is_none() {
            _mtx = cvar.wait(_mtx).unwrap();
        }
        match _mtx.take() {
            Some(ConnectThreadResponses::BurstWriteResult(r)) => Ok(r?),
            e => {
                error!("unexpected bridge burst write response: {:?}", e);
                Err(BridgeError::WrongResponse)
            }
        }
    }
}

impl Drop for UsbBridgeInner {
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
