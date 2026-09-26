# Ограниченный исходящий транспорт

Linux-модуль `tgsum_runner::egress` реализует host gateway для будущего сетевого
профиля Codex. Он уже передаёт TLS через private Unix socket. Relay внутри
namespace, его монтаж и запуск установленного Codex через этот gateway ещё
не подключены. Существующие `OfflineRunner` profiles остаются без внешней сети;
UI Run и production cloud support этим этапом не включены.

Основание: [исследование версии Codex и протоколов](../research/codex-egress-2026-09-27.md),
[ограничения auth refresh](../research/codex-auth-boundary-2026-09-27.md).

## Interface

`InferenceGateway::start(destination, limits, cancellation)` создаёт новый
каталог с явным mode `0700` и Unix socket `0600`. Он принимает один из двух
точных получателей: `Destination::OpenAiApi` → `api.openai.com:443` либо
`Destination::ChatGpt` → `chatgpt.com:443`. Один gateway разрешает один target.
Произвольного host, IP, proxy URL или параметра обхода private-IP policy нет.

`socket_path()` нужен доверенному runner для монтажа **только socket** в отдельный
network namespace. `finish()` останавливает приём и workers, возвращает статические
счётчики/категории ошибок. `Drop` делает ту же очистку и удаляет каталог.
`Report` не содержит request headers, tunnel bytes или account data.

Version qualification, Review и выбор фактического получателя остаются
обязанностями будущего adapter. Сам факт создания gateway не означает разрешение
запуска анализа или действительность auth-файла.

## Обработка соединения

1. Ограниченный HTTP/1.1 CONNECT: exact lower-case authority и порт 443, один
   совпадающий Host; разрешены только User-Agent, Connection, Proxy-Connection.
   Body framing, дубликаты, credentials headers, IP literals и другие hosts
   отклоняются до DNS. Это намеренно узкий профиль, ещё не квалифицированный
   на исходящем CONNECT установленного Codex.
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

Восемь Linux tests покрывают: CONNECT attacks/plaintext auth; public/private
IPv4/IPv6 и смешанный resolver result; fragmented ClientHello, SNI/ECH/duplicates;
opaque bytes и half-close; handshake timeout/cancel/admission; отдельный и
**общий между соединениями** byte budget; права/удаление Unix socket; реальный
TLS 1.3 с локальным self-signed certificate и synthetic Authorization.
`rustls` и `rcgen` — только Linux dev-dependencies, gateway не использует их
для TLS termination. В production library новых network/crypto dependencies нет.

Тесты подключают loopback server через private `Dial` seam; публичный constructor
не позволяет этот override. Реальный DNS/TCP к OpenAI и аккаунты не использованы.
Это доказывает gateway отдельно. Предстоят: Unix relay/namespace mounting,
установленный Codex + HTTPS fixture, negative CA/direct-network/refresh tests,
auth failure/result lifecycle и Review/Run. `tgsum-hzm.7` остаётся открытой.
