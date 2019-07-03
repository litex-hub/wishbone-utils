extern crate byteorder;

use std::io;
use std::io::{Read, Write, Cursor};
use std::net::{TcpListener, TcpStream};

use super::Config;
use super::bridge::{Bridge, BridgeError};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

/* The network protocol looks like this:

    // Packet header:
    wb_buffer[0] = 0x4e;        // Magic byte 0
    wb_buffer[1] = 0x6f;        // Magic byte 1
    wb_buffer[2] = 0x10;        // Version 1, all other flags 0
    wb_buffer[3] = 0x44;        // Address is 32-bits, port is 32-bits
    wb_buffer[4] = 0;           // Padding
    wb_buffer[5] = 0;           // Padding
    wb_buffer[6] = 0;           // Padding
    wb_buffer[7] = 0;           // Padding
    
    // Record header:
    wb_buffer[8] = 0;           // No wishbone flags supported (cyc, wca, wff, etc.)
    wb_buffer[9] = 0x0f;        // Byte enable flag
    wb_buffer[10] = ?;          // Number of write packets
    wb_buffer[11] = ?;          // Numer of read frames
    
    // Write data or read address
    wb_buffer[12] = byte0;
    wb_buffer[13] = byte1;
    wb_buffer[14] = byte2;
    wb_buffer[15] = byte3;

    // Write addres or 0
    wb_buffer[16] = addr0;
    wb_buffer[17] = addr1;
    wb_buffer[18] = addr2;
    wb_buffer[19] = addr3;
*/

pub struct WishboneServer {
    listener: TcpListener,
    connection: Option<TcpStream>,
}

#[derive(Debug)]
pub enum WishboneServerError {
    /// An error with TCP
    IoError(io::Error),

    /// There is no active connection
    ConnectionClosed,

    /// The packet didn't have the magic bytes 0x4e 0x6f
    NoMagic,

    /// The remote side didn't ask for reading or writing
    UnsupportedOperation,

    /// There was a problem with the device bridge
    BridgeError(BridgeError),
}

impl std::convert::From<io::Error> for WishboneServerError {
    fn from(e: io::Error) -> WishboneServerError {
        WishboneServerError::IoError(e)
    }
}

impl std::convert::From<BridgeError> for WishboneServerError {
    fn from(e: BridgeError) -> WishboneServerError {
        WishboneServerError::BridgeError(e)
    }
}

impl WishboneServer {
    pub fn new(cfg: &Config) -> Result<WishboneServer, WishboneServerError> {
        Ok(WishboneServer {
            connection: None,
            listener: TcpListener::bind(format!("{}:{}", cfg.bind_addr, cfg.bind_port))?,
        })
    }

    pub fn connect(&mut self) -> Result<(), WishboneServerError> {
        let (connection, _sockaddr) = self.listener.accept()?;
        self.connection = Some(connection);
        Ok(())
    }

    pub fn process(&mut self, bridge: &Bridge) -> Result<(), WishboneServerError> {
        let mut buffer = [0; 20];
        let mut buffer_offset = 0;
        let mut byte = [0; 1];

        if self.connection.is_none() {
            return Err(WishboneServerError::ConnectionClosed);
        }

        let connection = &mut self.connection.as_mut().unwrap();

        // XXX Replace this with a BufReader for performance
        while buffer_offset < buffer.len() {
            let len = connection.read(&mut byte)?;
            if len == 0 {
                return Err(WishboneServerError::ConnectionClosed);
            }
            buffer[buffer_offset] = byte[0];
            buffer_offset = buffer_offset + 1;
        }

        // Validate signature matches
        if buffer[0] != 0x4e || buffer[1] != 0x6f {
            return Err(WishboneServerError::NoMagic);
        }

        // Figure out if it's a read or a write
        if buffer[10] == 1 {
            // Write
            let mut addr_vec = Cursor::new(vec![buffer[12], buffer[13], buffer[14], buffer[15]]);
            let addr = addr_vec.read_u32::<BigEndian>()?;

            let mut value_vec = Cursor::new(vec![buffer[16], buffer[17], buffer[18], buffer[19]]);
            let value = value_vec.read_u32::<BigEndian>()?;

            bridge.poke(addr, value)?;
            Ok(())
        }
        else if buffer[11] == 1 {
            let mut addr_vec = Cursor::new(vec![buffer[16], buffer[17], buffer[18], buffer[19]]);
            let addr = addr_vec.read_u32::<BigEndian>()?;
            // Read
            let value = bridge.peek(addr)?;
            let mut value_vec = vec![];
            value_vec.write_u32::<BigEndian>(value)?;
            buffer[16] = value_vec[0];
            buffer[17] = value_vec[1];
            buffer[18] = value_vec[2];
            buffer[19] = value_vec[3];
            connection.write(&buffer)?;
            Ok(())
        }
        else {
            return Err(WishboneServerError::UnsupportedOperation);
        }
    }
}