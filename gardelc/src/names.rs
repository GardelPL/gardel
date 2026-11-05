//! Name resolution, before any Boogie is generated (spec §4.2).
//!
//! Every name in a run must mean something: a module, a type, a routine, a constructor or a
//! variable. Without this pass an undefined name reaches Boogie, which reports it against a
//! temporary file. A private member of another module is refused here too (`E0113`), since
//! the visibility rules of `check_visibility_and_semantics` see one module at a time.
//!
//! A receiver's type is followed only as far as parameters, `let`s and routine results go.
//! A member on a receiver of unknown type is left alone, and a failure there is reported by
//! the `E0199` backstop in `parse_boogie_output`.

use super::*;

const BUILTIN_TYPES: &[&str] = &[
    "U64", "NAT", "I64", "INT", "F64", "BOOL", "CHAR", "UNIT", "EXIT_CODE", "MATH_INT", "LIST",
    "TUPLE", "IO",
];

static ANY_VISIBILITY: Visibility = Visibility::Any;

struct Member<'a> {
    vis: &'a Visibility,
    /// Declared parameters, or `None` when the count does not decide a call: a field, a name
    /// declared twice, or a routine returning a function.
    arity: Option<usize>,
    ret: Option<&'a str>,
    /// Laws state facts, and may be invoked as lemmas from anywhere.
    is_law: bool,
}

struct Universe<'a> {
    modules: HashMap<&'a str, &'a ModuleDecl>,
    members: HashMap<&'a str, HashMap<&'a str, Member<'a>>>,
    concepts: &'a HashMap<String, ConceptDecl>,
    /// Constructor name -> user modules declaring it.
    user_ctors: HashMap<&'a str, Vec<&'a str>>,
    prelude_ctors: HashSet<&'a str>,
}

fn all_caps(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn starts_upper(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn ctor_names(m: &ModuleDecl) -> Vec<&str> {
    match &m.repr {
        ReprKind::Sum(variants) => variants.iter().map(|v| v.name.as_str()).collect(),
        ReprKind::Product(_) => Vec::new(),
    }
}

fn add_member<'a>(table: &mut HashMap<&'a str, Member<'a>>, name: &'a str, member: Member<'a>) {
    table
        .entry(name)
        .and_modify(|old| {
            if old.arity != member.arity {
                old.arity = None;
            }
        })
        .or_insert(member);
}

fn routine_member<'a>(vis: &'a Visibility, r: &'a RoutineDecl) -> Member<'a> {
    let returns_function = r.ret_type.contains("->");
    Member {
        vis,
        arity: (!returns_function).then_some(r.params.len()),
        ret: Some(&r.ret_type),
        is_law: false,
    }
}

fn add_concept_members<'a>(
    table: &mut HashMap<&'a str, Member<'a>>,
    name: &str,
    concepts: &'a HashMap<String, ConceptDecl>,
    seen: &mut HashSet<String>,
) {
    if !seen.insert(name.to_string()) {
        return;
    }
    let Some(c) = concepts.get(name) else { return };
    for s in &c.signatures {
        table.entry(&s.name).or_insert(Member {
            vis: &ANY_VISIBILITY,
            arity: None,
            ret: Some(&s.ret_type),
            is_law: false,
        });
    }
    for r in &c.default_routines {
        table.entry(&r.name).or_insert(Member {
            vis: &ANY_VISIBILITY,
            arity: None,
            ret: Some(&r.ret_type),
            is_law: false,
        });
    }
    for l in &c.laws {
        table.entry(&l.name).or_insert(Member {
            vis: &ANY_VISIBILITY,
            arity: None,
            ret: None,
            is_law: true,
        });
    }
    for parent in &c.refines {
        add_concept_members(table, parent, concepts, seen);
    }
}

fn members_of<'a>(
    m: &'a ModuleDecl,
    concepts: &'a HashMap<String, ConceptDecl>,
) -> HashMap<&'a str, Member<'a>> {
    let mut table = HashMap::new();
    if let ReprKind::Product(fields) = &m.repr {
        for (f, t) in fields {
            add_member(
                &mut table,
                f,
                Member { vis: &m.repr_vis, arity: None, ret: Some(t), is_law: false },
            );
        }
    }
    for (vis, r) in m
        .create_routines
        .iter()
        .chain(&m.feature_routines)
        .chain(&m.specify_routines)
    {
        add_member(&mut table, &r.name, routine_member(vis, r));
    }
    for (vis, l) in &m.specify_laws {
        add_member(
            &mut table,
            &l.name,
            Member { vis, arity: Some(l.params.len()), ret: None, is_law: true },
        );
    }
    if let Some(sat) = &m.satisfies {
        let mut seen = HashSet::new();
        for c in &sat.concepts {
            add_concept_members(&mut table, c, concepts, &mut seen);
        }
    }
    table
}

