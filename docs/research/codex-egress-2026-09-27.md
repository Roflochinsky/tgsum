# Codex 0.155.1: ограниченный CONNECT transport

Проверено **2026-09-27 MSK**, срез **tgsum-hzm.7**. Исследование для Linux
runner: TLS проходит от официального Codex до получателя через CONNECT gateway
и Unix relay из private network namespace. Это проектный контракт, не
подтверждение работающего cloud Run. Прочитаны сохранённые исходники
`openai/codex:rust-v0.155.1` и первичные документы; реальные config/auth/profile,
аккаунты, TLS к OpenAI и inference не использовались. Продолжает
[исследование auth](codex-auth-boundary-2026-09-27.md).

## Настройки транспорта в выбранной версии

| Поверхность | Установленный по исходникам контракт |
| --- | --- |
| HTTPS | `features.respect_system_proxy=false` выбирает обычную proxy-логику reqwest. Это default 0.155.1; задавать явно для квалифицированного профиля. |
| System proxy | При включённом флаге сначала platform discovery, затем env, затем direct. На Linux platform discovery не даёт PAC; env всё равно влияет. Не полагаться на discovery как на ограничение доступа. |
| WSS | Default dialer использует proxy-enabled tokio-tungstenite с `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY`. Явный resolved route также умеет CONNECT, включая TLS к самому proxy. |
| CA | `CODEX_CA_CERTIFICATE` выбирает PEM bundle; непустой `SSL_CERT_FILE` — fallback. Пустые значения считаются отсутствующими. |
| CA semantics | Custom bundle добавляет roots; это не pinning единственного сертификата. HTTP helper выбирает rustls, WSS добавляет CA к native roots. Нечитаемый/невалидный выбранный bundle даёт ошибку. |
| Responses transport | `model_providers.<id>.supports_websockets=false` выключает WSS для provider. Старые `features.responses_websockets*` помечены Removed и не являются выключателем WSS. |

