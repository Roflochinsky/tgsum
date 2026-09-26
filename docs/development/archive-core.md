# Telegram archive foundation

Область эпика `tgsum-d9b`: offline core, синтетические fixtures и симулятор
Bridge. Дата проверки: 2026-09-26. Это документация исходников ветки разработки,
не объявление нового desktop-релиза.

## Реализованные интерфейсы

| Интерфейс | Результат |
| --- | --- |
| `index_reader` / `extract_reader` | Полный `chats.list[]` и одиночный корневой чат с `id`/`messages`; прежний Markdown |
| `SnapshotStore::import_telegram` | Сохраняет один явно выбранный чат после проверки всего JSON |
| `SnapshotStore::load` | Читает сохранённый snapshot с проверкой schema, имени, уникальности и scope ключей |
| `Snapshot::diff` | `created`, `edited`, explicit `deleted`, `missing`, счётчик `unchanged` |
| `ExportJob::apply` | Проверяет событие клиента и публикует snapshot только после `Completed` активного запроса |

Исходники: [parser](../../core/src/stream.rs),
[snapshots](../../core/src/snapshot.rs), [Bridge](../../core/src/bridge.rs).

Срез `tgsum-hzm.4` добавил общий [connector contract](connectors.md): Telegram
использует тот же store, что и synthetic второй адаптер; network/OS driver
в этом срезе не подключён.

### Parser

Порядок корневых полей не важен. Пустой `messages: []` и пустой `chats.list: []`
допустимы; неизвестная форма, отсутствующие обязательные поля, смешение двух
форм и повтор ключевых полей дают ошибку. Native chat IDs и snapshot message/
reply IDs принимаются как строки или целые числа, без округления через float.

`extract_reader` сохраняет ранний выход после последнего выбранного чата.
Поэтому он не доказывает корректность оставшегося файла. Snapshot importer
всегда читает до EOF, включая невыбранные чаты, и не использует этот ранний выход.

### Snapshots и diff

Source namespace: `platform + account_local_id + conversation_id`.
Ключ сообщения добавляет `message_id`; имена чатов и текст не являются ключами.
Два одинаковых текста с разными ID сохраняются. Попытка сравнить разные source
namespaces отклоняется. Account namespace задаёт пользователь/Project:
single-chat JSON не позволяет доказать, какой аккаунт его экспортировал.

Один `<snapshot_id>.json` содержит выбранный чат, schema version, coverage и
canonical сообщения: timestamps без выдуманной UTC-конверсии, author, text,
reply/topic, edit metadata, service flag/action/title, attachment references.
Native key вместе с `snapshot_id` указывает на сохранённую версию записи.
Schema 2 добавляет content revisions, provenance и timestamp/coverage metadata:
[контракт и миграция](canonical-schema.md). Непрозрачные evidence IDs и bundle
реализуются отдельно.

Пути вложений не открываются и не копируются. Они получают
`unverified_reference`, а placeholders/неподходящие пути — `unavailable`.
Это не проверка существования файлов или безопасности будущего копирования.
Изменения содержимого медиа с прежними path/size этот diff обнаружить не может.

Импорт фиксирует `coverage: unknown`: валидный JSON не доказывает полноту
истории. `missing` означает отсутствие записи в новом snapshot; не deletion.
Сравниваются сохранённые canonical поля, а не все поля Telegram JSON: реакции
и неизвестные данные пока не входят в эту проекцию. Списки diff отсортированы
лексикографически по строковым IDs; порядок сообщений в snapshot остаётся исходным.
Изменение названия чата не превращает все сообщения в `edited`.

### Публикация и ошибки

Выбранный чат записывается во временный файл внутри private store. Только после
успешного разбора всего исходного JSON, проверки identities и `sync_all` файл
публикуется без замены существующего имени. Недописанный JSON, неверный chat ID,
дубли сообщений/выбранного чата и ошибка чтения не публикуют snapshot.
Повторная и конкурентная запись того же snapshot ID получают `AlreadyExists`.

Используется `tempfile::persist_noclobber`: опубликованное имя указывает на
полностью записанные данные. На системах с link/unlink fallback авария может
оставить дополнительное staging-имя. Power-loss durability каталога и изменение
данных другими локальными процессами этим интерфейсом не гарантируются.
Store должен находиться в приватном каталоге пользователя. Он содержит исходные
несанитизированные сообщения и не передаётся агенту как готовый context bundle.

### Bridge

Запрос содержит уникальный `run_id`, точный source namespace и абсолютный путь
назначенного архива. Driver возвращает тот же запрос вместе с событием.
Несовпадающие run/account/chat/path отклоняются до чтения файла. Это проверка
корреляции событий, не доказательство владельца Telegram-аккаунта.

