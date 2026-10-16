//! Developer tasks.
//!
//!   cargo xtask ci            fmt check, clippy -D warnings, tests, check-layers, check-ages
//!   cargo xtask check-layers  pure crates never depend on UI/GPU crates
//!   cargo xtask check-ages    every locked crate was published at least 14 days ago
//!   cargo xtask shots         run every scenario script headlessly (scripts/shots.sh)

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    let task = std::env::args().nth(1).unwrap_or_default();
    let root = workspace_root();
    std::env::set_current_dir(&root)?;
    match task.as_str() {
        "ci" => ci(),
        "check-layers" => check_layers(),
        "check-ages" => check_ages(),
        "shots" => run("bash", &["scripts/shots.sh"]),
        _ => {
            eprintln!("usage: cargo xtask <ci|check-layers|check-ages|shots>");
            std::process::exit(2);
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    eprintln!("$ {cmd} {}", args.join(" "));
    let status = Command::new(cmd).args(args).status().with_context(|| format!("running {cmd}"))?;
    if !status.success() {
        bail!("`{cmd} {}` failed", args.join(" "));
    }
    Ok(())
}

fn ci() -> Result<()> {
    run("cargo", &["fmt", "--all", "--check"])?;
    run("cargo", &["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"])?;
    run("cargo", &["test", "--workspace", "--locked"])?;
    check_layers()?;
    if std::env::var_os("SKIP_CHECK_AGES").is_none() {
        check_ages()?;
    }
    eprintln!("ci: all green");
    Ok(())
}

/// Crates that must stay free of UI and GPU code.
const PURE: &[&str] = &["ss_math", "ss_mesh", "ss_csg", "ss_doc", "ss_ops", "ss_guide", "ss_io", "ss_export"];
const UI_GPU: &[&str] = &["egui", "eframe", "winit", "glow", "rfd", "egui_glow", "egui-winit", "glutin"];

fn check_layers() -> Result<()> {
    let out = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .context("cargo metadata")?;
    if !out.status.success() {
        bail!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for p in meta["packages"].as_array().into_iter().flatten() {
        names
            .insert(p["id"].as_str().unwrap_or_default().to_owned(), p["name"].as_str().unwrap_or_default().to_owned());
    }
    // Normal (non-dev) dependency edges from the resolve graph.
    let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in meta["resolve"]["nodes"].as_array().into_iter().flatten() {
        let id = node["id"].as_str().unwrap_or_default().to_owned();
        let mut list = Vec::new();
        for d in node["deps"].as_array().into_iter().flatten() {
            let normal = d["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null());
            if normal {
                list.push(d["pkg"].as_str().unwrap_or_default().to_owned());
            }
        }
        deps.insert(id, list);
    }
    let by_name = |n: &str| names.iter().find(|(_, v)| v.as_str() == n).map(|(k, _)| k.clone());
    let mut problems = Vec::new();
    for krate in PURE {
        let Some(start) = by_name(krate) else { continue };
        let mut seen = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let name = &names[&id];
            if UI_GPU.contains(&name.as_str()) {
                problems.push(format!("{krate} depends on {name}"));
            }
            if name == "manifold-csg" && *krate != "ss_csg" && !seen.iter().any(|s| names[s] == "ss_csg") {
                problems.push(format!("{krate} links manifold-csg directly"));
            }
            stack.extend(deps.get(&id).cloned().unwrap_or_default());
        }
    }
    for (crate_name, only_in) in [("rfd", "shapeshop"), ("transform-gizmo-egui", "shapeshop")] {
        for (id, list) in &deps {
            let owner = &names[id];
            if owner != only_in && list.iter().any(|d| names.get(d).map(String::as_str) == Some(crate_name)) {
                problems.push(format!("{owner} uses {crate_name} (only {only_in} may)"));
            }
        }
    }
    if problems.is_empty() {
        eprintln!("check-layers: ok");
        Ok(())
    } else {
        bail!("layering problems:\n  {}", problems.join("\n  "))
    }
}

fn check_ages() -> Result<()> {
    let lock = std::fs::read_to_string("Cargo.lock")?;
    let mut pkgs = Vec::new();
    let (mut name, mut version) = (String::new(), String::new());
    for line in lock.lines() {
        if let Some(v) = line.strip_prefix("name = ") {
            name = v.trim_matches('"').to_owned();
        } else if let Some(v) = line.strip_prefix("version = ") {
            version = v.trim_matches('"').to_owned();
        } else if line.starts_with("source = \"registry+") {
            pkgs.push((name.clone(), version.clone()));
        }
    }
    let cache_path = Path::new("target/xtask-crate-dates.json");
    let mut cache: BTreeMap<String, String> =
        std::fs::read(cache_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let today = today_days()?;
    let mut too_new = Vec::new();
    for (n, v) in &pkgs {
        let key = format!("{n}@{v}");
        let date = match cache.get(&key) {
            Some(d) => d.clone(),
            None => {
                let d = fetch_publish_date(n, v)?;
                cache.insert(key.clone(), d.clone());
                d
            }
        };
        let days = days_from_ymd(&date)?;
        if today - days < 14 {
            too_new.push(format!("{key} (published {date})"));
        }
    }
    let _ = std::fs::create_dir_all("target");
    std::fs::write(cache_path, serde_json::to_vec_pretty(&cache)?)?;
    if too_new.is_empty() {
        eprintln!("check-ages: all {} crates are at least 14 days old", pkgs.len());
        Ok(())
    } else {
        bail!("crates published less than 14 days ago:\n  {}", too_new.join("\n  "))
    }
}

fn fetch_publish_date(name: &str, version: &str) -> Result<String> {
    let url = format!("https://crates.io/api/v1/crates/{name}/{version}");
    let out = Command::new("curl")
        .args(["-sS", "--fail", "-A", "shapeshop-xtask (github.com/Thor-Sharky/3DModeling)", &url])
        .output()
        .context("curl")?;
    if !out.status.success() {
        bail!("could not fetch {url}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let created = v["version"]["created_at"].as_str().context("no created_at")?;
    Ok(created[..10].to_owned())
}

fn today_days() -> Result<i64> {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    Ok((secs / 86_400) as i64)
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` date.
fn days_from_ymd(s: &str) -> Result<i64> {
    let mut it = s.split('-').map(|p| p.parse::<i64>());
    let (y, m, d) = match (it.next(), it.next(), it.next()) {
        (Some(Ok(y)), Some(Ok(m)), Some(Ok(d))) => (y, m, d),
        _ => bail!("bad date {s}"),
    };
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Ok(era * 146_097 + doe - 719_468)
}
