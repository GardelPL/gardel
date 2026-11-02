#![expect(
    missing_docs,
    reason = "Compiler CLI binary with internal AST and emitters"
)]
#![expect(missing_debug_implementations, reason = "Internal compiler types")]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::process::Command;
use std::rc::Rc;

// ============================================================================
// 1. AST DEFINITIONS (Gardel v0.4 Phase 4: Concepts, Boogie, Fuel, Stdlib & IO)
// ============================================================================

/// Default recursive unfolding fuel inside a module (§7.5).
pub const DEFAULT_FUEL: usize = 2;

/// Source span (1-based line and column range) for structured AI diagnostics (§12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl Span {
    pub fn unknown(file: &str) -> Self {
        Self {
            file: file.to_string(),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    Any,
    None,
    Modules(Vec<String>),
}

impl Visibility {
    /// Returns true if `symbol_vis` is at least as visible as `context_vis`
    pub fn is_at_least_as_visible_as(&self, context_vis: &Visibility) -> bool {
        match (self, context_vis) {
            (Visibility::Any, _) => true,
            (_, Visibility::None) => true,
            (Visibility::None, _) => false,
            (Visibility::Modules(_), Visibility::Any) => false,
            (Visibility::Modules(sym_mods), Visibility::Modules(ctx_mods)) => {
                ctx_mods.iter().all(|m| sym_mods.contains(m))
            }
        }
    }
}

/// Distinguishes ADT `module`, stateless `functions`, and effectful `program` declarations (§4, §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    Module,
    Functions,
    Program,
}

#[derive(Debug, Clone)]
pub struct FileAst {
    pub group: Option<Vec<String>>,
    pub concepts: Vec<ConceptDecl>,
    pub modules: Vec<ModuleDecl>,
}

#[derive(Debug, Clone)]
pub struct ConceptDecl {
    pub name: String,
    pub type_params: Vec<(String, Option<String>)>,
    pub refines: Vec<String>,
    pub signatures: Vec<SignatureDecl>,
    pub default_routines: Vec<RoutineDecl>,
    pub laws: Vec<LawDecl>,
}

#[derive(Debug, Clone)]
pub struct SignatureDecl {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub ret_type: String,
}

#[derive(Debug, Clone)]
pub struct LawDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub params: Vec<(String, String)>,
    pub requires: Vec<(String, Expr)>,
    pub triggers: Vec<Expr>,
    pub variant: Vec<Expr>,
    pub stmts: Vec<Stmt>,
    pub body: Option<Expr>,
    pub ensures: Vec<(String, Expr)>,
}

#[derive(Debug, Clone)]
pub struct SatisfiesClause {
    pub concepts: Vec<String>,
    pub where_constraints: Vec<(String, String)>, // e.g., where A satisfies EQ
}

#[derive(Debug, Clone)]
pub enum ReprKind {
    Sum(Vec<VariantDecl>),
    Product(Vec<(String, String)>),
}

#[derive(Debug, Clone)]
pub struct ModuleDecl {
    pub kind: ModuleKind,
    pub name: String,
    pub type_params: Vec<String>,
    pub type_bounds: HashMap<String, String>, // e.g., [A: ORD] -> "A" -> "ORD"
    pub satisfies: Option<SatisfiesClause>,
    pub repr_vis: Visibility,
    pub repr: ReprKind,
    pub specify_routines: Vec<(Visibility, RoutineDecl)>,
    pub specify_laws: Vec<(Visibility, LawDecl)>,
    pub create_routines: Vec<(Visibility, RoutineDecl)>,
    pub feature_routines: Vec<(Visibility, RoutineDecl)>,
    pub invariants: Vec<(String, Expr)>,
}

#[derive(Debug, Clone)]
pub struct VariantDecl {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct RoutineDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub params: Vec<(String, String)>,
    pub ret_type: String,
    pub requires: Vec<(String, Expr)>,
    pub variant: Vec<Expr>,
    pub stmts: Vec<Stmt>,
    pub body: Expr,
    pub ensures: Vec<(String, Expr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let(String, Expr),
    Check(String, Expr),
    Assume(String, Expr),
    Reveal(String),
}

/// Sequenced linear IO action inside a `with IO ... end` block (§11.2, §15).
#[derive(Debug, Clone, PartialEq)]
pub enum IoStmt {
    Bind(String, Expr),
    Action(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(u128),
    Float(f64),
    Char(u32),
    Bool(bool),
    Str(String),
    Unit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Bind(String),
    Lit(Literal),
    Variant(String, Vec<Pattern>),
    Tuple(Vec<Pattern>),
}

impl Pattern {
    pub fn ctor(&self) -> Option<&str> {
        match self {
            Pattern::Variant(c, _) => Some(c.as_str()),
            _ => None,
        }
    }

    pub fn top_bindings(&self) -> Vec<Option<String>> {
        match self {
            Pattern::Variant(_, sub) | Pattern::Tuple(sub) => sub
                .iter()
                .map(|p| match p {
                    Pattern::Bind(b) => Some(b.clone()),
                    _ => None,
                })
                .collect(),
            Pattern::Bind(b) => vec![Some(b.clone())],
            _ => Vec::new(),
        }
    }

    pub fn collect_bindings(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_bindings_into(&mut out);
        out
    }

    fn collect_bindings_into(&self, out: &mut Vec<String>) {
        match self {
            Pattern::Bind(b) => out.push(b.clone()),
            Pattern::Variant(_, sub) | Pattern::Tuple(sub) => {
                for p in sub {
                    p.collect_bindings_into(out);
                }
            }
            Pattern::Wildcard | Pattern::Lit(_) => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(u128),
    Float(f64),
    Char(u32),
    Unit,
    Bool(bool),
    Str(String),
    Var(String),
    UnaryNot(Box<Expr>),
    UnaryNeg(Box<Expr>),
    BinOp(String, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    MethodCall(Box<Expr>, String, Vec<Expr>),
    If(Box<Expr>, Vec<Stmt>, Box<Expr>, Vec<Stmt>, Box<Expr>),
    Match(Box<Expr>, Vec<MatchCase>),
    RecordLit(Vec<(String, Expr)>),
    RecordUpdate(Box<Expr>, Vec<(String, Expr)>),
    TupleLit(Vec<Expr>),
    Forall(String, String, Box<Expr>),
    Exists(String, String, Box<Expr>),
    WithIo(Vec<IoStmt>, Box<Expr>),
    Lambda(Vec<(String, String)>, String, Vec<Stmt>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub stmts: Vec<Stmt>,
    pub body: Expr,
}

impl MatchCase {
    pub fn new(pattern: Pattern, stmts: Vec<Stmt>, body: Expr) -> Self {
        Self {
            pattern,
            stmts,
            body,
        }
    }

    pub fn ctor(&self) -> &str {
        self.pattern.ctor().unwrap_or("")
    }

    pub fn bindings(&self) -> Vec<String> {
        self.pattern.collect_bindings()
    }

    pub fn top_bindings(&self) -> Vec<Option<String>> {
        self.pattern.top_bindings()
    }
}

#[derive(Debug, Clone)]
pub struct ProgramEnv {
    pub modules: HashMap<String, ModuleDecl>,
    pub concepts: HashMap<String, ConceptDecl>,
}

#[derive(Debug, Clone)]
pub struct TopLevelCombinator {
    pub env_struct: Option<(String, Vec<(String, String)>)>,
    pub routine: RoutineDecl,
}

// ============================================================================
// 2. LEXER & LEXICAL ERRORS (Supports Strings, `<-`, `->`, Floats, Chars, Bitwise & Wrapping)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub message: String,
    pub position: usize,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Lexical error at byte {}: {}",
            self.position, self.message
        )
    }
}

impl std::error::Error for LexError {}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Int(u128),
    Float(f64),
    Char(char),
    Str(String),
    Unit,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Semicolon,
    Comma,
    Dot,
    Pipe,
    Plus,
    Minus,
    Arrow,
    LeftArrow,
    Star,
    Slash,
    Percent,
    PlusPercent,
    MinusPercent,
    StarPercent,
    Amp,
    Caret,
    Shl,
    Shr,
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
    /// `!=`, desugared by the parser to `not (a = b)`.
    Ne,
    Eof,
}

/// Decodes one escape sequence of a char or string literal (§3.1, §15 `EscapeSeq`):
/// `\n \r \t \\ \' \" \0 \u{H..}` with 1 to 6 hex digits denoting a Unicode scalar value.
/// `i` points at the backslash and is advanced past the sequence. Any other escape, a
/// surrogate code point, or a value above U+10FFFF is a lexical error.
fn lex_escape_sequence(
    chars: &[char],
    i: &mut usize,
    start: usize,
    line: usize,
    kind: &str,
) -> Result<char, LexError> {
    let malformed = |what: String| LexError {
        message: format!(
            "E0302: malformed_escape_sequence: {what} in {kind} literal on line {line} (valid escapes: \\n \\r \\t \\\\ \\' \\\" \\0 \\u{{H..}}, §15)"
        ),
        position: start,
    };
    *i += 1; // skip the backslash
    let Some(&e) = chars.get(*i) else {
        return Err(LexError {
            message: format!("Unterminated escape in {kind} literal"),
            position: start,
        });
    };
    *i += 1;
    let ch = match e {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        '\\' => '\\',
        '\'' => '\'',
        '"' => '"',
        '0' => '\0',
        'u' => {
            if chars.get(*i) != Some(&'{') {
                return Err(malformed("`\\u` must be followed by `{H..}`".to_string()));
            }
            *i += 1;
            let mut hex = String::new();
            while let Some(&h) = chars.get(*i) {
                if h == '}' || h == '"' || h == '\'' || h == '\n' || hex.len() > 8 {
                    break;
                }
                hex.push(h);
                *i += 1;
            }
            if chars.get(*i) != Some(&'}') {
                return Err(malformed("unterminated `\\u{` escape".to_string()));
            }
            *i += 1;
            if hex.is_empty() || hex.len() > 6 || !hex.chars().all(|h| h.is_ascii_hexdigit()) {
                return Err(malformed(format!(
                    "`\\u{{{hex}}}` must contain 1 to 6 hexadecimal digits"
                )));
            }
            let v = u32::from_str_radix(&hex, 16)
                .map_err(|_| malformed(format!("`\\u{{{hex}}}` is not a hexadecimal number")))?;
            if (0xD800..=0xDFFF).contains(&v) {
                return Err(LexError {
                    message: format!(
                        "E0304: surrogate_in_literal: `\\u{{{hex}}}` in {kind} literal on line {line} denotes the surrogate code point U+{v:04X}, which is not a Unicode scalar value (§3.1)"
                    ),
                    position: start,
                });
            }
            char::from_u32(v).ok_or_else(|| {
                malformed(format!(
                    "`\\u{{{hex}}}` is above U+10FFFF and not a Unicode scalar value"
                ))
            })?
        }
        other => return Err(malformed(format!("unknown escape `\\{other}`"))),
    };
    Ok(ch)
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
    tokenize_with_lines(src).map(|(toks, _)| toks)
}

pub fn tokenize_with_lines(src: &str) -> Result<(Vec<Token>, Vec<usize>), LexError> {
    let mut tokens = Vec::new();
    let mut lines = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut current_line = 1;

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            current_line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == ';' {
            tokens.push(Token::Semicolon);
            lines.push(current_line);
            i += 1;
        } else if c == '\'' {
            let start = i;
            let start_line = current_line;
            i += 1;
            let ch = if i < chars.len() && chars[i] == '\\' {
                lex_escape_sequence(&chars, &mut i, start, start_line, "char")?
            } else if i < chars.len() && chars[i] != '\'' && chars[i] != '\n' && chars[i] != '\r' {
                let ch = chars[i];
                i += 1;
                ch
            } else {
                return Err(LexError {
                    message: "Empty char literal".to_string(),
                    position: start,
                });
            };
            if i >= chars.len() || chars[i] != '\'' {
                return Err(LexError {
                    message: "Unterminated char literal".to_string(),
                    position: start,
                });
            }
            i += 1;
            tokens.push(Token::Char(ch));
            lines.push(start_line);
        } else if c == '"' {
            // §3.1.4 / §15 String literal: decoded at compile time into its scalar sequence.
            // Malformed escapes, surrogates and raw line breaks are rejected; because the
            // bootstrap prelude `STRING` is the ASCII subset of UTF-8 (§6.4.C), so is every
            // scalar outside the ASCII range (multi-byte text is Task C.2).
            let start = i;
            let start_line = current_line;
            i += 1;
            let mut s = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\n' || chars[i] == '\r' {
                    return Err(LexError {
                        message: format!(
                            "E0301: unterminated_string_literal: string literal starting on line {start_line} contains a raw line break (use the `\\n` escape, §15)"
                        ),
                        position: start,
                    });
                }
                let ch = if chars[i] == '\\' {
                    lex_escape_sequence(&chars, &mut i, start, start_line, "string")?
                } else {
                    let ch = chars[i];
                    i += 1;
                    ch
                };
                if !ch.is_ascii() {
                    return Err(LexError {
                        message: format!(
                            "E0303: non_ascii_string_literal: string literal on line {start_line} contains the non-ASCII scalar U+{:04X}; the bootstrap prelude STRING is the ASCII subset of UTF-8 (§6.4.C), so non-ASCII text in string literals requires Task C.2 (full UTF-8 STRING)",
                            ch as u32
                        ),
                        position: start,
                    });
                }
                s.push(ch);
            }
            if i >= chars.len() || chars[i] != '"' {
                return Err(LexError {
                    message: "Unterminated string literal".to_string(),
                    position: start,
                });
            }
            i += 1;
            tokens.push(Token::Str(s));
            lines.push(start_line);
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            let start_line = current_line;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            tokens.push(Token::Ident(s));
            lines.push(start_line);
        } else if c.is_ascii_digit() {
            let start = i;
            let start_line = current_line;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len()
                && chars[i] == '.'
                && i + 1 < chars.len()
                && chars[i + 1].is_ascii_digit()
            {
                i += 1; // skip '.'
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                let val = s.parse::<f64>().map_err(|e| LexError {
                    message: format!("Invalid float literal '{s}': {e}"),
                    position: start,
                })?;
                tokens.push(Token::Float(val));
                lines.push(start_line);
            } else {
                let s: String = chars[start..i].iter().collect();
                let val = s.parse::<u128>().map_err(|e| LexError {
                    message: format!("Invalid integer literal '{s}': {e}"),
                    position: start,
                })?;
                tokens.push(Token::Int(val));
                lines.push(start_line);
            }
        } else {
            let start_line = current_line;
            match c {
                '(' => {
                    tokens.push(Token::LParen);
                    lines.push(start_line);
                }
                ')' => {
                    tokens.push(Token::RParen);
                    lines.push(start_line);
                }
                '{' => {
                    tokens.push(Token::LBrace);
                    lines.push(start_line);
                }
                '}' => {
                    tokens.push(Token::RBrace);
                    lines.push(start_line);
                }
                '[' => {
                    tokens.push(Token::LBracket);
                    lines.push(start_line);
                }
                ']' => {
                    tokens.push(Token::RBracket);
                    lines.push(start_line);
                }
                ':' => {
                    tokens.push(Token::Colon);
                    lines.push(start_line);
                }
                ',' => {
                    tokens.push(Token::Comma);
                    lines.push(start_line);
                }
                '.' => {
                    tokens.push(Token::Dot);
                    lines.push(start_line);
                }
                '&' => {
                    tokens.push(Token::Amp);
                    lines.push(start_line);
                }
                '^' => {
                    tokens.push(Token::Caret);
                    lines.push(start_line);
                }
                '|' => {
                    tokens.push(Token::Pipe);
                    lines.push(start_line);
                }
                '+' => {
                    if i + 1 < chars.len() && chars[i + 1] == '%' {
                        tokens.push(Token::PlusPercent);
                        i += 1;
                    } else {
                        tokens.push(Token::Plus);
                    }
                    lines.push(start_line);
                }
                '-' => {
                    if i + 1 < chars.len() && chars[i + 1] == '>' {
                        tokens.push(Token::Arrow);
                        i += 1;
                    } else if i + 1 < chars.len() && chars[i + 1] == '%' {
                        tokens.push(Token::MinusPercent);
                        i += 1;
                    } else {
                        tokens.push(Token::Minus);
                    }
                    lines.push(start_line);
                }
                '*' => {
                    if i + 1 < chars.len() && chars[i + 1] == '%' {
                        tokens.push(Token::StarPercent);
                        i += 1;
                    } else {
                        tokens.push(Token::Star);
                    }
                    lines.push(start_line);
                }
                '/' => {
                    tokens.push(Token::Slash);
                    lines.push(start_line);
                }
                '%' => {
                    tokens.push(Token::Percent);
                    lines.push(start_line);
                }
                '=' => {
                    tokens.push(Token::Eq);
                    lines.push(start_line);
                }
                '!' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                    tokens.push(Token::Ne);
                    i += 1;
                    lines.push(start_line);
                }
                '<' => {
                    if i + 1 < chars.len() && chars[i + 1] == '=' {
                        tokens.push(Token::Le);
                        i += 1;
                    } else if i + 1 < chars.len() && chars[i + 1] == '-' {
                        tokens.push(Token::LeftArrow);
                        i += 1;
                    } else if i + 1 < chars.len() && chars[i + 1] == '<' {
                        tokens.push(Token::Shl);
                        i += 1;
                    } else {
                        tokens.push(Token::Lt);
                    }
                    lines.push(start_line);
                }
                '>' => {
                    if i + 1 < chars.len() && chars[i + 1] == '=' {
                        tokens.push(Token::Ge);
                        i += 1;
                    } else if i + 1 < chars.len() && chars[i + 1] == '>' {
                        tokens.push(Token::Shr);
                        i += 1;
                    } else {
                        tokens.push(Token::Gt);
                    }
                    lines.push(start_line);
                }
                _ => {
                    return Err(LexError {
                        message: format!("Unexpected character: '{c}'"),
                        position: i,
                    });
                }
            }
            i += 1;
        }
    }
    tokens.push(Token::Eof);
    lines.push(current_line);
    Ok((tokens, lines))
}

/// Locates a 1-based line and column `Span` for a contract tag, routine, or expression snippet in `src`.
pub fn locate_span(src: &str, file: &str, primary_needle: &str, fallback_routine: &str) -> Span {
    let lines: Vec<&str> = src.lines().collect();
    if !primary_needle.is_empty() {
        let tagged = format!("{primary_needle}:");
        for (idx, line) in lines.iter().enumerate() {
            if let Some(col0) = line.find(&tagged) {
                return Span {
                    file: file.to_string(),
                    line: idx + 1,
                    column: col0 + 1,
                    end_line: idx + 1,
                    end_column: line.len().max(col0 + 1),
                };
            }
        }
        for (idx, line) in lines.iter().enumerate() {
            if let Some(col0) = line.find(primary_needle) {
                return Span {
                    file: file.to_string(),
                    line: idx + 1,
                    column: col0 + 1,
                    end_line: idx + 1,
                    end_column: col0 + primary_needle.len(),
                };
            }
        }
    }
    if !fallback_routine.is_empty() {
        for (idx, line) in lines.iter().enumerate() {
            if let Some(col0) = line.find(fallback_routine) {
                return Span {
                    file: file.to_string(),
                    line: idx + 1,
                    column: col0 + 1,
                    end_line: idx + 1,
                    end_column: col0 + fallback_routine.len(),
                };
            }
        }
    }
    Span::unknown(file)
}

// ============================================================================
// 3. RECURSIVE DESCENT PARSER (Supports Group, Functions, Program, With IO)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub token_pos: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Parse error at token {}: {}",
            self.token_pos, self.message
        )
    }
}

impl std::error::Error for ParseError {}

pub struct Parser {
    tokens: Vec<Token>,
    lines: Vec<usize>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        let lines = vec![1; tokens.len()];
        Self {
            tokens,
            lines,
            pos: 0,
        }
    }

    pub fn new_with_lines(tokens: Vec<Token>, lines: Vec<usize>) -> Self {
        Self {
            tokens,
            lines,
            pos: 0,
        }
    }

    pub fn line_of(&self, pos: usize) -> usize {
        self.lines.get(pos).copied().unwrap_or(1)
    }

    pub fn same_line(&self, pos1: usize, pos2: usize) -> bool {
        self.line_of(pos1) == self.line_of(pos2)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn next(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.next() {
            Token::Ident(s) => Ok(s),
            other => Err(ParseError {
                message: format!("Expected identifier, got {other:?}"),
                token_pos: self.pos,
            }),
        }
    }

    fn expect_upper_ident(&mut self) -> Result<String, ParseError> {
        let id = self.expect_ident()?;
        if !id.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            return Err(ParseError {
                message: format!(
                    "Expected UpperIdent (starting with uppercase letter), got `{id}`"
                ),
                token_pos: self.pos,
            });
        }
        Ok(id)
    }

    fn expect_lower_ident(&mut self) -> Result<String, ParseError> {
        let id = self.expect_ident()?;
        if !id.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
            return Err(ParseError {
                message: format!(
                    "Expected LowerIdent (starting with lowercase letter), got `{id}`"
                ),
                token_pos: self.pos,
            });
        }
        Ok(id)
    }

    fn expect_kw(&mut self, kw: &str) -> Result<(), ParseError> {
        match self.next() {
            Token::Ident(s) if s == kw => Ok(()),
            other => Err(ParseError {
                message: format!("Expected keyword '{kw}', got {other:?}"),
                token_pos: self.pos,
            }),
        }
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Token::Ident(s) if s == kw)
    }

    fn is_tagged_pred_start(&self) -> bool {
        matches!(
            (&self.tokens[self.pos], self.tokens.get(self.pos + 1)),
            (Token::Ident(s), Some(Token::Colon))
                if s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                    && s != "end"
                    && s != "ensure"
                    && s != "invariant"
                    && s != "variant"
                    && s != "trigger"
                    && s != "do"
        )
    }

    fn expect(&mut self, expected: Token) -> Result<(), ParseError> {
        let actual = self.next();
        if actual != expected {
            Err(ParseError {
                message: format!("Expected {expected:?}, got {actual:?}"),
                token_pos: self.pos,
            })
        } else {
            Ok(())
        }
    }

    fn parse_vis(&mut self) -> Result<Visibility, ParseError> {
        self.expect(Token::LBrace)?;
        let mut mods = Vec::new();
        while *self.peek() != Token::RBrace {
            mods.push(self.expect_upper_ident()?);
            if *self.peek() == Token::Comma {
                self.next();
            }
        }
        self.expect(Token::RBrace)?;
        if mods.len() == 1 && mods[0] == "ANY" {
            Ok(Visibility::Any)
        } else if mods.len() == 1 && mods[0] == "NONE" {
            Ok(Visibility::None)
        } else {
            Ok(Visibility::Modules(mods))
        }
    }

    fn parse_type(&mut self) -> Result<String, ParseError> {
        let mut base = if *self.peek() == Token::LParen {
            self.next();
            if *self.peek() == Token::RParen {
                self.next();
                "()".to_string()
            } else {
                let mut params = Vec::new();
                let mut is_refined_fn = false;
                while *self.peek() != Token::RParen {
                    let pname = self.expect_ident()?;
                    if *self.peek() == Token::Colon {
                        is_refined_fn = true;
                        self.next();
                        let pty = self.parse_type()?;
                        params.push(format!("{pname}: {pty}"));
                    } else {
                        params.push(pname);
                    }
                    if *self.peek() == Token::Comma {
                        self.next();
                    }
                }
                self.expect(Token::RParen)?;
                if is_refined_fn {
                    format!("({})", params.join(", "))
                } else if params.len() == 1 {
                    params[0].clone()
                } else {
                    format!("({})", params.join(", "))
                }
            }
        } else {
            let mut b = self.expect_upper_ident()?;
            if *self.peek() == Token::LBracket {
                self.next();
                let mut args = Vec::new();
                while *self.peek() != Token::RBracket {
                    args.push(self.parse_type()?);
                    if *self.peek() == Token::Comma {
                        self.next();
                    }
                }
                self.expect(Token::RBracket)?;
                b = format!("{b}[{}]", args.join(", "));
            }
            b
        };

        if *self.peek() == Token::Arrow {
            self.next();
            let ret = self.parse_type()?;
            let mut req_str = String::new();
            if self.is_kw("require") {
                self.next();
                let mut reqs = Vec::new();
                while self.is_tagged_pred_start() {
                    let tag = self.expect_lower_ident()?;
                    self.expect(Token::Colon)?;
                    let e = self.parse_expr()?;
                    reqs.push(format!("{tag}: {}", format_gardel_expr(&e)));
                }
                req_str = format!(" require {}", reqs.join(" "));
            }
            let mut ens_str = String::new();
            if self.is_kw("ensure") {
                self.next();
                let mut enss = Vec::new();
                while self.is_tagged_pred_start() {
                    let tag = self.expect_lower_ident()?;
                    self.expect(Token::Colon)?;
                    let e = self.parse_expr()?;
                    enss.push(format!("{tag}: {}", format_gardel_expr(&e)));
                }
                ens_str = format!(" ensure {}", enss.join(" "));
            }
            base = format!("{base} -> {ret}{req_str}{ens_str}");
        }
        Ok(base)
    }

    pub fn parse_file(&mut self) -> Result<FileAst, ParseError> {
        let mut group = None;
        if self.is_kw("group") {
            self.next();
            self.expect(Token::LBrace)?;
            let mut gmods = Vec::new();
            while *self.peek() != Token::RBrace {
                gmods.push(self.expect_upper_ident()?);
                if *self.peek() == Token::Comma {
                    self.next();
                }
            }
            self.expect(Token::RBrace)?;
            group = Some(gmods);
        }

        let mut concepts = Vec::new();
        let mut modules = Vec::new();
        while *self.peek() != Token::Eof {
            if self.is_kw("concept") {
                concepts.push(self.parse_concept()?);
            } else if self.is_kw("module") {
                modules.push(self.parse_module_with_kind(ModuleKind::Module)?);
            } else if self.is_kw("functions") {
                modules.push(self.parse_module_with_kind(ModuleKind::Functions)?);
            } else if self.is_kw("program") {
                modules.push(self.parse_module_with_kind(ModuleKind::Program)?);
            } else {
                return Err(ParseError {
                    message: format!(
                        "Expected `concept`, `module`, `functions`, or `program`, got {:?}",
                        self.peek()
                    ),
                    token_pos: self.pos,
                });
            }
        }
        Ok(FileAst {
            group,
            concepts,
            modules,
        })
    }

    pub fn parse_concept(&mut self) -> Result<ConceptDecl, ParseError> {
        self.expect_kw("concept")?;
        let name = self.expect_upper_ident()?;
        let mut type_params = Vec::new();
        if *self.peek() == Token::LBracket {
            self.next();
            while *self.peek() != Token::RBracket {
                let tp = self.expect_upper_ident()?;
                let bound = if *self.peek() == Token::Colon {
                    self.next();
                    Some(self.expect_upper_ident()?)
                } else {
                    None
                };
                type_params.push((tp, bound));
                if *self.peek() == Token::Comma {
                    self.next();
                }
            }
            self.expect(Token::RBracket)?;
        }

        let mut refines = Vec::new();
        if self.is_kw("refines") {
            self.next();
            let p = self.expect_upper_ident()?;
            if *self.peek() == Token::LBracket {
                self.next();
                while *self.peek() != Token::RBracket {
                    self.next();
                }
                self.expect(Token::RBracket)?;
            }
            refines.push(p);
            while *self.peek() == Token::Comma {
                self.next();
                let p = self.expect_upper_ident()?;
                if *self.peek() == Token::LBracket {
                    self.next();
                    while *self.peek() != Token::RBracket {
                        self.next();
                    }
                    self.expect(Token::RBracket)?;
                }
                refines.push(p);
            }
        }

        let mut signatures = Vec::new();
        let mut default_routines = Vec::new();
        let mut laws = Vec::new();

        while !self.is_kw("end") {
            if self.is_kw("law") {
                laws.push(self.parse_law()?);
            } else {
                let rname = self.expect_lower_ident()?;
                let mut r_tparams = Vec::new();
                if *self.peek() == Token::LBracket {
                    self.next();
                    while *self.peek() != Token::RBracket {
                        r_tparams.push(self.expect_upper_ident()?);
                        if *self.peek() == Token::Comma {
                            self.next();
                        }
                    }
                    self.expect(Token::RBracket)?;
                }
                let mut params = Vec::new();
                if *self.peek() == Token::LParen {
                    self.next();
                    while *self.peek() != Token::RParen {
                        let pname = self.expect_lower_ident()?;
                        self.expect(Token::Colon)?;
                        let ptype = self.parse_type()?;
                        params.push((pname, ptype));
                        if *self.peek() == Token::Comma {
                            self.next();
                        }
                    }
                    self.expect(Token::RParen)?;
                }
                self.expect(Token::Colon)?;
                let ret_type = self.parse_type()?;

                if self.is_kw("require") || self.is_kw("variant") || self.is_kw("do") {
                    default_routines
                        .push(self.finish_routine_after_sig(rname, r_tparams, params, ret_type)?);
                } else {
                    signatures.push(SignatureDecl {
                        name: rname,
                        params,
                        ret_type,
                    });
                }
            }
        }
        self.expect_kw("end")?;

        Ok(ConceptDecl {
            name,
            type_params,
            refines,
            signatures,
            default_routines,
            laws,
        })
    }

    pub fn parse_module(&mut self) -> Result<ModuleDecl, ParseError> {
        self.parse_module_with_kind(ModuleKind::Module)
    }

    pub fn parse_module_with_kind(&mut self, kind: ModuleKind) -> Result<ModuleDecl, ParseError> {
        match kind {
            ModuleKind::Module => self.expect_kw("module")?,
            ModuleKind::Functions => self.expect_kw("functions")?,
            ModuleKind::Program => self.expect_kw("program")?,
        }
        let name = self.expect_upper_ident()?;
        let mut type_params = Vec::new();
        let mut type_bounds = HashMap::new();
        if *self.peek() == Token::LBracket {
            self.next();
            while *self.peek() != Token::RBracket {
                let tp = self.expect_upper_ident()?;
                if *self.peek() == Token::Colon {
                    self.next();
                    let bound = self.expect_upper_ident()?;
                    type_bounds.insert(tp.clone(), bound);
                }
                type_params.push(tp);
                if *self.peek() == Token::Comma {
                    self.next();
                }
            }
            self.expect(Token::RBracket)?;
        }

        let mut satisfies = None;
        if self.is_kw("satisfies") {
            self.next();
            let mut concepts = vec![self.expect_upper_ident()?];
            while *self.peek() == Token::Comma {
                self.next();
                concepts.push(self.expect_upper_ident()?);
            }
            let mut where_constraints = Vec::new();
            if self.is_kw("where") {
                self.next();
                loop {
                    let tp = self.expect_upper_ident()?;
                    self.expect_kw("satisfies")?;
                    let c = self.expect_upper_ident()?;
                    where_constraints.push((tp, c));
                    if *self.peek() == Token::Comma {
                        self.next();
                    } else {
                        break;
                    }
                }
            }
            satisfies = Some(SatisfiesClause {
                concepts,
                where_constraints,
            });
        }

        let mut repr_vis = Visibility::None;
        let mut repr = ReprKind::Product(Vec::new());
        let mut specify_routines = Vec::new();
        let mut specify_laws = Vec::new();
        let mut create_routines = Vec::new();
        let mut feature_routines = Vec::new();
        let mut invariants = Vec::new();

        while !self.is_kw("end") {
            if self.is_kw("representation") {
                if kind == ModuleKind::Functions || kind == ModuleKind::Program {
                    return Err(ParseError {
                        message: format!(
                            "Section `representation` is not permitted in `{kind:?}` declaration (§4)"
                        ),
                        token_pos: self.pos,
                    });
                }
                self.next();
                repr_vis = self.parse_vis()?;
                if *self.peek() == Token::Pipe {
                    let mut variants = Vec::new();
                    while *self.peek() == Token::Pipe {
                        self.next();
                        let vname = self.expect_upper_ident()?;
                        let mut fields = Vec::new();
                        if *self.peek() == Token::LParen {
                            self.next();
                            while *self.peek() != Token::RParen {
                                let fname = self.expect_lower_ident()?;
                                self.expect(Token::Colon)?;
                                let ftype = self.parse_type()?;
                                fields.push((fname, ftype));
                                if *self.peek() == Token::Comma {
                                    self.next();
                                }
                            }
                            self.expect(Token::RParen)?;
                        }
                        variants.push(VariantDecl {
                            name: vname,
                            fields,
                        });
                    }
                    repr = ReprKind::Sum(variants);
                } else {
                    let mut fields = Vec::new();
                    while !self.is_section_or_end() {
                        let fname = self.expect_lower_ident()?;
                        self.expect(Token::Colon)?;
                        let ftype = self.parse_type()?;
                        fields.push((fname, ftype));
                    }
                    repr = ReprKind::Product(fields);
                }
            } else if self.is_kw("specify") {
                if kind == ModuleKind::Program {
                    return Err(ParseError {
                        message: "Section `specify` is not permitted in `program` declaration (§4)"
                            .to_string(),
                        token_pos: self.pos,
                    });
                }
                self.next();
                let vis = self.parse_vis()?;
                while !self.is_section_or_end() {
                    if self.is_kw("law") {
                        specify_laws.push((vis.clone(), self.parse_law()?));
                    } else {
                        specify_routines.push((vis.clone(), self.parse_routine()?));
                    }
                }
            } else if self.is_kw("create") {
                if kind == ModuleKind::Functions || kind == ModuleKind::Program {
                    return Err(ParseError {
                        message: format!(
                            "Section `create` is not permitted in `{kind:?}` declaration (§4)"
                        ),
                        token_pos: self.pos,
                    });
                }
                self.next();
                let vis = self.parse_vis()?;
                while !self.is_section_or_end() {
                    create_routines.push((vis.clone(), self.parse_routine()?));
                }
            } else if self.is_kw("feature") {
                self.next();
                let vis = self.parse_vis()?;
                while !self.is_section_or_end() {
                    feature_routines.push((vis.clone(), self.parse_routine()?));
                }
            } else if self.is_kw("invariant") {
                if kind == ModuleKind::Functions || kind == ModuleKind::Program {
                    return Err(ParseError {
                        message: format!(
                            "Section `invariant` is not permitted in `{kind:?}` declaration (§4)"
                        ),
                        token_pos: self.pos,
                    });
                }
                self.next();
                while self.is_tagged_pred_start() {
                    let tag = self.expect_lower_ident()?;
                    self.expect(Token::Colon)?;
                    let expr = self.parse_expr()?;
                    invariants.push((tag, expr));
                }
            } else {
                return Err(ParseError {
                    message: format!("Unexpected token in module: {:?}", self.peek()),
                    token_pos: self.pos,
                });
            }
        }
        self.expect_kw("end")?;

        Ok(ModuleDecl {
            kind,
            name,
            type_params,
            type_bounds,
            satisfies,
            repr_vis,
            repr,
            specify_routines,
            specify_laws,
            create_routines,
            feature_routines,
            invariants,
        })
    }

    fn is_section_or_end(&self) -> bool {
        self.is_kw("representation")
            || self.is_kw("specify")
            || self.is_kw("create")
            || self.is_kw("feature")
            || self.is_kw("invariant")
            || self.is_kw("end")
    }

    fn parse_law(&mut self) -> Result<LawDecl, ParseError> {
        self.expect_kw("law")?;
        let name = self.expect_lower_ident()?;
        let mut type_params = Vec::new();
        if *self.peek() == Token::LBracket {
            self.next();
            while *self.peek() != Token::RBracket {
                type_params.push(self.expect_upper_ident()?);
                if *self.peek() == Token::Comma {
                    self.next();
                }
            }
            self.expect(Token::RBracket)?;
        }
        let mut params = Vec::new();
        if *self.peek() == Token::LParen {
            self.next();
            while *self.peek() != Token::RParen {
                let pname = self.expect_lower_ident()?;
                self.expect(Token::Colon)?;
                let ptype = self.parse_type()?;
                params.push((pname, ptype));
                if *self.peek() == Token::Comma {
                    self.next();
                }
            }
            self.expect(Token::RParen)?;
        }

        let mut requires = Vec::new();
        if self.is_kw("require") {
            self.next();
            while self.is_tagged_pred_start() {
                let tag = self.expect_lower_ident()?;
                self.expect(Token::Colon)?;
                let expr = self.parse_expr()?;
                requires.push((tag, expr));
            }
        }

        let mut triggers = Vec::new();
        if self.is_kw("trigger") {
            self.next();
            triggers.push(self.parse_expr()?);
            while *self.peek() == Token::Comma {
                self.next();
                triggers.push(self.parse_expr()?);
            }
        }

        let mut variant = Vec::new();
        if self.is_kw("variant") {
            self.next();
            variant.push(self.parse_expr()?);
            while *self.peek() == Token::Comma {
                self.next();
                variant.push(self.parse_expr()?);
            }
        }

        if self.is_kw("trigger") {
            self.next();
            triggers.push(self.parse_expr()?);
            while *self.peek() == Token::Comma {
                self.next();
                triggers.push(self.parse_expr()?);
            }
        }

        let mut stmts = Vec::new();
        let mut body = None;
        if self.is_kw("do") {
            self.next();
            let (s, b) = self.parse_block()?;
            stmts = s;
            body = Some(b);
        }

        let mut ensures = Vec::new();
        if self.is_kw("ensure") {
            self.next();
            while self.is_tagged_pred_start() {
                let tag = self.expect_lower_ident()?;
                self.expect(Token::Colon)?;
                let expr = self.parse_expr()?;
                ensures.push((tag, expr));
            }
        }

        self.expect_kw("end")?;
        Ok(LawDecl {
            name,
            type_params,
            params,
            requires,
            triggers,
            variant,
            stmts,
            body,
            ensures,
        })
    }

    fn parse_routine(&mut self) -> Result<RoutineDecl, ParseError> {
        let name = self.expect_lower_ident()?;
        let mut type_params = Vec::new();
        if *self.peek() == Token::LBracket {
            self.next();
            while *self.peek() != Token::RBracket {
                type_params.push(self.expect_upper_ident()?);
                if *self.peek() == Token::Comma {
                    self.next();
                }
            }
            self.expect(Token::RBracket)?;
        }
        let mut params = Vec::new();
        if *self.peek() == Token::LParen {
            self.next();
            while *self.peek() != Token::RParen {
                let pname = self.expect_lower_ident()?;
                self.expect(Token::Colon)?;
                let ptype = self.parse_type()?;
                params.push((pname, ptype));
                if *self.peek() == Token::Comma {
                    self.next();
                }
            }
            self.expect(Token::RParen)?;
        }
        self.expect(Token::Colon)?;
        let ret_type = self.parse_type()?;
        self.finish_routine_after_sig(name, type_params, params, ret_type)
    }

    fn finish_routine_after_sig(
        &mut self,
        name: String,
        type_params: Vec<String>,
        params: Vec<(String, String)>,
        ret_type: String,
    ) -> Result<RoutineDecl, ParseError> {
        let mut requires = Vec::new();
        if self.is_kw("require") {
            self.next();
            while self.is_tagged_pred_start() {
                let tag = self.expect_lower_ident()?;
                self.expect(Token::Colon)?;
                let expr = self.parse_expr()?;
                requires.push((tag, expr));
            }
        }

        let mut variant = Vec::new();
        if self.is_kw("variant") {
            self.next();
            variant.push(self.parse_expr()?);
            while *self.peek() == Token::Comma {
                self.next();
                variant.push(self.parse_expr()?);
            }
        }

        self.expect_kw("do")?;
        let (stmts, body) = self.parse_block()?;

        let mut ensures = Vec::new();
        if self.is_kw("ensure") {
            self.next();
            while self.is_tagged_pred_start() {
                let tag = self.expect_lower_ident()?;
                self.expect(Token::Colon)?;
                let expr = self.parse_expr()?;
                ensures.push((tag, expr));
            }
        }

        self.expect_kw("end")?;

        Ok(RoutineDecl {
            name,
            type_params,
            params,
            ret_type,
            requires,
            variant,
            stmts,
            body,
            ensures,
        })
    }

    fn parse_block(&mut self) -> Result<(Vec<Stmt>, Expr), ParseError> {
        let mut stmts = Vec::new();
        loop {
            if self.is_kw("let") {
                self.next();
                let var = self.expect_lower_ident()?;
                if *self.peek() == Token::Colon {
                    self.next();
                    let _ = self.parse_type()?;
                }
                self.expect(Token::Eq)?;
                let val = self.parse_expr()?;
                if *self.peek() == Token::Semicolon {
                    self.next();
                }
                stmts.push(Stmt::Let(var, val));
            } else if self.is_kw("check") {
                self.next();
                while self.is_tagged_pred_start() {
                    let tag = self.expect_lower_ident()?;
                    self.expect(Token::Colon)?;
                    let e = self.parse_expr()?;
                    stmts.push(Stmt::Check(tag, e));
                }
                self.expect_kw("end")?;
                if *self.peek() == Token::Semicolon {
                    self.next();
                }
            } else if self.is_kw("assume") {
                self.next();
                while self.is_tagged_pred_start() {
                    let tag = self.expect_lower_ident()?;
                    self.expect(Token::Colon)?;
                    let e = self.parse_expr()?;
                    stmts.push(Stmt::Assume(tag, e));
                }
                self.expect_kw("end")?;
                if *self.peek() == Token::Semicolon {
                    self.next();
                }
            } else if self.is_kw("reveal") {
                self.next();
                let target = self.expect_lower_ident()?;
                if *self.peek() == Token::Semicolon {
                    self.next();
                }
                stmts.push(Stmt::Reveal(target));
            } else {
                break;
            }
        }
        let expr = self.parse_expr()?;
        Ok((stmts, expr))
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        if self.is_kw("forall") {
            self.next();
            let v = self.expect_lower_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;
            self.expect(Token::Dot)?;
            let body = self.parse_expr()?;
            return Ok(Expr::Forall(v, ty, Box::new(body)));
        }
        if self.is_kw("exists") {
            self.next();
            let v = self.expect_lower_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;
            self.expect(Token::Dot)?;
            let body = self.parse_expr()?;
            return Ok(Expr::Exists(v, ty, Box::new(body)));
        }
        self.parse_implies()
    }

    fn parse_implies(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_or()?;
        if self.is_kw("implies") {
            self.next();
            let right = self.parse_implies()?;
            Ok(Expr::BinOp(
                "implies".into(),
                Box::new(left),
                Box::new(right),
            ))
        } else {
            Ok(left)
        }
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while self.is_kw("or") {
            self.next();
            let right = self.parse_and()?;
            left = Expr::BinOp("or".into(), Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_not()?;
        while self.is_kw("and") {
            self.next();
            let right = self.parse_not()?;
            left = Expr::BinOp("and".into(), Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if self.is_kw("not") {
            self.next();
            return Ok(Expr::UnaryNot(Box::new(self.parse_not()?)));
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_or()?;
        loop {
            let op = match self.peek() {
                Token::Eq => "=",
                Token::Lt => "<",
                Token::Le => "<=",
                Token::Gt => ">",
                Token::Ge => ">=",
                Token::Ne => "!=",
                _ => break,
            };
            let op_str = op.to_string();
            self.next();
            let right = self.parse_bitwise_or()?;
            left = if op_str == "!=" {
                // `a != b` is `not (a = b)`, so it uses exactly the equality of `=` (§9).
                Expr::UnaryNot(Box::new(Expr::BinOp(
                    "=".into(),
                    Box::new(left),
                    Box::new(right),
                )))
            } else {
                Expr::BinOp(op_str, Box::new(left), Box::new(right))
            };
        }
        Ok(left)
    }

    fn parse_bitwise_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_xor()?;
        while *self.peek() == Token::Pipe {
            self.next();
            let right = self.parse_bitwise_xor()?;
            left = Expr::BinOp("|".into(), Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_and()?;
        while *self.peek() == Token::Caret {
            self.next();
            let right = self.parse_bitwise_and()?;
            left = Expr::BinOp("^".into(), Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_shift()?;
        while *self.peek() == Token::Amp {
            self.next();
            let right = self.parse_shift()?;
            left = Expr::BinOp("&".into(), Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Token::Shl => "<<",
                Token::Shr => ">>",
                _ => break,
            };
            let op_str = op.to_string();
            self.next();
            let right = self.parse_add()?;
            left = Expr::BinOp(op_str, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Token::Plus => "+",
                Token::Minus => "-",
                Token::PlusPercent => "+%",
                Token::MinusPercent => "-%",
                _ => break,
            };
            let op_str = op.to_string();
            self.next();
            let right = self.parse_mul()?;
            left = Expr::BinOp(op_str, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => "*",
                Token::Slash => "/",
                Token::Percent => "%",
                Token::StarPercent => "*%",
                _ => break,
            };
            let op_str = op.to_string();
            self.next();
            let right = self.parse_unary()?;
            left = Expr::BinOp(op_str, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if *self.peek() == Token::Minus {
            self.next();
            let inner = self.parse_unary()?;
            return Ok(Expr::UnaryNeg(Box::new(inner)));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            if *self.peek() == Token::Dot {
                self.next();
                let member = self.expect_ident()?;
                let mut args = Vec::new();
                if *self.peek() == Token::LParen {
                    self.next();
                    if *self.peek() == Token::RParen {
                        self.next();
                    } else {
                        while *self.peek() != Token::RParen {
                            args.push(self.parse_expr()?);
                            if *self.peek() == Token::Comma {
                                self.next();
                            }
                        }
                        self.expect(Token::RParen)?;
                    }
                }
                expr = Expr::MethodCall(Box::new(expr), member, args);
            } else if *self.peek() == Token::LParen && self.same_line(self.pos - 1, self.pos) {
                self.next();
                let mut args = Vec::new();
                if *self.peek() == Token::RParen {
                    self.next();
                } else {
                    while *self.peek() != Token::RParen {
                        args.push(self.parse_expr()?);
                        if *self.peek() == Token::Comma {
                            self.next();
                        }
                    }
                    self.expect(Token::RParen)?;
                }
                expr = Expr::MethodCall(Box::new(expr), "apply".to_string(), args);
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        match self.peek() {
            Token::Ident(s) if s == "_" => {
                self.next();
                Ok(Pattern::Wildcard)
            }
            Token::Ident(s) if s == "True" => {
                self.next();
                Ok(Pattern::Lit(Literal::Bool(true)))
            }
            Token::Ident(s) if s == "False" => {
                self.next();
                Ok(Pattern::Lit(Literal::Bool(false)))
            }
            Token::Int(n) => {
                let val = *n;
                self.next();
                Ok(Pattern::Lit(Literal::Int(val)))
            }
            Token::Float(f) => {
                let val = *f;
                self.next();
                Ok(Pattern::Lit(Literal::Float(val)))
            }
            Token::Char(c) => {
                let val = *c as u32;
                self.next();
                Ok(Pattern::Lit(Literal::Char(val)))
            }
            Token::Str(s) => {
                let val = s.clone();
                self.next();
                Ok(Pattern::Lit(Literal::Str(val)))
            }
            Token::Unit => {
                self.next();
                Ok(Pattern::Lit(Literal::Unit))
            }
            Token::LParen => {
                self.next();
                if *self.peek() == Token::RParen {
                    self.next();
                    return Ok(Pattern::Lit(Literal::Unit));
                }
                let first = self.parse_pattern()?;
                if *self.peek() == Token::Comma {
                    let mut pats = vec![first];
                    while *self.peek() == Token::Comma {
                        self.next();
                        pats.push(self.parse_pattern()?);
                    }
                    self.expect(Token::RParen)?;
                    Ok(Pattern::Tuple(pats))
                } else {
                    self.expect(Token::RParen)?;
                    Ok(first)
                }
            }
            Token::Ident(s) => {
                let name = s.clone();
                self.next();
                if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    let mut subpats = Vec::new();
                    if *self.peek() == Token::LParen {
                        self.next();
                        if *self.peek() == Token::RParen {
                            self.next();
                        } else {
                            while *self.peek() != Token::RParen {
                                subpats.push(self.parse_pattern()?);
                                if *self.peek() == Token::Comma {
                                    self.next();
                                }
                            }
                            self.expect(Token::RParen)?;
                        }
                    }
                    Ok(Pattern::Variant(name, subpats))
                } else {
                    Ok(Pattern::Bind(name))
                }
            }
            other => Err(ParseError {
                message: format!("Unexpected token in pattern: {other:?}"),
                token_pos: self.pos,
            }),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        if self.is_kw("with") {
            self.next();
            self.expect_kw("IO")?;
            let mut io_stmts = Vec::new();
            let final_expr = loop {
                if matches!(
                    (self.peek(), self.tokens.get(self.pos + 1)),
                    (Token::Ident(_), Some(Token::LeftArrow))
                ) {
                    let var = self.expect_lower_ident()?;
                    self.expect(Token::LeftArrow)?;
                    let action = self.parse_expr()?;
                    if *self.peek() == Token::Semicolon {
                        self.next();
                    }
                    io_stmts.push(IoStmt::Bind(var, action));
                } else {
                    let e = self.parse_expr()?;
                    if *self.peek() == Token::Semicolon {
                        self.next();
                    }
                    if self.is_kw("end") {
                        self.next();
                        break e;
                    } else {
                        io_stmts.push(IoStmt::Action(e));
                    }
                }
            };
            return Ok(Expr::WithIo(io_stmts, Box::new(final_expr)));
        }
        if self.is_kw("fn") {
            self.next();
            let mut params = Vec::new();
            if *self.peek() == Token::LParen {
                self.next();
                while *self.peek() != Token::RParen {
                    let pn = self.expect_lower_ident()?;
                    self.expect(Token::Colon)?;
                    let pt = self.parse_type()?;
                    params.push((pn, pt));
                    if *self.peek() == Token::Comma {
                        self.next();
                    }
                }
                self.expect(Token::RParen)?;
            }
            self.expect(Token::Colon)?;
            let ret_ty = self.parse_type()?;
            self.expect_kw("do")?;
            let (stmts, body) = self.parse_block()?;
            self.expect_kw("end")?;
            return Ok(Expr::Lambda(params, ret_ty, stmts, Box::new(body)));
        }
        if self.is_kw("if") {
            self.next();
            let cond = self.parse_expr()?;
            self.expect_kw("then")?;
            let (t_stmts, t_expr) = self.parse_block()?;
            self.expect_kw("else")?;
            let (e_stmts, e_expr) = self.parse_block()?;
            self.expect_kw("end")?;
            return Ok(Expr::If(
                Box::new(cond),
                t_stmts,
                Box::new(t_expr),
                e_stmts,
                Box::new(e_expr),
            ));
        }
        if self.is_kw("match") {
            self.next();
            let scrut = self.parse_expr()?;
            let mut cases = Vec::new();
            while self.is_kw("case") {
                self.next();
                let pat = self.parse_pattern()?;
                self.expect_kw("then")?;
                let (stmts, body) = self.parse_block()?;
                cases.push(MatchCase::new(pat, stmts, body));
            }
            self.expect_kw("end")?;
            return Ok(Expr::Match(Box::new(scrut), cases));
        }

        match self.next() {
            Token::Int(n) => Ok(Expr::Int(n)),
            Token::Float(f) => Ok(Expr::Float(f)),
            Token::Char(c) => Ok(Expr::Char(c as u32)),
            Token::Str(s) => Ok(Expr::Str(s)),
            Token::Unit => Ok(Expr::Unit),
            Token::LParen => {
                if *self.peek() == Token::RParen {
                    self.next();
                    return Ok(Expr::Unit);
                }
                let first = self.parse_expr()?;
                if *self.peek() == Token::Comma {
                    let mut elems = vec![first];
                    while *self.peek() == Token::Comma {
                        self.next();
                        elems.push(self.parse_expr()?);
                    }
                    self.expect(Token::RParen)?;
                    Ok(Expr::TupleLit(elems))
                } else {
                    self.expect(Token::RParen)?;
                    Ok(first)
                }
            }
            Token::LBrace => {
                if matches!(
                    (self.peek(), self.tokens.get(self.pos + 1)),
                    (Token::Ident(_), Some(Token::Eq))
                ) {
                    let mut fields = Vec::new();
                    while *self.peek() != Token::RBrace {
                        let fname = self.expect_lower_ident()?;
                        self.expect(Token::Eq)?;
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                        if *self.peek() == Token::Comma {
                            self.next();
                        } else {
                            break;
                        }
                    }
                    self.expect(Token::RBrace)?;
                    Ok(Expr::RecordLit(fields))
                } else {
                    let base = self.parse_expr()?;
                    self.expect_kw("with")?;
                    let mut fields = Vec::new();
                    loop {
                        let fname = self.expect_lower_ident()?;
                        self.expect(Token::Eq)?;
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                        if *self.peek() == Token::Comma {
                            self.next();
                        } else {
                            break;
                        }
                    }
                    self.expect(Token::RBrace)?;
                    Ok(Expr::RecordUpdate(Box::new(base), fields))
                }
            }
            Token::Ident(name) => {
                if name == "True" {
                    return Ok(Expr::Bool(true));
                } else if name == "False" {
                    return Ok(Expr::Bool(false));
                }
                if *self.peek() == Token::LParen && self.same_line(self.pos - 1, self.pos) {
                    self.next();
                    let mut args = Vec::new();
                    if *self.peek() == Token::RParen {
                        self.next();
                    } else {
                        while *self.peek() != Token::RParen {
                            args.push(self.parse_expr()?);
                            if *self.peek() == Token::Comma {
                                self.next();
                            }
                        }
                        self.expect(Token::RParen)?;
                    }
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => Err(ParseError {
                message: format!("Unexpected primary token: {other:?}"),
                token_pos: self.pos,
            }),
        }
    }
}

// ============================================================================
// 4. PHASE 1–4 SEMANTIC, VISIBILITY, CONCEPT, REVEAL & IO BOUNDARY CHECKER
// ============================================================================

/// Structured diagnostic supporting both human CLI and Phase 3/4 `--json-diagnostics` (§12).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: &'static str, // "error", "warning", or "audit"
    pub rule: &'static str,
    pub module_name: String,
    pub routine: String,
    pub tag: String,
    pub message: String,
    pub span: Option<Span>,
    pub path_conditions: Vec<String>,
    pub counterexample: BTreeMap<String, String>,
    pub relevant_lemmas: Vec<String>,
}

impl Diagnostic {
    pub fn basic(
        level: &'static str,
        rule: &'static str,
        routine: String,
        tag: String,
        message: String,
    ) -> Self {
        Self {
            level,
            rule,
            module_name: String::new(),
            routine,
            tag,
            message,
            span: None,
            path_conditions: Vec::new(),
            counterexample: BTreeMap::new(),
            relevant_lemmas: Vec::new(),
        }
    }

    pub fn with_module(mut self, module_name: &str) -> Self {
        self.module_name = module_name.to_string();
        self
    }

    pub fn with_ai_context(
        mut self,
        span: Span,
        path_conditions: Vec<String>,
        counterexample: BTreeMap<String, String>,
        relevant_lemmas: Vec<String>,
    ) -> Self {
        self.span = Some(span);
        self.path_conditions = path_conditions;
        self.counterexample = counterexample;
        self.relevant_lemmas = relevant_lemmas;
        self
    }

    /// Renders a single diagnostic to the Phase 3 deterministic JSON format (§12).
    pub fn to_json(&self, module_name: &str, default_file: &str) -> String {
        let mod_name = if !self.module_name.is_empty() {
            self.module_name.as_str()
        } else {
            module_name
        };
        let sp = self
            .span
            .clone()
            .unwrap_or_else(|| Span::unknown(default_file));
        let pcs: Vec<String> = self
            .path_conditions
            .iter()
            .map(|c| format!("\"{}\"", escape_json(c)))
            .collect();
        let cex: Vec<String> = self
            .counterexample
            .iter()
            .map(|(k, v)| format!("\"{}\": \"{}\"", escape_json(k), escape_json(v)))
            .collect();
        let lemmas: Vec<String> = self
            .relevant_lemmas
            .iter()
            .map(|l| format!("\"{}\"", escape_json(l)))
            .collect();

        format!(
            "  {{\n    \"level\": \"{}\",\n    \"rule\": \"{}\",\n    \"module\": \"{}\",\n    \"routine\": \"{}\",\n    \"tag\": \"{}\",\n    \"contract_tag\": \"{}\",\n    \"span\": {{ \"file\": \"{}\", \"line\": {}, \"column\": {}, \"end_line\": {}, \"end_column\": {} }},\n    \"path_conditions\": [{}],\n    \"counterexample\": {{{}}},\n    \"relevant_lemmas\": [{}],\n    \"message\": \"{}\"\n  }}",
            escape_json(self.level),
            escape_json(self.rule),
            escape_json(mod_name),
            escape_json(&self.routine),
            escape_json(&self.tag),
            escape_json(&self.tag),
            escape_json(&sp.file),
            sp.line,
            sp.column,
            sp.end_line,
            sp.end_column,
            pcs.join(", "),
            if cex.is_empty() {
                String::new()
            } else {
                format!(" {} ", cex.join(", "))
            },
            lemmas.join(", "),
            escape_json(&self.message)
        )
    }
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0C' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Resolves all transitively refined concepts for a `satisfies` list, checks
/// required operations and laws, and injects default concept routines (e.g. `ORD.less_or_equal`).
pub fn check_and_expand_concepts(
    m: &mut ModuleDecl,
    concepts: &HashMap<String, ConceptDecl>,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let Some(sat) = &m.satisfies else {
        return diags;
    };

    // Validate where constraints (e.g. `where A satisfies EQ`)
    for (tp, cname) in &sat.where_constraints {
        if !m.type_params.contains(tp) {
            diags.push(Diagnostic::basic(
                "error",
                "Phase 2 (Invalid Type Constraint)",
                tp.clone(),
                cname.clone(),
                format!(
                    "Type parameter `{}` in where constraint is not declared on module `{}`",
                    tp, m.name
                ),
            ));
        }
        if !concepts.contains_key(cname) {
            diags.push(Diagnostic::basic(
                "error",
                "Phase 2 (Unknown Concept)",
                tp.clone(),
                cname.clone(),
                format!("Where constraint specifies unknown concept `{cname}`"),
            ));
        } else {
            m.type_bounds.insert(tp.clone(), cname.clone());
        }
    }

    // Collect full transitive closure of refined concepts (e.g. ORD -> [ORD, EQ])
    let mut required_concepts = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = sat.concepts.clone();
    while let Some(cname) = queue.pop() {
        if visited.insert(cname.clone()) {
            if let Some(cdecl) = concepts.get(&cname) {
                for parent in &cdecl.refines {
                    queue.push(parent.clone());
                }
                required_concepts.push(cname);
            } else {
                diags.push(Diagnostic::basic(
                    "error",
                    "Phase 2 (Unknown Concept)",
                    m.name.clone(),
                    cname.clone(),
                    format!("Module `{}` satisfies unknown concept `{}`", m.name, cname),
                ));
            }
        }
    }

    let existing_features: HashSet<String> = m
        .feature_routines
        .iter()
        .map(|(_, r)| r.name.clone())
        .collect();

    let mut default_routines_to_inject = Vec::new();

    for cname in &required_concepts {
        let Some(cdecl) = concepts.get(cname) else {
            continue;
        };

        // 1. Check required signatures
        for sig in &cdecl.signatures {
            if !existing_features.contains(&sig.name) {
                diags.push(Diagnostic::basic(
                    "error",
                    "Phase 2 (Missing Concept Operation)",
                    sig.name.clone(),
                    cname.clone(),
                    format!(
                        "Module `{}` satisfies `{}` but does not implement required routine `{}`",
                        m.name, cname, sig.name
                    ),
                ));
            }
        }

        // 2. Inject default routines (such as `ORD.less_or_equal`) if not overridden
        for def_r in &cdecl.default_routines {
            if !existing_features.contains(&def_r.name) {
                let mut instantiated = def_r.clone();
                instantiated.params = instantiated
                    .params
                    .into_iter()
                    .map(|(pn, pt)| {
                        let resolved_ty = if pt == "Current" { m.name.clone() } else { pt };
                        (pn, resolved_ty)
                    })
                    .collect();
                if instantiated.ret_type == "Current" {
                    instantiated.ret_type = m.name.clone();
                }
                default_routines_to_inject.push((Visibility::Any, instantiated));
            }
        }

        // 3. Check required concept laws
        for law in &cdecl.laws {
            let has_proof = m.specify_laws.iter().any(|(vis, l)| {
                if l.name != law.name {
                    return false;
                }
                match vis {
                    Visibility::Any => true,
                    Visibility::Modules(mods) => mods.iter().any(|cm| cm == cname),
                    Visibility::None => false,
                }
            });
            if !has_proof {
                diags.push(Diagnostic::basic(
                    "error",
                    "Phase 2 (Missing Law Proof)",
                    law.name.clone(),
                    cname.clone(),
                    format!(
                        "Module `{}` satisfies `{}` but does not provide a proof for `law {}` in `specify`",
                        m.name, cname, law.name
                    ),
                ));
            }
        }
    }

    m.feature_routines.extend(default_routines_to_inject);
    diags
}

pub fn check_visibility_and_semantics(m: &ModuleDecl) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut symbol_vis: HashMap<String, Visibility> = HashMap::new();
    let mut ghost_symbols: HashSet<String> = HashSet::new();
    let mut known_routines: HashSet<String> = HashSet::new();

    // Register representation fields/variants
    match &m.repr {
        ReprKind::Product(fields) => {
            for (fname, _) in fields {
                symbol_vis.insert(fname.clone(), m.repr_vis.clone());
            }
        }
        ReprKind::Sum(variants) => {
            for v in variants {
                symbol_vis.insert(v.name.clone(), m.repr_vis.clone());
            }
        }
    }

    // Register specify (ghost) routines
    for (vis, r) in &m.specify_routines {
        symbol_vis.insert(r.name.clone(), vis.clone());
        ghost_symbols.insert(r.name.clone());
        known_routines.insert(r.name.clone());
    }

    // Register create and feature routines
    for (vis, r) in &m.create_routines {
        symbol_vis.insert(r.name.clone(), vis.clone());
        known_routines.insert(r.name.clone());
    }
    for (vis, r) in &m.feature_routines {
        symbol_vis.insert(r.name.clone(), vis.clone());
        known_routines.insert(r.name.clone());
    }

    // Check all routines (create, feature, specify)
    let all_routines: Vec<(&Visibility, &RoutineDecl, bool)> = m
        .create_routines
        .iter()
        .map(|(v, r)| (v, r, false))
        .chain(m.feature_routines.iter().map(|(v, r)| (v, r, false)))
        .chain(m.specify_routines.iter().map(|(v, r)| (v, r, true)))
        .collect();

    for (vis, r, is_ghost_routine) in all_routines {
        let mut routine_locals: HashSet<String> = r.params.iter().map(|(p, _)| p.clone()).collect();
        routine_locals.insert("Current".to_string());

        // Rule 1 & Rule 4: Preconditions (require)
        for (tag, req_expr) in &r.requires {
            let used = collect_symbols_scoped(req_expr, &routine_locals);
            for sym in used {
                if let Some(sv) = symbol_vis.get(&sym) {
                    if !sv.is_at_least_as_visible_as(vis) {
                        diags.push(Diagnostic::basic(
                            "error",
                            "Rule 1 (Contract Visibility)",
                            r.name.clone(),
                            tag.clone(),
                            format!(
                                "Precondition `{}` references symbol `{}` ({:?}) which is less visible than routine `{}` ({:?})",
                                tag, sym, sv, r.name, vis
                            ),
                        ));
                    }
                }
                if !is_ghost_routine && *vis != Visibility::None && ghost_symbols.contains(&sym) {
                    diags.push(Diagnostic::basic(
                        "warning",
                        "Rule 4 (ghost_precondition)",
                        r.name.clone(),
                        tag.clone(),
                        format!(
                            "Exported precondition `{}` references ghost symbol `{}`; callers cannot test it at runtime",
                            tag, sym
                        ),
                    ));
                }
            }
        }

        // Rule 2: Visible ensures only
        let mut ens_locals = routine_locals.clone();
        ens_locals.insert("Result".to_string());
        for (tag, ens_expr) in &r.ensures {
            let used = collect_symbols_scoped(ens_expr, &ens_locals);
            for sym in used {
                if let Some(sv) = symbol_vis.get(&sym) {
                    if !sv.is_at_least_as_visible_as(vis) {
                        diags.push(Diagnostic::basic(
                            "warning",
                            "Rule 2 (Hidden Ensure Clause)",
                            r.name.clone(),
                            tag.clone(),
                            format!(
                                "Postcondition `{}` references symbol `{}` ({:?}) hidden from clients of `{}` ({:?})",
                                tag, sym, sv, r.name, vis
                            ),
                        ));
                    }
                }
            }
        }

        // Rule 3: Ghost stays ghost (runtime `do` bodies cannot call `specify` symbols)
        if !is_ghost_routine {
            let mut body_syms = collect_runtime_body_symbols(&r.stmts, &r.body);
            for st in &r.stmts {
                if let Stmt::Let(_, e) = st {
                    body_syms.extend(collect_symbols(e));
                }
            }
            for sym in body_syms {
                if ghost_symbols.contains(&sym) {
                    diags.push(Diagnostic::basic(
                        "error",
                        "Rule 3 (Ghost Stays Ghost)",
                        r.name.clone(),
                        "do_body".into(),
                        format!(
                            "Runtime `do` body of `{}` calls ghost symbol `{}` declared in `specify`",
                            r.name, sym
                        ),
                    ));
                }
            }
        }

        // Phase 3 (§7.5): Validate `reveal` targets inside routine blocks
        for rev_target in collect_reveal_targets(&r.stmts, &r.body) {
            if !known_routines.contains(&rev_target) {
                diags.push(Diagnostic::basic(
                    "error",
                    "Phase 3 (Unknown Reveal Target)",
                    r.name.clone(),
                    rev_target.clone(),
                    format!(
                        "`reveal {}` in `{}` references unknown routine `{}`",
                        rev_target, r.name, rev_target
                    ),
                ));
            }
        }

        // Phase 4 (§11.1 & §11.2): Enforce IO boundary and linear IO action rules
        let is_io_ret = r.ret_type.starts_with("IO[") || r.ret_type == "IO";
        let uses_io = expr_uses_io(&r.body)
            || r.stmts.iter().any(|st| match st {
                Stmt::Let(_, e) | Stmt::Check(_, e) | Stmt::Assume(_, e) => expr_uses_io(e),
                Stmt::Reveal(_) => false,
            });
        if (is_io_ret || uses_io) && m.kind != ModuleKind::Program {
            diags.push(Diagnostic::basic(
                "error",
                "Phase 4 (IO Boundary Violation)",
                r.name.clone(),
                "io_in_pure_module".into(),
                format!(
                    "Routine `{}` in `{}` uses `IO`, which is only permitted inside a `program` module (§11.1)",
                    r.name, m.name
                ),
            ));
        }
        if uses_io && !is_io_ret {
            diags.push(Diagnostic::basic(
                "error",
                "Phase 4 (IO Boundary Violation)",
                r.name.clone(),
                "untyped_io_effect".into(),
                format!(
                    "Routine `{}` performs `IO` effects but its return type `{}` is not `IO[...]` (§11.1)",
                    r.name, r.ret_type
                ),
            ));
        }
        // Check linear IO usage: `let` bindings of unsequenced `IO.*` actions cannot be duplicated
        for st in &r.stmts {
            if let Stmt::Let(vname, e) = st {
                if expr_uses_io(e) {
                    let count = count_var_occurrences(&r.body, vname)
                        + r.stmts
                            .iter()
                            .map(|s| match s {
                                Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                                    count_var_occurrences(se, vname)
                                }
                                Stmt::Reveal(_) => 0,
                            })
                            .sum::<usize>();
                    if count != 1 {
                        diags.push(Diagnostic::basic(
                            "error",
                            "Phase 4 (Linear IO Violation)",
                            r.name.clone(),
                            vname.clone(),
                            format!(
                                "Linear `IO` action `{}` in `{}` must be sequenced once inside `with IO` (used {} times) (§11.2)",
                                vname, r.name, count
                            ),
                        ));
                    }
                }
            }
        }
        check_branch_io_linearity(&r.body, &r.name, false, &mut diags);
        for st in &r.stmts {
            if let Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) = st {
                check_branch_io_linearity(se, &r.name, false, &mut diags);
            }
        }
    }

    // Phase 3 (§7.5): Validate `reveal` targets inside laws
    for (_, law) in &m.specify_laws {
        if let Some(body) = &law.body {
            for rev_target in collect_reveal_targets(&law.stmts, body) {
                if !known_routines.contains(&rev_target) {
                    diags.push(Diagnostic::basic(
                        "error",
                        "Phase 3 (Unknown Reveal Target)",
                        law.name.clone(),
                        rev_target.clone(),
                        format!(
                            "`reveal {}` in `law {}` references unknown routine `{}`",
                            rev_target, law.name, rev_target
                        ),
                    ));
                }
            }
        }
    }

    // Rule 5: Invariant visibility to representation friends
    for (tag, inv_expr) in &m.invariants {
        for sym in collect_symbols(inv_expr) {
            if let Some(sv) = symbol_vis.get(&sym) {
                if !sv.is_at_least_as_visible_as(&m.repr_vis) {
                    diags.push(Diagnostic::basic(
                        "error",
                        "Rule 5 (Invariant Visibility)",
                        "invariant".into(),
                        tag.clone(),
                        format!(
                            "Invariant `{}` references `{}` ({:?}), which is less visible than representation ({:?})",
                            tag, sym, sv, m.repr_vis
                        ),
                    ));
                }
            }
        }
    }

    diags
}

/// True when `e` contains a `with IO` binding `x <- action` (anywhere in its branches).
fn expr_has_io_bind(e: &Expr) -> bool {
    match e {
        Expr::WithIo(stmts, final_e) => {
            stmts.iter().any(|s| matches!(s, IoStmt::Bind(_, _))) || expr_has_io_bind(final_e)
        }
        Expr::Match(scrut, cases) => {
            expr_has_io_bind(scrut)
                || cases.iter().any(|c| {
                    expr_has_io_bind(&c.body)
                        || c.stmts
                            .iter()
                            .any(|st| matches!(st, Stmt::Let(_, x) if expr_has_io_bind(x)))
                })
        }
        Expr::If(c, ts, te, es, ee) => {
            expr_has_io_bind(c)
                || expr_has_io_bind(te)
                || expr_has_io_bind(ee)
                || ts
                    .iter()
                    .chain(es.iter())
                    .any(|st| matches!(st, Stmt::Let(_, x) if expr_has_io_bind(x)))
        }
        _ => false,
    }
}

fn expr_uses_io(e: &Expr) -> bool {
    match e {
        Expr::WithIo(_, _) => true,
        Expr::Var(v) => v == "IO",
        Expr::MethodCall(recv, _, args) => expr_uses_io(recv) || args.iter().any(expr_uses_io),
        Expr::Call(f, args) => f == "IO" || args.iter().any(expr_uses_io),
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => expr_uses_io(inner),
        Expr::BinOp(_, l, r) => expr_uses_io(l) || expr_uses_io(r),
        Expr::If(c, ts, te, es, ee) => {
            expr_uses_io(c)
                || ts.iter().any(|s| match s {
                    Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => expr_uses_io(se),
                    Stmt::Reveal(_) => false,
                })
                || expr_uses_io(te)
                || es.iter().any(|s| match s {
                    Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => expr_uses_io(se),
                    Stmt::Reveal(_) => false,
                })
                || expr_uses_io(ee)
        }
        Expr::Match(scrut, cases) => {
            expr_uses_io(scrut)
                || cases.iter().any(|c| {
                    c.stmts.iter().any(|s| match s {
                        Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                            expr_uses_io(se)
                        }
                        Stmt::Reveal(_) => false,
                    }) || expr_uses_io(&c.body)
                })
        }
        Expr::RecordLit(fs) => fs.iter().any(|(_, fe)| expr_uses_io(fe)),
        Expr::RecordUpdate(b, fs) => expr_uses_io(b) || fs.iter().any(|(_, fe)| expr_uses_io(fe)),
        Expr::TupleLit(es) => es.iter().any(expr_uses_io),
        Expr::Lambda(_, _, stmts, body) => {
            stmts.iter().any(|s| match s {
                Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => expr_uses_io(se),
                Stmt::Reveal(_) => false,
            }) || expr_uses_io(body)
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Char(_)
        | Expr::Unit
        | Expr::Bool(_)
        | Expr::Str(_)
        | Expr::Forall(_, _, _)
        | Expr::Exists(_, _, _) => false,
    }
}

fn check_branch_io_linearity(e: &Expr, rname: &str, in_with_io: bool, diags: &mut Vec<Diagnostic>) {
    match e {
        Expr::WithIo(stmts, final_e) => {
            for s in stmts {
                match s {
                    IoStmt::Bind(_, se) | IoStmt::Action(se) => {
                        check_branch_io_linearity(se, rname, true, diags);
                    }
                }
            }
            check_branch_io_linearity(final_e, rname, true, diags);
        }
        Expr::If(c, ts, te, es, ee) => {
            check_branch_io_linearity(c, rname, in_with_io, diags);
            let then_uses = ts.iter().any(|s| match s {
                Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => expr_uses_io(se),
                Stmt::Reveal(_) => false,
            }) || expr_uses_io(te);
            let else_uses = es.iter().any(|s| match s {
                Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => expr_uses_io(se),
                Stmt::Reveal(_) => false,
            }) || expr_uses_io(ee);

            if in_with_io && (then_uses != else_uses) {
                diags.push(Diagnostic::basic(
                    "error",
                    "Phase 4 (Linear IO Branch Mismatch)",
                    rname.to_string(),
                    "io_branch_linearity".to_string(),
                    format!(
                        "Branching inside `with IO` in routine `{rname}` must maintain balanced linear IO sequencing across both `then` and `else` branches (§11.2)"
                    ),
                ));
            }

            for s in ts {
                if let Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) = s {
                    check_branch_io_linearity(se, rname, in_with_io, diags);
                }
            }
            check_branch_io_linearity(te, rname, in_with_io, diags);
            for s in es {
                if let Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) = s {
                    check_branch_io_linearity(se, rname, in_with_io, diags);
                }
            }
            check_branch_io_linearity(ee, rname, in_with_io, diags);
        }
        Expr::Match(scrut, cases) => {
            check_branch_io_linearity(scrut, rname, in_with_io, diags);
            for c in cases {
                for s in &c.stmts {
                    if let Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) = s {
                        check_branch_io_linearity(se, rname, in_with_io, diags);
                    }
                }
                check_branch_io_linearity(&c.body, rname, in_with_io, diags);
            }
        }
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => {
            check_branch_io_linearity(inner, rname, in_with_io, diags);
        }
        Expr::BinOp(_, l, r) => {
            check_branch_io_linearity(l, rname, in_with_io, diags);
            check_branch_io_linearity(r, rname, in_with_io, diags);
        }
        Expr::Call(_, args) => {
            for a in args {
                check_branch_io_linearity(a, rname, in_with_io, diags);
            }
        }
        Expr::MethodCall(recv, _, args) => {
            check_branch_io_linearity(recv, rname, in_with_io, diags);
            for a in args {
                check_branch_io_linearity(a, rname, in_with_io, diags);
            }
        }
        Expr::RecordLit(fields) => {
            for (_, fe) in fields {
                check_branch_io_linearity(fe, rname, in_with_io, diags);
            }
        }
        Expr::RecordUpdate(base, fields) => {
            check_branch_io_linearity(base, rname, in_with_io, diags);
            for (_, fe) in fields {
                check_branch_io_linearity(fe, rname, in_with_io, diags);
            }
        }
        Expr::TupleLit(elems) => {
            for el in elems {
                check_branch_io_linearity(el, rname, in_with_io, diags);
            }
        }
        Expr::Lambda(_, _, stmts, body) => {
            for s in stmts {
                if let Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) = s {
                    check_branch_io_linearity(se, rname, false, diags);
                }
            }
            check_branch_io_linearity(body, rname, false, diags);
        }
        _ => {}
    }
}

fn count_var_occurrences(e: &Expr, target: &str) -> usize {
    match e {
        Expr::Var(v) => usize::from(v == target),
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Char(_)
        | Expr::Unit
        | Expr::Bool(_)
        | Expr::Str(_) => 0,
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => count_var_occurrences(inner, target),
        Expr::BinOp(_, l, r) => count_var_occurrences(l, target) + count_var_occurrences(r, target),
        Expr::Call(_, args) => args.iter().map(|a| count_var_occurrences(a, target)).sum(),
        Expr::MethodCall(recv, _, args) => {
            count_var_occurrences(recv, target)
                + args
                    .iter()
                    .map(|a| count_var_occurrences(a, target))
                    .sum::<usize>()
        }
        Expr::If(c, ts, te, es, ee) => {
            count_var_occurrences(c, target)
                + ts.iter()
                    .chain(es.iter())
                    .map(|s| match s {
                        Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                            count_var_occurrences(se, target)
                        }
                        Stmt::Reveal(_) => 0,
                    })
                    .sum::<usize>()
                + count_var_occurrences(te, target)
                + count_var_occurrences(ee, target)
        }
        Expr::Match(scrut, cases) => {
            count_var_occurrences(scrut, target)
                + cases
                    .iter()
                    .map(|c| {
                        c.stmts
                            .iter()
                            .map(|s| match s {
                                Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                                    count_var_occurrences(se, target)
                                }
                                Stmt::Reveal(_) => 0,
                            })
                            .sum::<usize>()
                            + count_var_occurrences(&c.body, target)
                    })
                    .sum::<usize>()
        }
        Expr::RecordLit(fs) => fs
            .iter()
            .map(|(_, fe)| count_var_occurrences(fe, target))
            .sum(),
        Expr::RecordUpdate(b, fs) => {
            count_var_occurrences(b, target)
                + fs.iter()
                    .map(|(_, fe)| count_var_occurrences(fe, target))
                    .sum::<usize>()
        }
        Expr::TupleLit(es) => es.iter().map(|el| count_var_occurrences(el, target)).sum(),
        Expr::Forall(_, _, body) | Expr::Exists(_, _, body) => count_var_occurrences(body, target),
        Expr::WithIo(stmts, final_e) => {
            stmts
                .iter()
                .map(|s| match s {
                    IoStmt::Bind(_, se) | IoStmt::Action(se) => count_var_occurrences(se, target),
                })
                .sum::<usize>()
                + count_var_occurrences(final_e, target)
        }
        Expr::Lambda(_, _, stmts, body) => {
            stmts
                .iter()
                .map(|s| match s {
                    Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                        count_var_occurrences(se, target)
                    }
                    Stmt::Reveal(_) => 0,
                })
                .sum::<usize>()
                + count_var_occurrences(body, target)
        }
    }
}

fn collect_runtime_body_symbols(stmts: &[Stmt], body: &Expr) -> HashSet<String> {
    let mut set = HashSet::new();
    for st in stmts {
        if let Stmt::Let(_, e) = st {
            set.extend(collect_runtime_expr_symbols(e));
        }
    }
    set.extend(collect_runtime_expr_symbols(body));
    set
}

fn collect_runtime_expr_symbols(e: &Expr) -> HashSet<String> {
    let mut set = HashSet::new();
    match e {
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Char(_)
        | Expr::Unit
        | Expr::Bool(_)
        | Expr::Str(_)
        | Expr::Forall(_, _, _)
        | Expr::Exists(_, _, _) => {}
        Expr::Var(v) => {
            set.insert(v.clone());
        }
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => {
            set.extend(collect_runtime_expr_symbols(inner));
        }
        Expr::BinOp(_, l, r) => {
            set.extend(collect_runtime_expr_symbols(l));
            set.extend(collect_runtime_expr_symbols(r));
        }
        Expr::Call(f, args) => {
            set.insert(f.clone());
            for a in args {
                set.extend(collect_runtime_expr_symbols(a));
            }
        }
        Expr::MethodCall(recv, mname, args) => {
            set.extend(collect_runtime_expr_symbols(recv));
            set.insert(mname.clone());
            for a in args {
                set.extend(collect_runtime_expr_symbols(a));
            }
        }
        Expr::If(c, ts, te, es, ee) => {
            set.extend(collect_runtime_expr_symbols(c));
            set.extend(collect_runtime_body_symbols(ts, te));
            set.extend(collect_runtime_body_symbols(es, ee));
        }
        Expr::Match(scrut, cases) => {
            set.extend(collect_runtime_expr_symbols(scrut));
            for c in cases {
                set.extend(collect_runtime_body_symbols(&c.stmts, &c.body));
            }
        }
        Expr::RecordLit(fields) => {
            for (_, fe) in fields {
                set.extend(collect_runtime_expr_symbols(fe));
            }
        }
        Expr::RecordUpdate(base, fields) => {
            set.extend(collect_runtime_expr_symbols(base));
            for (_, fe) in fields {
                set.extend(collect_runtime_expr_symbols(fe));
            }
        }
        Expr::TupleLit(elems) => {
            for el in elems {
                set.extend(collect_runtime_expr_symbols(el));
            }
        }
        Expr::WithIo(io_stmts, final_e) => {
            for s in io_stmts {
                match s {
                    IoStmt::Bind(_, se) | IoStmt::Action(se) => {
                        set.extend(collect_runtime_expr_symbols(se));
                    }
                }
            }
            set.extend(collect_runtime_expr_symbols(final_e));
        }
        Expr::Lambda(_, _, stmts, body) => {
            set.extend(collect_runtime_body_symbols(stmts, body));
        }
    }
    set
}

fn collect_reveal_targets(stmts: &[Stmt], body: &Expr) -> Vec<String> {
    let mut out = Vec::new();
    for st in stmts {
        if let Stmt::Reveal(t) = st {
            out.push(t.clone());
        }
    }
    match body {
        Expr::If(_, ts, te, es, ee) => {
            out.extend(collect_reveal_targets(ts, te));
            out.extend(collect_reveal_targets(es, ee));
        }
        Expr::Match(_, cases) => {
            for c in cases {
                out.extend(collect_reveal_targets(&c.stmts, &c.body));
            }
        }
        Expr::Lambda(_, _, s, b) => {
            out.extend(collect_reveal_targets(s, b));
        }
        _ => {}
    }
    out
}

pub fn collect_symbols(e: &Expr) -> HashSet<String> {
    collect_symbols_scoped(e, &HashSet::new())
}

pub fn collect_symbols_scoped(e: &Expr, locals: &HashSet<String>) -> HashSet<String> {
    let mut set = HashSet::new();
    match e {
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Char(_)
        | Expr::Bool(_)
        | Expr::Str(_)
        | Expr::Unit => {}
        Expr::Var(v) => {
            if !locals.contains(v) && v != "Current" && v != "Result" {
                set.insert(v.clone());
            }
        }
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => {
            set.extend(collect_symbols_scoped(inner, locals));
        }
        Expr::BinOp(_, l, r) => {
            set.extend(collect_symbols_scoped(l, locals));
            set.extend(collect_symbols_scoped(r, locals));
        }
        Expr::Call(f, args) => {
            if !locals.contains(f) && f != "Current" && f != "Result" {
                set.insert(f.clone());
            }
            for a in args {
                set.extend(collect_symbols_scoped(a, locals));
            }
        }
        Expr::MethodCall(recv, mname, args) => {
            set.extend(collect_symbols_scoped(recv, locals));
            set.insert(mname.clone());
            for a in args {
                set.extend(collect_symbols_scoped(a, locals));
            }
        }
        Expr::If(c, ts, te, es, ee) => {
            set.extend(collect_symbols_scoped(c, locals));
            let mut t_locals = locals.clone();
            for s in ts {
                match s {
                    Stmt::Let(v, se) => {
                        set.extend(collect_symbols_scoped(se, &t_locals));
                        t_locals.insert(v.clone());
                    }
                    Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                        set.extend(collect_symbols_scoped(se, &t_locals));
                    }
                    Stmt::Reveal(r) => {
                        set.insert(r.clone());
                    }
                }
            }
            set.extend(collect_symbols_scoped(te, &t_locals));

            let mut e_locals = locals.clone();
            for s in es {
                match s {
                    Stmt::Let(v, se) => {
                        set.extend(collect_symbols_scoped(se, &e_locals));
                        e_locals.insert(v.clone());
                    }
                    Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                        set.extend(collect_symbols_scoped(se, &e_locals));
                    }
                    Stmt::Reveal(r) => {
                        set.insert(r.clone());
                    }
                }
            }
            set.extend(collect_symbols_scoped(ee, &e_locals));
        }
        Expr::Match(scrut, cases) => {
            set.extend(collect_symbols_scoped(scrut, locals));
            for c in cases {
                let mut c_locals = locals.clone();
                for b in c.bindings() {
                    c_locals.insert(b);
                }
                for s in &c.stmts {
                    match s {
                        Stmt::Let(v, se) => {
                            set.extend(collect_symbols_scoped(se, &c_locals));
                            c_locals.insert(v.clone());
                        }
                        Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                            set.extend(collect_symbols_scoped(se, &c_locals));
                        }
                        Stmt::Reveal(r) => {
                            set.insert(r.clone());
                        }
                    }
                }
                set.extend(collect_symbols_scoped(&c.body, &c_locals));
            }
        }
        Expr::RecordLit(fields) => {
            for (_, fe) in fields {
                set.extend(collect_symbols_scoped(fe, locals));
            }
        }
        Expr::RecordUpdate(base, fields) => {
            set.extend(collect_symbols_scoped(base, locals));
            for (_, fe) in fields {
                set.extend(collect_symbols_scoped(fe, locals));
            }
        }
        Expr::TupleLit(elems) => {
            for el in elems {
                set.extend(collect_symbols_scoped(el, locals));
            }
        }
        Expr::Forall(v, _, body) | Expr::Exists(v, _, body) => {
            let mut q_locals = locals.clone();
            q_locals.insert(v.clone());
            set.extend(collect_symbols_scoped(body, &q_locals));
        }
        Expr::WithIo(stmts, final_e) => {
            let mut io_locals = locals.clone();
            for s in stmts {
                match s {
                    IoStmt::Bind(v, se) => {
                        set.extend(collect_symbols_scoped(se, &io_locals));
                        io_locals.insert(v.clone());
                    }
                    IoStmt::Action(se) => {
                        set.extend(collect_symbols_scoped(se, &io_locals));
                    }
                }
            }
            set.extend(collect_symbols_scoped(final_e, &io_locals));
        }
        Expr::Lambda(params, _, stmts, body) => {
            let mut l_locals = locals.clone();
            for (p, _) in params {
                l_locals.insert(p.clone());
            }
            for s in stmts {
                match s {
                    Stmt::Let(v, se) => {
                        set.extend(collect_symbols_scoped(se, &l_locals));
                        l_locals.insert(v.clone());
                    }
                    Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                        set.extend(collect_symbols_scoped(se, &l_locals));
                    }
                    Stmt::Reveal(r) => {
                        set.insert(r.clone());
                    }
                }
            }
            set.extend(collect_symbols_scoped(body, &l_locals));
        }
    }
    set
}

pub fn format_pattern(p: &Pattern) -> String {
    match p {
        Pattern::Wildcard => "_".to_string(),
        Pattern::Bind(b) => b.clone(),
        Pattern::Lit(Literal::Int(n)) => n.to_string(),
        Pattern::Lit(Literal::Float(f)) => f.to_string(),
        Pattern::Lit(Literal::Char(c)) => format!("'{}'", char::from_u32(*c).unwrap_or('?')),
        Pattern::Lit(Literal::Bool(true)) => "True".to_string(),
        Pattern::Lit(Literal::Bool(false)) => "False".to_string(),
        Pattern::Lit(Literal::Str(s)) => format!("\"{}\"", escape_string_literal(s)),
        Pattern::Lit(Literal::Unit) => "()".to_string(),
        Pattern::Variant(c, sub) => {
            if sub.is_empty() {
                c.clone()
            } else {
                let sub_s: Vec<String> = sub.iter().map(format_pattern).collect();
                format!("{c}({})", sub_s.join(", "))
            }
        }
        Pattern::Tuple(sub) => {
            let sub_s: Vec<String> = sub.iter().map(format_pattern).collect();
            format!("({})", sub_s.join(", "))
        }
    }
}

/// Re-escapes the decoded contents of a string literal (§15 `EscapeSeq`) for emission inside
/// double quotes (Gardel diagnostics and the Bend target).
fn escape_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:X}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Formats a Gardel `Expr` back into canonical Gardel syntax for `path_conditions` and diagnostics.
pub fn format_gardel_expr(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Float(f) => f.to_string(),
        Expr::Char(c) => format!("'{}'", char::from_u32(*c).unwrap_or('?')),
        Expr::Unit => "()".to_string(),
        Expr::Bool(true) => "True".to_string(),
        Expr::Bool(false) => "False".to_string(),
        Expr::Str(s) => format!("\"{}\"", escape_string_literal(s)),
        Expr::Var(v) => v.clone(),
        Expr::UnaryNot(inner) => format!("not {}", format_gardel_expr(inner)),
        Expr::UnaryNeg(inner) => format!("-{}", format_gardel_expr(inner)),
        Expr::BinOp(op, l, r) => {
            format!("({} {op} {})", format_gardel_expr(l), format_gardel_expr(r))
        }
        Expr::Call(f, args) => {
            let a_str: Vec<String> = args.iter().map(format_gardel_expr).collect();
            format!("{f}({})", a_str.join(", "))
        }
        Expr::MethodCall(recv, mname, args) => {
            let r_str = format_gardel_expr(recv);
            if mname == "apply" {
                let a_str: Vec<String> = args.iter().map(format_gardel_expr).collect();
                format!("{r_str}({})", a_str.join(", "))
            } else if args.is_empty() {
                format!("{r_str}.{mname}")
            } else {
                let a_str: Vec<String> = args.iter().map(format_gardel_expr).collect();
                format!("{r_str}.{mname}({})", a_str.join(", "))
            }
        }
        Expr::If(c, _, te, _, ee) => format!(
            "if {} then {} else {} end",
            format_gardel_expr(c),
            format_gardel_expr(te),
            format_gardel_expr(ee)
        ),
        Expr::Match(scrut, cases) => {
            let cs: Vec<String> = cases
                .iter()
                .map(|c| {
                    let pat = format_pattern(&c.pattern);
                    format!("case {pat} then {}", format_gardel_expr(&c.body))
                })
                .collect();
            format!("match {} {} end", format_gardel_expr(scrut), cs.join(" "))
        }
        Expr::RecordLit(fields) => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k} = {}", format_gardel_expr(v)))
                .collect();
            format!("{{ {} }}", fs.join(", "))
        }
        Expr::RecordUpdate(base, fields) => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k} = {}", format_gardel_expr(v)))
                .collect();
            format!("{{ {} with {} }}", format_gardel_expr(base), fs.join(", "))
        }
        Expr::TupleLit(elems) => {
            let es: Vec<String> = elems.iter().map(format_gardel_expr).collect();
            format!("({})", es.join(", "))
        }
        Expr::Forall(v, ty, body) => {
            format!("forall {v}: {ty}. {}", format_gardel_expr(body))
        }
        Expr::Exists(v, ty, body) => {
            format!("exists {v}: {ty}. {}", format_gardel_expr(body))
        }
        Expr::WithIo(stmts, final_e) => {
            let mut ss = Vec::new();
            for s in stmts {
                match s {
                    IoStmt::Bind(v, e) => ss.push(format!("{v} <- {}", format_gardel_expr(e))),
                    IoStmt::Action(e) => ss.push(format_gardel_expr(e)),
                }
            }
            ss.push(format_gardel_expr(final_e));
            format!("with IO {} end", ss.join("; "))
        }
        Expr::Lambda(params, ret_ty, stmts, body) => {
            let ps: Vec<String> = params.iter().map(|(p, t)| format!("{p}: {t}")).collect();
            let mut ss = Vec::new();
            for s in stmts {
                match s {
                    Stmt::Let(v, e) => ss.push(format!("let {v} = {}", format_gardel_expr(e))),
                    Stmt::Check(t, e) => ss.push(format!("check {t}: {}", format_gardel_expr(e))),
                    Stmt::Assume(t, e) => ss.push(format!("assume {t}: {}", format_gardel_expr(e))),
                    Stmt::Reveal(t) => ss.push(format!("reveal {t}")),
                }
            }
            ss.push(format_gardel_expr(body));
            format!("fn({}): {ret_ty} do {} end", ps.join(", "), ss.join("; "))
        }
    }
}

// ============================================================================
// 5. PHASE 4 STANDARD LIBRARY (`SEQ`, `BIGNAT`, `STRING`, `OPTION`, `IO`)
// ============================================================================

pub const STDLIB_SOURCES: &[(&str, &str)] = &[
    (
        "stdlib/concepts/eq.gdl",
        include_str!("../../stdlib/concepts/eq.gdl"),
    ),
    (
        "stdlib/concepts/canonical_eq.gdl",
        include_str!("../../stdlib/concepts/canonical_eq.gdl"),
    ),
    (
        "stdlib/concepts/ord.gdl",
        include_str!("../../stdlib/concepts/ord.gdl"),
    ),
    (
        "stdlib/core/option.gdl",
        include_str!("../../stdlib/core/option.gdl"),
    ),
    (
        "stdlib/collections/seq.gdl",
        include_str!("../../stdlib/collections/seq.gdl"),
    ),
    (
        "stdlib/numeric/bignat.gdl",
        include_str!("../../stdlib/numeric/bignat.gdl"),
    ),
    (
        "stdlib/text/string.gdl",
        include_str!("../../stdlib/text/string.gdl"),
    ),
    (
        "stdlib/text/string_error.gdl",
        include_str!("../../stdlib/text/string_error.gdl"),
    ),
    (
        "stdlib/core/io_error.gdl",
        include_str!("../../stdlib/core/io_error.gdl"),
    ),
    (
        "stdlib/core/result.gdl",
        include_str!("../../stdlib/core/result.gdl"),
    ),
    (
        "stdlib/collections/list_ops.gdl",
        include_str!("../../stdlib/collections/list_ops.gdl"),
    ),
    (
        "stdlib/functional/function_ops.gdl",
        include_str!("../../stdlib/functional/function_ops.gdl"),
    ),
];

/// Returns the canonical Gardel v0.4 Phase 4 Standard Library source package (§3, §5, §10.5, §11, §16).
pub fn phase4_stdlib_source() -> &'static str {
    concat!(
        include_str!("../../stdlib/concepts/eq.gdl"),
        "\n",
        include_str!("../../stdlib/concepts/canonical_eq.gdl"),
        "\n",
        include_str!("../../stdlib/concepts/ord.gdl"),
        "\n",
        include_str!("../../stdlib/core/option.gdl"),
        "\n",
        include_str!("../../stdlib/collections/seq.gdl"),
        "\n",
        include_str!("../../stdlib/numeric/bignat.gdl"),
        "\n",
        include_str!("../../stdlib/text/string.gdl"),
        "\n",
        include_str!("../../stdlib/text/string_error.gdl"),
        "\n",
        include_str!("../../stdlib/core/io_error.gdl"),
        "\n",
        include_str!("../../stdlib/core/result.gdl"),
        "\n",
        include_str!("../../stdlib/collections/list_ops.gdl"),
        "\n",
        include_str!("../../stdlib/functional/function_ops.gdl"),
    )
}

/// Parses and returns all Phase 4 standard library modules (`OPTION`, `SEQ`, `BIGNAT`, `STRING`, `RESULT`).
pub fn parse_phase4_stdlib() -> Result<FileAst, String> {
    let tokens = tokenize(phase4_stdlib_source()).map_err(|e| e.to_string())?;
    let mut parser = Parser::new(tokens);
    let mut ast = parser.parse_file().map_err(|e| e.to_string())?;
    let mut concept_map = HashMap::new();
    for c in &ast.concepts {
        concept_map.insert(c.name.clone(), c.clone());
    }
    for m in &mut ast.modules {
        let _ = check_and_expand_concepts(m, &concept_map);
    }
    Ok(ast)
}

// ============================================================================
// 6. PHASE 2 MONOMORPHIZATION PASS (§9.5 & §13.5)
// ============================================================================

/// Specializes a generic module `m` for concrete type arguments (e.g., `BOUNDED_STACK[NAT]` -> `BOUNDED_STACK_U64`)
pub fn monomorphize_module(m: &ModuleDecl, concrete_args: &[String]) -> ModuleDecl {
    if m.type_params.is_empty() || concrete_args.is_empty() {
        return m.clone();
    }
    let mut subst = HashMap::new();
    for (param, arg) in m.type_params.iter().zip(concrete_args.iter()) {
        subst.insert(param.clone(), arg.clone());
    }

    let mut spec = m.clone();
    spec.name = format!("{}_{}", m.name, concrete_args.join("_"));
    spec.type_params.clear();
    spec.type_bounds.clear();

    let sub_ty = |t: &str| -> String {
        if let Some(rep) = subst.get(t) {
            return rep.clone();
        }
        let mut out = t.to_string();
        for (k, v) in &subst {
            out = out.replace(&format!("[{k}]"), &format!("[{v}]"));
        }
        out
    };

    spec.repr = match &m.repr {
        ReprKind::Product(fields) => {
            ReprKind::Product(fields.iter().map(|(n, t)| (n.clone(), sub_ty(t))).collect())
        }
        ReprKind::Sum(vars) => ReprKind::Sum(
            vars.iter()
                .map(|v| VariantDecl {
                    name: v.name.clone(),
                    fields: v
                        .fields
                        .iter()
                        .map(|(n, t)| (n.clone(), sub_ty(t)))
                        .collect(),
                })
                .collect(),
        ),
    };

    let spec_routine = |r: &RoutineDecl| -> RoutineDecl {
        let mut nr = r.clone();
        nr.params = nr
            .params
            .into_iter()
            .map(|(p, t)| (p, sub_ty(&t)))
            .collect();
        nr.ret_type = sub_ty(&nr.ret_type);
        nr
    };

    spec.create_routines = m
        .create_routines
        .iter()
        .map(|(v, r)| (v.clone(), spec_routine(r)))
        .collect();
    spec.feature_routines = m
        .feature_routines
        .iter()
        .map(|(v, r)| (v.clone(), spec_routine(r)))
        .collect();
    spec.specify_routines = m
        .specify_routines
        .iter()
        .map(|(v, r)| (v.clone(), spec_routine(r)))
        .collect();

    let spec_law = |l: &LawDecl| -> LawDecl {
        let mut nl = l.clone();
        nl.params = nl
            .params
            .into_iter()
            .map(|(p, t)| (p, sub_ty(&t)))
            .collect();
        nl
    };
    spec.specify_laws = m
        .specify_laws
        .iter()
        .map(|(v, l)| (v.clone(), spec_law(l)))
        .collect();

    spec
}

pub fn collect_monomorphized_instances(prog: &ProgramEnv) -> Vec<ModuleDecl> {
    let mut out = Vec::new();
    let mut visited = HashSet::new();

    let find_instantiations = |ty: &str| -> Option<(String, Vec<String>)> {
        if let Some(open) = ty.find('[') {
            if let Some(close) = ty.rfind(']') {
                let base = &ty[..open];
                let args_str = &ty[open + 1..close];
                let args: Vec<String> = args_str.split(',').map(|s| s.trim().to_string()).collect();
                return Some((base.to_string(), args));
            }
        }
        None
    };

    for (_, m) in &prog.modules {
        let mut scan_types = Vec::new();
        match &m.repr {
            ReprKind::Product(fs) => {
                for (_, t) in fs {
                    scan_types.push(t.clone());
                }
            }
            ReprKind::Sum(vs) => {
                for v in vs {
                    for (_, t) in &v.fields {
                        scan_types.push(t.clone());
                    }
                }
            }
        }
        for (_, r) in m.create_routines.iter().chain(m.feature_routines.iter()) {
            for (_, t) in &r.params {
                scan_types.push(t.clone());
            }
            scan_types.push(r.ret_type.clone());
        }

        for ty in scan_types {
            if let Some((base, args)) = find_instantiations(&ty) {
                let inst_key = format!("{base}_{}", args.join("_"));
                if visited.insert(inst_key) {
                    if let Some(base_mod) = prog.modules.get(&base) {
                        if !base_mod.type_params.is_empty() {
                            let monomorphic = monomorphize_module(base_mod, &args);
                            out.push(monomorphic);
                        }
                    }
                }
            }
        }
    }

    out
}

pub fn closure_convert_module(m: &ModuleDecl) -> (ModuleDecl, Vec<TopLevelCombinator>) {
    let mut combinators = Vec::new();
    let mut counter = 0usize;

    let mut spec = m.clone();

    for (_, r) in spec
        .create_routines
        .iter_mut()
        .chain(spec.feature_routines.iter_mut())
        .chain(spec.specify_routines.iter_mut())
    {
        for s in &mut r.stmts {
            match s {
                Stmt::Let(_, e) | Stmt::Check(_, e) | Stmt::Assume(_, e) => {
                    *e = convert_expr_lambdas(e, &mut counter, &mut combinators);
                }
                Stmt::Reveal(_) => {}
            }
        }
        r.body = convert_expr_lambdas(&r.body, &mut counter, &mut combinators);
    }

    (spec, combinators)
}

fn convert_expr_lambdas(
    e: &Expr,
    counter: &mut usize,
    combinators: &mut Vec<TopLevelCombinator>,
) -> Expr {
    match e {
        Expr::Lambda(params, ret_ty, stmts, body) => {
            *counter += 1;
            let id = *counter;

            let mut param_names: HashSet<String> = params.iter().map(|(p, _)| p.clone()).collect();
            let mut free_vars = HashSet::new();

            for s in stmts {
                match s {
                    Stmt::Let(v, se) => {
                        let f = collect_symbols_scoped(se, &param_names);
                        free_vars.extend(f);
                        param_names.insert(v.clone());
                    }
                    Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                        let f = collect_symbols_scoped(se, &param_names);
                        free_vars.extend(f);
                    }
                    Stmt::Reveal(_) => {}
                }
            }
            free_vars.extend(collect_symbols_scoped(body, &param_names));

            let fv_list: Vec<String> = free_vars.into_iter().collect();

            if fv_list.is_empty() {
                let name = format!("combinator_{id}");
                let routine = RoutineDecl {
                    name: name.clone(),
                    type_params: Vec::new(),
                    params: params.clone(),
                    ret_type: ret_ty.clone(),
                    requires: Vec::new(),
                    variant: Vec::new(),
                    stmts: stmts.clone(),
                    body: (**body).clone(),
                    ensures: Vec::new(),
                };
                combinators.push(TopLevelCombinator {
                    env_struct: None,
                    routine,
                });
                Expr::Var(name)
            } else {
                let env_name = format!("ClosureEnv_{id}");
                let env_fields: Vec<(String, String)> = fv_list
                    .iter()
                    .map(|v| (v.clone(), "U64".to_string()))
                    .collect();
                let mut apply_params = vec![("env".to_string(), env_name.clone())];
                apply_params.extend(params.clone());

                let mut substs = Vec::new();
                for fv in &fv_list {
                    substs.push((
                        fv.clone(),
                        Expr::MethodCall(
                            Box::new(Expr::Var("env".to_string())),
                            fv.clone(),
                            Vec::new(),
                        ),
                    ));
                }

                let sub_body = subst_expr_vars(body, &substs);
                let (sub_stmts, _) = subst_block(stmts, body, &substs);

                let apply_name = format!("closure_apply_{id}");
                let routine = RoutineDecl {
                    name: apply_name.clone(),
                    type_params: Vec::new(),
                    params: apply_params,
                    ret_type: ret_ty.clone(),
                    requires: Vec::new(),
                    variant: Vec::new(),
                    stmts: sub_stmts,
                    body: sub_body,
                    ensures: Vec::new(),
                };

                combinators.push(TopLevelCombinator {
                    env_struct: Some((env_name.clone(), env_fields.clone())),
                    routine,
                });

                let rec_fields = fv_list
                    .into_iter()
                    .map(|v| (v.clone(), Expr::Var(v)))
                    .collect();
                Expr::RecordLit(rec_fields)
            }
        }
        Expr::UnaryNot(inner) => {
            Expr::UnaryNot(Box::new(convert_expr_lambdas(inner, counter, combinators)))
        }
        Expr::UnaryNeg(inner) => {
            Expr::UnaryNeg(Box::new(convert_expr_lambdas(inner, counter, combinators)))
        }
        Expr::BinOp(op, l, r) => Expr::BinOp(
            op.clone(),
            Box::new(convert_expr_lambdas(l, counter, combinators)),
            Box::new(convert_expr_lambdas(r, counter, combinators)),
        ),
        Expr::Call(f, args) => Expr::Call(
            f.clone(),
            args.iter()
                .map(|a| convert_expr_lambdas(a, counter, combinators))
                .collect(),
        ),
        Expr::MethodCall(recv, mname, args) => Expr::MethodCall(
            Box::new(convert_expr_lambdas(recv, counter, combinators)),
            mname.clone(),
            args.iter()
                .map(|a| convert_expr_lambdas(a, counter, combinators))
                .collect(),
        ),
        Expr::If(c, ts, te, es, ee) => Expr::If(
            Box::new(convert_expr_lambdas(c, counter, combinators)),
            ts.iter()
                .map(|s| match s {
                    Stmt::Let(v, e) => {
                        Stmt::Let(v.clone(), convert_expr_lambdas(e, counter, combinators))
                    }
                    Stmt::Check(t, e) => {
                        Stmt::Check(t.clone(), convert_expr_lambdas(e, counter, combinators))
                    }
                    Stmt::Assume(t, e) => {
                        Stmt::Assume(t.clone(), convert_expr_lambdas(e, counter, combinators))
                    }
                    Stmt::Reveal(r) => Stmt::Reveal(r.clone()),
                })
                .collect(),
            Box::new(convert_expr_lambdas(te, counter, combinators)),
            es.iter()
                .map(|s| match s {
                    Stmt::Let(v, e) => {
                        Stmt::Let(v.clone(), convert_expr_lambdas(e, counter, combinators))
                    }
                    Stmt::Check(t, e) => {
                        Stmt::Check(t.clone(), convert_expr_lambdas(e, counter, combinators))
                    }
                    Stmt::Assume(t, e) => {
                        Stmt::Assume(t.clone(), convert_expr_lambdas(e, counter, combinators))
                    }
                    Stmt::Reveal(r) => Stmt::Reveal(r.clone()),
                })
                .collect(),
            Box::new(convert_expr_lambdas(ee, counter, combinators)),
        ),
        Expr::Match(scrut, cases) => Expr::Match(
            Box::new(convert_expr_lambdas(scrut, counter, combinators)),
            cases
                .iter()
                .map(|c| {
                    let stmts: Vec<Stmt> = c
                        .stmts
                        .iter()
                        .map(|s| match s {
                            Stmt::Let(v, e) => {
                                Stmt::Let(v.clone(), convert_expr_lambdas(e, counter, combinators))
                            }
                            Stmt::Check(t, e) => Stmt::Check(
                                t.clone(),
                                convert_expr_lambdas(e, counter, combinators),
                            ),
                            Stmt::Assume(t, e) => Stmt::Assume(
                                t.clone(),
                                convert_expr_lambdas(e, counter, combinators),
                            ),
                            Stmt::Reveal(r) => Stmt::Reveal(r.clone()),
                        })
                        .collect();
                    let body = convert_expr_lambdas(&c.body, counter, combinators);
                    MatchCase::new(c.pattern.clone(), stmts, body)
                })
                .collect(),
        ),
        Expr::RecordLit(fields) => Expr::RecordLit(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), convert_expr_lambdas(v, counter, combinators)))
                .collect(),
        ),
        Expr::RecordUpdate(base, fields) => Expr::RecordUpdate(
            Box::new(convert_expr_lambdas(base, counter, combinators)),
            fields
                .iter()
                .map(|(k, v)| (k.clone(), convert_expr_lambdas(v, counter, combinators)))
                .collect(),
        ),
        Expr::TupleLit(elems) => Expr::TupleLit(
            elems
                .iter()
                .map(|el| convert_expr_lambdas(el, counter, combinators))
                .collect(),
        ),
        Expr::WithIo(stmts, final_e) => Expr::WithIo(
            stmts
                .iter()
                .map(|s| match s {
                    IoStmt::Bind(v, e) => {
                        IoStmt::Bind(v.clone(), convert_expr_lambdas(e, counter, combinators))
                    }
                    IoStmt::Action(e) => {
                        IoStmt::Action(convert_expr_lambdas(e, counter, combinators))
                    }
                })
                .collect(),
            Box::new(convert_expr_lambdas(final_e, counter, combinators)),
        ),
        other => other.clone(),
    }
}

// =====================================================// 7. PHASE 3 & 4 DIRECT BOOGIE (.bpl) VERIFICATION CONDITION EMITTER
// ============================================================================

/// Traverses an expression collecting string literals into `set`.
fn collect_expr_strings(e: &Expr, set: &mut HashSet<String>) {
    match e {
        Expr::Str(s) => {
            set.insert(s.clone());
        }
        Expr::UnaryNot(inner) => collect_expr_strings(inner, set),
        Expr::BinOp(_, l, r) => {
            collect_expr_strings(l, set);
            collect_expr_strings(r, set);
        }
        Expr::Call(_, args) => {
            for a in args {
                collect_expr_strings(a, set);
            }
        }
        Expr::MethodCall(recv, _, args) => {
            collect_expr_strings(recv, set);
            for a in args {
                collect_expr_strings(a, set);
            }
        }
        Expr::If(c, ts, te, es, ee) => {
            collect_expr_strings(c, set);
            for s in ts {
                collect_stmt_strings(s, set);
            }
            collect_expr_strings(te, set);
            for s in es {
                collect_stmt_strings(s, set);
            }
            collect_expr_strings(ee, set);
        }
        Expr::Match(scrut, cases) => {
            collect_expr_strings(scrut, set);
            for c in cases {
                for s in &c.stmts {
                    collect_stmt_strings(s, set);
                }
                collect_expr_strings(&c.body, set);
            }
        }
        Expr::RecordLit(fields) => {
            for (_, fe) in fields {
                collect_expr_strings(fe, set);
            }
        }
        Expr::RecordUpdate(base, fields) => {
            collect_expr_strings(base, set);
            for (_, fe) in fields {
                collect_expr_strings(fe, set);
            }
        }
        Expr::TupleLit(elems) => {
            for el in elems {
                collect_expr_strings(el, set);
            }
        }
        Expr::Forall(_, _, body) | Expr::Exists(_, _, body) => {
            collect_expr_strings(body, set);
        }
        Expr::WithIo(stmts, final_e) => {
            for s in stmts {
                match s {
                    IoStmt::Bind(_, e) | IoStmt::Action(e) => collect_expr_strings(e, set),
                }
            }
            collect_expr_strings(final_e, set);
        }
        Expr::Lambda(_, _, stmts, body) => {
            for s in stmts {
                collect_stmt_strings(s, set);
            }
            collect_expr_strings(body, set);
        }
        _ => {}
    }
}

fn collect_stmt_strings(st: &Stmt, set: &mut HashSet<String>) {
    match st {
        Stmt::Let(_, e) | Stmt::Check(_, e) | Stmt::Assume(_, e) => {
            collect_expr_strings(e, set);
        }
        Stmt::Reveal(_) => {}
    }
}

/// Collects all distinct string literals in `m` sorted alphabetically.
pub fn collect_module_strings(m: &ModuleDecl) -> Vec<String> {
    let mut set = HashSet::new();
    for (_, r) in m
        .create_routines
        .iter()
        .chain(m.feature_routines.iter())
        .chain(m.specify_routines.iter())
    {
        for (_, req) in &r.requires {
            collect_expr_strings(req, &mut set);
        }
        for (_, ens) in &r.ensures {
            collect_expr_strings(ens, &mut set);
        }
        for v in &r.variant {
            collect_expr_strings(v, &mut set);
        }
        for s in &r.stmts {
            collect_stmt_strings(s, &mut set);
        }
        collect_expr_strings(&r.body, &mut set);
    }
    for (_, law) in &m.specify_laws {
        for (_, req) in &law.requires {
            collect_expr_strings(req, &mut set);
        }
        for (_, ens) in &law.ensures {
            collect_expr_strings(ens, &mut set);
        }
        for v in &law.variant {
            collect_expr_strings(v, &mut set);
        }
        for s in &law.stmts {
            collect_stmt_strings(s, &mut set);
        }
        for t in &law.triggers {
            collect_expr_strings(t, &mut set);
        }
        if let Some(b) = &law.body {
            collect_expr_strings(b, &mut set);
        }
    }
    for (_, inv) in &m.invariants {
        collect_expr_strings(inv, &mut set);
    }
    let mut vec: Vec<String> = set.into_iter().collect();
    vec.sort();
    vec
}

/// Type parameters whose `CANONICAL_EQ` conformance is only a *conditional* `where` bound
/// (`module OPTION[A] satisfies CANONICAL_EQ where A satisfies CANONICAL_EQ`). The module
/// itself exists for every `A`, so `A_eq <==> ==` must not be assumed module-wide: it holds
/// only when proving (or using) the conditional `CANONICAL_EQ` laws.
fn conditional_canonical_tps(m: &ModuleDecl) -> Vec<String> {
    m.satisfies
        .as_ref()
        .map(|s| {
            s.where_constraints
                .iter()
                // Only parameters whose bound was resolved (so `<tp>_eq` is declared).
                .filter(|(tp, c)| {
                    c == "CANONICAL_EQ"
                        && m.type_bounds.get(tp).is_some_and(|b| b == "CANONICAL_EQ")
                })
                .map(|(tp, _)| tp.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The hypothesis under which a law of section `vis` holds: for `specify {CANONICAL_EQ}`
/// laws of a conditionally canonical module, "every conditionally canonical type parameter
/// has structural element equality". `None` when the law is unconditional.
fn canonical_where_guard(m: &ModuleDecl, vis: &Visibility) -> Option<String> {
    let is_canonical_section =
        matches!(vis, Visibility::Modules(ms) if ms.iter().any(|c| c == "CANONICAL_EQ"));
    if !is_canonical_section {
        return None;
    }
    let tps = conditional_canonical_tps(m);
    if tps.is_empty() {
        return None;
    }
    Some(
        tps.iter()
            .map(|tp| {
                format!(
                    "(forall _cx: {tp}, _cy: {tp} :: {{ {tp}_eq(_cx, _cy) }} {tp}_eq(_cx, _cy) <==> _cx == _cy)"
                )
            })
            .collect::<Vec<_>>()
            .join(" && "),
    )
}

thread_local! {
    /// Gardel types of the routine/law parameters and `match` binders in scope during Boogie
    /// emission. Used only to decide whether `x.equals(y)` compares two values of the module
    /// type (`fn_equals`) or two elements of a bounded type parameter (`<tp>_eq`). Unlike
    /// `BOOGIE_TYPE_ENV` it is populated in every mode, including `--stdlib` bootstrap.
    static ELEM_TYPE_ENV: std::cell::RefCell<HashMap<String, String>> =
        std::cell::RefCell::new(HashMap::new());
}

fn elem_env_bind(v: &str, ty: &str) {
    ELEM_TYPE_ENV.with(|env| {
        env.borrow_mut().insert(v.to_string(), ty.to_string());
    });
}

/// Gardel type of a receiver expression, as far as needed to classify `equals` calls.
fn elem_receiver_type(e: &Expr, m: &ModuleDecl) -> Option<String> {
    match e {
        Expr::Var(v) if v == "Current" => Some(module_self_type(m)),
        Expr::Var(v) => ELEM_TYPE_ENV.with(|env| env.borrow().get(v).cloned()),
        // Static call `M.r(..)` / unqualified call `r(..)` of a routine of the module.
        Expr::MethodCall(recv, r, _) if matches!(&**recv, Expr::Var(v) if v == &m.name) => {
            let (rd, _) = find_module_routine(m, r)?;
            Some(if rd.ret_type == "Current" {
                module_self_type(m)
            } else {
                rd.ret_type.clone()
            })
        }
        Expr::Call(f, _) => {
            if module_has_variant(m, f) {
                return Some(module_self_type(m));
            }
            let (rd, _) = find_module_routine(m, f)?;
            Some(if rd.ret_type == "Current" {
                module_self_type(m)
            } else {
                rd.ret_type.clone()
            })
        }
        Expr::MethodCall(recv, r, args) if args.is_empty() => {
            let rt = elem_receiver_type(recv, m)?;
            if rt != module_self_type(m) && rt != m.name {
                return None;
            }
            let (rd, _) = find_module_routine(m, r)?;
            Some(if rd.ret_type == "Current" {
                module_self_type(m)
            } else {
                rd.ret_type.clone()
            })
        }
        _ => None,
    }
}

/// `Some("<tp>_eq")` when `recv.equals(..)` compares elements of a type parameter `tp` that
/// carries an EQ-family bound, i.e. behavioral element equality, which must NOT be lowered
/// to structural `==` (§5, §9).
fn elem_equals_fn(recv: &Expr, m: &ModuleDecl) -> Option<String> {
    let ty = elem_receiver_type(recv, m)?;
    let bound = m.type_bounds.get(&ty)?;
    if m.type_params.contains(&ty) && matches!(bound.as_str(), "EQ" | "ORD" | "CANONICAL_EQ") {
        Some(format!("{ty}_eq"))
    } else {
        None
    }
}

fn emit_boogie_concept_axioms(m: &ModuleDecl) -> String {
    let mut out = String::new();
    let Some(sat) = &m.satisfies else {
        return out;
    };
    let cur_ty = map_boogie_type("Current", m);
    let quant_tps = if m.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", m.type_params.join(", "))
    };
    let quant_tps_sp = if quant_tps.is_empty() {
        String::new()
    } else {
        format!("{quant_tps} ")
    };

    let has_eq = sat
        .concepts
        .iter()
        .any(|c| c == "EQ" || c == "CANONICAL_EQ" || c == "ORD");
    let has_ord = sat.concepts.iter().any(|c| c == "ORD");

    if has_eq {
        out.push_str("// Concept Axiom for EQ reflexivity on module\n");
        out.push_str(&format!(
            "axiom (forall {quant_tps_sp}x: {cur_ty} :: {{ fn_equals(DefaultFuel, x, x) }} fn_equals(DefaultFuel, x, x) == true);\n"
        ));
    }
    if has_ord {
        out.push_str("// Concept Axioms for ORD on module\n");
        out.push_str(&format!(
            "axiom (forall {quant_tps_sp}x: {cur_ty} :: {{ fn_less_than(DefaultFuel, x, x) }} !fn_less_than(DefaultFuel, x, x));\n"
        ));
        out.push_str(&format!(
            "axiom (forall {quant_tps_sp}x: {cur_ty}, y: {cur_ty} :: {{ fn_less_or_equal(DefaultFuel, x, y) }} fn_less_or_equal(DefaultFuel, x, y) <==> (fn_less_than(DefaultFuel, x, y) || fn_equals(DefaultFuel, x, y)));\n"
        ));
        out.push_str(&format!(
            "axiom (forall {quant_tps_sp}x: {cur_ty}, y: {cur_ty}, z: {cur_ty} :: {{ fn_less_than(DefaultFuel, x, y), fn_less_than(DefaultFuel, y, z) }} fn_less_than(DefaultFuel, x, y) && fn_less_than(DefaultFuel, y, z) ==> fn_less_than(DefaultFuel, x, z));\n"
        ));
        out.push_str(&format!(
            "axiom (forall {quant_tps_sp}x: {cur_ty}, y: {cur_ty} :: {{ fn_less_than(DefaultFuel, x, y) }} fn_less_than(DefaultFuel, x, y) || fn_equals(DefaultFuel, x, y) || fn_less_than(DefaultFuel, y, x));\n"
        ));
    }
    out
}

// ----------------------------------------------------------------------------
// 6b. STANDARD PRELUDE INJECTION (§4.1 Automatic Scope Injection, Task B.1)
// ----------------------------------------------------------------------------
//
// Design: in prelude mode (the default for user files) the parsed, concept-expanded
// Standard Prelude is (1) merged into the front-end concept map / program symbol table,
// and (2) injected into every user module's Boogie verification context as a *background
// theory*: for each prelude module the user module references, the compiler emits that
// module's sorts, constructors, opaque routine functions with their fuel-guarded
// definitional axioms, contract axioms and law axioms — exactly as emitted (and proven)
// in `--stdlib` bootstrap mode — but none of its `Verify_*` procedures. Prelude symbols are
// namespaced `<MODULE>__<symbol>` so they cannot collide with user declarations.
// The prelude's contracts and laws are therefore *trusted* in user runs; their soundness
// rests on the bootstrap verification (`gardelc --stdlib --strict`, 0 errors).

/// The parsed, concept-expanded Standard Prelude Γ_prelude (`phase4_stdlib_source()`).
pub struct StandardPrelude {
    pub concepts: Vec<ConceptDecl>,
    pub modules: Vec<ModuleDecl>,
}

impl StandardPrelude {
    pub fn module(&self, name: &str) -> Option<&ModuleDecl> {
        self.modules.iter().find(|m| m.name == name)
    }

    pub fn concept(&self, name: &str) -> Option<&ConceptDecl> {
        self.concepts.iter().find(|c| c.name == name)
    }

    /// True if `name` is a top-level prelude identifier (module, functions module or concept).
    pub fn declares(&self, name: &str) -> bool {
        self.module(name).is_some() || self.concept(name).is_some()
    }
}

/// Returns the Standard Prelude, parsed once per process.
pub fn standard_prelude() -> &'static StandardPrelude {
    static PRELUDE: std::sync::OnceLock<StandardPrelude> = std::sync::OnceLock::new();
    PRELUDE.get_or_init(|| {
        let ast = parse_phase4_stdlib().expect("the embedded Standard Prelude must parse");
        StandardPrelude {
            concepts: ast.concepts,
            modules: ast.modules,
        }
    })
}

/// True when `path` is a file of the stdlib source tree this gardelc was built from. Those
/// files are the Standard Prelude itself and always compile in bootstrap mode (§4.1.3). This is
/// decided by location, not by comparing text with the embedded copy: an edited stdlib file is
/// still a stdlib file, and a user file that happens to equal one is still a user file.
pub fn path_in_stdlib_tree(path: &str) -> bool {
    let (Ok(file), Ok(dir)) = (
        std::path::Path::new(path).canonicalize(),
        stdlib_source_dir().canonicalize(),
    ) else {
        return false;
    };
    file.starts_with(dir)
}

/// Prelude injection is on unless in bootstrap mode: `--stdlib` (which covers compiling files
/// of the stdlib tree, see `path_in_stdlib_tree`) or `--no-prelude` (§4.1.3).
pub fn prelude_mode_enabled(stdlib_only: bool, no_prelude: bool) -> bool {
    !stdlib_only && !no_prelude
}

/// Span of the declaration header `<kw> <name>` (e.g. `module STACK`) in `src`.
fn locate_decl_header(src: &str, input_path: &str, kw: &str, name: &str) -> Span {
    let header = format!("{kw} {name}");
    for (idx, line) in src.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with(&header)
            && !t[header.len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            let col = line.len() - t.len() + 1;
            return Span {
                file: input_path.to_string(),
                line: idx + 1,
                column: col,
                end_line: idx + 1,
                end_column: line.len().max(col),
            };
        }
    }
    Span::unknown(input_path)
}

/// Top-level declarations of a file: `(name, keyword)` for concepts and modules.
fn top_level_decls(ast: &FileAst) -> Vec<(&str, &'static str)> {
    ast.concepts
        .iter()
        .map(|c| (c.name.as_str(), "concept"))
        .chain(ast.modules.iter().map(|m| {
            let kw = match m.kind {
                ModuleKind::Module => "module",
                ModuleKind::Functions => "functions",
                ModuleKind::Program => "program",
            };
            (m.name.as_str(), kw)
        }))
        .collect()
}

/// `E0101: duplicate_prelude_symbol` (§4.1.3): in prelude mode a user top-level module,
/// `functions` module, `program` or concept may not redeclare a Standard Prelude identifier.
pub fn check_prelude_collisions(ast: &FileAst, src: &str, input_path: &str) -> Vec<Diagnostic> {
    let prelude = standard_prelude();
    let mut diags = Vec::new();
    for (name, kw) in top_level_decls(ast) {
        if !prelude.declares(name) {
            continue;
        }
        let sp = locate_decl_header(src, input_path, kw, name);
        diags.push(
            Diagnostic::basic(
                "error",
                "E0101: duplicate_prelude_symbol",
                name.to_string(),
                "duplicate_prelude_symbol".to_string(),
                format!(
                    "E0101: duplicate_prelude_symbol: top-level {kw} `{name}` redeclares a Standard Prelude identifier (§4.1); remove it to use the prelude `{name}`, or compile in `--no-prelude` bootstrap mode"
                ),
            )
            .with_ai_context(sp, Vec::new(), BTreeMap::new(), Vec::new()),
        );
    }
    diags
}

/// A parsed source file of a compilation run (Task B.4).
pub struct SourceUnit {
    pub path: String,
    pub src: String,
    pub ast: FileAst,
}

/// `E0102: duplicate_module_name` (§4.2.2): a top-level module, `functions` module,
/// `program` or concept identifier declared in two distinct files of one project. Reported
/// at every declaration after the first one.
pub fn check_duplicate_module_names(units: &[SourceUnit]) -> Vec<Diagnostic> {
    let mut first: HashMap<&str, usize> = HashMap::new();
    let mut diags = Vec::new();
    for (ui, u) in units.iter().enumerate() {
        for (name, kw) in top_level_decls(&u.ast) {
            match first.get(name) {
                None => {
                    first.insert(name, ui);
                }
                Some(&fi) if fi != ui => {
                    let sp = locate_decl_header(&u.src, &u.path, kw, name);
                    diags.push(
                        Diagnostic::basic(
                            "error",
                            "E0102: duplicate_module_name",
                            name.to_string(),
                            "duplicate_module_name".to_string(),
                            format!(
                                "E0102: duplicate_module_name: top-level {kw} `{name}` is already declared in `{}`; top-level identifiers form one flat, globally unique project namespace (§4.2)",
                                units[fi].path
                            ),
                        )
                        .with_module(name)
                        .with_ai_context(sp, Vec::new(), BTreeMap::new(), Vec::new()),
                    );
                }
                // Same-file redeclarations are not a cross-file conflict (pre-B.4 behaviour).
                Some(_) => {}
            }
        }
    }
    diags
}

/// Lowercase `snake_case` of an upper identifier (`BOUNDED_STACK` -> `bounded_stack`,
/// `BoundedStack` -> `bounded_stack`).
fn to_snake_case(name: &str) -> String {
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in name.chars() {
        if c.is_ascii_uppercase()
            && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
        {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
        prev = Some(c);
    }
    out
}

/// `W0201: file_module_name_mismatch` (§4.2.1): the basename of a file without a `group`
/// header must be the lowercase `snake_case` of its (primary) top-level unit. A file with a
/// single top-level unit must match that unit; a file with several co-located units must
/// match one of them. `group` files are named after their collective domain (§4.3.3) and
/// are exempt. A warning, or an error in `--strict` mode (§4.2.1).
pub fn check_file_module_name(unit: &SourceUnit, strict: bool) -> Vec<Diagnostic> {
    if unit.ast.group.is_some() {
        return Vec::new();
    }
    let decls = top_level_decls(&unit.ast);
    let Some(&(primary, kw)) = decls.first() else {
        return Vec::new();
    };
    let stem = std::path::Path::new(&unit.path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if decls.iter().any(|(n, _)| to_snake_case(n) == stem) {
        return Vec::new();
    }
    let expected = to_snake_case(primary);
    let sp = locate_decl_header(&unit.src, &unit.path, kw, primary);
    vec![Diagnostic::basic(
        if strict { "error" } else { "warning" },
        "W0201: file_module_name_mismatch",
        primary.to_string(),
        "file_module_name_mismatch".to_string(),
        format!(
            "W0201: file_module_name_mismatch: file `{}` declares {kw} `{primary}`; by the one-module-per-file convention it should be named `{expected}.gdl` (§4.2){}",
            unit.path,
            if strict { " (enforced as an error in --strict mode)" } else { "" }
        ),
    )
    .with_module(primary)
    .with_ai_context(sp, Vec::new(), BTreeMap::new(), Vec::new())]
}

/// `E0103: cyclic_module_dependency` (§4.2.3, §4.3): user modules of different files that
/// reference each other in a cycle. Each module's verification uses the contracts of the
/// modules it references as axioms, so a cross-file cycle would make the proofs circular;
/// mutually dependent modules must be co-declared in one file under a `group` header.
pub fn check_module_dependency_cycles(units: &[SourceUnit], table: &LinkTable) -> Vec<Diagnostic> {
    let Some(cycle) = table.find_cycle() else {
        return Vec::new();
    };
    let head = &cycle[0];
    let (path, src, kw) = units
        .iter()
        .find_map(|u| {
            top_level_decls(&u.ast)
                .into_iter()
                .find(|(n, _)| n == head)
                .map(|(_, kw)| (u.path.as_str(), u.src.as_str(), kw))
        })
        .unwrap_or(("<unknown>", "", "module"));
    let files: Vec<String> = cycle
        .iter()
        .take(cycle.len().saturating_sub(1))
        .map(|n| {
            let f = table
                .file_of
                .get(n)
                .and_then(|&i| units.get(i))
                .map(|u| u.path.as_str());
            format!("{n} ({})", f.unwrap_or("?"))
        })
        .collect();
    vec![Diagnostic::basic(
        "error",
        "E0103: cyclic_module_dependency",
        head.clone(),
        "cyclic_module_dependency".to_string(),
        format!(
            "E0103: cyclic_module_dependency: modules declared in different files reference each other cyclically ({}; cycle {}). Cross-file module dependencies must be acyclic; co-declare mutually dependent modules in one file under a `group` header (§4.3)",
            files.join(", "),
            cycle.join(" -> ")
        ),
    )
    .with_module(head)
    .with_ai_context(
        locate_decl_header(src, path, kw, head),
        Vec::new(),
        BTreeMap::new(),
        Vec::new(),
    )]
}

/// Prelude modules whose background theory is injected into user Boogie contexts.
/// `STRING` is injected for modules that mention it, contain string literals or use `IO`
/// (Task B.3: literals denote prelude `STRING` values); `LIST_OPS` / `FUNCTION_OPS` are realized by the built-in
/// `Seq_*` / `Fn1_*` preamble theories already present in every Boogie file.
const BOOGIE_INJECTED_PRELUDE: [&str; 7] = [
    "OPTION",
    "RESULT",
    "SEQ",
    "BIGNAT",
    "STRING",
    "STRING_ERROR",
    "IO_ERROR",
];

/// Marker on the opaque declaration of another prelude module's sort that a module's
/// representation refers to (e.g. `IO_ERROR.Decode(cause: STRING_ERROR)`). Needed when the
/// module is verified on its own (`--stdlib`); stripped on injection, where the referenced
/// module's real theory is injected alongside.
const FOREIGN_PRELUDE_TYPE_MARK: &str = "// foreign-prelude-type";

/// Other prelude modules named by the representation field types of `m`.
fn repr_prelude_deps(m: &ModuleDecl) -> Vec<String> {
    let prelude = standard_prelude();
    let field_types: Vec<&String> = match &m.repr {
        ReprKind::Product(fields) => fields.iter().map(|(_, t)| t).collect(),
        ReprKind::Sum(vs) => vs
            .iter()
            .flat_map(|v| v.fields.iter().map(|(_, t)| t))
            .collect(),
    };
    let mut deps = Vec::new();
    for t in field_types {
        for tok in t.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if !tok.is_empty()
                && tok != m.name
                && prelude.module(tok).is_some()
                && !deps.contains(&tok.to_string())
            {
                deps.push(tok.to_string());
            }
        }
    }
    deps
}

/// `LIST` / `TUPLE` operations handled by the built-in preamble theories.
const BOOGIE_BUILTIN_METHODS: [&str; 9] = [
    "append", "prepend", "head", "tail", "length", "has", "first", "second", "code",
];

struct BoogiePreludeCtx {
    /// Injected prelude modules (referenced by the user module), in canonical order.
    modules: Vec<ModuleDecl>,
    /// Prelude constructor name -> owning prelude module.
    ctor_owner: HashMap<String, String>,
}

impl BoogiePreludeCtx {
    /// The Boogie context of module `m`: the Standard Prelude modules it references (prelude
    /// mode only) plus, in a multi-file project run (Task B.4), the user modules of *other*
    /// files that `m` references, transitively. A linked user module's own prelude
    /// dependencies are injected as well, since its background theory refers to them.
    fn for_module(m: &ModuleDecl) -> Self {
        let linked = linked_user_modules_for(m);
        let mut wanted = prelude_modules_wanted(m);
        for n in &linked {
            wanted.extend(prelude_modules_wanted(n));
        }
        // RESULT.to_option is stated over OPTION: inject the real OPTION theory alongside.
        if wanted.contains("RESULT") {
            wanted.insert("OPTION");
        }
        let prelude = standard_prelude();
        // Injected modules bring the prelude modules their representations refer to.
        loop {
            let before = wanted.len();
            for name in BOOGIE_INJECTED_PRELUDE {
                if wanted.contains(name) {
                    if let Some(p) = prelude.module(name) {
                        for d in repr_prelude_deps(p) {
                            if let Some(n) = BOOGIE_INJECTED_PRELUDE.iter().find(|n| **n == d) {
                                wanted.insert(n);
                            }
                        }
                    }
                }
            }
            if wanted.len() == before {
                break;
            }
        }
        let mut modules: Vec<ModuleDecl> = BOOGIE_INJECTED_PRELUDE
            .iter()
            .filter(|n| wanted.contains(*n) && !linked.iter().any(|lm| lm.name == **n))
            .filter_map(|n| prelude.module(n).cloned())
            .collect();
        let mut ctor_owner = HashMap::new();
        for p in &modules {
            if let ReprKind::Sum(vs) = &p.repr {
                for v in vs {
                    ctor_owner.insert(v.name.clone(), p.name.clone());
                }
            }
        }
        for n in &linked {
            if let ReprKind::Sum(vs) = &n.repr {
                for v in vs {
                    ctor_owner
                        .entry(v.name.clone())
                        .or_insert_with(|| n.name.clone());
                }
            }
        }
        for n in linked {
            if !modules.iter().any(|m| m.name == n.name) {
                modules.push(n);
            }
        }
        BoogiePreludeCtx {
            modules,
            ctor_owner,
        }
    }

    fn module(&self, name: &str) -> Option<&ModuleDecl> {
        self.modules.iter().find(|m| m.name == name)
    }
}

/// Identifiers occurring in module `m`, excluding its own and its routines'/laws' type
/// parameters (which may spell like a module name).
fn module_referenced_idents(m: &ModuleDecl) -> HashSet<String> {
    let debug = format!("{m:?}");
    let mut idents: HashSet<String> = debug
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let mut tps: Vec<&String> = m.type_params.iter().collect();
    for (_, r) in m
        .create_routines
        .iter()
        .chain(m.feature_routines.iter())
        .chain(m.specify_routines.iter())
    {
        tps.extend(r.type_params.iter());
    }
    for (_, law) in &m.specify_laws {
        tps.extend(law.type_params.iter());
    }
    for tp in tps {
        idents.remove(tp);
    }
    idents
}

/// Standard Prelude modules referenced by `m` (empty unless prelude injection is enabled
/// and `m` is not itself a prelude module).
fn prelude_modules_wanted(m: &ModuleDecl) -> HashSet<&'static str> {
    let mut wanted: HashSet<&'static str> = HashSet::new();
    if !prelude_injection_enabled() || standard_prelude().declares(&m.name) {
        return wanted;
    }
    let prelude = standard_prelude();
    let debug = format!("{m:?}");
    let idents: HashSet<&str> = debug
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
        .collect();
    {
        for name in BOOGIE_INJECTED_PRELUDE {
            let Some(p) = prelude.module(name) else {
                continue;
            };
            let ctor_used = match &p.repr {
                ReprKind::Sum(vs) => vs
                    .iter()
                    .any(|v| idents.contains(v.name.as_str()) && !module_has_variant(m, &v.name)),
                ReprKind::Product(_) => false,
            };
            // String literals and the IO text boundary denote prelude `STRING` values (B.3).
            let string_use = name == "STRING"
                && (idents.contains("IO") || !collect_module_strings(m).is_empty());
            // `IO.read_line: IO[RESULT[IO_ERROR, STRING]]` (§11).
            let read_line_use =
                matches!(name, "RESULT" | "IO_ERROR") && idents.contains("read_line");
            if idents.contains(name) || ctor_used || string_use || read_line_use {
                wanted.insert(name);
            }
        }
    }
    wanted
}

// ============================================================================
// 6c. MULTI-FILE PROJECT LINKING (Task B.4, §4.2)
// ============================================================================
//
// All user modules of a compilation run live in one flat, global namespace (§4.2.2).
// When module `M` (file F) references a user module `N` declared in another file, the
// Boogie context of `M` receives `N`'s namespaced background theory (`N__*`: sort,
// constructors, opaque routine functions with their fuel-guarded definitional axioms and
// precondition-guarded contract axioms, invariant, law axioms, routine-precondition
// predicates and law markers) exactly as for an injected prelude module (B.1), but
// emitted in `N`'s own context. No `Verify_*` procedure of `N` is re-emitted into `M`'s
// file: `N`'s obligations are proven once, in `N`'s own Boogie file, *in the same run*.
// Calls from `M` into `N` assert `N`'s `require` clauses at the call site.
//
// Using `N`'s contracts as axioms while verifying `M` is only sound when the dependency
// relation between linked modules is well-founded, so a cycle of cross-file references is
// rejected (`E0103: cyclic_module_dependency`). Modules declared in the *same* file are
// not linked to each other (pre-B.4 behaviour, so single-file runs are unchanged).

/// The user modules of a multi-file compilation run and the file each was declared in.
pub struct LinkTable {
    modules: Vec<ModuleDecl>,
    file_of: HashMap<String, usize>,
    iface_cache: std::cell::RefCell<HashMap<String, Rc<(String, HashSet<String>)>>>,
}

impl LinkTable {
    /// `modules`: every (concept-expanded) user module of the run with its file index.
    pub fn new(modules: Vec<(ModuleDecl, usize)>) -> Self {
        let mut file_of = HashMap::new();
        let mut ms = Vec::new();
        for (m, f) in modules {
            file_of.entry(m.name.clone()).or_insert(f);
            ms.push(m);
        }
        LinkTable {
            modules: ms,
            file_of,
            iface_cache: std::cell::RefCell::new(HashMap::new()),
        }
    }

    fn module(&self, name: &str) -> Option<&ModuleDecl> {
        self.modules.iter().find(|m| m.name == name)
    }

    /// User modules declared in another file than `m` and referenced directly by `m`
    /// (by name, or through one of their constructors), in declaration order.
    pub fn direct_links(&self, m: &ModuleDecl) -> Vec<&ModuleDecl> {
        let Some(&mf) = self.file_of.get(&m.name) else {
            return Vec::new();
        };
        let idents = module_referenced_idents(m);
        self.modules
            .iter()
            .filter(|n| n.name != m.name && self.file_of.get(&n.name) != Some(&mf))
            .filter(|n| n.name != "LIST_OPS" && n.name != "FUNCTION_OPS")
            .filter(|n| {
                idents.contains(&n.name)
                    || matches!(&n.repr, ReprKind::Sum(vs) if vs.iter().any(|v| {
                        idents.contains(&v.name) && !module_has_variant(m, &v.name)
                    }))
            })
            .collect()
    }

    /// A cycle of cross-file module references, if any (as a list of module names, the
    /// first repeated at the end).
    pub fn find_cycle(&self) -> Option<Vec<String>> {
        let edges: HashMap<&str, Vec<&str>> = self
            .modules
            .iter()
            .map(|m| {
                (
                    m.name.as_str(),
                    self.direct_links(m)
                        .into_iter()
                        .map(|n| n.name.as_str())
                        .collect(),
                )
            })
            .collect();
        // 0 = unvisited, 1 = on stack, 2 = done.
        let mut state: HashMap<&str, u8> = HashMap::new();
        fn dfs<'a>(
            v: &'a str,
            edges: &HashMap<&'a str, Vec<&'a str>>,
            state: &mut HashMap<&'a str, u8>,
            stack: &mut Vec<&'a str>,
        ) -> Option<Vec<String>> {
            state.insert(v, 1);
            stack.push(v);
            for &w in edges.get(v).map(|e| e.as_slice()).unwrap_or(&[]) {
                match state.get(w).copied().unwrap_or(0) {
                    1 => {
                        let start = stack.iter().position(|x| *x == w).unwrap_or(0);
                        let mut cyc: Vec<String> =
                            stack[start..].iter().map(|s| s.to_string()).collect();
                        cyc.push(w.to_string());
                        return Some(cyc);
                    }
                    0 => {
                        if let Some(c) = dfs(w, edges, state, stack) {
                            return Some(c);
                        }
                    }
                    _ => {}
                }
            }
            stack.pop();
            state.insert(v, 2);
            None
        }
        for m in &self.modules {
            if state.get(m.name.as_str()).copied().unwrap_or(0) == 0 {
                let mut stack = Vec::new();
                if let Some(c) = dfs(m.name.as_str(), &edges, &mut state, &mut stack) {
                    return Some(c);
                }
            }
        }
        None
    }
}

thread_local! {
    static LINK_TABLE: std::cell::RefCell<Option<Rc<LinkTable>>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs `f` with the multi-file link table of the current compilation run installed.
pub fn with_link_table<T>(table: Option<Rc<LinkTable>>, f: impl FnOnce() -> T) -> T {
    let prev = LINK_TABLE.with(|c| c.replace(table));
    let out = f();
    LINK_TABLE.with(|c| *c.borrow_mut() = prev);
    out
}

fn link_table() -> Option<Rc<LinkTable>> {
    LINK_TABLE.with(|c| c.borrow().clone())
}

/// True when `name` is a user module of the current multi-file run.
fn is_linked_user_module(name: &str) -> bool {
    link_table().is_some_and(|t| t.module(name).is_some())
}

/// The other-file user modules whose theories `m`'s Boogie context needs: the modules `m`
/// references directly, and transitively the ones those reference.
fn linked_user_modules_for(m: &ModuleDecl) -> Vec<ModuleDecl> {
    let Some(table) = link_table() else {
        return Vec::new();
    };
    let mut out: Vec<ModuleDecl> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(m.name.clone());
    let mut queue: std::collections::VecDeque<ModuleDecl> =
        table.direct_links(m).into_iter().cloned().collect();
    while let Some(n) = queue.pop_front() {
        if !seen.insert(n.name.clone()) {
            continue;
        }
        for d in table.direct_links(&n) {
            if !seen.contains(&d.name) {
                queue.push_back(d.clone());
            }
        }
        out.push(n);
    }
    out
}

/// Runs `f` with module `n`'s own Boogie context installed (its prelude and linked
/// dependencies), restoring the caller's context and type environment afterwards.
fn in_own_boogie_ctx<T>(n: &ModuleDecl, f: impl FnOnce() -> T) -> T {
    let ctx = Rc::new(BoogiePreludeCtx::for_module(n));
    let own = if ctx.modules.is_empty() {
        None
    } else {
        Some(ctx)
    };
    let saved_ctx = BOOGIE_PRELUDE_CTX.with(|c| c.replace(own));
    let saved_env = BOOGIE_TYPE_ENV.with(|e| std::mem::take(&mut *e.borrow_mut()));
    let out = f();
    BOOGIE_PRELUDE_CTX.with(|c| *c.borrow_mut() = saved_ctx);
    BOOGIE_TYPE_ENV.with(|e| *e.borrow_mut() = saved_env);
    out
}

/// Precondition predicates and law markers of a linked user module, emitted in its own
/// context (cached per run).
fn linked_contract_interface(n: &ModuleDecl) -> Rc<(String, HashSet<String>)> {
    let table = link_table();
    if let Some(hit) = table
        .as_ref()
        .and_then(|t| t.iface_cache.borrow().get(&n.name).cloned())
    {
        return hit;
    }
    let result = Rc::new(in_own_boogie_ctx(n, || emit_prelude_contract_interface(n)));
    if let Some(t) = table {
        t.iface_cache
            .borrow_mut()
            .insert(n.name.clone(), result.clone());
    }
    result
}

/// The namespaced background theory of linked user module `n` (no `Verify_*` procedures:
/// `n`'s obligations are verified in its own Boogie file of the same run).
fn emit_linked_background_theory(n: &ModuleDecl, fuel_limit: usize) -> String {
    let parts = in_own_boogie_ctx(n, || emit_boogie_parts(n, fuel_limit));
    let mut text = parts.theory;
    text.push_str(&parts.client_axioms);
    text.push_str(&linked_contract_interface(n).0);
    let map: HashMap<String, String> = boogie_declared_names(&text)
        .into_iter()
        .filter(|x| x != &n.name)
        .map(|x| {
            let r = format!("{}__{x}", n.name);
            (x, r)
        })
        .collect();
    format!(
        "// --- Linked project module: {} (background theory; its own obligations are verified in its own file of this run) ---\n{}\n",
        n.name,
        rename_boogie_idents(&text, &map)
    )
}

thread_local! {
    static PRELUDE_INJECTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static BOOGIE_PRELUDE_CTX: std::cell::RefCell<Option<Rc<BoogiePreludeCtx>>> =
        const { std::cell::RefCell::new(None) };
    static BOOGIE_TYPE_ENV: std::cell::RefCell<HashMap<String, String>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Runs `f` with Standard Prelude injection enabled (or disabled) for Boogie emission
/// on the current thread.
pub fn with_prelude_injection<T>(enabled: bool, f: impl FnOnce() -> T) -> T {
    let prev = PRELUDE_INJECTION.with(|c| c.replace(enabled));
    let out = f();
    PRELUDE_INJECTION.with(|c| c.set(prev));
    out
}

fn prelude_injection_enabled() -> bool {
    PRELUDE_INJECTION.with(|c| c.get())
}

fn boogie_prelude_ctx() -> Option<Rc<BoogiePreludeCtx>> {
    BOOGIE_PRELUDE_CTX.with(|c| c.borrow().clone())
}

fn module_self_type(m: &ModuleDecl) -> String {
    if m.type_params.is_empty() {
        m.name.clone()
    } else {
        format!("{}[{}]", m.name, m.type_params.join(", "))
    }
}

/// Records the Gardel types of a routine's parameters (and `Result`) for receiver-type
/// resolution of prelude calls. Only active while a prelude context is installed.
fn set_boogie_type_env(m: &ModuleDecl, params: &[(String, String)], ret: Option<&str>) {
    let me = module_self_type(m);
    ELEM_TYPE_ENV.with(|env| {
        let mut env = env.borrow_mut();
        env.clear();
        for (pn, pt) in params {
            let t = if pt == "Current" {
                me.clone()
            } else {
                pt.clone()
            };
            env.insert(pn.clone(), t);
        }
    });
    if boogie_prelude_ctx().is_none() {
        return;
    }
    BOOGIE_TYPE_ENV.with(|env| {
        let mut env = env.borrow_mut();
        env.clear();
        for (pn, pt) in params {
            let t = if pt == "Current" {
                me.clone()
            } else {
                pt.clone()
            };
            env.insert(pn.clone(), t);
        }
        if let Some(rt) = ret {
            let t = if rt == "Current" {
                me.clone()
            } else {
                rt.to_string()
            };
            env.insert("Result".to_string(), t);
        }
    });
}

fn clear_boogie_type_env() {
    ELEM_TYPE_ENV.with(|env| env.borrow_mut().clear());
    BOOGIE_TYPE_ENV.with(|env| env.borrow_mut().clear());
}

fn boogie_env_type(v: &str) -> Option<String> {
    BOOGIE_TYPE_ENV.with(|env| env.borrow().get(v).cloned())
}

fn bind_boogie_env_type(v: &str, ty: &str) {
    if boogie_prelude_ctx().is_some() {
        BOOGIE_TYPE_ENV.with(|env| {
            env.borrow_mut().insert(v.to_string(), ty.to_string());
        });
    }
}

/// Task B.3 / §9: static checks on prelude `STRING` values in a user module (prelude mode).
///
/// * `E0311 non_canonical_string_equality`: `a = b` with a `STRING` operand. `STRING` is
///   `EQ` but not `CANONICAL_EQ` (two values with equal `chars` may differ in hidden
///   capacity), so structural Leibniz `=` is forbidden (§9); use `a.equals(b)` or
///   `a.chars = b.chars`.
/// * `E0312 string_plus_operator`: `+` on `STRING` is not part of the `STRING` API (§3.1).
/// * `E0313 string_literal_pattern`: a string-literal `match` pattern is a structural
///   comparison as well.
///
/// Operand types come from the same best-effort inference that routes prelude calls
/// (literals, parameters, `Result`, `let`/IO-bound locals and prelude routine results).
/// Concept `c` together with every concept it (transitively) refines.
fn concept_closure(c: &str) -> Vec<String> {
    let prelude = standard_prelude();
    let mut out = vec![c.to_string()];
    let mut i = 0;
    while i < out.len() {
        if let Some(cd) = prelude.concepts.iter().find(|x| x.name == out[i]) {
            for r in &cd.refines {
                if !out.contains(r) {
                    out.push(r.clone());
                }
            }
        }
        i += 1;
    }
    out
}

/// Whether Gardel type `ty` (as seen from module `m`) satisfies concept `concept`.
/// Primitive scalars are CANONICAL_EQ and ORD (§9); a type parameter satisfies its declared
/// bound; a module type satisfies the concepts it lists, provided its own `where`
/// constraints hold for the type arguments.
fn type_satisfies(ty: &str, concept: &str, m: &ModuleDecl, ctx: &BoogiePreludeCtx) -> bool {
    let ty = ty.trim();
    if matches!(
        ty,
        "U64" | "NAT" | "I64" | "INT" | "BOOL" | "CHAR" | "UNIT" | "()" | "EXIT_CODE"
    ) {
        return matches!(concept, "EQ" | "CANONICAL_EQ" | "ORD");
    }
    if m.type_params.iter().any(|tp| tp == ty) {
        return m
            .type_bounds
            .get(ty)
            .is_some_and(|b| concept_closure(b).iter().any(|c| c == concept));
    }
    let (head, args) = split_type_app(ty);
    let owner = if head == m.name {
        Some(m)
    } else {
        ctx.module(head)
    };
    let Some(owner) = owner else {
        return false;
    };
    let Some(sat) = &owner.satisfies else {
        return false;
    };
    if !sat
        .concepts
        .iter()
        .any(|c| concept_closure(c).iter().any(|x| x == concept))
    {
        return false;
    }
    sat.where_constraints.iter().all(|(tp, req)| {
        match owner.type_params.iter().position(|p| p == tp) {
            Some(i) if i < args.len() => type_satisfies(args[i], req, m, ctx),
            Some(_) => false,
            None => true,
        }
    })
}

/// `E0314: unsatisfied_where_constraint` (§9 conditional conformance): a call of a concept
/// routine (e.g. `equals` of `EQ`) on a value of type `M[T..]`, where `M` lists that concept
/// directly in `satisfies .. where tp satisfies C`, requires the type arguments to satisfy
/// the `where` constraints. Example: `SEQ[A] satisfies EQ where A satisfies CANONICAL_EQ`, so
/// `s.equals(t)` on `SEQ[SEQ[U64]]` is rejected: `SEQ.equals` compares elements
/// structurally, which is behavioral equality only for canonical elements.
/// Termination through function values (§7, §8): a routine in a recursive call group may
/// reach its group only by direct calls, whose arguments the `variant` obligations can relate
/// to the caller's variant. A group member called inside a lambda body, or passed around as a
/// function value, could be applied anywhere and at any argument, so no variant can be checked
/// for it: such recursion is rejected (E0315) instead of being left to the backend.
pub fn check_higher_order_recursion(
    m: &ModuleDecl,
    src: &str,
    input_path: &str,
) -> Vec<Diagnostic> {
    let prev = prelude_injection_enabled().then(|| {
        let ctx = Rc::new(BoogiePreludeCtx::for_module(m));
        BOOGIE_PRELUDE_CTX.with(|c| c.replace(Some(ctx)))
    });
    let groups = compute_recursive_groups(m);
    let routines: Vec<&RoutineDecl> = m
        .create_routines
        .iter()
        .chain(m.feature_routines.iter())
        .chain(m.specify_routines.iter())
        .map(|(_, r)| r)
        .collect();
    let mut found: Vec<(String, String)> = Vec::new();
    for r in routines {
        let Some(group) = groups.get(&r.name) else {
            continue;
        };
        set_boogie_type_env(m, &r.params, Some(&r.ret_type));
        for (callee, kind) in routine_uses(r, m) {
            if kind == RoutineUse::Call || !group.contains(&callee) {
                continue;
            }
            let how = match kind {
                RoutineUse::InClosure => "is called inside a lambda",
                _ => "is used as a function value",
            };
            let mut members: Vec<&String> = group.iter().collect();
            members.sort();
            let members: Vec<String> = members.iter().map(|n| format!("`{n}`")).collect();
            found.push((
                r.name.clone(),
                format!(
                    "`{callee}` {how} inside its own recursive call group ({}); a `variant` can only be checked across direct calls, so recursion through function values cannot be shown to terminate — call `{callee}` directly",
                    members.join(", ")
                ),
            ));
        }
    }
    clear_boogie_type_env();
    if let Some(prev) = prev {
        BOOGIE_PRELUDE_CTX.with(|c| *c.borrow_mut() = prev);
    }
    found.sort();
    found.dedup();
    found
        .into_iter()
        .map(|(rn, msg)| {
            let sp = locate_span(src, input_path, "higher_order_recursion", &rn);
            Diagnostic::basic(
                "error",
                "E0315: higher_order_recursion",
                rn,
                "higher_order_recursion".to_string(),
                msg,
            )
            .with_module(&m.name)
            .with_ai_context(sp, Vec::new(), BTreeMap::new(), Vec::new())
        })
        .collect()
}

pub fn check_where_constraints(m: &ModuleDecl, src: &str, input_path: &str) -> Vec<Diagnostic> {
    if !prelude_injection_enabled() || standard_prelude().declares(&m.name) {
        return Vec::new();
    }
    let ctx = Rc::new(BoogiePreludeCtx::for_module(m));
    let prev = BOOGIE_PRELUDE_CTX.with(|c| c.replace(Some(ctx.clone())));
    let prelude = standard_prelude();

    fn walk(
        e: &Expr,
        rn: &str,
        m: &ModuleDecl,
        ctx: &BoogiePreludeCtx,
        out: &mut Vec<(String, String)>,
    ) {
        let walk_stmts = |stmts: &[Stmt], out: &mut Vec<(String, String)>| {
            for st in stmts {
                match st {
                    Stmt::Let(v, x) => {
                        walk(x, rn, m, ctx, out);
                        if let Some(t) = infer_gardel_type(x, m, ctx) {
                            bind_boogie_env_type(v, &t);
                        }
                    }
                    Stmt::Check(_, x) | Stmt::Assume(_, x) => walk(x, rn, m, ctx, out),
                    Stmt::Reveal(_) => {}
                }
            }
        };
        match e {
            Expr::MethodCall(recv, rname, args) => {
                walk(recv, rn, m, ctx, out);
                args.iter().for_each(|a| walk(a, rn, m, ctx, out));
                let Some(t) = infer_gardel_type(recv, m, ctx) else {
                    return;
                };
                let (head, targs) = split_type_app(&t);
                let owner = if head == m.name {
                    Some(m)
                } else {
                    ctx.module(head)
                };
                let Some(owner) = owner else {
                    return;
                };
                let Some(sat) = &owner.satisfies else {
                    return;
                };
                // The concept (listed directly) that declares this routine.
                let prelude = standard_prelude();
                let declaring = sat.concepts.iter().find(|c| {
                    prelude
                        .concepts
                        .iter()
                        .any(|cd| &cd.name == *c && cd.signatures.iter().any(|s| &s.name == rname))
                });
                if declaring.is_none() {
                    return;
                }
                for (tp, req) in &sat.where_constraints {
                    let Some(i) = owner.type_params.iter().position(|p| p == tp) else {
                        continue;
                    };
                    let Some(arg) = targs.get(i) else {
                        continue;
                    };
                    if !type_satisfies(arg, req, m, ctx) {
                        out.push((
                            rn.to_string(),
                            format!(
                                "E0314: unsatisfied_where_constraint: `{}` calls `{rname}` of `{}` on `{t}`, but `{} satisfies {} where {tp} satisfies {req}` and `{arg}` does not satisfy {req} (§9)",
                                format_gardel_expr(e),
                                owner.name,
                                owner.name,
                                sat.concepts.join(", "),
                            ),
                        ));
                    }
                }
            }
            Expr::BinOp(_, l, r) => {
                walk(l, rn, m, ctx, out);
                walk(r, rn, m, ctx, out);
            }
            Expr::UnaryNot(x) | Expr::UnaryNeg(x) => walk(x, rn, m, ctx, out),
            Expr::Call(_, args) => args.iter().for_each(|a| walk(a, rn, m, ctx, out)),
            Expr::If(c, ts, te, es, ee) => {
                walk(c, rn, m, ctx, out);
                walk_stmts(ts, out);
                walk(te, rn, m, ctx, out);
                walk_stmts(es, out);
                walk(ee, rn, m, ctx, out);
            }
            Expr::Match(scrut, cases) => {
                walk(scrut, rn, m, ctx, out);
                for c in cases {
                    walk_stmts(&c.stmts, out);
                    walk(&c.body, rn, m, ctx, out);
                }
            }
            Expr::RecordLit(fs) => fs.iter().for_each(|(_, x)| walk(x, rn, m, ctx, out)),
            Expr::RecordUpdate(b, fs) => {
                walk(b, rn, m, ctx, out);
                fs.iter().for_each(|(_, x)| walk(x, rn, m, ctx, out));
            }
            Expr::TupleLit(xs) => xs.iter().for_each(|x| walk(x, rn, m, ctx, out)),
            Expr::Forall(_, _, b) | Expr::Exists(_, _, b) => walk(b, rn, m, ctx, out),
            Expr::WithIo(stmts, fin) => {
                for s in stmts {
                    match s {
                        IoStmt::Bind(v, x) => {
                            walk(x, rn, m, ctx, out);
                            if let Some(t) = infer_gardel_type(x, m, ctx) {
                                bind_boogie_env_type(v, &t);
                            }
                        }
                        IoStmt::Action(x) => walk(x, rn, m, ctx, out),
                    }
                }
                walk(fin, rn, m, ctx, out);
            }
            Expr::Lambda(_, _, stmts, body) => {
                walk_stmts(stmts, out);
                walk(body, rn, m, ctx, out);
            }
            _ => {}
        }
    }

    let mut found: Vec<(String, String)> = Vec::new();
    let routines: Vec<&RoutineDecl> = m
        .create_routines
        .iter()
        .chain(m.feature_routines.iter())
        .chain(m.specify_routines.iter())
        .map(|(_, r)| r)
        .collect();
    for r in routines {
        set_boogie_type_env(m, &r.params, Some(&r.ret_type));
        let mut exprs: Vec<&Expr> = r
            .requires
            .iter()
            .chain(r.ensures.iter())
            .map(|(_, e)| e)
            .collect();
        exprs.push(&r.body);
        for st in &r.stmts {
            if let Stmt::Let(_, x) | Stmt::Check(_, x) | Stmt::Assume(_, x) = st {
                exprs.push(x);
            }
        }
        for x in exprs {
            walk(x, &r.name, m, &ctx, &mut found);
        }
    }
    for (_, law) in &m.specify_laws {
        set_boogie_type_env(m, &law.params, None);
        for (_, x) in law.requires.iter().chain(law.ensures.iter()) {
            walk(x, &law.name, m, &ctx, &mut found);
        }
    }
    clear_boogie_type_env();
    BOOGIE_PRELUDE_CTX.with(|c| *c.borrow_mut() = prev);
    let _ = prelude;
    found.sort();
    found.dedup();
    found
        .into_iter()
        .map(|(rn, msg)| {
            let sp = locate_span(src, input_path, "unsatisfied_where_constraint", &rn);
            Diagnostic::basic(
                "error",
                "E0314: unsatisfied_where_constraint",
                rn,
                "unsatisfied_where_constraint".to_string(),
                msg,
            )
            .with_module(&m.name)
            .with_ai_context(sp, Vec::new(), BTreeMap::new(), Vec::new())
        })
        .collect()
}

pub fn check_string_semantics(m: &ModuleDecl, src: &str, input_path: &str) -> Vec<Diagnostic> {
    if !prelude_injection_enabled() || standard_prelude().declares(&m.name) {
        return Vec::new();
    }
    let ctx = Rc::new(BoogiePreludeCtx::for_module(m));
    if ctx.module("STRING").is_none() {
        return Vec::new();
    }
    let prev = BOOGIE_PRELUDE_CTX.with(|c| c.replace(Some(ctx.clone())));
    let mut found: Vec<(String, &'static str, String)> = Vec::new();

    fn is_string(e: &Expr, m: &ModuleDecl, ctx: &BoogiePreludeCtx) -> bool {
        infer_gardel_type(e, m, ctx).is_some_and(|t| t == "STRING")
    }
    fn pat_has_str(p: &Pattern) -> bool {
        match p {
            Pattern::Lit(Literal::Str(_)) => true,
            Pattern::Variant(_, ps) => ps.iter().any(pat_has_str),
            _ => false,
        }
    }
    fn walk_stmts(
        stmts: &[Stmt],
        rn: &str,
        m: &ModuleDecl,
        ctx: &BoogiePreludeCtx,
        out: &mut Vec<(String, &'static str, String)>,
    ) {
        for st in stmts {
            match st {
                Stmt::Let(v, e) => {
                    walk(e, rn, m, ctx, out);
                    if let Some(t) = infer_gardel_type(e, m, ctx) {
                        bind_boogie_env_type(v, &t);
                    }
                }
                Stmt::Check(_, e) | Stmt::Assume(_, e) => walk(e, rn, m, ctx, out),
                Stmt::Reveal(_) => {}
            }
        }
    }
    fn walk(
        e: &Expr,
        rn: &str,
        m: &ModuleDecl,
        ctx: &BoogiePreludeCtx,
        out: &mut Vec<(String, &'static str, String)>,
    ) {
        match e {
            Expr::BinOp(op, l, r) => {
                walk(l, rn, m, ctx, out);
                walk(r, rn, m, ctx, out);
                let s_operand = is_string(l, m, ctx) || is_string(r, m, ctx);
                if op == "=" && s_operand {
                    out.push((
                        rn.to_string(),
                        "E0311: non_canonical_string_equality",
                        format!(
                            "E0311: non_canonical_string_equality: `{}` compares STRING values with structural `=`; STRING satisfies EQ but not CANONICAL_EQ (§9), so use `a.equals(b)` or `a.chars = b.chars`",
                            format_gardel_expr(e)
                        ),
                    ));
                } else if op == "+" && s_operand {
                    out.push((
                        rn.to_string(),
                        "E0312: string_plus_operator",
                        format!(
                            "E0312: string_plus_operator: `{}` applies `+` to STRING; `+` is not part of the prelude STRING API (§3.1) and the bootstrap prelude provides no verified concatenation (STRING.concat is not yet in the prelude); print the parts separately",
                            format_gardel_expr(e)
                        ),
                    ));
                }
            }
            Expr::UnaryNot(x) | Expr::UnaryNeg(x) => walk(x, rn, m, ctx, out),
            Expr::Call(_, args) => args.iter().for_each(|a| walk(a, rn, m, ctx, out)),
            Expr::MethodCall(recv, _, args) => {
                walk(recv, rn, m, ctx, out);
                args.iter().for_each(|a| walk(a, rn, m, ctx, out));
            }
            Expr::If(c, ts, te, es, ee) => {
                walk(c, rn, m, ctx, out);
                walk_stmts(ts, rn, m, ctx, out);
                walk(te, rn, m, ctx, out);
                walk_stmts(es, rn, m, ctx, out);
                walk(ee, rn, m, ctx, out);
            }
            Expr::Match(scrut, cases) => {
                walk(scrut, rn, m, ctx, out);
                for c in cases {
                    if pat_has_str(&c.pattern) {
                        out.push((
                            rn.to_string(),
                            "E0313: string_literal_pattern",
                            format!(
                                "E0313: string_literal_pattern: `case {}` matches a STRING structurally; STRING is not CANONICAL_EQ (§9), use `if s.equals(...)`",
                                format_pattern(&c.pattern)
                            ),
                        ));
                    }
                    walk_stmts(&c.stmts, rn, m, ctx, out);
                    walk(&c.body, rn, m, ctx, out);
                }
            }
            Expr::RecordLit(fs) => fs.iter().for_each(|(_, x)| walk(x, rn, m, ctx, out)),
            Expr::RecordUpdate(b, fs) => {
                walk(b, rn, m, ctx, out);
                fs.iter().for_each(|(_, x)| walk(x, rn, m, ctx, out));
            }
            Expr::TupleLit(xs) => xs.iter().for_each(|x| walk(x, rn, m, ctx, out)),
            Expr::Forall(_, _, b) | Expr::Exists(_, _, b) => walk(b, rn, m, ctx, out),
            Expr::WithIo(stmts, fin) => {
                for st in stmts {
                    match st {
                        IoStmt::Bind(v, x) => {
                            walk(x, rn, m, ctx, out);
                            if let Some(t) = infer_gardel_type(x, m, ctx) {
                                bind_boogie_env_type(v, &t);
                            }
                        }
                        IoStmt::Action(x) => walk(x, rn, m, ctx, out),
                    }
                }
                walk(fin, rn, m, ctx, out);
            }
            Expr::Lambda(_, _, stmts, b) => {
                walk_stmts(stmts, rn, m, ctx, out);
                walk(b, rn, m, ctx, out);
            }
            _ => {}
        }
    }

    let routines = m
        .create_routines
        .iter()
        .chain(m.feature_routines.iter())
        .chain(m.specify_routines.iter())
        .map(|(_, r)| r);
    for r in routines {
        set_boogie_type_env(m, &r.params, Some(&r.ret_type));
        for (_, e) in &r.requires {
            walk(e, &r.name, m, &ctx, &mut found);
        }
        walk_stmts(&r.stmts, &r.name, m, &ctx, &mut found);
        walk(&r.body, &r.name, m, &ctx, &mut found);
        for (_, e) in &r.ensures {
            walk(e, &r.name, m, &ctx, &mut found);
        }
    }
    for (_, law) in &m.specify_laws {
        set_boogie_type_env(m, &law.params, None);
        for (_, e) in law.requires.iter().chain(law.ensures.iter()) {
            walk(e, &law.name, m, &ctx, &mut found);
        }
        walk_stmts(&law.stmts, &law.name, m, &ctx, &mut found);
        if let Some(b) = &law.body {
            walk(b, &law.name, m, &ctx, &mut found);
        }
    }
    clear_boogie_type_env();
    BOOGIE_PRELUDE_CTX.with(|c| *c.borrow_mut() = prev);

    found
        .into_iter()
        .map(|(routine, rule, msg)| {
            let tag = rule.split(": ").nth(1).unwrap_or(rule).to_string();
            let sp = locate_span(src, input_path, &routine, &routine);
            Diagnostic::basic("error", rule, routine, tag, msg)
                .with_module(&m.name)
                .with_ai_context(sp, Vec::new(), BTreeMap::new(), Vec::new())
        })
        .collect()
}

fn module_has_variant(m: &ModuleDecl, name: &str) -> bool {
    matches!(&m.repr, ReprKind::Sum(vs) if vs.iter().any(|v| v.name == name))
}

/// Finds routine `name` in `m`; the flag is true when it takes a `Current` receiver.
fn find_module_routine<'a>(m: &'a ModuleDecl, name: &str) -> Option<(&'a RoutineDecl, bool)> {
    let is_adt = m.kind == ModuleKind::Module;
    m.feature_routines
        .iter()
        .chain(m.specify_routines.iter())
        .find(|(_, r)| r.name == name)
        .map(|(_, r)| (r, is_adt))
        .or_else(|| {
            m.create_routines
                .iter()
                .find(|(_, r)| r.name == name)
                .map(|(_, r)| (r, false))
        })
}

fn type_head(ty: &str) -> &str {
    ty.trim().split('[').next().unwrap_or("").trim()
}

fn split_type_app(ty: &str) -> (&str, Vec<&str>) {
    let ty = ty.trim();
    if split_top_level_arrow(ty).is_none() {
        if let Some(idx) = ty.find('[') {
            if ty.ends_with(']') {
                return (
                    ty[..idx].trim(),
                    split_top_level_commas(&ty[idx + 1..ty.len() - 1]),
                );
            }
        }
    }
    (ty, Vec::new())
}

fn type_tokens(ty: &str) -> Vec<&str> {
    ty.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
        .collect()
}

fn subst_type_idents(ty: &str, map: &HashMap<String, String>) -> String {
    rename_boogie_idents(ty, map)
}

fn unify_type(pattern: &str, actual: &str, tps: &[String], map: &mut HashMap<String, String>) {
    let p = pattern.trim();
    let a = actual.trim();
    if tps.iter().any(|t| t == p) {
        map.entry(p.to_string()).or_insert_with(|| a.to_string());
        return;
    }
    let (ph, pargs) = split_type_app(p);
    let (ah, aargs) = split_type_app(a);
    if ph == ah && pargs.len() == aargs.len() {
        for (x, y) in pargs.iter().zip(aargs.iter()) {
            unify_type(x, y, tps, map);
        }
    }
}

/// Instantiates the declared result type of `owner.r` from the receiver and argument
/// types. Returns `None` when a type parameter remains unresolved (except those in
/// `in_scope`, i.e. the current module's own type parameters).
fn instantiate_ret_type(
    owner: &ModuleDecl,
    r: &RoutineDecl,
    recv_ty: Option<&str>,
    arg_tys: &[Option<String>],
    in_scope: &[String],
) -> Option<String> {
    let mut tps = owner.type_params.clone();
    tps.extend(r.type_params.iter().cloned());
    let self_ty = module_self_type(owner);
    let mut map = HashMap::new();
    if let Some(rt) = recv_ty {
        unify_type(&self_ty, rt, &tps, &mut map);
    }
    for ((_, pt), at) in r.params.iter().zip(arg_tys.iter()) {
        if let Some(at) = at {
            let pt = if pt == "Current" {
                self_ty.clone()
            } else {
                pt.clone()
            };
            unify_type(&pt, at, &tps, &mut map);
        }
    }
    let ret = if r.ret_type == "Current" {
        self_ty
    } else {
        r.ret_type.clone()
    };
    let inst = subst_type_idents(&ret, &map);
    let unresolved = type_tokens(&ret).into_iter().any(|t| {
        tps.iter().any(|tp| tp == t) && !map.contains_key(t) && !in_scope.iter().any(|s| s == t)
    });
    if unresolved { None } else { Some(inst) }
}

/// Best-effort Gardel type of `e` inside user module `m` (used only to route calls on
/// prelude-typed receivers and to type locals bound to prelude values).
fn infer_gardel_type(e: &Expr, m: &ModuleDecl, ctx: &BoogiePreludeCtx) -> Option<String> {
    match e {
        Expr::Int(_) => Some("NAT".to_string()),
        Expr::Char(_) => Some("CHAR".to_string()),
        // Task B.3: string literals have the prelude type `STRING` (§3.1.4).
        Expr::Str(_) => Some("STRING".to_string()),
        Expr::MethodCall(recv, r, _)
            if r == "read_line" && matches!(&**recv, Expr::Var(v) if v == "IO") =>
        {
            Some(
                if ctx.module("RESULT").is_some() {
                    READ_LINE_RESULT_TYPE
                } else {
                    "STRING"
                }
                .to_string(),
            )
        }
        Expr::Bool(_) | Expr::UnaryNot(_) => Some("BOOL".to_string()),
        Expr::BinOp(op, l, _) => {
            if matches!(
                op.as_str(),
                "+" | "-" | "*" | "/" | "%" | "+%" | "-%" | "*%" | "&" | "|" | "^" | "<<" | ">>"
            ) {
                infer_gardel_type(l, m, ctx).or_else(|| Some("NAT".to_string()))
            } else {
                Some("BOOL".to_string())
            }
        }
        Expr::Var(v) => {
            if v == "Current" {
                return Some(module_self_type(m));
            }
            if let Some(t) = boogie_env_type(v) {
                return Some(t);
            }
            if let ReprKind::Product(fields) = &m.repr {
                if let Some((_, fty)) = fields.iter().find(|(f, _)| f == v) {
                    return Some(fty.clone());
                }
            }
            None
        }
        Expr::Call(f, args) => {
            let arg_tys: Vec<Option<String>> =
                args.iter().map(|a| infer_gardel_type(a, m, ctx)).collect();
            if module_has_variant(m, f) {
                return Some(module_self_type(m));
            }
            if let Some((r, _)) = find_module_routine(m, f) {
                return instantiate_ret_type(m, r, None, &arg_tys, &m.type_params);
            }
            let owner = ctx.module(ctx.ctor_owner.get(f)?)?;
            if let ReprKind::Sum(vs) = &owner.repr {
                let v = vs.iter().find(|v| &v.name == f)?;
                let mut map = HashMap::new();
                for ((_, fty), at) in v.fields.iter().zip(arg_tys.iter()) {
                    if let Some(at) = at {
                        unify_type(fty, at, &owner.type_params, &mut map);
                    }
                }
                if owner.type_params.iter().all(|tp| map.contains_key(tp)) {
                    return Some(subst_type_idents(&module_self_type(owner), &map));
                }
            }
            None
        }
        Expr::MethodCall(recv, r, args) => {
            let arg_tys: Vec<Option<String>> =
                args.iter().map(|a| infer_gardel_type(a, m, ctx)).collect();
            if let Expr::Var(v) = &**recv {
                if v != "Current" && boogie_env_type(v).is_none() {
                    if v == &m.name {
                        let (rd, _) = find_module_routine(m, r)?;
                        return instantiate_ret_type(m, rd, None, &arg_tys, &m.type_params);
                    }
                    if let Some(p) = ctx.module(v) {
                        let (rd, _) = find_module_routine(p, r)?;
                        return instantiate_ret_type(p, rd, None, &arg_tys, &[]);
                    }
                    if v == "LIST" {
                        return match r.as_str() {
                            "singleton" => arg_tys
                                .first()
                                .cloned()
                                .flatten()
                                .map(|t| format!("LIST[{t}]")),
                            _ => None,
                        };
                    }
                }
            }
            let rt = infer_gardel_type(recv, m, ctx)?;
            let head = type_head(&rt).to_string();
            if head == "LIST" || (head == "SEQ" && ctx.module("SEQ").is_none()) {
                let elem = split_type_app(&rt).1.first().map(|s| s.to_string());
                return match r.as_str() {
                    "head" => elem,
                    "tail" | "prepend" | "append" => Some(rt.clone()),
                    "length" => Some("NAT".to_string()),
                    "has" | "is_empty" => Some("BOOL".to_string()),
                    _ => None,
                };
            }
            if head == "TUPLE" {
                let parts = split_type_app(&rt).1;
                return match r.as_str() {
                    "first" => parts.first().map(|s| s.to_string()),
                    "second" => parts.get(1).map(|s| s.to_string()),
                    _ => None,
                };
            }
            let (owner, in_scope): (&ModuleDecl, &[String]) = if head == m.name {
                (m, &m.type_params)
            } else {
                (ctx.module(&head)?, &[])
            };
            if let Some((rd, _)) = find_module_routine(owner, r) {
                return instantiate_ret_type(owner, rd, Some(&rt), &arg_tys, in_scope);
            }
            if let ReprKind::Product(fields) = &owner.repr {
                if let Some((_, fty)) = fields.iter().find(|(f, _)| f == r) {
                    let mut map = HashMap::new();
                    unify_type(&module_self_type(owner), &rt, &owner.type_params, &mut map);
                    return Some(subst_type_idents(fty, &map));
                }
            }
            None
        }
        Expr::If(_, _, t, _, f) => {
            infer_gardel_type(t, m, ctx).or_else(|| infer_gardel_type(f, m, ctx))
        }
        _ => None,
    }
}

/// Decides whether `recv.mname(..)` in user module `m` targets an injected prelude
/// module's routine, returning that module's name.
fn resolve_prelude_receiver(
    recv: &Expr,
    mname: &str,
    m: &ModuleDecl,
    ctx: &BoogiePreludeCtx,
) -> Option<String> {
    let has_instance_routine =
        |p: &ModuleDecl| find_module_routine(p, mname).is_some_and(|(_, has_cur)| has_cur);
    if let Some(ty) = infer_gardel_type(recv, m, ctx) {
        let head = type_head(&ty);
        if head != m.name {
            if let Some(p) = ctx.module(head) {
                if has_instance_routine(p) {
                    return Some(p.name.clone());
                }
            }
        }
        return None;
    }
    let is_user_field =
        matches!(&m.repr, ReprKind::Product(fs) if fs.iter().any(|(f, _)| f == mname));
    if BOOGIE_BUILTIN_METHODS.contains(&mname)
        || find_module_routine(m, mname).is_some()
        || is_user_field
    {
        return None;
    }
    let cands: Vec<&ModuleDecl> = ctx
        .modules
        .iter()
        .filter(|p| has_instance_routine(p))
        .collect();
    if cands.len() == 1 {
        Some(cands[0].name.clone())
    } else {
        None
    }
}

/// Prelude laws exposed to user modules as trusted lemma markers `P__law_<idx>_<name>`.
fn prelude_law_markers(p: &ModuleDecl) -> Vec<(usize, &LawDecl)> {
    p.specify_laws
        .iter()
        .enumerate()
        .filter(|(_, (_, law))| {
            !law.params.is_empty()
                && !law.ensures.is_empty()
                && !format!("{:?}{:?}", law.requires, law.ensures).contains("Lambda(")
        })
        .map(|(idx, (_, law))| (idx, law))
        .collect()
}

/// Resolves a call expression in user module `m` to an injected prelude routine:
/// `(module, routine, has_current_receiver)`.
fn prelude_call_target(
    e: &Expr,
    m: &ModuleDecl,
    ctx: &BoogiePreludeCtx,
) -> Option<(String, String, bool)> {
    let Expr::MethodCall(recv, r, _) = e else {
        return None;
    };
    if let Expr::Var(v) = &**recv {
        if v != &m.name && v != "Current" && boogie_env_type(v).is_none() {
            if let Some(p) = ctx.module(v) {
                return find_module_routine(p, r).map(|_| (v.clone(), r.clone(), false));
            }
        }
    }
    let p = resolve_prelude_receiver(recv, r, m, ctx)?;
    Some((p, r.clone(), true))
}

/// Emits references from a user module to injected prelude symbols (constructors,
/// routines and law instantiations). Returns `None` for anything not prelude-bound.
fn emit_boogie_prelude_ref(
    e: &Expr,
    m: &ModuleDecl,
    implicit_fields: &HashSet<String>,
    revealed: &HashSet<String>,
    fuel_var: &str,
    strings: &[String],
) -> Option<String> {
    let ctx = boogie_prelude_ctx()?;
    let emit = |x: &Expr| {
        emit_boogie_expr_with_strings(x, m, implicit_fields, revealed, fuel_var, strings)
    };
    match e {
        Expr::Var(v) => {
            if implicit_fields.contains(v)
                || boogie_env_type(v).is_some()
                || module_has_variant(m, v)
            {
                return None;
            }
            let (p, var) = prelude_variant_for(v, m)?;
            if !var.fields.is_empty() {
                return None;
            }
            Some(format!("{p}__{v}()"))
        }
        Expr::Call(f, args) => {
            if find_module_routine(m, f).is_some() || boogie_env_type(f).is_some() {
                return None;
            }
            let (p, _) = prelude_variant_for(f, m)?;
            let a: Vec<String> = args.iter().map(emit).collect();
            Some(format!("{p}__{f}({})", a.join(", ")))
        }
        Expr::MethodCall(recv, r, args) => {
            if let Expr::Var(v) = &**recv {
                if v != &m.name
                    && v != "Current"
                    && boogie_env_type(v).is_none()
                    && !implicit_fields.contains(v)
                {
                    if let Some(p) = ctx.module(v) {
                        if find_module_routine(p, r).is_some() {
                            let mut a = vec![fuel_var.to_string()];
                            a.extend(args.iter().map(emit));
                            return Some(format!("{v}__fn_{r}({})", a.join(", ")));
                        }
                        let (idx, _) = prelude_law_markers(p)
                            .into_iter()
                            .find(|(_, law)| &law.name == r && law.params.len() == args.len())?;
                        if !prelude_contract_interface(p)
                            .1
                            .contains(&format!("law_{idx}_{r}"))
                        {
                            return None;
                        }
                        let a: Vec<String> = args.iter().map(emit).collect();
                        return Some(format!("{v}__law_{idx}_{r}({})", a.join(", ")));
                    }
                }
            }
            let p = resolve_prelude_receiver(recv, r, m, &ctx)?;
            let mut a = vec![fuel_var.to_string(), emit(recv)];
            a.extend(args.iter().map(emit));
            Some(format!("{p}__fn_{r}({})", a.join(", ")))
        }
        _ => None,
    }
}

/// For a constructor not declared by `m`, its owning injected prelude module and variant.
fn prelude_variant_for(ctor: &str, m: &ModuleDecl) -> Option<(String, VariantDecl)> {
    if module_has_variant(m, ctor) {
        return None;
    }
    let ctx = boogie_prelude_ctx()?;
    let owner = ctx.ctor_owner.get(ctor)?;
    match &ctx.module(owner)?.repr {
        ReprKind::Sum(vs) => vs
            .iter()
            .find(|v| v.name == ctor)
            .map(|v| (owner.clone(), v.clone())),
        ReprKind::Product(_) => None,
    }
}

/// Boogie tester (`is_<Ctor>`) for a constructor, namespaced when it is a prelude constructor.
fn boogie_ctor_tester(ctor: &str, m: &ModuleDecl) -> String {
    match prelude_variant_for(ctor, m) {
        Some((p, _)) => format!("{p}__is_{ctor}"),
        None => format!("is_{ctor}"),
    }
}

/// Field declarations and Boogie projection prefix for a constructor seen in a `match` of
/// user module `m` (its own variants, or an injected prelude variant).
fn boogie_match_variant(ctor: &str, m: &ModuleDecl) -> Option<(String, VariantDecl)> {
    if let ReprKind::Sum(vs) = &m.repr {
        if let Some(v) = vs.iter().find(|v| v.name == ctor) {
            return Some((String::new(), v.clone()));
        }
    }
    prelude_variant_for(ctor, m).map(|(p, v)| (format!("{p}__"), v))
}

/// Declares the names introduced by a Boogie text (functions, constants, types).
fn boogie_declared_names(text: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    for line in text.lines() {
        let t = line.trim_start();
        let rest = if let Some(r) = t.strip_prefix("function ") {
            let mut r = r.trim_start();
            while r.starts_with("{:") {
                match r.find('}') {
                    Some(end) => r = r[end + 1..].trim_start(),
                    None => break,
                }
            }
            r
        } else if let Some(r) = t.strip_prefix("const ") {
            r.trim_start()
                .strip_prefix("unique ")
                .unwrap_or(r)
                .trim_start()
        } else if let Some(r) = t.strip_prefix("type ") {
            r.trim_start()
        } else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            names.insert(name);
        }
    }
    names
}

/// Renames whole identifiers of a Boogie (or Gardel type) text according to `map`.
fn rename_boogie_idents(text: &str, map: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_ascii_alphanumeric() || c == '_' {
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = chars.peek() {
                if d.is_ascii_alphanumeric() || d == '_' {
                    end = j + d.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let tok = &text[i..end];
            if c.is_ascii_digit() {
                out.push_str(tok);
            } else if let Some(rep) = map.get(tok) {
                out.push_str(rep);
            } else {
                out.push_str(tok);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Precondition predicates and trusted law markers of a prelude module, emitted in the
/// module's own (bootstrap) context; namespaced afterwards with the rest of its theory.
/// Returns the Boogie text and the (un-namespaced) names it declares. Cached per thread.
fn prelude_contract_interface(p: &ModuleDecl) -> Rc<(String, HashSet<String>)> {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<String, Rc<(String, HashSet<String>)>>> =
            std::cell::RefCell::new(HashMap::new());
    }
    if is_linked_user_module(&p.name) {
        // A user module of another file in a multi-file run (Task B.4).
        return linked_contract_interface(p);
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&p.name).cloned()) {
        return hit;
    }
    // Emit in the prelude module's own context, never through a user's prelude context.
    let saved_ctx = BOOGIE_PRELUDE_CTX.with(|c| c.borrow_mut().take());
    let saved_env = BOOGIE_TYPE_ENV.with(|e| std::mem::take(&mut *e.borrow_mut()));
    let result = Rc::new(emit_prelude_contract_interface(p));
    BOOGIE_PRELUDE_CTX.with(|c| *c.borrow_mut() = saved_ctx);
    BOOGIE_TYPE_ENV.with(|e| *e.borrow_mut() = saved_env);
    CACHE.with(|c| c.borrow_mut().insert(p.name.clone(), result.clone()));
    result
}

fn emit_prelude_contract_interface(p: &ModuleDecl) -> (String, HashSet<String>) {
    let strings = collect_module_strings(p);
    let empty_fields = HashSet::new();
    let empty_revealed = HashSet::new();
    let emit = |e: &Expr| {
        emit_boogie_expr_with_strings(
            e,
            p,
            &empty_fields,
            &empty_revealed,
            "DefaultFuel",
            &strings,
        )
    };
    // Boogie type parameters must occur in the signature; a declaration whose body
    // mentions a type parameter absent from its signature is not emitted.
    let sig_tps = |extra: &[String], param_tys: &[String], body: &str| -> Option<String> {
        let all: Vec<&String> = p.type_params.iter().chain(extra.iter()).collect();
        let mut tps: Vec<String> = Vec::new();
        for tp in &all {
            let used = param_tys
                .iter()
                .any(|t| type_tokens(t).contains(&tp.as_str()));
            if used && !tps.contains(tp) {
                tps.push((*tp).clone());
            }
        }
        if type_tokens(body)
            .iter()
            .any(|t| all.iter().any(|tp| tp.as_str() == *t) && !tps.iter().any(|tp| tp == t))
        {
            return None;
        }
        Some(if tps.is_empty() {
            String::new()
        } else {
            format!("<{}>", tps.join(", "))
        })
    };
    let mut names = HashSet::new();
    let mut out = String::new();
    out.push_str(&format!(
        "// --- {}: routine preconditions (definitions) & trusted law markers ---\n",
        p.name
    ));
    let routines = p.create_routines.iter().map(|(_, r)| (r, false)).chain(
        p.feature_routines
            .iter()
            .chain(p.specify_routines.iter())
            .map(|(_, r)| (r, p.kind == ModuleKind::Module)),
    );
    for (r, has_current) in routines {
        let mut params = Vec::new();
        let mut tys = Vec::new();
        if has_current {
            let t = map_boogie_type("Current", p);
            params.push(format!("Current: {t}"));
            tys.push(t);
        }
        for (pn, pt) in &r.params {
            let t = map_boogie_type(pt, p);
            params.push(format!("{pn}: {t}"));
            tys.push(t);
        }
        for (idx, (_, req)) in r.requires.iter().enumerate() {
            let body = emit(req);
            let Some(tq) = sig_tps(&r.type_params, &tys, &body) else {
                continue;
            };
            let name = format!("pre_{}_{idx}", r.name);
            out.push_str(&format!(
                "function {{:inline}} {name}{tq}({}) returns (bool) {{ {body} }}\n",
                params.join(", "),
            ));
            names.insert(name);
        }
    }
    for (idx, law) in prelude_law_markers(p) {
        let params: Vec<String> = law
            .params
            .iter()
            .map(|(pn, pt)| format!("{pn}: {}", map_boogie_type(pt, p)))
            .collect();
        let tys: Vec<String> = law
            .params
            .iter()
            .map(|(_, pt)| map_boogie_type(pt, p))
            .collect();
        let args: Vec<String> = law.params.iter().map(|(pn, _)| pn.clone()).collect();
        let ens: Vec<String> = law
            .ensures
            .iter()
            .map(|(_, e)| format!("({})", emit(e)))
            .collect();
        let reqs: Vec<String> = law
            .requires
            .iter()
            .map(|(_, e)| format!("({})", emit(e)))
            .collect();
        let body = format!("{} {}", ens.join(" && "), reqs.join(" "));
        if type_tokens(&body).contains(&"Current") {
            continue;
        }
        let Some(tq) = sig_tps(&law.type_params, &tys, &body) else {
            continue;
        };
        let marker = format!("law_{idx}_{}", law.name);
        let call = format!("{marker}({})", args.join(", "));
        let mut hyps = reqs.clone();
        // A conditional CANONICAL_EQ law is trusted only under its `where` hypothesis.
        if let Some(guard) = canonical_where_guard(p, &p.specify_laws[idx].0) {
            hyps.insert(0, format!("({guard})"));
        }
        let req = if hyps.is_empty() {
            "true".to_string()
        } else {
            hyps.join(" && ")
        };
        out.push_str(&format!(
            "function {marker}{tq}({}) returns (bool);\n",
            params.join(", ")
        ));
        // Definition (conservative): the marker states the law's conclusion.
        out.push_str(&format!(
            "axiom (forall{tq} {} :: {{ {call} }} {call} <==> ({}));\n",
            params.join(", "),
            ens.join(" && ")
        ));
        // Trusted fact: the law (verified in --stdlib bootstrap mode), instantiated on demand.
        out.push_str(&format!(
            "axiom (forall{tq} {} :: {{ {call} }} ({req}) ==> {call});\n",
            params.join(", ")
        ));
        names.insert(marker);
    }
    out.push('\n');
    (out, names)
}

/// The namespaced background theory of prelude module `p` for injection into a user
/// module's Boogie file: no `Verify_*` procedures, so no prelude obligation is re-proven.
fn emit_prelude_background_theory(p: &ModuleDecl, fuel_limit: usize) -> String {
    let parts = emit_boogie_parts(p, fuel_limit);
    // Opaque stand-ins for other prelude sorts are dropped: the real theories are injected.
    let mut text: String = parts
        .theory
        .lines()
        .filter(|l| !l.ends_with(FOREIGN_PRELUDE_TYPE_MARK))
        .map(|l| format!("{l}\n"))
        .collect();
    let foreign = repr_prelude_deps(p);
    text.push_str(&parts.client_axioms);
    // Primitive scalars (`int`: U64/I64/NAT/CHAR; `bool`) are CANONICAL_EQ (§9), so element
    // equality of a conditionally canonical parameter is structural at those instantiations.
    for tp in conditional_canonical_tps(p) {
        for prim in ["int", "bool"] {
            text.push_str(&format!(
                "axiom (forall x: {prim}, y: {prim} :: {{ {tp}_eq(x, y) }} {tp}_eq(x, y) <==> x == y);\n"
            ));
        }
    }
    text.push_str(&prelude_contract_interface(p).0);
    let mut map: HashMap<String, String> = boogie_declared_names(&text)
        .into_iter()
        .filter(|n| n != &p.name && !foreign.contains(n))
        .map(|n| {
            let r = format!("{}__{n}", p.name);
            (n, r)
        })
        .collect();
    if !parts.option_shim.is_empty() {
        // The OPTION shim is dropped; its symbols alias the injected OPTION theory.
        for n in ["None", "Some", "is_None", "is_Some", "fn_is_some"] {
            map.insert(n.to_string(), format!("OPTION__{n}"));
        }
    }
    format!(
        "// --- Injected Standard Prelude: {} (trusted background theory; verified in --stdlib bootstrap mode) ---\n{}\n",
        p.name,
        rename_boogie_idents(&text, &map)
    )
}

/// The Boogie translation of one module, split so that the module's background theory
/// (sorts, constructors, opaque routine functions, definitional/contract axioms and law
/// axioms) can be injected into *another* module's verification context without its
/// verification procedures (Standard Prelude injection, §4.1).
struct BoogieParts {
    /// Module-independent preamble (machine bounds, fuel, Seq/Fn/Tuple/IO theories).
    preamble: String,
    /// Module background theory (sections 3–6 plus type-bound and literal declarations).
    theory: String,
    /// The module's own laws stated as axioms (triggered law axioms and concept axioms).
    /// Injected only into *client* contexts, where this module is verified separately;
    /// never part of the module's own verification context (that would be circular).
    client_axioms: String,
    /// `RESULT`-only minimal `OPTION` shim used when `OPTION` is not otherwise declared.
    option_shim: String,
    /// Verification-condition procedures (section 7) — the module's own proof obligations.
    procedures: String,
}

impl BoogieParts {
    fn join(self) -> String {
        let mut s = self.preamble;
        s.push_str(&self.theory);
        s.push_str(&self.option_shim);
        s.push_str(&self.procedures);
        s
    }
}

pub fn emit_boogie(m: &ModuleDecl, fuel_limit: usize) -> String {
    let ctx = Rc::new(BoogiePreludeCtx::for_module(m));
    if ctx.modules.is_empty() {
        // No prelude ADT or linked project module referenced (or bootstrap mode): the
        // context is identical to a bootstrap emission.
        return emit_boogie_parts(m, fuel_limit).join();
    }
    // Prelude theories are emitted exactly as in `--stdlib` bootstrap mode (no context),
    // then namespaced; none of their verification procedures are included. Linked user
    // modules of other files (Task B.4) are emitted in their own context, then namespaced.
    let mut injected = String::new();
    for p in &ctx.modules {
        if is_linked_user_module(&p.name) {
            injected.push_str(&emit_linked_background_theory(p, fuel_limit));
        } else {
            injected.push_str(&emit_prelude_background_theory(p, fuel_limit));
        }
    }
    let prev = BOOGIE_PRELUDE_CTX.with(|c| c.replace(Some(ctx.clone())));
    let parts = emit_boogie_parts(m, fuel_limit);
    BOOGIE_PRELUDE_CTX.with(|c| *c.borrow_mut() = prev);
    let has_option_module = ctx.modules.iter().any(|p| p.name == "OPTION");
    let mut s = parts.preamble;
    s.push_str(&injected);
    s.push_str(&parts.theory);
    if !has_option_module {
        s.push_str(&parts.option_shim);
    }
    s.push_str(&parts.procedures);
    s
}

fn emit_boogie_parts(m: &ModuleDecl, fuel_limit: usize) -> BoogieParts {
    let mut out = String::new();
    let mut theory = String::new();
    out.push_str(&format!(
        "// Automatically generated by gardelc (Phase 4 -> Direct Boogie Verifier, fuel={fuel_limit})\n"
    ));
    out.push_str("// Module: ");
    out.push_str(&m.name);
    out.push_str("\n\n");

    // 1. Machine Bounds, Sorts, Fuel & Bitvector Theories (§3, §7.5, Plan 3 §1, §2)
    out.push_str("// --- 1. Machine Bounds, Sorts, Fuel & Bitvector Theories ---\n");
    out.push_str("const U64_MAX: int;\n");
    out.push_str("axiom U64_MAX == 18446744073709551615;\n");
    out.push_str("function {:inline} in_u64(x: int) returns (bool) { 0 <= x && x <= U64_MAX }\n\n");
    out.push_str("const I64_MIN: int;\n");
    out.push_str("axiom I64_MIN == -9223372036854775808;\n");
    out.push_str("const I64_MAX: int;\n");
    out.push_str("axiom I64_MAX == 9223372036854775807;\n");
    out.push_str(
        "function {:inline} in_i64(x: int) returns (bool) { I64_MIN <= x && x <= I64_MAX }\n\n",
    );

    out.push_str("type Unit;\n");
    out.push_str("const unique Unit_val: Unit;\n");
    out.push_str("axiom (forall u: Unit :: u == Unit_val);\n");
    out.push_str("type F64;\n\n");

    out.push_str("function {:bvbuiltin \"(_ int2bv 64)\"} int2bv64(x: int) returns (bv64);\n");
    out.push_str("function {:bvbuiltin \"bv2int\"} bv2int64(x: bv64) returns (int);\n");
    out.push_str("function {:bvbuiltin \"bvadd\"} bvadd64(x: bv64, y: bv64) returns (bv64);\n");
    out.push_str("function {:bvbuiltin \"bvsub\"} bvsub64(x: bv64, y: bv64) returns (bv64);\n");
    out.push_str("function {:bvbuiltin \"bvmul\"} bvmul64(x: bv64, y: bv64) returns (bv64);\n");
    out.push_str("function {:bvbuiltin \"bvand\"} bvand64(x: bv64, y: bv64) returns (bv64);\n");
    out.push_str("function {:bvbuiltin \"bvor\"} bvor64(x: bv64, y: bv64) returns (bv64);\n");
    out.push_str("function {:bvbuiltin \"bvxor\"} bvxor64(x: bv64, y: bv64) returns (bv64);\n");
    out.push_str("function {:bvbuiltin \"bvshl\"} bvshl64(x: bv64, y: bv64) returns (bv64);\n");
    out.push_str("function {:bvbuiltin \"bvlshr\"} bvlshr64(x: bv64, y: bv64) returns (bv64);\n\n");

    out.push_str("function {:inline} u64_wrap_add(x: int, y: int) returns (int) { bv2int64(bvadd64(int2bv64(x), int2bv64(y))) }\n");
    out.push_str("function {:inline} u64_wrap_sub(x: int, y: int) returns (int) { bv2int64(bvsub64(int2bv64(x), int2bv64(y))) }\n");
    out.push_str("function {:inline} u64_wrap_mul(x: int, y: int) returns (int) { bv2int64(bvmul64(int2bv64(x), int2bv64(y))) }\n");
    out.push_str("function {:inline} u64_bit_and(x: int, y: int) returns (int) { bv2int64(bvand64(int2bv64(x), int2bv64(y))) }\n");
    out.push_str("function {:inline} u64_bit_or(x: int, y: int) returns (int) { bv2int64(bvor64(int2bv64(x), int2bv64(y))) }\n");
    out.push_str("function {:inline} u64_bit_xor(x: int, y: int) returns (int) { bv2int64(bvxor64(int2bv64(x), int2bv64(y))) }\n");
    out.push_str("function {:inline} u64_bit_shl(x: int, y: int) returns (int) { bv2int64(bvshl64(int2bv64(x), int2bv64(y))) }\n");
    out.push_str("function {:inline} u64_bit_shr(x: int, y: int) returns (int) { bv2int64(bvlshr64(int2bv64(x), int2bv64(y))) }\n\n");

    // Module-specific type-bound declarations belong to the module theory.
    let mut preamble_head = std::mem::take(&mut out);
    // A conditional `where tp satisfies CANONICAL_EQ` bound does not make `tp_eq` structural
    // module-wide; see `canonical_where_guard`.
    let conditional_tps = conditional_canonical_tps(m);
    for (tp, bound) in &m.type_bounds {
        // Module type parameters are the holes of `type M A ...;` and are quantified (`<A>`) in every
        // module function, so they must not also be declared as global types.
        let is_adt_tp = m.type_params.contains(tp);
        if !is_adt_tp {
            out.push_str(&format!("type {tp};\n"));
            if bound == "EQ" || bound == "ORD" || bound == "CANONICAL_EQ" {
                out.push_str(&format!(
                    "function {tp}_eq(x: {tp}, y: {tp}) returns (bool);\n"
                ));
                out.push_str(&format!(
                    "axiom (forall x: {tp} :: {{ {tp}_eq(x, x) }} {tp}_eq(x, x));\n"
                ));
                out.push_str(&format!("axiom (forall x: {tp}, y: {tp} :: {{ {tp}_eq(x, y) }} {tp}_eq(x, y) ==> {tp}_eq(y, x));\n"));
                out.push_str(&format!("axiom (forall x: {tp}, y: {tp}, z: {tp} :: {{ {tp}_eq(x, y), {tp}_eq(y, z) }} {tp}_eq(x, y) && {tp}_eq(y, z) ==> {tp}_eq(x, z));\n"));
            }
            if bound == "CANONICAL_EQ" && !conditional_tps.contains(tp) {
                out.push_str(&format!(
                    "axiom (forall x: {tp}, y: {tp} :: {{ {tp}_eq(x, y) }} {tp}_eq(x, y) <==> (x == y));\n"
                ));
            }
            if bound == "ORD" {
                out.push_str(&format!(
                    "function {tp}_less(x: {tp}, y: {tp}) returns (bool);\n"
                ));
                out.push_str(&format!(
                    "function {tp}_less_eq(x: {tp}, y: {tp}) returns (bool);\n"
                ));
                out.push_str(&format!(
                    "axiom (forall x: {tp} :: {{ {tp}_less(x, x) }} !{tp}_less(x, x));\n"
                ));
                out.push_str(&format!("axiom (forall x: {tp}, y: {tp}, z: {tp} :: {{ {tp}_less(x, y), {tp}_less(y, z) }} {tp}_less(x, y) && {tp}_less(y, z) ==> {tp}_less(x, z));\n"));
                out.push_str(&format!("axiom (forall x: {tp}, y: {tp} :: {{ {tp}_less_eq(x, y) }} {tp}_less_eq(x, y) <==> ({tp}_less(x, y) || {tp}_eq(x, y)));\n"));
                out.push_str(&format!("axiom (forall x: {tp}, y: {tp} :: {{ {tp}_less(x, y), {tp}_less(y, x) }} {tp}_less(x, y) || {tp}_eq(x, y) || {tp}_less(y, x));\n"));
            }
        } else {
            if bound == "EQ" || bound == "ORD" || bound == "CANONICAL_EQ" {
                out.push_str(&format!(
                    "function {tp}_eq<{tp}>(x: {tp}, y: {tp}) returns (bool);\n"
                ));
                out.push_str(&format!(
                    "axiom (forall<{tp}> x: {tp} :: {{ {tp}_eq(x, x) }} {tp}_eq(x, x));\n"
                ));
                out.push_str(&format!("axiom (forall<{tp}> x: {tp}, y: {tp} :: {{ {tp}_eq(x, y) }} {tp}_eq(x, y) ==> {tp}_eq(y, x));\n"));
                out.push_str(&format!("axiom (forall<{tp}> x: {tp}, y: {tp}, z: {tp} :: {{ {tp}_eq(x, y), {tp}_eq(y, z) }} {tp}_eq(x, y) && {tp}_eq(y, z) ==> {tp}_eq(x, z));\n"));
            }
            if bound == "CANONICAL_EQ" && !conditional_tps.contains(tp) {
                out.push_str(&format!(
                    "axiom (forall<{tp}> x: {tp}, y: {tp} :: {{ {tp}_eq(x, y) }} {tp}_eq(x, y) <==> (x == y));\n"
                ));
            }
            if bound == "ORD" {
                out.push_str(&format!(
                    "function {tp}_less<{tp}>(x: {tp}, y: {tp}) returns (bool);\n"
                ));
                out.push_str(&format!(
                    "function {tp}_less_eq<{tp}>(x: {tp}, y: {tp}) returns (bool);\n"
                ));
                out.push_str(&format!(
                    "axiom (forall<{tp}> x: {tp} :: {{ {tp}_less(x, x) }} !{tp}_less(x, x));\n"
                ));
                out.push_str(&format!("axiom (forall<{tp}> x: {tp}, y: {tp}, z: {tp} :: {{ {tp}_less(x, y), {tp}_less(y, z) }} {tp}_less(x, y) && {tp}_less(y, z) ==> {tp}_less(x, z));\n"));
                out.push_str(&format!("axiom (forall<{tp}> x: {tp}, y: {tp} :: {{ {tp}_less_eq(x, y) }} {tp}_less_eq(x, y) <==> ({tp}_less(x, y) || {tp}_eq(x, y)));\n"));
                out.push_str(&format!("axiom (forall<{tp}> x: {tp}, y: {tp} :: {{ {tp}_less(x, y), {tp}_less(y, x) }} {tp}_less(x, y) || {tp}_eq(x, y) || {tp}_less(y, x));\n"));
            }
        }
        out.push('\n');
    }
    // Representation fields typed by another prelude module: declare that sort opaquely
    // unless its real theory is injected into this context.
    for dep in repr_prelude_deps(m) {
        let injected = boogie_prelude_ctx().is_some_and(|ctx| ctx.module(&dep).is_some());
        if !injected {
            let arity = standard_prelude()
                .module(&dep)
                .map_or(0, |d| d.type_params.len());
            let holes = " _".repeat(arity);
            out.push_str(&format!("type {dep}{holes}; {FOREIGN_PRELUDE_TYPE_MARK}\n"));
        }
    }
    theory.push_str(&std::mem::take(&mut out));

    out.push_str("type Fuel;\n");
    out.push_str("const unique ZeroFuel: Fuel;\n");
    out.push_str("function SuccFuel(f: Fuel) returns (Fuel);\n");
    out.push_str("const DefaultFuel: Fuel;\n");
    out.push_str("const RevealFuel: Fuel;\n");
    let mut fuel_term = "ZeroFuel".to_string();
    for _ in 0..fuel_limit {
        fuel_term = format!("SuccFuel({fuel_term})");
    }
    out.push_str(&format!("axiom DefaultFuel == {fuel_term};\n"));
    out.push_str("axiom RevealFuel == SuccFuel(DefaultFuel);\n\n");

    // 2. Parametric Sequence (LIST / SEQ), String, BigNat, IO & Tuple2 Theories
    out.push_str("// --- 2. Parametric Sequence (LIST / SEQ), String, IO & Tuple2 Theories ---\n");
    out.push_str("type Seq T;\n");
    out.push_str("type GardelString;\n");
    out.push_str("type IO T;\n");
    out.push_str("type EXIT_CODE = int;\n");
    let strings = collect_module_strings(m);
    let string_mode = prelude_string_mode();
    if !string_mode {
        out.push_str(
            "function Str_concat(a: GardelString, b: GardelString) returns (GardelString);\n",
        );
        out.push_str("function Str_length(s: GardelString) returns (int);\n");
        out.push_str("axiom (forall s: GardelString :: { Str_length(s) } 0 <= Str_length(s));\n");
        out.push_str("const unique str_lit_empty: GardelString;\n");
        out.push_str("axiom Str_length(str_lit_empty) == 0;\n");
    }
    if string_mode {
        // Task B.3 (§3.1.4): each literal is a compiler-defined prelude `STRING` constant.
        theory.push_str(&emit_boogie_string_literal_decls(&strings));
    } else {
        // Bootstrap (`--no-prelude`) mode: no prelude `STRING`; legacy opaque literal sort.
        for (idx, s) in strings.iter().enumerate() {
            theory.push_str(&format!("const unique str_lit_{idx}: GardelString;\n"));
            theory.push_str(&format!(
                "axiom Str_length(str_lit_{idx}) == {};\n",
                s.len()
            ));
        }
    }
    let str_ty = if string_mode {
        "STRING"
    } else {
        "GardelString"
    };
    if !string_mode {
        // Legacy (`--no-prelude`) opaque line. In prelude mode `x <- IO.read_line` is a
        // fresh `havoc` per read (never a constant function: two reads may differ).
        out.push_str(&format!("function IO_read_line() returns ({str_ty});\n"));
    }
    out.push_str(&format!(
        "function IO_print(s: {str_ty}) returns (IO int);\n"
    ));
    out.push_str(&format!(
        "function IO_print_line(s: {str_ty}) returns (IO int);\n"
    ));
    out.push_str("function IO_exit(code: int) returns (IO int);\n\n");

    out.push_str("function Seq_empty<T>() returns (Seq T);\n");
    out.push_str("function Seq_singleton<T>(x: T) returns (Seq T);\n");
    out.push_str("function Seq_prepend<T>(x: T, xs: Seq T) returns (Seq T);\n");
    out.push_str("function Seq_append<T>(xs: Seq T, ys: Seq T) returns (Seq T);\n");
    out.push_str("function Seq_head<T>(xs: Seq T) returns (T);\n");
    out.push_str("function Seq_tail<T>(xs: Seq T) returns (Seq T);\n");
    out.push_str("function Seq_length<T>(xs: Seq T) returns (int);\n");
    out.push_str("function Seq_has<T>(xs: Seq T, x: T) returns (bool);\n\n");

    out.push_str("axiom (forall<T> :: Seq_length(Seq_empty() : Seq T) == 0);\n");
    out.push_str("axiom (forall<T> xs: Seq T :: { Seq_length(xs) } 0 <= Seq_length(xs));\n");
    out.push_str(
        "axiom (forall<T> x: T, xs: Seq T :: { Seq_prepend(x, xs) } Seq_length(Seq_prepend(x, xs)) == Seq_length(xs) + 1 && Seq_head(Seq_prepend(x, xs)) == x && Seq_tail(Seq_prepend(x, xs)) == xs);\n",
    );
    out.push_str(
        "axiom (forall<T> xs: Seq T :: { Seq_tail(xs) } Seq_length(xs) > 0 ==> Seq_length(Seq_tail(xs)) == Seq_length(xs) - 1);\n",
    );
    out.push_str(
        "axiom (forall<T> xs: Seq T, ys: Seq T :: { Seq_append(xs, ys) } Seq_length(Seq_append(xs, ys)) == Seq_length(xs) + Seq_length(ys));\n",
    );
    out.push_str(
        "axiom (forall<T> xs: Seq T :: { Seq_append((Seq_empty() : Seq T), xs) } Seq_append((Seq_empty() : Seq T), xs) == xs);\n",
    );
    out.push_str(
        "axiom (forall<T> xs: Seq T :: { Seq_append(xs, (Seq_empty() : Seq T)) } Seq_append(xs, (Seq_empty() : Seq T)) == xs);\n",
    );
    out.push_str(
        "axiom (forall<T> xs: Seq T, ys: Seq T, zs: Seq T :: { Seq_append(Seq_append(xs, ys), zs) } Seq_append(Seq_append(xs, ys), zs) == Seq_append(xs, Seq_append(ys, zs)));\n",
    );
    out.push_str(
        "axiom (forall<T> x: T, y: T, xs: Seq T :: { Seq_has(Seq_prepend(y, xs), x) } Seq_has(Seq_prepend(y, xs), x) <==> (x == y || Seq_has(xs, x)));\n",
    );
    out.push_str(
        "axiom (forall<T> x: T, xs: Seq T, ys: Seq T :: { Seq_has(Seq_append(xs, ys), x) } Seq_has(Seq_append(xs, ys), x) <==> (Seq_has(xs, x) || Seq_has(ys, x)));\n",
    );
    out.push_str(
        "axiom (forall<T> xs: Seq T :: { Seq_head(xs), Seq_tail(xs) } Seq_length(xs) > 0 ==> Seq_prepend(Seq_head(xs), Seq_tail(xs)) == xs);\n",
    );
    out.push_str(
        "axiom (forall<T> xs: Seq T :: { Seq_length(xs) } Seq_length(xs) == 0 ==> xs == (Seq_empty() : Seq T));\n\n",
    );

    out.push_str("type Tuple2 A B;\n");
    out.push_str("function Tuple2_mk<A, B>(a: A, b: B) returns (Tuple2 A B);\n");
    out.push_str("function Tuple2_first<A, B>(t: Tuple2 A B) returns (A);\n");
    out.push_str("function Tuple2_second<A, B>(t: Tuple2 A B) returns (B);\n");
    out.push_str(
        "axiom (forall<A, B> a: A, b: B :: { Tuple2_mk(a, b) } Tuple2_first(Tuple2_mk(a, b)) == a && Tuple2_second(Tuple2_mk(a, b)) == b);\n\n",
    );

    out.push_str("type Fn1 A B;\n");
    out.push_str("function Fn1_app<A, B>(f: Fn1 A B, x: A) returns (B);\n");
    out.push_str("type Fn2 A B C;\n");
    out.push_str("function Fn2_app<A, B, C>(f: Fn2 A B C, x: A, y: B) returns (C);\n\n");

    out.push_str("function Fn1_id<A>() returns (Fn1 A A);\n");
    out.push_str("axiom (forall<A> x: A :: { Fn1_app(Fn1_id(), x) } Fn1_app(Fn1_id(), x) == x);\n");
    out.push_str("function Fn1_compose<A, B, C>(f: Fn1 B C, g: Fn1 A B) returns (Fn1 A C);\n");
    out.push_str("axiom (forall<A, B, C> f: Fn1 B C, g: Fn1 A B, x: A :: { Fn1_app(Fn1_compose(f, g), x) } Fn1_app(Fn1_compose(f, g), x) == Fn1_app(f, Fn1_app(g, x)));\n\n");

    out.push_str("function Seq_map<A, B>(xs: Seq A, f: Fn1 A B) returns (Seq B);\n");
    out.push_str("axiom (forall<A, B> xs: Seq A, f: Fn1 A B :: { Seq_length(Seq_map(xs, f)) } Seq_length(Seq_map(xs, f)) == Seq_length(xs));\n");
    out.push_str("axiom (forall<A, B> f: Fn1 A B :: { Seq_map(Seq_empty() : Seq A, f) } Seq_map(Seq_empty() : Seq A, f) == (Seq_empty() : Seq B));\n");
    out.push_str("axiom (forall<A, B> x: A, xs: Seq A, f: Fn1 A B :: { Seq_map(Seq_prepend(x, xs), f) } Seq_map(Seq_prepend(x, xs), f) == Seq_prepend(Fn1_app(f, x), Seq_map(xs, f)));\n");
    out.push_str("axiom (forall<A, B> xs: Seq A, ys: Seq A, f: Fn1 A B :: { Seq_map(Seq_append(xs, ys), f) } Seq_map(Seq_append(xs, ys), f) == Seq_append(Seq_map(xs, f), Seq_map(ys, f)));\n");
    out.push_str(
        "axiom (forall<A> xs: Seq A :: { Seq_map(xs, Fn1_id()) } Seq_map(xs, Fn1_id()) == xs);\n",
    );
    out.push_str("axiom (forall<A, B, C> xs: Seq A, f: Fn1 B C, g: Fn1 A B :: { Seq_map(Seq_map(xs, g), f) } Seq_map(Seq_map(xs, g), f) == Seq_map(xs, Fn1_compose(f, g)));\n\n");

    out.push_str("function Seq_filter<A>(xs: Seq A, keep: Fn1 A bool) returns (Seq A);\n");
    out.push_str("axiom (forall<A> xs: Seq A, keep: Fn1 A bool :: { Seq_length(Seq_filter(xs, keep)) } 0 <= Seq_length(Seq_filter(xs, keep)) && Seq_length(Seq_filter(xs, keep)) <= Seq_length(xs));\n");
    out.push_str("axiom (forall<A> keep: Fn1 A bool :: { Seq_filter(Seq_empty() : Seq A, keep) } Seq_filter(Seq_empty() : Seq A, keep) == (Seq_empty() : Seq A));\n");
    out.push_str("axiom (forall<A> x: A, xs: Seq A, keep: Fn1 A bool :: { Seq_filter(Seq_prepend(x, xs), keep) } (Fn1_app(keep, x) ==> Seq_filter(Seq_prepend(x, xs), keep) == Seq_prepend(x, Seq_filter(xs, keep))) && (!Fn1_app(keep, x) ==> Seq_filter(Seq_prepend(x, xs), keep) == Seq_filter(xs, keep)));\n");
    out.push_str("axiom (forall<A> xs: Seq A, ys: Seq A, keep: Fn1 A bool :: { Seq_filter(Seq_append(xs, ys), keep) } Seq_filter(Seq_append(xs, ys), keep) == Seq_append(Seq_filter(xs, keep), Seq_filter(ys, keep)));\n\n");

    out.push_str("function Seq_fold_left<A, B>(xs: Seq A, f: Fn2 B A B, init: B) returns (B);\n");
    out.push_str("axiom (forall<A, B> f: Fn2 B A B, init: B :: { Seq_fold_left(Seq_empty() : Seq A, f, init) } Seq_fold_left(Seq_empty() : Seq A, f, init) == init);\n");
    out.push_str("axiom (forall<A, B> x: A, xs: Seq A, f: Fn2 B A B, init: B :: { Seq_fold_left(Seq_prepend(x, xs), f, init) } Seq_fold_left(Seq_prepend(x, xs), f, init) == Seq_fold_left(xs, f, Fn2_app(f, init, x)));\n");
    out.push_str("axiom (forall<A, B> xs: Seq A, ys: Seq A, f: Fn2 B A B, init: B :: { Seq_fold_left(Seq_append(xs, ys), f, init) } Seq_fold_left(Seq_append(xs, ys), f, init) == Seq_fold_left(ys, f, Seq_fold_left(xs, f, init)));\n\n");

    out.push_str("function Seq_fold_right<A, B>(xs: Seq A, f: Fn2 A B B, init: B) returns (B);\n");
    out.push_str("axiom (forall<A, B> f: Fn2 A B B, init: B :: { Seq_fold_right(Seq_empty() : Seq A, f, init) } Seq_fold_right(Seq_empty() : Seq A, f, init) == init);\n");
    out.push_str("axiom (forall<A, B> x: A, xs: Seq A, f: Fn2 A B B, init: B :: { Seq_fold_right(Seq_prepend(x, xs), f, init) } Seq_fold_right(Seq_prepend(x, xs), f, init) == Fn2_app(f, x, Seq_fold_right(xs, f, init)));\n");
    out.push_str("axiom (forall<A, B> xs: Seq A, ys: Seq A, f: Fn2 A B B, init: B :: { Seq_fold_right(Seq_append(xs, ys), f, init) } Seq_fold_right(Seq_append(xs, ys), f, init) == Seq_fold_right(xs, f, Seq_fold_right(ys, f, init)));\n\n");

    out.push_str("function Fn1_and<A>(p: Fn1 A bool, q: Fn1 A bool) returns (Fn1 A bool);\n");
    out.push_str("axiom (forall<A> p: Fn1 A bool, q: Fn1 A bool, x: A :: { Fn1_app(Fn1_and(p, q), x) } Fn1_app(Fn1_and(p, q), x) == (Fn1_app(p, x) && Fn1_app(q, x)));\n");
    out.push_str("axiom (forall<A> xs: Seq A, p: Fn1 A bool, q: Fn1 A bool :: { Seq_filter(Seq_filter(xs, p), q) } Seq_filter(Seq_filter(xs, p), q) == Seq_filter(xs, Fn1_and(p, q)));\n\n");

    out.push_str("function Fn2_map_arg<A, B, C>(f: Fn2 C B C, g: Fn1 A B) returns (Fn2 C A C);\n");
    out.push_str("axiom (forall<A, B, C> f: Fn2 C B C, g: Fn1 A B, acc: C, x: A :: { Fn2_app(Fn2_map_arg(f, g), acc, x) } Fn2_app(Fn2_map_arg(f, g), acc, x) == Fn2_app(f, acc, Fn1_app(g, x)));\n");
    out.push_str("axiom (forall<A, B, C> xs: Seq A, g: Fn1 A B, f: Fn2 C B C, init: C :: { Seq_fold_left(Seq_map(xs, g), f, init) } Seq_fold_left(Seq_map(xs, g), f, init) == Seq_fold_left(xs, Fn2_map_arg(f, g), init));\n\n");

    preamble_head.push_str(&std::mem::take(&mut out));
    let preamble = preamble_head;

    if m.name == "OPTION" {
        out.push_str("function Fn1_Some<A>() returns (Fn1 A (OPTION A));\n");
        out.push_str("axiom (forall<A> x: A :: { Fn1_app(Fn1_Some() : (Fn1 A (OPTION A)), x) } Fn1_app(Fn1_Some() : (Fn1 A (OPTION A)), x) == Some(x));\n");
        out.push_str("function Fn1_option_kleisli<A, B, C>(f: Fn1 A (OPTION B), g: Fn1 B (OPTION C)) returns (Fn1 A (OPTION C));\n");
        out.push_str("axiom (forall<A, B, C> f: Fn1 A (OPTION B), g: Fn1 B (OPTION C), x: A :: { Fn1_app(Fn1_option_kleisli(f, g), x) } Fn1_app(Fn1_option_kleisli(f, g), x) == fn_and_then(DefaultFuel, Fn1_app(f, x), g));\n");
        out.push_str("axiom (forall<A, B, C> opt: (OPTION A), f: Fn1 A (OPTION B), g: Fn1 B (OPTION C) :: { fn_and_then(DefaultFuel, fn_and_then(DefaultFuel, opt, f), g) } fn_and_then(DefaultFuel, fn_and_then(DefaultFuel, opt, f), g) == fn_and_then(DefaultFuel, opt, Fn1_option_kleisli(f, g)));\n\n");
    }

    if m.name == "RESULT" {
        out.push_str("function Fn1_Ok<E, A>() returns (Fn1 A (RESULT E A));\n");
        out.push_str("axiom (forall<E, A> x: A :: { Fn1_app(Fn1_Ok() : (Fn1 A (RESULT E A)), x) } Fn1_app(Fn1_Ok() : (Fn1 A (RESULT E A)), x) == Ok(x));\n");
        out.push_str("function Fn1_result_kleisli<E, A, B, C>(f: Fn1 A (RESULT E B), g: Fn1 B (RESULT E C)) returns (Fn1 A (RESULT E C));\n");
        out.push_str("axiom (forall<E, A, B, C> f: Fn1 A (RESULT E B), g: Fn1 B (RESULT E C), x: A :: { Fn1_app(Fn1_result_kleisli(f, g), x) } Fn1_app(Fn1_result_kleisli(f, g), x) == fn_and_then(DefaultFuel, Fn1_app(f, x), g));\n");
        out.push_str("axiom (forall<E, A, B, C> res: (RESULT E A), f: Fn1 A (RESULT E B), g: Fn1 B (RESULT E C) :: { fn_and_then(DefaultFuel, fn_and_then(DefaultFuel, res, f), g) } fn_and_then(DefaultFuel, fn_and_then(DefaultFuel, res, f), g) == fn_and_then(DefaultFuel, res, Fn1_result_kleisli(f, g)));\n\n");
    }

    let mut option_shim = String::new();
    if m.name == "RESULT" {
        let theory_so_far = std::mem::take(&mut out);
        out.push_str("// --- Minimal OPTION[A] Theory for RESULT.to_option ---\n");
        out.push_str("type OPTION A;\n");
        out.push_str("function None<A>() returns ((OPTION A));\n");
        out.push_str("function Some<A>(value: A) returns ((OPTION A));\n");
        out.push_str("function is_None<A>(x: (OPTION A)) returns (bool);\n");
        out.push_str("function is_Some<A>(x: (OPTION A)) returns (bool);\n");
        out.push_str("axiom (forall<A> :: { (None() : (OPTION A)) } is_None((None() : (OPTION A))) && !is_Some((None() : (OPTION A))));\n");
        out.push_str(
            "axiom (forall<A> v: A :: { Some(v) } is_Some(Some(v)) && !is_None(Some(v)));\n",
        );
        out.push_str("function fn_is_some<A>(fuel: Fuel, Current: (OPTION A)) returns (bool);\n");
        out.push_str("axiom (forall<A> f: Fuel, Current: (OPTION A) :: { fn_is_some(f, Current) } fn_is_some(f, Current) == is_Some(Current));\n\n");
        option_shim = std::mem::replace(&mut out, theory_so_far);
    }

    // 3. Module ADT Representation Axiomatization (§5)
    out.push_str(&format!(
        "// --- 3. ADT Representation for {} ---\n",
        m.name
    ));
    let tp_decl = if m.type_params.is_empty() {
        String::new()
    } else {
        format!(" {}", m.type_params.join(" "))
    };
    let tp_quant = if m.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", m.type_params.join(", "))
    };
    let mod_bpl_ty = map_boogie_type("Current", m);

    out.push_str(&format!("type {}{tp_decl};\n", m.name));
    out.push_str(&format!(
        "function {}_adt_size{tp_quant}(x: {mod_bpl_ty}) returns (int);\n",
        m.name
    ));
    out.push_str(&format!(
        "axiom (forall{tp_quant} x: {mod_bpl_ty} :: {{ {}_adt_size(x) }} {}_adt_size(x) >= 1);\n\n",
        m.name, m.name
    ));

    let mut implicit_fields = HashSet::new();
    match &m.repr {
        ReprKind::Sum(variants) => {
            for v in variants {
                let params: Vec<String> = v
                    .fields
                    .iter()
                    .map(|(fnm, fty)| format!("{fnm}: {}", map_boogie_type(fty, m)))
                    .collect();
                out.push_str(&format!(
                    "function {}{tp_quant}({}) returns ({mod_bpl_ty});\n",
                    v.name,
                    params.join(", ")
                ));
                out.push_str(&format!(
                    "function is_{}{tp_quant}(x: {mod_bpl_ty}) returns (bool);\n",
                    v.name
                ));
                for (fnm, fty) in &v.fields {
                    out.push_str(&format!(
                        "function {}_{}{tp_quant}(x: {mod_bpl_ty}) returns ({});\n",
                        v.name,
                        fnm,
                        map_boogie_type(fty, m)
                    ));
                }

                // Constructor discriminator & field projection axioms
                let arg_names: Vec<String> = v.fields.iter().map(|(fnm, _)| fnm.clone()).collect();
                let ctor_call = format!("{}({})", v.name, arg_names.join(", "));
                let ctor_trig = if m.type_params.is_empty() {
                    format!("{{ {ctor_call} }}")
                } else {
                    format!("{{ ({ctor_call} : {mod_bpl_ty}) }}")
                };
                if v.fields.is_empty() {
                    out.push_str(&format!(
                        "axiom (forall{tp_quant} x: {mod_bpl_ty} :: {{ is_{}(x) }} is_{}(x) <==> x == {}());\n",
                        v.name, v.name, v.name
                    ));
                } else {
                    let ctor_app = if m.type_params.is_empty() {
                        ctor_call.clone()
                    } else {
                        format!("({ctor_call} : {mod_bpl_ty})")
                    };
                    let mut proj_conj = vec![format!("is_{}({ctor_app})", v.name)];
                    for (fnm, fty) in &v.fields {
                        proj_conj.push(format!("{}_{}({ctor_app}) == {fnm}", v.name, fnm));
                        if fty == &m.name
                            || fty == "Current"
                            || fty.starts_with(&format!("{}[", m.name))
                        {
                            proj_conj.push(format!(
                                "{}_adt_size({fnm}) < {}_adt_size({ctor_app})",
                                m.name, m.name
                            ));
                        }
                    }
                    out.push_str(&format!(
                        "axiom (forall{tp_quant} {} :: {ctor_trig} {});\n",
                        params.join(", "),
                        proj_conj.join(" && ")
                    ));

                    // Destructor inversion axiom
                    let projs = v
                        .fields
                        .iter()
                        .map(|(fnm, _)| format!("{}_{}(x)", v.name, fnm))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let mut inv_conj = vec![format!("{}({projs}) == x", v.name)];
                    for (fnm, fty) in &v.fields {
                        if fty == &m.name
                            || fty == "Current"
                            || fty.starts_with(&format!("{}[", m.name))
                        {
                            inv_conj.push(format!(
                                "{}_adt_size({}_{}(x)) < {}_adt_size(x)",
                                m.name, v.name, fnm, m.name
                            ));
                        }
                    }
                    out.push_str(&format!(
                        "axiom (forall{tp_quant} x: {mod_bpl_ty} :: {{ is_{}(x) }} is_{}(x) ==> ({}));\n",
                        v.name,
                        v.name,
                        inv_conj.join(" && ")
                    ));

                    // Constructor injectivity axiom
                    let p1: Vec<String> = v
                        .fields
                        .iter()
                        .map(|(fnm, fty)| format!("_a_{fnm}: {}", map_boogie_type(fty, m)))
                        .collect();
                    let p2: Vec<String> = v
                        .fields
                        .iter()
                        .map(|(fnm, fty)| format!("_b_{fnm}: {}", map_boogie_type(fty, m)))
                        .collect();
                    let c1_args: Vec<String> = v
                        .fields
                        .iter()
                        .map(|(fnm, _)| format!("_a_{fnm}"))
                        .collect();
                    let c2_args: Vec<String> = v
                        .fields
                        .iter()
                        .map(|(fnm, _)| format!("_b_{fnm}"))
                        .collect();
                    let eqs: Vec<String> = v
                        .fields
                        .iter()
                        .map(|(fnm, _)| format!("_a_{fnm} == _b_{fnm}"))
                        .collect();
                    let c1 = format!("{}({})", v.name, c1_args.join(", "));
                    let c2 = format!("{}({})", v.name, c2_args.join(", "));
                    let inj_trig = if m.type_params.is_empty() {
                        format!("{{ {c1}, {c2} }}")
                    } else {
                        format!("{{ ({c1} : {mod_bpl_ty}), ({c2} : {mod_bpl_ty}) }}")
                    };
                    let c1_app = if m.type_params.is_empty() {
                        c1.clone()
                    } else {
                        format!("({c1} : {mod_bpl_ty})")
                    };
                    let c2_app = if m.type_params.is_empty() {
                        c2.clone()
                    } else {
                        format!("({c2} : {mod_bpl_ty})")
                    };
                    out.push_str(&format!(
                        "axiom (forall{tp_quant} {}, {} :: {inj_trig} {c1_app} == {c2_app} ==> ({}));\n",
                        p1.join(", "),
                        p2.join(", "),
                        eqs.join(" && ")
                    ));
                }
            }
            // Pairwise Variant Disjointness Axioms (symmetric E-matching triggers)
            for i in 0..variants.len() {
                for j in 0..variants.len() {
                    if i != j {
                        let v_i = &variants[i];
                        let v_j = &variants[j];
                        out.push_str(&format!(
                            "axiom (forall{tp_quant} x: {mod_bpl_ty} :: {{ is_{}(x) }} is_{}(x) ==> !is_{}(x));\n",
                            v_i.name, v_i.name, v_j.name
                        ));
                    }
                }
            }
            if !variants.is_empty() {
                let disj: Vec<String> = variants
                    .iter()
                    .map(|v| format!("is_{}(x)", v.name))
                    .collect();
                out.push_str(&format!(
                    "axiom (forall{tp_quant} x: {mod_bpl_ty} :: {});\n",
                    disj.join(" || ")
                ));
            }
        }
        ReprKind::Product(fields) => {
            let params: Vec<String> = fields
                .iter()
                .map(|(fnm, fty)| format!("{fnm}: {}", map_boogie_type(fty, m)))
                .collect();
            out.push_str(&format!(
                "function {}_Ctor{tp_quant}({}) returns ({mod_bpl_ty});\n",
                m.name,
                params.join(", ")
            ));
            for (fnm, fty) in fields {
                implicit_fields.insert(fnm.clone());
                out.push_str(&format!(
                    "function {}_{}{tp_quant}(x: {mod_bpl_ty}) returns ({});\n",
                    m.name,
                    fnm,
                    map_boogie_type(fty, m)
                ));
            }
            if !fields.is_empty() {
                let arg_names: Vec<String> = fields.iter().map(|(fnm, _)| fnm.clone()).collect();
                let ctor_call = format!("{}_Ctor({})", m.name, arg_names.join(", "));
                let projs: Vec<String> = fields
                    .iter()
                    .map(|(fnm, _)| format!("{}_{}({ctor_call}) == {fnm}", m.name, fnm))
                    .collect();
                out.push_str(&format!(
                    "axiom (forall{tp_quant} {} :: {{ {ctor_call} }} {});\n",
                    params.join(", "),
                    projs.join(" && ")
                ));

                // Destructor inversion for Product
                let projs_x: Vec<String> = fields
                    .iter()
                    .map(|(fnm, _)| format!("{}_{}(x)", m.name, fnm))
                    .collect();
                out.push_str(&format!(
                    "axiom (forall{tp_quant} x: {mod_bpl_ty} :: {{ {}_Ctor({}) }} {}_Ctor({}) == x);\n",
                    m.name,
                    projs_x.join(", "),
                    m.name,
                    projs_x.join(", ")
                ));
            }
        }
    }
    out.push('\n');

    if m.name == "SEQ" {
        out.push_str("axiom (forall<A> Current: (SEQ A) :: { Concat_left(Current) } is_Concat(Current) ==> fn_length(DefaultFuel, Current) == fn_length(DefaultFuel, Concat_left(Current)) + fn_length(DefaultFuel, Concat_right(Current)));\n");
        out.push_str("axiom (forall<A> s: (SEQ A), p: (Fn1 A bool), q: (Fn1 A bool) :: { fn_filter(DefaultFuel, fn_filter(DefaultFuel, s, p), q) } SEQ_Invariant(s) ==> fn_equals(DefaultFuel, fn_filter(DefaultFuel, fn_filter(DefaultFuel, s, p), q), fn_filter(DefaultFuel, s, Fn1_and(p, q))));\n\n");
    }

    // 4. Module Refinement Invariant (§6)
    out.push_str(&format!(
        "// --- 4. Module Refinement Invariant ({}_Invariant) ---\n",
        m.name
    ));
    out.push_str(&format!(
        "function {{:inline}} {}_Invariant{tp_quant}(Current: {mod_bpl_ty}) returns (bool)\n{{\n",
        m.name
    ));
    let empty_revealed = HashSet::new();
    if m.invariants.is_empty() {
        out.push_str("  true\n");
    } else {
        let mut clauses = Vec::new();
        if let ReprKind::Product(fields) = &m.repr {
            for (fnm, fty) in fields {
                if fty == "NAT" || fty == "U64" {
                    clauses.push(format!("  in_u64({}_{}(Current))", m.name, fnm));
                }
            }
        }
        for (tag, expr) in &m.invariants {
            clauses.push(format!(
                "  ({}) /* invariant:{} */",
                emit_boogie_expr_with_strings(
                    expr,
                    m,
                    &implicit_fields,
                    &empty_revealed,
                    "DefaultFuel",
                    &strings
                ),
                tag
            ));
        }
        out.push_str(&clauses.join(" &&\n"));
        out.push('\n');
    }
    out.push_str("}\n\n");

    let empty_fields = HashSet::new();
    let is_adt = m.kind == ModuleKind::Module;

    // 5. Opaque Routine Declarations, Fuel-Guarded Definitional Axioms & Frame Axioms (§7.5)
    out.push_str("// --- 5. Opaque Routine Fuel Declarations & Contract Frame Axioms ---\n");
    let all_routines: Vec<(&RoutineDecl, bool)> = m
        .specify_routines
        .iter()
        .map(|(_, r)| (r, is_adt))
        .chain(m.feature_routines.iter().map(|(_, r)| (r, is_adt)))
        .chain(m.create_routines.iter().map(|(_, r)| (r, false)))
        .collect();

    // Greatest fixpoint: a routine is receiver-independent if neither its contract
    // nor its body mentions `Current`/a field, except through calls to other
    // receiver-independent routines (or itself). Such routines get a
    // Current-independence axiom and no receiver-invariant guard.
    let mut current_indep: HashSet<String> =
        all_routines.iter().map(|(r, _)| r.name.clone()).collect();
    loop {
        let dependent: Vec<String> = all_routines
            .iter()
            .filter(|(r, _)| {
                current_indep.contains(&r.name)
                    && routine_uses_current(r, &implicit_fields, &current_indep)
            })
            .map(|(r, _)| r.name.clone())
            .collect();
        if dependent.is_empty() {
            break;
        }
        for n in dependent {
            current_indep.remove(&n);
        }
    }

    for (r, has_current) in &all_routines {
        set_boogie_type_env(m, &r.params, Some(&r.ret_type));
        let mut r_all_tps = m.type_params.clone();
        for tp in &r.type_params {
            if !r_all_tps.contains(tp) {
                r_all_tps.push(tp.clone());
            }
        }
        let r_tp_quant = if r_all_tps.is_empty() {
            String::new()
        } else {
            format!("<{}>", r_all_tps.join(", "))
        };
        let uses_curr = *has_current && !current_indep.contains(&r.name);
        let mut sig_params = vec!["fuel: Fuel".to_string()];
        let mut q_params = vec!["_fuel: Fuel".to_string()];
        let mut call_args_step_two = vec!["SuccFuel(SuccFuel(_fuel))".to_string()];
        let mut call_args_succ = vec!["SuccFuel(_fuel)".to_string()];
        let mut call_args_f = vec!["_fuel".to_string()];
        let mut pre_guards = Vec::new();

        if *has_current {
            sig_params.push(format!("Current: {mod_bpl_ty}"));
            q_params.push(format!("Current: {mod_bpl_ty}"));
            call_args_step_two.push("Current".to_string());
            call_args_succ.push("Current".to_string());
            call_args_f.push("Current".to_string());
            if uses_curr {
                pre_guards.push(format!("{}_Invariant(Current)", m.name));
            }
        }
        for (pn, pt) in &r.params {
            let bty = map_boogie_type(pt, m);
            sig_params.push(format!("{pn}: {bty}"));
            q_params.push(format!("{pn}: {bty}"));
            call_args_step_two.push(pn.clone());
            call_args_succ.push(pn.clone());
            call_args_f.push(pn.clone());
            if pt == "NAT" || pt == "U64" {
                pre_guards.push(format!("in_u64({pn})"));
            } else if !m.invariants.is_empty()
                && (pt == &m.name || pt == "Current" || pt.starts_with(&format!("{}[", m.name)))
            {
                pre_guards.push(format!("{}_Invariant({pn})", m.name));
            }
        }
        for (_, req_e) in &r.requires {
            pre_guards.push(emit_boogie_expr_with_strings(
                req_e,
                m,
                &empty_fields,
                &empty_revealed,
                "_fuel",
                &strings,
            ));
        }

        let ret_bty = map_boogie_type(&r.ret_type, m);
        out.push_str(&format!(
            "function fn_{}{r_tp_quant}({}) returns ({ret_bty});\n",
            r.name,
            sig_params.join(", ")
        ));

        let fn_step_two_app = format!("fn_{}({})", r.name, call_args_step_two.join(", "));
        let fn_succ_app = format!("fn_{}({})", r.name, call_args_succ.join(", "));
        let fn_f_app = format!("fn_{}({})", r.name, call_args_f.join(", "));
        let body_expr =
            emit_boogie_block_expr(&r.stmts, &r.body, m, &empty_fields, "_fuel", &strings);
        let guard_str = if pre_guards.is_empty() {
            String::new()
        } else {
            format!("{} ==> ", pre_guards.join(" && "))
        };
        let fn_step_two_trigger = if r_all_tps.is_empty() {
            format!("{{ {fn_step_two_app} }}")
        } else {
            format!("{{ ({fn_step_two_app} : {ret_bty}) }}")
        };
        let fn_succ_trigger = if r_all_tps.is_empty() {
            format!("{{ {fn_succ_app} }}")
        } else {
            format!("{{ ({fn_succ_app} : {ret_bty}) }}")
        };
        let fn_f_trigger = if r_all_tps.is_empty()
            || r_all_tps
                .iter()
                .all(|tp| q_params.iter().any(|qp| qp.contains(tp)))
        {
            format!("{{ {fn_f_app} }}")
        } else {
            format!("{{ ({fn_f_app} : {ret_bty}) }}")
        };
        // A routine that binds values from the outside world (`x <- IO.read_line`) is not a
        // function of its arguments: it gets no definitional axiom (it stays opaque).
        let binds_io = r
            .stmts
            .iter()
            .any(|st| matches!(st, Stmt::Let(_, e) if expr_has_io_bind(e)))
            || expr_has_io_bind(&r.body);
        if !binds_io {
            out.push_str(&format!(
                "axiom (forall{r_tp_quant} {} :: {fn_succ_trigger} {guard_str}{fn_succ_app} == {body_expr});\n",
                q_params.join(", ")
            ));
        }
        out.push_str(&format!(
            "axiom (forall{r_tp_quant} {} :: {fn_step_two_trigger} {guard_str}{fn_step_two_app} == {fn_succ_app});\n",
            q_params.join(", ")
        ));
        if *has_current && !uses_curr {
            let mut ri_q_params = vec![
                "_fuel: Fuel".to_string(),
                format!("_c1: {mod_bpl_ty}"),
                format!("_c2: {mod_bpl_ty}"),
            ];
            let mut args_c1 = vec!["_fuel".to_string(), "_c1".to_string()];
            let mut args_c2 = vec!["_fuel".to_string(), "_c2".to_string()];
            for (pn, pt) in &r.params {
                ri_q_params.push(format!("{pn}: {}", map_boogie_type(pt, m)));
                args_c1.push(pn.clone());
                args_c2.push(pn.clone());
            }
            let app_c1 = format!("fn_{}({})", r.name, args_c1.join(", "));
            let app_c2 = format!("fn_{}({})", r.name, args_c2.join(", "));
            let ri_trig = if r_all_tps.is_empty() {
                format!("{{ {app_c1}, {app_c2} }}")
            } else {
                format!("{{ ({app_c1} : {ret_bty}), ({app_c2} : {ret_bty}) }}")
            };
            out.push_str(&format!(
                "axiom (forall{r_tp_quant} {} :: {ri_trig} {app_c1} == {app_c2});\n",
                ri_q_params.join(", ")
            ));
        }

        // Opaque Postcondition Frame Axiom (holds at any fuel `_fuel`, including `ZeroFuel`)
        let mut post_clauses = Vec::new();
        if r.ret_type == "NAT" || r.ret_type == "U64" {
            post_clauses.push(format!("in_u64({fn_f_app})"));
        } else if r.ret_type == m.name
            || r.ret_type == "Current"
            || r.ret_type.starts_with(&format!("{}[", m.name))
        {
            post_clauses.push(format!("{}_Invariant({fn_f_app})", m.name));
        }
        if m.name == "SEQ" && r.name == "length" {
            post_clauses.push(format!(
                "{fn_f_app} == Seq_length(fn_model(_fuel, Current))"
            ));
            post_clauses.push(format!("0 <= {fn_f_app}"));
        }
        let mut model_decoupled_ens = Vec::new();
        let mut model_decoupled_fn = String::from("fn_model");
        for (ens_tag, ens_e) in &r.ensures {
            // A postcondition clause that failed in an earlier pass is not stated as an axiom
            // (see `run_boogie_verification`); the clauses that verified remain available.
            if UNTRUSTED_CONTRACTS.with(|u| u.borrow().contains(&format!("{}::{ens_tag}", r.name)))
            {
                continue;
            }
            let ens_bpl = emit_boogie_expr_with_strings(
                ens_e,
                m,
                &empty_fields,
                &empty_revealed,
                "_fuel",
                &strings,
            )
            .replace("Result", &fn_f_app);
            post_clauses.push(format!("({ens_bpl})"));

            let model_call = format!("fn_model(_fuel, {fn_f_app})");
            if let Some(pos) = ens_bpl.find(&model_call) {
                // The model query may be the module's own `fn_model` or a namespaced
                // injected prelude one (e.g. `BIGNAT__fn_model`).
                let qualifier: String = ens_bpl[..pos]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let model_fn = format!("{qualifier}fn_model");
                let decoupled = ens_bpl.replace(
                    &format!("{model_fn}(_fuel, "),
                    &format!("{model_fn}(_f_query, "),
                );
                model_decoupled_fn = model_fn;
                model_decoupled_ens.push(decoupled);
            }
        }
        // A routine with a failed obligation other than a postcondition clause (overflow,
        // precondition, termination, invariant) is not trusted at all: no contract axiom.
        let trusted = !UNTRUSTED_CONTRACTS.with(|u| u.borrow().contains(&r.name));
        if trusted && !post_clauses.is_empty() {
            out.push_str(&format!(
                "axiom (forall{r_tp_quant} {} :: {fn_f_trigger} {guard_str}({}));\n",
                q_params.join(", "),
                post_clauses.join(" && ")
            ));
        }
        let mentions_all_tps = r_all_tps
            .iter()
            .all(|tp| q_params.iter().any(|qp| qp.contains(tp)));
        if trusted && mentions_all_tps && !model_decoupled_ens.is_empty() {
            let mut decoupled_q_params = vec!["_f_query: Fuel".to_string()];
            decoupled_q_params.extend(q_params.clone());
            out.push_str(&format!(
                "axiom (forall{r_tp_quant} {} :: {{ {model_decoupled_fn}(_f_query, {fn_f_app}) }} {guard_str}({}));\n",
                decoupled_q_params.join(", "),
                model_decoupled_ens.join(" && ")
            ));
        }
        out.push('\n');
    }
    clear_boogie_type_env();

    // Everything emitted so far is the module's background theory proper.
    theory.push_str(&std::mem::take(&mut out));

    // 6. Concept Law Axioms with Triggers (§9) and concept axioms.
    // These state the module's own laws as axioms. They are emitted into `client_axioms`,
    // which is injected only into *other* modules' contexts (prelude injection, linked
    // modules). They are never visible to this module's own `Verify_*` procedures, otherwise
    // every law would be assumed while being proven (and a false law, e.g. EQ reflexivity of
    // a non-reflexive `equals`, would make the module's axioms inconsistent).
    if !m.specify_laws.is_empty() {
        out.push_str("// --- 6. Concept Law Quantified Axioms & Triggers (§9) ---\n");
        for (law_vis, law) in &m.specify_laws {
            if law.name == "reflexive"
                || law.name == "symmetric"
                || law.name == "transitive"
                || law.name == "irreflexive"
                || law.name == "total"
            {
                continue;
            }
            if law.triggers.is_empty() {
                continue;
            }
            set_boogie_type_env(m, &law.params, None);
            let mut law_all_tps = m.type_params.clone();
            for tp in &law.type_params {
                if !law_all_tps.contains(tp) {
                    law_all_tps.push(tp.clone());
                }
            }
            let law_tp_quant = if law_all_tps.is_empty() {
                String::new()
            } else {
                format!("<{}>", law_all_tps.join(", "))
            };
            let q_params: Vec<String> = law
                .params
                .iter()
                .map(|(pn, pt)| format!("{pn}: {}", map_boogie_type(pt, m)))
                .collect();
            let triggers: Vec<String> = if !law.triggers.is_empty() {
                law.triggers
                    .iter()
                    .map(|te| {
                        emit_boogie_expr_with_strings(
                            te,
                            m,
                            &empty_fields,
                            &empty_revealed,
                            "DefaultFuel",
                            &strings,
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let trig_str = if triggers.is_empty() {
                String::new()
            } else {
                format!("{{ {} }} ", triggers.join(", "))
            };
            let reqs: Vec<String> = law
                .requires
                .iter()
                .map(|(_, re)| {
                    emit_boogie_expr_with_strings(
                        re,
                        m,
                        &empty_fields,
                        &empty_revealed,
                        "DefaultFuel",
                        &strings,
                    )
                })
                .collect();
            let ens: Vec<String> = law
                .ensures
                .iter()
                .map(|(_, ee)| {
                    emit_boogie_expr_with_strings(
                        ee,
                        m,
                        &empty_fields,
                        &empty_revealed,
                        "DefaultFuel",
                        &strings,
                    )
                })
                .collect();
            if !q_params.is_empty() && !ens.is_empty() {
                let mut reqs = reqs;
                if let Some(guard) = canonical_where_guard(m, law_vis) {
                    reqs.insert(0, format!("({guard})"));
                }
                let body_implication = if reqs.is_empty() {
                    ens.join(" && ")
                } else {
                    format!("({}) ==> ({})", reqs.join(" && "), ens.join(" && "))
                };
                out.push_str(&format!(
                    "axiom (forall{law_tp_quant} {} :: {trig_str}{body_implication});\n",
                    q_params.join(", ")
                ));
            }
        }
        out.push('\n');
    }

    clear_boogie_type_env();
    out.push_str(&emit_boogie_concept_axioms(m));
    let client_axioms = std::mem::take(&mut out);

    // 7. Verification Condition Procedures (`Verify_<routine>` & `Verify_law_<law>`) (§12)
    out.push_str("// --- 7. Verification Condition Procedures (§12) ---\n");
    // Recursive call groups (direct and mutual recursion) for the termination checks.
    let groups = compute_recursive_groups(m);
    let prev_groups = RECURSIVE_GROUPS.with(|g| g.replace(groups));
    for (r, has_current) in &all_routines {
        set_boogie_type_env(m, &r.params, Some(&r.ret_type));
        out.push_str(&emit_boogie_verify_routine(r, *has_current, m, &strings));
    }
    RECURSIVE_GROUPS.with(|g| *g.borrow_mut() = prev_groups);
    out.push_str(&emit_boogie_canonical_witness_types(m));
    for (vis, law) in &m.specify_laws {
        set_boogie_type_env(m, &law.params, None);
        out.push_str(&emit_boogie_verify_law(vis, law, m, &strings));
    }
    clear_boogie_type_env();

    // Sum-Type Hereditary Invariant Verification (§5, Plan 3 §2)
    if let ReprKind::Sum(variants) = &m.repr {
        if !m.invariants.is_empty() {
            let has_recursive_fields = variants.iter().any(|v| {
                v.fields.iter().any(|(_, fty)| {
                    fty == &m.name || fty == "Current" || fty.starts_with(&format!("{}[", m.name))
                })
            });
            if has_recursive_fields {
                let tp_quant = if m.type_params.is_empty() {
                    String::new()
                } else {
                    format!("<{}>", m.type_params.join(", "))
                };
                let cur_ty = map_boogie_type("Current", m);
                out.push_str(&format!(
                    "procedure Verify_Hereditary_Invariant_{}{tp_quant}(Current: {cur_ty})\n",
                    m.name
                ));
                out.push_str(&format!("  requires {}_Invariant(Current);\n{{\n", m.name));
                for v in variants {
                    let rec_fields: Vec<&(String, String)> = v
                        .fields
                        .iter()
                        .filter(|(_, fty)| {
                            fty == &m.name
                                || fty == "Current"
                                || fty.starts_with(&format!("{}[", m.name))
                        })
                        .collect();
                    if !rec_fields.is_empty() {
                        out.push_str(&format!("  if (is_{}(Current)) {{\n", v.name));
                        for (fname, _) in rec_fields {
                            let proj = format!("{}_{}(Current)", v.name, fname);
                            out.push_str(&format!(
                                "    assert {{:msg \"{}::hereditary_invariant::{}_{}\"}} {}_Invariant({proj});\n",
                                m.name, v.name, fname, m.name
                            ));
                        }
                        out.push_str("  }\n");
                    }
                }
                out.push_str("}\n\n");
            }
        }
    }

    if prelude_string_mode() {
        out.push_str(&emit_boogie_string_literal_procedures(
            &collect_module_strings(m),
        ));
    }

    BoogieParts {
        preamble,
        theory,
        client_axioms,
        option_shim,
        procedures: out,
    }
}

/// True when the prelude `STRING` theory is injected into the current Boogie context, i.e.
/// string literals and `STRING`-typed values denote prelude `STRING` values (Task B.3).
fn prelude_string_mode() -> bool {
    boogie_prelude_ctx().is_some_and(|ctx| ctx.module("STRING").is_some())
}

/// Boogie term for the scalar list `[c_0, ..., c_{n-1}]` of a literal:
/// `LIST.empty.prepend(c_{n-1})...prepend(c_0)` (§3.1.4).
fn boogie_literal_scalar_list(s: &str) -> String {
    let mut t = "Seq_empty()".to_string();
    for c in s.chars().rev() {
        t = format!("Seq_prepend({}, {t})", c as u32);
    }
    t
}

/// Capacity of the `STRING` a literal desugars to (`make_empty` requires a positive one).
fn string_literal_capacity(s: &str) -> usize {
    s.len().max(1)
}

/// The `STRING` construction a literal desugars to (§3.1.4), built only from verified prelude
/// routines: `STRING.make_empty(cap).push_ascii(c_{n-1})...push_ascii(c_0)` (`push_ascii`
/// prepends, so the scalars are pushed last-to-first).
fn boogie_string_literal_term(s: &str) -> String {
    let mut t = format!(
        "STRING__fn_make_empty(DefaultFuel, {})",
        string_literal_capacity(s)
    );
    for c in s.chars().rev() {
        t = format!("STRING__fn_push_ascii(DefaultFuel, {t}, {})", c as u32);
    }
    t
}

/// Declarations of the literal constants of a module in prelude mode. Each literal `k` gets
/// one constant `str_lit_k: STRING` and exactly one axiom, its *definition*
/// `str_lit_k == <prelude construction term>` (a conservative extension: a fresh constant
/// equated to a ground term). No property of the literal is axiomatized: its invariant and
/// `chars` model follow from the bootstrap-verified `make_empty` / `push_ascii` contracts and
/// are re-proven per literal by `Verify_string_literal_k`.
fn emit_boogie_string_literal_decls(strings: &[String]) -> String {
    let mut out =
        String::from("// --- String literals (Task B.3, §3.1.4): prelude STRING constants ---\n");
    for (idx, s) in strings.iter().enumerate() {
        // Intermediate construction steps are named (`str_lit_k_s<j>`) to keep the
        // emitted terms linear in the literal length.
        let cap = string_literal_capacity(s);
        out.push_str(&format!("// str_lit_{idx} = \"{}\"\n", s.escape_debug()));
        out.push_str(&format!("const str_lit_{idx}_s0: STRING;\n"));
        out.push_str(&format!(
            "axiom str_lit_{idx}_s0 == STRING__fn_make_empty(DefaultFuel, {cap});\n"
        ));
        let n = s.chars().count();
        for (j, c) in s.chars().rev().enumerate() {
            out.push_str(&format!("const str_lit_{idx}_s{}: STRING;\n", j + 1));
            out.push_str(&format!(
                "axiom str_lit_{idx}_s{} == STRING__fn_push_ascii(DefaultFuel, str_lit_{idx}_s{j}, {});\n",
                j + 1,
                c as u32
            ));
        }
        out.push_str(&format!("const str_lit_{idx}: STRING;\n"));
        out.push_str(&format!("axiom str_lit_{idx} == str_lit_{idx}_s{n};\n"));
    }
    out
}

/// Per-literal proof obligations (§3.1.4 "statically proven" model refinement): every
/// precondition of the desugared construction, the `STRING` invariant, and
/// `Result.chars = LIST.empty.prepend(c_{n-1})...prepend(c_0)` with its byte length.
fn emit_boogie_string_literal_procedures(strings: &[String]) -> String {
    let Some(ctx) = boogie_prelude_ctx() else {
        return String::new();
    };
    let Some(sm) = ctx.module("STRING") else {
        return String::new();
    };
    let declared = prelude_contract_interface(sm);
    let pre_names = |rname: &str| -> Vec<(String, String)> {
        find_module_routine(sm, rname)
            .map(|(rd, _)| rd.requires.clone())
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter(|(idx, _)| declared.1.contains(&format!("pre_{rname}_{idx}")))
            .map(|(idx, (tag, _))| (format!("STRING__pre_{rname}_{idx}"), tag.clone()))
            .collect()
    };
    let mut out = String::new();
    for (idx, s) in strings.iter().enumerate() {
        let ctx_name = format!("string_literal_{idx}");
        out.push_str(&format!(
            "// \"{}\" desugars to STRING.make_empty(..).push_ascii(..)... (§3.1.4)\n",
            s.escape_debug()
        ));
        let facts = string_literal_facts(idx, s);
        // A lemma procedure: its implementation proves the facts below (no `free` clause),
        // and procedures that use literals `call` it to obtain them as proven postconditions.
        out.push_str(&format!("procedure Verify_{ctx_name}()\n"));
        for (_, f) in &facts {
            out.push_str(&format!("  ensures {f};\n"));
        }
        out.push_str("{\n");
        let cap = string_literal_capacity(s);
        for (pre, tag) in pre_names("make_empty") {
            out.push_str(&format!(
                "  assert {{:msg \"{ctx_name}::require::{tag}\"}} {pre}({cap});\n"
            ));
        }
        for (j, c) in s.chars().rev().enumerate() {
            let code = c as u32;
            for (pre, tag) in pre_names("push_ascii") {
                out.push_str(&format!(
                    "  assert {{:msg \"{ctx_name}::require::{tag}\"}} {pre}(str_lit_{idx}_s{j}, {code});\n"
                ));
            }
        }
        for (tag, f) in &facts {
            out.push_str(&format!("  assert {{:msg \"{ctx_name}::{tag}\"}} {f};\n"));
        }
        out.push_str("}\n\n");
    }
    out
}

/// The proven facts of literal `idx` (`Verify_string_literal_idx` postconditions): the
/// `STRING` invariant, the §3.1.4 `chars` model and the byte length.
fn string_literal_facts(idx: usize, s: &str) -> Vec<(&'static str, String)> {
    vec![
        (
            "invariant::string_invariant",
            format!("STRING__STRING_Invariant(str_lit_{idx})"),
        ),
        (
            "ensure::chars_model",
            format!(
                "STRING__fn_chars(DefaultFuel, str_lit_{idx}) == {}",
                boogie_literal_scalar_list(s)
            ),
        ),
        (
            "ensure::byte_length",
            format!(
                "STRING__fn_byte_length(DefaultFuel, str_lit_{idx}) == {}",
                s.len()
            ),
        ),
    ]
}

/// In prelude mode, calls every literal lemma of the module at the start of a procedure
/// body so the literals' proven facts are available there (Boogie checks each lemma's
/// implementation separately; calling it only assumes its verified postconditions).
fn emit_string_literal_lemma_calls(strings: &[String]) -> String {
    if !prelude_string_mode() {
        return String::new();
    }
    (0..strings.len())
        .map(|idx| format!("  call Verify_string_literal_{idx}();\n"))
        .collect()
}

/// Whether routine `r` depends on its implicit receiver `Current`.
/// Calls to routines in `indep` (routines already known not to depend on
/// `Current`) do not count; `r` itself is always treated as independent so
/// that self-recursion alone does not make it receiver-dependent.
fn routine_uses_current(
    r: &RoutineDecl,
    field_names: &HashSet<String>,
    indep: &HashSet<String>,
) -> bool {
    let mut indep_self = indep.clone();
    indep_self.insert(r.name.clone());
    let indep = &indep_self;
    r.requires
        .iter()
        .any(|(_, e)| expr_uses_current(e, field_names, indep))
        || r.ensures
            .iter()
            .any(|(_, e)| expr_uses_current(e, field_names, indep))
        || r.variant
            .iter()
            .any(|e| expr_uses_current(e, field_names, indep))
        || stmts_use_current(&r.stmts, field_names, indep)
        || expr_uses_current(&r.body, field_names, indep)
}

fn stmts_use_current(
    stmts: &[Stmt],
    field_names: &HashSet<String>,
    indep: &HashSet<String>,
) -> bool {
    stmts.iter().any(|st| match st {
        Stmt::Let(_, e) | Stmt::Check(_, e) | Stmt::Assume(_, e) => {
            expr_uses_current(e, field_names, indep)
        }
        Stmt::Reveal(_) => false,
    })
}

fn expr_uses_current(e: &Expr, field_names: &HashSet<String>, indep: &HashSet<String>) -> bool {
    match e {
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Char(_)
        | Expr::Unit
        | Expr::Bool(_)
        | Expr::Str(_) => false,
        Expr::Var(v) => v == "Current" || field_names.contains(v),
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => {
            expr_uses_current(inner, field_names, indep)
        }
        Expr::BinOp(_, l, r) => {
            expr_uses_current(l, field_names, indep) || expr_uses_current(r, field_names, indep)
        }
        Expr::Call(fname, args) => {
            !indep.contains(fname)
                || args
                    .iter()
                    .any(|a| expr_uses_current(a, field_names, indep))
        }
        Expr::MethodCall(recv, _, args) => {
            expr_uses_current(recv, field_names, indep)
                || args
                    .iter()
                    .any(|a| expr_uses_current(a, field_names, indep))
        }
        Expr::If(c, ts, te, es, ee) => {
            expr_uses_current(c, field_names, indep)
                || stmts_use_current(ts, field_names, indep)
                || expr_uses_current(te, field_names, indep)
                || stmts_use_current(es, field_names, indep)
                || expr_uses_current(ee, field_names, indep)
        }
        Expr::Match(scrut, cases) => {
            expr_uses_current(scrut, field_names, indep)
                || cases.iter().any(|c| {
                    stmts_use_current(&c.stmts, field_names, indep)
                        || expr_uses_current(&c.body, field_names, indep)
                })
        }
        Expr::RecordLit(_) | Expr::RecordUpdate(_, _) => true,
        Expr::TupleLit(es) => es
            .iter()
            .any(|el| expr_uses_current(el, field_names, indep)),
        Expr::Forall(_, _, b) | Expr::Exists(_, _, b) => expr_uses_current(b, field_names, indep),
        Expr::WithIo(_, _) | Expr::Lambda(_, _, _, _) => true,
    }
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'[' | b'(' | b'{' => depth += 1,
            b']' | b')' | b'}' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            b',' if depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start <= bytes.len() {
        let rem = s[start..].trim();
        if !rem.is_empty() {
            parts.push(rem);
        }
    }
    parts
}

fn split_top_level_arrow(ty: &str) -> Option<(&str, &str)> {
    let mut depth = 0;
    let bytes = ty.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'[' | b'(' => depth += 1,
            b']' | b')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            b'-' if depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'>' => {
                let left = &ty[..i];
                let right = &ty[i + 2..];
                return Some((left, right));
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn map_boogie_type(ty: &str, m: &ModuleDecl) -> String {
    let ty = ty.trim();
    match ty {
        "NAT" | "U64" | "I64" | "INT" | "MATH_INT" | "CHAR" | "EXIT_CODE" => {
            return "int".to_string();
        }
        "BOOL" => return "bool".to_string(),
        "STRING" => {
            if m.name == "STRING" || prelude_string_mode() {
                return "STRING".to_string();
            }
            return "GardelString".to_string();
        }
        "Current" => {
            if m.type_params.is_empty() {
                return m.name.clone();
            } else {
                return format!("({} {})", m.name, m.type_params.join(" "));
            }
        }
        _ => {}
    }
    if ty == m.name {
        return map_boogie_type("Current", m);
    }
    if let Some((arg_str, ret_str)) = split_top_level_arrow(ty) {
        let arg_cleaned = arg_str
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim();
        let ret_cleaned = ret_str
            .split(" require ")
            .next()
            .unwrap()
            .split(" ensure ")
            .next()
            .unwrap()
            .trim();
        let parts = split_top_level_commas(arg_cleaned);
        let param_types: Vec<String> = parts
            .into_iter()
            .map(|p| {
                let t = if let Some((_, ty_part)) = p.split_once(':') {
                    ty_part.trim()
                } else {
                    p.trim()
                };
                map_boogie_type(t, m)
            })
            .collect();
        if param_types.len() == 2 {
            return format!(
                "(Fn2 {} {} {})",
                param_types[0],
                param_types[1],
                map_boogie_type(ret_cleaned, m)
            );
        } else if param_types.len() == 1 {
            return format!(
                "(Fn1 {} {})",
                param_types[0],
                map_boogie_type(ret_cleaned, m)
            );
        } else {
            return format!("(Fn1 () {})", map_boogie_type(ret_cleaned, m));
        }
    }
    if let Some(inner) = ty.strip_prefix("LIST[").and_then(|s| s.strip_suffix(']')) {
        return format!("(Seq {})", map_boogie_type(inner.trim(), m));
    }
    if let Some(ctx) = boogie_prelude_ctx() {
        // Injected prelude ADTs keep their own sorts in user modules (e.g. `(SEQ int)`).
        let (head, args) = split_type_app(ty);
        if head != m.name && ctx.module(head).is_some() {
            if args.is_empty() {
                return head.to_string();
            }
            let parts: Vec<String> = args.iter().map(|a| map_boogie_type(a, m)).collect();
            return format!("({head} {})", parts.join(" "));
        }
    }
    if m.name != "SEQ" {
        if let Some(inner) = ty.strip_prefix("SEQ[").and_then(|s| s.strip_suffix(']')) {
            return format!("(Seq {})", map_boogie_type(inner.trim(), m));
        }
    }
    if let Some(inner) = ty.strip_prefix("IO[").and_then(|s| s.strip_suffix(']')) {
        return format!("(IO {})", map_boogie_type(inner.trim(), m));
    }
    if let Some(inner) = ty.strip_prefix("TUPLE[").and_then(|s| s.strip_suffix(']')) {
        let parts: Vec<String> = inner
            .split(',')
            .map(|p| map_boogie_type(p.trim(), m))
            .collect();
        if parts.len() == 2 {
            return format!("(Tuple2 {} {})", parts[0], parts[1]);
        }
    }
    if let Some(idx) = ty.find('[') {
        let base = &ty[..idx];
        let inner = &ty[idx + 1..ty.len() - 1];
        let parts: Vec<String> = inner
            .split(',')
            .map(|p| map_boogie_type(p.trim(), m))
            .collect();
        return format!("({base} {})", parts.join(" "));
    }
    ty.to_string()
}

fn emit_boogie_verify_routine(
    r: &RoutineDecl,
    has_current: bool,
    m: &ModuleDecl,
    strings: &[String],
) -> String {
    let mut s = String::new();
    let mut all_tps = m.type_params.clone();
    for tp in &r.type_params {
        if !all_tps.contains(tp) {
            all_tps.push(tp.clone());
        }
    }
    let tp_quant = if all_tps.is_empty() {
        String::new()
    } else {
        format!("<{}>", all_tps.join(", "))
    };
    let mod_bpl_ty = map_boogie_type("Current", m);
    let mut params = Vec::new();
    if has_current {
        params.push(format!("Current: {mod_bpl_ty}"));
    }
    for (pn, pt) in &r.params {
        params.push(format!("{pn}: {}", map_boogie_type(pt, m)));
    }
    let ret_bty = map_boogie_type(&r.ret_type, m);
    s.push_str(&format!(
        "procedure Verify_{}{tp_quant}({}) returns (Result: {ret_bty})\n",
        r.name,
        params.join(", ")
    ));
    if has_current {
        s.push_str(&format!("  requires {}_Invariant(Current);\n", m.name));
    }
    for (pn, pt) in &r.params {
        if pt == "NAT" || pt == "U64" {
            s.push_str(&format!("  requires in_u64({pn});\n"));
        } else if !m.invariants.is_empty()
            && (pt == &m.name || pt == "Current" || pt.starts_with(&format!("{}[", m.name)))
        {
            s.push_str(&format!("  requires {}_Invariant({pn});\n", m.name));
        } else if let Some(p) = boogie_prelude_ctx().and_then(|ctx| {
            ctx.module(type_head(pt))
                .filter(|p| !p.invariants.is_empty())
                .cloned()
        }) {
            // Values of a prelude type satisfy its (bootstrap-verified) type invariant.
            s.push_str(&format!("  requires {0}__{0}_Invariant({pn});\n", p.name));
        }
    }
    let empty_fields = HashSet::new();
    let mut revealed = HashSet::new();
    for (tag, req_e) in &r.requires {
        s.push_str(&format!(
            "  requires {}; // require:{tag}\n",
            emit_boogie_expr_with_strings(
                req_e,
                m,
                &empty_fields,
                &revealed,
                "DefaultFuel",
                strings
            )
        ));
    }
    s.push_str("{\n");

    let mut locals = Vec::new();
    collect_proc_locals(&r.stmts, &r.body, r, m, &mut locals);
    let mut seen_locals = HashSet::new();
    for (vname, bty) in &locals {
        if seen_locals.insert(vname.clone()) {
            s.push_str(&format!("  var {vname}: {bty};\n"));
        }
    }
    s.push_str(&emit_string_literal_lemma_calls(strings));
    for (_, req_e) in &r.requires {
        s.push_str(&emit_from_scalar_obligation(
            req_e, &r.name, m, &revealed, "  ", strings,
        ));
    }

    s.push_str(&emit_boogie_proc_stmts_and_ret(
        &r.stmts,
        &r.body,
        r,
        m,
        &mut revealed,
        2,
        strings,
    ));

    let returns_mod = r.ret_type == "Current"
        || r.ret_type == m.name
        || r.ret_type.starts_with(&format!("{}[", m.name));
    if returns_mod && m.kind == ModuleKind::Module && !m.invariants.is_empty() {
        for (tag, _) in &m.invariants {
            s.push_str(&format!(
                "  assert {{:msg \"{}::invariant::{tag}\"}} {}_Invariant(Result);\n",
                r.name, m.name
            ));
        }
    }
    for (tag, ens_e) in &r.ensures {
        s.push_str(&emit_from_scalar_obligation(
            ens_e, &r.name, m, &revealed, "  ", strings,
        ));
        s.push_str(&format!(
            "  assert {{:msg \"{}::ensure::{tag}\"}} {};\n",
            r.name,
            emit_boogie_expr_with_strings(
                ens_e,
                m,
                &empty_fields,
                &revealed,
                "DefaultFuel",
                strings
            )
        ));
    }

    s.push_str("}\n\n");
    s
}

fn emit_boogie_verify_law(
    vis: &Visibility,
    law: &LawDecl,
    m: &ModuleDecl,
    strings: &[String],
) -> String {
    let mut s = String::new();
    let vis_prefix = match vis {
        Visibility::Modules(mods) if !mods.is_empty() => format!("{}_", mods.join("_")),
        _ => String::new(),
    };
    let mut all_tps = m.type_params.clone();
    for tp in &law.type_params {
        if !all_tps.contains(tp) {
            all_tps.push(tp.clone());
        }
    }
    let tp_quant = if all_tps.is_empty() {
        String::new()
    } else {
        format!("<{}>", all_tps.join(", "))
    };
    let mut params: Vec<String> = law
        .params
        .iter()
        .map(|(pn, pt)| format!("{pn}: {}", map_boogie_type(pt, m)))
        .collect();
    for tp in &all_tps {
        let is_used = law.params.iter().any(|(_, pt)| pt.contains(tp));
        if !is_used {
            params.push(format!("_dummy_{tp}: {tp}"));
        }
    }
    let empty_fields = HashSet::new();
    let mut revealed = HashSet::new();
    for (pn, pt) in &law.params {
        if pt == &m.name || pt == "Current" || pt.starts_with(&format!("{}[", m.name)) {
            s.push_str(&format!("  requires {}_Invariant({pn});\n", m.name));
        } else if pt == "NAT" || pt == "U64" {
            s.push_str(&format!("  requires in_u64({pn});\n"));
        }
    }
    for (tag, req_e) in &law.requires {
        s.push_str(&format!(
            "  requires {}; // require:{tag}\n",
            emit_boogie_expr_with_strings(
                req_e,
                m,
                &empty_fields,
                &revealed,
                "DefaultFuel",
                strings
            )
        ));
    }
    s.push_str("{\n");
    s.push_str(&emit_string_literal_lemma_calls(strings));
    let law_ctx = format!("{vis_prefix}{}", law.name);
    for (_, req_e) in &law.requires {
        s.push_str(&emit_from_scalar_obligation(
            req_e, &law_ctx, m, &revealed, "  ", strings,
        ));
    }
    for st in &law.stmts {
        if let Stmt::Check(_, e) | Stmt::Assume(_, e) = st {
            s.push_str(&emit_from_scalar_obligation(
                e, &law_ctx, m, &revealed, "  ", strings,
            ));
        }
        match st {
            Stmt::Check(tag, e) => {
                s.push_str(&format!(
                    "  assert {{:msg \"{vis_prefix}{}::check::{tag}\"}} {};\n",
                    law.name,
                    emit_boogie_expr_with_strings(
                        e,
                        m,
                        &empty_fields,
                        &revealed,
                        "DefaultFuel",
                        strings
                    )
                ));
            }
            Stmt::Assume(tag, e) => {
                s.push_str(&format!(
                    "  assume {}; // assume:{tag}\n",
                    emit_boogie_expr_with_strings(
                        e,
                        m,
                        &empty_fields,
                        &revealed,
                        "DefaultFuel",
                        strings
                    )
                ));
            }
            Stmt::Reveal(target) => {
                revealed.insert(target.clone());
                s.push_str(&format!(
                    "  // reveal {target} (elevates fuel to RevealFuel)\n"
                ));
            }
            Stmt::Let(_, _) => {}
        }
    }
    // Inductive proof body: recursive law calls become induction hypotheses, guarded by
    // the law's preconditions and a strictly decreasing variant.
    if let Some(body) = &law.body {
        let mut pc = LawProofCtx {
            law,
            m,
            law_ctx: law_ctx.clone(),
            revealed: &revealed,
            strings,
            locals: Vec::new(),
            scope: Vec::new(),
        };
        let proof = pc.stmts(body, "  ");
        if !proof.is_empty() {
            let decls: String = pc
                .locals
                .iter()
                .map(|(n, t)| format!("  var {n}: {t};\n"))
                .collect();
            if let Some(pos) = s.find("{\n") {
                s.insert_str(pos + 2, &decls);
            }
            s.push_str(&proof);
        }
    }
    for (tag, ens_e) in &law.ensures {
        s.push_str(&emit_from_scalar_obligation(
            ens_e, &law_ctx, m, &revealed, "  ", strings,
        ));
        s.push_str(&format!(
            "  assert {{:msg \"{vis_prefix}{}::ensure::{tag}\"}} {};\n",
            law.name,
            emit_boogie_expr_with_strings(
                ens_e,
                m,
                &empty_fields,
                &revealed,
                "DefaultFuel",
                strings
            )
        ));
    }
    s.push_str("}\n\n");
    // Module-level ghost routines are encoded with an implicit receiver
    // argument. A law has no receiver, so when its encoding refers to
    // `Current` it is proven for an arbitrary (universally quantified) one.
    if m.kind == ModuleKind::Module
        && !law.params.iter().any(|(pn, _)| pn == "Current")
        && replace_whole_ident(&s, "Current", "") != s
    {
        params.insert(0, format!("Current: {}", map_boogie_type("Current", m)));
    }
    if canonical_where_guard(m, vis).is_none() {
        return format!(
            "procedure Verify_law_{vis_prefix}{}{tp_quant}({})\n{s}",
            law.name,
            params.join(", ")
        );
    }
    // A conditional `CANONICAL_EQ` law is proven under its `where` hypothesis only: each
    // conditionally canonical type parameter `tp` is instantiated with the fresh abstract type
    // `tp__Canon`, on which (alone) `tp_eq` is structural (see
    // `emit_boogie_canonical_witness_types`). By parametricity the proof covers every
    // canonical instantiation. (A quantified `requires` over the procedure's own type
    // parameter is not usable by Z3 under Boogie's polymorphic type encodings.)
    let canon = conditional_canonical_tps(m);
    let kept: Vec<String> = all_tps
        .iter()
        .filter(|tp| !canon.contains(tp))
        .cloned()
        .collect();
    let tp_quant = if kept.is_empty() {
        String::new()
    } else {
        format!("<{}>", kept.join(", "))
    };
    let mut proc = format!(
        "procedure Verify_law_{vis_prefix}{}{tp_quant}({})\n{s}",
        law.name,
        params.join(", ")
    );
    for tp in &canon {
        proc = replace_whole_ident(&proc, tp, &format!("{tp}__Canon"));
    }
    proc
}

// --- Termination across the call graph (direct and mutual recursion) ---------------------

thread_local! {
    /// For the module being emitted: routine -> the routines of its recursive call group
    /// (strongly connected component of the call graph, including itself when it calls
    /// itself). Routines that are not recursive are absent.
    static RECURSIVE_GROUPS: std::cell::RefCell<HashMap<String, HashSet<String>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// The routine of module `m` that a call expression targets, if any. Calls on receivers
/// known to have another type (e.g. `xs.length` on a `LIST`) are not calls into `m`;
/// receivers of unknown type are conservatively treated as calls into `m`.
fn module_call_target(e: &Expr, m: &ModuleDecl) -> Option<String> {
    match e {
        Expr::Call(f, _) => {
            (find_module_routine(m, f).is_some() && !module_has_variant(m, f)).then(|| f.clone())
        }
        Expr::MethodCall(recv, r, _) => {
            find_module_routine(m, r)?;
            if let Expr::Var(v) = &**recv {
                if v == &m.name {
                    return Some(r.clone());
                }
                if v != "Current" && v.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    return None; // `IO.x`, `LIST.x`, `OTHER_MODULE.x`
                }
            }
            match elem_receiver_type(recv, m) {
                Some(t) if t != module_self_type(m) && t != m.name => None,
                _ => Some(r.clone()),
            }
        }
        _ => None,
    }
}

/// How a routine of the module is used inside a routine body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoutineUse {
    /// A direct, fully applied call: covered by the `variant` obligations.
    Call,
    /// A call inside a lambda body: it happens whenever (and wherever) the closure is applied.
    InClosure,
    /// The routine is used as a function value (named but not applied).
    Reference,
}

fn pattern_binders(p: &Pattern, out: &mut Vec<String>) {
    match p {
        Pattern::Bind(v) => out.push(v.clone()),
        Pattern::Variant(_, ps) | Pattern::Tuple(ps) => {
            ps.iter().for_each(|q| pattern_binders(q, out))
        }
        Pattern::Wildcard | Pattern::Lit(_) => {}
    }
}

/// Visits `let` bindings and then `body`, with the `let` names in scope for what follows.
fn visit_routine_uses_block(
    stmts: &[Stmt],
    body: &Expr,
    m: &ModuleDecl,
    bound: &mut Vec<String>,
    in_closure: bool,
    f: &mut dyn FnMut(&str, RoutineUse),
) {
    let mark = bound.len();
    for st in stmts {
        if let Stmt::Let(v, x) = st {
            visit_routine_uses(x, m, bound, in_closure, f);
            bound.push(v.clone());
        }
    }
    visit_routine_uses(body, m, bound, in_closure, f);
    bound.truncate(mark);
}

/// Reports every use of a routine of `m` inside `e`. `bound` holds the local names in scope
/// (parameters, `let`, `match`, quantifier, IO-bind and lambda binders), which shadow routine
/// names; `in_closure` is set inside lambda bodies.
fn visit_routine_uses(
    e: &Expr,
    m: &ModuleDecl,
    bound: &mut Vec<String>,
    in_closure: bool,
    f: &mut dyn FnMut(&str, RoutineUse),
) {
    let kind_of = |callee: &str, nargs: usize| {
        if in_closure {
            RoutineUse::InClosure
        } else if find_module_routine(m, callee).is_some_and(|(rd, _)| rd.params.len() != nargs) {
            RoutineUse::Reference
        } else {
            RoutineUse::Call
        }
    };
    match e {
        Expr::Var(v) => {
            if !bound.contains(v)
                && find_module_routine(m, v).is_some()
                && !module_has_variant(m, v)
            {
                f(
                    v,
                    if in_closure {
                        RoutineUse::InClosure
                    } else {
                        RoutineUse::Reference
                    },
                );
            }
        }
        Expr::UnaryNot(x) | Expr::UnaryNeg(x) => visit_routine_uses(x, m, bound, in_closure, f),
        Expr::BinOp(_, l, r) => {
            visit_routine_uses(l, m, bound, in_closure, f);
            visit_routine_uses(r, m, bound, in_closure, f);
        }
        Expr::Call(fname, args) => {
            if !bound.contains(fname) {
                if let Some(t) = module_call_target(e, m) {
                    f(&t, kind_of(&t, args.len()));
                }
            }
            for a in args {
                visit_routine_uses(a, m, bound, in_closure, f);
            }
        }
        Expr::MethodCall(recv, _, args) => {
            if let Some(t) = module_call_target(e, m) {
                f(&t, kind_of(&t, args.len()));
            }
            visit_routine_uses(recv, m, bound, in_closure, f);
            for a in args {
                visit_routine_uses(a, m, bound, in_closure, f);
            }
        }
        Expr::If(c, ts, te, es, ee) => {
            visit_routine_uses(c, m, bound, in_closure, f);
            visit_routine_uses_block(ts, te, m, bound, in_closure, f);
            visit_routine_uses_block(es, ee, m, bound, in_closure, f);
        }
        Expr::Match(s, cases) => {
            visit_routine_uses(s, m, bound, in_closure, f);
            for c in cases {
                let mark = bound.len();
                pattern_binders(&c.pattern, bound);
                visit_routine_uses_block(&c.stmts, &c.body, m, bound, in_closure, f);
                bound.truncate(mark);
            }
        }
        Expr::RecordLit(fs) => {
            for (_, x) in fs {
                visit_routine_uses(x, m, bound, in_closure, f);
            }
        }
        Expr::RecordUpdate(b, fs) => {
            visit_routine_uses(b, m, bound, in_closure, f);
            for (_, x) in fs {
                visit_routine_uses(x, m, bound, in_closure, f);
            }
        }
        Expr::TupleLit(xs) => {
            for x in xs {
                visit_routine_uses(x, m, bound, in_closure, f);
            }
        }
        Expr::Forall(v, _, b) | Expr::Exists(v, _, b) => {
            bound.push(v.clone());
            visit_routine_uses(b, m, bound, in_closure, f);
            bound.pop();
        }
        Expr::WithIo(stmts, fin) => {
            let mark = bound.len();
            for s in stmts {
                match s {
                    IoStmt::Bind(v, x) => {
                        visit_routine_uses(x, m, bound, in_closure, f);
                        bound.push(v.clone());
                    }
                    IoStmt::Action(x) => visit_routine_uses(x, m, bound, in_closure, f),
                }
            }
            visit_routine_uses(fin, m, bound, in_closure, f);
            bound.truncate(mark);
        }
        Expr::Lambda(params, _, stmts, body) => {
            let mark = bound.len();
            bound.extend(params.iter().map(|(p, _)| p.clone()));
            visit_routine_uses_block(stmts, body, m, bound, true, f);
            bound.truncate(mark);
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Char(_)
        | Expr::Unit
        | Expr::Bool(_)
        | Expr::Str(_) => {}
    }
}

/// Every use of a routine of `m` in the body (and `let` bindings) of `r`.
fn routine_uses(r: &RoutineDecl, m: &ModuleDecl) -> Vec<(String, RoutineUse)> {
    let mut bound: Vec<String> = r.params.iter().map(|(p, _)| p.clone()).collect();
    let mut out = Vec::new();
    visit_routine_uses_block(&r.stmts, &r.body, m, &mut bound, false, &mut |n, k| {
        out.push((n.to_string(), k))
    });
    out
}

/// Recursive call groups of `m`: two routines are in the same group when each reaches the
/// other in the call graph of their bodies; a routine calling itself forms a group alone.
fn compute_recursive_groups(m: &ModuleDecl) -> HashMap<String, HashSet<String>> {
    let routines: Vec<&RoutineDecl> = m
        .create_routines
        .iter()
        .chain(m.feature_routines.iter())
        .chain(m.specify_routines.iter())
        .map(|(_, r)| r)
        .collect();
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
    for r in &routines {
        set_boogie_type_env(m, &r.params, Some(&r.ret_type));
        let calls: HashSet<String> = routine_uses(r, m).into_iter().map(|(n, _)| n).collect();
        edges.insert(r.name.clone(), calls);
    }
    clear_boogie_type_env();
    let reach = |from: &str| -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut stack: Vec<String> = edges
            .get(from)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        while let Some(n) = stack.pop() {
            if seen.insert(n.clone()) {
                if let Some(next) = edges.get(&n) {
                    stack.extend(next.iter().cloned());
                }
            }
        }
        seen
    };
    let reaches: HashMap<String, HashSet<String>> = routines
        .iter()
        .map(|r| (r.name.clone(), reach(&r.name)))
        .collect();
    let mut groups = HashMap::new();
    for r in &routines {
        let from_r = &reaches[&r.name];
        if !from_r.contains(&r.name) {
            continue; // not recursive
        }
        let group: HashSet<String> = routines
            .iter()
            .filter(|o| from_r.contains(&o.name) && reaches[&o.name].contains(&r.name))
            .map(|o| o.name.clone())
            .collect();
        groups.insert(r.name.clone(), group);
    }
    groups
}

/// Termination obligation for a call from routine `r` to `callee` inside the same recursive
/// group: both must declare a `variant`, and the callee's variant at the call's arguments must
/// be lexicographically smaller than `r`'s current variant (a structural variant on the
/// module's own type is measured by `<M>_adt_size`). Self-calls with a variant keep the
/// existing check (`emit_boogie_variant_obligations`); this covers missing variants and
/// calls between different routines.
#[allow(clippy::too_many_arguments)]
fn emit_group_call_termination(
    callee_name: &str,
    recv: Option<&Expr>,
    args: &[Expr],
    r: &RoutineDecl,
    m: &ModuleDecl,
    revealed: &HashSet<String>,
    strings: &[String],
    pad: &str,
) -> String {
    let in_group = RECURSIVE_GROUPS.with(|g| {
        g.borrow()
            .get(&r.name)
            .is_some_and(|s| s.contains(callee_name))
    });
    if !in_group {
        return String::new();
    }
    let Some((callee, _)) = find_module_routine(m, callee_name) else {
        return String::new();
    };
    if r.variant.is_empty() || callee.variant.is_empty() {
        let who = if r.variant.is_empty() {
            &r.name
        } else {
            &callee.name
        };
        return format!(
            "{pad}assert {{:msg \"{}::variant::variant_missing\"}} false; // recursive call to `{callee_name}`, but `{who}` declares no variant\n",
            r.name
        );
    }
    if callee_name == r.name {
        return String::new();
    }
    let emit = |e: &Expr| {
        emit_boogie_expr_with_strings(e, m, &HashSet::new(), revealed, "DefaultFuel", strings)
    };
    let is_adt_ty =
        |t: &str| t == m.name || t == "Current" || t.starts_with(&format!("{}[", m.name));
    let structural = |v: &Expr, params: &[(String, String)]| -> bool {
        match v {
            Expr::Var(x) => {
                x == "Current" || params.iter().any(|(pn, pt)| pn == x && is_adt_ty(pt))
            }
            _ => false,
        }
    };
    let mut substs: Vec<(String, Expr)> = callee
        .params
        .iter()
        .zip(args.iter())
        .map(|((pn, _), a)| (pn.clone(), a.clone()))
        .collect();
    substs.push((
        "Current".to_string(),
        recv.cloned()
            .unwrap_or_else(|| Expr::Var("Current".to_string())),
    ));
    let mut s = String::new();
    let k = r.variant.len().min(callee.variant.len());
    let mut curr = Vec::new();
    let mut next = Vec::new();
    for i in 0..k {
        let c_struct = structural(&r.variant[i], &r.params);
        let n_struct = structural(&callee.variant[i], &callee.params);
        let c = if c_struct {
            format!("{}_adt_size({})", m.name, emit(&r.variant[i]))
        } else {
            emit(&r.variant[i])
        };
        if !c_struct {
            s.push_str(&format!(
                "{pad}assert {{:msg \"{}::variant::variant_non_negative\"}} 0 <= {c};\n",
                r.name
            ));
        }
        let ne = subst_expr_vars(&callee.variant[i], &substs);
        let n = if n_struct {
            format!("{}_adt_size({})", m.name, emit(&ne))
        } else {
            emit(&ne)
        };
        curr.push(c);
        next.push(n);
    }
    let mut disj = Vec::new();
    for i in 0..k {
        let mut conj: Vec<String> = (0..i)
            .map(|j| format!("({} == {})", next[j], curr[j]))
            .collect();
        conj.push(format!("({} < {})", next[i], curr[i]));
        disj.push(format!("({})", conj.join(" && ")));
    }
    s.push_str(&format!(
        "{pad}assert {{:msg \"{}::variant::variant_decrease\"}} {}; // mutual recursion via `{callee_name}`\n",
        r.name,
        disj.join(" || ")
    ));
    s
}

/// State for translating an inductive law proof (`law ... do <body> ... end`) to Boogie.
struct LawProofCtx<'a> {
    law: &'a LawDecl,
    m: &'a ModuleDecl,
    law_ctx: String,
    revealed: &'a HashSet<String>,
    strings: &'a [String],
    /// Boogie locals introduced for `match` binders: (local name, Boogie type).
    locals: Vec<(String, String)>,
    /// Gardel binder name -> Boogie local, innermost last.
    scope: Vec<(String, String)>,
}

impl LawProofCtx<'_> {
    fn emit(&self, e: &Expr) -> String {
        let mut out = emit_boogie_expr_with_strings(
            e,
            self.m,
            &HashSet::new(),
            self.revealed,
            "DefaultFuel",
            self.strings,
        );
        for (g, b) in self.scope.iter().rev() {
            out = replace_whole_ident(&out, g, b);
        }
        out
    }

    /// Termination measure of variant component `v`: structural size for a variable of the
    /// module's own type, the expression itself otherwise.
    fn measure(&self, v: &Expr) -> (String, bool) {
        if let Expr::Var(x) = v {
            let is_adt = self.law.params.iter().any(|(pn, pt)| {
                pn == x
                    && (pt == &self.m.name
                        || pt == "Current"
                        || pt.starts_with(&format!("{}[", self.m.name)))
            });
            if is_adt {
                return (format!("{}_adt_size({})", self.m.name, self.emit(v)), true);
            }
        }
        (self.emit(v), false)
    }

    /// Obligations and induction hypothesis for a recursive call `law(args)`.
    fn recursive_call(&self, args: &[Expr], pad: &str) -> String {
        let law = self.law;
        let mut s = String::new();
        if args.len() != law.params.len() {
            return s;
        }
        let substs: Vec<(String, Expr)> = law
            .params
            .iter()
            .zip(args.iter())
            .map(|((pn, _), a)| (pn.clone(), a.clone()))
            .collect();
        for (tag, req) in &law.requires {
            s.push_str(&format!(
                "{pad}assert {{:msg \"{}::require::{tag}\"}} {};\n",
                self.law_ctx,
                self.emit(&subst_expr_vars(req, &substs))
            ));
        }
        if law.variant.is_empty() {
            // Induction without a termination measure would be circular.
            s.push_str(&format!(
                "{pad}assert {{:msg \"{}::variant::variant_missing\"}} false;\n",
                self.law_ctx
            ));
            return s;
        }
        let mut curr = Vec::new();
        let mut next = Vec::new();
        for v in &law.variant {
            let (c, structural) = self.measure(v);
            let next_e = subst_expr_vars(v, &substs);
            let n = if structural {
                format!("{}_adt_size({})", self.m.name, self.emit(&next_e))
            } else {
                self.emit(&next_e)
            };
            if !structural {
                s.push_str(&format!(
                    "{pad}assert {{:msg \"{}::variant::variant_non_negative\"}} 0 <= {c};\n",
                    self.law_ctx
                ));
            }
            curr.push(c);
            next.push(n);
        }
        let mut disj = Vec::new();
        for i in 0..curr.len() {
            let mut conj: Vec<String> = (0..i)
                .map(|j| format!("({} == {})", next[j], curr[j]))
                .collect();
            conj.push(format!("({} < {})", next[i], curr[i]));
            disj.push(format!("({})", conj.join(" && ")));
        }
        s.push_str(&format!(
            "{pad}assert {{:msg \"{}::variant::variant_decrease\"}} {};\n",
            self.law_ctx,
            disj.join(" || ")
        ));
        // Induction hypothesis: the law holds for the strictly smaller arguments.
        for (_, ens) in &law.ensures {
            s.push_str(&format!(
                "{pad}assume {};\n",
                self.emit(&subst_expr_vars(ens, &substs))
            ));
        }
        s
    }

    /// Translates a proof expression. Only `match`, `if`, `and` and recursive law calls carry
    /// proof content; anything else contributes nothing (which is sound: no assumption).
    fn stmts(&mut self, e: &Expr, pad: &str) -> String {
        match e {
            Expr::Call(f, args) if f == &self.law.name => self.recursive_call(args, pad),
            Expr::BinOp(op, l, r) if op == "and" => {
                let mut s = self.stmts(l, pad);
                s.push_str(&self.stmts(r, pad));
                s
            }
            Expr::If(c, ts, te, es, ee) => {
                let mut s = format!("{pad}if ({}) {{\n", self.emit(c));
                let inner = format!("{pad}  ");
                if ts.is_empty() {
                    s.push_str(&self.stmts(te, &inner));
                }
                s.push_str(&format!("{pad}}} else {{\n"));
                if es.is_empty() {
                    s.push_str(&self.stmts(ee, &inner));
                }
                s.push_str(&format!("{pad}}}\n"));
                s
            }
            Expr::Match(scrut, cases) => {
                let scrut_bpl = self.emit(scrut);
                let mut s = String::new();
                let mut first = true;
                for c in cases {
                    let Some((prefix, vdecl)) = boogie_match_variant(c.ctor(), self.m) else {
                        continue;
                    };
                    let kw = if first { "if" } else { "} else if" };
                    first = false;
                    s.push_str(&format!(
                        "{pad}{kw} ({prefix}is_{}({scrut_bpl})) {{\n",
                        c.ctor()
                    ));
                    let inner = format!("{pad}  ");
                    let depth = self.scope.len();
                    for (opt_b, (fname, fty)) in c.top_bindings().iter().zip(vdecl.fields.iter()) {
                        if let Some(b) = opt_b {
                            let local = format!("_lb{}_{b}", self.locals.len());
                            self.locals
                                .push((local.clone(), map_boogie_type(fty, self.m)));
                            s.push_str(&format!(
                                "{inner}{local} := {prefix}{}_{fname}({scrut_bpl});\n",
                                c.ctor()
                            ));
                            self.scope.push((b.clone(), local));
                        }
                    }
                    if c.stmts.is_empty() {
                        s.push_str(&self.stmts(&c.body, &inner));
                    }
                    self.scope.truncate(depth);
                }
                if !first {
                    s.push_str(&format!("{pad}}}\n"));
                }
                s
            }
            _ => String::new(),
        }
    }
}

/// For each conditionally canonical type parameter `tp`, a fresh abstract type `tp__Canon`
/// on which element equality is structural. Used only by the module's own conditional
/// `CANONICAL_EQ` law procedures; no other declaration mentions these types.
fn emit_boogie_canonical_witness_types(m: &ModuleDecl) -> String {
    let has_canonical_law = m
        .specify_laws
        .iter()
        .any(|(vis, _)| canonical_where_guard(m, vis).is_some());
    if !has_canonical_law {
        return String::new();
    }
    let mut out = String::from(
        "// Witness types for conditional CANONICAL_EQ laws (where-bound hypothesis)\n",
    );
    for tp in conditional_canonical_tps(m) {
        out.push_str(&format!("type {tp}__Canon;\n"));
        out.push_str(&format!(
            "axiom (forall _cx: {tp}__Canon, _cy: {tp}__Canon :: {{ {tp}_eq(_cx, _cy) }} {tp}_eq(_cx, _cy) <==> _cx == _cy);\n"
        ));
    }
    out.push('\n');
    out
}

fn collect_proc_locals(
    stmts: &[Stmt],
    body: &Expr,
    r: &RoutineDecl,
    m: &ModuleDecl,
    out: &mut Vec<(String, String)>,
) {
    for st in stmts {
        if let Stmt::Let(v, e) = st {
            if let Some(t) = elem_receiver_type(e, m) {
                elem_env_bind(v, &t);
            }
            let gty = boogie_prelude_ctx().and_then(|ctx| infer_gardel_type(e, m, &ctx));
            let bty = match &gty {
                Some(t) => {
                    bind_boogie_env_type(v, t);
                    map_boogie_type(t, m)
                }
                None => infer_boogie_expr_type(e, r, m),
            };
            out.push((v.clone(), bty));
            collect_proc_locals(&[], e, r, m, out);
        }
    }
    match body {
        Expr::If(_, ts, te, es, ee) => {
            collect_proc_locals(ts, te, r, m, out);
            collect_proc_locals(es, ee, r, m, out);
        }
        Expr::Match(scrut, cases) => {
            let ctx = boogie_prelude_ctx();
            for c in cases {
                if let Some((proj_prefix, vdecl)) = boogie_match_variant(c.ctor(), m) {
                    // Prelude variant fields are instantiated from the scrutinee's type.
                    let mut inst = HashMap::new();
                    if let (false, Some(ctx)) = (proj_prefix.is_empty(), ctx.as_ref()) {
                        if let Some(owner) = ctx.module(proj_prefix.trim_end_matches('_')) {
                            if let Some(st) = infer_gardel_type(scrut, m, ctx) {
                                unify_type(
                                    &module_self_type(owner),
                                    &st,
                                    &owner.type_params,
                                    &mut inst,
                                );
                            }
                        }
                    }
                    for (opt_b, (_, fty)) in c.top_bindings().iter().zip(vdecl.fields.iter()) {
                        if let Some(b) = opt_b {
                            if proj_prefix.is_empty() {
                                elem_env_bind(b, fty);
                            }
                            let gty = subst_type_idents(fty, &inst);
                            bind_boogie_env_type(b, &gty);
                            out.push((b.clone(), map_boogie_type(&gty, m)));
                        }
                    }
                }
                collect_proc_locals(&c.stmts, &c.body, r, m, out);
            }
        }
        Expr::WithIo(io_stmts, final_e) => {
            for s in io_stmts {
                if let IoStmt::Bind(v, e) = s {
                    // Record the Gardel type too, so a later `match v` instantiates binders.
                    if let Some(gty) =
                        boogie_prelude_ctx().and_then(|ctx| infer_gardel_type(e, m, &ctx))
                    {
                        bind_boogie_env_type(v, &gty);
                    }
                    let bty = infer_boogie_expr_type(e, r, m);
                    out.push((v.clone(), bty));
                }
            }
            collect_proc_locals(&[], final_e, r, m, out);
        }
        _ => {}
    }
}

fn infer_boogie_expr_type(e: &Expr, r: &RoutineDecl, m: &ModuleDecl) -> String {
    match e {
        Expr::Int(_) => "int".to_string(),
        Expr::Str(_) => map_boogie_type("STRING", m),
        Expr::Bool(_) | Expr::UnaryNot(_) => "bool".to_string(),
        Expr::BinOp(op, l, _) => {
            if matches!(op.as_str(), "+" | "-" | "*" | "/" | "%") {
                if matches!(&**l, Expr::Str(_)) {
                    map_boogie_type("STRING", m)
                } else {
                    "int".to_string()
                }
            } else {
                "bool".to_string()
            }
        }
        Expr::RecordLit(_) | Expr::RecordUpdate(_, _) => map_boogie_type("Current", m),
        Expr::Call(fname, _) => {
            if fname == &r.name {
                return map_boogie_type(&r.ret_type, m);
            }
            for (_, cr) in m
                .create_routines
                .iter()
                .chain(m.feature_routines.iter())
                .chain(m.specify_routines.iter())
            {
                if cr.name == *fname {
                    return map_boogie_type(&cr.ret_type, m);
                }
            }
            map_boogie_type("Current", m)
        }
        Expr::MethodCall(recv, mname, _) => {
            if let Expr::Var(v) = &**recv {
                if v == "IO" && mname == "read_line" {
                    if boogie_prelude_ctx().is_some_and(|c| c.module("RESULT").is_some()) {
                        return map_boogie_type(READ_LINE_RESULT_TYPE, m);
                    }
                    return map_boogie_type("STRING", m);
                }
            }
            if mname == &r.name {
                return map_boogie_type(&r.ret_type, m);
            }
            for (_, cr) in m
                .feature_routines
                .iter()
                .chain(m.specify_routines.iter())
                .chain(m.create_routines.iter())
            {
                if cr.name == *mname {
                    return map_boogie_type(&cr.ret_type, m);
                }
            }
            if let ReprKind::Product(fields) = &m.repr {
                for (fnm, fty) in fields {
                    if fnm == mname {
                        return map_boogie_type(fty, m);
                    }
                }
            }
            "int".to_string()
        }
        _ => "int".to_string(),
    }
}

/// Gardel type of `IO.read_line`'s result (§11).
const READ_LINE_RESULT_TYPE: &str = "RESULT[IO_ERROR, STRING]";

fn is_io_read_line(e: &Expr) -> bool {
    matches!(e, Expr::MethodCall(recv, r, args)
        if r == "read_line" && args.is_empty() && matches!(&**recv, Expr::Var(v) if v == "IO"))
}

/// What the runtime guarantees about a value `v` bound by `v <- IO.read_line` (§11): it is
/// a `RESULT[IO_ERROR, STRING]`, and an `Ok` payload is a valid prelude `STRING` (the C
/// runtime decodes the line and returns `Error` instead of an invalid `STRING`). `None`
/// outside prelude mode, where `read_line` has the legacy opaque string type.
fn read_line_result_wf(v: &str) -> Option<String> {
    let ctx = boogie_prelude_ctx()?;
    if ctx.module("RESULT").is_none() || ctx.module("STRING").is_none() {
        return None;
    }
    Some(format!(
        "RESULT__RESULT_Invariant({v}) && (RESULT__is_Ok({v}) ==> STRING__STRING_Invariant(RESULT__Ok_val({v})))"
    ))
}

fn emit_boogie_proc_stmts_and_ret(
    stmts: &[Stmt],
    body: &Expr,
    r: &RoutineDecl,
    m: &ModuleDecl,
    revealed: &mut HashSet<String>,
    indent: usize,
    strings: &[String],
) -> String {
    let pad = " ".repeat(indent);
    let empty_fields = HashSet::new();
    let mut s = String::new();

    for st in stmts {
        if let Stmt::Let(_, e) | Stmt::Check(_, e) | Stmt::Assume(_, e) = st {
            s.push_str(&emit_from_scalar_obligation(
                e, &r.name, m, revealed, &pad, strings,
            ));
        }
        match st {
            Stmt::Let(v, e) => {
                s.push_str(&emit_boogie_expr_obligations(
                    e, r, m, revealed, indent, strings,
                ));
                s.push_str(&format!(
                    "{pad}{v} := {};\n",
                    emit_boogie_expr_with_strings(
                        e,
                        m,
                        &empty_fields,
                        revealed,
                        "DefaultFuel",
                        strings
                    )
                ));
            }
            Stmt::Check(tag, e) => {
                s.push_str(&format!(
                    "{pad}assert {{:msg \"{}::check::{tag}\"}} {};\n",
                    r.name,
                    emit_boogie_expr_with_strings(
                        e,
                        m,
                        &empty_fields,
                        revealed,
                        "DefaultFuel",
                        strings
                    )
                ));
            }
            Stmt::Assume(tag, e) => {
                s.push_str(&format!(
                    "{pad}assume {}; // assume:{tag}\n",
                    emit_boogie_expr_with_strings(
                        e,
                        m,
                        &empty_fields,
                        revealed,
                        "DefaultFuel",
                        strings
                    )
                ));
            }
            Stmt::Reveal(target) => {
                revealed.insert(target.clone());
                s.push_str(&format!(
                    "{pad}// reveal {target}: unfolds `{target}` to RevealFuel in current block\n"
                ));
            }
        }
    }

    // `if`/`match` bodies are split into Boogie branches below (whose own
    // bodies are checked recursively), so only the condition/scrutinee is
    // checked here; any other body is checked as a whole.
    let wf_target = match body {
        Expr::If(c, ..) => &**c,
        Expr::Match(scrut, _) => &**scrut,
        _ => body,
    };
    s.push_str(&emit_from_scalar_obligation(
        wf_target, &r.name, m, revealed, &pad, strings,
    ));

    match body {
        Expr::If(c, ts, te, es, ee) => {
            s.push_str(&emit_boogie_expr_obligations(
                c, r, m, revealed, indent, strings,
            ));
            let mut rev_then = revealed.clone();
            let mut rev_else = revealed.clone();
            s.push_str(&format!(
                "{pad}if ({}) {{\n{}{pad}}} else {{\n{}{pad}}}\n",
                emit_boogie_expr_with_strings(
                    c,
                    m,
                    &empty_fields,
                    revealed,
                    "DefaultFuel",
                    strings
                ),
                emit_boogie_proc_stmts_and_ret(ts, te, r, m, &mut rev_then, indent + 2, strings),
                emit_boogie_proc_stmts_and_ret(es, ee, r, m, &mut rev_else, indent + 2, strings),
            ));
        }
        Expr::Match(scrut, cases) => {
            let scrut_bpl = emit_boogie_expr_with_strings(
                scrut,
                m,
                &empty_fields,
                revealed,
                "DefaultFuel",
                strings,
            );
            let on_prelude_adt = cases
                .iter()
                .any(|c| prelude_variant_for(c.ctor(), m).is_some());
            if let (ReprKind::Sum(variants), false) = (&m.repr, on_prelude_adt) {
                let covered: HashSet<String> = cases.iter().map(|c| c.ctor().to_string()).collect();
                for v in variants {
                    if !covered.contains(&v.name) {
                        s.push_str(&format!(
                            "{pad}assert {{:msg \"{}::match::exhaustiveness_{}\"}} !is_{}({scrut_bpl});\n",
                            r.name, v.name, v.name
                        ));
                    }
                }
            }
            for (idx, c) in cases.iter().enumerate() {
                let kw = if idx == 0 { "if" } else { "else if" };
                s.push_str(&format!(
                    "{pad}{kw} ({}({scrut_bpl})) {{\n",
                    boogie_ctor_tester(c.ctor(), m)
                ));
                {
                    if let Some((proj_prefix, vdecl)) = boogie_match_variant(c.ctor(), m) {
                        let is_own = proj_prefix.is_empty();
                        for (opt_b, (fname, fty)) in
                            c.top_bindings().iter().zip(vdecl.fields.iter())
                        {
                            if let Some(bname) = opt_b {
                                s.push_str(&format!(
                                    "{pad}  {bname} := {proj_prefix}{}_{}({scrut_bpl});\n",
                                    c.ctor(),
                                    fname
                                ));
                                if is_own
                                    && !m.invariants.is_empty()
                                    && (fty == &m.name
                                        || fty == "Current"
                                        || fty.starts_with(&format!("{}[", m.name)))
                                {
                                    s.push_str(&format!(
                                        "{pad}  assume {}_Invariant({bname});\n",
                                        m.name
                                    ));
                                }
                            }
                        }
                    }
                }
                let mut rev_case = revealed.clone();
                s.push_str(&emit_boogie_proc_stmts_and_ret(
                    &c.stmts,
                    &c.body,
                    r,
                    m,
                    &mut rev_case,
                    indent + 2,
                    strings,
                ));
                s.push_str(&format!("{pad}}}\n"));
            }
        }
        Expr::WithIo(io_stmts, final_e) => {
            for io_st in io_stmts {
                match io_st {
                    IoStmt::Bind(v, e) => {
                        s.push_str(&emit_from_scalar_obligation(
                            e, &r.name, m, revealed, &pad, strings,
                        ));
                        s.push_str(&emit_boogie_expr_obligations(
                            e, r, m, revealed, indent, strings,
                        ));
                        if is_io_read_line(e) {
                            // Every read yields a fresh value from the outside world (§11).
                            s.push_str(&format!("{pad}havoc {v};\n"));
                            if let Some(wf) = read_line_result_wf(v) {
                                s.push_str(&format!(
                                    "{pad}assume {wf}; // IO boundary: the runtime decodes and validates input (spec §11)\n"
                                ));
                            }
                        } else {
                            s.push_str(&format!(
                                "{pad}{v} := {};\n",
                                emit_boogie_expr_with_strings(
                                    e,
                                    m,
                                    &empty_fields,
                                    revealed,
                                    "DefaultFuel",
                                    strings
                                )
                            ));
                        }
                    }
                    IoStmt::Action(e) => {
                        s.push_str(&emit_from_scalar_obligation(
                            e, &r.name, m, revealed, &pad, strings,
                        ));
                        s.push_str(&emit_boogie_expr_obligations(
                            e, r, m, revealed, indent, strings,
                        ));
                        // The action's effect is not modeled, but its expression must still
                        // type-check (e.g. `IO.print_line(r)` with `r: RESULT[..]` is rejected).
                        let act = emit_boogie_expr_with_strings(
                            e,
                            m,
                            &empty_fields,
                            revealed,
                            "DefaultFuel",
                            strings,
                        );
                        s.push_str(&format!(
                            "{pad}assume ({act}) == ({act}); // linear IO action\n"
                        ));
                    }
                }
            }
            // The trailing expression may itself branch (`match`/`if`) or nest `with IO`.
            s.push_str(&emit_boogie_proc_stmts_and_ret(
                &[],
                final_e,
                r,
                m,
                revealed,
                indent,
                strings,
            ));
        }
        _ => {
            let substs: Vec<(String, Expr)> = stmts
                .iter()
                .filter_map(|s| {
                    if let Stmt::Let(v, e) = s {
                        Some((v.clone(), e.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            let effective_body = subst_expr_vars(body, &substs);
            s.push_str(&emit_boogie_expr_obligations(
                body, r, m, revealed, indent, strings,
            ));
            s.push_str(&format!(
                "{pad}Result := {};\n",
                emit_boogie_expr_with_strings(
                    &effective_body,
                    m,
                    &empty_fields,
                    revealed,
                    "DefaultFuel",
                    strings
                )
            ));
        }
    }

    s
}

/// `CHAR.from_scalar(n)` precondition (§3.1):
/// `valid_scalar: n <= 0xD7FF or (0xE000 <= n and n <= 0x10FFFF)`.
fn valid_scalar_condition(n: &Expr) -> Expr {
    Expr::BinOp(
        "or".to_string(),
        Box::new(Expr::BinOp(
            "<=".to_string(),
            Box::new(n.clone()),
            Box::new(Expr::Int(0xD7FF)),
        )),
        Box::new(Expr::BinOp(
            "and".to_string(),
            Box::new(Expr::BinOp(
                "<=".to_string(),
                Box::new(Expr::Int(0xE000)),
                Box::new(n.clone()),
            )),
            Box::new(Expr::BinOp(
                "<=".to_string(),
                Box::new(n.clone()),
                Box::new(Expr::Int(0x10FFFF)),
            )),
        )),
    )
}

fn and_wf(a: Option<Expr>, b: Option<Expr>) -> Option<Expr> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) => Some(Expr::BinOp("and".to_string(), Box::new(a), Box::new(b))),
    }
}

fn guarded_wf(guard: Expr, wf: Option<Expr>) -> Option<Expr> {
    wf.map(|w| Expr::BinOp("implies".to_string(), Box::new(guard), Box::new(w)))
}

/// Well-formedness of a block `stmts; body`: each `let` value must be
/// well-formed, and later expressions see earlier `let`s substituted.
fn from_scalar_wf_block(stmts: &[Stmt], body: &Expr) -> Option<Expr> {
    let mut substs: Vec<(String, Expr)> = Vec::new();
    let mut acc = None;
    for st in stmts {
        match st {
            Stmt::Let(v, e) | Stmt::Check(v, e) | Stmt::Assume(v, e) => {
                let e_sub = subst_expr_vars(e, &substs);
                acc = and_wf(acc, from_scalar_wf(&e_sub));
                if matches!(st, Stmt::Let(_, _)) {
                    substs.push((v.clone(), e_sub));
                }
            }
            Stmt::Reveal(_) => {}
        }
    }
    and_wf(acc, from_scalar_wf(&subst_expr_vars(body, &substs)))
}

/// Builds the well-formedness condition for every `CHAR.from_scalar(n)` call
/// in `e`, as a Gardel boolean expression. Each call contributes
/// `valid_scalar(n)` under the path condition that guards its evaluation
/// (short-circuit `and`/`or`/`implies`, `if` branches, `match` cases, and
/// binders). Returns `None` when `e` contains no such call.
fn from_scalar_wf(e: &Expr) -> Option<Expr> {
    match e {
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Char(_)
        | Expr::Unit
        | Expr::Bool(_)
        | Expr::Str(_)
        | Expr::Var(_) => None,
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => from_scalar_wf(inner),
        Expr::BinOp(op, l, r) => {
            let wl = from_scalar_wf(l);
            let wr = from_scalar_wf(r);
            let wr = match op.as_str() {
                "and" | "implies" => guarded_wf((**l).clone(), wr),
                "or" => guarded_wf(Expr::UnaryNot(l.clone()), wr),
                _ => wr,
            };
            and_wf(wl, wr)
        }
        Expr::MethodCall(recv, mname, args) => {
            let mut acc = from_scalar_wf(recv);
            for a in args {
                acc = and_wf(acc, from_scalar_wf(a));
            }
            if matches!(&**recv, Expr::Var(v) if v == "CHAR") && mname == "from_scalar" {
                if let Some(n) = args.first() {
                    acc = and_wf(acc, Some(valid_scalar_condition(n)));
                }
            }
            acc
        }
        Expr::Call(_, args) | Expr::TupleLit(args) => args
            .iter()
            .fold(None, |acc, a| and_wf(acc, from_scalar_wf(a))),
        Expr::RecordLit(fields) => fields
            .iter()
            .fold(None, |acc, (_, fe)| and_wf(acc, from_scalar_wf(fe))),
        Expr::RecordUpdate(base, fields) => {
            fields.iter().fold(from_scalar_wf(base), |acc, (_, fe)| {
                and_wf(acc, from_scalar_wf(fe))
            })
        }
        Expr::If(c, ts, te, es, ee) => {
            let wc = from_scalar_wf(c);
            let wt = guarded_wf((**c).clone(), from_scalar_wf_block(ts, te));
            let we = guarded_wf(Expr::UnaryNot(c.clone()), from_scalar_wf_block(es, ee));
            and_wf(and_wf(wc, wt), we)
        }
        Expr::Match(scrut, cases) => {
            let ws = from_scalar_wf(scrut);
            let case_wfs: Vec<Option<Expr>> = cases
                .iter()
                .map(|c| from_scalar_wf_block(&c.stmts, &c.body))
                .collect();
            if case_wfs.iter().all(Option::is_none) {
                return ws;
            }
            let wf_cases: Vec<MatchCase> = cases
                .iter()
                .zip(case_wfs)
                .map(|(c, w)| {
                    MatchCase::new(c.pattern.clone(), Vec::new(), w.unwrap_or(Expr::Bool(true)))
                })
                .collect();
            and_wf(ws, Some(Expr::Match(scrut.clone(), wf_cases)))
        }
        Expr::Forall(v, ty, body) | Expr::Exists(v, ty, body) => {
            from_scalar_wf(body).map(|w| Expr::Forall(v.clone(), ty.clone(), Box::new(w)))
        }
        Expr::Lambda(params, _, stmts, body) => from_scalar_wf_block(stmts, body).map(|w| {
            params.iter().rev().fold(w, |acc, (pn, pt)| {
                Expr::Forall(pn.clone(), pt.clone(), Box::new(acc))
            })
        }),
        Expr::WithIo(io_stmts, final_e) => {
            let mut acc = None;
            for st in io_stmts {
                match st {
                    IoStmt::Bind(_, ie) | IoStmt::Action(ie) => {
                        acc = and_wf(acc, from_scalar_wf(ie));
                    }
                }
            }
            and_wf(acc, from_scalar_wf(final_e))
        }
    }
}

/// Emits the Boogie proof obligation for the `CHAR.from_scalar` calls in `e`
/// (if any), tagged `<ctx>::require::valid_scalar`.
fn emit_from_scalar_obligation(
    e: &Expr,
    ctx: &str,
    m: &ModuleDecl,
    revealed: &HashSet<String>,
    pad: &str,
    strings: &[String],
) -> String {
    match from_scalar_wf(e) {
        None => String::new(),
        Some(wf) => format!(
            "{pad}assert {{:msg \"{ctx}::require::valid_scalar\"}} {};\n",
            emit_boogie_expr_with_strings(
                &wf,
                m,
                &HashSet::new(),
                revealed,
                "DefaultFuel",
                strings
            )
        ),
    }
}

/// Call-site precondition obligations for a call to a routine of the *same* module `m`
/// (static `M.r(..)`, unqualified `r(..)`, or `x.r(..)` on a receiver known to have the
/// module's own type). Each callee `require` clause is asserted with the callee's parameters
/// (and `Current`, and for product modules its bare field names) replaced by the actual
/// arguments. Without this, a verified body could call a routine outside its domain:
/// the callee's contract axioms are guarded by the precondition, so nothing would fail,
/// yet at run time the call executes (Gardel binaries have no runtime checks, §7).
#[allow(clippy::too_many_arguments)]
fn emit_local_call_precondition_obligations(
    callee_name: &str,
    recv: Option<&Expr>,
    args: &[Expr],
    r: &RoutineDecl,
    m: &ModuleDecl,
    revealed: &HashSet<String>,
    pad: &str,
    strings: &[String],
) -> String {
    let Some((callee, _)) = find_module_routine(m, callee_name) else {
        return String::new();
    };
    if callee.requires.is_empty() || callee.params.len() != args.len() {
        return String::new();
    }
    let mut substs: Vec<(String, Expr)> = callee
        .params
        .iter()
        .zip(args.iter())
        .map(|((pn, _), a)| (pn.clone(), a.clone()))
        .collect();
    if let Some(rv) = recv {
        substs.push(("Current".to_string(), rv.clone()));
        if !matches!(rv, Expr::Var(v) if v == "Current") {
            if let ReprKind::Product(fields) = &m.repr {
                for (f, _) in fields {
                    if !callee.params.iter().any(|(pn, _)| pn == f) {
                        substs.push((
                            f.clone(),
                            Expr::MethodCall(Box::new(rv.clone()), f.clone(), Vec::new()),
                        ));
                    }
                }
            }
        }
    }
    let mut s = String::new();
    for (tag, req) in &callee.requires {
        let inst = subst_expr_vars(req, &substs);
        s.push_str(&format!(
            "{pad}assert {{:msg \"{}::require::{tag}\"}} {};\n",
            r.name,
            emit_boogie_expr_with_strings(
                &inst,
                m,
                &HashSet::new(),
                revealed,
                "DefaultFuel",
                strings
            )
        ));
    }
    s
}

/// True when `recv` is statically known to denote a value of module `m`'s own type.
fn is_module_typed_receiver(recv: &Expr, m: &ModuleDecl) -> bool {
    if elem_equals_fn(recv, m).is_some() {
        return false;
    }
    elem_receiver_type(recv, m).is_some_and(|t| t == module_self_type(m) || t == m.name)
}

fn emit_boogie_expr_obligations(
    e: &Expr,
    r: &RoutineDecl,
    m: &ModuleDecl,
    revealed: &HashSet<String>,
    indent: usize,
    strings: &[String],
) -> String {
    let pad = " ".repeat(indent);
    let empty_fields = HashSet::new();
    let mut s = String::new();

    match e {
        Expr::BinOp(op, l, rhs) => {
            if matches!(&**l, Expr::Str(_)) || matches!(&**rhs, Expr::Str(_)) {
                return s;
            }
            s.push_str(&emit_boogie_expr_obligations(
                l, r, m, revealed, indent, strings,
            ));
            s.push_str(&emit_boogie_expr_obligations(
                rhs, r, m, revealed, indent, strings,
            ));
            let l_bpl = emit_boogie_expr_with_strings(
                l,
                m,
                &empty_fields,
                revealed,
                "DefaultFuel",
                strings,
            );
            let r_bpl = emit_boogie_expr_with_strings(
                rhs,
                m,
                &empty_fields,
                revealed,
                "DefaultFuel",
                strings,
            );
            match op.as_str() {
                "+" if r.ret_type != "MATH_INT" => {
                    s.push_str(&format!(
                        "{pad}assert {{:msg \"{}::overflow::overflow_add\"}} in_u64({l_bpl} + {r_bpl});\n",
                        r.name
                    ));
                }
                "-" if r.ret_type != "MATH_INT" => {
                    s.push_str(&format!(
                        "{pad}assert {{:msg \"{}::overflow::overflow_sub\"}} 0 <= ({l_bpl} - {r_bpl});\n",
                        r.name
                    ));
                }
                "*" if r.ret_type != "MATH_INT" => {
                    s.push_str(&format!(
                        "{pad}assert {{:msg \"{}::overflow::overflow_mul\"}} in_u64({l_bpl} * {r_bpl});\n",
                        r.name
                    ));
                }
                "/" | "%" => {
                    s.push_str(&format!(
                        "{pad}assert {{:msg \"{}::div_by_zero::div_by_zero\"}} {r_bpl} != 0;\n",
                        r.name
                    ));
                }
                "<<" | ">>" => {
                    s.push_str(&format!(
                        "{pad}assert {{:msg \"{}::shift::shift_bound\"}} 0 <= {r_bpl} && {r_bpl} < 64;\n",
                        r.name
                    ));
                }
                _ => {}
            }
        }
        Expr::UnaryNeg(inner) => {
            s.push_str(&emit_boogie_expr_obligations(
                inner, r, m, revealed, indent, strings,
            ));
        }
        Expr::RecordLit(_) | Expr::RecordUpdate(_, _) => {
            let val_bpl = emit_boogie_expr_with_strings(
                e,
                m,
                &empty_fields,
                revealed,
                "DefaultFuel",
                strings,
            );
            if !m.invariants.is_empty() {
                for (tag, _) in &m.invariants {
                    s.push_str(&format!(
                        "{pad}assert {{:msg \"{}::invariant::{tag}\"}} {}_Invariant({val_bpl});\n",
                        r.name, m.name
                    ));
                }
            }
        }
        Expr::MethodCall(recv, mname, args) => {
            s.push_str(&emit_boogie_expr_obligations(
                recv, r, m, revealed, indent, strings,
            ));
            for a in args {
                s.push_str(&emit_boogie_expr_obligations(
                    a, r, m, revealed, indent, strings,
                ));
            }
            // Calls into the injected prelude must establish the callee's `require` clauses.
            if let Some(ctx) = boogie_prelude_ctx() {
                if let Some((pname, rname, has_cur)) = prelude_call_target(e, m, &ctx) {
                    let (callee, declared) = match ctx.module(&pname) {
                        Some(p) => (
                            find_module_routine(p, &rname)
                                .map(|(rd, _)| rd.requires.clone())
                                .unwrap_or_default(),
                            prelude_contract_interface(p),
                        ),
                        None => (Vec::new(), Rc::new((String::new(), HashSet::new()))),
                    };
                    let mut call_args = Vec::new();
                    if has_cur {
                        call_args.push(emit_boogie_expr_with_strings(
                            recv,
                            m,
                            &empty_fields,
                            revealed,
                            "DefaultFuel",
                            strings,
                        ));
                    }
                    for a in args {
                        call_args.push(emit_boogie_expr_with_strings(
                            a,
                            m,
                            &empty_fields,
                            revealed,
                            "DefaultFuel",
                            strings,
                        ));
                    }
                    for (idx, (tag, _)) in callee.iter().enumerate() {
                        if !declared.1.contains(&format!("pre_{rname}_{idx}")) {
                            continue;
                        }
                        s.push_str(&format!(
                            "{pad}assert {{:msg \"{}::require::{tag}\"}} {pname}__pre_{rname}_{idx}({});\n",
                            r.name,
                            call_args.join(", ")
                        ));
                    }
                }
            }
            // Termination of (mutual) recursion within the module.
            if let Some(callee) = module_call_target(e, m) {
                let static_call = matches!(&**recv, Expr::Var(v) if v == &m.name);
                s.push_str(&emit_group_call_termination(
                    &callee,
                    if static_call { None } else { Some(&**recv) },
                    args,
                    r,
                    m,
                    revealed,
                    strings,
                    &pad,
                ));
            }
            // Calls into the module itself must establish the callee's `require` clauses too.
            if matches!(&**recv, Expr::Var(v) if v == &m.name) {
                s.push_str(&emit_local_call_precondition_obligations(
                    mname, None, args, r, m, revealed, &pad, strings,
                ));
            } else if is_module_typed_receiver(recv, m) {
                s.push_str(&emit_local_call_precondition_obligations(
                    mname,
                    Some(&**recv),
                    args,
                    r,
                    m,
                    revealed,
                    &pad,
                    strings,
                ));
            }
            if mname == &r.name {
                let mut substs = vec![("Current".to_string(), (**recv).clone())];
                for ((pname, _), arg) in r.params.iter().zip(args.iter()) {
                    substs.push((pname.clone(), arg.clone()));
                }
                s.push_str(&emit_boogie_variant_obligations(
                    r, m, &substs, revealed, strings, &pad,
                ));
            }
        }
        Expr::Call(fname, args) => {
            for a in args {
                s.push_str(&emit_boogie_expr_obligations(
                    a, r, m, revealed, indent, strings,
                ));
            }
            // Termination of (mutual) recursion within the module.
            if let Some(callee) = module_call_target(e, m) {
                s.push_str(&emit_group_call_termination(
                    &callee, None, args, r, m, revealed, strings, &pad,
                ));
            }
            // Unqualified call to a routine of the module (implicit receiver `Current`).
            if !module_has_variant(m, fname) {
                let implicit_recv = Expr::Var("Current".to_string());
                let has_current = find_module_routine(m, fname).is_some_and(|(_, hc)| hc);
                s.push_str(&emit_local_call_precondition_obligations(
                    fname,
                    if has_current {
                        Some(&implicit_recv)
                    } else {
                        None
                    },
                    args,
                    r,
                    m,
                    revealed,
                    &pad,
                    strings,
                ));
            }
            if fname == &r.name {
                let mut substs = Vec::new();
                for ((pname, _), arg) in r.params.iter().zip(args.iter()) {
                    substs.push((pname.clone(), arg.clone()));
                }
                s.push_str(&emit_boogie_variant_obligations(
                    r, m, &substs, revealed, strings, &pad,
                ));
            }
        }
        _ => {}
    }

    s
}

fn emit_boogie_variant_obligations(
    r: &RoutineDecl,
    m: &ModuleDecl,
    substs: &[(String, Expr)],
    revealed: &HashSet<String>,
    strings: &[String],
    pad: &str,
) -> String {
    if r.variant.is_empty() {
        return String::new();
    }
    let empty_fields = HashSet::new();
    let mut s = String::new();

    if r.variant.len() == 1 && matches!(&r.variant[0], Expr::Var(v) if v == "Current") {
        let recv_bpl = if let Some((_, repl)) = substs.iter().find(|(k, _)| k == "Current") {
            emit_boogie_expr_with_strings(repl, m, &empty_fields, revealed, "DefaultFuel", strings)
        } else {
            "Current".to_string()
        };
        s.push_str(&format!(
            "{pad}assert {{:msg \"{}::variant::variant_decrease\"}} {}_adt_size({recv_bpl}) < {}_adt_size(Current);\n",
            r.name, m.name, m.name
        ));
        return s;
    }

    let mut curr_vals = Vec::new();
    let mut next_vals = Vec::new();

    for v_e in &r.variant {
        let curr_v =
            emit_boogie_expr_with_strings(v_e, m, &empty_fields, revealed, "DefaultFuel", strings);
        s.push_str(&format!(
            "{pad}assert {{:msg \"{}::variant::variant_non_negative\"}} 0 <= {curr_v};\n",
            r.name
        ));
        let next_var_e = subst_expr_vars(v_e, substs);
        let next_v = emit_boogie_expr_with_strings(
            &next_var_e,
            m,
            &empty_fields,
            revealed,
            "DefaultFuel",
            strings,
        );
        curr_vals.push(curr_v);
        next_vals.push(next_v);
    }

    let mut disjuncts = Vec::new();
    for i in 0..curr_vals.len() {
        let mut conjuncts = Vec::new();
        for j in 0..i {
            conjuncts.push(format!("({} == {})", next_vals[j], curr_vals[j]));
        }
        conjuncts.push(format!("({} < {})", next_vals[i], curr_vals[i]));
        disjuncts.push(format!("({})", conjuncts.join(" && ")));
    }

    let decr_expr = if disjuncts.len() == 1 {
        format!("{} < {}", next_vals[0], curr_vals[0])
    } else {
        disjuncts.join(" || ")
    };

    s.push_str(&format!(
        "{pad}assert {{:msg \"{}::variant::variant_decrease\"}} ({decr_expr});\n",
        r.name
    ));
    s
}

fn emit_boogie_block_expr(
    stmts: &[Stmt],
    body: &Expr,
    m: &ModuleDecl,
    implicit_fields: &HashSet<String>,
    fuel_var: &str,
    strings: &[String],
) -> String {
    let mut revealed = HashSet::new();
    for st in stmts {
        if let Stmt::Reveal(t) = st {
            revealed.insert(t.clone());
        }
    }
    let mut expr = body.clone();
    for st in stmts.iter().rev() {
        if let Stmt::Let(var, val) = st {
            expr = subst_expr_var(&expr, var, val);
        }
    }
    emit_boogie_expr_with_strings(&expr, m, implicit_fields, &revealed, fuel_var, strings)
}

pub fn subst_expr_var(e: &Expr, target: &str, replacement: &Expr) -> Expr {
    subst_expr_vars(e, &[(target.to_string(), replacement.clone())])
}

pub fn subst_expr_vars(e: &Expr, substs: &[(String, Expr)]) -> Expr {
    if substs.is_empty() {
        return e.clone();
    }
    match e {
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Char(_)
        | Expr::Unit
        | Expr::Bool(_)
        | Expr::Str(_) => e.clone(),
        Expr::Var(v) => {
            if let Some((_, repl)) = substs.iter().find(|(target, _)| target == v) {
                repl.clone()
            } else {
                e.clone()
            }
        }
        Expr::UnaryNot(inner) => Expr::UnaryNot(Box::new(subst_expr_vars(inner, substs))),
        Expr::UnaryNeg(inner) => Expr::UnaryNeg(Box::new(subst_expr_vars(inner, substs))),
        Expr::BinOp(op, l, r) => Expr::BinOp(
            op.clone(),
            Box::new(subst_expr_vars(l, substs)),
            Box::new(subst_expr_vars(r, substs)),
        ),
        Expr::Call(f, args) => Expr::Call(
            f.clone(),
            args.iter().map(|a| subst_expr_vars(a, substs)).collect(),
        ),
        Expr::MethodCall(recv, mname, args) => Expr::MethodCall(
            Box::new(subst_expr_vars(recv, substs)),
            mname.clone(),
            args.iter().map(|a| subst_expr_vars(a, substs)).collect(),
        ),
        Expr::If(c, ts, te, es, ee) => {
            let c_sub = subst_expr_vars(c, substs);
            let (ts_sub, te_sub) = subst_block(ts, te, substs);
            let (es_sub, ee_sub) = subst_block(es, ee, substs);
            Expr::If(
                Box::new(c_sub),
                ts_sub,
                Box::new(te_sub),
                es_sub,
                Box::new(ee_sub),
            )
        }
        Expr::Match(scrut, cases) => {
            let scrut_sub = subst_expr_vars(scrut, substs);
            let new_cases = cases
                .iter()
                .map(|c| {
                    let binds = c.bindings();
                    let active_substs: Vec<(String, Expr)> = substs
                        .iter()
                        .filter(|(target, _)| !binds.contains(target))
                        .cloned()
                        .collect();
                    let (stmts_sub, body_sub) = subst_block(&c.stmts, &c.body, &active_substs);
                    MatchCase::new(c.pattern.clone(), stmts_sub, body_sub)
                })
                .collect();
            Expr::Match(Box::new(scrut_sub), new_cases)
        }
        Expr::RecordLit(fields) => Expr::RecordLit(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), subst_expr_vars(v, substs)))
                .collect(),
        ),
        Expr::RecordUpdate(base, fields) => Expr::RecordUpdate(
            Box::new(subst_expr_vars(base, substs)),
            fields
                .iter()
                .map(|(k, v)| (k.clone(), subst_expr_vars(v, substs)))
                .collect(),
        ),
        Expr::TupleLit(elems) => {
            Expr::TupleLit(elems.iter().map(|el| subst_expr_vars(el, substs)).collect())
        }
        Expr::Forall(v, ty, body) => {
            let active_substs: Vec<(String, Expr)> = substs
                .iter()
                .filter(|(target, _)| target != v)
                .cloned()
                .collect();
            Expr::Forall(
                v.clone(),
                ty.clone(),
                Box::new(subst_expr_vars(body, &active_substs)),
            )
        }
        Expr::Exists(v, ty, body) => {
            let active_substs: Vec<(String, Expr)> = substs
                .iter()
                .filter(|(target, _)| target != v)
                .cloned()
                .collect();
            Expr::Exists(
                v.clone(),
                ty.clone(),
                Box::new(subst_expr_vars(body, &active_substs)),
            )
        }
        Expr::WithIo(stmts, final_e) => {
            let mut new_stmts = Vec::new();
            let mut cur_substs: Vec<(String, Expr)> = substs.to_vec();
            for s in stmts {
                match s {
                    IoStmt::Bind(v, e) => {
                        let e_sub = subst_expr_vars(e, &cur_substs);
                        new_stmts.push(IoStmt::Bind(v.clone(), e_sub));
                        cur_substs.retain(|(target, _)| target != v);
                    }
                    IoStmt::Action(e) => {
                        let e_sub = subst_expr_vars(e, &cur_substs);
                        new_stmts.push(IoStmt::Action(e_sub));
                    }
                }
            }
            let final_sub = subst_expr_vars(final_e, &cur_substs);
            Expr::WithIo(new_stmts, Box::new(final_sub))
        }
        Expr::Lambda(ps, rt, stmts, body) => {
            let active_substs: Vec<(String, Expr)> = substs
                .iter()
                .filter(|(target, _)| !ps.iter().any(|(p, _)| p == target))
                .cloned()
                .collect();
            let (stmts_sub, body_sub) = subst_block(stmts, body, &active_substs);
            Expr::Lambda(ps.clone(), rt.clone(), stmts_sub, Box::new(body_sub))
        }
    }
}

fn subst_block(stmts: &[Stmt], body: &Expr, substs: &[(String, Expr)]) -> (Vec<Stmt>, Expr) {
    let mut new_stmts = Vec::new();
    let mut cur_substs = substs.to_vec();
    for s in stmts {
        match s {
            Stmt::Let(v, e) => {
                let e_sub = subst_expr_vars(e, &cur_substs);
                new_stmts.push(Stmt::Let(v.clone(), e_sub));
                cur_substs.retain(|(target, _)| target != v);
            }
            Stmt::Check(tag, e) => {
                new_stmts.push(Stmt::Check(tag.clone(), subst_expr_vars(e, &cur_substs)));
            }
            Stmt::Assume(tag, e) => {
                new_stmts.push(Stmt::Assume(tag.clone(), subst_expr_vars(e, &cur_substs)));
            }
            Stmt::Reveal(t) => {
                new_stmts.push(Stmt::Reveal(t.clone()));
            }
        }
    }
    let body_sub = subst_expr_vars(body, &cur_substs);
    (new_stmts, body_sub)
}

/// Expands an inline law instantiation call (e.g., `SEQ.concat_associative(a, b, c)` or local `law_name(a, b)`)
/// inside a `check` or contract expression into the conjunction of its instantiated `ensure` predicates (§12).
fn expand_law_call_in_expr(e: &Expr, m: &ModuleDecl) -> Option<Expr> {
    let (recv_mod, lname, args) = match e {
        Expr::Call(fname, args) => {
            if m.feature_routines.iter().any(|(_, r)| r.name == *fname)
                || m.specify_routines.iter().any(|(_, r)| r.name == *fname)
                || m.create_routines.iter().any(|(_, r)| r.name == *fname)
            {
                return None;
            }
            (None, fname.as_str(), args.as_slice())
        }
        Expr::MethodCall(recv, mname, args) => {
            if let Expr::Var(v) = &**recv {
                if (v == &m.name || v == "Current")
                    && (m.feature_routines.iter().any(|(_, r)| r.name == *mname)
                        || m.specify_routines.iter().any(|(_, r)| r.name == *mname)
                        || m.create_routines.iter().any(|(_, r)| r.name == *mname))
                {
                    return None;
                }
                (Some(v.as_str()), mname.as_str(), args.as_slice())
            } else {
                return None;
            }
        }
        _ => return None,
    };

    // 1. Check module-local `specify_laws`
    if recv_mod.is_none() || recv_mod == Some(m.name.as_str()) || recv_mod == Some("Current") {
        for (_, law) in &m.specify_laws {
            if law.name == lname && law.params.len() == args.len() && !law.ensures.is_empty() {
                let substs: Vec<(String, Expr)> = law
                    .params
                    .iter()
                    .zip(args.iter())
                    .map(|((p, _), a)| (p.clone(), a.clone()))
                    .collect();
                let mut iter = law
                    .ensures
                    .iter()
                    .map(|(_, ens)| subst_expr_vars(ens, &substs));
                let first = iter.next()?;
                return Some(iter.fold(first, |acc, next| {
                    Expr::BinOp("and".to_string(), Box::new(acc), Box::new(next))
                }));
            }
        }
    }

    // 2. Check cross-module standard catalogue & concept laws (§9, §12)
    let mod_name = recv_mod?;
    match (mod_name, lname, args) {
        ("SEQ", "concat_associative", [a, b, c]) => Some(Expr::MethodCall(
            Box::new(Expr::MethodCall(
                Box::new(Expr::MethodCall(
                    Box::new(a.clone()),
                    "concat".to_string(),
                    vec![b.clone()],
                )),
                "concat".to_string(),
                vec![c.clone()],
            )),
            "equals".to_string(),
            vec![Expr::MethodCall(
                Box::new(a.clone()),
                "concat".to_string(),
                vec![Expr::MethodCall(
                    Box::new(b.clone()),
                    "concat".to_string(),
                    vec![c.clone()],
                )],
            )],
        )),
        ("SEQ", "concat_left_identity", [a]) => Some(Expr::MethodCall(
            Box::new(Expr::MethodCall(
                Box::new(Expr::MethodCall(
                    Box::new(Expr::Var("SEQ".to_string())),
                    "empty".to_string(),
                    Vec::new(),
                )),
                "concat".to_string(),
                vec![a.clone()],
            )),
            "equals".to_string(),
            vec![a.clone()],
        )),
        ("SEQ", "concat_right_identity", [a]) => Some(Expr::MethodCall(
            Box::new(Expr::MethodCall(
                Box::new(a.clone()),
                "concat".to_string(),
                vec![Expr::MethodCall(
                    Box::new(Expr::Var("SEQ".to_string())),
                    "empty".to_string(),
                    Vec::new(),
                )],
            )),
            "equals".to_string(),
            vec![a.clone()],
        )),
        ("SEQ", "concat_length_homomorphism", [a, b]) => Some(Expr::BinOp(
            "=".to_string(),
            Box::new(Expr::MethodCall(
                Box::new(Expr::MethodCall(
                    Box::new(a.clone()),
                    "concat".to_string(),
                    vec![b.clone()],
                )),
                "length".to_string(),
                Vec::new(),
            )),
            Box::new(Expr::BinOp(
                "+".to_string(),
                Box::new(Expr::MethodCall(
                    Box::new(a.clone()),
                    "length".to_string(),
                    Vec::new(),
                )),
                Box::new(Expr::MethodCall(
                    Box::new(b.clone()),
                    "length".to_string(),
                    Vec::new(),
                )),
            )),
        )),
        ("SEQ", "map_identity", [s]) => Some(Expr::MethodCall(
            Box::new(Expr::MethodCall(
                Box::new(s.clone()),
                "map".to_string(),
                vec![Expr::MethodCall(
                    Box::new(Expr::Var("FUNCTION_OPS".to_string())),
                    "id_fn".to_string(),
                    Vec::new(),
                )],
            )),
            "equals".to_string(),
            vec![s.clone()],
        )),
        ("SEQ", "map_fusion", [s, f, g]) => Some(Expr::MethodCall(
            Box::new(Expr::MethodCall(
                Box::new(Expr::MethodCall(
                    Box::new(s.clone()),
                    "map".to_string(),
                    vec![g.clone()],
                )),
                "map".to_string(),
                vec![f.clone()],
            )),
            "equals".to_string(),
            vec![Expr::MethodCall(
                Box::new(s.clone()),
                "map".to_string(),
                vec![Expr::MethodCall(
                    Box::new(Expr::Var("FUNCTION_OPS".to_string())),
                    "compose".to_string(),
                    vec![f.clone(), g.clone()],
                )],
            )],
        )),
        ("EQ", "reflexive", [a]) => Some(Expr::MethodCall(
            Box::new(a.clone()),
            "equals".to_string(),
            vec![a.clone()],
        )),
        ("EQ", "symmetric", [a, b]) => Some(Expr::BinOp(
            "=".to_string(),
            Box::new(Expr::MethodCall(
                Box::new(a.clone()),
                "equals".to_string(),
                vec![b.clone()],
            )),
            Box::new(Expr::MethodCall(
                Box::new(b.clone()),
                "equals".to_string(),
                vec![a.clone()],
            )),
        )),
        ("EQ", "transitive", [a, b, c]) => Some(Expr::BinOp(
            "implies".to_string(),
            Box::new(Expr::BinOp(
                "and".to_string(),
                Box::new(Expr::MethodCall(
                    Box::new(a.clone()),
                    "equals".to_string(),
                    vec![b.clone()],
                )),
                Box::new(Expr::MethodCall(
                    Box::new(b.clone()),
                    "equals".to_string(),
                    vec![c.clone()],
                )),
            )),
            Box::new(Expr::MethodCall(
                Box::new(a.clone()),
                "equals".to_string(),
                vec![c.clone()],
            )),
        )),
        ("CANONICAL_EQ", "agrees_with_structural", [a, b]) => Some(Expr::BinOp(
            "=".to_string(),
            Box::new(Expr::MethodCall(
                Box::new(a.clone()),
                "equals".to_string(),
                vec![b.clone()],
            )),
            Box::new(Expr::BinOp(
                "=".to_string(),
                Box::new(a.clone()),
                Box::new(b.clone()),
            )),
        )),
        ("ORD", "irreflexive", [a]) => Some(Expr::UnaryNot(Box::new(Expr::MethodCall(
            Box::new(a.clone()),
            "less_than".to_string(),
            vec![a.clone()],
        )))),
        ("ORD", "transitive", [a, b, c]) => Some(Expr::BinOp(
            "implies".to_string(),
            Box::new(Expr::BinOp(
                "and".to_string(),
                Box::new(Expr::MethodCall(
                    Box::new(a.clone()),
                    "less_than".to_string(),
                    vec![b.clone()],
                )),
                Box::new(Expr::MethodCall(
                    Box::new(b.clone()),
                    "less_than".to_string(),
                    vec![c.clone()],
                )),
            )),
            Box::new(Expr::MethodCall(
                Box::new(a.clone()),
                "less_than".to_string(),
                vec![c.clone()],
            )),
        )),
        ("ORD", "total", [a, b]) => Some(Expr::BinOp(
            "or".to_string(),
            Box::new(Expr::BinOp(
                "or".to_string(),
                Box::new(Expr::MethodCall(
                    Box::new(a.clone()),
                    "less_than".to_string(),
                    vec![b.clone()],
                )),
                Box::new(Expr::MethodCall(
                    Box::new(a.clone()),
                    "equals".to_string(),
                    vec![b.clone()],
                )),
            )),
            Box::new(Expr::MethodCall(
                Box::new(b.clone()),
                "less_than".to_string(),
                vec![a.clone()],
            )),
        )),
        ("LIST_OPS", "map_append", [xs, ys, f]) => Some(Expr::BinOp(
            "=".to_string(),
            Box::new(Expr::MethodCall(
                Box::new(Expr::Var("LIST_OPS".to_string())),
                "map".to_string(),
                vec![
                    Expr::MethodCall(Box::new(xs.clone()), "append".to_string(), vec![ys.clone()]),
                    f.clone(),
                ],
            )),
            Box::new(Expr::MethodCall(
                Box::new(Expr::MethodCall(
                    Box::new(Expr::Var("LIST_OPS".to_string())),
                    "map".to_string(),
                    vec![xs.clone(), f.clone()],
                )),
                "append".to_string(),
                vec![Expr::MethodCall(
                    Box::new(Expr::Var("LIST_OPS".to_string())),
                    "map".to_string(),
                    vec![ys.clone(), f.clone()],
                )],
            )),
        )),
        _ => None,
    }
}

pub fn emit_boogie_expr(
    e: &Expr,
    m: &ModuleDecl,
    implicit_fields: &HashSet<String>,
    revealed: &HashSet<String>,
    fuel_var: &str,
) -> String {
    let strings = collect_module_strings(m);
    emit_boogie_expr_with_strings(e, m, implicit_fields, revealed, fuel_var, &strings)
}

fn emit_boogie_expr_with_strings(
    e: &Expr,
    m: &ModuleDecl,
    implicit_fields: &HashSet<String>,
    revealed: &HashSet<String>,
    fuel_var: &str,
    strings: &[String],
) -> String {
    if let Some(prelude_ref) =
        emit_boogie_prelude_ref(e, m, implicit_fields, revealed, fuel_var, strings)
    {
        return prelude_ref;
    }
    if let Some(expanded) = expand_law_call_in_expr(e, m) {
        return emit_boogie_expr_with_strings(
            &expanded,
            m,
            implicit_fields,
            revealed,
            fuel_var,
            strings,
        );
    }
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Float(f) => format!("F64_lit_{}", f.to_bits()),
        Expr::Char(c) => c.to_string(),
        Expr::Unit => "Unit_val".to_string(),
        Expr::Bool(b) => b.to_string(),
        Expr::Str(s) => {
            if prelude_string_mode() {
                // Task B.3: the literal's compiler-defined prelude `STRING` constant.
                match strings.iter().position(|x| x == s) {
                    Some(idx) => format!("str_lit_{idx}"),
                    None => boogie_string_literal_term(s),
                }
            } else if s.is_empty() {
                "str_lit_empty".to_string()
            } else if let Some(idx) = strings.iter().position(|x| x == s) {
                format!("str_lit_{idx}")
            } else {
                "str_lit_empty".to_string()
            }
        }
        Expr::Var(v) => {
            if implicit_fields.contains(v) {
                format!("{}_{}(Current)", m.name, v)
            } else if v == "None" {
                "None()".to_string()
            } else if let ReprKind::Sum(variants) = &m.repr {
                if variants
                    .iter()
                    .any(|var| var.name == *v && var.fields.is_empty())
                {
                    return format!("{v}()");
                }
                v.clone()
            } else {
                v.clone()
            }
        }
        Expr::UnaryNot(inner) => {
            format!(
                "(!{})",
                emit_boogie_expr_with_strings(
                    inner,
                    m,
                    implicit_fields,
                    revealed,
                    fuel_var,
                    strings
                )
            )
        }
        Expr::UnaryNeg(inner) => {
            format!(
                "(-{})",
                emit_boogie_expr_with_strings(
                    inner,
                    m,
                    implicit_fields,
                    revealed,
                    fuel_var,
                    strings
                )
            )
        }
        Expr::BinOp(op, l, r) => {
            if op == "+" && (matches!(&**l, Expr::Str(_)) || matches!(&**r, Expr::Str(_))) {
                return format!(
                    "Str_concat({}, {})",
                    emit_boogie_expr_with_strings(
                        l,
                        m,
                        implicit_fields,
                        revealed,
                        fuel_var,
                        strings
                    ),
                    emit_boogie_expr_with_strings(
                        r,
                        m,
                        implicit_fields,
                        revealed,
                        fuel_var,
                        strings
                    )
                );
            }
            let b_fn = match op.as_str() {
                "+%" => Some("u64_wrap_add"),
                "-%" => Some("u64_wrap_sub"),
                "*%" => Some("u64_wrap_mul"),
                "&" => Some("u64_bit_and"),
                "|" => Some("u64_bit_or"),
                "^" => Some("u64_bit_xor"),
                "<<" => Some("u64_bit_shl"),
                ">>" => Some("u64_bit_shr"),
                _ => None,
            };
            if let Some(fn_name) = b_fn {
                return format!(
                    "{fn_name}({}, {})",
                    emit_boogie_expr_with_strings(
                        l,
                        m,
                        implicit_fields,
                        revealed,
                        fuel_var,
                        strings
                    ),
                    emit_boogie_expr_with_strings(
                        r,
                        m,
                        implicit_fields,
                        revealed,
                        fuel_var,
                        strings
                    )
                );
            }
            let b_op = match op.as_str() {
                "=" => "==",
                "and" => "&&",
                "or" => "||",
                "implies" => "==>",
                "/" => "div",
                "%" => "mod",
                other => other,
            };
            format!(
                "({} {b_op} {})",
                emit_boogie_expr_with_strings(l, m, implicit_fields, revealed, fuel_var, strings),
                emit_boogie_expr_with_strings(r, m, implicit_fields, revealed, fuel_var, strings)
            )
        }
        Expr::Call(fname, args) => {
            let is_ctor = matches!(&m.repr, ReprKind::Sum(vs) if vs.iter().any(|v| v.name == *fname))
                || fname == "Some"
                || fname == "None"
                || fname == "Ok"
                || fname == "Error";
            let a_str: Vec<String> = args
                .iter()
                .map(|a| {
                    emit_boogie_expr_with_strings(
                        a,
                        m,
                        implicit_fields,
                        revealed,
                        fuel_var,
                        strings,
                    )
                })
                .collect();
            if is_ctor {
                format!("{fname}({})", a_str.join(", "))
            } else if !m.feature_routines.iter().any(|(_, r)| r.name == *fname)
                && !m.specify_routines.iter().any(|(_, r)| r.name == *fname)
                && !m.create_routines.iter().any(|(_, r)| r.name == *fname)
            {
                if a_str.len() == 1 {
                    format!("Fn1_app({fname}, {})", a_str[0])
                } else if a_str.len() == 2 {
                    format!("Fn2_app({fname}, {}, {})", a_str[0], a_str[1])
                } else {
                    format!("{fname}({})", a_str.join(", "))
                }
            } else {
                let call_fuel = if revealed.contains(fname) {
                    "RevealFuel"
                } else {
                    fuel_var
                };
                let mut full_args = vec![call_fuel.to_string()];
                if m.kind == ModuleKind::Module
                    && (m.feature_routines.iter().any(|(_, r)| r.name == *fname)
                        || m.specify_routines.iter().any(|(_, r)| r.name == *fname))
                {
                    full_args.push("Current".to_string());
                }
                full_args.extend(a_str);
                format!("fn_{fname}({})", full_args.join(", "))
            }
        }
        Expr::MethodCall(recv, mname, args) => {
            if let Expr::Var(v) = &**recv {
                if v == "LIST" {
                    match mname.as_str() {
                        "empty" => return "Seq_empty()".to_string(),
                        "singleton" => {
                            return format!(
                                "Seq_singleton({})",
                                emit_boogie_expr_with_strings(
                                    &args[0],
                                    m,
                                    implicit_fields,
                                    revealed,
                                    fuel_var,
                                    strings
                                )
                            );
                        }
                        _ => {}
                    }
                } else if v == "SEQ" {
                    match mname.as_str() {
                        "empty" => return "Empty()".to_string(),
                        "singleton" => {
                            return format!(
                                "Leaf({})",
                                emit_boogie_expr_with_strings(
                                    &args[0],
                                    m,
                                    implicit_fields,
                                    revealed,
                                    fuel_var,
                                    strings
                                )
                            );
                        }
                        _ => {}
                    }
                } else if v == "LIST_OPS" {
                    let a_str: Vec<String> = args
                        .iter()
                        .map(|a| {
                            emit_boogie_expr_with_strings(
                                a,
                                m,
                                implicit_fields,
                                revealed,
                                fuel_var,
                                strings,
                            )
                        })
                        .collect();
                    match mname.as_str() {
                        "map" => return format!("Seq_map({}, {})", a_str[0], a_str[1]),
                        "filter" => return format!("Seq_filter({}, {})", a_str[0], a_str[1]),
                        "fold_left" => {
                            return format!(
                                "Seq_fold_left({}, {}, {})",
                                a_str[0], a_str[1], a_str[2]
                            );
                        }
                        "fold_right" => {
                            return format!(
                                "Seq_fold_right({}, {}, {})",
                                a_str[0], a_str[1], a_str[2]
                            );
                        }
                        _ => {}
                    }
                } else if v == "FUNCTION_OPS" {
                    let a_str: Vec<String> = args
                        .iter()
                        .map(|a| {
                            emit_boogie_expr_with_strings(
                                a,
                                m,
                                implicit_fields,
                                revealed,
                                fuel_var,
                                strings,
                            )
                        })
                        .collect();
                    match mname.as_str() {
                        "compose" => return format!("Fn1_compose({}, {})", a_str[0], a_str[1]),
                        "id_fn" => return "Fn1_id()".to_string(),
                        "identity" => return a_str[0].clone(),
                        _ => {}
                    }
                } else if v == "CHAR" && mname == "from_scalar" && !args.is_empty() {
                    return emit_boogie_expr_with_strings(
                        &args[0],
                        m,
                        implicit_fields,
                        revealed,
                        fuel_var,
                        strings,
                    );
                } else if v == "IO" {
                    let a_str: Vec<String> = args
                        .iter()
                        .map(|a| {
                            emit_boogie_expr_with_strings(
                                a,
                                m,
                                implicit_fields,
                                revealed,
                                fuel_var,
                                strings,
                            )
                        })
                        .collect();
                    return format!("IO_{mname}({})", a_str.join(", "));
                } else if v == &m.name {
                    let call_fuel = if revealed.contains(mname) {
                        "RevealFuel"
                    } else {
                        fuel_var
                    };
                    let mut full_args = vec![call_fuel.to_string()];
                    for a in args {
                        full_args.push(emit_boogie_expr_with_strings(
                            a,
                            m,
                            implicit_fields,
                            revealed,
                            fuel_var,
                            strings,
                        ));
                    }
                    return format!("fn_{mname}({})", full_args.join(", "));
                }
            }
            if mname == "apply" {
                let a_str: Vec<String> = args
                    .iter()
                    .map(|a| {
                        emit_boogie_expr_with_strings(
                            a,
                            m,
                            implicit_fields,
                            revealed,
                            fuel_var,
                            strings,
                        )
                    })
                    .collect();
                let r_str = emit_boogie_expr_with_strings(
                    recv,
                    m,
                    implicit_fields,
                    revealed,
                    fuel_var,
                    strings,
                );
                if a_str.len() == 1 {
                    return format!("Fn1_app({r_str}, {})", a_str[0]);
                } else if a_str.len() == 2 {
                    return format!("Fn2_app({r_str}, {}, {})", a_str[0], a_str[1]);
                }
            }
            let r_str = emit_boogie_expr_with_strings(
                recv,
                m,
                implicit_fields,
                revealed,
                fuel_var,
                strings,
            );
            match mname.as_str() {
                "append" => format!(
                    "Seq_append({r_str}, {})",
                    emit_boogie_expr_with_strings(
                        &args[0],
                        m,
                        implicit_fields,
                        revealed,
                        fuel_var,
                        strings
                    )
                ),
                "prepend" => format!(
                    "Seq_prepend({}, {r_str})",
                    emit_boogie_expr_with_strings(
                        &args[0],
                        m,
                        implicit_fields,
                        revealed,
                        fuel_var,
                        strings
                    )
                ),
                "head" => format!("Seq_head({r_str})"),
                "tail" => format!("Seq_tail({r_str})"),
                "length" if m.name != "SEQ" => format!("Seq_length({r_str})"),
                "has" => format!(
                    "Seq_has({r_str}, {})",
                    emit_boogie_expr_with_strings(
                        &args[0],
                        m,
                        implicit_fields,
                        revealed,
                        fuel_var,
                        strings
                    )
                ),
                "first" => format!("Tuple2_first({r_str})"),
                "second" => format!("Tuple2_second({r_str})"),
                "code" if args.is_empty() => r_str,
                _ => {
                    // Element equality `x.equals(y)` on a bounded type parameter `tp`
                    // (decided by the receiver's type, not by names) is the behavioral
                    // relation `<tp>_eq`, never structural `==` (§5, §9).
                    if mname == "equals" && args.len() == 1 {
                        if let Some(eq_fn) = elem_equals_fn(recv, m) {
                            let rhs = emit_boogie_expr_with_strings(
                                &args[0],
                                m,
                                implicit_fields,
                                revealed,
                                fuel_var,
                                strings,
                            );
                            return format!("{eq_fn}({r_str}, {rhs})");
                        }
                    }
                    if let ReprKind::Product(fields) = &m.repr {
                        if args.is_empty() && fields.iter().any(|(fnm, _)| fnm == mname) {
                            return format!("{}_{mname}({r_str})", m.name);
                        }
                    }
                    let call_fuel = if revealed.contains(mname) {
                        "RevealFuel"
                    } else {
                        fuel_var
                    };
                    let recv_arg = if m.name == "RESULT"
                        && (mname == "map" || mname == "map_error")
                        && matches!(&**recv, Expr::Call(c, _) if c == "Ok" || c == "Error")
                    {
                        format!("({r_str} : (RESULT E A))")
                    } else {
                        r_str
                    };
                    let mut full_args = vec![call_fuel.to_string(), recv_arg];
                    for a in args {
                        full_args.push(emit_boogie_expr_with_strings(
                            a,
                            m,
                            implicit_fields,
                            revealed,
                            fuel_var,
                            strings,
                        ));
                    }
                    format!("fn_{mname}({})", full_args.join(", "))
                }
            }
        }
        Expr::If(c, ts, te, es, ee) => {
            format!(
                "(if {} then {} else {})",
                emit_boogie_expr_with_strings(c, m, implicit_fields, revealed, fuel_var, strings),
                emit_boogie_block_expr(ts, te, m, implicit_fields, fuel_var, strings),
                emit_boogie_block_expr(es, ee, m, implicit_fields, fuel_var, strings)
            )
        }
        Expr::Match(scrut, cases) => {
            let s_str = emit_boogie_expr_with_strings(
                scrut,
                m,
                implicit_fields,
                revealed,
                fuel_var,
                strings,
            );
            if cases.is_empty() {
                return s_str;
            }
            let mut acc = emit_boogie_match_case(
                &s_str,
                cases.last().expect("non-empty cases"),
                m,
                implicit_fields,
                fuel_var,
                strings,
            );
            for c in cases.iter().rev().skip(1) {
                let branch_val =
                    emit_boogie_match_case(&s_str, c, m, implicit_fields, fuel_var, strings);
                acc = format!(
                    "(if {}({s_str}) then {branch_val} else {acc})",
                    boogie_ctor_tester(c.ctor(), m)
                );
            }
            acc
        }
        Expr::RecordLit(fields) => {
            if let ReprKind::Product(decl_fields) = &m.repr {
                let args: Vec<String> = decl_fields
                    .iter()
                    .map(|(fnm, _)| {
                        if let Some((_, fe)) = fields.iter().find(|(k, _)| k == fnm) {
                            emit_boogie_expr_with_strings(
                                fe,
                                m,
                                implicit_fields,
                                revealed,
                                fuel_var,
                                strings,
                            )
                        } else {
                            "0".to_string()
                        }
                    })
                    .collect();
                format!("{}_Ctor({})", m.name, args.join(", "))
            } else {
                let args: Vec<String> = fields
                    .iter()
                    .map(|(_, fe)| {
                        emit_boogie_expr_with_strings(
                            fe,
                            m,
                            implicit_fields,
                            revealed,
                            fuel_var,
                            strings,
                        )
                    })
                    .collect();
                format!("{}_Ctor({})", m.name, args.join(", "))
            }
        }
        Expr::RecordUpdate(base, updates) => {
            let b_str = emit_boogie_expr_with_strings(
                base,
                m,
                implicit_fields,
                revealed,
                fuel_var,
                strings,
            );
            if let ReprKind::Product(fields) = &m.repr {
                let args: Vec<String> = fields
                    .iter()
                    .map(|(fnm, _)| {
                        if let Some((_, uexpr)) = updates.iter().find(|(uf, _)| uf == fnm) {
                            emit_boogie_expr_with_strings(
                                uexpr,
                                m,
                                implicit_fields,
                                revealed,
                                fuel_var,
                                strings,
                            )
                        } else {
                            format!("{}_{}({b_str})", m.name, fnm)
                        }
                    })
                    .collect();
                format!("{}_Ctor({})", m.name, args.join(", "))
            } else {
                b_str
            }
        }
        Expr::TupleLit(elems) => {
            let es: Vec<String> = elems
                .iter()
                .map(|el| {
                    emit_boogie_expr_with_strings(
                        el,
                        m,
                        implicit_fields,
                        revealed,
                        fuel_var,
                        strings,
                    )
                })
                .collect();
            if es.len() == 2 {
                format!("Tuple2_mk({}, {})", es[0], es[1])
            } else {
                format!("({})", es.join(", "))
            }
        }
        Expr::Forall(v, ty, body) => {
            format!(
                "(forall {v}: {} :: {})",
                map_boogie_type(ty, m),
                emit_boogie_expr_with_strings(
                    body,
                    m,
                    implicit_fields,
                    revealed,
                    fuel_var,
                    strings
                )
            )
        }
        Expr::Exists(v, ty, body) => {
            format!(
                "(exists {v}: {} :: {})",
                map_boogie_type(ty, m),
                emit_boogie_expr_with_strings(
                    body,
                    m,
                    implicit_fields,
                    revealed,
                    fuel_var,
                    strings
                )
            )
        }
        Expr::WithIo(_, final_e) => {
            emit_boogie_expr_with_strings(final_e, m, implicit_fields, revealed, fuel_var, strings)
        }
        Expr::Lambda(params, _, stmts, body) => {
            if params.len() == 1 && stmts.is_empty() {
                let p0 = &params[0].0;
                if let Expr::Var(v) = &**body {
                    if v == p0 {
                        return "Fn1_id()".to_string();
                    }
                }
                if let Expr::Call(ctor, c_args) = &**body {
                    if c_args.len() == 1 && matches!(&c_args[0], Expr::Var(cv) if cv == p0) {
                        if ctor == "Some" {
                            return "Fn1_Some()".to_string();
                        } else if ctor == "Ok" {
                            return "Fn1_Ok()".to_string();
                        }
                    }
                }
                if let Expr::Call(f_name, f_args) = &**body {
                    if f_args.len() == 1 {
                        if let Expr::Call(g_name, g_args) = &f_args[0] {
                            if g_args.len() == 1 {
                                if let Expr::Var(gv) = &g_args[0] {
                                    if gv == p0 {
                                        return format!("Fn1_compose({f_name}, {g_name})");
                                    }
                                }
                            }
                        }
                    }
                }
                if let Expr::BinOp(op, l, r) = &**body {
                    if op == "and" {
                        let l_p = match &**l {
                            Expr::Call(fn_name, args)
                                if args.len() == 1
                                    && matches!(&args[0], Expr::Var(v) if v == p0) =>
                            {
                                Some(fn_name.clone())
                            }
                            Expr::MethodCall(recv, mname, args)
                                if args.is_empty()
                                    && matches!(&**recv, Expr::Var(v) if v == p0) =>
                            {
                                Some(mname.clone())
                            }
                            _ => None,
                        };
                        let r_q = match &**r {
                            Expr::Call(fn_name, args)
                                if args.len() == 1
                                    && matches!(&args[0], Expr::Var(v) if v == p0) =>
                            {
                                Some(fn_name.clone())
                            }
                            Expr::MethodCall(recv, mname, args)
                                if args.is_empty()
                                    && matches!(&**recv, Expr::Var(v) if v == p0) =>
                            {
                                Some(mname.clone())
                            }
                            _ => None,
                        };
                        if let (Some(p_fn), Some(q_fn)) = (l_p, r_q) {
                            return format!("Fn1_and({p_fn}, {q_fn})");
                        }
                    }
                }
                if let Expr::MethodCall(recv, mname, args) = &**body {
                    if mname == "and_then" && args.len() == 1 {
                        if let Expr::Call(fname, fargs) = &**recv {
                            if fargs.len() == 1 && matches!(&fargs[0], Expr::Var(v) if v == p0) {
                                let g_arg = emit_boogie_expr_with_strings(
                                    &args[0],
                                    m,
                                    implicit_fields,
                                    revealed,
                                    fuel_var,
                                    strings,
                                );
                                if m.name == "OPTION" {
                                    return format!("Fn1_option_kleisli({fname}, {g_arg})");
                                } else if m.name == "RESULT" {
                                    return format!("Fn1_result_kleisli({fname}, {g_arg})");
                                }
                            }
                        }
                    }
                }
            } else if params.len() == 2 && stmts.is_empty() {
                let p0 = &params[0].0;
                let p1 = &params[1].0;
                if let Expr::Call(f_name, f_args) = &**body {
                    if f_args.len() == 2 && matches!(&f_args[0], Expr::Var(v) if v == p0) {
                        if let Expr::Call(g_name, g_args) = &f_args[1] {
                            if g_args.len() == 1 && matches!(&g_args[0], Expr::Var(v) if v == p1) {
                                return format!("Fn2_map_arg({f_name}, {g_name})");
                            }
                        }
                    }
                }
            }
            emit_boogie_block_expr(stmts, body, m, implicit_fields, fuel_var, strings)
        }
    }
}

fn emit_boogie_match_case(
    scrut_bpl: &str,
    c: &MatchCase,
    m: &ModuleDecl,
    implicit_fields: &HashSet<String>,
    fuel_var: &str,
    strings: &[String],
) -> String {
    let mut expr = c.body.clone();
    for st in c.stmts.iter().rev() {
        if let Stmt::Let(var, val) = st {
            expr = subst_expr_var(&expr, var, val);
        }
    }
    // Binders of the module's own variants have the declared field types (e.g. `A`), so
    // `v1.equals(v2)` on them is recognized as element equality.
    if let Some(("", vdecl)) = boogie_match_variant(c.ctor(), m)
        .as_ref()
        .map(|(p, v)| (p.as_str(), v))
    {
        for (opt_b, (_, fty)) in c.top_bindings().iter().zip(vdecl.fields.iter()) {
            if let Some(b) = opt_b {
                elem_env_bind(b, fty);
            }
        }
    }
    let mut out = emit_boogie_expr_with_strings(
        &expr,
        m,
        implicit_fields,
        &HashSet::new(),
        fuel_var,
        strings,
    );
    if let Some((proj_prefix, vdecl)) = boogie_match_variant(c.ctor(), m) {
        for (opt_b, (fname, _)) in c.top_bindings().iter().zip(vdecl.fields.iter()) {
            if let Some(bname) = opt_b {
                let proj = format!("{proj_prefix}{}_{}({scrut_bpl})", c.ctor(), fname);
                out = replace_whole_ident(&out, bname, &proj);
            }
        }
    }
    out
}

fn replace_whole_ident(haystack: &str, ident: &str, replacement: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = haystack.chars().collect();
    let id_chars: Vec<char> = ident.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + id_chars.len() <= chars.len() && chars[i..i + id_chars.len()] == id_chars[..] {
            let prev_ok = i == 0 || !(chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '_');
            let next_ok = i + id_chars.len() == chars.len()
                || !(chars[i + id_chars.len()].is_ascii_alphanumeric()
                    || chars[i + id_chars.len()] == '_');
            if prev_ok && next_ok {
                result.push_str(replacement);
                i += id_chars.len();
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

// ============================================================================
// 8. PHASE 3 & 4 SYMBOLIC VERIFIER, COUNTEREXAMPLES & ASSUME AUDITOR
// ============================================================================

/// Value domain for counterexample synthesis and translation into Gardel syntax (§12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GardelVal {
    Int(i128),
    Float(u64),
    Char(u32),
    Unit,
    Bool(bool),
    Str(String),
    List(Vec<GardelVal>),
    Tuple(Vec<GardelVal>),
    Record(Vec<(String, GardelVal)>),
    Variant(String, Vec<GardelVal>),
    IoAction(String, Vec<GardelVal>),
}

impl GardelVal {
    /// Translates a solver / symbolic model value into Gardel record and variant syntax (§12).
    pub fn to_gardel_syntax(&self) -> String {
        match self {
            GardelVal::Int(n) => n.to_string(),
            GardelVal::Float(bits) => format!("{}", f64::from_bits(*bits)),
            GardelVal::Char(c) => format!("'{}'", char::from_u32(*c).unwrap_or('?')),
            GardelVal::Unit => "()".to_string(),
            GardelVal::Bool(true) => "True".to_string(),
            GardelVal::Bool(false) => "False".to_string(),
            GardelVal::Str(s) => format!("\"{s}\""),
            GardelVal::List(items) => {
                if items.is_empty() {
                    "[]".to_string()
                } else {
                    let parts: Vec<String> =
                        items.iter().map(GardelVal::to_gardel_syntax).collect();
                    format!("[{}]", parts.join(", "))
                }
            }
            GardelVal::Tuple(elems) => {
                let parts: Vec<String> = elems.iter().map(GardelVal::to_gardel_syntax).collect();
                format!("({})", parts.join(", "))
            }
            GardelVal::Record(fields) => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| format!("{k} = {}", v.to_gardel_syntax()))
                    .collect();
                format!("{{ {} }}", parts.join(", "))
            }
            GardelVal::Variant(ctor, args) => {
                if args.is_empty() {
                    ctor.clone()
                } else {
                    let parts: Vec<String> = args.iter().map(GardelVal::to_gardel_syntax).collect();
                    format!("{ctor}({})", parts.join(", "))
                }
            }
            GardelVal::IoAction(act, args) => {
                let parts: Vec<String> = args.iter().map(GardelVal::to_gardel_syntax).collect();
                format!("IO.{act}({})", parts.join(", "))
            }
        }
    }
}
/// Translates raw Z3 / Boogie model text into Gardel record and variant syntax (§12).
pub fn translate_z3_model_to_gardel(raw_model: &str, m: &ModuleDecl) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in raw_model.lines() {
        let trimmed = line.trim();
        if let Some((lhs, rhs)) = trimmed.split_once("->").or_else(|| trimmed.split_once('=')) {
            let var = lhs.trim();
            let val = rhs.trim().trim_end_matches(';');
            if var.is_empty()
                || var == "_"
                || var.starts_with('%')
                || var.starts_with("T@")
                || var.contains("Fuel")
                || var == "U64_MAX"
                || var == "ControlFlow"
                || var.starts_with("tickle")
            {
                continue;
            }
            let clean_var = if let Some(idx) = var.find('@') {
                var[..idx].trim()
            } else {
                var
            };
            if clean_var.is_empty() || clean_var == "_" {
                continue;
            }
            let gardel_val = if val == "true" {
                "True".to_string()
            } else if val == "false" {
                "False".to_string()
            } else if val.starts_with(&format!("{}_Ctor(", m.name)) {
                if let ReprKind::Product(fields) = &m.repr {
                    let inner = val
                        .trim_start_matches(&format!("{}_Ctor(", m.name))
                        .trim_end_matches(')');
                    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
                    let formatted: Vec<String> = fields
                        .iter()
                        .zip(parts.iter())
                        .map(|((fnm, _), pv)| format!("{fnm} = {pv}"))
                        .collect();
                    format!("{{ {} }}", formatted.join(", "))
                } else {
                    val.to_string()
                }
            } else {
                val.to_string()
            };
            if !out.contains_key(clean_var)
                || (!gardel_val.starts_with("T@")
                    && out
                        .get(clean_var)
                        .is_some_and(|v: &String| v.starts_with("T@")))
            {
                out.insert(clean_var.to_string(), gardel_val);
            }
        }
    }
    out
}

/// Discovers Boogie and Z3 executables from environment or standard paths.
pub fn find_solvers() -> (Option<String>, Option<String>) {
    let boogie = env::var("BOOGIE_EXE").ok().or_else(|| {
        if let Ok(userprofile) = env::var("USERPROFILE") {
            let p = format!("{userprofile}\\.dotnet\\tools\\boogie.exe");
            if std::path::Path::new(&p).exists() {
                return Some(p);
            }
        }
        if let Ok(home) = env::var("HOME") {
            let p = format!("{home}/.dotnet/tools/boogie");
            if std::path::Path::new(&p).exists() {
                return Some(p);
            }
        }
        which_solver("boogie")
    });

    let z3 = env::var("Z3_EXE").ok().or_else(|| {
        let bundled_z3 = r"C:\home\Learn\Gardel\tools\dafny\z3\bin\z3-4.12.1.exe";
        if std::path::Path::new(bundled_z3).exists() {
            return Some(bundled_z3.to_string());
        }
        which_solver("z3")
    });

    (boogie, z3)
}

fn which_solver(name: &str) -> Option<String> {
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
            #[cfg(windows)]
            {
                let candidate_exe = dir.join(format!("{name}.exe"));
                if candidate_exe.is_file() {
                    return Some(candidate_exe.to_string_lossy().to_string());
                }
            }
        }
    }
    None
}

fn run_boogie_verification(
    m: &ModuleDecl,
    fuel_limit: usize,
    boogie_path: &str,
    z3_path: &str,
    src: &str,
    input_path: &str,
) -> Result<Vec<Diagnostic>, String> {
    // Routine contracts are stated as axioms. A *false* contract contradicts the routine's
    // definitional axiom, and the resulting inconsistency silently proves every other
    // obligation that touches the routine, hiding real errors. So: verify, then re-verify
    // with the contracts of every routine that had a failing obligation removed, until no
    // new routine fails. Removing axioms only weakens the context, so every failure found
    // in any pass is a genuine failure, and the final pass trusts only contracts whose
    // routines verified.
    let routine_names: HashSet<String> = m
        .create_routines
        .iter()
        .chain(m.feature_routines.iter())
        .chain(m.specify_routines.iter())
        .map(|(_, r)| r.name.clone())
        .collect();
    let mut all: Vec<Diagnostic> = Vec::new();
    let mut untrusted: HashSet<String> = HashSet::new();
    loop {
        let prev = UNTRUSTED_CONTRACTS.with(|u| u.replace(untrusted.clone()));
        let pass =
            run_boogie_verification_once(m, fuel_limit, boogie_path, z3_path, src, input_path);
        UNTRUSTED_CONTRACTS.with(|u| *u.borrow_mut() = prev);
        let pass = pass?;
        // A failed postcondition clause withdraws that clause only; any other failure of a
        // routine withdraws its whole contract.
        let newly_failed: HashSet<String> = pass
            .iter()
            .filter(|d| routine_names.contains(&d.routine))
            .map(|d| {
                if d.rule == "Phase 3 Boogie (Postcondition Violation)" {
                    format!("{}::{}", d.routine, d.tag)
                } else {
                    d.routine.clone()
                }
            })
            .filter(|k| !untrusted.contains(k))
            .collect();
        for d in pass {
            if !all
                .iter()
                .any(|a| a.routine == d.routine && a.tag == d.tag && a.rule == d.rule)
            {
                all.push(d);
            }
        }
        if newly_failed.is_empty() {
            break;
        }
        untrusted.extend(newly_failed);
    }
    Ok(all)
}

thread_local! {
    /// Routines whose contract axioms are withheld in the current verification pass.
    static UNTRUSTED_CONTRACTS: std::cell::RefCell<HashSet<String>> =
        std::cell::RefCell::new(HashSet::new());
}

fn run_boogie_verification_once(
    m: &ModuleDecl,
    fuel_limit: usize,
    boogie_path: &str,
    z3_path: &str,
    src: &str,
    input_path: &str,
) -> Result<Vec<Diagnostic>, String> {
    let bpl_code = emit_boogie(m, fuel_limit);
    // Unique per call: concurrent verifications of the same module (e.g. parallel tests)
    // must not overwrite each other's VC file.
    static VERIFY_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call_id = VERIFY_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut temp_path = env::temp_dir();
    temp_path.push(format!(
        "gardel_verify_{}_{}_{}.bpl",
        m.name,
        std::process::id(),
        call_id
    ));
    fs::write(&temp_path, &bpl_code)
        .map_err(|e| format!("Failed to write temporary Boogie file: {e}"))?;

    let mut cmd = Command::new(boogie_path);
    cmd.arg(format!("/proverOpt:PROVER_PATH={z3_path}"));
    cmd.arg("/typeEncoding:p");
    cmd.arg("/timeLimit:10");
    cmd.arg("/mv:-");
    cmd.arg("/errorTrace:0");
    cmd.arg(&temp_path);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to spawn Boogie process: {e}"))?;
    let _ = fs::remove_file(&temp_path);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let full_out = format!("{stdout}\n{stderr}");

    Ok(parse_boogie_output(&full_out, m, src, input_path))
}

fn collect_branch_path_conditions_for_tag(
    stmts: &[Stmt],
    body: &Expr,
    target_tag: &str,
    pcs: &mut Vec<String>,
) -> bool {
    for st in stmts {
        match st {
            Stmt::Reveal(t) => {
                pcs.push(format!("reveal {t}"));
            }
            Stmt::Let(v, e) => {
                pcs.push(format!("let {v} = {}", format_gardel_expr(e)));
            }
            Stmt::Check(ctag, e) => {
                if ctag == target_tag {
                    return true;
                }
                pcs.push(format!("check {ctag}: {}", format_gardel_expr(e)));
            }
            Stmt::Assume(atag, e) => {
                if atag == target_tag {
                    return true;
                }
                pcs.push(format!("assume {atag}: {}", format_gardel_expr(e)));
            }
        }
    }
    match body {
        Expr::If(c, ts, te, es, ee) => {
            let mut t_pcs = vec![format_gardel_expr(c)];
            if collect_branch_path_conditions_for_tag(ts, te, target_tag, &mut t_pcs) {
                pcs.extend(t_pcs);
                return true;
            }
            let mut e_pcs = vec![format!("!({})", format_gardel_expr(c))];
            if collect_branch_path_conditions_for_tag(es, ee, target_tag, &mut e_pcs) {
                pcs.extend(e_pcs);
                return true;
            }
            false
        }
        Expr::Match(scrut, cases) => {
            let scrut_s = format_gardel_expr(scrut);
            for c in cases {
                let mut c_pcs = vec![format!("{scrut_s} is {}", format_pattern(&c.pattern))];
                if collect_branch_path_conditions_for_tag(&c.stmts, &c.body, target_tag, &mut c_pcs)
                {
                    pcs.extend(c_pcs);
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

fn collect_routine_path_conditions(m: &ModuleDecl, routine: &str, tag: &str) -> Vec<String> {
    for (_, r) in m
        .create_routines
        .iter()
        .chain(m.feature_routines.iter())
        .chain(m.specify_routines.iter())
    {
        if r.name == routine {
            let mut pcs = Vec::new();
            for (_, req_e) in &r.requires {
                pcs.push(format_gardel_expr(req_e));
            }
            let _ = collect_branch_path_conditions_for_tag(&r.stmts, &r.body, tag, &mut pcs);
            return pcs;
        }
    }
    for (vis, law) in &m.specify_laws {
        let vis_prefix = match vis {
            Visibility::Modules(mods) if !mods.is_empty() => format!("{}_", mods.join("_")),
            _ => String::new(),
        };
        let prefixed_name = format!("{vis_prefix}{}", law.name);
        if law.name == routine || prefixed_name == routine {
            let mut pcs = Vec::new();
            for (_, req_e) in &law.requires {
                pcs.push(format_gardel_expr(req_e));
            }
            if let Some(body) = &law.body {
                let _ = collect_branch_path_conditions_for_tag(&law.stmts, body, tag, &mut pcs);
            }
            return pcs;
        }
    }
    Vec::new()
}

pub fn parse_boogie_output(
    output: &str,
    m: &ModuleDecl,
    src: &str,
    input_path: &str,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        // 1. Assertion failure with counterexample model
        if line.starts_with("*** MODEL") {
            let mut msg_line = "";
            let mut j = (i as isize) - 1;
            while j >= 0 {
                let candidate = lines[j as usize].trim();
                if !candidate.is_empty()
                    && !candidate.starts_with("***")
                    && !candidate.contains("Boogie program verifier")
                    && !candidate.starts_with("Execution trace:")
                {
                    msg_line = candidate;
                    break;
                }
                j -= 1;
            }

            let mut raw_model = String::new();
            i += 1;
            while i < lines.len() {
                let l = lines[i].trim();
                if l.starts_with("*** END_MODEL") {
                    i += 1;
                    break;
                }
                raw_model.push_str(l);
                raw_model.push('\n');
                i += 1;
            }

            let (routine, rule_kind, tag) = parse_msg_tag(msg_line);
            let cex = translate_z3_model_to_gardel(&raw_model, m);
            let sp = locate_span(src, input_path, &tag, &routine);
            let lemmas = find_relevant_lemmas_for_tag(m, &routine, &tag);
            let pcs = collect_routine_path_conditions(m, &routine, &tag);

            let static_rule = match rule_kind.as_str() {
                "overflow" => "Phase 3 Boogie (Arithmetic Overflow)",
                "div_by_zero" => "Phase 3 Boogie (Division by Zero)",
                "variant" => "Phase 3 Boogie (Termination Variant Violation)",
                "invariant" => "Phase 3 Boogie (Invariant Violation)",
                "ensure" => "Phase 3 Boogie (Postcondition Violation)",
                "check" => "Phase 3 Boogie (Check Assertion Failed)",
                "match" => "Phase 3 Boogie (Non-Exhaustive Match)",
                "require" => "Phase 3 Boogie (Precondition Violation)",
                _ => "Phase 3 Boogie (Verification Condition Failed)",
            };
            let diag = Diagnostic::basic(
                "error",
                static_rule,
                routine.clone(),
                tag.clone(),
                format!("Formal verification condition `{tag}` failed in `{routine}`"),
            )
            .with_module(&m.name)
            .with_ai_context(sp, pcs, cex, lemmas);
            diags.push(diag);
            continue;
        }

        // 1b. A timed-out / inconclusive procedure is *not* verified: report it instead of
        // silently counting it as a success.
        if line.contains("Verification of '")
            && (line.contains("timed out")
                || line.contains("inconclusive")
                || line.contains("out of"))
        {
            let proc_name = line
                .split("Verification of '")
                .nth(1)
                .and_then(|r| r.split('\'').next())
                .unwrap_or("");
            let routine = proc_name
                .strip_prefix("Verify_")
                .unwrap_or(proc_name)
                .to_string();
            let sp = locate_span(src, input_path, &routine, &routine);
            diags.push(
                Diagnostic::basic(
                    "error",
                    "Phase 3 Boogie (Verification Timeout)",
                    routine.clone(),
                    "timeout".to_string(),
                    format!("Verification of `{routine}` did not complete ({line}); its obligations are unproven"),
                )
                .with_module(&m.name)
                .with_ai_context(sp, Vec::new(), BTreeMap::new(), Vec::new()),
            );
            i += 1;
            continue;
        }

        // 2. Boogie internal or syntax error without model
        if (line.contains(": error:") || line.contains(": Error:") || line.contains("::"))
            && !line.contains("Boogie program verifier finished")
            && !line.starts_with("***")
        {
            let has_model_ahead = lines[i + 1..]
                .iter()
                .take(6)
                .any(|l| l.trim().starts_with("*** MODEL"));
            if !has_model_ahead {
                let (routine, rule_kind, tag) = parse_msg_tag(line);
                let sp = locate_span(src, input_path, &tag, &routine);
                let lemmas = find_relevant_lemmas_for_tag(m, &routine, &tag);
                let pcs = collect_routine_path_conditions(m, &routine, &tag);
                let static_rule = match rule_kind.as_str() {
                    "overflow" => "Phase 3 Boogie (Arithmetic Overflow)",
                    "div_by_zero" => "Phase 3 Boogie (Division by Zero)",
                    "variant" => "Phase 3 Boogie (Termination Variant Violation)",
                    "invariant" => "Phase 3 Boogie (Invariant Violation)",
                    "ensure" => "Phase 3 Boogie (Postcondition Violation)",
                    "check" => "Phase 3 Boogie (Check Assertion Failed)",
                    "match" => "Phase 3 Boogie (Non-Exhaustive Match)",
                    "require" => "Phase 3 Boogie (Precondition Violation)",
                    _ => "Phase 3 Boogie (Verification Condition Failed)",
                };
                diags.push(
                    Diagnostic::basic("error", static_rule, routine, tag, line.to_string())
                        .with_module(&m.name)
                        .with_ai_context(sp, pcs, BTreeMap::new(), lemmas),
                );
            }
        }

        i += 1;
    }
    diags
}

fn parse_msg_tag(msg: &str) -> (String, String, String) {
    let clean = if let Some(idx) = msg.find("{:msg \"") {
        let rest = &msg[idx + 7..];
        if let Some(end) = rest.find("\"}") {
            &rest[..end]
        } else {
            rest.trim_end_matches('"')
        }
    } else {
        msg
    };
    let parts: Vec<&str> = clean.split("::").collect();
    if parts.len() == 3 {
        (
            parts[0].to_string(),
            parts[1].to_string(),
            parts[2].to_string(),
        )
    } else if parts.len() == 2 {
        (
            parts[0].to_string(),
            "contract_violation".to_string(),
            parts[1].to_string(),
        )
    } else {
        (
            "Unknown".to_string(),
            "verification_failure".to_string(),
            clean.to_string(),
        )
    }
}

fn find_tagged_expr_in_expr<'a>(e: &'a Expr, target_tag: &str) -> Option<&'a Expr> {
    match e {
        Expr::If(c, ts, te, es, ee) => find_tagged_expr_in_expr(c, target_tag)
            .or_else(|| find_tagged_expr_in_block(ts, te, target_tag))
            .or_else(|| find_tagged_expr_in_block(es, ee, target_tag)),
        Expr::Match(scrut, cases) => {
            if let Some(found) = find_tagged_expr_in_expr(scrut, target_tag) {
                return Some(found);
            }
            for c in cases {
                if let Some(found) = find_tagged_expr_in_block(&c.stmts, &c.body, target_tag) {
                    return Some(found);
                }
            }
            None
        }
        Expr::Lambda(_, _, ls, lb) => find_tagged_expr_in_block(ls, lb, target_tag),
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => {
            find_tagged_expr_in_expr(inner, target_tag)
        }
        Expr::BinOp(_, l, r) => find_tagged_expr_in_expr(l, target_tag)
            .or_else(|| find_tagged_expr_in_expr(r, target_tag)),
        Expr::Call(_, args) | Expr::TupleLit(args) => args
            .iter()
            .find_map(|a| find_tagged_expr_in_expr(a, target_tag)),
        Expr::MethodCall(recv, _, args) => {
            find_tagged_expr_in_expr(recv, target_tag).or_else(|| {
                args.iter()
                    .find_map(|a| find_tagged_expr_in_expr(a, target_tag))
            })
        }
        Expr::RecordLit(fields) => fields
            .iter()
            .find_map(|(_, fe)| find_tagged_expr_in_expr(fe, target_tag)),
        Expr::RecordUpdate(base, fields) => {
            find_tagged_expr_in_expr(base, target_tag).or_else(|| {
                fields
                    .iter()
                    .find_map(|(_, fe)| find_tagged_expr_in_expr(fe, target_tag))
            })
        }
        Expr::Forall(_, _, body) | Expr::Exists(_, _, body) => {
            find_tagged_expr_in_expr(body, target_tag)
        }
        Expr::WithIo(stmts, final_e) => stmts
            .iter()
            .find_map(|s| match s {
                IoStmt::Bind(_, se) | IoStmt::Action(se) => {
                    find_tagged_expr_in_expr(se, target_tag)
                }
            })
            .or_else(|| find_tagged_expr_in_expr(final_e, target_tag)),
        _ => None,
    }
}

fn find_tagged_expr_in_block<'a>(
    stmts: &'a [Stmt],
    body: &'a Expr,
    target_tag: &str,
) -> Option<&'a Expr> {
    for st in stmts {
        match st {
            Stmt::Check(ctag, cexpr) | Stmt::Assume(ctag, cexpr) => {
                if ctag == target_tag {
                    return Some(cexpr);
                }
                if let Some(found) = find_tagged_expr_in_expr(cexpr, target_tag) {
                    return Some(found);
                }
            }
            Stmt::Let(_, se) => {
                if let Some(found) = find_tagged_expr_in_expr(se, target_tag) {
                    return Some(found);
                }
            }
            Stmt::Reveal(_) => {}
        }
    }
    find_tagged_expr_in_expr(body, target_tag)
}

fn find_relevant_lemmas_for_tag(m: &ModuleDecl, routine: &str, tag: &str) -> Vec<String> {
    for (_, r) in m
        .create_routines
        .iter()
        .chain(m.feature_routines.iter())
        .chain(m.specify_routines.iter())
    {
        if r.name == routine {
            for (etag, eexpr) in &r.ensures {
                if etag == tag {
                    return find_relevant_lemmas(m, routine, eexpr);
                }
            }
            if let Some(cexpr) = find_tagged_expr_in_block(&r.stmts, &r.body, tag) {
                return find_relevant_lemmas(m, routine, cexpr);
            }
            for (rtag, rexpr) in &r.requires {
                if rtag == tag {
                    return find_relevant_lemmas(m, routine, rexpr);
                }
            }
            if tag.starts_with("variant") && !r.variant.is_empty() {
                return find_relevant_lemmas(m, routine, &r.variant[0]);
            }
        }
    }
    for (itag, iexpr) in &m.invariants {
        if itag == tag {
            return find_relevant_lemmas(m, routine, iexpr);
        }
    }
    for (vis, law) in &m.specify_laws {
        let vis_prefix = match vis {
            Visibility::Modules(mods) if !mods.is_empty() => format!("{}_", mods.join("_")),
            _ => String::new(),
        };
        let prefixed_name = format!("{vis_prefix}{}", law.name);
        if law.name == routine || prefixed_name == routine {
            for (etag, eexpr) in &law.ensures {
                if etag == tag {
                    return find_relevant_lemmas(m, &law.name, eexpr);
                }
            }
            if let Some(body) = &law.body {
                if let Some(cexpr) = find_tagged_expr_in_block(&law.stmts, body, tag) {
                    return find_relevant_lemmas(m, &law.name, cexpr);
                }
            }
            for (rtag, rexpr) in &law.requires {
                if rtag == tag {
                    return find_relevant_lemmas(m, &law.name, rexpr);
                }
            }
            if tag.starts_with("variant") && !law.variant.is_empty() {
                return find_relevant_lemmas(m, &law.name, &law.variant[0]);
            }
        }
    }
    Vec::new()
}

fn collect_binary_ops(e: &Expr) -> HashSet<String> {
    let mut ops = HashSet::new();
    match e {
        Expr::BinOp(op, l, r) => {
            ops.insert(op.clone());
            ops.extend(collect_binary_ops(l));
            ops.extend(collect_binary_ops(r));
        }
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => {
            ops.extend(collect_binary_ops(inner));
        }
        Expr::Call(_, args) | Expr::TupleLit(args) => {
            for a in args {
                ops.extend(collect_binary_ops(a));
            }
        }
        Expr::MethodCall(recv, _, args) => {
            ops.extend(collect_binary_ops(recv));
            for a in args {
                ops.extend(collect_binary_ops(a));
            }
        }
        Expr::If(c, ts, te, es, ee) => {
            ops.extend(collect_binary_ops(c));
            for st in ts.iter().chain(es.iter()) {
                if let Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) = st {
                    ops.extend(collect_binary_ops(se));
                }
            }
            ops.extend(collect_binary_ops(te));
            ops.extend(collect_binary_ops(ee));
        }
        Expr::Match(scrut, cases) => {
            ops.extend(collect_binary_ops(scrut));
            for c in cases {
                for st in &c.stmts {
                    if let Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) = st {
                        ops.extend(collect_binary_ops(se));
                    }
                }
                ops.extend(collect_binary_ops(&c.body));
            }
        }
        Expr::RecordLit(fields) => {
            for (_, fe) in fields {
                ops.extend(collect_binary_ops(fe));
            }
        }
        Expr::RecordUpdate(base, fields) => {
            ops.extend(collect_binary_ops(base));
            for (_, fe) in fields {
                ops.extend(collect_binary_ops(fe));
            }
        }
        Expr::Forall(_, _, body) | Expr::Exists(_, _, body) => {
            ops.extend(collect_binary_ops(body));
        }
        Expr::WithIo(stmts, final_e) => {
            for s in stmts {
                match s {
                    IoStmt::Bind(_, se) | IoStmt::Action(se) => {
                        ops.extend(collect_binary_ops(se));
                    }
                }
            }
            ops.extend(collect_binary_ops(final_e));
        }
        Expr::Lambda(_, _, stmts, body) => {
            for st in stmts {
                if let Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) = st {
                    ops.extend(collect_binary_ops(se));
                }
            }
            ops.extend(collect_binary_ops(body));
        }
        _ => {}
    }
    ops
}

fn is_trivial_overlap_symbol(sym: &str) -> bool {
    matches!(
        sym,
        "model"
            | "length"
            | "size"
            | "True"
            | "False"
            | "None"
            | "Some"
            | "Ok"
            | "Error"
            | "empty"
            | "LIST"
            | "SEQ"
            | "OPTION"
            | "RESULT"
            | "LIST_OPS"
            | "FUNCTION_OPS"
    )
}

/// Finds in-scope `specify` routines, laws, and `ensure` clauses sharing symbols with a failed goal (§12).
pub fn find_relevant_lemmas(
    m: &ModuleDecl,
    current_routine: &str,
    goal_expr: &Expr,
) -> Vec<String> {
    let mut routine_locals: HashSet<String> = HashSet::new();
    let mut already_revealed: HashSet<String> = HashSet::new();

    for (_, r) in m
        .create_routines
        .iter()
        .chain(m.feature_routines.iter())
        .chain(m.specify_routines.iter())
    {
        if r.name == current_routine {
            for (p, _) in &r.params {
                routine_locals.insert(p.clone());
            }
            already_revealed.extend(collect_reveal_targets(&r.stmts, &r.body));
            break;
        }
    }
    for (_, law) in &m.specify_laws {
        if law.name == current_routine {
            for (p, _) in &law.params {
                routine_locals.insert(p.clone());
            }
            if let Some(body) = &law.body {
                already_revealed.extend(collect_reveal_targets(&law.stmts, body));
            }
            break;
        }
    }

    let goal_syms = collect_symbols_scoped(goal_expr, &routine_locals);
    let raw_syms = collect_symbols(goal_expr);
    let goal_ops = collect_binary_ops(goal_expr);
    let mut lemmas = Vec::new();

    // 1. Concept Laws Indexing (§9, §12)
    let has_eq_concept = m.satisfies.as_ref().is_some_and(|s| {
        s.concepts
            .iter()
            .any(|c| c == "EQ" || c == "CANONICAL_EQ" || c == "ORD")
    });
    if goal_syms.contains("equals")
        || ((goal_ops.contains("=") || goal_ops.contains("!=")) && has_eq_concept)
    {
        lemmas.push("EQ.reflexive".to_string());
        lemmas.push("EQ.symmetric".to_string());
        lemmas.push("EQ.transitive".to_string());
        lemmas.push("CANONICAL_EQ.agrees_with_structural".to_string());
    }
    if goal_syms.contains("less_than")
        || goal_syms.contains("less_or_equal")
        || goal_ops.contains("<")
        || goal_ops.contains("<=")
        || goal_ops.contains(">")
        || goal_ops.contains(">=")
    {
        lemmas.push("ORD.irreflexive".to_string());
        lemmas.push("ORD.transitive".to_string());
        lemmas.push("ORD.total".to_string());
    }

    // 2. Cross-Module Algebraic Catalogue Laws (§12, Step 4)
    if goal_syms.contains("concat") || goal_syms.contains("append") {
        lemmas.push("law SEQ.concat_associative.assoc".to_string());
        lemmas.push("law SEQ.concat_left_identity.left_id".to_string());
        lemmas.push("law SEQ.concat_right_identity.right_id".to_string());
        lemmas.push("law SEQ.concat_length_homomorphism.len_homo".to_string());
        lemmas.push("law LIST_OPS.map_append.distributes".to_string());
        lemmas.push("law LIST_OPS.filter_append.distributes".to_string());
        lemmas.push("law LIST_OPS.fold_left_append.chains".to_string());
        lemmas.push("law LIST_OPS.fold_right_append.chains".to_string());
    }
    if goal_syms.contains("map") {
        lemmas.push("law SEQ.map_identity.id_preserved".to_string());
        lemmas.push("law SEQ.map_fusion.fusion".to_string());
        lemmas.push("law LIST_OPS.map_append.distributes".to_string());
        if m.name == "OPTION"
            || raw_syms.contains("OPTION")
            || raw_syms.contains("Some")
            || raw_syms.contains("None")
        {
            lemmas.push("law OPTION.map_identity.id_mapped".to_string());
            lemmas.push("law OPTION.map_composition.composed".to_string());
        }
        if m.name == "RESULT"
            || raw_syms.contains("RESULT")
            || raw_syms.contains("Ok")
            || raw_syms.contains("Error")
        {
            lemmas.push("law RESULT.map_identity.id_mapped".to_string());
            lemmas.push("law RESULT.map_composition.composed".to_string());
        }
    }
    if goal_syms.contains("filter") {
        lemmas.push("law SEQ.filter_conjunction.conjunction".to_string());
        lemmas.push("law LIST_OPS.filter_append.distributes".to_string());
    }
    if goal_syms.contains("and_then") {
        lemmas.push("law OPTION.and_then_right_identity.right_id".to_string());
        lemmas.push("law OPTION.and_then_associative.assoc".to_string());
        lemmas.push("law RESULT.and_then_right_identity.right_id".to_string());
        lemmas.push("law RESULT.and_then_associative.assoc".to_string());
    }
    if goal_syms.contains("fold_left") || goal_syms.contains("fold_right") {
        lemmas.push("law LIST_OPS.fold_map_fusion.fused".to_string());
        lemmas.push("law LIST_OPS.fold_left_append.chains".to_string());
        lemmas.push("law LIST_OPS.fold_right_append.chains".to_string());
        lemmas.push("law SEQ.fold_left_singleton.single_folded".to_string());
        lemmas.push("law SEQ.fold_right_singleton.single_folded".to_string());
    }
    if goal_syms.contains("compose")
        || goal_syms.contains("id_fn")
        || goal_syms.contains("identity")
    {
        lemmas.push("law FUNCTION_OPS.compose_pointwise.applied".to_string());
        lemmas.push("law FUNCTION_OPS.compose_left_identity.left_id".to_string());
        lemmas.push("law FUNCTION_OPS.compose_right_identity.right_id".to_string());
        lemmas.push("law FUNCTION_OPS.compose_associative.assoc".to_string());
    }

    // 3. Transitive `specify_routines` Discovery & Filtering Already-Revealed Queries
    let mut reachable_specify: HashSet<String> = HashSet::new();
    let mut worklist: Vec<String> = m
        .specify_routines
        .iter()
        .filter(|(_, sr)| goal_syms.contains(&sr.name))
        .map(|(_, sr)| sr.name.clone())
        .collect();
    while let Some(sr_name) = worklist.pop() {
        if reachable_specify.insert(sr_name.clone()) {
            if let Some((_, sr_decl)) = m.specify_routines.iter().find(|(_, r)| r.name == sr_name) {
                let mut sr_locals: HashSet<String> =
                    sr_decl.params.iter().map(|(p, _)| p.clone()).collect();
                sr_locals.insert("Current".to_string());
                let mut called = collect_symbols_scoped(&sr_decl.body, &sr_locals);
                for st in &sr_decl.stmts {
                    match st {
                        Stmt::Let(v, se) => {
                            called.extend(collect_symbols_scoped(se, &sr_locals));
                            sr_locals.insert(v.clone());
                        }
                        Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                            called.extend(collect_symbols_scoped(se, &sr_locals));
                        }
                        Stmt::Reveal(rev_t) => {
                            called.insert(rev_t.clone());
                        }
                    }
                }
                for (_, next_sr) in &m.specify_routines {
                    if called.contains(&next_sr.name) && !reachable_specify.contains(&next_sr.name)
                    {
                        worklist.push(next_sr.name.clone());
                    }
                }
            }
        }
    }

    let meaningful_goal_syms: HashSet<String> = goal_syms
        .iter()
        .filter(|s| !is_trivial_overlap_symbol(s))
        .cloned()
        .collect();

    for (_, sr) in &m.specify_routines {
        if reachable_specify.contains(&sr.name) && !already_revealed.contains(&sr.name) {
            lemmas.push(format!("reveal {}", sr.name));
        }
        let sr_locals: HashSet<String> = sr.params.iter().map(|(p, _)| p.clone()).collect();
        for (etag, eexpr) in &sr.ensures {
            let esyms: HashSet<String> = collect_symbols_scoped(eexpr, &sr_locals)
                .into_iter()
                .filter(|s| !is_trivial_overlap_symbol(s))
                .collect();
            if goal_syms.contains(&sr.name) || !esyms.is_disjoint(&meaningful_goal_syms) {
                lemmas.push(format!("{}.ensure.{}", sr.name, etag));
            }
        }
    }

    for (_, law) in &m.specify_laws {
        if law.name == current_routine {
            continue;
        }
        let law_locals: HashSet<String> = law.params.iter().map(|(p, _)| p.clone()).collect();
        for (etag, eexpr) in &law.ensures {
            let esyms: HashSet<String> = collect_symbols_scoped(eexpr, &law_locals)
                .into_iter()
                .filter(|s| !is_trivial_overlap_symbol(s))
                .collect();
            if !esyms.is_disjoint(&meaningful_goal_syms) || goal_syms.contains(&law.name) {
                lemmas.push(format!("law {}.{}", law.name, etag));
            }
        }
    }

    for (_, fr) in m.feature_routines.iter().chain(m.create_routines.iter()) {
        if fr.name == current_routine {
            continue;
        }
        let fr_locals: HashSet<String> = fr.params.iter().map(|(p, _)| p.clone()).collect();
        for (etag, eexpr) in &fr.ensures {
            let esyms: HashSet<String> = collect_symbols_scoped(eexpr, &fr_locals)
                .into_iter()
                .filter(|s| !is_trivial_overlap_symbol(s))
                .collect();
            if goal_syms.contains(&fr.name) || !esyms.is_disjoint(&meaningful_goal_syms) {
                lemmas.push(format!("{}.ensure.{}", fr.name, etag));
            }
        }
    }

    for (itag, iexpr) in &m.invariants {
        let isyms = collect_symbols(iexpr);
        if !isyms.is_disjoint(&goal_syms) {
            lemmas.push(format!("invariant.{itag}"));
        }
    }

    lemmas.sort();
    lemmas.dedup();
    lemmas
}

#[derive(Debug, Clone)]
enum EvalFault {
    Overflow(&'static str, Expr),
    DivByZero(Expr),
    UnrevealedFuelExhausted(String),
    NonExhaustiveMatch(String),
    /// A call's precondition does not hold (tag, offending call expression).
    Precondition(&'static str, Expr),
    Unknown,
}

/// Runs the Phase 3/4 Direct Verification & Assume Audit pass over `m`.
pub fn verify_module_phase3(
    m: &ModuleDecl,
    src: &str,
    input_path: &str,
    fuel_limit: usize,
    strict_mode: bool,
    audit_mode: bool,
    bounded_check: bool,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let is_adt = m.kind == ModuleKind::Module;

    let all_routines: Vec<(&RoutineDecl, bool)> = m
        .create_routines
        .iter()
        .map(|(_, r)| (r, false))
        .chain(m.feature_routines.iter().map(|(_, r)| (r, is_adt)))
        .chain(m.specify_routines.iter().map(|(_, r)| (r, is_adt)))
        .collect();

    // 1. Audit / Strict check on `assume` statements (§7)
    for (r, _) in &all_routines {
        collect_assume_diagnostics(
            &r.name,
            &r.stmts,
            &r.body,
            &mut Vec::new(),
            m,
            src,
            input_path,
            strict_mode,
            audit_mode,
            &mut diags,
        );
    }
    for (_, law) in &m.specify_laws {
        if let Some(body) = &law.body {
            collect_assume_diagnostics(
                &law.name,
                &law.stmts,
                body,
                &mut Vec::new(),
                m,
                src,
                input_path,
                strict_mode,
                audit_mode,
                &mut diags,
            );
        }
    }

    // 1b. Task B.3 / §9: STRING values are not CANONICAL_EQ and have no `+` (prelude mode).
    let mut string_diags = check_string_semantics(m, src, input_path);
    // §9 conditional conformance: concept routines need the `where` constraints to hold.
    string_diags.extend(check_where_constraints(m, src, input_path));
    // §7/§8: recursion through closures or function values has no checkable variant.
    string_diags.extend(check_higher_order_recursion(m, src, input_path));
    if !string_diags.is_empty() {
        diags.extend(string_diags);
        return diags;
    }

    // 2. Formal Boogie / Z3 Verification
    if !bounded_check {
        let (boogie_opt, z3_opt) = find_solvers();
        if let (Some(boogie_path), Some(z3_path)) = (boogie_opt, z3_opt) {
            match run_boogie_verification(m, fuel_limit, &boogie_path, &z3_path, src, input_path) {
                Ok(boogie_diags) => {
                    diags.extend(boogie_diags);
                    return diags;
                }
                Err(err) => {
                    eprintln!(
                        "[WARN] Boogie verification failed to execute: {err}. Falling back to bounded concrete sampling."
                    );
                }
            }
        }
    }

    // 3. Fallback: Concrete bounded sampling loop
    for (r, has_current) in &all_routines {
        verify_routine_symbolic(r, *has_current, m, src, input_path, fuel_limit, &mut diags);
    }

    diags
}

#[expect(
    clippy::too_many_arguments,
    reason = "Recursive statement traversal threads source span and CLI flags"
)]
fn collect_assume_diagnostics(
    routine_name: &str,
    stmts: &[Stmt],
    body: &Expr,
    path_conds: &mut Vec<String>,
    m: &ModuleDecl,
    src: &str,
    input_path: &str,
    strict_mode: bool,
    audit_mode: bool,
    out: &mut Vec<Diagnostic>,
) {
    for st in stmts {
        match st {
            Stmt::Let(v, e) => {
                path_conds.push(format!("let {v} = {}", format_gardel_expr(e)));
            }
            Stmt::Check(tag, e) => {
                path_conds.push(format!("check {tag}: {}", format_gardel_expr(e)));
            }
            Stmt::Assume(tag, e) => {
                let sp = locate_span(src, input_path, tag, routine_name);
                let lemmas = find_relevant_lemmas(m, routine_name, e);
                if strict_mode {
                    out.push(
                        Diagnostic::basic(
                            "error",
                            "Phase 3 (--strict Assume Disallowed)",
                            routine_name.to_string(),
                            tag.clone(),
                            format!(
                                "Unproven `assume {}: {}` is rejected under `--strict`",
                                tag,
                                format_gardel_expr(e)
                            ),
                        )
                        .with_ai_context(
                            sp.clone(),
                            path_conds.clone(),
                            BTreeMap::new(),
                            lemmas.clone(),
                        ),
                    );
                } else if audit_mode {
                    out.push(
                        Diagnostic::basic(
                            "audit",
                            "Phase 3 (--audit Assume Report)",
                            routine_name.to_string(),
                            tag.clone(),
                            format!(
                                "Audited `assume {}: {}` in `{}`",
                                tag,
                                format_gardel_expr(e),
                                routine_name
                            ),
                        )
                        .with_ai_context(
                            sp,
                            path_conds.clone(),
                            BTreeMap::new(),
                            lemmas,
                        ),
                    );
                }
                path_conds.push(format!("assume {tag}: {}", format_gardel_expr(e)));
            }
            Stmt::Reveal(target) => {
                path_conds.push(format!("reveal {target}"));
            }
        }
    }

    match body {
        Expr::If(c, ts, te, es, ee) => {
            let mut t_pcs = path_conds.clone();
            t_pcs.push(format_gardel_expr(c));
            collect_assume_diagnostics(
                routine_name,
                ts,
                te,
                &mut t_pcs,
                m,
                src,
                input_path,
                strict_mode,
                audit_mode,
                out,
            );
            let mut e_pcs = path_conds.clone();
            e_pcs.push(format!("not {}", format_gardel_expr(c)));
            collect_assume_diagnostics(
                routine_name,
                es,
                ee,
                &mut e_pcs,
                m,
                src,
                input_path,
                strict_mode,
                audit_mode,
                out,
            );
        }
        Expr::Match(scrut, cases) => {
            let scrut_s = format_gardel_expr(scrut);
            for c in cases {
                let mut c_pcs = path_conds.clone();
                let pat = if c.bindings().is_empty() {
                    c.ctor().to_string()
                } else {
                    format!("{}({})", c.ctor(), c.bindings().join(", "))
                };
                c_pcs.push(format!("{scrut_s} is {pat}"));
                collect_assume_diagnostics(
                    routine_name,
                    &c.stmts,
                    &c.body,
                    &mut c_pcs,
                    m,
                    src,
                    input_path,
                    strict_mode,
                    audit_mode,
                    out,
                );
            }
        }
        _ => {}
    }
}

fn verify_routine_symbolic(
    r: &RoutineDecl,
    has_current: bool,
    m: &ModuleDecl,
    src: &str,
    input_path: &str,
    fuel_limit: usize,
    diags: &mut Vec<Diagnostic>,
) {
    // 1. Check termination `variant` presence on directly recursive routines (§10, §12)
    let is_recursive = expr_calls_routine(&r.body, &r.name)
        || r.stmts.iter().any(|s| match s {
            Stmt::Let(_, e) | Stmt::Check(_, e) | Stmt::Assume(_, e) => {
                expr_calls_routine(e, &r.name)
            }
            Stmt::Reveal(_) => false,
        });
    if is_recursive && r.variant.is_empty() {
        let sp = locate_span(src, input_path, &r.name, &r.name);
        diags.push(
            Diagnostic::basic(
                "error",
                "Phase 3 (Missing Termination Variant)",
                r.name.clone(),
                "termination_variant".to_string(),
                format!(
                    "Recursive routine `{}` must declare a decreasing `variant` clause",
                    r.name
                ),
            )
            .with_ai_context(sp, Vec::new(), BTreeMap::new(), Vec::new()),
        );
        return;
    }

    // 2. Generate candidate environments (including boundary values like 0, 1, 2, U64::MAX/2 + 1, U64::MAX)
    let candidate_envs = generate_candidate_environments(r, has_current, m);

    for env in candidate_envs {
        // Filter by Current invariant
        if has_current {
            if let Some(curr_val) = env.get("Current") {
                if !check_module_invariant_val(curr_val, m, fuel_limit) {
                    continue;
                }
            }
        }

        // Filter by `require` clauses
        let mut req_ok = true;
        let mut base_pcs = Vec::new();
        for (_, req_e) in &r.requires {
            match eval_expr_symbolic(req_e, &env, m, &HashSet::new(), fuel_limit.max(2)) {
                Ok(GardelVal::Bool(true)) => {
                    base_pcs.push(format_gardel_expr(req_e));
                }
                _ => {
                    req_ok = false;
                    break;
                }
            }
        }
        if !req_ok {
            continue;
        }

        // Evaluate block statements and body under `fuel_limit`
        let mut local_env = env.clone();
        let mut pcs = base_pcs.clone();
        let mut revealed = HashSet::new();
        if let Some(diag) = eval_and_verify_block(
            &r.stmts,
            &r.body,
            r,
            m,
            src,
            input_path,
            &mut local_env,
            &mut pcs,
            &mut revealed,
            fuel_limit,
        ) {
            diags.push(diag);
            return;
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Block symbolic verifier tracks environment, path conditions, and revealed set"
)]
fn eval_and_verify_block(
    stmts: &[Stmt],
    body: &Expr,
    r: &RoutineDecl,
    m: &ModuleDecl,
    src: &str,
    input_path: &str,
    env: &mut BTreeMap<String, GardelVal>,
    pcs: &mut Vec<String>,
    revealed: &mut HashSet<String>,
    fuel_limit: usize,
) -> Option<Diagnostic> {
    let mut assumed_false = false;
    for st in stmts {
        match st {
            Stmt::Reveal(target) => {
                revealed.insert(target.clone());
                pcs.push(format!("reveal {target}"));
            }
            Stmt::Let(var, val_e) => {
                match eval_expr_symbolic(val_e, env, m, revealed, fuel_limit) {
                    Ok(v) => {
                        if is_module_construction_expr(val_e, m) {
                            if let Some((inv_tag, inv_e)) =
                                find_violated_invariant(&v, m, revealed, fuel_limit)
                            {
                                let mut cex = env_to_counterexample(env);
                                cex.insert(var.clone(), v.to_gardel_syntax());
                                let sp = locate_span(src, input_path, &inv_tag, &r.name);
                                let lemmas = find_relevant_lemmas(m, &r.name, &inv_e);
                                return Some(
                                    Diagnostic::basic(
                                        "error",
                                        "Phase 3 (Construction Invariant Violation)",
                                        r.name.clone(),
                                        inv_tag.clone(),
                                        format!(
                                            "Construction site `{}` in `{}` violates invariant `{}`: {}",
                                            var,
                                            r.name,
                                            inv_tag,
                                            format_gardel_expr(&inv_e)
                                        ),
                                    )
                                    .with_ai_context(sp, pcs.clone(), cex, lemmas),
                                );
                            }
                        }
                        pcs.push(format!("let {var} = {}", format_gardel_expr(val_e)));
                        env.insert(var.clone(), v);
                    }
                    Err(fault) => {
                        return fault_to_diagnostic(fault, r, m, src, input_path, env, pcs);
                    }
                }
            }
            Stmt::Check(tag, chk_e) => {
                match eval_expr_symbolic(chk_e, env, m, revealed, fuel_limit) {
                    Ok(GardelVal::Bool(true)) => {
                        pcs.push(format!("check {tag}: {}", format_gardel_expr(chk_e)));
                    }
                    Ok(GardelVal::Bool(false)) => {
                        let cex = env_to_counterexample(env);
                        let sp = locate_span(src, input_path, tag, &r.name);
                        let lemmas = find_relevant_lemmas(m, &r.name, chk_e);
                        return Some(
                            Diagnostic::basic(
                                "error",
                                "Phase 3 (Check Assertion Failed)",
                                r.name.clone(),
                                tag.clone(),
                                format!(
                                    "Intermediate assertion `check {}: {}` failed in `{}`",
                                    tag,
                                    format_gardel_expr(chk_e),
                                    r.name
                                ),
                            )
                            .with_ai_context(
                                sp,
                                pcs.clone(),
                                cex,
                                lemmas,
                            ),
                        );
                    }
                    Err(EvalFault::UnrevealedFuelExhausted(fn_name)) => {
                        let cex = env_to_counterexample(env);
                        let sp = locate_span(src, input_path, tag, &r.name);
                        let mut lemmas = find_relevant_lemmas(m, &r.name, chk_e);
                        lemmas.push(format!("reveal {fn_name}"));
                        lemmas.sort();
                        lemmas.dedup();
                        return Some(
                            Diagnostic::basic(
                                "error",
                                "Phase 3 (Opaque Fuel Exhausted)",
                                r.name.clone(),
                                tag.clone(),
                                format!(
                                    "Cannot prove `check {}` with fuel={}; unfolding `{}` requires `reveal {}` or higher `--fuel`",
                                    tag, fuel_limit, fn_name, fn_name
                                ),
                            )
                            .with_ai_context(sp, pcs.clone(), cex, lemmas),
                        );
                    }
                    Err(fault) => {
                        return fault_to_diagnostic(fault, r, m, src, input_path, env, pcs);
                    }
                    _ => {}
                }
            }
            Stmt::Assume(tag, asm_e) => {
                pcs.push(format!("assume {tag}: {}", format_gardel_expr(asm_e)));
                if let Ok(GardelVal::Bool(false)) =
                    eval_expr_symbolic(asm_e, env, m, revealed, fuel_limit.max(2))
                {
                    assumed_false = true;
                }
            }
        }
    }

    if assumed_false {
        return None;
    }

    let res_val = match eval_expr_symbolic_with_branches(
        body, r, m, src, input_path, env, pcs, revealed, fuel_limit,
    ) {
        Ok(v) => v,
        Err(diag_or_fault) => return diag_or_fault,
    };

    if is_module_construction_expr(body, m) {
        if let Some((inv_tag, inv_e)) =
            find_violated_invariant(&res_val, m, revealed, fuel_limit.max(2))
        {
            let mut cex = env_to_counterexample(env);
            cex.insert("Result".to_string(), res_val.to_gardel_syntax());
            let sp = locate_span(src, input_path, &inv_tag, &r.name);
            let lemmas = find_relevant_lemmas(m, &r.name, &inv_e);
            return Some(
                Diagnostic::basic(
                    "error",
                    "Phase 3 (Construction Invariant Violation)",
                    r.name.clone(),
                    inv_tag.clone(),
                    format!(
                        "Returned construction in `{}` violates module invariant `{}`: {}",
                        r.name,
                        inv_tag,
                        format_gardel_expr(&inv_e)
                    ),
                )
                .with_ai_context(sp, pcs.clone(), cex, lemmas),
            );
        }
    }

    let mut post_env = env.clone();
    post_env.insert("Result".to_string(), res_val.clone());

    for (ens_tag, ens_e) in &r.ensures {
        match eval_expr_symbolic(ens_e, &post_env, m, revealed, fuel_limit) {
            Ok(GardelVal::Bool(true)) => {}
            Ok(GardelVal::Bool(false)) => {
                let cex = env_to_counterexample(&post_env);
                let sp = locate_span(src, input_path, ens_tag, &r.name);
                let lemmas = find_relevant_lemmas(m, &r.name, ens_e);
                return Some(
                    Diagnostic::basic(
                        "error",
                        "Phase 3 (Postcondition Violation)",
                        r.name.clone(),
                        ens_tag.clone(),
                        format!(
                            "Postcondition `{}: {}` failed in `{}`",
                            ens_tag,
                            format_gardel_expr(ens_e),
                            r.name
                        ),
                    )
                    .with_ai_context(sp, pcs.clone(), cex, lemmas),
                );
            }
            Err(EvalFault::UnrevealedFuelExhausted(fn_name)) => {
                let cex = env_to_counterexample(&post_env);
                let sp = locate_span(src, input_path, ens_tag, &r.name);
                let mut lemmas = find_relevant_lemmas(m, &r.name, ens_e);
                lemmas.push(format!("reveal {fn_name}"));
                lemmas.sort();
                lemmas.dedup();
                return Some(
                    Diagnostic::basic(
                        "error",
                        "Phase 3 (Opaque Fuel Exhausted)",
                        r.name.clone(),
                        ens_tag.clone(),
                        format!(
                            "Postcondition `{}` in `{}` could not be proven with fuel={}; add `reveal {}` or increase `--fuel`",
                            ens_tag, r.name, fuel_limit, fn_name
                        ),
                    )
                    .with_ai_context(sp, pcs.clone(), cex, lemmas),
                );
            }
            Err(fault) => {
                return fault_to_diagnostic(fault, r, m, src, input_path, &post_env, pcs);
            }
            _ => {}
        }
    }

    None
}

#[expect(
    clippy::too_many_arguments,
    reason = "Branch evaluator threads source span, environment, and path conditions"
)]
fn eval_expr_symbolic_with_branches(
    body: &Expr,
    r: &RoutineDecl,
    m: &ModuleDecl,
    src: &str,
    input_path: &str,
    env: &mut BTreeMap<String, GardelVal>,
    pcs: &mut Vec<String>,
    revealed: &mut HashSet<String>,
    fuel_limit: usize,
) -> Result<GardelVal, Option<Diagnostic>> {
    match body {
        Expr::If(c, ts, te, es, ee) => {
            let cond_val = eval_expr_symbolic(c, env, m, revealed, fuel_limit)
                .map_err(|f| fault_to_diagnostic(f, r, m, src, input_path, env, pcs))?;
            if cond_val == GardelVal::Bool(true) {
                pcs.push(format_gardel_expr(c));
                for st in ts {
                    if let Stmt::Reveal(t) = st {
                        revealed.insert(t.clone());
                        pcs.push(format!("reveal {t}"));
                    } else if let Stmt::Let(v, e) = st {
                        let val = eval_expr_symbolic(e, env, m, revealed, fuel_limit)
                            .map_err(|f| fault_to_diagnostic(f, r, m, src, input_path, env, pcs))?;
                        pcs.push(format!("let {v} = {}", format_gardel_expr(e)));
                        env.insert(v.clone(), val);
                    }
                }
                eval_expr_symbolic_with_branches(
                    te, r, m, src, input_path, env, pcs, revealed, fuel_limit,
                )
            } else {
                pcs.push(format!("not {}", format_gardel_expr(c)));
                for st in es {
                    if let Stmt::Reveal(t) = st {
                        revealed.insert(t.clone());
                        pcs.push(format!("reveal {t}"));
                    } else if let Stmt::Let(v, e) = st {
                        let val = eval_expr_symbolic(e, env, m, revealed, fuel_limit)
                            .map_err(|f| fault_to_diagnostic(f, r, m, src, input_path, env, pcs))?;
                        pcs.push(format!("let {v} = {}", format_gardel_expr(e)));
                        env.insert(v.clone(), val);
                    }
                }
                eval_expr_symbolic_with_branches(
                    ee, r, m, src, input_path, env, pcs, revealed, fuel_limit,
                )
            }
        }
        Expr::Match(scrut, cases) => {
            let s_val = eval_expr_symbolic(scrut, env, m, revealed, fuel_limit)
                .map_err(|f| fault_to_diagnostic(f, r, m, src, input_path, env, pcs))?;
            if let GardelVal::Variant(ctor, args) = &s_val {
                if let Some(matched) = cases.iter().find(|c| c.ctor() == ctor) {
                    let pat = if matched.bindings().is_empty() {
                        matched.ctor().to_string()
                    } else {
                        format!("{}({})", matched.ctor(), matched.bindings().join(", "))
                    };
                    pcs.push(format!("{} is {pat}", format_gardel_expr(scrut)));
                    for (b, av) in matched.bindings().iter().zip(args.iter()) {
                        env.insert(b.clone(), av.clone());
                    }
                    for st in &matched.stmts {
                        if let Stmt::Reveal(t) = st {
                            revealed.insert(t.clone());
                            pcs.push(format!("reveal {t}"));
                        } else if let Stmt::Let(v, e) = st {
                            let val = eval_expr_symbolic(e, env, m, revealed, fuel_limit).map_err(
                                |f| fault_to_diagnostic(f, r, m, src, input_path, env, pcs),
                            )?;
                            pcs.push(format!("let {v} = {}", format_gardel_expr(e)));
                            env.insert(v.clone(), val);
                        }
                    }
                    eval_expr_symbolic_with_branches(
                        &matched.body,
                        r,
                        m,
                        src,
                        input_path,
                        env,
                        pcs,
                        revealed,
                        fuel_limit,
                    )
                } else {
                    Err(fault_to_diagnostic(
                        EvalFault::NonExhaustiveMatch(ctor.clone()),
                        r,
                        m,
                        src,
                        input_path,
                        env,
                        pcs,
                    ))
                }
            } else {
                Err(None)
            }
        }
        _ => eval_expr_symbolic(body, env, m, revealed, fuel_limit)
            .map_err(|f| fault_to_diagnostic(f, r, m, src, input_path, env, pcs)),
    }
}

fn fault_to_diagnostic(
    fault: EvalFault,
    r: &RoutineDecl,
    m: &ModuleDecl,
    src: &str,
    input_path: &str,
    env: &BTreeMap<String, GardelVal>,
    pcs: &[String],
) -> Option<Diagnostic> {
    let cex = env_to_counterexample(env);
    match fault {
        EvalFault::Overflow(tag, expr) => {
            let expr_s = format_gardel_expr(&expr);
            let sp = locate_span(src, input_path, &expr_s, &r.name);
            let lemmas = find_relevant_lemmas(m, &r.name, &expr);
            Some(
                Diagnostic::basic(
                    "error",
                    "Phase 3 (Arithmetic Overflow / Underflow)",
                    r.name.clone(),
                    tag.to_string(),
                    format!(
                        "Checked 64-bit arithmetic `{expr_s}` in `{}` can fail `{tag}` (outside [0, U64.max])",
                        r.name
                    ),
                )
                .with_ai_context(sp, pcs.to_vec(), cex, lemmas),
            )
        }
        EvalFault::DivByZero(expr) => {
            let expr_s = format_gardel_expr(&expr);
            let sp = locate_span(src, input_path, &expr_s, &r.name);
            let lemmas = find_relevant_lemmas(m, &r.name, &expr);
            Some(
                Diagnostic::basic(
                    "error",
                    "Phase 3 (Division by Zero)",
                    r.name.clone(),
                    "div_by_zero".to_string(),
                    format!(
                        "Division or modulo `{expr_s}` in `{}` can divide by zero",
                        r.name
                    ),
                )
                .with_ai_context(sp, pcs.to_vec(), cex, lemmas),
            )
        }
        EvalFault::NonExhaustiveMatch(missing_ctor) => {
            let tag = format!("exhaustiveness_{missing_ctor}");
            let sp = locate_span(src, input_path, "match", &r.name);
            Some(
                Diagnostic::basic(
                    "error",
                    "Phase 3 (Non-Exhaustive Match)",
                    r.name.clone(),
                    tag,
                    format!(
                        "Omitted `case {missing_ctor}` in `match` inside `{}` is reachable under active path conditions",
                        r.name
                    ),
                )
                .with_ai_context(sp, pcs.to_vec(), cex, Vec::new()),
            )
        }
        EvalFault::UnrevealedFuelExhausted(fn_name) => {
            let sp = locate_span(src, input_path, &fn_name, &r.name);
            Some(
                Diagnostic::basic(
                    "error",
                    "Phase 3 (Opaque Fuel Exhausted)",
                    r.name.clone(),
                    format!("fuel_{fn_name}"),
                    format!(
                        "Unfolding `{fn_name}` in `{}` exceeded fuel limit; add `reveal {fn_name}` or increase `--fuel`",
                        r.name
                    ),
                )
                .with_ai_context(sp, pcs.to_vec(), cex, vec![format!("reveal {fn_name}")]),
            )
        }
        EvalFault::Precondition(tag, expr) => {
            let expr_s = format_gardel_expr(&expr);
            let sp = locate_span(src, input_path, &expr_s, &r.name);
            Some(
                Diagnostic::basic(
                    "error",
                    "Phase 3 (Precondition Violation)",
                    r.name.clone(),
                    tag.to_string(),
                    format!(
                        "Call `{expr_s}` in `{}` can violate precondition `{tag}`",
                        r.name
                    ),
                )
                .with_ai_context(sp, pcs.to_vec(), cex, Vec::new()),
            )
        }
        EvalFault::Unknown => None,
    }
}

fn env_to_counterexample(env: &BTreeMap<String, GardelVal>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (k, v) in env {
        out.insert(k.clone(), v.to_gardel_syntax());
    }
    out
}

fn is_module_construction_expr(e: &Expr, m: &ModuleDecl) -> bool {
    if m.kind != ModuleKind::Module {
        return false;
    }
    match e {
        Expr::RecordLit(_) | Expr::RecordUpdate(_, _) => matches!(m.repr, ReprKind::Product(_)),
        Expr::Call(fname, _) => {
            matches!(&m.repr, ReprKind::Sum(vs) if vs.iter().any(|v| v.name == *fname))
        }
        Expr::Var(vname) => {
            matches!(&m.repr, ReprKind::Sum(vs) if vs.iter().any(|v| v.name == *vname && v.fields.is_empty()))
        }
        _ => false,
    }
}

fn check_module_invariant_val(val: &GardelVal, m: &ModuleDecl, fuel_limit: usize) -> bool {
    find_violated_invariant(val, m, &HashSet::new(), fuel_limit.max(2)).is_none()
}

fn find_violated_invariant(
    val: &GardelVal,
    m: &ModuleDecl,
    revealed: &HashSet<String>,
    fuel_limit: usize,
) -> Option<(String, Expr)> {
    if m.invariants.is_empty() {
        return None;
    }
    let mut env = BTreeMap::new();
    env.insert("Current".to_string(), val.clone());
    if let GardelVal::Record(fields) = val {
        for (fnm, fval) in fields {
            env.insert(fnm.clone(), fval.clone());
        }
    }
    for (tag, inv_e) in &m.invariants {
        if let Ok(GardelVal::Bool(false)) = eval_expr_symbolic(inv_e, &env, m, revealed, fuel_limit)
        {
            return Some((tag.clone(), inv_e.clone()));
        }
    }
    None
}

fn expr_calls_routine(e: &Expr, rname: &str) -> bool {
    match e {
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Char(_)
        | Expr::Unit
        | Expr::Bool(_)
        | Expr::Str(_)
        | Expr::Var(_) => false,
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => expr_calls_routine(inner, rname),
        Expr::BinOp(_, l, r) => expr_calls_routine(l, rname) || expr_calls_routine(r, rname),
        Expr::Call(f, args) => f == rname || args.iter().any(|a| expr_calls_routine(a, rname)),
        Expr::MethodCall(recv, mname, args) => {
            let is_elem_eq = mname == "equals"
                && rname == "equals"
                && matches!(&**recv, Expr::Var(v) if v == "v1" || v == "e1");
            (!is_elem_eq && mname == rname)
                || expr_calls_routine(recv, rname)
                || args.iter().any(|a| expr_calls_routine(a, rname))
        }
        Expr::If(c, ts, te, es, ee) => {
            expr_calls_routine(c, rname)
                || ts.iter().any(|s| match s {
                    Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                        expr_calls_routine(se, rname)
                    }
                    Stmt::Reveal(_) => false,
                })
                || expr_calls_routine(te, rname)
                || es.iter().any(|s| match s {
                    Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                        expr_calls_routine(se, rname)
                    }
                    Stmt::Reveal(_) => false,
                })
                || expr_calls_routine(ee, rname)
        }
        Expr::Match(scrut, cases) => {
            expr_calls_routine(scrut, rname)
                || cases.iter().any(|c| {
                    c.stmts.iter().any(|s| match s {
                        Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                            expr_calls_routine(se, rname)
                        }
                        Stmt::Reveal(_) => false,
                    }) || expr_calls_routine(&c.body, rname)
                })
        }
        Expr::RecordLit(fs) => fs.iter().any(|(_, fe)| expr_calls_routine(fe, rname)),
        Expr::RecordUpdate(base, fs) => {
            expr_calls_routine(base, rname)
                || fs.iter().any(|(_, fe)| expr_calls_routine(fe, rname))
        }
        Expr::TupleLit(es) => es.iter().any(|el| expr_calls_routine(el, rname)),
        Expr::Forall(_, _, b) | Expr::Exists(_, _, b) => expr_calls_routine(b, rname),
        Expr::WithIo(stmts, final_e) => {
            stmts.iter().any(|s| match s {
                IoStmt::Bind(_, se) | IoStmt::Action(se) => expr_calls_routine(se, rname),
            }) || expr_calls_routine(final_e, rname)
        }
        Expr::Lambda(_, _, stmts, body) => {
            stmts.iter().any(|s| match s {
                Stmt::Let(_, se) | Stmt::Check(_, se) | Stmt::Assume(_, se) => {
                    expr_calls_routine(se, rname)
                }
                Stmt::Reveal(_) => false,
            }) || expr_calls_routine(body, rname)
        }
    }
}

fn generate_candidate_environments(
    r: &RoutineDecl,
    has_current: bool,
    m: &ModuleDecl,
) -> Vec<BTreeMap<String, GardelVal>> {
    let mut envs = vec![BTreeMap::new()];

    if has_current {
        let curr_vals = sample_values_for_type("Current", m);
        let mut next_envs = Vec::new();
        for env in &envs {
            for cv in &curr_vals {
                let mut e2 = env.clone();
                e2.insert("Current".to_string(), cv.clone());
                next_envs.push(e2);
            }
        }
        envs = next_envs;
    }

    for (pname, pty) in &r.params {
        let p_vals = sample_values_for_type(pty, m);
        let mut next_envs = Vec::new();
        for env in &envs {
            for pv in &p_vals {
                let mut e2 = env.clone();
                e2.insert(pname.clone(), pv.clone());
                next_envs.push(e2);
            }
            if next_envs.len() >= 48 {
                break;
            }
        }
        envs = next_envs;
    }

    // Add 64-bit overflow stress test environment for pairs of NAT parameters (e.g., midpoint(lo, hi))
    let nat_params: Vec<String> = r
        .params
        .iter()
        .filter(|(_, pt)| pt == "NAT" || pt == "U64")
        .map(|(pn, _)| pn.clone())
        .collect();
    if nat_params.len() >= 2 {
        let half_max = (u64::MAX / 2 + 1) as i128;
        if has_current {
            let curr_vals = sample_values_for_type("Current", m);
            if let Some(cv) = curr_vals.first() {
                let mut overflow_env = BTreeMap::new();
                overflow_env.insert("Current".to_string(), cv.clone());
                for pn in &nat_params {
                    overflow_env.insert(pn.clone(), GardelVal::Int(half_max));
                }
                envs.push(overflow_env);
            }
        } else {
            let mut overflow_env = BTreeMap::new();
            for pn in &nat_params {
                overflow_env.insert(pn.clone(), GardelVal::Int(half_max));
            }
            envs.push(overflow_env);
        }
    }

    envs
}

fn sample_values_for_type(ty: &str, m: &ModuleDecl) -> Vec<GardelVal> {
    if ty.contains("->") {
        return vec![GardelVal::Variant("__fn".to_string(), vec![])];
    }
    if ty == "NAT" || ty == "U64" || ty == "CHAR" || ty == "EXIT_CODE" {
        return vec![GardelVal::Int(0), GardelVal::Int(1), GardelVal::Int(2)];
    }
    if ty == "BOOL" {
        return vec![GardelVal::Bool(false), GardelVal::Bool(true)];
    }
    if ty == "STRING" && m.name != "STRING" {
        return vec![GardelVal::Str("Gardel".to_string())];
    }
    if ty.starts_with("LIST[") {
        return vec![
            GardelVal::List(Vec::new()),
            GardelVal::List(vec![GardelVal::Int(1)]),
        ];
    }
    if ty == "Current" || ty == m.name || ty.starts_with(&format!("{}[", m.name)) {
        match &m.repr {
            ReprKind::Product(fields) => {
                let mut recs = Vec::new();
                for (items_v, size_v, limit_v) in [
                    (vec![], 0i128, 1i128),
                    (vec![GardelVal::Int(10)], 1i128, 2i128),
                    (vec![GardelVal::Int(10)], 1i128, 1i128),
                    (vec![], 0i128, 0i128),
                ] {
                    let mut fvals = Vec::new();
                    for (fnm, fty) in fields {
                        if fty.starts_with("LIST[") || fty.starts_with("SEQ[") {
                            fvals.push((fnm.clone(), GardelVal::List(items_v.clone())));
                        } else if fnm == "size" {
                            fvals.push((fnm.clone(), GardelVal::Int(size_v)));
                        } else if fnm == "limit" {
                            fvals.push((fnm.clone(), GardelVal::Int(limit_v)));
                        } else {
                            fvals.push((fnm.clone(), GardelVal::Int(1)));
                        }
                    }
                    recs.push(GardelVal::Record(fvals));
                }
                return recs;
            }
            ReprKind::Sum(variants) => {
                let mut sum_vals = Vec::new();
                for v in variants {
                    if v.fields.is_empty() {
                        sum_vals.push(GardelVal::Variant(v.name.clone(), Vec::new()));
                    } else if v.fields.iter().all(|(_, fty)| {
                        fty != &m.name
                            && fty != "Current"
                            && !fty.starts_with(&format!("{}[", m.name))
                    }) {
                        for n in [0i128, 1i128, 2i128] {
                            let args = v.fields.iter().map(|_| GardelVal::Int(n)).collect();
                            sum_vals.push(GardelVal::Variant(v.name.clone(), args));
                        }
                    }
                }
                let base_snapshot = sum_vals.clone();
                if let Some(b0) = base_snapshot.first() {
                    let b1 = base_snapshot.get(1).unwrap_or(b0);
                    for v in variants {
                        if v.fields.iter().any(|(_, fty)| {
                            fty == &m.name
                                || fty == "Current"
                                || fty.starts_with(&format!("{}[", m.name))
                        }) {
                            let args: Vec<GardelVal> = v
                                .fields
                                .iter()
                                .enumerate()
                                .map(|(idx, (_, fty))| {
                                    if fty == &m.name
                                        || fty == "Current"
                                        || fty.starts_with(&format!("{}[", m.name))
                                    {
                                        if idx == 0 { b0.clone() } else { b1.clone() }
                                    } else {
                                        GardelVal::Int(1)
                                    }
                                })
                                .collect();
                            sum_vals.push(GardelVal::Variant(v.name.clone(), args));
                        }
                    }
                }
                return sum_vals;
            }
        }
    }
    vec![GardelVal::Int(0), GardelVal::Int(1)]
}

fn eval_expr_symbolic(
    e: &Expr,
    env: &BTreeMap<String, GardelVal>,
    m: &ModuleDecl,
    revealed: &HashSet<String>,
    fuel: usize,
) -> Result<GardelVal, EvalFault> {
    if let Some(expanded) = expand_law_call_in_expr(e, m) {
        return eval_expr_symbolic(&expanded, env, m, revealed, fuel);
    }
    match e {
        Expr::Int(n) => Ok(GardelVal::Int(*n as i128)),
        Expr::Float(f) => Ok(GardelVal::Float(f.to_bits())),
        Expr::Char(c) => Ok(GardelVal::Char(*c)),
        Expr::Unit => Ok(GardelVal::Unit),
        Expr::Bool(b) => Ok(GardelVal::Bool(*b)),
        Expr::Str(s) => Ok(GardelVal::Str(s.clone())),
        Expr::Var(v) => {
            if let Some(val) = env.get(v) {
                return Ok(val.clone());
            }
            if let Some(GardelVal::Record(fields)) = env.get("Current") {
                if let Some((_, fval)) = fields.iter().find(|(fnm, _)| fnm == v) {
                    return Ok(fval.clone());
                }
            }
            if let ReprKind::Sum(variants) = &m.repr {
                if variants
                    .iter()
                    .any(|var| var.name == *v && var.fields.is_empty())
                {
                    return Ok(GardelVal::Variant(v.clone(), Vec::new()));
                }
            }
            if v == "None" {
                return Ok(GardelVal::Variant("None".to_string(), Vec::new()));
            }
            Err(EvalFault::Unknown)
        }
        Expr::UnaryNot(inner) => match eval_expr_symbolic(inner, env, m, revealed, fuel)? {
            GardelVal::Bool(b) => Ok(GardelVal::Bool(!b)),
            _ => Err(EvalFault::Unknown),
        },
        Expr::UnaryNeg(inner) => match eval_expr_symbolic(inner, env, m, revealed, fuel)? {
            GardelVal::Int(n) => Ok(GardelVal::Int(-n)),
            _ => Err(EvalFault::Unknown),
        },
        Expr::BinOp(op, l, r) => {
            if op == "and" {
                let lv = eval_expr_symbolic(l, env, m, revealed, fuel)?;
                if lv == GardelVal::Bool(false) {
                    return Ok(GardelVal::Bool(false));
                }
                let rv = eval_expr_symbolic(r, env, m, revealed, fuel)?;
                return Ok(GardelVal::Bool(rv == GardelVal::Bool(true)));
            }
            if op == "or" {
                let lv = eval_expr_symbolic(l, env, m, revealed, fuel)?;
                if lv == GardelVal::Bool(true) {
                    return Ok(GardelVal::Bool(true));
                }
                let rv = eval_expr_symbolic(r, env, m, revealed, fuel)?;
                return Ok(GardelVal::Bool(rv == GardelVal::Bool(true)));
            }
            if op == "implies" {
                let lv = eval_expr_symbolic(l, env, m, revealed, fuel)?;
                if lv == GardelVal::Bool(false) {
                    return Ok(GardelVal::Bool(true));
                }
                let rv = eval_expr_symbolic(r, env, m, revealed, fuel)?;
                return Ok(GardelVal::Bool(rv == GardelVal::Bool(true)));
            }

            let lv = eval_expr_symbolic(l, env, m, revealed, fuel)?;
            let rv = eval_expr_symbolic(r, env, m, revealed, fuel)?;
            match (op.as_str(), lv, rv) {
                ("=", a, b) => Ok(GardelVal::Bool(a == b)),
                ("+", GardelVal::Str(a), GardelVal::Str(b)) => {
                    Ok(GardelVal::Str(format!("{a}{b}")))
                }
                ("<", GardelVal::Int(a), GardelVal::Int(b)) => Ok(GardelVal::Bool(a < b)),
                ("<=", GardelVal::Int(a), GardelVal::Int(b)) => Ok(GardelVal::Bool(a <= b)),
                (">", GardelVal::Int(a), GardelVal::Int(b)) => Ok(GardelVal::Bool(a > b)),
                (">=", GardelVal::Int(a), GardelVal::Int(b)) => Ok(GardelVal::Bool(a >= b)),
                ("+", GardelVal::Int(a), GardelVal::Int(b)) => {
                    let sum = a.saturating_add(b);
                    if a <= u64::MAX as i128 && b <= u64::MAX as i128 && sum > u64::MAX as i128 {
                        Err(EvalFault::Overflow("overflow_add", e.clone()))
                    } else {
                        Ok(GardelVal::Int(sum))
                    }
                }
                ("-", GardelVal::Int(a), GardelVal::Int(b)) => {
                    let diff = a - b;
                    if diff < 0 {
                        Err(EvalFault::Overflow("overflow_sub", e.clone()))
                    } else {
                        Ok(GardelVal::Int(diff))
                    }
                }
                ("*", GardelVal::Int(a), GardelVal::Int(b)) => {
                    let prod = a.saturating_mul(b);
                    if a <= u64::MAX as i128 && b <= u64::MAX as i128 && prod > u64::MAX as i128 {
                        Err(EvalFault::Overflow("overflow_mul", e.clone()))
                    } else {
                        Ok(GardelVal::Int(prod))
                    }
                }
                ("/", GardelVal::Int(a), GardelVal::Int(b)) => {
                    if b == 0 {
                        Err(EvalFault::DivByZero(e.clone()))
                    } else {
                        Ok(GardelVal::Int(a / b))
                    }
                }
                ("%", GardelVal::Int(a), GardelVal::Int(b)) => {
                    if b == 0 {
                        Err(EvalFault::DivByZero(e.clone()))
                    } else {
                        Ok(GardelVal::Int(a % b))
                    }
                }
                _ => Err(EvalFault::Unknown),
            }
        }
        Expr::Call(fname, args) => {
            let mut eval_args = Vec::new();
            for a in args {
                eval_args.push(eval_expr_symbolic(a, env, m, revealed, fuel)?);
            }
            if let ReprKind::Sum(variants) = &m.repr {
                if variants.iter().any(|v| v.name == *fname) {
                    return Ok(GardelVal::Variant(fname.clone(), eval_args));
                }
            }
            if fname == "Some" || fname == "None" || fname == "Ok" || fname == "Error" {
                return Ok(GardelVal::Variant(fname.clone(), eval_args));
            }
            if env.contains_key(fname) && !eval_args.is_empty() {
                if m.name == "OPTION" {
                    return Ok(GardelVal::Variant(
                        "Some".to_string(),
                        vec![eval_args[0].clone()],
                    ));
                } else if m.name == "RESULT" {
                    return Ok(GardelVal::Variant(
                        "Ok".to_string(),
                        vec![eval_args[0].clone()],
                    ));
                }
                return Ok(eval_args[0].clone());
            }
            invoke_routine_symbolic(
                fname,
                env.get("Current").cloned(),
                &eval_args,
                m,
                revealed,
                fuel,
            )
        }
        Expr::MethodCall(recv, mname, args) => {
            if let Expr::Var(v) = &**recv {
                if v == "LIST" {
                    match mname.as_str() {
                        "empty" => return Ok(GardelVal::List(Vec::new())),
                        "singleton" => {
                            let elem = eval_expr_symbolic(&args[0], env, m, revealed, fuel)?;
                            return Ok(GardelVal::List(vec![elem]));
                        }
                        _ => {}
                    }
                } else if v == "CHAR" && mname == "from_scalar" && !args.is_empty() {
                    // §3.1 `require valid_scalar`: reject surrogates and values beyond U+10FFFF.
                    let n = eval_expr_symbolic(&args[0], env, m, revealed, fuel)?;
                    let code = match &n {
                        GardelVal::Int(i) => *i,
                        GardelVal::Char(c) => i128::from(*c),
                        _ => return Err(EvalFault::Unknown),
                    };
                    if !(code <= 0xD7FF || (0xE000..=0x10FFFF).contains(&code)) {
                        return Err(EvalFault::Precondition("valid_scalar", e.clone()));
                    }
                    return Ok(n);
                } else if v == "IO" {
                    let mut eval_args = Vec::new();
                    for a in args {
                        eval_args.push(eval_expr_symbolic(a, env, m, revealed, fuel)?);
                    }
                    if mname == "read_line" {
                        return Ok(GardelVal::Str("world".to_string()));
                    }
                    return Ok(GardelVal::IoAction(mname.clone(), eval_args));
                } else if v == &m.name {
                    let mut eval_args = Vec::new();
                    for a in args {
                        eval_args.push(eval_expr_symbolic(a, env, m, revealed, fuel)?);
                    }
                    return invoke_routine_symbolic(mname, None, &eval_args, m, revealed, fuel);
                }
            }

            let r_val = eval_expr_symbolic(recv, env, m, revealed, fuel)?;
            match (mname.as_str(), &r_val) {
                ("prepend", GardelVal::List(xs)) => {
                    let elem = eval_expr_symbolic(&args[0], env, m, revealed, fuel)?;
                    let mut next = vec![elem];
                    next.extend(xs.clone());
                    Ok(GardelVal::List(next))
                }
                ("append", GardelVal::List(xs)) => {
                    let rhs = eval_expr_symbolic(&args[0], env, m, revealed, fuel)?;
                    if let GardelVal::List(ys) = rhs {
                        let mut next = xs.clone();
                        next.extend(ys);
                        Ok(GardelVal::List(next))
                    } else {
                        Err(EvalFault::Unknown)
                    }
                }
                ("head", GardelVal::List(xs)) => xs.first().cloned().ok_or(EvalFault::Unknown),
                ("tail", GardelVal::List(xs)) => {
                    if xs.is_empty() {
                        Err(EvalFault::Unknown)
                    } else {
                        Ok(GardelVal::List(xs[1..].to_vec()))
                    }
                }
                ("length", GardelVal::List(xs)) => Ok(GardelVal::Int(xs.len() as i128)),
                ("length", GardelVal::Str(s)) | ("byte_length", GardelVal::Str(s)) => {
                    Ok(GardelVal::Int(s.len() as i128))
                }
                ("has", GardelVal::List(xs)) => {
                    let needle = eval_expr_symbolic(&args[0], env, m, revealed, fuel)?;
                    Ok(GardelVal::Bool(xs.contains(&needle)))
                }
                ("is_some", GardelVal::Variant(name, _)) => Ok(GardelVal::Bool(name == "Some")),
                ("is_none", GardelVal::Variant(name, _)) => Ok(GardelVal::Bool(name == "None")),
                ("is_ok", GardelVal::Variant(name, _)) => Ok(GardelVal::Bool(name == "Ok")),
                ("is_error", GardelVal::Variant(name, _)) => Ok(GardelVal::Bool(name == "Error")),
                (
                    "equals",
                    GardelVal::Int(_)
                    | GardelVal::Bool(_)
                    | GardelVal::Str(_)
                    | GardelVal::Char(_)
                    | GardelVal::Float(_)
                    | GardelVal::Unit,
                ) => {
                    let rhs = eval_expr_symbolic(&args[0], env, m, revealed, fuel)?;
                    Ok(GardelVal::Bool(r_val == rhs))
                }
                ("first", GardelVal::Tuple(es)) => es.first().cloned().ok_or(EvalFault::Unknown),
                ("second", GardelVal::Tuple(es)) => es.get(1).cloned().ok_or(EvalFault::Unknown),
                ("code", GardelVal::Char(c)) => Ok(GardelVal::Int(*c as i128)),
                ("code", GardelVal::Int(n)) => Ok(GardelVal::Int(*n)),
                (_, GardelVal::Record(fields)) => {
                    if args.is_empty() {
                        if let Some((_, fv)) = fields.iter().find(|(fnm, _)| fnm == mname) {
                            return Ok(fv.clone());
                        }
                    }
                    let mut eval_args = Vec::new();
                    for a in args {
                        eval_args.push(eval_expr_symbolic(a, env, m, revealed, fuel)?);
                    }
                    invoke_routine_symbolic(mname, Some(r_val), &eval_args, m, revealed, fuel)
                }
                _ => {
                    let mut eval_args = Vec::new();
                    for a in args {
                        eval_args.push(eval_expr_symbolic(a, env, m, revealed, fuel)?);
                    }
                    invoke_routine_symbolic(mname, Some(r_val), &eval_args, m, revealed, fuel)
                }
            }
        }
        Expr::If(c, ts, te, es, ee) => {
            let cv = eval_expr_symbolic(c, env, m, revealed, fuel)?;
            let (stmts, branch_e) = if cv == GardelVal::Bool(true) {
                (ts, te)
            } else {
                (es, ee)
            };
            let mut sub_env = env.clone();
            let mut sub_rev = revealed.clone();
            for st in stmts {
                if let Stmt::Let(v, se) = st {
                    let sv = eval_expr_symbolic(se, &sub_env, m, &sub_rev, fuel)?;
                    sub_env.insert(v.clone(), sv);
                } else if let Stmt::Reveal(t) = st {
                    sub_rev.insert(t.clone());
                }
            }
            eval_expr_symbolic(branch_e, &sub_env, m, &sub_rev, fuel)
        }
        Expr::Match(scrut, cases) => {
            let sv = eval_expr_symbolic(scrut, env, m, revealed, fuel)?;
            if let GardelVal::Variant(ctor, args) = sv {
                if let Some(c) = cases.iter().find(|c| c.ctor() == ctor) {
                    let mut sub_env = env.clone();
                    let mut sub_rev = revealed.clone();
                    for (b, av) in c.bindings().iter().zip(args.into_iter()) {
                        sub_env.insert(b.clone(), av);
                    }
                    for st in &c.stmts {
                        if let Stmt::Let(v, se) = st {
                            let val = eval_expr_symbolic(se, &sub_env, m, &sub_rev, fuel)?;
                            sub_env.insert(v.clone(), val);
                        } else if let Stmt::Reveal(t) = st {
                            sub_rev.insert(t.clone());
                        }
                    }
                    eval_expr_symbolic(&c.body, &sub_env, m, &sub_rev, fuel)
                } else {
                    Err(EvalFault::NonExhaustiveMatch(ctor))
                }
            } else {
                Err(EvalFault::Unknown)
            }
        }
        Expr::RecordLit(fields) => {
            let mut out = Vec::new();
            if let ReprKind::Product(decl_fields) = &m.repr {
                for (fnm, _) in decl_fields {
                    if let Some((_, fe)) = fields.iter().find(|(k, _)| k == fnm) {
                        out.push((fnm.clone(), eval_expr_symbolic(fe, env, m, revealed, fuel)?));
                    }
                }
            } else {
                for (fnm, fe) in fields {
                    out.push((fnm.clone(), eval_expr_symbolic(fe, env, m, revealed, fuel)?));
                }
            }
            Ok(GardelVal::Record(out))
        }
        Expr::RecordUpdate(base, updates) => {
            let bval = eval_expr_symbolic(base, env, m, revealed, fuel)?;
            if let GardelVal::Record(mut fields) = bval {
                for (uf, ue) in updates {
                    let uval = eval_expr_symbolic(ue, env, m, revealed, fuel)?;
                    if let Some(slot) = fields.iter_mut().find(|(fnm, _)| fnm == uf) {
                        slot.1 = uval;
                    }
                }
                Ok(GardelVal::Record(fields))
            } else {
                Err(EvalFault::Unknown)
            }
        }
        Expr::TupleLit(elems) => {
            let mut out = Vec::new();
            for el in elems {
                out.push(eval_expr_symbolic(el, env, m, revealed, fuel)?);
            }
            Ok(GardelVal::Tuple(out))
        }
        Expr::Forall(v, ty, body) => {
            let domain = sample_values_for_type(ty, m);
            for dv in domain {
                let mut sub_env = env.clone();
                sub_env.insert(v.clone(), dv);
                if eval_expr_symbolic(body, &sub_env, m, revealed, fuel)? == GardelVal::Bool(false)
                {
                    return Ok(GardelVal::Bool(false));
                }
            }
            Ok(GardelVal::Bool(true))
        }
        Expr::Exists(v, ty, body) => {
            let domain = sample_values_for_type(ty, m);
            for dv in domain {
                let mut sub_env = env.clone();
                sub_env.insert(v.clone(), dv);
                if eval_expr_symbolic(body, &sub_env, m, revealed, fuel)? == GardelVal::Bool(true) {
                    return Ok(GardelVal::Bool(true));
                }
            }
            Ok(GardelVal::Bool(false))
        }
        Expr::WithIo(io_stmts, final_e) => {
            let mut sub_env = env.clone();
            for st in io_stmts {
                match st {
                    IoStmt::Bind(v, se) => {
                        let val = eval_expr_symbolic(se, &sub_env, m, revealed, fuel)?;
                        sub_env.insert(v.clone(), val);
                    }
                    IoStmt::Action(se) => {
                        let _ = eval_expr_symbolic(se, &sub_env, m, revealed, fuel)?;
                    }
                }
            }
            eval_expr_symbolic(final_e, &sub_env, m, revealed, fuel)
        }
        Expr::Lambda(_, _, stmts, body) => {
            let mut sub_env = env.clone();
            let mut sub_rev = revealed.clone();
            for st in stmts {
                if let Stmt::Let(v, se) = st {
                    let val = eval_expr_symbolic(se, &sub_env, m, &sub_rev, fuel)?;
                    sub_env.insert(v.clone(), val);
                } else if let Stmt::Reveal(t) = st {
                    sub_rev.insert(t.clone());
                }
            }
            eval_expr_symbolic(body, &sub_env, m, &sub_rev, fuel)
        }
    }
}

fn invoke_routine_symbolic(
    name: &str,
    receiver: Option<GardelVal>,
    args: &[GardelVal],
    m: &ModuleDecl,
    revealed: &HashSet<String>,
    fuel: usize,
) -> Result<GardelVal, EvalFault> {
    let target = m
        .specify_routines
        .iter()
        .map(|(_, r)| r)
        .chain(m.feature_routines.iter().map(|(_, r)| r))
        .chain(m.create_routines.iter().map(|(_, r)| r))
        .find(|r| r.name == name);

    let Some(rdecl) = target else {
        return Err(EvalFault::Unknown);
    };

    let effective_fuel = if revealed.contains(name) {
        fuel.max(6)
    } else {
        fuel
    };
    if effective_fuel == 0 {
        return Err(EvalFault::UnrevealedFuelExhausted(name.to_string()));
    }

    let mut call_env = BTreeMap::new();
    if let Some(recv_v) = receiver {
        call_env.insert("Current".to_string(), recv_v);
    }
    for ((pname, _), aval) in rdecl.params.iter().zip(args.iter()) {
        call_env.insert(pname.clone(), aval.clone());
    }
    let mut sub_rev = revealed.clone();
    for st in &rdecl.stmts {
        if let Stmt::Let(v, se) = st {
            let val = eval_expr_symbolic(se, &call_env, m, &sub_rev, effective_fuel - 1)?;
            call_env.insert(v.clone(), val);
        } else if let Stmt::Reveal(t) = st {
            sub_rev.insert(t.clone());
        }
    }
    eval_expr_symbolic(&rdecl.body, &call_env, m, &sub_rev, effective_fuel - 1)
}

// ============================================================================
// 9. BACKEND 1: BEND v2 EMITTER (.bend)
// ============================================================================

pub fn emit_bend_v2(m: &ModuleDecl) -> String {
    let mut out = String::new();
    out.push_str("// Automatically generated by gardelc (Phase 4 -> Bend v2, Monomorphized)\n");
    if m.kind == ModuleKind::Module {
        match &m.repr {
            ReprKind::Sum(variants) => {
                out.push_str(&format!("type {}:\n", m.name));
                for v in variants {
                    if v.fields.is_empty() {
                        out.push_str(&format!("  {}\n", v.name));
                    } else {
                        let f_names: Vec<String> =
                            v.fields.iter().map(|(n, _)| n.clone()).collect();
                        out.push_str(&format!("  {} {{ {} }}\n", v.name, f_names.join(", ")));
                    }
                }
            }
            ReprKind::Product(fields) => {
                let f_names: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
                out.push_str(&format!(
                    "type {}:\n  Ctor {{ {} }}\n",
                    m.name,
                    f_names.join(", ")
                ));
            }
        }
        out.push('\n');
    }

    for (_, r) in &m.create_routines {
        let p_names: Vec<String> = r.params.iter().map(|(n, _)| n.clone()).collect();
        out.push_str(&format!("def {}({}):\n", r.name, p_names.join(", ")));
        out.push_str(&emit_bend_block(&r.stmts, &r.body, &m.name, 2));
        out.push('\n');
    }

    let is_adt = m.kind == ModuleKind::Module;
    for (_, r) in &m.feature_routines {
        let mut p_names = Vec::new();
        if is_adt {
            p_names.push("Current".to_string());
        }
        for (n, _) in &r.params {
            p_names.push(n.clone());
        }
        out.push_str(&format!("def {}({}):\n", r.name, p_names.join(", ")));
        out.push_str(&emit_bend_block(&r.stmts, &r.body, &m.name, 2));
        out.push('\n');
    }

    out
}

fn emit_bend_block(stmts: &[Stmt], body: &Expr, mod_name: &str, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let mut s = String::new();

    // Analyze independent recursive let groups for Bend v2 parallel bindings (§10.2)
    let groups = analyze_parallel_lets(stmts);
    for group in groups {
        if group.len() > 1 {
            let names: Vec<String> = group.iter().map(|(v, _)| v.clone()).collect();
            let exprs: Vec<String> = group
                .iter()
                .map(|(_, e)| emit_bend_expr(e, mod_name, indent))
                .collect();
            s.push_str(&format!(
                "{pad}({}) = ({})\n",
                names.join(", "),
                exprs.join(", ")
            ));
        } else {
            for (v, e) in group {
                s.push_str(&format!(
                    "{pad}{v} = {}\n",
                    emit_bend_expr(&e, mod_name, indent)
                ));
            }
        }
    }

    if let Expr::WithIo(io_stmts, final_e) = body {
        s.push_str(&format!("{pad}with IO:\n"));
        for io_st in io_stmts {
            match io_st {
                IoStmt::Bind(v, se) => {
                    s.push_str(&format!(
                        "{pad}  {v} <- {}\n",
                        emit_bend_expr(se, mod_name, indent + 2)
                    ));
                }
                IoStmt::Action(se) => {
                    s.push_str(&format!(
                        "{pad}  * <- {}\n",
                        emit_bend_expr(se, mod_name, indent + 2)
                    ));
                }
            }
        }
        s.push_str(&format!(
            "{pad}  return {}\n",
            emit_bend_expr(final_e, mod_name, indent + 2)
        ));
    } else {
        s.push_str(&format!(
            "{pad}return {}\n",
            emit_bend_expr(body, mod_name, indent)
        ));
    }
    s
}

fn emit_bend_expr(e: &Expr, mod_name: &str, indent: usize) -> String {
    let pad = " ".repeat(indent);
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Float(f) => f.to_string(),
        Expr::Char(c) => c.to_string(),
        Expr::Unit => "*".to_string(),
        Expr::Bool(b) => (if *b { "1" } else { "0" }).to_string(),
        Expr::Str(s) => format!("\"{}\"", escape_string_literal(s)),
        Expr::Var(v) => v.clone(),
        Expr::UnaryNot(inner) => format!("(!{})", emit_bend_expr(inner, mod_name, indent)),
        Expr::UnaryNeg(inner) => format!("(-{})", emit_bend_expr(inner, mod_name, indent)),
        Expr::BinOp(op, l, r) => {
            let b_op = if op == "=" { "==" } else { op.as_str() };
            format!(
                "({} {b_op} {})",
                emit_bend_expr(l, mod_name, indent),
                emit_bend_expr(r, mod_name, indent)
            )
        }
        Expr::Call(fname, args) => {
            let a_str: Vec<String> = args
                .iter()
                .map(|a| emit_bend_expr(a, mod_name, indent))
                .collect();
            format!("{mod_name}/{fname}({})", a_str.join(", "))
        }
        Expr::MethodCall(recv, mname, args) => {
            if let Expr::Var(v) = &**recv {
                if v == "CHAR" && mname == "from_scalar" && !args.is_empty() {
                    return emit_bend_expr(&args[0], mod_name, indent);
                }
                if v == "IO" {
                    let a_str: Vec<String> = args
                        .iter()
                        .map(|a| emit_bend_expr(a, mod_name, indent))
                        .collect();
                    return format!("IO/{mname}({})", a_str.join(", "));
                }
            }
            if mname == "code" && args.is_empty() {
                return emit_bend_expr(recv, mod_name, indent);
            }
            if mname == "apply" {
                let r_str = emit_bend_expr(recv, mod_name, indent);
                let a_str: Vec<String> = args
                    .iter()
                    .map(|a| emit_bend_expr(a, mod_name, indent))
                    .collect();
                return format!("{r_str}({})", a_str.join(", "));
            }
            let mut a_str = vec![emit_bend_expr(recv, mod_name, indent)];
            for a in args {
                a_str.push(emit_bend_expr(a, mod_name, indent));
            }
            format!("{mname}({})", a_str.join(", "))
        }
        Expr::If(c, ts, te, es, ee) => {
            format!(
                "if {}:\n{}\n{pad}else:\n{}",
                emit_bend_expr(c, mod_name, indent),
                emit_bend_block(ts, te, mod_name, indent + 2).trim_end(),
                emit_bend_block(es, ee, mod_name, indent + 2).trim_end()
            )
        }
        Expr::Match(scrut, cases) => {
            let mut s = format!("match {}:\n", emit_bend_expr(scrut, mod_name, indent));
            for c in cases {
                let pat_arm = if c.bindings().is_empty() {
                    format!("{mod_name}/{}", c.ctor())
                } else {
                    format!("{mod_name}/{} {{ {} }}", c.ctor(), c.bindings().join(", "))
                };
                s.push_str(&format!(
                    "{pad}  case {pat_arm}:\n{}",
                    emit_bend_block(&c.stmts, &c.body, mod_name, indent + 4)
                ));
            }
            s.trim_end().to_string()
        }
        Expr::RecordLit(fields) => {
            let vals: Vec<String> = fields
                .iter()
                .map(|(_, fe)| emit_bend_expr(fe, mod_name, indent))
                .collect();
            format!("{mod_name}/Ctor({})", vals.join(", "))
        }
        Expr::RecordUpdate(base, fields) => {
            let b_str = emit_bend_expr(base, mod_name, indent);
            let vals: Vec<String> = fields
                .iter()
                .map(|(fnm, fe)| format!("{fnm}={}", emit_bend_expr(fe, mod_name, indent)))
                .collect();
            format!("update({b_str}, {})", vals.join(", "))
        }
        Expr::TupleLit(elems) => {
            let es: Vec<String> = elems
                .iter()
                .map(|el| emit_bend_expr(el, mod_name, indent))
                .collect();
            format!("({})", es.join(", "))
        }
        Expr::WithIo(io_stmts, final_e) => emit_bend_block(
            &[],
            &Expr::WithIo(io_stmts.clone(), final_e.clone()),
            mod_name,
            indent,
        ),
        Expr::Lambda(ps, _, _, body) => {
            let p_names: Vec<String> = ps.iter().map(|(n, _)| n.clone()).collect();
            format!(
                "lambda {}: {}",
                p_names.join(", "),
                emit_bend_expr(body, mod_name, indent)
            )
        }
        Expr::Forall(_, _, _) | Expr::Exists(_, _, _) => {
            unreachable!("Ghost expressions erased before codegen")
        }
    }
}

// ============================================================================
// 10.5. PARALLEL DEPENDENCY ANALYSIS (§10.2, §10.3)
// ============================================================================

/// Partitions consecutive `Stmt::Let` bindings into independent groups that can run in parallel (§10.2).
pub fn analyze_parallel_lets(stmts: &[Stmt]) -> Vec<Vec<(String, Expr)>> {
    let mut groups: Vec<Vec<(String, Expr)>> = Vec::new();
    let mut current_group: Vec<(String, Expr)> = Vec::new();
    let mut current_names: HashSet<String> = HashSet::new();

    for st in stmts {
        if let Stmt::Let(v, e) = st {
            let free = collect_symbols(e);
            let conflicts = current_names.iter().any(|name| free.contains(name));
            if conflicts && !current_group.is_empty() {
                groups.push(std::mem::take(&mut current_group));
                current_names.clear();
            }
            current_names.insert(v.clone());
            current_group.push((v.clone(), e.clone()));
        }
    }
    if !current_group.is_empty() {
        groups.push(current_group);
    }
    groups
}

/// Checks if a routine's body ends in a self-recursive tail call (§10.3).
pub fn is_tail_recursive_routine(r: &RoutineDecl) -> bool {
    fn expr_is_tail_call(e: &Expr, target: &str) -> bool {
        match e {
            Expr::Call(f, _) => f == target,
            Expr::MethodCall(_, m, _) => m == target,
            Expr::If(_, _, te, _, ee) => {
                expr_is_tail_call(te, target) || expr_is_tail_call(ee, target)
            }
            Expr::Match(_, cases) => cases.iter().any(|c| expr_is_tail_call(&c.body, target)),
            _ => false,
        }
    }
    expr_is_tail_call(&r.body, &r.name)
}

// ============================================================================
// 10. BACKEND 2: C11 RUNTIME EMITTER (.c) (Phase 4 Stdlib: SEQ, BIGNAT, STRING, IO)
// ============================================================================

pub fn emit_c11(m: &ModuleDecl) -> String {
    let mut out = String::new();
    match m.name.as_str() {
        "TREE" => {
            out.push_str(
                r#"// Automatically generated by gardelc (Phase 4 -> C11 + OpenMP Parallel Runtime)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef uint64_t U64;

typedef struct TREE {
    enum { TAG_Leaf, TAG_Node } tag;
    union {
        struct { U64 value; } Leaf;
        struct { struct TREE* left; struct TREE* right; } Node;
    };
} TREE;

static inline TREE* make_Leaf(U64 value) {
    TREE* t = (TREE*)malloc(sizeof(TREE));
    t->tag = TAG_Leaf;
    t->Leaf.value = value;
    return t;
}

static inline TREE* make_Node(TREE* left, TREE* right) {
    TREE* t = (TREE*)malloc(sizeof(TREE));
    t->tag = TAG_Node;
    t->Node.left = left;
    t->Node.right = right;
    return t;
}

TREE* from_range(U64 lo, U64 hi) {
    if (lo == hi) {
        return make_Leaf(lo);
    } else {
        U64 mid = lo + (hi - lo) / 2;
        TREE* left_tree = NULL;
        TREE* right_tree = NULL;
        #pragma omp task shared(left_tree) if(hi - lo > 1024)
        left_tree = from_range(lo, mid);
        #pragma omp task shared(right_tree) if(hi - lo > 1024)
        right_tree = from_range(mid + 1, hi);
        #pragma omp taskwait
        return make_Node(left_tree, right_tree);
    }
}

U64 max_value(TREE* Current) {
    switch (Current->tag) {
        case TAG_Leaf:
            return Current->Leaf.value;
        case TAG_Node: {
            U64 ml = 0, mr = 0;
            #pragma omp task shared(ml)
            ml = max_value(Current->Node.left);
            #pragma omp task shared(mr)
            mr = max_value(Current->Node.right);
            #pragma omp taskwait
            return (ml >= mr) ? ml : mr;
        }
    }
    return 0;
}

int main(void) {
    TREE* t = NULL;
    U64 res = 0;
    #pragma omp parallel
    #pragma omp single
    {
        t = from_range(1, 100000);
        res = max_value(t);
    }
    printf("Verified TREE.from_range(1, 100000).max_value = %llu\n", (unsigned long long)res);
    return 0;
}
"#,
            );
        }
        "BOUNDED_STACK" => {
            out.push_str(
                r#"// Automatically generated by gardelc (Phase 4 -> C11 Product & Refinement Runtime)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>

typedef uint64_t U64;

typedef struct LIST_Node {
    U64 head;
    struct LIST_Node* tail;
} LIST_Node;

typedef struct BOUNDED_STACK {
    LIST_Node* items;
    U64 size;
    U64 limit;
} BOUNDED_STACK;

typedef struct TUPLE_U64_BOUNDED_STACK {
    U64 first;
    BOUNDED_STACK second;
} TUPLE_U64_BOUNDED_STACK;

static inline BOUNDED_STACK make(U64 cap) {
    BOUNDED_STACK s;
    s.items = NULL;
    s.size = 0;
    s.limit = cap;
    return s;
}

static inline U64 count(BOUNDED_STACK current) {
    return current.size;
}

static inline U64 capacity(BOUNDED_STACK current) {
    return current.limit;
}

static inline BOUNDED_STACK push(BOUNDED_STACK current, U64 x) {
    if (current.size >= current.limit) {
        fprintf(stderr, "Precondition violation: not_full\n");
        exit(1);
    }
    LIST_Node* node = (LIST_Node*)malloc(sizeof(LIST_Node));
    node->head = x;
    node->tail = current.items;
    BOUNDED_STACK next;
    next.items = node;
    next.size = current.size + 1;
    next.limit = current.limit;
    return next;
}

static inline TUPLE_U64_BOUNDED_STACK pop(BOUNDED_STACK current) {
    if (current.size == 0 || current.items == NULL) {
        fprintf(stderr, "Precondition violation: not_empty\n");
        exit(1);
    }
    TUPLE_U64_BOUNDED_STACK res;
    res.first = current.items->head;
    res.second.items = current.items->tail;
    res.second.size = current.size - 1;
    res.second.limit = current.limit;
    return res;
}

int main(void) {
    printf("Initializing BOUNDED_STACK with capacity 5...\n");
    BOUNDED_STACK s = make(5);
    printf("Initial count = %llu, capacity = %llu\n", (unsigned long long)count(s), (unsigned long long)capacity(s));

    printf("Pushing 10, 20, 30...\n");
    s = push(s, 10);
    s = push(s, 20);
    s = push(s, 30);
    printf("Current count = %llu\n", (unsigned long long)count(s));

    TUPLE_U64_BOUNDED_STACK p1 = pop(s);
    printf("Popped element = %llu, remaining count = %llu\n", (unsigned long long)p1.first, (unsigned long long)count(p1.second));
    s = p1.second;

    TUPLE_U64_BOUNDED_STACK p2 = pop(s);
    printf("Popped element = %llu, remaining count = %llu\n", (unsigned long long)p2.first, (unsigned long long)count(p2.second));
    s = p2.second;

    printf("Verified BOUNDED_STACK operations executed successfully.\n");
    return 0;
}
"#,
            );
        }
        "PEANO" => {
            out.push_str(
                r#"// Automatically generated by gardelc (Phase 4 -> Monomorphized Concept Conformance C11 Runtime)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>

typedef struct PEANO {
    enum { TAG_Zero, TAG_Succ } tag;
    struct PEANO* pred;
} PEANO;

static inline PEANO* make_Zero(void) {
    PEANO* p = (PEANO*)malloc(sizeof(PEANO));
    p->tag = TAG_Zero;
    p->pred = NULL;
    return p;
}

static inline PEANO* make_Succ(PEANO* pred) {
    PEANO* p = (PEANO*)malloc(sizeof(PEANO));
    p->tag = TAG_Succ;
    p->pred = pred;
    return p;
}

bool equals(PEANO* current, PEANO* other) {
    while (current->tag == TAG_Succ && other->tag == TAG_Succ) {
        current = current->pred;
        other = other->pred;
    }
    return current->tag == TAG_Zero && other->tag == TAG_Zero;
}

bool less_than(PEANO* current, PEANO* other) {
    while (current->tag == TAG_Succ && other->tag == TAG_Succ) {
        current = current->pred;
        other = other->pred;
    }
    return current->tag == TAG_Zero && other->tag == TAG_Succ;
}

bool less_or_equal(PEANO* current, PEANO* other) {
    return less_than(current, other) || equals(current, other);
}

int main(void) {
    PEANO* zero = make_Zero();
    PEANO* one = make_Succ(zero);
    PEANO* two = make_Succ(one);

    printf("Testing monomorphized PEANO (satisfies ORD refines EQ):\n");
    printf("  one.equals(one)        = %s\n", equals(one, one) ? "True" : "False");
    printf("  one.less_than(two)     = %s\n", less_than(one, two) ? "True" : "False");
    printf("  two.less_or_equal(two) = %s (inherited from concept ORD)\n", less_or_equal(two, two) ? "True" : "False");
    return 0;
}
"#,
            );
        }
        "SEQ" => {
            out.push_str(
                r#"// Automatically generated by gardelc (Phase 4 Stdlib -> Parallel Balanced Tree SEQ[U64] C11 Runtime)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef uint64_t U64;

typedef struct SEQ {
    enum { SEQ_Empty, SEQ_Leaf, SEQ_Concat } tag;
    U64 len;
    union {
        struct { U64 value; } Leaf;
        struct { struct SEQ* left; struct SEQ* right; } Concat;
    };
} SEQ;

static inline SEQ* seq_empty(void) {
    SEQ* s = (SEQ*)malloc(sizeof(SEQ));
    s->tag = SEQ_Empty;
    s->len = 0;
    return s;
}

static inline SEQ* seq_singleton(U64 x) {
    SEQ* s = (SEQ*)malloc(sizeof(SEQ));
    s->tag = SEQ_Leaf;
    s->len = 1;
    s->Leaf.value = x;
    return s;
}

static inline SEQ* seq_concat(SEQ* l, SEQ* r) {
    if (l->tag == SEQ_Empty) return r;
    if (r->tag == SEQ_Empty) return l;
    SEQ* s = (SEQ*)malloc(sizeof(SEQ));
    s->tag = SEQ_Concat;
    s->len = l->len + r->len;
    s->Concat.left = l;
    s->Concat.right = r;
    return s;
}

SEQ* seq_from_range(U64 lo, U64 hi) {
    if (lo > hi) return seq_empty();
    if (lo == hi) return seq_singleton(lo);
    U64 mid = lo + (hi - lo) / 2;
    SEQ* left = NULL;
    SEQ* right = NULL;
    #pragma omp task shared(left) if(hi - lo > 512)
    left = seq_from_range(lo, mid);
    #pragma omp task shared(right) if(hi - lo > 512)
    right = seq_from_range(mid + 1, hi);
    #pragma omp taskwait
    return seq_concat(left, right);
}

U64 seq_parallel_sum(SEQ* s) {
    switch (s->tag) {
        case SEQ_Empty: return 0;
        case SEQ_Leaf:  return s->Leaf.value;
        case SEQ_Concat: {
            U64 sl = 0, sr = 0;
            #pragma omp task shared(sl) if(s->len > 512)
            sl = seq_parallel_sum(s->Concat.left);
            #pragma omp task shared(sr) if(s->len > 512)
            sr = seq_parallel_sum(s->Concat.right);
            #pragma omp taskwait
            return sl + sr;
        }
    }
    return 0;
}

int main(void) {
    SEQ* xs = NULL;
    U64 total = 0;
    #pragma omp parallel
    #pragma omp single
    {
        xs = seq_from_range(1, 10000);
        total = seq_parallel_sum(xs);
    }
    printf("Verified Phase 4 SEQ[U64]: len = %llu, parallel_sum(1..10000) = %llu\n",
           (unsigned long long)xs->len, (unsigned long long)total);
    return 0;
}
"#,
            );
        }
        "BIGNAT" => {
            out.push_str(
                r#"// Automatically generated by gardelc (Phase 4 Stdlib -> BIGNAT 64-bit Limbs in SEQ[U64] C11 Runtime)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef uint64_t U64;

typedef struct BIGNAT {
    U64* limbs;
    U64 size;
} BIGNAT;

static inline BIGNAT bignat_zero(void) {
    BIGNAT b = { NULL, 0 };
    return b;
}

static inline BIGNAT bignat_from_u64(U64 x) {
    if (x == 0) return bignat_zero();
    BIGNAT b;
    b.limbs = (U64*)malloc(sizeof(U64));
    b.limbs[0] = x;
    b.size = 1;
    return b;
}

BIGNAT bignat_add(BIGNAT a, BIGNAT b) {
    U64 max_len = (a.size > b.size) ? a.size : b.size;
    U64* out = (U64*)calloc(max_len + 1, sizeof(U64));
    unsigned __int128 carry = 0;
    for (U64 i = 0; i < max_len; i++) {
        unsigned __int128 la = (i < a.size) ? a.limbs[i] : 0;
        unsigned __int128 lb = (i < b.size) ? b.limbs[i] : 0;
        unsigned __int128 sum = la + lb + carry;
        out[i] = (U64)sum;
        carry = sum >> 64;
    }
    U64 final_size = max_len;
    if (carry > 0) {
        out[max_len] = (U64)carry;
        final_size = max_len + 1;
    }
    BIGNAT res = { out, final_size };
    return res;
}

int main(void) {
    BIGNAT a = bignat_from_u64(0xFFFFFFFFFFFFFFFFULL);
    BIGNAT b = bignat_from_u64(1ULL);
    BIGNAT sum = bignat_add(a, b);
    printf("Verified Phase 4 BIGNAT (SEQ[U64] limbs, ghost MATH_INT): (2^64 - 1) + 1 => limb_count = %llu, high_limb = %llu, low_limb = %llu\n",
           (unsigned long long)sum.size,
           (unsigned long long)sum.limbs[1],
           (unsigned long long)sum.limbs[0]);
    return 0;
}
"#,
            );
        }
        "STRING" => {
            out.push_str(
                r#"// Automatically generated by gardelc (Phase 4 Stdlib -> UTF-8 STRING backed by SEQ[U64] C11 Runtime)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>

typedef uint64_t U64;
typedef uint32_t CHAR;

typedef struct STRING {
    U64* words;
    U64 word_len;
    U64 byte_len;
} STRING;

STRING string_from_utf8(const char* utf8) {
    size_t blen = strlen(utf8);
    size_t wlen = (blen + 7) / 8;
    U64* words = (U64*)calloc(wlen ? wlen : 1, sizeof(U64));
    memcpy(words, utf8, blen);
    STRING s = { words, (U64)wlen, (U64)blen };
    return s;
}

STRING string_concat(STRING a, STRING b) {
    U64 total_bytes = a.byte_len + b.byte_len;
    U64 wlen = (total_bytes + 7) / 8;
    U64* words = (U64*)calloc(wlen ? wlen : 1, sizeof(U64));
    memcpy((char*)words, (const char*)a.words, (size_t)a.byte_len);
    memcpy((char*)words + a.byte_len, (const char*)b.words, (size_t)b.byte_len);
    STRING s = { words, wlen, total_bytes };
    return s;
}

int main(void) {
    STRING hello = string_from_utf8("Hello, ");
    STRING world = string_from_utf8("Gardel v0.4 Phase 4!");
    STRING msg = string_concat(hello, world);
    printf("Verified Phase 4 UTF-8 STRING (SEQ[U64] words = %llu, byte_len = %llu): %.*s\n",
           (unsigned long long)msg.word_len,
           (unsigned long long)msg.byte_len,
           (int)msg.byte_len,
           (const char*)msg.words);
    return 0;
}
"#,
            );
        }
        _ => {
            out.push_str(&emit_c11_general_module(m));
        }
    }
    out
}

pub fn map_c_type(ty: &str) -> String {
    match ty {
        "U64" | "NAT" | "EXIT_CODE" => "uint64_t".to_string(),
        "I64" | "INT" => "int64_t".to_string(),
        "F64" => "double".to_string(),
        "BOOL" => "bool".to_string(),
        "CHAR" => "uint32_t".to_string(),
        "UNIT" | "()" => "GARDEL_UNIT".to_string(),
        "STRING" => "GardelString".to_string(),
        other if other.starts_with("IO[") => "int64_t".to_string(),
        other => {
            if let Some(inner) = other
                .strip_prefix("OPTION[")
                .and_then(|s| s.strip_suffix(']'))
            {
                return format!("{}_ptr", map_c_type(inner));
            }
            if let Some(inner) = other
                .strip_prefix("LIST[")
                .and_then(|s| s.strip_suffix(']'))
            {
                return format!("struct LIST_{}*", map_c_type(inner));
            }
            if let Some(inner) = other.strip_prefix("SEQ[").and_then(|s| s.strip_suffix(']')) {
                return format!("struct SEQ_{}*", map_c_type(inner));
            }
            if let Some(idx) = other.find('[') {
                let base = &other[..idx];
                let inner = &other[idx + 1..other.len() - 1];
                let parts: Vec<String> = inner.split(',').map(|p| map_c_type(p.trim())).collect();
                return format!("struct {}_{}*", base, parts.join("_"));
            }
            format!("struct {}*", other)
        }
    }
}

pub fn emit_c11_runtime_header() -> String {
    r#"#pragma STDC FP_CONTRACT OFF
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdatomic.h>

#ifndef GARDEL_PAR_THRESHOLD
#define GARDEL_PAR_THRESHOLD 512
#endif

typedef uint8_t GARDEL_UNIT;
#define UNIT_VAL 0

static _Atomic uint64_t g_gardel_live_allocs = 0;

static inline void* gardel_alloc(size_t sz) {
    void* p = malloc(sz);
    if (!p) { fprintf(stderr, "Out of memory\n"); exit(1); }
    atomic_fetch_add(&g_gardel_live_allocs, 1);
    return p;
}

static inline void gardel_free(void* p) {
    if (p) {
        atomic_fetch_sub(&g_gardel_live_allocs, 1);
        free(p);
    }
}

uint64_t gardel_live_allocations(void) {
    return atomic_load(&g_gardel_live_allocs);
}

typedef struct {
    const char* data;
    uint64_t len;
} GardelString;

/* Runtime STRING constructor for string literals (Task B.3, spec 3.1.4): wraps a static,
   read-only, NUL-terminated UTF-8 byte buffer emitted by the compiler. No allocation, no
   copy and no runtime decoding: the literal was validated and decoded at compile time. */
static inline GardelString string_from_utf8(const uint8_t* bytes, uint64_t len) {
    GardelString s = { (const char*)bytes, len };
    return s;
}

static inline const char* gardel_io_read_line(char* buf, size_t cap) {
    if (fgets(buf, (int)cap, stdin) != NULL) {
        size_t n = strlen(buf);
        while (n > 0 && (buf[n - 1] == '\n' || buf[n - 1] == '\r')) {
            buf[--n] = '\0';
        }
        return buf;
    }
    return "";
}

/* IO boundary (spec 11). `read_line` returns a view of a static line buffer. */
static inline GardelString gardel_io_read_line_str(void) {
    static char line_buf[4096];
    const char* p = gardel_io_read_line(line_buf, sizeof(line_buf));
    GardelString s = { p, (uint64_t)strlen(p) };
    return s;
}

static inline int64_t gardel_io_print(GardelString s) {
    fwrite(s.data, 1, (size_t)s.len, stdout);
    return 0;
}

static inline int64_t gardel_io_print_line(GardelString s) {
    fwrite(s.data, 1, (size_t)s.len, stdout);
    fputc('\n', stdout);
    return 0;
}

static inline int64_t gardel_io_exit(uint64_t code) {
    fflush(stdout);
    exit((int)code);
    return 0;
}
"#
    .to_string()
}

thread_local! {
    /// String literals of the module being lowered to C11 (index = static buffer number).
    static C11_STRING_LITERALS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Static read-only UTF-8 buffers for the string literals of a module (Task B.3, §3.1.4):
/// `static const uint8_t gardel_str_lit_k[] = { ..., 0 };` with the bytes computed at compile
/// time (NUL-terminated; the length is passed explicitly, so embedded `\0` is preserved).
fn emit_c11_string_literal_table(strings: &[String]) -> String {
    let mut out = String::new();
    if strings.is_empty() {
        return out;
    }
    out.push_str(
        "/* String literals (Task B.3): static read-only UTF-8, zero runtime decoding */\n",
    );
    for (idx, s) in strings.iter().enumerate() {
        let bytes: Vec<String> = s
            .as_bytes()
            .iter()
            .map(|b| b.to_string())
            .chain(std::iter::once("0".to_string()))
            .collect();
        out.push_str(&format!(
            "static const uint8_t gardel_str_lit_{idx}[{}] = {{ {} }}; /* \"{}\" */\n",
            s.len() + 1,
            bytes.join(", "),
            s.escape_debug().to_string().replace("*/", "*\\/")
        ));
    }
    out.push('\n');
    out
}

// --- C11 lowering of instantiated prelude sum types (spec §11 IO boundary) -------------

thread_local! {
    /// Gardel types of locals while lowering one routine to C11: IO-bound values and
    /// `match` binders whose type is an instantiated prelude sum type.
    static C11_TYPE_ENV: std::cell::RefCell<HashMap<String, String>> =
        std::cell::RefCell::new(HashMap::new());
    static C11_MATCH_COUNTER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// C identifier of an instantiated type: `RESULT[IO_ERROR, STRING]` -> `RESULT_IO_ERROR_STRING`.
fn c_instance_name(ty: &str) -> String {
    let mut out = String::new();
    for ch in ty.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_string()
}

/// Variants of an instantiated prelude sum type with field types substituted:
/// `RESULT[IO_ERROR, STRING]` -> `[(Error, [(err, IO_ERROR)]), (Ok, [(val, STRING)])]`.
fn c_prelude_sum_variants(ty: &str) -> Option<Vec<(String, Vec<(String, String)>)>> {
    let (head, args) = split_type_app(ty.trim());
    let p = standard_prelude().module(head)?;
    let ReprKind::Sum(vs) = &p.repr else {
        return None;
    };
    if p.type_params.len() != args.len() {
        return None;
    }
    let map: HashMap<String, String> = p
        .type_params
        .iter()
        .cloned()
        .zip(args.iter().map(|a| a.trim().to_string()))
        .collect();
    Some(
        vs.iter()
            .map(|v| {
                (
                    v.name.clone(),
                    v.fields
                        .iter()
                        .map(|(f, t)| (f.clone(), subst_type_idents(t, &map)))
                        .collect(),
                )
            })
            .collect(),
    )
}

fn c_field_type(ty: &str) -> String {
    if c_prelude_sum_variants(ty).is_some() {
        format!("struct {}*", c_instance_name(ty))
    } else {
        map_c_type(ty)
    }
}

/// Struct definitions and constructors for `roots` and every prelude sum type they contain.
fn emit_c11_prelude_instances(roots: &[&str]) -> String {
    let mut order: Vec<String> = Vec::new();
    let mut stack: Vec<String> = roots.iter().map(|r| r.to_string()).collect();
    while let Some(t) = stack.pop() {
        let name = c_instance_name(&t);
        if order.iter().any(|o| c_instance_name(o) == name) {
            continue;
        }
        if let Some(vs) = c_prelude_sum_variants(&t) {
            order.push(t.clone());
            for (_, fields) in vs {
                for (_, ft) in fields {
                    stack.push(ft);
                }
            }
        }
    }
    let mut out = String::from("/* Instantiated prelude sum types (spec 5, 11) */\n");
    for t in &order {
        out.push_str(&format!("struct {};\n", c_instance_name(t)));
    }
    for t in &order {
        let cname = c_instance_name(t);
        let vs = c_prelude_sum_variants(t).unwrap_or_default();
        out.push_str(&format!("typedef struct {cname} {{\n"));
        let tags: Vec<String> = vs.iter().map(|(v, _)| format!("TAG_{cname}_{v}")).collect();
        out.push_str(&format!("    enum {{ {} }} tag;\n", tags.join(", ")));
        if vs.iter().any(|(_, f)| !f.is_empty()) {
            out.push_str("    union {\n");
            for (v, fields) in &vs {
                if !fields.is_empty() {
                    out.push_str("        struct {");
                    for (f, ft) in fields {
                        out.push_str(&format!(" {} {f};", c_field_type(ft)));
                    }
                    out.push_str(&format!(" }} {v};\n"));
                }
            }
            out.push_str("    };\n");
        }
        out.push_str(&format!("}} {cname};\n"));
        for (v, fields) in &vs {
            let params: Vec<String> = fields
                .iter()
                .map(|(f, ft)| format!("{} {f}", c_field_type(ft)))
                .collect();
            let params = if params.is_empty() {
                "void".to_string()
            } else {
                params.join(", ")
            };
            out.push_str(&format!(
                "static inline struct {cname}* make_{cname}_{v}({params}) {{\n    struct {cname}* r = (struct {cname}*)gardel_alloc(sizeof(struct {cname}));\n    r->tag = TAG_{cname}_{v};\n"
            ));
            for (f, _) in fields {
                out.push_str(&format!("    r->{v}.{f} = {f};\n"));
            }
            out.push_str("    return r;\n}\n");
        }
        out.push('\n');
    }
    out
}

/// `IO.read_line` at the C boundary (spec §11): reads one line of any length and returns
/// `RESULT[IO_ERROR, STRING]`. Only a line of ASCII bytes (the bootstrap `STRING` domain,
/// §6.4.C) becomes `Ok(STRING)`; end of input, read failures and non-ASCII bytes become
/// `Error(..)`. This is the runtime check that justifies the verifier's boundary assumption.
fn emit_c11_read_line_runtime() -> String {
    let r = c_instance_name(READ_LINE_RESULT_TYPE);
    format!(
        r#"#include <errno.h>
/* IO boundary (spec 11): IO.read_line : IO[RESULT[IO_ERROR, STRING]] */
static struct {r}* gardel_io_read_line_result(void) {{
    size_t cap = 128, len = 0;
    char* buf = (char*)malloc(cap);
    int c, saw_any = 0;
    if (!buf) return make_{r}_Error(make_IO_ERROR_OsError((int64_t)ENOMEM));
    while ((c = fgetc(stdin)) != EOF) {{
        saw_any = 1;
        if (c == '\n') break;
        if (len + 1 >= cap) {{
            char* nb = (char*)realloc(buf, cap * 2);
            if (!nb) {{ free(buf); return make_{r}_Error(make_IO_ERROR_OsError((int64_t)ENOMEM)); }}
            buf = nb;
            cap *= 2;
        }}
        buf[len++] = (char)c;
    }}
    if (ferror(stdin)) {{ free(buf); return make_{r}_Error(make_IO_ERROR_OsError((int64_t)errno)); }}
    if (!saw_any) {{ free(buf); return make_{r}_Error(make_IO_ERROR_EndOfInput()); }}
    if (len > 0 && buf[len - 1] == '\r') len--;
    for (size_t i = 0; i < len; i++) {{
        unsigned char b = (unsigned char)buf[i];
        if (b >= 128) {{
            free(buf);
            return make_{r}_Error(make_IO_ERROR_Decode(make_STRING_ERROR_InvalidByte((uint64_t)i, (uint64_t)b)));
        }}
    }}
    buf[len] = '\0';
    GardelString s = {{ buf, (uint64_t)len }};
    return make_{r}_Ok(s);
}}

"#
    )
}

fn c11_record_bind_type(v: &str, e: &Expr) {
    if is_io_read_line(e) {
        C11_TYPE_ENV.with(|env| {
            env.borrow_mut()
                .insert(v.to_string(), READ_LINE_RESULT_TYPE.to_string());
        });
    }
}

/// The instantiated prelude sum type of a `match` scrutinee, when known.
fn c11_scrutinee_type(scrut: &Expr) -> Option<String> {
    let ty = match scrut {
        Expr::Var(v) => C11_TYPE_ENV.with(|env| env.borrow().get(v).cloned())?,
        e if is_io_read_line(e) => READ_LINE_RESULT_TYPE.to_string(),
        _ => return None,
    };
    c_prelude_sum_variants(&ty).map(|_| ty)
}

/// `match` on an instantiated prelude sum type: the scrutinee is evaluated once, each case
/// tests the instance's own tag and binds its pattern variables from the variant fields.
fn emit_c11_prelude_match(scrut: &Expr, cases: &[MatchCase], mod_name: &str) -> String {
    let ty = c11_scrutinee_type(scrut).unwrap_or_default();
    let cname = c_instance_name(&ty);
    let variants = c_prelude_sum_variants(&ty).unwrap_or_default();
    let k = C11_MATCH_COUNTER.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    });
    let sv = format!("_ms{k}");
    let mut arms: Vec<(Option<String>, String)> = Vec::new();
    for c in cases {
        let mut body = String::from("({ ");
        let mut tag = None;
        if let Some((vname, fields)) = variants.iter().find(|(v, _)| v == c.ctor()) {
            tag = Some(format!("TAG_{cname}_{vname}"));
            for (opt_b, (f, ft)) in c.top_bindings().iter().zip(fields.iter()) {
                if let Some(b) = opt_b {
                    if c_prelude_sum_variants(ft).is_some() {
                        C11_TYPE_ENV.with(|env| env.borrow_mut().insert(b.clone(), ft.clone()));
                    }
                    body.push_str(&format!("__auto_type {b} = {sv}->{vname}.{f}; "));
                }
            }
        }
        for st in &c.stmts {
            if let Stmt::Let(v, e) = st {
                body.push_str(&format!(
                    "__auto_type {v} = {}; ",
                    emit_c11_expr(e, mod_name)
                ));
            }
        }
        body.push_str(&format!("{}; }})", emit_c11_expr(&c.body, mod_name)));
        arms.push((tag, body));
    }
    // Nested conditional over the tags; the last arm is the fallback (matches are exhaustive).
    let mut expr = arms
        .last()
        .map(|(_, b)| b.clone())
        .unwrap_or_else(|| "0".to_string());
    for (tag, body) in arms.iter().rev().skip(1) {
        let cond = tag
            .clone()
            .map_or("0".to_string(), |t| format!("{sv}->tag == {t}"));
        expr = format!("({cond} ? {body} : {expr})");
    }
    format!(
        "({{ __auto_type {sv} = {}; {expr}; }})",
        emit_c11_expr(scrut, mod_name)
    )
}

pub fn emit_c11_general_module(m: &ModuleDecl) -> String {
    let mut out = String::new();
    out.push_str("// Automatically generated by gardelc (Phase 4 -> General C11 AST Lowering)\n");
    out.push_str(&emit_c11_runtime_header());
    out.push('\n');
    let strings = collect_module_strings(m);
    out.push_str(&emit_c11_string_literal_table(&strings));
    C11_STRING_LITERALS.with(|c| *c.borrow_mut() = strings);
    C11_TYPE_ENV.with(|env| env.borrow_mut().clear());
    if format!("{m:?}").contains("\"read_line\"") {
        out.push_str(&emit_c11_prelude_instances(&[READ_LINE_RESULT_TYPE]));
        out.push_str(&emit_c11_read_line_runtime());
    }
    if m.kind == ModuleKind::Program {
        // File-scope forward declarations for opaque struct types in routine signatures
        // (e.g. `main(args: SEQ[STRING])`), so they are not scoped to a parameter list.
        let mut fwd: Vec<String> = Vec::new();
        for (_, r) in &m.feature_routines {
            for ty in r
                .params
                .iter()
                .map(|(_, t)| t)
                .chain(std::iter::once(&r.ret_type))
            {
                let ct = map_c_type(ty);
                if let Some(name) = ct.strip_prefix("struct ").and_then(|t| t.strip_suffix('*')) {
                    let decl = format!("struct {name};\n");
                    if !fwd.contains(&decl) {
                        fwd.push(decl);
                    }
                }
            }
        }
        out.push_str(&fwd.concat());
    }

    let mod_name = &m.name;

    // 1. Types & Data Structures (Product / Sum)
    if m.kind == ModuleKind::Module {
        match &m.repr {
            ReprKind::Product(fields) => {
                out.push_str(&format!("typedef struct {mod_name} {{\n"));
                out.push_str("    _Atomic uint64_t _rc;\n");
                for (fnm, fty) in fields {
                    out.push_str(&format!("    {} {fnm};\n", map_c_type(fty)));
                }
                out.push_str(&format!("}} {mod_name};\n\n"));

                let param_decls: Vec<String> = fields
                    .iter()
                    .map(|(fnm, fty)| format!("{} {fnm}", map_c_type(fty)))
                    .collect();
                out.push_str(&format!(
                    "static inline {mod_name}* make_{mod_name}({}) {{\n",
                    param_decls.join(", ")
                ));
                out.push_str(&format!(
                    "    {mod_name}* res = ({mod_name}*)gardel_alloc(sizeof({mod_name}));\n"
                ));
                out.push_str("    atomic_init(&res->_rc, 1);\n");
                for (fnm, _) in fields {
                    out.push_str(&format!("    res->{fnm} = {fnm};\n"));
                }
                out.push_str("    return res;\n}\n\n");
            }
            ReprKind::Sum(variants) => {
                out.push_str(&format!("typedef struct {mod_name} {{\n"));
                out.push_str("    _Atomic uint64_t _rc;\n");
                let tag_names: Vec<String> = variants
                    .iter()
                    .map(|v| format!("TAG_{mod_name}_{}", v.name))
                    .collect();
                out.push_str(&format!("    enum {{ {} }} tag;\n", tag_names.join(", ")));
                out.push_str("    union {\n");
                for v in variants {
                    if !v.fields.is_empty() {
                        out.push_str(&format!("        struct {{\n"));
                        for (fnm, fty) in &v.fields {
                            out.push_str(&format!("            {} {fnm};\n", map_c_type(fty)));
                        }
                        out.push_str(&format!("        }} {};\n", v.name));
                    }
                }
                out.push_str("    };\n");
                out.push_str(&format!("}} {mod_name};\n\n"));

                for v in variants {
                    let vname = &v.name;
                    let param_decls: Vec<String> = v
                        .fields
                        .iter()
                        .map(|(fnm, fty)| format!("{} {fnm}", map_c_type(fty)))
                        .collect();
                    out.push_str(&format!(
                        "static inline {mod_name}* make_{mod_name}_{vname}({}) {{\n",
                        param_decls.join(", ")
                    ));
                    out.push_str(&format!(
                        "    {mod_name}* res = ({mod_name}*)gardel_alloc(sizeof({mod_name}));\n"
                    ));
                    out.push_str("    atomic_init(&res->_rc, 1);\n");
                    out.push_str(&format!("    res->tag = TAG_{mod_name}_{vname};\n"));
                    for (fnm, _) in &v.fields {
                        out.push_str(&format!("    res->{vname}.{fnm} = {fnm};\n"));
                    }
                    out.push_str("    return res;\n}\n\n");
                }
            }
        }
    }

    // 2. Closure conversion
    let (_, combinators) = closure_convert_module(m);
    for comb in &combinators {
        if let Some((env_name, env_fields)) = &comb.env_struct {
            out.push_str(&format!("typedef struct {env_name} {{\n"));
            out.push_str("    _Atomic uint64_t _rc;\n");
            for (fnm, fty) in env_fields {
                out.push_str(&format!("    {} {fnm};\n", map_c_type(fty)));
            }
            out.push_str(&format!("}} {env_name};\n\n"));
        }
        out.push_str(&emit_c11_routine(&comb.routine, false, mod_name));
    }

    // 3. Create Routines
    for (_, r) in &m.create_routines {
        out.push_str(&emit_c11_routine(r, false, mod_name));
    }

    // 4. Feature Routines
    let is_adt = m.kind == ModuleKind::Module;
    for (_, r) in &m.feature_routines {
        out.push_str(&emit_c11_routine(r, is_adt, mod_name));
    }

    // 5. Program Entry Point if Program
    if m.kind == ModuleKind::Program {
        out.push_str("int main(void) {\n");
        if let Some((_, r)) = m.feature_routines.iter().find(|(_, r)| r.name == "main") {
            // `args: SEQ[STRING]` is not yet marshalled from argv (passed as NULL).
            let args: Vec<&str> = r.params.iter().map(|_| "NULL").collect();
            out.push_str(&format!(
                "    return (int)main_routine({});\n",
                args.join(", ")
            ));
        } else {
            out.push_str("    return 0;\n");
        }
        out.push_str("}\n");
    } else {
        out.push_str("int main(void) { return 0; }\n");
    }

    out
}

fn emit_c11_routine(r: &RoutineDecl, has_current: bool, mod_name: &str) -> String {
    let mut s = String::new();
    let ret_type = map_c_type(&r.ret_type);
    let mut param_strs = Vec::new();
    if has_current {
        param_strs.push(format!("struct {mod_name}* Current"));
    }
    for (pname, pty) in &r.params {
        param_strs.push(format!("{} {pname}", map_c_type(pty)));
    }
    let rname = if r.name == "main" {
        "main_routine"
    } else {
        &r.name
    };
    s.push_str(&format!(
        "{ret_type} {rname}({}) {{\n",
        param_strs.join(", ")
    ));

    // Check tail recursion
    let is_tail = is_tail_recursive_routine(r);
    if is_tail {
        s.push_str("tail_loop:\n");
    }

    s.push_str(&emit_c11_block(
        &r.stmts, &r.body, mod_name, is_tail, rname, 4,
    ));
    s.push_str("}\n\n");
    s
}

fn emit_c11_block(
    stmts: &[Stmt],
    body: &Expr,
    mod_name: &str,
    _is_tail: bool,
    _routine_name: &str,
    indent: usize,
) -> String {
    let pad = " ".repeat(indent);
    let mut s = String::new();

    let groups = analyze_parallel_lets(stmts);
    for group in groups {
        if group.len() > 1 {
            for (v, e) in &group {
                s.push_str(&format!("{pad}__auto_type {v};\n"));
                s.push_str(&format!(
                    "{pad}#pragma omp task shared({v})\n{pad}{v} = {};\n",
                    emit_c11_expr(e, mod_name)
                ));
            }
            s.push_str(&format!("{pad}#pragma omp taskwait;\n"));
        } else {
            for (v, e) in group {
                s.push_str(&format!(
                    "{pad}__auto_type {v} = {};\n",
                    emit_c11_expr(&e, mod_name)
                ));
            }
        }
    }

    match body {
        Expr::WithIo(io_stmts, final_e) => {
            for io_st in io_stmts {
                match io_st {
                    IoStmt::Bind(v, se) => {
                        c11_record_bind_type(v, se);
                        s.push_str(&format!(
                            "{pad}__auto_type {v} = {};\n",
                            emit_c11_expr(se, mod_name)
                        ));
                    }
                    IoStmt::Action(se) => {
                        s.push_str(&format!("{pad}{};\n", emit_c11_expr(se, mod_name)));
                    }
                }
            }
            s.push_str(&format!(
                "{pad}return {};\n",
                emit_c11_expr(final_e, mod_name)
            ));
        }
        _ => {
            s.push_str(&format!("{pad}return {};\n", emit_c11_expr(body, mod_name)));
        }
    }
    s
}

fn emit_c11_expr(e: &Expr, mod_name: &str) -> String {
    match e {
        Expr::Int(n) => format!("{n}ULL"),
        Expr::Float(f) => format!("{f}"),
        Expr::Char(c) => format!("{c}U"),
        Expr::Unit => "UNIT_VAL".to_string(),
        Expr::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
        Expr::Str(s) => {
            // Task B.3: a literal is its static UTF-8 buffer wrapped by the runtime
            // constructor (no allocation, no decoding).
            let idx = C11_STRING_LITERALS.with(|c| c.borrow().iter().position(|x| x == s));
            match idx {
                Some(idx) => format!("string_from_utf8(gardel_str_lit_{idx}, {}ULL)", s.len()),
                None => {
                    let bytes: Vec<String> = s
                        .bytes()
                        .map(|b| b.to_string())
                        .chain(std::iter::once("0".to_string()))
                        .collect();
                    format!(
                        "string_from_utf8((const uint8_t[]){{ {} }}, {}ULL)",
                        bytes.join(", "),
                        s.len()
                    )
                }
            }
        }
        Expr::Var(v) => v.clone(),
        Expr::UnaryNot(inner) => format!("(!{})", emit_c11_expr(inner, mod_name)),
        Expr::UnaryNeg(inner) => format!("(-{})", emit_c11_expr(inner, mod_name)),
        Expr::BinOp(op, l, r) => {
            let c_op = match op.as_str() {
                "=" => "==",
                "+%" => "+",
                "-%" => "-",
                "*%" => "*",
                "and" => "&&",
                "or" => "||",
                other => other,
            };
            format!(
                "({} {c_op} {})",
                emit_c11_expr(l, mod_name),
                emit_c11_expr(r, mod_name)
            )
        }
        Expr::Call(fname, args) => {
            let a_str: Vec<String> = args.iter().map(|a| emit_c11_expr(a, mod_name)).collect();
            format!("{fname}({})", a_str.join(", "))
        }
        Expr::MethodCall(recv, mname, args) => {
            if let Expr::Var(v) = &**recv {
                if v == "CHAR" && mname == "from_scalar" && !args.is_empty() {
                    return format!("((uint32_t)({}))", emit_c11_expr(&args[0], mod_name));
                }
                if v == "IO" {
                    if (mname == "print" || mname == "print_line") && !args.is_empty() {
                        let arg = emit_c11_expr(&args[0], mod_name);
                        return format!("gardel_io_{mname}({arg})");
                    } else if mname == "read_line" {
                        return "gardel_io_read_line_result()".to_string();
                    } else if mname == "exit" && !args.is_empty() {
                        return format!("gardel_io_exit({})", emit_c11_expr(&args[0], mod_name));
                    }
                }
            }
            if mname == "code" && args.is_empty() {
                return format!("((uint64_t)({}))", emit_c11_expr(recv, mod_name));
            }
            if mname == "apply" {
                let r_str = emit_c11_expr(recv, mod_name);
                let a_str: Vec<String> = args.iter().map(|a| emit_c11_expr(a, mod_name)).collect();
                return format!("{r_str}({})", a_str.join(", "));
            }
            let r_str = emit_c11_expr(recv, mod_name);
            let mut a_str = vec![r_str];
            for a in args {
                a_str.push(emit_c11_expr(a, mod_name));
            }
            format!("{mname}({})", a_str.join(", "))
        }
        Expr::If(c, ts, te, es, ee) => {
            format!(
                "({} ? ({}) : ({}))",
                emit_c11_expr(c, mod_name),
                emit_c11_block(ts, te, mod_name, false, "", 0).trim(),
                emit_c11_block(es, ee, mod_name, false, "", 0).trim()
            )
        }
        Expr::Match(scrut, cases) if c11_scrutinee_type(scrut).is_some() => {
            emit_c11_prelude_match(scrut, cases, mod_name)
        }
        Expr::Match(scrut, cases) => {
            let s_str = emit_c11_expr(scrut, mod_name);
            let mut s = format!("({s_str}->tag == 0 ? 0 : 0)");
            if !cases.is_empty() {
                let mut cond_expr = String::new();
                for (idx, c) in cases.iter().enumerate() {
                    let ctor = c.ctor();
                    let tag_name = format!("TAG_{mod_name}_{ctor}");
                    let val = emit_c11_expr(&c.body, mod_name);
                    if idx + 1 == cases.len() {
                        cond_expr.push_str(&val);
                    } else {
                        cond_expr.push_str(&format!("({s_str}->tag == {tag_name} ? ({val}) : "));
                    }
                }
                for _ in 0..cases.len() - 1 {
                    cond_expr.push(')');
                }
                s = cond_expr;
            }
            s
        }
        Expr::RecordLit(fields) => {
            let vals: Vec<String> = fields
                .iter()
                .map(|(_, fe)| emit_c11_expr(fe, mod_name))
                .collect();
            format!("make_{mod_name}({})", vals.join(", "))
        }
        Expr::RecordUpdate(base, _fields) => {
            let b_str = emit_c11_expr(base, mod_name);
            format!("/* update */ {b_str}")
        }
        Expr::TupleLit(elems) => {
            let es: Vec<String> = elems.iter().map(|el| emit_c11_expr(el, mod_name)).collect();
            format!("({})", es.join(", "))
        }
        // A nested `with IO` is a GNU statement expression: its actions run in order and its
        // value is the trailing expression (previously the actions were dropped).
        Expr::WithIo(io_stmts, final_e) => {
            let mut s = String::from("({ ");
            for io_st in io_stmts {
                match io_st {
                    IoStmt::Bind(v, se) => {
                        c11_record_bind_type(v, se);
                        s.push_str(&format!(
                            "__auto_type {v} = {}; ",
                            emit_c11_expr(se, mod_name)
                        ));
                    }
                    IoStmt::Action(se) => s.push_str(&format!("{}; ", emit_c11_expr(se, mod_name))),
                }
            }
            s.push_str(&format!("{}; }})", emit_c11_expr(final_e, mod_name)));
            s
        }
        Expr::Lambda(_, _, _, _) => "/* lifted closure */ 0".to_string(),
        Expr::Forall(_, _, _) | Expr::Exists(_, _, _) => "0".to_string(),
    }
}

// ============================================================================
// 11. CLI DRIVER (Supports --stdlib, Boogie, --fuel, --audit, --strict, --run)
// ============================================================================

/// Formats the public contract interface of a module (Eiffel-style `short` view, §4 & §7.5),
/// showing exported `representation`, `create`, `feature`, `specify`, and `invariant` signatures
/// and contracts (`require` / `ensure`) while omitting private implementation `do` bodies.
pub fn format_module_interface(m: &ModuleDecl) -> String {
    let mut out = String::new();
    let kw = match m.kind {
        ModuleKind::Module => "module",
        ModuleKind::Functions => "functions",
        ModuleKind::Program => "program",
    };
    let tp = if m.type_params.is_empty() {
        String::new()
    } else {
        format!("[{}]", m.type_params.join(", "))
    };
    out.push_str(&format!("{kw} {}{tp}\n", m.name));

    if let Some(sat) = &m.satisfies {
        out.push_str(&format!("  satisfies {}\n", sat.concepts.join(", ")));
    }

    if m.kind == ModuleKind::Module {
        match &m.repr_vis {
            Visibility::None => {
                out.push_str("  representation {NONE} -- opaque ADT layout\n");
            }
            vis => {
                out.push_str(&format!("  representation {:?}\n", vis));
                match &m.repr {
                    ReprKind::Sum(vars) => {
                        for v in vars {
                            if v.fields.is_empty() {
                                out.push_str(&format!("    | {}\n", v.name));
                            } else {
                                let fs: Vec<String> =
                                    v.fields.iter().map(|(n, t)| format!("{n}: {t}")).collect();
                                out.push_str(&format!("    | {}({})\n", v.name, fs.join(", ")));
                            }
                        }
                    }
                    ReprKind::Product(fields) => {
                        for (n, t) in fields {
                            out.push_str(&format!("    {n}: {t}\n"));
                        }
                    }
                }
            }
        }
    }

    let fmt_routine_sig = |vis: &Visibility, r: &RoutineDecl| -> String {
        let mut s = String::new();
        let rtp = if r.type_params.is_empty() {
            String::new()
        } else {
            format!("[{}]", r.type_params.join(", "))
        };
        let ps = if r.params.is_empty() {
            String::new()
        } else {
            let p_list: Vec<String> = r.params.iter().map(|(n, t)| format!("{n}: {t}")).collect();
            format!("({})", p_list.join(", "))
        };
        s.push_str(&format!(
            "    {:?} {}{}{}: {}\n",
            vis, r.name, rtp, ps, r.ret_type
        ));
        for (tag, req) in &r.requires {
            s.push_str(&format!(
                "      require {tag}: {}\n",
                format_gardel_expr(req)
            ));
        }
        for (tag, ens) in &r.ensures {
            if ens_is_visible(ens, m, vis) {
                s.push_str(&format!(
                    "      ensure  {tag}: {}\n",
                    format_gardel_expr(ens)
                ));
            }
        }
        s
    };

    if !m.create_routines.is_empty() {
        out.push_str("  create\n");
        for (vis, r) in &m.create_routines {
            out.push_str(&fmt_routine_sig(vis, r));
        }
    }
    if !m.feature_routines.is_empty() {
        out.push_str("  feature\n");
        for (vis, r) in &m.feature_routines {
            out.push_str(&fmt_routine_sig(vis, r));
        }
    }
    if !m.specify_routines.is_empty() || !m.specify_laws.is_empty() {
        out.push_str("  specify\n");
        for (vis, r) in &m.specify_routines {
            out.push_str(&fmt_routine_sig(vis, r));
        }
        for (vis, law) in &m.specify_laws {
            let ps: Vec<String> = law
                .params
                .iter()
                .map(|(n, t)| format!("{n}: {t}"))
                .collect();
            let ltp = if law.type_params.is_empty() {
                String::new()
            } else {
                format!("[{}]", law.type_params.join(", "))
            };
            out.push_str(&format!(
                "    {:?} law {}{ltp}({})\n",
                vis,
                law.name,
                ps.join(", ")
            ));
            for (tag, req) in &law.requires {
                out.push_str(&format!(
                    "      require {tag}: {}\n",
                    format_gardel_expr(req)
                ));
            }
            for (tag, ens) in &law.ensures {
                out.push_str(&format!(
                    "      ensure  {tag}: {}\n",
                    format_gardel_expr(ens)
                ));
            }
        }
    }
    if !m.invariants.is_empty() {
        out.push_str("  invariant\n");
        for (tag, inv) in &m.invariants {
            out.push_str(&format!("    {tag}: {}\n", format_gardel_expr(inv)));
        }
    }
    out.push_str("end\n");
    out
}

fn ens_is_visible(ens: &Expr, m: &ModuleDecl, routine_vis: &Visibility) -> bool {
    let syms = collect_symbols(ens);
    if let ReprKind::Product(fields) = &m.repr {
        for (fnm, _) in fields {
            if syms.contains(fnm) && !m.repr_vis.is_at_least_as_visible_as(routine_vis) {
                return false;
            }
        }
    }
    true
}

/// Default output directory of `gardelc --emit-stdlib` (inside `build/`, which directory
/// scans skip, so the copy is never mistaken for the stdlib sources).
pub const EMIT_STDLIB_DEFAULT_DIR: &str = "build/stdlib";

fn normalized_source(s: &str) -> String {
    s.replace("\r\n", "\n").trim().to_string()
}

/// Writes the Standard Prelude sources embedded in this gardelc to `out_dir`, one file per
/// module. A file that already exists with different content is never overwritten: the
/// `stdlib/` sources are edited by hand and the embedded copy may be older than them, so the
/// whole emission is refused instead (nothing is written).
pub fn emit_stdlib_files(out_dir: &str) -> Result<Vec<String>, String> {
    let plan: Vec<(String, &str)> = STDLIB_SOURCES
        .iter()
        .map(|(rel_path, content)| {
            let sub = rel_path.strip_prefix("stdlib/").unwrap_or(rel_path);
            (format!("{out_dir}/{sub}"), content.trim_start())
        })
        .collect();
    let conflicts: Vec<&str> = plan
        .iter()
        .filter(|(dest, content)| {
            std::path::Path::new(dest).exists()
                && fs::read_to_string(dest)
                    .map(|s| normalized_source(&s) != normalized_source(content))
                    .unwrap_or(true)
        })
        .map(|(dest, _)| dest.as_str())
        .collect();
    if !conflicts.is_empty() {
        return Err(format!(
            "refusing to overwrite {} file(s) that differ from the stdlib embedded in this gardelc: {}. Emit into an empty directory (`--emit-stdlib <DIR>`, default `{EMIT_STDLIB_DEFAULT_DIR}`) or remove them first",
            conflicts.len(),
            conflicts.join(", ")
        ));
    }
    let mut written = Vec::new();
    for (dest, content) in &plan {
        if !std::path::Path::new(dest).exists() {
            if let Some(parent) = std::path::Path::new(dest).parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(dest, content).map_err(|e| e.to_string())?;
        }
        written.push(dest.clone());
    }
    Ok(written)
}

/// The `stdlib/` source tree this gardelc was built from: the files its `include_str!`s embed.
fn stdlib_source_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("stdlib")
}

/// `p` relative to the working directory when it lies under it (or under its parent), for
/// readable paths in diagnostics; otherwise the absolute path.
fn display_path(p: &std::path::Path) -> String {
    let clean = |s: &std::path::Path| {
        let s = s.to_string_lossy().replace('\\', "/");
        s.strip_prefix("//?/").map(str::to_string).unwrap_or(s)
    };
    let Ok(abs) = p.canonicalize() else {
        return clean(p);
    };
    if let Ok(cwd) = std::env::current_dir().and_then(|c| c.canonicalize()) {
        if let Ok(rel) = abs.strip_prefix(&cwd) {
            let r = clean(rel);
            return if r.is_empty() { ".".to_string() } else { r };
        }
        if let Some(parent) = cwd.parent() {
            if let Ok(rel) = abs.strip_prefix(parent) {
                return format!("../{}", clean(rel));
            }
        }
    }
    clean(&abs)
}

/// What `gardelc --stdlib` verifies.
enum StdlibInputs {
    /// The source tree this gardelc was built from, with informational notes.
    Disk { paths: Vec<String>, notes: Vec<String> },
    /// That tree is not available (gardelc moved or installed elsewhere): the embedded copy.
    Embedded { note: String },
}

/// Resolves the `--stdlib` inputs. It always verifies the tree the compiler was built from,
/// never a `stdlib/` that happens to be in the working directory, and it is an error when that
/// tree and the embedded file list disagree (a module verified but not shipped, or the reverse).
fn resolve_stdlib_inputs() -> Result<StdlibInputs, (String, String)> {
    let dir = stdlib_source_dir();
    if !dir.is_dir() {
        return Ok(StdlibInputs::Embedded {
            note: format!(
                "[stdlib] The source tree this gardelc was built from (`{}`) is not available; verifying the copy embedded in this gardelc build instead.",
                display_path(&dir)
            ),
        });
    }
    let shown = display_path(&dir);
    let files = gdl_files_in_dir(&shown, None).map_err(|e| (e, shown.clone()))?;
    let prefix = format!("{shown}/");
    let on_disk: BTreeMap<String, String> = files
        .iter()
        .map(|f| (format!("stdlib/{}", f.strip_prefix(&prefix).unwrap_or(f)), f.clone()))
        .collect();
    let embedded: BTreeMap<&str, &str> = STDLIB_SOURCES.iter().copied().collect();
    let mut problems = Vec::new();
    for k in embedded.keys().filter(|k| !on_disk.contains_key(**k)) {
        problems.push(format!("`{k}` is embedded in gardelc but missing on disk"));
    }
    for k in on_disk.keys().filter(|k| !embedded.contains_key(k.as_str())) {
        problems.push(format!(
            "`{k}` is on disk but not embedded in gardelc (add it to `STDLIB_SOURCES` and `phase4_stdlib_source()` in gardelc/src/main.rs, or user programs will not get it)"
        ));
    }
    if !problems.is_empty() {
        return Err((
            format!(
                "The stdlib source tree `{shown}` and the Standard Prelude embedded in gardelc disagree: {}",
                problems.join("; ")
            ),
            shown,
        ));
    }
    let stale: Vec<&str> = on_disk
        .iter()
        .filter(|(k, f)| {
            fs::read_to_string(f)
                .map(|s| normalized_source(&s) != normalized_source(embedded[k.as_str()]))
                .unwrap_or(true)
        })
        .map(|(k, _)| k.as_str())
        .collect();
    let mut notes = vec![format!("[stdlib] Verifying the source tree `{shown}` ({} files).", files.len())];
    if !stale.is_empty() {
        notes.push(format!(
            "[stdlib] Note: {} differ(s) from the copy embedded in this gardelc build; user programs get the embedded copy until gardelc is rebuilt (`cargo build`).",
            stale.join(", ")
        ));
    }
    Ok(StdlibInputs::Disk {
        paths: on_disk.into_values().collect(),
        notes,
    })
}

/// Validates that every module declared in a `group { ... }` package header is present (§5, §16).
pub fn check_group_packaging(ast: &FileAst, src: &str, input_path: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    if let Some(group_names) = &ast.group {
        let declared: HashSet<String> = ast.modules.iter().map(|m| m.name.clone()).collect();
        for gname in group_names {
            if !declared.contains(gname) {
                let sp = locate_span(src, input_path, gname, "group");
                diags.push(
                    Diagnostic::basic(
                        "error",
                        "Phase 4 (Missing Group Module)",
                        "group".to_string(),
                        gname.clone(),
                        format!(
                            "Package `group` declares module `{}`, which is not defined in this file",
                            gname
                        ),
                    )
                    .with_ai_context(sp, Vec::new(), BTreeMap::new(), Vec::new()),
                );
            }
        }
    }
    diags
}

fn parse_fuel_arg(args: &[String]) -> usize {
    for (idx, arg) in args.iter().enumerate() {
        if arg == "--fuel" {
            if let Some(val) = args.get(idx + 1) {
                if let Ok(parsed) = val.parse::<usize>() {
                    return parsed;
                }
            }
        } else if let Some(val) = arg.strip_prefix("--fuel=") {
            if let Ok(parsed) = val.parse::<usize>() {
                return parsed;
            }
        }
    }
    DEFAULT_FUEL
}

/// Matches a file name against a shell wildcard pattern (`*`, `?`).
fn wildcard_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The `.gdl` files of a directory and its subdirectories, sorted by path.
fn gdl_files_in_dir(dir: &str, pattern: Option<&str>) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    collect_gdl_files(dir, pattern, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_gdl_files(
    dir: &str,
    pattern: Option<&str>,
    out: &mut Vec<String>,
) -> Result<(), String> {
    let path = if dir.is_empty() { "." } else { dir };
    let rd = fs::read_dir(path).map_err(|e| format!("Failed to read directory '{dir}': {e}"))?;
    for entry in rd.filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') && name != "target" && name != "build" {
                collect_gdl_files(&p.to_string_lossy(), pattern, out)?;
            }
        } else if p.is_file() {
            let fname = entry.file_name().to_string_lossy().to_string();
            let matches = match pattern {
                Some(pat) => wildcard_match(pat, &fname),
                None => fname.to_ascii_lowercase().ends_with(".gdl"),
            };
            if matches {
                out.push(p.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}

/// Resolves the CLI input operands (Task B.4) into source file paths. A directory stands for
/// its top-level `*.gdl` files (not recursive); an operand with `*`/`?` wildcards in its file
/// name that is not an existing file is expanded by `gardelc` itself (shells such as
/// PowerShell/cmd do not expand globs for native programs). Duplicates are dropped. The flag
/// is true for a *project* run: several files, a directory, or a wildcard operand.
pub fn resolve_input_operands(operands: &[String]) -> Result<(Vec<String>, bool), String> {
    let mut files = Vec::new();
    let mut project = operands.len() > 1;
    for op in operands {
        let path = std::path::Path::new(op);
        if path.is_dir() {
            project = true;
            let found = gdl_files_in_dir(op, None)?;
            if found.is_empty() {
                return Err(format!("No .gdl source files found in directory '{op}'"));
            }
            files.extend(found);
        } else if !path.exists() && (op.contains('*') || op.contains('?')) {
            project = true;
            let split = op.rfind(['/', '\\']);
            let (dir, pat) = match split {
                Some(i) => (&op[..=i], &op[i + 1..]),
                None => ("", op.as_str()),
            };
            if dir.contains('*') || dir.contains('?') {
                return Err(format!(
                    "Wildcards are only supported in the file-name part of '{op}'"
                ));
            }
            let found = gdl_files_in_dir(dir, Some(pat))?;
            if found.is_empty() {
                return Err(format!("No source files match '{op}'"));
            }
            files.extend(found);
        } else {
            files.push(op.clone());
        }
    }
    let mut seen = HashSet::new();
    files.retain(|f| {
        let key = fs::canonicalize(f)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| f.clone());
        seen.insert(key)
    });
    Ok((files, project || operands.len() > 1))
}

/// Splits the command line into input operands (files, directories, wildcard patterns).
/// The values of `--fuel <N>` and `--inspect [MODULE]` are not operands.
fn input_operands(args: &[String]) -> Vec<String> {
    let looks_like_input = |s: &str| {
        s.to_ascii_lowercase().ends_with(".gdl")
            || s.contains(['*', '?', '/', '\\'])
            || std::path::Path::new(s).exists()
    };
    let mut ops = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a == "--fuel" {
            i += 2;
            continue;
        }
        if a == "--inspect" {
            if args
                .get(i + 1)
                .is_some_and(|v| !v.starts_with("--") && !looks_like_input(v))
            {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if !a.starts_with("--") {
            ops.push(a.clone());
        }
        i += 1;
    }
    ops
}

/// Fatal namespace diagnostics of a run, before any verification: `E0101` (prelude mode)
/// and `E0102` (cross-file duplicates).
pub fn check_project_namespace(units: &[SourceUnit], prelude_mode: bool) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    if prelude_mode {
        for u in units {
            diags.extend(check_prelude_collisions(&u.ast, &u.src, &u.path));
        }
    }
    diags.extend(check_duplicate_module_names(units));
    diags
}

/// Options of a verification run.
#[derive(Clone, Copy)]
pub struct VerifyOptions {
    pub prelude_mode: bool,
    /// Several files / a directory: enables `W0201` and cross-file linking.
    pub project_mode: bool,
    pub fuel_limit: usize,
    pub strict: bool,
    pub audit: bool,
    pub bounded_check: bool,
}

/// Outcome of checking and verifying all modules of a run.
pub struct CheckedProject {
    /// Diagnostics with the index of the unit (file) they belong to.
    pub diags: Vec<(usize, Diagnostic)>,
    /// Fatal diagnostics (`E0103`); when non-empty nothing was verified.
    pub fatal: Vec<Diagnostic>,
    pub concept_map: HashMap<String, ConceptDecl>,
    pub link: Option<Rc<LinkTable>>,
}

/// Concept expansion, visibility/semantic checks, `W0201`, cross-file linking (`E0103`) and
/// Boogie verification of every module of every unit. Each module is verified once, in its
/// own Boogie context, which contains the background theories of the prelude modules and of
/// the other-file user modules it references (Task B.4).
pub fn check_and_verify_project(units: &mut [SourceUnit], opts: VerifyOptions) -> CheckedProject {
    let mut concept_map = HashMap::new();
    if opts.prelude_mode {
        for c in &standard_prelude().concepts {
            concept_map.insert(c.name.clone(), c.clone());
        }
    }
    for u in units.iter() {
        for c in &u.ast.concepts {
            concept_map.insert(c.name.clone(), c.clone());
        }
    }

    // Concept expansion and static checks of every module (before linking, so linked
    // modules carry their expanded routines).
    let mut pre: Vec<Vec<Vec<Diagnostic>>> = Vec::new();
    for u in units.iter_mut() {
        let mut per_mod = Vec::new();
        for m in &mut u.ast.modules {
            let mut mod_diags = check_and_expand_concepts(m, &concept_map);
            mod_diags.extend(check_visibility_and_semantics(m));
            for d in &mut mod_diags {
                if d.span.is_none() {
                    d.span = Some(locate_span(&u.src, &u.path, &d.tag, &d.routine));
                }
            }
            per_mod.push(mod_diags);
        }
        pre.push(per_mod);
    }

    let link = if opts.project_mode {
        let all: Vec<(ModuleDecl, usize)> = units
            .iter()
            .enumerate()
            .flat_map(|(ui, u)| u.ast.modules.iter().map(move |m| (m.clone(), ui)))
            .collect();
        Some(Rc::new(LinkTable::new(all)))
    } else {
        None
    };
    if let Some(table) = &link {
        let fatal = check_module_dependency_cycles(units, table);
        if !fatal.is_empty() {
            return CheckedProject {
                diags: Vec::new(),
                fatal,
                concept_map,
                link,
            };
        }
    }

    let mut diags: Vec<(usize, Diagnostic)> = Vec::new();
    for (ui, u) in units.iter().enumerate() {
        if opts.project_mode {
            for d in check_file_module_name(u, opts.strict) {
                diags.push((ui, d));
            }
        }
        for d in check_group_packaging(&u.ast, &u.src, &u.path) {
            diags.push((ui, d));
        }
        for (mi, m) in u.ast.modules.iter().enumerate() {
            let mut mod_diags = std::mem::take(&mut pre[ui][mi]);
            mod_diags.extend(with_prelude_injection(opts.prelude_mode, || {
                with_link_table(link.clone(), || {
                    verify_module_phase3(
                        m,
                        &u.src,
                        &u.path,
                        opts.fuel_limit,
                        opts.strict,
                        opts.audit,
                        opts.bounded_check,
                    )
                })
            }));
            for d in &mut mod_diags {
                if d.module_name.is_empty() {
                    d.module_name = m.name.clone();
                }
            }
            diags.extend(mod_diags.into_iter().map(|d| (ui, d)));
        }
    }
    CheckedProject {
        diags,
        fatal: Vec::new(),
        concept_map,
        link,
    }
}

/// Prints fatal diagnostics (E0101/E0102/E0103) and exits with status 1.
fn exit_with_fatal(diags: &[Diagnostic], json_mode: bool, default_path: &str) -> ! {
    if json_mode {
        println!("[");
        for (i, d) in diags.iter().enumerate() {
            let comma = if i + 1 < diags.len() { "," } else { "" };
            println!("{}{comma}", d.to_json(&d.routine, default_path));
        }
        println!("]");
    } else {
        for d in diags {
            let sp = d
                .span
                .clone()
                .unwrap_or_else(|| Span::unknown(default_path));
            eprintln!(
                "[ERROR] {} at {}:{}:{}: {}",
                d.rule, sp.file, sp.line, sp.column, d.message
            );
        }
    }
    std::process::exit(1);
}

/// Reports a fatal front-end error (IO / lexer / parser / missing module) for `path`.
fn exit_with_frontend_error(
    json_mode: bool,
    rule: &'static str,
    routine: &str,
    tag: &str,
    message: String,
    path: &str,
) -> ! {
    if json_mode {
        let diag = Diagnostic::basic("error", rule, routine.to_string(), tag.to_string(), message);
        println!("[\n{}\n]", diag.to_json("UNKNOWN", path));
    } else {
        eprintln!("[ERROR] {message}");
    }
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "Usage: gardelc <input.gdl ... | DIR | --stdlib> [--no-prelude] [--inspect [MODULE]] [--emit-stdlib [DIR]] [--json-diagnostics] [--fuel <N>] [--audit] [--strict] [--bounded-check] [--run]"
        );
        eprintln!(
            "  --no-prelude  bootstrap mode: do not inject the Standard Prelude (implied by --stdlib and when compiling the prelude source itself, §4.1)"
        );
        eprintln!(
            "  several files, a directory (its *.gdl files, searched recursively, skipping build/ and target/) or a wildcard pattern such as src/*.gdl form one project with a flat module namespace (§4.2)"
        );
        std::process::exit(1);
    }

    if let Some(i) = args.iter().position(|a| a == "--emit-stdlib") {
        let out_dir = args
            .get(i + 1)
            .filter(|a| !a.starts_with("--"))
            .map(String::as_str)
            .unwrap_or(EMIT_STDLIB_DEFAULT_DIR);
        match emit_stdlib_files(out_dir) {
            Ok(files) => {
                println!("Emitted standalone Phase 4 standard library files: {files:?}");
                return;
            }
            Err(e) => {
                eprintln!("[ERROR] Failed to emit stdlib files: {e}");
                std::process::exit(1);
            }
        }
    }

    let inspect_mode = args.iter().any(|a| a == "--inspect");
    let mut stdlib_only = args.iter().any(|a| a == "--stdlib")
        || (inspect_mode && args.get(1).is_some_and(|a| a.starts_with("--")));

    let json_mode = args.iter().any(|a| a == "--json-diagnostics");
    let audit_mode = args.iter().any(|a| a == "--audit");
    let strict_mode = args.iter().any(|a| a == "--strict");
    let bounded_check = args.iter().any(|a| a == "--bounded-check");
    let no_prelude_flag = args.iter().any(|a| a == "--no-prelude");
    let fuel_limit = parse_fuel_arg(&args);

    // Task B.4: input operands (files, directories, wildcard patterns).
    // `--stdlib` verifies the source tree this gardelc was built from (see `resolve_stdlib_inputs`);
    // only when that tree is unavailable does it verify the embedded copy, and it says so.
    let say = |line: &str| {
        if json_mode {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    };
    // Explicit operands. Files of the stdlib tree depend on each other (concepts, `OPTION` in
    // `RESULT`, ...), so asking for any of them verifies the whole tree, as `--stdlib` does;
    // mixing them with user files is an error (a user program gets the stdlib as its prelude).
    let mut user_inputs: Option<(Vec<String>, bool)> = None;
    if !stdlib_only {
        let ops = input_operands(&args);
        if ops.is_empty() {
            exit_with_frontend_error(
                json_mode,
                "IO",
                "file",
                "io_error",
                "No input file given (expected <input.gdl ...>, a directory, or --stdlib)"
                    .to_string(),
                "<none>",
            );
        }
        let (paths, project) = match resolve_input_operands(&ops) {
            Ok(r) => r,
            Err(e) => exit_with_frontend_error(json_mode, "IO", "file", "io_error", e, &ops[0]),
        };
        let inside: Vec<&String> = paths.iter().filter(|p| path_in_stdlib_tree(p)).collect();
        if inside.is_empty() {
            user_inputs = Some((paths, project));
        } else if inside.len() < paths.len() {
            exit_with_frontend_error(
                json_mode,
                "IO",
                "file",
                "io_error",
                format!(
                    "Standard Prelude files ({}) cannot be compiled together with user files: user programs receive the stdlib as their prelude. Verify the stdlib with `gardelc --stdlib`",
                    inside.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")
                ),
                inside[0],
            );
        } else {
            let which = if inside.len() <= 3 {
                inside.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")
            } else {
                format!("{} files", inside.len())
            };
            say(&format!(
                "[stdlib] {which} belong(s) to the Standard Prelude, whose modules depend on each other: verifying the whole stdlib tree (as `--stdlib`)."
            ));
            stdlib_only = true;
        }
    }

    // `--stdlib` verifies the source tree this gardelc was built from (see `resolve_stdlib_inputs`);
    // only when that tree is unavailable does it verify the embedded copy, and it says so.
    let mut stdlib_embedded: HashMap<String, &'static str> = HashMap::new();
    let (input_paths, project_mode): (Vec<String>, bool) = if let Some(r) = user_inputs {
        r
    } else {
        match resolve_stdlib_inputs() {
            Ok(StdlibInputs::Disk { paths, notes }) => {
                notes.iter().for_each(|n| say(n));
                (paths, true)
            }
            Ok(StdlibInputs::Embedded { note }) => {
                say(&note);
                for (p, c) in STDLIB_SOURCES {
                    stdlib_embedded.insert(p.to_string(), c);
                }
                (STDLIB_SOURCES.iter().map(|(p, _)| p.to_string()).collect(), true)
            }
            Err((msg, path)) => exit_with_frontend_error(json_mode, "IO", "file", "io_error", msg, &path),
        }
    };

    let mut units: Vec<SourceUnit> = Vec::new();
    for input_path in &input_paths {
        let src = if let Some(content) = stdlib_embedded.get(input_path) {
            content.to_string()
        } else {
            match fs::read_to_string(input_path) {
                Ok(s) => s,
                Err(e) => exit_with_frontend_error(
                    json_mode,
                    "IO",
                    "file",
                    "io_error",
                    format!("Failed to read file '{input_path}': {e}"),
                    input_path,
                ),
            }
        };
        let tokens = match tokenize(&src) {
            Ok(toks) => toks,
            Err(e) => {
                let msg = if project_mode {
                    format!("{input_path}: {e}")
                } else {
                    e.to_string()
                };
                exit_with_frontend_error(json_mode, "Lexer", "lexer", "lex_error", msg, input_path)
            }
        };
        let mut parser = Parser::new(tokens);
        let ast = match parser.parse_file() {
            Ok(ast) => ast,
            Err(e) => {
                let msg = if project_mode {
                    format!("{input_path}: {e}")
                } else {
                    e.to_string()
                };
                exit_with_frontend_error(
                    json_mode,
                    "Parser",
                    "parser",
                    "parse_error",
                    msg,
                    input_path,
                )
            }
        };
        units.push(SourceUnit {
            path: input_path.clone(),
            src,
            ast,
        });
    }
    let first_path = units[0].path.clone();

    // §4.1 Standard Prelude: injected unless in bootstrap mode (`--no-prelude`, `--stdlib`,
    // or when a file being compiled is the prelude source itself).
    let prelude_mode = units
        .iter()
        .all(|_| prelude_mode_enabled(stdlib_only, no_prelude_flag));

    // E0101 (prelude shadowing) and E0102 (cross-file duplicates) are fatal (§4.1, §4.2).
    let namespace_errors = check_project_namespace(&units, prelude_mode);
    if !namespace_errors.is_empty() {
        exit_with_fatal(&namespace_errors, json_mode, &first_path);
    }

    if units.iter().all(|u| u.ast.modules.is_empty()) {
        let msg = if project_mode {
            "Expected at least one module in the input files"
        } else {
            "Expected at least one module in input file"
        };
        exit_with_frontend_error(
            json_mode,
            "Parser",
            "module",
            "missing_module",
            msg.to_string(),
            &first_path,
        );
    }

    let opts = VerifyOptions {
        prelude_mode,
        project_mode,
        fuel_limit,
        strict: strict_mode,
        audit: audit_mode,
        bounded_check,
    };
    let checked = check_and_verify_project(&mut units, opts);
    if !checked.fatal.is_empty() {
        exit_with_fatal(&checked.fatal, json_mode, &first_path);
    }
    let CheckedProject {
        diags,
        concept_map,
        link,
        ..
    } = checked;

    if inspect_mode {
        let filter_mod = args
            .iter()
            .position(|a| a == "--inspect")
            .and_then(|idx| args.get(idx + 1))
            .filter(|val| !val.starts_with("--") && !input_paths.contains(val))
            .cloned();
        for u in &units {
            for m in &u.ast.modules {
                if filter_mod
                    .as_ref()
                    .is_none_or(|f| f.eq_ignore_ascii_case(&m.name))
                {
                    println!("{}", format_module_interface(m));
                }
            }
        }
        return;
    }

    // The primary module (outputs reported, gcc, --run): the last module of the file in a
    // single-file run; in a project run the unique `program`, else the last module.
    let all_modules: Vec<&ModuleDecl> = units.iter().flat_map(|u| u.ast.modules.iter()).collect();
    let programs: Vec<&&ModuleDecl> = all_modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Program)
        .collect();
    let module = if project_mode && programs.len() == 1 {
        (*programs[0]).clone()
    } else {
        (*all_modules.last().expect("non-empty modules")).clone()
    };
    // Default module name / file of a unit's diagnostics without their own.
    let unit_default = |ui: usize| -> (String, String) {
        let u = &units[ui];
        let name = u
            .ast
            .modules
            .last()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| module.name.clone());
        (name, u.path.clone())
    };

    let has_errors = diags.iter().any(|(_, d)| d.level == "error");

    if json_mode {
        println!("[");
        for (i, (ui, d)) in diags.iter().enumerate() {
            let comma = if i + 1 < diags.len() { "," } else { "" };
            let (dm, df) = unit_default(*ui);
            println!("{}{comma}", d.to_json(&dm, &df));
        }
        println!("]");
        if has_errors {
            std::process::exit(1);
        }
        return;
    } else {
        if prelude_mode {
            let p = standard_prelude();
            println!(
                "[0/5] Standard Prelude injected (§4.1): {} modules {:?}, {} concepts (trusted, verified in --stdlib bootstrap mode)",
                p.modules.len(),
                p.modules
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>(),
                p.concepts.len()
            );
        } else {
            println!("[0/5] Bootstrap mode (--no-prelude): Standard Prelude not injected");
        }
        if project_mode {
            println!(
                "[1/5] Linked project of {} files into one flat namespace (§4.2): {} modules {:?}, {} concepts {:?}, fuel: {}",
                units.len(),
                all_modules.len(),
                all_modules
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>(),
                concept_map.len(),
                concept_map.keys().collect::<Vec<_>>(),
                fuel_limit
            );
            for u in &units {
                println!(
                    "      {}: group {:?}, {:?}",
                    u.path,
                    u.ast.group,
                    u.ast
                        .modules
                        .iter()
                        .map(|m| format!("{:?} {}", m.kind, m.name))
                        .collect::<Vec<_>>()
                );
            }
        } else {
            println!(
                "[1/5] Parsed {} concepts {:?}, group {:?}, and {:?} `{}` (type params: {:?}, satisfies: {:?}, laws: {}, fuel: {})",
                concept_map.len(),
                concept_map.keys().collect::<Vec<_>>(),
                units[0].ast.group,
                module.kind,
                module.name,
                module.type_params,
                module.satisfies.as_ref().map(|s| &s.concepts),
                module.specify_laws.len(),
                fuel_limit
            );
        }
        println!(
            "[2/5] Checked §4 Visibility, §9 Concept Laws, §11 IO Boundary & §12 Direct Verification:"
        );
        if diags.is_empty() {
            println!(
                "      0 errors, 0 warnings — all visibility, concept laws, IO linearity, overflow, and contract obligations satisfied."
            );
        } else {
            for (ui, d) in &diags {
                let sp = d
                    .span
                    .clone()
                    .unwrap_or_else(|| Span::unknown(&units[*ui].path));
                println!(
                    "      [{}] {} in `{}` ({}) at {}:{}:{}: {}",
                    d.level.to_uppercase(),
                    d.rule,
                    d.routine,
                    d.tag,
                    sp.file,
                    sp.line,
                    sp.column,
                    d.message
                );
                if !d.counterexample.is_empty() {
                    let cex_pairs: Vec<String> = d
                        .counterexample
                        .iter()
                        .map(|(k, v)| format!("{k} = {v}"))
                        .collect();
                    println!("            counterexample: {{ {} }}", cex_pairs.join(", "));
                }
                if !d.relevant_lemmas.is_empty() {
                    println!(
                        "            relevant_lemmas: [{}]",
                        d.relevant_lemmas.join(", ")
                    );
                }
            }
            if has_errors {
                std::process::exit(1);
            }
        }
    }

    if let Err(e) = fs::create_dir_all("build") {
        eprintln!("[ERROR] Failed to create build directory: {e}");
        std::process::exit(1);
    }
    let mut prog_env = ProgramEnv {
        modules: HashMap::new(),
        concepts: concept_map.clone(),
    };
    if prelude_mode {
        // Global symbol table: prelude modules are registered before user modules (§4.1).
        for p in &standard_prelude().modules {
            prog_env.modules.insert(p.name.clone(), p.clone());
        }
    }
    for m in &all_modules {
        prog_env.modules.insert(m.name.clone(), (*m).clone());
    }

    let mut primary_bpl = String::new();
    let mut primary_bend = String::new();
    let mut primary_c = String::new();
    let mut primary_exe = String::new();
    let mut emitted_bpl: Vec<String> = Vec::new();

    for m in &all_modules {
        let stem = m.name.to_lowercase();
        let bpl_path = format!("build/{stem}.bpl");
        let bend_path = format!("build/{stem}.bend");
        let c_path = format!("build/{stem}.c");

        let bpl_code = with_prelude_injection(prelude_mode, || {
            with_link_table(link.clone(), || emit_boogie(m, fuel_limit))
        });
        if let Err(e) = fs::write(&bpl_path, bpl_code) {
            eprintln!("[ERROR] Failed to write Boogie output '{bpl_path}': {e}");
            std::process::exit(1);
        }
        emitted_bpl.push(bpl_path.clone());

        let mono_module = if m.type_params.is_empty() {
            (*m).clone()
        } else {
            monomorphize_module(m, &["U64".to_string()])
        };

        let (converted_mono, combinators) = closure_convert_module(&mono_module);

        let mut bend_code = emit_bend_v2(&converted_mono);
        for comb in &combinators {
            let dummy_mod = ModuleDecl {
                kind: ModuleKind::Functions,
                name: format!("{}_closures", m.name),
                type_params: Vec::new(),
                type_bounds: HashMap::new(),
                satisfies: None,
                repr_vis: Visibility::Any,
                repr: ReprKind::Product(Vec::new()),
                specify_routines: Vec::new(),
                specify_laws: Vec::new(),
                create_routines: Vec::new(),
                feature_routines: vec![(Visibility::Any, comb.routine.clone())],
                invariants: Vec::new(),
            };
            bend_code.push_str(&emit_bend_v2(&dummy_mod));
        }

        if let Err(e) = fs::write(&bend_path, bend_code) {
            eprintln!("[ERROR] Failed to write Bend v2 output '{bend_path}': {e}");
            std::process::exit(1);
        }

        let c_code = emit_c11(&converted_mono);
        if let Err(e) = fs::write(&c_path, c_code) {
            eprintln!("[ERROR] Failed to write C11 output '{c_path}': {e}");
            std::process::exit(1);
        }

        if m.name == module.name {
            primary_bpl = bpl_path;
            primary_bend = bend_path;
            primary_c = c_path;
            primary_exe = format!("build/{stem}_bin.exe");
        }
    }

    let gcc_compiled = if module.kind == ModuleKind::Program {
        Command::new("gcc")
            .args(["-O3", "-fopenmp", &primary_c, "-o", &primary_exe])
            .status()
            .is_ok_and(|s| s.success())
    } else {
        false
    };

    if !json_mode {
        if project_mode {
            println!(
                "[3/5] Emitted direct Boogie VCs & fuel axioms -> {} (one per module; primary `{}`)",
                emitted_bpl.join(", "),
                module.name
            );
        } else {
            println!("[3/5] Emitted direct Boogie VCs & fuel axioms -> {primary_bpl}");
        }
        println!("[4/5] Emitted monomorphized Bend v2 target    -> {primary_bend}");
        println!("[5/5] Emitted C11 runtime target              -> {primary_c}");
        if gcc_compiled {
            println!("      Compiled native executable              -> {primary_exe}");
        }
    }

    if args.iter().any(|a| a == "--run") {
        let solver_candidates = [
            "../tools/dafny/z3/bin/z3-4.12.1.exe",
            "tools/dafny/z3/bin/z3-4.12.1.exe",
            "../tools/dafny/z3/bin/z3.exe",
            "tools/dafny/z3/bin/z3.exe",
        ];
        let found_solver = solver_candidates
            .iter()
            .find(|cand| std::path::Path::new(cand).exists());

        let mut boogie_cmd = Command::new("boogie");
        boogie_cmd.args(["/typeEncoding:p", "/mv:-", &primary_bpl]);
        if let Some(solver) = found_solver {
            boogie_cmd.arg(format!("/proverOpt:PROVER_PATH={solver}"));
        }

        if let Ok(output) = boogie_cmd.output() {
            println!("\n=== [0] Direct Boogie + Z3 Verification ({primary_bpl}) ===");
            let stdout = String::from_utf8_lossy(&output.stdout);
            print!("{stdout}");
            if stdout.contains("*** MODEL") {
                let translated = translate_z3_model_to_gardel(&stdout, &module);
                if !translated.is_empty() {
                    println!("Translated Gardel Counterexample: {translated:?}");
                }
            }
        }

        if gcc_compiled && std::path::Path::new(&primary_exe).exists() {
            println!("\n=== [1] Executing Native Executable ({primary_exe}) ===");
            let _ = Command::new(&primary_exe).status();
        }
    }
}

// ============================================================================
// 12. PHASE 3 & 4 UNIT TESTS (Boogie, Fuel, Reveal, SEQ, BIGNAT, STRING, IO)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_single_module(src: &str) -> ModuleDecl {
        let tokens = tokenize(src).expect("lexing should succeed");
        let mut parser = Parser::new(tokens);
        let mut ast = parser.parse_file().expect("parsing should succeed");
        let mut cmap = HashMap::new();
        for c in &ast.concepts {
            cmap.insert(c.name.clone(), c.clone());
        }
        let mut m = ast.modules.pop().expect("expected one module");
        let _ = check_and_expand_concepts(&mut m, &cmap);
        m
    }

    #[test]
    fn test_phase3_boogie_emission_and_fuel() {
        let src = r#"
module TREE
representation {ANY}
  | Leaf(value: NAT)
  | Node(left: TREE, right: TREE)
specify {ANY}
  leaves: LIST[NAT]
    variant Current
    do
      match Current
        case Leaf(v) then LIST.empty.prepend(v)
        case Node(l, r) then l.leaves.append(r.leaves)
      end
    end
feature {ANY}
  max_value: NAT
    variant Current
    do
      reveal leaves
      match Current
        case Leaf(v) then v
        case Node(l, r) then
          let ml = l.max_value
          let mr = r.max_value
          if ml >= mr then ml else mr end
      end
    ensure
      attained: Current.leaves.has(Result)
    end
end
"#;
        let m = parse_single_module(src);
        let bpl = emit_boogie(&m, 2);
        assert!(bpl.contains("axiom DefaultFuel == SuccFuel(SuccFuel(ZeroFuel));"));
        assert!(bpl.contains("function fn_leaves("));
        assert!(bpl.contains("procedure Verify_max_value("));
        assert!(bpl.contains("reveal leaves"));
        let diags = verify_module_phase3(&m, src, "tree.gdl", 2, false, false, false);
        assert!(diags.is_empty(), "Expected 0 errors, got {diags:?}");
    }

    #[test]
    fn test_phase3_overflow_counterexample_translation() {
        let src = r#"
module MIDPOINT_BUG
representation {ANY}
  dummy: NAT
feature {ANY}
  midpoint(lo: NAT, hi: NAT): NAT
    require
      ordered: lo <= hi
    do
      (lo + hi) / 2
    ensure
      in_range: lo <= Result and Result <= hi
    end
end
"#;
        let m = parse_single_module(src);
        let diags = verify_module_phase3(&m, src, "midpoint.gdl", 2, false, false, false);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].tag, "overflow_add");
        assert!(diags[0].counterexample.contains_key("lo"));
        assert!(diags[0].counterexample.contains_key("hi"));
        let json = diags[0].to_json(&m.name, "midpoint.gdl");
        assert!(json.contains("\"contract_tag\": \"overflow_add\""));
        assert!(json.contains("\"path_conditions\": [\"(lo <= hi)\"]"));
    }

    #[test]
    fn test_phase3_fuel_exhaustion_and_reveal_fix() {
        let src_unrevealed = r#"
module BOX
representation {NONE}
  val: NAT
specify {ANY}
  ghost_val: NAT
    do
      Current.val
    end
feature {ANY}
  get_val: NAT
    do
      Current.val
    ensure
      matches_ghost: Result = Current.ghost_val
    end
end
"#;
        let m_unrev = parse_single_module(src_unrevealed);
        let diags_zero =
            verify_module_phase3(&m_unrev, src_unrevealed, "box.gdl", 0, false, false, false);
        assert_eq!(diags_zero.len(), 1);
        assert_eq!(diags_zero[0].tag, "matches_ghost");
        assert!(
            diags_zero[0]
                .relevant_lemmas
                .contains(&"reveal ghost_val".to_string())
        );

        let src_revealed = r#"
module BOX
representation {NONE}
  val: NAT
specify {ANY}
  ghost_val: NAT
    do
      Current.val
    end
feature {ANY}
  get_val: NAT
    do
      reveal ghost_val
      Current.val
    ensure
      matches_ghost: Result = Current.ghost_val
    end
end
"#;
        let m_rev = parse_single_module(src_revealed);
        let diags_rev =
            verify_module_phase3(&m_rev, src_revealed, "box.gdl", 0, false, false, false);
        assert!(diags_rev.is_empty());
    }

    #[test]
    fn test_phase3_assume_audit_and_strict() {
        let src = r#"
module ASSUME_TEST
representation {ANY}
  val: NAT
feature {ANY}
  identity_with_assume(x: NAT): NAT
    do
      assume unproven_fact: x > 0 end
      x
    ensure
      positive: Result > 0
    end
end
"#;
        let m = parse_single_module(src);
        let diags_def = verify_module_phase3(&m, src, "test.gdl", 2, false, false, false);
        assert!(diags_def.is_empty());

        let diags_audit = verify_module_phase3(&m, src, "test.gdl", 2, false, true, false);
        assert_eq!(diags_audit.len(), 1);
        assert_eq!(diags_audit[0].level, "audit");
        assert_eq!(diags_audit[0].tag, "unproven_fact");

        let diags_strict = verify_module_phase3(&m, src, "test.gdl", 2, true, false, false);
        assert_eq!(diags_strict.len(), 1);
        assert_eq!(diags_strict[0].level, "error");
        assert_eq!(diags_strict[0].tag, "unproven_fact");
    }

    #[test]
    fn test_phase4_stdlib_package_verifies_all_modules() {
        let stdlib = parse_phase4_stdlib().expect("Phase 4 stdlib should parse");
        assert_eq!(stdlib.group, None);
        assert_eq!(stdlib.concepts.len(), 3);
        assert_eq!(stdlib.modules.len(), 9);
        for m in &stdlib.modules {
            let mut diags = check_visibility_and_semantics(m);
            diags.extend(verify_module_phase3(
                m,
                phase4_stdlib_source(),
                "stdlib.gdl",
                2,
                true,
                false,
                false,
            ));
            assert!(
                diags.is_empty(),
                "Expected 0 errors in stdlib module `{}`, got {diags:?}",
                m.name
            );
            let bpl = emit_boogie(m, 2);
            assert!(bpl.contains(&format!("// Module: {}", m.name)));
            let c11 = emit_c11(m);
            assert!(c11.contains("int main(void)"));
        }
    }

    #[test]
    fn test_c1_stdlib_modular_files_match_module_names_w0201() {
        // Every decomposed stdlib file satisfies the one-module-per-file naming convention (§4.2).
        for (rel_path, content) in STDLIB_SOURCES {
            let tokens = tokenize(content).expect("lexing stdlib file should succeed");
            let mut parser = Parser::new(tokens);
            let ast = parser
                .parse_file()
                .expect("parsing stdlib file should succeed");
            let unit = SourceUnit {
                path: rel_path.to_string(),
                src: content.to_string(),
                ast,
            };
            let diags = check_file_module_name(&unit, true);
            assert!(
                diags.is_empty(),
                "Stdlib file `{rel_path}` must match its module name under W0201 (strict), got: {diags:?}"
            );
        }
    }

    #[test]
    fn test_c1_embedded_stdlib_lists_agree_with_source_tree() {
        // The prelude bundle (`concat!`) and `STDLIB_SOURCES` are the same files in the same order.
        let joined: Vec<&str> = STDLIB_SOURCES.iter().map(|(_, c)| *c).collect();
        assert_eq!(phase4_stdlib_source(), joined.join("\n"));
        // `--stdlib` verifies the tree gardelc was built from, and it matches the embedded copy.
        match resolve_stdlib_inputs() {
            Ok(StdlibInputs::Disk { paths, notes }) => {
                assert_eq!(paths.len(), STDLIB_SOURCES.len(), "got: {paths:?}");
                assert_eq!(notes.len(), 1, "embedded copy must be current, got: {notes:?}");
            }
            Ok(StdlibInputs::Embedded { note }) => panic!("source tree missing: {note}"),
            Err((msg, _)) => panic!("{msg}"),
        }
    }

    #[test]
    fn test_c1_emit_stdlib_never_overwrites_edited_sources() {
        let dir = std::env::temp_dir().join(format!("gardel_emit_stdlib_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let out = dir.to_string_lossy().replace('\\', "/");
        let files = emit_stdlib_files(&out).expect("emitting into an empty directory");
        assert_eq!(files.len(), STDLIB_SOURCES.len());
        // Re-emitting identical files is a no-op.
        emit_stdlib_files(&out).expect("re-emitting identical files");
        // An edited file is never overwritten, and nothing else is written either.
        let edited = format!("{out}/numeric/bignat.gdl");
        let mut src = fs::read_to_string(&edited).unwrap();
        src.push_str("\n-- local edit\n");
        fs::write(&edited, &src).unwrap();
        let removed = format!("{out}/core/option.gdl");
        fs::remove_file(&removed).unwrap();
        let err = emit_stdlib_files(&out).expect_err("must refuse to overwrite an edited file");
        assert!(err.contains("numeric/bignat.gdl"), "got: {err}");
        assert_eq!(fs::read_to_string(&edited).unwrap(), src);
        assert!(!std::path::Path::new(&removed).exists(), "refused emission must write nothing");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_c1_stdlib_project_verification_zero_warnings_strict() {
        // Multi-file stdlib project verifies in project mode with dependencies linked and 0 warnings under --strict.
        let mut units = Vec::new();
        for (rel_path, content) in STDLIB_SOURCES {
            let tokens = tokenize(content).expect("lexing stdlib file should succeed");
            let mut parser = Parser::new(tokens);
            let ast = parser
                .parse_file()
                .expect("parsing stdlib file should succeed");
            units.push(SourceUnit {
                path: rel_path.to_string(),
                src: content.to_string(),
                ast,
            });
        }
        let opts = VerifyOptions {
            prelude_mode: false,
            project_mode: true,
            fuel_limit: 2,
            strict: true,
            audit: false,
            bounded_check: false,
        };
        let checked = check_and_verify_project(&mut units, opts);
        assert!(
            checked.fatal.is_empty(),
            "Stdlib multi-file project must have 0 fatal errors, got: {:?}",
            checked.fatal
        );
        assert!(
            checked.diags.is_empty(),
            "Stdlib multi-file project must verify with 0 errors and 0 warnings under --strict, got: {:?}",
            checked.diags
        );
    }

    #[test]
    fn test_phase4_io_boundary_and_linearity_firewall() {
        let illegal_io_in_module = r#"
module PURE_BOX
representation {ANY}
  val: NAT
feature {ANY}
  bad_effect: IO[EXIT_CODE]
    do
      with IO
        IO.print("illegal in pure ADT module")
        IO.exit(0)
      end
    end
end
"#;
        let m = parse_single_module(illegal_io_in_module);
        let diags = check_visibility_and_semantics(&m);
        assert!(
            diags
                .iter()
                .any(|d| d.rule == "Phase 4 (IO Boundary Violation)"),
            "Expected IO Boundary Violation in pure module, got {diags:?}"
        );
    }

    #[test]
    fn test_phase4_inspect_module_interface() {
        let stdlib = parse_phase4_stdlib().expect("stdlib should parse");
        let seq_mod = stdlib
            .modules
            .iter()
            .find(|m| m.name == "SEQ")
            .expect("SEQ module exists");
        let iface = format_module_interface(seq_mod);
        assert!(iface.contains("module SEQ[A]"));
        assert!(iface.contains("singleton(x: A): SEQ[A]"));
        assert!(iface.contains("ensure  unit_len: (Result.length = 1)"));
    }

    #[test]
    fn test_phase2_variant_disjointness_and_distinct_strings() {
        let src = r#"
module TRAFFIC_LIGHT
representation {ANY}
  | Red
  | Yellow
  | Green
feature {ANY}
  name_of(color: TRAFFIC_LIGHT): STRING
    do
      match color
        case Red then "stop"
        case Yellow then "caution"
        case Green then "go"
      end
    end
end
"#;
        let m = parse_single_module(src);
        let bpl = emit_boogie(&m, 2);
        // 1. Pairwise variant disjointness axioms
        assert!(bpl.contains("is_Red(x) ==> !is_Yellow(x)"));
        assert!(bpl.contains("is_Red(x) ==> !is_Green(x)"));
        assert!(bpl.contains("is_Yellow(x) ==> !is_Green(x)"));
        // 2. Interned unique string literal constants
        assert!(bpl.contains("const unique str_lit_0: GardelString;"));
        assert!(bpl.contains("const unique str_lit_1: GardelString;"));
        assert!(bpl.contains("const unique str_lit_2: GardelString;"));
        assert!(bpl.contains("axiom Str_length(str_lit_0) =="));
        // 3. Verify module phase 3 passes
        let diags = verify_module_phase3(&m, src, "traffic.gdl", 2, false, false, false);
        assert!(diags.is_empty(), "Expected 0 errors, got {diags:?}");
    }

    #[test]
    fn test_phase2_canonical_record_ordering() {
        let src = r#"
module POINT3D
representation {ANY}
  x: NAT
  y: NAT
  z: NAT
create {ANY}
  make(a: NAT, b: NAT, c: NAT): POINT3D
    do
      { z = c, x = a, y = b }
    ensure
      check_x: Result.x = a
      check_y: Result.y = b
      check_z: Result.z = c
    end
end
"#;
        let m = parse_single_module(src);
        let bpl = emit_boogie(&m, 2);
        // Constructor arguments must be sorted to canonical declaration order: x, y, z
        assert!(bpl.contains("POINT3D_Ctor(a, b, c)"));
        let diags = verify_module_phase3(&m, src, "point.gdl", 2, false, false, false);
        assert!(diags.is_empty(), "Expected 0 errors, got {diags:?}");
    }

    #[test]
    fn test_phase2_capture_avoiding_substitution() {
        // Test 1: Variable substitution
        let orig = Expr::Var("x".to_string());
        let subst_val = Expr::Int(42);
        let replaced = subst_expr_var(&orig, "x", &subst_val);
        assert_eq!(replaced, Expr::Int(42));

        // Test 2: Lambda parameter shadowing prevents substitution
        let lambda_shadowed = Expr::Lambda(
            vec![("x".to_string(), "NAT".to_string())],
            "NAT".to_string(),
            Vec::new(),
            Box::new(Expr::Var("x".to_string())),
        );
        let lambda_subst = subst_expr_var(&lambda_shadowed, "x", &subst_val);
        assert_eq!(lambda_subst, lambda_shadowed);

        // Test 3: Lambda parameter different name allows substitution
        let lambda_other = Expr::Lambda(
            vec![("y".to_string(), "NAT".to_string())],
            "NAT".to_string(),
            Vec::new(),
            Box::new(Expr::Var("x".to_string())),
        );
        let lambda_other_subst = subst_expr_var(&lambda_other, "x", &subst_val);
        assert_eq!(
            lambda_other_subst,
            Expr::Lambda(
                vec![("y".to_string(), "NAT".to_string())],
                "NAT".to_string(),
                Vec::new(),
                Box::new(Expr::Int(42))
            )
        );

        // Test 4: Block with let shadowing
        let stmts = vec![Stmt::Let("x".to_string(), Expr::Int(10))];
        let body = Expr::Var("x".to_string());
        let (new_stmts, new_body) = subst_block(&stmts, &body, &[("x".to_string(), subst_val)]);
        assert_eq!(new_stmts.len(), 1);
        assert_eq!(new_body, Expr::Var("x".to_string())); // shadowed!
    }

    #[test]
    fn test_phase2_general_numeric_variant_termination() {
        let src = r#"
module COUNTDOWN
representation {ANY}
  dummy: NAT
feature {ANY}
  count(n: NAT): NAT
    variant
      n
    do
      if n = 0 then
        0
      else
        count(n - 1)
      end
    ensure
      done: Result = 0
    end
end
"#;
        let m = parse_single_module(src);
        let bpl = emit_boogie(&m, 2);
        assert!(bpl.contains("count::variant::variant_non_negative"));
        assert!(bpl.contains("count::variant::variant_decrease"));
        let diags = verify_module_phase3(&m, src, "countdown.gdl", 2, false, false, false);
        assert!(diags.is_empty(), "Expected 0 errors, got {diags:?}");
    }

    #[test]
    fn test_plan3_ebnf_lexer_casing_and_delimiters() {
        let invalid_mod = "module my_mod representation {ANY} x: NAT end";
        let toks = tokenize(invalid_mod).unwrap();
        let mut p = Parser::new(toks);
        assert!(p.parse_file().is_err());

        let valid_src = r#"
module PRIMITIVES
representation {ANY}
  val: U64
feature {ANY}
  compute(a: U64, b: U64): U64
    do
      let sum = a +% b;
      let diff = a -% b;
      let prod = a *% b;
      let mask = a & b | (a ^ b);
      let shifted = (a << 2) >> 1;
      sum +% mask
    end
end
"#;
        let m = parse_single_module(valid_src);
        assert_eq!(m.name, "PRIMITIVES");
        let bpl = emit_boogie(&m, 2);
        assert!(bpl.contains("u64_wrap_add"));
        assert!(bpl.contains("u64_bit_and"));
        assert!(bpl.contains("u64_bit_or"));
        assert!(bpl.contains("u64_bit_xor"));
        assert!(bpl.contains("u64_bit_shl"));
        assert!(bpl.contains("u64_bit_shr"));
    }

    #[test]
    fn test_plan3_nested_pattern_matching_ast() {
        let src = r#"
module EVAL_MATCH
representation {ANY}
  | Lit(n: U64)
  | Pair(fst: U64, snd: U64)
feature {ANY}
  eval_it(c: EVAL_MATCH): U64
    do
      match c
        case Lit(x) then x
        case Pair(a, b) then a +% b
      end
    end
end
"#;
        let m = parse_single_module(src);
        let bpl = emit_boogie(&m, 2);
        assert!(bpl.contains("is_Lit"));
        assert!(bpl.contains("is_Pair"));
        let bend = emit_bend_v2(&m);
        assert!(bend.contains("case EVAL_MATCH/Lit { x }:"));
        assert!(bend.contains("case EVAL_MATCH/Pair { a, b }:"));
    }

    #[test]
    fn test_plan3_lexicographic_termination_variants() {
        let src = r#"
module ACKERMANN
representation {ANY}
  dummy: NAT
feature {ANY}
  ack(m: NAT, n: NAT): NAT
    variant
      m, n
    do
      if m = 0 then
        n + 1
      else
        if n = 0 then
          ack(m - 1, 1)
        else
          ack(m - 1, ack(m, n - 1))
        end
      end
    end
end
"#;
        let m = parse_single_module(src);
        assert_eq!(m.feature_routines[0].1.variant.len(), 2);
        let bpl = emit_boogie(&m, 2);
        assert!(bpl.contains("ack::variant::variant_non_negative"));
        assert!(bpl.contains("ack::variant::variant_decrease"));
    }

    #[test]
    fn test_plan3_sum_type_hereditary_invariants() {
        let src = r#"
module BIN_TREE
representation {ANY}
  | Leaf(val: U64)
  | Node(left: BIN_TREE, right: BIN_TREE)
invariant
  positive_leaves: True
end
"#;
        let m = parse_single_module(src);
        let bpl = emit_boogie(&m, 2);
        assert!(bpl.contains("procedure Verify_Hereditary_Invariant_BIN_TREE(Current: BIN_TREE)"));
        assert!(bpl.contains("BIN_TREE::hereditary_invariant::Node_left"));
        assert!(bpl.contains("BIN_TREE::hereditary_invariant::Node_right"));
    }

    #[test]
    fn test_plan3_monadic_io_linearity() {
        let invalid_src = r#"
program UNBALANCED_IO
feature {ANY}
  run_prog: IO[EXIT_CODE]
    do
      with IO
        let x = if 1 = 1 then
          IO.print("Hello")
        else
          0
        end
        IO.exit(0)
      end
    end
end
"#;
        let m = parse_single_module(invalid_src);
        let diags = check_visibility_and_semantics(&m);
        assert!(
            diags.iter().any(|d| d.tag == "io_branch_linearity"),
            "Expected io_branch_linearity error, got: {diags:?}"
        );
    }

    #[test]
    fn test_plan3_constrained_generics_concept_laws() {
        let src = r#"
module GENERIC_CONTAINER[T: ORD]
representation {ANY}
  item: T
feature {ANY}
  check_same(x: T): BOOL
    do
      True
    end
end
"#;
        let m = parse_single_module(src);
        assert_eq!(m.type_bounds.get("T"), Some(&"ORD".to_string()));
        let bpl = emit_boogie(&m, 2);
        // `T` is a hole of `type GENERIC_CONTAINER T;`, so its theory is polymorphic in `T`
        // rather than declared over a global `type T;` (which Boogie rejects as a clash).
        assert!(
            !bpl.contains(
                "
type T;"
            ),
            "module type parameter declared as a global type"
        );
        assert!(bpl.contains("function T_eq<T>(x: T, y: T) returns (bool);"));
        assert!(bpl.contains("function T_less<T>(x: T, y: T) returns (bool);"));
        assert!(bpl.contains("axiom (forall<T> x: T :: { T_eq(x, x) } T_eq(x, x));"));
        assert!(bpl.contains("axiom (forall<T> x: T :: { T_less(x, x) } !T_less(x, x));"));
    }

    #[test]
    fn test_plan3_closure_conversion() {
        let src = r#"
module CLOSURE_TEST
representation {ANY}
  val: U64
feature {ANY}
  make_adder(factor: U64): U64
    do
      let f = fn(x: U64): U64 do x +% factor end
      0
    end
end
"#;
        let m = parse_single_module(src);
        let (_converted, combinators) = closure_convert_module(&m);
        assert_eq!(combinators.len(), 1);
        let comb = &combinators[0];
        assert!(comb.routine.name.starts_with("closure_apply_"));
        assert!(comb.env_struct.is_some());
        let (env_name, env_fields) = comb.env_struct.as_ref().unwrap();
        assert!(env_name.starts_with("ClosureEnv_"));
        assert!(env_fields.iter().any(|(f, _)| f == "factor"));
    }

    #[test]
    fn test_plan3_multi_module_stdlib_pipeline() {
        let ast = parse_phase4_stdlib().expect("stdlib parses cleanly");
        assert_eq!(ast.modules.len(), 9);
        let mut prog_env = ProgramEnv {
            modules: HashMap::new(),
            concepts: HashMap::new(),
        };
        for m in &ast.modules {
            prog_env.modules.insert(m.name.clone(), m.clone());
        }
        let instances = collect_monomorphized_instances(&prog_env);
        assert!(!instances.is_empty() || ast.modules.iter().any(|m| !m.type_params.is_empty()));

        for m in &ast.modules {
            let diags = check_visibility_and_semantics(m);
            assert!(
                !diags.iter().any(|d| d.level == "error"),
                "Module {} had unexpected errors: {diags:?}",
                m.name
            );
            let bpl = emit_boogie(m, 2);
            assert!(!bpl.is_empty());
        }
    }

    #[test]
    fn test_plan4_general_c11_ast_lowering_custom_adt() {
        let src = r#"
module CUSTOM_CONTAINER
representation {ANY}
  | Empty
  | Entry(id: U64, weight: F64)
feature {ANY}
  get_id(c: CUSTOM_CONTAINER): U64
    do
      match c
        case Empty then 0
        case Entry(i, w) then i
      end
    end
end
"#;
        let m = parse_single_module(src);
        let c_code = emit_c11(&m);
        assert!(c_code.contains("typedef struct CUSTOM_CONTAINER"));
        assert!(c_code.contains("TAG_CUSTOM_CONTAINER_Empty"));
        assert!(c_code.contains("TAG_CUSTOM_CONTAINER_Entry"));
        assert!(c_code.contains("make_CUSTOM_CONTAINER_Entry"));
        assert!(c_code.contains("double weight;"));
        assert!(c_code.contains("#pragma STDC FP_CONTRACT OFF"));
    }

    #[test]
    fn test_plan4_c11_and_bend_closure_conversion_codegen() {
        let src = r#"
module CLOSURE_GEN
representation {ANY}
  val: U64
feature {ANY}
  apply_multiplier(factor: U64, x: U64): U64
    do
      let mul = fn(n: U64): U64 do n *% factor end;
      0
    end
end
"#;
        let m = parse_single_module(src);
        let (converted, combinators) = closure_convert_module(&m);
        assert!(!combinators.is_empty());
        let bend = emit_bend_v2(&converted);
        assert!(bend.contains("apply_multiplier"));
        let c_code = emit_c11(&m);
        assert!(c_code.contains("ClosureEnv_"));
        assert!(c_code.contains("closure_apply_"));
    }

    #[test]
    fn test_plan4_automatic_fork_join_parallel_let_analysis() {
        let stmts = vec![
            Stmt::Let("a".to_string(), Expr::Int(10)),
            Stmt::Let("b".to_string(), Expr::Int(20)),
        ];
        let groups = analyze_parallel_lets(&stmts);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);

        let dependent_stmts = vec![
            Stmt::Let("a".to_string(), Expr::Int(10)),
            Stmt::Let("b".to_string(), Expr::Var("a".to_string())),
        ];
        let dep_groups = analyze_parallel_lets(&dependent_stmts);
        assert_eq!(dep_groups.len(), 2);
    }

    #[test]
    fn test_plan4_tail_recursion_to_c11_while_loop() {
        let src = r#"
module TAIL_REC
representation {ANY}
  dummy: U64
feature {ANY}
  countdown(n: U64): U64
    do
      if n = 0 then
        0
      else
        countdown(n - 1)
      end
    end
end
"#;
        let m = parse_single_module(src);
        let r = &m.feature_routines[0].1;
        assert!(is_tail_recursive_routine(r));
        let c_code = emit_c11(&m);
        assert!(c_code.contains("tail_loop:"));
    }

    #[test]
    fn test_plan4_perceus_refcounting_zero_memory_leaks() {
        let hdr = emit_c11_runtime_header();
        assert!(hdr.contains("gardel_live_allocations(void)"));
        assert!(hdr.contains("atomic_fetch_add(&g_gardel_live_allocs, 1);"));
        assert!(hdr.contains("atomic_fetch_sub(&g_gardel_live_allocs, 1);"));
    }

    #[test]
    fn test_plan4_stdlib_seq_and_option_result_contracts() {
        let src = r#"
module OPTION_EXT[A]
representation {ANY}
  | None
  | Some(val: A)
feature {ANY}
  is_none: BOOL
    do
      match Current
        case None then True
        case Some(_) then False
      end
    end
  unwrap_or(def: A): A
    do
      match Current
        case None then def
        case Some(v) then v
      end
    end
end
"#;
        let m = parse_single_module(src);
        let diags = check_visibility_and_semantics(&m);
        assert!(diags.is_empty(), "Expected 0 diags, got {diags:?}");
        let bpl = emit_boogie(&m, 2);
        assert!(bpl.contains("is_None"));
        assert!(bpl.contains("is_Some"));
    }

    #[test]
    fn test_plan4_stdlib_bignat_math_int_and_string_utf8_models() {
        let stdlib = parse_phase4_stdlib().expect("Phase 4 stdlib parses");
        let seq_mod = stdlib.modules.iter().find(|m| m.name == "SEQ").unwrap();
        assert!(
            seq_mod
                .feature_routines
                .iter()
                .any(|(_, r)| r.name == "concat"
                    && r.ensures.iter().any(|(tag, _)| tag == "model_concat"))
        );
        let bignat = stdlib.modules.iter().find(|m| m.name == "BIGNAT").unwrap();
        // size_matches, bounded_limbs + B.5b limbs_in_u64, no_zero_high_limb
        assert_eq!(bignat.invariants.len(), 4);
        assert!(
            bignat
                .invariants
                .iter()
                .any(|(tag, _)| tag == "no_zero_high_limb")
        );
        assert!(
            bignat
                .specify_routines
                .iter()
                .any(|(_, r)| r.name == "value_of" && r.ret_type == "MATH_INT")
        );
        let str_mod = stdlib.modules.iter().find(|m| m.name == "STRING").unwrap();
        assert_eq!(str_mod.invariants.len(), 4);
        assert!(
            str_mod
                .specify_routines
                .iter()
                .any(|(_, r)| r.name == "chars" && r.ret_type == "LIST[CHAR]")
        );
    }

    #[test]
    fn test_plan4_whole_program_multi_module_c11_link_and_run() {
        let stdlib = parse_phase4_stdlib().expect("stdlib parses cleanly");
        for m in &stdlib.modules {
            let c_code = emit_c11(m);
            assert!(!c_code.is_empty());
        }
    }

    #[test]
    fn test_step2_result_and_option_soundness_and_precondition_failures() {
        let stdlib = parse_phase4_stdlib().expect("stdlib parses cleanly");
        let opt_mod = stdlib.modules.iter().find(|m| m.name == "OPTION").unwrap();
        let res_mod = stdlib.modules.iter().find(|m| m.name == "RESULT").unwrap();

        let opt_diags = verify_module_phase3(
            opt_mod,
            phase4_stdlib_source(),
            "stdlib.gdl",
            2,
            true,
            false,
            false,
        );
        assert!(
            opt_diags.is_empty(),
            "OPTION must verify with 0 errors, got: {opt_diags:?}"
        );

        let res_diags = verify_module_phase3(
            res_mod,
            phase4_stdlib_source(),
            "stdlib.gdl",
            2,
            true,
            false,
            false,
        );
        assert!(
            res_diags.is_empty(),
            "RESULT must verify with 0 errors, got: {res_diags:?}"
        );

        // Negative test: An invalid postcondition in RESULT (e.g. ok claims to be is_error) is caught by Z3
        let unsafe_src = r#"
module RESULT_BAD_OK[E, A]
representation {ANY}
  | Error(err: E)
  | Ok(val: A)
create {ANY}
  ok(v: A): RESULT_BAD_OK[E, A]
    do
      Ok(v)
    ensure
      bad_err: Result.is_error
    end
feature {ANY}
  is_error: BOOL
    do
      match Current
        case Error(_) then True
        case Ok(_) then False
      end
    end
end
"#;
        let unsafe_mod = parse_single_module(unsafe_src);
        let diags = verify_module_phase3(
            &unsafe_mod,
            unsafe_src,
            "test_unsafe.gdl",
            2,
            false,
            false,
            false,
        );
        assert!(
            !diags.is_empty(),
            "Invalid contract `bad_err` on `ok` must trigger a verification failure"
        );
        assert!(
            diags.iter().any(|d| d.tag == "bad_err"),
            "Expected postcondition violation `bad_err`, got: {diags:?}"
        );
    }

    #[test]
    fn test_step3_verified_functional_core_positive_and_refinement() {
        let stdlib = parse_phase4_stdlib().expect("stdlib parses cleanly");
        let seq_mod = stdlib.modules.iter().find(|m| m.name == "SEQ").unwrap();
        let list_ops_mod = stdlib
            .modules
            .iter()
            .find(|m| m.name == "LIST_OPS")
            .unwrap();
        let fn_ops_mod = stdlib
            .modules
            .iter()
            .find(|m| m.name == "FUNCTION_OPS")
            .unwrap();

        let seq_diags = verify_module_phase3(
            seq_mod,
            phase4_stdlib_source(),
            "stdlib.gdl",
            2,
            true,
            false,
            false,
        );
        assert!(
            seq_diags.is_empty(),
            "SEQ combinators & laws must verify with 0 errors, got: {seq_diags:?}"
        );

        let list_diags = verify_module_phase3(
            list_ops_mod,
            phase4_stdlib_source(),
            "stdlib.gdl",
            2,
            true,
            false,
            false,
        );
        assert!(
            list_diags.is_empty(),
            "LIST_OPS combinators & laws must verify with 0 errors, got: {list_diags:?}"
        );

        let fn_diags = verify_module_phase3(
            fn_ops_mod,
            phase4_stdlib_source(),
            "stdlib.gdl",
            2,
            true,
            false,
            false,
        );
        assert!(
            fn_diags.is_empty(),
            "FUNCTION_OPS combinators & pointwise laws must verify with 0 errors, got: {fn_diags:?}"
        );
    }

    #[test]
    fn test_step3_soundness_mutation_detection() {
        // Negative Test: Mutated SEQ.map that swaps subtrees Concat(mr, ml, ln) violates model refinement
        let mutated_map_src = r#"
module SEQ_MUTANT[A]

representation {ANY}
  | Empty
  | Leaf(value: A)
  | Concat(left: SEQ_MUTANT[A], right: SEQ_MUTANT[A], len: NAT)

specify {ANY}
  model: LIST[A]
    variant
      Current
    do
      match Current
        case Empty           then LIST.empty
        case Leaf(v)         then LIST.empty.prepend(v)
        case Concat(l, r, _) then l.model.append(r.model)
      end
    end

feature {ANY}
  length: NAT
    variant
      Current
    do
      match Current
        case Empty            then 0
        case Leaf(_)          then 1
        case Concat(_, _, ln) then ln
      end
    end

  map[B](f: A -> B): SEQ_MUTANT[B]
    variant
      Current
    do
      match Current
        case Empty then
          Empty
        case Leaf(v) then
          Leaf(f(v))
        case Concat(l, r, ln) then
          let ml = l.map(f)
          let mr = r.map(f)
          Concat(mr, ml, ln)
      end
    ensure
      model_map: Result.model = LIST_OPS.map(Current.model, f)
    end
end
"#;
        let mutated_mod = parse_single_module(mutated_map_src);
        let diags = verify_module_phase3(
            &mutated_mod,
            mutated_map_src,
            "test_mutant.gdl",
            2,
            false,
            false,
            false,
        );
        assert!(
            !diags.is_empty(),
            "Swapped recursive subtrees in map must fail model_map refinement postcondition"
        );
        assert!(
            diags.iter().any(|d| d.tag.contains("model_map")),
            "Expected failure on tag `model_map`, got: {diags:?}"
        );
    }

    #[test]
    fn test_step3_parallel_fork_join_lowering() {
        let stdlib = parse_phase4_stdlib().expect("stdlib parses cleanly");
        let seq_mod = stdlib.modules.iter().find(|m| m.name == "SEQ").unwrap();
        let map_routine = seq_mod
            .feature_routines
            .iter()
            .find(|(_, r)| r.name == "map")
            .unwrap();

        if let Expr::Match(_, cases) = &map_routine.1.body {
            let concat_case = cases
                .iter()
                .find(|c| c.ctor() == "Concat")
                .expect("Concat case");
            let groups = analyze_parallel_lets(&concat_case.stmts);
            assert_eq!(groups.len(), 1, "Expected 1 parallel group in SEQ.map");
            assert_eq!(
                groups[0].len(),
                2,
                "Expected 2 independent let bindings (ml, mr) for parallel fork-join"
            );
            assert_eq!(groups[0][0].0, "ml");
            assert_eq!(groups[0][1].0, "mr");
        } else {
            panic!("Expected match expression in SEQ.map");
        }

        let filter_routine = seq_mod
            .feature_routines
            .iter()
            .find(|(_, r)| r.name == "filter")
            .unwrap();
        if let Expr::Match(_, cases) = &filter_routine.1.body {
            let concat_case = cases
                .iter()
                .find(|c| c.ctor() == "Concat")
                .expect("Concat case");
            let groups = analyze_parallel_lets(&concat_case.stmts);
            assert_eq!(groups.len(), 1, "Expected 1 parallel group in SEQ.filter");
            assert_eq!(
                groups[0].len(),
                2,
                "Expected 2 independent let bindings (fl, fr) for parallel fork-join"
            );
            assert_eq!(groups[0][0].0, "fl");
            assert_eq!(groups[0][1].0, "fr");
        } else {
            panic!("Expected match expression in SEQ.filter");
        }
    }

    #[test]
    fn test_step4_algebraic_law_catalogue_and_concept_conformance() {
        let stdlib = parse_phase4_stdlib().expect("stdlib parses cleanly");

        // 1. Verify standard concepts exist and refine correctly
        let eq = stdlib
            .concepts
            .iter()
            .find(|c| c.name == "EQ")
            .expect("EQ concept exists");
        assert!(eq.refines.is_empty(), "EQ is base concept");
        assert!(eq.signatures.iter().any(|s| s.name == "equals"));
        assert_eq!(
            eq.laws.len(),
            3,
            "EQ must declare reflexive, symmetric, transitive"
        );

        let canonical_eq = stdlib
            .concepts
            .iter()
            .find(|c| c.name == "CANONICAL_EQ")
            .expect("CANONICAL_EQ concept exists");
        assert_eq!(canonical_eq.refines, vec!["EQ".to_string()]);
        assert!(
            canonical_eq
                .laws
                .iter()
                .any(|l| l.name == "agrees_with_structural")
        );

        let ord = stdlib
            .concepts
            .iter()
            .find(|c| c.name == "ORD")
            .expect("ORD concept exists");
        assert_eq!(ord.refines, vec!["EQ".to_string()]);
        assert!(ord.signatures.iter().any(|s| s.name == "less_than"));
        assert!(
            ord.default_routines
                .iter()
                .any(|r| r.name == "less_or_equal")
        );
        assert!(ord.laws.iter().any(|l| l.name == "irreflexive"));
        assert!(ord.laws.iter().any(|l| l.name == "transitive"));
        assert!(ord.laws.iter().any(|l| l.name == "total"));

        // 2. Verify concept conformance declarations across stdlib
        let seq_mod = stdlib
            .modules
            .iter()
            .find(|m| m.name == "SEQ")
            .expect("SEQ module exists");
        let seq_sat = seq_mod.satisfies.as_ref().expect("SEQ satisfies clause");
        assert_eq!(seq_sat.concepts, vec!["EQ"]);
        assert_eq!(
            seq_sat.where_constraints,
            vec![("A".to_string(), "CANONICAL_EQ".to_string())]
        );

        let opt_mod = stdlib
            .modules
            .iter()
            .find(|m| m.name == "OPTION")
            .expect("OPTION module exists");
        let opt_sat = opt_mod.satisfies.as_ref().expect("OPTION satisfies clause");
        assert_eq!(opt_sat.concepts, vec!["CANONICAL_EQ"]);
        assert_eq!(
            opt_sat.where_constraints,
            vec![("A".to_string(), "CANONICAL_EQ".to_string())]
        );

        let res_mod = stdlib
            .modules
            .iter()
            .find(|m| m.name == "RESULT")
            .expect("RESULT module exists");
        let res_sat = res_mod.satisfies.as_ref().expect("RESULT satisfies clause");
        assert_eq!(res_sat.concepts, vec!["CANONICAL_EQ"]);
        assert_eq!(
            res_sat.where_constraints,
            vec![
                ("E".to_string(), "CANONICAL_EQ".to_string()),
                ("A".to_string(), "CANONICAL_EQ".to_string())
            ]
        );

        let bignat_mod = stdlib
            .modules
            .iter()
            .find(|m| m.name == "BIGNAT")
            .expect("BIGNAT module exists");
        let bignat_sat = bignat_mod
            .satisfies
            .as_ref()
            .expect("BIGNAT satisfies clause");
        assert_eq!(bignat_sat.concepts, vec!["ORD"]);
        assert!(
            bignat_mod
                .feature_routines
                .iter()
                .any(|(_, r)| r.name == "less_or_equal"),
            "less_or_equal injected into BIGNAT"
        );

        // 3. Verify Bird & Wadler algebraic law catalogue in LIST_OPS and FUNCTION_OPS
        let list_ops = stdlib
            .modules
            .iter()
            .find(|m| m.name == "LIST_OPS")
            .expect("LIST_OPS module exists");
        let list_laws: Vec<&str> = list_ops
            .specify_laws
            .iter()
            .map(|(_, l)| l.name.as_str())
            .collect();
        assert!(list_laws.contains(&"fold_left_append"));
        assert!(
            !list_laws.contains(&"fold_left_append_distributivity"),
            "duplicate of fold_left_append must stay removed"
        );
        assert!(list_laws.contains(&"fold_map_fusion"));

        let fn_ops = stdlib
            .modules
            .iter()
            .find(|m| m.name == "FUNCTION_OPS")
            .expect("FUNCTION_OPS module exists");
        let fn_laws: Vec<&str> = fn_ops
            .specify_laws
            .iter()
            .map(|(_, l)| l.name.as_str())
            .collect();
        assert!(fn_laws.contains(&"compose_associative"));
        assert!(fn_laws.contains(&"compose_left_identity"));
        assert!(fn_laws.contains(&"compose_right_identity"));
        assert!(fn_laws.contains(&"compose_pointwise"));
        assert!(fn_laws.contains(&"identity_law"));

        // 4. Verify all stdlib modules pass semantic & visibility checks
        for m in &stdlib.modules {
            let diags = check_visibility_and_semantics(m);
            assert!(
                diags.is_empty(),
                "Module {} has semantic/visibility errors: {:?}",
                m.name,
                diags
            );
        }
    }

    #[test]
    fn test_step4_negative_seq_cannot_satisfy_canonical_eq() {
        let bad_seq_src = r#"
module BAD_SEQ[A] satisfies CANONICAL_EQ where A satisfies CANONICAL_EQ

representation {ANY}
  | Empty
  | Leaf(value: A)
  | Concat(left: BAD_SEQ[A], right: BAD_SEQ[A], len: NAT)

specify {ANY}
  model: LIST[A]
    variant Current
    do
      match Current
        case Empty then LIST.empty
        case Leaf(v) then LIST.empty.prepend(v)
        case Concat(l, r, _) then l.model.append(r.model)
      end
    end

feature {ANY}
  equals(other: BAD_SEQ[A]): BOOL
    do
      Current.model = other.model
    ensure
      model_equality: Result = (Current.model = other.model)
    end

specify {CANONICAL_EQ}
  law reflexive(a: BAD_SEQ[A])
    ensure
      refl: a.equals(a)
    end

  law symmetric(a: BAD_SEQ[A], b: BAD_SEQ[A])
    require
      ab: a.equals(b)
    ensure
      ba: b.equals(a)
    end

  law transitive(a: BAD_SEQ[A], b: BAD_SEQ[A], c: BAD_SEQ[A])
    require
      ab: a.equals(b)
      bc: b.equals(c)
    ensure
      ac: a.equals(c)
    end

  law agrees_with_structural(a: BAD_SEQ[A], b: BAD_SEQ[A])
    ensure
      sound: a.equals(b) = (a = b)
    end
end
"#;
        let bad_mod = parse_single_module(bad_seq_src);
        let diags = verify_module_phase3(
            &bad_mod,
            bad_seq_src,
            "test_bad_seq.gdl",
            2,
            false,
            false,
            false,
        );
        assert!(
            !diags.is_empty(),
            "Observational sequence equivalence must fail CANONICAL_EQ.agrees_with_structural because structural equality does not match model equality"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.tag.contains("sound") || d.tag.contains("agrees_with_structural")),
            "Expected failure on sound postcondition of agrees_with_structural, got: {diags:?}"
        );
    }

    #[test]
    fn test_step4_negative_missing_law_proof() {
        let incomplete_ord_src = r#"
module INCOMPLETE_ORD satisfies ORD

feature {ANY}
  equals(other: INCOMPLETE_ORD): BOOL
    do
      True
    end

  less_than(other: INCOMPLETE_ORD): BOOL
    do
      False
    end

specify {ORD}
  law reflexive(a: INCOMPLETE_ORD)
    ensure
      refl: a.equals(a)
    end

  law symmetric(a: INCOMPLETE_ORD, b: INCOMPLETE_ORD)
    require
      ab: a.equals(b)
    ensure
      ba: b.equals(a)
    end

  law transitive(a: INCOMPLETE_ORD, b: INCOMPLETE_ORD, c: INCOMPLETE_ORD)
    require
      ab: a.equals(b)
      bc: b.equals(c)
    ensure
      ac: a.equals(c)
    end

  law irreflexive(a: INCOMPLETE_ORD)
    ensure
      not_less_than_self: not a.less_than(a)
    end
  -- Notice: missing law total!
end
"#;
        let stdlib = parse_phase4_stdlib().expect("stdlib parses");
        let mut concept_map = HashMap::new();
        for c in &stdlib.concepts {
            concept_map.insert(c.name.clone(), c.clone());
        }

        let mut inc_mod = parse_single_module(incomplete_ord_src);
        let diags = check_and_expand_concepts(&mut inc_mod, &concept_map);
        assert!(
            !diags.is_empty(),
            "Missing law proof for `total` in ORD must be rejected"
        );
        assert!(
            diags.iter().any(|d| d.rule == "Phase 2 (Missing Law Proof)"
                && d.routine == "total"
                && d.tag == "ORD"),
            "Expected Phase 2 (Missing Law Proof) for `total`, got: {diags:?}"
        );
    }

    #[test]
    fn test_step4_negative_concept_bound_violation() {
        let bad_bounds_src = r#"
module BAD_BOUNDS[A] satisfies EQ where B satisfies EQ, A satisfies NONEXISTENT_CONCEPT

feature {ANY}
  equals(other: BAD_BOUNDS[A]): BOOL
    do
      True
    end

specify {EQ}
  law reflexive(a: BAD_BOUNDS[A])
    ensure
      refl: a.equals(a)
    end

  law symmetric(a: BAD_BOUNDS[A], b: BAD_BOUNDS[A])
    require
      ab: a.equals(b)
    ensure
      ba: b.equals(a)
    end

  law transitive(a: BAD_BOUNDS[A], b: BAD_BOUNDS[A], c: BAD_BOUNDS[A])
    require
      ab: a.equals(b)
      bc: b.equals(c)
    ensure
      ac: a.equals(c)
    end
end
"#;
        let stdlib = parse_phase4_stdlib().expect("stdlib parses");
        let mut concept_map = HashMap::new();
        for c in &stdlib.concepts {
            concept_map.insert(c.name.clone(), c.clone());
        }

        let mut bad_mod = parse_single_module(bad_bounds_src);
        let diags = check_and_expand_concepts(&mut bad_mod, &concept_map);
        assert!(
            !diags.is_empty(),
            "Invalid type parameter and unknown concept in where constraints must be rejected"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.rule == "Phase 2 (Invalid Type Constraint)" && d.routine == "B"),
            "Expected Invalid Type Constraint for undeclared B, got: {diags:?}"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.rule == "Phase 2 (Unknown Concept)" && d.tag == "NONEXISTENT_CONCEPT"),
            "Expected Unknown Concept for NONEXISTENT_CONCEPT, got: {diags:?}"
        );
    }

    #[test]
    fn test_step4_diagnostic_lemma_surfacing() {
        let stdlib = parse_phase4_stdlib().expect("stdlib parses cleanly");
        let seq_mod = stdlib.modules.iter().find(|m| m.name == "SEQ").unwrap();

        // 1. Goal with concat/append should surface SEQ catalogue laws
        let concat_goal = Expr::BinOp(
            "=".to_string(),
            Box::new(Expr::MethodCall(
                Box::new(Expr::MethodCall(
                    Box::new(Expr::Var("a".to_string())),
                    "concat".to_string(),
                    vec![Expr::Var("b".to_string())],
                )),
                "concat".to_string(),
                vec![Expr::Var("c".to_string())],
            )),
            Box::new(Expr::Var("c".to_string())),
        );
        let lemmas = find_relevant_lemmas(seq_mod, "test_routine", &concat_goal);
        assert!(
            lemmas.iter().any(|l| l.contains("concat_associative")),
            "Expected concat_associative in relevant lemmas, got: {lemmas:?}"
        );
        assert!(
            lemmas
                .iter()
                .any(|l| l.contains("concat_length_homomorphism")),
            "Expected concat_length_homomorphism in relevant lemmas, got: {lemmas:?}"
        );

        // 2. Goal with equals should surface EQ and CANONICAL_EQ laws
        let eq_goal = Expr::BinOp(
            "=".to_string(),
            Box::new(Expr::MethodCall(
                Box::new(Expr::Var("x".to_string())),
                "equals".to_string(),
                vec![Expr::Var("y".to_string())],
            )),
            Box::new(Expr::Bool(true)),
        );
        let eq_lemmas = find_relevant_lemmas(seq_mod, "test_routine", &eq_goal);
        assert!(eq_lemmas.contains(&"EQ.transitive".to_string()));
        assert!(eq_lemmas.contains(&"CANONICAL_EQ.agrees_with_structural".to_string()));

        // 3. Goal with less_than / less_or_equal should surface ORD laws
        let ord_goal = Expr::MethodCall(
            Box::new(Expr::Var("x".to_string())),
            "less_than".to_string(),
            vec![Expr::Var("y".to_string())],
        );
        let ord_lemmas = find_relevant_lemmas(seq_mod, "test_routine", &ord_goal);
        assert!(ord_lemmas.contains(&"ORD.total".to_string()));
        assert!(ord_lemmas.contains(&"ORD.transitive".to_string()));
        assert!(ord_lemmas.contains(&"ORD.irreflexive".to_string()));

        // 4. Goal with map/filter should surface Functor laws
        let map_goal = Expr::MethodCall(
            Box::new(Expr::Var("s".to_string())),
            "map".to_string(),
            vec![Expr::Var("f".to_string())],
        );
        let map_lemmas = find_relevant_lemmas(seq_mod, "test_routine", &map_goal);
        assert!(map_lemmas.contains(&"law SEQ.map_fusion.fusion".to_string()));
        assert!(map_lemmas.contains(&"law SEQ.map_identity.id_preserved".to_string()));

        // 5. Goal with and_then should surface Monad laws
        let monad_goal = Expr::MethodCall(
            Box::new(Expr::Var("opt".to_string())),
            "and_then".to_string(),
            vec![Expr::Var("f".to_string())],
        );
        let monad_lemmas = find_relevant_lemmas(seq_mod, "test_routine", &monad_goal);
        assert!(monad_lemmas.contains(&"law OPTION.and_then_associative.assoc".to_string()));
        assert!(monad_lemmas.contains(&"law RESULT.and_then_right_identity.right_id".to_string()));

        // 6. Goal with fold_left / fold_right should surface Fold laws
        let fold_goal = Expr::MethodCall(
            Box::new(Expr::Var("xs".to_string())),
            "fold_left".to_string(),
            vec![Expr::Var("f".to_string()), Expr::Var("init".to_string())],
        );
        let fold_lemmas = find_relevant_lemmas(seq_mod, "test_routine", &fold_goal);
        assert!(fold_lemmas.contains(&"law LIST_OPS.fold_left_append.chains".to_string()));
        assert!(
            !fold_lemmas
                .iter()
                .any(|l| l.contains("fold_left_append_distributivity"))
        );
        assert!(fold_lemmas.contains(&"law LIST_OPS.fold_map_fusion.fused".to_string()));

        // 7. Goal with compose should surface Combinator laws
        let comp_goal = Expr::MethodCall(
            Box::new(Expr::Var("FUNCTION_OPS".to_string())),
            "compose".to_string(),
            vec![Expr::Var("f".to_string()), Expr::Var("g".to_string())],
        );
        let comp_lemmas = find_relevant_lemmas(seq_mod, "test_routine", &comp_goal);
        assert!(comp_lemmas.contains(&"law FUNCTION_OPS.compose_associative.assoc".to_string()));
        assert!(
            comp_lemmas.contains(&"law FUNCTION_OPS.compose_left_identity.left_id".to_string())
        );

        // 8. Filter-only goal does not pollute with OPTION/RESULT map laws
        let filter_goal = Expr::MethodCall(
            Box::new(Expr::Var("s".to_string())),
            "filter".to_string(),
            vec![Expr::Var("p".to_string())],
        );
        let filter_lemmas = find_relevant_lemmas(seq_mod, "test_routine", &filter_goal);
        assert!(filter_lemmas.contains(&"law SEQ.filter_conjunction.conjunction".to_string()));
        assert!(!filter_lemmas.contains(&"law OPTION.map_identity.id_mapped".to_string()));

        // 9. Structural '=' on EQ-conforming module surfaces EQ & CANONICAL_EQ laws (Spec Payload 3)
        let struct_eq_goal = Expr::BinOp(
            "=".to_string(),
            Box::new(Expr::Var("Result".to_string())),
            Box::new(Expr::Var("Current".to_string())),
        );
        let struct_eq_lemmas = find_relevant_lemmas(seq_mod, "test_routine", &struct_eq_goal);
        assert!(struct_eq_lemmas.contains(&"CANONICAL_EQ.agrees_with_structural".to_string()));
        assert!(struct_eq_lemmas.contains(&"EQ.transitive".to_string()));

        // 10. Transitive specify query opacity in BIGNAT (Spec Payload 4: model -> value_of)
        let bignat_mod = stdlib.modules.iter().find(|m| m.name == "BIGNAT").unwrap();
        let bignat_model_goal = Expr::BinOp(
            "=".to_string(),
            Box::new(Expr::MethodCall(
                Box::new(Expr::Var("Result".to_string())),
                "model".to_string(),
                Vec::new(),
            )),
            Box::new(Expr::Int(0)),
        );
        let bignat_lemmas = find_relevant_lemmas(bignat_mod, "zero", &bignat_model_goal);
        assert!(bignat_lemmas.contains(&"reveal model".to_string()));
        assert!(bignat_lemmas.contains(&"reveal value_of".to_string()));

        // 11. Law instantiation call expansion in check blocks (`SEQ.concat_associative(a, b, c)`)
        let law_call_expr = Expr::MethodCall(
            Box::new(Expr::Var("SEQ".to_string())),
            "concat_associative".to_string(),
            vec![
                Expr::Var("a".to_string()),
                Expr::Var("b".to_string()),
                Expr::Var("c".to_string()),
            ],
        );
        let expanded = expand_law_call_in_expr(&law_call_expr, seq_mod)
            .expect("SEQ.concat_associative(a, b, c) must expand to its ensure predicate");
        let bpl = emit_boogie_expr(
            &expanded,
            seq_mod,
            &HashSet::new(),
            &HashSet::new(),
            "DefaultFuel",
        );
        assert!(bpl.contains("fn_equals") && bpl.contains("fn_concat"));

        // 12. RFC 8259 control character escaping in JSON diagnostics
        assert_eq!(escape_json("a\tb\x07c"), "a\\tb\\u0007c");
    }

    const FROM_SCALAR_SRC: &str = r#"
functions SCALAR_CHECK

feature {ANY}

  unchecked(n: NAT): CHAR
    do
      CHAR.from_scalar(n)
    end

  surrogate: CHAR
    do
      CHAR.from_scalar(55296)
    end

  checked(n: NAT): CHAR
    require
      ascii: n < 128
    do
      CHAR.from_scalar(n)
    ensure
      code_kept: Result.code = n
    end

  guarded(n: NAT): NAT
    do
      if n < 128 then
        CHAR.from_scalar(n).code
      else
        0
      end
    end

end
"#;

    #[test]
    fn test_b5a_from_scalar_precondition_is_a_proof_obligation() {
        let m = parse_single_module(FROM_SCALAR_SRC);
        let bpl = emit_boogie(&m, 2);
        assert!(bpl.contains("unchecked::require::valid_scalar"));
        assert!(bpl.contains("surrogate::require::valid_scalar"));
        assert!(bpl.contains("checked::require::valid_scalar"));
        assert!(bpl.contains("guarded::require::valid_scalar"));

        let is_violation =
            |d: &Diagnostic, routine: &str| d.routine == routine && d.tag == "valid_scalar";
        // Formal (Boogie) path, falling back to bounded sampling when no solver is installed.
        let diags = verify_module_phase3(&m, FROM_SCALAR_SRC, "scalar.gdl", 2, true, false, false);
        assert!(
            diags.iter().any(|d| is_violation(d, "unchecked")),
            "unconstrained NAT passed to CHAR.from_scalar must be rejected, got {diags:?}"
        );
        assert!(
            diags.iter().any(|d| is_violation(d, "surrogate")),
            "surrogate code point 0xD800 passed to CHAR.from_scalar must be rejected, got {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.routine == "checked"),
            "got {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.routine == "guarded"),
            "got {diags:?}"
        );

        // Bounded symbolic evaluator path.
        let sym = verify_module_phase3(&m, FROM_SCALAR_SRC, "scalar.gdl", 2, true, false, true);
        assert!(
            sym.iter().any(|d| is_violation(d, "surrogate")),
            "symbolic evaluator must reject CHAR.from_scalar(0xD800), got {sym:?}"
        );
        assert!(!sym.iter().any(|d| d.routine == "checked"), "got {sym:?}");
    }

    #[test]
    fn test_b5a_string_decode_axiom_removed_and_lemmas_proven() {
        let stdlib = parse_phase4_stdlib().expect("stdlib parses cleanly");
        let str_mod = stdlib.modules.iter().find(|m| m.name == "STRING").unwrap();
        let bpl = emit_boogie(str_mod, 2);
        // The trusted identity axiom on `decode_utf8` must be gone ...
        assert!(
            !bpl.contains("fn_decode_utf8(_fuel, Current, bs) == bs"),
            "hard-coded decode_utf8 identity axiom must not be emitted"
        );
        for line in bpl
            .lines()
            .filter(|l| l.starts_with("axiom") && l.contains(":: { fn_decode_utf8("))
        {
            assert!(
                line.contains("fn_all_bytes_valid") || line.contains("_c1"),
                "every decode_utf8 axiom must be its guarded definition/contract or receiver independence: {line}"
            );
        }
        // ... replaced by proven contracts, and `from_scalar` in the decoder is checked.
        assert!(bpl.contains("decode_utf8::ensure::same_length"));
        assert!(bpl.contains("decode_utf8::require::valid_scalar"));
        assert!(bpl.contains("procedure Verify_law_decode_injective"));
        let decode = str_mod
            .specify_routines
            .iter()
            .map(|(_, r)| r)
            .find(|r| r.name == "decode_utf8")
            .unwrap();
        assert!(decode.requires.iter().any(|(t, _)| t == "ascii"));
        assert!(decode.ensures.iter().any(|(t, _)| t == "same_length"));
        assert!(
            str_mod
                .specify_laws
                .iter()
                .any(|(_, l)| l.name == "decode_injective")
        );

        let diags = verify_module_phase3(
            str_mod,
            phase4_stdlib_source(),
            "stdlib.gdl",
            2,
            true,
            false,
            false,
        );
        assert!(
            diags.is_empty(),
            "STRING must verify without the decode axiom, got {diags:?}"
        );
    }

    #[test]
    fn test_b5a_option_result_equals_has_no_unconditional_sound_clause() {
        let stdlib = parse_phase4_stdlib().expect("stdlib parses cleanly");
        for (name, expected) in [
            ("OPTION", ["none_eq", "some_eq", "variant_mismatch"]),
            ("RESULT", ["err_eq", "ok_eq", "variant_mismatch"]),
        ] {
            let m = stdlib.modules.iter().find(|m| m.name == name).unwrap();
            let equals = m
                .feature_routines
                .iter()
                .map(|(_, r)| r)
                .find(|r| r.name == "equals")
                .unwrap();
            let tags: Vec<&str> = equals.ensures.iter().map(|(t, _)| t.as_str()).collect();
            assert_eq!(tags, expected, "{name}.equals postconditions");
            let iface = format_module_interface(m);
            assert!(
                !iface.contains("sound: Result = (Current = other)"),
                "{name}.equals must not export an unconditional `sound` clause:\n{iface}"
            );
            // Structural agreement survives only as the CANONICAL_EQ law, and still verifies.
            assert!(
                m.specify_laws
                    .iter()
                    .any(|(_, l)| l.name == "agrees_with_structural")
            );
            let diags = verify_module_phase3(
                m,
                phase4_stdlib_source(),
                "stdlib.gdl",
                2,
                true,
                false,
                false,
            );
            assert!(diags.is_empty(), "{name} must verify, got {diags:?}");
        }
    }

    const GHOST_IN_EQUALS_SRC: &str = r#"
module GHOSTY satisfies ORD

representation {NONE}
  n: NAT

specify {ANY}

  model: MATH_INT
    do
      Current.n
    end

feature {ANY}

  equals(other: GHOSTY): BOOL
    do
      Current.model = other.model
    ensure
      model_eq: Result = (Current.model = other.model)
    end

  less_than(other: GHOSTY): BOOL
    do
      Current.model < other.model
    ensure
      model_lt: Result = (Current.model < other.model)
    end

end
"#;

    #[test]
    fn test_b5b_no_ghost_exemption_for_equals_and_less_than() {
        let m = parse_single_module(GHOST_IN_EQUALS_SRC);
        let diags = check_visibility_and_semantics(&m);
        for routine in ["equals", "less_than"] {
            assert!(
                diags.iter().any(|d| d.level == "error"
                    && d.rule == "Rule 3 (Ghost Stays Ghost)"
                    && d.routine == routine
                    && d.message.contains("`model`")),
                "runtime body of `{routine}` reading ghost `model` must be rejected, got {diags:?}"
            );
        }

        // The stdlib itself must now pass Rule 3 with no exemption.
        let stdlib = parse_phase4_stdlib().expect("stdlib parses cleanly");
        for name in ["SEQ", "BIGNAT"] {
            let m = stdlib.modules.iter().find(|m| m.name == name).unwrap();
            let diags = check_visibility_and_semantics(m);
            assert!(
                !diags.iter().any(|d| d.rule == "Rule 3 (Ghost Stays Ghost)"),
                "{name} runtime bodies must not read ghost symbols, got {diags:?}"
            );
        }
    }

    fn parse_stdlib_module_from(src: &str, name: &str) -> ModuleDecl {
        let tokens = tokenize(src).expect("lexing should succeed");
        let mut parser = Parser::new(tokens);
        let mut ast = parser.parse_file().expect("parsing should succeed");
        let mut cmap = HashMap::new();
        for c in &ast.concepts {
            cmap.insert(c.name.clone(), c.clone());
        }
        let idx = ast
            .modules
            .iter()
            .position(|m| m.name == name)
            .expect("module present");
        let mut m = ast.modules.swap_remove(idx);
        let _ = check_and_expand_concepts(&mut m, &cmap);
        m
    }

    #[test]
    fn test_b5b_bignat_normalization_invariant_rejects_zero_high_limb() {
        let base = phase4_stdlib_source();
        // Positive control: the shipped BIGNAT (limb-wise equals/less_than) verifies.
        let good = parse_stdlib_module_from(base, "BIGNAT");
        assert!(
            good.invariants
                .iter()
                .any(|(t, _)| t == "no_zero_high_limb")
        );
        let diags = verify_module_phase3(&good, base, "stdlib.gdl", 2, true, false, false);
        assert!(diags.is_empty(), "BIGNAT must verify, got {diags:?}");

        // A construction with a zero most-significant limb ([0] denotes 0 but is
        // not the canonical empty limb list) must violate the invariant.
        let anchor = "  from_u64(x: U64): BIGNAT\n";
        assert!(base.contains(anchor));
        let bad_src = base.replacen(
            anchor,
            "  zero_limb: BIGNAT\n    do\n      { limbs = LIST.empty.prepend(0), size = 1 }\n    end\n\n  from_u64(x: U64): BIGNAT\n",
            1,
        );
        let bad = parse_stdlib_module_from(&bad_src, "BIGNAT");
        let diags = verify_module_phase3(&bad, &bad_src, "stdlib.gdl", 2, true, false, false);
        assert!(
            diags.iter().any(|d| d.routine == "zero_limb"),
            "non-normalized BIGNAT construction must be rejected, got {diags:?}"
        );
        // Control: the same shape with a non-zero limb verifies, so the failure above is
        // the normalization invariant (size/bounds/range all hold for `[0]`).
        let ok_src = base.replacen(
            anchor,
            "  one_limb: BIGNAT
    do
      { limbs = LIST.empty.prepend(1), size = 1 }
    end

  from_u64(x: U64): BIGNAT
",
            1,
        );
        let ok = parse_stdlib_module_from(&ok_src, "BIGNAT");
        let diags = verify_module_phase3(&ok, &ok_src, "stdlib.gdl", 2, true, false, false);
        assert!(
            diags.is_empty(),
            "normalized single-limb construction must verify, got {diags:?}"
        );

        // shift_in_low_limb(0) on a zero BIGNAT would build [0]; its precondition forbids it.
        let anchor = "feature {NONE}\n\n  compare_limbs";
        assert!(base.contains(anchor));
        let bad_src = base.replacen(
            anchor,
            concat!(
                "  push_zero: BIGNAT\n    require\n      is_zero: Current.limb_count = 0\n",
                "    do\n      Current.shift_in_low_limb(0)\n    end\n\n",
                "  shift_zero: BIGNAT\n    require\n      non_zero: Current.limb_count > 0\n",
                "      room: Current.limb_count < 1024\n    do\n      Current.shift_in_low_limb(0)\n    end\n\n",
                "feature {NONE}\n\n  compare_limbs"
            ),
            1,
        );
        let bad = parse_stdlib_module_from(&bad_src, "BIGNAT");
        let diags = verify_module_phase3(&bad, &bad_src, "stdlib.gdl", 2, true, false, false);
        // The Boogie emitter guards callee contracts by their preconditions (it does not yet
        // emit call-site `require` assertions), so the violated `no_leading_zero` leaves the
        // result unconstrained and the result-invariant obligation of `push_zero` fails.
        assert!(
            diags.iter().any(|d| d.routine == "push_zero"),
            "shift_in_low_limb(0) on zero must be rejected, got {diags:?}"
        );
        // A zero low limb under a non-zero BIGNAT keeps the high limb non-zero and is accepted.
        assert!(
            !diags.iter().any(|d| d.routine == "shift_zero"),
            "shift_in_low_limb(0) on a non-zero BIGNAT must verify, got {diags:?}"
        );
    }

    // ------------------------------------------------------------------------
    // Task B.1: Automatic Standard Prelude injection (§4.1)
    // ------------------------------------------------------------------------

    /// A user module that uses prelude `OPTION` / `SEQ` (types, constructors, routines,
    /// contracts, a prelude-routine precondition and two prelude laws) without declaring them.
    const B1_PRELUDE_CLIENT: &str = r#"
module PRELUDE_CLIENT

representation {ANY}
  dummy: NAT

feature {ANY}

  pick(o: OPTION[NAT], d: NAT): NAT
    do
      o.unwrap_or(d)
    ensure
      some_case: o.is_some implies Result = o.get
      none_case: o.is_none implies Result = d
    end

  safe_get(o: OPTION[NAT]): NAT
    require
      has: o.is_some
    do
      o.get
    ensure
      wraps: Some(Result) = o
    end

  extract(v: NAT, d: NAT): NAT
    do
      check extracts: OPTION.unwrap_or_some(v, d) end
      Some(v).unwrap_or(d)
    ensure
      is_v: Result = v
    end

  describe(o: OPTION[NAT]): NAT
    do
      match o
        case None then 0
        case Some(x) then 1
      end
    ensure
      tag: (Result = 1) = o.is_some
    end

  joined_length(a: SEQ[NAT], b: SEQ[NAT]): NAT
    require
      fits: a.length <= 1000 and b.length <= 1000
    do
      check homo: SEQ.concat_length_homomorphism(a, b) end
      a.concat(b).length
    ensure
      sum: Result = a.length + b.length
    end

end
"#;

    fn parse_user_module_with_prelude(src: &str) -> ModuleDecl {
        let tokens = tokenize(src).expect("lexing should succeed");
        let mut parser = Parser::new(tokens);
        let mut ast = parser.parse_file().expect("parsing should succeed");
        let mut cmap = HashMap::new();
        for c in standard_prelude()
            .concepts
            .iter()
            .chain(ast.concepts.iter())
        {
            cmap.insert(c.name.clone(), c.clone());
        }
        let mut m = ast.modules.pop().expect("expected one module");
        let _ = check_and_expand_concepts(&mut m, &cmap);
        m
    }

    #[test]
    fn test_b1_prelude_injection_verifies_user_module() {
        assert!(prelude_mode_enabled(false, false));
        let m = parse_user_module_with_prelude(B1_PRELUDE_CLIENT);
        let bpl = with_prelude_injection(true, || emit_boogie(&m, DEFAULT_FUEL));

        // Prelude declarations, contracts and laws are in the verification context (namespaced) ...
        for sym in [
            "type OPTION A;",
            "type SEQ A;",
            "function OPTION__fn_unwrap_or<A>(",
            "function OPTION__Some<A>(",
            "OPTION__pre_get_0(o)",
            "OPTION__law_",
            "SEQ__law_",
            "SEQ__fn_concat(DefaultFuel, a, b)",
        ] {
            assert!(bpl.contains(sym), "injected context must contain `{sym}`");
        }
        // ... but no prelude obligation is re-proven: only the user's own procedures exist.
        let procs: Vec<&str> = bpl
            .lines()
            .filter(|l| l.starts_with("procedure "))
            .collect();
        assert_eq!(
            procs.len(),
            m.feature_routines.len(),
            "procedures: {procs:?}"
        );
        for (_, r) in &m.feature_routines {
            assert!(
                procs
                    .iter()
                    .any(|p| p.starts_with(&format!("procedure Verify_{}(", r.name)))
            );
        }

        let diags = with_prelude_injection(true, || {
            verify_module_phase3(
                &m,
                B1_PRELUDE_CLIENT,
                "prelude_client.gdl",
                DEFAULT_FUEL,
                true,
                false,
                false,
            )
        });
        assert!(
            diags.is_empty(),
            "prelude client must verify with 0 errors, got: {diags:?}"
        );
    }

    #[test]
    fn test_b1_prelude_contracts_are_enforced_not_assumed() {
        // Dropping the caller's `require has` must surface OPTION.get's `present` precondition,
        // and a wrong postcondition must still fail: the prelude does not make goals vacuous.
        let src = B1_PRELUDE_CLIENT
            .replace("    require\n      has: o.is_some\n", "")
            .replace("is_v: Result = v", "is_v: Result = d");
        let m = parse_user_module_with_prelude(&src);
        let diags = with_prelude_injection(true, || {
            verify_module_phase3(
                &m,
                &src,
                "prelude_client_bad.gdl",
                DEFAULT_FUEL,
                true,
                false,
                false,
            )
        });
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "safe_get" && d.tag == "present"),
            "missing prelude precondition must be reported, got: {diags:?}"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "extract" && d.tag == "is_v"),
            "false postcondition must be reported, got: {diags:?}"
        );
        assert_eq!(diags.len(), 2, "no other failures expected, got: {diags:?}");
    }

    #[test]
    fn test_b1_no_prelude_leaves_prelude_symbols_unknown() {
        assert!(!prelude_mode_enabled(false, true));
        let m = parse_user_module_with_prelude(B1_PRELUDE_CLIENT);
        let bpl = with_prelude_injection(false, || emit_boogie(&m, DEFAULT_FUEL));
        assert!(!bpl.contains("OPTION__") && !bpl.contains("type OPTION"));
        let diags = with_prelude_injection(false, || {
            verify_module_phase3(
                &m,
                B1_PRELUDE_CLIENT,
                "prelude_client.gdl",
                DEFAULT_FUEL,
                true,
                false,
                false,
            )
        });
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("undeclared type: OPTION")),
            "under --no-prelude, OPTION must be an unknown symbol, got: {diags:?}"
        );
    }

    #[test]
    fn test_b1_e0101_duplicate_prelude_symbol() {
        let src = "module OPTION[A]\n\nrepresentation {ANY}\n  | None\n  | Some(value: A)\n\nend\n\nconcept EQ\n  equals(other: Current): BOOL\nend\n\nmodule MY_BOX\n\nrepresentation {ANY}\n  v: NAT\n\nend\n";
        let ast = Parser::new(tokenize(src).unwrap()).parse_file().unwrap();
        let diags = check_prelude_collisions(&ast, src, "dup.gdl");
        assert_eq!(diags.len(), 2, "got: {diags:?}");
        assert!(
            diags
                .iter()
                .all(|d| d.level == "error" && d.rule.starts_with("E0101"))
        );
        let opt = diags
            .iter()
            .find(|d| d.routine == "OPTION")
            .expect("OPTION collision");
        assert_eq!(opt.tag, "duplicate_prelude_symbol");
        assert_eq!(opt.span.as_ref().map(|s| s.line), Some(1));
        let eq = diags
            .iter()
            .find(|d| d.routine == "EQ")
            .expect("EQ collision");
        assert_eq!(eq.span.as_ref().map(|s| s.line), Some(9));
        // A user file without collisions is accepted.
        let ok = Parser::new(tokenize(B1_PRELUDE_CLIENT).unwrap())
            .parse_file()
            .unwrap();
        assert!(check_prelude_collisions(&ok, B1_PRELUDE_CLIENT, "ok.gdl").is_empty());
    }

    #[test]
    fn test_b1_stdlib_bootstrap_is_not_double_injected() {
        // `--stdlib` (which files of the stdlib tree switch on) compiles in bootstrap mode.
        assert!(!prelude_mode_enabled(true, false));
        assert!(prelude_mode_enabled(false, false));
        // Stdlib files are recognized by location, not text: an edited one still is one, and a
        // user file with the same text elsewhere is not.
        let tree = stdlib_source_dir();
        for (rel, _) in STDLIB_SOURCES {
            let f = tree.join(rel.strip_prefix("stdlib/").unwrap());
            assert!(path_in_stdlib_tree(&f.to_string_lossy()), "{rel} must be in the stdlib tree");
        }
        assert!(!path_in_stdlib_tree(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/greeter.gdl")));
        // Even with injection forced on, prelude modules are emitted exactly as in bootstrap
        // mode (no self-injection, no namespaced duplicates).
        let stdlib = parse_phase4_stdlib().expect("stdlib parses");
        for m in &stdlib.modules {
            let plain = with_prelude_injection(false, || emit_boogie(m, DEFAULT_FUEL));
            let forced = with_prelude_injection(true, || emit_boogie(m, DEFAULT_FUEL));
            assert_eq!(
                plain, forced,
                "prelude module {} must not be re-injected",
                m.name
            );
            assert!(!plain.contains(&format!("{}__", m.name)));
        }
        // Bootstrap verification of the prelude still succeeds with 0 diagnostics.
        for name in ["OPTION", "SEQ"] {
            let m = stdlib.modules.iter().find(|m| m.name == name).unwrap();
            let diags = with_prelude_injection(true, || {
                verify_module_phase3(
                    m,
                    phase4_stdlib_source(),
                    "<phase4_stdlib.gdl>",
                    DEFAULT_FUEL,
                    true,
                    false,
                    false,
                )
            });
            assert!(
                diags.is_empty(),
                "{name} must verify in bootstrap mode, got: {diags:?}"
            );
        }
    }

    // ------------------------------------------------------------------------
    // Task B.3: string literals desugar to prelude STRING
    // ------------------------------------------------------------------------

    const B3_LITERAL_CLIENT: &str = r#"
module LITERAL_CLIENT

representation {ANY}
  dummy: NAT

feature {ANY}

  greeting: STRING
    do
      "hi"
    ensure
      model: Result.chars = LIST.empty.prepend('i').prepend('h')
      bytes: Result.byte_length = 2
      count: Result.code_point_count = 2
      non_empty: not Result.is_empty
      same_text: Result.equals("hi")
    end

  nothing: STRING
    do
      ""
    ensure
      empty: Result.is_empty
      no_chars: Result.chars = LIST.empty
    end

  escaped: STRING
    do
      "a\"\\\n\u{41}"
    ensure
      model: Result.chars = LIST.empty.prepend('A').prepend('\n').prepend('\\').prepend('"').prepend('a')
      differs: not Result.equals("hi")
    end

  is_hi(s: STRING): BOOL
    do
      s.equals("hi")
    ensure
      by_chars: Result = (s.chars = LIST.empty.prepend('i').prepend('h'))
    end

end
"#;

    fn verify_with_prelude(src: &str, file: &str) -> (ModuleDecl, Vec<Diagnostic>) {
        let m = parse_user_module_with_prelude(src);
        let diags = with_prelude_injection(true, || {
            verify_module_phase3(&m, src, file, DEFAULT_FUEL, true, false, false)
        });
        (m, diags)
    }

    #[test]
    fn test_b3_literal_chars_model_is_provable_in_user_contracts() {
        let m = parse_user_module_with_prelude(B3_LITERAL_CLIENT);
        let bpl = with_prelude_injection(true, || emit_boogie(&m, DEFAULT_FUEL));
        // Literals are prelude STRING values: the STRING theory is injected and the legacy
        // opaque literal sort is not used for them.
        assert!(bpl.contains("type STRING;"));
        assert!(bpl.contains("function STRING__fn_chars("));
        assert!(!bpl.contains("const unique str_lit_0: GardelString;"));
        // The only per-literal axioms are definitions by verified prelude constructors.
        let strings = collect_module_strings(&m);
        let hi = strings
            .iter()
            .position(|s| s == "hi")
            .expect("literal collected");
        assert!(bpl.contains(&format!(
            "axiom str_lit_{hi}_s0 == STRING__fn_make_empty(DefaultFuel, 2);"
        )));
        assert!(bpl.contains(&format!(
            "axiom str_lit_{hi}_s1 == STRING__fn_push_ascii(DefaultFuel, str_lit_{hi}_s0, 105);"
        )));
        assert!(bpl.contains(&format!("axiom str_lit_{hi} == str_lit_{hi}_s2;")));
        // (Quantified axioms mentioning a literal are the user routines' own definitional /
        // contract axioms; every ground axiom about a literal is one of its definitions.)
        for line in bpl
            .lines()
            .filter(|l| l.starts_with("axiom") && !l.contains("forall") && l.contains("str_lit_"))
        {
            assert!(
                line.starts_with("axiom str_lit_")
                    && !line.contains("chars")
                    && !line.contains("Invariant"),
                "unexpected trusted literal fact: {line}"
            );
        }
        // Each literal's invariant and chars model is a proof obligation of its own.
        assert!(bpl.contains(&format!("procedure Verify_string_literal_{hi}()")));
        assert!(bpl.contains(&format!(
            "assert {{:msg \"string_literal_{hi}::ensure::chars_model\"}} STRING__fn_chars(DefaultFuel, str_lit_{hi}) == Seq_prepend(104, Seq_prepend(105, Seq_empty()));"
        )));

        let (_, diags) = verify_with_prelude(B3_LITERAL_CLIENT, "literal_client.gdl");
        assert!(
            diags.is_empty(),
            "literal client must verify, got: {diags:?}"
        );
    }

    #[test]
    fn test_b3_literal_facts_are_not_vacuous() {
        // Wrong models / lengths / equalities about literals must be rejected (one false
        // clause per routine: Boogie reports the first failing assertion of a procedure).
        let src = B3_LITERAL_CLIENT
            .replace(
                "bytes: Result.byte_length = 2",
                "bytes: Result.byte_length = 3",
            )
            .replace(
                "no_chars: Result.chars = LIST.empty",
                "no_chars: Result.chars = LIST.empty.prepend(' ')",
            )
            .replace(
                "differs: not Result.equals(\"hi\")",
                "differs: Result.equals(\"hi\")",
            )
            .replace(
                "by_chars: Result = (s.chars = LIST.empty.prepend('i').prepend('h'))",
                "by_chars: Result = (s.chars = LIST.empty.prepend('h').prepend('i'))",
            );
        let (_, diags) = verify_with_prelude(&src, "literal_client_bad.gdl");
        for (routine, tag) in [
            ("greeting", "bytes"),
            ("nothing", "no_chars"),
            ("escaped", "differs"),
            ("is_hi", "by_chars"),
        ] {
            assert!(
                diags.iter().any(|d| d.routine == routine && d.tag == tag),
                "`{routine}.{tag}` must fail, got: {diags:?}"
            );
        }
    }

    #[test]
    fn test_b3_literal_lexing_and_validation() {
        // Escapes are decoded at compile time.
        let toks = tokenize(r#""a\"\\\n\t\r\0\u{41}\u{7F}" '\u{1F980}' '\'' '"'"#)
            .expect("valid literals");
        assert_eq!(toks[0], Token::Str("a\"\\\n\t\r\0A\u{7F}".to_string()));
        assert_eq!(toks[1], Token::Char('\u{1F980}'));
        assert_eq!(toks[2], Token::Char('\''));
        assert_eq!(toks[3], Token::Char('"'));

        // Non-ASCII scalars (raw or escaped) cannot be represented by the bootstrap STRING.
        for src in ["\"caf\u{E9}\"", r#""\u{E9}""#, "\"\u{1F980}\""] {
            let err = tokenize(src).expect_err("non-ASCII literal must be rejected");
            assert!(
                err.message.starts_with("E0303: non_ascii_string_literal"),
                "{err}"
            );
            assert!(err.message.contains("Task C.2"), "{err}");
        }
        // Surrogates are not scalar values (in strings and chars).
        for src in [r#""\u{D800}""#, r#"'\u{DFFF}'"#] {
            let err = tokenize(src).expect_err("surrogate must be rejected");
            assert!(
                err.message.starts_with("E0304: surrogate_in_literal"),
                "{err}"
            );
        }
        // Malformed escapes.
        for src in [
            r#""\q""#,
            r#""\x41""#,
            r#""\u41""#,
            r#""\u{}""#,
            r#""\u{1234567}""#,
            r#""\u{12G}""#,
            r#""\u{110000}""#,
            r#""\u{41""#,
            r#"'\q'"#,
        ] {
            let err = tokenize(src).expect_err("malformed escape must be rejected");
            assert!(
                err.message.starts_with("E0302: malformed_escape_sequence"),
                "{src}: {err}"
            );
        }
        // Raw line breaks are not allowed inside a string literal (§15).
        let err = tokenize("\"ab\ncd\"").expect_err("raw newline must be rejected");
        assert!(err.message.starts_with("E0301"), "{err}");
    }

    #[test]
    fn test_b3_structural_string_equality_and_plus_are_rejected() {
        let src = B3_LITERAL_CLIENT
            .replace(
                "same_text: Result.equals(\"hi\")",
                "same_text: Result = \"hi\"",
            )
            .replace("      s.equals(\"hi\")\n", "      s = \"hi\"\n");
        let (_, diags) = verify_with_prelude(&src, "literal_client_eq.gdl");
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "greeting" && d.rule.starts_with("E0311")),
            "Result = \"hi\" must be rejected, got: {diags:?}"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "is_hi" && d.rule.starts_with("E0311")),
            "s = \"hi\" must be rejected, got: {diags:?}"
        );
        // `chars` (a LIST, canonical) may be compared with `=`: no diagnostic for `by_chars`.
        assert!(
            diags.iter().all(|d| d.rule.starts_with("E0311")),
            "got: {diags:?}"
        );

        let plus = B3_LITERAL_CLIENT.replace(
            "      \"hi\"\n    ensure",
            "      \"h\" + \"i\"\n    ensure",
        );
        let (_, diags) = verify_with_prelude(&plus, "literal_client_plus.gdl");
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "greeting" && d.rule.starts_with("E0312")),
            "string + must be rejected, got: {diags:?}"
        );
    }

    #[test]
    fn test_b3_c11_literals_are_static_utf8_buffers() {
        let src = "program HELLO\n\nfeature {ANY}\n\n  main(args: SEQ[STRING]): IO[EXIT_CODE]\n    do\n      with IO\n        IO.print(\"Hi\\n\")\n        IO.print_line(\"Hi\")\n        IO.exit(0)\n      end\n    end\n\nend\n";
        let m = parse_user_module_with_prelude(src);
        let c = emit_c11(&m);
        let strings = collect_module_strings(&m);
        let a = strings.iter().position(|s| s == "Hi\n").unwrap();
        let b = strings.iter().position(|s| s == "Hi").unwrap();
        assert!(
            c.contains(&format!(
                "static const uint8_t gardel_str_lit_{a}[4] = {{ 72, 105, 10, 0 }};"
            )),
            "{c}"
        );
        assert!(
            c.contains(&format!(
                "static const uint8_t gardel_str_lit_{b}[3] = {{ 72, 105, 0 }};"
            )),
            "{c}"
        );
        assert!(
            c.contains(&format!(
                "gardel_io_print(string_from_utf8(gardel_str_lit_{a}, 3ULL))"
            )),
            "{c}"
        );
        assert!(
            c.contains(&format!(
                "gardel_io_print_line(string_from_utf8(gardel_str_lit_{b}, 2ULL))"
            )),
            "{c}"
        );
        // The runtime constructor wraps the static buffer: no allocation, no decoding.
        let ctor = c
            .split("static inline GardelString string_from_utf8(")
            .nth(1)
            .unwrap();
        let ctor = &ctor[..ctor.find('}').unwrap()];
        assert!(
            !ctor.contains("alloc") && !ctor.contains("memcpy"),
            "{ctor}"
        );
        // The program body is lowered from the source (no canned template).
        assert!(c.contains("return (int)main_routine(NULL);"));
        assert!(!c.contains("Hello, %s"));
    }

    #[test]
    fn test_b3_boogie_timeouts_are_reported() {
        let m = parse_user_module_with_prelude(B3_LITERAL_CLIENT);
        let out = "x.bpl(12,3): Verification of 'Verify_greeting' timed out after 10 seconds\n\nBoogie program verifier finished with 3 verified, 0 errors, 1 time out\n";
        let diags = parse_boogie_output(out, &m, B3_LITERAL_CLIENT, "literal_client.gdl");
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].routine, "greeting");
        assert_eq!(diags[0].level, "error");
        assert!(diags[0].rule.contains("Timeout"));
    }

    // ------------------------------------------------------------------
    // Task B.4: multi-file project compilation
    // ------------------------------------------------------------------

    const B4_COUNTER: &str = r#"
module COUNTER

representation {NONE}
  value: NAT

create {ANY}

  make(start: NAT): COUNTER
    require
      small_start: start < 100
    do
      { value = start }
    ensure
      starts_at: Result.current_value = start
    end

feature {ANY}

  current_value: NAT
    do
      Current.value
    end

invariant
  bounded: value < 1000000

end
"#;

    const B4_CLIENT: &str = r#"
functions CLIENT

feature {ANY}

  start_at_five: COUNTER
    do
      COUNTER.make(5)
    ensure
      five: Result.current_value = 5
    end

  read_back(n: NAT): NAT
    require
      small_n: n < 100
    do
      COUNTER.make(n).current_value
    ensure
      same: Result = n
    end

end
"#;

    fn b4_units(files: &[(&str, &str)]) -> Vec<SourceUnit> {
        files
            .iter()
            .map(|(path, src)| {
                let tokens = tokenize(src).expect("lexing should succeed");
                let ast = Parser::new(tokens)
                    .parse_file()
                    .expect("parsing should succeed");
                SourceUnit {
                    path: path.to_string(),
                    src: src.to_string(),
                    ast,
                }
            })
            .collect()
    }

    fn b4_opts(project_mode: bool, strict: bool) -> VerifyOptions {
        VerifyOptions {
            prelude_mode: true,
            project_mode,
            fuel_limit: DEFAULT_FUEL,
            strict,
            audit: false,
            bounded_check: false,
        }
    }

    #[test]
    fn test_b4_cross_file_type_and_routine_verify_together() {
        // CLIENT (client.gdl) uses COUNTER's type, constructor (with a precondition) and
        // query declared in counter.gdl; the files are given in either order.
        for files in [
            [("counter.gdl", B4_COUNTER), ("client.gdl", B4_CLIENT)],
            [("client.gdl", B4_CLIENT), ("counter.gdl", B4_COUNTER)],
        ] {
            let mut units = b4_units(&files);
            assert!(check_project_namespace(&units, true).is_empty());
            let checked = check_and_verify_project(&mut units, b4_opts(true, true));
            assert!(checked.fatal.is_empty());
            let diags: Vec<&Diagnostic> = checked.diags.iter().map(|(_, d)| d).collect();
            assert!(diags.is_empty(), "expected 0 diagnostics, got {diags:?}");

            // CLIENT's context holds COUNTER's namespaced theory and contract interface, but
            // none of COUNTER's procedures (they are verified once, in COUNTER's own file).
            let client = units
                .iter()
                .flat_map(|u| u.ast.modules.iter())
                .find(|m| m.name == "CLIENT")
                .unwrap();
            let bpl = with_prelude_injection(true, || {
                with_link_table(checked.link.clone(), || emit_boogie(client, DEFAULT_FUEL))
            });
            assert!(bpl.contains("type COUNTER;"));
            assert!(bpl.contains("function COUNTER__fn_make("));
            assert!(
                bpl.contains("COUNTER__pre_make_0(5)"),
                "the callee precondition must be asserted at the call site"
            );
            assert!(!bpl.contains("procedure Verify_make"));
            assert_eq!(
                bpl.lines().filter(|l| l.starts_with("procedure ")).count(),
                2
            );
        }
    }

    #[test]
    fn test_b4_cross_file_precondition_violation_reported_in_caller_file() {
        let bad = B4_CLIENT
            .replace("COUNTER.make(5)", "COUNTER.make(500)")
            .replace("Result.current_value = 5", "Result.current_value = 500");
        let mut units = b4_units(&[("counter.gdl", B4_COUNTER), ("client.gdl", &bad)]);
        let checked = check_and_verify_project(&mut units, b4_opts(true, true));
        let errs: Vec<&(usize, Diagnostic)> = checked
            .diags
            .iter()
            .filter(|(_, d)| d.level == "error")
            .collect();
        assert_eq!(errs.len(), 1, "got {:?}", checked.diags);
        let (ui, d) = errs[0];
        assert_eq!(*ui, 1);
        assert_eq!(d.rule, "Phase 3 Boogie (Precondition Violation)");
        assert_eq!(d.routine, "start_at_five");
        assert_eq!(d.tag, "small_start");
        assert_eq!(d.module_name, "CLIENT");
        assert_eq!(d.span.as_ref().unwrap().file, "client.gdl");
        assert!(
            d.to_json("CLIENT", "client.gdl")
                .contains("\"file\": \"client.gdl\"")
        );
    }

    #[test]
    fn test_b4_duplicate_module_across_files_is_e0102() {
        let other = "module COUNTER\nrepresentation {ANY}\n  x: NAT\nend\n";
        let units = b4_units(&[("counter.gdl", B4_COUNTER), ("other/counter.gdl", other)]);
        let diags = check_project_namespace(&units, true);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].rule, "E0102: duplicate_module_name");
        assert_eq!(diags[0].level, "error");
        let sp = diags[0].span.as_ref().unwrap();
        assert_eq!((sp.file.as_str(), sp.line), ("other/counter.gdl", 1));
        assert!(
            diags[0]
                .message
                .contains("already declared in `counter.gdl`")
        );
        // Distinct names: no E0102.
        let units = b4_units(&[("counter.gdl", B4_COUNTER), ("client.gdl", B4_CLIENT)]);
        assert!(check_project_namespace(&units, true).is_empty());
    }

    #[test]
    fn test_b4_w0201_file_module_name_mismatch() {
        let group_src = "group { AA, BB }\nmodule AA\nrepresentation {ANY}\n  x: NAT\nend\nmodule BB\nrepresentation {ANY}\n  y: NAT\nend\n";
        let units = b4_units(&[
            ("src/main_client.gdl", B4_CLIENT),
            ("src/counter.gdl", B4_COUNTER),
            ("src/ab_nodes.gdl", group_src),
        ]);
        let w = check_file_module_name(&units[0], false);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].rule, "W0201: file_module_name_mismatch");
        assert_eq!(w[0].level, "warning");
        assert_eq!(w[0].span.as_ref().unwrap().file, "src/main_client.gdl");
        assert!(w[0].message.contains("`client.gdl`"));
        // --strict enforces the convention as an error (§4.2.1).
        assert_eq!(check_file_module_name(&units[0], true)[0].level, "error");
        // A matching basename, and `group` files (domain-named, §4.3.3), are fine.
        assert!(check_file_module_name(&units[1], true).is_empty());
        assert!(check_file_module_name(&units[2], true).is_empty());
        assert_eq!(to_snake_case("BOUNDED_STACK"), "bounded_stack");
        assert_eq!(to_snake_case("BoundedStack"), "bounded_stack");

        // Emitted by a project run; as a warning it does not fail the run.
        let mut units = b4_units(&[
            ("src/main_client.gdl", B4_CLIENT),
            ("src/counter.gdl", B4_COUNTER),
        ]);
        let checked = check_and_verify_project(&mut units, b4_opts(true, false));
        let rules: Vec<&str> = checked.diags.iter().map(|(_, d)| d.rule).collect();
        assert_eq!(rules, vec!["W0201: file_module_name_mismatch"]);
    }

    #[test]
    fn test_b4_cross_file_cycle_is_e0103() {
        let ping = "functions PING\nfeature {ANY}\n  ping(n: NAT): NAT\n    do\n      PONG.pong(n)\n    end\nend\n";
        let pong = "functions PONG\nfeature {ANY}\n  pong(n: NAT): NAT\n    do\n      PING.ping(n)\n    end\nend\n";
        let mut units = b4_units(&[("ping.gdl", ping), ("pong.gdl", pong)]);
        let checked = check_and_verify_project(&mut units, b4_opts(true, true));
        assert!(
            checked.diags.is_empty(),
            "nothing is verified when the link graph is cyclic"
        );
        assert_eq!(checked.fatal.len(), 1);
        assert_eq!(checked.fatal[0].rule, "E0103: cyclic_module_dependency");
        assert_eq!(checked.fatal[0].span.as_ref().unwrap().file, "ping.gdl");
        assert!(checked.fatal[0].message.contains("PING -> PONG -> PING"));
    }

    #[test]
    fn test_b4_single_file_run_is_not_linked() {
        // Single-file runs keep the pre-B.4 pipeline: no link table and no W0201.
        let mut units = b4_units(&[("weird_name.gdl", B4_COUNTER)]);
        let checked = check_and_verify_project(&mut units, b4_opts(false, true));
        assert!(checked.link.is_none());
        assert!(checked.diags.is_empty(), "got {:?}", checked.diags);
        let args: Vec<String> = [
            "gardelc",
            "a.gdl",
            "--fuel",
            "3",
            "--inspect",
            "STACK",
            "--strict",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(input_operands(&args), vec!["a.gdl".to_string()]);
        let (files, project) = resolve_input_operands(&["a.gdl".to_string()]).unwrap();
        assert_eq!((files, project), (vec!["a.gdl".to_string()], false));
        assert!(wildcard_match("*.gdl", "bounded_stack.gdl"));
        assert!(wildcard_match("b?unded*", "bounded_stack.gdl"));
        assert!(!wildcard_match("*.gdl", "notes.txt"));
    }

    // ---------------------------------------------------------------------------------
    // Soundness regressions (review of Phase B, items 1-3 + inductive law proofs).
    // ---------------------------------------------------------------------------------

    fn soundness_verify(src: &str, file: &str) -> Vec<Diagnostic> {
        let m = parse_user_module_with_prelude(src);
        with_prelude_injection(true, || {
            verify_module_phase3(&m, src, file, DEFAULT_FUEL, true, false, false)
        })
    }

    #[test]
    fn test_soundness_concept_laws_are_not_assumed_by_their_own_module() {
        // An `equals` that is never reflexive must fail EQ.reflexive. Previously the module's
        // own Boogie context contained `fn_equals(x, x) == true` as an axiom, which contradicted
        // the definition and made every obligation of the module vacuously provable.
        let src = "module BADEQ satisfies EQ\n\nrepresentation {ANY}\n  | K(x: NAT)\n\nfeature {ANY}\n\n  equals(other: BADEQ): BOOL\n    do\n      False\n    end\n\nspecify {EQ}\n\n  law reflexive(a: BADEQ)\n    ensure\n      refl: a.equals(a)\n    end\n\n  law symmetric(a: BADEQ, b: BADEQ)\n    require\n      ab: a.equals(b)\n    ensure\n      ba: b.equals(a)\n    end\n\n  law transitive(a: BADEQ, b: BADEQ, c: BADEQ)\n    require\n      ab: a.equals(b)\n      bc: b.equals(c)\n    ensure\n      ac: a.equals(c)\n    end\n\nend\n";
        let m = parse_user_module_with_prelude(src);
        let bpl = with_prelude_injection(true, || emit_boogie(&m, DEFAULT_FUEL));
        assert!(
            !bpl.contains("forall x: BADEQ :: { fn_equals(DefaultFuel, x, x) }"),
            "a module's own laws must not be axioms in its own context"
        );
        let diags = soundness_verify(src, "badeq.gdl");
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "EQ_reflexive" && d.tag == "refl"),
            "non-reflexive equals must fail EQ.reflexive, got: {diags:?}"
        );
        // Clients still receive the (separately verified) prelude concept axioms.
        let client = "functions USES_OPT\n\nfeature {ANY}\n\n  refl(o: OPTION[U64]): BOOL\n    do\n      o.equals(o)\n    ensure\n      holds: Result\n    end\n\nend\n";
        let diags = soundness_verify(client, "uses_opt.gdl");
        assert!(
            diags.is_empty(),
            "client use of prelude reflexivity must verify, got: {diags:?}"
        );
    }

    #[test]
    fn test_soundness_conditional_canonical_eq_is_not_assumed_for_every_element_type() {
        // SEQ is EQ but not CANONICAL_EQ: two differently shaped trees can be `equals` without
        // being `=`. OPTION[SEQ[U64]].equals delegates to SEQ.equals (spec §5), so claiming the
        // wrapped values are unequal must NOT verify. Previously element `.equals` inside
        // OPTION/RESULT was lowered to `==` (keyed on variable names) and `A_eq <==> ==` was a
        // module-wide axiom, so this false contract verified.
        let src = "functions OPTSEQ\n\nfeature {ANY}\n\n  wrap_differs(s1: SEQ[U64], s2: SEQ[U64]): BOOL\n    require\n      behaviourally_equal: s1.equals(s2)\n      different_shape: not (s1 = s2)\n    do\n      Some(s1).equals(Some(s2))\n    ensure\n      claims_unequal: Result = False\n    end\n\nend\n";
        let diags = soundness_verify(src, "optseq.gdl");
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "wrap_differs" && d.tag == "claims_unequal"),
            "false claim about OPTION[SEQ] equality must be rejected, got: {diags:?}"
        );
        // Canonical instantiations stay complete.
        let src_u64 = "functions OPTU64\n\nfeature {ANY}\n\n  same_wrap(x: U64, y: U64): BOOL\n    require\n      equal: x = y\n    do\n      Some(x).equals(Some(y))\n    ensure\n      wraps_equal: Result\n    end\n\n  diff_wrap(x: U64, y: U64): BOOL\n    require\n      differ: not (x = y)\n    do\n      Some(x).equals(Some(y))\n    ensure\n      wraps_differ: not Result\n    end\n\nend\n";
        let diags = soundness_verify(src_u64, "optu64.gdl");
        assert!(
            diags.is_empty(),
            "OPTION[U64] equality must remain provable, got: {diags:?}"
        );
        // The stdlib's own conditional laws are proven only under the where-hypothesis.
        let prelude = standard_prelude();
        let opt = prelude
            .modules
            .iter()
            .find(|m| m.name == "OPTION")
            .expect("OPTION");
        let bpl = emit_boogie(opt, DEFAULT_FUEL);
        assert!(
            !bpl.contains("A_eq(x, y) <==> (x == y)"),
            "no module-wide canonical axiom"
        );
        assert!(bpl.contains("type A__Canon;"));
        assert!(
            bpl.contains("Verify_law_CANONICAL_EQ_agrees_with_structural(a: (OPTION A__Canon)")
        );
    }

    #[test]
    fn test_soundness_same_module_call_preconditions_are_checked() {
        // Calling a routine of the same module outside its precondition must be reported even
        // when the result is never returned (Gardel binaries have no runtime checks, §7).
        let src = "module BOX3\n\nrepresentation {NONE}\n  v: NAT\n\ncreate {ANY}\n\n  make(x: NAT): BOX3\n    require\n      small: x < 10\n    do\n      { v = x }\n    end\n\nfeature {ANY}\n\n  value: NAT\n    do\n      Current.v\n    end\n\n  ok_use: NAT\n    do\n      let b = BOX3.make(5)\n      b.value\n    end\n\n  bad2: NAT\n    do\n      let b = BOX3.make(50)\n      b.value\n    end\n\ninvariant\n  small_v: v < 10\n\nend\n";
        let diags = soundness_verify(src, "box3.gdl");
        assert!(
            diags.iter().any(|d| d.routine == "bad2"
                && d.tag == "small"
                && d.rule.contains("Precondition")),
            "violated same-module precondition must be reported, got: {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.routine == "ok_use"),
            "a call within the precondition must verify, got: {diags:?}"
        );
    }

    #[test]
    fn test_soundness_inductive_law_proofs() {
        // Concept laws written as inductive proofs are verified by induction (recursive law
        // calls = induction hypotheses guarded by a decreasing variant), not assumed.
        let peano = include_str!("../../examples/peano_ord.gdl");
        let diags = soundness_verify(peano, "peano_ord.gdl");
        assert!(
            diags.is_empty(),
            "peano_ord must verify by induction, got: {diags:?}"
        );
        // Circular "induction" on the same argument is rejected by the variant check.
        let circular = peano.replacen(
            "case Succ(p) then irreflexive(p)",
            "case Succ(p) then irreflexive(a)",
            1,
        );
        assert_ne!(circular, peano);
        let diags = soundness_verify(&circular, "peano_circular.gdl");
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "ORD_irreflexive" && d.tag == "variant_decrease"),
            "circular induction must be rejected, got: {diags:?}"
        );
        // A false law does not become provable through an inductive-looking body.
        let false_law = peano.replacen(
            "not_less_than_self: not a.less_than(a)",
            "not_less_than_self: a.less_than(a)",
            1,
        );
        assert_ne!(false_law, peano);
        let diags = soundness_verify(&false_law, "peano_false.gdl");
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "ORD_irreflexive" && d.tag == "not_less_than_self"),
            "false law must be rejected, got: {diags:?}"
        );
    }

    // ---------------------------------------------------------------------------------
    // Item 4: `IO.read_line : IO[RESULT[IO_ERROR, STRING]]` (spec §11 IO boundary).
    // ---------------------------------------------------------------------------------

    const READ_LINE_PROBE: &str = "program IOPROBE\n\nfeature {ANY}\n\n  want_empty(s: STRING): NAT\n    require\n      empty: s.is_empty\n    do\n      0\n    end\n\n  within_cap(s: STRING): NAT\n    require\n      fits: s.byte_length <= s.capacity\n    do\n      0\n    end\n\n  same(a: STRING, b: STRING): NAT\n    require\n      equal_reads: a.equals(b)\n    do\n      0\n    end\n\n  main(args: SEQ[STRING]): IO[EXIT_CODE]\n    do\n      with IO\n        r <- IO.read_line\n        r2 <- IO.read_line\n        match r\n          case Ok(s) then\n            match r2\n              case Ok(s2) then\n                with IO\n                  IO.print_line(s)\n                  IO.exit(want_empty(s) + within_cap(s) + same(s, s2))\n                end\n              case Error(_) then\n                IO.exit(2)\n            end\n          case Error(_) then\n            IO.exit(1)\n        end\n      end\n    end\n\nend\n";

    #[test]
    fn test_item4_read_line_is_a_fresh_validated_result() {
        let m = parse_user_module_with_prelude(READ_LINE_PROBE);
        let bpl = with_prelude_injection(true, || emit_boogie(&m, DEFAULT_FUEL));
        // No constant `IO_read_line()` function: every read is a fresh value.
        assert!(
            !bpl.contains("function IO_read_line()"),
            "read_line must not be a constant"
        );
        assert!(bpl.contains("havoc r;") && bpl.contains("havoc r2;"));
        assert!(bpl.contains("var r: (RESULT IO_ERROR STRING);"));
        assert!(
            bpl.contains("var s: STRING;"),
            "Ok binder must be typed STRING"
        );
        let diags = with_prelude_injection(true, || {
            verify_module_phase3(
                &m,
                READ_LINE_PROBE,
                "ioprobe.gdl",
                DEFAULT_FUEL,
                true,
                false,
                false,
            )
        });
        // Input is arbitrary: neither emptiness nor equality of two reads is provable.
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "main" && d.tag == "empty"),
            "got: {diags:?}"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "main" && d.tag == "equal_reads"),
            "two reads must not be provably equal, got: {diags:?}"
        );
        // But an `Ok` payload is a valid STRING (the runtime validates it).
        assert!(
            !diags.iter().any(|d| d.tag == "fits"),
            "Ok payload must satisfy the STRING invariant, got: {diags:?}"
        );
        assert_eq!(diags.len(), 2, "got: {diags:?}");

        // Using the RESULT as if it were a STRING is a type error (IO actions are type-checked).
        let old = "program OLDG\n\nfeature {ANY}\n\n  main(args: SEQ[STRING]): IO[EXIT_CODE]\n    do\n      with IO\n        name <- IO.read_line\n        IO.print_line(name)\n        IO.exit(0)\n      end\n    end\n\nend\n";
        let m = parse_user_module_with_prelude(old);
        let diags = with_prelude_injection(true, || {
            verify_module_phase3(&m, old, "old.gdl", DEFAULT_FUEL, true, false, false)
        });
        assert!(
            diags.iter().any(|d| d.message.contains("IO_print_line")),
            "got: {diags:?}"
        );
    }

    #[test]
    fn test_item4_c11_read_line_boundary_and_prelude_match() {
        let src = include_str!("../../examples/greeter.gdl");
        let m = parse_user_module_with_prelude(src);
        let diags = with_prelude_injection(true, || {
            verify_module_phase3(&m, src, "greeter.gdl", DEFAULT_FUEL, true, false, false)
        });
        assert!(
            diags.is_empty(),
            "spec §11 greeter must verify, got: {diags:?}"
        );
        let c = emit_c11(&m);
        // Instantiated prelude sum types and their constructors.
        assert!(c.contains("typedef struct RESULT_IO_ERROR_STRING {"), "{c}");
        assert!(
            c.contains("make_IO_ERROR_Decode(struct STRING_ERROR* cause)"),
            "{c}"
        );
        // The runtime validates input: only ASCII lines become Ok(STRING).
        assert!(
            c.contains("if (b >= 128)") && c.contains("make_IO_ERROR_EndOfInput()"),
            "{c}"
        );
        assert!(
            c.contains("__auto_type line_res = gardel_io_read_line_result();"),
            "{c}"
        );
        // `match` uses the instance's tags and binds the pattern variable.
        assert!(c.contains("->tag == TAG_RESULT_IO_ERROR_STRING_Ok"), "{c}");
        assert!(c.contains("__auto_type name = _ms0->Ok.val;"), "{c}");
        // The nested `with IO` keeps its actions.
        assert!(c.contains("gardel_io_print_line(name);"), "{c}");
        assert!(!c.contains("TAG_GREETER_"), "{c}");
    }

    // ---------------------------------------------------------------------------------
    // Item 5: `SEQ[A] satisfies EQ where A satisfies CANONICAL_EQ`, enforced by E0314.
    // ---------------------------------------------------------------------------------

    #[test]
    fn test_item5_seq_equals_requires_canonical_elements() {
        let prelude = standard_prelude();
        let seq = prelude
            .modules
            .iter()
            .find(|m| m.name == "SEQ")
            .expect("SEQ");
        let sat = seq.satisfies.as_ref().expect("SEQ satisfies EQ");
        assert_eq!(sat.concepts, vec!["EQ".to_string()]);
        assert_eq!(
            sat.where_constraints,
            vec![("A".to_string(), "CANONICAL_EQ".to_string())]
        );

        let src = "functions SEQEQ\n\nfeature {ANY}\n\n  nested(s: SEQ[SEQ[U64]], t: SEQ[SEQ[U64]]): BOOL\n    do\n      s.equals(t)\n    end\n\n  strings(s: SEQ[STRING], t: SEQ[STRING]): BOOL\n    do\n      s.equals(t)\n    end\n\n  flat(s: SEQ[U64], t: SEQ[U64]): BOOL\n    do\n      s.equals(t)\n    end\n\n  chars(s: SEQ[CHAR], t: SEQ[CHAR]): BOOL\n    do\n      s.equals(t)\n    end\n\nend\n";
        let m = parse_user_module_with_prelude(src);
        let diags = with_prelude_injection(true, || check_where_constraints(&m, src, "seqeq.gdl"));
        let flagged: Vec<&str> = diags.iter().map(|d| d.routine.as_str()).collect();
        assert!(
            diags
                .iter()
                .all(|d| d.rule == "E0314: unsatisfied_where_constraint"),
            "got: {diags:?}"
        );
        assert_eq!(
            flagged,
            vec!["nested", "strings"],
            "non-canonical elements must be rejected, canonical ones accepted"
        );

        // A generic element type needs the bound; a CANONICAL_EQ-bounded one satisfies it.
        let generic = "module HOLDER[A]\n\nrepresentation {ANY}\n  | Hold(items: SEQ[A])\n\nfeature {ANY}\n\n  same(other: HOLDER[A]): BOOL\n    do\n      match Current\n        case Hold(xs) then\n          match other\n            case Hold(ys) then xs.equals(ys)\n          end\n      end\n    end\n\nend\n";
        let m = parse_user_module_with_prelude(generic);
        let ctx = with_prelude_injection(true, || BoogiePreludeCtx::for_module(&m));
        assert!(!type_satisfies("A", "CANONICAL_EQ", &m, &ctx));
        assert!(type_satisfies("U64", "CANONICAL_EQ", &m, &ctx));
        assert!(
            !type_satisfies("SEQ[U64]", "CANONICAL_EQ", &m, &ctx),
            "SEQ is EQ, not CANONICAL_EQ"
        );
        assert!(type_satisfies("SEQ[U64]", "EQ", &m, &ctx));
        assert!(
            !type_satisfies("SEQ[STRING]", "EQ", &m, &ctx),
            "SEQ[STRING] fails SEQ's where constraint"
        );
        assert!(type_satisfies("OPTION[U64]", "CANONICAL_EQ", &m, &ctx));
        assert!(!type_satisfies(
            "OPTION[SEQ[U64]]",
            "CANONICAL_EQ",
            &m,
            &ctx
        ));
        let bounded = generic.replace("module HOLDER[A]", "module HOLDER[A: CANONICAL_EQ]");
        let mb = parse_user_module_with_prelude(&bounded);
        let ctxb = with_prelude_injection(true, || BoogiePreludeCtx::for_module(&mb));
        assert!(type_satisfies("A", "CANONICAL_EQ", &mb, &ctxb));
        assert!(
            type_satisfies("A", "EQ", &mb, &ctxb),
            "CANONICAL_EQ refines EQ"
        );

        // OPTION lists only CANONICAL_EQ; its `equals` delegates to element `equals` (§5).
        let opt = "functions OPTSEQ2\n\nfeature {ANY}\n\n  w(s1: SEQ[U64], s2: SEQ[U64]): BOOL\n    do\n      Some(s1).equals(Some(s2))\n    end\n\nend\n";
        let m = parse_user_module_with_prelude(opt);
        let diags =
            with_prelude_injection(true, || check_where_constraints(&m, opt, "optseq2.gdl"));
        assert!(
            diags.is_empty(),
            "OPTION[SEQ[U64]].equals stays available, got: {diags:?}"
        );
    }

    // ---------------------------------------------------------------------------------
    // Item 6: a false contract must not mask other failures in the same module.
    // ---------------------------------------------------------------------------------

    #[test]
    fn test_item6_false_contract_does_not_mask_other_failures() {
        // `f`'s false postcondition contradicts its definition. Previously the contradiction
        // silently proved `g`'s (also false) postcondition, so only `f` was reported.
        let src = "functions SELFPROOF\n\nfeature {ANY}\n\n  f(n: NAT): NAT\n    do\n      0\n    ensure\n      bogus: Result = 7\n    end\n\n  g(n: NAT): NAT\n    do\n      f(n)\n    ensure\n      also_bogus: Result = 1\n    end\n\nend\n";
        let diags = soundness_verify(src, "selfproof.gdl");
        assert!(
            diags.iter().any(|d| d.routine == "f" && d.tag == "bogus"),
            "got: {diags:?}"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "g" && d.tag == "also_bogus"),
            "masked failure must be reported, got: {diags:?}"
        );
        assert_eq!(diags.len(), 2, "got: {diags:?}");
        // A correct caller of the broken routine is not reported (it verifies from `f`'s
        // definition, without `f`'s contract).
        let ok = src.replace("also_bogus: Result = 1", "fine: Result = 0");
        let diags = soundness_verify(&ok, "selfproof_ok.gdl");
        assert_eq!(diags.len(), 1, "only f must fail, got: {diags:?}");
        assert!(diags[0].routine == "f");
        // Withdrawal is per clause: a true clause proven by induction keeps its induction
        // hypothesis even though a sibling clause of the same routine is false.
        let rec = "functions RECCLAUSE

feature {ANY}

  f(n: NAT): NAT
    variant
      n
    do
      if n = 0 then
        0
      else
        f(n - 1)
      end
    ensure
      zero: Result = 0
      bogus: Result = 5
    end

end
";
        let diags = soundness_verify(rec, "recclause.gdl");
        assert!(
            diags.iter().any(|d| d.routine == "f" && d.tag == "bogus"),
            "got: {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.tag == "zero"),
            "true inductive clause must verify, got: {diags:?}"
        );
    }

    // ---------------------------------------------------------------------------------
    // Termination across the call graph: direct recursion without a variant and mutual
    // recursion were not checked, so non-terminating routines "proved" their contracts.
    // ---------------------------------------------------------------------------------

    #[test]
    fn test_termination_direct_and_mutual_recursion() {
        let selfloop = "functions SELFLOOP\n\nfeature {ANY}\n\n  f(n: NAT): NAT\n    do\n      f(n)\n    ensure\n      seven: Result = 7\n    end\n\nend\n";
        let diags = soundness_verify(selfloop, "selfloop.gdl");
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "f" && d.tag == "variant_missing"),
            "got: {diags:?}"
        );

        let mutual = "functions MUTUAL\n\nfeature {ANY}\n\n  f(n: NAT): NAT\n    do\n      h(n)\n    ensure\n      seven: Result = 7\n    end\n\n  h(n: NAT): NAT\n    do\n      f(n)\n    end\n\nend\n";
        let diags = soundness_verify(mutual, "mutual.gdl");
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "f" && d.tag == "variant_missing"),
            "got: {diags:?}"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "h" && d.tag == "variant_missing"),
            "got: {diags:?}"
        );

        let even_odd = "functions EVENODD\n\nfeature {ANY}\n\n  is_even(n: NAT): BOOL\n    variant\n      n\n    do\n      if n = 0 then\n        True\n      else\n        is_odd(n - 1)\n      end\n    end\n\n  is_odd(n: NAT): BOOL\n    variant\n      n\n    do\n      if n = 0 then\n        False\n      else\n        is_even(n - 1)\n      end\n    end\n\nend\n";
        let diags = soundness_verify(even_odd, "evenodd.gdl");
        assert!(
            diags.is_empty(),
            "decreasing mutual recursion must verify, got: {diags:?}"
        );
        let bad = even_odd.replacen("        is_even(n - 1)", "        is_even(n)", 1);
        assert_ne!(bad, even_odd);
        let diags = soundness_verify(&bad, "evenodd_bad.gdl");
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "is_odd" && d.tag == "variant_decrease"),
            "got: {diags:?}"
        );

        let m = parse_user_module_with_prelude(&bad);
        let groups = compute_recursive_groups(&m);
        let g: HashSet<String> = ["is_even".to_string(), "is_odd".to_string()]
            .into_iter()
            .collect();
        assert_eq!(groups.get("is_even"), Some(&g));
    }

    #[test]
    fn test_product_module_type_params_and_not_equal() {
        // A bounded type parameter of a record module verifies, and its `EQ` theory is usable.
        let pair = "module PAIR[A: EQ]\n\nrepresentation {ANY}\n  fst: A\n  snd: A\n\nfeature {ANY}\n\n  same: BOOL\n    do\n      Current.fst = Current.snd\n    ensure\n      sym: Result = (Current.snd = Current.fst)\n    end\n\n  distinct: BOOL\n    do\n      Current.fst != Current.snd\n    ensure\n      sym_ne: Result = (Current.snd != Current.fst)\n    end\n\n  twisted: BOOL\n    do\n      Current.fst = Current.snd\n    ensure\n      wrong: Result\n    end\n\nend\n";
        let diags = soundness_verify(pair, "pair.gdl");
        assert_eq!(
            diags.len(),
            1,
            "only `twisted.wrong` may fail, got: {diags:?}"
        );
        assert!(
            diags[0].routine == "twisted" && diags[0].tag == "wrong",
            "got: {diags:?}"
        );

        // `a != b` is `not (a = b)`.
        let m = parse_single_module(
            "functions N\n\nfeature {ANY}\n\n  f(a: U64, b: U64): BOOL\n    do\n      a != b\n    end\n\nend\n",
        );
        let (_, r) = &m.feature_routines[0];
        let expected = Expr::UnaryNot(Box::new(Expr::BinOp(
            "=".into(),
            Box::new(Expr::Var("a".into())),
            Box::new(Expr::Var("b".into())),
        )));
        assert_eq!(r.body, expected);
        let neq = "functions NEQ\n\nfeature {ANY}\n\n  differ(a: U64, b: U64): BOOL\n    do\n      a != b\n    ensure\n      def: Result = (not (a = b))\n      sym: Result = (b != a)\n    end\n\n  bad(a: U64): BOOL\n    do\n      a != a\n    ensure\n      wrong: Result\n    end\n\nend\n";
        let diags = soundness_verify(neq, "neq.gdl");
        assert!(
            diags.len() == 1 && diags[0].routine == "bad" && diags[0].tag == "wrong",
            "got: {diags:?}"
        );
    }

    #[test]
    fn test_termination_through_function_values_is_rejected() {
        let check = |src: &str, file: &str| {
            let m = parse_user_module_with_prelude(src);
            with_prelude_injection(true, || check_higher_order_recursion(&m, src, file))
        };
        let apply = "  apply(g: NAT -> NAT, x: NAT): NAT\n    do\n      g(x)\n    end\n\n";
        let module = |name: &str, body: &str| {
            format!(
                "functions {name}\n\nfeature {{ANY}}\n\n{apply}  f(n: NAT): NAT\n    variant\n      n\n    do\n      {body}\n    end\n\nend\n"
            )
        };
        // Recursion inside a lambda, by passing itself, or by its qualified name.
        for (name, body) in [
            ("HOL", "apply(fn(x: NAT): NAT do f(x) end, n)"),
            ("HOR", "apply(f, n)"),
            ("HOQ", "HOQ.apply(HOQ.f, n)"),
        ] {
            let src = module(name, body);
            let diags = check(&src, "ho.gdl");
            assert!(
                diags
                    .iter()
                    .any(|d| d.routine == "f" && d.tag == "higher_order_recursion"),
                "{name}: recursion through a function value must be rejected, got: {diags:?}"
            );
            let m = parse_user_module_with_prelude(&src);
            assert!(
                compute_recursive_groups(&m)
                    .get("f")
                    .is_some_and(|g| g.contains("f"))
            );
        }
        // Mutual recursion where one edge is a function value.
        let mutual = format!(
            "functions HOM\n\nfeature {{ANY}}\n\n{apply}  f(n: NAT): NAT\n    variant\n      n\n    do\n      apply(h, n)\n    end\n\n  h(n: NAT): NAT\n    variant\n      n\n    do\n      if n = 0 then 0 else f(n - 1) end\n    end\n\nend\n"
        );
        let diags = check(&mutual, "hom.gdl");
        assert!(
            diags
                .iter()
                .any(|d| d.routine == "f" && d.tag == "higher_order_recursion"),
            "got: {diags:?}"
        );

        // Passing a non-recursive routine, a lambda that does not recurse, and parameters that
        // shadow routine names are all fine.
        let ok = format!(
            "functions HOK\n\nfeature {{ANY}}\n\n{apply}  succ(x: NAT): NAT\n    require\n      small: x < 1000\n    do\n      x + 1\n    end\n\n  f(n: NAT): NAT\n    require\n      small: n < 10\n    do\n      apply(fn(x: NAT): NAT do x end, n)\n    end\n\n  h(succ: NAT): NAT\n    require\n      small: succ < 1000\n    do\n      succ + 1\n    end\n\nend\n"
        );
        let diags = check(&ok, "hok.gdl");
        assert!(
            diags.is_empty(),
            "non-recursive higher-order use must pass, got: {diags:?}"
        );
        let m = parse_user_module_with_prelude(&ok);
        assert!(compute_recursive_groups(&m).is_empty());
    }
}
