//! The `// === PARAMS BEGIN/END ===` protocol (docs/DESIGN.md §2.4, §2.5).
//!
//! A strategy's free parameters live in a delimited block inside its committed `lib.rs`:
//!
//! ```text
//! // === PARAMS BEGIN ===
//! const FEE_BPS: u128 = 500; // range: 1..=500
//! // === PARAMS END ===
//! ```
//!
//! Each line is a plain Rust `const` declaration plus a trailing `// range: MIN..=MAX`
//! comment declaring the frozen search space (docs/DESIGN.md §2.4's "declared in `NOTES.md`
//! and frozen before any search runs" — this is that same space, machine-readable next to
//! the value it bounds rather than duplicated into a second config file). The committed file
//! is valid Rust either way: a rewrite regenerates each parameter line from its parsed name,
//! type and bounds with the new value substituted, preserving the original line's leading
//! indentation — so the frozen space a search actually ran within is legible in every
//! rewritten scratch copy, not just the final commit. A `pub` modifier or attribute on a
//! parameter's `const` line is rejected at parse time rather than silently dropped by the
//! regeneration (no current family needs either); a composite type (e.g. an array) may render
//! with different token spacing than hand-written, since no formatting pass runs over the
//! regenerated line — every current family uses a plain primitive integer type, for which
//! this doesn't arise.

const BEGIN_MARKER: &str = "// === PARAMS BEGIN ===";
const END_MARKER: &str = "// === PARAMS END ===";
const RANGE_COMMENT: &str = "// range:";

/// One free parameter: its name and Rust type (both taken verbatim from the source, so a
/// rewrite reproduces exactly what was parsed) and its frozen inclusive bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSpec {
    pub name: String,
    pub ty: String,
    pub min: i128,
    pub max: i128,
    /// The value committed in the source this was parsed from — not used by the search
    /// itself (which explores the whole `[min, max]` range), but a natural sanity anchor
    /// (e.g. "does re-evaluating the committed point reproduce its own claimed edge").
    pub current: i128,
    /// The original line's leading whitespace, preserved verbatim on rewrite.
    pub indent: String,
}

/// Locates the single `PARAMS BEGIN`/`PARAMS END` pair and returns each line's byte range
/// (as line indices into `source.lines()`) plus the parsed specs, in source order.
fn locate_block(source: &str) -> anyhow::Result<(usize, usize, Vec<&str>)> {
    let lines: Vec<&str> = source.lines().collect();

    let begin_positions: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.trim() == BEGIN_MARKER)
        .map(|(i, _)| i)
        .collect();
    let end_positions: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.trim() == END_MARKER)
        .map(|(i, _)| i)
        .collect();

    let begin = match begin_positions.as_slice() {
        [] => anyhow::bail!("no `{BEGIN_MARKER}` line found"),
        [only] => *only,
        _ => anyhow::bail!("multiple `{BEGIN_MARKER}` lines found; exactly one is required"),
    };
    let end = match end_positions.as_slice() {
        [] => anyhow::bail!("no `{END_MARKER}` line found"),
        [only] => *only,
        _ => anyhow::bail!("multiple `{END_MARKER}` lines found; exactly one is required"),
    };

    if end <= begin {
        anyhow::bail!("`{END_MARKER}` must come after `{BEGIN_MARKER}`");
    }

    // Blank lines inside the block are dropped, not preserved — a rewrite emits exactly one
    // line per declared parameter and nothing else, so a blank line for readability between
    // parameters would not survive a round trip. No current family's block has one.
    let inner: Vec<&str> = lines[begin + 1..end]
        .iter()
        .copied()
        .filter(|l| !l.trim().is_empty())
        .collect();

    Ok((begin, end, inner))
}

/// Parses one `const NAME: TYPE = VALUE; // range: MIN..=MAX` line. Deliberately not part of
/// the `syn`-parsed token stream: a `//` comment carries no token, so the range is recovered
/// from the raw text instead — cheaper and more transparent than smuggling it through a
/// doc-comment attribute.
fn parse_param_line(line: &str) -> anyhow::Result<ParamSpec> {
    let indent = line[..line.len() - line.trim_start().len()].to_string();

    let (decl, range_text) = line.split_once(RANGE_COMMENT).ok_or_else(|| {
        anyhow::anyhow!("PARAMS line missing `{RANGE_COMMENT}` comment: `{line}`")
    })?;

    let item: syn::ItemConst = syn::parse_str(decl.trim())
        .map_err(|e| anyhow::anyhow!("failed to parse PARAMS const `{}`: {e}", decl.trim()))?;

    let name = item.ident.to_string();
    // A rewrite regenerates this line from name/type/value alone (see this module's doc
    // comment) — a `pub` modifier or attribute would be silently dropped rather than
    // preserved, so it's rejected here instead. No current family needs either.
    if !matches!(item.vis, syn::Visibility::Inherited) {
        anyhow::bail!(
            "PARAMS const `{name}` has a visibility modifier, which `rewrite_params` does not \
             preserve — remove it"
        );
    }
    if !item.attrs.is_empty() {
        anyhow::bail!(
            "PARAMS const `{name}` has an attribute, which `rewrite_params` does not preserve \
             — remove it"
        );
    }
    let ty = quote_type(&item.ty);
    let current = literal_i128(&item.expr)
        .ok_or_else(|| anyhow::anyhow!("PARAMS const `{name}` is not an integer literal"))?;

    let (min_text, max_text) = range_text.trim().split_once("..=").ok_or_else(|| {
        anyhow::anyhow!("PARAMS range for `{name}` is not `MIN..=MAX`: `{range_text}`")
    })?;
    let min: i128 = min_text
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("PARAMS range min for `{name}` is not an integer: {e}"))?;
    let max: i128 = max_text
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("PARAMS range max for `{name}` is not an integer: {e}"))?;
    if min > max {
        anyhow::bail!("PARAMS range for `{name}` has min {min} > max {max}");
    }

    Ok(ParamSpec {
        name,
        ty,
        min,
        max,
        current,
        indent,
    })
}

