# Расширение локального secrets scanner

Проверено **2026-09-27** для `tgsum-af2.1`. Исследование использовало публичные
первичные документы и спецификации. Реальные ключи, аккаунты, пользовательские
профили и credential storage не читались; запросы для проверки токенов не
выполнялись. Документ дополняет
[исследование минимального scanner](minimum-secrets-scanner-2026-09-26.md).
Исходное реализованное покрытие описано в
[sanitization.md](../development/sanitization.md).

`High` ниже — уверенность в необходимости скрыть фрагмент по локальной политике
TGSUM, а не утверждение, что credential настоящий, активен или даёт определённые
права. Рекомендации в этом документе не являются доказательством реализации.

## Решение для ближайшего среза

Расширить scanner через узнаваемые prefixes, явные credential headers/assignments,
PGP private armor, чувствительные query values и cookies. Сохранять целиком
значение: quoted password, составной rotating token и multiline secret не должны
оставлять нескрытый хвост. Entropy использовать только для review кандидатов рядом
с чувствительным именем; произвольные хеши, IDs и случайные строки не считать
секретами автоматически.

Минимум уже покрывает GitHub, Bearer/Basic, PEM/OpenSSH, URL userinfo и JWT/JWE.
Основной прирост здесь — другие провайдеры и корректные границы контейнеров.

## 1. Факты о провайдерах и пределы подтверждения

