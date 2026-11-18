//! Shared compilation input and diagnostics for CLI and editor clients.
use std::{cell::OnceCell, collections::HashMap, path::{Path, PathBuf}, sync::Arc};
use serde::Serialize;
use super::{ast::{Ident, Item, Program}, lexer::Lexer, resolve::{ModuleSearch, Resolver}, semantic::{SemanticAnalyzer, SemanticFacts}};
use crate::utils::{Error, Span};

#[derive(Debug, Clone, Serialize)]
pub struct Location {
    pub file: String,
    pub start: usize,
    pub end: usize,
    /// Human positions are one based; byte offsets are UTF-8 and end-exclusive.
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    #[serde(skip)]
    pub source_line: Arc<str>,
    /// LSP character offsets are zero based UTF-16 code units.
    pub utf16_column: usize,
    pub end_utf16_column: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: &'static str,
    pub message: String,
    pub location: Option<Location>,
}
impl Diagnostic {
    pub fn message(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), severity: "error", message: message.into(), location: None }
    }
    pub fn render(&self) -> String {
        if let Some(loc) = &self.location {
            format!("{}:{}:{}: {} [{}]\n  {}\n  {}^", loc.file, loc.line, loc.column,
                self.message, self.code, loc.source_line, " ".repeat(loc.column.saturating_sub(1)))
        } else { format!("{} [{}]", self.message, self.code) }
    }
}
impl From<String> for Diagnostic { fn from(message: String) -> Self { Self::message("E9000", message) } }
impl From<&str> for Diagnostic { fn from(message: &str) -> Self { Self::message("E9000", message) } }

pub struct Session {
    pub program: Program,
    pub resolver: Resolver,
    // Uninstantiated templates disappear from the executable AST. Keep only
    // their names and source spans for project navigation.
    pub(super) generic_enum_names: Vec<(Ident, Vec<Ident>)>,
    script_map: Option<SourceMap>,
}

impl Session {
    pub fn parse(path: &Path, source: String, search: ModuleSearch, overlays: HashMap<PathBuf, String>) -> Result<Self, Diagnostic> {
        Self::parse_for_target(path, source, search, overlays, "native")
    }
    pub fn parse_for_target(path: &Path, source: String, search: ModuleSearch, overlays: HashMap<PathBuf, String>, target: &str) -> Result<Self, Diagnostic> {
        let width = crate::types::PointerWidth::for_target(target).map_err(Diagnostic::from)?;
        let mut overlays=overlays;
        let (core, script_map) = if path.extension().is_some_and(|x| x == "ath") {
            let mut parser = crate::script::parser::Parser::new(&source);
            let module = parser.parse().map_err(|e| script_error(path, &source, "E1100", e))?;
            let mut transpiler = crate::script::transpiler::Transpiler::new().with_source_file(&path.to_string_lossy());
            let (prelude,requested)=crate::script::transpiler::Transpiler::import_prelude(&module);
            if !prelude.is_empty() {
                let mut imported=Resolver::new(search.clone()).with_overlays(overlays.clone());
                let imported_program=imported.resolve(path,prelude).map_err(|e|Self::make_diagnostic(&imported,None,&e))?;
                transpiler=transpiler.with_core_imports(&imported_program,&imported.root_bindings,&requested,width).map_err(|e|script_error(path,&source,"E1101",e))?;
                // Use exactly the declaration snapshots inspected above during
                // final resolution, including editor overlays.
                for file in imported.sources.into_iter().skip(1) {overlays.insert(file.path,file.text);}
            }
            let core = transpiler.transpile(&module).map_err(|e| script_error(path, &source, "E1101", e))?;
            let map = SourceMap::new(source, &core, &transpiler.source_lines);
            (core, Some(map))
        } else { (source, None) };
        let mut resolver = Resolver::new(search).with_overlays(overlays);
        let program = resolver.resolve(path, core).map_err(|e| Self::make_diagnostic(&resolver, script_map.as_ref(), &e))?;
        let generic_enum_names = program.items.iter().filter_map(|item| match item {
            Item::Enum(e) if !e.generic_params.is_empty() || !e.type_params.is_empty() =>
                Some((e.name.clone(), e.variants.iter().map(|v| v.name.clone()).collect())),
            _ => None,
        }).collect();
        let program = super::specialize::specialize_for_width(program, width).map_err(|e| Self::make_diagnostic(&resolver, script_map.as_ref(), &e))?;
        Ok(Self { program, resolver, script_map, generic_enum_names })
    }
    pub fn check(&self, target: &str, llvm_backend: bool) -> Result<SemanticFacts, Diagnostic> {
        let mut analyzer = SemanticAnalyzer::for_target(target).map_err(Diagnostic::from)?;
        if llvm_backend { analyzer.select_llvm_abi(target); }
        analyzer.analyze(&self.program).map_err(|e| self.diagnostic(&e))?;
        Ok(analyzer.into_facts())
    }
    pub fn diagnostic(&self, error: &Error) -> Diagnostic {
        Self::make_diagnostic(&self.resolver, self.script_map.as_ref(), error)
    }
    fn make_diagnostic(resolver: &Resolver, map: Option<&SourceMap>, error: &Error) -> Diagnostic {
        let mut diagnostic = Diagnostic::message(error_code(error), error.to_string());
        diagnostic.location = error.span().and_then(|span| location(resolver, map, span, None));
        diagnostic
    }
    pub fn location(&self, span: Span) -> Option<Location> { location(&self.resolver, self.script_map.as_ref(), span, None) }
    pub(super) fn indexed_location(&self, span: Span, lines: &LineIndex) -> Option<Location> {
        location(&self.resolver, self.script_map.as_ref(), span, Some(lines))
    }
    pub fn source(&self, file_id: usize) -> Option<&str> {
        if file_id == 0 { if let Some(map) = &self.script_map { return Some(&map.original); } }
        self.resolver.sources.get(file_id).map(|s| s.text.as_str())
    }
}

