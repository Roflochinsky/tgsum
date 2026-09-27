# Connector registry и проверка выпуска

`tgsum-99k`, 2026-09-27. Инвентарь:
[`registry.json`](../connectors/registry.json),
[`JSON Schema 2`](../connectors/registry.schema.json).
Rust API: [`core::registry`](../../core/src/registry/mod.rs).

## Три независимых факта

| Факт | Источник | Чего не доказывает |
| --- | --- | --- |
| `implemented` | Зарегистрированная в Rust фабрика, её `ConnectorDescriptor`, revision и список операций | Проверку конкретного установленного клиента/аккаунта |
| `qualified` | Запись проверки, точно совпадающая с implementation revision, format и при необходимости client version/OS | Поддержку за пределами указанного `qualifications.scope` |
| `unknown` | Для profile нет зарегистрированной реализации либо нет подходящей проверки/наблюдаемой версии | Невозможность будущей реализации |

`TechnicalSupport.state` обозначает уровень доказательств; отдельно всегда
доступны implementation, implemented operations, compatibility, qualifications
и review state. `qualified + format_contract` означает только проверенный
контракт формата. Это не проверка GUI, installer, полноценной истории, аккаунта
или platform terms. `synthetic_client` и `real_client` требуют точной версии
клиента и ОС; wildcard/«все будущие версии» отсутствует.

Сейчас зарегистрированы `telegram_full_export` и `telegram_single_export`:
оба используют настоящий `TelegramJson`, `0.2.0:canonical-2`,
`telegram_desktop_json`, `local_import`. Qualification фиксирует синтетический
format contract. Набор `capabilities` в inventory сохраняет исследовательское
описание платформы; runtime берёт возможности из **дескриптора реализации**.
Например, нормализатор выдаёт attachment references, а выбранные локальные файлы
позднее обрабатывает отдельный packager. Полнота конкретного snapshot остаётся
его собственным свойством.

`telegram_desktop_ui` относится к автоматическому acquisition driver. У него
пока нет фабрики/операций в loader. Реализованный
[assisted workflow](assisted-export.md) помогает человеку открыть выбранный
клиент и предоставить JSON; это не автоматический acquisition driver.
Аналогично наличие agent runner не включает API-коннектор мессенджера.

## Загрузка и версии

`load_importer(profile_id, ObservedVersion)` разрешает только явно
зарегистрированную в коде фабрику и совместимый формат. Unknown/unsupported
format отклоняется; затем сам parser валидирует фактический файл. Для архива
версия установленного клиента не требуется — критерий здесь формат данных.
`VersionRequirement` для будущего драйвера может закрепить точные пары
`client_version + os`; неизвестная версия не считается совместимой.

Inventory не регистрирует исполняемый код и не выдаёт credentials. Копирование
имени `telegram_json` в строку Discord не создаёт поддержку Discord.
Редактирование `enabled_operations` не включает код и не скрывает уже имеющуюся
реализацию от release review: валидатор требует совпадения с кодом.

`ai_policy`, review expiry и URL источника не участвуют в `load_importer`.
Loader не обращается к сети, не удаляет snapshot и не отключает импорт из-за
дат. `Registry::technical_support` соединяет research metadata с техническими
фактами для диагностики. При неподходящей revision/версии qualification не
переносится на новый код. Desktop-экран поддержки — следующий срез `2ty.2`.

## Локальная команда

Из корня репозитория:

```sh
cargo run --locked -p tgsum-core --bin connector-registry --
cargo run --locked -p tgsum-core --bin connector-registry -- --release
# Воспроизводимая проверка даты (не обновляет дату review):
cargo run --locked -p tgsum-core --bin connector-registry -- --release --today 2026-09-27
# Предстоящие/просроченные review; даты не изменяются:
cargo run --locked -p tgsum-core --bin connector-registry -- --reminders --within-days 14
```

`--repo DIR`, `--inventory FILE` позволяют выбрать локальный checkout/inventory;
FILE задаётся относительно DIR. Выход — JSON. Коды: `0` — проверка прошла,
`1` — ошибки в отчёте, `2` — ошибка чтения/структуры/аргументов. Без `--today`
используется текущая UTC-дата. CLI предназначен для checkout репозитория,
не запускается фоном установленным приложением.

`--reminders` возвращает envelope `as_of`, `within_days`, `reminders`,
`validation`. Внутри `validation` — тот же полный отчёт и те же exit codes;
`--release` по-прежнему проваливает просроченную shipping capability.
Горизонт — 0..365 дней (по умолчанию 14), граница включительна. Без `--reminders`
параметр `--within-days` отклоняется. Публичный `Registry::review_reminders`
возвращает owner/due/evidence и shipping-признак из compiled catalog. Даты или
сетевые источники не меняются. Порядок действий и расписание описаны в
[review cycle](../connectors/review-policy.md#напоминания-о-сроках).

Проверяются строгая schema/version и неизвестные поля, уникальность IDs/операций,
обязательные owner/dates, реальные календарные даты, cadence не более 30 дней
для network/client и 90 для file/local, ссылки HTTP(S), существование evidence
и tests/fixtures внутри repo, matching compiled implementation/revision,
границы qualification, отсутствие будущих review/qualification дат.

Development mode сообщает о просрочках предупреждениями. Release mode требует
актуального review для включённых shipping profiles. Не реализованные
research/candidate profiles остаются в отчёте с напоминанием и не превращаются
в доступные операции. Просроченный или отсутствующий review требует работы
maintainer перед выпуском; локальный анализ не меняется.

URL проверяется синтаксически. Команда **не проверяет доступность сайта и
не доказывает истинность содержимого evidence**. Существующий файл — необходимая
проверка целостности ссылок, а запись результата и решение о поддержке остаются
ответственностью maintainer по [review cycle](../connectors/review-policy.md).
Ни одна дата не обновляется автоматически, в том числе при недоступном источнике.

## CI/release

- Опубликованная JSON Schema дополнительно проверяется стандартным
  `jsonschema==4.26.0` (только development/CI):
  `python -m pip install -r scripts/requirements-registry.txt`, затем
  `python scripts/check-registry-schema.py`. Включены date/URI formats;
  разрешены только локальные `$ref`, внешние schemas не загружаются.
- CI `connectors` запускает тот же offline validator и сохраняет JSON artifact.
- Release `connectors` сначала выполняет core tests, затем `--release`.
- Создание draft, сборки installers/Arch и последующая публикация crates
  зависят от успешного gate. В `workflow_dispatch` пропуск tag-only draft
  сохраняет сборку artifacts при успешном gate.

Условие сборки явно проверяет `needs.connectors.result == 'success'` и status
functions, чтобы skipped draft не добавил неявный `success()` к manual run.
Семантика сверена 2026-09-27 с
[GitHub job dependencies](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idneeds)
и [status functions](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions#status-check-functions).
Изменение workflow не является запуском release; tag/manual release здесь не
инициировались. Полная квалификация установщиков остаётся в эпике `t8t`.

## Проверки контракта

`core/tests/registry.rs` проходит через публичный loader, `SnapshotStore`,
валидатор и настоящий CLI. Проверяются исправный inventory, scope квалификации,
exact format/version/OS, отсутствующий код, подложная implementation, устаревшая
revision, фиктивная real-client проверка, скрытие enabled operations, дубли,
неверные/будущие даты и отсутствующие/внешние evidence artifacts. Просроченный
release-check отклоняется, но реальный synthetic JSON импортируется и остаётся
в store; CLI оставляет bytes inventory неизменными. Реальные аккаунты не нужны.
