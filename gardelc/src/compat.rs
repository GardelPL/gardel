//! Contract compatibility between two versions of a package (Task E.7).
//!
//! A client of a package was proved against its old interface, so a new version keeps that
//! proof only if every call the old contract allowed is still allowed, and everything the old
//! contract promised is still promised:
//!
//! * for a routine or law, `old require` implies each new `require`;
//! * and `old require` with the new `ensure`s implies each old `ensure`.
//!
//! A clause found word for word on the other side needs no solver, which is what makes a
//! body-only change compatible at no cost. Every other implication goes to the solver, and
//! only a positive answer counts: a timeout, a failed run or a file Boogie refuses all leave
//! the change reported as breaking.
//!
//! The solver sees no contract or law of a user module, so every routine in a clause is an
//! unknown function. What it proves therefore holds whatever those routines mean, and a change
//! in what one of them means is reported at that routine. This also means one false contract
//! cannot make everything compatible. Being this careful costs some precision: an implication
//! that needs a law of the package is reported as breaking.

use crate::{
    check_and_expand_concepts, collect_module_own_symbols_in, collect_module_strings,
    boogie_seed_args, contract_only_module, current_z3_rlimit, current_z3_seed, emit_boogie_expr_with_strings,
    emit_boogie_parts_with_options, emit_boogie_string_literal_decls, emit_contract_interface,
    emit_linked_background_theory, emit_prelude_background_theory, is_linked_user_module,
    locate_span, manifest, map_boogie_type, module_assumes, module_self_type, output_within,
    resolve, resolve_package_inputs, set_boogie_type_env, clear_boogie_type_env, solver_backstop_secs,
    standard_prelude, subst_expr_vars, subst_type_idents, tokenize, type_is_self,
    with_fresh_boogie_numbering, with_link_table, with_prelude_injection, with_z3_rlimit, with_z3_seed, BoogieEmitScope,
    BoogiePreludeCtx, ConceptDecl, Diagnostic, Expr, FileAst, LawDecl, LinkTable, ModuleDecl,
    ModuleKind, Parser, ReprKind, RoutineDecl, SourceUnit, Visibility, BOOGIE_PRELUDE_CTX,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::process::Command;
use std::rc::Rc;

pub(crate) const BREAKING_TAG: &str = "breaking_change";
const NEW_NAME_TAG: &str = "new_public_name";
const RESTS_ON_ASSUME_TAG: &str = "rests_on_assume";
const DEPENDENCY_MOVED_TAG: &str = "dependency_moved";
const RULE: &str = "Contract Compatibility";

/// What the comparison found.
pub(crate) struct Report {
    /// One error for each breaking change, with the index of its source unit.
    pub breaking: Vec<(usize, Diagnostic)>,
    /// New names a client may have to qualify, which one word fixes.
    pub warnings: Vec<(usize, Diagnostic)>,
    /// What the new version adds, which breaks no client.
    pub added: Vec<String>,
    /// Implications the solver proved, so changed clauses that break nothing.
    pub proved: usize,
}

/// Another package of a build.
pub(crate) struct Dependency {
    pub name: String,
    pub version: String,
    /// Its text, which tells two copies of one `version` apart. Sorted, as the name of a file
    /// means nothing to a client, and with one line ending, as a checkout can change those.
    sources: Vec<String>,
}

impl Dependency {
    pub(crate) fn of(name: &str, version: &str, units: &[SourceUnit]) -> Self {
        let mut sources: Vec<String> = units.iter().map(|u| u.src.replace("\r\n", "\n")).collect();
        sources.sort();
        Self { name: name.to_string(), version: version.to_string(), sources }
    }
}

/// The public modules of an old version, after the same front end as a build.
struct OldVersion {
    package: String,
    modules: Vec<ModuleDecl>,
    concepts: Vec<ConceptDecl>,
    assumes: HashSet<(String, String)>,
    /// Every other package of the build.
    dependencies: Vec<Dependency>,
}

/// Reads the package in `dir` as far as its interface: it is not verified, as what matters
/// here is what its clients were told.
fn load_old_version(dir: &std::path::Path, prelude_mode: bool) -> Result<OldVersion, String> {
    let packages = resolve_package_inputs(dir).map_err(|e| e.message)?;
    let mut units = Vec::new();
    let mut list = Vec::new();
    let mut start = 0;
    let last = packages.len().saturating_sub(1);
    let mut package = String::new();
    for (i, (manifest, files)) in packages.iter().enumerate() {
        for path in files {
            let src = std::fs::read_to_string(path).map_err(|e| format!("`{path}`: {e}"))?;
            let tokens = tokenize(&src).map_err(|e| format!("`{path}`: {e}"))?;
            let ast: FileAst =
                Parser::new(tokens).parse_file().map_err(|e| format!("`{path}`: {e}"))?;
            units.push(SourceUnit { path: path.clone(), src, ast });
        }
        list.push(resolve::PackageUnits {
            name: manifest.project.name.clone(),
            is_root: i == last,
            deps: manifest
                .dependencies
                .iter()
                .map(|d: &manifest::Dependency| (d.name.clone(), d.alias.clone()))
                .collect(),
            units: start..start + files.len(),
        });
        start += files.len();
        package = manifest.project.name.clone();
    }
    let (of_package, errors) = resolve::resolve_packages(&mut units, &list, prelude_mode);
    if let Some(e) = errors.first() {
        return Err(e.message.clone());
    }
    let mut concepts: HashMap<String, ConceptDecl> = HashMap::new();
    if prelude_mode {
        for c in &standard_prelude().concepts {
            concepts.insert(c.name.clone(), c.clone());
        }
    }
    for c in units.iter().flat_map(|u| &u.ast.concepts) {
        concepts.insert(c.name.clone(), c.clone());
    }
    let mut modules = Vec::new();
    for u in &mut units {
        for m in &mut u.ast.modules {
            drop(check_and_expand_concepts(m, &concepts));
            if !of_package.contains_key(&m.name) && !m.kind.is_entry_point() {
                modules.push(m.clone());
            }
        }
    }
    let root = list.last().map_or(0..0, |p| p.units.clone());
    let concepts = units[root].iter().flat_map(|u| u.ast.concepts.iter().cloned()).collect();
    let assumes = assumes_of(&units).into_iter().collect();
    let dependencies = packages
        .iter()
        .zip(&list)
        .take(last)
        .map(|((m, _), p)| Dependency::of(&m.project.name, &m.project.version, &units[p.units.clone()]))
        .collect();
    Ok(OldVersion { package, modules, concepts, assumes, dependencies })
}

/// Every `assume` of a build as (module, `routine tag`), bodies and dependencies included, as
/// `--strict` refuses them all (decision 6).
fn assumes_of(units: &[SourceUnit]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for u in units {
        for m in &u.ast.modules {
            for a in module_assumes(m, &u.src, &u.path) {
                found.push((m.name.clone(), a));
            }
        }
    }
    found
}

/// Clauses as a caller reads them. A parameter is known by its position, so its name means
/// nothing, and a name in neither version avoids capture.
fn by_position(clauses: &[(String, Expr)], params: &[(String, String)]) -> Vec<Expr> {
    let args: Vec<(String, Expr)> = params
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (p.clone(), Expr::Var(format!("gdl_arg{i}"))))
        .collect();
    clauses.iter().map(|(_, e)| subst_expr_vars(e, &args)).collect()
}

