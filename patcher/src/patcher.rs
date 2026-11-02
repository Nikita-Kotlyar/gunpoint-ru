//! Install / restore / verify implementation.

use crate::formats::{ExePatch, GpcFile, Manifest, WadEntryPatch, WadPatch};
use crate::game::{self, State};
use crate::sha256;
use crate::win;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const EXE: &str = "Gunpoint.exe";
const WAD: &str = "Gunpoint.wad";
const SCRIPTS: &str = "Scripts";

struct WadEntry {
    name: String,
    size: u32,
    abs: u64,
}

fn backup_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("GunpointRU").join("backup")
}

fn ensure_writable(path: &Path) -> Result<(), String> {
    OpenOptions::new()
        .write(true)
        .open(path)
        .map(|_| ())
        .map_err(|e| {
            format!(
                "{}: {}. Закройте игру и запустите патчер с правами администратора, если игра установлена в Program Files.",
                path.display(),
                e
            )
        })
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    path.with_file_name(format!(".{}.gunpoint_ru_tmp", name))
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = temp_path(path);
    fs::write(&tmp, data)
        .map_err(|e| format!("не удалось записать {}: {}", tmp.display(), e))?;
    win::move_replace(&tmp, path)
}

fn make_backup(game_dir: &Path, gpc_names: &[String]) -> Result<PathBuf, String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let root = backup_root();
    fs::create_dir_all(&root)
        .map_err(|e| format!("не удалось создать {}: {}", root.display(), e))?;
    let mut dir = root.join(format!("backup-{ts}"));
    let mut n = 0;
    while dir.exists() {
        n += 1;
        dir = root.join(format!("backup-{ts}-{n}"));
    }
    fs::create_dir_all(dir.join(SCRIPTS))
        .map_err(|e| format!("не удалось создать {}: {}", dir.display(), e))?;
    for name in [EXE, WAD] {
        let src = game_dir.join(name);
        let dst = dir.join(name);
        fs::copy(&src, &dst)
            .map_err(|e| format!("резервная копия {}: {}", src.display(), e))?;
    }
    for name in gpc_names {
        let src = game_dir.join(SCRIPTS).join(name);
        let dst = dir.join(SCRIPTS).join(name);
        if !src.is_file() {
            return Err(format!(
                "не найден {} — файлы игры неполные, установка отменена",
                src.display()
            ));
        }
        fs::copy(&src, &dst)
            .map_err(|e| format!("резервная копия {}: {}", src.display(), e))?;
    }
    let info = format!(
        "game_dir={}\nversion={}\n",
        game_dir.display(),
        env!("CARGO_PKG_VERSION")
    );
    fs::write(dir.join("backup_info.txt"), info)
        .map_err(|e| format!("не удалось записать backup_info.txt: {}", e))?;
    Ok(dir)
}

fn patch_exe(game_dir: &Path, m: &Manifest, patch: &ExePatch) -> Result<(), String> {
    let path = game_dir.join(EXE);
    let orig = fs::read(&path).map_err(|e| format!("чтение {}: {}", path.display(), e))?;
    if orig.len() as u64 != m.orig_exe_size {
        return Err(format!(
            "неожиданный размер Gunpoint.exe: {} (ожидался {})",
            orig.len(),
            m.orig_exe_size
        ));
    }
    let mut out = orig.clone();
    out.resize(m.patched_exe_size as usize, 0);
    for (off, data) in &patch.writes {
        let o = *off as usize;
        if o + data.len() > orig.len() {
            return Err("патч exe выходит за границы файла".to_string());
        }
        out[o..o + data.len()].copy_from_slice(data);
    }
    let a = patch.append_offset as usize;
    if a != orig.len() || a + patch.append.len() != out.len() {
        return Err("патч exe: неверное смещение .gtxt".to_string());
    }
    out[a..].copy_from_slice(patch.append);
    let h = sha256::hash_bytes(&out);
    if h != m.patched_exe_sha {
        return Err("патч exe: контрольная сумма результата не совпала".to_string());
    }
    write_atomic(&path, &out)
}