fn error_code(error: &Error) -> &'static str {
    match error {
        Error::Semantic { code, .. } => code,
        Error::UnexpectedToken { .. } | Error::Expected(_, _) | Error::ExpectedIdent { .. }
        | Error::ExpectedType { .. } | Error::ExpectedExpr { .. } | Error::ExpectedPattern { .. }
        | Error::ExpectedArraySize { .. } | Error::InvalidOperator { .. } => "E1000",
        Error::UndefinedVariable { .. } => "E2001", Error::UndefinedType { .. } => "E2002",
        Error::DuplicateDefinition { .. } => "E2003", Error::TypeMismatch { .. } => "E2100",
        Error::ArgCountMismatch { .. } | Error::NotCallable { .. } => "E2101",
        Error::UnknownField { .. } | Error::NotAStruct { .. } => "E2102",
        Error::CannotDeref { .. } | Error::NotIndexable { .. } => "E2103",
        Error::EffectViolation { .. } => "E2200",
        Error::UseAfterMove { .. } | Error::CannotMoveWhileBorrowed { .. } | Error::CannotMutBorrowWhileBorrowed { .. }
        | Error::CannotBorrowWhileMutBorrowed { .. } | Error::CannotMutBorrowTwice { .. }
        | Error::CannotMoveOutOfBorrow { .. } | Error::CannotBorrowMutably { .. } => "E2300",
        Error::Io(_) => "E9001", Error::ModuleError(_) => "E2000", Error::Llvm(_) | Error::CodeGen(_) => "E3000",
    }
}

fn location(resolver: &Resolver, map: Option<&SourceMap>, span: Span, lines: Option<&LineIndex>) -> Option<Location> {
    let file = resolver.sources.get(span.file_id)?;
    let (text, start, end) = if span.file_id == 0 {
        if let Some(map) = map {
            let (start, end) = map.remap(span.start, span.end);
            (map.original.as_str(), start, end)
        } else { (file.text.as_str(), span.start, span.end) }
    } else { (file.text.as_str(), span.start, span.end) };
    Some(match lines {
        Some(index) => index.location(&file.path, text, start, end),
        None => text_location(&file.path, text, start, end),
    })
}

