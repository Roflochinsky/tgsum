# TGSUM: roadmap context gateway

Принят по уточнениям пользователя 2026-09-26. Версии обозначают целевой scope,
не даты и не обещание готовности внешних интеграций.
[Архитектура](../specs/context-gateway.md), [матрица платформ](../connectors/architecture.md),
[продуктовый принцип](../adr/0002-user-controlled-content.md).
Прогресс и задачи ведутся в Beads; этот документ задаёт порядок и критерии.

## Завершение эпиков и участие пользователя

Эпик завершён, когда выполнены все задачи его согласованного scope и подтверждены
критерии результата. Нельзя закрывать его по одному успешному тесту или наличию
каркаса кода. Возникшее действие, требующее участия пользователя, по его указанию
уходит в backlog с причиной и точным запросом к нему; независимая разработка продолжается.

Проверки с реальными аккаунтами выполняются **только под контролем пользователя**.
В текущей работе используются synthetic fixtures, локальные временные каталоги,
симулятор клиента и контрактные тесты. Нельзя открывать авторизованный Telegram,
инициировать экспорт, читать профиль или делать API-запрос от его имени для теста.

Перенос account verification в backlog не является доказательством совместимости:
кодовый эпик может завершиться в явно ограниченном scope, а production claim
Bridge остаётся неподтверждённым до контролируемого PoC. Каждая такая задача
сохраняет связь с выпуском/интеграцией, которую она проверяет.

## Версии

| Версия | Результат | Критерий выпуска |
| --- | --- | --- |
| **v0.3 Context Engine** | Project; canonical conversation/message/attachment; evidence; snapshot store/diff; date range/new since analysis; connector/agent interfaces; Export only; Codex/Claude adapters; recipes; onboarding/re-run | Telegram остаётся единственной реализуемой платформой. Legacy export совместим; повторные snapshots дают проверяемый diff. Минимальный preview/scope/secrets слой включён перед первым cloud Run; непроверенный adapter остаётся выключенным |
| **v0.4 Privacy & Files** | Расширенные secrets/PII/infrastructure rules, deterministic pseudonyms, company terms, Privacy Preview; выбранные text/log/config/code files | Проверяемые замены, приватный mapping, ограниченный bundle и корректная работа с путями/лимитами |
| **v0.5 Telegram 2.0** | Per-chat JSON, reference Official Client Bridge, remembered sources, repeat export/diff; manual и active-session refresh | Manual/assisted путь работает; автоматизация остаётся experimental до контролируемого PoC на конкретной ОС/версии. VM/headless не условие выпуска |
| **v0.6 Archive Pack** | WhatsApp, Slack, Yandex; Signal Desktop/Android backup и LINE по готовности fixtures; повторные snapshots | Для каждого формата доказаны структура, identity и coverage. Scheduled Slack export не считается автоматически доставленным на диск |
| **v0.7 Live Business** | Teams, Google Chat, Slack, Yandex Bot по одному вертикальному потоку | История + updates + gaps/revoke проверены; реальный grant отражён в UI; distribution и доставка событий решены |
| **v0.8 RU Pack** | VK qualification/connector и MAX bot-admin history/updates | API scopes доступны выбранному типу приложения; MAX Open Client не используется |
| **v0.9 Discord** | Server bot history/Gateway; optional own-messages archive | Permissions/intents и recovery проверены. Полноценные пользовательские DMs не обещаются; self-bot исключён |
| **v1.0 Workspace** | Несколько источников Project, Update & Analyze, история анализов/evidence, privacy и выбранный агент | Проверенный lifecycle source → scope → context → analysis → repeat; честная полнота и управление данными |

Расширенные privacy-функции идут в v0.4; минимальный preview перед передачей
данных нужен уже первому работающему agent adapter. Номер версии не откладывает
эту зависимость. OMP/local/custom adapters добавляются после первого проверенного
контракта runner и не требуют встраивать API всех LLM-провайдеров в core.

## Telegram Bridge раньше большинства API-интеграций

Основной PoC использует **установленный неизменённый Telegram Desktop** пользователя.
Dedicated VM/runtime — последующий advanced профиль. OS automation рассматривает
Windows UI Automation, macOS Accessibility, Linux AT-SPI только после проверки
доступности нужных элементов; один интерфейс не гарантирует одинаковую реализацию.

Контролируемая проверка: один заранее выбранный тестовый чат → официальный
export → подтверждённый JSON → второй export → deterministic created/edited/unchanged
по scoped message keys. Session DB, credentials, process memory не читаются.
Этот account PoC сейчас находится в backlog; код parser/store/diff/bridge contract
разрабатывается и тестируется автономно на synthetic data.

Три уровня продукта: **Import export → Assisted export → Experimental automatic
export**. Refresh now может требовать пользовательского действия и активной GUI
сессии; слово «всегда» не является технической гарантией. Далее — on-start/daily/6h
при разблокированном компьютере и явной настройке пользователя. Не рекламировать
частоту как опубликованный Telegram «безопасный лимит».

## За текущей очередью

Прямой Telegram MTProto — advanced research, вне ближайших релизов. Viber отложен.
Meta Messenger/Instagram — P3 после fixtures. WeChat/Kakao — после реального спроса.
Экспортировать через GUI можно только там, где такая функция официально существует;
универсальный Bridge не создаёт её для WhatsApp Desktop/VK автоматически.
