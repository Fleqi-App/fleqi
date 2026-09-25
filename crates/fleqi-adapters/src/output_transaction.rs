//! Generate into the destination filesystem first. Only complete, validated outputs
//! replace existing files; a failed conversion never truncates the old output.
use crate::capabilities::unique_destination;
use fleqi_application::run_service::NativeOutput;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

struct Publication {
    staged: PathBuf,
    target: PathBuf,
    backup: Option<PathBuf>,
    published: bool,
}

pub fn generate(
    directory: &Path,
    inputs: &[PathBuf],
    cancel: &AtomicBool,
    work: impl FnOnce(&Path) -> Result<NativeOutput, String>,
) -> Result<(NativeOutput, Vec<PathBuf>), String> {
    let staging = tempfile::Builder::new()
        .prefix(".fleqi-output-")
        .tempdir_in(directory)
        .map_err(|e| e.to_string())?;
    let mut result = work(staging.path())?;
    if cancel.load(Ordering::Acquire) {
        return Err("已取消，已有文件未被替换".into());
    }
    let mut entries = std::fs::read_dir(staging.path())
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut publications = Vec::new();
    for entry in entries {
        let staged = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
            return Err("输出包含不支持的文件类型，未替换已有文件".into());
        }
        let target = directory.join(entry.file_name());
        let protected = inputs.iter().any(|source| {
            target == *source
                || (source.is_dir() && target.starts_with(source))
                || target
                    .canonicalize()
                    .ok()
                    .zip(source.canonicalize().ok())
                    .is_some_and(|(target, source)| target == source)
        });
        let existing = std::fs::symlink_metadata(&target).ok();
        if existing
            .as_ref()
            .is_some_and(|meta| meta.file_type().is_symlink())
        {
            return Err(format!("目标是符号链接，未替换：{}", target.display()));
        }
        let keep_both =
            protected || kind.is_dir() || existing.as_ref().is_some_and(|meta| !meta.is_file());
        let target = if keep_both {
            unique_destination(directory, Path::new(&entry.file_name()))
        } else {
            target
        };
        publications.push(Publication {
            staged,
            target,
            backup: None,
            published: false,
        });
    }
    let backups = staging.path().join(".backup");
    std::fs::create_dir(&backups).map_err(|e| e.to_string())?;
    let commit = (|| -> Result<(), String> {
        for (index, item) in publications.iter_mut().enumerate() {
            if cancel.load(Ordering::Acquire) {
                return Err("已取消".into());
            }
            if item.target.exists() {
                if !item.target.is_file() {
                    return Err("目标已变为文件夹，停止替换".into());
                }
                let backup = backups.join(index.to_string());
                // Keep the old pathname readable until the single atomic rename
                // publishes the completed replacement on this filesystem.
                std::fs::hard_link(&item.target, &backup)
                    .or_else(|_| std::fs::copy(&item.target, &backup).map(|_| ()))
                    .map_err(|e| e.to_string())?;
                item.backup = Some(backup);
            }
            std::fs::rename(&item.staged, &item.target).map_err(|e| e.to_string())?;
            item.published = true;
        }
        Ok(())
    })();
    if let Err(error) = commit {
        let mut rollback_failed = false;
        for item in publications.iter().rev() {
            if item.published {
                let restored = if let Some(backup) = &item.backup {
                    std::fs::rename(backup, &item.target)
                } else if item.target.is_dir() {
                    std::fs::remove_dir_all(&item.target)
                } else {
                    std::fs::remove_file(&item.target)
                };
                rollback_failed |= restored.is_err();
            }
        }
        if rollback_failed {
            let retained = staging.keep();
            return Err(format!(
                "{error}；部分恢复失败，旧文件备份保留在 {}",
                retained.display()
            ));
        }
        return Err(format!("{error}；已有文件已恢复"));
    }
    for item in &publications {
        result.output = result.output.replace(
            item.staged.to_string_lossy().as_ref(),
            item.target.to_string_lossy().as_ref(),
        );
    }
    let replaced = publications
        .iter()
        .filter(|item| item.backup.is_some())
        .count();
    if replaced > 0 {
        result
            .output
            .push_str(&format!("\n已替换 {replaced} 个同名输出文件。\n"));
    }
    Ok((
        result,
        publications.into_iter().map(|item| item.target).collect(),
    ))
}
