pub struct RiscvCpu {

}

#[derive(Debug)]
pub enum RiscvCpuError {
    /// Someone tried to request an unrecognized feature file
    UnrecognizedFile(String /* requested filename */)
}

const TARGET_XML: &str = r#"<?xml version="1.0"?>
<!DOCTYPE target SYSTEM "gdb-target.dtd">
<target version="1.0">
<feature name="org.gnu.gdb.riscv.cpu">
<reg name="x0" bitsize="32" regnum="0" save-restore="no" type="int" group="general"/>
<reg name="x1" bitsize="32" regnum="1" save-restore="no" type="int" group="general"/>
<reg name="x2" bitsize="32" regnum="2" save-restore="no" type="int" group="general"/>
<reg name="x3" bitsize="32" regnum="3" save-restore="no" type="int" group="general"/>
<reg name="x4" bitsize="32" regnum="4" save-restore="no" type="int" group="general"/>
<reg name="x5" bitsize="32" regnum="5" save-restore="no" type="int" group="general"/>
<reg name="x6" bitsize="32" regnum="6" save-restore="no" type="int" group="general"/>
<reg name="x7" bitsize="32" regnum="7" save-restore="no" type="int" group="general"/>
<reg name="x8" bitsize="32" regnum="8" save-restore="no" type="int" group="general"/>
<reg name="x9" bitsize="32" regnum="9" save-restore="no" type="int" group="general"/>
<reg name="x10" bitsize="32" regnum="10" save-restore="no" type="int" group="general"/>
<reg name="x11" bitsize="32" regnum="11" save-restore="no" type="int" group="general"/>
<reg name="x12" bitsize="32" regnum="12" save-restore="no" type="int" group="general"/>
<reg name="x13" bitsize="32" regnum="13" save-restore="no" type="int" group="general"/>
<reg name="x14" bitsize="32" regnum="14" save-restore="no" type="int" group="general"/>
<reg name="x15" bitsize="32" regnum="15" save-restore="no" type="int" group="general"/>
<reg name="x16" bitsize="32" regnum="16" save-restore="no" type="int" group="general"/>
<reg name="x17" bitsize="32" regnum="17" save-restore="no" type="int" group="general"/>
<reg name="x18" bitsize="32" regnum="18" save-restore="no" type="int" group="general"/>
<reg name="x19" bitsize="32" regnum="19" save-restore="no" type="int" group="general"/>
<reg name="x20" bitsize="32" regnum="20" save-restore="no" type="int" group="general"/>
<reg name="x21" bitsize="32" regnum="21" save-restore="no" type="int" group="general"/>
<reg name="x22" bitsize="32" regnum="22" save-restore="no" type="int" group="general"/>
<reg name="x23" bitsize="32" regnum="23" save-restore="no" type="int" group="general"/>
<reg name="x24" bitsize="32" regnum="24" save-restore="no" type="int" group="general"/>
<reg name="x25" bitsize="32" regnum="25" save-restore="no" type="int" group="general"/>
<reg name="x26" bitsize="32" regnum="26" save-restore="no" type="int" group="general"/>
<reg name="x27" bitsize="32" regnum="27" save-restore="no" type="int" group="general"/>
<reg name="x28" bitsize="32" regnum="28" save-restore="no" type="int" group="general"/>
<reg name="x29" bitsize="32" regnum="29" save-restore="no" type="int" group="general"/>
<reg name="x30" bitsize="32" regnum="30" save-restore="no" type="int" group="general"/>
<reg name="x31" bitsize="32" regnum="31" save-restore="no" type="int" group="general"/>
<reg name="pc" bitsize="32" regnum="32" save-restore="no" type="int" group="general"/>
</feature>
<feature name="org.gnu.gdb.riscv.csr">
<reg name="ustatus" bitsize="32" regnum="65" save-restore="no" type="int" group="csr"/>
<reg name="fflags" bitsize="32" regnum="66" save-restore="no" type="int" group="csr"/>
<reg name="frm" bitsize="32" regnum="67" save-restore="no" type="int" group="csr"/>
<reg name="fcsr" bitsize="32" regnum="68" save-restore="no" type="int" group="csr"/>
<reg name="uie" bitsize="32" regnum="69" save-restore="no" type="int" group="csr"/>
<reg name="utvec" bitsize="32" regnum="70" save-restore="no" type="int" group="csr"/>
<reg name="uscratch" bitsize="32" regnum="129" save-restore="no" type="int" group="csr"/>
<reg name="uepc" bitsize="32" regnum="130" save-restore="no" type="int" group="csr"/>
<reg name="ucause" bitsize="32" regnum="131" save-restore="no" type="int" group="csr"/>
<reg name="utval" bitsize="32" regnum="132" save-restore="no" type="int" group="csr"/>
<reg name="uip" bitsize="32" regnum="133" save-restore="no" type="int" group="csr"/>
<reg name="mstatus" bitsize="32" regnum="833" save-restore="no" type="int" group="csr"/>
<reg name="misa" bitsize="32" regnum="834" save-restore="no" type="int" group="csr"/>
<reg name="medeleg" bitsize="32" regnum="835" save-restore="no" type="int" group="csr"/>
<reg name="mideleg" bitsize="32" regnum="836" save-restore="no" type="int" group="csr"/>
<reg name="mie" bitsize="32" regnum="837" save-restore="no" type="int" group="csr"/>
<reg name="mtvec" bitsize="32" regnum="838" save-restore="no" type="int" group="csr"/>
<reg name="mcounteren" bitsize="32" regnum="839" save-restore="no" type="int" group="csr"/>
<reg name="mscratch" bitsize="32" regnum="897" save-restore="no" type="int" group="csr"/>
<reg name="mepc" bitsize="32" regnum="898" save-restore="no" type="int" group="csr"/>
<reg name="mcause" bitsize="32" regnum="899" save-restore="no" type="int" group="csr"/>
<reg name="mtval" bitsize="32" regnum="900" save-restore="no" type="int" group="csr"/>
<reg name="mip" bitsize="32" regnum="901" save-restore="no" type="int" group="csr"/>
<reg name="mtohost" bitsize="32" regnum="1985" save-restore="no" type="int" group="csr"/>
<reg name="mfromhost" bitsize="32" regnum="1986" save-restore="no" type="int" group="csr"/>
<reg name="mreset" bitsize="32" regnum="1987" save-restore="no" type="int" group="csr"/>
<reg name="mipi" bitsize="32" regnum="1988" save-restore="no" type="int" group="csr"/>
<reg name="miobase" bitsize="32" regnum="1989" save-restore="no" type="int" group="csr"/>
<reg name="cycle" bitsize="32" regnum="3137" save-restore="no" type="int" group="csr"/>
<reg name="time" bitsize="32" regnum="3138" save-restore="no" type="int" group="csr"/>
<reg name="instret" bitsize="32" regnum="3139" save-restore="no" type="int" group="csr"/>
<reg name="cycleh" bitsize="32" regnum="3265" save-restore="no" type="int" group="csr"/>
<reg name="timeh" bitsize="32" regnum="3266" save-restore="no" type="int" group="csr"/>
<reg name="instreth" bitsize="32" regnum="3267" save-restore="no" type="int" group="csr"/>
<reg name="mvendorid" bitsize="32" regnum="3922" save-restore="no" type="int" group="csr"/>
<reg name="marchid" bitsize="32" regnum="3923" save-restore="no" type="int" group="csr"/>
<reg name="mimpid" bitsize="32" regnum="3924" save-restore="no" type="int" group="csr"/>
</feature>
</target>
"#;

const THREADS_XML: &str = r#"<?xml version="1.0"?>
<threads>
</threads>"#;

impl RiscvCpu {
    pub fn new() -> Result<RiscvCpu, RiscvCpuError> {
        Ok(RiscvCpu {})
    }

    pub fn get_feature(&self, name: &str) -> Result<Vec<u8>, RiscvCpuError> {
        if name == "target.xml" {
            let xml = TARGET_XML.to_string().into_bytes();
            Ok(xml)
        } else {
            Err(RiscvCpuError::UnrecognizedFile(name.to_string()))
        }
    }
}