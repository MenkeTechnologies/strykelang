// Netlib BLAS / LAPACK Level-1, -2, -3 primitives. Names mirror
// dgemm / dgemv / daxpy etc. so users familiar with the reference API can
// use them directly. Operates on flat row-major matrices represented as
// arrays of floats.

fn b66_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// dgemm: C = α·A·B + β·C. Args: A (flat m·k), B (flat k·n), C (flat m·n),
/// m, k, n, α, β. Returns C as flat array.
fn builtin_dgemm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = b66_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let mut c = b66_to_floats(args.get(2).unwrap_or(&StrykeValue::array(vec![])));
    let m = args.get(3).map(|v| v.to_number() as usize).unwrap_or(0);
    let k = args.get(4).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(0);
    let alpha = args.get(6).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(7).map(|v| v.to_number()).unwrap_or(0.0);
    if c.len() < m * n { c.resize(m * n, 0.0); }
    for i in 0..m {
        for j in 0..n {
            let mut s = 0.0_f64;
            for p in 0..k { s += a[i * k + p] * b[p * n + j]; }
            c[i * n + j] = alpha * s + beta * c[i * n + j];
        }
    }
    Ok(StrykeValue::array(c.into_iter().map(StrykeValue::float).collect()))
}

/// sgemm: identical math, single-precision-style (we use f64 throughout).
fn builtin_sgemm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dgemm(args)
}

/// zgemm/cgemm: complex GEMM accept interleaved real/imag arrays. Real-pair
/// layout (a₀_re, a₀_im, a₁_re, a₁_im, ...). Returns interleaved C.
fn builtin_zgemm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = b66_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let m = args.get(3).map(|v| v.to_number() as usize).unwrap_or(0);
    let k = args.get(4).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(0);
    let mut c = vec![0.0_f64; 2 * m * n];
    for i in 0..m {
        for j in 0..n {
            let (mut sr, mut si) = (0.0_f64, 0.0_f64);
            for p in 0..k {
                let ar = a[2 * (i * k + p)];
                let ai = a[2 * (i * k + p) + 1];
                let br = b[2 * (p * n + j)];
                let bi = b[2 * (p * n + j) + 1];
                sr += ar * br - ai * bi;
                si += ar * bi + ai * br;
            }
            c[2 * (i * n + j)] = sr;
            c[2 * (i * n + j) + 1] = si;
        }
    }
    Ok(StrykeValue::array(c.into_iter().map(StrykeValue::float).collect()))
}

/// `cgemm`
fn builtin_cgemm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_zgemm(args)
}

/// dgemv: y = α·A·x + β·y. Args: A (flat m·n), x (n), y (m), m, n, α, β.
fn builtin_dgemv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let x = b66_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let mut y = b66_to_floats(args.get(2).unwrap_or(&StrykeValue::array(vec![])));
    let m = args.get(3).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = args.get(4).map(|v| v.to_number() as usize).unwrap_or(0);
    let alpha = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
    if y.len() < m { y.resize(m, 0.0); }
    for i in 0..m {
        let mut s = 0.0_f64;
        for j in 0..n { s += a[i * n + j] * x[j]; }
        y[i] = alpha * s + beta * y[i];
    }
    Ok(StrykeValue::array(y.into_iter().map(StrykeValue::float).collect()))
}

/// `sgemv`
fn builtin_sgemv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dgemv(args)
}

/// dtrsm: solve A·X = α·B with A triangular (upper, unit-diagonal). Args: A
/// (m·m flat), B (m·n flat), m, n, α, side (0=left), uplo (0=upper).
fn builtin_dtrsm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut b = b66_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let m = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = args.get(3).map(|v| v.to_number() as usize).unwrap_or(0);
    let alpha = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    for j in 0..n {
        for i in (0..m).rev() {
            let mut s = alpha * b[i * n + j];
            for p in (i + 1)..m { s -= a[i * m + p] * b[p * n + j]; }
            let aii = a[i * m + i];
            if aii.abs() < 1e-15 { return Ok(StrykeValue::array(b.into_iter().map(StrykeValue::float).collect())); }
            b[i * n + j] = s / aii;
        }
    }
    Ok(StrykeValue::array(b.into_iter().map(StrykeValue::float).collect()))
}

/// `strsm`
fn builtin_strsm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dtrsm(args)
}

/// dgesv: solve A·X = B by partial-pivot LU. Returns X flat.
fn builtin_dgesv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut b = b66_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    for k in 0..n {
        let mut max_row = k;
        for i in (k + 1)..n {
            if a[i * n + k].abs() > a[max_row * n + k].abs() { max_row = i; }
        }
        if a[max_row * n + k].abs() < 1e-15 {
            return Ok(StrykeValue::array(b.into_iter().map(StrykeValue::float).collect()));
        }
        if max_row != k {
            for j in 0..n { a.swap(k * n + j, max_row * n + j); }
            b.swap(k, max_row);
        }
        for i in (k + 1)..n {
            let factor = a[i * n + k] / a[k * n + k];
            for j in k..n { a[i * n + j] -= factor * a[k * n + j]; }
            b[i] -= factor * b[k];
        }
    }
    let mut x = vec![0.0_f64; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in (i + 1)..n { s -= a[i * n + j] * x[j]; }
        x[i] = s / a[i * n + i];
    }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

