//! Constants & distribution helpers.
//! HTTP status codes return their numeric value. HTTP method names
//! return their canonical uppercase form. Distribution PDF/quantile/
//! sampler functions use the `statrs` crate.

use crate::value::StrykeValue;
use statrs::distribution::{
    Beta, Cauchy, ChiSquared, ContinuousCDF, Discrete, Exp, FisherSnedecor, Gamma, LogNormal,
    Normal, StudentsT, Uniform, Weibull,
};

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

macro_rules! http_status {
    ($name:ident, $code:expr) => {
        pub fn $name(_args: &[StrykeValue]) -> StrykeValue {
            StrykeValue::integer($code)
        }
    };
}

http_status!(http_status_continue, 100);
http_status!(http_status_switching_protocols, 101);
http_status!(http_status_ok, 200);
http_status!(http_status_created, 201);
http_status!(http_status_accepted, 202);
http_status!(http_status_no_content, 204);
http_status!(http_status_partial_content, 206);
http_status!(http_status_multiple_choices, 300);
http_status!(http_status_moved_permanently, 301);
http_status!(http_status_found, 302);
http_status!(http_status_see_other, 303);
http_status!(http_status_not_modified, 304);
http_status!(http_status_temporary_redirect, 307);
http_status!(http_status_permanent_redirect, 308);
http_status!(http_status_bad_request, 400);
http_status!(http_status_unauthorized, 401);
http_status!(http_status_payment_required, 402);
http_status!(http_status_forbidden, 403);
http_status!(http_status_not_found, 404);
http_status!(http_status_method_not_allowed, 405);
http_status!(http_status_not_acceptable, 406);
http_status!(http_status_conflict, 409);
http_status!(http_status_gone, 410);
http_status!(http_status_length_required, 411);
http_status!(http_status_precondition_failed, 412);
http_status!(http_status_payload_too_large, 413);
http_status!(http_status_uri_too_long, 414);
http_status!(http_status_unsupported_media_type, 415);
http_status!(http_status_range_not_satisfiable, 416);
http_status!(http_status_expectation_failed, 417);
http_status!(http_status_im_a_teapot, 418);
http_status!(http_status_unprocessable_entity, 422);
http_status!(http_status_too_many_requests, 429);
http_status!(http_status_internal_server_error, 500);
http_status!(http_status_not_implemented, 501);
http_status!(http_status_bad_gateway, 502);
http_status!(http_status_service_unavailable, 503);
http_status!(http_status_gateway_timeout, 504);
http_status!(http_status_http_version_not_supported, 505);

macro_rules! http_method {
    ($name:ident, $verb:expr) => {
        pub fn $name(_args: &[StrykeValue]) -> StrykeValue {
            StrykeValue::string($verb.to_string())
        }
    };
}

http_method!(http_method_get, "GET");
http_method!(http_method_post, "POST");
http_method!(http_method_put, "PUT");
http_method!(http_method_delete, "DELETE");
http_method!(http_method_patch, "PATCH");
http_method!(http_method_head, "HEAD");
http_method!(http_method_options, "OPTIONS");
http_method!(http_method_trace, "TRACE");
http_method!(http_method_connect, "CONNECT");

