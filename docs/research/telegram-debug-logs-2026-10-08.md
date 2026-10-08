# Connector review: Telegram Desktop diagnostic logs

Дата чтения первичных источников: **2026-10-08 UTC**. Владелец review:
TGSUM maintainer. Beads: `tgsum-i1w.12`. Причина: пользователь разрешил
попробовать приём сообщений из штатного Telegram, без бота, отдельного API-входа
и собственной сборки. Это **исследовательский локальный spike**, не поддержка
автоматического acquisition. Правила: [review-policy](../connectors/review-policy.md),
[ADR-0002](../adr/0002-user-controlled-content.md).

## Scope и решение до реализации

Профиль `telegram_desktop_debug_logs`, Arch Linux, штатный Desktop **7.2.5-1**.
Закреплённый upstream **v7.2.5**, commit
`2f41383dddd338fe17fd4711afd02688c418fd47`; `lib_tl` submodule
`29746d31247c915a9ec870241399a9d103c39f87`. Совпадение версии пакета не
квалифицирует поведение установленного binary или реального аккаунта.

Пользователь подтвердил один аккаунт и один выбранный чат. Selected scope — явно
заданный typed peer ID; имена не служат идентификаторами. Authorization/source
scope — уже действующий Desktop, все диагностические события его аккаунта,
включая другие чаты, ещё **до** фильтра сборщика. Не вводить новые credentials,
не читать содержимое session/database `tdata` или process memory. Сборщик не
вызывает account API и не отправляет сообщения.
При необходимости аккаунтных действий их выполняет пользователь.

Разрешён эксперимент с локальными текстовыми событиями и приватным результатом.
Attachments/download, автоматическое получение истории, inference и training не
включаются. Bootstrap остаётся предоставленным JSON export. Сборщик не удаляет,
не переписывает и не меняет retention исходных файлов Telegram.

## Первичные источники

Все ссылки прочитаны 2026-10-08; исходники закреплены указанными commit SHA.

| Источник | Подтверждённый факт |
| --- | --- |
| [launcher.cpp][launcher] | `-debug` включает runtime debug после чтения persistent setting; `-quit` — штатный quit; runtime flag сам не записывает persistent setting |
| [sandbox.cpp][sandbox] | Второй экземпляр передаёт existing instance `CMD:show` или `CMD:quit`; debug flag ему не передаётся; `execExternal("quit")` вызывает `Quit()` |
| [logs.cpp][logs] | DebugLogs внутри фактического working directory; MTP prefix, ротация каждые 15 минут, day header, flush, disabled multiple-instance path |
| [specific_linux.cpp][linux] | Default data path выбирает legacy `~/.TelegramDesktop` при settings, иначе Qt AppLocalDataLocation; `-workdir`/portable могут изменить путь |
| [session_private.cpp][transport] | `Recv` dump после decrypt/integrity/session checks, **до** регистрации duplicate transport ID; `Send` dump до encrypt |
| [connection_abstract.cpp][connection] | `ProtocolDcDebugId` выводит numeric DC с optional `test_` и `_media`, а не signed internal protocol DC |
| [core_types.h][core-types] | `mtpPrime = int32`, `mtpTypeId = uint32`; manual `mtpc_core_message` равен `-1`, передаваемый vector type tag имеет signed prime representation |
| [mtproto_dump_to_text.cpp][dump] | Decimal scalars; полный valid UTF-8 string; escaping только backslash/quote/LF; vector/gzip/layer wrappers |
| [lib_tl generate_tl.py][generator] | Constructor/field syntax; optional absent fields не печатаются; true flags имеют annotation; rpc/container/core wrappers |
| [lib_tl tl_basic_types.h][basic] | `id_flags = id_int`, `id_flags64 = id_long`: flags печатаются теми же scalar annotations |
| [api.tl][schema] | Native message/typed peer/sender IDs, short/new/edit/delete shapes; auth/sign-in fields тоже часть схемы; закреплённая схема имеет `LAYER 229` |
| [Content Licensing][terms] | Нет документированного отдельного исключения для diagnostic-log collection; AI/content ограничения не исчезают из-за локального источника |

