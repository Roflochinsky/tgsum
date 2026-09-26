# Ограниченный исходящий транспорт

Linux-модуль `tgsum_runner::egress` реализует host gateway и relay для отдельного
профиля `CodexNetworkRunner`. Цепочка namespace → relay → Unix socket → gateway
→ HTTPS проверена установленным Codex 0.155.1 на локальном сервере готовых ответов,
включая synthetic auth. Существующие `OfflineRunner` profiles остаются без внешней
сети. UI Run, реальные аккаунты и production cloud support ещё не квалифицированы.

Основание: [исследование версии Codex и протоколов](../research/codex-egress-2026-09-27.md),
[ограничения auth refresh](../research/codex-auth-boundary-2026-09-27.md).

## Interface

`InferenceGateway::start(destination, limits, cancellation)` создаёт новый
каталог с явным mode `0700` и Unix socket `0600`. Он принимает один из двух
точных получателей: `Destination::OpenAiApi` → `api.openai.com:443` либо
`Destination::ChatGpt` → `chatgpt.com:443`. Один gateway разрешает один target.
Произвольного host, IP, proxy URL или параметра обхода private-IP policy нет.

Доверенный runner монтирует **только socket** по закреплённому O_PATH inode в
`/gateway/proxy.sock`, read-only. Проверяются тип, owner, mode, nlink и fd ≥ 3;
весь каталог gateway не доступен. `socket_path()` используется для диагностики
жизненного цикла. `finish()` останавливает приём и workers, возвращает статические
счётчики/категории ошибок. `Drop` делает ту же очистку и удаляет каталог.
`Report` не содержит request headers, tunnel bytes или account data.

Version qualification входит в `CodexNetworkRunner`. Review и выбор получателя
остаются обязанностями будущей интеграции с приложением. Сам факт создания gateway
не означает разрешение запуска анализа или действительность auth-файла.

## Namespace relay и Codex

`CodexNetworkRunner::qualify(relay, codex, files, cancellation)` копирует точные
runtime files в private staging. `tgsum-codex-relay` запускает только
`/runtime/codex`; оба executable и динамические libraries задаёт доверенный host.
Версионный probe проходит в пустом context, без auth и gateway; требуются
`tgsum-codex-relay 1` и `codex-cli 0.155.1`. Он наследует пустой stdin pipe:
namespace не имеет `/dev/null`. Реальные profile/config не обнаруживаются.

`run(request, selected_auth, destination, limits, cancellation)` принимает
проверенный `CodexRequest`, один явно выбранный auth-файл и typed destination.
Используется отдельный `linux-x86_64-bwrap-codex-egress-v1`; `OfflineRunner`
отклоняет этот profile. Network namespace остаётся отдельным: relay слушает
`127.0.0.1` внутри него и передаёт bytes только в fixed Unix socket. Файл auth и
socket передаются bwrap как два O_PATH descriptor; bwrap закрывает их до payload.
Gateway живёт до возврата процесса, затем останавливается и удаляет свой каталог.

Relay очищает env, создаёт private `CODEX_HOME`, задаёт proxy env в обоих регистрах
и пустые `NO_PROXY/no_proxy`. Host endpoints, auth env и `ALL_PROXY` не наследуются.
Опциональный public CA передаётся только как заранее staged
`/runtime/provider-ca.pem` → `CODEX_CA_CERTIFICATE`, без поиска host trust files.
Версионный probe не получает auth или proxy/CA env. Relay имеет не более 32 admissions / 4 workers,
bounded buffers/bytes и deadline; nonblocking Unix connect отклоняет заполненную
очередь, поэтому join workers не ждёт заблокированного connect.

Codex получает собственный фиксированный provider `tgsum_openai`: Responses/SSE,
`requires_openai_auth=true`, без WebSocket и автоматических connection retries.
Built-in ID `openai` в 0.155.1 нельзя переопределять. Base URL не задаётся: CLI сам
выбирает API/ChatGPT endpoint по auth mode, а gateway пропускает только reviewed
destination. Несовпадение не даёт переключения получателя. API-key режим проверен
на synthetic auth; ChatGPT/OAuth modes ещё не квалифицированы.

Возвращается `NetworkOutput { process, gateway, destination }`. Debug не содержит
payload. Exit 0 не подтверждает schema/evidence или отсутствие отказов gateway:
`NetworkOutput::decode` проверяет process, отсутствие ошибок gateway, completed
transport и JSONL/typed validator. Низкоуровневый `run` не записывает анализ;
новый [`run_analysis`](analyses.md) сохраняет checked result и commit baseline.
Синхронный runner следует вызывать
на отдельном worker, удерживая его живым до возврата.

## Обработка соединения

1. Ограниченный HTTP/1.1 CONNECT: exact lower-case authority и порт 443, один
   совпадающий Host; разрешены только User-Agent, Connection, Proxy-Connection.
   Body framing, дубликаты, credentials headers, IP literals и другие hosts
   отклоняются до DNS. Этот узкий профиль проверен на исходящем CONNECT
   установленного Codex 0.155.1 в описанном SSE-тесте.
2. После CONNECT 200 собирается ClientHello, в том числе между TLS records.
   Проверяются длины, тип, session/cipher/compression vectors, отсутствие
   повторных extensions, ровно один SNI равный target. ECH, включая GREASE,
   запрещён. Upstream ещё не открыт; неверному TLS-имени байты не передаются.
3. Доверенный host запускает `/usr/bin/getent ahosts <fixed-host>` с очищенным
   env, закрытыми inherited FD, timeout и пределами stdout/stderr. Helper должен
   быть root-owned, обычным, без setuid/setgid/group/other write. Отдельный процесс
   позволяет отменить DNS/NSS без оставленного resolver thread. Системный NSS
   остаётся доверенной частью host; TGSUM не передаёт ему context или auth.
