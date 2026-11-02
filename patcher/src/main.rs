//! Gunpoint 1.82 Russian localization patcher.
//! Native, no external crates, no network, no packing.

mod formats;
mod game;
mod patcher;
mod sha256;
mod win;

use formats::{ExePatch, GpcFile, Manifest, WadPatch};
use game::State;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Interactive,
    Install,
    Restore,
    Verify,
}

struct Opts {
    game_dir: Option<PathBuf>,
    mode: Mode,
    yes: bool,
}

fn main() {
    win::init_console();
    let opts = parse_args();
    if let Err(e) = run(opts) {
        eprintln!("\nОШИБКА: {}", e);
        pause();
    }
}

fn parse_args() -> Opts {
    let mut o = Opts {
        game_dir: None,
        mode: Mode::Interactive,
        yes: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--game-dir" => {
                o.game_dir = it.next().map(PathBuf::from);
            }
            "--install" => o.mode = Mode::Install,
            "--restore" => o.mode = Mode::Restore,
            "--verify" => o.mode = Mode::Verify,
            "--yes" | "-y" => o.yes = true,
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => {
                eprintln!("Неизвестный аргумент: {}", other);
                std::process::exit(2);
            }
        }
    }
    o
}

fn print_help() {
    println!("Gunpoint RU Patcher");
    println!("  --game-dir <путь>   папка Gunpoint (иначе автопоиск)");
    println!("  --install           установить локализацию");
    println!("  --restore           восстановить оригинал из резервной копии");
    println!("  --verify            проверить состояние файлов");
    println!("  --yes               не спрашивать подтверждение");
    println!("  --help              эта справка");
    println!();
    println!("Без аргументов запускается интерактивное меню.");
}

fn run(opts: Opts) -> Result<(), String> {
    let m = formats::parse_manifest()?;
    let exe_p = formats::parse_exe_patch()?;
    let wad_p = formats::parse_wad_patch()?;
    let gpc_p = formats::parse_gpc_patch()?;

    println!("Gunpoint RU Patcher v{}", m.version);
    println!("========================================");

    match opts.mode {
        Mode::Restore => {
            let (game, backup) = patcher::restore(opts.game_dir.as_deref())?;
            println!("\nОригинал восстановлен.");
            println!("Игра:  {}", game.display());
            println!("Копия: {}", backup.display());
        }
        Mode::Verify => {
            let game = resolve_game(&opts)?;
            let (state, ok, bad) = patcher::verify(&game, &m)?;
            println!("\nИгра: {}", game.display());
            println!("Состояние: {}", state.ru());
            println!(".gpc: {} файлов совпадают, {} не совпадают", ok, bad);
        }
        Mode::Install => {
            let game = resolve_game(&opts)?;
            let state = game::detect_state(&game, &m);
            println!("\nИгра: {}", game.display());
            println!("Состояние: {}", state.ru());
            if state == State::Patched {
                println!("Игра уже пропатчена. Для переустановки сначала восстановите оригинал.");
                return Ok(());
            }
            if !opts.yes && !confirm("Установить русскую локализацию?") {
                return Ok(());
            }
            let backup = patcher::install(&game, &m, &exe_p, &wad_p, &gpc_p)?;
            println!("\nГотово. Резервная копия: {}", backup.display());
            println!("Запустите Gunpoint из Steam.");
        }
        Mode::Interactive => interactive(&opts, &m, &exe_p, &wad_p, &gpc_p)?,
    }
    Ok(())
}

fn resolve_game(opts: &Opts) -> Result<PathBuf, String> {
    if let Some(p) = &opts.game_dir {
        if !game::looks_like_game(p) {
            return Err(format!(
                "в {} нет Gunpoint.exe и Gunpoint.wad",
                p.display()
            ));
        }
        return Ok(p.clone());
    }
    let cands = game::candidates();
    if cands.is_empty() {
        println!("\nИгра не найдена автоматически.");
        println!("Выберите папку вручную (обычно ...\\steamapps\\common\\Gunpoint).");
        return manual_pick();
    }
    if cands.len() == 1 {
        return Ok(cands[0].clone());
    }
    println!("\nНайдено несколько папок:");
    for (i, p) in cands.iter().enumerate() {
        println!("  {}. {}", i + 1, p.display());
    }
    println!("  m. Выбрать вручную");
    loop {
        let s = prompt("Номер");
        if s.is_empty() {
            return Err(
                "не удалось прочитать выбор. Запустите из консоли или укажите --game-dir <путь>."
                    .to_string(),
            );
        }
        if s.eq_ignore_ascii_case("m") {
            return manual_pick();
        }
        if let Ok(n) = s.parse::<usize>() {
            if n >= 1 && n <= cands.len() {
                return Ok(cands[n - 1].clone());
            }
        }
        println!("Введите номер из списка или m.");
    }
}

