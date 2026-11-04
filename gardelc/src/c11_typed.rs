//! Typed C11 backend: lowers Gardel modules and programs to C from their source.
//!
//! The backend infers the Gardel type of every expression, so it can
//!   * name every routine instance after its owner type (`SEQ_U64__concat`),
//!   * instantiate generic modules and routine-level type parameters on demand
//!     (`SEQ[A].fold_left[B]` at `A = U64, B = U64` becomes `SEQ_U64__fold_left__U64`),
//!   * lower constructors, `match` (flat patterns; `patterns.rs` compiles the others),
//!     records, tuples, function values and closures, and
//!   * provide a C runtime for the specification type `LIST[T]` where executable code
//!     uses it (e.g. `SEQ.to_list`, `SEQ.equals`).
//! Every module instance a program uses is emitted into the program's translation unit.
//! Independent `let` groups become OpenMP tasks (spec §10) with a depth cutoff.
//! `STRING`, `BYTES` and `IO` are provided by the C runtime header, not compiled.
//!
//! # Memory: reference counting
//! Every heap value (module values, `LIST` cells, closure environments, string buffers)
//! carries an atomic reference count. Gardel values are immutable and acyclic, so
//! counting frees everything, deterministically. The ownership discipline is:
//!   * a call *borrows* its arguments and returns an *owned* result; a callee that stores
//!     an argument (a list cell, a closure) takes its own reference;
//!   * a constructor (`make_*`, tuple, record) *consumes* the values it stores;
//!   * parameters, `match` binders, captured variables and `Current` are borrowed and never
//!     released by the code that uses them; `let` and IO-bound locals are owned and are
//!     released at the end of their block;
//!   * an owned temporary used where a borrow is expected is bound, used, then released.
//! Every lowered expression reports whether its C value is owned or borrowed
//! (`V::owned`), and the lowering inserts `dup_*` / `drop_*` calls accordingly. Drops of
//! recursive values loop on their last same-type field, so long lists are freed without
//! deep C recursion.
//!
//! Anything the backend cannot lower is reported, and the emitted C then starts with
//! `#error`, so it never compiles into something that silently differs from the source.

use super::*;
use std::collections::VecDeque;

/// A lowered expression: C code, Gardel type, and whether the code yields a reference
/// the consumer owns (and must release) or one borrowed from a longer-lived value.
#[derive(Clone)]
struct V {
    code: String,
    ty: String,
    owned: bool,
}

type Lowered = Result<V, String>;

fn owned(code: String, ty: impl Into<String>) -> V {
    V { code, ty: ty.into(), owned: true }
}

fn borrowed(code: String, ty: impl Into<String>) -> V {
    V { code, ty: ty.into(), owned: false }
}

/// Marker error: the expression needs an expected type (e.g. `LIST.empty`).
const NEED_TYPE: &str = "\u{0}need-type";

/// True for errors that mean "lowerable with more type information" (they start with
/// `NEED_TYPE`, optionally followed by the reason).
fn need(e: &str) -> bool {
    e.starts_with(NEED_TYPE)
}

/// Asks the block that declares a `let` of an unannotated number to lower it again at a
/// type found from a later use. Followed by `<variable>\0<type>`.
const RETYPE: &str = "\u{0}retype:";

fn retype(var: &str, ty: &str) -> String {
    format!("{RETYPE}{var}\u{0}{ty}")
}

fn retype_request(e: &str) -> Option<(&str, &str)> {
    e.strip_prefix(RETYPE)?.split_once('\u{0}')
}

/// Operators whose result has the type of their operands.
fn is_arith_op(op: &str) -> bool {
    matches!(op, "+" | "-" | "*" | "/" | "%" | "+%" | "-%" | "*%" | "&" | "|" | "^" | "<<" | ">>")
}

/// True when C would convert between `a` and `b` and change what the value means, while the
/// verifier treats both as mathematical numbers.
fn numeric_conflict(a: &str, b: &str) -> bool {
    let class = |t: &str| {
        if is_unsigned(t) {
            Some(0)
        } else if is_signed(t) {
            Some(1)
        } else if t == "F64" {
            Some(2)
        } else {
            None
        }
    };
    matches!((class(a), class(b)), (Some(x), Some(y)) if x != y)
}

const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else",
    "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long", "register",
    "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch",
    "typedef", "union", "unsigned", "void", "volatile", "while", "bool", "true", "false",
    "main", "exit", "abort", "free", "malloc", "printf", "Current", "NULL", "argc", "argv",
];

/// Canonical spelling of a Gardel type: `RESULT[IO_ERROR,STRING]` and
/// `RESULT[IO_ERROR, STRING]` agree, and function types drop parameter names
/// (`(acc: B, x: A) -> B` becomes `(B, A) -> B`).
pub(crate) fn norm_type(t: &str) -> String {
    norm_type_with(t, false)
}

/// `norm_type` for C: also maps the scalar aliases to one spelling (`NAT` and
/// `EXIT_CODE` are `U64`, `INT` is `I64`), since they are the same C type. Without this a
/// function value `(U64) -> U64` and a parameter `(NAT) -> NAT` would get two different
/// C function types.
fn cnorm(t: &str) -> String {
    norm_type_with(t, true)
}

fn norm_type_with(t: &str, alias: bool) -> String {
    let t = t.trim();
    if let Some((a, r)) = split_top_level_arrow(t) {
        let a = a.trim();
        let inner = a
            .strip_prefix('(')
            .and_then(|x| x.strip_suffix(')'))
            .unwrap_or(a);
        let ps: Vec<String> = split_top_level_commas(inner)
            .iter()
            .map(|p| {
                let p = p.trim();
                let ty = match p.split_once(':') {
                    Some((_, ty)) if !p.starts_with('(') => ty,
                    _ => p,
                };
                norm_type_with(ty, alias)
            })
            .collect();
        let r = r.split(" require ").next().unwrap_or(r);
        let r = r.split(" ensure ").next().unwrap_or(r);
        return format!("({}) -> {}", ps.join(", "), norm_type_with(r, alias));
    }
    let (h, args) = split_type_app(t);
    if args.is_empty() {
        match h {
            "NAT" | "EXIT_CODE" if alias => "U64".to_string(),
            "INT" if alias => "I64".to_string(),
            _ => h.to_string(),
        }
    } else {
        let a: Vec<String> = args.iter().map(|x| norm_type_with(x, alias)).collect();
        format!("{h}[{}]", a.join(", "))
    }
}

/// Parameter and result types of a (normalized) function type.
pub(crate) fn fn_parts(t: &str) -> Option<(Vec<String>, String)> {
    fn_parts_with(t, false)
}

fn cfn_parts(t: &str) -> Option<(Vec<String>, String)> {
    fn_parts_with(t, true)
}

fn fn_parts_with(t: &str, alias: bool) -> Option<(Vec<String>, String)> {
    let (a, r) = split_top_level_arrow(t)?;
    let a = a.trim();
    let inner = a
        .strip_prefix('(')
        .and_then(|x| x.strip_suffix(')'))
        .unwrap_or(a);
    let ps = split_top_level_commas(inner)
        .iter()
        .map(|p| norm_type_with(p, alias))
        .collect();
    Some((ps, norm_type_with(r, alias)))
}

fn subst(t: &str, map: &HashMap<String, String>) -> String {
    cnorm(&subst_type_idents(t, map))
}

fn mentions_any(t: &str, names: &[String]) -> bool {
    type_tokens(t).iter().any(|tok| names.iter().any(|n| n == tok))
}

/// Unifies `pattern` (which may mention the type parameters `tps`) with `actual`.
fn unify(pattern: &str, actual: &str, tps: &[String], map: &mut HashMap<String, String>) {
    let p = cnorm(pattern);
    let a = cnorm(actual);
    if tps.iter().any(|t| *t == p) {
        map.entry(p).or_insert(a);
        return;
    }
    if let (Some((pp, pr)), Some((ap, ar))) = (cfn_parts(&p), cfn_parts(&a)) {
        if pp.len() == ap.len() {
            for (x, y) in pp.iter().zip(ap.iter()) {
                unify(x, y, tps, map);
            }
            unify(&pr, &ar, tps, map);
        }
        return;
    }
    let (ph, pargs) = split_type_app(&p);
    let (ah, aargs) = split_type_app(&a);
    if ph == ah && pargs.len() == aargs.len() {
        for (x, y) in pargs.iter().zip(aargs.iter()) {
            unify(x, y, tps, map);
        }
    }
}

fn is_unsigned(t: &str) -> bool {
    matches!(t, "U64" | "NAT" | "EXIT_CODE")
}
fn is_signed(t: &str) -> bool {
    matches!(t, "I64" | "INT")
}
fn is_scalar(t: &str) -> bool {
    is_unsigned(t) || is_signed(t) || matches!(t, "BOOL" | "CHAR" | "F64" | "UNIT")
}

#[derive(Clone)]
struct Local {
    name: String,
    cname: String,
    ty: String,
    /// A `let` of an unannotated number: its type is only the default, so a use may change it.
    soft: bool,
}

#[derive(Clone)]
struct Ctx {
    /// Generic name of the module that owns the routine being lowered.
    owner: String,
    /// Concrete self type (`SEQ[U64]`) when the routine has `Current`.
    self_ty: Option<String>,
    /// Module and routine type parameters -> concrete types.
    tmap: HashMap<String, String>,
    locals: Vec<Local>,
    /// C names already used in the current C function.
    used: HashSet<String>,
    /// The next expression lowered is in tail position of the routine being emitted.
    tail: bool,
    /// The next `lower_call` is that tail expression.
    tail_site: bool,
    /// Owned values in scope at the current tail position, outermost first. A tail call
    /// releases them before it jumps, as the blocks it leaves would have.
    tail_drops: Vec<(String, String)>,
    /// C name of the routine being emitted (empty inside a lambda).
    fn_cname: String,
    /// The routine's C parameters, `Current` first.
    params: Vec<(String, String)>,
}

impl Ctx {
    fn local(&self, v: &str) -> Option<&Local> {
        self.locals.iter().rev().find(|l| l.name == v)
    }
    fn fresh(&mut self, base: &str) -> String {
        let mut c = if C_KEYWORDS.contains(&base) || base.starts_with('_') {
            format!("{base}_")
        } else {
            base.to_string()
        };
        let mut k = 1;
        while self.used.contains(&c) {
            c = format!("{base}_{k}");
            k += 1;
        }
        self.used.insert(c.clone());
        c
    }
    fn bind(&mut self, name: &str, ty: &str) -> String {
        let cname = self.fresh(name);
        self.locals.push(Local {
            name: name.to_string(),
            cname: cname.clone(),
            ty: ty.to_string(),
            soft: false,
        });
        cname
    }
    fn bind_let(&mut self, name: &str, ty: &str, soft: bool) -> String {
        let cname = self.bind(name, ty);
        if let Some(l) = self.locals.last_mut() {
            l.soft = soft;
        }
        cname
    }
    /// True for a number with no type of its own: literals, soft `let`s and arithmetic on them.
    fn untyped_num(&self, e: &Expr) -> bool {
        match e {
            Expr::Int(_) => true,
            Expr::UnaryNeg(x) => self.untyped_num(x),
            Expr::Var(v) => self.local(v).is_some_and(|l| l.soft),
            Expr::BinOp(op, l, r) => is_arith_op(op) && self.untyped_num(l) && self.untyped_num(r),
            Expr::If(_, ts, te, es, ee) => {
                ts.is_empty() && es.is_empty() && self.untyped_num(te) && self.untyped_num(ee)
            }
            _ => false,
        }
    }
    /// A soft `let` that decides the type of `e`, if any.
    fn soft_var(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Var(v) if self.local(v).is_some_and(|l| l.soft) => Some(v.clone()),
            Expr::UnaryNeg(x) => self.soft_var(x),
            Expr::BinOp(op, l, r) if is_arith_op(op) => self.soft_var(l).or_else(|| self.soft_var(r)),
            Expr::If(_, _, te, _, ee) => self.soft_var(te).or_else(|| self.soft_var(ee)),
            _ => None,
        }
    }
}

struct FnReq {
    module: String,
    mod_targs: Vec<String>,
    routine: String,
    rtargs: Vec<String>,
    cname: String,
}

/// Statements to run before an expression (binding owned temporaries) and after it
/// (releasing them).
#[derive(Default)]
struct Temps {
    pre: String,
    post: String,
}

pub(crate) struct TypedC<'a> {
    mods: Vec<&'a ModuleDecl>,
    root: &'a ModuleDecl,
    struct_fwd: Vec<String>,
    fn_typedefs: Vec<String>,
    type_defs: Vec<String>,
    rc_protos: Vec<String>,
    protos: Vec<String>,
    bodies: Vec<String>,
    done_types: HashSet<String>,
    done_fns: HashSet<String>,
    done_helpers: HashSet<String>,
    typedefs: HashMap<String, String>,
    queue: VecDeque<FnReq>,
    counter: usize,
    needs_validator: bool,
    needs_read_line: bool,
    /// The routine being emitted jumped to its own start (spec §10: tail calls are loops).
    tail_used: bool,
    needs_from_utf8: bool,
    pub(crate) errors: Vec<String>,
}

