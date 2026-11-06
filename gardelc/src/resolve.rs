//! Package identity: which module, concept or constructor a name in a package means
//! (Roadmap decisions 4, 5, 7, 8).
//!
//! A module's identity is (package, name), and the compiler after this pass keeps one string
//! per identity: its internal name. The root project and the prelude keep their bare names,
//! so a project with no dependencies compiles to the same text as before. A dependency's names
//! become `PK<n>_<PACKAGE>_<NAME>` (see [`internal_name`]), which is built from the full package
//! name, never from an alias. Nothing else can hold that string, which makes the names
//! `gardelc` builds in (`EVENT`, `COMMAND`) identities: bare `COMMAND` can only be the prelude's.
//!
//! Each package is resolved against what its own manifest lists, nearest first: the package's
//! own names, then the prelude and its direct dependencies on one level. Several providers
//! on that level are an error, never a choice. A transitive dependency provides no name.
//!
//! The internal name can be read back, so diagnostics and source lookups need no table.

use super::*;
use std::ops::Range;

/// The prelude's full name (decision 8). Its internal names are its bare names.
pub(crate) const PRELUDE_PACKAGE: &str = "gardel.core";
const PRELUDE_ALIAS: &str = "GARDEL";

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

/// `name` as the source spells it.
pub(crate) fn source_spelling(name: &str) -> &str {
    identity_of(name).map_or(name, |(_, bare)| bare)
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
            Some((package, bare)) => out.push_str(&format!("{package}::{bare}")),
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
    ctors: HashSet<String>,
    /// Never exported (decision 7), though a use of one is told so, by `E0104`.
    entry_points: HashSet<String>,
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
        if let ReprKind::Sum(variants) = &m.repr {
            self.ctors.extend(variants.iter().map(|v| v.name.clone()));
        }
    }
}

fn prelude_provides(prelude_mode: bool) -> Provided {
    let mut p = Provided::default();
    p.tops.extend(names::BUILTIN_TYPES.iter().map(ToString::to_string));
    if prelude_mode {
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

/// One provider of a name that is not the package's own.
struct Source<'a> {
    /// `None` for the prelude.
    package: Option<&'a str>,
    alias: &'a str,
    provided: &'a Provided,
}

impl Source<'_> {
    fn internal(&self, name: &str) -> String {
        self.package.map_or_else(|| name.to_string(), |p| internal_name(p, name))
    }

    fn describe(&self) -> String {
        format!("`{}` (alias `{}`)", self.package.unwrap_or(PRELUDE_PACKAGE), self.alias)
    }
}

/// Resolves every name of one package.
struct Resolver<'a> {
    package: &'a str,
    is_root: bool,
    own: &'a Provided,
    sources: &'a [Source<'a>],
    src: &'a str,
    path: &'a str,
    module: String,
    routine: String,
    type_params: Vec<String>,
    diags: Vec<Diagnostic>,
    reported: HashSet<(&'static str, String, String)>,
}

