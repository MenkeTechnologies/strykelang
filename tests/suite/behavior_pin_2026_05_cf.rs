//! Behavior-pinning batch CF (2026-05-09): meteorology (**`heat_index`**, **`wind_chill`**, **`dew_point*`**, **`humidex`** \(2nd arg **dew °C**\), **`utci_simple`**),
//! fluids / dimensionless groups (**`reynolds_number`**, **`mach_number`**, **`weber_number`** — BUG-134 σ default footgun vs **`weber_number_step`**),
//! open-channel (**`manning_velocity`**, **`chezy_velocity`**), EE ladders (**`resistance_*`/`capacitance_*`/`inductance_*`** — BUG-126 array buckets),
//! **`dB_voltage`**/**`dB_power`** reference defaults (**BUG-135**), radiative / turbulence stubs (**`stefan_boltzmann_*`**, **`kolmogorov_microscale`**),
//! calendar / string / graph helpers.

use crate::common::*;

#[test]
fn heat_index_twenty_eight_c_sixty_rh_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", heat_index(28, 60))"#),
        "29.44978544177771"
    );
}

#[test]
fn heat_index_celsius_thirty_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", heat_index_celsius(30, 55))"#),
        "31.88767755555551"
    );
}

#[test]
fn wind_chill_negative_five_thirty_kmh_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", wind_chill(-5, 30))"#),
        "-12.99672481192107"
    );
}

#[test]
fn wind_chill_celsius_negative_eight_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", wind_chill_celsius(-8, 35))"#),
        "-17.53673134007923"
    );
}

#[test]
fn dew_point_from_rh_percent_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", dew_point(20, 65))"#),
        "13.21457187190135"
    );
}

#[test]
fn dew_point_magnus_rh_percent_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", dew_point_magnus(25, 60))"#),
        "16.69314900619895"
    );
}

#[test]
fn saturation_vapor_pressure_twenty_eight_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", saturation_vapor_pressure(28))"#),
        "37.81003311969642"
    );
}

#[test]
fn humidex_thirty_one_celsius_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", humidex(31, 58))"#),
        "134.87065135879260"
    );
}

#[test]
fn utci_simple_mild_conditions_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", utci_simple(22, 4, 280))"#),
        "30.70000000000000"
    );
}

#[test]
fn pressure_altitude_m_eight_four_five_hpa_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", pressure_altitude_m(845))"#),
        "1505.65869454172207"
    );
}

#[test]
fn density_altitude_m_pressure_and_offset_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", density_altitude_m(845, 32))"#),
        "2885.00000000000000"
    );
}

#[test]
fn reynolds_number_air_jet_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", reynolds_number(1.2, 25, 0.01, 1.81e-5))"#),
        "16574.58563535911526"
    );
}

#[test]
fn prandtl_number_air_like_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", prandtl_number(1005, 1.81e-5, 0.025))"#),
        "0.72762000000000"
    );
}

#[test]
fn mach_number_subsonic_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", mach_number(340, 343))"#),
        "0.99125364431487"
    );
}

#[test]
fn mach_full_step_compressible_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", mach_full_step(240, 343))"#),
        "0.69970845481050"
    );
}

#[test]
fn bernoulli_velocity_incompressible_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", bernoulli_velocity(200000, 101325, 1.225))"#),
        "401.37518709597197"
    );
}

#[test]
fn hypot_three_four_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", hypot(30, 40))"#),
        "50.00000000000000"
    );
}

#[test]
fn froude_number_open_channel_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", froude_number(2.5, 9.80665, 7))"#),
        "0.30173844707703"
    );
}

#[test]
fn froude_number_step_variant_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", froude_number_step(2.1, 10, 9.81))"#),
        "0.21206009759577"
    );
}

#[test]
fn weber_number_requires_sigma_fourth_arg_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", weber_number(998, 12, 0.05, 0.072))"#),
        "99800.00000000001455"
    );
}

