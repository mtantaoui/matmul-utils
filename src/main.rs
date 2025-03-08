use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::process::Command;

// Define structs to store cache information
#[derive(Debug, Default, Clone)]
struct CacheInfo {
    instruction_size: usize,
    data_size: usize,
    unified_size: usize,
}

impl CacheInfo {
    fn format(&self) -> Vec<String> {
        let mut result = Vec::new();

        if self.unified_size > 0 {
            result.push(format!(
                "L1 Cache (Unified): {}",
                format_size(self.unified_size)
            ));
        } else {
            if self.instruction_size > 0 {
                result.push(format!(
                    "L1 Instruction Cache: {}",
                    format_size(self.instruction_size)
                ));
            }

            if self.data_size > 0 {
                result.push(format!("L1 Data Cache: {}", format_size(self.data_size)));
            }
        }

        result
    }
}

#[derive(Debug, Default, Clone)]
struct ProcessorLevel {
    level_name: String,
    l1_cache: CacheInfo,
    l2_cache: usize,
    l3_cache: usize,
}

impl ProcessorLevel {
    fn new(name: &str) -> Self {
        Self {
            level_name: name.to_string(),
            ..Default::default()
        }
    }

    fn format(&self) -> Vec<String> {
        let mut result = Vec::new();

        result.push(format!("\n{}", self.level_name));
        result.push(format!("{}", "-".repeat(self.level_name.len())));

        // Add L1 cache info
        result.extend(self.l1_cache.format());

        // Add L2 and L3 cache info
        result.push(format!("L2 Cache: {}", format_size(self.l2_cache)));

        if self.l3_cache > 0 {
            result.push(format!("L3 Cache: {}", format_size(self.l3_cache)));
        }

        result
    }
}

#[derive(Debug, Default)]
struct ProcessorInfo {
    architecture: String,
    model_name: String,
    performance_levels: HashMap<String, ProcessorLevel>,
}

impl ProcessorInfo {
    fn new() -> Self {
        Self {
            architecture: env::consts::ARCH.to_string(),
            ..Default::default()
        }
    }

    fn detect_architecture(&mut self) -> &mut Self {
        self.architecture = match self.architecture.as_str() {
            "x86" | "x86_64" => "x86".to_string(),
            "aarch64" | "arm" | "arm64" => self.detect_arm_type(),
            _ => format!("Unknown: {}", self.architecture),
        };

        self.detect_model_name();
        self
    }

    fn detect_arm_type(&mut self) -> String {
        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = Command::new("sysctl")
                .args(&["-n", "machdep.cpu.brand_string"])
                .output()
            {
                let cpu_info = String::from_utf8_lossy(&output.stdout);
                if cpu_info.contains("Apple") {
                    return "Apple Silicon".to_string();
                }
            }
        }

