//! Output-formatting helpers that reproduce the bash `printf`/`awk`/`jq` number
//! rendering exactly, so the port is byte-faithful to the conformance vectors.
//!
//! Two facts pinned empirically against the live tools:
//!   * `awk 'printf "%d", x + 0.5'` rounds half **up** (truncate-toward-zero of x+0.5).
//!   * `awk 'printf "%.*f"'` and Rust's `{:.*}` both round half-to-even — they agree.
//!   * jq renders a *computed* double with the shortest round-trip form, which is
//!     exactly what Rust's `{}` for `f64` produces; jq preserves a 4-decimal *literal*
//!     only on a number it never touched arithmetically (the first calibration sample).

/// `awk 'printf "%d", x + 0.5'` — round half up.
pub fn round_i64(x: f64) -> i64 {
    (x + 0.5).trunc() as i64
}

/// `awk 'printf "%.<prec>f"'` — fixed decimals, round half-to-even.
pub fn fixed(x: f64, prec: usize) -> String {
    format!("{:.*}", prec, x)
}

/// jq's rendering of a computed number — shortest round-trip (`1.0` → `"1"`, `1.08` → `"1.08"`).
pub fn shortest(x: f64) -> String {
    format!("{}", x)
}