Состояния: `WaitingForClient`, `NeedsUserAction`, `Exporting`, `Ready`, `Failed`,
`Cancelled`. `Completed` допустим в `Exporting`; ошибка импорта завершает попытку
как `Failed`. После terminal state события отклоняются. Retry создаёт новый job
с новым run ID; запоздалое завершение предыдущего запроса не подходит новому.

Driver обязан подтвердить завершение штатного экспорта. Существование файла,
пауза в записи и даже валидный JSON не заменяют это событие. Сейчас driver —
симулятор; UIA/Accessibility/AT-SPI, выбор чата в настоящем клиенте, watcher,
расписания и Project UI сюда не входят. Синхронная валидация `apply` должна
выполняться на blocking worker будущего desktop host.

## Воспроизводимый пример

Из корня репозитория, с новым каталогом для каждого запуска:

```bash
cargo run --locked -p tgsum-core --example telegram_refresh -- /tmp/tgsum-synthetic-demo
```

На Windows передайте свой путь к новому каталогу. Пример использует только
встроенные synthetic fixtures, записывает `before.json`/`after.json` в store,
заново читает их и печатает diff:

- `created`: message `4`;
- `edited`: message `2`;
- `missing`: message `3`;
- `unchanged`: `2`.

Клиент Telegram не запускается; сеть, сессии и реальные аккаунты не используются.
Повторный запуск с тем же каталогом не перезаписывает результаты.

## Проверка памяти большого чата

Контрольный запуск 2026-09-26: Linux x86_64, Rust 1.98.1, release profile из
workspace, один запуск на вариант. Peak RSS приведён в KiB; время измеряется
самим примером, включая файловые операции. У каждого сообщения 4096 байт текста.

| Операция | Архив | Размер, байт | Время | Peak RSS, KiB |
| --- | --- | ---: | ---: | ---: |
| Index | Один чат, 20 000 сообщений | 84 028 957 | 170 ms | 6 160 |
| Index | Один чат, 100 000 сообщений | 420 188 958 | 848 ms | 23 068 |
| Index | 10 чатов по 10 000 сообщений | 420 089 601 | 850 ms | 4 432 |
| Snapshot | Один чат, 100 000 сообщений | 420 188 958 | 3 159 ms | 521 512 |

Это подтверждает зависимость индекса от размера чата при одинаковом общем
числе сообщений. Snapshot содержит полный текст и ожидаемо требует значительно
больше памяти. Небольшой индекс не означает небольшой RAM budget для snapshot.
Это замеры schema 1 до добавления revision hashes/metadata в schema 2.

Память parser ограничена размером одного чата. Индекс не держит message text;
snapshot importer держит записи одного чата при нормализации, затем освобождает
их до следующего. Diff требует два выбранных snapshots и индексы ключей в памяти.
Это не constant-memory обработка одного бесконечно большого чата.

Release-сборка: `cargo build --locked -p tgsum-core --examples --release`.
`archive_probe index|snapshot <synthetic-export.json>` выводит время выполнения,
а на Linux — peak RSS (`VmHWM` своего процесса). Генерация файла и компиляция
не входят в замер. `snapshot` выбирает chat ID `1` и удаляет свой временный store.

Генератор для воспроизведения (пишет только синтетические данные):

```python
import json
from pathlib import Path

root = Path("/tmp/tgsum-archive-measure")
root.mkdir(exist_ok=True)
for name, chats, count in [("single-20k", 1, 20_000),
                           ("single-100k", 1, 100_000),
                           ("full-10x10k", 10, 10_000)]:
    with (root / f"{name}.json").open("x", buffering=1_048_576) as f:
        if chats > 1:
            f.write('{"chats":{"list":[')
        for chat in range(chats):
            if chat:
                f.write(',')
            f.write('{"messages":[')
            for i in range(count):
                if i:
                    f.write(',')
                json.dump({"id": i + 1, "type": "message",
                           "date": "2026-01-01T10:00:00", "from": "Synthetic",
                           "from_id": "user1", "text": "x" * 4096},
                          f, separators=(',', ':'))
            f.write('],"id":' + str(chat + 1)
                    + ',"name":"Synthetic","type":"personal_chat"}')
        if chats > 1:
            f.write(']}}')
```

```bash
target/release/examples/archive_probe index /tmp/tgsum-archive-measure/single-20k.json
target/release/examples/archive_probe index /tmp/tgsum-archive-measure/single-100k.json
target/release/examples/archive_probe index /tmp/tgsum-archive-measure/full-10x10k.json
target/release/examples/archive_probe snapshot /tmp/tgsum-archive-measure/single-100k.json
```

Это проверка формы потребления памяти на одной Linux-машине, не SLA и не
benchmark реальных аккаунтов. Другие размеры сообщений, темы, диски и ОС
могут дать другие значения. Проверка настоящего Telegram остаётся в backlog
`tgsum-yu3` и проводится только под контролем пользователя.