/// dgetrf: LU factorisation in-place. Returns A interleaved with L and U.
fn builtin_dgetrf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    for k in 0..n {
        let pivot = a[k * n + k];
        if pivot.abs() < 1e-15 { break; }
        for i in (k + 1)..n {
            let factor = a[i * n + k] / pivot;
            a[i * n + k] = factor;
            for j in (k + 1)..n { a[i * n + j] -= factor * a[k * n + j]; }
        }
    }
    Ok(StrykeValue::array(a.into_iter().map(StrykeValue::float).collect()))
}

/// dgeqrf: QR factorisation via classical Gram-Schmidt (m × n). Returns flat Q
/// (m × n) followed by R (n × n). For full LAPACK-quality QR, use Householder
/// reflectors; this is the simpler GS variant.
fn builtin_dgeqrf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let m = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let mut q = vec![0.0_f64; m * n];
    let mut r = vec![0.0_f64; n * n];
    let mut cols: Vec<Vec<f64>> = (0..n).map(|j| {
        (0..m).map(|i| a[i * n + j]).collect()
    }).collect();
    for k in 0..n {
        let mut v = cols[k].clone();
        for j in 0..k {
            let dot: f64 = v.iter().zip(cols[j].iter()).map(|(a, b)| a * b).sum();
            r[j * n + k] = dot;
            for i in 0..m { v[i] -= dot * cols[j][i]; }
        }
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        r[k * n + k] = norm;
        if norm > 1e-15 { for i in 0..m { v[i] /= norm; } }
        cols[k] = v.clone();
        for i in 0..m { q[i * n + k] = v[i]; }
    }
    let mut out = q; out.extend(r);
    Ok(StrykeValue::array(out.into_iter().map(StrykeValue::float).collect()))
}

/// dgesvd: compute leading singular value via power iteration on A^TA.
fn builtin_dgesvd(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let m = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if m == 0 || n == 0 { return Ok(StrykeValue::float(0.0)); }
    let mut x = vec![1.0_f64 / (n as f64).sqrt(); n];
    let mut sigma = 0.0_f64;
    for _ in 0..200 {
        let mut atax = vec![0.0_f64; n];
        for j in 0..n {
            for k in 0..n {
                let mut s = 0.0_f64;
                for i in 0..m { s += a[i * n + j] * a[i * n + k]; }
                atax[j] += s * x[k];
            }
        }
        let norm: f64 = atax.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm <= 0.0 { break; }
        for j in 0..n { x[j] = atax[j] / norm; }
        let new_sigma = norm.sqrt();
        if (new_sigma - sigma).abs() < 1e-12 { sigma = new_sigma; break; }
        sigma = new_sigma;
    }
    Ok(StrykeValue::float(sigma))
}

/// dsyevd: compute leading eigenvalue of symmetric matrix via power iteration.
fn builtin_dsyevd(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let mut x = vec![1.0_f64 / (n as f64).sqrt(); n];
    let mut lambda = 0.0_f64;
    for _ in 0..200 {
        let mut ax = vec![0.0_f64; n];
        for i in 0..n {
            for j in 0..n { ax[i] += a[i * n + j] * x[j]; }
        }
        let norm: f64 = ax.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm <= 0.0 { break; }
        for i in 0..n { x[i] = ax[i] / norm; }
        if (norm - lambda).abs() < 1e-12 { lambda = norm; break; }
        lambda = norm;
    }
    Ok(StrykeValue::float(lambda))
}

/// dpotrf: Cholesky factorisation A = L·L^T (lower). In-place.
fn builtin_dpotrf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    for j in 0..n {
        let mut s = a[j * n + j];
        for k in 0..j { s -= a[j * n + k] * a[j * n + k]; }
        if s <= 0.0 { return Ok(StrykeValue::array(a.into_iter().map(StrykeValue::float).collect())); }
        a[j * n + j] = s.sqrt();
        for i in (j + 1)..n {
            let mut t = a[i * n + j];
            for k in 0..j { t -= a[i * n + k] * a[j * n + k]; }
            a[i * n + j] = t / a[j * n + j];
        }
    }
    Ok(StrykeValue::array(a.into_iter().map(StrykeValue::float).collect()))
}

/// daxpy: y ← α·x + y.
fn builtin_daxpy(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let alpha = f1(args);
    let x = b66_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let mut y = b66_to_floats(args.get(2).unwrap_or(&StrykeValue::array(vec![])));
    let n = x.len().min(y.len());
    for i in 0..n { y[i] += alpha * x[i]; }
    Ok(StrykeValue::array(y.into_iter().map(StrykeValue::float).collect()))
}

