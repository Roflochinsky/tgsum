# tgsum

Локальная утилита: превращает выгрузку Telegram Desktop (`result.json`) в чистые `.md` файлы для вставки в чат с ИИ. Не ходит в сеть, не зовёт LLM.

## Установка

Установка прямо с GitHub. Флаг `--install-links` обязателен — без него npm ставит
git-зависимость битым симлинком на временный clone в кэше (известный баг npm):

```bash
npm install -g github:Roflochinsky/tgsum --install-links
```

Либо без флага — из релизного tarball:

```bash
npm install -g https://github.com/Roflochinsky/tgsum/releases/download/v0.1.0/tgsum-0.1.0.tgz
```

## Запуск

```bash
tgsum
```

Дальше — пошаговый мастер: путь к `result.json` → выбор чатов/топиков → папка с файлами.

Не хотите терминал — используйте двойной-клик: `launchers/tgsum.command` (mac) или `launchers/tgsum.bat` (win).

## Как получить result.json

Telegram Desktop → Settings → Advanced → Export Telegram data → формат **JSON**, медиа можно не включать.