#[test]
fn weber_number_step_matches_definition_with_default_sigma_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", weber_number_step(998, 9, 0.02))"#),
        "22455.00000000000000"
    );
}

#[test]
fn weber_number_omitting_sigma_explodes_via_tiny_denominator_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.6e", weber_number(998, 9, 0.02))"#),
        "1.616760e+33"
    );
}

#[test]
fn grashof_number_natural_conv_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", grashof_number(9.80665, 3.4e-3, 20, 0.15, 1.51e-5))"#),
        "9870734.50725845247507"
    );
}

#[test]
fn grashof_number_step_b42_gravity_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", grashof_number_step(4e-4, 40, 0.2, 1.5e-5))"#),
        "5578894.22222222387791"
    );
}

#[test]
fn manning_velocity_shallow_slope_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", manning_velocity(0.015, 2.1, 0.0004))"#),
        "2.18651066373342"
    );
}

#[test]
fn chezy_velocity_rect_channel_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", chezy_velocity(72, 1.9, 0.00065))"#),
        "2.53026480827600"
    );
}

#[test]
fn orifice_velocity_low_pressure_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", orifice_velocity(95000, 101325))"#),
        "1.36936270095268"
    );
}

#[test]
fn friction_factor_laminar_hagen_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", friction_factor_laminar(1200))"#),
        "0.05333333333333"
    );
}

#[test]
fn swamee_jain_turbulent_pipe_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", swamee_jain_factor(50000, 0.00015))"#),
        "0.02136080373147"
    );
}

#[test]
fn pipe_pressure_drop_darcy_style_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", pipe_pressure_drop(0.02, 100, 0.15, 1000, 3))"#),
        "60000.00000000001455"
    );
}

#[test]
fn nusselt_dittus_boelter_turbulent_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", nusselt_dittus_boelter(16574, 0.73, 0.4))"#),
        "48.15028340663923"
    );
}

#[test]
fn nusselt_full_number_convection_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", nusselt_full_number(850, 0.02, 0.6))"#),
        "28.33333333333334"
    );
}

#[test]
fn peclet_schmidt_sherwood_biot_steps_cf() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.14f", peclet_number_step(1.2, 0.015, 2e-5)) . sprintf("\n%.14f", schmidt_number_step(1.5e-5, 2e-5)) . sprintf("\n%.14f", sherwood_number_step(3.2e-5, 0.02, 1.9e-5)) . sprintf("\n%.14f", biot_number_step(90, 0.01, 0.5))"#
        ),
        "899.99999999999989\n0.75000000000000\n0.03368421052632\n1.80000000000000"
    );
}

#[test]
fn rayleigh_from_gr_pr_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", rayleigh_number_step(9870734, 0.73))"#),
        "7205635.81999999936670"
    );
}

#[test]
fn strouhal_full_vortex_shed_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", strouhal_full(4, 0.05, 2.2))"#),
        "0.09090909090909"
    );
}

#[test]
fn courant_friedrichs_lewy_explicit_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", courant_friedrichs_lewy(340, 180, 480))"#),
        "127.50000000000000"
    );
}

#[test]
fn reynolds_full_number_variant_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", reynolds_full_number(15, 0.02, 1.5e-5))"#),
        "20000.00000000000000"
    );
}

#[test]
fn prandtl_number_step_kinematic_over_alpha_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", prandtl_number_step(1.5e-5, 2.06e-5))"#),
        "0.72815533980583"
    );
}

#[test]
fn turbulent_kinetic_energy_three_components_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", turbulent_kinetic_energy_step([1.2, -0.8, 0.6]))"#),
        "1.22000000000000"
    );
}

#[test]
fn resistance_parallel_three_resistors_array_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", resistance_parallel([100, 200, 300]))"#),
        "54.54545454545455"
    );
}

#[test]
fn resistance_parallel_variadic_ignores_trailing_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", resistance_parallel(100, 200))"#),
        "100.00000000000000"
    );
}

#[test]
fn resistance_series_array_sum_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", resistance_series([120, 330, 910]))"#),
        "1360.00000000000000"
    );
}

