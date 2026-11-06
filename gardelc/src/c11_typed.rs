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
pub(crate) fn cnorm(t: &str) -> String {
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
    /// C names of the owned locals the next constructor or call may take over, because
    /// nothing evaluated after it uses them. Like `tail`, this holds for one expression.
    moves: Vec<String>,
    /// Those that were taken over: their scope no longer releases them.
    moved: Vec<String>,
    /// Parameters a sink routine owns, for its outermost block to release.
    sink_params: Vec<(String, String)>,
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
    /// Emit the variant that owns its parameters (`is_sink`).
    sink: bool,
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
    /// Inside the argument of `IO.exit`: the process ends right after, so scopes do not
    /// release what they own (Task C.4.1).
    exiting: bool,
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
            exiting: false,
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

    /// `drop_stmt` for the end of a scope, where a dying program does not release. Only
    /// for names: an expression must still be evaluated.
    fn scope_drop(&mut self, ty: &str, name: &str) -> Result<String, String> {
        if self.exiting {
            return Ok(String::new());
        }
        self.drop_stmt(ty, name)
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
        let d = self.scope_drop(&v.ty, &format!("_t{k}"))?;
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
            "static void drop_{inst}(struct {inst}* p) {{\n    while (p && gardel_rc_release(&p->_rc)) {{\n        struct {inst}* next = NULL;\n{inner}        gardel_free(p, sizeof(struct {inst}));\n        p = next;\n    }}\n}}\n\n"
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
    while (p && gardel_rc_release(&p->_rc)) {{\n\
        struct {inst}* next = p->tail;\n        {drop_h}        gardel_free(p, sizeof(struct {inst}));\n        p = next;\n    }}\n}}\n\
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
                sink: false,
            });
        }
        cname
    }

    /// The variant of a routine that owns its counted parameters (Task C.4.3). Module names
    /// are upper case, so the prefix cannot meet another routine's C name.
    fn request_sink(&mut self, module: &str, mod_targs: &[String], routine: &str, rtargs: &[String]) -> String {
        let plain = self.request_fn(module, mod_targs, routine, rtargs);
        let cname = format!("sink_{plain}");
        if self.done_fns.insert(cname.clone()) {
            self.queue.push_back(FnReq {
                module: module.to_string(),
                mod_targs: mod_targs.to_vec(),
                routine: routine.to_string(),
                rtargs: rtargs.to_vec(),
                cname: cname.clone(),
                sink: true,
            });
        }
        cname
    }

    /// A routine whose body ends by storing a parameter in a new value, as `concat` does.
    /// The usual convention makes such a routine copy that parameter and its caller
    /// release it, so a caller finished with the value gives it to a variant instead.
    fn is_sink(md: &ModuleDecl, rd: &RoutineDecl) -> bool {
        let ReprKind::Sum(vs) = &md.repr else { return false };
        let Expr::Call(f, args) = &rd.body else { return false };
        vs.iter().any(|v| v.name == *f)
            && args.iter().any(|a| {
                matches!(a, Expr::Var(v) if v == "Current" || rd.params.iter().any(|(p, _)| p == v))
            })
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
            moves: Vec::new(),
            moved: Vec::new(),
            sink_params: Vec::new(),
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
        if req.sink {
            cx.sink_params = cx.params.clone();
        }
        let ret = subst(&rd.ret_type, &tmap);
        let cret = self.ctype(&ret)?;
        let body = self.lower_block(&mut cx, &rd.stmts, &rd.body, Some(&ret), has_cur)?;
        let body = self.own(body)?;
        if req.sink && self.tail_used {
            return Err("a routine that owns its parameters cannot be a loop".into());
        }
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
        let sink_params = std::mem::take(&mut cx.sink_params);
        loop {
            cx.tail = tail;
            cx.sink_params = sink_params.clone();
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

    /// A `let` whose value could not be built at its declared type.
    fn check_declared(v: &str, want: Option<&str>, got: &str) -> Result<(), String> {
        match want {
            Some(t) if t != "Current" && t != got => Err(format!("`{v}` is declared `{t}`, and its value is `{got}`")),
            _ => Ok(()),
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
        let mut owned_locals: Vec<(String, String)> = std::mem::take(&mut cx.sink_params);
        let groups = analyze_parallel_lets(stmts);
        // The declared type of each `let`, in the order `groups` holds them.
        let mut declared = stmts.iter().enumerate().filter(|(_, s)| matches!(s, Stmt::Let(..))).map(|(i, _)| crate::declared_let_type(stmts, i));
        for (at, group) in groups.iter().enumerate() {
            if group.len() == 1 {
                let (v, e) = &group[0];
                let want = declared.next().flatten();
                let soft = want.is_none() && !retyped.contains_key(v) && cx.untyped_num(e);
                cx.moves = Self::unused_after(cx, &owned_locals, &groups[at + 1..], body);
                let x = self.lower(cx, e, want.or(retyped.get(v).map(String::as_str)), has_cur);
                cx.moves.clear();
                Self::settle_moves(cx, &mut owned_locals);
                let x = x?;
                Self::check_declared(v, want, &x.ty)?;
                let t = x.ty.clone();
                let c = self.own(x)?;
                let ct = self.ctype(&t)?;
                let cn = cx.bind_let(v, &t, soft);
                code.push_str(&format!("{ct} {cn} = {c};\n"));
                owned_locals.push((cn, t));
            } else {
                let mut lowered = Vec::new();
                let mut soft = Vec::new();
                for (v, e) in group {
                    let want = declared.next().flatten();
                    soft.push(want.is_none() && !retyped.contains_key(v) && cx.untyped_num(e));
                    let x = self.lower(cx, e, want.or(retyped.get(v).map(String::as_str)), has_cur)?;
                    Self::check_declared(v, want, &x.ty)?;
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
        // Nothing of this block runs after its result, so all its locals may be taken over.
        cx.moves = owned_locals.iter().map(|(cn, _)| cn.clone()).collect();
        let b = self.lower(cx, body, expect, has_cur);
        cx.moves.clear();
        Self::settle_moves(cx, &mut owned_locals);
        let b = b?;
        cx.locals.truncate(mark);
        if code.is_empty() && owned_locals.is_empty() {
            return Ok(b);
        }
        let t = b.ty.clone();
        let bc = self.own(b)?;
        let mut drops = String::new();
        for (cn, lt) in owned_locals.iter().rev() {
            drops.push_str(&self.scope_drop(lt, cn)?);
        }
        let k = self.next();
        let ct = self.ctype(&t)?;
        Ok(owned(format!("({{\n{code}{ct} _b{k} = {bc};\n{drops}_b{k};\n}})"), t))
    }

    /// The owned locals that no later `let` and not the block's result can see. `Current`
    /// is left out, since a bare field name uses it without naming it.
    fn unused_after(
        cx: &Ctx,
        owned: &[(String, String)],
        later: &[Vec<(String, Expr)>],
        body: &Expr,
    ) -> Vec<String> {
        let mut free = Vec::new();
        for (_, e) in later.iter().flatten() {
            expr_free_vars(e, &mut Vec::new(), &mut free);
        }
        expr_free_vars(body, &mut Vec::new(), &mut free);
        owned
            .iter()
            .filter(|(cn, _)| {
                cx.locals.iter().any(|l| l.cname == *cn && !free.contains(&l.name))
            })
            .map(|(cn, _)| cn.clone())
            .collect()
    }

    /// Takes out of `owned` what the expression just lowered took over.
    fn settle_moves(cx: &mut Ctx, owned: &mut Vec<(String, String)>) {
        let moved = std::mem::take(&mut cx.moved);
        owned.retain(|(cn, _)| !moved.contains(cn));
    }

    /// An owned version of `v` for a consumer: a local in `moves` is taken over, once.
    fn take_or_own(&mut self, cx: &mut Ctx, moves: &mut Vec<String>, v: V) -> Result<String, String> {
        if !v.owned {
            if let Some(i) = moves.iter().position(|m| *m == v.code) {
                moves.remove(i);
                cx.moved.push(v.code.clone());
                return Ok(v.code);
            }
        }
        self.own(v)
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
        let mut moves = std::mem::take(&mut cx.moves);
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
            // The other arguments read a local taken here before the constructor stores
            // it, so it is alive for them.
            cargs.push(self.take_or_own(cx, &mut moves, v)?);
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
        let mut moves = std::mem::take(&mut cx.moves);
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
        let takes_current = Self::has_current(md, kind);
        if Self::is_sink(md, rd) && (!takes_current || recv.is_some() || (hc && md.name == cx.owner)) {
            let self_ty = Self::self_type(md, &mod_targs);
            let mut vals = Vec::new();
            if takes_current {
                vals.push(recv.clone().unwrap_or_else(|| borrowed("Current".to_string(), self_ty.clone())));
            }
            vals.extend(lowered.iter().flatten().cloned());
            // Worth it only when the caller has something to give: a value it would
            // otherwise release right after the call.
            let mut gives = false;
            for v in &vals {
                gives |= self.is_rc(&v.ty) && (v.owned || moves.contains(&v.code));
            }
            if gives {
                let sink = self.request_sink(&md.name, &mod_targs, r, &rtargs);
                let mut cargs = Vec::new();
                for v in vals {
                    cargs.push(self.take_or_own(cx, &mut moves, v)?);
                }
                map.insert("Current".into(), self_ty);
                let ret = subst(&rd.ret_type, &map);
                return Ok(owned(format!("{sink}({})", cargs.join(", ")), ret));
            }
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
            moves: Vec::new(),
            moved: Vec::new(),
            sink_params: Vec::new(),
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
        // A lambda runs later, perhaps many times: `IO.exit` around it says nothing about it.
        let outer = std::mem::take(&mut self.exiting);
        let b = self.lower_block(&mut lcx, stmts, body, Some(&rty), hc_inner);
        self.exiting = outer;
        let b = b?;
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
                "static void {env}_drop(struct gardel_env* h) {{\n    struct {env}* e = (struct {env}*)h;\n    {env_drops}    gardel_free(e, sizeof(struct {env}));\n}}\n\
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
            s.push_str(&self.scope_drop(&st, &sv)?);
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
        // Like `tail`, for this expression only, and only a constructor or a call uses it.
        let moves = std::mem::take(&mut cx.moves);
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
                    cx.moves = moves;
                    return self.lower_ctor(cx, md, f, args, expect, hc);
                }
                let owner = self.module(&cx.owner).ok_or("no owner module")?;
                if Self::find_routine(owner, f).is_some() {
                    cx.tail_site = tail;
                    cx.moves = moves;
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
                            .is_some_and(|fs| fs.iter().any(|(f, _)| f == v)))
                        // A nullary constructor as a receiver: `Shutdown.is_shutdown`.
                        || (self.module(v).is_none() && self.ctor_owner(v, None, cx).is_some());
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
                cx.moves = moves;
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
                    s.push_str(&self.scope_drop(lt, cn)?);
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
                // Nothing runs after the argument, so its scopes do not release.
                let outer = std::mem::replace(&mut self.exiting, true);
                let a = self.lower(cx, &args[0], Some("EXIT_CODE"), hc);
                self.exiting = outer;
                return Ok(owned(format!("gardel_io_exit({})", a?.code), "IO[EXIT_CODE]"));
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
/* The arguments as UTF-8, with the program's name first. An entry that cannot be converted is NULL. */
static int gardel_args_utf8(int argc, char** argv, char*** out) {
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
    *out = u;
    return n;
}
static struct SEQ_STRING* gardel_args_value(int n, char** u) {
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
static struct SEQ_STRING* gardel_main_args(int argc, char** argv) {
    char** u;
    int n = gardel_args_utf8(argc, argv, &u);
    return gardel_args_value(n, u);
}

"#
        .to_string())
    }

    /// The entry point of a `reactive program` (spec 11.6). `init` and `step` return owned
    /// values and borrow their arguments. The loop holds its own reference to the state, so
    /// the `STEP` that carried it can go. One OpenMP region surrounds the loop, as it does
    /// `main`: parked threads sleep during a wait and pick up the tasks of a `step`. A region
    /// per call would wake the whole team for every event (95 us instead of 3 us per event
    /// on 20 threads).
    fn reactive_host(&mut self, root: &ModuleDecl) -> Result<(String, String), String> {
        let name = root.name.clone();
        let ret = root
            .feature_routines
            .iter()
            .find(|(_, r)| r.name == "init")
            .map(|(_, r)| cnorm(&r.ret_type))
            .ok_or("a reactive program needs `init`")?;
        let state = ret
            .strip_prefix("STEP[")
            .and_then(|t| t.strip_suffix(']'))
            .ok_or("`init` must return `STEP[S]`")?
            .to_string();
        let args_rt = self.args_runtime()?;
        let init = self.request_fn(&name, &[], "init", &[]);
        let step = self.request_fn(&name, &[], "step", &[]);
        let rct = self.ctype(&ret)?;
        let sct = self.ctype(&state)?;
        self.needs_read_line = true;
        self.needs_validator = true;
        self.ctype(READ_LINE_RESULT_TYPE)?;
        let keyt = self.ctype("KEY")?;
        let evt = self.ctype("EVENT")?;
        let cmd = self.ctype("COMMAND")?;
        let cmds = self.ctype("LIST[COMMAND]")?;
        self.drain();

        // One case per variant, so a variant with no host code cannot be ignored at run time.
        let mut run = String::new();
        for v in self.variants_of("COMMAND")? {
            let body = host_command_code(&v).ok_or_else(|| format!("`COMMAND.{v}` has no host implementation"))?;
            let log = host_command_log(&v).ok_or_else(|| format!("`COMMAND.{v}` has no log form"))?;
            run.push_str(&format!(
                "        case TAG_COMMAND_{v}:\n            if (gardel_log_active()) {{ gardel_log_word(\"command \"); {log} gardel_log_finish(false); }}\n            {body} break;\n"
            ));
        }
        let mut make = String::new();
        for v in self.variants_of("EVENT")? {
            let case = host_event_code(&v).ok_or_else(|| format!("`EVENT.{v}` has no host implementation"))?;
            make.push_str(&format!("        {case}\n"));
        }
        let mut make_key = String::new();
        for v in self.variants_of("KEY")? {
            let case = host_key_code(&v).ok_or_else(|| format!("`KEY.{v}` has no host implementation"))?;
            make_key.push_str(&format!("        {case}\n"));
        }
        let inst = c_instance_name(&ret);
        let keep = self.own(borrowed("r->Continue.next".to_string(), state.clone()))?;
        let drop_step = self.drop_stmt(&ret, "r")?;
        let drop_state = self.drop_stmt(&state, "state")?;
        let drop_event = self.drop_stmt("EVENT", "e")?;
        let drop_args = self.drop_stmt("SEQ[STRING]", "args")?;
        Ok((
            format!(
                "{REACTIVE_RUNTIME}\n\
                 static void gardel_run_commands({cmds} p) {{\n    for (; p; p = p->tail) {{\n        {cmd} c = p->head;\n        switch (c->tag) {{\n{run}        default: abort();\n        }}\n    }}\n    gardel_log_flush();\n    /* The program may now wait for days: what it printed must already be out. */\n    fflush(stdout);\n}}\n\n\
                 static {keyt} gardel_key_value(struct gardel_host_event ev) {{\n    switch (ev.key) {{\n{make_key}        default: abort();\n    }}\n}}\n\n\
                 static {evt} gardel_event_value(struct gardel_host_event ev) {{\n    switch (ev.kind) {{\n{make}        default: abort();\n    }}\n}}\n\n\
                 int gardel_reactive_main(int argc, char** argv) {{\n\
                 \x20   gardel_host_install();\n\
                 \x20   gardel_timer_count = 0;\n\
                 \x20   const char* script = getenv(\"GARDEL_EVENTS\");\n\
                 \x20   if (script) gardel_host_script = script;\n\
                 \x20   char** argu;\n\
                 \x20   int argn = gardel_args_utf8(argc, argv, &argu);\n\
                 \x20   gardel_log_start(\"{name}\", &argn, &argu);\n\
                 \x20   struct SEQ_STRING* args = gardel_args_value(argn, argu);\n\
                 \x20   gardel_log_args(argn, argu);\n\
                 \x20   int code = 0;\n\
                 \x20   #pragma omp parallel\n\
                 \x20   #pragma omp single\n\
                 \x20   {{\n\
                 \x20       {rct} r = {init}(args);\n\
                 \x20       {drop_args}\
                 \x20       while (r->tag != TAG_{inst}_Stop) {{\n\
                 \x20           gardel_run_commands(r->Continue.cmds);\n\
                 \x20           {sct} state = {keep};\n\
                 \x20           {drop_step}\
                 \x20           struct gardel_host_event got = gardel_next_event();\n\
                 \x20           gardel_log_event(&got);\n\
                 \x20           {evt} e = gardel_event_value(got);\n\
                 \x20           r = {step}(state, e);\n\
                 \x20           {drop_event}\
                 \x20           {drop_state}\
                 #ifdef GARDEL_HOST_PROBE\n\
                 \x20           GARDEL_HOST_PROBE();\n\
                 #endif\n\
                 \x20       }}\n\
                 \x20       gardel_run_commands(r->Stop.cmds);\n\
                 \x20       code = (int)r->Stop.code;\n\
                 \x20       gardel_log_exit(code);\n\
                 \x20       {drop_step}\
                 \x20   }}\n\
                 \x20   gardel_net_finish();\n\
                 \x20   gardel_term_restore();\n\
                 \x20   gardel_host_finished();\n\
                 \x20   return code;\n\
                 }}\n\n\
                 int main(int argc, char** argv) {{\n\
                 \x20   int code = gardel_reactive_main(argc, argv);\n\
                 \x20   /* The reader may be blocked on stdin, and exit() would wait for that stream. */\n\
                 \x20   if (gardel_reader_started) {{\n\
                 \x20       fflush(stdout);\n\
                 \x20       fflush(stderr);\n\
                 \x20       _Exit(code);\n\
                 \x20   }}\n\
                 \x20   return code;\n\
                 }}\n"
            ),
            args_rt,
        ))
    }

    /// The variant names of a sum module in scope.
    fn variants_of(&self, module: &str) -> Result<Vec<String>, String> {
        match self.module(module).map(|m| &m.repr) {
            Some(ReprKind::Sum(vs)) => Ok(vs.iter().map(|v| v.name.clone()).collect()),
            _ => Err(format!("`{module}` is not available to the reactive host")),
        }
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
             #define gardel_task_depth (gardel_thread_get()->task_depth)\n\n\
             /* Counted closure environment: captured values follow the header. */\n\
             struct gardel_env { _Atomic uint64_t rc; void (*drop)(struct gardel_env*); };\n\
             static inline void gardel_env_dup(struct gardel_env* e) {\n    if (e) atomic_fetch_add_explicit(&e->rc, 1, memory_order_relaxed);\n}\n\
             static inline void gardel_env_drop(struct gardel_env* e) {\n    if (e && gardel_rc_release(&e->rc)) e->drop(e);\n}\n\n",
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

/// The host side of a `reactive program` (spec 11.6): shutdown signals, timers and the event
/// sources. The loop itself is generated by `reactive_host`, which knows the state type.
/// Event sources go through `gardel_next_event`, so tests and replay need no real signals.
const REACTIVE_RUNTIME: &str = r#"/* Reactive host (spec 11.6). */
enum gardel_host_kind {
    GARDEL_HOST_SHUTDOWN, GARDEL_HOST_TICK, GARDEL_HOST_LINE, GARDEL_HOST_KEY, GARDEL_HOST_ACCEPTED,
    GARDEL_HOST_LISTEN_FAILED, GARDEL_HOST_RECEIVED, GARDEL_HOST_SEND_FAILED, GARDEL_HOST_CLOSED
};
enum gardel_host_key {
    GARDEL_KEY_CHAR, GARDEL_KEY_UP, GARDEL_KEY_DOWN, GARDEL_KEY_LEFT, GARDEL_KEY_RIGHT,
    GARDEL_KEY_ENTER, GARDEL_KEY_ESCAPE, GARDEL_KEY_BACKSPACE, GARDEL_KEY_TAB,
    GARDEL_KEY_OTHER, GARDEL_KEY_END_OF_INPUT
};
struct gardel_host_event {
    enum gardel_host_kind kind;
    uint64_t id;                /* Tick: the timer. Sockets: the connection, or the port */
    void* line;                 /* Line: an owned RESULT[IO_ERROR, STRING] */
    enum gardel_host_key key;   /* Key */
    uint32_t ch;                /* Key: a Unicode scalar, for GARDEL_KEY_CHAR */
    GardelBytes data;           /* Received: owned */
    int64_t err;                /* ListenFailed, SendFailed, Line: the OS error code */
    int line_state;             /* Line: a GARDEL_LINE_* */
    char* raw;                  /* Line: the bytes read, kept only when recording */
    size_t raw_len;
};

/* Scripted events, from GARDEL_EVENTS: names separated by spaces. NULL reads the OS. */
static const char* gardel_host_script = NULL;
static const char* gardel_event_source = "GARDEL_EVENTS";
/* Set before any thread starts. A line's bytes are gone once it is a RESULT, and a bad line has
   no text to log, so the reader keeps them for a recording. */
static bool gardel_log_capture = false;

#ifdef _WIN32
__declspec(dllimport) int __stdcall SetConsoleCtrlHandler(int (__stdcall*)(unsigned long), int);
__declspec(dllimport) void* __stdcall CreateEventA(void*, int, int, const char*);
__declspec(dllimport) int __stdcall SetEvent(void*);
__declspec(dllimport) unsigned long __stdcall WaitForSingleObject(void*, unsigned long);
__declspec(dllimport) unsigned long long __stdcall GetTickCount64(void);
__declspec(dllimport) int __stdcall ResetEvent(void*);
__declspec(dllimport) unsigned long __stdcall WaitForMultipleObjects(unsigned long, void* const*, int, unsigned long);
__declspec(dllimport) void* __stdcall CreateThread(void*, size_t, unsigned long (__stdcall*)(void*), void*, unsigned long, unsigned long*);
__declspec(dllimport) void __stdcall AcquireSRWLockExclusive(void**);
__declspec(dllimport) void __stdcall ReleaseSRWLockExclusive(void**);
__declspec(dllimport) int __stdcall SleepConditionVariableSRW(void**, void**, unsigned long, unsigned long);
__declspec(dllimport) void __stdcall WakeConditionVariable(void**);
__declspec(dllimport) void* __stdcall GetStdHandle(unsigned long);
__declspec(dllimport) int __stdcall GetConsoleMode(void*, unsigned long*);
__declspec(dllimport) int __stdcall SetConsoleMode(void*, unsigned long);
__declspec(dllimport) int __stdcall ReadFile(void*, void*, unsigned long, unsigned long*, void*);
__declspec(dllimport) int __stdcall ReadConsoleInputW(void*, void*, unsigned long, unsigned long*);

/* Sockets: WS2_32, so a program links with -lws2_32. All sockets share one event: it only says
   that a socket changed, and WSAPoll says which. Levels are read again after each reset, so a
   change during a pass sets the event and cannot be lost. */
typedef unsigned long long gardel_sock;
#define GARDEL_NO_SOCK (~(gardel_sock)0)
#define GARDEL_POLLIN 0x0300
#define GARDEL_POLLOUT 0x0010
#define GARDEL_POLL_BASE 0
#define GARDEL_ERR_INVALID 10022
#define GARDEL_ERR_NO_BUFFERS 10055
struct gardel_pollfd { gardel_sock fd; short events, revents; };
struct gardel_sockaddr { short family; unsigned short port; uint32_t addr; char zero[8]; };
static struct gardel_pollfd* gardel_pfds = NULL;
static size_t gardel_poll_count = 0;
__declspec(dllimport) int __stdcall WSAStartup(unsigned short, void*);
__declspec(dllimport) int __stdcall WSAGetLastError(void);
__declspec(dllimport) int __stdcall WSAPoll(struct gardel_pollfd*, unsigned long, int);
__declspec(dllimport) int __stdcall WSAEventSelect(gardel_sock, void*, long);
__declspec(dllimport) gardel_sock __stdcall socket(int, int, int);
__declspec(dllimport) int __stdcall bind(gardel_sock, const struct gardel_sockaddr*, int);
__declspec(dllimport) int __stdcall listen(gardel_sock, int);
__declspec(dllimport) gardel_sock __stdcall accept(gardel_sock, void*, int*);
__declspec(dllimport) int __stdcall recv(gardel_sock, char*, int, int);
__declspec(dllimport) int __stdcall send(gardel_sock, const char*, int, int);
__declspec(dllimport) int __stdcall shutdown(gardel_sock, int);
__declspec(dllimport) int __stdcall closesocket(gardel_sock);
__declspec(dllimport) unsigned long __stdcall inet_addr(const char*);
static void* gardel_net_event = NULL;
static bool gardel_sock_start(void) {
    if (gardel_net_event) return true;
    _Alignas(8) char data[512]; /* WSADATA */
    if (WSAStartup(0x0202, data) != 0) return false;
    gardel_net_event = CreateEventA(NULL, 1, 0, NULL);
    return gardel_net_event != NULL;
}
static int gardel_sock_error(void) { return WSAGetLastError(); }
/* 10035 WSAEWOULDBLOCK, 10004 WSAEINTR */
static bool gardel_sock_again(int err) { return err == 10035 || err == 10004; }
/* 10053 WSAECONNABORTED, 10054 WSAECONNRESET: that client went, and the listener is fine. */
static bool gardel_sock_client_gone(int err) { return err == 10053 || err == 10054; }
static gardel_sock gardel_sock_tcp(void) { return socket(2, 1, 0); }
static uint32_t gardel_sock_ipv4(const char* text) { return (uint32_t)inet_addr(text); }
/* Windows' SO_REUSEADDR would let another process take the port over, so it is not set. */
static bool gardel_sock_bind(gardel_sock s, uint64_t port, uint32_t ip) {
    struct gardel_sockaddr a;
    memset(&a, 0, sizeof a);
    a.family = 2;
    a.port = (unsigned short)(((port & 0xFF) << 8) | (port >> 8));
    a.addr = ip;
    return bind(s, &a, sizeof a) == 0 && listen(s, 128) == 0;
}
static gardel_sock gardel_sock_accept(gardel_sock s) { return accept(s, NULL, NULL); }
/* FD_ACCEPT, or FD_READ | FD_WRITE | FD_CLOSE. WSAEventSelect also makes the socket non-blocking. */
static void gardel_sock_watch(gardel_sock s, bool listener) {
    if (WSAEventSelect(s, gardel_net_event, listener ? 8 : 1 | 2 | 32) != 0) abort();
}
static long gardel_sock_recv(gardel_sock s, char* buf, size_t n) { return recv(s, buf, (int)n, 0); }
static long gardel_sock_send(gardel_sock s, const char* buf, size_t n) {
    return send(s, buf, n > 0x40000000 ? 0x40000000 : (int)n, 0);
}
static void gardel_sock_shutdown(gardel_sock s) { shutdown(s, 1); }
static void gardel_sock_close(gardel_sock s) { closesocket(s); }
static int gardel_sock_poll(struct gardel_pollfd* p, size_t n, int ms) {
    return WSAPoll(p, (unsigned long)n, ms);
}
/* Every pass starts here, even one with nothing to watch: an event left set would spin the loop. */
static void gardel_sock_rearm(void) {
    if (gardel_net_event) ResetEvent(gardel_net_event);
}

static void* gardel_stop_requested = NULL;
static void* gardel_stop_finished = NULL;
static void* gardel_input_ready = NULL;
static int __stdcall gardel_console_ctrl(unsigned long kind) {
    SetEvent(gardel_stop_requested);
    /* 0 is Ctrl-C, 1 Ctrl-Break. For a closing console, a logoff or a shutdown, Windows ends
       the process when this handler returns, so let the Stop commands finish first. */
    if (kind >= 2) WaitForSingleObject(gardel_stop_finished, 4000);
    return 1;
}
static void gardel_host_install(void) {
    if (gardel_stop_requested) return;
    gardel_stop_requested = CreateEventA(NULL, 1, 0, NULL);
    gardel_stop_finished = CreateEventA(NULL, 1, 0, NULL);
    gardel_input_ready = CreateEventA(NULL, 1, 0, NULL);
    SetConsoleCtrlHandler(gardel_console_ctrl, 1);
}
static void gardel_host_finished(void) {
    if (gardel_stop_finished) SetEvent(gardel_stop_finished);
}
static uint64_t gardel_host_now_ms(void) {
    return (uint64_t)GetTickCount64();
}
/* Returns on a shutdown request or after `ms`. The request stays set, so it cannot be lost. */
static void gardel_host_wait(uint64_t ms) {
    void* all[3] = { gardel_stop_requested, gardel_input_ready, gardel_net_event };
    WaitForMultipleObjects(gardel_net_event ? 3 : 2, all, 0, ms > 0x7FFFFFFFULL ? 0x7FFFFFFFUL : (unsigned long)ms);
}
static bool gardel_host_stop_pending(void) {
    return WaitForSingleObject(gardel_stop_requested, 0) == 0;
}
static void gardel_input_signal(void) { SetEvent(gardel_input_ready); }
static void gardel_input_clear(void) { ResetEvent(gardel_input_ready); }

static void* gardel_lock_word = NULL;
static void* gardel_request_cv = NULL;
static void gardel_lock(void) { AcquireSRWLockExclusive(&gardel_lock_word); }
static void gardel_unlock(void) { ReleaseSRWLockExclusive(&gardel_lock_word); }
static void gardel_request_wait(void) { SleepConditionVariableSRW(&gardel_request_cv, &gardel_lock_word, 0xFFFFFFFFUL, 0); }
static void gardel_request_wake(void) { WakeConditionVariable(&gardel_request_cv); }
static void gardel_reader_main(void);
static unsigned long __stdcall gardel_reader_entry(void* unused) {
    (void)unused;
    gardel_reader_main();
    return 0;
}
static void gardel_reader_start(void) {
    if (!CreateThread(NULL, 0, gardel_reader_entry, NULL, 0, NULL)) abort();
}

/* The terminal. Ctrl-C stays a signal in raw mode (ENABLE_PROCESSED_INPUT). */
static void* gardel_term_in = NULL;
static unsigned long gardel_term_saved = 0;
static bool gardel_term_is_console = false, gardel_term_is_raw = false;
static void gardel_term_open(void) {
    gardel_term_in = GetStdHandle((unsigned long)-10);
    gardel_term_is_console = GetConsoleMode(gardel_term_in, &gardel_term_saved) != 0;
}
static void gardel_term_set_raw(bool raw) {
    if (!gardel_term_is_console || raw == gardel_term_is_raw) return;
    SetConsoleMode(gardel_term_in, raw ? (gardel_term_saved & ~6UL) : gardel_term_saved);
    gardel_term_is_raw = raw;
}
static int gardel_read_byte(void) {
    unsigned char b;
    unsigned long got = 0;
    if (!ReadFile(gardel_term_in, &b, 1, &got, NULL) || got == 0) return -1;
    return b;
}
static bool gardel_byte_soon(void) { return false; }
struct gardel_console_key {
    unsigned short type, pad;
    int down;
    unsigned short repeat, vk, scan;
    wchar_t ch;
    unsigned long state;
};
static void gardel_read_console_key(struct gardel_host_event* ev) {
    uint32_t high = 0;
    for (;;) {
        struct gardel_console_key k;
        unsigned long got = 0;
        if (!ReadConsoleInputW(gardel_term_in, &k, 1, &got) || got == 0) {
            ev->key = GARDEL_KEY_END_OF_INPUT;
            return;
        }
        if (k.type != 1 || !k.down) continue;
        switch (k.vk) {
            case 0x25: ev->key = GARDEL_KEY_LEFT; return;
            case 0x26: ev->key = GARDEL_KEY_UP; return;
            case 0x27: ev->key = GARDEL_KEY_RIGHT; return;
            case 0x28: ev->key = GARDEL_KEY_DOWN; return;
            case 0x0D: ev->key = GARDEL_KEY_ENTER; return;
            case 0x1B: ev->key = GARDEL_KEY_ESCAPE; return;
            case 0x08: ev->key = GARDEL_KEY_BACKSPACE; return;
            case 0x09: ev->key = GARDEL_KEY_TAB; return;
            default: break;
        }
        uint32_t c = (uint32_t)k.ch;
        if (c == 0) {
            /* Shift, Ctrl, Alt and Caps Lock alone are not keys of their own. */
            if ((k.vk >= 0x10 && k.vk <= 0x12) || k.vk == 0x14 || (k.vk >= 0xA0 && k.vk <= 0xA5)) continue;
            ev->key = GARDEL_KEY_OTHER;
            return;
        }
        if (c >= 0xD800 && c <= 0xDBFF) {
            high = c;
            continue;
        }
        if (c >= 0xDC00 && c <= 0xDFFF) {
            if (!high) {
                ev->key = GARDEL_KEY_OTHER;
                return;
            }
            c = 0x10000 + ((high - 0xD800) << 10) + (c - 0xDC00);
        }
        ev->key = c < 32 ? GARDEL_KEY_OTHER : GARDEL_KEY_CHAR;
        ev->ch = c;
        return;
    }
}
#else
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <sys/socket.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>
typedef int gardel_sock;
#define gardel_pollfd pollfd
#define GARDEL_NO_SOCK (-1)
#define GARDEL_POLLIN POLLIN
#define GARDEL_POLLOUT POLLOUT
#define GARDEL_POLL_BASE 2 /* the wake pipes come first */
#define GARDEL_ERR_INVALID EINVAL
#define GARDEL_ERR_NO_BUFFERS ENOBUFS
/* What the last pass over the sockets watched, after the two wake pipes. */
static struct pollfd* gardel_pfds = NULL;
static size_t gardel_poll_count = 0;
/* A peer that closed must end as an error on this send, not end the process. */
static bool gardel_sock_start(void) {
    signal(SIGPIPE, SIG_IGN);
    return true;
}
static int gardel_sock_error(void) { return errno; }
static bool gardel_sock_again(int err) { return err == EAGAIN || err == EWOULDBLOCK || err == EINTR; }
static bool gardel_sock_client_gone(int err) { return err == ECONNABORTED || err == ECONNRESET || err == EPROTO; }
static gardel_sock gardel_sock_tcp(void) { return socket(AF_INET, SOCK_STREAM, 0); }
static uint32_t gardel_sock_ipv4(const char* text) { return (uint32_t)inet_addr(text); }
/* SO_REUSEADDR: a server restarted at once can have its port back. */
static bool gardel_sock_bind(gardel_sock s, uint64_t port, uint32_t ip) {
    int on = 1;
    struct sockaddr_in a;
    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &on, sizeof on);
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_port = htons((uint16_t)port);
    a.sin_addr.s_addr = ip;
    return bind(s, (const struct sockaddr*)&a, sizeof a) == 0 && listen(s, 128) == 0;
}
static gardel_sock gardel_sock_accept(gardel_sock s) { return accept(s, NULL, NULL); }
static void gardel_sock_watch(gardel_sock s, bool listener) {
    (void)listener;
    int flags = fcntl(s, F_GETFL, 0);
    if (flags < 0 || fcntl(s, F_SETFL, flags | O_NONBLOCK) != 0) abort();
}
static long gardel_sock_recv(gardel_sock s, char* buf, size_t n) { return (long)recv(s, buf, n, 0); }
static long gardel_sock_send(gardel_sock s, const char* buf, size_t n) { return (long)send(s, buf, n, 0); }
static void gardel_sock_shutdown(gardel_sock s) { shutdown(s, SHUT_WR); }
static void gardel_sock_close(gardel_sock s) { close(s); }
static int gardel_sock_poll(struct gardel_pollfd* p, size_t n, int ms) { return poll(p, (nfds_t)n, ms); }
static void gardel_sock_rearm(void) {}

/* A self-pipe: the handler only writes a byte, and the loop waits with poll. */
static int gardel_signal_pipe[2] = {-1, -1};
static int gardel_input_pipe[2] = {-1, -1};
static void gardel_on_signal(int sig) {
    int saved = errno;
    char c = (char)sig;
    ssize_t ignored = write(gardel_signal_pipe[1], &c, 1);
    (void)ignored;
    errno = saved;
}
static void gardel_host_install(void) {
    if (gardel_signal_pipe[0] != -1) return;
    if (pipe(gardel_signal_pipe) != 0 || pipe(gardel_input_pipe) != 0) {
        perror("gardel: pipe");
        exit(2);
    }
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = gardel_on_signal;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = SA_RESTART;
    sigaction(SIGINT, &sa, NULL);
    sigaction(SIGTERM, &sa, NULL);
}
static void gardel_host_finished(void) {}
static uint64_t gardel_host_now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
/* The pipe is never read, so a shutdown request stays visible and cannot be lost. */
static void gardel_host_wait(uint64_t ms) {
    struct pollfd wake[2] = { { gardel_signal_pipe[0], POLLIN, 0 }, { gardel_input_pipe[0], POLLIN, 0 } };
    int timeout = ms > 0x3FFFFFFFULL ? 0x3FFFFFFF : (int)ms;
    struct pollfd* p = wake;
    size_t n = 2;
    if (gardel_poll_count > 0) {
        gardel_pfds[0] = wake[0];
        gardel_pfds[1] = wake[1];
        p = gardel_pfds;
        n = GARDEL_POLL_BASE + gardel_poll_count;
    }
    if (poll(p, (nfds_t)n, timeout) < 0 && errno != EINTR) {
        perror("gardel: poll");
        exit(2);
    }
}
static bool gardel_host_stop_pending(void) {
    struct pollfd p = { gardel_signal_pipe[0], POLLIN, 0 };
    return poll(&p, 1, 0) > 0;
}
static void gardel_input_signal(void) {
    char c = 1;
    ssize_t ignored = write(gardel_input_pipe[1], &c, 1);
    (void)ignored;
}
static void gardel_input_clear(void) {
    char c;
    ssize_t ignored = read(gardel_input_pipe[0], &c, 1);
    (void)ignored;
}

static pthread_mutex_t gardel_mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t gardel_request_cv = PTHREAD_COND_INITIALIZER;
static void gardel_lock(void) { pthread_mutex_lock(&gardel_mutex); }
static void gardel_unlock(void) { pthread_mutex_unlock(&gardel_mutex); }
static void gardel_request_wait(void) { pthread_cond_wait(&gardel_request_cv, &gardel_mutex); }
static void gardel_request_wake(void) { pthread_cond_signal(&gardel_request_cv); }
static void gardel_reader_main(void);
static void* gardel_reader_entry(void* unused) {
    (void)unused;
    gardel_reader_main();
    return NULL;
}
static void gardel_reader_start(void) {
    pthread_t t;
    /* Signals belong to the loop's self-pipe, not to this thread. */
    sigset_t all, old;
    sigfillset(&all);
    pthread_sigmask(SIG_BLOCK, &all, &old);
    if (pthread_create(&t, NULL, gardel_reader_entry, NULL) != 0) abort();
    pthread_sigmask(SIG_SETMASK, &old, NULL);
}

/* The terminal. Ctrl-C stays a signal in raw mode (ISIG is kept). */
static struct termios gardel_term_saved;
static bool gardel_term_is_console = false, gardel_term_is_raw = false;
static void gardel_term_open(void) {
    gardel_term_is_console = isatty(0) && tcgetattr(0, &gardel_term_saved) == 0;
}
static void gardel_term_set_raw(bool raw) {
    if (!gardel_term_is_console || raw == gardel_term_is_raw) return;
    struct termios t = gardel_term_saved;
    if (raw) {
        t.c_lflag &= ~(tcflag_t)(ICANON | ECHO);
        t.c_cc[VMIN] = 1;
        t.c_cc[VTIME] = 0;
    }
    tcsetattr(0, TCSANOW, &t);
    gardel_term_is_raw = raw;
}
static int gardel_read_byte(void) {
    unsigned char b;
    ssize_t n;
    while ((n = read(0, &b, 1)) < 0 && errno == EINTR) {}
    return n == 1 ? b : -1;
}
/* An escape sequence arrives at once; a lone Escape is followed by nothing. */
static bool gardel_byte_soon(void) {
    struct pollfd p = { 0, POLLIN, 0 };
    return poll(&p, 1, 50) > 0;
}
#endif

/* Pending one-shot timers. Fired in deadline order, and in setting order on a tie. */
struct gardel_timer { uint64_t id, deadline, seq; };
static struct gardel_timer* gardel_timers = NULL;
static size_t gardel_timer_count = 0, gardel_timer_cap = 0;
static uint64_t gardel_timer_seq = 0;

/* A pending timer with the same id is replaced. Scripted runs deliver Tick themselves, so
   they do not depend on the clock. */
static void gardel_host_set_timer(uint64_t id, uint64_t millis) {
    if (gardel_host_script) return;
    uint64_t now = gardel_host_now_ms();
    uint64_t deadline = millis > UINT64_MAX - now ? UINT64_MAX : now + millis;
    size_t i = 0;
    while (i < gardel_timer_count && gardel_timers[i].id != id) i++;
    if (i == gardel_timer_count) {
        if (gardel_timer_count == gardel_timer_cap) {
            gardel_timer_cap = gardel_timer_cap ? gardel_timer_cap * 2 : 8;
            gardel_timers = (struct gardel_timer*)realloc(gardel_timers, gardel_timer_cap * sizeof *gardel_timers);
            if (!gardel_timers) abort();
        }
        gardel_timer_count++;
    }
    gardel_timers[i] = (struct gardel_timer){ id, deadline, gardel_timer_seq++ };
}

/* Blocking input runs on one reader thread, so that a command never blocks the loop (spec
   11.6, rule 5). Requests are served in order, and each answer is queued for the loop. */
struct gardel_request { bool is_key; struct gardel_request* next; };
struct gardel_answer { struct gardel_host_event ev; struct gardel_answer* next; };
static struct gardel_request* gardel_requests = NULL;
static struct gardel_request** gardel_requests_end = &gardel_requests;
static struct gardel_answer* gardel_answers = NULL;
static struct gardel_answer** gardel_answers_end = &gardel_answers;
static bool gardel_reader_started = false;
static int gardel_pending_answers = 0;

static void gardel_keep_line(struct gardel_host_event* ev, const char* buf, size_t len) {
    if (!gardel_log_capture) return;
    ev->raw = (char*)malloc(len + 1);
    if (!ev->raw) abort();
    memcpy(ev->raw, buf, len);
    ev->raw_len = len;
}

/* Every exit path: after Stop, after a shutdown, and through exit(). */
static void gardel_term_restore(void) { gardel_term_set_raw(false); }

static void gardel_host_request_input(bool is_key) {
    if (gardel_host_script) return;
    struct gardel_request* r = (struct gardel_request*)malloc(sizeof *r);
    if (!r) abort();
    r->is_key = is_key;
    r->next = NULL;
    if (!gardel_reader_started) {
        gardel_reader_started = true;
        gardel_term_open();
        atexit(gardel_term_restore);
        gardel_reader_start();
    }
    gardel_lock();
    *gardel_requests_end = r;
    gardel_requests_end = &r->next;
    gardel_request_wake();
    gardel_unlock();
}

/* One Unicode scalar from its lead byte. A malformed sequence is Other, so that a Char always
   holds a valid CHAR (spec 3): these bytes are untrusted. */
static void gardel_decode_key_byte(int b, struct gardel_host_event* ev) {
    ev->key = GARDEL_KEY_OTHER;
    if (b < 0) { ev->key = GARDEL_KEY_END_OF_INPUT; return; }
    if (b == '\n' || b == '\r') { ev->key = GARDEL_KEY_ENTER; return; }
    if (b == '\t') { ev->key = GARDEL_KEY_TAB; return; }
    if (b == 8 || b == 127) { ev->key = GARDEL_KEY_BACKSPACE; return; }
    if (b == 27) {
        ev->key = GARDEL_KEY_ESCAPE;
        if (!gardel_byte_soon()) return;
        int c = gardel_read_byte();
        ev->key = GARDEL_KEY_OTHER;
        if (c != '[' && c != 'O') return;
        c = gardel_read_byte();
        switch (c) {
            case 'A': ev->key = GARDEL_KEY_UP; return;
            case 'B': ev->key = GARDEL_KEY_DOWN; return;
            case 'C': ev->key = GARDEL_KEY_RIGHT; return;
            case 'D': ev->key = GARDEL_KEY_LEFT; return;
            default: break;
        }
        /* Skip the rest of a longer sequence, such as ESC [ 3 ~. */
        while (c >= '0' && c <= ';') c = gardel_read_byte();
        return;
    }
    if (b < 32) return;
    if (b < 128) { ev->key = GARDEL_KEY_CHAR; ev->ch = (uint32_t)b; return; }
    int extra = b >= 0xF0 ? 3 : b >= 0xE0 ? 2 : b >= 0xC0 ? 1 : -1;
    if (extra < 0 || b > 0xF4) return;
    uint32_t c = (uint32_t)b & (0x3Fu >> extra);
    for (int i = 0; i < extra; i++) {
        int n = gardel_read_byte();
        if (n < 0x80 || n > 0xBF) return;
        c = (c << 6) | ((uint32_t)n & 0x3F);
    }
    static const uint32_t least[4] = { 0, 0x80, 0x800, 0x10000 };
    if (c < least[extra] || c > 0x10FFFF || (c >= 0xD800 && c <= 0xDFFF)) return;
    ev->key = GARDEL_KEY_CHAR;
    ev->ch = c;
}

static void gardel_read_key(struct gardel_host_event* ev) {
#ifdef _WIN32
    if (gardel_term_is_console) {
        gardel_read_console_key(ev);
        return;
    }
#endif
    gardel_decode_key_byte(gardel_read_byte(), ev);
}

static void gardel_reader_main(void) {
    for (;;) {
        gardel_lock();
        while (!gardel_requests) gardel_request_wait();
        struct gardel_request* r = gardel_requests;
        gardel_requests = r->next;
        if (!gardel_requests) gardel_requests_end = &gardel_requests;
        gardel_unlock();
        struct gardel_answer* a = (struct gardel_answer*)calloc(1, sizeof *a);
        if (!a) abort();
        gardel_term_set_raw(r->is_key);
        if (r->is_key) {
            a->ev.kind = GARDEL_HOST_KEY;
            gardel_read_key(&a->ev);
        } else {
            char* buf = NULL;
            size_t len = 0;
            a->ev.kind = GARDEL_HOST_LINE;
            a->ev.line_state = gardel_line_read(&buf, &len, &a->ev.err);
            if (a->ev.line_state == GARDEL_LINE_READ) gardel_keep_line(&a->ev, buf, len);
            a->ev.line = gardel_line_value(a->ev.line_state, buf, len, a->ev.err);
        }
        free(r);
        gardel_lock();
        *gardel_answers_end = a;
        gardel_answers_end = &a->next;
        gardel_pending_answers++;
        gardel_input_signal();
        gardel_unlock();
    }
}

static bool gardel_take_answer(struct gardel_host_event* ev) {
    if (!gardel_reader_started) return false;
    gardel_lock();
    struct gardel_answer* a = gardel_answers;
    if (a) {
        gardel_answers = a->next;
        if (!gardel_answers) gardel_answers_end = &gardel_answers;
#ifdef _WIN32
        if (--gardel_pending_answers == 0) gardel_input_clear();
#else
        gardel_pending_answers--;
        gardel_input_clear();
#endif
    }
    gardel_unlock();
    if (!a) return false;
    *ev = a->ev;
    free(a);
    return true;
}

/* Sockets (spec 11.6). A read happens when its event is delivered, never before: there is no
   buffered Received to outlive a Close, and every ready socket gets a turn per pass, so one busy
   client cannot starve the others. A command never blocks: what the kernel will not take now
   waits in the connection's queue.

   The handle guarantees of spec 11.6 come from here. Ids only grow, so each comes from exactly
   one Accepted. A connection leaves the table when its Closed is queued, and a command naming
   an id no longer in the table does nothing, so no event mentions an id after its Closed. */
#define GARDEL_NET_READ 65536
#define GARDEL_NET_PAUSE 65536       /* queued bytes at which a connection stops being read */
#define GARDEL_NET_QUEUE_MAX 1048576 /* queued bytes at which a Send fails */
#define GARDEL_NET_LINGER_MS 5000    /* how long a closing peer may take no data */
#define GARDEL_NET_EXIT_MS 1000      /* how long Stop waits for queued data */
#define GARDEL_NET_RETRY_MS 100      /* pause of a listener that cannot accept, say out of files */

struct gardel_listener { gardel_sock fd; uint64_t retry_at; };
struct gardel_conn {
    uint64_t id;
    gardel_sock fd;
    char* out;
    size_t out_off, out_len, out_cap;
    bool closing;       /* Close ran: send what is queued, then Closed */
    bool peer_ended;    /* the peer sent all it will: the same, but Send still works */
    uint64_t deadline;  /* of a connection ending, moved by each byte the peer takes */
};
struct gardel_poll_entry { uint64_t conn; size_t listener; }; /* conn 0 is a listener */

static struct gardel_listener* gardel_listeners = NULL;
static size_t gardel_listener_count = 0, gardel_listener_cap = 0;
static struct gardel_conn* gardel_conns = NULL; /* sorted by id: new ids are the largest */
static size_t gardel_conn_count = 0, gardel_conn_cap = 0;
static uint64_t gardel_next_conn = 1;
static struct gardel_answer* gardel_net_queue = NULL; /* failures and Closed, in order */
static struct gardel_answer** gardel_net_queue_end = &gardel_net_queue;
static struct gardel_poll_entry* gardel_poll_entries = NULL;
static size_t gardel_entry_cap = 0, gardel_pfd_cap = 0, gardel_poll_next = 0;
static bool gardel_net_turn = false, gardel_net_exiting = false;
static char gardel_net_buf[GARDEL_NET_READ];

static void* gardel_grow(void* p, size_t* cap, size_t need, size_t item) {
    if (need <= *cap) return p;
    size_t c = *cap ? *cap : 8;
    while (c < need) c *= 2;
    p = realloc(p, c * item);
    if (!p) abort();
    *cap = c;
    return p;
}

static GardelBytes gardel_bytes_copy(const void* src, size_t n) {
    void* owner = NULL;
    char* buf = gardel_buf_alloc(n + 1, &owner);
    memcpy(buf, src, n);
    buf[n] = '\0';
    GardelBytes b = { (const uint8_t*)buf, (uint64_t)n, owner };
    return b;
}

static void gardel_net_push(enum gardel_host_kind kind, uint64_t id, int64_t err) {
    struct gardel_answer* a = (struct gardel_answer*)calloc(1, sizeof *a);
    if (!a) abort();
    a->ev.kind = kind;
    a->ev.id = id;
    a->ev.err = err;
    *gardel_net_queue_end = a;
    gardel_net_queue_end = &a->next;
}

static bool gardel_net_pop(struct gardel_host_event* ev) {
    struct gardel_answer* a = gardel_net_queue;
    if (!a) return false;
    gardel_net_queue = a->next;
    if (!gardel_net_queue) gardel_net_queue_end = &gardel_net_queue;
    *ev = a->ev;
    free(a);
    return true;
}

/* After Close(c), the only event left for c is Closed(c). */
static void gardel_net_forget_failure(uint64_t id) {
    struct gardel_answer** p = &gardel_net_queue;
    while (*p) {
        struct gardel_answer* a = *p;
        if (a->ev.kind == GARDEL_HOST_SEND_FAILED && a->ev.id == id) {
            *p = a->next;
            free(a);
        } else {
            p = &a->next;
        }
    }
    gardel_net_queue_end = p;
}

static struct gardel_conn* gardel_conn_find(uint64_t id) {
    size_t lo = 0, hi = gardel_conn_count;
    while (lo < hi) {
        size_t mid = lo + (hi - lo) / 2;
        if (gardel_conns[mid].id == id) return &gardel_conns[mid];
        if (gardel_conns[mid].id < id) lo = mid + 1;
        else hi = mid;
    }
    return NULL;
}

static bool gardel_conn_has_queue(const struct gardel_conn* c) { return c->out_off < c->out_len; }
static bool gardel_conn_ending(const struct gardel_conn* c) { return c->closing || c->peer_ended; }

/* Input that was never read would turn the close into a reset, and a reset can take back data
   the peer has not read yet. */
static void gardel_conn_remove(struct gardel_conn* c) {
    gardel_sock_shutdown(c->fd);
    for (int i = 0; i < 16 && gardel_sock_recv(c->fd, gardel_net_buf, GARDEL_NET_READ) > 0; i++) {}
    gardel_sock_close(c->fd);
    free(c->out);
    size_t i = (size_t)(c - gardel_conns);
    memmove(c, c + 1, (gardel_conn_count - i - 1) * sizeof *c);
    gardel_conn_count--;
}

static void gardel_conn_ended(struct gardel_conn* c) {
    gardel_net_push(GARDEL_HOST_CLOSED, c->id, 0);
    gardel_conn_remove(c);
}

/* A failed send ends the connection, since the stream has a hole from there on. */
static void gardel_conn_failed(struct gardel_conn* c, int err) {
    if (!c->closing) gardel_net_push(GARDEL_HOST_SEND_FAILED, c->id, err);
    gardel_conn_ended(c);
}

/* False when the connection is gone. */
static bool gardel_conn_flush(struct gardel_conn* c) {
    while (gardel_conn_has_queue(c)) {
        long n = gardel_sock_send(c->fd, c->out + c->out_off, c->out_len - c->out_off);
        if (n < 0) {
            int err = gardel_sock_error();
            if (gardel_sock_again(err)) return true;
            gardel_conn_failed(c, err);
            return false;
        }
        c->out_off += (size_t)n;
        c->deadline = gardel_host_now_ms() + GARDEL_NET_LINGER_MS;
    }
    c->out_off = c->out_len = 0;
    if (c->out_cap > GARDEL_NET_PAUSE) {
        free(c->out);
        c->out = NULL;
        c->out_cap = 0;
    }
    return true;
}

/* A script has no sockets: the commands are written out, so a run can be checked. */
static void gardel_trace_bytes(GardelBytes b) {
    for (uint64_t i = 0; i < b.len; i++) {
        unsigned char c = b.data[i];
        bool plain = (c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z');
        if (plain) fputc(c, stderr);
        else fprintf(stderr, "%%%02X", c);
    }
}

/* All interfaces are an opt-in, GARDEL_LISTEN_ADDR=0.0.0.0: a program that only says `Listen`
   is not put on the network. */
static void gardel_host_listen(uint64_t port) {
    if (gardel_host_script) {
        fprintf(stderr, "net: Listen %llu\n", (unsigned long long)port);
        return;
    }
    /* Port 0 would be a port the program can never learn. */
    if (port == 0 || port > 65535) {
        gardel_net_push(GARDEL_HOST_LISTEN_FAILED, port, GARDEL_ERR_INVALID);
        return;
    }
    if (!gardel_sock_start()) {
        gardel_net_push(GARDEL_HOST_LISTEN_FAILED, port, GARDEL_ERR_NO_BUFFERS);
        return;
    }
    const char* addr = getenv("GARDEL_LISTEN_ADDR");
    uint32_t ip = gardel_sock_ipv4(addr ? addr : "127.0.0.1");
    if (ip == 0xFFFFFFFFu) {
        gardel_net_push(GARDEL_HOST_LISTEN_FAILED, port, GARDEL_ERR_INVALID);
        return;
    }
    gardel_sock s = gardel_sock_tcp();
    if (s == GARDEL_NO_SOCK || !gardel_sock_bind(s, port, ip)) {
        int err = gardel_sock_error();
        if (s != GARDEL_NO_SOCK) gardel_sock_close(s);
        gardel_net_push(GARDEL_HOST_LISTEN_FAILED, port, err);
        return;
    }
    gardel_sock_watch(s, true);
    gardel_listeners = (struct gardel_listener*)gardel_grow(gardel_listeners, &gardel_listener_cap, gardel_listener_count + 1, sizeof *gardel_listeners);
    gardel_listeners[gardel_listener_count++] = (struct gardel_listener){ s, 0 };
}

static void gardel_host_send(uint64_t id, GardelBytes data) {
    if (gardel_host_script) {
        fprintf(stderr, "net: Send %llu ", (unsigned long long)id);
        gardel_trace_bytes(data);
        fprintf(stderr, "\n");
        return;
    }
    struct gardel_conn* c = gardel_conn_find(id);
    if (!c || c->closing || data.len == 0) return;
    size_t queued = c->out_len - c->out_off;
    if (queued >= GARDEL_NET_QUEUE_MAX) {
        gardel_conn_failed(c, GARDEL_ERR_NO_BUFFERS);
        return;
    }
    if (c->out_off > 0) {
        memmove(c->out, c->out + c->out_off, queued);
        c->out_off = 0;
        c->out_len = queued;
    }
    c->out = (char*)gardel_grow(c->out, &c->out_cap, queued + (size_t)data.len, 1);
    memcpy(c->out + queued, data.data, (size_t)data.len);
    c->out_len = queued + (size_t)data.len;
    /* With data already queued the kernel is full, and the socket will say when it is not. */
    if (queued == 0) gardel_conn_flush(c);
}

static void gardel_host_close(uint64_t id) {
    if (gardel_host_script) {
        fprintf(stderr, "net: Close %llu\n", (unsigned long long)id);
        return;
    }
    gardel_net_forget_failure(id);
    struct gardel_conn* c = gardel_conn_find(id);
    if (!c || c->closing) return;
    c->closing = true;
    if (gardel_conn_has_queue(c)) c->deadline = gardel_host_now_ms() + GARDEL_NET_LINGER_MS;
    else gardel_conn_ended(c);
}

/* What the next pass watches: every listener not pausing, and each connection for what it can
   take. A connection with a long queue is not read: a client that sends and never reads then
   fills its own TCP window, and not this process. */
static void gardel_net_build(uint64_t now) {
    size_t want = gardel_listener_count + gardel_conn_count;
    gardel_poll_entries = (struct gardel_poll_entry*)gardel_grow(gardel_poll_entries, &gardel_entry_cap, want + 1, sizeof *gardel_poll_entries);
    gardel_pfds = (struct gardel_pollfd*)gardel_grow(gardel_pfds, &gardel_pfd_cap, want + 1 + GARDEL_POLL_BASE, sizeof *gardel_pfds);
    gardel_poll_count = 0;
    for (size_t i = 0; i < gardel_listener_count; i++) {
        if (gardel_listeners[i].retry_at > now) continue;
        gardel_pfds[GARDEL_POLL_BASE + gardel_poll_count] = (struct gardel_pollfd){ gardel_listeners[i].fd, GARDEL_POLLIN, 0 };
        gardel_poll_entries[gardel_poll_count++] = (struct gardel_poll_entry){ 0, i };
    }
    for (size_t i = 0; i < gardel_conn_count; i++) {
        const struct gardel_conn* c = &gardel_conns[i];
        short events = gardel_conn_has_queue(c) ? GARDEL_POLLOUT : 0;
        if (!gardel_net_exiting && !gardel_conn_ending(c) && c->out_len - c->out_off < GARDEL_NET_PAUSE) events |= GARDEL_POLLIN;
        gardel_pfds[GARDEL_POLL_BASE + gardel_poll_count] = (struct gardel_pollfd){ c->fd, events, 0 };
        gardel_poll_entries[gardel_poll_count++] = (struct gardel_poll_entry){ c->id, 0 };
    }
}

/* One pass: true when some socket is ready. */
static bool gardel_net_poll(void) {
    gardel_poll_next = gardel_poll_count = 0;
    gardel_sock_rearm();
    if (gardel_listener_count + gardel_conn_count == 0) return false;
    uint64_t now = gardel_host_now_ms();
    for (size_t i = gardel_conn_count; i-- > 0;) {
        struct gardel_conn* c = &gardel_conns[i];
        if (gardel_conn_ending(c) && c->deadline <= now) gardel_conn_ended(c);
    }
    gardel_net_build(now);
    if (gardel_poll_count == 0) return false;
    int ready = gardel_sock_poll(gardel_pfds + GARDEL_POLL_BASE, gardel_poll_count, 0);
    if (ready < 0 && !gardel_sock_again(gardel_sock_error())) {
        fprintf(stderr, "gardel: poll on sockets failed: %d\n", gardel_sock_error());
        exit(2);
    }
    if (ready > 0) return true;
    gardel_poll_next = gardel_poll_count;
    return false;
}

static bool gardel_net_accept(size_t i, struct gardel_host_event* ev) {
    gardel_sock s = gardel_sock_accept(gardel_listeners[i].fd);
    if (s == GARDEL_NO_SOCK) {
        /* The listener stays ready while there is no file to accept with, so it must rest. */
        int err = gardel_sock_error();
        if (!gardel_sock_again(err) && !gardel_sock_client_gone(err)) {
            gardel_listeners[i].retry_at = gardel_host_now_ms() + GARDEL_NET_RETRY_MS;
        }
        return false;
    }
    gardel_sock_watch(s, false);
    gardel_conns = (struct gardel_conn*)gardel_grow(gardel_conns, &gardel_conn_cap, gardel_conn_count + 1, sizeof *gardel_conns);
    memset(&gardel_conns[gardel_conn_count], 0, sizeof *gardel_conns);
    gardel_conns[gardel_conn_count].id = gardel_next_conn++;
    gardel_conns[gardel_conn_count].fd = s;
    ev->kind = GARDEL_HOST_ACCEPTED;
    ev->id = gardel_conns[gardel_conn_count].id;
    gardel_conn_count++;
    return true;
}

/* Serves one socket of the pass. Sends go first, since a read can only add to the queue. */
static bool gardel_net_serve(size_t i, struct gardel_host_event* ev) {
    if (gardel_pfds[GARDEL_POLL_BASE + i].revents == 0) return false;
    struct gardel_poll_entry e = gardel_poll_entries[i];
    if (e.conn == 0) return gardel_net_accept(e.listener, ev);
    /* Commands ran since the pass: this one may have gone, or be closing. */
    struct gardel_conn* c = gardel_conn_find(e.conn);
    if (!c || !gardel_conn_flush(c)) return false;
    if (gardel_conn_ending(c)) {
        if (!gardel_conn_has_queue(c)) gardel_conn_ended(c);
        return false;
    }
    if (c->out_len - c->out_off >= GARDEL_NET_PAUSE) return false;
    long n = gardel_sock_recv(c->fd, gardel_net_buf, GARDEL_NET_READ);
    if (n > 0) {
        ev->kind = GARDEL_HOST_RECEIVED;
        ev->id = c->id;
        ev->data = gardel_bytes_copy(gardel_net_buf, (size_t)n);
        return true;
    }
    if (n == 0) {
        /* What is still queued can reach a peer that only closed its sending side. */
        c->peer_ended = true;
        if (gardel_conn_has_queue(c)) c->deadline = gardel_host_now_ms() + GARDEL_NET_LINGER_MS;
        else gardel_conn_ended(c);
        return false;
    }
    if (!gardel_sock_again(gardel_sock_error())) gardel_conn_ended(c);
    return false;
}

static bool gardel_net_take(struct gardel_host_event* ev) {
    for (;;) {
        if (gardel_net_pop(ev)) return true;
        if (gardel_poll_next >= gardel_poll_count) {
            bool ready = gardel_net_poll();
            if (gardel_net_pop(ev)) return true;
            if (!ready) return false;
        }
        if (gardel_net_serve(gardel_poll_next++, ev)) return true;
    }
}

/* The longest wait that misses no deadline of the sockets. */
static uint64_t gardel_net_wait_limit(uint64_t ms) {
    uint64_t now = gardel_host_now_ms();
    for (size_t i = 0; i < gardel_listener_count; i++) {
        uint64_t at = gardel_listeners[i].retry_at;
        if (at > now && at - now < ms) ms = at - now;
    }
    for (size_t i = 0; i < gardel_conn_count; i++) {
        const struct gardel_conn* c = &gardel_conns[i];
        if (!gardel_conn_ending(c)) continue;
        uint64_t left = c->deadline > now ? c->deadline - now : 0;
        if (left < ms) ms = left;
    }
    return ms;
}

/* After Stop: its commands ran, so what they queued gets a moment to leave. */
static void gardel_net_finish(void) {
    uint64_t end = gardel_host_now_ms() + GARDEL_NET_EXIT_MS;
    while (gardel_listener_count > 0) gardel_sock_close(gardel_listeners[--gardel_listener_count].fd);
    gardel_net_exiting = true;
    for (;;) {
        bool pending = false;
        for (size_t i = gardel_conn_count; i-- > 0;) {
            struct gardel_conn* c = &gardel_conns[i];
            if (gardel_conn_has_queue(c) && gardel_conn_flush(c) && gardel_conn_has_queue(c)) pending = true;
        }
        uint64_t now = gardel_host_now_ms();
        if (!pending || now >= end) break;
        gardel_net_build(now);
        gardel_sock_poll(gardel_pfds + GARDEL_POLL_BASE, gardel_poll_count, (int)(end - now));
    }
    gardel_net_exiting = false;
    while (gardel_conn_count > 0) gardel_conn_remove(&gardel_conns[gardel_conn_count - 1]);
    struct gardel_host_event ev;
    while (gardel_net_pop(&ev)) {
        if (ev.kind == GARDEL_HOST_RECEIVED) gardel_bytes_drop(ev.data);
    }
    gardel_poll_next = gardel_poll_count = 0;
}

static int gardel_hex_digit(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    return -1;
}

/* Decimal digits of a script token, from `*i`. */
static bool gardel_script_number(const char* name, size_t n, size_t* i, uint64_t* out) {
    size_t start = *i;
    uint64_t v = 0;
    while (*i < n && name[*i] >= '0' && name[*i] <= '9') {
        uint64_t d = (uint64_t)(name[*i] - '0');
        if (v > (UINT64_MAX - d) / 10) return false;
        v = v * 10 + d;
        (*i)++;
    }
    *out = v;
    return *i > start;
}

/* The text of a script token from `i`, with %XX for a byte: a space cannot be written otherwise,
   and neither can bad UTF-8. NULL on a bad escape. */
static char* gardel_script_text(const char* name, size_t n, size_t i, size_t* len) {
    char* buf = (char*)malloc(n + 1);
    if (!buf) abort();
    *len = 0;
    while (i < n) {
        if (name[i] != '%') {
            buf[(*len)++] = name[i++];
            continue;
        }
        int hi = i + 2 < n ? gardel_hex_digit(name[i + 1]) : -1;
        int lo = i + 2 < n ? gardel_hex_digit(name[i + 2]) : -1;
        if (hi < 0 || lo < 0) {
            free(buf);
            return NULL;
        }
        buf[(*len)++] = (char)(hi * 16 + lo);
        i += 3;
    }
    return buf;
}

static const struct { const char* name; enum gardel_host_key key; } gardel_key_names[] = {
    {"Up", GARDEL_KEY_UP}, {"Down", GARDEL_KEY_DOWN}, {"Left", GARDEL_KEY_LEFT},
    {"Right", GARDEL_KEY_RIGHT}, {"Enter", GARDEL_KEY_ENTER}, {"Escape", GARDEL_KEY_ESCAPE},
    {"Backspace", GARDEL_KEY_BACKSPACE}, {"Tab", GARDEL_KEY_TAB}, {"Other", GARDEL_KEY_OTHER},
    {"KeysEnded", GARDEL_KEY_END_OF_INPUT},
};

/* The event log (spec 11.6). GARDEL_RECORD writes one, and GARDEL_REPLAY runs one through the
   scripted source. Lines in order of occurrence, each text %XX-escaped so that a line is a line:

       gardel-log 1 ECHO_SERVER
       arg some%20argument
       command Listen:8888
       event Accepted:1
       event Received:1:hi
       command Send:1:hi
       exit 0

   Events are what makes a run repeatable, and the commands and the exit code are what a replay
   checks. A log ends wherever its program was killed, so a last line with no line break is
   dropped, and a replay that runs out of log gets the `Shutdown` of a script's end. */
#define GARDEL_LOG_VERSION "1"
#define GARDEL_REPLAY_DIVERGED 3
static FILE* gardel_log_file = NULL;
static char* gardel_log_line = NULL;
static size_t gardel_log_len = 0, gardel_log_cap = 0;
static bool gardel_replaying = false, gardel_replay_cut = false;
static const char* gardel_replay_pos = NULL;
static const char* gardel_replay_end = NULL;
static uint64_t gardel_replay_line_no = 0;

static bool gardel_log_active(void) { return gardel_log_file || gardel_replaying; }

static void gardel_log_put(const char* s, size_t n) {
    gardel_log_line = (char*)gardel_grow(gardel_log_line, &gardel_log_cap, gardel_log_len + n + 1, 1);
    memcpy(gardel_log_line + gardel_log_len, s, n);
    gardel_log_len += n;
}
static void gardel_log_word(const char* s) { gardel_log_put(s, strlen(s)); }
static void gardel_log_number(uint64_t v) {
    char t[24];
    gardel_log_put(t, (size_t)snprintf(t, sizeof t, "%llu", (unsigned long long)v));
}
static void gardel_log_text(const void* data, uint64_t n) {
    static const char hex[] = "0123456789ABCDEF";
    const unsigned char* p = (const unsigned char*)data;
    gardel_log_line = (char*)gardel_grow(gardel_log_line, &gardel_log_cap, gardel_log_len + 3 * (size_t)n + 1, 1);
    char* w = gardel_log_line + gardel_log_len;
    for (uint64_t i = 0; i < n; i++) {
        unsigned char c = p[i];
        if ((c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z')) {
            *w++ = (char)c;
        } else {
            *w++ = '%';
            *w++ = hex[c >> 4];
            *w++ = hex[c & 15];
        }
    }
    gardel_log_len = (size_t)(w - gardel_log_line);
}

/* A full disk is no reason to end a server. What was written is a good log of a run cut short. */
static void gardel_log_lost(void) {
    fprintf(stderr, "warning: GARDEL_RECORD: cannot write the log, so recording stops here\n");
    fclose(gardel_log_file);
    gardel_log_file = NULL;
}

/* The next line of the log. False once it has run out, which a replay says once. */
static bool gardel_replay_next(const char** line, size_t* n) {
    size_t left = (size_t)(gardel_replay_end - gardel_replay_pos);
    const char* eol = left ? (const char*)memchr(gardel_replay_pos, '\n', left) : NULL;
    if (!eol) {
        if (!gardel_replay_cut) fprintf(stderr, "replay: the log ends before the run does, so the rest is not checked\n");
        gardel_replay_cut = true;
        return false;
    }
    *line = gardel_replay_pos;
    *n = (size_t)(eol - gardel_replay_pos);
    gardel_replay_pos = eol + 1;
    gardel_replay_line_no++;
    return true;
}

static void gardel_replay_show(const char* label, const char* s, size_t n) {
    fprintf(stderr, "  %s %.*s%s\n", label, (int)(n > 120 ? 120 : n), s, n > 120 ? "..." : "");
}
static void gardel_replay_diverged(const char* line, size_t n, const char* run, size_t run_n) {
    fflush(stdout);
    fprintf(stderr, "error: replay: the run differs from the log at line %llu\n", (unsigned long long)gardel_replay_line_no);
    gardel_replay_show("log:", line, n);
    gardel_replay_show("run:", run, run_n);
    exit(GARDEL_REPLAY_DIVERGED);
}

/* The line just built: written when recording, and compared with the log when replaying. */
static void gardel_log_finish(bool flush) {
    if (gardel_log_file) {
        gardel_log_put("\n", 1);
        if (fwrite(gardel_log_line, 1, gardel_log_len, gardel_log_file) != gardel_log_len
            || (flush && fflush(gardel_log_file) != 0)) gardel_log_lost();
    } else if (gardel_replaying) {
        const char* line;
        size_t n;
        if (gardel_replay_next(&line, &n) && (n != gardel_log_len || memcmp(line, gardel_log_line, n) != 0)) {
            gardel_replay_diverged(line, n, gardel_log_line, gardel_log_len);
        }
    }
    gardel_log_len = 0;
}

/* After a batch of commands, before the wait. */
static void gardel_log_flush(void) {
    if (gardel_log_file && fflush(gardel_log_file) != 0) gardel_log_lost();
}

static void gardel_log_refused(const char* source, const char* what) {
    fprintf(stderr, "error: %s: %s\n", source, what);
    exit(2);
}

static FILE* gardel_log_create(const char* path) {
#ifdef _WIN32
    return fopen(path, "wb");
#else
    /* A log holds what clients sent and what was typed, so it is the owner's alone. */
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    return fd < 0 ? NULL : fdopen(fd, "wb");
#endif
}

static char* gardel_log_load(const char* path, size_t* len) {
    FILE* f = fopen(path, "rb");
    if (!f) return NULL;
    size_t cap = 65536, n = 0, got;
    char* buf = (char*)malloc(cap);
    if (!buf) abort();
    while ((got = fread(buf + n, 1, cap - n, f)) > 0) {
        n += got;
        if (n == cap) {
            cap *= 2;
            buf = (char*)realloc(buf, cap);
            if (!buf) abort();
        }
    }
    bool failed = ferror(f) != 0;
    fclose(f);
    if (failed) {
        free(buf);
        return NULL;
    }
    *len = n;
    return buf;
}

/* A replay runs with the arguments of its run: they are an input like the events. */
static void gardel_replay_args(int* argn, char*** argu) {
    if (*argn > 1) gardel_log_refused("GARDEL_REPLAY", "a replay takes its arguments from the log");
    int n = 1;
    char** u = (char**)malloc(sizeof(char*));
    if (!u) abort();
    u[0] = NULL;
    while (gardel_replay_end - gardel_replay_pos >= 4 && memcmp(gardel_replay_pos, "arg ", 4) == 0) {
        const char* line;
        size_t len, got;
        if (!gardel_replay_next(&line, &len)) break;
        char* text = gardel_script_text(line, len, 4, &got);
        if (!text) gardel_log_refused("GARDEL_REPLAY", "an argument in the log is malformed");
        text[got] = '\0';
        u = (char**)realloc(u, sizeof(char*) * (size_t)(n + 1));
        if (!u) abort();
        u[n++] = text;
    }
    *argn = n;
    *argu = u;
}

/* Before anything else that can be observed: `init` may already print. */
static void gardel_log_start(const char* program, int* argn, char*** argu) {
    const char* record = getenv("GARDEL_RECORD");
    const char* replay = getenv("GARDEL_REPLAY");
    if (replay && (record || gardel_host_script)) {
        gardel_log_refused("GARDEL_REPLAY", "it cannot be used with GARDEL_RECORD or GARDEL_EVENTS");
    }
    gardel_log_word("gardel-log " GARDEL_LOG_VERSION " ");
    gardel_log_word(program);
    if (record) {
        gardel_log_capture = true;
        gardel_log_file = gardel_log_create(record);
        if (!gardel_log_file) gardel_log_refused("GARDEL_RECORD", "cannot create the log");
        gardel_log_finish(true);
    } else if (replay) {
        size_t len = 0;
        char* text = gardel_log_load(replay, &len);
        if (!text) gardel_log_refused("GARDEL_REPLAY", "cannot read the log");
        const char* line;
        size_t n;
        gardel_replay_pos = text;
        gardel_replay_end = text + len;
        if (!gardel_replay_next(&line, &n) || n != gardel_log_len || memcmp(line, gardel_log_line, n) != 0) {
            gardel_log_refused("GARDEL_REPLAY", "this is not a log of this program");
        }
        /* Scripted: timers, reads and sockets do not run. */
        gardel_host_script = "";
        gardel_event_source = "GARDEL_REPLAY";
        gardel_replaying = true;
        gardel_replay_args(argn, argu);
    }
    gardel_log_len = 0;
}

static void gardel_log_args(int argn, char** argu) {
    if (!gardel_log_file) return;
    for (int i = 1; i < argn; i++) {
        gardel_log_word("arg ");
        gardel_log_text(argu[i], strlen(argu[i]));
        gardel_log_finish(false);
    }
}

/* Called as an event is delivered, so the order in the log is the order `step` saw. The line
   reaches the disk before `step` runs: the event that breaks a run is the one to keep. */
static void gardel_log_event(struct gardel_host_event* ev) {
    if (gardel_log_file) {
        gardel_log_word("event ");
        switch (ev->kind) {
        case GARDEL_HOST_SHUTDOWN: gardel_log_word("Shutdown"); break;
        case GARDEL_HOST_TICK: gardel_log_word("Tick:"); gardel_log_number(ev->id); break;
        case GARDEL_HOST_LINE:
            if (ev->line_state == GARDEL_LINE_ENDED) {
                gardel_log_word("LineEnd");
            } else if (ev->line_state == GARDEL_LINE_FAILED) {
                gardel_log_word("LineFailed:");
                gardel_log_number((uint64_t)ev->err);
            } else {
                gardel_log_word("Line:");
                gardel_log_text(ev->raw, ev->raw_len);
            }
            break;
        case GARDEL_HOST_KEY:
            gardel_log_word("Key:");
            if (ev->key == GARDEL_KEY_CHAR) {
                gardel_log_number(ev->ch);
            } else {
                size_t k = 0;
                while (gardel_key_names[k].key != ev->key) k++;
                gardel_log_word(gardel_key_names[k].name);
            }
            break;
        case GARDEL_HOST_ACCEPTED: gardel_log_word("Accepted:"); gardel_log_number(ev->id); break;
        case GARDEL_HOST_CLOSED: gardel_log_word("Closed:"); gardel_log_number(ev->id); break;
        case GARDEL_HOST_RECEIVED:
            gardel_log_word("Received:");
            gardel_log_number(ev->id);
            gardel_log_word(":");
            gardel_log_text(ev->data.data, ev->data.len);
            break;
        case GARDEL_HOST_LISTEN_FAILED:
        case GARDEL_HOST_SEND_FAILED:
            gardel_log_word(ev->kind == GARDEL_HOST_SEND_FAILED ? "SendFailed:" : "ListenFailed:");
            gardel_log_number(ev->id);
            gardel_log_word(":");
            gardel_log_number((uint64_t)ev->err);
            break;
        default: abort();
        }
        gardel_log_finish(true);
    }
    free(ev->raw);
    ev->raw = NULL;
}

static void gardel_log_exit(int code) {
    if (!gardel_log_active()) return;
    gardel_log_word("exit ");
    gardel_log_number((uint32_t)code);
    gardel_log_finish(true);
    if (gardel_log_file && fclose(gardel_log_file) != 0) {
        fprintf(stderr, "warning: GARDEL_RECORD: cannot finish the log\n");
    }
    gardel_log_file = NULL;
}

/* Socket tokens of a script: `Accepted:<id>`, `Closed:<id>`, `Received:<id>:<text>`,
   `ListenFailed:<port>:<code>`, `SendFailed:<id>:<code>`. */
static bool gardel_script_net(const char* name, size_t n, struct gardel_host_event* ev) {
    static const struct { const char* name; enum gardel_host_kind kind; } kinds[] = {
        {"Accepted:", GARDEL_HOST_ACCEPTED}, {"Closed:", GARDEL_HOST_CLOSED},
        {"Received:", GARDEL_HOST_RECEIVED}, {"ListenFailed:", GARDEL_HOST_LISTEN_FAILED},
        {"SendFailed:", GARDEL_HOST_SEND_FAILED},
    };
    for (size_t k = 0; k < sizeof kinds / sizeof kinds[0]; k++) {
        size_t i = strlen(kinds[k].name);
        if (n <= i || memcmp(name, kinds[k].name, i) != 0) continue;
        uint64_t id, code;
        if (!gardel_script_number(name, n, &i, &id)) return false;
        if (kinds[k].kind == GARDEL_HOST_ACCEPTED || kinds[k].kind == GARDEL_HOST_CLOSED) {
            if (i != n) return false;
        } else {
            if (i == n || name[i] != ':') return false;
            i++;
            if (kinds[k].kind == GARDEL_HOST_RECEIVED) {
                size_t len;
                char* text = gardel_script_text(name, n, i, &len);
                if (!text) return false;
                ev->data = gardel_bytes_copy(text, len);
                free(text);
            } else {
                if (!gardel_script_number(name, n, &i, &code) || i != n) return false;
                ev->err = (int64_t)code;
            }
        }
        ev->kind = kinds[k].kind;
        ev->id = id;
        return true;
    }
    return false;
}

static struct gardel_host_event gardel_next_scripted_event(void) {
    struct gardel_host_event ev = { GARDEL_HOST_SHUTDOWN, 0, NULL, GARDEL_KEY_OTHER, 0, {NULL, 0, NULL}, 0 };
    const char* name;
    size_t n;
    /* The end of a script or of a log is a shutdown: `step` is proven to stop on it, so a
       scripted run ends. */
    if (gardel_replaying) {
        if (!gardel_replay_next(&name, &n)) return ev;
        if (n < 6 || memcmp(name, "event ", 6) != 0) {
            static const char waiting[] = "waiting for an event";
            gardel_replay_diverged(name, n, waiting, sizeof waiting - 1);
        }
        name += 6;
        n -= 6;
    } else {
        while (*gardel_host_script == ' ') gardel_host_script++;
        if (!*gardel_host_script) return ev;
        n = strcspn(gardel_host_script, " ");
        name = gardel_host_script;
        gardel_host_script += n;
    }
    if (n == 8 && memcmp(name, "Shutdown", 8) == 0) return ev;
    if (gardel_script_net(name, n, &ev)) return ev;
    if (n > 5 && memcmp(name, "Tick:", 5) == 0) {
        size_t i = 5;
        if (gardel_script_number(name, n, &i, &ev.id) && i == n) {
            ev.kind = GARDEL_HOST_TICK;
            return ev;
        }
    }
    if (n >= 5 && memcmp(name, "Line:", 5) == 0) {
        size_t len;
        char* buf = gardel_script_text(name, n, 5, &len);
        if (buf) {
            ev.kind = GARDEL_HOST_LINE;
            gardel_keep_line(&ev, buf, len);
            ev.line = gardel_line_from_bytes(buf, len);
            return ev;
        }
    }
    if (n == 7 && memcmp(name, "LineEnd", 7) == 0) {
        ev.kind = GARDEL_HOST_LINE;
        ev.line_state = GARDEL_LINE_ENDED;
        ev.line = gardel_line_at_end();
        return ev;
    }
    if (n > 11 && memcmp(name, "LineFailed:", 11) == 0) {
        size_t i = 11;
        uint64_t code;
        if (gardel_script_number(name, n, &i, &code) && i == n) {
            ev.kind = GARDEL_HOST_LINE;
            ev.line_state = GARDEL_LINE_FAILED;
            ev.err = (int64_t)code;
            ev.line = gardel_line_failed(ev.err);
            return ev;
        }
    }
    /* Key:<name>, or Key:<scalar> for a character. */
    if (n > 4 && memcmp(name, "Key:", 4) == 0) {
        ev.kind = GARDEL_HOST_KEY;
        for (size_t k = 0; k < sizeof gardel_key_names / sizeof gardel_key_names[0]; k++) {
            if (strlen(gardel_key_names[k].name) == n - 4 && memcmp(name + 4, gardel_key_names[k].name, n - 4) == 0) {
                ev.key = gardel_key_names[k].key;
                return ev;
            }
        }
        uint64_t c = 0;
        size_t i = 4;
        while (i < n && name[i] >= '0' && name[i] <= '9' && c <= 0x10FFFF) c = c * 10 + (uint64_t)(name[i++] - '0');
        if (i == n && c <= 0x10FFFF && !(c >= 0xD800 && c <= 0xDFFF)) {
            ev.key = GARDEL_KEY_CHAR;
            ev.ch = (uint32_t)c;
            return ev;
        }
    }
    fprintf(stderr, "error: %s: unknown event `%.*s`\n", gardel_event_source, (int)n, name);
    exit(2);
}

static struct gardel_host_event gardel_next_event(void) {
    if (gardel_host_script) return gardel_next_scripted_event();
    for (;;) {
        /* Before anything else: a program re-arming a zero timer cannot starve a shutdown. */
        struct gardel_host_event ev = { GARDEL_HOST_SHUTDOWN, 0, NULL, GARDEL_KEY_OTHER, 0, {NULL, 0, NULL}, 0 };
        if (gardel_host_stop_pending()) return ev;
        if (gardel_take_answer(&ev)) return ev;
        /* Sockets and timers take turns going first, so neither can starve the other. */
        gardel_net_turn = !gardel_net_turn;
        if (gardel_net_turn && gardel_net_take(&ev)) return ev;
        uint64_t wait = UINT64_MAX;
        if (gardel_timer_count > 0) {
            size_t first = 0;
            for (size_t i = 1; i < gardel_timer_count; i++) {
                const struct gardel_timer* a = &gardel_timers[i];
                const struct gardel_timer* b = &gardel_timers[first];
                if (a->deadline < b->deadline || (a->deadline == b->deadline && a->seq < b->seq)) first = i;
            }
            uint64_t now = gardel_host_now_ms();
            if (gardel_timers[first].deadline <= now) {
                ev.kind = GARDEL_HOST_TICK;
                ev.id = gardel_timers[first].id;
                gardel_timers[first] = gardel_timers[--gardel_timer_count];
                return ev;
            }
            wait = gardel_timers[first].deadline - now;
        }
        if (!gardel_net_turn && gardel_net_take(&ev)) return ev;
        gardel_host_wait(gardel_net_wait_limit(wait));
    }
}
"#;

/// What the host does for a `COMMAND` variant, bound as `c`. `None` stops the build: a variant
/// with no code here must not reach a program, or the runtime would ignore it.
pub(crate) fn host_command_code(variant: &str) -> Option<&'static str> {
    match variant {
        "Print" => Some("gardel_io_print_line(c->Print.text);"),
        "SetTimer" => Some("gardel_host_set_timer(c->SetTimer.id, c->SetTimer.millis);"),
        "ReadLine" => Some("gardel_host_request_input(false);"),
        "ReadKey" => Some("gardel_host_request_input(true);"),
        "Listen" => Some("gardel_host_listen(c->Listen.port);"),
        "Send" => Some("gardel_host_send(c->Send.conn->id, c->Send.data);"),
        "Close" => Some("gardel_host_close(c->Close.conn->id);"),
        _ => None,
    }
}

/// How a `COMMAND` variant, bound as `c`, is written in the event log, so a replay can compare
/// it. `None` stops the build, as for `host_command_code`: a command left out of the log would
/// be one that replay does not check.
pub(crate) fn host_command_log(variant: &str) -> Option<&'static str> {
    match variant {
        "Print" => Some("gardel_log_word(\"Print:\"); gardel_log_text(c->Print.text.data, c->Print.text.len);"),
        "SetTimer" => Some("gardel_log_word(\"SetTimer:\"); gardel_log_number(c->SetTimer.id); gardel_log_word(\":\"); gardel_log_number(c->SetTimer.millis);"),
        "ReadLine" => Some("gardel_log_word(\"ReadLine\");"),
        "ReadKey" => Some("gardel_log_word(\"ReadKey\");"),
        "Listen" => Some("gardel_log_word(\"Listen:\"); gardel_log_number(c->Listen.port);"),
        "Send" => Some("gardel_log_word(\"Send:\"); gardel_log_number(c->Send.conn->id); gardel_log_word(\":\"); gardel_log_text(c->Send.data.data, c->Send.data.len);"),
        "Close" => Some("gardel_log_word(\"Close:\"); gardel_log_number(c->Close.conn->id);"),
        _ => None,
    }
}

/// The `switch` case that builds an `EVENT` variant from the host's `ev`.
pub(crate) fn host_event_code(variant: &str) -> Option<&'static str> {
    match variant {
        "Shutdown" => Some("case GARDEL_HOST_SHUTDOWN: return make_EVENT_Shutdown();"),
        "Tick" => Some("case GARDEL_HOST_TICK: return make_EVENT_Tick(ev.id);"),
        "Line" => Some("case GARDEL_HOST_LINE: return make_EVENT_Line((struct RESULT_IO_ERROR_STRING*)ev.line);"),
        "Key" => Some("case GARDEL_HOST_KEY: return make_EVENT_Key(gardel_key_value(ev));"),
        "Accepted" => Some("case GARDEL_HOST_ACCEPTED: return make_EVENT_Accepted(make_CONN_ID(ev.id));"),
        "ListenFailed" => Some("case GARDEL_HOST_LISTEN_FAILED: return make_EVENT_ListenFailed(ev.id, make_IO_ERROR_OsError(ev.err));"),
        "Received" => Some("case GARDEL_HOST_RECEIVED: return make_EVENT_Received(make_CONN_ID(ev.id), ev.data);"),
        "SendFailed" => Some("case GARDEL_HOST_SEND_FAILED: return make_EVENT_SendFailed(make_CONN_ID(ev.id), make_IO_ERROR_OsError(ev.err));"),
        "Closed" => Some("case GARDEL_HOST_CLOSED: return make_EVENT_Closed(make_CONN_ID(ev.id));"),
        _ => None,
    }
}

/// The `switch` case that builds a `KEY` variant from the host's `ev`.
pub(crate) fn host_key_code(variant: &str) -> Option<&'static str> {
    match variant {
        "Char" => Some("case GARDEL_KEY_CHAR: return make_KEY_Char(ev.ch);"),
        "Up" => Some("case GARDEL_KEY_UP: return make_KEY_Up();"),
        "Down" => Some("case GARDEL_KEY_DOWN: return make_KEY_Down();"),
        "Left" => Some("case GARDEL_KEY_LEFT: return make_KEY_Left();"),
        "Right" => Some("case GARDEL_KEY_RIGHT: return make_KEY_Right();"),
        "Enter" => Some("case GARDEL_KEY_ENTER: return make_KEY_Enter();"),
        "Escape" => Some("case GARDEL_KEY_ESCAPE: return make_KEY_Escape();"),
        "Backspace" => Some("case GARDEL_KEY_BACKSPACE: return make_KEY_Backspace();"),
        "Tab" => Some("case GARDEL_KEY_TAB: return make_KEY_Tab();"),
        "Other" => Some("case GARDEL_KEY_OTHER: return make_KEY_Other();"),
        "KeysEnded" => Some("case GARDEL_KEY_END_OF_INPUT: return make_KEY_KeysEnded();"),
        _ => None,
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
                for (_, t) in &mr.params {
                    let t = cnorm(t);
                    if t == "SEQ[STRING]" {
                        match b.args_runtime() {
                            Ok(rt) => {
                                args_rt = rt;
                                setup.push_str("    struct SEQ_STRING* args = gardel_main_args(argc, argv);\n");
                                call_args.push("args".to_string());
                            }
                            Err(e) => b.errors.push(e),
                        }
                    } else {
                        b.errors.push(format!("`main` parameter of type `{t}` is not supported"));
                    }
                }
                b.drain();
                entry = format!(
                    "int main(int argc, char** argv) {{\n    (void)argc;\n    (void)argv;\n{setup}    int64_t rc = 0;\n    #pragma omp parallel\n    #pragma omp single\n    rc = (int64_t){cname}({});\n    return (int)rc;\n}}\n",
                    call_args.join(", ")
                );
            }
            None => b.errors.push("program has no `main`".into()),
        }
    } else if root.kind == ModuleKind::ReactiveProgram {
        match b.reactive_host(root) {
            Ok((host, rt)) => {
                entry = host;
                args_rt = rt;
            }
            Err(e) => b.errors.push(e),
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
