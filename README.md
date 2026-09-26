<div align="center">

```
████████╗ ██████╗ ███████╗██╗   ██╗███╗   ███╗
╚══██╔══╝██╔════╝ ██╔════╝██║   ██║████╗ ████║
   ██║   ██║  ███╗███████╗██║   ██║██╔████╔██║
   ██║   ██║   ██║╚════██║██║   ██║██║╚██╔╝██║
   ██║   ╚██████╔╝███████║╚██████╔╝██║ ╚═╝ ██║
   ╚═╝    ╚═════╝ ╚══════╝ ╚═════╝ ╚═╝     ╚═╝
```

**Telegram-выгрузка → чистые `.md` файлы под чат с ИИ.**
Десктоп-приложение для macOS, Windows и Linux. Локально: без API, без сети, без облака.

[![release](https://img.shields.io/github/v/release/Roflochinsky/tgsum?include_prereleases)](https://github.com/Roflochinsky/tgsum/releases)
![rust](https://img.shields.io/badge/core-Rust-dea584)
![tauri](https://img.shields.io/badge/app-Tauri%202-24c8db)
![license](https://img.shields.io/badge/license-MIT-blue)

<img src="https://raw.githubusercontent.com/Roflochinsky/tgsum/main/docs/screenshots/start.jpg" alt="Первый экран: ночное небо из мазков, неоновая подпись tgsum" width="760">

</div>

---

## Что это

`tgsum` берёт экспорт Telegram Desktop (огромный `result.json`, до пары ГБ) и даёт выбрать нужные
чаты и форум-топики. Каждый из них сохраняется в аккуратный, экономный по токенам Markdown,
который можно вставить в чат с Claude или ChatGPT и попросить саммари.

Сам инструмент **ничего не суммаризирует и никуда не отправляет**: он только вытаскивает
и причёсывает данные. Анализ делает ИИ-чат на твоей стороне.

## Возможности

- 🖱 **Перетащил, выбрал, сохранил.** Три шага, без терминала и флагов.
- 🔎 **Поиск по чатам и топикам**, фильтры (личные, группы, каналы, форумы), сортировка, мультивыбор.
- ⚡ **Потоковое чтение.** Движок на Rust обрабатывает по одному чату;
  память зависит от размера самого большого чата. [Воспроизводимые замеры](docs/development/archive-core.md).
- 🧵 **Форум-топики.** Супергруппы раскладываются по топикам (General и остальные).
- ✂️ **Автонарезка на части** под лимит контекста (30k–150k токенов или без нарезки) **без потерь**:
  у каждой части своя шапка.
- 🧹 **Чистый формат.** Спикеры, реплаи, даты; служебные сообщения и реакции вырезаны.
- 🌌 **Ночь в духе Ван Гога.** Фон — живопись, которая пишется на глазах из тысяч мазков: вихри,
  звёзды, луна, кипарис. Неоновая подпись, заголовки Literata, текст Onest, код JetBrains Mono
  (шрифты встроены в приложение).
- 🐧 **Omarchy / Hyprland.** Нативный Wayland, без лишней рамки в тайловом режиме; по желанию —
  цвета текущей темы Omarchy вместо картины.
- 🔒 **Полностью локально.** Ни сети, ни ключей, ни телеметрии.

## Установка

### Одной командой

Как `npm install -g`: скрипт сам ставит всё нужное (WebKitGTK и компилятор через pacman, apt, dnf
или zypper, Rust, если его нет), собирает tgsum через `cargo install` и открывает его. Linux и macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/Roflochinsky/tgsum/main/install.sh | sh
```

Для системных пакетов спросит пароль sudo. Сборка занимает несколько минут, дальше tgsum открывается
из лаунчера (в Omarchy — **Super + Space**). Обновить: та же команда, удалить: `cargo uninstall tgsum`.
Самая свежая версия из репозитория: `curl … | TGSUM_GIT=1 sh`.

### Через cargo вручную

Если Rust уже стоит, а зависимости хочется поставить самому:

```bash
cargo install tgsum --locked
```

На Linux перед этим нужен WebKitGTK (движок, которым приложение рисует окно):

| Система | Команда |
|---|---|
| Omarchy / Arch | `sudo pacman -S --needed base-devel webkit2gtk-4.1` |
| Debian / Ubuntu | `sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev` |
| Другие | [зависимости Tauri для Linux](https://v2.tauri.app/start/prerequisites/#linux) |

На Linux первый запуск (`tgsum` в терминале) добавляет приложение в лаунчер. Свежая версия
прямо из репозитория: `cargo install --git https://github.com/Roflochinsky/tgsum --locked`.

### Готовые установщики

Без Rust и без сборки: скачай файл для своей системы со страницы [**Releases**](https://github.com/Roflochinsky/tgsum/releases).

| Система | Файл |
|---|---|
| macOS (Apple Silicon: M1…M4) | `tgsum_…_aarch64.dmg` |
| macOS (Intel) | `tgsum_…_x64.dmg` |
| Windows 10/11 | `tgsum_…_x64-setup.exe` (или `.msi`) |
| Omarchy / Arch Linux | `tgsum-…-x86_64.pkg.tar.zst`, ставится командой `sudo pacman -U ./tgsum-*.pkg.tar.zst` |
| Другие Linux | `.deb` (Debian/Ubuntu), `.rpm` (Fedora) или `.AppImage` |

> **Первый запуск.** Сборки пока не подписаны сертификатом разработчика, поэтому система переспросит:
> - **macOS:** открой приложение через правый клик → **Открыть**. Если не помогло:
>   **Системные настройки → Конфиденциальность и безопасность → «Всё равно открыть»**.
> - **Windows:** в окне SmartScreen нажми **Подробнее → Выполнить в любом случае**.

> До v0.2 tgsum был консольным мастером на Node и ставился через npm (`@roflochinsky/tgsum`).
> Теперь это приложение на Rust: вместо `npm install -g` — `cargo install tgsum`. npm-пакет больше не обновляется.

### Omarchy

<img src="https://raw.githubusercontent.com/Roflochinsky/tgsum/main/docs/screenshots/done.jpg" alt="Готово: список сохранённых файлов" width="560" align="right">

Подходит любой способ выше: скрипт, `cargo install` или пакет `.pkg.tar.zst`. tgsum появится в лаунчере
(**Super + Space**). В Omarchy приложение:

- в Hyprland открывается без системной рамки, как остальные окна (закрыть — **Super + W**);
- работает нативно на Wayland.

AppImage на Arch с Hyprland может открыться пустым окном (он везёт свои графические библиотеки),
поэтому на Omarchy ставь через cargo или пакетом. Переменные окружения: `TGSUM_THEME=omarchy` — цвета
текущей темы Omarchy вместо картины (и смена темы на лету), `TGSUM_DECORATIONS=1` — вернуть системную
рамку окна.

<br clear="right">

## Как пользоваться

<img src="https://raw.githubusercontent.com/Roflochinsky/tgsum/main/docs/screenshots/select.jpg" alt="Выбор чатов и топиков" width="560" align="right">

1. **Файл.** Перетащи `result.json` (или всю папку выгрузки) в окно. Можно и кнопкой.
2. **Чаты.** Найди и отметь нужные чаты и топики.
3. **Сохранение.** Выбери папку и размер файла, нажми «Сохранить», потом «Открыть папку».

Готовые `.md` вставляй в чат с ИИ.

<br clear="right">

### Как получить result.json

Telegram Desktop → ⚙ **Настройки** → **Продвинутые настройки** → **Экспорт данных из Telegram**
(*Export Telegram data*) → формат **Машиночитаемый JSON** (медиа можно не включать) → **Экспортировать**.
В папке экспорта появится `result.json`.

Parser в текущих исходниках также поддерживает JSON отдельного чата:
меню чата **⋮ → Экспорт истории чата → JSON**. Затем используется тот же выбор
файла и сохранение Markdown. Режим описан Telegram в
[инструкции по экспорту](https://telegram.org/blog/export-and-more).

## Пример вывода

```markdown
# Чат: Команда / Топик: Релизы
# Период: 2026-06-18 — 2026-06-20 | сообщений: 142 | участники: Алиса, Боб, Вера

## 2026-06-20
[14:02] Алиса: когда катим релиз?
[14:03] Боб: давай в 15:00, сначала смёржь PR #210
[14:05] Вера ↳ Боб «смёржь PR #210»: смёржила
[14:06] Боб: [photo] логи деплоя
[14:40] Алиса: [voice 0:42]
```

## Что сохраняется, а что нет

| Сохраняется | Вырезается / сворачивается |
|---|---|
| Текст сообщений, имена авторов | Служебные сообщения (вошёл/закрепил/звонок) |
| Реплаи (`↳ Имя «цитата»`) | Реакции |
| Даты и время | Стикеры → `[sticker 👍]` |
| Ссылки, упоминания, подписи к фото | Фото/видео/голос → `[photo]` / `[voice 0:42]` / `[file: …]` |

> Голосовые и медиа сворачиваются в маркеры, потому что в выгрузке Telegram **нет
> расшифровок**, только файлы. Подписи к медиа (текст) сохраняются.

## Как это работает

В offline core добавлены snapshots одного выбранного чата, повторный diff и
контракт событий Bridge. Project UI позволяет выбрать scope и подготовить
очищенный context через Review → Export only. Автоматическое управление Telegram
ещё не подключено. [Контракт архива](docs/development/archive-core.md),
[context bundle и ограничения](docs/development/bundles.md).

Два потоковых прохода по `result.json`. Первый строит лёгкий индекс (чаты, топики, счётчики, даты)
и держит в памяти только один чат за раз, без текстов сообщений. Второй вытаскивает выбранное
и останавливается, как только нашёл всё нужное. Большой чат режется на `имя.part-1.md`,
`part-2.md`… **без потери сообщений**: каждая часть несёт ту же шапку, ни одно сообщение не выпадает.

```
core/          tgsum-core — Rust-библиотека: потоковый парсер, топики, Markdown, запись файлов
runner/        tgsum-runner — изолированный процессный runner (пока offline Linux profile)
src-tauri/     приложение tgsum на Tauri 2: окно, команды, диалоги, прогресс и отмена
src-tauri/ui/  интерфейс: HTML + CSS + JS без сборки и без npm
packaging/     пакет для Arch/Omarchy (PKGBUILD)
install.sh     установка одной командой: зависимости, Rust, cargo install
```

[Контракт runner, ограничения и отдельная проверка Linux-изоляции](docs/development/runner.md).

## Сборка из исходников

Установи [Rust через rustup](https://rustup.rs): версия для разработки и компоненты
выбираются из `rust-toolchain.toml`. На Linux ещё нужны системные библиотеки WebKitGTK:

```bash
# Debian/Ubuntu
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libxdo-dev libssl-dev
# Arch/Omarchy
sudo pacman -S --needed rust webkit2gtk-4.1 gtk3 librsvg xdg-utils
```

```bash
cargo run -p tgsum                       # запустить приложение
bash scripts/check.sh                    # полный набор проверок Rust

cargo install --path src-tauri --locked  # поставить собранное из этой папки
packaging/arch/build-local.sh -si        # собрать и поставить пакет для Arch/Omarchy

cargo install tauri-cli --version "^2" --locked
cargo tauri build                        # установщики → target/release/bundle/
```

Релизы собирает GitHub Actions: пуш тега `v*` публикует черновик релиза с установщиками
для всех платформ (`.github/workflows/release.yml`). Если в секретах репозитория есть
`CARGO_REGISTRY_TOKEN` (токен crates.io), тот же тег публикует и крейты для `cargo install tgsum`.

## Проверки для разработки

После небольшого изменения агент запускает быструю проверку, исправляет ошибки
и запускает её снова. Перед завершением задачи — полный набор:

```bash
bash scripts/check.sh quick           # типы, владение, заимствования, API
bash scripts/check.sh                 # форматирование → компилятор → Clippy → тесты
```

Скрипт показывает выполняемую команду и исходную диагностику, останавливается
на первой ошибке с ненулевым кодом выхода. `PASS` появляется только после всех
проверок выбранного режима. Файлы автоматически не исправляются.

| Проверка | Что ловит |
|---|---|
| `cargo fmt --all --check` | Отклонения от единого форматирования Rust |
| `cargo check` | Ошибки типов, времени жизни, владения, заимствований, отсутствующие методы и импорты |
| `cargo clippy … -- -D warnings` | Подозрительные конструкции, лишние операции, предупреждения компилятора; любое предупреждение проваливает проверку |
| `cargo test` | Ошибки поведения: тесты ядра, команд Tauri, интеграционные тесты и примеры из документации |

Компилятор уже выполняет роль типизатора Rust. `check` и Clippy охватывают оба
пакета, все цели (включая тесты и примеры) и все features на текущей ОС.
Тесты запускаются с обычным выбором целей Cargo, чтобы сохранить doctests.
Все команды сборки используют `--locked`: случайное изменение зависимостей
не переписывает `Cargo.lock` незаметно.

В обоих пакетах также включены ошибки на `unsafe`-код, проигнорированные значения
с `#[must_use]` (включая `Result`), `dbg!`, `todo!` и `unimplemented!`.
Правила заданы в `[workspace.lints]` корневого `Cargo.toml` и наследуются пакетами.
Обычные `unwrap`/`expect` не запрещены глобально: в тестах они уместны;
ошибки пользовательского ввода и файловых операций следует возвращать через `Result`.

Дополнительные режимы:

```bash
bash scripts/check.sh core            # форматирование всего проекта + проверки только ядра, без GTK/WebKit
bash scripts/check.sh lint            # форматирование + Clippy, как в CI
bash scripts/check.sh test            # тесты всего проекта, как в CI
cargo test --locked -p tgsum-core --test parse   # один набор тестов при разработке
cargo fmt --all                       # исправить форматирование
```

Нужны Bash (на Windows — Git Bash) и rustup. При первом запуске rustup установит
версию Rust, `rustfmt` и Clippy из `rust-toolchain.toml`; этот файл задаёт версию
для разработки и CI, а `rust-version` в Cargo.toml — заявленный минимальный Rust.
Первая сборка Tauri скачивает и компилирует зависимости, последующие используют кеш.
Обновление toolchain делается отдельной правкой с полным прогоном проверок.

| Если проверка упала | Что делать |
|---|---|
| Разница форматирования | `cargo fmt --all`, затем повторить проверку |
| Ошибка вроде `E0308`, `E0382`, `E0502` | Прочитать сообщение и `rustc --explain E0382` (подставить свой код), исправить типы или владение |
| Предупреждение Clippy | Исправить причину по диагностике; точечное исключение требует объяснения в коде |
| Нет `rustfmt` или Clippy | `rustup component add rustfmt clippy` |
| `pkg-config` не находит GTK/WebKit | Установить библиотеки из раздела сборки; режим `core` проверяет только ядро |
| Cargo требует изменить lockfile | При намеренном изменении зависимостей обновить и проверить diff `Cargo.lock`, затем снова запустить проверки с `--locked` |

CI запускается на каждый push и pull request и использует тот же скрипт:
lint на Linux, тесты на Linux/macOS/Windows. ShellCheck отдельно проверяет скрипты:
`shellcheck -s sh install.sh` и `shellcheck packaging/arch/build-local.sh scripts/check.sh`
(локально устанавливается через `apt install shellcheck`, `pacman -S shellcheck` или `brew install shellcheck`).
Отдельная задача собирает пакет Arch/Omarchy системным Rust. Локальный успех
проверяет текущую ОС; результаты остальных платформ видны в GitHub Actions.
Инструкция для агента — в [AGENTS.md](AGENTS.md). Изменения интерфейса дополнительно
проверяются запуском приложения: эти команды не проверяют поведение HTML/CSS/JS.

Справка: [Cargo check](https://doc.rust-lang.org/cargo/commands/cargo-check.html),
[Clippy](https://doc.rust-lang.org/clippy/usage.html),
[наследование lint-правил](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-lints-table).

## Работа агента и память

[AGENTS.md](AGENTS.md) задаёт порядок работы: восстановить контекст, выбрать
задачу в **Beads**, выполнить изменения, проверить результат и сохранить
состояние. Обоснование настроек SOL 6, рекомендации OpenAI и их ограничения —
в [исследовании](docs/research/openai-sol6-agent-workflow.md).

Задачи, решения по ходу работы и следующий шаг хранятся в Beads; долговечные
выводы — через `bd remember`. Спецификации и исследования остаются в `docs/`.
GitHub Issues можно связывать с задачами Beads для внешних обращений.
`/goal` удерживает цель текущего треда, а Beads помогает продолжить работу в другом.

```bash
bd prime                             # правила и сохранённые выводы
bd list --status in_progress          # незавершённая работа
bd ready                             # доступные задачи
bd show <id>                         # критерии, решения, следующий шаг
bd memories <ключевое-слово>           # найти сохранённое знание
```

База `.beads/` хранится локально и не включается в коммиты исходников.
Для её синхронизации используется отдельный Dolt remote в этом GitHub-репозитории:

```bash
bd dolt commit -m "Update project memory"
bd dolt push
```

После обычного `git clone`, при установленном `bd`, восстанови базу **из корня
клона**, до создания новых задач:

```bash
bd init --non-interactive --skip-agents --skip-hooks --setup-exclude \
  --remote git+https://github.com/Roflochinsky/tgsum.git
```

Для уже инициализированной базы используй `bd dolt pull`; сначала сохрани локальные
изменения через `bd dolt commit`. Ошибки синхронизации нужно устранить и проверить,
прежде чем считать память сохранённой на удалённой стороне.

## Направление развития

Текущая версия остаётся локальным импортёром полного экспорта Telegram Desktop.
Следующая архитектура — подготовка контекста из нескольких мессенджеров с явным
scope, privacy review и отдельным запуском выбранного AI-инструмента.

- [Архитектурное решение](docs/adr/0001-local-context-gateway.md) и
  [проект следующего pipeline](docs/specs/context-gateway.md).
- [Проверка платформ](docs/research/connector-support-2026-09-26.md) и
  [варианты автоматизации Telegram Desktop](docs/research/telegram-export-automation.md).
- [Реестр способов подключения](docs/connectors/registry.json) и
  [обязательный цикл проверки API/условий](docs/connectors/review-policy.md).
- [Матрица архитектуры коннекторов](docs/connectors/architecture.md),
  [roadmap](docs/plans/context-gateway-roadmap.md) и
  [граница ответственности продукта](docs/adr/0002-user-controlled-content.md).

Принцип интеграций: TGSUM не читает базы сессий и credentials мессенджеров
(`tdata`, session cookies, память клиента). Official Client Bridge получает
файлы штатного экспорта; API-интеграции используют специально выданные им токены.

Эти документы фиксируют направление и условия поддержки; новые коннекторы,
автовыгрузка и запуск агентов ещё не реализованы.

## Лицензия

MIT. Фон рисует генеративный движок живописи с сайта автора (`src-tauri/ui/paint.js`).
Иконка — фрагмент картины Винсента ван Гога «Пшеничное поле с кипарисами» (1889, общественное
достояние). Шрифты Literata, Onest и JetBrains Mono — SIL Open Font License 1.1
(`src-tauri/ui/fonts/`); подпись «tgsum» — контуры букв шрифта Caveat (OFL).
