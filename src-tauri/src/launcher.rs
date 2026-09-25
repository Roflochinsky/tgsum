//! Launcher entry for `cargo install` builds on Linux.
//!
//! The Linux packages install `tgsum.desktop` and the icons themselves. A
//! binary built by `cargo install` has neither, so on start it adds them to
//! the user's data dir (`~/.local/share`) and tgsum shows up in the app
//! launcher (Omarchy: Super + Space). The entry follows the binary if it
//! moves, and is removed once a package provides its own entry, so it never
//! shadows the package's.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The entry the Linux packages install.
const ENTRY: &str = include_str!("../tgsum.desktop");

/// Where the entry lives, relative to a data dir.
const ENTRY_PATH: &str = "applications/tgsum.desktop";

/// Marks entries written here: only those are ever replaced or removed.
const MARKER: &str = "X-Tgsum-Generated=true";

/// Icons for `Icon=tgsum`, relative to the data dir.
const ICONS: [(&str, &[u8]); 3] = [
    (
        "icons/hicolor/scalable/apps/tgsum.svg",
        include_bytes!("../icons/icon.svg"),
    ),
    (
        "icons/hicolor/128x128/apps/tgsum.png",
        include_bytes!("../icons/128x128.png"),
    ),
    (
        "icons/hicolor/256x256/apps/tgsum.png",
        include_bytes!("../icons/128x128@2x.png"),
    ),
];

/// Adds, refreshes or removes the user's launcher entry for the running
/// binary `exe`.
pub fn sync(var: impl Fn(&str) -> Option<String>, exe: &Path) -> io::Result<()> {
    let Some(data_home) = data_home(&var) else {
        return Ok(());
    };
    let entry = data_home.join(ENTRY_PATH);

    let packaged = data_dirs(&var)
        .iter()
        .map(|dir| dir.join(ENTRY_PATH))
        .any(|path| path.is_file() && !generated(&path));
    if packaged {
        if generated(&entry) {
            fs::remove_file(&entry)?;
            for (path, bytes) in ICONS {
                remove_if_same(&data_home.join(path), bytes)?;
            }
        }
        return Ok(());
    }

    // An entry the user wrote themselves stays as it is.
    if !installed_by_cargo(&var, exe) || (entry.exists() && !generated(&entry)) {
        return Ok(());
    }
    let Some(text) = entry_for(exe) else {
        return Ok(());
    };
    write_if_changed(&entry, text.as_bytes())?;
    for (path, bytes) in ICONS {
        write_if_changed(&data_home.join(path), bytes)?;
    }
    Ok(())
}

/// `$XDG_DATA_HOME`, or `~/.local/share`.
fn data_home(var: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    absolute(var("XDG_DATA_HOME"))
        .or_else(|| absolute(var("HOME")).map(|home| home.join(".local/share")))
}

/// `$XDG_DATA_DIRS`, or the spec's default.
fn data_dirs(var: &impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = var("XDG_DATA_DIRS")
        .unwrap_or_default()
        .split(':')
        .filter_map(|dir| absolute(Some(dir.to_owned())))
        .collect();
    if dirs.is_empty() {
        vec!["/usr/local/share".into(), "/usr/share".into()]
    } else {
        dirs
    }
}

/// Whether `exe` sits where `cargo install` puts binaries: `$CARGO_HOME/bin`
/// (`~/.cargo/bin`), the `$CARGO_INSTALL_ROOT` or `~/.local` root.
fn installed_by_cargo(var: &impl Fn(&str) -> Option<String>, exe: &Path) -> bool {
    let home = absolute(var("HOME"));
    let roots = [
        absolute(var("CARGO_HOME")).or_else(|| home.as_ref().map(|h| h.join(".cargo"))),
        absolute(var("CARGO_INSTALL_ROOT")),
        home.map(|h| h.join(".local")),
    ];
    let Some(dir) = exe
        .parent()
        .filter(|_| exe.is_file())
        .and_then(|dir| fs::canonicalize(dir).ok())
    else {
        return false;
    };
    roots
        .into_iter()
        .flatten()
        .filter_map(|root| fs::canonicalize(root.join("bin")).ok())
        .any(|bin| bin == dir)
}

