# Codex и Claude Code в desktop: Review, Run и восстановление

Срезы `tgsum-hzm.7` и `tgsum-hzm.8`, 2026-09-27. Код: `src-tauri/src/analysis.rs`,
`src-tauri/ui/analysis.js`, `runner/src/{codex,claude}/runtime.rs`.

## Пользовательский путь

Projects и повторная помощь описаны в [onboarding-контракте](onboarding.md).
После проверки контекста пользователь выбирает Export only либо агента.

1. Открыть Project, выбрать источники и подготовить контекст. Решить находки
   privacy review; неподготовленный контекст не запускается.
2. Выбрать Codex или Claude Code, один из [шести recipes](recipes.md), точный ID
   доступной модели, executable и auth-файл. Codex: `auth.json`, получатель
   `chatgpt.com` или `api.openai.com`; Claude: `.credentials.json`, единственный
   получатель `api.anthropic.com`. Скрытый файл можно выбрать через «Показывать
   скрытые файлы» native dialog (в GTK — Ctrl+H). Настройки действуют в текущем окне.
3. **Подготовить запуск**: version probe без авторизации/сети, подготовка
   изолированного runtime и сохранение immutable request. Инференса ещё нет.
4. Review показывает recipe/version, модель, получателя, executable, выбранный
   auth-файл и доступный агенту контекст. **Run** запускает ровно этот request.
   Изменение формы, контекста или Project отменяет подготовленный request.
5. Результат показывается как текст с evidence ID/revision. Только проверенный
   результат обновляет точку успешного анализа. Отмена/ошибка её не двигают.

Смена агента сбрасывает auth/model, выбирает его executable candidate и получателя,
отменяет Review. Несовместимый получатель отклоняется на backend до runtime/auth
доступа. Имя файла в picker — подсказка, выбранный путь проверяет backend.

Контроллер хранит одну capability `Ready` либо один `Working` job. Frontend
передаёт в Run только ID; аргументы/credentials заменить в этой команде нельзя.
Повторный Run того же ID отвергается. Изменение Project во время анализа
отклоняется; пользователь может отменить job общей кнопкой прогресса.
Другие экземпляры приложения защищены revision/CAS при сохранении baseline,
а не общим межпроцессным execution lock.

## После перезапуска

Последние 20 записей читаются из локального журнала. Время изменения каталога
задаёт порядок списка, не доказывает успех. Повреждённая запись показывается
недоступной. Ни один сохранённый request автоматически не выполняется снова.

| Запись | Действие |
| --- | --- |
| Нет terminal result | Прерванный запуск; можно закрыть запись и подготовить новый |
| Failed / Cancelled | Прочитать статус; при необходимости подготовить новый запуск |
| Validated, baseline не сохранён, revision совпадает | Явно принять результат; повторно проверить recipe/evidence и выполнить commit без inference |
| Validated, Project уже изменён | Читать результат; для текущего Project нужен новый анализ |
| Succeeded | Читать сохранённый результат; повторное принятие идемпотентно |

Журнал/границы атомарности описаны в [analyses.md](analyses.md).
Сырые stdout/stderr провайдера не передаются в UI или журнал ошибок.

## Linux runtime и упаковка

Текущие исполняемые профили: Linux x86_64, bubblewrap **0.12.0**.

| Агент | Native executable | Helper |
| --- | --- | --- |
| Codex | **0.155.1** | `tgsum-codex-relay` |
| Claude Code | **2.1.280** | `tgsum-claude-relay` |

На других платформах доступен Export only.
Наличие relay в каталоге не является квалификацией: версия проверяется при Prepare.

Relay сначала ищется рядом с приложением, затем в абсолютных каталогах PATH.
Агенты обнаруживаются по metadata, без запуска или поиска auth. Выбранный auth
остаётся read-only capability; TGSUM не копирует профиль пользователя.
Найденный кандидат может быть shell/npm wrapper; его probe будет отклонён.
В таком случае пользователь выбирает нативный executable через file picker.

