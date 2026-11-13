//! Package identity: which module, concept or constructor a name in a package means
//! (Roadmap decisions 4, 5, 7, 8).
//!
//! A module's identity is (package, name), and the compiler after this pass keeps one string
//! per identity: its internal name. The prelude keeps its bare names. So does the root project,
//! except a name the prelude also has, so a project that clashes with nothing compiles to the
//! same text as before. Every other name becomes `PK<n>_<PACKAGE>_<NAME>` (see
//! [`internal_name`]), which is built from the full package name, never from an alias. Nothing
//! else can hold that string, which makes the names `gardelc` builds in (`EVENT`, `COMMAND`)
//! identities: bare `COMMAND` can only be the prelude's.
//!
//! A constructor is also identified by its module when its package declares that name twice,
//! as `<MODULE>__<Ctor>`, since `TOK_A.Leaf` and `TOK_B.Leaf` are then different.
//!
//! Each package is resolved against what its own manifest lists, nearest first: the package's
//! own names, then the prelude and its direct dependencies on one level. Several providers
//! on that level are an error, never a choice, unless a type known for certain picks one of
//! several constructors (decision 9). A constructor is the exception to the one level: a
//! dependency's comes before the prelude's, so that the prelude can grow without breaking a
//! program. A transitive dependency provides no name.
//!
//! This is the only pass that sees `ALIAS::NAME` and `MODULE.Ctor`: it runs on every build,
//! and what it leaves behind is a plain internal name. The internal name can be read back, so
//! diagnostics and source lookups need no table.

use super::*;
use std::ops::Range;

/// The prelude's full name (decision 8). Its internal names are its bare names.
pub(crate) const PRELUDE_PACKAGE: &str = "gardel.core";
const PRELUDE_ALIAS: &str = "GARDEL";

/// The name of a project with no manifest. It has no dependencies, so nothing can share it.
pub(crate) const LOCAL_PACKAGE: &str = "local";

/// Between a module and its constructor in an internal name.
const CTOR_SEPARATOR: &str = "__";

/// Upper-case words that are never names of a package's own.
const RESERVED: &[&str] = &["Current", "Result", "Self"];

/// A package of a build and the source units that belong to it.
pub(crate) struct PackageUnits {
    pub(crate) name: String,
    pub(crate) is_root: bool,
    /// (full name, alias) of each direct dependency.
    pub(crate) deps: Vec<(String, String)>,
    pub(crate) units: Range<usize>,
}

fn mangled_package(package: &str) -> String {
    // A segment has no empty part and no `-` beside a `.`, so `_` is a `.` and `__` a `-`.
    package.to_ascii_uppercase().replace('-', "__").replace('.', "_")
}

/// The internal name of `name` in the dependency `package`. The length makes it injective:
/// `a` + `B__C` and `a-b` + `C` would otherwise be one string. It stays an upper identifier,
/// which is how the compiler tells a type or module from a variable.
pub(crate) fn internal_name(package: &str, name: &str) -> String {
    let p = mangled_package(package);
    format!("PK{}_{p}_{name}", p.len())
}

/// The (package, name) an internal name stands for, or `None` for any other word.
fn identity_of(word: &str) -> Option<(String, &str)> {
    let rest = word.strip_prefix("PK")?;
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let len: usize = rest[..digits].parse().ok()?;
    let rest = rest[digits..].strip_prefix('_')?;
    let (mangled, rest) = (rest.get(..len)?, rest.get(len..)?);
    let name = rest.strip_prefix('_').filter(|n| !n.is_empty())?;
    let package = mangled.replace("__", "-").replace('_', ".").to_ascii_lowercase();
    (internal_name(&package, name) == word).then_some((package, name))
}

/// The (module, constructor) of `<MODULE>__<Ctor>`.
fn owned_ctor(bare: &str) -> Option<(&str, &str)> {
    bare.rsplit_once(CTOR_SEPARATOR).filter(|(_, c)| c.chars().any(|ch| ch.is_ascii_lowercase()))
}

/// `name` as the source spells it.
pub(crate) fn source_spelling(name: &str) -> &str {
    identity_of(name).map_or(name, |(_, bare)| owned_ctor(bare).map_or(bare, |(_, c)| c))
}

