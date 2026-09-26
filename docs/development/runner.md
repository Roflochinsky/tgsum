# Изолированный runner

`tgsum-runner` — отдельный workspace crate вне offline core. Он реализует
границу процесса для будущих adapters. Приложение пока не подключает его к Run;
Codex/Claude, сеть, авторизация и проверка structured result — следующие срезы.
Реальные агенты и аккаунты для этой реализации не запускались.

Основание: [контракт agent adapters](../specs/context-gateway.md#agent-adapters)
и [исследование механизмов ОС](../research/runner-isolation-2026-09-26.md).

## Контракт

1. `discover(name, directories)` ищет обычные исполняемые файлы в явно переданных
   абсолютных каталогах, разрешает installation symlinks и убирает повторы.
   Ничего не запускает, не читает settings/auth; версия неизвестна,
   `authentication = Unknown`. Обнаружение executable не доказывает готовность.
2. Доверенный adapter задаёт `RuntimeSpec`: executable и точный список файлов
   runtime с назначениями. Это конфигурация host-кода, не данные переписки,
   recipe или manifest из архива. Каталоги целиком не принимаются. Library
   discovery через `ldd` над произвольным executable не выполняется.
3. `OfflineRunner::qualify` проверяет профиль, helper и staging runtime,
   затем выполняет version probe в sandbox с **пустым context**. Принимается
   точное совпадение stdout, включая newline, и exit 0. Смена CLI требует
   повторного research его аргументов и новой версии контракта. Qualification
   здесь проверяет процесс/version; она не заменяет проверки tools/auth/API
   конкретного AI-агента. В этом профиле auth не требуется.
4. `PreparedContext::from_bundle` принимает Project/bundle/revision и использует
   `ProjectStore::export_bundle`: unresolved privacy findings, неверная revision,
   повреждённые файлы и отмена блокируют подготовку. Произвольного filesystem
   path в качестве context публичный API не принимает. В staging попадают
   только проверенные публичные Markdown и manifest.
5. `run` ещё раз проверяет revision Project и helper. Передаёт `Invocation`
   через argv/stdin без shell, независимо читает stdout/stderr, ограничивает
   их объём и время, поддерживает `Cancellation`.
6. `RunOutput` содержит сырые байты, exit code и причину завершения. Его Debug
   показывает только размеры, не содержимое. Exit 0 **не** подтверждает schema,
   evidence или качество анализа. Runner не записывает результат в Project
   и не меняет baseline; это делает caller после проверки результата.

Revision проверяется непосредственно перед подготовкой запуска. Выданный bundle
является снимком согласованного scope; изменение Project во время выполнения
не меняет его байты. Будущий UI должен отменять/пересматривать запуск при изменении
настроек. Caller вызывает синхронный runner на выделенном worker и держит этот
поток живым до возврата: Linux parent-death signal относится к создавшему thread.

## Реализованный профиль

`linux-x86_64-bwrap-offline-v1` использует root-owned, несетuid `/usr/bin/bwrap`
**0.12.0** без group/other write. Версия проверяется перед каждым запуском.
Нужны strict user/mount/PID/network/IPC/UTS/cgroup namespaces и
`close_range(..., CLOSE_RANGE_CLOEXEC)` (Linux 5.11+). Недоступность механизма
даёт ошибку; прямого или ослабленного повторного запуска нет. Неизвестный
profile, другая ОС/архитектура или неподходящая версия дают `ExportOnly`.

| Объект | Доступ процесса |
| --- | --- |
| `/context` | Только read-only публичная staging-копия bundle |
| `/runtime/agent` и перечисленные runtime-файлы | Read-only отдельные копии, проверенные до передачи context |
| `/home/agent`, `/tmp` | Пустые отдельные tmpfs для каждого запуска, по 64 MiB |
| Host HOME, Project store, исходные exports, connector secrets | Не монтируются |
| Host `/proc`, `/sys`, `/run`, `/dev`, sockets | Не монтируются |
| Сеть | Отдельный namespace, доступа к host loopback/сокетам нет |
| Окружение | Только HOME, TMPDIR, PATH, LANG и PWD; env_clear применяется и к helper |
| Дополнительные FD | CLOEXEC на всех FD > 2 перед exec helper |

Root filesystem read-only, capabilities сброшены, новые user namespaces
запрещены, controlling terminal отсутствует. Source/guest пути runtime ограничены
по типам и местам назначения; нельзя перекрыть `/context`, HOME или родительский
mount. Установка runtime-файлов после qualification не меняет копию. До запуска
runtime staging доступен лишь host-пользователю; каталоги не передаются агенту
целиком вместе с соседними файлами.

## Лимиты и завершение

- Runtime: максимум 128 дополнительных файлов; 128 MiB на файл и 512 MiB всего.
- По умолчанию: stdin 64 KiB, stdout 4 MiB, stderr 256 KiB, timeout 300 s.
- Caller может выбрать пределы до stdin 1 MiB, stdout 16 MiB, stderr 1 MiB
  и timeout 30 min. Argv: до 128 аргументов и 64 KiB суммарно.
- Pipes неблокирующие. За итерацию читается не больше 64 KiB каждого потока,
  поэтому непрерывный вывод не вытесняет обработку отмены/другого потока.
- На timeout/cancel/overflow завершается bwrap monitor, затем ожидается его
  статус. Связка `--die-with-parent` и PID namespace завершает потомков,
  включая double-fork/setsid. После выхода supervisor закрытие pipes ожидается
  до 2 s; незакрывшийся pipe даёт `CleanupTimedOut`, никогда success.
- Одновременное переполнение обоих потоков может быть обнаружено в разном
  порядке. Оба буфера остаются ограничены; причина указывает обнаруженный предел.

Лимит времени относится к каждому процессу: helper/version probes имеют
собственный timeout 5 s; копирование context/runtime — отдельная подготовка
с поддержкой отмены. Это не deadline всей операции от первого чтения файла.

## Доказательства и запуск проверок

Проверено 2026-09-26 на **Omarchy 4.0.4, Linux 7.2.5-3-omarchy x86_64,
bubblewrap 0.12.0**, Rust fixture вместо AI-агента.

```sh
bash scripts/check.sh
cargo test -p tgsum-runner --all-features --locked --test isolation -- --ignored --nocapture
```

Вторая команда обязана запускаться отдельно: шесть isolation tests помечены
`ignored`, поскольку обычная CI-матрица не устанавливает этот backend и не
гарантирует доступность namespace. Пропуск не считается успешной квалификацией.
На Linux x86_64 с нужным backend suite должен реально выполнить шесть тестов;
на другой архитектуре он не является проверкой поддержки.

Покрыты:

- выбранный context доступен, запись/добавление файлов запрещены; host archive,
  private Project, синтетический credential file отсутствуют;
- отдельные HOME/tmp, параллельные запуски одного runtime; отсутствие inherited
  env, включая LD_PRELOAD и SSH_AUTH_SOCK; намеренно non-CLOEXEC sentinel FD;
- отсутствие host TCP loopback и Unix socket; никакой внешний сервер не нужен;
- точные argv с shell-метасимволами, stdin/stdout с invalid UTF-8, stderr и exit 17;
- замена установленного executable после qualification, другая версия,
  неполный runtime и изменение Project после Review;
- output flood, зависший stdin, timeout, cancel и normal exit с удерживающим
  pipes внуком процесса: handshake доказывает, что он успел вызвать setsid;
- обычные contract/unit tests дополнительно проверяют discovery без запуска,
  отказ неизвестному profile, запрещённые/перекрывающиеся mount paths,
  директории/FIFO и сохранение Rust spawn error после закрытия FD.

## Границы доказанного

- Это generic **offline** runner. Облачные CLI не работают через этот профиль;
  нельзя расширять mounts, копировать их auth-профиль или включать сеть ради
  успешного запуска. Конкретные adapters и их разрешённый доступ к inference
  исследуются отдельно (`tgsum-hzm.7/.8`).
- macOS/Windows и другие архитектуры пока возвращают Export only. Кандидаты
  механизмов описаны в research, но не являются реализованными backend.
- Нет общего ограничения RAM/CPU/числа процессов payload, seccomp allowlist
  или cgroup resource controller. Размер tmpfs/вывода и timeout не заменяют их.
- Есть раннее окно аварии parent до установки PDEATHSIG; абсолютная очистка
  при таком crash не доказана. Зависание ядра также не покрыто таймерами user space.
- Нет защиты от враждебного процесса того же host-пользователя, который меняет
  staging/Project, debugger или kernel exploit. Runtime manifest доверенный.
- TempDir удаляет staging при обычном Drop; авария host может оставить файлы.
  Общая retention/crash-cleanup политика остаётся отдельной задачей Workspace.
- Core manifest пока содержит `destination: export_only`. Подготовленный здесь
  context используется в synthetic проверках; будущий adapter должен связать
  Review с фактическим получателем перед первым пользовательским Run.

Полные agent/OS adversarial suites и проверка настоящих integrations остаются
`tgsum-hzm.12` и `tgsum-t8t.19/.20`. Реальные аккаунты требуют контроля пользователя.
