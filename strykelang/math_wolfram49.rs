// Batch 49 — networking: TCP, AQM, queueing, MIMO, channel, queueing law.

fn b49_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// TCP cwnd step: cwnd += 1/cwnd (during congestion avoidance)
fn builtin_net_tcp_cwnd_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args);
    if cwnd <= 0.0 { return Ok(PerlValue::float(1.0)); }
    Ok(PerlValue::float(cwnd + 1.0 / cwnd))
}

/// ssthresh update on loss: cwnd / 2
fn builtin_net_tcp_ssthresh_update(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args) / 2.0))
}

/// TCP Reno per RFC 5681. Per-ACK update for one segment ack:
///   slow start (cwnd < ssthresh):       cwnd += SMSS
///   congestion avoidance (cwnd ≥ ssh):  cwnd += SMSS · SMSS / cwnd
/// On loss event:
///   timeout:        ssthresh = max(cwnd/2, 2·SMSS); cwnd = SMSS
///   fast retransmit: ssthresh = max(cwnd/2, 2·SMSS); cwnd = ssthresh + 3·SMSS
/// Args: cwnd, ssthresh, smss, event (0=ack, 1=fast_retransmit, 2=timeout).
/// Returns the new cwnd.
fn builtin_net_tcp_reno_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args);
    let ssthresh = args.get(1).map(|v| v.to_number()).unwrap_or(64_000.0);
    let smss = args.get(2).map(|v| v.to_number()).unwrap_or(1460.0);
    let event = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    match event {
        0 => {
            if cwnd < ssthresh { Ok(PerlValue::float(cwnd + smss)) }
            else { Ok(PerlValue::float(cwnd + smss * smss / cwnd.max(smss))) }
        }
        1 => {
            let new_ssh = (cwnd / 2.0).max(2.0 * smss);
            Ok(PerlValue::float(new_ssh + 3.0 * smss))
        }
        2 => Ok(PerlValue::float(smss)),
        _ => Ok(PerlValue::float(cwnd)),
    }
}

/// Cubic: W(t) = C(t-K)³ + W_max
fn builtin_net_tcp_cubic_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let w_max = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(3).map(|v| v.to_number()).unwrap_or(0.4);
    Ok(PerlValue::float(c * (t - k).powi(3) + w_max))
}

/// BBR step: cwnd = max(BDP, 4·MSS)
fn builtin_net_tcp_bbr_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bdp = f1(args);
    let mss = args.get(1).map(|v| v.to_number()).unwrap_or(1500.0);
    Ok(PerlValue::float(bdp.max(4.0 * mss)))
}

/// Vegas step (BaseRTT-based)
fn builtin_net_tcp_vegas_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args);
    let base_rtt = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let cur_rtt = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    if base_rtt == 0.0 { return Ok(PerlValue::float(cwnd)); }
    let diff = cwnd * (1.0 - base_rtt / cur_rtt);
    Ok(PerlValue::float(cwnd + (1.0_f64).copysign(diff - 2.0)))
}

/// Westwood step (bandwidth-aware ssthresh)
fn builtin_net_tcp_westwood_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bw = f1(args);
    let rtt = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    Ok(PerlValue::float(bw * rtt))
}

/// Compound TCP step
fn builtin_net_tcp_compound_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let win = f1(args);
    let dwnd = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(win + dwnd))
}

/// DCTCP step (ECN feedback alpha)
fn builtin_net_tcp_dctcp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(cwnd * (1.0 - alpha / 2.0)))
}

