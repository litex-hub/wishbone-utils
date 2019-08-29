extern crate byteorder;
extern crate rppal;

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, Condvar};
use std::thread;
use std::time::Duration;

use log::{debug, error, info};

use rppal::gpio::{Gpio, IoPin, Mode};
use rppal::gpio::Level::{Low, High};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::bridge::BridgeError;
use crate::config::Config;

struct SpiPins {
    mosi: IoPin,
    miso: Option<IoPin>,
    clk: IoPin,
    cs: Option<IoPin>,
}

#[derive(Clone)]
pub struct SpiBridge {
    mosi: u8,
    miso: Option<u8>,
    clk: u8,
    cs: Option<u8>,
    baudrate: usize,
    main_tx: Sender<ConnectThreadRequests>,
    main_rx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
    mutex: Arc<Mutex<()>>,
}

enum ConnectThreadRequests {
    UpdateConfig(u8 /* mosi */, Option<u8> /* miso */, u8 /* clk */, Option<u8> /* cs */),
    Exit,
    Poke(u32 /* addr */, u32 /* val */),
    Peek(u32 /* addr */),
}

#[derive(Debug)]
enum ConnectThreadResponses {
    OpenedDevice,
    PeekResult(Result<u32, BridgeError>),
    PokeResult(Result<(), BridgeError>),
}

impl SpiBridge {
    pub fn new(cfg: &Config) -> Result<Self, BridgeError> {
        let (main_tx, thread_rx) = channel();
        let cv = Arc::new((Mutex::new(None), Condvar::new()));

        let pins = match &cfg.spi_pins {
            Some(s) => s.clone(),
            None => panic!("no serial port path was found"),
        };
        let baudrate = 0;

        // Try to open them first, just to make sure we can.
        {
            let gpio = Gpio::new().expect("unable to get gpio ports");
            let _mosi = gpio.get(pins.mosi).expect("unable to get spi mosi pin");
            if let Some(miso) = pins.miso {
                let _miso = Some(gpio.get(miso).expect("unable to get spi miso pin"));
            }
            let _clk = gpio.get(pins.clk).expect("unable to get spi clk pin");
            if let Some(cs) = pins.cs {
                let _cs = Some(gpio.get(cs).expect("unable to get spi cs pin"));
            }
        }

        let thr_cv = cv.clone();
        let thr_mosi = pins.mosi.clone();
        let thr_miso = pins.miso.clone();
        let thr_clk = pins.clk.clone();
        let thr_cs = pins.cs.clone();
        thread::spawn(move || {
            Self::spi_connect_thread(thr_cv, thread_rx, thr_mosi, thr_miso, thr_clk, thr_cs)
        });

        Ok(SpiBridge {
            mosi: pins.mosi,
            miso: pins.miso,
            clk: pins.clk,
            cs: pins.cs,
            baudrate,
            main_tx,
            main_rx: cv,
            mutex: Arc::new(Mutex::new(())),
        })
    }

    fn spi_connect_thread(
        tx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
        rx: Receiver<ConnectThreadRequests>,
        mosi: u8,
        miso: Option<u8>,
        clk: u8,
        cs: Option<u8>
    ) {
        let mut miso = miso;
        let mut mosi = mosi;
        let mut clk = clk;
        let mut cs = cs;
        let &(ref response, ref cvar) = &*tx;
        loop {
            let gpio = Gpio::new().expect("unable to get gpio ports");
            let mosi_pin = gpio.get(mosi).expect("unable to get spi mosi pin").into_io(Mode::Output);
            let miso_pin = if let Some(miso) = miso {
                Some(gpio.get(miso).expect("unable to get spi miso pin").into_io(Mode::Input))
            } else {
                None
            };
            let clk_pin = gpio.get(clk).expect("unable to get spi clk pin").into_io(Mode::Output);
            let cs_pin = if let Some(cs) = cs {
                Some(gpio.get(cs).expect("unable to get spi cs pin").into_io(Mode::Output))
            } else {
                None
            };
            let mut pins = SpiPins { mosi: mosi_pin, miso: miso_pin, clk: clk_pin, cs: cs_pin };
            *response.lock().unwrap() = Some(ConnectThreadResponses::OpenedDevice);
            cvar.notify_one();

            let mut keep_going = true;
            while keep_going {
                let var = rx.recv();
                match var {
                    Err(_) => {
                        error!("connection closed");
                        return;
                    },
                    Ok(o) => match o {
                        ConnectThreadRequests::Exit => {
                            debug!("spi_connect_thread requested exit");
                            return;
                        }
                        ConnectThreadRequests::UpdateConfig(i, o, k, s) => {
                            mosi = i;
                            miso = o;
                            clk = k;
                            cs = s;
                            keep_going = false;
                        }
                        ConnectThreadRequests::Peek(addr) => {
                            let result = Self::do_peek(&mut pins, addr);
                            keep_going = result.is_ok();
                            *response.lock().unwrap() = Some(ConnectThreadResponses::PeekResult(result));
                            cvar.notify_one();
                        }
                        ConnectThreadRequests::Poke(addr, val) => {
                            let result = Self::do_poke(&mut pins, addr, val);
                            keep_going = result.is_ok();
                            *response.lock().unwrap() = Some(ConnectThreadResponses::PokeResult(result));
                            cvar.notify_one();
                        }
                    },
                }
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
                        ConnectThreadRequests::UpdateConfig(i, o, k, s) => {
                            mosi = i;
                            miso = o;
                            clk = k;
                            cs = s;
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
            .send(ConnectThreadRequests::UpdateConfig(self.mosi, self.miso, self.clk, self.cs))
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

    fn do_tick(pins: &mut SpiPins) {
        pins.clk.write(Low);
        thread::park_timeout(Duration::from_nanos(83));
        pins.clk.write(High);
        thread::park_timeout(Duration::from_nanos(83));
    }

    fn do_poke(
        pins: &mut SpiPins,
        addr: u32,
        value: u32,
    ) -> Result<(), BridgeError> {
        if let Some(cs) = &mut pins.cs {
            cs.write(Low);
        }
        Self::do_tick(pins);
        // // WRITE, 1 word
        // serial.write(&[0x01, 0x01])?;
        // serial.write_u32::<BigEndian>(addr)?;
        // Ok(serial.write_u32::<BigEndian>(value)?)
        if let Some(cs) = &mut pins.cs {
            cs.write(High);
        }
        Ok(())
    }

    fn do_peek(pins: &mut SpiPins, addr: u32) -> Result<u32, BridgeError> {
        if let Some(cs) = &mut pins.cs {
            cs.write(Low);
        }
        Self::do_tick(pins);
        // // READ, 1 word
        // serial.write(&[0x02, 0x01])?;
        // serial.write_u32::<BigEndian>(addr)?;
        // Ok(serial.read_u32::<BigEndian>()?)
        if let Some(cs) = &mut pins.cs {
            cs.write(High);
        }
        Ok(0)
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

impl Drop for SpiBridge {
    fn drop(&mut self) {
        // If this is the last reference to the bridge, tell the control thread
        // to exit.
        if Arc::strong_count(&self.mutex) + Arc::weak_count(&self.mutex) <= 1 {
            let &(ref lock, ref _cvar) = &*self.main_rx;
            let mut _mtx = lock.lock().unwrap();
            self.main_tx
                .send(ConnectThreadRequests::Exit)
                .expect("Unable to send Exit request to thread");
        }
    }
}
