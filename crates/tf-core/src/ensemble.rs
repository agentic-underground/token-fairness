//! ensemble — the self-improving multi-algorithm predictor (Phase 2).
//!
//! Several formulas predict the next `actual/estimate` ratio for a job class, run CONCURRENTLY.
//! After every closed job each algorithm's standing prediction is scored against the realised
//! ratio (absolute percentage error, smoothed into an online MAPE), so the field's accuracy is
//! tracked over time. The lowest-error algorithm is the **champion** whose prediction drives the
//! estimate; the inverse-error-weighted **blend** is the "wisdom of the ensemble" cross-check.
//!
//! Predictors are PURE functions of the bounded ratio history (`samples_log`), so the only state
//! persisted is that log plus a per-algorithm smoothed error — which keeps the schema small and
//! lets a NEW candidate algorithm be backtested deterministically over real history before adoption.

/// Smoothing for the online error (MAPE) — higher = more reactive to recent accuracy.
const ERR_BETA: f64 = 0.3;
/// Cap a single absolute-percentage-error so one wild miss can't dominate the smoothed error.
const APE_CAP: f64 = 5.0;
/// Below this many samples the field can't be discriminated; default to the legacy champion.
const MIN_DISCRIMINATE: usize = 3;
/// The default/legacy champion (keeps first-samples behaviour identical to the bash EWMA-0.4).
pub const DEFAULT_CHAMPION: &str = "ewma@0.4";

/// A predictor: maps a ratio history (oldest→newest) to a prediction of the NEXT ratio.
pub type Predictor = fn(&[f64]) -> f64;

/// The registry of predictor algorithms, in stable order. Adding one here is the only change
/// needed to enter the field (the self-improvement substrate).
pub fn algorithms() -> Vec<(&'static str, Predictor)> {
    vec![
        ("ewma@0.2", |h| ewma(h, 0.2)),
        ("ewma@0.4", |h| ewma(h, 0.4)),
        ("ewma@0.6", |h| ewma(h, 0.6)),
        ("sma@5", |h| sma(h, 5)),
        ("median@7", |h| median(h, 7)),
        ("last", last),
        ("linreg", linreg),
    ]
}

fn ewma(h: &[f64], a: f64) -> f64 {
    let mut it = h.iter();
    let Some(&first) = it.next() else { return 1.0 };
    let mut e = first;
    for &x in it {
        e = a * x + (1.0 - a) * e;
    }
    e
}

fn sma(h: &[f64], k: usize) -> f64 {
    if h.is_empty() {
        return 1.0;
    }
    let tail = &h[h.len().saturating_sub(k)..];
    tail.iter().sum::<f64>() / tail.len() as f64
}

fn median(h: &[f64], k: usize) -> f64 {
    if h.is_empty() {
        return 1.0;
    }
    let mut tail: Vec<f64> = h[h.len().saturating_sub(k)..].to_vec();
    tail.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = tail.len();
    if n % 2 == 1 {
        tail[n / 2]
    } else {
        (tail[n / 2 - 1] + tail[n / 2]) / 2.0
    }
}

fn last(h: &[f64]) -> f64 {
    *h.last().unwrap_or(&1.0)
}