fn type_list(params: &[(String, String)]) -> Vec<&str> {
    params.iter().map(|(_, t)| t.as_str()).collect()
}

/// A concept has two sides. A client that satisfies it must supply and prove everything in it,
/// and a client generic over it relies on everything in it, so no change is safe in both
/// directions. Hence an exact comparison, and no solver. A default body is not compared, as
/// nothing can rely on it.
fn compare_concept(old: &ConceptDecl, new: &ConceptDecl) -> Vec<String> {
    let mut why = Vec::new();
    if old.type_params != new.type_params {
        why.push("its type parameters changed".to_string());
    }
    if old.refines != new.refines {
        why.push("what it refines changed".to_string());
    }
    let signature = |c: &ConceptDecl, name: &str| {
        c.signatures
            .iter()
            .find(|s| s.name == name)
            .map(|s| format!("{:?} {}", type_list(&s.params), s.ret_type))
    };
    for s in &old.signatures {
        match signature(new, &s.name) {
            None => why.push(format!("`{}` is removed", s.name)),
            Some(text) if Some(&text) != signature(old, &s.name).as_ref() => {
                why.push(format!("the signature of `{}` changed", s.name));
            }
            Some(_) => {}
        }
    }
    for s in new.signatures.iter().filter(|s| signature(old, &s.name).is_none()) {
        why.push(format!("`{}` is new, and a client that satisfies it must supply it", s.name));
    }
    let contract = |r: &RoutineDecl| {
        format!(
            "{:?} {:?} {} {:?} {:?}",
            r.type_params,
            type_list(&r.params),
            r.ret_type,
            by_position(&r.requires, &r.params),
            by_position(&r.ensures, &r.params)
        )
    };
    for r in &old.default_routines {
        match new.default_routines.iter().find(|n| n.name == r.name) {
            None => why.push(format!("the default routine `{}` is removed", r.name)),
            Some(n) if contract(n) != contract(r) => {
                why.push(format!("the default routine `{}` changed", r.name));
            }
            Some(_) => {}
        }
    }
    let law = |l: &LawDecl| {
        format!(
            "{:?} {:?} {:?} {:?}",
            l.type_params,
            type_list(&l.params),
            by_position(&l.requires, &l.params),
            by_position(&l.ensures, &l.params)
        )
    };
    for l in &old.laws {
        match new.laws.iter().find(|n| n.name == l.name) {
            None => why.push(format!("law `{}` is removed", l.name)),
            Some(n) if law(n) != law(l) => why.push(format!("law `{}` changed", l.name)),
            Some(_) => {}
        }
    }
    for l in new.laws.iter().filter(|n| !old.laws.iter().any(|l| l.name == n.name)) {
        why.push(format!("law `{}` is new, and a client that satisfies it must prove it", l.name));
    }
    why
}

/// One implication for the solver.
struct Claim {
    module: String,
    item: String,
    /// What it means if this is not proved.
    meaning: String,
    type_params: Vec<String>,
    has_current: bool,
    params: Vec<(String, String)>,
    ret: Option<String>,
    hypotheses: Vec<Expr>,
    goal: Expr,
}

/// A contract, of a routine or a law, in the terms both sides share.
struct Contract<'a> {
    item: String,
    type_params: &'a [String],
    has_current: bool,
    params: &'a [(String, String)],
    ret: Option<&'a str>,
    requires: &'a [(String, Expr)],
    ensures: &'a [(String, Expr)],
}

struct Comparison<'a> {
    old: &'a ModuleDecl,
    new: &'a ModuleDecl,
    /// Concept -> the concepts it refines, directly, for the new version.
    refines: &'a HashMap<String, Vec<String>>,
    /// Public names of the old version that the new one lacks or changed.
    gone: HashSet<String>,
    claims: Vec<Claim>,
    breaking: Vec<(String, String)>,
    added: Vec<String>,
}

