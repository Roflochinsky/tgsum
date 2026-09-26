# Встроенные recipes v1

Код: `core::recipe`, `runner::codex::RecipeRequest`; задача `tgsum-hzm.9`.
Каталог, инструкции, JSON Schema и validator скомпилированы в приложение.
Текст переписки не может создать recipe или изменить его версию, tools либо
получателя. UI выбора и отображения результата остаётся в `hzm.7`/`hzm.10`.

| ID | Разделы результата |
| --- | --- |
| `summary` | overview, topics, open_questions |
| `retro` | worked, failed, lessons, next_steps |
| `decisions` | decisions, reversals, open_questions |
| `actions` | unresolved; действия в actions |
| `incident` | timeline, symptoms, hypotheses, actions_taken, resolution |
| `handover` | context, people, systems, decisions, open_questions |

Каждый результат содержит `recipe`, `version: 1`, упорядоченные `sections` и
`actions`. Разделы обязательны; пустые массивы допустимы, когда выводов нет.
Claim — `{text, evidence: [{id, revision}]}`. У каждого утверждения есть
непустая ссылка на точную версию сообщения. Повтор ссылки между утверждениями
допустим; итоговый набор для журнала дедуплицируется. Дубликат внутри одного
claim, чужой ID или revision отклоняются.

## Owner и deadline

У action обязательны поля `task`, `owner`, `deadline`. Неизвестное значение
записывается как явный `null`. Известное — `{value, quote, evidence}`: значение
должно дословно входить в цитату, а цитата — в **sanitized body** именно указанного
сообщения. Имя отправителя и timestamp сами по себе не назначают owner/deadline.
Относительный срок сохраняется исходным текстом, например `next Friday`.

Проверка устанавливает наличие цитаты, а не смысловую истинность интерпретации:
упоминание имени ещё не доказывает назначение исполнителя. Все выводы остаются
доступны для проверки человеком по evidence. Полнота истории, gaps, число
сообщений и delta scope берутся из `RunRequest.coverage`; модель не может
переопределить их полем собственного ответа.

## Подключение к runner

1. `Recipe::from_version(id, version)` принимает только известные пары; `ALL`
   и `title()` доступны для селектора. `task()` и `schema()` формирует core.
2. `RecipeRequest::prepare(context, model, recipe, cancel)` читает только
   подготовленный public bundle, строит lookup evidence и фиксирует request.
3. Host создаёт reviewed ticket с теми же recipe ID/version, model и receiver.
4. `RecipeRequest::job(ticket)` проверяет recipe ID/version до launch.
   `CodexNetworkRunner::run_recipe` использует соответствующий validator и
   [managed result lifecycle](analyses.md). Общий `run_analysis` остаётся
   низкоуровневым API для доверенного кода других adapters.
5. Validator проверяет строгий тип, разделы, ограничения, citations и quotes.
   Журнал повторно проверяет ссылки по private index исходного bundle перед
   сохранением и commit baseline. UI должен безопасно отображать текст результата.

`RecipeEvidence::from_markdown` предназначен только для частей, полученных через
`ProjectStore::export_bundle`/`PreparedContext`. Формат bundle цитирует каждую
строку исходного текста с `> `, включая поддельные заголовки `## Evidence`.
Lookup учитывает только настоящие заголовки и message body; raw snapshots,
sender metadata, attachments и credential stores он не читает.

Ограничения: 1 MiB суммарного Markdown, 128 claims/actions на результат,
4096 UTF-8 bytes на текстовое поле, 32 ссылки в claim и 512 уникальных ссылок.
Schema ограничивает длину строк в символах, validator дополнительно проверяет
bytes. Unknown fields отклоняются во всех типах результата, включая EvidenceRef.
Данные не обрезаются молча; превышение лимита означает неуспешную проверку.

## Проверки и просмотр каталога

```sh
cargo test -p tgsum-core --locked --test recipe
cargo test -p tgsum-runner --all-features --locked --test codex
cargo run -p tgsum-core --locked --example recipes
# Linux с квалифицированным bwrap; только synthetic executable:
cargo test -p tgsum-runner --all-features --locked --test codex \
  process::six_compiled_recipes -- --ignored --nocapture
```

Пример печатает шесть определений с инструкциями, schema и пустым допустимым
результатом без аккаунта или сети. Schema проверены Draft 2020-12 validator:
12 допустимых и 30 недопустимых примеров. Core suite проводит все шесть recipes
через настоящий public bundle, validation, сохранение и baseline commit,
сохраняя trusted coverage. Runner suite проверяет control/data separation,
binding Review и шесть ответов из изолированного fake CLI.
