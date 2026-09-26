# TGSUM: полный состав для согласования

Снимок Beads от 2026-09-26 12:45 UTC. Решение по этой сверке: `tgsum-9x6`.

Это представление существующих задач для согласования по запросу пользователя. Состояния ниже относятся только к моменту выгрузки. Задачи, критерии, зависимости и дальнейшие изменения ведутся в **Beads**; этот документ не редактируется как отдельный трекер. Предыдущие отметки approval не являются ответом пользователя на текущую сверку.

Основание: [roadmap](context-gateway-roadmap.md), [спецификация](../specs/context-gateway.md), [архитектура коннекторов](../connectors/architecture.md), [цикл проверки](../connectors/review-policy.md).

## Что согласовываем

- 12 продуктовых и сквозных эпиков, 106 дочерних задач. Задача согласования не добавляет продуктовый эпик или функциональный срез.
- Из 106 задач: 3 выполнены, 1 в работе, 102 открыты. Все 12 эпиков остаются незавершёнными.
- 35 отдельных задач с меткой verification: 18 автономных проверок и 17 проверок/действий с участием пользователя. Это число задач, а не выполненных тестов.
- Закрытая основа `tgsum-d9b` переиспользуется: Telegram full/single JSON, snapshots/diff и offline Bridge simulator. Работающий экспорт через настоящий Telegram Desktop этим не доказан.
- Эпик закрывается только после выполнения всех задач согласованного состава. Перенос проверки в backlog не доказывает совместимость.
- Все проверки с реальными messenger/agent accounts — только под контролем пользователя. Требующие его участия действия по умолчанию остаются backlog; независимый код проверяется на synthetic fixtures и симуляторах.
- Перед изменением источника/API/авторизации/формата/автоматизации обязателен research. Плановый review: 30 дней для network/automation, 90 для file/local.
- Telegram: официальный неизменённый Desktop → выбранный чат → штатный export → локальный snapshot/diff. Import и assisted fallback доступны независимо от квалификации автоматизации. `tdata`, session databases и cookies не читаются.
- VM/изолированная графическая среда — отдельный исследовательский срез; работоспособность unattended GUI не предполагается.

## Порядок поставки

Context Engine → Privacy & Files → Telegram Bridge → Archive Pack → Live Business → RU → Discord → объединённый Workspace. Research/registry и проверки идут вместе с затронутыми срезами. OMP/local/custom — после общего runner. Later содержит исследования и решения go/no-go, а не обещание реализовать все исследуемые интеграции. Номера версий задают состав, не сроки.

## Эпики

