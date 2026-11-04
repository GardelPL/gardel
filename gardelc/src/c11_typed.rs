//! Typed C11 backend: lowers Gardel modules and programs to C from their source.
//!
//! Unlike the untyped emitter (`emit_c11_general_module`), this backend infers the
//! Gardel type of every expression, so it can
//!   * name every routine instance after its owner type (`SEQ_U64__concat`),
//!   * instantiate generic modules and routine-level type parameters on demand
//!     (`SEQ[A].fold_left[B]` at `A = U64, B = U64` becomes `SEQ_U64__fold_left__U64`),
//!   * lower constructors, `match` (with field binders and arm-local `let`s), record
//!     literals and updates, function values and non-capturing lambdas, and
//!   * provide a C runtime for the specification type `LIST[T]` where executable code
//!     uses it (e.g. `SEQ.to_list`, `SEQ.equals`).
//! Every module instance a program uses is emitted into the program's translation unit.
//! Independent `let` groups become OpenMP tasks (spec §10) with a depth cutoff.
//! `STRING`, `BYTES` and `IO` are provided by the C runtime header, not compiled.
//!
//! Anything the backend cannot lower is reported, and the emitted C then starts with
//! `#error`, so it never compiles into something that silently differs from the source.

use super::*;
use std::collections::VecDeque;

type Lowered = Result<(String, String), String>;

/// Marker error: the expression needs an expected type (e.g. `LIST.empty`).
const NEED_TYPE: &str = "\u{0}need-type";

const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else",
    "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long", "register",
    "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch",
    "typedef", "union", "unsigned", "void", "volatile", "while", "bool", "true", "false",
    "main", "exit", "abort", "free", "malloc", "printf", "Current", "NULL",
];

/// Canonical spelling of a Gardel type: `RESULT[IO_ERROR,STRING]` and
/// `RESULT[IO_ERROR, STRING]` agree, and function types drop parameter names
/// (`(acc: B, x: A) -> B` becomes `(B, A) -> B`).
pub(crate) fn norm_type(t: &str) -> String {
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
                norm_type(ty)
            })
            .collect();
        let r = r.split(" require ").next().unwrap_or(r);
        let r = r.split(" ensure ").next().unwrap_or(r);
        return format!("({}) -> {}", ps.join(", "), norm_type(r));
    }
    let (h, args) = split_type_app(t);
    if args.is_empty() {
        h.to_string()
    } else {
        let a: Vec<String> = args.iter().map(|x| norm_type(x)).collect();
        format!("{h}[{}]", a.join(", "))
    }
}

/// Parameter and result types of a (normalized) function type.
pub(crate) fn fn_parts(t: &str) -> Option<(Vec<String>, String)> {
    let (a, r) = split_top_level_arrow(t)?;
    let a = a.trim();
    let inner = a
        .strip_prefix('(')
        .and_then(|x| x.strip_suffix(')'))
        .unwrap_or(a);
    let ps = split_top_level_commas(inner)
        .iter()
        .map(|p| norm_type(p))
        .collect();
    Some((ps, norm_type(r)))
}

fn subst(t: &str, map: &HashMap<String, String>) -> String {
    norm_type(&subst_type_idents(t, map))
}

fn mentions_any(t: &str, names: &[String]) -> bool {
    type_tokens(t).iter().any(|tok| names.iter().any(|n| n == tok))
}

/// Unifies `pattern` (which may mention the type parameters `tps`) with `actual`.
fn unify(pattern: &str, actual: &str, tps: &[String], map: &mut HashMap<String, String>) {
    let p = norm_type(pattern);
    let a = norm_type(actual);
    if tps.iter().any(|t| *t == p) {
        map.entry(p).or_insert(a);
        return;
    }
    if let (Some((pp, pr)), Some((ap, ar))) = (fn_parts(&p), fn_parts(&a)) {
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
        });
        cname
    }
}

struct FnReq {
    module: String,
    mod_targs: Vec<String>,
    routine: String,
    rtargs: Vec<String>,
    cname: String,
}

