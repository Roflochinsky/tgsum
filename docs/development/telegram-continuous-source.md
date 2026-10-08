# Хранилище постоянного источника Telegram

Состояние 2026-10-08: реализован и независимо проверен **foundation хранения**.
Project apply, локальный пакет, Tauri UI, управление клиентом и фоновый запуск
ещё не подключены. Этот документ не объявляет постоянную автоматизацию готовой.
Registry остаётся `research`, без enabled operations.
Контракт: [спецификация](../specs/telegram-continuous-source.md).
[Публичное evidence](evidence/telegram-continuous-foundation-2026-10-08.json)
содержит hashes и синтетические результаты без личных данных.

## Реализованная часть

Linux-only module `core/src/telegram_debug/continuous.rs` предоставляет
`ContinuousCapture::open`, `poll`, `status`, `events_after`, `latest_message`
и `acknowledge`. Общий follower с экспериментальным CLI читает только регулярные
`mtp_HH_MM.txt` в явно выбранной `DebugLogs`. Namespace аккаунта допускает UTF-8;
typed peer и native message/sender IDs сохраняются строками без округления.
Подтверждение одного аккаунта обязательно; account switching не квалифицирован.

Output — новая приватная директория, содержащая `observations.redb`, `owner.json`
и `initialized`. Она связана с account/peer/input path/client version и inode DB.
Чужой существующий output не присваивается. Перед recovery проверяются marker,
UID, regular file, отсутствие links, права и блокировка; записи используют
удерживаемый DB handle. Каталог и его родитель синхронизируются.
Незавершённый seal дополняется только если его bytes — prefix ожидаемого hash
и восстановленная DB подтверждает исходный binding. Неопознаваемые остатки
первого создания сохраняются и отклоняются, без очистки/adoption чужих файлов.

Events/dedup/latest/tombstones/cursors фиксируются одной redb transaction
с Immediate durability. Лимит `CaptureLimits.max_events` относится к старому
JSON spike; durable backend не останавливается после 10 000 событий и не читает
всю историю в RAM. `max_state_bytes` также относится к JSON spike; metadata
durable backend ограничена 128 KiB. File/packet/poll bounds остаются общими.

Poll читает до 16 MiB по умолчанию (packet до 2 MiB), проверяет consumed range
повторным hash через buffer 8 KiB и сохраняет fingerprints считанных bytes.
Переписывание файла во время poll отменяет события/cursor и фиксируется отдельным
gap. Round-robin позиция между файлами сохраняется, чтобы backlog не задерживал
текущий лог бесконечно. Слишком большой packet возвращает явную Capacity error;
caller обязан остановить или показать ошибку, не повторять её в busy loop.

Read page ограничена одновременно 512 событиями и 32 MiB serialized bytes;
read transaction не удерживается между страницами. Cache — 8 MiB. Metadata
allocator redb может расти с размером DB; постоянный общий RSS не обещается.
Disk history не очищается. Tombstones не удаляют сохранённые версии сообщений.
Late history не отменяет более новый edit; одинаковые seconds сохраняют
ограничение порядка исходных transport observations.

`sequence` считает сохранённые события, `observation_revision` также учитывает
gap-only изменения. `AppliedCheckpoint` связывает обе позиции с опубликованным
snapshot ID. Повтор идентичного ack идемпотентен, уменьшение любой позиции или
смена snapshot на той же паре запрещены. Caller подтверждает ack **после**
Project CAS, а не до него. После I/O error poll/ack session останавливается;
reopen использует фактически recovered checkpoint. Failed sync не объявляется
доказанным rollback.

## Проверка

```sh
cargo test --locked -p tgsum-core --lib telegram_debug
bash scripts/check.sh quick
bash scripts/check.sh
git diff --check
```

Focused suite: 42 passed; один ignored subprocess helper вызывается самим
crash-тестом. В новом backend 14 действующих тестов: 10 005 событий и restart,
точные IDs/UTF-8, partial tail, monotonic/gap-only ack, late edit/tombstone,
transaction rollback, чужой output, links/path replacement включая DB Drop,
rewrite между read и fingerprints, fairness/rotation, partial seal, bytebudget,
SIGKILL до/после commit и injected write/sync failure. Старые parser/capture
регрессии остаются зелёными.

В отдельном disposable workspace возвращены lifetime cap и удалена проверка
полного consumed range. Оба behavioral tests стали RED после успешной компиляции;
исходное дерево не менялось. Временный workspace/build artifacts убраны.
Независимый verifier дополнительно проверил 17 сообщений по 1,99 MB:
3 poll, страницы 16+1, восстановление seal 0/8 bytes без потери events.
Это синтетические проверки, не real-account qualification или power-loss test.

Для воспроизведения использовать только искусственные данные. Реальные логи,
account IDs, payload и личные пути не публикуются. Хранилище не вызывает Telegram
API, не управляет клиентом, не делает export/download и не отправляет данные AI.

## Оставшаяся интеграция и rollback

Project apply должен сохранять bootstrap/media/replies/topics, выдерживать
snapshot/CAS/ack crash boundaries и обновлять local package из актуального
snapshot. UI/background lifecycle и installed live evidence обязательны до
promotion. Сбор реального нового сообщения, edits/deletes и полнота не доказаны
этими fixtures.

На этом этапе установленный TGSUM, Telegram и пользовательские настройки не
изменены. Для отмены foundation использовать обычный Git revert соответствующего
commit; новые private journals сохраняются. Не удалять исходные логи, журнал или
marker как способ rollback. Будущая установка требует отдельного backup и
команды восстановления установленного binary/config.
