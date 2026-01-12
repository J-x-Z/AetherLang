//! IR Optimizer for AetherLang
//!
//! Implements various optimization passes on Aether IR.

use crate::middle::ir::*;

/// Optimization pass trait
pub trait OptimizationPass {
    /// Name of the optimization pass
    fn name(&self) -> &'static str;
    
    /// Run the pass on a module
    fn run_on_module(&mut self, module: &mut IRModule) -> bool;
    
    /// Run the pass on a function
    fn run_on_function(&mut self, func: &mut IRFunction) -> bool;
}

/// The optimizer - runs optimization passes
pub struct Optimizer {
    passes: Vec<Box<dyn OptimizationPass>>,
}

impl Optimizer {
    pub fn new() -> Self {
        let mut opt = Self { passes: Vec::new() };
        // Register default passes
        opt.add_pass(Box::new(ConstantFolding::new()));
        opt.add_pass(Box::new(DeadCodeElimination::new()));
        opt.add_pass(Box::new(SimplifyBranches::new()));
        opt
    }

    /// Add an optimization pass
    pub fn add_pass(&mut self, pass: Box<dyn OptimizationPass>) {
        self.passes.push(pass);
    }

    /// Run all passes on the module
    pub fn optimize(&mut self, module: &mut IRModule) {
        let mut changed = true;
        let max_iterations = 10;
        let mut iteration = 0;

        // Keep running until no changes or max iterations
        while changed && iteration < max_iterations {
            changed = false;
            for pass in &mut self.passes {
                if pass.run_on_module(module) {
                    changed = true;
                }
            }
            iteration += 1;
        }
    }
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Constant Folding ====================

/// Folds constant expressions at compile time
pub struct ConstantFolding;

impl ConstantFolding {
    pub fn new() -> Self {
        Self
    }

    fn fold_binop(op: BinOp, left: &Constant, right: &Constant) -> Option<Constant> {
        match (left, right) {
            (Constant::Int(l), Constant::Int(r)) => {
                let result = match op {
                    BinOp::Add => l.checked_add(*r)?,
                    BinOp::Sub => l.checked_sub(*r)?,
                    BinOp::Mul => l.checked_mul(*r)?,
                    BinOp::Div => l.checked_div(*r)?,
                    BinOp::Mod => l.checked_rem(*r)?,
                    BinOp::Shl => l.checked_shl(*r as u32)?,
                    BinOp::Shr => l.checked_shr(*r as u32)?,
                    BinOp::And => l & r,
                    BinOp::Or => l | r,
                    BinOp::Xor => l ^ r,
                    BinOp::Eq => return Some(Constant::Bool(l == r)),
                    BinOp::Ne => return Some(Constant::Bool(l != r)),
                    BinOp::Lt => return Some(Constant::Bool(l < r)),
                    BinOp::Le => return Some(Constant::Bool(l <= r)),
                    BinOp::Gt => return Some(Constant::Bool(l > r)),
                    BinOp::Ge => return Some(Constant::Bool(l >= r)),
                };
                Some(Constant::Int(result))
            }
            (Constant::Float(l), Constant::Float(r)) => {
                let result = match op {
                    BinOp::Add => l + r,
                    BinOp::Sub => l - r,
                    BinOp::Mul => l * r,
                    BinOp::Div => l / r,
                    BinOp::Mod => l % r,
                    BinOp::Eq => return Some(Constant::Bool((l - r).abs() < f64::EPSILON)),
                    BinOp::Ne => return Some(Constant::Bool((l - r).abs() >= f64::EPSILON)),
                    BinOp::Lt => return Some(Constant::Bool(l < r)),
                    BinOp::Le => return Some(Constant::Bool(l <= r)),
                    BinOp::Gt => return Some(Constant::Bool(l > r)),
                    BinOp::Ge => return Some(Constant::Bool(l >= r)),
                    _ => return None,
                };
                Some(Constant::Float(result))
            }
            (Constant::Bool(l), Constant::Bool(r)) => {
                let result = match op {
                    BinOp::And => *l && *r,
                    BinOp::Or => *l || *r,
                    BinOp::Eq => l == r,
                    BinOp::Ne => l != r,
                    _ => return None,
                };
                Some(Constant::Bool(result))
            }
            _ => None,
        }
    }
}

impl OptimizationPass for ConstantFolding {
    fn name(&self) -> &'static str {
        "constant-folding"
    }

    fn run_on_module(&mut self, module: &mut IRModule) -> bool {
        let mut changed = false;
        for func in &mut module.functions {
            if self.run_on_function(func) {
                changed = true;
            }
        }
        changed
    }

    fn run_on_function(&mut self, func: &mut IRFunction) -> bool {
        let mut changed = false;

        for block in &mut func.blocks {
            for inst in &mut block.instructions {
                if let Instruction::BinOp { dest, op, left, right } = inst {
                    if let (Value::Constant(l), Value::Constant(r)) = (left, right) {
                        if let Some(result) = Self::fold_binop(*op, l, r) {
                            *inst = Instruction::Assign {
                                dest: *dest,
                                value: Value::Constant(result),
                            };
                            changed = true;
                        }
                    }
                }
            }
        }

        changed
    }
}