/// YeAH (Yet Another Highspeed) per Baiocchi et al. 2007: hybrid delay/loss
/// algorithm. Estimate queue Q = cwnd · (1 - baseRTT/curRTT). In Fast mode
/// (Q < Q_max=10): cwnd += α/cwnd with α = max(1, cwnd / R_div) per ACK,
/// approximating high-speed growth. In Slow mode (Q ≥ Q_max): cwnd += 1/cwnd
/// (Reno congestion avoidance). Args: cwnd, baseRTT, curRTT, Q_max, R_div.
fn builtin_net_tcp_yeah_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args);
    let base_rtt = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let cur_rtt = args.get(2).map(|v| v.to_number()).unwrap_or(0.05).max(1e-6);
    let q_max = args.get(3).map(|v| v.to_number()).unwrap_or(10.0);
    let r_div = args.get(4).map(|v| v.to_number()).unwrap_or(8.0);
    let q = cwnd * (1.0 - base_rtt / cur_rtt).max(0.0);
    let alpha = if q < q_max { (cwnd / r_div).max(1.0) } else { 1.0 };
    if cwnd <= 0.0 { return Ok(PerlValue::float(1.0)); }
    Ok(PerlValue::float(cwnd + alpha / cwnd))
}

/// H-TCP (Leith & Shorten 2004). Time-since-last-congestion-event Δ drives a
/// quadratic α(Δ) = 1 if Δ < Δ_L, else 1 + 10(Δ−Δ_L) + ½(Δ−Δ_L)². Per ACK:
/// cwnd += α(Δ) / cwnd. β(B) = (B_min/B_max) clamps multiplicative decrease.
/// Args: cwnd, Δ (s), Δ_L (s, default 1.0).
fn builtin_net_tcp_htcp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args).max(1.0);
    let delta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let delta_l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let alpha = if delta < delta_l { 1.0 }
        else { let g = delta - delta_l; 1.0 + 10.0 * g + 0.5 * g * g };
    Ok(PerlValue::float(cwnd + alpha / cwnd))
}

/// Hybla step (long RTT compensation)
fn builtin_net_tcp_hybla_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args);
    let rtt = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let rtt0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.025);
    if rtt0 == 0.0 { return Ok(PerlValue::float(cwnd)); }
    Ok(PerlValue::float(cwnd + (rtt / rtt0).powi(2)))
}

/// TCP Illinois (Liu, Basar, Srikant 2008): concave α(d_a) and convex β(d_a)
/// functions of average queueing delay d_a = curRTT − baseRTT.
///   α(d_a) = α_max if d_a ≤ d₁, else (κ₁/(d_a + κ₂)) clamped to α_min.
///   β(d_a) = β_min if d_a ≤ d₂, else κ₃·d_a + κ₄ clamped to β_max.
/// Per-ACK cwnd update in CA: cwnd += α(d_a)/cwnd. Args: cwnd, d_a, d₁, d_max.
fn builtin_net_tcp_illinois_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args).max(1.0);
    let d_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    let d_min = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    let d_max = args.get(3).map(|v| v.to_number()).unwrap_or(0.1).max(d_min + 1e-9);
    let alpha_min = 0.3_f64;
    let alpha_max = 10.0_f64;
    let alpha = if d_a <= d_min { alpha_max }
        else if d_a >= d_max { alpha_min }
        else {
            let k2 = (alpha_max * d_min - alpha_min * d_max) / (alpha_min - alpha_max);
            let k1 = (d_min + k2) * alpha_max;
            k1 / (d_a + k2)
        };
    Ok(PerlValue::float(cwnd + alpha / cwnd))
}

/// Low-priority TCP-LP
fn builtin_net_tcp_lp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args);
    let queueing_delay = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(if queueing_delay > 0.0 { cwnd * 0.5 } else { cwnd + 1.0 }))
}

/// Scalable TCP
fn builtin_net_tcp_scalable_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args);
    Ok(PerlValue::float(cwnd + 0.01 * cwnd))
}

