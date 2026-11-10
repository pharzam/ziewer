//! Install / control Ziewer as an OS service (systemd, launchd, Windows SCM).

use crate::config::ConfigStore;
use anyhow::{Context, bail};
use service_manager::{
    ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx, ServiceStopCtx,
    ServiceUninstallCtx,
};
use std::ffi::OsString;
use std::path::Path;

pub const LABEL: &str = "ziewer";

/// Arguments the service runs with: `serve --config <absolute path>`.
pub fn service_args(config: &Path) -> Vec<OsString> {
    vec!["serve".into(), "--config".into(), config.as_os_str().to_owned()]
}

fn label() -> ServiceLabel {
    LABEL.parse().expect("valid label")
}

fn manager() -> anyhow::Result<Box<dyn ServiceManager>> {
    let mut m = <dyn ServiceManager>::native().context("no supported service manager on this OS")?;
    m.set_level(ServiceLevel::System).context("system-level services are not supported here")?;
    if !m.available().unwrap_or(false) {
        bail!("the OS service manager is not available");
    }
    Ok(m)
}

/// Validate the config, register the service with it baked in, and start it.
pub fn install(config: &Path) -> anyhow::Result<()> {
    let config = config
        .canonicalize()
        .with_context(|| format!("config file '{}' not found", config.display()))?;
    if config.to_string_lossy().contains(char::is_whitespace) {
        bail!("config path '{}' contains spaces; move it to a path without spaces", config.display());
    }
    ConfigStore::load(&config).map_err(|e| anyhow::anyhow!("config invalid: {e}"))?;
    let program = std::env::current_exe().context("locating the ziewer executable")?;
    if program.to_string_lossy().contains(char::is_whitespace) {
        bail!("executable path '{}' contains spaces; move the binary (e.g. /usr/local/bin/ziewer)", program.display());
    }
    let m = manager()?;
    m.install(ServiceInstallCtx {
        label: label(),
        program,
        args: service_args(&config),
        contents: None,
        username: None,
        working_directory: config.parent().map(Path::to_path_buf),
        environment: None,
        autostart: true,
    })
    .context("installing the service (are you root / Administrator?)")?;
    m.start(ServiceStartCtx { label: label() }).context("starting the service")?;
    Ok(())
}

/// Stop (best effort) and unregister the service.
pub fn uninstall() -> anyhow::Result<()> {
    let m = manager()?;
    let _ = m.stop(ServiceStopCtx { label: label() });
    m.uninstall(ServiceUninstallCtx { label: label() })
        .context("uninstalling the service (are you root / Administrator?)")
}

pub fn start() -> anyhow::Result<()> {
    manager()?.start(ServiceStartCtx { label: label() }).context("starting the service")
}

pub fn stop() -> anyhow::Result<()> {
    manager()?.stop(ServiceStopCtx { label: label() }).context("stopping the service")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_use_absolute_config() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ziewer.toml");
        let args = service_args(&p);
        let expect: Vec<OsString> = vec!["serve".into(), "--config".into(), p.clone().into_os_string()];
        assert_eq!(args, expect);
        assert!(Path::new(&args[2]).is_absolute());
    }

    #[test]
    fn install_rejects_path_with_spaces() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("a b")).unwrap();
        let p = d.path().join("a b/ziewer.toml");
        std::fs::write(&p, "x").unwrap();
        let err = install(&p).unwrap_err().to_string();
        assert!(err.contains("spaces"), "{err}");
    }

    #[test]
    fn install_rejects_invalid_config() {
        let d = tempfile::tempdir().unwrap();
        let err = install(&d.path().join("missing.toml")).unwrap_err().to_string();
        assert!(err.contains("config"), "{err}");
    }
}