**Вывод:** локальный read-only parser не требует собственного Telegram API grant
или API_ID. Desktop продолжает обычную работу своей сессии. Это не доказательство
платформенного разрешения произвольной автоматизации или прав пользователя на
содержимое. Содержимое и AI destination решает пользователь/организация по ADR;
перечисленные правила нужны maintainer для решения о выпуске. Для этого spike AI
handoff отсутствует; local/cloud inference остаётся исследовательским вопросом,
training restricted. Нет обещания отсутствия account risk.

## Activation, path и rollback

Source-confirmed порядок: stable Desktop начинает с выключенного debug, читает
первый byte `cWorkingDir()/tdata/withdebug`, если файл есть; любой byte кроме `0`
включает debug. `-debug` затем принудительно включает его. Сам аргумент не вызывает
`WriteDebugModeSetting()`; UI или другой путь могут отдельно сохранить настройку.

Для того же клиента/profile: штатный `/usr/bin/Telegram -quit`, дождаться полного
завершения, затем `/usr/bin/Telegram -debug`. Запуск `-debug` при живом экземпляре
лишь покажет окно, не включит debug в нём. `-many` не использовать: этот путь не
записывает debug logs. Не менять launch shortcut/autostart или persistent setting.

Rollback **без изменения persistent setting**: штатно завершить экспериментальный
Desktop и запустить `/usr/bin/Telegram` без `-debug`. Это выключает debug только
при stable binary и заранее подтверждённом отсутствии включённого `withdebug`.
Если persistent debug уже включён, этот rollback недостаточен: нужна отдельная
проверка/восстановление исходного состояния. Исходные логи остаются на диске;
автоматическое удаление не входит в rollback.

Точный каталог определять по фактическому working directory, не по предположению
о `$HOME`. Источник на Linux: `<working-dir>/DebugLogs/mtp_HH_MM.txt`; release
Desktop normally использует app data path, а не каталог executable.

## Формат и граница parser

Это внутренний diagnostic text, **не публичный API и не accepted-message feed**.
Полезная граница record по закреплённому dumper:

```text
YYYYMMDD
[hh:mm:ss.zzz TT-EEEEEEE] (dc:2_main) Recv: { core_message
  msg_id: 123 [LONG],
  seq_no: 1 [INT],
  bytes: 100 [INT],
  body: { constructor
    field: value,
  },
} (dc:2,key:123,session:456)
```

Это **синтетический пример framing**, не реальный packet/согласованное bytes
значение. Thread/entry width — минимальные 2/7 decimal digits, могут расти.
Expanded DC бывает main/export/download/upload/temporary и другие роли.
Protocol DC в suffix — `2`, `test_2`, `2_media` или `test_2_media` по
`ProtocolDcDebugId`; отрицательный внутренний DC отображается через `_media`.
Suffix `key` означает key ID, а не authentication secret; key/DC/session **не**
account ID. Они не дают доказанного multi-account namespace.

