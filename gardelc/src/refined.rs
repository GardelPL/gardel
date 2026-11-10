//! Refined function types (spec §8, Task F.12).
//!
//! A refined function type names its parameters and carries clauses:
//! `(x: NAT) -> NAT require small: x < 100 ensure bounded: Result <= x`.
//! The clauses are enforced, never inert (decision 12 of the Phase F plan):
//!
//! * Each `ensure` becomes a contract of the place that holds the function, stated for every
//!   argument the `require` allows. On a routine or law parameter `f` it is a `require` of the
//!   routine, `f_bounded: forall x: NAT. (x < 100) implies (f(x) <= x)`, so every caller proves
//!   it of what it passes, a lambda or a function it holds, and the body assumes it. On a record
//!   field it is an invariant of the record, proven where one is built. On a routine's result it
//!   is an `ensure` of the routine.
//! * Each `require` is proven at every application of the function (in the verifier, see
//!   `emit_boogie_expr_obligations`), and a lambda passed where it is expected may assume it.
//!   That is sound only if the function is never applied outside it, so a parameter whose type
//!   has a `require` may only be applied, or passed as an argument, where the verifier proves
//!   that the receiving type's `require` implies its own. Anything else is `E0322`.
//!
//! A `require` is accepted on a parameter only, and the clauses anywhere other than a
//! parameter, a field or a result (a `let`, a lambda, a quantifier, a type argument, a nested
//! function type, a variant) are `E0321`: nothing would enforce them there.

use super::*;

/// The parts of a refined function type.
#[derive(Debug, Clone)]
pub(crate) struct FnContract {
    pub(crate) params: Vec<(String, String)>,
    pub(crate) requires: Vec<(String, Expr)>,
    pub(crate) ensures: Vec<(String, Expr)>,
}

impl FnContract {
    /// The `require` clauses, conjoined, with the parameters renamed to `names`.
    pub(crate) fn guard(&self, names: &[Expr]) -> Option<Expr> {
        let substs = self.renames(names);
        self.requires
            .iter()
            .map(|(_, e)| subst_expr_vars(e, &substs))
            .reduce(|a, b| Expr::BinOp("and".to_string(), Box::new(a), Box::new(b)))
    }

    /// Each parameter paired with the expression that replaces it.
    pub(crate) fn renames(&self, names: &[Expr]) -> Vec<(String, Expr)> {
        self.params.iter().map(|(p, _)| p.clone()).zip(names.iter().cloned()).collect()
    }
}

/// True when the type text carries the clauses of a refined function type, at any depth.
pub(crate) fn has_clauses(ty: &str) -> bool {
    ty.contains(" require ") || ty.contains(" ensure ")
}

/// The contract of a refined function type at the top of `ty`, or `None` when `ty` is not a
/// function type with named parameters, or its clauses belong to a nested type.
pub(crate) fn fn_contract(ty: &str) -> Option<FnContract> {
    let (lhs, rhs) = split_top_level_arrow(ty.trim())?;
    let inner = lhs.trim().strip_prefix('(')?.strip_suffix(')')?;
    let mut params = Vec::new();
    for p in split_top_level_commas(inner) {
        let (n, t) = p.split_once(':')?;
        params.push((n.trim().to_string(), t.trim().to_string()));
    }
    let cut = [" require ", " ensure "].iter().filter_map(|k| rhs.find(k)).min();
    let (ret, clauses) = match cut {
        Some(i) => (&rhs[..i], &rhs[i..]),
        None => (rhs, ""),
    };
    if ret.contains("->") || params.iter().any(|(_, t)| has_clauses(t)) {
        return None;
    }
    let tokens = tokenize(clauses).ok()?;
    let mut parser = Parser::new(tokens);
    let (requires, ensures) = parser.parse_type_clauses().ok()?;
    if *parser.peek() != Token::Eof {
        return None;
    }
    Some(FnContract { params, requires, ensures })
}

