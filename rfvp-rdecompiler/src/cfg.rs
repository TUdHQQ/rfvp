use anyhow::Result;
use crate::ir::Statement;


#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub start: usize,
    pub end: usize,  
    pub successors: Vec<usize>, // addresses of the next basic blocks
    pub statements: Vec<Statement>,
}

impl BasicBlock {
    pub fn new(start: usize, end: usize) -> Self {
        BasicBlock {
            start,
            end,
            successors: Vec::new(),
            statements: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CFGBuilder {

}

