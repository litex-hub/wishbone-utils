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
/// Describes a connection to a SPI bus. Note that not all platforms
/// support SPI connections.
pub struct SpiBridge {
    #[allow(dead_code)]
    copi: u8,
    #[allow(dead_code)]
    cipo: Option<u8>,
    #[allow(dead_code)]
    clk: u8,
    #[allow(dead_code)]
    cs: Option<u8>,
}

/// A builder to create a connection to a target via SPI. These
/// connections are currently only supported on Raspberry Pi through
/// the use of bit-banging. There are interesting opportunities to
/// add support for SPI connections to other platforms.
///
/// ```no_run
/// use wishbone_bridge::SpiBridge;
/// let bridge = SpiBridge::new("2,3,4,18").unwrap().create().unwrap();
/// ```
impl SpiBridge {
    /// Create a new SpiBridge struct with the provided `pinspec`.
    /// This spec is a comma-delimited list of pins to use for the SPI connection.
    /// The number of pins provided indicates the type of connection to use:
    ///
    /// ```text
    /// 2: Use two-wire communication (no chip select, shared I/O line)
    /// 3: Use three-wire communication (chip select, shared I/O line)
    /// 4: Use four-wire communication (chip select, output, and input lines)
    /// ```
    ///
    /// This function returns an error if the spec cannot be parsed.
    pub fn new(pinspec: &str) -> Result<Self, String> {
        let chars: Vec<&str> = pinspec.split(',').collect();

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
                    pinspec
                ))
            }
        };

        Ok(SpiBridge {
            copi,
            cipo,
            clk,
            cs,
        })
    }

    /// Create a `Bridge` struct based on the current configuration.
    /// This will return an error on platforms that do not support SPI.
    pub fn create(&self) -> Result<Bridge, BridgeError> {
        Bridge::new(BridgeConfig::SpiBridge(self.clone()))
    }
}

#[cfg(all(target_os = "linux", any(target_arch = "arm", target_arch = "aarch64")))]
pub mod raspberry_spi;
#[cfg(all(target_os = "linux", any(target_arch = "arm", target_arch = "aarch64")))]
pub use raspberry_spi::SpiBridgeInner;

#[cfg(not(all(target_os = "linux", any(target_arch = "arm", target_arch = "aarch64"))))]
pub mod dummy_spi;
#[cfg(not(all(target_os = "linux", any(target_arch = "arm", target_arch = "aarch64"))))]
pub use dummy_spi::SpiBridgeInner;
