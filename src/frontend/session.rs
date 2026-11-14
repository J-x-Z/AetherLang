//! Shared compilation input and diagnostics for CLI and editor clients.
use std::{collections::HashMap, path::{Path, PathBuf}};
use serde::Serialize;
use super::{ast::Program, lexer::Lexer, resolve::Resolver, semantic::{SemanticAnalyzer, SemanticFacts}};
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
    pub source_line: String,
    #[serde(skip)]
    pub utf16_column: usize,
    #[serde(skip)]
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
    script_map: Option<SourceMap>,
}

impl Session {
    pub fn parse(path: &Path, source: String, paths: Vec<PathBuf>, overlays: HashMap<PathBuf, String>) -> Result<Self, Diagnostic> {
        let (core, script_map) = if path.extension().is_some_and(|x| x == "ath") {
            let mut parser = crate::script::parser::Parser::new(&source);
            let module = parser.parse().map_err(|e| script_error(path, &source, "E1100", e))?;
            let mut transpiler = crate::script::transpiler::Transpiler::new().with_source_file(&path.to_string_lossy());
            let core = transpiler.transpile(&module).map_err(|e| script_error(path, &source, "E1101", e))?;
            let map = SourceMap::new(source, &core, &transpiler.source_lines);
            (core, Some(map))
        } else { (source, None) };
        let mut resolver = Resolver::new(paths).with_overlays(overlays);
        let program = resolver.resolve(path, core).map_err(|e| Self::make_diagnostic(&resolver, script_map.as_ref(), &e))?;
        let program = super::specialize::specialize(program).map_err(|e| Self::make_diagnostic(&resolver, script_map.as_ref(), &e))?;
        Ok(Self { program, resolver, script_map })
    }
    pub fn check(&self) -> Result<SemanticFacts, Diagnostic> {
        let mut analyzer = SemanticAnalyzer::new();
        analyzer.analyze(&self.program).map_err(|e| self.diagnostic(&e))?;
        Ok(analyzer.facts().clone())
    }
    pub fn diagnostic(&self, error: &Error) -> Diagnostic {
        Self::make_diagnostic(&self.resolver, self.script_map.as_ref(), error)
    }
    fn make_diagnostic(resolver: &Resolver, map: Option<&SourceMap>, error: &Error) -> Diagnostic {
        let mut diagnostic = Diagnostic::message(error_code(error), error.to_string());
        diagnostic.location = error.span().and_then(|span| location(resolver, map, span));
        diagnostic
    }
    pub fn location(&self, span: Span) -> Option<Location> { location(&self.resolver, self.script_map.as_ref(), span) }
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

fn location(resolver: &Resolver, map: Option<&SourceMap>, span: Span) -> Option<Location> {
    let file = resolver.sources.get(span.file_id)?;
    let (text, start, end) = if span.file_id == 0 {
        if let Some(map) = map {
            let (start, end) = map.remap(span.start, span.end);
            (map.original.as_str(), start, end)
        } else { (file.text.as_str(), span.start, span.end) }
    } else { (file.text.as_str(), span.start, span.end) };
    Some(text_location(&file.path, text, start, end))
}

pub fn text_location(path: &Path, text: &str, start: usize, end: usize) -> Location {
    let boundary = |offset: usize| { let mut n = offset.min(text.len()); while !text.is_char_boundary(n) { n -= 1; } n };
    let start = boundary(start); let end = boundary(end).max(start);
    let before = &text[..start]; let last = before.rsplit('\n').next().unwrap_or("");
    let before_end = &text[..end]; let last_end = before_end.rsplit('\n').next().unwrap_or("");
    Location { file: path.to_string_lossy().into(), start, end,
        line: before.bytes().filter(|b| *b == b'\n').count() + 1, column: last.chars().count() + 1,
        end_line: before_end.bytes().filter(|b| *b == b'\n').count() + 1, end_column: last_end.chars().count() + 1,
        source_line: format!("{}{}", last, text[start..].split('\n').next().unwrap_or("")),
        utf16_column: last.encode_utf16().count(), end_utf16_column: last_end.encode_utf16().count() }
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
        let hits: Vec<_> = self.tokens.iter().filter(|(a,b,_,_)| *a < end.max(start+1) && *b > start).collect();
        if let (Some(first), Some(last)) = (hits.first(), hits.last()) { return (first.2, last.3.max(first.2)); }
        if let Some((_,_,a,b)) = self.lines.iter().find(|(a,b,_,_)| *a <= start && start < *b) { return (*a,*b); }
        (0,0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