4. Результаты deduplicate; весь набор отклоняется при private/special IP или
   более 16 адресах. TCP connect получает проверенный `SocketAddr` без нового
   resolve; peer address сверяется. Только conservative public-unicast subset:
   IPv4 special/private/multicast/reserved исключены, IPv6 ограничен `2000::/3`
   с дополнительными исключениями. Источники диапазонов указаны в research.
5. Исходный ClientHello и дальнейшие байты передаются без изменения и
   расшифровки. Ограниченные buffers, backpressure, half-close, cancel, idle и
   общий deadline действуют на оба направления. Проверку сертификата делает
   TLS client, gateway не получает ключи TLS.

`auth.openai.com` не входит ни в один target. Это запрещает штатное отдельное
refresh authority выбранной версии, но не контролирует операции на разрешённом
host. Gateway не видит encrypted HTTP Host/path или domain fronting/coalescing;
не обещает «только /responses» либо отсутствие утечки разрешённому получателю.
Ограничена первая TLS identity, не каждый последующий encrypted request.

## Пределы

| Ресурс | Default / максимум |
| --- | --- |
| Время gateway | 300 s / 1800 s |
| CONNECT + ClientHello | 10 s / 30 s |
| Idle одного tunnel | 30 s / 60 s |
| Всего принятых соединений, включая отказанные | 16 / 32 |
| Одновременно работающих tunnels | 4 / 4 |
| Bytes одного tunnel, оба направления | 16 MiB / 64 MiB |
| Общие bytes всех tunnels | 64 MiB / 256 MiB |
| CONNECT headers | 8192 bytes |
| Первый handshake | 65536 bytes, максимум 16 records по 16384 bytes |
| Буферы после handshake | По 16 KiB в каждом направлении |
| DNS | 5 s, stdout 8192 bytes, stderr 1024 bytes, до 16 unique IP |
| Одна TCP connect попытка | До 1 s и оставшегося общего deadline |

Счётчик bytes означает допущенные в buffers bytes, не подтверждённую доставку
или provider billing. ClientHello входит в бюджет. Превышение прекращает tunnel.
После исчерпания числа admissions listener закрывается; существующие tunnels
доживают до закрытия/cancel/deadline. Отмена native TCP connect может ждать до
одной секунды. DNS helper отменяется/ожидается через существующий process runner.

Gateway сам по себе не защищён от другого процесса того же host-пользователя;
общие CPU/RAM/cgroup limits остаются задачей runner. Неизвестный сертификат или
TLS rejection не дают fallback на plaintext/другой host. Свежесть IANA registry
и endpoint mappings проверяется перед расширением/выпуском по connector policy.

## Проверки без аккаунтов

```sh
cargo test -p tgsum-runner --all-features --locked egress -- --nocapture
bash scripts/check.sh
```

Девять обычных Linux egress tests покрывают: CONNECT attacks/plaintext auth; public/private
IPv4/IPv6 и смешанный resolver result; fragmented ClientHello, SNI/ECH/duplicates;
opaque bytes и half-close; handshake timeout/cancel/admission; отдельный и
**общий между соединениями** byte budget; права/удаление Unix socket; реальный
TLS 1.3 с локальным self-signed certificate и synthetic Authorization; отказ
relay при заполненной очереди Unix socket без блокировки cleanup.
`rustls` и `rcgen` — только Linux dev-dependencies, gateway не использует их
для TLS termination. В production library включена feature `net` существующего
`rustix` для nonblocking Unix connect; TLS/crypto dependencies не добавлены.

Тесты подключают loopback server через private `Dial` seam; публичный constructor
не позволяет этот override. Реальный DNS/TCP к OpenAI и аккаунты не использованы.

Два дополнительных ignored-теста проверяют весь путь установленного CLI:

```sh
cargo build -p tgsum-runner --all-features --bins --locked
TGSUM_CODEX_TEST_BINARY=/absolute/path/to/codex \
TGSUM_RELAY_TEST_BINARY="$PWD/target/debug/tgsum-codex-relay" \
TGSUM_HTTPS_FIXTURE_BINARY="$PWD/target/debug/tgsum-codex-https-fixture" \
  cargo test -p tgsum-runner --all-features --locked --lib installed_codex_https -- --ignored --nocapture
```

Нужны static Linux x86_64 Codex 0.155.1 и bwrap 0.12.0. Test wrapper проверяет
env/FD/direct-TCP boundary и запускает CLI. В auth-варианте используется production
provider config; вариант без auth выбирает test provider. Локальный TLS server
имеет SAN `api.openai.com`: точные CONNECT/SNI остаются production, только Dial
направляется на `127.0.0.2`. Проверяются запрос без tools, schema, scoped evidence,
точный synthetic Bearer внутри TLS, отсутствие ключа в stdout/stderr, неверный CA
после фактического TLS handshake, запрет `auth.openai.com`, cancel/timeout и удаление
gateway. TLS handshake failure не считается успехом проверки, если CLI не дошёл
до сервера. Теперь success проходит до durable result и baseline через
`AnalysisJob`; negative cases оставляют его пустым. Добавлены HTTP 401 с synthetic
API key и mismatch API-key/ChatGPT destination без переключения получателя.

Предстоят synthetic ChatGPT/OAuth success/expiry/revoke, Review/Run, упаковка runtime и
контролируемая пользователем реальная квалификация `tgsum-t8t.19`. WSS, ChatGPT
OAuth и ОС кроме Linux этим не доказаны. `tgsum-hzm.7` остаётся открытой.
