# Gunpoint RU Patcher (Rust)

Нативный патчер русской локализации Gunpoint 1.82 (Steam). Без внешних
крейтов, без сети, без упаковщиков.

## Сборка

```powershell
cd patcher
cargo test
cargo build --release
```

Результат: `target\release\gunpoint_ru_patcher.exe`.

В release автоматически встраивается manifest `requireAdministrator`
(запись в `Program Files (x86)\Steam\...`). Для отладочной сборки без UAC:

```powershell
$env:GUNPOINT_RU_NO_MANIFEST="1"
cargo build --release
```

## Режимы

```text
GunpointRU_Patcher.exe                 интерактивное меню
GunpointRU_Patcher.exe --install       установить
GunpointRU_Patcher.exe --restore       восстановить оригинал
GunpointRU_Patcher.exe --verify        проверить файлы
  --game-dir <путь>                    указать папку игры
  --yes                                без подтверждения
```

## Что внутри

- `src/formats.rs` — разбор компактных бинарных патчей.
- `src/game.rs` — поиск Steam/Gunpoint, состояние файлов.
- `src/patcher.rs` — установка, бэкап, восстановление, проверка.
- `src/sha256.rs` — SHA-256 без зависимостей.
- `src/win.rs` — реестр, выбор папки, атомарная замена.
- `assets/` — данные патча (см. `assets/README.md`).

Полное техническое описание: `..\docs\PATCH_TECHNICAL_GUIDE.md`.
