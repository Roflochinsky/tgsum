# Connector architecture

План на 2026-09-26. Факты и ссылки: [проверка платформ](../research/connector-support-2026-09-26.md),
[Telegram Desktop](../research/telegram-export-automation.md).
Принцип выбора: [ADR-0002](../adr/0002-user-controlled-content.md).
Ни один новый connector этой таблицей не объявляется реализованным.

Сложность — предварительная инженерная оценка, без сроков: **S** — формат/fixtures,
**M** — несколько форматов или stateful sync, **L** — авторизация/distribution/UI
automation либо новый encrypted format. Риск аккаунта — описание механизма,
не измеренная вероятность бана. Файловый импорт не вызывает платформенный API;
создание самого архива официальным клиентом может обращаться к платформе.

| Platform | Bootstrap | Incremental | Official-client automation | Auth | Account-ban risk | Attachments | Complexity |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **Telegram** | Per-chat JSON; full JSON fallback | Repeat snapshot, native-ID/revision diff | Preferred после PoC; assisted сначала, active-session automation затем; VM advanced | Сессия у настоящего Desktop; Bridge получает файлы | Нет стороннего API-входа TGSUM; export delays и unattended automation остаются | Выбранные локальные файлы; полноту проверять | S import / L Bridge |
| **MAX** | Bot `GET /messages` в admin chats | Updates после проверки + reconciliation | Подходящий официальный user export не подтверждён | Bot token, admin membership | Token/platform enforcement; user client не используем | API representation/download проверить | L |
| **VK** | User API history, если scopes доступны приложению | Long Poll + восстановление gaps | Пригодный export flow не подтверждён | Новый явно выданный OAuth token | Выдача доступа и действующие ограничения не подтверждены | History/API metadata, download требует проверки | L |
| **Яндекс** | Password-protected archive | Bot polling; mapping/overlap проверить | Заказ/скачивание архива пользователем; auto-download не подтверждён | Для файла password; для live bot OAuth | Файл без API-вызовов TGSUM; bot token может ограничиваться | Fixture/API validation | M archive / L hybrid |
| **Slack** | JSON ZIP по доступным export permissions | Repeat export diff или Events | Есть scheduled exports у eligible workspace; автоматическую доставку ZIP на диск не предполагаем | Файл без auth; API OAuth, Socket Mode ещё app token | API rules/rate limits; internal и distributed app различаются | Часто ссылки, не включённые bytes | M archive / L live |
| **Teams** | Graph history; admin Export API отдельно | Selected-chat subscriptions + recovery | Bridge не нужен для основного API-сценария | Delegated OAuth; RSC/application профили отдельно | API/tenant restrictions; selected subscription не сужает Chat.Read token | References/files могут требовать других прав | L |
| **Google Chat** | API; Takeout для реально доступного scope | Space events до 28 дней + reconciliation | Bridge не нужен для основного API-сценария | User OAuth; файл без auth | API/app/admin restrictions; не обещать grant на один space | API/export representation зависит от источника | L |
| **WhatsApp Personal** | Per-chat TXT/media export | Repeat import; TXT matching с неопределённостью | Android/iOS share/export; Desktop automation не подтверждена | Только предоставленные файлы | TGSUM не входит в аккаунт; automation позже и только по доказательствам | Только реально экспортированные media | M |
| **Discord** | Bot channel history; Data Package лишь secondary own messages | Gateway + history recovery | Неполный Data Package не заменяет server connector | Bot token + permissions/intents | Official bot constraints; self-bot исключён | References/CDN, content intent и доступ проверить | L |
| **Signal** | Official Desktop/Android on-device backup | New backup snapshot diff после проверки identity | Backup UI подтверждён; Android schedule документирован, Desktop automation не проверена | Recovery key предоставляется локально; messenger session не нужна | TGSUM читает файл, не входит в аккаунт | Backup содержит доступные media; формат/покрытие проверить | L parser/decryption / M refresh |
| **LINE** | Per-chat TXT | Repeat Save Chat; identity heuristic | Desktop Save Chat есть; automation — PoC | Сессия остаётся у клиента; importer читает файл | Нет стороннего входа; надёжность UI automation неизвестна | Полноценные media в TXT не обещаны | M import / L Bridge |

## Существенные ограничения

- **Telegram:** не полагаться на date filter клиента как единственный delta
  механизм. Локальный diff работает над фактически полученными IDs/revisions;
  date range применяется также локально. «Полный» означает доказанное покрытие
  выбранного snapshot, а не наличие всех исторических/удалённых сообщений.
- **TXT:** native IDs могут отсутствовать. Повтор одинакового текста не означает
  дубль. Неуверенный match сохраняется явно, без удаления данных.
- **Slack:** scheduled export создаёт архив на стороне платформы; путь доставки
  на локальный диск — самостоятельная проверяемая часть acquisition.
- **Яндекс:** webhook гарантирует порядок bot+chat, но недоставленные события
  удаляются через 24 часа. Это не гарантия полноты при выключенном компьютере.
  [Webhook](https://yandex.ru/dev/messenger/doc/ru/api-requests/update-webhook).
- **Signal:** официальный Desktop Backup подтверждён; recovery key и сохранение
  в выбранную папку описаны в справке. Новый Android формат — каталог, старый
  `.backup` отдельно. Parser/decryption и совпадение форматов ещё не проверены.
  [Desktop](https://support.signal.org/hc/en-us/articles/10870366816410-Signal-Desktop-Backups),
  [Android](https://support.signal.org/hc/en-us/articles/10066926526362-Android-On-device-Backups).
- **Credentials:** обещание Bridge — не читать session DB, `tdata`, cookies,
  память клиента или существующие browser profiles. OAuth/bot используют только
  предназначенные интеграции токены с явным управлением пользователя.

## Coverage contract

`complete | partial | own_messages_only | future_only | unknown` хранится в
snapshot вместе с диапазоном, gaps и основанием оценки. Connector manifest
описывает возможности, snapshot — фактическое покрытие. Для смешанного Project
ограничения каждого источника видны в preview и передаются recipe.

## Очередность

Сначала context engine на Telegram, затем privacy/files и reference Bridge.
Первая продуктовая шестёрка: Telegram → WhatsApp → Slack → Teams → Google Chat
→ Яндекс. Signal/LINE входят в archive pack по готовности fixtures и спросу;
VK/MAX/Discord идут после основания и квалификации API.
Версии и критерии поставки — [roadmap](../plans/context-gateway-roadmap.md).
