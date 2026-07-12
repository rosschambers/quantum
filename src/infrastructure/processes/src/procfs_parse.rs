//! Pure parsers for the Linux `/proc` filesystem entries used to compute
//! system statistics. These take raw file contents as input so they can be
//! unit-tested without touching the real filesystem.

pub fn parse_proc_stat_cpu(input: &str) -> Option<(u64, u64)> {
    let line = input.lines().find(|l| l.starts_with("cpu "))?;
    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() < 8 {
        return None;
    }
    let user = parts[0];
    let nice = parts[1];
    let system = parts[2];
    let idle = parts[3];
    let iowait = parts[4];
    let irq = parts[5];
    let softirq = parts[6];
    let steal = parts[7];
    let busy = user + nice + system + irq + softirq + steal;
    let total = busy + idle + iowait;
    Some((busy, total))
}

pub fn parse_proc_meminfo(input: &str) -> Option<(u64, u64)> {
    let mut total_kb = None;
    let mut available_kb = None;
    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kb = rest.split_whitespace().next().and_then(|s| s.parse().ok());
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available_kb = rest.split_whitespace().next().and_then(|s| s.parse().ok());
        }
        if total_kb.is_some() && available_kb.is_some() {
            break;
        }
    }
    let t: u64 = total_kb?;
    let a: u64 = available_kb?;
    let used = t.saturating_sub(a);
    Some((used * 1024, t * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proc_stat_cpu_typical() {
        let input = "cpu  100 50 200 1000 10 5 8 0 0 0\n";
        let (busy, total) = parse_proc_stat_cpu(input).expect("parse failed");
        // busy = 100 + 50 + 200 + 5 + 8 + 0 = 363
        // total = 363 + 1000 + 10 = 1373
        assert_eq!(busy, 363);
        assert_eq!(total, 1373);
    }

    #[test]
    fn parse_proc_stat_cpu_invalid() {
        let empty = "";
        assert!(parse_proc_stat_cpu(empty).is_none());

        let malformed = "cpu0 100 50 200 1000 10 5 8 0 0 0\n";
        assert!(parse_proc_stat_cpu(malformed).is_none());

        let insufficient = "cpu 100\n";
        assert!(parse_proc_stat_cpu(insufficient).is_none());
    }

    #[test]
    fn parse_proc_meminfo_typical() {
        let input = "MemTotal:        8192 kB\nMemAvailable:    4096 kB\n";
        let (used, total) = parse_proc_meminfo(input).expect("parse failed");
        // available = 4096, total = 8192, used = 8192 - 4096 = 4096
        // multiply by 1024: used = 4194304, total = 8388608
        assert_eq!(used, 4194304);
        assert_eq!(total, 8388608);
    }

    #[test]
    fn parse_proc_meminfo_missing_fields() {
        let no_available = "MemTotal:        8192 kB\n";
        assert!(parse_proc_meminfo(no_available).is_none());

        let no_total = "MemAvailable:    4096 kB\n";
        assert!(parse_proc_meminfo(no_total).is_none());

        let empty = "";
        assert!(parse_proc_meminfo(empty).is_none());
    }
}
