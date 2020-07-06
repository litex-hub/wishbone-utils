use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use log::{debug, error, info};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use serialport::prelude::*;

use crate::{Bridge, BridgeConfig, BridgeError};

/// The default baud rate for the serial port. To change, call `set_baud()`
pub const DEFAULT_BAUD_RATE: u32 = 115_200;

/// Describes a connection to a UART or serial port
#[derive(Clone)]
pub struct UartBridge {
    serial_port: PathBuf,
    baud: u32,
}

impl UartBridge {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<UartBridge, BridgeError> {
        if !path.as_ref().exists() {
            return Err(BridgeError::InvalidAddress);
        }
        Ok(UartBridge {
            serial_port: path.as_ref().to_path_buf(),
            baud: DEFAULT_BAUD_RATE,
        })
    }

    pub fn baud(&mut self, new_baud: u32) -> &mut UartBridge {
        self.baud = new_baud;
        self
    }

    pub fn create(&self) -> Result<Bridge, BridgeError> {
        Bridge::new(BridgeConfig::UartBridge(self.clone()))
    }
}

pub struct UartBridgeInner {
    path: PathBuf,
    baudrate: u32,
    main_tx: Sender<ConnectThreadRequests>,
    main_rx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
    mutex: Arc<Mutex<()>>,
    poll_thread: Option<thread::JoinHandle<()>>,
}

impl Clone for UartBridgeInner {
    fn clone(&self) -> Self {
        UartBridgeInner {
            path: self.path.clone(),
            baudrate: self.baudrate,
            main_tx: self.main_tx.clone(),
            main_rx: self.main_rx.clone(),
            mutex: self.mutex.clone(),
            poll_thread: None,
        }
    }
}

enum ConnectThreadRequests {
    StartPolling(PathBuf /* path */, u32 /* baudrate */),
    Exit,
    Poke(u32 /* addr */, u32 /* val */),
    Peek(u32 /* addr */),
}

#[derive(Debug)]
enum ConnectThreadResponses {
    Exiting,
    OpenedDevice,
    PeekResult(Result<u32, BridgeError>),
    PokeResult(Result<(), BridgeError>),
}

impl UartBridgeInner {
    pub fn new(cfg: &UartBridge) -> Result<Self, BridgeError> {
        let (main_tx, thread_rx) = channel();
        let cv = Arc::new((Mutex::new(None), Condvar::new()));

        let path = cfg.serial_port.clone();
        let baudrate = cfg.baud;

        let thr_cv = cv.clone();
        let thr_path = path.clone();
        let poll_thread = Some(thread::spawn(move || {
            Self::serial_connect_thread(thr_cv, thread_rx, thr_path, baudrate)
        }));

        Ok(UartBridgeInner {
            path,
            baudrate,
            main_tx,
            main_rx: cv,
            mutex: Arc::new(Mutex::new(())),
            poll_thread,
        })
    }