fn read_wad_header(path: &Path) -> Result<(u32, Vec<WadEntry>), String> {
    let mut f = File::open(path).map_err(|e| format!("чтение {}: {}", path.display(), e))?;
    let mut b = [0u8; 8];
    f.read_exact(&mut b)
        .map_err(|e| format!("чтение заголовка WAD: {}", e))?;
    let base = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let count = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    if base != 0x1B74 || count != 162 {
        return Err(format!(
            "неожиданный заголовок WAD: base={:#x}, entries={}",
            base, count
        ));
    }
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let mut b4 = [0u8; 4];
        f.read_exact(&mut b4)
            .map_err(|e| format!("чтение таблицы WAD: {}", e))?;
        let nlen = u32::from_le_bytes(b4) as usize;
        let mut nb = vec![0u8; nlen];
        f.read_exact(&mut nb)
            .map_err(|e| format!("чтение имени WAD: {}", e))?;
        let name = String::from_utf8_lossy(&nb).to_string();
        let mut b8 = [0u8; 8];
        f.read_exact(&mut b8)
            .map_err(|e| format!("чтение записи WAD: {}", e))?;
        let size = u32::from_le_bytes([b8[0], b8[1], b8[2], b8[3]]);
        let off = u32::from_le_bytes([b8[4], b8[5], b8[6], b8[7]]);
        entries.push(WadEntry {
            name,
            size,
            abs: base as u64 + off as u64,
        });
    }
    Ok((base, entries))
}

fn copy_range(src: &mut File, dst: &mut File, off: u64, len: u64) -> Result<(), String> {
    src.seek(SeekFrom::Start(off))
        .map_err(|e| format!("seek WAD: {}", e))?;
    let mut remaining = len;
    let mut buf = vec![0u8; 1024 * 1024];
    while remaining > 0 {
        let n = remaining.min(buf.len() as u64) as usize;
        src.read_exact(&mut buf[..n])
            .map_err(|e| format!("чтение WAD: {}", e))?;
        dst.write_all(&buf[..n])
            .map_err(|e| format!("запись WAD: {}", e))?;
        remaining -= n as u64;
    }
    Ok(())
}

fn reconstruct_entry(
    src: &mut File,
    e: &WadEntry,
    p: &WadEntryPatch,
) -> Result<Vec<u8>, String> {
    if p.mode == 0 {
        if p.full.len() != p.patched_size as usize {
            return Err(format!("{}: неверный размер полной замены", p.name));
        }
        return Ok(p.full.to_vec());
    }
    if e.size != p.original_size {
        return Err(format!(
            "{}: размер в WAD {} не совпадает с ожидаемым {}",
            p.name, e.size, p.original_size
        ));
    }
    let mut data = vec![0u8; e.size as usize];
    src.seek(SeekFrom::Start(e.abs))
        .map_err(|e| format!("seek WAD: {}", e))?;
    src.read_exact(&mut data)
        .map_err(|e| format!("чтение WAD: {}", e))?;
    for (off, bytes) in &p.ranges {
        let o = *off as usize;
        if o + bytes.len() > data.len() {
            return Err(format!("{}: диапазон патча вне записи", p.name));
        }
        data[o..o + bytes.len()].copy_from_slice(bytes);
    }
    Ok(data)
}

