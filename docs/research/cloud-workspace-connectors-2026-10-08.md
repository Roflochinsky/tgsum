# Облачная обработка выбранной переписки

Проверено 2026-10-08; issue `tgsum-plb`. Область: личный аккаунт,
Telegram Desktop 7.2.5-1 на Arch, закрытый репозиторий пользователя,
отдельная managed ChatGPT OAuth-сессия Codex 0.155.1 и internal connection Notion.
Содержимое остаётся в локальном проекте, выбранном частном GitHub и Notion.
TGSUM не получает сообщения на собственный сервер. Автоматическая передача —
отдельная сохранённая настройка; существующие проекты её не получают миграцией.

## Первичные источники и решения

- [OpenAI: account auth in CI/CD](https://learn.chatgpt.com/docs/auth/ci-cd-auth):
  private trusted automation допускает managed account auth, одну очередь на
  сессию и сохранение обновлённого cache. Штатный Codex обновляет токены;
  самостоятельного refresh endpoint в приложении нет. API-key fallback запрещён
  выбранным пользователем способом. Отдельный CODEX_HOME не содержит основной
  рабочей сессии. GitHub-hosted job проверяет актуальность полученного secret
  после получения очереди и сохраняет auth.json даже при ошибке анализа.
- [GitHub Secrets REST](https://docs.github.com/en/rest/actions/secrets#create-or-update-a-repository-secret):
  запись repository secret требует Secrets write. [Workflow permissions](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#permissions)
  не предоставляют этому endpoint права через обычный GITHUB_TOKEN.
  Выбран отдельный fine-grained token только к репозиторию данных, Secrets
  read/write. `gh secret set` шифрует содержимое для GitHub, получает его через
  stdin. Token и cache не попадают в git, вывод, результаты, artifacts или prompt.
  [Secrets reference](https://docs.github.com/en/actions/reference/security/secrets)
  уточняет: repository secrets читаются при постановке workflow в очередь.
  Поэтому concurrency недостаточно: если secret обновлён не раньше created_at
  текущего run, этот job не использует OAuth вообще, оставляет checkpoint
  прежним и ждёт следующего свежего запуска. Точность timestamp до секунды
  обрабатывается консервативным сравнением `>=`. Перед writeback проверяется,
  что пользователь не заменил secret во время job. Это обходится без платного
  GitHub Environment и без дополнительных Environments permissions.
- [Notion internal connections](https://developers.notion.com/guides/get-started/internal-connections):
  read/insert/update content и доступ к конкретной личной странице; доступ
  наследуется её дочерними страницами. Соединение отделено от Codex.
- [Query data source](https://developers.notion.com/reference/query-a-data-source),
  [page update](https://developers.notion.com/reference/patch-page),
  [views](https://developers.notion.com/guides/data-apis/working-with-views):
  API `2026-03-11`, пагинация query, filters и PATCH views реализованы в API.
  После создания карточки ручные статус, участники и даты не отправляются в PATCH.
  План/ответственный/срок модели — отдельные поля предложений.
  API не даёт доказанного ключа идемпотентности создания: до POST нужен durable
  intent. После неоднозначного ответа отсутствие карточки в query не разрешает
  повторный POST; требуется сверка или ручное разрешение.

## Что проверять до включения

Сначала синтетические проверки scope/privacy, точных opaque ID/revisions,
всего диапазона входных commits, отдельных analysis/sync checkpoints,
последовательного владения OAuth и Notion outbox. Секреты не нужны этим тестам.
Затем установленное приложение, настоящий private hosted job и личная карточка.
Наличие секретов по имени не доказывает их содержимое, grant или подписку.
Не квалифицированы реальные quota/refresh/revoke до соответствующей проверки.
Обработка ограничивается явно заданной частотой и размером; ошибка квоты
останавливает попытку и ждёт следующего разрешённого запуска, без платного обхода.

Сбор из логов остаётся частичным: тему показываем только при известном ID,
неизвестная тема не угадывается по имени, отсутствие сообщения не означает удаление.
История Git хранит прошлые выбранные данные; очистка/retention не вводится.
Следующий review network/auth пути — 2026-11-07. Метаданные registry сами не
включают обработку. Текущий результат проверки — candidate, без live support claim.
