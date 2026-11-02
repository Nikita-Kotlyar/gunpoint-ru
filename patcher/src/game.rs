//! Steam/Gunpoint discovery and install-state detection.

use crate::formats::Manifest;
use crate::sha256;
use crate::win;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Original,
    Patched,
    Mixed,
    Unknown,
}

impl State {
    pub fn ru(&self) -> &'static str {
        match self {
            State::Original => "оригинал (не пропатчено)",
            State::Patched => "уже пропатчено",
            State::Mixed => "частично пропатчено",
            State::Unknown => "неизвестная версия",
        }
    }
}

pub fn looks_like_game(dir: &Path) -> bool {
    dir.join("Gunpoint.exe").is_file() && dir.join("Gunpoint.wad").is_file()
}

pub fn detect_state(dir: &Path, m: &Manifest) -> State {
    let exe = dir.join("Gunpoint.exe");
    let wad = dir.join("Gunpoint.wad");
    let exe_h = match sha256::hash_file(&exe) {
        Ok(h) => h,
        Err(_) => return State::Unknown,
    };
    let wad_h = match sha256::hash_file(&wad) {
        Ok(h) => h,
        Err(_) => return State::Unknown,
    };
    let exe_orig = exe_h == m.orig_exe_sha;
    let wad_orig = wad_h == m.orig_wad_sha;
    let exe_pat = exe_h == m.patched_exe_sha;
    let wad_pat = wad_h == m.patched_wad_sha;
    if exe_orig && wad_orig {
        State::Original
    } else if exe_pat && wad_pat {
        State::Patched
    } else if (exe_orig && wad_pat) || (exe_pat && wad_orig) {
        State::Mixed
    } else {
        State::Unknown
    }
}

fn quoted_strings(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c == '"' {
            let mut s = String::new();
            for c2 in it.by_ref() {
                if c2 == '"' {
                    break;
                }
                s.push(c2);
            }
            out.push(s);
        }
    }
    out
}

fn unescape_vdf(s: &str) -> String {
    s.replace("\\\\", "\\")
}

/// Deduplication key: case-insensitive and with `/` normalized to `\`.
fn path_key(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn parse_library_folders(vdf: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(text) = std::fs::read_to_string(vdf) else {
        return out;
    };
    for line in text.lines() {
        let q = quoted_strings(line);
        if q.iter().any(|s| s.eq_ignore_ascii_case("path")) {
            if let Some(p) = q.last() {
                let p = unescape_vdf(p).replace('/', "\\");
                if !p.is_empty() {
                    out.push(PathBuf::from(p));
                }
            }
        } else if q.len() >= 2 && q[0].chars().all(|c| c.is_ascii_digit()) {
            // Old libraryfolders.vdf: "1"  "D:\\SteamLibrary"
            out.push(PathBuf::from(unescape_vdf(&q[1]).replace('/', "\\")));
        }
    }
    out
}

fn parse_installdir(acf: &Path) -> Option<String> {
    let text = std::fs::read_to_string(acf).ok()?;
    for line in text.lines() {
        let q = quoted_strings(line);
        if q.iter().any(|s| s.eq_ignore_ascii_case("installdir")) {
            return q.last().cloned();
        }
    }
    None
}

pub fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(p) = win::reg_sz(win::HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath") {
        roots.push(PathBuf::from(p.replace('/', "\\")));
    }
    if let Some(p) = win::reg_sz(
        win::HKEY_LOCAL_MACHINE,
        r"SOFTWARE\WOW6432Node\Valve\Steam",
        "InstallPath",
    ) {
        roots.push(PathBuf::from(p.replace('/', "\\")));
    }
    let mut all = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for r in roots {
        if seen.insert(path_key(&r)) {
            all.push(r.clone());
        }
        let vdf = r.join("steamapps").join("libraryfolders.vdf");
        for lib in parse_library_folders(&vdf) {
            if seen.insert(path_key(&lib)) {
                all.push(lib);
            }
        }
    }
    all
}

#[cfg(test)]
mod tests {
    #[test]
    fn candidates_are_deduplicated() {
        let c = super::candidates();
        assert!(!c.is_empty(), "Steam Gunpoint not found on this machine");
        let mut keys: Vec<String> = c.iter().map(|p| super::path_key(p)).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), c.len(), "duplicate candidates: {:?}", c);
    }
}

pub fn candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |p: PathBuf, out: &mut Vec<PathBuf>| {
        if !looks_like_game(&p) {
            return;
        }
        if seen.insert(path_key(&p)) {
            out.push(p);
        }
    };
    for root in steam_roots() {
        push(root.join("steamapps").join("common").join("Gunpoint"), &mut out);
        if let Some(inst) = parse_installdir(&root.join("steamapps").join("appmanifest_206190.acf")) {
            push(root.join("steamapps").join("common").join(inst), &mut out);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        push(cwd.clone(), &mut out);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            push(dir.to_path_buf(), &mut out);
        }
    }
    // Also accept a game dir passed via the current directory's parent (common when
    // the patcher is unpacked next to the game).
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(parent) = cwd.parent() {
            push(parent.join("Gunpoint"), &mut out);
        }
    }
    out
}