Источники: [HTTP proxy routing](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/http-client/src/outbound_proxy.rs),
[feature definitions](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/features/src/lib.rs),
[resolved config](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/core/src/config/mod.rs),
[WSS dialer](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/websocket-client/src/dialer.rs),
[CA loader](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/http-client/src/custom_ca.rs),
[provider schema](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/model-provider-info/src/lib.rs),
[Responses selection/fallback](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/core/src/client.rs).
Текущая [официальная документация CA](https://learn.chatgpt.com/docs/auth#custom-ca-bundles)
подтверждает общий HTTPS/login/WSS контракт, но не заменяет проверку версии.

### Env для relay

Runner очищает env и задаёт обе формы `HTTPS_PROXY`/`https_proxy` и
`HTTP_PROXY`/`http_proxy` в `http://127.0.0.1:<relay-port>`. `NO_PROXY`/`no_proxy`
пустые; inherited `ALL_PROXY`/`all_proxy`, CA, endpoint overrides и auth env
отсутствуют. Relay слушает только loopback **внутри** namespace. HTTP proxy URL
здесь намеренно имеет `http` scheme: туннелируемое содержимое остаётся TLS.

Это рекомендация для нашего запуска, а не заявление, что каждая библиотека
Codex всегда соблюдает proxy env. Процессу по-прежнему запрещён обычный выход
из namespace; единственный внешний канал — выделенный Unix socket gateway.
Попытка direct fallback должна завершаться ошибкой. Встроенный command proxy
не охватывает model/auth traffic harness.
[Официальная граница command proxy](https://learn.chatgpt.com/docs/agent-approvals-security#traffic-outside-the-command-network-proxy).

## Квалификация на HTTPS fixture без аккаунта

Использовать отдельный синтетический TLS endpoint с SAN `fixture.test` и CA,
доступным Codex как `/runtime/fixture-ca.pem`. Сертификат проверяет **Codex**;
gateway не завершает TLS и не получает его ключи. Test gateway сопоставляет
ровно `fixture.test:443` с одним заранее созданным loopback listener; это
test-only dependency injection, не production разрешение private IP или DNS.

Только fixture wrapper добавляет к существующему фиксированному argv:

```toml
model_provider = "tgsum_fixture"
features.respect_system_proxy = false
features.unbounded_connection_retries = false

[model_providers.tgsum_fixture]
name = "TGSUM synthetic HTTPS provider"
base_url = "https://fixture.test/v1"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
stream_idle_timeout_ms = 3000
```

И env `CODEX_CA_CERTIFICATE=/runtime/fixture-ca.pem`. Private HOME пуст;
auth-файл в первом тесте отсутствует. Модель выбирается только для bundled
metadata, сервер отдаёт canned SSE. Во втором тесте допустим **synthetic**
API-key auth-файл и `requires_openai_auth=true`: проверяется TLS-доставка sentinel
самим Codex, без managed OAuth refresh. Refresh/401 отдельной ChatGPT-сессии
этим не квалифицируется. Provider и auth fields описаны в
[versioned schema](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/model-provider-info/src/lib.rs).

Проверить success через CONNECT; отсутствие Authorization в plaintext relay;
ошибку без доверенного CA; запрет другого CONNECT authority, другого SNI,
прямого TCP и refresh authority; timeout/cancel/cleanup. Не печатать raw
туннель/HTTP headers. Fixture server может сравнивать synthetic sentinel
в памяти, выводя только boolean. Успешный SSE-тест не квалифицирует WSS:
для него нужна отдельная fixture либо явный SSE-only provider profile.

## CONNECT: что ограничивать

Gateway принимает один HTTP/1.1 CONNECT на соединение, точный разрешённый DNS
host и явный порт 443. Отклоняет URL вместо authority, userinfo, IP literal,
wildcard/suffix matching, неизвестные headers, дубликаты Host, framing/body,
absolute-form forwarding и произвольный upstream proxy. Header bytes,
количество соединений, handshake time, общий срок и переданные bytes ограничены.

CONNECT не имеет request content; успешный response переключает соединение
на туннель после headers и **не содержит** `Content-Length`/`Transfer-Encoding`.
Данные после headers нельзя терять или ошибочно разбирать как следующий HTTP
request. После EOF передать оставшиеся уже принятые bytes в пределах deadline,
затем закрыть обе стороны. Эти требования следуют из
[RFC 9110 §9.3.6](https://www.rfc-editor.org/rfc/rfc9110.html#section-9.3.6).
Более узкий header/authority профиль — наше решение для pinned CLI;
совместимость проверяется фактическим CONNECT установленной версии.

Production DNS разрешает **gateway**, после exact-host allowlist. До connect
проверяет адреса, исключая loopback/private/link-local/multicast/unspecified,
IPv4-mapped обходы и специальные диапазоны по явной поддерживаемой политике.
Соединяется с проверенным `SocketAddr`, без повторного resolve hostname в
connect; это предотвращает подмену адреса между проверкой и соединением.
Не передавать поиск/DNS целиком процессу агента. Строгий отказ для смешанного
public/private набора — допустимый выбор политики. Значение «public» требует
теста классификатора и источника диапазонов; один `!is_private()` недостаточен.

## ClientHello: границы проверки SNI

До отправки payload upstream собрать ограниченный ClientHello, проверить все
длины и ровно один DNS `host_name`, совпадающий с CONNECT authority. SNI содержит
ASCII DNS name без trailing dot; IP literal там запрещён, имена case-insensitive.
Это [RFC 6066 §3](https://www.rfc-editor.org/rfc/rfc6066.html#section-3).
Отсутствующий/дублированный/невалидный SNI — отказ нашего профиля.

ClientHello может быть разбит между TLS records и TCP reads; один read или один
record не равен handshake. Проверять ContentType/record lengths, handshake
type/length, session ID, cipher suites, compression и весь extensions vector.
Повтор extension type запрещён. Можно ввести меньший фиксированный byte cap,
чем теоретический TLS максимум, явно объявив compatibility limit.
[RFC 8446 §4.1.2, §4.2, §5.1](https://www.rfc-editor.org/rfc/rfc8446.html#section-4.1.2).

ECH опубликован как **RFC 9849, март 2026**: `encrypted_client_hello` имеет
тип **0xfe0d**. Внешний SNI при ECH не доказывает внутренний server name.
Если профиль опирается на видимый SNI, отклонять **любое** присутствие этой
extension, включая GREASE. Надёжно отличать GREASE от настоящего ECH по одному
соединению не обещать. Это ограничение совместимости нашего профиля, не
требование TLS и не утверждение, что Codex 0.155.1 посылает ECH.
[RFC 9849 §5, §6.2, §10.10](https://www.rfc-editor.org/rfc/rfc9849.html).

### Что pass-through не доказывает

Gateway видит адрес, CONNECT authority, начальный открытый ClientHello и
размеры/время. Он **не видит** encrypted HTTP `Host`/`:authority`, URL path,
headers, credential, тело или протокол после handshake. Соответствие начального
SNI не является доказательством назначения каждого последующего HTTP request;
в том числе не контролирует HTTP/2 coalescing/domain fronting. Это вывод из
непрозрачности TLS application data, описанной
[RFC 8446 §5.1](https://www.rfc-editor.org/rfc/rfc8446.html#section-5.1).

Поэтому корректное обещание: ограничены разрешённые **TCP destinations и
начальные TLS names**. Не обещать «только POST /responses», отсутствие отправки
secrets разрешённому получателю или универсальный запрет серверной ротации.
Запрет `auth.openai.com` запрещает доступ к штатному refresh endpoint выбранной
версии при фиксированных настройках; операции на разрешённом host и новые
endpoint mappings требуют отдельного review. Нельзя расширять allowlist
автоматически после 401/redirect. Подробнее —
[auth boundary](codex-auth-boundary-2026-09-27.md#read-only-файл-и-refresh--разные-разрешения).

## Решение для среза

Реализовать отдельный opt-in профиль с Unix relay, bounded CONNECT gateway,
exact-host/port policy и ClientHello guard; оставить прежние offline profiles
без сети. Сначала квалифицировать установленный CLI через synthetic HTTPS.
Production cloud включать после реализации auth/error/Review contracts и
подконтрольной пользователю проверки **tgsum-t8t.19**. Этот документ не меняет
support status и не закрывает **tgsum-hzm.7**.

## Реализованный host gateway

В `tgsum_runner::egress` реализован отдельный `InferenceGateway`: private Unix
socket, exact target, CONNECT/ClientHello validation и ограниченное двустороннее
копирование TLS bytes. Первый тест выявил, что нельзя полагаться на default
permissions TempDir; теперь `0700` задаётся явно до появления socket `0600`.
На том этапе relay/монтаж ещё отсутствовал; последующий срез описан ниже.

Для DNS используется bounded `getent ahosts` subprocess вместо потенциально
неотменяемого resolver thread. Команда получает только fixed destination;
output ограничен, mixed public/private набор отклоняется. Проверенный literal
SocketAddr используется напрямую. `getent` и системный NSS — доверенная часть
host, доступность production DNS в этих тестах не проверялась.
[getent manual](https://man7.org/linux/man-pages/man1/getent.1.html).

IP policy — conservative subset по
[IANA IPv4 registry](https://www.iana.org/assignments/iana-ipv4-special-registry)
и [IANA IPv6 registry](https://www.iana.org/assignments/iana-ipv6-special-registry),
прочитанным 2026-09-27: private/special/multicast исключены, IPv6 только `2000::/3`
без `2001::/23`, документационных prefixes, 6to4. В том числе отклоняются
globally-reachable exceptions внутри исключённых ranges; это наша ограниченная
политика, не полная реализация флага IANA Globally Reachable.

Восемь tests прошли на local fixtures, включая настоящий TLS 1.3 с проверкой
сертификата и synthetic auth-header. Только тестовый TLS server знает private key;
gateway не получает TLS configuration или roots. Для тестов использованы
[Rustls](https://docs.rs/rustls/0.23.35/rustls/) и
[rcgen](https://docs.rs/rcgen/0.13.2/rcgen/); lockfile фиксирует rustls 0.23.45,
rcgen 0.13.2. Они добавлены только как Linux dev-dependencies.

Полный Rust gate прошёл: **139 passed, 11 environment tests ignored**.
Эти 11 прежних process qualification tests не повторялись в данном срезе:
поведение offline/auth launch не менялось. Новые восемь gateway tests входят
в обычный gate. Контракт, команды и текущие ограничения:
[inference egress](../development/inference-egress.md).

## Namespace integration и installed CLI

Следующий срез подключил `CodexNetworkRunner` / `tgsum-codex-relay`: O_PATH mount
только Unix socket, отдельный network namespace, fixed `/runtime/codex`, private
HOME и очищенный proxy env. Version probe проходит без auth/gateway. Отдельные
тесты подтвердили отсутствие обоих mount FD в payload и namespace init.

Реальная проверка CLI выявила ошибку первоначального argv: в 0.155.1 нельзя
переопределять built-in provider ID `openai`. Использован собственный
`tgsum_openai` с именем `TGSUM OpenAI`, `requires_openai_auth=true`, Responses,
`supports_websockets=false`, retries=0. Base URL отсутствует: `to_api_provider`
выбирает endpoint по auth mode. Эта конфигурация не включает `is_openai()`-ветки
по provider name; другие auth/model modes требуют проверки. Источник:
[provider implementation](https://github.com/openai/codex/blob/rust-v0.155.1/codex-rs/model-provider-info/src/lib.rs).

Пройден весь путь TLS в двух ignored-тестах (шесть сценариев): success без auth,
success с synthetic API key, wrong CA, refresh authority denial, cancel, timeout.
В отличие от предложенного выше примера с `fixture.test`, фактический fixture
использует SAN/CONNECT `api.openai.com` и private Dial → `127.0.0.2`; так exact-host
policy не подменяется. Auth-вариант сохраняет production provider config.
Проверены пустой tool catalog, schema, evidence и отсутствие baseline. Wrong-CA
тест требует фактически достигнутого TLS сервера; ранняя ошибка запуска его не
удовлетворяет. Сертификат проверяет Codex, gateway TLS не расшифровывает.

Relay Unix connect стал nonblocking: заполненная очередь немедленно отклоняется,
чтобы join workers не блокировал cleanup. Для этого включена `net` существующего
rustix; новый обычный regression test использует local backlog=0. Также обычный
тест запрещает OfflineRunner принимать egress profile до чтения runtime.

Полный gate: **141 passed, 13 ignored**. Отдельно выполнены все 13 environment
tests: isolation 7/7, прежние Codex process 4/4, новые HTTPS 2/2. Реальных профилей,
аккаунтов, DNS/TCP к OpenAI не использовано. Production DNS, ChatGPT OAuth, WSS,
runtime packaging, auth/result lifecycle и Review/Run остаются вне доказанного
среза. `tgsum-hzm.7` остаётся in progress.
