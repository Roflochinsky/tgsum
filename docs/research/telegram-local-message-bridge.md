# Локальный приём сообщений из штатного Telegram Desktop

Дата: **2026-10-08**. Beads: `tgsum-i1w.11`. Scope: Arch/Omarchy, локальный TGSUM.
**Исследование и предложение эксперимента; сборщик не реализован.**
Применён `mattpocock-skills:research`: фоновый разбор первичных исходников.
Проверены Desktop **v7.2.5**, commit `2f41383dddd338fe17fd4711afd02688c418fd47`,
и его `lib_tl` — `29746d31247c915a9ec870241399a9d103c39f87`.
На хосте 2026-10-08 подтверждён пакет `telegram-desktop 7.2.5-1`; его фактические
диагностические записи не проверялись. Совпадение версии не заменяет такую проверку.

## Предложение

Первым проверить **локальный сбор диагностических MTP-логов штатного Desktop**.
В исходниках есть режим записи уже расшифрованных входящих пакетов с текстом
и Telegram IDs. Внешний сборщик читает эти файлы, выделяет сообщения выбранного
чата и сохраняет их в TGSUM. Он не входит в Telegram, не вызывает его API,
не использует бота, не читает базу авторизации и не требует своей сборки клиента.
Это вывод о доступном механизме исходника, а не проверенная поддержка аккаунта.

```text
Штатный Desktop → общие диагностические файлы → локальный parser
                                               → фильтр чатов → TGSUM
```

**Существенная цена:** общие файлы содержат данные ДО фильтра выбранных чатов.
Поэтому для исходных логов невозможно обещать «на диск попадают только выбранные
обсуждения». Если это обязательное условие, такой способ не подходит.

## Сравнение

| Способ | Получаемые данные | Граница |
| --- | --- | --- |
| Уведомления | Часть новых сообщений | Обычный Notify без native IDs, preview до 255 UTF-16; пропуски. [Разбор](telegram-notifications-linux.md) |
| JSON export через UI | История, native IDs, полный текст в scope экспорта | Ручной fallback; автоматический Linux-драйвер не квалифицирован. [Разбор](telegram-linux-bridge-2026-09-27.md) |
| Диагностические логи | Расшифрованные входящие TL-объекты, в том числе текст и IDs | Первый кандидат без API-регистрации/сборки; глобальные чувствительные логи, нестабильный формат |
| Patch Desktop | Возможен выбор чатов до записи и собственный формат событий | Своя сборка/обновления и API-реквизиты; запасной вариант, не отвечает строгому требованию «без API-регистрации». [Сборка][build], [реквизиты][credentials] |

## Подтверждённая цепочка в исходниках

| Участок | Проверенный факт |
| --- | --- |
| `core/launcher.cpp` | Аргумент `-debug` устанавливает `gDebugMode`; `ComputeDebugMode()` включает `Logs::DebugEnabled()`. Этот путь не ограничен сборкой `_DEBUG`. [launcher][launcher] |
| `logs.h` | `MTP_LOG` проверяет runtime-флаг и вызывает `Logs::writeMtp`; это не выключаемый release-сборкой макрос. [logs.h][log-h] |
| `mtproto/session_private.cpp` | После `aesIgeDecrypt`, проверок целостности и session пишет `Recv: ` + `DumpToText(...)`. Запись стоит до регистрации duplicate message IDs и обработки обновлений приложением. Там же `Send:` выводится до шифрования. [transport][transport] |
| `mtproto_dump_to_text.cpp/.h` | UTF-8 string выводится целиком с escaping, числа — текстом, `gzip_packed` распаковывается. Буфер растёт; лимита preview 255 здесь нет. Произвольные binary strings могут сокращаться. [dumper][dump], [buffer][dump-h] |
| `codegen_scheme.py` → `lib_tl/generate_tl.py` | Включена генерация dump-to-text; `addTextSerialize` обходит поля TL-схемы. Нет фильтра по выбранному чату или маскирования полей по имени. [Конфигурация][codegen], [генератор][generator] |
| `mtproto/scheme/api.tl` | `message` содержит `id`, `peer_id`, `from_id?`, `date`, `message`, `entities?`; new/edit несут message, delete — IDs. Это позволяет получать native identity из записи, не угадывать её по имени окна. [schema][schema] |
| `logs.cpp` | `writeMtp` пишет UTF-8 в `cWorkingDir()/DebugLogs/mtp_HH_MM.txt`, с временем, thread/entry и DC-метаданными. [Файловая запись][logs] |

