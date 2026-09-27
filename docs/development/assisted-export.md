# Assisted Telegram export

`tgsum-i1w.1`, реализация 2026-09-27. Контракт опирается на
[исследование Desktop v7.2.9](../research/telegram-assisted-export-2026-09-27.md).

## Пользовательский путь

1. Подключить выбранный разговор из существующего Telegram JSON к Project.
2. В карточке источника открыть «Обновить через Telegram Desktop» и выбрать
   назначенную папку. TGSUM сохраняет её отдельно от последнего JSON.
3. При желании выбрать установленный **исполняемый файл** Desktop и нажать
   «Открыть выбранный клиент». Можно открыть клиент самостоятельно.
4. В Desktop выбрать нужный аккаунт и чат → ⋮ → экспорт истории → JSON,
   указать папку и дождаться завершения. Запрос подтверждения/ожидания выполняет
   пользователь в Telegram. TGSUM не пытается его обойти.
5. Выбрать фактический готовый `result.json`, подтвердить указанные аккаунт/чат
   и завершение экспорта, затем импортировать. Непустая папка может получить
   новый `ChatExport_*`, поэтому TGSUM не угадывает последний файл обходом папок.

Запуск возвращает `needs_user_action`; он не изменяет Project/snapshot и не
доказывает вход, выбранный чат, завершение или полноту архива. После повторного
открытия Project папка/клиент сохранены, подтверждение и неподтверждённый файл
сброшены. Ручные «Обновить из архива» и «Изменить файл…» сохранены.

## Границы реализации

- `Project` schema **8** хранит приватный `assisted_exports[source_id]` с
  `directory` и опциональным `client`. Schema 1–7 мигрирует в памяти; старые
  ревизии не переписываются. Настройки не входят в context bundle.
- Настройки разрешены только подключённому `telegram_json`. Смена scope или
  connector и отключение источника удаляют план. Перемещение JSON в том же scope
  сохраняет план. Все изменения проходят revision CAS и инвалидируют Review.
- `ProjectStore::import_assisted_export` проверяет подтверждение, revision,
  источник/план и абсолютный путь **до открытия reader**. Затем проверяет JSON
  до EOF и native chat ID, сохраняет snapshot и одним CAS обновляет JSON reference
  вместе с snapshot reference. Локальная метка аккаунта не аутентифицируется
  Telegram JSON; её подтверждает человек.
- Ошибка формата/scope, отмена чтения и конфликт версии сохраняют прежние
  source references и analysis baseline. После гонки CAS или поздней отмены
  может остаться непривязанный immutable snapshot; автоматического GC здесь нет.
- Импорт не запускает анализ и не обещает completeness/deletions. Вложения
  остаются отдельным явным выбором в Privacy; JSON не доказывает готовность media.
- Перед импортом UI требует сохранить черновик выбора сообщений/вложений.
  Изменения папки/клиента сохраняются без сброса этого черновика.

### Запуск клиента

Native picker выбирает один путь; нет PATH discovery, URI handler, shell,
profile flags, chat ID или содержимого архива в аргументах. Проверяются обычный
файл и заголовок ELF / PE `.exe` / Mach-O; проверка **не удостоверяет издателя**.
Клиент получает своё обычное окружение, рабочую папку executable и закрытый
stdin/stdout/stderr. Отдельный reaper ожидает процесс; отмена TGSUM не убивает
пользовательский Desktop. Сам Desktop после запуска может обращаться в сеть.

TGSUM не ищет/не читает `tdata`, session DB, cookies, память процесса или
messenger credentials. Он читает только выбранный executable для проверки
заголовка, выбранный JSON и явно разрешённые позднее вложения.

**Сейчас выбранный `.app`-каталог, Flatpak launcher, shell/batch/desktop shortcut
не запускается этим методом.** Пользователь открывает пакет самостоятельно и
пользуется тем же импортом. Исследованный macOS `NSWorkspace` launcher относится
к дальнейшему OS adapter; наличие Mach-O ветки не квалифицирует macOS сборку.
Это ограничение явно показано в UI. Автоклики, завершение по сигналам клиента,
watcher и расписание — отдельные задачи `i1w.2–6`/`l4c`.

## Проверки

- `core/tests/assisted.rs`: сохранение/смена namespace, schema 7 migration,
  отказ до открытия unconfirmed файла, nested export reference, неверный чат,
  truncated/trailing JSON, stale revision, отмена при чтении, concurrent edit.
- `src-tauri/tests/commands/assisted.rs`: реальные IPC payloads, нативный
  самостоятельно скомпилированный fake client с пробелами в пути (Linux также
  `; $literal`), ноль аргументов, `needs_user_action` без изменения Project,
  подтверждённый импорт, конфликт повторного запроса, отсутствие private paths
  и настроек acquisition в bundle Review.
- `scripts/ui-assisted-smoke.py`: настоящее окно Tauri в пустом изолированном
  XDG profile с `analysis-fixtures`, native GTK pickers через AT-SPI только
  собственного test PID, fake launch, wrong-chat failure, неподдержанный launcher
  и ручной fallback, nested JSON import, повторное открытие Project, забывание
  клиента. Сохраняет screenshot в собственном `/tmp/tgsum-assisted-ui-*`.

Тестовый клиент — `src-tauri/tests/fixtures/assisted-client.rs`; сетевого кода,
Telegram и профилей в нём нет. Реальные аккаунты/клиенты не используются.
Эти проверки квалифицируют локальные контракты и Linux UI. Реальный клиент,
Windows/macOS GUI и unattended export остаются непроверенными; `tgsum-yu3`
сохраняет пользовательский PoC. Статус автоматического connector в registry
остаётся `research`, без включения automation операций.