/// `text` with every internal name replaced by the full name a person reads.
pub(crate) fn readable(text: &str) -> String {
    if !text.contains("PK") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        match identity_of(word) {
            Some((package, bare)) => {
                if package != LOCAL_PACKAGE {
                    out.push_str(&format!("{package}::"));
                }
                match owned_ctor(bare) {
                    Some((module, ctor)) => out.push_str(&format!("{module}.{ctor}")),
                    None => out.push_str(bare),
                }
            }
            None => out.push_str(word),
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Makes a diagnostic readable, once nothing needs its internal names.
pub(crate) fn make_readable(d: &mut Diagnostic) {
    d.module_name = readable(&d.module_name);
    d.routine = readable(&d.routine);
    d.tag = readable(&d.tag);
    d.message = readable(&d.message);
    for c in &mut d.path_conditions {
        *c = readable(c);
    }
    for v in d.counterexample.values_mut() {
        *v = readable(v);
    }
    for l in &mut d.relevant_lemmas {
        *l = readable(l);
    }
}

/// What a package declares, or what the prelude provides.
#[derive(Default)]
struct Provided {
    /// Modules and concepts.
    tops: HashSet<String>,
    concepts: HashSet<String>,
    /// Each constructor and the modules that declare it.
    ctors: HashMap<String, Vec<String>>,
    /// Never exported (decision 7), though a use of one is told so, by `E0104`.
    entry_points: HashSet<String>,
    /// The type of each (module, routine or field), as that module spells it.
    members: HashMap<(String, String), String>,
}

impl Provided {
    fn add_file(&mut self, ast: &FileAst) {
        for c in &ast.concepts {
            self.tops.insert(c.name.clone());
            self.concepts.insert(c.name.clone());
        }
        for m in &ast.modules {
            self.add_module(m);
        }
    }

    fn add_module(&mut self, m: &ModuleDecl) {
        self.tops.insert(m.name.clone());
        if m.kind.is_entry_point() {
            self.entry_points.insert(m.name.clone());
        }
        let mut member = |name: &str, ty: &str| {
            let open = m.type_params.iter().any(|p| p == type_head(ty));
            if !open {
                self.members.insert((m.name.clone(), name.to_string()), ty.to_string());
            }
        };
        let routines = m.specify_routines.iter().chain(&m.create_routines).chain(&m.feature_routines);
        for (_, r) in routines {
            if !r.type_params.iter().any(|p| p == type_head(&r.ret_type)) {
                member(&r.name, &r.ret_type);
            }
        }
        match &m.repr {
            ReprKind::Product(fields) => fields.iter().for_each(|(f, t)| member(f, t)),
            ReprKind::Sum(variants) => {
                for v in variants {
                    self.ctors.entry(v.name.clone()).or_default().push(m.name.clone());
                }
            }
        }
    }
}

fn prelude_provides(prelude_mode: bool) -> Provided {
    let mut p = Provided::default();
    if prelude_mode {
        p.tops.extend(names::BUILTIN_TYPES.iter().map(ToString::to_string));
        let prelude = standard_prelude();
        for m in &prelude.modules {
            p.add_module(m);
        }
        for c in &prelude.concepts {
            p.tops.insert(c.name.clone());
            p.concepts.insert(c.name.clone());
        }
    }
    p
}

/// How one package's names become internal names.
#[derive(Clone, Copy)]
struct Naming<'a> {
    /// `None` for the prelude.
    package: Option<&'a str>,
    is_root: bool,
    provided: &'a Provided,
    prelude: &'a Provided,
}

impl Naming<'_> {
    fn top(&self, name: &str) -> String {
        match self.package {
            Some(p) if !self.is_root || self.prelude.tops.contains(name) => internal_name(p, name),
            _ => name.to_string(),
        }
    }

    fn ctor(&self, module: &str, name: &str) -> String {
        let Some(p) = self.package else { return name.to_string() };
        if self.provided.ctors.get(name).is_some_and(|owners| owners.len() > 1) {
            internal_name(p, &format!("{module}{CTOR_SEPARATOR}{name}"))
        } else if !self.is_root || self.prelude.ctors.contains_key(name) {
            internal_name(p, name)
        } else {
            name.to_string()
        }
    }

    /// The internal name of a head a routine of this package spells `ty`, if it names one.
    fn head(&self, ty: &str) -> Option<String> {
        let head = type_head(ty);
        if self.provided.tops.contains(head) {
            Some(self.top(head))
        } else {
            self.prelude.tops.contains(head).then(|| head.to_string())
        }
    }
}

/// One provider of a name that is not the package's own.
struct Source<'a> {
    naming: Naming<'a>,
    alias: &'a str,
}

impl Source<'_> {
    fn describe(&self) -> String {
        format!("`{}` (alias `{}`)", self.naming.package.unwrap_or(PRELUDE_PACKAGE), self.alias)
    }
}

/// A constructor a bare name could mean.
struct Candidate {
    internal: String,
    /// The internal name of its module.
    owner: String,
    spelling: String,
}

