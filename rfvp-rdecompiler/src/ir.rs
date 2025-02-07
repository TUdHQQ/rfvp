use anyhow::Result;
use rfvp_core::format::scenario::variant::{self, Variant};

#[derive(Debug, Clone)]
pub enum NamedVariant {
    StackIndexed { name: String, variant: Variant },
    ReturnValue {},
    Global { slot: u32 },
    Expr { expr: Box<Statement> },
}

impl NamedVariant {
    pub fn from_arg(index: i8, variant: Variant) -> Result<Self> {
        // argument index is always negetive and which starts from -2, while -1 is reserved for stack frame pointer
        if index >= -1 {
            anyhow::bail!("invalid argument index");
        }

        // convert the index to the actual argument index
        let index: u32 = index.abs() as u32 - 2;

        let arg = Self::StackIndexed {
            name: format!("arg{}", index),
            variant,
        };

        Ok(arg)
    }

    pub fn from_local(index: i8, variant: Variant) -> Result<Self> {
        // local index is always non-negetive
        if index < 0 {
            anyhow::bail!("invalid local index");
        }

        let arg = Self::StackIndexed {
            name: format!("local{}", index),
            variant,
        };

        Ok(arg)
    }

    pub fn from_global(slot: u32) -> Self {
        Self::Global { slot }
    }

    pub fn from_return_value() -> Self {
        Self::ReturnValue {}
    }

    pub fn from_expr(expr: Statement) -> Self {
        Self::Expr { expr: Box::new(expr) }
    }

    pub fn is_same_register(&self, other: &Self) -> Option<bool> {
        match (self, other) {
            (Self::StackIndexed { name: n1, .. }, Self::StackIndexed { name: n2, .. }) => {
                Some(n1 == n2)
            }
            _ => None,
        }
    }
}

pub struct StackAnalyzer {
    local_count: u32,
    args_count: u32,
    cur_top: u32,
    local_variables: Vec<NamedVariant>,
}

impl StackAnalyzer {
    pub fn new(local: u32, args: u32) -> Self {
        let mut local_variables = Vec::new();
        for i in 0..local {
            local_variables.push(NamedVariant::StackIndexed {
                name: format!("local{}", i),
                variant: Variant::Nil, // minicry the original behavior
            });
        }

        Self {
            local_count: local,
            args_count: args,
            cur_top: local,
            local_variables,
        }
    }

    fn push(&mut self, variant: Variant) -> Result<()> {
        let var = NamedVariant::from_local(self.cur_top as i8, variant)?;
        self.local_variables.push(var);
        self.cur_top += 1;
        Ok(())
    }

    fn push_named(&mut self, named: NamedVariant) -> Result<()> {
        self.local_variables.push(named);
        self.cur_top += 1;
        Ok(())
    }

    pub fn push_nil(&mut self) -> Result<()> {
        self.push(Variant::Nil)?;
        Ok(())
    }

    pub fn push_true(&mut self) -> Result<()> {
        self.push(Variant::True)?;
        Ok(())
    }

    pub fn push_int(&mut self, value: i32) -> Result<()> {
        self.push(Variant::Int(value))?;
        Ok(())
    }

    pub fn push_float(&mut self, value: f32) -> Result<()> {
        self.push(Variant::Float(value))?;
        Ok(())
    }

    pub fn push_string(&mut self, value: String) -> Result<()> {
        self.push(Variant::String(value))?;
        Ok(())
    }

    pub fn push_global(&mut self, slot: u32) -> Result<()> {
        self.push_named(NamedVariant::from_global(slot))?;
        Ok(())
    }

    pub fn push_stack(&mut self, idx: i8) -> Result<()> {
        // fetch the value from the stack
        if idx >= 0 && idx < self.local_count as i8 {
            let tmp = self.local_variables[idx as usize].clone();
            self.push_named(tmp)?;
        } else if idx < -1 {
            // create a symbolic value for the argument
            let tmp = NamedVariant::from_arg(idx, Variant::Nil)?;
            self.push_named(tmp)?;
        } else {
            log::error!("push_stack(): invalid stack index: {}", idx);
            anyhow::bail!("push_stack(): invalid stack index {}", idx);
        }
        
        Ok(())
    }

    pub fn push_top(&mut self) -> Result<()> {
        log::warn!("We encountered a push_top instruction, which seems never used in the original vm.");

        if self.cur_top > 0 {
            let tmp = self.local_variables[(self.cur_top - 1) as usize].clone();
            self.push_named(tmp)?;
        } else {
            anyhow::bail!("push_top(): stack underflow");
        }
        Ok(())
    }

    pub fn push_return_value(&mut self) -> Result<()> {
        self.push_named(NamedVariant::from_return_value())?;
        Ok(())
    }

