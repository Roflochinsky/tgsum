# Проверка границ runner

Срез `tgsum-hzm.12`, 2026-09-27. Проверяются публичные runner/adapter contracts
и фактическая граница процесса: системные вызовы из fixture, запросы native CLI
к локальному серверу и результат, возвращаемый вызывающему коду. Реальные
messenger/agent accounts, существующие auth-профили и внешнее inference не используются.

## Матрица доказательств

Все названия ниже относятся к Linux x86_64. Полные profile IDs начинаются с
`linux-x86_64-bwrap-`. Общий [контракт и ограничения](runner.md) сохраняются.

| Профиль | Read/write | Сеть | Выполнение и протокол |
| --- | --- | --- | --- |
| `offline-v1` | Только public context/runtime; host Project, export, credentials отсутствуют; write запрещён | Host TCP и Unix socket недоступны | Literal argv, env/FD isolation, timeout/cancel/flood/descendants; неизвестный profile/version отвергается |
| `offline-proc-v1` (Codex) | Те же проверки плюс private read-only procfs, отсутствие host root/PIDs | Те же TCP/Unix попытки | Fake Codex проверяет control/data, native 0.155.1 — пустой tool catalog; decoder отвергает tool events |
| `claude-offline-v1` | Те же проверки; только два разрешённых device bind | Те же TCP/Unix попытки | Fake Claude и native 2.1.280; только StructuredOutput; недопустимые tool/result events отвергаются |
| `codex-egress-v1` | Native wrapper пытается открыть host files, Project, config и proc roots, писать context/runtime; auth read-only, соседние настройки отсутствуют | Direct host TCP/Unix запрещены; relay/gateway допускает выбранный origin; неверные CONNECT/SNI/CA и refresh отвергаются | Native CLI через production relay; пустой tool catalog, проверенные schema/evidence; ошибочный transport не становится успехом даже при exit 0 |
| `claude-egress-v1` | Те же попытки, но `/context` пустой; corpus удерживается relay до settings preflight | Только выбранный Anthropic origin через relay/gateway; неверный receiver/CA/refresh отвергается | Исходная managed policy сохраняется, несовместимые hooks/MCP/helpers/settings блокируют передачу corpus; native catalog содержит только StructuredOutput |

### Недоверенный текст и файлы

`runner/tests/isolation.rs` передаёт импорт с 12 вариантами имён: traversal,
absolute/Windows path, shell syntax, leading option, newline, symlink и имена
`AGENTS.md`, `CLAUDE.md`, `.mcp.json`, `.codex/config.toml`, `.claude/settings.json`.
Существующие synthetic files и symlink создаются **до** bundle preparation.
Manifest сообщает 12 references / 0 included attachments. Ни содержимое файлов,
ни их имена как executable/config не добавляются в runtime. Это текущий контракт
без attachment bytes; будущий packager `tgsum-af2.6` требует своей квалификации.

Инструкции из сообщений остаются в `untrusted_documents`, отдельно от recipe,
schema и argv. Fake agents проверяют controls, а native локальные серверы —
реальный tool catalog. Decoder отвергает попытку вернуть tool event, чужое evidence,
неполный или подменённый результат. Эти ограничения не доказывают, что модель
всегда правильно понимает переписку или устойчива ко всем prompt injections.

### Host hooks/MCP и окружение

Offline suite прогоняет все три профиля под hostile parent env, включая
`LD_PRELOAD`, `NODE_OPTIONS`, `BASH_ENV`, `CODEX_HOME`, `CLAUDE_CONFIG_DIR`,
`SSH_AUTH_SOCK` и proxy. HTTPS fixtures помещают synthetic host repo/config и
Project paths в test-only control file. Из child process выполняются реальные
попытки read/overwrite, доступа через `/proc/{1,self}/root`, Unix connect и
запуска `/bin/sh`/`git`. Разрешённые private HOME/tmp остаются writable.

Создать в private `/tmp` файл с тем же текстовым путём, что у host directory,
можно: это другой filesystem. Поэтому overwrite-проверка открывает существующий
файл без `create`, а не объявляет любое создание scratch-файла утечкой.

Для Claude отдельно проверяются original managed files и same-process settings:
helpers, hooks, MCP, plugins, endpoint env и изменение policy после Review
отклоняются. Эти проверки не заменяют квалификацию настоящего enterprise account.

## Воспроизведение

Нужны bubblewrap **0.12.0**, namespaces/close_range, native static Codex **0.155.1**
и native Claude **2.1.280**. Команды запускаются из корня repo; пути ниже надо
заменить путями только к executable. Auth/settings paths не передаются.

```sh
cargo build -p tgsum-runner --all-features --bins --locked
TGSUM_CODEX_TEST_BINARY=/absolute/path/to/native/codex \
TGSUM_CLAUDE_TEST_BINARY=/absolute/path/to/native/claude \
TGSUM_RELAY_TEST_BINARY="$PWD/target/debug/tgsum-codex-relay" \
TGSUM_HTTPS_FIXTURE_BINARY="$PWD/target/debug/tgsum-codex-https-fixture" \
TGSUM_CLAUDE_RELAY_TEST_BINARY="$PWD/target/debug/tgsum-claude-relay" \
TGSUM_CLAUDE_HTTPS_FIXTURE_BINARY="$PWD/target/debug/tgsum-claude-https-fixture" \
TGSUM_SECRET_SENTINEL=SYNTHETIC_ENV \
BASH_ENV=/tgsum-absent-hook.sh NODE_OPTIONS=--require=/tgsum-absent-hook.js \
  cargo test -p tgsum-runner --all-features --locked -- --ignored --test-threads=1
bash scripts/check.sh
```

Игнорируемые проверки запускаются отдельно от обычного gate: их пропуск никогда
не считается квалификацией. В HTTPS suites только внутренний test Dial ведёт
на локальный TLS server с вымышленными credentials; public gateway такого override
не имеет. CONNECT/SNI сохраняют production origin. Скрипт не запускает CLI
с реальным HOME или авторизацией пользователя.

## Пределы результата

Прогон 2026-09-27: Linux **7.2.5-3-omarchy x86_64**, bwrap **0.12.0**,
Codex **0.155.1**, Claude **2.1.280**. Полный gate: **189 passed / 0 failed /
27 ignored**. Отдельная команда выше: **27 passed / 0 failed / 0 ignored** —
8 isolation, 6 Codex process, 5 Claude process, 7 HTTPS suites (32 сценария)
и 1 Claude manifest test. После этого только общий test fixture module
объявлен один раз для Clippy; full gate выполнен на финальном дереве.

Подтверждаются перечисленные технические профили и версии. Windows/macOS,
реальные subscription/enterprise accounts, живые provider policy, installer
roundtrip остаются в соответствующих задачах QA, включая `tgsum-t8t.19/.20`.
Неизвестные profile/version/OS не получают fallback без изоляции.

Gateway ограничивает origin, а не отдельные HTTPS paths; он не расшифровывает
TLS и не определяет назначение каждого запроса к разрешённому host. Auth bytes
читаются выбранным CLI. Общие пределы (same-user host tampering, kernel exploits,
отсутствие общего RAM/CPU/pid budget и crash retention) описаны в runner contract.
