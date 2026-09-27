# Projects, пошаговый flow и помощь

Срез `tgsum-hzm.10`, 2026-09-27. UI использует существующие Project/bundle/analysis
команды; протоколы агентов и способы получения архивов этим срезом не изменены.

## Flow

- Обычный запуск открывает Projects: создание без credentials и недавние проекты.
  Явный файл в аргументах сохраняет прежний путь разового экспорта.
- При первом запуске доступны три страницы знакомства: локальная подготовка и
  облачный inference; ручной JSON-экспорт Telegram; обнаруженные пути к агентам
  и выбор получателя по умолчанию. Наличие пути не считается квалификацией CLI.
- **Помощь** повторяет знакомство поверх текущего экрана. Она не пересоздаёт
  формы, не удаляет Project и не запускает аккаунт. Диалог можно закрыть без
  завершения; тогда первый запуск покажет его снова.
- Project: **Источники → Проверка → Получатель → Запуск → Результаты**.
  Навигация показывает один раздел за раз. Без подготовленного контекста нельзя
  перейти к получателю; нерешённые находки privacy сохраняют эту границу.
- **Export only** выбран по умолчанию: локальная папка, без auth/model/Run.
  Сохранение сразу открывает результат с точным путём и кнопкой папки.
  Для Codex/Claude есть отдельный Review с получателем и явный Run.
  [Контракт запуска](desktop-analysis.md) сохраняется.

Ошибка валидации или подготовки не перерисовывает форму: даты, модель и другие
введённые значения остаются для исправления. При revision conflict подготовленный
Run отменяется; форму можно сверить и явно загрузить изменения. Эта кнопка
предупреждает о сбросе формы. Backend CAS остаётся обязательным.

WebView хранит только `seen`, default agent ID и до 20 недавних Project IDs
в `tgsum.ui.v1`. Auth/source paths, tokens, сообщения и модель туда не записываются.
Повреждённая или недоступная UI storage не блокирует работу. Project store остаётся
источником содержимого/истории; UI preferences не являются execution capability.

## Воспроизводимая проверка

Linux/WebKitGTK, debug build, отдельный пустой XDG-профиль. Запустить приложение
в одном терминале; не использовать реальный Project store:

```sh
TGSUM_UI_TEST_DIR=$(mktemp -d /tmp/tgsum-ui.XXXXXX)
env XDG_DATA_HOME="$TGSUM_UI_TEST_DIR/data" \
    XDG_CONFIG_HOME="$TGSUM_UI_TEST_DIR/config" \
    XDG_CACHE_HOME="$TGSUM_UI_TEST_DIR/cache" \
    WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:9263 \
    TGSUM_ANALYSIS_FIXTURE=1 \
    cargo run -p tgsum --features analysis-fixtures
```

В другом терминале из корня репозитория:

```sh
PYTHONDONTWRITEBYTECODE=1 uv run --with websocket-client \
  python scripts/ui-onboarding-smoke.py 9263
```

Script отказывается работать без synthetic backend или с непустым Project store.
Это настоящий Tauri host: первая форма создаёт Project, public IPC добавляет
репозиторный synthetic source; UI проходит остальные шаги. Проверяются потеря
draft при ошибке дат, повторная помощь, конфликт второго редактора, явный reload,
scope/coverage, Export only choice и оба synthetic adapter Review/Run/result.
Native dialogs не подменяются; в этом suite их обходят при подготовке fixture.
Для повторного полного запуска нужен новый пустой профиль.

Дополнительно на настоящем окне проверены native выбор JSON и папки для Export
only, локальные manifest/Markdown и результат. После перезапуска default-сборки
проверены сохранённое завершение onboarding, Projects home, пустой auth, Export
only, чтение двух результатов без повторного Run и повторная помощь. Реальные
аккаунты не использовались. Проверки Windows/macOS и установщиков остаются
отдельной квалификацией; synthetic smoke её не заменяет.