`/usr/bin/Telegram -debug` — **предлагаемая команда будущего контролируемого
перезапуска того же клиента с тем же профилем**; сейчас не выполнялась.
`pacman -Q/-Ql telegram-desktop` подтвердил `7.2.5-1` и `/usr/bin/Telegram`.
Работу режима, поведение уже запущенного экземпляра, выключение и фактическую
папку логов ещё надо проверить. Читать/создавать `tdata/withdebug` внешним
инструментом для этого предложения не требуется.

## Какие данные возможны и чего нельзя обещать

Обычный message несёт native peer/message/sender-поля; имя отправителя может
потребовать связанных user/chat-объектов. Sender peer может быть каналом или
анонимным автором, а не установленным человеком. Для service/media сообщений
нельзя автоматически обещать обычный полный текст или оригинальные вложения.
[TL-схема][schema]

Лог содержит и сетевые ответы, и updates, включая containers/gzip. Одной строки
`Recv` недостаточно для утверждения «новое сообщение принято приложением».
Нужно различать историю, повторы, edits и служебные события. Short updates
требуют восстановления полей; исходящие связывают local/random ID с серверным
через `updateMessageID`/ответы отправки. Уже существующий Desktop выполняет
эту нормализацию в `Api::Updates`; parser должен проверить её необходимую часть
на fixtures, а не считать любой распознанный объект окончательным. [updates][updates]

Для non-channel delete в событии нет peer. Разрешение возможно по ранее
сохранённому индексу `account namespace + message ID → typed peer`.
Неизвестную связь оставить unresolved при обработке, не приписывать случайному
чату; в результат выбранных чатов передавать лишь признак непроверенной полноты
без чужих IDs. Channel delete содержит channel. [TL-схема][schema]

## Первый эксперимент: один аккаунт, только текст

Это **проектные решения**, ещё не поведение TGSUM:

1. Под контролем пользователя подтвердить, что источник обслуживает ровно один
   аккаунт; привязать его к явно заданному локальному `account namespace`.
   Не объявлять `dc`, `key` или transport `session` идентификатором аккаунта.
   В suffix `key` — `keyId`, не ключ авторизации; DC общий для многих аккаунтов.
   Внутри Desktop self user доступен через `Main::Session`, но его надёжное
   сопоставление с каждым transport-потоком из логов не доказано. [transport][transport],
   [self user][account]
2. Не поддерживать multi-account в первом эксперименте. Если единственный аккаунт
   не подтверждён, источник не подключать; при обнаруженной неоднозначности
   приостановить импорт. Переключение/добавление аккаунта — отдельный тест и
   возможный gap, а не повод молча продолжить под старым namespace.
3. Строить типизированный parser закреплённой версии dump-to-text/TL. Ограничить
   размер record, глубину, vectors и память. Lexer учитывает кавычки и escapes:
   dumper экранирует backslash, quote и LF, но не все controls/CR; поиск `Recv`
   регулярным выражением внутри произвольного текста ненадёжен. [dumper][dump]
   Неполная запись, `[ERROR]`, неизвестные версия/конструктор или превышение лимита
   дают ошибку покрытия. Не выполнять текст и не объявлять усечённое полным.
4. Сохранять только отфильтрованные события: native IDs строками, typed peer,
   sender peer при наличии, text/entities, source time, observed time и provenance.
   Имена — подписи. Два одинаковых текста сохраняются как разные сообщения.
   Фото/файлы не скачиваются; облачная передача отсутствует.
5. Хранить checkpoint чтения с поколением файла, offset и отпечатком; атомарно
   фиксировать событие и checkpoint. Отличать повторы после restart от отдельных
   сообщений и сохранять версии edits. Source log остаётся внешним файлом,
   его нельзя автоматически «очищать» или переписывать.

## Глобальные логи и чувствительные поля

