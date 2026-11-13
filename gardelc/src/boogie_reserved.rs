//! Names that Boogie reserves and Gardel does not, such as `old`, `var` or `type`.
//!
//! A routine's parameters, `let`s, `match` binders, lambda parameters, quantified variables and
//! `<-` binds reach the emitted Boogie as they are written, so `case Some(old)` was a parse error
//! in Boogie and the module was `E0199`. `rename_reserved` gives each such name a fresh one in
//! its routine, `old_1`, at every binding and at every use in its scope. All the bindings of one
//! name in a routine get the same new name, so shadowing is as it was. A name used where no
//! binding of it is in scope, such as a field of `Current`, is left alone.

use super::*;

/// Boogie's keywords and built-in type names that are not keywords of Gardel.
const RESERVED: &[&str] = &[
    "old", "var", "procedure", "function", "axiom", "type", "const", "requires", "ensures",
    "modifies", "returns", "call", "havoc", "assert", "while", "lambda", "int", "bool", "real",
    "break", "goto", "return", "free", "where", "unique", "complete", "extends", "finite",
    "implementation", "div", "mod", "yield", "par", "async", "invariant", "assume", "uses",
    "true", "false", "reveal", "hide", "hideable", "pure",
];

/// The name a field takes where it is bound in the emitted Boogie, as a parameter of its
/// constructor and in the constructor's axioms: the field's own, or, for one that Boogie
/// reserves, such as `uses`, that name with `__f` after it, which no Gardel name holds (F.11d).
pub(crate) fn field_binder(name: &str) -> String {
    if is_reserved(name) {
        format!("{name}__f")
    } else {
        name.to_string()
    }
}

pub(crate) fn is_reserved(name: &str) -> bool {
    RESERVED.contains(&name)
}

/// Renames, in each routine of `m`, every bound name that Boogie reserves.
pub(crate) fn rename_reserved(m: &mut ModuleDecl) {
    let all = m
        .create_routines
        .iter_mut()
        .chain(m.feature_routines.iter_mut())
        .chain(m.specify_routines.iter_mut());
    for (_, r) in all {
        let mut bound = Vec::new();
        r.params.iter().for_each(|(p, _)| bound.push(p.clone()));
        collect_block(&r.stmts, &r.body, &mut bound);
        // A clause binds names too: `forall k: NAT. ...`.
        for (_, e) in r.requires.iter().chain(r.ensures.iter()) {
            collect(e, &mut bound);
        }
        r.variant.iter().for_each(|e| collect(e, &mut bound));
        let reserved: Vec<String> = {
            let mut v: Vec<String> = bound.iter().filter(|b| is_reserved(b)).cloned().collect();
            v.sort();
            v.dedup();
            v
        };
        if reserved.is_empty() {
            continue;
        }
        let mut used = bound.clone();
        for (_, e) in r.requires.iter().chain(r.ensures.iter()) {
            expr_free_vars(e, &mut Vec::new(), &mut used);
        }
        let mut names: Vec<(String, String)> = Vec::new();
        for b in reserved {
            let nb = fresh_binder(&b, &used, &r.stmts, &r.body);
            used.push(nb.clone());
            names.push((b, nb));
        }
        let to = |v: &str| names.iter().find(|(a, _)| a == v).map(|(_, b)| b.clone());
        let mut scope: Vec<String> = Vec::new();
        for (p, _) in r.params.iter_mut() {
            if let Some(n) = to(p) {
                scope.push(p.clone());
                *p = n;
            }
        }
        let rn = Renamer { to: &to };
        for (_, e) in r.requires.iter_mut() {
            rn.expr(e, &mut scope);
        }
        for e in r.variant.iter_mut() {
            rn.expr(e, &mut scope);
        }
        rn.block(&mut r.stmts, &mut r.body, &mut scope);
        for (_, e) in r.ensures.iter_mut() {
            rn.expr(e, &mut scope);
        }
    }
}

/// Every name bound in a block, at any depth.
fn collect_block(stmts: &[Stmt], body: &Expr, out: &mut Vec<String>) {
    for st in stmts {
        match st {
            Stmt::Let(v, e) => {
                out.push(v.clone());
                collect(e, out);
            }
            Stmt::Check(_, e) | Stmt::Assume(_, e) => collect(e, out),
            Stmt::Reveal(_) | Stmt::LetType(_, _) => {}
        }
    }
    collect(body, out);
}

