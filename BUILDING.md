# Сборка

## Что нужно

- Windows 10/11.
- Rust stable с таргетом `x86_64-pc-windows-gnu` и MSYS2 MinGW
  (`C:\msys64\mingw64\bin` в PATH).
- Для запуска release-сборки — права администратора (UAC-манифест).

Python и игровые файлы для сборки **не нужны**: данные патча уже лежат
в `patcher/assets/`.

## Патчер

```powershell
cd patcher
cargo test
cargo build --release
```

Результат:

```text
patcher\target\release\gunpoint_ru_patcher.exe
```

В release автоматически встраивается manifest `requireAdministrator`.
Для отладочной сборки без UAC:

```powershell
$env:GUNPOINT_RU_NO_MANIFEST="1"
cargo build --release
```

## Данные патча

`patcher/assets/` уже содержит:

| Файл | Что внутри |
|---|---|
| `exe_patch.bin` | точечные изменения `Gunpoint.exe` + русские строки |
| `wad_patch.bin` | изменения 12 шрифтов |
| `gpc_patch.bin` | 44 переведённых `.gpc` |
| `manifest.bin` | контрольные суммы и список файлов |

Это не полные файлы игры: для `.phyre` хранятся только изменённые
диапазоны, для `.fnt` — наша изменённая XML-метаинформация.

Перегенерация assets выполняется в исходном рабочем каталоге перевода
и в публичный репозиторий не входит. Для обычной сборки патчера она
не требуется.

## Проверка на копии игры

Не запускайте install на рабочей установке во время разработки.
Сделайте копию `Gunpoint.exe`, `Gunpoint.wad` и `Scripts` и используйте:

```powershell
gunpoint_ru_patcher.exe --game-dir "C:\path\to\Gunpoint_copy" --verify
gunpoint_ru_patcher.exe --game-dir "C:\path\to\Gunpoint_copy" --install --yes
gunpoint_ru_patcher.exe --game-dir "C:\path\to\Gunpoint_copy" --restore --yes
```

Ожидаемые хэши после install/restore — в
`docs/PATCH_TECHNICAL_GUIDE.md`, раздел 0.
