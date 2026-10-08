# Хранилище постоянного источника Telegram

Состояние 2026-10-08: реализованы хранение, применение к Project и сборка
локального пакета из актуальной истории. Хранение и новый проход Project/package
прошли отдельную независимую проверку. Tauri Start/Stop UI,
worker, управление клиентом и фоновый запуск ещё не подключены. Этот документ
не объявляет постоянную автоматизацию готовой.
Registry остаётся `research`, без enabled operations.
Контракт: [спецификация](../specs/telegram-continuous-source.md).
[Публичное evidence](evidence/telegram-continuous-foundation-2026-10-08.json)
содержит hashes и синтетические результаты без личных данных.
[Project/package evidence](evidence/telegram-continuous-project-2026-10-08.json)
содержит итоговые hashes, checks и отдельные regression/independent fixtures.

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

## Project и локальный результат

Project schema 10 сохраняет `telegram_continuous` отдельно от старого
`telegram_refresh`. Schema 1–9 открываются без изменения старых revision-файлов;
непустой continuous plan под старым номером schema отвергается. Настройки
связывают source, typed peer, подтверждение одного аккаунта, директорию логов,
поколение журнала и bootstrap snapshot. Настройки не содержат credentials.
Тип native peer проверяется по виду чата в bootstrap; account namespace берётся
из SourceScope. Новая привязка не подменяет уже сохранённое поколение.

Linux `ProjectStore::open_telegram_capture` открывает приватный журнал внутри
Project. `apply_telegram_observations` применяет одну страницу, сохраняет новую
immutable observation, синхронизирует каталог snapshot, затем одним Project CAS
публикует pointer и frontier. После этого подтверждает позицию журналу. Сбой до
CAS повторно использует тот же snapshot после проверки содержимого; сбой после
CAS до ack восстанавливает ack без новой Project revision. Перед recovery ack
повторяется barrier snapshot file/dir, revision file/dir и Project dir: видимый
после crash файл ещё не доказывает успешный предыдущий fsync. Ошибка barrier
не подтверждает позицию журнала. Stop и конкурентный
edit инвалидируют stale CAS. Исходные snapshot/journal не удаляются.

При обработке страницы используется её содержимое, а не future latest-index
журнала. Последняя страница подтверждает poll revision и gaps. Сохраняются
bootstrap media/replies/topics, исходное время, текст и тела удалённых сообщений;
явно наблюдаемый delete меняет deletion metadata. Missing ничего не удаляет.
Оригинальное время и edit date разделены, native reply/thread fields читаются
из закреплённого layer 229 [api.tl](https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/mtproto/scheme/api.tl)
(прочитан 2026-10-08); cross-chat reply не превращается в локальный ID ответа.
Это не квалификация полноты всех видов forum/service/media update.

Имена берутся только из известных bootstrap sender IDs. Неизвестному sender ID
имя не придумывается. Новые наблюдения имеют `Partial` coverage и provenance
`telegram_debug_observation`; импорт исходного JSON остаётся отдельным фактом.
Journal read page ограничена; построение snapshot использует память для одной
выбранной переписки, как существующий canonical pipeline. Размер этой переписки
не объявляется постоянным общим RSS.

Локальный пакет использует latest snapshot для источников с retained continuous
plan, включая остановленные. Для остальных выбранных чатов общий JSON по-прежнему
импортируется. Live-only сборка не требует наличия JSON; bootstrap media читаются
из явно выбранной папки; отдельный media root сохраняется в plan после очистки
generated text choices. Выключенные media options не требуют открывать отсутствующую
папку. Fingerprint включает актуальное содержание/coverage и исходные
вложения. Старый JSON не может заменить continuous history ни через package,
ни через обычный RecordSnapshot/source refresh.

Receipt и manifest пакета с диагностическими наблюдениями имеют `local_only`.
Desktop не запускает публикацию такого результата. Publisher повторно проверяет
retained binding и receipt до сети и перед push. Сохранённая настройка GitHub
не удаляется. Это локальный scope текущей задачи, не юридическое решение за
пользователя. Старые receipts/manifests читаются с `local_only=false`.

Новый synthetic suite проверяет сохранение истории/media/точных IDs, late edit и
delete, две crash boundaries, CAS/Stop, pages/gap-only frontier, чужой journal и
conflicting snapshot, schema migration, local-only и mixed package, исходное
время, forum reply и General (включая bootstrap только с topic marker).
Установленный worker этим suite не проверяется.

Для воспроизведения использовать только искусственные данные. Реальные логи,
account IDs, payload и личные пути не публикуются. Хранилище не вызывает Telegram
API, не управляет клиентом, не делает export/download и не отправляет данные AI.

## Оставшаяся интеграция и rollback

UI/background lifecycle и installed live evidence обязательны до
promotion. Сбор реального нового сообщения, edits/deletes и полнота не доказаны
этими fixtures.

На этом этапе установленный TGSUM, Telegram и пользовательские настройки не
изменены. Для отмены foundation использовать обычный Git revert соответствующего
commit; новые private journals сохраняются. Не удалять исходные логи, журнал или
marker как способ rollback. Будущая установка требует отдельного backup и
команды восстановления установленного binary/config.