fn quote_type(ty: &syn::Type) -> String {
    use quote::ToTokens;
    ty.to_token_stream().to_string()
}

/// Extracts an integer literal's value, including a leading unary `-` — `u128` families
/// never need it, but a future signed-parameter family (docs/DESIGN.md §2.5's "1-4 free
/// parameters") shouldn't require a different block format.
fn literal_i128(expr: &syn::Expr) -> Option<i128> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(int),
            ..
        }) => int.base10_parse::<i128>().ok(),
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr,
            ..
        }) => literal_i128(expr).map(|v| -v),
        _ => None,
    }
}

/// Parses every parameter declared in `source`'s PARAMS block, in source order.
pub fn parse_params_block(source: &str) -> anyhow::Result<Vec<ParamSpec>> {
    let (_, _, inner) = locate_block(source)?;
    inner.iter().map(|line| parse_param_line(line)).collect()
}

/// Rewrites `source`'s PARAMS block, substituting `values[i]` (in declaration order) for
/// each parameter's current literal. Everything outside the block is byte-identical to
/// `source`; each rewritten line inside it is regenerated from the parsed name, type, bounds
/// and new value with the original line's leading indentation restored — not a byte-for-byte
/// copy of the original line with only the literal swapped (see this module's doc comment for
/// what that means for a `pub`/attributed const, which is rejected at parse time instead).
/// `values.len()` must equal the number of parameters declared in the block.
pub fn rewrite_params(source: &str, values: &[i128]) -> anyhow::Result<String> {
    let (begin, end, inner) = locate_block(source)?;
    let specs: Vec<ParamSpec> = inner
        .iter()
        .map(|line| parse_param_line(line))
        .collect::<anyhow::Result<_>>()?;

    if values.len() != specs.len() {
        anyhow::bail!(
            "rewrite_params given {} value(s) for {} declared parameter(s)",
            values.len(),
            specs.len()
        );
    }
    for (spec, value) in specs.iter().zip(values) {
        if *value < spec.min || *value > spec.max {
            anyhow::bail!(
                "value {value} for `{}` is outside its frozen range {}..={}",
                spec.name,
                spec.min,
                spec.max
            );
        }
    }

    let lines: Vec<&str> = source.lines().collect();
    let mut out = String::new();
    for line in &lines[..=begin] {
        out.push_str(line);
        out.push('\n');
    }
    for (spec, value) in specs.iter().zip(values) {
        out.push_str(&format!(
            "{}const {}: {} = {value}; {RANGE_COMMENT} {}..={}\n",
            spec.indent, spec.name, spec.ty, spec.min, spec.max
        ));
    }
    for line in &lines[end..] {
        out.push_str(line);
        out.push('\n');
    }

    if !source.ends_with('\n') {
        out.pop();
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> &'static str {
        "use pinocchio::foo;\n\n// === PARAMS BEGIN ===\nconst FEE_BPS: u128 = 500; // range: 1..=500\n// === PARAMS END ===\n\nfn compute_swap() {}\n"
    }

    #[test]
    fn parses_a_single_param() {
        let specs = parse_params_block(sample()).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "FEE_BPS");
        assert_eq!(specs[0].ty, "u128");
        assert_eq!(specs[0].min, 1);
        assert_eq!(specs[0].max, 500);
        assert_eq!(specs[0].current, 500);
    }

    #[test]
    fn parses_multiple_params_in_order() {
        let source = "// === PARAMS BEGIN ===\nconst A: u128 = 1; // range: 0..=10\nconst B: i64 = -2; // range: -5..=5\n// === PARAMS END ===\n";
        let specs = parse_params_block(source).unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "A");
        assert_eq!(specs[1].name, "B");
        assert_eq!(specs[1].current, -2);
        assert_eq!(specs[1].min, -5);
    }

    #[test]
    fn rewrite_substitutes_only_the_literal() {
        let rewritten = rewrite_params(sample(), &[30]).unwrap();
        assert!(rewritten.contains("const FEE_BPS: u128 = 30; // range: 1..=500"));
        assert!(rewritten.contains("use pinocchio::foo;"));
        assert!(rewritten.contains("fn compute_swap() {}"));
        // Re-parsing the rewritten source should reproduce the same frozen bounds.
        let specs = parse_params_block(&rewritten).unwrap();
        assert_eq!(specs[0].current, 30);
        assert_eq!(specs[0].min, 1);
        assert_eq!(specs[0].max, 500);
    }

    #[test]
    fn rewrite_preserves_leading_indentation() {
        let source = "// === PARAMS BEGIN ===\n    const A: u128 = 1; // range: 0..=10\n// === PARAMS END ===\n";
        let specs = parse_params_block(source).unwrap();
        assert_eq!(specs[0].indent, "    ");

        let rewritten = rewrite_params(source, &[5]).unwrap();
        assert!(rewritten.contains("\n    const A: u128 = 5; // range: 0..=10\n"));
    }

    #[test]
    fn pub_const_is_rejected() {
        let source =
            "// === PARAMS BEGIN ===\npub const A: u128 = 1; // range: 0..=10\n// === PARAMS END ===\n";
        let err = parse_params_block(source).unwrap_err();
        assert!(
            err.to_string().contains("visibility modifier"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn attributed_const_is_rejected() {
        // Each PARAMS line is parsed independently (one line = one param declaration), so
        // the attribute must be on the same line as the `const` it's rejected together with.
        let source = "// === PARAMS BEGIN ===\n#[allow(dead_code)] const A: u128 = 1; // range: 0..=10\n// === PARAMS END ===\n";
        let err = parse_params_block(source).unwrap_err();
        assert!(
            err.to_string().contains("attribute"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rewrite_preserves_no_trailing_newline() {
        let source =
            "// === PARAMS BEGIN ===\nconst A: u128 = 1; // range: 0..=10\n// === PARAMS END ===";
        let rewritten = rewrite_params(source, &[5]).unwrap();
        assert!(!rewritten.ends_with('\n'));
    }

    #[test]
    fn rewrite_rejects_wrong_value_count() {
        let err = rewrite_params(sample(), &[1, 2]).unwrap_err();
        assert!(err.to_string().contains("2 value(s) for 1"));
    }

    #[test]
    fn rewrite_rejects_out_of_range_value() {
        let err = rewrite_params(sample(), &[501]).unwrap_err();
        assert!(err.to_string().contains("outside its frozen range"));
    }

    #[test]
    fn missing_begin_marker_fails() {
        let err = parse_params_block("const A: u128 = 1;\n").unwrap_err();
        assert!(err.to_string().contains("no `// === PARAMS BEGIN ===`"));
    }

    #[test]
    fn missing_end_marker_fails() {
        let source = "// === PARAMS BEGIN ===\nconst A: u128 = 1; // range: 0..=10\n";
        let err = parse_params_block(source).unwrap_err();
        assert!(err.to_string().contains("no `// === PARAMS END ===`"));
    }

    #[test]
    fn duplicate_begin_marker_fails() {
        let source = "// === PARAMS BEGIN ===\n// === PARAMS BEGIN ===\nconst A: u128 = 1; // range: 0..=10\n// === PARAMS END ===\n";
        let err = parse_params_block(source).unwrap_err();
        assert!(err.to_string().contains("multiple"));
    }

    #[test]
    fn end_before_begin_fails() {
        let source =
            "// === PARAMS END ===\nconst A: u128 = 1; // range: 0..=10\n// === PARAMS BEGIN ===\n";
        let err = parse_params_block(source).unwrap_err();
        assert!(err.to_string().contains("must come after"));
    }

    #[test]
    fn missing_range_comment_fails() {
        let source = "// === PARAMS BEGIN ===\nconst A: u128 = 1;\n// === PARAMS END ===\n";
        let err = parse_params_block(source).unwrap_err();
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn malformed_range_fails() {
        let source =
            "// === PARAMS BEGIN ===\nconst A: u128 = 1; // range: 0-10\n// === PARAMS END ===\n";
        let err = parse_params_block(source).unwrap_err();
        assert!(err.to_string().contains("not `MIN..=MAX`"));
    }

    #[test]
    fn min_greater_than_max_fails() {
        let source =
            "// === PARAMS BEGIN ===\nconst A: u128 = 1; // range: 10..=0\n// === PARAMS END ===\n";
        let err = parse_params_block(source).unwrap_err();
        assert!(err.to_string().contains("min 10 > max 0"));
    }

    #[test]
    fn non_integer_literal_fails() {
        let source = "// === PARAMS BEGIN ===\nconst A: bool = true; // range: 0..=10\n// === PARAMS END ===\n";
        let err = parse_params_block(source).unwrap_err();
        assert!(err.to_string().contains("not an integer literal"));
    }
}
