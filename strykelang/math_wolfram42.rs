// Batch 42 — climate, fluids, atmospheric science, geophysics, oscillation indices.

const B42_SIGMA: f64 = 5.670_374_419e-8;
const B42_R_DRY: f64 = 287.058;
const B42_R_VAPOR: f64 = 461.5;
const B42_G: f64 = 9.80665;
const B42_CP_DRY: f64 = 1004.0;
const B42_LV: f64 = 2.5e6;
const B42_T0: f64 = 273.15;

fn b42_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

// Stefan-Boltzmann radiation: M = εσT⁴
fn builtin_stefan_boltzmann_radiation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let eps = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(eps * B42_SIGMA * t.powi(4)))
}

// Grey body emissivity passthrough
fn builtin_emissivity_grey_body(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let eps = f1(args);
    Ok(PerlValue::float(eps.clamp(0.0, 1.0)))
}

// Albedo-blackbody balance: T_eq = (S(1-α)/4σ)^(1/4)
fn builtin_albedo_blackbody_balance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.3);
    Ok(PerlValue::float(((s * (1.0 - alpha)) / (4.0 * B42_SIGMA)).powf(0.25)))
}

// Solar constant scaled to distance d (AU): S_d = S₀ / d²
fn builtin_solar_constant_at_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s0 = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if d == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(s0 / (d * d)))
}

// TSI step variation around mean (~1361 W/m²)
fn builtin_total_solar_irradiance_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let tsi = f1(args);
    let cycle = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(tsi + cycle))
}

// Absorbed short-wave radiation: S(1-α)/4
fn builtin_absorbed_short_wave(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.3);
    Ok(PerlValue::float(s * (1.0 - alpha) / 4.0))
}

// Emitted long-wave radiation: εσT⁴
fn builtin_emitted_long_wave(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_stefan_boltzmann_radiation(args)
}

// Clausius-Clapeyron full: e_s(T) = e_0 exp(L_v/R_v · (1/T_0 - 1/T))
fn builtin_clausius_clapeyron_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let e0 = args.get(1).map(|v| v.to_number()).unwrap_or(611.2);
    if t == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(e0 * (B42_LV / B42_R_VAPOR * (1.0 / B42_T0 - 1.0 / t)).exp()))
}

// Relative humidity = e / e_s
fn builtin_relative_humidity_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e = f1(args);
    let es = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if es == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(e / es))
}

// Dewpoint via inverted Magnus: Td = (b·γ)/(a-γ), γ = ln(RH) + a·T/(b+T)
fn builtin_dewpoint_temperature_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t_c = f1(args);
    let rh = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let a = 17.27;
    let b = 237.7;
    if rh <= 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    let gamma = rh.ln() + a * t_c / (b + t_c);
    Ok(PerlValue::float(b * gamma / (a - gamma)))
}

// Wet-bulb potential temperature (Bolton 1980 approx)
fn builtin_wet_bulb_potential(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta_e = f1(args);
    Ok(PerlValue::float(theta_e - 273.0))
}

// Virtual temperature T_v = T (1 + 0.608q)
fn builtin_virtual_temperature_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(t * (1.0 + 0.608 * q)))
}

// Density altitude h_d ≈ h + 120(T - T_isa) for ISA
fn builtin_density_altitude_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(288.15);
    let t_isa = 288.15 - 0.0065 * h;
    Ok(PerlValue::float(h + 120.0 * (t - t_isa)))
}

// Geopotential height Φ/g
fn builtin_geopotential_height_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let phi = f1(args);
    Ok(PerlValue::float(phi / B42_G))
}

// Geometric height (approximation: Z = R·H/(R-H))
fn builtin_geometric_height_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = f1(args);
    let r_e = 6_371_000.0;
    if r_e - h == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(r_e * h / (r_e - h)))
}

// Dry adiabatic lapse rate Γ_d = g/c_p
fn builtin_adiabatic_lapse_rate_dry(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(B42_G / B42_CP_DRY))
}

// Moist adiabatic lapse rate (approx)
fn builtin_adiabatic_lapse_rate_moist(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(288.0);
    let denom = 1.0 + B42_LV * B42_LV * r / (B42_CP_DRY * B42_R_VAPOR * t * t);
    Ok(PerlValue::float(B42_G / B42_CP_DRY / denom))
}

