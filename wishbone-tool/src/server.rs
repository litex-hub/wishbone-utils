use super::config::ConfigError;

pub enum ServerKind {
    /// Wishbone bridge
    Wishbone,

    /// GDB server
    GDB,

    /// Send random data back and forth
    RandomTest,

    /// No server
    None,
}

impl ServerKind {
    pub fn from_string(item: &Option<&str>) -> Result<ServerKind, ConfigError> {
        match item {
            None => Ok(ServerKind::None),
            Some(k) => match *k {
                "gdb" => Ok(ServerKind::GDB),
                "wishbone" => Ok(ServerKind::Wishbone),
                "random-test" => Ok(ServerKind::RandomTest),
                unknown => Err(ConfigError::UnknownServerKind(unknown.to_owned())),
            },
        }
    }
}