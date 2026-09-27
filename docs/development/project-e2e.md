# Project: сквозная проверка повторных архивов

Срез `tgsum-hzm.11`, 2026-09-27. Suite:
`src-tauri/tests/commands/project_e2e.rs`, подключён к integration target `commands`.

```sh
cargo test -p tgsum --features analysis-fixtures --locked --test commands project_e2e -- --nocapture
bash scripts/check.sh
```

Один сценарий выполняется для Codex и Claude через JSON IPC Tauri MockRuntime.
Только внешний агент заменён debug-only synthetic backend. Project commands,
streaming import, canonical snapshots, scope/diff, privacy, bundle, compiled recipe
validation, журнал и атомарный commit используются настоящие. Native GUI и сеть
этот suite не проверяет; [UI smoke](onboarding.md) покрывает отдельный слой.

## Независимые ожидаемые результаты

| Шаг | Ожидаемое наблюдение |
| --- | --- |
| Index полного synthetic archive | Различимы chat IDs `9007199254740993` и `9007199254740992` |
| Выбран один чат, период 2 января | 3 сообщения; второй чат и сообщения других дат отсутствуют |
| Локальный export | Секрет заменён; private path и native IDs не выходят; baseline пуст |
| Первый успешный анализ | Результат ссылается на точные ID/revision из публичного Markdown; baseline фиксирует snapshot и run |
| Повтор тех же bytes | Новый import snapshot; baseline прежний; 0 выбранных / 3 неизменённых |
| Правка старого сообщения и расширение периода до 3 января | 3 выбранных: 1 edited + 2 created (новое сообщение и ранее невыбранная дата); 1 unchanged пропущено |
| Сообщение отсутствует в новом архиве | 1 missing, 0 deleted |
| Evidence после правки | ID прежний, revision другая; новый message ID ниже максимального не теряется |
| Отказ агента | Project и baseline побайтно в JSON-представлении прежние, delta остаётся той же |
| Повторный успешный анализ | Baseline переходит на новый snapshot и расширенный период; delta становится пустой |
| Пустая delta | Подготовка bundle отклоняется до создания нового analysis run |
| Оборванный JSON | Ошибка импорта сохраняет последний Project/snapshot/baseline |
| Новый экземпляр приложения | Оба успеха и отказ читаются из журнала; старые evidence revisions не переписываются |

Проверка читает только public IPC responses и файлы, возвращённые командой export.
Она не вычисляет ожидаемые HMAC production-функциями и не обращается к private
evidence index или revisions через файловую систему. Значения счётчиков заданы
из ручного сценария, а не получены из проверяемого diff engine.

Первый прогон обнаружил неверное предположение теста: пустой bundle не создаётся,
команда уже отклоняет пустую выборку. Ожидание исправлено по существующему контракту;
production behavior для прохождения suite не менялся. Оба adapter journeys проходят.
Реальные messenger/agent accounts, provider inference и exporter automation не
используются и этой проверкой не квалифицируются.
