//! Moves the scalar parameters of a generic Boogie function after the others, in its
//! declaration and in every call.
//!
//! Boogie 3.4.3, under `/typeEncoding:p`, states the result type of a generic function with an
//! axiom that guards each argument's type. When an `int` or `bool` argument comes before
//! another one, the guards after it are shifted by one: for
//! `function on<S>(x: R S, k: int, h: Fn1 S S) returns (R S)` the guard on `h` says
//! `type(h) == int`. The axiom then never applies, the type of a call `on(..)` is unknown, and
//! no axiom of the function fires on a term that has such a call as an argument, so
//! `cnt(on(on(x, 1, h), 2, h)) == 2` does not verify while each step alone does. With the
//! scalars last, as in `function on<S>(x: R S, h: Fn1 S S, k: int)`, it verifies. A Gardel
//! routine such as `ROUTER[S].on(k: NAT, h: (s: S) -> S)` has that shape, so the emitted
//! program is rewritten here rather than asking each library to order its parameters.
//!
//! Only generic functions without `{:inline}` are rewritten: an inlined function has no such
//! axiom. A call whose argument count does not match its declaration is left as it is.

use std::collections::HashMap;

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$' | '#' | '\'' | '`' | '~' | '^' | '\\' | '?')
}

fn is_scalar(ty: &str) -> bool {
    matches!(ty.trim(), "int" | "bool" | "real")
}

