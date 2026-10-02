# Review: локальная выгрузка, OOXML и приватный GitHub

Дата 2026-10-02; owner TGSUM maintainer; исполнение `tgsum-6gf.8`, Linux/Omarchy
x86_64. Пользователь прямо запросил ручной экспорт, автоматическую подготовку,
сокращение ФИО в Word/XLSX, оригинальные картинки и приватный TGSUM-IMPORT.
Новый acquisition Telegram не вводится. Его прежние даты policy review не
изменяются; текущие [границы manual export](telegram-export-automation.md) остаются.

## Прочитанные первичные источники

- [WordprocessingML structure](https://learn.microsoft.com/en-us/office/open-xml/word/structure-of-a-wordprocessingml-document),
  2026-10-02: DOCX содержит package parts, paragraphs/runs/text; имя может
  пересекать несколько runs. Поэтому замена по отдельному ZIP/XML токену недостаточна.
- [Spreadsheet shared strings](https://learn.microsoft.com/en-us/office/open-xml/spreadsheet/working-with-the-shared-string-table),
  2026-10-02: строки могут храниться отдельно, использовать rich text и inline cells.
- [zip API](https://docs.rs/zip/latest/zip/) и [quick-xml API](https://docs.rs/quick-xml/latest/quick_xml/),
  2026-10-02: ZIP/XML обрабатываются библиотеками локально. Используемые версии
  закреплены Cargo.lock: zip 2.4.2, quick-xml 0.42.0. Это чтение API, не аудит
  безопасности всех зависимостей. Разархивирование в произвольный путь не используется.
- [GitHub file limits](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github),
  2026-10-02: обычный Git блокирует файлы свыше 100 МиБ. Наш бинарный вход
  ограничен 20 МиБ, OOXML expanded — 64 МиБ; LFS не включён.
- [gh repo create](https://cli.github.com/manual/gh_repo_create), 2026-10-02:
  приватная видимость выбирается отдельно. Создание выполнено по явному запросу,
  `gh repo view` подтвердил `isPrivate=true`; это не утверждение о вечной видимости.
- [GitHub history removal](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository),
  2026-10-02: удаление чувствительных данных из истории — отдельная операция,
  обычный новый коммит не гарантирует удаления прежних копий.

## Контракт и инженерное решение

Scope: один включённый Telegram source с точным native ID, фильтром периода и
сохранённой политикой. Читаются предоставленные локальные файлы; токены Telegram,
tdata и аккаунт не используются. Временная стадия JSON удаляется после импорта;
Project хранит выбранный snapshot, текущий публичный пакет заменяется отдельно.
Стабильность байтов не доказывает полноту истории и завершение клиентского экспорта.

Изображения намеренно сохраняют исходные байты и метаданные. DOCX/XLSX получают
детерминированное сокращение распознанного русского ФИО; не OCR и не полное
обезличивание. Макросы/embeddings, подозрительные пути, DTD и превышение бюджетов
останавливают обновление. Старые DOC/XLS и PDF пока не преобразуются.

GitHub — получатель готового пакета, а не новый источник данных. Непустая
сохранённая настройка включает отправку с существующей авторизацией `gh`;
приложение не извлекает и не сохраняет токен. Разрешения аккаунта gh могут быть
шире одного repo; команды ограничены указанным owner/repo. Проверка private
делается дважды, но изменение видимости владельцем после проверки не исключено.
Обработка удаляет предыдущую generated копию из текущего дерева, не Git history.
Нет вызова ChatGPT/облачного inference, обучения или обещания синхронизации ChatGPT.
Решение о содержимом и получателе принадлежит пользователю по ADR-0002.

Ошибки сети/авторизации/push не портят локальный готовый пакет. Повтор идёт из
свежего shallow clone без force push; таймаут 90 с на команду, следующий цикл
через 30 с после завершения. Нет обхода лимитов провайдера. GitHub API считывает
только признак private, передача содержимого идёт через обычный Git HTTPS.

## Evidence и пределы

[Контракт, команды и замеры](../development/local-package-mvp.md),
`core/tests/local_package.rs`, `src-tauri/src/package_github.rs`,
`scripts/desktop-e2e.py`, `scripts/local-package-install-smoke.py`.
Синтетические проверки не объявляются проверкой аккаунта Telegram. Реальная
публикация в созданный приватный repo разрешена отдельно пользователем.

Решение: реализовать локальный pipeline и opt-in private GitHub output на
проверенной ОС. Source registry дополнен ссылками на метод/квалификацию без
повышения статуса Official Client Bridge и без изменения дат Telegram review.
Следующий review этого network output — до 2026-11-01 или при изменении gh/Git,
формата публикации, видимости либо retention. Неизвестные организационные условия
использования содержимого не превращаются в юридическую анкету интерфейса.
