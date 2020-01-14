use crate::config::ConfigError;
use crate::config::ConfigError::SpiParseError;
use crate::config::parse_u32;

#[derive(Clone)]
pub struct SpiPins {
    #[allow(dead_code)]
    mosi: u8,
    #[allow(dead_code)]
    miso: Option<u8>,
    #[allow(dead_code)]
    clk: u8,
    #[allow(dead_code)]
    cs: Option<u8>,
}

impl SpiPins {
    pub fn from_string(spec: &str) -> Result<Self, ConfigError> {
        let chars: Vec<&str> = spec.split(",").collect();

        let (mosi, miso, clk, cs) = match chars.len() {
            2 => {
                (
                    parse_u32(chars[0])? as u8,
                    None,
                    parse_u32(chars[1])? as u8,
                    None,
                )
            }
            3 => {
                (
                    parse_u32(chars[0])? as u8,
                    None,
                    parse_u32(chars[1])? as u8,
                    Some(parse_u32(chars[2])? as u8),
                )
            },
            4 => {
                (
                    parse_u32(chars[0])? as u8,
                    Some(parse_u32(chars[1])? as u8),
                    parse_u32(chars[2])? as u8,
                    Some(parse_u32(chars[3])? as u8),
                )
            }
            _ => return Err(SpiParseError(format!("{} is not a valid pin spec -- must be MOSI,MISO,CLK,CS (e.g. \"2,3,4,18\")", spec)))
        };

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