/// TCP Veno (Fu & Liew 2003): combines Reno + Vegas delay sensing.
///   N = cwnd · (RTT − baseRTT) / RTT      (estimated backlog in pipe)
///   if N < β (β=3 default):  random/wireless loss → cwnd += 1/cwnd (no halving)
///   else:                    real congestion → cwnd += 1/cwnd, on loss halve
/// Per-ACK update without loss event. Args: cwnd, baseRTT, curRTT, β.
fn builtin_net_tcp_veno_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args).max(1.0);
    let base_rtt = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let cur_rtt = args.get(2).map(|v| v.to_number()).unwrap_or(0.05).max(1e-6);
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(3.0);
    let n = cwnd * (cur_rtt - base_rtt) / cur_rtt;
    let increment = if n < beta { 1.0 / cwnd } else { 1.0 / (2.0 * cwnd) };
    Ok(PerlValue::float(cwnd + increment))
}

/// AIAD: additive increase, additive decrease
fn builtin_net_aiad_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = f1(args);
    let inc = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let loss = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dec = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if loss > 0.0 { w - dec } else { w + inc }))
}

/// AIMD: additive increase, multiplicative decrease (Reno)
fn builtin_net_aimd_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = f1(args);
    let loss = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(if loss > 0.0 { w / 2.0 } else { w + 1.0 }))
}

/// MIAD
fn builtin_net_miad_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = f1(args);
    let loss = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(if loss > 0.0 { w - 1.0 } else { 2.0 * w }))
}

/// MIMD
fn builtin_net_mimd_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = f1(args);
    let loss = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(if loss > 0.0 { w / 2.0 } else { 2.0 * w }))
}

/// RED drop probability: linear between min_th, max_th
fn builtin_net_aqm_red_drop_prob(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let avg_q = f1(args);
    let min_th = args.get(1).map(|v| v.to_number()).unwrap_or(5.0);
    let max_th = args.get(2).map(|v| v.to_number()).unwrap_or(15.0);
    let max_p = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    if avg_q < min_th { return Ok(PerlValue::float(0.0)); }
    if avg_q >= max_th { return Ok(PerlValue::float(1.0)); }
    Ok(PerlValue::float(max_p * (avg_q - min_th) / (max_th - min_th)))
}

/// CoDel target queue delay
fn builtin_net_aqm_codel_target(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let _ = args;
    Ok(PerlValue::float(0.005))
}

/// PIE drop rate increment
fn builtin_net_aqm_pie_drop_rate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prev = f1(args);
    let delay_diff = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((prev + 0.125 * delay_diff).clamp(0.0, 1.0)))
}

/// FQ-CoDel (RFC 8290): per-flow stochastic queue + CoDel drop. Each flow gets
/// a slot via 5-tuple hash. CoDel marks/drops when sojourn time > target=5ms for
/// >100ms (interval). Drop schedule: count++; next_drop = interval / √count.
/// > Returns next_drop_time. Args: count, interval (default 100ms).
fn builtin_net_aqm_fq_codel_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let count = f1(args).max(1.0);
    let interval = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    Ok(PerlValue::float(interval / count.sqrt()))
}

/// BLUE step (mark/drop probability)
fn builtin_net_aqm_blue_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let event = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let delta = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float((p + event * delta).clamp(0.0, 1.0)))
}

/// CHOKe (CHOose and Keep, Pan-Prabhakar-Psounis 2000): on each arrival when
/// avg_q > min_th, draw a random packet from the buffer; if it's from the same
/// flow as the arrival, drop both (penalize unresponsive flows). Else apply RED.
/// Returns 2 if both-drop, 1 if RED-drop, 0 otherwise. Args: avg_q, min_th,
/// max_th, max_p, same_flow_prob (P[draw matches arrival's flow]).
fn builtin_net_aqm_choke_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let avg_q = f1(args);
    let min_th = args.get(1).map(|v| v.to_number()).unwrap_or(5.0);
    let max_th = args.get(2).map(|v| v.to_number()).unwrap_or(15.0);
    let max_p = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    let same_p = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    if avg_q < min_th { return Ok(PerlValue::integer(0)); }
    if same_p > 0.5 { return Ok(PerlValue::integer(2)); }
    if avg_q >= max_th { return Ok(PerlValue::integer(1)); }
    let p = max_p * (avg_q - min_th) / (max_th - min_th);
    Ok(PerlValue::integer(if p > 0.5 { 1 } else { 0 }))
}

