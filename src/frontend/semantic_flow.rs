//! Binding-aware definite assignment, lexical loans, and reference provenance.
//! Raw pointers remain an explicit manual-lifetime boundary; references do not.
use super::*;
use std::hash::{Hash, Hasher};

#[path = "semantic_flow_owned.rs"]
mod owned;

type BindingId = usize;
type Path = Vec<Projection>;
/// A finite abstract region: all storage of `ty` reachable from a parameter
/// origin through aggregate projections and references. It is never a concrete
/// pointer; call sites expand it against their actual binding/reference graph.
#[derive(Clone)]
struct Region {
    ty: ResolvedType,
}
impl PartialEq for Region {
    fn eq(&self, other: &Self) -> bool {
        SemanticAnalyzer::type_identity(&self.ty) == SemanticAnalyzer::type_identity(&other.ty)
    }
}
impl Eq for Region {}
impl Hash for Region {
    fn hash<H: Hasher>(&self, state: &mut H) {
        SemanticAnalyzer::type_identity(&self.ty).hash(state);
    }
}
impl std::fmt::Debug for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Region")
            .field(&SemanticAnalyzer::type_identity(&self.ty))
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Projection {
    Field(String),
    Index(Option<i64>),
    Variant(usize),
    Deref,
    Reachable(Region),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Root {
    Local(BindingId),
    Parameter(usize, Path),
    Static(String),
    Raw,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Place {
    root: Root,
    path: Path,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Reference {
    at: Path,
    target: Place,
    pointee: Region,
    mutable: bool,
    ancestors: HashSet<BindingId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Value {
    owners: HashSet<BindingId>,
    ty: ResolvedType,
    references: Vec<Reference>,
    holders: HashSet<BindingId>,
}
impl Value {
    fn empty(ty: ResolvedType) -> Self {
        Self {
            owners: HashSet::new(),
            ty,
            references: vec![],
            holders: HashSet::new(),
        }
    }
    fn unit() -> Self {
        Self::empty(ResolvedType::UNIT)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Initialization {
    default: bool,
    children: HashMap<Projection, Initialization>,
}
impl Initialization {
    fn new(default: bool) -> Self {
        Self {
            default,
            children: HashMap::new(),
        }
    }
    fn set(&mut self, path: &[Projection], initialized: bool) {
        if let Some((first, rest)) = path.split_first() {
            self.children
                .entry(first.clone())
                .or_insert_with(|| Self::new(self.default))
                .set(rest, initialized);
        } else {
            self.default = initialized;
            self.children.clear();
        }
    }
    fn at(&self, path: &[Projection]) -> Self {
        match path.split_first() {
            Some((first, rest)) => self
                .children
                .get(first)
                .cloned()
                .unwrap_or_else(|| Self::new(self.default))
                .at(rest),
            None => self.clone(),
        }
    }
    fn join(&self, other: &Self) -> Self {
        let mut result = Self::new(self.default || other.default);
        for key in self.children.keys().chain(other.children.keys()) {
            let a = self
                .children
                .get(key)
                .cloned()
                .unwrap_or_else(|| Self::new(self.default));
            let b = other
                .children
                .get(key)
                .cloned()
                .unwrap_or_else(|| Self::new(other.default));
            result.children.insert(key.clone(), a.join(&b));
        }
        result
    }
    fn meet(&self, other: &Self) -> Self {
        let mut result = Self::new(self.default && other.default);
        for key in self.children.keys().chain(other.children.keys()) {
            let a = self
                .children
                .get(key)
                .cloned()
                .unwrap_or_else(|| Self::new(self.default));
            let b = other
                .children
                .get(key)
                .cloned()
                .unwrap_or_else(|| Self::new(other.default));
            result.children.insert(key.clone(), a.meet(&b));
        }
        result
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Slot {
    // May still own a resource on some incoming path, distinct from definite initialization.
    owned_live: bool,
    initialized: Initialization,
    written: Initialization,
    value: Value,
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct State {
    slots: HashMap<BindingId, Slot>,
    writes: HashMap<Place, Value>,
    definite_writes: HashSet<Place>,
}
#[derive(Clone)]
struct Binding {
    declaration: Span,
    name: String,
    ty: ResolvedType,
    depth: usize,
    mutable: bool,
}
#[derive(Clone)]
struct Address {
    place: Place,
    alternatives: Vec<Place>,
    ty: ResolvedType,
    permits: HashSet<BindingId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct WriteSummary {
    target: Place,
    value: Value,
    definite: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(super) struct FunctionFlow {
    owned_bindings: HashMap<Span, ResolvedType>,
    owned_moves: HashMap<Span, Span>,
    drop_calls: HashSet<String>,
    returned: Vec<Reference>,
    writes: Vec<WriteSummary>,
}
#[derive(Default)]
struct LoopExits {
    breaks: Vec<State>,
    continues: Vec<State>,
    scope_depth: usize,
}

struct Flow<'a> {
    owned_bindings: HashMap<Span, ResolvedType>,
    owned_moves: HashMap<Span, Span>,
    drop_calls: HashSet<String>,
    destructor_parameter: Option<Span>,
    analyzer: &'a SemanticAnalyzer,
    summaries: &'a HashMap<String, FunctionFlow>,
    bindings: Vec<Binding>,
    declarations: HashMap<Span, BindingId>,
    scopes: Vec<HashMap<String, BindingId>>,
    state: State,
    loops: Vec<LoopExits>,
    returned: Vec<Reference>,
    return_states: Vec<State>,
    temporary_loans: Vec<Reference>,
    validate: bool,
    protected_live: Vec<HashSet<BindingId>>,
    self_type: Option<ResolvedType>,
    function_key: String,
    return_type: ResolvedType,
}

impl SemanticAnalyzer {
    pub(super) fn analyze_flows(&mut self, program: &Program) -> Result<()> {
        let mut functions = Vec::new();
        for item in &program.items {
            match item {
                Item::Function(function) => functions.push(function.clone()),
                Item::Impl(block) => {
                    for method in &block.methods {
                        let mut method = method.clone();
                        method.name.name = format!("{}::{}", block.target.name, method.name.name);
                        functions.push(method);
                    }
                }
                _ => {}
            }
        }
        // Reference return/write summaries are independent of declaration order,
        // including recursive forwarding functions. Validation uses the fixed point.
        let mut summaries = HashMap::<String, FunctionFlow>::new();
        loop {
            let mut changed = false;
            for function in &functions {
                if !function.type_params.is_empty() {
                    continue;
                }
                let summary = Flow::new(self, &summaries, false).function(function)?;
                if summaries.get(&function.name.name) != Some(&summary) {
                    summaries.insert(function.name.name.clone(), summary);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for function in functions {
            if !function.type_params.is_empty() {
                continue;
            }
            let result=Flow::new(self, &summaries, true).function(&function)?;
            let key=self.facts.method_signatures.get(&function.name.name).map(|s|s.emitted_name.clone()).unwrap_or_else(||function.name.name.clone());
            for (span,ty) in result.owned_bindings { self.facts.owned_bindings.insert((key.clone(),span),ty); }
            for (span,binding) in result.owned_moves { self.facts.owned_moves.insert((key.clone(),span),binding); }
            self.facts.drop_calls.insert(key,result.drop_calls);
        }
        Ok(())
    }
}

impl<'a> Flow<'a> {
    fn new(
        analyzer: &'a SemanticAnalyzer,
        summaries: &'a HashMap<String, FunctionFlow>,
        validate: bool,
    ) -> Self {
        Self {
            owned_bindings: HashMap::new(),
            owned_moves: HashMap::new(),
            drop_calls: HashSet::new(),
            destructor_parameter: None,
            analyzer,
            summaries,
            bindings: vec![],
            declarations: HashMap::new(),
            scopes: vec![HashMap::new()],
            state: State::default(),
            loops: vec![],
            returned: vec![],
            return_states: vec![],
            temporary_loans: vec![],
            validate,
            protected_live: vec![],
            self_type: None,
            function_key: String::new(),
            return_type: ResolvedType::UNIT,
        }
    }
    fn error(&self, message: impl Into<String>, span: Span) -> Error {
        Error::semantic_expected("E2302", message, span)
    }
    fn resolve(&self, ty: &Type) -> Result<ResolvedType> {
        match ty {
            Type::Named(name, _) if name == "Self" => self
                .self_type
                .clone()
                .ok_or_else(|| self.error("impl Self type", ty.span())),
            Type::Pointer(inner, _) => Ok(ResolvedType::Pointer(Box::new(self.resolve(inner)?))),
            Type::Owned { inner, .. } => Ok(ResolvedType::Owned(Box::new(self.resolve(inner)?))),
            Type::Ref { mutable, inner, .. } => Ok(ResolvedType::Reference {
                mutable: *mutable,
                inner: Box::new(self.resolve(inner)?),
            }),
            Type::Array { elem, size, .. } => Ok(ResolvedType::Array {
                elem: Box::new(self.resolve(elem)?),
                size: *size,
            }),
            _ => self.analyzer.resolve_type(ty),
        }
    }
    fn function(mut self, function: &Function) -> Result<FunctionFlow> {
        self.destructor_parameter = if function.annotations.iter().any(|a|a.name.name=="drop") { function.params.first().map(|p|p.name.span) } else {None};
        self.function_key = self.analyzer.facts.method_signatures.get(&function.name.name)
            .map(|signature| signature.emitted_name.clone()).unwrap_or_else(|| function.name.name.clone());
        if let Some((target, _)) = function.name.name.rsplit_once("::") {
            self.self_type = Some(self.analyzer.resolve_type(&Type::Named(target.to_string(), function.name.span))?);
        }
        self.return_type = function
            .ret_type
            .as_ref()
            .map(|ty| self.resolve(ty))
            .transpose()?
            .unwrap_or(ResolvedType::UNIT);
        for (index, param) in function.params.iter().enumerate() {
            let ty = self.resolve(&param.ty)?;
            let mut value = Value::empty(ty.clone());
            self.parameter_references(&ty, &mut vec![], index, &mut value.references);
            let id = self.declare(&param.name, ty, Some(value));
            self.bindings[id].mutable = param.mutable || param.ownership == Ownership::Mut;
        }
        let outcome = self.block(&function.body, &HashSet::new())?;
        if let Some(value) = outcome {
            self.check_return(&value, function.body.span)?;
            self.return_states.push(self.state.clone());
        }
        let mut writes = Vec::new();
        let targets: HashSet<_> = self
            .return_states
            .iter()
            .flat_map(|state| state.writes.keys().cloned())
            .collect();
        for target in targets {
            let values: Vec<_> = self
                .return_states
                .iter()
                .filter_map(|state| state.writes.get(&target))
                .collect();
            let mut value = values[0].clone();
            for other in values.iter().skip(1) {
                Self::union_value(&mut value, other);
            }
            for reference in &mut value.references {
                reference.ancestors.clear();
            }
            value.holders.clear();
            let definite = self
                .return_states
                .iter()
                .all(|state| state.definite_writes.contains(&target));
            writes.push(WriteSummary {
                target,
                value,
                definite,
            });
        }
        // Deterministic ordering makes fixed-point convergence independent of HashMap order.
        writes.sort_by_key(|write| format!("{:?}", write.target));
        for reference in &mut self.returned {
            reference.ancestors.clear();
        }
        self.returned
            .sort_by_key(|reference| format!("{:?}", reference));
        self.returned.dedup();
        Ok(FunctionFlow {
            owned_bindings:self.owned_bindings,
            owned_moves:self.owned_moves,
            drop_calls:self.drop_calls,
            returned: self.returned,
            writes,
        })
    }
    fn declare(&mut self, name: &Ident, ty: ResolvedType, value: Option<Value>) -> BindingId {
        if matches!(ty,ResolvedType::Owned(_)) { self.owned_bindings.insert(name.span,ty.clone()); }
        if let Some(value)=&value { self.consume_owned_value(value); }
        let id = if let Some(id) = self.declarations.get(&name.span) {
            *id
        } else {
            let id = self.bindings.len();
            self.bindings.push(Binding {
                declaration: name.span,
                name: name.name.clone(),
                ty: ty.clone(),
                depth: self.scopes.len() - 1,
                mutable: false,
            });
            self.declarations.insert(name.span, id);
            id
        };
        if let Some(previous) = self
            .scopes
            .last_mut()
            .unwrap()
            .insert(name.name.clone(), id)
        {
            if previous != id {
                self.record_owned_drop(previous);
                self.state.slots.remove(&previous);
            }
        }
        self.state.slots.insert(
            id,
            Slot {
                owned_live: value.is_some() && matches!(ty.unqualified(), ResolvedType::Owned(_)),
                initialized: Initialization::new(value.is_some()),
                written: Initialization::new(value.is_some()),
                value: {
                    let mut v = value.unwrap_or_else(|| Value::empty(ty.clone()));
                    v.ty = ty;
                    v.owners.clear();
                    v
                },
            },
        );
        id
    }
    fn binding(&self, name: &str) -> Option<BindingId> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }
    fn fields(&self, ty: &ResolvedType) -> Vec<(String, ResolvedType)> {
        if let ResolvedType::Owned(inner) = ty.unqualified() { return self.fields(inner); }
        if let ResolvedType::Struct { name, fields } = ty.unqualified() {
            if let Some(Symbol {
                kind: SymbolKind::Struct { fields, .. },
                ..
            }) = self.analyzer.symbols.lookup(name)
            {
                fields.clone()
            } else {
                fields.clone()
            }
        } else {
            vec![]
        }
    }
    fn child_type(&self, ty: &ResolvedType, projection: &Projection) -> ResolvedType {
        if let ResolvedType::Owned(inner) = ty.unqualified() { return self.child_type(inner, projection); }
        let qualified = matches!(ty, ResolvedType::Volatile(_));
        let result = match (ty.unqualified(), projection) {
            (_, Projection::Reachable(region)) => region.ty.clone(),
            (ResolvedType::Enum { name }, Projection::Variant(index)) => self
                .analyzer
                .facts
                .enum_variants
                .get(name)
                .and_then(|variants| variants.get(*index))
                .map(|(_, fields)| ResolvedType::Tuple(fields.clone()))
                .unwrap_or(ResolvedType::Unknown),
            (ResolvedType::Tuple(types), Projection::Index(Some(index))) => types
                .get(*index as usize)
                .cloned()
                .unwrap_or(ResolvedType::Unknown),
            (ResolvedType::Struct { .. }, Projection::Field(field)) => self
                .fields(ty)
                .into_iter()
                .find(|(name, _)| name == field)
                .map(|(_, ty)| ty)
                .unwrap_or(ResolvedType::Unknown),
            (
                ResolvedType::Array { elem, .. }
                | ResolvedType::Slice(elem)
                | ResolvedType::Pointer(elem),
                Projection::Index(_),
            ) => *elem.clone(),
            (
                ResolvedType::Reference { inner, .. } | ResolvedType::Pointer(inner),
                Projection::Deref,
            ) => *inner.clone(),
            _ => ResolvedType::Unknown,
        };
        if qualified {
            ResolvedType::Volatile(Box::new(result))
        } else {
            result
        }
    }
    fn contains_reference(&self, ty: &ResolvedType) -> bool {
        match ty.unqualified() {
            ResolvedType::Reference { .. } => true,
            ResolvedType::Enum { name } => {
                self.analyzer
                    .facts
                    .enum_variants
                    .get(name)
                    .is_some_and(|variants| {
                        variants
                            .iter()
                            .any(|(_, fields)| fields.iter().any(|ty| self.contains_reference(ty)))
                    })
            }
            ResolvedType::Struct { .. } => self
                .fields(ty)
                .iter()
                .any(|(_, ty)| self.contains_reference(ty)),
            ResolvedType::Array { elem, .. } | ResolvedType::Slice(elem) => {
                self.contains_reference(elem)
            }
            ResolvedType::Tuple(types) => types.iter().any(|ty| self.contains_reference(ty)),
            _ => false,
        }
    }
    fn parameter_references(
        &self,
        ty: &ResolvedType,
        at: &mut Path,
        index: usize,
        result: &mut Vec<Reference>,
    ) {
        if !self.contains_reference(ty) {
            return;
        }
        match ty.unqualified() {
            ResolvedType::Reference { mutable, inner } => {
                let mut path = at.clone();
                path.push(Projection::Deref);
                result.push(Reference {
                    at: at.clone(),
                    pointee: Region { ty: *inner.clone() },
                    target: Place {
                        root: Root::Parameter(index, path),
                        path: vec![],
                    },
                    mutable: *mutable,
                    ancestors: HashSet::new(),
                });
            }
            ResolvedType::Enum { name } => {
                if let Some(variants) = self.analyzer.facts.enum_variants.get(name) {
                    for (variant, (_, fields)) in variants.iter().enumerate() {
                        at.push(Projection::Variant(variant));
                        for (index_field, ty) in fields.iter().enumerate() {
                            at.push(Projection::Index(Some(index_field as i64)));
                            self.parameter_references(ty, at, index, result);
                            at.pop();
                        }
                        at.pop();
                    }
                }
            }
            ResolvedType::Tuple(types) => {
                for (field, ty) in types.iter().enumerate() {
                    at.push(Projection::Index(Some(field as i64)));
                    self.parameter_references(ty, at, index, result);
                    at.pop();
                }
            }
            ResolvedType::Struct { .. } => {
                for (field, ty) in self.fields(ty) {
                    at.push(Projection::Field(field));
                    self.parameter_references(&ty, at, index, result);
                    at.pop();
                }
            }
            ResolvedType::Array { elem, size } => {
                if !self.contains_reference(elem) {return;}
                for i in 0..*size {
                    at.push(Projection::Index(Some(i as i64)));
                    self.parameter_references(elem, at, index, result);
                    at.pop();
                }
            }
            _ => {}
        }
    }
    fn symbolic_value(&self, ty: ResolvedType, place: &Place) -> Value {
        let mut value = Value::empty(ty.clone());
        if let Root::Parameter(index, prefix) = &place.root {
            let mut references = vec![];
            self.parameter_references(&ty, &mut vec![], *index, &mut references);
            for mut reference in references {
                if let Root::Parameter(_, path) = &mut reference.target.root {
                    let mut full = prefix.clone();
                    full.extend(place.path.clone());
                    full.extend(path.clone());
                    *path = full;
                }
                Self::normalize_reference(&mut reference);
                value.references.push(reference);
            }
        }
        value
    }
    fn initialized(&self, init: &Initialization, ty: &ResolvedType) -> bool {
        if init.children.is_empty() {
            return init.default;
        }
        match ty.unqualified() {
            ResolvedType::Struct { .. } => self.fields(ty).iter().all(|(field, ty)| {
                self.initialized(&init.at(&[Projection::Field(field.clone())]), ty)
            }),
            ResolvedType::Array { elem, size } => (0..*size).all(|index| {
                self.initialized(&init.at(&[Projection::Index(Some(index as i64))]), elem)
            }),
            ResolvedType::Tuple(elements) => elements.iter().enumerate().all(|(index, ty)| {
                self.initialized(&init.at(&[Projection::Index(Some(index as i64))]), ty)
            }),
            _ => init.default && init.children.values().all(|child| child.default),
        }
    }
    fn overlap(a: &[Projection], b: &[Projection]) -> bool {
        a.iter().zip(b).all(|(a, b)| match (a, b) {
            (Projection::Index(None), Projection::Index(_))
            | (Projection::Index(_), Projection::Index(None)) => true,
            _ => a == b,
        })
    }
    fn prefix(prefix: &[Projection], path: &[Projection]) -> bool {
        prefix.len() <= path.len() && Self::overlap(prefix, path)
    }
    fn access(&self, address: &Address, write: bool, span: Span) -> Result<()> {
        for place in &address.alternatives {
            let mut alternate = address.clone();
            alternate.place = place.clone();
            alternate.alternatives.clear();
            self.access(&alternate, write, span)?;
        }
        if !self.validate {
            return Ok(());
        }
        if let Root::Local(root) = address.place.root {
            for (holder, slot) in &self.state.slots {
                if address.permits.contains(holder) {
                    continue;
                }
                for loan in &slot.value.references {
                    if loan.target.root == Root::Local(root)
                        && Self::overlap(&loan.target.path, &address.place.path)
                        && (write || loan.mutable)
                    {
                        return Err(self.error(
                            format!(
                                "access to '{}' consistent with its active {} borrow",
                                self.bindings[root].name,
                                if loan.mutable { "mutable" } else { "shared" }
                            ),
                            span,
                        ));
                    }
                }
            }
            for loan in &self.temporary_loans {
                if loan.target.root == Root::Local(root)
                    && Self::overlap(&loan.target.path, &address.place.path)
                    && (write || loan.mutable)
                {
                    return Err(self.error("non-overlapping function argument borrows", span));
                }
            }
        }
        Ok(())
    }
    fn read(&mut self, address: &Address, consume: bool, span: Span) -> Result<Value> {
        let mut primary = address.clone();
        primary.alternatives.clear();
        let mut value = self.read_one(&primary, consume, span)?;
        for place in &address.alternatives {
            primary.place = place.clone();
            let alternate = self.read_one(&primary, consume, span)?;
            Self::union_value(&mut value, &alternate);
        }
        Ok(value)
    }
    fn read_one(&mut self, address: &Address, consume: bool, span: Span) -> Result<Value> {
        let owned = matches!(address.ty.unqualified(), ResolvedType::Owned(_));
        if self.validate && consume && owned && (!matches!(address.place.root, Root::Local(_)) || !address.place.path.is_empty() || !address.permits.is_empty()) {
            return Err(Error::semantic_expected("E2405", "move from a direct owned binding, not borrowed or projected storage", span));
        }
        self.access(address, consume && owned, span)?;
        let mut value = match address.place.root {
            Root::Local(id) => {
                let slot = self
                    .state
                    .slots
                    .get(&id)
                    .ok_or_else(|| self.error("reference to live local storage", span))?;
                if self.validate && matches!(self.bindings[id].ty.unqualified(), ResolvedType::Owned(_)) && !slot.initialized.default {
                    return Err(Error::semantic_expected("E2301", format!("'{}' definitely initialized before use (it may be uninitialized or moved)", self.bindings[id].name), span));
                }
                let init = slot.initialized.at(&address.place.path);
                let ready = if let Some(index) = address
                    .place
                    .path
                    .iter()
                    .position(|p| *p == Projection::Index(None))
                {
                    let mut aggregate_ty = self.bindings[id].ty.clone();
                    for projection in &address.place.path[..index] {
                        aggregate_ty = self.child_type(&aggregate_ty, projection);
                    }
                    self.initialized(
                        &slot.initialized.at(&address.place.path[..index]),
                        &aggregate_ty,
                    )
                } else {
                    self.initialized(&init, &address.ty)
                };
                if self.validate && !ready {
                    return Err(Error::semantic_expected("E2301",format!("'{}' definitely initialized before use (it may be uninitialized or moved)", self.bindings[id].name), span));
                }
                let mut value =
                    self.project_value(&slot.value, &address.place.path, address.ty.clone());
                value.holders.insert(id);
                if consume && (owned || value.references.iter().any(|reference| reference.mutable)) {
                    let slot = self.state.slots.get_mut(&id).unwrap();
                    if owned {
                        slot.owned_live = false;
                        self.owned_moves.insert(span,self.bindings[id].declaration);
                    }
                    slot.initialized.set(&address.place.path, false);
                    slot.value
                        .references
                        .retain(|reference| !Self::prefix(&address.place.path, &reference.at));
                }
                value
            }
            _ => {
                let mut value = self.symbolic_value(address.ty.clone(), &address.place);
                for (target, stored) in &self.state.writes {
                    if target.root == address.place.root
                        && Self::prefix(&address.place.path, &target.path)
                    {
                        let suffix = target.path[address.place.path.len()..].to_vec();
                        Self::replace_references(&mut value, &suffix, stored, false);
                    }
                }
                value
            }
        };
        for reference in &mut value.references {
            reference.ancestors.extend(address.permits.iter().copied());
        }
        value.ty = value.ty.unqualified().clone();
        Ok(value)
    }
    fn project_value(&self, value: &Value, path: &[Projection], ty: ResolvedType) -> Value {
        let mut result = Value::empty(ty);
        result.holders = value.holders.clone();
        for reference in &value.references {
            if Self::prefix(path, &reference.at) {
                let mut reference = reference.clone();
                reference.at = reference.at[path.len()..].to_vec();
                result.references.push(reference);
            }
        }
        result
    }
    fn parameter_path(place: &Place) -> Option<(usize, Path)> {
        let Root::Parameter(index, path) = &place.root else {
            return None;
        };
        let mut full = path.clone();
        full.extend(place.path.clone());
        Some((*index, full))
    }
    fn region_place(place: &Place, ty: &ResolvedType) -> Place {
        let Some((index, mut path)) = Self::parameter_path(place) else {
            return place.clone();
        };
        if let Some(at) = path
            .iter()
            .position(|p| matches!(p, Projection::Reachable(_)))
        {
            path.truncate(at);
        }
        path.push(Projection::Reachable(Region { ty: ty.clone() }));
        Place {
            root: Root::Parameter(index, path),
            path: vec![],
        }
    }
    fn normalize_reference(reference: &mut Reference) {
        if let Some((index, path)) = Self::parameter_path(&reference.target) {
            reference.target = Place {
                root: Root::Parameter(index, path.clone()),
                path: vec![],
            };
            if path.iter().any(|p| matches!(p, Projection::Reachable(_))) {
                reference.target = Self::region_place(&reference.target, &reference.pointee.ty);
            }
        }
    }
    fn join_reference(references: &mut Vec<Reference>, mut next: Reference) {
        Self::normalize_reference(&mut next);
        let mut index = 0;
        while index < references.len() {
            let existing = &references[index];
            if existing.at != next.at
                || existing.mutable != next.mutable
                || existing.pointee != next.pointee
            {
                index += 1;
                continue;
            }
            let mut joined = existing.target == next.target;
            if !joined {
                if let (Some((a, ap)), Some((b, bp))) = (
                    Self::parameter_path(&existing.target),
                    Self::parameter_path(&next.target),
                ) {
                    if a == b {
                        let ar = ap
                            .iter()
                            .position(|p| matches!(p, Projection::Reachable(_)));
                        let br = bp
                            .iter()
                            .position(|p| matches!(p, Projection::Reachable(_)));
                        let abase = &ap[..ar.unwrap_or(ap.len())];
                        let bbase = &bp[..br.unwrap_or(bp.len())];
                        // A recursive call may append a field after traversing
                        // the graph, so neither complete path is necessarily a
                        // prefix (node.value versus node.next.value). Widen the
                        // common prefix when path length grows, retaining every
                        // reachable address of the result type. Equal-depth
                        // distinct fields remain precise until recursion occurs.
                        let common_length =
                            abase.iter().zip(bbase).take_while(|(a, b)| a == b).count();
                        let common = if ar.is_some() || br.is_some() || abase.len() != bbase.len() {
                            Some(&abase[..common_length])
                        } else {
                            None
                        };
                        if let Some(base) = common {
                            next.target = Self::region_place(
                                &Place {
                                    root: Root::Parameter(a, base.to_vec()),
                                    path: vec![],
                                },
                                &next.pointee.ty,
                            );
                            joined = true;
                        }
                    }
                }
            }
            if joined {
                next.ancestors.extend(references.remove(index).ancestors);
                index = 0;
            } else {
                index += 1;
            }
        }
        references.push(next);
        references.sort_by_key(|reference| format!("{:?}", reference));
    }
    fn union_value(left: &mut Value, right: &Value) {
        left.owners.extend(right.owners.iter().copied());
        for reference in &right.references {
            Self::join_reference(&mut left.references, reference.clone());
        }
        left.holders.extend(right.holders.iter().copied());
    }
    fn replace_references(target: &mut Value, path: &[Projection], value: &Value, union: bool) {
        if !union {
            target
                .references
                .retain(|reference| !Self::prefix(path, &reference.at));
        }
        for reference in &value.references {
            let mut reference = reference.clone();
            let mut at = path.to_vec();
            at.extend(reference.at);
            reference.at = at;
            Self::join_reference(&mut target.references, reference);
        }
    }
    fn write(&mut self, address: &Address, value: Value, definite: bool, span: Span) -> Result<()> {
        let mut primary = address.clone();
        primary.alternatives.clear();
        self.write_one(
            &primary,
            value.clone(),
            definite && address.alternatives.is_empty(),
            span,
        )?;
        for place in &address.alternatives {
            primary.place = place.clone();
            self.write_one(&primary, value.clone(), false, span)?;
        }
        Ok(())
    }
    fn write_one(
        &mut self,
        address: &Address,
        mut value: Value,
        definite: bool,
        span: Span,
    ) -> Result<()> {
        if self.validate && matches!(address.ty.unqualified(), ResolvedType::Owned(_)) {
            let Root::Local(id) = address.place.root else { return Err(Error::semantic_expected("E2405", "owned assignment to a direct local binding", span)); };
            if !address.place.path.is_empty() || !address.permits.is_empty() {
                return Err(Error::semantic_expected("E2405", "owned assignment to a direct local binding", span));
            }
            if self.state.slots.get(&id).is_some_and(|slot| slot.owned_live) {
                return Err(Error::semantic_expected("E2405", "consume the previous owned value before replacing it", span));
            }
        }
        self.consume_owned_value(&value);
        value.owners.clear();
        value.ty = address.ty.clone();
        for reference in &mut value.references {
            Self::normalize_reference(reference);
        }
        self.access(address, true, span)?;
        if self.validate {
            for reference in &value.references {
                if let Root::Local(source) = reference.target.root {
                    let escapes = match address.place.root {
                        Root::Local(target) => {
                            self.bindings[source].depth > self.bindings[target].depth
                                || source == target
                        }
                        Root::Parameter(_, _) | Root::Static(_) => true,
                        Root::Raw => false,
                    };
                    if escapes {
                        return Err(self.error(
                            format!(
                                "reference to '{}' not escaping its storage lifetime",
                                self.bindings[source].name
                            ),
                            span,
                        ));
                    }
                }
            }
        }
        match address.place.root {
            Root::Local(id) => {
                if let Some(slot) = self.state.slots.get_mut(&id) {
                    if definite && !address.place.path.contains(&Projection::Index(None)) {
                        slot.initialized.set(&address.place.path, true);
                    }
                    slot.written.set(&address.place.path, true);
                    if matches!(address.ty.unqualified(), ResolvedType::Owned(_)) { slot.owned_live = true; }
                    Self::replace_references(
                        &mut slot.value,
                        &address.place.path,
                        &value,
                        !definite,
                    );
                }
            }
            Root::Parameter(_, _) => {
                if definite {
                    self.state.definite_writes.insert(address.place.clone());
                }
                if let Some(previous) = self.state.writes.get_mut(&address.place) {
                    if definite {
                        *previous = value;
                    } else {
                        Self::union_value(previous, &value);
                    }
                } else {
                    self.state.writes.insert(address.place.clone(), value);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn check_return(&mut self, value: &Value, span: Span) -> Result<()> {
        for reference in &value.references {
            if let Root::Local(id) = reference.target.root {
                if self.validate {
                    return Err(self.error(
                        format!(
                            "returned reference not pointing into local '{}'",
                            self.bindings[id].name
                        ),
                        span,
                    ));
                }
            } else {
                Self::join_reference(&mut self.returned, reference.clone());
            }
        }
        self.consume_owned_value(value);
        self.record_owned_cleanup(0);
        Ok(())
    }
    fn merge(states: &[State]) -> State {
        let Some(first) = states.first() else {
            return State::default();
        };
        let mut result = first.clone();
        for state in &states[1..] {
            result
                .definite_writes
                .retain(|place| state.definite_writes.contains(place));
            result.slots.retain(|id, slot| {
                if let Some(other) = state.slots.get(id) {
                    slot.initialized = slot.initialized.meet(&other.initialized);
                    slot.written = slot.written.join(&other.written);
                    slot.owned_live |= other.owned_live;
                    Self::union_value(&mut slot.value, &other.value);
                    true
                } else {
                    false
                }
            });
            for (place, value) in &state.writes {
                if let Some(previous) = result.writes.get_mut(place) {
                    Self::union_value(previous, value);
                } else {
                    result.writes.insert(place.clone(), value.clone());
                }
            }
        }
        result
    }
    fn end_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for id in scope.values() {
                self.state.slots.remove(id);
            }
        }
    }
    fn expire(&mut self, live: &HashSet<String>, protected: &HashSet<BindingId>) {
        let mut identities: HashSet<_> =
            live.iter().filter_map(|name| self.binding(name)).collect();
        identities.extend(protected.iter().copied());
        for outer in &self.protected_live {
            identities.extend(outer.iter().copied());
        }
        // A binding remains live when a live reference points into it, even
        // if its source name never appears again (e.g. `outer: &&Tuple`).
        // Follow declaration identities to a finite fixed point before
        // dropping loan provenance; textual liveness alone loses origins.
        let mut pending: Vec<_> = identities.iter().copied().collect();
        while let Some(id) = pending.pop() {
            if let Some(slot) = self.state.slots.get(&id) {
                for reference in &slot.value.references {
                    if let Root::Local(target) = reference.target.root {
                        if identities.insert(target) { pending.push(target); }
                    }
                    for ancestor in &reference.ancestors {
                        if identities.insert(*ancestor) { pending.push(*ancestor); }
                    }
                }
            }
        }
        for (id, slot) in &mut self.state.slots {
            if !identities.contains(id) {
                slot.value.references.clear();
            }
        }
    }
    fn block(&mut self, block: &Block, live_after: &HashSet<String>) -> Result<Option<Value>> {
        let protected: HashSet<_> = live_after
            .iter()
            .filter_map(|name| self.binding(name))
            .collect();
        self.protected_live.push(protected.clone());
        self.scopes.push(HashMap::new());
        let mut result = Some(Value::unit());
        for (index, statement) in block.stmts.iter().enumerate() {
            if result.is_none() {
                break;
            }
            let local_after: HashSet<_> = live_after
                .iter()
                .filter(|name| self.binding(name).is_none_or(|id| protected.contains(&id)))
                .cloned()
                .collect();
            let live = live_names(&block.stmts[index..], &local_after);
            self.expire(&live, &protected);
            let after = live_names(&block.stmts[index + 1..], &local_after);
            result = self.statement(statement, &after)?;
            if result.is_some() {self.cleanup_owned_temporaries(self.scopes.len()-1,if index+1==block.stmts.len(){result.as_ref()}else{None});}
        }
        if let Some(value) = &result {
            if self.validate {
                for reference in &value.references {
                    if let Root::Local(id) = reference.target.root {
                        if self.bindings[id].depth >= self.scopes.len() - 1 {
                            return Err(self.error(
                                format!(
                                    "reference to '{}' not escaping its block",
                                    self.bindings[id].name
                                ),
                                block.span,
                            ));
                        }
                    }
                }
            }
        }
        if let Some(value)=&result { self.consume_owned_value(value); self.record_owned_cleanup(self.scopes.len()-1); }
        self.end_scope();
        self.protected_live.pop();
        Ok(result)
    }
    fn statement(&mut self, statement: &Stmt, live: &HashSet<String>) -> Result<Option<Value>> {
        match statement {
            Stmt::Let {
                name,
                ty,
                value,
                mutable,
                ..
            } => {
                let shared = ty
                    .as_ref()
                    .is_some_and(|ty| matches!(ty, Type::Ref { mutable: false, .. }));
                let value = if let Some(expr) = value {
                    match self.expr(expr, !shared, live)? {
                        Some(mut value) => {
                            if shared {
                                for reference in &mut value.references {
                                    reference.mutable = false;
                                    reference.ancestors.extend(value.holders.iter().copied());
                                }
                            }
                            Some(value)
                        }
                        None => return Ok(None),
                    }
                } else {
                    None
                };
                let ty = if let Some(ty) = ty {
                    self.resolve(ty)?
                } else {
                    value
                        .as_ref()
                        .map(|v| v.ty.clone())
                        .unwrap_or(ResolvedType::Unknown)
                };
                let id = self.declare(name, ty, value);
                self.bindings[id].mutable = *mutable;
                Ok(Some(Value::unit()))
            }
            Stmt::Expr(expr) => self.expr(expr, true, live),
            Stmt::Semi(expr) => Ok(self.expr(expr, true, live)?.map(|_| Value::unit())),
            Stmt::Return { value, span } => {
                let value = if let Some(expr) = value {
                    match self.expr(expr, true, live)? {
                        Some(value) => value,
                        None => return Ok(None),
                    }
                } else {
                    Value::unit()
                };
                self.check_return(&value, *span)?;
                self.return_states.push(self.state.clone());
                Ok(None)
            }
            Stmt::Break { .. } => {
                if let Some(exits)=self.loops.last() { self.record_owned_cleanup(exits.scope_depth); }
                if let Some(exits) = self.loops.last_mut() {
                    let mut state = self.state.clone();
                    state
                        .slots
                        .retain(|id, _| self.bindings[*id].depth < exits.scope_depth);
                    exits.breaks.push(state);
                }
                Ok(None)
            }
            Stmt::Continue { .. } => {
                if let Some(exits)=self.loops.last() { self.record_owned_cleanup(exits.scope_depth); }
                if let Some(exits) = self.loops.last_mut() {
                    let mut state = self.state.clone();
                    state
                        .slots
                        .retain(|id, _| self.bindings[*id].depth < exits.scope_depth);
                    exits.continues.push(state);
                }
                Ok(None)
            }
            Stmt::Empty { .. } => Ok(Some(Value::unit())),
        }
    }
    fn indirect(&self, value: Value, span: Span) -> Result<Address> {
        let ty = match value.ty.unqualified() {
            ResolvedType::Reference { inner, .. } | ResolvedType::Pointer(inner) => *inner.clone(),
            _ => return Err(self.error("pointer or reference", span)),
        };
        if matches!(value.ty.unqualified(), ResolvedType::Pointer(_)) {
            return Ok(Address {
                place: Place {
                    root: Root::Raw,
                    path: vec![],
                },
                alternatives: vec![],
                ty,
                permits: HashSet::new(),
            });
        }
        let Some(reference) = value
            .references
            .iter()
            .find(|reference| reference.at.is_empty())
        else {
            // Recursive summary discovery starts with no known returned origins.
            if !self.validate {
                return Ok(Address {
                    place: Place {
                        root: Root::Raw,
                        path: vec![],
                    },
                    alternatives: vec![],
                    ty,
                    permits: HashSet::new(),
                });
            }
            return Err(self.error("reference with a proven live origin", span));
        };
        let mut permits = value.holders;
        for origin in &value.references {
            permits.extend(origin.ancestors.iter().copied());
        }
        Ok(Address {
            place: reference.target.clone(),
            alternatives: value
                .references
                .iter()
                .filter(|r| r.at.is_empty() && r.target != reference.target)
                .map(|r| r.target.clone())
                .collect(),
            ty,
            permits,
        })
    }
    fn address(&mut self, expression: &Expr, live: &HashSet<String>) -> Result<Option<Address>> {
        let address = match expression {
            Expr::Ident(name) => {
                if let Some(id) = self.binding(&name.name) {
                    Address {
                        place: Place {
                            root: Root::Local(id),
                            path: vec![],
                        },
                        alternatives: vec![],
                        ty: self.bindings[id].ty.clone(),
                        permits: HashSet::new(),
                    }
                } else {
                    let ty = self
                        .analyzer
                        .symbols
                        .lookup(&name.name)
                        .map(|symbol| symbol.ty.clone())
                        .unwrap_or(ResolvedType::Unknown);
                    Address {
                        place: Place {
                            root: Root::Static(name.name.clone()),
                            path: vec![],
                        },
                        alternatives: vec![],
                        ty,
                        permits: HashSet::new(),
                    }
                }
            }
            Expr::Deref { expr, span } => {
                let Some(value) = self.expr(expr, false, live)? else {
                    return Ok(None);
                };
                self.indirect(value, *span)?
            }
            Expr::Field { expr, field, .. } => {
                let Some(mut base) = self.address(expr, live)? else {
                    return Ok(None);
                };
                while matches!(
                    base.ty.unqualified(),
                    ResolvedType::Reference { .. } | ResolvedType::Pointer(_)
                ) {
                    let value = self.read(&base, false, expr.span())?;
                    base = self.indirect(value, expr.span())?;
                }
                let field = if matches!(base.ty.unqualified(), ResolvedType::Tuple(_)) {
                    Projection::Index(field.name.parse::<i64>().ok())
                } else { Projection::Field(field.name.clone()) };
                base.ty = self.child_type(&base.ty, &field);
                base.place.path.push(field.clone());
                for alternative in &mut base.alternatives {
                    alternative.path.push(field.clone());
                }
                base
            }
            Expr::Index { expr, index, .. } => {
                let Some(mut base) = self.address(expr, live)? else {
                    return Ok(None);
                };
                // Snapshot the collection reference before evaluating the
                // index, just as the native address calculation does.
                let temporaries = self.temporary_loans.len();
                while matches!(base.ty.unqualified(), ResolvedType::Reference { .. }) {
                    let value = self.read(&base, false, expr.span())?;
                    self.hold_temporary(&value, expr.span())?;
                    base = self.indirect(value, expr.span())?;
                }
                let raw = matches!(base.ty.unqualified(), ResolvedType::Pointer(_));
                if raw {
                    let value = self.read(&base, false, expr.span())?;
                    base = self.indirect(value, expr.span())?;
                }
                let index_value = self.expr(index, true, live);
                self.temporary_loans.truncate(temporaries);
                if index_value?.is_none() {
                    return Ok(None);
                }
                if !raw {
                    let projection = Projection::Index(
                        if let Expr::Literal(Literal::Int(index, _)) = &**index {
                            i64::try_from(*index).ok()
                        } else {
                            None
                        },
                    );
                    base.ty = self.child_type(&base.ty, &projection);
                    base.place.path.push(projection.clone());
                    for alternative in &mut base.alternatives {
                        alternative.path.push(projection.clone());
                    }
                }
                base
            }

            _ => {
                let Some(value) = self.expr(expression, false, live)? else {
                    return Ok(None);
                };
                if matches!(
                    value.ty,
                    ResolvedType::Reference { .. } | ResolvedType::Pointer(_)
                ) {
                    self.indirect(value, expression.span())?
                } else {
                    return Err(self.error("addressable storage", expression.span()));
                }
            }
        };
        Ok(Some(address))
    }
    fn writable(&self, address: &Address, span: Span) -> Result<()> {
        if let Root::Static(name) = &address.place.root {
            if self.validate
                && !self
                    .analyzer
                    .symbols
                    .lookup(name)
                    .is_some_and(|symbol| symbol.mutable)
            {
                return Err(self.error(format!("mutable global '{}'", name), span));
            }
        }
        for place in &address.alternatives {
            let mut alternate = address.clone();
            alternate.place = place.clone();
            alternate.alternatives.clear();
            self.writable(&alternate, span)?;
        }
        if let Root::Local(id) = address.place.root {
            if self.validate && !address.place.path.is_empty() && matches!(self.bindings[id].ty.unqualified(), ResolvedType::Owned(_)) && self.state.slots.get(&id).is_some_and(|slot| !slot.initialized.default) {
                return Err(Error::semantic_expected("E2301", "initialized owner before accessing its fields", span));
            }
            if self.validate && address.permits.is_empty() && !self.bindings[id].mutable {
                if let Some(slot) = self.state.slots.get(&id) {
                    let path = if let Some(index) = address
                        .place
                        .path
                        .iter()
                        .position(|p| *p == Projection::Index(None))
                    {
                        &address.place.path[..index]
                    } else {
                        &address.place.path
                    };
                    let init = slot.written.at(path);
                    fn any(init: &Initialization) -> bool {
                        init.default || init.children.values().any(any)
                    }
                    if any(&init) {
                        return Err(Error::semantic_expected(
                            "E2303",
                            format!(
                                "mutable binding for reassignment of '{}'",
                                self.bindings[id].name
                            ),
                            span,
                        ));
                    }
                }
            }
        }
        Ok(())
    }
    /// A projection of an aggregate temporary reads a value, not borrowed
    /// storage. Preserve the existing place path for locals and references.
    fn projection_type(&self, expression: &Expr) -> Option<&ResolvedType> {
        let key = (self.function_key.clone(), expression.span());
        self.analyzer.facts.expr_types.get(&key)
            .or_else(|| self.analyzer.facts.places.get(&key).map(|place| &place.ty))
    }

    fn has_storage(&self, expression: &Expr) -> bool {
        match expression {
            Expr::Ident(_) | Expr::Deref { .. } => true,
            Expr::Field { expr, .. } | Expr::Index { expr, .. } => {
                self.has_storage(expr) || self.projection_type(expr)
                    .is_some_and(|ty| matches!(ty.unqualified(), ResolvedType::Pointer(_) | ResolvedType::Reference { .. }))
            }
            _ => false,
        }
    }

    fn value_aggregate(&self, expression: &Expr) -> bool {
        !self.has_storage(expression) && self.projection_type(expression)
            .is_some_and(|ty| matches!(ty.unqualified(), ResolvedType::Tuple(_) | ResolvedType::Struct { .. } | ResolvedType::Array { .. } | ResolvedType::Owned(_)))
    }

    fn expr_inner(
        &mut self,
        expression: &Expr,
        consume: bool,
        live: &HashSet<String>,
    ) -> Result<Option<Value>> {
        let value = match expression {
            Expr::Literal(literal) => Value::empty(self.analyzer.literal_type(literal)),
            Expr::Layout { .. } => Value::empty(ResolvedType::Primitive(PrimitiveType::Usize(self.analyzer.facts.pointer_width))),
            Expr::Ident(name) if self.binding(&name.name).is_none() => Value::empty(
                self.analyzer
                    .symbols
                    .lookup(&name.name)
                    .map(|symbol| symbol.ty.clone())
                    .unwrap_or(ResolvedType::Unknown),
            ),
            Expr::Field { expr, field, .. } if self.value_aggregate(expr) => {
                let Some(base) = self.expr(expr, consume, live)? else { return Ok(None); };
                let projection = if matches!(base.ty.unqualified(), ResolvedType::Tuple(_)) {
                    Projection::Index(field.name.parse::<i64>().ok())
                } else { Projection::Field(field.name.clone()) };
                let ty = self.child_type(&base.ty, &projection);
                self.project_value(&base, &[projection], ty)
            }
            Expr::Index { expr, index, .. } if self.value_aggregate(expr) => {
                let Some(base) = self.expr(expr, consume, live)? else { return Ok(None); };
                let temporaries = self.temporary_loans.len();
                self.hold_temporary(&base, expr.span())?;
                let index_value = self.expr(index, false, live)?;
                self.temporary_loans.truncate(temporaries);
                if index_value.is_none() { return Ok(None); }
                let projection = Projection::Index(if let Expr::Literal(Literal::Int(index, _)) = &**index { i64::try_from(*index).ok() } else { None });
                let ty = self.child_type(&base.ty, &projection);
                self.project_value(&base, &[projection], ty)
            }
            Expr::Ident(_) | Expr::Deref { .. } | Expr::Field { .. } | Expr::Index { .. } => {
                let Some(address) = self.address(expression, live)? else {
                    return Ok(None);
                };
                if self.validate && consume && matches!(address.ty.unqualified(), ResolvedType::Owned(_)) && !matches!(expression, Expr::Ident(_)) {
                    return Err(Error::semantic_expected("E2405", "move from a direct owned binding, not borrowed or projected storage", expression.span()));
                }
                self.read(&address, consume, expression.span())?
            }
            Expr::Path { segments, .. } => Value::empty(
                self.analyzer
                    .symbols
                    .lookup(
                        &segments
                            .iter()
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join("::"),
                    )
                    .map(|s| s.ty.clone())
                    .unwrap_or(ResolvedType::Unknown),
            ),
            Expr::Ref {
                mutable,
                expr,
                span,
            } => {
                let Some(address) = self.address(expr, live)? else {
                    return Ok(None);
                };
                self.read(&address, false, *span)?;
                self.access(&address, *mutable, *span)?;
                if *mutable {
                    self.writable(&address, *span)?;
                }
                let pointee = Region {
                    ty: address.ty.clone(),
                };
                let mut value = Value::empty(ResolvedType::Reference {
                    mutable: *mutable,
                    inner: Box::new(address.ty),
                });
                for target in std::iter::once(address.place).chain(address.alternatives) {
                    value.references.push(Reference {
                        at: vec![],
                        target,
                        pointee: pointee.clone(),
                        mutable: *mutable,
                        ancestors: address.permits.clone(),
                    });
                }
                value
            }
            Expr::Binary {
                left,
                op,
                right,
                span,
            } if op.is_assignment() =>
            {
                let Some(address) = self.address(left, live)? else {
                    return Ok(None);
                };
                if *op != BinOp::Assign {
                    self.read(&address, false, *span)?;
                }
                if *op == BinOp::Assign && address.place.path.is_empty() {
                    if let Root::Local(id) = address.place.root {
                        let mut used = HashSet::new();
                        expression_names(right, &mut used);
                        if !used.contains(&self.bindings[id].name) {
                            if let Some(slot) = self.state.slots.get_mut(&id) {
                                slot.value.references.clear();
                            }
                        }
                    }
                }
                let Some(value) = self.expr(right, true, live)? else {
                    return Ok(None);
                };
                self.writable(&address, *span)?;
                self.write(&address, value, true, *span)?;
                Value::unit()
            }
            Expr::Binary {
                left, op, right, ..
            } => {
                let skips = matches!(
                    (op, left.as_ref()),
                    (BinOp::And, Expr::Literal(Literal::Bool(false, _)))
                        | (BinOp::Or, Expr::Literal(Literal::Bool(true, _)))
                );
                let mandatory = matches!(
                    (op, left.as_ref()),
                    (BinOp::And, Expr::Literal(Literal::Bool(true, _)))
                        | (BinOp::Or, Expr::Literal(Literal::Bool(false, _)))
                );
                let Some(left) = self.expr(left, true, live)? else {
                    return Ok(None);
                };
                if skips {
                    return Ok(Some(Value::empty(ResolvedType::BOOL)));
                }
                let before = self.state.clone();
                let right = self.expr(right, true, live)?;
                if matches!(op, BinOp::And | BinOp::Or) && !mandatory {
                    self.state = if right.is_some() {
                        Self::merge(&[before, self.state.clone()])
                    } else {
                        before
                    };
                } else if right.is_none() {
                    return Ok(None);
                }
                let ty = if matches!(
                    op,
                    BinOp::Eq
                        | BinOp::Ne
                        | BinOp::Lt
                        | BinOp::Le
                        | BinOp::Gt
                        | BinOp::Ge
                        | BinOp::And
                        | BinOp::Or
                ) {
                    ResolvedType::BOOL
                } else {
                    left.ty
                };
                Value::empty(ty)
            }
            Expr::Unary { expr, .. } => {
                let Some(value) = self.expr(expr, consume, live)? else {
                    return Ok(None);
                };
                value
            }
            Expr::Try { expr, span } => {
                let Some(value) = self.expr(expr, true, live)? else {
                    return Ok(None);
                };
                let (ok, ok_type, err, error_type) =
                    self.analyzer.result_payloads(&value.ty, *span)?;
                let (_, _, returned_err, _) =
                    self.analyzer.result_payloads(&self.return_type, *span)?;
                let error = self.project_value(
                    &value,
                    &[Projection::Variant(err), Projection::Index(Some(0))],
                    error_type,
                );
                let mut returned = Value::empty(self.return_type.clone());
                Self::replace_references(
                    &mut returned,
                    &[
                        Projection::Variant(returned_err),
                        Projection::Index(Some(0)),
                    ],
                    &error,
                    false,
                );
                self.check_return(&returned, *span)?;
                self.return_states.push(self.state.clone());
                self.project_value(
                    &value,
                    &[Projection::Variant(ok), Projection::Index(Some(0))],
                    ok_type,
                )
            }
            Expr::Cast { expr, ty, .. } => {
                let Some(mut value) = self.expr(expr, consume, live)? else {
                    return Ok(None);
                };
                self.consume_owned_value(&value);
                value.owners.clear();
                value.ty = self.resolve(ty)?;
                if !matches!(value.ty, ResolvedType::Reference { .. }) {
                    value.references.clear();
                }
                value
            }
            Expr::Block(block) | Expr::Unsafe { body: block, .. } => {
                return self.block(block, live)
            }
            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                if self.condition(cond, live)?.is_none() {
                    return Ok(None);
                }
                let before = self.state.clone();
                let first = self.block(then_block, live)?;
                let first_state = self.state.clone();
                self.state = before.clone();
                let second = if let Some(block) = else_block {
                    self.block(block, live)?
                } else {
                    Some(Value::unit())
                };
                let mut states = vec![];
                if first.is_some() {
                    states.push(first_state);
                }
                if second.is_some() {
                    states.push(self.state.clone());
                }
                self.state = Self::merge(&states);
                match (first, second) {
                    (Some(mut a), Some(b)) => {
                        Self::union_value(&mut a, &b);
                        a
                    }
                    (Some(a), None) | (None, Some(a)) => a,
                    _ => return Ok(None),
                }
            }
            Expr::Match { expr, arms, .. } => {
                let Some(scrutinee) = self.condition(expr, live)? else {
                    return Ok(None);
                };
                let before = self.state.clone();
                let mut states = vec![];
                let mut result: Option<Value> = None;
                for arm in arms {
                    self.state = before.clone();
                    self.scopes.push(HashMap::new());
                    self.bind_pattern(&arm.pattern, &scrutinee)?;
                    if let Some(guard) = &arm.guard {
                        if self.condition(guard, live)?.is_none() {
                            self.end_scope();
                            continue;
                        }
                    }
                    if let Some(value) = self.expr(&arm.body, consume, live)? {
                        self.consume_owned_value(&value);
                        self.record_owned_cleanup(self.scopes.len()-1);
                        self.end_scope();
                        states.push(self.state.clone());
                        if let Some(previous) = &mut result {
                            Self::union_value(previous, &value);
                        } else {
                            result = Some(value);
                        }
                    } else {
                        self.end_scope();
                    }
                }
                self.state = Self::merge(&states);
                return Ok(result);
            }
            Expr::Loop { body, .. } => return self.loop_expr(None, body, None, live),
            Expr::While { cond, body, .. } => return self.loop_expr(Some(cond), body, None, live),
            Expr::For {
                var, iter, body, ..
            } => {
                if self.expr(iter, true, live)?.is_none() {
                    return Ok(None);
                }
                return self.loop_expr(None, body, Some(var), live);
            }
            Expr::ArrayRepeat {value,..} => {
                let Some(value)=self.expr(value,true,live)? else{return Ok(None)};
                let ty=self.projection_type(expression).cloned().unwrap_or(ResolvedType::Unknown);
                let ResolvedType::Array{size,..}=&ty else{return Err(Error::semantic_expected("E2501","checked array repeat type",expression.span()))};
                let size=*size;
                let mut result=Value::empty(ty);
                if size>0{Self::replace_references(&mut result,&[Projection::Index(if size==1{Some(0)}else{None})],&value,false);}
                result
            }
            Expr::Array { elements, .. } | Expr::Tuple { elements, .. } => {
                let temporaries = self.temporary_loans.len();
                let mut values = vec![];
                for expr in elements {
                    let Some(value) = self.expr(expr, true, live)? else {
                        self.temporary_loans.truncate(temporaries);
                        return Ok(None);
                    };
                    self.hold_temporary(&value, expr.span())?;
                    values.push(value);
                }
                self.temporary_loans.truncate(temporaries);
                let ty = if matches!(expression, Expr::Array { .. }) {
                    ResolvedType::Array {
                        elem: Box::new(
                            values
                                .first()
                                .map(|v| v.ty.clone())
                                .unwrap_or(ResolvedType::Unknown),
                        ),
                        size: values.len(),
                    }
                } else {
                    if values.is_empty() {
                        ResolvedType::UNIT
                    } else {
                        ResolvedType::Tuple(values.iter().map(|v| v.ty.clone()).collect())
                    }
                };
                let mut result = Value::empty(ty);
                for (index, value) in values.iter().enumerate() {
                    Self::replace_references(
                        &mut result,
                        &[Projection::Index(Some(index as i64))],
                        value,
                        false,
                    );
                }
                result
            }
            Expr::StructLit { name, fields, .. } => {
                let temporaries = self.temporary_loans.len();
                let ty = self.resolve(&Type::Named(name.name.clone(), name.span))?;
                let mut result = Value::empty(ty);
                for (field, expr) in fields {
                    let Some(value) = self.expr(expr, true, live)? else {
                        self.temporary_loans.truncate(temporaries);
                        return Ok(None);
                    };
                    self.hold_temporary(&value, expr.span())?;
                    Self::replace_references(
                        &mut result,
                        &[Projection::Field(field.name.clone())],
                        &value,
                        false,
                    );
                }
                self.temporary_loans.truncate(temporaries);
                result
            }
            Expr::Call { func, args, span } => return self.call(func, args, *span, live),
            Expr::MethodCall { expr, args, span, .. } => {
                let resolved = self.analyzer.facts.method_calls.get(&(self.function_key.clone(), *span))
                    .cloned().ok_or_else(|| self.error("resolved method call", *span))?;
                match resolved {
                    ResolvedMethodCall::DivergingReceiver => return self.expr(expr, false, live),
                    ResolvedMethodCall::PointerAdd { .. } => {
                        let Some(value) = self.expr(expr, false, live)? else { return Ok(None); };
                        for arg in args {
                            if self.expr(arg, true, live)?.is_none() { return Ok(None); }
                        }
                        value
                    }
                    ResolvedMethodCall::Function { symbol, receiver } => {
                        let receiver = match receiver {
                            ReceiverAdjustment::Value => expr.as_ref().clone(),
                            ReceiverAdjustment::Borrow { mutable } => Expr::Ref {
                                mutable, expr: expr.clone(), span: expr.span(),
                            },
                        };
                        let mut full_args = vec![receiver];
                        full_args.extend(args.iter().cloned());
                        return self.call(&Expr::Ident(Ident { name: symbol, span: *span }), &full_args, *span, live);
                    }
                }
            }
            Expr::Range { start, end, .. } => {
                for expr in [start, end].into_iter().flatten() {
                    if self.condition(expr, live)?.is_none() {
                        return Ok(None);
                    }
                }
                Value::unit()
            }
            Expr::Asm { operands, .. } => {
                let mut outputs = vec![];
                for operand in operands {
                    let Some(expr) = &operand.expr else {
                        continue;
                    };
                    match operand.kind {
                        AsmOperandKind::Output | AsmOperandKind::InOut => {
                            let Some(address) = self.address(expr, live)? else {
                                return Ok(None);
                            };
                            self.writable(&address, expr.span())?;
                            if matches!(operand.kind, AsmOperandKind::InOut) {
                                self.read(&address, false, expr.span())?;
                            }
                            outputs.push((address, expr.span()));
                        }
                        AsmOperandKind::Input => {
                            if self.expr(expr, false, live)?.is_none() {
                                return Ok(None);
                            }
                        }
                        AsmOperandKind::Clobber => {}
                    }
                }
                for (address, span) in outputs {
                    let value = Value::empty(address.ty.clone());
                    self.write(&address, value, true, span)?;
                }
                Value::unit()
            }
            Expr::Closure { .. } => Value::empty(ResolvedType::Unknown),
        };
        Ok(Some(value))
    }
    fn loop_expr(
        &mut self,
        condition: Option<&Expr>,
        body: &Block,
        var: Option<&Ident>,
        live: &HashSet<String>,
    ) -> Result<Option<Value>> {
        let entry = self.state.clone();
        let mut head = entry.clone();
        let mut loop_live = live.clone();
        block_names(body, &mut loop_live);
        if let Some(cond) = condition {
            expression_names(cond, &mut loop_live);
        }
        let zero = var.is_some()
            || condition.is_some_and(|cond| !matches!(cond, Expr::Literal(Literal::Bool(true, _))));
        loop {
            self.state = head.clone();
            if let Some(cond) = condition {
                if self.condition(cond, &loop_live)?.is_none() {
                    return Ok(None);
                }
            }
            let exit_condition = self.state.clone();
            self.scopes.push(HashMap::new());
            if let Some(var) = var {
                self.declare(
                    var,
                    ResolvedType::I64,
                    Some(Value::empty(ResolvedType::I64)),
                );
            }
            self.loops.push(LoopExits {
                scope_depth: self.scopes.len() - 1,
                ..LoopExits::default()
            });
            let outcome = self.block(body, &loop_live)?;
            let mut exits = self.loops.pop().unwrap();
            self.end_scope();
            if outcome.is_some() {
                exits.continues.push(self.state.clone());
            }
            let mut incoming = vec![entry.clone()];
            incoming.extend(exits.continues);
            let next = Self::merge(&incoming);
            if next == head {
                if zero {
                    exits.breaks.push(exit_condition);
                }
                if exits.breaks.is_empty() {
                    self.state = entry;
                    return Ok(None);
                }
                self.state = Self::merge(&exits.breaks);
                return Ok(Some(Value::unit()));
            }
            head = next;
        }
    }
    fn bind_pattern(&mut self, pattern: &Pattern, value: &Value) -> Result<()> {
        match pattern {
            Pattern::Binding { name, mutable, .. } => {
                let id = self.declare(name, value.ty.clone(), Some(value.clone()));
                self.bindings[id].mutable = *mutable;
            }
            Pattern::Variant {
                variant, fields, ..
            } => {
                if let ResolvedType::Enum { name } = &value.ty {
                    if let Some((tag, (_, types))) = self
                        .analyzer
                        .facts
                        .enum_variants
                        .get(name)
                        .and_then(|variants| {
                            variants
                                .iter()
                                .enumerate()
                                .find(|(_, (name, _))| name == &variant.name)
                        })
                    {
                        let types = types.clone();
                        for (index, (pattern, ty)) in fields.iter().zip(types).enumerate() {
                            let field = self.project_value(
                                value,
                                &[
                                    Projection::Variant(tag),
                                    Projection::Index(Some(index as i64)),
                                ],
                                ty,
                            );
                            self.bind_pattern(pattern, &field)?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn reachable_places(
        &mut self,
        mut queue: Vec<Address>,
        desired: &ResolvedType,
        span: Span,
    ) -> Result<Vec<Address>> {
        let mut visited = HashSet::new();
        let mut result = vec![];
        while let Some(address) = queue.pop() {
            if !visited.insert(address.place.clone()) {
                continue;
            }
            if matches!(address.place.root, Root::Parameter(..)) {
                result.push(Address {
                    place: Self::region_place(&address.place, desired),
                    alternatives: vec![],
                    ty: desired.clone(),
                    permits: address.permits,
                });
                continue;
            }
            if matches!(address.place.root, Root::Raw) {
                result.push(Address {
                    ty: desired.clone(),
                    ..address
                });
                continue;
            }
            if SemanticAnalyzer::type_identity(&address.ty)
                == SemanticAnalyzer::type_identity(desired)
            {
                result.push(address.clone());
            }
            let mut projections = vec![];
            match address.ty.unqualified() {
                ResolvedType::Reference { .. } => {
                    let value = self.read(&address, false, span)?;
                    for reference in value.references {
                        if reference.at.is_empty() {
                            let mut permits = address.permits.clone();
                            permits.extend(value.holders.iter().copied());
                            permits.extend(reference.ancestors);
                            queue.push(Address {
                                place: reference.target,
                                alternatives: vec![],
                                ty: reference.pointee.ty,
                                permits,
                            });
                        }
                    }
                }
                ResolvedType::Struct { .. } => {
                    for (field, _) in self.fields(&address.ty) {
                        projections.push(Projection::Field(field));
                    }
                }
                ResolvedType::Enum { name } => {
                    if let Some(variants) = self.analyzer.facts.enum_variants.get(name) {
                        for index in 0..variants.len() {
                            projections.push(Projection::Variant(index));
                        }
                    }
                }
                ResolvedType::Tuple(types) => {
                    for index in 0..types.len() {
                        projections.push(Projection::Index(Some(index as i64)));
                    }
                }
                ResolvedType::Array { size, .. } => {
                    for index in 0..*size {
                        projections.push(Projection::Index(Some(index as i64)));
                    }
                }
                _ => {}
            }
            for projection in projections {
                let mut child = address.clone();
                child.ty = self.child_type(&child.ty, &projection);
                child.place.path.push(projection);
                queue.push(child);
            }
        }
        Ok(result)
    }
    fn instantiate_place(
        &mut self,
        place: &Place,
        args: &[Value],
        span: Span,
    ) -> Result<Vec<Address>> {
        let Root::Parameter(index, path) = &place.root else {
            return Ok(vec![Address {
                place: place.clone(),
                alternatives: vec![],
                ty: ResolvedType::Unknown,
                permits: HashSet::new(),
            }]);
        };
        let Some(argument) = args.get(*index) else {
            return Ok(vec![]);
        };
        let mut values = vec![argument.clone()];
        let mut addresses: Vec<Address> = vec![];
        for projection in path.iter().chain(&place.path) {
            if *projection == Projection::Deref {
                if !addresses.is_empty() {
                    values.clear();
                    for address in &addresses {
                        values.push(self.read(address, false, span)?);
                    }
                }
                addresses.clear();
                for value in values.drain(..) {
                    for reference in &value.references {
                        if reference.at.is_empty() {
                            let mut permits = value.holders.clone();
                            permits.extend(reference.ancestors.iter().copied());
                            let ty = self.child_type(&value.ty, &Projection::Deref);
                            addresses.push(Address {
                                place: reference.target.clone(),
                                alternatives: vec![],
                                ty,
                                permits,
                            });
                        }
                    }
                }
            } else if let Projection::Reachable(region) = projection {
                if addresses.is_empty() {
                    // A by-value aggregate parameter may contain reference
                    // fields with no common projected address. Seed its region
                    // with all retained reference origins, never an empty set.
                    for value in values.drain(..) {
                        for reference in value.references {
                            let mut permits = value.holders.clone();
                            permits.extend(reference.ancestors);
                            addresses.push(Address {
                                place: reference.target,
                                alternatives: vec![],
                                ty: reference.pointee.ty,
                                permits,
                            });
                        }
                    }
                }
                addresses = self.reachable_places(addresses, &region.ty, span)?;
            } else if !addresses.is_empty() {
                for address in &mut addresses {
                    address.ty = self.child_type(&address.ty, projection);
                    address.place.path.push(projection.clone());
                }
            } else {
                values = values
                    .iter()
                    .map(|value| {
                        self.project_value(
                            value,
                            std::slice::from_ref(projection),
                            self.child_type(&value.ty, projection),
                        )
                    })
                    .collect();
            }
        }
        Ok(addresses)
    }
    fn instantiate_value(&mut self, value: &Value, args: &[Value], span: Span) -> Result<Value> {
        let mut result = Value::empty(value.ty.clone());
        for reference in &value.references {
            for address in self.instantiate_place(&reference.target, args, span)? {
                result.references.push(Reference {
                    at: reference.at.clone(),
                    target: address.place,
                    pointee: reference.pointee.clone(),
                    mutable: reference.mutable,
                    ancestors: address.permits,
                });
            }
        }
        Ok(result)
    }
    fn hold_temporary(&mut self, value: &Value, span: Span) -> Result<()> {
        for reference in &value.references {
            if self.validate
                && self.temporary_loans.iter().any(|loan| {
                    loan.target.root == reference.target.root
                        && Self::overlap(&loan.target.path, &reference.target.path)
                        && (loan.mutable || reference.mutable)
                })
            {
                return Err(self.error("non-overlapping temporary mutable references", span));
            }
            self.temporary_loans.push(reference.clone());
        }
        Ok(())
    }
    fn call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
        live: &HashSet<String>,
    ) -> Result<Option<Value>> {
        let name = match callee {
            Expr::Ident(name) => name.name.clone(),
            Expr::Path { segments, .. } => segments
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join("::"),
            _ => String::new(),
        };
        let signature = self.analyzer.symbols.lookup(&name).and_then(|symbol| {
            if let SymbolKind::Function { params, ret, .. } = &symbol.kind {
                Some((params.clone(), ret.clone()))
            } else {
                None
            }
        });
        let (params, ret) = signature.unwrap_or((vec![], ResolvedType::Unknown));
        let old_temporaries = self.temporary_loans.len();
        let mut values = vec![];
        for (index, arg) in args.iter().enumerate() {
            let consume = !matches!(params.get(index), Some(ResolvedType::Reference { .. }));
            let Some(value) = self.expr(arg, consume, live)? else {
                self.temporary_loans.truncate(old_temporaries);
                return Ok(None);
            };
            for reference in &value.references {
                if self.validate {
                    for loan in &self.temporary_loans[old_temporaries..] {
                        if loan.target.root == reference.target.root
                            && Self::overlap(&loan.target.path, &reference.target.path)
                            && (loan.mutable || reference.mutable)
                        {
                            return Err(
                                self.error("non-overlapping mutable argument references", span)
                            );
                        }
                    }
                }
                self.temporary_loans.push(reference.clone());
            }
            values.push(value);
        }
        self.temporary_loans.truncate(old_temporaries);
        for value in &values { self.consume_owned_value(value); }
        let mut result = Value::empty(ret);
        if let Some((enum_name, variant)) = name.rsplit_once("::") {
            if let Some(tag) = self
                .analyzer
                .facts
                .enum_variants
                .get(enum_name)
                .and_then(|variants| variants.iter().position(|(name, _)| name == variant))
            {
                for (index, value) in values.iter().enumerate() {
                    Self::replace_references(
                        &mut result,
                        &[
                            Projection::Variant(tag),
                            Projection::Index(Some(index as i64)),
                        ],
                        value,
                        false,
                    );
                }
                return Ok(Some(result));
            }
        }
        if let Some(summary) = self.summaries.get(&name).cloned() {
            result = self.instantiate_value(
                &Value {
                    owners: HashSet::new(),
                    ty: result.ty,
                    references: summary.returned,
                    holders: HashSet::new(),
                },
                &values,
                span,
            )?;
            for write in summary.writes {
                let value = self.instantiate_value(&write.value, &values, span)?;
                let destinations = self.instantiate_place(&write.target, &values, span)?;
                let definite = write.definite && destinations.len() == 1;
                for address in destinations {
                    self.write(&address, value.clone(), definite, span)?;
                }
            }
        } else if self.validate {
            let mut refs = vec![];
            self.parameter_references(&result.ty, &mut vec![], 0, &mut refs);
            if !refs.is_empty() {
                return Err(self.error("a defined function with proven reference return provenance (use raw pointers for foreign lifetime contracts)",span));
            }
        }
        if result.ty == ResolvedType::NEVER {
            Ok(None)
        } else {
            Ok(Some(result))
        }
    }
}

fn statement_names(statement: &Stmt, names: &mut HashSet<String>) {
    match statement {
        Stmt::Let {
            value: Some(expr), ..
        }
        | Stmt::Return {
            value: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Semi(expr) => expression_names(expr, names),
        _ => {}
    }
}
fn live_names(statements: &[Stmt], after: &HashSet<String>) -> HashSet<String> {
    let mut names = after.clone();
    for statement in statements.iter().rev() {
        match statement {
            Stmt::Let { name, .. } => {
                names.remove(&name.name);
            }
            Stmt::Expr(Expr::Binary {
                left,
                op: BinOp::Assign,
                ..
            })
            | Stmt::Semi(Expr::Binary {
                left,
                op: BinOp::Assign,
                ..
            }) => {
                if let Expr::Ident(name) = left.as_ref() {
                    names.remove(&name.name);
                }
            }
            _ => {}
        }
        statement_names(statement, &mut names);
    }
    names
}
fn block_names(block: &Block, names: &mut HashSet<String>) {
    names.extend(live_names(&block.stmts, &HashSet::new()));
}
fn pattern_names(pattern: &Pattern, names: &mut HashSet<String>) {
    match pattern {
        Pattern::Binding { name, .. } => {
            names.insert(name.name.clone());
        }
        Pattern::Variant { fields, .. }
        | Pattern::Tuple {
            elements: fields, ..
        } => {
            for field in fields {
                pattern_names(field, names);
            }
        }
        Pattern::Struct { fields, .. } => {
            for (_, field) in fields {
                pattern_names(field, names);
            }
        }
        _ => {}
    }
}
fn expression_names(expression: &Expr, names: &mut HashSet<String>) {
    match expression {
        Expr::Ident(name) => {
            names.insert(name.name.clone());
        }
        Expr::Binary {
            left, right, op, ..
        } => {
            if *op != BinOp::Assign || !matches!(left.as_ref(), Expr::Ident(_)) {
                expression_names(left, names);
            }
            expression_names(right, names);
        }
        Expr::Unary { expr, .. }
        | Expr::Deref { expr, .. }
        | Expr::Ref { expr, .. }
        | Expr::Cast { expr, .. }
        | Expr::Field { expr, .. }
        | Expr::Try { expr, .. } => expression_names(expr, names),
        Expr::Call { func, args, .. } => {
            expression_names(func, names);
            for arg in args {
                expression_names(arg, names);
            }
        }
        Expr::MethodCall { expr, args, .. } => {
            expression_names(expr, names);
            for arg in args {
                expression_names(arg, names);
            }
        }
        Expr::Index { expr, index, .. } => {
            expression_names(expr, names);
            expression_names(index, names);
        }
        Expr::Block(body) | Expr::Unsafe { body, .. } | Expr::Loop { body, .. } => {
            block_names(body, names)
        }
        Expr::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expression_names(cond, names);
            block_names(then_block, names);
            if let Some(body) = else_block {
                block_names(body, names);
            }
        }
        Expr::While { cond, body, .. } => {
            expression_names(cond, names);
            block_names(body, names);
        }
        Expr::For {
            var, iter, body, ..
        } => {
            expression_names(iter, names);
            let mut body_names = HashSet::new();
            block_names(body, &mut body_names);
            body_names.remove(&var.name);
            names.extend(body_names);
        }
        Expr::Match { expr, arms, .. } => {
            expression_names(expr, names);
            for arm in arms {
                let mut used = HashSet::new();
                if let Some(guard) = &arm.guard {
                    expression_names(guard, &mut used);
                }
                expression_names(&arm.body, &mut used);
                let mut bound = HashSet::new();
                pattern_names(&arm.pattern, &mut bound);
                used.retain(|name| !bound.contains(name));
                names.extend(used);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, expr) in fields {
                expression_names(expr, names);
            }
        }
        Expr::ArrayRepeat {value,..} => expression_names(value,names),
        Expr::Array { elements, .. } | Expr::Tuple { elements, .. } => {
            for expr in elements {
                expression_names(expr, names);
            }
        }
        Expr::Range { start, end, .. } => {
            for expr in [start, end].into_iter().flatten() {
                expression_names(expr, names);
            }
        }
        Expr::Asm { operands, .. } => {
            for operand in operands {
                if let Some(expr) = &operand.expr {
                    expression_names(expr, names);
                }
            }
        }
        Expr::Closure { body, .. } => expression_names(body, names),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser};
    use std::hash::{Hash, Hasher};
    fn check(source: &str) -> Result<()> {
        let program = Parser::new(Lexer::new(source, 0)).parse_program()?;
        SemanticAnalyzer::new().analyze(&program)
    }
    fn yes(source: &str) {
        assert!(check(source).is_ok(), "{source}\n{:?}", check(source));
    }
    fn no(source: &str) {
        let program = Parser::new(Lexer::new(source, 0))
            .parse_program()
            .expect("negative semantic regression must parse");
        assert!(
            SemanticAnalyzer::new().analyze(&program).is_err(),
            "unexpected acceptance: {source}"
        );
    }
    #[test]
    fn definite_assignment_paths_and_immutable_initialization() {
        no("fn f()->i32 { let x:i32; x }");
        yes("fn f()->i32 { let x:i32; x=7; x }");
        yes("fn f(b:bool)->i32 { let x:i32; if b {x=1;} else {x=2;} x }");
        no("fn f(b:bool)->i32 { let x:i32; if b {x=1;} x }");
        no("fn f(b:bool)->i32 { let mut x:i32; while b {x=1;} x }");
        yes("fn f()->i32 { let x:i32; loop {x=1;break;} x }");
        no("fn f()->i32 { let x:i32; loop {break;x=1;} x }");
        yes("fn f(b:bool)->i32 { let x:i32; if b {return 2;} else {x=1;} x }");
        no("fn f(b:bool) { let x:i32; if b {x=1;} x=2; }");
    }
    #[test]
    fn initialization_tracks_aggregate_places() {
        yes("struct S {x:i32,y:i32} fn f()->i32 {let s:S;s.x=1;s.y=2;s.x+s.y}");
        no("struct S {x:i32,y:i32} fn f()->S {let s:S;s.x=1;s}");
        yes("fn f()->i32 {let a:[i32;2];a[0]=1;a[1]=2;a[0]+a[1]}");
        no("fn f()->i32 {let a:[i32;2];a[0]=1;a[1]}");
    }
    #[test]
    fn loans_follow_declaration_identity_and_last_use() {
        yes("fn f()->i32 {let mut x:i32=1;{let r:&mut i32=&mut x;*r=2;}let r:&mut i32=&mut x;*r=3;x}");
        yes("fn f()->i32 {let mut x:i32=1;let r:&mut i32=&mut x;{let mut x:i32=2;let q:&mut i32=&mut x;*q=3;}*r=4;x}");
        yes("fn f()->i32{let mut x:i32=1;let r:&mut i32=&mut x;{let r:&mut i32=&mut x;*r=2;}x}");
        yes("fn f()->i32{let mut x:i32=1;let mut y:i32=2;let r:&mut i32=&mut x;{let r:&mut i32=&mut y;*r=3;}*r=4;x+y}");
        no("fn f()->i32 {let mut x:i32=1;let r:&i32=&x;x=2;*r}");
        no("fn f()->i32 {let mut x:i32=1;let r:&mut i32=&mut x;let q:&mut i32=r;*q=2;*r}");
        no("fn use2(a:&mut i32,b:&mut i32){} fn f(){let mut x:i32=1;use2(&mut x,&mut x);}");
        yes("fn f()->i32{let mut x:i32=1;let mut r:&mut i32=&mut x;r=&mut x;*r=2;x}");
        yes("fn use1(a:&mut i32){*a=2;} fn f()->i32{let mut x:i32=1;let r:&mut i32=&mut x;use1(r);use1(r);x}");
    }
    #[test]
    fn shadowed_live_after_names_keep_the_outer_identity_only() {
        yes("fn f()->i32{let mut x:i32=1;let r:&mut i32=&mut x;{let mut x:i32=2;let r:&mut i32=&mut x;*r=3;if x!=3{return 1;}{let n:i32=1;}}*r=4;x}");
    }
    #[test]
    fn reference_escape_follows_fields_calls_and_parameter_writes() {
        no("fn f()->&i32 {let x:i32=1;return &x;}");
        no("struct S {r:&i32} fn f()->S {let x:i32=1;S {r:&x}}");
        yes("fn identity(x:&i32)->&i32{x} fn f(x:&i32)->&i32{identity(x)}");
        no("fn identity(x:&i32)->&i32{x} fn f()->&i32{let x:i32=1;identity(&x)}");
        no("fn f(out:&mut &i32){let x:i32=1;*out=&x;}");
        no("fn store(out:&mut &i32,value:&i32){*out=value;} fn f()->i32{let a:i32=1;let mut r:&i32=&a;{let b:i32=2;store(&mut r,&b);}*r}");
        yes("fn f(x:&i32)->&i32 { let r:&i32; {r=x;} r }");
    }
    #[test]
    fn divergence_propagates_through_initializers_and_unreachable_jumps() {
        yes("fn f()->i32{let unused:i32={return 42;};}");
        yes("fn f()->i32{loop{return 42;break;}}");
        yes("fn sink(x:i32){} fn f()->i32{sink({return 42;});}");
        yes("fn f()->i32{let a:[i32;2]=[1,{return 42;}];}");
        yes("fn f()->i32{while true{return 42;}}");
        yes("fn f()->i32{let x:i32=if true {return 1;}else{return 2;};}");
    }
    #[test]
    fn effects_infer_through_wrappers_and_recursion() {
        no("extern \"C\" {fn puts(x:*u8)->i32;} fn f() pure {puts(\"x\");}");
        no("extern \"C\" {fn puts(x:*u8)->i32;} fn g(){puts(\"x\");} fn f() pure {g();}");
        no("extern \"C\" {fn puts(x:*u8)->i32;} fn g(b:bool){if b {f();}puts(\"x\");} fn f() pure {g(false);}");
        yes("fn g(x:i32)->i32{x+1} fn f()->i32 pure {g(1)}");
        no("extern \"mystery\" {fn f();}");
        no("struct S {x:i32} fn f(){S::missing();}");
        no("fn f()->i64{1?}");
    }
    #[test]
    fn assembly_outputs_initialize_after_all_inputs_are_read() {
        yes("fn f()->i32{let x:i32;unsafe{asm!(\"\",out(\"r\") x);}x}");
        no("fn f(){let x:i32;unsafe{asm!(\"\",in(\"r\") x);}}");
        no("fn f(){let mut x:i32;unsafe{asm!(\"\",inout(\"r\") x);}}");
        no("fn f(){let x:i32;unsafe{asm!(\"\",out(\"r\") x,in(\"r\") x);}}");
        no("fn f(){let x:i32=1;unsafe{asm!(\"\",out(\"r\") x);}}");
        no("fn f(){let mut x:i32=1;asm!(\"\",out(\"r\") x);}");
        no("fn f(){let r:&i32;unsafe{asm!(\"\",out(\"r\") r);}}");
    }
    #[test]
    fn aggregate_initializers_hold_temporary_loans() {
        no("fn f(){let mut x:i32=1;let refs:[&mut i32;2]=[&mut x,&mut x];}");
        no("struct S{r:&i32,n:i32}fn f(){let mut x:i32=1;let s:S=S{r:&x,n:{x=2;0}};}");
        yes("fn f(){let mut x:i32=1;let mut y:i32=2;let refs:[&mut i32;2]=[&mut x,&mut y];}");
    }
    #[test]
    fn conditional_reborrows_keep_all_parent_permissions() {
        yes("fn choose(a:&mut i32,b:&mut i32,c:bool)->&mut i32{if c{a}else{b}} fn f()->i32{let mut x:i32=1;let mut y:i32=2;let r:&mut i32=&mut x;let s:&mut i32=&mut y;let q:&mut i32=choose(r,s,false);*q=3;*r=4;*s=5;x+y}");
    }
    #[test]
    fn lazy_boolean_paths_preserve_initialization_and_divergence() {
        yes("fn f(b:bool)->i32{let x:i32;true&&{x=1;true};b&&{return 2;};x}");
        no("fn f(b:bool)->i32{let x:i32;b&&{x=1;true};x}");
        yes("fn f()->i32{true&&{return 1;};}");
        yes("fn f()->i32{loop{false&&{break;};}}");
        yes("fn f()->i32{let unused:i32=if {return 1;}{()}else{()};}");
    }
}