    pub fn push_expr(&mut self, expr: Statement) -> Result<()> {
        self.push_named(NamedVariant::from_expr(expr))?;
        Ok(())
    }

    pub fn pop(&mut self) -> Result<NamedVariant> {
        if self.cur_top > self.local_count {
            self.cur_top -= 1;
            let tmp = self.local_variables[self.cur_top as usize].clone();
            self.local_variables[self.cur_top as usize] = NamedVariant::StackIndexed {
                name: format!("local{}", self.cur_top),
                variant: Variant::Nil,
            };
            Ok(tmp)
        } else {
            anyhow::bail!("pop(): stack underflow {} {}", self.cur_top, self.local_count);
        }
    }

    pub fn get(&self, idx: i8) -> Result<NamedVariant> {
        if idx >= 0 {
            Ok(self.local_variables[idx as usize].clone())
        } else if idx < -1 {
            Ok(NamedVariant::from_arg(idx, Variant::Nil)?)
        } else {
            log::error!("get(): invalid stack index: {}", idx);
            anyhow::bail!("get(): invalid stack index {}", idx);
        }
    }

    pub fn current_stack_top(&self) -> u32 {
        self.cur_top
    }
}

#[derive(Debug, Clone)]
pub enum Statement {
    Call {
        address: u32, // for debugging purpose
        target: u32,
        args: Vec<NamedVariant>,
    },
    Syscall {
        address: u32,
        syscall_name: String,
        args: Vec<NamedVariant>,
    },
    Return {
        address: u32,
        has_value: bool,
    },
    Jmp {
        address: u32,
        target: u32,
    },
    Jz {
        address: u32,
        target: u32,
        condition: NamedVariant,
    },
    GlobalTableAccess {
        address: u32,
        tlb: NamedVariant,
        slot: NamedVariant,
    },
    LocalTableAccess {
        address: u32,
        tlb: NamedVariant,
        slot: NamedVariant,
    },
    BinaryOp {
        address: u32,
        op: String, // we wll create a set of helper functions to handle the binary operations
        lhs: NamedVariant,
        rhs: NamedVariant,
    },
    UnaryOp {
        address: u32,
        op: String, // we wll create a set of helper functions to handle the unary operations
        operand: NamedVariant,
    },
    // Usually, this expression represents a set of 'pop' instructions
    Assign {
        address: u32,
        target: NamedVariant,
        value: NamedVariant,
    },
}

impl Statement {
    pub fn from_call(address: u32, target: u32, args: Vec<NamedVariant>) -> Self {
        Self::Call {
            address,
            target,
            args,
        }
    }

    pub fn from_syscall(address: u32, syscall_name: String, args: Vec<NamedVariant>) -> Self {
        Self::Syscall {
            address,
            syscall_name,
            args,
        }
    }

    pub fn from_return(address: u32, has_value: bool) -> Self {
        Self::Return {
            address,
            has_value,
        }
    }

    pub fn from_jmp(address: u32, target: u32) -> Self {
        Self::Jmp {
            address,
            target,
        }
    }

    pub fn from_jz(address: u32, target: u32, condition: NamedVariant) -> Self {
        Self::Jz {
            address,
            target,
            condition,
        }
    }

    pub fn from_global_table_access(address: u32, tlb: NamedVariant, slot: NamedVariant) -> Self {
        Self::GlobalTableAccess {
            address,
            tlb,
            slot,
        }
    }

    pub fn from_local_table_access(address: u32, tlb: NamedVariant, slot: NamedVariant) -> Self {
        Self::LocalTableAccess {
            address,
            tlb,
            slot,
        }
    }

    pub fn from_binary_op(address: u32, op: String, lhs: NamedVariant, rhs: NamedVariant) -> Self {
        Self::BinaryOp {
            address,
            op,
            lhs,
            rhs,
        }
    }

    pub fn from_unary_op(address: u32, op: String, operand: NamedVariant) -> Self {
        Self::UnaryOp {
            address,
            op,
            operand,
        }
    }

    pub fn from_assign(address: u32, target: NamedVariant, value: NamedVariant) -> Self {
        Self::Assign {
            address,
            target,
            value,
        }
    }

    pub fn address(&self) -> u32 {
        match self {
            Self::Call { address, .. } => *address,
            Self::Syscall { address, .. } => *address,
            Self::Return { address, .. } => *address,
            Self::Jmp { address, .. } => *address,
            Self::Jz { address, .. } => *address,
            Self::GlobalTableAccess { address, .. } => *address,
            Self::LocalTableAccess { address, .. } => *address,
            Self::BinaryOp { address, .. } => *address,
            Self::UnaryOp { address, .. } => *address,
            Self::Assign { address, .. } => *address,
        }
    }
}