Режим пишет не только выбранный чат и не только `Recv`. TL содержит, например,
`auth.signIn` с `phone_number`, `phone_code_hash`, `phone_code?`; общий dumper
не маскирует эти поля. Поэтому при соответствующей активности в исходных логах
могут оказаться данные входа и посторонняя переписка. Отсечение `Send` на входе
TGSUM не удаляет уже созданный Telegram файл. [Send][transport], [schema][schema],
[генератор][generator]

Исходные логи нельзя включать в Git, Beads, диагностические вложения или AI bundle.
Для реального spike заранее нужны ограниченная длительность, проверенные права
каталога, лимит места и понятное выключение режима. Это не даёт изоляции от
других процессов того же пользователя. Retention/удаление исходных файлов
не меняются автоматически; rollback режима и обработка оставшихся логов должны
быть согласованы до включения. Реальные логи в этом исследовании не читались.

## Ротация, остановка и полнота

`reopenDebug()` выбирает файл по 15-минутному интервалу суток. При несовпадении
dayIndex ранее существующий одноимённый файл открывается без Append и может
быть усечён. `multipleInstances()` отключает debug logging. Запись вызывает
`write`/`flush`, но это не `fsync` и не подтверждение от сборщика. [logs][logs]

Parser обязан обнаруживать известную смену поколения/усечение, restart, пропущенные
файлы и ошибку чтения, но отсутствие видимой ошибки не доказывает отсутствие потерь.
Gap имеет диапазон, только если границы известны; иначе — неизвестную протяжённость.
Спящий/выключенный клиент не записывает новые события. Клиентский difference может
вернуть часть пропущенного, но `channelDifferenceTooLong` обновляет ограниченное
состояние, а обычный `differenceTooLong` в этой версии не поддержан. Полная история
после долгого отсутствия не гарантируется. [updates][updates]

Для начальной истории остаётся ручной JSON export. В будущем можно начать сбор,
сделать export и сверить перекрытие по native IDs/revisions; account namespace
архива необходимо явно связать с источником. Стык, пропуски и изменения должны
быть проверены; отсутствие записи не означает удаление. Такой сбор не обещает
«все сообщения» или автоматическое получение всей старой переписки.

## Следующие проверки и решение о поддержке

1. **Без аккаунта:** сгенерировать fixtures по закреплённому dumper: длинный текст,
   emoji, containers, short/new/edit/delete, исходящие ответы и одинаковые IDs
   в разных namespaces. Намеренные truncation/потеря edit/дубль делают тест красным.
2. **Симулятор файлов:** дописывание, restart, quarter-hour rollover, усечение на
   новом дне, неизвестный format, переполнение и disk failure. Проверить bounded
   memory, неизменность source и отсутствие посторонних данных в project output.
3. **Под контролем пользователя:** короткий тест установленного клиента с одним
   аккаунтом и выдуманными сообщениями, verified enable/disable/rollback; сверить
   IDs с контрольным JSON export/fixtures, текст/правки/удаления с тестовым сценарием.
   Не менять текущий клиент заранее.
4. Если account mapping, полнота или глобальная запись неприемлемы — зафиксировать
   no-go. Patch Desktop остаётся отдельным решением со своей ценой; UI export — fallback.

Перед реализацией/выпуском — [connector review](../connectors/review-policy.md)
и [ADR-0002](../adr/0002-user-controlled-content.md). Debug mode не создаёт нового
разрешения на использование содержимого. Сборщик, qualification и разрешённые
операции пока отсутствуют; registry/runtime не изменялись. Проверены публичные
исходники, а не сообщения, сессия или логирование реального пользователя.

[launcher]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/core/launcher.cpp
[log-h]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/logs.h
[transport]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/mtproto/session_private.cpp
[dump]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/mtproto/details/mtproto_dump_to_text.cpp
[dump-h]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/mtproto/details/mtproto_dump_to_text.h
[codegen]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/codegen/scheme/codegen_scheme.py
[generator]: https://github.com/desktop-app/lib_tl/blob/29746d31247c915a9ec870241399a9d103c39f87/tl/generate_tl.py
[schema]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/mtproto/scheme/api.tl
[logs]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/logs.cpp
[updates]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/api/api_updates.cpp
[account]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/main/main_session.cpp
[build]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/docs/building-linux.md
[credentials]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/docs/api_credentials.md