impl Resolver<'_> {
    fn own_internal(&self, name: &str) -> String {
        if self.is_root { name.to_string() } else { internal_name(self.package, name) }
    }

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
            self.sources.iter().filter(|s| of(s.provided).contains(name)).collect();
        if found.len() > 1 {
            found.retain(|s| !s.provided.entry_points.contains(name));
        }
        match found.as_slice() {
            [] => name.to_string(),
            [one] => one.internal(name),
            several => {
                let names: Vec<String> = several.iter().map(|s| s.describe()).collect();
                self.report(
                    rule,
                    name,
                    format!(
                        "`{name}` is provided by {}, and a bare name cannot tell them apart",
                        names.join(" and ")
                    ),
                );
                name.to_string()
            }
        }
    }

    fn top(&mut self, name: &str) -> String {
        if self.own.tops.contains(name) {
            return self.own_internal(name);
        }
        self.outside(name, |p| &p.tops, "E0105: ambiguous_module_reference")
    }

    fn ctor(&mut self, name: &str) -> String {
        if self.own.ctors.contains(name) {
            return self.own_internal(name);
        }
        self.outside(name, |p| &p.ctors, "E0112: ambiguous_constructor")
    }

    fn is_ctor(&self, name: &str) -> bool {
        self.own.ctors.contains(name) || self.sources.iter().any(|s| s.provided.ctors.contains(name))
    }

    fn is_top(&self, name: &str) -> bool {
        self.own.tops.contains(name) || self.sources.iter().any(|s| s.provided.tops.contains(name))
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
        if !self.resolves(name) {
            return;
        }
        let is_module = self.is_top(name) && (receiver || !self.is_ctor(name));
        *name = if is_module { self.top(name) } else { self.ctor(name) };
    }

    fn ty(&mut self, ty: &mut String) {
        let mut out = String::with_capacity(ty.len());
        let mut word = String::new();
        for c in ty.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_alphanumeric() || c == '_' {
                word.push(c);
                continue;
            }
            if !word.is_empty() {
                if self.resolves(&word) {
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
            if self.own.tops.contains(name.as_str()) {
                *name = self.own_internal(name);
            } else if self.sources.iter().any(|s| s.provided.concepts.contains(name.as_str())) {
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
                if self.resolves(c) {
                    *c = self.ctor(c);
                }
                for s in subs {
                    self.pattern(s);
                }
            }
            Pattern::Tuple(subs) => {
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
                Stmt::Let(_, e) | Stmt::Check(_, e) | Stmt::Assume(_, e) => self.expr(e),
                Stmt::Reveal(_) => {}
            }
        }
    }

    fn block(&mut self, stmts: &mut [Stmt], body: &mut Expr) {
        self.stmts(stmts);
        self.expr(body);
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
            Expr::MethodCall(recv, _, args) => {
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
            Expr::Match(scrut, cases) => {
                self.expr(scrut);
                for case in cases {
                    self.pattern(&mut case.pattern);
                    self.block(&mut case.stmts, &mut case.body);
                }
            }
            Expr::RecordLit(fields) => self.fields(fields),
            Expr::RecordUpdate(base, fields) => {
                self.expr(base);
                self.fields(fields);
            }
            Expr::TupleLit(items) => items.iter_mut().for_each(|a| self.expr(a)),
            Expr::Forall(_, t, body) | Expr::Exists(_, t, body) => {
                self.ty(t);
                self.expr(body);
            }
            Expr::WithIo(steps, result) => {
                for step in steps {
                    match step {
                        IoStmt::Bind(_, e) | IoStmt::Action(e) => self.expr(e),
                    }
                }
                self.expr(result);
            }
            Expr::Lambda(params, ret, stmts, body) => {
                self.types(params);
                self.ty(ret);
                self.block(stmts, body);
            }
        }
    }

    fn clauses(&mut self, clauses: &mut [(String, Expr)]) {
        for (_, e) in clauses {
            self.expr(e);
        }
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
            this.clauses(&mut r.requires);
            r.variant.iter_mut().for_each(|e| this.expr(e));
            this.stmts(&mut r.stmts);
            this.expr(&mut r.body);
            this.clauses(&mut r.ensures);
        });
    }

    fn law(&mut self, l: &mut LawDecl) {
        self.routine.clone_from(&l.name);
        let params = l.type_params.clone();
        self.with_type_params(&params, |this| {
            this.types(&mut l.params);
            this.clauses(&mut l.requires);
            l.triggers.iter_mut().for_each(|e| this.expr(e));
            l.variant.iter_mut().for_each(|e| this.expr(e));
            this.stmts(&mut l.stmts);
            if let Some(body) = &mut l.body {
                this.expr(body);
            }
            this.clauses(&mut l.ensures);
        });
    }

    fn concept(&mut self, c: &mut ConceptDecl) {
        self.module.clone_from(&c.name);
        c.name = self.own_internal(&c.name);
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
        m.name = self.own_internal(&m.name);
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
                        v.name = this.own_internal(&v.name);
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
        fatal.extend(check_project_namespace(&units[p.units.clone()], prelude_mode));
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
        let mut sources = vec![Source { package: None, alias: PRELUDE_ALIAS, provided: &prelude }];
        for (dep, alias) in &p.deps {
            sources.push(Source { package: Some(dep), alias, provided: &provided[dep.as_str()] });
        }
        for u in &mut units[p.units.clone()] {
            let SourceUnit { path, src, ast } = u;
            let mut r = Resolver {
                package: &p.name,
                is_root: p.is_root,
                own: &provided[p.name.as_str()],
                sources: &sources,
                src,
                path,
                module: String::new(),
                routine: String::new(),
                type_params: Vec::new(),
                diags: Vec::new(),
                reported: HashSet::new(),
            };
            if let Some(group) = &mut ast.group {
                for name in group.iter_mut() {
                    *name = r.own_internal(name);
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
