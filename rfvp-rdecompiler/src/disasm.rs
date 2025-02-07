use crate::ir::{NamedVariant, StackAnalyzer, Statement};
use anyhow::{bail, Result};
use bytes::Bytes;
use rfvp_core::format::scenario::instructions::{inst::*, Opcode, OpcodeBase};
use rfvp_core::format::scenario::{Nls, Scenario};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Function {
    address: u32,
    args_count: u8,
    locals_count: u8,
    statements: Vec<Statement>,
}

pub struct Disassembler {
    scenario: Scenario,
    cursor: usize,
    functions: HashMap<u32, Function>,
    stack_analyzer: Option<StackAnalyzer>,
    current_function_address: Option<u32>,
}

impl Disassembler {
    pub fn new(path: impl AsRef<Path>, nls: Nls) -> Result<Self> {
        let data = std::fs::read(path.as_ref())?;
        let data = Bytes::from(data);
        let scenario = Scenario::new(data, Some(nls))?;
        Ok(Self {
            scenario,
            cursor: 4,
            functions: HashMap::new(),
            stack_analyzer: None,
            current_function_address: None,
        })
    }

    pub fn get_scenario(&self) -> &Scenario {
        &self.scenario
    }

    pub fn get_pc(&self) -> usize {
        self.cursor
    }

    fn rewind(&mut self) {
        self.cursor = 4;
        self.current_function_address = None;
        self.stack_analyzer = None;
    }

    fn push_statement_to_current_function(&mut self, statement: Statement) -> Result<()> {
        if let Some(addr) = self.current_function_address {
            if let Some(func) = self.functions.get_mut(&addr) {
                func.statements.push(statement);
            } else {
                bail!("function not found: {:x}", addr);
            }
        } else {
            bail!("current function address is None");
        }

        Ok(())
    }

    /// 0x00 nop instruction
    /// nop, no operation
    pub fn nop(&mut self) -> Result<()> {
        self.cursor += 1;
        // do nothing here, just skip it

        Ok(())
    }

    pub fn one_byte_bypass(&mut self) -> Result<()> {
        self.cursor += 1;
        Ok(())
    }

    pub fn two_bytes_bypass(&mut self) -> Result<()> {
        self.cursor += 2;
        Ok(())
    }

    pub fn three_bytes_bypass(&mut self) -> Result<()> {
        self.cursor += 3;
        Ok(())
    }

    pub fn five_bytes_bypass(&mut self) -> Result<()> {
        self.cursor += 5;
        Ok(())
    }

    /// 0x01 init stack instruction
    /// initialize the local routine stack, as well as
    /// the post-phase of perforimg call instruction or launching a new routine
    pub fn init_stack(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        // how many arguments are passed to the routine
        self.cursor += size_of::<i8>();

        // how many locals are declared in the routine
        self.cursor += size_of::<i8>();

        // function should be inserted into the map in the previous phase
        if let Some(func) = self.functions.get(&addr) {
            let stack_analyzer =
                StackAnalyzer::new(func.locals_count as u32, func.args_count as u32);

            self.stack_analyzer = Some(stack_analyzer);
            self.current_function_address = Some(addr);
        } else {
            bail!("function not found: {:x}", addr);
        }

        Ok(())
    }

    pub fn init_stack_bypass(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        let args_count = scenario.read_i8(self.cursor)?;
        self.cursor += size_of::<i8>();

        let locals_count = scenario.read_i8(self.cursor)?;
        self.cursor += size_of::<i8>();

        self.functions.insert(
            addr,
            Function {
                address: addr,
                args_count: args_count as u8,
                locals_count: locals_count as u8,
                statements: Vec::new(),
            },
        );

        Ok(())
    }

    pub fn push_string_bypass(&mut self, scenario: &Scenario) -> Result<()> {
        self.cursor += 1;
        let len = scenario.read_u8(self.cursor)? as usize;
        self.cursor += size_of::<u8>();
        self.cursor += len;

        Ok(())
    }