/// Resolves every name of one package.
struct Resolver<'a> {
    package: &'a str,
    own: Naming<'a>,
    sources: &'a [Source<'a>],
    src: &'a str,
    path: &'a str,
    module: String,
    routine: String,
    type_params: Vec<String>,
    /// Binders in scope with the head of their type, when it is known for certain.
    scope: Vec<(String, Option<String>)>,
    /// The declared type of the `let` that comes next.
    let_type: Option<String>,
    /// The head of the type a constructor being resolved must have, if that is known.
    hint: Option<String>,
    diags: Vec<Diagnostic>,
    reported: HashSet<(&'static str, String, String)>,
}

impl<'a> Resolver<'a> {
    fn report(&mut self, rule: &'static str, name: &str, message: String) {
        if !self.reported.insert((rule, name.to_string(), self.module.clone())) {
            return;
        }
        let tag = rule.split_once(": ").map_or(rule, |(_, t)| t).to_string();
        let span = locate_span(self.src, self.path, name, &self.routine);
        self.diags.push(
            Diagnostic::basic("error", rule, self.routine.clone(), tag, format!("{rule}: {message}"))
                .with_module(&self.module)
                .with_ai_context(span, Vec::new(), BTreeMap::new(), Vec::new()),
        );
    }

    /// The internal name of `name` among the providers `of` selects.
    fn outside(
        &mut self,
        name: &str,
        of: fn(&Provided) -> &HashSet<String>,
        rule: &'static str,
    ) -> String {
        let mut found: Vec<&Source<'_>> =
            self.sources.iter().filter(|s| of(s.naming.provided).contains(name)).collect();
        if found.len() > 1 {
            found.retain(|s| !s.naming.provided.entry_points.contains(name));
        }
        match found.as_slice() {
            [] => name.to_string(),
            [one] => one.naming.top(name),
            several => {
                let names: Vec<String> = several.iter().map(|s| s.describe()).collect();
                let spellings: Vec<String> =
                    several.iter().map(|s| format!("`{}::{name}`", s.alias)).collect();
                self.report(
                    rule,
                    name,
                    format!(
                        "`{name}` is provided by {}, and a bare name cannot tell them apart. Write {}",
                        names.join(" and "),
                        spellings.join(" or ")
                    ),
                );
                name.to_string()
            }
        }
    }

    fn top(&mut self, name: &str) -> String {
        if self.own.provided.tops.contains(name) {
            return self.own.top(name);
        }
        self.outside(name, |p| &p.tops, "E0105: ambiguous_module_reference")
    }