fn manual_pick() -> Result<PathBuf, String> {
    match win::pick_folder("Выберите папку Gunpoint") {
        Some(p) if game::looks_like_game(&p) => Ok(p),
        Some(p) => Err(format!(
            "в выбранной папке нет Gunpoint.exe и Gunpoint.wad:\n{}",
            p.display()
        )),
        None => Err("папка не выбрана".to_string()),
    }
}

fn interactive(
    opts: &Opts,
    m: &Manifest,
    exe_p: &ExePatch,
    wad_p: &WadPatch,
    gpc_p: &[GpcFile],
) -> Result<(), String> {
    let mut game_dir = match resolve_game(opts) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("\n{}", e);
            pause();
            return Ok(());
        }
    };
    let mut state = game::detect_state(&game_dir, m);
    loop {
        println!("\nИгра: {}", game_dir.display());
        println!("Состояние: {}", state.ru());
        println!("----------------------------------------");
        println!("[1] Установить русскую локализацию");
        println!("[2] Восстановить оригинал");
        println!("[3] Выбрать папку вручную");
        println!("[4] Проверить файлы");
        println!("[0] Выход");
        let choice = prompt("Выбор");
        match choice.as_str() {
            "1" => {
                if state != State::Original {
                    println!(
                        "Установка возможна только на оригинальные файлы. Сначала восстановите оригинал (п. 2)."
                    );
                    continue;
                }
                if !confirm("Установить русскую локализацию?") {
                    continue;
                }
                match patcher::install(&game_dir, m, exe_p, wad_p, gpc_p) {
                    Ok(backup) => {
                        println!("\nГотово. Резервная копия: {}", backup.display());
                        println!("Запустите Gunpoint из Steam.");
                    }
                    Err(e) => {
                        eprintln!("\nОШИБКА: {}", e);
                        println!("Если файлы изменены частично, восстановите оригинал (п. 2).");
                    }
                }
                state = game::detect_state(&game_dir, m);
            }
            "2" => {
                if !confirm("Восстановить оригинальные файлы из резервной копии?") {
                    continue;
                }
                match patcher::restore(Some(&game_dir)) {
                    Ok((g, b)) => {
                        println!("\nОригинал восстановлен.");
                        println!("Игра:  {}", g.display());
                        println!("Копия: {}", b.display());
                    }
                    Err(e) => eprintln!("\nОШИБКА: {}", e),
                }
                state = game::detect_state(&game_dir, m);
            }
            "3" => match win::pick_folder("Выберите папку Gunpoint") {
                Some(p) if game::looks_like_game(&p) => {
                    game_dir = p;
                    state = game::detect_state(&game_dir, m);
                }
                Some(p) => println!("В {} нет Gunpoint.exe/Gunpoint.wad.", p.display()),
                None => {}
            },
            "4" => {
                let (st, ok, bad) = patcher::verify(&game_dir, m)?;
                println!("\nСостояние: {}", st.ru());
                println!(".gpc: {} совпадают, {} не совпадают", ok, bad);
                state = st;
            }
            "0" | "q" | "exit" | "" => break,
            _ => println!("Нет такого пункта."),
        }
    }
    Ok(())
}

fn confirm(prompt_text: &str) -> bool {
    print!("{} [Y/n]: ", prompt_text);
    io::stdout().flush().ok();
    let mut s = String::new();
    let n = io::stdin().read_line(&mut s).unwrap_or(0);
    if n == 0 {
        // EOF / no stdin: do not silently confirm.
        return false;
    }
    let s = s.trim().to_lowercase();
    s.is_empty() || s == "y" || s == "yes" || s == "д" || s == "да"
}

fn prompt(label: &str) -> String {
    print!("{}: ", label);
    io::stdout().flush().ok();
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
    s.trim().to_string()
}

fn pause() {
    print!("\nНажмите Enter, чтобы выйти…");
    io::stdout().flush().ok();
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
}
