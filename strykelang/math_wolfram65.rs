// Batch 65 — geology, seismology, earthquake engineering, mineralogy.

fn b65_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Moment magnitude M_w = (2/3) (log₁₀(M₀) − 9.1), where M₀ is in N·m.
fn builtin_moment_magnitude_mw(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m0 = f1(args).max(1e-9);
    Ok(PerlValue::float((2.0 / 3.0) * (m0.log10() - 9.1)))
}

/// Local magnitude (Richter): M_L = log₁₀(A_max/μm) + Q(Δ, h).
fn builtin_richter_local_ml(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let amp_um = f1(args).max(1e-9);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(2.5);
    Ok(PerlValue::float(amp_um.log10() + q))
}

/// Surface-wave magnitude M_s = log₁₀(A/T) + 1.66 log₁₀(Δ°) + 3.3.
fn builtin_surface_wave_ms(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_over_t = f1(args).max(1e-9);
    let delta_deg = args.get(1).map(|v| v.to_number()).unwrap_or(20.0).max(1e-3);
    Ok(PerlValue::float(a_over_t.log10() + 1.66 * delta_deg.log10() + 3.3))
}

/// Body-wave magnitude m_b = log₁₀(A/T) + Q_p(Δ, h).
fn builtin_body_wave_mb(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_over_t = f1(args).max(1e-9);
    let qp = args.get(1).map(|v| v.to_number()).unwrap_or(6.0);
    Ok(PerlValue::float(a_over_t.log10() + qp))
}

/// Gutenberg-Richter slope b: log₁₀ N = a − b·M from MLE on event magnitudes.
/// b = log₁₀(e) / (M̄ − M_min).
fn builtin_gutenberg_richter_b(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mags = b65_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if mags.is_empty() { return Ok(PerlValue::float(0.0)); }
    let m_min = mags.iter().cloned().fold(f64::INFINITY, f64::min);
    let m_mean = mags.iter().sum::<f64>() / mags.len() as f64;
    let denom = m_mean - m_min;
    if denom <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(std::f64::consts::E.log10() / denom))
}

/// Omori aftershock decay: n(t) = K / (t + c)^p.
fn builtin_omori_aftershock(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let p = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(k / (t + c).powf(p)))
}

/// Boore-Atkinson PGA attenuation: log₁₀(PGA) = a + b·M − c·log₁₀(R).
fn builtin_pga_attenuation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(10.0).max(1e-3);
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(-2.0);
    let b = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    let c = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(10f64.powf(a + b * m - c * r.log10())))
}

/// Arias intensity I_A = (π/2g) ∫ a(t)² dt.
fn builtin_arias_intensity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let acc = b65_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(0.005);
    let g = 9.80665;
    let s: f64 = acc.iter().map(|a| a * a).sum::<f64>() * dt;
    Ok(PerlValue::float(std::f64::consts::PI * s / (2.0 * g)))
}

/// ShakeMap PGA from Mw, R using a generic GMPE.
fn builtin_shake_map_pga(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_pga_attenuation(args)
}

/// Liquefaction Potential Index: integral of weighted F_S deficit from 0–20 m.
fn builtin_liquefaction_potential_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let factors = b65_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let depths = b65_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = factors.len().min(depths.len());
    let mut s = 0.0_f64;
    for i in 0..n {
        if factors[i] < 1.0 {
            let w = (10.0 - 0.5 * depths[i]).max(0.0);
            s += w * (1.0 - factors[i]);
        }
    }
    Ok(PerlValue::float(s))
}

/// SPT N correction: N₆₀ = N · CE · CB · CR · CS · (energy efficiency 60%).
fn builtin_spt_n_correction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let ce = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let cb = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let cr = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let cs = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(n * ce * cb * cr * cs))
}

/// Mineral Mohs hardness lookup table for 10 reference minerals.
fn builtin_mineral_mohs_hardness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let id = i1(args).clamp(1, 10);
    let h = [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
    Ok(PerlValue::float(h[id as usize - 1]))
}

/// Streak colour index from RGB triple of the streak. Returns dominant hue.
fn builtin_streak_color_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let max = r.max(g).max(b);
    Ok(PerlValue::integer(if max == r { 0 } else if max == g { 1 } else { 2 }))
}

/// Specific gravity = ρ_sample / ρ_water. ρ_water (4°C) = 1000 kg/m³.
fn builtin_specific_gravity_water(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = f1(args);
    Ok(PerlValue::float(rho / 1000.0))
}

