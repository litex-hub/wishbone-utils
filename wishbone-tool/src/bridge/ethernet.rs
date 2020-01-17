extern crate byteorder;

use std::net::UdpSocket;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, Condvar};
use std::thread;
use std::time::Duration;

use log::{debug, error, info};

use byteorder::{BigEndian, ByteOrder};

use super::BridgeError;
use crate::config::Config;

pub struct EthernetBridge {
    host: String,
    port: u16,
    main_tx: Sender<ConnectThreadRequests>,
    main_rx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
    mutex: Arc<Mutex<()>>,
    poll_thread: Option<thread::JoinHandle<()>>,
}

enum ConnectThreadRequests {
    StartPolling(String /* host */, u16 /* port */),
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

impl Clone for EthernetBridge {
    fn clone(&self) -> Self {
        EthernetBridge {
            host: self.host.clone(),
            port: self.port,
            main_tx: self.main_tx.clone(),
            main_rx: self.main_rx.clone(),
            mutex: self.mutex.clone(),
            poll_thread: None,
        }
    }
}
impl EthernetBridge {
    pub fn new(cfg: &Config) -> Result<Self, BridgeError> {
        let (main_tx, thread_rx) = channel();
        let cv = Arc::new((Mutex::new(None), Condvar::new()));

        let host = match &cfg.ethernet_host {
            Some(h) => h.clone(),
            None => panic!("no ethernet hostname path was found"),
        };
        let port = cfg.ethernet_port;

        let thr_cv = cv.clone();
        let thr_hostname = host.clone();
        let poll_thread = Some(thread::spawn(move || {
            Self::ethernet_thread(thr_cv, thread_rx, thr_hostname, port)
        }));

        Ok(EthernetBridge {
            host,
            port,
            main_tx,
            main_rx: cv,
            mutex: Arc::new(Mutex::new(())),
            poll_thread,
        })
    }

    fn ethernet_thread(
        tx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
        rx: Receiver<ConnectThreadRequests>,
        host: String,
        port: u16
    ) {
        let mut host = host;
        let mut port = port;
        let mut print_waiting_message = true;
        let mut first_run = true;
        let &(ref response, ref cvar) = &*tx;
        loop {
            let mut connection = match UdpSocket::bind(format!("0.0.0.0:{}", port)) {
                Ok(conn) => {
                    info!("Re-opened ethernet host {}:{}", host, port);
                    if first_run {
                        *response.lock().unwrap() = Some(ConnectThreadResponses::OpenedDevice);
                        first_run = false;
                        cvar.notify_one();
                    }
                    print_waiting_message = true;
                    conn
                },
                Err(e) => {
                    if print_waiting_message {
                        print_waiting_message = false;
                        error!("unable to open ethernet host {}:{}, will wait for it to appear again: {}", host, port, e);
                    }
                    thread::park_timeout(Duration::from_millis(500));
                    continue;
                }
            };
            if let Err(e) = connection.set_read_timeout(Some(Duration::from_millis(1000))) {
                error!("unable to set ethernet read duration timeout: {}", e);
            }
            if let Err(e) = connection.set_write_timeout(Some(Duration::from_millis(1000))) {
                error!("unable to set ethernet write duration timeout: {}", e);
            }

            let mut keep_going = true;
            let mut result_error = "".to_owned();
            while keep_going {
                let var = rx.recv();
                match var {
                    Err(_) => {
                        error!("connection closed");
                        return;
                    },
                    Ok(o) => match o {
                        ConnectThreadRequests::Exit => {
                            debug!("ethernet_thread requested exit");
                            *response.lock().unwrap() = Some(ConnectThreadResponses::Exiting);
                            cvar.notify_one();
                            return;
                        }
                        ConnectThreadRequests::StartPolling(h, p) => {
                            host = h.clone();
                            port = p;
                        }
                        ConnectThreadRequests::Peek(addr) => {
                            let result = Self::do_peek(&mut connection, &host, port, addr);
                            if let Err(err) = &result {
                                result_error = format!("peek {:?} @ {:08x}", err, addr);
                                keep_going = false;
                            }
                            *response.lock().unwrap() = Some(ConnectThreadResponses::PeekResult(result));
                            cvar.notify_one();
                        }
                        ConnectThreadRequests::Poke(addr, val) => {
                            let result = Self::do_poke(&mut connection, &host, port, addr, val);
                            if let Err(err) = &result {
                                result_error = format!("poke {:?} @ {:08x}", err, addr);
                                keep_going = false;
                            }
                            *response.lock().unwrap() = Some(ConnectThreadResponses::PokeResult(result));
                            cvar.notify_one();
                        }
                    },
                }
            }
            error!("ethernet connection was closed: {}", result_error);
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
                        ConnectThreadRequests::StartPolling(h, p) => {
                            host = h.clone();
                            port = p;
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
                self.host.clone(),
                self.port,
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

    fn do_poke(
        connection: &mut UdpSocket,
        host: &String,
        port: u16,
        addr: u32,
        value: u32,
    ) -> Result<(), BridgeError> {
        debug!("POKE @ {:08x} -> {:08x}", addr, value);
        let mut buffer: [u8;20] = [

            // 0
            0x4e,       // Magic byte 0
            0x6f,       // Magic byte 1
            0x10,       // Version 1, all other flags 0
            0x44,       // Address is 32-bits, port is 32-bits

            // 4
            0,          // Padding
            0,          // Padding
            0,          // Padding
            0,          // Padding

            // 8 - Record
            0,          // No Wishbone flags are set (cyc, wca, wff, etc.)
            0x0f,       // Byte enable
            1,          // Write count
            0,          // Read count

            // 12 - Address
            0,
            0,
            0,
            0,

            // 16 - Value
            0,
            0,
            0,
            0,
        ];
        BigEndian::write_u32(&mut buffer[12..16], addr);
        BigEndian::write_u32(&mut buffer[16..20], value);
        connection.send_to(&buffer, format!("{}:{}", host, port))?;
        Ok(())
    }

    fn do_peek(connection: &mut UdpSocket, host: &String, port: u16, addr: u32) -> Result<u32, BridgeError> {
        let mut buffer: [u8;20] = [

            // 0
            0x4e,       // Magic byte 0
            0x6f,       // Magic byte 1
            0x10,       // Version 1, all other flags 0
            0x44,       // Address is 32-bits, port is 32-bits

            // 4
            0,          // Padding
            0,          // Padding
            0,          // Padding
            0,          // Padding

            // 8 - Record
            0,          // No Wishbone flags are set (cyc, wca, wff, etc.)
            0x0f,       // Byte enable
            0,          // Write count
            1,          // Read count

            // 12 - Address
            0,
            0,
            0,
            0,

            // 16 - Value
            0,
            0,
            0,
            0,
        ];
        BigEndian::write_u32(&mut buffer[16..20], addr);
        connection.send_to(&buffer, format!("{}:{}", host, port))?;
        let (amt, _src) = connection.recv_from(&mut buffer)?;
        if amt != buffer.len() {
            return Err(BridgeError::LengthError(amt, buffer.len()));
        }
        let val = BigEndian::read_u32(&buffer[16..20]);
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

impl Drop for EthernetBridge {
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
