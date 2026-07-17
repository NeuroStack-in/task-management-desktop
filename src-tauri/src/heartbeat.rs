//! Heartbeat collection (`sysinfo`). Latest-only telemetry that folds to the `AgentDevice` fleet row
//! (LLD §18). Sent every 300 s while signed in — **auth-gated, not timer-gated** — so the fleet table
//! stays honest about an agent that is online but not currently tracking (BUILD-PLAN §4, Thread B).

use sysinfo::System;
use wp_agent_contract::Heartbeat;

/// Snapshot host health. `cpu_pct` needs two samples to be meaningful, so the very first cycle reads
/// ~0 (the 300 s loop refreshes across ticks). `outbox_mb` and `idle` are supplied by the caller
/// (the outbox knows its backlog; the monitor knows idle).
pub fn collect(agent_version: &str, outbox_mb: f32, idle: bool) -> Heartbeat {
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let total = sys.total_memory();
    let mem_pct = if total > 0 {
        (sys.used_memory() as f64 / total as f64 * 100.0) as f32
    } else {
        0.0
    };

    Heartbeat {
        hostname: System::host_name().unwrap_or_default(),
        os: std::env::consts::OS.to_string(),
        os_version: System::os_version().unwrap_or_default(),
        agent_version: agent_version.to_string(),
        ip: local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_default(),
        cpu_pct: sys.global_cpu_usage(),
        mem_pct,
        outbox_mb,
        idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_fills_identity_and_bounded_mem() {
        let hb = collect("9.9.9", 1.5, true);
        assert_eq!(hb.agent_version, "9.9.9");
        assert_eq!(hb.outbox_mb, 1.5);
        assert!(hb.idle);
        assert!(!hb.os.is_empty());
        assert!((0.0..=100.0).contains(&hb.mem_pct));
    }
}
