extern crate rppal;
extern crate spin_sleep;

use std::fmt;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use log::{debug, error, info};

use rppal::gpio::Mode::{Input, Output};
use rppal::gpio::{Gpio, IoPin};

use crate::{BridgeError, SpiBridge};

const TIMEOUT_COUNT: u32 = 20000;

struct SpiPins {
    copi: IoPin,
    cipo: Option<IoPin>,
    clk: IoPin,
    cs: Option<IoPin>,
    copi_is_input: bool,
    delay: Duration,
}

impl fmt::Display for SpiPins {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let copi = format!("COPI:{}", self.copi.pin());
        let cipo = if let Some(ref p) = self.cipo {
            format!("CIPO:{}", p.pin())
        } else {
            "none".to_owned()
        };
        let clk = format!("CLK:{}", self.clk.pin());
        let cs = if let Some(ref p) = self.cs {
            format!("CS:{}", p.pin())
        } else {
            "none".to_owned()
        };
        fmt.write_str(&format!("{} {} {} {}", copi, cipo, clk, cs))
    }
}

#[derive(Clone)]
pub struct SpiBridgeInner {
    baudrate: usize,
    main_tx: Sender<ConnectThreadRequests>,
    main_rx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
    mutex: Arc<Mutex<()>>,
}

enum ConnectThreadRequests {
    Exit,
    Poke(u32 /* addr */, u32 /* val */),
    Peek(u32 /* addr */),
}

#[derive(Debug)]
enum ConnectThreadResponses {
    PeekResult(Result<u32, BridgeError>),
    PokeResult(Result<(), BridgeError>),
}

impl SpiBridgeInner {
    pub fn new(cfg: &SpiBridge) -> Result<Self, BridgeError> {
        let (main_tx, thread_rx) = channel();
        let cv = Arc::new((Mutex::new(None), Condvar::new()));

        let pins = cfg.clone();
        let baudrate = 0;

        // Try to open them first, just to make sure we can.
        {
            let gpio = Gpio::new().expect("unable to get gpio ports");
            let _copi = gpio.get(pins.copi).expect("unable to get spi copi pin");
            if let Some(cipo) = pins.cipo {
                let _cipo = Some(gpio.get(cipo).expect("unable to get spi cipo pin"));
            }
            let _clk = gpio.get(pins.clk).expect("unable to get spi clk pin");
            if let Some(cs) = pins.cs {
                let _cs = Some(gpio.get(cs).expect("unable to get spi cs pin"));
            }
        }

        let thr_cv = cv.clone();
        let thr_copi = pins.copi.clone();
        let thr_cipo = pins.cipo.clone();
        let thr_clk = pins.clk.clone();
        let thr_cs = pins.cs.clone();
        thread::spawn(move || {
            Self::spi_connect_thread(thr_cv, thread_rx, thr_copi, thr_cipo, thr_clk, thr_cs)
        });

        Ok(SpiBridgeInner {
            baudrate,
            main_tx,
            main_rx: cv,
            mutex: Arc::new(Mutex::new(())),
        })
    }

