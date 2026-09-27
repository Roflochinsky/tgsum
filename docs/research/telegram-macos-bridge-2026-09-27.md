# Telegram Desktop: macOS Accessibility bridge

Проверено **2026-09-27** для `tgsum-i1w.4`. Это чтение официальной документации
Apple/Telegram и закреплённого исходного кода; установленное приложение,
Accessibility tree, аккаунт, профиль и экспорт пользователя не исследовались.
Результат дополняет [assisted export](telegram-assisted-export-2026-09-27.md) и
следует [циклу review](../connectors/review-policy.md). Выводы ниже относятся к
универсальному Telegram Desktop с [desktop.telegram.org](https://desktop.telegram.org/),
пока конкретный установленный bundle и его версия не подтверждены.

## Решение

**No-go для автоматического per-chat export в неизменённом Desktop v7.2.9:**
не доказаны точные account/chat identity, доступные AX действия для открытия
экспорта и настройки формата/папки, а также связь terminal UI с конкретным
запуском и выходным файлом. Для рассмотренной цепочки Qt/lib_ui есть более
сильное отрицательное свидетельство: `AXPress` не объявляется у export
`RoundButton` и menu action. Поэтому `can_export = false`; нельзя обещать
драйвер, подменяя AX действия координатами, глобальным Enter или названием
чата. Assisted export и ручной импорт остаются рабочим контрактом, а не
квалификацией Bridge.

**Go для ограниченного offline кода:** проверка выбранного `.app`, версии,
статуса Accessibility и диагностический evaluator снимка AX дерева/действий с
fake OS; все недоказанные переходы должны возвращать `unsupported`/`unknown`.
Проверка версии или permission сама по себе не повышает `can_export`.

## Версии и граница применимости

- Релиз Telegram Desktop [v7.2.9](https://github.com/telegramdesktop/tdesktop/releases/tag/v7.2.9)
  закреплён на `fb2e33209517e1a34637d837bfadb3783f2fd59c` (git ref
  проверен в соседнем [исследовании](telegram-assisted-export-2026-09-27.md)).
  Его `Telegram/lib_ui` закреплён на
  [`ae492d4015ce35daf697053776c79786ea7d9282`](https://github.com/desktop-app/lib_ui/tree/ae492d4015ce35daf697053776c79786ea7d9282).
  Рецепт macOS выбирает Qt **6.11.2** в
  [`qt_version.py`](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/build/qt_version.py#L4).
  Приведённая ниже цепочка Cocoa Accessibility — исходник Qt с тегом
  [`v6.11.2`](https://github.com/qt/qtbase/tree/v6.11.2/src/plugins/platforms/cocoa).
  Это не воспроизводимый audit выпущенного macOS binary: downstream patches,
  подпись, установленная версия и OS environment отдельно не сверены.
- Telegram различает native **Telegram for macOS** и **Telegram Lite** — вариант
  универсального Desktop в Mac App Store; FAQ прямо связывает экспорт с Lite.
  [FAQ](https://telegram.org/faq#q-why-do-you-have-two-apps-in-the-mac-app-store).
  В исходнике universal Desktop website build имеет bundle identifier
  `com.tdesktop.Telegram` и output `Telegram`, а `build_macstore` —
  `org.telegram.desktop` и `Telegram Lite`.
  [CMake](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/CMakeLists.txt#L2372).
  Это значения рецепта, не доказательство происхождения выбранного `.app`.
  Native macOS Telegram — другое приложение; `Telegram Lite`/Store сборку
  нельзя автоматически считать эквивалентом website build.

## Что видно и что можно вызвать через AX

Apple разрешает клиенту проверить собственный trust через
[`AXIsProcessTrustedWithOptions`](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions),
получить AX root по PID, читать атрибуты/действия и просить выполнить
объявленное действие через
[`AXUIElement`](https://developer.apple.com/documentation/applicationservices/axuielement).
`kAXTrustedCheckOptionPrompt` показывает запрос асинхронно, не превращая
текущий отрицательный ответ в разрешение. Список доступных действий надо
проверять через
[`AXUIElementCopyActionNames`](https://developer.apple.com/documentation/applicationservices/1462053-axuielementcopyactionnames)
перед `AXUIElementPerformAction`; ошибка messaging или invalid element —
не подтверждение успеха. Роли/тексты и исходные UI callbacks сами по себе
не означают доступного действия.

| Элемент | Исходник v7.2.9 / Qt 6.11.2 | Вывод |
| --- | --- | --- |
| Пункт `Export chat history` | `addExportChat()` создаёт `Ui::Menu::Action` для внутреннего peer, только если `canExportChatHistory()`; `Action` задаёт `MenuItem` и локализованный QAction text. [Peer menu](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/window/window_peer_menu.cpp#L1060), [Action](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/widgets/menu/menu_action.h#L26). | Текст и роль — кандидаты probe, но не native peer ID и не доказанный `AXPress`/`AXPick`. |
| `Export`, `Stop`, `Done` | Это `Ui::RoundButton` с текстом и ролью Button. [Settings](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L913), [progress](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_progress.cpp#L278), [Done](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_progress.cpp#L356). | Наблюдение текста возможно исследовать, нажатие через AX в этой цепочке не обеспечено. |
| Format/path website per-chat | Две внутренние ссылки `internal:edit_format` и `internal:edit_export_path` встроены в **одну** `Ui::FlatLabel`, открывающую `chooseFormat()` и `chooseFolder()` только по click handler. [Settings](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L355), [FlatLabel](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/widgets/labels.h#L103). | StaticText/plain text не даёт отдельных AX links/actions; внутренние URI не являются публичным deep link. Формат и путь не контролируются доказанным AX способом. |
| Format/path Store build | При `OS_MAC_STORE` обе строки location/format исключены, включая per-chat. Full-export radio options JSON/HTML остаются в другой ветви `setupPathAndFormat()`. [Settings](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L275). | Даже UI controls отличаются; не переносить website selectors на Telegram Lite. Фактический Store export/путь требует отдельного PoC. |

Причина с `AXPress` проверена по всей цепочке: `RpWidget` возвращает пустой
`accessibilityActionNames()`; `Ui::Accessible::Widget` добавляет только список
базового `QAccessibleWidget`; тот перечисляет focus и иногда showMenu, но не
press по роли. Qt Cocoa формирует `accessibilityActionNames` через
`effectiveActionNames()` и переводит только уже объявленный Qt `pressAction`
в `NSAccessibilityPressAction`; выполнение снова проверяет наличие action.
[RpWidget](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/rp_widget.cpp#L531),
[wrapper](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/accessible/ui_accessible_widget.cpp#L266),
[QAccessibleWidget](https://github.com/qt/qtbase/blob/v6.11.2/src/widgets/accessible/qaccessiblewidget.cpp#L363),
[Qt effective actions](https://github.com/qt/qtbase/blob/v6.11.2/src/gui/accessible/qaccessiblebridgeutils.cpp#L22),
[Cocoa action list](https://github.com/qt/qtbase/blob/v6.11.2/src/plugins/platforms/cocoa/qcocoaaccessibilityelement.mm#L1011),
[Cocoa translation](https://github.com/qt/qtbase/blob/v6.11.2/src/plugins/platforms/cocoa/qcocoaaccessibility.mm#L293).
`AbstractButton::accessibilityDoAction(press)` существует, но до него
объявленный AX action этой цепочкой не доходит.
[Handler](https://github.com/desktop-app/lib_ui/blob/ae492d4015ce35daf697053776c79786ea7d9282/ui/abstract_button.cpp#L230).
Неизвестная downstream сборка может отличаться; реальное `AXUIElementCopyActionNames`
надо измерить, а не предполагать по одной роли.

## Identity, completion, cancel

- Внутри export `SettingsWidget` хранятся session и `_singlePeerId`, но
  per-chat панель показывает общий заголовок и settings, не native ID.
  [Settings constructor](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_settings.cpp#L124),
  [panel title](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_panel_controller.cpp#L172).
  Chat row AX name построен из типа/названия/статусов; его identity token
  строится из адреса session-owned объекта с type bits, а Qt Cocoa
  `accessibilityIdentifier` — из Qt `accessibleId` (object name/class
  ancestry). Это не Telegram account ID или native chat ID.
  [Row name](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/dialogs/dialogs_inner_widget_accessibility.cpp#L161),
  [row token](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/dialogs/dialogs_inner_widget.cpp#L7068),
  [Cocoa identifier](https://github.com/qt/qtbase/blob/v6.11.2/src/plugins/platforms/cocoa/qcocoaaccessibilityelement.mm#L596),
  [Qt ID](https://github.com/qt/qtbase/blob/v6.11.2/src/gui/accessible/qaccessiblebridgeutils.cpp#L75).
  Другие UI paths не исследованы; exact pre-export identity остаётся **unknown**.
- Controller завершает writer, затем `finishExport`, затем публикует
  `FinishedState{path,count,bytes}`. Progress UI показывает «finished», count,
  size и Done, но не path, account/chat ID или run ID; path используется
  лишь в callback Done для Finder. [Controller](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/export_controller.cpp#L404),
  [state](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/export_controller.cpp#L829),
  [content](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_content.cpp#L170),
  [Done callback](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_panel_controller.cpp#L330).
  Уже открытая панель может просто активироваться вместо нового run;
  destination получает `ChatExport_YYYY-MM-DD`/collision suffix. Поэтому
  старый Done, `result.json` или стабильный размер файла не коррелируют новый
  export. [Manager](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/export_manager.cpp#L45),
  [path](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/output/export_output_abstract.cpp#L22),
  [JSON writer](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/output/export_output_json.cpp#L2431).
- Stop в progress вызывает confirmation, а подтверждение вызывает cancel;
  исходный UI callback не доказывает доступного AX action. При отмене со
  стороны TGSUM безопасно прекратить собственный probe и оставить external
  outcome **unknown**; нельзя считать Telegram export отменённым.
  [Stop flow](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/SourceFiles/export/view/export_view_panel_controller.cpp#L325).

## Bundle, permission, session

- `NSRunningApplication` даёт PID, bundle ID/URL и executable URL запущенного
  процесса; свойства могут измениться между чтением и действием, а объект
  остаётся после exit. Сверка выбранного `.app` с конкретным running instance
  — реализуемый preflight, не аутентификация Telegram аккаунта.
  [Apple](https://developer.apple.com/documentation/appkit/nsrunningapplication).
  Bundle Info.plist в рецепте содержит `CFBundleIdentifier` и
  `CFBundleShortVersionString`; сравнивать их с поддерживаемым профилем можно,
  но строка версии/ID сама по себе подделываема.
  [Telegram.plist](https://github.com/telegramdesktop/tdesktop/blob/fb2e33209517e1a34637d837bfadb3783f2fd59c/Telegram/Telegram.plist#L7).
  Для утверждения о происхождении нужна отдельная проверка code signature с
  заранее установленным требованием издателя, например
  [`SecStaticCodeCheckValidity`](https://developer.apple.com/documentation/security/secstaticcodecheckvalidity%28_%3A_%3A_%3A%29);
  требование подписи website release здесь не выведено из исходников и не
  сверено с artifact. Проверка static code также чувствительна к последующей
  модификации файла, согласно Apple.
- `NSWorkspace.sessionDidResignActiveNotification` сообщает переключение
  **пользовательской сессии**, а `screensDidSleepNotification` — сон экрана.
  Это основания остановить GUI действие, но не доказательство состояния lock
  и не гарантия, что после возврата UI остался тем же.
  [Session](https://developer.apple.com/documentation/appkit/nsworkspace/sessiondidresignactivenotification),
  [screen](https://developer.apple.com/documentation/appkit/nsworkspace/screensdidsleepnotification).
  Надёжный lock/unlock сигнал для выбранной конфигурации и поведение AX при
  lock **не установлены**; fail closed при inactive/sleep/AX error и требовать
  нового preflight после возврата. `AXIsProcessTrusted` проверяет permission
  клиента TGSUM, не состояние GUI session или Telegram.

## Следующий контролируемый PoC

Проводить только под управлением пользователя и отдельно для website Desktop
v7.2.9 и, если нужен scope Store, Telegram Lite/Mac App Store. Зафиксировать
macOS/Qt/Telegram версии, bundle URL/ID и проверенную подпись без чтения
`tdata`, credentials, сообщений или session DB. На синтетическом/выделенном
тестовом чате с известным native ID и контролируемым аккаунтом снять **только**
минимальные AX role/name/identifier/action names/selected states и изменения
панели на шагах: выбор чата → меню → настройки → format/path → Export →
progress → Done/Stop; проверить подтверждение отмены, смену аккаунта,
старую открытую панель, collision path, отказ permission, смену версии,
switch user, screen sleep и lock/unlock. Данные аккаунта/путь архива не
публиковать в репозитории или Beads. Для go нужны одновременно доказанные
exact account/chat до старта, все AX действия без координат, фактический
выбранный JSON/path, наблюдаемый новый terminal state с run correlation и
проверка готового JSON до EOF. Отсутствие любого пункта сохраняет assisted
режим и `can_export = false`.
