# Участники и PII: identity, границы распознавания и fixtures

Проверено **2026-09-27** для `tgsum-af2.3`; исходная revision TGSUM —
`46736e8`. Первичные источники: официальный Telegram Desktop source, Unicode,
RFC, ITU и документация библиотек. Реальные переписки, аккаунты, профили,
DNS, SMTP и телефонная верификация не использовались.

## Краткое решение

Это **предлагаемый контракт TGSUM**, а не гарантия полноты распознавания PII:

- PERSON строится по scoped native sender ID. Display name — alias,
  а не доказательство личности. Одинаковые имена не объединяют участников.
- Отдельные переключатели: participants, usernames, emails, phones. Их выключение не
  выключает независимый secrets scanner.
- Alias matching по умолчанию exact и с проверкой границ; без NER, склонений,
  transliteration, case folding и поиска личности во внешних сервисах.
- EMAIL сохраняет local-part без изменения регистра; domain получает отдельную
  явную нормализацию. PHONE распознаёт ограниченные международные формы с `+`.
- Все изменения работают с исходными UTF-8 spans. Нераспознанный текст не
  переписывается ради normalization. Private mappings остаются вне agent bundle.

## 1. Telegram Desktop: что действительно записано в JSON

Прочитан официальный `tdesktop` commit
[`6154cbe9fffa932690c457c7cff2c93a29d48553`](https://github.com/telegramdesktop/tdesktop/commit/6154cbe9fffa932690c457c7cff2c93a29d48553).
Это наблюдение конкретного producer, не универсальная schema всех Telegram API.

**Подтверждённые факты:**

- Writer сериализует peer IDs строками `userN`, `chatN`, `channelN`, сохраняя тип.
  `pushFrom()` одновременно пишет display name и `<label>_id`; `pushActor()`
  вызывает тот же helper с label `actor`.
  [Typed IDs и paired fields](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/output/export_output_json.cpp#L1573).
- Обычное сообщение использует `from`; service actions обычно используют
  `actor`. Однако `proximity_reached` записывает `from/from_id` и `to/to_id`.
  Правило «service всегда actor» неверно.
  [Service exception](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/output/export_output_json.cpp#L1814),
  [ordinary message](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/output/export_output_json.cpp#L2061).
- В `text_entities` entity называется **`mention_name`**, с числовым `user_id`.
  `mention` содержит текст, без целевого ID; `text_mention` — не имя этого
  Desktop JSON entity. `plain`, `email`, `phone`, `text_link` имеют отдельные types.
  [SerializeText](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/output/export_output_json.cpp#L166).
- Display name берётся из peers данного export slice; user name составлен из
  first/last name, chat name — из title. Writer не восстанавливает историю имён.
  [Peer lookup](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/output/export_output_json.cpp#L1529),
  [ContactInfo и Peer::name](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/data/export_data_types.cpp#L2130).

**Локальный аудит:** [RawMessage](../../core/src/model.rs) сохраняет sender fields,
но [deserializer](../../core/src/de.rs) сворачивает entities в текст.
[CanonicalMessage](../../core/src/snapshot.rs) не хранит entity type/target ID.
Поэтому первая реализация может опираться на sender ID/name; произвольное
`@username` или flattened mention нельзя объявлять разрешённым в native identity.
Это требует отдельного сохранения structured entities и проверки совместимости.

В исходной [Telegram normalization](../../core/src/snapshot/telegram.rs) независимые
`from_id.or(actor_id)` и `from.or(actor)` способны создать гибридную пару.
**Выбранное исправление:** если присутствует хотя бы одно `from`-поле, брать
только пару `from/from_id`; иначе только `actor/actor_id`. Missing поле остаётся
missing. Это defensive правило для malformed/mixed input, а не предположение,
что официальный writer должен выводить такие гибридные записи.

## 2. Participant identity и неоднозначные aliases

**Решения TGSUM:**

1. Identity кодировать однозначной tuple: platform, account_local_id, typed
   sender_id. Полный SourceScope, включая conversation, допустим как более
   строгая изоляция; выбранную границу нужно закрепить тестом. Не объединять
   людей между platform/account namespaces; не убирать `user`/`channel` prefix.
2. Header получает PERSON непосредственно по sender ID, даже если display name
   отсутствует. Name без ID — unresolved identity; его нельзя автоматически
   объединить с одноимённым известным участником.
3. Наблюдённое новое имя того же ID добавляется как private alias и сохраняет
   label. Не считать экспорт доказательством всех прошлых имён. Channel sender,
   bot и пользователь не доказывают наличие отдельного физического человека.
4. Для body substitutions строить alias → set of identities в релевантном
   conversation scope, включая сохранённые aliases. Если set содержит несколько
   людей, не выбирать одного по порядку, текущему автору или фамилии: generic
   ambiguous redaction либо unresolved finding с явным ограничением покрытия.
5. До body replacements учесть весь выбранный batch, включая новых участников
   и aliases. Расширение scope может сделать ранее уникальное имя неоднозначным;
   пересчитать resolver, сохранив уже выданные PERSON labels и старые bundles.
6. Не выводить aliases из отдельных частей полного имени. `Иван Петров` не
   создаёт автоматически aliases `Иван`, `Петров`, `Ваня` или `Ivan Petrov`.
   Короткие/common-word names в прозе неоднозначны даже при одном sender ID;
   metadata substitution имеет более сильное основание, чем body matching.

Private generations/version/reset уже описаны в
[mapping research](project-pseudonym-mapping-2026-09-27.md) и
[mapping contract](../development/pseudonyms.md).

### Username — отдельная сущность

Telegram документирует ASCII letters, digits и underscore, с регистронезависимым
сравнением username. Basic usernames и collectible usernames имеют разные
ограничения длины; collectible username может передаваться другому владельцу.
[Telegram FAQ](https://telegram.org/faq#q-what-can-i-use-as-my-username),
[collectible usernames](https://telegram.org/faq#q-what-are-collectible-usernames-how-are-they-different-from-basic-usernames),
[официальный checkUsername](https://core.telegram.org/method/account.checkUsername).
API не вызывался; прочитана только документация.

**Решение TGSUM:** явный `@handle` можно заменять отдельным USER label с private
identity platform/account + handle. Для Telegram ASCII case folding обоснован
источником; на другие платформы это правило не переносится автоматически.
Не извлекать username из email, SSH или произвольного слова и не связывать его
с PERSON по похожему имени. Такой label обозначает handle, не неизменного
владельца. Необязательное разрешение `mention_name/user_id` возможно только после
сохранения structured entity. Пределы длины detector — явно выбранная граница
покрытия, а не проверка существования аккаунта.

## 3. Unicode: эквивалентный текст не означает одну личность

**Факты:** NFC объединяет канонически эквивалентные последовательности;
NFKC дополнительно стирает compatibility distinctions. Unicode предостерегает
от слепого NFKC для произвольного текста.
[UAX #15, revision 58, 2026-08-12](https://www.unicode.org/reports/tr15/tr15-58.html#Normalization_Forms).
Case folding предназначен для caseless matching, не равен lowercase и сам по
себе не сохраняет normalization form.
[Unicode 18, §3.13.3](https://www.unicode.org/versions/Unicode18.0.0/core-spec/chapter-3/#G34092).

Default word boundaries требуют правил для combining/format characters;
надёжная сегментация ряда языков требует dictionary/locale tailoring.
[UAX #29, revision 49, 2026-09-01](https://www.unicode.org/reports/tr29/tr29-49.html#Word_Boundaries).
В имеющемся `regex 1.13.1`, `\w` включает Alphabetic, marks, decimal digits,
connector punctuation и Join_Control; `\d` также Unicode, не только `[0-9]`.
[regex Unicode classes](https://docs.rs/regex/1.13.1/regex/#perl-character-classes-unicode-friendly).

**Решения TGSUM:** exact matching сохраняет `Alex`/`alex`, Latin `A`/Cyrillic `А`
раздельно. Не использовать `char::is_alphanumeric()` как полную проверку границы:
combining marks могут продолжать имя. Unicode `\b` — полезный bounded guard,
но не NER и не полный UAX #29 word segmenter. На пунктуационных/emoji aliases
нужна явная проверка соседних Unicode word characters; не ставить `\b` механически
с двух сторон любого alias. Длинные aliases проверять раньше вложенных коротких.

Если позже включается NFC alias lookup, сохранять native ID главным ключом,
считать collisions неоднозначными и иметь соответствие normalized positions
исходным UTF-8 spans. Нельзя применять offsets normalized строки к оригиналу.
Для первого exact режима новая Unicode normalization dependency не требуется.

## 4. Email: identity и ограниченный detector

**Факты:** SMTP сохраняет регистр local-part; domain регистронезависим.
Semantics local-part определяет принимающий host.
[RFC 5321 §§2.3.11, 2.4](https://www.rfc-editor.org/rfc/rfc5321.html#section-2.4).
RFC 5322 допускает dot-atom, quoted local-part, comments/folding и domain literals:
обычная regex для `name@host.tld` не является полным mailbox parser.
[RFC 5322 §3.4.1](https://www.rfc-editor.org/rfc/rfc5322.html#section-3.4.1).
SMTPUTF8 расширяет local-part до non-ASCII и разрешает U-label domain.
[RFC 6531 §3.3](https://www.rfc-editor.org/rfc/rfc6531.html#section-3.3).
Рекомендации normalization адресованы в том числе владельцам delivery server;
они не дают стороннему scanner права склеить произвольные local-parts.
[RFC 6530 §10.1](https://www.rfc-editor.org/rfc/rfc6530.html#section-10.1).
IDNA label equivalence определяется через A-label и case-insensitive comparison.
[RFC 5890 §2.3.2.4](https://www.rfc-editor.org/rfc/rfc5890.html#section-2.3.2.4).

**Решения TGSUM:**

- Начальный detector: явно ограниченный dot-atom local-part + domain с dotted
  labels. Не принимать consecutive/leading/trailing local dots, domain underscore,
  пустые labels или неполный суффикс malformed candidate. Исключение quoted
  addresses/domain literals допустимо только как документированный coverage gap.
- Identity: exact local-part + canonical domain; `Alice@EXAMPLE.com` совпадает
  с `Alice@example.com`, но не с `alice@example.com`. Не убирать `+tag` и точки.
- IDNA преобразовать только domain. `idna 1.1.0` уже есть в lockfile транзитивно,
  `url 2.5.8` — прямая dependency core. WHATWG host parsing шире email domain
  grammar; добавить явную проверку допустимых DNS labels, не подставлять mailbox
  целиком в URL parser. `domain_to_ascii_strict` имеет более строгие ограничения
  и не обещает принятие всех реально используемых имён.
  [idna strict API](https://docs.rs/idna/1.1.0/idna/fn.domain_to_ascii_strict.html).
- Полнота SMTPUTF8 отдельно тестируется/заявляется. Если поддержан только ASCII
  local-part, нельзя извлекать ASCII-хвост из `кириллицаalice@example.test`.
- Email не связывает PERSON автоматически. Email shared inbox и display name
  не доказывают одного человека. SSH `user@host:/path` — отдельный контейнер.
- Политика URL spans должна быть явной: standalone PII detector не разрушает
  public URL, уже защищённый infrastructure scanner. `mailto:` и PII в URL query
  требуют своего контракта; отсутствие их обработки — известный scope limit.

## 5. Phones: international-only без угадывания страны

**Факты:** ITU-T E.164 (2026-02-13), §6.1 рекомендует максимум 15 digits для
международных номеров, без international prefix; §6.2 отделяет prefixes/suffixes
от numbering plan.
[E.164, edition 7, PDF p.12](https://www.itu.int/rec/dologin_pub.asp?id=T-REC-E.164-202602-I%21%21PDF-E&lang=e&type=items).
RFC 3966 обозначает global number через `+`; local number требует context.
Для `tel:` visual separators не влияют на сравнение, но extension — отдельный
значимый parameter. Пробелы допустимы в печатном представлении, не в tel URI.
[RFC 3966 §§4–5](https://www.rfc-editor.org/rfc/rfc3966.html#section-4).

libphonenumber различает length-based possibility и validity по metadata.
Даже validity не доказывает, что номер назначен человеку и доступен. `00` не
эквивалентен `+` во всех регионах; parser также умеет исправлять/преобразовывать
часть буквенных вводов, что не требуется TGSUM detector.
[Library README](https://github.com/google/libphonenumber#highlights-of-functionality),
[official FAQ](https://github.com/google/libphonenumber/blob/master/FAQ.md#parsing).

**Решения TGSUM:** только `+`, первая ASCII digit `[1-9]`, 8–15 ASCII digits
всего, ограниченный набор presentation separators (например, spaces/hyphens и
balanced parentheses). Нижняя граница 8 и whitelist separators — эвристики
низкого false-positive rate, не универсальные требования E.164. Не угадывать
locale, trunk prefix, `00`, национальную восьмёрку, vanity letters или Unicode
digits; не добавлять libphonenumber без потребности в расширенном покрытии.

Identity — `+` и digits после удаления только разрешённых separators.
Extensions либо сохраняются в identity и скрываются вместе с номером, либо
явно остаются неподдержанными; нельзя silently объединить два добавочных номера.
Candidate не пересекает newline, timestamp, token/URL span или adjoining word
characters. Слишком длинный numeric run отвергается целиком, не превращается
в допустимый 15-digit префикс. Shape recognition не подтверждает валидность,
принадлежность или существование номера; никаких звонков/SMS/network checks.

## 6. Детерминированные synthetic fixtures

| Срез | Проверка |
| --- | --- |
| Sender pairs | `from=A, actor_id=user2` не даёт `A/user2`; полная actor pair работает; service proximity использует from |
| Typed/scoped IDs | `user7`, `channel7`, другой account/platform не объединяются; ID больше 2^53 остаётся точным |
| Rename | user7: `Иван Петров` → `Иван Сидоров` сохраняет label; alias остаётся private |
| Ambiguity | два разных ID с `Alex`; reverse input order не меняет результат; новый duplicate alias не выбирает первый ID |
| Missing identity | display name без sender ID не присоединяется к одноимённому PERSON |
| Mention limits | mention_name/user_id после flatten не считается разрешённой identity; обычный @name не связывается по догадке |
| Usernames | Telegram `@Example`/`@example` имеют один handle label; другой platform/account изолирован; email/SSH и display name не создают PERSON association |
| Unicode | `é` vs `e\u0301`, Latin/Cyrillic lookalikes, `Straße`/`STRASSE`, combining mark справа от alias |
| Boundaries | `Ann` не заменяет `Anna`; `Иван` не заменяет `Иванов`; emoji/name punctuation и CJK adjacency не ломают UTF-8 |
| Email identity | `Alice@EXAMPLE.test` == `Alice@example.test`; `alice` и `Alice` различны; `a+tag` и `a` различны |
| Email scope | plus/dot local; IDN/A-label domain pair; quoted local и SMTPUTF8 получают заявленное поведение без partial matches |
| Phones | `+1 202-555-0100` == `+12025550100`; 16 digits, `2026-09-27`, `v1.2.3`, bare IDs и национальный номер не matches |
| Numeric adjacency | `+12345678 90 123456` не обрезается до prefix; `id+12345678x`, `+00…`, multiline и malformed parentheses отвергаются |
| Integration | category off сохраняет bytes; secrets внутри disabled PII всё ещё скрываются; counts без raw values |
| Repeatability | reorder/scope extension сохраняют существующие labels; old mapping refs и bundles не переписываются |

Эти проверки доказывают конкретные поддержанные формы. Они не доказывают
универсальную анонимизацию, распознавание ФИО/адресов/банковских реквизитов,
всех контактов в attachments, исторических имён или entity targets после flatten.
