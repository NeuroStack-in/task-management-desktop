//! Collects agent health folded into the batch envelope (never a separate call).
//! Skeleton returns placeholders; production reads real values via `sysinfo`.

use agent_shared::contract::Heartbeat;

pub fn collect() -> Heartbeat {
    Heartbeat {
        hostname: hostname(),
        os: std::env::consts::OS.to_string(),
        os_version: String::new(), // TODO(sysinfo): OS version
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        ip: String::new(), // TODO: primary interface ip
        cpu_pct: 0.0,      // TODO(sysinfo)
        mem_pct: 0.0,      // TODO(sysinfo)
    }
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
