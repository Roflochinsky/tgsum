# Приватный Project mapping: стабильность, версии и reset

Проверено **2026-09-27** для `tgsum-af2.5`. Исходная точка репозитория —
`8791f7f`; документы библиотек сверены с версиями из Cargo.lock. Исследование
использовало исходники репозитория, локальные dependency sources и публичную
первичную документацию. Реальные переписки, аккаунты и credential profiles
не читались; сетевых проверок пользовательских данных не было.

## Решение для реализации

Выбран **приватный сохранённый словарь с читаемыми последовательными labels**
(`PERSON_0001`, `HOST_0001`): immutable generations, независимая random epoch
каждого Project, указатель на текущую generation в новой Project revision.
Уже назначенные labels не перенумеровываются. Новые сущности одного batch
сортируются до назначения labels; расширение scope дополняет mapping.

Random-key HMAC изучен как альтернатива для labels, вычисляемых из identity.
Для выбранного последовательного словаря HMAC/key не нужны: соответствие
хранится непосредственно. Существующий evidence HMAC сохраняет собственное
назначение и не участвует в privacy reset.

Ниже **факты** описывают проверенные библиотеки/текущий код. **Решения TGSUM**
описывают предлагаемый контракт нового среза, не уже выпущенную поддержку.

## 1. Зафиксированные зависимости и существующие границы

| Зависимость | Cargo.lock | Использование / предел |
| --- | --- | --- |
| `hmac` | `0.12.1` | Уже используется для evidence IDs. |
| `sha2` | `0.10.9` | Уже используется для HMAC-SHA-256 и digests файлов. |
| `getrandom` | `0.3.4` у `tgsum-core` | Core требует `0.3`; lock также содержит `0.2.17` и `0.4.3` для других dependency paths. |
| `tempfile` | `3.27.0` | Использует `getrandom 0.4.3`; это не меняет версию прямого Core API `getrandom::fill`. |

Источники версий: [Cargo.lock](../../Cargo.lock),
[core/Cargo.toml](../../core/Cargo.toml).

