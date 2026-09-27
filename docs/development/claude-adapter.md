# Claude Code: protocol, выбранный auth и TLS qualification

Срез `tgsum-hzm.8`, CLI **2.1.280**, Linux x86_64.
[Первичные источники и ограничения](../research/claude-adapter-2026-09-27.md).
Реализованы `tgsum_runner::claude::{ClaudeRequest, RecipeRequest, decode}` и
отдельный offline профиль с явным выбором одного auth-файла. Fixed HTTPS relay и
внутренний network profile проверены на synthetic TLS peer. **Public cloud runner,
наследование исходной managed policy, managed lifecycle и Desktop Run пока не
подключены.** [Исследование auth/egress](../research/claude-auth-egress-2026-09-27.md).

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
серверную ротацию токена; сеть в этом профиле изолирована. Сохранение исходной managed
policy и поддержка реального refresh требуют следующих срезов; результаты synthetic
HTTPS qualification приведены ниже.

## HTTPS: внутренний профиль квалификации

`tgsum-claude-relay` запускает только `/runtime/claude` с очищенным env, private
HOME, фиксированными proxy variables, пустым NO_PROXY и необязательным публичным
CA через `NODE_EXTRA_CA_CERTS`. Auth берётся из выбранного read-only файла. Gateway
пропускает CONNECT и TLS SNI только для `api.anthropic.com:443`. Refresh/revoke
host `platform.claude.com` запрещён. На разрешённом host есть не только inference,
но и metadata/policy: TLS pass-through не различает HTTP paths.

Внутренний `linux-x86_64-bwrap-claude-egress-v1` используется только тестами.
`OfflineRunner` его отклоняет; public `ClaudeNetworkRunner` ещё нет. Профиль
содержит постоянный managed file с `forceRemoteSettingsRefresh:true`. Это
добавочное ограничение для проверки startup, **не перенос исходной host policy**.

Native CLI проверен с TLS peer, SAN `api.anthropic.com`, synthetic OAuth и без
`ANTHROPIC_BASE_URL`. Проверены 13 сценариев в трёх suites:

- Pro success; Team settings 200/204/404, затем policy_limits и Messages.
- Settings 403, 304 без cache и 200 с `requiredMinimumVersion:99.0.0`: штатный
  exit 1, **ни одного Messages**.
- Неверный CA, другой receiver, HTTP 401, expired token, cancel и timeout.

На expired token native CLI пытается refresh через запрещённый host, но затем
может получить canned Messages result и выйти с 0. `NetworkOutput::decode`
проверяет процесс, затем gateway failures/наличие transport, затем protocol и
еvidence validator. Поэтому такой результат отклоняется. Полный refresh lifecycle
не реализован; реальный токен не обновлялся.

`policy_limits` — отдельный endpoint от managed settings. Synthetic reply имеет
пустые `restrictions`/`compliance_taints`; это не доказательство поддержки настоящей
организации. Перед публичным Run остаются исходная endpoint policy, все её
требования/совместимость и binding Review/result/baseline.

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
skills/slash commands, отключённые analytics, единственный StructuredOutput
и его подтверждение. Принятый `structured_output` должен совпадать с его input;
строка `result` не используется как результат анализа. `success + is_error:true`,
permission denials, subagents, server tools, очередь следующего turn или другая
модель не принимаются. Невалидный exit/timeout/cancel имеет приоритет над JSON.

`product_feedback_disabled` проверяется как boolean: это metadata об организационной
политике, а не факт отправки feedback. При пустой policy_limits restriction он false
даже с отключёнными nonessential traffic и slash commands. Отдельная regression
fixture воспроизводит этот случай; analytics по-прежнему требуется disabled.

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

Для HTTPS suites сначала собрать fixture binaries, затем передать точные пути:

```sh
cargo build -p tgsum-runner --all-features --bins --locked
TGSUM_CLAUDE_TEST_BINARY=/absolute/path/to/native/claude \
TGSUM_CLAUDE_RELAY_TEST_BINARY="$PWD/target/debug/tgsum-claude-relay" \
TGSUM_CLAUDE_HTTPS_FIXTURE_BINARY="$PWD/target/debug/tgsum-claude-https-fixture" \
  cargo test -p tgsum-runner --all-features --locked --lib installed_claude_https -- --ignored --nocapture
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