pub(crate) struct TypedC<'a> {
    mods: Vec<&'a ModuleDecl>,
    root: &'a ModuleDecl,
    struct_fwd: Vec<String>,
    fn_typedefs: Vec<String>,
    type_defs: Vec<String>,
    protos: Vec<String>,
    bodies: Vec<String>,
    done_types: HashSet<String>,
    done_fns: HashSet<String>,
    done_helpers: HashSet<String>,
    typedefs: HashMap<String, String>,
    queue: VecDeque<FnReq>,
    counter: usize,
    needs_read_line: bool,
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
            protos: Vec::new(),
            bodies: Vec::new(),
            done_types: HashSet::new(),
            done_fns: HashSet::new(),
            done_helpers: HashSet::new(),
            typedefs: HashMap::new(),
            queue: VecDeque::new(),
            counter: 0,
            needs_read_line: false,
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
        let ty = norm_type(ty);
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
            t if fn_parts(t).is_some() => self.fn_typedef(t)?,
            t if type_head(t) == "LIST" => {
                let elem = split_type_app(t).1.first().map(|s| norm_type(s)).unwrap_or_default();
                self.ensure_list(&elem)?;
                format!("struct {}*", c_instance_name(t))
            }
            t => {
                self.ensure_adt(t)?;
                format!("struct {}*", c_instance_name(t))
            }
        })
    }

    fn fn_typedef(&mut self, t: &str) -> Result<String, String> {
        if let Some(n) = self.typedefs.get(t) {
            return Ok(n.clone());
        }
        let (ps, r) = fn_parts(t).ok_or_else(|| format!("bad function type `{t}`"))?;
        let name = format!("gfn_{}", c_instance_name(&format!("{}_to_{r}", ps.join("_"))));
        self.typedefs.insert(t.to_string(), name.clone());
        let mut cps = Vec::new();
        for p in &ps {
            cps.push(self.ctype(p)?);
        }
        let cr = self.ctype(&r)?;
        let cps = if cps.is_empty() { "void".to_string() } else { cps.join(", ") };
        self.fn_typedefs.push(format!("typedef {cr} (*{name})({cps});\n"));
        Ok(name)
    }

    /// Struct definition and constructors of a module instance (`SEQ[U64]`, `IO_ERROR`).
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
        let map: HashMap<String, String> = md
            .type_params
            .iter()
            .cloned()
            .zip(args.iter().map(|a| norm_type(a)))
            .collect();
        self.done_types.insert(inst.clone());
        self.struct_fwd.push(format!("struct {inst};\n"));
        let mut def = String::new();
        let self_ty = norm_type(ty);
        let fix = |t: &str| -> String {
            let t = if t.trim() == "Current" { self_ty.clone() } else { t.to_string() };
            subst(&t, &map)
        };
        match &md.repr {
            ReprKind::Product(fields) => {
                let mut cf = Vec::new();
                for (f, t) in fields {
                    cf.push((f.clone(), self.ctype(&fix(t))?));
                }
                def.push_str(&format!("struct {inst} {{\n"));
                for (f, c) in &cf {
                    def.push_str(&format!("    {c} {f};\n"));
                }
                if cf.is_empty() {
                    def.push_str("    char _unused;\n");
                }
                def.push_str("};\n");
                let params: Vec<String> = cf.iter().map(|(f, c)| format!("{c} {f}")).collect();
                def.push_str(&format!(
                    "static inline struct {inst}* make_{inst}({}) {{\n    struct {inst}* r = (struct {inst}*)gardel_alloc(sizeof(struct {inst}));\n",
                    if params.is_empty() { "void".to_string() } else { params.join(", ") }
                ));
                for (f, _) in &cf {
                    def.push_str(&format!("    r->{f} = {f};\n"));
                }
                def.push_str("    return r;\n}\n\n");
            }
            ReprKind::Sum(vs) => {
                let mut cvs = Vec::new();
                for v in vs {
                    let mut cf = Vec::new();
                    for (f, t) in &v.fields {
                        cf.push((f.clone(), self.ctype(&fix(t))?));
                    }
                    cvs.push((v.name.clone(), cf));
                }
                let tags: Vec<String> = cvs.iter().map(|(v, _)| format!("TAG_{inst}_{v}")).collect();
                def.push_str(&format!("struct {inst} {{\n    enum {{ {} }} tag;\n", tags.join(", ")));
                if cvs.iter().any(|(_, f)| !f.is_empty()) {
                    def.push_str("    union {\n");
                    for (v, cf) in &cvs {
                        if !cf.is_empty() {
                            def.push_str("        struct {");
                            for (f, c) in cf {
                                def.push_str(&format!(" {c} {f};"));
                            }
                            def.push_str(&format!(" }} {v};\n"));
                        }
                    }
                    def.push_str("    };\n");
                }
                def.push_str("};\n");
                for (v, cf) in &cvs {
                    let params: Vec<String> = cf.iter().map(|(f, c)| format!("{c} {f}")).collect();
                    def.push_str(&format!(
                        "static inline struct {inst}* make_{inst}_{v}({}) {{\n    struct {inst}* r = (struct {inst}*)gardel_alloc(sizeof(struct {inst}));\n    r->tag = TAG_{inst}_{v};\n",
                        if params.is_empty() { "void".to_string() } else { params.join(", ") }
                    ));
                    for (f, _) in cf {
                        def.push_str(&format!("    r->{v}.{f} = {f};\n"));
                    }
                    def.push_str("    return r;\n}\n");
                }
                def.push('\n');
            }
        }
        self.type_defs.push(def);
        Ok(())
    }

    /// Fields of a module instance's variant (or of its product) with concrete types.
    fn variant_fields(&self, ty: &str, variant: Option<&str>) -> Option<Vec<(String, String)>> {
        let (head, args) = split_type_app(ty);
        let md = self.module(head)?;
        let map: HashMap<String, String> = md
            .type_params
            .iter()
            .cloned()
            .zip(args.iter().map(|a| norm_type(a)))
            .collect();
        let self_ty = norm_type(ty);
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

    /// `LIST[T]` runtime (an immutable cons list, `NULL` = empty). Iterative, so long
    /// lists do not exhaust the C stack.
    fn ensure_list(&mut self, elem: &str) -> Result<(), String> {
        let lt = format!("LIST[{elem}]");
        let inst = c_instance_name(&lt);
        if !self.done_types.insert(inst.clone()) {
            return Ok(());
        }
        let ce = self.ctype(elem)?;
        let eq = self.eq_code(elem, "a->head", "b->head")?;
        let eqx = self.eq_code(elem, "xs->head", "x")?;
        self.struct_fwd.push(format!("struct {inst};\n"));
        self.type_defs.push(format!(
            "struct {inst} {{ {ce} head; struct {inst}* tail; }};\n"
        ));
        self.protos.push(format!(
            "static struct {inst}* {inst}__prepend(struct {inst}* xs, {ce} x);\n\
             static struct {inst}* {inst}__append(struct {inst}* xs, struct {inst}* ys);\n\
             static uint64_t {inst}__length(struct {inst}* xs);\n\
             static bool {inst}__eq(struct {inst}* a, struct {inst}* b);\n\
             static bool {inst}__has(struct {inst}* xs, {ce} x);\n"
        ));
        self.bodies.push(format!(
            "/* LIST[{elem}] runtime: immutable cons list, NULL is the empty list */\n\
static struct {inst}* {inst}__prepend(struct {inst}* xs, {ce} x) {{\n\
    struct {inst}* c = (struct {inst}*)gardel_alloc(sizeof(struct {inst}));\n\
    c->head = x;\n    c->tail = xs;\n    return c;\n}}\n\
static struct {inst}* {inst}__append(struct {inst}* xs, struct {inst}* ys) {{\n\
    struct {inst}* out = ys;\n    struct {inst}** at = &out;\n\
    for (; xs; xs = xs->tail) {{\n\
        struct {inst}* c = (struct {inst}*)gardel_alloc(sizeof(struct {inst}));\n\
        c->head = xs->head;\n        c->tail = ys;\n        *at = c;\n        at = &c->tail;\n    }}\n\
    return out;\n}}\n\
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

    /// C expression testing `a = b` at type `ty` (structural, Leibniz equality).
    fn eq_code(&mut self, ty: &str, a: &str, b: &str) -> Result<String, String> {
        let ty = norm_type(ty);
        if is_scalar(&ty) {
            return Ok(format!("({a} == {b})"));
        }
        match ty.as_str() {
            "STRING" => return Ok(format!("gardel_string_equals({a}, {b})")),
            "BYTES" => return Ok(format!("gardel_bytes_equals({a}, {b})")),
            _ => {}
        }
        if fn_parts(&ty).is_some() {
            return Err(format!("equality on function type `{ty}` cannot be executed"));
        }
        if type_head(&ty) == "LIST" {
            self.ctype(&ty)?;
            return Ok(format!("{}__eq({a}, {b})", c_instance_name(&ty)));
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
        };
        cx.used.insert("Current".into());
        let mut params = Vec::new();
        if has_cur {
            params.push(format!("{} Current", self.ctype(&self_ty)?));
        }
        for (p, t) in &rd.params {
            let ty = subst(t, &tmap);
            let ct = self.ctype(&ty)?;
            let cn = cx.bind(p, &ty);
            params.push(format!("{ct} {cn}"));
        }
        let ret = subst(&rd.ret_type, &tmap);
        let cret = self.ctype(&ret)?;
        let (body, _) = self.lower_block(&mut cx, &rd.stmts, &rd.body, Some(&ret), has_cur)?;
        let sig = format!(
            "static {cret} {}({})",
            req.cname,
            if params.is_empty() { "void".to_string() } else { params.join(", ") }
        );
        self.protos.push(format!("{sig};\n"));
        self.bodies.push(format!(
            "/* {}.{}{} */\n{sig} {{\n    return {body};\n}}\n\n",
            self_ty,
            rd.name,
            if req.rtargs.is_empty() { String::new() } else { format!("[{}]", req.rtargs.join(", ")) }
        ));
        Ok(())
    }

    fn drain(&mut self) {
        while let Some(req) = self.queue.pop_front() {
            let what = format!("{}.{}", req.module, req.routine);
            if let Err(e) = self.emit_fn(req) {
                if e != NEED_TYPE {
                    self.errors.push(format!("{what}: {e}"));
                } else {
                    self.errors.push(format!("{what}: cannot infer a type"));
                }
            }
        }
    }

    // ------------------------------------------------------------ expressions

    fn lower_block(
        &mut self,
        cx: &mut Ctx,
        stmts: &[Stmt],
        body: &Expr,
        expect: Option<&str>,
        has_cur: bool,
    ) -> Lowered {
        let mark = cx.locals.len();
        let mut code = String::new();
        for group in analyze_parallel_lets(stmts) {
            if group.len() == 1 {
                let (v, e) = &group[0];
                let (c, t) = self.lower(cx, e, None, has_cur)?;
                let ct = self.ctype(&t)?;
                let cn = cx.bind(v, &t);
                code.push_str(&format!("{ct} {cn} = {c};\n"));
            } else {
                let mut lowered = Vec::new();
                for (v, e) in &group {
                    let (c, t) = self.lower(cx, e, None, has_cur)?;
                    let ct = self.ctype(&t)?;
                    lowered.push((v.clone(), c, t, ct));
                }
                let mut names = Vec::new();
                for (v, _, t, ct) in &lowered {
                    let cn = cx.bind(v, t);
                    code.push_str(&format!("{ct} {cn};\n"));
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
        let (b, t) = self.lower(cx, body, expect, has_cur)?;
        cx.locals.truncate(mark);
        if code.is_empty() {
            Ok((b, t))
        } else {
            Ok((format!("({{\n{code}{b};\n}})"), t))
        }
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
            let (a, _) = self.lower(cx, l, Some("BOOL"), hc)?;
            let (b, _) = self.lower(cx, r, Some("BOOL"), hc)?;
            let c = match op {
                "and" => format!("({a} && {b})"),
                "or" => format!("({a} || {b})"),
                _ => format!("(!{a} || {b})"),
            };
            return Ok((c, "BOOL".into()));
        }
        let comparison = matches!(op, "=" | "!=" | "<" | "<=" | ">" | ">=");
        let operand_expect = if comparison { None } else { expect };
        // Type the non-literal side first so an integer literal adopts its type.
        let ((a, ta), (b, tb)) = if matches!(l, Expr::Int(_)) && !matches!(r, Expr::Int(_)) {
            let rb = self.lower(cx, r, operand_expect, hc)?;
            let la = self.lower(cx, l, Some(&rb.1.clone()), hc)?;
            (la, rb)
        } else {
            let la = self.lower(cx, l, operand_expect, hc)?;
            let rb = self.lower(cx, r, Some(&la.1.clone()), hc)?;
            (la, rb)
        };
        let _ = tb;
        match op {
            "=" => Ok((self.eq_code(&ta, &a, &b)?, "BOOL".into())),
            "!=" => Ok((format!("(!{})", self.eq_code(&ta, &a, &b)?), "BOOL".into())),
            "<" | "<=" | ">" | ">=" => Ok((format!("({a} {op} {b})"), "BOOL".into())),
            "+%" | "-%" | "*%" => {
                let o = &op[..1];
                if is_signed(&ta) {
                    Ok((format!("((int64_t)((uint64_t)({a}) {o} (uint64_t)({b})))"), ta))
                } else {
                    Ok((format!("({a} {o} {b})"), ta))
                }
            }
            "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "<<" | ">>" => {
                Ok((format!("({a} {op} {b})"), ta))
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
                return Some(a.iter().map(|x| norm_type(x)).collect());
            }
        }
        if md.type_params.iter().all(|t| known.contains_key(t)) {
            return Some(md.type_params.iter().map(|t| known[t].clone()).collect());
        }
        if md.name == cx.owner {
            if let Some(st) = &cx.self_ty {
                return Some(split_type_app(st).1.iter().map(|x| norm_type(x)).collect());
            }
        }
        None
    }

    fn lower_ctor(&mut self, cx: &mut Ctx, md: &'a ModuleDecl, ctor: &str, args: &[Expr], expect: Option<&str>, hc: bool) -> Lowered {
        let ReprKind::Sum(vs) = &md.repr else { unreachable!() };
        let vd = vs.iter().find(|v| v.name == ctor).expect("variant");
        if vd.fields.len() != args.len() {
            return Err(format!("`{ctor}` takes {} arguments", vd.fields.len()));
        }
        // Infer the instance from the expected type, the context, or the arguments.
        let mut known = HashMap::new();
        let mut pre: Vec<Option<(String, String)>> = vec![None; args.len()];
        if self.instance_targs(md, expect, cx, &known).is_none() {
            for (i, ((_, ft), a)) in vd.fields.iter().zip(args).enumerate() {
                match self.lower(cx, a, None, hc) {
                    Ok(x) => {
                        unify(ft, &x.1, &md.type_params, &mut known);
                        pre[i] = Some(x);
                    }
                    Err(e) if e == NEED_TYPE => {}
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
            let c = match pre[i].take() {
                Some((c, _)) => c,
                None => self.lower(cx, a, Some(ft), hc)?.0,
            };
            cargs.push(c);
        }
        Ok((format!("make_{}_{ctor}({})", c_instance_name(&ty), cargs.join(", ")), ty))
    }

    /// A call of routine `r` of module `md` (`recv` is the receiver for routines on `Current`).
    #[allow(clippy::too_many_arguments)]
    fn lower_call(
        &mut self,
        cx: &mut Ctx,
        md: &'a ModuleDecl,
        r: &str,
        recv: Option<(String, String)>,
        args: &[Expr],
        expect: Option<&str>,
        hc: bool,
    ) -> Lowered {
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
        if let Some((_, rt)) = &recv {
            unify(&generic_self, rt, &tps, &mut map);
        } else if md.name == cx.owner && !md.type_params.is_empty() {
            // Unqualified or `M.r` call inside module M: same instance as the caller.
            for tp in &md.type_params {
                if let Some(c) = cx.tmap.get(tp) {
                    map.insert(tp.clone(), c.clone());
                }
            }
        }
        if let Some(e) = expect {
            unify(&fix_cur(&rd.ret_type), e, &tps, &mut map);
        }
        let mut lowered: Vec<Option<String>> = vec![None; args.len()];
        for (i, ((_, pt), a)) in rd.params.iter().zip(args).enumerate() {
            let pt = subst(&fix_cur(pt), &map);
            if !mentions_any(&pt, &tps) {
                lowered[i] = Some(self.lower(cx, a, Some(&pt), hc)?.0);
                continue;
            }
            match self.lower(cx, a, None, hc) {
                Ok((c, t)) => {
                    unify(&pt, &t, &tps, &mut map);
                    lowered[i] = Some(c);
                }
                Err(e) if e == NEED_TYPE => {}
                Err(e) => return Err(e),
            }
        }
        for (i, ((_, pt), a)) in rd.params.iter().zip(args).enumerate() {
            if lowered[i].is_none() {
                let pt = subst(&fix_cur(pt), &map);
                if mentions_any(&pt, &tps) {
                    return Err(NEED_TYPE.into());
                }
                lowered[i] = Some(self.lower(cx, a, Some(&pt), hc)?.0);
            }
        }
        if let Some(tp) = tps.iter().find(|t| !map.contains_key(*t)) {
            return Err(format!(
                "cannot infer type parameter `{tp}` of `{}.{r}`",
                md.name
            ));
        }
        let mod_targs: Vec<String> = md.type_params.iter().map(|t| map[t].clone()).collect();
        let rtargs: Vec<String> = rd.type_params.iter().map(|t| map[t].clone()).collect();
        let cname = self.request_fn(&md.name, &mod_targs, r, &rtargs);
        let mut cargs = Vec::new();
        if Self::has_current(md, kind) {
            match recv {
                Some((c, _)) => cargs.push(c),
                None if hc && md.name == cx.owner => cargs.push("Current".into()),
                None => return Err(format!("`{}.{r}` needs a receiver", md.name)),
            }
        }
        cargs.extend(lowered.into_iter().map(|c| c.unwrap_or_default()));
        let self_ty = Self::self_type(md, &mod_targs);
        map.insert("Current".into(), self_ty);
        let ret = subst(&rd.ret_type, &map);
        Ok((format!("{cname}({})", cargs.join(", ")), ret))
    }

    fn lower_text_method(&mut self, cx: &mut Ctx, rc: String, rt: &str, m: &str, args: &[Expr], hc: bool) -> Lowered {
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
        let mut a = vec![rc];
        for x in args {
            let exp = match (m, rt) {
                ("character_at", _) | ("at", _) => "NAT",
                _ => rt,
            };
            a.push(self.lower(cx, x, Some(exp), hc)?.0);
        }
        let prefix = if rt == "STRING" { "gardel_string_" } else { "gardel_bytes_" };
        Ok((format!("{prefix}{m}({})", a.join(", ")), ret.to_string()))
    }

    fn lower_list_method(&mut self, cx: &mut Ctx, rc: String, rt: &str, m: &str, args: &[Expr], hc: bool) -> Lowered {
        let elem = split_type_app(rt).1.first().map(|s| norm_type(s)).unwrap_or_default();
        self.ctype(rt)?;
        let inst = c_instance_name(rt);
        Ok(match (m, args.len()) {
            ("prepend", 1) => {
                let (x, _) = self.lower(cx, &args[0], Some(&elem), hc)?;
                (format!("{inst}__prepend({rc}, {x})"), rt.to_string())
            }
            ("append", 1) => {
                let (y, _) = self.lower(cx, &args[0], Some(rt), hc)?;
                (format!("{inst}__append({rc}, {y})"), rt.to_string())
            }
            ("has", 1) => {
                let (x, _) = self.lower(cx, &args[0], Some(&elem), hc)?;
                (format!("{inst}__has({rc}, {x})"), "BOOL".into())
            }
            ("length", 0) => (format!("{inst}__length({rc})"), "NAT".into()),
            ("is_empty", 0) => (format!("(({rc}) == NULL)"), "BOOL".into()),
            ("head", 0) => (format!("(({rc})->head)"), elem),
            ("tail", 0) => (format!("(({rc})->tail)"), rt.to_string()),
            _ => return Err(format!("`LIST.{m}` has no C runtime implementation")),
        })
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
        if let Some(v) = free.iter().find(|v| cx.local(v).is_some() || *v == "Current") {
            return Err(format!(
                "lambda captures `{v}`; capturing lambdas are not supported by the C backend yet"
            ));
        }
        let k = self.next();
        let name = format!("gardel_lambda_{k}");
        let mut lcx = Ctx {
            owner: cx.owner.clone(),
            self_ty: None,
            tmap: cx.tmap.clone(),
            locals: Vec::new(),
            used: HashSet::new(),
        };
        let mut cps = Vec::new();
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
        let (b, _) = self.lower_block(&mut lcx, stmts, body, Some(&rty), false)?;
        let sig = format!("static {cret} {name}({})", if cps.is_empty() { "void".to_string() } else { cps.join(", ") });
        self.protos.push(format!("{sig};\n"));
        self.bodies.push(format!("{sig} {{\n    return {b};\n}}\n\n"));
        let fty = norm_type(&format!("({}) -> {rty}", tys.join(", ")));
        self.ctype(&fty)?;
        Ok((name, fty))
    }

    fn lower_match(&mut self, cx: &mut Ctx, scrut: &Expr, cases: &[MatchCase], expect: Option<&str>, hc: bool) -> Lowered {
        let (sc, st) = self.lower(cx, scrut, None, hc)?;
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
        let mut result_ty: Option<String> = expect.map(|e| e.to_string());
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
                                let ct = self.ctype(ft)?;
                                let cn = cx.bind(b, ft);
                                pre.push_str(&format!("{ct} {cn} = {sv}->{ctor}.{f}; "));
                            }
                            _ => return Err("nested patterns are not supported by the C backend yet".into()),
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
                _ => return Err("this `match` pattern is not supported by the C backend yet".into()),
            };
            let (b, t) = self.lower_block(cx, &c.stmts, &c.body, result_ty.as_deref(), hc)?;
            if result_ty.is_none() {
                result_ty = Some(t);
            }
            cx.locals.truncate(mark);
            arms.push(format!("{label} {{ {pre}{rv} = {b}; break; }}\n"));
        }
        let rt = result_ty.ok_or("empty `match`")?;
        let crt = self.ctype(&rt)?;
        let mut s = format!("({{\n{sct} {sv} = {sc};\n{crt} {rv};\nswitch ({sv}->tag) {{\n");
        for a in arms {
            s.push_str(&a);
        }
        if !has_default {
            s.push_str("default: abort();\n");
        }
        s.push_str(&format!("}}\n{rv};\n}})"));
        Ok((s, rt))
    }

    fn lower(&mut self, cx: &mut Ctx, e: &Expr, expect: Option<&str>, hc: bool) -> Lowered {
        let expect_n = expect.map(norm_type);
        let expect = expect_n.as_deref();
        match e {
            Expr::Int(n) => {
                let ty = match expect {
                    Some(t) if is_scalar(t) && t != "BOOL" && t != "UNIT" => t.to_string(),
                    _ => "U64".to_string(),
                };
                Ok((Self::int_lit(*n, &ty), ty))
            }
            Expr::Float(f) => Ok((format!("{f:?}"), "F64".into())),
            Expr::Char(c) => Ok((format!("{c}U"), "CHAR".into())),
            Expr::Bool(b) => Ok(((if *b { "true" } else { "false" }).into(), "BOOL".into())),
            Expr::Unit => Ok(("UNIT_VAL".into(), "UNIT".into())),
            Expr::Str(s) => {
                let k = self.next();
                let bytes: Vec<String> = s.bytes().map(|b| b.to_string()).chain(std::iter::once("0".into())).collect();
                Ok((
                    format!(
                        "({{ static const uint8_t _s{k}[] = {{ {} }}; string_from_utf8(_s{k}, {}ULL); }})",
                        bytes.join(", "),
                        s.len()
                    ),
                    "STRING".into(),
                ))
            }
            Expr::Var(v) => {
                if let Some(l) = cx.local(v) {
                    return Ok((l.cname.clone(), l.ty.clone()));
                }
                if v == "Current" && hc {
                    return Ok(("Current".into(), cx.self_ty.clone().unwrap_or_default()));
                }
                if hc {
                    if let Some(st) = cx.self_ty.clone() {
                        if let Some(fs) = self.variant_fields(&st, None) {
                            if let Some((_, ft)) = fs.iter().find(|(f, _)| f == v) {
                                return Ok((format!("Current->{v}"), ft.clone()));
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
                            let ps: Vec<String> = rd.params.iter().map(|(_, t)| t.clone()).collect();
                            let fty = norm_type(&format!("({}) -> {}", ps.join(", "), rd.ret_type));
                            self.ctype(&fty)?;
                            return Ok((cname, fty));
                        }
                    }
                }
                Err(format!("unknown name `{v}`"))
            }
            Expr::UnaryNot(x) => {
                let (c, _) = self.lower(cx, x, Some("BOOL"), hc)?;
                Ok((format!("(!{c})"), "BOOL".into()))
            }
            Expr::UnaryNeg(x) => {
                let (c, t) = self.lower(cx, x, expect, hc)?;
                Ok((format!("(-{c})"), t))
            }
            Expr::BinOp(op, l, r) => self.lower_binop(cx, op, l, r, expect, hc),
            Expr::Call(f, args) => {
                if let Some(l) = cx.local(f).cloned() {
                    let (ps, r) = fn_parts(&l.ty).ok_or_else(|| format!("`{f}` is not a function"))?;
                    let mut a = Vec::new();
                    for (x, pt) in args.iter().zip(ps.iter()) {
                        a.push(self.lower(cx, x, Some(pt), hc)?.0);
                    }
                    return Ok((format!("{}({})", l.cname, a.join(", ")), r));
                }
                if let Some(md) = self.ctor_owner(f, expect, cx) {
                    return self.lower_ctor(cx, md, f, args, expect, hc);
                }
                let owner = self.module(&cx.owner).ok_or("no owner module")?;
                if Self::find_routine(owner, f).is_some() {
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
                        return self.lower_static(cx, v, m, args, expect, hc);
                    }
                }
                let (rc, rt) = match self.lower(cx, recv, None, hc) {
                    // `LIST.empty.prepend(x)`: these methods return the receiver's type,
                    // so the receiver takes the expected type of the whole call.
                    Err(e) if e == NEED_TYPE && matches!(m.as_str(), "prepend" | "append" | "tail" | "concat") => {
                        self.lower(cx, recv, expect, hc)?
                    }
                    r => r?,
                };
                if rt == "STRING" || rt == "BYTES" {
                    return self.lower_text_method(cx, rc, &rt, m, args, hc);
                }
                // `equals` on a type without its own `equals` routine (scalars,
                // `CANONICAL_EQ` elements) is Leibniz equality.
                if m == "equals" && args.len() == 1 && self.module(type_head(&rt)).is_none_or(|md| Self::find_routine(md, "equals").is_none()) && type_head(&rt) != "LIST" {
                    let (b, _) = self.lower(cx, &args[0], Some(&rt), hc)?;
                    return Ok((self.eq_code(&rt, &rc, &b)?, "BOOL".into()));
                }
                if rt == "CHAR" && m == "code" && args.is_empty() {
                    return Ok((format!("((uint64_t)({rc}))"), "NAT".into()));
                }
                if type_head(&rt) == "LIST" {
                    return self.lower_list_method(cx, rc, &rt, m, args, hc);
                }
                if let Some((ps, r)) = fn_parts(&rt) {
                    if m == "apply" {
                        let mut a = Vec::new();
                        for (x, pt) in args.iter().zip(ps.iter()) {
                            a.push(self.lower(cx, x, Some(pt), hc)?.0);
                        }
                        return Ok((format!("{rc}({})", a.join(", ")), r));
                    }
                }
                let md = self
                    .module(type_head(&rt))
                    .ok_or_else(|| format!("no C lowering for `.{m}` on `{rt}`"))?;
                if args.is_empty() {
                    if let Some(fs) = self.variant_fields(&rt, None) {
                        if let Some((_, ft)) = fs.iter().find(|(f, _)| f == m) {
                            self.ctype(&rt)?;
                            return Ok((format!("(({rc})->{m})"), ft.clone()));
                        }
                    }
                }
                self.lower_call(cx, md, m, Some((rc, rt)), args, expect, hc)
            }
            Expr::If(c, ts, te, es, ee) => {
                let (cc, _) = self.lower(cx, c, Some("BOOL"), hc)?;
                let (a, ta) = match self.lower_block(cx, ts, te, expect, hc) {
                    Ok(x) => x,
                    Err(e) if e == NEED_TYPE && expect.is_none() => {
                        let (b, tb) = self.lower_block(cx, es, ee, None, hc)?;
                        let (a, _) = self.lower_block(cx, ts, te, Some(&tb), hc)?;
                        return Ok((format!("({cc} ? ({a}) : ({b}))"), tb));
                    }
                    Err(e) => return Err(e),
                };
                let (b, _) = self.lower_block(cx, es, ee, Some(expect.unwrap_or(&ta)), hc)?;
                Ok((format!("({cc} ? ({a}) : ({b}))"), ta))
            }
            Expr::Match(scrut, cases) => self.lower_match(cx, scrut, cases, expect, hc),
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
                    a.push(self.lower(cx, fe, Some(ft), hc)?.0);
                }
                Ok((format!("make_{}({})", c_instance_name(&ty), a.join(", ")), ty))
            }
            Expr::RecordUpdate(base, fields) => {
                let (bc, bt) = self.lower(cx, base, expect, hc)?;
                let fs = self
                    .variant_fields(&bt, None)
                    .ok_or_else(|| format!("record update of non-record type `{bt}`"))?;
                let inst = c_instance_name(&bt);
                let k = self.next();
                let mut s = format!(
                    "({{ struct {inst}* _u{k} = (struct {inst}*)gardel_alloc(sizeof(struct {inst})); *_u{k} = *({bc}); "
                );
                for (f, fe) in fields {
                    let ft = fs
                        .iter()
                        .find(|(n, _)| n == f)
                        .map(|(_, t)| t.clone())
                        .ok_or_else(|| format!("`{bt}` has no field `{f}`"))?;
                    let (c, _) = self.lower(cx, fe, Some(&ft), hc)?;
                    s.push_str(&format!("_u{k}->{f} = {c}; "));
                }
                s.push_str(&format!("_u{k}; }})"));
                Ok((s, bt))
            }
            Expr::WithIo(stmts, fin) => {
                let mark = cx.locals.len();
                let mut s = String::from("({\n");
                for st in stmts {
                    match st {
                        IoStmt::Bind(v, x) => {
                            let (c, t) = self.lower(cx, x, None, hc)?;
                            let inner = t
                                .strip_prefix("IO[")
                                .and_then(|x| x.strip_suffix(']'))
                                .map(norm_type)
                                .unwrap_or(t.clone());
                            let ct = self.ctype(&inner)?;
                            let cn = cx.bind(v, &inner);
                            s.push_str(&format!("{ct} {cn} = {c};\n"));
                        }
                        IoStmt::Action(x) => {
                            let (c, _) = self.lower(cx, x, None, hc)?;
                            s.push_str(&format!("{c};\n"));
                        }
                    }
                }
                let (c, t) = self.lower(cx, fin, expect, hc)?;
                cx.locals.truncate(mark);
                s.push_str(&format!("{c};\n}})"));
                Ok((s, t))
            }
            Expr::Lambda(ps, rt, stmts, body) => self.lower_lambda(cx, ps, rt, stmts, body),
            Expr::TupleLit(_) => Err("tuples are not supported by the C backend yet".into()),
            Expr::Forall(..) | Expr::Exists(..) => {
                Err("quantifiers are specification-only and cannot be executed".into())
            }
        }
    }

    /// `T.r(args)` where `T` names a type or a runtime facility rather than a value.
    fn lower_static(&mut self, cx: &mut Ctx, v: &str, m: &str, args: &[Expr], expect: Option<&str>, hc: bool) -> Lowered {
        match (v, m, args.len()) {
            ("IO", "print", 1) | ("IO", "print_line", 1) => {
                let (a, _) = self.lower(cx, &args[0], Some("STRING"), hc)?;
                return Ok((format!("gardel_io_{m}({a})"), "IO[UNIT]".into()));
            }
            ("IO", "read_line", 0) => {
                self.needs_read_line = true;
                self.ctype(READ_LINE_RESULT_TYPE)?;
                return Ok((
                    "gardel_io_read_line_result()".into(),
                    format!("IO[{}]", norm_type(READ_LINE_RESULT_TYPE)),
                ));
            }
            ("IO", "exit", 1) => {
                let (a, _) = self.lower(cx, &args[0], Some("EXIT_CODE"), hc)?;
                return Ok((format!("gardel_io_exit({a})"), "IO[EXIT_CODE]".into()));
            }
            ("IO", _, _) => return Err(format!("`IO.{m}` has no C runtime implementation")),
            ("STRING", "from_utf8", 1) => {
                self.needs_from_utf8 = true;
                self.ctype(FROM_UTF8_RESULT_TYPE)?;
                let (a, _) = self.lower(cx, &args[0], Some("BYTES"), hc)?;
                return Ok((format!("gardel_string_from_utf8({a})"), norm_type(FROM_UTF8_RESULT_TYPE)));
            }
            ("STRING", _, _) | ("BYTES", _, _) => {
                return Err(format!("`{v}.{m}` has no C runtime implementation"));
            }
            ("CHAR", "from_scalar", 1) => {
                let (a, _) = self.lower(cx, &args[0], Some("NAT"), hc)?;
                return Ok((format!("((uint32_t)({a}))"), "CHAR".into()));
            }
            ("LIST", "empty", 0) => {
                let lt = expect.filter(|e| type_head(e) == "LIST").ok_or(NEED_TYPE.to_string())?;
                let ct = self.ctype(lt)?;
                return Ok((format!("(({ct})NULL)"), lt.to_string()));
            }
            ("LIST", "singleton", 1) => {
                let elem = expect
                    .filter(|e| type_head(e) == "LIST")
                    .and_then(|e| split_type_app(e).1.first().map(|s| norm_type(s)));
                let (x, t) = self.lower(cx, &args[0], elem.as_deref(), hc)?;
                let lt = format!("LIST[{t}]");
                self.ctype(&lt)?;
                return Ok((format!("{}__prepend(NULL, {x})", c_instance_name(&lt)), lt));
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

    fn assemble(&mut self, entry: &str) -> String {
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
             static _Thread_local int gardel_task_depth = 0;\n\n",
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
        if self.needs_read_line || self.needs_from_utf8 {
            out.push_str(&emit_c11_utf8_validator());
        }
        if self.needs_read_line {
            out.push_str(&emit_c11_read_line_runtime());
        }
        if self.needs_from_utf8 {
            out.push_str(&emit_c11_from_utf8_runtime());
        }
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
/// `args` is passed as an empty `SEQ[STRING]` (argv is not marshalled yet).
pub fn emit_c11_typed(root: &ModuleDecl, extra: &[&ModuleDecl]) -> (String, Vec<String>) {
    let mut b = TypedC::new(root, extra);
    let mut entry = String::new();
    if root.kind == ModuleKind::Program {
        match root.feature_routines.iter().find(|(_, r)| r.name == "main") {
            Some((_, mr)) => {
                let cname = b.request_fn(&root.name, &[], "main", &[]);
                let mut args = Vec::new();
                for (_, t) in &mr.params {
                    let t = norm_type(t);
                    if t == "SEQ[STRING]" {
                        match b.ctype(&t) {
                            Ok(_) => args.push(format!("make_{}_Empty()", c_instance_name(&t))),
                            Err(e) => b.errors.push(e),
                        }
                    } else {
                        b.errors.push(format!("`main` parameter of type `{t}` is not supported"));
                    }
                }
                b.drain();
                entry = format!(
                    "int main(int argc, char** argv) {{\n    (void)argc;\n    (void)argv;\n    int64_t rc = 0;\n    #pragma omp parallel\n    #pragma omp single\n    rc = (int64_t){cname}({});\n    return (int)rc;\n}}\n",
                    args.join(", ")
                );
            }
            None => b.errors.push("program has no `main`".into()),
        }
    } else if matches!(root.name.as_str(), "STRING" | "BYTES") {
        // Runtime-provided: the C runtime header implements these types (`GardelString`,
        // `GardelBytes`); their Gardel source is the verification model.
        return (
            format!(
                "// `{}` is provided by the gardelc C runtime header; its Gardel source is the
// verification model and is not compiled.
{}",
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
        if !root.type_params.is_empty() || !matches!(&root.repr, ReprKind::Product(f) if f.is_empty()) {
            if root.kind == ModuleKind::Module {
                let st = TypedC::self_type(root, &targs);
                if let Err(e) = b.ctype(&st) {
                    b.errors.push(e);
                }
            }
        }
        b.drain();
    }
    let code = b.assemble(&entry);
    (code, b.errors)
}
