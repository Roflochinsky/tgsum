# Codex 0.155.1: auth capability и граница inference

Проверено **2026-09-27 MSK**, задача **tgsum-hzm.7**. Дополнение к
[контракту адаптера](codex-adapter-2026-09-26.md). Прочитаны сохранённый локальный
`exec --help`, runner, официальная документация и исходники тега
`openai/codex:rust-v0.155.1`. Реальные auth/config/profile, login/status,
аккаунты и inference не использовались. Offline mount реализован и проверен
на синтетическом auth; cloud transport остаётся проектированием.

## Рекомендуемый следующий срез

Реализовать явно выбранный **read-only auth-file mount только в существующем
offline profile**. TGSUM проверяет тип/владельца/границы пути и закрепляет файл,
не читает, не сериализует и не копирует его содержимое. Официальный Codex сам
читает этот файл в новом private `CODEX_HOME`; остальные файлы профиля остаются
недоступны. Version probe выполняется без auth. Проверка — синтетический
`auth.json` и установленный CLI с локальным canned provider.

Это полезный самостоятельный примитив; он **не включает сеть и cloud Run**.
Для следующего network profile потребуется отдельный транспорт с запретом
refresh authority. Read-only mount сам по себе не обеспечивает такой запрет.
Для полноценного автоматического refresh предпочтителен отдельный auth store,
которым с первого входа управляет Codex; это отдельный пользовательский login,
не бесшовное переиспользование текущего входа. Основания ниже.

## Что установлено по auth

| Вариант | Факт / предел применения |
| --- | --- |
| `file` | `auth.json` расположен в `CODEX_HOME`; `--ignore-user-config` сохраняет использование этого root для auth. Можно отдельно смонтировать файл, не весь профиль. |
| `keyring` | Требует доступного OS credential store. Новый private HOME не означает доступ к прежнему ключу; потребуется отдельная квалификация IPC и namespace. |
| `auto` | Может перейти с keyring на file. Для точного контракта mount выбирать явно `file`, без скрытого fallback. |
| `ephemeral` | In-memory store текущего процесса; **не импортирует** сохранённый вход из file/keyring. |