fn patch_wad(game_dir: &Path, m: &Manifest, patch: &WadPatch) -> Result<(), String> {
    let path = game_dir.join(WAD);
    let meta = fs::metadata(&path).map_err(|e| format!("stat {}: {}", path.display(), e))?;
    if meta.len() != m.orig_wad_size {
        return Err(format!(
            "неожиданный размер Gunpoint.wad: {} (ожидался {})",
            meta.len(),
            m.orig_wad_size
        ));
    }
    let (base, entries) = read_wad_header(&path)?;
    let mut map: HashMap<&str, &WadEntryPatch> = HashMap::new();
    for p in &patch.entries {
        let found = entries.iter().find(|e| e.name == p.name);
        let Some(e) = found else {
            return Err(format!("в WAD нет записи {}", p.name));
        };
        if e.size != p.original_size {
            return Err(format!(
                "{}: размер в WAD {} не совпадает с ожидаемым {}",
                p.name, e.size, p.original_size
            ));
        }
        if p.mode == 0 && p.full.len() != p.patched_size as usize {
            return Err(format!("{}: неверная длина полной замены", p.name));
        }
        map.insert(p.name.as_str(), p);
    }
    let new_base = 8 + entries
        .iter()
        .map(|e| 4 + e.name.len() + 8)
        .sum::<usize>();
    if new_base as u32 != base {
        return Err(format!(
            "WAD: вычисленный base {} не совпадает с исходным {}",
            new_base, base
        ));
    }
    let mut sizes = Vec::with_capacity(entries.len());
    let mut offs = Vec::with_capacity(entries.len());
    let mut cur = new_base as u64;
    for e in &entries {
        let size = map
            .get(e.name.as_str())
            .map(|p| p.patched_size as u64)
            .unwrap_or(e.size as u64);
        sizes.push(size);
        offs.push(cur);
        cur += size;
    }
    if cur != m.patched_wad_size {
        return Err(format!(
            "WAD: итоговый размер {} не совпадает с ожидаемым {}",
            cur, m.patched_wad_size
        ));
    }

    let tmp = temp_path(&path);
    {
        let mut out = BufWriter::new(
            File::create(&tmp).map_err(|e| format!("создание {}: {}", tmp.display(), e))?,
        );
        out.write_all(&(new_base as u32).to_le_bytes())
            .map_err(|e| format!("запись WAD: {}", e))?;
        out.write_all(&(entries.len() as u32).to_le_bytes())
            .map_err(|e| format!("запись WAD: {}", e))?;
        for (i, e) in entries.iter().enumerate() {
            let nb = e.name.as_bytes();
            out.write_all(&(nb.len() as u32).to_le_bytes())
                .map_err(|e| format!("запись WAD: {}", e))?;
            out.write_all(nb)
                .map_err(|e| format!("запись WAD: {}", e))?;
            out.write_all(&(sizes[i] as u32).to_le_bytes())
                .map_err(|e| format!("запись WAD: {}", e))?;
            out.write_all(&((offs[i] - new_base as u64) as u32).to_le_bytes())
                .map_err(|e| format!("запись WAD: {}", e))?;
        }
        out.flush().map_err(|e| format!("запись WAD: {}", e))?;
    }
    if fs::metadata(&tmp)
        .map_err(|e| format!("stat {}: {}", tmp.display(), e))?
        .len()
        != new_base as u64
    {
        return Err("WAD: заголовок записан неверно".to_string());
    }

    let mut src = File::open(&path).map_err(|e| format!("чтение {}: {}", path.display(), e))?;
    let mut dst = OpenOptions::new()
        .append(true)
        .open(&tmp)
        .map_err(|e| format!("открытие {}: {}", tmp.display(), e))?;
    for (i, e) in entries.iter().enumerate() {
        if let Some(p) = map.get(e.name.as_str()) {
            let data = reconstruct_entry(&mut src, e, p)?;
            if data.len() != sizes[i] as usize {
                return Err(format!("{}: неверный размер после патча", p.name));
            }
            dst.write_all(&data)
                .map_err(|e| format!("запись WAD: {}", e))?;
        } else {
            copy_range(&mut src, &mut dst, e.abs, e.size as u64)?;
        }
    }
    dst.flush().map_err(|e| format!("запись WAD: {}", e))?;
    drop(dst);
    drop(src);

    let sz = fs::metadata(&tmp)
        .map_err(|e| format!("stat {}: {}", tmp.display(), e))?
        .len();
    if sz != m.patched_wad_size {
        return Err(format!(
            "WAD: размер результата {} не совпадает с ожидаемым {}",
            sz, m.patched_wad_size
        ));
    }
    let h = sha256::hash_file(&tmp).map_err(|e| format!("хэш WAD: {}", e))?;
    if h != m.patched_wad_sha {
        return Err("WAD: контрольная сумма результата не совпала".to_string());
    }
    win::move_replace(&tmp, &path)
}

fn patch_gpc(game_dir: &Path, m: &Manifest, files: &[GpcFile]) -> Result<(), String> {
    let dir = game_dir.join(SCRIPTS);
    fs::create_dir_all(&dir).map_err(|e| format!("создание {}: {}", dir.display(), e))?;
    let by_name: HashMap<&str, &GpcFile> =
        files.iter().map(|f| (f.name.as_str(), f)).collect();
    for meta in &m.gpc {
        let Some(f) = by_name.get(meta.name.as_str()) else {
            return Err(format!("в патче нет {}", meta.name));
        };
        if f.data.len() as u64 != meta.size {
            return Err(format!("{}: неверный размер", meta.name));
        }
        if sha256::hash_bytes(f.data) != meta.sha {
            return Err(format!("{}: неверный хэш в патче", meta.name));
        }
        let path = dir.join(&meta.name);
        write_atomic(&path, f.data)?;
        let h = sha256::hash_file(&path)
            .map_err(|e| format!("проверка {}: {}", path.display(), e))?;
        if h != meta.sha {
            return Err(format!("{}: записанный файл не совпал по хэшу", meta.name));
        }
    }
    Ok(())
}

