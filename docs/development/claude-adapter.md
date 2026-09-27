# Claude Code: offline protocol и проверка результата

Срез `tgsum-hzm.8`, CLI **2.1.280**, Linux x86_64.
[Первичные источники и ограничения](../research/claude-adapter-2026-09-27.md).
Реализованы `tgsum_runner::claude::{ClaudeRequest, RecipeRequest, decode}` и
отдельный offline профиль с явным выбором одного auth-файла. **HTTPS gateway,
managed lifecycle и Desktop Run пока не подключены.** Этот срез не подтверждает
облачный запуск. [Исследование auth/egress](../research/claude-auth-egress-2026-09-27.md).

## Запрос

`ClaudeRequest::prepare` принимает проверенный `PreparedContext`, точный model ID,
доверенные task/schema и отмену. Все выбранные публичные документы входят в stdin
как JSON `task` + `untrusted_documents`. Источники не управляют system prompt,
argv, tools, profile или получателем. Подготовка и `invocation()` проверяют
revision. Oversize отклоняется целиком: stdin 1 MiB, task 32 KiB, schema 16 KiB.
Schema передаётся одним argv, отдельно от содержимого переписки; stdin заканчивается
EOF. Допускается отсутствующий `$schema` или draft-07, который использует CLI.
Schema — доверенная программа recipe, не произвольная схема из импорта.

Фиксированные аргументы: print/text/stream-json/verbose, safe-mode, пустые built-in
tools, запрет `mcp__*`, dontAsk + permission-prompts none, пустой strict MCP config,
пустой setting-sources, отключённые hooks/connectors/memory/slash commands/Chrome,
no-session-persistence, max-turns 4, system-prompt-snapshot off. Не используются
resume, bypass permissions, fallback model, extra directories или пользовательские
настройки. `--bare` не выбран: он не читает subscription OAuth и не подходит
как прозрачная интеграция с существующим входом пользователя.

Flags ограничивают поведение CLI, но сами по себе не изолируют процесс.
`safe-mode` не отключает всю managed policy; её сохранение ещё требует отдельной
квалификации. Не выдаём отсутствие enterprise настроек в private HOME за её поддержку.

## Offline runtime

`linux-x86_64-bwrap-claude-offline-v1` добавляет к private PID/network namespace
read-only procfs, **только** `/dev/null` и `/dev/urandom` через device bind,
и фиксированные переменные:

```text
CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1
CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL=1
ENABLE_CLAUDEAI_MCP_SERVERS=false
CLAUDE_CODE_MAX_RETRIES=0
```

Native Bun требует urandom; существующие профили без устройств не меняются.
Device mounts допускают открытие read/write; это не монтаж всего host `/dev`.
Без выбранного auth используются пустые HOME/tmp на каждом запуске;
read-only context и staging отдельных runtime-файлов сохраняют общий
[контракт runner](runner.md). Version probe выполняется без context/auth.

## Явный auth-файл: только offline

`claude::SelectedAuthFile::select` принимает абсолютный путь к одному приватному
`.credentials.json`. Общая с Codex внутренняя реализация закрепляет inode через
`O_PATH | O_NOFOLLOW | O_CLOEXEC`, проверяет owner/mode/тип/размер/число ссылок.
Host не читает, не разбирает и не копирует содержимое. Типы выбора Claude и Codex
раздельные, путь назначения и config env фиксированы для каждого провайдера.

`OfflineRunner::run_claude_with_auth` разрешён только для точной версии и
Claude offline profile. Один файл монтируется read-only в private
`/home/agent/.claude/.credentials.json`; `CLAUDE_CONFIG_DIR` указывает на эту
папку. Соседние settings/hooks/MCP/history не передаются. Descriptor для mount
закрыт до запуска payload. Проверка версии по-прежнему не получает auth.

Это capability выбранного файла, не проверка входа. `O_PATH` закрепляет inode,
но не защищает от одновременной записи в него на host. Read-only mount не запрещает
серверную ротацию токена; сеть в этом профиле изолирована. Сохранение managed policy,
default-hostname HTTPS, expiry/revoke и refresh требуют следующих срезов.

## Результат

Установленный CLI с `--json-schema` и пустыми built-in tools объявляет ровно
один внутренний инструмент **StructuredOutput**. Проверенный порядок:

```text
system/init
assistant: StructuredOutput(input)
user: успешный tool_result
result: success, is_error=false, structured_output
EOF + exit 0
```

Возможны текст/thinking перед StructuredOutput и известные idle/rate notices
после result. Неизвестные события, дополнительные tools, ошибочные notices,
потерянные/повторные события или другая session отклоняются. `num_turns` не равно
числу пользовательских запросов: в записанном успешном transcript значение 2.

Decoder проверяет версию, cwd, точный model ID, dontAsk, отсутствие MCP/plugins/
skills/slash commands, отключённые analytics/feedback, единственный StructuredOutput
и его подтверждение. Принятый `structured_output` должен совпадать с его input;
строка `result` не используется как результат анализа. `success + is_error:true`,
permission denials, subagents, server tools, очередь следующего turn или другая
модель не принимаются. Невалидный exit/timeout/cancel имеет приоритет над JSON.

Все JSON objects проверяются на повторные ключи **до** перехода к `Value`,
включая вложенный результат. Затем выполняются typed deserialize и обязательный
локальный validator. `RecipeRequest` связывает шесть compiled recipes с schema
и точными evidence revisions. Decoder не сохраняет результат и не двигает baseline.

Пределы: stdout 8 MiB, одна запись 2 MiB, результат 1 MiB, 4096 событий.
Ошибки и Debug не включают исходный текст, stderr или произвольные поля провайдера.
Успешная валидация ссылок доказывает их принадлежность контексту, не истинность вывода.

## Проверки

```sh
cargo test -p tgsum-runner --all-features --locked --test claude
cargo test -p tgsum-runner --all-features --locked --test claude fake_cli -- --ignored --nocapture
cargo test -p tgsum-runner --all-features --locked --test claude selected_auth -- --ignored --nocapture
TGSUM_CLAUDE_TEST_BINARY=/absolute/path/to/native/claude \
  cargo test -p tgsum-runner --all-features --locked --test claude installed_cli -- --ignored --nocapture
cargo test -p tgsum-runner --all-features --locked --test isolation -- --ignored --nocapture
bash scripts/check.sh
```

Последняя process fixture запускает **реальный executable с вымышленным ключом**
и локальным canned Messages server внутри того же offline namespace. Внешнего
inference, существующих профилей и аккаунтов нет. Проверяются все шесть draft-07
schemas и ошибка HTTP 401. Отдельная fixture передаёт вымышленный subscription
OAuth через выбранный файл без API-key/token env: native CLI отправляет ожидаемый
Bearer, host-файл не меняется, access/refresh sentinels отсутствуют в stdout/stderr.
В этих проверках `ANTHROPIC_BASE_URL` ведёт на private loopback HTTP; это не проверка
production TLS или server-managed policy. Fake CLI независимо проверяет argv, env, stdin, scope,
успех, ошибки, timeout/cancel. Другая версия/профиль не получает fallback вне sandbox.
Игнорируемые process tests нужно запускать явно; обычный gate не доказывает их прохождение.

До завершения auth/egress/review срезов Claude остаётся Export only в приложении.
Реальные аккаунты и OS/enterprise qualification — `tgsum-t8t.19` под контролем
пользователя. Offline fixture не заменяет эту приёмку.