fn public<'a, T>(list: &'a [(Visibility, T)], name: impl Fn(&T) -> &str) -> BTreeMap<&'a str, &'a T> {
    list.iter().filter(|(v, _)| *v == Visibility::Any).map(|(_, x)| (name(x), x)).collect()
}

fn normal_type(ty: &str, m: &ModuleDecl) -> String {
    let map = HashMap::from([("Current".to_string(), module_self_type(m))]);
    subst_type_idents(ty, &map)
}

fn same_signature(old: &RoutineDecl, om: &ModuleDecl, new: &RoutineDecl, nm: &ModuleDecl) -> bool {
    let types = |r: &RoutineDecl, m: &ModuleDecl| -> Vec<String> {
        r.params.iter().map(|(_, t)| normal_type(t, m)).collect()
    };
    old.type_params == new.type_params
        && types(old, om) == types(new, nm)
        && normal_type(&old.ret_type, om) == normal_type(&new.ret_type, nm)
}

fn same_law_signature(old: &LawDecl, om: &ModuleDecl, new: &LawDecl, nm: &ModuleDecl) -> bool {
    let types = |l: &LawDecl, m: &ModuleDecl| -> Vec<String> {
        l.params.iter().map(|(_, t)| normal_type(t, m)).collect()
    };
    old.type_params == new.type_params && types(old, om) == types(new, nm)
}

fn describe_repr(repr: &ReprKind, m: &ModuleDecl) -> Vec<String> {
    let fields = |fs: &[(String, String)]| -> String {
        fs.iter().map(|(n, t)| format!("{n}: {}", normal_type(t, m))).collect::<Vec<_>>().join(", ")
    };
    match repr {
        ReprKind::Sum(vs) => vs.iter().map(|v| format!("{}({})", v.name, fields(&v.fields))).collect(),
        ReprKind::Product(fs) => vec![format!("({})", fields(fs))],
    }
}

