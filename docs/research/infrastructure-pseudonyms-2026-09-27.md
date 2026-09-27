# Infrastructure pseudonyms: распознавание и стабильная identity

Проверено **2026-09-27** для `tgsum-af2.2`, исходная revision `ab35141`.
Использованы только первичные публичные спецификации, документация библиотек
и исходники репозитория. DNS, открытие найденных URL, filesystem probing
найденных путей, реальные аккаунты и данные не использовались.

В [Cargo.lock](../../Cargo.lock) зафиксированы `url 2.5.8` и `idna 1.1.0`.
На момент проверки это существующие транзитивные зависимости; прямой зависимости
`url` в [core/Cargo.toml](../../core/Cargo.toml) ещё нет. Локальный Rust —
`1.98.1`, совпадающий с прочитанной std documentation. Mapping уже предоставляет
project-local labels и immutable generations;
[его контракт](../development/pseudonyms.md) требует явной нормализации у detector.

## Рекомендуемый контракт

Это **решения TGSUM**, а не обязательства платформ:

- Категории IP, host/domain, internal URL, username, local path и cloud identifier
  включаются независимо. Высокая уверенность означает узнаваемую текстовую форму,
  а не доказательство существования, приватности или принадлежности объекта.
- Сохранять original UTF-8 span отдельно от canonical identity. Только выбранные
  spans заменяются label; остальные bytes остаются исходными.
- Internal URL заменяется **целиком** только своей URL-категорией. Каждый
  распознанный допустимый URL защищает весь span от standalone IP/host/path/user
  detectors, даже если URL-категория выключена. Public URL сохраняется byte-for-byte.
  Независимый secrets scanner по-прежнему скрывает credentials в его контексте.
- URL parsing нужен для классификации/canonical identity. Не сериализовать
  распознанный public URL обратно в output ради форматирования.
- Сначала распознавать структурные контейнеры, затем standalone формы вне них.
  Google full resource name и Azure resource ID не должны стать кусками POSIX path;
  части уже защищённого URL не должны стать cloud findings.

## 1. URL parsing и normalization: подтверждённые факты

