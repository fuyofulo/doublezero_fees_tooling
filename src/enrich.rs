use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug)]
struct Args {
    snapshot_path: PathBuf,
    onchain_dir: PathBuf,
    out_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub struct EnrichConfig {
    pub snapshot_path: PathBuf,
    pub onchain_dir: PathBuf,
    pub out_dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct EnrichedEpoch {
    dz_epoch: u64,
    epoch_window_us: EpochWindow,
    sources: Sources,
    onchain: OnchainBundle,
    totals: Totals,
    contributors: Vec<ContributorAggregate>,
}

#[derive(Debug, Serialize)]
struct EpochWindow {
    start_us: u64,
    end_us: u64,
}

#[derive(Debug, Serialize)]
struct Sources {
    snapshot_path: String,
    onchain_dir: String,
    distribution_file: String,
    program_config_file: Option<String>,
    journal_file: Option<String>,
}

#[derive(Debug, Serialize)]
struct OnchainBundle {
    distribution: Value,
    program_config: Option<Value>,
    journal: Option<Value>,
}

#[derive(Debug, Serialize)]
struct Totals {
    contributors: usize,
    devices: usize,
    links: usize,
    telemetry_links: usize,
    telemetry_entries: usize,
    telemetry_samples: usize,
}

#[derive(Debug, Serialize)]
struct ContributorAggregate {
    contributor_pk: String,
    code: String,
    status: String,
    devices: usize,
    links_total: usize,
    wan_links: usize,
    dzx_links: usize,
    other_links: usize,
    telemetry_links: usize,
    telemetry_entries: usize,
    telemetry_samples: usize,
    latency_p50_us: Option<f64>,
    latency_p95_us: Option<f64>,
    baseline_p50_us: Option<f64>,
    baseline_p95_us: Option<f64>,
    improvement_p50_pct: Option<f64>,
    improvement_p95_pct: Option<f64>,
    availability_pct: Option<f64>,
}

#[derive(Clone, Default)]
struct TelemetryAgg {
    telemetry_entries: usize,
    telemetry_samples: usize,
    sum_p50_weighted: f64,
    sum_p95_weighted: f64,
    baseline_samples: usize,
    baseline_sum_p50_weighted: f64,
    baseline_sum_p95_weighted: f64,
}

#[derive(Clone, Default)]
struct BaselineAgg {
    samples: usize,
    sum_p50_weighted: f64,
    sum_p95_weighted: f64,
}

#[derive(Clone, Debug)]
struct LinkInfo {
    contributor_pk: String,
    exchange_pair: Option<(String, String)>,
}

pub fn run_from_args<I>(args: I) -> Result<()>
where
    I: Iterator<Item = String>,
{
    let args = parse_args(args)?;
    run(EnrichConfig {
        snapshot_path: args.snapshot_path,
        onchain_dir: args.onchain_dir,
        out_dir: args.out_dir,
    })?;
    Ok(())
}

pub fn run(config: EnrichConfig) -> Result<PathBuf> {
    let distribution_path = find_distribution_file(&config.onchain_dir)?;
    let distribution = read_json(&distribution_path)?;
    let program_config =
        read_optional_json(&config.onchain_dir.join("program_config.json"))?;
    let journal = read_optional_json(&config.onchain_dir.join("journal.json"))?;

    let onchain_epoch = distribution
        .get("parsed")
        .and_then(|v| v.get("dz_epoch"))
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("distribution missing parsed.dz_epoch"))?;

    let snapshot_path = if config.snapshot_path.is_dir() {
        let filename = format!("mn-epoch-{onchain_epoch}-snapshot.json");
        config.snapshot_path.join(filename)
    } else {
        config.snapshot_path.clone()
    };

    let snapshot = read_json(&snapshot_path)?;
    let snapshot_epoch = snapshot
        .get("dz_epoch")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("snapshot missing dz_epoch"))?;

    if snapshot_epoch != onchain_epoch {
        return Err(anyhow!(
            "epoch mismatch: snapshot dz_epoch {} != onchain dz_epoch {}",
            snapshot_epoch,
            onchain_epoch
        ));
    }

    let fetch_data = snapshot
        .get("fetch_data")
        .ok_or_else(|| anyhow!("snapshot missing fetch_data"))?;
    let serviceability = fetch_data
        .get("dz_serviceability")
        .ok_or_else(|| anyhow!("snapshot missing dz_serviceability"))?;
    let telemetry = fetch_data
        .get("dz_telemetry")
        .ok_or_else(|| anyhow!("snapshot missing dz_telemetry"))?;
    let internet = fetch_data
        .get("dz_internet")
        .ok_or_else(|| anyhow!("snapshot missing dz_internet"))?;

