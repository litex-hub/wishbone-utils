use std::num::ParseIntError;

pub fn get_base(value: &str) -> (&str, u32) {
    if value.starts_with("0x") {
        (value.trim_start_matches("0x"), 16)
    } else if value.starts_with("0X") {
        (value.trim_start_matches("0X"), 16)
    } else if value.starts_with("0b") {
        (value.trim_start_matches("0b"), 2)
    } else if value.starts_with("0B") {
        (value.trim_start_matches("0B"), 2)
    } else if value.starts_with("0") && value != "0" {
        (value.trim_start_matches("0"), 8)
    } else {
        (value, 10)
    }
}

pub fn parse_u16(value: &str) -> Result<u16, ParseIntError> {
    let (value, base) = get_base(value);
    u16::from_str_radix(value, base)
}

pub fn parse_u32(value: &str) -> Result<u32, ParseIntError> {
    let (value, base) = get_base(value);
    u32::from_str_radix(value, base)
}