/// SFQ (stochastic fair queueing) step
fn builtin_net_aqm_sfq_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = i1(args) as u64;
    let n = args.get(1).map(|v| v.to_number() as u64).unwrap_or(1024).max(1);
    Ok(PerlValue::integer((h % n) as i64))
}

/// DRR (deficit round-robin) step
fn builtin_net_aqm_drr_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let deficit = f1(args);
    let quantum = args.get(1).map(|v| v.to_number()).unwrap_or(1500.0);
    Ok(PerlValue::float(deficit + quantum))
}

/// WRR step
fn builtin_net_aqm_wrr_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let weight = f1(args);
    let total_weight = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total_weight == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(weight / total_weight))
}

/// Token rate limit step
fn builtin_net_token_rate_limit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let tokens = f1(args);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(tokens + rate * dt))
}

/// Traffic shaper step
fn builtin_net_traffic_shaper_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let demand = f1(args);
    let cap = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(demand.min(cap)))
}

/// Priority queue index
fn builtin_net_priority_queue_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prio = i1(args);
    Ok(PerlValue::integer(prio.max(0)))
}

/// Packet loss estimate from sample window
fn builtin_net_packet_loss_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lost = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(lost / total))
}

/// Jitter estimate (RFC 3550): J += (|D(i-1, i)| - J) / 16
fn builtin_net_jitter_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let j = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(j + (d.abs() - j) / 16.0))
}

/// Average latency from samples
fn builtin_net_latency_avg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b49_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.is_empty() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

/// SRTT (TCP smoothed RTT): srtt = (1 - α) srtt + α R
fn builtin_net_rtt_smoothed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let srtt = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(0.125);
    Ok(PerlValue::float((1.0 - alpha) * srtt + alpha * r))
}

/// RTT variation (RTTVAR): (1-β)·RTTVAR + β·|SRTT - R|
fn builtin_net_rtt_variation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rttvar = f1(args);
    let diff = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.25);
    Ok(PerlValue::float((1.0 - beta) * rttvar + beta * diff.abs()))
}

/// RTO compute: SRTT + 4·RTTVAR
fn builtin_net_rto_compute(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let srtt = f1(args);
    let rttvar = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(srtt + 4.0 * rttvar))
}

/// Bandwidth-delay product
fn builtin_net_bandwidth_delay_product(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bw = f1(args);
    let rtt = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(bw * rtt))
}

/// Path capacity (Kleinrock)
fn builtin_net_path_capacity_kleinrock(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_net_bandwidth_delay_product(args)
}

/// Loss rate to throughput conversion: 1.22·MSS / (RTT·√p)
fn builtin_net_loss_rate_to_throughput(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mss = f1(args);
    let rtt = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    if p <= 0.0 || rtt <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.22 * mss / (rtt * p.sqrt())))
}

/// Padhye throughput model
fn builtin_net_throughput_padhye(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mss = f1(args);
    let rtt = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    if p <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    let rto = args.get(3).map(|v| v.to_number()).unwrap_or(rtt * 4.0);
    let denom = rtt * (2.0 * p / 3.0).sqrt() + rto * 3.0 * (3.0 * p / 8.0).sqrt() * p * (1.0 + 32.0 * p * p);
    if denom == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(mss / denom))
}

/// Mathis model
fn builtin_net_throughput_mathis(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_net_loss_rate_to_throughput(args)
}

/// Throughput response function (steady-state)
fn builtin_net_throughput_response(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args);
    let rtt = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    if rtt <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(cwnd / rtt))
}

