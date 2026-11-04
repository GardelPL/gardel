//! Pattern-match compilation (desugaring pass after parsing).
//!
//! Literal patterns (`case 0`), tuple patterns (`case (a, b)`), nested variant patterns
//! (`case Some(Some(x))`) and a top-level binder (`case x`) are compiled into the forms
//! the verifier and the C backend lower directly:
//!   * flat variant matches whose sub-patterns are binders or `_`,
//!   * a `case _` default arm,
//!   * `if` chains comparing a scalar with literals, and
//!   * arm-local `let`s for bound names.
//! This is the classic clause-matrix algorithm: rows are tried in order, so the first
//! matching row wins, as in the source. Gardel is pure, so a column expression such as
//! `p.first` may be evaluated more than once without changing the meaning.
//! Exhaustiveness is still decided by the flat matches produced (a missing constructor
//! stays a missing constructor). A literal column without a default row is left alone
//! unless it is a complete `BOOL` (`true` and `false`), so the original diagnostics apply.

use super::*;

#[derive(Clone)]
struct Row {
    pats: Vec<Pattern>,
    lets: Vec<(String, Expr)>,
    stmts: Vec<Stmt>,
    body: Expr,
}

struct Compiler {
    counter: usize,
}

fn is_flat(p: &Pattern) -> bool {
    match p {
        Pattern::Wildcard => true,
        Pattern::Variant(_, subs) => subs.iter().all(|s| matches!(s, Pattern::Wildcard | Pattern::Bind(_))),
        _ => false,
    }
}

fn lit_expr(l: &Literal) -> Expr {
    match l {
        Literal::Int(n) => Expr::Int(*n),
        Literal::Float(f) => Expr::Float(*f),
        Literal::Char(c) => Expr::Char(*c),
        Literal::Bool(b) => Expr::Bool(*b),
        Literal::Str(s) => Expr::Str(s.clone()),
        Literal::Unit => Expr::Unit,
    }
}

fn same_lit(a: &Literal, b: &Literal) -> bool {
    match (a, b) {
        (Literal::Float(x), Literal::Float(y)) => x.to_bits() == y.to_bits(),
        _ => a == b,
    }
}

impl Compiler {
    fn fresh(&mut self) -> String {
        self.counter += 1;
        format!("_pm{}", self.counter)
    }

