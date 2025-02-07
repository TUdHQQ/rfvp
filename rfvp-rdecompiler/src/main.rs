use anyhow::Result;
use std::path::Path;

mod ir;
mod cfg;
mod disasm;

fn main() {
    println!("Hello, world!");
}

#[cfg(test)]
mod tests {
    use rfvp_core::format::scenario::Nls;
    use crate::disasm::Disassembler;

    use super::*;

    #[test]
    fn test_disassembler() -> Result<()> {
        let input = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/testcase/Snow.hcb"));
        let mut disassembler = Disassembler::new(input, Nls::ShiftJIS)?;
        disassembler.disassemble()?;

        Ok(())
    }
}