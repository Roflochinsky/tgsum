# Telegram Desktop: Linux accessibility bridge

Проверено **2026-09-27** для `tgsum-i1w.2`. Исследованы публичные исходники
и спецификации. Telegram, аккаунты, профиль, accessibility tree пользователя
и credentials не открывались. Это source review, **не квалификация Linux
экспорта**. Дополняет [assisted export](telegram-assisted-export-2026-09-27.md)
и следует [циклу review](../connectors/review-policy.md).

## Решение

**Автоматический export driver пока experimental/disabled; `can_export = false`.**
В исследованной версии есть доступные имена и роли многих controls, однако
не доказаны exact account/chat identity, управление ссылками формата/папки и
полная корреляция завершения. Более того, цепочка объявления действий Qt/lib_ui
не публикует `press` для экспортных RoundButton, хотя внутренний обработчик
такого действия существует. Нельзя заменить эти пробелы координатами,
совпадением названия чата или синтетическим «Telegram selector profile».

Полезный ближайший кодовый результат — **offline evaluator сохранённого probe**:
он объясняет отсутствующие доказательства и проверяет контракт будущего
транспорта на fake tree. Это часть работы над драйвером; сам export driver
и исходный acceptance `tgsum-i1w.2` этим результатом не считаются выполненными.

## Зафиксированные версии