    fn serial_connect_thread(
        tx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
        rx: Receiver<ConnectThreadRequests>,
        path: PathBuf,
        baud: u32,
    ) {
        let mut path = path;
        let mut baud = baud;
        let mut print_waiting_message = true;
        let mut first_run = true;
        let &(ref response, ref cvar) = &*tx;
        loop {
            let mut port = match serialport::open(&path) {
                Ok(port) => {
                    info!("Re-opened serial device {}", path.display());
                    if first_run {
                        *response.lock().unwrap() = Some(ConnectThreadResponses::OpenedDevice);
                        first_run = false;
                        cvar.notify_one();
                    }
                    print_waiting_message = true;
                    port
                }
                Err(e) => {
                    if print_waiting_message {
                        print_waiting_message = false;
                        error!(
                            "unable to open serial device, will wait for it to appear again: {}",
                            e
                        );
                    }
                    thread::park_timeout(Duration::from_millis(500));
                    continue;
                }
            };
            port.set_baud_rate(baud as _)
                .unwrap_or_else(|e| error!("unable to set serial port speed: {}", e));
            port.set_data_bits(DataBits::Eight)
                .unwrap_or_else(|e| error!("unable to set data bits: {}", e));
            port.set_parity(Parity::None)
                .unwrap_or_else(|e| error!("unable to set parity: {}", e));
            port.set_stop_bits(StopBits::One)
                .unwrap_or_else(|e| error!("unable to set stop bits: {}", e));
            port.set_flow_control(FlowControl::None)
                .unwrap_or_else(|e| error!("unable to set flow control: {}", e));
            if let Err(e) = port.set_timeout(Duration::from_millis(1000)) {
                error!("unable to set port duration timeout: {}", e);
            }

            let mut keep_going = true;
            let mut result_error = "".to_owned();
            while keep_going {
                let var = rx.recv();
                match var {
                    Err(_) => {
                        error!("connection closed");
                        return;
                    }
                    Ok(o) => match o {
                        ConnectThreadRequests::Exit => {
                            debug!("serial_connect_thread requested exit");
                            *response.lock().unwrap() = Some(ConnectThreadResponses::Exiting);
                            cvar.notify_one();
                            return;
                        }
                        ConnectThreadRequests::StartPolling(p, v) => {
                            path = p.clone();
                            baud = v;
                        }
                        ConnectThreadRequests::Peek(addr) => {
                            let result = Self::do_peek(&mut port, addr);
                            if let Err(err) = &result {
                                result_error = format!("peek {:?} @ {:08x}", err, addr);
                                keep_going = false;
                            }
                            *response.lock().unwrap() =
                                Some(ConnectThreadResponses::PeekResult(result));
                            cvar.notify_one();
                        }
                        ConnectThreadRequests::Poke(addr, val) => {
                            let result = Self::do_poke(&mut port, addr, val);
                            if let Err(err) = &result {
                                result_error = format!("poke {:?} @ {:08x}", err, addr);
                                keep_going = false;
                            }
                            *response.lock().unwrap() =
                                Some(ConnectThreadResponses::PokeResult(result));
                            cvar.notify_one();
                        }
                    },
                }
            }
            error!("serial port was closed: {}", result_error);
            thread::park_timeout(Duration::from_millis(500));

            // Respond to any messages in the buffer with NotConnected.  As soon
            // as the channel is empty, loop back to the start of this function.
            loop {
                match rx.try_recv() {
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => panic!("main thread disconnected"),
                    Ok(m) => match m {
                        ConnectThreadRequests::Exit => {
                            *response.lock().unwrap() = Some(ConnectThreadResponses::Exiting);
                            cvar.notify_one();
                            debug!("main thread requested exit");
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
                            path = p.clone();
                            baud = v;
                        }
                    },
                }
            }
        }
    }

    pub fn mutex(&self) -> &Arc<Mutex<()>> {
        &self.mutex
    }

    pub fn connect(&self) -> Result<(), BridgeError> {
        self.main_tx
            .send(ConnectThreadRequests::StartPolling(
                self.path.clone(),
                self.baudrate,
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

    fn do_poke(
        serial: &mut std::boxed::Box<dyn serialport::SerialPort>,
        addr: u32,
        value: u32,
    ) -> Result<(), BridgeError> {
        debug!("POKE @ {:08x} -> {:08x}", addr, value);
        // WRITE, 1 word
        serial.write_all(&[0x01, 0x01])?;

        // LiteX ignores the bottom two Wishbone bits, so shift it by
        // two when writing the address.
        serial.write_u32::<BigEndian>(addr >> 2)?;
        serial.write_u32::<BigEndian>(value)?;
        serial.flush()?;
        Ok(())
    }

    fn do_peek(
        serial: &mut std::boxed::Box<dyn serialport::SerialPort>,
        addr: u32,
    ) -> Result<u32, BridgeError> {
        // READ, 1 word
        debug!("Peeking @ {:08x}", addr);
        serial.write_all(&[0x02, 0x01])?;

        // LiteX ignores the bottom two Wishbone bits, so shift it by
        // two when writing the address.
        serial.write_u32::<BigEndian>(addr >> 2)?;

        let val = serial.read_u32::<BigEndian>()?;
        debug!("PEEK @ {:08x} = {:08x}", addr, val);
        Ok(val)
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

impl Drop for UartBridgeInner {
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

            *mtx = None;
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
