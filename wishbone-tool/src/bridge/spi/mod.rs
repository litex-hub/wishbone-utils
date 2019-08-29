use crate::config::ConfigError;
use crate::config::ConfigError::SpiParseError;
use crate::config::parse_u32;

pub struct SpiPins {
    mosi: u8,
    miso: Option<u8>,
    clk: u8,
    cs: Option<u8>,
}

impl SpiPins {
    pub fn from_string(spec: &str) -> Result<Self, ConfigError> {
        let chars: Vec<&str> = spec.split(",").collect();
        if chars.len() != 4 {
            return Err(SpiParseError(format!("{} is not a valid pin spec -- must be MOSI,MISO,CLK,CS (e.g. \"2,3,4,18\")", spec)))
        }
        let mosi = parse_u32(chars[1])? as u8;
        let miso = Some(parse_u32(chars[0])? as u8);
        let clk = parse_u32(chars[2])? as u8;
        let cs = Some(parse_u32(chars[3])? as u8);

        Ok(SpiPins { mosi, miso, clk, cs})
    }
}

#[cfg(all(target_os = "linux", any(target_arch = "arm", target_arch = "aarch64")))]
pub mod raspberry_spi;
#[cfg(all(target_os = "linux", any(target_arch = "arm", target_arch = "aarch64")))]
pub use raspberry_spi::SpiBridge;

#[cfg(not(all(target_os = "linux", any(target_arch = "arm", target_arch = "aarch64"))))]
pub mod dummy_spi;
#[cfg(not(all(target_os = "linux", any(target_arch = "arm", target_arch = "aarch64"))))]
pub use dummy_spi::SpiBridge;