    /// The source a qualified name starts from.
    fn source(&mut self, alias: &str, name: &str) -> Option<Naming<'a>> {
        if let Some(s) = self.sources.iter().find(|s| s.alias == alias) {
            return Some(s.naming);
        }
        let known: Vec<String> = self.sources.iter().map(|s| format!("`{}`", s.alias)).collect();
        self.report(
            "E0106: unknown_module",
            alias,
            format!("`{alias}::{name}`: `{alias}` is not an alias of `{}`, which has {}", self.package, known.join(", ")),
        );
        None
    }

    fn qualified_top(&mut self, alias: &str, name: &str) -> String {
        let Some(n) = self.source(alias, name) else { return name.to_string() };
        if n.provided.tops.contains(name) {
            return n.top(name);
        }
        self.not_provided(alias, name)
    }

    fn not_provided(&mut self, alias: &str, name: &str) -> String {
        self.report(
            "E0106: unknown_module",
            name,
            format!("`{alias}::{name}`: `{alias}` provides no `{name}`"),
        );
        name.to_string()
    }

    /// `ALIAS::NAME` in an expression, where it may be a module or a constructor.
    fn qualified(&mut self, alias: &str, name: &str, receiver: bool) -> String {
        let Some(n) = self.source(alias, name) else { return name.to_string() };
        let owners = n.provided.ctors.get(name);
        if n.provided.tops.contains(name) && (receiver || owners.is_none()) {
            return n.top(name);
        }
        match owners.map(Vec::as_slice) {
            None | Some([]) => self.not_provided(alias, name),
            Some([owner]) => n.ctor(owner, name),
            Some(several) => {
                let candidates = several
                    .iter()
                    .map(|owner| Candidate {
                        internal: n.ctor(owner, name),
                        owner: n.top(owner),
                        spelling: format!("{alias}::{owner}.{name}"),
                    })
                    .collect();
                self.choose(name, candidates)
            }
        }
    }

    /// The constructors named `name` that other packages and the prelude declare, and whether
    /// each is the prelude's.
    fn outside_ctors(&self, name: &str) -> Vec<(Candidate, bool)> {
        let mut found: Vec<(Candidate, bool)> = Vec::new();
        for s in self.sources {
            for owner in s.naming.provided.ctors.get(name).into_iter().flatten() {
                let internal = s.naming.ctor(owner, name);
                if !found.iter().any(|(c, _)| c.internal == internal) {
                    let spelling = format!("{}::{owner}.{name}", s.alias);
                    let prelude = s.naming.package.is_none();
                    found.push((Candidate { internal, owner: s.naming.top(owner), spelling }, prelude));
                }
            }
        }
        found
    }

    /// One of several constructors. A type known for certain breaks a tie, and is not asked
    /// when there is none, so it can never change what code means (decision 9).
    fn choose(&mut self, name: &str, candidates: Vec<Candidate>) -> String {
        let hint = self.hint.take();
        if let [one] = candidates.as_slice() {
            return one.internal.clone();
        }
        if let Some(c) = candidates.iter().find(|c| hint.as_deref() == Some(c.owner.as_str())) {
            return c.internal.clone();
        }
        let spellings: Vec<String> = candidates.iter().map(|c| format!("`{}`", c.spelling)).collect();
        self.report(
            "E0112: ambiguous_constructor",
            name,
            format!("`{name}` could be any of several constructors here. Write {}", spellings.join(" or ")),
        );
        name.to_string()
    }

    /// Nearest first: the module's own, then its package's, then its dependencies', then the
    /// prelude's. The prelude comes last so that a constructor added to it cannot make a name
    /// that a program already uses ambiguous. A type known for certain still picks among all of
    /// them, since any other choice would not type-check.
    fn ctor(&mut self, name: &str) -> String {
        let own = self.own;
        if let Some(owners) = own.provided.ctors.get(name) {
            if owners.iter().any(|o| *o == self.module) {
                self.hint = None;
                return own.ctor(&self.module, name);
            }
            let candidates = owners
                .iter()
                .map(|o| Candidate {
                    internal: own.ctor(o, name),
                    owner: own.top(o),
                    spelling: format!("{o}.{name}"),
                })
                .collect();
            return self.choose(name, candidates);
        }
        let mut found = self.outside_ctors(name);
        if found.is_empty() {
            self.hint = None;
            return name.to_string();
        }
        if let Some((c, _)) = found.iter().find(|(c, _)| self.hint.as_deref() == Some(c.owner.as_str())) {
            self.hint = None;
            return c.internal.clone();
        }
        if found.iter().any(|(_, prelude)| !prelude) {
            found.retain(|(_, prelude)| !prelude);
        }
        self.choose(name, found.into_iter().map(|(c, _)| c).collect())
    }

    /// The package and bare name of the module `module` spells here, reporting what `top`
    /// would.
    fn module_named(&mut self, module: &str) -> Option<(Naming<'a>, String)> {
        if let Some((alias, name)) = module.split_once("::") {
            let n = self.source(alias, name)?;
            return n.provided.tops.contains(name).then(|| (n, name.to_string()));
        }
        if self.own.provided.tops.contains(module) {
            return Some((self.own, module.to_string()));
        }
        let mut found = self.sources.iter().filter(|s| s.naming.provided.tops.contains(module));
        match (found.next(), found.next()) {
            (Some(one), None) => Some((one.naming, module.to_string())),
            _ => {
                self.top(module);
                None
            }
        }
    }

    /// `MODULE.Ctor`, where `module` is as the source spells it.
    fn module_ctor(&mut self, module: &str, ctor: &str) -> String {
        self.hint = None;
        let Some((n, bare)) = self.module_named(module) else {
            return ctor.to_string();
        };
        if n.provided.ctors.get(ctor).is_some_and(|owners| owners.contains(&bare)) {
            return n.ctor(&bare, ctor);
        }
        self.report(
            "E0109: unknown_constructor",
            ctor,
            format!("`{module}` declares no constructor `{ctor}`"),
        );
        ctor.to_string()
    }

    fn is_ctor(&self, name: &str) -> bool {
        self.own.provided.ctors.contains_key(name)
            || self.sources.iter().any(|s| s.naming.provided.ctors.contains_key(name))
    }

    fn is_top(&self, name: &str) -> bool {
        self.own.provided.tops.contains(name)
            || self.sources.iter().any(|s| s.naming.provided.tops.contains(name))
    }

    /// The package and bare name of an internal module name.
    fn internal_provider(&self, head: &str) -> Option<(Naming<'a>, String)> {
        let bare = source_spelling(head);
        std::iter::once(self.own)
            .chain(self.sources.iter().map(|s| s.naming))
            .find(|n| n.provided.tops.contains(bare) && n.top(bare) == head)
            .map(|n| (n, bare.to_string()))
    }

    /// The internal head of the type of a resolved `e`, when it is known for certain.
    fn head_of(&self, e: &Expr) -> Option<String> {
        let own_member = |name: &str| {
            let ty = self.own.provided.members.get(&(self.module.clone(), name.to_string()))?;
            if ty == "Current" { Some(self.own.top(&self.module)) } else { self.own.head(ty) }
        };
        match e {
            Expr::Var(v) if v == "Current" => Some(self.own.top(&self.module)),
            Expr::Var(v) => match self.scope.iter().rev().find(|(n, _)| n == v) {
                Some((_, ty)) => ty.clone(),
                None => own_member(v),
            },
            Expr::Call(f, _) => own_member(f),
            Expr::MethodCall(recv, routine, _) => {
                if routine == "read_line" && matches!(&**recv, Expr::Var(v) if v == "IO") {
                    return Some("RESULT".to_string());
                }
                let head = match &**recv {
                    Expr::Var(v) if v.starts_with(|c: char| c.is_ascii_uppercase()) && v != "Current" => v.clone(),
                    other => self.head_of(other)?,
                };
                let (n, bare) = self.internal_provider(&head)?;
                let ty = n.provided.members.get(&(bare, routine.clone()))?;
                if ty == "Current" { Some(head) } else { n.head(ty) }
            }
            _ => None,
        }
    }

    /// Whether an upper-case word can name a module or constructor. Every value binder is
    /// lower-case, so type parameters are the only things that can shadow one.
    fn resolves(&self, name: &str) -> bool {
        name.starts_with(|c: char| c.is_ascii_uppercase())
            && !RESERVED.contains(&name)
            && !self.type_params.iter().any(|t| t == name)
    }

    /// An upper-case word in an expression. Only a receiver can be a module when a constructor
    /// has the same name, as `Leaf` alone builds a value.
    fn word(&mut self, name: &mut String, receiver: bool) {
        if let Some((alias, bare)) = name.clone().split_once("::") {
            self.hint = None;
            *name = self.qualified(alias, bare, receiver);
            return;
        }
        if !self.resolves(name) {
            self.hint = None;
            return;
        }
        let is_module = self.is_top(name) && (receiver || !self.is_ctor(name));
        *name = if is_module { self.top(name) } else { self.ctor(name) };
    }

    fn ty(&mut self, ty: &mut String) {
        let mut out = String::with_capacity(ty.len());
        let mut word = String::new();
        let chars: Vec<char> = ty.chars().chain(std::iter::once(' ')).collect();
        for (i, &c) in chars.iter().enumerate() {
            // A refined function type has `:` too, but never two together.
            let in_colons = c == ':' && (chars.get(i + 1) == Some(&':') || (i > 0 && chars[i - 1] == ':'));
            if c.is_ascii_alphanumeric() || c == '_' || in_colons {
                word.push(c);
                continue;
            }
            if !word.is_empty() {
                if let Some((alias, bare)) = word.clone().split_once("::") {
                    word = self.qualified_top(alias, bare);
                } else if self.resolves(&word) {
                    word = self.top(&word);
                }
                out.push_str(&word);
                word.clear();
            }
            out.push(c);
        }
        out.pop();
        *ty = out;
    }

    fn types(&mut self, typed: &mut [(String, String)]) {
        for (_, t) in typed {
            self.ty(t);
        }
    }

    /// Decision 7: an export list names modules of this package, or a concept.
    fn visibility(&mut self, vis: &mut Visibility) {
        let Visibility::Modules(names) = vis else { return };
        for name in names {
            if self.own.provided.tops.contains(name.as_str()) {
                *name = self.own.top(name);
            } else if self.sources.iter().any(|s| s.naming.provided.concepts.contains(name.as_str())) {
                *name = self.top(name);
            } else if self.is_top(name) {
                self.report(
                    "E0114: export_outside_package",
                    name,
                    format!("an export list names modules of the same package, and `{name}` is not in `{}`", self.package),
                );
            }
        }
    }

    fn pattern(&mut self, p: &mut Pattern) {
        match p {
            Pattern::Variant(c, subs) => {
                *c = match c.clone() {
                    spelled if spelled.contains('.') => {
                        let (module, ctor) = spelled.rsplit_once('.').expect("it has a `.`");
                        self.module_ctor(module, ctor)
                    }
                    spelled => match spelled.split_once("::") {
                        Some((alias, bare)) => self.qualified(alias, bare, false),
                        None if self.resolves(&spelled) => self.ctor(&spelled),
                        None => spelled,
                    },
                };
                self.hint = None;
                for s in subs {
                    self.pattern(s);
                }
            }
            Pattern::Tuple(subs) => {
                self.hint = None;
                for s in subs {
                    self.pattern(s);
                }
            }
            Pattern::Wildcard | Pattern::Bind(_) | Pattern::Lit(_) => {}
        }
    }

    fn stmts(&mut self, stmts: &mut [Stmt]) {
        for st in stmts {
            match st {
                Stmt::Let(v, e) => {
                    self.expr(e);
                    let head = self.let_type.take().or_else(|| self.head_of(e));
                    self.scope.push((v.clone(), head));
                }
                Stmt::Check(_, e) | Stmt::Assume(_, e) => self.expr(e),
                Stmt::LetType(_, t) => {
                    self.ty(t);
                    self.let_type = Some(type_head(t).to_string());
                }
                Stmt::Reveal(_) => {}
            }
        }
    }

    fn block(&mut self, stmts: &mut [Stmt], body: &mut Expr) {
        self.block_of(stmts, body, None);
    }

    /// A block whose value is the routine's result when `head` is given.
    fn block_of(&mut self, stmts: &mut [Stmt], body: &mut Expr, head: Option<&str>) {
        let mark = self.scope.len();
        self.stmts(stmts);
        match head {
            Some(h) => self.returned(body, h),
            None => self.expr(body),
        }
        self.scope.truncate(mark);
    }

    /// `e` where a routine returns it, so its declared type settles a constructor built
    /// directly. An `if` or `match` passes that on to its branches.
    fn returned(&mut self, e: &mut Expr, head: &str) {
        match e {
            Expr::Var(v) if self.resolves(v) || v.contains("::") => {
                self.hint = Some(head.to_string());
                self.word(v, false);
                self.hint = None;
            }
            Expr::Call(f, args) => {
                self.hint = Some(head.to_string());
                self.word(f, false);
                self.hint = None;
                args.iter_mut().for_each(|a| self.expr(a));
            }
            Expr::If(cond, then_stmts, then_e, else_stmts, else_e) => {
                self.expr(cond);
                self.block_of(then_stmts, then_e, Some(head));
                self.block_of(else_stmts, else_e, Some(head));
            }
            Expr::Match(scrut, cases) => self.match_cases(scrut, cases, Some(head)),
            other => self.expr(other),
        }
    }

    fn match_cases(&mut self, scrut: &mut Expr, cases: &mut [MatchCase], head: Option<&str>) {
        self.expr(scrut);
        let ty = self.head_of(scrut);
        for case in cases {
            self.hint.clone_from(&ty);
            self.pattern(&mut case.pattern);
            self.hint = None;
            let mark = self.scope.len();
            self.scope.extend(case.pattern.collect_bindings().into_iter().map(|b| (b, None)));
            self.block_of(&mut case.stmts, &mut case.body, head);
            self.scope.truncate(mark);
        }
    }

    fn fields(&mut self, fields: &mut [(String, Expr)]) {
        for (_, e) in fields {
            self.expr(e);
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Int(_) | Expr::Float(_) | Expr::Char(_) | Expr::Unit | Expr::Bool(_) | Expr::Str(_) => {}
            Expr::Var(v) => self.word(v, false),
            Expr::UnaryNot(a) | Expr::UnaryNeg(a) => self.expr(a),
            Expr::BinOp(_, a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Call(f, args) => {
                self.word(f, false);
                args.iter_mut().for_each(|a| self.expr(a));
            }
            Expr::MethodCall(recv, member, args) => {
                if member.starts_with(|c: char| c.is_ascii_uppercase()) {
                    if let Expr::Var(v) = &**recv {
                        if v.contains("::") || self.resolves(v) {
                            let ctor = self.module_ctor(v, member);
                            let mut args = std::mem::take(args);
                            args.iter_mut().for_each(|a| self.expr(a));
                            *e = if args.is_empty() { Expr::Var(ctor) } else { Expr::Call(ctor, args) };
                            return;
                        }
                    }
                }
                match &mut **recv {
                    Expr::Var(v) => self.word(v, true),
                    other => self.expr(other),
                }
                args.iter_mut().for_each(|a| self.expr(a));
            }
            Expr::If(cond, then_stmts, then_e, else_stmts, else_e) => {
                self.expr(cond);
                self.block(then_stmts, then_e);
                self.block(else_stmts, else_e);
            }
            Expr::Match(scrut, cases) => self.match_cases(scrut, cases, None),
            Expr::RecordLit(fields) => self.fields(fields),
            Expr::RecordUpdate(base, fields) => {
                self.expr(base);
                self.fields(fields);
            }
            Expr::TupleLit(items) => items.iter_mut().for_each(|a| self.expr(a)),
            Expr::Forall(v, t, body) | Expr::Exists(v, t, body) => {
                self.ty(t);
                self.scope.push((v.clone(), Some(type_head(t).to_string())));
                self.expr(body);
                self.scope.pop();
            }
            Expr::WithIo(steps, result) => {
                let mark = self.scope.len();
                for step in steps {
                    match step {
                        IoStmt::Bind(v, e) => {
                            self.expr(e);
                            let head = self.head_of(e);
                            self.scope.push((v.clone(), head));
                        }
                        IoStmt::Action(e) => self.expr(e),
                    }
                }
                self.expr(result);
                self.scope.truncate(mark);
            }
            Expr::Lambda(params, ret, stmts, body) => {
                self.types(params);
                self.ty(ret);
                let mark = self.scope.len();
                self.bind_params(params);
                self.block(stmts, body);
                self.scope.truncate(mark);
            }
        }
    }

    fn clauses(&mut self, clauses: &mut [(String, Expr)]) {
        for (_, e) in clauses {
            self.expr(e);
        }
    }

    fn bind_params(&mut self, params: &[(String, String)]) {
        self.scope.extend(params.iter().map(|(p, t)| (p.clone(), Some(type_head(t).to_string()))));
    }

    fn with_type_params(&mut self, params: &[String], f: impl FnOnce(&mut Self)) {
        let mark = self.type_params.len();
        self.type_params.extend(params.iter().cloned());
        f(self);
        self.type_params.truncate(mark);
    }

    fn routine(&mut self, r: &mut RoutineDecl) {
        self.routine.clone_from(&r.name);
        let params = r.type_params.clone();
        self.with_type_params(&params, |this| {
            this.types(&mut r.params);
            this.ty(&mut r.ret_type);
            let mark = this.scope.len();
            this.bind_params(&r.params);
            this.clauses(&mut r.requires);
            r.variant.iter_mut().for_each(|e| this.expr(e));
            let head = if r.ret_type == "Current" {
                this.own.top(&this.module)
            } else {
                type_head(&r.ret_type).to_string()
            };
            this.block_of(&mut r.stmts, &mut r.body, Some(&head));
            this.scope.push(("Result".to_string(), Some(head)));
            this.clauses(&mut r.ensures);
            this.scope.truncate(mark);
        });
    }

    fn law(&mut self, l: &mut LawDecl) {
        self.routine.clone_from(&l.name);
        let params = l.type_params.clone();
        self.with_type_params(&params, |this| {
            this.types(&mut l.params);
            let mark = this.scope.len();
            this.bind_params(&l.params);
            this.clauses(&mut l.requires);
            l.triggers.iter_mut().for_each(|e| this.expr(e));
            l.variant.iter_mut().for_each(|e| this.expr(e));
            this.stmts(&mut l.stmts);
            if let Some(body) = &mut l.body {
                this.expr(body);
            }
            this.clauses(&mut l.ensures);
            this.scope.truncate(mark);
        });
    }

    fn concept(&mut self, c: &mut ConceptDecl) {
        self.module.clone_from(&c.name);
        c.name = self.own.top(&c.name);
        let params: Vec<String> = c.type_params.iter().map(|(n, _)| n.clone()).collect();
        self.with_type_params(&params, |this| {
            for (_, bound) in &mut c.type_params {
                if let Some(b) = bound {
                    this.ty(b);
                }
            }
            for parent in &mut c.refines {
                this.ty(parent);
            }
            for s in &mut c.signatures {
                this.routine.clone_from(&s.name);
                this.types(&mut s.params);
                this.ty(&mut s.ret_type);
            }
            c.default_routines.iter_mut().for_each(|r| this.routine(r));
            c.laws.iter_mut().for_each(|l| this.law(l));
        });
    }

    fn module(&mut self, m: &mut ModuleDecl) {
        self.module.clone_from(&m.name);
        self.routine.clear();
        m.name = self.own.top(&m.name);
        let params = m.type_params.clone();
        self.with_type_params(&params, |this| {
            for bound in m.type_bounds.values_mut() {
                this.ty(bound);
            }
            if let Some(sat) = &mut m.satisfies {
                sat.concepts.iter_mut().for_each(|c| this.ty(c));
                sat.where_constraints.iter_mut().for_each(|(_, c)| this.ty(c));
            }
            this.visibility(&mut m.repr_vis);
            match &mut m.repr {
                ReprKind::Product(fields) => this.types(fields),
                ReprKind::Sum(variants) => {
                    for v in variants {
                        let source_module = this.module.clone();
                        v.name = this.own.ctor(&source_module, &v.name);
                        this.types(&mut v.fields);
                    }
                }
            }
            for (vis, r) in m
                .specify_routines
                .iter_mut()
                .chain(&mut m.create_routines)
                .chain(&mut m.feature_routines)
            {
                this.visibility(vis);
                this.routine(r);
            }
            for (vis, l) in &mut m.specify_laws {
                this.visibility(vis);
                this.law(l);
            }
            this.routine.clear();
            this.clauses(&mut m.invariants);
        });
    }
}