В исходной revision [EvidenceKey](../../core/src/bundle/files.rs) создаёт
32 random bytes, сохраняет `evidence-key.bin` через `persist_noclobber` и
вычисляет HMAC-SHA-256 над сериализованной tuple `(version, domain, value)`.
Domains `e` и `r` различают identity и revision. Ключ не имеет Debug/Serialize;
bundle содержит opaque references и отдельный приватный evidence index.
[Текущий контракт evidence](../development/bundles.md#stable-evidence).

В исходной revision [ProjectStore](../../core/src/project.rs) использует
schema 3, `expected_revision` и immutable `revisions/{number}.json`. Сначала
синхронизируется временный файл, затем публикуется следующий номер без
перезаписи. Конкурентный победитель оставляет проигравшему conflict. Чтение
использует последнюю опубликованную revision и не скрывает её повреждение
возвратом к старой. [Контракт Project](../development/projects.md).

## 2. Факты о random, HMAC и digest

`getrandom::fill` 0.3.4 заполняет буфер из предпочитаемого системного источника
случайности. При любой ошибке, включая частичное чтение, возвращает error;
содержимое буфера после error не гарантируется. В некоторых условиях вызов
может блокироваться, например при раннем старте системы.
[getrandom 0.3.4 fill](https://docs.rs/getrandom/0.3.4/getrandom/fn.fill.html).

**Решение TGSUM:** использовать успешно заполненные random bytes для opaque
epoch/map IDs. Ошибка RNG завершает операцию; время, process ID, счётчик или
пустой буфер не заменяют random ID. Проверка коллизии при no-clobber всё равно
нужна. Random ID является именем объекта, а не шифровальным ключом или
доказательством защиты данных.

RustCrypto `hmac` поддерживает `Hmac<Sha256>` с `new_from_slice`, `update`,
`finalize`; `verify_slice` предназначен для проверки MAC. `sha2` реализует
SHA-256 и incremental hashing. Эти API сами не предоставляют хранилище,
шифрование или политику псевдонимизации.
[hmac 0.12.1](https://docs.rs/hmac/0.12.1/hmac/),
[sha2 0.10.9](https://docs.rs/sha2/0.10.9/sha2/).

**Решение TGSUM:** SHA-256 в приватном MapRef связывает ссылку с точными bytes
опубликованного файла и позволяет обнаружить случайную подмену/порчу. Это не
authentication от процесса, который может переписать и файл, и ссылку.
Digest plaintext mapping не передаётся в public bundle; наружу при необходимости
выходит только random opaque map ID и версия правил.

### Изученная альтернатива: HMAC-derived labels

RFC 2104 определяет keyed HMAC, рекомендует случайные keys и не рекомендует
ключ короче hash output. Для HMAC-SHA-256 самостоятельный random 32-byte key
соответствует этому выбору размера. Исторические примеры алгоритмов из RFC
не являются рекомендацией заменить используемый SHA-256.
[RFC 2104 §3](https://www.rfc-editor.org/rfc/rfc2104.html#section-3).

Если в будущем labels будут вычисляться прямо из identity, проектное решение
может использовать независимый key на Project/epoch и однозначное сообщение
`(algorithm_version, category, identity_namespace, canonical_identity)`.
Нельзя конкатенировать неоднозначные строки без framing или выводить key из
названия Project/даты. Domain separation — часть формата TGSUM. Для сравнения,
RFC 5869 объясняет context binding через `info` в HKDF; это другой алгоритм,
и tuple/HMAC TGSUM не следует называть HKDF.
[RFC 5869 §3.2](https://www.rfc-editor.org/rfc/rfc5869.html#section-3.2).

Такой key должен быть независим от `evidence-key.bin`, чтобы privacy reset
не менял message evidence. Он не зашифрует private mapping. Утрата или порча
существующего key потребует явной ошибки/восстановления, а не скрытой генерации
замены. **Выбранный для `af2.5` последовательный словарь этого key не вводит.**

## 3. Факты о публикации файлов

`NamedTempFile::persist_noclobber` публикует файл только если target отсутствует
и не перезаписывает существующий target. Перенос между filesystems не
поддерживается. Документация явно не обещает atomicity на всех платформах:
может остаться первоначальный hard link на temporary file. При ошибке API
возвращает temporary file в `PersistError`. Публикация использует путь, поэтому
документация отдельно оговаривает риски вмешательства в temporary location.
[tempfile 3.27.0 NamedTempFile](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist_noclobber).

В Unix implementation 3.27.0 используется no-replace rename, где это
поддерживается; fallback создаёт hard link и удаляет старое имя. Ошибка удаления
старого имени игнорируется. Named temporary files создаются через `create_new`
с Unix mode `0600`, если не заданы другие permissions.
[tempfile 3.27.0 Unix source](https://docs.rs/tempfile/3.27.0/src/tempfile/file/imp/unix.rs.html).

`File::sync_all` пытается синхронизировать содержимое и metadata файла.
Ошибки закрытия файла через Drop игнорируются, поэтому явная синхронизация
позволяет обработать часть I/O ошибок до завершения операции.
[Rust File::sync_all](https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all).

**Решение TGSUM:** staging находится внутри приватного destination directory;
после сериализации выполнить flush, file sync, no-clobber publication, затем
поддерживаемую синхронизацию directory. Не писать в опубликованный handle.
Это не multi-file transaction и не универсальная гарантия сохранности после
power loss. Тест успешного повторного открытия не доказывает поведение диска
при отключении питания. Защита от другого процесса того же пользователя и
полная устойчивость к filesystem races не заявляются.

## 4. Контракт mapping и Project CAS

Это решения для реализации:

1. **Identity.** Entry key — `(category, canonical_identity)` с явно
   определённой нормализацией/namespace. Для участника canonical identity
   должна включать платформу/аккаунт и stable sender ID, где он доступен.
   Display name становится alias; переименование участника не создаёт другую
   личность. Совпавшие имена разных sender IDs автоматически не объединяются.
2. **Label.** Каждая entry получает один читаемый `TYPE_0001`. После выдачи
   label сохраняется при перестановке сообщений, повторном импорте, удалении
   части scope и добавлении источников. Только новые entries получают следующие
   значения. Сортировка новых entries делает один batch независимым от порядка
   обнаружения; старые entries не пересортировываются ради новой нумерации.
3. **Независимость.** Новый Project начинает собственную epoch и пустой mapping.
   Одинаковая надпись `PERSON_0001` в разных Projects не означает общую identity;
   полная ссылка включает Project/epoch. Mapping нельзя переносить между
   Projects лишь по совпадению label, display name или номера generation.
4. **Версии.** Schema mapping, normalization version и opaque epoch являются
   проверяемыми полями. Изменение нормализации не должно незаметно объединить
   или разделить прежние entries. Неизвестные версии явно отвергаются.
5. **Generation.** Новая immutable generation хранит полный словарь с
   добавленными entries/aliases. Приватный MapRef фиксирует random ID, epoch,
   generation и SHA-256 bytes. Ограничить общий размер, число entries/aliases,
   длины identity и counters; overflow выдаёт ошибку до публикации partial map.
6. **Commit.** Сначала полностью опубликовать generation, затем CAS Project
   revision с новым MapRef. Именно Project pointer определяет current mapping;
   «самый большой generation-файл в directory» не выбирает current.
7. **Schema Project.** Добавить ссылку в schema 4 с чтением старых Projects
   как `mapping: None`. Сохранение в schema 3 позволило бы старому reader
   проигнорировать новое поле и потерять его при следующей записи.
8. **История.** Private bundle/result provenance фиксирует конкретный MapRef.
   Исторический результат разрешает свои labels по прежней generation,
   даже если Project теперь использует другую. Legacy bundle без mapping ref
   остаётся legacy; для него нельзя изобретать привязку к текущему mapping.

### Ошибки и reset

| Событие | Требуемое наблюдаемое состояние |
| --- | --- |
| Serialize/size/RNG/write error до publication | Current Project pointer остаётся прежним; partial map не используется. |
| Generation опубликована, Project CAS проигран | Conflict и прежний/победивший pointer; staged generation может остаться недостижимой. Её наличие не подтверждает commit. |
| Ошибка directory sync после успешной публикации revision | Revision уже может быть видна. Перечитать authoritative state; не обещать, что любой `Err` означает rollback. |
| Current ref указывает на отсутствующий, повреждённый или чужой map | Явная ошибка; не fallback на другой map и не тихий reset. |
| Reset | Полностью опубликовать пустую новую epoch, затем переключить Project через CAS; прежние map files и refs остаются. |
| Старый Review после reset | Revision mismatch требует новой подготовки/Review; старый готовый результат сохраняет свою provenance. |

Новый epoch reset меняет namespace будущих labels, но не переписывает уже
экспортированный Markdown, чужие копии или прошлые результаты. Это не удаление
истории. Удаление mapping/Project — отдельная retention operation; backup и
посторонние копии требуют самостоятельного учёта. Автоматический GC не должен
удалять generation, пока на неё ссылается сохранённый bundle/result.

## 5. Приватность и проверяемые границы

Private mapping содержит исходные identities/aliases и хранится вне agent
workspace. На Unix новые directories должны быть `0700`, файлы `0600`; на
других ОС нужны действующие access controls application-data directory. Эти
ограничения доступа не являются encryption at rest. Упрощение текста и
стабильные labels не обещают анонимность: оставшийся контекст, частоты и связи
могут позволить узнать человека/систему. Privacy reset не устраняет такие связи
в уже переданных материалах.

В public DTO/bundle/logs не включать entries, canonical values, aliases, private
paths и plaintext mapping digest. Error messages не должны печатать проблемный
identity целиком. No-findings в secrets scanner и псевдонимизация — отдельные
свойства; одно не доказывает другое.

## 6. Синтетические acceptance scenarios

- Один Project: повтор после рестарта, перестановка сообщений и расширение scope
  сохраняют все прежние labels; новый batch получает уникальные новые labels.
- Alias rename при том же sender ID сохраняет label; совпавшее имя другого
  sender ID не объединяет записи. Category/namespace разделяют одинаковые bytes.
- Два Projects не читают mapping друг друга; чужой MapRef явно отвергается.
- Два concurrent writers с одной expected revision: один commit, один conflict;
  generation проигравшего не становится current после повторного открытия.
- Reset создаёт новую epoch, оставляет старую generation доступной для старого
  private ref и инвалидирует прежний Review без изменения evidence IDs.
- Missing/corrupt map, неправильные identity/version/digest, duplicate labels,
  symlink locations и исчерпанные budgets дают явную ошибку, без fallback.
- Экспорт public bundle и сериализуемые ответы проверяются на отсутствие
  синтетических originals/aliases, map payload и private digest. Оpaque map ID
  допустим только в том public поле, которое заявляет контракт.

Эти сценарии проверяют поведение API и сохраняемых artifacts. Проверка текста
этой записки не заменяет исполнение тестов реализации; crash/power-loss
durability и поддержка иной ОС не выводятся из обычного Linux test run.
