# Telegram: assisted export и запуск выбранного клиента

Проверено **2026-09-27** для `tgsum-i1w.1`. Область: пользователь выбирает
разговор, TGSUM запоминает источник и путь, показывает инструкцию и по явному
действию открывает выбранный клиент. Экспорт запускает и подтверждает человек.
Это исследование документации и исходников; Telegram не устанавливался,
реальные клиенты, аккаунты, профили и сессии не открывались.

Дополняет [исследование автоматизации](telegram-export-automation.md), следует
[циклу review](../connectors/review-policy.md) и
[границе ответственности](../adr/0002-user-controlled-content.md).
Следующая проверка способа запуска/клиента: **2026-10-27** либо раньше при
изменении интеграции или перед её выпуском.

## Версия и достоверность

На дату чтения официальный release endpoint указывает **v7.2.9**.
`git ls-remote` для `refs/tags/v7.2.9` вернул
`fb2e33209517e1a34637d837bfadb3783f2fd59c`. Ниже ссылки на этот commit,
а не на изменяемую ветку. Проверка исходника не удостоверяет установленный
бинарник и не является account PoC.
[Release](https://github.com/telegramdesktop/tdesktop/releases/tag/v7.2.9).

GitHub REST API при проверке head/release вернул HTTP 403 rate limit;
версию установили по публичной release-странице и git ref, исходники прочитаны
по полному commit через `raw.githubusercontent.com`. Недоступный REST endpoint
не использован как доказательство. Apple AppKit прочитан также через официальный
`developer.apple.com/tutorials/data/documentation/...json`, поскольку основная
страница требует JavaScript.

## Экспорт: подтверждённые факты

| Факт | Первичный источник | Следствие для assisted UI |
| --- | --- | --- |
| В Telegram Desktop отдельный чат экспортируется через меню `⋮` → `Export chat history`; доступны JSON/HTML и выбранные медиа. | [Объявление Telegram](https://telegram.org/blog/export-and-more), [официальная инструкция](https://bugs.telegram.org/c/60). | Инструкция ведёт к одному разговору и JSON. Полный экспорт аккаунта не требуется. |
| После свежего входа может потребоваться 24 часа ожидания либо подтверждение запроса на другом устройстве. Takeout также описывает задержку в секундах. | [Инструкция](https://bugs.telegram.org/c/60), [account.initTakeoutSession](https://core.telegram.org/method/account.initTakeoutSession). | Показать, что пользователь продолжает в клиенте. Не считать задержку ошибкой TGSUM, не повторять login/export автоматически. |
| Per-chat настройки содержат выбор формата и папки; ссылки открывают `chooseFormat()` и `chooseFolder()`. В сборке `OS_MAC_STORE` соответствующая строка исключена препроцессором. | [SettingsWidget](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L355). | Формат и путь пользователь задаёт в самом Desktop. Нельзя обещать одинаковый диалог во всех сборках. |
| Пустая/ещё не созданная выбранная папка используется напрямую, если `forceSubPath` выключен. Для непустой папки либо `forceSubPath` создаётся `ChatExport_YYYY-MM-DD`, при коллизии — суффикс ` (1)` и далее. | [NormalizePath](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/output/export_output_abstract.cpp#L22). | Сохранённый путь — привязка TGSUM, а не приказ Telegram перезаписывать этот файл. Нужен выбор фактического нового файла и перепривязка. |
| JSON writer открывает основной файл до окончания обработки; его имя — `result.json`. | [JsonWriter::start](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/output/export_output_json.cpp#L2431), [mainFileRelativePath](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/output/export_output_json.cpp#L3051). | Появление файла и неизменность его размера не удостоверяют завершение экспорта. |
| Controller завершает writer, вызывает `finishExport`, затем публикует `FinishedState`; кнопка завершения показывает фактический результат в файловом менеджере. | [exportNext](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/export_controller.cpp#L404), [doneClicks](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_panel_controller.cpp#L330). | Человек ждёт окончания в Telegram, выбирает файл и подтверждает готовность; затем TGSUM отдельно валидирует полный JSON и идентичность разговора. |

Официальная FAQ различает native Telegram for macOS и Telegram Lite — macOS
версию универсального Desktop; экспорт описан для Lite. Сама FAQ предупреждает,
что отдельные сведения могут устаревать. Поэтому название «Telegram» и расширение
`.app` недостаточны для определения возможности экспорта. Для описанного
per-chat диалога reference — универсальный Desktop с
[desktop.telegram.org](https://desktop.telegram.org/); поведение Mac App Store
сборки остаётся отдельной проверкой.
[FAQ о двух macOS приложениях](https://telegram.org/faq#q-why-do-you-have-two-apps-in-the-mac-app-store).

## Запуск: факты и выбранный контракт

Реализация assisted leaf и её более узкая граница описаны в
[assisted-export](../development/assisted-export.md): первый launch принимает
native executable; `.app` через NSWorkspace остаётся дальнейшим OS adapter.
Для пакетов доступен самостоятельный запуск с тем же manual import.

В просмотренном `Launcher::processArguments` есть `-workdir`, `-autostart`,
`-startintray` и URL после `--`; команды автоматического экспорта в этой таблице
нет. Просмотренные локальные URL handlers содержат экспорт тестовой темы,
но не команду экспорта переписки. Это ограниченный результат чтения этих файлов,
не доказательство отсутствия любого внутреннего интерфейса во всех версиях.
[Launcher](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/core/launcher.cpp#L588),
[URL handlers](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/core/local_url_handlers.cpp).

Telegram документирует регистрацию системных обработчиков `tg:` и `t.me`
ссылок. Из этого следует, что открытие такой ссылки само по себе не выбирает
нужный экземпляр Desktop и не удостоверяет его происхождение. Для assisted
launch не использовать URI как замену явного выбора приложения.
[Telegram deep links](https://core.telegram.org/api/links).

| ОС | Механизм, выбранный для реализации | Граница доказательства |
| --- | --- | --- |
| Linux | Явно выбранный абсолютный путь к native executable; `Command::new(path).spawn()` без дополнительных аргументов Telegram. | Rust документирует отсутствие аргументов по умолчанию и поиск через PATH при не абсолютном пути. Не принимать командную строку, `.desktop` или shell wrapper вместо бинарника. Это инженерное ограничение TGSUM, не требование Telegram. [Rust Command](https://doc.rust-lang.org/std/process/struct.Command.html). |
| Windows | Явно выбранный абсолютный путь к `.exe`; прямой process launch, без `cmd`, `start`, `.bat`, `.cmd`, `.lnk` или URI. | Windows `CreateProcessW` запускает executable; Rust отдельно предупреждает об обработке batch через командный интерпретатор. Расширение `.exe` само по себе не проверяет издателя. [CreateProcessW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw), [Rust process](https://doc.rust-lang.org/std/process/). |
| macOS | Явно выбранный `.app` передать как file URL в `NSWorkspace.openApplication(at:configuration:completionHandler:)`; `OpenConfiguration.arguments` оставить пустым. | Apple API запускает приложение по его адресу и сообщает результат запуска; аргументы по умолчанию — пустой массив. `.app` является выбранным приложением, не документом для обработчика `tg:`. [NSWorkspace](https://developer.apple.com/documentation/appkit/nsworkspace/openapplication(at:configuration:completionhandler:)), [arguments](https://developer.apple.com/documentation/appkit/nsworkspace/openconfiguration/arguments). |

**Решение TGSUM:** picker и явная кнопка запуска; запуск выбранного пути без
shell, URL, profile flags, export flags, chat IDs или archive contents в argv.
Не искать установленный клиент через профили, `tdata`, cookies, session DB или
process memory. Не запускать `--version` как проверку: неподдержанный аргумент
может всё равно открыть клиент. Настройки аккаунта читает и обслуживает сам
Telegram; TGSUM не меняет его `HOME`, XDG/profile directory или `-workdir`.

Путь и выбранный пользователем тип приложения не доказывают подлинность кода.
До отдельной проверки подписи/пакета UI сообщает о **выбранном клиенте** и
ссылается на официальный источник установки; фраза «официальный клиент проверен»
не подтверждена этим исследованием. Проверять существование/type выбранной
цели допустимо без обхода профиля. Ссылки и package wrappers требуют отдельного
контракта; при неподдерживаемой установке пользователь открывает Desktop сам.

Успешный spawn/AppKit callback означает только принятую попытку запуска.
Он не удостоверяет вход, активный аккаунт, выбранный чат, экспорт, полноту
истории или готовность медиа. Собственный Telegram может обращаться в сеть
после запуска; нельзя описывать открытие клиента как полностью offline.
Assisted source остаётся `needs_user_action` до явного предоставления и
проверки архива. Ошибка запуска сохраняет ручной путь.

## Контракт источника и refresh

Следующие пункты — инженерные решения TGSUM на основе указанных фактов:

1. Хранить выбранные conversation ID, локальный account namespace, display
   label и конкретный путь JSON в приватных данных Project. Account namespace
   задаёт человек; одиночный архив не аутентифицирует владельца аккаунта.
2. При первом подключении и смене файла явно показывать найденный разговор
   и требовать подтверждение scope; повторный импорт обязан сверять native ID,
   а не только название чата. Другой scope требует явной перепривязки.
3. Отделить назначение пути/launch от `imported`: человек ждёт завершения
   Telegram и предоставляет фактический `result.json`. TGSUM валидирует до EOF
   и публикует snapshot только после успешной проверки.
4. Для повторной выгрузки не обещать Telegram delta/date-range semantics.
   Локальный snapshot/diff сравнивает IDs/версии; отсутствие сообщения не
   превращается автоматически в deletion, coverage остаётся явно ограниченной.
5. Не сканировать родительские каталоги «в поисках последнего экспорта» и не
   копировать полный archive directory автоматически. Для файлов действует
   отдельный выбор/packager. Пользователь может сменить путь или использовать
   обычный импорт даже после ошибки assisted launch.
6. Очищенный context bundle не получает путь приложения, источник archive path,
   данные профиля или запуск messenger. После refresh анализ требует обычного
   review и явного Run.

## Условия платформы

API Terms по-прежнему содержат ограничение AI/ML, включая deployment, а Content
Licensing Terms описывают более широкие ограничения использования данных и
условия исключений. Само наличие официального экспорта не является отдельным
доказательством разрешения любого downstream AI use.
[API Terms](https://core.telegram.org/api/terms),
[Content Licensing Terms](https://telegram.org/tos/content-licensing).

Для этого среза TGSUM не получает Telegram API credentials и не вызывает API
аккаунта; это решение о способе интеграции. По ADR-0002 вопросы использования
содержимого остаются у пользователя/организации. Исследование не вводит анкету
согласий или автоматический legal gate на импорт/Run и не обещает нулевой риск
аккаунту.

## Неизвестное и область дальнейшей проверки

- Реальный запуск и экспорт на Linux/Windows/macOS, установленная версия,
  существующий процесс, screen lock, смена аккаунта и подтверждения — account
  PoC только под контролем пользователя; до него fake-client тест удостоверяет
  только TGSUM-контракт. Отложенная проверка не повышает статус support.
- JSON и выбор папки в Telegram Lite/Mac App Store при `OS_MAC_STORE`, а также
  запуск через Flatpak/Snap/store wrappers — отдельные platform cases.
  Universal website build и manual fallback не доказывают эти случаи.
- Native signature/package provenance, надёжный OS driver, accessibility,
  watcher completion, unattended schedule и isolated runtime относятся к
  следующим согласованным задачам. Assisted launch не заменяет ни один из них.
- Старый Apple URL для man page `open(1)` вернул 404. CLI `open -a` здесь не
  квалифицировался; для выбранного `.app` подтверждён именно AppKit API.

**Вывод:** реализовать remembered source + инструкцию + явный запуск выбранного
клиента + привязку готового JSON с ручным fallback. Не выдавать это за
автоматический Official Client Bridge или проверенный export на реальном
аккаунте. Повышение registry support требует отдельной evidence реализации
и фактической области тестирования.