#[test]
fn resistance_series_variadic_first_only_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", resistance_series(120, 330))"#),
        "120.00000000000000"
    );
}

#[test]
fn capacitance_parallel_series_array_buckets_cf() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.14f", capacitance_parallel([22e-9, 47e-9])) . sprintf("\n%.14f", capacitance_series([2.2e-6, 4.7e-6]))"#
        ),
        "0.00000006900000\n0.00000149855072"
    );
}

#[test]
fn inductance_parallel_formula_matches_reciprocal_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", inductance_parallel([4.7e-3, 10e-3]))"#),
        "0.00319727891156"
    );
}

#[test]
fn inductance_series_linear_sum_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", inductance_series([1e-3, 2.2e-3]))"#),
        "0.00320000000000"
    );
}

#[test]
fn voltage_divider_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", voltage_divider(12, 4700, 1000))"#),
        "2.10526315789474"
    );
}

#[test]
fn current_divider_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", current_divider(0.5, 8, 24))"#),
        "0.37500000000000"
    );
}

#[test]
fn lc_resonant_hertz_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", lc_resonant(10e-6, 100e-12))"#),
        "5032921.21044870372862"
    );
}

#[test]
fn q_factor_rlc_underdamped_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", q_factor_rlc(50e-6, 20e-12, 12))"#),
        "131.76156917368249"
    );
}

#[test]
fn db_voltage_two_reference_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", dB_voltage(2, 1))"#),
        "6.02059991327962"
    );
}

#[test]
fn db_power_two_reference_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", dB_power(10, 1))"#),
        "10.00000000000000"
    );
}

#[test]
fn db_voltage_missing_reference_balloons_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", dB_voltage(2))"#),
        "606.02059991327963"
    );
}

#[test]
fn skin_depth_copper_microwave_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.17e", skin_depth(5.8e7, 900e6))"#),
        "3.19864970984752474e-13"
    );
}

#[test]
fn wire_resistance_copper_slug_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", wire_resistance(1.7e-8, 50, 2.5))"#),
        "0.00000034000000"
    );
}

#[test]
fn motor_torque_from_power_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", motor_torque(3.2, 1800))"#),
        "0.00177777777778"
    );
}

#[test]
fn efficiency_ratio_output_over_input_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", efficiency_ratio(760, 980))"#),
        "0.77551020408163"
    );
}

#[test]
fn apy_to_apr_monthly_inverse_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", apy_to_apr(0.049, 12))"#),
        "0.04793280666380"
    );
}

#[test]
fn compound_interest_nominal_twenty_four_per_year_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", compound_interest_periods(1000, 0.048, 24, 60))"#),
        "17763.10998939525234"
    );
}

#[test]
fn simple_interest_three_terms_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", simple_interest_compute(5000, 0.06, 18))"#),
        "5400.00000000000000"
    );
}

#[test]
fn dewpoint_temperature_full_mass_fraction_gamma_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", dewpoint_temperature_full(22, 0.65))"#),
        "15.11024819677105"
    );
}

#[test]
fn relative_humidity_step_ratio_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", relative_humidity_step(28, 3100))"#),
        "0.00903225806452"
    );
}

#[test]
fn wet_bulb_potential_theta_e_minus_offset_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", wet_bulb_potential(301))"#),
        "28.00000000000000"
    );
}

#[test]
fn mixing_length_prandtl_kappa_z_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", mixing_length_prandtl(12, 0.41))"#),
        "4.92000000000000"
    );
}

#[test]
fn virtual_temperature_moisture_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", virtual_temperature_full(293, 0.015))"#),
        "295.67216000000002"
    );
}

#[test]
fn potential_temperature_isa_reference_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", potential_temperature_step(295, 100000, 100000))"#),
        "295.00000000000000"
    );
}

#[test]
fn clausius_clapeyron_saturation_hpa_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.8f", clausius_clapeyron_full(298.15))"#),
        "3223.90238317"
    );
}

