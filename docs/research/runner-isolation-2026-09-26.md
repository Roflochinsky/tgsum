# Изоляция локального CLI runner

Проверено **2026-09-26**. Область: граница процесса на Linux, macOS и Windows
для будущего runner вне offline core. Это исследование механизмов, а не
подтверждение поддержки Codex, Claude Code или произвольной команды.
Публичные первичные источники прочитаны без авторизации. Локально выполнены
только `bwrap --version` и `bwrap --help`: установлен **bubblewrap 0.12.0**.
Реальные агенты, аккаунты и credential stores не запускались и не читались.

**Вывод для TGSUM:** первым проверять Linux offline-профиль с синтетическим
executable, точным списком runtime-файлов и отдельной копией context bundle.
Наличие executable или bubblewrap само по себе не означает проверенную
изоляцию. Для неизвестного сочетания агента, версии и ОС остаётся `Export only`,
как требует [gateway design](../specs/context-gateway.md#agent-adapters).

## Linux: подтверждённые свойства

Bubblewrap создаёт пустое дерево файлов в новом mount namespace. Его авторы
возлагают выбор политики на вызывающее приложение. В README пример с целым
`/usr` прямо назван неполным. Mount сокета D-Bus может дать выполнение команд
через systemd; read-only mount не запрещает взаимодействие с таким сокетом.
Bubblewrap применяет `PR_SET_NO_NEW_PRIVS`; готового универсального запрета
всех опасных системных вызовов он не предоставляет.
[README v0.12.0](https://github.com/containers/bubblewrap/blob/v0.12.0/README.md).

| Параметр | Подтверждённая семантика |
| --- | --- |
| `--unshare-all` | Включает user/ipc/pid/net/uts/cgroup; user и cgroup используют варианты `-try` |
| `--unshare-user`, `--unshare-cgroup` | Явные обязательные варианты соответствующих namespace |
| `--disable-userns` | Запрещает дальнейшие user namespaces; требует `--unshare-user` |
| `--ro-bind` | Открывает указанный источник для чтения внутри sandbox |
| `--clearenv` | Очищает среду; остаются `PWD` и последующие `--setenv` |
| `--new-session` | Вызывает `setsid`, отделяя controlling terminal |
| `--die-with-parent` | Привязывает жизненный цикл цепочки bwrap к родителю через `PDEATHSIG` |
| `--size N --tmpfs PATH` | Ограничивает размер следующего tmpfs |

Источник таблицы: [руководство v0.12.0](https://github.com/containers/bubblewrap/blob/v0.12.0/bwrap.xml).
Его предупреждение о `TIOCSTI` требует новой terminal session либо подходящего
seccomp-фильтра. Для headless runner дополнительно нужны pipes вместо TTY.

`--proc /proc` создаёт новый procfs при собственном PID namespace; без него
исходник переиспользует host `/proc`. `--dev /dev` создаёт ограниченный набор
устройств `null/zero/full/random/urandom/tty`, ссылки stdio и отдельный devpts.
Он существенно отличается от `--dev-bind /dev /dev`.
[Исходник v0.12.0, SETUP_MOUNT_PROC/SETUP_MOUNT_DEV](https://github.com/containers/bubblewrap/blob/v0.12.0/bubblewrap.c#L1383).

## Предлагаемый Linux-профиль

Это проектные решения TGSUM на основе свойств выше; их ещё требуется проверить
синтетическими negative tests на каждой заявленной конфигурации ОС.

- Использовать явные `--unshare-user --unshare-ipc --unshare-pid --unshare-net
  --unshare-uts --unshare-cgroup`, затем `--disable-userns --new-session
  --die-with-parent --clearenv`. Отсутствие нужного namespace — ошибка профиля.
  Автоматическое ослабление флагами `-try`, `--share-net` или
  `--not-a-security-boundary` не соответствует этому контракту.
- Монтировать отдельные проверенные файлы executable, ELF interpreter и
  зависимостей в фиксированные назначения. Не разрешать caller или содержимому
  bundle добавлять произвольные mount paths/arguments. Интерпретатор shell,
  Node/Python runtime, плагины и subprocess tools требуют собственного review.
- Подготовленный immutable bundle монтировать read-only в `/context`.
  Не монтировать исходный архив, project store, mapping, соседние проекты,
  домашний каталог или auth-профиль агента. Копия должна пройти проверки типов
  файлов, traversal и ссылок; read-only mount не делает host-источник snapshot.
  Эти требования уже заданы [gateway design](../specs/context-gateway.md).
- Создать пустой приватный HOME и ограниченный scratch. Фиксировать cwd внутри
  sandbox. Задание передавать через stdin, результат — stdout; запись результата
  в project store выполняет доверенный родитель после проверки лимитов.
- Начать headless fixture без `/proc` и `/dev`, если runtime работает так.
  При необходимости добавлять `--proc /proc` только с PID namespace и
  минимальный `--dev /dev`; не bind-ить host `/proc`, `/sys`, `/run` или `/tmp`.
  Никаких session bus, SSH agent, desktop portal или display sockets.

**Почему не целый `/usr` или runtime-каталог:** такой mount делает доступным
всё его содержимое, включая интерпретаторы, дополнительные инструменты и
локально установленные файлы. Это расширяет declared read/execute scope;
read-only характеризует только запись. Allowlist отдельных файлов облегчает
проверку фактического scope. При этом filesystem allowlist сам по себе не
доказывает запрет произвольного вычисления, исполнения из scratch или kernel
exploit: такие обещания требуют отдельной модели и дополнительных механизмов.
Это вывод из модели mount namespace, а не заявление upstream о любой
конкретной установке `/usr`.

Динамический загрузчик читает `LD_LIBRARY_PATH`, `LD_PRELOAD`, `LD_AUDIT`
до запуска основной программы. **Вывод:** `Command::env_clear()` нужен уже
при запуске самого bwrap по доверенному абсолютному пути; одного внутреннего
`--clearenv` недостаточно. Runtime allowlist должен учитывать ELF interpreter,
прямые/транзитивные библиотеки и реально нужные динамические загрузки.
[Linux ld.so(8)](https://man7.org/linux/man-pages/man8/ld.so.8.html).

Файловые дескрипторы без `FD_CLOEXEC` сохраняются при `execve`.
**Вывод:** launcher передаёт только согласованные pipes и необходимые служебные
дескрипторы, остальные закрывает или создаёт с close-on-exec. Mount allowlist
не отзывает ранее открытый FD на файл/сокет вне sandbox.
[Linux execve(2)](https://man7.org/linux/man-pages/man2/execve.2.html).

### Rust launcher и унаследованные FD

Bubblewrap 0.12.0 **не закрывает все FD > 2 у payload**: комментарии
`monitor_child` и ветки init явно указывают, что дополнительные FD уже переданы
child; очистка выполняется в monitor/init, а не глобально перед payload exec.
[bubblewrap.c, строки 498 и 3455](https://github.com/containers/bubblewrap/blob/v0.12.0/bubblewrap.c#L498).
Поэтому отсутствие FD из std Rust не доказывает отсутствие дескрипторов,
открытых desktop-библиотеками или полученных самим приложением при старте.

`CommandExt::pre_exec` — unsafe API: после fork разрешены только операции,
безопасные в этом контексте; нельзя выделять память, брать mutex или обращаться
к окружению. Ошибка callback возвращается как ошибка spawn.
[Rust std CommandExt](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#tymethod.pre_exec).

**Предложение для Linux:** отдельная минимальная функция launcher с
обоснованным локальным исключением `unsafe_code` для `pre_exec`, вызывающим
`close_range(3, UINT_MAX, CLOSE_RANGE_CLOEXEC)` и возвращающим OS error при
неудаче. Флаг CLOEXEC доступен с Linux 5.11, wrapper glibc — с 2.34; syscall
ошибка не должна включать более слабый fallback. Установка флагов вместо
немедленного закрытия сохраняет служебные FD до exec. Нельзя применять это
в многопоточном родителе как глобальную мутацию его таблицы FD.
[Linux close_range(2)](https://man7.org/linux/man-pages/man2/close_range.2.html).

Это проектный вариант, требующий проверки на реальном Rust launcher, включая
ошибку exec и специально унаследованный non-CLOEXEC sentinel FD.
`close_fds` 0.3.2 предоставляет helpers и описывает требования async-signal-safety,
но интеграция с `Command` всё равно использует unsafe `pre_exec`.
[Документация close_fds](https://docs.rs/close_fds/0.3.2/close_fds/).
Оставлять offline core без unsafe можно независимо от реализации отдельного
runner crate. Изменять глобальные lint-настройки workspace для этого не нужно.

## Завершение процессов и pipe hang

В `bubblewrap.c` monitor возвращает статус после смерти начального процесса
PID 2. Init PID 1 вызывает `handle_die_with_parent`, устанавливающий
`PR_SET_PDEATHSIG(SIGKILL)`. Без этого флага init продолжает ждать оставшихся
процессов. Extra FD передаются запускаемой программе; закрытие их в monitor
не закрывает копии у payload.
[Исходник v0.12.0, monitor_child/do_init](https://github.com/containers/bubblewrap/blob/v0.12.0/bubblewrap.c#L475).

Linux при смерти init PID namespace посылает SIGKILL всем его процессам.
Потомок не может перейти в родительский PID namespace.
[Linux pid_namespaces(7)](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html).
**Вывод:** после установки parent-death цепочки выход/kill bwrap monitor
приводит к смерти PID 1 и остальных потомков, включая double-fork и отдельную
session. Один `kill` только основного CLI или его process group этого не
доказывает. На cancel/timeout следует завершить и дождаться bwrap supervisor,
затем ограниченно дождаться закрытия потоков.

Есть существенная граница: `PDEATHSIG` относится к **создавшему thread**.
Если родитель умер до установки настройки, сигнал задним числом не приходит;
настройка также не наследуется автоматически через `fork`.
[Linux PR_SET_PDEATHSIG](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html).
**Вывод:** создающий bwrap supervisor thread должен жить до завершения run.
Нельзя обещать безусловную очистку при аварии приложения в самый ранний момент
setup только на основании `--die-with-parent`. Upstream test проверяет смерть
родителя после захвата sandbox lock; это более узкое условие.
[Тест v0.12.0](https://github.com/containers/bubblewrap/blob/v0.12.0/tests/test-run.sh#L211).

Для более строгого supervisor можно отдельно исследовать cgroup v2:
`cgroup.kill` завершает всё поддерево с учётом конкурентных fork/migration.
Нужны реальные права delegation и независимый владелец жизненного цикла;
`--unshare-cgroup` сам по себе не создаёт resource limits или такого владельца.
[Linux cgroup v2](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html).

Предлагаемые acceptance tests на fake executable:

1. Чтение sentinel вне mounts, изменение context и доступ к host socket
   отвергаются; выбранный context читается; внешний TCP и host loopback закрыты.
2. Parent environment sentinel отсутствует; fixture видит только разрешённые
   FD и приватный HOME. `LD_PRELOAD` не наследуется запускаемым bwrap.
3. Success/error/cancel/timeout/overflow завершают процесс с double-fork/setsid
   потомком, удерживающим stdout/stderr. Потоки читаются одновременно с
   ограничением байтов и deadline; EOF не является единственным условием выхода.
4. Проверяются одновременные runs, зависший stdin, flood обоих потоков,
   invalid UTF-8, утраченный runtime-файл и недоступный namespace.
5. Ошибка setup никогда не вызывает прямой запуск executable вне sandbox.

Лимиты stdout/stderr и timeout ограничивают runner, но не доказывают ограничение
CPU/RAM/числа процессов payload. Resource DoS и crash/setup race остаются
отдельными открытыми вопросами до соответствующей реализации и тестов.

## macOS: отдельный профиль, пока исследование

Apple документирует встраивание подписанного CLI helper в sandboxed app,
включая tools внешней системы сборки. Helper получает entitlements
`com.apple.security.app-sandbox` и `com.apple.security.inherit`. Этот способ
применим также к Developer ID distribution. Документ рекомендует рассмотреть
XPC service для выполнения кода отдельным процессом.
[Apple, Embedding a command-line tool](https://developer.apple.com/documentation/xcode/embedding-a-helper-tool-in-a-sandboxed-app)
([прочитанный JSON Apple](https://developer.apple.com/tutorials/data/documentation/xcode/embedding-a-helper-tool-in-a-sandboxed-app.json)).

Архивный Entitlement Key Reference разрешает настраивать sandbox отдельно для
app и XPC targets. Унаследованный child получает статические sandbox rights;
для файлов, открытых после старта, документ предлагает передачу данных или
bookmark. Это старый источник, его детали требуют проверки на выбранном SDK.
[Apple Entitlement Key Reference](https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/EnablingAppSandbox.html).

Apple DTS в ответе ноября 2021 года указывает, что SBPL язык `sandbox-exec`
не документирован для third-party использования, и не рекомендует строить
на нём продукт. Это ответ сотрудника Apple, не обещание даты удаления API.
[Apple Developer Forums, thread 661939](https://developer.apple.com/forums/thread/661939).

**Вывод для TGSUM:** проверить узкий подписанный helper/XPC с передачей bundle
через контролируемый канал. Нельзя считать этот документ доказательством, что
произвольный уже установленный агент с собственными подписью, tools и auth
автоматически помещается в такой профиль. Проверка signing/entitlements,
чтения чужих файлов, IPC, сети и полного cleanup необходима на настоящей macOS.
Таких испытаний в этом исследовании нет; `sandbox-exec` не квалифицирован как
поддержанный production backend. Дата его удаления здесь не установлена.

## Windows: AppContainer плюс управление процессами

Microsoft описывает запуск AppContainer/LPAC через `STARTUPINFOEX`,
`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` и `CreateProcess`.
Доступ определяется пересечением пользовательских прав и AppContainer
SID/capabilities. Без network capability сеть недоступна. Обычный AppContainer
имеет доступ к некоторым системным ресурсам; LPAC требует дополнительных
явных capabilities. Это не модель пустого файлового root.
[Microsoft, Launch an AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer).

Job Object объединяет процессы для завершения и resource limits.
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` завершает процессы при закрытии последнего
handle. Потомки `CreateProcess` обычно входят в job; breakaway flags меняют это,
а `Win32_Process.Create` указан как исключение. Security limits на современных
Windows задаются отдельным процессам.
[Microsoft, Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).
Microsoft показывает создание процесса suspended, добавление в job и только
затем resume, устраняя окно до назначения job.
[Microsoft, The Old New Thing, 2013-12-09](https://devblogs.microsoft.com/oldnewthing/20131209-00/?p=2433/).

**Вывод для TGSUM:** кандидат — AppContainer/LPAC без сети, отдельная staging
копия с минимальными ACL, контролируемые handles и Job Object без breakaway.
Нельзя выдавать один Job Object за filesystem/network sandbox. Отдельно
проверить наследование handles, закрытие последнего job handle, nested jobs,
IPC/broker обходы, reparse points, зависимости и подпись конкретного CLI.
Не менять ACL исходных архивов/профилей агента ради совместимости.

Не путать этот API с новой упаковкой **Win32 app isolation**: прочитанная
страница называет её preview и требует Windows 11 24H2 build 26100+ и
Visual Studio 17.10.2+ для packaging. Эти требования не являются минимальной
версией самого AppContainer API.
[Microsoft, Win32 app isolation overview](https://learn.microsoft.com/en-us/windows/win32/secauthz/app-isolation-overview).

## Что ещё нужно для support decision

Для каждой ОС зафиксировать backend/version, kernel/OS, точный runtime
allowlist, файл/IPC/network scope, resource limits и результаты перечисленных
negative tests. При смене runtime или прав повторять qualification.
Linux namespace availability и fake-process tests должны быть отдельным
evidence реализации; проверка `--help` их не заменяет.

Сетевой анализ и agent auth — следующий самостоятельный профиль: offline
sandbox не обеспечивает облачному CLI inference. Требуется согласованный
destination, способ доступа к inference без копирования чужих credentials,
контроль tools/MCP/hooks и проверка конкретной версии CLI. Этот документ
не разрешает читать профиль агента, открывать общую сеть или расширять mounts
автоматически при ошибке. До доказательства такого профиля доступен
`Export only`. Решение соответствует
[review policy](../connectors/review-policy.md) и
[ответственности TGSUM](../adr/0002-user-controlled-content.md).
