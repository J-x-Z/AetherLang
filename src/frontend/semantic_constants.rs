//! Canonical scalar constants shared by type checking and both code generators.
use super::*;

#[derive(Debug, Clone, Default)]
pub struct SemanticFacts {
    pub aliases: HashMap<String, ResolvedType>,
    pub constants: HashMap<String, TypedConstant>,
}

#[derive(Debug, Clone)]
pub struct TypedConstant {
    pub ty: ResolvedType,
    pub value: ScalarConstant,
}

#[derive(Debug, Clone)]
pub enum ScalarConstant {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    String(String),
}

impl SemanticAnalyzer {
    pub fn facts(&self) -> &SemanticFacts { &self.facts }

    pub(super) fn prepare_constant_facts(&mut self) -> Result<()> {
        for (name, ty) in &self.type_aliases {
            self.facts.aliases.insert(name.clone(), self.resolve_type(ty)?);
        }
        for name in self.constant_defs.keys().cloned().collect::<Vec<_>>() {
            let constant = self.evaluate_constant(&name, &mut HashSet::new())?;
            if let Some(symbol) = self.symbols.scopes[0].symbols.get_mut(&name) { symbol.ty = constant.ty; }
        }
        Ok(())
    }

    fn constant_error(message: &str, span: Span) -> Error {
        Error::Expected(format!("valid constant expression: {message}"), span)
    }

    fn evaluate_constant(&mut self, name: &str, visiting: &mut HashSet<String>) -> Result<TypedConstant> {
        if let Some(value) = self.facts.constants.get(name) { return Ok(value.clone()); }
        let definition = self.constant_defs.get(name).cloned().ok_or_else(|| Error::UndefinedVariable { name: name.into(), span: Span::dummy() })?;
        if !visiting.insert(name.into()) { return Err(Self::constant_error("cyclic constant dependency", definition.span)); }
        let expected = definition.ty.as_ref().map(|ty| self.resolve_type(ty)).transpose()?;
        let result = self.evaluate_scalar(&definition.value, expected.as_ref(), visiting)?;
        visiting.remove(name);
        self.facts.constants.insert(name.into(), result.clone());
        Ok(result)
    }

