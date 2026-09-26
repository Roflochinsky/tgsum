# Codex: подготовка запроса и проверка результата

Часть `tgsum-hzm.7`, реализованная в `tgsum_runner::codex`. Контракт рассчитан
на CLI **0.155.1**; [исследование](../research/codex-adapter-2026-09-26.md)
разделяет проверенные аргументы, wire format и ещё не доказанную изоляцию.

**Это ещё не работающий облачный adapter.** Модуль не запускает Codex,
не читает его профиль/авторизацию и не подключён к UI Run. Реализованы
подготовка запроса и decoder, проверенные на отдельном synthetic executable.
Публичный bundle по-прежнему имеет `destination: export_only`.

## Запрос

`CodexRequest::prepare(context, model, task, schema, cancellation)` принимает
только `PreparedContext`, полученный из проверенного core bundle. Model, task
и result schema задаёт доверенный код recipe/host; поля архива не управляют argv.
Отдельные recipes и их result types остаются задачей `tgsum-hzm.9`.

- В stdin сериализуются инструкция анализа, task и `untrusted_documents`:
  публичный manifest и перечисленные в нём файлы. Каталог исходного экспорта
  не обходится. Неподключённые чаты, private IDs, пути и секреты не добавляются.
- Инструкция задаёт отношение к документам как к данным. JSON escaping сохраняет
  границы полей; оно не является защитой от смысловой prompt injection. Для
  cloud запуска всё равно нужна внешняя изоляция tools и всего процесса.
- Context целиком включён в stdin: агенту не требуется shell, чтобы прочитать
  `/context`. Oversize даёт ошибку, без незаметного обрезания или смены scope.
- Проверяются отмена и revision Project. Ссылка на `PreparedContext` удерживает
  staging; `invocation()` повторно проверяет revision. При будущем запуске
  runner также должен проверять именно `request.context()`.
- Schema хранится в отдельном приватном temporary file. `schema_runtime_file()`
  задаёт фиксированное назначение `/runtime/tgsum-codex-result.schema.json`;
  runner копирует его при qualification. Файл не добавляется в public bundle
  и удаляется при обычном Drop request.
- Аргументы фиксированы: root `--ask-for-approval never`, затем `exec`,
  `--ignore-user-config`, `--ignore-rules`, `--ephemeral`,
  `--skip-git-repo-check`, `--strict-config`, read-only, JSONL, model, schema,
  отключение web search и перечисленных в коде features. Последний аргумент `-`.
  Дополнительные argv, shell command, config path и env caller не передаёт.

Имена features взяты из локального inventory 0.155.1; removed flags не используются.
Это ещё не доказательство отсутствия всех tools, MCP, hooks или config inheritance
в реальном CLI. Native read-only также не изолирует весь harness.

## Результат

`decode<T>(output, validate)` сначала проверяет `Termination::Exited` и exit 0,
затем ограниченный JSONL. Принимается один thread и один turn, текстовые,
reasoning и todo items, последний завершённый `agent_message` и `turn.completed`.
Промежуточный комментарий не обязан быть JSON; завершённый последний ответ обязан.

Незавершённые items, повторы ID, смена item type, повторные starts/completions,
лишние события после completion, неизвестные поля/события и tool/error items
отклоняются. Todo содержит только проверяемые текст/boolean данные; план не
исполняется. Command execution, file change, MCP, web search и collab не принимаются.
JSONL допускает LF/CRLF и отсутствие последнего newline; пустые records запрещены.

Финальный JSON десериализуется в тип `T`. Для recipe нужен строгий тип с
`deny_unknown_fields`. Обязательный callback `validate` проверяет ограничения
recipe и evidence/revision по **тому же bundle**, который прошёл Review.
`--output-schema` — инструкция провайдеру; универсального локального JSON Schema
validator здесь нет. Builder ограничивает размер schema и требует object root,
остальную корректность schema должен обеспечивать доверенный recipe.

Decoder не сохраняет результат, не меняет baseline и не доказывает фактическую
правильность выводов модели. `Decoded`, request и process output не включают
текст context/результата в Debug. Ошибки содержат тип, номер строки и статическую
причину; provider stderr, произвольный JSON и ошибки serde не выводятся в logs.

## Ограничения

| Часть | Максимум |
| --- | ---: |
| Полный stdin с JSON escaping и инструкциями | 1 MiB |
| Trusted task | 32 KiB |
| Trusted result schema | 64 KiB |
| JSONL stdout | 8 MiB |
| Один event | 2 MiB |
| Events | 4096 |
| Финальный текст ответа | 1 MiB |
| Stderr / timeout по умолчанию | 256 KiB / 300 s |

Это лимиты протокола одного запуска; streaming import больших архивов их не
наследует. Larger context/chunk orchestration нельзя заменять silent truncation.
Жизненный цикл процесса и ограничения ОС описаны в [runner](runner.md).

## Проверки

```sh
bash scripts/check.sh
cargo test -p tgsum-runner --all-features --locked --test codex -- --ignored --nocapture
```

Обычные тесты проверяют request scope/redaction, literal metacharacters,
cancel/stale revision, UTF-8, JSON/type/evidence, lifecycle и пределы вывода.
Два `ignored` теста требуют того же Linux x86_64/bubblewrap 0.12.0 profile,
что и generic runner. Они запускают **tgsum-codex-fixture**, а не Codex:
проверяют argv/stdin/read-only schema, evidence resolution, malformed/truncated
output, tool event, nonzero exit, timeout и cancel после уже выданного ответа.
Пропуск этих тестов не считается проверкой backend.

## Что остаётся до первого cloud Run

1. Реализовать auth/inference transport с выбранной границей всего процесса,
   без копирования auth tokens или всего профиля Codex.
2. Проверить effective tools/config/hooks/MCP, filesystem/network/IPC и auth
   refresh на конкретной версии. Fake CLI этого не доказывает.
3. Связать reviewed bundle, фактического получателя/model, recipe validator и
   durable result/baseline lifecycle; подключить adapter к Review/Run UI.
4. Провести контролируемую пользователем квалификацию `tgsum-t8t.19`.

Пункты 1–3 остаются инженерной работой. Только пункт 4 требует реального входа
пользователя. `tgsum-hzm.7` и весь эпик этим протокольным этапом не закрываются.