    let start_us = fetch_data
        .get("start_us")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let end_us = fetch_data
        .get("end_us")
        .and_then(Value::as_u64)
        .unwrap_or_default();

    let contributors_obj = serviceability
        .get("contributors")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("snapshot missing contributors map"))?;
    let devices_obj = serviceability
        .get("devices")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("snapshot missing devices map"))?;
    let links_obj = serviceability
        .get("links")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("snapshot missing links map"))?;
    let telemetry_samples = telemetry
        .get("device_latency_samples")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("snapshot missing device_latency_samples"))?;
    let internet_samples = internet
        .get("internet_latency_samples")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("snapshot missing internet_latency_samples"))?;

    let mut contributor_meta: HashMap<String, (String, String)> = HashMap::new();
    for (pk, value) in contributors_obj {
        let code = value.get("code").and_then(Value::as_str).unwrap_or("").to_string();
        let status = value.get("status").and_then(Value::as_str).unwrap_or("").to_string();
        contributor_meta.insert(pk.clone(), (code, status));
    }

    let mut device_counts: HashMap<String, usize> = HashMap::new();
    for value in devices_obj.values() {
        let Some(contributor_pk) = value.get("contributor_pk").and_then(Value::as_str) else {
            continue;
        };
        *device_counts.entry(contributor_pk.to_string()).or_insert(0) += 1;
    }

    let mut device_to_exchange: HashMap<String, String> = HashMap::new();
    for (device_pk, value) in devices_obj {
        if let Some(exchange_pk) = value.get("exchange_pk").and_then(Value::as_str) {
            if !exchange_pk.is_empty() {
                device_to_exchange.insert(device_pk.clone(), exchange_pk.to_string());
            }
        }
    }

    let mut link_counts: HashMap<String, (usize, usize, usize, usize)> = HashMap::new();
    let mut link_info: HashMap<String, LinkInfo> = HashMap::new();
    for (link_pk, value) in links_obj {
        let Some(contributor_pk) = value.get("contributor_pk").and_then(Value::as_str) else {
            continue;
        };
        let link_type = value.get("link_type").and_then(Value::as_str).unwrap_or("");
        let entry = link_counts
            .entry(contributor_pk.to_string())
            .or_insert((0, 0, 0, 0));
        entry.0 += 1;
        match link_type {
            "WAN" => entry.1 += 1,
            "DZX" => entry.2 += 1,
            _ => entry.3 += 1,
        }

        let exchange_pair = {
            let side_a_pk = value.get("side_a_pk").and_then(Value::as_str);
            let side_z_pk = value.get("side_z_pk").and_then(Value::as_str);
            match (side_a_pk, side_z_pk) {
                (Some(a), Some(z)) => {
                    let ex_a = device_to_exchange.get(a);
                    let ex_z = device_to_exchange.get(z);
                    match (ex_a, ex_z) {
                        (Some(ex_a), Some(ex_z)) => Some(normalize_pair(ex_a, ex_z)),
                        _ => None,
                    }
                }
                _ => None,
            }
        };

        link_info.insert(
            link_pk.clone(),
            LinkInfo {
                contributor_pk: contributor_pk.to_string(),
                exchange_pair,
            },
        );
    }

    let mut baseline_by_pair: HashMap<(String, String), BaselineAgg> = HashMap::new();
    for sample in internet_samples {
        let Some(origin_exchange_pk) = sample.get("origin_exchange_pk").and_then(Value::as_str)
        else {
            continue;
        };
        let Some(target_exchange_pk) = sample.get("target_exchange_pk").and_then(Value::as_str)
        else {
            continue;
        };
        let Some(samples) = sample.get("samples").and_then(Value::as_array) else {
            continue;
        };

        let mut vals = Vec::with_capacity(samples.len());
        for value in samples {
            if let Some(v) = value.as_f64() {
                vals.push(v);
            } else if let Some(v) = value.as_u64() {
                vals.push(v as f64);
            }
        }
        if vals.is_empty() {
            continue;
        }

        let p50 = median(&vals);
        let p95 = percentile(&vals, 95.0);
        let count = vals.len();

        let pair = normalize_pair(origin_exchange_pk, target_exchange_pk);
        let entry = baseline_by_pair.entry(pair).or_default();
        entry.samples += count;
        entry.sum_p50_weighted += p50 * count as f64;
        entry.sum_p95_weighted += p95 * count as f64;
    }

    let mut telemetry_by_contributor: HashMap<String, TelemetryAgg> = HashMap::new();
    let mut telemetry_links_unique: HashMap<String, std::collections::HashSet<String>> =
        HashMap::new();
    let mut total_telemetry_entries = 0usize;
    let mut total_telemetry_samples = 0usize;

    for sample in telemetry_samples {
        let Some(link_pk) = sample.get("link_pk").and_then(Value::as_str) else {
            continue;
        };
        let Some(samples) = sample.get("samples").and_then(Value::as_array) else {
            continue;
        };
        let Some(link_info) = link_info.get(link_pk) else {
            continue;
        };

        let mut vals = Vec::with_capacity(samples.len());
        for value in samples {
            if let Some(v) = value.as_f64() {
                vals.push(v);
            } else if let Some(v) = value.as_u64() {
                vals.push(v as f64);
            }
        }
        if vals.is_empty() {
            continue;
        }

        let p50 = median(&vals);
        let p95 = percentile(&vals, 95.0);
        let count = vals.len();

        let agg = telemetry_by_contributor
            .entry(link_info.contributor_pk.to_string())
            .or_default();
        agg.telemetry_entries += 1;
        agg.telemetry_samples += count;
        agg.sum_p50_weighted += p50 * count as f64;
        agg.sum_p95_weighted += p95 * count as f64;

        total_telemetry_entries += 1;
        total_telemetry_samples += count;

        telemetry_links_unique
            .entry(link_info.contributor_pk.to_string())
            .or_default()
            .insert(link_pk.to_string());

        if let Some(pair) = link_info.exchange_pair.as_ref() {
            if let Some(baseline) = baseline_by_pair.get(pair) {
                if baseline.samples > 0 {
                    let baseline_p50 = baseline.sum_p50_weighted / baseline.samples as f64;
                    let baseline_p95 = baseline.sum_p95_weighted / baseline.samples as f64;
                    agg.baseline_samples += count;
                    agg.baseline_sum_p50_weighted += baseline_p50 * count as f64;
                    agg.baseline_sum_p95_weighted += baseline_p95 * count as f64;
                }
            }
        }
    }

    let mut contributors = Vec::with_capacity(contributor_meta.len());
    for (pk, (code, status)) in contributor_meta {
        let devices = device_counts.get(&pk).copied().unwrap_or(0);
        let (links_total, wan_links, dzx_links, other_links) =
            link_counts.get(&pk).copied().unwrap_or((0, 0, 0, 0));
        let telemetry = telemetry_by_contributor.get(&pk).cloned().unwrap_or_default();
        let unique_links = telemetry_links_unique
            .get(&pk)
            .map(|set| set.len())
            .unwrap_or(0);

        let latency_p50_us = if telemetry.telemetry_samples > 0 {
            Some(telemetry.sum_p50_weighted / telemetry.telemetry_samples as f64)
        } else {
            None
        };
        let latency_p95_us = if telemetry.telemetry_samples > 0 {
            Some(telemetry.sum_p95_weighted / telemetry.telemetry_samples as f64)
        } else {
            None
        };
        let baseline_p50_us = if telemetry.baseline_samples > 0 {
            Some(telemetry.baseline_sum_p50_weighted / telemetry.baseline_samples as f64)
        } else {
            None
        };
        let baseline_p95_us = if telemetry.baseline_samples > 0 {
            Some(telemetry.baseline_sum_p95_weighted / telemetry.baseline_samples as f64)
        } else {
            None
        };
        let improvement_p50_pct = match (baseline_p50_us, latency_p50_us) {
            (Some(baseline), Some(dz)) if baseline > 0.0 => {
                Some(((baseline - dz) / baseline) * 100.0)
            }
            _ => None,
        };
        let improvement_p95_pct = match (baseline_p95_us, latency_p95_us) {
            (Some(baseline), Some(dz)) if baseline > 0.0 => {
                Some(((baseline - dz) / baseline) * 100.0)
            }
            _ => None,
        };
        let availability_pct = if links_total > 0 {
            Some((unique_links as f64 / links_total as f64) * 100.0)
        } else {
            None
        };

        contributors.push(ContributorAggregate {
            contributor_pk: pk,
            code,
            status,
            devices,
            links_total,
            wan_links,
            dzx_links,
            other_links,
            telemetry_links: unique_links,
            telemetry_entries: telemetry.telemetry_entries,
            telemetry_samples: telemetry.telemetry_samples,
            latency_p50_us,
            latency_p95_us,
            baseline_p50_us,
            baseline_p95_us,
            improvement_p50_pct,
            improvement_p95_pct,
            availability_pct,
        });
    }

    contributors.sort_by(|a, b| a.code.cmp(&b.code));
    let total_telemetry_links: usize =
        telemetry_links_unique.values().map(|set| set.len()).sum();

    let out_dir = config.out_dir.clone().unwrap_or_else(|| {
        config
            .onchain_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| config.onchain_dir.clone())
    });
    fs::create_dir_all(&out_dir).context("create output directory")?;

    let enriched = EnrichedEpoch {
        dz_epoch: snapshot_epoch,
        epoch_window_us: EpochWindow { start_us, end_us },
        sources: Sources {
            snapshot_path: snapshot_path.display().to_string(),
            onchain_dir: config.onchain_dir.display().to_string(),
            distribution_file: distribution_path.display().to_string(),
            program_config_file: program_config
                .as_ref()
                .map(|_| {
                    config
                        .onchain_dir
                        .join("program_config.json")
                        .display()
                        .to_string()
                }),
            journal_file: journal
                .as_ref()
                .map(|_| {
                    config
                        .onchain_dir
                        .join("journal.json")
                        .display()
                        .to_string()
                }),
        },
        onchain: OnchainBundle {
            distribution: distribution
                .get("parsed")
                .cloned()
                .unwrap_or(Value::Null),
            program_config: program_config
                .as_ref()
                .and_then(|value| value.get("parsed"))
                .cloned(),
            journal: journal.as_ref().and_then(|value| value.get("parsed")).cloned(),
        },
        totals: Totals {
            contributors: contributors.len(),
            devices: devices_obj.len(),
            links: links_obj.len(),
            telemetry_links: total_telemetry_links,
            telemetry_entries: total_telemetry_entries,
            telemetry_samples: total_telemetry_samples,
        },
        contributors,
    };

    write_json(&out_dir, "enriched_epoch.json", &enriched)?;
    Ok(out_dir.join("enriched_epoch.json"))
}