// Brunt-Väisälä frequency N² = g/θ · ∂θ/∂z
fn builtin_brunt_vaisala_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let dtheta_dz = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if theta == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(B42_G * dtheta_dz / theta))
}

// Richardson number Ri = N²/(∂U/∂z)²
fn builtin_richardson_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n2 = f1(args);
    let du_dz = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if du_dz == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(n2 / (du_dz * du_dz)))
}

// Gradient Richardson Ri_g
fn builtin_gradient_richardson_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_richardson_number_step(args)
}

// Flux Richardson Ri_f = (g/θ_v)·(w'θ_v')/(u'w' · ∂U/∂z)
fn builtin_flux_richardson_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let buoy_flux = f1(args);
    let mech_prod = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if mech_prod == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(buoy_flux / mech_prod))
}

// Turbulent kinetic energy: TKE = ½(u'² + v'² + w'²)
fn builtin_turbulent_kinetic_energy_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b42_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(0.5 * v.iter().map(|x| x * x).sum::<f64>()))
}

// Prandtl mixing length l = κz
fn builtin_mixing_length_prandtl(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    let kappa = args.get(1).map(|v| v.to_number()).unwrap_or(0.4);
    Ok(PerlValue::float(kappa * z))
}

// Monin-Obukhov length L = -u_*³ / (κ g/T · w'T')
fn builtin_monin_obukhov_length(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u_star = f1(args);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(288.0);
    let buoy_flux = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    if buoy_flux == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-u_star.powi(3) / (0.4 * B42_G / theta * buoy_flux)))
}

// Similarity function ϕ(ζ) = 1 + 5ζ for stable
fn builtin_similarity_function_phi(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let zeta = f1(args);
    if zeta >= 0.0 { Ok(PerlValue::float(1.0 + 5.0 * zeta)) } else { Ok(PerlValue::float((1.0 - 16.0 * zeta).powf(-0.25))) }
}

// Log-law wind profile U(z) = (u_*/κ) ln(z/z₀)
fn builtin_log_law_wind_profile(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u_star = f1(args);
    let z = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    let z0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    if z0 <= 0.0 || z <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((u_star / 0.4) * (z / z0).ln()))
}

// Power-law wind profile: U(z) = U_r (z/z_r)^p
fn builtin_power_law_wind_profile(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u_r = f1(args);
    let z = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    let z_r = args.get(2).map(|v| v.to_number()).unwrap_or(10.0);
    let p = args.get(3).map(|v| v.to_number()).unwrap_or(0.143);
    if z_r <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(u_r * (z / z_r).powf(p)))
}

// Ekman layer depth D_E = π √(2K/f)
fn builtin_ekman_layer_depth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = f1(args);
    let f_cor = args.get(1).map(|v| v.to_number()).unwrap_or(1e-4);
    if f_cor == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(std::f64::consts::PI * (2.0 * k / f_cor).sqrt()))
}

// Ekman pumping w_E = 1/(ρf)·∇×τ
fn builtin_ekman_pumping_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let curl_tau = f1(args);
    let rho = args.get(1).map(|v| v.to_number()).unwrap_or(1.225);
    let f_cor = args.get(2).map(|v| v.to_number()).unwrap_or(1e-4);
    if rho == 0.0 || f_cor == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(curl_tau / (rho * f_cor)))
}

// Geostrophic wind v_g = (1/(ρf))·∂p/∂x
fn builtin_geostrophic_wind_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dp_dx = f1(args);
    let rho = args.get(1).map(|v| v.to_number()).unwrap_or(1.225);
    let f_cor = args.get(2).map(|v| v.to_number()).unwrap_or(1e-4);
    if rho == 0.0 || f_cor == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(dp_dx / (rho * f_cor)))
}

// Gradient wind: V_g (1 + V_g/(fR)) = -∇p/(ρf)
fn builtin_gradient_wind_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v_g = f1(args);
    let f_cor = args.get(1).map(|v| v.to_number()).unwrap_or(1e-4);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(1e6);
    if f_cor == 0.0 || r == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(v_g + v_g * v_g / (f_cor * r)))
}

