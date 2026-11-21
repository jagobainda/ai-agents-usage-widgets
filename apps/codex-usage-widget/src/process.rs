use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const DEFAULT_PATH_EXTENSIONS: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.JS;.WS;.MSC";

pub fn find_codex() -> Option<PathBuf> {
    let path = env::var_os("PATH").unwrap_or_else(|| r".;C:\bin".into());
    if path.is_empty() {
        return None;
    }
    let cwd = env::current_dir().ok()?;
    let mut directories: Vec<_> = env::split_paths(&path)
        .map(|path| {
            if path.as_os_str().is_empty() {
                cwd.clone()
            } else {
                path
            }
        })
        .collect();
    if env::var_os("NoDefaultCurrentDirectoryInExePath").is_none() {
        directories.insert(0, cwd.clone());
    }
    let extensions = env::var("PATHEXT")
        .ok()
        .filter(|extensions| !extensions.is_empty())
        .unwrap_or_else(|| DEFAULT_PATH_EXTENSIONS.into());
    find_codex_in(directories, &cwd, &extensions)
}

fn find_codex_in(directories: Vec<PathBuf>, cwd: &Path, extensions: &str) -> Option<PathBuf> {
    let candidates: Vec<_> = extensions
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| format!("codex{}", extension.trim_end_matches('.')))
        .collect();
    for directory in directories {
        let Ok(paths) = env::join_paths([directory]) else {
            continue;
        };
        for name in &candidates {
            if let Ok(matches) = which::which_in_all(name, Some(&paths), cwd)
                && let Some(path) = matches.into_iter().find(|path| {
                    path.file_name()
                        .is_some_and(|found| found.eq_ignore_ascii_case(OsStr::new(name)))
                })
            {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_uses_directory_then_extension_order_and_finds_cmd() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let script = first.path().join("codex.cmd");
        let executable = first.path().join("codex.exe");
        std::fs::write(first.path().join("codex"), "").unwrap();
        std::fs::write(&script, "").unwrap();
        std::fs::write(second.path().join("codex.exe"), "").unwrap();
        let directories = vec![first.path().to_owned(), second.path().to_owned()];
        assert_eq!(
            find_codex_in(directories.clone(), first.path(), ".EXE;.CMD"),
            Some(script)
        );
        std::fs::write(&executable, "").unwrap();
        assert_eq!(
            find_codex_in(directories, first.path(), ".EXE;.CMD"),
            Some(executable)
        );
    }
}