impl<'a> TypedC<'a> {
    fn new(root: &'a ModuleDecl, extra: &[&'a ModuleDecl]) -> Self {
        let mut mods: Vec<&ModuleDecl> = vec![root];
        for m in extra {
            if !mods.iter().any(|x| x.name == m.name) {
                mods.push(m);
            }
        }
        for m in &standard_prelude().modules {
            if !mods.iter().any(|x| x.name == m.name) {
                mods.push(m);
            }
        }
        TypedC {
            mods,
            root,
            struct_fwd: Vec::new(),
            fn_typedefs: Vec::new(),
            type_defs: Vec::new(),
            rc_protos: Vec::new(),
            protos: Vec::new(),
            bodies: Vec::new(),
            done_types: HashSet::new(),
            done_fns: HashSet::new(),
            done_helpers: HashSet::new(),
            typedefs: HashMap::new(),
            queue: VecDeque::new(),
            counter: 0,
            needs_validator: false,
            needs_read_line: false,
            tail_used: false,
            needs_from_utf8: false,
            errors: Vec::new(),
        }
    }

    fn module(&self, name: &str) -> Option<&'a ModuleDecl> {
        self.mods.iter().copied().find(|m| m.name == name)
    }

    fn next(&mut self) -> usize {
        self.counter += 1;
        self.counter
    }

    // ------------------------------------------------------------------ types

    /// C type of a (concrete, normalized) Gardel type, emitting its definition on demand.
    fn ctype(&mut self, ty: &str) -> Result<String, String> {
        let ty = cnorm(ty);
        Ok(match ty.as_str() {
            "U64" | "NAT" | "EXIT_CODE" => "uint64_t".into(),
            "I64" | "INT" => "int64_t".into(),
            "BOOL" => "bool".into(),
            "CHAR" => "uint32_t".into(),
            "F64" => "double".into(),
            "UNIT" => "GARDEL_UNIT".into(),
            "STRING" => "GardelString".into(),
            "BYTES" => "GardelBytes".into(),
            "MATH_INT" => return Err("`MATH_INT` is a specification type with no C representation".into()),
            t if t.starts_with("IO[") => "int64_t".into(),
            t if cfn_parts(t).is_some() => self.fn_typedef(t)?,
            t if type_head(t) == "TUPLE" => {
                self.ensure_tuple(t)?;
                format!("struct {}", c_instance_name(t))
            }
            t if type_head(t) == "LIST" => {
                let elem = split_type_app(t).1.first().map(|s| cnorm(s)).unwrap_or_default();
                self.ensure_list(&elem)?;
                format!("struct {}*", c_instance_name(t))
            }
            t => {
                self.ensure_adt(t)?;
                format!("struct {}*", c_instance_name(t))
            }
        })
    }

    /// True when values of `ty` hold counted references.
    fn is_rc(&mut self, ty: &str) -> bool {
        let ty = cnorm(ty);
        if is_scalar(&ty) || ty.starts_with("IO[") {
            return false;
        }
        if let Some((a, b)) = Self::tuple_parts(&ty) {
            return self.is_rc(&a) || self.is_rc(&b);
        }
        true
    }

    /// `dup_*` applied to `code` (a borrowed value of type `ty`), yielding an owned one.
    fn dup(&mut self, ty: &str, code: &str) -> Result<String, String> {
        if !self.is_rc(ty) {
            return Ok(code.to_string());
        }
        Ok(format!("{}({code})", self.rc_name(ty, "dup")?))
    }

    /// Statement releasing the owned value `code` of type `ty` (empty for plain values).
    fn drop_stmt(&mut self, ty: &str, code: &str) -> Result<String, String> {
        if !self.is_rc(ty) {
            return Ok(String::new());
        }
        Ok(format!("{}({code});\n", self.rc_name(ty, "drop")?))
    }

    fn rc_name(&mut self, ty: &str, which: &str) -> Result<String, String> {
        let ty = cnorm(ty);
        Ok(match ty.as_str() {
            "STRING" => format!("gardel_string_{which}"),
            "BYTES" => format!("gardel_bytes_{which}"),
            _ => {
                let ct = self.ctype(&ty)?;
                let inst = if cfn_parts(&ty).is_some() { ct } else { c_instance_name(&ty) };
                format!("{which}_{inst}")
            }
        })
    }

    /// An owned version of `v` (adds a reference when `v` is borrowed).
    fn own(&mut self, v: V) -> Result<String, String> {
        if v.owned {
            Ok(v.code)
        } else {
            self.dup(&v.ty, &v.code)
        }
    }

    /// A borrowable C expression for `v`: an owned counted value is bound to a temporary
    /// in `t.pre` and released in `t.post`.
    fn borrow(&mut self, v: V, t: &mut Temps) -> Result<String, String> {
        if !v.owned || !self.is_rc(&v.ty) {
            return Ok(v.code);
        }
        let k = self.next();
        let ct = self.ctype(&v.ty)?;
        t.pre.push_str(&format!("{ct} _t{k} = {};\n", v.code));
        let d = self.drop_stmt(&v.ty, &format!("_t{k}"))?;
        t.post = format!("{d}{}", t.post);
        Ok(format!("_t{k}"))
    }

    /// `expr` (of type `ty`) evaluated between the temporaries' setup and release.
    fn with_temps(&mut self, t: Temps, expr: String, ty: &str) -> Result<String, String> {
        if t.pre.is_empty() {
            return Ok(expr);
        }
        let k = self.next();
        let ct = self.ctype(ty)?;
        Ok(format!("({{\n{}{ct} _r{k} = {expr};\n{}_r{k};\n}})", t.pre, t.post))
    }

    fn fn_typedef(&mut self, t: &str) -> Result<String, String> {
        if let Some(n) = self.typedefs.get(t) {
            return Ok(n.clone());
        }
        let (ps, r) = cfn_parts(t).ok_or_else(|| format!("bad function type `{t}`"))?;
        let name = format!("gfn_{}", c_instance_name(&format!("{}_to_{r}", ps.join("_"))));
        self.typedefs.insert(t.to_string(), name.clone());
        let mut cps = vec!["struct gardel_env*".to_string()];
        for p in &ps {
            cps.push(self.ctype(p)?);
        }
        let cr = self.ctype(&r)?;
        // A function value is a code pointer plus a counted environment (NULL when the
        // function captures nothing).
        self.fn_typedefs.push(format!(
            "typedef struct {{ {cr} (*fn)({}); struct gardel_env* env; }} {name};\n\
             static inline {name} dup_{name}({name} f) {{ gardel_env_dup(f.env); return f; }}\n\
             static inline void drop_{name}({name} f) {{ gardel_env_drop(f.env); }}\n",
            cps.join(", ")
        ));
        Ok(name)
    }

    /// Struct definition, constructors and reference counting of a module instance.
    fn ensure_adt(&mut self, ty: &str) -> Result<(), String> {
        let inst = c_instance_name(ty);
        if self.done_types.contains(&inst) {
            return Ok(());
        }
        let (head, args) = split_type_app(ty);
        let md = self
            .module(head)
            .ok_or_else(|| format!("type `{ty}` has no C representation"))?;
        if md.kind != ModuleKind::Module {
            return Err(format!("`{head}` is not a data type"));
        }
        if md.type_params.len() != args.len() {
            return Err(format!("type `{ty}` has the wrong number of type arguments"));
        }
        self.done_types.insert(inst.clone());
        self.struct_fwd.push(format!("struct {inst};\n"));
        let self_ty = cnorm(ty);
        let mut def = String::new();
        // (variant or None for a record, fields: name, Gardel type, C type)
        let mut shapes: Vec<(Option<String>, Vec<(String, String, String)>)> = Vec::new();
        match &md.repr {
            ReprKind::Product(_) => {
                let fs = self.variant_fields(&self_ty, None).unwrap_or_default();
                let mut cf = Vec::new();
                for (f, t) in fs {
                    let c = self.ctype(&t)?;
                    cf.push((f, t, c));
                }
                shapes.push((None, cf));
            }
            ReprKind::Sum(vs) => {
                for v in vs {
                    let fs = self.variant_fields(&self_ty, Some(&v.name)).unwrap_or_default();
                    let mut cf = Vec::new();
                    for (f, t) in fs {
                        let c = self.ctype(&t)?;
                        cf.push((f, t, c));
                    }
                    shapes.push((Some(v.name.clone()), cf));
                }
            }
        }
        def.push_str(&format!("struct {inst} {{\n    _Atomic uint64_t _rc;\n"));
        let is_sum = matches!(md.repr, ReprKind::Sum(_));
        if is_sum {
            let tags: Vec<String> = shapes
                .iter()
                .map(|(v, _)| format!("TAG_{inst}_{}", v.as_deref().unwrap_or("")))
                .collect();
            def.push_str(&format!("    enum {{ {} }} tag;\n", tags.join(", ")));
            if shapes.iter().any(|(_, f)| !f.is_empty()) {
                def.push_str("    union {\n");
                for (v, cf) in &shapes {
                    if !cf.is_empty() {
                        def.push_str("        struct {");
                        for (f, _, c) in cf {
                            def.push_str(&format!(" {c} {f};"));
                        }
                        def.push_str(&format!(" }} {};\n", v.as_deref().unwrap_or("")));
                    }
                }
                def.push_str("    };\n");
            }
        } else {
            for (f, _, c) in &shapes[0].1 {
                def.push_str(&format!("    {c} {f};\n"));
            }
        }
        def.push_str("};\n");
        // Constructors consume the values they store.
        for (v, cf) in &shapes {
            let params: Vec<String> = cf.iter().map(|(f, _, c)| format!("{c} {f}")).collect();
            let fname = match v {
                Some(v) => format!("make_{inst}_{v}"),
                None => format!("make_{inst}"),
            };
            def.push_str(&format!(
                "static inline struct {inst}* {fname}({}) {{\n    struct {inst}* r = (struct {inst}*)gardel_alloc(sizeof(struct {inst}));\n    atomic_init(&r->_rc, 1);\n",
                if params.is_empty() { "void".to_string() } else { params.join(", ") }
            ));
            if let Some(v) = v {
                def.push_str(&format!("    r->tag = TAG_{inst}_{v};\n"));
            }
            for (f, _, _) in cf {
                match v {
                    Some(v) => def.push_str(&format!("    r->{v}.{f} = {f};\n")),
                    None => def.push_str(&format!("    r->{f} = {f};\n")),
                }
            }
            def.push_str("    return r;\n}\n");
        }
        def.push_str(&format!(
            "static inline struct {inst}* dup_{inst}(struct {inst}* p) {{\n    atomic_fetch_add_explicit(&p->_rc, 1, memory_order_relaxed);\n    return p;\n}}\n\n"
        ));
        self.type_defs.push(def);
        // Release: children first, looping (not recursing) on the last field of the same
        // type, so a long chain is freed in constant C stack.
        let mut cases = String::new();
        for (v, cf) in &shapes {
            let acc = |f: &str| match v {
                Some(v) => format!("p->{v}.{f}"),
                None => format!("p->{f}"),
            };
            let loop_field = cf.iter().rposition(|(_, t, _)| cnorm(t) == self_ty);
            let mut body = String::new();
            for (i, (f, t, _)) in cf.iter().enumerate() {
                if Some(i) == loop_field {
                    body.push_str(&format!("next = {};\n", acc(f)));
                } else {
                    body.push_str(&self.drop_stmt(t, &acc(f))?);
                }
            }
            match v {
                Some(v) => cases.push_str(&format!("        case TAG_{inst}_{v}: {{\n{body}        break; }}\n")),
                None => cases.push_str(&body),
            }
        }
        let inner = if is_sum {
            format!("        switch (p->tag) {{\n{cases}        }}\n")
        } else {
            cases
        };
        self.rc_protos.push(format!("static void drop_{inst}(struct {inst}* p);\n"));
        self.bodies.push(format!(
            "static void drop_{inst}(struct {inst}* p) {{\n    while (p && atomic_fetch_sub_explicit(&p->_rc, 1, memory_order_acq_rel) == 1) {{\n        struct {inst}* next = NULL;\n{inner}        gardel_free(p);\n        p = next;\n    }}\n}}\n\n"
        ));
        Ok(())
    }

    /// `TUPLE[A, B]` as a C value struct `{ first; second; }` (no allocation).
    fn ensure_tuple(&mut self, ty: &str) -> Result<(), String> {
        let inst = c_instance_name(ty);
        if self.done_types.contains(&inst) {
            return Ok(());
        }
        let (pa, pb) = Self::tuple_parts(ty).ok_or_else(|| format!("`{ty}`: only pairs are supported"))?;
        let a = self.ctype(&pa)?;
        let b = self.ctype(&pb)?;
        if !self.done_types.insert(inst.clone()) {
            return Ok(());
        }
        self.type_defs.push(format!("struct {inst} {{ {a} first; {b} second; }};\n\n"));
        let da = self.dup(&pa, "t.first")?;
        let db = self.dup(&pb, "t.second")?;
        let xa = self.drop_stmt(&pa, "t.first")?;
        let xb = self.drop_stmt(&pb, "t.second")?;
        self.rc_protos.push(format!(
            "static inline struct {inst} dup_{inst}(struct {inst} t);\nstatic inline void drop_{inst}(struct {inst} t);\n"
        ));
        self.bodies.push(format!(
            "static inline struct {inst} dup_{inst}(struct {inst} t) {{ return (struct {inst}){{ {da}, {db} }}; }}\n\
             static inline void drop_{inst}(struct {inst} t) {{ {xa}{xb}(void)t; }}\n\n"
        ));
        Ok(())
    }

    fn tuple_parts(ty: &str) -> Option<(String, String)> {
        if type_head(ty) != "TUPLE" {
            return None;
        }
        let parts: Vec<String> = split_type_app(ty).1.iter().map(|a| cnorm(a)).collect();
        (parts.len() == 2).then(|| (parts[0].clone(), parts[1].clone()))
    }

    /// Fields of a module instance's variant (or of its product) with concrete types.
    fn variant_fields(&self, ty: &str, variant: Option<&str>) -> Option<Vec<(String, String)>> {
        let (head, args) = split_type_app(ty);
        let md = self.module(head)?;
        let map: HashMap<String, String> = md
            .type_params
            .iter()
            .cloned()
            .zip(args.iter().map(|a| cnorm(a)))
            .collect();
        let self_ty = cnorm(ty);
        let fix = |t: &str| -> String {
            let t = if t.trim() == "Current" { self_ty.clone() } else { t.to_string() };
            subst(&t, &map)
        };
        match (&md.repr, variant) {
            (ReprKind::Sum(vs), Some(v)) => {
                let vd = vs.iter().find(|x| x.name == v)?;
                Some(vd.fields.iter().map(|(f, t)| (f.clone(), fix(t))).collect())
            }
            (ReprKind::Product(fs), None) => Some(fs.iter().map(|(f, t)| (f.clone(), fix(t))).collect()),
            _ => None,
        }
    }

    /// `LIST[T]` runtime: an immutable, counted cons list (`NULL` is the empty list).
    /// The functions borrow their arguments and take their own references to what they
    /// store; they are iterative, so long lists do not exhaust the C stack.
    fn ensure_list(&mut self, elem: &str) -> Result<(), String> {
        let lt = format!("LIST[{elem}]");
        let inst = c_instance_name(&lt);
        if !self.done_types.insert(inst.clone()) {
            return Ok(());
        }
        let ce = self.ctype(elem)?;
        self.struct_fwd.push(format!("struct {inst};\n"));
        self.type_defs.push(format!(
            "struct {inst} {{ _Atomic uint64_t _rc; {ce} head; struct {inst}* tail; }};\n\
             static inline struct {inst}* dup_{inst}(struct {inst}* p) {{\n    if (p) atomic_fetch_add_explicit(&p->_rc, 1, memory_order_relaxed);\n    return p;\n}}\n\n"
        ));
        let eq = self.eq_code(elem, "a->head", "b->head")?;
        let eqx = self.eq_code(elem, "xs->head", "x")?;
        let dup_x = self.dup(elem, "x")?;
        let dup_h = self.dup(elem, "xs->head")?;
        let drop_h = self.drop_stmt(elem, "p->head")?;
        self.rc_protos.push(format!("static void drop_{inst}(struct {inst}* p);\n"));
        self.protos.push(format!(
            "static struct {inst}* {inst}__prepend(struct {inst}* xs, {ce} x);\n\
             static struct {inst}* {inst}__append(struct {inst}* xs, struct {inst}* ys);\n\
             static uint64_t {inst}__length(struct {inst}* xs);\n\
             static bool {inst}__eq(struct {inst}* a, struct {inst}* b);\n\
             static bool {inst}__has(struct {inst}* xs, {ce} x);\n"
        ));
        self.bodies.push(format!(
            "/* LIST[{elem}] runtime: immutable counted cons list, NULL is the empty list */\n\
static void drop_{inst}(struct {inst}* p) {{\n\
    while (p && atomic_fetch_sub_explicit(&p->_rc, 1, memory_order_acq_rel) == 1) {{\n\
        struct {inst}* next = p->tail;\n        {drop_h}        gardel_free(p);\n        p = next;\n    }}\n}}\n\
static struct {inst}* {inst}__prepend(struct {inst}* xs, {ce} x) {{\n\
    struct {inst}* c = (struct {inst}*)gardel_alloc(sizeof(struct {inst}));\n\
    atomic_init(&c->_rc, 1);\n    c->head = {dup_x};\n    c->tail = dup_{inst}(xs);\n    return c;\n}}\n\
static struct {inst}* {inst}__append(struct {inst}* xs, struct {inst}* ys) {{\n\
    struct {inst}* out = NULL;\n    struct {inst}** at = &out;\n\
    for (; xs; xs = xs->tail) {{\n\
        struct {inst}* c = (struct {inst}*)gardel_alloc(sizeof(struct {inst}));\n\
        atomic_init(&c->_rc, 1);\n        c->head = {dup_h};\n        c->tail = NULL;\n        *at = c;\n        at = &c->tail;\n    }}\n\
    *at = dup_{inst}(ys);\n    return out;\n}}\n\
static uint64_t {inst}__length(struct {inst}* xs) {{\n\
    uint64_t n = 0;\n    for (; xs; xs = xs->tail) n++;\n    return n;\n}}\n\
static bool {inst}__eq(struct {inst}* a, struct {inst}* b) {{\n\
    for (; a && b; a = a->tail, b = b->tail)\n        if (!({eq})) return false;\n\
    return a == NULL && b == NULL;\n}}\n\
static bool {inst}__has(struct {inst}* xs, {ce} x) {{\n\
    for (; xs; xs = xs->tail)\n        if ({eqx}) return true;\n    return false;\n}}\n\n"
        ));
        Ok(())
    }

    /// C expression testing `a = b` at type `ty` (structural, Leibniz equality) on two
    /// borrowed values.
    fn eq_code(&mut self, ty: &str, a: &str, b: &str) -> Result<String, String> {
        let ty = cnorm(ty);
        if is_scalar(&ty) {
            return Ok(format!("({a} == {b})"));
        }
        match ty.as_str() {
            "STRING" => return Ok(format!("gardel_string_equals({a}, {b})")),
            "BYTES" => return Ok(format!("gardel_bytes_equals({a}, {b})")),
            _ => {}
        }
        if cfn_parts(&ty).is_some() {
            return Err(format!("equality on function type `{ty}` cannot be executed"));
        }
        if type_head(&ty) == "LIST" {
            self.ctype(&ty)?;
            return Ok(format!("{}__eq({a}, {b})", c_instance_name(&ty)));
        }
        if let Some((ta, tb)) = Self::tuple_parts(&ty) {
            let x = self.eq_code(&ta, &format!("({a}).first"), &format!("({b}).first"))?;
            let y = self.eq_code(&tb, &format!("({a}).second"), &format!("({b}).second"))?;
            return Ok(format!("({x} && {y})"));
        }
        let ct = self.ctype(&ty)?;
        let inst = c_instance_name(&ty);
        let fname = format!("{inst}__struct_eq");
        if self.done_helpers.insert(fname.clone()) {
            let md = self
                .module(type_head(&ty))
                .ok_or_else(|| format!("no equality for `{ty}`"))?;
            let mut body = String::new();
            match &md.repr {
                ReprKind::Product(_) => {
                    let fs = self.variant_fields(&ty, None).unwrap_or_default();
                    let mut parts = vec!["true".to_string()];
                    for (f, ft) in fs {
                        parts.push(self.eq_code(&ft, &format!("a->{f}"), &format!("b->{f}"))?);
                    }
                    body.push_str(&format!("    return {};\n", parts.join(" && ")));
                }
                ReprKind::Sum(vs) => {
                    body.push_str("    if (a->tag != b->tag) return false;\n    switch (a->tag) {\n");
                    for v in vs {
                        let fs = self.variant_fields(&ty, Some(&v.name)).unwrap_or_default();
                        let mut parts = vec!["true".to_string()];
                        for (f, ft) in fs {
                            parts.push(self.eq_code(
                                &ft,
                                &format!("a->{}.{f}", v.name),
                                &format!("b->{}.{f}", v.name),
                            )?);
                        }
                        body.push_str(&format!(
                            "    case TAG_{inst}_{}: return {};\n",
                            v.name,
                            parts.join(" && ")
                        ));
                    }
                    body.push_str("    }\n    return false;\n");
                }
            }
            self.protos.push(format!("static bool {fname}({ct} a, {ct} b);\n"));
            self.bodies.push(format!("static bool {fname}({ct} a, {ct} b) {{\n{body}}}\n\n"));
        }
        Ok(format!("{fname}({a}, {b})"))
    }

    // --------------------------------------------------------------- routines

    fn find_routine(md: &'a ModuleDecl, r: &str) -> Option<(&'a RoutineDecl, &'static str)> {
        if let Some((_, rd)) = md.create_routines.iter().find(|(_, x)| x.name == r) {
            return Some((rd, "create"));
        }
        if let Some((_, rd)) = md.feature_routines.iter().find(|(_, x)| x.name == r) {
            return Some((rd, "feature"));
        }
        if let Some((_, rd)) = md.specify_routines.iter().find(|(_, x)| x.name == r) {
            return Some((rd, "specify"));
        }
        None
    }

    fn has_current(md: &ModuleDecl, kind: &str) -> bool {
        md.kind == ModuleKind::Module && kind == "feature"
    }

    fn self_type(md: &ModuleDecl, targs: &[String]) -> String {
        if targs.is_empty() {
            md.name.clone()
        } else {
            format!("{}[{}]", md.name, targs.join(", "))
        }
    }

    fn request_fn(&mut self, module: &str, mod_targs: &[String], routine: &str, rtargs: &[String]) -> String {
        let mut cname = format!("{}__{routine}", c_instance_name(&Self::self_type(
            self.module(module).expect("module exists"),
            mod_targs,
        )));
        if !rtargs.is_empty() {
            cname.push_str(&format!("__{}", c_instance_name(&rtargs.join("_"))));
        }
        if self.done_fns.insert(cname.clone()) {
            self.queue.push_back(FnReq {
                module: module.to_string(),
                mod_targs: mod_targs.to_vec(),
                routine: routine.to_string(),
                rtargs: rtargs.to_vec(),
                cname: cname.clone(),
            });
        }
        cname
    }

    fn emit_fn(&mut self, req: FnReq) -> Result<(), String> {
        let md = self.module(&req.module).ok_or("unknown module")?;
        let (rd, kind) = Self::find_routine(md, &req.routine).ok_or("unknown routine")?;
        if kind == "specify" {
            return Err(format!(
                "`{}.{}` is a specification (ghost) routine and has no executable form",
                md.name, rd.name
            ));
        }
        let mut tmap: HashMap<String, String> = md
            .type_params
            .iter()
            .cloned()
            .zip(req.mod_targs.iter().cloned())
            .collect();
        tmap.extend(rd.type_params.iter().cloned().zip(req.rtargs.iter().cloned()));
        let self_ty = Self::self_type(md, &req.mod_targs);
        tmap.insert("Current".into(), self_ty.clone());
        let has_cur = Self::has_current(md, kind);
        let mut cx = Ctx {
            owner: md.name.clone(),
            self_ty: if md.kind == ModuleKind::Module { Some(self_ty.clone()) } else { None },
            tmap: tmap.clone(),
            locals: Vec::new(),
            used: HashSet::new(),
            tail: true,
            tail_site: false,
            tail_drops: Vec::new(),
            fn_cname: req.cname.clone(),
            params: Vec::new(),
        };
        self.tail_used = false;
        cx.used.insert("Current".into());
        let mut params = Vec::new();
        if has_cur {
            params.push(format!("{} Current", self.ctype(&self_ty)?));
            cx.params.push(("Current".to_string(), self_ty.clone()));
        }
        for (p, t) in &rd.params {
            let ty = subst(t, &tmap);
            let ct = self.ctype(&ty)?;
            let cn = cx.bind(p, &ty);
            params.push(format!("{ct} {cn}"));
            cx.params.push((cn, ty));
        }
        let ret = subst(&rd.ret_type, &tmap);
        let cret = self.ctype(&ret)?;
        let body = self.lower_block(&mut cx, &rd.stmts, &rd.body, Some(&ret), has_cur)?;
        let body = self.own(body)?;
        let sig = format!(
            "static {cret} {}({})",
            req.cname,
            if params.is_empty() { "void".to_string() } else { params.join(", ") }
        );
        self.protos.push(format!("{sig};\n"));
        let code = if self.tail_used {
            // Parameters are borrowed on the first iteration and owned after a tail call.
            let drops = self.param_drops(&cx)?;
            format!("    bool _owned = false;\n_tail:;\n    {cret} _res = {body};\n    if (_owned) {{\n{drops}}}\n    return _res;\n")
        } else {
            format!("    return {body};\n")
        };
        self.bodies.push(format!(
            "/* {}.{}{} */\n{sig} {{\n{code}}}\n\n",
            self_ty,
            rd.name,
            if req.rtargs.is_empty() { String::new() } else { format!("[{}]", req.rtargs.join(", ")) }
        ));
        Ok(())
    }

    fn param_drops(&mut self, cx: &Ctx) -> Result<String, String> {
        let mut drops = String::new();
        for (cn, ty) in &cx.params {
            drops.push_str(&self.drop_stmt(ty, cn)?);
        }
        Ok(drops)
    }

    /// A self call in tail position (spec §10): no stack frame. The new arguments become
    /// owned first, since they may borrow from what is released next. Then the scopes the
    /// call leaves are released, then the previous iteration's parameters.
    fn tail_jump(&mut self, cx: &Ctx, args: Vec<V>, ret: &str) -> Result<String, String> {
        if args.len() != cx.params.len() {
            return Err("tail call does not match the routine's parameters".into());
        }
        let k = self.next();
        let mut code = String::new();
        let mut assign = String::new();
        for (i, (v, (cn, ty))) in args.into_iter().zip(&cx.params).enumerate() {
            let c = self.own(v)?;
            code.push_str(&format!("{} _t{k}_{i} = {c};\n", self.ctype(ty)?));
            assign.push_str(&format!("{cn} = _t{k}_{i};\n"));
        }
        for (cn, ty) in cx.tail_drops.iter().rev() {
            code.push_str(&self.drop_stmt(ty, cn)?);
        }
        let drops = self.param_drops(cx)?;
        let cret = self.ctype(ret)?;
        self.tail_used = true;
        Ok(format!(
            "({{\n{code}if (_owned) {{\n{drops}}}\n{assign}_owned = true;\ngoto _tail;\n{cret} _u{k};\n_u{k};\n}})"
        ))
    }

    fn drain(&mut self) {
        while let Some(req) = self.queue.pop_front() {
            let what = format!("{}.{}", req.module, req.routine);
            if let Err(e) = self.emit_fn(req) {
                if let Some((v, t)) = retype_request(&e) {
                    self.errors.push(format!("{what}: `{v}` is used as `{t}` but cannot be typed so here"));
                    continue;
                }
                let msg = e.strip_prefix(NEED_TYPE).unwrap_or(&e);
                let msg = if msg.is_empty() { "cannot infer a type" } else { msg };
                self.errors.push(format!("{what}: {msg}"));
            }
        }
    }

    // ------------------------------------------------------------ expressions

    /// A block: its `let`s are owned and released after the (owned) result is computed.
    /// `let zero = 0` has no type of its own, so a use such as `x < zero` on an `I64` restarts
    /// the block with `zero` at that type. Each `let` changes type at most once.
    fn lower_block(
        &mut self,
        cx: &mut Ctx,
        stmts: &[Stmt],
        body: &Expr,
        expect: Option<&str>,
        has_cur: bool,
    ) -> Lowered {
        let mark = cx.locals.len();
        let tail = cx.tail;
        let depth = cx.tail_drops.len();
        let mut retyped: HashMap<String, String> = HashMap::new();
        loop {
            cx.tail = tail;
            let res = self.lower_block_once(cx, stmts, body, expect, has_cur, &retyped);
            cx.tail_drops.truncate(depth);
            let Err(e) = &res else { return res };
            let Some((v, t)) = retype_request(e) else { return res };
            let declared = stmts.iter().any(|s| matches!(s, Stmt::Let(n, _) if n == v));
            if !declared || retyped.contains_key(v) {
                return res;
            }
            retyped.insert(v.to_string(), t.to_string());
            cx.locals.truncate(mark);
        }
    }

    fn lower_block_once(
        &mut self,
        cx: &mut Ctx,
        stmts: &[Stmt],
        body: &Expr,
        expect: Option<&str>,
        has_cur: bool,
        retyped: &HashMap<String, String>,
    ) -> Lowered {
        let tail = std::mem::take(&mut cx.tail);
        let mark = cx.locals.len();
        let mut code = String::new();
        let mut owned_locals: Vec<(String, String)> = Vec::new();
        for group in analyze_parallel_lets(stmts) {
            if group.len() == 1 {
                let (v, e) = &group[0];
                let soft = !retyped.contains_key(v) && cx.untyped_num(e);
                let x = self.lower(cx, e, retyped.get(v).map(String::as_str), has_cur)?;
                let t = x.ty.clone();
                let c = self.own(x)?;
                let ct = self.ctype(&t)?;
                let cn = cx.bind_let(v, &t, soft);
                code.push_str(&format!("{ct} {cn} = {c};\n"));
                owned_locals.push((cn, t));
            } else {
                let mut lowered = Vec::new();
                let mut soft = Vec::new();
                for (v, e) in &group {
                    soft.push(!retyped.contains_key(v) && cx.untyped_num(e));
                    let x = self.lower(cx, e, retyped.get(v).map(String::as_str), has_cur)?;
                    let t = x.ty.clone();
                    let c = self.own(x)?;
                    let ct = self.ctype(&t)?;
                    lowered.push((v.clone(), c, t, ct));
                }
                let mut names = Vec::new();
                for ((v, _, t, ct), soft) in lowered.iter().zip(soft) {
                    let cn = cx.bind_let(v, t, soft);
                    code.push_str(&format!("{ct} {cn};\n"));
                    owned_locals.push((cn.clone(), t.clone()));
                    names.push(cn);
                }
                // Fork-join (spec §10). Every branch of the group, including the one the
                // current thread runs inline, executes one level deeper, so the depth
                // cutoff bounds the number of tasks (about 2^GARDEL_TASK_DEPTH for a binary
                // recursion). A task restores the depth of the thread that ran it, since
                // a thread may run it while it waits in an unrelated `taskwait`.
                let last = lowered.len() - 1;
                let mut forked = String::new();
                for (i, ((_, c, _, _), cn)) in lowered.iter().zip(names.iter()).enumerate() {
                    if i != last {
                        forked.push_str(&format!("#pragma omp task shared({cn}) firstprivate(_pd)\n"));
                    }
                    forked.push_str(&format!(
                        "{{ int _s = gardel_task_depth; gardel_task_depth = _pd; {cn} = {c}; gardel_task_depth = _s; }}\n"
                    ));
                }
                forked.push_str("#pragma omp taskwait\n");
                let small = lowered.iter().map(|(_, c, _, _)| c.len()).sum::<usize>() <= 4096;
                code.push_str("{\nint _pd = gardel_task_depth + 1;\n");
                if small {
                    // Past the cutoff, run the group sequentially without calling the
                    // OpenMP runtime at all (the expressions appear twice in the C).
                    code.push_str(&format!("if (_pd <= GARDEL_TASK_DEPTH) {{\n{forked}}} else {{\n"));
                    for ((_, c, _, _), cn) in lowered.iter().zip(names.iter()) {
                        code.push_str(&format!("{cn} = {c};\n"));
                    }
                    code.push_str("}\n");
                } else {
                    // Large expressions are not duplicated: undeferred tasks past the cutoff.
                    code.push_str(&forked.replacen(
                        "firstprivate(_pd)\n",
                        "firstprivate(_pd) if(_pd <= GARDEL_TASK_DEPTH)\n",
                        usize::MAX,
                    ));
                }
                code.push_str("}\n");
            }
        }
        if tail {
            cx.tail = true;
            cx.tail_drops.extend(owned_locals.iter().cloned());
        }
        let b = self.lower(cx, body, expect, has_cur)?;
        cx.locals.truncate(mark);
        if code.is_empty() {
            return Ok(b);
        }
        let t = b.ty.clone();
        let bc = self.own(b)?;
        let mut drops = String::new();
        for (cn, lt) in owned_locals.iter().rev() {
            drops.push_str(&self.drop_stmt(lt, cn)?);
        }
        let k = self.next();
        let ct = self.ctype(&t)?;
        Ok(owned(format!("({{\n{code}{ct} _b{k} = {bc};\n{drops}_b{k};\n}})"), t))
    }

    fn int_lit(n: u128, ty: &str) -> String {
        if is_signed(ty) {
            format!("{n}LL")
        } else if ty == "CHAR" {
            format!("{n}U")
        } else if ty == "F64" {
            format!("{n}.0")
        } else {
            format!("{n}ULL")
        }
    }

    /// Lowers a binary operator; `l`/`r` are typed so literals adopt the other side's type.
    fn lower_binop(&mut self, cx: &mut Ctx, op: &str, l: &Expr, r: &Expr, expect: Option<&str>, hc: bool) -> Lowered {
        let logical = matches!(op, "and" | "or" | "implies");
        if logical {
            let a = self.lower(cx, l, Some("BOOL"), hc)?.code;
            let b = self.lower(cx, r, Some("BOOL"), hc)?.code;
            let c = match op {
                "and" => format!("({a} && {b})"),
                "or" => format!("({a} || {b})"),
                _ => format!("(!{a} || {b})"),
            };
            return Ok(owned(c, "BOOL"));
        }
        let comparison = matches!(op, "=" | "!=" | "<" | "<=" | ">" | ">=");
        let operand_expect = if comparison { None } else { expect };
        // A shift count has its own type. Otherwise type the side that has a type first, so
        // an untyped number adopts it.
        let (la, rb) = if matches!(op, "<<" | ">>") {
            let la = self.lower(cx, l, operand_expect, hc)?;
            let rb = self.lower(cx, r, None, hc)?;
            (la, rb)
        } else if cx.untyped_num(l) && !cx.untyped_num(r) {
            let rb = self.lower(cx, r, operand_expect, hc)?;
            let la = self.lower(cx, l, Some(&rb.ty.clone()), hc)?;
            (la, rb)
        } else {
            let la = self.lower(cx, l, operand_expect, hc)?;
            let rb = self.lower(cx, r, Some(&la.ty.clone()), hc)?;
            (la, rb)
        };
        let ta = la.ty.clone();
        match op {
            "=" | "!=" => {
                let mut t = Temps::default();
                let a = self.borrow(la, &mut t)?;
                let b = self.borrow(rb, &mut t)?;
                let mut e = self.eq_code(&ta, &a, &b)?;
                if op == "!=" {
                    e = format!("(!{e})");
                }
                Ok(owned(self.with_temps(t, e, "BOOL")?, "BOOL"))
            }
            "<" | "<=" | ">" | ">=" => Ok(owned(format!("({} {op} {})", la.code, rb.code), "BOOL")),
            "+%" | "-%" | "*%" => {
                let o = &op[..1];
                let (a, b) = (la.code, rb.code);
                if is_signed(&ta) {
                    Ok(owned(format!("((int64_t)((uint64_t)({a}) {o} (uint64_t)({b})))"), ta))
                } else {
                    Ok(owned(format!("({a} {o} {b})"), ta))
                }
            }
            "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "<<" | ">>" => {
                Ok(owned(format!("({} {op} {})", la.code, rb.code), ta))
            }
            other => Err(format!("operator `{other}` is not supported by the C backend")),
        }
    }

    fn ctor_owner(&self, ctor: &str, expect: Option<&str>, cx: &Ctx) -> Option<&'a ModuleDecl> {
        let has = |m: &ModuleDecl| matches!(&m.repr, ReprKind::Sum(vs) if vs.iter().any(|v| v.name == ctor));
        if let Some(e) = expect {
            if let Some(m) = self.module(type_head(e)) {
                if has(m) {
                    return Some(m);
                }
            }
        }
        if let Some(m) = self.module(&cx.owner) {
            if has(m) {
                return Some(m);
            }
        }
        self.mods.iter().copied().find(|m| has(m))
    }

    /// Concrete type arguments of `md` for a value built in this context.
    fn instance_targs(&self, md: &ModuleDecl, expect: Option<&str>, cx: &Ctx, known: &HashMap<String, String>) -> Option<Vec<String>> {
        if md.type_params.is_empty() {
            return Some(Vec::new());
        }
        if let Some(e) = expect {
            let (h, a) = split_type_app(e);
            if h == md.name && a.len() == md.type_params.len() {
                return Some(a.iter().map(|x| cnorm(x)).collect());
            }
        }
        if md.type_params.iter().all(|t| known.contains_key(t)) {
            return Some(md.type_params.iter().map(|t| known[t].clone()).collect());
        }
        if md.name == cx.owner {
            if let Some(st) = &cx.self_ty {
                return Some(split_type_app(st).1.iter().map(|x| cnorm(x)).collect());
            }
        }
        None
    }

    /// A constructor call; it consumes owned values of its fields.
    fn lower_ctor(&mut self, cx: &mut Ctx, md: &'a ModuleDecl, ctor: &str, args: &[Expr], expect: Option<&str>, hc: bool) -> Lowered {
        let ReprKind::Sum(vs) = &md.repr else { unreachable!() };
        let vd = vs.iter().find(|v| v.name == ctor).expect("variant");
        if vd.fields.len() != args.len() {
            return Err(format!("`{ctor}` takes {} arguments", vd.fields.len()));
        }
        // Infer the instance from the expected type, the context, or the arguments.
        let mut known = HashMap::new();
        let mut pre: Vec<Option<V>> = vec![None; args.len()];
        if self.instance_targs(md, expect, cx, &known).is_none() {
            for (i, ((_, ft), a)) in vd.fields.iter().zip(args).enumerate() {
                match self.lower(cx, a, None, hc) {
                    Ok(x) => {
                        unify(ft, &x.ty, &md.type_params, &mut known);
                        pre[i] = Some(x);
                    }
                    Err(e) if need(&e) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        let targs = self
            .instance_targs(md, expect, cx, &known)
            .ok_or(NEED_TYPE.to_string())?;
        let ty = Self::self_type(md, &targs);
        self.ctype(&ty)?;
        let fields = self.variant_fields(&ty, Some(ctor)).unwrap_or_default();
        let mut cargs = Vec::new();
        for (i, (a, (_, ft))) in args.iter().zip(fields.iter()).enumerate() {
            let v = match pre[i].take() {
                Some(v) => v,
                None => self.lower(cx, a, Some(ft), hc)?,
            };
            cargs.push(self.own(v)?);
        }
        Ok(owned(format!("make_{}_{ctor}({})", c_instance_name(&ty), cargs.join(", ")), ty))
    }

    /// A call of routine `r` of module `md` (`recv` is the receiver for routines on
    /// `Current`). Arguments and receiver are borrowed; the result is owned.
    #[allow(clippy::too_many_arguments)]
    fn lower_call(
        &mut self,
        cx: &mut Ctx,
        md: &'a ModuleDecl,
        r: &str,
        recv: Option<V>,
        args: &[Expr],
        expect: Option<&str>,
        hc: bool,
    ) -> Lowered {
        let tail_site = std::mem::take(&mut cx.tail_site);
        let (rd, kind) = Self::find_routine(md, r).ok_or_else(|| format!("`{}` has no routine `{r}`", md.name))?;
        if kind == "specify" {
            return Err(format!(
                "`{}.{r}` is a specification (ghost) routine and has no executable form",
                md.name
            ));
        }
        if rd.params.len() != args.len() {
            return Err(format!("`{}.{r}` takes {} arguments", md.name, rd.params.len()));
        }
        let mut tps = md.type_params.clone();
        tps.extend(rd.type_params.iter().cloned());
        let generic_self = Self::self_type(md, &md.type_params);
        let fix_cur = |t: &str| -> String {
            if t.trim() == "Current" { generic_self.clone() } else { t.to_string() }
        };
        let mut map: HashMap<String, String> = HashMap::new();
        if let Some(rv) = &recv {
            unify(&generic_self, &rv.ty, &tps, &mut map);
        } else if md.name == cx.owner && hc && Self::has_current(md, kind) {
            // The call uses the caller's `Current`, so it is the caller's instance.
            for tp in &md.type_params {
                if let Some(c) = cx.tmap.get(tp) {
                    map.insert(tp.clone(), c.clone());
                }
            }
        }
        if let Some(e) = expect {
            unify(&fix_cur(&rd.ret_type), e, &tps, &mut map);
        }
        let mut lowered: Vec<Option<V>> = vec![None; args.len()];
        for (i, ((_, pt), a)) in rd.params.iter().zip(args).enumerate() {
            let pt = subst(&fix_cur(pt), &map);
            if !mentions_any(&pt, &tps) {
                lowered[i] = Some(self.lower(cx, a, Some(&pt), hc)?);
                continue;
            }
            // An untyped number says nothing about a type parameter: it adopts one found
            // from the other arguments or from the caller's instance below.
            if cx.untyped_num(a) {
                continue;
            }
            match self.lower(cx, a, None, hc) {
                Ok(v) => {
                    unify(&pt, &v.ty, &tps, &mut map);
                    lowered[i] = Some(v);
                }
                Err(e) if need(&e) => {}
                Err(e) => return Err(e),
            }
        }
        // `M.make(..)` inside module M: the expected type and the arguments come first (a
        // `map[B]` on `BOX[A]` builds a `BOX[B]`). Only what they leave open is the caller's.
        if md.name == cx.owner {
            for tp in &md.type_params {
                if let (false, Some(c)) = (map.contains_key(tp), cx.tmap.get(tp)) {
                    map.insert(tp.clone(), c.clone());
                }
            }
        }
        for (i, ((_, pt), a)) in rd.params.iter().zip(args).enumerate() {
            if lowered[i].is_none() {
                let pt = subst(&fix_cur(pt), &map);
                if !mentions_any(&pt, &tps) {
                    lowered[i] = Some(self.lower(cx, a, Some(&pt), hc)?);
                } else if cx.untyped_num(a) {
                    let v = self.lower(cx, a, None, hc)?;
                    unify(&pt, &v.ty, &tps, &mut map);
                    lowered[i] = Some(v);
                } else {
                    return Err(NEED_TYPE.into());
                }
            }
        }
        if let Some(tp) = tps.iter().find(|t| !map.contains_key(*t)) {
            return Err(format!(
                "{NEED_TYPE}cannot infer type parameter `{tp}` of `{}.{r}`",
                md.name
            ));
        }
        let mod_targs: Vec<String> = md.type_params.iter().map(|t| map[t].clone()).collect();
        let rtargs: Vec<String> = rd.type_params.iter().map(|t| map[t].clone()).collect();
        let cname = self.request_fn(&md.name, &mod_targs, r, &rtargs);
        if tail_site && cname == cx.fn_cname {
            let self_ty = Self::self_type(md, &mod_targs);
            let mut vals = Vec::new();
            if Self::has_current(md, kind) {
                vals.push(recv.unwrap_or_else(|| borrowed("Current".to_string(), self_ty.clone())));
            }
            vals.extend(lowered.into_iter().flatten());
            map.insert("Current".into(), self_ty);
            let ret = subst(&rd.ret_type, &map);
            let code = self.tail_jump(cx, vals, &ret)?;
            return Ok(owned(code, ret));
        }
        let mut t = Temps::default();
        let mut cargs = Vec::new();
        if Self::has_current(md, kind) {
            match recv {
                Some(rv) => cargs.push(self.borrow(rv, &mut t)?),
                None if hc && md.name == cx.owner => cargs.push("Current".into()),
                None => return Err(format!("`{}.{r}` needs a receiver", md.name)),
            }
        }
        for v in lowered.into_iter().flatten() {
            cargs.push(self.borrow(v, &mut t)?);
        }
        let self_ty = Self::self_type(md, &mod_targs);
        map.insert("Current".into(), self_ty);
        let ret = subst(&rd.ret_type, &map);
        let call = format!("{cname}({})", cargs.join(", "));
        Ok(owned(self.with_temps(t, call, &ret)?, ret))
    }

    /// Calls a runtime function `f(recv, args..)` that borrows its arguments.
    fn runtime_call(&mut self, f: &str, vals: Vec<V>, ret: &str, result_owned: bool) -> Lowered {
        let mut t = Temps::default();
        let mut cargs = Vec::new();
        for v in vals {
            cargs.push(self.borrow(v, &mut t)?);
        }
        let call = format!("{f}({})", cargs.join(", "));
        if !result_owned && !t.pre.is_empty() {
            // A borrowed result of a temporary must not outlive it: own it first.
            let d = self.dup(ret, &call)?;
            return Ok(owned(self.with_temps(t, d, ret)?, ret));
        }
        Ok(V { code: self.with_temps(t, call, ret)?, ty: ret.to_string(), owned: result_owned })
    }

    fn lower_text_method(&mut self, cx: &mut Ctx, rv: V, m: &str, args: &[Expr], hc: bool) -> Lowered {
        let rt = rv.ty.clone();
        let table: &[(&str, &str, usize, &str)] = &[
            ("STRING", "byte_length", 0, "NAT"),
            ("STRING", "code_point_count", 0, "NAT"),
            ("STRING", "is_empty", 0, "BOOL"),
            ("STRING", "equals", 1, "BOOL"),
            ("STRING", "character_at", 1, "CHAR"),
            ("STRING", "concat", 1, "STRING"),
            ("STRING", "to_utf8", 0, "BYTES"),
            ("BYTES", "length", 0, "NAT"),
            ("BYTES", "is_empty", 0, "BOOL"),
            ("BYTES", "at", 1, "U64"),
            ("BYTES", "equals", 1, "BOOL"),
        ];
        let Some((_, _, _, ret)) = table.iter().find(|(t, n, k, _)| *t == rt && *n == m && *k == args.len()) else {
            return Err(format!("`{rt}.{m}` has no C runtime implementation"));
        };
        let mut vals = vec![rv];
        for x in args {
            let exp = match m {
                "character_at" | "at" => "NAT",
                _ => rt.as_str(),
            };
            vals.push(self.lower(cx, x, Some(exp), hc)?);
        }
        let prefix = if rt == "STRING" { "gardel_string_" } else { "gardel_bytes_" };
        self.runtime_call(&format!("{prefix}{m}"), vals, ret, true)
    }

    fn lower_list_method(&mut self, cx: &mut Ctx, rv: V, m: &str, args: &[Expr], hc: bool) -> Lowered {
        let rt = rv.ty.clone();
        let elem = split_type_app(&rt).1.first().map(|s| cnorm(s)).unwrap_or_default();
        self.ctype(&rt)?;
        let inst = c_instance_name(&rt);
        match (m, args.len()) {
            ("prepend", 1) => {
                let x = self.lower(cx, &args[0], Some(&elem), hc)?;
                self.runtime_call(&format!("{inst}__prepend"), vec![rv, x], &rt, true)
            }
            ("append", 1) => {
                let y = self.lower(cx, &args[0], Some(&rt), hc)?;
                self.runtime_call(&format!("{inst}__append"), vec![rv, y], &rt, true)
            }
            ("has", 1) => {
                let x = self.lower(cx, &args[0], Some(&elem), hc)?;
                self.runtime_call(&format!("{inst}__has"), vec![rv, x], "BOOL", true)
            }
            ("length", 0) => self.runtime_call(&format!("{inst}__length"), vec![rv], "NAT", true),
            ("is_empty", 0) => {
                let mut t = Temps::default();
                let c = self.borrow(rv, &mut t)?;
                Ok(owned(self.with_temps(t, format!("(({c}) == NULL)"), "BOOL")?, "BOOL"))
            }
            ("head", 0) => self.project(rv, "->head", &elem),
            ("tail", 0) => self.project(rv, "->tail", &rt),
            _ => Err(format!("`LIST.{m}` has no C runtime implementation")),
        }
    }

    /// A component of `base` (`->field`, `.first`): borrowed from a borrowed base; from an
    /// owned base it is copied out (owned) before the base is released.
    fn project(&mut self, base: V, access: &str, ty: &str) -> Lowered {
        if !base.owned || !self.is_rc(&base.ty) {
            return Ok(V { code: format!("(({}){access})", base.code), ty: ty.to_string(), owned: !self.is_rc(ty) });
        }
        let mut t = Temps::default();
        let b = self.borrow(base, &mut t)?;
        let d = self.dup(ty, &format!("(({b}){access})"))?;
        Ok(owned(self.with_temps(t, d, ty)?, ty))
    }

    fn lower_lambda(&mut self, cx: &Ctx, params: &[(String, String)], ret: &str, stmts: &[Stmt], body: &Expr) -> Lowered {
        let mut free = Vec::new();
        let mut bound: Vec<String> = params.iter().map(|(p, _)| p.clone()).collect();
        let blk = Expr::If(
            Box::new(Expr::Bool(true)),
            stmts.to_vec(),
            Box::new(body.clone()),
            Vec::new(),
            Box::new(Expr::Unit),
        );
        expr_free_vars(&blk, &mut bound, &mut free);
        // Captured values: locals of the enclosing function (and `Current`).
        let mut captures: Vec<(String, String, String)> = Vec::new(); // (gardel name, C source, type)
        for v in &free {
            if let Some(l) = cx.local(v) {
                captures.push((v.clone(), l.cname.clone(), l.ty.clone()));
            } else if v == "Current" {
                if let Some(st) = &cx.self_ty {
                    captures.push(("Current".into(), "Current".into(), st.clone()));
                }
            }
        }
        let k = self.next();
        let name = format!("gardel_lambda_{k}");
        let env = format!("{name}_env");
        let mut lcx = Ctx {
            owner: cx.owner.clone(),
            self_ty: cx.self_ty.clone(),
            tmap: cx.tmap.clone(),
            locals: Vec::new(),
            used: HashSet::new(),
            tail: false,
            tail_site: false,
            tail_drops: Vec::new(),
            fn_cname: String::new(),
            params: Vec::new(),
        };
        let mut prologue = String::new();
        let mut env_fields = String::new();
        let mut env_drops = String::new();
        let mut env_init = String::new();
        for (g, src, ty) in &captures {
            let ct = self.ctype(ty)?;
            let field = format!("c_{}", g.to_lowercase());
            env_fields.push_str(&format!(" {ct} {field};"));
            env_drops.push_str(&self.drop_stmt(ty, &format!("e->{field}"))?);
            let d = self.dup(ty, src)?;
            env_init.push_str(&format!("_e->{field} = {d}; "));
            let cn = if g == "Current" {
                lcx.used.insert("Current".into());
                "Current".to_string()
            } else {
                lcx.bind(g, ty)
            };
            // Captures are borrowed from the environment, which outlives the call.
            prologue.push_str(&format!("{ct} {cn} = ((struct {env}*)_env)->{field};\n"));
        }
        let mut cps = vec!["struct gardel_env* _env".to_string()];
        let mut tys = Vec::new();
        for (p, t) in params {
            let ty = subst(t, &cx.tmap);
            let ct = self.ctype(&ty)?;
            let cn = lcx.bind(p, &ty);
            cps.push(format!("{ct} {cn}"));
            tys.push(ty);
        }
        let rty = subst(ret, &cx.tmap);
        let cret = self.ctype(&rty)?;
        let hc_inner = captures.iter().any(|(g, _, _)| g == "Current");
        let b = self.lower_block(&mut lcx, stmts, body, Some(&rty), hc_inner)?;
        let b = self.own(b)?;
        let sig = format!("static {cret} {name}({})", cps.join(", "));
        self.protos.push(format!("{sig};\n"));
        if captures.is_empty() {
            self.bodies.push(format!("{sig} {{\n    (void)_env;\n    return {b};\n}}\n\n"));
        } else {
            self.type_defs.push(format!(
                "struct {env} {{ struct gardel_env hdr;{env_fields} }};\n"
            ));
            self.protos.push(format!("static void {env}_drop(struct gardel_env* h);\n"));
            self.bodies.push(format!(
                "static void {env}_drop(struct gardel_env* h) {{\n    struct {env}* e = (struct {env}*)h;\n    {env_drops}    gardel_free(e);\n}}\n\
                 {sig} {{\n{prologue}    return {b};\n}}\n\n"
            ));
        }
        let fty = cnorm(&format!("({}) -> {rty}", tys.join(", ")));
        let fct = self.ctype(&fty)?;
        let code = if captures.is_empty() {
            format!("(({fct}){{ {name}, NULL }})")
        } else {
            format!(
                "({{ struct {env}* _e = (struct {env}*)gardel_alloc(sizeof(struct {env})); atomic_init(&_e->hdr.rc, 1); _e->hdr.drop = {env}_drop; {env_init}({fct}){{ {name}, &_e->hdr }}; }})"
            )
        };
        Ok(owned(code, fty))
    }

    /// A routine used as a function value: a wrapper with the closure calling convention.
    fn routine_value(&mut self, cname: &str, ps: &[String], ret: &str) -> Lowered {
        let fty = cnorm(&format!("({}) -> {ret}", ps.join(", ")));
        let fct = self.ctype(&fty)?;
        let wrap = format!("{cname}__value");
        if self.done_helpers.insert(wrap.clone()) {
            let mut decl = vec!["struct gardel_env* _env".to_string()];
            let mut names = Vec::new();
            for (i, p) in ps.iter().enumerate() {
                decl.push(format!("{} a{i}", self.ctype(p)?));
                names.push(format!("a{i}"));
            }
            let cret = self.ctype(ret)?;
            let sig = format!("static {cret} {wrap}({})", decl.join(", "));
            self.protos.push(format!("{sig};\n"));
            self.bodies.push(format!("{sig} {{\n    (void)_env;\n    return {cname}({});\n}}\n\n", names.join(", ")));
        }
        Ok(owned(format!("(({fct}){{ {wrap}, NULL }})"), fty))
    }

    /// Calls a function value `f` (borrowed) with borrowed arguments.
    fn call_value(&mut self, cx: &mut Ctx, f: V, args: &[Expr], hc: bool) -> Lowered {
        let (ps, r) = cfn_parts(&f.ty).ok_or_else(|| format!("`{}` is not a function", f.code))?;
        let mut t = Temps::default();
        let fc = self.borrow(f, &mut t)?;
        let mut a = vec![format!("({fc}).env")];
        for (x, pt) in args.iter().zip(ps.iter()) {
            let v = self.lower(cx, x, Some(pt), hc)?;
            a.push(self.borrow(v, &mut t)?);
        }
        let call = format!("({fc}).fn({})", a.join(", "));
        Ok(owned(self.with_temps(t, call, &r)?, r))
    }

    fn lower_match(&mut self, cx: &mut Ctx, scrut: &Expr, cases: &[MatchCase], expect: Option<&str>, hc: bool) -> Lowered {
        let tail = std::mem::take(&mut cx.tail);
        let depth = cx.tail_drops.len();
        let sv0 = self.lower(cx, scrut, None, hc)?;
        let st = sv0.ty.clone();
        let md = self
            .module(type_head(&st))
            .filter(|m| matches!(m.repr, ReprKind::Sum(_)))
            .ok_or_else(|| format!("`match` on `{st}` is not supported by the C backend (only sum types)"))?;
        let ReprKind::Sum(vs) = &md.repr else { unreachable!() };
        let inst = c_instance_name(&st);
        let sct = self.ctype(&st)?;
        let k = self.next();
        let sv = format!("_m{k}");
        let rv = format!("_r{k}");
        let scrut_owned = sv0.owned;
        let sc = sv0.code;
        let mut result_ty: Option<String> = expect.map(|e| e.to_string());
        let mut soft_result = false;
        if tail && scrut_owned {
            cx.tail_drops.push((sv.clone(), st.clone()));
        }
        let mut arms = Vec::new();
        let mut has_default = false;
        for c in cases {
            let mark = cx.locals.len();
            let mut pre = String::new();
            let label = match &c.pattern {
                Pattern::Variant(ctor, subs) => {
                    let vd = vs
                        .iter()
                        .find(|v| &v.name == ctor)
                        .ok_or_else(|| format!("`{ctor}` is not a variant of `{st}`"))?;
                    let fields = self.variant_fields(&st, Some(&vd.name)).unwrap_or_default();
                    for (sp, (f, ft)) in subs.iter().zip(fields.iter()) {
                        match sp {
                            Pattern::Wildcard => {}
                            Pattern::Bind(b) => {
                                // Binders borrow from the scrutinee, alive for the arm.
                                let ct = self.ctype(ft)?;
                                let cn = cx.bind(b, ft);
                                pre.push_str(&format!("{ct} {cn} = {sv}->{ctor}.{f}; "));
                            }
                            _ => return Err("nested pattern reached the C backend (pattern compilation should have removed it)".into()),
                        }
                    }
                    format!("case TAG_{inst}_{ctor}:")
                }
                Pattern::Wildcard => {
                    has_default = true;
                    "default:".to_string()
                }
                Pattern::Bind(b) => {
                    has_default = true;
                    let cn = cx.bind(b, &st);
                    pre.push_str(&format!("{sct} {cn} = {sv}; "));
                    "default:".to_string()
                }
                _ => return Err("this `match` pattern is not supported by the C backend".into()),
            };
            let untyped = cx.untyped_num(&c.body);
            let arm_expect = if soft_result { None } else { result_ty.as_deref() };
            cx.tail = tail;
            let bv = self.lower_block(cx, &c.stmts, &c.body, arm_expect, hc)?;
            if soft_result && result_ty.as_deref().is_some_and(|t| numeric_conflict(t, &bv.ty)) {
                // Earlier arms were untyped numbers: this arm decides the type of the `match`.
                cx.locals.truncate(mark);
                let t = bv.ty.clone();
                cx.tail = tail;
                cx.tail_drops.truncate(depth);
                return self.lower_match(cx, scrut, cases, Some(&t), hc);
            }
            if result_ty.is_none() {
                result_ty = Some(bv.ty.clone());
                soft_result = untyped;
            } else if !untyped {
                soft_result = false;
            }
            let b = self.own(bv)?;
            cx.locals.truncate(mark);
            arms.push(format!("{label} {{ {pre}{rv} = {b}; break; }}\n"));
        }
        cx.tail_drops.truncate(depth);
        let rt = result_ty.ok_or("empty `match`")?;
        let crt = self.ctype(&rt)?;
        let mut s = format!("({{\n{sct} {sv} = {sc};\n{crt} {rv};\nswitch ({sv}->tag) {{\n");
        for a in arms {
            s.push_str(&a);
        }
        if !has_default {
            s.push_str("default: abort();\n");
        }
        s.push_str("}\n");
        if scrut_owned {
            s.push_str(&self.drop_stmt(&st, &sv)?);
        }
        s.push_str(&format!("{rv};\n}})"));
        Ok(owned(s, rt))
    }

    /// Lowers `e`. A numeric value of another kind than the context expects is never left to
    /// C's implicit conversion: the verifier reads both as mathematical numbers, so a signed
    /// value compared as unsigned would make the binary disagree with its proof.
    fn lower(&mut self, cx: &mut Ctx, e: &Expr, expect: Option<&str>, hc: bool) -> Lowered {
        let x = self.lower_expr(cx, e, expect, hc)?;
        if let Some(want) = expect.map(norm_type) {
            if numeric_conflict(&want, &x.ty) {
                return Err(match cx.soft_var(e) {
                    Some(v) => retype(&v, &want),
                    None => format!("a `{}` value is used where `{want}` is required", x.ty),
                });
            }
            // C only warns about a pointer to another instance, and the struct layout differs.
            let wrong_instance = self.module(type_head(&want)).is_some()
                && type_head(&want) == type_head(&x.ty)
                && cnorm(&want) != cnorm(&x.ty);
            if wrong_instance {
                return Err(format!("a `{}` value is used where `{want}` is required", x.ty));
            }
        }
        Ok(x)
    }

    fn lower_expr(&mut self, cx: &mut Ctx, e: &Expr, expect: Option<&str>, hc: bool) -> Lowered {
        // Tail position passes through blocks, `if` and `match` only: every other
        // subexpression starts with it cleared.
        let tail = std::mem::take(&mut cx.tail);
        cx.tail_site = false;
        let expect_n = expect.map(norm_type);
        let expect = expect_n.as_deref();
        match e {
            Expr::Int(n) => {
                let ty = match expect {
                    Some(t) if is_scalar(t) && t != "BOOL" && t != "UNIT" => t.to_string(),
                    _ => "U64".to_string(),
                };
                if is_signed(&ty) && *n > i64::MAX as u128 {
                    return Err(format!("the literal {n} does not fit in `{ty}`"));
                }
                Ok(owned(Self::int_lit(*n, &ty), ty))
            }
            Expr::Float(f) => Ok(owned(format!("{f:?}"), "F64")),
            Expr::Char(c) => Ok(owned(format!("{c}U"), "CHAR")),
            Expr::Bool(b) => Ok(owned((if *b { "true" } else { "false" }).into(), "BOOL")),
            Expr::Unit => Ok(owned("UNIT_VAL".into(), "UNIT")),
            Expr::Str(s) => {
                // A static buffer: the string has no owner and is never released.
                let k = self.next();
                let bytes: Vec<String> = s.bytes().map(|b| b.to_string()).chain(std::iter::once("0".into())).collect();
                Ok(owned(
                    format!(
                        "({{ static const uint8_t _s{k}[] = {{ {} }}; string_from_utf8(_s{k}, {}ULL); }})",
                        bytes.join(", "),
                        s.len()
                    ),
                    "STRING",
                ))
            }
            Expr::Var(v) => {
                if let Some(l) = cx.local(v) {
                    return Ok(borrowed(l.cname.clone(), l.ty.clone()));
                }
                if v == "Current" && hc {
                    return Ok(borrowed("Current".into(), cx.self_ty.clone().unwrap_or_default()));
                }
                if hc {
                    if let Some(st) = cx.self_ty.clone() {
                        if let Some(fs) = self.variant_fields(&st, None) {
                            if let Some((_, ft)) = fs.iter().find(|(f, _)| f == v) {
                                return Ok(borrowed(format!("Current->{v}"), ft.clone()));
                            }
                        }
                    }
                }
                if let Some(md) = self.ctor_owner(v, expect, cx) {
                    return self.lower_ctor(cx, md, v, &[], expect, hc);
                }
                if let Some(md) = self.module(&cx.owner) {
                    if let Some((rd, kind)) = Self::find_routine(md, v) {
                        if rd.params.is_empty() && (!Self::has_current(md, kind) || hc) {
                            return self.lower_call(cx, md, v, None, &[], expect, hc);
                        }
                        if !Self::has_current(md, kind) && rd.type_params.is_empty() && md.type_params.is_empty() {
                            let cname = self.request_fn(&md.name, &[], v, &[]);
                            let ps: Vec<String> = rd.params.iter().map(|(_, t)| cnorm(t)).collect();
                            return self.routine_value(&cname, &ps, &cnorm(&rd.ret_type));
                        }
                    }
                }
                Err(format!("unknown name `{v}`"))
            }
            Expr::UnaryNot(x) => {
                let c = self.lower(cx, x, Some("BOOL"), hc)?.code;
                Ok(owned(format!("(!{c})"), "BOOL"))
            }
            Expr::UnaryNeg(x) => {
                // A negated literal is signed unless the context says otherwise.
                let inner_expect = match (&**x, expect) {
                    (Expr::Int(_), Some(t)) if t == "F64" || is_signed(t) => Some(t),
                    (Expr::Int(0), Some(t)) if is_unsigned(t) => Some(t),
                    (Expr::Int(n), Some(t)) if is_unsigned(t) => {
                        return Err(format!("the negative literal -{n} cannot have type `{t}`"));
                    }
                    (Expr::Int(_), _) => Some("I64"),
                    _ => expect,
                };
                if matches!(**x, Expr::Int(n) if n == 1u128 << 63) && inner_expect.is_some_and(is_signed) {
                    // `9223372036854775808LL` itself is not a valid C literal.
                    let ty = inner_expect.unwrap_or("I64").to_string();
                    return Ok(owned("(-9223372036854775807LL - 1)".into(), ty));
                }
                let v = self.lower(cx, x, inner_expect, hc)?;
                if is_unsigned(&v.ty) && !matches!(**x, Expr::Int(0)) {
                    return Err(match cx.soft_var(x) {
                        Some(soft) => retype(&soft, "I64"),
                        None => format!("unary `-` on the unsigned type `{}`", v.ty),
                    });
                }
                Ok(owned(format!("(-{})", v.code), v.ty))
            }
            Expr::BinOp(op, l, r) => self.lower_binop(cx, op, l, r, expect, hc),
            Expr::Call(f, args) => {
                if let Some(l) = cx.local(f).cloned() {
                    return self.call_value(cx, borrowed(l.cname, l.ty), args, hc);
                }
                if let Some(md) = self.ctor_owner(f, expect, cx) {
                    return self.lower_ctor(cx, md, f, args, expect, hc);
                }
                let owner = self.module(&cx.owner).ok_or("no owner module")?;
                if Self::find_routine(owner, f).is_some() {
                    cx.tail_site = tail;
                    return self.lower_call(cx, owner, f, None, args, expect, hc);
                }
                Err(format!("unknown function `{f}`"))
            }
            Expr::MethodCall(recv, m, args) => {
                if let Expr::Var(v) = &**recv {
                    let is_value = cx.local(v).is_some()
                        || (v == "Current" && hc)
                        || (hc && cx
                            .self_ty
                            .clone()
                            .and_then(|st| self.variant_fields(&st, None))
                            .is_some_and(|fs| fs.iter().any(|(f, _)| f == v)));
                    if !is_value {
                        cx.tail_site = tail;
                        return self.lower_static(cx, v, m, args, expect, hc);
                    }
                }
                let rv = match self.lower(cx, recv, None, hc) {
                    // `SEQ.empty.concat(s)`, `LIST.empty.prepend(x)`: the receiver's type
                    // comes from the method's arguments or the expected result type.
                    Err(e) if need(&e) => match self.infer_receiver_type(cx, recv, m, args, expect, hc) {
                        Some(rty) => self.lower(cx, recv, Some(&rty), hc)?,
                        None => return Err(e),
                    },
                    r => r?,
                };
                let rt = rv.ty.clone();
                if rt == "STRING" || rt == "BYTES" {
                    return self.lower_text_method(cx, rv, m, args, hc);
                }
                // `equals` on a type without its own `equals` routine (scalars,
                // `CANONICAL_EQ` elements) is Leibniz equality.
                if m == "equals" && args.len() == 1 && self.module(type_head(&rt)).is_none_or(|md| Self::find_routine(md, "equals").is_none()) && type_head(&rt) != "LIST" {
                    let b = self.lower(cx, &args[0], Some(&rt), hc)?;
                    let mut t = Temps::default();
                    let ac = self.borrow(rv, &mut t)?;
                    let bc = self.borrow(b, &mut t)?;
                    let e = self.eq_code(&rt, &ac, &bc)?;
                    return Ok(owned(self.with_temps(t, e, "BOOL")?, "BOOL"));
                }
                if rt == "CHAR" && m == "code" && args.is_empty() {
                    return Ok(owned(format!("((uint64_t)({}))", rv.code), "NAT"));
                }
                if let Some((ta, tb)) = Self::tuple_parts(&rt) {
                    return match (m.as_str(), args.len()) {
                        ("first", 0) => self.project(rv, ".first", &ta),
                        ("second", 0) => self.project(rv, ".second", &tb),
                        _ => Err(format!("`TUPLE.{m}` has no C lowering")),
                    };
                }
                if type_head(&rt) == "LIST" {
                    return self.lower_list_method(cx, rv, m, args, hc);
                }
                if cfn_parts(&rt).is_some() && m == "apply" {
                    return self.call_value(cx, rv, args, hc);
                }
                let md = self
                    .module(type_head(&rt))
                    .ok_or_else(|| format!("no C lowering for `.{m}` on `{rt}`"))?;
                if args.is_empty() {
                    if let Some(fs) = self.variant_fields(&rt, None) {
                        if let Some((_, ft)) = fs.iter().find(|(f, _)| f == m) {
                            self.ctype(&rt)?;
                            return self.project(rv, &format!("->{m}"), ft);
                        }
                    }
                }
                cx.tail_site = tail;
                self.lower_call(cx, md, m, Some(rv), args, expect, hc)
            }
            // A block written as an expression by pattern compilation. Its dead branch is a
            // copy, and lowering it would double the C at every nested level.
            Expr::If(c, ts, te, _, _) if matches!(**c, Expr::Bool(true)) => {
                cx.tail = tail;
                self.lower_block(cx, ts, te, expect, hc)
            }
            Expr::If(c, ts, te, es, ee) => {
                let cc = self.lower(cx, c, Some("BOOL"), hc)?.code;
                // An untyped number takes its type from the other branch.
                let else_first = expect.is_none() && cx.untyped_num(te) && !cx.untyped_num(ee);
                cx.tail = tail;
                let first = if else_first {
                    Err(NEED_TYPE.to_string())
                } else {
                    self.lower_block(cx, ts, te, expect, hc)
                };
                let a = match first {
                    Ok(x) => x,
                    Err(e) if need(&e) && expect.is_none() => {
                        cx.tail = tail;
                        let b = self.lower_block(cx, es, ee, None, hc)?;
                        let tb = b.ty.clone();
                        cx.tail = tail;
                        let a = self.lower_block(cx, ts, te, Some(&tb), hc)?;
                        let (ac, bc) = (self.own(a)?, self.own(b)?);
                        return Ok(owned(format!("({cc} ? ({ac}) : ({bc}))"), tb));
                    }
                    Err(e) => return Err(e),
                };
                let ta = a.ty.clone();
                cx.tail = tail;
                let b = self.lower_block(cx, es, ee, Some(expect.unwrap_or(&ta)), hc)?;
                let (ac, bc) = (self.own(a)?, self.own(b)?);
                Ok(owned(format!("({cc} ? ({ac}) : ({bc}))"), ta))
            }
            Expr::Match(scrut, cases) => {
                cx.tail = tail;
                self.lower_match(cx, scrut, cases, expect, hc)
            }
            Expr::RecordLit(fields) => {
                let ty = expect
                    .map(|s| s.to_string())
                    .or_else(|| cx.self_ty.clone())
                    .ok_or(NEED_TYPE.to_string())?;
                let fs = self
                    .variant_fields(&ty, None)
                    .ok_or_else(|| format!("record literal of non-record type `{ty}`"))?;
                self.ctype(&ty)?;
                let mut a = Vec::new();
                for (f, ft) in &fs {
                    let (_, fe) = fields
                        .iter()
                        .find(|(n, _)| n == f)
                        .ok_or_else(|| format!("record literal misses field `{f}`"))?;
                    let v = self.lower(cx, fe, Some(ft), hc)?;
                    a.push(self.own(v)?);
                }
                Ok(owned(format!("make_{}({})", c_instance_name(&ty), a.join(", ")), ty))
            }
            Expr::RecordUpdate(base, fields) => {
                let bv = self.lower(cx, base, expect, hc)?;
                let bt = bv.ty.clone();
                let fs = self
                    .variant_fields(&bt, None)
                    .ok_or_else(|| format!("record update of non-record type `{bt}`"))?;
                let inst = c_instance_name(&bt);
                let mut t = Temps::default();
                let bc = self.borrow(bv, &mut t)?;
                let mut args = Vec::new();
                for (f, ft) in &fs {
                    match fields.iter().find(|(n, _)| n == f) {
                        Some((_, fe)) => {
                            let v = self.lower(cx, fe, Some(ft), hc)?;
                            args.push(self.own(v)?);
                        }
                        None => args.push(self.dup(ft, &format!("({bc})->{f}"))?),
                    }
                }
                if let Some((f, _)) = fields.iter().find(|(n, _)| !fs.iter().any(|(x, _)| x == n)) {
                    return Err(format!("`{bt}` has no field `{f}`"));
                }
                let call = format!("make_{inst}({})", args.join(", "));
                Ok(owned(self.with_temps(t, call, &bt)?, bt))
            }
            Expr::WithIo(stmts, fin) => {
                let mark = cx.locals.len();
                let mut s = String::from("({\n");
                let mut owned_locals = Vec::new();
                for st in stmts {
                    match st {
                        IoStmt::Bind(v, x) => {
                            let xv = self.lower(cx, x, None, hc)?;
                            let inner = xv
                                .ty
                                .strip_prefix("IO[")
                                .and_then(|x| x.strip_suffix(']'))
                                .map(norm_type)
                                .unwrap_or(xv.ty.clone());
                            let ct = self.ctype(&inner)?;
                            let c = if xv.owned { xv.code } else { self.dup(&inner, &xv.code)? };
                            let cn = cx.bind(v, &inner);
                            s.push_str(&format!("{ct} {cn} = {c};\n"));
                            owned_locals.push((cn, inner));
                        }
                        IoStmt::Action(x) => {
                            let xv = self.lower(cx, x, None, hc)?;
                            if xv.owned && self.is_rc(&xv.ty) {
                                let d = self.drop_stmt(&xv.ty, &xv.code)?;
                                s.push_str(&d);
                            } else {
                                s.push_str(&format!("{};\n", xv.code));
                            }
                        }
                    }
                }
                let fv = self.lower(cx, fin, expect, hc)?;
                let t = fv.ty.clone();
                let fc = self.own(fv)?;
                cx.locals.truncate(mark);
                let k = self.next();
                let ct = self.ctype(&t)?;
                s.push_str(&format!("{ct} _io{k} = {fc};\n"));
                for (cn, lt) in owned_locals.iter().rev() {
                    s.push_str(&self.drop_stmt(lt, cn)?);
                }
                s.push_str(&format!("_io{k};\n}})"));
                Ok(owned(s, t))
            }
            Expr::Lambda(ps, rt, stmts, body) => self.lower_lambda(cx, ps, rt, stmts, body),
            Expr::TupleLit(xs) => {
                if xs.len() != 2 {
                    return Err("only pairs are supported by the C backend".into());
                }
                let (ea, eb) = match expect.and_then(Self::tuple_parts) {
                    Some((a, b)) => (Some(a), Some(b)),
                    None => (None, None),
                };
                let a = self.lower(cx, &xs[0], ea.as_deref(), hc)?;
                let b = self.lower(cx, &xs[1], eb.as_deref(), hc)?;
                let ty = format!("TUPLE[{}, {}]", a.ty, b.ty);
                let ct = self.ctype(&ty)?;
                let (ac, bc) = (self.own(a)?, self.own(b)?);
                Ok(owned(format!("(({ct}){{ {ac}, {bc} }})"), ty))
            }
            Expr::Forall(..) | Expr::Exists(..) => {
                Err("quantifiers are specification-only and cannot be executed".into())
            }
        }
    }

    /// The module whose values `e` produces when only that is known: a static call
    /// `T.r(..)` (or a chain on it) whose result type is `T`.
    fn static_head(&self, e: &Expr, cx: &Ctx) -> Option<&'a ModuleDecl> {
        let Expr::MethodCall(recv, r, _) = e else {
            return None;
        };
        let owner = match &**recv {
            Expr::Var(v) if cx.local(v).is_none() && v != "Current" => self.module(v)?,
            inner => self.static_head(inner, cx)?,
        };
        let (rd, _) = Self::find_routine(owner, r)?;
        let ret = rd.ret_type.trim();
        (ret == "Current" || type_head(ret) == owner.name).then_some(owner)
    }

    /// Type of a receiver that could not be typed on its own, from the method it receives:
    /// its parameters unified with the argument types, and its result with `expect`.
    fn infer_receiver_type(
        &mut self,
        cx: &mut Ctx,
        recv: &Expr,
        m: &str,
        args: &[Expr],
        expect: Option<&str>,
        hc: bool,
    ) -> Option<String> {
        if let Expr::MethodCall(r0, _, _) = recv {
            if matches!(&**r0, Expr::Var(v) if v == "LIST" && cx.local(v).is_none()) {
                return match (m, args.len()) {
                    ("prepend", 1) => self.lower(cx, &args[0], None, hc).ok().map(|v| format!("LIST[{}]", v.ty)),
                    ("append", 1) => self.lower(cx, &args[0], None, hc).ok().map(|v| v.ty),
                    _ => expect.filter(|e| type_head(e) == "LIST").map(|e| e.to_string()),
                };
            }
        }
        let md = self.static_head(recv, cx)?;
        let (rd, _) = Self::find_routine(md, m)?;
        let mut tps = md.type_params.clone();
        tps.extend(rd.type_params.iter().cloned());
        let self_generic = Self::self_type(md, &md.type_params);
        let fix = |t: &str| if t.trim() == "Current" { self_generic.clone() } else { t.to_string() };
        let mut map = HashMap::new();
        for ((_, pt), a) in rd.params.iter().zip(args) {
            if let Ok(v) = self.lower(cx, a, None, hc) {
                unify(&fix(pt), &v.ty, &tps, &mut map);
            }
        }
        if let Some(e) = expect {
            unify(&fix(&rd.ret_type), e, &tps, &mut map);
        }
        let targs: Option<Vec<String>> = md.type_params.iter().map(|t| map.get(t).cloned()).collect();
        Some(Self::self_type(md, &targs?))
    }

    /// `T.r(args)` where `T` names a type or a runtime facility rather than a value.
    fn lower_static(&mut self, cx: &mut Ctx, v: &str, m: &str, args: &[Expr], expect: Option<&str>, hc: bool) -> Lowered {
        match (v, m, args.len()) {
            ("IO", "print", 1) | ("IO", "print_line", 1) => {
                let a = self.lower(cx, &args[0], Some("STRING"), hc)?;
                return self.runtime_call(&format!("gardel_io_{m}"), vec![a], "IO[UNIT]", true);
            }
            ("IO", "read_line", 0) => {
                self.needs_read_line = true;
                self.needs_validator = true;
                self.ctype(READ_LINE_RESULT_TYPE)?;
                return Ok(owned(
                    "gardel_io_read_line_result()".into(),
                    format!("IO[{}]", cnorm(READ_LINE_RESULT_TYPE)),
                ));
            }
            ("IO", "exit", 1) => {
                let a = self.lower(cx, &args[0], Some("EXIT_CODE"), hc)?;
                return Ok(owned(format!("gardel_io_exit({})", a.code), "IO[EXIT_CODE]"));
            }
            ("IO", _, _) => return Err(format!("`IO.{m}` has no C runtime implementation")),
            ("STRING", "from_utf8", 1) => {
                self.needs_from_utf8 = true;
                self.needs_validator = true;
                self.ctype(FROM_UTF8_RESULT_TYPE)?;
                let a = self.lower(cx, &args[0], Some("BYTES"), hc)?;
                return self.runtime_call("gardel_string_from_utf8", vec![a], &cnorm(FROM_UTF8_RESULT_TYPE), true);
            }
            ("STRING", _, _) | ("BYTES", _, _) => {
                return Err(format!("`{v}.{m}` has no C runtime implementation"));
            }
            ("CHAR", "from_scalar", 1) => {
                let a = self.lower(cx, &args[0], Some("NAT"), hc)?;
                return Ok(owned(format!("((uint32_t)({}))", a.code), "CHAR"));
            }
            ("LIST", "empty", 0) => {
                let lt = expect.filter(|e| type_head(e) == "LIST").ok_or(NEED_TYPE.to_string())?;
                let ct = self.ctype(lt)?;
                return Ok(owned(format!("(({ct})NULL)"), lt.to_string()));
            }
            ("LIST", "singleton", 1) => {
                let elem = expect
                    .filter(|e| type_head(e) == "LIST")
                    .and_then(|e| split_type_app(e).1.first().map(|s| cnorm(s)));
                let x = self.lower(cx, &args[0], elem.as_deref(), hc)?;
                let lt = format!("LIST[{}]", x.ty);
                let ct = self.ctype(&lt)?;
                let inst = c_instance_name(&lt);
                return self.runtime_call(&format!("{inst}__prepend"), vec![owned(format!("(({ct})NULL)"), lt.clone()), x], &lt, true);
            }
            ("LIST", _, _) => {
                return Err(format!("`LIST.{m}` has no C runtime implementation"));
            }
            _ => {}
        }
        let md = self.module(v).ok_or_else(|| format!("unknown name `{v}`"))?;
        if Self::find_routine(md, m).is_some() {
            return self.lower_call(cx, md, m, None, args, expect, hc);
        }
        Err(format!("`{v}` has no routine `{m}`"))
    }

    // ---------------------------------------------------------------- output

    /// `main(args: SEQ[STRING])`: argv[1..] as a balanced `SEQ[STRING]`. Every argument
    /// must be well-formed UTF-8 (RFC 3629, the `read_line` validator); otherwise the
    /// program stops with exit code 2 before `main` runs (spec §11). On Windows the
    /// arguments are taken from the UTF-16 command line and converted, since the C
    /// `argv` there is in the ANSI code page.
    fn args_runtime(&mut self) -> Result<String, String> {
        self.needs_validator = true;
        self.ctype("STRING_ERROR")?;
        self.ctype("SEQ[STRING]")?;
        Ok(r#"/* Command-line boundary (spec 11): argv[1..] as SEQ[STRING], each argument validated. */
#ifdef _WIN32
__declspec(dllimport) wchar_t* __stdcall GetCommandLineW(void);
__declspec(dllimport) wchar_t** __stdcall CommandLineToArgvW(const wchar_t*, int*);
__declspec(dllimport) int __stdcall WideCharToMultiByte(unsigned int, unsigned long, const wchar_t*, int, char*, int, const char*, int*);
__declspec(dllimport) void* __stdcall LocalFree(void*);
#endif
static struct SEQ_STRING* gardel_args_range(GardelString* xs, uint64_t lo, uint64_t hi) {
    if (hi - lo == 1) return make_SEQ_STRING_Leaf(xs[lo]);
    uint64_t mid = lo + (hi - lo) / 2;
    struct SEQ_STRING* l = gardel_args_range(xs, lo, mid);
    struct SEQ_STRING* r = gardel_args_range(xs, mid, hi);
    return make_SEQ_STRING_Concat(l, r, hi - lo);
}
static struct SEQ_STRING* gardel_main_args(int argc, char** argv) {
    int n = argc;
    char** u = argv;
#ifdef _WIN32
    wchar_t** w = CommandLineToArgvW(GetCommandLineW(), &n);
    if (w) {
        u = (char**)malloc(sizeof(char*) * (size_t)(n + 1));
        for (int i = 0; i < n; i++) {
            int sz = WideCharToMultiByte(65001, 0x80 /* WC_ERR_INVALID_CHARS */, w[i], -1, NULL, 0, NULL, NULL);
            u[i] = NULL;
            if (sz > 0) {
                u[i] = (char*)malloc((size_t)sz);
                WideCharToMultiByte(65001, 0x80, w[i], -1, u[i], sz, NULL, NULL);
            }
        }
        LocalFree(w);
    } else {
        n = argc;
    }
#endif
    if (n <= 1) return make_SEQ_STRING_Empty();
    GardelString* xs = (GardelString*)malloc(sizeof(GardelString) * (size_t)(n - 1));
    for (int i = 1; i < n; i++) {
        uint64_t len = u[i] ? (uint64_t)strlen(u[i]) : 0;
        uint64_t chars = 0;
        struct STRING_ERROR* err = u[i] ? gardel_utf8_validate((const uint8_t*)u[i], len, &chars) : NULL;
        if (!u[i] || err) {
            fprintf(stderr, "error: command-line argument %d is not valid UTF-8\n", i);
            exit(2);
        }
        /* The arguments live until the process ends: no owner, never released. */
        xs[i - 1] = (GardelString){ u[i], len, chars, NULL };
    }
    struct SEQ_STRING* s = gardel_args_range(xs, 0, (uint64_t)(n - 1));
    free(xs);
    return s;
}

"#
        .to_string())
    }

    fn assemble(&mut self, entry: &str, args_rt: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "// Generated by gardelc: typed C11 lowering of `{}` and the module instances it uses.\n",
            self.root.name
        ));
        if !self.errors.is_empty() {
            for e in &self.errors {
                out.push_str(&format!("#error \"gardelc C backend: {}\"\n", e.replace('"', "'")));
            }
        }
        out.push_str(&emit_c11_runtime_header());
        out.push_str(
            "\n#ifndef GARDEL_TASK_DEPTH\n#define GARDEL_TASK_DEPTH 12\n#endif\n\
             /* Task nesting depth of the running code; deeper `let` groups run inline. */\n\
             static _Thread_local int gardel_task_depth __attribute__((unused)) = 0;\n\n\
             /* Counted closure environment: captured values follow the header. */\n\
             struct gardel_env { _Atomic uint64_t rc; void (*drop)(struct gardel_env*); };\n\
             static inline void gardel_env_dup(struct gardel_env* e) {\n    if (e) atomic_fetch_add_explicit(&e->rc, 1, memory_order_relaxed);\n}\n\
             static inline void gardel_env_drop(struct gardel_env* e) {\n    if (e && atomic_fetch_sub_explicit(&e->rc, 1, memory_order_acq_rel) == 1) e->drop(e);\n}\n\n",
        );
        for s in &self.struct_fwd {
            out.push_str(s);
        }
        out.push('\n');
        for s in &self.fn_typedefs {
            out.push_str(s);
        }
        out.push('\n');
        for s in &self.type_defs {
            out.push_str(s);
        }
        for s in &self.rc_protos {
            out.push_str(s);
        }
        out.push('\n');
        if self.needs_validator {
            out.push_str(&emit_c11_utf8_validator());
        }
        if self.needs_read_line {
            out.push_str(&emit_c11_read_line_runtime());
        }
        if self.needs_from_utf8 {
            out.push_str(&emit_c11_from_utf8_runtime());
        }
        out.push_str(args_rt);
        for s in &self.protos {
            out.push_str(s);
        }
        out.push('\n');
        for s in &self.bodies {
            out.push_str(s);
        }
        out.push_str(entry);
        out
    }
}

