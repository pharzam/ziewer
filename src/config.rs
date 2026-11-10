//! Configuration: load from TOML, apply env overrides, validate, save atomically.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Folder {
    pub name: String,
    pub path: PathBuf,
}

impl Folder {
    pub fn slug(&self) -> String {
        slugify(&self.name)
    }
}

fn default_listen() -> String {
    "0.0.0.0:8080".into()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,
    pub admin_ip: String,
    #[serde(default)]
    pub trust_proxy: bool,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub folders: Vec<Folder>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config I/O error: {0}")]
    Io(String),
    #[error("config parse error: {0}")]
    Parse(String),
    #[error("invalid config: {0}")]
    Invalid(String),
}

#[derive(Clone, Copy, Default, Debug, Serialize)]
pub struct EnvLocks {
    pub listen: bool,
    pub admin_ip: bool,
    pub trust_proxy: bool,
    pub allowed_ips: bool,
    pub folders: bool,
}

/// Lowercase; runs of non-alphanumeric chars become a single `-`; trimmed of `-`.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Parse a single IP (as a host network) or a CIDR.
pub fn parse_ip_rule(s: &str) -> Option<ipnet::IpNet> {
    let s = s.trim();
    if let Ok(net) = s.parse::<ipnet::IpNet>() {
        return Some(net);
    }
    s.parse::<IpAddr>().ok().map(ipnet::IpNet::from)
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let inv = |m: String| Err(ConfigError::Invalid(m));
        if self.admin_ip.trim().parse::<IpAddr>().is_err() {
            return inv(format!("admin_ip: '{}' is not an IP address", self.admin_ip));
        }
        for r in &self.allowed_ips {
            if parse_ip_rule(r).is_none() {
                return inv(format!("allowed_ips: '{r}' is not an IP or CIDR"));
            }
        }
        let mut slugs = HashSet::new();
        for f in &self.folders {
            let slug = f.slug();
            if slug.is_empty() {
                return inv(format!("folders: name '{}' produces an empty slug", f.name));
            }
            if !slugs.insert(slug.clone()) {
                return inv(format!("folders: duplicate slug '{slug}' (from name '{}')", f.name));
            }
            if !f.path.is_absolute() {
                return inv(format!("folders: path '{}' must be absolute", f.path.display()));
            }
            if !f.path.is_dir() {
                return inv(format!("folders: path '{}' is not an existing directory", f.path.display()));
            }
        }
        Ok(())
    }

    pub fn folder_by_slug(&self, slug: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| f.slug() == slug)
    }

    /// The admin IP. Panics only if the config was never validated.
    pub fn admin_ip(&self) -> IpAddr {
        self.admin_ip.trim().parse().expect("validated admin_ip")
    }
}

pub struct ConfigStore {
    path: PathBuf,
    file: Config,
    effective: Config,
    locks: EnvLocks,
}

fn apply_env(cfg: &mut Config, env: &HashMap<String, String>) -> Result<EnvLocks, ConfigError> {
    let mut locks = EnvLocks::default();
    if let Some(v) = env.get("ZIEWER_LISTEN") {
        cfg.listen = v.trim().into();
        locks.listen = true;
    }
    if let Some(v) = env.get("ZIEWER_ADMIN_IP") {
        cfg.admin_ip = v.trim().into();
        locks.admin_ip = true;
    }
    if let Some(v) = env.get("ZIEWER_TRUST_PROXY") {
        cfg.trust_proxy = match v.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => true,
            "false" | "0" => false,
            o => return Err(ConfigError::Invalid(format!("ZIEWER_TRUST_PROXY: '{o}' is not true/false"))),
        };
        locks.trust_proxy = true;
    }
    if let Some(v) = env.get("ZIEWER_ALLOWED_IPS") {
        cfg.allowed_ips = v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
        locks.allowed_ips = true;
    }
    if let Some(v) = env.get("ZIEWER_FOLDERS") {
        let mut folders = Vec::new();
        for part in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (name, path) = part.split_once('=').ok_or_else(|| {
                ConfigError::Invalid(format!("ZIEWER_FOLDERS: '{part}' must be Name=/path"))
            })?;
            folders.push(Folder { name: name.trim().into(), path: PathBuf::from(path.trim()) });
        }
        cfg.folders = folders;
        locks.folders = true;
    }
    Ok(locks)
}

