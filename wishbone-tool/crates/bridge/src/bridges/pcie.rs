use memmap::{MmapMut, MmapOptions};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use log::{debug, error};

use crate::{Bridge, BridgeConfig, BridgeError};

/// Describes a connection to a target via PCI Express.
#[derive(Clone)]
pub struct PCIeBridge {
    path: PathBuf,
}

/// A builder to create a connection to a target via PCIe. Specify
/// a PCIe resource file as part of the path.
///
/// **Note:** PCIe bridges to not expose the entire Wishbine bus. You
/// will probably need to translate your addresses to take this into
/// account. For example, address `0x0000_1000` on your Wishbone bus
/// may actually correspond to address `0xe000_1000` on your target device.
///
/// ```no_run
/// use wishbone_bridge::PCIeBridge;
/// let bridge = PCIeBridge::new("/sys/devices/pci0001:00/0001:00:07.0/resource0").unwrap().create().unwrap();
/// ```
impl PCIeBridge {
    /// Create a new `PCIeBridge` struct. The file must exist. This does
    /// not check to ensure you have access permissions.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<PCIeBridge, BridgeError> {
        if !path.as_ref().exists() {
            return Err(BridgeError::InvalidAddress);
        }
        Ok(PCIeBridge {
            path: path.as_ref().to_path_buf(),
        })
    }

    /// Create a new `Bridge` with the given file. This will produce
    /// an error if the PCIe device could not be opened.
    pub fn create(&self) -> Result<Bridge, BridgeError> {
        Bridge::new(BridgeConfig::PCIeBridge(self.clone()))
    }
}

impl From<&str> for PCIeBridge {
    fn from(f: &str) -> Self {
        PCIeBridge {
            path: PathBuf::from(f),
        }
    }
}

pub struct PCIeBridgeInner {
    path: PathBuf,
    main_tx: Sender<ConnectThreadRequests>,
    main_rx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
    mutex: Arc<Mutex<()>>,
    poll_thread: Option<thread::JoinHandle<()>>,
}

enum ConnectThreadRequests {
    StartPolling(PathBuf /* new path */),
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

fn mmap_mut_path(path: &Path) -> MmapMut {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("Couldn't open PCIe BAR");
    unsafe {
        MmapOptions::new()
            .map_mut(&file)
            .expect("Couldn't mmap PCIe BAR")
    }
}

impl Clone for PCIeBridgeInner {
    fn clone(&self) -> Self {
        PCIeBridgeInner {
            path: self.path.clone(),
            main_tx: self.main_tx.clone(),
            main_rx: self.main_rx.clone(),
            mutex: self.mutex.clone(),
            poll_thread: None,
        }
    }
}

impl PCIeBridgeInner {
    pub fn new(cfg: &PCIeBridge) -> Result<Self, BridgeError> {
        let (main_tx, thread_rx) = channel();
        let cv = Arc::new((Mutex::new(None), Condvar::new()));

        let path = cfg.path.clone();

        let thr_cv = cv.clone();
        let thr_path = path.clone();
        let poll_thread = Some(thread::spawn(move || {
            Self::pcie_thread(thr_cv, thread_rx, thr_path)
        }));

        Ok(PCIeBridgeInner {
            path,
            main_tx,
            main_rx: cv,
            mutex: Arc::new(Mutex::new(())),
            poll_thread,
        })
    }

    fn pcie_thread(
        tx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
        rx: Receiver<ConnectThreadRequests>,
        mut path: PathBuf,
    ) {
        let mut first_run = true;
        let &(ref response, ref cvar) = &*tx;
        loop {
            let mut mem = mmap_mut_path(&path);

            if first_run {
                *response.lock().unwrap() = Some(ConnectThreadResponses::OpenedDevice);
                first_run = false;
                cvar.notify_one();
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
                            debug!("pcie_thread requested exit");
                            *response.lock().unwrap() = Some(ConnectThreadResponses::Exiting);
                            cvar.notify_one();
                            return;
                        }
                        ConnectThreadRequests::StartPolling(b) => {
                            path = b;
                        }
                        ConnectThreadRequests::Peek(addr) => {
                            let result = Self::do_peek_32(&mut mem, addr);
                            if let Err(err) = &result {
                                result_error = format!("peek {:?} @ {:08x}", err, addr);
                                keep_going = false;
                            }
                            *response.lock().unwrap() =
                                Some(ConnectThreadResponses::PeekResult(result));
                            cvar.notify_one();
                        }
                        ConnectThreadRequests::Poke(addr, val) => {
                            let result = Self::do_poke_32(&mut mem, addr, val);
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
            error!("pcie connection was closed: {}", result_error);
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
                        ConnectThreadRequests::StartPolling(p) => {
                            path = p;
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
            .send(ConnectThreadRequests::StartPolling(self.path.clone()))
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

    fn do_poke_32(mem: &mut MmapMut, addr: u32, value: u32) -> Result<(), BridgeError> {
        debug!("POKE @ {:08x} -> {:08x}", addr, value);
        #[allow(clippy::cast_ptr_alignment)]
        let memory_range = mem.as_mut_ptr() as *mut u32;
        unsafe { memory_range.add(addr as usize / 4).write_volatile(value) };
        Ok(())
    }

    fn do_peek_32(mem: &mut MmapMut, addr: u32) -> Result<u32, BridgeError> {
        #[allow(clippy::cast_ptr_alignment)]
        let memory_range = mem.as_mut_ptr() as *mut u32;
        let val = unsafe { memory_range.add(addr as usize / 4).read_volatile() };
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

impl Drop for PCIeBridgeInner {
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
