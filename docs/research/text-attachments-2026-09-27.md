# Выбранные текстовые вложения: формат и файловая граница

Проверено: 2026-09-27. Scope: `tgsum-af2.6`, локальные файлы из уже
импортированного Telegram JSON, Windows/macOS/Linux. Это research перед
реализацией по [connector review](../connectors/review-policy.md), а не
подтверждение готовой поддержки. Реальные аккаунты, профили и клиентские
экспорты не использовались. Загружались только публичные исходники/документация.

## Решение для реализации

Рекомендация: выделить небольшой packager, получающий выбранные attachment IDs,
явно заданный export root и лимиты. Использовать `cap-std` + `cap-fs-ext` 4.0.3:
открытый directory handle, проход по одному компоненту без следования symlink,
открытие конечного файла без следования symlink, проверка типа на handle,
ограниченное чтение, sanitization и новая независимая копия. Это инженерный
вывод из источников ниже; конкретный lock/build ещё должен пройти Rust 1.88.

Не использовать `canonicalize → starts_with(root) → File::open` как security
boundary. Не использовать hardlink к изменяемому входу. Форматы в allowlist
определяют допустимое содержимое, но не разрешают исполнение файлов, раскрытие
архивов, HTTP-загрузку или загрузку конфигурации агента из исходного имени.

## 1. Telegram Desktop: что лежит в JSON