/// `forall x1: T1. .. (require) implies (clause)`, each parameter renamed to `{owner}_{x}` so
/// that no name of the place that holds the function is captured, and `Result` replaced by
/// `holder(x1, ..)`, the function applied.
fn contract_fact(c: &FnContract, owner: &str, holder: &Expr, clause: &Expr) -> Expr {
    let vars: Vec<(String, String)> =
        c.params.iter().map(|(p, t)| (format!("{owner}_{p}"), t.clone())).collect();
    let names: Vec<Expr> = vars.iter().map(|(v, _)| Expr::Var(v.clone())).collect();
    let applied = Expr::MethodCall(Box::new(holder.clone()), "apply".to_string(), names.clone());
    let mut substs = c.renames(&names);
    substs.push(("Result".to_string(), applied));
    let ens = subst_expr_vars(clause, &substs);
    let mut body = match c.guard(&names) {
        Some(g) => Expr::BinOp("implies".to_string(), Box::new(g), Box::new(ens)),
        None => ens,
    };
    for (v, t) in vars.into_iter().rev() {
        body = Expr::Forall(v, t, Box::new(body));
    }
    body
}

/// What a value of type `from` needs to be passed where `to` is expected, beyond `to`'s own
/// `ensure` clauses: `to`'s `require` implies each of `from`'s. One fact per clause of `from`,
/// with its tag. `None` when `to` is no refined type of the same arity, so nothing can.
pub(crate) fn require_weakening(from: &FnContract, to: Option<&FnContract>, owner: &str) -> Option<Vec<(String, Expr)>> {
    let to = to.filter(|t| t.params.len() == from.params.len())?;
    let vars: Vec<(String, String)> =
        to.params.iter().map(|(p, t)| (format!("{owner}_{p}"), t.clone())).collect();
    let names: Vec<Expr> = vars.iter().map(|(v, _)| Expr::Var(v.clone())).collect();
    let guard = to.guard(&names);
    let substs = from.renames(&names);
    Some(
        from.requires
            .iter()
            .map(|(tag, e)| {
                let need = subst_expr_vars(e, &substs);
                let mut body = match &guard {
                    Some(g) => Expr::BinOp("implies".to_string(), Box::new(g.clone()), Box::new(need)),
                    None => need,
                };
                for (v, t) in vars.iter().rev() {
                    body = Expr::Forall(v.clone(), t.clone(), Box::new(body));
                }
                (tag.clone(), body)
            })
            .collect(),
    )
}

fn position_error(m: &str, site: &str, what: &str) -> String {
    format!(
        "E0321: refined_function_type_position: in `{m}.{site}`, {what}. Clauses on a function type are enforced only on a routine or law parameter, a record field and a routine's result, and a `require` only on a parameter (§8)"
    )
}

/// The contract of a type in a position that allows clauses, checked: `Ok(None)` for a type
/// with none, an error for clauses this position cannot enforce.
fn contract_at(ty: &str, m: &str, site: &str, what: &str, allow_require: bool) -> Result<Option<FnContract>, String> {
    if !has_clauses(ty) {
        return Ok(None);
    }
    let Some(c) = fn_contract(ty) else {
        return Err(position_error(m, site, &format!("{what} has clauses on a nested function type")));
    };
    if !allow_require && !c.requires.is_empty() {
        return Err(position_error(m, site, &format!("{what} has a `require`, which is accepted on a parameter only")));
    }
    Ok(Some(c))
}

