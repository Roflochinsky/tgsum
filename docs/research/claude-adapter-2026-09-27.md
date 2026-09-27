# Claude Code adapter: контракт и границы

Проверено **2026-09-27**, срез `tgsum-hzm.8`. Первоначальный сбор
источников описан ниже; выполненные затем offline проверки вынесены в последний
раздел. При сборе первичных источников читались только публичные документы и
пакеты. Пользовательские профили и реальные credentials не читались.

## Версии и воспроизводимые источники

- [CLI package 2.1.280](https://registry.npmjs.org/@anthropic-ai/claude-code/2.1.280):
  опубликован 2026-09-22T15:44:39.443Z. Пакет распространяет binary/wrapper;
  наличие package не даёт открытого исходного кода всего CLI.
- [SDK package 0.3.280](https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/0.3.280):
  опубликован 2026-09-22T15:51:11.813Z. Прочитаны `package/sdk.d.ts` и `sdk.mjs`
  из [фиксированного tarball](https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/-/claude-agent-sdk-0.3.280.tgz).
  Это соседняя версия SDK с типами протокола, а не доказательство каждого
  runtime-события native CLI.
- [Changelog, commit 7779afb](https://github.com/anthropics/claude-code/blob/7779afb12e3635f46f56ec823979d68350ae000b/CHANGELOG.md):
  commit 2026-09-25T21:49:55Z; `--bare` добавлен в 2.1.81, `--safe-mode` —
  в 2.1.169; 2.1.205 исправил молчаливое игнорирование invalid JSON Schema.
  На дату чтения npm latest — 2.1.283 / SDK 0.3.283; они **не** выбраны
  автоматически вместо установленной 2.1.280.

Веб-документы ниже не имеют закреплённой версии/даты публикации; дата выше —
дата чтения. Упоминания новых флагов нужно сверять с выбранным binary.
Попытка скачать `.md` варианты docs через Python получила HTTP 403;
HTML-страницы доступны через web tool. Не считать эту ошибку проверкой markdown.

## Факты: ввод, результат, ошибки

`-p` читает stdin; аргумент prompt необязателен для pipe. `json` выдаёт envelope,
`stream-json` — NDJSON; для подробного stream используется `--verbose`.
Документированный предел stdin — 10 MB. Неверные flags могут завершиться
со stderr до старта; ошибка внутри запуска может оказаться в stdout/result.
Обычный успех имеет exit 0, ошибка — ненулевой exit. SIGTERM завершает процесс
с 143 без результата незавершённого turn; могут исполняться SessionEnd hooks.
[Headless reference](https://code.claude.com/docs/en/headless).

В SDK 0.3.280 `SDKSystemMessage` (`system/init`) объявляет
`claude_code_version`, `cwd`, `model`, `tools`, `mcp_servers`, `permissionMode`,
`skills`, `plugins`, `apiKeySource`. Последний **не идентифицирует** OAuth:
`none` также допускает bearer/cloud. `SDKResultMessage` — один result на turn;
после него допустимы информационные system events. Single-prompt режим
завершает процесс после turn. `subtype:success` может иметь `is_error:true`
при API error. Ошибочные subtype: `error_during_execution`, `error_max_turns`,
`error_max_budget_usd`, `error_max_structured_output_retries`. Есть
`permission_denials`, `errors` у error, `structured_output` у success;
`result_index` необязателен. SDK сериализует `settingSources:[]` в
`--setting-sources=`, `tools:[]` в два аргумента `--tools`, `""`.
[Типы и argv builder 0.3.280](https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/-/claude-agent-sdk-0.3.280.tgz)
(`sdk.d.ts`: `SDKSystemMessage`, `SDKResultMessage`, `Options`; `sdk.mjs`).

`--json-schema` возвращает данные в `structured_output`, не требует парсить
текст `result`. Документация указывает **draft-07**: декларация более нового
draft отвергается; `format` является аннотацией. Даже success может не содержать
structured output. Validator retries не гарантируют результат. Семантика evidence
и достоверность выводов этим validator не проверяются.
[Structured outputs](https://code.claude.com/docs/en/agent-sdk/structured-outputs).

## Факты: конфигурация и полномочия

`--tools ""` убирает builtin tools, но не MCP. Для MCP есть
`--strict-mcp-config`, `--mcp-config` и deny `mcp__*`. `--safe-mode` выключает
customizations, сохраняя auth; managed hooks и некоторые managed commands
остаются. `--no-session-persistence` отключает сохраняемые/resumable sessions.
`--system-prompt` заменяет prompt; `--disable-slash-commands` выключает
skills/commands. Флаги не создают OS sandbox.
[CLI reference](https://code.claude.com/docs/en/cli-reference).

`--bare` не читает OAuth credentials/keychain и рассчитан на API key/helper
или отдельные cloud credentials. Обычный `-p` загружает hooks/MCP из проекта
без интерактивного trust dialog. Поэтому **bare не является прозрачным
режимом existing subscription login**.
[Headless: bare mode](https://code.claude.com/docs/en/headless#start-faster-with-bare-mode).

`dontAsk` отказывает только действиям, требующим approval; допустимые чтения и
заранее разрешённые tools остаются. Это не read-only sandbox.
[Permissions](https://code.claude.com/docs/en/permissions#permission-modes).

Порядок settings: managed → CLI → local → project → user. Пропущенные поля
сохраняют значения нижних уровней; списки обычно объединяются. Settings могут
перечитываться во время работы. `--setting-sources` перечисляет user/project/local,
не managed tier. Нельзя считать пустые обычные settings заменой managed policy.
[Settings precedence](https://code.claude.com/docs/en/settings#settings-precedence).

`disableAllHooks:true` из CLI не отключает managed hooks. Hooks из разных
уровней объединяются. Любые hooks требуют отдельного учёта в процессе/egress,
даже если model tools выключены.
[Hooks](https://code.claude.com/docs/en/hooks#disable-or-remove-hooks).

## Факты: auth, сеть, retention

Linux хранит login в `~/.claude/.credentials.json` (0600), Windows — в профиле,
macOS — Keychain с файловым fallback. `CLAUDE_CONFIG_DIR` меняет location и
Keychain identity. Документированный выбор auth допускает cloud/gateway,
bearer, API key, helper, setup-token, Anthropic profiles и subscription OAuth;
API key в окружении `-p` имеет приоритет над subscription. Console login
может быть Anthropic profile, а не `.credentials.json`. Поэтому импорт одного
файла не доказывает поддержку всех авторизаций. Expired/unrefreshable login
прекращает запросы; восстановление требует login пользователя.
[Authentication](https://code.claude.com/docs/en/authentication).

Inference использует `api.anthropic.com`; на том же host есть flags/telemetry.
OAuth exchange/refresh/revoke используют `platform.claude.com`, auth также
`claude.ai`. Cloud connectors используют `mcp-proxy.anthropic.com` и по
умолчанию включены для claude.ai login. HTTPS proxy и дополнительный CA
поддержаны; произвольный `ANTHROPIC_BASE_URL` меняет receiver.
[Network configuration](https://code.claude.com/docs/en/network-config).

`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` отключает updates/telemetry/error
reporting/flags, но не official marketplace auto-install. Для него отдельно
`CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL=1`.
`ENABLE_CLAUDEAI_MCP_SERVERS=false` отключает fetch cloud connectors.
`CLAUDE_CONFIG_DIR` включает settings/history/plugins, поэтому открывать
всю директорию как auth capability избыточно. Переменные из settings способны
переопределять унаследованное окружение.
[Environment variables](https://code.claude.com/docs/en/env-vars).

CLI выполняется локально, но prompts/results передаются провайдеру.
Локальный transcript по умолчанию plaintext с retention 30 дней. Cloud retention
зависит от account/preferences: consumer opt-in model improvement — 5 лет,
без него — 30 дней; commercial default — 30 дней, отдельные организации могут
иметь ZDR. `--no-session-persistence` не означает server ZDR и не доказывает
отсутствие всех локальных debug/cache файлов.
[Data usage](https://code.claude.com/docs/en/data-usage).

## Предлагаемый контракт TGSUM: только для синтетического spike

Ниже **argv-массив**, не shell command и не разрешение запускать реальный аккаунт.
`MODEL`, `SCHEMA_DRAFT7`, `SYSTEM_PROMPT` — проверенные значения адаптера;
контекст целиком поступает через ограниченный stdin, затем EOF.

```text
[
  "--print", "--safe-mode", "--input-format", "text",
  "--output-format", "stream-json", "--verbose",
  "--tools", "", "--disallowedTools", "mcp__*",
  "--permission-mode", "dontAsk", "--permission-prompts", "none",
  "--disable-slash-commands", "--no-chrome",
  "--strict-mcp-config", "--mcp-config", "{\"mcpServers\":{}}",
  "--setting-sources=",
  "--settings", "{\"disableAllHooks\":true,\"disableClaudeAiConnectors\":true,\"autoMemoryEnabled\":false}",
  "--no-session-persistence", "--max-turns", "4",
  "--model", MODEL, "--json-schema", SCHEMA_DRAFT7,
  "--system-prompt", SYSTEM_PROMPT
]
```

Это инженерное предложение на основе источников выше. Не добавлять resume,
fallback model, extra directories, plugin paths, debug output или bypass permissions.
Лимит четыре turn — первоначальный test parameter, не доказанный минимум:
structured output может потребовать внутренний tool/повтор; его реальную форму
нужно установить canned server fixture, не объявлять любой `tool_use` атакой.

Предлагаемая оболочка: отдельные mount/user/network namespaces, private runtime
и cwd, ограниченные CPU/time/output, очищенное окружение, явный receiver gateway;
только синтетический auth fixture в явно выбранном location. Установить три
переменные отключения трафика/marketplace/connectors выше. Ни реальный профиль,
ни tgsum project store/исходный архив, ни host shell/socket/keyring туда не входят.
При managed policy, которую оболочка не умеет сохранить, поддержку оставить
неподтверждённой; приватный runtime не считать способом её обхода.

Предлагаемое принятие результата: bounded NDJSON → проверенные init/version/model
и scope → ровно один успешный result, `is_error:false`, без permission denials →
typed `structured_output` → локальная recipe/evidence validation → exit 0,
EOF и чистый gateway → атомарное сохранение/baseline. Не доверять одному subtype,
строке текста или раннему result до завершения процесса. Любой неразобранный
terminal error/прерванный stream/ошибочный exit не двигает baseline.

## Что ещё требуется доказать

1. На native **2.1.280**: точные flags, stdin/EOF, init/result ordering,
   StructuredOutput при пустом tools, все шесть recipe schemas в draft-07.
2. Synthetic TLS peer: действительные endpoint/headers, fresh/expired OAuth,
   missing auth, 401/429, reconnect, denial чужого receiver, отсутствие утечки
   fixture tokens в stdout/stderr. Документация не задаёт полный wire contract.
3. Негативные process fixtures: fake hooks/plugins/MCP/CLAUDE.md вне scope,
   corrupt/oversized output, duplicate result, success+is_error, cancel/timeout,
   process tree cleanup, остаточные файлы. Не заменять эти проверки разбором flags.
4. Managed settings/server policy, macOS Keychain, Windows runtime, Anthropic
   profiles и реальные account flows — отдельная квалификация; участие пользователя
   уходит в backlog по правилам проекта. До evidence нельзя повышать support.

Исследование поддерживает продолжение реализации с fixtures; оно не подтверждает
живой Claude inference, сохранение enterprise policy или доступность моделей.

## Локальные проверки реализации, 2026-09-27

Этот раздел добавлен после сбора первичных источников. Среда: Omarchy 4.0.4,
Linux 7.2.5-3-omarchy x86_64, bubblewrap 0.12.0. Проверялся выбранный native
CLI **2.1.280**, ELF 233 709 640 bytes, с Bun 1.4.3. Пользовательский profile,
реальные keys, OAuth и внешнее inference не использовались.

### Runtime

Native binary и отдельные библиотеки glibc/libgcc перенесены в private read-only
staging. В namespace доступны `librt.so.1`, `libc.so.6`, `libpthread.so.0`,
`libdl.so.2`, `libm.so.6`, `libgcc_s.so.1`, loader `ld-linux-x86-64.so.2`.
Системные каталоги целиком не монтировались. Точная версия возвращает
`2.1.280 (Claude Code)\n`; `--help` — 21 644 bytes и exit 0.

Без `/dev`, а также только с `/dev/null`, Bun abort-ится до вывода версии.
С null + urandom version/help и canned inference проходят. Отдельная попытка
с read-only remount urandom не завершила version probe за 7 секунд; этот вариант
не принят. Реализован отдельный `claude-offline-v1` с двумя device bind,
private procfs, очищенным environment и прежней полной сетевой изоляцией.
Никакого автоматического ослабления старых profiles нет.

### Реальный print protocol с готовым ответом

`runner/tests/support/claude_server.rs` запускает native CLI и localhost HTTP
Messages server внутри одного offline network namespace. Только fixture задаёт
`ANTHROPIC_BASE_URL` и вымышленный `ANTHROPIC_API_KEY`. В production request API
нет base URL или произвольного environment.

При штатном ответе наблюдались `HEAD /api/hello` и один
`POST /v1/messages?beta=true`, stream=true. В tools API-запроса присутствует
**только StructuredOutput**, с точной схемой recipe; shell/file/MCP tools отсутствуют.
Все шесть schema из core успешно прошли native CLI validation. Они не объявляют
`$schema` и используют совместимый набор draft-07 keywords.

Образец `runner/tests/fixtures/claude-success.jsonl` записан с native CLI и
готового синтетического ответа; session/event IDs и timestamps нормализованы.
Последовательность: init → assistant tool_use StructuredOutput → user tool_result
→ success result с structured_output. `num_turns=2`, stop_reason=tool_use.
Source/host paths и реальные credentials в fixture отсутствуют. Формат canned SSE
построен по [Messages streaming](https://platform.claude.com/docs/en/build-with-claude/streaming).
Это не проверка модели или качества анализа.

### Retries и HTTP 401

Первый 401 fixture ошибочно ожидал единственный запрос и аварийно завершал
локальный listener после второго; следующее ожидание child могло зависать.
Проход теста после ручного завершения этого процесса **не засчитан** как обработка
401. Проверка усилена до штатного `Termination::Exited`, exit 1; timeout/kill
такой результат не заменяют. Listener оставался bounded.

Без retry override наблюдались пять попыток за 10 секунд, ещё без terminal result.
Документированный `CLAUDE_CODE_MAX_RETRIES` имеет default 10;
[актуальная таблица env](https://code.claude.com/docs/en/env-vars) прочитана отдельно.
В профиль добавлен фиксированный `CLAUDE_CODE_MAX_RETRIES=0`: повторный Run решает
пользователь, transport не должен скрыто продолжать запросы. В этом режиме
canned 401 штатно завершает CLI с exit 1, decoder отвергает результат. Watchdog
не включён. Это локальная ошибка вымышленного ключа, не реальная проверка login.

### Что квалифицировано и что остаётся

Подтверждены ограниченный offline runtime, запрос/протокол, шесть recipes,
ошибочный exit, HTTP 401 без повторов, bounded decode и synthetic cancel/timeout.
Общий isolation suite отдельно проверяет новый proc/dev профиль: host files,
процессы, произвольные устройства и environment остаются вне namespace.
Подробности/команды: [контракт реализации](../development/claude-adapter.md).

Не квалифицированы subscription OAuth, read-only auth capability, TLS/proxy/refresh
контракт, egress destinations, enterprise managed policy, persisted result/baseline
flow и UI для Claude. Эти части остаются работой `tgsum-hzm.8`; реальные аккаунты
и OS-проверки остаются backlog `tgsum-t8t.19`. Статус messenger connectors не повышен.
