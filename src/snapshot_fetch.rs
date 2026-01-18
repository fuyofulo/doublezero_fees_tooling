use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;

pub const DEFAULT_SNAPSHOT_BUCKET: &str =
    "https://doublezero-contributor-rewards-mn-beta-snapshots.s3.amazonaws.com";
const DEFAULT_SNAPSHOT_RETRIES: usize = 3;
const DEFAULT_SNAPSHOT_TIMEOUT_SECS: u64 = 120;

#[derive(Debug)]
struct Args {
    epoch: Option<u64>,
    latest: bool,
    out_dir: Option<PathBuf>,
    bucket: String,
}

pub fn run_from_args<I>(args: I) -> Result<()>
where
    I: Iterator<Item = String>,
{
    let args = parse_args(args)?;
    let client = Client::new();

    let epoch = if args.latest {
        detect_latest_snapshot_epoch(&client, &args.bucket)?
            .context("no snapshot epochs found")?
    } else {
        args.epoch.expect("epoch required")
    };

    let out_dir = args
        .out_dir
        .unwrap_or_else(|| PathBuf::from(format!("out/epoch_{epoch}/snapshot")));

    let out_path = fetch_snapshot(&client, epoch, &out_dir, &args.bucket)?;
    eprintln!("Saved snapshot to {}", out_path.display());
    Ok(())
}

pub fn fetch_snapshot(
    client: &Client,
    epoch: u64,
    out_dir: &Path,
    bucket: &str,
) -> Result<PathBuf> {
    let filename = format!("mn-epoch-{epoch}-snapshot.json");
    let url = format!("{}/{}", bucket.trim_end_matches('/'), filename);
    let out_path = out_dir.join(&filename);
    if let Ok(metadata) = fs::metadata(&out_path) {
        if metadata.len() > 0 {
            return Ok(out_path);
        }
    }

    let bytes = fetch_bytes_with_retry(
        client,
        &url,
        DEFAULT_SNAPSHOT_RETRIES,
        Duration::from_secs(DEFAULT_SNAPSHOT_TIMEOUT_SECS),
    )
    .with_context(|| format!("fetch snapshot from {url}"))?;
    write_file(&out_path, &bytes)?;
    Ok(out_path)
}

fn parse_args<I>(mut args: I) -> Result<Args>
where
    I: Iterator<Item = String>,
{
    let mut epoch: Option<u64> = None;
    let mut latest = false;
    let mut out_dir: Option<PathBuf> = None;
    let mut bucket = DEFAULT_SNAPSHOT_BUCKET.to_string();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--epoch" => {
                let value = args.next().context("missing value for --epoch")?;
                epoch = Some(value.parse().context("invalid epoch")?);
            }
            "--latest" => latest = true,
            "--out-dir" => {
                let value = args.next().context("missing value for --out-dir")?;
                out_dir = Some(PathBuf::from(value));
            }
            "--bucket" => {
                let value = args.next().context("missing value for --bucket")?;
                bucket = value;
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            _ => return Err(anyhow!("unexpected argument: {arg}")),
        }
    }

    if epoch.is_none() && !latest {
        return Err(anyhow!("provide --epoch <n> or --latest"));
    }
    if epoch.is_some() && latest {
        return Err(anyhow!("cannot combine --epoch and --latest"));
    }

    Ok(Args {
        epoch,
        latest,
        out_dir,
        bucket,
    })
}

fn fetch_bytes_with_retry(
    client: &Client,
    url: &str,
    retries: usize,
    timeout: Duration,
) -> Result<Vec<u8>> {
    let mut last_err = None;
    for attempt in 1..=retries {
        let response = client
            .get(url)
            .timeout(timeout)
            .send()
            .context("http request failed")
            .and_then(|resp| resp.error_for_status().context("http error status"))
            .and_then(|resp| resp.bytes().context("read response body").map(|b| b.to_vec()));

        match response {
            Ok(bytes) => return Ok(bytes),
            Err(err) => {
                last_err = Some(err);
                if attempt < retries {
                    let delay = Duration::from_secs(2 * attempt as u64);
                    thread::sleep(delay);
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("request failed")))
}

fn detect_latest_snapshot_epoch(client: &Client, bucket: &str) -> Result<Option<u64>> {
    let xml = list_bucket_xml(client, bucket)?;
    let mut epochs = Vec::new();
    for key in parse_keys(&xml) {
        if let Some(epoch) = parse_snapshot_epoch(&key) {
            epochs.push(epoch);
        }
    }
    epochs.sort_unstable();
    Ok(epochs.pop())
}

fn list_bucket_xml(client: &Client, bucket: &str) -> Result<String> {
    let url = format!("{}/?list-type=2", bucket.trim_end_matches('/'));
    let response = client
        .get(url)
        .send()
        .context("http request failed")?
        .error_for_status()
        .context("http error status")?;
    response.text().context("read response body")
}

fn parse_snapshot_epoch(key: &str) -> Option<u64> {
    let prefix = "mn-epoch-";
    let suffix = "-snapshot.json";
    if !key.starts_with(prefix) || !key.ends_with(suffix) {
        return None;
    }
    let middle = &key[prefix.len()..key.len() - suffix.len()];
    middle.parse().ok()
}

fn parse_keys(xml: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = xml;
    let open = "<Key>";
    let close = "</Key>";

    while let Some(start) = rest.find(open) {
        let after = &rest[start + open.len()..];
        if let Some(end) = after.find(close) {
            let key = &after[..end];
            keys.push(key.to_string());
            rest = &after[end + close.len()..];
        } else {
            break;
        }
    }

    keys
}

fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create output directory")?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

pub fn print_usage() {
    eprintln!(
        r#"Usage:
  fees-tooling snapshot --epoch <n> [--out-dir <path>] [--bucket <url>]
  fees-tooling snapshot --latest [--out-dir <path>] [--bucket <url>]

Defaults:
  --out-dir  out/epoch_<dz_epoch>/snapshot
  --bucket   https://doublezero-contributor-rewards-mn-beta-snapshots.s3.amazonaws.com
"#
    );
}
