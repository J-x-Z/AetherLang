//! Canonical scalar constants shared by type checking and both code generators.
use super::*;

#[derive(Debug, Clone, Default)]
pub struct SemanticFacts {
    pub pointer_width: PointerWidth,
    pub places: HashMap<(String, Span), ResolvedPlace>,
    pub method_signatures: HashMap<String, ResolvedMethodSignature>,
    pub method_calls: HashMap<(String, Span), ResolvedMethodCall>,
    pub expr_types: HashMap<(String, Span), ResolvedType>,
    pub memory_orders: HashMap<Span, (crate::intrinsics::MemoryOrder, crate::intrinsics::MemoryOrder)>,
    pub enum_variants: HashMap<String, Vec<(String, Vec<ResolvedType>)>>,
    pub aliases: HashMap<String, ResolvedType>,
    pub constants: HashMap<String, TypedConstant>,
    pub global_types: HashMap<String, ResolvedType>,
    pub globals: HashMap<String, TypedInitializer>,
    pub unions: HashSet<String>,
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

#[derive(Debug, Clone)]
pub struct TypedInitializer {
    pub ty: ResolvedType,
    pub value: InitializerValue,
}

#[derive(Debug, Clone)]
pub enum InitializerValue {
    Zero,
    Scalar(ScalarConstant),
    Aggregate(Vec<(usize, TypedInitializer)>),
    Address { global: String, path: Vec<usize> },
    Layout { query: LayoutQuery, ty: ResolvedType, field: Option<usize> },
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