`url 2.5.8` реализует WHATWG URL Standard. `Url` предоставляет отдельные scheme,
host, username/password, port, path, query и fragment. Parsing может вернуть
ошибку; для relative URL нужен base. Некоторые schemes имеют opaque path вместо
обычной host/path структуры.
[url 2.5.8](https://docs.rs/url/2.5.8/url/).

`idna 1.1.0` реализует преобразования WHATWG/UTS #46 и Punycode. Это инструмент
представления domain names, а не проверка их доступности или владельца.
[idna 1.1.0](https://docs.rs/idna/1.1.0/idna/).

RFC 3986 разделяет authority (`userinfo`, host, port), path, query и fragment.
Scheme/host регистронезависимы; остальные generic components предполагаются
регистрозависимыми, если конкретный scheme не определяет иное. Percent-encoded
reserved delimiters нельзя произвольно декодировать до выделения компонентов:
это способно изменить структуру URI. RFC описывает query как компонент с
данными, не задавая универсального правила сортировки key/value parameters.
[RFC 3986 §§2.2, 3.2, 3.4, 6.2.2](https://www.rfc-editor.org/rfc/rfc3986.html).

**Решения TGSUM:**

- Начальный набор URL schemes должен быть явным. Наличие `:` не означает
  network URL: AWS ARN, Windows drive и opaque URI имеют другой смысл.
- Для canonical internal URL допустима pinned `Url` normalization, но path/query
  case сохраняется; query order и повторные keys не сортируются. `?a=1&a=2`
  не объединяется с переставленным вариантом. Не выкидывать fragment без
  явного контракта: client-side routes могут различаться именно им.
- Default port removal, dot-segment processing, IDNA и прочая parser normalization
  могут изменить serialization. Поэтому `url::Position` относится к parsed URL
  representation; его нельзя использовать как offsets исходного сообщения.
- Userinfo извлекается как компонент, а не regex по первому `@` во всём тексте.
  `%40` в username/password и `@` в path/query имеют разную роль. Username-only
  URL не доказывает наличие password; secrecy policy работает отдельно.
- Public URL с `/home/person/`, IP-подобным query, mixed case и percent encoding
  остаётся неизменным, если secrets scanner не нашёл отдельный credential.
- Malformed/unsupported URL не становится «проверенным public URL». Не скрывать
  только распознанную середину, оставляя опасный или случайно сломанный хвост.
  Конкретное поведение на таких candidates закрепить отрицательными fixtures.

## 2. IP, ports и IPv6 zones

Rust `IpAddr` представляет IPv4/IPv6. `Ipv4Addr::from_str` принимает четыре
decimal octets и отвергает octal/hex формы, включая leading-zero octets.
`SocketAddr` отдельно содержит IP и 16-bit port; port не является частью IP.
[IpAddr](https://doc.rust-lang.org/std/net/enum.IpAddr.html),
[Ipv4Addr textual representation](https://doc.rust-lang.org/std/net/struct.Ipv4Addr.html#textual-representation),
[SocketAddr](https://doc.rust-lang.org/std/net/enum.SocketAddr.html).

Private IPv4 ranges RFC 1918: `10/8`, `172.16/12`, `192.168/16`.
Unique-local IPv6 prefix — `fc00::/7`; это отдельный класс от link-local/loopback.
[RFC 1918 §3](https://www.rfc-editor.org/rfc/rfc1918.html#section-3),
[RFC 4193 §3.1](https://www.rfc-editor.org/rfc/rfc4193.html#section-3.1).

RFC 9844 (August 2025) **отменяет RFC 6874**, включая прежнее расширение URI
syntax для zone IDs, и задаёт требования к UI вводу scoped addresses. Zone
рассматривается отдельно от IPv6 address. Поэтому старый RFC 6874 нельзя
использовать как обещание, что любой современный URL parser принимает `%25zone`.
[RFC 9844 §§1–3](https://www.rfc-editor.org/rfc/rfc9844.html).

**Решения TGSUM:**

- Standalone IP проверять полным parse, не совпадением подстроки. Canonical
  identity — address family + bytes; разные допустимые IPv6 spellings получают
  одну identity. Не объединять IPv4 и IPv4-mapped IPv6 автоматически без
  отдельного versioned правила.
- `10.0.0.1:5432` и `10.0.0.1:443` используют один IP label; port сохраняется
  отдельным suffix, если сам endpoint не выбран как другая категория. URL
  identity включает port по правилам URL parser.
- IPv6 endpoint требует явных brackets для отделения port. Последнюю hextet
  у неокаймлённого IPv6 нельзя считать port.
- Scoped literal распознавать целиком; canonical identity включает address и
  точный zone identifier. `%eth0` и `%eth1` не объединяются. Interface names
  не преобразуются в числовой index через ОС. Неподдержанный zone grammar
  явно остаётся ограничением; удаление только IP с сохранением zone недопустимо.
- URL-host parsing и standalone `IpAddr` имеют разные grammar. WHATWG host
  parser может принимать дополнительные numeric IPv4 representations;
  классифицировать URL через его parsed Host, не standalone regex.
- Internal URL host policy перечисляет private/link-local/loopback/ULA ranges
  явно. «Всё, что не global» шире RFC1918 и не доказывает внутренний ресурс.
  Standalone IP-category может покрывать публичные literals независимо от этого.

## 3. Hostnames и private domains

| Подтверждённая форма | Первичный источник | Решение для default policy |
| --- | --- | --- |
| `localhost` и names под `.localhost` | [RFC 6761 §6.3](https://www.rfc-editor.org/rfc/rfc6761.html#section-6.3) задаёт loopback semantics | Поддерживать exact name и suffix по границе label. |
| `.local` | [RFC 6762 §3](https://www.rfc-editor.org/rfc/rfc6762.html#section-3) определяет link-local significance | Поддерживать names в этом namespace. |
| `.home.arpa` | [RFC 8375](https://www.rfc-editor.org/rfc/rfc8375.html) резервирует residential homenet namespace, не глобально уникальный | Поддерживать exact namespace/suffix. |
| `.internal` | [ICANN resolution 2024.07.29.06](https://www.icann.org/en/board-activities-and-meetings/materials/approved-resolutions-special-meeting-of-the-icann-board-29-07-2024-en) постоянно исключает его делегирование в root для private use | Допустим обоснованный default; это ICANN reservation, не утверждение об отдельной mDNS semantics. |

**Решения TGSUM:** host identity использует проверенное domain-to-ASCII и
case normalization; trailing root dot обрабатывается явно. Сравнение suffix
требует `name == suffix` либо `name.ends_with("." + suffix)` после нормализации.
`notlocalhost.example` и `local.example` не принадлежат `.localhost`/`.local`.

`.corp`, `.lan`, публичный домен организации и bare host вроде `prod-db-03`
не классифицировать универсально по предположению. Для них нужны explicit
internal-domain/host settings или узкий контекст `host=…`, `server=…`.
Dotless слово в обычном предложении не становится hostname автоматически.
Не дополнять short names поисковым domain ОС и не выполнять DNS lookup.
Политика не выводит связь hostname↔IP из сетевой доступности.

## 4. Local paths и infrastructure usernames

Microsoft различает drive-rooted `C:\path`, drive-relative `C:path`, UNC
`\\server\share\path` и специальные device namespaces. Drive letters
регистронезависимы; filesystem behavior не универсально case-insensitive.
Prefix `\\?\` изменяет обычную обработку path strings.
[Windows naming and namespaces](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).

Rust `Path::components` выполняет ограниченную lexical normalization, но
сохраняет `..`: возможный symlink меняет смысл его родителя. `canonicalize`
обращается к filesystem, разрешает symlinks и требует существующих components.
[Rust Path](https://doc.rust-lang.org/std/path/struct.Path.html#method.components).

OpenSSH документирует `[user@]hostname`, `ssh://[user@]hostname[:port]` и
`-l login_name`; SCP — `[user@]host:[path]` и `scp://…`.
[OpenSSH ssh](https://man.openbsd.org/ssh), [OpenSSH scp](https://man.openbsd.org/scp).

**Решения TGSUM:**

- Парсить Windows/UNC/POSIX-style strings собственной явной lexical grammar,
  независимо от ОС, где работает TGSUM. Linux `Path` не является parser Windows.
  Не использовать `canonicalize`, `exists`, `read_link`, shell expansion или
  процессные `$HOME`/cwd для интерпретации переписки.
- Для POSIX-style сохранять case и `..`; не объявлять два разных lexical пути
  одним реальным файлом. Не сворачивать leading `//` в `/` без отдельного
  правила: он пересекается с schemeless URL/cloud names.
- Drive letter можно нормализовать отдельно. Остальные Windows components
  консервативно сохранять: слепой lowercase может объединить разные файлы на
  case-sensitive storage. UNC hostname и share/path — разные компоненты.
- Сначала поддержать узнаваемые absolute/UNC и quoted paths. Пробел внутри
  quoted path входит в полный span; unquoted prose не позволяет безошибочно
  определить, где кончается имя файла с пробелом. Relative/device paths и shell
  escapes должны иметь собственные fixtures либо оставаться явным ограничением.
- Slash сам по себе неоднозначен: division, dates, HTTP routes и Markdown links
  не объявлять local path. Нужны path grammar/context и защита URL/cloud spans.
- Username находить в явных assignments (`user`, `username`, `login`) и
  распознанных SSH/SCP contexts. `user=…` — эвристика TGSUM, не стандарт всех
  config languages. Голое `person@host` может быть email и не доказывает SSH.
- Username case сохраняется. Если доступен host/realm, включать namespace в
  identity: одинаковый `deploy` на разных системах не доказывает один account.
  Не выводить identity человека из системного username. Credentials остаются
  отдельной задачей secrets scanner.

## 5. Cloud resource identifiers

**AWS.** ARN содержит partition, service, region, account и resource suffix;
region/account для некоторых ресурсов могут быть пустыми. Suffix может
содержать `/`, `:` и qualifier; конкретная grammar зависит от сервиса.
AWS не считает ARN credential/секретом сам по себе.
[AWS ARN reference](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference-arns.html).
В IAM username внутри ARN/policy регистрозависим, даже если login name
сравнивается иначе. [IAM identifiers](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_identifiers.html).

**Azure.** ARM IDs зависят от scope: subscriptions, resource groups, management
groups, tenant и extension resources имеют разные prefixes; common resource
form содержит `/subscriptions/.../resourceGroups/.../providers/...`.
[ARM resource functions](https://learn.microsoft.com/en-us/azure/azure-resource-manager/templates/template-functions-resource#resourceid).
Microsoft описывает resource/group names как case-insensitive с отдельными
исключениями для resource types; API может вернуть другой casing.
[Azure name rules](https://learn.microsoft.com/en-us/azure/azure-resource-manager/management/resource-name-rules).

**Google Cloud.** AIP-122 задаёт full resource name как schemeless URI с service
name и relative resource name: `//service.googleapis.com/collection/id/...`.
Это отличается от API URL с protocol/version; полное имя сохраняется между
версиями API. Конкретные resource ID constraints принадлежат сервису.
[AIP-122](https://google.aip.dev/122#full-resource-names).

**Решения TGSUM:**

- Cloud category распознаёт полный standalone identifier и заменяет его целиком.
  Не искать отдельные UUID/12-digit numbers/project words как cloud IDs.
- ARN разбивать только до resource suffix; не терять следующие `:` или `/`.
  Сохранить case suffix. Wildcard ARN — resource pattern, а не один concrete
  resource; его поддержку назвать отдельно, без ложной identity inference.
- Для Azure поддерживаемые scopes/structure перечислить. Structural keywords
  можно сравнивать case-insensitive; resource name normalization требует
  knowledge конкретного типа. Консервативное сохранение неизвестного case
  может оставить разные labels для variants — это ограничение, а не право
  безусловно lowercase все IDs.
- Google service name нормализуется как host; resource path сохраняет case и
  segment structure. API URL автоматически не переводится в full resource name:
  service/version/endpoints не имеют универсального взаимно-однозначного mapping.
- `//service.googleapis.com/...` обрабатывается раньше generic paths.
  `https://management.azure.com/subscriptions/...` и Google API HTTPS URL
  остаются защищёнными URL spans при выбранном whole-URL контракте. Для
  будущего cloud-URL detector потребуется явное отдельное правило precedence.

## 6. Mapping и проверка на синтетических данных

Canonical identity должна быть versioned и включать тип: `ip/v1`, `host/v1`,
`url/v1`, `path-posix/v1`, `path-windows/v1`, `username/v1`, `cloud-aws/v1`
и т. п. Это предложения namespaces, не названия уже существующего API.
Повторный identical/canonically equivalent объект возвращает прежний label;
original spellings хранятся только в private aliases. Host и whole URL остаются
разными категориями/объектами; URL не обязан иметь label standalone host.

| Сценарий | Ожидаемый контракт |
| --- | --- |
| Public mixed-case URL с encoded path, repeated query keys и fragment | Output bytes идентичны; отдельное изменение query order не склеивает URL identities. |
| Internal URL при URL off, IP/path/host/user on | Весь распознанный URL остаётся целым. |
| Internal URL при URL on | Одна замена полного span; query/path не оставляют чувствительных хвостов. |
| IPv6 compressed/expanded и literal с port | Адрес стабилен между spellings/ports; port сохраняется отдельно. |
| Scoped IPv6 с двумя zones | Полные spans и разные identities; unknown zone grammar не скрывает только часть. |
| `.LOCALHOST.`, `.local`, `.home.arpa`, `.internal` | Canonical host policy; похожий public suffix не совпадает. |
| Explicit company domain/short host и disabled host category | Settings применяются только своей категорией; нет DNS inference. |
| Windows drive/UNC, quoted spaces, POSIX `..`, одинаковое имя с разным case | Полные lexical spans, без filesystem access и ложного case merge. |
| SSH/SCP username, plain email, `user=…` | Узкий context; username policy независима от email/PII. |
| ARN с пустым region/account и qualifier; ARM ID; Google full name | Полный identifier, корректный приоритет перед paths; public API URL защищён. |
| Reorder, scope extension, restart, reset | Прежние mapping labels стабильны; reset не меняет исторические references. |
| Unicode и budget overflow | UTF-8 byte ranges, сохранение bytes вне spans; лимиты дают явную ошибку без partial-clean success. |

Открытие ссылки или существование ресурса не являются условием detection.
Одного corpus недостаточно для обещания полного распознавания всех URL/path
диалектов, cloud сервисов и PII. Registry/документация должны перечислять
реально проверенные forms и категории; pseudonyms не гарантируют анонимность.