    fn spi_connect_thread(
        tx: Arc<(Mutex<Option<ConnectThreadResponses>>, Condvar)>,
        rx: Receiver<ConnectThreadRequests>,
        copi: u8,
        cipo: Option<u8>,
        clk: u8,
        cs: Option<u8>,
    ) {
        use ConnectThreadRequests::*;
        use ConnectThreadResponses::*;
        let &(ref response, ref cvar) = &*tx;
        loop {
            let gpio = Gpio::new().expect("unable to get gpio ports");
            let mut copi_pin = gpio
                .get(copi)
                .expect("unable to get spi copi pin")
                .into_io(Output);
            copi_pin.set_high();
            let cipo_pin = if let Some(cipo) = cipo {
                Some(
                    gpio.get(cipo)
                        .expect("unable to get spi cipo pin")
                        .into_io(Input),
                )
            } else {
                None
            };
            let mut clk_pin = gpio
                .get(clk)
                .expect("unable to get spi clk pin")
                .into_io(Output);
            clk_pin.set_low();
            let cs_pin = if let Some(cs) = cs {
                let mut pin = gpio
                    .get(cs)
                    .expect("unable to get spi cs pin")
                    .into_io(Output);
                pin.set_high();
                Some(pin)
            } else {
                None
            };
            let mut pins = SpiPins {
                copi: copi_pin,
                cipo: cipo_pin,
                clk: clk_pin,
                cs: cs_pin,
                copi_is_input: false,
                delay: Duration::from_nanos(333),
            };
            info!("re-initialized spi device with pins {}", pins);

            let mut keep_going = true;
            while keep_going {
                let var = rx.recv();
                match var {
                    Err(_) => {
                        error!("connection closed");
                        return;
                    }
                    Ok(o) => match o {
                        Exit => {
                            debug!("spi_connect_thread requested exit");
                            return;
                        }
                        Peek(addr) => {
                            let result = Self::do_peek(&mut pins, addr);
                            keep_going = result.is_ok();
                            *response.lock().unwrap() = Some(PeekResult(result));
                            cvar.notify_one();
                        }
                        Poke(addr, val) => {
                            let result = Self::do_poke(&mut pins, addr, val);
                            keep_going = result.is_ok();
                            *response.lock().unwrap() = Some(PokeResult(result));
                            cvar.notify_one();
                        }
                    },
                }
            }

            thread::sleep(Duration::from_millis(50));

            // Respond to any messages in the buffer with NotConnected.  As soon
            // as the channel is empty, loop back to the start of this function.
            loop {
                match rx.try_recv() {
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => panic!("main thread disconnected"),
                    Ok(m) => match m {
                        Exit => {
                            debug!("main thread requested exit");
                            return;
                        }
                        Peek(_addr) => {
                            *response.lock().unwrap() =
                                Some(PeekResult(Err(BridgeError::NotConnected)));
                            cvar.notify_one();
                        }
                        Poke(_addr, _val) => {
                            *response.lock().unwrap() =
                                Some(PokeResult(Err(BridgeError::NotConnected)));
                            cvar.notify_one();
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
        Ok(())
    }

    /// Get the appropriate input pin.  If COPI is the input, ensure that
    /// it is set as an Input.
    fn get_input(pins: &mut SpiPins) -> (&mut IoPin, &mut IoPin, &Duration) {
        // If there's a CIPO pin, use that.
        // Otherwise, turn COPI into an output if necessary.
        if let Some(ref mut pin) = pins.cipo {
            (pin, &mut pins.clk, &pins.delay)
        } else {
            if !pins.copi_is_input {
                pins.copi.set_mode(Input);
                pins.copi_is_input = true;
            }
            (&mut pins.copi, &mut pins.clk, &pins.delay)
        }
    }

    /// Get the appropriate output pin.  If COPI is the output, ensure that
    /// it is set as an Output.
    fn get_output(pins: &mut SpiPins) -> (&mut IoPin, &mut IoPin, &Duration) {
        // If we're running with less than four wires, change the
        // COPI pin to an output if necessary
        if pins.cipo.is_none() && pins.copi_is_input {
            pins.copi.set_mode(Output);
            pins.copi_is_input = false;
        }
        (&mut pins.copi, &mut pins.clk, &pins.delay)
    }

    fn do_start(pins: &mut SpiPins) {
        pins.clk.set_low();
        if pins.cipo.is_none() && pins.copi_is_input {
            pins.copi.set_mode(Output);
        }
        pins.copi.set_low();
        if let Some(cs) = &mut pins.cs {
            cs.set_low();
        } else {
            Self::do_write_byte(pins, 0xab);
        }
    }

    fn do_finish(pins: &mut SpiPins) {
        if let Some(cs) = &mut pins.cs {
            cs.set_high();
        }
        if pins.cipo.is_none() && pins.copi_is_input {
            pins.copi.set_mode(Output);
        }
        pins.copi.set_low();
        pins.clk.set_low();
    }

    fn do_write_byte(pins: &mut SpiPins, b: u8) {
        let (pin, clk, delay) = Self::get_output(pins);
        for i in &[7, 6, 5, 4, 3, 2, 1, 0] {
            clk.set_low();
            spin_sleep::sleep(*delay);
            if (b & ((1 << i) as u8)) == 0 {
                pin.set_low();
            } else {
                pin.set_high();
            }
            clk.set_high();
            spin_sleep::sleep(*delay);
        }
    }

    fn do_read_byte(pins: &mut SpiPins) -> u8 {
        let mut val = 0;

        // If running with less than four wires, use the
        // copi pin in INPUT mode.
        let (pin, clk, delay) = Self::get_input(pins);

        for i in &[7, 6, 5, 4, 3, 2, 1, 0] {
            clk.set_low();
            spin_sleep::sleep(*delay);
            clk.set_high();
            spin_sleep::sleep(*delay);
            if pin.is_high() {
                val = val | ((1 << i) as u8);
            }
        }
        val
    }

    fn do_poke(pins: &mut SpiPins, addr: u32, value: u32) -> Result<(), BridgeError> {
        debug!("poke: writing 0x{:08x} to 0x{:08x}", value, addr);
        let write_cmd = 0;

        Self::do_start(pins);

        // Send the "Write" command
        Self::do_write_byte(pins, write_cmd);

        // Send the "Address"
        for shift in &[24, 16, 8, 0] {
            Self::do_write_byte(pins, (addr >> shift) as u8);
        }

        // Send the "Value"
        for shift in &[24, 16, 8, 0] {
            Self::do_write_byte(pins, (value >> shift) as u8);
        }

        // Wait for the response indicating the write has completed.
        let mut timeout_counter = 0;
        loop {
            let val = Self::do_read_byte(pins);
            if val == write_cmd {
                break;
            }
            if val != 0xff {
                error!("write: val was not {} or 0xff: {:02x}", write_cmd, val);
                return Err(BridgeError::WrongResponse);
            }
            if timeout_counter > TIMEOUT_COUNT {
                Self::do_finish(pins);
                return Err(BridgeError::Timeout);
            }
            timeout_counter = timeout_counter + 1;
        }

        Self::do_finish(pins);
        Ok(())
    }

    fn do_peek(pins: &mut SpiPins, addr: u32) -> Result<u32, BridgeError> {
        let read_cmd = 1;
        Self::do_start(pins);

        // Send the "Read" command
        Self::do_write_byte(pins, read_cmd);

        // Send the "Address"
        for shift in &[24, 16, 8, 0] {
            Self::do_write_byte(pins, (addr >> shift) as u8);
        }

        // Wait for the response indicating the write has completed.
        let mut timeout_counter = 0;
        loop {
            let val = Self::do_read_byte(pins);
            // warn!("read: val was 0x{:02x}", val);
            if val == read_cmd {
                break;
            }
            if val != 0xff {
                error!("read: val was not {} or 0xff: {:02x}", read_cmd, val);
                return Err(BridgeError::WrongResponse);
            }
            if timeout_counter > TIMEOUT_COUNT {
                Self::do_finish(pins);
                // info!("peek: value ???? at addr 0x{:08x}", addr);
                return Err(BridgeError::Timeout);
            }
            timeout_counter = timeout_counter + 1;
        }

        // Send the "Value"
        let mut value: u32 = 0;
        for shift in &[24, 16, 8, 0] {
            let b = Self::do_read_byte(pins);
            value = value | ((b as u32) << shift);
            // warn!("byte {}: 0x{:02x} (value: 0x{:08x}", shift, b, value);
        }

        Self::do_finish(pins);
        debug!("peek: value 0x{:08x} at addr 0x{:08x}", value, addr);
        Ok(value)
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

impl Drop for SpiBridgeInner {
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