    /// Compiles `rows` against the column expressions `cols` into a block. `None` when
    /// the matrix cannot be compiled without changing diagnostics (see module docs).
    fn compile(&mut self, cols: &[Expr], mut rows: Vec<Row>) -> Option<(Vec<Stmt>, Expr)> {
        // Binders become `let`s; their columns become wildcards.
        for r in &mut rows {
            for (i, p) in r.pats.iter_mut().enumerate() {
                if let Pattern::Bind(x) = p {
                    r.lets.push((x.clone(), cols[i].clone()));
                    *p = Pattern::Wildcard;
                }
            }
        }
        let first = rows.first()?;
        let Some(col) = first.pats.iter().position(|p| !matches!(p, Pattern::Wildcard)) else {
            let r = rows.into_iter().next()?;
            let mut stmts: Vec<Stmt> = r.lets.into_iter().map(|(x, e)| Stmt::Let(x, e)).collect();
            stmts.extend(r.stmts);
            return Some((stmts, r.body));
        };
        let scrut = cols[col].clone();
        let rest_cols = |extra: Vec<Expr>| -> Vec<Expr> {
            let mut c: Vec<Expr> = cols[..col].to_vec();
            c.extend(extra);
            c.extend_from_slice(&cols[col + 1..]);
            c
        };
        let splice = |r: &Row, extra: Vec<Pattern>| -> Row {
            let mut pats: Vec<Pattern> = r.pats[..col].to_vec();
            pats.extend(extra);
            pats.extend_from_slice(&r.pats[col + 1..]);
            Row { pats, ..r.clone() }
        };
        match &first.pats[col] {
            Pattern::Tuple(subs) => {
                let k = subs.len();
                if k != 2 {
                    return None;
                }
                let new_cols = rest_cols(vec![
                    Expr::MethodCall(Box::new(scrut.clone()), "first".into(), Vec::new()),
                    Expr::MethodCall(Box::new(scrut), "second".into(), Vec::new()),
                ]);
                let mut out = Vec::new();
                for r in &rows {
                    match &r.pats[col] {
                        Pattern::Tuple(ps) if ps.len() == k => out.push(splice(r, ps.clone())),
                        Pattern::Wildcard => out.push(splice(r, vec![Pattern::Wildcard; k])),
                        _ => return None,
                    }
                }
                self.compile(&new_cols, out)
            }
            Pattern::Lit(_) => {
                let mut lits: Vec<Literal> = Vec::new();
                for r in &rows {
                    if let Pattern::Lit(l) = &r.pats[col] {
                        if !lits.iter().any(|x| same_lit(x, l)) {
                            lits.push(l.clone());
                        }
                    }
                }
                let new_cols = rest_cols(Vec::new());
                let default_rows: Vec<Row> = rows
                    .iter()
                    .filter(|r| matches!(r.pats[col], Pattern::Wildcard))
                    .map(|r| splice(r, Vec::new()))
                    .collect();
                let complete_bool = lits.len() == 2
                    && lits.iter().any(|l| matches!(l, Literal::Bool(true)))
                    && lits.iter().any(|l| matches!(l, Literal::Bool(false)));
                let mut branches = Vec::new();
                for l in &lits {
                    let spec: Vec<Row> = rows
                        .iter()
                        .filter(|r| match &r.pats[col] {
                            Pattern::Lit(x) => same_lit(x, l),
                            Pattern::Wildcard => true,
                            _ => false,
                        })
                        .map(|r| splice(r, Vec::new()))
                        .collect();
                    branches.push((l.clone(), self.compile(&new_cols, spec)?));
                }
                let (mut es, mut ee) = if !default_rows.is_empty() {
                    self.compile(&new_cols, default_rows)?
                } else if complete_bool {
                    branches.pop()?.1
                } else {
                    return None;
                };
                for (l, (ts, te)) in branches.into_iter().rev() {
                    let cond = Expr::BinOp("=".into(), Box::new(scrut.clone()), Box::new(lit_expr(&l)));
                    ee = Expr::If(Box::new(cond), ts, Box::new(te), es, Box::new(ee));
                    es = Vec::new();
                }
                Some((es, ee))
            }
            Pattern::Variant(_, _) => {
                // One flat arm per constructor, in order of first appearance.
                let mut ctors: Vec<(String, usize)> = Vec::new();
                for r in &rows {
                    if let Pattern::Variant(c, subs) = &r.pats[col] {
                        if !ctors.iter().any(|(x, _)| x == c) {
                            ctors.push((c.clone(), subs.len()));
                        }
                    }
                }
                let mut cases = Vec::new();
                for (c, k) in &ctors {
                    let vars: Vec<String> = (0..*k).map(|_| self.fresh()).collect();
                    let new_cols = rest_cols(vars.iter().map(|v| Expr::Var(v.clone())).collect());
                    let mut spec = Vec::new();
                    for r in &rows {
                        match &r.pats[col] {
                            Pattern::Variant(x, subs) if x == c && subs.len() == *k => {
                                spec.push(splice(r, subs.clone()))
                            }
                            Pattern::Wildcard => spec.push(splice(r, vec![Pattern::Wildcard; *k])),
                            _ => {}
                        }
                    }
                    let (stmts, body) = self.compile(&new_cols, spec)?;
                    let pat = Pattern::Variant(c.clone(), vars.iter().map(|v| Pattern::Bind(v.clone())).collect());
                    cases.push(MatchCase::new(pat, stmts, body));
                }
                let default_rows: Vec<Row> = rows
                    .iter()
                    .filter(|r| matches!(r.pats[col], Pattern::Wildcard))
                    .map(|r| splice(r, Vec::new()))
                    .collect();
                if !default_rows.is_empty() {
                    let (stmts, body) = self.compile(&rest_cols(Vec::new()), default_rows)?;
                    cases.push(MatchCase::new(Pattern::Wildcard, stmts, body));
                }
                Some((Vec::new(), Expr::Match(Box::new(scrut), cases)))
            }
            _ => None,
        }
    }
}