Object: `{ name` + поля `field: value,` + `}`; каждый последний field тоже имеет
trailing comma. Пустой constructor: `{ name }`. Dots в имени constructor заменены
на underscore: `messages.channelMessages` → `messages_channelMessages`.
Scalar: signed decimal `[INT]`, decimal `[LONG]`, `[DOUBLE]`. Один flags word
использует `[INT]`; при наличии flags2 generator **сливает оба words** в одно
поле `flags: ... [LONG]`, отдельный `flags2` не печатает. True flag:
`YES [ BY BIT N IN FIELD flags ]`; отсутствующий optional field не печатается.
Vector: `[ vector<0xHEX> (N)` + N values с trailing comma + `]`; пустой vector
`[ vector<0xHEX> (0) ]`. Здесь HEX допускает ведущий минус: dumper получает
`mtpPrime` signed int32 и пишет manual container tag как `vector<0x-1>`;
другие bare type tags также могут иметь signed hex representation. Это знак
**type tag**, не count: допустимый count остаётся неотрицательным и bounded.
String: quoted valid UTF-8 + `[STRING]`, backslash→`\\`,
quote→`\"`, LF→`\n`; CR/tab/другие valid controls могут остаться raw. Это **не
JSON-string**, поэтому обычный JSON decoder и line regex не равнозначны dumper.
Binary non-UTF8 string выводится hex + `[N BYTES]`, при N ≥ 64 — только первые
16 bytes и `...`; это не текст `[STRING]`. Такие нерелевантные media/profile поля
не следует интерпретировать как текст сообщения. `[GZIPPED]` и `[LAYERn]` — prefixes,
gzip уже распакован dumper. Закреплённая API schema — layer **229**; эксперимент
принимает явный `[LAYER229]`, а иной explicit layer остаётся неквалифицированным.
Containers/rpc_result/core_message остаются вложенными. `rpc_result.result` может
быть vector/scalar (например, `users.getUsers` возвращает `Vector<User>`): корректный
dump такого ответа не равнозначен malformed message и не доказывает наличие
message events. Для текстового spike это unsupported capture path.

Нужны quote-aware структурный разбор, bounded record/depth/vector/text, exact
integer IDs и явные unknown/error outcomes. `[ERROR]`/bad constructor/truncation
не дают подтверждённого события. Нельзя искать `Recv` внутри произвольного текста,
принимать `Send` как message event или исполнять строки. Неизвестные конструкции
и short outgoing response без peer/text не достраивать по догадке.

## Identity, coverage и local storage

Первый spike привязывается к одному явно подтверждённому account namespace.
Несколько аккаунтов/переключение — unsupported; никаких выводов account ID из
transport suffix. Peer type отделяет user/chat/channel; sender peer не всегда
человек (channel/anonymous author). Short incoming message требует direction
и self identity для корректной sender attribution; неизвестный sender должен
остаться отсутствующим, не получить случайное имя. Edit/delete требуют отдельного
контракта. Non-channel delete не содержит peer; без известного предыдущего mapping
он unresolved. Не передавать чужие IDs в отфильтрованный output.

Raw logs пишутся для всего клиента: в том числе исходящие auth/sign-in поля могут
оказаться там. Не копировать raw logs в Git, Beads, AI bundle или публичный отчёт.
Сборщик сохраняет только selected native peer события и технические счётчики;
ошибки не должны печатать payload. Metadata, file paths и results тоже приватны.

Файл меняется каждые 15 минут по local time. Первая строка содержит dayIndex;
тот же день append дописывает separator `NEW LOGGING INSTANCE STARTED!!!`, иной
день открывает без Append и может truncate одноимённый файл. Write/flush не fsync.
Restart/rotation/truncation и bounds failures должны давать gaps/checkpoint
generation; отсутствие замеченного gap не означает отсутствие потерь. Offline
Desktop не пишет события. Logs не дают completion для всей истории и не заменяют
JSON bootstrap. Дубликаты packet возможны до application acceptance.

## Verification и support decision

Проверка исходного review выполнена до реализации по публичным закреплённым
исходникам. Затем реализован отдельный экспериментальный Rust parser/follower/CLI
с искусственными данными и файловым симулятором. Отдельный проверяющий подтвердил
финальные файлы; 28 тестов parser/capture, 2 теста CLI и quick/full workspace gates
прошли. Регрессии signed vector type tag и escaping LF проверены удалением
исправления: без него тест красный, с ним зелёный.

Контролируемый запуск на установленном Arch Desktop 7.2.5-1 длился 120 секунд.
Пользователь открыл выбранный чат; runtime debug и возврат исходного обычного
запуска проверены, постоянная настройка debug осталась отсутствующей. После
исправления грамматики существующие локальные логи разобраны повторно без нового
аккаунтного запроса: 51 отдельное выбранное сообщение с native message/sender IDs,
46 непустых текстов, 51 точное соответствие строковому представлению источника.
Чужие события не попали в журнал; реальные логи и payload не опубликованы.

