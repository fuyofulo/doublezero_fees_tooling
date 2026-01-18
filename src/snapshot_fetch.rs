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
    out_dir: Option<PathBuf>,
    bucket: String,
}

pub fn run_from_args<I>(args: I) -> Result<()>
where
    I: Iterator<Item = String>,
{
    let args = parse_args(args)?;
    let client = Client::new();

    let epoch = args.epoch.expect("epoch required");

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
    let mut out_dir: Option<PathBuf> = None;
    let mut bucket = DEFAULT_SNAPSHOT_BUCKET.to_string();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--epoch" => {
                let value = args.next().context("missing value for --epoch")?;
                epoch = Some(value.parse().context("invalid epoch")?);
            }
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

    if epoch.is_none() {
        return Err(anyhow!("provide --epoch <n>"));
    }

    Ok(Args {
        epoch,
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

Defaults:
  --out-dir  out/epoch_<dz_epoch>/snapshot
  --bucket   https://doublezero-contributor-rewards-mn-beta-snapshots.s3.amazonaws.com
"#
    );
}