fn absolute(path: Option<String>) -> Option<PathBuf> {
    path.map(PathBuf::from).filter(|p| p.is_absolute())
}

/// The packaged entry, launching `exe` by its full path (`~/.cargo/bin` is
/// often not on the launcher's `PATH`).
fn entry_for(exe: &Path) -> Option<String> {
    let path = exe.to_str().filter(|p| !p.chars().any(char::is_control))?;
    let mut text = String::new();
    for line in ENTRY.lines() {
        match line.strip_prefix("Exec=") {
            Some(command) => {
                text += &format!("Exec={}", exec_arg(path));
                if let Some((_, args)) = command.split_once(' ') {
                    text += &format!(" {args}");
                }
                text += &format!("\nTryExec={}\n", path.replace('\\', "\\\\"));
            }
            None => text += &format!("{line}\n"),
        }
    }
    Some(text + MARKER + "\n")
}

/// `path` as one quoted `Exec` argument: the spec's quoting rules, then its
/// string escapes on top, and `%%` for a literal `%`.
fn exec_arg(path: &str) -> String {
    let mut arg = String::from('"');
    for c in path.chars() {
        match c {
            '"' | '`' | '$' => {
                arg += "\\\\";
                arg.push(c);
            }
            '\\' => arg += "\\\\\\\\",
            '%' => arg += "%%",
            _ => arg.push(c),
        }
    }
    arg + "\""
}

fn generated(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|text| text.lines().any(|line| line.trim() == MARKER))
}

/// Writes through a temporary file, so launchers never read half an entry.
fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(());
    };
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp", name.to_string_lossy()));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

