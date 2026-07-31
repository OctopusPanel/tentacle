use bollard::models::ContainerStatsResponse;

#[derive(Debug, PartialEq)]
pub struct SystemMetrics {
    pub cpu_percent: f64,
    pub memory_mb: f64,
}

pub fn calculate_metrics(stats: &ContainerStatsResponse) -> SystemMetrics {
    let mut cpu_percent = 0.0;
    
    // CPU Delta
    let cpu_delta = stats.cpu_stats.as_ref()
        .and_then(|c| c.cpu_usage.as_ref())
        .and_then(|u| u.total_usage)
        .unwrap_or(0) as f64
        - stats.precpu_stats.as_ref()
        .and_then(|c| c.cpu_usage.as_ref())
        .and_then(|u| u.total_usage)
        .unwrap_or(0) as f64;
        
    // System CPU Delta
    let system_cpu_delta = stats.cpu_stats.as_ref()
        .and_then(|c| c.system_cpu_usage)
        .unwrap_or(0) as f64
        - stats.precpu_stats.as_ref()
        .and_then(|c| c.system_cpu_usage)
        .unwrap_or(0) as f64;

    // CPU Percent Berechnung (Docker Standard Formel)
    if system_cpu_delta > 0.0 && cpu_delta > 0.0 {
        let percpu_len = stats.cpu_stats.as_ref()
            .and_then(|c| c.cpu_usage.as_ref())
            .and_then(|u| u.percpu_usage.as_ref())
            .map(|v| v.len())
            .unwrap_or(1) as f64;
            
        cpu_percent = (cpu_delta / system_cpu_delta) * percpu_len * 100.0;
    }

    // Memory in Megabyte
    let memory_mb = stats.memory_stats.as_ref()
        .and_then(|m| m.usage)
        .unwrap_or(0) as f64 / (1024.0 * 1024.0);

    SystemMetrics {
        cpu_percent,
        memory_mb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_calculate_metrics_from_stats() {
        // Ein minimalistisches JSON das die Docker Stats API simuliert
        let raw_json = json!({
            "cpu_stats": {
                "cpu_usage": {
                    "total_usage": 50000000,
                    "percpu_usage": [10000000, 10000000, 10000000, 20000000] // 4 Cores
                },
                "system_cpu_usage": 150000000
            },
            "precpu_stats": {
                "cpu_usage": {
                    "total_usage": 10000000
                },
                "system_cpu_usage": 100000000
            },
            "memory_stats": {
                "usage": 104857600 // 100 MB in Bytes
            }
        });

        let stats: ContainerStatsResponse = serde_json::from_value(raw_json).unwrap();
        
        let metrics = calculate_metrics(&stats);

        // cpu_delta = 50M - 10M = 40M
        // system_delta = 150M - 100M = 50M
        // (40 / 50) * 4 Cores * 100 = 320%
        assert_eq!(metrics.cpu_percent, 320.0);
        
        // 104857600 Bytes = 100 MB
        assert_eq!(metrics.memory_mb, 100.0);
    }
}