#[test]
fn stefan_boltzmann_grey_body_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", stefan_boltzmann_radiation(310))"#),
        "523.67098538092989"
    );
}

#[test]
fn albedo_blackbody_equilibrium_temp_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", albedo_blackbody_balance(1361, 0.3))"#),
        "254.57814011657169"
    );
}

#[test]
fn kolmogorov_microscale_from_nu_eps_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", kolmogorov_microscale(1.5e-5, 0.005))"#),
        "0.00090641261921"
    );
}

#[test]
fn adiabatic_lapse_rate_dry_scalar_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", adiabatic_lapse_rate_dry())"#),
        "0.00976757968127"
    );
}

#[test]
fn monin_obukhov_stable_length_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", monin_obukhov_length(0.35, 295, -0.02))"#),
        "161.21872657839319"
    );
}

#[test]
fn density_altitude_full_isa_offset_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", density_altitude_full(500, 303))"#),
        "2672.00000000000273"
    );
}

#[test]
fn vo2_max_estimate_cooper_style_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", vo2_max_estimate(42, 182, 65))"#),
        "14.96373626373626"
    );
}

#[test]
fn max_heart_rate_linear_age_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", max_heart_rate(35))"#),
        "185.00000000000000"
    );
}

#[test]
fn target_heart_rate_percent_reserve_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", target_heart_rate(155, 0.65))"#),
        "45.69499999999999"
    );
}

#[test]
fn bmr_harris_benedict_male_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", bmr_harris_benedict_male(182, 80, 40))"#),
        "2683.45600000000013"
    );
}

#[test]
fn mean_arterial_pressure_map_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", mean_arterial_pressure(120, 80))"#),
        "93.33333333333333"
    );
}

#[test]
fn pulse_pressure_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", pulse_pressure(120, 75))"#),
        "45.00000000000000"
    );
}

#[test]
fn knapsack_01_dp_value_max_step_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", knapsack_01_dp_value(7, 9))"#),
        "9.00000000000000"
    );
}

#[test]
fn knapsack_fractional_density_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", knapsack_fractional_step(30, 5))"#),
        "6.00000000000000"
    );
}

#[test]
fn days_in_year_leap_cf() {
    assert_eq!(eval_string(r#"days_in_year(2028)"#), "366");
}

#[test]
fn zeller_thursday_independence_day_cf() {
    assert_eq!(eval_string(r#"zeller_day_of_week(2024, 7, 4)"#), "3");
}

#[test]
fn age_from_birthdate_same_calendar_day_cf() {
    assert_eq!(
        eval_string(r#"age_from_birthdate(1971, 5, 9, 2026, 5, 9)"#),
        "55"
    );
}

#[test]
fn geohash_encode_precision_six_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%s", geohash_encode(37.7749, -122.4194, 6))"#),
        "9q8yyk"
    );
}

#[test]
fn palindromic_q_case_insensitive_cf() {
    assert_eq!(eval_string(r#"palindromic_q("Racecar")"#), "1");
}

#[test]
fn substring_count_overlapping_matches_cf() {
    assert_eq!(eval_string(r#"substring_count("abababa", "aba")"#), "2");
}

#[test]
fn string_camel_to_snake_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%s", string_camel_to_snake("FooBar"))"#),
        "foo_bar"
    );
}

#[test]
fn string_normalize_spaces_cf() {
    assert_eq!(
        eval_string(r#"sprintf("%s", string_normalize_spaces("  a   b c  "))"#),
        "a b c"
    );
}

#[test]
fn graph_density_complete_three_cf() {
    assert_eq!(
        eval_string(
            r#"my $k3 = [[1,2],[0,2],[0,1]];
sprintf("%.14f", graph_density($k3))"#
        ),
        "1.00000000000000"
    );
}

#[test]
fn graph_is_tree_path_three_cf() {
    assert_eq!(eval_string(r#"graph_is_tree([[1],[0,2],[1]])"#), "1");
}