/// The index of the bracket that closes the one at `open`, counting `(`, `[`, `{` and `<`
/// only when `angle` is set, and skipping quoted text.
fn closing(s: &[char], open: usize, angle: bool) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < s.len() {
        let c = s[i];
        if c == '"' {
            i += 1;
            while i < s.len() && s[i] != '"' {
                i += 1;
            }
        } else if c == '(' || c == '[' || c == '{' || (angle && c == '<') {
            depth += 1;
        } else if c == ')' || c == ']' || c == '}' || (angle && c == '>') {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// `s` split at the commas that are not inside brackets or quotes.
fn split_top(s: &[char]) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    let mut quoted = false;
    for &c in s {
        if quoted {
            quoted = c != '"';
            cur.push(c);
            continue;
        }
        match c {
            '"' => quoted = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() || !parts.is_empty() {
        parts.push(cur);
    }
    parts
}

/// The new order of the parameters of a declaration, as indexes into the old order, or `None`
/// if no scalar comes before another parameter.
fn scalars_last_order(params: &[String]) -> Option<Vec<usize>> {
    let scalar: Vec<bool> = params
        .iter()
        .map(|p| is_scalar(p.rsplit_once(':').map_or(p.as_str(), |(_, t)| t)))
        .collect();
    let first_scalar = scalar.iter().position(|s| *s)?;
    if scalar[first_scalar..].iter().all(|s| *s) {
        return None;
    }
    let mut order: Vec<usize> = (0..params.len()).filter(|i| !scalar[*i]).collect();
    order.extend((0..params.len()).filter(|i| scalar[*i]));
    Some(order)
}

/// Each generic, non-inline function whose parameters need reordering, with its new order.
fn declarations(text: &[char]) -> HashMap<String, Vec<usize>> {
    let mut out = HashMap::new();
    let src: String = text.iter().collect();
    let mut offset = 0usize;
    for line in src.split_inclusive('\n') {
        let start = offset;
        offset += line.chars().count();
        let Some(rest) = line.strip_prefix("function ") else { continue };
        if rest.starts_with("{:inline") {
            continue;
        }
        let name: String = rest.chars().take_while(|c| is_ident_char(*c)).collect();
        let after = start + "function ".chars().count() + name.chars().count();
        if text.get(after) != Some(&'<') {
            continue;
        }
        let Some(gt) = closing(text, after, true) else { continue };
        if text.get(gt + 1) != Some(&'(') {
            continue;
        }
        let Some(close) = closing(text, gt + 1, false) else { continue };
        let params = split_top(&text[gt + 2..close]);
        if let Some(order) = scalars_last_order(&params) {
            out.insert(name, order);
        }
    }
    out
}

fn permute(parts: &[String], order: &[usize]) -> String {
    order.iter().map(|i| parts[*i].trim()).collect::<Vec<_>>().join(", ")
}

/// `text` with each call of a function in `funs`, and its declaration, in the new order.
fn rewrite(text: &[char], funs: &HashMap<String, Vec<usize>>) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == '"' {
            let end = text[i + 1..].iter().position(|c| *c == '"').map_or(text.len(), |p| i + 2 + p);
            out.extend(&text[i..end]);
            i = end;
            continue;
        }
        if !is_ident_char(c) || (i > 0 && is_ident_char(text[i - 1])) {
            out.push(c);
            i += 1;
            continue;
        }
        let end = i + text[i..].iter().take_while(|c| is_ident_char(**c)).count();
        let name: String = text[i..end].iter().collect();
        out.push_str(&name);
        i = end;
        let Some(order) = funs.get(&name) else { continue };
        // A declaration: `name<..>(params)`.
        let open = if text.get(i) == Some(&'<') && out.ends_with(&format!("function {name}")) {
            let Some(gt) = closing(text, i, true) else { continue };
            out.extend(&text[i..=gt]);
            i = gt + 1;
            i
        } else {
            i
        };
        if text.get(open) != Some(&'(') {
            continue;
        }
        let Some(close) = closing(text, open, false) else { continue };
        let parts: Vec<String> = split_top(&text[open + 1..close])
            .iter()
            .map(|p| rewrite(&p.chars().collect::<Vec<_>>(), funs))
            .collect();
        if parts.len() == order.len() {
            out.push('(');
            out.push_str(&permute(&parts, order));
            out.push(')');
        } else {
            out.push('(');
            out.push_str(&parts.join(","));
            out.push(')');
        }
        i = close + 1;
    }
    out
}

/// `bpl` with the scalar parameters of each generic function last. See the module comment.
pub(crate) fn scalars_last(bpl: &str) -> String {
    let text: Vec<char> = bpl.chars().collect();
    let funs = declarations(&text);
    if funs.is_empty() {
        return bpl.to_string();
    }
    rewrite(&text, &funs)
}

#[cfg(test)]
mod tests {
    use super::scalars_last;

    #[test]
    fn a_scalar_before_another_parameter_moves_last_everywhere() {
        let bpl = "function on<S>(x: R S, k: int, h: (Fn1 S S)) returns (R S);\n\
                   axiom (forall<S> x: R S, k: int, h: (Fn1 S S) :: { on(x, k, h) } cnt(on(x, k, h)) == cnt(x) + 1);\n\
                   procedure p() { assert {:msg \"on(a, b)\"} cnt(on(on(x, 1, f(2, g)), 2, h2)) == 2; }\n";
        let out = scalars_last(bpl);
        assert!(out.contains("function on<S>(x: R S, h: (Fn1 S S), k: int) returns"), "{out}");
        assert!(out.contains("{ on(x, h, k) } cnt(on(x, h, k))"), "{out}");
        assert!(out.contains("cnt(on(on(x, f(2, g), 1), h2, 2))"), "{out}");
        // Binders and quoted text are not calls.
        assert!(out.contains("forall<S> x: R S, k: int, h: (Fn1 S S) ::"), "{out}");
        assert!(out.contains("\"on(a, b)\""), "{out}");
    }

    #[test]
    fn other_functions_are_unchanged() {
        let bpl = "function a<S>(x: R S, h: H, k: int) returns (int);\n\
                   function {:inline} b<S>(k: int, x: R S) returns (bool) { true }\n\
                   function c(k: int, x: R) returns (int);\n\
                   function d<S>(k: int, j: bool) returns (S);\n\
                   axiom a(x, h, 1) == c(1, x) && b(1, x) && d(1, true) == d(2, false);\n";
        assert_eq!(scalars_last(bpl), bpl);
    }

    #[test]
    fn a_name_that_extends_another_is_not_a_call_of_it() {
        let bpl = "function on<S>(k: int, x: R S) returns (R S);\n\
                   axiom on_x(1, y) == on(1, y) && Pon(2, y) == 0;\n";
        let out = scalars_last(bpl);
        assert!(out.contains("on_x(1, y) == on(y, 1) && Pon(2, y)"), "{out}");
    }
}