impl<'a> Universe<'a> {
    fn new(
        user: &'a [SourceUnit],
        prelude: &'a [ModuleDecl],
        concepts: &'a HashMap<String, ConceptDecl>,
    ) -> Self {
        let mut u = Universe {
            modules: HashMap::new(),
            members: HashMap::new(),
            concepts,
            user_ctors: HashMap::new(),
            prelude_ctors: HashSet::new(),
        };
        for m in prelude {
            u.modules.insert(&m.name, m);
            u.members.insert(&m.name, members_of(m, concepts));
            u.prelude_ctors.extend(ctor_names(m));
        }
        for m in user.iter().flat_map(|unit| &unit.ast.modules) {
            u.modules.insert(&m.name, m);
            u.members.insert(&m.name, members_of(m, concepts));
            for c in ctor_names(m) {
                u.user_ctors.entry(c).or_default().push(&m.name);
            }
        }
        u
    }

    fn knows_type(&self, name: &str) -> bool {
        BUILTIN_TYPES.contains(&name)
            || self.modules.contains_key(name)
            || self.concepts.contains_key(name)
    }

    fn member(&self, module: &str, name: &str) -> Option<&Member<'a>> {
        self.members.get(module)?.get(name)
    }
}

struct Walker<'a, 'u> {
    universe: &'u Universe<'a>,
    module: &'a ModuleDecl,
    unit: &'a SourceUnit,
    routine: String,
    type_params: Vec<String>,
    /// Names in scope, innermost last, with their type when it is known.
    scope: Vec<(String, Option<String>)>,
    diags: &'u mut Vec<Diagnostic>,
    reported: &'u mut HashSet<(&'static str, String, String)>,
}