    fn evaluate_scalar(&mut self, expr: &Expr, expected: Option<&ResolvedType>, visiting: &mut HashSet<String>) -> Result<TypedConstant> {
        let span = expr.span();
        let value = match expr {
            Expr::Literal(literal) => TypedConstant { ty: self.literal_type(literal), value: match literal {
                Literal::Int(value, _) => ScalarConstant::Int(*value),
                Literal::Float(value, _) => ScalarConstant::Float(*value),
                Literal::Bool(value, _) => ScalarConstant::Bool(*value),
                Literal::Char(value, _) => ScalarConstant::Char(*value),
                Literal::String(value, _) => ScalarConstant::String(value.clone()),
            } },
            Expr::Ident(name) => {
                if !self.constant_defs.contains_key(&name.name) { return Err(Self::constant_error("initializer references a non-constant value", name.span)); }
                self.evaluate_constant(&name.name, visiting)?
            }
            Expr::Cast { expr, ty, .. } => {
                let source = self.evaluate_scalar(expr, None, visiting)?;
                let target = self.resolve_type(ty)?;
                self.cast_constant(source, &target, span)?
            }
            Expr::Unary { op, expr: operand, .. } => {
                let context = expected.filter(|ty| Self::is_numeric(ty));
                // A negative signed literal must be range-checked after negation,
                // so the magnitude 128 remains valid in an i8 initializer -128.
                if *op == UnOp::Neg {
                    if let Some(value) = Self::integer_constant(expr) {
                        let ty = context.cloned().unwrap_or(ResolvedType::I64);
                        let result = Self::integer_result(value, &ty, span)?;
                        return Ok(result);
                    }
                }
                let value = self.evaluate_scalar(operand, context, visiting)?;
                match (op, &value.value) {
                    (UnOp::Not, ScalarConstant::Bool(value)) => TypedConstant { ty: ResolvedType::BOOL, value: ScalarConstant::Bool(!value) },
                    (UnOp::Neg, ScalarConstant::Int(_)) => Self::integer_result(Self::integer_value(&value).unwrap().checked_neg().ok_or_else(|| Self::constant_error("integer overflow", span))?, &value.ty, span)?,
                    (UnOp::Neg, ScalarConstant::Float(number)) => TypedConstant { ty: value.ty, value: ScalarConstant::Float(-number) },
                    (UnOp::BitNot, ScalarConstant::Int(_)) => {
                        let number = !Self::integer_value(&value).unwrap();
                        let number = if let ResolvedType::Primitive(p) = &value.ty {
                            if p.is_integer() && !p.is_signed() { number & ((1i128 << (p.size_of() * 8)) - 1) } else { number }
                        } else { number };
                        Self::integer_result(number, &value.ty, span)?
                    }
                    _ => return Err(Self::constant_error("invalid unary operands", span)),
                }
            }
            Expr::Binary { left, op, right, .. } => {
                if matches!(op, BinOp::And | BinOp::Or) {
                    let left = self.evaluate_scalar(left, Some(&ResolvedType::BOOL), visiting)?;
                    let ScalarConstant::Bool(left) = left.value else { return Err(Self::constant_error("logical operands must be bool", span)); };
                    if (*op == BinOp::And && !left) || (*op == BinOp::Or && left) {
                        return Ok(TypedConstant { ty: ResolvedType::BOOL, value: ScalarConstant::Bool(left) });
                    }
                    return self.evaluate_scalar(right, Some(&ResolvedType::BOOL), visiting);
                }
                let context = if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr) {
                    expected.filter(|ty| Self::is_numeric(ty))
                } else { None };
                let mut lhs = self.evaluate_scalar(left, context, visiting)?;
                let mut rhs = self.evaluate_scalar(right, context, visiting)?;
                let (left_type, right_type) = self.common_operand_types(left, lhs.ty.clone(), right, rhs.ty.clone());
                self.check_binary_op(&left_type, *op, &right_type, span)?;
                lhs = self.coerce_constant(lhs, &left_type, left)?;
                rhs = self.coerce_constant(rhs, &right_type, right)?;
                self.scalar_binary(lhs, *op, rhs, span)?
            }
            _ => return Err(Self::constant_error("only scalar literals, constant references, casts, and pure operators are supported", span)),
        };
        if let Some(expected) = expected { self.coerce_constant(value, expected, expr) }
        else { self.normalize_constant(value, span) }
    }

    fn integer_value(value: &TypedConstant) -> Option<i128> {
        let ScalarConstant::Int(number) = value.value else { return None; };
        Some(if matches!(&value.ty, ResolvedType::Primitive(p) if p.is_integer() && !p.is_signed()) { (number as u64) as i128 } else { number as i128 })
    }

    fn integer_result(value: i128, ty: &ResolvedType, span: Span) -> Result<TypedConstant> {
        let ResolvedType::Primitive(primitive) = ty else { return Err(Self::constant_error("integer result needs an integer type", span)); };
        if !primitive.is_integer() { return Err(Self::constant_error("integer result needs an integer type", span)); }
        let bits = primitive.size_of() * 8;
        let (minimum, maximum) = if primitive.is_signed() { (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1) }
            else { (0, (1i128 << bits) - 1) };
        if value < minimum || value > maximum { return Err(Self::constant_error("integer overflow", span)); }
        Ok(TypedConstant { ty: ty.clone(), value: ScalarConstant::Int(value as i64) })
    }

    fn normalize_constant(&self, mut value: TypedConstant, span: Span) -> Result<TypedConstant> {
        if let ScalarConstant::Float(number) = &mut value.value {
            if value.ty == ResolvedType::F32 { *number = (*number as f32) as f64; }
            if !number.is_finite() { return Err(Self::constant_error("floating-point overflow or non-finite result", span)); }
        }
        Ok(value)
    }

    fn coerce_constant(&self, value: TypedConstant, expected: &ResolvedType, expr: &Expr) -> Result<TypedConstant> {
        if value.ty == *expected { return self.normalize_constant(value, expr.span()); }
        if !Self::literal_fits(expr, expected) { self.require_type(expected, &value.ty, expr.span())?; }
        let mut value = value;
        value.ty = expected.clone();
        if let ScalarConstant::Int(number) = value.value {
            if Self::is_integer(expected) { return Self::integer_result(number as i128, expected, expr.span()); }
        }
        self.normalize_constant(value, expr.span())
    }

    fn cast_constant(&self, value: TypedConstant, target: &ResolvedType, span: Span) -> Result<TypedConstant> {
        if value.ty == *target { return Ok(value); }
        match (&value.value, target) {
            (ScalarConstant::Int(number), ResolvedType::Pointer(_)) => Ok(TypedConstant { ty: target.clone(), value: ScalarConstant::Int(*number) }),
            (ScalarConstant::Int(_), ResolvedType::Primitive(p)) if p.is_integer() => {
                let number = Self::integer_value(&value).unwrap();
                let bits = p.size_of() * 8;
                let mask = (1i128 << bits) - 1;
                let mut truncated = number & mask;
                if p.is_signed() && truncated & (1i128 << (bits - 1)) != 0 { truncated -= 1i128 << bits; }
                Self::integer_result(truncated, target, span)
            }
            (ScalarConstant::Int(_), ResolvedType::Primitive(p)) if p.is_float() => self.normalize_constant(TypedConstant { ty: target.clone(), value: ScalarConstant::Float(Self::integer_value(&value).unwrap() as f64) }, span),
            (ScalarConstant::Float(number), ResolvedType::Primitive(p)) if p.is_float() => self.normalize_constant(TypedConstant { ty: target.clone(), value: ScalarConstant::Float(*number) }, span),
            (ScalarConstant::Float(number), ResolvedType::Primitive(p)) if p.is_integer() => Self::integer_result(number.trunc() as i128, target, span),
            _ => Err(Self::constant_error("unsupported constant cast", span)),
        }
    }

    fn scalar_binary(&self, left: TypedConstant, op: BinOp, right: TypedConstant, span: Span) -> Result<TypedConstant> {
        let comparison = |value| TypedConstant { ty: ResolvedType::BOOL, value: ScalarConstant::Bool(value) };
        if let (Some(a), Some(b)) = (Self::integer_value(&left), Self::integer_value(&right)) {
            let compared = match op { BinOp::Eq => Some(a == b), BinOp::Ne => Some(a != b), BinOp::Lt => Some(a < b), BinOp::Le => Some(a <= b), BinOp::Gt => Some(a > b), BinOp::Ge => Some(a >= b), _ => None };
            if let Some(value) = compared { return Ok(comparison(value)); }
            if matches!(op, BinOp::Div | BinOp::Mod) && b == -1 {
                if let ResolvedType::Primitive(primitive) = &left.ty {
                    if primitive.is_signed() && a == -(1i128 << (primitive.size_of() * 8 - 1)) {
                        return Err(Self::constant_error("signed division overflow", span));
                    }
                }
            }
            let value = match op {
                BinOp::Add => a.checked_add(b), BinOp::Sub => a.checked_sub(b), BinOp::Mul => a.checked_mul(b),
                BinOp::Div => a.checked_div(b), BinOp::Mod => a.checked_rem(b),
                BinOp::BitAnd => Some(a & b), BinOp::BitOr => Some(a | b), BinOp::BitXor => Some(a ^ b),
                BinOp::Shl | BinOp::Shr => {
                    let bits = if let ResolvedType::Primitive(p) = &left.ty { p.size_of() * 8 } else { 0 };
                    if b < 0 || b >= bits as i128 { return Err(Self::constant_error("shift count outside operand width", span)); }
                    if op == BinOp::Shl { a.checked_shl(b as u32) } else { a.checked_shr(b as u32) }
                }
                _ => None,
            }.ok_or_else(|| Self::constant_error("division by zero or integer overflow", span))?;
            return Self::integer_result(value, &left.ty, span);
        }
        if let (ScalarConstant::Float(a), ScalarConstant::Float(b)) = (&left.value, &right.value) {
            let compared = match op { BinOp::Eq => Some(a == b), BinOp::Ne => Some(a != b), BinOp::Lt => Some(a < b), BinOp::Le => Some(a <= b), BinOp::Gt => Some(a > b), BinOp::Ge => Some(a >= b), _ => None };
            if let Some(value) = compared { return Ok(comparison(value)); }
            let value = match op { BinOp::Add => a + b, BinOp::Sub => a - b, BinOp::Mul => a * b, BinOp::Div => a / b,
                _ => return Err(Self::constant_error("invalid floating-point operator", span)) };
            return self.normalize_constant(TypedConstant { ty: left.ty, value: ScalarConstant::Float(value) }, span);
        }
        if let (ScalarConstant::Bool(a), ScalarConstant::Bool(b)) = (&left.value, &right.value) {
            return match op { BinOp::Eq => Ok(comparison(a == b)), BinOp::Ne => Ok(comparison(a != b)), _ => Err(Self::constant_error("invalid bool operator", span)) };
        }
        Err(Self::constant_error("unsupported scalar operands", span))
    }
}