/// Immutable line offsets and lazily shared diagnostic excerpts for one snapshot.
pub(super) struct LineIndex {
    lines: Vec<IndexedLine>,
    ascii: bool,
}
struct IndexedLine {
    start: usize,
    excerpt: OnceCell<Arc<str>>,
}

impl LineIndex {
    pub(super) fn new(text: &str) -> Self {
        let entry = |start| IndexedLine { start, excerpt: OnceCell::new() };
        let mut lines = vec![entry(0)];
        lines.extend(text.match_indices('\n').map(|(offset, _)| entry(offset + 1)));
        Self { lines, ascii: text.is_ascii() }
    }

    pub(super) fn location(&self, path: &Path, text: &str, start: usize, end: usize) -> Location {
        let start = text.floor_char_boundary(start.min(text.len()));
        let end = text.floor_char_boundary(end.min(text.len())).max(start);
        let position = |offset| {
            let line = self.lines.partition_point(|line| line.start <= offset) - 1;
            let prefix = &text[self.lines[line].start..offset];
            let (characters, utf16) = if self.ascii || prefix.is_ascii() {
                (prefix.len(), prefix.len())
            } else {
                (prefix.chars().count(), prefix.encode_utf16().count())
            };
            (line, characters + 1, utf16)
        };
        let (line, column, utf16_column) = position(start);
        let (end_line, end_column, end_utf16_column) = position(end);
        let source_line = self.lines[line].excerpt.get_or_init(|| {
            let end = self.lines.get(line + 1).map_or(text.len(), |next| next.start - 1);
            Arc::from(&text[self.lines[line].start..end])
        }).clone();
        Location { file: path.to_string_lossy().into(), start, end,
            line: line + 1, column, end_line: end_line + 1, end_column,
            source_line, utf16_column, end_utf16_column }
    }
}

pub fn text_location(path: &Path, text: &str, start: usize, end: usize) -> Location {
    LineIndex::new(text).location(path, text, start, end)
}

fn script_error(path: &Path, source: &str, code: &str, message: String) -> Diagnostic {
    let mut diagnostic = Diagnostic::message(code, &message);
    let numbers = message.strip_prefix("line ").unwrap_or("").split(':').collect::<Vec<_>>();
    let line = numbers.first().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1).max(1);
    let column = numbers.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(1).max(1);
    let line_offset = source.split_inclusive('\n').take(line - 1).map(str::len).sum::<usize>().min(source.len());
    let within = source[line_offset..].split('\n').next().unwrap_or("");
    let col_offset = within.char_indices().nth(column - 1).map(|x| x.0).unwrap_or(within.len());
    let start = line_offset + col_offset;
    diagnostic.location = Some(text_location(path, source, start, start));
    diagnostic
}