// ══════════════════════════════════════════════════════════════════════
// Distribution PDF / quantile functions (statrs backed)
// ══════════════════════════════════════════════════════════════════════
/// `dbeta` — see implementation.
pub fn dbeta(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let a = arg_f64(args, 1).unwrap_or(1.0);
    let b = arg_f64(args, 2).unwrap_or(1.0);
    match Beta::new(a, b) {
        Ok(d) => {
            use statrs::distribution::Continuous;
            StrykeValue::float(d.pdf(x))
        }
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `qbeta` — see implementation.
pub fn qbeta(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let a = arg_f64(args, 1).unwrap_or(1.0);
    let b = arg_f64(args, 2).unwrap_or(1.0);
    match Beta::new(a, b) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `rbeta` — see implementation.
pub fn rbeta(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let p: f64 = rng.gen();
    qbeta(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(1.0)),
        args.get(1).cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `dcauchy` — see implementation.
pub fn dcauchy(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let loc = arg_f64(args, 1).unwrap_or(0.0);
    let scale = arg_f64(args, 2).unwrap_or(1.0);
    match Cauchy::new(loc, scale) {
        Ok(d) => {
            use statrs::distribution::Continuous;
            StrykeValue::float(d.pdf(x))
        }
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `qcauchy` — see implementation.
pub fn qcauchy(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let loc = arg_f64(args, 1).unwrap_or(0.0);
    let scale = arg_f64(args, 2).unwrap_or(1.0);
    match Cauchy::new(loc, scale) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `rcauchy` — see implementation.
pub fn rcauchy(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qcauchy(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(0.0)),
        args.get(1).cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `dexp` — see implementation.
pub fn dexp(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let rate = arg_f64(args, 1).unwrap_or(1.0);
    match Exp::new(rate) {
        Ok(d) => {
            use statrs::distribution::Continuous;
            StrykeValue::float(d.pdf(x))
        }
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `qexp` — see implementation.
pub fn qexp(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let rate = arg_f64(args, 1).unwrap_or(1.0);
    match Exp::new(rate) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `rexp` — see implementation.
pub fn rexp(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qexp(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `dgamma` — see implementation.
pub fn dgamma(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let shape = arg_f64(args, 1).unwrap_or(1.0);
    let rate = arg_f64(args, 2).unwrap_or(1.0);
    match Gamma::new(shape, rate) {
        Ok(d) => {
            use statrs::distribution::Continuous;
            StrykeValue::float(d.pdf(x))
        }
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `qgamma` — see implementation.
pub fn qgamma(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let shape = arg_f64(args, 1).unwrap_or(1.0);
    let rate = arg_f64(args, 2).unwrap_or(1.0);
    match Gamma::new(shape, rate) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `rgamma` — see implementation.
pub fn rgamma(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qgamma(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(1.0)),
        args.get(1).cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `dlnorm` — see implementation.
pub fn dlnorm(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(1.0);
    let mu = arg_f64(args, 1).unwrap_or(0.0);
    let sigma = arg_f64(args, 2).unwrap_or(1.0);
    match LogNormal::new(mu, sigma) {
        Ok(d) => {
            use statrs::distribution::Continuous;
            StrykeValue::float(d.pdf(x))
        }
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `qlnorm` — see implementation.
pub fn qlnorm(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let mu = arg_f64(args, 1).unwrap_or(0.0);
    let sigma = arg_f64(args, 2).unwrap_or(1.0);
    match LogNormal::new(mu, sigma) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `rlnorm` — see implementation.
pub fn rlnorm(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qlnorm(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(0.0)),
        args.get(1).cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `dlogis` — see implementation.
pub fn dlogis(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let loc = arg_f64(args, 1).unwrap_or(0.0);
    let scale = arg_f64(args, 2).unwrap_or(1.0).max(1e-12);
    let z = (x - loc) / scale;
    let pdf = (-z).exp() / (scale * (1.0 + (-z).exp()).powi(2));
    StrykeValue::float(pdf)
}
/// `qlogis` — see implementation.
pub fn qlogis(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5).clamp(1e-12, 1.0 - 1e-12);
    let loc = arg_f64(args, 1).unwrap_or(0.0);
    let scale = arg_f64(args, 2).unwrap_or(1.0);
    StrykeValue::float(loc + scale * (p / (1.0 - p)).ln())
}
/// `rlogis` — see implementation.
pub fn rlogis(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qlogis(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(0.0)),
        args.get(1).cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `dpois` — see implementation.
pub fn dpois(args: &[StrykeValue]) -> StrykeValue {
    let k = arg_f64(args, 0).unwrap_or(0.0).max(0.0).round() as u64;
    let lambda = arg_f64(args, 1).unwrap_or(1.0);
    match statrs::distribution::Poisson::new(lambda) {
        Ok(d) => StrykeValue::float(d.pmf(k)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `qpois` — see implementation.
pub fn qpois(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let lambda = arg_f64(args, 1).unwrap_or(1.0);
    let Ok(d) = statrs::distribution::Poisson::new(lambda) else {
        return StrykeValue::UNDEF;
    };
    use statrs::distribution::DiscreteCDF;
    let mut k: u64 = 0;
    while d.cdf(k) < p && k < 1_000_000 {
        k += 1;
    }
    StrykeValue::integer(k as i64)
}
/// `rpois` — see implementation.
pub fn rpois(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qpois(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `dweibull` — see implementation.
pub fn dweibull(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let shape = arg_f64(args, 1).unwrap_or(1.0);
    let scale = arg_f64(args, 2).unwrap_or(1.0);
    match Weibull::new(shape, scale) {
        Ok(d) => {
            use statrs::distribution::Continuous;
            StrykeValue::float(d.pdf(x))
        }
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `qweibull` — see implementation.
pub fn qweibull(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let shape = arg_f64(args, 1).unwrap_or(1.0);
    let scale = arg_f64(args, 2).unwrap_or(1.0);
    match Weibull::new(shape, scale) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `rweibull` — see implementation.
pub fn rweibull(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qweibull(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(1.0)),
        args.get(1).cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `qnorm` — see implementation.
pub fn qnorm(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let mu = arg_f64(args, 1).unwrap_or(0.0);
    let sigma = arg_f64(args, 2).unwrap_or(1.0);
    match Normal::new(mu, sigma) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `rnorm` — see implementation.
pub fn rnorm(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qnorm(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(0.0)),
        args.get(1).cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `qunif` — see implementation.
pub fn qunif(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let lo = arg_f64(args, 1).unwrap_or(0.0);
    let hi = arg_f64(args, 2).unwrap_or(1.0);
    match Uniform::new(lo, hi) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `runif` — see implementation.
pub fn runif(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let lo = arg_f64(args, 0).unwrap_or(0.0);
    let hi = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(rand::thread_rng().gen_range(lo..hi))
}
/// `qbinom` — see implementation.
pub fn qbinom(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let n = arg_f64(args, 1).unwrap_or(10.0).max(0.0).round() as u64;
    let pr = arg_f64(args, 2).unwrap_or(0.5);
    let Ok(d) = statrs::distribution::Binomial::new(pr, n) else {
        return StrykeValue::UNDEF;
    };
    use statrs::distribution::DiscreteCDF;
    for k in 0..=n {
        if d.cdf(k) >= p {
            return StrykeValue::integer(k as i64);
        }
    }
    StrykeValue::integer(n as i64)
}
/// `rbinom` — see implementation.
pub fn rbinom(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qbinom(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(10.0)),
        args.get(1).cloned().unwrap_or(StrykeValue::float(0.5)),
    ])
}
/// `qgeom` — see implementation.
pub fn qgeom(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let pr = arg_f64(args, 1).unwrap_or(0.5).clamp(1e-12, 1.0);
    // Inverse CDF for geometric: k = ceil(ln(1-p) / ln(1-pr)) - 1
    let k = ((1.0 - p).ln() / (1.0 - pr).max(1e-12).ln()).ceil() as i64 - 1;
    StrykeValue::integer(k.max(0))
}
/// `rgeom` — see implementation.
pub fn rgeom(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qgeom(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(0.5)),
    ])
}
/// `qhyper` — see implementation.
pub fn qhyper(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let pop = arg_f64(args, 1).unwrap_or(10.0).max(1.0).round() as u64;
    let succ = arg_f64(args, 2).unwrap_or(5.0).max(0.0).round() as u64;
    let draws = arg_f64(args, 3).unwrap_or(3.0).max(0.0).round() as u64;
    let Ok(d) = statrs::distribution::Hypergeometric::new(pop, succ, draws) else {
        return StrykeValue::UNDEF;
    };
    use statrs::distribution::DiscreteCDF;
    let max = draws.min(succ);
    for k in 0..=max {
        if d.cdf(k) >= p {
            return StrykeValue::integer(k as i64);
        }
    }
    StrykeValue::integer(max as i64)
}
/// `rhyper` — see implementation.
pub fn rhyper(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qhyper(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(10.0)),
        args.get(1).cloned().unwrap_or(StrykeValue::float(5.0)),
        args.get(2).cloned().unwrap_or(StrykeValue::float(3.0)),
    ])
}
/// `qchisq` — see implementation.
pub fn qchisq(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let df = arg_f64(args, 1).unwrap_or(1.0);
    match ChiSquared::new(df) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `rchisq` — see implementation.
pub fn rchisq(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qchisq(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `qf` — see implementation.
pub fn qf(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let df1 = arg_f64(args, 1).unwrap_or(1.0);
    let df2 = arg_f64(args, 2).unwrap_or(1.0);
    match FisherSnedecor::new(df1, df2) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `rf` — see implementation.
pub fn rf(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qf(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(1.0)),
        args.get(1).cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}
/// `qt` — see implementation.
pub fn qt(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_f64(args, 0).unwrap_or(0.5);
    let df = arg_f64(args, 1).unwrap_or(1.0);
    match StudentsT::new(0.0, 1.0, df) {
        Ok(d) => StrykeValue::float(d.inverse_cdf(p)),
        Err(_) => StrykeValue::UNDEF,
    }
}
/// `rt` — see implementation.
pub fn rt(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p: f64 = rand::thread_rng().gen();
    qt(&[
        StrykeValue::float(p),
        args.first().cloned().unwrap_or(StrykeValue::float(1.0)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    // ─── http_status_* ───────────────────────────────────────────────────

    #[test]
    fn http_status_macro_pins_canonical_codes() {
        assert_eq!(http_status_ok(&[]).to_int(), 200);
        assert_eq!(http_status_not_found(&[]).to_int(), 404);
        assert_eq!(http_status_internal_server_error(&[]).to_int(), 500);
        assert_eq!(http_status_im_a_teapot(&[]).to_int(), 418);
        assert_eq!(http_status_too_many_requests(&[]).to_int(), 429);
    }

    #[test]
    fn http_status_ignores_args() {
        // _args is unused — passing junk must not crash and must return code.
        assert_eq!(
            http_status_ok(&[StrykeValue::string("ignored".into())]).to_int(),
            200
        );
    }

    // ─── http_method_* ───────────────────────────────────────────────────

    #[test]
    fn http_method_macro_emits_uppercase_verb() {
        assert_eq!(http_method_get(&[]).to_string(), "GET");
        assert_eq!(http_method_post(&[]).to_string(), "POST");
        assert_eq!(http_method_delete(&[]).to_string(), "DELETE");
        assert_eq!(http_method_options(&[]).to_string(), "OPTIONS");
    }

    // ─── dbeta / qbeta ───────────────────────────────────────────────────

    #[test]
    fn dbeta_uniform_special_case() {
        // Beta(1,1) is Uniform(0,1) — pdf is 1.0 everywhere in (0,1).
        let r = dbeta(&[
            StrykeValue::float(0.5),
            StrykeValue::float(1.0),
            StrykeValue::float(1.0),
        ]);
        assert!(approx(r.to_number(), 1.0, 1e-9));
    }

    #[test]
    fn dbeta_invalid_params_returns_undef() {
        // Beta requires a,b > 0. Zero is invalid.
        let r = dbeta(&[
            StrykeValue::float(0.5),
            StrykeValue::float(0.0),
            StrykeValue::float(1.0),
        ]);
        assert!(r.is_undef());
    }

    #[test]
    fn qbeta_median_of_uniform_is_half() {
        let r = qbeta(&[
            StrykeValue::float(0.5),
            StrykeValue::float(1.0),
            StrykeValue::float(1.0),
        ]);
        assert!(approx(r.to_number(), 0.5, 1e-9));
    }

    // ─── dexp / qexp ─────────────────────────────────────────────────────

    #[test]
    fn dexp_at_zero_equals_rate() {
        // Exponential PDF at x=0: f(0) = lambda.
        let r = dexp(&[StrykeValue::float(0.0), StrykeValue::float(2.0)]);
        assert!(approx(r.to_number(), 2.0, 1e-9));
    }

    #[test]
    fn qexp_quartile_relation() {
        // Exp(1) quantile: F^{-1}(p) = -ln(1-p). p=0.5 → ln(2).
        let r = qexp(&[StrykeValue::float(0.5), StrykeValue::float(1.0)]);
        assert!(approx(r.to_number(), 2f64.ln(), 1e-9));
    }

    #[test]
    fn dexp_negative_rate_returns_undef() {
        let r = dexp(&[StrykeValue::float(1.0), StrykeValue::float(-1.0)]);
        assert!(r.is_undef());
    }

    // ─── qnorm ───────────────────────────────────────────────────────────

    #[test]
    fn qnorm_median_is_mean() {
        // qnorm(0.5, mu, sigma) = mu for any sigma > 0.
        let r = qnorm(&[
            StrykeValue::float(0.5),
            StrykeValue::float(7.0),
            StrykeValue::float(3.0),
        ]);
        assert!(approx(r.to_number(), 7.0, 1e-9));
    }

    #[test]
    fn qnorm_invalid_sigma_returns_undef() {
        let r = qnorm(&[
            StrykeValue::float(0.5),
            StrykeValue::float(0.0),
            StrykeValue::float(-1.0),
        ]);
        assert!(r.is_undef());
    }

    // ─── qunif / runif ───────────────────────────────────────────────────

    #[test]
    fn qunif_linear_in_p() {
        // U[2,10] median = 6.
        let r = qunif(&[
            StrykeValue::float(0.5),
            StrykeValue::float(2.0),
            StrykeValue::float(10.0),
        ]);
        assert!(approx(r.to_number(), 6.0, 1e-9));
    }

    #[test]
    fn runif_within_range() {
        // 1000 samples must all fall in [lo, hi).
        for _ in 0..1000 {
            let r = runif(&[StrykeValue::float(-5.0), StrykeValue::float(5.0)]).to_number();
            assert!((-5.0..5.0).contains(&r), "out of range: {r}");
        }
    }

    // ─── qlogis ─────────────────────────────────────────────────────────

    #[test]
    fn qlogis_median_returns_location() {
        // Logistic median = loc.
        let r = qlogis(&[
            StrykeValue::float(0.5),
            StrykeValue::float(4.0),
            StrykeValue::float(2.0),
        ]);
        assert!(approx(r.to_number(), 4.0, 1e-6));
    }

    // ─── dlogis ──────────────────────────────────────────────────────────

    #[test]
    fn dlogis_at_location_is_quarter_over_scale() {
        // f(loc) = 1/(4*scale) for logistic distribution.
        let r = dlogis(&[
            StrykeValue::float(0.0),
            StrykeValue::float(0.0),
            StrykeValue::float(1.0),
        ]);
        assert!(approx(r.to_number(), 0.25, 1e-9));
    }

    // ─── dpois ───────────────────────────────────────────────────────────

    #[test]
    fn dpois_k_zero_equals_exp_neg_lambda() {
        // P(X=0) = e^{-lambda}.
        let r = dpois(&[StrykeValue::float(0.0), StrykeValue::float(2.5)]);
        assert!(approx(r.to_number(), (-2.5f64).exp(), 1e-9));
    }

    #[test]
    fn dpois_invalid_lambda_returns_undef() {
        let r = dpois(&[StrykeValue::float(0.0), StrykeValue::float(-1.0)]);
        assert!(r.is_undef());
    }

    // ─── qgeom ───────────────────────────────────────────────────────────

    #[test]
    fn qgeom_min_zero() {
        // Quantile clamped to >= 0.
        let r = qgeom(&[StrykeValue::float(0.0), StrykeValue::float(0.5)]);
        assert!(r.to_int() >= 0);
    }

    // ─── qbinom ──────────────────────────────────────────────────────────

    #[test]
    fn qbinom_median_balanced() {
        // Binomial(n=10, p=0.5) median ≈ 5.
        let r = qbinom(&[
            StrykeValue::float(0.5),
            StrykeValue::float(10.0),
            StrykeValue::float(0.5),
        ]);
        let k = r.to_int();
        assert!((4..=5).contains(&k), "expected 4 or 5, got {k}");
    }

    // ─── qchisq ──────────────────────────────────────────────────────────

    #[test]
    fn qchisq_invalid_df_returns_undef() {
        let r = qchisq(&[StrykeValue::float(0.5), StrykeValue::float(0.0)]);
        assert!(r.is_undef());
    }

    // ─── arg_f64 ────────────────────────────────────────────────────────

    #[test]
    fn arg_f64_missing_index_returns_none() {
        assert!(arg_f64(&[], 0).is_none());
        assert!(arg_f64(&[StrykeValue::float(1.0)], 5).is_none());
    }
}