/// The expressions directly inside `e`, those inside its statements included, and its
/// statements.
fn parts(e: &Expr) -> (Vec<&Expr>, Vec<&Stmt>) {
    let mut es: Vec<&Expr> = Vec::new();
    let mut ss: Vec<&Stmt> = Vec::new();
    fn add<'a>(stmts: &'a [Stmt], es: &mut Vec<&'a Expr>, ss: &mut Vec<&'a Stmt>) {
        for s in stmts {
            ss.push(s);
            if let Stmt::Let(_, x) | Stmt::Check(_, x) | Stmt::Assume(_, x) = s {
                es.push(x);
            }
        }
    }
    match e {
        Expr::Int(_) | Expr::Float(_) | Expr::Char(_) | Expr::Unit | Expr::Bool(_) | Expr::Str(_) | Expr::Var(_) => {}
        Expr::UnaryNot(x) | Expr::UnaryNeg(x) => es.push(x),
        Expr::BinOp(_, l, r) => {
            es.push(l);
            es.push(r);
        }
        Expr::Call(_, args) | Expr::TupleLit(args) => es.extend(args.iter()),
        Expr::MethodCall(recv, _, args) => {
            es.push(recv);
            es.extend(args.iter());
        }
        Expr::If(c, ts, te, xs, xe) => {
            es.push(c);
            add(ts, &mut es, &mut ss);
            es.push(te);
            add(xs, &mut es, &mut ss);
            es.push(xe);
        }
        Expr::Match(s, cases) => {
            es.push(s);
            for c in cases {
                add(&c.stmts, &mut es, &mut ss);
                es.push(&c.body);
            }
        }
        Expr::RecordLit(fs) => es.extend(fs.iter().map(|(_, v)| v)),
        Expr::RecordUpdate(b, fs) => {
            es.push(b);
            es.extend(fs.iter().map(|(_, v)| v));
        }
        Expr::Forall(_, _, b) | Expr::Exists(_, _, b) => es.push(b),
        Expr::WithIo(io, fin) => {
            for s in io {
                match s {
                    IoStmt::Bind(_, x) | IoStmt::Action(x) => es.push(x),
                }
            }
            es.push(fin);
        }
        Expr::Lambda(_, _, stmts, body) => {
            add(stmts, &mut es, &mut ss);
            es.push(body);
        }
    }
    (es, ss)
}

/// A block as one expression, for the walks below.
fn as_block(stmts: &[Stmt], body: &Expr) -> Expr {
    Expr::If(Box::new(Expr::Bool(true)), stmts.to_vec(), Box::new(body.clone()), Vec::new(), Box::new(Expr::Unit))
}

/// The first type written inside `e` (a lambda's, a quantifier's or a `let`'s) that has clauses.
fn clauses_inside(e: &Expr) -> Option<String> {
    let own: Vec<&str> = match e {
        Expr::Lambda(ps, ret, _, _) => ps.iter().map(|(_, t)| t.as_str()).chain([ret.as_str()]).collect(),
        Expr::Forall(_, t, _) | Expr::Exists(_, t, _) => vec![t.as_str()],
        _ => Vec::new(),
    };
    if let Some(t) = own.into_iter().find(|t| has_clauses(t)) {
        return Some(t.to_string());
    }
    let (es, ss) = parts(e);
    for s in ss {
        if let Stmt::LetType(_, t) = s {
            if has_clauses(t) {
                return Some(t.clone());
            }
        }
    }
    es.into_iter().find_map(clauses_inside)
}

/// The first use of a name in `names` that neither applies it nor passes it as an argument.
fn escape(e: &Expr, names: &HashSet<String>, passed: bool) -> Option<String> {
    match e {
        Expr::Var(v) if names.contains(v) && !passed => Some(v.clone()),
        Expr::Call(_, args) => args.iter().find_map(|a| escape(a, names, true)),
        Expr::MethodCall(recv, mname, args) => {
            let applied = mname == "apply" && matches!(&**recv, Expr::Var(_));
            escape(recv, names, applied).or_else(|| args.iter().find_map(|a| escape(a, names, true)))
        }
        _ => parts(e).0.into_iter().find_map(|x| escape(x, names, false)),
    }
}