/// Wraps a block as an expression: `if true then <block> else <block>` when it has
/// statements (the verifier and the C backend both lower `if` blocks; the dead branch
/// is never taken).
fn block_expr(stmts: Vec<Stmt>, body: Expr) -> Expr {
    if stmts.is_empty() {
        body
    } else {
        Expr::If(
            Box::new(Expr::Bool(true)),
            stmts.clone(),
            Box::new(body.clone()),
            stmts,
            Box::new(body),
        )
    }
}

fn rewrite_expr(e: &mut Expr, c: &mut Compiler) {
    match e {
        Expr::Int(_) | Expr::Float(_) | Expr::Char(_) | Expr::Unit | Expr::Bool(_) | Expr::Str(_) | Expr::Var(_) => {}
        Expr::UnaryNot(x) | Expr::UnaryNeg(x) => rewrite_expr(x, c),
        Expr::BinOp(_, l, r) => {
            rewrite_expr(l, c);
            rewrite_expr(r, c);
        }
        Expr::Call(_, args) => args.iter_mut().for_each(|a| rewrite_expr(a, c)),
        Expr::MethodCall(recv, _, args) => {
            rewrite_expr(recv, c);
            args.iter_mut().for_each(|a| rewrite_expr(a, c));
        }
        Expr::If(cnd, ts, te, es, ee) => {
            rewrite_expr(cnd, c);
            rewrite_block(ts, te, c);
            rewrite_block(es, ee, c);
        }
        Expr::Match(scrut, cases) => {
            rewrite_expr(scrut, c);
            for k in cases.iter_mut() {
                rewrite_block(&mut k.stmts, &mut k.body, c);
            }
            if cases.iter().all(|k| is_flat(&k.pattern)) {
                return;
            }
            let rows: Vec<Row> = cases
                .iter()
                .map(|k| Row {
                    pats: vec![k.pattern.clone()],
                    lets: Vec::new(),
                    stmts: k.stmts.clone(),
                    body: k.body.clone(),
                })
                .collect();
            let saved = c.counter;
            match c.compile(&[(**scrut).clone()], rows) {
                Some((stmts, body)) => *e = block_expr(stmts, body),
                None => c.counter = saved,
            }
        }
        Expr::RecordLit(fs) => fs.iter_mut().for_each(|(_, x)| rewrite_expr(x, c)),
        Expr::RecordUpdate(b, fs) => {
            rewrite_expr(b, c);
            fs.iter_mut().for_each(|(_, x)| rewrite_expr(x, c));
        }
        Expr::TupleLit(xs) => xs.iter_mut().for_each(|x| rewrite_expr(x, c)),
        Expr::Forall(_, _, b) | Expr::Exists(_, _, b) => rewrite_expr(b, c),
        Expr::WithIo(io, fin) => {
            for st in io.iter_mut() {
                match st {
                    IoStmt::Bind(_, x) | IoStmt::Action(x) => rewrite_expr(x, c),
                }
            }
            rewrite_expr(fin, c);
        }
        Expr::Lambda(_, _, stmts, body) => rewrite_block(stmts, body, c),
    }
}

fn rewrite_block(stmts: &mut [Stmt], body: &mut Expr, c: &mut Compiler) {
    for st in stmts.iter_mut() {
        match st {
            Stmt::Let(_, x) | Stmt::Check(_, x) | Stmt::Assume(_, x) => rewrite_expr(x, c),
            Stmt::Reveal(_) => {}
        }
    }
    rewrite_expr(body, c);
}

/// Compiles every non-flat `match` of `m` (routine bodies, contracts and law bodies).
pub(crate) fn desugar_patterns(m: &mut ModuleDecl) {
    let mut c = Compiler { counter: 0 };
    for (_, r) in m
        .create_routines
        .iter_mut()
        .chain(m.feature_routines.iter_mut())
        .chain(m.specify_routines.iter_mut())
    {
        for (_, e) in r.requires.iter_mut().chain(r.ensures.iter_mut()) {
            rewrite_expr(e, &mut c);
        }
        rewrite_block(&mut r.stmts, &mut r.body, &mut c);
    }
    for (_, e) in m.invariants.iter_mut() {
        rewrite_expr(e, &mut c);
    }
}