/// Least-squares fit `ratio ≈ a·i + b` over (index, ratio); predict at the next index. Captures a
/// drift the EWMAs lag. Falls back to the mean when there are fewer than two points.
fn linreg(h: &[f64]) -> f64 {
    let n = h.len();
    if n < 2 {
        return last(h);
    }
    let nf = n as f64;
    let sx: f64 = (0..n).map(|i| i as f64).sum();
    let sy: f64 = h.iter().sum();
    let sxx: f64 = (0..n).map(|i| (i * i) as f64).sum();
    let sxy: f64 = h.iter().enumerate().map(|(i, &y)| i as f64 * y).sum();
    let denom = nf * sxx - sx * sx;
    if denom.abs() < 1e-12 {
        return sy / nf;
    }
    let slope = (nf * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / nf;
    slope * n as f64 + intercept
}

/// Absolute percentage error of `pred` vs the realised `actual` ratio, capped.
pub fn ape(pred: f64, actual: f64) -> f64 {
    let denom = actual.abs().max(1e-9);
    ((pred - actual).abs() / denom).min(APE_CAP)
}

/// Smoothed online error update (EWMA of APE). `prior` is the previous err (negative = unset).
pub fn fold_err(prior: f64, pred: f64, actual: f64) -> f64 {
    let e = ape(pred, actual);
    if prior < 0.0 {
        e
    } else {
        ERR_BETA * e + (1.0 - ERR_BETA) * prior
    }
}

/// The outcome of one re-scoring: who leads, the headline prediction, the blend, and the board.
pub struct Verdict {
    pub champion: String,
    pub champion_ratio: f64,
    pub blend_ratio: f64,
    /// The champion's smoothed error (the headline MAPE), as a fraction (0.08 == 8%).
    pub mape: f64,
    /// (name, smoothed_err, current_prediction) for every algorithm, in registry order.
    pub board: Vec<(String, f64, f64)>,
}

/// Pick the champion + blend from the current history and per-algorithm errors.
/// `errs(name)` returns the smoothed error for an algorithm (negative if it has none yet).
pub fn decide(history: &[f64], errs: impl Fn(&str) -> f64) -> Verdict {
    let algos = algorithms();
    let board: Vec<(String, f64, f64)> = algos
        .iter()
        .map(|(name, f)| (name.to_string(), errs(name), f(history)))
        .collect();

    // Champion: lowest scored error; until the field is discriminable, the legacy default leads.
    let scored: Vec<&(String, f64, f64)> = board.iter().filter(|(_, e, _)| *e >= 0.0).collect();
    let champion = if history.len() < MIN_DISCRIMINATE || scored.is_empty() {
        DEFAULT_CHAMPION.to_string()
    } else {
        scored
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(n, _, _)| n.clone())
            .unwrap_or_else(|| DEFAULT_CHAMPION.to_string())
    };

    let champ = board.iter().find(|(n, _, _)| *n == champion);
    let champion_ratio = champ.map(|(_, _, p)| *p).unwrap_or(1.0);
    let mape = champ.map(|(_, e, _)| e.max(0.0)).unwrap_or(0.0);

    // Blend: inverse-error weighted over scored algorithms (fall back to the champion's value).
    let mut wsum = 0.0;
    let mut acc = 0.0;
    for (_, e, p) in &board {
        if *e >= 0.0 {
            let w = 1.0 / (e + 0.01);
            wsum += w;
            acc += w * p;
        }
    }
    let blend_ratio = if wsum > 0.0 {
        acc / wsum
    } else {
        champion_ratio
    };

    Verdict {
        champion,
        champion_ratio,
        blend_ratio,
        mape,
        board,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ewma_folds_in_order() {
        assert!((ewma(&[2.0], 0.4) - 2.0).abs() < 1e-9);
        // 0.4*2 + 0.6*1 = 1.4
        assert!((ewma(&[1.0, 2.0], 0.4) - 1.4).abs() < 1e-9);
        assert!((ewma(&[], 0.4) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sma_median_last_linreg_basics() {
        assert!((sma(&[1.0, 2.0, 3.0], 2) - 2.5).abs() < 1e-9);
        assert!((median(&[3.0, 1.0, 2.0], 3) - 2.0).abs() < 1e-9);
        assert!((median(&[4.0, 1.0, 2.0, 3.0], 4) - 2.5).abs() < 1e-9);
        assert!((last(&[1.0, 9.0]) - 9.0).abs() < 1e-9);
        assert!((last(&[]) - 1.0).abs() < 1e-9);
        // perfectly linear 1,2,3 → next is 4
        assert!((linreg(&[1.0, 2.0, 3.0]) - 4.0).abs() < 1e-6);
        assert!((linreg(&[5.0]) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn ape_and_fold_err() {
        assert!((ape(1.1, 1.0) - 0.1).abs() < 1e-9);
        assert_eq!(ape(100.0, 1.0), APE_CAP); // capped
                                              // first sample sets the error; then it smooths
        let e1 = fold_err(-1.0, 1.1, 1.0);
        assert!((e1 - 0.1).abs() < 1e-9);
        let e2 = fold_err(e1, 1.0, 1.0);
        assert!(e2 < e1 && e2 > 0.0);
    }

    #[test]
    fn decide_defaults_to_legacy_until_discriminable() {
        // < MIN_DISCRIMINATE samples → champion is the legacy default regardless of errors.
        let v = decide(&[1.2, 1.1], |_| 0.01);
        assert_eq!(v.champion, DEFAULT_CHAMPION);
    }

    #[test]
    fn decide_promotes_lowest_error_when_enough_samples() {
        let hist = [1.0, 1.0, 1.0, 1.0];
        // give 'last' a near-zero error, others high → 'last' should win
        let v = decide(&hist, |n| if n == "last" { 0.001 } else { 0.5 });
        assert_eq!(v.champion, "last");
        assert!(v.mape >= 0.0);
        // blend sits between the per-algo predictions
        assert!(v.blend_ratio > 0.0);
        assert_eq!(v.board.len(), algorithms().len());
    }
}