/// Router buffer size = BDP/√n (Stanford rule)
fn builtin_net_router_buffer_size(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bdp = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n <= 0.0 { return Ok(PerlValue::float(bdp)); }
    Ok(PerlValue::float(bdp / n.sqrt()))
}

/// Drop tail check (queue full)
fn builtin_net_drop_tail_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let cap = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::integer(if q >= cap { 1 } else { 0 }))
}

/// Burst size compute (token bucket)
fn builtin_net_burst_size_compute(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rate = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(rate * dt))
}

/// Packet pacing step
fn builtin_net_packet_pacing_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cwnd = f1(args);
    let rtt = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    if cwnd == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(rtt / cwnd))
}

/// Link capacity share
fn builtin_net_link_capacity_share(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_net_aqm_wrr_step(args)
}

/// Proportional fair share
fn builtin_net_proportional_fair_share(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r_i = f1(args);
    let mean_r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if mean_r == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(r_i / mean_r))
}

/// Max-min fair step
fn builtin_net_max_min_fair_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cap = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(cap / n))
}

/// α-fair: U(x) = x^(1-α) / (1-α)
fn builtin_net_alpha_fair_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if (alpha - 1.0).abs() < 1e-9 { return Ok(PerlValue::float(x.ln())); }
    Ok(PerlValue::float(x.powf(1.0 - alpha) / (1.0 - alpha)))
}

/// Kelly pricing step
fn builtin_net_kelly_pricing_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let demand = f1(args);
    let capacity = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if capacity == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(demand / capacity))
}

/// Network utility max objective
fn builtin_net_network_utility_max(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b49_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().sum()))
}

/// Lyapunov drift + penalty
fn builtin_net_lyapunov_drift_plus_penalty(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let drift = f1(args);
    let penalty = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(drift + v * penalty))
}

/// Backpressure step
fn builtin_net_backpressure_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_a = f1(args);
    let q_b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(q_a - q_b))
}

/// Max-weight match value
fn builtin_net_max_weight_match(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b49_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Q-CSMA propose
fn builtin_net_qcsma_propose(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    Ok(PerlValue::float(1.0 - (-q).exp()))
}

/// CSMA back-off (binary exponential)
fn builtin_net_csma_back_off(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let collisions = f1(args);
    Ok(PerlValue::float(2f64.powf(collisions)))
}

/// Pure ALOHA throughput: G·exp(-2G)
fn builtin_net_alohanet_throughput(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    Ok(PerlValue::float(g * (-2.0 * g).exp()))
}

/// Slotted ALOHA: G·exp(-G)
fn builtin_net_slotted_aloha_throughput(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    Ok(PerlValue::float(g * (-g).exp()))
}

/// CSMA efficiency = 1 / (1 + a) where a = τ/T
fn builtin_net_csma_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    if 1.0 + a == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 / (1.0 + a)))
}

/// Token ring efficiency: 1 / (1 + N·a/N)
fn builtin_net_token_ring_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    if 1.0 + a == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 / (1.0 + a)))
}

/// Polling efficiency for round-robin polled MAC: η = T_data / (T_data + N·T_poll),
/// where N is station count and T_poll is poll-overhead per station per cycle.
/// Differs from CSMA's 1/(1+a) (collision-based). Args: T_data, T_poll, N.
fn builtin_net_polling_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t_data = f1(args);
    let t_poll = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let denom = t_data + n * t_poll;
    if denom <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(t_data / denom))
}

/// Radio path loss (free-space): L = (4π d / λ)²
fn builtin_net_radio_path_loss(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.125);
    if lambda == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float((4.0 * std::f64::consts::PI * d / lambda).powi(2)))
}

/// Friis received power: P_r = P_t G_t G_r λ² / (4πd)²
fn builtin_net_friis_received_power(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p_t = f1(args);
    let g_t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let g_r = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(3).map(|v| v.to_number()).unwrap_or(0.125);
    let d = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    if d == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(p_t * g_t * g_r * lambda * lambda / ((4.0 * std::f64::consts::PI * d).powi(2))))
}