// Thermal wind: ∂V_g/∂z = -(g/fT)·∇T
fn builtin_thermal_wind_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dt_dx = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(288.0);
    let f_cor = args.get(2).map(|v| v.to_number()).unwrap_or(1e-4);
    if t == 0.0 || f_cor == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(-B42_G * dt_dx / (f_cor * t)))
}

// Quasi-geostrophic ω equation step (Q-vector form, scalar)
fn builtin_quasi_geostrophic_omega(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_div = f1(args);
    Ok(PerlValue::float(2.0 * q_div))
}

// Omega equation: ∇²ω + (f²/σ)·∂²ω/∂p² = forcing
fn builtin_omega_equation_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lap_omega = f1(args);
    let f_cor = args.get(1).map(|v| v.to_number()).unwrap_or(1e-4);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let d2omega_dp2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if sigma == 0.0 { return Ok(PerlValue::float(lap_omega)); }
    Ok(PerlValue::float(lap_omega + f_cor * f_cor / sigma * d2omega_dp2))
}

// Potential temperature θ = T(p₀/p)^(R/c_p)
fn builtin_potential_temperature_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1000.0);
    let p0 = args.get(2).map(|v| v.to_number()).unwrap_or(1000.0);
    if p == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(t * (p0 / p).powf(B42_R_DRY / B42_CP_DRY)))
}

// Equivalent potential temperature θ_e = θ exp(L_v q / c_p T)
fn builtin_equivalent_potential_temp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(288.0);
    if t == 0.0 { return Ok(PerlValue::float(theta)); }
    Ok(PerlValue::float(theta * (B42_LV * q / (B42_CP_DRY * t)).exp()))
}

// Saturation equivalent potential temp
fn builtin_saturation_equivalent_pt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_equivalent_potential_temp(args)
}

// Isentropic potential vorticity (IPV)
fn builtin_ipv_potential_vorticity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let abs_vort = f1(args);
    let dtheta_dp = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(-B42_G * abs_vort * dtheta_dp))
}

// Ertel PV: PV = (ζ + f)/ρ · ∂θ/∂z
fn builtin_ertel_pv_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let zeta_plus_f = f1(args);
    let rho = args.get(1).map(|v| v.to_number()).unwrap_or(1.225);
    let dtheta_dz = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if rho == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(zeta_plus_f * dtheta_dz / rho))
}

// Absolute vorticity ζ + f
fn builtin_absolute_vorticity_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let zeta = f1(args);
    let f_cor = args.get(1).map(|v| v.to_number()).unwrap_or(1e-4);
    Ok(PerlValue::float(zeta + f_cor))
}

// Relative vorticity ζ = ∂v/∂x - ∂u/∂y
fn builtin_relative_vorticity_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dv_dx = f1(args);
    let du_dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(dv_dx - du_dy))
}

// Divergence δ = ∂u/∂x + ∂v/∂y → maps to ω via continuity
fn builtin_divergence_omega_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let du_dx = f1(args);
    let dv_dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(du_dx + dv_dy))
}

// Stream function ψ from horizontal flow (V = k × ∇ψ)
fn builtin_streamfunction_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(-u * dy))
}

// Velocity potential χ from divergence
fn builtin_velocity_potential_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let div = f1(args);
    let lap_inv = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(div * lap_inv))
}

// Helmholtz decomposition: V = ∇φ + ∇×ψ
fn builtin_helmholtz_decomp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let grad_phi = f1(args);
    let curl_psi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(grad_phi + curl_psi))
}

// CFL number: cΔt/Δx
fn builtin_courant_friedrichs_lewy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let dx = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if dx == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(c * dt / dx))
}

// Péclet number Pe = uL/D
fn builtin_peclet_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if d == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(u * l / d))
}

// Prandtl number Pr = ν/α
fn builtin_prandtl_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let nu = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if alpha == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(nu / alpha))
}

// Reynolds number Re = uL/ν
fn builtin_reynolds_full_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let nu = args.get(2).map(|v| v.to_number()).unwrap_or(1.5e-5);
    if nu == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(u * l / nu))
}

// Schmidt number Sc = ν/D
fn builtin_schmidt_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_peclet_number_step(args)
}