Официальный `dev` проверен через `git ls-remote`: commit
[`6154cbe9fffa932690c457c7cff2c93a29d48553`](https://github.com/telegramdesktop/tdesktop/commit/6154cbe9fffa932690c457c7cff2c93a29d48553).
GitHub REST commit endpoint вернул rate-limit 403; pin установлен Git refs,
а файлы прочитаны по immutable raw URL. Это версия исходников, не обещание
идентичного формата всех выпущенных клиентов.

| Поле обычного сообщения | Значение |
| --- | --- |
| `file` | `Data::File.relativePath` либо строка о недоступности |
| `file_name` | Исходное имя документа; не путь для открытия |
| `file_size` | Размер документа из metadata |
| `mime_type` | MIME из metadata, может отсутствовать |
| `photo` | Относительный путь фотографии либо строка о недоступности |
| `photo_file_size` | Размер фотографии; имя поля отличается от `photo_size` |
| `media_type` | Дополнительный вид media; обычный документ может обходиться без него |

В `file` встречаются человеческие сообщения о недоступности, исключении
по размеру или настройкам типов. Их нельзя открывать как файлы. Rich writer
имеет также отдельные `file_skip_reason` / `photo_skip_reason`, включая
`date_limits`, с отсутствующим path. Rich/nested attachments требуют отдельного
подтверждения parser coverage.
[Обычный writer](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/output/export_output_json.cpp#L1642),
[document metadata](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/output/export_output_json.cpp#L2091),
[rich availability](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/output/export_output_json.cpp#L263).

Downloader создаёт файл как `_settings->path + relativePath`; JSON writer
использует тот же export path для `result.json`. Следовательно, относительный
attachment path разрешается от корня экспорта, а не от каталога отдельного чата.
Перенос одного JSON без его файлов не делает вложения доступными.
[prepareFileProcess](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/export_api_wrap.cpp#L3398),
[mainFileRelativePath](https://github.com/telegramdesktop/tdesktop/blob/6154cbe9fffa932690c457c7cff2c93a29d48553/Telegram/SourceFiles/export/output/export_output_json.cpp#L3051).

Локальное наблюдение до реализации: `core/src/snapshot/telegram.rs::Record`
сохраняет `file_size`, но читает `photo_size`. Это конкретный gap относительно
проверенного writer. Existing `safe_relative_path` уже исключает строки,
начинающиеся с `(`, traversal, backslash, colon и control characters;
этого недостаточно для безопасного открытия файла на диске.

## 2. Почему проверка пути перед открытием недостаточна

`std::fs::canonicalize` разрешает symlinks и возвращает новый абсолютный путь.
Последующее открытие выполняет новое разрешение pathname: между этими
операциями другой процесс может заменить parent либо leaf. Аналогичная гонка
есть у `symlink_metadata(path)` перед `File::open(path)`. Это вывод из
раздельных операций; `canonicalize` не выдаёт capability на проверенный inode.
На Windows результат canonicalize использует extended-length syntax.
[Rust canonicalize](https://doc.rust-lang.org/std/fs/fn.canonicalize.html).

`Path::components()` нормализует повторные separators и часть `.`; при этом
Unix и Windows имеют разные правила separators/prefixes. Поэтому отклонять
неподдерживаемые сырые archive paths нужно **до** преобразования в `Path`,
иначе нормализация скроет запрещённую форму. Не выполнять URL percent-decoding,
Unicode normalization или case folding исходного имени при открытии.
[Rust Path](https://doc.rust-lang.org/std/path/struct.Path.html#method.components).

Для переносимой archive reference policy разумно оставить `/` единственным
разделителем, разрешить непустые обычные компоненты и запретить `.`, `..`,
NUL/control, backslash, colon, абсолютные paths, drive prefixes, UNC и device
namespace. Дополнительно отклонять trailing space/dot и Windows reserved device
names, включая имена с расширением. Это выбранная более узкая политика TGSUM,
а не утверждение, что такие имена невозможны на Unix. Colon опасен в том числе
из-за alternate data streams; `C:name` является drive-relative, не обычным
относительным именем.
[Microsoft naming rules](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).

## 3. Предлагаемая dependency и платформенные детали

Проверен tag `v4.0.3`, peeled commit
[`b7acf8e8807fe3fab991884d2208b7e03d35a409`](https://github.com/bytecodealliance/cap-std/commit/b7acf8e8807fe3fab991884d2208b7e03d35a409).
README заявляет Rust 1.70 для default features и поддержку Linux/macOS/Windows.
Workspace manifest также содержит `rust-version = "1.70"`; отдельные package
manifests не задают собственный `rust-version`. Это пригодная исходная база
для проекта с MSRV 1.88, но не результат проверки текущего dependency resolver.
[Pinned README](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/README.md#minimum-supported-rust-version-msrv),
[workspace manifest](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/Cargo.toml#L15).

Локальный `Cargo.lock` до изменений содержит `rustix 1.1.5`, `libc 0.2.189`,
несколько `windows-sys`, в том числе 0.61.2. `cap-std` / `cap-fs-ext` отсутствуют.
`core/Cargo.toml` не имеет прямых зависимостей от этих OS crates; workspace
запрещает собственный unsafe. Проверенный local manifest rustix 1.1.5 указывает
MSRV 1.65. Наличие transitive dependency не делает её публичным API core.

У cap 4.0.3 есть новые transitive crates, включая `io-lifetimes 3`, `io-extras`,
`cap-primitives`; Windows branch использует `winx` и `windows-sys >=0.60,<0.62`.
Поэтому это небольшой application interface, но не изменение с нулевой
стоимостью dependency. Альтернатива — собственный Unix `openat` walker на
rustix плюс отдельная Windows handle-relative реализация; её объём и audit
surface существенно больше. Это инженерная оценка, не измеренный benchmark.
[cap-std manifest](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-std/Cargo.toml),
[cap-primitives manifest](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-primitives/Cargo.toml).

### Handle-relative walk

`cap_std::fs::Dir` ограничивает разрешение путей своим деревом, но обычный
`Dir::open` допускает symlinks, которые остаются внутри этого дерева. Для
политики TGSUM «вложенные symlinks не читаем» этого недостаточно.
`DirExt::open_dir_nofollow` и `OpenOptionsFollowExt::follow(No)` запрещают
symlink в **последнем** компоненте. Передавать по одному проверенному компоненту:

1. Держать root handle до окончания чтения выбранных вложений.
2. Каждый промежуточный компонент открыть через `open_dir_nofollow(component)`.
3. Leaf открыть через `open_with(component, read + follow(No) + nonblock)`.
4. Проверить metadata открытого handle, затем читать из этого же handle.

Это вывод для нашей более строгой политики из
[DirExt](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-fs-ext/src/dir_ext.rs#L85),
[follow extension](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-fs-ext/src/open_options_follow_ext.rs),
[open directory implementation](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-primitives/src/fs/open_dir.rs#L30).

На Windows cap использует `NtCreateFile` с `RootDirectory` handle;
`FollowSymlinks::No` устанавливает `FILE_FLAG_OPEN_REPARSE_POINT`. Проверка
cap на отказ symlink использует `file_type().is_symlink()`. Reparse points
шире symlinks: для нашей политики проверять `FILE_ATTRIBUTE_REPARSE_POINT`
на metadata каждого открытого parent и leaf, до descent/read. Не обещать
квалификацию произвольных filesystem filter drivers по чтению исходников.
[Windows relative open](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-primitives/src/windows/fs/create_file_at_w.rs#L155),
[flags](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-primitives/src/windows/fs/oflags.rs#L16),
[post-open check](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-primitives/src/windows/fs/open_unchecked.rs#L150),
[Microsoft reparse behavior](https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-points-and-file-operations).

### Как открыть сам root

`Dir::open_ambient_dir(path, ambient_authority())` явно unsandboxed. Если
передать весь сохранённый root, его родители могут разрешаться через symlinks.
Root trust нельзя незаметно получить от capability API.
[Ambient root implementation](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-primitives/src/fs/open_dir.rs#L49).

Рекомендованный строгий вариант TGSUM:

- Unix: открыть ambient только `/`, затем пройти абсолютный выбранный root
  по обычным компонентам с `open_dir_nofollow`.
- Windows: открыть ambient только явно выбранный local drive root, затем
  аналогичный проход с дополнительным reparse-bit check. Archive references
  никогда не задают drive/prefix. Поддержку UNC/device roots не включать
  случайно через generic parser prefix.
- Если пользовательский root проходит через известный OS symlink, например
  macOS `/tmp`, можно разрешить его при выборе root и сохранить показанный
  пользователю canonical target. Последующее чтение всё равно выполняет
  nofollow walk; canonicalization здесь разрешает выбор, не защищает чтение.
- После restart сохранённый путь обозначает **нынешний каталог по этому пути**.
  Сам pathname не доказывает, что это прежний directory object. Для отдельного
  требования постоянной identity понадобятся persisted device/inode либо
  Windows file identity и обработка перемещения/замены; данное исследование
  не объявляет такое pinning уже реализованным.

Handle защищает от перенаправления через подменённый symlink, но не запрещает
владельцу файловой системы переименовать уже открытый каталог либо подменить
обычный файл до открытия. Это object-capability boundary, не замораживание
всего дерева. Mount points и привилегированный доступ к filesystem также не
устраняются lexical nofollow политикой. Не обещать отсутствие OS-managed
network I/O на сетевом/виртуальном mount: packager не делает HTTP-загрузку,
но не управляет реализацией файловой системы.

### Не зависать на FIFO и не читать устройства

`OpenOptionsSyncExt::nonblock(true)` передаётся в Unix `O_NONBLOCK`.
Открытый handle затем должен иметь regular-file type; FIFO, socket,
directory и device отклоняются до чтения содержимого. Не ограничиваться
предварительным `metadata(path)`: final pathname может измениться.
[nonblock extension](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-fs-ext/src/open_options_sync_ext.rs),
[Unix flags](https://github.com/bytecodealliance/cap-std/blob/b7acf8e8807fe3fab991884d2208b7e03d35a409/cap-primitives/src/rustix/fs/oflags.rs#L39).

Оговорка: nonblock обычно не делает regular-file disk I/O неблокирующим;
Windows flags implementation не переводит его в asynchronous I/O. Открытие
произвольного устройства само по себе может иметь специфические эффекты,
поэтому не заявлять доказанное отсутствие любых device-open side effects.
Для рассматриваемых user-owned export folders нужны отказ специальных типов,
Unix FIFO regression и Windows reserved-name/reparse regression. Timeout
не должен интерпретироваться как успешная проверка.

## 4. Независимая копия и изменяемые bytes

Rust прямо предупреждает: наличие `File` handle не исключает изменение
содержимого другими процессами. Проверка размера/mtime до и после чтения
обнаруживает некоторые изменения, но не доказывает атомарный snapshot;
same-size overwrite и восстановленные timestamps могут остаться незаметны.
Стандартные `File::lock*` стабилизированы в Rust 1.89, выше MSRV проекта,
и семантика взаимодействия locks с обычными reads/writes зависит от ОС.
Не использовать lock как универсальное доказательство immutable input.
[Rust File](https://doc.rust-lang.org/std/fs/struct.File.html).

Рекомендованный контракт:

- Считать только ограниченное количество bytes из одного проверенного handle.
  Metadata size — ранний фильтр, фактический read budget — обязательный лимит.
  Читать максимум `limit + 1`, чтобы отличить ровно допустимый файл от
  превышения; все арифметические операции budget выполнять с overflow check.
  [Read::take](https://doc.rust-lang.org/std/io/trait.Read.html#method.take).
- Вычислять content digest по реально прочитанным bytes. Если metadata
  изменилось, можно отклонить чтение как unstable; успех такой проверки
  всё равно не является atomic-snapshot guarantee.
- Санитизировать bytes локально, создать новый файл с генерируемым именем
  в private staging, затем включить его в проверяемый bundle. После публикации
  изменения источника не меняют ранее подготовленную копию.
- Hardlink указывает на тот же underlying file и не даёт независимости.
  Копировать bytes из открытого handle, не делать `hard_link(source, target)`.
  [Rust hard_link](https://doc.rust-lang.org/std/fs/fn.hard_link.html).
- Исходный regular file сам может иметь дополнительные hardlinks. Nofollow
  их не выявляет и не доказывает уникальное владение source bytes. Главное
  обязательство этого среза — отсутствие hardlink между input и output.

Данные могут быть валидным UTF-8, но оставаться бинарными по назначению.
Предлагаемая product policy: allowlist расширений + ограниченное полное
UTF-8 decoding без replacement characters + явный отказ NUL и недопустимых
control bytes. MIME и extension — metadata, не доказательство типа.
Без charset guessing, запуска `.sh`/`.ps1`, JSON parsing как команды,
remote fetch и archive extraction. Точный control allowlist/BOM behavior
должен стать явным контрактом реализации и fixtures.

## 5. Синтетические acceptance fixtures

Это предложенная матрица для `af2.6` и отдельного adversarial `af2.9`, а не
список уже пройденных тестов. Задачи и статус остаются в Beads.

| Группа | Проверка наблюдаемого результата |
| --- | --- |
| Формат | `file`/`file_name` различаются; nested relative path; `photo_file_size`; missing/type/size placeholders видны и не открываются |
| Выбор | Читаются только выбранные attachments выбранных сообщений; одинаковые basenames не перезаписываются; дубли не обходят budgets |
| Paths | `../`, `a/../b`, абсолютный, empty/`.` component, `a//b`, NUL/control, backslash, `C:x`, `C:/x`, UNC, device namespace, ADS, trailing dot/space, reserved names отклоняются |
| Symlink | Final и parent symlink внутри/вне root, dangling link, root parent substitution; вне-root sentinel никогда не оказывается в output |
| Windows | Junction, file/directory symlink, доступный reparse fixture, ADS и reserved names; недоступная ОС/привилегия записывается как qualification gap |
| Тип | Directory, FIFO без writer, socket и допустимый synthetic special-type fixture не принимаются как text; FIFO тест ограничен по времени и реально не висит |
| Размер | Empty, ровно per-file limit, limit+1, много малых файлов, общий лимит/count, ложный advertised size, overflow, growth во время чтения |
| Содержимое | UTF-8 Cyrillic/emoji, граница multibyte, invalid UTF-8, NUL, ZIP bytes с `.txt`, выбранный BOM/control contract; secrets/PII/terms проходят тот же sanitizer |
| Гонки | Управляемая замена parent/leaf между этапами; rename уже открытого файла; append/truncate/same-size write; скопированные bytes и digest согласованы, atomic-source claim отсутствует |
| Независимость | После Prepare изменить/удалить source: ранее подготовленный output неизменен; output не разделяет inode с source там, где это проверяемо |
| Ошибки | Missing/permission denied/rejected/oversize статусы различимы; ошибки не раскрывают raw secrets/paths в public diagnostics; отмена/ошибка не публикуют частичный bundle |
| Передача | Agent/export видит только manifest-listed sanitized artifacts с генерируемыми именами; исходные `AGENTS.md`/`.env` не становятся рабочей конфигурацией |
| Никакой acquisition | URL остаётся ссылкой/unsupported status; HTTP клиент, клиент мессенджера, архиватор и shell не вызываются |

Linux fixtures не квалифицируют Windows/macOS. Тесты реальных messenger
accounts не требуются для этой файловой границы и без контроля пользователя
не запускаются. Итоговые версии dependencies, реальные команды проверок,
runner evidence и ограничения поддержки должны быть записаны root worker
после реализации; эта заметка сама по себе support status не повышает.
