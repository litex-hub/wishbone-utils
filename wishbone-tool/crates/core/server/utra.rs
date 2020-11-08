#[allow(dead_code)]

pub mod spinor {
    use core::convert::TryInto;

    pub struct Register {
        /// Offset of this register within this CSR
        pub offset: usize,
    }
    impl Register {
        pub const fn new(offset: usize) -> Register {
            Register { offset }
        }
    }
    pub struct Field {
        /// A bitmask we use to AND to the value, unshifted.
        /// E.g. for a width of `3` bits, this mask would be 0b111.
        mask: usize,
        /// Offset of the first bit in this field
        offset: usize,
        /// A copy of the register address that this field
        /// is a member of. Ideally this is optimized out by the
        /// compiler.
        register: Register,
    }
    impl Field {
        /// Define a new CSR field with the given width at a specified
        /// offset from the start of the register.
        pub const fn new(width: usize, offset: usize, register: Register) -> Field {
            // Asserts don't work in const fn yet.
            // assert!(width != 0, "field width cannot be 0");
            // assert!((width + offset) < 32, "field with and offset must fit within a 32-bit value");
            // It would be lovely if we could call `usize::pow()` in a const fn.
            let mask = match width {
                0 => 0,
                1 => 1,
                2 => 3,
                3 => 7,
                4 => 15,
                5 => 31,
                6 => 63,
                7 => 127,
                8 => 255,
                9 => 511,
                10 => 1023,
                11 => 2047,
                12 => 4095,
                13 => 8191,
                14 => 16383,
                15 => 32767,
                16 => 65535,
                17 => 131071,
                18 => 262143,
                19 => 524287,
                20 => 1048575,
                21 => 2097151,
                22 => 4194303,
                23 => 8388607,
                24 => 16777215,
                25 => 33554431,
                26 => 67108863,
                27 => 134217727,
                28 => 268435455,
                29 => 536870911,
                30 => 1073741823,
                31 => 2147483647,
                32 => 4294967295,
                _ => 0,
            };
            Field {
                mask,
                offset,
                register,
            }
        }
    }
    pub struct CSR<T> {
        base: *mut T,
    }
    impl<T> CSR<T>
    where
        T: core::convert::TryFrom<usize> + core::convert::TryInto<usize> + core::default::Default,
    {
        pub fn new(base: *mut T) -> Self {
            CSR { base }
        }
        /// Read the contents of this register
        pub fn r(&self, reg: Register) -> T {
            let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
            unsafe { usize_base.add(reg.offset).read_volatile() }
                .try_into()
                .unwrap_or_default()
        }
        /// Read a field from this CSR
        pub fn rf(&self, field: Field) -> T {
            let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
            ((unsafe { usize_base.add(field.register.offset).read_volatile() } >> field.offset)
                & field.mask)
                .try_into()
                .unwrap_or_default()
        }
        /// Read-modify-write a given field in this CSR
        pub fn rmwf(&mut self, field: Field, value: T) {
            let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
            let value_as_usize: usize = value.try_into().unwrap_or_default() << field.offset;
            let previous =
                unsafe { usize_base.add(field.register.offset).read_volatile() } & !field.mask;
            unsafe {
                usize_base
                    .add(field.register.offset)
                    .write_volatile(previous | value_as_usize)
            };
        }
        /// Write a given field without reading it first
        pub fn wfo(&mut self, field: Field, value: T) {
            let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
            let value_as_usize: usize = (value.try_into().unwrap_or_default() & field.mask) << field.offset;
            unsafe {
                usize_base
                    .add(field.register.offset)
                    .write_volatile(value_as_usize)
            };
        }
        /// Write the entire contents of a register without reading it first
        pub fn wo(&mut self, reg: Register, value: T) {
            let usize_base: *mut usize = unsafe { core::mem::transmute(self.base) };
            let value_as_usize: usize = value.try_into().unwrap_or_default();
            unsafe { usize_base.add(reg.offset).write_volatile(value_as_usize) };
        }
        /// Zero a field from a provided value
        pub fn zf(&mut self, field: Field, value: T) -> T {
            let value_as_usize: usize = value.try_into().unwrap_or_default();
            (value_as_usize & !(field.mask << field.offset))
                .try_into()
                .unwrap_or_default()
        }
        /// Shift & mask a value to its final field position
        pub fn ms(&mut self, field: Field, value: T) -> T {
            let value_as_usize: usize = value.try_into().unwrap_or_default();
            ((value_as_usize & field.mask) << field.offset)
                .try_into()
                .unwrap_or_default()
        }
    }


    pub const CONFIG: Register = Register::new(0);
    pub const CONFIG_DUMMY: Field = Field::new(5, 0, CONFIG);

    pub const DELAY_CONFIG: Register = Register::new(1);
    pub const DELAY_CONFIG_D: Field = Field::new(5, 0, DELAY_CONFIG);
    pub const DELAY_CONFIG_LOAD: Field = Field::new(1, 5, DELAY_CONFIG);

    pub const DELAY_STATUS: Register = Register::new(2);
    pub const DELAY_STATUS_Q: Field = Field::new(5, 0, DELAY_STATUS);

    pub const COMMAND: Register = Register::new(3);
    pub const COMMAND_WAKEUP: Field = Field::new(1, 0, COMMAND);
    pub const COMMAND_EXEC_CMD: Field = Field::new(1, 1, COMMAND);
    pub const COMMAND_CMD_CODE: Field = Field::new(8, 2, COMMAND);
    pub const COMMAND_HAS_ARG: Field = Field::new(1, 10, COMMAND);
    pub const COMMAND_DUMMY_CYCLES: Field = Field::new(5, 11, COMMAND);
    pub const COMMAND_DATA_WORDS: Field = Field::new(8, 16, COMMAND);
    pub const COMMAND_LOCK_READS: Field = Field::new(1, 24, COMMAND);

    pub const CMD_ARG: Register = Register::new(4);
    pub const CMD_ARG_CMD_ARG: Field = Field::new(32, 0, CMD_ARG);

    pub const CMD_RBK_DATA: Register = Register::new(5);
    pub const CMD_RBK_DATA_CMD_RBK_DATA: Field = Field::new(32, 0, CMD_RBK_DATA);

    pub const STATUS: Register = Register::new(6);
    pub const STATUS_WIP: Field = Field::new(1, 0, STATUS);

    pub const WDATA: Register = Register::new(7);
    pub const WDATA_WDATA: Field = Field::new(16, 0, WDATA);

    pub const EV_STATUS: Register = Register::new(8);
    pub const EV_STATUS_STATUS: Field = Field::new(1, 0, EV_STATUS);

    pub const EV_PENDING: Register = Register::new(9);
    pub const EV_PENDING_PENDING: Field = Field::new(1, 0, EV_PENDING);

    pub const EV_ENABLE: Register = Register::new(10);
    pub const EV_ENABLE_ENABLE: Field = Field::new(1, 0, EV_ENABLE);

    pub const ECC_ADDRESS: Register = Register::new(11);
    pub const ECC_ADDRESS_ECC_ADDRESS: Field = Field::new(32, 0, ECC_ADDRESS);

    pub const ECC_STATUS: Register = Register::new(12);
    pub const ECC_STATUS_ECC_ERROR: Field = Field::new(1, 0, ECC_STATUS);
    pub const ECC_STATUS_ECC_OVERFLOW: Field = Field::new(1, 1, ECC_STATUS);

}