        "ARM".to_string()
    }

    fn detect_model_name(&mut self) {
        #[cfg(target_os = "linux")]
        {
            if let Ok(mut file) = File::open("/proc/cpuinfo") {
                let mut contents = String::new();
                if file.read_to_string(&mut contents).is_ok() {
                    for line in contents.lines() {
                        if line.starts_with("model name") {
                            if let Some(model) = line.split(':').nth(1) {
                                self.model_name = model.trim().to_string();
                                break;
                            }
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = Command::new("sysctl")
                .args(&["-n", "machdep.cpu.brand_string"])
                .output()
            {
                self.model_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            }
        }

        #[cfg(windows)]
        {
            if let Ok(output) = Command::new("wmic")
                .args(&["cpu", "get", "name", "/value"])
                .output()
            {
                let output_str = String::from_utf8_lossy(&output.stdout);
                for line in output_str.lines() {
                    if line.starts_with("Name=") {
                        self.model_name = line.trim_start_matches("Name=").trim().to_string();
                        break;
                    }
                }
            }
        }
    }

    fn collect_cache_info(&mut self) -> io::Result<&mut Self> {
        match env::consts::OS {
            "macos" => {
                #[cfg(target_os = "macos")]
                self.collect_macos_cache_info()?;
            }
            "linux" => {
                #[cfg(target_os = "linux")]
                self.collect_linux_cache_info()?;
            }
            "windows" => {
                #[cfg(windows)]
                self.collect_windows_cache_info()?;
            }
            _ => {
                eprintln!("Unsupported operating system: {}", env::consts::OS);
            }
        }

        Ok(self)
    }

    #[cfg(target_os = "macos")]
    fn collect_macos_cache_info(&mut self) -> io::Result<()> {
        if self.architecture == "Apple Silicon" {
            self.collect_apple_silicon_cache_info()
        } else {
            self.collect_intel_mac_cache_info()
        }
    }

    #[cfg(target_os = "macos")]
    fn collect_apple_silicon_cache_info(&mut self) -> io::Result<()> {
        // Get number of performance levels
        let perf_levels = run_sysctl("hw.nperflevels")?.parse::<usize>().unwrap_or(1);

        // For each performance level
        for level in 0..perf_levels {
            let level_name = if level == 0 {
                "Performance Cores".to_string()
            } else {
                format!("Efficiency Cores (Level {})", level)
            };

            let mut proc_level = ProcessorLevel::new(&level_name);

            // L1 instruction cache
            proc_level.l1_cache.instruction_size =
                run_sysctl(&format!("hw.perflevel{}.l1icachesize", level))?
                    .parse::<usize>()
                    .unwrap_or(0);

            // L1 data cache
            proc_level.l1_cache.data_size =
                run_sysctl(&format!("hw.perflevel{}.l1dcachesize", level))?
                    .parse::<usize>()
                    .unwrap_or(0);

            // L2 cache
            proc_level.l2_cache = run_sysctl(&format!("hw.perflevel{}.l2cachesize", level))?
                .parse::<usize>()
                .unwrap_or(0);

            // L3 cache (shared across all cores usually)
            if level == 0 {
                proc_level.l3_cache = run_sysctl("hw.l3cachesize")?.parse::<usize>().unwrap_or(0);
            }

            self.performance_levels.insert(level_name, proc_level);
        }

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn collect_intel_mac_cache_info(&mut self) -> io::Result<()> {
        let mut proc_level = ProcessorLevel::new("Default");

        // Try unified L1 cache first
        match run_sysctl("hw.l1cachesize") {
            Ok(value) if !value.is_empty() => {
                proc_level.l1_cache.unified_size = value.parse::<usize>().unwrap_or(0);
            }
            _ => {
                // Try separate instruction and data caches
                if let Ok(value) = run_sysctl("hw.l1icachesize") {
                    proc_level.l1_cache.instruction_size = value.parse::<usize>().unwrap_or(0);
                }

                if let Ok(value) = run_sysctl("hw.l1dcachesize") {
                    proc_level.l1_cache.data_size = value.parse::<usize>().unwrap_or(0);
                }
            }
        }

        // L2 cache
        if let Ok(value) = run_sysctl("hw.l2cachesize") {
            proc_level.l2_cache = value.parse::<usize>().unwrap_or(0);
        }

        // L3 cache
        if let Ok(value) = run_sysctl("hw.l3cachesize") {
            proc_level.l3_cache = value.parse::<usize>().unwrap_or(0);
        }

        self.performance_levels
            .insert("Default".to_string(), proc_level);

        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn collect_linux_cache_info(&mut self) -> io::Result<()> {
        let mut proc_level = ProcessorLevel::new("Default");

        // Read cache information from sysfs
        for i in 0..10 {
            let cache_dir = format!("/sys/devices/system/cpu/cpu0/cache/index{}", i);

            // Check if this cache index exists
            let level = match read_file(&format!("{}/level", cache_dir)) {
                Ok(content) => content.trim().parse::<usize>().unwrap_or(0),
                Err(_) => continue,
            };

            // Get cache type
            let cache_type = match read_file(&format!("{}/type", cache_dir)) {
                Ok(content) => content.trim().to_string(),
                Err(_) => continue,
            };

            // Get cache size
            let size_str = match read_file(&format!("{}/size", cache_dir)) {
                Ok(content) => content.trim().to_string(),
                Err(_) => continue,
            };

            // Parse the size (e.g., "32K" or "4M")
            let size = parse_size_with_unit(&size_str);

            // Store the cache size based on its level and type
            match level {
                1 => match cache_type.as_str() {
                    "Data" => proc_level.l1_cache.data_size = size,
                    "Instruction" => proc_level.l1_cache.instruction_size = size,
                    "Unified" => proc_level.l1_cache.unified_size = size,
                    _ => {}
                },
                2 => proc_level.l2_cache = size,
                3 => proc_level.l3_cache = size,
                _ => {} // Ignore other levels
            }
        }

        self.performance_levels
            .insert("Default".to_string(), proc_level);

        Ok(())
    }

    #[cfg(windows)]
    fn collect_windows_cache_info(&mut self) -> io::Result<()> {
        let mut proc_level = ProcessorLevel::new("Default");

        // Use wmic to get cache information on Windows
        if let Ok(output) = Command::new("wmic")
            .args(&[
                "cpu",
                "get",
                "L1CacheSize,L2CacheSize,L3CacheSize",
                "/value",
            ])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);

            // Parse the output to extract cache sizes
            for line in output_str.lines() {
                if line.starts_with("L1CacheSize=") {
                    if let Ok(size) = line
                        .trim_start_matches("L1CacheSize=")
                        .trim()
                        .parse::<usize>()
                    {
                        proc_level.l1_cache.unified_size = size * 1024; // Windows reports in KB
                    }
                } else if line.starts_with("L2CacheSize=") {
                    if let Ok(size) = line
                        .trim_start_matches("L2CacheSize=")
                        .trim()
                        .parse::<usize>()
                    {
                        proc_level.l2_cache = size * 1024; // Windows reports in KB
                    }
                } else if line.starts_with("L3CacheSize=") {
                    if let Ok(size) = line
                        .trim_start_matches("L3CacheSize=")
                        .trim()
                        .parse::<usize>()
                    {
                        proc_level.l3_cache = size * 1024; // Windows reports in KB
                    }
                }
            }
        }

        self.performance_levels
            .insert("Default".to_string(), proc_level);

        Ok(())
    }

    fn display(&mut self) -> String {
        let mut result = Vec::new();

        result.push(format!(
            "Architecture: {} - {}",
            self.architecture,
            env::consts::ARCH.to_string()
        ));

        if !self.model_name.is_empty() {
            result.push(format!("CPU Model: {}", self.model_name));
        }

        result.push("\nCache Information:".to_string());
        result.push("==================".to_string());

        for (_, level) in &self.performance_levels {
            result.extend(level.format());
        }

        result.join("\n")
    }
}

// Helper functions

#[cfg(target_os = "macos")]
fn run_sysctl(parameter: &str) -> io::Result<String> {
    let output = Command::new("sysctl").args(&["-n", parameter]).output()?;

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn read_file(path: &str) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

fn parse_size_with_unit(size_str: &str) -> usize {
    let numeric_part: String = size_str.chars().take_while(|c| c.is_digit(10)).collect();

    let base_size = numeric_part.parse::<usize>().unwrap_or(0);

    // Convert to bytes based on the unit
    if size_str.ends_with('K') {
        base_size * 1024
    } else if size_str.ends_with('M') {
        base_size * 1024 * 1024
    } else if size_str.ends_with('G') {
        base_size * 1024 * 1024 * 1024
    } else {
        base_size
    }
}

fn format_size(size: usize) -> String {
    if size == 0 {
        return "Not detected".to_string();
    }

    if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.2} KB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.2} MB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn main() -> io::Result<()> {
    let mut processor = ProcessorInfo::new();
    let processor_info = processor.detect_architecture().collect_cache_info()?;

    println!("{}", processor_info.display());

    Ok(())
}