/// Task F.12: turns the clauses of refined function types into contracts (see the module
/// documentation), and refuses those that nothing would enforce.
pub(crate) fn desugar_refined_fn_types(m: &mut ModuleDecl) -> Result<(), String> {
    let mname = m.name.clone();
    match &m.repr {
        ReprKind::Product(fields) => {
            let mut added = Vec::new();
            for (f, ty) in fields {
                if let Some(c) = contract_at(ty, &mname, f, &format!("field `{f}`"), false)? {
                    let holder = Expr::MethodCall(Box::new(Expr::Var("Current".to_string())), f.clone(), Vec::new());
                    for (tag, ens) in &c.ensures {
                        added.push((format!("{f}_{tag}"), contract_fact(&c, f, &holder, ens)));
                    }
                }
            }
            m.invariants.extend(added);
        }
        ReprKind::Sum(variants) => {
            for v in variants {
                if let Some((f, _)) = v.fields.iter().find(|(_, t)| has_clauses(t)) {
                    return Err(position_error(&mname, &v.name, &format!("field `{f}` of a variant has clauses")));
                }
            }
        }
    }
    for (tag, e) in &m.invariants {
        if clauses_inside(e).is_some() {
            return Err(position_error(&mname, tag, "a type inside the invariant has clauses"));
        }
    }
    let routines = m
        .specify_routines
        .iter_mut()
        .chain(m.create_routines.iter_mut())
        .chain(m.feature_routines.iter_mut())
        .map(|(_, r)| r);
    for r in routines {
        let mut requires = Vec::new();
        let mut restricted = HashSet::new();
        for (p, ty) in &r.params {
            if let Some(c) = contract_at(ty, &mname, &r.name, &format!("parameter `{p}`"), true)? {
                let holder = Expr::Var(p.clone());
                for (tag, ens) in &c.ensures {
                    requires.push((format!("{p}_{tag}"), contract_fact(&c, p, &holder, ens)));
                }
                if !c.requires.is_empty() {
                    restricted.insert(p.clone());
                }
            }
        }
        if let Some(c) = contract_at(&r.ret_type, &mname, &r.name, "the result", false)? {
            let holder = Expr::Var("Result".to_string());
            for (tag, ens) in &c.ensures {
                let fact = contract_fact(&c, "result", &holder, ens);
                r.ensures.push((format!("result_{tag}"), fact));
            }
        }
        let body = as_block(&r.stmts, &r.body);
        let contracts = r.requires.iter().chain(r.ensures.iter()).map(|(_, e)| e);
        if clauses_inside(&body).is_some() || contracts.chain(r.variant.iter()).any(|e| clauses_inside(e).is_some()) {
            return Err(position_error(&mname, &r.name, "a type written in the routine has clauses"));
        }
        if let Some(f) = escape(&body, &restricted, false) {
            return Err(format!(
                "E0322: refined_function_escapes: in `{mname}.{}`, `{f}` has a `require`, so it may only be applied or passed as an argument, where the receiving type's `require` must imply it. Stored, returned or bound to a name, it could be applied outside its `require` (§8)",
                r.name
            ));
        }
        r.requires.extend(requires);
    }
    for (_, l) in &mut m.specify_laws {
        let mut requires = Vec::new();
        for (p, ty) in &l.params {
            if let Some(c) = contract_at(ty, &mname, &l.name, &format!("parameter `{p}`"), false)? {
                let holder = Expr::Var(p.clone());
                for (tag, ens) in &c.ensures {
                    requires.push((format!("{p}_{tag}"), contract_fact(&c, p, &holder, ens)));
                }
            }
        }
        let mut exprs: Vec<Expr> = l.requires.iter().chain(l.ensures.iter()).map(|(_, e)| e.clone()).collect();
        exprs.extend(l.triggers.iter().cloned());
        exprs.extend(l.variant.iter().cloned());
        exprs.push(as_block(&l.stmts, l.body.as_ref().unwrap_or(&Expr::Unit)));
        if exprs.iter().any(|e| clauses_inside(e).is_some()) {
            return Err(position_error(&mname, &l.name, "a type written in the law has clauses"));
        }
        l.requires.extend(requires);
    }
    Ok(())
}
