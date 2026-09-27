# Codex в desktop: Review, Run и восстановление

Срез `tgsum-hzm.7`, 2026-09-27. Код: `src-tauri/src/analysis.rs`,
`src-tauri/ui/analysis.js`, `runner/src/codex/runtime.rs`.

## Пользовательский путь

1. Открыть Project, выбрать источники и подготовить контекст. Решить находки
   privacy review; неподготовленный контекст не запускается.
2. Выбрать один из [шести recipes](recipes.md), ID доступной модели, получателя
   OpenAI (`chatgpt.com` или `api.openai.com`), исполняемый файл Codex и явно
   указать его `auth.json`. Настройки этой формы действуют в текущем окне.
3. **Подготовить запуск**: version probe без авторизации/сети, подготовка
   изолированного runtime и сохранение immutable request. Инференса ещё нет.
4. Review показывает recipe/version, модель, получателя, executable, выбранный
   auth-файл и доступный агенту контекст. **Run** запускает ровно этот request.
   Изменение формы, контекста или Project отменяет подготовленный request.
5. Результат показывается как текст с evidence ID/revision. Только проверенный
   результат обновляет точку успешного анализа. Отмена/ошибка её не двигают.

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

Текущий исполняемый профиль: Linux x86_64, bubblewrap **0.12.0**, static
Codex **0.155.1**, `tgsum-codex-relay`. На других платформах доступен Export only.
Наличие relay в каталоге не является квалификацией: версия проверяется при Prepare.

Relay сначала ищется рядом с приложением, затем в абсолютных каталогах PATH.
Codex обнаруживается по metadata, без запуска или поиска auth. Выбранный auth
остаётся read-only capability; TGSUM не копирует профиль пользователя.
Найденный кандидат может быть shell/npm wrapper; его probe будет отклонён.
В таком случае пользователь выбирает нативный executable через file picker.

Runtime manifest выбирает root-owned regular libc/libgcc/loader и публичный
system CA bundle, запрещает group/world-writable и слишком большие файлы.
Не запускает `ldd`, shell или npm wrapper. Кодекс и relay проходят существующую
namespace qualification. Неизвестная версия или неполная изоляция оставляет
доступным сохранение контекста, без fallback на обычный процесс.

Для разработки:

```sh
cargo build -p tgsum-runner --bin tgsum-codex-relay --locked
cargo run -p tgsum
```

Для Tauri Linux bundle:

```sh
bash scripts/build-relay.sh x86_64-unknown-linux-gnu
cargo tauri build --config src-tauri/tauri.linux-relay.conf.json
```

Скрипт создаёт sidecar с target suffix; override включает `bundle.externalBin`.
Отдельный override сохраняет обычный `cargo check/run` возможным без заранее
собранного sidecar. Arch package устанавливает оба бинарника в `/usr/bin`.
При `cargo install --path src-tauri` helper ставится отдельно:
`cargo install --path runner --bin tgsum-codex-relay --locked`.
Release workflow собирает Linux sidecar и публикует runner перед desktop crate.
Эти изменения не запускают публикацию или релиз.

Первичный источник упаковки: [Tauri external binaries](https://v2.tauri.app/develop/sidecar/),
прочитан 2026-09-27: `externalBin`, путь относительно `tauri.conf.json`, target
suffix. Решение TGSUM: доставка через sidecar manifest, выполнение через наш
изолированный Rust runner; shell plugin приложению не добавлен.

## Проверки и границы

- IPC regression tests: immutable single-use Run, смена Review/Project,
  fail/cancel без baseline, результат после нового экземпляра приложения,
  явное восстановление без повторного запуска.
- Core: bounded listing, повреждённая запись, symlink rejection, отсутствие
  изменения Project при чтении истории.
- `installed_runtime_manifest_qualifies_without_accounts`: установленный static
  Codex проверяется только через изолированный version probe; иной executable
  отвергается. Нет реальных credentials, профиля или provider calls.
- Настоящий `cargo run -p tgsum --features analysis-fixtures`: локальный backend
  включается только debug-сборкой и `TGSUM_ANALYSIS_FIXTURE=1`. Модели стенда:
  `fixture-success`, `fixture-failure`, `fixture-wait`, `fixture-saved` (artifact
  до baseline commit). Release-сборка не содержит этот backend.
- В окне Tauri проверены Review/Run, изменение формы, HTML в ответе как текст,
  ошибка, отмена, история после перезапуска и принятие uncommitted result.
- Default-сборка также проверена до Run: кандидат wrapper отклонён, явный
  static Codex проходит version Prepare, Review показывает `api.openai.com`
  и выбранный пустой synthetic auth-файл. Run не выполнялся.

Успешные synthetic проверки не подтверждают настоящие API-key/ChatGPT аккаунты,
enterprise requirements, agent identity, keyring или полный installer roundtrip
на каждой ОС. Реальная квалификация остаётся `tgsum-t8t.19` под контролем
пользователя. [Сетевая граница](inference-egress.md) и
[исследование auth](../research/codex-auth-boundary-2026-09-27.md) сохраняют
свои ограничения. Ни один messenger connector не повышает support status
из-за появления кнопки Run.