| Эпик | Результат | Срезов | Выполнено | Проверок | Требуют пользователя |
| --- | --- | ---: | ---: | ---: | ---: |
| [tgsum-hzm](#tgsum-hzm) | v0.3 — Context Engine, Projects и первые агенты | 13 | 3 | 2 | 0 |
| [tgsum-af2](#tgsum-af2) | v0.4 — Privacy, псевдонимы и выбранные вложения | 9 | 0 | 2 | 0 |
| [tgsum-i1w](#tgsum-i1w) | v0.5 — Telegram 2.0 и Official Client Bridge | 8 | 0 | 2 | 0 |
| [tgsum-b5v](#tgsum-b5v) | v0.6 — Archive Pack: WhatsApp, Slack, Яндекс, Signal, LINE, Google Chat | 11 | 0 | 1 | 0 |
| [tgsum-ycq](#tgsum-ycq) | v0.7 — Live Business: Teams, Google Chat, Slack, Яндекс Bot | 7 | 0 | 1 | 0 |
| [tgsum-cs2](#tgsum-cs2) | v0.8 — RU Pack: VK и MAX Bot | 5 | 0 | 1 | 0 |
| [tgsum-7hv](#tgsum-7hv) | v0.9 — Discord Server Connector | 3 | 0 | 1 | 0 |
| [tgsum-6gf](#tgsum-6gf) | v1.0 — Multi-source Workspace и Update & Analyze | 7 | 0 | 1 | 0 |
| [tgsum-brk](#tgsum-brk) | После первых adapters — OMP, локальные модели и recipes | 5 | 0 | 1 | 0 |
| [tgsum-2ty](#tgsum-2ty) | Сквозной — Research и жизненный цикл поддержки коннекторов | 4 | 0 | 1 | 0 |
| [tgsum-t8t](#tgsum-t8t) | Сквозной — Тестирование, квалификация интеграций и выпуск | 23 | 0 | 22 | 17 |
| [tgsum-506](#tgsum-506) | Отложено — Исследования и расширения после основного roadmap | 11 | 0 | 0 | 0 |

## Все срезы

В каждом срезе указан конкретный результат и критерий приёмки. Зависимости ниже — обязательные предшественники из Beads, без связи parent-child.

<a id="tgsum-hzm"></a>

### tgsum-hzm — v0.3 — Context Engine, Projects и первые агенты

Пользователь создаёт Project на Telegram, выбирает scope, готовит проверяемый bundle и запускает Export only, Codex или Claude Code. Переиспользовать закрытый tgsum-d9b; повторно parser/store/diff не реализовывать.

**Закрытие эпика:** Все дочерние срезы выполнены. Сквозной synthetic сценарий Project → scope → privacy → bundle → fake agent → evidence проходит. Cloud adapter доступен только с проверенным isolation profile; до этого Export only. Реальная интеграционная квалификация отдельно в эпике QA. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-ax6"></a>

#### tgsum-ax6 — Evidence и приватный Export only context bundle

**На момент сверки:** Открыта.

Разделить private store и bundle: стабильные opaque evidence IDs, revision mapping, canonical Markdown, manifest и provenance. Ссылки на выбранные вложения подготовить к safe packager из Privacy; raw paths/mappings агенту не выдавать.

**Приёмка:** Вывод сопоставляется с конкретной версией сообщения; одинаковый текст с разными ID сохраняется; secrets/raw source paths/credential refs вне bundle; legacy Markdown совместим.

**Зависит от:** [tgsum-hzm.2](#tgsum-hzm.2), [tgsum-hzm.5](#tgsum-hzm.5), [tgsum-hzm.3](#tgsum-hzm.3).

<a id="tgsum-hzm.1"></a>

#### tgsum-hzm.1 — Project store и управление источниками

**На момент сверки:** Выполнена.

Создание/открытие/переименование Project, private directory, настройки и ссылки на выбранные Source/Snapshot; миграции manifest. Использовать существующий SnapshotStore.

**Приёмка:** После перезапуска Project восстанавливается; исходный полный архив не копируется в project storage; повреждённый manifest и изменение пути источника дают recoverable error.

**Зависит от:** `tgsum-d9b.1`.

<a id="tgsum-hzm.2"></a>

#### tgsum-hzm.2 — Canonical schema: coverage, provenance и версии данных

**На момент сверки:** Выполнена.

Достроить source/conversation/message/attachment metadata, диапазоны coverage/gaps, timezone status, revision/provenance и миграции поверх tgsum-d9b.

**Приёмка:** Native IDs остаются точными; неопределённые timestamp/coverage не превращаются в UTC/complete; старая версия snapshot читается или корректно мигрирует; missing не становится deletion.

**Зависит от:** `tgsum-d9b.1`.

<a id="tgsum-hzm.3"></a>

#### tgsum-hzm.3 — Выбор чатов, тем, периода и new since analysis

**На момент сверки:** В работе.

Project scope UI и локальные фильтры по фактически импортированным данным; отдельный baseline последнего успешного анализа.

**Приёмка:** В bundle попадает только выбранный scope; изменения старых сообщений учитываются; неуспешный анализ не передвигает baseline; Telegram date filter не является delta-источником истины.

**Зависит от:** [tgsum-hzm.1](#tgsum-hzm.1), [tgsum-hzm.2](#tgsum-hzm.2).

<a id="tgsum-hzm.4"></a>

#### tgsum-hzm.4 — Интерфейсы importer, acquisition и local-data connector

**На момент сверки:** Выполнена.

Отделить получение данных от нормализации и форматирования; технические capabilities/coverage адаптера используются одинаково для archive/API/bot/local backup.

**Приёмка:** Telegram сохраняет прежнее поведение; offline Export only не требует network/auth dependencies; второй synthetic adapter проходит тот же контракт.

**Зависит от:** [tgsum-hzm.2](#tgsum-hzm.2).

<a id="tgsum-hzm.5"></a>

#### tgsum-hzm.5 — Минимальный secrets scanner и Review перед AI

**На момент сверки:** Открыта.

Локальное скрытие high-confidence секретов, preview замен и явный получатель анализа до первого runner. Medium-confidence находки показываются для review.

**Приёмка:** Known synthetic tokens/keys/credentials не попадают в подготовленный bundle; preview показывает объём и получателя; Export only доступен без агента. Полная анонимность не обещается.

**Зависит от:** [tgsum-hzm.3](#tgsum-hzm.3).

<a id="tgsum-hzm.6"></a>

#### tgsum-hzm.6 — Изолированный runner и контракт agent adapter

**На момент сверки:** Открыта.

Discovery executable/version/auth availability, аргументы без shell interpolation, read-only bundle, отдельный result channel, timeout/cancel и минимальная среда для выбранных ОС.

**Приёмка:** Fake agent не читает raw exports/домашний каталог/connector secrets и не пишет в source; неизвестный isolation profile даёт Export only; CLI feature flags проверяются по установленной версии и первичным docs.

**Зависит от:** [tgsum-ax6](#tgsum-ax6).

<a id="tgsum-hzm.7"></a>

#### tgsum-hzm.7 — Codex adapter: запуск и structured result

**На момент сверки:** Открыта.

Реализовать актуальный поддержанный non-interactive CLI flow с ограниченным bundle, structured output и обработкой ошибок; OpenAI docs/local CLI research до кодирования.

**Приёмка:** Fake executable проверяет stdin/args, timeout, cancel, exit codes и невалидный output; receiver виден пользователю; настоящая авторизация проверяется отдельно под контролем пользователя.

**Зависит от:** [tgsum-hzm.6](#tgsum-hzm.6).

<a id="tgsum-hzm.8"></a>

#### tgsum-hzm.8 — Claude Code adapter: запуск и structured result

**На момент сверки:** Открыта.

Проверить актуальные CLI/version/permission controls и реализовать ограниченный analysis flow через установленный executable.

**Приёмка:** Fake CLI suite покрывает output/errors/cancel, tool restrictions и отсутствие source credentials; непроверенный profile не запускается.

**Зависит от:** [tgsum-hzm.6](#tgsum-hzm.6).

<a id="tgsum-hzm.9"></a>

#### tgsum-hzm.9 — Recipes: Summary, Retro, Decisions, Actions, Incident, Handover

**На момент сверки:** Открыта.

Шесть версионируемых recipes с обязательными evidence/revision references, coverage и понятным result schema.

**Приёмка:** Synthetic ответы проверяются на ссылочную целостность; неизвестный owner/deadline не выдумывается; untrusted chat text не получает роль управляющей инструкции.

**Зависит от:** [tgsum-ax6](#tgsum-ax6).

<a id="tgsum-hzm.10"></a>

#### tgsum-hzm.10 — Home, guided flow и повторный onboarding

**На момент сверки:** Открыта.

Recent Projects → Source & Scope → Privacy → Recipe/Agent → Review → Result; обнаружение agents, Export only и Help → повторить onboarding.

**Приёмка:** Первый Project создаётся без credentials; пользователь видит selected scope, destination и доступные действия; сценарий можно повторить; ошибки не теряют выбранные настройки.

**Зависит от:** [tgsum-hzm.5](#tgsum-hzm.5), [tgsum-hzm.9](#tgsum-hzm.9), [tgsum-hzm.3](#tgsum-hzm.3), [tgsum-hzm.8](#tgsum-hzm.8), [tgsum-hzm.1](#tgsum-hzm.1), [tgsum-hzm.7](#tgsum-hzm.7).

<a id="tgsum-hzm.11"></a>

#### tgsum-hzm.11 — Тестирование: Project → bundle → fake agent → evidence

**На момент сверки:** Открыта · проверка.

Сквозной synthetic сценарий приложения, повторный импорт, изменение scope, отказ агента и baseline последнего успешного анализа.

**Приёмка:** Один reproducible suite проходит через публичные интерфейсы/UI host; проверяет отсутствие лишних чатов, правильный diff и evidence до/после edits; результаты сохранены в задаче.

**Зависит от:** [tgsum-hzm.10](#tgsum-hzm.10).

<a id="tgsum-hzm.12"></a>

#### tgsum-hzm.12 — Тестирование runner: prompt injection и попытки выхода за scope

**На момент сверки:** Открыта · проверка.

Adversarial synthetic chat/attachment content, malicious file names, repo hooks/MCP inheritance и fake executables для каждого заявленного isolation profile.

**Приёмка:** Read/write/network/tool ограничения подтверждены отдельными сценариями; недоказанная изоляция отключает соответствующий запуск; реальных agent accounts не использовать.

**Зависит от:** [tgsum-hzm.8](#tgsum-hzm.8), [tgsum-hzm.6](#tgsum-hzm.6), [tgsum-hzm.7](#tgsum-hzm.7).

<a id="tgsum-af2"></a>

### tgsum-af2 — v0.4 — Privacy, псевдонимы и выбранные вложения

Расширенный локальный sanitizer, стабильные замены, пользовательские словари, preview и безопасное включение файлов в context bundle.

**Закрытие эпика:** Все дочерние задачи выполнены; секреты и приватные mappings не уходят в bundle, соответствия стабильны, adversarial fixtures не обходят scope/path/size ограничения. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-af2.1"></a>

#### tgsum-af2.1 — Расширенные secrets rules и уровни уверенности

**На момент сверки:** Открыта.

API keys, tokens, JWT/Bearer, private keys, DSN/URL passwords, env credentials, cookies/session values и контекстная entropy detection поверх минимального scanner.

**Приёмка:** Версионированный synthetic corpus измеряет true/false positives; high-confidence замены и medium-confidence preview различимы; plaintext secrets не появляются в отчётах.

**Зависит от:** [tgsum-hzm.5](#tgsum-hzm.5).

<a id="tgsum-af2.2"></a>

#### tgsum-af2.2 — Псевдонимизация инфраструктуры

**На момент сверки:** Открыта.

IP, hostnames, internal domains/URLs, usernames, локальные/UNC paths и cloud resource identifiers; сохранять повторные связи сущностей.

**Приёмка:** Один объект получает один псевдоним внутри Project; safe URLs не ломаются произвольным regex; policy categories можно включать отдельно.

**Зависит от:** [tgsum-af2.5](#tgsum-af2.5).

<a id="tgsum-af2.3"></a>

#### tgsum-af2.3 — Участники и PII: имена, usernames, email, телефоны

**На момент сверки:** Открыта.

Детерминированные замены известных участников по source IDs, email/phone и выбранных типов PII; пределы извлечения свободного текста явно описаны.

**Приёмка:** Повторные упоминания и participant fields согласованы; коллизии имён не склеивают разных людей; Unicode/case/multisource fixtures проходят.

**Зависит от:** [tgsum-af2.5](#tgsum-af2.5).

<a id="tgsum-af2.4"></a>

#### tgsum-af2.4 — Пользовательский словарь чувствительных терминов

**На момент сверки:** Открыта.

Компании, проекты, домены, репозитории и другие явные значения; правила границ совпадения и их preview.

**Приёмка:** Словарь сохраняется в Project, применяется детерминированно, не раскрывает исходные значения в bundle или diagnostic logs.

**Зависит от:** [tgsum-af2.5](#tgsum-af2.5).

<a id="tgsum-af2.5"></a>

#### tgsum-af2.5 — Приватный mapping и стабильность замен между запусками

**На момент сверки:** Открыта.

Хранение project-local соответствий вне bundle, versioning/reset и namespace независимых проектов; порядок сообщений не меняет прежние псевдонимы.

**Приёмка:** Повторный импорт и расширение scope сохраняют ранее выданные обозначения; новый Project не получает чужие mappings; reset не подменяет старые analysis references.

**Зависит от:** [tgsum-hzm.1](#tgsum-hzm.1), [tgsum-hzm.5](#tgsum-hzm.5).

<a id="tgsum-af2.6"></a>

#### tgsum-af2.6 — Безопасное включение выбранных текстовых вложений

**На момент сверки:** Открыта.

txt/md/log/json/jsonl/yaml/toml/ini/csv/code/sql/sh/ps1: metadata, выбор, path/symlink/type/size/budget проверки и изолированная копия; архивы вложений не раскрываются.

**Приёмка:** Traversal, symlink escapes и превышение бюджета отклоняются; hardlinks с mutable source не используются; отсутствующие файлы видны; remote URLs не скачиваются скрыто.

**Зависит от:** [tgsum-ax6](#tgsum-ax6).

<a id="tgsum-af2.7"></a>

#### tgsum-af2.7 — Privacy presets и preview текста и файлов

**На момент сверки:** Открыта.

Категории, counts, сравнение до/после, исключения и company terms; выбор preset сохраняется в Project.

**Приёмка:** UI честно показывает applied/pending находки и точный набор файлов; смена preset инвалидирует устаревший bundle; minimum preview остаётся до любого Run.

**Зависит от:** [tgsum-af2.6](#tgsum-af2.6), [tgsum-af2.3](#tgsum-af2.3), [tgsum-af2.1](#tgsum-af2.1), [tgsum-af2.4](#tgsum-af2.4), [tgsum-af2.2](#tgsum-af2.2).

<a id="tgsum-af2.8"></a>

#### tgsum-af2.8 — Тестирование sanitizer: качество и стабильность

**На момент сверки:** Открыта · проверка.

Synthetic corpus по всем категориям, конфликты правил, repeated runs, Unicode, false positives, отсутствие утечек в diagnostics и results metadata.

**Приёмка:** Есть воспроизводимые пороги/ожидаемые результаты без обещания universal NER; mapping не попадает в bundle; regressions блокируют соответствующие правила.

**Зависит от:** [tgsum-af2.7](#tgsum-af2.7).

<a id="tgsum-af2.9"></a>

#### tgsum-af2.9 — Тестирование packager: traversal, symlinks, лимиты и гонки

**На момент сверки:** Открыта · проверка.

Созданные тестом файлы/ссылки, изменение source во время copying, коллизии имён, отсутствующие файлы, binary masquerading и отмена.

**Приёмка:** Ни один тест не читает вне своего temp root; ошибки не публикуют непроверенный bundle; результат независим от дальнейшего изменения исходного файла.

**Зависит от:** [tgsum-af2.6](#tgsum-af2.6).

<a id="tgsum-i1w"></a>

### tgsum-i1w — v0.5 — Telegram 2.0 и Official Client Bridge

Запомненные источники, assisted export, watcher, OS drivers неизменённого официального клиента и refresh при активной пользовательской сессии. Основа tgsum-d9b уже реализована.

**Закрытие эпика:** Все дочерние задачи реализации и synthetic проверки выполнены. По непроверенным ОС/версиям остаётся assisted/manual fallback; promotion автоматизации требует связанной user-controlled QA, без обещания нулевого риска. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-l4c"></a>

#### tgsum-l4c — Watcher завершённых экспортов и повторный импорт

**На момент сверки:** Открыта.

Использовать готовый store/diff из tgsum-d9b; watcher только выбранной папки, debounce, completion manifest/manual confirmation или text-only scope, staging hash и обнаружение изменения source.

**Приёмка:** Partial writes, повторные события, cancel/disk errors и изменение при копировании не публикуют неподтверждённый snapshot. Появление файла не запускает облачный анализ автоматически.

**Зависит от:** [tgsum-i1w.1](#tgsum-i1w.1), `tgsum-2pc`.

<a id="tgsum-i1w.1"></a>

#### tgsum-i1w.1 — Запомненный Telegram source и assisted export UI

**На момент сверки:** Открыта.

Подключить выбранный разговор к Project; инструкции/launch официального клиента, needs_user_action, назначенный export path и permanent manual fallback.

**Приёмка:** TGSUM не ищет tdata/session/cookies; scope подтверждён человеком; статус не выдаёт launch за успешный export; UI можно прогнать с fake client.

**Зависит от:** [tgsum-hzm.1](#tgsum-hzm.1), [tgsum-hzm.4](#tgsum-hzm.4).

<a id="tgsum-i1w.2"></a>

#### tgsum-i1w.2 — Telegram Bridge driver — Linux

**На момент сверки:** Открыта.

Исследовать accessibility/AT-SPI выбранной версии официального Desktop и реализовать export driver с fake accessibility tree; учитывать Wayland/X11 и активную session.

**Приёмка:** Есть detect/version/can_export и bounded retry; exact chat selection и completion не выводятся из координат/роста файла; до controlled PoC driver experimental/disabled.

**Зависит от:** [tgsum-i1w.1](#tgsum-i1w.1), [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-2ty.3](#tgsum-2ty.3).

<a id="tgsum-i1w.3"></a>

#### tgsum-i1w.3 — Telegram Bridge driver — Windows

**На момент сверки:** Открыта.

UI Automation для установленного неизменённого Desktop: selectors, exact source, export completion, permissions/lock состояния; тестировать на fake tree.

**Приёмка:** Неверная версия/селектор/чат прекращает export attempt; session storage не читается; без контролируемого PoC нет supported automation claim.

**Зависит от:** [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-i1w.1](#tgsum-i1w.1), [tgsum-2ty.3](#tgsum-2ty.3).

<a id="tgsum-i1w.4"></a>

#### tgsum-i1w.4 — Telegram Bridge driver — macOS

**На момент сверки:** Открыта.

Accessibility driver с явным permission status, version detection, chat/export/completion; учитывается locked/inactive GUI session.

**Приёмка:** Fake OS suite проверяет отказ permissions, change of version, cancellation; credential/session access отсутствует; реальный клиент только в QA с пользователем.

**Зависит от:** [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-i1w.1](#tgsum-i1w.1), [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-i1w.5"></a>

#### tgsum-i1w.5 — Refresh now и расписание при активном компьютере

**На момент сверки:** Открыта.

Manual/on-start/daily/6h как настраиваемые варианты, один export за раз, следующий active/unlocked session, уважение клиентских задержек и backoff.

**Приёмка:** Fake clock/session покрывает sleep/lock/restart/cancel; расписание opt-in и не дублирует jobs; частота не рекламируется как безопасный лимит платформы.

**Зависит от:** [tgsum-l4c](#tgsum-l4c), [tgsum-i1w.4](#tgsum-i1w.4), [tgsum-i1w.3](#tgsum-i1w.3), [tgsum-i1w.2](#tgsum-i1w.2).

<a id="tgsum-i1w.6"></a>

#### tgsum-i1w.6 — Тестирование drivers: версии, селекторы и ошибки клиента

**На момент сверки:** Открыта · проверка.

Контрактные сценарии трёх OS adapters на synthetic accessibility fixtures: wrong chat, delay, missing export capability, stale callbacks, lock, cancel.

**Приёмка:** Каждый declared OS profile проходит одинаковые safety invariants и собственные selectors; unsupported ветки оставляют assisted fallback.

**Зависит от:** [tgsum-i1w.4](#tgsum-i1w.4), [tgsum-i1w.2](#tgsum-i1w.2), [tgsum-i1w.3](#tgsum-i1w.3).

<a id="tgsum-i1w.7"></a>

#### tgsum-i1w.7 — Тестирование UI: assisted export и повторный refresh

**На момент сверки:** Открыта · проверка.

С fake client пройти подключение одного/нескольких чатов, completion, +/edited/missing, ошибки файла, retry и настройки расписания.

**Приёмка:** UI отображает реальное состояние, scope и coverage; не импортирует другой источник; нет вызовов настоящего Telegram.

**Зависит от:** [tgsum-i1w.5](#tgsum-i1w.5), [tgsum-i1w.6](#tgsum-i1w.6).

<a id="tgsum-b5v"></a>

### tgsum-b5v — v0.6 — Archive Pack: WhatsApp, Slack, Яндекс, Signal, LINE, Google Chat

Нормализация официальных экспортов/backup в общий context store, честная полнота и повторный импорт. Signal Desktop/Android различаются; LINE Bridge — отдельный experimental срез.

**Закрытие эпика:** Все принятые в scope importer-задачи и матрица synthetic fixtures выполнены. Для неподтверждённого формата сохраняется явный unsupported/research. Реальные fixtures и клиентские проверки отдельно в QA; неизвестный формат не объявляется поддержанным. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-b5v.1"></a>

#### tgsum-b5v.1 — Общий безопасный reader контейнеров и определение формата

**На момент сверки:** Открыта.

Поддержанные source ZIP/directories/password-protected envelopes читаются с budgets и path checks. Это отдельный allowlist source formats, не произвольная распаковка вложений.

**Приёмка:** Повреждённый/неизвестный формат отклоняется; zip bombs, traversal и огромные entries ограничены; passwords/recovery keys не пишутся в logs/store.

**Зависит от:** [tgsum-hzm.4](#tgsum-hzm.4).

<a id="tgsum-b5v.2"></a>

#### tgsum-b5v.2 — TXT identities и честный matching повторных архивов

**На момент сверки:** Открыта.

Snapshot-local evidence/ordinal, неоднозначные даты/timezones и conservative matching при отсутствии native IDs.

**Приёмка:** Два одинаковых сообщения не склеиваются; ambiguous matches остаются явно uncertain; отсутствие строки не доказывает удаление.

**Зависит от:** [tgsum-hzm.2](#tgsum-hzm.2).

<a id="tgsum-b5v.3"></a>

#### tgsum-b5v.3 — WhatsApp Personal — per-chat TXT и выбранные media

**На момент сверки:** Открыта.

Проверить реальные варианты официального export; importer multiline/locale/system messages/attachments через canonical contract. Логин и Web scraping вне scope.

**Приёмка:** Synthetic format matrix и coverage проходят; native IDs не выдумываются; только предоставленные файлы читаются; fixtures пользователя собираются отдельно.

**Зависит от:** [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-b5v.1](#tgsum-b5v.1), [tgsum-af2.6](#tgsum-af2.6), [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-b5v.2](#tgsum-b5v.2).

<a id="tgsum-b5v.4"></a>

#### tgsum-b5v.4 — Slack — JSON ZIP importer и recurring export inbox

**На момент сверки:** Открыта.

Каналы, users, threads, edits и file references; импорт повторного локального ZIP. Scheduled export на стороне Slack не считается доставленным на диск.

**Приёмка:** Exact ts/IDs, thread links и scope/permissions coverage сохраняются; ссылки не выдаются за локальные bytes; delta проверен на synthetic snapshots.

**Зависит от:** [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-hzm.2](#tgsum-hzm.2), [tgsum-b5v.1](#tgsum-b5v.1), [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-l4c](#tgsum-l4c).

<a id="tgsum-b5v.5"></a>

#### tgsum-b5v.5 — Яндекс Мессенджер — password-protected archive importer

**На момент сверки:** Открыта.

Квалифицировать структуру официального архива и реализовать parser, participants/attachments/IDs/coverage; секрет архива используется только локально.

**Приёмка:** Версия/структура подтверждены fixture contract; неизвестный archive shape не даёт пустой success; identity mapping к bot отложен до отдельной проверки.

**Зависит от:** [tgsum-b5v.1](#tgsum-b5v.1), [tgsum-hzm.2](#tgsum-hzm.2), [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-b5v.6"></a>

#### tgsum-b5v.6 — Signal Desktop — локальный encrypted backup importer

**На момент сверки:** Открыта.

Исследовать формат официального Desktop backup и допустимый локальный parser/decryption; recovery key предоставляет пользователь. Не читать работающую session DB.

**Приёмка:** Synthetic encrypted fixtures покрывают wrong key, corruption, messages/media и coverage; совместимость реального backup подтверждается в QA, не приравнивается к Android.

**Зависит от:** [tgsum-hzm.2](#tgsum-hzm.2), [tgsum-b5v.1](#tgsum-b5v.1), [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-b5v.7"></a>

#### tgsum-b5v.7 — Signal Android — старый и новый on-device backup

**На момент сверки:** Открыта.

Отдельные format profiles для legacy .backup и directory backup; локальная расшифровка и нормализация с явным coverage.

**Приёмка:** Wrong key/version/corruption обрабатываются без утечки; для каждого заявленного профиля собственные fixtures; gaps/disappearing exclusions видны.

**Зависит от:** [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-b5v.1](#tgsum-b5v.1), [tgsum-hzm.2](#tgsum-hzm.2).

<a id="tgsum-b5v.8"></a>

#### tgsum-b5v.8 — LINE — Desktop/mobile TXT importer

**На момент сверки:** Открыта.

Save Chat/export TXT, доступная на клиенте история и locale variants; snapshots и uncertain matching.

**Приёмка:** Coverage partial/unknown отражён в manifest; повторные одинаковые строки сохраняются; вложения, отсутствующие в TXT, не обещаются.

**Зависит от:** [tgsum-b5v.2](#tgsum-b5v.2), [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-b5v.9"></a>

#### tgsum-b5v.9 — LINE — experimental Desktop export Bridge

**На момент сверки:** Открыта.

Проверить штатный Save Chat на целевых Desktop версиях и адаптировать acquisition contract; offline selectors/simulation до controlled PoC.

**Приёмка:** Unsupported OS/version честно возвращает manual fallback; никаких session DB; реальная автоматизация не enabled до отдельного QA.

**Зависит от:** [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-i1w.6](#tgsum-i1w.6), [tgsum-b5v.8](#tgsum-b5v.8).

<a id="tgsum-b5v.10"></a>

#### tgsum-b5v.10 — Google Chat — Takeout importer с проверкой доступного scope

**На момент сверки:** Открыта.

Квалифицировать содержимое Takeout для выбранного типа аккаунта/space и нормализовать разрешённый формат; не предполагать полноту work/school exports.

**Приёмка:** Fixture contract сохраняет messages/participants/attachments и known gaps; непредставленный space не объявляется пустой полной историей.

**Зависит от:** [tgsum-hzm.2](#tgsum-hzm.2), [tgsum-b5v.1](#tgsum-b5v.1), [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-b5v.11"></a>

#### tgsum-b5v.11 — Тестирование Archive Pack: форматы, encoding, identity и coverage

**На момент сверки:** Открыта · проверка.

Матрица по всем importer profiles: Unicode/locales/timezones, multiline, повторные exports, corrupt/encrypted archives, attachment references и budgets.

**Приёмка:** Каждый реализованный profile имеет fixtures и regression suite; expected capability/coverage совпадает с registry; fixture provenance фиксируется.

**Зависит от:** [tgsum-b5v.3](#tgsum-b5v.3), [tgsum-b5v.6](#tgsum-b5v.6), [tgsum-b5v.4](#tgsum-b5v.4), [tgsum-b5v.5](#tgsum-b5v.5), [tgsum-b5v.10](#tgsum-b5v.10), [tgsum-b5v.7](#tgsum-b5v.7), [tgsum-b5v.8](#tgsum-b5v.8).

<a id="tgsum-ycq"></a>

### tgsum-ycq — v0.7 — Live Business: Teams, Google Chat, Slack, Яндекс Bot

Официальные API/bot-подключения: выбранные разговоры, bootstrap, updates, checkpoint, восстановление gaps, revoke и хранение предназначенных интеграции credentials.

**Закрытие эпика:** Все дочерние задачи и mock contract suites завершены; scope/grant и доставка событий описаны для каждого adapter. Production enablement требует research и контролируемой проверки соответствующего tenant/account из QA. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-8hj"></a>

#### tgsum-8hj — Research: Яндекс Bot, доступ и archive/bot identity

**На момент сверки:** Открыта.

Существующая задача квалификации: корпоративный use case, регистрация, scopes, retention событий, один polling consumer, ordering и mapping архива к bot.

**Приёмка:** Есть датированное решение со ссылками, contract fixtures и открытыми вопросами; реальный доступ/регистрация пользователя — отдельная QA задача.

**Зависит от:** [tgsum-hzm.4](#tgsum-hzm.4).

<a id="tgsum-ycq.1"></a>

#### tgsum-ycq.1 — Общий lifecycle официального API подключения

**На момент сверки:** Открыта.

Явно выданные OAuth/bot tokens, OS secret store references, scopes/revoke, subscription renewal, atomic checkpoint, single consumer и доставка событий для local desktop.

**Приёмка:** Mocks доказывают commit-before-cursor и отсутствие потерь/дублей при restart; auth grant отделён от выбранных чатов; tokens не доступны runner и не извлекаются из чужих профилей.

**Зависит от:** [tgsum-99k](#tgsum-99k), [tgsum-hzm.4](#tgsum-hzm.4).

<a id="tgsum-ycq.2"></a>

#### tgsum-ycq.2 — Teams — Graph history и selected-chat subscriptions

**На момент сверки:** Открыта.

Перед реализацией проверить применимые delegated/application/RSC profiles, permissions и tenant requirements; history/pagination/date range плюс renewal/recovery.

**Приёмка:** Mock API suite покрывает bootstrap/create/update/delete/gaps/revoke; UI показывает реальную ширину grant; production validation выполняется отдельной задачей на tenant пользователя.

**Зависит от:** [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-ycq.1](#tgsum-ycq.1).

<a id="tgsum-ycq.3"></a>

#### tgsum-ycq.3 — Google Chat — OAuth spaces, history и events

**На момент сверки:** Открыта.

Актуальный user OAuth flow, выбранные spaces, time/thread filters, events и reconciliation после offline gap; проверить действующие ограничения.

**Приёмка:** Mock fixtures подтверждают pagination, created/updated/deleted и gap recovery; missed window явно отражён; архив/API identity объединяется лишь при проверенном mapping.

**Зависит от:** [tgsum-ycq.1](#tgsum-ycq.1), [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-2ty.3](#tgsum-2ty.3).

<a id="tgsum-ycq.4"></a>

#### tgsum-ycq.4 — Slack — OAuth, history bootstrap и Events

**На момент сверки:** Открыта.

Выбрать поддерживаемый internal/distributed app profile и способ доставки events; rate limits/backoff, threads/files, подписки выбранных conversations.

**Приёмка:** Mocks проверяют применимые лимиты, retries и revoke; history не опрашивается как бесконечный realtime poll; нет обещания автоматического download workspace export.

**Зависит от:** [tgsum-ycq.1](#tgsum-ycq.1), [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-ycq.5"></a>

#### tgsum-ycq.5 — Яндекс Bot — selected work chats и archive bootstrap

**На момент сверки:** Открыта.

Локальный polling с offset либо отдельно поддержанный webhook; subscription scope, commit-before-offset и gap reporting. Archive overlap объединять только при подтверждённом mapping.

**Приёмка:** Mocks покрывают restart, duplicate delivery, порядок, expiry и edits/deletes по реальным возможностям; future-only режим явно подписан.

**Зависит от:** [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-8hj](#tgsum-8hj), [tgsum-b5v.5](#tgsum-b5v.5), [tgsum-ycq.1](#tgsum-ycq.1), [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-ycq.6"></a>

#### tgsum-ycq.6 — Тестирование Live Business: mock servers и recovery

**На момент сверки:** Открыта · проверка.

Общая suite и отдельные контрактные fixtures Teams/Google/Slack/Yandex: pagination boundaries, duplicates/out-of-order, 429/5xx, revoke, subscription expiry и offline gaps.

**Приёмка:** Результаты подтверждают сохраняемый checkpoint и scope; неподдержанное удаление/вложение/история явно отмечены; аккаунты и внешние tenant endpoints не используются.

**Зависит от:** [tgsum-ycq.2](#tgsum-ycq.2), [tgsum-ycq.5](#tgsum-ycq.5), [tgsum-ycq.3](#tgsum-ycq.3), [tgsum-ycq.4](#tgsum-ycq.4).

<a id="tgsum-cs2"></a>

### tgsum-cs2 — v0.8 — RU Pack: VK и MAX Bot

Квалификация распространяемого VK приложения и MAX Bot; выбранные peers/chats, история, delta, вложения. MAX Open Client для этой архитектуры не используется.

**Закрытие эпика:** Research даёт решение для конкретного типа приложения; все согласованные code/test задачи выполнены. Если доступ недоступен, интеграция остаётся research и scope эпика пересогласуется, а не закрывается фиктивной поддержкой. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-cs2.1"></a>

#### tgsum-cs2.1 — Research: VK OAuth scopes и распространяемое приложение

**На момент сверки:** Открыта.

Проверить выдачу messages permissions, app registration/distribution, history/Long Poll и актуальные ограничения данных/AI по первичным источникам.

**Приёмка:** Решение go/no-go относится к конкретному app profile; наличие метода в SDK не выдаётся за доступ; действия пользователя вынесены в controlled QA.

**Зависит от:** [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-cs2.2"></a>

#### tgsum-cs2.2 — VK — selected peers, history и Long Poll

**На момент сверки:** Открыта.

После положительного qualification реализовать user OAuth adapter: выбор peers, bootstrap, delta/recovery, attachments metadata и revoke.

**Приёмка:** Fake API suite проходит; account-wide grant не скрыт за selected scope; при недоступных scopes integration остаётся disabled/research.

**Зависит от:** [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-cs2.1](#tgsum-cs2.1), [tgsum-ycq.1](#tgsum-ycq.1).

<a id="tgsum-cs2.3"></a>

#### tgsum-cs2.3 — Research: MAX Bot-admin history и updates

**На момент сверки:** Открыта.

Проверить регистрацию/получение bot token, admin scope, фактические from/to/count границы, events и ограничения хранения/использования. Open Client исключён.

**Приёмка:** Есть versioned contract и решение для выбранного сценария; неподтверждённые единицы времени/границы не используются по предположению.

**Зависит от:** [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-cs2.4"></a>

#### tgsum-cs2.4 — MAX Bot — история выбранных рабочих чатов и delta

**На момент сверки:** Открыта.

Admin-selected chats через официальный Bot API, history pages, events/checkpoints, token lifecycle и вложения.

**Приёмка:** Mocks проверяют time boundaries, scope/permissions loss, retries и dedup; не реализуется user-account/Open Client connector.

**Зависит от:** [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-ycq.1](#tgsum-ycq.1), [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-cs2.3](#tgsum-cs2.3).

<a id="tgsum-cs2.5"></a>

#### tgsum-cs2.5 — Тестирование VK/MAX: scopes, pagination и recovery

**На момент сверки:** Открыта · проверка.

Synthetic contract suite двух adapters с истёкшими/отозванными tokens, неверными peers, потерей admin и повторными событиями.

**Приёмка:** Проверки воспроизводимы локально; все доступы mock; production compatibility остаётся отдельными controlled QA задачами.

**Зависит от:** [tgsum-cs2.2](#tgsum-cs2.2), [tgsum-cs2.4](#tgsum-cs2.4).

<a id="tgsum-7hv"></a>

### tgsum-7hv — v0.9 — Discord Server Connector

Официальный бот для server channels, history/Gateway и ограниченный собственными сообщениями Data Package importer. Self-bot и полноценные personal DMs вне scope.

**Закрытие эпика:** Все принятые дочерние срезы выполнены; permissions/intents, recovery и own_messages_only отражены в результатах. Bot promotion зависит от QA с пользователем. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-7hv.1"></a>

#### tgsum-7hv.1 — Discord Bot — server channels, history и Gateway

**На момент сверки:** Открыта.

Квалифицировать intents/permissions/app rules; реализовать выбранные каналы, history, Gateway resume/reconciliation и attachment references.

**Приёмка:** Mock events подтверждают identity/recovery/permission loss; self-bot/user tokens не поддерживаются; content limitations и scope видны.

**Зависит от:** [tgsum-ycq.1](#tgsum-ycq.1), [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-2ty.3](#tgsum-2ty.3).

<a id="tgsum-7hv.2"></a>

#### tgsum-7hv.2 — Discord Data Package — архив собственных сообщений

**На момент сверки:** Открыта.

Опциональный importer personal package с native IDs/timestamps и явным own_messages_only; не использовать как полный transcript.

**Приёмка:** Coverage передаётся в preview/recipes; нельзя сформировать обещание полного ретро из own-only корпуса; fixtures проходят.

**Зависит от:** [tgsum-b5v.1](#tgsum-b5v.1), [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-hzm.2](#tgsum-hzm.2), [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-7hv.3"></a>

#### tgsum-7hv.3 — Тестирование Discord: Gateway gaps и ограниченные данные

**На момент сверки:** Открыта · проверка.

Mock Gateway reconnect/resume/history, missing intent, deleted channel, permission changes и own-only archive fixtures.

**Приёмка:** Ни gaps, ни пустой message content не маскируются полнотой; при потере доступа acquisition останавливается; никаких реальных user tokens.

**Зависит от:** [tgsum-7hv.1](#tgsum-7hv.1), [tgsum-7hv.2](#tgsum-7hv.2).

<a id="tgsum-6gf"></a>

### tgsum-6gf — v1.0 — Multi-source Workspace и Update & Analyze

Несколько источников Project, повторяемый анализ, история результатов/evidence, управление хранением и цельный пользовательский lifecycle.

**Закрытие эпика:** Все дочерние задачи выполнены; synthetic multi-source end-to-end проходит; пользователь видит покрытие, получателя и последствия удаления. Квалифицированы только фактически проверенные коннекторы. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-6gf.1"></a>

#### tgsum-6gf.1 — Project с несколькими платформами и объединённым контекстом

**На момент сверки:** Открыта.

Единый Project corpus из нескольких явно выбранных источников, namespaces и coverage per source; participants не склеиваются по одному display name.

**Приёмка:** Telegram+другой synthetic connector дают проверяемый bundle; scope одного источника не расширяет другой; provenance каждого evidence сохранён.

**Зависит от:** [tgsum-ax6](#tgsum-ax6), [tgsum-hzm.4](#tgsum-hzm.4), [tgsum-hzm.1](#tgsum-hzm.1).

<a id="tgsum-6gf.2"></a>

#### tgsum-6gf.2 — Update & Analyze: оркестрация полного цикла

**На момент сверки:** Открыта.

Обновить разрешённые источники, показать diff/coverage/privacy, подготовить bundle и явно запустить выбранный recipe/agent; recoverable ошибки по source.

**Приёмка:** Source failure не даёт ложный полный результат; Run показывает текущий scope/получателя; watcher сам не запускает cloud analysis; повторный запуск идемпотентен.

**Зависит от:** [tgsum-hzm.10](#tgsum-hzm.10), [tgsum-6gf.1](#tgsum-6gf.1), [tgsum-af2.7](#tgsum-af2.7), [tgsum-l4c](#tgsum-l4c).

<a id="tgsum-6gf.3"></a>

#### tgsum-6gf.3 — История analysis runs и переход к evidence revisions

**На момент сверки:** Открыта.

Сохранять snapshot pair, recipe/sanitizer/adapter версии и destination; сравнение результатов и открытие использованной версии сообщения.

**Приёмка:** Поздняя правка сообщения не подменяет evidence старого анализа; failed/cancelled runs явно обозначены и не двигают successful baseline.

**Зависит от:** [tgsum-hzm.9](#tgsum-hzm.9), [tgsum-ax6](#tgsum-ax6).

<a id="tgsum-6gf.4"></a>

#### tgsum-6gf.4 — Retention, удаление Project и управление credentials

**На момент сверки:** Открыта.

Настраиваемые сроки локального хранения, удаление store/bundles/results, отключение refresh и явное управление токенами с учётом общих подключений.

**Приёмка:** Удаление не затрагивает исходный export или другой Project; общий token не отзывается скрыто; уже отправленные внешнему AI данные не объявляются удалёнными.

**Зависит от:** [tgsum-hzm.1](#tgsum-hzm.1), [tgsum-ycq.1](#tgsum-ycq.1), [tgsum-6gf.3](#tgsum-6gf.3).

<a id="tgsum-6gf.5"></a>

#### tgsum-6gf.5 — Состояние источников и локальная диагностика

**На момент сверки:** Открыта.

Last update, counts, partial/gaps/stale, requires action, receiver и доступные recovery actions; технические детали по запросу пользователя.

**Приёмка:** Логи/diagnostic bundle не содержат tokens и сообщений по умолчанию; UI объясняет фактическую проблему и не заявляет zero risk.

**Зависит от:** [tgsum-6gf.1](#tgsum-6gf.1), [tgsum-2ty.2](#tgsum-2ty.2).

<a id="tgsum-6gf.6"></a>

#### tgsum-6gf.6 — Recipes: Customer Voice, Weekly/Release Digest, Risks и Knowledge

**На момент сверки:** Открыта.

Дополнить первые шесть scenarios анализом feedback, weekly delta, blockers/risks и FAQ/runbooks; shared evidence/coverage contract.

**Приёмка:** Synthetic cases требуют evidence и отличают факт, интерпретацию и неизвестное; new-since baseline связан с конкретным успешным run.

**Зависит от:** [tgsum-6gf.3](#tgsum-6gf.3), [tgsum-hzm.9](#tgsum-hzm.9).

<a id="tgsum-6gf.7"></a>

#### tgsum-6gf.7 — Тестирование multi-source lifecycle и удаления данных

**На момент сверки:** Открыта · проверка.

Synthetic E2E: mixed platforms → update → privacy → fake analysis → rerun → evidence → retention/delete; отказ одного connector.

**Приёмка:** Проверены изоляция проектов, исторические ссылки, scope и cleanup; экспорт пользователя и другие проекты не меняются; результаты воспроизводимы.

**Зависит от:** [tgsum-6gf.4](#tgsum-6gf.4), [tgsum-6gf.5](#tgsum-6gf.5), [tgsum-6gf.6](#tgsum-6gf.6), [tgsum-6gf.2](#tgsum-6gf.2).

<a id="tgsum-brk"></a>

### tgsum-brk — После первых adapters — OMP, локальные модели и recipes

Расширение проверенного runner: OMP, локальные backend, custom executable/args и переносимые recipes; не встраивать API всех LLM-провайдеров в core.

**Закрытие эпика:** Все задачи выполнены, единые adapter contract tests проходят, scope/destination и ограничения изоляции известны. Непроверенные режимы остаются выключенными. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-brk.1"></a>

#### tgsum-brk.1 — OMP adapter через проверенный runner

**На момент сверки:** Открыта.

Проверить установленный CLI/версии/profiles и реализовать discovery, local/non-interactive запуск и result contract.

**Приёмка:** Fake CLI suite проходит; provider/destination виден; tokens не копируются из чужих профилей; unsupported controls дают понятный fallback.

**Зависит от:** [tgsum-hzm.6](#tgsum-hzm.6), [tgsum-hzm.12](#tgsum-hzm.12).

<a id="tgsum-brk.2"></a>

#### tgsum-brk.2 — Локальные Ollama, llama.cpp и LM Studio flows

**На момент сверки:** Открыта.

Описать и реализовать поддержанный запуск через OMP/runner или отдельный локальный adapter, без встраивания всех provider SDK.

**Приёмка:** Явный localhost endpoint/executable, offline fixture tests, limits/timeouts/cancel; local destination не выдаётся за cloud и наоборот.

**Зависит от:** [tgsum-brk.1](#tgsum-brk.1).

<a id="tgsum-brk.3"></a>

#### tgsum-brk.3 — Custom executable и аргументы без shell interpolation

**На момент сверки:** Открыта.

Пользовательский adapter по тому же bundle/result/isolation contract; explicit executable/args, environment allowlist и отсутствие eval chat text.

**Приёмка:** Malicious argument/text fixtures не исполняются как shell; доступ ограничен тем же профилем; предупреждение о фактическом destination основано на настройке.

**Зависит от:** [tgsum-hzm.6](#tgsum-hzm.6), [tgsum-hzm.12](#tgsum-hzm.12).

<a id="tgsum-brk.4"></a>

#### tgsum-brk.4 — Версионируемые и переносимые recipe packs

**На момент сверки:** Открыта.

Schema, импорт/экспорт и versioning текстовых recipes; совместимость с agent/result contract, без скрытых hooks/tools/commands.

**Приёмка:** Unknown schema/unsafe capabilities отклоняются; pack не получает права запускать код; результат сохраняет recipe version и evidence requirements.

**Зависит от:** [tgsum-hzm.9](#tgsum-hzm.9).

<a id="tgsum-brk.5"></a>

#### tgsum-brk.5 — Тестирование adapters и recipe packs

**На момент сверки:** Открыта · проверка.

Общая fake CLI/localhost suite: errors, malformed output, streaming/cancel, stdout limits, resource budgets и переносимые recipe schemas.

**Приёмка:** Каждый adapter проходит одинаковый contract и собственные версии; нет вызова настоящих cloud accounts; результаты записаны.

**Зависит от:** [tgsum-brk.2](#tgsum-brk.2), [tgsum-brk.3](#tgsum-brk.3), [tgsum-brk.1](#tgsum-brk.1), [tgsum-brk.4](#tgsum-brk.4).

<a id="tgsum-2ty"></a>

### tgsum-2ty — Сквозной — Research и жизненный цикл поддержки коннекторов

Обязательные источники и review, registry/capabilities, контрактная проверка новых форматов и технический статус в UI. Право пользователя на содержимое приложение не определяет.

**Закрытие эпика:** Все задачи выполнены; заявленная поддержка проверяется по implementation evidence и версии; ai_policy остаётся исследовательскими данными и не становится per-chat legal gate. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-99k"></a>

#### tgsum-99k — Registry: schema, release checks и technical capability loader

**На момент сверки:** Открыта.

Реализовать существующую задачу уникальности IDs/schema/review freshness/evidence и загрузки технических возможностей/version compatibility.

**Приёмка:** CI/release validation различает implemented/qualified/unknown; ai_policy не блокирует локальный анализ; нет скрытого network lookup или удаления по истёкшей дате.

<a id="tgsum-2ty.1"></a>

#### tgsum-2ty.1 — Обязательный research/review цикл и обновление доказательств

**На момент сверки:** Открыта.

Шаблон перед изменением API/auth/archive/client: primary URLs/date/version, scopes, limits, completion, storage/use rules, decision. Повторять 30 дней network/automation и 90 file/local; локальные/CI напоминания.

**Приёмка:** Каждый connector имеет owner, review due и доказательства; недоступный источник не обновляет дату успешной проверки; expiry требует release review, а не отключения local import.

**Зависит от:** [tgsum-99k](#tgsum-99k).

<a id="tgsum-2ty.2"></a>

#### tgsum-2ty.2 — UI технических возможностей и фактического доступа

**На момент сверки:** Открыта.

Connect conversation/Refresh source: acquisition method, реальный authorization grant, selected scope, credentials owner, attachments/coverage и AI destination.

**Приёмка:** OAuth selected UI не выдаётся за scoped token; official-client export не обещает zero risk; implementation detail виден только когда помогает выбрать режим.

**Зависит от:** [tgsum-99k](#tgsum-99k), [tgsum-hzm.4](#tgsum-hzm.4).

<a id="tgsum-2ty.3"></a>

#### tgsum-2ty.3 — Тестирование: единый contract harness для connectors

**На момент сверки:** Открыта · проверка.

Общие fixtures/invariants для identity, coverage, timestamps, missing/deleted, retry/checkpoint и enabled operations; reusable offline suite.

**Приёмка:** Telegram и synthetic second adapter проходят harness; unsupported capabilities не появляются из одного registry flag; новые connectors подключают suite.

**Зависит от:** [tgsum-99k](#tgsum-99k), [tgsum-hzm.4](#tgsum-hzm.4).

<a id="tgsum-t8t"></a>

### tgsum-t8t — Сквозной — Тестирование, квалификация интеграций и выпуск

Отдельные тестовые задачи: desktop/cross-platform, performance/fault cases, реальные fixtures и клиентские PoC под контролем пользователя, packaging/signing и release qualification.

**Закрытие эпика:** Эпик закрывается только после всех принятых дочерних проверок. Отчёты содержат ОС/версии, fixture origin, результат и открытые дефекты. Задачи с user_control_required остаются backlog до участия пользователя; их незавершённость не маскируется green code tests. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-yu3"></a>

#### tgsum-yu3 — Под контролем пользователя: Telegram Desktop PoC — Linux

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Существующий tgsum-yu3: установленный неизменённый официальный клиент, один явно выбранный чат, два штатных экспорта и deterministic diff; delays/cancel/lock/version. Никакого tdata/session/process-memory access. Влияет на Linux automation qualification.

**Приёмка:** Пользователь присутствует и контролирует аккаунт; exact source, завершение, JSON и повторный diff подтверждены; нет другого chat scope. Пока не проведён — Linux driver experimental.

**Зависит от:** [tgsum-i1w.6](#tgsum-i1w.6), [tgsum-i1w.2](#tgsum-i1w.2).

<a id="tgsum-t8t.1"></a>

#### tgsum-t8t.1 — Desktop E2E на Linux, Windows и macOS с synthetic exports

**На момент сверки:** Открыта · проверка.

Отдельная матрица поддержанных ОС: импорт full/single JSON, scope/topics, output, Project/review/result flow. Инструмент GUI-тестирования выбрать по фактической Tauri/WebView поддержке.

**Приёмка:** На каждой заявленной ОС есть воспроизводимый прогон и версии; core tests не выдаются за UI coverage; недоступная среда явно остаётся unverified.

**Зависит от:** [tgsum-hzm.11](#tgsum-hzm.11).

<a id="tgsum-t8t.2"></a>

#### tgsum-t8t.2 — Fault/property/fuzz проверки parser, store и migrations

**На момент сверки:** Открыта · проверка.

Синтетические corrupt/oversized input, identities, truncation, IO failure, concurrent writers, миграции и crash-restart staging cases.

**Приёмка:** Нет silent success/data loss/overwrite; unsupported durability promises не появляются; найденные дефекты заведены и обязательные исправлены.

**Зависит от:** [tgsum-2ty.3](#tgsum-2ty.3), [tgsum-hzm.2](#tgsum-hzm.2).

<a id="tgsum-t8t.3"></a>

#### tgsum-t8t.3 — Регрессионные budgets памяти, времени и диска

**На момент сверки:** Открыта · проверка.

Повторить измеримый benchmark single huge chat/many chats/mixed attachments/diff двух snapshots; определить реальные budgets и отмену при исчерпании ресурсов.

**Приёмка:** Данные synthetic; generation/build вне измерения; отчёт CPU/OS/revision/RSS/time/disk воспроизводим; известный O(one chat) bound отражён в UI/docs.

**Зависит от:** [tgsum-hzm.2](#tgsum-hzm.2), [tgsum-af2.6](#tgsum-af2.6).

<a id="tgsum-t8t.4"></a>

#### tgsum-t8t.4 — Packaging и install/update smoke tests

**На момент сверки:** Открыта · проверка.

Сборки и чистые VM/CI среды Linux packages/AppImage, Windows installer, macOS bundles; первая установка, update и повторный запуск.

**Приёмка:** Приложение запускается и проходит synthetic import; пользовательские Projects сохраняются при update; destructive install-тесты не выполняются в рабочем окружении пользователя.

**Зависит от:** [tgsum-t8t.1](#tgsum-t8t.1).

<a id="tgsum-t8t.5"></a>

#### tgsum-t8t.5 — Code signing и macOS notarization pipeline

**На момент сверки:** Открыта.

Подготовить release pipeline, secure secret references и проверку подписанных artifacts для Windows/macOS. Настоящие сертификаты предоставляет пользователь отдельной задачей.

**Приёмка:** Dry-run/config checks проходят; credentials не попадают в репозиторий/логи; signed/notarized статус заявляется только по проверенному artifact.

**Зависит от:** [tgsum-t8t.4](#tgsum-t8t.4).

<a id="tgsum-t8t.6"></a>

#### tgsum-t8t.6 — Участие пользователя: сертификаты и signing accounts

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Пользователь выбирает/оформляет certificates и Apple/Windows signing доступ; секреты вводятся в поддержанный secret store/CI под его контролем. Влияет на signed desktop release.

**Приёмка:** Доступ настроен без передачи значений в chat/Beads; проверенный подписанный artifact/notarization report приложен; без участия задача остаётся backlog.

**Зависит от:** [tgsum-t8t.5](#tgsum-t8t.5).

<a id="tgsum-t8t.7"></a>

#### tgsum-t8t.7 — Участие пользователя: квалификация реальных архивных fixtures

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Пользователь предоставляет разрешённые локальные обезличенные примеры Telegram full/single/topics/media, WhatsApp locales, Slack, Яндекс, Google Takeout, LINE и Signal Desktop/Android. Пароли/keys локально, содержимое не коммитится.

**Приёмка:** По каждому заявленному profile фиксируются client/version, формат, expected counts/coverage и parser result; отсутствующий fixture остаётся явным gap и не считается проверенным. Влияет на qualification соответствующего importer.

**Зависит от:** [tgsum-t8t.8](#tgsum-t8t.8), [tgsum-b5v.11](#tgsum-b5v.11).

<a id="tgsum-t8t.8"></a>

#### tgsum-t8t.8 — Участие пользователя: получить разрешённые fixtures и доступные среды

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

До квалификации конкретного формата пользователь выбирает разрешённые обезличенные файлы/backup и доступные тестовые ОС. Предоставляет их локально, без публикации сообщений или ключей. Можно передавать по одному источнику, не ждать весь Archive Pack.

**Приёмка:** Для каждого источника есть client/version/format, происхождение и ожидаемый scope либо зафиксирован недостающий input. Наличие файла не означает, что parser уже прошёл qualification; блокируется только зависимый формат.

<a id="tgsum-t8t.9"></a>

#### tgsum-t8t.9 — Под контролем пользователя: Telegram Desktop PoC — Windows

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Тот же официальный-client сценарий на явно выбранной Windows/Telegram версии; account/export действия только с пользователем. Влияет на Windows driver promotion.

**Приёмка:** Два экспорта одного выбранного чата, diff, permissions/lock/cancel и completion проверены; credentials не читаются; результат отдельный от Linux.

**Зависит от:** [tgsum-i1w.6](#tgsum-i1w.6), [tgsum-i1w.3](#tgsum-i1w.3).

<a id="tgsum-t8t.10"></a>

#### tgsum-t8t.10 — Под контролем пользователя: Telegram Desktop PoC — macOS

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Проверка Accessibility permissions, версии официального клиента, выбранного чата и двух экспортов на macOS. Влияет на macOS driver promotion.

**Приёмка:** Есть evidence source selection/completion/diff и отказов permission/lock; ни session DB, ни credentials не читаются.

**Зависит от:** [tgsum-i1w.6](#tgsum-i1w.6), [tgsum-i1w.4](#tgsum-i1w.4).

<a id="tgsum-t8t.11"></a>

#### tgsum-t8t.11 — Под контролем пользователя: Teams tenant qualification

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Пользователь предоставляет тестовый tenant/app permissions и выбранные разговоры; проверить OAuth/grant/history/subscriptions/renewal/revoke. Влияет на Teams production support.

**Приёмка:** Версии/scopes и фактически доступные chats задокументированы; fixture/mock ожидания совпали; непроверенные profiles остаются выключенными.

**Зависит от:** [tgsum-ycq.2](#tgsum-ycq.2).

<a id="tgsum-t8t.12"></a>

#### tgsum-t8t.12 — Под контролем пользователя: Google Chat OAuth qualification

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Тестовое приложение/аккаунт пользователя и явные spaces: grant, history/events, expiry/gap recovery и revoke. Влияет на Google Chat live support.

**Приёмка:** Реальный grant и coverage совпадают с UI/manifest; неподдержанные типы аккаунтов не объявляются поддержанными; нет автономного входа.

**Зависит от:** [tgsum-ycq.3](#tgsum-ycq.3).

<a id="tgsum-t8t.13"></a>

#### tgsum-t8t.13 — Под контролем пользователя: Slack app qualification

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Выбрать реальный internal/distributed app profile, workspace/scopes и delivery transport; проверить история/events/limits/revoke. Влияет на Slack live support.

**Приёмка:** Rate/delivery/distribution ожидания подтверждены на выбранном profile; чужие каналы не читаются; неизвестный profile остаётся research.

**Зависит от:** [tgsum-ycq.4](#tgsum-ycq.4).

<a id="tgsum-t8t.14"></a>

#### tgsum-t8t.14 — Под контролем пользователя: Яндекс Bot qualification

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Корпоративный bot OAuth/регистрация, выбранные чаты, polling/offset и archive mapping проверяются с пользователем. Влияет на Яндекс live/hybrid support.

**Приёмка:** Новые сообщения, restart/gap и grants проверены; identity overlap доказан или остаётся раздельным; tokens не сохраняются в отчёте.

**Зависит от:** [tgsum-ycq.5](#tgsum-ycq.5).

<a id="tgsum-t8t.15"></a>

#### tgsum-t8t.15 — Под контролем пользователя: VK app и scopes qualification

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Действия регистрации/выдачи scopes и account test выполняет пользователь; выбранный peer, history/Long Poll и revoke. Влияет на VK availability.

**Приёмка:** Доступ реально выдаётся выбранному распространяемому app type; acquisition работает в заявленном scope; отрицательный результат не маскируется mock success.

**Зависит от:** [tgsum-cs2.2](#tgsum-cs2.2).

<a id="tgsum-t8t.16"></a>

#### tgsum-t8t.16 — Под контролем пользователя: MAX Bot-admin qualification

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Выданный для интеграции bot token и выбранный admin-chat; history boundaries/updates, permissions loss и revoke. Влияет на MAX Bot support.

**Приёмка:** Реальная граница admin scope и единицы pagination проверены; Open Client/user session не используется; report без secrets.

**Зависит от:** [tgsum-cs2.4](#tgsum-cs2.4).

<a id="tgsum-t8t.17"></a>

#### tgsum-t8t.17 — Под контролем пользователя: Discord Bot qualification

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Тестовый server/bot пользователя с нужными intents/permissions; selected channels, Gateway recovery и content availability. Влияет на Discord Bot support.

**Приёмка:** Grant/intents и history/update поведение подтверждены; self-bot/personal user token не используется; gaps явно отражены.

**Зависит от:** [tgsum-7hv.1](#tgsum-7hv.1), [tgsum-7hv.3](#tgsum-7hv.3).

<a id="tgsum-t8t.18"></a>

#### tgsum-t8t.18 — Под контролем пользователя: LINE Desktop Bridge PoC

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Штатный Save Chat в выбранной версии Desktop, два экспорта, matching/coverage и ручной fallback. Влияет только на заявленный LINE OS profile.

**Приёмка:** Доступная история и ограничения TXT проверены; automation не расширяет scope; неподтверждённые ОС не promoted.

**Зависит от:** [tgsum-b5v.9](#tgsum-b5v.9).

<a id="tgsum-t8t.19"></a>

#### tgsum-t8t.19 — Под контролем пользователя: Codex и Claude Code qualification

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Проверить первые два CLI adapter на выбранных версиях/ОС с пользователем и только synthetic bundle: авторизация, результат, destination и реальная изоляция. Не зависит от реализации OMP/local/custom.

**Приёмка:** Для каждого заявленного agent/OS profile есть отдельный протокол; если isolation не подтверждён, остаётся Export only. Настоящие экспорты мессенджеров не передаются.

**Зависит от:** [tgsum-hzm.8](#tgsum-hzm.8), [tgsum-hzm.7](#tgsum-hzm.7), [tgsum-hzm.12](#tgsum-hzm.12).

<a id="tgsum-t8t.20"></a>

#### tgsum-t8t.20 — Под контролем пользователя: OMP, custom и local models qualification

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Пользователь контролирует OMP/custom executable и выбранную local runtime; передавать только synthetic bundle. Проверить CLI contracts, destination и isolation на заявленных ОС.

**Приёмка:** Версии и profiles записаны; исходные exports/credentials недоступны; cloud/local различимы. Непроверенный профиль остаётся Export only.

**Зависит от:** [tgsum-brk.5](#tgsum-brk.5), [tgsum-hzm.12](#tgsum-hzm.12).

<a id="tgsum-t8t.21"></a>

#### tgsum-t8t.21 — Участие пользователя: приёмка onboarding и Update & Analyze

**На момент сверки:** Открыта · проверка · backlog: нужно участие пользователя.

Пользователь проходит согласованный сценарий PM/retro на synthetic Project; оценивает scope, privacy preview, результаты и recovery, без настоящих messenger accounts.

**Приёмка:** Есть протокол сценариев и решение по обязательным UX дефектам; найденная доработка заведена, важные ошибки устранены перед заявленным release.

**Зависит от:** [tgsum-6gf.7](#tgsum-6gf.7), [tgsum-t8t.1](#tgsum-t8t.1).

<a id="tgsum-t8t.22"></a>

#### tgsum-t8t.22 — Квалификация выпуска и честная матрица поддержки

**На момент сверки:** Открыта · проверка.

Собрать release evidence по каждому объявляемому OS/connector/agent profile: code tests, контролируемые проверки, подпись artifacts, docs и актуальный research. Непроверенные profiles остаются experimental/disabled.

**Приёмка:** Итоговый v1.0 release manifest однозначно показывает tested/unsupported/experimental; нельзя promoted connector одним изменением JSON. Ранние релизы используют подмножество собственных профильных gates и не ждут будущие платформы. Сам publish/merge/release требует отдельного разрешения.

**Зависит от:** [tgsum-99k](#tgsum-99k), [tgsum-t8t.12](#tgsum-t8t.12), [tgsum-t8t.14](#tgsum-t8t.14), [tgsum-t8t.21](#tgsum-t8t.21), [tgsum-2ty.1](#tgsum-2ty.1), [tgsum-t8t.20](#tgsum-t8t.20), [tgsum-t8t.11](#tgsum-t8t.11), [tgsum-t8t.6](#tgsum-t8t.6), [tgsum-t8t.9](#tgsum-t8t.9), [tgsum-yu3](#tgsum-yu3), [tgsum-t8t.16](#tgsum-t8t.16), [tgsum-t8t.13](#tgsum-t8t.13), [tgsum-t8t.4](#tgsum-t8t.4), [tgsum-t8t.18](#tgsum-t8t.18), [tgsum-t8t.10](#tgsum-t8t.10), [tgsum-t8t.15](#tgsum-t8t.15), [tgsum-t8t.2](#tgsum-t8t.2), [tgsum-t8t.17](#tgsum-t8t.17), [tgsum-t8t.7](#tgsum-t8t.7), [tgsum-t8t.19](#tgsum-t8t.19), [tgsum-t8t.1](#tgsum-t8t.1), [tgsum-t8t.3](#tgsum-t8t.3).

<a id="tgsum-506"></a>

### tgsum-506 — Отложено — Исследования и расширения после основного roadmap

Явный backlog из разделов Later: изолированный runtime, MTProto research, Meta/Viber/WeChat/Kakao, PDF/Office/OCR/STT, archives, team policy packs и scheduled analysis.

**Закрытие эпика:** Каждая дочерняя исследовательская задача заканчивается отдельным go/no-go и предлагаемым scope; это не обещание реализации всех экспериментов и не разрешение работать с аккаунтами. Эпик закрывается только после всех задач согласованного scope; optional/later задачи исключаются лишь явным решением пользователя.

<a id="tgsum-506.1"></a>

#### tgsum-506.1 — Research: прямой Telegram MTProto/Takeout — advanced only

**На момент сверки:** Открыта.

Оценить спрос, API/access/AI terms и поддержку отдельного user-client пути после Bridge; не начинать live login или писать production connector в этой задаче.

**Приёмка:** Датированный go/no-go, риски и предлагаемая граница; любой account spike отдельной задачей под контролем пользователя.

**Зависит от:** [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-506.2"></a>

#### tgsum-506.2 — Research: Dedicated Messenger Environment / VM

**На момент сверки:** Открыта.

Неизменённый официальный клиент внутри отдельной graphical session/runtime, только export directory наружу; cost/OS/resource/update модель.

**Приёмка:** Threat model и synthetic prototype показывают filesystem boundary; unattended GUI работоспособность не предполагается; реальный аккаунт только отдельный controlled PoC.

**Зависит от:** [tgsum-i1w.6](#tgsum-i1w.6).

<a id="tgsum-506.3"></a>

#### tgsum-506.3 — Research: Messenger/Instagram официальные data exports

**На момент сверки:** Открыта.

Получить через пользователя format fixtures, проверить completeness/IDs/media и стоимость importer; P3 без обещания готовой поддержки.

**Приёмка:** Есть источник и fixture contract либо явный blocked-by-user input; отдельное решение об implementation scope.

**Зависит от:** [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-506.4"></a>

#### tgsum-506.4 — Research: Viber backup portability и реальный спрос

**На момент сверки:** Открыта.

Проверить пригодность restore-oriented backup для local analysis и отличие bot interactions; не проектировать universal personal API по наличию Bot API.

**Приёмка:** Решение defer/go/no-go с доказательствами формата и спроса; account/phone действия требуют пользователя.

**Зависит от:** [tgsum-2ty.1](#tgsum-2ty.1).

<a id="tgsum-506.5"></a>

#### tgsum-506.5 — Demand gate: WeChat/Kakao

**На момент сверки:** Открыта.

Зафиксировать конкретный запрос аудитории прежде research/parser затрат; при спросе проверить официальные источники получения данных.

**Приёмка:** Решение с use case и next scope либо явное defer; никакого scraping/session extraction.

<a id="tgsum-506.6"></a>

#### tgsum-506.6 — Research: PDF и Office attachments

**На момент сверки:** Открыта.

Локальные readers PDF/DOCX/XLSX: качество, limits, malicious files, licensing и изоляция; не запускать embedded scripts/macros.

**Приёмка:** Synthetic corpus и go/no-go для каждого формата; разрешённый extraction profile предложен отдельно.

**Зависит от:** [tgsum-af2.6](#tgsum-af2.6).

<a id="tgsum-506.7"></a>

#### tgsum-506.7 — Research: локальный OCR изображений

**На момент сверки:** Открыта.

Движки/модели, язык, качество, ресурсы и metadata/evidence linking для выбранных изображений.

**Приёмка:** Offline synthetic benchmark и решение о runtime/download model; никаких скрытых cloud OCR вызовов.

**Зависит от:** [tgsum-af2.6](#tgsum-af2.6).

<a id="tgsum-506.8"></a>

#### tgsum-506.8 — Research: локальная расшифровка voice/audio

**На момент сверки:** Открыта.

Local STT модели, language/quality/resource cost и provenance time ranges; явный выбор файлов.

**Приёмка:** Воспроизводимый synthetic benchmark, limits и go/no-go; transcript связан с source audio и не выдаётся за точную цитату без проверки.

**Зависит от:** [tgsum-af2.6](#tgsum-af2.6).

<a id="tgsum-506.9"></a>

#### tgsum-506.9 — Research: раскрытие архивов вложений по явному выбору

**На момент сверки:** Открыта.

Sandbox extraction, recursion/file-count/size budgets, traversal/symlink/zip-bomb защита. Отдельно от обязательных source ZIP importers.

**Приёмка:** Adversarial synthetic suite и решение по allowlist; по умолчанию архивы вложений остаются закрытыми.

**Зависит от:** [tgsum-af2.9](#tgsum-af2.9).

<a id="tgsum-506.10"></a>

#### tgsum-506.10 — Research: team privacy/retention policy packs

**На момент сверки:** Открыта.

Переносимые policy presets для команды, sensitive terms и retention defaults без немедленного расширения до SSO/DLP SaaS.

**Приёмка:** Предложенный schema/ownership/override UX и migration boundaries согласованы до реализации.

**Зависит от:** [tgsum-6gf.4](#tgsum-6gf.4), [tgsum-af2.7](#tgsum-af2.7).

<a id="tgsum-506.11"></a>

#### tgsum-506.11 — Scheduled analysis как отдельная явная настройка

**На момент сверки:** Открыта.

Исследовать opt-in запуск анализа по расписанию с фиксированным source scope, recipe, destination и budgets; изменение scope требует нового review.

**Приёмка:** Согласованный lifecycle/expiry/cancel и mock schedule tests в предлагаемом срезе; новый архив сам по себе не разрешает cloud Run.

**Зависит от:** [tgsum-i1w.5](#tgsum-i1w.5), [tgsum-6gf.2](#tgsum-6gf.2).

## Индекс задач на тестирование

| ID | Проверка | Режим |
| --- | --- | --- |
| [tgsum-hzm.11](#tgsum-hzm.11) | Тестирование: Project → bundle → fake agent → evidence | Synthetic / mock / локальная среда |
| [tgsum-hzm.12](#tgsum-hzm.12) | Тестирование runner: prompt injection и попытки выхода за scope | Synthetic / mock / локальная среда |
| [tgsum-af2.8](#tgsum-af2.8) | Тестирование sanitizer: качество и стабильность | Synthetic / mock / локальная среда |
| [tgsum-af2.9](#tgsum-af2.9) | Тестирование packager: traversal, symlinks, лимиты и гонки | Synthetic / mock / локальная среда |
| [tgsum-i1w.6](#tgsum-i1w.6) | Тестирование drivers: версии, селекторы и ошибки клиента | Synthetic / mock / локальная среда |
| [tgsum-i1w.7](#tgsum-i1w.7) | Тестирование UI: assisted export и повторный refresh | Synthetic / mock / локальная среда |
| [tgsum-b5v.11](#tgsum-b5v.11) | Тестирование Archive Pack: форматы, encoding, identity и coverage | Synthetic / mock / локальная среда |
| [tgsum-ycq.6](#tgsum-ycq.6) | Тестирование Live Business: mock servers и recovery | Synthetic / mock / локальная среда |
| [tgsum-cs2.5](#tgsum-cs2.5) | Тестирование VK/MAX: scopes, pagination и recovery | Synthetic / mock / локальная среда |
| [tgsum-7hv.3](#tgsum-7hv.3) | Тестирование Discord: Gateway gaps и ограниченные данные | Synthetic / mock / локальная среда |
| [tgsum-6gf.7](#tgsum-6gf.7) | Тестирование multi-source lifecycle и удаления данных | Synthetic / mock / локальная среда |
| [tgsum-brk.5](#tgsum-brk.5) | Тестирование adapters и recipe packs | Synthetic / mock / локальная среда |
| [tgsum-2ty.3](#tgsum-2ty.3) | Тестирование: единый contract harness для connectors | Synthetic / mock / локальная среда |
| [tgsum-yu3](#tgsum-yu3) | Под контролем пользователя: Telegram Desktop PoC — Linux | Только с пользователем; backlog |
| [tgsum-t8t.1](#tgsum-t8t.1) | Desktop E2E на Linux, Windows и macOS с synthetic exports | Synthetic / mock / локальная среда |
| [tgsum-t8t.2](#tgsum-t8t.2) | Fault/property/fuzz проверки parser, store и migrations | Synthetic / mock / локальная среда |
| [tgsum-t8t.3](#tgsum-t8t.3) | Регрессионные budgets памяти, времени и диска | Synthetic / mock / локальная среда |
| [tgsum-t8t.4](#tgsum-t8t.4) | Packaging и install/update smoke tests | Synthetic / mock / локальная среда |
| [tgsum-t8t.6](#tgsum-t8t.6) | Участие пользователя: сертификаты и signing accounts | Только с пользователем; backlog |
| [tgsum-t8t.7](#tgsum-t8t.7) | Участие пользователя: квалификация реальных архивных fixtures | Только с пользователем; backlog |
| [tgsum-t8t.8](#tgsum-t8t.8) | Участие пользователя: получить разрешённые fixtures и доступные среды | Только с пользователем; backlog |
| [tgsum-t8t.9](#tgsum-t8t.9) | Под контролем пользователя: Telegram Desktop PoC — Windows | Только с пользователем; backlog |
| [tgsum-t8t.10](#tgsum-t8t.10) | Под контролем пользователя: Telegram Desktop PoC — macOS | Только с пользователем; backlog |
| [tgsum-t8t.11](#tgsum-t8t.11) | Под контролем пользователя: Teams tenant qualification | Только с пользователем; backlog |
| [tgsum-t8t.12](#tgsum-t8t.12) | Под контролем пользователя: Google Chat OAuth qualification | Только с пользователем; backlog |
| [tgsum-t8t.13](#tgsum-t8t.13) | Под контролем пользователя: Slack app qualification | Только с пользователем; backlog |
| [tgsum-t8t.14](#tgsum-t8t.14) | Под контролем пользователя: Яндекс Bot qualification | Только с пользователем; backlog |
| [tgsum-t8t.15](#tgsum-t8t.15) | Под контролем пользователя: VK app и scopes qualification | Только с пользователем; backlog |
| [tgsum-t8t.16](#tgsum-t8t.16) | Под контролем пользователя: MAX Bot-admin qualification | Только с пользователем; backlog |
| [tgsum-t8t.17](#tgsum-t8t.17) | Под контролем пользователя: Discord Bot qualification | Только с пользователем; backlog |
| [tgsum-t8t.18](#tgsum-t8t.18) | Под контролем пользователя: LINE Desktop Bridge PoC | Только с пользователем; backlog |
| [tgsum-t8t.19](#tgsum-t8t.19) | Под контролем пользователя: Codex и Claude Code qualification | Только с пользователем; backlog |
| [tgsum-t8t.20](#tgsum-t8t.20) | Под контролем пользователя: OMP, custom и local models qualification | Только с пользователем; backlog |
| [tgsum-t8t.21](#tgsum-t8t.21) | Участие пользователя: приёмка onboarding и Update & Analyze | Только с пользователем; backlog |
| [tgsum-t8t.22](#tgsum-t8t.22) | Квалификация выпуска и честная матрица поддержки | Synthetic / mock / локальная среда |

## Проверка полноты этой сверки

- 106 задач имеют ровно один родительский эпик из перечисленных 12.
- У всех эпиков и задач заполнены критерии приёмки.
- Все ссылки зависимостей разрешаются; граф blocks не содержит циклов.
- Все 17 задач roadmap с user_control_required имеют метку backlog.
- В этой сверке не выполнялись аккаунтные проверки и не закрывались задачи реализации.

Для актуального состояния: `bd show <id>`, `bd list --status in_progress`, `bd list --label verification --all --limit 0`. Согласование и поправки записываются в Beads.