Общий runtime manifest выбирает root-owned regular libc/libgcc/loader и публичный
system CA bundle, запрещает group/world-writable и слишком большие файлы.
Claude дополнительно получает librt/libpthread/libdl/libm. Manifest не запускает
`ldd`, shell или npm wrapper. Native executable и relay проходят существующую
namespace qualification. Неизвестная версия или неполная изоляция оставляет
доступным сохранение контекста, без fallback на обычный процесс.

Claude Prepare захватывает fixed Linux managed policy; при Run изменения требуют
новой подготовки. Корпус передаётся только после same-process settings preflight.
Профиль не разрешает refresh hosts; истёкший вход пользователь обновляет отдельно.
Ограничения policy/auth описаны в [контракте Claude](claude-adapter.md).

Для разработки:

```sh
cargo build -p tgsum-runner --bin tgsum-codex-relay --bin tgsum-claude-relay --locked
cargo run -p tgsum
```

Для Tauri Linux bundle:

```sh
bash scripts/build-relay.sh x86_64-unknown-linux-gnu
cargo tauri build --config src-tauri/tauri.linux-relay.conf.json
```

Скрипт создаёт два sidecar с target suffix; override включает `bundle.externalBin`.
Отдельный override сохраняет обычный `cargo check/run` возможным без заранее
собранных sidecar. Arch package устанавливает приложение и оба helper в `/usr/bin`.
При `cargo install --path src-tauri` helper ставится отдельно:
`cargo install --path runner --bin tgsum-codex-relay --bin tgsum-claude-relay --locked`.
Release workflow собирает Linux sidecar и публикует runner перед desktop crate.
Эти изменения не запускают публикацию или релиз.

Первичный источник упаковки: [Tauri external binaries](https://v2.tauri.app/develop/sidecar/),
прочитан 2026-09-27: `externalBin`, путь относительно `tauri.conf.json`, target
suffix. Решение TGSUM: доставка через sidecar manifest, выполнение через наш
изолированный Rust runner; shell plugin приложению не добавлен.

## Проверки и границы

- IPC regression tests для обоих агентов: immutable single-use Run, смена Review/Project,
  fail/cancel без baseline, результат после нового экземпляра приложения,
  явное восстановление без повторного запуска. Отдельный default-backend test
  отвергает cross-provider receiver до executable/auth/policy доступа.
- Core: bounded listing, повреждённая запись, symlink rejection, отсутствие
  изменения Project при чтении истории.
- Native manifest tests Codex и Claude: установленный executable проверяется
  только через изолированный version probe; иной executable отвергается.
  Claude использует synthetic policy. Нет реальных credentials, профиля или provider calls.
- Настоящий `cargo run -p tgsum --features analysis-fixtures`: локальный backend
  включается только debug-сборкой и `TGSUM_ANALYSIS_FIXTURE=1`. Модели стенда:
  `fixture-success`, `fixture-failure`, `fixture-wait`, `fixture-saved` (artifact
  до baseline commit). Release-сборка не содержит этот backend.
- В окне Tauri проверены Review/Run, изменение формы, HTML в ответе как текст,
  ошибка, отмена, история после перезапуска и принятие uncommitted result.
- Default-сборка также проверена до Run: кандидат wrapper отклонён, явный
  static Codex проходит version Prepare, Review показывает `api.openai.com`
  и выбранный пустой synthetic auth-файл. Run не выполнялся.
- Default Claude UI проверен до Prepare/Run: fixed Anthropic receiver, native
  picker скрытого synthetic auth-файла, сброс auth/model при смене агента;
  после перезапуска pending Run не повторяется и закрывается явно.
- Финальный Rust gate этого среза: 188 passed, 26 ignored, clippy/doctests проходят.
  Отдельно выполнены 15 Claude process tests (включая обычно ignored), два native
  manifest tests и 5 IPC tests. Обе sidecar debug-сборки и ShellCheck проходят.

Успешные synthetic проверки не подтверждают настоящие OpenAI/Anthropic аккаунты,
enterprise requirements, agent identity, keyring или полный installer roundtrip
на каждой ОС. Реальная квалификация остаётся `tgsum-t8t.19` под контролем
пользователя. [Сетевая граница](inference-egress.md) и
[исследование auth](../research/codex-auth-boundary-2026-09-27.md) сохраняют
свои ограничения. Ни один messenger connector не повышает support status
из-за появления кнопки Run.