/// Match tokens in each generated line to its original Script line. Generated
/// declarations and punctuation map to the containing source statement; matching
/// user tokens retain exact UTF-8 ranges, including repeated names and strings.
struct SourceMap {
    original: String,
    lines: Vec<(usize, usize, usize, usize)>, // generated range, source line range
    tokens: Vec<(usize, usize, usize, usize)>,
}
impl SourceMap {
    fn new(original: String, generated: &str, source_lines: &[usize]) -> Self {
        let mut starts = vec![0];
        for (offset, b) in original.bytes().enumerate() { if b == b'\n' { starts.push(offset + 1); } }
        let mut map = Self { original, lines: vec![], tokens: vec![] };
        let mut generated_offset = 0;
        for (index, line) in generated.split_inclusive('\n').enumerate() {
            let source_line = source_lines.get(index).copied().unwrap_or(1).saturating_sub(1).min(starts.len()-1);
            let start = starts[source_line];
            let end = starts.get(source_line + 1).copied().unwrap_or(map.original.len());
            map.lines.push((generated_offset, generated_offset + line.len(), start, end));
            let src = &map.original[start..end];
            let a: Vec<_> = Lexer::new(line, 0).tokenize().into_iter().filter(|t| t.span.end > t.span.start).collect();
            let b: Vec<_> = Lexer::new(src, 0).tokenize().into_iter().filter(|t| t.span.end > t.span.start).collect();
            let mut lengths = vec![vec![0; b.len()+1]; a.len()+1];
            for i in (0..a.len()).rev() { for j in (0..b.len()).rev() {
                lengths[i][j] = if line[a[i].span.start..a[i].span.end] == src[b[j].span.start..b[j].span.end] {
                    lengths[i+1][j+1]+1
                } else { lengths[i+1][j].max(lengths[i][j+1]) };
            } }
            let (mut i, mut j) = (0, 0);
            while i<a.len() && j<b.len() {
                if line[a[i].span.start..a[i].span.end] == src[b[j].span.start..b[j].span.end] {
                    map.tokens.push((generated_offset+a[i].span.start, generated_offset+a[i].span.end, start+b[j].span.start, start+b[j].span.end));
                    i+=1; j+=1;
                } else if lengths[i+1][j] >= lengths[i][j+1] { i+=1; } else { j+=1; }
            }
            generated_offset += line.len();
        }
        map
    }
    fn remap(&self, start: usize, end: usize) -> (usize, usize) {
        // Tokens and lines are disjoint and ordered by their generated offsets.
        let first = self.tokens.partition_point(|&(_, finish, _, _)| finish <= start);
        let end = end.max(start.saturating_add(1));
        let past = self.tokens.partition_point(|&(begin, _, _, _)| begin < end);
        if first < past {
            let source_start = self.tokens[first].2;
            return (source_start, self.tokens[past - 1].3.max(source_start));
        }
        let line = self.lines.partition_point(|&(_, finish, _, _)| finish <= start);
        match self.lines.get(line) {
            Some(&(begin, _, a, b)) if begin <= start => (a, b),
            _ => (0, 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qualified_references_cover_only_the_referenced_name() {
        use crate::frontend::symbols::SymbolIndex;
        let directory = std::env::temp_dir().join("aether-qualified-symbol-overlay");
        let root = directory.join("main.aeth");
        let library = directory.join("library.aeth");
        let source = "mod library;\nfn main()->i32{let text:*u8=\"😀é\";return library::answer();}";
        let session = Session::parse(&root, source.into(), ModuleSearch::default(),
            HashMap::from([(library, "pub fn answer()->i32{7}".into())])).unwrap();
        let facts = session.check("native", false).unwrap();
        let index = SymbolIndex::new(&session, Some(&facts));
        let symbol = index.symbols.iter().find(|s| s.name == "answer").unwrap();
        let loc = &index.references.iter().find(|r| r.symbol == symbol.id).unwrap().location;
        let start = source.find("answer").unwrap();
        let line_prefix = source[..start].rsplit('\n').next().unwrap();
        let serialized = serde_json::to_value(loc).unwrap();
        assert_eq!((loc.start, loc.end), (start, start + "answer".len()));
        assert_eq!(serialized["column"], line_prefix.chars().count() + 1);
        assert_eq!(serialized["utf16_column"], line_prefix.encode_utf16().count());
        assert!(index.at(&loc.file, source.find("library::").unwrap()).is_none());
        assert!(index.at(&loc.file, start - 1).is_none());
        assert_eq!(index.at(&loc.file, start).unwrap().name, "answer");
    }

    #[test]
    fn script_lookup_preserves_token_overlaps_and_line_fallbacks() {
        let map = SourceMap { original: String::new(),
            lines: vec![(0, 8, 10, 20), (8, 15, 25, 30)],
            tokens: vec![(1, 3, 12, 14), (4, 6, 15, 17), (11, 13, 26, 28)] };
        for start in 0..=16 {
            for end in start..=16 {
                let hits: Vec<_> = map.tokens.iter()
                    .filter(|&&(a, b, _, _)| a < end.max(start + 1) && b > start).collect();
                let expected = match (hits.first(), hits.last()) {
                    (Some(first), Some(last)) => (first.2, last.3.max(first.2)),
                    _ => map.lines.iter().find(|&&(a, b, _, _)| a <= start && start < b)
                        .map_or((0, 0), |&(_, _, a, b)| (a, b)),
                };
                assert_eq!(map.remap(start, end), expected);
            }
        }
    }

    #[test]
    fn many_symbols_on_one_line_share_the_complete_diagnostic_excerpt() {
        let text = "fn square(x: i32)->i32{x*x}";
        let index = LineIndex::new(text);
        let first = index.location(Path::new("main.aeth"), text, 3, 9);
        let second = index.location(Path::new("main.aeth"), text, 10, 11);
        assert!(Arc::ptr_eq(&first.source_line, &second.source_line));
        let diagnostic = Diagnostic { code: "TEST".into(), severity: "error",
            message: "example".into(), location: Some(second) };
        assert!(diagnostic.render().contains(text));
    }

    #[test]
    fn indexed_locations_match_unicode_and_multiline_prefixes() {
        let path = Path::new("sample.aeth");
        for text in ["", "plain", "é😀\r\n\nα尾\n", "final\nline"] {
            let index = LineIndex::new(text);
            for start in 0..=text.len() + 1 {
                for end in 0..=text.len() + 1 {
                    let loc = index.location(path, text, start, end);
                    let first = text.floor_char_boundary(start.min(text.len()));
                    let last = text.floor_char_boundary(end.min(text.len())).max(first);
                    let reference = |byte| {
                        let lines: Vec<_> = text[..byte].split('\n').collect();
                        let prefix = lines.last().unwrap();
                        (lines.len(), prefix.chars().count() + 1, prefix.encode_utf16().count())
                    };
                    assert_eq!((loc.start, loc.end), (first, last));
                    assert_eq!((loc.line, loc.column, loc.utf16_column), reference(first));
                    assert_eq!((loc.end_line, loc.end_column, loc.end_utf16_column), reference(last));
                    assert_eq!(&*loc.source_line, text.split('\n').nth(loc.line - 1).unwrap());
                }
            }
        }
    }

    #[test]
    fn typed_script_imports_use_editor_overlays_in_both_passes() {
        // No on-disk module: metadata inference and final Core checking must
        // both use the same unsaved editor snapshot.
        let directory = std::env::temp_dir().join("aether-script-import-overlay");
        let root = directory.join("main.ath");
        let module = directory.join("values.aeth");
        let source = "from values import supplied\ndef main():\n    if supplied() + 1 != 8:\n        return 1\n    return 0\n";
        let snapshot = "pub fn supplied()->i64{7}";
        let session = Session::parse(&root, source.into(), ModuleSearch::default(),
            HashMap::from([(module.clone(), snapshot.into())])).unwrap();
        session.check("native", false).unwrap();
        assert_eq!(session.resolver.sources.len(), 2);
        assert_eq!(session.resolver.sources[1].text, snapshot);
        let session = Session::parse(&root, source.into(), ModuleSearch::default(),
            HashMap::from([(module, "pub fn supplied()->bool{false}".into())])).unwrap();
        let diagnostic = session.check("native", false).err().unwrap();
        assert_eq!(diagnostic.code, "E2100");
        assert_eq!(diagnostic.location.unwrap().line, 3);
    }

    #[test]
    fn script_mapping_preserves_unicode_and_original_error_line() {
        let original = "def main():\n    print(\"你好\")\n    return True\n".to_string();
        let mut parser = crate::script::parser::Parser::new(&original);
        let module = parser.parse().unwrap();
        let mut transpiler = crate::script::transpiler::Transpiler::new();
        let core = transpiler.transpile(&module).unwrap();
        let map = SourceMap::new(original.clone(), &core, &transpiler.source_lines);
        let start = core.find("true").unwrap();
        let (a, _) = map.remap(start, start+4);
        assert_eq!(text_location(Path::new("test.ath"), &original, a, a).line, 3);
        let start = core.find("你好").unwrap();
        let (a,b) = map.remap(start, start+6);
        assert_eq!(&original[a..b], "\"你好\"");
    }
}