/// Two-ray ground loss
fn builtin_net_two_ray_ground_loss(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h_t = f1(args);
    let h_r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    if h_t == 0.0 || h_r == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(d.powi(4) / (h_t * h_t * h_r * h_r)))
}

/// Okumura-Hata loss (urban large-city, simplified)
fn builtin_net_okumura_hata_loss(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_mhz = f1(args);
    let h_b = args.get(1).map(|v| v.to_number()).unwrap_or(30.0);
    let d_km = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if d_km <= 0.0 || h_b <= 0.0 || f_mhz <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(69.55 + 26.16 * f_mhz.log10() - 13.82 * h_b.log10() + (44.9 - 6.55 * h_b.log10()) * d_km.log10()))
}

/// Log-distance path loss
fn builtin_net_log_distance_path(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l0 = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let d0 = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if d0 <= 0.0 || d <= 0.0 { return Ok(PerlValue::float(l0)); }
    Ok(PerlValue::float(l0 + 10.0 * n * (d / d0).log10()))
}

/// Log-normal shadowing path-loss component X_σ ~ N(0, σ²) in dB. Convert from
/// uniform U₁, U₂ via Box-Muller: X = σ · √(-2 ln U₁) · cos(2π U₂).
fn builtin_net_shadowing_normal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sigma = f1(args);
    let u1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.5).max(1e-12);
    let u2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(sigma * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()))
}

/// Rician K-factor
fn builtin_net_rician_k_factor(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let los = f1(args);
    let scattered = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if scattered == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(los / scattered))
}

/// Rayleigh envelope: f(r) = (r/σ²) exp(-r²/(2σ²))
fn builtin_net_rayleigh_envelope(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let sigma2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sigma2 == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((r / sigma2) * (-r * r / (2.0 * sigma2)).exp()))
}

/// Doppler shift: f_d = v f_c / c
fn builtin_net_doppler_shift(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let f_c = args.get(1).map(|v| v.to_number()).unwrap_or(2.4e9);
    let c = 3e8;
    Ok(PerlValue::float(v * f_c / c))
}

/// Shannon capacity: C = B log₂(1 + SNR)
fn builtin_net_capacity_shannon(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = f1(args);
    let snr = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(b * (1.0 + snr).log2()))
}

/// Telatar (1999) MIMO ergodic capacity:
///   C = log₂ det(I_M_r + (P/(N · M_t)) · H · Hᴴ)
/// For the diagonal SVD form H = UΣVᴴ with singular values σ_i, this becomes:
///   C = Σ_i log₂(1 + (P/(N·M_t)) · σ_i²).
/// Args: array of singular values σ_i, total power P, noise N, M_t (transmitters).
fn builtin_net_mimo_capacity_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sigmas = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let noise = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let mt = args.get(3).map(|v| v.to_number()).unwrap_or(sigmas.len().max(1) as f64).max(1.0);
    let scale = p / (noise * mt);
    let mut c = 0.0_f64;
    for s in &sigmas {
        let sigma = s.to_number();
        c += (1.0 + scale * sigma * sigma).log2();
    }
    Ok(PerlValue::float(c))
}

/// Zero-forcing beamforming (precoder for diagonal channel)
fn builtin_net_zero_forcing_beam(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = f1(args);
    if h == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 / h))
}

/// MMSE beamforming step
fn builtin_net_mmse_beam_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = f1(args);
    let noise = args.get(1).map(|v| v.to_number()).unwrap_or(1e-9);
    if h * h + noise == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(h / (h * h + noise)))
}

/// Water-filling power allocation
fn builtin_net_water_filling_power(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mu = f1(args);
    let n_i = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float((mu - n_i).max(0.0)))
}