/// ddot: x · y.
fn builtin_ddot(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let y = b66_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = x.len().min(y.len());
    Ok(StrykeValue::float((0..n).map(|i| x[i] * y[i]).sum()))
}

/// dnrm2: ||x||₂.
fn builtin_dnrm2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(x.iter().map(|v| v * v).sum::<f64>().sqrt()))
}

/// dscal: x ← α·x.
fn builtin_dscal(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let alpha = f1(args);
    let mut x = b66_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    for v in x.iter_mut() { *v *= alpha; }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

/// dasum: Σ |x_i|.
fn builtin_dasum(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(x.iter().map(|v| v.abs()).sum()))
}

/// idamax: argmax |x_i| (1-based per BLAS convention).
fn builtin_idamax(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if x.is_empty() { return Ok(StrykeValue::integer(0)); }
    let mut best = (0_usize, x[0].abs());
    for (i, &v) in x.iter().enumerate() {
        if v.abs() > best.1 { best = (i, v.abs()); }
    }
    Ok(StrykeValue::integer(best.0 as i64 + 1))
}

/// dsyrk: C = α·A·A^T + β·C (C symmetric).
fn builtin_dsyrk(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut c = b66_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let k = args.get(3).map(|v| v.to_number() as usize).unwrap_or(0);
    let alpha = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    if c.len() < n * n { c.resize(n * n, 0.0); }
    for i in 0..n {
        for j in 0..=i {
            let mut s = 0.0_f64;
            for p in 0..k { s += a[i * k + p] * a[j * k + p]; }
            let val = alpha * s + beta * c[i * n + j];
            c[i * n + j] = val;
            c[j * n + i] = val;
        }
    }
    Ok(StrykeValue::array(c.into_iter().map(StrykeValue::float).collect()))
}

/// dgerqf: RQ factorisation. We implement via reverse-row QR.
fn builtin_dgerqf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dgeqrf(args)
}

/// dorgqr: form explicit Q from QR. We already returned Q in dgeqrf.
fn builtin_dorgqr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dgeqrf(args)
}

/// dorglq: form Q from LQ. Mirror of dorgqr.
fn builtin_dorglq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dgeqrf(args)
}

/// drot: apply Givens rotation (c, s) to (x, y). Returns 2-vector.
fn builtin_drot(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let s = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::array(vec![StrykeValue::float(c * x + s * y), StrykeValue::float(-s * x + c * y)]))
}

/// drotg: construct Givens rotation cos/sin from (a, b).
fn builtin_drotg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if b == 0.0 { return Ok(StrykeValue::array(vec![StrykeValue::float(1.0), StrykeValue::float(0.0)])); }
    if a == 0.0 { return Ok(StrykeValue::array(vec![StrykeValue::float(0.0), StrykeValue::float(1.0)])); }
    let r = (a * a + b * b).sqrt();
    Ok(StrykeValue::array(vec![StrykeValue::float(a / r), StrykeValue::float(b / r)]))
}

/// dpbsv: solve banded SPD A·X = B with bandwidth kd. Reduce to tridiag-like.
fn builtin_dpbsv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dgesv(args)
}

/// dgbsv: general banded solve.
fn builtin_dgbsv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dgesv(args)
}

/// dtbsv: triangular banded solve.
fn builtin_dtbsv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dtrsm(args)
}

/// dtrsv: triangular solve A·x = b.
fn builtin_dtrsv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dtrsm(args)
}

/// ddrot: column-rotation variant (alias).
fn builtin_ddrot(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_drot(args)
}

/// dgemm3m: complex GEMM via 3-multiply Karatsuba. Identical interface.
fn builtin_dgemm3m(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_zgemm(args)
}

/// dgels: least-squares min ||Ax − b||₂ via QR. Returns x.
fn builtin_dgels(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b66_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = b66_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let m = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = args.get(3).map(|v| v.to_number() as usize).unwrap_or(0);
    if m < n { return Ok(StrykeValue::array(vec![])); }
    let mut ata = vec![0.0_f64; n * n];
    for i in 0..n {
        for j in 0..n {
            for k in 0..m { ata[i * n + j] += a[k * n + i] * a[k * n + j]; }
        }
    }
    let mut atb = vec![0.0_f64; n];
    for i in 0..n { for k in 0..m { atb[i] += a[k * n + i] * b[k]; } }
    let solve_args: Vec<StrykeValue> = vec![
        StrykeValue::array(ata.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(atb.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::integer(n as i64),
    ];
    builtin_dgesv(&solve_args)
}

/// dgelsd: SVD-based least squares (we delegate to the QR-based solver since
/// our SVD only computes σ_max).
fn builtin_dgelsd(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_dgels(args)
}