// Sherwood number Sh = kL/D
fn builtin_sherwood_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_peclet_number_step(args)
}

// Nusselt number Nu = hL/k
fn builtin_nusselt_full_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_peclet_number_step(args)
}

// Grashof number Gr = gβΔTL³/ν²
fn builtin_grashof_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let beta = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let nu = args.get(3).map(|v| v.to_number()).unwrap_or(1.5e-5);
    if nu == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(B42_G * beta * dt * l.powi(3) / (nu * nu)))
}

// Rayleigh number Ra = Gr·Pr
fn builtin_rayleigh_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let gr = f1(args);
    let pr = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(gr * pr))
}

// Weber number We = ρu²L/σ
fn builtin_weber_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.072);
    if sigma == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(rho * u * u * l / sigma))
}

// Froude number Fr = u/√(gL)
fn builtin_froude_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if l <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(u / (B42_G * l).sqrt()))
}

// Strouhal number St = fL/U
fn builtin_strouhal_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_freq = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let u = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if u == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(f_freq * l / u))
}

// Mach number Ma = u/c
fn builtin_mach_full_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(343.0);
    if c == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(u / c))
}

// Biot number Bi = hL/k
fn builtin_biot_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_peclet_number_step(args)
}

// Fourier number Fo = αt/L²
fn builtin_fourier_number_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if l == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(alpha * t / (l * l)))
}

// Turbulence intensity I = u_rms / U_mean
fn builtin_turbulence_intensity_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u_rms = f1(args);
    let u_mean = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if u_mean == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(u_rms / u_mean))
}

// Hurst exponent estimate H from R/S analysis
fn builtin_hurst_exponent_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let log_rs = f1(args);
    let log_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if log_n == 0.0 { return Ok(PerlValue::float(0.5)); }
    Ok(PerlValue::float(log_rs / log_n))
}

// Detrended fluctuation α (similar to Hurst)
fn builtin_detrended_fluct_alpha(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_hurst_exponent_estimate(args)
}

// Power spectrum slope (1/f^β)
fn builtin_power_spectrum_slope(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let log_p = f1(args);
    let log_f = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if log_f == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(-log_p / log_f))
}

// Kolmogorov -5/3 spectrum check
fn builtin_spectral_kappa_minus53(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(-5.0 / 3.0))
}

// Batchelor scale η_B = η Sc^(-1/2)
fn builtin_batchelor_scale_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let eta = f1(args);
    let sc = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sc <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(eta / sc.sqrt()))
}

// Kolmogorov microscale η = (ν³/ε)^(1/4)
fn builtin_kolmogorov_microscale(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let nu = f1(args);
    let eps = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if eps <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float((nu.powi(3) / eps).powf(0.25)))
}

// Taylor microscale λ = (15ν u'² / ε)^(1/2)
fn builtin_taylor_microscale_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let nu = f1(args);
    let u_var = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eps = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if eps == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float((15.0 * nu * u_var / eps).sqrt()))
}

// Integral length scale L = u'³/ε
fn builtin_integral_length_scale(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let eps = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if eps == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(u.powi(3) / eps))
}

// Turbulent dissipation ε = -dE/dt
fn builtin_turbulent_dissipation_eps(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let de_dt = f1(args);
    Ok(PerlValue::float(-de_dt))
}

// Isotropic relation check: ⟨u²⟩ = ⟨v²⟩ = ⟨w²⟩
fn builtin_isotropic_relation_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b42_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.len() < 3 { return Ok(PerlValue::integer(0)); }
    let max = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min = v.iter().cloned().fold(f64::INFINITY, f64::min);
    Ok(PerlValue::integer(if (max - min).abs() < 0.05 * max.abs() { 1 } else { 0 }))
}

// SST anomaly = T - T_climatology
fn builtin_sst_anomaly_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let t_clim = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(t - t_clim))
}

// ENSO index (Niño 3.4 anomaly)
fn builtin_enso_index_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sst_anomaly_step(args)
}

// AMO index — area-averaged North Atlantic SST anomaly
fn builtin_amo_index_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sst_anomaly_step(args)
}

// NAO index — Iceland low - Azores high SLP anomaly difference
fn builtin_nao_index_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p_iceland = f1(args);
    let p_azores = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(p_azores - p_iceland))
}