    pub(super) fn evaluate_scalar(&mut self, expr: &Expr, expected: Option<&ResolvedType>, visiting: &mut HashSet<String>) -> Result<TypedConstant> {
        let span = expr.span();
        let value = match expr {
            Expr::Literal(Literal::Int(value, _)) => {
                let ty = expected.filter(|ty| Self::is_integer(ty)).cloned().unwrap_or(ResolvedType::I64);
                Self::integer_result(*value, &ty, span)?
            }
            Expr::Literal(literal) => TypedConstant { ty: self.literal_type(literal), value: match literal {
                Literal::Int(_, _) => unreachable!(),
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
                let target = self.resolve_type(ty)?;
                let context = Self::wide_cast_context(expr, &target);
                let source = self.evaluate_scalar(expr, context.as_ref(), visiting)?;
                self.cast_constant(source, &target, span)?
            }
            Expr::Unary { op, expr: operand, .. } => {
                let context = expected.filter(|ty| Self::is_numeric(ty));
                // A negative signed literal must be range-checked after negation,
                // so the magnitude 128 remains valid in an i8 initializer -128.
                if *op == UnOp::Neg && matches!(operand.as_ref(), Expr::Literal(Literal::Int(_, _))) {
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
                let (mut lhs, mut rhs) = if context.is_none() && Self::contextual_number(left) && !Self::contextual_number(right) {
                    let rhs = self.evaluate_scalar(right, None, visiting)?;
                    let lhs = self.evaluate_scalar(left, Some(&rhs.ty), visiting)?;
                    (lhs, rhs)
                } else {
                    let lhs = self.evaluate_scalar(left, context, visiting)?;
                    let hint = if context.is_none() && Self::contextual_number(right) { Some(&lhs.ty) } else { context };
                    let rhs = self.evaluate_scalar(right, hint, visiting)?;
                    (lhs, rhs)
                };
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
        if let Some(number) = Self::integer_value(&value) {
            if Self::is_integer(expected) { return Self::integer_result(number, expected, expr.span()); }
        }
        let mut value = value;
        value.ty = expected.clone();
        self.normalize_constant(value, expr.span())
    }

    fn cast_constant(&self, value: TypedConstant, target: &ResolvedType, span: Span) -> Result<TypedConstant> {
        if value.ty == *target { return Ok(value); }
        match (&value.value, target) {
            (ScalarConstant::Char(character), ResolvedType::Primitive(p)) if p.is_integer() => {
                self.cast_constant(TypedConstant {
                    ty: ResolvedType::Primitive(PrimitiveType::U32),
                    value: ScalarConstant::Int(*character as i64),
                }, target, span)
            }
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
            (ScalarConstant::Float(number), ResolvedType::Primitive(p)) if p.is_integer() => {
                let bits = p.size_of() * 8;
                let (min, max) = if p.is_signed() { (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1) } else { (0, (1i128 << bits) - 1) };
                let integer = if number.is_nan() { 0 } else { (number.trunc() as i128).clamp(min, max) };
                Self::integer_result(integer, target, span)
            },
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

impl SemanticAnalyzer {
    fn initializer_fields(&self, ty: &ResolvedType, span: Span) -> Result<Vec<(String, ResolvedType)>> {
        if let ResolvedType::Tuple(elements) = ty {
            return Ok(elements.iter().enumerate().map(|(i, ty)| (i.to_string(), ty.clone())).collect());
        }
        let ResolvedType::Struct { name, .. } = ty else { return Err(Self::constant_error("aggregate initializer needs a nominal type", span)); };
        match self.symbols.lookup(name).map(|symbol| &symbol.kind) {
            Some(SymbolKind::Struct { fields, .. }) => Ok(fields.clone()),
            _ => Err(Self::constant_error("missing aggregate declaration", span)),
        }
    }

    pub(super) fn check_static_initializer(&mut self, definition: &StaticDef) -> Result<()> {
        let ty = self.resolve_type(&definition.ty)?;
        let value = if let Some(value) = &definition.value {
            self.check_expected_expr(value, &ty)?;
            self.evaluate_initializer(value, &ty)?
        } else {
            if !self.zero_initialized_type(&ty, definition.span)? {
                return Err(Self::constant_error("this type requires an explicit valid initializer", definition.span));
            }
            TypedInitializer { ty: ty.clone(), value: InitializerValue::Zero }
        };
        self.facts.globals.insert(definition.name.name.clone(), value);
        Ok(())
    }

    fn zero_initialized_type(&self, ty: &ResolvedType, span: Span) -> Result<bool> {
        Ok(match ty {
            ResolvedType::Primitive(_) | ResolvedType::Pointer(_) => true,
            ResolvedType::Array { elem, .. } => self.zero_initialized_type(elem, span)?,
            ResolvedType::Tuple(elements) => {
                let mut valid = true;
                for element in elements { valid &= self.zero_initialized_type(element, span)?; }
                valid
            }
            ResolvedType::Struct { name, .. } if self.facts.unions.contains(name) => true,
            ResolvedType::Struct { .. } => {
                let mut valid = true;
                for (_, field) in self.initializer_fields(ty, span)? { valid &= self.zero_initialized_type(&field, span)?; }
                valid
            }
            _ => false,
        })
    }

    fn initializer_address(&mut self, expr: &Expr) -> Result<(String, Vec<usize>, ResolvedType)> {
        match expr {
            Expr::Ident(name) => {
                let ty = self.facts.global_types.get(&name.name).cloned().ok_or_else(|| Self::constant_error("address initializer requires static storage", name.span))?;
                Ok((name.name.clone(), Vec::new(), ty))
            }
            Expr::Field { expr, field, span } => {
                let (name, mut path, ty) = self.initializer_address(expr)?;
                let fields = self.initializer_fields(&ty, *span)?;
                let (index, (_, ty)) = fields.into_iter().enumerate().find(|(_, (name, _))| *name == field.name)
                    .ok_or_else(|| Error::UnknownField { field: field.name.clone(), span: field.span })?;
                path.push(index);
                Ok((name, path, ty))
            }
            Expr::Index { expr, index, span } => {
                let (name, mut path, ty) = self.initializer_address(expr)?;
                let ResolvedType::Array { elem, size } = ty else { return Err(Self::constant_error("static address indexing requires an array", *span)); };
                let value = self.evaluate_scalar(index, None, &mut HashSet::new())?;
                let index = Self::integer_value(&value).ok_or_else(|| Self::constant_error("constant integer array index", *span))?;
                if index < 0 || index >= size as i128 { return Err(Self::constant_error("static address index is out of bounds", *span)); }
                path.push(index as usize);
                Ok((name, path, *elem))
            }
            _ => Err(Self::constant_error("address initializer requires a static field or element", expr.span())),
        }
    }

    fn evaluate_initializer(&mut self, expr: &Expr, expected: &ResolvedType) -> Result<TypedInitializer> {
        let value = match expr {
            Expr::Tuple { elements, .. } if elements.is_empty() => InitializerValue::Zero,
            Expr::Tuple { elements, span } => {
                let ResolvedType::Tuple(types) = expected else { return Err(Self::constant_error("tuple initializer type", *span)); };
                if elements.len() != types.len() { return Err(Self::constant_error("tuple initializer length", *span)); }
                InitializerValue::Aggregate(elements.iter().zip(types).enumerate()
                    .map(|(i, (value, ty))| self.evaluate_initializer(value, ty).map(|value| (i, value)))
                    .collect::<Result<_>>()?)
            }
            Expr::Array { elements, span } => {
                let ResolvedType::Array { elem, size } = expected else { return Err(Self::constant_error("array initializer type", *span)); };
                if elements.len() != *size { return Err(Self::constant_error("array initializer length", *span)); }
                let mut values = Vec::new();
                for (index, element) in elements.iter().enumerate() { values.push((index, self.evaluate_initializer(element, elem)?)); }
                InitializerValue::Aggregate(values)
            }
            Expr::StructLit { name, fields, span } => {
                let ResolvedType::Struct { name: target, .. } = expected else { return Err(Self::constant_error("aggregate initializer type", *span)); };
                if name.name != *target { return Err(Self::constant_error("aggregate initializer nominal type mismatch", *span)); }
                let definitions = self.initializer_fields(expected, *span)?;
                let mut values = Vec::new();
                for (field, value) in fields {
                    let (index, (_, ty)) = definitions.iter().enumerate().find(|(_, (name, _))| *name == field.name)
                        .ok_or_else(|| Error::UnknownField { field: field.name.clone(), span: field.span })?;
                    values.push((index, self.evaluate_initializer(value, ty)?));
                }
                values.sort_by_key(|(index, _)| *index);
                InitializerValue::Aggregate(values)
            }
            Expr::Cast { expr: value, ty, .. } if matches!(self.resolve_type(ty)?, ResolvedType::Pointer(_)) && matches!(value.as_ref(), Expr::Ref { .. }) => {
                let Expr::Ref { expr, .. } = value.as_ref() else { unreachable!() };
                let (global, path, _) = self.initializer_address(expr)?;
                InitializerValue::Address { global, path }
            }
            Expr::Ref { expr, .. } => {
                let (global, path, pointee) = self.initializer_address(expr)?;
                match expected {
                    ResolvedType::Pointer(inner) | ResolvedType::Reference { inner, .. } => self.require_type(inner, &pointee, expr.span())?,
                    _ => return Err(Self::constant_error("address initializer needs a pointer or reference", expr.span())),
                }
                InitializerValue::Address { global, path }
            }
            Expr::Layout { query, ty, field, span } => {
                let ty = self.resolve_type(ty)?;
                let field = if let Some(field) = field {
                    Some(self.initializer_fields(&ty, *span)?.iter().position(|(name, _)| *name == field.name)
                        .ok_or_else(|| Error::UnknownField { field: field.name.clone(), span: field.span })?)
                } else { None };
                InitializerValue::Layout { query: *query, ty, field }
            }
            _ => InitializerValue::Scalar(self.evaluate_scalar(expr, Some(expected), &mut HashSet::new())?.value),
        };
        Ok(TypedInitializer { ty: expected.clone(), value })
    }
}