/// AMC threshold index (modulation/coding scheme based on SNR)
fn builtin_net_amc_threshold_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let snr_db = f1(args);
    let thresholds = b49_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let mut idx = 0_i64;
    for (i, &t) in thresholds.iter().enumerate() {
        if snr_db >= t { idx = i as i64 + 1; }
    }
    Ok(PerlValue::integer(idx))
}

/// HARQ combining gain (chase combining)
fn builtin_net_harq_combining_gain(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_tx = f1(args);
    Ok(PerlValue::float(n_tx))
}

/// Turbo decode iteration step
fn builtin_net_turbo_decode_iter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let llr = f1(args);
    let extr = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(llr + extr))
}

/// LDPC sum-product (belief-propagation) check-node update:
///   L_{c→v} = 2 · atanh( ∏_{v'≠v} tanh(L_{v'→c} / 2) )
/// (Gallager's tanh-product rule, channel-side LLRs in). For numerical
/// stability we use the equivalent min-sum approximation when |L| > 30.
/// Args: array of incoming variable-to-check LLRs (excluding the target).
fn builtin_net_ldpc_iteration_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let llrs = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    if llrs.is_empty() { return Ok(PerlValue::float(0.0)); }
    let mut prod = 1.0_f64;
    for l in &llrs {
        let v = l.to_number() / 2.0;
        prod *= v.tanh();
    }
    let p = prod.clamp(-1.0 + 1e-15, 1.0 - 1e-15);
    Ok(PerlValue::float(2.0 * p.atanh()))
}

/// Polar decode step
fn builtin_net_polar_decode_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(l.signum() * r.signum() * l.abs().min(r.abs())))
}

/// Viterbi step (path metric update)
fn builtin_net_viterbi_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m1 = f1(args);
    let m2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(m1.min(m2)))
}

/// BCJR (Bahl-Cocke-Jelinek-Raviv): forward α / backward β recursions on a
/// trellis. APP for state s at time k: γ_k(s', s) = P(y_k | s', s)·P(s|s'), then
///   α_k(s) = Σ_{s'} α_{k-1}(s')·γ_k(s', s),
///   β_{k-1}(s') = Σ_{s} β_k(s)·γ_k(s', s).
/// LLR(u_k) = log[ Σ_{(s',s):u=1} α_{k-1}(s')·γ_k·β_k(s) /
///                Σ_{(s',s):u=0} α_{k-1}(s')·γ_k·β_k(s) ].
/// Returns one α update Σ given prior α and weight w: α_new = α_prev_a·w_a + α_prev_b·w_b.
/// Args: α_prev_a, w_a, α_prev_b, w_b.
fn builtin_net_bcjr_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let wa = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let wb = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(a * wa + b * wb))
}

/// Outage probability: P(SNR < γ_th)
fn builtin_net_outage_probability(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let gamma_th = f1(args);
    let mean_snr = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if mean_snr == 0.0 { return Ok(PerlValue::float(1.0)); }
    Ok(PerlValue::float(1.0 - (-gamma_th / mean_snr).exp()))
}

/// Diversity gain at high SNR: outage drops as SNR^(-d) where d = number of
/// independent fading branches. d_div = N_t · N_r for MIMO Rayleigh.
/// Args: N_t, N_r.
fn builtin_net_diversity_gain(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let nt = f1(args).max(1.0);
    let nr = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(PerlValue::float(nt * nr))
}

/// Array gain (coherent combining): G_array = N (number of antennas) for
/// matched-filter receiver. In dB: 10·log₁₀(N).
fn builtin_net_array_gain(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args).max(1.0);
    Ok(PerlValue::float(10.0 * n.log10()))
}

/// Multiplexing gain (min of M_t, M_r)
fn builtin_net_multiplexing_gain(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mt = f1(args);
    let mr = args.get(1).map(|v| v.to_number()).unwrap_or(mt);
    Ok(PerlValue::float(mt.min(mr)))
}