impl ConfigStore {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let env: HashMap<String, String> = std::env::vars().collect();
        Self::load_with_env(path, &env)
    }

    pub fn load_with_env(path: &Path, env: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let text = fs::read_to_string(path)
            .map_err(|e| ConfigError::Io(format!("{}: {e}", path.display())))?;
        let file: Config = toml::from_str(&text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        let mut effective = file.clone();
        let locks = apply_env(&mut effective, env)?;
        effective.validate()?;
        Ok(Self { path: path.to_path_buf(), file, effective, locks })
    }

    pub fn effective(&self) -> &Config {
        &self.effective
    }

    pub fn locks(&self) -> EnvLocks {
        self.locks
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Apply `f` to both the on-disk and effective copies, validate, persist atomically.
    /// On error nothing changes.
    pub fn update(&mut self, f: impl Fn(&mut Config)) -> Result<(), ConfigError> {
        let mut file = self.file.clone();
        let mut effective = self.effective.clone();
        f(&mut file);
        f(&mut effective);
        effective.validate()?;
        let text = toml::to_string_pretty(&file).map_err(|e| ConfigError::Parse(e.to_string()))?;
        write_atomic(&self.path, text.as_bytes()).map_err(|e| ConfigError::Io(e.to_string()))?;
        self.file = file;
        self.effective = effective;
        Ok(())
    }
}

/// Write to `<path>.tmp` then rename over `path`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    fn write_cfg(dir: &Path, body: &str) -> PathBuf {
        let p = dir.join("ziewer.toml");
        fs::write(&p, body).unwrap();
        p
    }

    fn base_cfg(dir: &Path) -> Config {
        fs::create_dir_all(dir.join("docs")).unwrap();
        Config {
            listen: "0.0.0.0:8080".into(),
            admin_ip: "1.1.1.1".into(),
            trust_proxy: false,
            allowed_ips: vec!["2.2.2.2".into()],
            folders: vec![Folder { name: "Docs".into(), path: dir.join("docs") }],
        }
    }

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("My Docs!"), "my-docs");
        assert_eq!(slugify("  A__b  "), "a-b");
    }

    #[test]
    fn parse_rule_ip_and_cidr() {
        assert_eq!(parse_ip_rule("10.0.0.5").unwrap().to_string(), "10.0.0.5/32");
        assert_eq!(parse_ip_rule("10.0.0.0/24").unwrap().to_string(), "10.0.0.0/24");
        assert!(parse_ip_rule("abc").is_none());
    }

    #[test]
    fn validate_rejects_bad_allowed_ip() {
        let d = tempfile::tempdir().unwrap();
        let mut c = base_cfg(d.path());
        c.allowed_ips.push("abc".into());
        let e = c.validate().unwrap_err().to_string();
        assert!(e.contains("allowed_ips"), "{e}");
    }

    #[test]
    fn validate_rejects_bad_admin_ip() {
        let d = tempfile::tempdir().unwrap();
        let mut c = base_cfg(d.path());
        c.admin_ip = "x".into();
        assert!(c.validate().unwrap_err().to_string().contains("admin_ip"));
    }

    #[test]
    fn validate_rejects_missing_folder() {
        let d = tempfile::tempdir().unwrap();
        let mut c = base_cfg(d.path());
        c.folders.push(Folder { name: "X".into(), path: d.path().join("nope") });
        assert!(c.validate().unwrap_err().to_string().contains("folders"));
    }

    #[test]
    fn validate_rejects_duplicate_slug() {
        let d = tempfile::tempdir().unwrap();
        let mut c = base_cfg(d.path());
        c.folders.push(Folder { name: "docs".into(), path: d.path().join("docs") });
        assert!(c.validate().unwrap_err().to_string().contains("slug"));
    }

    #[test]
    fn env_overrides_and_locks() {
        let d = tempfile::tempdir().unwrap();
        let c = base_cfg(d.path());
        let p = write_cfg(d.path(), &toml::to_string(&c).unwrap());
        let s = ConfigStore::load_with_env(
            &p,
            &env(&[("ZIEWER_ADMIN_IP", "3.3.3.3"), ("ZIEWER_ALLOWED_IPS", "4.4.4.4, 5.5.5.0/24")]),
        )
        .unwrap();
        assert_eq!(s.effective().admin_ip, "3.3.3.3");
        assert_eq!(s.effective().allowed_ips, vec!["4.4.4.4", "5.5.5.0/24"]);
        assert!(s.locks().admin_ip && s.locks().allowed_ips && !s.locks().folders);
    }

    #[test]
    fn env_folders_parse() {
        let d = tempfile::tempdir().unwrap();
        let c = base_cfg(d.path());
        fs::create_dir_all(d.path().join("a")).unwrap();
        fs::create_dir_all(d.path().join("b")).unwrap();
        let p = write_cfg(d.path(), &toml::to_string(&c).unwrap());
        let v = format!("Docs={},Notes={}", d.path().join("a").display(), d.path().join("b").display());
        let s = ConfigStore::load_with_env(&p, &env(&[("ZIEWER_FOLDERS", &v)])).unwrap();
        assert_eq!(s.effective().folders.len(), 2);
        assert_eq!(s.effective().folders[1].name, "Notes");
        assert!(s.locks().folders);
    }

    #[test]
    fn update_writes_file_without_env_values() {
        let d = tempfile::tempdir().unwrap();
        let c = base_cfg(d.path());
        let p = write_cfg(d.path(), &toml::to_string(&c).unwrap());
        let mut s = ConfigStore::load_with_env(&p, &env(&[("ZIEWER_ADMIN_IP", "3.3.3.3")])).unwrap();
        s.update(|c| c.allowed_ips.push("9.9.9.9".into())).unwrap();
        let on_disk: Config = toml::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(on_disk.admin_ip, "1.1.1.1");
        assert!(on_disk.allowed_ips.contains(&"9.9.9.9".to_string()));
        assert!(s.effective().allowed_ips.contains(&"9.9.9.9".to_string()));
    }

    #[test]
    fn update_rolls_back_on_invalid() {
        let d = tempfile::tempdir().unwrap();
        let c = base_cfg(d.path());
        let p = write_cfg(d.path(), &toml::to_string(&c).unwrap());
        let before = fs::read(&p).unwrap();
        let mut s = ConfigStore::load_with_env(&p, &env(&[])).unwrap();
        assert!(s.update(|c| c.allowed_ips.push("nope".into())).is_err());
        assert_eq!(s.effective().allowed_ips, vec!["2.2.2.2"]);
        assert_eq!(fs::read(&p).unwrap(), before);
    }
}