impl Comparison<'_> {
    fn is_breaking(&mut self, item: &str, why: String) {
        self.breaking.push((item.to_string(), why));
    }

    /// Whether `e`, a clause of the old version, can be stated in the new one.
    fn mentions_gone(&self, e: &Expr, c: &Contract<'_>) -> Option<String> {
        let mut bound = HashSet::from(["Current".to_string(), "Result".to_string()]);
        let mut not_self = HashSet::new();
        for (n, t) in c.params {
            bound.insert(n.clone());
            if !type_is_self(t, &self.new.name) {
                not_self.insert(n.clone());
            }
        }
        if c.ret.is_some_and(|r| !type_is_self(r, &self.new.name)) {
            not_self.insert("Result".to_string());
        }
        let mut found = HashSet::new();
        collect_module_own_symbols_in(e, &self.new.name, &bound, &not_self, &mut found);
        let mut names: Vec<String> = found.into_iter().filter(|s| self.gone.contains(s)).collect();
        names.sort();
        names.into_iter().next()
    }

    /// The two implications of the header. Both sides are first stated over the same
    /// parameters, as a caller passes them by position. A name that is in neither version
    /// avoids capture: renaming `x` to `y` in `forall y: ...` would change the clause.
    fn contracts(&mut self, old: Contract<'_>, new: Contract<'_>) {
        let args: Vec<(String, String)> = new
            .params
            .iter()
            .enumerate()
            .map(|(i, (_, t))| (format!("gdl_arg{i}"), t.clone()))
            .collect();
        let restate = |clauses: &[(String, Expr)], params: &[(String, String)]| {
            let to_args: Vec<(String, Expr)> = params
                .iter()
                .zip(&args)
                .map(|((p, _), (a, _))| (p.clone(), Expr::Var(a.clone())))
                .collect();
            clauses
                .iter()
                .map(|(t, e)| (t.clone(), subst_expr_vars(e, &to_args)))
                .collect::<Vec<_>>()
        };
        let old_requires = restate(old.requires, old.params);
        let old_ensures = restate(old.ensures, old.params);
        let new_requires = restate(new.requires, new.params);
        let new_ensures = restate(new.ensures, new.params);
        let new = Contract { params: &args, requires: &new_requires, ensures: &new_ensures, ..new };
        // Leaving out a hypothesis can only lose a proof.
        let given: Vec<Expr> = old_requires
            .iter()
            .filter(|(_, e)| self.mentions_gone(e, &new).is_none())
            .map(|(_, e)| e.clone())
            .collect();
        let claim = |hypotheses: Vec<Expr>, goal: &Expr, meaning: String| Claim {
            module: self.new.name.clone(),
            item: new.item.clone(),
            meaning,
            type_params: new.type_params.to_vec(),
            has_current: new.has_current,
            params: new.params.to_vec(),
            ret: new.ret.map(str::to_string),
            hypotheses,
            goal: goal.clone(),
        };
        let mut claims = Vec::new();
        for (tag, e) in new.requires {
            if !old_requires.iter().any(|(_, o)| o == e) {
                claims.push(claim(
                    given.clone(),
                    e,
                    format!("`require {tag}` is not implied by the old `require`, so a call that was valid may now fail"),
                ));
            }
        }
        let mut gone_ensures = Vec::new();
        for (tag, e) in &old_ensures {
            if new.ensures.iter().any(|(_, n)| n == e) {
                continue;
            }
            if let Some(name) = self.mentions_gone(e, &new) {
                gone_ensures.push(format!("`ensure {tag}` of the old version uses `{name}`, which the new version does not offer"));
                continue;
            }
            let mut hypotheses = given.clone();
            hypotheses.extend(new.ensures.iter().map(|(_, n)| n.clone()));
            claims.push(claim(
                hypotheses,
                e,
                format!("`ensure {tag}` of the old version no longer follows, so a client may have relied on a promise that is gone"),
            ));
        }
        self.claims.extend(claims);
        for why in gone_ensures {
            self.is_breaking(&new.item, why);
        }
    }

    fn routines(
        &mut self,
        section: &str,
        has_current: bool,
        old: &[(Visibility, RoutineDecl)],
        new: &[(Visibility, RoutineDecl)],
    ) {
        let (old_map, new_map) = (public(old, |r| &r.name), public(new, |r| &r.name));
        for (name, o) in &old_map {
            let item = format!("{}.{name}", self.new.name);
            let Some(n) = new_map.get(name) else {
                self.is_breaking(&item, format!("public `{section}` routine `{name}` is removed or no longer public"));
                continue;
            };
            if !same_signature(o, self.old, n, self.new) {
                self.is_breaking(&item, format!("the signature of `{name}` changed"));
                continue;
            }
            self.contracts(
                Contract {
                    item: item.clone(),
                    type_params: &o.type_params,
                    has_current,
                    params: &o.params,
                    ret: Some(&o.ret_type),
                    requires: &o.requires,
                    ensures: &o.ensures,
                },
                Contract {
                    item,
                    type_params: &n.type_params,
                    has_current,
                    params: &n.params,
                    ret: Some(&n.ret_type),
                    requires: &n.requires,
                    ensures: &n.ensures,
                },
            );
        }
        for name in new_map.keys().filter(|n| !old_map.contains_key(*n)) {
            self.added.push(format!("{}.{name}", self.new.name));
        }
    }

    fn laws(&mut self, old: &[(Visibility, LawDecl)], new: &[(Visibility, LawDecl)]) {
        let (old_map, new_map) = (public(old, |l| &l.name), public(new, |l| &l.name));
        for (name, o) in &old_map {
            let item = format!("{}.{name}", self.new.name);
            let Some(n) = new_map.get(name) else {
                self.is_breaking(&item, format!("public law `{name}` is removed or no longer public"));
                continue;
            };
            if !same_law_signature(o, self.old, n, self.new) {
                self.is_breaking(&item, format!("the parameters of law `{name}` changed"));
                continue;
            }
            self.contracts(
                Contract {
                    item: item.clone(),
                    type_params: &o.type_params,
                    has_current: false,
                    params: &o.params,
                    ret: None,
                    requires: &o.requires,
                    ensures: &o.ensures,
                },
                Contract {
                    item,
                    type_params: &n.type_params,
                    has_current: false,
                    params: &n.params,
                    ret: None,
                    requires: &n.requires,
                    ensures: &n.ensures,
                },
            );
        }
        for name in new_map.keys().filter(|n| !old_map.contains_key(*n)) {
            self.added.push(format!("{}.{name}", self.new.name));
        }
    }

    fn shape(&mut self) {
        let (old, new) = (self.old, self.new);
        let item = new.name.clone();
        if old.kind != new.kind {
            self.is_breaking(&item, "the kind of the module changed".to_string());
        }
        if old.type_params != new.type_params {
            self.is_breaking(&item, "the type parameters changed".to_string());
        }
        let bounds = |m: &ModuleDecl| -> BTreeMap<String, String> { m.type_bounds.clone().into_iter().collect() };
        if bounds(old) != bounds(new) {
            self.is_breaking(&item, "the bounds of the type parameters changed".to_string());
        }
        let wheres = |m: &ModuleDecl| {
            m.satisfies.as_ref().map(|s| s.where_constraints.clone()).unwrap_or_default()
        };
        let concepts = |m: &ModuleDecl| {
            m.satisfies.as_ref().map(|s| s.concepts.clone()).unwrap_or_default()
        };
        // A concept the new version lists, or one those refine: `satisfies ORD` keeps `EQ`.
        let mut kept = concepts(new);
        let mut i = 0;
        while i < kept.len() {
            for r in self.refines.get(&kept[i]).into_iter().flatten() {
                if !kept.contains(r) {
                    kept.push(r.clone());
                }
            }
            i += 1;
        }
        for c in concepts(old).iter().filter(|c| !kept.contains(c)) {
            self.is_breaking(&item, format!("it no longer satisfies `{c}`"));
        }
        if wheres(old) != wheres(new) {
            self.is_breaking(&item, "the `where` clause of `satisfies` changed".to_string());
        }
        // A client can build and match an exported representation, so it is part of the
        // interface exactly: even a new constructor breaks a `match` that was complete.
        if old.repr_vis == Visibility::Any
            && (new.repr_vis != Visibility::Any
                || describe_repr(&old.repr, old) != describe_repr(&new.repr, new))
        {
            self.is_breaking(&item, "the public `representation` changed".to_string());
        }
    }

    fn invariants(&mut self) {
        let hypotheses: Vec<Expr> = self.new.invariants.iter().map(|(_, e)| e.clone()).collect();
        let (old, new) = (self.old, self.new);
        for (tag, e) in &old.invariants {
            if hypotheses.contains(e) {
                continue;
            }
            let item = format!("{}.invariant {tag}", new.name);
            let contract = Contract {
                item: item.clone(),
                type_params: &[],
                has_current: true,
                params: &[],
                ret: None,
                requires: &[],
                ensures: &[],
            };
            if let Some(name) = self.mentions_gone(e, &contract) {
                self.is_breaking(&item, format!("it uses `{name}`, which the new version does not offer"));
                continue;
            }
            self.claims.push(Claim {
                module: new.name.clone(),
                item,
                meaning: format!("`invariant {tag}` of the old version no longer follows"),
                type_params: Vec::new(),
                has_current: true,
                params: Vec::new(),
                ret: None,
                hypotheses: hypotheses.clone(),
                goal: e.clone(),
            });
        }
    }
}

