use crate::{Bridge, BridgeConfig, BridgeError};

pub fn get_base(value: &str) -> (&str, u32) {
    if value.starts_with("0x") {
        (value.trim_start_matches("0x"), 16)
    } else if value.starts_with("0X") {
        (value.trim_start_matches("0X"), 16)
    } else if value.starts_with("0b") {
        (value.trim_start_matches("0b"), 2)
    } else if value.starts_with("0B") {
        (value.trim_start_matches("0B"), 2)
    } else if value.starts_with('0') && value != "0" {
        (value.trim_start_matches('0'), 8)
    } else {
        (value, 10)
    }
}

pub fn parse_u32(value: &str) -> Result<u32, String> {
    let (value, base) = get_base(value);
    u32::from_str_radix(value, base)
        .or_else(|e| Err(format!("unable to parse pin '{}': {}", value, e)))
}

#[derive(Clone)]
pub struct SpiBridgeConfig {
    #[allow(dead_code)]
    copi: u8,
    #[allow(dead_code)]
    cipo: Option<u8>,
    #[allow(dead_code)]
    clk: u8,
    #[allow(dead_code)]
    cs: Option<u8>,
}

impl SpiBridgeConfig {
    pub fn new(spec: &str) -> Result<Self, String> {
        let chars: Vec<&str> = spec.split(',').collect();

        let (copi, cipo, clk, cs) = match chars.len() {
            2 => (
                parse_u32(chars[0])? as u8,
                None,
                parse_u32(chars[1])? as u8,
                None,
            ),
            3 => (
                parse_u32(chars[0])? as u8,
                None,
                parse_u32(chars[1])? as u8,
                Some(parse_u32(chars[2])? as u8),
            ),
            4 => (
                parse_u32(chars[0])? as u8,
                Some(parse_u32(chars[1])? as u8),
                parse_u32(chars[2])? as u8,
                Some(parse_u32(chars[3])? as u8),
            ),
            _ => {
                return Err(format!(
                    "{} is not a valid pin spec -- must be COPI,CIPO,CLK,CS (e.g. \"2,3,4,18\")",
                    spec
                ))
            }
        };

        Ok(SpiBridgeConfig {
            copi,
            cipo,
            clk,
            cs,
        })
    }

    pub fn create(&self) -> Result<Bridge, BridgeError> {
        Bridge::new(BridgeConfig::SpiBridge(self.clone()))
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