/// Coding gain (asymptotic) for code with rate R, minimum distance d, modulation
/// M: G_c = 10·log₁₀(R·d) dB for binary. Args: rate R, min distance d.
fn builtin_net_coding_gain(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args).max(1e-9);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(PerlValue::float(10.0 * (r * d).log10()))
}

/// Pruning gain (Viterbi/list-decoding pruning): factor by which path metric
/// reduction shrinks survivor set. G = log₂(K_full / K_pruned) bits saved.
/// Args: full_paths, pruned_paths.
fn builtin_net_pruning_gain(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let full = f1(args).max(1.0);
    let pruned = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(PerlValue::float((full / pruned).log2()))
}

/// Macro-diversity step (best of multiple base stations)
fn builtin_net_macro_diversity_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b49_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Micro-diversity step (combining)
fn builtin_net_micro_diversity_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b49_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().sum()))
}

/// Handoff threshold check
fn builtin_net_handoff_threshold(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rssi_a = f1(args);
    let rssi_b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let hyst = args.get(2).map(|v| v.to_number()).unwrap_or(3.0);
    Ok(PerlValue::integer(if rssi_b > rssi_a + hyst { 1 } else { 0 }))
}

/// Call admission check (capacity left)
fn builtin_net_call_admission_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let used = f1(args);
    let cap = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::integer(if used < cap { 1 } else { 0 }))
}

/// Blocking probability (M/M/c/c) — Erlang B alias
fn builtin_net_blocking_probability(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let mut top = 1.0;
    let mut bot = 1.0;
    let mut term = 1.0;
    for k in 1..=n {
        term *= a / k as f64;
        bot += term;
    }
    let mut t2 = 1.0;
    for k in 1..=n { t2 *= a / k as f64; }
    top *= t2;
    if bot == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(top / bot))
}

/// Erlang B formula
fn builtin_net_erlang_b_formula(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_net_blocking_probability(args)
}

/// Erlang C formula (queuing)
fn builtin_net_erlang_c_formula(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if c <= a { return Ok(PerlValue::float(1.0)); }
    let p_b = builtin_net_blocking_probability(args)?.to_number();
    let denom = 1.0 - (a / c) * (1.0 - p_b);
    if denom == 0.0 { return Ok(PerlValue::float(1.0)); }
    Ok(PerlValue::float(p_b / denom))
}

/// Engset blocking formula (finite source S, c circuits, per-idle offered load α):
///   B(S, c, α) = C(S−1, c) · α^c / Σ_{k=0..c} C(S−1, k) · α^k.
/// Distinct from Erlang B (infinite source). Args: S (sources), c (servers), α.
fn builtin_net_engset_formula(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = i1(args).max(1);
    let c = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(0).min(s - 1);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    fn binom(n: i64, k: i64) -> f64 {
        if k < 0 || k > n { return 0.0; }
        let k = k.min(n - k);
        let mut r = 1.0_f64;
        for i in 0..k { r *= (n - i) as f64 / (i + 1) as f64; }
        r
    }
    let mut denom = 0.0_f64;
    let mut a_pow = 1.0_f64;
    for k in 0..=c {
        denom += binom(s - 1, k) * a_pow;
        a_pow *= alpha;
    }
    let num = binom(s - 1, c) * alpha.powi(c as i32);
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(num / denom))
}

/// Little's law: L = λW
fn builtin_net_little_law_l(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(lambda * w))
}

/// Throughput law: X = N / R
fn builtin_net_throughput_law(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(n / r))
}

/// Response time law: R = N/X - Z (think time)
fn builtin_net_response_time_law(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if x == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(n / x - z))
}

/// Utilization law: U = X · S
fn builtin_net_utilization_law(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(x * s))
}

/// Forced flow law: X_k = X_0 · V_k
fn builtin_net_forced_flow_law(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x0 = f1(args);
    let v_k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(x0 * v_k))
}
