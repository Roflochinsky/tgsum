# Telegram Desktop: Windows UI Automation bridge

Проверено **2026-09-27** для `tgsum-i1w.3`. Это чтение первичных источников и
исходников, **не проверка установленного Windows-бинарника**. Telegram, аккаунты,
профили, `tdata`, сессии и реальное UIA-дерево не открывались; экспорт не запускался.
Исследование дополняет [assisted export](telegram-assisted-export-2026-09-27.md)
и следует [connector review](../connectors/review-policy.md).

## Решение

**No-go для автоматического экспорта неизменённым Windows Desktop:** профиль
`can_export` остаётся `false`, client automation — `experimental/disabled`.
Нельзя доказать до нажатия точный аккаунт и native conversation ID; ссылки
выбора формата и папки не имеют доказанного действия UIA; экран Done не
связывает завершение с запрошенными аккаунтом, чатом, запуском и итоговым путём.
UIA transport, fake tree и диагностику можно разрабатывать отдельно. Ни
успешный `Invoke`, ни source review не повышают статус поддержки.

## Версии и пределы источников

- [Telegram Desktop v7.2.9](https://github.com/telegramdesktop/tdesktop/releases/tag/v7.2.9):
  `git ls-remote ... refs/tags/v7.2.9` вернул
  `fb2e33209517e1a34637d837bfadb3783f2fd59c`. Исходники Telegram ниже
  закреплены этим commit. Его `Telegram/lib_ui` закреплён на
  [`ae492d4015ce35daf697053776c79786ea7d9282`](https://github.com/desktop-app/lib_ui/tree/ae492d4015ce35daf697053776c79786ea7d9282).
- Механизм Windows UIA изучен по
  [Qt `qtbase` v6.11.2](https://github.com/qt/qtbase/tree/v6.11.2/src/plugins/platforms/windows/uiautomation).
  Проверенный ранее [Linux build recipe](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/build/docker/centos_env/Dockerfile#L883)
  указывает эту версию Qt, но **не удостоверяет Qt в выпускаемом Windows
  бинарнике**. [Windows workflow](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/.github/workflows/win.yml)
  имеет варианты Qt/Qt6, но не доказывает состав установленного релиза.
  Выводы о конкретных UIA patterns ниже условны для данной Qt-цепочки.
- Документация [Microsoft UI Automation](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-clientsoverview)
  описывает контракт ОС, а не фактически доступные selectors Telegram. Имена
  кнопок в исходниках локализуются; [Microsoft предупреждает](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-usefortesting),
  что `Name` не уникален, `AutomationId` не обязан быть стабильным между
  сборками, а `RuntimeId` следует считать непрозрачным и временным.

## Проверка по необходимым возможностям

| Возможность | Факт источника и вывод | Состояние |
| --- | --- | --- |
| Привязка к процессу и версия | Qt UIA отдаёт `UIA_ProcessId` как текущий PID, `AutomationId` через `QAccessibleBridgeUtils::accessibleId`, а `RuntimeId` из `QAccessible::Id`. Это идентификаторы UI/процесса, не Telegram account/chat ID. [Qt свойства](https://github.com/qt/qtbase/blob/v6.11.2/src/plugins/platforms/windows/uiautomation/qwindowsuiamainprovider.cpp#L529), [Qt RuntimeId](https://github.com/qt/qtbase/blob/v6.11.2/src/plugins/platforms/windows/uiautomation/qwindowsuiamainprovider.cpp#L777). Привязка PID к пути процесса возможна через [QueryFullProcessImageNameW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamew); версия файла читается через [GetFileVersionInfoW](https://learn.microsoft.com/en-us/windows/win32/api/winver/nf-winver-getfileversioninfow), а доверие к PE-файлу можно проверить [WinVerifyTrust](https://learn.microsoft.com/en-us/windows/win32/api/wintrust/nf-wintrust-winverifytrust). В advanced settings есть `currentVersionText()`, но его доступность зависит от UI. [Telegram settings](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/settings/sections/settings_advanced.cpp#L1019). | **Реализуем как диагностический probe**, с allowlist точной версии/сборки и ошибкой при unknown/mismatch. Путь, строка версии или подпись по отдельности не доказывают, что запущен исследованный релиз; Windows артефакт не измерен. |
| Точный аккаунт и чат до действия | `SettingsWidget` хранит внутренние `_session` и `_singlePeerId`, полученный из `MTPInputPeer`; в export UI эти значения как native ID не выведены. [SettingsWidget](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L124). Пункт меню создаётся из внутреннего `_peer`, если `canExportChatHistory()`, но видимый текст — локализованный `Export chat history`. [Peer menu](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/window/window_peer_menu.cpp#L1060). Chat-list accessibility отдаёт имя/статусы; `accessibilityChildIdentity()` — token из адреса session-owned объекта с type bits, не native peer ID. [Row name](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/dialogs/dialogs_inner_widget_accessibility.cpp#L161), [token](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/dialogs/dialogs_inner_widget.cpp#L7068). | **Не доказано.** Display name, process, UIA ID и вручную заданный `account_local_id` не являются независимой native identity. Другие UI-пути не исключены, но для исследованного пути точного selector нет. При `Unknown`/mismatch остановить попытку до Export. |
| Доступность UIA `Invoke` для кнопок | У Qt `QAccessibleWidget` есть `ActionInterface` даже при пустом `actionNames()`. Windows provider выдаёт `InvokePattern`, если `actionInterface()` существует, а `Invoke()` без проверки имени вызывает `doAction(press)` и возвращает `S_OK`. [Qt interface](https://github.com/qt/qtbase/blob/v6.11.2/src/widgets/accessible/qaccessiblewidget.cpp#L444), [Qt pattern](https://github.com/qt/qtbase/blob/v6.11.2/src/plugins/platforms/windows/uiautomation/qwindowsuiamainprovider.cpp#L367), [Qt Invoke](https://github.com/qt/qtbase/blob/v6.11.2/src/plugins/platforms/windows/uiautomation/qwindowsuiainvokeprovider.cpp#L30). `Ui::Accessible::Widget::doAction()` передаёт вызов `RpWidget`; `AbstractButton::accessibilityDoAction(press)` вызывает `clicked()`, если кнопка активна. [lib_ui wrapper](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/accessible/ui_accessible_widget.cpp#L271), [button](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/abstract_button.cpp#L230). Menu `Action` наследует `ItemBase → RippleButton`; Export/Cancel/Done создаются как `RoundButton`. [Menu](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/widgets/menu/menu_item_base.h#L17), [settings buttons](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L902). | **Правдоподобный кандидат для кнопок в этой Qt-цепочке**, не проверенное действие Windows-релиза. `S_OK` означает вызов provider, не бизнес-результат. [Microsoft Invoke](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-implementinginvoke). |
| Формат и папка | Per-chat settings строит **один** `FlatLabel` с внутренними click handlers `internal:edit_format` и `internal:edit_export_path`; это не публичные deep links. [Telegram settings](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L355). `FlatLabel` предоставляет StaticText/plain name и не переопределяет действие; у `RpWidget` действие пустое. Значит Qt UIA может отдать `InvokePattern` и даже `S_OK` **без открытия** выбора формата/папки. [FlatLabel](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/widgets/labels.h#L103), [пустой handler](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/rp_widget.cpp#L531). Уже открытый `ChooseFormatBox` содержит три radio-варианта и Save; Qt UIA `SelectionItem::Select` вызывает `press` для RadioButton. [Telegram dialog](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L69), [Qt Select](https://github.com/qt/qtbase/blob/v6.11.2/src/plugins/platforms/windows/uiautomation/qwindowsuiaselectionitemprovider.cpp#L32). Выбор папки вызывает `FileDialog::GetFolder`; фактическое дерево диалога не измерено. [chooseFolder](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L948). | **Блокер для unattended настройки**: ни Link children, ни работающий UIA action для этих двух ссылок не доказаны. `Invoke` у `FlatLabel` нельзя считать достаточным. Не подменять его координатой, глобальными клавишами или отправкой `internal:` URL. |
| Связь результата с попыткой | Export controller закрывает writer, ждёт `finishExport()` и публикует `FinishedState(path, count, bytes)`; UI показывает completed/count/size и кнопку Done. [controller](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/export_controller.cpp#L404), [finished UI](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_content.cpp#L170), [Done](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_progress.cpp#L356). `FinishedState.path` используется в `ShowInFolder`, но не выведен в completed labels. [Done callback](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_panel_controller.cpp#L330). Если панель уже открыта, `Manager::start()` лишь активирует её; это может быть прежний export другого peer. [Manager](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/export_manager.cpp#L45). Для непустой папки создаётся `ChatExport_YYYY-MM-DD` с возможным суффиксом; файл JSON появляется до завершения writer. [NormalizePath](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/output/export_output_abstract.cpp#L22), [JsonWriter](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/output/export_output_json.cpp#L2431). | **Блокер для корреляции.** Старый Done, файл `result.json`, стабильный размер/mtime и даже валидный JSON не доказывают *новый* export нужного source. Готовый JSON можно проверять как отдельный импорт и сверять native conversation ID уже после пользовательского экспорта. |

## Сессия, права, ошибки и отмена

- Перед каждым возможным действием нужно проверять, что клиент и TGSUM находятся
  в нужной Windows-сессии, она активна и unlocked. `WTSSessionInfoEx` даёт
  `SessionState` и `SessionFlags`; `WTS_SESSIONSTATE_UNKNOWN` — остановка.
  `WTSRegisterSessionNotification` сообщает lock/unlock/disconnect, но событие
  требует повторной проверки состояния. В Windows 7/Server 2008 R2 значения
  lock/unlock в `SessionFlags` перепутаны; эти версии не следует молча
  квалифицировать. [WTSINFOEX_LEVEL1_W](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ns-wtsapi32-wtsinfoex_level1_w),
  [WTS notifications](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsregistersessionnotification),
  [message types](https://learn.microsoft.com/en-us/windows/win32/termserv/wm-wtssession-change).
  Локальный passcode Telegram — **другое** состояние; source показывает отдельный
  [PasscodeLockWidget](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/window/window_lock_widgets.cpp#L116).
  Не вводить passcode и не запускать разблокировку.
- Windows UIPI ограничивает взаимодействие с приложением более высокого
  integrity level. [Microsoft UIA security](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview)
  и [описание elevated automation](https://learn.microsoft.com/en-us/power-automate/desktop-flows/how-to/enable-ui-access)
  отделяют обычный same-user/same-integrity случай от UIAccess. Для TGSUM
  elevated Telegram, UAC/secure desktop и отказ доступа — unsupported/unknown;
  не повышать привилегии и не включать UIAccess ради экспорта.
- UIA-вызовы выполнять на отдельном COM MTA потоке с дедлайнами. Microsoft
  предупреждает о зависании UI thread и требует аккуратно подписывать/снимать
  события. [Threading](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading).
  `IUIAutomation2` задаёт [ConnectionTimeout](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-put_connectiontimeout)
  и [TransactionTimeout](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-put_transactiontimeout).
  Ограничить также число узлов, глубину, число polling/event cycles и время
  всей попытки; численные лимиты — инженерный контракт TGSUM, не гарантия ОС.
  Кэш UIA устаревает после изменения интерфейса; подписка на events не
  гарантирует всех событий. После каждого перехода нужен новый bounded snapshot,
  проверка PID/session/version/identity и ожидаемого состояния.
  [Caching](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients),
  [events](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsforclients).
- `Invoke` кнопки Cancel в прогрессе вызывает
  [stopWithConfirmation()](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_panel_controller.cpp#L325):
  при `ProcessingState` Telegram показывает отдельное подтверждение, после
  которого вызывает `cancelExportFast()`. Состояние отмены требуется
  наблюдать, а не выводить из `S_OK`. Автоматический retry после timeout
  опасен: первая попытка могла начаться, `Manager::start()` может активировать
  прежнюю панель, а результат может появиться позже. Поэтому **никаких слепых
  повторных Export/Stop**; при неоднозначном состоянии завершить попытку как
  `needs_user_action`, сохранить ручной import fallback. [Controller cancel](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/export_controller.cpp#L399),
  [Microsoft Invoke semantics](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-implementinginvoke).

## Что проверять в контролируемом PoC

PoC для `tgsum-t8t.9` должен проходить **под контролем пользователя** на
конкретном Windows build и установленном неизменённом Telegram Desktop.
Записать ОС, источник установки, hash/подпись и версию фактически запущенного
`.exe`, Qt/runtime provenance, язык UI и тип установки; без `tdata`, секретов,
снимков личных чатов и содержимого сообщений в документации или Beads.

1. Захватить обезличенное UIA-дерево с roles, names, AutomationId, RuntimeId,
   PID, supported patterns и состояниями: выбранный чат, меню, settings,
   `ChooseFormatBox`, папочный диалог, progress, Done, confirmation Cancel.
   Проверить отдельно `Invoke` кнопок и ложноположительный `Invoke` у `FlatLabel`.
2. Найти **наблюдаемый native account ID и conversation ID до мутации** либо
   подтвердить их отсутствие в квалифицированном UI-пути. Несколько аккаунтов,
   одноимённые чаты, topic, несколько окон/процессов и уже открытая панель —
   обязательные negative cases. Совпадение display name не закрывает критерий.
3. Доказать programmatic выбор JSON и фактической папки с readback после
   действия, затем один новый export: начальное отсутствие прежнего Done,
   наблюдение переходов, terminal state и фактического пути нового файла.
   Проверить изменение имени `ChatExport_*`, конфликт имени, старый Done и
   частично записанный `result.json`; готовый файл валидировать до EOF и
   сверить conversation ID.
4. Проверить lock/unlock Windows, локальный passcode Telegram, disconnect/RDP,
   elevated target/отказ доступа, смену версии или PID, timeout, зависший UIA
   provider, Cancel с подтверждением и отсутствие второго запуска при
   неоднозначном исходе. Снимки UIA и synthetic fake tree сохранить без данных
   аккаунта.

Только измеренный, воспроизводимый путь с точной identity, управляемыми
форматом/папкой и коррелированным terminal state позволяет пересмотреть
`can_export`. Если любой обязательный пункт не выполнен, остаётся assisted
export с ручным подтверждением готового JSON; отсрочка PoC не является
доказательством поддержки.
