# Журнал анализа и commit результата

Код: `core::analysis`, `runner::codex::AnalysisJob`, срез `tgsum-hzm.7`.
Это реализация backend со [встроенными recipes](recipes.md). Экран Review/Run,
упаковка runtime и реальная квалификация аккаунтов ещё не завершены.

## Порядок операций

1. Host готовит и показывает bundle; выбирает доверенный recipe, модель,
   adapter, version/isolation profile и фактического получателя.
2. `ProjectStore::begin_analysis` проверяет текущую revision, privacy findings,
   hashes всех public files, manifest и evidence index. Создаёт приватный
   `RunTicket`, не меняя revision: `PreparedContext` остаётся действительным.
3. `AnalysisJob::new` сопоставляет ticket с конкретным prepared context,
   bundle ID, revision, моделью, CLI version и поддержанным destination.
   `run_analysis` ещё раз проверяет профиль, revision и отсутствие terminal record
   непосредственно перед запуском. Данные чата не задают metadata или argv.
4. Runner завершает процесс и gateway. `NetworkOutput::decode` требует exit 0,
   завершённый transport без ошибок gateway, затем проверяет JSONL, тип ответа
   и обязательный recipe validator. Cancel/timeout не превращаются в успех.
5. `save_analysis_result` проверяет все evidence ID/revision по **тому же bundle**,
   сериализует результат с пределом 1 MiB и атомарно публикует `completion.json`.
   Он содержит checksum результата и hash request, включая receiver/recipe/model.
6. `commit_analysis` повторно проверяет artifact и bundle. Одна новая revision
   Project одновременно записывает result reference и baselines всех inputs.
   Используется compare-and-publish по исходной revision; другая запись не затирается.

`AnalysisSpec` — данные доверенного host, которые он обязан показать пользователю.
Строка destination в core является метаданными, а не сетевой настройкой.
Codex adapter принимает только `api.openai.com` или `chatgpt.com` и сопоставляет
их с typed gateway target. Recipe ID/version записываются до запуска; соответствие
task/schema конкретному recipe обеспечивает `RecipeRequest`, а `run_recipe`
применяет связанный validator. Core не
оценивает фактическую правдивость вывода и не заменяет этот validator.

## Хранение и восстановление

```text
project-.../
  analyses/run-.../
    request.json       # точный bundle/revision, inputs, recipe/model/receiver
    completion.json    # validated result ИЛИ static failure ИЛИ cancelled
  revisions/000...json # success + result hash + baselines одним commit
```

Unix directories имеют `0700`, файлы `0600`. Публикация через synced temporary
file и `persist_noclobber`; parent directories синхронизируются на Unix.
File/directory symlinks отклоняются. Записи ограничены 4 MiB, сам JSON result —
1 MiB, references — максимум 512 уникальных пар по 128 bytes на компонент.
Ошибки не включают текст результата или provider diagnostics. Debug результата
показывает только metadata/counts. Результат остаётся untrusted text: будущий UI
должен выводить его текстом либо безопасно отрисованным Markdown.

Состояние без `completion.json` — pending/interrupted, **не успех**. Фоновый
повторный запуск из такого состояния не производится. Caller должен сериализовать
запуски одного ticket; проверка pending сама по себе не является процессной блокировкой.

`Validated` означает сохранённый проверенный ответ. Это ещё не commit baseline.
При сбое после записи ответа `resume_analysis` + явный `commit_analysis` позволяют
закончить тот же commit. Повтор после уже опубликованной revision идемпотентен,
даже если затем были другие изменения Project: проверяется исходная историческая
revision, текущая не перезаписывается.

При изменении Project во время выполнения ответ может сохраниться как historical
artifact, но baseline не передвигается. При отмене до commit baseline также не
меняется; если validated artifact уже опубликован, он остаётся без committed
revision. Его нельзя автоматически считать успешным или автоматически возобновлять.
Cancel после точки commit не откатывает уже опубликованную revision.

Checksums обнаруживают повреждение результата и provenance до/после commit;
это не защита от владельца host, намеренно переписавшего всё private storage.
Отдельный тест аварийного отключения питания не проводился.

## Evidence и coverage

References перечисляет trusted validator, выводя их из typed answer. Core не
принимает отдельный непроверенный список модели за доказательство. Чужой ID,
чужая revision и дубликаты отклоняются; index читается потоком один раз без
повторного разбора больших snapshots для каждого reference.

Пустой список допустим для recipe без утверждений, например `actions: []`.
Validator конкретного recipe должен потребовать evidence для каждого вывода;
core не заставляет модель придумать действие или citation ради непустого списка.
Coverage/gaps/delta mode/message counts записываются из bundle manifest, не из
заявления модели о полноте истории.

## Совместимость

Project schema **3** добавляет `analysis_run.result`. Schema 1/2 читаются без
перезаписи; следующий update создаёт v3. Старый baseline-only run сохраняется с
`result: None`: миграция не приписывает ему файл или проверку результата.
Legacy host operations `ProjectChange::BeginAnalysis/FinishAnalysis` остаются
для прежнего scope API, описанного в [scope](scope.md). Новый adapter использует
только managed pipeline выше; legacy primitive сам по себе не валидирует результат.

## Проверено

- Core: restart между artifact и baseline, повторный commit после новых revisions,
  параллельные результаты, stale scope, removed source, rejected/oversize output,
  ошибки/cancel, payload/request corruption и symlink substitution, private modes.
- Пустые findings сохраняют coverage, не требуют выдуманной evidence.
- v1/v2 migration сохраняет исходные bytes и не создаёт result reference.
- Codex job отклоняет другой bundle/model/version/profile/receiver до launch;
  terminal run не запускается повторно. Gateway rejection не скрывается exit 0.
- Installed CLI 0.155.1: local HTTPS success без auth и с synthetic API key теперь
  доходят до durable result и baseline. Wrong CA, refresh denial, cancel/timeout,
  HTTP 401 и auth-mode/receiver mismatch сохраняют failure/cancel без baseline.

В HTTP 401 тесте сообщение ошибки остаётся только в ограниченных raw process
buffers, не в durable record/Display. Adapter пока сохраняет общий `Agent` failure:
JSONL этой версии не даёт здесь надёжного typed HTTP status, строка ошибки не
используется как универсальный детектор истёкшей авторизации.

Synthetic ChatGPT file auth проверен на локальном HTTPS: success, expiry,
stale cache, 401 и mismatch receiver; см. [границы проверки](inference-egress.md).
Открыто: выдача понятного recovery в UI, Review/Run/result UI, runtime packaging. Реальная
авторизация — отдельно `tgsum-t8t.19`, только под контролем пользователя. Эти
локальные tests не доказывают production support, WSS или другие ОС.