| Источник | Что подтверждено | Что из этого следует для scanner |
| --- | --- | --- |
| GitHub | Документированы `ghp_`, `github_pat_`, `gho_`, `ghu_`, `ghs_`, `ghr_`; новый `ghs_` может включать точки и быть существенно длиннее старых токенов. [Token formats](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-authentication-to-github#githubs-token-formats) | Сохранить существующий detector и regression на длинный dotted `ghs_`; не вводить общий предел 40 символов. |
| GitLab | Общая таблица перечисляет `glpat-`, `gloas-`, `gldt-`, `glrt-`, `glrtr-`, `glcbt-`, `glptt-`, `glft-`, `glimt-`, `glagent-`, `glwt-`, `glsoat-`, `glffct-`; session cookie — `_gitlab_session`. PAT prefix может настраиваться. [Token overview](https://docs.gitlab.com/security/tokens/) | Это хороший перечень точных prefixes. Он не покрывает изменённый администратором prefix и не задаёт универсальные длину/алфавит суффикса. |
| Slack | Документированы `xoxb-`, `xoxp-`, `xapp-`, `xwfp-`. Старые user tokens могут иметь короткую финальную secret-часть. [Token types](https://docs.slack.dev/authentication/tokens/) | Проверять весь токен, не требовать только современную длину финальной части. Другие `xox*` без источника не считать подтверждёнными форматами. |
| Slack rotation | Access tokens получают `xoxe.` перед прежним prefix, например `xoxe.xoxb-`/`xoxe.xoxp-`; refresh token показан как `xoxe-1-…`. [Token rotation](https://docs.slack.dev/authentication/using-token-rotation/) | Detector обязан поглотить внешний `xoxe.` вместе с вложенным токеном. Отдельное правило нужно для `xoxe-`. |
| OpenAI | Cookbook показывает `sk-proj-…`; reference выдачи service account key описывает значение как string и показывает пример `sk-…`. [Cookbook](https://developers.openai.com/cookbook/examples/agents_sdk/session_memory), [API reference](https://developers.openai.com/api/reference/resources/admin/subresources/organization/subresources/projects/subresources/service_accounts/methods/create) | Пример подтверждает наблюдаемый prefix, но не полную грамматику всех ключей. `sk-proj-` с длинным суффиксом — полезная локальная эвристика. Просто `sk-` без контекста требует review. В проверенных страницах не найдена нормативная спецификация `sk-svcacct-` или общего точного размера. |
| Anthropic | Таблица типов различает `sk-ant-api03-`, `sk-ant-api01-`, `sk-ant-admin01-`; отдельный routine API использует `sk-ant-oat01-`. [Key types](https://platform.claude.com/docs/en/manage-claude/compliance-api-access), [Routine authentication](https://platform.claude.com/docs/en/api/claude-code/routines-fire) | Добавить точные prefixes; общую длину не считать заданной. Не выводить разрешения конкретного кандидата из его внешнего вида. |
| Google/Gemini | Официальная страница описывает standard/auth keys; новые AI Studio keys с 2026-05-28 создаются как auth keys. Подтверждены `GEMINI_API_KEY`, `GOOGLE_API_KEY` и header `x-goog-api-key`. [Gemini API keys](https://ai.google.dev/gemini-api/docs/api-key) | Для актуального покрытия обязательны именованные поля/header независимо от prefix. `AIza`-only detector не обеспечивает покрытие новых Google keys. Проверенная страница не задаёт полную строковую грамматику auth keys. |
| Google/Firebase | Firebase описывает свои API keys как несекретные элементы конфигурации. [Firebase checklist](https://firebase.google.com/support/guides/security-checklist#cloud_functions_safety) | Голый Google-like key без service context — review. Нельзя по одной строке узнать ограничения ключа или утверждать, что это именно Gemini secret. |
| AWS | Credential состоит из access key ID и secret access key; для обычной подписи нужны обе части. `AKIA` обозначает access key, `ASIA` — temporary access key ID. [IAM access keys](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_access-keys.html), [ID prefixes](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_identifiers.html#identifiers-unique-ids) | `AKIA…`/`ASIA…` отдельно — идентификатор потенциального credential и кандидат для review. Secret/session token в явном поле скрывать независимо от длины. Не называть любой 40-символьный Base64-like текст AWS secret. |
| Telegram | Bot API показывает `число:secret`, путь `/bot<token>/METHOD_NAME` и download path `/file/bot<token>/<file_path>` на `api.telegram.org`; полная грамматика и фиксированные длины не опубликованы в этих разделах. [Authorization / requests](https://core.telegram.org/bots/api#authorizing-your-bot), [File](https://core.telegram.org/bots/api#file) | Точный host/path context и `TELEGRAM_BOT_TOKEN` дают уверенный контекст. Standalone `digits:long-suffix` — эвристический review; сходный текст может быть прикладным ID. |

Для GitLab отдельно подтверждён header `PRIVATE-TOKEN`:
[PAT authentication](https://docs.gitlab.com/user/profile/personal_access_tokens/#use-rest-api).
Anthropic подтверждает `Authorization: Bearer` и сохраняет поддержку `x-api-key`:
[Authentication](https://platform.claude.com/docs/en/manage-claude/authentication).
Эти headers позволяют скрыть новые/короткие формы без угадывания их prefix.

## 2. Credential containers

### SSH, PEM и PGP

Существующий набор PEM/OpenSSH остаётся из предыдущего исследования. Дополнение:
RFC 9580 §6.2 определяет `BEGIN PGP PRIVATE KEY BLOCK` для private keys и отдельно
различает public-key, message и signature armor. Скрывать весь private armor,
включая дополнительные armor headers и тело; не только строку BEGIN. Парольная
защита внутри контейнера не превращает его в public key.
[RFC 9580 §6.2](https://www.rfc-editor.org/rfc/rfc9580.html#section-6.2).

Решение TGSUM: при отсутствующем совпадающем END скрыть остаток поля и сохранить
признак incomplete. Для `PGP PUBLIC KEY BLOCK`, `PGP MESSAGE`, `PGP SIGNATURE`,
`PUBLIC KEY`, `CERTIFICATE` само наличие границ не создаёт private-key finding.
Внутри такого текста всё равно могут независимо встретиться другие credentials.

### `.env`, JSON-подобные поля и connection strings

Единого стандарта `.env` нет; Node публикует свой диалект: quoted values могут
занимать несколько строк, `#` вне кавычек начинает комментарий, внутри кавычек
является частью значения. Даже `true` и `0` после разбора являются строками.
[Node environment variables](https://nodejs.org/api/environment_variables.html#dotenv).

PostgreSQL libpq допускает `keyword=value` и URI. В первом формате пробелы
разделяют assignments, single quotes сохраняют пробелы, а `\'` и `\\`
экранируют quote/backslash. Поэтому `password='two words'` — один credential.
[PostgreSQL 18 connection strings](https://www.postgresql.org/docs/18/libpq-connect.html#LIBPQ-CONNSTRING).

SQL Server connection strings разделяют пары `;`, однако quoted values могут
содержать `;`; внутренний delimiter quote может экранироваться удвоением.
Правило «остановиться на первом `;` или quote» оставит часть пароля.
[SqlConnection.ConnectionString](https://learn.microsoft.com/dotnet/api/microsoft.data.sqlclient.sqlconnection.connectionstring).

Решения TGSUM:

- Явные names `password`, `passwd`, `pwd`, `api_key`, `access_token`,
  `refresh_token`, `client_secret`, `secret_access_key`, `session_token`,
  `session_id` и их согласованные provider prefixes скрывать как high.
- Обрабатывать escaped quotes и doubled quotes в заявленном наборе контейнеров;
  multiline quoted credential читать до закрытия quote в пределах бюджета поля.
  Оборванный quoted credential нельзя считать полностью очищенным после первой
  строки: скрыть остаток соответствующего контейнера/поля либо явно прервать
  подготовку. Выбор консервативного поведения закрепить тестом.
- Полные boolean/null-подобные значения и `${VARIABLE}` можно пропускать только
  как явную политику placeholders. Это снижает шум, но может пропустить реальный
  пароль `true` или буквальное `${VARIABLE}`. Не описывать такое исключение как
  доказательство отсутствия credential.
- Само имя `DSN`, `DATABASE_URL` или `connection_string` не доказывает наличие
  пароля. В распознанном контейнере скрывать credential components; соединение
  может использовать интегрированную аутентификацию.
  [ADO.NET connection syntax](https://learn.microsoft.com/en-us/dotnet/framework/data/adonet/connection-string-syntax).
- CamelCase (`apiKey`, `clientSecret`, `accessToken`) и prefix/suffix names
  потребуют отдельного согласованного словаря. Совпадение подстроки `key` внутри
  произвольного идентификатора не даёт основания скрыть значение.

### URL userinfo, query и signed URLs

Правило userinfo остаётся по RFC 3986: сначала определить authority/path/query,
затем чувствительные компоненты; percent-encoded `@` не является разделителем.
[RFC 3986 §3.2.1](https://www.rfc-editor.org/rfc/rfc3986.html#section-3.2.1).

Query — отдельный путь утечки. AWS presigned URL передаёт подпись через
`X-Amz-Signature`, а S3 документация описывает presigned URL как bearer capability.
Это не secret access key, однако полную подписанную ссылку можно использовать
до истечения разрешённого срока.
[AWS presigned URL overview](https://docs.aws.amazon.com/prescriptive-guidance/latest/presigned-url-best-practices/overview.html),
[S3 presigned URLs](https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-presigned-url.html).

Решение TGSUM: redaction для явно чувствительных query names (`access_token`,
`api_key`, `password`, `X-Amz-Signature`, `X-Amz-Security-Token`); generic `key`
без контекста — review. Находить каждый повтор параметра. Не разделять значение
по percent-encoded `&`/`=` и не считать username/password внутри URL path
userinfo. Имена параметров с percent-encoding проверять после ограниченного
однократного декодирования, заменять диапазон в исходных UTF-8 bytes. Полностью
закодированный URL и рекурсивные encoding layers требуют отдельного покрытия.

### Cookies и sessions

RFC 6265 различает request `Cookie` (последовательность cookie pairs) и response
`Set-Cookie` (одна cookie pair, затем attributes). Значение может содержать `=`;
для выделения пары используется первое `=`. `Expires`, `Path`, `Domain`
и другие attributes нельзя разбирать как дополнительные session
credentials. Cookies не обязаны содержать аутентификацию.
[RFC 6265 §§4.1.1, 4.2.1, 5.2](https://www.rfc-editor.org/rfc/rfc6265.html#section-4.1.1).

Решение TGSUM: high для session/auth cookie names из явного словаря, включая
подтверждённый `_gitlab_session` и эвристические `session`, `sid`, `sessionid`,
`session_id`, `JSESSIONID`, `PHPSESSID`, `connect.sid`, `auth`, `auth_token`,
`access_token`, `csrf`. Этот словарь — политика redaction, не нормативные имена
из RFC; CSRF token не следует называть login/session credential. Остальные
высокоэнтропийные cookie values в явном header — medium review; короткие
настройки вроде `theme=dark` можно пропустить для снижения шума. Такой выбор
может пропустить custom session cookie с коротким значением. При обработке Set-Cookie
сохранить attributes за первой pair. Для raw copied header не читать browser
cookie database, не воспроизводить сессию и не проверять cookie по сети.

## 3. Confidence, entropy и границы кандидатов

Все числовые пороги этого раздела — **предлагаемые эвристики TGSUM**, а не
спецификации перечисленных провайдеров. Перед выпуском их надо зафиксировать
в версии правил и corpus.

| Контекст | Предложенное действие |
| --- | --- |
| Явный credential header, sensitive assignment, private-key block, Telegram Bot API path | High → скрыть без entropy/minimum-length фильтра |
| Точный подтверждённый provider prefix + длинный непрерывный суффикс | High → скрыть целиком; разумный начальный порог 20 символов суффикса, с отдельными fixtures для более коротких подтверждённых форм |
| `sk-`, Google-like key, Telegram-like standalone, AWS access key ID, JWT/JWE | Medium → review; явный credential context повышает confidence |
| Значение рядом с неоднозначным `key`, `token`, `secret` | Medium, если контекст и форма подходят; generic имя не доказывает секретность |
| Случайная строка/хеш без sensitive context | Не создавать finding только из entropy |

Для дополнительного detector можно считать Shannon entropy по байтам ASCII
candidate только после нахождения ограниченного локального контекста: exact
label `key`/`token`/`secret`/`password`, затем separator и одно значение в той же
строке. Начальная эвристика: длина не менее 20, entropy не менее 3.5 bits/char;
это лишь отправная точка для synthetic positive/negative corpus. Не искать
любое «секретное слово» в радиусе абзаца: это создаёт случайные связи.

Не применять entropy как условие скрытия известного токена/короткого пароля.
Повторяющийся синтетический credential и низкоэнтропийный реальный пароль
всё равно требуют redaction в явном credential context. Хеши могут иметь
высокую entropy; fixture `sha256=<hex>` должен оставаться вне generic detector.

Границы prefixes должны исключать совпадение в середине обычного слова.
Суффикс читать целиком до допустимой текстовой границы; верхний лимит размера
поля не должен превращаться в обрезание finding с оставшимся хвостом. Не
объявлять выбранный regex полным validator. Примеры, истёкшие и фиктивные
ключи закономерно сработают; выяснять их действительность не задача scanner.

## 4. Синтетический corpus для реализации

Использовать явно сконструированные строки; не копировать чьи-либо credentials.
Fixtures должны проверять точный оставшийся текст, confidence, ranges и
отсутствие исходного значения в отчёте.

| Группа | Positive / обязательный результат | Negative / граница |
| --- | --- | --- |
| Provider prefixes | Каждый подтверждённый prefix; Slack `xoxe.xoxb-…`, `xoxe.xoxp-…`, `xoxe-…`; длинный dotted GitHub; токен рядом с кириллицей | Короткое имя prefix; prefix в середине identifier; обычный длинный hash |
| Opaque fields | Незнакомая форма в `GEMINI_API_KEY`, `x-goog-api-key`, `PRIVATE-TOKEN`, `x-api-key`; короткий Bearer | `WWW-Authenticate`; комментарий с именем header без значения |
| AWS | Secret/session assignments скрыты; standalone `AKIA`/`ASIA` review | `AIDA`/`AROA` IAM IDs и UUID не являются secrets этого правила |
| Telegram | Bot API path скрывает целый token, сохраняя method; standalone review; явный bot-token assignment high | Обычные `number:identifier`, host с похожим именем, token-like text в URL path другого сервиса |
| Private blocks | PGP с armor headers; encrypted PEM; CRLF; несколько блоков; незакрытый последний блок | PGP public key/message/signature, SSH public key, certificate |
| Assignments/DSN | Multiline quoted password; backslash/doubled quotes; `;`, `#`, spaces и `=` внутри; несколько credentials подряд | Connection string без password; `key=primary`; placeholder policy проверяется отдельно |
| URL | Encoded userinfo; IPv6; query token с `%26`/`%3D`; повтор sensitive параметра; signed URL | Email; username-only authority; path с `user:pass@host`; `monkey=` не `key=` |
| Cookies | Session cookie с `=`/padding; две request pairs; Set-Cookie с Expires и attributes; high-entropy custom cookie — medium | `theme=dark` не становится high; attributes не становятся credential findings |
| Entropy | Высокоэнтропийный candidate у sensitive label — medium; low entropy explicit password — high | Такой же candidate у `sha256`/`request_id` и без label — без entropy finding |
| Общие инварианты | Перекрытие private block/provider/JWT даёт одну полную замену; UTF-8 ranges; idempotence; scanning до Markdown splitting | Overflow бюджета завершает подготовку ошибкой, не отдаёт частично проверенный bundle |

## 5. Что эта проверка не подтверждает

Не заявлять полный DLP, полную анонимизацию или отсутствие секретов при нуле
findings. Неподписанные пароли, новые prefixes, обфускация, escape layers,
разделённые между сообщениями credentials и бинарные вложения требуют
дополнительного покрытия. Scanner работает с переданным декодированным текстом;
выбор источников и чтение файлов остаются обязанностью внешнего pipeline.

Завершение `af2.1` должно ссылаться на реально выполненный corpus и regression
существующего минимума. Непроверенный формат явно остаётся ограничением;
изменение таблицы или этой записки само по себе поддержку не добавляет.
