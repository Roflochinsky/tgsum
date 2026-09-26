# Codex adapter: контракт CLI и граница cloud inference

Проверено: **2026-09-26**. Задача: **tgsum-hzm.7**.
Это исследование официальной документации OpenAI и локального CLI help;
реальная авторизованная квалификация относится к **tgsum-t8t.19**.

## Решение о поддержке

`codex exec` подходит как интерфейс одноразового анализа: stdin, JSONL events
и JSON Schema для финального ответа документированы. Его wire adapter можно
реализовывать и проверять отдельно от авторизации. Однако существующий
[offline runner](runner-isolation-2026-09-26.md) с private HOME, allowlist
файлов и запрещённой сетью **не является готовым cloud adapter**.

Облачный запуск с существующей авторизацией CLI требует отдельной реализации
границы auth/inference и проверки на выбранной версии. Native sandbox Codex
ограничивает команды; он не доказывает изоляцию всего процесса Codex.
До появления проверенного launch profile поддержка cloud analysis остаётся
**planned / unqualified**, с доступным `Export only`. Это решение не закрывает
оставшуюся реализацию и не сокращает цель проекта до mock-only.
[Permissions: scope and enforcement](https://learn.chatgpt.com/docs/permissions#scope-and-enforcement).

## Что именно проверено

Родительский агент получил в очищенных HOME/env, без авторизации:

- executable: `/home/nikitatrubaev/.local/share/mise/installs/codex/0.155.1/bin/codex`;
- `codex --version`: `codex-cli 0.155.1`;
- `codex --help` и `codex exec --help`; результаты прочитаны из
  `/tmp/tgsum-codex-cli-research/{version,help,exec-help}.txt`;
- `codex features list` в новой очищенной среде; результат прочитан из
  `/tmp/tgsum-codex-cli-research/features.txt`;
- формат установленного executable: ELF, по проверке родительского агента.

Временные help-файлы не являются постоянными fixtures. Исследователь не запускал
CLI повторно, не читал реальные config/auth/credentials, не вызывал login/status,
не запускал анализ и не обращался к аккаунтам. Источники ниже прочитаны на
официальных доменах; Markdown получен через HTTPS, поскольку web reader не
поддержал MIME `text/markdown`. Позже при реализации отдельно просмотрен
versioned `exec_events.rs`; дополнение ниже.

Документация обновляется независимо от установленного бинарника. В changelog
0.155.1 от 18 сентября исправляет default reasoning summaries в TUI;
на дату исследования уже опубликован 0.157.0. Это не основание автоматически
обновлять CLI или переносить текущую JSON schema на 0.155.1.
[Changelog](https://learn.chatgpt.com/docs/changelog).

## Минимальный argv-контракт

Ниже проект контракта для реализации, а не готовый cloud launch profile.
Каждый элемент передаётся как отдельный аргумент процесса; shell не нужен.
`/context` и schema — проверенные файлы нового analysis run, не raw export.

```json
[
  "<absolute-qualified-codex-executable>",
  "--ask-for-approval", "never",
  "exec",
  "--ignore-user-config",
  "--ignore-rules",
  "--ephemeral",
  "--skip-git-repo-check",
  "--sandbox", "read-only",
  "--cd", "/context",
  "--color", "never",
  "--json",
  "--output-schema", "/context/result.schema.json",
  "-"
]
```

У 0.155.1 `--ask-for-approval` перечислен в root help; ставить его до `exec`.
Остальные флаги выше есть в `exec --help`. `-` читает весь prompt из stdin;
переданный одновременно prompt argument превращает stdin в дополнительный
контекст. `--skip-git-repo-check` нужен для bundle вне Git.
[Developer commands](https://learn.chatgpt.com/docs/developer-commands?surface=cli#cli-codex-exec).

`--full-auto` отсутствует в просмотренном help 0.155.1, хотя текущая документация
описывает deprecated compatibility path. В адаптер его не добавлять.
`--remote` документирован для TUI и некоторых interactive subcommands;
поддержку `codex exec --remote` из этого вывести нельзя.
[Developer commands](https://learn.chatgpt.com/docs/developer-commands?surface=cli).

## Events, финальный результат и ошибки

По документации обычный exec пишет progress в stderr, final message в stdout;
`--json` превращает stdout в поток JSONL. Документированы типы `thread.started`,
`turn.started`, `turn.completed`, `turn.failed`, `item.*`, `error`.
Пример финального item имеет `item.type = "agent_message"`, `item.text`;
`turn.completed` может содержать `usage`. `--output-schema` задаёт форму
финального ответа, не форму event stream. `--ephemeral` отключает session rollout
files; это не обещание отсутствия всех logs/cache или удаления у провайдера.
[Non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode).

**Решение tgsum для парсера:** ограничить размер строки/потока, проверить UTF-8
и JSON, учитывать terminal outcome, exit status, timeout/cancel, а затем
валидировать финальный JSON и evidence references независимо от модели.
Не принимать произвольный JSON из stderr, промежуточный agent message или
обрыв потока за результат. Не сохранять stdout/stderr целиком в обычные logs:
там могут быть фрагменты bundle. Конкретную политику по новым event types
зафиксировать в versioned parser.

**Не установлено:** полная JSONL schema `exec` 0.155.1; исчерпывающая таблица
exit codes; стабильная форма всех `turn.failed`/`error`; соответствие каждого
auth/transport failure числовому exit code; все ограничения поддерживаемого
JSON Schema subset. Перечисленных на странице event names недостаточно для
таких гарантий. App-server `codexErrorInfo` нельзя автоматически считать
форматом ошибок `exec`.

**Дополнение при реализации:** прочитан официальный tagged source
[`exec_events.rs` для rust-v0.155.1](https://raw.githubusercontent.com/openai/codex/rust-v0.155.1/codex-rs/exec/src/exec_events.rs).
Он уточняет поля `usage` (в том числе default для `cache_write_input_tokens`),
форму текстовых/reasoning/todo items и имена ошибок/tool events. Decoder
реализует строгое подмножество этого wire format. Таблица типов не доказывает
runtime ordering, фактический tool inventory или соответствие всех failure
состояний CLI этим событиям; реальная квалификация по-прежнему не проведена.

Нужны synthetic transcript fixtures для success, malformed/truncated stream,
nonzero exit, schema mismatch, timeout, cancel и failure после частичного ответа.
Реальный success/expired-auth/rate-limit contract дополнительно проверяется
пользователем. Отказ/ошибка должны сохраняться как неуспешный analysis run,
без публикации частичного результата как готового.

## Конфигурация и отключение возможностей

`-c key=value` принимает TOML; `--disable NAME` эквивалентен
`features.NAME=false`. Root/exec help подтверждают синтаксис, но не весь набор
feature names. `--strict-config` полезен для неизвестных config fields;
успешный help не доказывает, что все overrides распознаны и действуют.
[Developer commands](https://learn.chatgpt.com/docs/developer-commands?surface=cli).

| Поверхность | Документированный контроль | Предел доказательства |
| --- | --- | --- |
| Web search | `-c 'web_search="disabled"'` | Не `--search`; command network off сам не отключает hosted search |
| Shell | `--disable shell_tool` | Выключает default shell tool; не является глобальным запретом всех tools |
| PTY exec / snapshots | `--disable unified_exec`, `--disable shell_snapshot` | Проверять одновременно с shell control; смена exec implementation не равна изоляции |
| Hooks | `--disable hooks` | Managed requirements могут требовать hooks; конфликт должен давать отказ квалификации |
| Apps | `--disable apps` | Отдельная поверхность, не управляется command network policy |
| Remote plugin catalog | `--disable remote_plugin` | Отключение каталога не доказывает выключение уже установленных plugins |
| Subagents / continuation / memories | `--disable multi_agent`, `--disable goals`, `--disable memories` | Не нужны для одного ограниченного analysis run |

Назначение этих feature flags и web mode описано в
[Config basics](https://learn.chatgpt.com/docs/config-file/config-basic).
Hooks из managed sources могут быть закреплены через requirements; пропуск
пользовательского config этого не отменяет.
[Hooks](https://learn.chatgpt.com/docs/hooks#managed-hooks-from-requirementstoml).

Для MCP описаны `mcp_servers.<name>.enabled=false`, allow/deny lists tools,
а также отдельные настройки plugin MCP. Не найден документированный глобальный
`--no-mcp`. Нельзя без проверки считать `-c mcp_servers={}` удалением всех
унаследованных серверов. При создании стерильной среды источники MCP config
должны отсутствовать либо иметь проверенную эффективную deny policy.
[MCP](https://learn.chatgpt.com/docs/extend/mcp).

Для plugins текущая JSON schema содержит `features.plugins` и
`plugins.<plugin>.enabled`; configuration reference также описывает
управляемый запрет plugins. Для standalone skills описаны отключения по
`[[skills.config]]` и `path`, `enabled=false`. Discovery включает repository,
`$HOME/.agents/skills`, `/etc/codex/skills`, bundled skills и symlink targets.
Одного пустого `$CODEX_HOME/config.toml` для очистки skills недостаточно.
[Config schema](https://learn.chatgpt.com/docs/config-schema.json),
[Build skills](https://learn.chatgpt.com/docs/build-skills#enable-or-disable-local-codex-skills).

**Дополнительная локальная проверка 0.155.1:** `features list` перечисляет
`shell_tool`, `unified_exec`, `shell_snapshot`, `hooks`, `apps`, `multi_agent`,
`goals`, `memories`, `plugins`, `remote_plugin`, `browser_use`,
`browser_use_external`, `browser_use_full_cdp_access`, `computer_use`,
`code_mode`, `code_mode_host`, `skill_search`, `skip_host_skill_discovery`,
`view_image`. `skip_host_skill_discovery` помечен under development;
`apply_patch_freeform`, `js_repl` и `plugin_hooks` — **removed**.
Последние нельзя использовать как доказательство отключения соответствующих
возможностей. Inventory подтверждает имя/состояние feature, не отсутствие
всех инструментов в реальном inference request.

**Schema-only кандидаты:** `skills.bundled.enabled=false`,
`skills.include_instructions=false`. Для всей группы нужны поведенческие
проверки. Наличие bool в текущей schema не доказывает полную семантику,
совместимость версии или отсутствие альтернативной реализации инструмента.
`skills.include_instructions=false` по описанию убирает instructions block;
это само по себе не запрет чтения skill files.
[Config schema](https://learn.chatgpt.com/docs/config-schema.json).

Есть расхождение источников: reference перечисляет `tools.view_image`,
но текущая JSON schema не включает его в `ToolsToml` и содержит
`features.view_image`. Нельзя молча выбрать старый ключ и назвать tool
отключённым. Единого документированного `--no-tools` не найдено.
Для release qualification требуется фактический inventory tools на выбранной
версии и внешнее ограничение ресурсов.
[Config reference](https://learn.chatgpt.com/docs/config-file/config-reference),
[Config schema](https://learn.chatgpt.com/docs/config-schema.json).

## Наследование config и инструкций

`--ignore-user-config` пропускает **только** `$CODEX_HOME/config.toml`;
установленный help прямо сохраняет использование `CODEX_HOME` для auth.
`--ignore-rules` пропускает user/project execpolicy files; это не отключение
AGENTS.md, skills, hooks, системной конфигурации и managed requirements.
Документированный порядок включает CLI, trusted project, selected profile,
user, cloud-managed defaults, `/etc/codex/config.toml`, built-ins.
[Config basics](https://learn.chatgpt.com/docs/config-file/config-basic#configuration-precedence).

**Вывод для runner:** отдельные HOME/CODEX_HOME, cwd без project config и
инструкций, очищенные env/FD, отсутствие discovery roots и точная OS allowlist
остаются необходимыми. Нельзя открывать настоящий HOME ради auth. Config
пользователя не следует читать, копировать или временно менять. Нельзя обходить
административную политику: несовместимая политика делает profile неподдержанным.

Отдельные profile files с 0.134.0 заменили старые `[profiles.name]` таблицы.
`--profile` наслаивает file на base user config, поэтому не служит clean-room
режимом. Установка `CODEX_HOME` меняет state root, включающий config, auth,
history, logs и caches.
[Advanced configuration](https://learn.chatgpt.com/docs/config-file/config-advanced).

## Auth и network: проверенные альтернативы

### Обычный exec и существующая авторизация

По умолчанию exec использует сохранённую авторизацию CLI. Codex хранит её в
`CODEX_HOME/auth.json` либо OS credential store; managed ChatGPT auth обновляет
tokens во время работы. Режимы `file`, `keyring`, `auto`, `ephemeral` имеют
разную доступность/персистентность. Из этого не следует, что пустой private
CODEX_HOME автоматически получит существующий вход.
[Authentication](https://learn.chatgpt.com/docs/auth#credential-storage).

**Вывод:** нельзя добавить только network access в текущий runner и объявить
cloud support. Требуются и transport, и способ дать официальному Codex ровно
его auth capability. Копирование `auth.json`, передача tokens через env и
монтирование всего пользовательского профиля не удовлетворяют выбранной
[границе продукта](../specs/context-gateway.md#agent-adapters).

Отдельный mount существующего auth-файла без чтения/копирования его содержимого
tgsum теоретически отличается от копирования профиля, но **не квалифицирован**:
нужно решить refresh/atomic replacement, concurrency, keyring backend,
доступ model tools к этому mount и разрешённый scope всего harness. Это
инженерная гипотеза, не документированный готовый auth broker и не выбранный
launch profile.

Другая гипотеза — новый выделенный private CODEX_HOME, в котором пользователь
сам проходит официальный Codex-managed login. Тогда tgsum не переносит tokens,
а Codex владеет ими с самого начала. Это **новый пользовательский вход**,
не reuse existing sign-in; требуются user-controlled setup, тот же отдельный
сетевой profile и проверка недоступности auth для model tools. В исследовании
такой вход не запускался.

### App-server, который сам управляет auth

App-server предназначен для custom clients. Managed ChatGPT mode оставляет
OAuth flow, storage и refresh у Codex. Stdio transport — JSONL JSON-RPC,
без поля `jsonrpc`; старт требует initialization handshake. Версионные schemas
можно генерировать из установленного CLI через
`codex app-server generate-json-schema --out <dir>`. Это отдельный протокол,
не JSONL `exec`.
[App-server](https://learn.chatgpt.com/docs/app-server).

**Возможное направление:** доверенный официальный app-server владеет auth,
а tgsum передаёт только подготовленный bundle и принимает ограниченный result
channel. Но размещение app-server на обычном host сохраняет его доступ к HOME;
native sandbox не устраняет эту проблему. Нужна дополнительная OS boundary
для harness либо документированный узкий inference broker. Полный app-server
RPC также нельзя выдавать недоверенному процессу без ограничения методов:
он содержит команды, filesystem и account operations.

Режим `chatgptAuthTokens` требует от host application самого `accessToken`
и управления refresh, поэтому **не выбран** для no-copy integration.
Remote Code Mode host переносит code execution host, но документирован как
experimental, unsupported for production; доказательства переноса всех
filesystem surfaces harness в этот host нет. App-server на странице также
имеет experimental production caveat.
[App-server: auth modes and remote host](https://learn.chatgpt.com/docs/app-server).

### Native exact filesystem scopes

Текущие permission profiles поддерживают `read/write/deny`, exact paths,
`:minimal`, отключение command network. Они несовместимы с одновременным
выбором старого `--sandbox`: старый флаг может переключить режим обратно.
App-server `readOnly.access` допускает
`{ "type": "restricted", "includePlatformDefaults": false, "readableRoots": [...] }`;
по умолчанию read access — `fullAccess`.
[Permissions](https://learn.chatgpt.com/docs/permissions),
[App-server: ReadOnlyAccess](https://learn.chatgpt.com/docs/app-server#sandbox-read-access-readonlyaccess).

**Вывод:** это полезный второй слой для model-generated commands. Его
документированный scope не охватывает весь harness. Network proxy исключает
model/auth service requests, MCP, apps, browser/Computer Use и web search;
cloud inference может работать при запрете command network, но это не
сетевая allowlist всего Codex.
[Agent approvals & security](https://learn.chatgpt.com/docs/agent-approvals-security#traffic-outside-the-command-network-proxy).

### Responses API proxy, Agents API и credential broker

GitHub Action официально запускает Responses API proxy для API key, затем
`codex exec`. Это подтверждает реалистичность отделения API secret от
agent process, но страница не документирует переиспользование существующего
ChatGPT OAuth cache таким proxy. Нельзя считать его готовым решением для
пользовательского входа CLI.
[GitHub Action](https://learn.chatgpt.com/docs/github-action).

Agents API отдельно запускает hosted harness и `codex exec-server` в
self-hosted sandbox: executor имеет ограниченный environment key, application
key остаётся снаружи. Это другой API/auth путь с отдельными capabilities;
он не заявляет переиспользование подписки/авторизации локального CLI.
[Self-hosted sandboxes](https://developers.openai.com/api/docs/guides/agents-api/environments/self-hosted).

В текущей config schema также есть `features.network_proxy.credential_broker`
и environment-backed credential providers. Changelog 0.155.0 упоминает эту
реализацию и hardening shell snapshots. Это evidence наличия механизма,
но не опубликованный auth-only ChatGPT inference endpoint для tgsum.
[Config schema](https://learn.chatgpt.com/docs/config-schema.json),
[Changelog](https://learn.chatgpt.com/docs/changelog).

**Результат поиска:** в прочитанных официальных источниках не установлен
готовый стабильный интерфейс, который одновременно переиспользует имеющуюся
ChatGPT auth, держит tokens вне tgsum и обеспечивает OS allowlist всего
harness без HOME/raw exports/connector secrets. Это ограничение текущего
доказательства, не утверждение технической невозможности.

## Следующие проверяемые этапы

1. Versioned argv builder и строгий JSONL decoder реализованы и проверяются
   на synthetic fixtures: [контракт и границы](../development/codex-adapter.md).
   Финальный typed result требует обязательного recipe/evidence validator;
   это не универсальная реализация JSON Schema. Конкретные recipes ещё нужны.
2. На основании собранного inventory **0.155.1** проверить неизвестные config
   keys и эффективный tool catalog в изолированной среде без реальных аккаунтов.
   Проверить, что hooks/MCP/skills не стартуют и не читаются при запуске.
3. Реализовать выбранный auth/inference transport и OS profile всего процесса;
   доказать allowlist filesystem/network/IPC, auth ownership, refresh и cleanup.
   Отдельно проверить обходы через встроенные tools, symlinks и inherited state.
4. Под контролем пользователя выполнить реальную квалификацию
   **tgsum-t8t.19**: выбранная auth method, модель, version/OS, success,
   auth expiry, cancellation, rate limit и отсутствие лишнего data handoff.
5. После evidence обновить registry/support status и Review UI с настоящим
   получателем и полномочиями. Одни metadata, help checks или mocks поддержку
   cloud analysis не доказывают.

Этапы 1–3 — остающаяся инженерная работа, которую нельзя заменить переносом
одной проверки аккаунта в backlog. Исследование не даёт основания закрывать
эпик runner/adapters целиком.