impl<'a> Walker<'a, '_> {
    fn report(&mut self, rule: &'static str, name: &str, message: String) {
        if !self.reported.insert((rule, name.to_string(), self.routine.clone())) {
            return;
        }
        let tag = rule.split_once(": ").map_or(rule, |(_, t)| t).to_string();
        let span = self.span_of(name);
        self.diags.push(
            Diagnostic::basic("error", rule, self.routine.clone(), tag, format!("{rule}: {message}"))
                .with_module(&self.module.name)
                .with_ai_context(span, Vec::new(), BTreeMap::new(), Vec::new()),
        );
    }

    /// Where `name` is first used in the current routine, not in the file.
    fn span_of(&self, name: &str) -> Span {
        let lines: Vec<&str> = self.unit.src.lines().collect();
        let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
        let header = lines.iter().position(|l| {
            l.trim_start()
                .strip_prefix(self.routine.as_str())
                .is_some_and(|rest| rest.starts_with(['(', ':', '[']))
        });
        for (idx, line) in lines.iter().enumerate().skip(header.unwrap_or(0)) {
            for (col, _) in line.match_indices(name) {
                let before = line[..col].chars().next_back();
                let after = line[col + name.len()..].chars().next();
                if !before.is_some_and(is_word) && !after.is_some_and(is_word) {
                    return Span {
                        file: self.unit.path.clone(),
                        line: idx + 1,
                        column: col + 1,
                        end_line: idx + 1,
                        end_column: col + name.len(),
                    };
                }
            }
        }
        locate_span(&self.unit.src, &self.unit.path, name, &self.routine)
    }

    fn in_scope(&self, name: &str) -> bool {
        self.scope.iter().any(|(n, _)| n == name)
    }

    fn check_type(&mut self, ty: &str) {
        for tok in type_tokens(ty) {
            if !all_caps(tok)
                || self.type_params.iter().any(|t| t == tok)
                || self.universe.knows_type(tok)
            {
                continue;
            }
            self.report(
                "E0107: unknown_type",
                tok,
                format!("type `{tok}` is not declared in this run, the prelude or `{}`", self.module.name),
            );
        }
    }

    fn check_ctor(&mut self, name: &str) {
        if ctor_names(self.module).contains(&name) {
            return;
        }
        let owners = self.universe.user_ctors.get(name).cloned().unwrap_or_default();
        if owners.len() > 1 {
            self.report(
                "E0112: ambiguous_constructor",
                name,
                format!(
                    "constructor `{name}` is declared in `{}`, and `{}` cannot tell them apart. Constructors resolve by name alone, so rename one of them",
                    owners.join("`, `"),
                    self.module.name
                ),
            );
        } else if owners.is_empty() && !self.universe.prelude_ctors.contains(name) {
            self.report(
                "E0109: unknown_constructor",
                name,
                format!("constructor `{name}` is not declared by any module in this run or the prelude"),
            );
        } else if let Some(owner) = self.ctor_owner(name, &owners) {
            // Building or matching one would open a representation, and with it the invariant.
            if self.is_hidden(&owner.repr_vis) {
                self.report(
                    "E0113: hidden_member",
                    name,
                    format!(
                        "the representation of `{}` is not exported to `{}`, so `{name}` cannot be built or matched here",
                        owner.name, self.module.name
                    ),
                );
            }
        }
    }

    fn ctor_owner(&self, name: &str, user_owners: &[&str]) -> Option<&'a ModuleDecl> {
        if let Some(owner) = user_owners.first() {
            return self.universe.modules.get(owner).copied();
        }
        self.universe.modules.values().copied().find(|m| ctor_names(m).contains(&name))
    }

    fn check_pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Variant(c, subs) => {
                self.check_ctor(c);
                for s in subs {
                    self.check_pattern(s);
                }
            }
            Pattern::Tuple(subs) => {
                for s in subs {
                    self.check_pattern(s);
                }
            }
            Pattern::Wildcard | Pattern::Bind(_) | Pattern::Lit(_) => {}
        }
    }

    /// The type of `e` when parameters, `let`s and routine results can tell it.
    fn type_of(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Var(v) if v == "Current" && !self.in_scope("Current") => {
                Some(module_self_type(self.module))
            }
            Expr::Var(v) => self.scope.iter().rev().find(|(n, _)| n == v).and_then(|(_, t)| t.clone()),
            Expr::Str(_) => Some("STRING".to_string()),
            Expr::Call(f, _) if !self.in_scope(f) => self.result_type(&self.module.name, f),
            Expr::MethodCall(recv, name, _) => {
                let owner = match &**recv {
                    Expr::Var(v) if self.is_static_receiver(v) => v.clone(),
                    other => type_head(&self.type_of(other)?).to_string(),
                };
                self.result_type(&owner, name)
            }
            _ => None,
        }
    }

    fn result_type(&self, owner: &str, name: &str) -> Option<String> {
        let ret = self.universe.member(owner, name)?.ret?;
        Some(if ret == "Current" { owner.to_string() } else { ret.to_string() })
    }

    fn is_static_receiver(&self, v: &str) -> bool {
        all_caps(v) && !self.in_scope(v)
    }

    /// `owner.name(args)`, where `owner` is a module, from the current module.
    fn check_member(&mut self, owner: &str, name: &str, argc: Option<usize>) {
        if !self.universe.modules.contains_key(owner) {
            return;
        }
        let Some(member) = self.universe.member(owner, name) else {
            self.report(
                "E0108: unknown_routine",
                name,
                format!("`{owner}` has no routine or field `{name}`"),
            );
            return;
        };
        let (vis, arity, is_law) = (member.vis, member.arity, member.is_law);
        if owner != self.module.name && !is_law && self.is_hidden(vis) {
            self.report(
                "E0113: hidden_member",
                name,
                format!(
                    "`{owner}.{name}` is not exported to `{}`. Use a routine `{owner}` exports",
                    self.module.name
                ),
            );
        }
        if let (Some(want), Some(got)) = (arity, argc) {
            if want != got {
                self.report(
                    "E0111: wrong_argument_count",
                    name,
                    format!("`{owner}.{name}` takes {want} argument(s), and this call has {got}"),
                );
            }
        }
    }

    fn is_hidden(&self, vis: &Visibility) -> bool {
        match vis {
            Visibility::Any => false,
            Visibility::None => true,
            // `specify {EQ}` names a concept, not a client.
            Visibility::Modules(names) => {
                !names.iter().any(|n| n == &self.module.name)
                    && !names.iter().any(|n| self.universe.concepts.contains_key(n))
            }
        }
    }

    fn block(&mut self, stmts: &[Stmt], body: &Expr) {
        let mark = self.scope.len();
        for st in stmts {
            self.stmt(st);
        }
        self.expr(body);
        self.scope.truncate(mark);
    }

    fn stmt(&mut self, st: &Stmt) {
        match st {
            Stmt::Let(v, e) => {
                self.expr(e);
                let ty = self.type_of(e);
                self.scope.push((v.clone(), ty));
            }
            Stmt::Check(_, e) | Stmt::Assume(_, e) => self.expr(e),
            Stmt::Reveal(target) => {
                let known = self.universe.members.values().any(|t| t.contains_key(target.as_str()));
                if !known {
                    self.report(
                        "E0108: unknown_routine",
                        target,
                        format!("`reveal {target}` names no routine of this run"),
                    );
                }
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Int(_) | Expr::Float(_) | Expr::Char(_) | Expr::Unit | Expr::Bool(_) | Expr::Str(_) => {}
            Expr::Var(v) => self.var(v),
            Expr::UnaryNot(x) | Expr::UnaryNeg(x) => self.expr(x),
            Expr::BinOp(_, l, r) => {
                self.expr(l);
                self.expr(r);
            }
            Expr::Call(f, args) => {
                for a in args {
                    self.expr(a);
                }
                self.call(f, args.len());
            }
            Expr::MethodCall(recv, name, args) => {
                for a in args {
                    self.expr(a);
                }
                self.method(recv, name, args.len());
            }
            Expr::If(c, ts, te, es, ee) => {
                self.expr(c);
                self.block(ts, te);
                self.block(es, ee);
            }
            Expr::Match(scrut, cases) => {
                self.expr(scrut);
                for c in cases {
                    self.check_pattern(&c.pattern);
                    let mark = self.scope.len();
                    self.scope.extend(c.bindings().into_iter().map(|b| (b, None)));
                    self.block(&c.stmts, &c.body);
                    self.scope.truncate(mark);
                }
            }
            Expr::RecordLit(fields) => {
                for (_, v) in fields {
                    self.expr(v);
                }
            }
            Expr::RecordUpdate(base, fields) => {
                self.expr(base);
                for (_, v) in fields {
                    self.expr(v);
                }
            }
            Expr::TupleLit(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            Expr::Forall(v, ty, body) | Expr::Exists(v, ty, body) => {
                self.check_type(ty);
                self.scope.push((v.clone(), Some(ty.clone())));
                self.expr(body);
                self.scope.pop();
            }
            Expr::WithIo(io_stmts, final_e) => {
                let mark = self.scope.len();
                for st in io_stmts {
                    match st {
                        IoStmt::Bind(v, x) => {
                            self.expr(x);
                            self.scope.push((v.clone(), None));
                        }
                        IoStmt::Action(x) => self.expr(x),
                    }
                }
                self.expr(final_e);
                self.scope.truncate(mark);
            }
            Expr::Lambda(params, ret, stmts, body) => {
                self.check_type(ret);
                let mark = self.scope.len();
                for (p, t) in params {
                    self.check_type(t);
                    self.scope.push((p.clone(), Some(t.clone())));
                }
                self.block(stmts, body);
                self.scope.truncate(mark);
            }
        }
    }

    fn var(&mut self, v: &str) {
        if self.in_scope(v) || v == "Current" || v == "Result" {
            return;
        }
        if all_caps(v) {
            if !self.universe.knows_type(v) && !self.type_params.iter().any(|t| t == v) {
                self.report(
                    "E0106: unknown_module",
                    v,
                    format!("`{v}` is not a module of this run or the prelude"),
                );
            }
        } else if starts_upper(v) {
            self.check_ctor(v);
        } else if self.universe.member(&self.module.name, v).is_none() {
            self.report(
                "E0110: unknown_variable",
                v,
                format!("`{v}` is not a parameter, local, field or routine visible here"),
            );
        }
    }

    fn call(&mut self, f: &str, argc: usize) {
        if self.in_scope(f) {
            return;
        }
        if starts_upper(f) {
            self.check_ctor(f);
        } else if self.universe.member(&self.module.name, f).is_none() {
            self.report(
                "E0108: unknown_routine",
                f,
                format!("`{}` has no routine `{f}`", self.module.name),
            );
        } else {
            self.check_member(&self.module.name.clone(), f, Some(argc));
        }
    }

    fn method(&mut self, recv: &Expr, name: &str, argc: usize) {
        if let Expr::Var(v) = recv {
            if self.is_static_receiver(v) {
                if self.universe.modules.contains_key(v.as_str()) {
                    self.check_member(v, name, Some(argc));
                } else if !self.universe.knows_type(v) && !self.type_params.iter().any(|t| t == v) {
                    self.report(
                        "E0106: unknown_module",
                        v,
                        format!("`{v}` is not a module of this run or the prelude"),
                    );
                }
                return;
            }
        }
        self.expr(recv);
        if let Some(t) = self.type_of(recv) {
            let owner = type_head(&t).to_string();
            let argc = self.universe.member(&owner, name).and_then(|m| m.arity).map(|_| argc);
            self.check_member(&owner, name, argc);
        }
    }

    fn routine(&mut self, r: &RoutineDecl) {
        self.routine = r.name.clone();
        self.type_params = self.module.type_params.iter().chain(&r.type_params).cloned().collect();
        self.scope = r.params.iter().map(|(p, t)| (p.clone(), Some(t.clone()))).collect();
        self.scope.push(("Result".to_string(), Some(r.ret_type.clone())));
        for (_, t) in &r.params {
            self.check_type(t);
        }
        self.check_type(&r.ret_type);
        for (_, e) in &r.requires {
            self.expr(e);
        }
        for e in &r.variant {
            self.expr(e);
        }
        let mark = self.scope.len();
        for st in &r.stmts {
            self.stmt(st);
        }
        self.expr(&r.body);
        self.scope.truncate(mark);
        for (_, e) in &r.ensures {
            self.expr(e);
        }
    }

    fn law(&mut self, l: &LawDecl) {
        self.routine = l.name.clone();
        self.type_params = self.module.type_params.iter().chain(&l.type_params).cloned().collect();
        self.scope = l.params.iter().map(|(p, t)| (p.clone(), Some(t.clone()))).collect();
        for (_, t) in &l.params {
            self.check_type(t);
        }
        for (_, e) in l.requires.iter().chain(&l.ensures) {
            self.expr(e);
        }
        for e in l.triggers.iter().chain(&l.variant) {
            self.expr(e);
        }
        for st in &l.stmts {
            self.stmt(st);
        }
        if let Some(b) = &l.body {
            self.expr(b);
        }
    }

    fn module_decl(&mut self) {
        let m = self.module;
        self.routine = m.name.clone();
        self.type_params = m.type_params.clone();
        self.scope = Vec::new();
        match &m.repr {
            ReprKind::Product(fields) => {
                for (_, t) in fields {
                    self.check_type(t);
                }
            }
            ReprKind::Sum(variants) => {
                for (_, t) in variants.iter().flat_map(|v| &v.fields) {
                    self.check_type(t);
                }
            }
        }
        for (_, e) in &m.invariants {
            self.expr(e);
        }
        for (_, r) in m.create_routines.iter().chain(&m.feature_routines).chain(&m.specify_routines) {
            self.routine(r);
        }
        for (_, l) in &m.specify_laws {
            self.law(l);
        }
    }
}

/// Resolves every name of every user module of the run. Bootstrap runs of a partial library
/// (`--no-prelude` on a file) have no complete universe, so they are left to the verifier.
pub(crate) fn check_names(
    units: &[SourceUnit],
    prelude_mode: bool,
    concepts: &HashMap<String, ConceptDecl>,
) -> Vec<Diagnostic> {
    let has_core = units.iter().flat_map(|u| &u.ast.modules).any(|m| m.name == "OPTION");
    if !prelude_mode && !has_core {
        return Vec::new();
    }
    let prelude: &[ModuleDecl] = if prelude_mode { &standard_prelude().modules } else { &[] };
    let universe = Universe::new(units, prelude, concepts);
    let mut diags = Vec::new();
    let mut reported = HashSet::new();
    for unit in units {
        for module in &unit.ast.modules {
            Walker {
                universe: &universe,
                module,
                unit,
                routine: String::new(),
                type_params: Vec::new(),
                scope: Vec::new(),
                diags: &mut diags,
                reported: &mut reported,
            }
            .module_decl();
        }
    }
    diags
}
