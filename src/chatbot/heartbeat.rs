use std::time::Instant;

use once_cell::sync::Lazy;
use tokio::sync::RwLock;

use super::types::StreamVariant;

pub static SYSINFO: Lazy<RwLock<(sysinfo::System, Instant)>> =
    Lazy::new(|| RwLock::new(((sysinfo::System::new_all()), Instant::now())));

/// Returns a StreamVariant::ServerHint that contains some information about the server.
/// Is intended to be sent as a heartbeat to the client.
pub async fn heartbeat_content() -> StreamVariant {
    let mut heartbeat_json = serde_json::Map::new();

    maybe_update(); // Update the system information to get the most recent data.

    // Insert different info into the map.
    let sys = &SYSINFO.read().await.0;

    let pid = std::process::id();

    // Add memory information.
    let memory = sys.used_memory();
    heartbeat_json.insert(
        "memory".to_string(),
        serde_json::Value::Number(serde_json::Number::from(memory)),
    );
    let total_memory = sys.total_memory();
    heartbeat_json.insert(
        "total_memory".to_string(),
        serde_json::Value::Number(serde_json::Number::from(total_memory)),
    );

    // Add CPU information.
    let cpu_usage = sys.global_cpu_usage();
    if let Some(cpu_usage) = serde_json::Number::from_f64(cpu_usage as f64) {
        heartbeat_json.insert(
            "cpu_usage".to_string(),
            serde_json::Value::Number(cpu_usage),
        );
    };

    let cpu_last_minute = sysinfo::System::load_average().one;
    if let Some(cpu_last_minute) = serde_json::Number::from_f64(cpu_last_minute) {
        heartbeat_json.insert(
            "cpu_last_minute".to_string(),
            serde_json::Value::Number(cpu_last_minute),
        );
    };

    // Create a list of all the processes we are interested in.
    // These are this process and all its children.
    let mut process_list = vec![pid];
    let mut found_some = true;
    while found_some {
        found_some = false;
        for (pid, process) in sys.processes() {
            // If this process is a child of some process in the list, we'll add it to the list.
            if let Some(parent) = process.parent() {
                if process_list.contains(&parent.as_u32()) && !process_list.contains(&pid.as_u32())
                {
                    process_list.push(pid.as_u32());
                    found_some = true;
                }
            }
        }
    }

    // Add up the CPU and memory usage of all the processes we are interested in.

    let mut process_cpu = 0.0;
    let mut process_memory = 0;
    for (pid, process) in sys.processes() {
        if process_list.contains(&pid.as_u32()) {
            process_cpu += process.cpu_usage();
            process_memory += process.memory();
        }
    }

    // The conversion from f64 might fail, so we'll check if it's possible.
    if let Some(process_cpu) = serde_json::Number::from_f64(process_cpu as f64) {
        heartbeat_json.insert(
            "process_cpu".to_string(),
            serde_json::Value::Number(process_cpu),
        );
    };
    heartbeat_json.insert(
        "process_memory".to_string(),
        serde_json::Value::Number(serde_json::Number::from(process_memory)),
    );

    let heartbeat_string = serde_json::Value::Object(heartbeat_json).to_string();

    StreamVariant::ServerHint(heartbeat_string)
}

/// Maybe update the system information, but only if it's more than 4 seconds since the last update.
fn maybe_update() {
    if let Ok(mut lock) = SYSINFO.try_write() {
        if lock.1.elapsed().as_secs() > 4 {
            *lock = (sysinfo::System::new_all(), Instant::now());
        }
    }
}