Режимы описаны в [Authentication](https://learn.chatgpt.com/docs/auth#credential-storage).
Ключ `cli_auth_credentials_store` существует в
[config 0.155.1](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/core/src/config/mod.rs).
В [storage 0.155.1](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/login/src/auth/storage.rs)
direct-keyring key зависит от SHA-256 canonical `CODEX_HOME`; альтернативный
Secrets backend также использует этот root. Один общий доступ к пользовательской
шине/credential service не доказывает ограничение единственной записью Codex.

### Read-only файл и refresh — разные разрешения

В 0.155.1 `FileAuthStorage::save` открывает файл через
`truncate(true).write(true).create(true)`, затем пишет JSON и flush.
**Это запись на месте, не atomic rename.** Возможность замены файла другой
версией/процессом всё равно нужно учитывать при mount.
[Versioned storage](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/login/src/auth/storage.rs).

`refresh_and_persist_chatgpt_token` сначала получает новый token от сервера,
затем сохраняет его. Поэтому разрешённый refresh при read-only auth может
ротировать серверный token и потерять новый token при ошибке записи.
Локальная неизменность файла не доказывает неизменность сессии на сервере.
Refresh semaphore находится в процессе; guarded reload уменьшает гонки, но
не является межпроцессной эксклюзивной блокировкой.
[Versioned auth manager](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/login/src/auth/manager.rs).

Официальный CI guide требует сохранять обновлённый cache и не делить один файл
между параллельными jobs/машинами. Поэтому writable mount текущего пользовательского
cache не выбран для автоматического запуска рядом с обычным Codex.
[Maintain account auth](https://learn.chatgpt.com/docs/auth/ci-cd-auth).

**Вывод:** для будущего режима чтения существующей сессии запрещать refresh
authority вне Codex. При expiry/401 возвращать понятный неуспешный run и действие
«обновить вход в Codex»; не менять режим mount и не открывать auth host автоматически.
Полное отсутствие попыток refresh этим способом не обещать: запрещённая попытка
может произойти. Пока сеть полностью изолирована, внешняя ротация невозможна.

### Можно ли выключить refresh только конфигурацией

Документированного флага для отключения refresh **managed ChatGPT auth** не
установлено. Versioned manager проверяет JWT expiry с окном 5 минут; если
expiry не разобран, использует `last_refresh` старше 8 дней. Есть refresh-on-401.
Поэтому свежая дата cache сама по себе недостаточна.
[Manager](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/login/src/auth/manager.rs),
[refresh regression tests](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/login/tests/suite/auth_refresh.rs).

Внешний `chatgptAuthTokens` не использует managed refresh authority; официальный
app-server требует, чтобы host передал access token и обслуживал refresh RPC.
Это не готовый broker существующей CLI-сессии. Изменять/пересобирать `auth.json`
для подмены его режима потребовало бы чтения и копирования secrets TGSUM.
[App-server auth](https://learn.chatgpt.com/docs/app-server),
[load/storage-mode implementation](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/login/src/auth/manager.rs).

## Известные endpoint defaults 0.155.1

Это исходные defaults, **не полная проверенная сетевая allowlist**. Реальные
права, модели, redirect и дополнительные обязательные запросы ещё не проверены.

| Назначение | Default |
| --- | --- |
| ChatGPT inference base | `https://chatgpt.com/backend-api/codex` |
| API-key inference base | `https://api.openai.com/v1` |
| OAuth refresh | `https://auth.openai.com/oauth/token` |
| OAuth revoke | `https://auth.openai.com/oauth/revoke` |
| Agent identity registration | `https://auth.openai.com/api/accounts/v1/agent/register` |
| Agent task registration | `https://auth.openai.com/api/accounts/v1/agent/{id}/task/register` |
| Agent identity JWKS | ChatGPT base + `wham/agent-identities/jwks` для base с `/backend-api`; иначе + `agent-identities/jwks` |

Источники: [provider defaults](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/model-provider-info/src/lib.rs),
[OAuth manager](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/login/src/auth/manager.rs),
[agent identity URLs](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/agent-identity/src/lib.rs),
[identity auth integration](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/login/src/auth/agent_identity.rs).

Запрет `auth.openai.com` предотвращает доступ к штатной OAuth-ротации, но может
также сделать недоступным agent identity bootstrap. Эти режимы нельзя считать
проверенными на основании API-key mock. Production env должен исключать
endpoint overrides, включая `CODEX_REFRESH_TOKEN_URL_OVERRIDE` и
`CODEX_AGENT_IDENTITY_*`; настройки custom provider не брать из corpus.

## Network boundary: реализуемый вариант

Встроенный command network proxy **не фильтрует model/auth requests harness**.
Он не заменяет границу всего процесса.
[Scope of network controls](https://learn.chatgpt.com/docs/agent-approvals-security#traffic-outside-the-command-network-proxy).

**Предложение для реализации:** сохранить private network namespace и дать ему
только отдельный IPC-канал к ограниченному egress helper. Локальный relay внутри
namespace предоставляет HTTP proxy для CLI, а наружу общается через единственный
разрешённый Unix socket. Helper принимает только CONNECT к явным authority:port,
отказывает auth authority в режиме существующего read-only cache, проверяет
DNS/IP и бюджет соединений/времени/байтов. Никакого host-network fallback.

TLS остаётся между Codex и получателем: TGSUM не завершает TLS, не добавляет
Authorization и не пишет tunnel bytes в logs. CONNECT ограничивает назначение,
но не видит HTTP paths или содержимое TLS; это **не** полный фильтр запросов и
не доказательство отсутствия domain fronting. Для строгих обещаний потребуются
проверки SNI/redirect/обходов и конкретного transport. Endpoint allowlist должна
соответствовать фактической квалификации, без wildcards на все домены OpenAI.

0.155.1 содержит routing через `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` и
`NO_PROXY`; это техническое основание для relay spike, не подтверждение всех
клиентов и WebSocket путей. Прямой fallback должен блокироваться namespace.
[Outbound routing source](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/http-client/src/outbound_proxy.rs).

### Почему найденные brokers не дают готовый reuse

- Официальный Responses proxy читает API key из stdin и вставляет header;
  он не документирует загрузку/refresh существующего ChatGPT cache.
  [Tagged proxy](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/responses-api-proxy/README.md).
- Command-backed provider auth передаёт token из stdout helper в Codex.
  Это механизм для внешнего владельца auth, не auth-only API установленного CLI.
  [Custom provider auth](https://learn.chatgpt.com/docs/config-file/config-advanced).
- Network credential brokerage обрабатывает environment credentials и MITM
  command traffic; это не отдельный managed ChatGPT inference broker.
  [Broker source](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/network-proxy/src/credential_broker.rs),
  [broker configuration](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/network-proxy/src/config.rs).

## Контракт offline проверки mount

Синтетический API-key fixture достаточен для первой проверки доступа к auth-файлу:

```json
{"auth_mode":"apikey","OPENAI_API_KEY":"tgsum-synthetic-not-a-real-key"}
```

`AuthMode` сериализуется как `apikey`; отсутствующие optional fields допустимы.
Mock provider задаёт `requires_openai_auth=true` и локальный test-only base URL.
Сервер проверяет ожидаемый Bearer sentinel, но не печатает header. Это проверяет
чтение auth официальным CLI; ChatGPT OAuth lifecycle требует отдельного fixture.
[Auth schema](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/login/src/auth/storage.rs),
[AuthMode](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/protocol/src/auth.rs),
[provider auth setting](https://learn.chatgpt.com/docs/auth#alternative-model-providers).

Необходимые результаты среза: соседние файлы/profile недоступны; mount нельзя
писать/удалять; конечный symlink/FIFO/directory отклоняются, подмена пути не
меняет выбранный inode; no-auth probe
не видит файл; cancel/cleanup не оставляет доступ; результат и ошибки не содержат
sentinel. Pin через `O_PATH` удерживает inode, **не снимок байтов** при записи
другого процесса в тот же inode. Такой сценарий должен давать отказ/повторное
подключение по выбранному контракту, без обещания immutable auth snapshot.

## Оставшаяся работа

Код: квалифицировать mount и egress helper, выбрать поддержанные auth modes,
сохранить effective managed requirements, обработать auth failure без изменения
baseline, добавить Review/Run с реальным получателем. Дополнительно проверить
tool attempts и утечки auth при ошибках: пустой advertised catalog сам по себе
не доказывает отсутствие всех read surfaces harness.

Под контролем пользователя (**tgsum-t8t.19**, backlog): существующий file/keyring
вход, настоящий success/expiry/401/rate limit, необходимые endpoint grants,
совместимость с параллельным обычным Codex. Новый dedicated login, если выбран,
также выполняет пользователь. Эти проверки не заменяют оставшийся код и не
закрывают **tgsum-hzm.7** или весь эпик.

## Реализация offline среза

`codex::SelectedAuthFile` проверяет metadata и закрепляет inode через `O_PATH`;
`OfflineRunner::run_codex_with_auth` даёт доступ только к этому read-only файлу
в offline-proc profile. Auth не передаётся version probe; TGSUM не читает bytes.
Источник через `/proc/<parent>/fd` был отвергнут ОС (Permission denied).
Использован штатный `--ro-bind-fd` bubblewrap 0.12.0: проверка dev/inode
после монтажа, затем закрытие FD; payload и namespace init не наследуют его.
Это подтверждено [tagged source](https://raw.githubusercontent.com/containers/bubblewrap/v0.12.0/bubblewrap.c)
и process fixture.

Установленный Codex 0.155.1 сам прочитал synthetic API-key и отправил ожидаемый
header локальному canned server внутри того же offline namespace. Host auth
остался неизменным, другой файл по исходному пути не заменил выбранный inode,
соседний config отсутствовал, write/unlink были запрещены, sentinel не попал
в stdout/stderr. Unit tests проверили EBADF при попытке прочитать O_PATH,
типы/permissions/hardlink/size и metadata revalidation. Реальный ключ не нужен.

Проверки и команды: [Codex adapter](../development/codex-adapter.md#проверки).
Результат не доказывает cloud auth или отсутствие всех способов утечки через
непроверенные model tools. Следующий обязательный срез — внешний egress helper
с запретом refresh authority и его проверки на synthetic endpoints.