fn collect(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Var(_) | Expr::Int(_) | Expr::Float(_) | Expr::Char(_) | Expr::Unit | Expr::Bool(_) | Expr::Str(_) => {}
        Expr::UnaryNot(x) | Expr::UnaryNeg(x) => collect(x, out),
        Expr::BinOp(_, l, r) => {
            collect(l, out);
            collect(r, out);
        }
        Expr::Call(_, args) => args.iter().for_each(|a| collect(a, out)),
        Expr::MethodCall(recv, _, args) => {
            collect(recv, out);
            args.iter().for_each(|a| collect(a, out));
        }
        Expr::If(c, ts, te, es, ee) => {
            collect(c, out);
            collect_block(ts, te, out);
            collect_block(es, ee, out);
        }
        Expr::Match(sc, cases) => {
            collect(sc, out);
            for c in cases {
                out.extend(c.bindings());
                collect_block(&c.stmts, &c.body, out);
            }
        }
        Expr::RecordLit(fs) => fs.iter().for_each(|(_, x)| collect(x, out)),
        Expr::RecordUpdate(b, fs) => {
            collect(b, out);
            fs.iter().for_each(|(_, x)| collect(x, out));
        }
        Expr::TupleLit(xs) => xs.iter().for_each(|x| collect(x, out)),
        Expr::Forall(v, _, b) | Expr::Exists(v, _, b) => {
            out.push(v.clone());
            collect(b, out);
        }
        Expr::WithIo(io, fin) => {
            for st in io {
                match st {
                    IoStmt::Bind(v, x) => {
                        out.push(v.clone());
                        collect(x, out);
                    }
                    IoStmt::Action(x) => collect(x, out),
                }
            }
            collect(fin, out);
        }
        Expr::Lambda(ps, _, stmts, body) => {
            out.extend(ps.iter().map(|(p, _)| p.clone()));
            collect_block(stmts, body, out);
        }
    }
}

struct Renamer<'a> {
    to: &'a dyn Fn(&str) -> Option<String>,
}

impl Renamer<'_> {
    /// The new name of `v` if a binding of it is in scope.
    fn name(&self, v: &str, scope: &[String]) -> Option<String> {
        if scope.iter().any(|s| s == v) { (self.to)(v) } else { None }
    }

    /// Renames the binder `v`, which comes into scope.
    fn bind(&self, v: &mut String, scope: &mut Vec<String>) {
        if let Some(n) = (self.to)(v) {
            scope.push(v.clone());
            *v = n;
        }
    }

    fn block(&self, stmts: &mut [Stmt], body: &mut Expr, scope: &mut Vec<String>) {
        let mark = scope.len();
        for st in stmts.iter_mut() {
            match st {
                Stmt::Let(v, e) => {
                    self.expr(e, scope);
                    self.bind(v, scope);
                }
                // The declared type of the `Let` that follows, under the same name.
                Stmt::LetType(v, _) => {
                    if let Some(n) = (self.to)(v) {
                        *v = n;
                    }
                }
                Stmt::Check(_, e) | Stmt::Assume(_, e) => self.expr(e, scope),
                Stmt::Reveal(_) => {}
            }
        }
        self.expr(body, scope);
        scope.truncate(mark);
    }

    fn expr(&self, e: &mut Expr, scope: &mut Vec<String>) {
        match e {
            Expr::Var(v) => {
                if let Some(n) = self.name(v, scope) {
                    *v = n;
                }
            }
            Expr::Int(_) | Expr::Float(_) | Expr::Char(_) | Expr::Unit | Expr::Bool(_) | Expr::Str(_) => {}
            Expr::UnaryNot(x) | Expr::UnaryNeg(x) => self.expr(x, scope),
            Expr::BinOp(_, l, r) => {
                self.expr(l, scope);
                self.expr(r, scope);
            }
            // A call of a local of function type, `h(r)`, names the local.
            Expr::Call(f, args) => {
                if let Some(n) = self.name(f, scope) {
                    *f = n;
                }
                args.iter_mut().for_each(|a| self.expr(a, scope));
            }
            Expr::MethodCall(recv, _, args) => {
                self.expr(recv, scope);
                args.iter_mut().for_each(|a| self.expr(a, scope));
            }
            Expr::If(c, ts, te, es, ee) => {
                self.expr(c, scope);
                self.block(ts, te, scope);
                self.block(es, ee, scope);
            }
            Expr::Match(sc, cases) => {
                self.expr(sc, scope);
                for c in cases.iter_mut() {
                    let mark = scope.len();
                    for b in c.bindings() {
                        if let Some(n) = (self.to)(&b) {
                            c.pattern = rename_pattern_binder(&c.pattern, &b, &n);
                            scope.push(b);
                        }
                    }
                    self.block(&mut c.stmts, &mut c.body, scope);
                    scope.truncate(mark);
                }
            }
            Expr::RecordLit(fs) => fs.iter_mut().for_each(|(_, x)| self.expr(x, scope)),
            Expr::RecordUpdate(b, fs) => {
                self.expr(b, scope);
                fs.iter_mut().for_each(|(_, x)| self.expr(x, scope));
            }
            Expr::TupleLit(xs) => xs.iter_mut().for_each(|x| self.expr(x, scope)),
            Expr::Forall(v, _, b) | Expr::Exists(v, _, b) => {
                let mark = scope.len();
                self.bind(v, scope);
                self.expr(b, scope);
                scope.truncate(mark);
            }
            Expr::WithIo(io, fin) => {
                let mark = scope.len();
                for st in io.iter_mut() {
                    match st {
                        IoStmt::Bind(v, x) => {
                            self.expr(x, scope);
                            self.bind(v, scope);
                        }
                        IoStmt::Action(x) => self.expr(x, scope),
                    }
                }
                self.expr(fin, scope);
                scope.truncate(mark);
            }
            Expr::Lambda(ps, _, stmts, body) => {
                let mark = scope.len();
                for (p, _) in ps.iter_mut() {
                    self.bind(p, scope);
                }
                self.block(stmts, body, scope);
                scope.truncate(mark);
            }
        }
    }
}