fn remove_if_same(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::read(path).is_ok_and(|old| old == bytes) {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A fake home with `~/.local/share`, a system data dir and a binary.
    struct Home {
        dir: TempDir,
    }

    impl Home {
        fn new() -> Self {
            Home {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.dir.path().join(rel)
        }

        fn var(&self) -> impl Fn(&str) -> Option<String> + '_ {
            move |key| match key {
                "HOME" => Some(self.dir.path().to_string_lossy().into_owned()),
                "XDG_DATA_DIRS" => Some(self.path("usr/share").to_string_lossy().into_owned()),
                _ => None,
            }
        }

        fn binary(&self, rel: &str) -> PathBuf {
            let exe = self.path(rel);
            fs::create_dir_all(exe.parent().unwrap()).unwrap();
            fs::write(&exe, b"").unwrap();
            exe
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.path(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        fn read(&self, rel: &str) -> Option<String> {
            fs::read_to_string(self.path(rel)).ok()
        }
    }

    const USER_ENTRY: &str = ".local/share/applications/tgsum.desktop";
    const SYSTEM_ENTRY: &str = "usr/share/applications/tgsum.desktop";
    const USER_ICON: &str = ".local/share/icons/hicolor/scalable/apps/tgsum.svg";

    #[test]
    fn registers_a_cargo_install_binary() {
        let home = Home::new();
        let exe = home.binary(".cargo/bin/tgsum");
        sync(home.var(), &exe).unwrap();

        let entry = home.read(USER_ENTRY).unwrap();
        let exe = exe.to_str().unwrap();
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains(&format!("\nExec=\"{exe}\" %f\nTryExec={exe}\n")));
        assert!(entry.contains("\nIcon=tgsum\n"));
        assert!(entry.contains("\nName=tgsum\n"));
        assert!(entry.ends_with(&format!("\n{MARKER}\n")));
        assert_eq!(entry.lines().filter(|l| l.starts_with("Exec=")).count(), 1);
        for (path, bytes) in ICONS {
            assert_eq!(
                fs::read(home.path(".local/share").join(path)).unwrap(),
                bytes
            );
        }
    }

    #[test]
    fn follows_the_binary_and_cargo_home() {
        let home = Home::new();
        sync(home.var(), &home.binary(".cargo/bin/tgsum")).unwrap();

        let moved = home.binary(".local/bin/tgsum");
        sync(home.var(), &moved).unwrap();
        let entry = home.read(USER_ENTRY).unwrap();
        assert!(entry.contains(&format!("TryExec={}\n", moved.display())));

        let cargo_home = home.path("rust/cargo");
        let exe = home.binary("rust/cargo/bin/tgsum");
        let var = |key: &str| match key {
            "CARGO_HOME" => Some(cargo_home.to_string_lossy().into_owned()),
            _ => home.var()(key),
        };
        sync(var, &exe).unwrap();
        let entry = home.read(USER_ENTRY).unwrap();
        assert!(entry.contains(&format!("TryExec={}\n", exe.display())));
    }

    #[test]
    fn ignores_other_binaries() {
        let home = Home::new();
        // `cargo run`, an unpacked AppImage, a missing file.
        for exe in [
            home.binary("src/tgsum/target/release/tgsum"),
            home.binary("tmp/.mount_tgsum/usr/bin/tgsum"),
            home.path(".cargo/bin/gone"),
        ] {
            sync(home.var(), &exe).unwrap();
        }
        assert_eq!(home.read(USER_ENTRY), None);
        assert_eq!(home.read(USER_ICON), None);
    }

    #[test]
    fn keeps_an_entry_the_user_wrote() {
        let home = Home::new();
        let own = "[Desktop Entry]\nName=my tgsum\nExec=tgsum --flag\n";
        home.write(USER_ENTRY, own);
        sync(home.var(), &home.binary(".cargo/bin/tgsum")).unwrap();
        assert_eq!(home.read(USER_ENTRY).as_deref(), Some(own));
    }

    #[test]
    fn a_package_entry_takes_over() {
        let home = Home::new();
        let exe = home.binary(".cargo/bin/tgsum");
        sync(home.var(), &exe).unwrap();
        assert!(home.read(USER_ENTRY).is_some());

        home.write(SYSTEM_ENTRY, ENTRY);
        sync(home.var(), &exe).unwrap();
        assert_eq!(home.read(USER_ENTRY), None);
        assert_eq!(home.read(USER_ICON), None);

        // Nothing comes back while the package is installed, and an entry the
        // user wrote is never removed.
        sync(home.var(), &exe).unwrap();
        assert_eq!(home.read(USER_ENTRY), None);
        home.write(USER_ENTRY, "[Desktop Entry]\nName=mine\n");
        sync(home.var(), &exe).unwrap();
        assert!(home.read(USER_ENTRY).is_some());
    }

    #[test]
    fn its_own_entry_on_the_data_dirs_is_no_package() {
        // Some sessions list ~/.local/share in XDG_DATA_DIRS too.
        let home = Home::new();
        let share = home.path(".local/share").to_string_lossy().into_owned();
        let var = |key: &str| match key {
            "XDG_DATA_DIRS" => Some(format!("{share}:/nonexistent")),
            _ => home.var()(key),
        };
        let exe = home.binary(".cargo/bin/tgsum");
        sync(var, &exe).unwrap();
        sync(var, &exe).unwrap();
        assert!(home.read(USER_ENTRY).is_some());
    }

    #[test]
    fn data_dirs_follow_the_xdg_spec() {
        let none = |_: &str| None;
        assert_eq!(data_home(&none), None);
        assert_eq!(
            data_dirs(&none),
            [PathBuf::from("/usr/local/share"), "/usr/share".into()]
        );
        let set = |key: &str| match key {
            "HOME" => Some("/home/me".to_owned()),
            "XDG_DATA_HOME" => Some("relative/is/ignored".to_owned()),
            "XDG_DATA_DIRS" => Some("/opt/share::relative:/usr/share".to_owned()),
            _ => None,
        };
        assert_eq!(data_home(&set), Some("/home/me/.local/share".into()));
        assert_eq!(
            data_dirs(&set),
            [PathBuf::from("/opt/share"), "/usr/share".into()]
        );
    }

    #[test]
    fn exec_paths_are_quoted_and_escaped() {
        assert_eq!(
            exec_arg("/home/me/.cargo/bin/tgsum"),
            r#""/home/me/.cargo/bin/tgsum""#
        );
        assert_eq!(
            exec_arg(r#"/home/a b/"q"/$x/`c`/back\slash/100%/tgsum"#),
            r#""/home/a b/\\"q\\"/\\$x/\\`c\\`/back\\\\slash/100%%/tgsum""#
        );
        assert_eq!(entry_for(Path::new("/bin/tg\nsum")), None);
    }
}
