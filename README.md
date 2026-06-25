# tgsum

Локальная утилита: превращает выгрузку Telegram Desktop (`result.json`) в чистые `.md` файлы для вставки в чат с ИИ. Не ходит в сеть, не зовёт LLM.

## Установка

```bash
npm install -g @roflochinsky/tgsum
```

## Запуск

```bash
tgsum
```

Дальше — пошаговый мастер: путь к `result.json` → выбор чатов/топиков → папка с файлами.

Не хотите терминал — используйте двойной-клик: `launchers/tgsum.command` (mac) или `launchers/tgsum.bat` (win).

## Как получить result.json

Telegram Desktop → Settings → Advanced → Export Telegram data → формат **JSON**, медиа можно не включать.