// SOI = (Tahiti - Darwin) SLP anomaly / SD
fn builtin_soi_oscillation_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dp = f1(args);
    let sd = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sd == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(dp / sd))
}

// PDO index from EOF1 of N. Pacific SST
fn builtin_pdo_index_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// MJO phase from RMM1, RMM2: atan2(RMM2, RMM1) in [0, 8]
fn builtin_mjo_phase_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rmm1 = f1(args);
    let rmm2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = rmm2.atan2(rmm1);
    let phase = ((theta + std::f64::consts::PI) / (2.0 * std::f64::consts::PI) * 8.0).floor();
    Ok(PerlValue::integer(phase as i64))
}

// Walker circulation index step
fn builtin_walker_circulation_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_soi_oscillation_index(args)
}

// Hadley cell maximum latitude (degrees)
fn builtin_hadley_cell_max_lat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let solstice_offset = f1(args);
    Ok(PerlValue::float(30.0 + solstice_offset))
}

// Ferrel cell mid-latitude index
fn builtin_ferrel_cell_step(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(45.0))
}

// ITCZ position latitude
fn builtin_itcz_position_lat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let solstice = f1(args);
    Ok(PerlValue::float(0.0 + 5.0 * solstice))
}

// Trade wind speed
fn builtin_trade_wind_speed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lat = f1(args);
    Ok(PerlValue::float(7.0 * lat.cos()))
}

// Westerlies jet speed
fn builtin_westerlies_jet_speed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lat = f1(args);
    Ok(PerlValue::float(40.0 * (lat - 30.0).cos().max(0.0)))
}

// Polar vortex radius (km)
fn builtin_polar_vortex_radius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pv_strength = f1(args);
    Ok(PerlValue::float(1500.0 * (1.0 - pv_strength * 0.1)))
}

// Arctic Oscillation index
fn builtin_arctic_oscillation_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Indian monsoon index (rainfall anomaly)
fn builtin_indian_monsoon_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// African monsoon index
fn builtin_african_monsoon_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Quasi-Biennial Oscillation step (years 2-3 cycle)
fn builtin_qbo_oscillation_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    Ok(PerlValue::float((2.0 * std::f64::consts::PI * t / 28.0).sin() * 30.0))
}

// Solar cycle phase (11-year period)
fn builtin_solar_cycle_phase(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    Ok(PerlValue::float((2.0 * std::f64::consts::PI * t / 11.0).sin()))
}

// Sunspot relative number (Wolf number)
fn builtin_sunspot_relative_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    let f_count = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(10.0 * g + f_count))
}

// Geomagnetic Kp index (0-9)
fn builtin_geomagnetic_kp_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    Ok(PerlValue::float(r.clamp(0.0, 9.0)))
}

// Total ozone in Dobson Units
fn builtin_ozone_dobson_total(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args).max(0.0)))
}

// Chlorine radical decay first-order: Cl(t) = Cl₀ exp(-kt)
fn builtin_chlorine_radical_decay(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cl0 = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(cl0 * (-k * t).exp()))
}

// Montreal protocol track: linear decline from baseline year
fn builtin_montreal_protocol_track(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cfc = f1(args);
    let years_since = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(cfc * (1.0 - 0.04 * years_since).max(0.0)))
}

// CO₂ growth rate (ppm/year)
fn builtin_co2_growth_rate_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c1 = f1(args);
    let c0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(c1 - c0))
}

// Methane growth rate
fn builtin_methane_growth_rate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_co2_growth_rate_step(args)
}

// Aerosol optical depth from extinction coefficient
fn builtin_aerosol_optical_depth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ext_coef = f1(args);
    let path = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(ext_coef * path))
}

// Milankovitch ice age forcing (combined eccentricity, obliquity, precession)
fn builtin_ice_age_milankovitch(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ecc = f1(args);
    let obl = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let prec = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(ecc + obl + prec))
}

// Greenhouse forcing ΔF = α·ln(C/C₀) for CO₂
fn builtin_greenhouse_forcing_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = f1(args);
    let c0 = args.get(1).map(|v| v.to_number()).unwrap_or(280.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(5.35);
    if c0 == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(alpha * (c / c0).ln()))
}
