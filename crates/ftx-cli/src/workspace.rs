//! Persistent wearer workspaces. Old rounds and measurement archives are never overwritten.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ftx_core::{Refusal, RefusalCode};
use serde_json::Value;

use super::{Args, bad_input, read_json};

pub(super) struct Evidence {
    /// Original bytes, kept separately from the recomputed fused view.
    pub bytes: Vec<u8>,
    pub value: Value,
}

pub(super) fn evidence(path: &str, args: &Args) -> Result<Evidence, Refusal> {
    let (raw, bytes) = read_json(path)?;
    if !raw.is_object() {
        return Err(bad_input("measurements must be an object"));
    }
    let value = if raw.get("records").is_some() || raw.get("schema").is_some() {
        ftx_measure::fuse(&raw, !args.options.contains_key("no-priors"))?
    } else {
        raw // Legacy flat maps remain explicitly unknown-provenance at the readiness gate.
    };
    Ok(Evidence { bytes, value })
}

pub(super) fn io_error(path: &Path, e: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display()))
}

pub(super) fn json_bytes(value: &Value) -> Result<Vec<u8>, Refusal> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| bad_input(e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    let mut file = private_options()
        .create_new(true)
        .open(path)
        .map_err(|e| io_error(path, e))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| io_error(path, e))
}

pub(super) fn write_json(path: &Path, value: &Value) -> Result<(), Refusal> {
    write_new(path, &json_bytes(value)?)
}

/// Refuse links for mutable workspace metadata. User-selected input paths may still be links.
fn no_link(path: &Path) -> Result<(), Refusal> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(bad_input(format!(
            "{}: workspace output must not be a symbolic link",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(path, e)),
    }
}

/// Atomic replacement only for current workspace metadata, never old round files. Failed temp
/// writes are retained for recovery; the destination is not opened for truncation.
pub(super) fn replace(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    no_link(path)?;
    let parent = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| bad_input("output needs a filename"))?;
    loop {
        let temp = parent.join(format!(
            ".{}.ftx-{}-{}.tmp",
            name.to_string_lossy(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = match private_options().create_new(true).open(&temp) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error(&temp, e)),
        };
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| io_error(&temp, e))?;
        drop(file);
        fs::rename(&temp, path).map_err(|e| io_error(path, e))?;
        return Ok(());
    }
}

pub(super) fn read(path: &Path) -> Result<Value, Refusal> {
    let bytes = fs::read(path).map_err(|e| io_error(path, e))?;
    serde_json::from_slice(&bytes).map_err(|e| bad_input(format!("{}: {e}", path.display())))
}

/// A process-scoped OS lock serializes workspace mutations; dropping the file releases it, even
/// after a failed round. The persistent lock file has no stale PID ownership to recover or delete.
pub(super) struct Workspace {
    pub root: PathBuf,
    _lock: File,
}

impl Workspace {
    pub(super) fn open(root: &Path) -> Result<Self, Refusal> {
        fs::create_dir_all(root).map_err(|e| io_error(root, e))?;
        for name in [
            ".ftx-lock",
            "rounds",
            "rounds.jsonl",
            "measurements.json",
            "fit_spec.json",
            "report.html",
        ] {
            no_link(&root.join(name))?;
        }
        let path = root.join(".ftx-lock");
        let lock = private_options()
            .read(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| io_error(&path, e))?;
        lock.try_lock().map_err(|e| {
            io_error(&path, e)
                .repair("another ftx command may be using this workspace; retry after it finishes")
        })?;
        Ok(Self {
            root: root.to_path_buf(),
            _lock: lock,
        })
    }

    pub(super) fn reserve_round(&self) -> Result<(u64, PathBuf), Refusal> {
        let rounds = self.root.join("rounds");
        fs::create_dir_all(&rounds).map_err(|e| io_error(&rounds, e))?;
        let number = next_round(&self.root)?;
        let dir = rounds.join(format!("{number:03}"));
        fs::create_dir(&dir).map_err(|e| io_error(&dir, e))?;
        Ok((number, dir))
    }

    pub(super) fn adopt(
        &self,
        evidence: &Evidence,
        update: bool,
    ) -> Result<Option<String>, Refusal> {
        check_adoption(&self.root, &evidence.bytes, update)?;
        let path = self.root.join("measurements.json");
        let old = match fs::read(&path) {
            Ok(old) => old,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                write_new(&path, &evidence.bytes)?;
                return Ok(None);
            }
            Err(e) => return Err(io_error(&path, e)),
        };
        if old == evidence.bytes {
            return Ok(None);
        }
        let base = format!("measurements.before-round-{:03}", next_round(&self.root)?);
        for suffix in 0_u64.. {
            let name = if suffix == 0 {
                format!("{base}.json")
            } else {
                format!("{base}.{suffix:03}.json")
            };
            let archive = self.root.join(&name);
            let mut file = match private_options().create_new(true).open(&archive) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_error(&archive, e)),
            };
            file.write_all(&old).map_err(|e| io_error(&archive, e))?;
            let permissions = fs::metadata(&path)
                .map_err(|e| io_error(&path, e))?
                .permissions();
            file.set_permissions(permissions)
                .and_then(|()| file.sync_all())
                .map_err(|e| io_error(&archive, e))?;
            replace(&path, &evidence.bytes)?;
            return Ok(Some(format!(
                "measurements updated; the previous file is kept as {name}"
            )));
        }
        Err(bad_input("measurement archive slots exhausted"))
    }
}

pub(super) fn check_adoption(root: &Path, new: &[u8], update: bool) -> Result<(), Refusal> {
    let path = root.join("measurements.json");
    no_link(&path)?;
    match fs::read(&path) {
        Ok(old) if old != new && !update => Err(bad_input(format!(
            "{} differs from the supplied measurements. A workspace is one body: add --update-measurements to adopt the new file (the old one is kept), or start a new workspace.",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(&path, e)),
    }
}

fn numbered_rounds(root: &Path) -> Result<Vec<(u64, PathBuf)>, Refusal> {
    let path = root.join("rounds");
    no_link(&path)?;
    let entries = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(&path, e)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io_error(&path, e))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        no_link(&entry.path())?;
        if entry
            .file_type()
            .map_err(|e| io_error(&entry.path(), e))?
            .is_dir()
        {
            let number = name
                .parse()
                .map_err(|_| bad_input("round number is out of range"))?;
            out.push((number, entry.path()));
        }
    }
    out.sort();
    Ok(out)
}

pub(super) fn next_round(root: &Path) -> Result<u64, Refusal> {
    numbered_rounds(root)?.last().map_or(Ok(0), |(n, _)| {
        n.checked_add(1)
            .ok_or_else(|| bad_input("round number is out of range"))
    })
}