pub fn install(
    game_dir: &Path,
    m: &Manifest,
    exe_patch: &ExePatch,
    wad_patch: &WadPatch,
    gpc_patch: &[GpcFile],
) -> Result<PathBuf, String> {
    if !game::looks_like_game(game_dir) {
        return Err(format!(
            "в {} нет Gunpoint.exe и Gunpoint.wad",
            game_dir.display()
        ));
    }
    let state = game::detect_state(game_dir, m);
    match state {
        State::Original => {}
        State::Patched => {
            return Err("Игра уже пропатчена. Используйте «Восстановить оригинал».".to_string())
        }
        State::Mixed => {
            return Err(
                "Файлы игры частично пропатчены. Восстановите оригинал из резервной копии или через Steam → «Проверить целостность файлов»."
                    .to_string(),
            )
        }
        State::Unknown => {
            return Err(
                "Версия игры не совпадает с поддерживаемой (Steam 1.82). Патч не применён."
                    .to_string(),
            )
        }
    }
    ensure_writable(&game_dir.join(EXE))?;
    ensure_writable(&game_dir.join(WAD))?;

    println!("[1/4] Резервная копия…");
    let names: Vec<String> = m.gpc.iter().map(|g| g.name.clone()).collect();
    let backup = make_backup(game_dir, &names)?;
    println!("      {}", backup.display());

    println!("[2/4] Патч Gunpoint.exe…");
    patch_exe(game_dir, m, exe_patch)?;

    println!("[3/4] Патч Gunpoint.wad (может занять минуту)…");
    patch_wad(game_dir, m, wad_patch)?;

    println!("[4/4] Запись 44 файлов .gpc…");
    patch_gpc(game_dir, m, gpc_patch)?;

    println!("Проверка результата…");
    let final_state = game::detect_state(game_dir, m);
    if final_state != State::Patched {
        return Err(format!(
            "после установки состояние: {:?}. Восстановите оригинал из {}",
            final_state,
            backup.display()
        ));
    }
    Ok(backup)
}

fn read_backup_game_dir(backup: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(backup.join("backup_info.txt")).ok()?;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("game_dir=") {
            return Some(PathBuf::from(v.trim()));
        }
    }
    None
}

pub fn backups(hint: Option<&Path>) -> Vec<PathBuf> {
    let root = backup_root();
    let Ok(rd) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("backup-"))
                .unwrap_or(false)
        })
        .collect();
    dirs.sort();
    dirs.reverse();
    if let Some(h) = hint {
        let hs = h.to_string_lossy().to_lowercase();
        dirs.retain(|d| {
            read_backup_game_dir(d)
                .map(|g| g.to_string_lossy().to_lowercase() == hs)
                .unwrap_or(false)
        });
    }
    dirs
}

pub fn restore(hint: Option<&Path>) -> Result<(PathBuf, PathBuf), String> {
    let list = backups(hint);
    let Some(backup) = list.first() else {
        return Err(
            "Резервная копия не найдена. Восстановите игру через Steam → Свойства → «Проверить целостность файлов»."
                .to_string(),
        );
    };
    let game_dir = hint
        .map(|p| p.to_path_buf())
        .or_else(|| read_backup_game_dir(backup))
        .ok_or_else(|| "в резервной копии нет пути к игре".to_string())?;
    if !game::looks_like_game(&game_dir) {
        return Err(format!(
            "в {} нет Gunpoint.exe/Gunpoint.wad",
            game_dir.display()
        ));
    }
    ensure_writable(&game_dir.join(EXE))?;
    ensure_writable(&game_dir.join(WAD))?;
    for name in [EXE, WAD] {
        let src = backup.join(name);
        let tmp = temp_path(&game_dir.join(name));
        fs::copy(&src, &tmp).map_err(|e| format!("восстановление {}: {}", src.display(), e))?;
        win::move_replace(&tmp, &game_dir.join(name))?;
    }
    let scripts = backup.join(SCRIPTS);
    if let Ok(rd) = fs::read_dir(&scripts) {
        fs::create_dir_all(game_dir.join(SCRIPTS))
            .map_err(|e| format!("создание Scripts: {}", e))?;
        for e in rd.filter_map(|e| e.ok()) {
            let src = e.path();
            if !src.is_file() {
                continue;
            }
            let name = e.file_name();
            let dst = game_dir.join(SCRIPTS).join(&name);
            let tmp = temp_path(&dst);
            fs::copy(&src, &tmp)
                .map_err(|e| format!("восстановление {}: {}", src.display(), e))?;
            win::move_replace(&tmp, &dst)?;
        }
    }
    Ok((game_dir, backup.clone()))
}

pub fn verify(game_dir: &Path, m: &Manifest) -> Result<(State, usize, usize), String> {
    let state = game::detect_state(game_dir, m);
    let mut ok = 0usize;
    let mut bad = 0usize;
    for meta in &m.gpc {
        let p = game_dir.join(SCRIPTS).join(&meta.name);
        let good = fs::metadata(&p)
            .map(|md| md.len() == meta.size)
            .unwrap_or(false)
            && sha256::hash_file(&p)
                .map(|h| h == meta.sha)
                .unwrap_or(false);
        if good {
            ok += 1;
        } else {
            bad += 1;
        }
    }
    Ok((state, ok, bad))
}