fn public_names(m: &ModuleDecl) -> BTreeMap<String, String> {
    let self_type = |t: &str| normal_type(t, m);
    let mut names = BTreeMap::new();
    for (vis, r) in m.create_routines.iter().chain(&m.feature_routines).chain(&m.specify_routines) {
        if *vis == Visibility::Any {
            let params: Vec<String> = r.params.iter().map(|(_, t)| self_type(t)).collect();
            names.insert(r.name.clone(), format!("{:?}{params:?}{}", r.type_params, self_type(&r.ret_type)));
        }
    }
    names
}

/// `m` with nothing a solver could use: its routines become unknown functions.
fn without_contracts(m: &ModuleDecl) -> ModuleDecl {
    let mut bare = contract_only_module(m);
    for (_, r) in bare
        .create_routines
        .iter_mut()
        .chain(bare.feature_routines.iter_mut())
        .chain(bare.specify_routines.iter_mut())
    {
        r.requires.clear();
        r.ensures.clear();
    }
    bare.specify_laws.clear();
    bare.invariants.clear();
    bare
}

fn range_of(name: &str, ty: &str) -> Option<String> {
    match ty {
        "NAT" | "U64" => Some(format!("in_u64({name})")),
        "INT" | "I64" => Some(format!("in_i64({name})")),
        "CHAR" => Some(format!("in_char({name})")),
        _ => None,
    }
}

/// The Boogie program asking every claim of `bare`, each as `Compare_<index in claims>`.
fn query(bare: &ModuleDecl, claims: &[(usize, &Claim)], fuel: usize) -> String {
    // What the claims mention decides which theories are needed, and the bare module no
    // longer mentions it. This module exists to be read, never to be emitted.
    let mut carrier = bare.clone();
    let clauses: Vec<(String, Expr)> = claims
        .iter()
        .flat_map(|(_, c)| c.hypotheses.iter().chain([&c.goal]))
        .map(|e| (String::new(), e.clone()))
        .collect();
    let holder = RoutineDecl {
        name: "compare_clauses".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        ret_type: "BOOL".to_string(),
        requires: clauses,
        variant: Vec::new(),
        stmts: Vec::new(),
        body: Expr::Int(0),
        ensures: Vec::new(),
    };
    carrier.specify_routines.push((Visibility::Any, holder));

    with_fresh_boogie_numbering(|| {
        let ctx = Rc::new(BoogiePreludeCtx::for_module(&carrier));
        let prev = (!ctx.modules.is_empty())
            .then(|| BOOGIE_PRELUDE_CTX.with(|c| c.replace(Some(ctx.clone()))));
        let mut injected = String::new();
        for p in &ctx.modules {
            if is_linked_user_module(&p.name) {
                injected.push_str(&emit_linked_background_theory(p, fuel, BoogieEmitScope::ContractOnly));
            } else {
                injected.push_str(&emit_prelude_background_theory(p, fuel, BoogieEmitScope::ContractOnly));
            }
        }
        let parts = emit_boogie_parts_with_options(bare, fuel, BoogieEmitScope::ContractOnly);
        let strings = collect_module_strings(&carrier);
        let mut s = parts.preamble;
        s.push_str(&injected);
        s.push_str(&parts.theory);
        s.push_str(&parts.client_axioms);
        s.push_str(&emit_contract_interface(bare).0);
        if !ctx.modules.iter().any(|p| p.name == "OPTION") {
            s.push_str(&parts.option_shim);
        }
        s.push_str(&emit_boogie_string_literal_decls(&strings, false));
        for (index, claim) in claims {
            s.push_str(&procedure(*index, claim, bare, &strings));
        }
        if let Some(prev) = prev {
            BOOGIE_PRELUDE_CTX.with(|c| *c.borrow_mut() = prev);
        }
        s
    })
}

fn procedure(index: usize, c: &Claim, m: &ModuleDecl, strings: &[String]) -> String {
    let mut tps = m.type_params.clone();
    tps.extend(c.type_params.iter().filter(|t| !m.type_params.contains(t)).cloned());
    let quant = if tps.is_empty() { String::new() } else { format!("<{}>", tps.join(", ")) };
    let mut params = Vec::new();
    if c.has_current {
        params.push(format!("Current: {}", map_boogie_type("Current", m)));
    }
    let mut ranges = Vec::new();
    for (n, t) in &c.params {
        params.push(format!("{n}: {}", map_boogie_type(t, m)));
        ranges.extend(range_of(n, t));
    }
    if let Some(ret) = &c.ret {
        params.push(format!("Result: {}", map_boogie_type(ret, m)));
        ranges.extend(range_of("Result", ret));
    }
    set_boogie_type_env(m, &c.params, c.ret.as_deref());
    let fields = HashSet::new();
    let revealed = HashSet::new();
    let emit = |e: &Expr| {
        emit_boogie_expr_with_strings(e, m, &fields, &revealed, "DefaultFuel", strings)
    };
    let mut s = format!("procedure Compare_{index}{quant}({})\n", params.join(", "));
    for r in &ranges {
        s.push_str(&format!("  requires {r};\n"));
    }
    for h in &c.hypotheses {
        s.push_str(&format!("  requires {};\n", emit(h)));
    }
    s.push_str(&format!("{{\n  assert {};\n}}\n\n", emit(&c.goal)));
    clear_boogie_type_env();
    s
}

