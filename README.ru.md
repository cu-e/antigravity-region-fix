> **Предупреждение.** ПО предоставляется исключительно в исследовательских и диагностических целях, без каких-либо гарантий. Пользователь несёт полную ответственность за соблюдение правил сервиса и применимого законодательства.

# Antigravity Regional Fix

[English](README.md) · [MIT](LICENSE) · [Credits](THIRD_PARTY_NOTICES.md) · [Тестирование](TESTING.md)

Инструмент для устранения клиентских региональных ограничений в Antigravity (CLI, Desktop App, IDE).

## Компоненты

- **`pagy`** — wrapper для CLI `agy`. Который автоматически проверяет и применяет патч перед запуском, передавая все аргументы оригинальному бинарнику.
- **`antigravity-region-fix`** — утилита управления патчами: проверка статуса, патчинг и откат CLI, десктопного приложения и IDE.

## Установка

### Готовые бинарники

Скачайте релиз для вашей платформы со страницы [Releases](https://github.com/cu-e/antigravity-region-fix/releases):

- **Windows:** скачайте и запустите `antigravity-region-fix-windows-installer.exe` (установка в один клик) либо распакуйте архив и выполните:
  ```powershell
  .\install.ps1
  ```
- **Linux / macOS:** скачайте и распакуйте архив, затем запустите скрипт установки:
  ```sh
  sh install.sh
  ```
  Также в релизах доступны отдельные автономные бинарники `pagy` и `antigravity-region-fix`.

Каталоги установки по умолчанию:

- Linux / macOS: `~/.local/bin`
- Windows: `%LOCALAPPDATA%\Programs\AntigravityRegionFix\bin`

Для установки в произвольный каталог используйте параметр `--bin-dir`:

```sh
sh install.sh --bin-dir /path/to/bin
```

### Сборка из исходников

Требуется Rust 1.88+:

```sh
cargo build --release --locked --bins
./target/release/antigravity-region-fix install
```

В Windows:

```powershell
.\target\release\antigravity-region-fix.exe install
```

## Использование

### Запуск CLI

Используйте `pagy` вместо `agy`. Все флаги и аргументы передаются напрямую:

```sh
pagy
pagy --help
pagy models
pagy --mode plan --print "Hello"
```

### Управление патчами

Перед патчингом или откатом закройте соответствующие приложения и активные сессии CLI.

```sh
# Проверка состояния
antigravity-region-fix status

# Применение патчей
antigravity-region-fix patch cli
antigravity-region-fix patch app
antigravity-region-fix patch ide

# Восстановление оригиналов
antigravity-region-fix restore all
# или по отдельности: restore cli | app | ide

# Удаление
antigravity-region-fix uninstall
```

## Пути и переменные окружения

Стандартные пути определяются автоматически. При нестандартном расположении используйте переменные окружения или параметры CLI:

| Переменная       | Параметр CLI | Назначение                                           |
| ---------------- | ------------ | ---------------------------------------------------- |
| `PAGY_AGY`       | `--path-cli` | Путь к бинарнику `agy`                               |
| `PAGY_APP`       | `--path-app` | Путь к `resources/bin/language_server` приложения    |
| `PAGY_IDE`       | `--path-ide` | Путь к `resources/app/out/main.js` IDE               |
| `PAGY_STATE_DIR` | —            | Каталог состояния (блокировки, манифест, кэш)        |
| `PAGY_NO_CACHE`  | —            | Принудительная проверка без использования кэша (`1`) |

## Резервные копии и целостность

- Оригиналы сохраняются рядом с целевыми файлами с расширением `.agybak`.
- Контрольные суммы SHA-256 сохраняются в `.pagy.json` для надежного отката.
- В macOS автоматически выполняется ad-hoc подпись модифицированных бинарников и снятие атрибута карантина.

## Разработка

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked --bins
```
