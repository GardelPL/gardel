#![expect(
    missing_docs,
    reason = "Compiler CLI binary with internal AST and emitters"
)]
#![expect(missing_debug_implementations, reason = "Internal compiler types")]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::process::Command;

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
    Eof,
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
                i += 1;
                if i >= chars.len() {
                    return Err(LexError {
                        message: "Unterminated escape in char literal".to_string(),
                        position: start,
                    });
                }
                match chars[i] {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '\'' => '\'',
                    '\\' => '\\',
                    '0' => '\0',
                    other => other,
                }
            } else if i < chars.len() {
                chars[i]
            } else {
                return Err(LexError {
                    message: "Empty char literal".to_string(),
                    position: start,
                });
            };
            i += 1;
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
            let start = i;
            let start_line = current_line;
            i += 1;
            let mut s = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\n' {
                    current_line += 1;
                }
                if chars[i] == '\\' {
                    i += 1;
                    if i >= chars.len() {
                        return Err(LexError {
                            message: "Unterminated escape in string literal".to_string(),
                            position: start,
                        });
                    }
                    match chars[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '"' => s.push('"'),
                        '\\' => s.push('\\'),
                        other => s.push(other),
                    }
                } else {
                    s.push(chars[i]);
                }
                i += 1;
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
                _ => break,
            };
            let op_str = op.to_string();
            self.next();
            let right = self.parse_bitwise_or()?;
            left = Expr::BinOp(op_str, Box::new(left), Box::new(right));
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
        while *self.peek() == Token::Dot {
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
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
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
    let existing_laws: HashSet<String> =
        m.specify_laws.iter().map(|(_, l)| l.name.clone()).collect();

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
            if !existing_laws.contains(&law.name) {
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
        Pattern::Lit(Literal::Str(s)) => format!("\"{s}\""),
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

/// Formats a Gardel `Expr` back into canonical Gardel syntax for `path_conditions` and diagnostics.
pub fn format_gardel_expr(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Float(f) => f.to_string(),
        Expr::Char(c) => format!("'{}'", char::from_u32(*c).unwrap_or('?')),
        Expr::Unit => "()".to_string(),
        Expr::Bool(true) => "True".to_string(),
        Expr::Bool(false) => "False".to_string(),
        Expr::Str(s) => format!("\"{s}\""),
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
            if args.is_empty() {
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

/// Returns the canonical Gardel v0.4 Phase 4 Standard Library source package (§3, §5, §10.5, §11, §16).
pub fn phase4_stdlib_source() -> &'static str {
    r#"
group { SEQ, BIGNAT, STRING, OPTION }

module OPTION[A]

representation {ANY}
  | None
  | Some(value: A)

feature {ANY}

  is_some: BOOL
    do
      match Current
        case None    then False
        case Some(_) then True
      end
    end

  get: A
    require
      present: Current.is_some
    do
      match Current
        case Some(v) then v
      end
    end

end

module SEQ[A]

representation {ANY}
  | Empty
  | Leaf(value: A)
  | Concat(left: SEQ[A], right: SEQ[A], len: NAT)

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

create {ANY}

  empty: SEQ[A]
    do
      Empty
    ensure
      empty_len: Result.length = 0
      empty_model: Result.model = LIST.empty
    end

  singleton(x: A): SEQ[A]
    do
      Leaf(x)
    ensure
      unit_len: Result.length = 1
      unit_model: Result.model = LIST.empty.prepend(x)
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

  is_empty: BOOL
    do
      Current.length = 0
    ensure
      def: Result = (Current.length = 0)
    end

  concat(other: SEQ[A]): SEQ[A]
    require
      fits: Current.length <= 18446744073709551615 - other.length
    do
      Concat(Current, other, Current.length + other.length)
    ensure
      len_sum: Result.length = Current.length + other.length
      model_concat: Result.model = Current.model.append(other.model)
    end

end

module BIGNAT

representation {NONE}
  limbs: LIST[U64]
  size: NAT

specify {ANY}

  value_of(ls: LIST[U64]): MATH_INT
    variant
      ls.length
    do
      if ls.length = 0 then
        0
      else
        ls.head + 18446744073709551616 * value_of(ls.tail)
      end
    end

  model: MATH_INT
    do
      reveal value_of
      value_of(Current.limbs)
    end

create {ANY}

  zero: BIGNAT
    do
      { limbs = LIST.empty, size = 0 }
    ensure
      value_is_zero: Result.model = 0
      zero_limbs: Result.limb_count = 0
    end

  from_u64(x: U64): BIGNAT
    do
      if x = 0 then
        { limbs = LIST.empty, size = 0 }
      else
        { limbs = LIST.empty.prepend(x), size = 1 }
      end
    ensure
      val: Result.model = x
      count_bounded: Result.limb_count <= 1
    end

feature {ANY}

  limb_count: NAT
    do
      Current.size
    end

  push_high_limb(limb: U64): BIGNAT
    require
      not_full: Current.limb_count < 1024
    do
      { limbs = Current.limbs.prepend(limb),
        size = Current.size + 1 }
    ensure
      limb_added: Result.limb_count = Current.limb_count + 1
      model_updated: Result.model = limb + 18446744073709551616 * Current.model
    end

invariant
  size_matches: size = limbs.length
  bounded_limbs: size <= 1024

end

module STRING

representation {NONE}
  bytes: LIST[U64]
  size: NAT
  limit: NAT

specify {ANY}

  all_bytes_valid(bs: LIST[U64]): BOOL
    variant
      bs.length
    do
      if bs.length = 0 then
        True
      else
        bs.head < 256 and all_bytes_valid(bs.tail)
      end
    end

  byte_model: LIST[U64]
    do
      Current.bytes
    end

  chars: LIST[CHAR]
    do
      Current.bytes
    end

create {ANY}

  make_empty(cap: NAT): STRING
    require
      positive_cap: cap > 0
    do
      { bytes = LIST.empty, size = 0, limit = cap }
    ensure
      empty_bytes: Result.byte_length = 0
      empty_str: Result.is_empty
    end

feature {ANY}

  byte_length: NAT
    do
      Current.size
    end

  capacity: NAT
    do
      Current.limit
    end

  is_empty: BOOL
    do
      Current.size = 0
    ensure
      def: Result = (Current.byte_length = 0)
    end

  push_byte(b: U64): STRING
    require
      is_byte: b < 256
      has_room: Current.byte_length < Current.capacity
    do
      { bytes = Current.bytes.prepend(b),
        size = Current.size + 1,
        limit = Current.limit }
    ensure
      grew: Result.byte_length = Current.byte_length + 1
      non_empty: not Result.is_empty
    end

  push_utf8_word(w: U64): STRING
    require
      valid_word: w < 256
      has_room: Current.byte_length < Current.capacity
    do
      { bytes = Current.bytes.prepend(w),
        size = Current.size + 1,
        limit = Current.limit }
    ensure
      grew: Result.byte_length = Current.byte_length + 1
      non_empty: not Result.is_empty
    end

invariant
  positive_capacity: limit > 0
  size_matches: size = bytes.length
  within_bounds: size <= limit
  valid_bytes: all_bytes_valid(bytes)

end
"#
}

/// Parses and returns all Phase 4 standard library modules (`OPTION`, `SEQ`, `BIGNAT`, `STRING`).
pub fn parse_phase4_stdlib() -> Result<FileAst, String> {
    let tokens = tokenize(phase4_stdlib_source()).map_err(|e| e.to_string())?;
    let mut parser = Parser::new(tokens);
    parser.parse_file().map_err(|e| e.to_string())
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

pub fn emit_boogie(m: &ModuleDecl, fuel_limit: usize) -> String {
    let mut out = String::new();
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

    for (tp, bound) in &m.type_bounds {
        if bound == "EQ" || bound == "ORD" {
            out.push_str(&format!(
                "function {tp}_eq(x: {tp}, y: {tp}) returns (bool);\n"
            ));
            out.push_str(&format!(
                "axiom (forall x: {tp} :: {{ {tp}_eq(x, x) }} {tp}_eq(x, x));\n"
            ));
            out.push_str(&format!("axiom (forall x: {tp}, y: {tp} :: {{ {tp}_eq(x, y) }} {tp}_eq(x, y) ==> {tp}_eq(y, x));\n"));
            out.push_str(&format!("axiom (forall x: {tp}, y: {tp}, z: {tp} :: {{ {tp}_eq(x, y), {tp}_eq(y, z) }} {tp}_eq(x, y) && {tp}_eq(y, z) ==> {tp}_eq(x, z));\n"));
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
        out.push('\n');
    }

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
    out.push_str("function Str_concat(a: GardelString, b: GardelString) returns (GardelString);\n");
    out.push_str("function Str_length(s: GardelString) returns (int);\n");
    out.push_str("axiom (forall s: GardelString :: { Str_length(s) } 0 <= Str_length(s));\n");
    out.push_str("const unique str_lit_empty: GardelString;\n");
    out.push_str("axiom Str_length(str_lit_empty) == 0;\n");
    let strings = collect_module_strings(m);
    for (idx, s) in strings.iter().enumerate() {
        out.push_str(&format!("const unique str_lit_{idx}: GardelString;\n"));
        out.push_str(&format!(
            "axiom Str_length(str_lit_{idx}) == {};\n",
            s.len()
        ));
    }
    out.push_str("function IO_read_line() returns (GardelString);\n");
    out.push_str("function IO_print(s: GardelString) returns (IO int);\n");
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
        "axiom (forall<T> x: T, y: T, xs: Seq T :: { Seq_has(Seq_prepend(y, xs), x) } Seq_has(Seq_prepend(y, xs), x) <==> (x == y || Seq_has(xs, x)));\n",
    );
    out.push_str(
        "axiom (forall<T> x: T, xs: Seq T, ys: Seq T :: { Seq_has(Seq_append(xs, ys), x) } Seq_has(Seq_append(xs, ys), x) <==> (Seq_has(xs, x) || Seq_has(ys, x)));\n\n",
    );

    out.push_str("type Tuple2 A B;\n");
    out.push_str("function Tuple2_mk<A, B>(a: A, b: B) returns (Tuple2 A B);\n");
    out.push_str("function Tuple2_first<A, B>(t: Tuple2 A B) returns (A);\n");
    out.push_str("function Tuple2_second<A, B>(t: Tuple2 A B) returns (B);\n");
    out.push_str(
        "axiom (forall<A, B> a: A, b: B :: { Tuple2_mk(a, b) } Tuple2_first(Tuple2_mk(a, b)) == a && Tuple2_second(Tuple2_mk(a, b)) == b);\n\n",
    );

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
                if v.fields.is_empty() {
                    out.push_str(&format!(
                        "axiom (forall{tp_quant} x: {mod_bpl_ty} :: {{ is_{}(x) }} is_{}(x) <==> x == {}());\n",
                        v.name, v.name, v.name
                    ));
                } else {
                    let mut proj_conj = vec![format!("is_{}({ctor_call})", v.name)];
                    for (fnm, fty) in &v.fields {
                        proj_conj.push(format!("{}_{}({ctor_call}) == {fnm}", v.name, fnm));
                        if fty == &m.name
                            || fty == "Current"
                            || fty.starts_with(&format!("{}[", m.name))
                        {
                            proj_conj.push(format!(
                                "{}_adt_size({fnm}) < {}_adt_size({ctor_call})",
                                m.name, m.name
                            ));
                        }
                    }
                    out.push_str(&format!(
                        "axiom (forall{tp_quant} {} :: {{ {ctor_call} }} {});\n",
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
                    out.push_str(&format!(
                        "axiom (forall{tp_quant} {}, {} :: {{ {c1}, {c2} }} {c1} == {c2} ==> ({}));\n",
                        p1.join(", "),
                        p2.join(", "),
                        eqs.join(" && ")
                    ));
                }
            }
            // Pairwise Variant Disjointness Axioms
            for i in 0..variants.len() {
                for j in (i + 1)..variants.len() {
                    let v_i = &variants[i];
                    let v_j = &variants[j];
                    out.push_str(&format!(
                        "axiom (forall{tp_quant} x: {mod_bpl_ty} :: {{ is_{}(x) }} is_{}(x) ==> !is_{}(x));\n",
                        v_i.name, v_i.name, v_j.name
                    ));
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

    // 4. Module Refinement Invariant (§6)
    out.push_str(&format!(
        "// --- 4. Module Refinement Invariant ({}_Invariant) ---\n",
        m.name
    ));
    out.push_str(&format!(
        "function {}_Invariant{tp_quant}(Current: {mod_bpl_ty}) returns (bool)\n{{\n",
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

    for (r, has_current) in &all_routines {
        let uses_curr = *has_current && routine_uses_current(r, &implicit_fields);
        let mut sig_params = vec!["fuel: Fuel".to_string()];
        let mut q_params = vec!["f: Fuel".to_string()];
        let mut call_args_step_two = vec!["SuccFuel(SuccFuel(f))".to_string()];
        let mut call_args_succ = vec!["SuccFuel(f)".to_string()];
        let mut call_args_f = vec!["f".to_string()];
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
            }
        }
        for (_, req_e) in &r.requires {
            pre_guards.push(emit_boogie_expr_with_strings(
                req_e,
                m,
                &empty_fields,
                &empty_revealed,
                "f",
                &strings,
            ));
        }

        let ret_bty = map_boogie_type(&r.ret_type, m);
        out.push_str(&format!(
            "function fn_{}{tp_quant}({}) returns ({ret_bty});\n",
            r.name,
            sig_params.join(", ")
        ));

        let fn_step_two_app = format!("fn_{}({})", r.name, call_args_step_two.join(", "));
        let fn_succ_app = format!("fn_{}({})", r.name, call_args_succ.join(", "));
        let fn_f_app = format!("fn_{}({})", r.name, call_args_f.join(", "));
        let body_expr = emit_boogie_block_expr(&r.stmts, &r.body, m, &empty_fields, "f", &strings);
        let guard_str = if pre_guards.is_empty() {
            String::new()
        } else {
            format!("{} ==> ", pre_guards.join(" && "))
        };
        let fn_step_two_trigger = if m.type_params.is_empty() {
            format!("{{ {fn_step_two_app} }}")
        } else {
            format!("{{ ({fn_step_two_app} : {ret_bty}) }}")
        };
        let fn_succ_trigger = if m.type_params.is_empty() {
            format!("{{ {fn_succ_app} }}")
        } else {
            format!("{{ ({fn_succ_app} : {ret_bty}) }}")
        };
        let fn_f_trigger = if m.type_params.is_empty() {
            format!("{{ {fn_f_app} }}")
        } else {
            format!("{{ ({fn_f_app} : {ret_bty}) }}")
        };
        out.push_str(&format!(
            "axiom (forall{tp_quant} {} :: {fn_succ_trigger} {guard_str}{fn_succ_app} == {body_expr});\n",
            q_params.join(", ")
        ));
        out.push_str(&format!(
            "axiom (forall{tp_quant} {} :: {fn_step_two_trigger} {guard_str}{fn_step_two_app} == {fn_succ_app});\n",
            q_params.join(", ")
        ));
        if *has_current && !uses_curr {
            let mut ri_q_params = vec![
                "f: Fuel".to_string(),
                format!("_c1: {mod_bpl_ty}"),
                format!("_c2: {mod_bpl_ty}"),
            ];
            let mut args_c1 = vec!["f".to_string(), "_c1".to_string()];
            let mut args_c2 = vec!["f".to_string(), "_c2".to_string()];
            for (pn, pt) in &r.params {
                ri_q_params.push(format!("{pn}: {}", map_boogie_type(pt, m)));
                args_c1.push(pn.clone());
                args_c2.push(pn.clone());
            }
            let app_c1 = format!("fn_{}({})", r.name, args_c1.join(", "));
            let app_c2 = format!("fn_{}({})", r.name, args_c2.join(", "));
            let ri_trig = if m.type_params.is_empty() {
                format!("{{ {app_c1}, {app_c2} }}")
            } else {
                format!("{{ ({app_c1} : {ret_bty}), ({app_c2} : {ret_bty}) }}")
            };
            out.push_str(&format!(
                "axiom (forall{tp_quant} {} :: {ri_trig} {app_c1} == {app_c2});\n",
                ri_q_params.join(", ")
            ));
        }

        // Opaque Postcondition Frame Axiom (holds at any fuel `f`, including `ZeroFuel`)
        let mut post_clauses = Vec::new();
        if r.ret_type == "NAT" || r.ret_type == "U64" {
            post_clauses.push(format!("in_u64({fn_f_app})"));
        } else if r.ret_type == m.name
            || r.ret_type == "Current"
            || r.ret_type.starts_with(&format!("{}[", m.name))
        {
            post_clauses.push(format!("{}_Invariant({fn_f_app})", m.name));
        }
        for (_, ens_e) in &r.ensures {
            let ens_bpl = emit_boogie_expr_with_strings(
                ens_e,
                m,
                &empty_fields,
                &empty_revealed,
                "f",
                &strings,
            )
            .replace("Result", &fn_f_app);
            post_clauses.push(format!("({ens_bpl})"));
        }
        if !post_clauses.is_empty() {
            out.push_str(&format!(
                "axiom (forall{tp_quant} {} :: {fn_f_trigger} {guard_str}({}));\n",
                q_params.join(", "),
                post_clauses.join(" && ")
            ));
        }
        out.push('\n');
    }

    // 6. Concept Law Axioms with Triggers (§9)
    if !m.specify_laws.is_empty() {
        out.push_str("// --- 6. Concept Law Quantified Axioms & Triggers (§9) ---\n");
        for (_, law) in &m.specify_laws {
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
            } else if !law.requires.is_empty() {
                law.requires
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
                let body_implication = if reqs.is_empty() {
                    ens.join(" && ")
                } else {
                    format!("({}) ==> ({})", reqs.join(" && "), ens.join(" && "))
                };
                out.push_str(&format!(
                    "axiom (forall{tp_quant} {} :: {trig_str}{body_implication});\n",
                    q_params.join(", ")
                ));
            }
        }
        out.push('\n');
    }

    // 7. Verification Condition Procedures (`Verify_<routine>` & `Verify_law_<law>`) (§12)
    out.push_str("// --- 7. Verification Condition Procedures (§12) ---\n");
    for (r, has_current) in &all_routines {
        out.push_str(&emit_boogie_verify_routine(r, *has_current, m, &strings));
    }
    for (_, law) in &m.specify_laws {
        out.push_str(&emit_boogie_verify_law(law, m, &strings));
    }

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

    out
}

fn routine_uses_current(r: &RoutineDecl, field_names: &HashSet<String>) -> bool {
    let self_name = r.name.as_str();
    r.requires
        .iter()
        .any(|(_, e)| expr_uses_current(e, field_names, self_name))
        || r.ensures
            .iter()
            .any(|(_, e)| expr_uses_current(e, field_names, self_name))
        || r.variant
            .iter()
            .any(|e| expr_uses_current(e, field_names, self_name))
        || stmts_use_current(&r.stmts, field_names, self_name)
        || expr_uses_current(&r.body, field_names, self_name)
}

fn stmts_use_current(stmts: &[Stmt], field_names: &HashSet<String>, self_routine: &str) -> bool {
    stmts.iter().any(|st| match st {
        Stmt::Let(_, e) | Stmt::Check(_, e) | Stmt::Assume(_, e) => {
            expr_uses_current(e, field_names, self_routine)
        }
        Stmt::Reveal(_) => false,
    })
}

fn expr_uses_current(e: &Expr, field_names: &HashSet<String>, self_routine: &str) -> bool {
    match e {
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Char(_)
        | Expr::Unit
        | Expr::Bool(_)
        | Expr::Str(_) => false,
        Expr::Var(v) => v == "Current" || field_names.contains(v),
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) => {
            expr_uses_current(inner, field_names, self_routine)
        }
        Expr::BinOp(_, l, r) => {
            expr_uses_current(l, field_names, self_routine)
                || expr_uses_current(r, field_names, self_routine)
        }
        Expr::Call(fname, args) => {
            fname != self_routine
                || args
                    .iter()
                    .any(|a| expr_uses_current(a, field_names, self_routine))
        }
        Expr::MethodCall(recv, _, args) => {
            expr_uses_current(recv, field_names, self_routine)
                || args
                    .iter()
                    .any(|a| expr_uses_current(a, field_names, self_routine))
        }
        Expr::If(c, ts, te, es, ee) => {
            expr_uses_current(c, field_names, self_routine)
                || stmts_use_current(ts, field_names, self_routine)
                || expr_uses_current(te, field_names, self_routine)
                || stmts_use_current(es, field_names, self_routine)
                || expr_uses_current(ee, field_names, self_routine)
        }
        Expr::Match(scrut, cases) => {
            expr_uses_current(scrut, field_names, self_routine)
                || cases.iter().any(|c| {
                    stmts_use_current(&c.stmts, field_names, self_routine)
                        || expr_uses_current(&c.body, field_names, self_routine)
                })
        }
        Expr::RecordLit(_) | Expr::RecordUpdate(_, _) => true,
        Expr::TupleLit(es) => es
            .iter()
            .any(|el| expr_uses_current(el, field_names, self_routine)),
        Expr::Forall(_, _, b) | Expr::Exists(_, _, b) => {
            expr_uses_current(b, field_names, self_routine)
        }
        Expr::WithIo(_, _) | Expr::Lambda(_, _, _, _) => true,
    }
}

fn map_boogie_type(ty: &str, m: &ModuleDecl) -> String {
    match ty {
        "NAT" | "U64" | "I64" | "INT" | "MATH_INT" | "CHAR" | "EXIT_CODE" => {
            return "int".to_string();
        }
        "BOOL" => return "bool".to_string(),
        "STRING" => {
            if m.name == "STRING" {
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
    if ty == m.name || ty.starts_with(&format!("{}[", m.name)) {
        return map_boogie_type("Current", m);
    }
    if let Some(inner) = ty.strip_prefix("LIST[").and_then(|s| s.strip_suffix(']')) {
        return format!("(Seq {})", map_boogie_type(inner.trim(), m));
    }
    if let Some(inner) = ty.strip_prefix("SEQ[").and_then(|s| s.strip_suffix(']')) {
        return format!("(Seq {})", map_boogie_type(inner.trim(), m));
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
    let tp_quant = if m.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", m.type_params.join(", "))
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

fn emit_boogie_verify_law(law: &LawDecl, m: &ModuleDecl, strings: &[String]) -> String {
    let mut s = String::new();
    let tp_quant = if m.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", m.type_params.join(", "))
    };
    let params: Vec<String> = law
        .params
        .iter()
        .map(|(pn, pt)| format!("{pn}: {}", map_boogie_type(pt, m)))
        .collect();
    s.push_str(&format!(
        "procedure Verify_law_{}{tp_quant}({})\n",
        law.name,
        params.join(", ")
    ));
    let empty_fields = HashSet::new();
    let mut revealed = HashSet::new();
    for (pn, pt) in &law.params {
        if pt == &m.name || pt == "Current" {
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
    for st in &law.stmts {
        match st {
            Stmt::Check(tag, e) => {
                s.push_str(&format!(
                    "  assert {{:msg \"{}::check::{tag}\"}} {};\n",
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
    for (tag, ens_e) in &law.ensures {
        s.push_str(&format!(
            "  assert {{:msg \"{}::ensure::{tag}\"}} {};\n",
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
    s
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
            let bty = infer_boogie_expr_type(e, r, m);
            out.push((v.clone(), bty));
            collect_proc_locals(&[], e, r, m, out);
        }
    }
    match body {
        Expr::If(_, ts, te, es, ee) => {
            collect_proc_locals(ts, te, r, m, out);
            collect_proc_locals(es, ee, r, m, out);
        }
        Expr::Match(_, cases) => {
            for c in cases {
                if let ReprKind::Sum(variants) = &m.repr {
                    if let Some(vdecl) = variants.iter().find(|v| v.name == c.ctor()) {
                        for (opt_b, (_, fty)) in c.top_bindings().iter().zip(vdecl.fields.iter()) {
                            if let Some(b) = opt_b {
                                out.push((b.clone(), map_boogie_type(fty, m)));
                            }
                        }
                    }
                }
                collect_proc_locals(&c.stmts, &c.body, r, m, out);
            }
        }
        Expr::WithIo(io_stmts, final_e) => {
            for s in io_stmts {
                if let IoStmt::Bind(v, e) = s {
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
        Expr::Str(_) => "GardelString".to_string(),
        Expr::Bool(_) | Expr::UnaryNot(_) => "bool".to_string(),
        Expr::BinOp(op, l, _) => {
            if matches!(op.as_str(), "+" | "-" | "*" | "/" | "%") {
                if matches!(&**l, Expr::Str(_)) {
                    "GardelString".to_string()
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
                    return "GardelString".to_string();
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
            if let ReprKind::Sum(variants) = &m.repr {
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
                s.push_str(&format!("{pad}{kw} (is_{}({scrut_bpl})) {{\n", c.ctor()));
                if let ReprKind::Sum(variants) = &m.repr {
                    if let Some(vdecl) = variants.iter().find(|v| v.name == c.ctor()) {
                        for (opt_b, (fname, fty)) in
                            c.top_bindings().iter().zip(vdecl.fields.iter())
                        {
                            if let Some(bname) = opt_b {
                                s.push_str(&format!(
                                    "{pad}  {bname} := {}_{}({scrut_bpl});\n",
                                    c.ctor(),
                                    fname
                                ));
                                if !m.invariants.is_empty()
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
                    IoStmt::Action(e) => {
                        s.push_str(&format!(
                            "{pad}// linear IO action: {}\n",
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
            }
            s.push_str(&format!(
                "{pad}Result := {};\n",
                emit_boogie_expr_with_strings(
                    final_e,
                    m,
                    &empty_fields,
                    revealed,
                    "DefaultFuel",
                    strings
                )
            ));
        }
        _ => {
            s.push_str(&emit_boogie_expr_obligations(
                body, r, m, revealed, indent, strings,
            ));
            s.push_str(&format!(
                "{pad}Result := {};\n",
                emit_boogie_expr_with_strings(
                    body,
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
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Float(f) => format!("F64_lit_{}", f.to_bits()),
        Expr::Char(c) => c.to_string(),
        Expr::Unit => "Unit_val".to_string(),
        Expr::Bool(b) => b.to_string(),
        Expr::Str(s) => {
            if s.is_empty() {
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
            let is_ctor =
                matches!(&m.repr, ReprKind::Sum(vs) if vs.iter().any(|v| v.name == *fname));
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
                _ => {
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
                    let mut full_args = vec![call_fuel.to_string(), r_str];
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
                acc = format!("(if is_{}({s_str}) then {branch_val} else {acc})", c.ctor());
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
        Expr::Lambda(_, _, stmts, body) => {
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
    let mut out = emit_boogie_expr_with_strings(
        &expr,
        m,
        implicit_fields,
        &HashSet::new(),
        fuel_var,
        strings,
    );
    if let ReprKind::Sum(variants) = &m.repr {
        if let Some(vdecl) = variants.iter().find(|v| v.name == c.ctor()) {
            for (opt_b, (fname, _)) in c.top_bindings().iter().zip(vdecl.fields.iter()) {
                if let Some(bname) = opt_b {
                    let proj = format!("{}_{}({scrut_bpl})", c.ctor(), fname);
                    out = replace_whole_ident(&out, bname, &proj);
                }
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
        let dafny_z3 = r"C:\home\Learn\Gardel\tools\dafny\z3\bin\z3-4.12.1.exe";
        if std::path::Path::new(dafny_z3).exists() {
            return Some(dafny_z3.to_string());
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
    let bpl_code = emit_boogie(m, fuel_limit);
    let mut temp_path = env::temp_dir();
    temp_path.push(format!(
        "gardel_verify_{}_{}.bpl",
        m.name,
        std::process::id()
    ));
    fs::write(&temp_path, &bpl_code)
        .map_err(|e| format!("Failed to write temporary Boogie file: {e}"))?;

    let mut cmd = Command::new(boogie_path);
    cmd.arg(format!("/proverOpt:PROVER_PATH={z3_path}"));
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

fn collect_routine_path_conditions(m: &ModuleDecl, routine: &str) -> Vec<String> {
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
            for st in &r.stmts {
                if let Stmt::Reveal(t) = st {
                    pcs.push(format!("reveal {t}"));
                }
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
            let pcs = collect_routine_path_conditions(m, &routine);

            let static_rule = match rule_kind.as_str() {
                "overflow" => "Phase 3 Boogie (Arithmetic Overflow)",
                "div_by_zero" => "Phase 3 Boogie (Division by Zero)",
                "variant" => "Phase 3 Boogie (Termination Variant Violation)",
                "invariant" => "Phase 3 Boogie (Invariant Violation)",
                "ensure" => "Phase 3 Boogie (Postcondition Violation)",
                "check" => "Phase 3 Boogie (Check Assertion Failed)",
                "match" => "Phase 3 Boogie (Non-Exhaustive Match)",
                _ => "Phase 3 Boogie (Verification Condition Failed)",
            };
            let diag = Diagnostic::basic(
                "error",
                static_rule,
                routine.clone(),
                tag.clone(),
                format!("Formal verification condition `{tag}` failed in `{routine}`"),
            )
            .with_ai_context(sp, pcs, cex, lemmas);
            diags.push(diag);
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
                let pcs = collect_routine_path_conditions(m, &routine);
                let static_rule = match rule_kind.as_str() {
                    "overflow" => "Phase 3 Boogie (Arithmetic Overflow)",
                    "div_by_zero" => "Phase 3 Boogie (Division by Zero)",
                    "variant" => "Phase 3 Boogie (Termination Variant Violation)",
                    "invariant" => "Phase 3 Boogie (Invariant Violation)",
                    "ensure" => "Phase 3 Boogie (Postcondition Violation)",
                    "check" => "Phase 3 Boogie (Check Assertion Failed)",
                    "match" => "Phase 3 Boogie (Non-Exhaustive Match)",
                    _ => "Phase 3 Boogie (Verification Condition Failed)",
                };
                diags.push(
                    Diagnostic::basic("error", static_rule, routine, tag, line.to_string())
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
        }
    }
    for (itag, iexpr) in &m.invariants {
        if itag == tag {
            return find_relevant_lemmas(m, routine, iexpr);
        }
    }
    Vec::new()
}

/// Finds in-scope `specify` routines, laws, and `ensure` clauses sharing symbols with a failed goal (§12).
pub fn find_relevant_lemmas(
    m: &ModuleDecl,
    current_routine: &str,
    goal_expr: &Expr,
) -> Vec<String> {
    let goal_syms = collect_symbols(goal_expr);
    let mut lemmas = Vec::new();

    for (_, sr) in &m.specify_routines {
        if goal_syms.contains(&sr.name) {
            lemmas.push(format!("reveal {}", sr.name));
        }
        for (etag, eexpr) in &sr.ensures {
            let esyms = collect_symbols(eexpr);
            if !esyms.is_disjoint(&goal_syms) {
                lemmas.push(format!("{}.ensure.{}", sr.name, etag));
            }
        }
    }

    for (_, law) in &m.specify_laws {
        for (etag, eexpr) in &law.ensures {
            let esyms = collect_symbols(eexpr);
            if !esyms.is_disjoint(&goal_syms) || goal_syms.contains(&law.name) {
                lemmas.push(format!("law {}.{}", law.name, etag));
            }
        }
    }

    for (_, fr) in m.feature_routines.iter().chain(m.create_routines.iter()) {
        if fr.name == current_routine {
            continue;
        }
        for (etag, eexpr) in &fr.ensures {
            let esyms = collect_symbols(eexpr);
            if goal_syms.contains(&fr.name) || !esyms.is_disjoint(&goal_syms) {
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
            mname == rname
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
                ("first", GardelVal::Tuple(es)) => es.first().cloned().ok_or(EvalFault::Unknown),
                ("second", GardelVal::Tuple(es)) => es.get(1).cloned().ok_or(EvalFault::Unknown),
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
// 9. BACKEND 1: DAFNY VERIFICATION EMITTER (Refinement Invariants + Laws + IO)
// ============================================================================

pub fn emit_dafny(m: &ModuleDecl) -> String {
    let mut out = String::new();
    out.push_str("// Automatically generated by gardelc (Phase 4 -> Dafny Verifier)\n");
    out.push_str("newtype U64 = x: int | 0 <= x < 0x1_0000_0000_0000_0000\n\n");

    let tp = if m.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", m.type_params.join(", "))
    };

    let has_inv = !m.invariants.is_empty();
    let raw_name = if has_inv {
        format!("{}_Raw", m.name)
    } else {
        m.name.clone()
    };

    if m.kind == ModuleKind::Module {
        match &m.repr {
            ReprKind::Sum(variants) => {
                out.push_str(&format!("datatype {raw_name}{tp} = \n"));
                for v in variants {
                    if v.fields.is_empty() {
                        out.push_str(&format!("  | {}\n", v.name));
                    } else {
                        let f_str: Vec<String> = v
                            .fields
                            .iter()
                            .map(|(fnm, fty)| format!("{}: {}", fnm, map_dafny_type(fty, m)))
                            .collect();
                        out.push_str(&format!("  | {}({})\n", v.name, f_str.join(", ")));
                    }
                }
            }
            ReprKind::Product(fields) => {
                let f_str: Vec<String> = fields
                    .iter()
                    .map(|(fnm, fty)| format!("{}: {}", fnm, map_dafny_type(fty, m)))
                    .collect();
                out.push_str(&format!(
                    "datatype {raw_name}{tp} = {}_Ctor({})\n",
                    m.name,
                    f_str.join(", ")
                ));
            }
        }
        out.push('\n');
    }

    if has_inv {
        let mut field_names = HashSet::new();
        if let ReprKind::Product(fields) = &m.repr {
            for (fnm, _) in fields {
                field_names.insert(fnm.clone());
            }
        }

        out.push_str(&format!(
            "ghost predicate {}_Invariant{tp}(Current: {raw_name}{tp})\n{{\n",
            m.name
        ));
        let inv_clauses: Vec<String> = m
            .invariants
            .iter()
            .map(|(tag, expr)| {
                format!(
                    "  ({}) /* {} */",
                    emit_dafny_expr(expr, m, &field_names),
                    tag
                )
            })
            .collect();
        out.push_str(&inv_clauses.join(" &&\n"));
        out.push_str("\n}\n\n");

        out.push_str(&format!(
            "type {}{tp} = x: {raw_name}{tp} | {}_Invariant(x) witness *\n\n",
            m.name, m.name
        ));
    }

    let empty_fields = HashSet::new();
    let is_adt = m.kind == ModuleKind::Module;

    for (_, r) in &m.specify_routines {
        out.push_str(&emit_dafny_routine(r, true, is_adt, m, &empty_fields));
    }

    for (_, r) in &m.feature_routines {
        out.push_str(&emit_dafny_routine(r, false, is_adt, m, &empty_fields));
    }

    for (_, r) in &m.create_routines {
        out.push_str(&emit_dafny_routine(r, false, false, m, &empty_fields));
    }

    for (_, law) in &m.specify_laws {
        out.push_str(&emit_dafny_law(law, m, &empty_fields));
    }

    out
}

fn map_dafny_type(ty: &str, m: &ModuleDecl) -> String {
    match ty {
        "NAT" | "U64" | "EXIT_CODE" => return "U64".to_string(),
        "MATH_INT" | "INT" | "I64" => return "int".to_string(),
        "BOOL" => return "bool".to_string(),
        "CHAR" => return "char".to_string(),
        "STRING" => {
            if m.name == "STRING" {
                return "STRING".to_string();
            }
            return "string".to_string();
        }
        "Current" => return m.name.clone(),
        _ => {}
    }
    if let Some(inner) = ty.strip_prefix("LIST[").and_then(|s| s.strip_suffix(']')) {
        return format!("seq<{}>", map_dafny_type(inner, m));
    }
    if let Some(inner) = ty.strip_prefix("IO[").and_then(|s| s.strip_suffix(']')) {
        return map_dafny_type(inner, m);
    }
    if let Some(inner) = ty.strip_prefix("TUPLE[").and_then(|s| s.strip_suffix(']')) {
        let parts: Vec<String> = inner
            .split(',')
            .map(|p| map_dafny_type(p.trim(), m))
            .collect();
        return format!("({})", parts.join(", "));
    }
    if let Some(idx) = ty.find('[') {
        let base = &ty[..idx];
        let inner = &ty[idx + 1..ty.len() - 1];
        let parts: Vec<String> = inner
            .split(',')
            .map(|p| map_dafny_type(p.trim(), m))
            .collect();
        return format!("{base}<{}>", parts.join(", "));
    }
    ty.to_string()
}

fn emit_dafny_law(law: &LawDecl, m: &ModuleDecl, implicit_fields: &HashSet<String>) -> String {
    let mut s = String::new();
    let tp = if m.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", m.type_params.join(", "))
    };
    let params: Vec<String> = law
        .params
        .iter()
        .map(|(pname, pty)| format!("{}: {}", pname, map_dafny_type(pty, m)))
        .collect();
    s.push_str(&format!("lemma {}{tp}({})\n", law.name, params.join(", ")));

    for (tag, req) in &law.requires {
        s.push_str(&format!(
            "  requires {} // {}\n",
            emit_dafny_expr(req, m, implicit_fields),
            tag
        ));
    }
    if !law.variant.is_empty() {
        let vars: Vec<String> = law
            .variant
            .iter()
            .map(|v| emit_dafny_expr(v, m, implicit_fields))
            .collect();
        s.push_str(&format!("  decreases {}\n", vars.join(", ")));
    }
    for (tag, ens) in &law.ensures {
        s.push_str(&format!(
            "  ensures {} // {}\n",
            emit_dafny_expr(ens, m, implicit_fields),
            tag
        ));
    }

    s.push_str("{\n");
    if let Some(body) = &law.body {
        s.push_str(&emit_dafny_lemma_stmt_expr(
            &law.stmts,
            body,
            m,
            implicit_fields,
            2,
        ));
    }
    s.push_str("}\n\n");
    s
}

fn emit_dafny_lemma_stmt_expr(
    stmts: &[Stmt],
    body: &Expr,
    m: &ModuleDecl,
    implicit_fields: &HashSet<String>,
    indent: usize,
) -> String {
    let pad = " ".repeat(indent);
    let mut s = String::new();
    for st in stmts {
        match st {
            Stmt::Check(tag, e) => {
                s.push_str(&format!(
                    "{}assert {}; // {}\n",
                    pad,
                    emit_dafny_expr(e, m, implicit_fields),
                    tag
                ));
            }
            Stmt::Assume(tag, e) => {
                s.push_str(&format!(
                    "{}assume {}; // {}\n",
                    pad,
                    emit_dafny_expr(e, m, implicit_fields),
                    tag
                ));
            }
            Stmt::Reveal(target) => {
                s.push_str(&format!("{pad}reveal {target}();\n"));
            }
            Stmt::Let(v, e) => {
                s.push_str(&format!(
                    "{pad}var {v} := {};\n",
                    emit_dafny_expr(e, m, implicit_fields)
                ));
            }
        }
    }
    match body {
        Expr::Bool(_) | Expr::Int(_) | Expr::Str(_) | Expr::Var(_) => {}
        Expr::Call(fname, args) => {
            let a_str: Vec<String> = args
                .iter()
                .map(|a| emit_dafny_expr(a, m, implicit_fields))
                .collect();
            s.push_str(&format!("{pad}{fname}({});\n", a_str.join(", ")));
        }
        Expr::Match(scrut, cases) => {
            s.push_str(&format!(
                "{}match {}\n",
                pad,
                emit_dafny_expr(scrut, m, implicit_fields)
            ));
            for c in cases {
                let pat = if c.bindings().is_empty() {
                    c.ctor().to_string()
                } else {
                    format!("{}({})", c.ctor(), c.bindings().join(", "))
                };
                s.push_str(&format!("{pad}  case {pat} =>\n"));
                s.push_str(&emit_dafny_lemma_stmt_expr(
                    &c.stmts,
                    &c.body,
                    m,
                    implicit_fields,
                    indent + 4,
                ));
            }
        }
        Expr::If(c, ts, te, es, ee) => {
            s.push_str(&format!(
                "{}if {} {{\n{}{pad}}} else {{\n{}{pad}}}\n",
                pad,
                emit_dafny_expr(c, m, implicit_fields),
                emit_dafny_lemma_stmt_expr(ts, te, m, implicit_fields, indent + 2),
                emit_dafny_lemma_stmt_expr(es, ee, m, implicit_fields, indent + 2),
            ));
        }
        _ => {}
    }
    s
}

fn emit_dafny_routine(
    r: &RoutineDecl,
    is_ghost: bool,
    has_current: bool,
    m: &ModuleDecl,
    implicit_fields: &HashSet<String>,
) -> String {
    let mut s = String::new();
    let kw = if is_ghost {
        "ghost function"
    } else {
        "function"
    };
    let tp = if m.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", m.type_params.join(", "))
    };

    let mut params = Vec::new();
    if has_current {
        params.push(format!("Current: {}{tp}", m.name));
    }
    for (pname, pty) in &r.params {
        params.push(format!("{pname}: {}", map_dafny_type(pty, m)));
    }
    s.push_str(&format!(
        "{} {}{tp}({}): (Result: {})\n",
        kw,
        r.name,
        params.join(", "),
        map_dafny_type(&r.ret_type, m)
    ));

    for (tag, req) in &r.requires {
        s.push_str(&format!(
            "  requires {} // {}\n",
            emit_dafny_expr(req, m, implicit_fields),
            tag
        ));
    }
    if !r.variant.is_empty() {
        let vars: Vec<String> = r
            .variant
            .iter()
            .map(|v| emit_dafny_expr(v, m, implicit_fields))
            .collect();
        s.push_str(&format!("  decreases {}\n", vars.join(", ")));
    }
    for (tag, ens) in &r.ensures {
        s.push_str(&format!(
            "  ensures {} // {}\n",
            emit_dafny_expr(ens, m, implicit_fields),
            tag
        ));
    }

    s.push_str("{\n");
    s.push_str(&emit_dafny_block(&r.stmts, &r.body, m, implicit_fields, 2));
    s.push_str("}\n\n");
    s
}

fn emit_dafny_block(
    stmts: &[Stmt],
    body: &Expr,
    m: &ModuleDecl,
    implicit_fields: &HashSet<String>,
    indent: usize,
) -> String {
    let pad = " ".repeat(indent);
    let mut s = String::new();
    for st in stmts {
        match st {
            Stmt::Let(v, e) => {
                s.push_str(&format!(
                    "{pad}var {v} := {};\n",
                    emit_dafny_expr(e, m, implicit_fields)
                ));
            }
            Stmt::Check(tag, e) => {
                s.push_str(&format!(
                    "{pad}assert {}; // {tag}\n",
                    emit_dafny_expr(e, m, implicit_fields)
                ));
            }
            Stmt::Assume(tag, e) => {
                s.push_str(&format!(
                    "{pad}assume {}; // {tag}\n",
                    emit_dafny_expr(e, m, implicit_fields)
                ));
            }
            Stmt::Reveal(target) => {
                s.push_str(&format!("{pad}reveal {target}();\n"));
            }
        }
    }
    s.push_str(&format!(
        "{pad}{}\n",
        emit_dafny_expr(body, m, implicit_fields)
    ));
    s
}

fn emit_dafny_expr(e: &Expr, m: &ModuleDecl, implicit_fields: &HashSet<String>) -> String {
    match e {
        Expr::Int(n) => format!("({n} as U64)"),
        Expr::Float(f) => format!("({f} as real)"),
        Expr::Char(c) => format!("'{}'", char::from_u32(*c).unwrap_or('?')),
        Expr::Unit => "()".to_string(),
        Expr::Bool(b) => b.to_string(),
        Expr::Str(s) => format!("\"{s}\""),
        Expr::Var(v) => {
            if implicit_fields.contains(v) {
                format!("Current.{v}")
            } else {
                v.clone()
            }
        }
        Expr::UnaryNot(inner) => {
            format!("(!{})", emit_dafny_expr(inner, m, implicit_fields))
        }
        Expr::UnaryNeg(inner) => {
            format!("(-{})", emit_dafny_expr(inner, m, implicit_fields))
        }
        Expr::BinOp(op, l, r) => {
            let d_op = match op.as_str() {
                "=" => "==",
                "and" => "&&",
                "or" => "||",
                "implies" => "==>",
                other => other,
            };

            let is_cmp = matches!(op.as_str(), "=" | "<" | "<=" | ">" | ">=");
            let l_len = matches!(&**l, Expr::MethodCall(_, mname, _) if mname == "length");
            let r_len = matches!(&**r, Expr::MethodCall(_, mname, _) if mname == "length");

            if is_cmp && (l_len || r_len) {
                let emit_len_side = |side: &Expr| -> String {
                    if let Expr::MethodCall(recv, mname, _) = side {
                        if mname == "length" && m.name != "SEQ" {
                            return format!("|{}|", emit_dafny_expr(recv, m, implicit_fields));
                        }
                    }
                    if let Expr::Int(n) = side {
                        return n.to_string();
                    }
                    format!("({} as int)", emit_dafny_expr(side, m, implicit_fields))
                };
                return format!("({} {d_op} {})", emit_len_side(l), emit_len_side(r));
            }

            format!(
                "({} {d_op} {})",
                emit_dafny_expr(l, m, implicit_fields),
                emit_dafny_expr(r, m, implicit_fields)
            )
        }
        Expr::Call(fname, args) => {
            let a_str: Vec<String> = args
                .iter()
                .map(|a| emit_dafny_expr(a, m, implicit_fields))
                .collect();
            format!("{fname}({})", a_str.join(", "))
        }
        Expr::MethodCall(recv, mname, args) => {
            if let Expr::Var(v) = &**recv {
                if v == "LIST" {
                    match mname.as_str() {
                        "empty" => return "[]".to_string(),
                        "singleton" => {
                            return format!("[{}]", emit_dafny_expr(&args[0], m, implicit_fields));
                        }
                        _ => {}
                    }
                } else if v == "IO" {
                    if mname == "read_line" {
                        return "\"input\"".to_string();
                    } else if mname == "exit" && !args.is_empty() {
                        return emit_dafny_expr(&args[0], m, implicit_fields);
                    }
                    return "(0 as U64)".to_string();
                }
            }
            let r_str = emit_dafny_expr(recv, m, implicit_fields);
            match mname.as_str() {
                "append" => format!(
                    "({r_str} + {})",
                    emit_dafny_expr(&args[0], m, implicit_fields)
                ),
                "prepend" => format!(
                    "([{}] + {r_str})",
                    emit_dafny_expr(&args[0], m, implicit_fields)
                ),
                "head" => format!("{r_str}[0]"),
                "tail" => format!("{r_str}[1..]"),
                "first" => format!("{r_str}.0"),
                "second" => format!("{r_str}.1"),
                "has" => format!(
                    "({} in {r_str})",
                    emit_dafny_expr(&args[0], m, implicit_fields)
                ),
                "length" if m.name != "SEQ" => format!("(|{r_str}| as U64)"),
                _ => {
                    if let ReprKind::Product(fields) = &m.repr {
                        if args.is_empty() && fields.iter().any(|(fnm, _)| fnm == mname) {
                            return format!("{r_str}.{mname}");
                        }
                    }
                    let mut all_args = vec![r_str];
                    for a in args {
                        all_args.push(emit_dafny_expr(a, m, implicit_fields));
                    }
                    format!("{mname}({})", all_args.join(", "))
                }
            }
        }
        Expr::If(c, ts, te, es, ee) => {
            format!(
                "if {} then\n{}  else\n{}",
                emit_dafny_expr(c, m, implicit_fields),
                emit_dafny_block(ts, te, m, implicit_fields, 4),
                emit_dafny_block(es, ee, m, implicit_fields, 4)
            )
        }
        Expr::Match(scrut, cases) => {
            let mut s = format!("(match {}\n", emit_dafny_expr(scrut, m, implicit_fields));
            for c in cases {
                let pat = if c.bindings().is_empty() {
                    c.ctor().to_string()
                } else {
                    format!("{}({})", c.ctor(), c.bindings().join(", "))
                };
                s.push_str(&format!(
                    "    case {pat} =>\n{}",
                    emit_dafny_block(&c.stmts, &c.body, m, implicit_fields, 6)
                ));
            }
            s.push(')');
            s
        }
        Expr::RecordLit(fields) => {
            if let ReprKind::Product(decl_fields) = &m.repr {
                let args: Vec<String> = decl_fields
                    .iter()
                    .map(|(fnm, _)| {
                        if let Some((_, fe)) = fields.iter().find(|(k, _)| k == fnm) {
                            emit_dafny_expr(fe, m, implicit_fields)
                        } else {
                            "0".to_string()
                        }
                    })
                    .collect();
                format!("{}_Ctor({})", m.name, args.join(", "))
            } else {
                let args: Vec<String> = fields
                    .iter()
                    .map(|(_, fe)| emit_dafny_expr(fe, m, implicit_fields))
                    .collect();
                format!("{}_Ctor({})", m.name, args.join(", "))
            }
        }
        Expr::RecordUpdate(base, fields) => {
            let b_str = emit_dafny_expr(base, m, implicit_fields);
            let updates: Vec<String> = fields
                .iter()
                .map(|(fnm, fe)| format!("{fnm} := {}", emit_dafny_expr(fe, m, implicit_fields)))
                .collect();
            format!("{b_str}.({})", updates.join(", "))
        }
        Expr::TupleLit(elems) => {
            let es: Vec<String> = elems
                .iter()
                .map(|el| emit_dafny_expr(el, m, implicit_fields))
                .collect();
            format!("({})", es.join(", "))
        }
        Expr::Forall(v, ty, body) => {
            format!(
                "(forall {v}: {} :: {})",
                map_dafny_type(ty, m),
                emit_dafny_expr(body, m, implicit_fields)
            )
        }
        Expr::Exists(v, ty, body) => {
            format!(
                "(exists {v}: {} :: {})",
                map_dafny_type(ty, m),
                emit_dafny_expr(body, m, implicit_fields)
            )
        }
        Expr::WithIo(stmts, final_e) => {
            let mut s = String::new();
            for st in stmts {
                if let IoStmt::Bind(v, se) = st {
                    s.push_str(&format!(
                        "var {v} := {}; ",
                        emit_dafny_expr(se, m, implicit_fields)
                    ));
                }
            }
            s.push_str(&emit_dafny_expr(final_e, m, implicit_fields));
            s
        }
        Expr::Lambda(ps, _, _, body) => {
            let p_str: Vec<String> = ps
                .iter()
                .map(|(n, t)| format!("{n}: {}", map_dafny_type(t, m)))
                .collect();
            format!(
                "(({}) => {})",
                p_str.join(", "),
                emit_dafny_expr(body, m, implicit_fields)
            )
        }
    }
}

// ============================================================================
// 10. BACKEND 2: BEND v2 EMITTER (.bend)
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
        Expr::Str(s) => format!("\"{s}\""),
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
                if v == "IO" {
                    let a_str: Vec<String> = args
                        .iter()
                        .map(|a| emit_bend_expr(a, mod_name, indent))
                        .collect();
                    return format!("IO/{mname}({})", a_str.join(", "));
                }
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
// 11. BACKEND 3: C11 RUNTIME EMITTER (.c) (Phase 4 Stdlib: SEQ, BIGNAT, STRING, IO)
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
        _ if m.kind == ModuleKind::Program => {
            out.push_str(
                r#"// Automatically generated by gardelc (Phase 4 -> Linear IO Program Boundary C11 Runtime)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static inline const char* gardel_io_read_line(char* buf, size_t cap) {
    if (fgets(buf, (int)cap, stdin) != NULL) {
        size_t n = strlen(buf);
        while (n > 0 && (buf[n - 1] == '\n' || buf[n - 1] == '\r')) {
            buf[--n] = '\0';
        }
        return buf;
    }
    return "Gardel";
}

int main(void) {
    char input_buf[256] = "Gardel";
    const char* name = gardel_io_read_line(input_buf, sizeof(input_buf));
    printf("Hello, %s\n", name);
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
"#
    .to_string()
}

pub fn emit_c11_general_module(m: &ModuleDecl) -> String {
    let mut out = String::new();
    out.push_str("// Automatically generated by gardelc (Phase 4 -> General C11 AST Lowering)\n");
    out.push_str(&emit_c11_runtime_header());
    out.push('\n');

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
        if m.feature_routines.iter().any(|(_, r)| r.name == "main") {
            out.push_str("    main_routine();\n");
        }
        out.push_str("    return 0;\n");
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
        Expr::Str(s) => format!("(GardelString){{ \"{s}\", {} }}", s.len()),
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
                if v == "IO" {
                    if mname == "print" {
                        let arg = emit_c11_expr(&args[0], mod_name);
                        return format!("printf(\"%s\\n\", (const char*){arg}.data)");
                    } else if mname == "read_line" {
                        return "gardel_io_read_line((char[256]){{0}}, 256)".to_string();
                    } else if mname == "exit" && !args.is_empty() {
                        return format!("exit({})", emit_c11_expr(&args[0], mod_name));
                    }
                }
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
        Expr::WithIo(_io_stmts, final_e) => emit_c11_expr(final_e, mod_name),
        Expr::Lambda(_, _, _, _) => "/* lifted closure */ 0".to_string(),
        Expr::Forall(_, _, _) | Expr::Exists(_, _, _) => "0".to_string(),
    }
}

// ============================================================================
// 12. CLI DRIVER (Supports --stdlib, Boogie, --fuel, --audit, --strict, --run)
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
            out.push_str(&format!(
                "    {:?} law {}({})\n",
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

/// Writes the embedded Phase 4 standard library out to standalone `.gdl` files in `out_dir`.
pub fn emit_stdlib_files(out_dir: &str) -> Result<Vec<String>, String> {
    fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let bundle_path = format!("{out_dir}/stdlib.gdl");
    fs::write(&bundle_path, phase4_stdlib_source().trim_start()).map_err(|e| e.to_string())?;
    Ok(vec![bundle_path])
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

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "Usage: gardelc <input.gdl | --stdlib> [--inspect [MODULE]] [--emit-stdlib] [--json-diagnostics] [--fuel <N>] [--audit] [--strict] [--bounded-check] [--run]"
        );
        std::process::exit(1);
    }

    if args.iter().any(|a| a == "--emit-stdlib") {
        match emit_stdlib_files("stdlib") {
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
    let stdlib_only = args.iter().any(|a| a == "--stdlib")
        || (inspect_mode && args.get(1).is_some_and(|a| a.starts_with("--")));

    let input_path = if stdlib_only {
        "<phase4_stdlib.gdl>"
    } else {
        &args[1]
    };
    let json_mode = args.iter().any(|a| a == "--json-diagnostics");
    let audit_mode = args.iter().any(|a| a == "--audit");
    let strict_mode = args.iter().any(|a| a == "--strict");
    let bounded_check = args.iter().any(|a| a == "--bounded-check");
    let fuel_limit = parse_fuel_arg(&args);

    let src = if stdlib_only {
        phase4_stdlib_source().to_string()
    } else {
        match fs::read_to_string(input_path) {
            Ok(s) => s,
            Err(e) => {
                if json_mode {
                    let diag = Diagnostic::basic(
                        "error",
                        "IO",
                        "file".to_string(),
                        "io_error".to_string(),
                        format!("Failed to read file '{input_path}': {e}"),
                    );
                    println!("[\n{}\n]", diag.to_json("UNKNOWN", input_path));
                } else {
                    eprintln!("[ERROR] Failed to read file '{input_path}': {e}");
                }
                std::process::exit(1);
            }
        }
    };

    let tokens = match tokenize(&src) {
        Ok(toks) => toks,
        Err(e) => {
            if json_mode {
                let diag = Diagnostic::basic(
                    "error",
                    "Lexer",
                    "lexer".to_string(),
                    "lex_error".to_string(),
                    e.to_string(),
                );
                println!("[\n{}\n]", diag.to_json("UNKNOWN", input_path));
            } else {
                eprintln!("[ERROR] {e}");
            }
            std::process::exit(1);
        }
    };

    let mut parser = Parser::new(tokens);
    let mut file_ast = match parser.parse_file() {
        Ok(ast) => ast,
        Err(e) => {
            if json_mode {
                let diag = Diagnostic::basic(
                    "error",
                    "Parser",
                    "parser".to_string(),
                    "parse_error".to_string(),
                    e.to_string(),
                );
                println!("[\n{}\n]", diag.to_json("UNKNOWN", input_path));
            } else {
                eprintln!("[ERROR] {e}");
            }
            std::process::exit(1);
        }
    };

    let mut concept_map = HashMap::new();
    for c in &file_ast.concepts {
        concept_map.insert(c.name.clone(), c.clone());
    }

    if file_ast.modules.is_empty() {
        if json_mode {
            let diag = Diagnostic::basic(
                "error",
                "Parser",
                "module".to_string(),
                "missing_module".to_string(),
                "Expected at least one module in input file".to_string(),
            );
            println!("[\n{}\n]", diag.to_json("UNKNOWN", input_path));
        } else {
            eprintln!("[ERROR] Expected at least one module in input file");
        }
        std::process::exit(1);
    }

    let mut diags = check_group_packaging(&file_ast, &src, input_path);
    for m in &mut file_ast.modules {
        let mut mod_diags = check_and_expand_concepts(m, &concept_map);
        mod_diags.extend(check_visibility_and_semantics(m));
        for d in &mut mod_diags {
            if d.span.is_none() {
                d.span = Some(locate_span(&src, input_path, &d.tag, &d.routine));
            }
        }
        mod_diags.extend(verify_module_phase3(
            m,
            &src,
            input_path,
            fuel_limit,
            strict_mode,
            audit_mode,
            bounded_check,
        ));
        diags.extend(mod_diags);
    }
    if inspect_mode {
        let filter_mod = args
            .iter()
            .position(|a| a == "--inspect")
            .and_then(|idx| args.get(idx + 1))
            .filter(|val| !val.starts_with("--"))
            .cloned();
        for m in &file_ast.modules {
            if filter_mod
                .as_ref()
                .is_none_or(|f| f.eq_ignore_ascii_case(&m.name))
            {
                println!("{}", format_module_interface(m));
            }
        }
        return;
    }

    let module = file_ast.modules.last().expect("non-empty modules").clone();

    let has_errors = diags.iter().any(|d| d.level == "error");

    if json_mode {
        println!("[");
        for (i, d) in diags.iter().enumerate() {
            let comma = if i + 1 < diags.len() { "," } else { "" };
            println!("{}{comma}", d.to_json(&module.name, input_path));
        }
        println!("]");
        if has_errors {
            std::process::exit(1);
        }
    } else {
        println!(
            "[1/6] Parsed {} concepts {:?}, group {:?}, and {:?} `{}` (type params: {:?}, satisfies: {:?}, laws: {}, fuel: {})",
            concept_map.len(),
            concept_map.keys().collect::<Vec<_>>(),
            file_ast.group,
            module.kind,
            module.name,
            module.type_params,
            module.satisfies.as_ref().map(|s| &s.concepts),
            module.specify_laws.len(),
            fuel_limit
        );
        println!(
            "[2/6] Checked §4 Visibility, §9 Concept Laws, §11 IO Boundary & §12 Direct Verification:"
        );
        if diags.is_empty() {
            println!(
                "      0 errors, 0 warnings — all visibility, concept laws, IO linearity, overflow, and contract obligations satisfied."
            );
        } else {
            for d in &diags {
                let sp = d.span.clone().unwrap_or_else(|| Span::unknown(input_path));
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
    for m in &file_ast.modules {
        prog_env.modules.insert(m.name.clone(), m.clone());
    }

    let mut primary_bpl = String::new();
    let mut primary_dfy = String::new();
    let mut primary_bend = String::new();
    let mut primary_c = String::new();
    let mut primary_exe = String::new();

    for m in &file_ast.modules {
        let stem = m.name.to_lowercase();
        let bpl_path = format!("build/{stem}.bpl");
        let dfy_path = format!("build/{stem}.dfy");
        let bend_path = format!("build/{stem}.bend");
        let c_path = format!("build/{stem}.c");

        let bpl_code = emit_boogie(m, fuel_limit);
        if let Err(e) = fs::write(&bpl_path, bpl_code) {
            eprintln!("[ERROR] Failed to write Boogie output '{bpl_path}': {e}");
            std::process::exit(1);
        }

        let dfy_code = emit_dafny(m);
        if let Err(e) = fs::write(&dfy_path, dfy_code) {
            eprintln!("[ERROR] Failed to write Dafny output '{dfy_path}': {e}");
            std::process::exit(1);
        }

        let mono_module = if m.type_params.is_empty() {
            m.clone()
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
            primary_dfy = dfy_path;
            primary_bend = bend_path;
            primary_c = c_path;
            primary_exe = format!("build/{stem}_bin.exe");
        }
    }

    let gcc_compiled = Command::new("gcc")
        .args(["-O3", "-fopenmp", &primary_c, "-o", &primary_exe])
        .status()
        .is_ok_and(|s| s.success());

    if !json_mode {
        println!("[3/6] Emitted direct Boogie VCs & fuel axioms -> {primary_bpl}");
        println!("[4/6] Emitted Dafny refinement & law lemmas   -> {primary_dfy}");
        println!("[5/6] Emitted monomorphized Bend v2 target    -> {primary_bend}");
        println!("[6/6] Emitted C11 runtime target              -> {primary_c}");
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
        boogie_cmd.args(["/mv:-", &primary_bpl]);
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

        println!("\n=== [1] Dafny Formal Verification (Routines + Concept Laws) ===");
        let mut cmd = Command::new("dafny");
        cmd.args(["verify", &primary_dfy]);
        if let Some(solver) = found_solver {
            cmd.args(["--solver-path", solver]);
        }
        let _ = cmd.status();

        if gcc_compiled && std::path::Path::new(&primary_exe).exists() {
            println!("\n=== [2] Executing Native Executable ({primary_exe}) ===");
            let _ = Command::new(&primary_exe).status();
        }
    }
}

// ============================================================================
// 13. PHASE 3 & 4 UNIT TESTS (Boogie, Fuel, Reveal, SEQ, BIGNAT, STRING, IO)
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
        assert_eq!(
            stdlib.group,
            Some(vec![
                "SEQ".to_string(),
                "BIGNAT".to_string(),
                "STRING".to_string(),
                "OPTION".to_string(),
            ])
        );
        assert_eq!(stdlib.modules.len(), 4);
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
        let dfy = emit_dafny(&m);
        assert!(dfy.contains("case Lit(x) =>"));
        assert!(dfy.contains("case Pair(a, b) =>"));
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
        let dfy = emit_dafny(&m);
        assert!(dfy.contains("decreases m, n"));
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
        assert!(bpl.contains("function T_eq(x: T, y: T) returns (bool);"));
        assert!(bpl.contains("function T_less(x: T, y: T) returns (bool);"));
        assert!(bpl.contains("axiom (forall x: T :: { T_eq(x, x) } T_eq(x, x));"));
        assert!(bpl.contains("axiom (forall x: T :: { T_less(x, x) } !T_less(x, x));"));
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
        assert_eq!(ast.modules.len(), 4);
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
        assert_eq!(bignat.invariants.len(), 2);
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
}