/// The indices of the claims Boogie reports as proved. Only a procedure it names as verified
/// counts, so a run that failed in any way proves nothing.
fn proved_in(output: &str) -> HashSet<usize> {
    let mut proved = HashSet::new();
    let mut current = None;
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("Verifying Compare_") {
            current = rest.split_whitespace().next().and_then(|n| n.parse::<usize>().ok());
        } else if line.trim_end().ends_with("]  verified") {
            proved.extend(current.take());
        } else if line.trim_end().ends_with("]  error") || line.contains("]  timed out") {
            current = None;
        }
    }
    proved
}

/// Why Boogie gave no answer.
enum NoAnswer {
    /// It could not read the query, which one unreadable clause is enough for.
    Refused(String),
    Failed(String),
}

impl NoAnswer {
    fn reason(self) -> String {
        match self {
            Self::Refused(why) | Self::Failed(why) => why,
        }
    }
}

/// The limits and the seed are the ones this run was given, as for any other query.
fn solver_args(z3: &str, backstop: u32) -> Vec<String> {
    let mut args = vec![format!("/proverOpt:PROVER_PATH={z3}")];
    if let Some(seed) = current_z3_seed() {
        args.extend(boogie_seed_args(seed));
    }
    args.push(crate::BOOGIE_TYPE_ENCODING.to_string());
    args.push(format!("/rlimit:{}", current_z3_rlimit()));
    args.push(format!("/timeLimit:{backstop}"));
    args.push("/trace".to_string());
    args.push("/errorTrace:0".to_string());
    args
}

fn ask_solver(bpl: &str, module: &str, solvers: (&str, &str)) -> Result<HashSet<usize>, NoAnswer> {
    let path = std::env::temp_dir().join(format!("gardel_compare_{module}_{}.bpl", std::process::id()));
    std::fs::write(&path, bpl).map_err(|e| NoAnswer::Failed(e.to_string()))?;
    let backstop = solver_backstop_secs();
    let mut cmd = Command::new(solvers.0);
    cmd.args(solver_args(solvers.1, backstop)).arg(&path);
    let output = output_within(&mut cmd, std::time::Duration::from_secs(backstop.into()));
    if std::env::var_os("GARDEL_KEEP_BPL").is_none() {
        drop(std::fs::remove_file(&path));
    }
    match output {
        Ok(Some(o)) => {
            let text = String::from_utf8_lossy(&o.stdout);
            if text.contains("Boogie program verifier finished") {
                Ok(proved_in(&text))
            } else {
                let first = text.lines().find(|l| l.contains("rror")).unwrap_or("no result");
                Err(NoAnswer::Refused(format!("Boogie refused the comparison: {}", first.trim())))
            }
        }
        Ok(None) => Err(NoAnswer::Failed(format!("the solver was stopped after {backstop} s"))),
        Err(e) => Err(NoAnswer::Failed(format!("cannot run Boogie: {e}"))),
    }
}

pub(crate) struct Run<'a> {
    pub package: &'a str,
    pub units: &'a [SourceUnit],
    /// The units of the package being compared, as a build lists its dependencies first.
    pub root_units: std::ops::Range<usize>,
    /// Every other package of the build.
    pub dependencies: &'a [Dependency],
    /// The package of each dependency module, as the build found them.
    pub packages: &'a HashMap<String, String>,
    pub prelude_mode: bool,
    pub fuel: usize,
    /// Boogie and Z3, if this machine has them.
    pub solvers: Option<(String, String)>,
    pub z3_seed: Option<u32>,
    pub z3_rlimit: Option<u64>,
}

