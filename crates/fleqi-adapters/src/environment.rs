//! Share executable search paths between GUI-launched tasks and tool discovery.
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

pub(crate) fn executable_paths(inherited: &OsStr) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    // App bundles launched by Finder usually inherit only the system PATH.
    let defaults = if cfg!(target_os = "macos") {
        vec![
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/opt/homebrew/opt/whisper-cpp/bin",
            "/usr/local/opt/whisper-cpp/bin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
        ]
    } else {
        Vec::new()
    };
    for directory in defaults
        .into_iter()
        .map(PathBuf::from)
        .chain(std::env::split_paths(inherited))
    {
        if directory.is_absolute() && !paths.contains(&directory) {
            paths.push(directory);
        }
    }
    paths
}

pub(crate) fn executable_path(inherited: &OsStr) -> OsString {
    std::env::join_paths(executable_paths(inherited)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn gui_path_includes_homebrew_without_searching_working_directory() {
        let directories =
            executable_paths(OsStr::new("/usr/bin:/bin::.:relative:/custom/bin:/usr/bin"));
        assert!(directories.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(directories.contains(&PathBuf::from("/usr/local/bin")));
        assert!(directories.contains(&PathBuf::from("/custom/bin")));
        assert!(directories.iter().all(|path| path.is_absolute()));
        assert_eq!(
            directories
                .iter()
                .filter(|path| path.as_path() == std::path::Path::new("/usr/bin"))
                .count(),
            1
        );
    }
}
