use crate::config::ConfigError;
use crate::config::ConfigError::SpiParseError;
use crate::config::parse_u32;

pub struct SpiPins {
    miso: u32,
    mosi: u32,
    clk: u32,
    cs: u32,
}

impl SpiPins {
    pub fn from_string(spec: &str) -> Result<Self, ConfigError> {
        let chars: Vec<&str> = spec.split(",").collect();
        if chars.len() != 4 {
            return Err(SpiParseError(format!("{} is not a valid pin spec -- must be MOSI,MISO,CLK,CS (e.g. \"2,3,4,18\")", spec)))
        }
        let miso = parse_u32(chars[0])?;
        let mosi = parse_u32(chars[1])?;
        let clk = parse_u32(chars[2])?;
        let cs = parse_u32(chars[3])?;

        Ok(SpiPins { miso, mosi, clk, cs})
    }
}

#[cfg(any(target_os = "android",
          target_os = "dragonfly",
          target_os = "freebsd",
          target_os = "ios",
          target_os = "linux",
          target_os = "macos",
          target_os = "netbsd",
          target_os = "openbsd"))]
pub mod raspberry_spi;

#[cfg(target_os = "windows")]
pub mod dummy_spi;

#[cfg(any(target_os = "android",
          target_os = "dragonfly",
          target_os = "freebsd",
          target_os = "ios",
          target_os = "linux",
          target_os = "macos",
          target_os = "netbsd",
          target_os = "openbsd"))]
pub use raspberry_spi::SpiBridge;
#[cfg(target_os = "windows")]
pub use dummy_spi::SpiBridge;