impl Default for ConstantFolding {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Dead Code Elimination ====================

/// Removes unreachable code and unused definitions
pub struct DeadCodeElimination;

impl DeadCodeElimination {
    pub fn new() -> Self {
        Self
    }

    fn remove_unreachable_blocks(&self, func: &mut IRFunction) -> bool {
        if func.blocks.is_empty() {
            return false;
        }

        use std::collections::HashSet;
        let mut reachable = HashSet::new();
        let mut worklist = vec![func.entry_block];

        // Find all reachable blocks using DFS
        while let Some(block_id) = worklist.pop() {
            if reachable.contains(&block_id) {
                continue;
            }
            reachable.insert(block_id);

            if let Some(block) = func.blocks.get(block_id.0) {
                if let Some(ref term) = block.terminator {
                    match term {
                        Terminator::Jump { target } => {
                            worklist.push(*target);
                        }
                        Terminator::Branch { then_target, else_target, .. } => {
                            worklist.push(*then_target);
                            worklist.push(*else_target);
                        }
                        Terminator::Return { .. } | Terminator::Unreachable => {}
                    }
                }
            }
        }

        // Mark unreachable blocks (don't remove to preserve indices)
        let original_len = func.blocks.len();
        for (i, block) in func.blocks.iter_mut().enumerate() {
            if !reachable.contains(&BlockId(i)) {
                block.instructions.clear();
                block.terminator = Some(Terminator::Unreachable);
            }
        }

        func.blocks.len() < original_len
    }
}

impl OptimizationPass for DeadCodeElimination {
    fn name(&self) -> &'static str {
        "dead-code-elimination"
    }

    fn run_on_module(&mut self, module: &mut IRModule) -> bool {
        let mut changed = false;
        for func in &mut module.functions {
            if self.run_on_function(func) {
                changed = true;
            }
        }
        changed
    }

    fn run_on_function(&mut self, func: &mut IRFunction) -> bool {
        self.remove_unreachable_blocks(func)
    }
}

impl Default for DeadCodeElimination {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Simplify Branches ====================

/// Simplifies branch instructions with constant conditions
pub struct SimplifyBranches;

impl SimplifyBranches {
    pub fn new() -> Self {
        Self
    }
}

impl OptimizationPass for SimplifyBranches {
    fn name(&self) -> &'static str {
        "simplify-branches"
    }

    fn run_on_module(&mut self, module: &mut IRModule) -> bool {
        let mut changed = false;
        for func in &mut module.functions {
            if self.run_on_function(func) {
                changed = true;
            }
        }
        changed
    }

    fn run_on_function(&mut self, func: &mut IRFunction) -> bool {
        let mut changed = false;

        for block in &mut func.blocks {
            if let Some(ref mut term) = block.terminator {
                if let Terminator::Branch { cond, then_target, else_target } = term {
                    // If condition is a constant, simplify to unconditional jump
                    if let Value::Constant(Constant::Bool(b)) = cond {
                        let target = if *b { *then_target } else { *else_target };
                        *term = Terminator::Jump { target };
                        changed = true;
                    }
                }
            }
        }

        changed
    }
}

impl Default for SimplifyBranches {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Algebraic Simplification ====================

/// Simplifies algebraic identities (x + 0 = x, x * 1 = x, etc.)
pub struct AlgebraicSimplification;

impl AlgebraicSimplification {
    pub fn new() -> Self {
        Self
    }

    fn simplify(inst: &mut Instruction) -> bool {
        if let Instruction::BinOp { dest, op, left, right } = inst {
            // x + 0 = x, x - 0 = x
            if let Value::Constant(Constant::Int(0)) = right {
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Or | BinOp::Xor) {
                    *inst = Instruction::Assign { dest: *dest, value: left.clone() };
                    return true;
                }
            }
            // 0 + x = x
            if let Value::Constant(Constant::Int(0)) = left {
                if matches!(op, BinOp::Add | BinOp::Or | BinOp::Xor) {
                    *inst = Instruction::Assign { dest: *dest, value: right.clone() };
                    return true;
                }
            }
            // x * 1 = x, x / 1 = x
            if let Value::Constant(Constant::Int(1)) = right {
                if matches!(op, BinOp::Mul | BinOp::Div) {
                    *inst = Instruction::Assign { dest: *dest, value: left.clone() };
                    return true;
                }
            }
            // 1 * x = x
            if let Value::Constant(Constant::Int(1)) = left {
                if matches!(op, BinOp::Mul) {
                    *inst = Instruction::Assign { dest: *dest, value: right.clone() };
                    return true;
                }
            }
            // x * 0 = 0, 0 * x = 0
            if let Value::Constant(Constant::Int(0)) = left {
                if matches!(op, BinOp::Mul) {
                    *inst = Instruction::Assign { dest: *dest, value: Value::Constant(Constant::Int(0)) };
                    return true;
                }
            }
            if let Value::Constant(Constant::Int(0)) = right {
                if matches!(op, BinOp::Mul) {
                    *inst = Instruction::Assign { dest: *dest, value: Value::Constant(Constant::Int(0)) };
                    return true;
                }
            }
            // x & 0 = 0
            if let Value::Constant(Constant::Int(0)) = right {
                if matches!(op, BinOp::And) {
                    *inst = Instruction::Assign { dest: *dest, value: Value::Constant(Constant::Int(0)) };
                    return true;
                }
            }
        }
        false
    }
}