    /// 0x02 call instruction
    /// call a routine
    pub fn call(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;
        let target = scenario.read_u32(self.cursor)?;
        self.cursor += size_of::<u32>();

        let callee_args_count = if let Some(func) = self.functions.get(&target) {
            func.args_count
        } else {
            bail!("function not found: {:x}", target);
        };

        let mut args = Vec::new();
        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            for _ in 0..callee_args_count {
                if let Ok(arg) = stack_analyzer.pop() {
                    args.push(arg);
                } else {
                    bail!("stack analyzer underflow");
                }
            }
        } else {
            bail!("stack analyzer not found");
        }

        let statement = Statement::from_call(addr, target, args);
        self.push_statement_to_current_function(statement)?;

        Ok(())
    }

    /// 0x03 syscall
    /// call a system call
    pub fn syscall(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;
        let id = scenario.read_u16(self.cursor)?;
        self.cursor += size_of::<u16>();

        if let Some(syscall) = scenario.get_syscall(id) {
            let mut args = Vec::new();
            if let Some(stack_analyzer) = &mut self.stack_analyzer {
                for _ in 0..syscall.args {
                    if let Ok(arg) = stack_analyzer.pop() {
                        args.push(arg);
                    } else {
                        bail!("stack analyzer underflow");
                    }
                }
            } else {
                bail!("stack analyzer not found");
            }

            let statement = Statement::from_syscall(addr, syscall.name.clone(), args);
            self.push_statement_to_current_function(statement)?;
        } else {
            bail!("syscall not found: {}", id);
        }

        Ok(())
    }

    /// 0x04 ret instruction
    /// return from a routine
    pub fn ret(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        let statement = Statement::from_return(addr, false);
        self.push_statement_to_current_function(statement)?;

        Ok(())
    }

    /// 0x05 retv instruction
    /// return from a routine with a value
    pub fn retv(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        let statement = Statement::from_return(addr, true);
        self.push_statement_to_current_function(statement)?;

        Ok(())
    }

    /// 0x06 jmp instruction
    /// jump to the address
    pub fn jmp(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;
        let target = scenario.read_u32(self.cursor)?;
        self.cursor += size_of::<u32>();

        let statement = Statement::from_jmp(addr, target);
        self.push_statement_to_current_function(statement)?;

        Ok(())
    }

    /// 0x07 jz instruction
    /// jump to the address if the top of the stack is zero
    pub fn jz(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;
        let target = scenario.read_u32(self.cursor)?;
        self.cursor += size_of::<u32>();

        let condition_var = if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.pop()?
        } else {
            bail!("stack analyzer not found");
        };

        let statement = Statement::from_jz(addr, target, condition_var);
        self.push_statement_to_current_function(statement)?;

        Ok(())
    }

    /// 0x08 push nil
    /// push a nil value onto the stack
    pub fn push_nil(&mut self) -> Result<()> {
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_nil()?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x09 push true
    /// push a true value onto the stack
    pub fn push_true(&mut self) -> Result<()> {
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_true()?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x0A push i32
    /// push an i32 value onto the stack
    pub fn push_i32(&mut self, scenario: &Scenario) -> Result<()> {
        self.cursor += 1;

        let value = scenario.read_i32(self.cursor)?;
        self.cursor += size_of::<i32>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_int(value)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x0B push i16
    /// push an i16 value onto the stack
    pub fn push_i16(&mut self, scenario: &Scenario) -> Result<()> {
        self.cursor += 1;
        let value = scenario.read_i16(self.cursor)?;
        self.cursor += size_of::<i16>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_int(value as i32)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x0C push i8
    /// push an i8 value onto the stack
    pub fn push_i8(&mut self, scenario: &Scenario) -> Result<()> {
        self.cursor += 1;
        let value = scenario.read_i8(self.cursor)?;
        self.cursor += size_of::<i8>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_int(value as i32)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x0D push f32
    /// push an f32 value onto the stack
    pub fn push_f32(&mut self, scenario: &Scenario) -> Result<()> {
        self.cursor += 1;
        let value = scenario.read_f32(self.cursor)?;
        self.cursor += size_of::<f32>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_float(value)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x0E push string
    /// push a string onto the stack
    pub fn push_string(&mut self, scenario: &Scenario) -> Result<()> {
        self.cursor += 1;
        let len = scenario.read_u8(self.cursor)? as usize;
        self.cursor += size_of::<u8>();

        let s = scenario.read_cstring(self.cursor, len)?;
        self.cursor += len;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_string(s)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x0F push global
    /// push a global variable onto the stack
    pub fn push_global(&mut self, scenario: &Scenario) -> Result<()> {
        self.cursor += 1;
        let key = scenario.read_u16(self.cursor)?;
        self.cursor += size_of::<u16>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_global(key as u32)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x10 push stack
    /// push a stack variable onto the stack
    pub fn push_stack(&mut self, scenario: &Scenario) -> Result<()> {
        self.cursor += 1;
        let offset = scenario.read_i8(self.cursor)?;
        self.cursor += size_of::<i8>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_stack(offset)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x11 push global table
    /// push a value than stored in the global table by immediate key onto the stack
    /// we assume that if any failure occurs, such as the key not found,
    /// we will push a nil value onto the stack for compatibility reasons.
    pub fn push_global_table(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;
        let key = scenario.read_u16(self.cursor)?;
        self.cursor += size_of::<u16>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let table = NamedVariant::from_global(key as u32);
            let table_key = stack_analyzer.pop()?;
            let statement = Statement::from_global_table_access(addr, table, table_key);
            
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x12 push local table
    /// push a value than stored in the local table by key onto the stack
    pub fn push_local_table(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;
        let idx = scenario.read_i8(self.cursor)?;
        self.cursor += size_of::<i8>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let table = stack_analyzer.get(idx)?;
            let table_key = stack_analyzer.pop()?;
            let statement = Statement::from_local_table_access(addr, table, table_key);
            
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x13 push top
    /// push the top of the stack onto the stack
    pub fn push_top(&mut self) -> Result<()> {
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_top()?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x14 push return value
    /// push the return value onto the stack
    pub fn push_return_value(&mut self) -> Result<()> {
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            stack_analyzer.push_return_value()?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x15 pop global
    /// pop the top of the stack and store it in the global table
    pub fn pop_global(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;
        let key = scenario.read_u16(self.cursor)?;
        self.cursor += size_of::<u16>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = NamedVariant::from_global(key as u32);
            let statement = Statement::from_assign(addr, left, right);

            self.push_statement_to_current_function(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x16 local copy
    /// copy the top of the stack to the local variable
    pub fn local_copy(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;
        let idx = scenario.read_i8(self.cursor)?;
        self.cursor += size_of::<i8>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.get(idx)?;
            let statement = Statement::from_assign(addr, left, right);

            self.push_statement_to_current_function(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x17 pop global table
    /// pop the top of the stack and store it in the global table by key
    pub fn pop_global_table(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;
        let key = scenario.read_u16(self.cursor)?;
        self.cursor += size_of::<u16>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let table_key = stack_analyzer.pop()?;
            let table = NamedVariant::from_global(key as u32);
            let left = Statement::from_local_table_access(addr, table, table_key);
            let left = NamedVariant::from_expr(left);
            let statement = Statement::from_assign(addr, left, right);

            self.push_statement_to_current_function(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x18 pop local table
    /// pop the top of the stack and store it in the local table by key
    pub fn pop_local_table(&mut self, scenario: &Scenario) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;
        let idx = scenario.read_i8(self.cursor)?;
        self.cursor += size_of::<i8>();

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let table_key = stack_analyzer.pop()?;
            let table = stack_analyzer.get(idx)?;
            let left = Statement::from_local_table_access(addr, table, table_key);
            let left = NamedVariant::from_expr(left);
            let statement = Statement::from_assign(addr, left, right);

            self.push_statement_to_current_function(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x19 neg
    /// negate the top of the stack, only works for integers and floats
    pub fn neg(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let var = stack_analyzer.pop()?;
            let statement = Statement::from_unary_op(addr, "pvm_neg".into(), var);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x1A add
    /// add the top two values on the stack
    pub fn add(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_add".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x1B sub
    /// subtract the top two values on the stack
    pub fn sub(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_sub".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x1C mul
    /// multiply the top two values on the stack
    pub fn mul(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_mul".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x1D div
    /// divide the top two values on the stack
    pub fn div(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_div".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x1E modulo
    /// modulo the top two values on the stack
    pub fn modulo(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_mod".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x1F bittest
    /// test with the top two values on the stack
    pub fn bittest(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_bittest".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x20 and
    /// push true if both the top two values on the stack are none-nil
    pub fn and(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_and".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x21 or
    /// push true if either of the top two values on the stack is none-nil
    pub fn or(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_or".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x22 sete
    /// set the top of the stack to true if the top two values on the stack are equal
    pub fn sete(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_sete".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x23 setne
    /// set the top of the stack to true if the top two values on the stack are not equal
    pub fn setne(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_setne".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x24 setg
    /// set the top of the stack to true if the top two values on the stack are greater
    pub fn setg(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_setg".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x25 setle
    /// set the top of the stack to true if the top two values on the stack are less or equal
    pub fn setle(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_setle".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x26 setl
    /// set the top of the stack to true if the top two values on the stack are less
    pub fn setl(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_setl".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    /// 0x27 setge
    /// set the top of the stack to true if the top two values on the stack are greater or equal
    pub fn setge(&mut self) -> Result<()> {
        let addr = self.get_pc() as u32;
        self.cursor += 1;

        if let Some(stack_analyzer) = &mut self.stack_analyzer {
            let right = stack_analyzer.pop()?;
            let left = stack_analyzer.pop()?;
            let statement = Statement::from_binary_op(addr, "pvm_setge".into(), left, right);
            stack_analyzer.push_expr(statement)?;
        } else {
            bail!("stack analyzer not found");
        }

        Ok(())
    }

    fn routine_defination_pass(&mut self, scenario: &Scenario) -> Result<()> {
        let opcode = scenario.read_u8(self.get_pc())? as i32;

        match opcode.try_into() {
            Ok(Opcode::Nop) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::InitStack) => {
                self.init_stack_bypass(scenario)?;
            }
            Ok(Opcode::Call) => {
                self.five_bytes_bypass()?;
            }
            Ok(Opcode::Syscall) => {
                self.three_bytes_bypass()?;
            }
            Ok(Opcode::Ret) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::RetV) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::Jmp) => {
                self.five_bytes_bypass()?;
            }
            Ok(Opcode::Jz) => {
                self.five_bytes_bypass()?;
            }
            Ok(Opcode::PushNil) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::PushTrue) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::PushI32) => {
                self.five_bytes_bypass()?;
            }
            Ok(Opcode::PushI16) => {
                self.three_bytes_bypass()?;
            }
            Ok(Opcode::PushI8) => {
                self.two_bytes_bypass()?;
            }
            Ok(Opcode::PushF32) => {
                self.five_bytes_bypass()?;
            }
            Ok(Opcode::PushString) => {
                self.push_string_bypass(scenario)?;
            }
            Ok(Opcode::PushGlobal) => {
                self.three_bytes_bypass()?;
            }
            Ok(Opcode::PushStack) => {
                self.two_bytes_bypass()?;
            }
            Ok(Opcode::PushGlobalTable) => {
                self.three_bytes_bypass()?;
            }
            Ok(Opcode::PushLocalTable) => {
                self.two_bytes_bypass()?;
            }
            Ok(Opcode::PushTop) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::PushReturn) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::PopGlobal) => {
                self.three_bytes_bypass()?;
            }
            Ok(Opcode::PopStack) => {
                self.two_bytes_bypass()?;
            }
            Ok(Opcode::PopGlobalTable) => {
                self.three_bytes_bypass()?;
            }
            Ok(Opcode::PopLocalTable) => {
                self.two_bytes_bypass()?;
            }
            Ok(Opcode::Neg) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::Add) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::Sub) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::Mul) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::Div) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::Mod) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::BitTest) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::And) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::Or) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::SetE) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::SetNE) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::SetG) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::SetLE) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::SetL) => {
                self.one_byte_bypass()?;
            }
            Ok(Opcode::SetGE) => {
                self.one_byte_bypass()?;
            }
            _ => {
                bail!("unexpected opcode: {}", opcode);
            }
        }

        Ok(())
    }

    fn disassemble_pass(&mut self, scenario: &Scenario) -> Result<()> {
        let opcode = scenario.read_u8(self.get_pc())? as i32;

        match opcode.try_into() {
            Ok(Opcode::Nop) => {
                self.nop()?;
            }
            Ok(Opcode::InitStack) => {
                self.init_stack()?;
            }
            Ok(Opcode::Call) => {
                self.call(scenario)?;
            }
            Ok(Opcode::Syscall) => {
                self.syscall(scenario)?;
            }
            Ok(Opcode::Ret) => {
                self.ret()?;
            }
            Ok(Opcode::RetV) => {
                self.retv()?;
            }
            Ok(Opcode::Jmp) => {
                self.jmp(scenario)?;
            }
            Ok(Opcode::Jz) => {
                self.jz(scenario)?;
            }
            Ok(Opcode::PushNil) => {
                self.push_nil()?;
            }
            Ok(Opcode::PushTrue) => {
                self.push_true()?;
            }
            Ok(Opcode::PushI32) => {
                self.push_i32(scenario)?;
            }
            Ok(Opcode::PushI16) => {
                self.push_i16(scenario)?;
            }
            Ok(Opcode::PushI8) => {
                self.push_i8(scenario)?;
            }
            Ok(Opcode::PushF32) => {
                self.push_f32(scenario)?;
            }
            Ok(Opcode::PushString) => {
                self.push_string(scenario)?;
            }
            Ok(Opcode::PushGlobal) => {
                self.push_global(scenario)?;
            }
            Ok(Opcode::PushStack) => {
                self.push_stack(scenario)?;
            }
            Ok(Opcode::PushGlobalTable) => {
                self.push_global_table(scenario)?;
            }
            Ok(Opcode::PushLocalTable) => {
                self.push_local_table(scenario)?;
            }
            Ok(Opcode::PushTop) => {
                self.push_top()?;
            }
            Ok(Opcode::PushReturn) => {
                self.push_return_value()?;
            }
            Ok(Opcode::PopGlobal) => {
                self.pop_global(scenario)?;
            }
            Ok(Opcode::PopStack) => {
                self.local_copy(scenario)?;
            }
            Ok(Opcode::PopGlobalTable) => {
                self.pop_global_table(scenario)?;
            }
            Ok(Opcode::PopLocalTable) => {
                self.pop_local_table(scenario)?;
            }
            Ok(Opcode::Neg) => {
                self.neg()?;
            }
            Ok(Opcode::Add) => {
                self.add()?;
            }
            Ok(Opcode::Sub) => {
                self.sub()?;
            }
            Ok(Opcode::Mul) => {
                self.mul()?;
            }
            Ok(Opcode::Div) => {
                self.div()?;
            }
            Ok(Opcode::Mod) => {
                self.modulo()?;
            }
            Ok(Opcode::BitTest) => {
                self.bittest()?;
            }
            Ok(Opcode::And) => {
                self.and()?;
            }
            Ok(Opcode::Or) => {
                self.or()?;
            }
            Ok(Opcode::SetE) => {
                self.sete()?;
            }
            Ok(Opcode::SetNE) => {
                self.setne()?;
            }
            Ok(Opcode::SetG) => {
                self.setg()?;
            }
            Ok(Opcode::SetLE) => {
                self.setle()?;
            }
            Ok(Opcode::SetL) => {
                self.setl()?;
            }
            Ok(Opcode::SetGE) => {
                self.setge()?;
            }
            _ => {
                self.nop()?;
                log::error!("unknown opcode: {}", opcode);
            }
        };

        Ok(())
    }

    pub fn disassemble(&mut self) -> Result<()> {
        let scenario = self.scenario.clone();

        // pass 1: routine defination
        while self.get_pc() < scenario.get_sys_desc_offset() as usize {
            self.routine_defination_pass(&scenario)?;
        }

        // pass 2: disassemble
        self.rewind();
        while self.get_pc() < scenario.get_sys_desc_offset() as usize {
            self.disassemble_pass(&scenario)?;
        }

        println!("{:?}", self.functions.values().last().unwrap());

        Ok(())
    }
}