/// C11 for a `program`: its routines and every module instance they use, in one
/// translation unit, with `main` running the program's `main` inside an OpenMP region.
/// For a module, a library build of its executable routines. Returns the C text and the
/// constructs that could not be lowered (when there are any, the text starts with
/// `#error`).
pub fn emit_c11_typed(root: &ModuleDecl, extra: &[&ModuleDecl]) -> (String, Vec<String>) {
    let mut b = TypedC::new(root, extra);
    let mut entry = String::new();
    let mut args_rt = String::new();
    if root.kind == ModuleKind::Program {
        match root.feature_routines.iter().find(|(_, r)| r.name == "main") {
            Some((_, mr)) => {
                let cname = b.request_fn(&root.name, &[], "main", &[]);
                let mut setup = String::new();
                let mut call_args = Vec::new();
                let mut release = String::new();
                for (_, t) in &mr.params {
                    let t = cnorm(t);
                    if t == "SEQ[STRING]" {
                        match b.args_runtime() {
                            Ok(rt) => {
                                args_rt = rt;
                                setup.push_str("    struct SEQ_STRING* args = gardel_main_args(argc, argv);\n");
                                call_args.push("args".to_string());
                                release.push_str("    drop_SEQ_STRING(args);\n");
                            }
                            Err(e) => b.errors.push(e),
                        }
                    } else {
                        b.errors.push(format!("`main` parameter of type `{t}` is not supported"));
                    }
                }
                b.drain();
                entry = format!(
                    "int main(int argc, char** argv) {{\n    (void)argc;\n    (void)argv;\n{setup}    int64_t rc = 0;\n    #pragma omp parallel\n    #pragma omp single\n    rc = (int64_t){cname}({});\n{release}    return (int)rc;\n}}\n",
                    call_args.join(", ")
                );
            }
            None => b.errors.push("program has no `main`".into()),
        }
    } else if matches!(root.name.as_str(), "STRING" | "BYTES") {
        // Runtime-provided: the C runtime header implements these types (`GardelString`,
        // `GardelBytes`); their Gardel source is the verification model.
        return (
            format!(
                "// `{}` is provided by the gardelc C runtime header; its Gardel source is the\n// verification model and is not compiled.\n{}",
                root.name,
                emit_c11_runtime_header()
            ),
            Vec::new(),
        );
    } else {
        // A library build: every executable routine at `U64` for each type parameter.
        let targs: Vec<String> = root.type_params.iter().map(|_| "U64".to_string()).collect();
        for (_, r) in root.create_routines.iter().chain(&root.feature_routines) {
            let rtargs: Vec<String> = r.type_params.iter().map(|_| "U64".to_string()).collect();
            b.request_fn(&root.name, &targs, &r.name, &rtargs);
        }
        if root.kind == ModuleKind::Module {
            let st = TypedC::self_type(root, &targs);
            if let Err(e) = b.ctype(&st) {
                b.errors.push(e);
            }
        }
        b.drain();
    }
    let code = b.assemble(&entry, &args_rt);
    (code, b.errors)
}