fn parse_args<I>(mut args: I) -> Result<Args>
where
    I: Iterator<Item = String>,
{
    let mut snapshot_path: Option<PathBuf> = None;
    let mut onchain_dir: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--snapshot" => {
                let value = args.next().context("missing value for --snapshot")?;
                snapshot_path = Some(PathBuf::from(value));
            }
            "--onchain-dir" => {
                let value = args.next().context("missing value for --onchain-dir")?;
                onchain_dir = Some(PathBuf::from(value));
            }
            "--out-dir" => {
                let value = args.next().context("missing value for --out-dir")?;
                out_dir = Some(PathBuf::from(value));
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            _ => return Err(anyhow!("unexpected argument: {arg}")),
        }
    }

    Ok(Args {
        snapshot_path: snapshot_path.context("missing --snapshot")?,
        onchain_dir: onchain_dir.context("missing --onchain-dir")?,
        out_dir,
    })
}

fn read_json(path: &Path) -> Result<Value> {
    let data = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&data).context("parse json")
}

fn read_optional_json(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }
    read_json(path).map(Some)
}

fn find_distribution_file(dir: &Path) -> Result<PathBuf> {
    let direct = dir.join("distribution.json");
    if direct.exists() {
        return Ok(direct);
    }
    let mut matches = Vec::new();
    for entry in fs::read_dir(dir).context("read onchain dir")? {
        let entry = entry.context("read entry")?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("distribution_") && name.ends_with(".json") {
                matches.push(path);
            }
        }
    }
    match matches.len() {
        0 => Err(anyhow!("no distribution_*.json found in {}", dir.display())),
        1 => Ok(matches.remove(0)),
        _ => Err(anyhow!(
            "multiple distribution_*.json files found in {}",
            dir.display()
        )),
    }
}

fn write_json<T: Serialize>(out_dir: &Path, name: &str, value: &T) -> Result<()> {
    let path = out_dir.join(name);
    let data = serde_json::to_vec_pretty(value).context("serialize json")?;
    fs::write(&path, data).with_context(|| format!("write {name}"))?;
    Ok(())
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = (p / 100.0) * (sorted.len() - 1) as f64;
    let lower = index.floor() as usize;
    let upper = index.ceil() as usize;
    let weight = index - lower as f64;
    sorted[lower] * (1.0 - weight) + sorted[upper] * weight
}

fn normalize_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

pub fn print_usage() {
    eprintln!(
        r#"Usage:
  fees-tooling enrich --snapshot <path> --onchain-dir <path> [--out-dir <path>]

Notes:
  --snapshot may be a file or a directory containing mn-epoch-<n>-snapshot.json

Example:
  fees-tooling enrich \
    --snapshot out/epoch_77/snapshot \
    --onchain-dir out/epoch_77/onchain
"#
    );
}