impl OptimizationPass for AlgebraicSimplification {
    fn name(&self) -> &'static str {
        "algebraic-simplification"
    }

    fn run_on_module(&mut self, module: &mut IRModule) -> bool {
        let mut changed = false;
        for func in &mut module.functions {
            if self.run_on_function(func) {
                changed = true;
            }
        }
        changed
    }

    fn run_on_function(&mut self, func: &mut IRFunction) -> bool {
        let mut changed = false;

        for block in &mut func.blocks {
            for inst in &mut block.instructions {
                if Self::simplify(inst) {
                    changed = true;
                }
            }
        }

        changed
    }
}

impl Default for AlgebraicSimplification {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_module() -> IRModule {
        let mut module = IRModule::new("test");
        let mut func = IRFunction::new("test", vec![], IRType::Void);
        func.add_block("entry");
        module.functions.push(func);
        module
    }

    #[test]
    fn test_constant_folding_add() {
        let mut module = make_module();
        
        // Add instruction
        module.functions[0].blocks[0].push(Instruction::BinOp {
            dest: Register(0),
            op: BinOp::Add,
            left: Value::Constant(Constant::Int(10)),
            right: Value::Constant(Constant::Int(20)),
        });

        let mut pass = ConstantFolding::new();
        let changed = pass.run_on_module(&mut module);
        assert!(changed);

        // Should be folded to Assign { dest: %0, value: 30 }
        if let Instruction::Assign { value, .. } = &module.functions[0].blocks[0].instructions[0] {
            if let Value::Constant(Constant::Int(n)) = value {
                assert_eq!(*n, 30);
            } else {
                panic!("Expected Int constant");
            }
        } else {
            panic!("Expected Assign instruction");
        }
    }

    #[test]
    fn test_constant_folding_comparison() {
        let mut module = make_module();
        
        module.functions[0].blocks[0].push(Instruction::BinOp {
            dest: Register(0),
            op: BinOp::Lt,
            left: Value::Constant(Constant::Int(5)),
            right: Value::Constant(Constant::Int(10)),
        });

        let mut pass = ConstantFolding::new();
        let changed = pass.run_on_module(&mut module);
        assert!(changed);

        if let Instruction::Assign { value, .. } = &module.functions[0].blocks[0].instructions[0] {
            if let Value::Constant(Constant::Bool(b)) = value {
                assert!(*b);
            } else {
                panic!("Expected Bool constant");
            }
        } else {
            panic!("Expected Assign instruction");
        }
    }

    #[test]
    fn test_simplify_branch_true() {
        let mut module = make_module();
        module.functions[0].add_block("then");
        module.functions[0].add_block("else");
        
        module.functions[0].blocks[0].set_terminator(Terminator::Branch {
            cond: Value::Constant(Constant::Bool(true)),
            then_target: BlockId(1),
            else_target: BlockId(2),
        });

        let mut pass = SimplifyBranches::new();
        let changed = pass.run_on_module(&mut module);
        assert!(changed);

        if let Some(Terminator::Jump { target }) = &module.functions[0].blocks[0].terminator {
            assert_eq!(target.0, 1); // Should jump to then block
        } else {
            panic!("Expected Jump terminator");
        }
    }

    #[test]
    fn test_algebraic_add_zero() {
        let mut module = make_module();
        
        module.functions[0].blocks[0].push(Instruction::BinOp {
            dest: Register(0),
            op: BinOp::Add,
            left: Value::Register(Register(1)),
            right: Value::Constant(Constant::Int(0)),
        });

        let mut pass = AlgebraicSimplification::new();
        let changed = pass.run_on_module(&mut module);
        assert!(changed);

        if let Instruction::Assign { value, .. } = &module.functions[0].blocks[0].instructions[0] {
            if let Value::Register(r) = value {
                assert_eq!(r.0, 1);
            } else {
                panic!("Expected Register value");
            }
        } else {
            panic!("Expected Assign instruction");
        }
    }

    #[test]
    fn test_full_optimizer() {
        let mut module = make_module();
        
        // Add foldable expression: 2 + 3
        module.functions[0].blocks[0].push(Instruction::BinOp {
            dest: Register(0),
            op: BinOp::Add,
            left: Value::Constant(Constant::Int(2)),
            right: Value::Constant(Constant::Int(3)),
        });
        module.functions[0].blocks[0].set_terminator(Terminator::Return { 
            value: Some(Value::Register(Register(0))) 
        });

        let mut optimizer = Optimizer::new();
        optimizer.optimize(&mut module);

        // Should be folded to 5
        if let Instruction::Assign { value, .. } = &module.functions[0].blocks[0].instructions[0] {
            if let Value::Constant(Constant::Int(n)) = value {
                assert_eq!(*n, 5);
            }
        }
    }
}