- Официальная [latest release](https://github.com/telegramdesktop/tdesktop/releases/latest)
  ведёт на [v7.2.9](https://github.com/telegramdesktop/tdesktop/releases/tag/v7.2.9).
  `git ls-remote` подтвердил commit
  `fb2e33209517e1a34637d837bfadb3783f2fd59c`.
- Git tree этого commit закрепляет `Telegram/lib_ui` на
  [`ae492d4015ce35daf697053776c79786ea7d9282`](https://github.com/desktop-app/lib_ui/tree/ae492d4015ce35daf697053776c79786ea7d9282).
  Проверены именно эти исходники submodule, не его текущая ветка.
- Linux build recipe задаёт **Qt 6.11.2**, QPA `wayland;xcb` и Qt patches
  [`519aaa084608fa6f9a2bfbd1959d133c44d94227`](https://github.com/desktop-app/patches/tree/519aaa084608fa6f9a2bfbd1959d133c44d94227/qtbase_6.11.2).
  [Dockerfile](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/build/docker/centos_env/Dockerfile#L883).
  Это recipe выбранного source release; установленный бинарник, distro,
  Flatpak/Snap и воспроизводимость release artifact здесь не проверены.

GitHub REST contents endpoint вернул HTTP 403 rate limit. Pin submodule получен
через shallow git fetch + `git ls-tree`; code archives прочитаны с codeload,
Qt и systemd — также с raw.githubusercontent.com. GitHub API failure не
подменён предположением. Основной freedesktop systemd man URL вернул HTTP 403;
для семантики session использован официальный source tag **systemd v256**.

## Export controls: что действительно следует из source

Таблица описывает Qt accessibility implementation. Реальные object paths,
полный AT-SPI hierarchy, locale, platform packaging и доступность конкретного
control в запущенном Desktop ещё не измерены. Названия ниже относятся к
английскому [lang.strings](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/Resources/langs/lang.strings#L7404),
не являются универсальными selectors.

| Control | Подтверждено исходником | Граница автоматизации |
| --- | --- | --- |
| Chat menu → export | `addExportChat()` добавляет локализованное действие только при `canExportChatHistory()`; topic имеет отдельный путь. `Ui::Menu::Action` предоставляет `MenuItem` и текст QAction. [Peer menu](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/window/window_peer_menu.cpp#L1060), [menu role/name](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/widgets/menu/menu_action.h#L26). | Пункт относится к уже выбранному внутреннему peer; его наличие не удостоверяет native ID или аккаунт. Действие меню по AT-SPI отдельно не квалифицировано. |
| Export / Cancel в настройках | Это `Ui::RoundButton`; name возвращает полный текст, роль наследуется как Button. [Создание](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L913), [name](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/widgets/buttons.h#L141), [role](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/abstract_button.h#L79). | Наличие `accessibilityDoAction(press)` не означает, что press объявлен клиентам. См. цепочку ниже. |
| JSON / HTML / оба формата | `ChooseFormatBox` создаёт `Ui::Radioenum`; это RadioButton с текстом и checked state. Save — обычная box button. [Выбор формата](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L69), [checkbox/radio](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/widgets/checkbox.h#L208). | Состояние формата можно исследовать, когда диалог уже открыт. Открытие этого диалога из per-chat settings не обеспечено доступным link action. |
| Format / Path в per-chat settings | Одна `FlatLabel` с двумя внутренними click handlers: `internal:edit_format` и `internal:edit_export_path`. [Создание и обработчики](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L355). `FlatLabel` отдаёт StaticText и plain-text name; собственные link children, Hypertext/Hyperlink или action overrides в этом классе не реализованы. [FlatLabel](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/widgets/labels.h#L103), [accessible wrapper](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/accessible/ui_accessible_widget.cpp#L93). | Видимость текста `JSON` и папки не предоставляет команду сменить их. Internal URL — click-handler payload, не публичный Telegram deep link. Нельзя выдумать два Link nodes. |
| Выбор папки | `chooseFolder()` вызывает `FileDialog::GetFolder`; результат меняет path и `forceSubPath`. [Source](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L948). | Native/portal dialog и его owner, controls, подтверждение пути требуют отдельного probe для конкретного окружения. |
| Завершение | `FinishedState` создаёт строки с completed text/count/size; `showDone()` удаляет Cancel и создаёт RoundButton с `lng_export_done`. [Content](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_content.cpp#L170), [ProgressWidget](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_progress.cpp#L310). | Это реальный кандидат наблюдения terminal UI: StaticText + Button. Он не содержит account/chat ID, run ID или фактический output path. Одного уже открытого старого Done недостаточно. |

### Почему внутренний press пока не даёт AT-SPI action

1. `AbstractButton::accessibilityDoAction()` обрабатывает press; Checkbox
   имеет аналогичный handler. Но
   [`RpWidget::accessibilityActionNames()`](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/rp_widget.cpp#L531)
   возвращает пустой список. RoundButton/Checkbox его не переопределяют.
2. [`Ui::Accessible::Widget::actionNames()`](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/accessible/ui_accessible_widget.cpp#L266)
   складывает этот список с базовым Qt list.
   [`QAccessibleWidget::actionNames()` Qt 6.11.2](https://github.com/qt/qtbase/blob/v6.11.2/src/widgets/accessible/qaccessiblewidget.cpp#L363)
   добавляет focus и, при соответствующем context-menu policy, showMenu.
   Press по одной роли Button он не добавляет.
3. [`effectiveActionNames`](https://github.com/qt/qtbase/blob/v6.11.2/src/gui/accessible/qaccessiblebridgeutils.cpp#L22)
   дополнительно синтезирует только increase/decrease для Value interface.
   [`AtSpiAdaptor::actionInterface`](https://github.com/qt/qtbase/blob/v6.11.2/src/gui/accessible/linux/atspiadaptor.cpp#L2040)
   принимает индекс именно этого списка и выполняет соответствующее действие.
4. Прочитанные Telegram patches для Qt 6.11.2 не меняют эту цепочку;
   accessibility patch меняет генерацию fallback identifier через RTTI.
   [Patch](https://github.com/desktop-app/patches/blob/519aaa084608fa6f9a2bfbd1959d133c44d94227/qtbase_6.11.2/0026-accessible-class-name-from-rtti.patch).

**Вывод из source:** для этой цепочки нельзя заявить работающее нажатие Export
через AT-SPI. Probe должен читать фактические actions; отсутствие требуемого
действия — blocker, а не повод вызвать индекс `0` или отправить глобальный Enter.
Изменения downstream package/другой версии могут менять результат и требуют
новой evidence. Это не утверждение о каждой когда-либо выпущенной сборке.

## Identity, версия, completion

### Exact account/chat identity не получена

`SettingsWidget` действительно держит внутренние session и `_singlePeerId`.
Они поступают из `MTPInputPeer`, но не выводятся в export widgets как native IDs.
Заголовок панели — общее название chat/topic settings.
[Settings constructor](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L124),
[panel title](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_panel_controller.cpp#L172).

Chat list accessibility возвращает тип, имя и статусы разговора.
Его `accessibilityChildIdentity()` построен из адреса session-owned C++ object
с type bits либо hash для hashtag. Это внутренний token устойчивости
accessible item в текущем процессе, **не Telegram conversation ID**.
[Row name](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/dialogs/dialogs_inner_widget_accessibility.cpp#L161),
[identity](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/dialogs/dialogs_inner_widget.cpp#L7068).

AT-SPI AccessibleId/object path тоже не Telegram ID. Qt fallback identifier
составляется из objectName/class ancestry; Telegram patch использует RTTI.
Ни display name, ни PID, ни локальный `account_local_id`, ни объявленный
«selected source» не подтверждают активный Telegram account. Просмотренные
пути не дают доказательства exact native account/chat до экспорта; другие
неисследованные UI paths остаются unknown.
[Qt identifier](https://github.com/qt/qtbase/blob/v6.11.2/src/gui/accessible/qaccessiblebridgeutils.cpp#L75).

Сверка native conversation ID готового JSON полезна **после** экспорта, но не
исправляет ошибочное предварительное чтение другого чата. Per-chat JSON сам
по себе не аутентифицирует account owner. Поэтому account/chat evidence надо
типизировать `Unknown / UserDeclared / ObservedNative`, не одним boolean.

### AT-SPI Version — версия Qt

`org.a11y.atspi.Application.Version` описывает toolkit; новые спецификации
называют это ToolkitVersion. Qt возвращает `qVersion()`. Это **не версия
Telegram** и не свидетельство его официального происхождения.
[AT-SPI Application](https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/doc-org.a11y.atspi.Application.html),
[Qt GetVersion](https://github.com/qt/qtbase/blob/v6.11.2/src/gui/accessible/linux/atspiadaptor.cpp#L1594).

В Telegram advanced settings есть FlatLabel с `currentVersionText()`, но
секция зависит от `HasUpdate()`. Это кандидат чтения версии уже открытого UI;
стабильная навигация и точный parser не квалифицированы. У diagnostic result
должны быть отдельные `application_version` и `toolkit_version`, допускающие
unknown. Не запускать executable с `--version` для получения предположения.
[Version UI](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/settings/sections/settings_advanced.cpp#L1019).

### FinishedState отражён в UI, но не полностью коррелирован

Controller сначала успешно завершает writer, затем ждёт `finishExport`
и устанавливает FinishedState. Это более сильный исходный signal, чем файл
на диске. Однако FinishedState.path не включён в completed labels: он
передаётся файловому менеджеру при нажатии Done.
[Controller](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/export_controller.cpp#L404),
[Done callback](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_panel_controller.cpp#L330).

Существующая export panel может просто активироваться повторно вместо
создания нового export. Destination может получить `ChatExport_YYYY-MM-DD`
и collision suffix. Поэтому snapshot одного Done screen, совпавшее имя
файла, размер, `mtime` и валидный EOF не доказывают новое завершение для
запрошенного source/run.
[Manager::start](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/export_manager.cpp#L45),
[NormalizePath](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/output/export_output_abstract.cpp#L22).

## Linux transport и session

AT-SPI — D-Bus интерфейсы приложения, не протокол координатного клика.
Qt 6.11.2 получает a11y bus через session bus `org.a11y.Bus.GetAddress`,
учитывает status IsEnabled/ScreenReaderEnabled; XCB имеет дополнительный
способ получить address через X atom. Общий D-Bus путь применим и при Wayland;
это не доказательство работоспособности Telegram на любом compositor.
[Architecture](https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/architecture.html),
[Qt connection](https://github.com/qt/qtbase/blob/v6.11.2/src/gui/accessible/linux/dbusconnection.cpp#L35).

Минимальные семантики будущего transport:

- `Accessible` даёт role/name/state/interfaces/children; данные могут меняться.
  ObjectRef — пара bus name + object path, а не постоянный selector.
  [Accessible](https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/doc-org.a11y.atspi.Accessible.html).
- `Action.GetName(index)` даёт machine name, `GetActions` по спецификации
  содержит локализованные описания; `DoAction(index)` возвращает bool.
  Перед mutation нужны свежая проверка node/action и последующее наблюдение
  перехода. `true` не доказывает завершение бизнес-операции.
  [Action](https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/doc-org.a11y.atspi.Action.html).
- PID/UID получать у bus daemon для уникального connection owner через
  `GetConnectionUnixProcessID`/`GetConnectionCredentials`, не Application.Id.
  ProcessFD, если доступен, устраняет повторное использование PID.
  Эти данные связывают процесс с соединением; подпись binary/account они
  не удостоверяют. [D-Bus specification](https://dbus.freedesktop.org/doc/dbus-specification.html#bus-messages-get-connection-credentials).
- `login1.Session.Type` различает x11/wayland; Active означает foreground
  session; LockedHint задаётся desktop environment. Active не значит unlocked,
  IdleHint не значит locked. Отсутствие/устаревание lock signal — unknown.
  Надёжность обновления hint конкретным compositor/locker ещё не проверена.
  [systemd v256](https://github.com/systemd/systemd/blob/v256/man/org.freedesktop.login1.xml#L1385).

**Решение TGSUM:** unknown/locked/inactive session останавливает automation.
Не включать accessibility глобально, не менять compositor config, не
форсировать X11, не разблокировать session и не поднимать скрытый Telegram.
ОС session и внутренний Telegram passcode lock — разные состояния.
UI action разрешается только в будущем квалифицированном профиле; transport
и a11y bus availability сами по себе не создают такой профиль.

### Rust API — исследован, зависимость не добавлена

Проверен [`atspi` 0.30.0](https://docs.rs/atspi/0.30.0/atspi/),
`atspi-proxies`/`atspi-connection` 0.14.0; source `.cargo_vcs_info.json` указывает
commit `7c3ce193accd9b48929c6c845afca9c6cf250710` проекта
[odilia-app/atspi](https://github.com/odilia-app/atspi/tree/7c3ce193accd9b48929c6c845afca9c6cf250710).
Документированы `AccessibleProxy::{get_children,get_role,get_state,get_interfaces,name}`,
`ActionProxy::{get_actions,get_name,do_action}` и
`AccessibilityConnection::{new,connection}`. Version semantics у proxy также
относятся к toolkit.
[Accessible source](https://docs.rs/crate/atspi-proxies/0.14.0/source/src/accessible.rs),
[Action source](https://docs.rs/crate/atspi-proxies/0.14.0/source/src/action.rs).

`AccessibilityConnection::new()` обращается к session/a11y bus. Отдельная
`set_session_accessibility(true)` **меняет** session setting, это не probe.
Для нынешнего offline evaluator ни один такой вызов не нужен. При будущем
transport следует отдельно ограничить deadline каждой D-Bus операции и
обход дерева: proxy API сам не доказывает наши resource bounds.
[Connection source](https://docs.rs/crate/atspi-connection/0.14.0/source/src/lib.rs).

## Предлагаемый узкий контракт реализации

Следующее — проектное предложение, **не опубликованные Telegram selectors**:

```text
ProbeEnvelope
  provenance: synthetic | controlled_capture
  source_revision, captured_at, session_epoch
  application: bus_owner, object_path, process_binding
  application_version: Unknown | Reported(value, evidence)
  toolkit_version: Unknown | Reported(value, evidence)
  session: type, active, lock_state, freshness
  identity: account_evidence, conversation_evidence
  nodes[]: parent/ref, role, name, states, interfaces, actions[]

evaluate(probe, requested_source) -> Diagnostic
  observed_capabilities[]
  missing_evidence[]
  can_export: false
  disposition: experimental_disabled | needs_user_action
```

1. Реализовать bounded parsing/evaluation сохранённых synthetic probes без
   D-Bus, Telegram discovery, launch или credentials. Предлагаемые лимиты:
   1 MiB, 2048 nodes, depth 32; reject cycles/duplicate refs/foreign owner.
   Это наши начальные engineering limits, не пределы AT-SPI.
2. Сохранить Unknown для identity/version/lock/path, различать user-declared
   и independently observed evidence. Наличие строки, похожей на ID,
   не повышает её происхождение. Source-review fixture помечается synthetic
   и никогда не превращает `can_export` в true.
3. Зафиксировать transport trait для ограниченного чтения и явного action
   intent. Fake tree проверяет stale node, смену owner/session/account,
   отсутствующий/двойной control, locale, unsupported version и cancel.
   Профиль callable selectors для real Telegram оставить отсутствующим.
4. Bounded retry — максимум три **read-only** попытки с общим deadline;
   fake clock проверяет пределы. Mutation никогда не повторяется после
   ambiguous timeout: сначала наблюдение, затем needs_user_action. Cancel
   локальной попытки не должен притворяться подтверждённым Telegram cancel.
5. Completion observer может хранить переход running → completed UI одного
   owner/panel/attempt, но до доказательства identity/path он не создаёт
   успешный `ExportJob` и не публикует новый snapshot. Уже имеющийся
   `core/src/bridge.rs` проверяет supplied correlation, а не происхождение
   этих observations.

## Controlled PoC и оставшаяся граница

Первичная контролируемая проверка доступных сигналов выделена в backlog
`tgsum-t8t.23`, prerequisite для `tgsum-i1w.2`. Она может установить no-go
для этой сборки, не требуя реализации фиктивного export driver. В последующем
user-controlled PoC `tgsum-yu3` требуется проверить одну точную
официальную Linux build, locale и session stack: ограниченный export subtree,
actual actions, format/path selection, два последовательных экспорта одного
согласованного чата, interrupted run, stale completion и lock/unlock. Проверку
инициирует и контролирует пользователь; raw desktop tree и переписки не
публикуются в repo/Beads. Нужна также **доказуемая** account/chat привязка до
acquisition; пока допустимый механизм неизвестен. Если он не обнаружится,
автоматический selected-chat export не квалифицируется и остаётся assisted.

Сейчас можно написать evaluator, error model, transport boundary, bounds и
synthetic regression cases. Нельзя честно завершить real export driver,
заявить unattended refresh или cross-compositor support. Отсутствующие
selectors/actions/identity — конкретные implementation gaps, не только
формальность отложенного тестирования.

Account auth и сетевые запросы остаются у Telegram Desktop; TGSUM не читает
`tdata`, session DB, память процесса или cookies. Этот способ не доказывает
нулевой account risk и не отменяет content-use conditions; platform review
остаётся внутренним engineering/release решением согласно
[ADR-0002](../adr/0002-user-controlled-content.md), без per-chat legal gate.
Terms review наследуется из указанного assisted-export исследования этой даты;
нового утверждения о разрешённости AI-use этот accessibility review не делает.
Срок review в registry остаётся **2026-10-26**; повторная проверка также нужна
при смене клиента/Qt/package и перед продвижением support. Это исследование
accessibility не переносит дату platform-policy review.