/// `E0115`: a name of a package may not look like an internal name, or it could be another
/// package's.
fn reserved_names(ast: &FileAst, u: &SourceUnit) -> Vec<Diagnostic> {
    let ctors = ast.modules.iter().flat_map(|m| match &m.repr {
        ReprKind::Sum(variants) => variants.iter().map(|v| v.name.as_str()).collect(),
        ReprKind::Product(_) => Vec::new(),
    });
    top_level_decls(ast)
        .into_iter()
        .map(|(name, _)| name)
        .chain(ctors)
        .filter(|name| identity_of(name).is_some())
        .map(|name| {
            let rule = "E0115: reserved_name";
            Diagnostic::basic(
                "error",
                rule,
                name.to_string(),
                "reserved_name".to_string(),
                format!("{rule}: `{name}` has the form of a dependency's internal name, so it cannot be declared"),
            )
            .with_module(name)
            .with_ai_context(locate_span(&u.src, &u.path, name, ""), Vec::new(), BTreeMap::new(), Vec::new())
        })
        .collect()
}

/// Checks each package's own namespace, then gives every name of every package its internal
/// name. Returns the package of each dependency module, and what stops the build.
pub(crate) fn resolve_packages(
    units: &mut [SourceUnit],
    packages: &[PackageUnits],
    prelude_mode: bool,
) -> (HashMap<String, String>, Vec<Diagnostic>) {
    let mut fatal = Vec::new();
    for p in packages {
        fatal.extend(check_project_namespace(&units[p.units.clone()]));
        for u in &units[p.units.clone()] {
            fatal.extend(reserved_names(&u.ast, u));
        }
    }
    if !fatal.is_empty() {
        return (HashMap::new(), fatal);
    }

    let prelude = prelude_provides(prelude_mode);
    let mut provided: HashMap<&str, Provided> = HashMap::new();
    for p in packages {
        let mut own = Provided::default();
        for u in &units[p.units.clone()] {
            own.add_file(&u.ast);
        }
        provided.insert(&p.name, own);
    }

    let mut owners = HashMap::new();
    for p in packages {
        let core = Naming { package: None, is_root: false, provided: &prelude, prelude: &prelude };
        let mut sources = vec![Source { naming: core, alias: PRELUDE_ALIAS }];
        for (dep, alias) in &p.deps {
            let provided = &provided[dep.as_str()];
            let naming = Naming { package: Some(dep), is_root: false, provided, prelude: &prelude };
            sources.push(Source { naming, alias });
        }
        let own = Naming {
            package: Some(&p.name),
            is_root: p.is_root,
            provided: &provided[p.name.as_str()],
            prelude: &prelude,
        };
        for u in &mut units[p.units.clone()] {
            let SourceUnit { path, src, ast } = u;
            let mut r = Resolver {
                package: &p.name,
                own,
                sources: &sources,
                src,
                path,
                module: String::new(),
                routine: String::new(),
                type_params: Vec::new(),
                scope: Vec::new(),
                let_type: None,
                hint: None,
                diags: Vec::new(),
                reported: HashSet::new(),
            };
            if let Some(group) = &mut ast.group {
                for name in group.iter_mut() {
                    *name = r.own.top(name);
                }
            }
            ast.concepts.iter_mut().for_each(|c| r.concept(c));
            ast.modules.iter_mut().for_each(|m| r.module(m));
            fatal.append(&mut r.diags);
            if !p.is_root {
                owners.extend(ast.modules.iter().map(|m| (m.name.clone(), p.name.clone())));
            }
        }
    }
    (owners, fatal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_internal_name_is_one_identity_and_reads_back() {
        for (package, name) in [
            ("acme.json", "PARSER"),
            ("acme.http-client", "Leaf"),
            ("a", "B__C"),
            ("a-b", "C"),
            ("a.b", "C"),
            ("a--b.c2", "X_Y"),
        ] {
            let internal = internal_name(package, name);
            assert_eq!(identity_of(&internal), Some((package.to_string(), name)), "{internal}");
            assert_eq!(source_spelling(&internal), name);
        }
        assert_ne!(internal_name("a", "B__C"), internal_name("a-b", "C"));
        assert_ne!(internal_name("a.b", "C"), internal_name("a-b", "C"));
        assert_eq!(internal_name("acme.json", "PARSER"), "PK9_ACME_JSON_PARSER");
    }

    #[test]
    fn other_words_are_left_alone() {
        for word in ["PARSER", "PK", "PK9", "PK9_ACME_JSON", "PK99_ACME_JSON_PARSER", "PK2_a_X", "PK02_AB_X"] {
            assert_eq!(identity_of(word), None, "{word}");
            assert_eq!(source_spelling(word), word);
        }
    }

    #[test]
    fn readable_replaces_whole_words() {
        let text = "`PK9_ACME_JSON_PARSER` and SEQ[PK9_ACME_JSON_Leaf], not PK9_ACME_JSON";
        assert_eq!(readable(text), "`acme.json::PARSER` and SEQ[acme.json::Leaf], not PK9_ACME_JSON");
    }
}