/// Feldspar classification by anorthite mol%: 0–10 albite, 10–30 oligoclase,
/// 30–50 andesine, 50–70 labradorite, 70–90 bytownite, 90–100 anorthite.
fn builtin_feldspar_classify(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let an_pct = f1(args).clamp(0.0, 100.0);
    let id = if an_pct < 10.0 { 0 } else if an_pct < 30.0 { 1 } else if an_pct < 50.0 { 2 }
        else if an_pct < 70.0 { 3 } else if an_pct < 90.0 { 4 } else { 5 };
    Ok(PerlValue::integer(id))
}

/// Silicate classification by SiO₂ wt%: ultramafic <45, mafic 45–52,
/// intermediate 52–63, felsic 63–69, silicic >69.
fn builtin_silicate_classify(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sio2 = f1(args);
    let id = if sio2 < 45.0 { 0 } else if sio2 < 52.0 { 1 } else if sio2 < 63.0 { 2 }
        else if sio2 < 69.0 { 3 } else { 4 };
    Ok(PerlValue::integer(id))
}

/// IUGS QAPF igneous rock classification: input quartz%, alkali feldspar%,
/// plagioclase%, foid%. Returns rock-type id 0..15.
fn builtin_igneous_qapf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let total = q + a + p;
    if total <= 0.0 { return Ok(PerlValue::integer(-1)); }
    let q_norm = 100.0 * q / total;
    let a_norm = 100.0 * a / total;
    let id = if q_norm > 60.0 { 0 }   // quartz-rich
        else if q_norm > 20.0 { if a_norm > 65.0 { 1 } else if a_norm > 35.0 { 2 } else { 3 } } // granitoids
        else if q_norm > 5.0 { if a_norm > 65.0 { 4 } else if a_norm > 35.0 { 5 } else { 6 } }  // syenites
        else { if a_norm > 65.0 { 7 } else if a_norm > 35.0 { 8 } else { 9 } };               // foid-poor diorites
    Ok(PerlValue::integer(id))
}

/// Metamorphic grade by temperature (°C): low <300, medium 300–500,
/// high 500–700, very high >700.
fn builtin_metamorphic_grade(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t_c = f1(args);
    let id = if t_c < 300.0 { 0 } else if t_c < 500.0 { 1 } else if t_c < 700.0 { 2 } else { 3 };
    Ok(PerlValue::integer(id))
}

/// Crustal density vs depth (PREM-like): ρ(z) = ρ₀ + Δρ · z / z_moho.
fn builtin_crustal_density_depth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    let z_moho = args.get(1).map(|v| v.to_number()).unwrap_or(35.0).max(1e-3);
    let rho0 = args.get(2).map(|v| v.to_number()).unwrap_or(2700.0);
    let drho = args.get(3).map(|v| v.to_number()).unwrap_or(600.0);
    Ok(PerlValue::float(rho0 + drho * (z / z_moho).clamp(0.0, 1.0)))
}

/// P-wave velocity at depth (PREM linear approx in upper crust).
fn builtin_pwave_velocity_depth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    Ok(PerlValue::float(5800.0 + 0.05 * z * 1000.0))
}

/// S-wave velocity at depth (PREM linear approx).
fn builtin_swave_velocity_depth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    Ok(PerlValue::float(3360.0 + 0.03 * z * 1000.0))
}

/// Geothermal gradient: dT/dz ≈ q / k = surface_heat_flow / thermal_conductivity.
fn builtin_gradient_geothermal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_surface = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(2.5).max(1e-9);
    Ok(PerlValue::float(q_surface / k))
}

/// Radiogenic heat production from U, Th, K concentrations (μW/m³).
/// A = ρ · (9.52·U + 2.56·Th + 3.48·K) · 1e-5 in standard units.
fn builtin_heat_flow_radiogenic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = f1(args);
    let u_ppm = args.get(1).map(|v| v.to_number()).unwrap_or(2.5);
    let th_ppm = args.get(2).map(|v| v.to_number()).unwrap_or(10.0);
    let k_pct = args.get(3).map(|v| v.to_number()).unwrap_or(2.5);
    Ok(PerlValue::float(rho * (9.52 * u_ppm + 2.56 * th_ppm + 3.48 * k_pct) * 1e-5))
}