/// Compares the package of `run` with the older version in `old_dir`.
pub(crate) fn compare(old_dir: &std::path::Path, run: &Run<'_>) -> Result<Report, String> {
    let old = load_old_version(old_dir, run.prelude_mode)?;
    let mut report = Report { breaking: Vec::new(), warnings: Vec::new(), added: Vec::new(), proved: 0 };
    let unit_of = |module: &str| -> usize {
        run.units
            .iter()
            .position(|u| {
                u.ast.modules.iter().any(|m| m.name == module)
                    || u.ast.concepts.iter().any(|c| c.name == module)
            })
            .unwrap_or(run.units.len() - 1)
    };
    let record = |module: &str, item: String, why: String, report: &mut Report| {
        let ui = unit_of(module);
        let u = &run.units[ui];
        let readable = resolve::readable(&item);
        let routine = item.rsplit('.').next().unwrap_or(&item).to_string();
        let mut d = Diagnostic::basic("error", RULE, readable.clone(), BREAKING_TAG.to_string(), format!("{readable}: {why}"))
            .with_module(&resolve::readable(module));
        d.span = Some(locate_span(&u.src, &u.path, &routine, module));
        report.breaking.push((ui, d));
    };
    let warn = |module: &str, tag: &str, message: String, report: &mut Report| {
        let ui = unit_of(module);
        let u = &run.units[ui];
        let readable = resolve::readable(module);
        let mut d = Diagnostic::basic("warning", RULE, readable.clone(), tag.to_string(), message)
            .with_module(&readable);
        d.span = Some(locate_span(&u.src, &u.path, module, module));
        report.warnings.push((ui, d));
    };
    // A name that is new here may be one a client already uses bare from another package,
    // which is then `E0105`, or `E0112` for a constructor. This cannot be seen from here, and
    // one qualifier fixes it, so it is a warning.
    let new_name = |name: &str, what: String, report: &mut Report| {
        let message = format!("{what} A client that uses the same name bare from another package must now qualify it.");
        warn(name, NEW_NAME_TAG, message, report);
    };
    if old.package != run.package {
        record(
            "",
            run.package.to_string(),
            format!("the package was named `{}`, and a client finds a package by name", old.package),
            &mut report,
        );
    }

    // A build holds one package of a name (decision 5), and transitive packages count, so a
    // client with its own copy of one of these is refused.
    for dep in run.dependencies {
        let name = &dep.name;
        match old.dependencies.iter().find(|d| d.name == *name) {
            None => record(
                "",
                run.package.to_string(),
                format!("it now depends on `{name}`, and a client with another copy of that package is refused"),
                &mut report,
            ),
            Some(was) if was.version != dep.version => record(
                "",
                run.package.to_string(),
                format!("its dependency `{name}` went from {} to {}, and a client with the old copy is refused", was.version, dep.version),
                &mut report,
            ),
            // Two copies of one version: nothing says what changed, or that it matters.
            Some(was) if was.sources != dep.sources => warn(
                run.package,
                DEPENDENCY_MOVED_TAG,
                format!("Dependency `{name}` changed but is still {}. Give it a new version, and compare it with its old one.", dep.version),
                &mut report,
            ),
            Some(_) => {}
        }
    }

    // No contract shows this, and a body-only change can bring it in.
    let mut kept = Vec::new();
    for (module, assume) in assumes_of(run.units) {
        let (routine, tag) = assume.split_once(' ').unwrap_or((&assume, ""));
        let item = format!("{module}.{routine}");
        if old.assumes.contains(&(module.clone(), assume.clone())) {
            kept.push(format!("`assume {tag}` in {}", resolve::readable(&item)));
        } else {
            record(&module, item, format!("`assume {tag}` is new, and `--strict` refuses it in every client"), &mut report);
        }
    }
    // A proof that newly rests on an old `assume` looks like any other, as its contract need
    // not change. `--strict` never gets here, as it refuses them first.
    if !kept.is_empty() {
        let message = format!("Taken as stated: {}. A proof of this version may rest on these, and `--strict` refuses them.", kept.join(", "));
        warn(run.package, RESTS_ON_ASSUME_TAG, message, &mut report);
    }

    let new_concepts: Vec<&ConceptDecl> =
        run.units[run.root_units.clone()].iter().flat_map(|u| &u.ast.concepts).collect();
    for o in &old.concepts {
        let Some(n) = new_concepts.iter().find(|c| c.name == o.name) else {
            record(&o.name, o.name.clone(), "the concept is removed".to_string(), &mut report);
            continue;
        };
        for why in compare_concept(o, n) {
            record(&o.name, o.name.clone(), why, &mut report);
        }
    }
    for c in new_concepts.iter().filter(|c| !old.concepts.iter().any(|o| o.name == c.name)) {
        report.added.push(resolve::readable(&c.name));
        new_name(&c.name, format!("Concept `{}` is new.", resolve::readable(&c.name)), &mut report);
    }

    let new_modules: Vec<&ModuleDecl> = run
        .units
        .iter()
        .flat_map(|u| &u.ast.modules)
        .filter(|m| !run.packages.contains_key(&m.name) && !m.kind.is_entry_point())
        .collect();
    let refines: HashMap<String, Vec<String>> = standard_prelude()
        .concepts
        .iter()
        .chain(run.units.iter().flat_map(|u| &u.ast.concepts))
        .map(|c| (c.name.clone(), c.refines.clone()))
        .collect();
    let mut claims: Vec<Claim> = Vec::new();
    for raw_old in &old.modules {
        let Some(raw_new) = new_modules.iter().find(|m| m.name == raw_old.name) else {
            record(&raw_old.name, raw_old.name.clone(), "the module is removed".to_string(), &mut report);
            continue;
        };
        let (o, n) = (contract_only_module(raw_old), contract_only_module(raw_new));
        if o.repr_vis != Visibility::Any && n.repr_vis == Visibility::Any {
            new_name(&n.name, format!("The representation of `{}` is now public, with its constructors.", resolve::readable(&n.name)), &mut report);
        }
        let new_names = public_names(&n);
        let gone = public_names(&o)
            .into_iter()
            .filter(|(k, v)| new_names.get(k) != Some(v))
            .map(|(k, _)| k)
            .collect();
        let mut c = Comparison { old: &o, new: &n, refines: &refines, gone, claims: Vec::new(), breaking: Vec::new(), added: Vec::new() };
        c.shape();
        let self_routines = n.kind == ModuleKind::Module;
        c.routines("create", false, &o.create_routines, &n.create_routines);
        c.routines("feature", self_routines, &o.feature_routines, &n.feature_routines);
        c.routines("specify", self_routines, &o.specify_routines, &n.specify_routines);
        c.laws(&o.specify_laws, &n.specify_laws);
        c.invariants();
        let (found, more, added) = (c.breaking, c.claims, c.added);
        for (item, why) in found {
            record(&n.name, item, why, &mut report);
        }
        claims.extend(more);
        report.added.extend(added.iter().map(|a| resolve::readable(a)));
    }
    for m in &new_modules {
        if !old.modules.iter().any(|o| o.name == m.name) {
            report.added.push(resolve::readable(&m.name));
            new_name(&m.name, format!("Module `{}` is new.", resolve::readable(&m.name)), &mut report);
        }
    }
    report.added.sort();

    let mut proved = HashSet::new();
    let mut why_not: HashMap<usize, String> = HashMap::new();
    let by_module: BTreeMap<&str, Vec<(usize, &Claim)>> =
        claims.iter().enumerate().fold(BTreeMap::new(), |mut map, (i, c)| {
            map.entry(c.module.as_str()).or_insert_with(Vec::new).push((i, c));
            map
        });
    if !by_module.is_empty() {
        let bare: Vec<(ModuleDecl, usize)> = run
            .units
            .iter()
            .flat_map(|u| &u.ast.modules)
            .enumerate()
            .map(|(i, m)| (without_contracts(m), i))
            .collect();
        // Every module is its own package, so only signatures cross between them.
        let of_package: HashMap<String, String> =
            bare.iter().map(|(m, _)| (m.name.clone(), m.name.clone())).collect();
        let table = Rc::new(LinkTable::with_packages(bare.clone(), of_package));
        with_z3_seed(run.z3_seed, || with_z3_rlimit(run.z3_rlimit, || with_prelude_injection(run.prelude_mode, || {
            with_link_table(Some(table), || {
                for (module, list) in &by_module {
                    let Some(solvers) = &run.solvers else {
                        why_not.extend(list.iter().map(|(i, _)| (*i, "no working Boogie and Z3".to_string())));
                        continue;
                    };
                    let Some((m, _)) = bare.iter().find(|(m, _)| m.name == *module) else {
                        continue;
                    };
                    let ask = |claims: &[(usize, &Claim)]| {
                        ask_solver(&query(m, claims, run.fuel), module, (solvers.0.as_str(), solvers.1.as_str()))
                    };
                    match ask(list) {
                        Ok(yes) => proved.extend(yes),
                        // One clause Boogie cannot read must not answer for the others.
                        Err(NoAnswer::Refused(_)) if list.len() > 1 => {
                            for one in list {
                                match ask(std::slice::from_ref(one)) {
                                    Ok(yes) => proved.extend(yes),
                                    Err(e) => {
                                        why_not.insert(one.0, e.reason());
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let reason = e.reason();
                            why_not.extend(list.iter().map(|(i, _)| (*i, reason.clone())));
                        }
                    }
                }
            });
        })));
    }
    report.proved = proved.len();
    for (i, c) in claims.iter().enumerate() {
        if !proved.contains(&i) {
            let mut why = c.meaning.clone();
            if let Some(reason) = why_not.get(&i) {
                why.push_str(&format!(" (not compared: {reason})"));
            }
            record(&c.module, c.item.clone(), why, &mut report);
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_procedure_named_as_verified_is_proved() {
        let out = "Verifying Compare_0 ...\n  [0.1 s, 1 proof obligation]  verified\n\n\
                   Verifying Compare_1 ...\n  [0.1 s, 1 proof obligation]  error\n\n\
                   Verifying Compare_2 ...\n  [0.1 s, 1 proof obligation]  timed out\n\n\
                   Verifying Compare_3 ...\n\n\
                   Boogie program verifier finished with 1 verified, 1 error\n";
        assert_eq!(proved_in(out), HashSet::from([0]));
        assert!(proved_in("a.bpl(3,1): Error: type mismatch\n1 name resolution errors\n").is_empty());
    }

    fn unit(path: &str, src: &str) -> SourceUnit {
        let ast = Parser::new(tokenize(src).unwrap()).parse_file().unwrap();
        SourceUnit { path: path.to_string(), src: src.to_string(), ast }
    }

    /// No comparison, no compatibility: a change the solver never answered counts as breaking.
    #[test]
    fn an_unproven_comparison_is_breaking() {
        let module = |limit: u32| {
            format!("functions CALC

feature {{ANY}}

  f(x: U64): U64
    require
      r: x < {limit}
    do
      x
    end
end
")
        };
        let dir = std::env::temp_dir().join(format!("gardel_compat_unit_{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Gardel.toml"), "[project]
name = \"acme.lib\"
version = \"0.1.0\"
").unwrap();
        std::fs::write(dir.join("src/calc.gdl"), module(10)).unwrap();

        let units = [unit("calc.gdl", &module(20))];
        let packages = HashMap::new();
        let run = |solvers: Option<(String, String)>| {
            compare(
                &dir,
                &Run { package: "acme.lib", units: &units, root_units: 0..1, dependencies: &[], packages: &packages, prelude_mode: true, fuel: 2, solvers, z3_seed: None, z3_rlimit: None },
            )
            .unwrap()
        };
        for solvers in [None, Some(("no-such-boogie".to_string(), "no-such-z3".to_string()))] {
            let report = run(solvers);
            assert_eq!(report.proved, 0);
            assert_eq!(report.breaking.len(), 1);
            assert!(report.breaking[0].1.message.contains("not compared"), "{}", report.breaking[0].1.message);
        }
        drop(std::fs::remove_dir_all(&dir));
    }

    /// `--z3-seed` and `--z3-rlimit` reach this query as they reach every other one.
    #[test]
    fn a_query_uses_the_limits_of_the_run() {
        let args = with_z3_seed(Some(7), || with_z3_rlimit(Some(1234), || solver_args("z3", 60)));
        assert!(args.iter().any(|a| a == "/rlimit:1234"), "{args:?}");
        assert!(args.iter().any(|a| a.contains("random_seed=7")), "{args:?}");
        assert!(!solver_args("z3", 60).iter().any(|a| a.contains("random_seed")));
    }
}