Все 51 события — наблюдения истории (`observed`), а не подтверждённый live delta.
Реальные новые сообщения, edits/deletes, полнота, multi-account, application
acceptance и постоянное обновление TGSUM остаются непроверенными. Для текущего
частного эксперимента output принадлежит одному сборщику; path-based publication
не защищена от одновременной замены пути/файла другой программой того же пользователя.
Перед постоянным сбором нужна отдельная проверка этой границы. Итоги:
[публичные счётчики и хеши](../development/evidence/telegram-debug-spike-2026-10-08.json),
[локальная инструкция](../development/telegram-debug-spike.md), Beads `tgsum-i1w.12`.

Registry: `research`, empty enabled operations, null implementation и empty
qualifications. Последний successful initial policy/source review **2026-10-08**;
следующий **2026-11-07** (30 дней client automation). UI не должен объявлять
историю полной или источник поддерживаемым. Улучшение статуса требует независимой
проверки и evidence установленного клиента. [Предварительное предложение](telegram-local-message-bridge.md)
остаётся контекстом; текущий review уточняет activation/framing.

## Последующая проверка постоянной установки

Beads `tgsum-i1w.13` отдельно реализовал durable selected journal, Project apply
и обычную личную release-установку. Independent verifier подтвердил закрытый
checkpoint из 72 событий: 70 observed, 1 NEW и 1 EDIT, точные ID/автор/текст,
сохранение 96 bootstrap-сообщений. После дальнейшего сбора snapshot и готовый
локальный пакет содержат 167 уникальных сообщений. Исторические 51 наблюдение
первого spike не выдаются за эту последующую проверку.

Перед исправлением scan перечитаны закреплённый
[Linux launcher](https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/platform/linux/launcher_linux.cpp)
и [lib_webview spawn](https://github.com/desktop-app/lib_webview/blob/71301e35911f0801ee2890c8b4f04555e34d4f72/webview/platform/linux/webview_linux_webkitgtk.cpp).
Helper использует тот же executable, но `-webviewhelper` направляет его в
WebKitGTK до Core launcher. Кандидаты Desktop теперь исключают эту известную
служебную форму; whitelist управляемого запуска не расширен. Два финальных
plain restart постоянного пользовательского сервиса со штатными helpers
продолжили прежний журнал. Unit enabled для graphical session; свежий login
не выполнялся и записан отдельным user-controlled backlog `tgsum-i1w.16`.

[Публичный агрегированный отчёт](../development/evidence/telegram-continuous-installed-2026-10-08.json)
не содержит личных IDs, текстов, профиля или screenshots. Registry остаётся
`research`; реальные DELETE, много аккаунтов, новые медиа, полнота и AI-handoff
не квалифицированы. Scope и platform policy этой частной проверкой не расширяются.

[launcher]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/core/launcher.cpp
[sandbox]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/core/sandbox.cpp
[logs]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/logs.cpp
[linux]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/platform/linux/specific_linux.cpp
[transport]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/mtproto/session_private.cpp
[connection]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/mtproto/connection_abstract.cpp
[core-types]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/mtproto/core_types.h
[dump]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/mtproto/details/mtproto_dump_to_text.cpp
[generator]: https://github.com/desktop-app/lib_tl/blob/29746d31247c915a9ec870241399a9d103c39f87/tl/generate_tl.py
[basic]: https://github.com/desktop-app/lib_tl/blob/29746d31247c915a9ec870241399a9d103c39f87/tl/tl_basic_types.h
[schema]: https://github.com/telegramdesktop/tdesktop/blob/2f41383dddd338fe17fd4711afd02688c418fd47/Telegram/SourceFiles/mtproto/scheme/api.tl
[terms]: https://telegram.org/tos/content-licensing
